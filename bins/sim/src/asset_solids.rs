// SPDX-License-Identifier: AGPL-3.0-or-later
//! The solids of the tutorial's asset entities (level-collision note 2026-09-20): the slices an
//! encounter or event spawns after the level loads, whose solids the level collision file keeps
//! apart ([`nw_world::collision::DeferredSlice`]). While the server shows one to a player
//! ([`crate::world`] level assets), its solids stand in that player's ground: the boat-wreck
//! encounter's `AI_Wall_Collision` box on the deck and the Thorpe arena's fog-wall gate
//! (`ftue_v2_enc_thorpe_gate_fogwall.dynamicslice`, two gate meshes).
//!
//! An asset entity of the tutorial table names its slice and where it came into view; the
//! deferred slice of that slice whose spawner places it there is its own (Observed: the boat
//! wreck at 484.350, 848.598, 91.383 against the retail 484.350, 848.598, 91.366, the fog wall at
//! 474.000, 238.000, 23.000 against 474.000, 238.000, 22.985).

use std::sync::Arc;

use nw_content::ftue_entities::AssetEntity;
use nw_world::collision::{Body, LevelCollision};
use nw_world::collision_index::CollisionIndex;

/// How far an asset entity's place may be from its deferred slice's origin, m (our choice: the
/// retail places are within 0.02 m of them; deferred slices of one kind stand metres apart).
const ASSET_MATCH_M: f32 = 0.5;

/// The solids of each tutorial asset entity.
#[derive(Debug, Clone, Default)]
pub struct AssetSolids {
    /// The level's collision, whose shapes the solids use.
    level: Option<Arc<CollisionIndex>>,
    /// The placed shapes of each asset entity, by index in the level table.
    bodies: Vec<Vec<Body>>,
}

/// Two are equal when their solids are: the level index only lends them its shapes.
impl PartialEq for AssetSolids {
    fn eq(&self, other: &Self) -> bool {
        self.bodies == other.bodies
    }
}

impl AssetSolids {
    /// The solids of each of `assets` in `collision`, whose index is `level`.
    pub fn of(
        collision: &LevelCollision,
        level: &Arc<CollisionIndex>,
        assets: &[AssetEntity],
    ) -> Self {
        let bodies = assets
            .iter()
            .map(|asset| {
                asset
                    .position
                    .and_then(|at| {
                        collision.deferred_at(asset.asset, asset.asset_sub_id, at, ASSET_MATCH_M)
                    })
                    .map(|deferred| deferred.bodies.clone())
                    .unwrap_or_default()
            })
            .collect();
        Self {
            level: Some(Arc::clone(level)),
            bodies,
        }
    }

    /// The asset entities that have solids.
    pub fn solid_assets(&self) -> usize {
        self.bodies
            .iter()
            .filter(|bodies| !bodies.is_empty())
            .count()
    }

    /// The solids of the shown asset entities `shown` (indices in the level table); `None` when
    /// they have none.
    pub fn shown(&self, shown: impl Iterator<Item = usize>) -> Option<Arc<CollisionIndex>> {
        let bodies: Vec<Body> = shown
            .filter_map(|index| self.bodies.get(index))
            .flatten()
            .copied()
            .collect();
        if bodies.is_empty() {
            return None;
        }
        self.level
            .as_ref()
            .map(|level| Arc::new(level.with_bodies(bodies)))
    }
}

#[cfg(test)]
mod tests {
    use nw_world::collision::{DeferredSlice, Geometry, IDENTITY_AFFINE};

    use super::*;

    /// A fog wall: a box 1 m high deferred slice at (10, 0, 0).
    fn level() -> LevelCollision {
        let mut place = IDENTITY_AFFINE;
        place[0][3] = 10.0;
        place[2][3] = 0.5;
        LevelCollision {
            level: "ftue_v2".to_owned(),
            geometries: vec![Geometry::Box {
                half_extents: [0.25, 5.0, 0.5],
            }],
            bodies: Vec::new(),
            deferred: vec![DeferredSlice {
                slice: [7; 16],
                sub_id: 2,
                origin: [10.0, 0.0, 0.0],
                bodies: vec![Body { geometry: 0, place }],
            }],
        }
    }

    fn asset(at: [f32; 3]) -> AssetEntity {
        AssetEntity {
            asset: [7; 16],
            asset_sub_id: 2,
            retail_gde_ref: [0; 16],
            position: Some(at),
            spawns: Vec::new(),
            changes: Vec::new(),
            despawns: Vec::new(),
        }
    }

    #[test]
    fn a_shown_asset_s_slice_stands_in_the_ground() {
        let collision = level();
        let index = Arc::new(CollisionIndex::new(collision.clone()));
        let solids = AssetSolids::of(
            &collision,
            &index,
            &[asset([10.0, 0.0, 0.01]), asset([40.0, 0.0, 0.0])],
        );
        assert_eq!(solids.solid_assets(), 1, "the far one is no fog wall");
        assert!(solids.shown([1].into_iter()).is_none());
        let shown = solids.shown([0].into_iter()).expect("shown");
        assert!((shown.floor_at(10.0, 0.0, 1.0).expect("top") - 1.0).abs() < 1e-4);
        assert!(
            index.floor_at(10.0, 0.0, 1.0).is_none(),
            "not in the level itself"
        );
    }
}
