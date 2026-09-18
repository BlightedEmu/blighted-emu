// SPDX-License-Identifier: AGPL-3.0-or-later
//! The studio (admin studio spec sections 5.2 to 5.4): the team's design files, their checks,
//! and Apply, which merges them and restarts the local world.

pub mod api;
pub mod apply;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nw_design::DesignPaths;

use crate::config::StudioConfig;
use crate::live::SharedState;
use crate::studio::apply::ApplyStatus;

/// What the studio routes and the Apply job share.
#[derive(Debug, Clone)]
pub struct Studio {
    pub(crate) paths: Arc<DesignPaths>,
    client_build: Arc<str>,
    control_fifo: Arc<PathBuf>,
    restart_timeout: Duration,
    live: Option<SharedState>,
    apply: Arc<Mutex<ApplyStatus>>,
}

impl Studio {
    /// A studio that merges into `out_dir` (the folder the servers read) for `client_build`, and
    /// watches the sims of `live` after a restart.
    pub fn new(
        config: &StudioConfig,
        out_dir: PathBuf,
        client_build: &str,
        live: Option<SharedState>,
    ) -> Self {
        Self {
            paths: Arc::new(DesignPaths {
                design_dir: config.design_dir.clone(),
                retail_dir: config.retail_dir.clone(),
                out_dir,
            }),
            client_build: client_build.into(),
            control_fifo: Arc::new(config.control_fifo.clone()),
            restart_timeout: Duration::from_secs(config.restart_timeout_s),
            live,
            apply: Arc::default(),
        }
    }
}
