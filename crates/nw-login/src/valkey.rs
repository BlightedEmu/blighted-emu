// SPDX-License-Identifier: AGPL-3.0-or-later
//! Valkey stores: game sessions, rate limits and Discord login attempts. Nothing here is the only
//! copy of durable data; losing it means "log in again" (M2 spec section 7).

use nw_protocol::launcher_api::LauncherSession;
use redis::aio::ConnectionManager;
use uuid::Uuid;

use crate::queue::{
    ENTRY_TICKET_SECONDS, EntryRecord, QueueAction, QueueSettings, QueueStatus, QueueStore,
};
use crate::store::{DiscordAttempt, DiscordStateStore, GameSessionStore, RateLimiter, StoreError};

/// Key prefix of game session keys.
const GAME_SESSION_PREFIX: &str = "nw:gs:";
/// Key prefix of rate-limit counters.
const RATE_LIMIT_PREFIX: &str = "nw:rl:";
/// Key prefix of Discord PKCE verifiers.
const DISCORD_VERIFIER_PREFIX: &str = "nw:dv:";
/// Key prefix of finished Discord logins.
const DISCORD_RESULT_PREFIX: &str = "nw:dr:";
/// Queue order of a world (`<prefix><world>`).
const QUEUE_ORDER_PREFIX: &str = "nw:q:";
/// Last refresh of waiting entries of a world.
const QUEUE_REFRESH_PREFIX: &str = "nw:qr:";
/// Live entry records of a world.
const QUEUE_ADMITTED_PREFIX: &str = "nw:qd:";
/// Entry hash (`account`, `character`) by ticket.
const QUEUE_ENTRY_PREFIX: &str = "nw:qe:";
/// Entry record by ticket; `rep-gateway` reads and deletes it in M3.
pub const ENTRY_RECORD_PREFIX: &str = "nw:et:";
/// Entry of an account (`<prefix><world>:<account>`).
const QUEUE_ACCOUNT_PREFIX: &str = "nw:qa:";
const QUEUE_SCRIPT: &str = include_str!("queue_script.lua");
const JOIN_ACTION: &str = "join";
const REFRESH_ACTION: &str = "refresh";
const STATUS_ADMITTED: &str = "admitted";
const STATUS_WAITING: &str = "waiting";

/// A connection to Valkey, shared by every store.
#[derive(Clone)]
pub struct Valkey {
    connection: ConnectionManager,
}

impl Valkey {
    /// Connects to `url`, for example `redis://127.0.0.1:6379`.
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let client = redis::Client::open(url).map_err(StoreError::unavailable)?;
        let connection = client
            .get_connection_manager()
            .await
            .map_err(StoreError::unavailable)?;
        Ok(Self { connection })
    }

    async fn query<T: redis::FromRedisValue>(&self, command: &redis::Cmd) -> Result<T, StoreError> {
        let mut connection = self.connection.clone();
        command
            .query_async(&mut connection)
            .await
            .map_err(StoreError::unavailable)
    }
}

impl GameSessionStore for Valkey {
    async fn create(
        &self,
        key: &str,
        account_id: Uuid,
        claim_seconds: u64,
    ) -> Result<(), StoreError> {
        self.query::<()>(
            redis::cmd("SET")
                .arg(format!("{GAME_SESSION_PREFIX}{key}"))
                .arg(account_id.to_string())
                .arg("EX")
                .arg(claim_seconds),
        )
        .await
    }

    async fn resolve(&self, key: &str, ttl_seconds: u64) -> Result<Option<Uuid>, StoreError> {
        let value: Option<String> = self
            .query(
                redis::cmd("GETEX")
                    .arg(format!("{GAME_SESSION_PREFIX}{key}"))
                    .arg("EX")
                    .arg(ttl_seconds),
            )
            .await?;
        Ok(value.and_then(|text| Uuid::parse_str(&text).ok()))
    }
}

impl RateLimiter for Valkey {
    async fn count(&self, key: &str) -> Result<u64, StoreError> {
        let value: Option<u64> = self
            .query(redis::cmd("GET").arg(format!("{RATE_LIMIT_PREFIX}{key}")))
            .await?;
        Ok(value.unwrap_or(0))
    }

    async fn increment(&self, key: &str, window_seconds: u64) -> Result<u64, StoreError> {
        let key = format!("{RATE_LIMIT_PREFIX}{key}");
        let mut connection = self.connection.clone();
        let (count, _): (u64, i64) = redis::pipe()
            .atomic()
            .cmd("INCR")
            .arg(&key)
            .cmd("EXPIRE")
            .arg(&key)
            .arg(window_seconds)
            .arg("NX")
            .query_async(&mut connection)
            .await
            .map_err(StoreError::unavailable)?;
        Ok(count)
    }
}

impl DiscordStateStore for Valkey {
    async fn begin(&self, state: &str, verifier: &str, ttl_seconds: u64) -> Result<(), StoreError> {
        self.query::<()>(
            redis::cmd("SET")
                .arg(format!("{DISCORD_VERIFIER_PREFIX}{state}"))
                .arg(verifier)
                .arg("EX")
                .arg(ttl_seconds),
        )
        .await
    }

    async fn verifier(&self, state: &str) -> Result<Option<String>, StoreError> {
        self.query(redis::cmd("GET").arg(format!("{DISCORD_VERIFIER_PREFIX}{state}")))
            .await
    }

    async fn complete(
        &self,
        state: &str,
        session: &LauncherSession,
        ttl_seconds: u64,
    ) -> Result<(), StoreError> {
        let value = serde_json::to_string(session).map_err(StoreError::unavailable)?;
        self.query::<()>(
            redis::cmd("SET")
                .arg(format!("{DISCORD_RESULT_PREFIX}{state}"))
                .arg(value)
                .arg("EX")
                .arg(ttl_seconds),
        )
        .await
    }

    async fn poll(&self, state: &str) -> Result<DiscordAttempt, StoreError> {
        let result: Option<String> = self
            .query(redis::cmd("GETDEL").arg(format!("{DISCORD_RESULT_PREFIX}{state}")))
            .await?;
        if let Some(text) = result {
            self.query::<()>(redis::cmd("DEL").arg(format!("{DISCORD_VERIFIER_PREFIX}{state}")))
                .await?;
            let session = serde_json::from_str(&text).map_err(StoreError::unavailable)?;
            return Ok(DiscordAttempt::Ready(session));
        }
        let started: bool = self
            .query(redis::cmd("EXISTS").arg(format!("{DISCORD_VERIFIER_PREFIX}{state}")))
            .await?;
        Ok(if started {
            DiscordAttempt::Pending
        } else {
            DiscordAttempt::Unknown
        })
    }
}

impl QueueStore for Valkey {
    async fn update(
        &self,
        settings: &QueueSettings,
        account_id: Uuid,
        action: QueueAction,
        now_ms: u64,
    ) -> Result<Option<QueueStatus>, StoreError> {
        let world = settings.world_id;
        let (name, character, new_ticket, ticket) = match &action {
            QueueAction::Join {
                character_id,
                new_ticket_id,
            } => (
                JOIN_ACTION,
                character_id.to_string(),
                new_ticket_id.as_str(),
                "",
            ),
            QueueAction::Refresh { ticket_id } => {
                (REFRESH_ACTION, String::new(), "", ticket_id.as_str())
            }
        };
        let mut connection = self.connection.clone();
        let reply: Vec<String> = redis::Script::new(QUEUE_SCRIPT)
            .key(format!("{QUEUE_ORDER_PREFIX}{world}"))
            .key(format!("{QUEUE_REFRESH_PREFIX}{world}"))
            .key(format!("{QUEUE_ADMITTED_PREFIX}{world}"))
            .arg(now_ms)
            .arg(now_ms.saturating_sub(settings.stale_after_ms))
            .arg(settings.capacity)
            .arg(ENTRY_TICKET_SECONDS)
            .arg(world.to_string())
            .arg(QUEUE_ENTRY_PREFIX)
            .arg(ENTRY_RECORD_PREFIX)
            .arg(format!("{QUEUE_ACCOUNT_PREFIX}{world}:"))
            .arg(name)
            .arg(account_id.to_string())
            .arg(character)
            .arg(new_ticket)
            .arg(ticket)
            .invoke_async(&mut connection)
            .await
            .map_err(StoreError::unavailable)?;
        match reply.as_slice() {
            [status, _, record] if status == STATUS_ADMITTED => {
                let record: EntryRecord =
                    serde_json::from_str(record).map_err(StoreError::unavailable)?;
                Ok(Some(QueueStatus::Admitted(record)))
            }
            [status, ticket_id, position] if status == STATUS_WAITING => {
                let position = position.parse().map_err(StoreError::unavailable)?;
                Ok(Some(QueueStatus::Waiting {
                    ticket_id: ticket_id.clone(),
                    position,
                }))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(all(test, feature = "infra"))]
pub(crate) mod tests {
    use super::*;
    use crate::memory::contract;

    /// Valkey URL for infrastructure tests (`NW_TEST_VALKEY_URL` or the Docker Compose default).
    pub(crate) fn test_valkey_url() -> String {
        std::env::var("NW_TEST_VALKEY_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_owned())
    }

    async fn valkey() -> Valkey {
        Valkey::connect(&test_valkey_url())
            .await
            .expect("connect to Valkey")
    }

    #[tokio::test]
    async fn valkey_game_sessions_follow_the_contract() {
        contract::game_sessions(valkey().await).await;
    }

    #[tokio::test]
    async fn valkey_rate_limiter_follows_the_contract() {
        contract::rate_limiter(valkey().await).await;
    }

    #[tokio::test]
    async fn valkey_discord_states_follow_the_contract() {
        contract::discord_states(valkey().await).await;
    }

    #[tokio::test]
    async fn valkey_queue_follows_the_contract() {
        contract::queue(valkey().await).await;
    }

    /// Parallel joins on one world.
    const PARALLEL_JOINS: usize = 200;
    /// Capacity of the parallel test world.
    const PARALLEL_CAPACITY: u32 = 50;

    #[tokio::test(flavor = "multi_thread")]
    async fn parallel_joins_never_exceed_capacity() {
        let store = valkey().await;
        let settings = QueueSettings {
            world_id: Uuid::new_v4(),
            capacity: PARALLEL_CAPACITY,
            stale_after_ms: contract::STALE_AFTER_MS,
        };
        let accounts: Vec<Uuid> = (0..PARALLEL_JOINS).map(|_| Uuid::new_v4()).collect();
        let joins = accounts.clone().into_iter().map(|account| {
            let store = store.clone();
            tokio::spawn(async move {
                let action = QueueAction::Join {
                    character_id: Uuid::new_v4(),
                    new_ticket_id: contract::unique("t"),
                };
                store
                    .update(&settings, account, action, contract::START_MS)
                    .await
                    .expect("join")
                    .expect("status")
            })
        });
        let mut admitted = 0;
        let mut waiting = Vec::new();
        for (account, join) in accounts.into_iter().zip(joins.collect::<Vec<_>>()) {
            match join.await.expect("task") {
                QueueStatus::Admitted(_) => admitted += 1,
                QueueStatus::Waiting { ticket_id, .. } => waiting.push((account, ticket_id)),
            }
        }
        assert_eq!(admitted, PARALLEL_CAPACITY as usize);
        let mut positions = Vec::new();
        for (account, ticket_id) in waiting {
            let action = QueueAction::Refresh { ticket_id };
            match store
                .update(&settings, account, action, contract::START_MS)
                .await
                .expect("refresh")
            {
                Some(QueueStatus::Waiting { position, .. }) => positions.push(position),
                other => panic!("expected waiting, got {other:?}"),
            }
        }
        positions.sort_unstable();
        let expected: Vec<u64> =
            (1..=(PARALLEL_JOINS - PARALLEL_CAPACITY as usize) as u64).collect();
        assert_eq!(positions, expected);
    }
}
