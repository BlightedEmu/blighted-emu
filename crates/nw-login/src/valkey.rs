// SPDX-License-Identifier: AGPL-3.0-or-later
//! Valkey stores: game sessions, rate limits and Discord login attempts. Nothing here is the only
//! copy of durable data; losing it means "log in again" (M2 spec section 7).

use nw_protocol::launcher_api::LauncherSession;
use redis::aio::ConnectionManager;
use uuid::Uuid;

use crate::store::{DiscordAttempt, DiscordStateStore, GameSessionStore, RateLimiter, StoreError};

/// Key prefix of game session keys.
const GAME_SESSION_PREFIX: &str = "nw:gs:";
/// Key prefix of rate-limit counters.
const RATE_LIMIT_PREFIX: &str = "nw:rl:";
/// Key prefix of Discord PKCE verifiers.
const DISCORD_VERIFIER_PREFIX: &str = "nw:dv:";
/// Key prefix of finished Discord logins.
const DISCORD_RESULT_PREFIX: &str = "nw:dr:";

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
}
