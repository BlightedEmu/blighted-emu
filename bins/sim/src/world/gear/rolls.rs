// SPDX-License-Identifier: AGPL-3.0-or-later
//! Items that come with rolls of their own (run-15 items2 study): a crafted gear item at the
//! gear score the player picked and a dropped gear item, each with its rolled perks. Each such
//! item is one new stack: two items of different rolls never share a stack.

use nw_content::item_rolls::RolledItem;
use nw_content::starter::{KitItem, StarterKits};

use super::{Gear, MAX_STACK_QUANTITY};

impl Gear {
    /// Puts one new stack of `quantity` of `item` with the gear score and perks of `rolled` into
    /// the bag; whether it fit.
    pub(in crate::world) fn add_rolled(
        &mut self,
        kits: &StarterKits,
        item: &KitItem,
        quantity: u32,
        rolled: &RolledItem,
    ) -> bool {
        if quantity == 0 {
            return false;
        }
        let mut stack = self.new_stack(item, quantity.min(MAX_STACK_QUANTITY));
        stack.item.gear_score = rolled.gear_score;
        stack.set_perks(kits, rolled.perks.clone());
        let added = self.put_in_bag(stack);
        if added {
            self.changed = true;
            self.unsaved = true;
        }
        added
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{BACKSTORY, CHARACTER, FLINT, kits};
    use super::*;

    const ROLLED_GEAR_SCORE: u32 = 425;
    const PERK: &str = "PerkID_Weapon_DmgCrit";

    #[test]
    fn a_rolled_item_is_a_stack_of_its_own_with_its_rolls() {
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        let flint = kits.item(FLINT).expect("flint").clone();
        let rolled = RolledItem {
            gear_score: ROLLED_GEAR_SCORE,
            perks: vec![PERK.to_owned()],
        };
        assert!(gear.add_rolled(&kits, &flint, 1, &rolled));
        assert!(gear.add_rolled(&kits, &flint, 1, &rolled));
        assert!(!gear.add_rolled(&kits, &flint, 0, &rolled));
        let stacks: Vec<_> = gear
            .bag
            .iter()
            .flatten()
            .filter(|stack| stack.item_id == FLINT)
            .collect();
        assert_eq!(stacks.len(), 2, "rolled items do not stack");
        assert!(stacks.iter().all(|stack| stack.item.gear_score == ROLLED_GEAR_SCORE
            && stack.perks == [PERK]));
        assert_ne!(stacks[0].item.instance, stacks[1].item.instance);
        assert_eq!(gear.take_unsaved().map(|items| items.len() > 1), Some(true));
    }
}
