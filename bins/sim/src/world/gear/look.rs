// SPDX-License-Identifier: AGPL-3.0-or-later
//! The look of the worn items (run-15 items2 study): the skin over each and the dyes, as the
//! transmog calls (5692, 5693) and the paperdoll dye (2224) change them. A change goes out with
//! the next gear change and is saved with the items (world migration 0017).

use nw_messages::ItemLook;
use nw_messages::wire::RAW_ID_LEN;

use super::Gear;

/// A worn item as the transmog calls see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) struct WornLook<'a> {
    /// Its item id.
    pub item_id: &'a str,
    /// Its instance id.
    pub instance: [u8; RAW_ID_LEN],
    /// Its look.
    pub look: ItemLook,
}

impl Gear {
    /// The item worn in paperdoll slot `slot_id` and its look; `None` for an empty slot.
    pub(in crate::world) fn worn_look(&self, slot_id: u32) -> Option<WornLook<'_>> {
        self.worn_stacks()
            .find(|(worn, _)| *worn == slot_id)
            .map(|(_, stack)| WornLook {
                item_id: &stack.item_id,
                instance: stack.item.instance,
                look: stack.item.look,
            })
    }

    /// Gives the item worn in paperdoll slot `slot_id` the look `look`; whether it changed (an
    /// empty slot and the same look change nothing).
    pub(in crate::world) fn set_worn_look(&mut self, slot_id: u32, look: ItemLook) -> bool {
        let Some(stack) = self
            .worn
            .iter_mut()
            .find(|(worn, stack)| *worn == slot_id && stack.is_some())
            .and_then(|(_, stack)| stack.as_mut())
        else {
            return false;
        };
        if stack.item.look == look {
            return false;
        }
        stack.item.look = look;
        self.changed = true;
        self.unsaved = true;
        true
    }

    /// The item ids of every worn and carried stack.
    pub(in crate::world) fn held_item_ids(&self) -> impl Iterator<Item = &str> {
        self.worn_stacks()
            .map(|(_, stack)| stack.item_id.as_str())
            .chain(
                self.bag
                    .iter()
                    .flatten()
                    .map(|stack| stack.item_id.as_str()),
            )
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::paperdoll::slot;

    use super::super::tests::{BACKSTORY, CHARACTER, SWORD, kits};
    use super::*;

    #[test]
    fn a_worn_item_takes_a_new_look_once() {
        let kits = kits();
        let mut gear = Gear::starter(&kits, CHARACTER, Some(BACKSTORY)).expect("gear");
        let look = ItemLook {
            dye: [4, 0, 0, 0],
            appearance: 0xb051_8525,
            skin_dye: [0, 6, 0, 0],
        };
        let sword = gear.worn_look(slot::MAIN_HAND_OPTION_1).expect("sword");
        assert_eq!((sword.item_id, sword.look), (SWORD, ItemLook::default()));
        assert!(gear.set_worn_look(slot::MAIN_HAND_OPTION_1, look));
        assert!(!gear.set_worn_look(slot::MAIN_HAND_OPTION_1, look));
        assert!(!gear.set_worn_look(slot::HEAD, look), "no helmet is worn");
        assert_eq!(
            gear.worn_look(slot::MAIN_HAND_OPTION_1)
                .map(|worn| worn.look),
            Some(look)
        );
        assert!(gear.take_unsaved().is_some());
        assert!(gear.held_item_ids().any(|item_id| item_id == SWORD));
    }
}
