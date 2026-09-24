// SPDX-License-Identifier: AGPL-3.0-or-later
//! The item holders of a player besides the paperdoll (paperdoll-inventory note, sections 7 and
//! 8): `MB::ContainerComponentReplicatedState` (type 1755; player components 61, the bag, and
//! 76, of unknown use) and `MB::ItemManagementComponentReplicatedState` (type 5437, component 2:
//! the owned items that the loadouts name by key).

use crate::items::{FULL_FORM, FullList, ItemRecord, VERSION_PRESENT};
use crate::list_delta::{ReplicatedList, child_change, value_change};
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
/// Entries per group of a delta form (one mask byte each).
const DELTA_GROUP: usize = 8;
const CONTAINER_FIELD: &str = "container";
const OWNED_FIELD: &str = "owned items";

/// The container state; `None` is a child that is not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerState {
    /// Child 0: one item per container slot, empty slots included; a change sends the changed
    /// slots alone (retail newchar 2026-09-18: the bag slot of each equipped item).
    pub items: Option<ReplicatedList<ItemRecord>>,
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
    /// The children that changed from `before` (`None`: never sent) to `now` at `version`.
    pub(crate) fn change(before: Option<&Self>, now: &Self, version: u64) -> Self {
        Self {
            items: child_change(
                before.and_then(|before| before.items.as_ref()),
                now.items.as_ref(),
                version,
            ),
            item_classes: value_change(
                before.and_then(|before| before.item_classes.as_ref()),
                now.item_classes.as_ref(),
            ),
            bonus_max_encumbrance: value_change(
                before.and_then(|before| before.bonus_max_encumbrance.as_ref()),
                now.bonus_max_encumbrance.as_ref(),
            ),
            can_transfer: value_change(
                before.and_then(|before| before.can_transfer.as_ref()),
                now.can_transfer.as_ref(),
            ),
            was_emptied: value_change(
                before.and_then(|before| before.was_emptied.as_ref()),
                now.was_emptied.as_ref(),
            ),
        }
    }

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
                        let items = ReplicatedList::read(reader, CONTAINER_FIELD, ItemRecord::read)?;
                        let count = match &items {
                            ReplicatedList::Full(list) => list.elements.len(),
                            ReplicatedList::Changes(changes) => changes.len(),
                        };
                        if count > MAX_CONTAINER_ITEMS {
                            return Err(MessageError::CountTooLarge {
                                count: count as u64,
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

/// One change of the owned items: the item of a key set, or the key removed (retail newchar
/// 2026-09-18 at 1792696532378: the chest taken off removed key 0 and the new chest set key 11
/// in one change).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedChange {
    /// The key.
    pub key: u16,
    /// The version; retail sends it on a change's first entry and 0 on the others.
    pub version: u64,
    /// The item; `None` removes the key.
    pub item: Option<ItemRecord>,
}

/// The owned items whole or as changes. The changes of this map name each entry by its key
/// (a big-endian `u16`) where a list's changes name an index, and an entry without its mask
/// bit removes the key (retail newchar 2026-09-18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedItems {
    /// Every owned item.
    Full(FullList<OwnedItem>),
    /// The keys set or removed.
    Changes(Vec<OwnedChange>),
}

impl OwnedItems {
    /// The changes from the owned items `before` (`None`: never sent) to `now` at `version`:
    /// the keys whose item is new or changed, and the keys gone; `None` when nothing changed.
    pub fn change(before: Option<&[OwnedItem]>, now: &[OwnedItem], version: u64) -> Option<Self> {
        let Some(before) = before else {
            return Some(Self::Full(FullList {
                version: Some(version),
                elements: now.to_vec(),
            }));
        };
        let item_of = |items: &[OwnedItem], key: u16| {
            items.iter().find(|owned| owned.key == key).map(|owned| owned.item)
        };
        let removed = before
            .iter()
            .filter(|old| item_of(now, old.key).is_none())
            .map(|old| (old.key, None));
        let set = now
            .iter()
            .filter(|new| item_of(before, new.key) != Some(new.item))
            .map(|new| (new.key, Some(new.item)));
        let changes: Vec<OwnedChange> = removed
            .chain(set)
            .enumerate()
            .map(|(position, (key, item))| OwnedChange {
                key,
                version: if position == 0 { version } else { 0 },
                item,
            })
            .collect();
        (!changes.is_empty()).then_some(Self::Changes(changes))
    }

    /// The whole map of a full form; `None` for changes.
    pub const fn full(&self) -> Option<&FullList<OwnedItem>> {
        match self {
            Self::Full(list) => Some(list),
            Self::Changes(_) => None,
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        if Reader::new(reader.rest()).wide_prefixed()? == FULL_FORM {
            return Ok(Self::Full(FullList::read(reader, |reader| {
                Ok(OwnedItem {
                    key: reader.u16()?,
                    item: ItemRecord::read(reader)?,
                })
            })?));
        }
        let count = reader.wide_prefixed()?;
        // Each entry takes at least four bytes.
        if count > reader.remaining() as u64 {
            return Err(MessageError::LengthMismatch { field: OWNED_FIELD });
        }
        // At most the bytes left.
        let mut left = count as usize;
        let mut changes = Vec::with_capacity(left);
        while left > 0 {
            let mask = reader.u8()?;
            let group = left.min(DELTA_GROUP);
            for entry in 0..group {
                let key = reader.u16()?;
                let flag = reader.u8()?;
                if flag != VERSION_PRESENT {
                    return Err(MessageError::Unsized {
                        field: OWNED_FIELD,
                        byte: flag,
                    });
                }
                let version = reader.wide_prefixed()?;
                let item = (mask & 1 << entry != 0)
                    .then(|| ItemRecord::read(reader))
                    .transpose()?;
                changes.push(OwnedChange { key, version, item });
            }
            left -= group;
        }
        Ok(Self::Changes(changes))
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        match self {
            Self::Full(list) => list.write(writer, |writer, owned| {
                writer.u16(owned.key);
                owned.item.write(writer)
            }),
            Self::Changes(changes) => {
                // A `usize` count fits 64 bits.
                writer.wide_prefixed(changes.len() as u64);
                for group in changes.chunks(DELTA_GROUP) {
                    let mask = (0_u8..)
                        .zip(group)
                        .filter(|(_, change)| change.item.is_some())
                        .fold(0_u8, |mask, (entry, _)| mask | 1 << entry);
                    writer.u8(mask);
                    for change in group {
                        writer
                            .u16(change.key)
                            .u8(VERSION_PRESENT)
                            .wide_prefixed(change.version);
                        if let Some(item) = &change.item {
                            item.write(writer)?;
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

/// The item management state: slot 1 child 0, `ownedItems`, when sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemManagementState {
    /// The owned items.
    pub owned: Option<OwnedItems>,
}

impl ItemManagementState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets slot 0, or a change of the map
    /// carries no version.
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
                state.owned = Some(OwnedItems::read(reader)?);
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
        write_children(writer, 1, OWNED_CHILDREN, |writer, _| owned.write(writer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// Retail newchar 2026-09-18 at 1792695867310, the owned items 37 ms after the equip of the
    /// greatsword: key 4 set.
    const RETAIL_KEY_SET: &str = concat!(
        "02010101000401904900013a169be38ac3bc0a084bb312000000000000000083544b8beb8d41509668eb37",
        "262c7422010000000000000000"
    );
    /// The same at 1792696532378: the chest taken off removes key 0, the new chest sets key 11.
    const RETAIL_REMOVE_AND_SET: &str = concat!(
        "02010202000001cdb002000b010000013a169b625edcb7051b4b00000000000000000068d37fe38e2945",
        "489c192307826aa2f1010000000000000000"
    );
    /// The bag at 1792695867310: slot 2 emptied by the same equip.
    const RETAIL_BAG_SLOT_EMPTIED: &str = concat!(
        "01010101020190490000132500d00000000000000000000000000000000000000000000000000000000000",
        "00000000000000000000"
    );
    const CHANGE_VERSION: u64 = 4688;
    const GREATSWORD_INDEX: u32 = 40_263;
    const OLD_CHEST_KEY: u16 = 0;
    const NEW_CHEST_KEY: u16 = 11;
    const GREATSWORD_KEY: u16 = 4;

    fn encode_owned(state: &ItemManagementState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    #[test]
    fn the_owned_item_changes_read_and_write_the_retail_bodies() {
        let bytes = hex(RETAIL_KEY_SET);
        let state = ItemManagementState::read(&mut Reader::new(&bytes)).expect("read");
        let Some(OwnedItems::Changes(changes)) = &state.owned else {
            panic!("a change");
        };
        assert_eq!(changes.len(), 1);
        assert_eq!(
            (changes[0].key, changes[0].version),
            (GREATSWORD_KEY, CHANGE_VERSION)
        );
        assert_eq!(
            changes[0].item.map(|item| item.item_index),
            Some(GREATSWORD_INDEX)
        );
        assert_eq!(encode_owned(&state), bytes);
        let bytes = hex(RETAIL_REMOVE_AND_SET);
        let state = ItemManagementState::read(&mut Reader::new(&bytes)).expect("read");
        let Some(OwnedItems::Changes(changes)) = &state.owned else {
            panic!("a change");
        };
        assert_eq!(
            changes
                .iter()
                .map(|change| (change.key, change.item.is_some()))
                .collect::<Vec<_>>(),
            [(OLD_CHEST_KEY, false), (NEW_CHEST_KEY, true)]
        );
        assert_eq!(changes[1].version, 0, "only the first entry carries it");
        assert_eq!(encode_owned(&state), bytes);
    }

    #[test]
    fn an_owned_change_removes_first_then_sets_as_retail() {
        let bytes = hex(RETAIL_REMOVE_AND_SET);
        let Some(OwnedItems::Changes(changes)) = ItemManagementState::read(&mut Reader::new(
            &bytes,
        ))
        .expect("read")
        .owned
        else {
            panic!("a change");
        };
        let new_chest = changes[1].item.expect("set");
        let old_chest = ItemRecord {
            item_index: GREATSWORD_INDEX,
            ..new_chest
        };
        let before = [OwnedItem {
            key: OLD_CHEST_KEY,
            item: old_chest,
        }];
        let now = [OwnedItem {
            key: NEW_CHEST_KEY,
            item: new_chest,
        }];
        let version = changes[0].version;
        let change = OwnedItems::change(Some(&before), &now, version);
        assert_eq!(change, Some(OwnedItems::Changes(changes)));
        assert_eq!(OwnedItems::change(Some(&now), &now, version), None);
        assert!(matches!(
            OwnedItems::change(None, &now, version),
            Some(OwnedItems::Full(_))
        ));
    }

    #[test]
    fn a_bag_change_reads_and_writes_the_retail_body() {
        let bytes = hex(RETAIL_BAG_SLOT_EMPTIED);
        let state = ContainerState::read(&mut Reader::new(&bytes)).expect("read");
        let Some(ReplicatedList::Changes(changes)) = &state.items else {
            panic!("a change");
        };
        assert_eq!(changes.len(), 1);
        assert_eq!(
            (changes[0].index, changes[0].version, changes[0].value),
            (2, CHANGE_VERSION, ItemRecord::EMPTY_SLOT)
        );
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        assert_eq!(writer.finish(), bytes);
    }
}
