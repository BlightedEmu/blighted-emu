// SPDX-License-Identifier: AGPL-3.0-or-later
//! Health floors of mobs, one rule for every boss script (the story trial bosses of
//! [`super::boss_scripts`], the linked expedition bosses): an owner sets a floor, a share of a
//! mob's max health; a hit that would take the mob below it stops at it
//! ([`SimWorld::floored_hit`], in every hit on a mob), and the owner takes the mobs that reached
//! their floor ([`HealthFloors::take_reached`]) to play what comes then. Retail sends such
//! floors as status effects (`AI_Isabella_MinHealth_80`, `HealthMinPercent` 0.8: "Sets
//! Isabella's minimum health to 80% so she doesn't take extra damage when playing the
//! reaction"); an owner may show that effect too.

use std::collections::HashMap;

use super::SimWorld;

/// The floor of one mob.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Floor {
    /// The share of its max health its health does not go below.
    share: f64,
    /// Who set it (the owner's own name): only it takes its reaches.
    owner: &'static str,
}

/// A hit took a mob down to its floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FloorReached {
    /// The mob.
    pub(super) mob: u64,
    /// Who hit it there.
    pub(super) attacker: u64,
    /// The owner of the floor.
    owner: &'static str,
}

/// The health floors of the mobs that have one.
#[derive(Debug, Default)]
pub(super) struct HealthFloors {
    floors: HashMap<u64, Floor>,
    /// The floors reached since their owners last took them.
    reached: Vec<FloorReached>,
}

impl HealthFloors {
    /// Mob `mob` takes no hit below `share` of its max health, by `owner`'s rule (a new floor
    /// replaces its old one).
    pub(super) fn set(&mut self, mob: u64, share: f64, owner: &'static str) {
        self.floors.insert(mob, Floor { share, owner });
    }

    /// Mob `mob` has no floor again: hits take it down to 0.
    pub(super) fn clear(&mut self, mob: u64) {
        self.floors.remove(&mob);
        self.reached.retain(|reached| reached.mob != mob);
    }

    /// The mobs of `owner`'s floors that a hit took down to their floor since the last call,
    /// in hit order.
    pub(super) fn take_reached(&mut self, owner: &str) -> Vec<FloorReached> {
        let (taken, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.reached)
            .into_iter()
            .partition(|reached| reached.owner == owner);
        self.reached = kept;
        taken
    }

    /// The share of a hit of `amount` by `attacker` that mob `mob`, at `current` of `max`
    /// health, takes: at most down to its floor. A hit that comes down onto the floor from above
    /// it is a reach for the floor's owner.
    fn hit(&mut self, mob: u64, attacker: u64, (current, max): (u32, u32), amount: u32) -> u32 {
        let Some(floor) = self.floors.get(&mob).copied() else {
            return amount;
        };
        // A share of a health of millions at most.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a share of the max health"
        )]
        let floor_health = (f64::from(max) * floor.share).ceil() as u32;
        let taken = amount.min(current.saturating_sub(floor_health));
        if current > floor_health && current - taken == floor_health {
            self.reached.push(FloorReached {
                mob,
                attacker,
                owner: floor.owner,
            });
        }
        taken
    }
}

impl SimWorld {
    /// The share of a hit of `amount` by `attacker` that mob `target` takes: down to its health
    /// floor at most ([`HealthFloors`]).
    pub(super) fn floored_hit(&mut self, target: u64, attacker: u64, amount: u32) -> u32 {
        let Some(health) = self.mobs.get(target).map(|mob| mob.health) else {
            return amount;
        };
        self.health_floors
            .hit(target, attacker, (health.current(), health.max()), amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOB: u64 = 3;
    const PLAYER: u64 = 9;
    const OWNER: &str = "test";
    const MAX: u32 = 108_888;

    /// Retail `99g_msq_sm_destroyenemy`: Isabella's health stopped at exactly 80.0 % (87,110.4
    /// of 108,888, 155.045 s) under hits of thousands.
    #[test]
    fn a_hit_stops_at_the_floor_and_its_owner_takes_the_reach_once() {
        let mut floors = HealthFloors::default();
        assert_eq!(floors.hit(MOB, PLAYER, (MAX, MAX), 500), 500, "no floor");
        floors.set(MOB, 0.8, OWNER);
        let floor = 87_111;
        assert_eq!(floors.hit(MOB, PLAYER, (floor + 100, MAX), 3_000), 100);
        assert_eq!(
            floors.hit(MOB, PLAYER, (floor, MAX), 3_000),
            0,
            "at the floor"
        );
        assert!(floors.take_reached("other").is_empty(), "another owner's");
        assert_eq!(
            floors.take_reached(OWNER),
            [FloorReached {
                mob: MOB,
                attacker: PLAYER,
                owner: OWNER
            }]
        );
        assert!(floors.take_reached(OWNER).is_empty(), "once");
        floors.set(MOB, 0.65, OWNER);
        assert_eq!(
            floors.hit(MOB, PLAYER, (floor, MAX), 3_000),
            3_000,
            "the next floor"
        );
        floors.clear(MOB);
        assert_eq!(
            floors.hit(MOB, PLAYER, (10, MAX), 3_000),
            3_000,
            "down to 0"
        );
    }
}
