// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::VitalsComponentReplicatedState` (type 15), slot 0, on the bundle path (vitals note,
//! second pass). The eight children of slot 0, in order: `vitalsData`, `healthChangeFlags`,
//! `vitalsId`, `vitalsCategoryId`, `vitalsLevel`, `invulnerability`,
//! `displayImmuneWhenInvulnerable`, `maxHealth`. A body that uses another slot is refused: those
//! slots are not sized.
//!
//! `vitalsData` and `healthChangeFlags` are not sized either. The note gives them 18 bytes
//! (`FUN_1469515b0`: a byte, two `u64`, a byte) and 1 byte, but small captured bundles hold each
//! in 4 bytes, whose first byte is in the `f32` range. What selects the form is Unknown, so a body
//! with child 0 or 1 is [`MessageError::Unsized`] and the server sends neither.

use crate::slots::{CHILD_BITS, read_slot_0, write_slot_0};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of the class.
pub const VITALS_TYPE_INDEX: u64 = 15;
const CHILDREN: u8 = 8;
const FIELD: &str = "vitals";
/// `vitalsData` and `healthChangeFlags`: children 0 and 1.
const UNSIZED_CHILDREN: u16 = 0b11;
const CHILD_ID: u16 = 1 << 2;
const CHILD_CATEGORY: u16 = 1 << 3;
const CHILD_LEVEL: u16 = 1 << 4;
const CHILD_INVULNERABILITY: u16 = 1 << 5;
const CHILD_DISPLAY_IMMUNE: u16 = 1 << 6;
const CHILD_MAX_HEALTH: u16 = 1 << 7;

/// Slot 0 of the vitals state without `vitalsData` and `healthChangeFlags`; every child is
/// optional.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VitalsSlot0 {
    /// `vitalsId`.
    pub vitals_id: Option<u32>,
    /// `vitalsCategoryId`.
    pub vitals_category_id: Option<u32>,
    /// `vitalsLevel`.
    pub vitals_level: Option<u32>,
    /// `invulnerability`.
    pub invulnerability: Option<u8>,
    /// `displayImmuneWhenInvulnerable`.
    pub display_immune: Option<u8>,
    /// `maxHealth`.
    pub max_health: Option<u16>,
}

impl VitalsSlot0 {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, uses a slot other than 0 or an
    /// unsized child, or has a form `write` cannot make.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let Some(children) = read_slot_0(reader, FIELD, CHILDREN)? else {
            return Ok(Self::default());
        };
        let has = |child| children & child != 0;
        if has(UNSIZED_CHILDREN) {
            return Err(MessageError::Unsized {
                field: FIELD,
                // The child bits of the first child byte.
                byte: children.to_le_bytes()[0] & CHILD_BITS,
            });
        }
        Ok(Self {
            vitals_id: if has(CHILD_ID) {
                Some(reader.u32()?)
            } else {
                None
            },
            vitals_category_id: if has(CHILD_CATEGORY) {
                Some(reader.u32()?)
            } else {
                None
            },
            vitals_level: if has(CHILD_LEVEL) {
                Some(reader.u32()?)
            } else {
                None
            },
            invulnerability: if has(CHILD_INVULNERABILITY) {
                Some(reader.u8()?)
            } else {
                None
            },
            display_immune: if has(CHILD_DISPLAY_IMMUNE) {
                Some(reader.u8()?)
            } else {
                None
            },
            max_health: if has(CHILD_MAX_HEALTH) {
                Some(reader.u16()?)
            } else {
                None
            },
        })
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let flags = [
            (self.vitals_id.is_some(), CHILD_ID),
            (self.vitals_category_id.is_some(), CHILD_CATEGORY),
            (self.vitals_level.is_some(), CHILD_LEVEL),
            (self.invulnerability.is_some(), CHILD_INVULNERABILITY),
            (self.display_immune.is_some(), CHILD_DISPLAY_IMMUNE),
            (self.max_health.is_some(), CHILD_MAX_HEALTH),
        ];
        let children = flags
            .iter()
            .filter(|(present, _)| *present)
            .fold(0, |bits, (_, child)| bits | child);
        write_slot_0(writer, Some(children), CHILDREN);
        for value in [self.vitals_id, self.vitals_category_id, self.vitals_level]
            .into_iter()
            .flatten()
        {
            writer.u32(value);
        }
        for byte in [self.invulnerability, self.display_immune]
            .into_iter()
            .flatten()
        {
            writer.u8(byte);
        }
        if let Some(max_health) = self.max_health {
            writer.u16(max_health);
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn round(state: &VitalsSlot0) -> VitalsSlot0 {
        let mut writer = Writer::new();
        state.write(&mut writer);
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        let read = VitalsSlot0::read(&mut reader).expect("read");
        reader.finish().expect("to the end");
        read
    }

    proptest! {
           #[test]
           fn vitals_slot_0_round_trips(
               vitals_id in proptest::option::of(any::<u32>()),
               vitals_category_id in proptest::option::of(any::<u32>()),
               vitals_level in proptest::option::of(any::<u32>()),
               invulnerability in proptest::option::of(any::<u8>()),
               display_immune in proptest::option::of(any::<u8>()),
               max_health in proptest::option::of(any::<u16>()),
           ) {
               let state = VitalsSlot0 {
    vitals_id, vitals_category_id, vitals_level,
                   invulnerability, display_immune, max_health,
               };
               prop_assert_eq!(round(&state), state);
           }
       }

    #[test]
    fn a_vitals_body_on_another_slot_is_refused() {
        let mut reader = Reader::new(&[0b10, 0x01, 0]);
        assert!(VitalsSlot0::read(&mut reader).is_err());
    }

    #[test]
    fn the_vitals_data_child_is_not_sized() {
        // Child 0 then four bytes, as the short form in the captures.
        let mut reader = Reader::new(&[0x01, 0x01, 0x44, 0, 0, 0]);
        assert!(matches!(
            VitalsSlot0::read(&mut reader),
            Err(MessageError::Unsized { .. })
        ));
    }

    #[test]
    fn the_health_change_flags_child_is_not_sized() {
        // Child 1 then four bytes, as the short form in the captures.
        let mut reader = Reader::new(&[0x01, 0x02, 0x3e, 0, 0, 0]);
        assert!(matches!(
            VitalsSlot0::read(&mut reader),
            Err(MessageError::Unsized { .. })
        ));
    }

    #[test]
    fn only_max_health_is_two_child_bytes_and_a_u16() {
        let state = VitalsSlot0 {
            max_health: Some(500),
            ..VitalsSlot0::default()
        };
        let mut writer = Writer::new();
        state.write(&mut writer);
        assert_eq!(writer.finish(), vec![0x01, 0x80, 0x01, 0x01, 0xf4]);
    }
}
