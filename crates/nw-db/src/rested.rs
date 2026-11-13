// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rested experience of a character (world migration 0048). It is part of the progression
//! ([`crate::SavedProgression`]): it loads and saves with it, in the same transaction.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};

/// A character's rested experience pool and when it was saved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SavedRested {
    /// The pool: the bonus experience the next gains take from it.
    pub experience: u32,
    /// Unix nanoseconds of the save; 0 for a character that never saved one (no time away).
    pub saved_ns: u64,
}

/// The rested experience of a character that exists.
pub(crate) async fn character_rested(
    world: &PgPool,
    character_id: Uuid,
) -> Result<SavedRested, DbError> {
    let row: Option<(i64, i64)> = sqlx::query_as(
        "SELECT rested_experience, rested_saved_ns FROM player_character \
         WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    let (experience, saved_ns) = row.ok_or(DbError::NoSuchCharacter)?;
    Ok(SavedRested {
        experience: from_column(experience, "rested_experience")?,
        saved_ns: from_column(saved_ns, "rested_saved_ns")?,
    })
}

/// Saves `rested` as the rested experience of a character whose progression row the
/// transaction already updated.
pub(crate) async fn save_character_rested(
    transaction: &mut Transaction<'_, Postgres>,
    character_id: Uuid,
    rested: &SavedRested,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE player_character SET rested_experience = $2, rested_saved_ns = $3 \
         WHERE character_id = $1",
    )
    .bind(character_id)
    .bind(i64::from(rested.experience))
    .bind(to_column(rested.saved_ns, "rested_saved_ns")?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
