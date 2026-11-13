// SPDX-License-Identifier: AGPL-3.0-or-later
//! `serve`: reads the configuration and the world file, connects to the world database, Valkey
//! and NATS, and runs the loop until Ctrl-C.

use std::path::Path;
use std::time::Duration;

use metrics_exporter_prometheus::PrometheusBuilder;
use nw_bus::{Bus, BusError};
use nw_common::config::ConfigError;
use nw_common::logging::{LoggingError, init_logging};
use nw_content::client_build::load_client_build;
use nw_content::container::ContainerError;
use nw_content::war_forts::WarForts;
use nw_db::{DbError, GLOBAL_MIGRATOR, WORLD_MIGRATOR, check_schema};
use nw_login::store::StoreError;
use nw_login::valkey::Valkey;
use nw_sim_core::{CellGrid, CellGridError};
use nw_world::WorldFile;
use nw_world::world_file::world_file_path;

use crate::config::{DirectorConfig, DirectorConfigError, InstancesConfig, load_director_config};
use crate::instances::{InstanceSettings, LaunchSettings};
use crate::matchmaking::{MatchSettings, QueueRules};
use crate::server::{DirectorSettings, ServeError, serve as serve_loop};

/// Failure while starting or running `world-director`.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The configuration file is missing or not valid.
    #[error(transparent)]
    Config(#[from] DirectorConfigError),
    /// The client build file cannot be read.
    #[error("client build file: {0}")]
    ClientBuild(#[source] ConfigError),
    /// The world file cannot be read.
    #[error("world file: {0}")]
    WorldFile(#[from] ContainerError),
    /// The cell size does not fit the level.
    #[error("cell grid: {0}")]
    Grid(#[from] CellGridError),
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
    /// Valkey cannot be reached.
    #[error("Valkey connection failed")]
    Valkey(#[source] StoreError),
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

/// Longest wait for a connection of the global database (the admin ranks).
const RANK_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(250);

/// Runs `world-director` until Ctrl-C.
///
/// # Errors
///
/// Returns [`RunError`] when the configuration, the world file, the start position, a secret,
/// the database, Valkey, NATS, or the loop fails.
pub fn serve(config_path: &Path) -> Result<(), RunError> {
    let config = load_director_config(config_path)?;
    let password = std::env::var(&config.world_database_password_env)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RunError::MissingSecret(config.world_database_password_env.clone()))?;
    init_logging(&config.log_filter)?;
    let build = load_client_build(&config.client_build_config).map_err(RunError::ClientBuild)?;
    let path = world_file_path(&config.content_dir, &build, &config.level);
    let world_file = WorldFile::load(&path, &build)?;
    let settings = DirectorSettings {
        world_id: config.world_id,
        level: config.level.clone(),
        starts: config.starts(&world_file)?,
        grid: CellGrid::of_level(&world_file.level, config.cell_size_m)?,
        admit_timeout: Duration::from_millis(config.admit_timeout_ms),
        heartbeat_interval: Duration::from_millis(config.heartbeat_interval_ms),
        heartbeat_misses: config.heartbeat_misses,
        instances: config.instances.as_ref().map(|instances| {
            let mut settings = instance_settings(instances, war_forts(&config, &build));
            settings.invasion_territories = invasion_territories(&config, &build);
            settings
        }),
    };
    // The heights are needed only for the start position.
    drop(world_file);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(RunError::Runtime)?
        .block_on(serve_async(config, password, settings))
}

/// The forts of the territory wars of the content of `config` for client build `build`; none
/// (and so no war instance) when the table is missing.
fn war_forts(config: &DirectorConfig, build: &str) -> WarForts {
    match WarForts::load(&config.content_dir, build) {
        Ok(forts) => {
            tracing::info!(forts = forts.len(), "war forts");
            forts
        }
        Err(error) => {
            tracing::warn!(%error, "no war forts table: no territory war gets an instance");
            WarForts::default()
        }
    }
}

/// The territories whose fort has an invasion in the content of `config` for client build
/// `build`; none (and so no darkness and no invasion) when the table is missing.
fn invasion_territories(config: &DirectorConfig, build: &str) -> std::collections::BTreeSet<u16> {
    match nw_content::invasion_forts::InvasionForts::load(&config.content_dir, build) {
        Ok(forts) => {
            tracing::info!(forts = forts.len(), "invasion forts");
            forts.territories().collect()
        }
        Err(error) => {
            tracing::warn!(%error, "no invasion forts table: no territory gets an invasion");
            std::collections::BTreeSet::new()
        }
    }
}

/// The instance rules of the `[instances]` section, with the forts of the wars `war_forts`.
fn instance_settings(config: &InstancesConfig, war_forts: WarForts) -> InstanceSettings {
    InstanceSettings {
        empty_timeout: Duration::from_millis(config.empty_timeout_ms),
        open_timeout: Duration::from_millis(config.open_timeout_ms),
        group_timeout: Duration::from_millis(config.group_timeout_ms),
        launch: config.launch.as_ref().map(|launch| LaunchSettings {
            program: launch.program.clone(),
            args: launch.args.clone(),
            spare_per_level: launch.spare_per_level,
            max_sims: launch.max_sims,
            id_prefix: launch.id_prefix.clone(),
            forget_after: Duration::from_millis(launch.forget_after_ms),
        }),
        queues: QueueRules {
            arena: MatchSettings::arena(
                config.arena.match_size,
                Duration::from_millis(config.arena.accept_timeout_ms),
            ),
            outpost_rush: MatchSettings::outpost_rush(
                config.outpost_rush.match_size,
                Duration::from_millis(config.outpost_rush.accept_timeout_ms),
            ),
            capture_the_flag: MatchSettings::capture_the_flag(
                config.capture_the_flag.match_size,
                Duration::from_millis(config.capture_the_flag.accept_timeout_ms),
            ),
            catacombs: MatchSettings::catacombs(
                Duration::from_millis(config.catacombs.accept_timeout_ms),
                Duration::from_millis(config.catacombs.fill_wait_ms),
            ),
        },
        war_forts,
        invasion_territories: std::collections::BTreeSet::new(),
    }
}

async fn serve_async(
    config: DirectorConfig,
    password: String,
    settings: DirectorSettings,
) -> Result<(), RunError> {
    let db = nw_db::connect(&config.world_database, &password)
        .await
        .map_err(RunError::Database)?;
    check_schema(&db, &WORLD_MIGRATOR).await?;
    let ranks = match &config.global_database {
        Some(global) => {
            let password = std::env::var(&global.password_env)
                .ok()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| RunError::MissingSecret(global.password_env.clone()))?;
            // Lazy: the admin ranks are optional, so a global database that is down at start
            // gives no GM rights instead of stopping the world.
            let pool = nw_db::connect_lazy(&global.database, &password, RANK_ACQUIRE_TIMEOUT)
                .map_err(RunError::Database)?;
            if let Err(error) = check_schema(&pool, &GLOBAL_MIGRATOR).await {
                tracing::warn!(%error, "global database not ready; no GM rights until it is");
            }
            Some(pool)
        }
        None => None,
    };
    let store = Valkey::connect(&config.valkey_url)
        .await
        .map_err(RunError::Valkey)?;
    let bus = Bus::connect(&config.nats_url).await?;
    PrometheusBuilder::new()
        .with_http_listener(config.metrics_listen)
        .install()
        .map_err(|error| RunError::Metrics(error.to_string()))?;
    // The trading post's orders are the world's; one director expires them for every sim.
    tokio::spawn(crate::trading_post::sweep_expired_orders(
        db.clone(),
        crate::trading_post::EXPIRY_SWEEP_INTERVAL,
    ));
    tracing::info!(world = %config.world_id, level = %config.level, "serving");
    let shutdown = async {
        // A failed signal handler means the director runs until killed.
        let _ = nw_common::shutdown::stop_signal().await;
    };
    Ok(serve_loop(settings, bus, db, ranks, store, shutdown).await?)
}
