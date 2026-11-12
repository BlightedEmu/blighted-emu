// SPDX-License-Identifier: AGPL-3.0-or-later
//! The season pass state of a character (world migration 0041): its redeemed rewards, its
//! tracked stats and its activity card. It is part of the progression ([`crate::SavedProgression`]):
//! it loads and saves with it, in the same transaction. A character without a row has the
//! default state (no season yet).

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::from_column;
use crate::travel::{id_column, id_value};

/// A character's season pass state.
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

/// A character's activity card.
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

/// The columns of a `character_season` row.
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

/// The season state of a character; the default one when it has no row.
pub(crate) async fn character_season(
    world: &PgPool,
    character_id: Uuid,
) -> Result<SavedSeason, DbError> {
    let row: Option<SeasonColumns> = sqlx::query_as(
        "SELECT season_id, redeemed, stat_ids, stat_values, card_template, card_tasks, \
         card_claimed, card_stamped, card_wild_stamped, wild_stamps, rerolls, cards_done, \
         card_stat_ids, card_stat_values, first_connect_ns \
         FROM character_season WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    row.map_or_else(|| Ok(SavedSeason::default()), season_of)
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

/// Saves `season` as the whole season state of a character whose progression row the
/// transaction already updated; a state of no season removes the row.
pub(crate) async fn save_character_season(
    transaction: &mut Transaction<'_, Postgres>,
    character_id: Uuid,
    season: &SavedSeason,
) -> Result<(), DbError> {
    if season == &SavedSeason::default() {
        sqlx::query("DELETE FROM character_season WHERE character_id = $1")
            .bind(character_id)
            .execute(&mut **transaction)
            .await?;
        return Ok(());
    }
    let (stat_ids, stat_values) = stat_columns(&season.stats);
    let (card_stat_ids, card_stat_values) = stat_columns(&season.card.stats);
    let card = &season.card;
    sqlx::query(
        "INSERT INTO character_season (character_id, season_id, redeemed, stat_ids, stat_values, \
         card_template, card_tasks, card_claimed, card_stamped, card_wild_stamped, wild_stamps, \
         rerolls, cards_done, card_stat_ids, card_stat_values, first_connect_ns) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) \
         ON CONFLICT (character_id) DO UPDATE SET \
         season_id = $2, redeemed = $3, stat_ids = $4, stat_values = $5, card_template = $6, \
         card_tasks = $7, card_claimed = $8, card_stamped = $9, card_wild_stamped = $10, \
         wild_stamps = $11, rerolls = $12, cards_done = $13, card_stat_ids = $14, \
         card_stat_values = $15, first_connect_ns = $16",
    )
    .bind(character_id)
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
    .execute(&mut **transaction)
    .await?;
    Ok(())
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
