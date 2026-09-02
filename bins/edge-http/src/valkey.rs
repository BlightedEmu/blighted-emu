// SPDX-License-Identifier: AGPL-3.0-or-later
//! Valkey copy of the login queue.
//!
//! The state key is updated with a compare-and-set so two `edge-http` copies
//! cannot admit the same seat. A new entry ticket is a separate key with a
//! 60 second lifetime.

use std::sync::Arc;

use nw_protocol::LoginQueueResponse;
use redis::aio::ConnectionManager;
use uuid::Uuid;

use crate::queue::{
    ENTRY_TICKET_TTL_SECONDS, LoginQueue, PreparedJoin, QueueError, QueueSettings, Transition,
    VALKEY_UPDATE_ATTEMPTS, WorldQueue, apply_join, apply_refresh,
};

/// Key prefix for the production queue.
pub(crate) const PRODUCTION_KEY_PREFIX: &str = "nw:login:queue:";

const CAS_SCRIPT: &str = r#"
local current = redis.call('GET', KEYS[1])
if current == false then
  current = ''
end
if current ~= ARGV[1] then
  return 0
end
redis.call('SET', KEYS[1], ARGV[2])
local ttl = ARGV[3]
local index = 4
local writes = tonumber(ARGV[index])
index = index + 1
for _ = 1, writes do
  redis.call('SET', ARGV[index], ARGV[index + 1], 'EX', ttl)
  index = index + 2
end
local deletes = tonumber(ARGV[index])
index = index + 1
for _ = 1, deletes do
  redis.call('DEL', ARGV[index])
  index = index + 1
end
return 1
"#;

/// Queue stored in Valkey.
#[derive(Clone)]
pub(crate) struct ValkeyQueue {
    connection: ConnectionManager,
    prefix: String,
    settings: Arc<QueueSettings>,
}

impl ValkeyQueue {
    /// Connects to Valkey.
    pub(crate) async fn connect(
        url: &str,
        prefix: &str,
        settings: Arc<QueueSettings>,
    ) -> Result<Self, QueueError> {
        let client = redis::Client::open(url).map_err(store)?;
        let connection = client.get_connection_manager().await.map_err(store)?;
        Ok(Self {
            connection,
            prefix: prefix.to_owned(),
            settings,
        })
    }

    async fn update(
        &self,
        change: impl Fn(&mut WorldQueue) -> Result<Transition, QueueError>,
    ) -> Result<LoginQueueResponse, QueueError> {
        let key = state_key(&self.prefix, &self.settings.world_id);
        for _ in 0..VALKEY_UPDATE_ATTEMPTS {
            let mut connection = self.connection.clone();
            let raw: Option<String> = redis::cmd("GET")
                .arg(&key)
                .query_async(&mut connection)
                .await
                .map_err(store)?;
            let current = match raw.as_deref() {
                None => WorldQueue::default(),
                Some(text) => serde_json::from_str(text).map_err(store)?,
            };
            let mut next = current;
            let transition = change(&mut next)?;
            let new_state = serde_json::to_string(&next).map_err(store)?;
            let expected = raw.unwrap_or_default();
            if self
                .compare_and_set(&key, &expected, &new_state, &transition)
                .await?
            {
                return transition.outcome;
            }
        }
        Err(QueueError::Unavailable(Box::new(std::io::Error::other(
            "login queue update lost the race",
        ))))
    }

    async fn compare_and_set(
        &self,
        key: &str,
        expected: &str,
        new_state: &str,
        transition: &Transition,
    ) -> Result<bool, QueueError> {
        let mut command = redis::cmd("EVAL");
        command
            .arg(CAS_SCRIPT)
            .arg(1)
            .arg(key)
            .arg(expected)
            .arg(new_state)
            .arg(ENTRY_TICKET_TTL_SECONDS)
            .arg(transition.new_entries.len());
        for entry in &transition.new_entries {
            command
                .arg(entry_key(
                    &self.prefix,
                    &self.settings.world_id,
                    &entry.ticket_id,
                ))
                .arg(&entry.payload);
        }
        command.arg(transition.expired_ticket_ids.len());
        for ticket_id in &transition.expired_ticket_ids {
            command.arg(entry_key(&self.prefix, &self.settings.world_id, ticket_id));
        }
        let mut connection = self.connection.clone();
        let updated: i64 = command.query_async(&mut connection).await.map_err(store)?;
        Ok(updated == 1)
    }
}

impl LoginQueue for ValkeyQueue {
    async fn join(
        &self,
        prepared: PreparedJoin,
        now_unix: i64,
    ) -> Result<LoginQueueResponse, QueueError> {
        let settings = Arc::clone(&self.settings);
        let ticket_id = Uuid::new_v4().to_string();
        self.update(move |state| {
            apply_join(state, prepared.clone(), &ticket_id, now_unix, &settings)
        })
        .await
    }

    async fn refresh(
        &self,
        ticket_id: &str,
        now_unix: i64,
    ) -> Result<LoginQueueResponse, QueueError> {
        let settings = Arc::clone(&self.settings);
        let ticket_id = ticket_id.to_owned();
        self.update(move |state| apply_refresh(state, &ticket_id, now_unix, &settings))
            .await
    }
}

pub(crate) fn state_key(prefix: &str, world_id: &str) -> String {
    format!("{prefix}{}:state", hex::encode(world_id.as_bytes()))
}

pub(crate) fn entry_key(prefix: &str, world_id: &str, ticket_id: &str) -> String {
    format!(
        "{prefix}{}:entry:{ticket_id}",
        hex::encode(world_id.as_bytes())
    )
}

fn store(error: impl std::error::Error + Send + Sync + 'static) -> QueueError {
    QueueError::Unavailable(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::ENTRY_TICKET_TTL_SECONDS;
    use nw_protocol::LoginQueueRequest;

    const VALKEY_URL: &str = "redis://127.0.0.1:6379";
    const WORLD: &str = "world-1";
    const NOW: i64 = 1_700_000_000;

    fn settings() -> Arc<QueueSettings> {
        Arc::new(QueueSettings {
            world_id: WORLD.to_owned(),
            queue_name: WORLD.to_owned(),
            capacity: 1,
            refresh_interval_seconds: 5,
            estimated_seconds_per_position: 5,
            rep_address: "127.0.0.1:33435".to_owned(),
            signature_key: b"change-me-signature-key".to_vec(),
        })
    }

    #[tokio::test]
    async fn valkey_stores_an_entry_ticket_with_a_ttl() {
        let prefix = format!("nw:login:test:{}:", Uuid::new_v4().simple());
        let queue = ValkeyQueue::connect(VALKEY_URL, &prefix, settings())
            .await
            .expect("valkey");
        let prepared = crate::queue::prepare_join(
            LoginQueueRequest {
                world_id: Some(WORLD.to_owned()),
                ..LoginQueueRequest::default()
            },
            &queue.settings,
        )
        .expect("prepare");
        let reply = queue.join(prepared, NOW).await.expect("join");
        let token = reply.token.expect("token");
        let ticket_id = token.ticket_id.expect("ticket");
        assert!(
            token
                .signature
                .as_deref()
                .is_some_and(|value| !value.is_empty())
        );
        let key = entry_key(&prefix, WORLD, &ticket_id);
        let mut connection = queue.connection.clone();
        let ttl: i64 = redis::cmd("TTL")
            .arg(&key)
            .query_async(&mut connection)
            .await
            .expect("ttl");
        assert!(ttl > 0 && ttl <= ENTRY_TICKET_TTL_SECONDS);
        let _: () = redis::cmd("DEL")
            .arg(state_key(&prefix, WORLD))
            .arg(&key)
            .query_async(&mut connection)
            .await
            .expect("delete");
    }
}
