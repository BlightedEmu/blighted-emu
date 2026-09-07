// SPDX-License-Identifier: AGPL-3.0-or-later
//! The live panel (admin tools spec section 5): the state of the running servers, read from the
//! bus, the metrics pages and `/proc`, and sent to the page.

pub mod bots;
pub mod bus;
pub mod cpu;
pub mod scrape;
pub mod state;
pub mod stream;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::config::LiveConfig;
use crate::live::bots::{BotSettings, Bots};
use crate::live::cpu::{CpuSampler, PROC_DIR};
use crate::live::state::LiveState;

/// The live state, shared by the readers and the routes.
pub type SharedState = Arc<Mutex<LiveState>>;

/// A new, empty shared state.
pub fn shared() -> SharedState {
    Arc::default()
}

/// The state; a reader that panicked does not lock the others out.
pub fn lock(state: &SharedState) -> MutexGuard<'_, LiveState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the live routes need.
#[derive(Debug, Clone)]
pub struct Live {
    state: SharedState,
    state_interval: Duration,
    bots: Bots,
}

impl Live {
    /// Routes that send `state` every `state_interval` and run `bots`.
    pub fn new(state: SharedState, state_interval: Duration, bots: Bots) -> Self {
        Self {
            state,
            state_interval,
            bots,
        }
    }
}

/// Starts the bus reader and the scraper of `config` on the current runtime; batches start
/// from the routes.
pub fn start(config: &LiveConfig) -> Live {
    let state = shared();
    tokio::spawn(bus::run(
        config.nats_url.clone(),
        config.world_id,
        state.clone(),
    ));
    tokio::spawn(scrape::run(
        config.metrics_urls.clone(),
        Duration::from_millis(config.scrape_interval_ms),
        CpuSampler::new(PathBuf::from(PROC_DIR), config.processes.clone()),
        state.clone(),
    ));
    let bots = Bots::new(BotSettings::of(config), state.clone());
    Live::new(state, Duration::from_millis(config.state_interval_ms), bots)
}
