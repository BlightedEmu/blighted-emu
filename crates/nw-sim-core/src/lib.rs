// SPDX-License-Identifier: AGPL-3.0-or-later
//! World simulation rules of M4 (M4 spec section 7), with no I/O and no clock reads: the caller
//! passes positions and times in. `sim` runs them.

pub mod cells;
pub mod interest;
pub mod movement;
#[cfg(any(test, feature = "test-util"))]
pub mod test_support;
pub mod tick;

pub use cells::{CellGrid, CellGridError, CellId, CellOwners, MAX_CELLS, assign_columns};
pub use interest::{Interest, InterestChanges};
pub use movement::{
    CellBounds, Ground, GroundCheck, Motion, MoveLimits, MoveLimitsError, MoveRefusal, Position,
};
pub use tick::{DueTicks, TICKS_PER_SECOND, TickClock};
