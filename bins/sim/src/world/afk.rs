// SPDX-License-Identifier: AGPL-3.0-or-later
//! The AFK rule ([`nw_content::afk_modes`]): a player that stands still for the warn time of
//! its rule gets a warning (2139) with the seconds left, a player that moves the rule's
//! distance after a warning gets the message that takes it back, and a player that stands
//! still for the disconnect time is removed (the server loop sends it the AFK rejection 1630
//! and closes its connection). The rule is the one of the sim's game mode (an instance), or the
//! default one (the open world).
//!
//! Observed (retail 2139): the warning and its end. Observed (client `FUN_146455c70` and
//! `lyshineui/popup/uierrors.luac`): the rejection 1630 with reason 13 is the client error
//! `Errors::aoi_afk_kicked`, whose popup is `@mm_rejected_Client_Kicked_AFK`; no kick 271 reason
//! gives it (`FUN_146447430`). Inferred: only a move ends the still time (the rule's column is
//! `ResetMinMove`; no capture shows a player who acts without moving); no capture holds an AFK
//! removal.

use nw_content::afk_modes::AfkMode;
use nw_messages::{AfkWarningMsg, ServerMessage};
use nw_sim_core::Position;

use super::{Body, Player, SimWorld, send_now};

const MS_PER_SECOND: f32 = 1_000.0;
/// The rule of a 3v3 arena round (states 3 and 5 of the match, [`super::arena_match`]):
/// Observed, the four retail warnings of an arena (`3v3` 528.310 s, 554.441 s, 587.641 s in a
/// round, `arena-4165c8761c29c499` 27.047 s in an overtime) carried 15.0 s left, the row's 30 -
/// 15; outside a round the game mode's row `3v3Arena` holds.
const ARENA_ROUND_ROW: &str = "3v3ArenaRound";

/// What the AFK rule knows of one player.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct AfkWatch {
    /// Where its still time started, and when (sim ms); `None` until its first tick.
    still: Option<(Position, u64)>,
    /// Its client holds a warning.
    warned: bool,
}

/// What a tick of the rule does for one player.
#[derive(Debug, Clone, Copy, PartialEq)]
enum AfkStep {
    Nothing,
    /// Warn it: this many seconds are left.
    Warn(f32),
    /// Take the warning back.
    Clear,
    /// Remove it.
    Remove,
}

impl AfkWatch {
    /// Follows the player at `at` at sim ms `now_ms` under `rule`.
    fn step(&mut self, rule: AfkMode, at: Position, now_ms: u64) -> AfkStep {
        let moved = self.still.is_none_or(|(from, _)| {
            let (east, north, up) = (at.x - from.x, at.y - from.y, at.z - from.z);
            (east * east + north * north + up * up).sqrt() >= rule.reset_min_move_m
        });
        if moved {
            self.still = Some((at, now_ms));
            return if std::mem::take(&mut self.warned) {
                AfkStep::Clear
            } else {
                AfkStep::Nothing
            };
        }
        let still_ms = self
            .still
            .map_or(0, |(_, since_ms)| now_ms.saturating_sub(since_ms));
        if still_ms >= rule.disconnect_ms {
            // A removal that fails (the player is on its way out) is not asked for again.
            self.still = None;
            self.warned = false;
            AfkStep::Remove
        } else if still_ms >= rule.warn_ms && !self.warned {
            self.warned = true;
            AfkStep::Warn((rule.disconnect_ms - still_ms) as f32 / MS_PER_SECOND)
        } else {
            AfkStep::Nothing
        }
    }
}

impl SimWorld {
    /// Runs the AFK rule for every player at sim ms `now_ms`.
    pub(super) fn tick_afk(&mut self, now_ms: u64) {
        let game_mode = self.instance_run.as_ref().map(|run| run.game_mode());
        let round = self
            .in_arena_round()
            .then(|| self.content.afk_modes.of_row(ARENA_ROUND_ROW))
            .flatten();
        let Some(rule) = round.or_else(|| self.content.afk_modes.of_game_mode(game_mode)) else {
            return;
        };
        let mut query = self.world.query::<(&mut Player, &Body)>();
        for (mut player, body) in query.iter_mut(&mut self.world) {
            let message = match player.afk.step(rule, body.motion.position(), now_ms) {
                AfkStep::Nothing => continue,
                AfkStep::Warn(seconds_left) => AfkWarningMsg::warning(player.number, seconds_left),
                AfkStep::Clear => AfkWarningMsg::cleared(player.number),
                AfkStep::Remove => {
                    tracing::info!(player = player.number, "player stood still too long");
                    self.afk_removals.push(player.number);
                    continue;
                }
            };
            send_now(
                &mut self.combat_messages,
                &player.connection,
                &[ServerMessage::AfkWarning(message)],
            );
        }
    }

    /// The players the AFK rule removes: the server loop kicks each.
    pub fn take_afk_removals(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.afk_removals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The retail `Expedition` row.
    const RULE: AfkMode = AfkMode {
        warn_ms: 180_000,
        disconnect_ms: 300_000,
        reset_min_move_m: 1.0,
    };
    const START_MS: u64 = 10_000;
    const HERE: Position = Position {
        x: 100.0,
        y: 200.0,
        z: 30.0,
    };

    #[test]
    fn a_still_player_is_warned_once_then_removed() {
        let mut watch = AfkWatch::default();
        assert_eq!(watch.step(RULE, HERE, START_MS), AfkStep::Nothing);
        assert_eq!(
            watch.step(RULE, HERE, START_MS + RULE.warn_ms - 1),
            AfkStep::Nothing
        );
        // Retail: 120 s left at the warning of an expedition.
        assert_eq!(
            watch.step(RULE, HERE, START_MS + RULE.warn_ms),
            AfkStep::Warn(120.0)
        );
        assert_eq!(
            watch.step(RULE, HERE, START_MS + RULE.warn_ms + 1),
            AfkStep::Nothing
        );
        assert_eq!(
            watch.step(RULE, HERE, START_MS + RULE.disconnect_ms),
            AfkStep::Remove
        );
    }

    #[test]
    fn a_move_of_the_rule_distance_takes_the_warning_back() {
        let mut watch = AfkWatch::default();
        watch.step(RULE, HERE, START_MS);
        let warned_ms = START_MS + RULE.warn_ms;
        assert!(matches!(
            watch.step(RULE, HERE, warned_ms),
            AfkStep::Warn(_)
        ));
        // Less than the rule's distance is no move.
        let near = Position {
            x: HERE.x + 0.5,
            ..HERE
        };
        assert_eq!(watch.step(RULE, near, warned_ms + 1), AfkStep::Nothing);
        let away = Position {
            x: HERE.x + 1.0,
            ..HERE
        };
        assert_eq!(watch.step(RULE, away, warned_ms + 2), AfkStep::Clear);
        // The still time starts again at the new place.
        assert_eq!(
            watch.step(RULE, away, warned_ms + 2 + RULE.warn_ms - 1),
            AfkStep::Nothing
        );
    }
}
