// SPDX-License-Identifier: AGPL-3.0-or-later
//! The gear states of a player's own spawn card (paperdoll-inventory note, sections 9 and 12):
//! the paperdoll (3183), the bag and the second container (1755, components 61 and 76) and the
//! owned items (5437), as full first states built from the equipped items. The shapes follow the
//! retail first states of a new character ("Soul Survivor" capture).

use crate::containers::{ContainerState, ItemManagementState, OwnedItem};
use crate::items::{FullList, ITEM_BOUND, ItemRecord, SYNC_LOGIN, SYNC_OWNED};
use crate::paperdoll::{
    ItemVisual, Loadout, PaperdollOwner, PaperdollPublic, PaperdollState, SlotBits, slot,
};
use crate::wire::RAW_ID_LEN;

/// Bag slots of a new character: the retail tutorial bag held 5 empty slots at entry.
pub const STARTER_BAG_SLOTS: usize = 5;
/// The name of the first loadout, as retail sends it.
const FIRST_LOADOUT_NAME: &[u8] = b"@lo_name 0";

/// One item worn at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquippedItem {
    /// The paperdoll slot id ([`slot`]).
    pub slot: u32,
    /// The client item index.
    pub item_index: u32,
    /// Gear score.
    pub gear_score: u32,
    /// Full durability (the item is new).
    pub durability: u32,
    /// Whether the item is bound to the character.
    pub bound: bool,
    /// The item instance id.
    pub instance: [u8; RAW_ID_LEN],
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

impl PlayerGear {
    /// The first states of a player who wears `equipped` (in that order) and has an empty bag.
    pub fn first(equipped: &[EquippedItem]) -> Self {
        let records: Vec<ItemRecord> = (0_u32..)
            .zip(equipped)
            .map(|(counter, item)| ItemRecord {
                quantity: 1,
                sync_reason: SYNC_LOGIN,
                container_slot: None,
                item_index: item.item_index,
                durability: item.durability,
                max_durability: item.durability,
                counter,
                gear_score: item.gear_score,
                instance: item.instance,
                flags: if item.bound { ITEM_BOUND } else { 0 },
                ..ItemRecord::EMPTY_SLOT
            })
            .collect();
        let slots: Vec<u32> = equipped.iter().map(|item| item.slot).collect();
        // The owned-items key of an item is its place in `equipped`.
        let keys = 0_u16..;
        let paperdoll = PaperdollState {
            owner: Some(PaperdollOwner {
                visible_durability: Some(FullList::first(
                    equipped.iter().map(|item| item.durability).collect(),
                )),
                visible_items: Some(FullList::first(records.clone())),
                hidden_items: Some(FullList::first(Vec::new())),
                hidden_slots: Some(FullList::first(Vec::new())),
                hidden_durability: Some(FullList::first(Vec::new())),
                bonus_equip_load: Some(0),
                is_local_player: Some(true),
                is_loadout_panel_open: Some(false),
                loadouts: Some(FullList::first(vec![Loadout {
                    name: FIRST_LOADOUT_NAME.to_vec(),
                    slots: slots.iter().copied().zip(keys.clone()).collect(),
                    flag: false,
                    extra: Vec::new(),
                }])),
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
                visible_slots: Some(FullList::first(slots)),
                visuals: Some(FullList::first(
                    equipped
                        .iter()
                        .map(|item| ItemVisual::of(item.item_index))
                        .collect(),
                )),
                loadout_swaps: None,
            }),
        };
        let owned = keys
            .zip(records)
            .map(|(key, item)| OwnedItem {
                key,
                item: ItemRecord {
                    sync_reason: SYNC_OWNED,
                    ..item
                },
            })
            .collect();
        Self {
            paperdoll,
            bag: ContainerState {
                items: Some(FullList::first(vec![
                    ItemRecord::EMPTY_SLOT;
                    STARTER_BAG_SLOTS
                ])),
                bonus_max_encumbrance: Some(0),
                was_emptied: Some(true),
                ..ContainerState::default()
            },
            second_container: ContainerState {
                bonus_max_encumbrance: Some(0),
                was_emptied: Some(false),
                ..ContainerState::default()
            },
            owned: ItemManagementState {
                owned: Some(FullList::first(owned)),
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

    fn wanderer() -> Vec<EquippedItem> {
        let armor = |slot, item_index| EquippedItem {
            slot,
            item_index,
            gear_score: ARMOR_GEAR_SCORE,
            durability: ARMOR_DURABILITY,
            bound: true,
            instance: [0; RAW_ID_LEN],
        };
        vec![
            armor(slot::CHEST, CAPTURE_CHEST_INDEX),
            armor(slot::HANDS, CAPTURE_CHEST_INDEX + 1),
            armor(slot::LEGS, CAPTURE_CHEST_INDEX + 2),
            armor(slot::FEET, CAPTURE_CHEST_INDEX + 3),
            EquippedItem {
                gear_score: SWORD_GEAR_SCORE,
                durability: SWORD_DURABILITY,
                ..armor(slot::MAIN_HAND_OPTION_1, CAPTURE_SWORD_INDEX)
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
