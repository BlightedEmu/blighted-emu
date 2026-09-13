// SPDX-License-Identifier: AGPL-3.0-or-later
//! A player's gear: the login kit of its archetype pick (starter-kit note), as the gear states
//! of its own spawn card. There is no item store yet, so every login wears the kit.

use nw_content::starter::{EquipSlot, StarterKits};
use nw_messages::paperdoll::slot;
use nw_messages::{EquippedItem, PlayerGear};
use uuid::Uuid;

/// The paperdoll slot id of `slot`.
const fn paperdoll_slot(slot: EquipSlot) -> u32 {
    match slot {
        EquipSlot::Head => slot::HEAD,
        EquipSlot::Chest => slot::CHEST,
        EquipSlot::Hands => slot::HANDS,
        EquipSlot::Legs => slot::LEGS,
        EquipSlot::Feet => slot::FEET,
        EquipSlot::MainHand => slot::MAIN_HAND_OPTION_1,
        EquipSlot::OffHand => slot::OFF_HAND_OPTION_1,
    }
}

/// The gear of `character` with the backstory `backstory`; `None` without a backstory or a kit
/// of it. Each item's instance id is derived from the character and the item, so it stays the
/// same over logins and handoffs.
pub(super) fn starter_gear(
    kits: &StarterKits,
    character: Uuid,
    backstory: Option<&str>,
) -> Option<PlayerGear> {
    let kit = kits.kit(backstory?)?;
    let equipped: Vec<EquippedItem> = kit
        .iter()
        .map(|item| EquippedItem {
            slot: paperdoll_slot(item.slot),
            item_index: item.item_index,
            gear_score: item.gear_score,
            durability: item.durability,
            bound: item.bound,
            instance: *Uuid::new_v5(&character, item.item_id.as_bytes()).as_bytes(),
        })
        .collect();
    Some(PlayerGear::first(&equipped))
}

#[cfg(test)]
pub(crate) mod tests {
    use nw_content::starter::{ItemRow, StarterSource};

    use super::*;

    pub(crate) const BACKSTORY: &str = "Archetype_Mystic";
    const CHARACTER: Uuid = Uuid::from_u128(0x2b87_684d_c82c_46a4_90e4_c764_0247_7507);

    /// A two-item kit of [`BACKSTORY`].
    pub(crate) fn kits() -> StarterKits {
        let row = |item_id: &str, item_class: &str| ItemRow {
            item_id: item_id.to_owned(),
            item_class: Some(item_class.to_owned()),
            gear_score: Some(75),
            durability: Some(375),
            bind_on_pickup: true,
            ..ItemRow::default()
        };
        StarterKits::build(StarterSource {
            item_sheets: vec![(
                "MasterItemDefinitions_Quest".to_owned(),
                vec![
                    row("MysticArchetypeChest", "EquippableChest+Armor+Light"),
                    row("1hSwordT2_FTUE", "Weapon+EquippableMainHand+Sword+Melee"),
                ],
            )],
            backstories: vec![(
                BACKSTORY.to_owned(),
                "MysticArchetypeChest:1,1hSwordT2_FTUE:1".to_owned(),
            )],
        })
    }

    #[test]
    fn the_kit_is_worn_with_stable_instance_ids() {
        let kits = kits();
        let gear = starter_gear(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        let public = gear.paperdoll.public.as_ref().expect("public");
        assert_eq!(
            public
                .visible_slots
                .as_ref()
                .map(|list| list.elements.clone()),
            Some(vec![slot::CHEST, slot::MAIN_HAND_OPTION_1])
        );
        let items = |gear: &PlayerGear| {
            gear.paperdoll
                .owner
                .as_ref()
                .and_then(|owner| owner.visible_items.clone())
                .expect("items")
                .elements
        };
        let first = items(&gear);
        assert_ne!(first[0].instance, first[1].instance);
        let again = starter_gear(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        assert_eq!(items(&again), first, "the same ids at the next login");
        assert_eq!(starter_gear(&kits, CHARACTER, None), None);
        assert_eq!(
            starter_gear(&kits, CHARACTER, Some("Archetype_Nobody")),
            None
        );
    }
}
