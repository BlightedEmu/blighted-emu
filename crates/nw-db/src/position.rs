// SPDX-License-Identifier: AGPL-3.0-or-later
//! The saved world position of a character (M4 spec section 8). `sim` loads it at a join and
//! saves it every 10 s and at a departure.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;

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
}

type PositionRow = (
    Option<String>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f32>,
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
        "SELECT level_name, pos_x, pos_y, pos_z, yaw FROM player_character \
         WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    match row.ok_or(DbError::NoSuchCharacter)? {
        (Some(level), Some(x), Some(y), Some(z), Some(yaw)) => Ok(Some(SavedPosition {
            level,
            x,
            y,
            z,
            yaw,
        })),
        _ => Ok(None),
    }
}

/// Saves the position of a character.
///
/// # Errors
///
/// Returns [`DbError::NoSuchCharacter`] when the world has no such character, and
/// [`DbError::Database`] for a value that is not finite (a table constraint refuses it).
pub async fn save_character_position(
    world: &PgPool,
    character_id: Uuid,
    position: &SavedPosition,
) -> Result<(), DbError> {
    let updated = sqlx::query(
        "UPDATE player_character SET level_name = $2, pos_x = $3, pos_y = $4, pos_z = $5, \
         yaw = $6, position_saved_at = now() WHERE character_id = $1",
    )
    .bind(character_id)
    .bind(&position.level)
    .bind(position.x)
    .bind(position.y)
    .bind(position.z)
    .bind(position.yaw)
    .execute(world)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(DbError::NoSuchCharacter);
    }
    Ok(())
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::test_db::with_test_schema;
    use crate::{NewCharacter, insert_character};

    const LEVEL: &str = "newworld_vitaeeterna";

    fn spot() -> SavedPosition {
        SavedPosition {
            level: LEVEL.to_owned(),
            x: 8000.5,
            y: 3000.25,
            z: 120.0,
            yaw: 1.5,
        }
    }

    #[tokio::test]
    async fn a_position_is_empty_until_saved_and_then_loads() {
        let (before, after, missing, missing_save, not_finite) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let character = insert_character(
                pool,
                &NewCharacter {
                    id: Uuid::new_v4(),
                    account_id: Uuid::new_v4(),
                    name: "Ada".to_owned(),
                    appearance: serde_json::json!({}),
                },
            )
            .await
            .expect("insert");
            let before = character_position(pool, character.id).await.expect("load");
            save_character_position(pool, character.id, &spot())
                .await
                .expect("save");
            let after = character_position(pool, character.id).await.expect("load");
            let missing = character_position(pool, Uuid::new_v4()).await;
            let missing_save = save_character_position(pool, Uuid::new_v4(), &spot()).await;
            let not_finite = save_character_position(
                pool,
                character.id,
                &SavedPosition {
                    x: f64::NAN,
                    ..spot()
                },
            )
            .await;
            (before, after, missing, missing_save, not_finite)
        })
        .await;
        assert_eq!(before, None);
        assert_eq!(after, Some(spot()));
        assert!(matches!(missing, Err(DbError::NoSuchCharacter)));
        assert!(matches!(missing_save, Err(DbError::NoSuchCharacter)));
        assert!(matches!(not_finite, Err(DbError::Database(_))));
    }
}
