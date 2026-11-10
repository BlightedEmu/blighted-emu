// SPDX-License-Identifier: AGPL-3.0-or-later
//! The UI item class numbers of the "by class" item calls (run-15 items2 study: 2079
//! `RepairAllItemClass`, 3938 `DropInventoryItemsByClass`, 2946 `GiveItemsByClass`, 2945
//! `TakeItemsByClass`) and the `MasterItemDefinitions.UiItemClass` names they stand for.
//!
//! **Observed.** Matched against the `UiItemClass` of the items each retail call moved:
//! 74 `UI_Consumable`, 86 `UI_Alchemy`, 90 `UI_Refining`, 92 `UI_Smelting`,
//! 94 `UI_Leatherworking`, 95 `UI_Weaving`; 71 `UI_Weapon` and 72 `UI_Armor` (live).
//!
//! The other numbers are from the client (build 25810808). It registers each `eItemClass` name
//! with a function of its own, and those functions lie in the order of the numbers, one every
//! 1120 bytes from `eItemClass_FoodForHorse` to `eItemClass_LootContainer` (`UI_Weapon` at
//! 0x144c7e1e0, `UI_Dye` at 0x144c81200, `UI_Alchemy` at 0x144c82380). That order gives all 8
//! observed numbers. It is not the order of the client's name strings, which has `UI_Cooking`,
//! `UI_Furniture`, `UI_OutpostRush` and `UI_Alchemy` before `UI_Bait` and `UI_Dye`: a table in
//! string order left the "move all" and "take all" of 75 to 89 without a class (live
//! 2026-11-10: the dye section sent 82, the cooking section 83, and nothing moved).
//! `eItemClass_UI_Catacombs` has no function in that run; its number is Unknown.

/// The `UiItemClass` names in the order of their numbers, from [`FIRST_NUMBER`].
const UI_NAMES: [&str; 34] = [
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
    "UI_Bait",
    "UI_Dye",
    "UI_Cooking",
    "UI_Furniture",
    "UI_OutpostRush",
    "UI_Alchemy",
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
];
/// The number of the first UI class (`eItemClass_UI_Weapon`).
const FIRST_NUMBER: u64 = 71;

/// The `UiItemClass` name of wire number `number`; `None` for a number no class is known for.
pub fn ui_item_class(number: u64) -> Option<&'static str> {
    let index = number.checked_sub(FIRST_NUMBER)?;
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
            assert_eq!(ui_item_class(number), Some(name), "retail {number}");
        }
        for (number, name) in [
            (75, "UI_CraftingModifiers"),
            (82, "UI_Dye"),
            (83, "UI_Cooking"),
            (98, "UI_BasicFood"),
            (104, "UI_PatternItem"),
        ] {
            assert_eq!(ui_item_class(number), Some(name), "client {number}");
        }
        assert_eq!(ui_item_class(70), None, "below the UI classes");
        assert_eq!(ui_item_class(105), None, "above the UI classes");
        assert!(is_of_ui_class(Some("ui_weapon"), 71));
        assert!(!is_of_ui_class(None, 71));
    }
}
