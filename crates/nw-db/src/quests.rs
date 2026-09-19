// SPDX-License-Identifier: AGPL-3.0-or-later
//! The quests of a character (admin studio spec section 6.3): the active run and the finished
//! objectives. `sim` saves them; the director loads them at a join. The run is the sim's own
//! value, kept as JSON.
//!
//! Each save carries the time its state was taken; an older state never overwrites a newer one.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;
use crate::position::SaveOutcome;

/// Milliseconds per second, for the stored time.
const MILLIS_PER_SECOND: f64 = 1000.0;
const ACTIVE: &str = "active";
const DONE: &str = "done";

/// A character's quests.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SavedQuests {
    /// The active objective and its run.
    pub active: Option<(String, serde_json::Value)>,
    /// The finished objectives, sorted.
    pub done: Vec<String>,
}

/// The saved quests of a character: `Ok(None)` when it exists and never saved quests.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character.
pub async fn character_quests(
    world: &PgPool,
    character_id: Uuid,
) -> Result<Option<SavedQuests>, DbError> {
    let saved: Option<(bool,)> = sqlx::query_as(
        "SELECT quests_saved_at IS NOT NULL FROM player_character WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    let (ever_saved,) = saved.ok_or(DbError::NoSuchCharacter)?;
    if !ever_saved {
        return Ok(None);
    }
    let rows: Vec<(String, String, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT objective_id, state, run FROM character_quest WHERE character_id = $1 \
         ORDER BY objective_id",
    )
    .bind(character_id)
    .fetch_all(world)
    .await?;
    let mut quests = SavedQuests::default();
    for (objective, state, run) in rows {
        match (state.as_str(), run) {
            (ACTIVE, Some(run)) => quests.active = Some((objective, run)),
            _ => quests.done.push(objective),
        }
    }
    Ok(Some(quests))
}

/// Saves `quests` as the whole quest state of a character, taken at `taken_at_ms` (Unix ms),
/// when no state taken at the same time or later is stored.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character, and
/// [`DbError::Database`] for a state a table constraint refuses (the active objective also in
/// `done`).
pub async fn save_character_quests(
    world: &PgPool,
    character_id: Uuid,
    quests: &SavedQuests,
    taken_at_ms: u64,
) -> Result<SaveOutcome, DbError> {
    let taken_at_seconds = taken_at_ms as f64 / MILLIS_PER_SECOND;
    let mut transaction = world.begin().await?;
    let updated = sqlx::query(
        "UPDATE player_character SET quests_saved_at = to_timestamp($2) \
         WHERE character_id = $1 \
         AND (quests_saved_at IS NULL OR quests_saved_at < to_timestamp($2))",
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
    sqlx::query("DELETE FROM character_quest WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut *transaction)
        .await?;
    let rows = quests
        .active
        .iter()
        .map(|(objective, run)| (objective, ACTIVE, Some(run)))
        .chain(quests.done.iter().map(|objective| (objective, DONE, None)));
    for (objective, state, run) in rows {
        sqlx::query(
            "INSERT INTO character_quest (character_id, objective_id, state, run) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(character_id)
        .bind(objective)
        .bind(state)
        .bind(run)
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

    fn quests() -> SavedQuests {
        SavedQuests {
            active: Some((
                "13_Wolves".to_owned(),
                serde_json::json!({"tasks": ["Idle"], "listed": [], "tracker_version": 4}),
            )),
            done: vec!["01_Tutorial".to_owned(), "12_GetOffBeach".to_owned()],
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
    async fn quests_round_trip_and_an_older_state_loses() {
        let (before, after, older, cleared, missing) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let character = new_character(pool).await;
            let before = character_quests(pool, character).await.expect("load");
            save_character_quests(pool, character, &quests(), TAKEN_AT_MS)
                .await
                .expect("save");
            let after = character_quests(pool, character).await.expect("load");
            let older =
                save_character_quests(pool, character, &SavedQuests::default(), TAKEN_AT_MS)
                    .await
                    .expect("save");
            save_character_quests(
                pool,
                character,
                &SavedQuests::default(),
                TAKEN_AT_MS + LATER_MS,
            )
            .await
            .expect("save");
            let cleared = character_quests(pool, character).await.expect("load");
            let missing = character_quests(pool, Uuid::new_v4()).await;
            (before, after, older, cleared, missing)
        })
        .await;
        assert_eq!(before, None);
        assert_eq!(after, Some(quests()));
        assert_eq!(older, SaveOutcome::Older);
        assert_eq!(cleared, Some(SavedQuests::default()));
        assert!(matches!(missing, Err(DbError::NoSuchCharacter)));
    }
}
