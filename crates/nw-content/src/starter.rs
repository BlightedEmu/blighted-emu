// SPDX-License-Identifier: AGPL-3.0-or-later
//! The starter kit of each archetype pick and the client's item index (starter-kit and
//! paperdoll notes, 2026-09-10).
//!
//! The pick saves a backstory id (`Archetype_Mystic`, ...). Its `BackstoryDefinition` row lists
//! `InventoryItem` as `item:quantity` pairs. A retail new character has the items of quantity 1
//! equipped at login (four armor pieces and the tutorial sword); the items of quantity 0 (the
//! archetype's weapons and helmet) come later from the bag on the tutorial beach (live-tutorial
//! note, section 5.2).
//!
//! The wire names an item by its client item index: the client walks the `MasterItemDefinitions`
//! sheets in byte order of their unique id, the rows of each in file order, skips `HousingItem`
//! rows, and numbers the others from 1 (index 0 is no item).

use std::collections::HashMap;

use crate::Tables;

/// The item type whose rows the client leaves out of its item index.
const HOUSING_ITEM_TYPE: &str = "HousingItem";
/// Separates the pairs of a backstory's `InventoryItem` cell.
const KIT_PAIR_SEPARATOR: char = ',';
/// Separates an item id from its quantity in one pair.
const KIT_QUANTITY_SEPARATOR: char = ':';
/// Separates the parts of an `ItemClass` cell.
const ITEM_CLASS_SEPARATOR: char = '+';
/// A kit item of at least this quantity is equipped at login; one of less is in the beach bag.
const EQUIPPED_AT_LOGIN: u32 = 1;

/// Where an equipped item goes on the paperdoll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipSlot {
    /// `EquippableHead`.
    Head,
    /// `EquippableChest`.
    Chest,
    /// `EquippableHands`.
    Hands,
    /// `EquippableLegs`.
    Legs,
    /// `EquippableFeet`.
    Feet,
    /// `EquippableMainHand` or `EquippableTwoHand`: the first main-hand option.
    MainHand,
    /// `EquippableOffHand`: the first off-hand option.
    OffHand,
}

impl EquipSlot {
    /// The slot of an `ItemClass` cell such as `EquippableChest+Armor+Heavy`; `None` for an item
    /// that is not worn.
    pub fn of_item_class(item_class: &str) -> Option<Self> {
        item_class
            .split(ITEM_CLASS_SEPARATOR)
            .find_map(|part| match part {
                "EquippableHead" => Some(Self::Head),
                "EquippableChest" => Some(Self::Chest),
                "EquippableHands" => Some(Self::Hands),
                "EquippableLegs" => Some(Self::Legs),
                "EquippableFeet" => Some(Self::Feet),
                "EquippableMainHand" | "EquippableTwoHand" => Some(Self::MainHand),
                "EquippableOffHand" => Some(Self::OffHand),
                _ => None,
            })
    }
}

/// One `MasterItemDefinitions` row, as the kit needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemRow {
    /// `ItemID`.
    pub item_id: String,
    /// `ItemType`.
    pub item_type: Option<String>,
    /// `ItemClass`.
    pub item_class: Option<String>,
    /// `GearScoreOverride`, else `MinGearScore`.
    pub gear_score: Option<u32>,
    /// `Durability`.
    pub durability: Option<u32>,
    /// `BindOnPickup` is set.
    pub bind_on_pickup: bool,
}

/// The plain content of the kit datasheets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StarterSource {
    /// Each `MasterItemDefinitions` sheet: its unique id and its rows in file order.
    pub item_sheets: Vec<(String, Vec<ItemRow>)>,
    /// `BackstoryDefinition`: (backstory id, `InventoryItem` cell).
    pub backstories: Vec<(String, String)>,
}

impl StarterSource {
    /// The kit content of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let number = |value: Option<i64>| value.and_then(|value| u32::try_from(value).ok());
        Self {
            item_sheets: tables
                .master_item_definitions
                .iter()
                .map(|table| {
                    let rows = table
                        .rows
                        .iter()
                        .map(|row| ItemRow {
                            item_id: row.item_id.clone(),
                            item_type: row.item_type.clone(),
                            item_class: row.item_class.clone(),
                            gear_score: number(row.gear_score_override)
                                .or_else(|| number(row.min_gear_score)),
                            durability: number(row.durability),
                            bind_on_pickup: row.bind_on_pickup.is_some_and(|bind| bind != 0),
                        })
                        .collect();
                    (table.unique_id.clone(), rows)
                })
                .collect(),
            backstories: tables
                .backstory_definition
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let items = row.inventory_item.clone()?;
                    Some((row.backstory_id.clone(), items))
                })
                .collect(),
        }
    }
}

/// One item of the item catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitItem {
    /// `ItemID`.
    pub item_id: String,
    /// The client item index.
    pub item_index: u32,
    /// Where it is worn; `None` for an item that is not worn (a potion).
    pub slot: Option<EquipSlot>,
    /// Its gear score.
    pub gear_score: u32,
    /// Its full durability.
    pub durability: u32,
    /// Whether it is bound to the character.
    pub bound: bool,
}

/// The kits of one backstory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Kit {
    /// Worn at login.
    login: Vec<KitItem>,
    /// In the beach bag.
    bag: Vec<KitItem>,
}

/// The kits of every backstory, and every item by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StarterKits {
    kits: HashMap<String, Kit>,
    items: HashMap<String, KitItem>,
}

impl StarterKits {
    /// The kits of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(StarterSource::from_tables(tables))
    }

    /// The kits of `source`. A kit item that has no item row or no worn slot is left out.
    pub fn build(mut source: StarterSource) -> Self {
        source
            .item_sheets
            .sort_by(|(left, _), (right, _)| left.as_bytes().cmp(right.as_bytes()));
        let indexed = source
            .item_sheets
            .iter()
            .flat_map(|(_, rows)| rows)
            .filter(|row| row.item_type.as_deref() != Some(HOUSING_ITEM_TYPE));
        let items: HashMap<String, KitItem> = (1_u32..)
            .zip(indexed)
            .map(|(item_index, row)| {
                let item = KitItem {
                    item_id: row.item_id.clone(),
                    item_index,
                    slot: row.item_class.as_deref().and_then(EquipSlot::of_item_class),
                    gear_score: row.gear_score.unwrap_or_default(),
                    durability: row.durability.unwrap_or_default(),
                    bound: row.bind_on_pickup,
                };
                (row.item_id.clone(), item)
            })
            .collect();
        let kits = source
            .backstories
            .iter()
            .map(|(backstory, inventory)| {
                let mut kit = Kit::default();
                for (item_id, quantity) in kit_pairs(inventory) {
                    let Some(item) = items.get(item_id).filter(|item| item.slot.is_some()) else {
                        continue;
                    };
                    if quantity >= EQUIPPED_AT_LOGIN {
                        kit.login.push(item.clone());
                    } else {
                        kit.bag.push(item.clone());
                    }
                }
                (backstory.clone(), kit)
            })
            .collect();
        Self { kits, items }
    }

    /// The items that a character of `backstory` has equipped at login, in kit order; `None` for
    /// a backstory without a row.
    pub fn kit(&self, backstory: &str) -> Option<&[KitItem]> {
        self.kits.get(backstory).map(|kit| kit.login.as_slice())
    }

    /// The items in the beach bag of a character of `backstory`, in kit order; empty for a
    /// backstory without a row.
    pub fn bag(&self, backstory: &str) -> &[KitItem] {
        self.kits
            .get(backstory)
            .map_or(&[], |kit| kit.bag.as_slice())
    }

    /// The item `item_id`.
    pub fn item(&self, item_id: &str) -> Option<&KitItem> {
        self.items.get(item_id)
    }
}

/// The (item id, quantity) pairs of an `InventoryItem` cell; a pair without a number is left
/// out.
fn kit_pairs(inventory: &str) -> impl Iterator<Item = (&str, u32)> {
    inventory.split(KIT_PAIR_SEPARATOR).filter_map(|pair| {
        let (item_id, quantity) = pair.trim().split_once(KIT_QUANTITY_SEPARATOR)?;
        Some((item_id.trim(), quantity.trim().parse().ok()?))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKSTORY: &str = "Archetype_Swordsman";
    const ARMOR_GEAR_SCORE: u32 = 75;
    const ARMOR_DURABILITY: u32 = 375;
    const SWORD_GEAR_SCORE: u32 = 60;
    const SWORD_DURABILITY: u32 = 1000;

    fn row(item_id: &str, item_class: &str, gear_score: u32, durability: u32) -> ItemRow {
        ItemRow {
            item_id: item_id.to_owned(),
            item_type: Some("Armor".to_owned()),
            item_class: Some(item_class.to_owned()),
            gear_score: Some(gear_score),
            durability: Some(durability),
            bind_on_pickup: true,
        }
    }

    fn armor(item_id: &str, item_class: &str) -> ItemRow {
        row(item_id, item_class, ARMOR_GEAR_SCORE, ARMOR_DURABILITY)
    }

    /// Two sheets given out of order, a housing row that takes no index, and the wanderer kit.
    fn source() -> StarterSource {
        StarterSource {
            item_sheets: vec![
                (
                    "MasterItemDefinitions_Quest".to_owned(),
                    vec![
                        armor("SwordsmanArchetypeHead", "EquippableHead+Armor+Heavy"),
                        armor(
                            "SwordsmanArchetype_2hGreatsword",
                            "Weapon+EquippableTwoHand+GreatSword+Melee",
                        ),
                        armor("SwordsmanArchetypeChest", "EquippableChest+Armor+Heavy"),
                        armor("SwordsmanArchetypeHands", "EquippableHands+Armor+Heavy"),
                        armor("SwordsmanArchetypeLegs", "EquippableLegs+Armor+Heavy"),
                        armor("SwordsmanArchetypeFeet", "EquippableFeet+Armor+Heavy"),
                    ],
                ),
                (
                    "MasterItemDefinitions_Common".to_owned(),
                    vec![
                        ItemRow {
                            item_type: Some(HOUSING_ITEM_TYPE.to_owned()),
                            ..armor("House_Chair", "Furniture")
                        },
                        ItemRow {
                            item_type: Some("Consumable".to_owned()),
                            ..armor("PotionHealthT1", "Consumable+Potion")
                        },
                        row(
                            "1hSwordT2_FTUE",
                            "Weapon+EquippableMainHand+Sword+Melee",
                            SWORD_GEAR_SCORE,
                            SWORD_DURABILITY,
                        ),
                    ],
                ),
            ],
            backstories: vec![(
                BACKSTORY.to_owned(),
                "SwordsmanArchetypeHead:0,SwordsmanArchetypeChest:1,SwordsmanArchetypeHands:1,\
                 SwordsmanArchetypeLegs:1,SwordsmanArchetypeFeet:1,1hSwordT2_FTUE:1,\
                 SwordsmanArchetype_2hGreatsword:0,NoSuchItem:1"
                    .to_owned(),
            )],
        }
    }

    #[test]
    fn the_login_kit_is_the_quantity_one_items_and_the_bag_the_rest() {
        let kits = StarterKits::build(source());
        let kit = kits.kit(BACKSTORY).expect("kit");
        let summary: Vec<(&str, u32, Option<EquipSlot>)> = kit
            .iter()
            .map(|item| (item.item_id.as_str(), item.item_index, item.slot))
            .collect();
        // Common sorts before Quest; the housing row takes no index.
        assert_eq!(
            summary,
            vec![
                ("SwordsmanArchetypeChest", 5, Some(EquipSlot::Chest)),
                ("SwordsmanArchetypeHands", 6, Some(EquipSlot::Hands)),
                ("SwordsmanArchetypeLegs", 7, Some(EquipSlot::Legs)),
                ("SwordsmanArchetypeFeet", 8, Some(EquipSlot::Feet)),
                ("1hSwordT2_FTUE", 2, Some(EquipSlot::MainHand)),
            ]
        );
        let bag: Vec<&str> = kits
            .bag(BACKSTORY)
            .iter()
            .map(|item| item.item_id.as_str())
            .collect();
        assert_eq!(
            bag,
            ["SwordsmanArchetypeHead", "SwordsmanArchetype_2hGreatsword"]
        );
        assert!(kits.bag("Archetype_Nobody").is_empty());
        let potion = kits.item("PotionHealthT1").expect("potion");
        assert_eq!((potion.item_index, potion.slot), (1, None));
        let sword = &kit[4];
        assert_eq!(
            (sword.gear_score, sword.durability, sword.bound),
            (SWORD_GEAR_SCORE, SWORD_DURABILITY, true)
        );
        assert_eq!(kits.kit("Archetype_Nobody"), None);
    }

    #[test]
    fn item_classes_give_worn_slots() {
        assert_eq!(
            EquipSlot::of_item_class("Weapon+EquippableTwoHand+GreatSword+Melee"),
            Some(EquipSlot::MainHand)
        );
        assert_eq!(
            EquipSlot::of_item_class("Weapon+EquippableOffHand+RoundShield"),
            Some(EquipSlot::OffHand)
        );
        assert_eq!(EquipSlot::of_item_class("Consumable+Potion"), None);
    }
}
