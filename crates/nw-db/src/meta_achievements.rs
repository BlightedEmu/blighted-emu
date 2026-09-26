// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mapped ability slots of a character's weapon trees, its meta achievements and the title
//! it shows (world migration 0013). They belong to the progression ([`crate::SavedProgression`]): loaded and
//! saved with it, in its transaction.

use std::collections::BTreeMap;

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};

/// One mapped slot: the weapon mastery of the tree, the slot (0 to 2) and the ability's
/// `ProgressionPointId`.
pub type SavedSlot = (String, u8, String);

/// A character's mapped slots and meta achievements.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SavedMeta {
    /// The mapped slots, by tree and slot.
    pub ability_slots: Vec<SavedSlot>,
    /// The meta achievement counters, by lowercase key.
    pub counters: BTreeMap<String, u64>,
    /// The completed meta achievements in completion order, each with whether its reward
    /// waits.
    pub completed: Vec<(String, bool)>,
    /// The title it shows.
    pub title: Option<u32>,
}

type SlotRow = (String, i16, String);
type CounterRow = (String, i64);
type CompletedRow = (String, bool);

/// The mapped slots and meta achievements of `character_id`.
pub(crate) async fn character_meta(
    world: &PgPool,
    character_id: Uuid,
) -> Result<SavedMeta, DbError> {
    let slots: Vec<SlotRow> = sqlx::query_as(
        "SELECT mastery, slot, ability FROM character_ability_slot WHERE character_id = $1 \
         ORDER BY mastery, slot",
    )
    .bind(character_id)
    .fetch_all(world)
    .await?;
    let counters: Vec<CounterRow> =
        sqlx::query_as("SELECT counter, value FROM character_meta_counter WHERE character_id = $1")
            .bind(character_id)
            .fetch_all(world)
            .await?;
    let title: Option<i64> =
        sqlx::query_scalar("SELECT shown_title FROM player_character WHERE character_id = $1")
            .bind(character_id)
            .fetch_optional(world)
            .await?
            .flatten();
    let completed: Vec<CompletedRow> = sqlx::query_as(
        "SELECT meta_achievement, pending FROM character_meta_achievement \
         WHERE character_id = $1 ORDER BY position",
    )
    .bind(character_id)
    .fetch_all(world)
    .await?;
    Ok(SavedMeta {
        ability_slots: slots
            .into_iter()
            .map(|(mastery, slot, ability)| {
                Ok((
                    mastery,
                    from_column(i64::from(slot), "ability slot")?,
                    ability,
                ))
            })
            .collect::<Result<_, DbError>>()?,
        counters: counters
            .into_iter()
            .map(|(counter, value)| Ok((counter, from_column(value, "meta counter")?)))
            .collect::<Result<_, DbError>>()?,
        completed,
        title: title
            .map(|title| from_column(title, "shown title"))
            .transpose()?,
    })
}

/// Replaces the mapped slots and meta achievements of `character_id` with `meta`, in
/// `transaction`.
pub(crate) async fn save_character_meta(
    transaction: &mut Transaction<'_, Postgres>,
    character_id: Uuid,
    meta: &SavedMeta,
) -> Result<(), DbError> {
    sqlx::query("UPDATE player_character SET shown_title = $2 WHERE character_id = $1")
        .bind(character_id)
        .bind(meta.title.map(i64::from))
        .execute(&mut **transaction)
        .await?;
    sqlx::query("DELETE FROM character_ability_slot WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut **transaction)
        .await?;
    for (mastery, slot, ability) in &meta.ability_slots {
        sqlx::query(
            "INSERT INTO character_ability_slot (character_id, mastery, slot, ability) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(character_id)
        .bind(mastery)
        .bind(i16::from(*slot))
        .bind(ability)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query("DELETE FROM character_meta_counter WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut **transaction)
        .await?;
    for (counter, value) in &meta.counters {
        sqlx::query(
            "INSERT INTO character_meta_counter (character_id, counter, value) \
             VALUES ($1, $2, $3)",
        )
        .bind(character_id)
        .bind(counter)
        .bind(to_column(*value, "meta counter")?)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query("DELETE FROM character_meta_achievement WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut **transaction)
        .await?;
    for (position, (meta_achievement, pending)) in meta.completed.iter().enumerate() {
        sqlx::query(
            "INSERT INTO character_meta_achievement \
             (character_id, meta_achievement, position, pending) VALUES ($1, $2, $3, $4)",
        )
        .bind(character_id)
        .bind(meta_achievement)
        .bind(i32::try_from(position).map_err(|_| DbError::OutOfRange {
            field: "meta achievement position",
        })?)
        .bind(*pending)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}
