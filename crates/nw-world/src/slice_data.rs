// SPDX-License-Identifier: AGPL-3.0-or-later
//! The slices that a level's slices spawn (GDE id note, `docs/re/2026-09-18-gde-ids-of-level-
//! entities.md`). Each region's `region.slicedata` (an ObjectStream) lists, per slice and
//! variant, the spawner entries of the slice: the transform of the spawned slice relative to its
//! parent, the spawned slice's asset GUID, its path and its variant. A capital's slice spawns
//! them, and they spawn their own in turn.
//!
//! A spawned slice's GDE id (tag 7) is its packed world position, yaw sector and asset GUID
//! ([`spawned_gde_id`]); the client's spawn card names it as the id's little-endian bytes and 8
//! zero bytes. Observed: 1,387 of the 1,424 tag-7 ids of the captures; of the other 37, 6 lie
//! within 0.05 degrees of a yaw sector edge and 31 are not spawned by any slice (the note).

use std::collections::HashMap;

use nw_formats::objectstream::{Element, ObjectStreamError, lowercase_crc32};
use serde::{Deserialize, Serialize};

use crate::records::{Capital, Position};

/// The low 3 bits of a spawned slice's GDE id.
pub const SPAWNED_TAG: u64 = 0b111;
/// Steps per metre of the x and y fields, and their bit widths and shifts.
const XY_STEPS_PER_METRE: f64 = 8.0;
const XY_BITS: u32 = 18;
const X_SHIFT: u32 = 3;
const Y_SHIFT: u32 = X_SHIFT + XY_BITS;
/// Steps per metre of the z field, its width and shift.
const Z_STEPS_PER_METRE: f64 = 4.0;
const Z_BITS: u32 = 12;
const Z_SHIFT: u32 = Y_SHIFT + XY_BITS;
/// Degrees of one yaw sector, the field's width and shift.
const YAW_SECTOR_DEGREES: f64 = 23.0;
const YAW_BITS: u32 = 4;
const YAW_SHIFT: u32 = Z_SHIFT + Z_BITS;
/// The asset field: the GUID's first byte, and bit 0 of its second byte above it.
const ASSET_SHIFT: u32 = YAW_SHIFT + YAW_BITS;
const BYTE_BITS: u32 = 8;
/// Degrees of a turn.
const TURN_DEGREES: f64 = 360.0;
/// Yaws are rounded to this many steps per degree before their sector is taken: designers place
/// slices at whole degrees, and the float noise of the transforms must not move them across a
/// sector edge (Observed: 115.0, 184.0 and 276.0 degrees fall in the upper sector).
const YAW_STEPS_PER_DEGREE: f64 = 1_000.0;
/// Deepest chain of spawners followed from a capital (Observed: at most 4 in the open world).
pub const MAX_SPAWN_DEPTH: usize = 8;
/// Floats of a serialized transform: 3 basis columns, then the translation.
const TRANSFORM_FLOATS: usize = 12;
const BASIS_FLOATS: usize = 9;
pub(crate) const AXES: usize = 3;
const FLOAT_BYTES: usize = 4;
/// Bytes of an asset GUID.
pub const GUID_LEN: usize = 16;
/// The file suffix of a dynamic slice.
const DYNAMIC_SLICE_SUFFIX: &str = ".dynamicslice";

/// A 3x3 basis (rotation and uniform scale) by rows.
pub(crate) type Basis = [[f64; AXES]; AXES];
pub(crate) const IDENTITY: Basis = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A `region.slicedata` that cannot be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SliceDataError {
    /// A value has another size or text form than its field needs.
    #[error("slice data: {0}")]
    Value(#[from] ObjectStreamError),
    /// A spawner's transform is not 12 floats.
    #[error("slice data: a spawner transform has {0} bytes")]
    Transform(usize),
}

/// One spawner entry of a slice.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceSpawner {
    /// The spawned slice's basis relative to its parent, by rows.
    pub basis: Basis,
    /// The spawned slice's position relative to its parent.
    pub translation: [f64; AXES],
    /// `sliceAssetId`'s GUID.
    pub asset: [u8; GUID_LEN],
    /// `sliceName`: the spawned slice's path.
    pub slice: String,
    /// `variationName`: its variant (empty for none).
    pub variant: String,
}

/// The spawners of every slice and variant, by lowercase (slice, variant).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SliceSpawners {
    by_slice: HashMap<(String, String), Vec<SliceSpawner>>,
}

/// One slice that a level's slices spawn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpawnedSlice {
    /// Its GDE id ([`spawned_gde_id`]).
    pub id: u64,
    /// The spawned slice's path.
    pub slice: String,
    /// Its variant (empty for none).
    pub variant: String,
    /// Its world position.
    pub position: Position,
}

/// The names of the fields read, as their name CRCs.
struct Names {
    map: u32,
    key: u32,
    metadata: u32,
    slice: u32,
    variant: u32,
    spawners: u32,
    transform: u32,
    asset: u32,
    variation: u32,
}

impl Names {
    fn new() -> Self {
        Self {
            map: lowercase_crc32("sliceMetaDataMap"),
            key: lowercase_crc32("value1"),
            metadata: lowercase_crc32("value2"),
            slice: lowercase_crc32("SliceName"),
            variant: lowercase_crc32("VariantName"),
            spawners: lowercase_crc32("spawners"),
            transform: lowercase_crc32("worldTM"),
            asset: lowercase_crc32("sliceAssetId"),
            variation: lowercase_crc32("variationName"),
        }
    }
}

/// The text of `element`, lowercase; empty when absent.
fn text(element: Option<&Element>) -> Result<String, ObjectStreamError> {
    element.map_or(Ok(String::new()), |element| {
        element.as_string().map(str::to_lowercase)
    })
}

impl SliceSpawners {
    /// Adds the spawners of the `region.slicedata` whose root element is `root`. A slice already
    /// known keeps its first list (every region lists the same slices alike).
    ///
    /// # Errors
    ///
    /// Returns [`SliceDataError`] when a name, GUID or transform has the wrong form.
    pub fn add_region(&mut self, root: &Element) -> Result<(), SliceDataError> {
        let names = Names::new();
        let Some(map) = root.child(names.map) else {
            return Ok(());
        };
        for pair in &map.children {
            let (Some(key), Some(metadata)) = (pair.child(names.key), pair.child(names.metadata))
            else {
                continue;
            };
            let Some(spawners) = metadata.child(names.spawners) else {
                continue;
            };
            let list = spawners
                .children
                .iter()
                .map(|entry| spawner(entry, &names))
                .collect::<Result<Vec<_>, _>>()?;
            if list.is_empty() {
                continue;
            }
            let slice = (
                text(key.child(names.slice))?,
                text(key.child(names.variant))?,
            );
            self.by_slice.entry(slice).or_insert(list);
        }
        Ok(())
    }

    /// The slices the slice `slice` (variant `variant`) spawns, any case. The data names a
    /// generated slice with its `.dynamicslice` suffix and a spawner names its slice without
    /// one, so both forms are looked up.
    pub fn of(&self, slice: &str, variant: &str) -> &[SliceSpawner] {
        let slice = slice.to_lowercase();
        let variant = variant.to_lowercase();
        let bare = slice
            .strip_suffix(DYNAMIC_SLICE_SUFFIX)
            .unwrap_or(&slice)
            .to_owned();
        let suffixed = format!("{bare}{DYNAMIC_SLICE_SUFFIX}");
        [bare, suffixed]
            .into_iter()
            .find_map(|name| self.by_slice.get(&(name, variant.clone())))
            .map_or(&[], Vec::as_slice)
    }

    /// Every slice that the `capitals` spawn, followed through [`MAX_SPAWN_DEPTH`] spawners.
    pub fn spawned_by(&self, capitals: &[Capital]) -> Vec<SpawnedSlice> {
        let mut spawned = Vec::new();
        for capital in capitals {
            let basis = scaled(
                &quaternion_basis(capital.rotation),
                capital.scale.unwrap_or(1.0),
            );
            let position = capital.position.map(f64::from);
            let variant = capital.variant_name.as_deref().unwrap_or_default();
            self.spawn(
                &capital.slice_name,
                variant,
                &basis,
                position,
                MAX_SPAWN_DEPTH,
                &mut spawned,
            );
        }
        spawned
    }

    fn spawn(
        &self,
        slice: &str,
        variant: &str,
        basis: &Basis,
        position: [f64; AXES],
        depth: usize,
        spawned: &mut Vec<SpawnedSlice>,
    ) {
        let Some(depth) = depth.checked_sub(1) else {
            return;
        };
        for spawner in self.of(slice, variant) {
            let offset = apply(basis, &spawner.translation);
            let world = [
                position[0] + offset[0],
                position[1] + offset[1],
                position[2] + offset[2],
            ];
            let child = multiply(basis, &spawner.basis);
            let yaw = child[1][0].atan2(child[0][0]).to_degrees();
            #[allow(clippy::cast_possible_truncation, reason = "world metres")]
            let position = world.map(|value| value as f32);
            spawned.push(SpawnedSlice {
                id: spawned_gde_id(world, yaw, &spawner.asset),
                slice: spawner.slice.clone(),
                variant: spawner.variant.clone(),
                position,
            });
            self.spawn(
                &spawner.slice,
                &spawner.variant,
                &child,
                world,
                depth,
                spawned,
            );
        }
    }
}

/// The basis (by rows) and translation of a serialized transform: 12 big-endian floats, the 3
/// basis columns, then the translation.
///
/// # Errors
///
/// Returns [`SliceDataError::Transform`] when `floats` is not 12 floats.
pub(crate) fn transform_from_bytes(floats: &[u8]) -> Result<(Basis, [f64; AXES]), SliceDataError> {
    if floats.len() != TRANSFORM_FLOATS * FLOAT_BYTES {
        return Err(SliceDataError::Transform(floats.len()));
    }
    let value = |index: usize| {
        let at = index * FLOAT_BYTES;
        f64::from(f32::from_be_bytes([
            floats[at],
            floats[at + 1],
            floats[at + 2],
            floats[at + 3],
        ]))
    };
    // The floats are the basis columns: row r of column c is float c * 3 + r.
    let mut basis = IDENTITY;
    for (row, values) in basis.iter_mut().enumerate() {
        for (column, cell) in values.iter_mut().enumerate() {
            *cell = value(column * AXES + row);
        }
    }
    Ok((
        basis,
        [
            value(BASIS_FLOATS),
            value(BASIS_FLOATS + 1),
            value(BASIS_FLOATS + 2),
        ],
    ))
}

fn spawner(entry: &Element, names: &Names) -> Result<SliceSpawner, SliceDataError> {
    let floats = entry
        .child(names.transform)
        .map(|transform| transform.value.as_slice())
        .unwrap_or_default();
    let (basis, translation) = transform_from_bytes(floats)?;
    let asset = entry
        .child(names.asset)
        .and_then(|asset| asset.children.first())
        .map(Element::as_uuid)
        .transpose()?
        .unwrap_or_default();
    Ok(SliceSpawner {
        basis,
        translation,
        asset,
        slice: text(entry.child(names.slice))?,
        variant: text(entry.child(names.variation))?,
    })
}

/// The GDE id of a slice spawned at `position` (world metres) with yaw `yaw_degrees`, whose
/// asset GUID is `asset`: tag 7; x and y in eighths of a metre (18 bits each), z in quarters
/// (12 bits), all rounded; the yaw's 23-degree sector (4 bits); the GUID's first byte, and bit 0
/// of its second byte above it.
pub fn spawned_gde_id(position: [f64; AXES], yaw_degrees: f64, asset: &[u8; GUID_LEN]) -> u64 {
    let field = |value: f64, steps: f64, bits: u32, shift: u32| -> u64 {
        // Two's complement of the rounded steps, cut to the field (negative values: Unknown).
        #[allow(clippy::cast_possible_truncation, reason = "world coordinates")]
        let steps = (value * steps).round() as i64;
        #[allow(clippy::cast_sign_loss, reason = "the field keeps the low bits")]
        let steps = steps as u64;
        (steps & ((1 << bits) - 1)) << shift
    };
    let turned = ((yaw_degrees * YAW_STEPS_PER_DEGREE).round() / YAW_STEPS_PER_DEGREE)
        .rem_euclid(TURN_DEGREES);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a sector of a turn"
    )]
    let sector = (turned / YAW_SECTOR_DEGREES).floor() as u64;
    let asset_bits = u64::from(asset[0]) | (u64::from(asset[1] & 1) << BYTE_BITS);
    SPAWNED_TAG
        | field(position[0], XY_STEPS_PER_METRE, XY_BITS, X_SHIFT)
        | field(position[1], XY_STEPS_PER_METRE, XY_BITS, Y_SHIFT)
        | field(position[2], Z_STEPS_PER_METRE, Z_BITS, Z_SHIFT)
        | ((sector & ((1 << YAW_BITS) - 1)) << YAW_SHIFT)
        | (asset_bits << ASSET_SHIFT)
}

/// The basis of quaternion `[x, y, z, w]`, by rows.
pub(crate) fn quaternion_basis([x, y, z, w]: [f32; 4]) -> Basis {
    let [x, y, z, w] = [x, y, z, w].map(f64::from);
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

pub(crate) fn scaled(basis: &Basis, scale: f32) -> Basis {
    basis.map(|row| row.map(|cell| cell * f64::from(scale)))
}

pub(crate) fn multiply(left: &Basis, right: &Basis) -> Basis {
    let mut product = [[0.0; AXES]; AXES];
    for (row, values) in product.iter_mut().enumerate() {
        for (column, cell) in values.iter_mut().enumerate() {
            *cell = (0..AXES).map(|k| left[row][k] * right[k][column]).sum();
        }
    }
    product
}

pub(crate) fn apply(basis: &Basis, vector: &[f64; AXES]) -> [f64; AXES] {
    basis.map(|row| (0..AXES).map(|k| row[k] * vector[k]).sum())
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::{parse_object_stream, write_object_stream};

    use super::*;

    /// `int_watchtowercampt1`'s asset GUID (`9b5ec8d1-ae80-5776-9288-ca4601bde669`).
    const WATCHTOWER_CAMP: [u8; GUID_LEN] = [
        0x9b, 0x5e, 0xc8, 0xd1, 0xae, 0x80, 0x57, 0x76, 0x92, 0x88, 0xca, 0x46, 0x01, 0xbd, 0xe6,
        0x69,
    ];
    /// The watchtowers' spawner 1 (`region.slicedata`): identity basis, this translation.
    const CAMP_TRANSLATION: [f32; AXES] = [-2.138_671_9, 0.344_970_7, 0.253_189_1];
    const CAMP_SLICE: &str = "slices/pois/interactions/int_watchtowercampt1";
    const MB_WATCHTOWER: &str =
        "CoatGen\\\\27f3faf3\\Outpost_Watchtower_C_12_MonarchsBluffs_1841409740.dynamicslice";
    const WW_WATCHTOWER: &str =
        "CoatGen\\\\8d616dd7\\Outpost_Watchtower_C_06_Windsward_2523401405.dynamicslice";
    /// The retail cards of the two campfires (first 8 `GdeRef` bytes, little-endian).
    const MB_CAMPFIRE: u64 = 0x4d80_5a8c_13e7_21ff;
    const WW_CAMPFIRE: u64 = 0x4db8_5a09_3fc9_61a7;

    fn capital(slice: &str, position: Position, rotation: [f32; 4]) -> Capital {
        Capital {
            slice_name: slice.to_owned(),
            position,
            rotation,
            ..Capital::default()
        }
    }

    fn transform(translation: [f32; AXES]) -> Vec<u8> {
        let mut floats = vec![1.0_f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        floats.extend(translation);
        floats
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect()
    }

    /// A `region.slicedata` with one slice that spawns `child` at `translation`.
    fn slicedata(parent: &str, child: &str, translation: [f32; AXES]) -> Element {
        let named = |name: &str, children: Vec<Element>| {
            Element::with_children(lowercase_crc32(name), children)
        };
        let value =
            |name: &str, bytes: &[u8]| Element::with_value(lowercase_crc32(name), bytes.to_vec());
        let entry = named(
            "element",
            vec![
                value("worldTM", &transform(translation)),
                named("sliceAssetId", vec![value("guid", &WATCHTOWER_CAMP)]),
                value("sliceName", child.as_bytes()),
                value("variationName", b""),
            ],
        );
        let pair = named(
            "element",
            vec![
                named(
                    "value1",
                    vec![
                        value("SliceName", parent.as_bytes()),
                        value("VariantName", b""),
                    ],
                ),
                named("value2", vec![named("spawners", vec![entry])]),
            ],
        );
        let root = Element {
            children: vec![named("sliceMetaDataMap", vec![pair])],
            ..Element::default()
        };
        parse_object_stream(&write_object_stream(&root).expect("write")).expect("read")
    }

    #[test]
    fn the_campfires_get_their_retail_ids() {
        let mut spawners = SliceSpawners::default();
        spawners
            .add_region(&slicedata(MB_WATCHTOWER, CAMP_SLICE, CAMP_TRANSLATION))
            .expect("mb");
        spawners
            .add_region(&slicedata(WW_WATCHTOWER, CAMP_SLICE, CAMP_TRANSLATION))
            .expect("ww");
        // The two watchtower capitals of the world file.
        let capitals = [
            capital(
                MB_WATCHTOWER,
                [7_306.045_4, 3_091.882_3, 44.987_1],
                [0.0, 0.0, 0.069_756_5, 0.997_564],
            ),
            capital(
                WW_WATCHTOWER,
                [9_604.494, 2_368.597_2, 44.688_2],
                [0.0, 0.0, 0.993_571_94, 0.113_202_296],
            ),
        ];
        let spawned = spawners.spawned_by(&capitals);
        let ids: Vec<u64> = spawned.iter().map(|slice| slice.id).collect();
        assert_eq!(ids, [MB_CAMPFIRE, WW_CAMPFIRE]);
        assert_eq!(spawned[0].slice, CAMP_SLICE);
        let [x, y, _] = spawned[0].position;
        assert!((x - 7_303.879).abs() < 0.01 && (y - 3_091.926).abs() < 0.01);
    }

    #[test]
    fn the_id_packs_position_yaw_and_asset() {
        // Captured slice-placed entities (ledger scan of zero-suffix cards, 2026-09-18): the
        // spawned slice's world position, yaw (radians) and asset GUID's first two bytes, and
        // the retail id.
        const TINDERBOX: [u8; 2] = [0xb0, 0x0a];
        const TELEPORTER: [u8; 2] = [0xb4, 0xc8];
        const WAR_BOARD: [u8; 2] = [0xda, 0x7d];
        const TRAINING_DUMMY: [u8; 2] = [0xe0, 0x85];
        let cases = [
            (
                [5_510.780, 4_359.501, 91.792],
                3.1867,
                TINDERBOX,
                0x5838_b791_0785_61b7,
            ),
            (
                [4_810.167, 7_921.592, 206.923],
                5.8025,
                TINDERBOX,
                0x5871_9e1e_f1a4_b28f,
            ),
            (
                [3_879.066, 6_983.294, 156.701],
                1.1115,
                TELEPORTER,
                0x5a11_399b_4743_c9cf,
            ),
            (
                [5_556.846, 4_310.045, 42.411],
                2.8972,
                TELEPORTER,
                0x5a38_5510_d605_6d3f,
            ),
            (
                [4_760.151, 6_900.389, 154.024],
                4.7808,
                WAR_BOARD,
                0xed59_341a_f464_a60f,
            ),
            (
                [6_953.949, 1_227.112, 66.271],
                4.3808,
                WAR_BOARD,
                0xed50_8484_cb26_ca87,
            ),
            (
                [5_755.128, 7_612.747, 71.721],
                3.9095,
                TRAINING_DUMMY,
                0xf048_8f9d_bcc5_9ecf,
            ),
            (
                [5_748.937, 7_605.253, 71.721],
                4.2586,
                TRAINING_DUMMY,
                0xf050_8f9d_b545_9d3f,
            ),
        ];
        let mut asset = [0; GUID_LEN];
        for (position, yaw, first, id) in cases {
            asset[..first.len()].copy_from_slice(&first);
            let yaw: f64 = yaw;
            assert_eq!(spawned_gde_id(position, yaw.to_degrees(), &asset), id);
        }
        let edge = spawned_gde_id([0.0; AXES], 115.0, &asset);
        assert_eq!((edge >> YAW_SHIFT) & 0xf, 5, "115 degrees is sector 5");
        let turn = spawned_gde_id([0.0; AXES], 360.0, &asset);
        assert_eq!((turn >> YAW_SHIFT) & 0xf, 0, "a full turn is sector 0");
        assert_eq!(turn & SPAWNED_TAG, SPAWNED_TAG);
    }

    #[test]
    fn a_bad_transform_is_an_error() {
        let mut root = slicedata("a", "b", [0.0; AXES]);
        let transform = &mut root.children[0].children[0].children[1].children[0].children[0];
        transform.children[0].value.truncate(FLOAT_BYTES);
        assert_eq!(
            SliceSpawners::default().add_region(&root),
            Err(SliceDataError::Transform(FLOAT_BYTES))
        );
    }
}
