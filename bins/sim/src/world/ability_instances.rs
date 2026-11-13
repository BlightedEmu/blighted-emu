// SPDX-License-Identifier: AGPL-3.0-or-later
//! The placed ability instances of a player (ability instance tracking study, 2026-11-12): each
//! spell entity of a player's ability whose slice has an `AbilityInstanceType`
//! ([`nw_content::ability_instances`]) takes a slot of its type's row of the replicated state 2267
//! ([`nw_messages::ability_instances`]) while it is in the world.
//!
//! Observed (71 ledgers): the slot fills when the spell entity is made (`20260927-223955-324`:
//! the Sacred Ground card and its slot at 234.425 s, 2.7 ms after the cooldown's start) and
//! empties when its time ends (246.422 s, 12.0 s later: Sacred Ground's `Duration`; Beacon
//! 15 s, Ice Storm 10 s, Ice Shower 8 s, Oblivion 7 s, Meteor Storm 6 s, Sandworm 3 s, in 1,474
//! fill and empty pairs). A new instance takes the first free slot of its row. The own spawn
//! group repeats every slot the session changed with its value now (spawn groups of 56 members
//! with emptied slots); a session that never made an instance sends no state 2267
//! (`newchar-retail-2026-09-18`).
//!
//! Inferred: the time is the server clock in ms, times 1,000,000 (retail keeps ns); a new
//! instance of a full row takes the slot of the oldest, whose entity goes.

use nw_messages::ability_instances::{
    AbilityInstance, AbilityInstanceTrackingState, INSTANCE_TYPES, SLOTS_PER_TYPE, instance_child,
};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{BundleMember, NS_PER_MS};

/// One tracked instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tracked {
    /// The slot value the client gets.
    value: AbilityInstance,
    /// The spell entity's number in the sim.
    entity: u64,
}

/// A new instance's slot: the spell entity it put out of its slot, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Placed {
    /// The spell entity of the oldest instance of a full row; it goes.
    pub(super) evicted: Option<u64>,
}

/// The ability instances of one player.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct AbilityInstances {
    /// The rows by `AbilityInstanceType`.
    rows: [[Option<Tracked>; SLOTS_PER_TYPE]; INSTANCE_TYPES],
    /// The children changed since the last send.
    changed: u64,
    /// The children the session ever changed: the spawn group repeats them.
    touched: u64,
}

impl AbilityInstances {
    /// Tracks the spell entity `entity` (raw id `raw_id`) of type `instance_type`, made at
    /// `server_ms`; the type has `slots` slots (its limit). `None` for a type outside the rows.
    pub(super) fn track(
        &mut self,
        instance_type: u8,
        slots: usize,
        (entity, raw_id): (u64, [u8; RAW_ID_LEN]),
        server_ms: u64,
    ) -> Option<Placed> {
        let row_index = usize::from(instance_type);
        let row = self.rows.get_mut(row_index)?;
        let usable = slots.clamp(1, SLOTS_PER_TYPE);
        let free = row[..usable].iter().position(Option::is_none);
        let (slot, evicted) = match free {
            Some(slot) => (slot, None),
            None => {
                let (slot, oldest) = row[..usable]
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, tracked)| tracked.map(|tracked| (slot, tracked)))
                    .min_by_key(|(_, tracked)| tracked.value.created_ns)?;
                (slot, Some(oldest.entity))
            }
        };
        row[slot] = Some(Tracked {
            value: AbilityInstance::of_entity(server_ms.saturating_mul(NS_PER_MS), raw_id),
            entity,
        });
        self.mark(row_index, slot);
        Some(Placed { evicted })
    }

    /// Empties the slot of the spell entity `entity`, when one holds it.
    pub(super) fn gone(&mut self, entity: u64) {
        let found = self.rows.iter().enumerate().find_map(|(row, slots)| {
            slots
                .iter()
                .position(|tracked| tracked.is_some_and(|tracked| tracked.entity == entity))
                .map(|slot| (row, slot))
        });
        if let Some((row, slot)) = found {
            self.rows[row][slot] = None;
            self.mark(row, slot);
        }
    }

    fn mark(&mut self, row: usize, slot: usize) {
        if let Some(child) = instance_child(row, slot) {
            self.changed |= 1 << child;
            self.touched |= 1 << child;
        }
    }

    /// The state to send: the changed slots, or with the own spawn card (`spawn`) every slot the
    /// session changed; `None` when there is none.
    pub(super) fn take_member(&mut self, spawn: bool) -> Option<BundleMember> {
        let children = if spawn { self.touched } else { self.changed };
        self.changed = 0;
        if children == 0 {
            return None;
        }
        let slots = self
            .rows
            .iter()
            .enumerate()
            .flat_map(|(row, slots)| {
                slots.iter().enumerate().filter_map(move |(slot, tracked)| {
                    let child = instance_child(row, slot)?;
                    (children & 1 << child != 0).then(|| {
                        (
                            child,
                            tracked.map_or(AbilityInstance::EMPTY, |tracked| tracked.value),
                        )
                    })
                })
            })
            .collect();
        AbilityInstanceTrackingState { slots }.member().ok()
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::MemberState;
    use nw_messages::ability_instances::{
        ABILITY_INSTANCE_TRACKING_COMPONENT, ABILITY_INSTANCE_TRACKING_TYPE_INDEX,
    };

    use super::*;

    const SACRED_GROUND: u8 = 3;
    const ICE_SHOWER: u8 = 6;
    /// The retail Sacred Ground of `20260927-223955-324` (234.425 s).
    const RETAIL_RAW: [u8; RAW_ID_LEN] = [
        0x80, 0xa2, 0xe6, 0xb4, 0x7c, 0x38, 0x5f, 0xdc, 0x11, 0xa5, 0x55, 0x05, 0xca, 0x38, 0x4e,
        0xc4,
    ];
    /// Its time (843,857,054,614,229,505 ns) in ms.
    const RETAIL_MS: u64 = 843_857_054_614;
    const RETAIL_GONE: &str =
        "0180800200000000000000000000000000000000000000000000000000000000ffffffff";

    fn body(member: Option<BundleMember>) -> Vec<u8> {
        let member = member.expect("a member");
        assert_eq!(member.component, ABILITY_INSTANCE_TRACKING_COMPONENT);
        match member.state {
            MemberState::Recorded { class, body } => {
                assert_eq!(class, ABILITY_INSTANCE_TRACKING_TYPE_INDEX);
                body
            }
            other => panic!("not recorded: {other:?}"),
        }
    }

    #[test]
    fn a_sacred_ground_fills_child_15_then_empties_it_as_retail() {
        let mut instances = AbilityInstances::default();
        assert_eq!(instances.take_member(true), None, "no instance: no state");
        let placed = instances.track(SACRED_GROUND, 1, (7, RETAIL_RAW), RETAIL_MS);
        assert_eq!(placed, Some(Placed { evicted: None }));
        let made = body(instances.take_member(false));
        assert_eq!(
            hex::encode(&made),
            "018080020bb5fb852d79918080a2e6b47c385fdc11a55505ca384ec4dc5f387cb4e6a280",
            "the retail body with the time in whole ms"
        );
        assert_eq!(instances.take_member(false), None, "sent once");
        instances.gone(7);
        assert_eq!(hex::encode(body(instances.take_member(false))), RETAIL_GONE);
        instances.gone(7);
        assert_eq!(instances.take_member(false), None, "gone once");
        assert_eq!(
            hex::encode(body(instances.take_member(true))),
            RETAIL_GONE,
            "the spawn group repeats the emptied slot"
        );
    }

    #[test]
    fn a_new_instance_takes_the_first_free_slot_and_a_full_row_the_oldest() {
        let mut instances = AbilityInstances::default();
        for (entity, ms) in [(1, 10), (2, 20)] {
            instances.track(ICE_SHOWER, SLOTS_PER_TYPE, (entity, [2; RAW_ID_LEN]), ms);
        }
        instances.gone(1);
        instances.take_member(false);
        instances.track(ICE_SHOWER, SLOTS_PER_TYPE, (3, [2; RAW_ID_LEN]), 30);
        let state = AbilityInstanceTrackingState::read(&mut nw_messages::wire::Reader::new(&body(
            instances.take_member(false),
        )))
        .expect("read");
        assert_eq!(
            state
                .slots
                .iter()
                .map(|(child, _)| *child)
                .collect::<Vec<_>>(),
            [instance_child(usize::from(ICE_SHOWER), 0).expect("child")],
            "slot 0 again, as the retail Ice Showers"
        );
        let mut one = AbilityInstances::default();
        one.track(SACRED_GROUND, 1, (4, [2; RAW_ID_LEN]), 10);
        let placed = one.track(SACRED_GROUND, 1, (5, [2; RAW_ID_LEN]), 20);
        assert_eq!(placed, Some(Placed { evicted: Some(4) }));
        one.gone(4);
        assert_eq!(
            one.rows[usize::from(SACRED_GROUND)][0].map(|t| t.entity),
            Some(5)
        );
        assert_eq!(
            one.track(
                u8::try_from(INSTANCE_TYPES).expect("small"),
                1,
                (6, [2; RAW_ID_LEN]),
                30
            ),
            None
        );
    }
}
