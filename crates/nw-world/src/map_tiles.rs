// SPDX-License-Identifier: AGPL-3.0-or-later
//! The index of the world map tiles that `content-tool map-tiles` writes (admin tools spec
//! section 3.1): per level, the tile size in metres and the grid. Level 1 is the finest; each
//! level above it halves the grid.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name of the index in a level's map folder.
pub const TILES_FILE: &str = "tiles.json";
/// File extension of a written tile.
const TILE_EXTENSION: &str = "png";

/// The file of tile (`row`, `column`) of `level` in a level's map folder:
/// `<dir>/<level>/<row>_<column>.png`.
pub fn tile_file(dir: &Path, level: u8, row: u32, column: u32) -> PathBuf {
    dir.join(level.to_string())
        .join(format!("{row}_{column}.{TILE_EXTENSION}"))
}

/// Metres of a level 1 tile (14,336 m over 56 tiles; admin tools spec F7, Inferred).
pub const LEVEL_1_TILE_M: u32 = 256;

/// One level of the tile pyramid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileLevel {
    /// Level number, from 1.
    pub level: u8,
    /// Edge of one tile, in metres.
    pub tile_m: u32,
    /// Tiles from west to east.
    pub columns: u32,
    /// Tiles from one edge to the other in y.
    pub rows: u32,
}

/// The tile pyramid of one level's world map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TilesIndex {
    /// Level name, e.g. `newworld_vitaeeterna`.
    pub level: String,
    /// Edge of the square world, in metres.
    pub world_m: u32,
    /// Levels 1 to the last, in order.
    pub levels: Vec<TileLevel>,
    /// Whether tile row 0 is the north edge (`content-tool map-tiles --y-index`).
    pub y_index_north_up: bool,
}

impl TilesIndex {
    /// The pyramid of `levels` levels over a world of `world_m` metres.
    pub fn new(level: &str, world_m: u32, levels: u8, y_index_north_up: bool) -> Self {
        let levels = (1..=levels)
            .map(|number| {
                let tile_m = LEVEL_1_TILE_M
                    .checked_shl(u32::from(number - 1))
                    .unwrap_or(u32::MAX);
                let count = world_m.div_ceil(tile_m);
                TileLevel {
                    level: number,
                    tile_m,
                    columns: count,
                    rows: count,
                }
            })
            .collect();
        Self {
            level: level.to_owned(),
            world_m,
            levels,
            y_index_north_up,
        }
    }

    /// One level.
    pub fn level(&self, level: u8) -> Option<&TileLevel> {
        self.levels.iter().find(|entry| entry.level == level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN_WORLD_M: u32 = 14_336;
    const LEVELS: u8 = 8;

    #[test]
    fn the_index_has_half_the_grid_per_level() {
        let index = TilesIndex::new("newworld_vitaeeterna", OPEN_WORLD_M, LEVELS, true);
        let grid: Vec<(u8, u32, u32, u32)> = index
            .levels
            .iter()
            .map(|level| (level.level, level.tile_m, level.columns, level.rows))
            .collect();
        assert_eq!(
            grid,
            vec![
                (1, 256, 56, 56),
                (2, 512, 28, 28),
                (3, 1_024, 14, 14),
                (4, 2_048, 7, 7),
                (5, 4_096, 4, 4),
                (6, 8_192, 2, 2),
                (7, 16_384, 1, 1),
                (8, 32_768, 1, 1),
            ]
        );
        assert_eq!(index.level(4).map(|level| level.tile_m), Some(2_048));
        assert_eq!(index.level(9), None);
    }

    #[test]
    fn a_tile_file_is_level_then_row_and_column() {
        assert_eq!(
            tile_file(Path::new("map"), 2, 3, 4),
            Path::new("map").join("2").join("3_4.png")
        );
    }

    #[test]
    fn the_index_reads_back_from_json() {
        let index = TilesIndex::new("level", OPEN_WORLD_M, 2, false);
        let json = serde_json::to_string(&index).expect("json");
        assert_eq!(
            serde_json::from_str::<TilesIndex>(&json).expect("read"),
            index
        );
    }
}
