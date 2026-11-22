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
//!
//! The open world's settlement stations are such slices too ([`SliceSolids`]): the sim spawns
//! each at its slot's place ([`crate::world`] town stations), and their solids stand in every
//! player's ground there (Observed, retail towns session 2026-11-19 against the client's own
//! trace: 188 records whose client stood 0.4 to 0.5 m above the level's floor near (9584, 6336)
//! stood on the carpentry station `settlement_station_carpentry_t5.dynamicslice`, whose solids
//! the collision file keeps only as a deferred slice).
//!
//! The gates, bridges, blockers, pressure plates, teleporters and portal stones of an instance
//! level are such slices too ([`crate::world`] gates): the instance sim spawns each where its
//! level script's spawner places it, and while it stands its solids stand in every player's
//! ground there, those of its `Healthy` or `Depleted` entity by its gatherable state
//! ([`nw_world::collision::SliceState`]; level-collision note section 5.2: retail players stood
//! on the Shattered Mountain throne bridge at 1,345 own places while its entity was healthy, on
//! the Amrine light bridge at 116 while it was depleted).

use std::sync::Arc;

use nw_content::ftue_entities::AssetEntity;
use nw_world::collision::{Body, DeferredSlice, LevelCollision, SliceState};
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
                    .map(|deferred| spawned_bodies(deferred, SliceState::Healthy))
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
        solids_index(self.level.as_ref(), bodies)
    }
}

/// The shapes of `deferred` that stand while its entity is in `state`.
fn spawned_bodies(deferred: &DeferredSlice, state: SliceState) -> Vec<Body> {
    deferred.bodies_in(state).copied().collect()
}

/// The index of `bodies` over the shapes of `level`; `None` without bodies or a level.
fn solids_index(
    level: Option<&Arc<CollisionIndex>>,
    bodies: Vec<Body>,
) -> Option<Arc<CollisionIndex>> {
    if bodies.is_empty() {
        return None;
    }
    level.map(|level| Arc::new(level.with_bodies(bodies)))
}

/// The solids of the deferred slices a spawner of the level places, by slice and place: the
/// settlement stations the open world's sim spawns.
#[derive(Debug, Clone, Default)]
pub struct SliceSolids {
    /// The level's collision, whose shapes the solids use.
    level: Option<Arc<CollisionIndex>>,
    /// The level's deferred slices.
    deferred: Arc<LevelCollision>,
}

/// Two are equal when their slices are: the level index only lends them its shapes.
impl PartialEq for SliceSolids {
    fn eq(&self, other: &Self) -> bool {
        self.deferred == other.deferred
    }
}

impl SliceSolids {
    /// The deferred slices of `collision`, whose index is `level`.
    pub fn of(collision: &LevelCollision, level: &Arc<CollisionIndex>) -> Self {
        let deferred: Vec<DeferredSlice> = collision.deferred.clone();
        Self {
            level: Some(Arc::clone(level)),
            deferred: Arc::new(LevelCollision {
                deferred,
                ..LevelCollision::default()
            }),
        }
    }

    /// The solids of the slice `slice` / `sub_id` that a spawner places at `at`, as it spawns
    /// (healthy); empty when the level has none there ([`ASSET_MATCH_M`]).
    pub fn bodies_of(&self, slice: [u8; 16], sub_id: u32, at: [f32; 3]) -> Vec<Body> {
        self.bodies_in(slice, sub_id, at, SliceState::Healthy)
    }

    /// The solids of the slice `slice` / `sub_id` that a spawner places at `at` while its entity
    /// is in `state`; empty when the level has none there ([`ASSET_MATCH_M`]).
    pub fn bodies_in(
        &self,
        slice: [u8; 16],
        sub_id: u32,
        at: [f32; 3],
        state: SliceState,
    ) -> Vec<Body> {
        self.deferred
            .deferred_at(slice, sub_id, at, ASSET_MATCH_M)
            .map(|deferred| spawned_bodies(deferred, state))
            .unwrap_or_default()
    }

    /// The solids `bodies` as a ground's structures; `None` without any.
    pub fn index(&self, bodies: Vec<Body>) -> Option<Arc<CollisionIndex>> {
        solids_index(self.level.as_ref(), bodies)
    }
}

/// The solids a sim shows now (the standing slices of an instance), as the players' ground
/// has them: the index is made again only when the solids change.
#[derive(Debug, Clone, Default)]
pub struct ShownSolids {
    bodies: Vec<Body>,
    index: Option<Arc<CollisionIndex>>,
}

/// Two are equal when their solids are: the index only places them.
impl PartialEq for ShownSolids {
    fn eq(&self, other: &Self) -> bool {
        self.bodies == other.bodies
    }
}

impl ShownSolids {
    /// Shows `bodies` over the shapes of `solids`.
    pub fn show(&mut self, solids: &SliceSolids, bodies: Vec<Body>) {
        if bodies != self.bodies {
            self.index = solids.index(bodies.clone());
            self.bodies = bodies;
        }
    }

    /// The shown solids as a ground's structures; `None` without any.
    pub const fn index(&self) -> Option<&Arc<CollisionIndex>> {
        self.index.as_ref()
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
                healthy: Vec::new(),
                depleted: Vec::new(),
            }],
            volumes: Vec::new(),
            ..LevelCollision::default()
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
    fn a_slice_spawned_at_its_spawner_s_place_has_its_solids() {
        let collision = level();
        let index = Arc::new(CollisionIndex::new(collision.clone()));
        let solids = SliceSolids::of(&collision, &index);
        let bodies = solids.bodies_of([7; 16], 2, [10.0, 0.0, 0.01]);
        assert_eq!(bodies.len(), 1);
        assert!(solids.bodies_of([7; 16], 2, [40.0, 0.0, 0.0]).is_empty());
        assert!(solids.index(Vec::new()).is_none());
        let ground = solids.index(bodies).expect("solids");
        assert!((ground.floor_at(10.0, 0.0, 1.0).expect("top") - 1.0).abs() < 1e-4);
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
