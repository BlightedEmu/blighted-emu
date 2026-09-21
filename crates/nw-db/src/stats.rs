// SPDX-License-Identifier: AGPL-3.0-or-later
//! Stats and leaderboards (global database; study note
//! `docs/re/2026-09-18-leaderboards-and-analytics.md`). The sims add values; `edge-http` answers
//! the client's stats and leaderboard requests from them. A leaderboard is every value of one
//! stat id, ranked.

use sqlx::PgPool;

use crate::error::DbError;

/// The entity of a `Global` stat: global stats belong to no entity.
pub const GLOBAL_ENTITY: &str = "";

/// How a new value joins the stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatMerge {
    /// The stored value plus the new one.
    Add,
    /// The larger of the two.
    Greatest,
    /// The smaller of the two.
    Least,
}

impl StatMerge {
    fn sql_name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Greatest => "greatest",
            Self::Least => "least",
        }
    }
}

/// One value to add to a stat.
#[derive(Debug, Clone, PartialEq)]
pub struct StatUpdate {
    /// The client's namespace, such as `pc`.
    pub namespace: String,
    /// The stat id; for a leaderboard, the board id the client asks for.
    pub stat_id: String,
    /// `Character`, `Company`, `Group` or `Global`.
    pub scope: String,
    /// The entity, [`GLOBAL_ENTITY`] for a global stat.
    pub entity_id: String,
    /// The new value.
    pub value: f64,
    /// How it joins the stored value.
    pub merge: StatMerge,
}

/// Which end of a leaderboard ranks first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardOrder {
    /// The largest value is rank 1 (`top-` boards).
    HighestFirst,
    /// The smallest value is rank 1 (`min-` boards, such as clear times).
    LowestFirst,
}

/// One row of a leaderboard. Equal values share a rank; the next rank skips the shared places.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardEntry {
    /// The entity.
    pub entity_id: String,
    /// Its value.
    pub value: f64,
    /// Its place, from 1.
    pub rank: u64,
}

/// One page of a leaderboard and the board's size.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardPage {
    /// The rows of the page, best first.
    pub entries: Vec<BoardEntry>,
    /// The rows of the whole board.
    pub size: u64,
}

/// One stored stat value.
#[derive(Debug, Clone, PartialEq)]
pub struct StatValue {
    /// The stat id.
    pub stat_id: String,
    /// The entity, [`GLOBAL_ENTITY`] for a global stat.
    pub entity_id: String,
    /// The value.
    pub value: f64,
}

/// Stored stat values and the time of the newest change among them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatValues {
    /// The values, by stat id, then entity.
    pub values: Vec<StatValue>,
    /// Unix milliseconds of the newest change; `None` when there is no value.
    pub last_updated_ms: Option<u64>,
}

/// Adds `updates` in one transaction.
///
/// # Errors
///
/// Returns [`DbError::Database`] when PostgreSQL fails; then no update is stored.
pub async fn submit_stats(pool: &PgPool, updates: &[StatUpdate]) -> Result<(), DbError> {
    let mut transaction = pool.begin().await?;
    for update in updates {
        sqlx::query(
            "INSERT INTO stat_value (namespace, stat_id, scope, entity_id, value) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (namespace, stat_id, entity_id) DO UPDATE SET \
             value = CASE $6 \
                 WHEN 'add' THEN stat_value.value + EXCLUDED.value \
                 WHEN 'greatest' THEN GREATEST(stat_value.value, EXCLUDED.value) \
                 ELSE LEAST(stat_value.value, EXCLUDED.value) END, \
             scope = EXCLUDED.scope, updated_at = now()",
        )
        .bind(&update.namespace)
        .bind(&update.stat_id)
        .bind(&update.scope)
        .bind(&update.entity_id)
        .bind(update.value)
        .bind(update.merge.sql_name())
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// The ranked rows of board `board_id`, best first: equal values rank alike, the older change
/// first.
const RANKED_BOARD: &str = "SELECT entity_id, value, updated_at, \
     RANK() OVER (ORDER BY CASE WHEN $3 THEN value ELSE -value END) AS place \
     FROM stat_value WHERE namespace = $1 AND stat_id = $2";

type EntryRow = (String, f64, i64);

fn entry((entity_id, value, rank): EntryRow) -> BoardEntry {
    BoardEntry {
        entity_id,
        value,
        rank: u64::try_from(rank).unwrap_or(0),
    }
}

/// Page `page_number` (from 1) of `page_size` rows of leaderboard `board_id`.
///
/// # Errors
///
/// Returns [`DbError::Database`] when PostgreSQL fails.
pub async fn leaderboard_page(
    pool: &PgPool,
    namespace: &str,
    board_id: &str,
    order: BoardOrder,
    page_number: u32,
    page_size: u32,
) -> Result<BoardPage, DbError> {
    let offset = i64::from(page_number.saturating_sub(1)) * i64::from(page_size);
    let rows: Vec<EntryRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT entity_id, value, place FROM ({RANKED_BOARD}) AS ranked \
         ORDER BY place, updated_at, entity_id LIMIT $4 OFFSET $5"
    )))
    .bind(namespace)
    .bind(board_id)
    .bind(order == BoardOrder::LowestFirst)
    .bind(i64::from(page_size))
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let size: i64 =
        sqlx::query_scalar("SELECT count(*) FROM stat_value WHERE namespace = $1 AND stat_id = $2")
            .bind(namespace)
            .bind(board_id)
            .fetch_one(pool)
            .await?;
    Ok(BoardPage {
        entries: rows.into_iter().map(entry).collect(),
        size: u64::try_from(size).unwrap_or(0),
    })
}

/// The rows of `entity_ids` on leaderboard `board_id`, best first; an entity with no value has no
/// row.
///
/// # Errors
///
/// Returns [`DbError::Database`] when PostgreSQL fails.
pub async fn leaderboard_entries(
    pool: &PgPool,
    namespace: &str,
    board_id: &str,
    order: BoardOrder,
    entity_ids: &[String],
) -> Result<Vec<BoardEntry>, DbError> {
    let rows: Vec<EntryRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT entity_id, value, place FROM ({RANKED_BOARD}) AS ranked \
         WHERE entity_id = ANY($4) ORDER BY place, updated_at, entity_id"
    )))
    .bind(namespace)
    .bind(board_id)
    .bind(order == BoardOrder::LowestFirst)
    .bind(entity_ids)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(entry).collect())
}

/// The values of `stat_ids` in `scope` for `entity_ids` (for a global stat, [`GLOBAL_ENTITY`]).
///
/// # Errors
///
/// Returns [`DbError::Database`] when PostgreSQL fails.
pub async fn stat_values(
    pool: &PgPool,
    namespace: &str,
    scope: &str,
    stat_ids: &[String],
    entity_ids: &[String],
) -> Result<StatValues, DbError> {
    let rows: Vec<(String, String, f64, i64)> = sqlx::query_as(
        "SELECT stat_id, entity_id, value, \
         (EXTRACT(EPOCH FROM updated_at) * 1000)::BIGINT \
         FROM stat_value WHERE namespace = $1 AND scope = $2 \
         AND stat_id = ANY($3) AND entity_id = ANY($4) ORDER BY stat_id, entity_id",
    )
    .bind(namespace)
    .bind(scope)
    .bind(stat_ids)
    .bind(entity_ids)
    .fetch_all(pool)
    .await?;
    let last_updated_ms = rows
        .iter()
        .filter_map(|(_, _, _, updated_ms)| u64::try_from(*updated_ms).ok())
        .max();
    let values = rows
        .into_iter()
        .map(|(stat_id, entity_id, value, _)| StatValue {
            stat_id,
            entity_id,
            value,
        })
        .collect();
    Ok(StatValues {
        values,
        last_updated_ms,
    })
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use super::*;
    use crate::migrate::GLOBAL_MIGRATOR;
    use crate::test_db::with_test_schema;

    const NAMESPACE: &str = "pc";
    const BOARD: &str = "top-boss-trial-completions.HercyneRaid_Reward.w.m41";
    const CLEAR_TIME: &str = "min-expedition-clear-time.w.w178";
    const CHARACTER: &str = "Character";
    const GLOBAL: &str = "Global";
    const FACTION_STAT: &str = "influence$open-world.Faction1.w.s11";
    const PAGE_SIZE: u32 = 2;

    fn update(stat_id: &str, entity: &str, value: f64, merge: StatMerge) -> StatUpdate {
        StatUpdate {
            namespace: NAMESPACE.to_owned(),
            stat_id: stat_id.to_owned(),
            scope: CHARACTER.to_owned(),
            entity_id: entity.to_owned(),
            value,
            merge,
        }
    }

    fn ranks(entries: &[BoardEntry]) -> Vec<(&str, f64, u64)> {
        entries
            .iter()
            .map(|entry| (entry.entity_id.as_str(), entry.value, entry.rank))
            .collect()
    }

    #[tokio::test]
    async fn values_merge_and_boards_rank_both_ways() {
        let (first, second, mine, low, global) = with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            submit_stats(
                pool,
                &[
                    update(BOARD, "a", 1.0, StatMerge::Add),
                    update(BOARD, "a", 2.0, StatMerge::Add),
                    update(BOARD, "b", 3.0, StatMerge::Add),
                    update(BOARD, "c", 1.0, StatMerge::Add),
                    update(CLEAR_TIME, "a", 90.0, StatMerge::Least),
                    update(CLEAR_TIME, "a", 120.0, StatMerge::Least),
                    update(CLEAR_TIME, "b", 100.0, StatMerge::Least),
                    StatUpdate {
                        scope: GLOBAL.to_owned(),
                        ..update(FACTION_STAT, GLOBAL_ENTITY, 7.0, StatMerge::Greatest)
                    },
                ],
            )
            .await
            .expect("submit");
            let order = BoardOrder::HighestFirst;
            let first = leaderboard_page(pool, NAMESPACE, BOARD, order, 1, PAGE_SIZE)
                .await
                .expect("page 1");
            let second = leaderboard_page(pool, NAMESPACE, BOARD, order, 2, PAGE_SIZE)
                .await
                .expect("page 2");
            let mine = leaderboard_entries(
                pool,
                NAMESPACE,
                BOARD,
                order,
                &["c".to_owned(), "nobody".to_owned()],
            )
            .await
            .expect("entries");
            let low = leaderboard_page(
                pool,
                NAMESPACE,
                CLEAR_TIME,
                BoardOrder::LowestFirst,
                1,
                PAGE_SIZE,
            )
            .await
            .expect("clear times");
            let global = stat_values(
                pool,
                NAMESPACE,
                GLOBAL,
                &[FACTION_STAT.to_owned(), "missing".to_owned()],
                &[GLOBAL_ENTITY.to_owned()],
            )
            .await
            .expect("stats");
            (first, second, mine, low, global)
        })
        .await;
        assert_eq!(first.size, 3);
        assert_eq!(ranks(&first.entries), [("a", 3.0, 1), ("b", 3.0, 1)]);
        assert_eq!(ranks(&second.entries), [("c", 1.0, 3)]);
        assert_eq!(ranks(&mine), [("c", 1.0, 3)]);
        assert_eq!(ranks(&low.entries), [("a", 90.0, 1), ("b", 100.0, 2)]);
        assert_eq!(
            global.values,
            [StatValue {
                stat_id: FACTION_STAT.to_owned(),
                entity_id: GLOBAL_ENTITY.to_owned(),
                value: 7.0
            }]
        );
        assert!(global.last_updated_ms.is_some());
    }

    #[tokio::test]
    async fn an_empty_board_has_no_rows() {
        let (page, stats) = with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            let page = leaderboard_page(
                pool,
                NAMESPACE,
                BOARD,
                BoardOrder::HighestFirst,
                1,
                PAGE_SIZE,
            )
            .await
            .expect("page");
            let stats = stat_values(pool, NAMESPACE, CHARACTER, &[BOARD.to_owned()], &[])
                .await
                .expect("stats");
            (page, stats)
        })
        .await;
        assert_eq!(
            page,
            BoardPage {
                entries: Vec::new(),
                size: 0
            }
        );
        assert_eq!(stats, StatValues::default());
    }
}
