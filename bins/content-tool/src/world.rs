// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool world` (M1b spec sections 3.4 and 6): builds one world file per level, reloads
//! each file, and counts spawn IDs that no datasheet row has.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use nw_content::gathering::InstancedObject;
use nw_content::ids::{gatherable_ids, lore_ids, npc_ids, vitals_ids};
use nw_content::{ContainerError, ContentBundle};
use nw_formats::asset_catalog::{
    ASSET_CATALOG_ENTRY, AssetCatalogError, DYNAMIC_SLICE_EXTENSION, paths_of,
};
use nw_formats::azcs::{AzcsError, decode_azcs, is_azcs};
use nw_formats::heightmap::{HeightmapError, parse_heightmap};
use nw_formats::objectstream::{Element, ObjectStreamError, lowercase_crc32, parse_object_stream};
use nw_world::capitals::parse_capitals_and_cookies;
use nw_world::height::{VERIFIED_MOUNTAIN_HEIGHTS, rescaled_sample};
use nw_world::instanced_loot::{InstancedGather, instanced_object};
use nw_world::interact_tags::{InteractTagged, TagOwner, interact_tags_of};
use nw_world::metadata::{MetadataError, MetadataRecords, SkipCounts, read_metadata};
use nw_world::records::{
    HeightGrid, LevelInfo, PatrolPath, RecordCounts, Region, SpawnAi, WorldFile,
};
use nw_world::slice_data::{SliceDataError, SliceSpawners, SpawnedSlice};
use nw_world::spawner_slices::SpawnerSlice;
use nw_world::stations::{CraftingStation, StationSlice};
use nw_world::time_of_day::{
    TimeOfDayPoints, TimedHolder, TimedSlice, TimedSlicePath, points_under,
};
use nw_world::travel_slices::{HousingPlot, TeleporterSlice, plot_house_type};
use nw_world::world_file::world_file_path;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::collision::{AssetKey, CHUNKS_EXTENSION, capital_asset, chunk_lists, chunk_placements};
use crate::error::ToolError;
use crate::source::GameSource;

/// Folder of the level data inside the paks.
pub const LEVELS_PREFIX: &str = "sharedassets/coatlicue/";
const TRACTS_FILE: &str = "tracts.json";
const TERRAIN_FILE: &str = "terrain.json";
const REGIONS_DIR: &str = "regions/";
const HEIGHTMAP_FILE: &str = "region.heightmap";
const REGION_METADATA_FILE: &str = "region.metadata";
const METADATA_EXTENSION: &str = ".metadata";
const CAPITALS_EXTENSION: &str = ".capitals.json";
/// Each region's list of the slices that slices spawn (GDE id note).
const SLICE_DATA_FILE: &str = "region.slicedata";
const REGION_FOLDER_PREFIX: &str = "r_+";
const REGION_FOLDER_SEPARATOR: &str = "_+";
const PATH_SEPARATOR: char = '/';

/// The files of one level, by path relative to the level folder.
pub type LevelFiles = BTreeMap<String, Vec<u8>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tracts {
    region_size: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Terrain {
    ocean_level: f32,
    mountain_height: f32,
}

/// Error from building one world file. Paths are pak entry paths.
#[derive(Debug, thiserror::Error)]
pub enum WorldBuildError {
    /// The asset catalog cannot be read.
    #[error("asset catalog: {0}")]
    Catalog(#[from] AssetCatalogError),
    /// A required level file is missing.
    #[error("level {level}: {file} is missing")]
    MissingFile {
        /// Level folder name.
        level: String,
        /// File name.
        file: &'static str,
    },
    /// A JSON file does not have the expected shape.
    #[error("{path}: {source}")]
    Json {
        /// Entry path.
        path: String,
        /// Parser error.
        #[source]
        source: serde_json::Error,
    },
    /// A level with terrain has a region size other than the heightmap size.
    #[error("level {level}: region size {size} does not match the heightmap size")]
    RegionSize {
        /// Level folder name.
        level: String,
        /// `regionSize` from `tracts.json`.
        size: u32,
    },
    /// A region folder name is not `r_+XX_+YY`.
    #[error("{path}: region folder name is not r_+XX_+YY")]
    BadRegionFolder {
        /// Entry path.
        path: String,
    },
    /// A heightmap cannot be read.
    #[error("{path}: {source}")]
    Heightmap {
        /// Entry path.
        path: String,
        /// Reader error.
        #[source]
        source: HeightmapError,
    },
    /// An AZCS wrapper cannot be decoded.
    #[error("{path}: {source}")]
    Azcs {
        /// Entry path.
        path: String,
        /// Reader error.
        #[source]
        source: AzcsError,
    },
    /// An ObjectStream cannot be read.
    #[error("{path}: {source}")]
    ObjectStream {
        /// Entry path.
        path: String,
        /// Reader error.
        #[source]
        source: ObjectStreamError,
    },
    /// A metadata record does not fit its typed reader.
    #[error("{path}: {source}")]
    Metadata {
        /// Entry path.
        path: String,
        /// Reader error.
        #[source]
        source: MetadataError,
    },
    /// A `region.slicedata` does not fit its reader.
    #[error("{path}: {source}")]
    SliceData {
        /// Entry path.
        path: String,
        /// Reader error.
        #[source]
        source: SliceDataError,
    },
    /// The world file cannot be written or read back.
    #[error("world file: {0}")]
    WorldFile(#[from] ContainerError),
    /// The size of a written world file cannot be read.
    #[error("cannot read the size of {}: {source}", .path.display())]
    Io {
        /// The world file.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The reloaded world file does not hold what was written.
    #[error("level {level}: reloaded counts {found:?} differ from built counts {expected:?}")]
    CountMismatch {
        /// Level folder name.
        level: String,
        /// Counts before writing.
        expected: Box<RecordCounts>,
        /// Counts after reload.
        found: Box<RecordCounts>,
    },
}

/// Spawn and point records whose ID is in no datasheet row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnmatchedIds {
    /// NPC points.
    pub npcs: usize,
    /// Creature spawn points.
    pub vitals: usize,
    /// Gatherable points.
    pub gatherables: usize,
    /// Lore points.
    pub lore: usize,
}

impl std::ops::AddAssign for UnmatchedIds {
    fn add_assign(&mut self, other: Self) {
        self.npcs += other.npcs;
        self.vitals += other.vitals;
        self.gatherables += other.gatherables;
        self.lore += other.lore;
    }
}

/// The IDs of the content bundle, as crc32 of the lower-case id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownIds {
    npcs: HashSet<u32>,
    vitals: HashSet<u32>,
    gatherables: HashSet<u32>,
    lore: HashSet<u32>,
}

impl KnownIds {
    /// Collects the ids of `NPCData`; `VitalsData`, `VitalsBaseData` and
    /// `VitalsLevelVariantData`; `GatherableData`; and `LoreData`.
    pub fn from_bundle(bundle: &ContentBundle) -> Self {
        let tables = &bundle.tables;
        Self {
            npcs: crc_set(npc_ids(tables)),
            vitals: crc_set(vitals_ids(tables)),
            gatherables: crc_set(gatherable_ids(tables)),
            lore: crc_set(lore_ids(tables)),
        }
    }

    /// Counts the records of `world` whose ID is not known.
    pub fn unmatched(&self, world: &WorldFile) -> UnmatchedIds {
        UnmatchedIds {
            npcs: count_unknown(&self.npcs, world.npcs.iter().map(|point| point.npc_id)),
            vitals: count_unknown(
                &self.vitals,
                world.vitals.iter().map(|spawn| spawn.vitals_id),
            ),
            gatherables: count_unknown(
                &self.gatherables,
                world.gatherables.iter().map(|point| point.gatherable_id),
            ),
            lore: count_unknown(&self.lore, world.lore.iter().map(|point| point.lore_id)),
        }
    }
}

fn crc_set<'a>(ids: impl Iterator<Item = &'a str>) -> HashSet<u32> {
    ids.map(lowercase_crc32).collect()
}

fn count_unknown(known: &HashSet<u32>, ids: impl Iterator<Item = u32>) -> usize {
    ids.filter(|id| !known.contains(id)).count()
}

/// Result of one level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelReport {
    /// Level folder name.
    pub level: String,
    /// The world file.
    pub path: PathBuf,
    /// Size of the world file in bytes.
    pub file_bytes: u64,
    /// Record counts after reload.
    pub counts: RecordCounts,
    /// What the metadata readers skipped.
    pub skipped: SkipCounts,
    /// Records whose ID is in no datasheet row.
    pub unmatched: UnmatchedIds,
}

/// Sums over every level of a `world` run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorldTotals {
    /// Levels built.
    pub levels: usize,
    /// Size of all world files in bytes.
    pub file_bytes: u64,
    /// Record counts of all levels.
    pub counts: RecordCounts,
    /// What the metadata readers skipped in all levels.
    pub skipped: SkipCounts,
    /// Unmatched IDs of all levels.
    pub unmatched: UnmatchedIds,
}

/// Adds up the reports of a `world` run.
pub fn world_totals(reports: &[LevelReport]) -> WorldTotals {
    let mut totals = WorldTotals {
        levels: reports.len(),
        ..WorldTotals::default()
    };
    for report in reports {
        totals.file_bytes += report.file_bytes;
        totals.counts += report.counts;
        totals.skipped.merge(&report.skipped);
        totals.unmatched += report.unmatched;
    }
    totals
}

/// True for the pak entries that `world` reads.
pub fn is_world_input(path: &str) -> bool {
    path.starts_with(LEVELS_PREFIX)
        && (path.ends_with(TRACTS_FILE)
            || path.ends_with(TERRAIN_FILE)
            || path.ends_with(HEIGHTMAP_FILE)
            || path.ends_with(METADATA_EXTENSION)
            || path.ends_with(CAPITALS_EXTENSION)
            || path.ends_with(CHUNKS_EXTENSION)
            || path.ends_with(SLICE_DATA_FILE))
}

/// Groups entries by level folder. Only folders with a `tracts.json` are levels.
pub fn group_levels(entries: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, LevelFiles> {
    let mut levels: BTreeMap<String, LevelFiles> = BTreeMap::new();
    for (path, bytes) in entries {
        let Some((level, relative)) = path
            .strip_prefix(LEVELS_PREFIX)
            .and_then(|rest| rest.split_once(PATH_SEPARATOR))
        else {
            continue;
        };
        levels
            .entry(level.to_owned())
            .or_default()
            .insert(relative.to_owned(), bytes);
    }
    levels.retain(|_, files| files.contains_key(TRACTS_FILE));
    levels
}

/// Parses a region folder name `r_+XX_+YY` into (x, y).
pub fn parse_region_folder(folder: &str) -> Option<(u32, u32)> {
    let (x, y) = folder
        .strip_prefix(REGION_FOLDER_PREFIX)?
        .split_once(REGION_FOLDER_SEPARATOR)?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

/// Builds the world of one level from its files (M1b spec section 6). The capitals' terrain
/// cookies are put into the region heights ([`WorldFile::stamp_cookies`]).
pub fn build_level(
    level: &str,
    files: &LevelFiles,
) -> Result<(WorldFile, SkipCounts), WorldBuildError> {
    let tracts: Tracts = parse_json(level, files, TRACTS_FILE)?;
    let terrain: Terrain = parse_json(level, files, TERRAIN_FILE)?;
    let has_terrain = VERIFIED_MOUNTAIN_HEIGHTS.contains(&terrain.mountain_height);
    let mut regions = Vec::new();
    for (path, bytes) in files {
        let Some(folder) = path
            .strip_prefix(REGIONS_DIR)
            .and_then(|rest| rest.strip_suffix(HEIGHTMAP_FILE))
            .and_then(|rest| rest.strip_suffix(PATH_SEPARATOR))
        else {
            continue;
        };
        let (x, y) =
            parse_region_folder(folder).ok_or_else(|| WorldBuildError::BadRegionFolder {
                path: entry_path(level, path),
            })?;
        let heights = if has_terrain {
            let map = parse_heightmap(bytes).map_err(|source| WorldBuildError::Heightmap {
                path: entry_path(level, path),
                source,
            })?;
            if map.size != tracts.region_size as usize {
                return Err(WorldBuildError::RegionSize {
                    level: level.to_owned(),
                    size: tracts.region_size,
                });
            }
            let samples = map
                .samples
                .into_iter()
                .map(|raw| rescaled_sample(raw, terrain.mountain_height))
                .collect();
            Some(HeightGrid { samples })
        } else {
            None
        };
        regions.push(Region { x, y, heights });
    }
    regions.sort_by_key(|region| (region.y, region.x));
    let mut records = MetadataRecords::default();
    let mut skipped = SkipCounts::default();
    for path in metadata_paths(files) {
        read_metadata_file(level, path, &files[path], &mut records, &mut skipped)?;
    }
    let mut capitals = Vec::new();
    let mut cookies = Vec::new();
    for (path, bytes) in files
        .iter()
        .filter(|(path, _)| path.ends_with(CAPITALS_EXTENSION))
    {
        let read = parse_capitals_and_cookies(bytes).map_err(|source| WorldBuildError::Json {
            path: entry_path(level, path),
            source,
        })?;
        capitals.extend(read.capitals);
        cookies.extend(read.cookies);
    }
    let mut spawners = SliceSpawners::default();
    for (path, bytes) in files
        .iter()
        .filter(|(path, _)| path.ends_with(SLICE_DATA_FILE))
    {
        let root = object_stream(level, path, bytes)?;
        spawners
            .add_region(&root)
            .map_err(|source| WorldBuildError::SliceData {
                path: entry_path(level, path),
                source,
            })?;
    }
    let spawned = spawners.spawned_by(&capitals);
    let mut world = WorldFile {
        level: LevelInfo {
            name: level.to_owned(),
            region_size: tracts.region_size,
            grid_width: regions.iter().map(|region| region.x + 1).max().unwrap_or(0),
            grid_height: regions.iter().map(|region| region.y + 1).max().unwrap_or(0),
            ocean_level: terrain.ocean_level,
            has_terrain,
        },
        regions,
        npcs: records.npcs,
        vitals: records.vitals,
        gatherables: records.gatherables,
        lore: records.lore,
        landmarks: records.landmarks,
        encounters: records.encounters,
        capitals,
        spawned,
        ..WorldFile::default()
    };
    world.stamp_cookies(&cookies);
    world.fill_point_keys();
    Ok((world, skipped))
}

/// Builds, writes and reloads the world file of every level in `source`, and checks the spawn
/// IDs against the content bundle at `bundle_path`. The crafting stations and house plots among
/// each level's spawned slices come from the slices' `.dynamicslice` files, and the teleporters
/// among its capitals from the capitals' slices, found by their `sliceAssetId` in the asset
/// catalog. The time of day of its spawns and gatherable points comes from the slices that its
/// capitals and chunk lists place ([`time_of_day_points`]).
pub fn run_world(
    source: &GameSource,
    client_build: &str,
    bundle_path: &Path,
    out_dir: &Path,
) -> Result<Vec<LevelReport>, ToolError> {
    let known = KnownIds::from_bundle(&ContentBundle::load(bundle_path, client_build)?);
    let mut entries = BTreeMap::new();
    source.for_each_entry(is_world_input, |path, bytes| {
        entries.insert(path.to_owned(), bytes);
        Ok::<(), ToolError>(())
    })?;
    let levels: Vec<(String, LevelFiles)> = group_levels(entries).into_iter().collect();
    let mut built = in_parallel(&levels, |(level, files)| build_level(level, files))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let slice_files: HashSet<String> = built
        .iter()
        .flat_map(|(world, _)| &world.spawned)
        .map(|spawned| slice_file(&spawned.slice))
        .collect();
    let mut stations = HashMap::new();
    let mut plots = HashMap::new();
    let mut slice_tags = HashMap::new();
    let mut instanced_slices = HashMap::new();
    source.for_each_entry(
        |path| slice_files.contains(path),
        |path, bytes| {
            let root = stream_of(path, &bytes)?;
            let tags = interact_tags_of(&root);
            if !tags.is_empty() {
                slice_tags.insert(path.to_owned(), tags);
            }
            if let Some(object) = instanced_object(&root) {
                instanced_slices.insert(path.to_owned(), object);
            }
            if let Some(station) = StationSlice::of(&root) {
                stations.insert(path.to_owned(), station);
            }
            if let Some(house_type) = plot_house_type(&root) {
                plots.insert(path.to_owned(), house_type);
            }
            Ok::<(), ToolError>(())
        },
    )?;
    let catalog = asset_catalog(source)?;
    let CapitalSlices {
        teleporters,
        spawners,
        interact_tags: capital_tags,
        instanced: instanced_capitals,
    } = capital_slices(source, &built, catalog.as_deref())?;
    let timed = timed_slices(source)?;
    let chunks = levels
        .iter()
        .map(|(level, files)| level_chunks(level, files))
        .collect::<Result<Vec<_>, _>>()?;
    let timed_assets = timed_by_asset(catalog.as_deref(), &built, &chunks, &timed)?;
    for ((world, _), chunks) in built.iter_mut().zip(&chunks) {
        world.time_of_day = time_of_day_points(world, chunks, &timed_assets, &timed);
        add_spawner_paths(world, &spawners);
        world.stations = stations_of(&world.spawned, &stations);
        world.plots = plots_of(&world.spawned, &plots);
        world.interact_tags = interact_tags(world, &capital_tags, &slice_tags);
        world.instanced_gathers = instanced_gathers(world, &instanced_capitals, &instanced_slices);
        world.teleporters = world
            .capitals
            .iter()
            .filter_map(|capital| {
                let key = capital_asset(&capital.slice_asset_id)?;
                Some(teleporters.get(&key)?.at(capital))
            })
            .collect();
    }
    let known = &known;
    let reports = in_parallel(&built, |(world, skipped)| {
        write_level(world, skipped, client_build, out_dir, known)
    });
    Ok(reports.into_iter().collect::<Result<Vec<_>, _>>()?)
}

/// The pak path of the `.dynamicslice` of spawned slice `slice`.
fn slice_file(slice: &str) -> String {
    format!("{slice}{DYNAMIC_SLICE_EXTENSION}")
}

/// What the capitals' own slices give, by the capitals' slice asset id.
#[derive(Debug, Default)]
struct CapitalSlices {
    /// The teleporter slices.
    teleporters: HashMap<AssetKey, TeleporterSlice>,
    /// The spawner slices with waypoints.
    spawners: HashMap<AssetKey, SpawnerSlice>,
    /// The interact tags of the slices that hold any.
    interact_tags: HashMap<AssetKey, Vec<String>>,
    /// The values of each slice whose gathers are instanced ([`instanced_object`]).
    instanced: HashMap<AssetKey, InstancedObject>,
}

/// The largest difference on each axis between a spawner capital and its level vitals spawn, m
/// (Observed: 0 on the beach's graze and patrol spawners; a float's rounding).
const SPAWN_ON_CAPITAL_M: f32 = 0.01;
/// The id of the path of a spawner capital: this prefix, then the capital's GUID.
const SPAWNER_PATH_PREFIX: &str = "spawner:";

/// Gives each vitals spawn of `world` that stands on a spawner capital of `spawners` the path of
/// its waypoints.
fn add_spawner_paths(world: &mut WorldFile, spawners: &HashMap<AssetKey, SpawnerSlice>) {
    let paths: Vec<(String, [f32; 3], PatrolPath)> = world
        .capitals
        .iter()
        .filter_map(|capital| {
            let slice = spawners.get(&capital_asset(&capital.slice_asset_id)?)?;
            Some((
                format!("{SPAWNER_PATH_PREFIX}{}", capital.id),
                capital.position,
                slice.at(capital),
            ))
        })
        .collect();
    for (id, at, path) in paths {
        let Some(index) = world.vitals.iter().position(|spawn| {
            spawn
                .position
                .iter()
                .zip(at)
                .all(|(a, b)| (a - b).abs() <= SPAWN_ON_CAPITAL_M)
        }) else {
            continue;
        };
        world
            .vitals_ai
            .resize(world.vitals.len(), SpawnAi::default());
        world.vitals_ai[index].path = Some(id.clone());
        world.paths.insert(id, path);
    }
}

/// The asset catalog of `source`; `None` when the paks hold none.
fn asset_catalog(source: &GameSource) -> Result<Option<Vec<u8>>, ToolError> {
    let mut catalog = None;
    source.for_each_entry(
        |path| path == ASSET_CATALOG_ENTRY,
        |_, bytes| {
            catalog = Some(bytes);
            Ok::<(), ToolError>(())
        },
    )?;
    Ok(catalog)
}

/// The asset ids of the capitals of `built`.
fn capital_assets(built: &[(WorldFile, SkipCounts)]) -> HashSet<AssetKey> {
    built
        .iter()
        .flat_map(|(world, _)| &world.capitals)
        .filter_map(|capital| capital_asset(&capital.slice_asset_id))
        .collect()
}

/// The teleporter and spawner slices of the capitals of `built`, by the capitals' slice asset
/// id, found in the asset catalog `catalog`.
fn capital_slices(
    source: &GameSource,
    built: &[(WorldFile, SkipCounts)],
    catalog: Option<&[u8]>,
) -> Result<CapitalSlices, ToolError> {
    let Some(catalog) = catalog else {
        return Ok(CapitalSlices::default());
    };
    let paths = paths_of(catalog, &capital_assets(built)).map_err(WorldBuildError::from)?;
    let keys_by_path: HashMap<String, AssetKey> = paths
        .into_iter()
        .filter(|(_, path)| path.ends_with(DYNAMIC_SLICE_EXTENSION))
        .map(|(key, path)| (path, key))
        .collect();
    let mut slices = CapitalSlices::default();
    source.for_each_entry(
        |path| keys_by_path.contains_key(path),
        |path, bytes| {
            let root = stream_of(path, &bytes)?;
            if let Some(teleporter) = TeleporterSlice::of(&root) {
                slices.teleporters.insert(keys_by_path[path], teleporter);
            }
            if let Some(spawner) = SpawnerSlice::of(&root) {
                slices.spawners.insert(keys_by_path[path], spawner);
            }
            let tags = interact_tags_of(&root);
            if !tags.is_empty() {
                slices.interact_tags.insert(keys_by_path[path], tags);
            }
            if let Some(object) = instanced_object(&root) {
                slices.instanced.insert(keys_by_path[path], object);
            }
            Ok::<(), ToolError>(())
        },
    )?;
    Ok(slices)
}

/// Every dynamic slice of `source` with a time of day, by pak path.
fn timed_slices(source: &GameSource) -> Result<HashMap<String, TimedSlice>, ToolError> {
    let mut timed = HashMap::new();
    source.for_each_entry(
        |path| path.ends_with(DYNAMIC_SLICE_EXTENSION),
        |path, bytes| {
            let stream = unwrapped(path, &bytes)?;
            if TimedSlice::may_hold(&stream)
                && let Some(slice) = TimedSlice::of(&parsed(path, &stream)?)
            {
                timed.insert(path.to_owned(), slice);
            }
            Ok::<(), ToolError>(())
        },
    )?;
    Ok(timed)
}

/// The slices that the chunk lists of `level` (its `files`) place: asset id and world position.
fn level_chunks(
    level: &str,
    files: &LevelFiles,
) -> Result<Vec<(AssetKey, [f32; 3])>, WorldBuildError> {
    let names: Vec<&str> = files.keys().map(String::as_str).collect();
    let mut placed = Vec::new();
    for path in chunk_lists(&names) {
        placed.extend(chunk_placements(&object_stream(level, path, &files[path])?));
    }
    Ok(placed)
}

/// The slices of `timed` (by pak path) that the capitals of `built` or the chunk lists `chunks`
/// place, by asset id (the asset catalog `catalog`).
fn timed_by_asset(
    catalog: Option<&[u8]>,
    built: &[(WorldFile, SkipCounts)],
    chunks: &[Vec<(AssetKey, [f32; 3])>],
    timed: &HashMap<String, TimedSlice>,
) -> Result<HashMap<AssetKey, TimedSlice>, ToolError> {
    let Some(catalog) = catalog else {
        return Ok(HashMap::new());
    };
    let mut wanted = capital_assets(built);
    wanted.extend(chunks.iter().flatten().map(|(key, _)| *key));
    Ok(paths_of(catalog, &wanted)
        .map_err(WorldBuildError::from)?
        .into_iter()
        .filter_map(|(key, path)| Some((key, timed.get(&path)?.clone())))
        .collect())
}

/// The time of day of `world`'s vitals spawns and gatherable points: those on which an entity
/// with a time of day stands, in a slice that one of its capitals or of its chunk lists
/// (`chunks`) places (`timed_assets`, by asset id). Every slice of `timed` is listed by its path
/// too: the encounter slices that spawn groups spawn have no place of their own.
fn time_of_day_points(
    world: &WorldFile,
    chunks: &[(AssetKey, [f32; 3])],
    timed_assets: &HashMap<AssetKey, TimedSlice>,
    timed: &HashMap<String, TimedSlice>,
) -> TimeOfDayPoints {
    let mut holders: Vec<TimedHolder> = world
        .capitals
        .iter()
        .filter_map(|capital| {
            let slice = timed_assets.get(&capital_asset(&capital.slice_asset_id)?)?;
            Some(slice.at_capital(capital))
        })
        .flatten()
        .collect();
    holders.extend(
        chunks
            .iter()
            .filter_map(|(key, at)| Some(timed_assets.get(key)?.at_chunk(*at)))
            .flatten(),
    );
    let mut slices: Vec<TimedSlicePath> = timed
        .iter()
        .filter_map(|(path, slice)| {
            Some(TimedSlicePath {
                slice: path.clone(),
                window: slice.window()?,
            })
        })
        .collect();
    slices.sort_by(|a, b| a.slice.cmp(&b.slice));
    TimeOfDayPoints {
        vitals: points_under(&holders, world.vitals.iter().map(|spawn| spawn.position)),
        gatherables: points_under(
            &holders,
            world.gatherables.iter().map(|point| point.position),
        ),
        slices,
    }
}

/// The capitals (their slices' tags by asset id, `capital_tags`) and spawned slices (by pak path,
/// `slice_tags`) of `world` whose slices hold interact tags.
fn interact_tags(
    world: &WorldFile,
    capital_tags: &HashMap<AssetKey, Vec<String>>,
    slice_tags: &HashMap<String, Vec<String>>,
) -> Vec<InteractTagged> {
    let capitals = world.capitals.iter().filter_map(|capital| {
        let tags = capital_tags.get(&capital_asset(&capital.slice_asset_id)?)?;
        Some(InteractTagged {
            owner: TagOwner::Capital(capital.id.clone()),
            tags: tags.clone(),
        })
    });
    let spawned = world.spawned.iter().filter_map(|slice| {
        Some(InteractTagged {
            owner: TagOwner::Spawned(slice.id),
            tags: slice_tags.get(&slice_file(&slice.slice))?.clone(),
        })
    });
    capitals.chain(spawned).collect()
}

/// The capitals (whose slices' asset ids are in `capital_slices`) and spawned slices (whose pak
/// paths are in `spawned_slices`) of `world` whose gathers are instanced, each with its slice's
/// values.
fn instanced_gathers(
    world: &WorldFile,
    capital_slices: &HashMap<AssetKey, InstancedObject>,
    spawned_slices: &HashMap<String, InstancedObject>,
) -> Vec<InstancedGather> {
    let capitals = world.capitals.iter().filter_map(|capital| {
        let key = capital_asset(&capital.slice_asset_id)?;
        Some(InstancedGather {
            owner: TagOwner::Capital(capital.id.clone()),
            object: *capital_slices.get(&key)?,
        })
    });
    let spawned = world.spawned.iter().filter_map(|slice| {
        Some(InstancedGather {
            owner: TagOwner::Spawned(slice.id),
            object: *spawned_slices.get(&slice_file(&slice.slice))?,
        })
    });
    capitals.chain(spawned).collect()
}

/// The spawned slices whose `.dynamicslice` is a house plot of `plots` (house type by pak path).
fn plots_of(spawned: &[SpawnedSlice], plots: &HashMap<String, String>) -> Vec<HousingPlot> {
    spawned
        .iter()
        .filter_map(|slice| {
            Some(HousingPlot {
                id: slice.id,
                slice: slice.slice.clone(),
                position: slice.position,
                house_type: plots.get(&slice_file(&slice.slice))?.clone(),
            })
        })
        .collect()
}

/// The spawned slices whose `.dynamicslice` is a station of `stations` (by pak path).
fn stations_of(
    spawned: &[SpawnedSlice],
    stations: &HashMap<String, StationSlice>,
) -> Vec<CraftingStation> {
    spawned
        .iter()
        .filter_map(|slice| {
            Some(CraftingStation {
                id: slice.id,
                slice: slice.slice.clone(),
                position: slice.position,
                components: stations.get(&slice_file(&slice.slice))?.clone(),
            })
        })
        .collect()
}

/// `work` over `items` on every core, in order.
pub(crate) fn in_parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let chunk_size = items.len().div_ceil(workers).max(1);
    let work = &work;
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk_size)
            .map(|chunk| scope.spawn(move || chunk.iter().map(work).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}

fn write_level(
    world: &WorldFile,
    skipped: &SkipCounts,
    client_build: &str,
    out_dir: &Path,
    known: &KnownIds,
) -> Result<LevelReport, WorldBuildError> {
    let level = &world.level.name;
    let path = world_file_path(out_dir, client_build, level);
    world.save(&path, client_build)?;
    let expected = world.counts();
    let found = WorldFile::load(&path, client_build)?.counts();
    if found != expected {
        return Err(WorldBuildError::CountMismatch {
            level: level.clone(),
            expected: Box::new(expected),
            found: Box::new(found),
        });
    }
    let file_bytes = std::fs::metadata(&path)
        .map_err(|source| WorldBuildError::Io {
            path: path.clone(),
            source,
        })?
        .len();
    Ok(LevelReport {
        level: level.clone(),
        path,
        file_bytes,
        counts: found,
        skipped: skipped.clone(),
        unmatched: known.unmatched(world),
    })
}

/// The metadata files to read: per region folder, `region.metadata` when it exists (it is the
/// union of the capital files), else every `.metadata` file of the folder.
fn metadata_paths(files: &LevelFiles) -> Vec<&str> {
    let mut by_region: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for path in files
        .keys()
        .filter(|path| path.ends_with(METADATA_EXTENSION))
    {
        let region = path
            .strip_prefix(REGIONS_DIR)
            .and_then(|rest| rest.split_once(PATH_SEPARATOR))
            .map_or("", |(folder, _)| folder);
        by_region.entry(region).or_default().push(path);
    }
    by_region
        .into_iter()
        .flat_map(|(region, paths)| {
            let aggregate = format!("{REGIONS_DIR}{region}{PATH_SEPARATOR}{REGION_METADATA_FILE}");
            match paths.iter().find(|path| **path == aggregate) {
                Some(path) => vec![*path],
                None => paths,
            }
        })
        .collect()
}

fn read_metadata_file(
    level: &str,
    path: &str,
    bytes: &[u8],
    records: &mut MetadataRecords,
    skipped: &mut SkipCounts,
) -> Result<(), WorldBuildError> {
    let root = object_stream(level, path, bytes)?;
    read_metadata(&root, records, skipped).map_err(|source| WorldBuildError::Metadata {
        path: entry_path(level, path),
        source,
    })
}

/// The root element of the ObjectStream `bytes` (AZCS-wrapped or not) of level file `path`.
fn object_stream(level: &str, path: &str, bytes: &[u8]) -> Result<Element, WorldBuildError> {
    stream_of(&entry_path(level, path), bytes)
}

/// The root element of the ObjectStream `bytes` (AZCS-wrapped or not) of pak entry `path`.
pub(crate) fn stream_of(path: &str, bytes: &[u8]) -> Result<Element, WorldBuildError> {
    parsed(path, &unwrapped(path, bytes)?)
}

/// The ObjectStream of pak entry `path`: `bytes` without their AZCS wrapper, when they have one.
fn unwrapped<'a>(path: &str, bytes: &'a [u8]) -> Result<Cow<'a, [u8]>, WorldBuildError> {
    if is_azcs(bytes) {
        Ok(Cow::Owned(decode_azcs(bytes).map_err(|source| {
            WorldBuildError::Azcs {
                path: path.to_owned(),
                source,
            }
        })?))
    } else {
        Ok(Cow::Borrowed(bytes))
    }
}

/// The root element of the ObjectStream `stream` of pak entry `path`.
fn parsed(path: &str, stream: &[u8]) -> Result<Element, WorldBuildError> {
    parse_object_stream(stream).map_err(|source| WorldBuildError::ObjectStream {
        path: path.to_owned(),
        source,
    })
}

fn parse_json<T: DeserializeOwned>(
    level: &str,
    files: &LevelFiles,
    file: &'static str,
) -> Result<T, WorldBuildError> {
    let bytes = files
        .get(file)
        .ok_or_else(|| WorldBuildError::MissingFile {
            level: level.to_owned(),
            file,
        })?;
    serde_json::from_slice(bytes).map_err(|source| WorldBuildError::Json {
        path: entry_path(level, file),
        source,
    })
}

/// The pak entry path of a level file.
fn entry_path(level: &str, relative: &str) -> String {
    format!("{LEVELS_PREFIX}{level}{PATH_SEPARATOR}{relative}")
}

#[cfg(test)]
mod tests {
    use nw_formats::heightmap::{
        HEIGHTMAP_SIZE, Heightmap, SMALL_HEIGHTMAP_SIZE, encode_heightmap,
    };
    use nw_formats::objectstream::{Element, write_object_stream};
    use nw_world::height::HEIGHT_UNITS_PER_METRE;
    use nw_world::instanced_loot::POI_LOOT;
    use nw_world::metadata::*;

    use super::*;

    const LEVEL: &str = "testlevel";
    const TRACTS: &[u8] = br#"{"regionSize": 2048, "tracts": []}"#;
    const SMALL_TRACTS: &[u8] = br#"{"regionSize": 1024}"#;
    const TERRAIN: &[u8] = br#"{"oceanLevel": 40.0, "mountainHeight": 2048.0}"#;
    const FLAT_TERRAIN: &[u8] = br#"{"oceanLevel": -1000.0, "mountainHeight": 256.0}"#;
    /// The FTUE levels' terrain: raw samples span 1024 m.
    const FTUE_TERRAIN: &[u8] = br#"{"oceanLevel": 2.0, "mountainHeight": 1024.0}"#;
    const CAPITALS: &[u8] = br#"{"Capitals": [{"id": "a",
        "worldPosition": {"x": 2050, "y": 10, "z": 3}, "rotation": {"x": 0, "y": 0, "z": 0, "w": 1},
        "sliceName": "<PLOT>", "sliceAssetId": "b"}]}"#;
    const REGION_HEIGHTMAP: &str = "regions/r_+01_+00/region.heightmap";
    const REGION_METADATA: &str = "regions/r_+01_+00/region.metadata";
    const CAPITAL_METADATA: &str = "regions/r_+01_+00/capitals/ai/ai.metadata";
    const OTHER_CAPITAL_METADATA: &str = "regions/r_+01_+00/capitals/lore/lore.metadata";
    const CAPITALS_FILE: &str = "regions/r_+01_+00/capitals/ai/ai.capitals.json";
    const REGION_SLICE_DATA: &str = "regions/r_+01_+00/region.slicedata";
    /// The slice the capital of `CAPITALS` spawns, one metre east of it.
    const SPAWNED_SLICE: &str = "slices/child";
    const VITALS_ID: u32 = 0xae65_fdc7;
    const RAW_HEIGHT: u16 = 64;
    /// `regionSize` in `SMALL_TRACTS`.
    const SMALL_REGION_SIZE: u32 = 1024;
    const POSITION_BYTES: usize = 3 * size_of::<f32>();

    /// A metadata stream with one vitals record.
    fn vitals_metadata() -> Vec<u8> {
        let crc = |field, value: u32| {
            Element::with_children(
                field,
                vec![Element::with_value(
                    FIELD_CRC_VALUE,
                    value.to_be_bytes().to_vec(),
                )],
            )
        };
        let record = Element {
            children: vec![
                Element::with_children(FIELD_TERRITORY_IDS, Vec::new()),
                crc(FIELD_VITALS_ID, VITALS_ID),
                crc(FIELD_VITALS_CATEGORY_ID, 0),
                Element::with_value(FIELD_VITALS_LEVEL, 1_u32.to_be_bytes().to_vec()),
                Element::with_value(FIELD_WORLD_POSITION, vec![0; POSITION_BYTES]),
                Element::with_value(FIELD_IS_ENCOUNTER, vec![0]),
            ],
            ..Element::default()
        };
        let root = Element {
            children: vec![Element::with_children(SECTION_VITALS, vec![record])],
            ..Element::default()
        };
        write_object_stream(&root).unwrap()
    }

    fn flat_heightmap_of(size: usize) -> Vec<u8> {
        encode_heightmap(&Heightmap {
            size,
            samples: vec![RAW_HEIGHT; size * size],
        })
        .unwrap()
    }

    fn flat_heightmap() -> Vec<u8> {
        flat_heightmap_of(HEIGHTMAP_SIZE)
    }

    fn level_files(entries: &[(&str, Vec<u8>)]) -> LevelFiles {
        entries
            .iter()
            .map(|(path, bytes)| ((*path).to_owned(), bytes.clone()))
            .collect()
    }

    fn terrain_level() -> LevelFiles {
        level_files(&[
            (TRACTS_FILE, TRACTS.to_vec()),
            (TERRAIN_FILE, TERRAIN.to_vec()),
            (REGION_HEIGHTMAP, flat_heightmap()),
            (REGION_METADATA, vitals_metadata()),
            (CAPITAL_METADATA, vitals_metadata()),
            (CAPITALS_FILE, CAPITALS.to_vec()),
        ])
    }

    #[test]
    fn region_folder_names_parse() {
        assert_eq!(parse_region_folder("r_+04_+06"), Some((4, 6)));
        assert_eq!(parse_region_folder("r_04_06"), None);
        assert_eq!(parse_region_folder("r_+4x_+0"), None);
    }

    #[test]
    fn world_inputs_are_selected_by_name() {
        let path = |relative: &str| format!("{LEVELS_PREFIX}{LEVEL}/{relative}");
        assert!(is_world_input(&path(TRACTS_FILE)));
        assert!(is_world_input(&path(REGION_HEIGHTMAP)));
        assert!(is_world_input(&path(CAPITAL_METADATA)));
        assert!(is_world_input(&path(CAPITALS_FILE)));
        assert!(is_world_input(&path(REGION_SLICE_DATA)));
        assert!(!is_world_input(&path(
            "regions/r_+01_+00/region.vegetation"
        )));
        assert!(!is_world_input("slices/x/terrain.json"));
    }

    /// A `region.slicedata` in which the capital's `<PLOT>` slice spawns [`SPAWNED_SLICE`] one
    /// metre east of it, or (`broken`) with a transform cut short.
    fn slice_data(broken: bool) -> Vec<u8> {
        let named = |name: &str, children: Vec<Element>| {
            Element::with_children(lowercase_crc32(name), children)
        };
        let value = |name: &str, bytes: Vec<u8>| Element::with_value(lowercase_crc32(name), bytes);
        let mut floats = vec![
            1.0_f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0,
        ];
        if broken {
            floats.pop();
        }
        let transform = floats
            .iter()
            .flat_map(|float| float.to_be_bytes())
            .collect();
        let spawner = named(
            "element",
            vec![
                value("worldTM", transform),
                value("sliceName", SPAWNED_SLICE.as_bytes().to_vec()),
            ],
        );
        let pair = named(
            "element",
            vec![
                named("value1", vec![value("SliceName", b"<PLOT>".to_vec())]),
                named("value2", vec![named("spawners", vec![spawner])]),
            ],
        );
        write_object_stream(&Element {
            children: vec![named("sliceMetaDataMap", vec![pair])],
            ..Element::default()
        })
        .unwrap()
    }

    #[test]
    fn a_spawn_on_a_spawner_capital_walks_the_spawners_waypoints() {
        const GUID: &str = "64f2702e-9c3b-a5a5-1dae-1b0d80c5666b";
        let named = |name: &str, value: Vec<u8>| {
            Element::with_value(nw_formats::objectstream::lowercase_crc32(name), value)
        };
        let mut local = [0.0_f32; 12];
        local[0] = 1.0;
        local[4] = 1.0;
        local[8] = 1.0;
        local[9] = 2.0;
        let waypoint = Element {
            type_id: 0x7565_1658_8663_478d_9090_2432_dfca_fa44_u128.to_be_bytes(),
            ..Element::with_children(
                0,
                vec![
                    Element::with_children(
                        nw_formats::objectstream::lowercase_crc32("Id"),
                        vec![named("Id", 1_u64.to_be_bytes().to_vec())],
                    ),
                    named("Name", b"wp1".to_vec()),
                    Element::with_children(
                        0,
                        vec![named(
                            "m_localTM",
                            local.iter().flat_map(|value| value.to_be_bytes()).collect(),
                        )],
                    ),
                    Element::with_children(
                        0,
                        vec![
                            named("m_taskName", b"POI_Graze_LOOP".to_vec()),
                            named("m_minDuration", (-1.0_f32).to_be_bytes().to_vec()),
                            named("m_maxDuration", (-1.0_f32).to_be_bytes().to_vec()),
                        ],
                    ),
                ],
            )
        };
        let slice = SpawnerSlice::of(&Element::with_children(0, vec![waypoint])).expect("slice");
        let asset = "{3D51B368-7E53-5FF9-AF51-58FD8602F34B}:2";
        let at = [7305.945, 3117.7996, 44.19863];
        let mut world = WorldFile {
            capitals: vec![nw_world::records::Capital {
                id: GUID.to_owned(),
                position: at,
                rotation: [0.0, 0.0, 0.0, 1.0],
                slice_asset_id: asset.to_owned(),
                ..nw_world::records::Capital::default()
            }],
            vitals: vec![
                nw_world::records::VitalsSpawn::default(),
                nw_world::records::VitalsSpawn {
                    position: at,
                    ..nw_world::records::VitalsSpawn::default()
                },
            ],
            ..WorldFile::default()
        };
        let spawners = HashMap::from([(capital_asset(asset).expect("asset"), slice)]);
        add_spawner_paths(&mut world, &spawners);
        let id = format!("{SPAWNER_PATH_PREFIX}{GUID}");
        assert_eq!(world.vitals_ai.len(), 2);
        assert_eq!(world.vitals_ai[0], SpawnAi::default());
        assert_eq!(world.vitals_ai[1].path.as_deref(), Some(id.as_str()));
        let path = &world.paths[&id];
        assert_eq!(path.points, [[7307.945, 3117.7996, 44.19863]]);
        assert_eq!(
            path.tasks[0].as_ref().map(|task| task.task.as_str()),
            Some("POI_Graze_LOOP")
        );
    }

    /// A slice of one entity at `at` in its frame that holds a night-only time of day, as the
    /// retail Dunwood dye plant chunks hold it (Observed, `17dw_gatherables_32x32_static_chunk_*`).
    fn night_only_slice(at: [f32; 3]) -> TimedSlice {
        let named = |name: &str, value: Vec<u8>| {
            Element::with_value(nw_formats::objectstream::lowercase_crc32(name), value)
        };
        let mut local = [0.0_f32; 12];
        local[0] = 1.0;
        local[4] = 1.0;
        local[8] = 1.0;
        local[9..].copy_from_slice(&at);
        let flags = ["m_night", "m_dawn", "m_day", "m_dusk", "m_despawnOnTimeEnd"]
            .into_iter()
            .map(|name| {
                named(
                    name,
                    vec![u8::from(
                        name != "m_dawn" && name != "m_day" && name != "m_dusk",
                    )],
                )
            })
            .collect();
        let holder = Element {
            type_id: 0x7565_1658_8663_478d_9090_2432_dfca_fa44_u128.to_be_bytes(),
            ..Element::with_children(
                0,
                vec![
                    Element::with_children(
                        nw_formats::objectstream::lowercase_crc32("Id"),
                        vec![named("Id", 1_u64.to_be_bytes().to_vec())],
                    ),
                    Element::with_children(
                        0,
                        vec![named(
                            "m_localTM",
                            local.iter().flat_map(|value| value.to_be_bytes()).collect(),
                        )],
                    ),
                    Element {
                        type_id: nw_world::time_of_day::TIME_OF_DAY_CONSTRAINT,
                        ..Element::with_children(0, flags)
                    },
                ],
            )
        };
        TimedSlice::of(&Element::with_children(0, vec![holder])).expect("a holder")
    }

    #[test]
    fn spawns_and_gatherables_under_a_timed_entity_take_its_time_of_day() {
        const CAPITAL_ASSET: &str = "{3D51B368-7E53-5FF9-AF51-58FD8602F34B}:2";
        const CHUNK_PATH: &str = "coatgen/x/17dw_gatherables_32x32_static_chunk_1_1_1.dynamicslice";
        let capital_at = [100.0, 200.0, 30.0];
        let chunk_at = [400.0, 500.0, 60.0];
        let chunk_key = capital_asset("{11111111-2222-3333-4444-555555555555}:1").expect("key");
        let world = WorldFile {
            capitals: vec![nw_world::records::Capital {
                position: capital_at,
                rotation: [0.0, 0.0, 0.0, 1.0],
                slice_asset_id: CAPITAL_ASSET.to_owned(),
                ..nw_world::records::Capital::default()
            }],
            vitals: vec![
                nw_world::records::VitalsSpawn::default(),
                nw_world::records::VitalsSpawn {
                    position: capital_at,
                    ..nw_world::records::VitalsSpawn::default()
                },
            ],
            gatherables: vec![nw_world::records::GatherablePoint {
                position: [400.0 - 13.88, 501.47, 60.0],
                ..nw_world::records::GatherablePoint::default()
            }],
            ..WorldFile::default()
        };
        let spawner = night_only_slice([0.0; 3]);
        let chunk = night_only_slice([-13.88, 1.47, 0.0]);
        let window = chunk.window().expect("a window");
        let timed_assets = HashMap::from([
            (capital_asset(CAPITAL_ASSET).expect("key"), spawner),
            (chunk_key, chunk.clone()),
        ]);
        let timed = HashMap::from([(CHUNK_PATH.to_owned(), chunk)]);
        let points = time_of_day_points(&world, &[(chunk_key, chunk_at)], &timed_assets, &timed);
        assert!(window.night && !window.day && window.despawn_on_end);
        assert_eq!(
            points.vitals,
            [nw_world::time_of_day::TimedPoint { index: 1, window }]
        );
        assert_eq!(
            points.gatherables,
            [nw_world::time_of_day::TimedPoint { index: 0, window }]
        );
        assert_eq!(
            points.slices,
            [TimedSlicePath {
                slice: CHUNK_PATH.to_owned(),
                window
            }]
        );
    }

    #[test]
    fn capitals_and_spawned_slices_of_instanced_slices_have_instanced_gathers() {
        const CHEST_ASSET: &str = "{3D51B368-7E53-5FF9-AF51-58FD8602F34B}:2";
        const LOCK_ASSET: &str = "{11111111-2222-3333-4444-555555555555}:1";
        const CHEST_SLICE: &str = "slices/master_lootcontainer";
        const LOCK_SLICE: &str = "slices/master_lootcontainer_pvp";
        const SPAWNED_CHEST: u64 = 0x1234_5678_9abc_def0;
        /// `master_lootcontainer`: `InstancedLootType::POILoot`, `m_aoiRadius` 80 m.
        const CHEST: InstancedObject = InstancedObject {
            loot_type: POI_LOOT,
            aoi_radius_m: Some(80.0),
        };
        /// `master_quest_instanced`: `InstancedLootType::Quest`, `m_aoiRadius` 100 m.
        const QUEST: InstancedObject = InstancedObject {
            loot_type: 4,
            aoi_radius_m: Some(100.0),
        };
        let capital = |id: &str, asset: &str| nw_world::records::Capital {
            id: id.to_owned(),
            slice_asset_id: asset.to_owned(),
            ..nw_world::records::Capital::default()
        };
        let spawned = |id: u64, slice: &str| SpawnedSlice {
            id,
            slice: slice.to_owned(),
            variant: String::new(),
            position: [0.0; 3],
        };
        let world = WorldFile {
            capitals: vec![capital("chest", CHEST_ASSET), capital("lock", LOCK_ASSET)],
            spawned: vec![spawned(SPAWNED_CHEST, CHEST_SLICE), spawned(1, LOCK_SLICE)],
            ..WorldFile::default()
        };
        let chest_key = capital_asset(CHEST_ASSET).expect("key");
        let capital_slices = HashMap::from([(chest_key, CHEST)]);
        let spawned_slices = HashMap::from([(slice_file(CHEST_SLICE), QUEST)]);
        assert_eq!(
            instanced_gathers(&world, &capital_slices, &spawned_slices),
            [
                InstancedGather {
                    owner: TagOwner::Capital("chest".to_owned()),
                    object: CHEST,
                },
                InstancedGather {
                    owner: TagOwner::Spawned(SPAWNED_CHEST),
                    object: QUEST,
                },
            ]
        );
    }

    #[test]
    fn a_regions_chunk_list_stands_for_its_capitals_lists() {
        let files = [
            "regions/r_+00_+00/region.chunks",
            "regions/r_+00_+00/capitals/a/a.chunks",
            "regions/r_+01_+00/capitals/b/b.chunks",
            "regions/r_+01_+00/capitals/b/b.capitals.json",
        ];
        assert_eq!(
            chunk_lists(&files),
            [
                "regions/r_+00_+00/region.chunks",
                "regions/r_+01_+00/capitals/b/b.chunks"
            ]
        );
        assert!(is_world_input(
            "sharedassets/coatlicue/lv/regions/r_+00_+00/region.chunks"
        ));
    }

    #[test]
    fn the_capitals_spawned_slices_are_read() {
        let mut files = terrain_level();
        files.insert(REGION_SLICE_DATA.to_owned(), slice_data(false));
        let (world, _) = build_level(LEVEL, &files).unwrap();
        assert_eq!(world.spawned.len(), 1);
        assert_eq!(world.spawned[0].slice, SPAWNED_SLICE);
        assert_eq!(world.spawned[0].position, [2051.0, 10.0, 3.0]);
        assert_eq!(world.counts().spawned, 1);
        files.insert(REGION_SLICE_DATA.to_owned(), slice_data(true));
        assert!(matches!(
            build_level(LEVEL, &files),
            Err(WorldBuildError::SliceData { path, .. })
                if path == format!("{LEVELS_PREFIX}{LEVEL}/{REGION_SLICE_DATA}")
        ));
    }

    #[test]
    fn levels_are_folders_with_tracts() {
        let entries = BTreeMap::from([
            (
                format!("{LEVELS_PREFIX}{LEVEL}/{TRACTS_FILE}"),
                TRACTS.to_vec(),
            ),
            (
                format!("{LEVELS_PREFIX}{LEVEL}/{TERRAIN_FILE}"),
                TERRAIN.to_vec(),
            ),
            (
                format!("{LEVELS_PREFIX}notalevel/{TERRAIN_FILE}"),
                TERRAIN.to_vec(),
            ),
        ]);
        let levels = group_levels(entries);
        assert_eq!(levels.keys().collect::<Vec<_>>(), [LEVEL]);
        assert_eq!(levels[LEVEL].len(), 2);
    }

    #[test]
    fn builds_a_level_with_terrain_from_region_metadata() {
        let (world, skipped) = build_level(LEVEL, &terrain_level()).unwrap();
        assert_eq!(
            world.level,
            LevelInfo {
                name: LEVEL.to_owned(),
                region_size: HEIGHTMAP_SIZE as u32,
                grid_width: 2,
                grid_height: 1,
                ocean_level: 40.0,
                has_terrain: true,
            }
        );
        assert_eq!(world.regions.len(), 1);
        assert_eq!((world.regions[0].x, world.regions[0].y), (1, 0));
        // region.metadata is read; the capital file of the same region is not.
        assert_eq!(world.vitals.len(), 1);
        assert_eq!(world.vitals[0].vitals_id, VITALS_ID);
        assert_eq!(world.capitals.len(), 1);
        assert_eq!(
            world.height_at(2050.0, 10.0),
            Some((f64::from(RAW_HEIGHT) / HEIGHT_UNITS_PER_METRE) as f32)
        );
        assert_eq!(skipped, SkipCounts::default());
    }

    #[test]
    fn a_capitals_cookie_sets_the_built_terrain_height() {
        const COOKIE_CAPITALS: &[u8] = br#"{"Capitals": [{"id": "a",
            "worldPosition": {"x": 2100, "y": 100, "z": 10}, "rotation": {"x": 0, "y": 0, "z": 0, "w": 1},
            "cookie": {"innerRadius": 5.0, "outerRadius": 20.0, "maxHeightBlend": 1.0},
            "sliceName": "<PLOT>", "sliceAssetId": "b"}]}"#;
        const COOKIE_HEIGHT: f32 = 10.0;
        let mut files = terrain_level();
        files.insert(CAPITALS_FILE.to_owned(), COOKIE_CAPITALS.to_vec());
        let (world, _) = build_level(LEVEL, &files).unwrap();
        assert_eq!(world.height_at(2100.0, 100.0), Some(COOKIE_HEIGHT));
        assert_eq!(
            world.height_at(2050.0, 10.0),
            Some((f64::from(RAW_HEIGHT) / HEIGHT_UNITS_PER_METRE) as f32)
        );
    }

    #[test]
    fn build_level_gives_every_vitals_spawn_a_key() {
        let (world, _) = build_level(LEVEL, &terrain_level()).unwrap();
        assert_eq!(world.vitals_keys.len(), world.vitals.len());
        assert!(world.vitals_keys.iter().all(|key| !key.is_empty()));
        assert_eq!(world.npc_keys.len(), world.npcs.len());
    }

    #[test]
    fn capital_metadata_is_read_without_region_metadata() {
        let mut files = terrain_level();
        files.remove(REGION_METADATA);
        files.insert(OTHER_CAPITAL_METADATA.to_owned(), vitals_metadata());
        let (world, _) = build_level(LEVEL, &files).unwrap();
        assert_eq!(world.vitals.len(), 2);
    }

    #[test]
    fn level_without_terrain_does_not_read_heightmaps() {
        let files = level_files(&[
            (TRACTS_FILE, SMALL_TRACTS.to_vec()),
            (TERRAIN_FILE, FLAT_TERRAIN.to_vec()),
            (REGION_HEIGHTMAP, b"not a heightmap".to_vec()),
        ]);
        let (world, _) = build_level(LEVEL, &files).unwrap();
        assert!(!world.level.has_terrain);
        assert_eq!(world.level.region_size, SMALL_REGION_SIZE);
        assert_eq!(world.regions[0].heights, None);
    }

    #[test]
    fn missing_terrain_file_is_an_error() {
        let files = level_files(&[(TRACTS_FILE, TRACTS.to_vec())]);
        assert!(matches!(
            build_level(LEVEL, &files),
            Err(WorldBuildError::MissingFile {
                file: TERRAIN_FILE,
                ..
            })
        ));
    }

    #[test]
    fn terrain_level_needs_the_heightmap_region_size() {
        let mut files = terrain_level();
        files.insert(TRACTS_FILE.to_owned(), SMALL_TRACTS.to_vec());
        assert!(matches!(
            build_level(LEVEL, &files),
            Err(WorldBuildError::RegionSize {
                size: SMALL_REGION_SIZE,
                ..
            })
        ));
    }

    #[test]
    fn an_ftue_level_keeps_its_small_terrain_at_the_open_world_scale() {
        let files = level_files(&[
            (TRACTS_FILE, SMALL_TRACTS.to_vec()),
            (TERRAIN_FILE, FTUE_TERRAIN.to_vec()),
            (
                "regions/r_+00_+00/region.heightmap",
                flat_heightmap_of(SMALL_HEIGHTMAP_SIZE),
            ),
        ]);
        let (world, _) = build_level(LEVEL, &files).unwrap();
        assert!(world.level.has_terrain);
        assert_eq!(world.level.region_size, SMALL_REGION_SIZE);
        // Raw 64 spans 1024 m over the full raw range: 1 m.
        assert_eq!(world.height_at(500.0, 500.0), Some(1.0));
    }

    #[test]
    fn bad_region_folder_is_an_error() {
        let mut files = terrain_level();
        files.insert("regions/r_x/region.heightmap".to_owned(), flat_heightmap());
        assert!(matches!(
            build_level(LEVEL, &files),
            Err(WorldBuildError::BadRegionFolder { path })
                if path == format!("{LEVELS_PREFIX}{LEVEL}/regions/r_x/region.heightmap")
        ));
    }

    #[test]
    fn broken_metadata_names_the_entry() {
        let mut files = terrain_level();
        files.insert(REGION_METADATA.to_owned(), vec![0]);
        assert!(matches!(
            build_level(LEVEL, &files),
            Err(WorldBuildError::ObjectStream { path, .. })
                if path == format!("{LEVELS_PREFIX}{LEVEL}/{REGION_METADATA}")
        ));
    }

    #[test]
    fn totals_sum_every_level() {
        const UNKNOWN_SECTION: u32 = 0x7cc6_6b4d;
        let report = |vitals, bytes, skipped_records| LevelReport {
            level: LEVEL.to_owned(),
            path: PathBuf::from(LEVEL),
            file_bytes: bytes,
            counts: RecordCounts {
                vitals,
                ..RecordCounts::default()
            },
            skipped: SkipCounts {
                sections: BTreeMap::from([(UNKNOWN_SECTION, skipped_records)]),
                ..SkipCounts::default()
            },
            unmatched: UnmatchedIds {
                vitals: 1,
                ..UnmatchedIds::default()
            },
        };
        let totals = world_totals(&[report(2, 10, 3), report(5, 20, 4)]);
        assert_eq!(totals.levels, 2);
        assert_eq!(totals.file_bytes, 30);
        assert_eq!(totals.counts.vitals, 7);
        assert_eq!(totals.skipped.sections[&UNKNOWN_SECTION], 7);
        assert_eq!(totals.unmatched.vitals, 2);
    }

    #[test]
    fn unmatched_counts_unknown_ids() {
        let (world, _) = build_level(LEVEL, &terrain_level()).unwrap();
        let empty = KnownIds::from_bundle(&ContentBundle::default());
        assert_eq!(empty.unmatched(&world).vitals, 1);
        let known = KnownIds {
            vitals: HashSet::from([VITALS_ID]),
            ..KnownIds::default()
        };
        assert_eq!(known.unmatched(&world), UnmatchedIds::default());
    }
}
