// SPDX-License-Identifier: AGPL-3.0-or-later
//! A player's items: what it wears and what is in its bag, sent as the gear states of its own
//! player. A login wears the kit of its archetype pick (starter-kit note); the tutorial then
//! takes the sword, fills the bag and equips from it (live-tutorial note, section 5). There is
//! no item store yet, so every login starts from the kit again.

use nw_bus::{CarriedItem, ItemPlace};
use nw_content::starter::{EquipSlot, KitItem, StarterKits};
use nw_messages::paperdoll::slot;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{EquippedItem, GearItem, PlayerGear, STARTER_BAG_SLOTS, is_visible_slot};
use uuid::Uuid;

/// The version of the first gear states (retail value).
const FIRST_GEAR_VERSION: u64 = 3;

/// The paperdoll slots an item of `kind` may go into; an item that is not worn goes into a
/// slot that does not show (amulet, ring, quick slots).
fn fits(kind: Option<EquipSlot>, slot_id: u32) -> bool {
    match kind {
        Some(EquipSlot::Head) => slot_id == slot::HEAD,
        Some(EquipSlot::Chest) => slot_id == slot::CHEST,
        Some(EquipSlot::Hands) => slot_id == slot::HANDS,
        Some(EquipSlot::Legs) => slot_id == slot::LEGS,
        Some(EquipSlot::Feet) => slot_id == slot::FEET,
        Some(EquipSlot::MainHand) => {
            (slot::MAIN_HAND_OPTION_1..=slot::MAIN_HAND_OPTION_3).contains(&slot_id)
        }
        Some(EquipSlot::OffHand) => {
            (slot::OFF_HAND_OPTION_1..=slot::OFF_HAND_OPTION_3).contains(&slot_id)
        }
        None => !is_visible_slot(slot_id),
    }
}

/// The first worn slot of `kind`.
const fn first_slot(kind: EquipSlot) -> u32 {
    match kind {
        EquipSlot::Head => slot::HEAD,
        EquipSlot::Chest => slot::CHEST,
        EquipSlot::Hands => slot::HANDS,
        EquipSlot::Legs => slot::LEGS,
        EquipSlot::Feet => slot::FEET,
        EquipSlot::MainHand => slot::MAIN_HAND_OPTION_1,
        EquipSlot::OffHand => slot::OFF_HAND_OPTION_1,
    }
}

/// One item stack of a player.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stack {
    item_id: String,
    kind: Option<EquipSlot>,
    item: GearItem,
}

/// Why an equip request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EquipRefusal {
    /// The bag slot is empty or holds another item.
    NoSuchItem,
    /// The item does not go into that paperdoll slot.
    WrongSlot,
    /// The worn item has no free bag slot to go to.
    BagFull,
}

/// What a player wears and carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Gear {
    character: Uuid,
    /// Worn stacks in the order they were put on.
    worn: Vec<(u32, Stack)>,
    /// One entry per bag slot.
    bag: Vec<Option<Stack>>,
    /// The version of the last states sent.
    version: u64,
    /// Whether the states changed since they were sent.
    changed: bool,
}

impl Gear {
    /// The gear of `character` with the backstory `backstory`: its login kit worn and an empty
    /// bag; `None` without a backstory or a kit of it.
    pub(super) fn starter(
        kits: &StarterKits,
        character: Uuid,
        backstory: Option<&str>,
    ) -> Option<Self> {
        let kit = kits.kit(backstory?)?;
        let worn = kit
            .iter()
            .filter_map(|item| Some((first_slot(item.slot?), stack(character, item, 1))))
            .collect();
        Some(Self {
            character,
            worn,
            bag: vec![None; STARTER_BAG_SLOTS],
            version: FIRST_GEAR_VERSION,
            changed: false,
        })
    }

    /// The gear of `character` from the `items` another sim sent; an item `kits` does not know
    /// is left out, and one whose bag slot is taken or out of range goes to a free one.
    pub(super) fn carried_in(kits: &StarterKits, character: Uuid, items: &[CarriedItem]) -> Self {
        let mut gear = Self {
            character,
            worn: Vec::new(),
            bag: vec![None; STARTER_BAG_SLOTS],
            version: FIRST_GEAR_VERSION,
            changed: false,
        };
        for carried in items {
            let Some(item) = kits.item(&carried.item_id) else {
                continue;
            };
            let stack = stack(character, item, carried.quantity);
            match carried.place {
                ItemPlace::Worn(slot_id) => gear.worn.push((slot_id, stack)),
                ItemPlace::Bag(bag_slot) => {
                    let wanted = gear
                        .bag
                        .get_mut(usize::from(bag_slot))
                        .filter(|entry| entry.is_none());
                    match wanted {
                        Some(entry) => *entry = Some(stack),
                        None => {
                            if let Some(free) = gear.bag.iter_mut().find(|entry| entry.is_none()) {
                                *free = Some(stack);
                            }
                        }
                    }
                }
            }
        }
        gear
    }

    /// The items to carry to another sim.
    pub(super) fn carried(&self) -> Vec<CarriedItem> {
        let worn = self.worn.iter().map(|(slot_id, stack)| CarriedItem {
            item_id: stack.item_id.clone(),
            quantity: stack.item.quantity,
            place: ItemPlace::Worn(*slot_id),
        });
        let bag = (0_u16..).zip(&self.bag).filter_map(|(bag_slot, entry)| {
            let stack = entry.as_ref()?;
            Some(CarriedItem {
                item_id: stack.item_id.clone(),
                quantity: stack.item.quantity,
                place: ItemPlace::Bag(bag_slot),
            })
        });
        worn.chain(bag).collect()
    }

    /// The whole gear states.
    pub(super) fn states(&self) -> PlayerGear {
        let equipped: Vec<EquippedItem> = self
            .worn
            .iter()
            .map(|(slot_id, stack)| EquippedItem {
                slot: *slot_id,
                item: stack.item,
            })
            .collect();
        let bag: Vec<Option<GearItem>> = self
            .bag
            .iter()
            .map(|stack| stack.as_ref().map(|stack| stack.item))
            .collect();
        PlayerGear::of(&equipped, &bag, self.version)
    }

    /// Every stack of `item` becomes one new stack of `quantity` worn in paperdoll slot
    /// `slot_id`.
    pub(super) fn renew(&mut self, item: &KitItem, quantity: u32, slot_id: u32) {
        self.remove(&item.item_id);
        self.worn.retain(|(worn, _)| *worn != slot_id);
        self.worn
            .push((slot_id, stack(self.character, item, quantity)));
        self.changed = true;
    }

    /// Uses one of the stack worn in paperdoll slot `slot_id`: its item id; `None` when the slot
    /// is empty. The last one empties the slot.
    pub(super) fn use_one(&mut self, slot_id: u32) -> Option<String> {
        let at = self.worn.iter().position(|(worn, _)| *worn == slot_id)?;
        let stack = &mut self.worn[at].1;
        let item_id = stack.item_id.clone();
        stack.item.quantity = stack.item.quantity.saturating_sub(1);
        if stack.item.quantity == 0 {
            self.worn.remove(at);
        }
        self.changed = true;
        Some(item_id)
    }

    /// The item id worn in paperdoll slot `slot_id`.
    pub(super) fn worn_item(&self, slot_id: u32) -> Option<&str> {
        self.worn
            .iter()
            .find(|(worn, _)| *worn == slot_id)
            .map(|(_, stack)| stack.item_id.as_str())
    }

    /// The weapon in the hand: the worn main-hand item of the lowest weapon option slot (Inferred:
    /// retail's first island hits are the slot 24 weapon's), with its gear score.
    pub(super) fn main_weapon(&self) -> Option<(&str, u32)> {
        self.worn
            .iter()
            .filter(|(slot_id, _)| {
                (slot::MAIN_HAND_OPTION_1..=slot::MAIN_HAND_OPTION_3).contains(slot_id)
            })
            .min_by_key(|(slot_id, _)| *slot_id)
            .map(|(_, stack)| (stack.item_id.as_str(), stack.item.gear_score))
    }

    /// The states after a change, at a new version, once; `None` without a change.
    pub(super) fn take_change(&mut self) -> Option<PlayerGear> {
        if !self.changed {
            return None;
        }
        self.changed = false;
        self.version += 1;
        Some(self.states())
    }

    /// Removes every stack of `item_id`, worn or in the bag; whether there was one.
    pub(super) fn remove(&mut self, item_id: &str) -> bool {
        let before = self.worn.len();
        self.worn.retain(|(_, stack)| stack.item_id != item_id);
        let mut removed = self.worn.len() != before;
        for entry in &mut self.bag {
            if entry.as_ref().is_some_and(|stack| stack.item_id == item_id) {
                *entry = None;
                removed = true;
            }
        }
        self.changed |= removed;
        removed
    }

    /// Puts `quantity` of `item` into the bag: onto a stack of it, else into the first free
    /// slot; whether it fit.
    pub(super) fn add(&mut self, item: &KitItem, quantity: u32) -> bool {
        if let Some(stack) = self
            .bag
            .iter_mut()
            .flatten()
            .find(|stack| stack.item_id == item.item_id)
        {
            stack.item.quantity = stack.item.quantity.saturating_add(quantity);
            self.changed = true;
            return true;
        }
        let Some(free) = self.bag.iter_mut().find(|entry| entry.is_none()) else {
            return false;
        };
        *free = Some(stack(self.character, item, quantity));
        self.changed = true;
        true
    }

    /// Equips `quantity` of the item of instance `instance` in bag slot `bag_slot` into paperdoll
    /// slot `slot_id`; an item worn there goes back to the bag. The id of the equipped item.
    pub(super) fn equip(
        &mut self,
        bag_slot: u32,
        slot_id: u32,
        quantity: u32,
        instance: [u8; RAW_ID_LEN],
    ) -> Result<String, EquipRefusal> {
        let bag_index = usize::try_from(bag_slot).map_err(|_| EquipRefusal::NoSuchItem)?;
        let stack = self
            .bag
            .get(bag_index)
            .and_then(Option::as_ref)
            .filter(|stack| stack.item.instance == instance)
            .ok_or(EquipRefusal::NoSuchItem)?;
        if !fits(stack.kind, slot_id) {
            return Err(EquipRefusal::WrongSlot);
        }
        let moved = quantity.clamp(1, stack.item.quantity);
        let whole = moved == stack.item.quantity;
        let replaced = self.worn.iter().position(|(worn, _)| *worn == slot_id);
        if replaced.is_some() && !whole && !self.bag.iter().any(Option::is_none) {
            return Err(EquipRefusal::BagFull);
        }
        let mut put_on = if whole {
            self.bag[bag_index].take().ok_or(EquipRefusal::NoSuchItem)?
        } else {
            let left = self.bag[bag_index]
                .as_mut()
                .ok_or(EquipRefusal::NoSuchItem)?;
            left.item.quantity -= moved;
            left.clone()
        };
        put_on.item.quantity = moved;
        let item_id = put_on.item_id.clone();
        if let Some(index) = replaced {
            let (_, taken_off) = self.worn.remove(index);
            let free = if whole {
                Some(bag_index)
            } else {
                self.bag.iter().position(Option::is_none)
            };
            if let Some(free) = free {
                self.bag[free] = Some(taken_off);
            }
        }
        self.worn.push((slot_id, put_on));
        self.changed = true;
        Ok(item_id)
    }
}

/// A stack of `quantity` of `item` for `character`.
fn stack(character: Uuid, item: &KitItem, quantity: u32) -> Stack {
    Stack {
        item_id: item.item_id.clone(),
        kind: item.slot,
        item: GearItem {
            item_index: item.item_index,
            quantity,
            gear_score: item.gear_score,
            durability: item.durability,
            bound: item.bound,
            instance: instance_id(character, &item.item_id),
        },
    }
}

/// The instance id of `item_id` for `character`: derived from both, so it stays the same over
/// logins, handoffs, and container moves (the beach bag's records carry these ids).
pub(super) fn instance_id(character: Uuid, item_id: &str) -> [u8; RAW_ID_LEN] {
    *Uuid::new_v5(&character, item_id.as_bytes()).as_bytes()
}

#[cfg(test)]
pub(crate) mod tests {
    use nw_content::starter::{ItemRow, StarterSource};

    use super::*;

    pub(crate) const BACKSTORY: &str = "Archetype_Mystic";
    const CHARACTER: Uuid = Uuid::from_u128(0x2b87_684d_c82c_46a4_90e4_c764_0247_7507);
    const SWORD: &str = "1hSwordT2_FTUE";
    const STAFF: &str = "MysticArchetype_2hStaffLife";
    const POTION: &str = "PotionHealthT1";
    const POTION_SLOT: u32 = 14;
    const POTIONS: u32 = 2;

    /// A kit of [`BACKSTORY`]: a chest and the sword at login, the staff in the bag.
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
                    row(SWORD, "Weapon+EquippableMainHand+Sword+Melee"),
                    row(STAFF, "Weapon+EquippableTwoHand+LifeStaff+Magic"),
                    row(POTION, "Consumable+Potion"),
                ],
            )],
            backstories: vec![(
                BACKSTORY.to_owned(),
                format!("MysticArchetypeChest:1,{SWORD}:1,{STAFF}:0"),
            )],
        })
    }

    fn visible_slots(gear: &PlayerGear) -> Vec<u32> {
        gear.paperdoll
            .public
            .as_ref()
            .and_then(|public| public.visible_slots.clone())
            .expect("slots")
            .elements
    }

    fn instance(item_id: &str) -> [u8; RAW_ID_LEN] {
        *Uuid::new_v5(&CHARACTER, item_id.as_bytes()).as_bytes()
    }

    #[test]
    fn the_kit_is_worn_with_stable_instance_ids() {
        let kits = kits();
        let gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        let states = gear.states();
        assert_eq!(
            visible_slots(&states),
            vec![slot::CHEST, slot::MAIN_HAND_OPTION_1]
        );
        let again = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        assert_eq!(again.states(), states, "the same ids at the next login");
        assert_eq!(Gear::starter(&kits, CHARACTER, None), None);
        assert_eq!(
            Gear::starter(&kits, CHARACTER, Some("Archetype_Nobody")),
            None
        );
    }

    #[test]
    fn the_tutorial_takes_the_sword_and_equips_from_the_bag() {
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        assert_eq!(gear.take_change(), None);
        assert!(gear.remove(SWORD));
        assert!(!gear.remove(SWORD));
        let after = gear.take_change().expect("change");
        assert_eq!(visible_slots(&after), vec![slot::CHEST]);
        for item in kits.bag(BACKSTORY) {
            assert!(gear.add(item, 1));
        }
        let potion = kits.item(POTION).expect("potion");
        assert!(gear.add(potion, 1));
        assert!(gear.add(potion, 1), "onto the same stack");
        assert_eq!(
            gear.equip(0, slot::CHEST, 1, instance(STAFF)),
            Err(EquipRefusal::WrongSlot)
        );
        assert_eq!(
            gear.equip(0, slot::MAIN_HAND_OPTION_1, 1, instance(POTION)),
            Err(EquipRefusal::NoSuchItem)
        );
        assert_eq!(
            gear.equip(0, slot::MAIN_HAND_OPTION_1, 1, instance(STAFF)),
            Ok(STAFF.to_owned())
        );
        assert_eq!(
            gear.equip(1, POTION_SLOT, POTIONS, instance(POTION)),
            Ok(POTION.to_owned())
        );
        let states = gear.take_change().expect("change");
        assert_eq!(
            visible_slots(&states),
            vec![slot::CHEST, slot::MAIN_HAND_OPTION_1]
        );
        let hidden = states
            .paperdoll
            .owner
            .as_ref()
            .and_then(|owner| owner.hidden_items.clone())
            .expect("hidden")
            .elements;
        assert_eq!(hidden[0].quantity, POTIONS);
        assert!(
            gear.bag.iter().all(Option::is_none),
            "the bag is empty again"
        );
        assert_eq!(
            gear.main_weapon(),
            Some((STAFF, 75)),
            "the staff in the hand"
        );
        // Drinking: one potion less per use; the last one empties the slot.
        assert_eq!(gear.worn_item(POTION_SLOT), Some(POTION));
        assert_eq!(gear.use_one(POTION_SLOT), Some(POTION.to_owned()));
        assert!(gear.take_change().is_some());
        assert_eq!(gear.use_one(POTION_SLOT), Some(POTION.to_owned()));
        assert_eq!(gear.worn_item(POTION_SLOT), None);
        assert_eq!(gear.use_one(POTION_SLOT), None);
        // The quest's end: a fresh stack of potions in the quick slot.
        gear.renew(potion, POTIONS, POTION_SLOT);
        assert_eq!(gear.worn_item(POTION_SLOT), Some(POTION));
        assert_eq!(gear.use_one(POTION_SLOT), Some(POTION.to_owned()));
        assert_eq!(gear.worn_item(POTION_SLOT), Some(POTION), "one left");
    }

    #[test]
    fn carried_items_come_back_the_same() {
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        for item in kits.bag(BACKSTORY) {
            gear.add(item, 1);
        }
        let mut carried = gear.carried();
        let again = Gear::carried_in(&kits, CHARACTER, &carried);
        assert_eq!(again.states(), gear.states());
        carried.push(CarriedItem {
            item_id: "NoSuchItem".to_owned(),
            quantity: 1,
            place: ItemPlace::Bag(0),
        });
        let potion = CarriedItem {
            item_id: POTION.to_owned(),
            quantity: 1,
            place: ItemPlace::Bag(0),
        };
        carried.push(potion);
        let moved = Gear::carried_in(&kits, CHARACTER, &carried);
        assert_eq!(
            moved.bag[1].as_ref().map(|stack| stack.item_id.as_str()),
            Some(POTION),
            "a taken bag slot sends it to a free one"
        );
    }

    #[test]
    fn a_worn_item_goes_back_to_the_bag_slot() {
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        for item in kits.bag(BACKSTORY) {
            gear.add(item, 1);
        }
        assert_eq!(
            gear.equip(0, slot::MAIN_HAND_OPTION_1, 1, instance(STAFF)),
            Ok(STAFF.to_owned())
        );
        let back = gear.bag[0].as_ref().expect("the sword is in the bag");
        assert_eq!(back.item_id, SWORD);
    }
}
