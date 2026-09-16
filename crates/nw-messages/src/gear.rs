// SPDX-License-Identifier: AGPL-3.0-or-later
//! The gear states of a player (paperdoll-inventory note, sections 2, 9 and 12): the paperdoll
//! (3183), the bag and the second container (1755, components 61 and 76) and the owned items
//! (5437), as full states built from the worn items and the bag. The shapes follow the retail
//! first states of a new character ("Soul Survivor" capture). A later change sends the full
//! states again with a higher version: a list's reader takes a full form at any time (its first
//! number 0), as retail equip bodies do (live-tutorial note, section 5.3).

use crate::containers::{ContainerState, ItemManagementState, OwnedItem};
use crate::items::{FIRST_VERSION, FullList, ITEM_BOUND, ItemRecord, SYNC_LOGIN, SYNC_OWNED};
use crate::paperdoll::{
    ItemVisual, Loadout, PaperdollOwner, PaperdollPublic, PaperdollState, SlotBits, slot,
};
use crate::wire::RAW_ID_LEN;

/// Bag slots of a new character: the retail tutorial bag held 5 empty slots at entry.
pub const STARTER_BAG_SLOTS: usize = 5;
/// The name of the first loadout, as retail sends it.
const FIRST_LOADOUT_NAME: &[u8] = b"@lo_name 0";
/// Sync reason of an item moved into a bag slot (paperdoll note, section 10.3).
const SYNC_MOVED_IN: u8 = 6;

/// One item of a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GearItem {
    /// The client item index.
    pub item_index: u32,
    /// Stack size.
    pub quantity: u32,
    /// Gear score.
    pub gear_score: u32,
    /// Full durability (the item is new).
    pub durability: u32,
    /// Whether the item is bound to the character.
    pub bound: bool,
    /// The item instance id.
    pub instance: [u8; RAW_ID_LEN],
}

/// One worn item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquippedItem {
    /// The paperdoll slot id ([`slot`]).
    pub slot: u32,
    /// The item.
    pub item: GearItem,
}

/// Whether paperdoll slot `slot_id` shows on the character: the armor and the weapon options.
/// Other slots (amulet, ring, quick slots, ...) are the paperdoll's hidden items (retail: the
/// amulet in slot 5 and potions in slot 14).
pub const fn is_visible_slot(slot_id: u32) -> bool {
    slot_id <= slot::FEET
        || slot::MAIN_HAND_OPTION_1 <= slot_id && slot_id <= slot::OFF_HAND_OPTION_3
}

/// The gear states of one player.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerGear {
    /// Type 3183, component 62.
    pub paperdoll: PaperdollState,
    /// Type 1755, component 61.
    pub bag: ContainerState,
    /// Type 1755, component 76.
    pub second_container: ContainerState,
    /// Type 5437, component 2.
    pub owned: ItemManagementState,
}

/// The full list of `elements` at `version`.
const fn list<T>(version: u64, elements: Vec<T>) -> FullList<T> {
    FullList {
        version: Some(version),
        elements,
    }
}

/// The record of `item`, sent for `sync_reason`, with the per-item `counter`.
fn record(
    item: &GearItem,
    sync_reason: u8,
    container_slot: Option<u16>,
    counter: u32,
) -> ItemRecord {
    ItemRecord {
        quantity: item.quantity,
        sync_reason,
        container_slot,
        item_index: item.item_index,
        durability: item.durability,
        max_durability: item.durability,
        counter,
        gear_score: item.gear_score,
        instance: item.instance,
        flags: if item.bound { ITEM_BOUND } else { 0 },
        ..ItemRecord::EMPTY_SLOT
    }
}

impl PlayerGear {
    /// The first states of a player who wears `equipped` (in that order) and has an empty bag.
    pub fn first(equipped: &[EquippedItem]) -> Self {
        Self::of(equipped, &[None; STARTER_BAG_SLOTS], FIRST_VERSION)
    }

    /// The states of a player who wears `equipped` (in that order) and has `bag` (one entry per
    /// bag slot), with lists of `version`. The owned-items key of a worn item is its place in
    /// `equipped`; the loadout pairs point at those keys.
    pub fn of(equipped: &[EquippedItem], bag: &[Option<GearItem>], version: u64) -> Self {
        // Worn items take the counters 0.. in order; bag items the ones after them.
        let worn: Vec<(u32, &EquippedItem)> = (0_u32..).zip(equipped).collect();
        let (visible, hidden): (Vec<_>, Vec<_>) = worn
            .iter()
            .partition(|(_, worn)| is_visible_slot(worn.slot));
        let records = |items: &[&(u32, &EquippedItem)]| -> Vec<ItemRecord> {
            items
                .iter()
                .map(|(counter, worn)| record(&worn.item, SYNC_LOGIN, None, *counter))
                .collect()
        };
        let visible: Vec<&(u32, &EquippedItem)> = visible.iter().collect();
        let hidden: Vec<&(u32, &EquippedItem)> = hidden.iter().collect();
        let durability = |items: &[&(u32, &EquippedItem)]| -> Vec<u32> {
            items.iter().map(|(_, worn)| worn.item.durability).collect()
        };
        let slots = |items: &[&(u32, &EquippedItem)]| -> Vec<u32> {
            items.iter().map(|(_, worn)| worn.slot).collect()
        };
        let keys = 0_u16..;
        let paperdoll = PaperdollState {
            owner: Some(PaperdollOwner {
                visible_durability: Some(list(version, durability(&visible))),
                visible_items: Some(list(version, records(&visible))),
                hidden_items: Some(list(version, records(&hidden))),
                hidden_slots: Some(list(version, slots(&hidden))),
                hidden_durability: Some(list(version, durability(&hidden))),
                bonus_equip_load: Some(0),
                is_local_player: Some(true),
                is_loadout_panel_open: Some(false),
                loadouts: Some(list(
                    version,
                    vec![Loadout {
                        name: FIRST_LOADOUT_NAME.to_vec(),
                        slots: equipped
                            .iter()
                            .map(|worn| worn.slot)
                            .zip(keys.clone())
                            .collect(),
                        flag: false,
                        extra: Vec::new(),
                    }],
                )),
                ..PaperdollOwner::default()
            }),
            public: Some(PaperdollPublic {
                sheathed: Some(SlotBits::of(
                    std::iter::once(slot::GATHERABLE_HAND)
                        .chain(slot::MAIN_HAND_OPTION_1..=slot::OFF_HAND_OPTION_3),
                )),
                attached: Some(SlotBits::default()),
                active: Some(SlotBits::of([
                    slot::MAIN_HAND_OPTION_1,
                    slot::OFF_HAND_OPTION_1,
                ])),
                visible_slots: Some(list(version, slots(&visible))),
                visuals: Some(list(
                    version,
                    visible
                        .iter()
                        .map(|(_, worn)| ItemVisual::of(worn.item.item_index))
                        .collect(),
                )),
                loadout_swaps: None,
            }),
        };
        let owned = keys
            .zip(&worn)
            .map(|(key, (counter, worn))| OwnedItem {
                key,
                item: record(&worn.item, SYNC_OWNED, None, *counter),
            })
            .collect();
        let first_bag_counter = u32::try_from(equipped.len()).unwrap_or(u32::MAX);
        let bag_records = (0_u16..)
            .zip(bag)
            .zip(first_bag_counter..)
            .map(|((bag_slot, item), counter)| match item {
                Some(item) => record(item, SYNC_MOVED_IN, Some(bag_slot), counter),
                None => ItemRecord::EMPTY_SLOT,
            })
            .collect();
        Self {
            paperdoll,
            bag: ContainerState {
                items: Some(list(version, bag_records)),
                bonus_max_encumbrance: Some(0),
                was_emptied: Some(bag.iter().all(Option::is_none)),
                ..ContainerState::default()
            },
            second_container: ContainerState {
                bonus_max_encumbrance: Some(0),
                was_emptied: Some(false),
                ..ContainerState::default()
            },
            owned: ItemManagementState {
                owned: Some(list(version, owned)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Reader, Writer};

    /// The retail first paperdoll of a new wanderer (paperdoll note, section 10.1), with the
    /// five instance ids set to zero and the unknown ammo child left out: chest, hands, legs and
    /// feet at capture indexes 40226 to 40229, the tutorial sword at 1223; compared up to the
    /// loadout list, whose pairs retail sends in another order.
    const RETAIL_PAPERDOLL_WITHOUT_IDS: &str = "\
        06ff0001030500000177000001770000017700000177000003e8000103050001084aebff0519b705004b000000000000\
        000000000000000000000000000000000000000100000000000000000001084aebff245ab705014b0000000000000000\
        00000000000000000000000000000000000100000000000000000001084aebff439bb705024b00000000000000000000\
        0000000000000000000000000000000100000000000000000001084aebff62dcb705034b000000000000000000000000\
        000000000000000000000000000100000000000000000001084ad965536fa80f043c0000000000000000000000000000\
        0000000000000000000000010000000000000000000103000001030000010300000001090000010301";
    /// The retail first bag (section 10.2) and second container (section 10.4).
    const RETAIL_SECOND_CONTAINER: [u8; 7] = [0x01, 0x14, 0, 0, 0, 0, 0];
    const ARMOR_DURABILITY: u32 = 375;
    const ARMOR_GEAR_SCORE: u32 = 75;
    const SWORD_DURABILITY: u32 = 1000;
    const SWORD_GEAR_SCORE: u32 = 60;
    const CAPTURE_CHEST_INDEX: u32 = 40_226;
    const CAPTURE_SWORD_INDEX: u32 = 1223;
    /// The retail bag (236 bytes): slot byte, children 0, 2 and 4, then 5 empty items of 45 bytes.
    const RETAIL_BAG_HEAD: [u8; 6] = [0x01, 0x15, 0x00, 0x01, 0x03, 0x05];
    const EMPTY_ITEM_BYTES: usize = 45;
    const RETAIL_BAG_TAIL: [u8; 5] = [0, 0, 0, 0, 0x01];

    fn item(item_index: u32, gear_score: u32, durability: u32) -> GearItem {
        GearItem {
            item_index,
            quantity: 1,
            gear_score,
            durability,
            bound: true,
            instance: [0; RAW_ID_LEN],
        }
    }

    fn wanderer() -> Vec<EquippedItem> {
        let armor = |slot, item_index| EquippedItem {
            slot,
            item: item(item_index, ARMOR_GEAR_SCORE, ARMOR_DURABILITY),
        };
        vec![
            armor(slot::CHEST, CAPTURE_CHEST_INDEX),
            armor(slot::HANDS, CAPTURE_CHEST_INDEX + 1),
            armor(slot::LEGS, CAPTURE_CHEST_INDEX + 2),
            armor(slot::FEET, CAPTURE_CHEST_INDEX + 3),
            EquippedItem {
                slot: slot::MAIN_HAND_OPTION_1,
                item: item(CAPTURE_SWORD_INDEX, SWORD_GEAR_SCORE, SWORD_DURABILITY),
            },
        ]
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    fn encode(write: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut writer = Writer::new();
        write(&mut writer);
        writer.finish()
    }

    #[test]
    fn the_paperdoll_owner_slot_is_the_retail_one() {
        let gear = PlayerGear::first(&wanderer());
        let bytes = encode(|writer| gear.paperdoll.write(writer).expect("write"));
        let retail = hex(RETAIL_PAPERDOLL_WITHOUT_IDS);
        assert_eq!(bytes[..retail.len()], retail[..]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(PaperdollState::read(&mut reader), Ok(gear.paperdoll));
        assert!(reader.is_empty());
    }

    #[test]
    fn the_containers_are_the_retail_ones() {
        let gear = PlayerGear::first(&wanderer());
        let bag = encode(|writer| gear.bag.write(writer).expect("bag"));
        assert_eq!(bag[..RETAIL_BAG_HEAD.len()], RETAIL_BAG_HEAD);
        assert_eq!(
            bag.len(),
            RETAIL_BAG_HEAD.len() + STARTER_BAG_SLOTS * EMPTY_ITEM_BYTES + RETAIL_BAG_TAIL.len()
        );
        assert_eq!(bag[bag.len() - RETAIL_BAG_TAIL.len()..], RETAIL_BAG_TAIL);
        assert_eq!(ContainerState::read(&mut Reader::new(&bag)), Ok(gear.bag));
        let second = encode(|writer| gear.second_container.write(writer).expect("second"));
        assert_eq!(second, RETAIL_SECOND_CONTAINER);
    }

    #[test]
    fn a_later_state_holds_the_bag_and_hidden_slots_at_its_version() {
        const VERSION: u64 = 9;
        const POTION_INDEX: u32 = 4885;
        const POTION_SLOT: u32 = 14;
        const POTIONS: u32 = 2;
        let mut equipped = wanderer();
        equipped.push(EquippedItem {
            slot: POTION_SLOT,
            item: GearItem {
                quantity: POTIONS,
                ..item(POTION_INDEX, 0, 0)
            },
        });
        let bag = [
            None,
            Some(item(
                CAPTURE_CHEST_INDEX,
                ARMOR_GEAR_SCORE,
                ARMOR_DURABILITY,
            )),
        ];
        let gear = PlayerGear::of(&equipped, &bag, VERSION);
        let owner = gear.paperdoll.owner.as_ref().expect("owner");
        let visible = owner.visible_items.as_ref().expect("visible");
        assert_eq!(visible.version, Some(VERSION));
        assert_eq!(visible.elements.len(), wanderer().len());
        let hidden = owner
            .hidden_items
            .as_ref()
            .expect("hidden")
            .elements
            .clone();
        assert_eq!(
            (hidden[0].item_index, hidden[0].quantity),
            (POTION_INDEX, POTIONS)
        );
        assert_eq!(
            owner.hidden_slots.as_ref().expect("slots").elements,
            [POTION_SLOT]
        );
        let loadout = &owner.loadouts.as_ref().expect("loadouts").elements[0];
        assert_eq!(loadout.slots.last(), Some(&(POTION_SLOT, 5)));
        let items = gear.bag.items.as_ref().expect("bag").elements.clone();
        assert_eq!(items[0], ItemRecord::EMPTY_SLOT);
        assert_eq!(
            (
                items[1].container_slot,
                items[1].sync_reason,
                items[1].item_index
            ),
            (Some(1), SYNC_MOVED_IN, CAPTURE_CHEST_INDEX)
        );
        assert_eq!(gear.bag.was_emptied, Some(false));
        let bytes = encode(|writer| gear.paperdoll.write(writer).expect("write"));
        assert_eq!(
            PaperdollState::read(&mut Reader::new(&bytes)),
            Ok(gear.paperdoll)
        );
    }

    #[test]
    fn the_owned_items_carry_the_loadout_keys() {
        let gear = PlayerGear::first(&wanderer());
        let bytes = encode(|writer| gear.owned.write(writer).expect("owned"));
        assert_eq!(bytes[..6], [0x02, 0x01, 0x00, 0x01, 0x03, 0x05]);
        let read = ItemManagementState::read(&mut Reader::new(&bytes)).expect("read");
        let owned = read.owned.expect("owned").elements;
        assert_eq!(owned.len(), wanderer().len());
        assert!(
            owned
                .iter()
                .all(|owned| owned.item.sync_reason == SYNC_OWNED)
        );
        assert_eq!(owned[4].key, 4);
        assert_eq!(owned[4].item.item_index, CAPTURE_SWORD_INDEX);
    }
}
