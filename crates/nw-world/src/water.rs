// SPDX-License-Identifier: AGPL-3.0-or-later
//! The water of a level (water study 2026-09-20): the surface of the sea, the lakes, the rivers
//! and the ponds, from the level's region water trees
//! (`sharedassets/coatlicue/<level>/regions/r_+XX_+YY/region.waterqt`: an ObjectStream with
//! `regionSize` and a list of quadtree nodes, each a `height`, a `floorHeight` and `flags`).
//!
//! The nodes are in breadth-first order: node 0 is the whole region, a node whose height is
//! [`INNER_NODE_HEIGHT`] has four children, the next four unused nodes, low x and low y first
//! (child `q` covers x half `q & 1` and y half `q >> 1`). Any other node is a leaf with one
//! water surface for its whole square, water where its flags are not 0 (the flags' first byte:
//! sea `0x0a`, lake or river `0x07`, pond `0x04`). Retail: every sea leaf holds the level's ocean level (open world
//! 40.0, 350,020 leaves; tutorial 20.0, 8,899 leaves); the own state's water depth followed the
//! leaf heights of the lakes of an open-world capture and of the tutorial sea.
//!
//! The file is the shared header of `nw_content::container` with magic `NWWA`, then a postcard
//! body of one [`WaterMap`]. `content-tool water` writes it.

use std::path::{Path, PathBuf};

use nw_content::container::{
    ContainerError, ContainerSpec, decode_value, encode_value, read_file, write_file,
};
use serde::{Deserialize, Serialize};

/// First 4 bytes of every water file.
pub const WATER_MAGIC: [u8; 4] = *b"NWWA";
/// Current water file format version.
pub const WATER_FORMAT_VERSION: u16 = 1;
/// Version of the record types. Raise it when a type in this module changes.
pub const WATER_SCHEMA_VERSION: u32 = 1;
/// Header fields of the water file.
pub const WATER_CONTAINER: ContainerSpec = ContainerSpec {
    magic: WATER_MAGIC,
    format_version: WATER_FORMAT_VERSION,
    schema_hash: WATER_SCHEMA_VERSION,
};
/// The height of a node that has children (with floor height 65535 and flags 0).
pub const INNER_NODE_HEIGHT: f32 = 65_535.0;
/// Children of an inner node.
const CHILDREN: u32 = 4;
/// The bit of a child's x half in its index.
const X_HALF: u32 = 1;
/// The shift of a child's y half in its index.
const Y_HALF_SHIFT: u32 = 1;

/// The water file path of one level: `<dir>/water-<client_build>-<level>.bin`.
pub fn water_file_path(dir: &Path, client_build: &str, level: &str) -> PathBuf {
    dir.join(format!("water-{client_build}-{level}.bin"))
}

/// One node of a region's water tree.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WaterNode {
    /// The water surface of a leaf, m; [`INNER_NODE_HEIGHT`] for a node with children.
    pub height: f32,
    /// The water's kind (the file's big-endian `flags`); 0 where a leaf holds no water.
    pub flags: u32,
}

impl WaterNode {
    /// Whether the node has children.
    fn is_inner(self) -> bool {
        self.height >= INNER_NODE_HEIGHT && self.flags == 0
    }
}

/// The water tree of one region.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WaterRegion {
    /// The region's column (`XX` of `r_+XX_+YY`).
    pub x: u32,
    /// The region's row (`YY`).
    pub y: u32,
    /// The nodes in the file's order.
    pub nodes: Vec<WaterNode>,
    /// Per node, the index of its first child; 0 for a leaf.
    pub first_child: Vec<u32>,
}

impl WaterRegion {
    /// The tree of region (`x`, `y`) from its nodes in the file's order. `None` when the inner
    /// nodes do not name exactly the nodes there are.
    pub fn from_nodes(x: u32, y: u32, nodes: Vec<WaterNode>) -> Option<Self> {
        let count = u32::try_from(nodes.len()).ok()?;
        let mut next = 1;
        let first_child = nodes
            .iter()
            .map(|node| {
                if node.is_inner() {
                    let first = next;
                    next += CHILDREN;
                    first
                } else {
                    0
                }
            })
            .collect();
        (count > 0 && next == count).then_some(Self {
            x,
            y,
            nodes,
            first_child,
        })
    }

    /// The leaf at the local place (`x`, `y`) of a region of `size` metres.
    fn leaf_at(&self, x: f32, y: f32, size: f32) -> Option<WaterNode> {
        let (mut left, mut bottom, mut side) = (0.0, 0.0, size);
        let mut index = 0_usize;
        loop {
            let node = *self.nodes.get(index)?;
            if !node.is_inner() {
                return Some(node);
            }
            side /= 2.0;
            let east = x >= left + side;
            let north = y >= bottom + side;
            if east {
                left += side;
            }
            if north {
                bottom += side;
            }
            let quadrant = u32::from(east) * X_HALF + (u32::from(north) << Y_HALF_SHIFT);
            index = usize::try_from(*self.first_child.get(index)? + quadrant).ok()?;
        }
    }
}

/// The water of one level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WaterMap {
    /// The level folder name.
    pub level: String,
    /// The side of one region, m (`regionSize`).
    pub region_size_m: f32,
    /// The regions with a water tree.
    pub regions: Vec<WaterRegion>,
}

impl WaterMap {
    /// The water surface at (`x`, `y`), m: the height of the leaf there when it holds water.
    /// `None` on dry land and outside every region with a water tree.
    pub fn surface_at(&self, x: f32, y: f32) -> Option<f32> {
        self.water_leaf(x, y).map(|leaf| leaf.height)
    }

    /// The water flags at (`x`, `y`) where water is there (sea `0x0a`, lake or river `0x07`,
    /// pond `0x04`); `None` on dry land and outside every region with a water tree.
    pub fn flags_at(&self, x: f32, y: f32) -> Option<u32> {
        self.water_leaf(x, y).map(|leaf| leaf.flags)
    }

    /// The leaf at (`x`, `y`) when it holds water.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "region indices of a level are a few dozen, checked not negative"
    )]
    fn water_leaf(&self, x: f32, y: f32) -> Option<WaterNode> {
        let size = self.region_size_m;
        if !x.is_finite() || !y.is_finite() || size <= 0.0 {
            return None;
        }
        let (region_x, region_y) = ((x / size).floor(), (y / size).floor());
        if region_x < 0.0 || region_y < 0.0 {
            return None;
        }
        let (index_x, index_y) = (region_x as u32, region_y as u32);
        let region = self
            .regions
            .iter()
            .find(|region| region.x == index_x && region.y == index_y)?;
        let leaf = region.leaf_at(x - region_x * size, y - region_y * size, size)?;
        (leaf.flags != 0).then_some(leaf)
    }

    /// Encodes the water map with its header.
    pub fn to_bytes(&self, client_build: &str) -> Result<Vec<u8>, ContainerError> {
        encode_value(&WATER_CONTAINER, client_build, self)
    }

    /// Checks the header, then decodes the water map.
    pub fn from_bytes(bytes: &[u8], expected_build: &str) -> Result<Self, ContainerError> {
        decode_value(&WATER_CONTAINER, bytes, expected_build)
    }

    /// Writes the water file to `path`, creating parent folders.
    pub fn save(&self, path: &Path, client_build: &str) -> Result<(), ContainerError> {
        write_file(path, &self.to_bytes(client_build)?)
    }

    /// Reads and checks the water file at `path`.
    pub fn load(path: &Path, expected_build: &str) -> Result<Self, ContainerError> {
        Self::from_bytes(&read_file(path)?, expected_build)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tutorial's region size, m, its sea and one of its ponds.
    const REGION_M: f32 = 1024.0;
    const SEA_M: f32 = 20.0;
    const POND_M: f32 = 22.331;
    const SEA: u32 = 0x0a;
    const POND: u32 = 0x04;
    const DRY: u32 = 0;
    const BUILD: &str = "25810808";

    const fn inner() -> WaterNode {
        WaterNode {
            height: INNER_NODE_HEIGHT,
            flags: 0,
        }
    }

    const fn leaf(height: f32, flags: u32) -> WaterNode {
        WaterNode { height, flags }
    }

    /// Region (0, 0): the south-west quarter split again (sea, pond, dry, sea), the other
    /// quarters dry, sea, sea.
    fn map() -> WaterMap {
        let nodes = vec![
            inner(),
            inner(),
            leaf(0.0, DRY),
            leaf(SEA_M, SEA),
            leaf(SEA_M, SEA),
            leaf(SEA_M, SEA),
            leaf(POND_M, POND),
            leaf(0.0, DRY),
            leaf(SEA_M, SEA),
        ];
        WaterMap {
            level: "ftue_v2".to_owned(),
            region_size_m: REGION_M,
            regions: vec![WaterRegion::from_nodes(0, 0, nodes).expect("a whole tree")],
        }
    }

    #[test]
    fn a_place_takes_the_surface_of_its_leaf() {
        let map = map();
        // The south-west quarter's quarters, 256 m each.
        assert_eq!(map.surface_at(100.0, 100.0), Some(SEA_M));
        assert_eq!(map.surface_at(300.0, 100.0), Some(POND_M));
        assert_eq!(map.surface_at(100.0, 300.0), None, "dry");
        assert_eq!(map.surface_at(300.0, 300.0), Some(SEA_M));
        // The other quarters, 512 m each: south-east dry, north-west and north-east sea.
        assert_eq!(map.surface_at(700.0, 100.0), None);
        assert_eq!(map.surface_at(100.0, 700.0), Some(SEA_M));
        assert_eq!(map.surface_at(700.0, 700.0), Some(SEA_M));
        assert_eq!(map.surface_at(1100.0, 100.0), None, "no region");
        assert_eq!(map.surface_at(-1.0, 100.0), None);
    }

    #[test]
    fn a_place_takes_the_flags_of_its_leaf() {
        let map = map();
        assert_eq!(map.flags_at(100.0, 100.0), Some(SEA));
        assert_eq!(map.flags_at(300.0, 100.0), Some(POND));
        assert_eq!(map.flags_at(100.0, 300.0), None, "dry");
        assert_eq!(map.flags_at(1100.0, 100.0), None, "no region");
    }

    #[test]
    fn a_tree_whose_inner_nodes_miss_children_is_refused() {
        assert_eq!(WaterRegion::from_nodes(0, 0, vec![inner()]), None);
        assert_eq!(WaterRegion::from_nodes(0, 0, Vec::new()), None);
        assert!(WaterRegion::from_nodes(0, 0, vec![leaf(SEA_M, SEA)]).is_some());
    }

    #[test]
    fn the_water_file_round_trips_and_checks_its_build() {
        let map = map();
        let bytes = map.to_bytes(BUILD).expect("encode");
        assert_eq!(bytes[..WATER_MAGIC.len()], WATER_MAGIC);
        assert_eq!(WaterMap::from_bytes(&bytes, BUILD).expect("decode"), map);
        assert!(WaterMap::from_bytes(&bytes, "1").is_err());
        assert_eq!(
            water_file_path(Path::new("content"), BUILD, "ftue_v2"),
            Path::new("content").join("water-25810808-ftue_v2.bin")
        );
    }
}
