// SPDX-License-Identifier: AGPL-3.0-or-later
//! The storage search items (7048) that go with each change of a player's items.
//!
//! Retail sends one 7048 with no search and no items before the bundle of a change of the
//! player's items (tutorial-quest note, section 10). Newchar-retail-2026-09-18 (106 sends):
//! 5 around the first card, whose gear is five worn starter items; one within 30 ms after each
//! equip (11 of 11 `ServerEquipItem`) and each quest turn-in (6 of 6
//! `RequestCompleteObjective`); 2 after the take-all of a loot bag (2939) and 3 after the end of
//! a chest's use (3746). Inferred: one per item whose stacks changed. Which items the two loot
//! cases changed is not in the decoded members, so whether their counts follow that rule is
//! Unknown.

use std::collections::BTreeMap;

use nw_bus::{CarriedItem, ItemPlace};

/// The stacks of one item: each place and quantity, in order.
type Stacks = Vec<(ItemPlace, u32)>;

/// The items a player held at its last change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ItemChanges {
    held: BTreeMap<String, Stacks>,
}

impl ItemChanges {
    /// How many items changed from the last call to `carried`: an item counts once when it
    /// came, went, moved or changed quantity. The first call counts every item.
    pub(super) fn changes(&mut self, carried: &[CarriedItem]) -> usize {
        let held = stacks_by_item(carried);
        let changed = held
            .iter()
            .filter(|(item, stacks)| self.held.get(*item) != Some(*stacks))
            .count()
            + self
                .held
                .keys()
                .filter(|item| !held.contains_key(*item))
                .count();
        self.held = held;
        changed
    }
}

/// The stacks of `carried` by item id, each item's stacks in place order.
fn stacks_by_item(carried: &[CarriedItem]) -> BTreeMap<String, Stacks> {
    let mut held: BTreeMap<String, Stacks> = BTreeMap::new();
    for item in carried {
        held.entry(item.item_id.clone())
            .or_default()
            .push((item.place, item.quantity));
    }
    for stacks in held.values_mut() {
        stacks.sort_unstable_by_key(|(place, quantity)| (place_order(*place), *quantity));
    }
    held
}

/// A sort key of `place`: worn slots, then bag slots.
fn place_order(place: ItemPlace) -> (u8, u32) {
    match place {
        ItemPlace::Worn(slot) => (0, slot),
        ItemPlace::Bag(slot) => (1, u32::from(slot)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SWORD: &str = "1hSwordT2_FTUE";
    const CHEST: &str = "SwordsmanArchetypeChest";
    const POTION: &str = "PotionHealthT1";
    const MAIN_HAND: u32 = 24;
    const CHEST_SLOT: u32 = 1;

    fn item(item_id: &str, quantity: u32, place: ItemPlace) -> CarriedItem {
        CarriedItem {
            item_id: item_id.to_owned(),
            quantity,
            place,
        }
    }

    #[test]
    fn the_first_call_counts_every_item() {
        let mut changes = ItemChanges::default();
        let worn = [
            item(SWORD, 1, ItemPlace::Worn(MAIN_HAND)),
            item(CHEST, 1, ItemPlace::Worn(CHEST_SLOT)),
        ];
        assert_eq!(changes.changes(&worn), 2);
        assert_eq!(changes.changes(&worn), 0, "nothing changed");
    }

    #[test]
    fn a_move_a_gain_a_quantity_and_a_removal_count_once_each() {
        let mut changes = ItemChanges::default();
        changes.changes(&[
            item(SWORD, 1, ItemPlace::Bag(0)),
            item(POTION, 1, ItemPlace::Bag(1)),
        ]);
        assert_eq!(
            changes.changes(&[
                item(SWORD, 1, ItemPlace::Worn(MAIN_HAND)),
                item(POTION, 1, ItemPlace::Bag(1)),
            ]),
            1,
            "an equip moves one item"
        );
        assert_eq!(
            changes.changes(&[
                item(SWORD, 1, ItemPlace::Worn(MAIN_HAND)),
                item(POTION, 2, ItemPlace::Bag(1)),
                item(CHEST, 1, ItemPlace::Bag(2)),
            ]),
            2,
            "a new quantity and a new item"
        );
        assert_eq!(
            changes.changes(&[
                item(POTION, 2, ItemPlace::Bag(1)),
                item(CHEST, 1, ItemPlace::Bag(2)),
            ]),
            1,
            "a removal"
        );
    }
}
