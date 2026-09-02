// SPDX-License-Identifier: AGPL-3.0-or-later
//! `serve` and `migrate`.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::routing::get;
use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use metrics_exporter_prometheus::PrometheusBuilder;
use nw_common::logging::{LoggingError, init_logging};
use nw_db::{DbError, GLOBAL_MIGRATOR, WORLD_MIGRATOR, check_schema};
use nw_login::config::{EdgeConfig, EdgeConfigError, load_edge_config};
use nw_login::discord::{DiscordError, DiscordHttp};
use nw_login::install_crypto_provider;
use nw_login::launcher::{LauncherState, launcher_router};
use nw_login::password::{PasswordError, PasswordHasher};
use nw_login::pg::PgAccounts;
use nw_login::secrets::{SecretError, Secrets, load_database_passwords, load_secrets};
use nw_login::store::StoreError;
use nw_login::valkey::Valkey;

use crate::database::{EnsureDatabaseError, connect_all, ensure_database};
use crate::layers::with_limits;

/// Seconds to finish open requests after Ctrl-C.
const SHUTDOWN_GRACE_SECONDS: u64 = 10;
/// Path of the Prometheus scrape route on the metrics listener.
const METRICS_PATH: &str = "/metrics";

/// Failure while starting or running `edge-http`.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The configuration file is missing or not valid.
    #[error(transparent)]
    Config(#[from] EdgeConfigError),
    /// A secret is missing or not valid.
    #[error(transparent)]
    Secret(#[from] SecretError),
    /// Logging could not start.
    #[error(transparent)]
    Logging(#[from] LoggingError),
    /// The async runtime could not start.
    #[error("runtime failed")]
    Runtime(#[source] std::io::Error),
    /// PostgreSQL could not be reached.
    #[error("database connection failed")]
    Database(#[source] sqlx::Error),
    /// A database schema check or migration failed.
    #[error(transparent)]
    Schema(#[from] DbError),
    /// Valkey could not be reached.
    #[error("Valkey connection failed")]
    Valkey(#[source] StoreError),
    /// The password worker pool could not start.
    #[error(transparent)]
    Password(#[from] PasswordError),
    /// The Discord client could not start.
    #[error(transparent)]
    Discord(#[from] DiscordError),
    /// The certificate files cannot be read.
    #[error("TLS certificate files failed")]
    Tls(#[source] std::io::Error),
    /// A listener failed.
    #[error("listener failed")]
    Listen(#[source] std::io::Error),
    /// A database could not be created.
    #[error(transparent)]
    EnsureDatabase(#[from] EnsureDatabaseError),
    /// The Prometheus recorder could not be installed.
    #[error("metrics recorder failed: {0}")]
    Metrics(String),
}

fn env_lookup(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn runtime() -> Result<tokio::runtime::Runtime, RunError> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(RunError::Runtime)
}

/// Creates missing databases, then applies the migrations of the global and every world
/// database.
pub fn migrate(config_path: &Path) -> Result<(), RunError> {
    let config = load_edge_config(config_path)?;
    let passwords = load_database_passwords(&config, env_lookup)?;
    init_logging(&config.log_filter)?;
    runtime()?.block_on(async {
        ensure_database(&config.global_database, &passwords.global).await?;
        for (world, password) in config.worlds.iter().zip(&passwords.worlds) {
            ensure_database(&world.database, password).await?;
        }
        let (global, worlds) = connect_all(&config, &passwords)
            .await
            .map_err(RunError::Database)?;
        GLOBAL_MIGRATOR.run(&global).await.map_err(DbError::from)?;
        for world in &worlds {
            WORLD_MIGRATOR.run(world).await.map_err(DbError::from)?;
        }
        tracing::info!(worlds = worlds.len(), "migrations applied");
        Ok(())
    })
}

/// Serves HTTPS until Ctrl-C.
pub fn serve(config_path: &Path) -> Result<(), RunError> {
    let config = load_edge_config(config_path)?;
    let secrets = load_secrets(&config, env_lookup)?;
    init_logging(&config.log_filter)?;
    install_crypto_provider();
    runtime()?.block_on(serve_async(config, secrets))
}

async fn serve_async(config: EdgeConfig, secrets: Secrets) -> Result<(), RunError> {
    let (global, worlds) = connect_all(&config, &secrets.database_passwords)
        .await
        .map_err(RunError::Database)?;
    check_schema(&global, &GLOBAL_MIGRATOR).await?;
    for world in &worlds {
        check_schema(world, &WORLD_MIGRATOR).await?;
    }
    let valkey = Valkey::connect(&config.valkey.url)
        .await
        .map_err(RunError::Valkey)?;
    let discord = match &secrets.discord_client_secret {
        Some(secret) => Some(DiscordHttp::new(
            config.discord.client_id.clone(),
            secret.clone(),
        )?),
        None => None,
    };
    let state = LauncherState {
        config: Arc::new(config.clone()),
        accounts: PgAccounts::new(global),
        game_sessions: valkey.clone(),
        limits: valkey.clone(),
        discord_states: valkey,
        discord,
        hasher: PasswordHasher::new(config.auth.argon2_parallel)?,
    };
    let app = with_limits(
        launcher_router(state),
        Duration::from_secs(config.request_timeout_seconds),
        config.max_connections,
    );
    spawn_metrics(config.metrics_listen).await?;
    let tls = RustlsConfig::from_pem_file(
        &config.tls.certificate_pem_path,
        &config.tls.private_key_pem_path,
    )
    .await
    .map_err(RunError::Tls)?;
    let handle = Handle::new();
    let shutdown = handle.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown.graceful_shutdown(Some(Duration::from_secs(SHUTDOWN_GRACE_SECONDS)));
        }
    });
    tracing::info!(address = %config.https_listen, "serving HTTPS");
    axum_server::bind_rustls(config.https_listen, tls)
        .handle(handle)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .map_err(RunError::Listen)
}

/// Installs the Prometheus recorder and serves it on the internal address (review finding I1).
async fn spawn_metrics(address: SocketAddr) -> Result<(), RunError> {
    let handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|error| RunError::Metrics(error.to_string()))?;
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(RunError::Listen)?;
    let app = Router::new().route(
        METRICS_PATH,
        get(move || std::future::ready(handle.render())),
    );
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::error!(%error, "metrics listener stopped");
        }
    });
    tracing::info!(%address, "serving metrics");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn committed_config() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/edge.toml")
    }

    #[test]
    fn committed_example_config_is_valid() {
        let config = load_edge_config(&committed_config()).expect("config/edge.toml");
        assert!(config.metrics_listen.ip().is_loopback());
        assert!(!config.worlds.is_empty());
    }

    #[test]
    fn serve_refuses_to_start_without_secrets() {
        let config = load_edge_config(&committed_config()).expect("config");
        assert!(matches!(
            load_secrets(&config, |_| None),
            Err(SecretError::Missing(_))
        ));
    }
}
