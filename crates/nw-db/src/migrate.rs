// SPDX-License-Identifier: AGPL-3.0-or-later
//! Embedded `sqlx` migrations. `Migrator::run` takes a PostgreSQL advisory lock, so two copies
//! of `edge-http migrate` never run the same DDL at once (review finding I5).

use sqlx::PgPool;
use sqlx::migrate::Migrator;

use crate::error::DbError;

/// Migrations of the global database.
pub static GLOBAL_MIGRATOR: Migrator = sqlx::migrate!("./migrations/global");

/// Migrations of every world database.
pub static WORLD_MIGRATOR: Migrator = sqlx::migrate!("./migrations/world");

/// Migration version when no migration has run.
const NO_MIGRATION: i64 = 0;

/// Fails unless every migration of `migrator` is applied to `pool`.
pub async fn check_schema(pool: &PgPool, migrator: &Migrator) -> Result<(), DbError> {
    let expected = migrator
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap_or(NO_MIGRATION);
    let table_exists: bool =
        sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
            .fetch_one(pool)
            .await?;
    let found = if table_exists {
        sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(version) FROM _sqlx_migrations WHERE success",
        )
        .fetch_one(pool)
        .await?
        .unwrap_or(NO_MIGRATION)
    } else {
        NO_MIGRATION
    };
    if found == expected {
        Ok(())
    } else {
        Err(DbError::SchemaNotCurrent { expected, found })
    }
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use super::*;
    use crate::test_db::with_test_schema;

    #[tokio::test]
    async fn schema_check_fails_before_and_passes_after_migration() {
        let (before, after, again) = with_test_schema(async |pool| {
            let before = check_schema(pool, &GLOBAL_MIGRATOR).await;
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            let after = check_schema(pool, &GLOBAL_MIGRATOR).await;
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate again");
            let again = check_schema(pool, &GLOBAL_MIGRATOR).await;
            (before, after, again)
        })
        .await;
        assert!(matches!(
            before,
            Err(DbError::SchemaNotCurrent {
                found: NO_MIGRATION,
                ..
            })
        ));
        assert!(after.is_ok());
        assert!(again.is_ok());
    }

    #[tokio::test]
    async fn world_schema_is_separate() {
        let result = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate world");
            check_schema(pool, &WORLD_MIGRATOR).await
        })
        .await;
        assert!(result.is_ok());
    }
}
