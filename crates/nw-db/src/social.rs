// SPDX-License-Identifier: AGPL-3.0-or-later
//! What other players and the title list need (world migration 0009): the earned titles of an
//! account with their new marks, and the stored data of characters by id.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;

/// One earned title of an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountTitle {
    /// The CRC-32 of the lowercase `TitleID`.
    pub title: u32,
    /// The client still marks it new.
    pub is_new: bool,
}

/// The stored data of one character, for other players.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredPlayer {
    /// The character id.
    pub id: Uuid,
    /// The name.
    pub name: String,
    /// The client's create request body.
    pub appearance: serde_json::Value,
    /// The saved experience points.
    pub experience: u32,
}

type TitleRow = (i64, bool);
type PlayerRow = (Uuid, String, serde_json::Value, i64);

/// The column of a title id.
const TITLE_FIELD: &str = "title_id";
/// The column of the experience.
const EXPERIENCE_FIELD: &str = "experience";

/// Stores `earned` as titles of the account of `character` (new when the account did not have
/// them), then returns every title of that account, ascending by id.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character, and
/// [`DbError::Database`] when PostgreSQL fails.
pub async fn earn_account_titles(
    world: &PgPool,
    character: Uuid,
    earned: &[u32],
) -> Result<Vec<AccountTitle>, DbError> {
    let mut transaction = world.begin().await?;
    let account: Uuid =
        sqlx::query_scalar("SELECT account_id FROM player_character WHERE character_id = $1")
            .bind(character)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(DbError::NoSuchCharacter)?;
    let ids: Vec<i64> = earned.iter().map(|title| i64::from(*title)).collect();
    sqlx::query(
        "INSERT INTO account_title (account_id, title_id) SELECT $1, UNNEST($2::BIGINT[]) \
         ON CONFLICT (account_id, title_id) DO NOTHING",
    )
    .bind(account)
    .bind(&ids)
    .execute(&mut *transaction)
    .await?;
    let rows: Vec<TitleRow> = sqlx::query_as(
        "SELECT title_id, is_new FROM account_title WHERE account_id = $1 ORDER BY title_id",
    )
    .bind(account)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    rows.into_iter()
        .map(|(title, is_new)| {
            Ok(AccountTitle {
                title: u32::try_from(title)
                    .map_err(|_| DbError::OutOfRange { field: TITLE_FIELD })?,
                is_new,
            })
        })
        .collect()
}

/// Clears the new marks of every title of the account of `character`.
///
/// # Errors
///
/// Returns [`DbError::Database`] when PostgreSQL fails.
pub async fn clear_new_account_titles(world: &PgPool, character: Uuid) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE account_title SET is_new = false WHERE is_new AND account_id = \
         (SELECT account_id FROM player_character WHERE character_id = $1)",
    )
    .bind(character)
    .execute(world)
    .await?;
    Ok(())
}

/// The stored data of the characters of `ids` that the world has, in no set order.
///
/// # Errors
///
/// Returns [`DbError::Database`] when PostgreSQL fails, and [`DbError::OutOfRange`] for a stored
/// experience outside `u32`.
pub async fn stored_players(world: &PgPool, ids: &[Uuid]) -> Result<Vec<StoredPlayer>, DbError> {
    let rows: Vec<PlayerRow> = sqlx::query_as(
        "SELECT character_id, name, appearance, experience FROM player_character \
         WHERE character_id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(world)
    .await?;
    rows.into_iter()
        .map(|(id, name, appearance, experience)| {
            Ok(StoredPlayer {
                id,
                name,
                appearance,
                experience: u32::try_from(experience).map_err(|_| DbError::OutOfRange {
                    field: EXPERIENCE_FIELD,
                })?,
            })
        })
        .collect()
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::test_db::with_test_schema;
    use crate::{NewCharacter, insert_character};

    /// CRC-32 of `playertitle_stoic` and `playertitle_freshstart`.
    const STOIC: u32 = 0x431d_f4ea;
    const FRESH_START: u32 = 0xb187_3b49;

    async fn new_character(pool: &PgPool, account: Uuid, name: &str) -> Uuid {
        insert_character(
            pool,
            &NewCharacter {
                id: Uuid::new_v4(),
                account_id: account,
                name: name.to_owned(),
                appearance: serde_json::json!({}),
            },
        )
        .await
        .expect("insert")
        .id
    }

    #[tokio::test]
    async fn titles_belong_to_the_account_and_stay_new_until_cleared() {
        let (first, other, cleared, missing) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let account = Uuid::new_v4();
            let ada = new_character(pool, account, "Ada").await;
            let bea = new_character(pool, account, "Bea").await;
            let first = earn_account_titles(pool, ada, &[STOIC])
                .await
                .expect("earn");
            let other = earn_account_titles(pool, bea, &[FRESH_START, STOIC])
                .await
                .expect("earn");
            clear_new_account_titles(pool, ada).await.expect("clear");
            let cleared = earn_account_titles(pool, bea, &[]).await.expect("list");
            let missing = earn_account_titles(pool, Uuid::new_v4(), &[STOIC]).await;
            (first, other, cleared, missing)
        })
        .await;
        let new = |title| AccountTitle {
            title,
            is_new: true,
        };
        let seen = |title| AccountTitle {
            title,
            is_new: false,
        };
        assert_eq!(first, [new(STOIC)]);
        assert_eq!(
            other,
            [new(STOIC), new(FRESH_START)],
            "the account's, by id"
        );
        assert_eq!(cleared, [seen(STOIC), seen(FRESH_START)]);
        assert!(matches!(missing, Err(DbError::NoSuchCharacter)));
    }

    #[tokio::test]
    async fn stored_players_are_found_by_id() {
        let (found, ada) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let ada = new_character(pool, Uuid::new_v4(), "Ada").await;
            let found = stored_players(pool, &[ada, Uuid::new_v4()])
                .await
                .expect("read");
            (found, ada)
        })
        .await;
        let [player] = found.as_slice() else {
            panic!("one stored player: {found:?}");
        };
        assert_eq!((player.id, player.name.as_str()), (ada, "Ada"));
        assert_eq!(player.experience, 0);
    }
}
