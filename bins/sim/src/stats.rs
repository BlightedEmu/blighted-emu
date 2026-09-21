// SPDX-License-Identifier: AGPL-3.0-or-later
//! Leaderboard stats of the players (study note
//! `docs/re/2026-09-18-leaderboards-and-analytics.md`). The world records what a player did as
//! a [`StatEvent`]; the loop turns the events into values of the boards they feed (the boards of
//! `LeaderboardStatData`, named as the client names them) and adds them to the global database,
//! where `edge-http` serves them to the client's leaderboard screen.

use nw_content::leaderboards::{Aggregation, LeaderboardStats, RotationCalendar, board_name};
use nw_db::{StatMerge, StatUpdate};
use uuid::Uuid;

/// The stat type of a game event: `LeaderboardStatData` rows of this type name the event in
/// their `GameMode` column (`HercyneRaid_Reward` feeds the boss trial completion boards).
pub const GAME_EVENT_STAT: &str = "GameEvent";
/// One game event counts once (Inferred: the boards it feeds count completions).
pub const ONE_COMPLETION: f64 = 1.0;
/// The scope of a character's own boards; boards of companies, groups and factions need values
/// this server does not have yet.
const CHARACTER_SCOPE: &str = "Character";

/// Something a character did that a leaderboard stat may count.
#[derive(Debug, Clone, PartialEq)]
pub struct StatEvent {
    /// The character.
    pub character: Uuid,
    /// The `LeaderboardStatData` stat type, such as [`GAME_EVENT_STAT`].
    pub stat_type: &'static str,
    /// The `LeaderboardStatData` game mode; for a game event, its id.
    pub game_mode: String,
    /// The value.
    pub value: f64,
}

fn merge(aggregation: Aggregation) -> StatMerge {
    match aggregation {
        Aggregation::Sum => StatMerge::Add,
        Aggregation::Max => StatMerge::Greatest,
        Aggregation::Min => StatMerge::Least,
    }
}

/// The board values of `events` at `unix_seconds` in world `world_id`: one per character board
/// each event's stat feeds, in the board's current rotation. A board whose name needs a value
/// this server does not fill gets none.
pub fn stat_updates(
    stats: &LeaderboardStats,
    calendar: &RotationCalendar,
    world_id: Uuid,
    events: &[StatEvent],
    unix_seconds: i64,
) -> Vec<StatUpdate> {
    let world = world_id.to_string();
    events
        .iter()
        .flat_map(|event| {
            stats
                .boards(event.stat_type, &event.game_mode)
                .iter()
                .filter(|board| board.scope == CHARACTER_SCOPE)
                .filter_map(|board| {
                    let rotation = calendar.name(board.rotation, unix_seconds)?;
                    let stat_id = board_name(&board.template, &world, &rotation)?;
                    Some(StatUpdate {
                        namespace: board.namespace.clone(),
                        stat_id,
                        scope: board.scope.clone(),
                        entity_id: event.character.to_string(),
                        value: event.value,
                        merge: merge(board.aggregation),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use nw_common::time::CivilDate;
    use nw_content::generated::LeaderboardStatData;
    use nw_content::leaderboards::test_rows::{board_row as board, stat_row};

    use super::*;

    const HERCYNE: &str = "HercyneRaid_Reward";
    const WORLD: Uuid = Uuid::from_u128(0x8d0f_4c2e_0000_4000_8000_0000_0000_0001);
    const CHARACTER: Uuid = Uuid::from_u128(0x6feb_6367_0000_4000_8000_0000_0000_0002);
    /// 2026-06-30 09:53:59 UTC (the retail capture: `m41`, `s11`).
    const NOW: i64 = 1_782_813_239;
    const CALENDAR: RotationCalendar = RotationCalendar {
        start: CivilDate {
            year: 2023,
            month: 2,
            day: 7,
        },
        reset_hour: 2,
        season: 11,
    };

    fn stat(id: &str, rotation: &str, scope: &str) -> LeaderboardStatData {
        stat_row(id, GAME_EVENT_STAT, HERCYNE, "Sum", rotation, scope)
    }

    #[test]
    fn a_game_event_counts_on_the_character_boards_it_feeds() {
        let stats = LeaderboardStats::build(
            &[
                stat("MonthRaidCompletionsHercyne", "Month", CHARACTER_SCOPE),
                stat("SeasonRaidCompletionsHercyne", "Season", CHARACTER_SCOPE),
                stat("CompanyRaids", "Season", "Company"),
                stat("TerritoryRaids", "Season", CHARACTER_SCOPE),
            ],
            &[
                board(
                    "MonthRaidCompletionsHercyne",
                    "top-boss-trial-completions.HercyneRaid_Reward.{world}.{month}",
                ),
                board(
                    "SeasonRaidCompletionsHercyne",
                    "top-boss-trial-completions.HercyneRaid_Reward.{world}.{season}",
                ),
                board("CompanyRaids", "top-company-raids.{world}.{season}"),
                board("TerritoryRaids", "top-raids.{territory}.{season}"),
            ],
        );
        let events = [StatEvent {
            character: CHARACTER,
            stat_type: GAME_EVENT_STAT,
            game_mode: HERCYNE.to_lowercase(),
            value: ONE_COMPLETION,
        }];
        let updates = stat_updates(&stats, &CALENDAR, WORLD, &events, NOW);
        let world = WORLD.to_string();
        assert_eq!(
            updates,
            ["m41", "s11"].map(|rotation| StatUpdate {
                namespace: "pc".to_owned(),
                stat_id: format!(
                    "top-boss-trial-completions.HercyneRaid_Reward.{world}.{rotation}"
                ),
                scope: CHARACTER_SCOPE.to_owned(),
                entity_id: CHARACTER.to_string(),
                value: ONE_COMPLETION,
                merge: StatMerge::Add,
            })
        );
        let other = StatEvent {
            game_mode: "01_SkinningKnife_Success".to_owned(),
            ..events[0].clone()
        };
        assert!(stat_updates(&stats, &CALENDAR, WORLD, &[other], NOW).is_empty());
    }
}
