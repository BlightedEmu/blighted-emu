// SPDX-License-Identifier: AGPL-3.0-or-later
//! The saved items of a character (admin studio spec section 6.1). `sim` saves them; the
//! director loads them at a join.
//!
//! Each save carries the time its item list was taken. The rows change only when that time is
//! newer than the stored one, so a late save of one `sim` never overwrites a newer list of
//! another.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;
use crate::position::SaveOutcome;

/// Milliseconds per second, for the stored time.
const MILLIS_PER_SECOND: f64 = 1000.0;
/// `character_item.place` of a worn item.
const PLACE_WORN: i16 = 0;
/// `character_item.place` of a bag item.
const PLACE_BAG: i16 = 1;

/// Where a saved item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedPlace {
    /// Worn in this paperdoll slot.
    Worn(u32),
    /// In this bag slot.
    Bag(u16),
}

/// One saved item stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedItem {
    /// The item id (`MasterItemDefinitions` `ItemID`).
    pub item_id: String,
    /// Stack size, 1 or more.
    pub quantity: u32,
    /// Where it is.
    pub place: SavedPlace,
}

impl SavedPlace {
    fn columns(self) -> (i16, i64) {
        match self {
            Self::Worn(slot) => (PLACE_WORN, i64::from(slot)),
            Self::Bag(slot) => (PLACE_BAG, i64::from(slot)),
        }
    }

    fn of_columns(place: i16, slot: i32) -> Option<Self> {
        match place {
            PLACE_WORN => Some(Self::Worn(u32::try_from(slot).ok()?)),
            PLACE_BAG => Some(Self::Bag(u16::try_from(slot).ok()?)),
            _ => None,
        }
    }
}

/// The saved items of a character in place order: `Ok(None)` when the character exists and
/// never saved items. A row the program cannot read (a slot out of range) is left out.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character.
pub async fn character_items(
    world: &PgPool,
    character_id: Uuid,
) -> Result<Option<Vec<SavedItem>>, DbError> {
    let saved: Option<(bool,)> = sqlx::query_as(
        "SELECT items_saved_at IS NOT NULL FROM player_character WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    let (ever_saved,) = saved.ok_or(DbError::NoSuchCharacter)?;
    if !ever_saved {
        return Ok(None);
    }
    let rows: Vec<(i16, i32, String, i32)> = sqlx::query_as(
        "SELECT place, slot, item_id, quantity FROM character_item WHERE character_id = $1 \
         ORDER BY place, slot",
    )
    .bind(character_id)
    .fetch_all(world)
    .await?;
    Ok(Some(
        rows.into_iter()
            .filter_map(|(place, slot, item_id, quantity)| {
                Some(SavedItem {
                    item_id,
                    quantity: u32::try_from(quantity).ok()?,
                    place: SavedPlace::of_columns(place, slot)?,
                })
            })
            .collect(),
    ))
}

/// Saves `items` as the whole item list of a character, taken at `taken_at_ms` (Unix ms), when
/// no list taken at the same time or later is stored.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character, and
/// [`DbError::Database`] for a row a table constraint refuses (a quantity of 0, two items in
/// one place).
pub async fn save_character_items(
    world: &PgPool,
    character_id: Uuid,
    items: &[SavedItem],
    taken_at_ms: u64,
) -> Result<SaveOutcome, DbError> {
    let taken_at_seconds = taken_at_ms as f64 / MILLIS_PER_SECOND;
    let mut transaction = world.begin().await?;
    let updated = sqlx::query(
        "UPDATE player_character SET items_saved_at = to_timestamp($2) WHERE character_id = $1 \
         AND (items_saved_at IS NULL OR items_saved_at < to_timestamp($2))",
    )
    .bind(character_id)
    .bind(taken_at_seconds)
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
    sqlx::query("DELETE FROM character_item WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut *transaction)
        .await?;
    for item in items {
        let (place, slot) = item.place.columns();
        sqlx::query(
            "INSERT INTO character_item (character_id, place, slot, item_id, quantity) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(character_id)
        .bind(place)
        .bind(slot)
        .bind(&item.item_id)
        .bind(i64::from(item.quantity))
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(SaveOutcome::Saved)
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::test_db::with_test_schema;
    use crate::{NewCharacter, insert_character};

    /// 2027-01-01, with milliseconds.
    const TAKEN_AT_MS: u64 = 1_798_761_600_123;
    const LATER_MS: u64 = 1_000;
    const SWORD: &str = "1hSwordT2_FTUE";
    const POTION: &str = "PotionHealthT1";
    const MAIN_HAND: u32 = 3;
    const POTIONS: u32 = 5;

    fn kit() -> Vec<SavedItem> {
        vec![
            SavedItem {
                item_id: SWORD.to_owned(),
                quantity: 1,
                place: SavedPlace::Worn(MAIN_HAND),
            },
            SavedItem {
                item_id: POTION.to_owned(),
                quantity: POTIONS,
                place: SavedPlace::Bag(0),
            },
        ]
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
    async fn items_are_none_until_saved_and_then_load_in_place_order() {
        let (before, saved, after, missing) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let character = new_character(pool).await;
            let before = character_items(pool, character).await.expect("load");
            let saved = save_character_items(pool, character, &kit(), TAKEN_AT_MS)
                .await
                .expect("save");
            let after = character_items(pool, character).await.expect("load");
            let missing = character_items(pool, Uuid::new_v4()).await;
            (before, saved, after, missing)
        })
        .await;
        assert_eq!(before, None);
        assert_eq!(saved, SaveOutcome::Saved);
        assert_eq!(after, Some(kit()));
        assert!(matches!(missing, Err(DbError::NoSuchCharacter)));
    }

    #[tokio::test]
    async fn an_older_list_loses_and_an_empty_bag_stays_empty() {
        let (older, emptied, after, missing_save) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let character = new_character(pool).await;
            save_character_items(pool, character, &kit(), TAKEN_AT_MS)
                .await
                .expect("save");
            let older = save_character_items(pool, character, &[], TAKEN_AT_MS)
                .await
                .expect("save");
            let emptied = save_character_items(pool, character, &[], TAKEN_AT_MS + LATER_MS)
                .await
                .expect("save");
            let after = character_items(pool, character).await.expect("load");
            let missing_save =
                save_character_items(pool, Uuid::new_v4(), &kit(), TAKEN_AT_MS).await;
            (older, emptied, after, missing_save)
        })
        .await;
        assert_eq!(older, SaveOutcome::Older);
        assert_eq!(emptied, SaveOutcome::Saved);
        assert_eq!(after, Some(Vec::new()));
        assert!(matches!(missing_save, Err(DbError::NoSuchCharacter)));
    }
}
