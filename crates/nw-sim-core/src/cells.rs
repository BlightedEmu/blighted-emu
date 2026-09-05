// SPDX-License-Identifier: AGPL-3.0-or-later
//! The cells of a level and who owns them (M5 spec section 4), with no I/O.
//!
//! The grid covers the level from (0, 0). Cells are squares of `cell_size_m`; the last column and
//! row may be narrower, and the far edge of the level belongs to them. Owners get columns of
//! cells in stripes, so each owner has the same number of columns, plus or minus one.

use std::collections::BTreeSet;

use nw_world::records::LevelInfo;
use thiserror::Error;

use crate::movement::{CellBounds, Position};

/// Most cells in one grid; the director sends the owner of each cell in one bus message.
pub const MAX_CELLS: u64 = 1 << 16;

/// A grid value that is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CellGridError {
    /// A size is not a finite number above 0.
    #[error("level and cell sizes must be finite numbers above 0")]
    NotPositive,
    /// The cells are so small that the grid has too many.
    #[error("the grid would have more than {MAX_CELLS} cells")]
    TooManyCells,
}

/// One cell: its column (east) and row (north).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellId {
    /// Column, from x = 0.
    pub column: u32,
    /// Row, from y = 0.
    pub row: u32,
}

/// The cells of one level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellGrid {
    width_m: f32,
    height_m: f32,
    cell_size_m: f32,
    columns: u32,
    rows: u32,
}

impl CellGrid {
    /// The grid of a level of `width_m` by `height_m` with cells of `cell_size_m`.
    ///
    /// # Errors
    ///
    /// Returns [`CellGridError`] for a size that is not a finite number above 0, or a grid of
    /// more than [`MAX_CELLS`] cells.
    pub fn new(width_m: f32, height_m: f32, cell_size_m: f32) -> Result<Self, CellGridError> {
        let positive = |value: f32| value.is_finite() && value > 0.0;
        if !(positive(width_m) && positive(height_m) && positive(cell_size_m)) {
            return Err(CellGridError::NotPositive);
        }
        let count = |length: f32| (f64::from(length) / f64::from(cell_size_m)).ceil();
        let (columns, rows) = (count(width_m), count(height_m));
        if columns * rows > MAX_CELLS as f64 {
            return Err(CellGridError::TooManyCells);
        }
        Ok(Self {
            width_m,
            height_m,
            cell_size_m,
            // Both counts are at most MAX_CELLS, so they fit u32.
            columns: columns as u32,
            rows: rows as u32,
        })
    }

    /// The grid of a level from its world file frame: `region_size` times the region counts.
    ///
    /// # Errors
    ///
    /// Returns [`CellGridError`] as [`CellGrid::new`] does, for example for a level without
    /// regions.
    pub fn of_level(level: &LevelInfo, cell_size_m: f32) -> Result<Self, CellGridError> {
        let bounds = level_bounds(level);
        Self::new(bounds.max_x, bounds.max_y, cell_size_m)
    }

    /// Columns of cells.
    pub const fn columns(&self) -> u32 {
        self.columns
    }

    /// Rows of cells.
    pub const fn rows(&self) -> u32 {
        self.rows
    }

    /// Edge length of a cell, in metres.
    pub const fn cell_size_m(&self) -> f32 {
        self.cell_size_m
    }

    /// Cells in the grid.
    pub const fn cell_count(&self) -> usize {
        self.columns as usize * self.rows as usize
    }

    /// The area of the level.
    pub const fn bounds(&self) -> CellBounds {
        CellBounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: self.width_m,
            max_y: self.height_m,
        }
    }

    /// The cell at (`x`, `y`), or `None` outside the level.
    pub fn cell_of(&self, x: f32, y: f32) -> Option<CellId> {
        if !((0.0..=self.width_m).contains(&x) && (0.0..=self.height_m).contains(&y)) {
            return None;
        }
        Some(CellId {
            column: self.line_of(x, self.columns),
            row: self.line_of(y, self.rows),
        })
    }

    /// The place of `cell` in a list of every cell, row by row.
    pub const fn index(&self, cell: CellId) -> usize {
        cell.row as usize * self.columns as usize + cell.column as usize
    }

    /// The cells with a point within `radius_m` of (`x`, `y`) in the ground plane.
    pub fn cells_within(&self, x: f32, y: f32, radius_m: f32) -> Vec<CellId> {
        let first = |centre: f32, lines: u32| self.line_of((centre - radius_m).max(0.0), lines);
        let last = |centre: f32, lines: u32, length: f32| {
            self.line_of((centre + radius_m).min(length), lines)
        };
        if !(x.is_finite() && y.is_finite() && radius_m.is_finite()) {
            return Vec::new();
        }
        let columns = first(x, self.columns)..=last(x, self.columns, self.width_m);
        let rows = first(y, self.rows)..=last(y, self.rows, self.height_m);
        rows.flat_map(|row| columns.clone().map(move |column| CellId { column, row }))
            .filter(|cell| self.distance_to(*cell, x, y) <= radius_m)
            .collect()
    }

    /// The column or row of `value`; the far edge belongs to the last one.
    fn line_of(&self, value: f32, lines: u32) -> u32 {
        // `value` is at least 0 here, so the cast floors it; the result is capped.
        ((value / self.cell_size_m) as u32).min(lines.saturating_sub(1))
    }

    /// Distance from (`x`, `y`) to the nearest point of `cell`.
    fn distance_to(&self, cell: CellId, x: f32, y: f32) -> f32 {
        let span = |line: u32, length: f32| {
            let start = line as f32 * self.cell_size_m;
            (start, (start + self.cell_size_m).min(length))
        };
        let gap = |value: f32, (start, end): (f32, f32)| (start - value).max(value - end).max(0.0);
        let dx = gap(x, span(cell.column, self.width_m));
        let dy = gap(y, span(cell.row, self.height_m));
        dx.hypot(dy)
    }
}

/// The area of a level from its world file frame: `region_size` times the region counts.
pub fn level_bounds(level: &LevelInfo) -> CellBounds {
    let length = |regions: u32| (f64::from(level.region_size) * f64::from(regions)) as f32;
    CellBounds {
        min_x: 0.0,
        min_y: 0.0,
        max_x: length(level.grid_width),
        max_y: length(level.grid_height),
    }
}

/// The owner of each column: owner `i` of `owners` gets the columns from `floor(i * C / K)` up to
/// the next owner's first column. With more owners than columns, some owners get none.
pub fn assign_columns(columns: u32, owners: usize) -> Vec<usize> {
    if owners == 0 {
        return Vec::new();
    }
    let columns = columns as usize;
    (0..columns)
        .map(|column| {
            // The last owner whose first column is at or before this column.
            (0..owners)
                .rev()
                .find(|owner| owner * columns / owners <= column)
                .unwrap_or_default()
        })
        .collect()
}

/// The owner of every cell of a grid.
#[derive(Debug, Clone, PartialEq)]
pub struct CellOwners<O> {
    grid: CellGrid,
    cells: Vec<Option<O>>,
}

impl<O: Copy + Ord> CellOwners<O> {
    /// The cells of `grid` shared in column stripes over `owners`, in that order.
    pub fn columns(grid: CellGrid, owners: &[O]) -> Self {
        let by_column = assign_columns(grid.columns(), owners.len());
        let cells = (0..grid.cell_count())
            .map(|index| {
                let column = index % grid.columns() as usize;
                by_column.get(column).map(|owner| owners[*owner])
            })
            .collect();
        Self { grid, cells }
    }

    /// Owners from a list of every cell, row by row; `None` when the list does not fit the grid.
    pub fn from_cells(grid: CellGrid, cells: Vec<Option<O>>) -> Option<Self> {
        (cells.len() == grid.cell_count()).then_some(Self { grid, cells })
    }

    /// The owner of every cell, row by row.
    pub fn cells(&self) -> &[Option<O>] {
        &self.cells
    }

    /// The grid.
    pub const fn grid(&self) -> &CellGrid {
        &self.grid
    }

    /// The owner of the cell at `position`.
    pub fn owner_at(&self, position: Position) -> Option<O> {
        let cell = self.grid.cell_of(position.x, position.y)?;
        self.owner_of(cell)
    }

    /// The owner that must take a player of `me` at `position`: another owner owns its cell, and
    /// no cell of `me` lies within `margin_m`.
    pub fn handoff_target(&self, me: O, position: Position, margin_m: f32) -> Option<O> {
        let target = self.owner_at(position).filter(|owner| *owner != me)?;
        let near_own = self
            .grid
            .cells_within(position.x, position.y, margin_m)
            .into_iter()
            .any(|cell| self.owner_of(cell) == Some(me));
        (!near_own).then_some(target)
    }

    /// Owners other than `me` with a cell within `radius_m` of `position`.
    pub fn others_within(&self, me: O, position: Position, radius_m: f32) -> BTreeSet<O> {
        self.grid
            .cells_within(position.x, position.y, radius_m)
            .into_iter()
            .filter_map(|cell| self.owner_of(cell))
            .filter(|owner| *owner != me)
            .collect()
    }

    fn owner_of(&self, cell: CellId) -> Option<O> {
        self.cells.get(self.grid.index(cell)).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// 7 x 7 regions of 2048 m (`newworld_vitaeeterna`).
    const LEVEL_M: f32 = 14_336.0;
    const CELL_M: f32 = 2048.0;
    const COLUMNS: u32 = 7;
    const MARGIN_M: f32 = 5.0;
    const RADIUS_M: f32 = 100.0;

    fn grid() -> CellGrid {
        CellGrid::new(LEVEL_M, LEVEL_M, CELL_M).expect("grid")
    }

    fn at(x: f32, y: f32) -> Position {
        Position { x, y, z: 0.0 }
    }

    #[test]
    fn cells_cover_the_level_and_the_far_edge() {
        let grid = grid();
        assert_eq!((grid.columns(), grid.rows()), (COLUMNS, COLUMNS));
        assert_eq!(grid.cell_count(), 49);
        assert_eq!(grid.cell_of(0.0, 0.0), Some(CellId { column: 0, row: 0 }));
        assert_eq!(
            grid.cell_of(CELL_M, CELL_M - 1.0),
            Some(CellId { column: 1, row: 0 })
        );
        assert_eq!(
            grid.cell_of(LEVEL_M, LEVEL_M),
            Some(CellId { column: 6, row: 6 })
        );
        assert_eq!(grid.cell_of(-1.0, 0.0), None);
        assert_eq!(grid.cell_of(0.0, LEVEL_M + 1.0), None);
        assert_eq!(grid.cell_of(f32::NAN, 0.0), None);
        assert_eq!(grid.index(CellId { column: 2, row: 1 }), 9);
        let bounds = grid.bounds();
        assert_eq!((bounds.max_x, bounds.max_y), (LEVEL_M, LEVEL_M));
        let uneven = CellGrid::new(LEVEL_M, LEVEL_M, 3000.0).expect("grid");
        assert_eq!(uneven.columns(), 5);
    }

    #[test]
    fn a_level_frame_gives_its_grid() {
        let level = LevelInfo {
            region_size: 2048,
            grid_width: 7,
            grid_height: 7,
            ..LevelInfo::default()
        };
        assert_eq!(CellGrid::of_level(&level, CELL_M), Ok(grid()));
        assert_eq!(level_bounds(&level), grid().bounds());
        let empty = LevelInfo::default();
        assert!(CellGrid::of_level(&empty, CELL_M).is_err());
    }

    #[test]
    fn bad_grid_values_are_refused() {
        for (width, height, cell) in [
            (0.0, LEVEL_M, CELL_M),
            (LEVEL_M, f32::NAN, CELL_M),
            (LEVEL_M, LEVEL_M, 0.0),
            (LEVEL_M, LEVEL_M, f32::INFINITY),
            (LEVEL_M, LEVEL_M, 0.001),
        ] {
            assert!(
                CellGrid::new(width, height, cell).is_err(),
                "{width} {height} {cell}"
            );
        }
    }

    #[test]
    fn columns_are_shared_in_stripes() {
        assert_eq!(assign_columns(COLUMNS, 1), vec![0; 7]);
        assert_eq!(assign_columns(COLUMNS, 2), vec![0, 0, 0, 1, 1, 1, 1]);
        assert_eq!(assign_columns(COLUMNS, 4), vec![0, 1, 1, 2, 2, 3, 3]);
        assert_eq!(assign_columns(COLUMNS, 7), vec![0, 1, 2, 3, 4, 5, 6]);
        let eight = assign_columns(COLUMNS, 8);
        assert_eq!(eight.len(), 7);
        assert!(eight.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(assign_columns(COLUMNS, 0).is_empty());
        let owners = CellOwners::columns(grid(), &['a', 'b']);
        assert_eq!(owners.owner_at(at(10.0, LEVEL_M - 1.0)), Some('a'));
        assert_eq!(owners.owner_at(at(LEVEL_M - 1.0, 10.0)), Some('b'));
        assert_eq!(owners.owner_at(at(-5.0, 10.0)), None);
        let nobody = CellOwners::<char>::columns(grid(), &[]);
        assert!(nobody.cells().iter().all(Option::is_none));
    }

    #[test]
    fn owners_rebuild_from_their_cells() {
        let owners = CellOwners::columns(grid(), &[1_u16, 2]);
        let copy = CellOwners::from_cells(grid(), owners.cells().to_vec()).expect("same size");
        assert_eq!(copy.cells(), owners.cells());
        assert!(CellOwners::from_cells(grid(), vec![Some(1_u16)]).is_none());
    }

    #[test]
    fn a_handoff_needs_the_margin() {
        let owners = CellOwners::columns(grid(), &['a', 'b']);
        // Column 3 (x 6144 to 8192) is the first column of `b`.
        let border = 3.0 * CELL_M;
        let y = 1000.0;
        assert_eq!(
            owners.handoff_target('a', at(border - 1.0, y), MARGIN_M),
            None
        );
        assert_eq!(
            owners.handoff_target('a', at(border + 1.0, y), MARGIN_M),
            None
        );
        assert_eq!(
            owners.handoff_target('a', at(border + MARGIN_M + 1.0, y), MARGIN_M),
            Some('b')
        );
        assert_eq!(
            owners.handoff_target('b', at(border + 100.0, y), MARGIN_M),
            None
        );
        assert_eq!(owners.handoff_target('a', at(-1.0, y), MARGIN_M), None);
    }

    #[test]
    fn others_within_the_radius_are_found() {
        let owners = CellOwners::columns(grid(), &['a', 'b', 'c', 'd']);
        // Columns: a 0, b 1-2, c 3-4, d 5-6. At x = 6144 + 50 (c), b is 50 m away.
        let near_b = at(3.0 * CELL_M + 50.0, 5000.0);
        assert_eq!(
            owners.others_within('c', near_b, RADIUS_M),
            BTreeSet::from(['b'])
        );
        let middle = at(4.0 * CELL_M, 5000.0);
        assert!(owners.others_within('c', middle, RADIUS_M).is_empty());
        let corner = at(CELL_M + 10.0, 10.0);
        assert_eq!(
            owners.others_within('b', corner, RADIUS_M),
            BTreeSet::from(['a'])
        );
        assert_eq!(
            owners.others_within('a', at(10.0, 10.0), RADIUS_M),
            BTreeSet::new()
        );
    }
}
