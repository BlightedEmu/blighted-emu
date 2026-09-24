// SPDX-License-Identifier: AGPL-3.0-or-later
//! The UI item class numbers of the "by class" item calls (run-15 items2 study: 2079
//! `RepairAllItemClass`, 3938 `DropInventoryItemsByClass`, 2946 `GiveItemsByClass`, 2945
//! `TakeItemsByClass`) and the `MasterItemDefinitions.UiItemClass` names they stand for.
//!
//! **Observed.** Matched against the `UiItemClass` of the items each retail call moved:
//! 74 `UI_Consumable`, 86 `UI_Alchemy`, 90 `UI_Refining`, 92 `UI_Smelting`,
//! 94 `UI_Leatherworking`, 95 `UI_Weaving`. In the client's `eItemClass` name table
//! (`eItemClass_UI_Weapon` at index 63 to `eItemClass_UI_Catacombs` at 97) a number is its
//! name's index plus [`NAME_INDEX_OFFSET`] for every match but `UI_Alchemy` (index 76, number
//! 86). **Best guess:** the offset holds up to [`LAST_LOW_NUMBER`] and from
//! [`FIRST_HIGH_NUMBER`] on (71 `UI_Weapon` and 72 `UI_Armor` follow it); the numbers between
//! are not known except 86, and match no class (needs live check).

/// The `UiItemClass` names in the client's `eItemClass` order, from index [`FIRST_UI_INDEX`].
const UI_NAMES: [&str; 35] = [
    "UI_Weapon",
    "UI_Armor",
    "UI_Ammo",
    "UI_Consumable",
    "UI_CraftingModifiers",
    "UI_Tools",
    "UI_Material",
    "UI_Augments",
    "UI_Lore",
    "UI_RepairKit",
    "UI_Cooking",
    "UI_Furniture",
    "UI_OutpostRush",
    "UI_Alchemy",
    "UI_Bait",
    "UI_Dye",
    "UI_TuningOrbs",
    "UI_Quest",
    "UI_JewelCrafting",
    "UI_Refining",
    "UI_AttributeFood",
    "UI_Smelting",
    "UI_TradeSkillFood",
    "UI_Leatherworking",
    "UI_Weaving",
    "UI_Woodworking",
    "UI_Stonecutting",
    "UI_BasicFood",
    "UI_Rewards",
    "UI_Instruments",
    "UI_PvPArenas",
    "UI_Runes",
    "UI_MountItem",
    "UI_PatternItem",
    "UI_Catacombs",
];
/// The `eItemClass` index of the first UI class (`eItemClass_UI_Weapon`).
const FIRST_UI_INDEX: u64 = 63;
/// A wire number is its name's index plus this (Observed in 7 of 8 matches).
const NAME_INDEX_OFFSET: u64 = 8;
/// The highest number below the unsettled range.
const LAST_LOW_NUMBER: u64 = 74;
/// The lowest number above the unsettled range.
const FIRST_HIGH_NUMBER: u64 = 90;
/// The one number of the unsettled range that a retail call settled.
const ALCHEMY_NUMBER: u64 = 86;
/// The class [`ALCHEMY_NUMBER`] names.
const ALCHEMY_NAME: &str = "UI_Alchemy";

/// The `UiItemClass` name of wire number `number`; `None` for a number no class is known for.
pub fn ui_item_class(number: u64) -> Option<&'static str> {
    if number == ALCHEMY_NUMBER {
        return Some(ALCHEMY_NAME);
    }
    if number > LAST_LOW_NUMBER && number < FIRST_HIGH_NUMBER {
        return None;
    }
    let index = number.checked_sub(NAME_INDEX_OFFSET + FIRST_UI_INDEX)?;
    UI_NAMES.get(usize::try_from(index).ok()?).copied()
}

/// Whether an item whose `UiItemClass` cell is `cell` is of wire class `number` (any case: the
/// datasheet writes `UI_Jewelcrafting`, the client `UI_JewelCrafting`).
pub fn is_of_ui_class(cell: Option<&str>, number: u64) -> bool {
    match (cell, ui_item_class(number)) {
        (Some(cell), Some(name)) => cell.trim().eq_ignore_ascii_case(name),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retail_numbers_name_their_classes() {
        for (number, name) in [
            (71, "UI_Weapon"),
            (72, "UI_Armor"),
            (74, "UI_Consumable"),
            (86, "UI_Alchemy"),
            (90, "UI_Refining"),
            (92, "UI_Smelting"),
            (94, "UI_Leatherworking"),
            (95, "UI_Weaving"),
        ] {
            assert_eq!(ui_item_class(number), Some(name), "{number}");
        }
        assert_eq!(ui_item_class(80), None, "unsettled");
        assert_eq!(ui_item_class(70), None, "below the UI classes");
        assert_eq!(ui_item_class(200), None);
        assert!(is_of_ui_class(Some("ui_weapon"), 71));
        assert!(!is_of_ui_class(None, 71));
    }
}
