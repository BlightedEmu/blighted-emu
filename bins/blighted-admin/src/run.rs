// SPDX-License-Identifier: AGPL-3.0-or-later
//! `serve`: reads the configuration, the world file and the content bundle names, starts the
//! live readers when `[live]` is set, and serves the admin pages until Ctrl-C.

use std::path::Path;

use nw_common::logging::{LoggingError, init_logging};
use tokio::sync::watch;

use crate::captures::Captures;
use crate::config::{AdminConfigError, load_admin_config};
use crate::live;
use crate::server::{AppState, router};
use crate::world::{WorldLayer, WorldLayerError, map_dir};

/// Environment variable that names the New World install folder (for `typeindex.json`).
pub const GAME_DIR_ENV: &str = "NW_GAME_DIR";

/// `blighted-admin serve` failed.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The configuration cannot be used.
    #[error(transparent)]
    Config(#[from] AdminConfigError),
    /// Logging cannot start.
    #[error(transparent)]
    Logging(#[from] LoggingError),
    /// The world layer cannot be loaded.
    #[error(transparent)]
    World(#[from] WorldLayerError),
    /// The runtime, the listener or the server failed.
    #[error("server failed")]
    Io(#[source] std::io::Error),
}

/// Serves the admin pages of `config_path` until Ctrl-C.
///
/// # Errors
///
/// Returns [`RunError`] when the configuration or the data cannot be read, or the server fails.
pub fn serve(config_path: &Path) -> Result<(), RunError> {
    let config = load_admin_config(config_path)?;
    init_logging(&config.log_filter)?;
    let world = WorldLayer::load(&config)?;
    let game_dir = std::env::var_os(GAME_DIR_ENV).map(std::path::PathBuf::from);
    let captures = Captures::load(&config.captures, game_dir.as_deref());
    let map = map_dir(&config);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(RunError::Io)?
        .block_on(async {
            let (close, closing) = watch::channel(false);
            let live = config.live.as_ref().map(|live| live::start(live, closing));
            let state = AppState::new(world, map, config.listen, captures, live);
            let listener = tokio::net::TcpListener::bind(config.listen)
                .await
                .map_err(RunError::Io)?;
            tracing::info!(listen = %config.listen, "serving");
            axum::serve(listener, router(state))
                .with_graceful_shutdown(async move {
                    // A failed signal handler means the server runs until killed.
                    let _ = tokio::signal::ctrl_c().await;
                    // The live streams never end by themselves; the server waits for them.
                    let _ = close.send(true);
                })
                .await
                .map_err(RunError::Io)
        })
}
