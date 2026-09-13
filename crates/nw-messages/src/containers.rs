// SPDX-License-Identifier: AGPL-3.0-or-later
//! The item holders of a player besides the paperdoll (paperdoll-inventory note, sections 7 and
//! 8): `MB::ContainerComponentReplicatedState` (type 1755; player components 61, the bag, and
//! 76, of unknown use) and `MB::ItemManagementComponentReplicatedState` (type 5437, component 2:
//! the owned items that the loadouts name by key).

use crate::items::{FullList, ItemRecord};
use crate::slots::{
    SLOT_0, children_mask, read_children, read_slot_0, read_slot_byte, write_children,
};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::ContainerComponentReplicatedState`.
pub const CONTAINER_TYPE_INDEX: u64 = 1755;
/// Type index of `MB::ItemManagementComponentReplicatedState`.
pub const ITEM_MANAGEMENT_TYPE_INDEX: u64 = 5437;
/// The player component of the bag.
pub const BAG_COMPONENT: u64 = 61;

/// Children of the container's slot 0.
const CONTAINER_CHILDREN: u8 = 5;
/// Most items of a full container list (a fixed array of 500).
const MAX_CONTAINER_ITEMS: usize = 500;
/// Slots of type 5437: the owned items are in slot 1.
const ITEM_MANAGEMENT_SLOTS: u8 = 2;
/// Slot byte bit of slot 1.
const OWNED_SLOT_BIT: u8 = 0b10;
/// Children of type 5437 slot 1.
const OWNED_CHILDREN: u8 = 1;
const CONTAINER_FIELD: &str = "container";
const OWNED_FIELD: &str = "owned items";

/// The container state; `None` is a child that is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerState {
    /// Child 0: one item per container slot, empty slots included.
    pub items: Option<FullList<ItemRecord>>,
    /// Child 1: `Item Class`, a set of 64-bit values.
    pub item_classes: Option<Vec<u64>>,
    /// Child 2: `Bonus Max Encumbrance`.
    pub bonus_max_encumbrance: Option<u32>,
    /// Child 3: `Can Transfer items`.
    pub can_transfer: Option<bool>,
    /// Child 4: `Container Was Emptied`.
    pub was_emptied: Option<bool>,
}

impl ContainerState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another slot, or a list is a
    /// delta.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            CONTAINER_FIELD,
            CONTAINER_CHILDREN,
            |reader, child| {
                match child {
                    0 => {
                        let items = FullList::read(reader, ItemRecord::read)?;
                        if items.elements.len() > MAX_CONTAINER_ITEMS {
                            return Err(MessageError::CountTooLarge {
                                count: items.elements.len() as u64,
                                limit: MAX_CONTAINER_ITEMS as u64,
                            });
                        }
                        state.items = Some(items);
                    }
                    1 => {
                        let count = reader.wide_prefixed()?;
                        // Each class takes 8 bytes.
                        if count > (reader.remaining() / size_of::<u64>()) as u64 {
                            return Err(MessageError::LengthMismatch {
                                field: CONTAINER_FIELD,
                            });
                        }
                        state.item_classes =
                            Some((0..count).map(|_| reader.u64()).collect::<Result<_, _>>()?);
                    }
                    2 => state.bonus_max_encumbrance = Some(reader.u32()?),
                    3 => state.can_transfer = Some(reader.boolean()?),
                    _ => state.was_emptied = Some(reader.boolean()?),
                }
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path. Without a child, slot 0 is not sent.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when an item does not fit its packed form.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let children = children_mask(&[
            self.items.is_some(),
            self.item_classes.is_some(),
            self.bonus_max_encumbrance.is_some(),
            self.can_transfer.is_some(),
            self.was_emptied.is_some(),
        ]);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_children(writer, children, CONTAINER_CHILDREN, |writer, child| {
            match child {
                0 => {
                    if let Some(items) = &self.items {
                        items.write(writer, |writer, item| item.write(writer))?;
                    }
                }
                1 => {
                    if let Some(classes) = &self.item_classes {
                        // A `usize` count fits 64 bits.
                        writer.wide_prefixed(classes.len() as u64);
                        for class in classes {
                            writer.u64(*class);
                        }
                    }
                }
                2 => {
                    self.bonus_max_encumbrance.map(|bonus| writer.u32(bonus));
                }
                3 => {
                    self.can_transfer.map(|can| writer.boolean(can));
                }
                _ => {
                    self.was_emptied.map(|emptied| writer.boolean(emptied));
                }
            }
            Ok(())
        })
    }
}

/// One owned item: its key (named by the loadouts) and the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedItem {
    /// The key.
    pub key: u16,
    /// The item.
    pub item: ItemRecord,
}

/// The item management state: slot 1 child 0, `ownedItems`, when sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemManagementState {
    /// The owned items.
    pub owned: Option<FullList<OwnedItem>>,
}

impl ItemManagementState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets slot 0, or the map is a delta.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, OWNED_FIELD, ITEM_MANAGEMENT_SLOTS)?;
        if slots & !OWNED_SLOT_BIT != 0 {
            return Err(MessageError::Unsized {
                field: OWNED_FIELD,
                byte: slots,
            });
        }
        let mut state = Self::default();
        if slots == OWNED_SLOT_BIT {
            read_children(reader, OWNED_FIELD, OWNED_CHILDREN, |reader, _| {
                state.owned = Some(FullList::read(reader, |reader| {
                    Ok(OwnedItem {
                        key: reader.u16()?,
                        item: ItemRecord::read(reader)?,
                    })
                })?);
                Ok(())
            })?;
        }
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when an item does not fit its packed form.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let Some(owned) = &self.owned else {
            writer.u8(0);
            return Ok(());
        };
        writer.u8(OWNED_SLOT_BIT);
        write_children(writer, 1, OWNED_CHILDREN, |writer, _| {
            owned.write(writer, |writer, owned| {
                writer.u16(owned.key);
                owned.item.write(writer)
            })
        })
    }
}
