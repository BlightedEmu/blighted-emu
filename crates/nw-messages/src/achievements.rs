// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::AchievementComponentReplicatedState` (type 3210, player component 66): the player's
//! achievements as one bitset, bit `AchievementIndex` (tutorial-intro note, sections 1 and 2).
//!
//! Slot 0 child 0 is a byte vector in the shared list grammar. The first state is the full form
//! (`00`, version, length, the bytes). A later change is the delta form: a count, then groups of
//! up to 8 entries, each group a mask byte (bit `i` set: entry `i` carries a value) and each entry
//! the byte index, a version and the new byte.

use crate::items::FullList;
use crate::slots::{SLOT_0, read_slot_0, write_known_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::AchievementComponentReplicatedState`.
pub const ACHIEVEMENT_TYPE_INDEX: u64 = 3210;
/// The player component of the achievements.
pub const ACHIEVEMENT_COMPONENT: u64 = 66;

/// Children of slot 0.
const ACHIEVEMENT_CHILDREN: u8 = 1;
/// Entries per delta group.
const DELTA_GROUP: usize = 8;
/// The version flag byte: a version follows.
const VERSION_PRESENT: u8 = 1;
const FIELD: &str = "achievements";

/// One changed byte of the bitset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteChange {
    /// The byte index: achievement index / 8.
    pub byte: u32,
    /// The version; it rises with each change.
    pub version: u64,
    /// The new byte.
    pub value: u8,
}

/// The bitset child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AchievementBits {
    /// The whole bitset.
    Full(FullList<u8>),
    /// Changed bytes.
    Changes(Vec<ByteChange>),
}

/// The achievement state; `None` sends no slot (no change).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AchievementState {
    /// The bitset child.
    pub bits: Option<AchievementBits>,
}

impl AchievementState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another slot or child, or a delta
    /// entry removes a byte.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut bits = None;
        read_slot_0(reader, FIELD, ACHIEVEMENT_CHILDREN, |reader, _| {
            let rest = reader.rest();
            let mut peek = Reader::new(rest);
            bits = Some(if peek.wide_prefixed()? == 0 {
                AchievementBits::Full(FullList::read(reader, |reader| reader.u8())?)
            } else {
                AchievementBits::Changes(read_changes(reader)?)
            });
            Ok(())
        })?;
        Ok(Self { bits })
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] from the list writer.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let Some(bits) = &self.bits else {
            writer.u8(0);
            return Ok(());
        };
        writer.u8(SLOT_0);
        let mut result = Ok(());
        write_known_children(writer, 1, ACHIEVEMENT_CHILDREN, |writer, _| {
            result = match bits {
                AchievementBits::Full(list) => list.write(writer, |writer, byte| {
                    writer.u8(*byte);
                    Ok(())
                }),
                AchievementBits::Changes(changes) => {
                    write_changes(writer, changes);
                    Ok(())
                }
            };
        });
        result
    }
}

fn read_changes(reader: &mut Reader<'_>) -> Result<Vec<ByteChange>, MessageError> {
    let count = reader.wide_prefixed()?;
    // Each entry takes at least four bytes.
    if count > reader.remaining() as u64 {
        return Err(MessageError::LengthMismatch { field: FIELD });
    }
    // At most the bytes left.
    let mut left = count as usize;
    let mut changes = Vec::with_capacity(left);
    while left > 0 {
        let mask = reader.u8()?;
        let group = left.min(DELTA_GROUP);
        for entry in 0..group {
            let byte = reader.wide_prefixed()?;
            if reader.u8()? != VERSION_PRESENT || mask & 1 << entry == 0 {
                return Err(MessageError::Unsized {
                    field: FIELD,
                    byte: mask,
                });
            }
            changes.push(ByteChange {
                byte: u32::try_from(byte).map_err(|_| MessageError::CountTooLarge {
                    count: byte,
                    limit: u64::from(u32::MAX),
                })?,
                version: reader.wide_prefixed()?,
                value: reader.u8()?,
            });
        }
        left -= group;
    }
    Ok(changes)
}

fn write_changes(writer: &mut Writer, changes: &[ByteChange]) {
    // A `usize` count fits 64 bits.
    writer.wide_prefixed(changes.len() as u64);
    for group in changes.chunks(DELTA_GROUP) {
        // Every entry of a group carries a value; a group has at most 8 entries.
        writer.u8(u8::MAX >> (DELTA_GROUP - group.len()));
        for change in group {
            writer
                .wide_prefixed(u64::from(change.byte))
                .u8(VERSION_PRESENT)
                .wide_prefixed(change.version)
                .u8(change.value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail deltas (tutorial-intro note, section 2): `FTUE_PCCM_Cinematic_Unlock` at byte 1899
    /// (version 146, value `40`), and `FTUE_Intro2_Cinematic_Watched` at byte 1850.
    const RETAIL_PCCM_UNLOCK: [u8; 9] = [0x01, 0x01, 0x01, 0x01, 0xab, 0x1d, 0x01, 0x92, 0x02];
    const RETAIL_PCCM_VALUE: u8 = 0x40;
    const RETAIL_INTRO_WATCHED: [u8; 9] = [0x01, 0x01, 0x01, 0x01, 0xba, 0x1c, 0x01, 0xbb, 0x51];

    fn encode(state: &AchievementState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    #[test]
    fn a_change_is_the_retail_delta() {
        let state = AchievementState {
            bits: Some(AchievementBits::Changes(vec![ByteChange {
                byte: 1899,
                version: 146,
                value: RETAIL_PCCM_VALUE,
            }])),
        };
        let bytes = encode(&state);
        assert_eq!(bytes[..RETAIL_PCCM_UNLOCK.len()], RETAIL_PCCM_UNLOCK);
        assert_eq!(bytes[RETAIL_PCCM_UNLOCK.len()..], [RETAIL_PCCM_VALUE]);
        assert_eq!(AchievementState::read(&mut Reader::new(&bytes)), Ok(state));
        let watched = [RETAIL_INTRO_WATCHED.as_slice(), &[0x30]].concat();
        let read = AchievementState::read(&mut Reader::new(&watched)).expect("read");
        assert_eq!(
            read.bits,
            Some(AchievementBits::Changes(vec![ByteChange {
                byte: 1850,
                version: 5243,
                value: 0x30,
            }]))
        );
    }

    #[test]
    fn the_full_bitset_has_the_retail_head_and_round_trips() {
        const BYTES: usize = 1910;
        let mut bitset = vec![0; BYTES];
        bitset[1850] = 0x10;
        let state = AchievementState {
            bits: Some(AchievementBits::Full(FullList::first(bitset))),
        };
        let bytes = encode(&state);
        assert_eq!(bytes[..7], [0x01, 0x01, 0x00, 0x01, 0x03, 0xb6, 0x1d]);
        assert_eq!(bytes.len(), 7 + BYTES);
        assert_eq!(AchievementState::read(&mut Reader::new(&bytes)), Ok(state));
        assert_eq!(encode(&AchievementState::default()), [0]);
    }

    #[test]
    fn the_retail_door_change_grows_the_bitset() {
        // Retail 175193 (tutorial capture): bytes 1911 to 1914, the version on the first entry
        // only, bit 15317 (the ruins door) in byte 1914.
        const RETAIL_DOOR: [u8; 25] = [
            0x01, 0x01, 0x04, 0x0f, 0xb7, 0x1d, 0x01, 0x8b, 0x49, 0x00, 0xb8, 0x1d, 0x01, 0x00,
            0x00, 0xb9, 0x1d, 0x01, 0x00, 0x00, 0xba, 0x1d, 0x01, 0x00, 0x20,
        ];
        const RETAIL_VERSION: u64 = 4683;
        let change = |byte, version, value| ByteChange {
            byte,
            version,
            value,
        };
        let state = AchievementState {
            bits: Some(AchievementBits::Changes(vec![
                change(1911, RETAIL_VERSION, 0),
                change(1912, 0, 0),
                change(1913, 0, 0),
                change(1914, 0, 0x20),
            ])),
        };
        assert_eq!(encode(&state), RETAIL_DOOR);
        assert_eq!(
            AchievementState::read(&mut Reader::new(&RETAIL_DOOR)),
            Ok(state)
        );
    }

    #[test]
    fn many_changes_take_groups_of_eight() {
        let changes: Vec<ByteChange> = (0..9)
            .map(|byte| ByteChange {
                byte,
                version: 1,
                value: 1,
            })
            .collect();
        let state = AchievementState {
            bits: Some(AchievementBits::Changes(changes)),
        };
        let bytes = encode(&state);
        assert_eq!(bytes[2..4], [9, 0xff]);
        assert_eq!(AchievementState::read(&mut Reader::new(&bytes)), Ok(state));
    }
}
