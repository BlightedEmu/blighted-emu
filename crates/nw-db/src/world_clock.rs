// SPDX-License-Identifier: AGPL-3.0-or-later
//! The world's clock anchor (world migration 0035): the time of `SyncTimeChange`, kept over
//! restarts and shared by every sim of the world.

use sqlx::PgPool;

use crate::error::DbError;
use crate::progression::{from_column, to_column};

/// The one row of the table.
const CLOCK_ROW: i16 = 1;

/// The world's clock anchor, ns since 2000-01-01: the stored one, or `candidate_ns` when none is
/// stored yet (it is stored then, and a sim that starts at the same time reads the same one).
///
/// # Errors
///
/// Returns [`DbError::OutOfRange`] for a time its column cannot hold, and [`DbError::Database`]
/// when a query fails.
pub async fn world_clock_start(world: &PgPool, candidate_ns: u64) -> Result<u64, DbError> {
    sqlx::query(
        "INSERT INTO world_clock (id, start_ns) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING",
    )
    .bind(CLOCK_ROW)
    .bind(to_column(candidate_ns, "world clock start")?)
    .execute(world)
    .await?;
    let (start,): (i64,) = sqlx::query_as("SELECT start_ns FROM world_clock WHERE id = $1")
        .bind(CLOCK_ROW)
        .fetch_one(world)
        .await?;
    from_column(start, "world clock start")
}

/// Sets the world's clock anchor to `anchor_ns` (ns since 2000-01-01): a game master's time.
/// Every sim reads it again ([`world_clock_start`]) and follows it.
///
/// # Errors
///
/// Returns [`DbError::OutOfRange`] for a time its column cannot hold, and [`DbError::Database`]
/// when the query fails.
pub async fn set_world_clock(world: &PgPool, anchor_ns: u64) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO world_clock (id, start_ns) VALUES ($1, $2) \
         ON CONFLICT (id) DO UPDATE SET start_ns = EXCLUDED.start_ns",
    )
    .bind(CLOCK_ROW)
    .bind(to_column(anchor_ns, "world clock start")?)
    .execute(world)
    .await?;
    Ok(())
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::test_db::with_test_schema;

    const FIRST_NS: u64 = 845_485_830_908_027_957;
    const LATER_NS: u64 = FIRST_NS + 777_000_000_000;

    #[tokio::test]
    async fn the_first_start_sets_the_anchor_and_every_later_one_reads_it() {
        let (first, later) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            (
                world_clock_start(pool, FIRST_NS).await.expect("first"),
                world_clock_start(pool, LATER_NS).await.expect("later"),
            )
        })
        .await;
        assert_eq!((first, later), (FIRST_NS, FIRST_NS));
    }

    #[tokio::test]
    async fn a_set_anchor_is_what_every_later_start_reads() {
        let read = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            world_clock_start(pool, FIRST_NS).await.expect("first");
            set_world_clock(pool, LATER_NS).await.expect("set");
            world_clock_start(pool, FIRST_NS).await.expect("read")
        })
        .await;
        assert_eq!(read, LATER_NS);
    }
}
