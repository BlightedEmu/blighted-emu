// SPDX-License-Identifier: AGPL-3.0-or-later
//! How often each event notification was shown to a character (world migration 0022). It is
//! part of the progression ([`crate::SavedProgression`]): it loads and saves with it, in the same
//! transaction.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};

/// A character's shown notifications: (CRC-32 of the lowercase `NotificationID`, times shown), in
/// ascending id order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedNotifications {
    /// The counts.
    pub shown: Vec<(u32, u32)>,
}

/// The shown notifications of a character.
pub(crate) async fn character_notifications(
    world: &PgPool,
    character_id: Uuid,
) -> Result<SavedNotifications, DbError> {
    let rows: Vec<(i64, i32)> = sqlx::query_as(
        "SELECT notification, shown FROM character_notification WHERE character_id = $1 \
         ORDER BY notification",
    )
    .bind(character_id)
    .fetch_all(world)
    .await?;
    let shown = rows
        .into_iter()
        .map(|(notification, shown)| {
            Ok((
                from_column(notification, "notification")?,
                from_column(i64::from(shown), "shown")?,
            ))
        })
        .collect::<Result<_, DbError>>()?;
    Ok(SavedNotifications { shown })
}

/// Saves `notifications` as the shown notifications of a character whose progression row the
/// transaction already updated: the stored rows become exactly these.
pub(crate) async fn save_character_notifications(
    transaction: &mut Transaction<'_, Postgres>,
    character_id: Uuid,
    notifications: &SavedNotifications,
) -> Result<(), DbError> {
    sqlx::query("DELETE FROM character_notification WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut **transaction)
        .await?;
    for &(notification, shown) in &notifications.shown {
        let notification: i64 = to_column(notification, "notification")?;
        let shown = i32::try_from(shown).map_err(|_| DbError::OutOfRange { field: "shown" })?;
        sqlx::query(
            "INSERT INTO character_notification (character_id, notification, shown) \
             VALUES ($1, $2, $3)",
        )
        .bind(character_id)
        .bind(notification)
        .bind(shown)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}
