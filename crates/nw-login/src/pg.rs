// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`AccountStore`] on the global PostgreSQL database.

use nw_db::{Account, DbError, PgPool};
use uuid::Uuid;

use crate::store::{AccountStore, StoreError};

/// Accounts and launcher sessions in PostgreSQL.
#[derive(Debug, Clone)]
pub struct PgAccounts {
    pool: PgPool,
}

impl PgAccounts {
    /// Uses a pool of the migrated global database.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn store_error(error: DbError) -> StoreError {
    match error {
        DbError::DuplicateUsername => StoreError::DuplicateUsername,
        other => StoreError::unavailable(other),
    }
}

impl AccountStore for PgAccounts {
    async fn create_password_account(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<Account, StoreError> {
        nw_db::create_password_account(&self.pool, username, password_hash)
            .await
            .map_err(store_error)
    }

    async fn password_login(
        &self,
        username: &str,
    ) -> Result<Option<(Account, String)>, StoreError> {
        nw_db::password_login(&self.pool, username)
            .await
            .map_err(store_error)
    }

    async fn discord_account(
        &self,
        discord_user_id: &str,
        display_name: &str,
    ) -> Result<Account, StoreError> {
        nw_db::discord_account(&self.pool, discord_user_id, display_name)
            .await
            .map_err(store_error)
    }

    async fn create_launcher_session(
        &self,
        token_hash: [u8; 32],
        account_id: Uuid,
        expires_at: i64,
    ) -> Result<(), StoreError> {
        nw_db::create_launcher_session(&self.pool, &token_hash, account_id, expires_at)
            .await
            .map_err(store_error)
    }

    async fn launcher_session_account(
        &self,
        token_hash: [u8; 32],
    ) -> Result<Option<Account>, StoreError> {
        nw_db::launcher_session_account(&self.pool, &token_hash)
            .await
            .map_err(store_error)
    }

    async fn delete_launcher_session(&self, token_hash: [u8; 32]) -> Result<(), StoreError> {
        nw_db::delete_launcher_session(&self.pool, &token_hash)
            .await
            .map_err(store_error)
    }
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use nw_db::GLOBAL_MIGRATOR;
    use nw_db::test_db::with_test_schema;

    use super::*;
    use crate::memory::contract;

    #[tokio::test]
    async fn pg_accounts_follow_the_contract() {
        with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            contract::accounts(PgAccounts::new(pool.clone())).await;
        })
        .await;
    }
}
