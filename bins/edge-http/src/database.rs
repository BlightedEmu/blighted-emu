// SPDX-License-Identifier: AGPL-3.0-or-later
//! PostgreSQL pools of the global and the world databases.

use nw_db::{DatabaseConfig, connect, connect_options};
use nw_login::config::EdgeConfig;
use nw_login::secrets::DatabasePasswords;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Maintenance database that every PostgreSQL server has.
const MAINTENANCE_DATABASE: &str = "postgres";

/// Error from [`ensure_database`].
#[derive(Debug, thiserror::Error)]
pub enum EnsureDatabaseError {
    /// The URL names no database.
    #[error("database URL {0} names no database")]
    NoDatabaseName(String),
    /// The database name has characters other than `a-z 0-9 _`.
    #[error("database name {0} must use only a-z, 0-9 and _")]
    BadName(String),
    /// PostgreSQL failed.
    #[error("database creation failed")]
    Database(#[from] sqlx::Error),
}

/// Creates the database of `database` when it does not exist (`edge-http migrate` on a new
/// server). The user needs the `CREATEDB` right.
pub(crate) async fn ensure_database(
    database: &DatabaseConfig,
    password: &str,
) -> Result<(), EnsureDatabaseError> {
    let options = connect_options(database, password)?;
    let name = options
        .get_database()
        .ok_or_else(|| EnsureDatabaseError::NoDatabaseName(database.url.clone()))?
        .to_owned();
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(EnsureDatabaseError::BadName(name));
    }
    let maintenance = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.database(MAINTENANCE_DATABASE))
        .await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&name)
            .fetch_one(&maintenance)
            .await?;
    if !exists {
        // The name was checked above, so it is a safe identifier.
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&maintenance)
            .await?;
        tracing::info!(database = %name, "created database");
    }
    maintenance.close().await;
    Ok(())
}

/// The global pool and one pool per world, in config order.
pub(crate) async fn connect_all(
    config: &EdgeConfig,
    passwords: &DatabasePasswords,
) -> Result<(PgPool, Vec<PgPool>), sqlx::Error> {
    let global = connect(&config.global_database, &passwords.global).await?;
    let mut worlds = Vec::with_capacity(config.worlds.len());
    for (world, password) in config.worlds.iter().zip(&passwords.worlds) {
        worlds.push(connect(&world.database, password).await?);
    }
    Ok((global, worlds))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_CONNECTIONS: u32 = 5;

    #[tokio::test]
    async fn unsafe_database_name_is_refused_before_connecting() {
        let database = DatabaseConfig {
            url: "postgres://nw@127.0.0.1:5432/World-1".to_owned(),
            max_connections: MAX_CONNECTIONS,
        };
        assert!(matches!(
            ensure_database(&database, "pw").await,
            Err(EnsureDatabaseError::BadName(name)) if name == "World-1"
        ));
    }
}
