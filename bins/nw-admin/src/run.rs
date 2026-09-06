// SPDX-License-Identifier: AGPL-3.0-or-later
//! `serve`: reads the configuration, the world file and the content bundle names, and serves
//! the admin pages until Ctrl-C.

use std::path::Path;

use nw_common::logging::{LoggingError, init_logging};

use crate::captures::Captures;
use crate::config::{AdminConfigError, load_admin_config};
use crate::server::{AppState, router};
use crate::world::{WorldLayer, WorldLayerError, map_dir};

/// Environment variable that names the New World install folder (for `typeindex.json`).
pub const GAME_DIR_ENV: &str = "NW_GAME_DIR";

/// `nw-admin serve` failed.
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
    let state = AppState::new(world, map_dir(&config), config.listen, captures);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(RunError::Io)?
        .block_on(async {
            let listener = tokio::net::TcpListener::bind(config.listen)
                .await
                .map_err(RunError::Io)?;
            tracing::info!(listen = %config.listen, "serving");
            axum::serve(listener, router(state))
                .with_graceful_shutdown(async {
                    // A failed signal handler means the server runs until killed.
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await
                .map_err(RunError::Io)
        })
}
