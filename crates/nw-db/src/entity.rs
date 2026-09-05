// SPDX-License-Identifier: AGPL-3.0-or-later
//! Entity numbers of a world (M5 spec section 10.1): one sequence per world database, so a
//! number never repeats, also after a restart of the director.

use sqlx::PgPool;

use crate::error::DbError;

/// The largest entity number: the 5-byte prefix code of a bundle member holds 35 bits.
pub const MAX_ENTITY_NUMBER: u64 = (1 << 35) - 1;

/// A new entity number of the world.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the sequence is used up or the database fails.
pub async fn next_entity_number(world: &PgPool) -> Result<u64, DbError> {
    let (number,): (i64,) = sqlx::query_as("SELECT nextval('entity_number')")
        .fetch_one(world)
        .await?;
    // The sequence starts at 1, so the number is never negative.
    Ok(u64::try_from(number).unwrap_or_default())
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::test_db::with_test_schema;

    const TAKES: usize = 3;

    #[tokio::test]
    async fn entity_numbers_rise_and_never_repeat() {
        let numbers = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let mut numbers = Vec::new();
            for _ in 0..TAKES {
                numbers.push(next_entity_number(pool).await.expect("number"));
            }
            numbers
        })
        .await;
        assert_eq!(numbers, vec![1, 2, 3]);
        assert!(numbers.iter().all(|number| *number <= MAX_ENTITY_NUMBER));
    }
}
