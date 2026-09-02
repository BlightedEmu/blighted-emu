// SPDX-License-Identifier: AGPL-3.0-or-later
//! Character lookup used before a player with a character id enters the queue.

use sqlx::PgPool;

use nw_db::{DbError, Uuid, character_by_id};

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

/// PostgreSQL character records of the one configured world database.
#[derive(Clone)]
pub(crate) struct PgCharacters {
    pool: PgPool,
}

impl PgCharacters {
    /// Uses the migrated world pool.
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl CharacterLookup for PgCharacters {
    /// The pool is the database of the configured world, so a row in it belongs to that world.
    async fn exists(&self, _world_id: &str, character_id: &str) -> Result<bool, LookupError> {
        let Ok(id) = Uuid::parse_str(character_id) else {
            return Ok(false);
        };
        match character_by_id(&self.pool, id).await {
            Ok(found) => Ok(found.is_some()),
            Err(cause) => Err(LookupError { cause }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_db::test_db::with_test_schema;
    use nw_db::{NewCharacter, WORLD_MIGRATOR, insert_character};

    const WORLD: &str = "world-1";

    #[tokio::test]
    async fn lookup_finds_world_rows_only() {
        let found = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let character = insert_character(
                pool,
                &NewCharacter {
                    id: Uuid::new_v4(),
                    account_id: Uuid::new_v4(),
                    name: "Ada".to_owned(),
                    appearance: serde_json::json!({}),
                },
            )
            .await
            .expect("character");
            let lookup = PgCharacters::new(pool.clone());
            let same = lookup
                .exists(WORLD, &character.id.to_string())
                .await
                .expect("same");
            let missing = lookup
                .exists(WORLD, &Uuid::new_v4().to_string())
                .await
                .expect("missing");
            let not_uuid = lookup.exists(WORLD, "missing").await.expect("not uuid");
            (same, missing, not_uuid)
        })
        .await;
        assert!(found.0);
        assert!(!found.1);
        assert!(!found.2);
    }
}
