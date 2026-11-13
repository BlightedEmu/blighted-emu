// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::DoorComponentReplicatedState` (type 2330): the state of an entity's `DoorComponent`.
//!
//! Observed (client build 25810808): the class has slot 0 with one child, `doorState`, one byte
//! (the member table). The byte is the client enum `DoorState`, registered by `FUN_1430020e0`:
//! 0 `Open` (the string at `DAT_1480e97f0`), 1 `Closed`, 2 `Count` (the end of the enum, no
//! state). The same function registers the component's edit fields `m_collisionEntity` and
//! `m_disableCollisionBlendPctThreshold`; the client facet is `DoorComponentClientFacet`
//! (`FUN_142fb4e40`).
//!
//! Observed (retail captures, door study 2026-11-12): 8,848 bodies in 148 ledgers, every one
//! `01 01 01` (slot 0, child 0, `Closed`), in the spawn group of its entity. No ledger holds a
//! change of the state.

use crate::slots::{read_slot_0, write_known};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::DoorComponentReplicatedState`.
pub const DOOR_TYPE_INDEX: u64 = 2330;

/// Children of slot 0: `doorState`.
const DOOR_CHILDREN: u8 = 1;
/// The child bitmap with `doorState` set.
const DOOR_STATE_CHILD: u16 = 1;
/// The field name in read errors.
const DOOR_FIELD: &str = "door state";

/// The client enum `DoorState` (`FUN_1430020e0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoorState {
    /// `Open` (0).
    Open,
    /// `Closed` (1): the state of every door of the retail captures.
    Closed,
}

impl DoorState {
    /// The wire byte of the state.
    #[must_use]
    pub const fn byte(self) -> u8 {
        match self {
            Self::Open => 0,
            Self::Closed => 1,
        }
    }

    /// The state of wire byte `byte`; `None` for `Count` (2) and above, which name no state.
    #[must_use]
    pub const fn of_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Open),
            1 => Some(Self::Closed),
            _ => None,
        }
    }
}

/// `MB::DoorComponentReplicatedState`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DoorComponentState {
    /// `doorState`.
    pub door_state: Option<DoorState>,
}

impl DoorComponentState {
    /// The state with `doorState` set to `state`.
    #[must_use]
    pub const fn of(state: DoorState) -> Self {
        Self {
            door_state: Some(state),
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, has a slot or child the class has not,
    /// or holds a byte that names no `DoorState`.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, DOOR_FIELD, DOOR_CHILDREN, |reader, _| {
            let byte = reader.u8()?;
            state.door_state = Some(DoorState::of_byte(byte).ok_or(MessageError::BadTag {
                field: DOOR_FIELD,
                tag: byte,
            })?);
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let children = if self.door_state.is_some() {
            DOOR_STATE_CHILD
        } else {
            0
        };
        write_known(writer, children, DOOR_CHILDREN, |writer, _| {
            if let Some(state) = self.door_state {
                writer.u8(state.byte());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::{BundleMember, MemberState};

    /// The body of every retail door (`opr_que_and_lost` 578.667 s, an `or_fort_barriers`).
    const RETAIL_CLOSED: [u8; 3] = [0x01, 0x01, 0x01];
    /// The component number of the retail barrier's door.
    const BARRIER_DOOR_COMPONENT: u64 = 4;

    #[test]
    fn the_retail_body_reads_closed_and_writes_back_the_same_bytes() {
        let state = MemberState::decode(DOOR_TYPE_INDEX, &RETAIL_CLOSED).expect("decode");
        assert_eq!(
            state,
            MemberState::Door(DoorComponentState::of(DoorState::Closed))
        );
        let member = BundleMember {
            component: BARRIER_DOOR_COMPONENT,
            state,
        };
        let encoded = member.encode().expect("encode");
        assert!(encoded.ends_with(&RETAIL_CLOSED));
        assert_eq!(member.state.type_index(), DOOR_TYPE_INDEX);
    }

    #[test]
    fn the_enum_bytes_are_the_client_order() {
        assert_eq!(DoorState::Open.byte(), 0);
        assert_eq!(DoorState::Closed.byte(), 1);
        assert_eq!(DoorState::of_byte(0), Some(DoorState::Open));
        assert_eq!(DoorState::of_byte(1), Some(DoorState::Closed));
        // `Count` names no state.
        assert_eq!(DoorState::of_byte(2), None);
    }

    #[test]
    fn an_open_door_and_an_empty_slot_round_trip() {
        let mut writer = Writer::new();
        DoorComponentState::of(DoorState::Open).write(&mut writer);
        let open = writer.finish();
        assert_eq!(open, [0x01, 0x01, 0x00]);
        assert_eq!(
            DoorComponentState::read(&mut Reader::new(&open)).expect("read"),
            DoorComponentState::of(DoorState::Open)
        );
        let mut writer = Writer::new();
        DoorComponentState::default().write(&mut writer);
        let empty = writer.finish();
        assert_eq!(empty, [0x00]);
        assert_eq!(
            DoorComponentState::read(&mut Reader::new(&empty)).expect("read"),
            DoorComponentState::default()
        );
    }

    #[test]
    fn a_byte_past_the_enum_is_refused() {
        assert!(matches!(
            DoorComponentState::read(&mut Reader::new(&[0x01, 0x01, 0x02])),
            Err(MessageError::BadTag { tag: 2, .. })
        ));
    }
}
