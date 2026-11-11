// SPDX-License-Identifier: AGPL-3.0-or-later
//! The depleted gather nodes of a level (world migration 0034): world state that a sim restart
//! keeps, so a harvested node does not grow back before its time.

use sqlx::PgPool;

use crate::error::DbError;
use crate::progression::{from_column, to_column};

/// Bytes of a node id (`GdeRef`).
pub const NODE_ID_LEN: usize = 16;

/// One depleted gather node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedGatherNode {
    /// The node's id (`GdeRef`).
    pub gde_ref: [u8; NODE_ID_LEN],
    /// The level gatherable point it stands at.
    pub point: u32,
    /// Where its first gatherer stood: east, north, height.
    pub position: [f32; 3],
    /// When it grows back, Unix ms.
    pub respawn_at_unix_ms: u64,
}

type NodeRow = (Vec<u8>, i32, f32, f32, f32, i64);

/// The depleted nodes of `level` whose time has not come at `now_unix_ms`; the rows whose time
/// came are removed.
///
/// # Errors
///
/// Returns [`DbError::OutOfRange`] for a stored value the program's types do not hold, and
/// [`DbError::Database`] when a query fails.
pub async fn level_gather_nodes(
    world: &PgPool,
    level: &str,
    now_unix_ms: u64,
) -> Result<Vec<SavedGatherNode>, DbError> {
    let now = to_column(now_unix_ms, "gather node time")?;
    sqlx::query("DELETE FROM world_gather_node WHERE level = $1 AND respawn_at_unix_ms <= $2")
        .bind(level)
        .bind(now)
        .execute(world)
        .await?;
    let rows: Vec<NodeRow> = sqlx::query_as(
        "SELECT gde_ref, point, x, y, z, respawn_at_unix_ms FROM world_gather_node \
         WHERE level = $1 ORDER BY point",
    )
    .bind(level)
    .fetch_all(world)
    .await?;
    rows.into_iter()
        .map(|(gde_ref, point, x, y, z, respawn_at)| {
            Ok(SavedGatherNode {
                gde_ref: gde_ref.try_into().map_err(|_| DbError::OutOfRange {
                    field: "gather node id",
                })?,
                point: from_column(i64::from(point), "gather node point")?,
                position: [x, y, z],
                respawn_at_unix_ms: from_column(respawn_at, "gather node time")?,
            })
        })
        .collect()
}

/// Saves `node` as a depleted node of `level`, in place of an older row of the same node.
///
/// # Errors
///
/// Returns [`DbError::OutOfRange`] for a value its column cannot hold, and
/// [`DbError::Database`] when the query fails.
pub async fn save_gather_node(
    world: &PgPool,
    level: &str,
    node: &SavedGatherNode,
) -> Result<(), DbError> {
    let point = i32::try_from(node.point).map_err(|_| DbError::OutOfRange {
        field: "gather node point",
    })?;
    sqlx::query(
        "INSERT INTO world_gather_node (level, gde_ref, point, x, y, z, respawn_at_unix_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (level, gde_ref) DO UPDATE SET point = $3, x = $4, y = $5, z = $6, \
         respawn_at_unix_ms = $7",
    )
    .bind(level)
    .bind(node.gde_ref.as_slice())
    .bind(point)
    .bind(node.position[0])
    .bind(node.position[1])
    .bind(node.position[2])
    .bind(to_column(node.respawn_at_unix_ms, "gather node time")?)
    .execute(world)
    .await?;
    Ok(())
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::test_db::with_test_schema;

    const LEVEL: &str = "newworld_vitaeeterna";
    const NOW_MS: u64 = 1_798_761_600_000;
    const LATER_MS: u64 = 600_000;

    fn node(id: u8, respawn_at_unix_ms: u64) -> SavedGatherNode {
        SavedGatherNode {
            gde_ref: [id; NODE_ID_LEN],
            point: u32::from(id),
            position: [7_700.5, 7_300.25, 92.0],
            respawn_at_unix_ms,
        }
    }

    #[tokio::test]
    async fn a_depleted_node_loads_until_its_time_and_a_newer_save_replaces_it() {
        let waits = node(1, NOW_MS + LATER_MS);
        let (now, later) = with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            save_gather_node(pool, LEVEL, &node(1, NOW_MS + 1))
                .await
                .expect("save");
            save_gather_node(pool, LEVEL, &waits)
                .await
                .expect("save again");
            save_gather_node(pool, LEVEL, &node(2, NOW_MS))
                .await
                .expect("save grown");
            save_gather_node(pool, "other_level", &node(3, NOW_MS + LATER_MS))
                .await
                .expect("save other level");
            (
                level_gather_nodes(pool, LEVEL, NOW_MS).await.expect("load"),
                level_gather_nodes(pool, LEVEL, NOW_MS + LATER_MS)
                    .await
                    .expect("load later"),
            )
        })
        .await;
        assert_eq!(now, vec![waits]);
        assert!(later.is_empty());
    }
}
