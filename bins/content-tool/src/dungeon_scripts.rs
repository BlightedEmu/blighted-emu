// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool dungeon-scripts`: the dungeon script file ([`nw_world::dungeon_scripts`],
//! dungeon2 study, run 15) from the game paks and the content bundle.
//!
//! For each instance game mode (`InstanceCatalog`) with a `SlicePath`, its level script slice
//! (`slices/<SlicePath>.dynamicslice`) is placed where a capital of its level names it (the
//! level's `dungeon_script` layer: `slices/dungeon/levelscripts/amrinedungeon` at 780, 674, 134
//! in `nw_dungeon_windsward_00`). Each spawner of the level script that spawns an encounter slice
//! (a slice with an `EncounterManagerComponent`) is an encounter; each that spawns a loot
//! container (a slice with a loot table) is a chest. Every spawn and tracked target of an
//! encounter is resolved to its slices (an alias asset to its entries) and each slice to the mob
//! its spawner slice names (`AIVariantProviderComponent`), whose level 0 is the level of the
//! vitals row (`VitalsLevelVariantData`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use nw_content::Tables;
use nw_content::instances::InstanceCatalog;
use nw_content::mob_prefabs::{ASSET_ID_LEN, MobPrefabs};
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, AssetCatalogError, slice_assets_in};
use nw_formats::azcs::AzcsError;
use nw_formats::objectstream::{Element, ObjectStreamError, parse_object_stream};
use nw_formats::pak::PakError;
use nw_world::capitals::parse_capitals;
use nw_world::collision::Affine;
use nw_world::dungeon_scripts::{
    Chest, DungeonScripts, DungeonScriptsError, Encounter, LevelScript, OPEN_WORLD_LEVEL,
    ObjectiveSpec, Spawn, Stage, StageAction, StageObjective, Target, TargetMob, TargetSlice,
    dungeon_scripts_path,
};
use nw_world::level_script::{
    Action, AliasSlice, AssetRef, DYNAMIC_SLICE_SUFFIX, LevelScriptError, ObjectiveKind,
    ScriptEntity, SliceScript, Transform, VitalsVariant, alias_slices, pak_path, slice_script,
};
use nw_world::metadata::{MetadataRecords, SkipCounts, read_metadata};
use nw_world::records::{Capital, PatrolPath};
use nw_world::spawner_slices::SpawnerSlice;

use crate::breach_tiers::{BreachTier, load_breach_tiers};
use crate::collision::{PakIndex, detector_volume};
use crate::roads::without_azcs;

/// The pak folder of the catacombs gameplay tiles.
const CATACOMB_TILE_FOLDER: &str = "slices/catacombs/gameplay/tiles/";
/// The folder of the levels' data in the paks.
const COATLICUE_DIR: &str = "sharedassets/coatlicue";
/// The catalog folder that holds every slice (the empty prefix).
const ANY_FOLDER: &str = "";
/// The suffix of a capitals file.
const CAPITALS_SUFFIX: &str = ".capitals.json";
/// The folder of a slice path of the game mode table.
const SLICES_DIR: &str = "slices";
/// The suffix of an alias asset.
const ALIAS_SUFFIX: &str = ".aliasasset";
/// The marks of a slice's difficulty at the end of its file name (`corruption_swarmer_dt3`,
/// `darkness_hazard_corruption_minor_l2`; Observed in the corrupted breaches' aliases).
const DIFFICULTY_MARKS: [&str; 2] = ["dt", "l"];
/// The mark of a vitals row's difficulty at the end of its id (`Corruption_Swarmer_DT3`).
const ROW_DIFFICULTY_MARK: &str = "_DT";
/// The difficulties of a corrupted breach (Observed: the reward lists name `DT1` to `DT6`).
const FIRST_DIFFICULTY: u32 = 1;
const LAST_DIFFICULTY: u32 = 6;
/// Metres within which a retail map point is a breach place's (the map points of one place lay
/// within a few metres of its encounter root in the breach ledgers; our margin).
const TIER_MATCH_M: f32 = 15.0;
/// The scale of a capital without one.
const UNIT_SCALE: f32 = 1.0;
/// The suffix of a metadata file.
const METADATA_SUFFIX: &str = ".metadata";
/// The folder of creature slices; spell and targeting slices under it are no creatures.
const CREATURE_DIR: &str = "slices/characters/";
const NOT_CREATURE_DIRS: [&str; 2] = ["/spellslices/", "/targeting/"];
/// Metres within which a level's encounter spawn point (`IsEncounter` in its metadata) is an
/// encounter's: the export writes it at the encounter's place (Observed: the 25 points of
/// `nw_dungeon_windsward_00` lie at 0.0 m from its 5 encounters; best guess for the margin).
const ENCOUNTER_POINT_MATCH_M: f32 = 1.0;

/// Failure while building the dungeon script file or the gates file.
#[derive(Debug, thiserror::Error)]
pub enum DungeonScriptsBuildError {
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
    /// A slice is not a readable ObjectStream.
    #[error("{path}: {source}")]
    ObjectStream {
        /// The entry.
        path: String,
        /// Why.
        source: ObjectStreamError,
    },
    /// A slice's script cannot be read.
    #[error("{path}: {source}")]
    Script {
        /// The entry.
        path: String,
        /// Why.
        source: LevelScriptError,
    },
    /// The asset catalog cannot be read.
    #[error(transparent)]
    Catalog(#[from] AssetCatalogError),
    /// A metadata file cannot be read.
    #[error("{path}: {source}")]
    Metadata {
        /// The entry.
        path: String,
        /// Why.
        source: nw_world::metadata::MetadataError,
    },
    /// A capitals file is not JSON.
    #[error("{path}: {source}")]
    Capitals {
        /// The entry.
        path: String,
        /// Why.
        source: serde_json::Error,
    },
    /// The file cannot be written or read back.
    #[error(transparent)]
    File(#[from] DungeonScriptsError),
    /// The gates file cannot be written or read back.
    #[error(transparent)]
    Gates(#[from] nw_content::dungeon_gates::DungeonGatesError),
    /// The breach tiers file cannot be read.
    #[error(transparent)]
    Tiers(#[from] crate::breach_tiers::BreachTiersError),
    /// The file read back is not the one written.
    #[error("{0}: the file read back differs from the one written")]
    Reload(PathBuf),
}

/// The slices a build reads: by pak path, the ObjectStream root (`None` for a path that is no
/// pak entry).
pub trait SliceSource {
    /// The root of the slice or alias asset at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonScriptsBuildError`] when the entry cannot be read or decoded.
    fn root(&mut self, path: &str) -> Result<Option<Element>, DungeonScriptsBuildError>;

    /// The catalog path of each dynamic slice GUID, when the source has an asset catalog.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonScriptsBuildError`] when the catalog cannot be read.
    fn slice_catalog(
        &mut self,
    ) -> Result<Option<&HashMap<[u8; ASSET_ID_LEN], String>>, DungeonScriptsBuildError> {
        Ok(None)
    }

    /// The pak path of the slice `asset` names: its stored path when that is an entry (or
    /// empty), else the catalog's path of its GUID, else the stored path.
    ///
    /// Observed: a spawner's stored path can be out of date while its GUID is right
    /// (`WorldEvent_SpawnGroup_Single_Master_448598026` names
    /// `slices/pois/ancient/design/enc_worldevent_holdout_setup.dynamicslice`, whose entry is in
    /// the `holdout` folder below it; retail spawned the GUID's asset `a8cf2442...` in
    /// `99a_msq_01b_learnsoulwardens` 180.442 s).
    ///
    /// # Errors
    ///
    /// Returns [`DungeonScriptsBuildError`] when the entry or the catalog cannot be read.
    fn asset_path(&mut self, asset: &AssetRef) -> Result<String, DungeonScriptsBuildError> {
        if asset.path.is_empty() || self.root(&asset.path)?.is_some() {
            return Ok(asset.path.clone());
        }
        Ok(self
            .slice_catalog()?
            .and_then(|catalog| catalog.get(&asset.guid))
            .cloned()
            .unwrap_or_else(|| asset.path.clone()))
    }
}

/// The pak entries, read once each.
pub struct PakSlices<'a> {
    index: &'a PakIndex,
    read: HashMap<String, Option<Element>>,
    /// The catalog path of each dynamic slice GUID, read at the first stored path that is no
    /// entry.
    catalog: Option<HashMap<[u8; ASSET_ID_LEN], String>>,
}

impl<'a> PakSlices<'a> {
    /// The slices of `index`.
    pub fn new(index: &'a PakIndex) -> Self {
        Self {
            index,
            read: HashMap::new(),
            catalog: None,
        }
    }

    /// The catalog path of each dynamic slice GUID (sub id 2) of the paks' asset catalog.
    fn catalog(
        &mut self,
    ) -> Result<&HashMap<[u8; ASSET_ID_LEN], String>, DungeonScriptsBuildError> {
        if self.catalog.is_none() {
            let mut by_guid = HashMap::new();
            for (_, bytes) in self
                .index
                .read_each(&[ASSET_CATALOG_ENTRY.to_owned()], |_, bytes| bytes)
            {
                for (path, (guid, _)) in slice_assets_in(&bytes?, ANY_FOLDER)? {
                    by_guid.insert(guid, path);
                }
            }
            self.catalog = Some(by_guid);
        }
        Ok(self.catalog.get_or_insert_default())
    }
}

impl SliceSource for PakSlices<'_> {
    fn root(&mut self, path: &str) -> Result<Option<Element>, DungeonScriptsBuildError> {
        if let Some(known) = self.read.get(path) {
            return Ok(known.clone());
        }
        let mut root = None;
        for (entry, bytes) in self.index.read_each(&[path.to_owned()], |_, bytes| bytes) {
            let bytes = without_azcs(bytes?).map_err(|source| DungeonScriptsBuildError::Azcs {
                path: entry.clone(),
                source,
            })?;
            root = Some(parse_object_stream(&bytes).map_err(|source| {
                DungeonScriptsBuildError::ObjectStream {
                    path: entry.clone(),
                    source,
                }
            })?);
        }
        self.read.insert(path.to_owned(), root.clone());
        Ok(root)
    }

    fn slice_catalog(
        &mut self,
    ) -> Result<Option<&HashMap<[u8; ASSET_ID_LEN], String>>, DungeonScriptsBuildError> {
        self.catalog().map(Some)
    }
}

/// What a build found and left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DungeonScriptsReport {
    /// Level scripts written (one per script slice; game modes may share one).
    pub levels: usize,
    /// Encounters written.
    pub encounters: usize,
    /// Stages written.
    pub stages: usize,
    /// Spawns written.
    pub spawns: usize,
    /// Chests written.
    pub chests: usize,
    /// Encounters of the open world's spawn groups, of [`Self::encounters`].
    pub open_world_encounters: usize,
    /// Encounters whose spawner entity has spawn rules (an area or a point spawner).
    pub encounters_with_spawner_rules: usize,
    /// Encounters of an alias spawner (one per alias slice and spawn point), of
    /// [`Self::encounters`].
    pub grouped_encounters: usize,
    /// Mobs of grouped encounters' difficulties that come from a sibling difficulty's row.
    pub sibling_mobs: usize,
    /// Grouped encounters whose difficulty a retail map point at their place gives.
    pub observed_difficulties: usize,
    /// Grouped encounters whose difficulty is their group's or the nearest group's.
    pub inferred_difficulties: usize,
    /// Encounters with a detection volume of a shape this reader places.
    pub detectors: usize,
    /// Encounters whose detection volume's shape is of a kind not read (they keep its radius).
    pub unread_detectors: usize,
    /// Target slices with a mob from their spawner slice.
    pub spawner_mobs: usize,
    /// Target slices without one (creature slices: the sim's prefab table; props).
    pub other_slices: usize,
    /// Spawner mobs whose level is in neither the spawner nor the vitals tables.
    pub levelless_mobs: usize,
    /// Game modes left out: no slice path, no capital of it, or no slice in the paks.
    pub skipped_modes: Vec<String>,
    /// Slice paths named but not in the paks.
    pub missing_slices: Vec<String>,
    /// The file written.
    pub path: PathBuf,
}

/// How deep a spawned slice is followed to its mob: a spawn of an encounter may be a spawner
/// slice that spawns a nested encounter slice whose stage spawns the mob's spawner slice (the
/// storm priest's `..._spawn_minionspawn`, then `..._minionspawn`, then `worldevent_ai_..._minion`).
const NESTED_DEPTH: u32 = 4;
/// The mobs of one spawn of a mob's own spawner slice.
const ONE_MOB: u32 = 1;
/// The rotation quaternion (x, y, z, w) of no rotation.
const NO_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// The separator of a pak path, and the doubled one of a generated capital's slice name.
const PATH_SEPARATOR: &str = "/";
const DOUBLED_SEPARATOR: &str = "//";
/// The file names of the corrupted breaches' spawn groups start with this (lower case:
/// `Darkness_SpawnGroup_Minor_Master`, `_Major_Master` and their `_Special_` kinds).
const BREACH_GROUP_PREFIX: &str = "darkness_spawngroup_";
/// The file names of the open world's spawn group slices start with one of these (lower case).
const SPAWN_GROUP_PREFIXES: [&str; 3] = ["worldevent_", "worldboss_", BREACH_GROUP_PREFIX];

/// The wanted script path that capital slice name `slice_name` places, if any. A story
/// instance's capital names its script without the slices folder
/// (`Dungeon/MedeaWindsward/MedeaScript` in the open world's `06sw_medea_script` capitals), so
/// the name is tried as it is and under that folder.
fn placed_script(slice_name: &str, wanted: impl Fn(&str) -> bool) -> Option<String> {
    let bare = capital_script_path(slice_name);
    let in_slices = format!("{SLICES_DIR}/{bare}");
    [bare, in_slices].into_iter().find(|script| wanted(script))
}

/// The pak path of the slice a capital names: a level script capital names it without its
/// extension (`Dungeon/LevelScripts/AmrineDungeon`), a generated one with it and with a doubled
/// separator (`CoatGen\\\\efc337e6\\WorldEvent_SpawnGroup_Single_Master_3815808514.dynamicslice`).
fn capital_script_path(slice_name: &str) -> String {
    let path = pak_path(slice_name).replace(DOUBLED_SEPARATOR, PATH_SEPARATOR);
    if path.ends_with(DYNAMIC_SLICE_SUFFIX) {
        path
    } else {
        format!("{path}{DYNAMIC_SLICE_SUFFIX}")
    }
}

/// Whether the slice at `path` is a corrupted breach spawn group: the only spawn groups whose
/// alias spawner's encounters the file holds (the alias spawners of other levels and world
/// events are left out: no retail capture of theirs was studied).
fn is_breach_group(path: &str) -> bool {
    file_name(path).starts_with(BREACH_GROUP_PREFIX)
}

/// Whether `path` is a catacombs gameplay tile: its alias slots (`EncounterNamedEnemy`,
/// `EncounterBoss`, `EncounterHazard`, `Hallway`) spawn one of their entries per run (Observed:
/// `CATACOMBS2` shows `dg_enc_catacombs_namedenemy_werebeast` and `_warden`, two entries of
/// `catacombs_gameplay_tile_namedenemy.aliasasset`, in two named rooms).
fn is_catacomb_tile(path: &str) -> bool {
    path.starts_with(CATACOMB_TILE_FOLDER)
}

/// The file name of the pak path `path`.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether the slice at `path` is a spawn group of the open world.
fn is_spawn_group(path: &str) -> bool {
    let file = file_name(path);
    SPAWN_GROUP_PREFIXES
        .iter()
        .any(|prefix| file.starts_with(prefix))
}

/// The pak path of a game mode's `SlicePath` (`Dungeon/LevelScripts/AmrineDungeon`).
pub fn script_slice_path(slice_path: &str) -> String {
    pak_path(&format!("{SLICES_DIR}/{slice_path}{DYNAMIC_SLICE_SUFFIX}"))
}

/// Whether `path` is a capitals file of level `folder`.
fn is_capitals_of(path: &str, folder: &str) -> bool {
    path.starts_with(&format!("{COATLICUE_DIR}/{folder}/")) && path.ends_with(CAPITALS_SUFFIX)
}

/// The levels of every capital of `paths` whose slice is a level script of `wanted`, as
/// (script path, placement).
fn script_placements(
    index: &PakIndex,
    folder: &str,
    wanted: impl Fn(&str) -> bool,
) -> Result<Vec<(String, Transform)>, DungeonScriptsBuildError> {
    let paths = index.paths(|path| is_capitals_of(path, folder));
    let mut found = Vec::new();
    for (path, bytes) in index.read_each(&paths, |_, bytes| bytes) {
        let bytes = without_azcs(bytes?).map_err(|source| DungeonScriptsBuildError::Azcs {
            path: path.clone(),
            source,
        })?;
        let capitals = parse_capitals(&bytes)
            .map_err(|source| DungeonScriptsBuildError::Capitals { path, source })?;
        for capital in capitals {
            if let Some(script) = placed_script(&capital.slice_name, &wanted) {
                found.push((
                    script,
                    Transform::placed(
                        capital.position,
                        capital.rotation,
                        capital.scale.unwrap_or(UNIT_SCALE),
                    ),
                ));
            }
        }
    }
    Ok(found)
}

/// An open world encounter slice that no spawn group, alias asset or capital of the client data
/// places, at the place of its retail root.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RetailPlacement {
    /// The name the encounter takes (its slice's root entity).
    name: &'static str,
    /// The slice's pak path.
    slice: &'static str,
    /// The retail root's place (world metres).
    position: [f32; 3],
    /// The retail root's yaw (degrees counter-clockwise).
    yaw_deg: f32,
}

/// The retail-placed encounters (Observed): the stronghold siren's root group came at 4558.190 s
/// of `Playthrough_Part_18` with a dynamic `GdeRef` at x 11490.633, y 2738.5813 and the raw height
/// 4606 (47.594 m), with no rotation (yaw 0). No dynamic slice spawner names the slice by its path
/// or its GUID `501044708f825265...`, no alias asset lists it and no capital of any level names it
/// (door study 2026-11-12, every pak of build 25810808); the server places it from data the
/// client does not have.
const RETAIL_PLACED_ENCOUNTERS: [RetailPlacement; 1] = [RetailPlacement {
    name: "Enc_WorldEvent_Stronghold_Siren",
    slice: "slices/worldevents/elite_stronghold_siege/siren/enc_worldevent_stronghold_siren.dynamicslice",
    position: [11_490.633, 2_738.581_3, 47.594],
    yaw_deg: 0.0,
}];

/// The encounter of `placement`; `None` when its slice is not in the paks or holds no
/// encounter manager.
fn retail_placed_encounter(
    source: &mut impl SliceSource,
    placement: &RetailPlacement,
    mobs: MobSources<'_>,
    points: &[EncounterPoint],
    report: &mut DungeonScriptsReport,
) -> Result<Option<Encounter>, DungeonScriptsBuildError> {
    let Some(script) = read_script(source, placement.slice, &mut report.missing_slices)? else {
        return Ok(None);
    };
    if script.manager().is_none() {
        return Ok(None);
    }
    let half_yaw = placement.yaw_deg.to_radians() / 2.0;
    let placed = Transform::placed(
        placement.position,
        [0.0, 0.0, half_yaw.sin(), half_yaw.cos()],
        UNIT_SCALE,
    );
    let entity = ScriptEntity {
        name: placement.name.to_owned(),
        ..ScriptEntity::default()
    };
    build_encounter(
        source,
        MobSources {
            prefabs: None,
            ..mobs
        },
        report,
        (&entity, placement.slice, &script, placed),
        points,
    )
    .map(Some)
}

/// The level of each vitals row (`VitalsLevelVariantData`), by lower-case id.
pub fn vitals_levels(tables: &Tables) -> HashMap<String, u32> {
    tables
        .vitals_level_variant_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let level = u32::try_from(row.level?).ok()?;
            Some((row.vitals_id.to_lowercase(), level))
        })
        .collect()
}

/// A mob that a level's metadata lists for an encounter (`IsEncounter`): the export resolves the
/// encounters' creature slices to their vitals rows there.
#[derive(Debug, Clone, PartialEq)]
pub struct EncounterPoint {
    /// The vitals row id.
    pub vitals_id: String,
    /// The level.
    pub level: u32,
    /// The encounter's place.
    pub position: [f32; 3],
}

/// The vitals row id of each lower-case id CRC.
fn vitals_names(tables: &Tables) -> HashMap<u32, String> {
    let base = tables
        .vitals_base_data
        .iter()
        .flat_map(|table| &table.rows)
        .map(|row| row.vitals_id.clone());
    let variants = tables
        .vitals_level_variant_data
        .iter()
        .flat_map(|table| &table.rows)
        .map(|row| row.vitals_id.clone());
    base.chain(variants)
        .map(|id| (nw_formats::objectstream::lowercase_crc32(&id), id))
        .collect()
}

/// The encounter mobs of the metadata files of level `folder`, once each.
fn encounter_points(
    index: &PakIndex,
    folder: &str,
    names: &HashMap<u32, String>,
) -> Result<Vec<EncounterPoint>, DungeonScriptsBuildError> {
    let paths = index.paths(|path| {
        path.starts_with(&format!("{COATLICUE_DIR}/{folder}/")) && path.ends_with(METADATA_SUFFIX)
    });
    let mut records = MetadataRecords::default();
    let mut skipped = SkipCounts::default();
    for (path, bytes) in index.read_each(&paths, |_, bytes| bytes) {
        let bytes = without_azcs(bytes?).map_err(|source| DungeonScriptsBuildError::Azcs {
            path: path.clone(),
            source,
        })?;
        let root = parse_object_stream(&bytes).map_err(|source| {
            DungeonScriptsBuildError::ObjectStream {
                path: path.clone(),
                source,
            }
        })?;
        read_metadata(&root, &mut records, &mut skipped)
            .map_err(|source| DungeonScriptsBuildError::Metadata { path, source })?;
    }
    let mut points: Vec<EncounterPoint> = Vec::new();
    for spawn in records.vitals.iter().filter(|spawn| spawn.is_encounter) {
        let Some(vitals_id) = names.get(&spawn.vitals_id) else {
            continue;
        };
        let point = EncounterPoint {
            vitals_id: vitals_id.clone(),
            level: spawn.level,
            position: spawn.position,
        };
        if !points.contains(&point) {
            points.push(point);
        }
    }
    Ok(points)
}

/// Gives the creature slices of `encounter` without a mob the encounter points of its place
/// whose vitals rows none of its spawner slices name, in target order (best guess: the export
/// writes one point per directly spawned creature; the order of several is Unknown).
fn assign_creatures(encounter: &mut Encounter, points: &[EncounterPoint]) {
    let named: Vec<String> = encounter
        .targets
        .iter()
        .flat_map(|target| &target.slices)
        .filter_map(|slice| slice.mob.as_ref())
        .map(|mob| mob.vitals_id.to_lowercase())
        .collect();
    let place = encounter.position;
    let mut free = points.iter().filter(|point| {
        let near = point
            .position
            .iter()
            .zip(place)
            .all(|(a, b)| (a - b).abs() <= ENCOUNTER_POINT_MATCH_M);
        near && !named.contains(&point.vitals_id.to_lowercase())
    });
    for slice in encounter
        .targets
        .iter_mut()
        .flat_map(|target| &mut target.slices)
        .filter(|slice| slice.mob.is_none() && is_creature(&slice.path))
    {
        let Some(point) = free.next() else {
            break;
        };
        slice.mob = Some(TargetMob {
            vitals_id: point.vitals_id.clone(),
            category_id: String::new(),
            level: point.level,
        });
    }
}

/// Whether `path` is a creature slice.
fn is_creature(path: &str) -> bool {
    path.starts_with(CREATURE_DIR) && !NOT_CREATURE_DIRS.iter().any(|dir| path.contains(dir))
}

/// One instance level script placed in its level.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacedScript {
    /// The level folder.
    pub(crate) folder: String,
    /// The level script's pak path.
    pub(crate) script: String,
    /// Where a capital of the level places it.
    pub(crate) placement: Transform,
    /// The game modes that run it.
    pub(crate) modes: Vec<String>,
}

/// The level script of every instance game mode of `tables` (`InstanceCatalog`, its
/// `SlicePath`) that a capital of its level places, once per script; the modes without a slice
/// path or a capital go to `skipped`.
///
/// # Errors
///
/// Returns [`DungeonScriptsBuildError`] when a capitals file cannot be read.
pub(crate) fn placed_scripts(
    index: &PakIndex,
    tables: &Tables,
    skipped: &mut Vec<String>,
) -> Result<Vec<PlacedScript>, DungeonScriptsBuildError> {
    let catalog = InstanceCatalog::from_tables(tables);
    let slice_paths: HashMap<&str, &str> = tables
        .game_mode_map_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| Some((row.game_mode_id.as_str(), row.slice_path.as_deref()?)))
        .collect();
    // Level folder → script path → game modes.
    let mut wanted: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for mode in catalog.modes() {
        match slice_paths.get(mode.id.as_str()) {
            Some(slice) => wanted
                .entry(mode.map.folder.clone())
                .or_default()
                .entry(script_slice_path(slice))
                .or_default()
                .push(mode.id.clone()),
            None => skipped.push(mode.id.clone()),
        }
    }
    let mut placed = Vec::new();
    for (folder, by_script) in wanted {
        let scripts_here: Vec<String> = by_script.keys().cloned().collect();
        let placements = script_placements(index, &folder, |script| {
            scripts_here.iter().any(|known| known == script)
        })?;
        for (script, modes) in by_script {
            match placements.iter().find(|(path, _)| *path == script) {
                Some((_, placement)) => placed.push(PlacedScript {
                    folder: folder.clone(),
                    script,
                    placement: *placement,
                    modes,
                }),
                None => skipped.extend(modes),
            }
        }
    }
    Ok(placed)
}

/// Builds the dungeon script file of `tables` from the paks of `index` into `out_dir`.
///
/// # Errors
///
/// Returns [`DungeonScriptsBuildError`] when a pak entry cannot be read or decoded, or the file
/// cannot be written or reads back other.
pub fn build_dungeon_scripts(
    index: &PakIndex,
    tables: &Tables,
    client_build: &str,
    out_dir: &Path,
) -> Result<DungeonScriptsReport, DungeonScriptsBuildError> {
    let (levels, prefabs) = mob_tables(tables, out_dir, client_build);
    let names = vitals_names(tables);
    let mobs = MobSources {
        levels: &levels,
        prefabs: Some(&prefabs),
    };
    let mut report = DungeonScriptsReport::default();
    let mut slices = PakSlices::new(index);
    let mut scripts = DungeonScripts::default();
    let mut points_of: HashMap<String, Vec<EncounterPoint>> = HashMap::new();
    for placed in placed_scripts(index, tables, &mut report.skipped_modes)? {
        if !points_of.contains_key(&placed.folder) {
            let points = encounter_points(index, &placed.folder, &names)?;
            points_of.insert(placed.folder.clone(), points);
        }
        let points = points_of.get(&placed.folder).map_or(&[][..], Vec::as_slice);
        let Some(mut level) = build_level(
            &mut slices,
            &placed.script,
            placed.placement,
            mobs,
            points,
            &mut report,
        )?
        else {
            report.skipped_modes.extend(placed.modes);
            continue;
        };
        level.level.clone_from(&placed.folder);
        report.levels += 1;
        for mode in placed.modes {
            scripts.game_modes.insert(mode, level.clone());
        }
    }
    let points = encounter_points(index, OPEN_WORLD_LEVEL, &names)?;
    let mut open_world = LevelScript {
        level: OPEN_WORLD_LEVEL.to_owned(),
        ..LevelScript::default()
    };
    for (script, placement) in script_placements(index, OPEN_WORLD_LEVEL, is_spawn_group)? {
        let Some(group) = build_level(&mut slices, &script, placement, mobs, &points, &mut report)?
        else {
            continue;
        };
        open_world.encounters.extend(group.encounters);
        open_world.chests.extend(group.chests);
    }
    for placement in &RETAIL_PLACED_ENCOUNTERS {
        let placed = open_world
            .encounters
            .iter()
            .any(|encounter| encounter.slice == placement.slice);
        if placed {
            continue;
        }
        if let Some(encounter) =
            retail_placed_encounter(&mut slices, placement, mobs, &points, &mut report)?
        {
            open_world.encounters.push(encounter);
        }
    }
    let tiers = load_breach_tiers(out_dir, client_build)?;
    finish_grouped_encounters(&mut open_world.encounters, &tiers, &levels, &mut report);
    report.open_world_encounters = open_world.encounters.len();
    scripts
        .game_modes
        .insert(OPEN_WORLD_LEVEL.to_owned(), open_world);
    let path = dungeon_scripts_path(out_dir, client_build);
    scripts.write(&path)?;
    if DungeonScripts::load(&path)? != scripts {
        return Err(DungeonScriptsBuildError::Reload(path));
    }
    report.path = path;
    Ok(report)
}

/// The level of each vitals row ([`vitals_levels`]) and the vitals row of each creature slice
/// asset of the mob prefab file in `out_dir` ([`prefab_mobs`]; empty without the file): the
/// tables a [`MobSources`] borrows.
pub fn mob_tables(
    tables: &Tables,
    out_dir: &Path,
    client_build: &str,
) -> (HashMap<String, u32>, HashMap<[u8; ASSET_ID_LEN], String>) {
    let names = vitals_names(tables);
    let prefabs = MobPrefabs::load(out_dir, client_build)
        .map(|prefabs| prefab_mobs(&prefabs, &names))
        .unwrap_or_default();
    (vitals_levels(tables), prefabs)
}

/// The script of the level script slice `script` placed at `placement`; `None` when the slice
/// is not in `source`.
///
/// # Errors
///
/// Returns [`DungeonScriptsBuildError`] when a slice cannot be read.
pub fn build_level(
    source: &mut impl SliceSource,
    script: &str,
    placement: Transform,
    mobs: MobSources<'_>,
    points: &[EncounterPoint],
    report: &mut DungeonScriptsReport,
) -> Result<Option<LevelScript>, DungeonScriptsBuildError> {
    let Some(level_script) = read_script(source, script, &mut report.missing_slices)? else {
        return Ok(None);
    };
    let mut level = LevelScript {
        script_slice: script.to_owned(),
        ..LevelScript::default()
    };
    for entity in &level_script.entities {
        let Some(spawner) = entity.spawner.as_ref() else {
            continue;
        };
        let own_place = placement.then(&level_script.placement_of(entity.id));
        if let Some(spawned) = &spawner.slice {
            let spawned_path = source.asset_path(spawned)?;
            let Some(spawned_script) =
                read_script(source, &spawned_path, &mut report.missing_slices)?
            else {
                continue;
            };
            if spawned_script.manager().is_some() {
                let encounter = build_encounter(
                    source,
                    MobSources {
                        prefabs: None,
                        ..mobs
                    },
                    report,
                    (entity, &spawned_path, &spawned_script, own_place),
                    points,
                )?;
                level.encounters.push(encounter);
            } else if let Some(table) = spawned_script.loot_table() {
                report.chests += 1;
                level.chests.push(Chest {
                    name: entity.name.clone(),
                    slice: spawned_path,
                    position: position_of(&own_place),
                    loot_table: table.to_owned(),
                });
            }
        } else if let Some(alias) = spawner.alias.as_ref().filter(|alias| {
            (is_breach_group(script) || is_catacomb_tile(script))
                && alias.path.ends_with(ALIAS_SUFFIX)
        }) {
            let places = spawn_points(&level_script, entity.id)
                .map(|point| placement.then(&level_script.placement_of(point)))
                .collect::<Vec<_>>();
            let places = if places.is_empty() {
                vec![own_place]
            } else {
                places
            };
            let group = format!("{script}#{}", entity.id);
            for choice in read_alias(source, &alias.path, &mut report.missing_slices)? {
                let path = source.asset_path(&choice.slice)?;
                let Some(spawned_script) = read_script(source, &path, &mut report.missing_slices)?
                else {
                    continue;
                };
                if spawned_script.manager().is_none() {
                    continue;
                }
                for place in &places {
                    // The level's encounter points are no help here: the metadata lists every
                    // difficulty's mob of every breach kind at a group's place, so the mobs of a
                    // grouped encounter come from the mob prefab table
                    // ([`finish_grouped_encounters`]).
                    let mut encounter = build_encounter(
                        source,
                        mobs,
                        report,
                        (entity, &path, &spawned_script, *place),
                        &[],
                    )?;
                    encounter.spawn_group = Some(group.clone());
                    report.grouped_encounters += 1;
                    level.encounters.push(encounter);
                }
            }
        }
    }
    Ok(Some(level))
}

/// The encounter of `spawned_script` (the slice at `path`), spawned by the level script entity
/// `entity` at `placed`, with the encounter points `points` of its level.
fn build_encounter(
    source: &mut impl SliceSource,
    mobs: MobSources<'_>,
    report: &mut DungeonScriptsReport,
    (entity, path, spawned_script, placed): (&ScriptEntity, &str, &SliceScript, Transform),
    points: &[EncounterPoint],
) -> Result<Encounter, DungeonScriptsBuildError> {
    let mut builder = EncounterBuilder::new(source, mobs, report);
    let mut encounter = builder.encounter(&entity.name, path, spawned_script, placed)?;
    encounter.spawner = entity.rules;
    encounter.rewards = spawned_script.rewards().cloned();
    if entity.rules.is_some() {
        report.encounters_with_spawner_rules += 1;
    }
    assign_creatures(&mut encounter, points);
    report.encounters += 1;
    report.stages += encounter.stages.len();
    report.spawns += encounter
        .stages
        .iter()
        .map(|stage| stage.spawns.len())
        .sum::<usize>();
    Ok(encounter)
}

/// The spawn points of the alias spawner entity `spawner` of `script`: the script's entities
/// with no spawner, encounter, stage or mob of their own (Observed: the corrupted breaches'
/// `Darkness_SpawnGroup_Minor_Master` holds `sp01` to `sp07`, 70 to 320 m from the group, and the
/// retail minor breaches stood at such points; `Darkness_SpawnGroup_Major_Master` holds `sp01`
/// at the group's own place, where the retail major breaches stood).
fn spawn_points(script: &SliceScript, spawner: u64) -> impl Iterator<Item = u64> + '_ {
    script
        .entities
        .iter()
        .filter(move |entity| {
            entity.id != spawner
                && entity.spawner.is_none()
                && entity.manager.is_none()
                && entity.stage.is_none()
                && entity.variant.is_none()
        })
        .map(|entity| entity.id)
}

/// The entries of the alias asset at `path`; none (and the path noted in `missing`) when it is
/// not there.
fn read_alias(
    source: &mut impl SliceSource,
    path: &str,
    missing: &mut Vec<String>,
) -> Result<Vec<AliasSlice>, DungeonScriptsBuildError> {
    let Some(root) = source.root(path)? else {
        if !missing.iter().any(|known| known == path) {
            missing.push(path.to_owned());
        }
        return Ok(Vec::new());
    };
    alias_slices(&root).map_err(|source| DungeonScriptsBuildError::Script {
        path: path.to_owned(),
        source,
    })
}

/// The script of the slice at `path`; `None` (and the path noted in `missing`) when it is not
/// there.
pub(crate) fn read_script(
    source: &mut impl SliceSource,
    path: &str,
    missing: &mut Vec<String>,
) -> Result<Option<SliceScript>, DungeonScriptsBuildError> {
    let Some(root) = source.root(path)? else {
        if !missing.iter().any(|known| known == path) {
            missing.push(path.to_owned());
        }
        return Ok(None);
    };
    slice_script(&root)
        .map(Some)
        .map_err(|source| DungeonScriptsBuildError::Script {
            path: path.to_owned(),
            source,
        })
}

/// The turn of `place` from `from`'s yaw, degrees counter-clockwise (Observed: the holdout's
/// one-way door faced its `sp03 - Entrance` location's way, `99a_msq_01b_learnsoulwardens`
/// 245.635 s).
#[allow(clippy::cast_possible_truncation, reason = "degrees")]
fn turn_deg(place: &Transform, from: &Transform) -> f32 {
    (place.yaw_degrees() - from.yaw_degrees()) as f32
}

/// World metres of a transform's translation.
#[allow(clippy::cast_possible_truncation, reason = "world metres")]
pub(crate) fn position_of(transform: &Transform) -> [f32; 3] {
    transform.translation.map(|value| value as f32)
}

/// `transform` as a collision place: its basis rows and its translation.
#[allow(
    clippy::cast_possible_truncation,
    reason = "world metres and unit factors"
)]
fn affine_of(transform: &Transform) -> Affine {
    let mut place = [[0.0; 4]; 3];
    for (row, out) in place.iter_mut().enumerate() {
        for (column, value) in out.iter_mut().take(3).enumerate() {
            *value = transform.basis[row][column] as f32;
        }
        out[3] = transform.translation[row] as f32;
    }
    place
}

/// Builds one encounter, resolving its targets once each.
struct EncounterBuilder<'a, S> {
    source: &'a mut S,
    mobs: MobSources<'a>,
    report: &'a mut DungeonScriptsReport,
    targets: Vec<Target>,
    /// Target index by (slice path, alias path).
    target_index: HashMap<(String, String), usize>,
}

impl<'a, S: SliceSource> EncounterBuilder<'a, S> {
    fn new(source: &'a mut S, mobs: MobSources<'a>, report: &'a mut DungeonScriptsReport) -> Self {
        Self {
            source,
            mobs,
            report,
            targets: Vec::new(),
            target_index: HashMap::new(),
        }
    }

    fn encounter(
        &mut self,
        name: &str,
        path: &str,
        script: &SliceScript,
        placed: Transform,
    ) -> Result<Encounter, DungeonScriptsBuildError> {
        let Some((root, manager)) = script.manager() else {
            return Ok(Encounter::default());
        };
        // Stage index of each stage entity, in stream order.
        let stage_ids: Vec<u64> = script
            .entities
            .iter()
            .filter(|entity| entity.stage.is_some())
            .map(|entity| entity.id)
            .collect();
        let index_of = |id: &u64| stage_ids.iter().position(|known| known == id);
        let mut stages = Vec::with_capacity(stage_ids.len());
        for entity in script
            .entities
            .iter()
            .filter(|entity| entity.stage.is_some())
        {
            let Some(stage) = &entity.stage else {
                continue;
            };
            let mut spawns = Vec::new();
            for definition in &stage.spawns {
                let Some(target) =
                    self.target(definition.slice.as_ref(), definition.alias.as_ref())?
                else {
                    continue;
                };
                let places: Vec<Transform> = definition
                    .locations
                    .iter()
                    .map(|location| placed.then(&script.placement_of(*location)))
                    .collect();
                spawns.push(Spawn {
                    target,
                    delay_s: definition.delay_s,
                    count: definition.count,
                    locations: places.iter().map(position_of).collect(),
                    turns_deg: places
                        .iter()
                        .map(|place| turn_deg(place, &placed))
                        .collect(),
                });
            }
            let mut objectives = Vec::new();
            for objective in &stage.objectives {
                let kind = match &objective.kind {
                    ObjectiveKind::SliceDestroyed {
                        slice,
                        alias,
                        required,
                    } => ObjectiveSpec::SliceDestroyed {
                        target: self.target(slice.as_ref(), alias.as_ref())?,
                        required: *required,
                    },
                    ObjectiveKind::BossPhase {
                        slice,
                        alias,
                        phase,
                    } => ObjectiveSpec::BossPhase {
                        target: self.target(slice.as_ref(), alias.as_ref())?,
                        phase: *phase,
                    },
                    ObjectiveKind::EncounterEvent { event } => ObjectiveSpec::EncounterEvent {
                        event: event.clone(),
                    },
                    ObjectiveKind::EntityActiveState {
                        entity,
                        on_activated,
                        on_deactivated,
                    } => ObjectiveSpec::EntityActiveState {
                        entity: *entity,
                        on_activated: *on_activated,
                        on_deactivated: *on_deactivated,
                    },
                    ObjectiveKind::WaitForParent => ObjectiveSpec::WaitForParent,
                    ObjectiveKind::InvasionWave => ObjectiveSpec::InvasionWave,
                };
                objectives.push(StageObjective {
                    time_limit_s: objective.time_limit_s,
                    succeed_on_timeout: objective.succeed_on_timeout,
                    failure: objective.failure,
                    kind,
                });
            }
            stages.push(Stage {
                name: entity.name.clone(),
                entity: entity.id,
                child_stages_optional: stage.child_stages_optional,
                children: stage.stages.iter().filter_map(index_of).collect(),
                spawns,
                objectives,
                start_actions: self.actions(&stage.start_actions)?,
                end_actions: self.actions(&stage.end_actions)?,
            });
        }
        #[allow(clippy::cast_possible_truncation, reason = "degrees")]
        let yaw_deg = placed.yaw_degrees() as f32;
        let root_place = placed.then(&script.placement_of(root.id));
        let detector = root.detection_shape.as_ref().and_then(|(shape, offset)| {
            let detector = detector_volume(shape, *offset, &affine_of(&root_place));
            if detector.is_none() {
                self.report.unread_detectors += 1;
            }
            detector
        });
        if detector.is_some() {
            self.report.detectors += 1;
        }
        Ok(Encounter {
            name: name.to_owned(),
            slice: path.to_owned(),
            position: position_of(&root_place),
            yaw_deg,
            auto_activate: manager.auto_activate,
            auto_activate_delay_ms: manager.auto_activate_delay_ms,
            grace_period_s: manager.grace_period_s,
            reset_event: manager.reset_event.clone(),
            detection_radius_m: root.detection_radius_m,
            detector,
            top_stages: manager.stages.iter().filter_map(index_of).collect(),
            stages,
            targets: std::mem::take(&mut self.targets),
            spawner: None,
            spawn_group: None,
            difficulty: None,
            rewards: None,
        })
    }

    fn actions(
        &mut self,
        actions: &[Action],
    ) -> Result<Vec<StageAction>, DungeonScriptsBuildError> {
        let mut built = Vec::with_capacity(actions.len());
        for action in actions {
            built.push(match action {
                Action::EntityEvent {
                    entity,
                    event,
                    recursive,
                    local_player_only,
                } => StageAction::EntityEvent {
                    entity: *entity,
                    event: event_value(*event),
                    recursive: *recursive,
                    local_player_only: *local_player_only,
                },
                Action::DestroySlice { slice, alias } => StageAction::DestroySlice {
                    target: self.target(slice.as_ref(), alias.as_ref())?,
                },
                Action::KillSpawns => StageAction::KillSpawns,
            });
        }
        Ok(built)
    }

    /// The index of the target of `slice` and `alias`; `None` when both are empty.
    fn target(
        &mut self,
        slice: Option<&AssetRef>,
        alias: Option<&AssetRef>,
    ) -> Result<Option<usize>, DungeonScriptsBuildError> {
        if slice.is_none() && alias.is_none() {
            return Ok(None);
        }
        // A spawn and an action may name one slice by an old and a new path (the holdout's
        // `Wave_01 - Interact` spawns `.../design/worldevent_object_holdout_door_solid` and
        // destroys `.../design/holdout/worldevent_object_holdout_door_solid`): one target.
        let slice = match slice {
            Some(asset) => Some(AssetRef {
                path: self.source.asset_path(asset)?,
                guid: asset.guid,
            }),
            None => None,
        };
        let slice = slice.as_ref();
        let key = (
            slice.map(|asset| asset.path.clone()).unwrap_or_default(),
            alias.map(|asset| asset.path.clone()).unwrap_or_default(),
        );
        if let Some(index) = self.target_index.get(&key) {
            return Ok(Some(*index));
        }
        let mut target = Target {
            name: if key.0.is_empty() {
                key.1.clone()
            } else {
                key.0.clone()
            },
            slices: Vec::new(),
        };
        if let Some(slice) = slice {
            target.slices.push(self.target_slice(slice, 0)?);
        }
        if let Some(alias) = alias
            && alias.path.ends_with(ALIAS_SUFFIX)
        {
            for AliasSlice { slice, percent } in self.alias(&alias.path)? {
                target.slices.push(self.target_slice(&slice, percent)?);
            }
        }
        let index = self.targets.len();
        self.targets.push(target);
        self.target_index.insert(key, index);
        Ok(Some(index))
    }

    fn alias(&mut self, path: &str) -> Result<Vec<AliasSlice>, DungeonScriptsBuildError> {
        read_alias(&mut *self.source, path, &mut self.report.missing_slices)
    }

    fn target_slice(
        &mut self,
        slice: &AssetRef,
        percent: u32,
    ) -> Result<TargetSlice, DungeonScriptsBuildError> {
        let slice = &AssetRef {
            path: self.source.asset_path(slice)?,
            guid: slice.guid,
        };
        let found = self.mob_of(slice, NESTED_DEPTH)?;
        let mobs_per_spawn = found.count;
        let level_of = |vitals_id: &str| {
            self.mobs
                .levels
                .get(&vitals_id.to_lowercase())
                .copied()
                .unwrap_or_default()
        };
        let difficulty_mobs = found
            .by_difficulty
            .iter()
            .map(|(difficulty, vitals_id)| {
                (
                    *difficulty,
                    TargetMob {
                        vitals_id: vitals_id.clone(),
                        category_id: String::new(),
                        level: level_of(vitals_id),
                    },
                )
            })
            .collect();
        let mob = found.variant.map(|variant| {
            let level = match variant.level {
                0 => level_of(&variant.vitals_id),
                level => level,
            };
            if level == 0 {
                self.report.levelless_mobs += 1;
            }
            TargetMob {
                vitals_id: variant.vitals_id,
                category_id: variant.category_id,
                level,
            }
        });
        if mob.is_some() {
            self.report.spawner_mobs += 1;
        } else {
            self.report.other_slices += 1;
        }
        Ok(TargetSlice {
            path: slice.path.clone(),
            guid: slice.guid,
            percent,
            mob,
            mobs_per_spawn: mobs_per_spawn.max(ONE_MOB),
            waypoints: self.waypoints(&slice.path)?,
            difficulty: slice_difficulty(&slice.path),
            difficulty_mobs,
        })
    }

    /// The waypoints of the spawner slice at `path`, from the spawner's own place.
    fn waypoints(&mut self, path: &str) -> Result<Option<PatrolPath>, DungeonScriptsBuildError> {
        let origin = Capital {
            rotation: NO_ROTATION,
            ..Capital::default()
        };
        Ok(self
            .source
            .root(path)?
            .and_then(|root| SpawnerSlice::of(&root))
            .map(|spawner| spawner.at(&origin)))
    }

    /// The mob the slice `slice` spawns and how many of it one spawn makes: its own vitals
    /// row, the mob the retail captures showed for its asset (only with the mob prefab table:
    /// a grouped encounter's), or the mobs of the slices its spawners and its stages spawn, at
    /// most `depth` slices down (each open world spawn group spawns its encounter slice, which
    /// may spawn a nested one). No mob for a slice that leads to none. With the table, the mobs
    /// of the nested slices of one difficulty each (`_dt<n>`) are kept by difficulty.
    fn mob_of(
        &mut self,
        slice: &AssetRef,
        depth: u32,
    ) -> Result<SliceMobs, DungeonScriptsBuildError> {
        let path = self.source.asset_path(slice)?;
        let Some(script) = read_script(&mut *self.source, &path, &mut self.report.missing_slices)?
        else {
            return Ok(SliceMobs::default());
        };
        if let Some(variant) = script.variant() {
            // A spawner slice with more than one spawner entity makes one mob for each (the
            // storm priest's `worldevent_ai_cultist_chaplain_minion`).
            let spawners = script
                .entities
                .iter()
                .filter(|entity| entity.variant.is_some())
                .count();
            return Ok(SliceMobs {
                variant: Some(variant.clone()),
                count: u32::try_from(spawners).unwrap_or(ONE_MOB),
                by_difficulty: BTreeMap::new(),
            });
        }
        if let Some(vitals_id) = self
            .mobs
            .prefabs
            .and_then(|prefabs| prefabs.get(&slice.guid))
        {
            return Ok(SliceMobs {
                variant: Some(VitalsVariant {
                    vitals_id: vitals_id.clone(),
                    category_id: String::new(),
                    level: 0,
                }),
                count: ONE_MOB,
                by_difficulty: slice_difficulty(&path)
                    .map(|difficulty| BTreeMap::from([(difficulty, vitals_id.clone())]))
                    .unwrap_or_default(),
            });
        }
        if depth == 0 {
            return Ok(SliceMobs::default());
        }
        // Each spawn: its slice, else its alias (the first slice of the alias that is a mob).
        let spawned: Vec<(Option<AssetRef>, Option<AssetRef>, u32)> = script
            .entities
            .iter()
            .flat_map(|entity| match &entity.stage {
                Some(stage) => stage
                    .spawns
                    .iter()
                    .map(|spawn| {
                        (
                            named(spawn.slice.as_ref()),
                            named(spawn.alias.as_ref()),
                            spawn.count,
                        )
                    })
                    .collect::<Vec<_>>(),
                None => entity
                    .spawner
                    .iter()
                    .map(|spawner| {
                        (
                            named(spawner.slice.as_ref()),
                            named(spawner.alias.as_ref()),
                            ONE_MOB,
                        )
                    })
                    .collect(),
            })
            .collect();
        let mut found = SliceMobs::default();
        for (slice, alias, count) in spawned {
            let mut candidates: Vec<AssetRef> = slice.into_iter().collect();
            if let Some(alias) = alias.filter(|alias| alias.path.ends_with(ALIAS_SUFFIX)) {
                candidates.extend(
                    self.alias(&alias.path)?
                        .into_iter()
                        .map(|entry| entry.slice),
                );
            }
            let mut by_difficulty = BTreeMap::new();
            let mut first = None;
            for nested in &candidates {
                let mobs = self.mob_of(nested, depth - 1)?;
                for (difficulty, vitals_id) in &mobs.by_difficulty {
                    by_difficulty
                        .entry(*difficulty)
                        .or_insert_with(|| vitals_id.clone());
                }
                if first.is_none() && mobs.variant.is_some() {
                    first = Some(mobs);
                }
            }
            if let Some(mobs) = first {
                found.count += count * mobs.count;
                found.variant = mobs.variant;
                found.by_difficulty = by_difficulty;
            }
        }
        Ok(found)
    }
}

/// The mobs a slice leads to ([`EncounterBuilder::mob_of`]).
#[derive(Debug, Default)]
struct SliceMobs {
    /// Its mob.
    variant: Option<VitalsVariant>,
    /// How many mobs one spawn makes.
    count: u32,
    /// The vitals row of each difficulty, when its nested slices hold one per difficulty.
    by_difficulty: BTreeMap<u32, String>,
}

/// What the builder knows of the mobs.
#[derive(Debug, Clone, Copy)]
pub struct MobSources<'a> {
    /// The level of each vitals row, by lower-case id ([`vitals_levels`]).
    pub levels: &'a HashMap<String, u32>,
    /// The vitals row of each creature slice asset the retail captures showed ([`prefab_mobs`]);
    /// used for the encounters of an alias spawner only, `None` for the others.
    pub prefabs: Option<&'a HashMap<[u8; ASSET_ID_LEN], String>>,
}

/// `asset` when it names a pak path.
fn named(asset: Option<&AssetRef>) -> Option<AssetRef> {
    asset.filter(|asset| !asset.path.is_empty()).cloned()
}

/// The difficulty of the slice at `path` (its file name ends in `_dt<n>` or `_l<n>`).
fn slice_difficulty(path: &str) -> Option<u32> {
    let stem = path
        .rsplit('/')
        .next()?
        .strip_suffix(DYNAMIC_SLICE_SUFFIX)?;
    let (_, last) = stem.rsplit_once('_')?;
    DIFFICULTY_MARKS
        .iter()
        .find_map(|mark| last.strip_prefix(mark))
        .and_then(|number| number.parse().ok())
}

/// The vitals row id of the creature slice asset of each mob the retail captures showed (the
/// mob prefab table's asset seen most often of each row; of two rows with one asset, the lower
/// CRC).
fn prefab_mobs(
    prefabs: &MobPrefabs,
    names: &HashMap<u32, String>,
) -> HashMap<[u8; ASSET_ID_LEN], String> {
    let mut by_asset: BTreeMap<[u8; ASSET_ID_LEN], u32> = BTreeMap::new();
    for (vitals, asset) in prefabs.default_assets() {
        by_asset
            .entry(asset)
            .and_modify(|known| *known = (*known).min(vitals))
            .or_insert(vitals);
    }
    by_asset
        .into_iter()
        .filter_map(|(asset, vitals)| Some((asset, names.get(&vitals)?.clone())))
        .collect()
}

/// Gives the grouped encounters (the corrupted breaches) the mobs of the difficulties no capture
/// showed and their difficulty (their target slices have the mobs the retail captures showed for
/// their assets, [`EncounterBuilder::mob_of`], Observed):
///
/// - a slice or nested slice of a difficulty without a mob takes the row of a sibling
///   difficulty's with that difficulty's mark (`Corruption_Impaler_DT2` gives
///   `Corruption_Impaler_DT5` when the vitals table has it; Inferred from the rows' names);
/// - the difficulty is the number of the retail map point at the encounter's place (Observed,
///   [`crate::breach_tiers`]); an encounter without one takes the most common difficulty of its
///   group, a group without one that of the nearest group with one (Inferred).
fn finish_grouped_encounters(
    encounters: &mut [Encounter],
    tiers: &[BreachTier],
    levels: &HashMap<String, u32>,
    report: &mut DungeonScriptsReport,
) {
    for encounter in encounters
        .iter_mut()
        .filter(|encounter| encounter.spawn_group.is_some())
    {
        for target in &mut encounter.targets {
            fill_sibling_mobs(target, levels, report);
        }
        encounter.difficulty = observed_difficulty(encounter.position, tiers);
        if encounter.difficulty.is_some() {
            report.observed_difficulties += 1;
        }
    }
    let mut by_group: BTreeMap<String, BTreeMap<u32, usize>> = BTreeMap::new();
    for encounter in encounters.iter() {
        if let (Some(group), Some(difficulty)) = (&encounter.spawn_group, encounter.difficulty) {
            *by_group
                .entry(group.clone())
                .or_default()
                .entry(difficulty)
                .or_default() += 1;
        }
    }
    let group_difficulty: BTreeMap<&String, u32> = by_group
        .iter()
        .filter_map(|(group, counts)| {
            let (difficulty, _) = counts.iter().rev().max_by_key(|(_, seen)| **seen)?;
            Some((group, *difficulty))
        })
        .collect();
    let known: Vec<([f32; 3], u32)> = encounters
        .iter()
        .filter_map(|encounter| {
            let group = encounter.spawn_group.as_ref()?;
            Some((encounter.position, *group_difficulty.get(group)?))
        })
        .collect();
    for encounter in encounters
        .iter_mut()
        .filter(|encounter| encounter.spawn_group.is_some() && encounter.difficulty.is_none())
    {
        let of_group = encounter
            .spawn_group
            .as_ref()
            .and_then(|group| group_difficulty.get(group))
            .copied();
        encounter.difficulty = of_group.or_else(|| {
            known
                .iter()
                .min_by(|(a, _), (b, _)| {
                    distance_2d(*a, encounter.position)
                        .total_cmp(&distance_2d(*b, encounter.position))
                })
                .map(|(_, difficulty)| *difficulty)
        });
        if encounter.difficulty.is_some() {
            report.inferred_difficulties += 1;
        }
    }
}

/// Gives the difficulty slices of `target` without a mob, and the missing difficulties of the
/// nested slices' mobs, a sibling difficulty's row with their mark (see
/// [`finish_grouped_encounters`]).
fn fill_sibling_mobs(
    target: &mut Target,
    levels: &HashMap<String, u32>,
    report: &mut DungeonScriptsReport,
) {
    let siblings: Vec<(u32, String)> = target
        .slices
        .iter()
        .filter_map(|slice| Some((slice.difficulty?, slice.mob.as_ref()?.vitals_id.clone())))
        .collect();
    for slice in &mut target.slices {
        if slice.mob.is_none()
            && let Some(difficulty) = slice.difficulty
            && let Some(mob) = sibling_mob(&siblings, difficulty, levels)
        {
            slice.mob = Some(mob);
            report.sibling_mobs += 1;
        }
        if slice.difficulty_mobs.is_empty() {
            continue;
        }
        let nested: Vec<(u32, String)> = slice
            .difficulty_mobs
            .iter()
            .map(|(difficulty, mob)| (*difficulty, mob.vitals_id.clone()))
            .collect();
        for difficulty in FIRST_DIFFICULTY..=LAST_DIFFICULTY {
            if slice.difficulty_mobs.contains_key(&difficulty) {
                continue;
            }
            if let Some(mob) = sibling_mob(&nested, difficulty, levels) {
                slice.difficulty_mobs.insert(difficulty, mob);
                report.sibling_mobs += 1;
            }
        }
    }
}

/// The mob of `difficulty` from the rows `siblings` of other difficulties: a row whose id ends
/// in its difficulty's mark, with the mark of `difficulty`, when the vitals table has that row.
fn sibling_mob(
    siblings: &[(u32, String)],
    difficulty: u32,
    levels: &HashMap<String, u32>,
) -> Option<TargetMob> {
    siblings.iter().find_map(|(known, vitals)| {
        let mark = format!("{ROW_DIFFICULTY_MARK}{known}");
        let base = vitals
            .len()
            .checked_sub(mark.len())
            .filter(|&at| vitals.is_char_boundary(at) && vitals[at..].eq_ignore_ascii_case(&mark))
            .map(|at| &vitals[..at])?;
        let vitals_id = format!("{base}{ROW_DIFFICULTY_MARK}{difficulty}");
        let level = *levels.get(&vitals_id.to_lowercase())?;
        Some(TargetMob {
            vitals_id,
            category_id: String::new(),
            level,
        })
    })
}

/// The difficulty of the retail map point nearest `place` within [`TIER_MATCH_M`].
fn observed_difficulty(place: [f32; 3], tiers: &[BreachTier]) -> Option<u32> {
    tiers
        .iter()
        .map(|tier| (distance_2d([tier.x, tier.y, 0.0], place), tier.difficulty))
        .filter(|(distance, _)| *distance <= TIER_MATCH_M)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, difficulty)| difficulty)
}

/// The distance of two places on the ground plane.
fn distance_2d(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The `EventType` value of `event`.
const fn event_value(event: nw_world::slice_triggers::TriggerEvent) -> u32 {
    use nw_world::slice_triggers::TriggerEvent;
    /// `EventType` values (`slice_triggers`).
    const TRIGGER: u32 = 0;
    const UNTRIGGER: u32 = 1;
    const ACTIVATE: u32 = 2;
    const DEACTIVATE: u32 = 3;
    match event {
        TriggerEvent::Trigger => TRIGGER,
        TriggerEvent::UnTrigger => UNTRIGGER,
        TriggerEvent::Activate => ACTIVATE,
        TriggerEvent::Deactivate => DEACTIVATE,
        TriggerEvent::Unnamed(value) => value,
    }
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::{UUID_SIZE, lowercase_crc32};
    use nw_world::level_script::{
        ENCOUNTER_MANAGER_TYPE, ENCOUNTER_STAGE_TYPE, GATHERABLE_CONTROLLER_TYPE,
        PREFAB_SPAWNER_TYPE, VARIANT_PROVIDER_TYPE, objective_type,
    };

    use super::*;

    const SCRIPT: &str = "slices/dungeon/levelscripts/testdungeon.dynamicslice";
    const ENCOUNTER: &str = "slices/dungeon_eventmanager/encounters/dg_enc_test_boss.dynamicslice";
    const SPAWNER: &str = "slices/dungeon_eventmanager/spawners/dg_test_boss.dynamicslice";
    const CREATURE: &str = "slices/characters/test_add.dynamicslice";
    const ALIAS: &str = "sharedassets/genericassets/aliasassets/dungeon/test.aliasasset";
    const CHEST: &str = "slices/interactobjects/dungeonlootcontainers/chest.dynamicslice";
    const VITALS: &str = "Undead_Brute_DG_Test";
    const NAMED_ADD: &str = "Ghost_Named_Test";
    const LEVEL: u32 = 25;
    const PLACE: [f32; 3] = [780.0, 674.0, 134.0];
    const ENCOUNTER_OFFSET: [f32; 3] = [106.0, 232.0, 7.0];
    const LOCATION_OFFSET: [f32; 3] = [2.0, 3.0, 0.0];
    const NO_PARENT: u64 = 0xffff_ffff;
    const ASSET_HINT_LENGTH_AT: usize = 48;

    /// The slices of a test level, by path.
    struct Slices(HashMap<String, Element>);

    impl SliceSource for Slices {
        fn root(&mut self, path: &str) -> Result<Option<Element>, DungeonScriptsBuildError> {
            Ok(self.0.get(path).cloned())
        }
    }

    fn named(name: &str, children: Vec<Element>) -> Element {
        Element::with_children(lowercase_crc32(name), children)
    }

    fn value(name: &str, bytes: &[u8]) -> Element {
        Element::with_value(lowercase_crc32(name), bytes.to_vec())
    }

    fn typed(mut element: Element, type_id: [u8; UUID_SIZE]) -> Element {
        element.type_id = type_id;
        element
    }

    fn id_holder(name: &str, id: u64) -> Element {
        named(name, vec![value("Id", &id.to_be_bytes())])
    }

    fn asset(name: &str, guid: u8, hint: &str) -> Element {
        let mut bytes = vec![guid; UUID_SIZE];
        bytes.resize(ASSET_HINT_LENGTH_AT, 0);
        bytes.extend(u64::try_from(hint.len()).expect("length").to_be_bytes());
        bytes.extend(hint.as_bytes());
        value(name, &bytes)
    }

    fn transform(parent: u64, translation: [f32; 3]) -> Element {
        let mut floats = Vec::new();
        for column in [
            [1.0_f32, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            translation,
        ] {
            for cell in column {
                floats.extend(cell.to_be_bytes());
            }
        }
        named(
            "element",
            vec![value("m_localTM", &floats), id_holder("m_parentId", parent)],
        )
    }

    fn entity(id: u64, name: &str, components: Vec<Element>) -> Element {
        named(
            "element",
            vec![
                id_holder("Id", id),
                value("Name", name.as_bytes()),
                named("Components", components),
            ],
        )
    }

    fn slice(entities: Vec<Element>) -> Element {
        named(
            "root",
            vec![named(
                "Components",
                vec![named("element", vec![named("Entities", entities)])],
            )],
        )
    }

    fn spawner(path: &str) -> Element {
        typed(
            named("element", vec![asset("m_sliceAsset", 1, path)]),
            PREFAB_SPAWNER_TYPE,
        )
    }

    fn level() -> Slices {
        let script = slice(vec![
            entity(1, "Encounters", vec![transform(NO_PARENT, [0.0; 3])]),
            entity(
                2,
                "EncounterBoss",
                vec![transform(1, ENCOUNTER_OFFSET), spawner(ENCOUNTER)],
            ),
            entity(
                3,
                "LockedChest",
                vec![transform(1, [1.0; 3]), spawner(CHEST)],
            ),
        ]);
        let manager = typed(
            named(
                "element",
                vec![
                    value("m_autoActivate", &[1]),
                    named(
                        "m_stages",
                        vec![named("element", vec![id_holder("EntityId", 11)])],
                    ),
                ],
            ),
            ENCOUNTER_MANAGER_TYPE,
        );
        let wave = typed(
            named(
                "element",
                vec![
                    named("m_stages", Vec::new()),
                    named(
                        "m_spawnTimeline",
                        vec![named(
                            "element",
                            vec![
                                asset("m_sliceAsset", 2, SPAWNER),
                                asset("m_aliasAsset", 0, ""),
                                value("m_count", &1_u32.to_be_bytes()),
                                named(
                                    "m_spawnLocations",
                                    vec![named("element", vec![id_holder("EntityId", 12)])],
                                ),
                            ],
                        )],
                    ),
                    named(
                        "m_objectives",
                        vec![named(
                            "element",
                            vec![typed(
                                named(
                                    "element",
                                    vec![
                                        asset("m_trackedSlice", 0, ""),
                                        asset("m_trackedAlias", 3, ALIAS),
                                        value("m_requiredCount", &2_u32.to_be_bytes()),
                                    ],
                                ),
                                objective_type::SLICE_DESTROYED,
                            )],
                        )],
                    ),
                ],
            ),
            ENCOUNTER_STAGE_TYPE,
        );
        let encounter = slice(vec![
            entity(
                10,
                "DG_Enc_Test_Boss",
                vec![transform(NO_PARENT, [0.0; 3]), manager],
            ),
            entity(11, "Wave_01", vec![transform(10, [0.0; 3]), wave]),
            entity(12, "Location", vec![transform(10, LOCATION_OFFSET)]),
        ]);
        let variant = typed(
            named(
                "element",
                vec![
                    value("m_vitalsTableRowId", VITALS.as_bytes()),
                    value("m_vitalsCategoryTableRowId", VITALS.as_bytes()),
                    value("m_vitalsLevel", &0_u32.to_be_bytes()),
                ],
            ),
            VARIANT_PROVIDER_TYPE,
        );
        let boss_spawner = slice(vec![entity(20, "Spawner", vec![variant])]);
        let creature = slice(vec![entity(30, "Add", Vec::new())]);
        let alias = named(
            "root",
            vec![named(
                "slices",
                vec![named(
                    "element",
                    vec![
                        asset("slice", 4, CREATURE),
                        value("Percent", &100_u32.to_be_bytes()),
                    ],
                )],
            )],
        );
        let chest = slice(vec![entity(
            40,
            "Chest",
            vec![typed(
                named("element", vec![value("m_lootTableId", b"ChestLootElite")]),
                GATHERABLE_CONTROLLER_TYPE,
            )],
        )]);
        Slices(HashMap::from([
            (SCRIPT.to_owned(), script),
            (ENCOUNTER.to_owned(), encounter),
            (SPAWNER.to_owned(), boss_spawner),
            (CREATURE.to_owned(), creature),
            (ALIAS.to_owned(), alias),
            (CHEST.to_owned(), chest),
        ]))
    }

    #[test]
    fn a_story_instance_script_is_found_without_its_slices_folder() {
        let wanted = script_slice_path("Dungeon/MedeaWindsward/MedeaScript");
        assert_eq!(
            placed_script("Dungeon/MedeaWindsward/MedeaScript", |path| path == wanted).as_deref(),
            Some(wanted.as_str())
        );
        let generated =
            "CoatGen\\\\efc337e6\\WorldEvent_SpawnGroup_Single_Master_3815808514.dynamicslice";
        assert_eq!(
            placed_script(generated, is_spawn_group).as_deref(),
            Some("coatgen/efc337e6/worldevent_spawngroup_single_master_3815808514.dynamicslice")
        );
        assert_eq!(
            placed_script("Dungeon/Other/OtherScript", |path| path == wanted),
            None
        );
    }

    #[test]
    fn a_level_script_makes_its_encounters_and_chests_in_world_metres() {
        let mut source = level();
        let mut report = DungeonScriptsReport::default();
        let levels = HashMap::from([(VITALS.to_lowercase(), LEVEL)]);
        let placement = Transform::placed(PLACE, [0.0, 0.0, 0.0, 1.0], UNIT_SCALE);
        let points = [EncounterPoint {
            vitals_id: NAMED_ADD.to_owned(),
            level: LEVEL,
            position: [
                PLACE[0] + ENCOUNTER_OFFSET[0],
                PLACE[1] + ENCOUNTER_OFFSET[1],
                PLACE[2] + ENCOUNTER_OFFSET[2],
            ],
        }];
        let level = build_level(
            &mut source,
            SCRIPT,
            placement,
            MobSources {
                levels: &levels,
                prefabs: None,
            },
            &points,
            &mut report,
        )
        .expect("build")
        .expect("level");
        assert_eq!(level.encounters.len(), 1);
        let encounter = &level.encounters[0];
        assert_eq!(encounter.name, "EncounterBoss");
        assert!(encounter.auto_activate);
        let at = |offsets: &[[f32; 3]]| {
            let mut sum = PLACE;
            for offset in offsets {
                for (cell, add) in sum.iter_mut().zip(offset) {
                    *cell += add;
                }
            }
            sum
        };
        assert_eq!(encounter.position, at(&[ENCOUNTER_OFFSET]));
        assert_eq!(encounter.top_stages, [0]);
        let stage = &encounter.stages[0];
        assert_eq!(stage.name, "Wave_01");
        assert_eq!(
            stage.spawns[0].locations,
            [at(&[ENCOUNTER_OFFSET, LOCATION_OFFSET])]
        );
        let boss = &encounter.targets[stage.spawns[0].target];
        assert_eq!(
            boss.slices[0].mob,
            Some(TargetMob {
                vitals_id: VITALS.to_owned(),
                category_id: VITALS.to_owned(),
                level: LEVEL,
            })
        );
        let ObjectiveSpec::SliceDestroyed { target, required } = &stage.objectives[0].kind else {
            panic!("slice destroyed objective");
        };
        assert_eq!(*required, 2);
        let adds = &encounter.targets[target.expect("alias target")];
        assert_eq!(adds.name, ALIAS);
        assert_eq!(adds.slices[0].path, CREATURE);
        assert_eq!(adds.slices[0].percent, 100);
        assert_eq!(
            adds.slices[0].mob,
            Some(TargetMob {
                vitals_id: NAMED_ADD.to_owned(),
                category_id: String::new(),
                level: LEVEL,
            }),
            "the creature slice takes the encounter point its spawners do not name"
        );
        assert_eq!(level.chests.len(), 1);
        assert_eq!(level.chests[0].loot_table, "ChestLootElite");
        assert_eq!(report.encounters, 1);
        assert_eq!((report.spawner_mobs, report.other_slices), (1, 1));
    }

    #[test]
    fn a_missing_script_gives_no_level_and_is_noted() {
        let mut source = Slices(HashMap::new());
        let mut report = DungeonScriptsReport::default();
        let built = build_level(
            &mut source,
            SCRIPT,
            Transform::default(),
            MobSources {
                levels: &HashMap::new(),
                prefabs: None,
            },
            &[],
            &mut report,
        )
        .expect("build");
        assert!(built.is_none());
        assert_eq!(report.missing_slices, [SCRIPT]);
    }

    /// A test level whose spawners name slices by an out of date path, with the asset catalog
    /// that holds their GUIDs' paths.
    struct MovedSlices {
        slices: Slices,
        catalog: HashMap<[u8; ASSET_ID_LEN], String>,
    }

    impl SliceSource for MovedSlices {
        fn root(&mut self, path: &str) -> Result<Option<Element>, DungeonScriptsBuildError> {
            self.slices.root(path)
        }

        fn slice_catalog(
            &mut self,
        ) -> Result<Option<&HashMap<[u8; ASSET_ID_LEN], String>>, DungeonScriptsBuildError>
        {
            Ok(Some(&self.catalog))
        }
    }

    #[test]
    fn a_spawner_s_out_of_date_path_reads_its_guid_s_catalog_path() {
        const STALE: &str = "slices/dungeon_eventmanager/dg_enc_test_boss.dynamicslice";
        let Slices(mut slices) = level();
        slices.insert(
            SCRIPT.to_owned(),
            slice(vec![
                entity(1, "Encounters", vec![transform(NO_PARENT, [0.0; 3])]),
                entity(
                    2,
                    "EncounterBoss",
                    vec![transform(1, ENCOUNTER_OFFSET), spawner(STALE)],
                ),
            ]),
        );
        let mut source = MovedSlices {
            slices: Slices(slices),
            catalog: HashMap::from([([1; ASSET_ID_LEN], ENCOUNTER.to_owned())]),
        };
        let levels = HashMap::new();
        let mut report = DungeonScriptsReport::default();
        let level = build_level(
            &mut source,
            SCRIPT,
            Transform::default(),
            MobSources {
                levels: &levels,
                prefabs: None,
            },
            &[],
            &mut report,
        )
        .expect("build")
        .expect("level");
        assert_eq!(level.encounters.len(), 1);
        assert_eq!(level.encounters[0].slice, ENCOUNTER);
        assert!(
            report.missing_slices.is_empty(),
            "{:?}",
            report.missing_slices
        );
        let Slices(mut without) = source.slices;
        without.remove(ENCOUNTER);
        let mut unknown = Slices(without);
        let mut report = DungeonScriptsReport::default();
        let level = build_level(
            &mut unknown,
            SCRIPT,
            Transform::default(),
            MobSources {
                levels: &levels,
                prefabs: None,
            },
            &[],
            &mut report,
        )
        .expect("build")
        .expect("level");
        assert!(level.encounters.is_empty());
        assert_eq!(report.missing_slices, [STALE]);
    }

    #[test]
    fn an_old_and_a_new_path_of_one_slice_are_one_target() {
        const STALE: &str = "slices/characters/old_folder_test_add.dynamicslice";
        let mut source = MovedSlices {
            slices: level(),
            catalog: HashMap::from([([4; ASSET_ID_LEN], CREATURE.to_owned())]),
        };
        let levels = HashMap::new();
        let mut report = DungeonScriptsReport::default();
        let mut builder = EncounterBuilder::new(
            &mut source,
            MobSources {
                levels: &levels,
                prefabs: None,
            },
            &mut report,
        );
        let fresh = AssetRef {
            guid: [4; ASSET_ID_LEN],
            path: CREATURE.to_owned(),
        };
        let stale = AssetRef {
            path: STALE.to_owned(),
            ..fresh.clone()
        };
        let first = builder.target(Some(&stale), None).expect("target");
        let second = builder.target(Some(&fresh), None).expect("target");
        assert_eq!(first, Some(0));
        assert_eq!(second, first);
        assert_eq!(builder.targets[0].slices[0].path, CREATURE);
    }

    #[test]
    fn a_retail_placed_encounter_stands_at_its_retail_root() {
        const YAW_DEG: f32 = 30.0;
        let placement = RetailPlacement {
            name: "EncounterBoss",
            slice: ENCOUNTER,
            position: PLACE,
            yaw_deg: YAW_DEG,
        };
        let mut source = level();
        let levels = HashMap::from([(VITALS.to_lowercase(), LEVEL)]);
        let mobs = MobSources {
            levels: &levels,
            prefabs: None,
        };
        let mut report = DungeonScriptsReport::default();
        let encounter = retail_placed_encounter(&mut source, &placement, mobs, &[], &mut report)
            .expect("build")
            .expect("encounter");
        assert_eq!(encounter.name, placement.name);
        assert_eq!(encounter.slice, ENCOUNTER);
        for (cell, retail) in encounter.position.iter().zip(PLACE) {
            assert!((cell - retail).abs() < 1e-3, "{:?}", encounter.position);
        }
        assert!(
            (encounter.yaw_deg - YAW_DEG).abs() < 1e-3,
            "{}",
            encounter.yaw_deg
        );
        assert_eq!(encounter.stages[0].name, "Wave_01");
        let missing = RetailPlacement {
            slice: "slices/worldevents/none.dynamicslice",
            ..placement
        };
        assert!(
            retail_placed_encounter(&mut source, &missing, mobs, &[], &mut report)
                .expect("build")
                .is_none()
        );
        let chest = RetailPlacement {
            slice: CHEST,
            ..placement
        };
        assert!(
            retail_placed_encounter(&mut source, &chest, mobs, &[], &mut report)
                .expect("build")
                .is_none(),
            "a slice with no encounter manager"
        );
    }

    #[test]
    fn a_game_mode_slice_path_is_a_pak_path() {
        assert_eq!(
            script_slice_path("Dungeon/LevelScripts/AmrineDungeon"),
            "slices/dungeon/levelscripts/amrinedungeon.dynamicslice"
        );
        assert!(is_capitals_of(
            "sharedassets/coatlicue/nw_dungeon_windsward_00/regions/r_+00_+00/capitals/dungeon_script/dungeon_script.capitals.json",
            "nw_dungeon_windsward_00"
        ));
        assert!(!is_capitals_of(
            "sharedassets/coatlicue/other/x.capitals.json",
            "nw_dungeon"
        ));
        assert!(is_creature(
            "slices/characters/undead_brute_boss.dynamicslice"
        ));
        assert!(!is_creature(
            "slices/characters/spellslices/typhon/wall.dynamicslice"
        ));
        assert!(!is_creature(
            "slices/dungeon/raids/typhon_pillar.dynamicslice"
        ));
    }

    #[test]
    fn a_breach_slice_names_its_difficulty_at_the_end() {
        assert_eq!(
            slice_difficulty("slices/characters/corruption_swarmer_dt3.dynamicslice"),
            Some(3)
        );
        assert_eq!(
            slice_difficulty(
                "slices/darkness/events/general/hazards/darkness_hazard_corruption_minor_l2.dynamicslice"
            ),
            Some(2)
        );
        assert_eq!(
            slice_difficulty(
                "slices/darkness/events/general/darkness_ai_swarmer_wander_00.dynamicslice"
            ),
            None
        );
        assert!(is_breach_group(
            "coatgen/16c655c7/darkness_spawngroup_minor_master_3023200876.dynamicslice"
        ));
        assert!(!is_breach_group(
            "coatgen/efc337e6/worldevent_spawngroup_single_master_3815808514.dynamicslice"
        ));
    }

    #[test]
    fn a_missing_difficulty_takes_its_sibling_s_row_with_its_mark() {
        let levels = HashMap::from([
            ("corruption_impaler_dt2".to_owned(), 30),
            ("corruption_impaler_dt5".to_owned(), 60),
        ]);
        let siblings = [(2, "Corruption_Impaler_DT2".to_owned())];
        let mob = sibling_mob(&siblings, 5, &levels).expect("row");
        assert_eq!(
            (mob.vitals_id.as_str(), mob.level),
            ("Corruption_Impaler_DT5", 60)
        );
        assert!(sibling_mob(&siblings, 6, &levels).is_none(), "no such row");
        let other = [(6, "Corruption_Swarmer_07SM06_DARKNESS".to_owned())];
        assert!(
            sibling_mob(&other, 5, &levels).is_none(),
            "a row without the mark"
        );
    }

    #[test]
    fn a_breach_takes_its_place_s_map_difficulty_else_its_group_s_or_the_nearest() {
        const GROUP_A: &str = "a#1";
        const GROUP_B: &str = "b#1";
        let at = |x: f32, group: &str| Encounter {
            position: [x, 0.0, 0.0],
            spawn_group: Some(group.to_owned()),
            ..Encounter::default()
        };
        let mut encounters = vec![
            at(0.0, GROUP_A),
            at(100.0, GROUP_A),
            at(1_000.0, GROUP_B),
            at(5.0, ""),
        ];
        encounters[3].spawn_group = None;
        let tiers = [BreachTier {
            x: 3.0,
            y: 4.0,
            difficulty: 3,
            ledgers: 2,
        }];
        let mut report = DungeonScriptsReport::default();
        finish_grouped_encounters(&mut encounters, &tiers, &HashMap::new(), &mut report);
        let difficulties: Vec<Option<u32>> = encounters
            .iter()
            .map(|encounter| encounter.difficulty)
            .collect();
        assert_eq!(
            difficulties,
            [Some(3), Some(3), Some(3), None],
            "the map point, its group, the nearest group; none for an encounter of no group"
        );
        assert_eq!(
            (report.observed_difficulties, report.inferred_difficulties),
            (1, 2)
        );
    }
}
