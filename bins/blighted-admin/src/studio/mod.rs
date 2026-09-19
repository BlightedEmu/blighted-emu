// SPDX-License-Identifier: AGPL-3.0-or-later
//! The studio (admin studio spec sections 5.2 to 5.4): the team's design files, their checks,
//! and Apply, which merges them and restarts the local world.

pub mod api;
pub mod apply;
pub mod catalog;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nw_content::ContentBundle;
use nw_content::bundle::bundle_path;
use nw_design::DesignPaths;
use tokio::sync::{Mutex as AsyncMutex, OnceCell, Semaphore};

use crate::config::StudioConfig;
use crate::live::SharedState;
use crate::studio::apply::ApplyStatus;
use crate::studio::catalog::Catalog;
use crate::todo::github::GitHub;

/// Full design runs at a time.
const ONE_RUN: usize = 1;

/// What the studio routes and the Apply job share.
#[derive(Debug, Clone)]
pub struct Studio {
    pub(crate) paths: Arc<DesignPaths>,
    client_build: Arc<str>,
    control_fifo: Arc<PathBuf>,
    restart_timeout: Duration,
    live: Option<SharedState>,
    apply: Arc<Mutex<ApplyStatus>>,
    /// One full design run at a time: a Check or a whole Apply. Each loads the bundle and
    /// world files, so runs side by side would hold several copies in memory.
    work: Arc<Semaphore>,
    /// The client records, read from the retail bundle at the first editor request.
    catalog: Arc<OnceCell<Arc<Catalog>>>,
    /// One change of the to-do list at a time.
    pub(crate) todo_lock: Arc<AsyncMutex<()>>,
    /// The issue source of the to-do import.
    pub(crate) github: GitHub,
}

impl Studio {
    /// A studio that merges into `out_dir` (the folder the servers read) for `client_build`, and
    /// watches the sims of `live` after a restart.
    ///
    /// # Errors
    ///
    /// Returns the reason when the HTTP client of the GitHub import cannot be made.
    pub fn new(
        config: &StudioConfig,
        out_dir: PathBuf,
        client_build: &str,
        live: Option<SharedState>,
    ) -> Result<Self, String> {
        let token = config
            .github_token_env
            .as_ref()
            .and_then(|name| std::env::var(name).ok());
        let github = GitHub::new(&config.github_api_url, token)?;
        Ok(Self {
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
            work: Arc::new(Semaphore::new(ONE_RUN)),
            catalog: Arc::default(),
            todo_lock: Arc::default(),
            github,
        })
    }

    /// The client records; the first call reads the retail bundle.
    ///
    /// # Errors
    ///
    /// Returns the reason when the retail bundle cannot be read.
    pub async fn catalog(&self) -> Result<Arc<Catalog>, String> {
        let path = bundle_path(&self.paths.retail_dir, &self.client_build);
        let build = Arc::clone(&self.client_build);
        self.catalog
            .get_or_try_init(|| async move {
                tokio::task::spawn_blocking(move || {
                    let bundle = ContentBundle::load(&path, &build)
                        .map_err(|error| format!("{}: {error}", path.display()))?;
                    Ok(Arc::new(Catalog::of_bundle(&bundle)))
                })
                .await
                .map_err(|error| error.to_string())?
            })
            .await
            .cloned()
    }
}
