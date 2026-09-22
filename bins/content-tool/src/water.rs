// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool water`: the water file of a level ([`nw_world::water`], water study 2026-09-20)
//! from the game paks: the region water trees
//! (`sharedassets/coatlicue/<level>/regions/r_+XX_+YY/region.waterqt`, binary ObjectStreams: a
//! `regionSize` and a list of `element`s, each with a `height`, a `floorHeight` and `flags`).

use std::path::{Path, PathBuf};

use nw_formats::azcs::AzcsError;
use nw_formats::objectstream::{Element, ObjectStreamError, parse_object_stream};
use nw_formats::pak::PakError;
use nw_world::water::{WaterMap, WaterNode, WaterRegion, water_file_path};

use crate::collision::PakIndex;
use crate::roads::{region_file, without_azcs};

/// The water tree of a region, under the level's `regions` folder.
const WATERQT_FILE: &str = "region.waterqt";
/// `lowercase_crc32("regionSize")`.
const REGION_SIZE_CRC: u32 = 0xcae6_d679;
/// The name CRC of the node list (its name is Unknown).
const NODE_LIST_CRC: u32 = 0x843b_f3eb;
/// `lowercase_crc32("element")`.
const ELEMENT_CRC: u32 = 0x4140_5e39;
/// `lowercase_crc32("height")`.
const HEIGHT_CRC: u32 = 0xf54d_e50f;
/// `lowercase_crc32("flags")`.
const FLAGS_CRC: u32 = 0x0b05_41ba;

/// Failure while building a water file.
#[derive(Debug, thiserror::Error)]
pub enum WaterError {
    /// A pak cannot be read.
    #[error(transparent)]
    Pak(#[from] PakError),
    /// An AZCS wrapper cannot be decoded.
    #[error("{path}: {source}")]
    Azcs {
        /// The entry.
        path: String,
        /// Why.
        source: AzcsError,
    },
    /// A water tree is not a readable ObjectStream.
    #[error("{path}: {source}")]
    ObjectStream {
        /// The entry.
        path: String,
        /// Why.
        source: ObjectStreamError,
    },
    /// A water tree lacks a field or its nodes do not make a whole tree.
    #[error("{path}: {what}")]
    Tree {
        /// The entry.
        path: String,
        /// What is wrong.
        what: &'static str,
    },
    /// The water trees of one level hold different region sizes.
    #[error("{path}: region size {found}, other regions {expected}")]
    RegionSize {
        /// The entry.
        path: String,
        /// Its region size.
        found: f32,
        /// The size of the regions read before.
        expected: f32,
    },
    /// The water file cannot be written or read back.
    #[error("water file: {0}")]
    File(#[from] nw_content::ContainerError),
    /// The water file read back is not the one written.
    #[error("water file {0} reads back different")]
    Reload(PathBuf),
}

/// What one level's water build did.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterReport {
    /// The level folder name.
    pub level: String,
    /// The regions with a water tree.
    pub regions: usize,
    /// Their nodes.
    pub nodes: usize,
    /// The file written.
    pub path: PathBuf,
}

/// The region size and the nodes of one water tree file.
///
/// # Errors
///
/// Returns [`WaterError`] when the file is no ObjectStream or lacks a field.
pub fn water_tree(path: &str, bytes: &[u8]) -> Result<(f32, Vec<WaterNode>), WaterError> {
    let tree = |what| WaterError::Tree {
        path: path.to_owned(),
        what,
    };
    let stream = |source| WaterError::ObjectStream {
        path: path.to_owned(),
        source,
    };
    let root = parse_object_stream(bytes).map_err(stream)?;
    let size = root
        .child(REGION_SIZE_CRC)
        .ok_or_else(|| tree("no regionSize"))?
        .as_u32()
        .map_err(stream)?;
    let size = i32::from_be_bytes(size.to_be_bytes());
    let size = u16::try_from(size).map_err(|_| tree("region size out of range"))?;
    let nodes = root
        .child(NODE_LIST_CRC)
        .ok_or_else(|| tree("no node list"))?
        .children_named(ELEMENT_CRC)
        .map(|element: &Element| {
            let value = |crc, what| element.child(crc).ok_or_else(|| tree(what));
            Ok(WaterNode {
                height: value(HEIGHT_CRC, "a node without height")?
                    .as_f32()
                    .map_err(stream)?,
                flags: value(FLAGS_CRC, "a node without flags")?
                    .as_u32()
                    .map_err(stream)?,
            })
        })
        .collect::<Result<Vec<_>, WaterError>>()?;
    Ok((f32::from(size), nodes))
}

/// The water map of `level` from its water tree files `trees` (path and bytes).
///
/// # Errors
///
/// Returns [`WaterError`] when a tree cannot be read or the trees disagree on the region size.
pub fn water_map(
    level: &str,
    trees: impl IntoIterator<Item = (String, Vec<u8>)>,
) -> Result<WaterMap, WaterError> {
    let mut map = WaterMap {
        level: level.to_owned(),
        region_size_m: 0.0,
        regions: Vec::new(),
    };
    for (path, bytes) in trees {
        let Some((x, y)) = region_file(&path, level, WATERQT_FILE) else {
            continue;
        };
        let (size, nodes) = water_tree(&path, &bytes)?;
        if map.regions.is_empty() {
            map.region_size_m = size;
        } else if (size - map.region_size_m).abs() > f32::EPSILON {
            return Err(WaterError::RegionSize {
                path,
                found: size,
                expected: map.region_size_m,
            });
        }
        let region = WaterRegion::from_nodes(x, y, nodes).ok_or_else(|| WaterError::Tree {
            path: path.clone(),
            what: "the inner nodes do not name every node",
        })?;
        map.regions.push(region);
    }
    map.regions.sort_by_key(|region| (region.x, region.y));
    Ok(map)
}

/// Builds the water file of `level` from the paks of `index` into `out_dir` and checks it by
/// reading it back.
///
/// # Errors
///
/// Returns [`WaterError`] when an entry cannot be read or the file cannot be written.
pub fn build_level_water(
    index: &PakIndex,
    level: &str,
    client_build: &str,
    out_dir: &Path,
) -> Result<WaterReport, WaterError> {
    let paths = index.paths(|path| region_file(path, level, WATERQT_FILE).is_some());
    let mut trees = Vec::with_capacity(paths.len());
    for (path, bytes) in index.read_each(&paths, |path, bytes| {
        without_azcs(bytes).map_err(|source| WaterError::Azcs {
            path: path.to_owned(),
            source,
        })
    }) {
        trees.push((path, bytes??));
    }
    let map = water_map(level, trees)?;
    let path = water_file_path(out_dir, client_build, level);
    map.save(&path, client_build)?;
    if WaterMap::load(&path, client_build)? != map {
        return Err(WaterError::Reload(path));
    }
    Ok(WaterReport {
        level: level.to_owned(),
        regions: map.regions.len(),
        nodes: map.regions.iter().map(|region| region.nodes.len()).sum(),
        path,
    })
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::write_object_stream;
    use nw_world::water::INNER_NODE_HEIGHT;

    use super::*;

    const LEVEL: &str = "ftue_v2";
    /// The tutorial's region size, m, and its sea.
    const REGION_M: u32 = 1024;
    const SEA_M: f32 = 20.0;
    /// The flags of a sea leaf (first byte 0x0a) and of a dry one.
    const SEA: u32 = 0x0a00_0000;
    const DRY: u32 = 0;

    fn node(height: f32, flags: u32) -> Element {
        Element::with_children(
            ELEMENT_CRC,
            vec![
                Element::with_value(HEIGHT_CRC, height.to_be_bytes().to_vec()),
                Element::with_value(FLAGS_CRC, flags.to_be_bytes().to_vec()),
            ],
        )
    }

    /// A water tree: the region split into four, the south-west quarter sea, the rest dry.
    fn tree_file() -> Vec<u8> {
        let nodes = vec![
            node(INNER_NODE_HEIGHT, DRY),
            node(SEA_M, SEA),
            node(0.0, DRY),
            node(0.0, DRY),
            node(0.0, DRY),
        ];
        write_object_stream(&Element {
            children: vec![
                Element::with_value(REGION_SIZE_CRC, REGION_M.to_be_bytes().to_vec()),
                Element::with_children(NODE_LIST_CRC, nodes),
            ],
            ..Element::default()
        })
        .unwrap()
    }

    #[test]
    fn a_water_tree_file_makes_its_region() {
        let path = format!("sharedassets/coatlicue/{LEVEL}/regions/r_+00_+00/{WATERQT_FILE}");
        let other = format!("sharedassets/coatlicue/{LEVEL}/regions/r_+00_+00/region.heightmap");
        let map = water_map(LEVEL, [(path, tree_file()), (other, Vec::new())]).unwrap();
        assert_eq!(map.regions.len(), 1);
        assert!((map.region_size_m - 1024.0).abs() < f32::EPSILON);
        assert_eq!(map.surface_at(100.0, 100.0), Some(SEA_M));
        assert_eq!(map.surface_at(900.0, 100.0), None);
    }

    #[test]
    fn a_tree_without_its_fields_is_refused() {
        let empty = write_object_stream(&Element::default()).unwrap();
        assert!(matches!(
            water_tree("x", &empty),
            Err(WaterError::Tree { .. })
        ));
        assert!(matches!(
            water_tree("x", &[1, 2, 3]),
            Err(WaterError::ObjectStream { .. })
        ));
    }
}
