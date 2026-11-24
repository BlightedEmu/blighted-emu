// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mob walk map: where a mob body may stand (the walk map plan, phase 1).
//!
//! The retail server walked its mobs on a navigation mesh that no file of the client holds
//! (findings `2026-11-24-walk-map-phase0.md`). This module is the rule of our own map: one place
//! is on the map when a body of the mob's size stands there on the level's ground. The rule
//! reads only [`Ground`], so the sim and the tool that checks the map against retail mob places
//! (`mapcheck`, the tools repository's `walk-map/`) use the same code.
//!
//! The sim's mobs walk on [`WalkMapGround`]: their ground with the map as one more reason why a
//! mob may not stand at a place ([`Ground::no_stand`]). The sim's floor checks already refuse a
//! step from a place where a mob may stand to one where it may not, and let a mob that stands in
//! such a place walk out, so the map needs no other rule of its own there. The layer is off
//! unless the sim's configuration turns it on.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::movement::{Ground, Position};

/// The distance between the points of the ground that the rule looks at around the feet (our
/// engineering limit: half of the common body radius of 0.5 m).
pub const RING_STEP_M: f32 = 0.25;
/// A body stands on a floor this near its feet (our engineering limit: the measure's own limit
/// for "on our floor").
pub const ON_FLOOR_M: f32 = 0.3;
/// The line of a body's side is this far over the step height, so that a ledge the mob may step
/// up is no wall for it (our engineering limit).
const OVER_STEP_M: f32 = 0.05;

/// The body of the sim's mob map: the values measured from retail mobs on our floor (findings
/// `2026-11-24-walk-map-phase0.md`): the radius of section 7 (with 0.25 m the map holds 99.33 to
/// 99.87 % of the places of moving retail mobs in 51 open world and 3 expedition captures; with
/// the slices' `m_agentRadius` of 0.5 m three of five sets are under 99 %), the step of section 4
/// (755,544 retail moves: 1,095 rise 0.25 to 0.30 m, 101 rise 0.30 to 0.35 m) and the slope of
/// section 7 (moving samples per 5 degrees: 797 at 45 to 50, then 237 and 74). The height is
/// Inferred (a standing humanoid): the retail builder's value is Unknown.
pub const MOB_BODY: Body = Body {
    radius_m: 0.25,
    height_m: 1.6,
    step_m: 0.3,
    max_slope_deg: 50.0,
};
/// The side of a cell of the map that [`WalkMapGround`] keeps (the rule's own grid step).
const CELL_M: f32 = RING_STEP_M;
/// The height of one layer of kept cells: a bridge and the ground under it are two layers (our
/// engineering limit: under the body height).
const LAYER_M: f32 = 1.0;
/// The most cells [`WalkMapGround`] keeps before it starts again (our engineering limit: about
/// 100 MB of answers).
const MAX_KEPT_CELLS: usize = 4_000_000;

/// The size and the limits of the body that a map is for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    /// Its radius: no solid and no ledge is this near its middle.
    pub radius_m: f32,
    /// Its height: no solid is over its feet up to here.
    pub height_m: f32,
    /// The highest ledge it steps up or down without a fall.
    pub step_m: f32,
    /// The steepest floor it stands on, degrees.
    pub max_slope_deg: f32,
}

/// The points of the ground around the feet that the rule looks at: every point of a grid of
/// [`RING_STEP_M`] within `radius_m`, without the middle, nearest first; each as its grid cell
/// (east, north).
fn ring(radius_m: f32) -> Vec<(i32, i32)> {
    // A handful of grid steps.
    #[allow(clippy::cast_possible_truncation, reason = "a handful of grid steps")]
    let reach = (radius_m / RING_STEP_M).floor() as i32;
    let mut cells: Vec<(i32, i32)> = (-reach..=reach)
        .flat_map(|row| (-reach..=reach).map(move |column| (column, row)))
        .filter(|cell| *cell != (0, 0) && offset_m(*cell).2 <= radius_m)
        .collect();
    cells.sort_by(|left, right| offset_m(*left).2.total_cmp(&offset_m(*right).2));
    cells
}

/// A grid cell's offset from the feet, east and north, and its distance, m.
#[allow(clippy::cast_precision_loss, reason = "a handful of grid steps")]
fn offset_m(cell: (i32, i32)) -> (f32, f32, f32) {
    let (dx, dy) = (cell.0 as f32 * RING_STEP_M, cell.1 as f32 * RING_STEP_M);
    (dx, dy, dx.hypot(dy))
}

/// Why a place is not on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Refusal {
    /// No floor is within [`ON_FLOOR_M`] of the feet.
    NoFloor,
    /// The feet are where a mob may not stand ([`Ground::no_stand`]).
    Closed,
    /// A solid is over the feet below the body's height.
    LowRoof,
    /// The ground has no floor at a point within the body's radius.
    NoFloorBeside,
    /// The floor at a point within the body's radius is higher or lower than a step and the
    /// slope allow: a ledge, a drop or a floor too steep.
    Ledge,
    /// A point within the body's radius is where a mob may not stand.
    ClosedBeside,
    /// A solid stands between the body's middle and a point within its radius.
    Wall,
}

/// Whether a body of the size `body` stands with its feet at `feet`: `Ok` with the floor's
/// height, or the first reason why not.
///
/// The rule, in this order:
/// 1. a floor is within [`ON_FLOOR_M`] of the feet, and the place is not closed for a mob;
/// 2. no solid is over the feet from the step height to the body's height;
/// 3. at each point of [`ring`]: the ground has a floor; from the floor of the grid point one
///    step nearer the feet, that floor is no more than a step up or down, or the slope's rise
///    over that step when it is more; the point is not closed for a mob; and no solid stands on
///    the lines from the body's middle to the point at the step height and at the body's height
///    (both lines follow the floor, so a slope is no wall).
pub fn stands(ground: &dyn Ground, feet: Position, body: &Body) -> Result<f32, Refusal> {
    let floor = ground
        .floor_at(feet.x, feet.y, feet.z)
        .filter(|floor| (feet.z - floor).abs() <= ON_FLOOR_M)
        .ok_or(Refusal::NoFloor)?;
    let middle = Position { z: floor, ..feet };
    if ground.no_stand(middle) {
        return Err(Refusal::Closed);
    }
    let at_height = |place: Position, height: f32| Position {
        z: place.z + height,
        ..place
    };
    let side_low = body.step_m + OVER_STEP_M;
    if ground
        .first_solid(
            at_height(middle, side_low),
            at_height(middle, body.height_m),
        )
        .is_some()
    {
        return Err(Refusal::LowRoof);
    }
    let rise_per_m = body.max_slope_deg.to_radians().tan();
    let mut floors: Vec<((i32, i32), f32)> = vec![((0, 0), floor)];
    for cell in ring(body.radius_m) {
        let (dx, dy, _) = offset_m(cell);
        let (x, y) = (feet.x + dx, feet.y + dy);
        let nearer = (cell.0 - cell.0.signum(), cell.1 - cell.1.signum());
        let from = floors
            .iter()
            .find(|(known, _)| *known == nearer)
            .map_or(floor, |(_, height)| *height);
        let beside = ground
            .floor_at(x, y, from + body.step_m)
            .ok_or(Refusal::NoFloorBeside)?;
        let hop_m = offset_m((cell.0 - nearer.0, cell.1 - nearer.1)).2;
        if (beside - from).abs() > body.step_m.max(rise_per_m * hop_m) {
            return Err(Refusal::Ledge);
        }
        let point = Position { x, y, z: beside };
        if ground.no_stand(point) {
            return Err(Refusal::ClosedBeside);
        }
        for height in [side_low, body.height_m] {
            if ground
                .first_solid(at_height(middle, height), at_height(point, height))
                .is_some()
            {
                return Err(Refusal::Wall);
            }
        }
        floors.push((cell, beside));
    }
    Ok(floor)
}

/// A ground with the walk map of `body` as one more reason why a mob may not stand at a place.
///
/// The map is made cell by cell as the mobs ask ([`CELL_M`] cells, one answer for each cell and
/// height layer): the rule runs at the cell's middle. A place with no floor near the feet is not
/// judged by the map (a mob in the air or on a deck over the height map keeps the sim's own
/// rules).
#[derive(Debug)]
pub struct WalkMapGround<G> {
    ground: G,
    body: Body,
    enabled: bool,
    cells: RwLock<HashMap<(i32, i32, i32), bool>>,
}

impl<G: Ground> WalkMapGround<G> {
    /// `ground` with the map of `body`; with `enabled` false it answers as `ground` alone.
    pub fn new(ground: G, body: Body, enabled: bool) -> Self {
        Self {
            ground,
            body,
            enabled,
            cells: RwLock::new(HashMap::new()),
        }
    }

    /// The ground without the map.
    pub const fn ground(&self) -> &G {
        &self.ground
    }

    /// Whether the map closes the cell of `feet` for a body (`false` where it does not judge).
    fn closed_by_the_map(&self, feet: Position) -> bool {
        // Cells of a level of some kilometres.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "cells of a level of some kilometres"
        )]
        let cell = (
            (feet.x / CELL_M).floor() as i32,
            (feet.y / CELL_M).floor() as i32,
            (feet.z / LAYER_M).round() as i32,
        );
        if let Some(closed) = self
            .cells
            .read()
            .ok()
            .and_then(|cells| cells.get(&cell).copied())
        {
            return closed;
        }
        // The middle of a cell of a level of some kilometres.
        #[allow(clippy::cast_precision_loss, reason = "a cell of a level")]
        let middle = Position {
            x: (cell.0 as f32 + 0.5) * CELL_M,
            y: (cell.1 as f32 + 0.5) * CELL_M,
            z: feet.z,
        };
        let closed = match stands(&self.ground, middle, &self.body) {
            Ok(_) | Err(Refusal::NoFloor) => false,
            Err(_) => true,
        };
        if let Ok(mut cells) = self.cells.write() {
            if cells.len() >= MAX_KEPT_CELLS {
                cells.clear();
            }
            cells.insert(cell, closed);
        }
        closed
    }
}

impl<G: Ground> Ground for WalkMapGround<G> {
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
        self.ground.wall(from, to)
    }

    fn first_solid(&self, from: Position, to: Position) -> Option<(Position, Option<u64>)> {
        self.ground.first_solid(from, to)
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

    fn no_stand(&self, feet: Position) -> bool {
        self.ground.no_stand(feet) || (self.enabled && self.closed_by_the_map(feet))
    }

    fn closed_by_walk_map(&self, feet: Position) -> bool {
        self.enabled && !self.ground.no_stand(feet) && self.closed_by_the_map(feet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUND_Z: f32 = 10.0;
    const BODY: Body = Body {
        radius_m: 0.5,
        height_m: 1.8,
        step_m: 0.3,
        max_slope_deg: 50.0,
    };
    const HERE: Position = Position {
        x: 100.0,
        y: 100.0,
        z: GROUND_Z,
    };

    /// Flat ground with a block from `EDGE_EAST_M` east of [`HERE`] on: its top is `rise` m up.
    struct Block {
        rise: f32,
    }

    const EDGE_EAST_M: f32 = 2.0;

    impl Ground for Block {
        fn height_at(&self, x: f32, _: f32) -> Option<f32> {
            Some(if x - HERE.x >= EDGE_EAST_M {
                GROUND_Z + self.rise
            } else {
                GROUND_Z
            })
        }
    }

    fn east(metres: f32) -> Position {
        Position {
            x: HERE.x + metres,
            ..HERE
        }
    }

    #[test]
    fn the_ring_holds_the_grid_points_within_the_radius() {
        let cells = ring(BODY.radius_m);
        assert_eq!(cells.len(), 12, "{cells:?}");
        assert!(cells.iter().all(|cell| offset_m(*cell).2 <= BODY.radius_m));
        assert_eq!(offset_m(cells[0]).2, RING_STEP_M, "nearest first");
        assert!(ring(0.2).is_empty(), "a body thinner than the grid");
    }

    #[test]
    fn a_body_stands_on_open_ground_and_not_within_its_radius_of_a_block() {
        const BRICKS_M: f32 = 0.9;
        let bricks = Block { rise: BRICKS_M };
        assert_eq!(stands(&bricks, HERE, &BODY), Ok(GROUND_Z));
        assert_eq!(
            stands(&bricks, east(EDGE_EAST_M - 0.6), &BODY),
            Ok(GROUND_Z),
            "more than its radius from the block"
        );
        assert_eq!(
            stands(&bricks, east(EDGE_EAST_M - 0.4), &BODY),
            Err(Refusal::Ledge),
            "the block is within its radius"
        );
    }

    #[test]
    fn a_step_and_a_slope_are_no_ledge() {
        let step = Block { rise: 0.25 };
        assert_eq!(stands(&step, east(EDGE_EAST_M - 0.1), &BODY), Ok(GROUND_Z));
        assert_eq!(
            stands(&step, east(EDGE_EAST_M + 0.1), &BODY).map(|floor| floor - GROUND_Z),
            Ok(0.25),
            "it stands on the step too"
        );
    }

    #[test]
    fn the_map_layer_closes_the_places_beside_a_block_only_when_it_is_on() {
        const BRICKS_M: f32 = 0.9;
        let beside = east(EDGE_EAST_M - 0.2);
        let on = WalkMapGround::new(Block { rise: BRICKS_M }, MOB_BODY, true);
        assert!(!on.no_stand(HERE), "open ground");
        assert!(on.no_stand(beside), "the block is within the body's radius");
        assert!(on.no_stand(beside), "the kept answer");
        assert!(on.closed_by_walk_map(beside));
        assert!(!on.closed_by_walk_map(HERE));
        let in_the_air = Position {
            z: GROUND_Z + 5.0,
            ..beside
        };
        assert!(
            !on.no_stand(in_the_air),
            "no floor near: the map does not judge"
        );
        let off = WalkMapGround::new(Block { rise: BRICKS_M }, MOB_BODY, false);
        assert!(!off.no_stand(beside), "off: the ground alone answers");
        assert!(!off.closed_by_walk_map(beside));
        assert_eq!(on.floor_at(HERE.x, HERE.y, HERE.z), Some(GROUND_Z));
    }

    #[test]
    fn a_place_in_the_air_or_closed_for_mobs_is_not_on_the_map() {
        struct Closed;
        impl Ground for Closed {
            fn height_at(&self, _: f32, _: f32) -> Option<f32> {
                Some(GROUND_Z)
            }
            fn no_stand(&self, feet: Position) -> bool {
                feet.x - HERE.x >= EDGE_EAST_M
            }
        }
        let in_the_air = Position {
            z: GROUND_Z + 1.0,
            ..HERE
        };
        assert_eq!(
            stands(&Block { rise: 0.0 }, in_the_air, &BODY),
            Err(Refusal::NoFloor)
        );
        assert_eq!(
            stands(&Closed, east(EDGE_EAST_M + 1.0), &BODY),
            Err(Refusal::Closed)
        );
        assert_eq!(
            stands(&Closed, east(EDGE_EAST_M - 0.3), &BODY),
            Err(Refusal::ClosedBeside)
        );
    }
}
