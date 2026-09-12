// SPDX-License-Identifier: AGPL-3.0-or-later
//! World simulation rules of M4 (M4 spec section 7), with no I/O and no clock reads: the caller
//! passes positions and times in. `sim` runs them.

pub mod brain;
pub mod cells;
pub mod combat;
pub mod grid;
pub mod interest;
pub mod movement;
pub mod new_player;
#[cfg(any(test, feature = "test-util"))]
pub mod test_support;
pub mod tick;

pub use brain::{
    Brain, BrainAction, BrainSettings, MobState, Seen, died, forget, hurt, step as brain_step,
};
pub use cells::{
    CellGrid, CellGridError, CellId, CellOwners, MAX_CELLS, assign_columns, level_bounds,
};
pub use combat::{
    Damage, Dice, Health, Hit, MIN_HIT, MobAttack, PlayerAttack, Threat, mob_hit, mob_max_health,
    player_hit,
};
pub use grid::SpatialGrid;
pub use interest::{Interest, InterestChanges};
pub use movement::{
    CellBounds, Ground, GroundCheck, Motion, MotionState, MoveLimits, MoveLimitsError, MoveRefusal,
    Position,
};
pub use new_player::{
    OPEN_WORLD_START, TUTORIAL_EXIT, TUTORIAL_EXIT_RADIUS_M, TUTORIAL_FOLDER, TUTORIAL_START,
    at_tutorial_exit,
};
pub use tick::{DueTicks, TICKS_PER_SECOND, TickClock};
