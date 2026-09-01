// SPDX-License-Identifier: AGPL-3.0-or-later
//! Character lookup used before a player with a character id enters the queue.

use sqlx::PgPool;

use nw_db::{DbError, character_by_id};

/// The character store could not answer.
#[derive(Debug, thiserror::Error)]
#[error("character store is unavailable")]
pub(crate) struct LookupError {
    #[source]
    cause: DbError,
}

/// Reads character rows.
pub(crate) trait CharacterLookup: Clone + Send + Sync + 'static {
    /// Returns whether this character id belongs to the world.
    fn exists(
        &self,
        world_id: &str,
        character_id: &str,
    ) -> impl std::future::Future<Output = Result<bool, LookupError>> + Send;
}

/// PostgreSQL character records.
#[derive(Clone)]
pub(crate) struct PgCharacters {
    pool: PgPool,
}

impl PgCharacters {
    /// Uses the migrated pool.
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl CharacterLookup for PgCharacters {
    async fn exists(&self, world_id: &str, character_id: &str) -> Result<bool, LookupError> {
        match character_by_id(&self.pool, character_id).await {
            Ok(Some(character)) => Ok(character.world_id == world_id),
            Ok(None) => Ok(false),
            Err(cause) => Err(LookupError { cause }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_db::test_db::with_test_schema;
    use nw_db::{create_account, create_character, migrate};

    const USERNAME: &str = "ada";
    const PASSWORD: &str = "password-ok";
    const WORLD: &str = "world-1";
    const NAME: &str = "Ada";

    #[tokio::test]
    async fn lookup_matches_the_character_world() {
        let found = with_test_schema(async |pool| {
            migrate(pool).await.expect("migrate");
            let account = create_account(pool, USERNAME, PASSWORD)
                .await
                .expect("account");
            let character = create_character(pool, &account.id, WORLD, NAME)
                .await
                .expect("character");
            let lookup = PgCharacters::new(pool.clone());
            let same = lookup.exists(WORLD, &character.id).await.expect("same");
            let other = lookup
                .exists("world-2", &character.id)
                .await
                .expect("other");
            let missing = lookup.exists(WORLD, "missing").await.expect("missing");
            (same, other, missing)
        })
        .await;
        assert!(found.0);
        assert!(!found.1);
        assert!(!found.2);
    }
}
