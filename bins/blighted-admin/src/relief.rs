// SPDX-License-Identifier: AGPL-3.0-or-later
//! Relief tiles drawn from the world file heights (admin tools spec section 3.2): grey by
//! height, light from the north-west, blue under the ocean level. Same tile grid as the map
//! tiles, so the page can lay one over the other to check them (section 3.3).

use nw_formats::png_image::encode_rgba_png;
use nw_world::WorldFile;
use nw_world::map_tiles::TilesIndex;

/// Edge of a relief tile, in pixels.
pub const RELIEF_TILE_PX: u32 = 256;
/// Colour under the ocean level.
pub const OCEAN_RGB: [u8; 3] = [38, 84, 128];
/// Alpha of a drawn pixel.
pub const OPAQUE: u8 = 255;
/// Pixel where the world has no ground.
const EMPTY: [u8; 4] = [0, 0, 0, 0];
/// Heights at and above this are the lightest grey.
const TOP_M: f32 = 1_000.0;
const GREY_LOW: f32 = 70.0;
const GREY_SPAN: f32 = 150.0;
/// Light from the north-west, from above: x west (−), y north (+), z up.
const LIGHT: [f32; 3] = [-0.577, 0.577, 0.577];
/// Light on a flat pixel is this share of the grey.
const AMBIENT: f32 = 0.35;
const RGBA: usize = 4;

/// One relief tile as PNG bytes; `None` when the level or the tile is outside `tiles`.
pub fn relief_tile(
    world: &WorldFile,
    tiles: &TilesIndex,
    level: u8,
    row: u32,
    column: u32,
) -> Option<Vec<u8>> {
    let grid = tiles.level(level)?;
    if row >= grid.rows || column >= grid.columns {
        return None;
    }
    let tile_m = grid.tile_m as f32;
    let pixel_m = tile_m / RELIEF_TILE_PX as f32;
    let west = column as f32 * tile_m;
    let north = if tiles.y_index_north_up {
        tiles.world_m as f32 - row as f32 * tile_m
    } else {
        (row + 1) as f32 * tile_m
    };
    let ocean = world.level.ocean_level;
    let side = RELIEF_TILE_PX as usize;
    let mut rgba = vec![0_u8; side * side * RGBA];
    for (index, pixel) in rgba.chunks_exact_mut(RGBA).enumerate() {
        let x = west + ((index % side) as f32 + 0.5) * pixel_m;
        let y = north - ((index / side) as f32 + 0.5) * pixel_m;
        pixel.copy_from_slice(&shade(world, x, y, pixel_m, ocean));
    }
    encode_rgba_png(RELIEF_TILE_PX, RELIEF_TILE_PX, &rgba).ok()
}

fn shade(world: &WorldFile, x: f32, y: f32, step: f32, ocean: f32) -> [u8; 4] {
    let Some(height) = world.height_at(x, y) else {
        return EMPTY;
    };
    if height < ocean {
        return [OCEAN_RGB[0], OCEAN_RGB[1], OCEAN_RGB[2], OPAQUE];
    }
    let at = |dx: f32, dy: f32| world.height_at(x + dx, y + dy).unwrap_or(height);
    let slope_x = (at(step, 0.0) - at(-step, 0.0)) / (2.0 * step);
    let slope_y = (at(0.0, step) - at(0.0, -step)) / (2.0 * step);
    let length = (slope_x * slope_x + slope_y * slope_y + 1.0).sqrt();
    let normal = [-slope_x / length, -slope_y / length, 1.0 / length];
    let light: f32 = normal.iter().zip(LIGHT).map(|(a, b)| a * b).sum();
    let grey = GREY_LOW + GREY_SPAN * (height / TOP_M).clamp(0.0, 1.0);
    let value = (grey * (AMBIENT + (1.0 - AMBIENT) * light.max(0.0))).clamp(0.0, 255.0) as u8;
    [value, value, value, OPAQUE]
}

#[cfg(test)]
mod tests {
    use nw_world::map_tiles::TilesIndex;

    use super::*;
    use crate::world::tests::{REGION_M, test_world};

    const WHOLE_REGION_LEVEL: u8 = 4;

    fn pixels(png_bytes: &[u8]) -> (u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
        let mut reader = decoder.read_info().expect("png");
        let mut buffer = vec![0; reader.output_buffer_size().expect("size")];
        let info = reader.next_frame(&mut buffer).expect("frame");
        (info.width, buffer)
    }

    #[test]
    fn relief_below_ocean_is_blue_and_above_is_grey() {
        let tiles = TilesIndex::new("test", REGION_M, WHOLE_REGION_LEVEL, false);
        let png_bytes =
            relief_tile(&test_world(), &tiles, WHOLE_REGION_LEVEL, 0, 0).expect("a tile");
        let (width, rgba) = pixels(&png_bytes);
        assert_eq!(width, RELIEF_TILE_PX);
        let middle_row = (RELIEF_TILE_PX / 2) as usize * width as usize * 4;
        let west = &rgba[middle_row + 10 * 4..middle_row + 10 * 4 + 4];
        assert_eq!(west, &[OCEAN_RGB[0], OCEAN_RGB[1], OCEAN_RGB[2], OPAQUE]);
        let east_at = middle_row + (width as usize - 10) * 4;
        let east = &rgba[east_at..east_at + 4];
        assert!(
            east[0] == east[1] && east[1] == east[2] && east[3] == OPAQUE,
            "{east:?}"
        );
    }

    #[test]
    fn a_tile_outside_the_grid_is_none() {
        let tiles = TilesIndex::new("test", REGION_M, WHOLE_REGION_LEVEL, false);
        assert!(relief_tile(&test_world(), &tiles, WHOLE_REGION_LEVEL, 1, 0).is_none());
        assert!(relief_tile(&test_world(), &tiles, WHOLE_REGION_LEVEL + 1, 0, 0).is_none());
    }

    #[test]
    fn rows_count_from_the_edge_the_index_names() {
        // Level 1 tiles are 256 m: the south row of a north-up grid is row 7.
        let north_up = TilesIndex::new("test", REGION_M, 1, true);
        let south_up = TilesIndex::new("test", REGION_M, 1, false);
        let world = test_world();
        let east = 7;
        assert_eq!(
            relief_tile(&world, &north_up, 1, 7, east),
            relief_tile(&world, &south_up, 1, 0, east)
        );
        assert_ne!(
            relief_tile(&world, &north_up, 1, 0, east),
            relief_tile(&world, &south_up, 1, 0, east)
        );
    }
}
