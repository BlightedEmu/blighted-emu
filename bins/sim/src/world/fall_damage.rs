// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fall damage (run-15 fall study: 385 retail fall hits in 150 ledgers, all with the item tag of
//! `DamageTypeData` `Falling`).
//!
//! The client has no fall formula and no data row gives one; retail's falls fit the drop from
//! the top of the fall to the landing: the share of the max health is 0.01194 per metre above
//! 9.97 m, at most 0.5 (32 of 37 clean uncapped falls, residual RMS 0.0135; 9 hits at exactly
//! 0.5, none above, which is the `AddDmg` 0.5 of the player rows `Fall` and `Fall_Hard`). Falls of
//! 10.39 m and 10.66 m did no damage, falls of 10.99 m and 11.03 m did. The armor takes nothing
//! (`Falling` is of category `True`; all 385 item scales 1.0).
//!
//! A damaging landing sends the player the fall 3601 and, 0 to 4 ms later, the fall 2071; a
//! rider whose fall took a tenth of its health or more gets off ([`SimWorld::mount_fall`]).

use bevy_ecs::entity::Entity;
use nw_messages::{DamageDealtMsg, OnDamageMsg, ServerMessage};

use super::attack::Vitals;
use super::{Player, SimWorld, send_now};

/// The drop at which the fit's damage starts, metres.
const FALL_ZERO_M: f32 = 9.97;
/// The share of the max health each metre above [`FALL_ZERO_M`] takes.
const FALL_SHARE_PER_M: f64 = 0.011_94;
/// The most a fall takes: the `AddDmg` of the player's `Fall` row.
const FALL_MAX_SHARE: f64 = 0.5;
/// The shortest drop that takes health (best guess: between the longest retail fall without
/// damage, 10.66 m, and the shortest with, 10.99 m).
const FALL_MIN_DROP_M: f32 = 10.8;

/// The fall of one body: the top since it left the floor.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct FallTrack {
    apex_z: Option<f32>,
}

impl FallTrack {
    /// The body is at height `z`, `grounded` or not, `swimming` or not: the drop of a landing,
    /// from the top of its fall. Water ends a fall without a landing.
    pub(super) fn step(&mut self, z: f32, grounded: bool, swimming: bool) -> Option<f32> {
        if swimming {
            self.apex_z = None;
            return None;
        }
        if !grounded {
            self.apex_z = Some(self.apex_z.map_or(z, |apex| apex.max(z)));
            return None;
        }
        self.apex_z.take().map(|apex| apex - z)
    }

    /// The server put the body somewhere: no fall runs.
    pub(super) const fn forget(&mut self) {
        self.apex_z = None;
    }
}

/// The share of the max health a drop of `drop_m` takes.
pub(super) fn fall_share(drop_m: f32) -> f64 {
    if drop_m < FALL_MIN_DROP_M {
        return 0.0;
    }
    (f64::from(drop_m - FALL_ZERO_M) * FALL_SHARE_PER_M).clamp(0.0, FALL_MAX_SHARE)
}

impl SimWorld {
    /// The landings of the players' falls at `now_ms`: each drop that takes health hurts its
    /// player. A body the server moves, and a dead one, has no fall.
    pub(super) fn tick_falls(&mut self, now_ms: u64) {
        let mut landed: Vec<(Entity, u64, f64)> = Vec::new();
        let mut query = self.world.query::<(Entity, &mut Player, &Vitals)>();
        for (entity, mut player, vitals) in query.iter_mut(&mut self.world) {
            let player = &mut *player;
            if vitals.health.is_dead() || player.push.is_some() || player.teleport_step.is_some() {
                player.fall.forget();
                continue;
            }
            let z = player.tracked.position().z;
            let grounded = player.tracked.is_grounded();
            let swimming = player.tracked.is_swimming();
            if let Some(drop_m) = player.fall.step(z, grounded, swimming) {
                let share = fall_share(drop_m);
                if share > 0.0 {
                    landed.push((entity, player.number, share));
                }
            }
        }
        for (entity, number, share) in landed {
            self.fall_hurts(entity, number, share, now_ms);
        }
    }

    /// Player `number` (`entity`) landed a fall that takes `share` of its max health at
    /// `now_ms`: the fall 3601, a rider's dismount, the fall 2071; a fall that takes the last
    /// health is a hit that runs the death.
    fn fall_hurts(&mut self, entity: Entity, number: u64, share: f64, now_ms: u64) {
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return;
        };
        let taken = {
            let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) else {
                return;
            };
            let max_health = vitals.health.max();
            // A share of at most half the max health.
            let amount = (f64::from(max_health) * share).round() as u32;
            (amount < vitals.health.current()).then(|| {
                vitals.health.take(amount);
                (amount, max_health)
            })
        };
        self.player_hit_taken(entity);
        let Some((amount, max_health)) = taken else {
            let lethal = self
                .world
                .get::<Vitals>(entity)
                .map_or(0, |vitals| vitals.health.current());
            self.apply_hit(number, number, lethal, 0, now_ms);
            return;
        };
        // Health of a few thousand: f32 holds it.
        let (amount, max_health) = (amount as f32, max_health as f32);
        send_now(
            &mut self.combat_messages,
            &connection,
            &[ServerMessage::OnDamage(OnDamageMsg::fall(number, amount, max_health))],
        );
        let dismounted = self.mount_fall(entity, share, now_ms);
        send_now(
            &mut self.combat_messages,
            &connection,
            &[ServerMessage::DamageDealt(DamageDealtMsg::fall(number, amount))],
        );
        if dismounted {
            self.fall_dismount_notification(entity);
        }
        tracing::debug!(player = number, share, "fall damage");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUND_Z: f32 = 100.0;

    /// The drop of a fall from `height_m` above the ground to it.
    fn landing(height_m: f32) -> Option<f32> {
        let mut track = FallTrack::default();
        assert_eq!(track.step(GROUND_Z, true, false), None, "standing");
        assert_eq!(track.step(GROUND_Z + height_m, false, false), None, "the top");
        assert_eq!(track.step(GROUND_Z + 1.0, false, false), None, "falling");
        track.step(GROUND_Z, true, false)
    }

    #[test]
    fn a_landing_gives_the_drop_from_the_top() {
        assert_eq!(landing(20.0), Some(20.0));
        let mut track = FallTrack::default();
        track.step(GROUND_Z + 30.0, false, false);
        assert_eq!(track.step(GROUND_Z - 2.0, false, true), None, "water ends it");
        assert_eq!(track.step(GROUND_Z, true, false), None);
    }

    #[test]
    fn the_share_follows_the_retail_fit() {
        assert_eq!(fall_share(10.66), 0.0, "retail: no damage at 10.66 m");
        assert!(fall_share(10.99) > 0.0, "retail: damage at 10.99 m");
        let share = fall_share(30.0);
        assert!((share - (30.0 - 9.97) * FALL_SHARE_PER_M).abs() < 1e-6);
        assert_eq!(fall_share(80.0), FALL_MAX_SHARE, "at most half");
    }
}
