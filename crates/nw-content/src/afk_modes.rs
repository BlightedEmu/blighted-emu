// SPDX-License-Identifier: AGPL-3.0-or-later
//! The AFK rules (`AfkModeData`, `javelindata_afkmodes`): how long a player may stand still
//! before the server warns it (`WarnSeconds`) and before it removes it (`DisconnectSeconds`),
//! and the distance that counts as a move (`ResetMinMove`, metres). A game mode names its row
//! (`GameModeData.AfkModeId`).
//!
//! Observed (retail 2139 `AFKWarning`, 19 messages in 4 ledgers): the warning carries the
//! seconds left until the removal, 15.0 in a 3v3 arena round (`3v3ArenaRound`: 30 - 15) and
//! 120.0 in an expedition (`Expedition`: 300 - 180). Inferred: the open world and a game mode
//! with no row of its own use the row `Default` (a warning after 23 hours). `ScaleAfkTimerAmount`
//! is 1 and `UpdatePlayerPopulationCheckDelaySeconds` is 10 in every row; what they do is
//! Unknown, and they are not used.

use std::collections::HashMap;

use crate::Tables;

/// The row of the open world and of a game mode with no row of its own.
const DEFAULT_MODE: &str = "default";
const MS_PER_SECOND: u64 = 1_000;

/// One AFK rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AfkMode {
    /// The still time after which the player is warned.
    pub warn_ms: u64,
    /// The still time after which the player is removed.
    pub disconnect_ms: u64,
    /// The distance from where the still time started that ends it, in metres.
    pub reset_min_move_m: f32,
}

/// The AFK rules by lowercase `AfkModeId`, and the rule each game mode names.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AfkModes {
    by_id: HashMap<String, AfkMode>,
    by_game_mode: HashMap<String, String>,
}

impl AfkModes {
    /// The rules of `modes` (id, warn seconds, disconnect seconds, min move) and the rule id
    /// of each game mode of `game_modes`. A row that removes nobody (0 seconds, the retail
    /// `Disabled` row) or whose numbers are negative is no rule.
    pub fn new<'a>(
        modes: impl IntoIterator<Item = (&'a str, i64, i64, f64)>,
        game_modes: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let by_id = modes
            .into_iter()
            .filter_map(|(id, warn, disconnect, min_move)| {
                let warn = u64::try_from(warn).ok()?;
                let disconnect = u64::try_from(disconnect)
                    .ok()
                    .filter(|seconds| *seconds > 0)?;
                Some((
                    id.to_lowercase(),
                    AfkMode {
                        warn_ms: warn * MS_PER_SECOND,
                        disconnect_ms: disconnect * MS_PER_SECOND,
                        reset_min_move_m: min_move as f32,
                    },
                ))
            })
            .collect();
        let by_game_mode = game_modes
            .into_iter()
            .map(|(game_mode, id)| (game_mode.to_lowercase(), id.to_lowercase()))
            .collect();
        Self {
            by_id,
            by_game_mode,
        }
    }

    /// The rules of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::new(
            tables
                .afk_mode_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    (
                        row.afk_mode_id.as_str(),
                        row.warn_seconds,
                        row.disconnect_seconds,
                        row.reset_min_move,
                    )
                }),
            tables
                .game_mode_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| Some((row.game_mode_id.as_str(), row.afk_mode_id.as_deref()?))),
        )
    }

    /// The rule of row `id` (`AfkModeId`, any case); `None` when the row is no rule.
    pub fn of_row(&self, id: &str) -> Option<AfkMode> {
        self.by_id.get(&id.to_lowercase()).copied()
    }

    /// The rule of a player in game mode `game_mode` (`None`: the open world): the rule the
    /// game mode names when it has a row, else the default rule. `None` when no rule holds.
    pub fn of_game_mode(&self, game_mode: Option<&str>) -> Option<AfkMode> {
        game_mode
            .and_then(|game_mode| self.by_game_mode.get(&game_mode.to_lowercase()))
            .and_then(|id| self.by_id.get(id))
            .or_else(|| self.by_id.get(DEFAULT_MODE))
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The retail rows `Default`, `Disabled` and `Expedition`.
    fn modes() -> AfkModes {
        AfkModes::new(
            [
                ("Default", 82_800, 86_400, 0.01),
                ("Disabled", 0, 0, 0.0),
                ("Expedition", 180, 300, 1.0),
            ],
            [
                ("DungeonAmrine", "Expedition"),
                ("CaptureTheFlag", "CaptureTheFlag"),
            ],
        )
    }

    #[test]
    fn a_game_mode_takes_its_rule_and_the_rest_take_the_default() {
        let modes = modes();
        let expedition = modes.of_game_mode(Some("dungeonamrine")).expect("a rule");
        assert_eq!(
            (expedition.warn_ms, expedition.disconnect_ms),
            (180_000, 300_000)
        );
        let default = modes.of_game_mode(None).expect("the default rule");
        assert_eq!(default.disconnect_ms, 86_400_000);
        // A game mode with no rule of its own, or whose rule has no row.
        assert_eq!(modes.of_game_mode(Some("Duel")), Some(default));
        assert_eq!(modes.of_game_mode(Some("CaptureTheFlag")), Some(default));
    }

    /// Retail `3v3ArenaRound`: warn after 15 s, remove after 30 s (the 2139 of an arena round
    /// carried 15.0 s left).
    #[test]
    fn a_row_is_found_by_its_id_in_any_case() {
        let modes = AfkModes::new([("3v3ArenaRound", 15, 30, 1.0)], []);
        let round = modes.of_row("3V3ARENAROUND").expect("the round rule");
        assert_eq!((round.warn_ms, round.disconnect_ms), (15_000, 30_000));
        assert_eq!(modes.of_row("Missing"), None);
    }

    #[test]
    fn a_row_that_removes_nobody_is_no_rule() {
        let modes = AfkModes::new([("Default", 0, 0, 0.0)], []);
        assert_eq!(modes.of_game_mode(None), None);
    }
}
