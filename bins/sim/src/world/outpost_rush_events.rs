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
//! - The structures have no other event: these nine names are every event of the 12 ledgers;
//!   gate builds, upgrades and ruins show only in `outpoststructurestatuses` (`OPR_TEST3` gate
//!   1 tier 2 at 1032.951 s, ruined 1151.810 s, no event).
//!
//! Inferred: `commandpostupgraded` carries the outpost's index in its low byte as
//! `commandpostbuilt` does (only outpost A, index 0, was upgraded in a capture); a command post
//! rebuilt from its ruin keeps its tier and sends no event; `outpostattacked` comes when a foe of
//! the owner team hits one of the outpost's structures or starts to take the owned outpost.
//!
//! The sim sends `armorybuilt`, `bossspawned`, `bosskilled`, `commandpostbuilt`,
//! `commandpostupgraded` and `outpostattacked`. The corrupted portal's events wait for the
//! portal.

use nw_messages::game_mode_state::{EVENT_SLOTS, GameModeEvent};

/// `armorybuilt`.
pub(super) const ARMORY_BUILT: u32 = 0x9d69_f1fc;
/// `bossspawned`.
pub(super) const BOSS_SPAWNED: u32 = 0x3c26_75ad;
/// `bosskilled`.
pub(super) const BOSS_KILLED: u32 = 0xde64_0472;
/// `commandpostbuilt`.
pub(super) const COMMAND_POST_BUILT: u32 = 0x234b_695e;
/// `commandpostupgraded`.
pub(super) const COMMAND_POST_UPGRADED: u32 = 0x7782_3389;
/// `outpostattacked`.
pub(super) const OUTPOST_ATTACKED: u32 = 0x8d70_0253;
/// The shortest gap between two `outpostattacked` of one outpost, ms (Observed: 20.03 s).
const ATTACKED_GAP_MS: u64 = 20_000;
/// The shift of the new tier in `commandpostupgraded` (Observed: tier 2 is `20000`).
const UPGRADED_TIER_SHIFT: u32 = 16;
/// The command post's place in an outpost's structure status: its slot (1) times the three
/// bits of a slot (Observed, `outpoststructurestatusesa` 1052.250 to 1068.182 s).
const COMMAND_POST_STATUS_SHIFT: u32 = 3;
/// The tier bits of a structure's status.
const TIER_MASK: u64 = 0b11;
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

/// The command post events of the change of the outposts' structure statuses from `before` to
/// `after`: name and number, in outpost order.
pub(super) fn command_post_events(before: &[u64], after: &[u64]) -> Vec<(u32, u32)> {
    let tier = |status: u64| status >> COMMAND_POST_STATUS_SHIFT & TIER_MASK;
    before
        .iter()
        .zip(after)
        .zip(0u32..)
        .filter_map(|((before, after), outpost)| {
            let (old, new) = (tier(*before), tier(*after));
            if new <= old {
                return None;
            }
            Some(if old == 0 {
                (COMMAND_POST_BUILT, outpost)
            } else {
                // A tier of two bits.
                #[allow(clippy::cast_possible_truncation, reason = "a tier of two bits")]
                let new = new as u32;
                (COMMAND_POST_UPGRADED, new << UPGRADED_TIER_SHIFT | outpost)
            })
        })
        .collect()
}

/// When each outpost was last reported attacked, server ms.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct AttackWatch {
    last_ms: [Option<u64>; 3],
}

impl AttackWatch {
    /// Outpost `outpost` was attacked at `now_ms`: `true` when that is news (no report in the
    /// last [`ATTACKED_GAP_MS`]).
    pub(super) fn attacked(&mut self, outpost: usize, now_ms: u64) -> bool {
        let Some(last) = self.last_ms.get_mut(outpost) else {
            return false;
        };
        if last.is_some_and(|at| now_ms.saturating_sub(at) < ATTACKED_GAP_MS) {
            return false;
        }
        *last = Some(now_ms);
        true
    }
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
            ("commandpostbuilt", COMMAND_POST_BUILT),
            ("commandpostupgraded", COMMAND_POST_UPGRADED),
            ("outpostattacked", OUTPOST_ATTACKED),
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

    #[test]
    fn a_command_post_tier_rise_is_built_then_upgraded_as_retail() {
        // `OPR_TEST3` outpost A: tiers 1, 2 and 3 (1052.205 s `0`, 1060.082 s `20000`,
        // 1068.115 s `30000`); the armory's tier 1 stays.
        let status = |tier: u64| 1 | tier << COMMAND_POST_STATUS_SHIFT;
        assert_eq!(
            command_post_events(&[status(0), 0, 0], &[status(1), 0, 0]),
            vec![(COMMAND_POST_BUILT, 0)]
        );
        assert_eq!(
            command_post_events(&[status(1), 0, 0], &[status(2), 0, 0]),
            vec![(COMMAND_POST_UPGRADED, 0x2_0000)]
        );
        assert_eq!(
            command_post_events(&[0, 0, status(2)], &[0, 0, status(3)]),
            vec![(COMMAND_POST_UPGRADED, 0x3_0002)]
        );
        let ruined = status(2) | 0b100 << COMMAND_POST_STATUS_SHIFT;
        assert!(command_post_events(&[ruined, 0, 0], &[status(2), 0, 0]).is_empty(), "a repair");
        assert!(command_post_events(&[status(2), 0, 0], &[ruined, 0, 0]).is_empty(), "a ruin");
    }

    #[test]
    fn an_outpost_is_reported_attacked_at_most_once_in_twenty_seconds() {
        let mut watch = AttackWatch::default();
        assert!(watch.attacked(0, 729_037));
        assert!(!watch.attacked(0, 740_000));
        assert!(watch.attacked(2, 740_000), "each outpost has its own gap");
        assert!(watch.attacked(0, 749_070), "20.03 s later");
        assert!(!watch.attacked(3, 0), "no fourth outpost");
    }
}
