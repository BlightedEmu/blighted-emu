// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ground height from region heightmaps (M1b spec section 5).

use nw_formats::heightmap::HEIGHTMAP_SIZE;

use crate::records::WorldFile;

/// Raw heightmap units per metre where `mountainHeight` is [`TERRAIN_MOUNTAIN_HEIGHT`].
pub const HEIGHT_UNITS_PER_METRE: f64 = 32.0;
/// `terrain.json` `mountainHeight` of the levels whose height scale is verified.
pub const TERRAIN_MOUNTAIN_HEIGHT: f32 = 2048.0;
/// Index of the last sample in a heightmap row or column.
const LAST_SAMPLE: f64 = (HEIGHTMAP_SIZE - 1) as f64;

impl WorldFile {
    /// Ground height in metres at world position (`x`, `y`). Column = local x, row =
    /// 2047 - local y, bilinear blend of the 4 samples around the point, clamped inside the
    /// region. `None` when the level has no terrain or the point is outside every region with a
    /// height grid.
    pub fn height_at(&self, x: f32, y: f32) -> Option<f32> {
        if !self.level.has_terrain {
            return None;
        }
        let size = f64::from(self.level.region_size);
        let (x, y) = (f64::from(x), f64::from(y));
        let (region_x, region_y) = ((x / size).floor(), (y / size).floor());
        if region_x < 0.0 || region_y < 0.0 {
            return None;
        }
        let (index_x, index_y) = (region_x as u32, region_y as u32);
        let grid = self
            .regions
            .iter()
            .find(|region| region.x == index_x && region.y == index_y)?
            .heights
            .as_ref()?;
        let column = (x - region_x * size).clamp(0.0, LAST_SAMPLE);
        let row = (LAST_SAMPLE - (y - region_y * size)).clamp(0.0, LAST_SAMPLE);
        let column0 = column.floor().min(LAST_SAMPLE - 1.0);
        let row0 = row.floor().min(LAST_SAMPLE - 1.0);
        let (column_weight, row_weight) = (column - column0, row - row0);
        let sample = |column: f64, row: f64| {
            grid.samples
                .get(row as usize * HEIGHTMAP_SIZE + column as usize)
                .map(|&raw| f64::from(raw))
        };
        let blended = sample(column0, row0)? * (1.0 - column_weight) * (1.0 - row_weight)
            + sample(column0 + 1.0, row0)? * column_weight * (1.0 - row_weight)
            + sample(column0, row0 + 1.0)? * (1.0 - column_weight) * row_weight
            + sample(column0 + 1.0, row0 + 1.0)? * column_weight * row_weight;
        Some((blended / HEIGHT_UNITS_PER_METRE) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{HeightGrid, LevelInfo, Region};

    const REGION_SIZE: f32 = HEIGHTMAP_SIZE as f32;
    /// The test grid is region (1, 0): x from 2048 to 4096, y from 0 to 2048.
    const REGION_X: u32 = 1;
    const ORIGIN_X: f32 = REGION_SIZE;
    const PEAK: u16 = 320;

    /// raw = column + row, so height = (local x + 2047 - local y) / 32 everywhere inside.
    fn linear_grid() -> HeightGrid {
        HeightGrid {
            samples: (0..HEIGHTMAP_SIZE * HEIGHTMAP_SIZE)
                .map(|index| (index / HEIGHTMAP_SIZE + index % HEIGHTMAP_SIZE) as u16)
                .collect(),
        }
    }

    fn world(heights: Option<HeightGrid>, has_terrain: bool) -> WorldFile {
        WorldFile {
            level: LevelInfo {
                region_size: HEIGHTMAP_SIZE as u32,
                grid_width: REGION_X + 1,
                grid_height: 1,
                has_terrain,
                ..LevelInfo::default()
            },
            regions: vec![Region {
                x: REGION_X,
                y: 0,
                heights,
            }],
            ..WorldFile::default()
        }
    }

    fn metres(raw: f64) -> f32 {
        (raw / HEIGHT_UNITS_PER_METRE) as f32
    }

    #[test]
    fn corners_follow_the_row_flip() {
        let world = world(Some(linear_grid()), true);
        // South-west corner: column 0, row 2047.
        assert_eq!(world.height_at(ORIGIN_X, 0.0), Some(metres(LAST_SAMPLE)));
        // North-west sample: column 0, row 0.
        assert_eq!(world.height_at(ORIGIN_X, LAST_SAMPLE as f32), Some(0.0));
    }

    #[test]
    fn inside_points_blend_linearly() {
        let world = world(Some(linear_grid()), true);
        let height = world.height_at(ORIGIN_X + 10.5, 100.25).unwrap();
        assert_eq!(height, metres(10.5 + LAST_SAMPLE - 100.25));
    }

    #[test]
    fn edges_clamp_inside_the_region() {
        let world = world(Some(linear_grid()), true);
        let east_north = world.height_at(ORIGIN_X + REGION_SIZE - 0.1, REGION_SIZE - 0.1);
        assert_eq!(east_north, Some(metres(LAST_SAMPLE)));
    }

    #[test]
    fn bilinear_weights_mix_four_samples() {
        let mut grid = HeightGrid {
            samples: vec![0; HEIGHTMAP_SIZE * HEIGHTMAP_SIZE],
        };
        grid.samples[0] = PEAK;
        let world = world(Some(grid), true);
        // Column 0.5, row 0.5: a quarter of the peak sample.
        let height = world.height_at(ORIGIN_X + 0.5, LAST_SAMPLE as f32 - 0.5);
        assert_eq!(height, Some(metres(f64::from(PEAK) / 4.0)));
    }

    #[test]
    fn no_height_outside_the_grid_or_without_terrain() {
        let world_with_grid = world(Some(linear_grid()), true);
        assert_eq!(world_with_grid.height_at(-1.0, 10.0), None);
        assert_eq!(world_with_grid.height_at(10.0, 10.0), None);
        assert_eq!(world_with_grid.height_at(ORIGIN_X, REGION_SIZE + 1.0), None);
        assert_eq!(world(None, true).height_at(ORIGIN_X, 0.0), None);
        assert_eq!(
            world(Some(linear_grid()), false).height_at(ORIGIN_X, 0.0),
            None
        );
    }

    #[test]
    fn short_grid_gives_no_height() {
        let grid = HeightGrid {
            samples: vec![0; 2],
        };
        assert_eq!(world(Some(grid), true).height_at(ORIGIN_X, 0.0), None);
    }
}
