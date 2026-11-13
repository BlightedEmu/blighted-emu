// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool effect-areas`: the effect areas file ([`nw_world::effect_areas`], findings
//! `2026-11-13-catacomb-erosion.md` section C10) from the game paks and the content bundle.
//!
//! For each level read, every slice that its capitals and chunk lists place (the collision
//! build's placements) is read entity by entity; each `PersistentAreaStatusEffect` component
//! whose added or removed effects change an affliction meter ([`AfflictionRules::tick`]:
//! `ApplyBlight`, `ApplyCurse`, `Env_SulfurPool_DoT`, `Fatigue`, `Breath`, ...) keeps those
//! effects of its two lists (the others, such as the `Drowning` of the coastal `Damage` areas,
//! are left out), its rules and the
//! world volume of its detection entity ([`detector_volume`]). For each `SpellData` row with a
//! `SpellPrefabPath`, the same areas of its slice are kept in the slice's own space (the
//! Fulgoris gas and spew pools). An area whose detection entity has no readable query shape is
//! counted, not kept.
//!
//! The encounter stages of the placed slices ([`stage_spawns`]): each slice of a stage's spawn
//! timeline is read too, placed at each of its spawn locations (the stage's own entity when it
//! names none, Inferred), and its areas are kept by level and stage name (the corruption areas
//! of the dunwood fort's `Setup` stage, [`nw_world::effect_areas::StageAreas`]).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use nw_content::Tables;
use nw_content::afflictions::AfflictionRules;
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, AssetCatalogError, AssetKey, paths_of};
use nw_formats::objectstream::{Element, lowercase_crc32};
use nw_formats::pak::PakError;
use nw_world::collision::{Affine, IDENTITY_AFFINE, compose};
use nw_world::effect_areas::{
    EffectArea, EffectAreas, EffectAreasError, StageAreas, effect_areas_path, slice_areas,
};
use nw_world::level_script::{
    ENCOUNTER_STAGE_TYPE, LevelScriptError, SliceScript, pak_path, slice_script,
};

use crate::collision::{CollisionError, PakIndex, detector_volume, level_slice_places};
use crate::dungeon_scripts::affine_of;
use crate::world::{WorldBuildError, stream_of};

/// What made the file.
const SOURCE: &str = "content-tool effect-areas (game paks and content bundle)";
/// The folder of the spell slices' paths (`SpellPrefabPath` is relative to it).
const SLICES_FOLDER: &str = "slices/";
/// The extension of a spell slice's pak entry.
const DYNAMIC_SLICE_EXTENSION: &str = ".dynamicslice";

/// A failed build.
#[derive(Debug, thiserror::Error)]
pub enum EffectAreasBuildError {
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
    /// A level's placements cannot be read.
    #[error(transparent)]
    Placements(#[from] CollisionError),
    /// A slice does not parse.
    #[error(transparent)]
    Slice(#[from] WorldBuildError),
    /// A slice's entities do not read.
    #[error(transparent)]
    Script(#[from] LevelScriptError),
    /// An area does not read, or the file cannot be written or read back.
    #[error(transparent)]
    File(#[from] EffectAreasError),
    /// The written file reads back other than it was written.
    #[error("{0} reads back other than written")]
    Reload(PathBuf),
}

/// What a build did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectAreasReport {
    /// The file written.
    pub path: PathBuf,
    /// The slices read.
    pub slices: usize,
    /// The level areas kept, by level.
    pub level_areas: BTreeMap<String, usize>,
    /// The spells with areas kept.
    pub spells: usize,
    /// The areas the encounter stages of the placed slices spawn, by level.
    pub stage_areas: BTreeMap<String, usize>,
    /// The areas with affliction effects whose detection volume does not read.
    pub unread_volumes: usize,
}

/// The areas of the slice whose ObjectStream root is `root`, placed at `place`, that give
/// effects of `kept`; the count of those whose volume does not read.
///
/// # Errors
///
/// Returns [`EffectAreasBuildError`] when the slice's entities or an area do not read.
pub fn placed_areas(
    root: &Element,
    place: &Affine,
    kept: &HashSet<u32>,
) -> Result<(Vec<EffectArea>, usize), EffectAreasBuildError> {
    let areas = slice_areas(root)?;
    if areas.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let script = slice_script(root)?;
    let mut placed = Vec::new();
    let mut unread = 0;
    let keep = |effects: Vec<String>| -> Vec<String> {
        effects
            .into_iter()
            .filter(|effect| kept.contains(&lowercase_crc32(effect)))
            .collect()
    };
    for area in areas {
        let effects = keep(area.effects);
        let removes = keep(area.removes);
        if effects.is_empty() && removes.is_empty() {
            continue;
        }
        let volume = script
            .entity(area.detection_entity)
            .and_then(|entity| entity.detection_shape.as_ref())
            .and_then(|(shape, offset)| {
                let at = compose(place, &affine_of(&script.placement_of(area.detection_entity)));
                detector_volume(shape, *offset, &at)
            });
        match volume {
            Some(volume) => placed.push(EffectArea {
                effects,
                removes,
                volume,
                rules: area.rules,
            }),
            None => unread += 1,
        }
    }
    Ok((placed, unread))
}

/// One slice an encounter stage of a placed slice spawns: the `Name` of the stage's entity, the
/// spawned slice's pak path and where it stands in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct StageSpawn {
    /// The stage entity's `Name`.
    pub stage: String,
    /// The spawned slice's pak path.
    pub slice: String,
    /// Its place in the world.
    pub place: Affine,
}

/// The slices that the encounter stages of `script`, a slice placed at `place`, spawn: one per
/// spawn definition with a slice and per spawn location, at the location's place (module text).
pub fn stage_spawns(script: &SliceScript, place: &Affine) -> Vec<StageSpawn> {
    let mut spawns = Vec::new();
    for entity in &script.entities {
        let Some(stage) = &entity.stage else {
            continue;
        };
        for spawn in &stage.spawns {
            let Some(slice) = &spawn.slice else {
                continue;
            };
            let own = [entity.id];
            let locations = if spawn.locations.is_empty() {
                &own[..]
            } else {
                spawn.locations.as_slice()
            };
            spawns.extend(locations.iter().map(|location| StageSpawn {
                stage: entity.name.clone(),
                slice: slice.path.clone(),
                place: compose(place, &affine_of(&script.placement_of(*location))),
            }));
        }
    }
    spawns
}

/// Whether the slice `root` holds an encounter stage component anywhere (only those slices are
/// read for their stage spawns).
fn has_stage(root: &Element) -> bool {
    root.type_id == ENCOUNTER_STAGE_TYPE || root.children.iter().any(has_stage)
}

/// The CRC-32 of each effect whose ticks change an affliction meter.
fn affliction_effects(tables: &Tables) -> HashSet<u32> {
    AfflictionRules::of_tables(tables)
        .ticks
        .iter()
        .map(|tick| tick.effect)
        .collect()
}

/// The pak path of each spell's slice, by lowercase `SpellID`.
fn spell_slices(tables: &Tables) -> BTreeMap<String, String> {
    tables
        .spell_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let hint = row.spell_prefab_path.as_deref()?.trim();
            (!hint.is_empty()).then(|| {
                (
                    row.spell_id.to_lowercase(),
                    format!("{SLICES_FOLDER}{}{DYNAMIC_SLICE_EXTENSION}", pak_path(hint)),
                )
            })
        })
        .collect()
}

/// Reads each slice of `paths` and gives it, with its path, to `visit`.
fn read_slices(
    index: &PakIndex,
    paths: &[String],
    mut visit: impl FnMut(&str, &Element) -> Result<(), EffectAreasBuildError>,
) -> Result<(), EffectAreasBuildError> {
    for (path, read) in index.read_each(paths, |path, bytes| stream_of(path, &bytes)) {
        let root = read.map_err(|source| EffectAreasBuildError::Pak {
            path: path.clone(),
            source,
        })??;
        visit(&path, &root)?;
    }
    Ok(())
}

/// The areas of the slices of `spawns` that give effects of `kept`, each at its spawn's place,
/// by stage name in the order the stages are first met; the slices read and the volumes that do
/// not read go into `report`.
fn spawned_areas(
    index: &PakIndex,
    spawns: &[StageSpawn],
    kept: &HashSet<u32>,
    report: &mut EffectAreasReport,
) -> Result<Vec<StageAreas>, EffectAreasBuildError> {
    let mut by_slice: BTreeMap<&str, Vec<&StageSpawn>> = BTreeMap::new();
    for spawn in spawns {
        by_slice.entry(spawn.slice.as_str()).or_default().push(spawn);
    }
    let paths: Vec<String> = by_slice.keys().map(|path| (*path).to_owned()).collect();
    let mut stages: Vec<StageAreas> = Vec::new();
    read_slices(index, &paths, |path, root| {
        report.slices += 1;
        for spawn in by_slice.get(path).map_or(&[][..], Vec::as_slice) {
            let (placed, unread) = placed_areas(root, &spawn.place, kept)?;
            report.unread_volumes += unread;
            if placed.is_empty() {
                continue;
            }
            match stages.iter_mut().find(|known| known.stage == spawn.stage) {
                Some(known) => known.areas.extend(placed),
                None => stages.push(StageAreas {
                    stage: spawn.stage.clone(),
                    areas: placed,
                }),
            }
        }
        Ok(())
    })?;
    Ok(stages)
}

/// Builds the effect areas file of `levels` and of the spells of `tables` from the paks of
/// `index` into `out_dir`.
///
/// # Errors
///
/// Returns [`EffectAreasBuildError`] when the catalog, a level's placements or a slice cannot be
/// read or parsed, or the file cannot be written or reads back other.
pub fn build_effect_areas(
    index: &PakIndex,
    tables: &Tables,
    levels: &[String],
    client_build: &str,
    out_dir: &std::path::Path,
) -> Result<EffectAreasReport, EffectAreasBuildError> {
    let kept = affliction_effects(tables);
    let (catalog_path, catalog) = index
        .read_each(&[ASSET_CATALOG_ENTRY.to_owned()], |_, bytes| bytes)
        .into_iter()
        .next()
        .ok_or(EffectAreasBuildError::NoCatalog)?;
    let catalog = catalog.map_err(|source| EffectAreasBuildError::Pak {
        path: catalog_path,
        source,
    })?;
    let mut file = EffectAreas {
        source: SOURCE.to_owned(),
        ..EffectAreas::default()
    };
    let mut report = EffectAreasReport::default();
    for level in levels {
        let places = level_slice_places(index, level)?;
        let wanted: HashSet<AssetKey> = places.iter().map(|(slice, _)| *slice).collect();
        let slices = paths_of(&catalog, &wanted)?;
        let mut places_of: BTreeMap<&str, Vec<&Affine>> = BTreeMap::new();
        for (slice, place) in &places {
            if let Some(path) = slices.get(slice) {
                places_of.entry(path.as_str()).or_default().push(place);
            }
        }
        let paths: Vec<String> = places_of.keys().map(|path| (*path).to_owned()).collect();
        let mut areas = Vec::new();
        let mut spawns = Vec::new();
        read_slices(index, &paths, |path, root| {
            report.slices += 1;
            let script = has_stage(root).then(|| slice_script(root)).transpose()?;
            for place in places_of.get(path).map_or(&[][..], Vec::as_slice) {
                let (placed, unread) = placed_areas(root, place, &kept)?;
                areas.extend(placed);
                report.unread_volumes += unread;
                if let Some(script) = &script {
                    spawns.extend(stage_spawns(script, place));
                }
            }
            Ok(())
        })?;
        report.level_areas.insert(level.clone(), areas.len());
        if !areas.is_empty() {
            file.levels.insert(level.clone(), areas);
        }
        let stages = spawned_areas(index, &spawns, &kept, &mut report)?;
        report.stage_areas.insert(
            level.clone(),
            stages.iter().map(|spawned| spawned.areas.len()).sum(),
        );
        if !stages.is_empty() {
            file.stages.insert(level.clone(), stages);
        }
    }
    let spells = spell_slices(tables);
    let mut by_slice: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (spell, path) in &spells {
        by_slice.entry(path.as_str()).or_default().push(spell.as_str());
    }
    let paths: Vec<String> = by_slice.keys().map(|path| (*path).to_owned()).collect();
    read_slices(index, &paths, |path, root| {
        report.slices += 1;
        let (placed, unread) = placed_areas(root, &IDENTITY_AFFINE, &kept)?;
        report.unread_volumes += unread;
        if !placed.is_empty() {
            for spell in by_slice.get(path).map_or(&[][..], Vec::as_slice) {
                file.spells.insert((*spell).to_owned(), placed.clone());
            }
        }
        Ok(())
    })?;
    report.spells = file.spells.len();
    let path = effect_areas_path(out_dir, client_build);
    file.write(&path)?;
    if EffectAreas::load(out_dir, client_build)? != file {
        return Err(EffectAreasBuildError::Reload(path));
    }
    report.path = path;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use nw_world::collision::Geometry;
    use nw_world::level_script::AREA_STATUS_EFFECT_TYPE;

    use super::*;

    /// `{41BC3BB3-EF30-41E4-B751-12C4EAB97C0E}` `DetectionVolumeComponent`.
    const DETECTION_VOLUME: [u8; 16] = [
        0x41, 0xbc, 0x3b, 0xb3, 0xef, 0x30, 0x41, 0xe4, 0xb7, 0x51, 0x12, 0xc4, 0xea, 0xb9, 0x7c,
        0x0e,
    ];
    /// `{7F2EF312-4089-4582-89C5-5D4156DAA7FB}`: the sphere query shape.
    const SPHERE_SHAPE: [u8; 16] = [
        0x7f, 0x2e, 0xf3, 0x12, 0x40, 0x89, 0x45, 0x82, 0x89, 0xc5, 0x5d, 0x41, 0x56, 0xda, 0xa7,
        0xfb,
    ];
    /// The entity of the `14ac14_props_32x32_static_chunk_26_40_6` area (`CorruptionComponent`).
    const ENTITY: u64 = 0xd1c1_2ca2_5525_f58f;
    /// Its sphere's radius, metres.
    const RADIUS_M: f32 = 2.0;
    /// Its place in the chunk slice (x, y, z), metres.
    const LOCAL_AT: [f32; 3] = [14.45, 1.49, -0.33];

    fn value(name: &str, bytes: &[u8]) -> Element {
        Element::with_value(lowercase_crc32(name), bytes.to_vec())
    }

    fn node(name: &str, children: Vec<Element>) -> Element {
        Element::with_children(lowercase_crc32(name), children)
    }

    fn typed(type_id: [u8; 16], children: Vec<Element>) -> Element {
        let mut element = node("element", children);
        element.type_id = type_id;
        element
    }

    /// The `m_localTM` bytes of an unturned place at `at`: the 9 floats of the basis columns,
    /// then the translation.
    fn local_tm(at: [f32; 3]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for column in 0..3 {
            for row in 0..3 {
                let unit: f32 = if row == column { 1.0 } else { 0.0 };
                bytes.extend(unit.to_be_bytes());
            }
        }
        for translation in at {
            bytes.extend(translation.to_be_bytes());
        }
        bytes
    }

    /// A chunk slice of one area entity that gives `effect` and removes `removed` in a sphere
    /// of its own.
    fn chunk(effect: &str, removed: &[&str]) -> Element {
        let removes = removed
            .iter()
            .map(|effect| node("element", vec![value("m_effectId", effect.as_bytes())]))
            .collect();
        let area = typed(
            AREA_STATUS_EFFECT_TYPE,
            vec![node(
                "BaseClass1",
                vec![
                    node(
                        "m_detectionEntity",
                        vec![node("EntityId", vec![value("id", &ENTITY.to_be_bytes())])],
                    ),
                    node(
                        "m_addStatusEffects",
                        vec![node("element", vec![value("m_effectId", effect.as_bytes())])],
                    ),
                    node("m_removeStatusEffects", removes),
                    value("m_removeOnExit", &[1]),
                ],
            )],
        );
        let volume = typed(
            DETECTION_VOLUME,
            vec![node(
                "m_shape",
                vec![typed(SPHERE_SHAPE, vec![value("m_radius", &RADIUS_M.to_be_bytes())])],
            )],
        );
        let transform = node("element", vec![value("m_localTM", &local_tm(LOCAL_AT))]);
        let entity = node(
            "element",
            vec![
                node("Id", vec![value("id", &ENTITY.to_be_bytes())]),
                node("Components", vec![transform, area, volume]),
            ],
        );
        Element::with_children(
            0,
            vec![node(
                "Components",
                vec![node("element", vec![node("Entities", vec![entity])])],
            )],
        )
    }

    #[test]
    fn a_chunk_area_of_an_affliction_effect_stands_at_its_entity_in_the_placed_chunk() {
        let kept = HashSet::from([lowercase_crc32("ApplyBlight")]);
        let mut place = IDENTITY_AFFINE;
        place[0][3] = 1_000.0;
        place[1][3] = 2_000.0;
        let (areas, unread) = placed_areas(&chunk("ApplyBlight", &[]), &place, &kept).expect("areas");
        assert_eq!(unread, 0);
        let [area] = areas.as_slice() else {
            panic!("one area: {areas:?}");
        };
        assert_eq!(area.effects, vec!["ApplyBlight".to_owned()]);
        assert!(area.removes.is_empty());
        assert!(area.rules.remove_on_exit);
        assert_eq!(area.volume.shape, Geometry::Sphere { radius: RADIUS_M });
        let centre = [1_014.45, 2_001.49, -0.33];
        assert!(area.contains(centre));
        assert!(!area.contains([centre[0] + RADIUS_M + 0.1, centre[1], centre[2]]));
    }

    #[test]
    fn a_stage_spawns_its_slice_at_each_spawn_location_in_the_placed_slice() {
        use nw_world::level_script::{
            AssetRef, EncounterStage, ScriptEntity, SpawnDefinition, Transform,
        };
        /// The `Setup` stage entity and two of its spawn locations (`CorruptionArea1`, `2`).
        const STAGE: u64 = 1;
        const FIRST: u64 = 2;
        const SECOND: u64 = 3;
        const SLICE: &str = "slices/structures/siege/invasion/invasion_corruptionarea.dynamicslice";
        let at = |x: f64| Transform {
            translation: [x, 0.0, 0.0],
            ..Transform::default()
        };
        let location = |id: u64, x: f64| ScriptEntity {
            id,
            name: format!("CorruptionArea{id}"),
            local: at(x),
            ..ScriptEntity::default()
        };
        let spawn = |locations: Vec<u64>| SpawnDefinition {
            slice: Some(AssetRef {
                guid: [0; 16],
                path: SLICE.to_owned(),
            }),
            locations,
            ..SpawnDefinition::default()
        };
        let script = SliceScript {
            entities: vec![
                ScriptEntity {
                    id: STAGE,
                    name: "Setup".to_owned(),
                    local: at(5.0),
                    stage: Some(EncounterStage {
                        spawns: vec![spawn(vec![FIRST, SECOND]), spawn(Vec::new())],
                        ..EncounterStage::default()
                    }),
                    ..ScriptEntity::default()
                },
                location(FIRST, 10.0),
                location(SECOND, 20.0),
            ],
        };
        let mut place = IDENTITY_AFFINE;
        place[0][3] = 1_000.0;
        let spawns = stage_spawns(&script, &place);
        let xs: Vec<f32> = spawns.iter().map(|spawn| spawn.place[0][3]).collect();
        assert_eq!(
            xs,
            [1_010.0, 1_020.0, 1_005.0],
            "a spawn without locations stands at its stage"
        );
        assert!(spawns.iter().all(|spawn| spawn.stage == "Setup" && spawn.slice == SLICE));
    }

    #[test]
    fn only_a_slice_with_an_encounter_stage_is_read_for_its_spawns() {
        assert!(!has_stage(&chunk("ApplyBlight", &[])));
        let staged = node("Components", vec![typed(ENCOUNTER_STAGE_TYPE, Vec::new())]);
        assert!(has_stage(&Element::with_children(0, vec![staged])));
    }

    #[test]
    fn an_area_of_effects_that_change_no_meter_is_left_out() {
        let kept = HashSet::from([lowercase_crc32("ApplyBlight")]);
        let (areas, unread) =
            placed_areas(&chunk("Stunned_Afflict", &[]), &IDENTITY_AFFINE, &kept).expect("areas");
        assert!(areas.is_empty());
        assert_eq!(unread, 0);
    }

    #[test]
    fn a_coastal_damage_area_keeps_only_the_affliction_effect_it_removes() {
        let kept = HashSet::from([lowercase_crc32("Breath")]);
        let (areas, _) = placed_areas(&chunk("Drowning", &["Breath"]), &IDENTITY_AFFINE, &kept)
            .expect("areas");
        let [area] = areas.as_slice() else {
            panic!("one area: {areas:?}");
        };
        assert!(area.effects.is_empty(), "Drowning changes no meter");
        assert_eq!(area.removes, vec!["Breath".to_owned()]);
    }
}
