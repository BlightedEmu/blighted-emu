// SPDX-License-Identifier: AGPL-3.0-or-later
//! The secret doors of a catacombs run and the wall torch levers that open them, run as the
//! instance's gates ([`super::super::gates`]).
//!
//! Observed (`CATACOMBS2`, the one retail pull): a tile with the secret slots spawns the hidden
//! gate (`catacombs_gate_hidden`) with its gatherable at 4 (closed), its open timeline unplayed
//! and its close timeline at its end, and the lever (`catacombs_lever_wall_torch`) with its
//! gatherable at 4 and its timeline unplayed. The player's pull is a gather of the lever's
//! gatherable `Raid_CutlassKeys00_SplitSwitch` (the slice's gatherable id; the client's requests
//! name its CRC `639022248` from 431.525 s). In the tick the pull began (431.572 s) the gate,
//! 4 m from the lever, went to 8 and played its open timeline (431.605 s); at the pull's end
//! (433.574 s, 2 s later) the lever went to 8 and played its own timeline (433.608 s), whose
//! end came 3.325 s later (436.938 s).
//!
//! Each lever opens the door of its own letter in its room (`SecretLeverA` the door
//! `SecretDoorA`; Inferred from the slots: in `tile1_ba` each lever stands 2 m from the door of
//! its letter and 25 m from the other).

use nw_content::dungeon_gates::{
    Entry, Gate, GateAction, GateLevel, GateRule, GateRuleSet, LevelRules, TimelineRole, Trigger,
};
use nw_messages::{DEPLETED_GATHERABLE, HEALTHY_GATHERABLE};
use nw_world::catacomb_tiles::PropGate;

use super::PlacedPiece;

/// The slot name start of a secret door.
const DOOR_SLOT: &str = "SecretDoor";
/// The slot name start of a secret door's lever.
const LEVER_SLOT: &str = "SecretLever";
/// What the gates of a run name as their level script (a run has none of its own).
const RUN_SCRIPT: &str = "catacombs run";

/// A secret door or lever of a run's room, placed in the world.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SecretProp {
    /// Its room, by index in the layout.
    pub(super) room: usize,
    /// Its slot (`SecretDoorA`, `SecretLeverB`).
    pub(super) slot: String,
    /// Its entity.
    pub(super) piece: PlacedPiece,
    /// Its gatherable and timelines.
    pub(super) gate: PropGate,
}

/// Whether the slot `slot` holds a secret door or its lever.
pub(super) fn is_secret_gate_slot(slot: &str) -> bool {
    slot.starts_with(DOOR_SLOT) || slot.starts_with(LEVER_SLOT)
}

/// The gate name of `prop`: its slot and its room.
fn gate_name(slot: &str, room: usize) -> String {
    format!("{slot}@{room}")
}

/// The gate of `prop`, the `index`th secret gate of its run.
fn gate_of(index: usize, prop: &SecretProp) -> Gate {
    Gate {
        name: gate_name(&prop.slot, prop.room),
        entity: u64::try_from(index).unwrap_or(u64::MAX),
        slice: prop.piece.slice.clone(),
        asset: prop.piece.asset,
        position: prop.piece.position,
        yaw_deg: prop.piece.yaw_deg,
        position_component: Some(prop.piece.position_component),
        gatherable_component: Some(prop.gate.gatherable_component),
        timelines: prop.gate.timelines.clone(),
        interact_component: prop.gate.interact_component,
    }
}

/// Every secret door and lever of `props` as the gates of the run, with their rules: each
/// stands closed (4); a lever's pull start opens its door (8, the door's open timeline); its end
/// moves the lever (8, its own timeline). A door without its lever stays closed and a lever
/// without its door only moves (Inferred).
pub(super) fn secret_gates(props: &[SecretProp]) -> (GateLevel, LevelRules) {
    let gates: Vec<Gate> = props
        .iter()
        .enumerate()
        .map(|(index, prop)| gate_of(index, prop))
        .collect();
    let mut rule_sets: Vec<GateRuleSet> = gates
        .iter()
        .map(|gate| GateRuleSet {
            gate: gate.name.clone(),
            entry: Entry::Present {
                state: Some(HEALTHY_GATHERABLE),
            },
            rules: Vec::new(),
        })
        .collect();
    for (lever, prop) in props.iter().enumerate() {
        let Some(letter) = prop.slot.strip_prefix(LEVER_SLOT) else {
            continue;
        };
        let switch = gates[lever].name.clone();
        rule_sets[lever].rules.push(GateRule {
            when: Trigger::SwitchUsed {
                switch: switch.clone(),
            },
            then: GateAction::Set {
                state: DEPLETED_GATHERABLE,
                timeline: gates[lever]
                    .timelines
                    .first()
                    .map(|timeline| timeline.name.clone()),
            },
        });
        let door = props.iter().position(|other| {
            other.room == prop.room && other.slot.strip_prefix(DOOR_SLOT) == Some(letter)
        });
        if let Some(door) = door {
            rule_sets[door].rules.push(GateRule {
                when: Trigger::SwitchStarted { switch },
                then: GateAction::Set {
                    state: DEPLETED_GATHERABLE,
                    timeline: gates[door]
                        .timeline(TimelineRole::Open)
                        .map(|timeline| timeline.name.clone()),
                },
            });
        }
    }
    (
        GateLevel {
            script_slice: RUN_SCRIPT.to_owned(),
            gates,
            ..GateLevel::default()
        },
        LevelRules {
            script_slice: RUN_SCRIPT.to_owned(),
            gates: rule_sets,
            ..LevelRules::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use nw_content::dungeon_gates::GateTimeline;
    use nw_messages::timeline_state::TimelineState;

    use super::super::super::gates::{GateChange, LevelGates, gatherable_member, timeline_member};
    use super::*;

    const NOW_MS: u64 = 431_572;
    /// The lever's pull: 2 s (`CATACOMBS2` 431.572 to 433.574 s).
    const PULL_MS: u64 = 2_000;
    /// The lever timeline's length (433.608 to 436.938 s).
    const LEVER_S: f32 = 3.325;
    const LEVER_MS: u64 = 3_325;
    /// The hidden gate's timelines (its close timeline stood at its end, 6.865 s, at the spawn).
    const GATE_S: f32 = 6.865;
    const GATHERABLE: u64 = 2;
    const OPEN_TIMELINE: u64 = 3;
    const CLOSE_TIMELINE: u64 = 4;
    const LEVER_TIMELINE: u64 = 3;
    /// The lever's interact component (retail's 4, `CATACOMBS2` 431.572 s).
    const LEVER_INTERACT: u64 = 4;
    const NAMESPACE: u64 = 9;

    fn prop(room: usize, slot: &str, timelines: Vec<GateTimeline>) -> SecretProp {
        SecretProp {
            room,
            slot: slot.to_owned(),
            piece: PlacedPiece {
                slice: format!("slices/test/{slot}.dynamicslice"),
                asset: [7; 16],
                position_component: 1,
                position: [100.0, 200.0, 120.0],
                yaw_deg: 90.0,
            },
            gate: PropGate {
                gatherable_component: GATHERABLE,
                timelines,
                interact_component: slot.starts_with(LEVER_SLOT).then_some(LEVER_INTERACT),
            },
        }
    }

    fn door(room: usize, slot: &str) -> SecretProp {
        let timeline = |name: &str, component| GateTimeline {
            name: name.to_owned(),
            component,
            length_s: GATE_S,
        };
        prop(
            room,
            slot,
            vec![
                timeline("Timeline_Open", OPEN_TIMELINE),
                timeline("Timeline_Close", CLOSE_TIMELINE),
            ],
        )
    }

    fn lever(room: usize, slot: &str) -> SecretProp {
        prop(
            room,
            slot,
            vec![GateTimeline {
                name: "Handle".to_owned(),
                component: LEVER_TIMELINE,
                length_s: LEVER_S,
            }],
        )
    }

    /// The gates of `props` in the world, each with its index as its slot key.
    fn live(props: &[SecretProp]) -> (LevelGates, Vec<GateChange>) {
        let (level, rules) = secret_gates(props);
        let (mut gates, spawns) = LevelGates::new(&level, Some(&rules), NAMESPACE);
        for index in 0..props.len() {
            gates.set_number(index, Some(index as u64));
        }
        (gates, spawns)
    }

    /// Every door and lever stands closed at the run's start: gatherable 4, the door's close
    /// timeline at its end and its open timeline unplayed, the lever's timeline unplayed.
    #[test]
    fn the_secret_doors_and_levers_stand_closed_at_the_start() {
        let (_, spawns) = live(&[door(0, "SecretDoorA"), lever(0, "SecretLeverA")]);
        assert_eq!(spawns.len(), 2);
        let GateChange::Spawn { members, .. } = &spawns[0] else {
            panic!("a door spawn");
        };
        assert!(members.contains(&gatherable_member(GATHERABLE, HEALTHY_GATHERABLE)));
        let close_end = timeline_member(CLOSE_TIMELINE, &TimelineState::end(4, GATE_S));
        let open_unplayed = timeline_member(OPEN_TIMELINE, &TimelineState::unplayed(4));
        assert!(close_end.is_some_and(|member| members.contains(&member)));
        assert!(open_unplayed.is_some_and(|member| members.contains(&member)));
    }

    /// The pull's start opens the lever's door of its letter in its room (8 and the open
    /// timeline in that tick, at the retail version 6: 431.605 s) and gives the lever one
    /// interactor; its end turns the lever's interact state off and moves it (8, its timeline),
    /// whose end follows 3.325 s later at the retail version 7 (436.938 s); the other room's
    /// door and the other letter's door stay closed.
    #[test]
    fn a_lever_opens_its_door_at_the_pull_start_and_moves_at_its_end() {
        const DOOR_A: usize = 0;
        const LEVER_A: usize = 1;
        let props = [
            door(0, "SecretDoorA"),
            lever(0, "SecretLeverA"),
            door(0, "SecretDoorB"),
            door(1, "SecretDoorA"),
        ];
        let (mut gates, _) = live(&props);
        gates.switch_started(LEVER_A as u64);
        let opened = gates.step(&[], NOW_MS);
        assert_eq!(opened.len(), 2, "the door and the lever's interact state: {opened:?}");
        let Some(GateChange::Change { members, .. }) = opened
            .iter()
            .find(|change| matches!(change, GateChange::Change { gate, .. } if *gate == DOOR_A))
        else {
            panic!("the door changes");
        };
        assert!(members.contains(&gatherable_member(GATHERABLE, DEPLETED_GATHERABLE)));
        let open_start = timeline_member(OPEN_TIMELINE, &TimelineState::start(6));
        assert!(open_start.is_some_and(|member| members.contains(&member)));
        gates.switch_used(LEVER_A as u64);
        let pulled = gates.step(&[], NOW_MS + PULL_MS);
        let lever_changes: Vec<&GateChange> = pulled
            .iter()
            .filter(|change| matches!(change, GateChange::Change { gate, .. } if *gate == LEVER_A))
            .collect();
        assert_eq!(lever_changes.len(), 2, "its interact state, then its move: {pulled:?}");
        let GateChange::Change { members, .. } = lever_changes[1] else {
            panic!("a change");
        };
        assert!(members.contains(&gatherable_member(GATHERABLE, DEPLETED_GATHERABLE)));
        let ended = gates.step(&[], NOW_MS + PULL_MS + LEVER_MS);
        let lever_end = timeline_member(LEVER_TIMELINE, &TimelineState::end(7, LEVER_S));
        assert!(ended.iter().any(|change| matches!(
            change,
            GateChange::Change { gate, members } if *gate == LEVER_A
                && lever_end.as_ref().is_some_and(|end| members.contains(end))
        )));
        assert!(
            !ended
                .iter()
                .chain(&pulled)
                .any(|change| matches!(change, GateChange::Change { gate, .. } if *gate > LEVER_A)),
            "the other doors stay closed"
        );
    }
}
