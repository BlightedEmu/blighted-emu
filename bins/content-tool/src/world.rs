// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool world` (M1b spec sections 3.4 and 6): builds one world file per level, reloads
//! each file, and counts spawn IDs that no datasheet row has.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use nw_content::ContentBundle;
use nw_formats::azcs::{AzcsError, decode_azcs, is_azcs};
use nw_formats::heightmap::{HEIGHTMAP_SIZE, HeightmapError, parse_heightmap};
use nw_formats::objectstream::{ObjectStreamError, lowercase_crc32, parse_object_stream};
use nw_world::WorldError;
use nw_world::capitals::parse_capitals;
use nw_world::height::TERRAIN_MOUNTAIN_HEIGHT;
use nw_world::metadata::{MetadataError, MetadataRecords, SkipCounts, read_metadata};
use nw_world::records::{HeightGrid, LevelInfo, RecordCounts, Region, WorldFile};
use nw_world::world_file::world_file_path;
use serde::Deserialize;
use serde::de::DeserializeOwned;

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
    /// The world file cannot be written or read back.
    #[error(transparent)]
    WorldFile(#[from] WorldError),
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
            npcs: crc_set(
                tables
                    .npc_data
                    .iter()
                    .flat_map(|table| &table.rows)
                    .map(|row| row.npc_id.as_str()),
            ),
            vitals: crc_set(
                tables
                    .vitals_data
                    .iter()
                    .flat_map(|table| &table.rows)
                    .map(|row| row.vitals_id.as_str())
                    .chain(
                        tables
                            .vitals_base_data
                            .iter()
                            .flat_map(|table| &table.rows)
                            .map(|row| row.vitals_id.as_str()),
                    )
                    .chain(
                        tables
                            .vitals_level_variant_data
                            .iter()
                            .flat_map(|table| &table.rows)
                            .map(|row| row.vitals_id.as_str()),
                    ),
            ),
            gatherables: crc_set(
                tables
                    .gatherable_data
                    .iter()
                    .flat_map(|table| &table.rows)
                    .map(|row| row.gatherable_id.as_str()),
            ),
            lore: crc_set(
                tables
                    .lore_data
                    .iter()
                    .flat_map(|table| &table.rows)
                    .map(|row| row.lore_id.as_str()),
            ),
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

/// True for the pak entries that `world` reads.
pub fn is_world_input(path: &str) -> bool {
    path.starts_with(LEVELS_PREFIX)
        && (path.ends_with(TRACTS_FILE)
            || path.ends_with(TERRAIN_FILE)
            || path.ends_with(HEIGHTMAP_FILE)
            || path.ends_with(METADATA_EXTENSION)
            || path.ends_with(CAPITALS_EXTENSION))
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

/// Builds the world of one level from its files (M1b spec section 6).
pub fn build_level(
    level: &str,
    files: &LevelFiles,
) -> Result<(WorldFile, SkipCounts), WorldBuildError> {
    let tracts: Tracts = parse_json(level, files, TRACTS_FILE)?;
    let terrain: Terrain = parse_json(level, files, TERRAIN_FILE)?;
    let has_terrain = terrain.mountain_height == TERRAIN_MOUNTAIN_HEIGHT;
    if has_terrain && tracts.region_size as usize != HEIGHTMAP_SIZE {
        return Err(WorldBuildError::RegionSize {
            level: level.to_owned(),
            size: tracts.region_size,
        });
    }
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
            Some(HeightGrid {
                samples: map.samples,
            })
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
    for (path, bytes) in files
        .iter()
        .filter(|(path, _)| path.ends_with(CAPITALS_EXTENSION))
    {
        capitals.extend(
            parse_capitals(bytes).map_err(|source| WorldBuildError::Json {
                path: entry_path(level, path),
                source,
            })?,
        );
    }
    let world = WorldFile {
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
    };
    Ok((world, skipped))
}

/// Builds, writes and reloads the world file of every level in `source`, and checks the spawn
/// IDs against the content bundle at `bundle_path`.
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
    let workers = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let chunk_size = levels.len().div_ceil(workers).max(1);
    let known = &known;
    let reports = std::thread::scope(|scope| {
        let handles: Vec<_> = levels
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(level, files)| {
                            write_level(level, files, client_build, out_dir, known)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    Ok(reports)
}

fn write_level(
    level: &str,
    files: &LevelFiles,
    client_build: &str,
    out_dir: &Path,
    known: &KnownIds,
) -> Result<LevelReport, WorldBuildError> {
    let (world, skipped) = build_level(level, files)?;
    let path = world_file_path(out_dir, client_build, level);
    world.save(&path, client_build)?;
    let expected = world.counts();
    let found = WorldFile::load(&path, client_build)?.counts();
    if found != expected {
        return Err(WorldBuildError::CountMismatch {
            level: level.to_owned(),
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
        level: level.to_owned(),
        path,
        file_bytes,
        counts: found,
        skipped,
        unmatched: known.unmatched(&world),
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
    let stream = if is_azcs(bytes) {
        Cow::Owned(decode_azcs(bytes).map_err(|source| WorldBuildError::Azcs {
            path: entry_path(level, path),
            source,
        })?)
    } else {
        Cow::Borrowed(bytes)
    };
    let root = parse_object_stream(&stream).map_err(|source| WorldBuildError::ObjectStream {
        path: entry_path(level, path),
        source,
    })?;
    read_metadata(&root, records, skipped).map_err(|source| WorldBuildError::Metadata {
        path: entry_path(level, path),
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
    use nw_formats::heightmap::{Heightmap, encode_heightmap};
    use nw_formats::objectstream::{Element, write_object_stream};
    use nw_world::metadata::*;

    use super::*;

    const LEVEL: &str = "testlevel";
    const TRACTS: &[u8] = br#"{"regionSize": 2048, "tracts": []}"#;
    const SMALL_TRACTS: &[u8] = br#"{"regionSize": 1024}"#;
    const TERRAIN: &[u8] = br#"{"oceanLevel": 40.0, "mountainHeight": 2048.0}"#;
    const FLAT_TERRAIN: &[u8] = br#"{"oceanLevel": -1000.0, "mountainHeight": 256.0}"#;
    const CAPITALS: &[u8] = br#"{"Capitals": [{"id": "a",
        "worldPosition": {"x": 2050, "y": 10, "z": 3}, "rotation": {"x": 0, "y": 0, "z": 0, "w": 1},
        "sliceName": "<PLOT>", "sliceAssetId": "b"}]}"#;
    const REGION_HEIGHTMAP: &str = "regions/r_+01_+00/region.heightmap";
    const REGION_METADATA: &str = "regions/r_+01_+00/region.metadata";
    const CAPITAL_METADATA: &str = "regions/r_+01_+00/capitals/ai/ai.metadata";
    const OTHER_CAPITAL_METADATA: &str = "regions/r_+01_+00/capitals/lore/lore.metadata";
    const CAPITALS_FILE: &str = "regions/r_+01_+00/capitals/ai/ai.capitals.json";
    const VITALS_ID: u32 = 0xae65_fdc7;
    const RAW_HEIGHT: u16 = 64;

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
                Element::with_value(FIELD_WORLD_POSITION, vec![0; 12]),
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

    fn flat_heightmap() -> Vec<u8> {
        encode_heightmap(&Heightmap {
            samples: vec![RAW_HEIGHT; HEIGHTMAP_SIZE * HEIGHTMAP_SIZE],
        })
        .unwrap()
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
        assert!(!is_world_input(&path(
            "regions/r_+01_+00/region.vegetation"
        )));
        assert!(!is_world_input("slices/x/terrain.json"));
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
            Some(f32::from(RAW_HEIGHT) / 32.0)
        );
        assert_eq!(skipped, SkipCounts::default());
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
        assert_eq!(world.level.region_size, 1024);
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
            Err(WorldBuildError::RegionSize { size: 1024, .. })
        ));
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
