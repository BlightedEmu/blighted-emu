// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ground height from region heightmaps (M1b spec section 5).

use std::collections::HashMap;

use crate::records::{HeightGrid, Position, WorldFile};

/// Raw heightmap units per metre where `mountainHeight` is [`TERRAIN_MOUNTAIN_HEIGHT`].
pub const HEIGHT_UNITS_PER_METRE: f64 = 32.0;
/// `terrain.json` `mountainHeight` of the open world, whose height scale is verified.
pub const TERRAIN_MOUNTAIN_HEIGHT: f32 = 2048.0;
/// `mountainHeight` of the FTUE levels. Observed: their raw samples / 64 put the retail tutorial
/// creatures' spawn heights 0.5 to 1.1 m above the ground (risen, swarmers, legionnaire 2), so a
/// full raw range spans 1024 m.
pub const FTUE_MOUNTAIN_HEIGHT: f32 = 1024.0;
/// The `mountainHeight` values whose height scale is verified.
pub const VERIFIED_MOUNTAIN_HEIGHTS: [f32; 2] = [FTUE_MOUNTAIN_HEIGHT, TERRAIN_MOUNTAIN_HEIGHT];

/// A raw sample of a level with `mountainHeight` `mountain_height` (one of
/// [`VERIFIED_MOUNTAIN_HEIGHTS`]) at [`HEIGHT_UNITS_PER_METRE`]: raw samples span
/// `mountain_height` metres.
pub fn rescaled_sample(raw: u16, mountain_height: f32) -> u16 {
    (f32::from(raw) * mountain_height / TERRAIN_MOUNTAIN_HEIGHT).round() as u16
}

/// A capital's terrain `cookie`: the client raises or lowers the terrain around the capital to its
/// height. The client reads it in `FUN_1469b55c0` and the console variable
/// `coat_terrainDebugCookies` shows it; the region heightmaps do not hold it (Observed: retail
/// player feet inside the cookies of `monarchsbluffs_starterbeach`, `brghtwd_a01_brackwater_props`
/// and others stand at the cookie's height, not on the heightmap).
///
/// The blend (Inferred, fitted to 7,000 retail player samples of 34 captures in 13 zones; the
/// client's blend function is not found): full at most `inner_radius` from the centre, a
/// smoothstep down to none at `outer_radius`, times `max_height_blend`. Where cookies overlap,
/// the one with the greatest weight sets the height. The client's `edgeNoise`,
/// `edgeNoiseFeatureSize` and `groundNoiseScale` noise is not modelled.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TerrainCookie {
    /// The capital's position; its z is the height the cookie blends to.
    pub center: Position,
    /// Distance in metres of full blend.
    pub inner_radius: f32,
    /// Distance in metres where the blend ends.
    pub outer_radius: f32,
    /// The blend at full weight, 0 to 1.
    pub max_height_blend: f32,
}

impl TerrainCookie {
    /// The share of the cookie's height at `distance` metres from its centre.
    pub fn weight_at(&self, distance: f64) -> f64 {
        let (inner, outer) = (f64::from(self.inner_radius), f64::from(self.outer_radius));
        let falloff = if distance <= inner {
            1.0
        } else if distance >= outer {
            0.0
        } else {
            let t = (distance - inner) / (outer - inner);
            1.0 - t * t * (3.0 - 2.0 * t)
        };
        falloff * f64::from(self.max_height_blend)
    }

    /// The farthest distance with a weight.
    fn reach(&self) -> f64 {
        f64::from(self.inner_radius.max(self.outer_radius))
    }
}

impl HeightGrid {
    /// Rows and columns of the square grid; `None` when the sample count is not a square.
    pub fn edge(&self) -> Option<usize> {
        let edge = self.samples.len().isqrt();
        (edge > 0 && edge * edge == self.samples.len()).then_some(edge)
    }
}

impl WorldFile {
    /// Ground height in metres at world position (`x`, `y`). With `n` samples a side over the
    /// region's metres, column = local x x n / size, row = n - 1 - local y x n / size; bilinear
    /// blend of the 4 samples around the point, clamped inside the region. `None` when the level
    /// has no terrain, when `x` or `y` is not finite, or when the point is outside every region
    /// with a square height grid.
    pub fn height_at(&self, x: f32, y: f32) -> Option<f32> {
        if !self.level.has_terrain || !x.is_finite() || !y.is_finite() {
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
        let edge = grid.edge()?;
        let last_sample = (edge - 1) as f64;
        let samples_per_metre = edge as f64 / size;
        let column = ((x - region_x * size) * samples_per_metre).clamp(0.0, last_sample);
        let row = (last_sample - (y - region_y * size) * samples_per_metre).clamp(0.0, last_sample);
        let column0 = column.floor().min(last_sample - 1.0);
        let row0 = row.floor().min(last_sample - 1.0);
        let (column_weight, row_weight) = (column - column0, row - row0);
        let sample = |column: f64, row: f64| {
            grid.samples
                .get(row as usize * edge + column as usize)
                .map(|&raw| f64::from(raw))
        };
        let blended = sample(column0, row0)? * (1.0 - column_weight) * (1.0 - row_weight)
            + sample(column0 + 1.0, row0)? * column_weight * (1.0 - row_weight)
            + sample(column0, row0 + 1.0)? * (1.0 - column_weight) * row_weight
            + sample(column0 + 1.0, row0 + 1.0)? * column_weight * row_weight;
        Some((blended / HEIGHT_UNITS_PER_METRE) as f32)
    }

    /// Puts `cookies` into the height samples of every region, as the client shows its terrain:
    /// each sample takes the height of the cookie with the greatest weight over it.
    pub fn stamp_cookies(&mut self, cookies: &[TerrainCookie]) {
        if !self.level.has_terrain {
            return;
        }
        let size = f64::from(self.level.region_size);
        for region in &mut self.regions {
            let Some(grid) = region.heights.as_mut() else {
                continue;
            };
            let Some(edge) = grid.edge() else {
                continue;
            };
            let samples_per_metre = edge as f64 / size;
            let last_sample = (edge - 1) as f64;
            let (origin_x, origin_y) = (f64::from(region.x) * size, f64::from(region.y) * size);
            let mut strongest: HashMap<usize, (f64, f64)> = HashMap::new();
            for cookie in cookies {
                let (center_x, center_y) =
                    (f64::from(cookie.center[0]), f64::from(cookie.center[1]));
                let reach = cookie.reach();
                let (Some(columns), Some(rows)) = (
                    sample_span(center_x - origin_x, reach, samples_per_metre, last_sample),
                    sample_span(
                        origin_y + size - center_y - size / edge as f64,
                        reach,
                        samples_per_metre,
                        last_sample,
                    ),
                ) else {
                    continue;
                };
                for row in rows {
                    let y = origin_y + (last_sample - row as f64) / samples_per_metre;
                    for column in columns.clone() {
                        let x = origin_x + column as f64 / samples_per_metre;
                        let weight = cookie.weight_at((x - center_x).hypot(y - center_y));
                        if weight <= 0.0 {
                            continue;
                        }
                        let stamp = (weight, f64::from(cookie.center[2]));
                        strongest
                            .entry(row * edge + column)
                            .and_modify(|best| {
                                if weight > best.0 {
                                    *best = stamp;
                                }
                            })
                            .or_insert(stamp);
                    }
                }
            }
            for (index, (weight, height)) in strongest {
                let sample = &mut grid.samples[index];
                let raw = f64::from(*sample);
                let target = height * HEIGHT_UNITS_PER_METRE;
                *sample = (raw + weight * (target - raw))
                    .round()
                    .clamp(0.0, f64::from(u16::MAX)) as u16;
            }
        }
    }
}

/// The sample indices within `reach` metres of `local` metres along one axis of a grid; `None`
/// when no sample is that near.
fn sample_span(
    local: f64,
    reach: f64,
    samples_per_metre: f64,
    last_sample: f64,
) -> Option<std::ops::RangeInclusive<usize>> {
    let first = ((local - reach) * samples_per_metre).floor();
    let last = ((local + reach) * samples_per_metre).ceil();
    (last >= 0.0 && first <= last_sample)
        .then(|| first.max(0.0) as usize..=last.min(last_sample) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{LevelInfo, Region};
    use nw_formats::heightmap::{HEIGHTMAP_SIZE, SMALL_HEIGHTMAP_SIZE};

    /// Index of the last sample in a row or column of the open-world grid.
    const LAST_SAMPLE: f64 = (HEIGHTMAP_SIZE - 1) as f64;

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

    /// A cookie at local (100, 100) of the test region, 10 m high, full to 5 m, none from 25 m.
    const COOKIE_LOCAL: f32 = 100.0;
    const COOKIE_HEIGHT: f32 = 10.0;
    const INNER_RADIUS: f32 = 5.0;
    const OUTER_RADIUS: f32 = 25.0;
    /// Halfway between the radii, where the smoothstep gives half the weight.
    const HALFWAY: f32 = 15.0;

    fn flat_world() -> WorldFile {
        world(
            Some(HeightGrid {
                samples: vec![0; HEIGHTMAP_SIZE * HEIGHTMAP_SIZE],
            }),
            true,
        )
    }

    fn cookie(offset: f32, height: f32, inner: f32, outer: f32, blend: f32) -> TerrainCookie {
        TerrainCookie {
            center: [ORIGIN_X + COOKIE_LOCAL + offset, COOKIE_LOCAL, height],
            inner_radius: inner,
            outer_radius: outer,
            max_height_blend: blend,
        }
    }

    fn height_east_of_the_cookie(world: &WorldFile, distance: f32) -> Option<f32> {
        world.height_at(ORIGIN_X + COOKIE_LOCAL + distance, COOKIE_LOCAL)
    }

    #[test]
    fn a_cookie_sets_its_height_inside_and_blends_out_to_its_outer_radius() {
        let mut world = flat_world();
        world.stamp_cookies(&[cookie(0.0, COOKIE_HEIGHT, INNER_RADIUS, OUTER_RADIUS, 1.0)]);
        assert_eq!(height_east_of_the_cookie(&world, 0.0), Some(COOKIE_HEIGHT));
        assert_eq!(
            height_east_of_the_cookie(&world, INNER_RADIUS),
            Some(COOKIE_HEIGHT)
        );
        assert_eq!(
            height_east_of_the_cookie(&world, HALFWAY),
            Some(COOKIE_HEIGHT / 2.0)
        );
        assert_eq!(height_east_of_the_cookie(&world, OUTER_RADIUS), Some(0.0));
        assert_eq!(world.height_at(ORIGIN_X, 0.0), Some(0.0));
    }

    #[test]
    fn the_max_height_blend_scales_the_cookie() {
        const HALF_BLEND: f32 = 0.5;
        let mut world = flat_world();
        world.stamp_cookies(&[cookie(
            0.0,
            COOKIE_HEIGHT,
            INNER_RADIUS,
            OUTER_RADIUS,
            HALF_BLEND,
        )]);
        assert_eq!(
            height_east_of_the_cookie(&world, 0.0),
            Some(COOKIE_HEIGHT * HALF_BLEND)
        );
    }

    #[test]
    fn where_cookies_overlap_the_strongest_sets_the_height() {
        const SECOND_OFFSET: f32 = 12.0;
        const SECOND_HEIGHT: f32 = 20.0;
        const SECOND_OUTER: f32 = 10.0;
        /// Inside the first cookie's inner radius, near the second one's outer edge.
        const NEAR_FIRST: f32 = 4.0;
        let mut world = flat_world();
        world.stamp_cookies(&[
            cookie(0.0, COOKIE_HEIGHT, INNER_RADIUS, OUTER_RADIUS, 1.0),
            cookie(SECOND_OFFSET, SECOND_HEIGHT, 0.0, SECOND_OUTER, 1.0),
        ]);
        assert_eq!(
            height_east_of_the_cookie(&world, SECOND_OFFSET),
            Some(SECOND_HEIGHT)
        );
        assert_eq!(
            height_east_of_the_cookie(&world, NEAR_FIRST),
            Some(COOKIE_HEIGHT)
        );
    }

    #[test]
    fn a_level_without_terrain_takes_no_cookies() {
        let mut world = world(Some(linear_grid()), false);
        world.stamp_cookies(&[cookie(0.0, COOKIE_HEIGHT, INNER_RADIUS, OUTER_RADIUS, 1.0)]);
        assert_eq!(world.regions[0].heights, Some(linear_grid()));
    }

    #[test]
    fn samples_rescale_to_the_open_world_scale() {
        const RAW: u16 = 1274;
        assert_eq!(rescaled_sample(RAW, TERRAIN_MOUNTAIN_HEIGHT), RAW);
        // FTUE raw 1274 / 64 = 19.9 m, stored as 637 at 32 units per metre.
        assert_eq!(rescaled_sample(RAW, FTUE_MOUNTAIN_HEIGHT), RAW / 2);
    }

    #[test]
    fn a_small_grid_covers_its_own_region_size() {
        let edge = SMALL_HEIGHTMAP_SIZE;
        let grid = HeightGrid {
            samples: (0..edge * edge)
                .map(|index| (index / edge + index % edge) as u16)
                .collect(),
        };
        let world = WorldFile {
            level: LevelInfo {
                region_size: edge as u32,
                grid_width: 1,
                grid_height: 1,
                has_terrain: true,
                ..LevelInfo::default()
            },
            regions: vec![Region {
                x: 0,
                y: 0,
                heights: Some(grid),
            }],
            ..WorldFile::default()
        };
        let last = (edge - 1) as f64;
        // Column 10.5, row last - 100.25.
        assert_eq!(
            world.height_at(10.5, 100.25),
            Some(metres(10.5 + last - 100.25))
        );
        assert_eq!(world.height_at(edge as f32 + 1.0, 0.0), None);
    }

    #[test]
    fn positions_that_are_not_finite_give_no_height() {
        let world = world(Some(linear_grid()), true);
        assert_eq!(world.height_at(f32::NAN, 10.0), None);
        assert_eq!(world.height_at(ORIGIN_X, f32::NAN), None);
        assert_eq!(world.height_at(f32::INFINITY, 10.0), None);
        assert_eq!(world.height_at(ORIGIN_X, f32::NEG_INFINITY), None);
    }
}
