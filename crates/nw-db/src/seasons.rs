// SPDX-License-Identifier: AGPL-3.0-or-later
//! The season of an account (global migration 0010): its season experience and its season pass
//! state (the redeemed rewards, the tracked stats and the activity card). Observed: retail keeps
//! the season per account; a new character came in with season rank 39 and 433 points (newchar
//! ledger, seasons finding 2026-11-12). An account without a row has the default season (none
//! yet).
//!
//! Until global migration 0010 the server kept the season per character (world migration 0041,
//! `character_season`, and the character's `SeasonPass` mastery). The first load of an account
//! ([`account_season`]) moves the season of the character it loads into the account once; the
//! season rows of the account's other characters are dropped at their next load (Inferred: our
//! design; the first character loaded keeps its season, as two per-character seasons cannot be
//! one retail account season).

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;
use crate::position::SaveOutcome;
use crate::progression::{MILLIS_PER_SECOND, from_column};
use crate::travel::{id_column, id_value};

/// The categorical progression whose total experience is the account's season experience (its
/// rank is the season level).
pub const SEASON_PASS_PROGRESSION: &str = "SeasonPass";
/// The columns of the season pass state, in [`SeasonColumns`] order.
const SEASON_COLUMNS: &str = "season_id, redeemed, stat_ids, stat_values, card_template, \
     card_tasks, card_claimed, card_stamped, card_wild_stamped, wild_stamps, rerolls, cards_done, \
     card_stat_ids, card_stat_values, first_connect_ns";
/// The `saved_at` of a season moved from a character, Unix seconds: older than every save.
const MOVED_AT_S: f64 = 0.0;

/// An account's season.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedAccountSeason {
    /// The total experience of the [`SEASON_PASS_PROGRESSION`] progression.
    pub experience: u32,
    /// The season pass state.
    pub season: SavedSeason,
}

/// A season pass state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedSeason {
    /// The CRC-32 of the lowercase `SeasonId` it is for; 0 for none.
    pub season: u32,
    /// The redeemed rewards: bit `n` of word `n / 64` is `RewardIndex` `n`.
    pub redeemed: Vec<u64>,
    /// The season's tracked stats: (CRC-32 of the lowercase `TrackedStatId`, count).
    pub stats: Vec<(u32, u32)>,
    /// The activity card.
    pub card: SavedCard,
    /// `firstCharacterConnectTime`: Unix nanoseconds.
    pub first_connect_ns: u64,
}

/// An activity card.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedCard {
    /// `cardTemplate`.
    pub template: u8,
    /// The CRC-32 of each lowercase `ActivitiesTaskID`; empty for no card yet.
    pub tasks: Vec<u32>,
    /// The bit of each task that stamped a square.
    pub claimed: u32,
    /// The stamped squares (bit `row * 4 + column`).
    pub stamped: u16,
    /// The squares a wild stamp filled.
    pub wild_stamped: u16,
    /// `wildStampCount`.
    pub wild_stamps: u16,
    /// `rerollCount`.
    pub rerolls: u8,
    /// `cardCount`.
    pub cards_done: u16,
    /// The card tasks' tracked stats since the card's start: (stat CRC-32, count).
    pub stats: Vec<(u32, u32)>,
}

/// The season pass columns of an `account_season` or a `character_season` row.
#[derive(Debug, Clone, sqlx::FromRow)]
struct SeasonColumns {
    season_id: i64,
    redeemed: Vec<i64>,
    stat_ids: Vec<i64>,
    stat_values: Vec<i64>,
    card_template: i16,
    card_tasks: Vec<i64>,
    card_claimed: i64,
    card_stamped: i32,
    card_wild_stamped: i32,
    wild_stamps: i16,
    rerolls: i16,
    cards_done: i32,
    card_stat_ids: Vec<i64>,
    card_stat_values: Vec<i64>,
    first_connect_ns: i64,
}

/// The (id, count) pairs of two array columns.
fn stat_pairs(ids: Vec<i64>, values: Vec<i64>) -> Result<Vec<(u32, u32)>, DbError> {
    ids.into_iter()
        .zip(values)
        .map(|(id, value)| {
            Ok((
                from_column(id, "stat id")?,
                from_column(value, "stat value")?,
            ))
        })
        .collect()
}

/// The columns of an `account_season` row.
#[derive(Debug, Clone, sqlx::FromRow)]
struct AccountSeasonColumns {
    experience: i64,
    #[sqlx(flatten)]
    season: SeasonColumns,
}

/// The season of `account`; `None` when it has no row.
async fn account_row(
    global: &PgPool,
    account: Uuid,
) -> Result<Option<SavedAccountSeason>, DbError> {
    let row: Option<AccountSeasonColumns> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT experience, {SEASON_COLUMNS} FROM account_season WHERE account_id = $1"
    )))
    .bind(account)
    .fetch_optional(global)
    .await?;
    row.map(|row| {
        Ok(SavedAccountSeason {
            experience: from_column(row.experience, "experience")?,
            season: season_of(row.season)?,
        })
    })
    .transpose()
}

/// The season a character kept before global migration 0010: its `character_season` row and its
/// `SeasonPass` mastery; `None` when it has neither.
async fn character_season(
    world: &PgPool,
    character_id: Uuid,
) -> Result<Option<SavedAccountSeason>, DbError> {
    let row: Option<SeasonColumns> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {SEASON_COLUMNS} FROM character_season WHERE character_id = $1"
    )))
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    let experience: Option<(i64,)> = sqlx::query_as(
        "SELECT experience FROM character_mastery WHERE character_id = $1 AND category = $2",
    )
    .bind(character_id)
    .bind(SEASON_PASS_PROGRESSION)
    .fetch_optional(world)
    .await?;
    if row.is_none() && experience.is_none() {
        return Ok(None);
    }
    Ok(Some(SavedAccountSeason {
        experience: experience
            .map(|(experience,)| from_column(experience, "experience"))
            .transpose()?
            .unwrap_or_default(),
        season: row.map(season_of).transpose()?.unwrap_or_default(),
    }))
}

/// The season of `account` (global database `global`), loaded for its character `character`
/// (world database `world`). An account without a season takes the season the character kept
/// before global migration 0010, once: the character's season row goes, and its `SeasonPass`
/// mastery goes with the character's next progression save, which leaves that progression out.
///
/// # Errors
///
/// Returns [`DbError`] when a query fails or a stored value does not fit its type.
pub async fn account_season(
    global: &PgPool,
    world: &PgPool,
    account: Uuid,
    character: Uuid,
) -> Result<SavedAccountSeason, DbError> {
    if account_row(global, account).await?.is_none()
        && let Some(moved) = character_season(world, character).await?
    {
        store_account_season(global, account, &moved, MOVED_AT_S).await?;
    }
    sqlx::query("DELETE FROM character_season WHERE character_id = $1")
        .bind(character)
        .execute(world)
        .await?;
    Ok(account_row(global, account).await?.unwrap_or_default())
}

fn season_of(row: SeasonColumns) -> Result<SavedSeason, DbError> {
    Ok(SavedSeason {
        season: from_column(row.season_id, "season_id")?,
        redeemed: row.redeemed.into_iter().map(id_value).collect(),
        stats: stat_pairs(row.stat_ids, row.stat_values)?,
        card: SavedCard {
            template: from_column(i64::from(row.card_template), "card_template")?,
            tasks: row
                .card_tasks
                .into_iter()
                .map(|task| from_column(task, "card_tasks"))
                .collect::<Result<_, _>>()?,
            claimed: from_column(row.card_claimed, "card_claimed")?,
            stamped: from_column(i64::from(row.card_stamped), "card_stamped")?,
            wild_stamped: from_column(i64::from(row.card_wild_stamped), "card_wild_stamped")?,
            wild_stamps: from_column(i64::from(row.wild_stamps), "wild_stamps")?,
            rerolls: from_column(i64::from(row.rerolls), "rerolls")?,
            cards_done: from_column(i64::from(row.cards_done), "cards_done")?,
            stats: stat_pairs(row.card_stat_ids, row.card_stat_values)?,
        },
        first_connect_ns: id_value(row.first_connect_ns),
    })
}

/// The id and the count columns of `stats`.
fn stat_columns(stats: &[(u32, u32)]) -> (Vec<i64>, Vec<i64>) {
    stats
        .iter()
        .map(|(id, value)| (i64::from(*id), i64::from(*value)))
        .unzip()
}

/// Saves `season`, taken at Unix ms `taken_at_ms`, as the whole season of `account`: a save of
/// an older state than the stored one changes nothing ([`SaveOutcome::Older`]).
///
/// # Errors
///
/// Returns [`DbError`] when the query fails (an account the global database does not hold, too)
/// or a value does not fit its column.
pub async fn save_account_season(
    global: &PgPool,
    account: Uuid,
    season: &SavedAccountSeason,
    taken_at_ms: u64,
) -> Result<SaveOutcome, DbError> {
    let taken_at_s = taken_at_ms as f64 / MILLIS_PER_SECOND;
    Ok(
        if store_account_season(global, account, season, taken_at_s).await? {
            SaveOutcome::Saved
        } else {
            SaveOutcome::Older
        },
    )
}

/// Writes `saved` as the season of `account`, taken at Unix second `saved_at_s`, unless the
/// stored one was taken at that time or later; whether it was written.
async fn store_account_season(
    global: &PgPool,
    account: Uuid,
    saved: &SavedAccountSeason,
    saved_at_s: f64,
) -> Result<bool, DbError> {
    let season = &saved.season;
    let (stat_ids, stat_values) = stat_columns(&season.stats);
    let (card_stat_ids, card_stat_values) = stat_columns(&season.card.stats);
    let card = &season.card;
    let written = sqlx::query(
        "INSERT INTO account_season (account_id, saved_at, experience, season_id, redeemed, \
         stat_ids, stat_values, card_template, card_tasks, card_claimed, card_stamped, \
         card_wild_stamped, wild_stamps, rerolls, cards_done, card_stat_ids, card_stat_values, \
         first_connect_ns) \
         VALUES ($1, to_timestamp($2), $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
         $16, $17, $18) \
         ON CONFLICT (account_id) DO UPDATE SET saved_at = EXCLUDED.saved_at, \
         experience = EXCLUDED.experience, season_id = EXCLUDED.season_id, \
         redeemed = EXCLUDED.redeemed, stat_ids = EXCLUDED.stat_ids, \
         stat_values = EXCLUDED.stat_values, card_template = EXCLUDED.card_template, \
         card_tasks = EXCLUDED.card_tasks, card_claimed = EXCLUDED.card_claimed, \
         card_stamped = EXCLUDED.card_stamped, card_wild_stamped = EXCLUDED.card_wild_stamped, \
         wild_stamps = EXCLUDED.wild_stamps, rerolls = EXCLUDED.rerolls, \
         cards_done = EXCLUDED.cards_done, card_stat_ids = EXCLUDED.card_stat_ids, \
         card_stat_values = EXCLUDED.card_stat_values, \
         first_connect_ns = EXCLUDED.first_connect_ns \
         WHERE account_season.saved_at < EXCLUDED.saved_at",
    )
    .bind(account)
    .bind(saved_at_s)
    .bind(i64::from(saved.experience))
    .bind(i64::from(season.season))
    .bind(
        season
            .redeemed
            .iter()
            .map(|word| id_column(*word))
            .collect::<Vec<_>>(),
    )
    .bind(stat_ids)
    .bind(stat_values)
    .bind(i16::from(card.template))
    .bind(
        card.tasks
            .iter()
            .map(|task| i64::from(*task))
            .collect::<Vec<_>>(),
    )
    .bind(i64::from(card.claimed))
    .bind(i32::from(card.stamped))
    .bind(i32::from(card.wild_stamped))
    .bind(
        i16::try_from(card.wild_stamps).map_err(|_| DbError::OutOfRange {
            field: "wild_stamps",
        })?,
    )
    .bind(i16::from(card.rerolls))
    .bind(i32::from(card.cards_done))
    .bind(card_stat_ids)
    .bind(card_stat_values)
    .bind(id_column(season.first_connect_ns))
    .execute(global)
    .await?;
    Ok(written.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns(season: &SavedSeason) -> SeasonColumns {
        let (stat_ids, stat_values) = stat_columns(&season.stats);
        let (card_stat_ids, card_stat_values) = stat_columns(&season.card.stats);
        SeasonColumns {
            season_id: i64::from(season.season),
            redeemed: season
                .redeemed
                .iter()
                .map(|word| id_column(*word))
                .collect(),
            stat_ids,
            stat_values,
            card_template: i16::from(season.card.template),
            card_tasks: season
                .card
                .tasks
                .iter()
                .map(|task| i64::from(*task))
                .collect(),
            card_claimed: i64::from(season.card.claimed),
            card_stamped: i32::from(season.card.stamped),
            card_wild_stamped: i32::from(season.card.wild_stamped),
            wild_stamps: i16::try_from(season.card.wild_stamps).expect("small"),
            rerolls: i16::from(season.card.rerolls),
            cards_done: i32::from(season.card.cards_done),
            card_stat_ids,
            card_stat_values,
            first_connect_ns: id_column(season.first_connect_ns),
        }
    }

    #[test]
    fn a_season_row_reads_back_as_it_was_written() {
        // Made-up values in the retail forms: `Season10`, the redeemed words of reward indexes
        // 0 to 26 and 63, a full card of template 5.
        let season = SavedSeason {
            season: 0x2221_5ad9,
            redeemed: vec![0x8000_0000_07ff_ffff],
            stats: vec![(0x1234_5678, 6), (0xffff_fffe, 0)],
            card: SavedCard {
                template: 5,
                tasks: vec![0x199d_1dce, 0x9475_c5d2],
                claimed: 0b11,
                stamped: u16::MAX,
                wild_stamped: 0xefcf,
                wild_stamps: 3,
                rerolls: 2,
                cards_done: 6,
                stats: vec![(0x0d7c_9468, 10)],
            },
            first_connect_ns: 0x1870_3ee3_5200_8884,
        };
        assert_eq!(season_of(columns(&season)).expect("read"), season);
        let mut bad = columns(&season);
        bad.season_id = -1;
        assert!(matches!(
            season_of(bad),
            Err(DbError::OutOfRange { field: "season_id" })
        ));
    }
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::account::create_password_account;
    use crate::migrate::{GLOBAL_MIGRATOR, WORLD_MIGRATOR};
    use crate::test_db::with_test_schema;
    use crate::{NewCharacter, insert_character};

    /// The newchar ledger's season experience (seasons finding 2026-11-12: rank 39, 433 points).
    const NEWCHAR_EXPERIENCE: i64 = 107_683;
    /// Two save times, Unix ms.
    const TAKEN_AT_MS: u64 = 1_798_761_600_123;
    const EARLIER_MS: u64 = TAKEN_AT_MS - 1_000;

    async fn character_of(world: &PgPool, account: Uuid) -> Uuid {
        insert_character(
            world,
            &NewCharacter {
                id: Uuid::new_v4(),
                account_id: account,
                name: "Made Up".to_owned(),
                appearance: serde_json::json!({}),
            },
        )
        .await
        .expect("insert")
        .id
    }

    /// A season a character kept before global migration 0010: a row with two redeemed
    /// rewards and the newchar season experience as its `SeasonPass` mastery.
    async fn keep_old_season(world: &PgPool, character: Uuid) {
        sqlx::query(
            "INSERT INTO character_season (character_id, season_id, redeemed, stat_ids, \
             stat_values, card_template, card_tasks, card_claimed, card_stamped, \
             card_wild_stamped, wild_stamps, rerolls, cards_done, card_stat_ids, \
             card_stat_values, first_connect_ns) \
             VALUES ($1, 572611289, '{3}', '{}', '{}', 1, '{}', 0, 0, 0, 3, 0, 0, '{}', '{}', 7)",
        )
        .bind(character)
        .execute(world)
        .await
        .expect("old season");
        sqlx::query(
            "INSERT INTO character_mastery (character_id, category, experience) VALUES ($1, $2, $3)",
        )
        .bind(character)
        .bind(SEASON_PASS_PROGRESSION)
        .bind(NEWCHAR_EXPERIENCE)
        .execute(world)
        .await
        .expect("old experience");
    }

    #[tokio::test]
    async fn a_character_s_season_moves_into_its_account_once_and_saves_there() {
        let (moved, again, other, saved, older, after) = with_test_schema(async |global| {
            GLOBAL_MIGRATOR.run(global).await.expect("migrate global");
            with_test_schema(async |world| {
                WORLD_MIGRATOR.run(world).await.expect("migrate world");
                let account = create_password_account(global, "seasons", "$argon2id$x")
                    .await
                    .expect("account")
                    .id;
                let first = character_of(world, account).await;
                let second = character_of(world, account).await;
                keep_old_season(world, first).await;
                keep_old_season(world, second).await;
                let moved = account_season(global, world, account, first)
                    .await
                    .expect("move");
                let again = account_season(global, world, account, first)
                    .await
                    .expect("again");
                let other = account_season(global, world, account, second)
                    .await
                    .expect("other character");
                let left: (i64,) = sqlx::query_as("SELECT count(*) FROM character_season")
                    .fetch_one(world)
                    .await
                    .expect("count");
                assert_eq!(left.0, 0, "both old rows are gone");
                let newer = SavedAccountSeason {
                    experience: moved.experience + 50,
                    ..moved.clone()
                };
                let saved = save_account_season(global, account, &newer, TAKEN_AT_MS)
                    .await
                    .expect("save");
                let older = save_account_season(global, account, &moved, EARLIER_MS)
                    .await
                    .expect("older");
                let after = account_season(global, world, account, second)
                    .await
                    .expect("after");
                (moved, again, other, saved, older, after)
            })
            .await
        })
        .await;
        assert_eq!(i64::from(moved.experience), NEWCHAR_EXPERIENCE);
        assert_eq!(moved.season.season, 0x2221_5ad9, "Season10");
        assert_eq!(moved.season.redeemed, [3]);
        assert_eq!(moved.season.card.wild_stamps, 3);
        assert_eq!(again, moved, "moved once");
        assert_eq!(
            other, moved,
            "every character of the account has the account's season"
        );
        assert_eq!(saved, SaveOutcome::Saved);
        assert_eq!(older, SaveOutcome::Older);
        assert_eq!(i64::from(after.experience), NEWCHAR_EXPERIENCE + 50);
    }
}
