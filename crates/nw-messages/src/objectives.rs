// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ObjectivesComponentReplicatedState` (type 3857, own player component 22): the quests the
//! player has and the state of their tasks; the client's quest tracker and journal read it
//! (objectives-state note, sections 2 and 3).
//!
//! Slot 1 holds seven list children; the tutorial uses four: the tracked objectives (child 1),
//! the completed ones (2), the active ones (3) and the task states (4). Each is sent in the full
//! list form. The server sends no text: the client takes the quest and task texts from its own
//! data for the ids.

use crate::items::FullList;
use crate::slots::{read_children, read_slot_byte, write_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `ObjectivesComponentReplicatedState`.
pub const OBJECTIVES_TYPE_INDEX: u64 = 3857;
/// The own player component of the objectives.
pub const OBJECTIVES_COMPONENT: u64 = 22;

/// Slots of the class: slot 0 (`gracePeriodEndTime`) and slot 1 (the lists).
const SLOTS: u8 = 2;
/// Slot byte bit of slot 1.
const SLOT_1: u8 = 0x02;
/// Children of slot 1.
const SLOT_1_CHILDREN: u8 = 7;
const TRACKED_CHILD: u8 = 1;
const COMPLETED_CHILD: u8 = 2;
const ACTIVE_CHILD: u8 = 3;
const TASKS_CHILD: u8 = 4;
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
}

impl TaskWireState {
    const ACTIVE: u8 = 1;
    const COMPLETE: u8 = 2;

    const fn byte(self) -> u8 {
        match self {
            Self::Active => Self::ACTIVE,
            Self::Complete => Self::COMPLETE,
        }
    }

    fn of(byte: u8) -> Result<Self, MessageError> {
        match byte {
            Self::ACTIVE => Ok(Self::Active),
            Self::COMPLETE => Ok(Self::Complete),
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

/// The objectives state; a list that is `None` is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectivesState {
    /// Child 1: the tracked objective instances.
    pub tracked: Option<FullList<u64>>,
    /// Child 2: the completed objective instances.
    pub completed: Option<FullList<u64>>,
    /// Child 3: the active objectives.
    pub active: Option<FullList<ObjectiveRecord>>,
    /// Child 4: the task states.
    pub tasks: Option<FullList<TaskEntry>>,
}

impl ObjectivesState {
    fn children(&self) -> u16 {
        [
            (TRACKED_CHILD, self.tracked.is_some()),
            (COMPLETED_CHILD, self.completed.is_some()),
            (ACTIVE_CHILD, self.active.is_some()),
            (TASKS_CHILD, self.tasks.is_some()),
        ]
        .into_iter()
        .filter(|(_, present)| *present)
        .fold(0, |bits, (child, _)| bits | 1 << child)
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets slot 0 or a child other than 1
    /// to 4, sends a list in the delta form, or a task state is not 1 or 2.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, FIELD, SLOTS)?;
        let mut state = Self::default();
        if slots == 0 {
            return Ok(state);
        }
        if slots != SLOT_1 {
            return Err(MessageError::Unsized {
                field: FIELD,
                byte: slots,
            });
        }
        read_children(reader, FIELD, SLOT_1_CHILDREN, |reader, child| {
            match child {
                TRACKED_CHILD => {
                    state.tracked = Some(FullList::read(reader, |reader| reader.u64())?)
                }
                COMPLETED_CHILD => {
                    state.completed = Some(FullList::read(reader, |reader| reader.u64())?)
                }
                ACTIVE_CHILD => {
                    state.active = Some(FullList::read(reader, |reader| {
                        ObjectiveRecord::read(reader)
                    })?);
                }
                TASKS_CHILD => state.tasks = Some(FullList::read(reader, TaskEntry::read)?),
                child => {
                    return Err(MessageError::Unsized {
                        field: FIELD,
                        byte: child,
                    });
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
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_1);
        write_children(writer, children, SLOT_1_CHILDREN, |writer, child| {
            match child {
                TRACKED_CHILD => self.tracked.as_ref().map(|list| {
                    list.write(writer, |writer, id| {
                        writer.u64(*id);
                        Ok(())
                    })
                }),
                COMPLETED_CHILD => self.completed.as_ref().map(|list| {
                    list.write(writer, |writer, id| {
                        writer.u64(*id);
                        Ok(())
                    })
                }),
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
        };
        assert_eq!(state, expected);
        let mut writer = Writer::new();
        expected.write(&mut writer).expect("write");
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
}
