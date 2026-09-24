// SPDX-License-Identifier: AGPL-3.0-or-later
//! The musical performance lockout of a character (world migration 0018): when it may create a
//! performance again (the client's `MusicalPerformancePlayerPersistence`
//! `m_nextPerformanceStartTime`). It is part of the progression ([`crate::SavedProgression`]): it
//! loads and saves with it, in the same transaction.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};

/// A character's performance lockout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SavedMusic {
    /// Server time (ms since 2000-01-01) from which it may create a performance again; `None`
    /// for no lockout.
    pub next_performance_ms: Option<u64>,
}

/// The performance lockout of a character that exists.
pub(crate) async fn character_music(
    world: &PgPool,
    character_id: Uuid,
) -> Result<SavedMusic, DbError> {
    let row: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT next_performance_ms FROM player_character WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    let (next,) = row.ok_or(DbError::NoSuchCharacter)?;
    Ok(SavedMusic {
        next_performance_ms: next
            .map(|ms| from_column(ms, "next_performance_ms"))
            .transpose()?,
    })
}

/// Saves `music` as the performance lockout of a character whose progression row the
/// transaction already updated.
pub(crate) async fn save_character_music(
    transaction: &mut Transaction<'_, Postgres>,
    character_id: Uuid,
    music: &SavedMusic,
) -> Result<(), DbError> {
    let next = music
        .next_performance_ms
        .map(|ms| to_column(ms, "next_performance_ms"))
        .transpose()?;
    sqlx::query("UPDATE player_character SET next_performance_ms = $2 WHERE character_id = $1")
        .bind(character_id)
        .bind(next)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}
