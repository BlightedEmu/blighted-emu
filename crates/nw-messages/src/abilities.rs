// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::AbilityComponentReplicatedState` (type 185, player component 46): the abilities a
//! character has mapped (progression note, section 6).
//!
//! Slot 0 holds five children (constructor `FUN_1466eba40`): `persistentAbilityData`,
//! `hitDataNumHits`, `hitDataAbilityIds`, `actionDataCount` and `actionDataAbilityIds`. Only
//! child 0 is read and written here; a body with another child is [`MessageError::Unsized`], so
//! a bundle walk stops there.
//!
//! Child 0 (writer `FUN_146856460`, reader `FUN_14669f600`): a count, then for each ability
//! table its id (`u32`, the CRC-32 of the lowercase `AbilityData` table id) and a count (at most
//! 3, `FUN_14669fb50`) of mapped slots, each an ability id (`u32`, CRC-32 of the lowercase
//! ability id; 0 for an empty slot) and a slot key (`u32`); then a map (`FUN_143025640`) of
//! weapon pool id to the pool's `VersionNumber` (`ProgressionPoolData`).

use crate::slots::{read_slot_0, write_known};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::AbilityComponentReplicatedState`.
pub const ABILITY_TYPE_INDEX: u64 = 185;
/// The player component of the abilities.
pub const ABILITY_COMPONENT: u64 = 46;

/// Children of slot 0.
const CHILDREN: u8 = 5;
const PERSISTENT_CHILD: u8 = 0;
/// The child bit of `persistentAbilityData`.
const PERSISTENT_BIT: u16 = 1 << PERSISTENT_CHILD;
/// Most mapped slots of one table (`FUN_14669fb50` refuses 4 or more).
pub const MAX_MAPPED_SLOTS: usize = 3;
const FIELD: &str = "abilities";

/// One mapped slot of an ability table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MappedSlot {
    /// The ability id; 0 when the slot is empty.
    pub ability: u32,
    /// The slot key (Unknown meaning; retail sends 402, 115 and 139 for the three slots of
    /// every table).
    pub key: u32,
}

/// The mapped slots of one ability table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedTable {
    /// The table id.
    pub table: u32,
    /// Its slots, at most [`MAX_MAPPED_SLOTS`].
    pub slots: Vec<MappedSlot>,
}

/// `persistentAbilityData`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersistentAbilities {
    /// The mapped slots of each ability table.
    pub tables: Vec<MappedTable>,
    /// Each weapon pool id and its `VersionNumber`.
    pub pool_versions: Vec<(u32, u32)>,
}

impl PersistentAbilities {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut tables = Vec::new();
        for _ in 0..reader.wide_prefixed()? {
            let table = reader.u32()?;
            let count = reader.wide_prefixed()?;
            if count > MAX_MAPPED_SLOTS as u64 {
                return Err(MessageError::CountTooLarge {
                    count,
                    limit: MAX_MAPPED_SLOTS as u64,
                });
            }
            let mut slots = Vec::new();
            for _ in 0..count {
                slots.push(MappedSlot {
                    ability: reader.u32()?,
                    key: reader.u32()?,
                });
            }
            tables.push(MappedTable { table, slots });
        }
        let mut pool_versions = Vec::new();
        for _ in 0..reader.wide_prefixed()? {
            pool_versions.push((reader.u32()?, reader.u32()?));
        }
        Ok(Self {
            tables,
            pool_versions,
        })
    }

    fn write(&self, writer: &mut Writer) {
        // `usize` counts fit 64 bits.
        writer.wide_prefixed(self.tables.len() as u64);
        for table in &self.tables {
            writer
                .u32(table.table)
                .wide_prefixed(table.slots.len() as u64);
            for slot in &table.slots {
                writer.u32(slot.ability).u32(slot.key);
            }
        }
        writer.wide_prefixed(self.pool_versions.len() as u64);
        for (pool, version) in &self.pool_versions {
            writer.u32(*pool).u32(*version);
        }
    }
}

/// The ability state; `None` sends no slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AbilityState {
    /// Child 0.
    pub persistent: Option<PersistentAbilities>,
}

impl AbilityState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Unsized`] for a child other than 0, and another
    /// [`MessageError`] when the body ends early or maps too many slots.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CHILDREN, |reader, child| {
            if child != PERSISTENT_CHILD {
                return Err(MessageError::Unsized {
                    field: FIELD,
                    byte: child,
                });
            }
            state.persistent = Some(PersistentAbilities::read(reader)?);
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let children = if self.persistent.is_some() {
            PERSISTENT_BIT
        } else {
            0
        };
        write_known(writer, children, CHILDREN, |writer, _| {
            if let Some(persistent) = &self.persistent {
                persistent.write(writer);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slots::SLOT_0;
    use nw_formats::objectstream::lowercase_crc32;

    /// The slot keys of every retail table (newchar 2026-09-18 and NH-MSQ-2.1).
    const RETAIL_KEYS: [u32; 3] = [402, 115, 139];
    /// Retail's greatsword table after the first apply (newchar 2026-09-18): the combo in the
    /// first slot.
    const RETAIL_GREATSWORD: [u8; 29] = [
        0x18, 0x0b, 0x9a, 0x9f, 0x03, 0x2e, 0xdf, 0x28, 0xbe, 0x00, 0x00, 0x01, 0x92, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x73, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x8b,
    ];
    /// Retail's map entry of the greatsword pool: version 1.
    const RETAIL_GREATSWORD_VERSION: [u8; 8] = [0x18, 0x0b, 0x9a, 0x9f, 0x00, 0x00, 0x00, 0x01];

    fn greatsword() -> PersistentAbilities {
        let table = lowercase_crc32("GreatSwordAbilityTable");
        PersistentAbilities {
            tables: vec![MappedTable {
                table,
                slots: RETAIL_KEYS
                    .iter()
                    .enumerate()
                    .map(|(index, key)| MappedSlot {
                        ability: if index == 0 {
                            lowercase_crc32("Ability_Greatsword_Combo")
                        } else {
                            0
                        },
                        key: *key,
                    })
                    .collect(),
            }],
            pool_versions: vec![(table, 1)],
        }
    }

    #[test]
    fn a_mapped_table_is_the_retail_bytes() {
        let state = AbilityState {
            persistent: Some(greatsword()),
        };
        let mut writer = Writer::new();
        state.write(&mut writer);
        let bytes = writer.finish();
        let mut expected = vec![SLOT_0, 0x01, 0x01];
        expected.extend_from_slice(&RETAIL_GREATSWORD);
        expected.push(0x01);
        expected.extend_from_slice(&RETAIL_GREATSWORD_VERSION);
        assert_eq!(bytes, expected);
        let mut reader = Reader::new(&bytes);
        assert_eq!(AbilityState::read(&mut reader).expect("read"), state);
        reader.finish().expect("whole");
    }

    #[test]
    fn other_children_stop_the_walk_and_too_many_slots_are_refused() {
        // Child 1 (`hitDataNumHits`).
        assert!(matches!(
            AbilityState::read(&mut Reader::new(&[0x01, 0x02, 0x00])),
            Err(MessageError::Unsized { .. })
        ));
        // One table with 4 slots.
        let four = [0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04];
        assert!(matches!(
            AbilityState::read(&mut Reader::new(&four)),
            Err(MessageError::CountTooLarge { .. })
        ));
        let mut writer = Writer::new();
        AbilityState::default().write(&mut writer);
        assert_eq!(writer.finish(), [0]);
    }
}
