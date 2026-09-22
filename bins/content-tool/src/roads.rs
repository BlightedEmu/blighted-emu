// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool roads`: the road file of a level ([`nw_world::roads`], road study 2026-09-20)
//! from the game paks: the level's `tracts.json` (`sharedassets/coatlicue/<level>/tracts.json`:
//! `regionSize` and the `tracts` list, whose position is a tract map cell's value) and its region
//! tract maps (`sharedassets/coatlicue/<level>/regions/r_+XX_+YY/region.tractmap.tif`). A level
//! whose list has no [`ROAD_TRACT_NAME`] tract gets a road file without regions.

use std::path::{Path, PathBuf};

use nw_formats::azcs::{AzcsError, decode_azcs, is_azcs};
use nw_formats::heightmap::{HeightmapError, parse_tractmap};
use nw_formats::pak::PakError;
use nw_world::roads::{ROAD_TRACT_NAME, RoadMap, RoadRegion, road_file_path};
use serde::Deserialize;

use crate::collision::PakIndex;

/// The folder of a level's tract data in the paks.
pub(crate) const COATLICUE_DIR: &str = "sharedassets/coatlicue";
/// The tract map of a region, under the level's `regions` folder.
const TRACTMAP_FILE: &str = "region.tractmap.tif";
/// The start of a region folder's name: `r_+XX_+YY`.
const REGION_PREFIX: &str = "r_";

/// Failure while building a road file.
#[derive(Debug, thiserror::Error)]
pub enum RoadsError {
    /// A pak cannot be read.
    #[error(transparent)]
    Pak(#[from] PakError),
    /// The level's `tracts.json` is missing from the paks.
    #[error("{0} is missing from the paks")]
    Missing(String),
    /// An AZCS wrapper cannot be decoded.
    #[error("{path}: {source}")]
    Azcs {
        /// The entry.
        path: String,
        /// Why.
        source: AzcsError,
    },
    /// The `tracts.json` cannot be read.
    #[error("{path}: {source}")]
    Json {
        /// The entry.
        path: String,
        /// Why.
        source: serde_json::Error,
    },
    /// The road tract's position does not fit a tract map cell.
    #[error("{path}: tract {ROAD_TRACT_NAME} is number {index}, past a cell's 255")]
    RoadIndex {
        /// The entry.
        path: String,
        /// Its position in the list.
        index: usize,
    },
    /// A tract map cannot be read.
    #[error("{path}: {source}")]
    Tractmap {
        /// The entry.
        path: String,
        /// Why.
        source: HeightmapError,
    },
    /// The road file cannot be written or read back.
    #[error("road file: {0}")]
    File(#[from] nw_content::ContainerError),
    /// The road file read back is not the one written.
    #[error("road file {0} reads back different")]
    Reload(PathBuf),
}

/// What one level's road build did.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadsReport {
    /// The level folder name.
    pub level: String,
    /// The tract maps read.
    pub tractmaps: usize,
    /// The regions with a road cell.
    pub road_regions: usize,
    /// The road cells.
    pub road_cells: u64,
    /// The file written.
    pub path: PathBuf,
}

/// The fields of `tracts.json` the road file needs.
#[derive(Debug, Deserialize)]
struct Tracts {
    #[serde(rename = "regionSize")]
    region_size: f32,
    tracts: Vec<Tract>,
}

/// One tract of `tracts.json`.
#[derive(Debug, Deserialize)]
struct Tract {
    name: String,
}

/// The `tracts.json` path of `level`.
fn tracts_path(level: &str) -> String {
    format!("{COATLICUE_DIR}/{level}/tracts.json")
}

/// The region of the region file `file` of `level` at `path`: `XX` and `YY` of
/// `<coatlicue>/<level>/regions/r_+XX_+YY/<file>`.
pub(crate) fn region_file(path: &str, level: &str, file: &str) -> Option<(u32, u32)> {
    let region = path
        .strip_prefix(&format!("{COATLICUE_DIR}/{level}/regions/"))?
        .strip_suffix(&format!("/{file}"))?
        .strip_prefix(REGION_PREFIX)?;
    let (x, y) = region.split_once('_')?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

/// The region of a tract map path of `level`.
fn tractmap_region(path: &str, level: &str) -> Option<(u32, u32)> {
    region_file(path, level, TRACTMAP_FILE)
}

/// The bytes of a pak entry, out of its AZCS wrapper when it has one.
pub(crate) fn without_azcs(bytes: Vec<u8>) -> Result<Vec<u8>, AzcsError> {
    if is_azcs(&bytes) {
        decode_azcs(&bytes)
    } else {
        Ok(bytes)
    }
}

/// [`without_azcs`] of the entry at `path`.
fn unwrapped(path: &str, bytes: Vec<u8>) -> Result<Vec<u8>, RoadsError> {
    without_azcs(bytes).map_err(|source| RoadsError::Azcs {
        path: path.to_owned(),
        source,
    })
}

/// The road map of `level` from its `tracts.json` bytes `tracts_json` (read from `tracts_path`)
/// and its tract map files `tractmaps` (path and bytes).
///
/// # Errors
///
/// Returns [`RoadsError`] when the list or a tract map cannot be read.
pub fn road_map(
    level: &str,
    tracts_path: &str,
    tracts_json: &[u8],
    tractmaps: impl IntoIterator<Item = (String, Vec<u8>)>,
) -> Result<RoadMap, RoadsError> {
    let tracts: Tracts =
        serde_json::from_slice(tracts_json).map_err(|source| RoadsError::Json {
            path: tracts_path.to_owned(),
            source,
        })?;
    let mut map = RoadMap {
        level: level.to_owned(),
        region_size_m: tracts.region_size,
        regions: Vec::new(),
    };
    let Some(index) = tracts
        .tracts
        .iter()
        .position(|tract| tract.name == ROAD_TRACT_NAME)
    else {
        return Ok(map);
    };
    let road = u8::try_from(index).map_err(|_| RoadsError::RoadIndex {
        path: tracts_path.to_owned(),
        index,
    })?;
    for (path, bytes) in tractmaps {
        let Some((x, y)) = tractmap_region(&path, level) else {
            continue;
        };
        let cells = parse_tractmap(&bytes).map_err(|source| RoadsError::Tractmap {
            path: path.clone(),
            source,
        })?;
        let edge = u32::try_from(cells.size).map_err(|_| RoadsError::Tractmap {
            path: path.clone(),
            source: HeightmapError::SampleCount {
                size: cells.size,
                actual: cells.cells.len(),
            },
        })?;
        if let Some(region) =
            RoadRegion::from_tracts(x, y, edge, &cells.cells, road).filter(RoadRegion::has_road)
        {
            map.regions.push(region);
        }
    }
    map.regions.sort_by_key(|region| (region.x, region.y));
    Ok(map)
}

/// Builds the road file of `level` from the paks of `index` into `out_dir` and checks it by
/// reading it back.
///
/// # Errors
///
/// Returns [`RoadsError`] when an entry cannot be read or the file cannot be written.
pub fn build_level_roads(
    index: &PakIndex,
    level: &str,
    client_build: &str,
    out_dir: &Path,
) -> Result<RoadsReport, RoadsError> {
    let tracts = tracts_path(level);
    let mut read = index.read_each(std::slice::from_ref(&tracts), |path, bytes| {
        unwrapped(path, bytes)
    });
    let (_, tracts_json) = read
        .pop()
        .ok_or_else(|| RoadsError::Missing(tracts.clone()))?;
    let tracts_json = tracts_json??;
    let paths = index.paths(|path| tractmap_region(path, level).is_some());
    let mut tractmaps = Vec::with_capacity(paths.len());
    for (path, bytes) in index.read_each(&paths, unwrapped) {
        tractmaps.push((path, bytes??));
    }
    let count = tractmaps.len();
    let map = road_map(level, &tracts, &tracts_json, tractmaps)?;
    let path = road_file_path(out_dir, client_build, level);
    map.save(&path, client_build)?;
    if RoadMap::load(&path, client_build)? != map {
        return Err(RoadsError::Reload(path));
    }
    Ok(RoadsReport {
        level: level.to_owned(),
        tractmaps: count,
        road_regions: map.regions.len(),
        road_cells: map
            .regions
            .iter()
            .flat_map(|region| &region.cells)
            .map(|word| u64::from(word.count_ones()))
            .sum(),
        path,
    })
}

#[cfg(test)]
mod tests {
    use nw_formats::heightmap::{SMALL_HEIGHTMAP_SIZE, Tractmap, encode_tractmap};

    use super::*;

    const LEVEL: &str = "newworld_vitaeeterna";
    /// The open world's list starts with these tracts (`roads` is number 1).
    const TRACTS: &str = r#"{
        "tractmapCellSize": 1,
        "regionSize": 2048,
        "tracts": [
            { "name": "starterarea", "displayColor": { "r": 255, "g": 255, "b": 255 } },
            { "name": "roads", "displayColor": { "r": 191, "g": 191, "b": 191 } }
        ]
    }"#;

    #[test]
    fn tract_map_paths_name_their_region() {
        let path =
            |region: &str| format!("{COATLICUE_DIR}/{LEVEL}/regions/{region}/{TRACTMAP_FILE}");
        assert_eq!(tractmap_region(&path("r_+04_+01"), LEVEL), Some((4, 1)));
        assert_eq!(tractmap_region(&path("r_+00_+04"), LEVEL), Some((0, 4)));
        assert_eq!(tractmap_region(&path("r_+04_+01"), "ftue_v2"), None);
        assert_eq!(
            tractmap_region(
                &format!("{COATLICUE_DIR}/{LEVEL}/regions/r_+04_+01/region.heightmap"),
                LEVEL
            ),
            None
        );
    }

    #[test]
    fn a_level_without_a_road_tract_gets_no_regions() {
        let tracts = r#"{ "regionSize": 1024, "tracts": [ { "name": "beach" } ] }"#;
        let map = road_map("ftue_v2", "tracts.json", tracts.as_bytes(), Vec::new()).unwrap();
        assert!(map.regions.is_empty());
        assert!((map.region_size_m - 1024.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_tract_map_s_road_cells_make_its_region() {
        // Cells read 0 and 1 in turn along each row: the odd columns are road.
        const ROAD: u8 = 1;
        const OTHER: u8 = 0;
        let cells = (0..SMALL_HEIGHTMAP_SIZE * SMALL_HEIGHTMAP_SIZE)
            .map(|index| if index % 2 == 1 { ROAD } else { OTHER })
            .collect();
        let bytes = encode_tractmap(&Tractmap {
            size: SMALL_HEIGHTMAP_SIZE,
            cells,
        })
        .unwrap();
        let path = format!("{COATLICUE_DIR}/{LEVEL}/regions/r_+04_+01/{TRACTMAP_FILE}");
        let other = format!("{COATLICUE_DIR}/{LEVEL}/regions/r_+04_+01/region.heightmap");
        let map = road_map(
            LEVEL,
            "tracts.json",
            TRACTS.as_bytes(),
            [(path, bytes), (other, Vec::new())],
        )
        .unwrap();
        assert_eq!(map.regions.len(), 1);
        let region = &map.regions[0];
        assert_eq!((region.x, region.y), (4, 1));
        // Column 101: the client's cells are 2046 / 1024 m apart from 1 m in.
        const CELL_STEP_M: f32 = 2046.0 / 1024.0;
        let x = 4.0 * 2048.0 + 1.0 + 101.0 * CELL_STEP_M;
        let y = 2048.0 + 1000.0;
        assert!((map.road_share(x, y) - 1.0).abs() < 1e-3, "odd column");
        assert!(
            map.road_share(x - CELL_STEP_M, y).abs() < 1e-3,
            "even column"
        );
    }
}
