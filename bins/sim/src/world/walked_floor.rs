// SPDX-License-Identifier: AGPL-3.0-or-later
//! The floor the players walked on: the server has no level collision, but every move a client
//! sends went through the client's own walls. So a spot a player stood on is open floor, and a
//! server push (a mob's shove or pull) goes only as far as such floor reaches along its line. A
//! push through a wall ends at the wall (live 2026-09-18: Thorpe's shockwave pushed the player 5 m
//! through the arena wall into a pocket it could not leave).

use std::collections::HashSet;

use nw_sim_core::Position;

/// The side of one floor cell (our choice: about a body's width).
const CELL_M: f32 = 1.0;
/// The most cells kept (our choice: a few MB; a sim's players cover far fewer).
const MAX_CELLS: usize = 1_000_000;
/// How far apart the points a push checks on its line are (our choice: a quarter cell).
const PATH_STEP_M: f32 = 0.25;

/// The cells of the floor the players walked on.
#[derive(Debug, Default)]
pub(super) struct WalkedFloor {
    cells: HashSet<(i32, i32)>,
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
        if self.cells.len() < MAX_CELLS {
            self.cells.insert(cell_of(at));
        }
    }

    /// The farthest point of the straight line from `from` to `to` that a body reaches over
    /// walked floor only: `to` when the whole line is walked floor, else the last point before
    /// the first cell nobody walked on (`from` when that is the first step).
    pub(super) fn reach(&self, from: Position, to: Position) -> Position {
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
            if !self.cells.contains(&cell_of(point)) {
                return reached;
            }
            reached = point;
        }
        reached
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let reached = floor.reach(START, east(PUSH_M));
        assert!(reached.x <= east(WALKED_M + CELL_M).x, "stops at the wall");
        assert!(reached.x >= east(WALKED_M).x, "goes over the floor walked");
        assert!(
            floor.reach(START, east(-PUSH_M)).x >= east(-CELL_M).x,
            "nobody walked west of its cell"
        );
        assert_eq!(
            floor.reach(START, east(1.0)),
            east(1.0),
            "a short push lands"
        );
    }
}
