// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool level-doors`: the level doors file ([`nw_content::level_doors`], level doors
//! finding 2026-11-13) of one level, from its world file and the game paks.
//!
//! Each capital's slice is found by the capital's `sliceAssetId` in the paks' asset catalog (the
//! world builder's way) and read entity by entity. A slice is a door slice when one of its
//! components has the door type ([`DOOR_TYPE`]) and a replica index above 0. For each capital of
//! a door slice the file keeps its id, place and slice, and every component of the slice of a
//! kind of [`DoorComponentKind`] with its replica index (`m_replicationIndex`, the component
//! number on the wire; Observed: the door, slayer script, timeline, delayed event, obstruction
//! and gatherable members of the 21 captured door capitals sit on exactly these numbers).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use nw_content::level_doors::{
    DoorComponent, DoorComponentKind, LevelDoor, LevelDoors, LevelDoorsError, level_doors_path,
};
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, AssetCatalogError, AssetKey, paths_of};
use nw_formats::objectstream::{Element, lowercase_crc32};
use nw_formats::pak::PakError;
use nw_world::capitals::capital_id;
use nw_world::level_script::{
    DELAYED_EVENT_TYPE, GATHERABLE_CONTROLLER_TYPE, PLACEMENT_OBSTRUCTION_TYPE, REPLICA_INDEX_NAME,
    TIMELINE_TYPE,
};
use nw_world::records::WorldFile;

use crate::collision::{PakIndex, capital_asset};
use crate::world::{WorldBuildError, stream_of};

/// `{0FF532C2-E078-4F5F-A559-3473A4AEB5B5}`: the door component (Observed: the entity
/// `DoorFunctionality` of every captured door slice; its replica index is the door member's
/// component: 11 on `Gimmick_Door_Varlaam_A`, 5 on the 17DW gate, 8 on
/// `CustomMQ_0517_AncientTowerDoor_01`).
pub const DOOR_TYPE: [u8; 16] = [
    0x0f, 0xf5, 0x32, 0xc2, 0xe0, 0x78, 0x4f, 0x5f, 0xa5, 0x59, 0x34, 0x73, 0xa4, 0xae, 0xb5, 0xb5,
];
/// `{3D939617-3F24-41DC-A05F-2F298E8F4CB8}`: the slayer script component (Observed: replica 2 of
/// the 17DW gate slices, the component of retail's `SlayerScriptReplicatedState` member).
pub const SLAYER_SCRIPT_TYPE: [u8; 16] = [
    0x3d, 0x93, 0x96, 0x17, 0x3f, 0x24, 0x41, 0xdc, 0xa0, 0x5f, 0x2f, 0x29, 0x8e, 0x8f, 0x4c, 0xb8,
];
/// How many base classes deep a component's replica index may sit (the level script reader's
/// field depth).
const MAX_BASE_CLASSES: usize = 3;
/// What made the file.
const SOURCE: &str = "content-tool level-doors (world file and game paks)";

/// A failed build.
#[derive(Debug, thiserror::Error)]
pub enum LevelDoorsBuildError {
    /// The asset catalog does not read.
    #[error("asset catalog: {0}")]
    Catalog(#[from] AssetCatalogError),
    /// The paks have no asset catalog.
    #[error("the paks have no {ASSET_CATALOG_ENTRY}")]
    NoCatalog,
    /// A pak entry cannot be read.
    #[error("{path}: {source}")]
    Pak {
        /// The entry.
        path: String,
        /// Why.
        source: PakError,
    },
    /// A slice does not parse.
    #[error(transparent)]
    Slice(#[from] WorldBuildError),
    /// The file cannot be written or read back.
    #[error(transparent)]
    File(#[from] LevelDoorsError),
    /// The written file reads back other than it was written.
    #[error("{0} reads back other than written")]
    Reload(PathBuf),
}

/// What a build did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LevelDoorsReport {
    /// The file written.
    pub path: PathBuf,
    /// The capitals of the level.
    pub capitals: usize,
    /// The capitals whose slice asset the catalog has not.
    pub unresolved: usize,
    /// The distinct capital slices read.
    pub slices: usize,
    /// The slices with a door.
    pub door_slices: usize,
    /// The door capitals written.
    pub doors: usize,
}

/// The kind of a component of type `type_id`, when it is one the file keeps.
fn kind_of(type_id: &[u8; 16]) -> Option<DoorComponentKind> {
    match *type_id {
        DOOR_TYPE => Some(DoorComponentKind::Door),
        SLAYER_SCRIPT_TYPE => Some(DoorComponentKind::SlayerScript),
        TIMELINE_TYPE => Some(DoorComponentKind::Timeline),
        DELAYED_EVENT_TYPE => Some(DoorComponentKind::DelayedEvent),
        PLACEMENT_OBSTRUCTION_TYPE => Some(DoorComponentKind::PlacementObstruction),
        GATHERABLE_CONTROLLER_TYPE => Some(DoorComponentKind::Gatherable),
        _ => None,
    }
}

/// The replica index of `component`: `m_replicationIndex` on it or on its base classes.
fn replica_index(component: &Element) -> Option<u64> {
    let base_class = lowercase_crc32("BaseClass1");
    let mut level = component;
    for _ in 0..MAX_BASE_CLASSES {
        if let Some(index) = level.child(REPLICA_INDEX_NAME) {
            return index.as_u32().ok().map(u64::from);
        }
        level = level.child(base_class)?;
    }
    None
}

/// The kept replicated components of the slice `root` (the slice's entities' components with a
/// replica index above 0), in component order; empty when none is a door.
pub fn door_components(root: &Element) -> Vec<DoorComponent> {
    let components = lowercase_crc32("Components");
    let entities = lowercase_crc32("Entities");
    let Some(list) = root.child(components).and_then(|list| {
        list.children
            .iter()
            .find_map(|component| component.child(entities))
    }) else {
        return Vec::new();
    };
    let mut kept: Vec<DoorComponent> = list
        .children
        .iter()
        .filter_map(|entity| entity.child(components))
        .flat_map(|entity_components| &entity_components.children)
        .filter_map(|component| {
            let kind = kind_of(&component.type_id)?;
            let index = replica_index(component).filter(|index| *index > 0)?;
            Some(DoorComponent {
                component: index,
                kind,
            })
        })
        .collect();
    if !kept
        .iter()
        .any(|component| component.kind == DoorComponentKind::Door)
    {
        return Vec::new();
    }
    kept.sort_by_key(|component| component.component);
    kept.dedup();
    kept
}

/// The door capitals of `world` whose slice (by `slices`, the slice path of each capital asset)
/// has door components (`components`, by slice path).
pub fn level_doors(
    world: &WorldFile,
    slices: &HashMap<AssetKey, String>,
    components: &HashMap<String, Vec<DoorComponent>>,
) -> Vec<LevelDoor> {
    world
        .capitals
        .iter()
        .filter_map(|capital| {
            let slice = slices.get(&capital_asset(&capital.slice_asset_id)?)?;
            let kept = components.get(slice).filter(|kept| !kept.is_empty())?;
            Some(LevelDoor {
                capital: capital_id(&capital.id)?,
                position: capital.position,
                slice: slice.clone(),
                components: kept.clone(),
            })
        })
        .collect()
}

/// Builds the level doors file of level `level` (world file `world`) from the paks of `index`
/// into `out_dir`.
///
/// # Errors
///
/// Returns [`LevelDoorsBuildError`] when the catalog or a slice cannot be read or parsed, or the
/// file cannot be written or reads back other.
pub fn build_level_doors(
    index: &PakIndex,
    world: &WorldFile,
    level: &str,
    client_build: &str,
    out_dir: &Path,
) -> Result<LevelDoorsReport, LevelDoorsBuildError> {
    let wanted: HashSet<AssetKey> = world
        .capitals
        .iter()
        .filter_map(|capital| capital_asset(&capital.slice_asset_id))
        .collect();
    let (catalog_path, catalog) = index
        .read_each(&[ASSET_CATALOG_ENTRY.to_owned()], |_, bytes| bytes)
        .into_iter()
        .next()
        .ok_or(LevelDoorsBuildError::NoCatalog)?;
    let catalog = catalog.map_err(|source| LevelDoorsBuildError::Pak {
        path: catalog_path,
        source,
    })?;
    let slices = paths_of(&catalog, &wanted)?;
    let mut paths: Vec<String> = slices
        .values()
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    paths.sort();
    let mut components = HashMap::new();
    for (path, read) in index.read_each(&paths, |path, bytes| {
        stream_of(path, &bytes).map(|root| door_components(&root))
    }) {
        let kept = read.map_err(|source| LevelDoorsBuildError::Pak {
            path: path.clone(),
            source,
        })??;
        components.insert(path, kept);
    }
    let doors = LevelDoors {
        source: SOURCE.to_owned(),
        level: level.to_owned(),
        doors: level_doors(world, &slices, &components),
    };
    let path = level_doors_path(out_dir, client_build, level);
    doors.write(&path)?;
    if LevelDoors::load(out_dir, client_build, level)? != doors {
        return Err(LevelDoorsBuildError::Reload(path));
    }
    Ok(LevelDoorsReport {
        path,
        capitals: world.capitals.len(),
        unresolved: world
            .capitals
            .iter()
            .filter(|capital| {
                capital_asset(&capital.slice_asset_id).is_none_or(|key| !slices.contains_key(&key))
            })
            .count(),
        slices: components.len(),
        door_slices: components.values().filter(|kept| !kept.is_empty()).count(),
        doors: doors.len(),
    })
}

#[cfg(test)]
mod tests {
    use nw_world::records::Capital;

    use super::*;

    /// The replica index field in a component's base class.
    fn indexed(type_id: [u8; 16], index: u32) -> Element {
        let mut component = Element::with_children(
            lowercase_crc32("element"),
            vec![Element::with_children(
                lowercase_crc32("BaseClass1"),
                vec![Element::with_value(
                    REPLICA_INDEX_NAME,
                    index.to_be_bytes().to_vec(),
                )],
            )],
        );
        component.type_id = type_id;
        component
    }

    /// A slice of one entity per component list of `entities`.
    fn slice(entities: Vec<Vec<Element>>) -> Element {
        let entities = entities
            .into_iter()
            .map(|components| {
                Element::with_children(
                    lowercase_crc32("element"),
                    vec![Element::with_children(
                        lowercase_crc32("Components"),
                        components,
                    )],
                )
            })
            .collect();
        Element::with_children(
            0,
            vec![Element::with_children(
                lowercase_crc32("Components"),
                vec![Element::with_children(
                    lowercase_crc32("element"),
                    vec![Element::with_children(
                        lowercase_crc32("Entities"),
                        entities,
                    )],
                )],
            )],
        )
    }

    /// A made-up type of a component the file does not keep (a map marker's place).
    const MARKER_TYPE: [u8; 16] = [7; 16];

    /// The 17DW gate's shape (Observed: slayer script 2, door 5; position 1 and marker 3 are not
    /// kept).
    fn gate_slice() -> Element {
        slice(vec![
            vec![
                indexed(nw_world::level_script::NETWORK_POSITION_TYPE, 1),
                indexed(SLAYER_SCRIPT_TYPE, 2),
            ],
            vec![indexed(MARKER_TYPE, 3)],
            vec![indexed(DOOR_TYPE, 5), indexed(TIMELINE_TYPE, 0)],
        ])
    }

    #[test]
    fn a_door_slice_keeps_its_door_and_slayer_script_on_their_replica_indexes() {
        assert_eq!(
            door_components(&gate_slice()),
            [
                DoorComponent {
                    component: 2,
                    kind: DoorComponentKind::SlayerScript,
                },
                DoorComponent {
                    component: 5,
                    kind: DoorComponentKind::Door,
                },
            ]
        );
    }

    #[test]
    fn a_slice_without_a_door_keeps_nothing() {
        let chest = slice(vec![vec![
            indexed(GATHERABLE_CONTROLLER_TYPE, 2),
            indexed(TIMELINE_TYPE, 3),
        ]]);
        assert!(door_components(&chest).is_empty());
    }

    #[test]
    fn a_door_capital_takes_its_id_place_and_slice() {
        const GUID: &str = "1d477ea9-03b5-c757-054c-0998c9ac76fc";
        const ASSET: &str = "{94644FC5-7A62-55A4-87C5-10BFA0DDEA23}:1f21915a";
        const OTHER_ASSET: &str = "{94644FC5-7A62-55A4-87C5-10BFA0DDEA23}:2";
        const PATH: &str =
            "coatgen/f46c8fa3/master_simpledoor_teleport_a_17dw_msq_gate_3362621797.dynamicslice";
        let capital = |id: &str, asset: &str| Capital {
            id: id.to_owned(),
            position: [3402.029, 5808.956, 240.55014],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: None,
            slice_name: String::new(),
            variant_name: None,
            slice_asset_id: asset.to_owned(),
        };
        let world = WorldFile {
            capitals: vec![
                capital(GUID, ASSET),
                capital("ff6c1447-df9d-20ba-4b5c-d57461b2c425", OTHER_ASSET),
            ],
            ..WorldFile::default()
        };
        let slices = HashMap::from([
            (capital_asset(ASSET).expect("asset"), PATH.to_owned()),
            (
                capital_asset(OTHER_ASSET).expect("asset"),
                "slices/other.dynamicslice".to_owned(),
            ),
        ]);
        let components = HashMap::from([
            (PATH.to_owned(), door_components(&gate_slice())),
            ("slices/other.dynamicslice".to_owned(), Vec::new()),
        ]);
        let doors = level_doors(&world, &slices, &components);
        assert_eq!(doors.len(), 1);
        // Observed: the card `GdeRef` of this capital starts `abb119ca31770b18`.
        assert_eq!(
            doors[0].capital.to_le_bytes(),
            [0xab, 0xb1, 0x19, 0xca, 0x31, 0x77, 0x0b, 0x18]
        );
        assert_eq!(doors[0].slice, PATH);
        assert_eq!(doors[0].components.len(), 2);
    }
}
