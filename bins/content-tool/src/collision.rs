// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool collision` (level-collision note 2026-09-20): builds the collision file of a
//! level from the game paks, as the client places its solid shapes.
//!
//! The level's capitals (`*.capitals.json`) place slices with a position, a rotation and a scale;
//! its chunk lists (`region.chunks`, else the capitals' `.chunks`) place chunk slices at their
//! position. In a slice, an entity's place is its `GameTransformComponent` `m_worldTM`, in the
//! slice's space (Observed: `m_localTM` equals it in every entity of the build's slices, a child
//! holds nearly its parent's values, and the tutorial's Thorpe arena floor is at the retail own
//! place only without its parent's offset added again). A `PrefabSpawnerComponent` that spawns on
//! initialize places its slice at its entity. An entity with a `GameRigidBodyComponent` is
//! solid when its filter blocks the `Player` filter (`sharedassets/physics/default.collisionfilters`):
//! its shape is the configuration's `m_rnrAsset` when set, else its collision shape, else the
//! physics asset of its `MeshComponent`'s mesh (the `.rnr` at the mesh's path, see
//! [`nw_formats::rnr`]; Observed: the tutorial galleon's `galleon_body.cgf` has none in its rigid
//! body and its `galleon_body.rnr` puts the deck at the retail own place, 89.38 m at the
//! tutorial start). An entity whose `IsRuntimeActive` is false does not activate as the level
//! loads: it has no solid, and its spawner spawns nothing then (Observed: the tutorial's
//! `ftue_v2_corruptedwall` slice, whose inactive `mainvfxwall` box stands 82 m wide across the
//! path at 480, 296, which the retail player walked through in both newchar captures).
//!
//! A capital with a `variantName` places its slice as that variant shows it: the slice's
//! `VariationDataComponent` names a variation table (a `VariationData` datasheet) and binds
//! columns to actions on its entities (`Change Mesh` on `StartMesh`); the variant's row gives the
//! values (a tree's own trunk mesh, an ore's own boulder). The gatherable's depleted state
//! ([`DEPLETED_GROUP`]) is not shown as the level loads.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, AssetCatalogError, GUID_LEN, paths_of};
use nw_formats::azcs::{AzcsError, decode_azcs, is_azcs};
use nw_formats::cgf::has_physics_geometry;
use nw_formats::chunk_file::ChunkFileError;
use nw_formats::datasheet::{DATASHEET_EXTENSION, Datasheet, DatasheetError};
use nw_formats::objectstream::{Element, ObjectStreamError, lowercase_crc32, parse_object_stream};
use nw_formats::pak::{PakArchive, PakEntry, PakError, find_paks};
use nw_formats::rnr::{RNR_EXTENSION, RnrError, Shape, ShapeKind, parse_rnr};
use nw_oodle::KrakenDecompressor;
use nw_world::capitals::parse_capitals;
use nw_world::collision::{
    Affine, Body, DeferredSlice, Geometry, IDENTITY_AFFINE, LevelCollision, collision_file_path,
    compose, hull_planes, place_of,
};
use nw_world::collision_index::CollisionIndex;

use crate::world::{LEVELS_PREFIX, in_parallel};

/// The collision filter file.
const COLLISION_FILTERS_ENTRY: &str = "sharedassets/physics/default.collisionfilters";
/// The filter of the player's character (`slices/player.dynamicslice`: the capsule shape
/// component's `m_strFilter`).
const PLAYER_FILTER: &str = "Player";
/// Placements of slices in a capitals file.
const CAPITALS_EXTENSION: &str = ".capitals.json";
/// The chunk lists of capitals.
const CHUNKS_EXTENSION: &str = ".chunks";
/// The union of a region's capital chunk lists (open world only).
const REGION_CHUNKS_FILE: &str = "region.chunks";
const REGIONS_DIR: &str = "regions/";
const PATH_SEPARATOR: char = '/';
/// The capitals entry of a slice placed by a plot (its content is in the chunks).
const PLOT_SLICE: &str = "<PLOT>";
/// How deep spawners may spawn spawners (our choice: the build's deepest chain is far shorter;
/// a cycle stops here).
const MAX_SPAWN_DEPTH: usize = 8;
/// The bytes of an `Asset` value before its path: the GUID, the sub id and padding, the type.
const ASSET_SUB_ID_AT: usize = GUID_LEN;
const ASSET_SUB_ID_END: usize = ASSET_SUB_ID_AT + 4;
/// Floats in a stored `Transform`: the 3 x 3 part by columns, then the translation (Inferred:
/// the order of `AZ::Transform`'s column-major store; a slab rotated about z by a capital puts
/// the tutorial deck at the retail own place either way, so the open-world roofs check it).
const TRANSFORM_FLOATS: usize = 12;
const LINEAR_FLOATS: usize = 9;
const AXES: usize = 3;
/// Half, for half sizes.
const HALF: f32 = 0.5;
/// The entity of a gatherable's master slice that holds its depleted state: shown once it is
/// gathered, not as the level loads (Observed in the build's master gatherable slices: a
/// `Healthy` and a `Depleted` entity under the root, the tree's stump and the ore's fragments in
/// `Depleted`).
const DEPLETED_GROUP: &str = "Depleted";
/// The datasheet type of the variation tables.
const VARIATION_DATA_TYPE: &str = "VariationData";
/// The variation tables' column of the variant id (a capital's `variantName`).
const VARIANT_ID_COLUMN: &str = "VariantID";
/// Variation actions that change a solid entity (the names the build's slices bind).
const CHANGE_MESH: &str = "Change Mesh";
const SET_COLLISION_ASSET: &str = "Set Collision Asset Override";
const SET_STATIC_COLLISION: &str = "Set Should Have Static Collision";
/// `Change Transform Scale Multiplier X` and the translation actions, by axis letter.
const CHANGE_SCALE_PREFIX: &str = "Change Transform Scale Multiplier ";
const CHANGE_TRANSLATION_PREFIX: &str = "Change Transform Translation ";
const AXIS_LETTERS: [&str; AXES] = ["X", "Y", "Z"];
/// A true value of a variation table's flag column.
const TRUE_VALUES: [&str; 2] = ["1", "true"];

/// ObjectStream type UUIDs of the components read here (raw stream order; names from the
/// install's XML ObjectStreams, `mission0.entities_xml`).
mod types {
    pub const GAME_TRANSFORM: [u8; 16] =
        *b"\x48\x4a\xe6\x7d\xab\xd0\x4d\x9c\xb2\xc8\x9b\xb0\xee\xc9\x00\xe0";
    pub const GAME_RIGID_BODY: [u8; 16] =
        *b"\x5b\xa2\xd1\xfc\xdb\x9d\x4b\x81\x88\xc9\x89\x78\x7d\x5c\x19\xf1";
    pub const MESH: [u8; 16] = *b"\x2f\x4b\xad\x46\xc8\x57\x4d\xcb\xa4\x54\xc4\x12\xde\x67\x85\x2a";
    pub const PREFAB_SPAWNER: [u8; 16] =
        *b"\xb5\x2d\x0d\x52\x99\xcd\x41\xbe\xb6\xf7\xd2\x51\x9c\x15\xbf\x14";
    pub const QUERY_SHAPE_SPHERE: [u8; 16] =
        *b"\x7f\x2e\xf3\x12\x40\x89\x45\x82\x89\xc5\x5d\x41\x56\xda\xa7\xfb";
    // The query shapes: names from the client's reflection strings, UUIDs from the build's
    // slices, fields by name CRC (`QueryShapeBox` `m_box`, ...).
    /// `QueryShapeAabb` (`m_aabb`).
    pub const QUERY_SHAPE_AABB: [u8; 16] =
        *b"\x27\x46\x20\x17\xfe\x0f\x4b\x81\x96\xe9\x88\x75\xb7\x50\xec\x69";
    /// `QueryShapeCylinder` (`m_height`, `m_radius`).
    pub const QUERY_SHAPE_CYLINDER: [u8; 16] =
        *b"\x70\x9b\x11\xea\xfd\x56\x4f\xef\xb8\x41\x7c\xea\x54\x93\x68\xe6";
    /// `QueryShapeCapsule` (`m_height`, `m_radius`, `m_axis`).
    pub const QUERY_SHAPE_CAPSULE: [u8; 16] =
        *b"\x74\x95\xc6\x5c\x91\x93\x41\x93\xbb\xb2\xde\x33\x43\xb9\xeb\x03";
    /// `QueryShapeBox` (`m_box`).
    pub const QUERY_SHAPE_BOX: [u8; 16] =
        *b"\xc6\x65\x1a\x66\x23\xd4\x45\x08\xb4\xad\x18\x0c\x51\x66\x55\xa8";
    /// `Javelin::VariationDataComponent` (`{4A2D9C0A-B147-4186-A9E0-5805B610D3CC}` in the
    /// client's reflection, `FUN_1442dbfb0`).
    pub const VARIATION_DATA: [u8; 16] =
        *b"\x4a\x2d\x9c\x0a\xb1\x47\x41\x86\xa9\xe0\x58\x05\xb6\x10\xd3\xcc";
}

/// An asset id: GUID and sub id.
pub type AssetKey = ([u8; GUID_LEN], u32);

/// A failure of the collision build.
#[derive(Debug, thiserror::Error)]
pub enum CollisionError {
    /// A pak cannot be read.
    #[error(transparent)]
    Pak(#[from] PakError),
    /// A file the build needs is missing from the paks.
    #[error("{0} is missing from the paks")]
    Missing(String),
    /// The asset catalog cannot be read.
    #[error("asset catalog: {0}")]
    Catalog(#[from] AssetCatalogError),
    /// An AZCS wrapper cannot be decoded.
    #[error("{path}: {source}")]
    Azcs {
        /// The entry.
        path: String,
        /// Why.
        source: AzcsError,
    },
    /// An ObjectStream cannot be read.
    #[error("{path}: {source}")]
    ObjectStream {
        /// The entry.
        path: String,
        /// Why.
        source: ObjectStreamError,
    },
    /// A capitals file cannot be read.
    #[error("{path}: {source}")]
    Json {
        /// The entry.
        path: String,
        /// Why.
        source: serde_json::Error,
    },
    /// A datasheet cannot be read.
    #[error("{path}: {source}")]
    Datasheet {
        /// The entry.
        path: String,
        /// Why.
        source: DatasheetError,
    },
    /// A mesh file cannot be read.
    #[error("{path}: {source}")]
    Mesh {
        /// The entry.
        path: String,
        /// Why.
        source: ChunkFileError,
    },
    /// A physics asset cannot be read.
    #[error("{path}: {source}")]
    Rnr {
        /// The entry.
        path: String,
        /// Why.
        source: RnrError,
    },
    /// The level has no capitals.
    #[error("level {0} has no capitals")]
    NoLevel(String),
    /// The collision file cannot be written or read back.
    #[error("collision file: {0}")]
    File(#[from] nw_content::ContainerError),
}

/// Every pak entry by path.
pub struct PakIndex {
    paks: Vec<PathBuf>,
    entries: HashMap<String, (usize, PakEntry)>,
}

/// Entries read by one worker from one pak (our choice: few central-directory reads, many
/// workers).
const ENTRIES_PER_WORK: usize = 256;

impl PakIndex {
    /// The entries of every pak under `<game_dir>/assets`; the first pak (in path order) of a
    /// path wins.
    ///
    /// # Errors
    ///
    /// Returns [`PakError`] when a pak cannot be listed.
    pub fn open(game_dir: &Path) -> Result<Self, PakError> {
        let paks = find_paks(game_dir)?;
        let listed = in_parallel(&paks, |pak| PakArchive::open(pak)?.entries());
        let mut entries = HashMap::new();
        for (index, listing) in listed.into_iter().enumerate() {
            for entry in listing? {
                entries.entry(entry.path.clone()).or_insert((index, entry));
            }
        }
        Ok(Self { paks, entries })
    }

    /// The paths of the entries that pass `select`.
    pub fn paths(&self, select: impl Fn(&str) -> bool) -> Vec<String> {
        let mut paths: Vec<String> = self
            .entries
            .keys()
            .filter(|path| select(path))
            .cloned()
            .collect();
        paths.sort();
        paths
    }

    /// Reads the entries `paths` on every core and gives each to `visit`; a path that is no
    /// entry is left out.
    pub fn read_each<R: Send>(
        &self,
        paths: &[String],
        visit: impl Fn(&str, Vec<u8>) -> R + Sync,
    ) -> Vec<(String, Result<R, PakError>)> {
        let mut by_pak: BTreeMap<usize, Vec<&PakEntry>> = BTreeMap::new();
        for path in paths {
            if let Some((pak, entry)) = self.entries.get(path) {
                by_pak.entry(*pak).or_default().push(entry);
            }
        }
        let work: Vec<(usize, Vec<&PakEntry>)> = by_pak
            .into_iter()
            .flat_map(|(pak, entries)| {
                entries
                    .chunks(ENTRIES_PER_WORK)
                    .map(|chunk| (pak, chunk.to_vec()))
                    .collect::<Vec<_>>()
            })
            .collect();
        in_parallel(&work, |(pak, entries)| {
            let archive = PakArchive::open(&self.paks[*pak]);
            match archive {
                Ok(mut archive) => entries
                    .iter()
                    .map(|entry| {
                        let read = archive
                            .read(entry, &KrakenDecompressor)
                            .map(|bytes| visit(&entry.path, bytes));
                        (entry.path.clone(), read)
                    })
                    .collect::<Vec<_>>(),
                Err(error) => {
                    let message = error.to_string();
                    entries
                        .iter()
                        .map(|entry| {
                            (
                                entry.path.clone(),
                                Err(PakError::Io {
                                    path: self.paks[*pak].clone(),
                                    source: std::io::Error::other(message.clone()),
                                }),
                            )
                        })
                        .collect()
                }
            }
        })
        .into_iter()
        .flatten()
        .collect()
    }

    /// Reads one entry.
    fn read(&self, path: &str) -> Result<Vec<u8>, CollisionError> {
        let (pak, entry) = self
            .entries
            .get(path)
            .ok_or_else(|| CollisionError::Missing(path.to_owned()))?;
        Ok(PakArchive::open(&self.paks[*pak])?.read(entry, &KrakenDecompressor)?)
    }
}

/// Where a solid entity's shape comes from.
#[derive(Debug, Clone, PartialEq)]
enum ShapeSource {
    /// A physics asset by id.
    Rnr(AssetKey),
    /// The physics asset of a mesh asset.
    Mesh(AssetKey),
    /// The physics asset of the mesh at a pak path (a variant's mesh).
    MeshPath(String),
    /// The rigid body's own query shape, placed in the entity's space.
    Own(Geometry, Affine),
}

/// One spawner of a slice: where it places the slice, the slice, and whether it spawns when the
/// level loads (else later: an encounter, an event).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Spawn {
    place: Affine,
    slice: AssetKey,
    on_initialize: bool,
}

/// One solid entity of a slice.
#[derive(Debug, Clone, PartialEq)]
struct SolidEntity {
    /// Its place in the slice.
    place: Affine,
    /// Its shape; `None` when it names none (a variant may give it a mesh).
    source: Option<ShapeSource>,
    /// Its entity id, which a variation binding names.
    entity: Option<u64>,
    /// It is in the gatherable's depleted state ([`DEPLETED_GROUP`]).
    depleted: bool,
}

/// What one slice holds that the collision needs.
#[derive(Debug, Clone, Default)]
struct SliceSolids {
    /// Each solid entity.
    bodies: Vec<SolidEntity>,
    /// Each slice its spawners place.
    spawns: Vec<Spawn>,
    /// Solid entities whose query shape is of a kind no slice of the build uses with a rigid
    /// body (`QueryShapePoint`, `QueryShapePolygonPrismTEMP`).
    unread: usize,
    /// Entities with a rigid body that do not activate as the level loads.
    inactive: usize,
    /// The slice's variation data component, when it has one.
    variation: Option<Variation>,
}

/// A slice's `VariationDataComponent`: its variation table and what each column changes.
#[derive(Debug, Clone, Default, PartialEq)]
struct Variation {
    /// The table (a `VariationData` datasheet's id, `Gatherable_Trees`).
    table: String,
    /// Each column's action on an entity.
    bindings: Vec<VariationBinding>,
}

/// One binding of a variation: the entity, the action (`Change Mesh`) and the column.
#[derive(Debug, Clone, PartialEq)]
struct VariationBinding {
    entity: u64,
    action: String,
    column: String,
}

/// Every variation table by id: each variant by id, its values by column.
type VariationTables = HashMap<String, HashMap<String, HashMap<String, String>>>;

/// One entity of a slice as read.
#[derive(Debug, Default)]
struct SliceEntity {
    id: Option<u64>,
    name: String,
    /// Its `IsRuntimeActive` is false: it does not activate as the level loads.
    inactive: bool,
    parent: Option<u64>,
    place: Option<Affine>,
    body: Option<RigidBody>,
    mesh: Option<AssetKey>,
    spawn: Option<(AssetKey, bool)>,
    variation: Option<Variation>,
}

/// A `GameRigidBodyComponent`'s configuration.
#[derive(Debug, Clone)]
struct RigidBody {
    filter: String,
    rnr: Option<AssetKey>,
    shape: Option<Element>,
    centre: [f32; AXES],
}

/// Name CRCs of the fields read here.
struct Names {
    components: u32,
    entities: u32,
    world_tm: u32,
    configuration: u32,
    rnr_asset: u32,
    collision_shape: u32,
    filter: u32,
    center: u32,
    radius: u32,
    half_box: u32,
    half_aabb: u32,
    height: u32,
    axis: u32,
    render_node: u32,
    static_mesh: u32,
    slice_asset: u32,
    spawn_on_initialize: u32,
    is_runtime_active: u32,
    id: u32,
    entity_name: u32,
    parent_id: u32,
    entity_id: u32,
    binding_entity: u32,
    binding_action: u32,
}

impl Names {
    fn new() -> Self {
        Self {
            components: lowercase_crc32("Components"),
            entities: lowercase_crc32("Entities"),
            world_tm: lowercase_crc32("m_worldTM"),
            configuration: lowercase_crc32("m_configuration"),
            rnr_asset: lowercase_crc32("m_rnrAsset"),
            collision_shape: lowercase_crc32("m_collisionShape"),
            filter: lowercase_crc32("m_strFilter"),
            center: lowercase_crc32("m_center"),
            radius: lowercase_crc32("m_radius"),
            half_box: lowercase_crc32("m_box"),
            half_aabb: lowercase_crc32("m_aabb"),
            height: lowercase_crc32("m_height"),
            axis: lowercase_crc32("m_axis"),
            render_node: lowercase_crc32("Static Mesh Render Node"),
            static_mesh: lowercase_crc32("Static Mesh"),
            slice_asset: lowercase_crc32("m_sliceAsset"),
            spawn_on_initialize: lowercase_crc32("m_spawnOnInitialize"),
            is_runtime_active: lowercase_crc32("IsRuntimeActive"),
            id: lowercase_crc32("Id"),
            entity_name: lowercase_crc32("name"),
            parent_id: lowercase_crc32("m_parentId"),
            entity_id: lowercase_crc32("EntityId"),
            binding_entity: lowercase_crc32("m_entity"),
            binding_action: lowercase_crc32("m_action"),
        }
    }
}

/// The asset id of an `Asset` value, or `None` for a null asset.
fn asset_key(element: &Element) -> Option<AssetKey> {
    let guid: [u8; GUID_LEN] = element.value.get(..GUID_LEN)?.try_into().ok()?;
    if guid == [0; GUID_LEN] {
        return None;
    }
    let sub: [u8; 4] = element
        .value
        .get(ASSET_SUB_ID_AT..ASSET_SUB_ID_END)?
        .try_into()
        .ok()?;
    Some((guid, u32::from_be_bytes(sub)))
}

/// The [`Affine`] of a stored `Transform` value (big-endian floats; the 3 x 3 part by columns).
fn transform_of(element: &Element) -> Option<Affine> {
    if element.value.len() != TRANSFORM_FLOATS * size_of::<f32>() {
        return None;
    }
    let float = |index: usize| {
        let start = index * size_of::<f32>();
        f32::from_be_bytes([
            element.value[start],
            element.value[start + 1],
            element.value[start + 2],
            element.value[start + 3],
        ])
    };
    let mut out = [[0.0; 4]; AXES];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (column, value) in out_row.iter_mut().enumerate() {
            *value = if column == AXES {
                float(LINEAR_FLOATS + row)
            } else {
                float(column * AXES + row)
            };
        }
    }
    Some(out)
}

/// The entities of a slice root (`AZ::Entity` > `Components` > `SliceComponent` > `Entities`).
fn slice_entities(root: &Element, names: &Names) -> Vec<SliceEntity> {
    let mut entities = Vec::new();
    let lists = root
        .child(names.components)
        .into_iter()
        .flat_map(|components| &components.children)
        .filter_map(|component| component.child(names.entities));
    for list in lists {
        for entity in &list.children {
            let mut read = SliceEntity {
                id: entity
                    .child(names.id)
                    .and_then(|id| id.child(names.id))
                    .and_then(|id| id.as_u64().ok()),
                name: entity
                    .child(names.entity_name)
                    .and_then(|name| name.as_string().ok())
                    .unwrap_or_default()
                    .to_owned(),
                inactive: entity
                    .child(names.is_runtime_active)
                    .and_then(|active| active.as_bool().ok())
                    == Some(false),
                ..SliceEntity::default()
            };
            let components = entity
                .child(names.components)
                .map_or(&[][..], |list| list.children.as_slice());
            for component in components {
                read_component(component, names, &mut read);
            }
            entities.push(read);
        }
    }
    entities
}

/// Reads the parts of `component` this build uses into `entity`.
fn read_component(component: &Element, names: &Names, entity: &mut SliceEntity) {
    match component.type_id {
        types::GAME_TRANSFORM => {
            entity.place = component.child(names.world_tm).and_then(transform_of);
            entity.parent = component
                .child(names.parent_id)
                .and_then(|parent| parent.child(names.id))
                .and_then(|id| id.as_u64().ok());
        }
        types::VARIATION_DATA => entity.variation = Some(variation_of(component, names)),
        types::GAME_RIGID_BODY => {
            let Some(config) = component.child(names.configuration) else {
                return;
            };
            entity.body = Some(RigidBody {
                filter: config
                    .child(names.filter)
                    .and_then(|filter| filter.as_string().ok())
                    .unwrap_or_default()
                    .to_owned(),
                rnr: config.child(names.rnr_asset).and_then(asset_key),
                shape: config
                    .child(names.collision_shape)
                    .and_then(|shape| shape.children.first())
                    .cloned(),
                centre: config
                    .child(names.center)
                    .and_then(|centre| centre.as_vec3().ok())
                    .unwrap_or([0.0; AXES]),
            });
        }
        types::MESH => {
            entity.mesh = component
                .child(names.render_node)
                .and_then(|node| node.child(names.static_mesh))
                .and_then(asset_key);
        }
        types::PREFAB_SPAWNER => {
            let on_initialize = component
                .child(names.spawn_on_initialize)
                .and_then(|flag| flag.as_bool().ok())
                .unwrap_or(false);
            entity.spawn = component
                .child(names.slice_asset)
                .and_then(asset_key)
                .map(|slice| (slice, on_initialize));
        }
        // Other spawners (an encounter's spawn group, an event's) name their slice in their
        // server part and place it on a trigger, not when the level loads.
        _ => {
            if let Some(slice) = slice_asset_in(component, names) {
                entity.spawn = Some((slice, false));
            }
        }
    }
}

/// The first non-null `m_sliceAsset` under `element`.
fn slice_asset_in(element: &Element, names: &Names) -> Option<AssetKey> {
    element.children.iter().find_map(|child| {
        if child.name_crc == Some(names.slice_asset) {
            asset_key(child)
        } else {
            slice_asset_in(child, names)
        }
    })
}

/// The solids and spawns of a slice root, for the filters in `solid_filters`.
fn slice_solids(root: &Element, names: &Names, solid_filters: &HashSet<String>) -> SliceSolids {
    let entities = slice_entities(root, names);
    let parents: HashMap<u64, (Option<u64>, &str)> = entities
        .iter()
        .filter_map(|entity| Some((entity.id?, (entity.parent, entity.name.as_str()))))
        .collect();
    let mut solids = SliceSolids::default();
    for entity in &entities {
        if solids.variation.is_none() {
            solids.variation.clone_from(&entity.variation);
        }
        let place = entity.place.unwrap_or(IDENTITY_AFFINE);
        if let Some((slice, on_initialize)) = entity.spawn {
            solids.spawns.push(Spawn {
                place,
                slice,
                on_initialize: on_initialize && !entity.inactive,
            });
        }
        let Some(body) = &entity.body else {
            continue;
        };
        if entity.inactive {
            solids.inactive += 1;
            continue;
        }
        if !solid_filters.contains(&body.filter) {
            continue;
        }
        let source = if let Some(rnr) = body.rnr {
            Some(ShapeSource::Rnr(rnr))
        } else if let Some(shape) = &body.shape {
            let own = query_shape(shape, body.centre, names);
            if own.is_none() {
                solids.unread += 1;
                continue;
            }
            own
        } else {
            entity.mesh.map(ShapeSource::Mesh)
        };
        solids.bodies.push(SolidEntity {
            place,
            source,
            entity: entity.id,
            depleted: in_depleted_group(entity, &parents),
        });
    }
    solids
}

/// Whether `entity` is a gatherable's depleted state ([`DEPLETED_GROUP`]) or in it.
fn in_depleted_group(entity: &SliceEntity, parents: &HashMap<u64, (Option<u64>, &str)>) -> bool {
    if entity.name == DEPLETED_GROUP {
        return true;
    }
    let mut next = entity.parent;
    for _ in 0..parents.len() {
        let Some((parent, name)) = next.and_then(|id| parents.get(&id)) else {
            return false;
        };
        if *name == DEPLETED_GROUP {
            return true;
        }
        next = *parent;
    }
    false
}

/// A `VariationDataComponent`'s table and bindings (field names Unknown, meaning from the
/// build's gatherable slices: the table `Gatherable_Trees`, then per binding an entity, an
/// action and a column of the table).
fn variation_of(component: &Element, names: &Names) -> Variation {
    /// The variation table's name.
    const TABLE: u32 = 0x095a_1d04;
    /// The list of bindings.
    const BINDINGS: u32 = 0xaedd_dde9;
    /// A binding's column.
    const COLUMN: u32 = 0xa77a_4396;
    let text = |element: Option<&Element>| {
        element
            .and_then(|value| value.as_string().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let bindings = component
        .child(BINDINGS)
        .into_iter()
        .flat_map(|list| &list.children)
        .filter_map(|binding| {
            Some(VariationBinding {
                entity: binding
                    .child(names.binding_entity)?
                    .child(names.entity_id)?
                    .child(names.id)?
                    .as_u64()
                    .ok()?,
                action: text(binding.child(names.binding_action)),
                column: text(binding.child(COLUMN)),
            })
        })
        .collect();
    Variation {
        table: text(component.child(TABLE)),
        bindings,
    }
}

/// The shape of a rigid body's own query shape, centred on `centre` (`m_center`), as the
/// client turns it into a physics shape (`QueryShape*` virtual methods, `FUN_14169e770` box and
/// box-like shapes, `FUN_14169ea40` cylinder, `FUN_1416987a0` capsule):
///
/// - sphere: `m_radius`;
/// - box: `m_box`, its half size (the client builds a `QueryShapeBox` from bounds as
///   `(max - min) * 0.5`, `FUN_143534110` with the constant 0.5 at `0x147f41308`, and makes the
///   physics box of the half sizes as they are);
/// - axis-aligned box: `m_aabb`, its half size (the same physics box method);
/// - cylinder: radius `m_radius` from the origin up `m_height`;
/// - capsule: radius `m_radius` around a segment `m_height` long along `m_axis` (up when it is
///   zero); a capsule along x or y lies on the origin (its middle a radius up), any other stands
///   on it (its middle half the length and a radius up).
///
/// `None` for the other kinds.
fn query_shape(shape: &Element, centre: [f32; AXES], names: &Names) -> Option<ShapeSource> {
    let float = |name: u32| shape.child(name).and_then(|value| value.as_f32().ok());
    let vector = |name: u32| shape.child(name).and_then(|value| value.as_vec3().ok());
    let mut place = IDENTITY_AFFINE;
    for (axis, value) in centre.iter().enumerate() {
        place[axis][AXES] = *value;
    }
    let geometry = match shape.type_id {
        types::QUERY_SHAPE_SPHERE => Geometry::Sphere {
            radius: float(names.radius)?,
        },
        types::QUERY_SHAPE_BOX => Geometry::Box {
            half_extents: vector(names.half_box)?,
        },
        types::QUERY_SHAPE_AABB => Geometry::Box {
            half_extents: vector(names.half_aabb)?,
        },
        types::QUERY_SHAPE_CYLINDER => Geometry::Cylinder {
            from: [0.0; AXES],
            to: [0.0, 0.0, float(names.height)?],
            radius: float(names.radius)?,
        },
        types::QUERY_SHAPE_CAPSULE => {
            let (height, radius) = (float(names.height)?, float(names.radius)?);
            capsule(height, radius, vector(names.axis).unwrap_or([0.0; AXES]))
        }
        _ => return None,
    };
    Some(ShapeSource::Own(geometry, place))
}

/// The physics capsule of a `QueryShapeCapsule` (see [`query_shape`]).
fn capsule(height: f32, radius: f32, axis: [f32; AXES]) -> Geometry {
    const UP: [f32; AXES] = [0.0, 0.0, 1.0];
    const X: [f32; AXES] = [1.0, 0.0, 0.0];
    const Y: [f32; AXES] = [0.0, 1.0, 0.0];
    let length = axis.iter().map(|value| value * value).sum::<f32>().sqrt();
    let axis = if length > 0.0 {
        axis.map(|value| value / length)
    } else {
        UP
    };
    let half = height * HALF;
    let lying = axis == X || axis == Y;
    let middle = if lying { radius } else { half + radius };
    Geometry::Capsule {
        from: [-axis[0] * half, -axis[1] * half, middle - axis[2] * half],
        to: [axis[0] * half, axis[1] * half, middle + axis[2] * half],
        radius,
    }
}

/// The filters that block `player` (`default.collisionfilters`): a filter blocks it when the
/// categories it is in meet the categories the player collides with, each with its base
/// filters' (Inferred from the file's content: per filter its name, its description, its base
/// filters, the categories it is in and the categories it collides with; `Structure` is in
/// `Static` by its base and `Player` collides with `Static` by its base `Character`).
fn blocking_filters(root: &Element, player: &str) -> HashSet<String> {
    /// Name CRCs of the filter fields (no name in the install; meaning from the content).
    const FILTERS: u32 = 0x7877_678d;
    const BASES: u32 = 0xa1ef_e138;
    const CATEGORIES: u32 = 0x7097_8a1a;
    const COLLIDES: u32 = 0xe6a3_571c;
    let name_crc = lowercase_crc32("name");
    let strings = |element: Option<&Element>| -> Vec<String> {
        element
            .into_iter()
            .flat_map(|list| &list.children)
            .filter_map(|item| item.as_string().ok().map(str::to_owned))
            .collect()
    };
    let mut filters: HashMap<String, FilterLists> = HashMap::new();
    for filter in root
        .child(FILTERS)
        .into_iter()
        .flat_map(|list| &list.children)
    {
        let Some(name) = filter
            .child(name_crc)
            .and_then(|name| name.as_string().ok())
        else {
            continue;
        };
        filters.insert(
            name.to_owned(),
            FilterLists {
                bases: strings(filter.child(BASES)),
                categories: strings(filter.child(CATEGORIES)),
                collides: strings(filter.child(COLLIDES)),
            },
        );
    }
    let resolved = |name: &str, pick: fn(&FilterLists) -> &Vec<String>| {
        let mut out = HashSet::new();
        let mut todo = vec![name.to_owned()];
        let mut seen = HashSet::new();
        while let Some(next) = todo.pop() {
            if !seen.insert(next.clone()) {
                continue;
            }
            if let Some(filter) = filters.get(&next) {
                out.extend(pick(filter).iter().cloned());
                todo.extend(filter.bases.iter().cloned());
            }
        }
        out
    };
    let collides = resolved(player, |filter| &filter.collides);
    filters
        .keys()
        .filter(|name| !resolved(name, |filter| &filter.categories).is_disjoint(&collides))
        .cloned()
        .collect()
}

/// One collision filter's lists, each without its base filters' entries.
struct FilterLists {
    /// The filters it extends.
    bases: Vec<String>,
    /// The categories it is in.
    categories: Vec<String>,
    /// The categories it collides with.
    collides: Vec<String>,
}

/// The root of the ObjectStream `bytes` of entry `path`.
fn stream_of(path: &str, bytes: &[u8]) -> Result<Element, CollisionError> {
    let stream = if is_azcs(bytes) {
        Cow::Owned(decode_azcs(bytes).map_err(|source| CollisionError::Azcs {
            path: path.to_owned(),
            source,
        })?)
    } else {
        Cow::Borrowed(bytes)
    };
    parse_object_stream(&stream).map_err(|source| CollisionError::ObjectStream {
        path: path.to_owned(),
        source,
    })
}

/// The asset id of a capitals `sliceAssetId` text `{GUID}:subIdHex`.
pub(crate) fn capital_asset(text: &str) -> Option<AssetKey> {
    let (guid, sub) = text.strip_prefix('{')?.split_once("}:")?;
    let hex: String = guid.chars().filter(|c| *c != '-').collect();
    let bytes = (0..GUID_LEN)
        .map(|index| u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    let guid: [u8; GUID_LEN] = bytes.try_into().ok()?;
    (guid != [0; GUID_LEN]).then_some((guid, u32::from_str_radix(sub, 16).ok()?))
}

/// The chunk slices of a chunk list root: each slice's asset id and world position.
fn chunk_placements(root: &Element) -> Vec<(AssetKey, [f32; AXES])> {
    let chunks = lowercase_crc32("chunks");
    let asset = lowercase_crc32("assetId");
    let guid = lowercase_crc32("guid");
    let sub = lowercase_crc32("subId");
    let position = lowercase_crc32("worldPosition");
    root.child(chunks)
        .into_iter()
        .flat_map(|list| &list.children)
        .filter_map(|chunk| {
            let id = chunk.child(asset)?;
            let guid: [u8; GUID_LEN] = id.child(guid)?.as_uuid().ok()?;
            let sub = id.child(sub)?.as_u32().ok()?;
            let at = chunk.child(position)?.as_vec3().ok()?;
            (guid != [0; GUID_LEN]).then_some(((guid, sub), at))
        })
        .collect()
}

/// The shapes of a physics asset, flattened: each solid shape with its place in the asset's
/// space; and the count of shapes of kinds the server does not read.
fn flatten_shapes(shape: &Shape, parent: &Affine, out: &mut Vec<(Affine, Geometry)>) -> usize {
    let place = shape
        .pose
        .as_ref()
        .map_or(*parent, |pose| compose(parent, pose));
    match &shape.kind {
        ShapeKind::Box { half_extents, .. } => out.push((
            place,
            Geometry::Box {
                half_extents: *half_extents,
            },
        )),
        ShapeKind::Sphere { radius } => out.push((place, Geometry::Sphere { radius: *radius })),
        ShapeKind::Capsule { from, to, radius } => out.push((
            place,
            Geometry::Capsule {
                from: *from,
                to: *to,
                radius: *radius,
            },
        )),
        ShapeKind::Cylinder {
            from, to, radius, ..
        } => out.push((
            place,
            Geometry::Cylinder {
                from: *from,
                to: *to,
                radius: *radius,
            },
        )),
        ShapeKind::ConvexHull { points, .. } => out.push((
            place,
            Geometry::Convex {
                planes: hull_planes(points),
                points: points.clone(),
            },
        )),
        ShapeKind::TriangleMesh { points, triangles } => out.push((
            place,
            Geometry::Mesh {
                points: points.clone(),
                triangles: triangles.clone(),
            },
        )),
        ShapeKind::Compound(children) => {
            return children
                .iter()
                .map(|(child_place, child)| {
                    flatten_shapes(child, &compose(&place, child_place), out)
                })
                .sum();
        }
        ShapeKind::Transformed(child_place, child) => {
            return flatten_shapes(child, &compose(&place, child_place), out);
        }
        ShapeKind::Scaled(scale, child) => {
            let mut scaled = IDENTITY_AFFINE;
            for axis in 0..AXES {
                scaled[axis][axis] = scale[axis];
            }
            return flatten_shapes(child, &compose(&place, &scaled), out);
        }
        ShapeKind::Unsupported { .. } => return 1,
    }
    0
}

/// What a level's collision build found.
#[derive(Debug, Clone, Default)]
pub struct CollisionReport {
    /// The level.
    pub level: String,
    /// The file written.
    pub path: PathBuf,
    /// Its size in bytes.
    pub file_bytes: u64,
    /// Slice placements read (capitals, chunks and spawns).
    pub placements: usize,
    /// Capitals that place a variant of their slice.
    pub variant_placements: usize,
    /// Of those, the ones whose variant no variation table of their slice holds (placed as the
    /// slice is).
    pub unknown_variants: usize,
    /// Distinct slices read.
    pub slices: usize,
    /// Solid entities placed when the level loads.
    pub solid_entities: usize,
    /// Solid entities that name no shape (no physics asset, query shape or mesh, and no mesh
    /// from a variant).
    pub shapeless_entities: usize,
    /// Solid entities whose query shape is of a kind the server does not read.
    pub unread_entities: usize,
    /// Entities with a rigid body that do not activate as the level loads (no solid).
    pub inactive_entities: usize,
    /// Solid entities whose mesh carries no physics geometry (no `.rnr` either).
    pub meshes_without_physics: usize,
    /// Solid entities whose mesh carries physics geometry but has no `.rnr`.
    pub meshes_with_physics_without_rnr: usize,
    /// Solid entities whose physics asset or mesh is not in the catalog or no `.rnr`.
    pub missing_assets: usize,
    /// Physics shapes of kinds the server does not read.
    pub unsupported_shapes: usize,
    /// Distinct physics assets read.
    pub assets: usize,
    /// Shapes in the file.
    pub geometries: usize,
    /// Placed shapes of the level as it loads.
    pub bodies: usize,
    /// Slices with solids that spawners place later.
    pub deferred_slices: usize,
    /// Their placed shapes.
    pub deferred_bodies: usize,
    /// Triangles over all placed meshes (as it loads).
    pub placed_triangles: usize,
    /// The bytes the loaded index holds.
    pub index_bytes: usize,
}

/// One slice to place: the slice, its place, its spawn depth, and the deferred slice it belongs
/// to (`None`: the level as it loads).
#[derive(Debug, Clone)]
struct Pending {
    slice: AssetKey,
    place: Affine,
    depth: usize,
    deferred: Option<usize>,
    /// The variant the capital places (`variantName`).
    variant: Option<String>,
}

/// The slices placed so far and those still to read.
struct Placements {
    pending: Vec<Pending>,
    total: usize,
}

/// A solid entity placed in the world, and the deferred slice it belongs to.
type PlacedSolid = (Affine, ShapeSource, Option<usize>);

/// Builds, writes and reloads the collision file of `level`.
///
/// # Errors
///
/// Returns [`CollisionError`] when a needed file is missing or cannot be read, or the file
/// cannot be written.
pub fn build_level_collision(
    index: &PakIndex,
    level: &str,
    client_build: &str,
    out_dir: &Path,
) -> Result<CollisionReport, CollisionError> {
    let names = Names::new();
    let filters_root = stream_of(
        COLLISION_FILTERS_ENTRY,
        &index.read(COLLISION_FILTERS_ENTRY)?,
    )?;
    let solid_filters = blocking_filters(&filters_root, PLAYER_FILTER);
    let catalog = index.read(ASSET_CATALOG_ENTRY)?;
    let variation_tables = variation_tables(index)?;
    let mut placements = level_placements(index, level)?;
    let mut solids_by_slice: HashMap<AssetKey, SliceSolids> = HashMap::new();
    let mut placed_solids: Vec<PlacedSolid> = Vec::new();
    let mut deferred_slices: Vec<(AssetKey, [f32; AXES])> = Vec::new();
    let mut report = CollisionReport {
        level: level.to_owned(),
        ..CollisionReport::default()
    };
    while !placements.pending.is_empty() {
        let wanted: HashSet<AssetKey> = placements
            .pending
            .iter()
            .map(|pending| pending.slice)
            .filter(|slice| !solids_by_slice.contains_key(slice))
            .collect();
        let paths = paths_of(&catalog, &wanted)?;
        let by_path: HashMap<&String, AssetKey> =
            paths.iter().map(|(key, path)| (path, *key)).collect();
        let mut path_list: Vec<String> = by_path.keys().map(|path| (*path).clone()).collect();
        path_list.sort();
        for (path, read) in index.read_each(&path_list, |path, bytes| {
            stream_of(path, &bytes).map(|root| slice_solids(&root, &names, &solid_filters))
        }) {
            let solids = read??;
            solids_by_slice.insert(by_path[&path], solids);
        }
        for slice in wanted {
            solids_by_slice.entry(slice).or_default();
        }
        for pending in std::mem::take(&mut placements.pending) {
            let solids = &solids_by_slice[&pending.slice];
            report.unread_entities += solids.unread;
            report.inactive_entities += solids.inactive;
            let row = pending.variant.as_ref().map(|variant| {
                report.variant_placements += 1;
                let row = solids
                    .variation
                    .as_ref()
                    .and_then(|variation| variation_tables.get(&variation.table))
                    .and_then(|table| table.get(variant));
                if row.is_none() {
                    report.unknown_variants += 1;
                }
                row
            });
            for body in &solids.bodies {
                let body = match (row, &solids.variation) {
                    (Some(Some(row)), Some(variation)) => varied(body, variation, row),
                    _ => Some(body.clone()),
                };
                let Some(body) = body else {
                    continue;
                };
                match body.source {
                    Some(source) => placed_solids.push((
                        compose(&pending.place, &body.place),
                        source,
                        pending.deferred,
                    )),
                    None => report.shapeless_entities += 1,
                }
            }
            if pending.depth >= MAX_SPAWN_DEPTH {
                continue;
            }
            for spawn in &solids.spawns {
                let place = compose(&pending.place, &spawn.place);
                let deferred = if spawn.on_initialize {
                    pending.deferred
                } else {
                    deferred_slices.push((
                        spawn.slice,
                        [place[0][AXES], place[1][AXES], place[2][AXES]],
                    ));
                    Some(deferred_slices.len() - 1)
                };
                placements.pending.push(Pending {
                    slice: spawn.slice,
                    place,
                    depth: pending.depth + 1,
                    deferred,
                    variant: None,
                });
            }
        }
        placements.total += placements.pending.len();
    }
    report.placements = placements.total;
    report.slices = solids_by_slice.len();
    report.solid_entities = placed_solids
        .iter()
        .filter(|(_, _, deferred)| deferred.is_none())
        .count();
    let collision = place_solids(
        index,
        &catalog,
        level,
        &placed_solids,
        &deferred_slices,
        &mut report,
    )?;
    let path = collision_file_path(out_dir, client_build, level);
    collision.save(&path, client_build)?;
    let reloaded = LevelCollision::load(&path, client_build)?;
    report.file_bytes = std::fs::metadata(&path)
        .map_err(|source| PakError::Io {
            path: path.clone(),
            source,
        })?
        .len();
    report.geometries = reloaded.geometries.len();
    report.bodies = reloaded.bodies.len();
    report.deferred_slices = reloaded.deferred.len();
    report.deferred_bodies = reloaded
        .deferred
        .iter()
        .map(|slice| slice.bodies.len())
        .sum();
    report.placed_triangles = reloaded
        .bodies
        .iter()
        .map(|body| match &reloaded.geometries[body.geometry as usize] {
            Geometry::Mesh { triangles, .. } => triangles.len(),
            _ => 0,
        })
        .sum();
    report.index_bytes = CollisionIndex::new(reloaded).memory_bytes();
    report.path = path;
    Ok(report)
}

/// The variation tables of the paks: every `VariationData` datasheet by its id.
fn variation_tables(index: &PakIndex) -> Result<VariationTables, CollisionError> {
    let paths = index.paths(|path| path.ends_with(DATASHEET_EXTENSION));
    let mut tables = VariationTables::new();
    for (_, read) in index.read_each(&paths, |path, bytes| {
        Datasheet::parse(&bytes).map_err(|source| CollisionError::Datasheet {
            path: path.to_owned(),
            source,
        })
    }) {
        let sheet = read??;
        if sheet.type_name != VARIATION_DATA_TYPE {
            continue;
        }
        let Some(id_column) = sheet
            .columns
            .iter()
            .position(|column| column.name == VARIANT_ID_COLUMN)
        else {
            continue;
        };
        let table = tables.entry(sheet.unique_id.clone()).or_default();
        for row in &sheet.rows {
            let values = sheet
                .columns
                .iter()
                .zip(row)
                .map(|(column, value)| (column.name.clone(), value.clone()))
                .collect();
            table.insert(row[id_column].clone(), values);
        }
    }
    Ok(tables)
}

/// `body` as variant `row` of `variation` shows it: `None` when the variant has it hidden (the
/// depleted state, or no static collision); its mesh, collision asset, scale and translation as
/// the variant's columns set them (`VariationDataComponent` bindings; an empty value keeps the
/// slice's).
fn varied(
    body: &SolidEntity,
    variation: &Variation,
    row: &HashMap<String, String>,
) -> Option<SolidEntity> {
    if body.depleted {
        return None;
    }
    let bindings = || {
        variation
            .bindings
            .iter()
            .filter(|binding| Some(binding.entity) == body.entity)
    };
    let value_of = |binding: &VariationBinding| {
        row.get(&binding.column)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    };
    // A body with the static-collision flag has collision only when its variant sets it.
    let hidden = bindings()
        .filter(|binding| binding.action == SET_STATIC_COLLISION)
        .any(|flag| {
            !value_of(flag).is_some_and(|value| {
                TRUE_VALUES
                    .iter()
                    .any(|truth| value.eq_ignore_ascii_case(truth))
            })
        });
    if hidden {
        return None;
    }
    let mut varied = body.clone();
    for binding in bindings() {
        let Some(value) = value_of(binding) else {
            continue;
        };
        let action = binding.action.as_str();
        match action {
            CHANGE_MESH => {
                if matches!(
                    varied.source,
                    None | Some(ShapeSource::Mesh(_) | ShapeSource::MeshPath(_))
                ) {
                    varied.source = Some(ShapeSource::MeshPath(pak_path(value)));
                }
            }
            SET_COLLISION_ASSET => varied.source = Some(ShapeSource::MeshPath(pak_path(value))),
            _ => {
                let Ok(number) = value.parse::<f32>() else {
                    continue;
                };
                for (axis, letter) in AXIS_LETTERS.iter().enumerate() {
                    if action.strip_prefix(CHANGE_SCALE_PREFIX) == Some(letter) {
                        for line in &mut varied.place {
                            line[axis] *= number;
                        }
                    }
                    if action.strip_prefix(CHANGE_TRANSLATION_PREFIX) == Some(letter) {
                        varied.place[axis][AXES] = number;
                    }
                }
            }
        }
    }
    Some(varied)
}

/// A variation table's asset path as a pak path (`Objects\Nature\...cgf` is
/// `objects/nature/...cgf`; the paks' paths are lowercase with forward slashes).
fn pak_path(value: &str) -> String {
    value.replace('\\', "/").to_lowercase()
}

/// The slice placements of a level: its capitals and its chunks.
fn level_placements(index: &PakIndex, level: &str) -> Result<Placements, CollisionError> {
    let folder = format!("{LEVELS_PREFIX}{level}{PATH_SEPARATOR}{REGIONS_DIR}");
    let files = index.paths(|path| {
        path.starts_with(&folder)
            && (path.ends_with(CAPITALS_EXTENSION) || path.ends_with(CHUNKS_EXTENSION))
    });
    if files.is_empty() {
        return Err(CollisionError::NoLevel(level.to_owned()));
    }
    let region_lists: HashSet<String> = files
        .iter()
        .filter(|path| path.ends_with(REGION_CHUNKS_FILE))
        .filter_map(|path| path.strip_suffix(REGION_CHUNKS_FILE).map(str::to_owned))
        .collect();
    let chunk_files: Vec<String> = files
        .iter()
        .filter(|path| path.ends_with(CHUNKS_EXTENSION))
        .filter(|path| {
            path.ends_with(REGION_CHUNKS_FILE)
                || !region_lists.iter().any(|region| path.starts_with(region))
        })
        .cloned()
        .collect();
    let capital_files: Vec<String> = files
        .iter()
        .filter(|path| path.ends_with(CAPITALS_EXTENSION))
        .cloned()
        .collect();
    let mut pending = Vec::new();
    for (_, read) in index.read_each(&capital_files, |path, bytes| {
        parse_capitals(&bytes).map_err(|source| CollisionError::Json {
            path: path.to_owned(),
            source,
        })
    }) {
        for capital in read?? {
            if capital.slice_name == PLOT_SLICE {
                continue;
            }
            if let Some(slice) = capital_asset(&capital.slice_asset_id) {
                let place = place_of(
                    capital.rotation,
                    capital.scale.unwrap_or(1.0),
                    capital.position,
                );
                pending.push(Pending {
                    slice,
                    place,
                    depth: 0,
                    deferred: None,
                    variant: capital.variant_name.clone(),
                });
            }
        }
    }
    for (_, read) in index.read_each(&chunk_files, |path, bytes| {
        stream_of(path, &bytes).map(|root| chunk_placements(&root))
    }) {
        for (slice, at) in read?? {
            let mut place = IDENTITY_AFFINE;
            for axis in 0..AXES {
                place[axis][AXES] = at[axis];
            }
            pending.push(Pending {
                slice,
                place,
                depth: 0,
                deferred: None,
                variant: None,
            });
        }
    }
    Ok(Placements {
        total: pending.len(),
        pending,
    })
}

/// The collision of the placed solids: their physics assets read, flattened and shared; the
/// solids of each of `deferred` (a slice and its spawner's place) kept apart.
fn place_solids(
    index: &PakIndex,
    catalog: &[u8],
    level: &str,
    placed: &[PlacedSolid],
    deferred: &[(AssetKey, [f32; AXES])],
    report: &mut CollisionReport,
) -> Result<LevelCollision, CollisionError> {
    let wanted: HashSet<AssetKey> = placed
        .iter()
        .filter_map(|(_, source, _)| match source {
            ShapeSource::Rnr(key) | ShapeSource::Mesh(key) => Some(*key),
            ShapeSource::MeshPath(_) | ShapeSource::Own(..) => None,
        })
        .collect();
    let asset_paths = paths_of(catalog, &wanted)?;
    let rnr_path_of = |source: &ShapeSource| match source {
        // A few rigid bodies name an asset that is no physics asset (a `.slice.meta`): no shape.
        ShapeSource::Rnr(key) => asset_paths
            .get(key)
            .filter(|path| path.ends_with(RNR_EXTENSION))
            .cloned(),
        ShapeSource::Mesh(key) => asset_paths.get(key).map(|path| mesh_rnr_path(path)),
        ShapeSource::MeshPath(path) => Some(mesh_rnr_path(path)),
        ShapeSource::Own(..) => None,
    };
    let mesh_path_of = |source: &ShapeSource| match source {
        ShapeSource::Mesh(key) => asset_paths.get(key).cloned(),
        ShapeSource::MeshPath(path) => Some(path.clone()),
        ShapeSource::Rnr(_) | ShapeSource::Own(..) => None,
    };
    let path_list: Vec<String> = {
        let unique: BTreeSet<String> = placed
            .iter()
            .filter_map(|(_, source, _)| rnr_path_of(source))
            .collect();
        unique.into_iter().collect()
    };
    let mut geometries: Vec<Geometry> = Vec::new();
    let mut shapes_by_path: HashMap<String, Vec<(Affine, u32)>> = HashMap::new();
    for (path, read) in index.read_each(&path_list, |path, bytes| {
        parse_rnr(&bytes).map_err(|source| CollisionError::Rnr {
            path: path.to_owned(),
            source,
        })
    }) {
        let asset = read??;
        let mut flat = Vec::new();
        for shape in &asset.shapes {
            report.unsupported_shapes += flatten_shapes(shape, &IDENTITY_AFFINE, &mut flat);
        }
        let shared = flat
            .into_iter()
            .map(|(place, geometry)| {
                geometries.push(geometry);
                (place, small_index(geometries.len() - 1))
            })
            .collect();
        shapes_by_path.insert(path, shared);
    }
    report.assets = shapes_by_path.len();
    let unread_meshes: Vec<String> = placed
        .iter()
        .filter_map(|(_, source, _)| mesh_path_of(source))
        .filter(|path| !shapes_by_path.contains_key(&mesh_rnr_path(path)))
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect();
    let physics_meshes = meshes_with_physics(index, &unread_meshes)?;
    let mut own_shapes: HashMap<String, u32> = HashMap::new();
    let mut bodies = Vec::new();
    let mut deferred_bodies: Vec<Vec<Body>> = vec![Vec::new(); deferred.len()];
    for (place, source, group) in placed {
        let target = match group {
            Some(group) => &mut deferred_bodies[*group],
            None => &mut bodies,
        };
        if let ShapeSource::Own(shape, local) = source {
            let key = format!("{shape:?}");
            let geometry = *own_shapes.entry(key).or_insert_with(|| {
                geometries.push(shape.clone());
                small_index(geometries.len() - 1)
            });
            target.push(Body {
                geometry,
                place: compose(place, local),
            });
            continue;
        }
        let shapes = rnr_path_of(source).and_then(|path| shapes_by_path.get(&path));
        let Some(shapes) = shapes else {
            if group.is_none() {
                match source {
                    ShapeSource::Mesh(_) | ShapeSource::MeshPath(_) => {
                        match mesh_path_of(source).and_then(|path| physics_meshes.get(&path)) {
                            Some(false) => report.meshes_without_physics += 1,
                            Some(true) => report.meshes_with_physics_without_rnr += 1,
                            None => report.missing_assets += 1,
                        }
                    }
                    _ => report.missing_assets += 1,
                }
            }
            continue;
        };
        target.extend(shapes.iter().map(|(local, geometry)| Body {
            geometry: *geometry,
            place: compose(place, local),
        }));
    }
    let deferred = deferred
        .iter()
        .zip(deferred_bodies)
        .filter(|(_, bodies)| !bodies.is_empty())
        .map(|((slice, origin), bodies)| DeferredSlice {
            slice: slice.0,
            sub_id: slice.1,
            origin: *origin,
            bodies,
        })
        .collect();
    Ok(LevelCollision {
        level: level.to_owned(),
        geometries,
        bodies,
        deferred,
    })
}

/// The `.rnr` path of the mesh at `mesh_path`: the same path with the physics asset's extension.
fn mesh_rnr_path(mesh_path: &str) -> String {
    let stem = mesh_path
        .rsplit_once('.')
        .map_or(mesh_path, |(stem, _)| stem);
    format!("{stem}{RNR_EXTENSION}")
}

/// For each mesh at `mesh_paths`: whether it carries physics geometry ([`has_physics_geometry`]);
/// a path that is no pak entry is left out.
fn meshes_with_physics(
    index: &PakIndex,
    mesh_paths: &[String],
) -> Result<HashMap<String, bool>, CollisionError> {
    let mut physics = HashMap::new();
    for (path, read) in index.read_each(mesh_paths, |path, bytes| {
        has_physics_geometry(&bytes).map_err(|source| CollisionError::Mesh {
            path: path.to_owned(),
            source,
        })
    }) {
        physics.insert(path, read??);
    }
    Ok(physics)
}

/// `value` as a geometry index (a level has far fewer than 4 billion shapes).
#[allow(clippy::cast_possible_truncation, reason = "see the doc comment")]
const fn small_index(value: usize) -> u32 {
    value as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(name: &str, type_id: [u8; 16], value: Vec<u8>, children: Vec<Element>) -> Element {
        Element {
            name_crc: Some(lowercase_crc32(name)),
            version: None,
            type_id,
            value,
            children,
        }
    }

    fn string(name: &str, text: &str) -> Element {
        element(name, [0; 16], text.as_bytes().to_vec(), Vec::new())
    }

    fn list(name: &str, items: Vec<Element>) -> Element {
        element(name, [0; 16], Vec::new(), items)
    }

    fn transform(columns: [[f32; 3]; 3], translation: [f32; 3]) -> Element {
        let value = columns
            .iter()
            .flatten()
            .chain(&translation)
            .flat_map(|value| value.to_be_bytes())
            .collect();
        element("m_worldTM", [0; 16], value, Vec::new())
    }

    fn asset(name: &str, guid: u8, sub: u32) -> Element {
        let mut value = vec![guid; GUID_LEN];
        value.extend_from_slice(&sub.to_be_bytes());
        value.extend_from_slice(&[0; 36]);
        element(name, [0; 16], value, Vec::new())
    }

    /// A slice: an entity 1 m up (in the slice's space) that has a rigid body of `filter` with a
    /// mesh, and a spawner entity 5 m north.
    fn slice(filter: &str) -> Element {
        let entity = |translation: [f32; 3], components: Vec<Element>| {
            let mut all = vec![element(
                "element",
                types::GAME_TRANSFORM,
                Vec::new(),
                vec![transform(
                    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    translation,
                )],
            )];
            all.extend(components);
            list("element", vec![list("Components", all)])
        };
        let body = element(
            "element",
            types::GAME_RIGID_BODY,
            Vec::new(),
            vec![list(
                "m_configuration",
                vec![
                    string("m_strFilter", filter),
                    asset("m_rnrAsset", 0, 0),
                    list("m_collisionShape", Vec::new()),
                ],
            )],
        );
        let mesh = element(
            "element",
            types::MESH,
            Vec::new(),
            vec![list(
                "Static Mesh Render Node",
                vec![asset("Static Mesh", 7, 0)],
            )],
        );
        let spawner = element(
            "element",
            types::PREFAB_SPAWNER,
            Vec::new(),
            vec![
                asset("m_sliceAsset", 9, 2),
                element("m_spawnOnInitialize", [0; 16], vec![1], Vec::new()),
            ],
        );
        let entities = vec![
            entity([0.0, 0.0, 1.0], vec![body, mesh]),
            entity([0.0, 5.0, 0.0], vec![spawner]),
        ];
        list(
            "root",
            vec![list(
                "Components",
                vec![list("element", vec![list("Entities", entities)])],
            )],
        )
    }

    #[test]
    fn a_solid_entity_is_placed_by_its_slice_matrix_with_its_mesh_s_physics() {
        let names = Names::new();
        let filters: HashSet<String> = ["Structure".to_owned()].into();
        let solids = slice_solids(&slice("Structure"), &names, &filters);
        assert_eq!(solids.bodies.len(), 1);
        let SolidEntity { place, source, .. } = &solids.bodies[0];
        assert_eq!(*source, Some(ShapeSource::Mesh(([7; GUID_LEN], 0))));
        assert_eq!([place[0][3], place[1][3], place[2][3]], [0.0, 0.0, 1.0]);
        assert_eq!(solids.spawns.len(), 1);
        assert_eq!(solids.spawns[0].slice, ([9; GUID_LEN], 2));
        assert_eq!(solids.spawns[0].place[1][3], 5.0);
        assert!(solids.spawns[0].on_initialize);
        let none = slice_solids(&slice("Empty"), &names, &filters);
        assert!(none.bodies.is_empty(), "a filter that does not block");
    }

    #[test]
    fn an_inactive_entity_has_no_solid_and_spawns_nothing_as_the_level_loads() {
        let names = Names::new();
        let filters: HashSet<String> = ["Structure".to_owned()].into();
        let mut root = slice("Structure");
        for entity in &mut root.children[0].children[0].children[0].children {
            entity
                .children
                .push(element("IsRuntimeActive", [0; 16], vec![0], Vec::new()));
        }
        let solids = slice_solids(&root, &names, &filters);
        assert!(solids.bodies.is_empty(), "{:?}", solids.bodies);
        assert_eq!(solids.inactive, 1);
        assert_eq!(solids.spawns.len(), 1);
        assert!(!solids.spawns[0].on_initialize, "spawns once it activates");
    }

    /// An element named by its name CRC.
    fn raw(name_crc: u32, value: Vec<u8>, children: Vec<Element>) -> Element {
        Element {
            name_crc: Some(name_crc),
            version: None,
            type_id: [0; 16],
            value,
            children,
        }
    }

    /// An `EntityId` field `name` of `id`.
    fn entity_id(name: &str, id: u64) -> Element {
        list(
            name,
            vec![element(
                "id",
                [0; 16],
                id.to_be_bytes().to_vec(),
                Vec::new(),
            )],
        )
    }

    /// A master gatherable: the root with its variation component (the start mesh and the
    /// depleted mesh change by the columns `StartMesh` and `DepletedMesh`), a `Healthy` group
    /// with the start mesh, and a `Depleted` group with the stump; both meshes are solid.
    fn gatherable() -> Element {
        let entity = |id: u64, name: &str, parent: u64, components: Vec<Element>| {
            let mut all = vec![element(
                "element",
                types::GAME_TRANSFORM,
                Vec::new(),
                vec![
                    transform(
                        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                        [0.0; 3],
                    ),
                    entity_id("m_parentId", parent),
                ],
            )];
            all.extend(components);
            list(
                "element",
                vec![
                    entity_id("Id", id),
                    string("name", name),
                    list("Components", all),
                ],
            )
        };
        let solid = |mesh: u8| {
            vec![
                element(
                    "element",
                    types::GAME_RIGID_BODY,
                    Vec::new(),
                    vec![list("m_configuration", vec![string("m_strFilter", "Tree")])],
                ),
                element(
                    "element",
                    types::MESH,
                    Vec::new(),
                    vec![list(
                        "Static Mesh Render Node",
                        vec![asset("Static Mesh", mesh, 0)],
                    )],
                ),
            ]
        };
        let binding = |id: u64, column: &str| {
            list(
                "element",
                vec![
                    list("m_entity", vec![entity_id("EntityId", id)]),
                    string("m_action", CHANGE_MESH),
                    raw(0xa77a_4396, column.as_bytes().to_vec(), Vec::new()),
                ],
            )
        };
        let variation = element(
            "element",
            types::VARIATION_DATA,
            Vec::new(),
            vec![
                raw(0x095a_1d04, b"Gatherable_Trees".to_vec(), Vec::new()),
                raw(
                    0xaedd_dde9,
                    Vec::new(),
                    vec![binding(3, "StartMesh"), binding(5, "DepletedMesh")],
                ),
            ],
        );
        const NO_PARENT: u64 = 0xffff_ffff;
        let entities = vec![
            entity(1, "Master_Tree", NO_PARENT, vec![variation]),
            entity(2, "Healthy", 1, Vec::new()),
            entity(3, "StartMesh", 2, solid(7)),
            entity(4, DEPLETED_GROUP, 1, Vec::new()),
            entity(5, "DepletedMesh", 4, solid(8)),
        ];
        list(
            "root",
            vec![list(
                "Components",
                vec![list("element", vec![list("Entities", entities)])],
            )],
        )
    }

    #[test]
    fn a_variant_shows_its_own_start_mesh_and_not_its_depleted_state() {
        let names = Names::new();
        let filters: HashSet<String> = ["Tree".to_owned()].into();
        let solids = slice_solids(&gatherable(), &names, &filters);
        let variation = solids.variation.clone().expect("variation component");
        assert_eq!(variation.table, "Gatherable_Trees");
        assert_eq!(variation.bindings.len(), 2);
        assert_eq!(solids.bodies.len(), 2);
        let start = &solids.bodies[0];
        let stump = &solids.bodies[1];
        assert!(!start.depleted && stump.depleted, "{:?}", solids.bodies);
        let row: HashMap<String, String> = [(
            "StartMesh".to_owned(),
            r"Objects\Trees\Oak\Oak_A.cgf".to_owned(),
        )]
        .into();
        let shown = varied(start, &variation, &row).expect("the start mesh is shown");
        assert_eq!(
            shown.source,
            Some(ShapeSource::MeshPath(
                "objects/trees/oak/oak_a.cgf".to_owned()
            ))
        );
        assert_eq!(varied(stump, &variation, &row), None, "depleted");
        let empty = HashMap::new();
        assert_eq!(
            varied(start, &variation, &empty).and_then(|body| body.source),
            Some(ShapeSource::Mesh(([7; GUID_LEN], 0))),
            "an empty column keeps the slice's mesh"
        );
    }

    #[test]
    fn a_static_collision_flag_shows_a_body_only_when_its_variant_sets_it() {
        let body = SolidEntity {
            place: IDENTITY_AFFINE,
            source: Some(ShapeSource::Rnr(([1; GUID_LEN], 100))),
            entity: Some(9),
            depleted: false,
        };
        let flag = |action: &str, column: &str| VariationBinding {
            entity: 9,
            action: action.to_owned(),
            column: column.to_owned(),
        };
        let variation = Variation {
            table: "Gatherable_Quest_Damageable".to_owned(),
            bindings: vec![
                flag(SET_STATIC_COLLISION, "ShouldHaveStaticCollision"),
                flag(SET_COLLISION_ASSET, "CollisionAssetOverride"),
                flag("Change Transform Scale Multiplier Z", "Collision_Scale_Z"),
                flag("Change Transform Translation Z", "Collision_Offset_Z"),
            ],
        };
        let row = |set: &str| -> HashMap<String, String> {
            [
                ("ShouldHaveStaticCollision".to_owned(), set.to_owned()),
                (
                    "CollisionAssetOverride".to_owned(),
                    r"Objects\Base\Sphere.cgf".to_owned(),
                ),
                ("Collision_Scale_Z".to_owned(), "2".to_owned()),
                ("Collision_Offset_Z".to_owned(), "1.5".to_owned()),
            ]
            .into()
        };
        assert_eq!(varied(&body, &variation, &row("")), None);
        assert_eq!(varied(&body, &variation, &row("0")), None);
        let shown = varied(&body, &variation, &row("1")).expect("static collision");
        assert_eq!(
            shown.source,
            Some(ShapeSource::MeshPath("objects/base/sphere.cgf".to_owned()))
        );
        assert_eq!(shown.place[2][2], 2.0, "scaled up");
        assert_eq!(shown.place[2][AXES], 1.5, "raised");
    }

    #[test]
    fn a_mesh_s_physics_asset_is_at_its_path() {
        assert_eq!(
            mesh_rnr_path("objects/ships/galleon_body.cgf"),
            "objects/ships/galleon_body.rnr"
        );
        assert_eq!(
            mesh_rnr_path("slices/cart/handcart_settlement__28236438930.cgf"),
            "slices/cart/handcart_settlement__28236438930.rnr"
        );
    }

    /// A query shape element of `type_id` with float and vector fields.
    fn query(type_id: [u8; 16], floats: &[(&str, f32)], vectors: &[(&str, [f32; 3])]) -> Element {
        let mut children: Vec<Element> = floats
            .iter()
            .map(|(name, value)| element(name, [0; 16], value.to_be_bytes().to_vec(), Vec::new()))
            .collect();
        children.extend(vectors.iter().map(|(name, values)| {
            let value = values.iter().flat_map(|v| v.to_be_bytes()).collect();
            element(name, [0; 16], value, Vec::new())
        }));
        element("element", type_id, Vec::new(), children)
    }

    /// The geometry and the translation of an own shape.
    fn own(source: Option<ShapeSource>) -> (Geometry, [f32; 3]) {
        let Some(ShapeSource::Own(geometry, place)) = source else {
            panic!("{source:?}");
        };
        (geometry, [place[0][3], place[1][3], place[2][3]])
    }

    #[test]
    fn a_rigid_body_s_own_query_shapes_are_read() {
        let names = Names::new();
        let up = [0.0, 0.0, 1.0];
        let shape = |element: Element| own(query_shape(&element, up, &names));
        assert_eq!(
            shape(query(
                types::QUERY_SHAPE_BOX,
                &[],
                &[("m_box", [0.25, 5.0, 3.0])]
            )),
            (
                Geometry::Box {
                    half_extents: [0.25, 5.0, 3.0]
                },
                up
            )
        );
        assert_eq!(
            shape(query(
                types::QUERY_SHAPE_AABB,
                &[],
                &[("m_aabb", [4.7, 5.8, 6.0])]
            ))
            .0,
            Geometry::Box {
                half_extents: [4.7, 5.8, 6.0]
            }
        );
        assert_eq!(
            shape(query(types::QUERY_SHAPE_SPHERE, &[("m_radius", 1.5)], &[])).0,
            Geometry::Sphere { radius: 1.5 }
        );
        assert_eq!(
            shape(query(
                types::QUERY_SHAPE_CYLINDER,
                &[("m_height", 6.5), ("m_radius", 0.15)],
                &[]
            ))
            .0,
            Geometry::Cylinder {
                from: [0.0; 3],
                to: [0.0, 0.0, 6.5],
                radius: 0.15
            }
        );
        // A capsule with no axis stands up on the origin; one along x lies on it.
        let segment = |element: Element| match shape(element).0 {
            Geometry::Capsule { from, to, radius } => {
                [from, to, [radius; 3]].map(|v| v.map(|value| (value * 100.0).round() / 100.0))
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(
            segment(query(
                types::QUERY_SHAPE_CAPSULE,
                &[("m_height", 6.0), ("m_radius", 1.7)],
                &[("m_axis", [0.0; 3])]
            )),
            [[0.0, 0.0, 1.7], [0.0, 0.0, 7.7], [1.7; 3]]
        );
        assert_eq!(
            segment(query(
                types::QUERY_SHAPE_CAPSULE,
                &[("m_height", 2.0), ("m_radius", 0.5)],
                &[("m_axis", [2.0, 0.0, 0.0])]
            )),
            [[-1.0, 0.0, 0.5], [1.0, 0.0, 0.5], [0.5; 3]]
        );
        let other = element("element", [9; 16], Vec::new(), Vec::new());
        assert_eq!(query_shape(&other, up, &names), None);
    }

    #[test]
    fn a_stored_transform_is_read_by_columns() {
        // A quarter turn about z: x goes to y.
        let place = transform_of(&transform(
            [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [1.0, 2.0, 3.0],
        ))
        .expect("transform");
        assert_eq!(place[1][0], 1.0, "x's image has a y part");
        assert_eq!(place[0][1], -1.0);
        assert_eq!([place[0][3], place[1][3], place[2][3]], [1.0, 2.0, 3.0]);
    }

    #[test]
    fn the_player_is_blocked_by_filters_in_the_categories_it_collides_with() {
        let filter = |name: &str, bases: &[&str], categories: &[&str], collides: &[&str]| {
            let strings = |crc: u32, items: &[&str]| Element {
                name_crc: Some(crc),
                children: items.iter().map(|item| string("element", item)).collect(),
                ..Element::default()
            };
            list(
                "element",
                vec![
                    string("name", name),
                    strings(0xa1ef_e138, bases),
                    strings(0x7097_8a1a, categories),
                    strings(0xe6a3_571c, collides),
                ],
            )
        };
        let root = Element {
            children: vec![Element {
                name_crc: Some(0x7877_678d),
                children: vec![
                    filter("Character", &[], &["Character"], &["Static", "Terrain"]),
                    filter("Player", &["Character"], &["Player"], &["BossShield"]),
                    filter("Static", &[], &["Static"], &[]),
                    filter("Structure", &["Static"], &["Structure"], &[]),
                    filter("Empty", &[], &[], &[]),
                    filter("Water", &[], &["Water"], &[]),
                ],
                ..Element::default()
            }],
            ..Element::default()
        };
        let blocking = blocking_filters(&root, "Player");
        let mut names: Vec<&str> = blocking.iter().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["Static", "Structure"]);
    }

    #[test]
    fn a_capital_asset_text_is_a_guid_and_a_hex_sub_id() {
        let key = capital_asset("{3D51B368-7E53-5FF9-AF51-58FD8602F34B}:709924").expect("asset id");
        assert_eq!(key.0[0], 0x3d);
        assert_eq!(key.0[15], 0x4b);
        assert_eq!(key.1, 0x0070_9924);
        assert_eq!(
            capital_asset("{00000000-0000-0000-0000-000000000000}:0"),
            None
        );
    }

    #[test]
    fn compound_shapes_flatten_with_their_places() {
        let child = Shape {
            kind: ShapeKind::Sphere { radius: 1.0 },
            pose: None,
        };
        let mut moved = IDENTITY_AFFINE;
        moved[0][3] = 4.0;
        let compound = Shape {
            kind: ShapeKind::Compound(vec![
                (moved, child.clone()),
                (
                    IDENTITY_AFFINE,
                    Shape {
                        kind: ShapeKind::Unsupported { kind: 4 },
                        pose: None,
                    },
                ),
            ]),
            pose: Some({
                let mut up = IDENTITY_AFFINE;
                up[2][3] = 1.0;
                up
            }),
        };
        let mut out = Vec::new();
        assert_eq!(flatten_shapes(&compound, &IDENTITY_AFFINE, &mut out), 1);
        assert_eq!(out.len(), 1);
        assert_eq!([out[0].0[0][3], out[0].0[2][3]], [4.0, 1.0]);
    }
}
