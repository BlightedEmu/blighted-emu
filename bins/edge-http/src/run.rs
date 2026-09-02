// SPDX-License-Identifier: AGPL-3.0-or-later
//! Process startup for the login HTTPS listener.

use std::error::Error;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use metrics_exporter_prometheus::PrometheusBuilder;
use nw_common::logging::{LoggingError, init_logging};
use nw_db::WORLD_MIGRATOR;
use nw_edge_config::server::{EdgeConfigError, load_edge_config};
use sqlx::postgres::PgPoolOptions;

use crate::characters::PgCharacters;
use crate::config::{LoginConfig, LoginConfigError, load_login_config};
use crate::http::{AppState, router};
use crate::queue::{QueueError, QueueSettings};
use crate::valkey::{PRODUCTION_KEY_PREFIX, ValkeyQueue};

/// Seconds to wait for open HTTPS connections after Ctrl-C.
const SHUTDOWN_GRACE_SECONDS: u64 = 10;

/// Failure while starting or serving the login listener.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The edge file could not be loaded.
    #[error("edge config failed")]
    Edge(#[source] Box<dyn Error + Send + Sync>),
    /// The login file could not be loaded.
    #[error("login config failed")]
    Login(#[source] Box<dyn Error + Send + Sync>),
    /// The log filter is not valid, or a logger is already installed.
    #[error("logging failed")]
    Log(#[source] Box<dyn Error + Send + Sync>),
    /// PostgreSQL could not be reached or migrated.
    #[error("database failed")]
    Database(#[source] Box<dyn Error + Send + Sync>),
    /// Valkey could not be reached.
    #[error("login queue failed")]
    Queue(#[source] Box<dyn Error + Send + Sync>),
    /// The TLS certificate files could not be read.
    #[error("tls certificate files failed")]
    Tls(#[source] std::io::Error),
    /// The HTTPS listener stopped with an error.
    #[error("https listener failed")]
    Serve(#[source] std::io::Error),
    /// The Prometheus recorder could not be installed.
    #[error("metrics recorder failed: {0}")]
    Metrics(String),
    /// The async runtime could not start.
    #[error("runtime failed")]
    Runtime(#[source] std::io::Error),
}

/// Loads both config files, connects to PostgreSQL and Valkey, and serves HTTPS.
pub fn run(edge_path: &Path, login_path: &Path) -> Result<(), RunError> {
    let edge = load_edge_config(edge_path).map_err(edge_error)?;
    let login = load_login_config(login_path).map_err(login_error)?;
    init_logging(&login.log_filter).map_err(log_error)?;
    let settings = login
        .queue_settings(edge.rep_address)
        .map_err(login_error)?;
    install_ring_provider();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(RunError::Runtime)?;
    runtime.block_on(serve(edge.https_listen, login, settings))
}

async fn serve(
    address: SocketAddr,
    login: LoginConfig,
    settings: Arc<QueueSettings>,
) -> Result<(), RunError> {
    let pool = PgPoolOptions::new()
        .max_connections(login.max_connections)
        .connect(&login.database_url)
        .await
        .map_err(database_error)?;
    WORLD_MIGRATOR.run(&pool).await.map_err(database_error)?;
    let queue = ValkeyQueue::connect(
        &login.valkey_url,
        PRODUCTION_KEY_PREFIX,
        Arc::clone(&settings),
    )
    .await
    .map_err(queue_error)?;
    let metrics = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|error| RunError::Metrics(error.to_string()))?;
    let app = router(
        AppState {
            queue,
            characters: PgCharacters::new(pool),
            settings,
        },
        Some(metrics),
    );
    let tls = RustlsConfig::from_pem_file(&login.certificate_pem_path, &login.private_key_pem_path)
        .await
        .map_err(RunError::Tls)?;
    let handle = Handle::new();
    let shutdown = handle.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown.graceful_shutdown(Some(Duration::from_secs(SHUTDOWN_GRACE_SECONDS)));
        }
    });
    axum_server::tls_rustls::bind_rustls(address, tls)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .map_err(RunError::Serve)?;
    Ok(())
}

fn install_ring_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn edge_error(error: EdgeConfigError) -> RunError {
    RunError::Edge(Box::new(error))
}

fn login_error(error: LoginConfigError) -> RunError {
    RunError::Login(Box::new(error))
}

fn log_error(error: LoggingError) -> RunError {
    RunError::Log(Box::new(error))
}

fn database_error(error: impl Error + Send + Sync + 'static) -> RunError {
    RunError::Database(Box::new(error))
}

fn queue_error(error: QueueError) -> RunError {
    RunError::Queue(Box::new(error))
}
