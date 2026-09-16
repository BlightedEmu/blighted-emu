// SPDX-License-Identifier: AGPL-3.0-or-later
//! `CooldownTimersComponentReplicatedState` (type 2932, own player component 45): the running
//! consumable cooldowns (xp-potion-exit note, section 2.3).
//!
//! Slot 0 child 3 (`ccdmap`) is a list in the full form; each entry names the consumable, the end
//! of its cooldown (µs since 2000-01-01, the server clock) and the cooldown group. Retail sends the
//! whole list after a potion and an empty list when the tutorial ends. Children 0 to 2 are
//! **Unknown**; a body that sends them is [`MessageError::Unsized`].

use crate::items::FullList;
use crate::slots::{SLOT_0, read_slot_0, write_known_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `CooldownTimersComponentReplicatedState`.
pub const COOLDOWNS_TYPE_INDEX: u64 = 2932;
/// The own player component of the cooldowns.
pub const COOLDOWNS_COMPONENT: u64 = 45;

/// Children of slot 0 that this server knows: 0 to 3.
const CHILDREN: u8 = 4;
const CONSUMABLES_CHILD: u8 = 3;
const FIELD: &str = "cooldowns";

/// One running consumable cooldown (20 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsumableCooldown {
    /// CRC-32 of the lowercase consumable id.
    pub consumable: u32,
    /// When it ends: µs since 2000-01-01.
    pub end_us: u64,
    /// CRC-32 of the lowercase cooldown id (the group of consumables that share it).
    pub group: u32,
}

impl ConsumableCooldown {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let consumable = reader.u32()?;
        let end_us = reader.u64()?;
        let group = reader.u32()?;
        let again = reader.u32()?;
        if again != consumable {
            return Err(MessageError::Unsized {
                field: FIELD,
                byte: 0,
            });
        }
        Ok(Self {
            consumable,
            end_us,
            group,
        })
    }

    /// Retail writes the consumable twice: before the end time and after the group.
    fn write(&self, writer: &mut Writer) {
        writer
            .u32(self.consumable)
            .u64(self.end_us)
            .u32(self.group)
            .u32(self.consumable);
    }
}

/// The cooldowns state; `None` sends no change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CooldownsState {
    /// The consumable cooldowns.
    pub consumables: Option<FullList<ConsumableCooldown>>,
}

impl CooldownsState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sends a child other than 3, or names
    /// two consumables in one entry.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CHILDREN, |reader, child| {
            if child != CONSUMABLES_CHILD {
                return Err(MessageError::Unsized {
                    field: FIELD,
                    byte: child,
                });
            }
            state.consumables = Some(FullList::read(reader, |reader| {
                ConsumableCooldown::read(reader)
            })?);
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
        let Some(list) = &self.consumables else {
            writer.u8(0);
            return Ok(());
        };
        writer.u8(SLOT_0);
        let mut result = Ok(());
        write_known_children(writer, 1 << CONSUMABLES_CHILD, CHILDREN, |writer, _| {
            result = list.write(writer, |writer, cooldown| {
                cooldown.write(writer);
                Ok(())
            });
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail after the potion (xp-potion-exit note, section 2.3).
    const RETAIL: [u8; 25] = [
        0x01, 0x08, 0x00, 0x01, 0xb6, 0xd7, 0x01, 0x57, 0x31, 0x5d, 0x70, 0x00, 0x03, 0x01, 0x30,
        0x0c, 0x2a, 0x3e, 0x84, 0x3b, 0xe0, 0xa5, 0x4a, 0x57, 0x31,
    ];

    #[test]
    fn the_retail_potion_cooldown_reads_and_writes_the_same_bytes() {
        let mut body = RETAIL.to_vec();
        body.extend([0x5d, 0x70]);
        let mut reader = Reader::new(&body);
        let state = CooldownsState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        assert_eq!(
            state.consumables.as_ref().map(|list| list.elements.clone()),
            Some(vec![ConsumableCooldown {
                consumable: 0x5731_5d70,
                end_us: 0x0003_0130_0c2a_3e84,
                group: 0x3be0_a54a,
            }])
        );
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), body);
        let mut writer = Writer::new();
        CooldownsState::default().write(&mut writer).expect("write");
        assert_eq!(writer.finish(), [0]);
    }
}
