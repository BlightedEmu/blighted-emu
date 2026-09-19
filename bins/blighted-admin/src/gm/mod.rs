// SPDX-License-Identifier: AGPL-3.0-or-later
//! GM commands (admin studio spec section 8): sent to the sim that owns the target, answered
//! by it, and written to the audit log.

pub mod api;

use std::sync::Arc;
use std::time::Duration;

use nw_bus::{AdminCommand, AdminReply, Bus, BusId, subject};
use tokio::sync::OnceCell;
use uuid::Uuid;

/// Longest wait for a sim's answer.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a command got no answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// The sim did not answer in [`COMMAND_TIMEOUT`].
    #[error("the sim did not answer in time")]
    Timeout,
    /// The bus failed.
    #[error("bus: {0}")]
    Bus(String),
}

/// Sends GM commands to the sims of one world.
#[derive(Debug, Clone)]
pub struct Commands {
    world: Uuid,
    sender: Sender,
}

#[derive(Debug, Clone)]
enum Sender {
    /// The bus, connected on the first command.
    Nats {
        url: Arc<str>,
        bus: Arc<OnceCell<Bus>>,
    },
    /// Answers given by a test, in order; `None` is a time-out.
    #[cfg(test)]
    Fake(Arc<std::sync::Mutex<fake::FakeSims>>),
}

impl Commands {
    /// Commands over the bus at `nats_url` to the sims of `world`.
    pub fn over_bus(nats_url: &str, world: Uuid) -> Self {
        Self {
            world,
            sender: Sender::Nats {
                url: nats_url.into(),
                bus: Arc::default(),
            },
        }
    }

    /// Sends `command` to `sim` and waits for its answer.
    pub async fn send(&self, sim: &BusId, command: &AdminCommand) -> Result<AdminReply, SendError> {
        match &self.sender {
            Sender::Nats { url, bus } => {
                let bus = bus
                    .get_or_try_init(|| Bus::connect(url))
                    .await
                    .map_err(|error| SendError::Bus(error.to_string()))?;
                bus.request(
                    subject::sim_admin(self.world, sim),
                    command,
                    COMMAND_TIMEOUT,
                )
                .await
                .map_err(|error| {
                    if error.is_timeout() {
                        SendError::Timeout
                    } else {
                        SendError::Bus(error.to_string())
                    }
                })
            }
            #[cfg(test)]
            Sender::Fake(sims) => fake::answer(sims, sim, command),
        }
    }
}

#[cfg(test)]
pub mod fake {
    //! Fake sims for the route tests.

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex, PoisonError};

    use super::*;

    /// The commands sent, and the answers to give.
    #[derive(Debug, Default)]
    pub struct FakeSims {
        /// Every command sent, with its sim.
        pub sent: Vec<(BusId, AdminCommand)>,
        /// Answers in order; `None` is a time-out.
        pub answers: VecDeque<Option<AdminReply>>,
    }

    /// Commands that go to `sims`.
    pub fn commands(sims: &Arc<Mutex<FakeSims>>) -> Commands {
        Commands {
            world: Uuid::nil(),
            sender: Sender::Fake(sims.clone()),
        }
    }

    pub(super) fn answer(
        sims: &Arc<Mutex<FakeSims>>,
        sim: &BusId,
        command: &AdminCommand,
    ) -> Result<AdminReply, SendError> {
        let mut sims = sims.lock().unwrap_or_else(PoisonError::into_inner);
        sims.sent.push((sim.clone(), command.clone()));
        sims.answers.pop_front().flatten().ok_or(SendError::Timeout)
    }
}
