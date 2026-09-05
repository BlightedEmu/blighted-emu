// SPDX-License-Identifier: AGPL-3.0-or-later
//! The saved world position of a character (M4 spec section 8, M5 spec section 10.2). `sim`
//! saves it every 10 s and at a departure; the director loads it at a join.
//!
//! Each save carries the time the position was taken. The row changes only when that time is
//! newer than the stored one, so a late save of one `sim` never overwrites a newer save of
//! another.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;

/// Milliseconds per second, for the stored time.
const MILLIS_PER_SECOND: f64 = 1000.0;

/// A saved position.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedPosition {
    /// The level, for example `newworld_vitaeeterna`.
    pub level: String,
    /// East, metres.
    pub x: f64,
    /// North, metres.
    pub y: f64,
    /// Up, metres.
    pub z: f64,
    /// Facing, radians.
    pub yaw: f32,
    /// Unix milliseconds when the position was taken.
    pub saved_at_ms: u64,
}

impl SavedPosition {
    /// East, north and up as `f32`, the precision of the simulation; world positions are below
    /// 2^24 metres.
    pub const fn metres(&self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
}

/// The result of a save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    /// The position is stored.
    Saved,
    /// A position taken at the same time or later is already stored; nothing changed.
    Older,
}

type PositionRow = (
    Option<String>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f32>,
    Option<i64>,
);

/// The saved position of a character: `Ok(None)` when the character exists and was never in the
/// world.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character.
pub async fn character_position(
    world: &PgPool,
    character_id: Uuid,
) -> Result<Option<SavedPosition>, DbError> {
    let row: Option<PositionRow> = sqlx::query_as(
        "SELECT level_name, pos_x, pos_y, pos_z, yaw, \
         (EXTRACT(EPOCH FROM position_saved_at) * 1000)::BIGINT FROM player_character \
         WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    match row.ok_or(DbError::NoSuchCharacter)? {
        (Some(level), Some(x), Some(y), Some(z), Some(yaw), Some(saved_at_ms)) => {
            Ok(Some(SavedPosition {
                level,
                x,
                y,
                z,
                yaw,
                // A stored time before 1970 cannot come from a save; it reads as 0.
                saved_at_ms: u64::try_from(saved_at_ms).unwrap_or_default(),
            }))
        }
        _ => Ok(None),
    }
}

/// Saves the position of a character when no newer position is stored.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character, and
/// [`DbError::Database`] for a value that is not finite (a table constraint refuses it).
pub async fn save_character_position(
    world: &PgPool,
    character_id: Uuid,
    position: &SavedPosition,
) -> Result<SaveOutcome, DbError> {
    // Seconds with a fraction; `to_timestamp` keeps microseconds, so whole milliseconds come
    // back unchanged.
    let saved_at_seconds = position.saved_at_ms as f64 / MILLIS_PER_SECOND;
    let updated = sqlx::query(
        "UPDATE player_character SET level_name = $2, pos_x = $3, pos_y = $4, pos_z = $5, \
         yaw = $6, position_saved_at = to_timestamp($7) WHERE character_id = $1 \
         AND (position_saved_at IS NULL OR position_saved_at < to_timestamp($7))",
    )
    .bind(character_id)
    .bind(&position.level)
    .bind(position.x)
    .bind(position.y)
    .bind(position.z)
    .bind(position.yaw)
    .bind(saved_at_seconds)
    .execute(world)
    .await?;
    if updated.rows_affected() > 0 {
        return Ok(SaveOutcome::Saved);
    }
    let exists: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM player_character WHERE character_id = $1")
            .bind(character_id)
            .fetch_optional(world)
            .await?;
    exists
        .map(|_| SaveOutcome::Older)
        .ok_or(DbError::NoSuchCharacter)
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::test_db::with_test_schema;
    use crate::{NewCharacter, insert_character};

    const LEVEL: &str = "newworld_vitaeeterna";
    /// 2027-01-01, with milliseconds.
    const TAKEN_AT_MS: u64 = 1_798_761_600_123;
    const LATER_MS: u64 = 1_000;

    fn spot() -> SavedPosition {
        SavedPosition {
            level: LEVEL.to_owned(),
            x: 8000.5,
            y: 3000.25,
            z: 120.0,
            yaw: 1.5,
            saved_at_ms: TAKEN_AT_MS,
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

    #[test]
    fn a_saved_position_gives_metres_as_f32() {
        assert_eq!(spot().metres(), [8000.5, 3000.25, 120.0]);
    }

    #[tokio::test]
    async fn a_position_is_empty_until_saved_and_then_loads() {
        let (before, saved, after, missing, missing_save, not_finite) =
            with_test_schema(async |pool| {
                WORLD_MIGRATOR.run(pool).await.expect("migrate");
                let character = new_character(pool).await;
                let before = character_position(pool, character).await.expect("load");
                let saved = save_character_position(pool, character, &spot())
                    .await
                    .expect("save");
                let after = character_position(pool, character).await.expect("load");
                let missing = character_position(pool, Uuid::new_v4()).await;
                let missing_save = save_character_position(pool, Uuid::new_v4(), &spot()).await;
                let not_finite = save_character_position(
                    pool,
                    character,
                    &SavedPosition {
                        x: f64::NAN,
                        saved_at_ms: TAKEN_AT_MS + LATER_MS,
                        ..spot()
                    },
                )
                .await;
                (before, saved, after, missing, missing_save, not_finite)
            })
            .await;
        assert_eq!(before, None);
        assert_eq!(saved, SaveOutcome::Saved);
        assert_eq!(after, Some(spot()));
        assert!(matches!(missing, Err(DbError::NoSuchCharacter)));
        assert!(matches!(missing_save, Err(DbError::NoSuchCharacter)));
        assert!(matches!(not_finite, Err(DbError::Database(_))));
    }

    #[tokio::test]
    async fn an_older_save_does_not_overwrite_a_newer_one() {
        let (older, same, newer, loaded) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let character = new_character(pool).await;
            let newest = SavedPosition {
                x: 1.0,
                saved_at_ms: TAKEN_AT_MS + LATER_MS,
                ..spot()
            };
            save_character_position(pool, character, &newest)
                .await
                .expect("save");
            let older = save_character_position(pool, character, &spot()).await;
            let same = save_character_position(pool, character, &newest).await;
            let newer = save_character_position(
                pool,
                character,
                &SavedPosition {
                    x: 2.0,
                    saved_at_ms: TAKEN_AT_MS + 2 * LATER_MS,
                    ..spot()
                },
            )
            .await;
            let loaded = character_position(pool, character).await.expect("load");
            (older, same, newer, loaded)
        })
        .await;
        assert_eq!(older.expect("older"), SaveOutcome::Older);
        assert_eq!(same.expect("same"), SaveOutcome::Older);
        assert_eq!(newer.expect("newer"), SaveOutcome::Saved);
        let loaded = loaded.expect("saved");
        assert_eq!(loaded.x, 2.0);
        assert_eq!(loaded.saved_at_ms, TAKEN_AT_MS + 2 * LATER_MS);
    }
}
