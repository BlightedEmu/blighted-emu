// SPDX-License-Identifier: AGPL-3.0-or-later
//! Accounts and their logins in the global database. Names and passwords are checked and hashed
//! by `nw-login`; this module only stores what it gets.

use sqlx::PgPool;
use uuid::Uuid;

use crate::Account;
use crate::error::{DbError, is_unique_violation};

const PASSWORD_LOGIN_PKEY: &str = "password_login_pkey";
const DISCORD_LOGIN_PKEY: &str = "discord_login_pkey";

/// Columns of [`Account`], in this order, for `query_as`.
const ACCOUNT_COLUMNS: &str = "account.account_id, account.display_name, \
     CAST(EXTRACT(EPOCH FROM account.created_at) AS BIGINT), account.banned";

type AccountRow = (Uuid, String, i64, bool);

fn account_from_row((id, display_name, created_at_unix, banned): AccountRow) -> Account {
    Account {
        id,
        display_name,
        created_at_unix,
        banned,
    }
}

/// Creates an account with a password login. `username` must already be in lower case.
pub async fn create_password_account(
    pool: &PgPool,
    username: &str,
    password_hash: &str,
) -> Result<Account, DbError> {
    let mut transaction = pool.begin().await?;
    let account = insert_account(&mut transaction, username).await?;
    let inserted = sqlx::query(
        "INSERT INTO password_login (username, account_id, password_hash) VALUES ($1, $2, $3)",
    )
    .bind(username)
    .bind(account.id)
    .bind(password_hash)
    .execute(&mut *transaction)
    .await;
    match inserted {
        Ok(_) => {}
        Err(error) if is_unique_violation(&error, PASSWORD_LOGIN_PKEY) => {
            return Err(DbError::DuplicateUsername);
        }
        Err(error) => return Err(error.into()),
    }
    transaction.commit().await?;
    Ok(account)
}

/// The account and stored password hash of a lower-case username.
pub async fn password_login(
    pool: &PgPool,
    username: &str,
) -> Result<Option<(Account, String)>, DbError> {
    let row: Option<(Uuid, String, i64, bool, String)> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {ACCOUNT_COLUMNS}, password_login.password_hash FROM password_login \
             JOIN account ON account.account_id = password_login.account_id \
             WHERE password_login.username = $1"
        )))
        .bind(username)
        .fetch_optional(pool)
        .await?;
    Ok(
        row.map(|(id, display_name, created_at_unix, banned, hash)| {
            (
                account_from_row((id, display_name, created_at_unix, banned)),
                hash,
            )
        }),
    )
}

/// Finds the account of a Discord user, or creates it with `display_name`.
pub async fn discord_account(
    pool: &PgPool,
    discord_user_id: &str,
    display_name: &str,
) -> Result<Account, DbError> {
    if let Some(account) = account_by_discord(pool, discord_user_id).await? {
        return Ok(account);
    }
    let mut transaction = pool.begin().await?;
    let account = insert_account(&mut transaction, display_name).await?;
    let inserted =
        sqlx::query("INSERT INTO discord_login (discord_user_id, account_id) VALUES ($1, $2)")
            .bind(discord_user_id)
            .bind(account.id)
            .execute(&mut *transaction)
            .await;
    match inserted {
        Ok(_) => {
            transaction.commit().await?;
            Ok(account)
        }
        // Another request created the account first; use that one.
        Err(error) if is_unique_violation(&error, DISCORD_LOGIN_PKEY) => {
            drop(transaction);
            account_by_discord(pool, discord_user_id)
                .await?
                .ok_or(DbError::Database(Box::new(error)))
        }
        Err(error) => Err(error.into()),
    }
}

/// Loads one account.
pub async fn account_by_id(pool: &PgPool, account_id: Uuid) -> Result<Option<Account>, DbError> {
    let row: Option<AccountRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ACCOUNT_COLUMNS} FROM account WHERE account.account_id = $1"
    )))
    .bind(account_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(account_from_row))
}

/// Sets or clears the ban flag. Returns false when the account does not exist.
pub async fn set_banned(pool: &PgPool, account_id: Uuid, banned: bool) -> Result<bool, DbError> {
    let result = sqlx::query("UPDATE account SET banned = $2 WHERE account_id = $1")
        .bind(account_id)
        .bind(banned)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

async fn account_by_discord(
    pool: &PgPool,
    discord_user_id: &str,
) -> Result<Option<Account>, DbError> {
    let row: Option<AccountRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ACCOUNT_COLUMNS} FROM discord_login \
         JOIN account ON account.account_id = discord_login.account_id \
         WHERE discord_login.discord_user_id = $1"
    )))
    .bind(discord_user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(account_from_row))
}

async fn insert_account(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    display_name: &str,
) -> Result<Account, DbError> {
    let row: AccountRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO account (account_id, display_name) VALUES ($1, $2) \
         RETURNING {ACCOUNT_COLUMNS}"
    )))
    .bind(Uuid::new_v4())
    .bind(display_name)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(account_from_row(row))
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use super::*;
    use crate::migrate::GLOBAL_MIGRATOR;
    use crate::test_db::with_test_schema;

    const USERNAME: &str = "ada";
    const HASH: &str = "$argon2id$stored";
    const DISCORD_ID: &str = "80351110224678912";
    const DISCORD_NAME: &str = "Nelly";

    #[tokio::test]
    async fn password_accounts_are_unique_and_found_by_username() {
        let (created, found, duplicate, missing) = with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            let created = create_password_account(pool, USERNAME, HASH)
                .await
                .expect("create");
            let found = password_login(pool, USERNAME).await.expect("find");
            let duplicate = create_password_account(pool, USERNAME, HASH).await;
            let missing = password_login(pool, "nobody").await.expect("missing");
            (created, found, duplicate, missing)
        })
        .await;
        assert_eq!(created.display_name, USERNAME);
        assert!(!created.banned);
        assert!(created.created_at_unix > 0);
        assert_eq!(found, Some((created, HASH.to_owned())));
        assert!(matches!(duplicate, Err(DbError::DuplicateUsername)));
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn discord_account_is_created_once() {
        let (first, second) = with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            let first = discord_account(pool, DISCORD_ID, DISCORD_NAME)
                .await
                .expect("first");
            let second = discord_account(pool, DISCORD_ID, "renamed")
                .await
                .expect("second");
            (first, second)
        })
        .await;
        assert_eq!(first, second);
        assert_eq!(first.display_name, DISCORD_NAME);
    }

    #[tokio::test]
    async fn ban_flag_is_stored() {
        let (banned, loaded, unknown) = with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            let account = create_password_account(pool, USERNAME, HASH)
                .await
                .expect("create");
            let banned = set_banned(pool, account.id, true).await.expect("ban");
            let loaded = account_by_id(pool, account.id).await.expect("load");
            let unknown = set_banned(pool, Uuid::new_v4(), true)
                .await
                .expect("unknown");
            (banned, loaded, unknown)
        })
        .await;
        assert!(banned);
        assert!(loaded.expect("row").banned);
        assert!(!unknown);
    }
}
