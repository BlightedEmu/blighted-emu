// SPDX-License-Identifier: AGPL-3.0-or-later
//! A region's `region.slicedata` file: for each slice and variant that the region's
//! distribution places, the meshes the client draws for it and whether it has collision.
//!
//! The file is a binary ObjectStream (Observed on every open-world region of build 25810808).
//! Its root holds `sliceMetaDataMap`: a list of `ELEMENT` pairs, each with a key (`Value1`:
//! `SliceName`, `VariantName`) and a value (`Value2`: `hasCollision`, `meshes`, and fields the
//! server does not need, such as view distances). Each mesh has its `meshAssetID` (the mesh's
//! asset id and sub id), its `materialOverrideAssetId` and its `rootRelativeTransform`: its place
//! in the slice's root space. Every field name is the lowercase CRC-32 of a name in the client's
//! own strings.
//!
//! The variant names of a distribution (`Bush_WaxMyrtle_a` of `gatherables/master_bush`) are in
//! no variation table of the build; this file is where the client finds their meshes.

use std::collections::HashMap;

use nw_formats::objectstream::{Element, ObjectStreamError, lowercase_crc32, parse_object_stream};

use crate::collision::{AssetKey, transform_of};

/// The slice file extension a slice name may carry.
const SLICE_EXTENSION: &str = ".dynamicslice";
/// The folder a slice name may start with.
const SLICES_PREFIX: &str = "slices/";

/// A slice and variant as a lookup key: lowercase, without the `slices/` folder and the
/// extension (the distribution writes `gatherables/master_tree`, the slice data
/// `slices/gatherables/master_tree` for the same slice).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SliceVariant {
    slice: String,
    variant: String,
}

impl SliceVariant {
    /// The key of `slice` and `variant` as either file writes them.
    pub fn of(slice: &str, variant: &str) -> Self {
        let slice = slice.to_lowercase().replace('\\', "/");
        let slice = slice.strip_prefix(SLICES_PREFIX).unwrap_or(&slice);
        let slice = slice.strip_suffix(SLICE_EXTENSION).unwrap_or(slice);
        Self {
            slice: slice.to_owned(),
            variant: variant.to_lowercase(),
        }
    }
}

/// One mesh of a slice variant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetaMesh {
    /// The mesh asset.
    pub mesh: AssetKey,
    /// Its place in the slice's root space.
    pub place: [[f32; 4]; 3],
}

/// What the client keeps of one slice variant.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SliceMeta {
    /// It has collision (`hasCollision`).
    pub has_collision: bool,
    /// Its meshes.
    pub meshes: Vec<MetaMesh>,
}

/// Why a slice data file does not read.
#[derive(Debug, thiserror::Error)]
pub enum SliceMetaError {
    /// The bytes are not an ObjectStream.
    #[error("slice data: {0}")]
    Stream(#[from] ObjectStreamError),
    /// The root holds no `sliceMetaDataMap`.
    #[error("slice data: no sliceMetaDataMap")]
    NoMap,
    /// An entry has no slice name or variant name.
    #[error("slice data: entry {index} has no slice or variant name")]
    NoKey {
        /// The entry's place in the map.
        index: usize,
    },
}

/// The field names this reader uses.
struct Names {
    map: u32,
    key: u32,
    value: u32,
    slice_name: u32,
    variant_name: u32,
    has_collision: u32,
    meshes: u32,
    mesh_asset: u32,
    guid: u32,
    sub_id: u32,
    place: u32,
}

impl Names {
    fn new() -> Self {
        Self {
            map: lowercase_crc32("sliceMetaDataMap"),
            key: lowercase_crc32("Value1"),
            value: lowercase_crc32("Value2"),
            slice_name: lowercase_crc32("SliceName"),
            variant_name: lowercase_crc32("VariantName"),
            has_collision: lowercase_crc32("hasCollision"),
            meshes: lowercase_crc32("meshes"),
            mesh_asset: lowercase_crc32("meshAssetID"),
            guid: lowercase_crc32("guid"),
            sub_id: lowercase_crc32("subId"),
            place: lowercase_crc32("rootRelativeTransform"),
        }
    }
}

/// The asset of an `AssetId` element (its `guid` and `subId` children); `None` for a null one.
fn asset_id(element: &Element, names: &Names) -> Option<AssetKey> {
    let guid = element.child(names.guid)?.as_uuid().ok()?;
    let sub = element.child(names.sub_id)?.as_u32().ok()?;
    (guid != [0; 16]).then_some((guid, sub))
}

/// Every slice variant of slice data file `bytes`. A variant without a value, or a mesh without
/// an asset or a place, is left out.
///
/// # Errors
///
/// Returns [`SliceMetaError`] when the bytes are not a slice data file.
pub fn parse_slice_meta(bytes: &[u8]) -> Result<HashMap<SliceVariant, SliceMeta>, SliceMetaError> {
    let names = Names::new();
    let root = parse_object_stream(bytes)?;
    let map = root.child(names.map).ok_or(SliceMetaError::NoMap)?;
    let mut out = HashMap::new();
    for (index, entry) in map.children.iter().enumerate() {
        let text = |element: Option<&Element>| {
            element
                .and_then(|element| element.as_string().ok())
                .map(str::to_owned)
        };
        let key = entry.child(names.key);
        let (Some(slice), Some(variant)) = (
            text(key.and_then(|key| key.child(names.slice_name))),
            text(key.and_then(|key| key.child(names.variant_name))),
        ) else {
            return Err(SliceMetaError::NoKey { index });
        };
        let Some(value) = entry.child(names.value) else {
            continue;
        };
        let has_collision = value
            .child(names.has_collision)
            .and_then(|flag| flag.as_bool().ok())
            .unwrap_or(false);
        let meshes = value
            .child(names.meshes)
            .into_iter()
            .flat_map(|list| &list.children)
            .filter_map(|mesh| {
                Some(MetaMesh {
                    mesh: asset_id(mesh.child(names.mesh_asset)?, &names)?,
                    place: transform_of(mesh.child(names.place)?)?,
                })
            })
            .collect();
        out.insert(
            SliceVariant::of(&slice, &variant),
            SliceMeta {
                has_collision,
                meshes,
            },
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::write_object_stream;

    use super::*;

    /// A transform that moves by (1, 2, 3): the 3 x 3 part by columns, then the move.
    fn moved() -> Vec<u8> {
        [
            1.0_f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0,
        ]
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
    }

    fn named(name: &str, value: Vec<u8>) -> Element {
        Element::with_value(lowercase_crc32(name), value)
    }

    fn entry(slice: &str, variant: &str, collision: bool, mesh: [u8; 16]) -> Element {
        let mesh_asset = Element::with_children(
            lowercase_crc32("meshAssetID"),
            vec![
                named("guid", mesh.to_vec()),
                named("subId", 0_u32.to_be_bytes().to_vec()),
            ],
        );
        Element::with_children(
            lowercase_crc32("ELEMENT"),
            vec![
                Element::with_children(
                    lowercase_crc32("Value1"),
                    vec![
                        named("SliceName", slice.as_bytes().to_vec()),
                        named("VariantName", variant.as_bytes().to_vec()),
                    ],
                ),
                Element::with_children(
                    lowercase_crc32("Value2"),
                    vec![
                        named("hasCollision", vec![u8::from(collision)]),
                        Element::with_children(
                            lowercase_crc32("meshes"),
                            vec![Element::with_children(
                                lowercase_crc32("ELEMENT"),
                                vec![mesh_asset, named("rootRelativeTransform", moved())],
                            )],
                        ),
                    ],
                ),
            ],
        )
    }

    fn file(entries: Vec<Element>) -> Vec<u8> {
        let root = Element::with_children(
            0,
            vec![Element::with_children(
                lowercase_crc32("sliceMetaDataMap"),
                entries,
            )],
        );
        write_object_stream(&root).expect("write")
    }

    #[test]
    fn a_variant_reads_with_its_mesh_its_place_and_its_collision() {
        const TREE: [u8; 16] = [0x9d; 16];
        let bytes = file(vec![
            entry("slices/gatherables/master_tree", "damnedtree_c", true, TREE),
            entry(
                "gatherables/master_bush",
                "Bush_WaxMyrtle_a",
                false,
                [0x54; 16],
            ),
        ]);
        let metas = parse_slice_meta(&bytes).expect("read");
        let tree = &metas[&SliceVariant::of("gatherables/master_tree", "DamnedTree_c")];
        assert!(tree.has_collision);
        assert_eq!(tree.meshes.len(), 1);
        assert_eq!(tree.meshes[0].mesh, (TREE, 0));
        assert_eq!(
            [
                tree.meshes[0].place[0][3],
                tree.meshes[0].place[1][3],
                tree.meshes[0].place[2][3]
            ],
            [1.0, 2.0, 3.0]
        );
        let bush = &metas[&SliceVariant::of(
            "slices/gatherables/master_bush.dynamicslice",
            "bush_waxmyrtle_a",
        )];
        assert!(!bush.has_collision, "a bush has no collision");
    }

    #[test]
    fn a_stream_without_the_map_or_an_entry_without_names_is_refused() {
        let empty = write_object_stream(&Element::with_children(0, Vec::new())).expect("write");
        assert!(matches!(
            parse_slice_meta(&empty),
            Err(SliceMetaError::NoMap)
        ));
        let nameless = file(vec![Element::with_children(
            lowercase_crc32("ELEMENT"),
            Vec::new(),
        )]);
        assert!(matches!(
            parse_slice_meta(&nameless),
            Err(SliceMetaError::NoKey { index: 0 })
        ));
    }
}
