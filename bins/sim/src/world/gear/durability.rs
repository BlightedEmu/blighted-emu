// SPDX-License-Identifier: AGPL-3.0-or-later
//! The durability and gear score of a player's items (run-15 items2 study): the wear of a
//! death, a gather or a hit, the repair of the items a call names (3003, 3007, 2079), and the
//! gear score upgrade of one item (3008). The costs are the caller's
//! ([`crate::world::repair`]); these only read and change the stacks.

use nw_content::item_repair::death_loss;
use nw_content::item_rolls::ItemRolls;
use nw_messages::wire::RAW_ID_LEN;

use super::{Gear, Stack};

/// The items a repair call names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) enum RepairPick {
    /// One item by its reference (3003): location, slot or key, instance id.
    One {
        /// [`nw_messages::item_calls::LOCATION_BAG`] or a key location.
        location: u32,
        /// The bag slot or the owned-items key.
        slot: u32,
        /// The instance id; all zero names the item by its place alone.
        instance: [u8; RAW_ID_LEN],
    },
    /// Every worn item (3007).
    Worn,
    /// Every worn and bag item of one UI item class (2079), as the caller's filter decides.
    ItemClass,
}

/// One damaged item a repair takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::world) struct Damaged {
    /// Its item id.
    pub(in crate::world) item_id: String,
    /// Its durability.
    pub(in crate::world) durability: u32,
}

/// Why an upgrade (3008) is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) enum UpgradeRefusal {
    /// The reference names no item.
    NoSuchItem,
    /// The item's gear score is not the one the call starts from.
    WrongGearScore,
}

impl Stack {
    /// Whether its item has durability at all.
    const fn max_durability_known(&self) -> bool {
        self.item.max_durability > 0
    }

    /// Whether it has durability and lost some.
    const fn is_damaged(&self) -> bool {
        self.item.max_durability > 0 && self.item.durability < self.item.max_durability
    }

    /// Takes `amount` of its durability, down to 0 (a broken item stays usable: the client
    /// switch `javelin.disable-broken-equipment` is 0); whether it changed.
    fn wear(&mut self, amount: u32) -> bool {
        let worn = self.item.durability.saturating_sub(amount);
        let changed = worn != self.item.durability;
        self.item.durability = worn;
        changed
    }
}

impl Gear {
    /// Marks a change of an item's own values: the client and the next save get it.
    fn item_changed(&mut self) {
        self.changed = true;
        self.unsaved = true;
    }

    /// Every worn item loses what a death takes of it (`DurabilityDmgOnDeath` of its full
    /// durability, less in PvP); unworn items lose nothing (`javelin-server.disable-unequipped-
    /// items-take-durability-on-death` defaults to 1). Whether an item changed.
    pub(in crate::world) fn wear_at_death(&mut self, rolls: &ItemRolls, pvp: bool) -> bool {
        let mut changed = false;
        for (_, stack) in &mut self.worn {
            let Some(stack) = stack.as_mut() else {
                continue;
            };
            let Some(row) = rolls.item(&stack.item_id) else {
                continue;
            };
            changed |= stack.wear(death_loss(row, pvp));
        }
        if changed {
            self.item_changed();
        }
        changed
    }

    /// The main-hand option slot whose weapon is in the hand.
    pub(in crate::world) fn held_slot(&self) -> Option<u32> {
        self.held_option()
    }

    /// The item worn in paperdoll slot `slot_id` loses `amount` of its durability; whether it
    /// changed.
    pub(in crate::world) fn wear_slot(&mut self, slot_id: u32, amount: u32) -> bool {
        let changed = self
            .worn
            .iter_mut()
            .filter(|(worn, _)| *worn == slot_id)
            .filter_map(|(_, stack)| stack.as_mut())
            .any(|stack| stack.max_durability_known() && stack.wear(amount));
        if changed {
            self.item_changed();
        }
        changed
    }

    /// The stacks `pick` names whose item `in_class` takes (only [`RepairPick::ItemClass`] asks
    /// it); a reference to no item names none.
    fn picked_mut(
        &mut self,
        pick: RepairPick,
        in_class: &impl Fn(&str) -> bool,
    ) -> Vec<&mut Stack> {
        match pick {
            RepairPick::One {
                location,
                slot,
                instance,
            } => self
                .referenced_mut(location, slot, instance)
                .map_or_else(|_| Vec::new(), |stack| vec![stack]),
            RepairPick::Worn => self
                .worn
                .iter_mut()
                .filter_map(|(_, stack)| stack.as_mut())
                .collect(),
            RepairPick::ItemClass => self
                .worn
                .iter_mut()
                .filter_map(|(_, stack)| stack.as_mut())
                .chain(self.bag.iter_mut().flatten())
                .filter(|stack| in_class(&stack.item_id))
                .collect(),
        }
    }

    /// The damaged items `pick` names, as a repair would take them.
    pub(in crate::world) fn damaged(
        &mut self,
        pick: RepairPick,
        in_class: impl Fn(&str) -> bool,
    ) -> Vec<Damaged> {
        self.picked_mut(pick, &in_class)
            .into_iter()
            .filter(|stack| stack.is_damaged())
            .map(|stack| Damaged {
                item_id: stack.item_id.clone(),
                durability: stack.item.durability,
            })
            .collect()
    }

    /// Repairs the damaged items `pick` names whose item `repairable` allows to their full
    /// durability ("Repair Max Durability Cost" 0: the full durability stays); how many.
    pub(in crate::world) fn repair(
        &mut self,
        pick: RepairPick,
        in_class: impl Fn(&str) -> bool,
        repairable: impl Fn(&str) -> bool,
    ) -> usize {
        let mut repaired = 0;
        for stack in self.picked_mut(pick, &in_class) {
            if stack.is_damaged() && repairable(&stack.item_id) {
                stack.item.durability = stack.item.max_durability;
                repaired += 1;
            }
        }
        if repaired > 0 {
            self.item_changed();
        }
        repaired
    }

    /// The item id of the item an upgrade from gear score `from` names (3008): a bag slot or an
    /// owned-items key, whose gear score must be `from`.
    pub(in crate::world) fn upgrade_target(
        &mut self,
        location: u32,
        slot: u32,
        instance: [u8; RAW_ID_LEN],
        from: u32,
    ) -> Result<String, UpgradeRefusal> {
        let stack = self
            .referenced_mut(location, slot, instance)
            .map_err(|_| UpgradeRefusal::NoSuchItem)?;
        if stack.item.gear_score != from {
            return Err(UpgradeRefusal::WrongGearScore);
        }
        Ok(stack.item_id.clone())
    }

    /// Sets the gear score of the item an upgrade names to `to`; durability, perks, counter and
    /// flags stay (retail: only the gear score of the record changed).
    pub(in crate::world) fn set_gear_score(
        &mut self,
        location: u32,
        slot: u32,
        instance: [u8; RAW_ID_LEN],
        to: u32,
    ) -> Result<(), UpgradeRefusal> {
        let stack = self
            .referenced_mut(location, slot, instance)
            .map_err(|_| UpgradeRefusal::NoSuchItem)?;
        stack.item.gear_score = to;
        self.item_changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nw_content::item_rolls::ItemRollRow;
    use nw_content::perks::PerkTable;
    use nw_messages::item_calls::LOCATION_BAG;
    use nw_messages::paperdoll::slot;

    use super::super::tests::{BACKSTORY, CHARACTER, FLINT, SWORD, kits};
    use super::*;

    const SWORD_DURABILITY: u32 = 375;
    const DEATH_SHARE: f64 = 0.12;

    fn rolls() -> ItemRolls {
        ItemRolls::build(
            [ItemRollRow {
                item_id: SWORD.to_owned(),
                durability: SWORD_DURABILITY,
                durability_loss_on_death: DEATH_SHARE,
                repairable: true,
                ..ItemRollRow::default()
            }],
            PerkTable::default(),
        )
    }

    fn gear() -> Gear {
        Gear::starter(&kits(), CHARACTER, Some(BACKSTORY)).expect("gear")
    }

    fn sword(gear: &Gear) -> &Stack {
        gear.worn
            .iter()
            .filter_map(|(_, stack)| stack.as_ref())
            .find(|stack| stack.item_id == SWORD)
            .expect("sword")
    }

    #[test]
    fn a_death_wears_the_worn_items_and_a_repair_makes_them_whole() {
        let mut gear = gear();
        assert!(gear.wear_at_death(&rolls(), false));
        // ceil(375 × 0.12) = 45.
        assert_eq!(sword(&gear).item.durability, SWORD_DURABILITY - 45);
        assert_eq!(sword(&gear).item.max_durability, SWORD_DURABILITY);
        assert!(gear.take_unsaved().is_some(), "the wear is saved");
        let damaged = gear.damaged(RepairPick::Worn, |_| true);
        assert_eq!(
            damaged,
            [Damaged {
                item_id: SWORD.to_owned(),
                durability: SWORD_DURABILITY - 45
            }]
        );
        assert_eq!(gear.repair(RepairPick::Worn, |_| true, |_| false), 0);
        assert_eq!(gear.repair(RepairPick::Worn, |_| true, |_| true), 1);
        assert_eq!(sword(&gear).item.durability, SWORD_DURABILITY);
        assert!(gear.damaged(RepairPick::Worn, |_| true).is_empty());
    }

    #[test]
    fn a_tool_slot_wears_by_its_cost_and_stops_at_zero() {
        let mut gear = gear();
        assert!(gear.wear_slot(slot::MAIN_HAND_OPTION_1, SWORD_DURABILITY * 2));
        assert_eq!(sword(&gear).item.durability, 0);
        assert!(
            !gear.wear_slot(slot::MAIN_HAND_OPTION_1, 1),
            "already broken"
        );
        assert!(!gear.wear_slot(slot::HEAD, 1), "an empty slot");
    }

    #[test]
    fn a_class_repair_takes_only_its_class() {
        let mut gear = gear();
        gear.wear_at_death(&rolls(), true);
        assert!(
            gear.damaged(RepairPick::ItemClass, |item| item == FLINT)
                .is_empty()
        );
        assert_eq!(
            gear.damaged(RepairPick::ItemClass, |item| item == SWORD)
                .len(),
            1
        );
    }

    #[test]
    fn an_upgrade_names_its_item_by_its_gear_score() {
        let mut gear = gear();
        let flint = kits().item(FLINT).expect("flint").clone();
        assert!(gear.add(&flint, 1));
        let (bag_slot, gear_score, instance) = gear
            .bag
            .iter()
            .enumerate()
            .find_map(|(at, stack)| {
                let stack = stack.as_ref()?;
                (stack.item_id == FLINT).then_some((at, stack.item.gear_score, stack.item.instance))
            })
            .expect("flint in the bag");
        let bag_slot = u32::try_from(bag_slot).expect("slot");
        assert_eq!(
            gear.upgrade_target(LOCATION_BAG, bag_slot, instance, gear_score + 1),
            Err(UpgradeRefusal::WrongGearScore)
        );
        assert_eq!(
            gear.upgrade_target(LOCATION_BAG, bag_slot, instance, gear_score),
            Ok(FLINT.to_owned())
        );
        assert_eq!(
            gear.set_gear_score(LOCATION_BAG, bag_slot, instance, gear_score + 1),
            Ok(())
        );
        assert_eq!(
            gear.upgrade_target(LOCATION_BAG, bag_slot + 1, instance, gear_score + 1),
            Err(UpgradeRefusal::NoSuchItem)
        );
    }
}
