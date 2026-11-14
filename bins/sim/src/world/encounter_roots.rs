// SPDX-License-Identifier: AGPL-3.0-or-later
//! The root groups of the open world's encounters (encounter states note, 2026-11-10): the
//! entity of each encounter slice that a player near it sees, with the encounter manager's
//! status (6786) and each stage's status history (2133).
//!
//! Observed (retail captures): a root group is the card of the encounter slice, the position,
//! the manager state `01 01 status:u32`, one stage state `01 01 count:u8 (sequence:u32
//! status:u32)*count` per `EncounterComponent` of the slice, and the states of the slice's other
//! replicated components (spawner, timelines, placement obstructions). Each component keeps its
//! number in every placement of one slice, so the table ([`nw_content::town_props`] in the
//! encounter roots file) holds the usual retail members of each slice, and this module rewrites
//! the position, the manager and the stages. The `k`-th stage state (by component number) is the
//! `k`-th `EncounterComponent` of the slice in stream order: the stage counts of all 240 placed
//! slices that retail shows match, and in one slice the finished, running and waiting waves had
//! 4, 3 and 2 history entries in that order. The status values are the client's
//! `eEncounterStatus_*` names in their order in our `NewWorld.exe` (Inferred: 0 to 4); a history
//! starts as retail's waiting stages do, `(0, 0) (1, 1)`.
//!
//! A stage's entity event on an entity of the encounter slice (dungeon gates note 2026-11-11,
//! Observed in the Dynasty Shipyard cave monolith) goes out on the root group, at the component
//! number of the target's component ([`EncounterRoots::entity_event`]): `Activate` on a
//! timeline starts it (1647, the root's next version), `Trigger` on a delayed event sends its
//! `totalDelayDuration` and, the delay later, its completion, and `Activate` or `Deactivate` on
//! a placement obstruction sets it to 1 or 0. Retail sent nothing for `Trigger` on a timeline
//! nor for `Activate` on a delayed event (4 of 4), and neither does the sim. Inferred: a started
//! timeline ends after its length as a gate's does ([`super::gates`]).
//!
//! The start objects ([`EncounterObjects`]) are the gatherable slices a stage spawns and its
//! destroyed objective counts (`Wave_01 - Objective: Interact IO`): retail sends each one a card
//! of its slice with a variation, position, gatherable controller, delayed event and placement
//! obstruction state (the encounter objects file, mined as the roots are). A player gathers it
//! as any gatherable ([`super::gathering`]); the gather ends it, and its stage counts it. The
//! table also holds objects a stage spawns that no player gathers (the holdout's solid door and
//! door animation; their checks are in `world_event_doors.rs`): they stand until a stage or the
//! encounter's end takes them away.

use std::collections::HashMap;

use nw_content::dungeon_gates::EntityComponent;
use nw_content::town_props::TownProps;
use nw_messages::position::{PositionInTheWorldReplicatedState, Rotation, WorldPosition};
use nw_messages::timeline_state::{TIMELINE_TYPE_INDEX, TimelineState};
use nw_messages::keyed_list::KeyedList;
use nw_messages::{
    AssetId, BundleGroup, BundleMember, DYNAMIC_SLICE_SUB_ID, DelayedEventState, FullList,
    GdeMetadataReplicatedState, InteractState, MemberState, OBJECTIVES_TYPE_INDEX,
    ObjectiveRecord, ObjectivesState, PlacementObstructionState, Reader, ReplicatedList,
    TaskEntry, TaskKey, TaskWireState, Writer,
};
use nw_sim_core::Position;
use nw_world::dungeon_scripts::{Encounter, EncounterTasks};

use super::crafting::{CARD_COMPONENT, dynamic_ref};
use super::dungeon_script::{
    EVENT_ACTIVATE, EVENT_DEACTIVATE, EVENT_TRIGGER, ObjectivePhase, TaskRun,
};
use super::unix_ns;
use super::gates::{merge_members, seconds_ms, timeline_member};
use super::slots::Slot;

/// The entity numbers of the encounter roots start here.
pub(super) const ENCOUNTER_ROOT_ENTITY_BASE: u64 = 1 << 43;
/// The entity numbers of the encounters' start objects start here.
pub(super) const ENCOUNTER_OBJECT_ENTITY_BASE: u64 = 1 << 44;
/// Names the ids this server makes for start objects.
const OBJECT_NAMESPACE: u64 = 0x656e_636f_626a_6563;
/// Names the ids this server makes for encounter roots.
const ROOT_NAMESPACE: u64 = 0x656e_636f_756e_7472;
/// `MB::PositionInTheWorldReplicatedState`.
const POSITION_CLASS: u64 = 13;
/// `MB::EncounterManagerComponentReplicatedState`.
pub(super) const MANAGER_CLASS: u64 = 6786;
/// `MB::EncounterComponentReplicatedState`.
pub(super) const STAGE_CLASS: u64 = 2133;
/// The slot byte and child mask of a manager or stage state.
const STATE_HEAD: [u8; 2] = [0x01, 0x01];
/// `eEncounterStatus_Uninitialized`.
pub(super) const STATUS_UNINITIALIZED: u32 = 0;
/// `eEncounterStatus_NotStarted`.
pub(super) const STATUS_NOT_STARTED: u32 = 1;
/// `eEncounterStatus_InProgress`.
pub(super) const STATUS_IN_PROGRESS: u32 = 2;
/// `eEncounterStatus_Succeeded`.
pub(super) const STATUS_SUCCEEDED: u32 = 3;
/// `eEncounterStatus_Failed`.
pub(super) const STATUS_FAILED: u32 = 4;
/// The most entries a stage history keeps (Observed: the corrupted breach ledgers of
/// 2026-10-28 send histories of up to 10 entries, the longest from sequence 150 to 159: the
/// stage's newest 10, as a ring).
const MAX_HISTORY: usize = 10;
/// The entity event changes a root keeps for its viewers (Inferred: a viewer catches up within a
/// tick; a stage sends at most a few at once).
const MAX_EVENT_CHANGES: usize = 16;
/// The completion of a delayed event (retail `0583310103000001`: no delay left, completed).
const DELAY_DONE: DelayedEventState = DelayedEventState {
    total_delay_s: Some(0.0),
    completed: Some(true),
};

/// The status history of one stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct History {
    /// Its entries, oldest first: (sequence, status).
    entries: Vec<(u32, u32)>,
    /// The sequence of the next entry.
    next: u32,
}

impl Default for History {
    fn default() -> Self {
        Self {
            entries: vec![(0, STATUS_UNINITIALIZED), (1, STATUS_NOT_STARTED)],
            next: 2,
        }
    }
}

impl History {
    /// Its newest status.
    pub(super) fn status(&self) -> u32 {
        self.entries
            .last()
            .map_or(STATUS_UNINITIALIZED, |entry| entry.1)
    }

    /// Adds `status` when it is not the newest; `true` when it changed.
    pub(super) fn set(&mut self, status: u32) -> bool {
        if self.status() == status {
            return false;
        }
        self.entries.push((self.next, status));
        self.next += 1;
        if self.entries.len() > MAX_HISTORY {
            self.entries.remove(0);
        }
        true
    }

    pub(super) fn body(&self) -> Vec<u8> {
        let mut body = STATE_HEAD.to_vec();
        body.push(u8::try_from(self.entries.len()).unwrap_or(u8::MAX));
        for (sequence, status) in &self.entries {
            body.extend_from_slice(&sequence.to_be_bytes());
            body.extend_from_slice(&status.to_be_bytes());
        }
        body
    }
}

/// The root group of one encounter.
#[derive(Debug, Clone, PartialEq)]
struct Root {
    /// The encounter's index in the open world script.
    encounter: usize,
    position: Position,
    /// The card, the position and the slice's other states, as retail sent them.
    members: Vec<BundleMember>,
    manager_component: u64,
    /// The stage states' component numbers, by stage.
    stage_components: Vec<u64>,
    manager: u32,
    stages: Vec<History>,
    /// The sim owns the encounter: the root is shown.
    shown: bool,
    /// Counts its state changes, so each player gets each one once.
    version: u32,
    /// The member changes of stage entity events, with the version each came with.
    changes: Vec<(u32, BundleMember)>,
    /// Its objective member (3857), when the encounter has tasks and its slice an objectives
    /// component.
    objective: Option<RootObjective>,
}

/// The objective of an encounter's root (encounter objectives note): the task tree of the
/// encounter and the member's lists. Observed: retail makes it when the first task's stage
/// starts (accept time then), lists each task entry from its first progress in that order, a
/// timed task's start time while it runs, and at the end the instance in the completed list
/// with every other list empty. One version counter counts the member's changes; a list sent
/// carries the version of its own last change (`NH-MSQ-6.3` 29.864 s: completed v7, the
/// others v8).
#[derive(Debug, Clone, PartialEq)]
struct RootObjective {
    /// The component number of the root's objectives component.
    component: u64,
    tasks: EncounterTasks,
    /// The objective id, `PoI<name>[<entity id>]` (client `FUN_143a2f6b0`; Inferred: the
    /// encounter's name and the root's entity number, retail uses its runtime id).
    objective: u32,
    /// The encounter's index, for the instance numbers.
    encounter: usize,
    /// How many objectives it made.
    made: u64,
    /// The running objective: its record and task entries; `None` before the first and after
    /// the end.
    live: Option<(ObjectiveRecord, Vec<TaskEntry>)>,
    /// The running timed tasks' starts (Unix ns).
    start_times: Vec<(TaskKey, u64)>,
    /// A start time was sent: the list goes out from then on, also empty.
    start_times_sent: bool,
    /// The instances of its ended objectives (the last one).
    completed: Vec<u64>,
    /// The member's change counter.
    version: u64,
    /// The version of each list's last change: start times, completed, active and tasks;
    /// tracked keeps the first version (it stays empty).
    versions: ListVersions,
}

/// The version of each list's last change ([`RootObjective::versions`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ListVersions {
    start_times: u64,
    tracked: u64,
    completed: u64,
    active: u64,
    tasks: u64,
}

/// The prefix of an objective id (`DAT_148259cd8`, client `FUN_143a2f6b0`).
const OBJECTIVE_ID_PREFIX: &str = "PoI";
/// The high bits of the objective instances of the roots (Inferred: retail instances are
/// runtime numbers no capture explains; ours are the encounter's index and its count of
/// objectives under this prefix, apart from the players' quest instances).
const OBJECTIVE_INSTANCE_NAMESPACE: u64 = 0x506f_4900_0000_0000;
/// The bits of an objective instance that count the encounter's objectives.
const OBJECTIVE_COUNT_BITS: u32 = 20;
/// The progress of a done task that does not count, and of a done container.
const DONE_PROGRESS: u32 = 1;

impl RootObjective {
    fn new(
        component: u64,
        tasks: EncounterTasks,
        name: &str,
        number: u64,
        encounter: usize,
    ) -> Self {
        let objective = nw_formats::objectstream::lowercase_crc32(&format!(
            "{OBJECTIVE_ID_PREFIX}{name}[{number}]"
        ));
        Self {
            component,
            tasks,
            objective,
            encounter,
            made: 0,
            live: None,
            start_times: Vec::new(),
            start_times_sent: false,
            completed: Vec::new(),
            version: 0,
            versions: ListVersions::default(),
        }
    }

    /// The member's next version.
    fn bump(&mut self) -> u64 {
        self.version += 1;
        self.version
    }

    /// The task entries of `runs` (in task order), kept in the order of `entries`' first
    /// appearance: a task from its first count or its end, its container from the task's end,
    /// the root from the first container's end (progress: done containers; complete when all
    /// are).
    fn entries(&self, instance: u64, runs: &[TaskRun], entries: &[TaskEntry]) -> Vec<TaskEntry> {
        let mut computed = Vec::new();
        let entry = |index, task, progress, state| TaskEntry {
            instance,
            index,
            task,
            progress,
            state,
        };
        let mut done = 0_u32;
        for (task, run) in self.tasks.tasks.iter().zip(runs) {
            let state = if run.done {
                TaskWireState::Complete
            } else {
                TaskWireState::Active
            };
            let progress = if run.counts { run.count } else { DONE_PROGRESS };
            if run.done || (run.counts && run.count > 0) {
                computed.push(entry(task.index, task.task, progress, state));
            }
            if run.done {
                done += 1;
                computed.push(entry(
                    task.index + 1,
                    task.container,
                    DONE_PROGRESS,
                    TaskWireState::Complete,
                ));
            }
        }
        if done > 0 {
            let all = usize::try_from(done).is_ok_and(|done| done == self.tasks.tasks.len());
            let state = if all {
                TaskWireState::Complete
            } else {
                TaskWireState::Active
            };
            computed.push(entry(self.tasks.root_index, self.tasks.root_task, done, state));
        }
        let mut kept: Vec<TaskEntry> = entries
            .iter()
            .filter_map(|old| computed.iter().find(|new| new.index == old.index).copied())
            .collect();
        for new in computed {
            if !kept.iter().any(|old| old.index == new.index) {
                kept.push(new);
            }
        }
        kept
    }

    /// The objective after its encounter's step: `phase` (see [`ObjectivePhase`]); whether the
    /// member changed.
    fn step(&mut self, phase: Option<ObjectivePhase>) -> bool {
        match (self.live.take(), phase) {
            (None, Some(ObjectivePhase::Running(runs))) => {
                let Some(first_ms) = runs.iter().filter_map(|run| run.started_ms).min() else {
                    return false;
                };
                let index = u64::try_from(self.encounter).unwrap_or(u64::MAX);
                let instance = OBJECTIVE_INSTANCE_NAMESPACE
                    | (index << OBJECTIVE_COUNT_BITS)
                    | (self.made & ((1 << OBJECTIVE_COUNT_BITS) - 1));
                self.made += 1;
                let mut record = ObjectiveRecord::available(self.objective, instance);
                record.accept_time = unix_ns(first_ms);
                record.poi = true;
                let version = self.bump();
                self.versions.active = version;
                self.versions.tasks = version;
                if self.versions.tracked == 0 {
                    self.versions.tracked = version;
                }
                self.live = Some((record, Vec::new()));
                self.update(&runs);
                true
            }
            (Some(live), Some(ObjectivePhase::Running(runs))) => {
                self.live = Some(live);
                self.update(&runs)
            }
            (Some((record, _)), ended) => {
                let version = self.bump();
                if matches!(ended, Some(ObjectivePhase::Ended)) {
                    self.completed = vec![record.instance];
                    self.versions.completed = version;
                }
                self.versions.active = version;
                self.versions.tasks = version;
                if !self.start_times.is_empty() {
                    self.start_times.clear();
                    self.versions.start_times = version;
                }
                true
            }
            (None, Some(ObjectivePhase::Ended) | None) => false,
        }
    }

    /// Updates the running objective's entries and start times from `runs`; whether they
    /// changed.
    fn update(&mut self, runs: &[TaskRun]) -> bool {
        let Some((record, entries)) = &self.live else {
            return false;
        };
        let instance = record.instance;
        let new_entries = self.entries(instance, runs, entries);
        let new_starts: Vec<(TaskKey, u64)> = self
            .tasks
            .tasks
            .iter()
            .zip(runs)
            .filter(|(_, run)| run.timed && !run.done)
            .filter_map(|(task, run)| {
                Some((
                    TaskKey {
                        instance,
                        index: task.index,
                    },
                    unix_ns(run.started_ms?),
                ))
            })
            .collect();
        let entries_changed = new_entries != *entries;
        let starts_changed = new_starts != self.start_times;
        if !entries_changed && !starts_changed {
            return false;
        }
        let version = self.bump();
        if entries_changed {
            self.versions.tasks = version;
            if let Some((_, entries)) = &mut self.live {
                *entries = new_entries;
            }
        }
        if starts_changed {
            self.versions.start_times = version;
            self.start_times_sent |= !new_starts.is_empty();
            self.start_times = new_starts;
        }
        true
    }

    /// The member, once it made an objective.
    fn member(&self) -> Option<BundleMember> {
        if self.version == 0 {
            return None;
        }
        let full = |version: u64| Some(version);
        let (active, tasks) = match &self.live {
            Some((record, entries)) => (vec![record.clone()], entries.clone()),
            None => (Vec::new(), Vec::new()),
        };
        let state = ObjectivesState {
            task_start_times: self
                .start_times_sent
                .then(|| KeyedList::full(self.versions.start_times, self.start_times.clone())),
            tracked: Some(FullList {
                version: full(self.versions.tracked),
                elements: Vec::new(),
            }),
            completed: (!self.completed.is_empty()).then(|| {
                ReplicatedList::Full(FullList {
                    version: full(self.versions.completed),
                    elements: self.completed.clone(),
                })
            }),
            active: Some(FullList {
                version: full(self.versions.active),
                elements: active,
            }),
            tasks: Some(FullList {
                version: full(self.versions.tasks),
                elements: tasks,
            }),
            ..ObjectivesState::default()
        };
        let mut writer = Writer::new();
        match state.write(&mut writer) {
            Ok(()) => Some(recorded(self.component, OBJECTIVES_TYPE_INDEX, writer.finish())),
            Err(reason) => {
                tracing::warn!(component = self.component, %reason, "encounter objective body");
                None
            }
        }
    }
}

/// A root change that goes out later.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DueChange {
    /// A started timeline ends.
    TimelineEnd { component: u64, length_s: f32 },
    /// A triggered delayed event completes.
    DelayDone { component: u64 },
}

impl Root {
    /// The highest version of its timeline states (one counter per entity, Inferred).
    fn timeline_version(&self) -> u64 {
        self.members
            .iter()
            .filter_map(|member| match &member.state {
                MemberState::Recorded { class, body } if *class == TIMELINE_TYPE_INDEX => {
                    TimelineState::read(&mut Reader::new(body)).ok()?.version()
                }
                _ => None,
            })
            .max()
            .unwrap_or_default()
    }

    /// `member` changes: a new viewer gets it in the spawn group, a viewer that has the root
    /// gets it once.
    fn change(&mut self, member: BundleMember) {
        merge_members(&mut self.members, std::slice::from_ref(&member));
        self.version += 1;
        self.changes.push((self.version, member));
        if self.changes.len() > MAX_EVENT_CHANGES {
            self.changes.remove(0);
        }
    }

    /// Its timeline `component` starts or ends (`length_s` given) at its next version.
    fn play(&mut self, component: u64, length_s: Option<f32>) {
        let version = self.timeline_version() + 1;
        let state = match length_s {
            Some(length_s) => TimelineState::end(version, length_s),
            None => TimelineState::start(version),
        };
        if let Some(member) = timeline_member(component, &state) {
            self.change(member);
        }
    }

    fn manager_member(&self) -> BundleMember {
        recorded(
            self.manager_component,
            MANAGER_CLASS,
            manager_body(self.manager),
        )
    }

    /// The manager, the stages and the objective (once made) of the root.
    fn state_members(&self) -> impl Iterator<Item = BundleMember> + '_ {
        std::iter::once(self.manager_member())
            .chain(
                self.stage_components
                    .iter()
                    .zip(&self.stages)
                    .map(|(component, history)| recorded(*component, STAGE_CLASS, history.body())),
            )
            .chain(self.objective.as_ref().and_then(RootObjective::member))
    }
}

/// The card of slice `asset` with id `gde_ref`.
pub(super) fn card(asset: [u8; 16], gde_ref: [u8; nw_messages::RAW_ID_LEN]) -> BundleMember {
    BundleMember {
        component: CARD_COMPONENT,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: Some(AssetId {
                id: asset,
                sub_id: DYNAMIC_SLICE_SUB_ID,
            }),
            gde_ref: Some(gde_ref),
        }),
    }
}

/// The position state of component `component` at `at`, turned by `half_yaw` (radians, half
/// the yaw).
pub(super) fn place_member(component: u64, at: [f32; 3], half_yaw: f32) -> BundleMember {
    let [x, y, z] = at;
    BundleMember {
        component,
        state: MemberState::Position(PositionInTheWorldReplicatedState {
            position: WorldPosition::from_metres(x, y, z),
            rotation: Some(Rotation::of_quaternion([
                0.0,
                0.0,
                half_yaw.sin(),
                half_yaw.cos(),
            ])),
            scale: None,
        }),
    }
}

/// The members of a table row; `None` when one does not read.
fn row_members(members: &[Vec<u8>]) -> Option<Vec<BundleMember>> {
    members
        .iter()
        .map(|bytes| BundleMember::read_from(bytes).map(|(member, _)| member))
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

/// The encounter manager state body of `status` (`01 01 status:u32`).
pub(super) fn manager_body(status: u32) -> Vec<u8> {
    let mut body = STATE_HEAD.to_vec();
    body.extend_from_slice(&status.to_be_bytes());
    body
}

pub(super) fn recorded(component: u64, class: u64, body: Vec<u8>) -> BundleMember {
    BundleMember {
        component,
        state: MemberState::Recorded { class, body },
    }
}

/// The encounter roots of one sim.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct EncounterRoots {
    roots: Vec<Root>,
    /// The root of each encounter index.
    by_encounter: HashMap<usize, usize>,
    /// Changes that go out later: (due time, root, change).
    due: Vec<(u64, usize, DueChange)>,
}

impl EncounterRoots {
    /// The roots of the open world's `encounters` whose slice the table `table` holds with as
    /// many stage states as the encounter has stages; the others have no root (no retail group
    /// of their slice shows its layout).
    pub(super) fn build(table: &TownProps, encounters: &[Encounter]) -> Self {
        let by_slice: HashMap<&str, _> = table
            .iter()
            .filter_map(|row| Some((row.slice.as_deref()?, row)))
            .collect();
        let mut roots = Self::default();
        for (index, encounter) in encounters.iter().enumerate() {
            let Some(row) = by_slice.get(encounter.slice.as_str()) else {
                continue;
            };
            let Some(states) = row_members(&row.members) else {
                continue;
            };
            let class_of = |member: &BundleMember| member.state.type_index();
            let Some(manager_component) = states
                .iter()
                .find(|member| class_of(member) == MANAGER_CLASS)
                .map(|member| member.component)
            else {
                continue;
            };
            let mut stage_components: Vec<u64> = states
                .iter()
                .filter(|member| class_of(member) == STAGE_CLASS)
                .map(|member| member.component)
                .collect();
            stage_components.sort_unstable();
            if stage_components.len() != encounter.stages.len() {
                continue;
            }
            let [x, y, z] = encounter.position;
            let position = Position { x, y, z };
            let half_yaw = encounter.yaw_deg.to_radians() / 2.0;
            let members = std::iter::once(card(row.asset, dynamic_ref(ROOT_NAMESPACE, index)))
                .chain(
                    states
                        .into_iter()
                        .filter_map(|member| match class_of(&member) {
                            // A mined objectives member is a frozen retail body (another
                            // instance, accept time and tasks): the root's live objective
                            // replaces it.
                            MANAGER_CLASS | STAGE_CLASS | OBJECTIVES_TYPE_INDEX => None,
                            POSITION_CLASS => {
                                Some(place_member(member.component, encounter.position, half_yaw))
                            }
                            _ => Some(member),
                        }),
                )
                .collect();
            let number = ENCOUNTER_ROOT_ENTITY_BASE
                + u64::try_from(roots.roots.len()).unwrap_or_default();
            let objective = encounter
                .objectives_component
                .zip(encounter.objective_tasks())
                .map(|(component, tasks)| {
                    RootObjective::new(component, tasks, &encounter.name, number, index)
                });
            roots.by_encounter.insert(index, roots.roots.len());
            roots.roots.push(Root {
                encounter: index,
                position,
                members,
                manager_component,
                stages: vec![History::default(); stage_components.len()],
                stage_components,
                manager: STATUS_NOT_STARTED,
                shown: false,
                version: 0,
                changes: Vec::new(),
                objective,
            });
        }
        roots
    }

    /// How many encounters have a root.
    pub(super) fn len(&self) -> usize {
        self.roots.len()
    }

    /// Encounter `encounter` is `manager` and its stages `stages`, and this sim owns it
    /// (`owned`); a change goes to the players who see its root.
    pub(super) fn set(&mut self, encounter: usize, owned: bool, manager: u32, stages: &[u32]) {
        let Some(root) = self
            .by_encounter
            .get(&encounter)
            .and_then(|index| self.roots.get_mut(*index))
        else {
            return;
        };
        let mut changed = root.shown != owned || root.manager != manager;
        root.shown = owned;
        root.manager = manager;
        for (history, status) in root.stages.iter_mut().zip(stages) {
            changed |= history.set(*status);
        }
        if changed {
            root.version += 1;
        }
    }

    /// The objective of encounter `encounter` after its step: `phase` gives where it stands from
    /// the root's task tree (`None` when the root has no objective); a change goes to the
    /// players who see the root.
    pub(super) fn set_objective(
        &mut self,
        encounter: usize,
        phase: impl FnOnce(&EncounterTasks) -> Option<ObjectivePhase>,
    ) {
        let Some(root) = self
            .by_encounter
            .get(&encounter)
            .and_then(|index| self.roots.get_mut(*index))
        else {
            return;
        };
        let Some(objective) = &mut root.objective else {
            return;
        };
        let phase = phase(&objective.tasks);
        if objective.step(phase) {
            root.version += 1;
        }
    }

    /// A stage of encounter `encounter` sent entity event `event` at `now_ms` to an entity of
    /// its slice whose replicated components are `components` (see the module text).
    pub(super) fn entity_event(
        &mut self,
        encounter: usize,
        components: &[EntityComponent],
        event: u32,
        now_ms: u64,
    ) {
        let Some(&index) = self.by_encounter.get(&encounter) else {
            return;
        };
        let Some(root) = self.roots.get_mut(index) else {
            return;
        };
        for component in components {
            match (*component, event) {
                (
                    EntityComponent::Timeline {
                        component,
                        length_s,
                    },
                    EVENT_ACTIVATE,
                ) => {
                    root.play(component, None);
                    self.due.push((
                        now_ms.saturating_add(seconds_ms(length_s)),
                        index,
                        DueChange::TimelineEnd {
                            component,
                            length_s,
                        },
                    ));
                }
                (EntityComponent::DelayedEvent { component, delay_s }, EVENT_TRIGGER) => {
                    root.change(BundleMember {
                        component,
                        state: MemberState::DelayedEvent(DelayedEventState {
                            total_delay_s: Some(delay_s),
                            completed: None,
                        }),
                    });
                    self.due.push((
                        now_ms.saturating_add(seconds_ms(delay_s)),
                        index,
                        DueChange::DelayDone { component },
                    ));
                }
                (
                    EntityComponent::PlacementObstruction { component },
                    EVENT_ACTIVATE | EVENT_DEACTIVATE,
                ) => root.change(BundleMember {
                    component,
                    state: MemberState::PlacementObstruction(PlacementObstructionState {
                        has_completion_obstruction: Some(u8::from(event == EVENT_ACTIVATE)),
                    }),
                }),
                _ => {}
            }
        }
    }

    /// Sends the changes due at `now_ms` (timeline ends, delayed event completions).
    pub(super) fn tick(&mut self, now_ms: u64) {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.due)
            .into_iter()
            .partition(|(at_ms, _, _)| *at_ms <= now_ms);
        self.due = later;
        for (_, index, change) in due {
            let Some(root) = self.roots.get_mut(index) else {
                continue;
            };
            match change {
                DueChange::TimelineEnd {
                    component,
                    length_s,
                } => root.play(component, Some(length_s)),
                DueChange::DelayDone { component } => root.change(BundleMember {
                    component,
                    state: MemberState::DelayedEvent(DELAY_DONE),
                }),
            }
        }
    }

    /// The entity numbers of the shown roots within `radius_m` of `position`.
    pub(super) fn numbers_near(
        &self,
        position: Position,
        radius_m: f32,
    ) -> impl Iterator<Item = u64> + '_ {
        (0_u64..)
            .zip(&self.roots)
            .filter(move |(_, root)| {
                root.shown && root.position.horizontal_distance(position) <= radius_m
            })
            .map(|(index, _)| ENCOUNTER_ROOT_ENTITY_BASE + index)
    }

    /// The group of root `number` for a viewer at `slot`: the whole group in a new slot, its
    /// manager and stage states after a change, else `None`. `sent` holds the versions the
    /// viewer has.
    pub(super) fn group(
        &self,
        number: u64,
        slot: Slot,
        sent: &mut HashMap<u64, u32>,
    ) -> Option<BundleGroup> {
        let index = usize::try_from(number.checked_sub(ENCOUNTER_ROOT_ENTITY_BASE)?).ok()?;
        let root = self.roots.get(index)?;
        let known = sent.get(&number).copied();
        if !slot.new && known == Some(root.version) {
            return None;
        }
        sent.insert(number, root.version);
        let members = if slot.new {
            root.members
                .iter()
                .cloned()
                .chain(root.state_members())
                .collect()
        } else {
            let unsent = root
                .changes
                .iter()
                .filter(move |(version, _)| known.is_none_or(|known| *version > known))
                .map(|(_, member)| member.clone());
            root.state_members().chain(unsent).collect()
        };
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members,
        })
    }

    /// The encounter of each root, for the tests.
    #[cfg(test)]
    fn encounters(&self) -> Vec<usize> {
        self.roots.iter().map(|root| root.encounter).collect()
    }
}

/// The slice of a start object as retail sent it.
#[derive(Debug, Clone, PartialEq)]
struct ObjectKind {
    asset: [u8; 16],
    /// Its states besides the card; the position state is placed at each object's place.
    members: Vec<BundleMember>,
    /// How a gather ends it, when it is not at once.
    vanish: Option<GatherVanish>,
    /// The component of its interact state: its spawn group's, else its vanish's.
    interact: Option<u64>,
}

/// A start object that a gather does not remove at once: the gather empties it and starts its
/// delayed event, and the object goes when the event's delay ends.
///
/// Observed (retail `99a_msq_01b_learnsoulwardens`, the holdout glyph): at the gather's end
/// (242.570 s) the server sends the glyph's gatherable controller `Depleted` (c3 `010108`), its
/// interact state off with no interactor (c4 `01030000000000`) and its delayed event with its
/// whole delay, 3 s (c6 `01014200`); the glyph's slot stops 3.015 s later (245.585 s), in the tick
/// that ends the encounter's `Interact` stage. The glyph's spawn group has no interact member, so
/// the table names its component.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GatherVanish {
    /// The start object's slice.
    slice: &'static str,
    /// The component of its interact state.
    interact: u64,
    /// Its delayed event's delay, seconds.
    delay_s: f32,
}

/// The start objects whose gather a retail capture shows ending this way.
const GATHER_VANISHES: [GatherVanish; 1] = [GatherVanish {
    slice: "slices/pois/ancient/design/holdout/worldevent_holdout_interactobject_master.dynamicslice",
    interact: 4,
    delay_s: 3.0,
}];

/// The interact state of a gathered start object (Observed: the holdout glyph at 242.570 s).
const GATHERED_INTERACT: InteractState = InteractState {
    enabled: Some(false),
    has_interactors: Some(0),
};

/// The interact change of a start object whose gather starts: one interactor, `HasInteractors`
/// alone (Observed: the holdout glyph c4 `010200000001` at 240.569 s, the gather's start; the
/// interact states of 513 ledgers count the players who gather a gatherable, as a change of
/// `HasInteractors` alone at each start and each stop).
const GATHERING_INTERACT: InteractState = InteractState {
    enabled: None,
    has_interactors: Some(1),
};

/// The interact change of a start object whose gather stopped undone or emptied it to stay: no
/// interactor (Observed: `HasInteractors` 1 then 0 on 222 retail gatherable slots; the
/// `CATACOMBS5` chest c3 `010200000001` at 156.081 s and `010200000000` at 157.079 s).
const STOPPED_INTERACT: InteractState = InteractState {
    enabled: None,
    has_interactors: Some(0),
};

/// One start object in the world.
#[derive(Debug, Clone, PartialEq)]
struct StartObject {
    /// Its slice's pak path.
    slice: String,
    gde_ref: [u8; nw_messages::RAW_ID_LEN],
    position: Position,
    members: Vec<BundleMember>,
    /// Bumped by each change of its states after its spawn.
    version: u32,
    /// It was gathered and stays, empty ([`EncounterObjects::deplete`]).
    depleted: bool,
    /// How a gather ends it, when it is not at once.
    vanish: Option<GatherVanish>,
    /// When it goes after its gather ([`EncounterObjects::gathered`]).
    vanish_at_ms: Option<u64>,
    /// The interact state's component, when its slice has one.
    interact: Option<u64>,
    /// The states its last change set, as a viewer that holds its slot gets them.
    changed: Vec<BundleMember>,
}

impl StartObject {
    /// Its interact state takes the set children of `change` (the whole state stays for a new
    /// viewer) and `change` joins its last change; nothing for a slice without an interact
    /// state.
    fn set_interact(&mut self, change: InteractState) {
        let Some(component) = self.interact else {
            return;
        };
        match self
            .members
            .iter_mut()
            .find(|member| member.component == component)
        {
            Some(BundleMember {
                state: MemberState::Interact(state),
                ..
            }) => {
                state.enabled = change.enabled.or(state.enabled);
                state.has_interactors = change.has_interactors.or(state.has_interactors);
            }
            _ => {
                self.members.push(BundleMember {
                    component,
                    state: MemberState::Interact(change),
                });
                self.members.sort_by_key(|member| member.component);
            }
        }
        self.changed.push(BundleMember {
            component,
            state: MemberState::Interact(change),
        });
    }

    /// Its interact state changes to `change` alone, as its own change.
    fn interact_change(&mut self, change: InteractState) {
        if self.interact.is_none() {
            return;
        }
        self.changed.clear();
        self.set_interact(change);
        self.version += 1;
    }
}

/// The gatherable controller state of an emptied gatherable (Observed: a catacombs chest goes
/// from 4 to 8 when gathered and stays, `CATACOMBS5` 157.079 s).
const DEPLETED_GATHERABLE_STATE: u8 = 8;

/// The start objects of the open world's encounters.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct EncounterObjects {
    kinds: HashMap<String, ObjectKind>,
    /// The objects standing, by entity number.
    objects: HashMap<u64, StartObject>,
    /// The objects spawned so far: each gets its own number and id.
    spawned: u64,
}

impl EncounterObjects {
    /// The start object slices of table `table` (the encounter objects file).
    pub(super) fn build(table: &TownProps) -> Self {
        let kinds = table
            .iter()
            .filter_map(|row| {
                let slice = row.slice.clone()?;
                let members = row_members(&row.members)?;
                let vanish = GATHER_VANISHES
                    .iter()
                    .find(|vanish| vanish.slice == slice)
                    .copied();
                let interact = members
                    .iter()
                    .find(|member| matches!(member.state, MemberState::Interact(_)))
                    .map(|member| member.component)
                    .or(vanish.map(|vanish| vanish.interact));
                Some((
                    slice,
                    ObjectKind {
                        asset: row.asset,
                        members,
                        vanish,
                        interact,
                    },
                ))
            })
            .collect();
        Self {
            kinds,
            ..Self::default()
        }
    }

    /// Spawns a start object of slice `slice` at `at`, turned `yaw_deg` about the up axis; its
    /// entity number, or `None` for a slice that is no start object of the table.
    pub(super) fn spawn(&mut self, slice: &str, at: [f32; 3], yaw_deg: f32) -> Option<u64> {
        let kind = self.kinds.get(slice)?;
        let index = usize::try_from(self.spawned).ok()?;
        let number = ENCOUNTER_OBJECT_ENTITY_BASE + self.spawned;
        self.spawned += 1;
        let gde_ref = dynamic_ref(OBJECT_NAMESPACE, index);
        let members = std::iter::once(card(kind.asset, gde_ref))
            .chain(kind.members.iter().map(|member| {
                if member.state.type_index() == POSITION_CLASS {
                    place_member(member.component, at, yaw_deg.to_radians() / 2.0)
                } else {
                    member.clone()
                }
            }))
            .collect();
        let [x, y, z] = at;
        self.objects.insert(
            number,
            StartObject {
                slice: slice.to_owned(),
                gde_ref,
                position: Position { x, y, z },
                members,
                version: 0,
                depleted: false,
                vanish: kind.vanish,
                vanish_at_ms: None,
                interact: kind.interact,
                changed: Vec::new(),
            },
        );
        Some(number)
    }

    /// Start object `number` was gathered but stays, emptied: its gatherable controller state
    /// goes to [`DEPLETED_GATHERABLE_STATE`], its interact state counts no interactor, and no
    /// one can gather it again. Whether it stood and was not emptied yet.
    pub(super) fn deplete(&mut self, number: u64) -> bool {
        let Some(object) = self.objects.get_mut(&number).filter(|object| !object.depleted) else {
            return false;
        };
        object.changed.clear();
        for member in &mut object.members {
            if let MemberState::GatherableController(state) = &mut member.state {
                state.gatherable_state = Some(DEPLETED_GATHERABLE_STATE);
                object.changed.push(member.clone());
            }
        }
        // Observed: `CATACOMBS5` chest c3 `010200000000` at 157.079 s, with its controller at 8.
        object.set_interact(STOPPED_INTERACT);
        object.depleted = true;
        object.version += 1;
        true
    }

    /// A player's gather of start object `number` starts: its interact state counts one
    /// interactor ([`GATHERING_INTERACT`]).
    pub(super) fn gather_started(&mut self, number: u64) {
        if let Some(object) = self.objects.get_mut(&number).filter(|object| !object.depleted) {
            object.interact_change(GATHERING_INTERACT);
        }
    }

    /// A player's gather of start object `number` stopped undone: its interact state counts no
    /// interactor ([`STOPPED_INTERACT`]).
    pub(super) fn gather_stopped(&mut self, number: u64) {
        if let Some(object) = self.objects.get_mut(&number).filter(|object| !object.depleted) {
            object.interact_change(STOPPED_INTERACT);
        }
    }

    /// A player's gather of start object `number` ended at `now_ms`: an object of
    /// [`GATHER_VANISHES`] is emptied, its interact state goes off and its delayed event starts
    /// (it goes at the event's end, [`Self::tick`]); any other goes now.
    pub(super) fn gathered(&mut self, number: u64, now_ms: u64) {
        let Some(object) = self.objects.get_mut(&number) else {
            return;
        };
        let Some(vanish) = object.vanish.filter(|_| !object.depleted) else {
            self.objects.remove(&number);
            return;
        };
        object.changed.clear();
        let StartObject {
            members, changed, ..
        } = object;
        for member in members.iter_mut() {
            match &mut member.state {
                MemberState::GatherableController(state) => {
                    state.gatherable_state = Some(DEPLETED_GATHERABLE_STATE);
                }
                MemberState::DelayedEvent(state) => {
                    *state = DelayedEventState {
                        total_delay_s: Some(vanish.delay_s),
                        completed: None,
                    };
                }
                _ => continue,
            }
            changed.push(member.clone());
        }
        object.set_interact(GATHERED_INTERACT);
        object.changed.sort_by_key(|member| member.component);
        object.depleted = true;
        object.version += 1;
        object.vanish_at_ms = Some(now_ms.saturating_add(seconds_ms(vanish.delay_s)));
    }

    /// The gathered start objects whose delayed event ended by `now_ms` go.
    pub(super) fn tick(&mut self, now_ms: u64) {
        self.objects.retain(|_, object| {
            object
                .vanish_at_ms
                .is_none_or(|vanish_at_ms| now_ms < vanish_at_ms)
        });
    }

    /// Whether `slice` is the slice of a start object of the table.
    pub(super) fn is_kind(&self, slice: &str) -> bool {
        self.kinds.contains_key(slice)
    }

    /// Whether `entity` is a start object's number.
    pub(super) const fn is_object(entity: u64) -> bool {
        entity >= ENCOUNTER_OBJECT_ENTITY_BASE
    }

    /// The slice path of standing start object `number`.
    pub(super) fn slice_of(&self, number: u64) -> Option<&str> {
        self.objects.get(&number).map(|object| object.slice.as_str())
    }

    /// Whether start object `number` stands.
    pub(super) fn stands(&self, number: u64) -> bool {
        self.objects.contains_key(&number)
    }

    /// Start object `number` goes (gathered, or its encounter reset).
    pub(super) fn remove(&mut self, number: u64) {
        self.objects.remove(&number);
    }

    /// The start object whose id is `gde_ref` and that stands within `reach_m` of `position`.
    pub(super) fn at(
        &self,
        gde_ref: &[u8; nw_messages::RAW_ID_LEN],
        position: Position,
        reach_m: f32,
    ) -> Option<u64> {
        self.objects
            .iter()
            .find(|(_, object)| {
                object.gde_ref == *gde_ref
                    && !object.depleted
                    && object.position.horizontal_distance(position) <= reach_m
            })
            .map(|(number, _)| *number)
    }

    /// The numbers of the start objects within `radius_m` of `position`.
    pub(super) fn numbers_near(
        &self,
        position: Position,
        radius_m: f32,
    ) -> impl Iterator<Item = u64> + '_ {
        self.objects
            .iter()
            .filter(move |(_, object)| object.position.horizontal_distance(position) <= radius_m)
            .map(|(number, _)| *number)
    }

    /// The group of start object `number` for a viewer that gives it `slot` and was sent the
    /// versions `sent`: every state in a new slot, then the states of each change
    /// ([`Self::deplete`], [`Self::gathered`]); `None` when the viewer has the object's last
    /// version.
    pub(super) fn group(
        &self,
        number: u64,
        slot: Slot,
        sent: &mut HashMap<u64, u32>,
    ) -> Option<BundleGroup> {
        let object = self.objects.get(&number)?;
        if !slot.new && sent.get(&number) == Some(&object.version) {
            return None;
        }
        sent.insert(number, object.version);
        let members = if slot.new {
            object.members.clone()
        } else {
            object.changed.clone()
        };
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members,
        })
    }
}

#[cfg(test)]
mod tests {
    use nw_world::dungeon_scripts::Stage;

    use super::*;

    const SLICE: &str =
        "slices/darkness/events/minor/minor_grove/enc_darkness_minor_grove_00.dynamicslice";
    const AT: [f32; 3] = [9551.7, 6655.5, 103.0];

    /// The usual retail root group of the minor grove (the mined table's row): spawner (4005),
    /// manager, position, objectives (3857), seven stages, placement obstructions (2187) and
    /// timelines (1647).
    const MEMBERS: [&str; 18] = [
        "01a53e010200000003",
        "02826a010100000002",
        "030d0103461a03d945ae1ee7212b03acd2",
        "04913c021a000108000001080120af901a664930392f7e005d000000000000000000000000000000000000000018d6d48b6efba0f00000010001000000000000000000000000010800",
        "059521010103000000000000000000000001000000010000000200000002",
        "0695210101040000000000000000000000010000000100000002000000020000000300000003",
        "07952101010200000000000000000000000100000001",
        "08952101010200000000000000000000000100000001",
        "098b22010101",
        "0aaf19010300010701000001070100000000",
        "0baf19010300010701000001070140800000",
        "0c952101010200000000000000000000000100000001",
        "0d9521010103000000000000000000000001000000010000000200000002",
        "0e952101010200000000000000000000000100000001",
        "0f8b22010100",
        "108b22010101",
        "11af19010300010701000001070140d147b4",
        "12af19010300010701000001070100000000",
    ];
    /// Its stages.
    const STAGES: usize = 7;

    fn table() -> TownProps {
        let members: Vec<String> = MEMBERS.iter().map(|hex| format!("\"{hex}\"")).collect();
        TownProps::parse(&format!(
            r#"{{"props": [{{"asset": "6e56650a3eb85decb1f4f0f709a8fc24", "slice": "{SLICE}",
            "members": [{}]}}]}}"#,
            members.join(",")
        ))
        .expect("table")
    }

    fn encounter(stages: usize) -> Encounter {
        Encounter {
            slice: SLICE.to_owned(),
            position: AT,
            stages: vec![Stage::default(); stages],
            ..Encounter::default()
        }
    }

    fn slot(new: bool) -> Slot {
        Slot { interest: 5, new }
    }

    fn body_of(group: &BundleGroup, component: u64) -> Vec<u8> {
        group
            .members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Recorded { body, .. } if member.component == component => {
                    Some(body.clone())
                }
                _ => None,
            })
            .expect("member")
    }

    #[test]
    fn a_root_comes_whole_then_sends_only_its_status_changes() {
        let mut roots =
            EncounterRoots::build(&table(), &[encounter(STAGES - 1), encounter(STAGES)]);
        assert_eq!(
            roots.encounters(),
            [1],
            "the stage count must match the slice's"
        );
        assert_eq!(
            roots
                .numbers_near(
                    Position {
                        x: AT[0],
                        y: AT[1],
                        z: AT[2]
                    },
                    50.0
                )
                .count(),
            0
        );
        roots.set(1, true, STATUS_NOT_STARTED, &[STATUS_NOT_STARTED; STAGES]);
        let near = Position {
            x: AT[0],
            y: AT[1],
            z: AT[2],
        };
        let number = roots
            .numbers_near(near, 50.0)
            .next()
            .expect("shown when owned");
        let mut sent = HashMap::new();
        let whole = roots.group(number, slot(true), &mut sent).expect("whole");
        assert!(matches!(
            whole.members[0].state,
            MemberState::GdeMetadata(_)
        ));
        assert!(
            whole
                .members
                .iter()
                .any(|member| member.state.type_index() == 4005)
        );
        assert_eq!(
            whole.members.len(),
            MEMBERS.len(),
            "the card and every state but the mined objectives body"
        );
        assert!(
            whole
                .members
                .iter()
                .all(|member| member.state.type_index() != OBJECTIVES_TYPE_INDEX),
            "no objective before its first task's stage starts"
        );
        assert_eq!(body_of(&whole, 2), [1, 1, 0, 0, 0, 1], "not started");
        assert_eq!(
            body_of(&whole, 5),
            [1, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1]
        );
        assert!(
            roots.group(number, slot(false), &mut sent).is_none(),
            "no change"
        );
        let mut stages = [STATUS_NOT_STARTED; STAGES];
        stages[0] = STATUS_IN_PROGRESS;
        roots.set(1, true, STATUS_IN_PROGRESS, &stages);
        let change = roots.group(number, slot(false), &mut sent).expect("change");
        assert_eq!(
            change.members.len(),
            STAGES + 1,
            "the manager and the stages"
        );
        assert_eq!(body_of(&change, 2), [1, 1, 0, 0, 0, 2]);
        assert_eq!(
            body_of(&change, 5),
            [
                1, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 2
            ],
            "the first stage's history gains (2, in progress)"
        );
    }

    /// The minor grove's objectives component (the mined member's component number).
    const OBJECTIVES_COMPONENT: u64 = 4;
    /// The kills its one task needs.
    const KILLS: u32 = 2;
    /// When the task's stage starts, server time.
    const STAGE_START_MS: u64 = 5_000;

    /// An encounter of the minor grove's slice whose first stage is a user wave with one kill
    /// objective (`time_limit_s` its limit).
    fn tasked_encounter(time_limit_s: u32) -> Encounter {
        let mut encounter = encounter(STAGES);
        encounter.name = "Enc_Darkness_Minor_Grove_00".to_owned();
        encounter.objectives_component = Some(OBJECTIVES_COMPONENT);
        encounter.top_stages = vec![0];
        encounter.stages[0] = Stage {
            user_wave: true,
            objectives: vec![nw_world::dungeon_scripts::StageObjective {
                time_limit_s,
                succeed_on_timeout: false,
                failure: false,
                kind: nw_world::dungeon_scripts::ObjectiveSpec::SliceDestroyed {
                    target: None,
                    required: KILLS,
                },
            }],
            ..Stage::default()
        };
        encounter
    }

    fn running(count: u32, timed: bool, done: bool) -> Option<ObjectivePhase> {
        Some(ObjectivePhase::Running(vec![TaskRun {
            started_ms: Some(STAGE_START_MS),
            count,
            counts: true,
            timed,
            done,
        }]))
    }

    /// The root's objective as a viewer that has the root gets it after a change.
    fn objective_of(
        roots: &EncounterRoots,
        number: u64,
        sent: &mut HashMap<u64, u32>,
    ) -> ObjectivesState {
        let group = roots.group(number, slot(false), sent).expect("change");
        let body = body_of(&group, OBJECTIVES_COMPONENT);
        ObjectivesState::read(&mut Reader::new(&body)).expect("objectives")
    }

    fn entries(state: &ObjectivesState) -> Vec<(u8, u32, u32, TaskWireState)> {
        state
            .tasks
            .as_ref()
            .expect("tasks")
            .elements
            .iter()
            .map(|entry| (entry.index, entry.task, entry.progress, entry.state))
            .collect()
    }

    /// Retail (encounter objectives note): the objective comes when the first task's stage
    /// starts (accept time then, tasks empty), the task from its first kill, the task done with
    /// its container and the root, and at the end the instance in the completed list with the
    /// others empty; the next run makes a new instance.
    #[test]
    fn a_root_sends_its_live_objective_from_the_first_task_to_its_end() {
        const SLICE_DESTROYED: u32 = 0x2306_90d4;
        const CONTAINER_2: u32 = 0xa278_1fd7;
        const ROOT_CONTAINER: u32 = 0x1004_d537;
        let mut roots = EncounterRoots::build(&table(), &[tasked_encounter(0)]);
        roots.set(0, true, STATUS_IN_PROGRESS, &[STATUS_NOT_STARTED; STAGES]);
        let near = Position {
            x: AT[0],
            y: AT[1],
            z: AT[2],
        };
        let number = roots.numbers_near(near, 50.0).next().expect("shown");
        let mut sent = HashMap::new();
        roots.group(number, slot(true), &mut sent).expect("whole");
        roots.set_objective(0, |_| None);
        assert!(roots.group(number, slot(false), &mut sent).is_none(), "no objective yet");
        roots.set_objective(0, |tasks| {
            assert_eq!(tasks.root_index, 3);
            running(0, false, false)
        });
        let made = objective_of(&roots, number, &mut sent);
        let record = made.active.as_ref().expect("active").elements[0].clone();
        assert_eq!(record.accept_time, unix_ns(STAGE_START_MS));
        assert!(record.available && record.poi);
        assert!(entries(&made).is_empty(), "no task entry before a kill");
        assert!(made.tracked.as_ref().is_some_and(|list| list.elements.is_empty()));
        assert!(made.completed.is_none() && made.task_start_times.is_none());
        roots.set_objective(0, |_| running(0, false, false));
        assert!(roots.group(number, slot(false), &mut sent).is_none(), "no change");
        roots.set_objective(0, |_| running(1, false, false));
        assert_eq!(
            entries(&objective_of(&roots, number, &mut sent)),
            [(1, SLICE_DESTROYED, 1, TaskWireState::Active)]
        );
        roots.set_objective(0, |_| running(KILLS, false, true));
        assert_eq!(
            entries(&objective_of(&roots, number, &mut sent)),
            [
                (1, SLICE_DESTROYED, KILLS, TaskWireState::Complete),
                (2, CONTAINER_2, 1, TaskWireState::Complete),
                (3, ROOT_CONTAINER, 1, TaskWireState::Complete),
            ]
        );
        roots.set_objective(0, |_| Some(ObjectivePhase::Ended));
        let ended = objective_of(&roots, number, &mut sent);
        let completed = ended.completed.as_ref().and_then(ReplicatedList::full).expect("full");
        assert_eq!(completed.elements, [record.instance]);
        assert!(ended.active.as_ref().is_some_and(|list| list.elements.is_empty()));
        assert!(entries(&ended).is_empty());
        roots.set_objective(0, |_| Some(ObjectivePhase::Ended));
        roots.set_objective(0, |_| None);
        assert!(roots.group(number, slot(false), &mut sent).is_none(), "ended stays ended");
        roots.set_objective(0, |_| running(0, false, false));
        let again = objective_of(&roots, number, &mut sent);
        let next = &again.active.as_ref().expect("active").elements[0];
        assert_ne!(next.instance, record.instance, "a new instance");
        assert_eq!(next.objective, record.objective);
    }

    /// A timed task's start goes in the start times while it runs (retail holdout key 3 at
    /// 245.585 s) and leaves at its end; the list goes out empty from then on.
    #[test]
    fn a_timed_task_s_start_time_stands_while_it_runs() {
        let mut roots = EncounterRoots::build(&table(), &[tasked_encounter(60)]);
        roots.set(0, true, STATUS_IN_PROGRESS, &[STATUS_NOT_STARTED; STAGES]);
        let near = Position {
            x: AT[0],
            y: AT[1],
            z: AT[2],
        };
        let number = roots.numbers_near(near, 50.0).next().expect("shown");
        let mut sent = HashMap::new();
        roots.group(number, slot(true), &mut sent).expect("whole");
        roots.set_objective(0, |_| running(0, true, false));
        let started = objective_of(&roots, number, &mut sent);
        let Some(KeyedList::Full(starts)) = &started.task_start_times else {
            panic!("start times");
        };
        let instance = started.active.as_ref().expect("active").elements[0].instance;
        assert_eq!(
            starts.elements,
            [(TaskKey { instance, index: 1 }, unix_ns(STAGE_START_MS))]
        );
        roots.set_objective(0, |_| running(KILLS, true, true));
        let done = objective_of(&roots, number, &mut sent);
        let Some(KeyedList::Full(starts)) = &done.task_start_times else {
            panic!("start times");
        };
        assert!(starts.elements.is_empty(), "the key goes at the task's end");
    }

    /// A reset (the encounter idle again) drops the running objective without completing it.
    #[test]
    fn a_reset_drops_the_objective_without_completing_it() {
        let mut roots = EncounterRoots::build(&table(), &[tasked_encounter(0)]);
        roots.set(0, true, STATUS_IN_PROGRESS, &[STATUS_NOT_STARTED; STAGES]);
        let near = Position {
            x: AT[0],
            y: AT[1],
            z: AT[2],
        };
        let number = roots.numbers_near(near, 50.0).next().expect("shown");
        let mut sent = HashMap::new();
        roots.group(number, slot(true), &mut sent).expect("whole");
        roots.set_objective(0, |_| running(1, false, false));
        objective_of(&roots, number, &mut sent);
        roots.set_objective(0, |_| None);
        let dropped = objective_of(&roots, number, &mut sent);
        assert!(dropped.completed.is_none());
        assert!(dropped.active.as_ref().is_some_and(|list| list.elements.is_empty()));
        assert!(entries(&dropped).is_empty());
    }

    /// The retail start object of the Lucian event (its usual group): variation, position,
    /// gatherable controller, delayed event, placement obstruction.
    const OBJECT_SLICE: &str =
        "slices/quest/nighthaven/io_worldevent_17zqus1_encstartlucian.dynamicslice";

    fn object_table() -> TownProps {
        let members: Vec<String> = MEMBERS[2..3]
            .iter()
            .map(|hex| format!("\"{hex}\""))
            .collect();
        TownProps::parse(&format!(
            r#"{{"props": [{{"asset": "11223344556677889900aabbccddeeff", "slice": "{OBJECT_SLICE}",
            "members": [{}]}}]}}"#,
            members.join(",")
        ))
        .expect("table")
    }

    #[test]
    fn a_start_object_spawns_at_its_place_shows_once_and_goes_when_removed() {
        let mut objects = EncounterObjects::build(&object_table());
        assert_eq!(objects.spawn("slices/other.dynamicslice", AT, 0.0), None);
        let number = objects.spawn(OBJECT_SLICE, AT, 0.0).expect("spawned");
        assert!(EncounterObjects::is_object(number) && objects.stands(number));
        assert_eq!(objects.slice_of(number), Some(OBJECT_SLICE));
        let near = Position {
            x: AT[0],
            y: AT[1],
            z: AT[2],
        };
        assert_eq!(
            objects.numbers_near(near, 10.0).collect::<Vec<_>>(),
            [number]
        );
        let mut sent = HashMap::new();
        let group = objects
            .group(number, slot(true), &mut sent)
            .expect("spawn group");
        let MemberState::GdeMetadata(card) = &group.members[0].state else {
            panic!("card first");
        };
        let gde_ref = card.gde_ref.expect("id");
        assert_eq!(objects.at(&gde_ref, near, 3.0), Some(number));
        assert_eq!(
            objects.at(
                &gde_ref,
                Position {
                    x: AT[0] + 10.0,
                    ..near
                },
                3.0
            ),
            None,
            "out of reach"
        );
        let MemberState::Position(place) = &group.members[1].state else {
            panic!("the place");
        };
        assert!((place.position.expect("place").x - AT[0]).abs() < 0.01);
        assert!(
            objects.group(number, slot(false), &mut sent).is_none(),
            "no change since"
        );
        objects.remove(number);
        assert!(!objects.stands(number));
        assert_eq!(objects.slice_of(number), None);
        assert_eq!(objects.numbers_near(near, 10.0).count(), 0);
    }

    /// Retail `CATACOMBS5` 157.079 s: a gathered catacombs chest stays with its gatherable
    /// controller at 8; a viewer that saw it gets that state once, and no one can gather it.
    #[test]
    fn a_depleted_start_object_stays_sends_its_state_once_and_cannot_be_gathered() {
        let table = TownProps::parse(
            r#"{"props": [{"asset": "4f02cd285a915193bb49a98e5ca0ab49",
            "slice": "slices/test/chest.dynamicslice",
            "members": ["010d010342f14580447f28da1b0f03da81", "020c010104"]}]}"#,
        )
        .expect("table");
        let mut objects = EncounterObjects::build(&table);
        let number = objects
            .spawn("slices/test/chest.dynamicslice", AT, 0.0)
            .expect("spawned");
        let near = Position {
            x: AT[0],
            y: AT[1],
            z: AT[2],
        };
        let mut sent = HashMap::new();
        let first = objects.group(number, slot(true), &mut sent).expect("card");
        let MemberState::GdeMetadata(card) = &first.members[0].state else {
            panic!("card first");
        };
        let gde_ref = card.gde_ref.expect("id");
        assert!(objects.deplete(number));
        assert!(!objects.deplete(number), "once");
        assert!(objects.stands(number));
        assert_eq!(objects.at(&gde_ref, near, 3.0), None, "emptied");
        let change = objects
            .group(number, slot(false), &mut sent)
            .expect("the change");
        assert_eq!(change.members.len(), 1);
        assert!(matches!(
            change.members[0].state,
            MemberState::GatherableController(nw_messages::GatherableControllerState {
                gatherable_state: Some(DEPLETED_GATHERABLE_STATE)
            })
        ));
        assert!(objects.group(number, slot(false), &mut sent).is_none());
    }

    /// The encoded members of `group`.
    fn encoded_members(group: &BundleGroup) -> Vec<String> {
        group
            .members
            .iter()
            .map(|member| hex::encode(member.encode().expect("encodes")))
            .collect()
    }

    /// Retail `99a_msq_01b_learnsoulwardens`: the holdout glyph's interact state (c4, not in its
    /// spawn group) counts its gatherer at the gather's start (240.569 s, `010200000001`) and
    /// goes off at its end (242.570 s); a stop in between counts none.
    #[test]
    fn a_start_object_counts_its_gatherer_from_the_gathers_start() {
        let table = TownProps::parse(&format!(
            r#"{{"props": [{{"asset": "7b34ec22115254cc843a326be210eac6",
            "slice": "{}", "members": ["020d0103460597454585b419228a032986", "030c010104"]}}]}}"#,
            GATHER_VANISHES[0].slice
        ))
        .expect("table");
        let mut objects = EncounterObjects::build(&table);
        let number = objects
            .spawn(GATHER_VANISHES[0].slice, AT, 0.0)
            .expect("spawned");
        let mut sent = HashMap::new();
        objects.group(number, slot(true), &mut sent).expect("card");
        objects.gather_started(number);
        let started = objects.group(number, slot(false), &mut sent).expect("start");
        assert_eq!(encoded_members(&started), ["04b22d010200000001"]);
        objects.gather_stopped(number);
        let stopped = objects.group(number, slot(false), &mut sent).expect("stop");
        assert_eq!(encoded_members(&stopped), ["04b22d010200000000"]);
        objects.gather_started(number);
        objects.group(number, slot(false), &mut sent).expect("start again");
        objects.gathered(number, 0);
        let ended = objects.group(number, slot(false), &mut sent).expect("end");
        assert_eq!(
            encoded_members(&ended),
            ["030c010108", "04b22d01030000000000"]
        );
        let viewer = objects.group(number, slot(true), &mut HashMap::new()).expect("card");
        assert!(
            encoded_members(&viewer).contains(&"04b22d01030000000000".to_owned()),
            "a new viewer gets the whole interact state"
        );
        objects.gather_started(number);
        assert!(objects.group(number, slot(false), &mut sent).is_none(), "gathered once");
    }

    /// Retail `CATACOMBS5`: the chest's interact state (c3) counts its gatherer from 156.081 s
    /// and none at 157.079 s, when its controller goes to 8 and it stays.
    #[test]
    fn a_depleted_start_object_counts_no_gatherer() {
        let table = TownProps::parse(
            r#"{"props": [{"asset": "4f02cd285a915193bb49a98e5ca0ab49",
            "slice": "slices/test/chest.dynamicslice",
            "members": ["010d010342f14580447f28da1b0f03da81", "020c010104", "03b22d010101"]}]}"#,
        )
        .expect("table");
        let mut objects = EncounterObjects::build(&table);
        let number = objects
            .spawn("slices/test/chest.dynamicslice", AT, 0.0)
            .expect("spawned");
        let mut sent = HashMap::new();
        objects.group(number, slot(true), &mut sent).expect("card");
        objects.gather_started(number);
        let started = objects.group(number, slot(false), &mut sent).expect("start");
        assert_eq!(encoded_members(&started), ["03b22d010200000001"]);
        assert!(objects.deplete(number));
        let emptied = objects.group(number, slot(false), &mut sent).expect("emptied");
        assert_eq!(
            encoded_members(&emptied),
            ["020c010108", "03b22d010200000000"]
        );
    }

    #[test]
    fn stage_entity_events_go_out_on_the_root_at_the_target_s_component() {
        // The minor grove root: timelines 0x0a and 0x0b (version 7), placement obstruction 9.
        const TIMELINE: u64 = 0x0a;
        const OBSTRUCTION: u64 = 9;
        const DELAYED: u64 = 0x13;
        const DELAY_S: f32 = 3.5;
        const NOW: u64 = 1_000;
        let mut roots = EncounterRoots::build(&table(), &[encounter(STAGES)]);
        roots.set(0, true, STATUS_IN_PROGRESS, &[STATUS_NOT_STARTED; STAGES]);
        let number = ENCOUNTER_ROOT_ENTITY_BASE;
        let mut sent = HashMap::new();
        roots.group(number, slot(true), &mut sent).expect("whole");
        roots.entity_event(
            0,
            &[
                EntityComponent::Timeline {
                    component: TIMELINE,
                    length_s: 1.0,
                },
                EntityComponent::DelayedEvent {
                    component: DELAYED,
                    delay_s: DELAY_S,
                },
            ],
            EVENT_ACTIVATE,
            NOW,
        );
        roots.entity_event(
            0,
            &[EntityComponent::DelayedEvent {
                component: DELAYED,
                delay_s: DELAY_S,
            }],
            EVENT_TRIGGER,
            NOW,
        );
        roots.entity_event(
            0,
            &[EntityComponent::PlacementObstruction {
                component: OBSTRUCTION,
            }],
            EVENT_DEACTIVATE,
            NOW,
        );
        let change = roots.group(number, slot(false), &mut sent).expect("change");
        let member = |component: u64| {
            change
                .members
                .iter()
                .find(|member| member.component == component)
                .map(|member| member.encode().expect("member"))
        };
        assert_eq!(
            body_of(&change, TIMELINE),
            TimelineState::start(8).body().expect("start"),
            "Activate starts the timeline at the root's next version"
        );
        // Retail Dynasty cave monolith: `05833101014300` and `078b22010100` (component bytes
        // differ here).
        assert_eq!(
            member(DELAYED).expect("delay")[3..],
            [0x01, 0x01, 0x43, 0x00]
        );
        assert_eq!(
            member(OBSTRUCTION).expect("obstruction")[3..],
            [0x01, 0x01, 0x00]
        );
        roots.tick(NOW + 3_499);
        roots.tick(NOW + 3_500);
        let done = roots.group(number, slot(false), &mut sent).expect("done");
        assert!(done.members.iter().any(|member| {
            member.component == DELAYED && member.state == MemberState::DelayedEvent(DELAY_DONE)
        }));
        assert!(
            done.members
                .iter()
                .all(|member| member.component != OBSTRUCTION),
            "a change goes to a viewer once"
        );
    }

    #[test]
    fn a_history_keeps_its_newest_entries() {
        // A repeated wave, as the breaches' sub-waves run: in progress and waiting by turns.
        const TURNS: u32 = 5;
        let mut history = History::default();
        for _ in 0..TURNS {
            assert!(history.set(STATUS_IN_PROGRESS));
            assert!(history.set(STATUS_NOT_STARTED));
        }
        assert!(history.set(STATUS_SUCCEEDED));
        assert!(
            !history.set(STATUS_SUCCEEDED),
            "the same status adds nothing"
        );
        assert_eq!(history.entries.len(), MAX_HISTORY);
        assert_eq!(history.entries.first(), Some(&(3, STATUS_NOT_STARTED)));
        assert_eq!(history.entries.last(), Some(&(12, STATUS_SUCCEEDED)));
        // Retail breach body (2026-10-28 ledgers): its first and last bytes.
        let body = history.body();
        assert_eq!(&body[..3], &[1, 1, 10]);
        assert_eq!(&body[body.len() - 8..], &[0, 0, 0, 12, 0, 0, 0, 3]);
    }
}
