// SPDX-License-Identifier: AGPL-3.0-or-later
//! The entity slots (`interestId`) of one client (world-entry note, "Spawn condition traced").
//! The client's own player is slot [`LOCAL_PLAYER_INTEREST`]; every other entity it sees gets a
//! slot of its own while it stays in view. A slot that leaves the view is stopped in a later
//! bundle's index block, and only after that stop has gone out can it be given to another entity.

use std::collections::BTreeMap;

use nw_messages::LOCAL_PLAYER_INTEREST;

/// The first slot of another entity. The captures use slot 1 for the own player and slots from 3
/// up for the rest; slot 2 is never used there (its meaning is Unknown).
pub(super) const FIRST_REMOTE_INTEREST: u16 = 3;

/// The slots of one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Slots {
    /// The slot of each entity in view.
    assigned: BTreeMap<u64, u16>,
    /// Stopped slots whose stop has gone out.
    free: Vec<u16>,
    /// The lowest slot never given.
    next: Option<u16>,
    /// Slots to stop in the next bundles.
    stops: Vec<u16>,
}

/// The slot of one entity in this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Slot {
    /// The slot.
    pub(super) interest: u16,
    /// Whether the entity got it in this tick (so its spawn card must go out).
    pub(super) new: bool,
}

impl Slots {
    /// A client that sees nothing yet.
    pub(super) const fn new() -> Self {
        Self {
            assigned: BTreeMap::new(),
            free: Vec::new(),
            next: Some(FIRST_REMOTE_INTEREST),
            stops: Vec::new(),
        }
    }

    /// The slots another sim gave: `assigned` entities and the `stops` it did not send yet.
    /// A slot that is in neither may still be tied on the client, so the next new slot is above
    /// every slot given here.
    pub(super) fn restored(assigned: BTreeMap<u64, u16>, stops: Vec<u16>) -> Self {
        let highest = assigned
            .values()
            .chain(&stops)
            .copied()
            .max()
            .unwrap_or(LOCAL_PLAYER_INTEREST);
        Self {
            assigned,
            free: Vec::new(),
            next: highest.max(FIRST_REMOTE_INTEREST - 1).checked_add(1),
            stops,
        }
    }

    /// Keeps the slots of the entities in `seen` and stops the rest.
    pub(super) fn keep_only(&mut self, seen: &[u64]) {
        let gone: Vec<u64> = self
            .assigned
            .keys()
            .filter(|entity| seen.binary_search(entity).is_err())
            .copied()
            .collect();
        for entity in gone {
            if let Some(interest) = self.assigned.remove(&entity) {
                self.stops.push(interest);
            }
        }
    }

    /// The slot of `entity`, given now when it has none; `None` when every slot is in use.
    pub(super) fn slot(&mut self, entity: u64) -> Option<Slot> {
        if let Some(interest) = self.assigned.get(&entity) {
            return Some(Slot {
                interest: *interest,
                new: false,
            });
        }
        let interest = match self.free.pop() {
            Some(interest) => interest,
            None => {
                let interest = self.next?;
                self.next = interest.checked_add(1);
                interest
            }
        };
        self.assigned.insert(entity, interest);
        Some(Slot {
            interest,
            new: true,
        })
    }

    /// Takes at most `limit` of the slots to stop, for one bundle; they are free from now on.
    pub(super) fn take_stops(&mut self, limit: usize) -> Vec<u16> {
        let taken: Vec<u16> = self.stops.drain(..limit.min(self.stops.len())).collect();
        self.free.extend(&taken);
        taken
    }

    /// The slot of each entity in view, for a handoff.
    pub(super) fn assigned(&self) -> &BTreeMap<u64, u16> {
        &self.assigned
    }

    /// The slots still to stop, for a handoff.
    pub(super) fn pending_stops(&self) -> &[u16] {
        &self.stops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOB: u64 = 40;
    const OTHER: u64 = 41;
    const LATE: u64 = 42;
    const ALL_STOPS: usize = 10;

    #[test]
    fn a_slot_is_given_once_and_kept_while_seen() {
        let mut slots = Slots::new();
        let first = slots.slot(MOB).expect("slot");
        assert_eq!(
            first,
            Slot {
                interest: FIRST_REMOTE_INTEREST,
                new: true
            }
        );
        assert_eq!(
            slots.slot(MOB),
            Some(Slot {
                interest: FIRST_REMOTE_INTEREST,
                new: false
            })
        );
        assert_eq!(
            slots.slot(OTHER).map(|slot| slot.interest),
            Some(FIRST_REMOTE_INTEREST + 1)
        );
    }

    #[test]
    fn a_left_slot_is_stopped_and_reused_only_after_its_stop_went_out() {
        let mut slots = Slots::new();
        let mob = slots.slot(MOB).expect("slot").interest;
        slots.keep_only(&[]);
        assert_eq!(slots.pending_stops(), &[mob]);
        let other = slots.slot(OTHER).expect("slot").interest;
        assert_ne!(other, mob, "not reused before its stop went out");
        assert_eq!(slots.take_stops(ALL_STOPS), vec![mob]);
        assert_eq!(slots.slot(LATE).map(|slot| slot.interest), Some(mob));
    }

    #[test]
    fn stops_go_out_in_parts() {
        let mut slots = Slots::new();
        for entity in [MOB, OTHER, LATE] {
            slots.slot(entity);
        }
        slots.keep_only(&[]);
        assert_eq!(slots.take_stops(2).len(), 2);
        assert_eq!(slots.take_stops(2).len(), 1);
        assert!(slots.take_stops(2).is_empty());
    }

    #[test]
    fn restored_slots_keep_their_ties_and_new_slots_go_above_them() {
        let assigned = BTreeMap::from([(MOB, FIRST_REMOTE_INTEREST + 4)]);
        let mut slots = Slots::restored(assigned, vec![FIRST_REMOTE_INTEREST + 7]);
        assert_eq!(
            slots.slot(MOB),
            Some(Slot {
                interest: FIRST_REMOTE_INTEREST + 4,
                new: false
            })
        );
        assert_eq!(
            slots.slot(OTHER).map(|slot| slot.interest),
            Some(FIRST_REMOTE_INTEREST + 8)
        );
        let empty = Slots::restored(BTreeMap::new(), Vec::new());
        assert_eq!(empty, Slots::new());
    }

    #[test]
    fn no_slot_is_given_past_the_last() {
        let mut slots = Slots::restored(BTreeMap::from([(MOB, u16::MAX)]), Vec::new());
        assert_eq!(slots.slot(OTHER), None);
    }
}
