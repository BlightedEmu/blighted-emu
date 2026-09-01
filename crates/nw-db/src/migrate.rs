// SPDX-License-Identifier: AGPL-3.0-or-later
//! Applies the account and character tables.

use sqlx::PgPool;

use crate::error::{DbError, database};

const MIGRATION_VERSION: &str = "001_accounts_and_characters";
const MIGRATION_SQL: &str = include_str!("../sql/001_accounts_and_characters.sql");
const POSTGRES_UNIQUE_VIOLATION: &str = "23505";

/// Creates the account and character tables when this version is not recorded yet.
pub async fn migrate(pool: &PgPool) -> Result<(), DbError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS nw_schema_migration (
            version TEXT PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .map_err(database)?;

    let applied: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM nw_schema_migration WHERE version = $1)")
            .bind(MIGRATION_VERSION)
            .fetch_one(pool)
            .await
            .map_err(database)?;
    if applied {
        return Ok(());
    }

    let mut tx = pool.begin().await.map_err(database)?;
    for statement in statements(MIGRATION_SQL) {
        // The statement text is the embedded migration file, split on `;`.
        // It does not contain request data.
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    }
    sqlx::query(
        "INSERT INTO nw_schema_migration (version) VALUES ($1)
         ON CONFLICT (version) DO NOTHING",
    )
    .bind(MIGRATION_VERSION)
    .execute(&mut *tx)
    .await
    .map_err(database)?;
    tx.commit().await.map_err(database)?;
    Ok(())
}

fn statements(sql: &str) -> Vec<String> {
    sql.split(';')
        .filter_map(|part| {
            let statement: String = part
                .lines()
                .filter(|line| !line.trim().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n");
            let statement = statement.trim().to_owned();
            if statement.is_empty() {
                None
            } else {
                Some(statement)
            }
        })
        .collect()
}

pub(crate) fn is_unique_violation(error: &DbError, constraint: &str) -> bool {
    let DbError::Database(sql) = error else {
        return false;
    };
    let Some(database_error) = sql.as_database_error() else {
        return false;
    };
    database_error.code().as_deref() == Some(POSTGRES_UNIQUE_VIOLATION)
        && database_error.constraint() == Some(constraint)
}

pub(crate) fn is_foreign_key_violation(error: &DbError, constraint: &str) -> bool {
    const POSTGRES_FOREIGN_KEY_VIOLATION: &str = "23503";
    let DbError::Database(sql) = error else {
        return false;
    };
    let Some(database_error) = sql.as_database_error() else {
        return false;
    };
    database_error.code().as_deref() == Some(POSTGRES_FOREIGN_KEY_VIOLATION)
        && database_error.constraint() == Some(constraint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_text_has_both_tables() {
        let found = statements(MIGRATION_SQL);
        assert_eq!(found.len(), 3);
        assert!(found[0].contains("login_account"));
        assert!(found[1].contains("player_character"));
        assert!(found[2].contains("player_character_account_world_idx"));
    }
}
