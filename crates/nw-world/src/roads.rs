// SPDX-License-Identifier: AGPL-3.0-or-later
//! The roads of a level (road study 2026-09-20): the cells of the level's region tract maps
//! (`sharedassets/coatlicue/<level>/regions/r_+XX_+YY/region.tractmap.tif`, 1024 x 1024 cells over
//! a region of `regionSize` 2048 m) whose tract is the one named [`ROAD_TRACT_NAME`] in the
//! level's `tracts.json`. The client's `RoadStateComponent` (`FUN_143d58850`) blends the four
//! cells around the player's place bilinearly and sums the weights of the road cells; that share
//! is what [`RoadMap::road_share`] gives. The road boost's `IfOnRoad` reads this share as it is,
//! with no delay (`FUN_142c41000`, `FUN_145c52f40`).
//!
//! The file is the shared header of `nw_content::container` with magic `NWRD`, then a postcard
//! body of one [`RoadMap`]. `content-tool roads` writes it.

use std::path::{Path, PathBuf};

use nw_content::container::{
    ContainerError, ContainerSpec, decode_value, encode_value, read_file, write_file,
};
use serde::{Deserialize, Serialize};

/// First 4 bytes of every road file.
pub const ROADS_MAGIC: [u8; 4] = *b"NWRD";
/// Current road file format version.
pub const ROADS_FORMAT_VERSION: u16 = 1;
/// Version of the record types. Raise it when a type in this module changes.
pub const ROADS_SCHEMA_VERSION: u32 = 1;
/// Header fields of the road file.
pub const ROADS_CONTAINER: ContainerSpec = ContainerSpec {
    magic: ROADS_MAGIC,
    format_version: ROADS_FORMAT_VERSION,
    schema_hash: ROADS_SCHEMA_VERSION,
};
/// The tract of the road cells (open world `tracts.json`: index 1, `mapCategory` `ROADS`; the
/// client finds it by this name).
pub const ROAD_TRACT_NAME: &str = "roads";
/// Half of a cell: the client insets a region's tract bounds by it (`FUN_146a0f410`,
/// `DAT_147fc1d30` 0.5).
const HALF: f64 = 0.5;
/// Bits of one word of a region's cell set.
const WORD_BITS: usize = u64::BITS as usize;

/// The road file path of one level: `<dir>/roads-<client_build>-<level>.bin`.
pub fn road_file_path(dir: &Path, client_build: &str, level: &str) -> PathBuf {
    dir.join(format!("roads-{client_build}-{level}.bin"))
}

/// The road cells of one region.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadRegion {
    /// The region's column (`XX` of `r_+XX_+YY`).
    pub x: u32,
    /// The region's row (`YY`).
    pub y: u32,
    /// Cells per side.
    pub edge: u32,
    /// One bit per cell, row 0 (the north edge) first, set for a road cell.
    pub cells: Vec<u64>,
}

impl RoadRegion {
    /// The road cells of region (`x`, `y`) from its tract map cells (`edge` rows of `edge` tract
    /// indices, row 0 first): those of tract `road`. `None` when the cells are not `edge` x
    /// `edge`.
    pub fn from_tracts(x: u32, y: u32, edge: u32, tracts: &[u8], road: u8) -> Option<Self> {
        let side = usize::try_from(edge).ok()?;
        if side == 0 || tracts.len() != side * side {
            return None;
        }
        let mut cells = vec![0_u64; tracts.len().div_ceil(WORD_BITS)];
        for (index, _) in tracts
            .iter()
            .enumerate()
            .filter(|(_, tract)| **tract == road)
        {
            cells[index / WORD_BITS] |= 1 << (index % WORD_BITS);
        }
        Some(Self { x, y, edge, cells })
    }

    /// Whether the cell in `column` and `row` is road.
    fn is_road(&self, column: usize, row: usize) -> bool {
        let index = row * self.edge as usize + column;
        self.cells
            .get(index / WORD_BITS)
            .is_some_and(|word| word >> (index % WORD_BITS) & 1 == 1)
    }

    /// Whether the region holds a road cell at all.
    pub fn has_road(&self) -> bool {
        self.cells.iter().any(|word| *word != 0)
    }
}

/// The road cells of one level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoadMap {
    /// The level folder name.
    pub level: String,
    /// The side of one region, m (`tracts.json` `regionSize`).
    pub region_size_m: f32,
    /// The regions with a road cell.
    pub regions: Vec<RoadRegion>,
}

impl RoadMap {
    /// The share of road at (`x`, `y`), 0 to 1, as the client's tract query reads it
    /// (`FUN_146a0f410`, `FUN_146a07d50`, `FUN_146a25fd0`): the region's bounds inset by half a
    /// cell on each side, `f = (local - half cell) x n / (size - cell)`, the four cells from
    /// `floor(f)` clamped to the map, bilinear weights, summed over the road cells. The client
    /// counts rows from the south; the file's row 0 is the north edge. 0 outside every region with
    /// a road.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "region and cell indices of a few thousand, checked not negative; a share from 0 to 1"
    )]
    pub fn road_share(&self, x: f32, y: f32) -> f32 {
        if !x.is_finite() || !y.is_finite() || self.region_size_m <= 0.0 {
            return 0.0;
        }
        let size = f64::from(self.region_size_m);
        let (x, y) = (f64::from(x), f64::from(y));
        let (region_x, region_y) = ((x / size).floor(), (y / size).floor());
        if region_x < 0.0 || region_y < 0.0 {
            return 0.0;
        }
        let (index_x, index_y) = (region_x as u32, region_y as u32);
        let Some(region) = self
            .regions
            .iter()
            .find(|region| region.x == index_x && region.y == index_y)
        else {
            return 0.0;
        };
        let edge = region.edge as usize;
        if edge == 0 {
            return 0.0;
        }
        let cells = edge as f64;
        let half_cell = size / cells * HALF;
        let scale = cells / (size - 2.0 * half_cell);
        let along = |local: f64| {
            let f = (local - half_cell) * scale;
            let first = f.floor();
            (first, f - first)
        };
        let (column0, column_weight) = along(x - region_x * size);
        let (row0, row_weight) = along(y - region_y * size);
        let last = cells - 1.0;
        let cell = |value: f64| value.clamp(0.0, last) as usize;
        let weights = [
            (column0, row0, (1.0 - column_weight) * (1.0 - row_weight)),
            (column0 + 1.0, row0, column_weight * (1.0 - row_weight)),
            (column0, row0 + 1.0, (1.0 - column_weight) * row_weight),
            (column0 + 1.0, row0 + 1.0, column_weight * row_weight),
        ];
        let share: f64 = weights
            .iter()
            .filter(|(column, row, _)| region.is_road(cell(*column), edge - 1 - cell(*row)))
            .map(|(_, _, weight)| weight)
            .sum();
        share as f32
    }

    /// Encodes the road map with its header.
    pub fn to_bytes(&self, client_build: &str) -> Result<Vec<u8>, ContainerError> {
        encode_value(&ROADS_CONTAINER, client_build, self)
    }

    /// Checks the header, then decodes the road map.
    pub fn from_bytes(bytes: &[u8], expected_build: &str) -> Result<Self, ContainerError> {
        decode_value(&ROADS_CONTAINER, bytes, expected_build)
    }

    /// Writes the road file to `path`, creating parent folders.
    pub fn save(&self, path: &Path, client_build: &str) -> Result<(), ContainerError> {
        write_file(path, &self.to_bytes(client_build)?)
    }

    /// Reads and checks the road file at `path`.
    pub fn load(path: &Path, expected_build: &str) -> Result<Self, ContainerError> {
        Self::from_bytes(&read_file(path)?, expected_build)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The open world's region size, m, and its tract map edge.
    const REGION_M: f32 = 2048.0;
    const EDGE: u32 = 1024;
    /// The open world's road tract index.
    const ROAD: u8 = 1;
    const OTHER: u8 = 0;
    const BUILD: &str = "25810808";
    /// f32 places of some thousand metres hold a share to about this.
    const CLOSE: f32 = 1e-3;

    /// Region (4, 1) with one road column: cells of column 100, every row.
    fn map() -> RoadMap {
        let side = EDGE as usize;
        let mut tracts = vec![OTHER; side * side];
        for row in 0..side {
            tracts[row * side + 100] = ROAD;
        }
        RoadMap {
            level: "newworld_vitaeeterna".to_owned(),
            region_size_m: REGION_M,
            regions: vec![RoadRegion::from_tracts(4, 1, EDGE, &tracts, ROAD).expect("square")],
        }
    }

    /// The cell's size, m.
    const CELL_M: f32 = REGION_M / EDGE as f32;

    /// The local place of a cell index of the client's query: inset by half a cell, stretched
    /// over the region less a cell.
    fn local(cell: f32) -> f32 {
        CELL_M / 2.0 + cell * (REGION_M - CELL_M) / EDGE as f32
    }

    /// The world x of a column of region 4, at the cell's own place.
    fn column_x(column: f32) -> f32 {
        4.0 * REGION_M + local(column)
    }

    #[test]
    fn a_place_on_a_road_cell_is_all_road_and_its_neighbours_blend() {
        let map = map();
        let y = REGION_M + 700.0;
        assert!((map.road_share(column_x(100.0), y) - 1.0).abs() < CLOSE);
        assert!((map.road_share(column_x(100.25), y) - 0.75).abs() < CLOSE);
        assert!((map.road_share(column_x(99.5), y) - 0.5).abs() < CLOSE);
        assert!(map.road_share(column_x(98.0), y).abs() < CLOSE);
        assert!(map.road_share(column_x(102.0), y).abs() < CLOSE);
    }

    #[test]
    fn no_road_outside_the_regions_with_roads() {
        let map = map();
        assert!(map.road_share(column_x(100.0), 3.0 * REGION_M + 5.0).abs() < CLOSE);
        assert!(map.road_share(-5.0, 10.0).abs() < CLOSE);
        assert!(map.road_share(f32::NAN, 10.0).abs() < CLOSE);
        assert!(RoadMap::default().road_share(10.0, 10.0).abs() < CLOSE);
    }

    #[test]
    fn rows_count_from_the_north_edge() {
        // One road cell in row 0: the north edge of the region.
        let side = EDGE as usize;
        let mut tracts = vec![OTHER; side * side];
        tracts[10] = ROAD;
        let map = RoadMap {
            regions: vec![RoadRegion::from_tracts(0, 0, EDGE, &tracts, ROAD).expect("square")],
            ..map()
        };
        let x = local(10.0);
        assert!(
            (map.road_share(x, local(1023.0)) - 1.0).abs() < 1e-3,
            "north edge"
        );
        assert!(
            (map.road_share(x, REGION_M - 1e-3) - 1.0).abs() < 1e-3,
            "clamped"
        );
        assert!(map.road_share(x, local(0.0)).abs() < CLOSE, "south edge");
        assert!(map.road_share(local(11.0), local(1023.0)).abs() < 1e-3);
    }

    #[test]
    fn cells_that_are_not_square_make_no_region() {
        assert_eq!(RoadRegion::from_tracts(0, 0, 4, &[ROAD; 15], ROAD), None);
        assert_eq!(RoadRegion::from_tracts(0, 0, 0, &[], ROAD), None);
        let none = RoadRegion::from_tracts(0, 0, 2, &[OTHER; 4], ROAD).expect("square");
        assert!(!none.has_road());
        assert!(map().regions[0].has_road());
    }

    #[test]
    fn the_road_file_round_trips_and_checks_its_build() {
        let map = map();
        let bytes = map.to_bytes(BUILD).expect("encode");
        assert_eq!(bytes[..ROADS_MAGIC.len()], ROADS_MAGIC);
        assert_eq!(RoadMap::from_bytes(&bytes, BUILD).expect("decode"), map);
        assert!(RoadMap::from_bytes(&bytes, "1").is_err());
        assert_eq!(
            road_file_path(Path::new("content"), BUILD, "ftue_v2"),
            Path::new("content").join("roads-25810808-ftue_v2.bin")
        );
    }
}
