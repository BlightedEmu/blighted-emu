// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool dungeon-gates`: the gates file ([`nw_content::dungeon_gates`], dungeon gates
//! note 2026-11-11) from the game paks and the content bundle.
//!
//! Each instance level script ([`placed_scripts`]: the game mode's `SlicePath` slice where a
//! capital of its level places it) is read entity by entity. A spawner is a gate when its slice
//! has a replicated `GatherableControllerComponent` and its slice is an interact object or names
//! a gate part ([`GATE_PARTS`]: a gate, a door, a light bridge, a portal stone, a switch, a
//! pressure plate, rubble), or when its slice has a replicated position and no gatherable and its
//! entity or slice names a blocker ([`BLOCKER_PARTS`]: an arena blocker). The name test is
//! Inferred: the findings' 64 gate-like slice kinds of the captures all pass it. Loot
//! containers, checkpoints, gathering nodes, encounter slices and mob spawners are left out (the
//! dungeon script file has the encounters and the chests). Each gate keeps its name, its place
//! (the level-script placement after the spawner's own transform), its slice asset and the
//! component numbers of its position, gatherable and timelines, each timeline with its entity
//! name and length.
//!
//! For every encounter slice the level scripts spawn, the entities with a replicated timeline,
//! delayed event or placement obstruction are listed with their component numbers: the targets
//! of the stages' entity events.
//!
//! A timeline's length is the latest clip end (field `547e44c2`, name Unknown) of its
//! `.timeline` asset (Inferred: it gives the retail end bodies' play heads of the Amrine,
//! Starstone and Dynasty gates).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use nw_content::Tables;
use nw_content::dungeon_gates::{
    DungeonGates, EntityComponent, Gate, GateLevel, GateTimeline, SliceEntity, dungeon_gates_path,
};
use nw_formats::objectstream::Element;
use nw_world::level_script::{ReplicatedKind, ScriptEntity, SliceScript, Spawner, Transform};

use crate::collision::PakIndex;
use crate::dungeon_scripts::{
    DungeonScriptsBuildError, PakSlices, SliceSource, placed_scripts, position_of, read_script,
};

/// The name CRC of a timeline clip's end, seconds (a big-endian `f32`; the text is Unknown).
const CLIP_END_NAME: u32 = 0x547e_44c2;
/// The bytes of a clip end.
const CLIP_END_LEN: usize = size_of::<f32>();
/// The parts of a gate-like gatherable slice's path (lower case).
const GATE_PARTS: [&str; 12] = [
    "interactobject",
    "gate",
    "door",
    "wall",
    "fog",
    "bridge",
    "blocker",
    "seal",
    "portalstone",
    "switch",
    "pressureplate",
    "rubble",
];
/// The parts of a slice path that is no gate although it has a gatherable (lower case).
const NOT_GATE_PARTS: [&str; 3] = ["lootcontainer", "checkpoint", "/master_"];
/// The parts of an arena blocker's entity or slice name (lower case).
const BLOCKER_PARTS: [&str; 2] = ["blocker", "barrier"];
/// What the file says it was built from.
const SOURCE: &str = "content-tool dungeon-gates (game paks)";

/// What a build wrote and left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DungeonGatesReport {
    /// Level scripts written.
    pub levels: usize,
    /// Gates written.
    pub gates: usize,
    /// Gates without a gatherable (arena blockers), of [`Self::gates`].
    pub blockers: usize,
    /// Timelines written on gates.
    pub timelines: usize,
    /// Timelines whose asset is not in the paks (length 0).
    pub unread_timelines: usize,
    /// Encounter slices with replicated entities.
    pub encounter_slices: usize,
    /// Their replicated entities.
    pub encounter_entities: usize,
    /// Game modes without a placed script.
    pub skipped_modes: Vec<String>,
    /// Slice paths named but not in the paks.
    pub missing_slices: Vec<String>,
    /// The file written.
    pub path: PathBuf,
}

/// The latest clip end under `element`, seconds; `None` when it has none.
fn latest_clip_end(element: &Element) -> Option<f32> {
    let own = (element.name_crc == Some(CLIP_END_NAME) && element.value.len() == CLIP_END_LEN)
        .then(|| element.as_f32().ok())
        .flatten();
    element
        .children
        .iter()
        .filter_map(latest_clip_end)
        .chain(own)
        .reduce(f32::max)
}

/// Reads slices and timeline lengths once each.
struct Reader<'a> {
    slices: PakSlices<'a>,
    lengths: HashMap<String, f32>,
    report: DungeonGatesReport,
}

impl Reader<'_> {
    fn script(&mut self, path: &str) -> Result<Option<SliceScript>, DungeonScriptsBuildError> {
        read_script(&mut self.slices, path, &mut self.report.missing_slices)
    }

    /// The length of the timeline asset at `path`, seconds; 0 when it is not in the paks.
    fn length(&mut self, path: &str) -> Result<f32, DungeonScriptsBuildError> {
        if let Some(known) = self.lengths.get(path) {
            return Ok(*known);
        }
        let length = match self.slices.root(path)? {
            Some(root) => latest_clip_end(&root).unwrap_or_default(),
            None => {
                self.report.unread_timelines += 1;
                0.0
            }
        };
        self.lengths.insert(path.to_owned(), length);
        Ok(length)
    }

    /// The length of a timeline component of `kind`.
    fn timeline_length(&mut self, kind: &ReplicatedKind) -> Result<f32, DungeonScriptsBuildError> {
        match kind {
            ReplicatedKind::Timeline { asset: Some(asset) } => self.length(&asset.path),
            _ => Ok(0.0),
        }
    }

    /// The gate that `entity` (a spawner of `spawner`'s slice `slice`, placed at `placed`)
    /// spawns, when the slice is gate-like.
    fn gate(
        &mut self,
        entity: &ScriptEntity,
        spawner: &Spawner,
        spawned: &SliceScript,
        placed: &Transform,
    ) -> Result<Option<Gate>, DungeonScriptsBuildError> {
        let Some(slice) = &spawner.slice else {
            return Ok(None);
        };
        let component_of = |wanted: fn(&ReplicatedKind) -> bool| {
            spawned
                .entities
                .iter()
                .flat_map(|entity| &entity.replicated)
                .find(|component| wanted(&component.kind))
                .map(|component| u64::from(component.index))
        };
        let gatherable_component = component_of(|kind| matches!(kind, ReplicatedKind::Gatherable));
        let position_component = component_of(|kind| matches!(kind, ReplicatedKind::Position));
        let path = slice.path.to_lowercase();
        let named = |parts: &[&str], text: &str| parts.iter().any(|part| text.contains(part));
        let gate = gatherable_component.is_some()
            && named(&GATE_PARTS, &path)
            && !named(&NOT_GATE_PARTS, &path);
        let blocker = gatherable_component.is_none()
            && position_component.is_some()
            && (named(&BLOCKER_PARTS, &path) || named(&BLOCKER_PARTS, &entity.name.to_lowercase()));
        if !gate && !blocker {
            return Ok(None);
        }
        let mut timelines = Vec::new();
        for owner in &spawned.entities {
            for component in &owner.replicated {
                if matches!(component.kind, ReplicatedKind::Timeline { .. }) {
                    timelines.push(GateTimeline {
                        name: owner.name.clone(),
                        component: u64::from(component.index),
                        length_s: self.timeline_length(&component.kind)?,
                    });
                }
            }
        }
        self.report.gates += 1;
        self.report.blockers += usize::from(blocker);
        self.report.timelines += timelines.len();
        // Degrees of a yaw.
        #[allow(clippy::cast_possible_truncation, reason = "degrees")]
        let yaw_deg = placed.yaw_degrees() as f32;
        Ok(Some(Gate {
            name: entity.name.clone(),
            entity: entity.id,
            slice: slice.path.clone(),
            asset: slice.guid,
            position: position_of(placed),
            yaw_deg,
            position_component,
            gatherable_component,
            timelines,
        }))
    }

    /// The replicated entities of the encounter slice `script`.
    fn slice_entities(
        &mut self,
        script: &SliceScript,
    ) -> Result<Vec<SliceEntity>, DungeonScriptsBuildError> {
        let mut entities = Vec::new();
        for entity in &script.entities {
            let mut components = Vec::new();
            for component in &entity.replicated {
                let index = u64::from(component.index);
                components.push(match &component.kind {
                    ReplicatedKind::Timeline { .. } => EntityComponent::Timeline {
                        component: index,
                        length_s: self.timeline_length(&component.kind)?,
                    },
                    ReplicatedKind::DelayedEvent { delay_s } => EntityComponent::DelayedEvent {
                        component: index,
                        delay_s: *delay_s,
                    },
                    ReplicatedKind::PlacementObstruction => {
                        EntityComponent::PlacementObstruction { component: index }
                    }
                    ReplicatedKind::Position | ReplicatedKind::Gatherable => continue,
                });
            }
            if !components.is_empty() {
                entities.push(SliceEntity {
                    entity: entity.id,
                    name: entity.name.clone(),
                    components,
                });
            }
        }
        Ok(entities)
    }
}

/// Builds the gates file of `tables` from the paks of `index` into `out_dir`.
///
/// # Errors
///
/// Returns [`DungeonScriptsBuildError`] when a pak entry cannot be read or decoded, or the file
/// cannot be written or reads back other.
pub fn build_dungeon_gates(
    index: &PakIndex,
    tables: &Tables,
    client_build: &str,
    out_dir: &Path,
) -> Result<DungeonGatesReport, DungeonScriptsBuildError> {
    let mut reader = Reader {
        slices: PakSlices::new(index),
        lengths: HashMap::new(),
        report: DungeonGatesReport::default(),
    };
    let placed = placed_scripts(index, tables, &mut reader.report.skipped_modes)?;
    let mut levels = Vec::new();
    let mut encounter_entities: BTreeMap<String, Vec<SliceEntity>> = BTreeMap::new();
    for script in placed {
        let Some(level_script) = reader.script(&script.script)? else {
            reader.report.skipped_modes.extend(script.modes);
            continue;
        };
        let mut level = GateLevel {
            script_slice: script.script.clone(),
            level: script.folder.clone(),
            game_modes: script.modes,
            gates: Vec::new(),
        };
        for entity in &level_script.entities {
            let Some(spawner) = &entity.spawner else {
                continue;
            };
            let Some(slice) = &spawner.slice else {
                continue;
            };
            let Some(spawned) = reader.script(&slice.path)? else {
                continue;
            };
            if spawned.manager().is_some() {
                if !encounter_entities.contains_key(&slice.path) {
                    let entities = reader.slice_entities(&spawned)?;
                    if !entities.is_empty() {
                        reader.report.encounter_slices += 1;
                        reader.report.encounter_entities += entities.len();
                        encounter_entities.insert(slice.path.clone(), entities);
                    }
                }
                continue;
            }
            if spawned.loot_table().is_some() || spawned.variant().is_some() {
                continue;
            }
            let placed = script.placement.then(&level_script.placement_of(entity.id));
            level
                .gates
                .extend(reader.gate(entity, spawner, &spawned, &placed)?);
        }
        reader.report.levels += 1;
        levels.push(level);
    }
    let gates = DungeonGates::new(SOURCE.to_owned(), levels, encounter_entities);
    let path = dungeon_gates_path(out_dir, client_build);
    gates.write(&path)?;
    if DungeonGates::load(out_dir, client_build)? != gates {
        return Err(DungeonScriptsBuildError::Reload(path));
    }
    reader.report.path = path;
    Ok(reader.report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip_end(seconds: f32) -> Element {
        Element::with_value(CLIP_END_NAME, seconds.to_be_bytes().to_vec())
    }

    #[test]
    fn a_timeline_lasts_until_its_latest_clip_end() {
        const TRACK: u32 = 0x10;
        const CLIP: u32 = 0x11;
        let root = Element::with_children(
            TRACK,
            vec![
                Element::with_children(CLIP, vec![clip_end(6.0)]),
                Element::with_children(CLIP, vec![clip_end(8.0), clip_end(2.0)]),
            ],
        );
        assert_eq!(latest_clip_end(&root), Some(8.0));
        assert_eq!(
            latest_clip_end(&Element::with_children(TRACK, Vec::new())),
            None
        );
    }
}
