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
//! as any gatherable ([`super::gathering`]); the gather ends it, and its stage counts it.

use std::collections::HashMap;

use nw_content::dungeon_gates::EntityComponent;
use nw_content::town_props::TownProps;
use nw_messages::position::{PositionInTheWorldReplicatedState, Rotation, WorldPosition};
use nw_messages::timeline_state::{TIMELINE_TYPE_INDEX, TimelineState};
use nw_messages::{
    AssetId, BundleGroup, BundleMember, DYNAMIC_SLICE_SUB_ID, DelayedEventState,
    GdeMetadataReplicatedState, MemberState, PlacementObstructionState, Reader,
};
use nw_sim_core::Position;
use nw_world::dungeon_scripts::Encounter;

use super::crafting::{CARD_COMPONENT, dynamic_ref};
use super::dungeon_script::{EVENT_ACTIVATE, EVENT_DEACTIVATE, EVENT_TRIGGER};
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
const MANAGER_CLASS: u64 = 6786;
/// `MB::EncounterComponentReplicatedState`.
const STAGE_CLASS: u64 = 2133;
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
struct History {
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
    fn status(&self) -> u32 {
        self.entries
            .last()
            .map_or(STATUS_UNINITIALIZED, |entry| entry.1)
    }

    /// Adds `status` when it is not the newest; `true` when it changed.
    fn set(&mut self, status: u32) -> bool {
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

    fn body(&self) -> Vec<u8> {
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
        let mut body = STATE_HEAD.to_vec();
        body.extend_from_slice(&self.manager.to_be_bytes());
        recorded(self.manager_component, MANAGER_CLASS, body)
    }

    fn state_members(&self) -> impl Iterator<Item = BundleMember> + '_ {
        std::iter::once(self.manager_member()).chain(
            self.stage_components
                .iter()
                .zip(&self.stages)
                .map(|(component, history)| recorded(*component, STAGE_CLASS, history.body())),
        )
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

fn recorded(component: u64, class: u64, body: Vec<u8>) -> BundleMember {
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
                            MANAGER_CLASS | STAGE_CLASS => None,
                            POSITION_CLASS => {
                                Some(place_member(member.component, encounter.position, half_yaw))
                            }
                            _ => Some(member),
                        }),
                )
                .collect();
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
}

/// One start object in the world.
#[derive(Debug, Clone, PartialEq)]
struct StartObject {
    gde_ref: [u8; nw_messages::RAW_ID_LEN],
    position: Position,
    members: Vec<BundleMember>,
}

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
                Some((
                    row.slice.clone()?,
                    ObjectKind {
                        asset: row.asset,
                        members: row_members(&row.members)?,
                    },
                ))
            })
            .collect();
        Self {
            kinds,
            ..Self::default()
        }
    }

    /// Spawns a start object of slice `slice` at `at`; its entity number, or `None` for a
    /// slice that is no start object of the table.
    pub(super) fn spawn(&mut self, slice: &str, at: [f32; 3]) -> Option<u64> {
        let kind = self.kinds.get(slice)?;
        let index = usize::try_from(self.spawned).ok()?;
        let number = ENCOUNTER_OBJECT_ENTITY_BASE + self.spawned;
        self.spawned += 1;
        let gde_ref = dynamic_ref(OBJECT_NAMESPACE, index);
        let members = std::iter::once(card(kind.asset, gde_ref))
            .chain(kind.members.iter().map(|member| {
                if member.state.type_index() == POSITION_CLASS {
                    place_member(member.component, at, 0.0)
                } else {
                    member.clone()
                }
            }))
            .collect();
        let [x, y, z] = at;
        self.objects.insert(
            number,
            StartObject {
                gde_ref,
                position: Position { x, y, z },
                members,
            },
        );
        Some(number)
    }

    /// Whether `slice` is the slice of a start object of the table.
    pub(super) fn is_kind(&self, slice: &str) -> bool {
        self.kinds.contains_key(slice)
    }

    /// Whether `entity` is a start object's number.
    pub(super) const fn is_object(entity: u64) -> bool {
        entity >= ENCOUNTER_OBJECT_ENTITY_BASE
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

    /// The spawn group of start object `number` for a viewer that gives it `slot`: only in a
    /// new slot (the object does not change; it goes when gathered).
    pub(super) fn group(&self, number: u64, slot: Slot) -> Option<BundleGroup> {
        let object = self.objects.get(&number).filter(|_| slot.new)?;
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members: object.members.clone(),
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
            MEMBERS.len() + 1,
            "the card and every state"
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
        assert_eq!(objects.spawn("slices/other.dynamicslice", AT), None);
        let number = objects.spawn(OBJECT_SLICE, AT).expect("spawned");
        assert!(EncounterObjects::is_object(number) && objects.stands(number));
        let near = Position {
            x: AT[0],
            y: AT[1],
            z: AT[2],
        };
        assert_eq!(
            objects.numbers_near(near, 10.0).collect::<Vec<_>>(),
            [number]
        );
        let group = objects.group(number, slot(true)).expect("spawn group");
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
            objects.group(number, slot(false)).is_none(),
            "it does not change"
        );
        objects.remove(number);
        assert!(!objects.stands(number));
        assert_eq!(objects.numbers_near(near, 10.0).count(), 0);
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
