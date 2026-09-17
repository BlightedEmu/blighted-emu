// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::GatheringComponentReplicatedState` (type 81): the player's gather, which runs the progress
//! ring of the client's interact prompt (Azoth catalog field note for the names).
//!
//! The state has slot 1 only, with nine children. Their widths are fitted on every retail body of
//! the tutorial chest (own component 36, 237308-239343 of the tutorial capture) and of two
//! open-world gathering captures: 114 different bodies, each ending exactly where the next member
//! or group starts (Observed). Child 5 (`m_waterGatherRefCount`) is in no captured body, so its
//! width is Unknown and a body that sends it is refused.

use crate::slots::{children_mask, read_children, read_slot_byte, write_known_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::GatheringComponentReplicatedState`.
pub const GATHERING_TYPE_INDEX: u64 = 81;
/// The gatherable id of no gatherable (retail: `00000000ffffffff` while a gather starts).
pub const NO_GATHERABLE: u64 = 0x0000_0000_ffff_ffff;

const FIELD: &str = "gathering";
const SLOTS: u8 = 2;
const SLOT_1: u8 = 0b10;
const CHILDREN: u8 = 9;
const STATUS: u8 = 0;
const AMOUNT_REMAINING: u8 = 1;
const EFFICIENCY: u8 = 2;
const GATHERER_COUNT: u8 = 3;
const GATHER_REF_COUNT: u8 = 4;
const IS_GATHERING: u8 = 6;
const GATHERABLE: u8 = 7;
const SYNC_REF_COUNT: u8 = 8;

/// The children of slot 1 that are sent; every child is optional.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GatheringState {
    /// `m_status` (retail: 1 while gathering, 3 when done).
    pub status: Option<u8>,
    /// `m_amtRemaining` (retail chest 2.5; open-world nodes 15, 25, 35).
    pub amount_remaining: Option<f32>,
    /// `m_gatherEfficiency` (retail chest 1.0).
    pub efficiency: Option<f32>,
    /// `m_numGathering` (retail chest 0, open-world nodes 1).
    pub gatherer_count: Option<u8>,
    /// `m_gatherRefCount`: a counter that grows over the session.
    pub gather_ref_count: Option<u32>,
    /// `m_isGathering`.
    pub is_gathering: Option<bool>,
    /// `m_gatherableGDEID`: the facet entity of the gatherable, or [`NO_GATHERABLE`].
    pub gatherable: Option<u64>,
    /// `m_syncGatherRefCount`: a counter, one up with each state that sends it.
    pub sync_ref_count: Option<u32>,
}

impl GatheringState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets slot 0 or child 5 (layouts
    /// Unknown), or a flag is not 0 or 1.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, FIELD, SLOTS)?;
        let mut state = Self::default();
        if slots & !SLOT_1 != 0 {
            return Err(MessageError::UnknownBits {
                field: FIELD,
                bits: slots & !SLOT_1,
            });
        }
        if slots == 0 {
            return Ok(state);
        }
        read_children(reader, FIELD, CHILDREN, |reader, child| {
            match child {
                STATUS => state.status = Some(reader.u8()?),
                AMOUNT_REMAINING => state.amount_remaining = Some(reader.f32()?),
                EFFICIENCY => state.efficiency = Some(reader.f32()?),
                GATHERER_COUNT => state.gatherer_count = Some(reader.u8()?),
                GATHER_REF_COUNT => state.gather_ref_count = Some(reader.u32()?),
                IS_GATHERING => state.is_gathering = Some(reader.boolean()?),
                GATHERABLE => state.gatherable = Some(reader.u64()?),
                SYNC_REF_COUNT => state.sync_ref_count = Some(reader.u32()?),
                _ => {
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

    /// Writes the state for the bundle path; without a child, the slot byte 0 alone.
    pub fn write(&self, writer: &mut Writer) {
        let present = [
            self.status.is_some(),
            self.amount_remaining.is_some(),
            self.efficiency.is_some(),
            self.gatherer_count.is_some(),
            self.gather_ref_count.is_some(),
            false,
            self.is_gathering.is_some(),
            self.gatherable.is_some(),
            self.sync_ref_count.is_some(),
        ];
        let children = children_mask(&present);
        if children == 0 {
            writer.u8(0);
            return;
        }
        writer.u8(SLOT_1);
        write_known_children(writer, children, CHILDREN, |writer, child| {
            match child {
                STATUS => self.status.map(|value| writer.u8(value)),
                AMOUNT_REMAINING => self.amount_remaining.map(|value| writer.f32(value)),
                EFFICIENCY => self.efficiency.map(|value| writer.f32(value)),
                GATHERER_COUNT => self.gatherer_count.map(|value| writer.u8(value)),
                GATHER_REF_COUNT => self.gather_ref_count.map(|value| writer.u32(value)),
                IS_GATHERING => self.is_gathering.map(|value| writer.boolean(value)),
                GATHERABLE => self.gatherable.map(|value| writer.u64(value)),
                _ => self.sync_ref_count.map(|value| writer.u32(value)),
            };
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The retail chest facet entity (tutorial capture).
    const CHEST: u64 = 0x5870_3581_9fa0_a5ef;

    fn encoded(state: &GatheringState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer);
        writer.finish()
    }

    fn round_trip(bytes: &[u8]) -> GatheringState {
        let mut reader = Reader::new(bytes);
        let state = GatheringState::read(&mut reader).expect("read");
        assert!(reader.is_empty(), "{bytes:02x?}");
        assert_eq!(encoded(&state), bytes, "written back the same");
        state
    }

    #[test]
    fn the_retail_chest_bodies_read_and_write_back() {
        // 237308: the gather starts.
        let started = round_trip(&[
            0x02, 0xc0, 0x01, 0x03, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0x01,
        ]);
        assert_eq!(
            started,
            GatheringState {
                is_gathering: Some(true),
                gatherable: Some(NO_GATHERABLE),
                sync_ref_count: Some(1),
                ..GatheringState::default()
            }
        );
        // 237342: the whole running gather.
        let running = round_trip(&[
            0x02, 0xdf, 0x01, 0x40, 0x20, 0, 0, 0x3f, 0x80, 0, 0, 0, 0, 0, 0, 0x01, 0x01, 0x03, 0,
            0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0x01,
        ]);
        assert_eq!(
            (running.status, running.amount_remaining, running.efficiency),
            (Some(1), Some(2.5), Some(1.0))
        );
        assert_eq!(
            (running.gatherer_count, running.gather_ref_count),
            (Some(0), Some(1))
        );
        // 239343: the end, naming the chest.
        let ended = round_trip(&[
            0x02, 0xd7, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02, 0x00, 0x03, 0x58, 0x70, 0x35,
            0x81, 0x9f, 0xa0, 0xa5, 0xef, 0, 0, 0, 0x02,
        ]);
        assert_eq!(
            ended,
            GatheringState {
                status: Some(3),
                amount_remaining: Some(0.0),
                efficiency: Some(0.0),
                gather_ref_count: Some(2),
                is_gathering: Some(false),
                gatherable: Some(CHEST),
                sync_ref_count: Some(2),
                gatherer_count: None,
            }
        );
    }

    #[test]
    fn an_empty_state_is_the_slot_byte_alone() {
        assert_eq!(encoded(&GatheringState::default()), [0x00]);
        assert_eq!(round_trip(&[0x00]), GatheringState::default());
    }

    #[test]
    fn child_5_and_slot_0_are_refused() {
        // Child 5 (`m_waterGatherRefCount`) has no captured width.
        let water = [0x02, 0x20, 0x00];
        assert!(GatheringState::read(&mut Reader::new(&water)).is_err());
        assert!(GatheringState::read(&mut Reader::new(&[0x01, 0x01, 0x00])).is_err());
    }
}
