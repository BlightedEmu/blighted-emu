// SPDX-License-Identifier: AGPL-3.0-or-later
//! Account rows in the global PostgreSQL database.

use sqlx::PgPool;
use uuid::Uuid;

use crate::Account;
use crate::error::{DbError, database};
use crate::limits::{MAX_USERNAME_LEN, MIN_PASSWORD_LEN};
use crate::migrate::is_unique_violation;
use crate::password::{hash_password, password_matches};

const USERNAME_CONSTRAINT: &str = "login_account_username_unique";

/// Inserts an account. The password is stored as an Argon2id hash.
pub async fn create_account(
    pool: &PgPool,
    username: &str,
    password: &str,
) -> Result<Account, DbError> {
    check_username(username)?;
    check_password(password)?;
    let id = Uuid::new_v4().to_string();
    let hash = hash_password(password)?;
    let inserted = insert_account(pool, &id, username, &hash).await;
    match inserted {
        Ok(account) => Ok(account),
        Err(error) if is_unique_violation(&error, USERNAME_CONSTRAINT) => {
            Err(DbError::DuplicateUsername)
        }
        Err(error) => Err(error),
    }
}

/// Returns the account when the username and password match.
///
/// An unknown username and a wrong password return the same error.
pub async fn authenticate(
    pool: &PgPool,
    username: &str,
    password: &str,
) -> Result<Account, DbError> {
    let Some(row) = account_row_by_username(pool, username).await? else {
        let _ = hash_password(password);
        return Err(DbError::InvalidCredentials);
    };
    if password_matches(password, &row.password_hash)? {
        Ok(row.account)
    } else {
        Err(DbError::InvalidCredentials)
    }
}

/// Loads one account by id.
pub async fn account_by_id(pool: &PgPool, account_id: &str) -> Result<Option<Account>, DbError> {
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT account_id, username, CAST(EXTRACT(EPOCH FROM created_at) AS bigint)
         FROM login_account
         WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(database)?;
    Ok(row.map(account_from_row))
}

pub(crate) fn check_username(username: &str) -> Result<(), DbError> {
    if username.is_empty() || username.trim().is_empty() {
        return Err(DbError::EmptyUsername);
    }
    if username.len() > MAX_USERNAME_LEN {
        return Err(DbError::UsernameTooLong);
    }
    if username
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(DbError::UsernameInvalid);
    }
    Ok(())
}

pub(crate) fn check_password(password: &str) -> Result<(), DbError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(DbError::PasswordTooShort);
    }
    if password.len() > crate::limits::MAX_PASSWORD_LEN {
        return Err(DbError::PasswordTooLong);
    }
    Ok(())
}

struct StoredAccount {
    account: Account,
    password_hash: String,
}

async fn insert_account(
    pool: &PgPool,
    account_id: &str,
    username: &str,
    password_hash: &str,
) -> Result<Account, DbError> {
    let row: (String, String, i64) = sqlx::query_as(
        "INSERT INTO login_account (account_id, username, password_hash)
         VALUES ($1, $2, $3)
         RETURNING account_id, username, CAST(EXTRACT(EPOCH FROM created_at) AS bigint)",
    )
    .bind(account_id)
    .bind(username)
    .bind(password_hash)
    .fetch_one(pool)
    .await
    .map_err(database)?;
    Ok(account_from_row(row))
}

async fn account_row_by_username(
    pool: &PgPool,
    username: &str,
) -> Result<Option<StoredAccount>, DbError> {
    let row: Option<(String, String, String, i64)> = sqlx::query_as(
        "SELECT account_id, username, password_hash,
                CAST(EXTRACT(EPOCH FROM created_at) AS bigint)
         FROM login_account
         WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await
    .map_err(database)?;
    Ok(row.map(
        |(id, username, password_hash, created_at_unix)| StoredAccount {
            account: Account {
                id,
                username,
                created_at_unix,
            },
            password_hash,
        },
    ))
}

fn account_from_row(row: (String, String, i64)) -> Account {
    Account {
        id: row.0,
        username: row.1,
        created_at_unix: row.2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::migrate;
    use crate::test_db::with_test_schema;

    const USERNAME: &str = "ada";
    const PASSWORD: &str = "password-ok";
    const SHORT_PASSWORD: &str = "short";

    #[test]
    fn rejects_bad_username_and_password_before_a_database_call() {
        assert!(matches!(check_username(""), Err(DbError::EmptyUsername)));
        assert!(matches!(
            check_username("a name"),
            Err(DbError::UsernameInvalid)
        ));
        assert!(matches!(
            check_password(SHORT_PASSWORD),
            Err(DbError::PasswordTooShort)
        ));
    }

    #[tokio::test]
    async fn creates_and_authenticates_an_account() {
        let account = with_test_schema(async |pool| {
            migrate(pool).await.expect("migrate");
            let created = create_account(pool, USERNAME, PASSWORD)
                .await
                .expect("create");
            let loaded = account_by_id(pool, &created.id)
                .await
                .expect("load")
                .expect("row");
            let signed_in = authenticate(pool, USERNAME, PASSWORD)
                .await
                .expect("authenticate");
            (created, loaded, signed_in)
        })
        .await;
        let (created, loaded, signed_in) = account;
        assert_eq!(loaded, created);
        assert_eq!(signed_in.id, created.id);
        assert_eq!(signed_in.username, USERNAME);
        assert!(created.created_at_unix > 0);
    }

    #[tokio::test]
    async fn duplicate_username_and_bad_credentials_fail() {
        let errors = with_test_schema(async |pool| {
            migrate(pool).await.expect("migrate");
            create_account(pool, USERNAME, PASSWORD)
                .await
                .expect("create");
            let duplicate = create_account(pool, USERNAME, "other-password-ok").await;
            let wrong = authenticate(pool, USERNAME, "other-password-ok").await;
            let missing = authenticate(pool, "nobody", PASSWORD).await;
            (duplicate, wrong, missing)
        })
        .await;
        assert!(matches!(errors.0, Err(DbError::DuplicateUsername)));
        assert!(matches!(errors.1, Err(DbError::InvalidCredentials)));
        assert!(matches!(errors.2, Err(DbError::InvalidCredentials)));
    }
}
