// SPDX-License-Identifier: AGPL-3.0-or-later
//! The status effect areas of the levels and of the spell slices (affliction helper C10,
//! findings `2026-11-13-catacomb-erosion.md`, section C10): each `PersistentAreaStatusEffect`
//! component ([`crate::level_script::AREA_STATUS_EFFECT_TYPE`]) gives its `m_addStatusEffects`
//! to the characters in the detection volume of its `m_detectionEntity`.
//!
//! **Observed** (build 25810808, the game paks; client `FUN_143ff0820`, the component's field
//! list): `m_detectionEntity`, `m_addStatusEffects` (each `m_effectId`), `m_removeStatusEffects`,
//! `m_removeOnExit`, `m_initialDelaySeconds`, `m_reapplicationDelaySeconds`,
//! `m_inheritStatusEffects`, `m_detectedTargetType`.
//!
//! - The level areas stand in the static chunk slices that a level's chunk lists place
//!   (`coatgen/<hash>/<layer>_<n>x<n>_static_chunk_*`): `ApplyBlight` (2 a second) in
//!   `14ac14_props` (Edengrove), `ApplyCurse` in `16bs08_hedetetcanyon_props`,
//!   `Env_SulfurPool_DoT` in `16bs04_deadwater_props`, `Fatigue` and `Breath` in the
//!   `persistence` chunks of the coasts (retail `Breath`: Cutlass Keys, 282.831 s of
//!   `c10a_q4_spectralartifacts`). Their entity `CorruptionComponent` (`14ac14_props_32x32_static_chunk_26_40_6`)
//!   detects itself, with a 2 m sphere and `OnlyPlayerTarget`; `m_removeOnExit` on, both delays 0.
//! - The Fulgoris gas: the spell `AI_Fulgoris_Blight_Self_Destruct_Catacombs` spawns
//!   `catacombs_fulgoris_tissue_gas`, whose entity `AreaStatusEffect` gives `AI_Fulgoris_Gas`
//!   (`AfflictionBlight=5`, 1 s ticks) in a 1 m sphere, `m_removeOnExit` off, both delays 0.5 s.
//!   Retail `CATACOMBS6` applies it again about every 0.5 s while the player stands in it
//!   (applications 1 to 9, 1310.975 s to 1316.506 s, each one back to a 19.8 s rest), and the
//!   effect stays after the player leaves (it ends 20 s after its last application: a new
//!   application 1 at 1336.306 s). The spew pools (`AI_Fulgoris_Blight_Spew*`, 3 m) give
//!   `AI_Fulgoris_Gas_Catacombs` in the catacombs and `AI_Fulgoris_Gas` in the open world.
//!
//! - The areas an encounter stage spawns ([`StageAreas`]; findings section C13): the fort slice
//!   `coatgen/bfbb7d6d/siege_fort_dunwood_3407430195` has nine encounter stage entities,
//!   `Setup` and `Stage_1` to `Stage_8`; the spawn timeline of `Setup` spawns
//!   `Invasion_CorruptionArea.slice` (entity `CorruptionArea`: `ApplyCurseL4`, remove on exit,
//!   delays 0) at its four spawn locations `CorruptionArea1` to `CorruptionArea4`. Of the 774
//!   static chunk slices that name a slice, no other stage spawns an affliction area.
//!
//! **Inferred**: a detection volume stands where its entity stands in the placed slice, as the
//! encounter detectors do (without the entity's scale); a slice a stage spawns stands at its spawn
//! location's place in the placed slice, as the encounter mobs do; an area of a slice that a
//! placed slice spawns in another way is not read (every other static area found lies in a
//! chunk slice itself).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nw_formats::objectstream::{Element, ObjectStreamError, lowercase_crc32};
use serde::{Deserialize, Serialize};

use crate::dungeon_scripts::Detector;
use crate::level_script::{AREA_STATUS_EFFECT_TYPE, FIELD_DEPTH, find};

/// The file name before the client build.
const FILE_PREFIX: &str = "effect-areas-";
/// The file name after the client build.
const FILE_SUFFIX: &str = ".json";
/// Milliseconds in a second.
const MS_PER_S: f32 = 1_000.0;
/// The column of an affine place that holds its translation.
const TRANSLATION_COLUMN: usize = 3;

/// The file of client build `client_build` in content directory `dir`.
pub fn effect_areas_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad effect areas file or slice.
#[derive(Debug, thiserror::Error)]
pub enum EffectAreasError {
    /// The file cannot be read or written.
    #[error("effect areas file {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("effect areas file: {0}")]
    Json(#[from] serde_json::Error),
    /// A slice field has the wrong form.
    #[error("effect area field: {0}")]
    Field(#[from] ObjectStreamError),
}

/// When an area gives its effects and takes them back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaRules {
    /// `m_removeOnExit`: a character that leaves loses the effects.
    pub remove_on_exit: bool,
    /// `m_initialDelaySeconds`, in ms: the time from a character's entry to the first effects.
    pub initial_delay_ms: u64,
    /// `m_reapplicationDelaySeconds`, in ms: the time between two applications to a character
    /// that stays; 0 applies them once per entry.
    pub reapply_ms: u64,
}

/// One status effect area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectArea {
    /// The status effects it gives (`m_addStatusEffects`), in order.
    pub effects: Vec<String>,
    /// The status effects it takes from a character that enters (`m_removeStatusEffects`), in
    /// order; see [`SliceArea::removes`].
    #[serde(default)]
    pub removes: Vec<String>,
    /// Its volume: in the world for a level area, in the slice's own space for a spell's.
    pub volume: Detector,
    /// When it gives them.
    pub rules: AreaRules,
}

impl EffectArea {
    /// Whether point `point` (in the volume's space) is inside it.
    pub fn contains(&self, point: [f32; 3]) -> bool {
        self.volume.contains(point)
    }

    /// Where its volume stands (in the volume's space).
    pub fn centre(&self) -> [f32; 3] {
        let [x, y, z] = &self.volume.place;
        [x[TRANSLATION_COLUMN], y[TRANSLATION_COLUMN], z[TRANSLATION_COLUMN]]
    }
}

/// The areas that one encounter stage of a placed slice spawns, in the world: they stand while
/// the stage runs (module text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageAreas {
    /// The `Name` of the stage's entity in its slice (`Setup`).
    pub stage: String,
    /// The areas.
    pub areas: Vec<EffectArea>,
}

/// The effect areas of the levels and of the spell slices.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectAreas {
    /// What made the file.
    pub source: String,
    /// The areas of each level folder, in the world.
    pub levels: BTreeMap<String, Vec<EffectArea>>,
    /// The areas of the slice of each spell (lowercase `SpellID`), in the slice's own space:
    /// its origin is the spell's centre.
    pub spells: BTreeMap<String, Vec<EffectArea>>,
    /// The areas the encounter stages of each level folder's placed slices spawn, in the world.
    #[serde(default)]
    pub stages: BTreeMap<String, Vec<StageAreas>>,
}

impl EffectAreas {
    /// The file of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`EffectAreasError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, EffectAreasError> {
        let path = effect_areas_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| EffectAreasError::Io { path, source })?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Writes the file to `path`.
    ///
    /// # Errors
    ///
    /// Returns [`EffectAreasError`] when the file cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), EffectAreasError> {
        std::fs::write(path, serde_json::to_string_pretty(self)?).map_err(|source| {
            EffectAreasError::Io {
                path: path.to_owned(),
                source,
            }
        })
    }

    /// The areas of level `level`.
    pub fn of_level(&self, level: &str) -> &[EffectArea] {
        self.levels.get(level).map_or(&[], Vec::as_slice)
    }

    /// The areas of the slice of spell `spell` (any case).
    pub fn of_spell(&self, spell: &str) -> &[EffectArea] {
        self.spells
            .get(&spell.to_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// The areas that the stages named `stage` (any case) of the placed slices of every level
    /// spawn.
    pub fn of_stage<'a>(&'a self, stage: &'a str) -> impl Iterator<Item = &'a EffectArea> + 'a {
        self.stages
            .values()
            .flatten()
            .filter(move |spawned| spawned.stage.eq_ignore_ascii_case(stage))
            .flat_map(|spawned| &spawned.areas)
    }

    /// How many areas the file holds.
    pub fn len(&self) -> usize {
        let staged: usize = self
            .stages
            .values()
            .flatten()
            .map(|spawned| spawned.areas.len())
            .sum();
        self.levels.values().chain(self.spells.values()).map(Vec::len).sum::<usize>() + staged
    }

    /// Whether the file holds no area.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A `PersistentAreaStatusEffect` component of a slice, before its volume is placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceArea {
    /// The entity of its detection volume (`m_detectionEntity`).
    pub detection_entity: u64,
    /// Its effects.
    pub effects: Vec<String>,
    /// The effects it removes (`m_removeStatusEffects`). **Observed** (client
    /// `FUN_143fe6ca0`, the server facet's entry handler, and `FUN_143ff3af0`, its
    /// `RemoveStatusEffects`): when a character enters, the area waits its initial delay, then
    /// removes these from a living character once, before it applies its own effects; the
    /// reapplications do not remove them again. Observed data: the coastal `persistence` chunks
    /// (`08queensport_persistence_256x256_static_chunk_2_0_0`, `10wshores_persistence_v2`)
    /// hold an entity `Damage` that adds `Drowning` and removes `Breath`.
    pub removes: Vec<String>,
    /// Its rules.
    pub rules: AreaRules,
}

/// The names of the fields read, as their name CRCs.
struct Names {
    components: u32,
    entities: u32,
    id: u32,
    entity_id: u32,
    detection_entity: u32,
    add_effects: u32,
    effect_id: u32,
    remove_effects: u32,
    remove_on_exit: u32,
    initial_delay: u32,
    reapply_delay: u32,
}

impl Names {
    fn new() -> Self {
        Self {
            components: lowercase_crc32("Components"),
            entities: lowercase_crc32("Entities"),
            id: lowercase_crc32("Id"),
            entity_id: lowercase_crc32("EntityId"),
            detection_entity: lowercase_crc32("m_detectionEntity"),
            add_effects: lowercase_crc32("m_addStatusEffects"),
            effect_id: lowercase_crc32("m_effectId"),
            remove_effects: lowercase_crc32("m_removeStatusEffects"),
            remove_on_exit: lowercase_crc32("m_removeOnExit"),
            initial_delay: lowercase_crc32("m_initialDelaySeconds"),
            reapply_delay: lowercase_crc32("m_reapplicationDelaySeconds"),
        }
    }
}

/// Seconds field `name` of `component` in whole ms; 0 when it is missing or negative.
fn delay_ms(component: &Element, name: u32) -> Result<u64, EffectAreasError> {
    let seconds = find(component, name, FIELD_DEPTH).map_or(Ok(0.0), Element::as_f32)?;
    // A few seconds.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a delay of seconds"
    )]
    Ok((seconds.max(0.0) * MS_PER_S).round() as u64)
}

/// The effect ids of effect list `list` of `component` (each entry's `m_effectId`), in
/// order, without the empty ones.
fn effect_list(
    component: &Element,
    list: u32,
    names: &Names,
) -> Result<Vec<String>, EffectAreasError> {
    Ok(find(component, list, FIELD_DEPTH)
        .map(|list| {
            list.children
                .iter()
                .filter_map(|entry| find(entry, names.effect_id, FIELD_DEPTH))
                .map(|id| id.as_string().map(|id| id.trim().to_owned()))
                .filter(|id| id.as_ref().map_or(true, |id| !id.is_empty()))
                .collect::<Result<_, _>>()
        })
        .transpose()?
        .unwrap_or_default())
}

/// The area of `component` of entity `owner`; `None` when it neither gives nor removes an
/// effect.
fn slice_area(
    component: &Element,
    owner: u64,
    names: &Names,
) -> Result<Option<SliceArea>, EffectAreasError> {
    let effects = effect_list(component, names.add_effects, names)?;
    let removes = effect_list(component, names.remove_effects, names)?;
    if effects.is_empty() && removes.is_empty() {
        return Ok(None);
    }
    let detection_entity = find(component, names.detection_entity, FIELD_DEPTH)
        .and_then(|entity| find(entity, names.entity_id, FIELD_DEPTH))
        .and_then(|entity| entity.child(names.id))
        .map(Element::as_u64)
        .transpose()?
        .unwrap_or(owner);
    Ok(Some(SliceArea {
        detection_entity,
        effects,
        removes,
        rules: AreaRules {
            remove_on_exit: find(component, names.remove_on_exit, FIELD_DEPTH)
                .map_or(Ok(false), Element::as_bool)?,
            initial_delay_ms: delay_ms(component, names.initial_delay)?,
            reapply_ms: delay_ms(component, names.reapply_delay)?,
        },
    }))
}

/// The `PersistentAreaStatusEffect` components with effects of the slice whose ObjectStream
/// root is `root`, in stream order; none for a root without slice entities.
///
/// # Errors
///
/// Returns [`EffectAreasError`] when a field read has the wrong form.
pub fn slice_areas(root: &Element) -> Result<Vec<SliceArea>, EffectAreasError> {
    let names = Names::new();
    let Some(entities) = root.child(names.components).and_then(|components| {
        components
            .children
            .iter()
            .find_map(|component| component.child(names.entities))
    }) else {
        return Ok(Vec::new());
    };
    let mut areas = Vec::new();
    for entity in &entities.children {
        let Some(owner) = entity
            .child(names.id)
            .and_then(|id| id.child(names.id))
            .map(Element::as_u64)
            .transpose()?
        else {
            continue;
        };
        for component in entity
            .child(names.components)
            .map(|list| list.children.as_slice())
            .unwrap_or_default()
            .iter()
            .filter(|component| component.type_id == AREA_STATUS_EFFECT_TYPE)
        {
            areas.extend(slice_area(component, owner, &names)?);
        }
    }
    Ok(areas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::{Geometry, IDENTITY_AFFINE};

    /// A named field of `value`'s bytes.
    fn value(name: &str, bytes: &[u8]) -> Element {
        Element::with_value(lowercase_crc32(name), bytes.to_vec())
    }

    /// A named struct of `children`.
    fn node(name: &str, children: Vec<Element>) -> Element {
        Element::with_children(lowercase_crc32(name), children)
    }

    /// An effect list of `effects` (each an `m_effectId`; an empty id stays an empty entry).
    fn effect_list(name: &str, effects: &[&str]) -> Element {
        node(
            name,
            effects
                .iter()
                .map(|effect| node("element", vec![value("m_effectId", effect.as_bytes())]))
                .collect(),
        )
    }

    /// The `catacombs_fulgoris_tissue_gas` shape: its area entity `owner` detects with entity
    /// `detector`, gives `effect`, removes `removed`, does not remove on exit, and waits 0.5 s
    /// twice.
    fn gas_slice(owner: u64, detector: u64, effect: &str, removed: &[&str]) -> Element {
        let half_second = 0.5_f32.to_be_bytes();
        let data = node(
            "BaseClass1",
            vec![
                node(
                    "m_detectionEntity",
                    vec![node("EntityId", vec![value("id", &detector.to_be_bytes())])],
                ),
                effect_list("m_addStatusEffects", &[effect]),
                effect_list("m_removeStatusEffects", removed),
                value("m_removeOnExit", &[0]),
                value("m_initialDelaySeconds", &half_second),
                value("m_reapplicationDelaySeconds", &half_second),
            ],
        );
        let mut component = node("element", vec![data]);
        component.type_id = AREA_STATUS_EFFECT_TYPE;
        let entity = node(
            "element",
            vec![
                node("Id", vec![value("id", &owner.to_be_bytes())]),
                node("Components", vec![component]),
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
    fn a_gas_area_reads_its_effect_its_detection_entity_and_its_delays() {
        const OWNER: u64 = 0xd8c5_3967_dc3d_1580;
        const DETECTOR: u64 = 0xd8c5_3b3b_e8ad_5461;
        let areas = slice_areas(&gas_slice(OWNER, DETECTOR, "AI_Fulgoris_Gas", &[])).expect("areas");
        assert_eq!(
            areas,
            vec![SliceArea {
                detection_entity: DETECTOR,
                effects: vec!["AI_Fulgoris_Gas".to_owned()],
                removes: Vec::new(),
                rules: AreaRules {
                    remove_on_exit: false,
                    initial_delay_ms: 500,
                    reapply_ms: 500,
                },
            }]
        );
    }

    #[test]
    fn an_area_without_effects_and_a_slice_without_entities_give_nothing() {
        assert!(slice_areas(&gas_slice(1, 1, "", &[])).expect("areas").is_empty());
        assert!(slice_areas(&Element::with_children(0, Vec::new())).expect("areas").is_empty());
    }

    #[test]
    fn an_area_that_only_removes_an_effect_is_kept_with_its_remove_list() {
        let areas = slice_areas(&gas_slice(3, 3, "", &["Breath", ""])).expect("areas");
        assert_eq!(areas.len(), 1);
        assert!(areas[0].effects.is_empty());
        assert_eq!(areas[0].removes, vec!["Breath".to_owned()]);
    }

    #[test]
    fn the_file_finds_the_areas_of_a_level_and_of_a_spell_in_any_case() {
        let area = EffectArea {
            effects: vec!["ApplyBlight".to_owned()],
            removes: Vec::new(),
            volume: Detector {
                shape: Geometry::Sphere { radius: 2.0 },
                place: IDENTITY_AFFINE,
            },
            rules: AreaRules {
                remove_on_exit: true,
                ..AreaRules::default()
            },
        };
        let areas = EffectAreas {
            source: String::new(),
            levels: BTreeMap::from([("newworld_vitaeeterna".to_owned(), vec![area.clone()])]),
            spells: BTreeMap::from([(
                "ai_fulgoris_blight_self_destruct_catacombs".to_owned(),
                vec![area.clone()],
            )]),
            stages: BTreeMap::from([(
                "newworld_vitaeeterna".to_owned(),
                vec![StageAreas {
                    stage: "Setup".to_owned(),
                    areas: vec![area.clone()],
                }],
            )]),
        };
        assert_eq!(areas.of_level("newworld_vitaeeterna"), std::slice::from_ref(&area));
        assert!(areas.of_level("nw_catacomb_00").is_empty());
        assert_eq!(
            areas.of_spell("AI_Fulgoris_Blight_Self_Destruct_Catacombs"),
            std::slice::from_ref(&area)
        );
        assert_eq!(areas.of_stage("setup").collect::<Vec<_>>(), [&area]);
        assert_eq!(areas.of_stage("Stage_1").count(), 0);
        assert_eq!(areas.len(), 3);
        assert!(area.contains([1.0, 1.0, 1.0]) && !area.contains([2.0, 1.0, 0.0]));
        let text = serde_json::to_string(&areas).expect("json");
        assert_eq!(serde_json::from_str::<EffectAreas>(&text).expect("back"), areas);
    }

    #[test]
    fn an_area_stands_at_its_volumes_place_and_an_old_file_has_no_stage_areas() {
        let mut place = IDENTITY_AFFINE;
        place[0][TRANSLATION_COLUMN] = 10.0;
        place[1][TRANSLATION_COLUMN] = 20.0;
        place[2][TRANSLATION_COLUMN] = 30.0;
        let area = EffectArea {
            effects: vec!["ApplyCurseL4".to_owned()],
            removes: Vec::new(),
            volume: Detector {
                shape: Geometry::Sphere { radius: 2.0 },
                place,
            },
            rules: AreaRules::default(),
        };
        assert_eq!(area.centre(), [10.0, 20.0, 30.0]);
        let old: EffectAreas =
            serde_json::from_str(r#"{"source": "", "levels": {}, "spells": {}}"#).expect("old");
        assert!(old.stages.is_empty() && old.is_empty());
    }
}
