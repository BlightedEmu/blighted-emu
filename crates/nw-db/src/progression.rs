// SPDX-License-Identifier: AGPL-3.0-or-later
//! The progression of a character besides its items and quests (world migrations 0006 and
//! 0007): its experience, coins, achievement bits, loot limit counts and weapon masteries. `sim`
//! saves it; the director loads it at a join.
//!
//! Each save carries the time its state was taken; an older state never overwrites a newer one.

use std::collections::BTreeMap;

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;
use crate::position::SaveOutcome;

/// Milliseconds per second, for the stored time.
const MILLIS_PER_SECOND: f64 = 1000.0;
/// The column of the achievement bits.
const ACHIEVEMENTS: &str = "achievements";

/// One loot limit's count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedLootLimit {
    /// The lowercase `LootLimitID`.
    pub limit_id: String,
    /// The count.
    pub count: u64,
    /// Unix nanoseconds of its last count.
    pub counted_ns: u64,
    /// Unix nanoseconds when it goes back to 0.
    pub ends_ns: u64,
}

/// A character's progression.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedProgression {
    /// Experience points.
    pub experience: u32,
    /// Coins.
    pub coins: u64,
    /// The set achievement bits, ascending.
    pub achievements: Vec<u32>,
    /// The loot limit counts, by limit id.
    pub loot_limits: Vec<SavedLootLimit>,
    /// The total experience of each weapon mastery, by its `CategoricalProgressionID`.
    pub masteries: BTreeMap<String, u32>,
}

type ProgressionRow = (bool, i64, i64, Vec<i32>);
type LootLimitRow = (String, i64, i64, i64);
type MasteryRow = (String, i64);

/// A stored value as the program's type.
fn from_column<T: TryFrom<i64>>(value: i64, field: &'static str) -> Result<T, DbError> {
    T::try_from(value).map_err(|_| DbError::OutOfRange { field })
}

/// A program value as its column.
fn to_column<T: TryInto<i64>>(value: T, field: &'static str) -> Result<i64, DbError> {
    value.try_into().map_err(|_| DbError::OutOfRange { field })
}

/// The saved progression of a character: `Ok(None)` when it exists and never saved one.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character, and
/// [`DbError::OutOfRange`] for a stored value the program's types do not hold.
pub async fn character_progression(
    world: &PgPool,
    character_id: Uuid,
) -> Result<Option<SavedProgression>, DbError> {
    let row: Option<ProgressionRow> = sqlx::query_as(
        "SELECT progression_saved_at IS NOT NULL, experience, coins, achievements \
         FROM player_character WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    let (ever_saved, experience, coins, achievements) = row.ok_or(DbError::NoSuchCharacter)?;
    if !ever_saved {
        return Ok(None);
    }
    let limits: Vec<LootLimitRow> = sqlx::query_as(
        "SELECT limit_id, count, counted_ns, ends_ns FROM character_loot_limit \
         WHERE character_id = $1 ORDER BY limit_id",
    )
    .bind(character_id)
    .fetch_all(world)
    .await?;
    let masteries: Vec<MasteryRow> = sqlx::query_as(
        "SELECT category, experience FROM character_mastery WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_all(world)
    .await?;
    Ok(Some(SavedProgression {
        experience: from_column(experience, "experience")?,
        coins: from_column(coins, "coins")?,
        achievements: achievements
            .into_iter()
            .map(|bit| from_column(i64::from(bit), ACHIEVEMENTS))
            .collect::<Result<_, _>>()?,
        loot_limits: limits
            .into_iter()
            .map(|(limit_id, count, counted_ns, ends_ns)| {
                Ok(SavedLootLimit {
                    limit_id,
                    count: from_column(count, "count")?,
                    counted_ns: from_column(counted_ns, "counted_ns")?,
                    ends_ns: from_column(ends_ns, "ends_ns")?,
                })
            })
            .collect::<Result<_, DbError>>()?,
        masteries: masteries
            .into_iter()
            .map(|(category, experience)| Ok((category, from_column(experience, "mastery")?)))
            .collect::<Result<_, DbError>>()?,
    }))
}

/// Saves `progression` as the whole progression of a character, taken at `taken_at_ms` (Unix
/// ms), when no state taken at the same time or later is stored.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character,
/// [`DbError::OutOfRange`] for a value its column cannot hold, and [`DbError::Database`] for a
/// row a table constraint refuses (two counts of one limit, an id that is not lowercase).
pub async fn save_character_progression(
    world: &PgPool,
    character_id: Uuid,
    progression: &SavedProgression,
    taken_at_ms: u64,
) -> Result<SaveOutcome, DbError> {
    let coins = to_column(progression.coins, "coins")?;
    let achievements = progression
        .achievements
        .iter()
        .map(|bit| {
            i32::try_from(*bit).map_err(|_| DbError::OutOfRange {
                field: ACHIEVEMENTS,
            })
        })
        .collect::<Result<Vec<i32>, _>>()?;
    let limits = progression
        .loot_limits
        .iter()
        .map(|limit| {
            Ok((
                limit.limit_id.as_str(),
                to_column(limit.count, "count")?,
                to_column(limit.counted_ns, "counted_ns")?,
                to_column(limit.ends_ns, "ends_ns")?,
            ))
        })
        .collect::<Result<Vec<_>, DbError>>()?;
    let taken_at_seconds = taken_at_ms as f64 / MILLIS_PER_SECOND;
    let mut transaction = world.begin().await?;
    let updated = sqlx::query(
        "UPDATE player_character SET progression_saved_at = to_timestamp($2), \
         experience = $3, coins = $4, achievements = $5 \
         WHERE character_id = $1 \
         AND (progression_saved_at IS NULL OR progression_saved_at < to_timestamp($2))",
    )
    .bind(character_id)
    .bind(taken_at_seconds)
    .bind(i64::from(progression.experience))
    .bind(coins)
    .bind(&achievements)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() == 0 {
        let exists: Option<(i32,)> =
            sqlx::query_as("SELECT 1 FROM player_character WHERE character_id = $1")
                .bind(character_id)
                .fetch_optional(&mut *transaction)
                .await?;
        return exists
            .map(|_| SaveOutcome::Older)
            .ok_or(DbError::NoSuchCharacter);
    }
    sqlx::query("DELETE FROM character_loot_limit WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut *transaction)
        .await?;
    for (limit_id, count, counted_ns, ends_ns) in limits {
        sqlx::query(
            "INSERT INTO character_loot_limit (character_id, limit_id, count, counted_ns, ends_ns) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(character_id)
        .bind(limit_id)
        .bind(count)
        .bind(counted_ns)
        .bind(ends_ns)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query("DELETE FROM character_mastery WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut *transaction)
        .await?;
    for (category, experience) in &progression.masteries {
        sqlx::query(
            "INSERT INTO character_mastery (character_id, category, experience) \
             VALUES ($1, $2, $3)",
        )
        .bind(character_id)
        .bind(category)
        .bind(i64::from(*experience))
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(SaveOutcome::Saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_out_of_a_column_range_are_refused() {
        assert_eq!(to_column(u64::MAX, "coins").ok(), None);
        assert_eq!(to_column(375_u64, "coins").ok(), Some(375));
        assert!(matches!(
            from_column::<u32>(i64::from(u32::MAX) + 1, "experience"),
            Err(DbError::OutOfRange {
                field: "experience"
            })
        ));
        assert_eq!(from_column::<u64>(1, "count").ok(), Some(1));
    }
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::test_db::with_test_schema;
    use crate::{NewCharacter, insert_character};

    /// 2027-01-01, with milliseconds.
    const TAKEN_AT_MS: u64 = 1_798_761_600_123;
    const LATER_MS: u64 = 1_000;

    /// The tutorial's experience and achievements, coins of a quest reward, and the retail
    /// newchar loot limit count.
    fn progression() -> SavedProgression {
        SavedProgression {
            experience: 1_250,
            coins: 375,
            achievements: vec![14_809, 15_317],
            loot_limits: vec![
                SavedLootLimit {
                    limit_id: "2hwarhammer_ftue_corruptedsailer_v2".to_owned(),
                    count: 1,
                    counted_ns: 1_793_091_600_894_621_000,
                    ends_ns: 1_793_091_600_894_621_000,
                },
                SavedLootLimit {
                    limit_id: "gypsumgreen".to_owned(),
                    count: 32,
                    counted_ns: 1_792_696_951_894_621_000,
                    ends_ns: 1_792_746_000_894_621_000,
                },
            ],
            masteries: BTreeMap::from([
                ("GreatSwordAbilityTable".to_owned(), 20),
                ("BlunderbussAbilityTable".to_owned(), 4),
            ]),
        }
    }

    async fn new_character(pool: &PgPool) -> Uuid {
        insert_character(
            pool,
            &NewCharacter {
                id: Uuid::new_v4(),
                account_id: Uuid::new_v4(),
                name: "Ada".to_owned(),
                appearance: serde_json::json!({}),
            },
        )
        .await
        .expect("insert")
        .id
    }

    #[tokio::test]
    async fn progression_round_trips_and_an_older_state_loses() {
        let (before, after, older, cleared, missing, too_rich) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let character = new_character(pool).await;
            let before = character_progression(pool, character).await.expect("load");
            save_character_progression(pool, character, &progression(), TAKEN_AT_MS)
                .await
                .expect("save");
            let after = character_progression(pool, character).await.expect("load");
            let older = save_character_progression(
                pool,
                character,
                &SavedProgression::default(),
                TAKEN_AT_MS,
            )
            .await
            .expect("save");
            save_character_progression(
                pool,
                character,
                &SavedProgression::default(),
                TAKEN_AT_MS + LATER_MS,
            )
            .await
            .expect("save");
            let cleared = character_progression(pool, character).await.expect("load");
            let missing = character_progression(pool, Uuid::new_v4()).await;
            let too_rich = save_character_progression(
                pool,
                character,
                &SavedProgression {
                    coins: u64::MAX,
                    ..progression()
                },
                TAKEN_AT_MS + LATER_MS + LATER_MS,
            )
            .await;
            (before, after, older, cleared, missing, too_rich)
        })
        .await;
        assert_eq!(before, None, "a new character has saved no progression");
        assert_eq!(after, Some(progression()));
        assert_eq!(older, SaveOutcome::Older);
        assert_eq!(cleared, Some(SavedProgression::default()));
        assert!(matches!(missing, Err(DbError::NoSuchCharacter)));
        assert!(matches!(
            too_rich,
            Err(DbError::OutOfRange { field: "coins" })
        ));
    }

    #[tokio::test]
    async fn the_migration_adds_the_columns_with_their_checks() {
        let (defaults, negative, upper_id) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let character = new_character(pool).await;
            let defaults: (Option<f64>, i64, i64, Vec<i32>) = sqlx::query_as(
                "SELECT extract(epoch FROM progression_saved_at)::float8, experience, coins, \
                 achievements FROM player_character WHERE character_id = $1",
            )
            .bind(character)
            .fetch_one(pool)
            .await
            .expect("row");
            let negative = sqlx::query("UPDATE player_character SET coins = -1")
                .execute(pool)
                .await;
            let upper_id =
                sqlx::query("INSERT INTO character_loot_limit VALUES ($1, 'GypsumGreen', 1, 0, 0)")
                    .bind(character)
                    .execute(pool)
                    .await;
            (defaults, negative, upper_id)
        })
        .await;
        assert_eq!(defaults, (None, 0, 0, Vec::new()));
        assert!(negative.is_err(), "coins are never negative");
        assert!(upper_id.is_err(), "limit ids are stored lowercase");
    }
}
