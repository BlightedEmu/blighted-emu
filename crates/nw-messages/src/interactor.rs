// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::InteractorComponentReplicatedState` (type 3752): the entity the player interacts with,
//! and whether it may interact (Azoth catalog field note for the names).
//!
//! Slot 0 child 0 is `m_cachedCommittedInteractGDEID`, a big-endian `u64` facet entity; slot 1
//! child 0 is `m_enabled`, a flag. Every captured body has these forms: `01 01 <u64>`,
//! `02 01 <flag>` and `03 01 <u64> 01 <flag>` (1358 bodies of the tutorial and an open-world
//! gathering capture, each ending where the next member starts; Observed). Other children are in
//! no body, so they are refused.

use crate::slots::{read_children, read_slot_byte, write_known_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::InteractorComponentReplicatedState`.
pub const INTERACTOR_TYPE_INDEX: u64 = 3752;

const FIELD: &str = "interactor";
const SLOTS: u8 = 2;
const SLOT_0: u8 = 0b01;
const SLOT_1: u8 = 0b10;
/// Each slot has one known child.
const CHILDREN: u8 = 1;
const ONLY_CHILD: u16 = 1;

/// The sent children of both slots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InteractorState {
    /// `m_cachedCommittedInteractGDEID`: the facet entity of the interaction's target.
    pub target: Option<u64>,
    /// `m_enabled`.
    pub enabled: Option<bool>,
}

impl InteractorState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a flag is not 0 or 1, or a slot sets a
    /// child other than child 0.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, FIELD, SLOTS)?;
        let mut state = Self::default();
        if slots & SLOT_0 != 0 {
            read_children(reader, FIELD, CHILDREN, |reader, _| {
                state.target = Some(reader.u64()?);
                Ok(())
            })?;
        }
        if slots & SLOT_1 != 0 {
            read_children(reader, FIELD, CHILDREN, |reader, _| {
                state.enabled = Some(reader.boolean()?);
                Ok(())
            })?;
        }
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let slots = if self.target.is_some() { SLOT_0 } else { 0 }
            | if self.enabled.is_some() { SLOT_1 } else { 0 };
        writer.u8(slots);
        if let Some(target) = self.target {
            write_known_children(writer, ONLY_CHILD, CHILDREN, |writer, _| {
                writer.u64(target);
            });
        }
        if let Some(enabled) = self.enabled {
            write_known_children(writer, ONLY_CHILD, CHILDREN, |writer, _| {
                writer.boolean(enabled);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(bytes: &[u8]) -> InteractorState {
        let mut reader = Reader::new(bytes);
        let state = InteractorState::read(&mut reader).expect("read");
        assert!(reader.is_empty(), "{bytes:02x?}");
        let mut writer = Writer::new();
        state.write(&mut writer);
        assert_eq!(writer.finish(), bytes, "written back the same");
        state
    }

    #[test]
    fn the_retail_forms_read_and_write_back() {
        // The chest at its accept (tutorial 237308).
        assert_eq!(
            round_trip(&[0x01, 0x01, 0x58, 0x70, 0x35, 0x81, 0x9f, 0xa0, 0xa5, 0xef]),
            InteractorState {
                target: Some(0x5870_3581_9fa0_a5ef),
                enabled: None,
            }
        );
        // Interaction off (tutorial 77331).
        assert_eq!(
            round_trip(&[0x02, 0x01, 0x00]),
            InteractorState {
                target: None,
                enabled: Some(false),
            }
        );
        // Both (tutorial 122450).
        assert_eq!(
            round_trip(&[0x03, 0x01, 0, 0, 0, 0, 0x12, 0x99, 0x61, 0x19, 0x01, 0x01]),
            InteractorState {
                target: Some(0x1299_6119),
                enabled: Some(true),
            }
        );
    }

    #[test]
    fn an_unknown_child_is_refused() {
        assert!(InteractorState::read(&mut Reader::new(&[0x01, 0x02, 0x00])).is_err());
    }
}
