// SPDX-License-Identifier: AGPL-3.0-or-later
//! A player's items: what it wears and what is in its bag, sent as the gear states of its own
//! player. A login wears the kit of its archetype pick (starter-kit note); the tutorial then
//! takes the sword, fills the bag and equips from it (live-tutorial note, section 5). Outside the
//! tutorial the items are saved (admin studio S5), and a login starts from the saved items.

use nw_bus::{CarriedItem, ItemPlace};
use nw_content::starter::{EquipSlot, KitItem, StarterKits};
use nw_messages::paperdoll::slot;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    EquippedItem, GearItem, Hands, LoadedAmmo, PlayerGear, STARTER_BAG_SLOTS, is_visible_slot,
};
use uuid::Uuid;

/// The version of the first gear states (retail value).
const FIRST_GEAR_VERSION: u64 = 3;
/// How long a drawn weapon takes to reach the hand (retail 331 to 418 ms after the key).
const ATTACH_MS: u64 = 331;
/// The main-hand options whose ammo the paperdoll carries (owner children 8 and 9).
const AMMO_OPTIONS: usize = 2;
/// Milliseconds from 1970-01-01 to 2000-01-01, the server clock's start.
const UNIX_MS_AT_2000: u64 = 946_684_800_000;
/// The largest stack quantity: the world database stores it as an `INTEGER`.
pub(crate) const MAX_STACK_QUANTITY: u32 = i32::MAX as u32;
/// Nanoseconds per millisecond.
const NS_PER_MS: u64 = 1_000_000;

/// The ns since 1970-01-01 at server time `server_ms` (ms since 2000-01-01): the time of a
/// loaded-ammo state (client-gates note: retail Unix time in ns).
pub(super) const fn unix_ns(server_ms: u64) -> u64 {
    server_ms
        .saturating_add(UNIX_MS_AT_2000)
        .saturating_mul(NS_PER_MS)
}

/// The ammo of a main-hand option slot.
fn ammo_index(slot_id: u32) -> Option<usize> {
    let index = usize::try_from(slot_id.checked_sub(slot::MAIN_HAND_OPTION_1)?).ok()?;
    (index < AMMO_OPTIONS).then_some(index)
}

/// The shots of one main-hand option.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Ammo {
    /// The shots its weapon holds (0 for a melee weapon).
    max: u8,
    loaded: u8,
    /// How long its weapon takes to load again.
    reload_ms: u64,
    /// When the empty weapon is loaded again.
    reload_at_ms: Option<u64>,
    /// When the option was last put away (ns since 1970-01-01).
    time_ns: u64,
}

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
    /// A slot's item is not the one the request names.
    NotWorn,
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
    /// Whether the items changed since they were last taken for a save.
    unsaved: bool,
    /// The main-hand option slot drawn; `None`: every weapon is sheathed.
    drawn: Option<u32>,
    /// The drawn weapon reaches the hand at this time.
    attach_at_ms: Option<u64>,
    /// The drawn weapon is in the hand.
    attached: bool,
    /// The ammo of main-hand options 1 and 2.
    ammo: [Ammo; AMMO_OPTIONS],
    /// The items whose ammo [`Gear::arm`] last set, per option.
    armed: [Option<String>; AMMO_OPTIONS],
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
            unsaved: false,
            drawn: None,
            attach_at_ms: None,
            attached: false,
            ammo: [Ammo::default(); AMMO_OPTIONS],
            armed: [None, None],
        })
    }

    /// The gear of `character` from the `items` another sim sent, with main-hand option slot
    /// `drawn` in the hand; an item `kits` does not know is left out, and one whose bag slot is
    /// taken or out of range goes to a free one.
    pub(super) fn carried_in(
        kits: &StarterKits,
        character: Uuid,
        items: &[CarriedItem],
        drawn: Option<u32>,
    ) -> Self {
        let mut gear = Self {
            character,
            worn: Vec::new(),
            bag: vec![None; STARTER_BAG_SLOTS],
            version: FIRST_GEAR_VERSION,
            changed: false,
            unsaved: false,
            drawn,
            attach_at_ms: None,
            attached: drawn.is_some(),
            ammo: [Ammo::default(); AMMO_OPTIONS],
            armed: [None, None],
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

    /// The items to save when they changed since the last call; `None` when they did not.
    pub(super) fn take_unsaved(&mut self) -> Option<Vec<CarriedItem>> {
        if !self.unsaved {
            return None;
        }
        self.unsaved = false;
        Some(self.carried())
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
        PlayerGear::of(&equipped, &bag, self.version, self.hands())
    }

    /// Every stack of `item` becomes one new stack of `quantity` worn in paperdoll slot
    /// `slot_id`.
    pub(super) fn renew(&mut self, item: &KitItem, quantity: u32, slot_id: u32) {
        self.remove(&item.item_id);
        self.worn.retain(|(worn, _)| *worn != slot_id);
        self.worn
            .push((slot_id, stack(self.character, item, quantity)));
        self.changed = true;
        self.unsaved = true;
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
        self.unsaved = true;
        Some(item_id)
    }

    /// The item id worn in paperdoll slot `slot_id`.
    pub(super) fn worn_item(&self, slot_id: u32) -> Option<&str> {
        self.worn
            .iter()
            .find(|(worn, _)| *worn == slot_id)
            .map(|(_, stack)| stack.item_id.as_str())
    }

    /// The weapon in the hand, with its gear score: the item of the drawn option slot, else the
    /// worn main-hand item of the lowest option slot (Inferred: retail's first island hits are
    /// the slot 24 weapon's).
    pub(super) fn main_weapon(&self) -> Option<(&str, u32)> {
        let held = self.held_option()?;
        self.main_hand()
            .find(|(slot_id, _)| *slot_id == held)
            .map(|(_, stack)| (stack.item_id.as_str(), stack.item.gear_score))
    }

    /// The main-hand option slot in the hand: the drawn one when it holds a weapon, else the
    /// lowest one that does.
    fn held_option(&self) -> Option<u32> {
        self.main_hand()
            .find(|(slot_id, _)| Some(*slot_id) == self.drawn)
            .or_else(|| self.main_hand().min_by_key(|(slot_id, _)| *slot_id))
            .map(|(slot_id, _)| slot_id)
    }

    /// The worn items of the main-hand option slots.
    fn main_hand(&self) -> impl Iterator<Item = (u32, &Stack)> {
        self.worn
            .iter()
            .filter(|(slot_id, _)| {
                (slot::MAIN_HAND_OPTION_1..=slot::MAIN_HAND_OPTION_3).contains(slot_id)
            })
            .map(|(slot_id, stack)| (*slot_id, stack))
    }

    /// The main-hand option slot drawn, when one is.
    pub(super) const fn drawn(&self) -> Option<u32> {
        self.drawn
    }

    /// The player draws main-hand option `option` (0 for the first key) at `now_ms`: its weapon
    /// reaches the hand [`ATTACH_MS`] later, and the option put away keeps the time `time_ns`
    /// (client-gates note: retail answered each weapon key so).
    pub(super) fn draw(&mut self, option: u32, now_ms: u64, time_ns: u64) {
        let picked = slot::MAIN_HAND_OPTION_1.saturating_add(option);
        if picked > slot::MAIN_HAND_OPTION_3 || self.drawn == Some(picked) {
            return;
        }
        if let Some(stowed) = self.drawn.and_then(ammo_index) {
            self.ammo[stowed].time_ns = time_ns;
        }
        self.drawn = Some(picked);
        self.attached = false;
        self.attach_at_ms = Some(now_ms.saturating_add(ATTACH_MS));
        self.changed = true;
    }

    /// Gives main-hand options 1 and 2 the shots and reload time of their weapons by
    /// `weapon_of(item)`, at time `time_ns`: a new weapon in an option comes loaded.
    pub(super) fn arm(&mut self, weapon_of: impl Fn(&str) -> Option<(u8, u64)>, time_ns: u64) {
        for (index, slot_id) in (slot::MAIN_HAND_OPTION_1..).take(AMMO_OPTIONS).enumerate() {
            let item = self.worn_item(slot_id);
            if item == self.armed[index].as_deref() {
                continue;
            }
            let (max, reload_ms) = item.and_then(&weapon_of).unwrap_or_default();
            self.armed[index] = item.map(str::to_owned);
            let ammo = &mut self.ammo[index];
            if (ammo.max, ammo.reload_ms) != (max, reload_ms) {
                *ammo = Ammo {
                    max,
                    loaded: max,
                    reload_ms,
                    reload_at_ms: None,
                    time_ns,
                };
                self.changed = true;
            }
        }
    }

    /// Fires one shot of the drawn option at `now_ms`; whether one was loaded. The last shot
    /// starts the reload (retail: loaded again 1.0 to 5 s after the second shot).
    pub(super) fn fire(&mut self, now_ms: u64) -> bool {
        let Some(index) = self.held_option().and_then(ammo_index) else {
            return false;
        };
        let ammo = &mut self.ammo[index];
        if ammo.loaded == 0 {
            return false;
        }
        ammo.loaded -= 1;
        if ammo.loaded == 0 {
            ammo.reload_at_ms = Some(now_ms.saturating_add(ammo.reload_ms));
        }
        self.changed = true;
        true
    }

    /// Moves the hands on to `now_ms`: the drawn weapon reaches the hand, an empty weapon is
    /// loaded again.
    pub(super) fn tick_hands(&mut self, now_ms: u64) {
        if self.attach_at_ms.is_some_and(|at| now_ms >= at) {
            self.attach_at_ms = None;
            self.attached = true;
            self.changed = true;
        }
        for ammo in &mut self.ammo {
            if ammo.reload_at_ms.is_some_and(|at| now_ms >= at) {
                ammo.reload_at_ms = None;
                ammo.loaded = ammo.max;
                self.changed = true;
            }
        }
    }

    /// How the player holds its weapons, for the paperdoll.
    fn hands(&self) -> Hands {
        Hands {
            drawn: self.drawn,
            attached: self.attached,
            ammo: self.ammo.map(|ammo| {
                Some(LoadedAmmo {
                    count: ammo.loaded,
                    time_ns: ammo.time_ns,
                })
            }),
        }
    }

    /// Swaps the worn items of paperdoll slots `slot_a` and `slot_b`, which hold the items of
    /// instance `instance_a` and `instance_b` (all zero: the slot is empty); each must fit the
    /// other slot.
    pub(super) fn swap(
        &mut self,
        slot_a: u32,
        slot_b: u32,
        instance_a: [u8; RAW_ID_LEN],
        instance_b: [u8; RAW_ID_LEN],
    ) -> Result<(), EquipRefusal> {
        let holds = |gear: &Self, slot_id: u32, instance: [u8; RAW_ID_LEN]| match gear
            .worn
            .iter()
            .find(|(worn, _)| *worn == slot_id)
        {
            Some((_, stack)) => stack.item.instance == instance,
            None => instance == [0; RAW_ID_LEN],
        };
        if !holds(self, slot_a, instance_a) || !holds(self, slot_b, instance_b) {
            return Err(EquipRefusal::NotWorn);
        }
        let fits_other = self.worn.iter().all(|(slot_id, stack)| match *slot_id {
            id if id == slot_a => fits(stack.kind, slot_b),
            id if id == slot_b => fits(stack.kind, slot_a),
            _ => true,
        });
        if !fits_other {
            return Err(EquipRefusal::WrongSlot);
        }
        for (slot_id, _) in &mut self.worn {
            if *slot_id == slot_a {
                *slot_id = slot_b;
            } else if *slot_id == slot_b {
                *slot_id = slot_a;
            }
        }
        self.changed = true;
        self.unsaved = true;
        Ok(())
    }

    /// Takes off the worn item of paperdoll slot `slot_id` into bag slot `bag_slot`, or the first
    /// free one when that is taken or out of range (`u32::MAX`: any).
    pub(super) fn unequip(&mut self, slot_id: u32, bag_slot: u32) -> Result<(), EquipRefusal> {
        let worn = self
            .worn
            .iter()
            .position(|(worn, _)| *worn == slot_id)
            .ok_or(EquipRefusal::NotWorn)?;
        let wanted = usize::try_from(bag_slot)
            .ok()
            .filter(|index| self.bag.get(*index).is_some_and(Option::is_none));
        let free = wanted
            .or_else(|| self.bag.iter().position(Option::is_none))
            .ok_or(EquipRefusal::BagFull)?;
        let (_, stack) = self.worn.remove(worn);
        self.bag[free] = Some(stack);
        self.changed = true;
        self.unsaved = true;
        Ok(())
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
        self.unsaved |= removed;
        removed
    }

    /// Puts `quantity` of `item` into the bag: onto a stack of it, else into the first free
    /// slot; whether it fit.
    pub(super) fn add(&mut self, item: &KitItem, quantity: u32) -> bool {
        if quantity == 0 {
            return false;
        }
        let quantity = quantity.min(MAX_STACK_QUANTITY);
        if let Some(stack) = self
            .bag
            .iter_mut()
            .flatten()
            .find(|stack| stack.item_id == item.item_id)
        {
            stack.item.quantity = stack
                .item
                .quantity
                .saturating_add(quantity)
                .min(MAX_STACK_QUANTITY);
            self.changed = true;
            self.unsaved = true;
            return true;
        }
        let Some(free) = self.bag.iter_mut().find(|entry| entry.is_none()) else {
            return false;
        };
        *free = Some(stack(self.character, item, quantity));
        self.changed = true;
        self.unsaved = true;
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
        self.unsaved = true;
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
    pub(crate) const CHARACTER: Uuid = Uuid::from_u128(0x2b87_684d_c82c_46a4_90e4_c764_0247_7507);
    pub(crate) const SWORD: &str = "1hSwordT2_FTUE";
    const STAFF: &str = "MysticArchetype_2hStaffLife";
    pub(crate) const POTION: &str = "PotionHealthT1";
    /// A resource the kit does not give: the bag takes it from a gather.
    pub(crate) const FLINT: &str = "FlintT1";
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
                    row(FLINT, "Resource"),
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
        let again = Gear::carried_in(&kits, CHARACTER, &carried, None);
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
        let moved = Gear::carried_in(&kits, CHARACTER, &carried, None);
        assert_eq!(
            moved.bag[1].as_ref().map(|stack| stack.item_id.as_str()),
            Some(POTION),
            "a taken bag slot sends it to a free one"
        );
    }

    #[test]
    fn a_change_of_items_is_taken_for_a_save_once() {
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        assert_eq!(gear.take_unsaved(), None, "a new kit has nothing to save");
        gear.draw(slot::MAIN_HAND_OPTION_1, 0, 0);
        assert_eq!(
            gear.take_unsaved(),
            None,
            "drawing a weapon changes no item"
        );
        let flint = kits.item(FLINT).expect("flint").clone();
        assert!(gear.add(&flint, 1));
        assert_eq!(gear.take_unsaved(), Some(gear.carried()));
        assert_eq!(gear.take_unsaved(), None);
        assert!(gear.remove(FLINT));
        assert!(gear.take_unsaved().is_some());
    }

    #[test]
    fn a_stack_holds_one_to_the_largest_stored_quantity() {
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        let flint = kits.item(FLINT).expect("flint").clone();
        assert!(!gear.add(&flint, 0), "no stack of nothing");
        assert!(gear.add(&flint, MAX_STACK_QUANTITY));
        assert!(gear.add(&flint, 1));
        let held: u32 = gear
            .carried()
            .iter()
            .filter(|stack| stack.item_id == FLINT)
            .map(|stack| stack.quantity)
            .sum();
        assert_eq!(
            held, MAX_STACK_QUANTITY,
            "the database stores at most i32::MAX"
        );
    }

    #[test]
    fn the_drawn_weapon_goes_with_the_items_to_another_sim() {
        const SECOND_OPTION: u32 = slot::MAIN_HAND_OPTION_1 + 1;
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        gear.draw(1, 0, 0);
        assert_eq!(gear.drawn(), Some(SECOND_OPTION));
        let taken = Gear::carried_in(&kits, CHARACTER, &gear.carried(), gear.drawn());
        assert_eq!(taken.drawn(), Some(SECOND_OPTION));
        assert!(taken.hands().attached, "already in the hand");
        let fresh = Gear::carried_in(&kits, CHARACTER, &gear.carried(), None);
        assert_eq!(fresh.drawn(), None);
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

    #[test]
    fn a_drawn_weapon_reaches_the_hand_and_a_shot_weapon_reloads() {
        const SHOTS: u8 = 2;
        const RELOAD_MS: u64 = 1_460;
        const NOW: u64 = 10_000;
        const TIME_NS: u64 = 7;
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        // The kit sword is a two-shot weapon here.
        gear.arm(
            |item| (item == SWORD).then_some((SHOTS, RELOAD_MS)),
            TIME_NS,
        );
        let hands = gear.hands();
        assert_eq!(hands.drawn, None, "every weapon sheathed at login");
        assert_eq!(
            hands.ammo[0],
            Some(LoadedAmmo {
                count: SHOTS,
                time_ns: TIME_NS
            })
        );
        gear.draw(0, NOW, TIME_NS);
        assert_eq!(gear.hands().drawn, Some(slot::MAIN_HAND_OPTION_1));
        assert!(!gear.hands().attached);
        gear.tick_hands(NOW + ATTACH_MS);
        assert!(gear.hands().attached, "in the hand after the attach time");
        assert!(gear.fire(NOW));
        assert!(gear.fire(NOW));
        assert!(!gear.fire(NOW), "empty after two shots");
        assert_eq!(gear.hands().ammo[0].map(|ammo| ammo.count), Some(0));
        gear.tick_hands(NOW + RELOAD_MS - 1);
        assert!(!gear.fire(NOW + RELOAD_MS - 1));
        gear.tick_hands(NOW + RELOAD_MS);
        assert_eq!(gear.hands().ammo[0].map(|ammo| ammo.count), Some(SHOTS));
        // The same weapon again keeps its shots; arming checks only changed slots.
        assert!(gear.fire(NOW + RELOAD_MS));
        gear.arm(|_| Some((SHOTS, RELOAD_MS)), TIME_NS);
        assert_eq!(gear.hands().ammo[0].map(|ammo| ammo.count), Some(SHOTS - 1));
    }

    #[test]
    fn weapons_swap_slots_and_the_picked_one_is_in_the_hand() {
        const SECOND_OPTION: u32 = slot::MAIN_HAND_OPTION_1 + 1;
        const ANY_BAG_SLOT: u32 = u32::MAX;
        const EMPTY: [u8; RAW_ID_LEN] = [0; RAW_ID_LEN];
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        assert_eq!(
            gear.swap(slot::MAIN_HAND_OPTION_1, SECOND_OPTION, EMPTY, EMPTY),
            Err(EquipRefusal::NotWorn),
            "the ids must match the worn items"
        );
        assert_eq!(
            gear.swap(
                slot::CHEST,
                SECOND_OPTION,
                instance("MysticArchetypeChest"),
                EMPTY
            ),
            Err(EquipRefusal::WrongSlot)
        );
        assert_eq!(
            gear.swap(
                slot::MAIN_HAND_OPTION_1,
                SECOND_OPTION,
                instance(SWORD),
                EMPTY
            ),
            Ok(())
        );
        assert!(gear.take_change().is_some());
        assert_eq!(gear.main_weapon(), Some((SWORD, 75)), "the only weapon");
        gear.draw(0, 0, 0);
        assert_eq!(
            gear.main_weapon(),
            Some((SWORD, 75)),
            "an empty pick keeps one"
        );
        gear.draw(1, 0, 0);
        assert_eq!(gear.main_weapon(), Some((SWORD, 75)));
        assert_eq!(
            gear.unequip(slot::MAIN_HAND_OPTION_1, ANY_BAG_SLOT),
            Err(EquipRefusal::NotWorn)
        );
        assert_eq!(gear.unequip(SECOND_OPTION, ANY_BAG_SLOT), Ok(()));
        assert_eq!(gear.main_weapon(), None);
        assert_eq!(
            gear.bag[0].as_ref().map(|stack| stack.item_id.as_str()),
            Some(SWORD)
        );
    }
}
