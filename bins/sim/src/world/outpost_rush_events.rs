// SPDX-License-Identifier: AGPL-3.0-or-later
//! The event feed of the Outpost Rush match (Outpost Rush study 2026-11-12, item 5 of
//! `local-data/notes/2026-11-12-big-systems-plan.md`): the events of the game mode state
//! (`Event1` to `Event10`, [`nw_messages::game_mode_state::GameModeEvent`]), their names and
//! their numbers.
//!
//! Observed (all 12 retail Outpost Rush ledgers of `local-data/capture-index.tsv`; seconds from
//! each ledger's first record; each name is the CRC-32 of the lowercase word, found by a search
//! over the words of the client's `lyshineui/hud/outpostrush/outpostrushhud.luac`, whose
//! banners are `@ui_outpost_rush_armory_built`, `_boss_spawned`, `_boss_killed`,
//! `_outpost_under_attack`, `_corrupted_portal_soon`, `_corrupted_portal_started`,
//! `_corrupted_portal_finished`, `_command_post_built` and `_command_post_upgrade`):
//!
//! - The feed is a ring of ten slots: each new event takes the next slot, after `Event10`
//!   `Event1` again (`opr_que_and_lost` 1289.679 s `Event10`, 1315.314 s `Event1`). An event's
//!   time is the server's ms since 2000-01-01, the unit of the timers (`portalsoon` 1 ms after
//!   the end of its timer `5543c08f`, `opr_que_and_lost` 935.442 s).
//! - `armorybuilt` (`9d69f1fc`): three events in the match's first state, all at the match's
//!   opening, numbered 0, 1 and 2 (8 of 8 ledgers that saw the opening state): the armory of
//!   each outpost at tier 1 (each `outpoststructurestatuses` value is 1 at the opening).
//! - `bossspawned` (`3c2675ad`), number 0: the boss spawned (16 ms before her card,
//!   `OPR---ISLAND-2` 662.607 s; also on Trench and Vertical, `opr_que_and_lost` 1265.479 s).
//! - `bosskilled` (`de640472`): the freeze's whole seconds shifted left by
//!   `OutpostRush_FrozenEventTeamIdxBits` (1, the client's constant table `FUN_143265fd0`: the
//!   same getter as `ORStructure_CommandPost` = 1) and the killing team in the low bit
//!   (`OPR_TEST3` 836.139 s `7b`: 61 s, team 1 killed, `frozenstatus` 0; 1463.566 s `f4`: 122 s,
//!   team 0 killed, `frozenstatus` 1, the freeze 122.08 s).
//! - `outpostattacked` (`8d700253`): the number is the outpost's index (0 A, 2 C; B was never
//!   attacked in a capture); at most one each 20 s for an outpost (the closest pair 20.03 s,
//!   `OPR_TEST3` 729.037 and 749.070 s); the outpost's gate fell soon after (A at 741.242 s, C at
//!   1236.140 s after 1225 s, `OPR_TEST3`).
//! - `portalsoon` (`8d4f3fa5`), `portalopen` (`7a15db45`) and `portalclose` (`c4d23672`), number
//!   0: the corrupted portal. `portalsoon` at the end of the timer `5543c08f` (the start + 270 s,
//!   and each close + 270 s: `opr_que_and_lost` 1818.325 to 2088.355 s, `OPR_TEST3` 869.501 to
//!   1139.510 s), `portalopen` at the end of the timer `7a0740b0` 30 s later, `portalclose` when
//!   its encounter ended (119 to 852 s after the opening).
//! - `commandpostbuilt` (`234b695e`) with the outpost's index and `commandpostupgraded`
//!   (`77823389`) with the new tier in the third byte (`OPR_TEST3` 1052.205 s `0`, 1060.082 s
//!   `20000`, 1068.115 s `30000`: outpost A's command post at tiers 1, 2 and 3 in
//!   `outpoststructurestatusesa`).
//!
//! The sim sends `armorybuilt`, `bossspawned` and `bosskilled`. Not built: the outposts'
//! structures, the corrupted portal and the attacks on structures, which the other events
//! report.

use nw_messages::game_mode_state::{EVENT_SLOTS, GameModeEvent};

/// `armorybuilt`.
pub(super) const ARMORY_BUILT: u32 = 0x9d69_f1fc;
/// `bossspawned`.
pub(super) const BOSS_SPAWNED: u32 = 0x3c26_75ad;
/// `bosskilled`.
pub(super) const BOSS_KILLED: u32 = 0xde64_0472;
/// The number of an event that carries none (Observed: `bossspawned` and the portal's events).
pub(super) const NO_NUMBER: u32 = 0;
/// `OutpostRush_FrozenEventTeamIdxBits` (Observed: 1).
const FROZEN_EVENT_TEAM_IDX_BITS: u32 = 1;
const MS_PER_S: u64 = 1_000;

/// The number of `bosskilled`: the freeze of `freeze_ms` in whole seconds and the killing team.
pub(super) fn boss_killed_number(freeze_ms: u64, killing_team: u8) -> u32 {
    let seconds = u32::try_from(freeze_ms / MS_PER_S).unwrap_or(u32::MAX >> 1);
    seconds << FROZEN_EVENT_TEAM_IDX_BITS | u32::from(killing_team.min(1))
}

/// The ring of one match: the events noted since the last state change, and the slot of the next.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct EventFeed {
    next_slot: u8,
    pending: Vec<(u8, GameModeEvent)>,
}

impl EventFeed {
    /// Notes event `name` at server ms `at_ms` with `number` in the next slot.
    pub(super) fn note(&mut self, name: u32, at_ms: u64, number: u32) {
        let slot = self.next_slot;
        self.next_slot = (slot + 1) % EVENT_SLOTS;
        self.pending.push((
            slot,
            GameModeEvent {
                name,
                at_ms,
                number,
            },
        ));
    }

    /// The events noted since the last call, for the next state change; a slot noted twice
    /// sends its last event.
    pub(super) fn take(&mut self) -> Vec<(u8, GameModeEvent)> {
        let mut events = std::mem::take(&mut self.pending);
        let mut index = 0;
        while index < events.len() {
            let slot = events[index].0;
            if events[index + 1..].iter().any(|(later, _)| *later == slot) {
                events.remove(index);
            } else {
                index += 1;
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::lowercase_crc32;

    use super::*;

    #[test]
    fn the_names_are_the_crcs_of_the_hud_words() {
        for (name, key) in [
            ("armorybuilt", ARMORY_BUILT),
            ("bossspawned", BOSS_SPAWNED),
            ("bosskilled", BOSS_KILLED),
        ] {
            assert_eq!(lowercase_crc32(name), key, "{name}");
        }
    }

    #[test]
    fn the_numbers_pack_as_retail() {
        // `OPR_TEST3` 836.139 s and 1463.566 s.
        assert_eq!(boss_killed_number(61_349, 1), 0x7b);
        assert_eq!(boss_killed_number(122_099, 0), 0xf4);
    }

    #[test]
    fn the_ring_fills_ten_slots_in_turn_then_starts_again() {
        let mut feed = EventFeed::default();
        for at in 0..u64::from(EVENT_SLOTS) {
            feed.note(ARMORY_BUILT, at, NO_NUMBER);
        }
        let slots: Vec<u8> = feed.take().iter().map(|(slot, _)| *slot).collect();
        assert_eq!(slots, (0..EVENT_SLOTS).collect::<Vec<_>>());
        assert!(feed.take().is_empty(), "taken once");
        feed.note(BOSS_KILLED, 99, 0x79);
        assert_eq!(
            feed.take(),
            vec![(
                0,
                GameModeEvent {
                    name: BOSS_KILLED,
                    at_ms: 99,
                    number: 0x79
                }
            )]
        );
        for at in 0..=u64::from(EVENT_SLOTS) {
            feed.note(BOSS_SPAWNED, at, NO_NUMBER);
        }
        let taken = feed.take();
        assert_eq!(
            taken.len(),
            usize::from(EVENT_SLOTS),
            "a slot noted twice sends once"
        );
        assert!(
            taken
                .iter()
                .any(|(slot, event)| *slot == 1 && event.at_ms == 10)
        );
    }
}
