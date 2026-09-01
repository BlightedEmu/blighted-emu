// SPDX-License-Identifier: AGPL-3.0-or-later
//! In-memory queue. Tests use it. Production uses Valkey so every `edge-http` copy
//! shares one queue.

use std::sync::{Arc, Mutex};

use nw_protocol::LoginQueueResponse;
use uuid::Uuid;

use crate::queue::{
    LoginQueue, PreparedJoin, QueueError, QueueSettings, WorldQueue, apply_join, apply_refresh,
};

/// Queue kept in this process.
#[derive(Clone)]
pub(crate) struct MemoryQueue {
    settings: Arc<QueueSettings>,
    state: Arc<Mutex<WorldQueue>>,
}

impl MemoryQueue {
    /// Empty queue.
    pub(crate) fn new(settings: Arc<QueueSettings>) -> Self {
        Self {
            settings,
            state: Arc::new(Mutex::new(WorldQueue::default())),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, WorldQueue>, QueueError> {
        self.state.lock().map_err(|error| {
            QueueError::Unavailable(Box::new(std::io::Error::other(error.to_string())))
        })
    }
}

impl LoginQueue for MemoryQueue {
    async fn join(
        &self,
        prepared: PreparedJoin,
        now_unix: i64,
    ) -> Result<LoginQueueResponse, QueueError> {
        let mut state = self.lock()?;
        let ticket_id = Uuid::new_v4().to_string();
        let transition = apply_join(&mut state, prepared, &ticket_id, now_unix, &self.settings)?;
        transition.outcome
    }

    async fn refresh(
        &self,
        ticket_id: &str,
        now_unix: i64,
    ) -> Result<LoginQueueResponse, QueueError> {
        let mut state = self.lock()?;
        let transition = apply_refresh(&mut state, ticket_id, now_unix, &self.settings)?;
        transition.outcome
    }
}
