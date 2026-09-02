// SPDX-License-Identifier: AGPL-3.0-or-later
//! Errors from database records.

/// Failure while reading or writing a record.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// That username is already registered.
    #[error("username is already registered")]
    DuplicateUsername,
    /// That character name is already used in some world.
    #[error("character name is already used")]
    DuplicateCharacterName,
    /// The database schema is not at the version this program needs.
    #[error(
        "database schema is at migration {found}, this program needs {expected}; run `edge-http migrate`"
    )]
    SchemaNotCurrent {
        /// Newest migration of this program.
        expected: i64,
        /// Newest applied migration, 0 when none.
        found: i64,
    },
    /// A migration failed.
    #[error("migration failed")]
    Migrate(#[source] Box<sqlx::migrate::MigrateError>),
    /// PostgreSQL returned an error.
    #[error("database error")]
    Database(#[source] Box<sqlx::Error>),
}

impl From<sqlx::Error> for DbError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(Box::new(error))
    }
}

impl From<sqlx::migrate::MigrateError> for DbError {
    fn from(error: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(Box::new(error))
    }
}

/// True when `error` is a unique violation of the named constraint.
pub(crate) fn is_unique_violation(error: &sqlx::Error, constraint: &str) -> bool {
    error.as_database_error().is_some_and(|database| {
        database.is_unique_violation() && database.constraint() == Some(constraint)
    })
}
