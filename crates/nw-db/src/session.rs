// SPDX-License-Identifier: AGPL-3.0-or-later
//! Launcher sessions (M2 spec section 5.1). Only SHA-256 of a launcher token is stored.

use sqlx::PgPool;
use uuid::Uuid;

use crate::Account;
use crate::account::account_by_id;
use crate::error::DbError;

/// Stores a launcher session that ends at `expires_at_unix` (seconds).
pub async fn create_launcher_session(
    pool: &PgPool,
    token_hash: &[u8],
    account_id: Uuid,
    expires_at_unix: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO launcher_session (token_hash, account_id, expires_at) \
         VALUES ($1, $2, to_timestamp($3))",
    )
    .bind(token_hash)
    .bind(account_id)
    .bind(expires_at_unix)
    .execute(pool)
    .await?;
    Ok(())
}

/// The account of a launcher session that has not expired.
pub async fn launcher_session_account(
    pool: &PgPool,
    token_hash: &[u8],
) -> Result<Option<Account>, DbError> {
    let account_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT account_id FROM launcher_session WHERE token_hash = $1 AND expires_at > now()",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    match account_id {
        Some(id) => account_by_id(pool, id).await,
        None => Ok(None),
    }
}

/// Deletes one launcher session and every expired one.
pub async fn delete_launcher_session(pool: &PgPool, token_hash: &[u8]) -> Result<(), DbError> {
    sqlx::query("DELETE FROM launcher_session WHERE token_hash = $1 OR expires_at <= now()")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use super::*;
    use crate::account::create_password_account;
    use crate::migrate::GLOBAL_MIGRATOR;
    use crate::test_db::with_test_schema;

    const HASH_A: [u8; 32] = [1; 32];
    const HASH_B: [u8; 32] = [2; 32];
    const FAR_FUTURE: i64 = 4_102_444_800;
    const PAST: i64 = 1_000_000_000;

    #[tokio::test]
    async fn live_session_resolves_and_expired_or_deleted_does_not() {
        let (account, live, expired, deleted) = with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            let account = create_password_account(pool, "ada", "hash")
                .await
                .expect("account");
            create_launcher_session(pool, &HASH_A, account.id, FAR_FUTURE)
                .await
                .expect("live");
            create_launcher_session(pool, &HASH_B, account.id, PAST)
                .await
                .expect("expired");
            let live = launcher_session_account(pool, &HASH_A).await.expect("live");
            let expired = launcher_session_account(pool, &HASH_B)
                .await
                .expect("expired");
            delete_launcher_session(pool, &HASH_A)
                .await
                .expect("delete");
            let deleted = launcher_session_account(pool, &HASH_A)
                .await
                .expect("deleted");
            (account, live, expired, deleted)
        })
        .await;
        assert_eq!(live, Some(account));
        assert!(expired.is_none());
        assert!(deleted.is_none());
    }
}
