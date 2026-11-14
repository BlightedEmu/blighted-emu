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
//! - The areas of the encounter slices and of the slices their stages spawn ([`EffectAreas::slices`];
//!   findings section E2), in the slice's own space: the darkness hazards
//!   (`darkness_hazard_corruption_minor_l1` to `_major_l4`: `ApplyCurseL1` to `L4`, remove on exit,
//!   delays 0, a 30 m by 25 m `CylinderShapeComponent` beside a `DetectionVolumeComponent` with no
//!   query shape) that every corrupted breach's setup stage spawns, and the Myrkgard portal's
//!   own `Darkness_Hazard_Corruption_Minor_L4` entity (`enc_darkness_major_portal_sm_01`:
//!   `ApplyCurseL4` in a `PolygonPrismShapeComponent` of 17 corners, 13 m high). Only the areas of
//!   entities that are active as their slice spawns (`IsRuntimeActive`) are kept
//!   ([`SliceArea::active`]). **Observed** (`Playthrough_Part_18`, the portal at 9766.9,
//!   9616.5): other players held `ApplyCurseL4` from 1175.7 s, 9 s after the portal's manager
//!   showed its run; the own player from 1270.6 s, as it came near, to 1427.3 s, 10.2 s after the
//!   manager's success (1417.1 s).
//!
//! **Inferred**: a detection volume stands where its entity stands in the placed slice, as the
//! encounter detectors do (without the entity's scale); a detection volume with no query shape
//! detects with its entity's engine shape component (the darkness hazards hold no other shape);
//! a polygon prism stands on its entity, its corners in the entity's plane and its height up (the
//! engine's `PolygonPrismShape`); a slice a stage spawns stands at its spawn location's place in
//! the placed slice, as the encounter mobs do; an area of a slice that a placed slice spawns in
//! another way is not read (every other static area found lies in a chunk slice itself).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use nw_formats::objectstream::{Element, ObjectStreamError, UUID_SIZE, lowercase_crc32};
use nw_formats::rigid::Vec3;
use serde::{Deserialize, Serialize};

use crate::collision::{Geometry, invert, transform_point};
use crate::dungeon_scripts::Detector;
use crate::level_script::{AREA_STATUS_EFFECT_TYPE, FIELD_DEPTH, SHAPE_HEIGHT_NAME, find};

/// The file name before the client build.
const FILE_PREFIX: &str = "effect-areas-";
/// The file name after the client build.
const FILE_SUFFIX: &str = ".json";
/// Milliseconds in a second.
const MS_PER_S: f32 = 1_000.0;
/// The column of an affine place that holds its translation.
const TRANSLATION_COLUMN: usize = 3;
/// `{AD882674-1D5D-4E40-B079-449B47D2492C}` the engine's `PolygonPrismShapeComponent`
/// (`Configuration`, `PolygonPrism`: `Height` and `VertexContainer`'s `Vertices`).
pub const POLYGON_PRISM_TYPE: [u8; UUID_SIZE] = [
    0xad, 0x88, 0x26, 0x74, 0x1d, 0x5d, 0x4e, 0x40, 0xb0, 0x79, 0x44, 0x9b, 0x47, 0xd2, 0x49, 0x2c,
];
/// `{3D80F623-C85C-4741-90D0-E4E66164E6BF}` `AZ::Vector2`: one corner of a polygon prism.
const VECTOR2_TYPE: [u8; UUID_SIZE] = [
    0x3d, 0x80, 0xf6, 0x23, 0xc8, 0x5c, 0x47, 0x41, 0x90, 0xd0, 0xe4, 0xe6, 0x61, 0x64, 0xe6, 0xbf,
];
/// The bytes of one big-endian `f32`.
const F32_LEN: usize = 4;
/// The fewest corners of a polygon.
const MIN_CORNERS: usize = 3;
/// How deep a polygon prism component keeps its fields (Observed in
/// `enc_darkness_major_portal_sm_01`: `Configuration`, `PolygonPrism`, its base, then `Height`
/// and `VertexContainer`'s `Vertices`, 4 and 5 levels down).
const PRISM_DEPTH: usize = 5;

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
    /// Whether point `point` (in the volume's space) is inside it: a closed mesh (a polygon
    /// prism, [`prism_mesh`]) by [`mesh_contains`], any other shape as a detector.
    pub fn contains(&self, point: [f32; 3]) -> bool {
        match &self.volume.shape {
            Geometry::Mesh { points, triangles } => invert(&self.volume.place).is_some_and(
                |inverse| mesh_contains(points, triangles, transform_point(&inverse, point)),
            ),
            _ => self.volume.contains(point),
        }
    }

    /// The area placed by `place` (the world from the area's slice space).
    #[must_use]
    pub fn placed(&self, place: &crate::collision::Affine) -> Self {
        Self {
            volume: Detector {
                shape: self.volume.shape.clone(),
                place: crate::collision::compose(place, &self.volume.place),
            },
            ..self.clone()
        }
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
    /// The areas of each encounter slice and of each slice an encounter's stage spawns, by pak
    /// path, in the slice's own space (module text).
    #[serde(default)]
    pub slices: BTreeMap<String, Vec<EffectArea>>,
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

    /// The areas of encounter slice or stage spawn slice `path`, in its own space.
    pub fn of_slice(&self, path: &str) -> &[EffectArea] {
        self.slices.get(path).map_or(&[], Vec::as_slice)
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
        self.levels
            .values()
            .chain(self.spells.values())
            .chain(self.slices.values())
            .map(Vec::len)
            .sum::<usize>()
            + staged
    }

    /// Whether the file holds no area.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A `PersistentAreaStatusEffect` component of a slice, before its volume is placed.
#[derive(Debug, Clone, PartialEq)]
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
    /// Its entity is active as the slice spawns (`IsRuntimeActive`; true when the entity names
    /// none).
    pub active: bool,
    /// The closed mesh of its detection entity's polygon prism ([`prism_mesh`]), when that entity
    /// has one.
    pub prism: Option<Geometry>,
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
    runtime_active: u32,
    vertices: u32,
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
            runtime_active: lowercase_crc32("IsRuntimeActive"),
            vertices: lowercase_crc32("Vertices"),
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

/// The closed mesh of a polygon prism with the corners `corners` (in its entity's plane) and
/// the height `height` (up from the plane): the two rings of corners, a fan of triangles on each
/// cap and two on each side. A fan of a simple polygon covers each inside point an odd number of
/// times, so [`mesh_contains`] holds for a polygon that is not convex too.
pub fn prism_mesh(corners: &[[f32; 2]], height: f32) -> Option<Geometry> {
    if corners.len() < MIN_CORNERS || height <= 0.0 {
        return None;
    }
    // Both rings' corners take a `u16` index.
    let count = u16::try_from(corners.len())
        .ok()
        .filter(|count| count.checked_mul(2).is_some())?;
    let points: Vec<Vec3> = corners
        .iter()
        .map(|[x, y]| [*x, *y, 0.0])
        .chain(corners.iter().map(|[x, y]| [*x, *y, height]))
        .collect();
    let mut triangles = Vec::new();
    for corner in 1..count - 1 {
        triangles.push([0, corner + 1, corner]);
        triangles.push([count, count + corner, count + corner + 1]);
    }
    for corner in 0..count {
        let next = (corner + 1) % count;
        triangles.push([corner, next, count + next]);
        triangles.push([corner, count + next, count + corner]);
    }
    Some(Geometry::Mesh { points, triangles })
}

/// Whether `local` is inside the closed mesh of `points` and `triangles`: a ray from it straight
/// up crosses its triangles an odd number of times (triangles that stand upright are not
/// crossed).
pub fn mesh_contains(points: &[Vec3], triangles: &[[u16; 3]], local: Vec3) -> bool {
    let [x, y, z] = local;
    let crossings = triangles
        .iter()
        .filter_map(|triangle| {
            let [a, b, c] = triangle.map(|corner| points.get(usize::from(corner)).copied());
            let (a, b, c) = (a?, b?, c?);
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if area.abs() <= f32::EPSILON {
                return None;
            }
            let u = ((b[0] - x) * (c[1] - y) - (c[0] - x) * (b[1] - y)) / area;
            let v = ((c[0] - x) * (a[1] - y) - (a[0] - x) * (c[1] - y)) / area;
            let w = 1.0 - u - v;
            let inside = u >= 0.0 && v >= 0.0 && w >= 0.0;
            let height = u * a[2] + v * b[2] + w * c[2];
            (inside && height > z).then_some(())
        })
        .count();
    crossings % 2 == 1
}

/// The corners and the height of `component`, a [`POLYGON_PRISM_TYPE`] component, as its closed
/// mesh; `None` when it has fewer than three corners or no height.
fn component_prism(component: &Element, names: &Names) -> Result<Option<Geometry>, EffectAreasError> {
    let Some(height) = find(component, SHAPE_HEIGHT_NAME, PRISM_DEPTH)
        .map(Element::as_f32)
        .transpose()?
    else {
        return Ok(None);
    };
    let corners = find(component, names.vertices, PRISM_DEPTH)
        .map(|vertices| {
            vertices
                .children
                .iter()
                .filter(|corner| corner.type_id == VECTOR2_TYPE)
                .filter_map(|corner| {
                    let bytes = corner.value.as_slice();
                    let x = bytes.get(..F32_LEN)?.try_into().ok()?;
                    let y = bytes.get(F32_LEN..2 * F32_LEN)?.try_into().ok()?;
                    Some([f32::from_be_bytes(x), f32::from_be_bytes(y)])
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(prism_mesh(&corners, height))
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
        active: true,
        prism: None,
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
    let mut prisms: HashMap<u64, Geometry> = HashMap::new();
    for entity in &entities.children {
        let Some(owner) = entity
            .child(names.id)
            .and_then(|id| id.child(names.id))
            .map(Element::as_u64)
            .transpose()?
        else {
            continue;
        };
        let active = entity
            .child(names.runtime_active)
            .map_or(Ok(true), Element::as_bool)?;
        let components = entity
            .child(names.components)
            .map(|list| list.children.as_slice())
            .unwrap_or_default();
        for component in components {
            if component.type_id == AREA_STATUS_EFFECT_TYPE
                && let Some(area) = slice_area(component, owner, &names)?
            {
                areas.push(SliceArea { active, ..area });
            } else if component.type_id == POLYGON_PRISM_TYPE
                && let Some(prism) = component_prism(component, &names)?
            {
                prisms.insert(owner, prism);
            }
        }
    }
    for area in &mut areas {
        area.prism = prisms.get(&area.detection_entity).cloned();
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
                active: true,
                prism: None,
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
            slices: BTreeMap::from([(HAZARD.to_owned(), vec![area.clone()])]),
        };
        assert_eq!(areas.of_level("newworld_vitaeeterna"), std::slice::from_ref(&area));
        assert!(areas.of_level("nw_catacomb_00").is_empty());
        assert_eq!(
            areas.of_spell("AI_Fulgoris_Blight_Self_Destruct_Catacombs"),
            std::slice::from_ref(&area)
        );
        assert_eq!(areas.of_stage("setup").collect::<Vec<_>>(), [&area]);
        assert_eq!(areas.of_stage("Stage_1").count(), 0);
        assert_eq!(areas.of_slice(HAZARD), std::slice::from_ref(&area));
        assert!(areas.of_slice("slices/other.dynamicslice").is_empty());
        assert_eq!(areas.len(), 4);
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
        assert!(old.stages.is_empty() && old.slices.is_empty() && old.is_empty());
        let mut turn = IDENTITY_AFFINE;
        turn[0][TRANSLATION_COLUMN] = 5.0;
        assert_eq!(area.placed(&turn).centre(), [15.0, 20.0, 30.0]);
    }

    /// A darkness hazard's slice path.
    const HAZARD: &str =
        "slices/darkness/events/general/hazards/darkness_hazard_corruption_minor_l4.dynamicslice";

    /// An L-shaped polygon prism 13 m high (the Myrkgard portal's hazard is a polygon of 17
    /// corners that is not convex): inside in the L, outside in its notch, below and above it.
    #[test]
    fn a_polygon_prism_holds_the_points_of_its_polygon_up_to_its_height() {
        const HEIGHT_M: f32 = 13.0;
        let corners = [[0.0, 0.0], [10.0, 0.0], [10.0, 4.0], [4.0, 4.0], [4.0, 10.0], [0.0, 10.0]];
        let prism = prism_mesh(&corners, HEIGHT_M).expect("prism");
        let area = EffectArea {
            effects: vec!["ApplyCurseL4".to_owned()],
            removes: Vec::new(),
            volume: Detector {
                shape: prism,
                place: IDENTITY_AFFINE,
            },
            rules: AreaRules::default(),
        };
        assert!(area.contains([2.0, 8.0, 1.0]), "in the upright arm");
        assert!(area.contains([8.0, 2.0, 12.0]), "in the flat arm");
        assert!(!area.contains([8.0, 8.0, 1.0]), "in the notch");
        assert!(!area.contains([2.0, 2.0, -1.0]), "below");
        assert!(!area.contains([2.0, 2.0, 14.0]), "above");
        assert!(prism_mesh(&corners[..2], HEIGHT_M).is_none(), "two corners");
        let moved = area.placed(&{
            let mut place = IDENTITY_AFFINE;
            place[2][TRANSLATION_COLUMN] = 100.0;
            place
        });
        assert!(moved.contains([2.0, 8.0, 101.0]) && !moved.contains([2.0, 8.0, 1.0]));
    }

    /// A polygon prism component's corners (`AZ::Vector2`, big-endian) and height; an area's
    /// entity that is not active as the slice spawns marks its area.
    #[test]
    fn a_slice_area_takes_its_detection_entitys_prism_and_its_activity() {
        const OWNER: u64 = 0x2b1f_a2d1_bc77;
        let corner = |x: f32, y: f32| {
            let mut element = value("element", &[x.to_be_bytes(), y.to_be_bytes()].concat());
            element.type_id = VECTOR2_TYPE;
            element
        };
        let mut prism = node(
            "element",
            vec![node(
                "Configuration",
                vec![node(
                    "PolygonPrism",
                    vec![node(
                        "element",
                        vec![
                            value("Height", &13.0_f32.to_be_bytes()),
                            node(
                                "VertexContainer",
                                vec![node(
                                    "Vertices",
                                    vec![corner(0.0, 0.0), corner(4.0, 0.0), corner(0.0, 4.0)],
                                )],
                            ),
                        ],
                    )],
                )],
            )],
        );
        prism.type_id = POLYGON_PRISM_TYPE;
        let mut slice = gas_slice(OWNER, OWNER, "ApplyCurseL4", &[]);
        let entity = &mut slice.children[0].children[0].children[0].children[0];
        entity.children.push(value("IsRuntimeActive", &[0]));
        entity.children[1].children.push(prism);
        let areas = slice_areas(&slice).expect("areas");
        assert_eq!(areas.len(), 1);
        assert!(!areas[0].active);
        let Some(Geometry::Mesh { points, .. }) = &areas[0].prism else {
            panic!("a prism");
        };
        assert_eq!(points.len(), 6);
        assert_eq!(points[4], [4.0, 0.0, 13.0]);
    }
}
