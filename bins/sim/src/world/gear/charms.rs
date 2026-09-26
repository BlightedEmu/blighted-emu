// SPDX-License-Identifier: AGPL-3.0-or-later
//! A gem or a charm of the bag put into a perk slot of an item (6983, run-15 items2 study).
//!
//! Retail MSQ15 (9 of 9 requests): the named slot of the item's perks held the empty perk of
//! the gem's socket; it took the gem's bucket perk that fits the item (a weapon got the gambit
//! gem perk, an armor piece the ward perk) in that same slot, the other slots kept their places
//! (no new sort), one gem left the bag stack, and nothing else changed: no coins, no counter,
//! gear score or durability.

use nw_content::charms::{GEM_PERK_TYPE, empty_perk};
use nw_content::item_rolls::{ItemRolls, SOCKET_GEM};
use nw_content::starter::StarterKits;
use nw_messages::item_calls::{LOCATION_BAG, LOCATION_PAPERDOLL_SLOT};
use nw_messages::wire::RAW_ID_LEN;

use super::{Gear, Stack};

/// An instance id that names no stack: the reference names it by its place alone (every
/// retail 6983 body).
const NO_INSTANCE: [u8; RAW_ID_LEN] = [0; RAW_ID_LEN];

/// What a slotted gem or charm gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::world) struct SlottedCharm {
    /// The item that took it.
    pub(in crate::world) item_id: String,
    /// The perk its slot took.
    pub(in crate::world) perk_id: String,
}

/// Why a gem or charm was not slotted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) enum CharmRefusal {
    /// The named item is not there.
    NoSuchItem,
    /// The named bag slot holds no gem or charm (no socket type or bucket), or it is locked.
    NoCharm,
    /// The perk slot does not exist or does not take this gem or charm.
    WrongSlot,
    /// No perk of the bucket fits the item.
    NoFittingPerk,
}

/// A place a gem's or charm's target is named by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) struct CharmTarget {
    /// [`LOCATION_PAPERDOLL_SLOT`] or [`LOCATION_BAG`].
    pub(in crate::world) location: u32,
    /// The paperdoll slot or the bag slot.
    pub(in crate::world) slot: u32,
    /// The instance id; all zero names the place alone.
    pub(in crate::world) instance: [u8; RAW_ID_LEN],
}

impl Gear {
    /// The stack `target` names.
    fn charm_target(&self, target: CharmTarget) -> Option<&Stack> {
        let stack = match target.location {
            LOCATION_PAPERDOLL_SLOT => self
                .worn
                .iter()
                .find(|(slot_id, stack)| *slot_id == target.slot && stack.is_some())
                .and_then(|(_, stack)| stack.as_ref()),
            LOCATION_BAG => self
                .bag
                .get(usize::try_from(target.slot).ok()?)
                .and_then(Option::as_ref),
            _ => None,
        }?;
        (target.instance == NO_INSTANCE || stack.item.instance == target.instance).then_some(stack)
    }

    /// The stack `target` names, to change.
    fn charm_target_mut(&mut self, target: CharmTarget) -> Option<&mut Stack> {
        let stack = match target.location {
            LOCATION_PAPERDOLL_SLOT => self
                .worn
                .iter_mut()
                .find(|(slot_id, stack)| *slot_id == target.slot && stack.is_some())
                .and_then(|(_, stack)| stack.as_mut()),
            LOCATION_BAG => self
                .bag
                .get_mut(usize::try_from(target.slot).ok()?)
                .and_then(Option::as_mut),
            _ => None,
        }?;
        (target.instance == NO_INSTANCE || stack.item.instance == target.instance).then_some(stack)
    }

    /// Puts one gem or charm of bag slot `charm_slot` (instance `charm_instance`, all zero for
    /// any) into perk slot `perk_index` of the item `target` names (6983): the slot must hold
    /// the empty perk of the charm's socket type, or a gem perk when the charm is a gem and the
    /// item `CanReplaceGem` (best guess: the old gem is gone, as no capture replaced one). The
    /// slot takes a perk of the charm's bucket that fits the item and no other perk of it
    /// clashes with ([`nw_content::perks::PerkTable::pick`]); one charm leaves the bag. `below(n)`
    /// is a random number below `n`.
    #[allow(clippy::too_many_arguments, reason = "one request's fields")]
    pub(in crate::world) fn slot_charm(
        &mut self,
        kits: &StarterKits,
        rolls: &ItemRolls,
        target: CharmTarget,
        charm_slot: u32,
        charm_instance: [u8; RAW_ID_LEN],
        perk_index: u32,
        below: &mut impl FnMut(u32) -> u32,
    ) -> Result<SlottedCharm, CharmRefusal> {
        let (charm_id, _) = self.bag_item(charm_slot).ok_or(CharmRefusal::NoCharm)?;
        let charm_stack = self
            .bag
            .get(usize::try_from(charm_slot).map_err(|_| CharmRefusal::NoCharm)?)
            .and_then(Option::as_ref)
            .ok_or(CharmRefusal::NoCharm)?;
        if charm_stack.item.locked
            || (charm_instance != NO_INSTANCE && charm_stack.item.instance != charm_instance)
        {
            return Err(CharmRefusal::NoCharm);
        }
        let charm = rolls.item(charm_id).ok_or(CharmRefusal::NoCharm)?;
        let socket = charm.socket_type.as_deref().ok_or(CharmRefusal::NoCharm)?;
        let bucket = charm.resource_bucket.clone().ok_or(CharmRefusal::NoCharm)?;
        let empty = empty_perk(socket).ok_or(CharmRefusal::NoCharm)?;
        let stack = self.charm_target(target).ok_or(CharmRefusal::NoSuchItem)?;
        let item_id = stack.item_id.clone();
        let row = rolls.item(&item_id).ok_or(CharmRefusal::NoSuchItem)?;
        let index = usize::try_from(perk_index).map_err(|_| CharmRefusal::WrongSlot)?;
        let mut perks = stack.perks.clone();
        let current = perks.get(index).ok_or(CharmRefusal::WrongSlot)?;
        let replaces_gem = socket.eq_ignore_ascii_case(SOCKET_GEM)
            && row.can_replace_gem
            && rolls
                .perks
                .perk(current)
                .is_some_and(|perk| perk.perk_type.eq_ignore_ascii_case(GEM_PERK_TYPE));
        if !current.eq_ignore_ascii_case(empty) && !replaces_gem {
            return Err(CharmRefusal::WrongSlot);
        }
        let held: Vec<String> = perks
            .iter()
            .enumerate()
            .filter(|(at, _)| *at != index)
            .map(|(_, perk_id)| perk_id.clone())
            .collect();
        let perk_id = rolls
            .perks
            .pick(&bucket, &item_id, &row.classes, &held, below)
            .ok_or(CharmRefusal::NoFittingPerk)?;
        if !self.take_from_slot(charm_slot, 1) {
            return Err(CharmRefusal::NoCharm);
        }
        perks[index].clone_from(&perk_id);
        let stack = self
            .charm_target_mut(target)
            .ok_or(CharmRefusal::NoSuchItem)?;
        stack.set_perks(kits, perks);
        self.changed = true;
        self.unsaved = true;
        Ok(SlottedCharm { item_id, perk_id })
    }
}

#[cfg(test)]
mod tests {
    use nw_content::item_rolls::ItemRollRow;
    use nw_content::perks::{Perk, PerkBucketRow, PerkSheet, PerkSource, PerkTable, parts};
    use nw_messages::paperdoll::slot;

    use super::super::tests::{BACKSTORY, CHARACTER, SWORD, kits_with};
    use super::*;

    const GEM: &str = "OpalCutT4";
    const GEM_BUCKET: &str = "OpalCutT4";
    const EMPTY_GEM: &str = "PerkID_Gem_EmptyGemSlot";
    const WEAPON_GEM: &str = "PerkID_Gem_GambitGem4";
    const ARMOR_GEM: &str = "PerkID_Gem_ElementalWard4";
    const SWORD_PERK: &str = "PerkID_Weapon_DmgCrit";
    const GEMS: u32 = 2;

    fn perk(perk_id: &str, perk_type: &str, classes: &str) -> Perk {
        Perk {
            perk_id: perk_id.to_owned(),
            perk_type: perk_type.to_owned(),
            item_classes: parts(Some(classes)),
            excluded_classes: Vec::new(),
            labels: Vec::new(),
            display_name: Some(format!("@{perk_id}_Name")),
        }
    }

    fn rolls(can_replace_gem: bool) -> ItemRolls {
        let table = PerkTable::build(PerkSource {
            sheets: vec![PerkSheet {
                unique_id: "ItemPerks_Gems".to_owned(),
                perks: vec![
                    perk(EMPTY_GEM, "Gem", "EquippableMainHand+Armor"),
                    perk(WEAPON_GEM, "Gem", "EquippableMainHand"),
                    perk(ARMOR_GEM, "Gem", "Armor"),
                    perk(SWORD_PERK, "Generated", "EquippableMainHand"),
                ],
            }],
            buckets: vec![PerkBucketRow {
                id: GEM_BUCKET.to_owned(),
                chance: Some(1.0),
                entries: vec![ARMOR_GEM.to_owned(), WEAPON_GEM.to_owned()],
                ..PerkBucketRow::default()
            }],
            ..PerkSource::default()
        });
        ItemRolls::build(
            [
                ItemRollRow {
                    item_id: SWORD.to_owned(),
                    classes: parts(Some("Weapon+EquippableMainHand+Sword+Melee")),
                    can_replace_gem,
                    ..ItemRollRow::default()
                },
                ItemRollRow {
                    item_id: GEM.to_owned(),
                    classes: parts(Some("Resource")),
                    resource_bucket: Some(GEM_BUCKET.to_owned()),
                    socket_type: Some(SOCKET_GEM.to_owned()),
                    ..ItemRollRow::default()
                },
            ],
            table,
        )
    }

    /// The kit's gear with gems in the bag and the sword's perks `perks`: the gems' bag slot.
    fn gear_with_gems(perks: &[&str]) -> (Gear, StarterKits, u32) {
        let kits = kits_with(&[(GEM, "Resource")]);
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        assert!(gear.add(kits.item(GEM).expect("gem"), GEMS));
        let sword = gear
            .worn
            .iter_mut()
            .find(|(slot_id, _)| *slot_id == slot::MAIN_HAND_OPTION_1)
            .and_then(|(_, stack)| stack.as_mut())
            .expect("sword");
        sword.set_perks(&kits, perks.iter().map(|perk| (*perk).to_owned()).collect());
        let gem_slot = gear
            .bag_stacks()
            .iter()
            .find(|(_, item_id, _, _)| item_id == GEM)
            .map(|(bag_slot, ..)| *bag_slot)
            .expect("gems");
        (gear, kits, gem_slot)
    }

    fn sword_target() -> CharmTarget {
        CharmTarget {
            location: LOCATION_PAPERDOLL_SLOT,
            slot: slot::MAIN_HAND_OPTION_1,
            instance: NO_INSTANCE,
        }
    }

    fn sword_perks(gear: &Gear) -> Vec<String> {
        gear.charm_target(sword_target())
            .expect("sword")
            .perks
            .clone()
    }

    /// Slots a gem of bag slot `gem_slot` into perk slot `index` of `target`.
    fn try_slot(
        gear: &mut Gear,
        kits: &StarterKits,
        rolls: &ItemRolls,
        target: CharmTarget,
        gem_slot: u32,
        index: u32,
    ) -> Result<SlottedCharm, CharmRefusal> {
        gear.slot_charm(
            kits,
            rolls,
            target,
            gem_slot,
            NO_INSTANCE,
            index,
            &mut |_: u32| 0_u32,
        )
    }

    #[test]
    fn a_gem_fills_the_empty_slot_in_place_with_the_perk_that_fits() {
        let (mut gear, kits, gem_slot) = gear_with_gems(&[SWORD_PERK, EMPTY_GEM]);
        gear.take_unsaved();
        let slotted =
            try_slot(&mut gear, &kits, &rolls(false), sword_target(), gem_slot, 1).expect("slot");
        assert_eq!(slotted.item_id, SWORD);
        assert_eq!(slotted.perk_id, WEAPON_GEM);
        assert_eq!(sword_perks(&gear), [SWORD_PERK, WEAPON_GEM], "no new sort");
        assert_eq!(gear.bag_count(GEM), GEMS - 1);
        assert!(gear.take_unsaved().is_some(), "saved");
    }

    #[test]
    fn a_gem_needs_an_empty_slot_or_a_replaceable_gem() {
        let (mut gear, kits, gem_slot) = gear_with_gems(&[SWORD_PERK, WEAPON_GEM]);
        let fixed = rolls(false);
        for (index, why) in [
            (0, "a perk that is no socket"),
            (1, "a gem the item may not replace"),
            (4, "no such perk slot"),
        ] {
            assert_eq!(
                try_slot(&mut gear, &kits, &fixed, sword_target(), gem_slot, index),
                Err(CharmRefusal::WrongSlot),
                "{why}"
            );
        }
        assert_eq!(gear.bag_count(GEM), GEMS, "nothing taken");
        let replaceable = rolls(true);
        let replaced = try_slot(&mut gear, &kits, &replaceable, sword_target(), gem_slot, 1);
        assert_eq!(
            replaced.map(|slotted| slotted.perk_id),
            Ok(WEAPON_GEM.to_owned())
        );
        assert_eq!(gear.bag_count(GEM), GEMS - 1);
        let staff = CharmTarget {
            slot: slot::MAIN_HAND_OPTION_1 + 1,
            ..sword_target()
        };
        assert_eq!(
            try_slot(&mut gear, &kits, &replaceable, staff, gem_slot, 1),
            Err(CharmRefusal::NoSuchItem)
        );
    }
}
