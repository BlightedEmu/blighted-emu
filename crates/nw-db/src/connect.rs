// SPDX-License-Identifier: AGPL-3.0-or-later
//! PostgreSQL connections. The URL comes from a config file, the password from the environment.

use std::str::FromStr;

use serde::Deserialize;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// A PostgreSQL connection. The password comes from the environment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// URL without a password.
    pub url: String,
    /// Pool size.
    pub max_connections: u32,
}

/// Connection options: the URL from the config file and the password from the environment.
///
/// # Errors
///
/// Returns the `sqlx` error for a URL that does not parse.
pub fn connect_options(
    database: &DatabaseConfig,
    password: &str,
) -> Result<PgConnectOptions, sqlx::Error> {
    Ok(PgConnectOptions::from_str(&database.url)?.password(password))
}

/// A pool for one database.
///
/// # Errors
///
/// Returns the `sqlx` error when the URL does not parse or the database cannot be reached.
pub async fn connect(database: &DatabaseConfig, password: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(database.max_connections)
        .connect_with(connect_options(database, password)?)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "postgres://nw@127.0.0.1:5432/nw_global";
    const MAX_CONNECTIONS: u32 = 5;

    #[test]
    fn password_comes_from_the_secret() {
        let database = DatabaseConfig {
            url: URL.to_owned(),
            max_connections: MAX_CONNECTIONS,
        };
        let options = connect_options(&database, "s3cret").expect("options");
        assert_eq!(options.get_username(), "nw");
        assert_eq!(options.get_database(), Some("nw_global"));
        assert_eq!(options.get_port(), 5432);
    }

    #[test]
    fn bad_url_is_an_error() {
        let database = DatabaseConfig {
            url: "not a url".to_owned(),
            max_connections: MAX_CONNECTIONS,
        };
        assert!(connect_options(&database, "pw").is_err());
    }
}
