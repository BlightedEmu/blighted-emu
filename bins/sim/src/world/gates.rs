// SPDX-License-Identifier: AGPL-3.0-or-later
//! The gates, doors, light bridges, portal stones, switches and arena blockers of the running
//! instance ([`nw_content::dungeon_gates`]; dungeon gates note 2026-11-11).
//!
//! Observed (retail captures): each one is a network entity of its own that the players of the
//! instance see: a card of its slice, its position, its `GatherableControllerReplicatedState`
//! and one `TimelineComponentReplicatedState` per timeline of the slice, on the slice's own
//! component numbers. A closed gate spawns with gatherable 4, its close timeline stopped at its
//! end and its open timeline stopped at 0 (Amrine tall gate at 243.836). When it opens, the
//! server sends gatherable 8 and the open timeline's start body in the same tick, then the end
//! body the timeline's length later; a close is the same with 4 and the close timeline (Amrine
//! boss door 349.098 and 356.931). A change that comes before the last one's timeline ended
//! drops that end (the Amrine boss door's close end never went out: its open came 7.8 s into the
//! 8 s close). Starstone's arena blocker spawns with a card and a position when the boss fight
//! starts and its slot stops at the boss encounter's success.
//!
//! What changes a gate is the level script's server state machine, which no client data holds
//! (the rule source is Unknown). The sim runs the rules mined from the retail captures
//! ([`nw_content::dungeon_gates::GateRules`], Observed): an encounter's success, start or stage
//! start, a player's use of a switch, or a player coming into the boss encounter's volume (the
//! stand-in for the level script's boss area, Inferred). A close made by an encounter's start or
//! engage is undone when that encounter resets after a wipe (Inferred: the players must be able
//! to come back in).
//!
//! A level with no mined rule, and a gate of a mined level that no capture shows: every gate is
//! open from the start and no blocker spawns, so a run can never be blocked (Inferred). The
//! general rule "a gate opens when its nearest encounter succeeds, or when a switch nearer than
//! any encounter is used; the gate nearest the boss closes at its start and opens at its
//! success" reproduces 181 of the 984 mined changes of the non-switch gates (18.4 %, below the
//! 80 % bar; `gaterules.py` of the capture tools over 65 retail ledgers of 28 level scripts), so
//! it is not used.

use nw_content::dungeon_gates::{
    Entry, Gate, GateAction, GateLevel, GateRule, LevelRules, TimelineRole, Trigger,
};
use nw_messages::timeline_state::{TIMELINE_TYPE_INDEX, TimelineState};
use nw_messages::{
    BundleMember, DEPLETED_GATHERABLE, GatherableControllerState, HEALTHY_GATHERABLE, MemberState,
    RAW_ID_LEN,
};
use nw_sim_core::Position;

use super::SimWorld;
use super::crafting::{Station, dynamic_ref};
use super::dungeon_script::DungeonRun;
use super::encounter_roots::{
    STATUS_IN_PROGRESS, STATUS_NOT_STARTED, STATUS_SUCCEEDED, card, place_member,
};

/// The namespace of the `GdeRef`s of gate entities ("gatesdor").
const GATE_NAMESPACE: u64 = 0x6761_7465_7364_6f72;
/// The version of a gate's timeline lists at its spawn (Observed: 4 on a closed Amrine tall gate
/// at 243.836; Inferred for every gate).
const ENTRY_VERSION: u64 = 4;
/// Milliseconds per second.
const MS_PER_S: f32 = 1_000.0;
/// Half of an angle.
const HALF: f32 = 0.5;

/// What the level script did in one tick that a gate rule can name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ScriptEvent {
    /// The encounter of that name succeeded.
    EncounterSucceeded(String),
    /// The encounter started.
    EncounterStarted(String),
    /// A living player came into the running encounter's volume.
    EncounterEngaged(String),
    /// The running encounter reset (a wipe).
    EncounterReset(String),
    /// A stage of the encounter started: (encounter, stage).
    StageStarted(String, String),
    /// A player used the switch of that gate name.
    SwitchUsed(String),
}

/// A change of the gate entities for the sim to send.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum GateChange {
    /// Gate `gate` comes with its spawn members.
    Spawn {
        /// The gate's index in its level.
        gate: usize,
        /// Its card and states.
        members: Vec<BundleMember>,
    },
    /// The states of gate `gate` change.
    Change {
        /// The gate's index in its level.
        gate: usize,
        /// The changed states.
        members: Vec<BundleMember>,
    },
    /// Gate `gate`'s slot stops.
    Remove {
        /// The gate's index in its level.
        gate: usize,
    },
}

/// A timeline end that goes out later: (due time, component, length).
type TimelineEnd = (u64, u64, f32);

/// Which timeline plays with a gatherable change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Play<'a> {
    /// The timeline of that name, or none (a mined rule: retail played it, or none).
    Named(Option<&'a str>),
    /// The timeline of the new state's role ([`role_of`]).
    OfRole,
}

/// One gate of the running instance.
#[derive(Debug, Clone, PartialEq)]
struct LiveGate {
    def: Gate,
    gde_ref: [u8; RAW_ID_LEN],
    /// Its entity is in the world.
    present: bool,
    /// Its gatherable value.
    state: Option<u8>,
    /// The version of its timeline lists (one counter per entity).
    version: u64,
    end: Option<TimelineEnd>,
    /// The encounter whose start or engage closed it.
    closed_by: Option<String>,
    /// Its slot key in [`super::crafting::Stations`].
    number: Option<u64>,
}

/// The gatherable state member of component `component`.
fn gatherable_member(component: u64, state: u8) -> BundleMember {
    BundleMember {
        component,
        state: MemberState::GatherableController(GatherableControllerState {
            gatherable_state: Some(state),
        }),
    }
}

/// `seconds` in whole milliseconds (a timeline's length, a delay).
pub(super) fn seconds_ms(seconds: f32) -> u64 {
    // A length of at most a few minutes.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a length of a few seconds"
    )]
    let ms = (seconds.max(0.0) * MS_PER_S) as u64;
    ms
}

/// The 1647 member of component `component` in `state`; `None` (logged) when it cannot encode.
pub(super) fn timeline_member(component: u64, state: &TimelineState) -> Option<BundleMember> {
    match state.body() {
        Ok(body) => Some(BundleMember {
            component,
            state: MemberState::Recorded {
                class: TIMELINE_TYPE_INDEX,
                body,
            },
        }),
        Err(reason) => {
            tracing::warn!(component, %reason, "gate timeline body");
            None
        }
    }
}

/// The timeline role that plays when a gate takes `state`.
const fn role_of(state: u8) -> TimelineRole {
    match state {
        DEPLETED_GATHERABLE => TimelineRole::Open,
        HEALTHY_GATHERABLE => TimelineRole::Close,
        _ => TimelineRole::Other,
    }
}

/// Puts `changed` into `members`: each replaces the member of its component, or joins them.
pub(super) fn merge_members(members: &mut Vec<BundleMember>, changed: &[BundleMember]) {
    for change in changed {
        match members
            .iter_mut()
            .find(|member| member.component == change.component)
        {
            Some(member) => *member = change.clone(),
            None => members.push(change.clone()),
        }
    }
}

impl LiveGate {
    fn new(def: Gate, namespace: u64, index: usize) -> Self {
        Self {
            def,
            gde_ref: dynamic_ref(namespace, index),
            present: false,
            state: None,
            version: ENTRY_VERSION,
            end: None,
            closed_by: None,
            number: None,
        }
    }

    fn position(&self) -> Position {
        let [x, y, z] = self.def.position;
        Position { x, y, z }
    }

    /// Its spawn members: card, position, gatherable and every timeline, the one of its state
    /// stopped at its end and the others at 0.
    fn spawn_members(&self) -> Vec<BundleMember> {
        let mut members = vec![card(self.def.asset, self.gde_ref)];
        if let Some(component) = self.def.position_component {
            let half_yaw = self.def.yaw_deg.to_radians() * HALF;
            members.push(place_member(component, self.def.position, half_yaw));
        }
        if let (Some(component), Some(state)) = (self.def.gatherable_component, self.state) {
            members.push(gatherable_member(component, state));
        }
        let current = self.state.map(role_of);
        for timeline in &self.def.timelines {
            let state =
                if current == Some(timeline.role()) && timeline.role() != TimelineRole::Other {
                    TimelineState::end(self.version, timeline.length_s)
                } else {
                    TimelineState::unplayed(self.version)
                };
            members.extend(timeline_member(timeline.component, &state));
        }
        members
    }

    /// It takes gatherable `state` at `now_ms`, with the timeline `play` names; `None` when
    /// nothing changes.
    fn set(&mut self, state: u8, play: Play<'_>, now_ms: u64) -> Option<Vec<BundleMember>> {
        if !self.present || self.state == Some(state) {
            return None;
        }
        self.state = Some(state);
        let mut members: Vec<BundleMember> = self
            .def
            .gatherable_component
            .map(|component| gatherable_member(component, state))
            .into_iter()
            .collect();
        let played = match play {
            Play::Named(name) => {
                name.and_then(|name| self.def.timelines.iter().find(|t| t.name == name))
            }
            Play::OfRole => self.def.timeline(role_of(state)),
        };
        self.end = None;
        if let Some(played) = played {
            self.version += 1;
            members.extend(timeline_member(
                played.component,
                &TimelineState::start(self.version),
            ));
            self.end = Some((
                now_ms.saturating_add(seconds_ms(played.length_s)),
                played.component,
                played.length_s,
            ));
        }
        Some(members)
    }
}

/// Whether `trigger` names `event`.
fn fires(trigger: &Trigger, event: &ScriptEvent) -> bool {
    match (trigger, event) {
        (Trigger::EncounterSucceeded { encounter }, ScriptEvent::EncounterSucceeded(name))
        | (Trigger::EncounterStarted { encounter }, ScriptEvent::EncounterStarted(name))
        | (Trigger::EncounterEngaged { encounter }, ScriptEvent::EncounterEngaged(name)) => {
            encounter == name
        }
        (
            Trigger::StageStarted { encounter, stage },
            ScriptEvent::StageStarted(name, stage_name),
        ) => encounter == name && stage == stage_name,
        (Trigger::SwitchUsed { switch }, ScriptEvent::SwitchUsed(name)) => switch == name,
        _ => false,
    }
}

/// The encounter whose start or engage `trigger` names.
fn closing_encounter(trigger: &Trigger) -> Option<&str> {
    match trigger {
        Trigger::EncounterStarted { encounter } | Trigger::EncounterEngaged { encounter } => {
            Some(encounter)
        }
        _ => None,
    }
}

/// The gates of the running instance and their rules.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LevelGates {
    gates: Vec<LiveGate>,
    /// The rules: (gate index, rule).
    rules: Vec<(usize, GateRule)>,
    /// Switches used since the last step.
    switches_used: Vec<String>,
    /// The encounter statuses of the last step.
    watch: EncounterWatch,
}

impl LevelGates {
    /// The gates of `level` with the mined `rules` of its level script (`None`: every gate open
    /// from the start), their ids in `namespace`; and their entry spawns.
    pub(super) fn new(
        level: &GateLevel,
        rules: Option<&LevelRules>,
        namespace: u64,
    ) -> (Self, Vec<GateChange>) {
        let mut gates: Vec<LiveGate> = level
            .gates
            .iter()
            .enumerate()
            .map(|(index, gate)| LiveGate::new(gate.clone(), namespace, index))
            .collect();
        let mut all_rules = Vec::new();
        let mut spawns = Vec::new();
        for (index, gate) in gates.iter_mut().enumerate() {
            let mined = rules.and_then(|rules| rules.of_gate(&gate.def.name));
            let entry = match mined {
                Some(set) => set.entry,
                None if gate.def.gatherable_component.is_some() => Entry::Present {
                    state: Some(DEPLETED_GATHERABLE),
                },
                None => Entry::Absent,
            };
            if let Some(set) = mined {
                all_rules.extend(set.rules.iter().map(|rule| (index, rule.clone())));
            }
            if let Entry::Present { state } = entry {
                gate.present = true;
                gate.state = state;
                spawns.push(GateChange::Spawn {
                    gate: index,
                    members: gate.spawn_members(),
                });
            }
        }
        let gates = Self {
            gates,
            rules: all_rules,
            switches_used: Vec::new(),
            watch: EncounterWatch::default(),
        };
        (gates, spawns)
    }

    /// Gate `gate`'s entity has slot key `number`.
    pub(super) fn set_number(&mut self, gate: usize, number: Option<u64>) {
        if let Some(live) = self.gates.get_mut(gate) {
            live.number = number;
        }
    }

    /// The slot key of gate `gate`.
    pub(super) fn number(&self, gate: usize) -> Option<u64> {
        self.gates.get(gate).and_then(|live| live.number)
    }

    /// The slot keys of every gate in the world.
    pub(super) fn numbers(&self) -> impl Iterator<Item = u64> + '_ {
        self.gates.iter().filter_map(|live| live.number)
    }

    /// The `GdeRef` and place of gate `gate`.
    pub(super) fn identity(&self, gate: usize) -> Option<([u8; RAW_ID_LEN], Position)> {
        self.gates
            .get(gate)
            .map(|live| (live.gde_ref, live.position()))
    }

    /// The slot key of the standing gate with `GdeRef` `gde_ref` within `reach_m` of
    /// `position` (a switch a player gathers).
    pub(super) fn switch_at(
        &self,
        gde_ref: &[u8; RAW_ID_LEN],
        position: Position,
        reach_m: f32,
    ) -> Option<u64> {
        self.gates
            .iter()
            .find(|live| {
                live.present
                    && live.gde_ref == *gde_ref
                    && live.position().horizontal_distance(position) <= reach_m
            })
            .and_then(|live| live.number)
    }

    /// A player used the gate of slot key `number` (its gather ended).
    pub(super) fn switch_used(&mut self, number: u64) {
        if let Some(live) = self.gates.iter().find(|live| live.number == Some(number)) {
            self.switches_used.push(live.def.name.clone());
        }
    }

    /// One step at `now_ms` with the script's `events` (and the switches used since the last
    /// step): the rules that fire, the reopenings after a reset, and the timeline ends due.
    pub(super) fn step(&mut self, events: &[ScriptEvent], now_ms: u64) -> Vec<GateChange> {
        let mut events = events.to_vec();
        events.extend(
            std::mem::take(&mut self.switches_used)
                .into_iter()
                .map(ScriptEvent::SwitchUsed),
        );
        let mut changes = Vec::new();
        for event in &events {
            if let ScriptEvent::EncounterReset(name) = event {
                for (index, live) in self.gates.iter_mut().enumerate() {
                    if live.closed_by.as_deref() == Some(name.as_str()) {
                        live.closed_by = None;
                        if let Some(members) = live.set(DEPLETED_GATHERABLE, Play::OfRole, now_ms) {
                            changes.push(GateChange::Change {
                                gate: index,
                                members,
                            });
                        }
                    }
                }
            }
            for (index, rule) in &self.rules {
                if !fires(&rule.when, event) {
                    continue;
                }
                let Some(live) = self.gates.get_mut(*index) else {
                    continue;
                };
                match &rule.then {
                    GateAction::Set { state, timeline } => {
                        if let Some(members) =
                            live.set(*state, Play::Named(timeline.as_deref()), now_ms)
                        {
                            live.closed_by = (*state == HEALTHY_GATHERABLE)
                                .then(|| closing_encounter(&rule.when).map(str::to_owned))
                                .flatten();
                            changes.push(GateChange::Change {
                                gate: *index,
                                members,
                            });
                        }
                    }
                    GateAction::Spawn if !live.present => {
                        live.present = true;
                        live.state = live.def.gatherable_component.map(|_| HEALTHY_GATHERABLE);
                        live.closed_by = closing_encounter(&rule.when).map(str::to_owned);
                        changes.push(GateChange::Spawn {
                            gate: *index,
                            members: live.spawn_members(),
                        });
                    }
                    GateAction::Remove if live.present => {
                        live.present = false;
                        live.end = None;
                        live.closed_by = None;
                        changes.push(GateChange::Remove { gate: *index });
                    }
                    GateAction::Spawn | GateAction::Remove => {}
                }
            }
        }
        for (index, live) in self.gates.iter_mut().enumerate() {
            let Some((due_ms, component, length_s)) = live.end else {
                continue;
            };
            if now_ms < due_ms {
                continue;
            }
            live.end = None;
            live.version += 1;
            if let Some(member) =
                timeline_member(component, &TimelineState::end(live.version, length_s))
            {
                changes.push(GateChange::Change {
                    gate: index,
                    members: vec![member],
                });
            }
        }
        changes
    }
}

/// The encounter statuses a script showed at the last step, to tell what changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct EncounterWatch {
    managers: Vec<u32>,
    stages: Vec<Vec<u32>>,
    engaged: Vec<bool>,
}

impl EncounterWatch {
    /// What changed in `run` since the last call.
    pub(super) fn events(&mut self, run: &DungeonRun) -> Vec<ScriptEvent> {
        let encounters = &run.script().encounters;
        self.managers.resize(encounters.len(), STATUS_NOT_STARTED);
        self.stages.resize(encounters.len(), Vec::new());
        self.engaged.resize(encounters.len(), false);
        let mut events = Vec::new();
        for (index, encounter) in encounters.iter().enumerate() {
            let Some((manager, stages)) = run.client_status(index) else {
                continue;
            };
            let name = &encounter.name;
            let before = self.managers[index];
            if manager != before {
                match manager {
                    STATUS_IN_PROGRESS => events.push(ScriptEvent::EncounterStarted(name.clone())),
                    STATUS_SUCCEEDED => events.push(ScriptEvent::EncounterSucceeded(name.clone())),
                    STATUS_NOT_STARTED if before == STATUS_IN_PROGRESS => {
                        events.push(ScriptEvent::EncounterReset(name.clone()));
                    }
                    _ => {}
                }
            }
            let known = &self.stages[index];
            for (stage, status) in stages.iter().enumerate() {
                let was = known.get(stage).copied().unwrap_or(STATUS_NOT_STARTED);
                if *status == STATUS_IN_PROGRESS && was != STATUS_IN_PROGRESS {
                    let stage_name = encounter
                        .stages
                        .get(stage)
                        .map(|def| def.name.clone())
                        .unwrap_or_default();
                    events.push(ScriptEvent::StageStarted(name.clone(), stage_name));
                }
            }
            let engaged = manager == STATUS_IN_PROGRESS && run.engaged(index);
            if engaged && !self.engaged[index] {
                events.push(ScriptEvent::EncounterEngaged(name.clone()));
            }
            self.managers[index] = manager;
            self.stages[index] = stages;
            self.engaged[index] = engaged;
        }
        events
    }
}

impl SimWorld {
    /// The gates of game mode `game_mode`'s level for the instance whose ids take `salt`: the
    /// last instance's gates go, the new ones spawn in their entry state.
    pub(super) fn start_gates(&mut self, game_mode: Option<&str>, salt: u64) {
        self.stop_gates();
        let Some(level) = game_mode.and_then(|mode| self.content.dungeon_gates.of_mode(mode))
        else {
            return;
        };
        let rules = self
            .content
            .dungeon_gate_rules
            .of_level(&level.script_slice);
        tracing::info!(
            level = %level.script_slice,
            gates = level.gates.len(),
            mined = rules.is_some(),
            "dungeon gates"
        );
        let namespace = GATE_NAMESPACE ^ self.me.generation ^ salt;
        let (gates, spawns) = LevelGates::new(level, rules, namespace);
        self.dungeon_gates = Some(gates);
        self.apply_gate_changes(spawns);
    }

    /// The gates of the instance that ended go.
    pub(super) fn stop_gates(&mut self) {
        if let Some(gates) = self.dungeon_gates.take() {
            for number in gates.numbers() {
                self.stations.remove_camp(number);
            }
        }
    }

    /// The gates' part of the tick at `now_ms`, after `run`'s step.
    pub(super) fn tick_gates(&mut self, run: &DungeonRun, now_ms: u64) {
        let Some(mut gates) = self.dungeon_gates.take() else {
            return;
        };
        let events = gates.watch.events(run);
        let changes = gates.step(&events, now_ms);
        self.dungeon_gates = Some(gates);
        self.apply_gate_changes(changes);
    }

    /// The slot key of the switch with `GdeRef` `gde_ref` within `reach_m` of `position`.
    pub(super) fn gate_switch_at(
        &self,
        gde_ref: &[u8; RAW_ID_LEN],
        position: Position,
        reach_m: f32,
    ) -> Option<u64> {
        self.dungeon_gates
            .as_ref()
            .and_then(|gates| gates.switch_at(gde_ref, position, reach_m))
    }

    /// A player's gather of the gate of slot key `number` ended.
    pub(super) fn gate_switch_used(&mut self, number: u64) {
        if let Some(gates) = &mut self.dungeon_gates {
            gates.switch_used(number);
        }
    }

    fn apply_gate_changes(&mut self, changes: Vec<GateChange>) {
        for change in changes {
            let Some(gates) = &mut self.dungeon_gates else {
                return;
            };
            match change {
                GateChange::Spawn { gate, members } => {
                    let Some((gde_ref, position)) = gates.identity(gate) else {
                        continue;
                    };
                    let number = self.stations.place_camp(|_| Station {
                        gde_ref,
                        position,
                        station_type: String::new(),
                        name: Vec::new(),
                        members,
                    });
                    gates.set_number(gate, Some(number));
                }
                GateChange::Change { gate, members } => {
                    if let Some(number) = gates.number(gate) {
                        self.stations.change_camp(
                            number,
                            |spawn| merge_members(spawn, &members),
                            members.clone(),
                        );
                    }
                }
                GateChange::Remove { gate } => {
                    if let Some(number) = gates.number(gate) {
                        self.stations.remove_camp(number);
                    }
                    gates.set_number(gate, None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_content::dungeon_gates::GateRuleSet;
    use nw_content::dungeon_gates::GateTimeline;

    use super::super::crafting::{CARD_COMPONENT, Stations};
    use super::super::slots::Slot;
    use super::*;

    const NOW: u64 = 10_000;
    const GATE_S: f32 = 8.0;
    const GATE_MS: u64 = 8_000;
    const NAMESPACE: u64 = 7;
    const POSITION: u64 = 1;
    const GATHERABLE: u64 = 2;
    const CLOSE_TIMELINE: u64 = 3;
    const OPEN_TIMELINE: u64 = 4;
    /// An Amrine-shaped made-up level: an exit door, the boss door and the portal stone.
    const EXIT_DOOR: &str = "ChamberExitDoor";
    const BOSS_DOOR: &str = "ObeliskEntryDoor";
    const PORTAL: &str = "PortalStoneArt";
    const CHAMBER: &str = "EncounterChamber";
    const BOSS: &str = "EncounterBoss";

    fn gate(name: &str, at: [f32; 3]) -> Gate {
        Gate {
            name: name.to_owned(),
            entity: 1,
            slice: "slices/test/gate.dynamicslice".to_owned(),
            asset: [9; 16],
            position: at,
            yaw_deg: 90.0,
            position_component: Some(POSITION),
            gatherable_component: Some(GATHERABLE),
            timelines: vec![
                GateTimeline {
                    name: "Timeline_Close".to_owned(),
                    component: CLOSE_TIMELINE,
                    length_s: GATE_S,
                },
                GateTimeline {
                    name: "Timeline_Open".to_owned(),
                    component: OPEN_TIMELINE,
                    length_s: GATE_S,
                },
            ],
        }
    }

    fn level() -> GateLevel {
        GateLevel {
            script_slice: "slices/dungeon/levelscripts/test.dynamicslice".to_owned(),
            level: "nw_dungeon_test_00".to_owned(),
            game_modes: vec!["DungeonTest".to_owned()],
            gates: vec![
                gate(EXIT_DOOR, [895.5, 797.3, 145.2]),
                gate(BOSS_DOOR, [899.7, 861.6, 141.8]),
                gate(PORTAL, [868.2, 891.9, 142.6]),
            ],
        }
    }

    fn set(state: u8, timeline: &str) -> GateAction {
        GateAction::Set {
            state,
            timeline: Some(timeline.to_owned()),
        }
    }

    fn rules() -> LevelRules {
        let succeeded = |encounter: &str| Trigger::EncounterSucceeded {
            encounter: encounter.to_owned(),
        };
        let closed = Entry::Present {
            state: Some(HEALTHY_GATHERABLE),
        };
        let rule = |when, then| GateRule { when, then };
        LevelRules {
            script_slice: level().script_slice,
            ledgers: vec!["test".to_owned()],
            gates: vec![
                GateRuleSet {
                    gate: EXIT_DOOR.to_owned(),
                    entry: closed,
                    rules: vec![rule(
                        succeeded(CHAMBER),
                        set(DEPLETED_GATHERABLE, "Timeline_Open"),
                    )],
                },
                GateRuleSet {
                    gate: BOSS_DOOR.to_owned(),
                    entry: closed,
                    rules: vec![
                        rule(
                            succeeded(CHAMBER),
                            set(DEPLETED_GATHERABLE, "Timeline_Open"),
                        ),
                        rule(
                            Trigger::EncounterEngaged {
                                encounter: BOSS.to_owned(),
                            },
                            set(HEALTHY_GATHERABLE, "Timeline_Close"),
                        ),
                        rule(succeeded(BOSS), set(DEPLETED_GATHERABLE, "Timeline_Open")),
                    ],
                },
                GateRuleSet {
                    gate: PORTAL.to_owned(),
                    entry: closed,
                    rules: vec![rule(
                        succeeded(BOSS),
                        set(DEPLETED_GATHERABLE, "Timeline_Open"),
                    )],
                },
            ],
        }
    }

    fn body_of(members: &[BundleMember], component: u64) -> Option<Vec<u8>> {
        members.iter().find_map(|member| match &member.state {
            MemberState::Recorded { body, .. } if member.component == component => {
                Some(body.clone())
            }
            _ => None,
        })
    }

    fn gatherable_of(members: &[BundleMember]) -> Option<u8> {
        members.iter().find_map(|member| match &member.state {
            MemberState::GatherableController(state) if member.component == GATHERABLE => {
                state.gatherable_state
            }
            _ => None,
        })
    }

    fn change_of(changes: &[GateChange], wanted: usize) -> Option<&[BundleMember]> {
        changes.iter().find_map(|change| match change {
            GateChange::Change { gate, members } if *gate == wanted => Some(members.as_slice()),
            _ => None,
        })
    }

    #[test]
    fn an_encounter_success_opens_its_door_in_the_same_tick_and_ends_the_timeline_later() {
        let rules = rules();
        let (mut gates, spawns) = LevelGates::new(&level(), Some(&rules), NAMESPACE);
        assert_eq!(spawns.len(), 3, "every mined gate spawns closed");
        let changes = gates.step(&[ScriptEvent::EncounterSucceeded(CHAMBER.to_owned())], NOW);
        let opened = change_of(&changes, 0).expect("the exit door changes");
        assert_eq!(gatherable_of(opened), Some(DEPLETED_GATHERABLE));
        assert_eq!(
            body_of(opened, OPEN_TIMELINE),
            Some(
                TimelineState::start(ENTRY_VERSION + 1)
                    .body()
                    .expect("start")
            )
        );
        assert!(
            change_of(&changes, 1).is_some(),
            "the boss door opens with it"
        );
        assert!(gates.step(&[], NOW + GATE_MS - 1).is_empty());
        let ended = gates.step(&[], NOW + GATE_MS);
        assert_eq!(
            body_of(change_of(&ended, 0).expect("end"), OPEN_TIMELINE),
            Some(
                TimelineState::end(ENTRY_VERSION + 2, GATE_S)
                    .body()
                    .expect("end")
            )
        );
    }

    #[test]
    fn the_boss_door_closes_at_engage_and_opens_with_the_portal_stone_at_success() {
        let rules = rules();
        let (mut gates, _) = LevelGates::new(&level(), Some(&rules), NAMESPACE);
        gates.step(&[ScriptEvent::EncounterSucceeded(CHAMBER.to_owned())], NOW);
        let closed = gates.step(&[ScriptEvent::EncounterEngaged(BOSS.to_owned())], NOW + 1);
        let door = change_of(&closed, 1).expect("the boss door closes");
        assert_eq!(gatherable_of(door), Some(HEALTHY_GATHERABLE));
        assert!(body_of(door, CLOSE_TIMELINE).is_some());
        let opened = gates.step(
            &[ScriptEvent::EncounterSucceeded(BOSS.to_owned())],
            NOW + GATE_MS,
        );
        assert_eq!(
            gatherable_of(change_of(&opened, 1).expect("door")),
            Some(DEPLETED_GATHERABLE)
        );
        assert_eq!(
            gatherable_of(change_of(&opened, 2).expect("portal stone")),
            Some(DEPLETED_GATHERABLE)
        );
        assert!(
            body_of(change_of(&opened, 1).expect("door"), CLOSE_TIMELINE).is_none(),
            "the close's end is dropped: the open came before it"
        );
    }

    #[test]
    fn a_reset_reopens_the_door_its_encounter_closed() {
        let rules = rules();
        let (mut gates, _) = LevelGates::new(&level(), Some(&rules), NAMESPACE);
        gates.step(&[ScriptEvent::EncounterSucceeded(CHAMBER.to_owned())], NOW);
        gates.step(&[ScriptEvent::EncounterEngaged(BOSS.to_owned())], NOW + 1);
        let reset = gates.step(&[ScriptEvent::EncounterReset(BOSS.to_owned())], NOW + 2);
        assert_eq!(
            gatherable_of(change_of(&reset, 1).expect("door")),
            Some(DEPLETED_GATHERABLE)
        );
    }

    #[test]
    fn a_level_without_mined_rules_is_open_from_the_start() {
        let (mut gates, spawns) = LevelGates::new(&level(), None, NAMESPACE);
        for spawn in &spawns {
            let GateChange::Spawn { members, .. } = spawn else {
                panic!("a spawn");
            };
            assert_eq!(gatherable_of(members), Some(DEPLETED_GATHERABLE));
        }
        assert!(
            gates
                .step(&[ScriptEvent::EncounterSucceeded(BOSS.to_owned())], NOW)
                .is_empty()
        );
    }

    #[test]
    fn a_new_viewer_gets_card_position_gatherable_and_timelines_on_the_slice_components() {
        let rules = rules();
        let (gates, spawns) = LevelGates::new(&level(), Some(&rules), NAMESPACE);
        let GateChange::Spawn { gate, members } = spawns[0].clone() else {
            panic!("a spawn");
        };
        let (gde_ref, position) = gates.identity(gate).expect("identity");
        let mut stations = Stations::default();
        let number = stations.place_camp(|_| Station {
            gde_ref,
            position,
            station_type: String::new(),
            name: Vec::new(),
            members,
        });
        let group = stations
            .group(
                number,
                Slot {
                    interest: 5,
                    new: true,
                },
            )
            .expect("the spawn group");
        let components: Vec<u64> = group
            .members
            .iter()
            .map(|member| member.component)
            .collect();
        assert_eq!(
            components,
            [
                CARD_COMPONENT,
                POSITION,
                GATHERABLE,
                CLOSE_TIMELINE,
                OPEN_TIMELINE
            ]
        );
        assert!(matches!(
            &group.members[0].state,
            MemberState::GdeMetadata(card) if card.asset.is_some_and(|asset| asset.id == [9; 16])
        ));
        assert!(matches!(group.members[1].state, MemberState::Position(_)));
        assert_eq!(gatherable_of(&group.members), Some(HEALTHY_GATHERABLE));
        // Retail closed tall gate (Amrine 243.836): close at its end, open at 0.
        assert_eq!(
            body_of(&group.members, CLOSE_TIMELINE),
            Some(
                TimelineState::end(ENTRY_VERSION, GATE_S)
                    .body()
                    .expect("end")
            )
        );
        assert_eq!(
            body_of(&group.members, OPEN_TIMELINE),
            Some(
                TimelineState::unplayed(ENTRY_VERSION)
                    .body()
                    .expect("unplayed")
            )
        );
    }

    #[test]
    fn an_arena_blocker_spawns_at_the_fight_stage_and_its_slot_stops_at_success() {
        const BLOCKER: &str = "BossArenaBlockerInivs";
        const FIGHT_STAGE: &str = "Phase 1 - Wait";
        let mut level = level();
        let mut blocker = gate(BLOCKER, [401.2, 575.0, 335.8]);
        blocker.gatherable_component = None;
        blocker.timelines.clear();
        level.gates.push(blocker);
        let mut rules = rules();
        rules.gates.push(GateRuleSet {
            gate: BLOCKER.to_owned(),
            entry: Entry::Absent,
            rules: vec![
                GateRule {
                    when: Trigger::StageStarted {
                        encounter: BOSS.to_owned(),
                        stage: FIGHT_STAGE.to_owned(),
                    },
                    then: GateAction::Spawn,
                },
                GateRule {
                    when: Trigger::EncounterSucceeded {
                        encounter: BOSS.to_owned(),
                    },
                    then: GateAction::Remove,
                },
            ],
        });
        let (mut gates, spawns) = LevelGates::new(&level, Some(&rules), NAMESPACE);
        assert_eq!(spawns.len(), 3, "the blocker waits for its rule");
        let spawned = gates.step(
            &[ScriptEvent::StageStarted(
                BOSS.to_owned(),
                FIGHT_STAGE.to_owned(),
            )],
            NOW,
        );
        let [GateChange::Spawn { gate: 3, members }] = spawned.as_slice() else {
            panic!("the blocker spawns: {spawned:?}");
        };
        assert_eq!(
            members
                .iter()
                .map(|member| member.component)
                .collect::<Vec<_>>(),
            [CARD_COMPONENT, POSITION],
            "Starstone form: card and position only"
        );
        gates.set_number(3, Some(43));
        let removed = gates.step(&[ScriptEvent::EncounterSucceeded(BOSS.to_owned())], NOW + 1);
        assert!(removed.contains(&GateChange::Remove { gate: 3 }));
    }

    #[test]
    fn a_switch_use_opens_what_names_it() {
        const SWITCH: &str = "PressureGlyphVestibule";
        let mut level = level();
        level.gates.push(gate(SWITCH, [855.1, 604.2, 141.0]));
        let mut rules = rules();
        rules.gates.push(GateRuleSet {
            gate: SWITCH.to_owned(),
            entry: Entry::Present {
                state: Some(HEALTHY_GATHERABLE),
            },
            rules: vec![GateRule {
                when: Trigger::SwitchUsed {
                    switch: SWITCH.to_owned(),
                },
                then: set(DEPLETED_GATHERABLE, "Timeline_Open"),
            }],
        });
        let (mut gates, _) = LevelGates::new(&level, Some(&rules), NAMESPACE);
        gates.set_number(3, Some(42));
        let (gde_ref, position) = gates.identity(3).expect("switch");
        assert_eq!(gates.switch_at(&gde_ref, position, 3.0), Some(42));
        gates.switch_used(42);
        let changes = gates.step(&[], NOW);
        assert_eq!(
            gatherable_of(change_of(&changes, 3).expect("the switch")),
            Some(DEPLETED_GATHERABLE)
        );
    }
}
