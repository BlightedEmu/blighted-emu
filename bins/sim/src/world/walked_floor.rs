// SPDX-License-Identifier: AGPL-3.0-or-later
//! The floor the players walked on: every move a client sends went through the client's own walls
//! and stood on the client's own floors. So a spot a player stood on, at that height, is open
//! floor, and a server push (a mob's shove or pull) goes only as far along its line as such floor
//! reaches, and lands on it, and it stops at a wall of the level's collision on the way. A push
//! through a wall ends at the wall (live 2026-09-18: Thorpe's shockwave pushed the player 5 m
//! through the arena wall into a pocket), and a pull never lands under a platform (live
//! 2026-09-18: his grab pulled a player who stood in the water at 19 m under his 22.2 m platform,
//! stuck).
//!
//! The floors themselves are the level collision's ([`nw_sim_core::LevelGround`]), with the solids
//! of the tutorial's shown asset entities (the arena's fog-wall gate, the boat wreck's
//! `AI_Wall_Collision` box: [`crate::asset_solids`]). The walked floor stays for pushes because
//! the collision file does not hold every solid the client stands on or stops at
//! (level-collision note 2026-09-20: solids that name no shape, assets the build lacks, variant
//! capitals, vegetation, housing, and the open world's later slices, which no sim shows yet);
//! the walked floor is what the clients' own moves proved open.

use std::collections::HashMap;

use nw_sim_core::character::STEP_UP_M;
use nw_sim_core::{Ground, Position};

/// The side of one floor cell (our choice: about a body's width).
const CELL_M: f32 = 1.0;
/// The most cells kept (our choice: a few MB; a sim's players cover far fewer).
const MAX_CELLS: usize = 1_000_000;
/// The most floor heights one cell keeps (our choice: a few stacked floors).
const MAX_FLOORS_PER_CELL: usize = 8;
/// Heights nearer than this are one floor (our choice: the ground under one floor is uneven).
const FLOOR_MERGE_M: f32 = 0.5;
/// How far above or below its line a push may find a floor (Inferred: about a step up or a
/// short drop of a body).
const FLOOR_REACH_M: f32 = 1.5;
/// How far apart the points a push checks on its line are (our choice: a quarter cell).
const PATH_STEP_M: f32 = 0.25;

/// The floors the players walked on: the heights stood at, per cell.
#[derive(Debug, Default)]
pub(super) struct WalkedFloor {
    cells: HashMap<(i32, i32), Vec<f32>>,
}

/// The cell of `at`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "level coordinates of a few thousand metres"
)]
fn cell_of(at: Position) -> (i32, i32) {
    (
        (at.x / CELL_M).floor() as i32,
        (at.y / CELL_M).floor() as i32,
    )
}

impl WalkedFloor {
    /// A player stood on `at`.
    pub(super) fn walked(&mut self, at: Position) {
        let cell = cell_of(at);
        if !self.cells.contains_key(&cell) && self.cells.len() >= MAX_CELLS {
            return;
        }
        let floors = self.cells.entry(cell).or_default();
        if floors.len() < MAX_FLOORS_PER_CELL
            && floors
                .iter()
                .all(|floor| (floor - at.z).abs() > FLOOR_MERGE_M)
        {
            floors.push(at.z);
        }
    }

    /// The walked floor of the cell of `at` nearest its height, within [`FLOOR_REACH_M`].
    pub(super) fn floor_at(&self, at: Position) -> Option<f32> {
        self.cells
            .get(&cell_of(at))?
            .iter()
            .copied()
            .filter(|floor| (floor - at.z).abs() <= FLOOR_REACH_M)
            .min_by(|a, b| (a - at.z).abs().total_cmp(&(b - at.z).abs()))
    }

    /// The farthest point of the straight line from `from` to `to` that a body reaches over
    /// walked floor only, standing on that floor, with no wall of `ground` a step above that
    /// floor on the way: `to` (on its floor) when the whole line is open walked floor near its
    /// height, else the last point before the first one that is not (`from` when that is the
    /// first step).
    pub(super) fn reach(
        &self,
        from: Position,
        to: Position,
        ground: &(impl Ground + ?Sized),
    ) -> Position {
        let length = to.horizontal_distance(from);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a push of a few metres"
        )]
        let steps = (length / PATH_STEP_M).ceil() as u32;
        let mut reached = from;
        for step in 1..=steps {
            #[allow(clippy::cast_precision_loss, reason = "a few steps")]
            let share = (step as f32 * PATH_STEP_M / length).min(1.0);
            let point = Position {
                x: from.x + (to.x - from.x) * share,
                y: from.y + (to.y - from.y) * share,
                z: from.z + (to.z - from.z) * share,
            };
            let Some(floor) = self.floor_at(point) else {
                return reached;
            };
            let knee = |at: Position| Position {
                z: at.z + STEP_UP_M,
                ..at
            };
            let stand = Position { z: floor, ..point };
            if ground.blocked(knee(reached), knee(stand)) {
                return reached;
            }
            reached = stand;
        }
        reached
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_sim_core::test_support::NoGround;

    const START: Position = Position {
        x: 480.5,
        y: 210.5,
        z: 22.2,
    };

    fn east(metres: f32) -> Position {
        Position {
            x: START.x + metres,
            ..START
        }
    }

    #[test]
    fn a_push_goes_only_over_walked_floor() {
        const WALKED_M: f32 = 3.0;
        const PUSH_M: f32 = 5.0;
        let mut floor = WalkedFloor::default();
        let mut metres = 0.0;
        while metres <= WALKED_M {
            floor.walked(east(metres));
            metres += PATH_STEP_M;
        }
        let reached = floor.reach(START, east(PUSH_M), &NoGround);
        assert!(reached.x <= east(WALKED_M + CELL_M).x, "stops at the wall");
        assert!(reached.x >= east(WALKED_M).x, "goes over the floor walked");
        assert!(
            floor.reach(START, east(-PUSH_M), &NoGround).x >= east(-CELL_M).x,
            "nobody walked west of its cell"
        );
        assert_eq!(
            floor.reach(START, east(1.0), &NoGround),
            east(1.0),
            "a short push lands"
        );
    }

    #[test]
    fn a_push_stops_at_a_wall_of_the_level() {
        /// A wall of the level 2 m east of the start.
        const WALL_M: f32 = 2.0;
        struct Wall;
        impl Ground for Wall {
            fn height_at(&self, _: f32, _: f32) -> Option<f32> {
                None
            }

            fn blocked(&self, from: Position, to: Position) -> bool {
                from.x < START.x + WALL_M && to.x >= START.x + WALL_M
            }
        }
        let mut floor = WalkedFloor::default();
        let mut metres = 0.0;
        while metres <= 5.0 {
            floor.walked(east(metres));
            metres += PATH_STEP_M;
        }
        let reached = floor.reach(START, east(5.0), &Wall);
        assert!(reached.x < east(WALL_M).x, "{reached:?}");
        assert!(
            reached.x >= east(WALL_M - PATH_STEP_M).x - 1e-3,
            "{reached:?}"
        );
    }

    #[test]
    fn a_pull_never_lands_under_a_platform() {
        /// The water a player stood in, and a platform 3.2 m above it.
        const WATER_Z: f32 = 19.0;
        const PLATFORM_Z: f32 = 22.2;
        let at = |metres: f32, z: f32| Position { z, ..east(metres) };
        let mut floor = WalkedFloor::default();
        // The water under the whole line, and the platform's top at its far end.
        let mut metres = 0.0;
        while metres <= 4.0 {
            floor.walked(at(metres, WATER_Z));
            metres += PATH_STEP_M;
        }
        floor.walked(at(4.0, PLATFORM_Z));
        // Pulled toward a mob on the platform: never onto the water floor under it at the end.
        let reached = floor.reach(at(0.0, WATER_Z), at(4.0, PLATFORM_Z), &NoGround);
        assert!(
            (reached.z - WATER_Z).abs() < f32::EPSILON
                || (reached.z - PLATFORM_Z).abs() < f32::EPSILON,
            "it stands on a walked floor: {reached:?}"
        );
        assert!(
            !(reached.x >= east(4.0).x && (reached.z - WATER_Z).abs() < f32::EPSILON),
            "not in the water under the platform"
        );
        // A floor nobody stood on at that height is not floor.
        let mut high_only = WalkedFloor::default();
        high_only.walked(at(0.0, PLATFORM_Z));
        high_only.walked(at(1.0, PLATFORM_Z));
        assert_eq!(
            high_only.reach(at(0.0, WATER_Z), at(1.0, WATER_Z), &NoGround),
            at(0.0, WATER_Z),
            "no floor at the water's height"
        );
    }
}
