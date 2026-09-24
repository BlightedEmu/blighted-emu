// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::TemporaryAffiliationReplicatedState` (type 3563, own player component 17): the temporary
//! sides of the player (conversation-notify note, section 4).
//!
//! Slot 0 has one child (constructor `FUN_14405d4d0`): `affiliations`, a replicated list of
//! [`Affiliation`] (reader `FUN_14406ce50`, element `FUN_145d39900`, full form
//! `FUN_144058d40`). Retail open-world cards carry an empty list or the pairs (0, 0), (1, 4),
//! (1, 0) and (0, 4); instanced content (expeditions, trials) carries large ids with kinds 1 to 4.

use crate::list_delta::ReplicatedList;
use crate::slots::{read_slot_0, write_known};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::TemporaryAffiliationReplicatedState`.
pub const TEMPORARY_AFFILIATION_TYPE_INDEX: u64 = 3563;
/// The own player component of the temporary affiliations.
pub const TEMPORARY_AFFILIATION_COMPONENT: u64 = 17;

/// Children of slot 0.
const CHILDREN: u8 = 1;
/// The child bit of `affiliations`.
const AFFILIATIONS: u16 = 1;
const FIELD: &str = "temporary affiliation";

/// One temporary affiliation (two big-endian `u32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Affiliation {
    /// The first value (Inferred: the affiliation id; consecutive ids in instanced content).
    pub id: u32,
    /// The second value (Inferred: its kind; retail 0 to 4).
    pub kind: u32,
}

impl Affiliation {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            id: reader.u32()?,
            kind: reader.u32()?,
        })
    }

    fn write(self, writer: &mut Writer) {
        writer.u32(self.id).u32(self.kind);
    }
}

/// The temporary affiliation state; `None` sends no change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemporaryAffiliationState {
    /// `affiliations`.
    pub affiliations: Option<ReplicatedList<Affiliation>>,
}

impl TemporaryAffiliationState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets a slot other than 0, or the list
    /// does not read (a delta entry that removes an element: 1 of 25,624 retail bodies).
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CHILDREN, |reader, _| {
            state.affiliations = Some(ReplicatedList::read(reader, FIELD, Affiliation::read)?);
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the list cannot be written.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let Some(list) = &self.affiliations else {
            write_known(writer, 0, CHILDREN, |_, _| {});
            return Ok(());
        };
        let mut result = Ok(());
        write_known(writer, AFFILIATIONS, CHILDREN, |writer, _| {
            result = list.write(writer, |writer, affiliation| {
                affiliation.write(writer);
                Ok(())
            });
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::FullList;

    fn round_trip(state: &TemporaryAffiliationState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            TemporaryAffiliationState::read(&mut reader).expect("read"),
            *state
        );
        reader.finish().expect("whole body");
        bytes
    }

    #[test]
    fn an_open_world_card_writes_the_retail_layout() {
        let state = TemporaryAffiliationState {
            affiliations: Some(ReplicatedList::Full(FullList {
                version: Some(8),
                elements: vec![
                    Affiliation { id: 0, kind: 0 },
                    Affiliation { id: 1, kind: 4 },
                ],
            })),
        };
        assert_eq!(
            round_trip(&state),
            [
                0x01, 0x01, 0x00, 0x01, 0x08, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 4
            ]
        );
        assert_eq!(round_trip(&TemporaryAffiliationState::default()), [0x00]);
    }

    #[test]
    fn a_removal_entry_is_refused() {
        // Count 1, mask 0 (the entry removes element 0).
        let body = [0x01, 0x01, 0x01, 0x00, 0x00, 0x01, 0x05];
        assert!(TemporaryAffiliationState::read(&mut Reader::new(&body)).is_err());
    }
}
