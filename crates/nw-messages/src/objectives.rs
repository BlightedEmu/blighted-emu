// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ObjectivesComponentReplicatedState` (type 3857, own player component 22): the quests the
//! player has and the state of their tasks; the client's quest tracker and journal read it
//! (objectives-state note, sections 2 and 3).
//!
//! Slot 0 holds `gracePeriodEndTime`; slot 1 holds seven children (client reflection
//! `FUN_143a1f0b0`, in this order): the task start times (child 0), the tracked objectives (1),
//! the completed ones (2), the active ones (3), the task states (4), the POI entity ids (5) and
//! the dynamic POI indices (6). The server sends no text: the client takes the quest and task
//! texts from its own data for the ids.
//!
//! Observed (encounter objectives finding 2026-11-13: 9,234 bodies on encounter roots and game
//! modes and 16,781 own bodies of the capture index, every one read to its end): children 1, 3
//! and 4 come in the full list form only; child 2 also as list changes; child 0 is a map in the
//! full form or as keyed changes; child 5 also as index-keyed changes with removals; child 6 is
//! always the empty full list.

use crate::items::{FULL_FORM, FullList, VERSION_PRESENT};
use crate::keyed_list::{IndexedList, KeyedList, MapKey};
use crate::list_delta::ReplicatedList;
use crate::slots::{read_children, read_slot_byte, write_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `ObjectivesComponentReplicatedState`.
pub const OBJECTIVES_TYPE_INDEX: u64 = 3857;
/// The own player component of the objectives.
pub const OBJECTIVES_COMPONENT: u64 = 22;

/// Slots of the class: slot 0 (`gracePeriodEndTime`) and slot 1 (the lists).
const SLOTS: u8 = 2;
/// Slot byte bit of slot 0.
const SLOT_0: u8 = 0x01;
/// Slot byte bit of slot 1.
const SLOT_1: u8 = 0x02;
/// Children of slot 0: `gracePeriodEndTime` alone.
const SLOT_0_CHILDREN: u8 = 1;
/// The child bitmap of slot 0.
const GRACE_PERIOD_CHILDREN: u16 = 1;
/// Children of slot 1.
const SLOT_1_CHILDREN: u8 = 7;
const START_TIMES_CHILD: u8 = 0;
const TRACKED_CHILD: u8 = 1;
const COMPLETED_CHILD: u8 = 2;
const ACTIVE_CHILD: u8 = 3;
const TASKS_CHILD: u8 = 4;
const POI_ENTITIES_CHILD: u8 = 5;
const DYNAMIC_POI_CHILD: u8 = 6;
/// The element count of the dynamic POI index list (Observed: always empty).
const EMPTY: u64 = 0;
/// Bytes of `m_destinationOverride`.
const DESTINATION_LEN: usize = 16;
const FIELD: &str = "objectives";

/// The state of a task on the wire (`EObjectiveTaskState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskWireState {
    /// It runs.
    Active,
    /// It is done.
    Complete,
    /// It is done and still runs (`CompleteActive`).
    CompleteActive,
    /// It stopped undone: a branch of a partial container that another branch finished
    /// (beach-quests note: `SwapTimerNeverComplete_container` of `12_skinningknife`, progress 2).
    Inactive,
    /// It failed (`Failed`).
    Failed,
}

impl TaskWireState {
    // The values of the client's `EObjectiveTaskState` (Observed: its reflection in
    // `FUN_143aa5810` names them in this order).
    const ACTIVE: u8 = 1;
    const COMPLETE: u8 = 2;
    const COMPLETE_ACTIVE: u8 = 3;
    const INACTIVE: u8 = 4;
    const FAILED: u8 = 5;

    const fn byte(self) -> u8 {
        match self {
            Self::Active => Self::ACTIVE,
            Self::Complete => Self::COMPLETE,
            Self::CompleteActive => Self::COMPLETE_ACTIVE,
            Self::Inactive => Self::INACTIVE,
            Self::Failed => Self::FAILED,
        }
    }

    fn of(byte: u8) -> Result<Self, MessageError> {
        match byte {
            Self::ACTIVE => Ok(Self::Active),
            Self::COMPLETE => Ok(Self::Complete),
            Self::COMPLETE_ACTIVE => Ok(Self::CompleteActive),
            Self::INACTIVE => Ok(Self::Inactive),
            Self::FAILED => Ok(Self::Failed),
            byte => Err(MessageError::Unsized { field: FIELD, byte }),
        }
    }
}

/// One entry of the task states (18 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskEntry {
    /// The objective instance the task belongs to.
    pub instance: u64,
    /// The task's post-order number in its objective's task tree.
    pub index: u8,
    /// CRC-32 of the lowercase task id.
    pub task: u32,
    /// Progress: children done, kills, items, or 1 for a done leaf.
    pub progress: u32,
    /// Its state.
    pub state: TaskWireState,
}

impl TaskEntry {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            instance: reader.u64()?,
            index: reader.u8()?,
            task: reader.u32()?,
            progress: reader.u32()?,
            state: TaskWireState::of(reader.u8()?)?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .u64(self.instance)
            .u8(self.index)
            .u32(self.task)
            .u32(self.progress)
            .u8(self.state.byte());
    }
}

/// The key of a task start time (child 0): the objective instance and the task's post-order
/// number. Observed: the holdout's key `a24bddd6263090ad 03` with the start of its timed task 3
/// at 245.585 s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskKey {
    /// The objective instance.
    pub instance: u64,
    /// The task's post-order number in its objective's task tree.
    pub index: u8,
}

impl MapKey for TaskKey {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            instance: reader.u64()?,
            index: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.u64(self.instance).u8(self.index);
        Ok(())
    }
}

/// One active objective (56 bytes with no ingredients). Every retail objective record of the
/// captures has `available` set and every other field 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveRecord {
    /// CRC-32 of the lowercase objective id.
    pub objective: u32,
    /// The instance id of the player's copy of the objective.
    pub instance: u64,
    /// `m_missionId`.
    pub mission: u32,
    /// `m_destinationOverride`.
    pub destination: [u8; DESTINATION_LEN],
    /// `m_acceptTime`.
    pub accept_time: u64,
    /// `m_originTerritoryId`.
    pub origin_territory: u16,
    /// `m_available`.
    pub available: bool,
    /// `m_isCommunityGoal`.
    pub community_goal: bool,
    /// `m_isPoiObjective`.
    pub poi: bool,
    /// `m_isDynamicPoiObjective`.
    pub dynamic_poi: bool,
    /// `m_parentObjectiveId`.
    pub parent: u64,
    /// `m_isResetObjective`.
    pub reset: bool,
    /// `m_selectedIngredientVector`.
    pub ingredients: Vec<u32>,
}

impl ObjectiveRecord {
    /// The record of instance `instance` of objective `objective`, as retail sends it.
    pub const fn available(objective: u32, instance: u64) -> Self {
        Self {
            objective,
            instance,
            mission: 0,
            destination: [0; DESTINATION_LEN],
            accept_time: 0,
            origin_territory: 0,
            available: true,
            community_goal: false,
            poi: false,
            dynamic_poi: false,
            parent: 0,
            reset: false,
            ingredients: Vec::new(),
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut record = Self {
            objective: reader.u32()?,
            instance: reader.u64()?,
            mission: reader.u32()?,
            destination: reader.array()?,
            accept_time: reader.u64()?,
            origin_territory: reader.u16()?,
            available: reader.boolean()?,
            community_goal: reader.boolean()?,
            poi: reader.boolean()?,
            dynamic_poi: reader.boolean()?,
            parent: reader.u64()?,
            reset: reader.boolean()?,
            ingredients: Vec::new(),
        };
        let count = reader.wide_prefixed()?;
        for _ in 0..count {
            record.ingredients.push(reader.u32()?);
        }
        Ok(record)
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .u32(self.objective)
            .u64(self.instance)
            .u32(self.mission)
            .bytes(&self.destination)
            .u64(self.accept_time)
            .u16(self.origin_territory)
            .boolean(self.available)
            .boolean(self.community_goal)
            .boolean(self.poi)
            .boolean(self.dynamic_poi)
            .u64(self.parent)
            .boolean(self.reset);
        // A `usize` count fits 64 bits.
        writer.wide_prefixed(self.ingredients.len() as u64);
        for ingredient in &self.ingredients {
            writer.u32(*ingredient);
        }
    }
}

/// The objectives state; a part that is `None` is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectivesState {
    /// Slot 0: `gracePeriodEndTime`.
    pub grace_period_end: Option<u64>,
    /// Child 0: the start time of each timed task, Unix nanoseconds (Observed: the holdout's
    /// timed task 3 at 245.585 s = the ledger start + 245.585 s, 67 ms later).
    pub task_start_times: Option<KeyedList<TaskKey, u64>>,
    /// Child 1: the tracked objective instances.
    pub tracked: Option<FullList<u64>>,
    /// Child 2: the completed objective instances, whole or as changes: retail adds a quest that
    /// ends as one change (newchar 2026-09-18, 492.95 s: index 1, the tutorial's instance, in
    /// four bundles, then every other list empty in three).
    pub completed: Option<ReplicatedList<u64>>,
    /// Child 3: the active objectives.
    pub active: Option<FullList<ObjectiveRecord>>,
    /// Child 4: the task states.
    pub tasks: Option<FullList<TaskEntry>>,
    /// Child 5: the entity ids of the objectives' POIs (Observed: CATACOMBS3 1023.049 s removed
    /// index 1 as a change).
    pub poi_entities: Option<IndexedList<u64>>,
    /// Child 6: the version of the dynamic POI index list, which retail always sends empty
    /// (3,504 of 3,504 bodies); its element type is Unknown, so a list with elements does not
    /// read.
    pub dynamic_poi_version: Option<u64>,
}

impl ObjectivesState {
    fn children(&self) -> u16 {
        [
            (START_TIMES_CHILD, self.task_start_times.is_some()),
            (TRACKED_CHILD, self.tracked.is_some()),
            (COMPLETED_CHILD, self.completed.is_some()),
            (ACTIVE_CHILD, self.active.is_some()),
            (TASKS_CHILD, self.tasks.is_some()),
            (POI_ENTITIES_CHILD, self.poi_entities.is_some()),
            (DYNAMIC_POI_CHILD, self.dynamic_poi_version.is_some()),
        ]
        .into_iter()
        .filter(|(_, present)| *present)
        .fold(0, |bits, (child, _)| bits | 1 << child)
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sends a list in a form retail never
    /// sends (module text), a dynamic POI index, or a task state that is not 1 to 5.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, FIELD, SLOTS)?;
        let mut state = Self::default();
        if slots & SLOT_0 != 0 {
            read_children(reader, FIELD, SLOT_0_CHILDREN, |reader, _| {
                state.grace_period_end = Some(reader.u64()?);
                Ok(())
            })?;
        }
        if slots & SLOT_1 == 0 {
            return Ok(state);
        }
        read_children(reader, FIELD, SLOT_1_CHILDREN, |reader, child| {
            match child {
                START_TIMES_CHILD => {
                    state.task_start_times = Some(KeyedList::read(reader, |reader| reader.u64())?);
                }
                TRACKED_CHILD => {
                    state.tracked = Some(FullList::read(reader, |reader| reader.u64())?)
                }
                COMPLETED_CHILD => {
                    state.completed =
                        Some(ReplicatedList::read(reader, FIELD, |reader| reader.u64())?);
                }
                ACTIVE_CHILD => {
                    state.active = Some(FullList::read(reader, |reader| {
                        ObjectiveRecord::read(reader)
                    })?);
                }
                TASKS_CHILD => state.tasks = Some(FullList::read(reader, TaskEntry::read)?),
                POI_ENTITIES_CHILD => {
                    state.poi_entities = Some(IndexedList::read(reader, |reader| reader.u64())?);
                }
                _ => {
                    let list = FullList::<u64>::read(reader, |_| {
                        Err(MessageError::Unsized {
                            field: FIELD,
                            byte: DYNAMIC_POI_CHILD,
                        })
                    })?;
                    state.dynamic_poi_version = list.version;
                }
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list cannot be written.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let children = self.children();
        let slot_0 = if self.grace_period_end.is_some() {
            SLOT_0
        } else {
            0
        };
        let slot_1 = if children == 0 { 0 } else { SLOT_1 };
        writer.u8(slot_0 | slot_1);
        if let Some(end) = self.grace_period_end {
            write_children(writer, GRACE_PERIOD_CHILDREN, SLOT_0_CHILDREN, |writer, _| {
                writer.u64(end);
                Ok::<(), MessageError>(())
            })?;
        }
        if children == 0 {
            return Ok(());
        }
        let ids = |writer: &mut Writer, id: &u64| {
            writer.u64(*id);
            Ok::<(), MessageError>(())
        };
        write_children(writer, children, SLOT_1_CHILDREN, |writer, child| {
            match child {
                START_TIMES_CHILD => self
                    .task_start_times
                    .as_ref()
                    .map(|map| map.write(writer, ids)),
                TRACKED_CHILD => self.tracked.as_ref().map(|list| list.write(writer, ids)),
                COMPLETED_CHILD => self.completed.as_ref().map(|list| list.write(writer, ids)),
                ACTIVE_CHILD => self.active.as_ref().map(|list| {
                    list.write(writer, |writer, record| {
                        record.write(writer);
                        Ok(())
                    })
                }),
                TASKS_CHILD => self.tasks.as_ref().map(|list| {
                    list.write(writer, |writer, entry| {
                        entry.write(writer);
                        Ok(())
                    })
                }),
                POI_ENTITIES_CHILD => self
                    .poi_entities
                    .as_ref()
                    .map(|list| list.write(writer, ids)),
                DYNAMIC_POI_CHILD => self.dynamic_poi_version.map(|version| {
                    writer
                        .wide_prefixed(FULL_FORM)
                        .u8(VERSION_PRESENT)
                        .wide_prefixed(version)
                        .wide_prefixed(EMPTY);
                    Ok(())
                }),
                _ => None,
            }
            .unwrap_or(Ok(()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let text: String = text.split_whitespace().collect();
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    /// The retail state of the first own card (objectives-state note, section 3, 316 ms).
    const RETAIL_FIRST: &str = "02 1a
        00 01 03 01 4a41a9251454158b
        00 01 03 01 eea940b3 4a41a9251454158b 00000000
           00000000000000000000000000000000 0000000000000000 0000 01 00 00 00 0000000000000000 00 00
        00 01 03 02
           4a41a9251454158b 00 24affe65 00000001 02
           4a41a9251454158b 07 2a36e8c7 00000001 01";
    const INSTANCE: u64 = 0x4a41_a925_1454_158b;
    const TUTORIAL: u32 = 0xeea9_40b3;

    #[test]
    fn a_retail_quest_end_adds_its_instance_to_the_completed_list_as_a_change() {
        // Newchar 2026-09-18, 492.95 s: child 2 (completed) as one delta entry, index 1 (the list
        // held the tutorial's earlier quest), the ended quest's instance.
        const ENDED: u64 = 0xb741_6b22_fdfc_9500;
        let bytes = hex("02 04 01 01 01 01 94d0 b7416b22fdfc9500");
        let mut reader = Reader::new(&bytes);
        let state = ObjectivesState::read(&mut reader).expect("read");
        reader.finish().expect("whole body");
        let Some(ReplicatedList::Changes(changes)) = &state.completed else {
            panic!("a change: {state:?}");
        };
        assert_eq!(
            changes
                .iter()
                .map(|change| (change.index, change.value))
                .collect::<Vec<_>>(),
            [(1, ENDED)]
        );
        assert_eq!(
            (&state.tracked, &state.active, &state.tasks),
            (&None, &None, &None)
        );
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), bytes);
    }

    #[test]
    fn a_task_state_reads_every_value_of_the_clients_enum() {
        for (byte, state) in [
            (1, TaskWireState::Active),
            (2, TaskWireState::Complete),
            (3, TaskWireState::CompleteActive),
            (4, TaskWireState::Inactive),
            (5, TaskWireState::Failed),
        ] {
            assert_eq!(TaskWireState::of(byte), Ok(state));
            assert_eq!(state.byte(), byte);
        }
        assert!(TaskWireState::of(6).is_err());
    }

    #[test]
    fn the_first_retail_state_reads_and_writes_the_same_bytes() {
        let bytes = hex(RETAIL_FIRST);
        let mut reader = Reader::new(&bytes);
        let state = ObjectivesState::read(&mut reader).expect("read");
        reader.finish().expect("whole body");
        let expected = ObjectivesState {
            tracked: Some(FullList::first(vec![INSTANCE])),
            completed: None,
            active: Some(FullList::first(vec![ObjectiveRecord::available(
                TUTORIAL, INSTANCE,
            )])),
            tasks: Some(FullList::first(vec![
                TaskEntry {
                    instance: INSTANCE,
                    index: 0,
                    task: 0x24af_fe65,
                    progress: 1,
                    state: TaskWireState::Complete,
                },
                TaskEntry {
                    instance: INSTANCE,
                    index: 7,
                    task: 0x2a36_e8c7,
                    progress: 1,
                    state: TaskWireState::Active,
                },
            ])),
            ..ObjectivesState::default()
        };
        assert_eq!(state, expected);
        let mut writer = Writer::new();
        expected.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), bytes);
    }

    #[test]
    fn a_stopped_branch_is_inactive() {
        // Retail 12_skinningknife task list after the knife (beach-quests note, q2 fragment
        // 21238): wire task 17, two children done, state 4.
        const RETAIL: &str = "02 10 00 01 db9803 0f
            ff4dcb963adf2874 00 e8c3135a 00000001 02
            ff4dcb963adf2874 01 32edcfd1 00000001 02
            ff4dcb963adf2874 02 872c6d15 00000001 02
            ff4dcb963adf2874 13 39d64279 00000004 02
            ff4dcb963adf2874 06 91915849 00000001 02
            ff4dcb963adf2874 08 f8522a74 00000002 02
            ff4dcb963adf2874 07 5b3e6aa2 00000001 02
            ff4dcb963adf2874 0a a49a7a51 00000001 02
            ff4dcb963adf2874 0d 403c9da3 00000003 02
            ff4dcb963adf2874 0e 873461ce 00000001 02
            ff4dcb963adf2874 0f b55ade17 00000001 02
            ff4dcb963adf2874 11 cfc907fc 00000002 04
            ff4dcb963adf2874 0c b55ade17 00000001 02
            ff4dcb963adf2874 0b 03ce66c4 00000001 02
            ff4dcb963adf2874 12 2f7459a3 00000001 02";
        const STOPPED: u8 = 0x11;
        let bytes = hex(RETAIL);
        let mut reader = Reader::new(&bytes);
        let state = ObjectivesState::read(&mut reader).expect("read");
        reader.finish().expect("whole body");
        let tasks = state.tasks.clone().expect("tasks").elements;
        let stopped = tasks
            .iter()
            .find(|entry| entry.index == STOPPED)
            .expect("stopped");
        assert_eq!(stopped.state, TaskWireState::Inactive);
        assert_eq!(stopped.progress, 2);
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), bytes);
    }

    #[test]
    fn a_later_change_sends_the_task_list_alone_and_an_empty_state_no_slot() {
        // Retail 34700 ms: `02 10 00 01 a80a 03 ...`.
        let state = ObjectivesState {
            tasks: Some(FullList {
                version: Some(680),
                elements: Vec::new(),
            }),
            ..ObjectivesState::default()
        };
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), hex("02 10 00 01 a80a 00"));
        let mut writer = Writer::new();
        ObjectivesState::default()
            .write(&mut writer)
            .expect("write");
        assert_eq!(writer.finish(), [0]);
    }

    /// Reads `body` to its end and writes it back.
    fn read_and_write(body: &[u8]) -> ObjectivesState {
        let mut reader = Reader::new(body);
        let state = ObjectivesState::read(&mut reader).expect("read");
        assert!(reader.is_empty(), "left: {:02x?}", reader.rest());
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), body);
        state
    }

    /// The holdout root's objective instance (`99a_msq_01b_learnsoulwardens`, slot 38, c4).
    const HOLDOUT_INSTANCE: u64 = 0xa24b_ddd6_2630_90ad;

    #[test]
    fn the_holdout_root_s_three_retail_objective_states_read_and_write_the_same_bytes() {
        // 224.972 s: the member's first body (tracked, active, tasks; version 4).
        let first = read_and_write(&hex("02 1a 00 01 04 00
            00 01 04 01 5d5de770 a24bddd6263090ad 00000000 00000000000000000000000000000000
               18d9397566e96bd0 0000 01 00 01 00 0000000000000000 00 00
            00 01 04 00"));
        let record = &first.active.as_ref().expect("active").elements[0];
        assert_eq!(record.objective, 0x5d5d_e770);
        assert_eq!(record.accept_time, 0x18d9_3975_66e9_6bd0);
        assert!(record.available && record.poi);
        // 245.585 s (Holdout start): the timed task 3's start and the task states (version 6).
        let holdout = read_and_write(&hex("02 11
            00 01 06 01 a24bddd6263090ad 03 18d93b89e7c7d7f8
            00 01 06 03 a24bddd6263090ad 01 230690d4 00000001 02
                        a24bddd6263090ad 02 a2781fd7 00000001 02
                        a24bddd6263090ad 07 1004d537 00000001 01"));
        let Some(KeyedList::Full(starts)) = holdout.task_start_times else {
            panic!("start times");
        };
        assert_eq!(
            starts.elements,
            [(
                TaskKey {
                    instance: HOLDOUT_INSTANCE,
                    index: 3,
                },
                0x18d9_3b89_e7c7_d7f8,
            )]
        );
        // 305.600 s (Get Out start): the objective is done (version 7).
        let done = read_and_write(&hex(
            "02 1f 00 01 07 00 00 01 07 00 00 01 07 01 a24bddd6263090ad 00 01 07 00 00 01 07 00",
        ));
        assert_eq!(
            done.completed.and_then(|list| list.full().cloned()),
            Some(FullList {
                version: Some(7),
                elements: vec![HOLDOUT_INSTANCE],
            })
        );
    }

    #[test]
    fn a_task_start_time_can_come_as_a_keyed_change() {
        // Playthrough part 21, 2103.561 s: child 0 as one change with version 8.
        let state = read_and_write(&hex("02 11
            01 01 9245c9999d02eabe 03 01 08 18e4e99333c0eb3c
            00 01 08 03 9245c9999d02eabe 01 d54b12b0 00000001 02
                        9245c9999d02eabe 02 a2781fd7 00000001 02
                        9245c9999d02eabe 11 1004d537 00000001 01"));
        let Some(KeyedList::Changes(changes)) = state.task_start_times else {
            panic!("changes");
        };
        assert_eq!(changes[0].key.index, 3);
        assert_eq!(changes[0].version, Some(8));
        assert_eq!(changes[0].value, Some(0x18e4_e993_33c0_eb3c));
    }

    #[test]
    fn the_own_grace_period_poi_ids_and_dynamic_poi_list_read_as_retail_sent_them() {
        // EXP Amrine Excavation 381.530 s: slot 0 alone.
        let grace = read_and_write(&hex("01 01 0000000000000000"));
        assert_eq!(grace.grace_period_end, Some(0));
        // CATACOMBS3 1023.049 s: child 5 removes index 1.
        let removed = read_and_write(&hex("02 20 01 00 01 01 d5b903"));
        let Some(IndexedList::Changes(changes)) = removed.poi_entities else {
            panic!("changes");
        };
        assert_eq!(changes[0].value, None);
        // A spin test 0.327 s: children 1, 3, 4 and the empty dynamic POI list (6).
        let dynamic = read_and_write(&hex("02 5a 00 01 8e24 00 00 01 06 00 00 01 06 00 00 01 06 00"));
        assert_eq!(dynamic.dynamic_poi_version, Some(6));
        let mut reader = Reader::new(&[0x02, 0x40, 0x00, 0x01, 0x06, 0x01, 0x00]);
        assert!(ObjectivesState::read(&mut reader).is_err());
    }
}
