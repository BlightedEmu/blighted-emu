// SPDX-License-Identifier: AGPL-3.0-or-later
//! A private schema on the Docker PostgreSQL database for tests.
//!
//! The schema is created in the database named by `deploy/.env` and dropped
//! when the test body returns.

use std::fs;
use std::path::PathBuf;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

const TEST_HOST: &str = "127.0.0.1";
const TEST_PORT: u16 = 5432;
const TEST_POOL_CONNECTIONS: u32 = 2;
const SCHEMA_PREFIX: &str = "nw_test_";
const ENV_USER: &str = "POSTGRES_USER";
const ENV_PASSWORD: &str = "POSTGRES_PASSWORD";
const ENV_DATABASE: &str = "POSTGRES_DB";
const DATABASE_URL_ENV: &str = "NW_TEST_DATABASE_URL";

/// Connects to the Docker PostgreSQL database, migrates nothing itself, and
/// drops the private schema after `test` returns.
pub async fn with_test_schema<T>(test: impl AsyncFnOnce(&PgPool) -> T) -> T {
    let url = database_url();
    let schema = schema_name();
    require_test_schema(&schema);
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect to the Docker PostgreSQL database");
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .expect("create test schema");
    let pool = pool_for_schema(&url, &schema).await;
    let value = test(&pool).await;
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .expect("drop test schema");
    admin.close().await;
    value
}

fn schema_name() -> String {
    format!("{SCHEMA_PREFIX}{}", Uuid::new_v4().simple())
}

fn require_test_schema(schema: &str) {
    let Some(rest) = schema.strip_prefix(SCHEMA_PREFIX) else {
        panic!("test schema must start with {SCHEMA_PREFIX}");
    };
    assert!(
        !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "test schema has a character that is not hexadecimal"
    );
}

async fn pool_for_schema(url: &str, schema: &str) -> PgPool {
    let schema = schema.to_owned();
    PgPoolOptions::new()
        .max_connections(TEST_POOL_CONNECTIONS)
        .after_connect(move |conn, _meta| {
            let schema = schema.clone();
            Box::pin(async move {
                sqlx::query(sqlx::AssertSqlSafe(format!("SET search_path TO {schema}")))
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .connect(url)
        .await
        .expect("connect test pool")
}

fn database_url() -> String {
    if let Ok(url) = std::env::var(DATABASE_URL_ENV) {
        return url;
    }
    let path = deploy_env_path();
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("set {DATABASE_URL_ENV} or provide {}", path.display()));
    let user = env_value(&text, ENV_USER);
    let password = env_value(&text, ENV_PASSWORD);
    let database = env_value(&text, ENV_DATABASE);
    format!(
        "postgres://{}:{}@{TEST_HOST}:{TEST_PORT}/{database}",
        percent_encode(&user),
        percent_encode(&password),
    )
}

fn deploy_env_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/.env")
}

fn env_value(text: &str, key: &str) -> String {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((found, value)) = line.split_once('=') else {
            continue;
        };
        if found == key {
            return value.to_owned();
        }
    }
    panic!("deploy/.env has no {key}");
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
