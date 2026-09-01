// SPDX-License-Identifier: AGPL-3.0-or-later
//! Character rows in the global PostgreSQL database.

use sqlx::PgPool;
use uuid::Uuid;

use crate::Character;
use crate::error::{DbError, database};
use crate::limits::{MAX_CHARACTER_NAME_LEN, MAX_WORLD_ID_LEN};
use crate::migrate::{is_foreign_key_violation, is_unique_violation};

const NAME_CONSTRAINT: &str = "player_character_world_name_unique";
const ACCOUNT_FK: &str = "player_character_account_fk";

/// Inserts a character for an account that already exists.
pub async fn create_character(
    pool: &PgPool,
    account_id: &str,
    world_id: &str,
    name: &str,
) -> Result<Character, DbError> {
    let world_id = check_world_id(world_id)?;
    let name = check_name(name)?;
    let id = Uuid::new_v4().to_string();
    let inserted = insert_character(pool, &id, account_id, &world_id, &name).await;
    match inserted {
        Ok(character) => Ok(character),
        Err(error) if is_unique_violation(&error, NAME_CONSTRAINT) => {
            Err(DbError::DuplicateCharacterName)
        }
        Err(error) if is_foreign_key_violation(&error, ACCOUNT_FK) => Err(DbError::UnknownAccount),
        Err(error) => Err(error),
    }
}

/// Loads one character by id.
pub async fn character_by_id(
    pool: &PgPool,
    character_id: &str,
) -> Result<Option<Character>, DbError> {
    let row = fetch_optional(
        pool,
        "SELECT character_id, account_id, world_id, name,
                CAST(EXTRACT(EPOCH FROM created_at) AS bigint)
         FROM player_character
         WHERE character_id = $1",
        character_id,
    )
    .await?;
    Ok(row)
}

/// Lists characters for one account in one world, oldest first.
pub async fn characters_for_account(
    pool: &PgPool,
    account_id: &str,
    world_id: &str,
) -> Result<Vec<Character>, DbError> {
    let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
        "SELECT character_id, account_id, world_id, name,
                CAST(EXTRACT(EPOCH FROM created_at) AS bigint)
         FROM player_character
         WHERE account_id = $1 AND world_id = $2
         ORDER BY created_at, character_id",
    )
    .bind(account_id)
    .bind(world_id)
    .fetch_all(pool)
    .await
    .map_err(database)?;
    Ok(rows.into_iter().map(character_from_row).collect())
}

pub(crate) fn check_world_id(world_id: &str) -> Result<String, DbError> {
    if world_id.is_empty() || world_id.trim().is_empty() {
        return Err(DbError::EmptyWorldId);
    }
    if world_id.len() > MAX_WORLD_ID_LEN {
        return Err(DbError::WorldIdTooLong);
    }
    if world_id
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(DbError::WorldIdInvalid);
    }
    Ok(world_id.to_owned())
}

pub(crate) fn check_name(name: &str) -> Result<String, DbError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(DbError::EmptyCharacterName);
    }
    if name.len() > MAX_CHARACTER_NAME_LEN {
        return Err(DbError::CharacterNameTooLong);
    }
    if name.chars().any(char::is_control) {
        return Err(DbError::CharacterNameInvalid);
    }
    Ok(name.to_owned())
}

async fn insert_character(
    pool: &PgPool,
    character_id: &str,
    account_id: &str,
    world_id: &str,
    name: &str,
) -> Result<Character, DbError> {
    let row: (String, String, String, String, i64) = sqlx::query_as(
        "INSERT INTO player_character (character_id, account_id, world_id, name)
         VALUES ($1, $2, $3, $4)
         RETURNING character_id, account_id, world_id, name,
                   CAST(EXTRACT(EPOCH FROM created_at) AS bigint)",
    )
    .bind(character_id)
    .bind(account_id)
    .bind(world_id)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(database)?;
    Ok(character_from_row(row))
}

async fn fetch_optional(
    pool: &PgPool,
    sql: &'static str,
    character_id: &str,
) -> Result<Option<Character>, DbError> {
    let row: Option<(String, String, String, String, i64)> = sqlx::query_as(sql)
        .bind(character_id)
        .fetch_optional(pool)
        .await
        .map_err(database)?;
    Ok(row.map(character_from_row))
}

fn character_from_row(row: (String, String, String, String, i64)) -> Character {
    Character {
        id: row.0,
        account_id: row.1,
        world_id: row.2,
        name: row.3,
        created_at_unix: row.4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::create_account;
    use crate::migrate::migrate;
    use crate::test_db::with_test_schema;

    const USERNAME: &str = "ada";
    const PASSWORD: &str = "password-ok";
    const WORLD_ID: &str = "world-1";
    const OTHER_WORLD: &str = "world-2";
    const NAME: &str = "Ada";

    #[test]
    fn rejects_blank_name_and_world() {
        assert!(matches!(
            check_name("   "),
            Err(DbError::EmptyCharacterName)
        ));
        assert!(matches!(check_world_id(""), Err(DbError::EmptyWorldId)));
        assert!(matches!(
            check_world_id("world 1"),
            Err(DbError::WorldIdInvalid)
        ));
    }

    #[tokio::test]
    async fn stores_a_character_and_lists_it() {
        let found = with_test_schema(async |pool| {
            migrate(pool).await.expect("migrate");
            let account = create_account(pool, USERNAME, PASSWORD)
                .await
                .expect("account");
            let character = create_character(pool, &account.id, WORLD_ID, "  Ada  ")
                .await
                .expect("character");
            let loaded = character_by_id(pool, &character.id)
                .await
                .expect("load")
                .expect("row");
            let listed = characters_for_account(pool, &account.id, WORLD_ID)
                .await
                .expect("list");
            let missing = character_by_id(pool, "missing").await.expect("missing");
            (character, loaded, listed, missing)
        })
        .await;
        let (character, loaded, listed, missing) = found;
        assert_eq!(loaded, character);
        assert_eq!(character.name, NAME);
        assert_eq!(listed, vec![character]);
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn name_is_unique_per_world_and_account_must_exist() {
        let errors = with_test_schema(async |pool| {
            migrate(pool).await.expect("migrate");
            let account = create_account(pool, USERNAME, PASSWORD)
                .await
                .expect("account");
            create_character(pool, &account.id, WORLD_ID, NAME)
                .await
                .expect("first");
            let duplicate = create_character(pool, &account.id, WORLD_ID, NAME).await;
            let other_world = create_character(pool, &account.id, OTHER_WORLD, NAME).await;
            let unknown = create_character(pool, "missing-account", WORLD_ID, "Bea").await;
            (duplicate, other_world, unknown)
        })
        .await;
        assert!(matches!(errors.0, Err(DbError::DuplicateCharacterName)));
        assert!(errors.1.is_ok());
        assert!(matches!(errors.2, Err(DbError::UnknownAccount)));
    }
}
