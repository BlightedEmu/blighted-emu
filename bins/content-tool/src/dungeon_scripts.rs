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
use nw_formats::azcs::AzcsError;
use nw_formats::objectstream::{Element, ObjectStreamError, parse_object_stream};
use nw_formats::pak::PakError;
use nw_world::capitals::parse_capitals;
use nw_world::dungeon_scripts::{
    Chest, DungeonScripts, DungeonScriptsError, Encounter, LevelScript, ObjectiveSpec, Spawn,
    Stage, StageAction, StageObjective, Target, TargetMob, TargetSlice, dungeon_scripts_path,
};
use nw_world::level_script::{
    Action, AliasSlice, AssetRef, DYNAMIC_SLICE_SUFFIX, LevelScriptError, ObjectiveKind,
    SliceScript, Transform, alias_slices, pak_path, slice_script,
};
use nw_world::metadata::{MetadataRecords, SkipCounts, read_metadata};

use crate::collision::PakIndex;
use crate::roads::without_azcs;

/// The folder of the levels' data in the paks.
const COATLICUE_DIR: &str = "sharedassets/coatlicue";
/// The suffix of a capitals file.
const CAPITALS_SUFFIX: &str = ".capitals.json";
/// The folder of a slice path of the game mode table.
const SLICES_DIR: &str = "slices";
/// The suffix of an alias asset.
const ALIAS_SUFFIX: &str = ".aliasasset";
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

/// Failure while building the dungeon script file.
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
}

/// The pak entries, read once each.
pub struct PakSlices<'a> {
    index: &'a PakIndex,
    read: HashMap<String, Option<Element>>,
}

impl<'a> PakSlices<'a> {
    /// The slices of `index`.
    pub fn new(index: &'a PakIndex) -> Self {
        Self {
            index,
            read: HashMap::new(),
        }
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
    wanted: &[String],
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
            let script = pak_path(&format!("{}{DYNAMIC_SLICE_SUFFIX}", capital.slice_name));
            if wanted.contains(&script) {
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
    let catalog = InstanceCatalog::from_tables(tables);
    let slice_paths: HashMap<&str, &str> = tables
        .game_mode_map_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| Some((row.game_mode_id.as_str(), row.slice_path.as_deref()?)))
        .collect();
    let levels = vitals_levels(tables);
    let names = vitals_names(tables);
    let mut report = DungeonScriptsReport::default();
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
            None => report.skipped_modes.push(mode.id.clone()),
        }
    }
    let mut slices = PakSlices::new(index);
    let mut scripts = DungeonScripts::default();
    for (folder, by_script) in &wanted {
        let scripts_here: Vec<String> = by_script.keys().cloned().collect();
        let placements = script_placements(index, folder, &scripts_here)?;
        let points = encounter_points(index, folder, &names)?;
        for (script, modes) in by_script {
            let Some((_, placement)) = placements.iter().find(|(path, _)| path == script) else {
                report.skipped_modes.extend(modes.iter().cloned());
                continue;
            };
            let Some(mut level) = build_level(
                &mut slices,
                script,
                *placement,
                &levels,
                &points,
                &mut report,
            )?
            else {
                report.skipped_modes.extend(modes.iter().cloned());
                continue;
            };
            level.level.clone_from(folder);
            report.levels += 1;
            for mode in modes {
                scripts.game_modes.insert(mode.clone(), level.clone());
            }
        }
    }
    let path = dungeon_scripts_path(out_dir, client_build);
    scripts.write(&path)?;
    if DungeonScripts::load(&path)? != scripts {
        return Err(DungeonScriptsBuildError::Reload(path));
    }
    report.path = path;
    Ok(report)
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
    levels: &HashMap<String, u32>,
    points: &[EncounterPoint],
    report: &mut DungeonScriptsReport,
) -> Result<Option<LevelScript>, DungeonScriptsBuildError> {
    let Some(level_script) = read_script(source, script, report)? else {
        return Ok(None);
    };
    let mut level = LevelScript {
        script_slice: script.to_owned(),
        ..LevelScript::default()
    };
    for entity in &level_script.entities {
        let Some(spawned) = entity
            .spawner
            .as_ref()
            .and_then(|spawner| spawner.slice.as_ref())
        else {
            continue;
        };
        let Some(spawned_script) = read_script(source, &spawned.path, report)? else {
            continue;
        };
        let placed = placement.then(&level_script.placement_of(entity.id));
        if spawned_script.manager().is_some() {
            let mut builder = EncounterBuilder::new(source, levels, report);
            let mut encounter =
                builder.encounter(&entity.name, &spawned.path, &spawned_script, placed)?;
            assign_creatures(&mut encounter, points);
            report.encounters += 1;
            report.stages += encounter.stages.len();
            report.spawns += encounter
                .stages
                .iter()
                .map(|stage| stage.spawns.len())
                .sum::<usize>();
            level.encounters.push(encounter);
        } else if let Some(table) = spawned_script.loot_table() {
            report.chests += 1;
            level.chests.push(Chest {
                name: entity.name.clone(),
                slice: spawned.path.clone(),
                position: position_of(&placed),
                loot_table: table.to_owned(),
            });
        }
    }
    Ok(Some(level))
}

/// The script of the slice at `path`; `None` (and the path noted) when it is not there.
fn read_script(
    source: &mut impl SliceSource,
    path: &str,
    report: &mut DungeonScriptsReport,
) -> Result<Option<SliceScript>, DungeonScriptsBuildError> {
    let Some(root) = source.root(path)? else {
        if !report.missing_slices.iter().any(|known| known == path) {
            report.missing_slices.push(path.to_owned());
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

/// World metres of a transform's translation.
#[allow(clippy::cast_possible_truncation, reason = "world metres")]
fn position_of(transform: &Transform) -> [f32; 3] {
    transform.translation.map(|value| value as f32)
}

/// Builds one encounter, resolving its targets once each.
struct EncounterBuilder<'a, S> {
    source: &'a mut S,
    levels: &'a HashMap<String, u32>,
    report: &'a mut DungeonScriptsReport,
    targets: Vec<Target>,
    /// Target index by (slice path, alias path).
    target_index: HashMap<(String, String), usize>,
}

impl<'a, S: SliceSource> EncounterBuilder<'a, S> {
    fn new(
        source: &'a mut S,
        levels: &'a HashMap<String, u32>,
        report: &'a mut DungeonScriptsReport,
    ) -> Self {
        Self {
            source,
            levels,
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
                spawns.push(Spawn {
                    target,
                    delay_s: definition.delay_s,
                    count: definition.count,
                    locations: definition
                        .locations
                        .iter()
                        .map(|location| position_of(&placed.then(&script.placement_of(*location))))
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
        Ok(Encounter {
            name: name.to_owned(),
            slice: path.to_owned(),
            position: position_of(&placed.then(&script.placement_of(root.id))),
            yaw_deg,
            auto_activate: manager.auto_activate,
            auto_activate_delay_ms: manager.auto_activate_delay_ms,
            grace_period_s: manager.grace_period_s,
            reset_event: manager.reset_event.clone(),
            detection_radius_m: root.detection_radius_m,
            top_stages: manager.stages.iter().filter_map(index_of).collect(),
            stages,
            targets: std::mem::take(&mut self.targets),
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
        let Some(root) = self.source.root(path)? else {
            if !self.report.missing_slices.iter().any(|known| known == path) {
                self.report.missing_slices.push(path.to_owned());
            }
            return Ok(Vec::new());
        };
        alias_slices(&root).map_err(|source| DungeonScriptsBuildError::Script {
            path: path.to_owned(),
            source,
        })
    }

    fn target_slice(
        &mut self,
        slice: &AssetRef,
        percent: u32,
    ) -> Result<TargetSlice, DungeonScriptsBuildError> {
        let variant = read_script(&mut *self.source, &slice.path, &mut *self.report)?
            .and_then(|script| script.variant().cloned());
        let mob = variant.map(|variant| {
            let level = match variant.level {
                0 => self
                    .levels
                    .get(&variant.vitals_id.to_lowercase())
                    .copied()
                    .unwrap_or_default(),
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
        })
    }
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
            &levels,
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
            &HashMap::new(),
            &[],
            &mut report,
        )
        .expect("build");
        assert!(built.is_none());
        assert_eq!(report.missing_slices, [SCRIPT]);
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
}
