// SPDX-License-Identifier: AGPL-3.0-or-later
//! GM commands that the admin tool sends to this sim (admin studio spec section 8.1).

use nw_bus::{AdminAction, AdminCommand, AdminReply, Point};
use nw_sim_core::Motion;

use super::{Body, Player, SimWorld};

impl SimWorld {
    /// Carries out `command` at `now_ms`. A target this sim does not own gives
    /// [`AdminReply::NotMine`].
    pub fn admin(&mut self, command: &AdminCommand, now_ms: u64) -> AdminReply {
        match &command.action {
            AdminAction::TeleportPlayer { entity, to } => {
                self.teleport_player(*entity, *to, now_ms)
            }
        }
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
}

#[cfg(test)]
mod tests {
    use nw_bus::{AdminAction, AdminCommand, AdminReply, Point};
    use uuid::Uuid;

    use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};

    const PLAYER: u64 = 41;
    const TARGET: Point = Point {
        x: 500.0,
        y: 600.0,
        z: 30.0,
    };

    fn teleport(entity: u64, to: Point) -> AdminCommand {
        AdminCommand {
            request: Uuid::from_u128(1),
            actor: None,
            action: AdminAction::TeleportPlayer { entity, to },
        }
    }

    #[test]
    fn a_teleport_moves_the_player() {
        let mut world = world_with_spawns(Vec::new());
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
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
    }

    #[test]
    fn a_target_off_the_level_fails() {
        let mut world = world_with_spawns(Vec::new());
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let off = Point { x: -10.0, ..TARGET };
        assert!(matches!(
            world.admin(&teleport(PLAYER, off), NOW),
            AdminReply::Failed { .. }
        ));
    }
}
