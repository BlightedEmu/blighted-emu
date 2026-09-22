// SPDX-License-Identifier: AGPL-3.0-or-later
//! A level's ground with its structures (level-collision note 2026-09-20): the terrain of the
//! world file and the solid shapes of the collision file ([`nw_world::collision`]). The floor
//! under a body is the higher of the two: a deck, a roof, stairs or a bridge over the terrain, as
//! the client's character stands on them; a wall is a solid on a line at a body's height. A
//! level ground can stand on another (the solids the server shows for one player over the
//! level's own). The level's roads ([`nw_world::roads`]) and water ([`nw_world::water`]) go with
//! it: the road share and the water surface of a place are the maps' where the level ground holds
//! them, else its terrain's.

use std::sync::Arc;

use nw_world::collision_index::{CollisionIndex, FLOOR_SEARCH_ABOVE_M, MAX_WALKABLE_SLOPE_DEG};
use nw_world::roads::RoadMap;
use nw_world::water::WaterMap;

use crate::movement::{Ground, Position};

/// The terrain of `terrain` and the structures of `collision`.
#[derive(Debug, Clone)]
pub struct LevelGround<T> {
    terrain: T,
    collision: Arc<CollisionIndex>,
    roads: Option<Arc<RoadMap>>,
    water: Option<Arc<WaterMap>>,
}

impl<T: Ground> LevelGround<T> {
    /// The ground of `terrain` with the structures of `collision`.
    pub const fn new(terrain: T, collision: Arc<CollisionIndex>) -> Self {
        Self {
            terrain,
            collision,
            roads: None,
            water: None,
        }
    }

    /// The same ground with the water of `water`.
    #[must_use]
    pub fn with_water(self, water: Arc<WaterMap>) -> Self {
        Self {
            water: Some(water),
            ..self
        }
    }

    /// The same ground with the roads of `roads`.
    #[must_use]
    pub fn with_roads(self, roads: Arc<RoadMap>) -> Self {
        Self {
            roads: Some(roads),
            ..self
        }
    }

    /// The terrain.
    pub const fn terrain(&self) -> &T {
        &self.terrain
    }

    /// The structures.
    pub fn collision(&self) -> &CollisionIndex {
        &self.collision
    }
}

impl<T: Ground> Ground for LevelGround<T> {
    fn height_at(&self, x: f32, y: f32) -> Option<f32> {
        self.terrain.height_at(x, y)
    }

    /// The structure's floor under the feet ([`CollisionIndex::floor_at`]) when it is above the
    /// terrain, else the terrain; the terrain too far over the feet to be stepped onto is no
    /// floor where a structure is under them (a body under an overhang of the terrain).
    fn floor_at(&self, x: f32, y: f32, z: f32) -> Option<f32> {
        let terrain = self.terrain.floor_at(x, y, z);
        match (self.collision.floor_at(x, y, z), terrain) {
            (Some(structure), Some(terrain)) if terrain <= z + FLOOR_SEARCH_ABOVE_M => {
                Some(structure.max(terrain))
            }
            (Some(structure), _) => Some(structure),
            (None, terrain) => terrain,
        }
    }

    /// A wall: a wall of the terrain's ground, or the first solid surface of the structures on
    /// the line when it is too steep to stand on (a ramp or a stair's top on the line is floor,
    /// not wall).
    fn blocked(&self, from: Position, to: Position) -> bool {
        let min_up = MAX_WALKABLE_SLOPE_DEG.to_radians().cos();
        self.terrain.blocked(from, to)
            || self
                .collision
                .raycast([from.x, from.y, from.z], [to.x, to.y, to.z])
                .is_some_and(|hit| hit.normal[2] < min_up)
    }

    fn road_share(&self, x: f32, y: f32) -> f32 {
        self.roads.as_ref().map_or_else(
            || self.terrain.road_share(x, y),
            |roads| roads.road_share(x, y),
        )
    }

    fn water_surface(&self, x: f32, y: f32, sea_level_m: f32) -> Option<f32> {
        self.water.as_ref().map_or_else(
            || self.terrain.water_surface(x, y, sea_level_m),
            |water| water.surface_at(x, y),
        )
    }
}

#[cfg(test)]
mod tests {
    use nw_world::collision::{Body, Geometry, IDENTITY_AFFINE, LevelCollision};

    use super::*;
    use crate::test_support::FlatGround;

    const TERRAIN_Z: f32 = 50.0;
    const DECK_Z: f32 = 89.38;
    const TOLERANCE: f32 = 1e-4;

    /// A box deck 10 m square with its top at [`DECK_Z`], over flat terrain.
    fn ship() -> LevelGround<FlatGround> {
        let mut place = IDENTITY_AFFINE;
        place[2][3] = DECK_Z - 0.5;
        let collision = CollisionIndex::new(LevelCollision {
            level: "ftue_v2".to_owned(),
            geometries: vec![Geometry::Box {
                half_extents: [5.0, 5.0, 0.5],
            }],
            bodies: vec![Body { geometry: 0, place }],
            deferred: Vec::new(),
        });
        LevelGround::new(FlatGround(TERRAIN_Z), Arc::new(collision))
    }

    #[test]
    fn the_deck_is_the_floor_over_the_terrain_and_the_terrain_beside_it() {
        let ground = ship();
        let floor = ground.floor_at(1.0, 1.0, DECK_Z).expect("deck");
        assert!((floor - DECK_Z).abs() < TOLERANCE);
        assert_eq!(ground.floor_at(20.0, 1.0, DECK_Z), Some(TERRAIN_Z));
        assert_eq!(
            ground.floor_at(1.0, 1.0, TERRAIN_Z),
            Some(TERRAIN_Z),
            "under the deck"
        );
        assert_eq!(ground.height_at(1.0, 1.0), Some(TERRAIN_Z));
    }

    #[test]
    fn the_deck_s_side_is_a_wall_at_its_height() {
        let ground = ship();
        let at = |x: f32, z: f32| Position { x, y: 0.0, z };
        assert!(ground.blocked(at(-8.0, DECK_Z - 0.3), at(-2.0, DECK_Z - 0.3)));
        assert!(!ground.blocked(at(-8.0, DECK_Z + 0.3), at(-2.0, DECK_Z + 0.3)));
    }

    #[test]
    fn the_road_share_is_the_road_map_s_and_passes_through_a_level_ground_on_top() {
        use nw_world::roads::RoadRegion;
        /// Region (0, 0) of 2 x 2 cells over 4 m, all road.
        const REGION_M: f32 = 4.0;
        const EDGE: u32 = 2;
        const ROAD: u8 = 1;
        let roads = Arc::new(RoadMap {
            level: "ftue_v2".to_owned(),
            region_size_m: REGION_M,
            regions: vec![RoadRegion::from_tracts(0, 0, EDGE, &[ROAD; 4], ROAD).expect("square")],
        });
        assert!(ship().road_share(1.0, 1.0).abs() < TOLERANCE, "no road map");
        let ground = ship().with_roads(roads);
        assert!((ground.road_share(1.0, 1.0) - 1.0).abs() < TOLERANCE);
        assert!(
            ground.road_share(10.0, 1.0).abs() < TOLERANCE,
            "another region"
        );
        let shown = LevelGround::new(&ground, Arc::new(CollisionIndex::default()));
        assert!((shown.road_share(1.0, 1.0) - 1.0).abs() < TOLERANCE);
    }

    #[test]
    fn the_water_surface_is_the_water_map_s_and_the_sea_without_one() {
        use nw_world::water::{WaterNode, WaterRegion};
        /// Region (0, 0) of 4 m, one lake leaf.
        const REGION_M: f32 = 4.0;
        const LAKE_M: f32 = 292.2249;
        const LAKE: u32 = 0x07;
        const SEA_M: f32 = 40.0;
        let lake = WaterNode {
            height: LAKE_M,
            flags: LAKE,
        };
        let water = Arc::new(WaterMap {
            level: "newworld_vitaeeterna".to_owned(),
            region_size_m: REGION_M,
            regions: vec![WaterRegion::from_nodes(0, 0, vec![lake]).expect("one leaf")],
        });
        assert_eq!(ship().water_surface(1.0, 1.0, SEA_M), Some(SEA_M), "no map");
        let ground = ship().with_water(water);
        assert_eq!(ground.water_surface(1.0, 1.0, SEA_M), Some(LAKE_M));
        assert_eq!(ground.water_surface(10.0, 1.0, SEA_M), None, "dry");
        let shown = LevelGround::new(&ground, Arc::new(CollisionIndex::default()));
        assert_eq!(shown.water_surface(1.0, 1.0, SEA_M), Some(LAKE_M));
    }
}
