// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`AccountStore`] and [`CharacterStore`] on PostgreSQL.

use std::collections::HashMap;
use std::sync::Arc;

use nw_db::{Account, Character, DbError, NewCharacter, PgPool};
use uuid::Uuid;

use crate::store::{AccountStore, CharacterStore, StoreError};

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
        DbError::DuplicateCharacterName => StoreError::DuplicateCharacterName,
        other => StoreError::unavailable(other),
    }
}

/// Characters in the world databases and their names in the global database.
#[derive(Debug, Clone)]
pub struct PgCharacters {
    global: PgPool,
    worlds: Arc<HashMap<Uuid, PgPool>>,
}

impl PgCharacters {
    /// Uses the migrated global database and one migrated database per world id.
    pub fn new(global: PgPool, worlds: HashMap<Uuid, PgPool>) -> Self {
        Self {
            global,
            worlds: Arc::new(worlds),
        }
    }

    fn world(&self, world_id: Uuid) -> Result<&PgPool, StoreError> {
        self.worlds.get(&world_id).ok_or(StoreError::UnknownWorld)
    }
}

impl CharacterStore for PgCharacters {
    async fn characters(
        &self,
        world_id: Uuid,
        account_id: Uuid,
    ) -> Result<Vec<Character>, StoreError> {
        nw_db::characters_for_account(self.world(world_id)?, account_id)
            .await
            .map_err(store_error)
    }

    async fn character(
        &self,
        world_id: Uuid,
        character_id: Uuid,
    ) -> Result<Option<Character>, StoreError> {
        nw_db::character_by_id(self.world(world_id)?, character_id)
            .await
            .map_err(store_error)
    }

    async fn name_taken(&self, name: &str) -> Result<bool, StoreError> {
        nw_db::character_name_taken(&self.global, name)
            .await
            .map_err(store_error)
    }

    async fn create(&self, world_id: Uuid, new: &NewCharacter) -> Result<Character, StoreError> {
        let world = self.world(world_id)?;
        nw_db::reserve_character_name(&self.global, &new.name, world_id, new.id)
            .await
            .map_err(store_error)?;
        match nw_db::insert_character(world, new).await {
            Ok(character) => Ok(character),
            Err(error) => {
                if let Err(release) = nw_db::release_character_name(&self.global, new.id).await {
                    tracing::error!(error = %release, "character name not released");
                }
                Err(store_error(error))
            }
        }
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
    use nw_db::test_db::with_test_schema;
    use nw_db::{GLOBAL_MIGRATOR, WORLD_MIGRATOR};

    use super::*;
    use crate::memory::contract;

    const NAME: &str = "Ada";

    #[tokio::test]
    async fn pg_accounts_follow_the_contract() {
        with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            contract::accounts(PgAccounts::new(pool.clone())).await;
        })
        .await;
    }

    #[tokio::test]
    async fn pg_characters_follow_the_contract() {
        with_test_schema(async |global| {
            GLOBAL_MIGRATOR.run(global).await.expect("migrate global");
            with_test_schema(async |first| {
                WORLD_MIGRATOR.run(first).await.expect("migrate first");
                with_test_schema(async |second| {
                    WORLD_MIGRATOR.run(second).await.expect("migrate second");
                    let (first_id, second_id) = (Uuid::new_v4(), Uuid::new_v4());
                    let worlds =
                        HashMap::from([(first_id, first.clone()), (second_id, second.clone())]);
                    let store = PgCharacters::new(global.clone(), worlds);
                    contract::characters(store, first_id, second_id).await;
                })
                .await;
            })
            .await;
        })
        .await;
    }

    #[tokio::test]
    async fn failed_world_insert_frees_the_name() {
        with_test_schema(async |global| {
            GLOBAL_MIGRATOR.run(global).await.expect("migrate global");
            // The world database is not migrated, so the insert fails.
            with_test_schema(async |world| {
                let world_id = Uuid::new_v4();
                let store =
                    PgCharacters::new(global.clone(), HashMap::from([(world_id, world.clone())]));
                let new = NewCharacter {
                    id: Uuid::new_v4(),
                    account_id: Uuid::new_v4(),
                    name: NAME.to_owned(),
                    appearance: serde_json::json!({}),
                };
                assert!(matches!(
                    store.create(world_id, &new).await,
                    Err(StoreError::Unavailable(_))
                ));
                assert!(!store.name_taken(NAME).await.expect("name"));
            })
            .await;
        })
        .await;
    }
}
