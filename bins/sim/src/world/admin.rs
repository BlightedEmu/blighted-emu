// SPDX-License-Identifier: AGPL-3.0-or-later
//! GM commands that the admin tool sends to this sim (admin studio spec sections 8.1, 8.2).
//! `Kick` needs the server loop (it saves and closes the connection): see
//! [`SimWorld::connection_of`].

use bevy_ecs::component::Component;
use nw_bus::{AdminAction, AdminCommand, AdminReply, ConnectionRef, Point};
use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::Motion;
use nw_world::records::VitalsSpawn;

use super::attack::Vitals;
use super::{Body, Player, SimWorld};

/// Most GM mobs one command spawns.
pub const MAX_GM_SPAWN: u32 = 50;
/// Metres between GM mobs of one command, on a ring around the point.
const GM_SPAWN_SPACING_M: f32 = 2.0;
const LOWEST_LEVEL: u32 = 1;
const HIGHEST_LEVEL: u32 = 65;

/// A player that takes no damage (GM god mode).
#[derive(Component, Debug, Clone, Copy)]
pub(super) struct GodMode;

impl SimWorld {
    /// Carries out `command` at `now_ms`. A target this sim does not own gives
    /// [`AdminReply::NotMine`]. `Kick` is the server's: here it is refused.
    pub fn admin(&mut self, command: &AdminCommand, now_ms: u64) -> AdminReply {
        match &command.action {
            AdminAction::TeleportPlayer { entity, to } => {
                self.teleport_player(*entity, *to, now_ms)
            }
            AdminAction::Heal { entity } => self.with_vitals(*entity, |vitals| {
                vitals.health.refill();
                vitals.dead_since_ms = None;
            }),
            AdminAction::Kill { entity } => self.kill_player(*entity, now_ms),
            AdminAction::GodMode { entity, on } => self.god_mode(*entity, *on),
            AdminAction::GiveItem {
                entity,
                item,
                quantity,
            } => self.give_item(*entity, item, *quantity),
            AdminAction::TakeItem { entity, item } => self.take_item(*entity, item),
            AdminAction::SpawnMobs {
                vitals,
                level,
                at,
                count,
            } => self.spawn_gm_mobs(vitals, *level, *at, *count),
            AdminAction::DespawnMobs { entities } => self.despawn_gm_mobs(entities),
            AdminAction::Kick { .. } => AdminReply::Failed {
                reason: "a kick is the server loop's".to_owned(),
            },
        }
    }

    /// The connection of the player with entity number `entity`, when it is here.
    pub fn connection_of(&mut self, entity: u64) -> Option<ConnectionRef> {
        let target = self.player_by_number(entity)?;
        self.world
            .get::<Player>(target)
            .map(|player| player.connection.clone())
    }

    /// True when the player with entity number `entity` takes no damage.
    pub(super) fn is_god(&self, entity: bevy_ecs::entity::Entity) -> bool {
        self.world.get::<GodMode>(entity).is_some()
    }

    /// Moves the player with entity number `entity` to `to`. The next own state of the player
    /// carries the teleport to its client.
    fn teleport_player(&mut self, entity: u64, to: Point, now_ms: u64) -> AdminReply {
        let bounds = self.settings.bounds;
        let inside = (bounds.min_x..=bounds.max_x).contains(&to.x)
            && (bounds.min_y..=bounds.max_y).contains(&to.y)
            && to.z.is_finite();
        if !inside {
            return AdminReply::Failed {
                reason: format!("({}, {}) is off the level", to.x, to.y),
            };
        }
        let Some(target) = self.player_by_number(entity) else {
            return AdminReply::NotMine;
        };
        let position = nw_sim_core::Position {
            x: to.x,
            y: to.y,
            z: to.z,
        };
        if let Some(mut body) = self.world.get_mut::<Body>(target) {
            body.motion = Motion::new(position, now_ms, &self.settings.limits);
        }
        if let Some(mut player) = self.world.get_mut::<Player>(target) {
            player.moved = true;
        }
        AdminReply::Done
    }

    /// Changes the vitals of the player `entity`; the next own state sends the health.
    fn with_vitals(&mut self, entity: u64, change: impl FnOnce(&mut Vitals)) -> AdminReply {
        let Some(target) = self.player_by_number(entity) else {
            return AdminReply::NotMine;
        };
        match self.world.get_mut::<Vitals>(target) {
            Some(mut vitals) => {
                change(&mut vitals);
                AdminReply::Done
            }
            None => AdminReply::Failed {
                reason: "the player has no vitals".to_owned(),
            },
        }
    }

    /// Takes all of the player's health as a hit of itself, so its client sees the death.
    fn kill_player(&mut self, entity: u64, now_ms: u64) -> AdminReply {
        let Some(target) = self.player_by_number(entity) else {
            return AdminReply::NotMine;
        };
        let Some(health) = self
            .world
            .get::<Vitals>(target)
            .map(|vitals| vitals.health.current())
        else {
            return AdminReply::Failed {
                reason: "the player has no vitals".to_owned(),
            };
        };
        self.world.entity_mut(target).remove::<GodMode>();
        match self.apply_hit(entity, entity, health, 0, now_ms) {
            Some(_) => AdminReply::Done,
            None => AdminReply::Failed {
                reason: "the player is dead or moving between sims".to_owned(),
            },
        }
    }

    fn god_mode(&mut self, entity: u64, on: bool) -> AdminReply {
        let Some(target) = self.player_by_number(entity) else {
            return AdminReply::NotMine;
        };
        let mut player = self.world.entity_mut(target);
        if on {
            player.insert(GodMode);
        } else {
            player.remove::<GodMode>();
        }
        AdminReply::Done
    }

    fn give_item(&mut self, entity: u64, item_id: &str, quantity: u32) -> AdminReply {
        let Some(target) = self.player_by_number(entity) else {
            return AdminReply::NotMine;
        };
        let Some(item) = self.content.kits.item(item_id).cloned() else {
            return AdminReply::Failed {
                reason: format!("no item {item_id}"),
            };
        };
        let given = self
            .world
            .get_mut::<Player>(target)
            .is_some_and(|mut player| {
                player
                    .gear
                    .as_mut()
                    .is_some_and(|gear| gear.add(&item, quantity))
            });
        if given {
            AdminReply::Done
        } else {
            AdminReply::Failed {
                reason: "the bag is full or not loaded".to_owned(),
            }
        }
    }

    fn take_item(&mut self, entity: u64, item_id: &str) -> AdminReply {
        let Some(target) = self.player_by_number(entity) else {
            return AdminReply::NotMine;
        };
        let taken = self
            .world
            .get_mut::<Player>(target)
            .is_some_and(|mut player| {
                player
                    .gear
                    .as_mut()
                    .is_some_and(|gear| gear.remove(item_id))
            });
        if taken {
            AdminReply::Done
        } else {
            AdminReply::Failed {
                reason: format!("the player has no {item_id}"),
            }
        }
    }

    /// Spawns `count` GM mobs of `vitals` on a ring around `at`. The admin sends this to the
    /// sim that owns the cell of `at`.
    fn spawn_gm_mobs(&mut self, vitals: &str, level: u32, at: Point, count: u32) -> AdminReply {
        if !(LOWEST_LEVEL..=HIGHEST_LEVEL).contains(&level) {
            return AdminReply::Failed {
                reason: format!("level {level} is not {LOWEST_LEVEL} to {HIGHEST_LEVEL}"),
            };
        }
        if count == 0 || count > MAX_GM_SPAWN {
            return AdminReply::Failed {
                reason: format!("count {count} is not 1 to {MAX_GM_SPAWN}"),
            };
        }
        let mut entities = Vec::new();
        for (x, y) in ring(at.x, at.y, count) {
            let spawn = VitalsSpawn {
                vitals_id: lowercase_crc32(vitals),
                level,
                position: [x, y, at.z],
                ..VitalsSpawn::default()
            };
            match self.mobs.spawn_gm(spawn) {
                Some(entity) => entities.push(entity),
                None => {
                    return AdminReply::Failed {
                        reason: format!("no mob type {vitals} at level {level}"),
                    };
                }
            }
        }
        AdminReply::Spawned { entities }
    }

    fn despawn_gm_mobs(&mut self, entities: &[u64]) -> AdminReply {
        let targets = if entities.is_empty() {
            self.mobs.gm_mobs()
        } else {
            entities.to_vec()
        };
        let removed: Vec<u64> = targets
            .into_iter()
            .filter(|entity| self.mobs.remove_gm(*entity))
            .collect();
        if removed.is_empty() && !entities.is_empty() {
            AdminReply::NotMine
        } else {
            AdminReply::Done
        }
    }
}

/// `count` points on a ring around (`x`, `y`), [`GM_SPAWN_SPACING_M`] apart; one point is the
/// centre itself.
fn ring(x: f32, y: f32, count: u32) -> Vec<(f32, f32)> {
    if count == 1 {
        return vec![(x, y)];
    }
    let steps = count as f32;
    let radius = GM_SPAWN_SPACING_M * steps / std::f32::consts::TAU;
    (0..count)
        .map(|step| {
            let angle = std::f32::consts::TAU * step as f32 / steps;
            (x + radius * angle.cos(), y + radius * angle.sin())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use nw_bus::{AdminAction, AdminCommand, AdminReply, Point};
    use uuid::Uuid;

    use super::*;
    use crate::combat::test_support::{TEST_LEVEL, TEST_MOB};
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};

    const PLAYER: u64 = 41;
    const TARGET: Point = Point {
        x: 500.0,
        y: 600.0,
        z: 30.0,
    };

    fn command(action: AdminAction) -> AdminCommand {
        AdminCommand {
            request: Uuid::from_u128(1),
            actor: None,
            action,
        }
    }

    fn teleport(entity: u64, to: Point) -> AdminCommand {
        command(AdminAction::TeleportPlayer { entity, to })
    }

    fn admitted() -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        world
    }

    fn health(world: &mut SimWorld) -> u32 {
        let target = world.player_by_number(PLAYER).expect("player");
        world
            .world
            .get::<Vitals>(target)
            .expect("vitals")
            .health
            .current()
    }

    #[test]
    fn a_teleport_moves_the_player() {
        let mut world = admitted();
        assert_eq!(
            world.admin(&teleport(PLAYER, TARGET), NOW),
            AdminReply::Done
        );
        let (players, _, _) = world.snapshot();
        let player = players
            .iter()
            .find(|player| player.entity == PLAYER)
            .expect("player");
        assert_eq!(
            (player.x, player.y, player.z),
            (TARGET.x, TARGET.y, TARGET.z)
        );
    }

    #[test]
    fn an_unknown_player_is_not_mine() {
        let mut world = world_with_spawns(Vec::new());
        assert_eq!(
            world.admin(&teleport(PLAYER, TARGET), NOW),
            AdminReply::NotMine
        );
        assert_eq!(
            world.admin(&command(AdminAction::Heal { entity: PLAYER }), NOW),
            AdminReply::NotMine
        );
    }

    #[test]
    fn a_target_off_the_level_fails() {
        let mut world = admitted();
        let off = Point { x: -10.0, ..TARGET };
        assert!(matches!(
            world.admin(&teleport(PLAYER, off), NOW),
            AdminReply::Failed { .. }
        ));
    }

    #[test]
    fn kill_then_heal_brings_the_player_back() {
        let mut world = admitted();
        let full = health(&mut world);
        assert!(full > 0);
        assert_eq!(
            world.admin(&command(AdminAction::Kill { entity: PLAYER }), NOW),
            AdminReply::Done
        );
        assert_eq!(health(&mut world), 0);
        assert_eq!(
            world.admin(&command(AdminAction::Heal { entity: PLAYER }), NOW),
            AdminReply::Done
        );
        assert_eq!(health(&mut world), full);
    }

    #[test]
    fn god_mode_stops_damage_until_it_is_turned_off() {
        let mut world = admitted();
        let full = health(&mut world);
        world.admin(
            &command(AdminAction::GodMode {
                entity: PLAYER,
                on: true,
            }),
            NOW,
        );
        assert_eq!(world.apply_hit(PLAYER, 1, 10, 0, NOW), None);
        assert_eq!(health(&mut world), full);
        world.admin(
            &command(AdminAction::GodMode {
                entity: PLAYER,
                on: false,
            }),
            NOW,
        );
        assert!(world.apply_hit(PLAYER, 1, 10, 0, NOW).is_some());
        assert!(health(&mut world) < full);
    }

    #[test]
    fn an_unknown_item_fails() {
        let mut world = admitted();
        assert!(matches!(
            world.admin(
                &command(AdminAction::GiveItem {
                    entity: PLAYER,
                    item: "NoSuchItem".to_owned(),
                    quantity: 1,
                }),
                NOW
            ),
            AdminReply::Failed { .. }
        ));
    }

    #[test]
    fn gm_mobs_spawn_on_a_ring_and_despawn() {
        let mut world = world_with_spawns(Vec::new());
        let spawned = world.admin(
            &command(AdminAction::SpawnMobs {
                vitals: TEST_MOB.to_owned(),
                level: TEST_LEVEL,
                at: TARGET,
                count: 3,
            }),
            NOW,
        );
        let AdminReply::Spawned { entities } = spawned else {
            panic!("spawned: {spawned:?}");
        };
        assert_eq!(entities.len(), 3);
        assert_eq!(world.mobs.gm_mobs(), entities);
        assert_eq!(
            world.admin(
                &command(AdminAction::DespawnMobs {
                    entities: Vec::new()
                }),
                NOW
            ),
            AdminReply::Done
        );
        assert!(world.mobs.gm_mobs().is_empty());
    }

    #[test]
    fn a_spawn_out_of_range_fails() {
        let mut world = world_with_spawns(Vec::new());
        for (level, count) in [(0, 1), (TEST_LEVEL, 0), (TEST_LEVEL, MAX_GM_SPAWN + 1)] {
            assert!(matches!(
                world.admin(
                    &command(AdminAction::SpawnMobs {
                        vitals: TEST_MOB.to_owned(),
                        level,
                        at: TARGET,
                        count,
                    }),
                    NOW
                ),
                AdminReply::Failed { .. }
            ));
        }
    }

    #[test]
    fn a_ring_keeps_its_spacing() {
        let points = ring(0.0, 0.0, 4);
        let (x0, y0) = points[0];
        let (x1, y1) = points[1];
        let apart = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        assert!(apart > GM_SPAWN_SPACING_M * 0.8 && apart < GM_SPAWN_SPACING_M * 1.2);
        assert_eq!(ring(5.0, 6.0, 1), [(5.0, 6.0)]);
    }
}
