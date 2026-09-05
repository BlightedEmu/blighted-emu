// SPDX-License-Identifier: AGPL-3.0-or-later
//! `serve`: reads the configuration and the world file, checks the start position, connects to
//! the world database and NATS, and runs the loop until Ctrl-C.

use std::path::Path;

use metrics_exporter_prometheus::PrometheusBuilder;
use nw_bus::{Bus, BusError};
use nw_common::config::ConfigError;
use nw_common::logging::{LoggingError, init_logging};
use nw_content::client_build::load_client_build;
use nw_content::container::ContainerError;
use nw_db::{DbError, WORLD_MIGRATOR, check_schema};
use nw_world::WorldFile;
use nw_world::world_file::world_file_path;

use crate::config::{SimConfig, SimConfigError, load_sim_config};
use crate::server::{LoopSettings, ServeError, serve as serve_loop};
use crate::world::WorldSettings;

/// Failure while starting or running `sim`.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The configuration file is missing or not valid.
    #[error(transparent)]
    Config(#[from] SimConfigError),
    /// The client build file cannot be read.
    #[error("client build file: {0}")]
    ClientBuild(#[source] ConfigError),
    /// The world file cannot be read.
    #[error("world file: {0}")]
    WorldFile(#[from] ContainerError),
    /// The database password variable is missing or empty.
    #[error("environment variable {0} is not set")]
    MissingSecret(String),
    /// Logging could not start.
    #[error(transparent)]
    Logging(#[from] LoggingError),
    /// The async runtime could not start.
    #[error("runtime failed")]
    Runtime(#[source] std::io::Error),
    /// The world database cannot be reached.
    #[error("database connection failed")]
    Database(#[source] sqlx::Error),
    /// The world database schema is not current.
    #[error(transparent)]
    Schema(#[from] DbError),
    /// NATS cannot be reached.
    #[error(transparent)]
    Bus(#[from] BusError),
    /// The loop failed.
    #[error(transparent)]
    Serve(#[from] ServeError),
    /// The Prometheus listener could not start.
    #[error("metrics listener failed: {0}")]
    Metrics(String),
}

/// Runs `sim` until Ctrl-C.
///
/// # Errors
///
/// Returns [`RunError`] when the configuration, the world file, the start position, a secret,
/// the database, NATS, or the loop fails.
pub fn serve(config_path: &Path) -> Result<(), RunError> {
    let config = load_sim_config(config_path)?;
    let password = std::env::var(&config.world_database_password_env)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RunError::MissingSecret(config.world_database_password_env.clone()))?;
    init_logging(&config.log_filter)?;
    let build = load_client_build(&config.client_build_config).map_err(RunError::ClientBuild)?;
    let path = world_file_path(&config.content_dir, &build, &config.level);
    config.check_start(&WorldFile::load(&path, &build)?)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(RunError::Runtime)?
        .block_on(serve_async(config, password))
}

async fn serve_async(config: SimConfig, password: String) -> Result<(), RunError> {
    let db = nw_db::connect(&config.world_database, &password)
        .await
        .map_err(RunError::Database)?;
    check_schema(&db, &WORLD_MIGRATOR).await?;
    let bus = Bus::connect(&config.nats_url).await?;
    PrometheusBuilder::new()
        .with_http_listener(config.metrics_listen)
        .install()
        .map_err(|error| RunError::Metrics(error.to_string()))?;
    tracing::info!(sim = %config.sim_id, world = %config.world_id, level = %config.level, "serving");
    let settings = LoopSettings {
        sim_id: config.sim_id.clone(),
        world_id: config.world_id,
        level: config.level.clone(),
        start: config.start_position.position(),
        world: WorldSettings {
            interest_radius_m: config.interest_radius_m,
        },
        save_interval: std::time::Duration::from_millis(config.save_interval_ms),
        heartbeat_interval: std::time::Duration::from_millis(config.heartbeat_interval_ms),
        heartbeat_misses: config.heartbeat_misses,
    };
    let shutdown = async {
        // A failed signal handler means sim runs until killed.
        let _ = tokio::signal::ctrl_c().await;
    };
    Ok(serve_loop(settings, bus, db, shutdown).await?)
}
