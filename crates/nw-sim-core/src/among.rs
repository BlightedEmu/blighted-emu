// SPDX-License-Identifier: AGPL-3.0-or-later
//! The ground with the characters standing on it: each creature's body is a capsule
//! (`AICharacterCollider_*` filters, which block the player's: `default.collisionfilters`;
//! [`nw_content::creature_animations::CreatureAnimation::collider_radius_m`]), and the player's
//! capsule does not walk or attack through one. A wall test that ends inside a capsule and goes
//! into it meets the capsule; one that starts inside leaves freely.

use crate::movement::{Ground, Position};

/// One character's body on the ground plane: the centre of its capsule and its radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterBody {
    /// East, m.
    pub x: f32,
    /// North, m.
    pub y: f32,
    /// The capsule's radius, m.
    pub radius_m: f32,
}

/// A ground with `characters` standing on it.
#[derive(Debug, Clone, Copy)]
pub struct AmongCharacters<'a, G: ?Sized> {
    /// The level's ground.
    pub ground: &'a G,
    /// The characters on it.
    pub characters: &'a [CharacterBody],
}

impl<G: Ground + ?Sized> AmongCharacters<'_, G> {
    /// The outward unit normal of the first character's capsule that the line from `from` to
    /// `to` ends in and goes into.
    fn character_wall(&self, from: Position, to: Position) -> Option<[f32; 2]> {
        self.characters.iter().find_map(|body| {
            let (to_x, to_y) = (to.x - body.x, to.y - body.y);
            let to_m = to_x.hypot(to_y);
            let from_m = (from.x - body.x).hypot(from.y - body.y);
            (to_m < body.radius_m && to_m < from_m && to_m > f32::EPSILON)
                .then(|| [to_x / to_m, to_y / to_m])
        })
    }
}

impl<G: Ground + ?Sized> Ground for AmongCharacters<'_, G> {
    fn height_at(&self, x: f32, y: f32) -> Option<f32> {
        self.ground.height_at(x, y)
    }

    fn floor_at(&self, x: f32, y: f32, z: f32) -> Option<f32> {
        self.ground.floor_at(x, y, z)
    }

    fn blocked(&self, from: Position, to: Position) -> bool {
        self.ground.blocked(from, to)
    }

    fn wall(&self, from: Position, to: Position) -> Option<[f32; 2]> {
        self.ground
            .wall(from, to)
            .or_else(|| self.character_wall(from, to))
    }

    fn road_share(&self, x: f32, y: f32) -> f32 {
        self.ground.road_share(x, y)
    }

    fn water_surface(&self, x: f32, y: f32, sea_level_m: f32) -> Option<f32> {
        self.ground.water_surface(x, y, sea_level_m)
    }

    fn water_flags(&self, x: f32, y: f32) -> Option<u32> {
        self.ground.water_flags(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;

    impl Ground for Flat {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
    }

    const AT: Position = Position {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    fn to(x: f32, y: f32) -> Position {
        Position { x, y, z: 0.0 }
    }

    #[test]
    fn a_line_into_a_character_meets_its_capsule_and_one_out_of_it_leaves() {
        let characters = [CharacterBody {
            x: 2.0,
            y: 0.0,
            radius_m: 0.5,
        }];
        let ground = AmongCharacters {
            ground: &Flat,
            characters: &characters,
        };
        assert_eq!(ground.wall(AT, to(1.8, 0.0)), Some([-1.0, 0.0]));
        assert_eq!(ground.wall(AT, to(1.0, 0.0)), None, "short of it");
        assert_eq!(ground.wall(AT, to(0.0, 1.0)), None, "beside it");
        assert_eq!(
            ground.wall(to(1.9, 0.0), to(1.6, 0.0)),
            None,
            "out of it from inside"
        );
        assert_eq!(ground.height_at(5.0, 5.0), Some(0.0), "the level's ground");
    }
}
