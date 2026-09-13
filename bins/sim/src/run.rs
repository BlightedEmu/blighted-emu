// SPDX-License-Identifier: AGPL-3.0-or-later
//! `serve`: reads the configuration and the world file, connects to the world database and NATS,
//! and runs the loop until Ctrl-C.

use std::path::Path;
use std::sync::Arc;

use metrics_exporter_prometheus::{Matcher, PrometheusBuilder};
use nw_bus::{Bus, BusError};
use nw_common::config::ConfigError;
use nw_common::logging::{LoggingError, init_logging};
use nw_content::ContentBundle;
use nw_content::bundle::bundle_path;
use nw_content::client_build::load_client_build;
use nw_content::combat::{CombatData, CombatDataError};
use nw_content::container::ContainerError;
use nw_db::{DbError, WORLD_MIGRATOR, check_schema};
use nw_sim_core::level_bounds;
use nw_world::WorldFile;
use nw_world::world_file::world_file_path;

use crate::config::{SimConfig, SimConfigError, load_sim_config};
use crate::mobs::spawns_of_level;
use crate::player_content::PlayerContent;
use crate::server::{
    LATENCY_BUCKETS_SECONDS, LoopSettings, METRIC_HANDOFF_SECONDS, METRIC_MOB_TICK_SECONDS,
    METRIC_SNAPSHOT_SECONDS, METRIC_TICK_SECONDS, ServeError, serve as serve_loop,
};

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
    /// The content bundle cannot be read.
    #[error("content bundle: {0}")]
    Bundle(#[source] ContainerError),
    /// A combat row of the config is not in the bundle.
    #[error(transparent)]
    Combat(#[from] CombatDataError),
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
/// Returns [`RunError`] when the configuration, the world file, a secret, the database, NATS, or
/// the loop fails.
pub fn serve(config_path: &Path) -> Result<(), RunError> {
    let config = load_sim_config(config_path)?;
    let password = std::env::var(&config.world_database_password_env)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RunError::MissingSecret(config.world_database_password_env.clone()))?;
    init_logging(&config.log_filter)?;
    let build = load_client_build(&config.client_build_config).map_err(RunError::ClientBuild)?;
    let path = world_file_path(&config.content_dir, &build, &config.level);
    let world_file = Arc::new(WorldFile::load(&path, &build)?);
    let bundle = ContentBundle::load(&bundle_path(&config.content_dir, &build), &build)
        .map_err(RunError::Bundle)?;
    // Only the combat rows and the player content stay; the rest of the bundle is dropped here.
    let combat = Arc::new(CombatData::from_tables(
        &bundle.tables,
        &config.combat.rows(),
    )?);
    let content = Arc::new(PlayerContent::from_tables(&bundle.tables));
    drop(bundle);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(RunError::Runtime)?
        .block_on(serve_async(config, password, world_file, combat, content))
}

async fn serve_async(
    config: SimConfig,
    password: String,
    ground: Arc<WorldFile>,
    combat: Arc<CombatData>,
    content: Arc<PlayerContent>,
) -> Result<(), RunError> {
    let db = nw_db::connect(&config.world_database, &password)
        .await
        .map_err(RunError::Database)?;
    check_schema(&db, &WORLD_MIGRATOR).await?;
    let bus = Bus::connect(&config.nats_url).await?;
    PrometheusBuilder::new()
        .with_http_listener(config.metrics_listen)
        .set_buckets_for_metric(
            Matcher::Full(METRIC_HANDOFF_SECONDS.to_owned()),
            &LATENCY_BUCKETS_SECONDS,
        )
        .and_then(|builder| {
            builder.set_buckets_for_metric(
                Matcher::Full(METRIC_TICK_SECONDS.to_owned()),
                &LATENCY_BUCKETS_SECONDS,
            )
        })
        .and_then(|builder| {
            builder.set_buckets_for_metric(
                Matcher::Full(METRIC_MOB_TICK_SECONDS.to_owned()),
                &LATENCY_BUCKETS_SECONDS,
            )
        })
        .and_then(|builder| {
            builder.set_buckets_for_metric(
                Matcher::Full(METRIC_SNAPSHOT_SECONDS.to_owned()),
                &LATENCY_BUCKETS_SECONDS,
            )
        })
        .and_then(PrometheusBuilder::install)
        .map_err(|error| RunError::Metrics(error.to_string()))?;
    tracing::info!(sim = %config.sim_id, world = %config.world_id, level = %config.level, "serving");
    let settings = LoopSettings {
        sim_id: config.sim_id.clone(),
        world_id: config.world_id,
        level: config.level.clone(),
        world: config.world_settings(level_bounds(&ground.level))?,
        handoff_timeout: std::time::Duration::from_millis(config.handoff_timeout_ms),
        forward_ttl_ms: config.forward_ttl_ms,
        save_interval: std::time::Duration::from_millis(config.save_interval_ms),
        heartbeat_interval: std::time::Duration::from_millis(config.heartbeat_interval_ms),
        heartbeat_misses: config.heartbeat_misses,
        combat: config.combat_settings(combat, spawns_of_level(&config.level, &ground.vitals)),
        content,
        snapshot_interval: (config.admin.snapshot_interval_ms != 0)
            .then(|| std::time::Duration::from_millis(config.admin.snapshot_interval_ms)),
    };
    let shutdown = async {
        // A failed signal handler means sim runs until killed.
        let _ = tokio::signal::ctrl_c().await;
    };
    Ok(serve_loop(settings, ground, bus, db, shutdown).await?)
}
