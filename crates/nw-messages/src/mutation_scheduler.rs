// SPDX-License-Identifier: AGPL-3.0-or-later
//! The weekly expedition mutations: `Javelin::GameModeMutationSchedulerReplicatedState` (type
//! 2201), the state of the `GameModeMutationSchedulerComponent` (run 15 dungeon study,
//! `local-data/scratch-work/run15/dungeon2/REPORT-mutations.md`).
//!
//! Observed (549 retail ledgers, 19 weeks): the server sends the state as component
//! [`MUTATION_SCHEDULER_COMPONENT`] of entity slot [`MUTATION_SCHEDULER_INTEREST`], a slot the
//! client never gets a spawn card for (retail newchar 2026-09-18: the first slot-2 group 9 s
//! after the first bundle). The constructor `FUN_14333b440` registers two children:
//!
//! - `gameModeMutationSets`: a version list of 16-byte entries, each four big-endian CRC-32s of
//!   lowercase ids: the game mode (`dungeoncutlasskeys00`), the elemental category (`ice`), the
//!   promotion (`barbaric`) and the curse (`fiendish`) — the script struct `GameModeMutationSet`
//!   (`elementalId`, `promotionId`, `curseId`). Every capture has 3 entries, all with the same
//!   elemental, promotion and curse.
//! - `mutationCadenceStartTime`: a big-endian `u64`, Unix nanoseconds of the week's start
//!   (Tuesday 07:00 UTC in all 19 weeks).
//!
//! The body is the generic slot reader: one slot, two children (`crate::slots`).

use crate::items::FullList;
use crate::payload::{BundleGroup, BundleMember, MemberState};
use crate::slots::{read_slot_byte, read_wide_children, write_wide_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `Javelin::GameModeMutationSchedulerReplicatedState`.
pub const MUTATION_SCHEDULER_TYPE_INDEX: u64 = 2201;
/// The component of the slot-2 entity that takes the state (Observed in all 549 ledgers).
pub const MUTATION_SCHEDULER_COMPONENT: u64 = 6;
/// The entity slot (`interestId`) of the scheduler (Observed: slot 2, never carded).
pub const MUTATION_SCHEDULER_INTEREST: u64 = 2;

/// Children of the state's only slot.
const SCHEDULER_CHILDREN: u8 = 2;
/// Slots of the state.
const SCHEDULER_SLOTS: u8 = 1;
/// The slot byte with slot 0 set.
const SLOT_0: u8 = 0x01;
/// The slot byte with no slot set.
const NO_SLOT: u8 = 0;
/// Child `gameModeMutationSets`.
const SETS_CHILD: u8 = 0;
/// Child `mutationCadenceStartTime`.
const CADENCE_CHILD: u8 = 1;
/// The field name of the errors.
const FIELD: &str = "GameModeMutationSchedulerReplicatedState";

/// One `GameModeMutationSet` with its game mode: CRC-32s of the lowercase ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationSetEntry {
    /// The game mode (`GameModeID`).
    pub game_mode: u32,
    /// The elemental category (`ElementalMutationStaticData.CategoryWildcard`).
    pub elemental: u32,
    /// The promotion (`PromotionMutationID`).
    pub promotion: u32,
    /// The curse (`CurseMutationID`).
    pub curse: u32,
}

impl MutationSetEntry {
    fn write(&self, writer: &mut Writer) {
        writer
            .u32(self.game_mode)
            .u32(self.elemental)
            .u32(self.promotion)
            .u32(self.curse);
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            game_mode: reader.u32()?,
            elemental: reader.u32()?,
            promotion: reader.u32()?,
            curse: reader.u32()?,
        })
    }
}

/// The scheduler state: `None` children are not in the body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationSchedulerState {
    /// `gameModeMutationSets`.
    pub sets: Option<FullList<MutationSetEntry>>,
    /// `mutationCadenceStartTime`, Unix nanoseconds.
    pub cadence_start_ns: Option<u64>,
}

impl MutationSchedulerState {
    /// Writes the body: the slot byte, then the children.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the list does not fit its prefix code.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let mask = u32::from(self.sets.is_some()) << SETS_CHILD
            | u32::from(self.cadence_start_ns.is_some()) << CADENCE_CHILD;
        if mask == 0 {
            writer.u8(NO_SLOT);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_wide_children(writer, mask, SCHEDULER_CHILDREN, |writer, child| {
            match child {
                SETS_CHILD => {
                    if let Some(sets) = &self.sets {
                        sets.write(writer, |writer, entry| {
                            entry.write(writer);
                            Ok(())
                        })?;
                    }
                }
                _ => {
                    writer.u64(self.cadence_start_ns.unwrap_or_default());
                }
            }
            Ok(())
        })
    }

    /// Reads a body.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        if read_slot_byte(reader, FIELD, SCHEDULER_SLOTS)? == NO_SLOT {
            return Ok(state);
        }
        read_wide_children(reader, FIELD, SCHEDULER_CHILDREN, |reader, child| {
            match child {
                SETS_CHILD => state.sets = Some(FullList::read(reader, MutationSetEntry::read)?),
                _ => state.cadence_start_ns = Some(reader.u64()?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the list does not fit its prefix code.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.write(&mut writer)?;
        Ok(writer.finish())
    }

    /// The state of the body `body`, which it must use up.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let state = Self::read(&mut reader)?;
        reader.finish()?;
        Ok(state)
    }

    /// The bundle member with this state (component [`MUTATION_SCHEDULER_COMPONENT`]).
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the state cannot be encoded.
    pub fn member(&self) -> Result<BundleMember, MessageError> {
        Ok(BundleMember {
            component: MUTATION_SCHEDULER_COMPONENT,
            state: MemberState::Recorded {
                class: MUTATION_SCHEDULER_TYPE_INDEX,
                body: self.encode()?,
            },
        })
    }

    /// The bundle group of the scheduler entity (slot [`MUTATION_SCHEDULER_INTEREST`]) with this
    /// state as its only member.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the state cannot be encoded.
    pub fn group(&self) -> Result<BundleGroup, MessageError> {
        Ok(BundleGroup {
            interest: MUTATION_SCHEDULER_INTEREST,
            members: vec![self.member()?],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The retail body of the first slot-2 group (newchar 2026-09-18): the week of 2026-09-14
    /// 07:00 UTC, cutlass keys, brimstone sands and shattered mountain, all ice, barbaric and
    /// fiendish.
    const RETAIL_BODY: &str = "01030001cf3e0403\
        234825a2cb4f840ecb5e7b4da78a7b02\
        b4e9c55dcb4f840ecb5e7b4da78a7b02\
        7f1c8a63cb4f840ecb5e7b4da78a7b02\
        18e02ab301226000";
    /// CRC-32 of `dungeoncutlasskeys00`.
    const CUTLASS_KEYS: u32 = 0x2348_25a2;
    /// CRC-32 of `ice`, `barbaric` and `fiendish`.
    const ICE: u32 = 0xcb4f_840e;
    const BARBARIC: u32 = 0xcb5e_7b4d;
    const FIENDISH: u32 = 0xa78a_7b02;
    /// 2026-10-20 07:00:00 UTC.
    const CADENCE_NS: u64 = 1_792_479_600_000_000_000;

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn the_retail_week_reads_and_writes_back_the_same_bytes() {
        let body = bytes(RETAIL_BODY);
        let state = MutationSchedulerState::decode(&body).expect("decode");
        let sets = state.sets.as_ref().expect("sets");
        assert_eq!(sets.elements.len(), 3);
        assert_eq!(
            sets.elements[0],
            MutationSetEntry {
                game_mode: CUTLASS_KEYS,
                elemental: ICE,
                promotion: BARBARIC,
                curse: FIENDISH,
            }
        );
        assert_eq!(state.cadence_start_ns, Some(CADENCE_NS));
        assert_eq!(state.encode().expect("encode"), body);
    }

    #[test]
    fn the_member_is_component_six_of_slot_two() {
        let state = MutationSchedulerState {
            sets: Some(FullList::first(Vec::new())),
            cadence_start_ns: Some(CADENCE_NS),
        };
        let group = state.group().expect("group");
        assert_eq!(group.interest, MUTATION_SCHEDULER_INTEREST);
        assert_eq!(group.members.len(), 1);
        assert_eq!(group.members[0].component, MUTATION_SCHEDULER_COMPONENT);
        assert_eq!(
            group.members[0].state.type_index(),
            MUTATION_SCHEDULER_TYPE_INDEX
        );
    }

    #[test]
    fn an_empty_state_is_the_bare_slot_byte() {
        let state = MutationSchedulerState::default();
        assert_eq!(state.encode().expect("encode"), [NO_SLOT]);
        assert_eq!(MutationSchedulerState::decode(&[NO_SLOT]), Ok(state));
    }
}
