// SPDX-License-Identifier: AGPL-3.0-or-later
//! Character names (global database) and characters (world databases), M2 spec section 7.
//! `nw-login` checks names, reserves the name first, then inserts the world row, and releases
//! the name when the insert fails.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{DbError, is_unique_violation};
use crate::{Character, NewCharacter};

const CHARACTER_NAME_PKEY: &str = "character_name_pkey";

type CharacterRow = (Uuid, Uuid, String, serde_json::Value, i64);

const CHARACTER_COLUMNS: &str = "character_id, account_id, name, appearance, \
     CAST(EXTRACT(EPOCH FROM created_at) AS BIGINT)";

fn character_from_row(
    (id, account_id, name, appearance, created_at_unix): CharacterRow,
) -> Character {
    Character {
        id,
        account_id,
        name,
        appearance,
        created_at_unix,
    }
}

/// Reserves `name` (compared in lower case) for a character in `world_id`.
pub async fn reserve_character_name(
    global: &PgPool,
    name: &str,
    world_id: Uuid,
    character_id: Uuid,
) -> Result<(), DbError> {
    let inserted = sqlx::query(
        "INSERT INTO character_name (name_key, world_id, character_id) VALUES ($1, $2, $3)",
    )
    .bind(name.to_lowercase())
    .bind(world_id)
    .bind(character_id)
    .execute(global)
    .await;
    match inserted {
        Ok(_) => Ok(()),
        Err(error) if is_unique_violation(&error, CHARACTER_NAME_PKEY) => {
            Err(DbError::DuplicateCharacterName)
        }
        Err(error) => Err(error.into()),
    }
}

/// Frees the name of a character.
pub async fn release_character_name(global: &PgPool, character_id: Uuid) -> Result<(), DbError> {
    sqlx::query("DELETE FROM character_name WHERE character_id = $1")
        .bind(character_id)
        .execute(global)
        .await?;
    Ok(())
}

/// Inserts a character into a world database.
pub async fn insert_character(world: &PgPool, new: &NewCharacter) -> Result<Character, DbError> {
    let row: CharacterRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO player_character (character_id, account_id, name, appearance) \
         VALUES ($1, $2, $3, $4) RETURNING {CHARACTER_COLUMNS}"
    )))
    .bind(new.id)
    .bind(new.account_id)
    .bind(&new.name)
    .bind(&new.appearance)
    .fetch_one(world)
    .await?;
    Ok(character_from_row(row))
}

/// The characters of one account in one world, oldest first.
pub async fn characters_for_account(
    world: &PgPool,
    account_id: Uuid,
) -> Result<Vec<Character>, DbError> {
    let rows: Vec<CharacterRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CHARACTER_COLUMNS} FROM player_character \
         WHERE account_id = $1 ORDER BY created_at, character_id"
    )))
    .bind(account_id)
    .fetch_all(world)
    .await?;
    Ok(rows.into_iter().map(character_from_row).collect())
}

/// One character of a world database.
pub async fn character_by_id(
    world: &PgPool,
    character_id: Uuid,
) -> Result<Option<Character>, DbError> {
    let row: Option<CharacterRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CHARACTER_COLUMNS} FROM player_character WHERE character_id = $1"
    )))
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    Ok(row.map(character_from_row))
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use super::*;
    use crate::migrate::{GLOBAL_MIGRATOR, WORLD_MIGRATOR};
    use crate::test_db::with_test_schema;

    const NAME: &str = "Ada";

    fn new_character(account_id: Uuid) -> NewCharacter {
        NewCharacter {
            id: Uuid::new_v4(),
            account_id,
            name: NAME.to_owned(),
            appearance: serde_json::json!({"face": 3}),
        }
    }

    #[tokio::test]
    async fn names_are_unique_over_worlds_and_case() {
        let (first, other_world, other_case, after_release) = with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            let id = Uuid::new_v4();
            let first = reserve_character_name(pool, NAME, Uuid::new_v4(), id).await;
            let other_world =
                reserve_character_name(pool, NAME, Uuid::new_v4(), Uuid::new_v4()).await;
            let other_case =
                reserve_character_name(pool, "ADA", Uuid::new_v4(), Uuid::new_v4()).await;
            release_character_name(pool, id).await.expect("release");
            let after_release =
                reserve_character_name(pool, NAME, Uuid::new_v4(), Uuid::new_v4()).await;
            (first, other_world, other_case, after_release)
        })
        .await;
        assert!(first.is_ok());
        assert!(matches!(other_world, Err(DbError::DuplicateCharacterName)));
        assert!(matches!(other_case, Err(DbError::DuplicateCharacterName)));
        assert!(after_release.is_ok());
    }

    #[tokio::test]
    async fn characters_are_listed_per_account() {
        let (inserted, listed, other, loaded) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let account = Uuid::new_v4();
            let inserted = insert_character(pool, &new_character(account))
                .await
                .expect("insert");
            let listed = characters_for_account(pool, account).await.expect("list");
            let other = characters_for_account(pool, Uuid::new_v4())
                .await
                .expect("other");
            let loaded = character_by_id(pool, inserted.id).await.expect("load");
            (inserted, listed, other, loaded)
        })
        .await;
        assert_eq!(inserted.name, NAME);
        assert_eq!(inserted.appearance, serde_json::json!({"face": 3}));
        assert_eq!(listed, vec![inserted.clone()]);
        assert!(other.is_empty());
        assert_eq!(loaded, Some(inserted));
    }
}
