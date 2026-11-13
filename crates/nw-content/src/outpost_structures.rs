// SPDX-License-Identifier: AGPL-3.0-or-later
//! The structures of the Outpost Rush outposts (game mode helper B10, findings
//! `2026-11-13-outpost-rush-structures.md`): for each map the structure spawners of its layout
//! slice, for each structure slice the members retail sent of it, and the blueprint values of the
//! `OR_*` rows.
//!
//! Observed (client slices, `oprdoors` over `outpostrush_layout_004_trench`,
//! `gamemode_outpostrush_005_vertical` and `outpostrush_layout_00`): each outpost has the spawners
//! `Armory<X>`, `CommandPost<X>`, `DefensiveStructure<X>1-4` (the turret slices themselves),
//! `Gate<X>1-2` (Trench `1-3`), `ProtectionWard<X>` (outposts A and C only), `StorageShed<X>` and
//! `AIObjectiveManager<X>`; none spawns on its own (`m_spawnOnInitialize` off): the game mode
//! spawns them. Each tier of a structure is its own slice (`or_commandpost_t1`, `_t2`, `_t3`).
//!
//! The table file is mined from the retail captures (no tracked file holds capture data;
//! `blighted-emu-tools/scratch-work/oprstructures/mine.py`): per slice the first group retail
//! sent of it as a foundation (buildable state 4, or the first group of a slice with no buildable
//! state) and the first built one (state 5), each member's encoded bytes without the card, the
//! place and the ownership. A slice no capture showed has no template.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::Tables;

const FILE_PREFIX: &str = "outpost-structures-";
const FILE_SUFFIX: &str = ".json";
/// The bytes of an asset id.
pub const ASSET_LEN: usize = 16;
/// The outposts of a map: A, B and C.
pub const OUTPOSTS: usize = 3;
/// The prefix of the Outpost Rush blueprint rows.
const BLUEPRINT_PREFIX: &str = "OR_";
/// The number of status slots before the first defensive structure and before the first gate
/// (Observed: the client's `OutpostRush_OutpostStructure` order Armory, CommandPost,
/// DefensiveStructure1-4, Gate1-4, ProtectionWard, StorageShed, AIObjectiveManager).
const DEFENSIVE_BASE: u32 = 1;
const GATE_BASE: u32 = 5;
const ARMORY_SLOT: u32 = 0;
const COMMAND_POST_SLOT: u32 = 1;
const WARD_SLOT: u32 = 10;
const STORAGE_SLOT: u32 = 11;
const AI_MANAGER_SLOT: u32 = 12;
/// The most defensive structures and gates of an outpost (four status slots each).
const MOST_NUMBERED: u8 = 4;

/// The table file of client build `client_build` in content directory `dir`.
pub fn outpost_structures_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum OutpostStructuresError {
    /// The file cannot be read.
    #[error("outpost structures table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("outpost structures table: {0}")]
    Json(#[from] serde_json::Error),
    /// A hex field does not read.
    #[error("outpost structures table: {0}")]
    Hex(#[from] hex::FromHexError),
    /// An asset id has the wrong length.
    #[error("outpost structures table: an asset id of {0} bytes")]
    AssetLen(usize),
}

/// The part a structure plays in its outpost (the spawner's name).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructureRole {
    /// `Armory<X>`: the armory shop.
    Armory,
    /// `CommandPost<X>`.
    CommandPost,
    /// `DefensiveStructure<X><n>`: a turret.
    DefensiveStructure,
    /// `Gate<X><n>`.
    Gate,
    /// `ProtectionWard<X>`.
    ProtectionWard,
    /// `StorageShed<X>`.
    StorageShed,
    /// `AIObjectiveManager<X>`.
    AiObjectiveManager,
}

impl StructureRole {
    /// The status slot of the structure `number` (1 to 4 for defensive structures and gates) of
    /// this role in an outpost's structure status (three bits a slot; module doc); `None` for a
    /// number past four.
    pub const fn status_slot(self, number: u8) -> Option<u32> {
        match self {
            Self::Armory => Some(ARMORY_SLOT),
            Self::CommandPost => Some(COMMAND_POST_SLOT),
            Self::DefensiveStructure if number >= 1 && number <= MOST_NUMBERED => {
                Some(DEFENSIVE_BASE + number as u32)
            }
            Self::Gate if number >= 1 && number <= MOST_NUMBERED => Some(GATE_BASE + number as u32),
            Self::DefensiveStructure | Self::Gate => None,
            Self::ProtectionWard => Some(WARD_SLOT),
            Self::StorageShed => Some(STORAGE_SLOT),
            Self::AiObjectiveManager => Some(AI_MANAGER_SLOT),
        }
    }
}

/// The members retail sent of a structure slice in one state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// The component of its card.
    pub card_component: u64,
    /// The component of its place.
    pub place_component: u64,
    /// The component of its ownership, when it has one.
    pub ownership_component: Option<u64>,
    /// Every other member, each as one bundle member's bytes, as retail sent it.
    pub members: Vec<Vec<u8>>,
}

/// One structure slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureSlice {
    /// Its asset id.
    pub asset: [u8; ASSET_LEN],
    /// Its pak path.
    pub slice: String,
    /// Its blueprint row (`OR_Outpost_Gate_T1`, ...); `None` for a slice that is not built.
    pub blueprint: Option<String>,
    /// Its tier (1 for a slice of one tier).
    pub tier: u8,
    /// The members of its foundation.
    pub foundation: Option<Template>,
    /// The members of its built state.
    pub built: Option<Template>,
    /// Its templates are another slice's (Inferred: no capture shows this slice; the miner gives
    /// it the templates of the captured slice whose replicated components it shares, B14:
    /// `or_outpost_gate_sol_t2`/`_t3` those of `or_outpost_gate_t2`/`_t3`, `or_armory_t2`/`_t3`
    /// those of `or_armory_t1`).
    pub template_inferred: bool,
}

/// One structure spawner of a layout slice.
#[derive(Debug, Clone, PartialEq)]
pub struct StructureSpawner {
    /// The spawner's name (`GateA1`, ...).
    pub name: String,
    /// The outpost: 0 for A, 1 for B, 2 for C.
    pub outpost: u8,
    /// Its part in the outpost.
    pub role: StructureRole,
    /// Its number among the defensive structures or gates of its outpost (1 for the others).
    pub number: u8,
    /// The slice it spawns (the first tier).
    pub asset: [u8; ASSET_LEN],
    /// Its place in the layout slice, metres.
    pub place: [f32; 3],
    /// Its yaw, radians (`atan2(basis[1][0], basis[0][0])` of its world basis).
    pub yaw: f32,
}

/// The structures of one map.
#[derive(Debug, Clone, PartialEq)]
pub struct StructureMap {
    /// The map's level folder.
    pub folder: String,
    /// The structure spawners of its layout slice.
    pub spawners: Vec<StructureSpawner>,
}

/// The table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutpostStructures {
    slices: Vec<StructureSlice>,
    maps: Vec<StructureMap>,
}

#[derive(Deserialize)]
struct TableFile {
    kinds: Vec<SliceRow>,
    maps: Vec<MapRow>,
}

#[derive(Deserialize)]
struct SliceRow {
    asset: String,
    slice: String,
    blueprint: Option<String>,
    tier: u8,
    foundation: Option<TemplateRow>,
    built: Option<TemplateRow>,
    #[serde(default)]
    template_inferred: bool,
}

#[derive(Deserialize)]
struct TemplateRow {
    card_component: u64,
    place_component: u64,
    ownership_component: Option<u64>,
    members: Vec<String>,
}

#[derive(Deserialize)]
struct MapRow {
    folder: String,
    spawners: Vec<SpawnerRow>,
}

#[derive(Deserialize)]
struct SpawnerRow {
    name: String,
    outpost: u8,
    role: StructureRole,
    number: u8,
    asset: String,
    place: [f32; 3],
    yaw: f32,
}

fn asset_of(text: &str) -> Result<[u8; ASSET_LEN], OutpostStructuresError> {
    let bytes = hex::decode(text)?;
    let len = bytes.len();
    bytes
        .try_into()
        .map_err(|_| OutpostStructuresError::AssetLen(len))
}

fn template_of(row: TemplateRow) -> Result<Template, OutpostStructuresError> {
    Ok(Template {
        card_component: row.card_component,
        place_component: row.place_component,
        ownership_component: row.ownership_component,
        members: row
            .members
            .iter()
            .map(hex::decode)
            .collect::<Result<_, _>>()?,
    })
}

impl OutpostStructures {
    /// Loads the table of client build `client_build` from content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`OutpostStructuresError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, OutpostStructuresError> {
        let path = outpost_structures_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| OutpostStructuresError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`OutpostStructuresError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, OutpostStructuresError> {
        let file: TableFile = serde_json::from_str(text)?;
        let slices = file
            .kinds
            .into_iter()
            .map(|row| {
                Ok(StructureSlice {
                    asset: asset_of(&row.asset)?,
                    slice: row.slice,
                    blueprint: row.blueprint,
                    tier: row.tier,
                    foundation: row.foundation.map(template_of).transpose()?,
                    built: row.built.map(template_of).transpose()?,
                    template_inferred: row.template_inferred,
                })
            })
            .collect::<Result<_, OutpostStructuresError>>()?;
        let maps = file
            .maps
            .into_iter()
            .map(|map| {
                Ok(StructureMap {
                    folder: map.folder,
                    spawners: map
                        .spawners
                        .into_iter()
                        .map(|row| {
                            Ok(StructureSpawner {
                                name: row.name,
                                outpost: row.outpost,
                                role: row.role,
                                number: row.number,
                                asset: asset_of(&row.asset)?,
                                place: row.place,
                                yaw: row.yaw,
                            })
                        })
                        .collect::<Result<_, OutpostStructuresError>>()?,
                })
            })
            .collect::<Result<_, OutpostStructuresError>>()?;
        Ok(Self { slices, maps })
    }

    /// The slice of asset `asset`.
    pub fn slice(&self, asset: &[u8; ASSET_LEN]) -> Option<&StructureSlice> {
        self.slices.iter().find(|slice| &slice.asset == asset)
    }

    /// The structures of the map in level folder `folder`.
    pub fn map(&self, folder: &str) -> Option<&StructureMap> {
        self.maps
            .iter()
            .find(|map| map.folder.eq_ignore_ascii_case(folder))
    }

    /// The slice of the next tier of the structure slice `asset`: the slice whose blueprint is
    /// the same row family (`OR_Outpost_Gate_T1` -> `OR_Outpost_Gate_T2`) one tier up; `None` at
    /// the last tier or for a slice of one tier.
    pub fn next_tier(&self, asset: &[u8; ASSET_LEN]) -> Option<&StructureSlice> {
        let slice = self.slice(asset)?;
        let family = tier_family(slice.blueprint.as_deref()?, slice.tier)?;
        let wanted = format!("{family}{}", slice.tier + 1);
        self.slices.iter().find(|next| {
            next.blueprint
                .as_deref()
                .is_some_and(|blueprint| blueprint.eq_ignore_ascii_case(&wanted))
        })
    }

    /// The number of structure slices.
    pub fn len(&self) -> usize {
        self.slices.len()
    }

    /// Whether the table has no slice.
    pub fn is_empty(&self) -> bool {
        self.slices.is_empty()
    }
}

/// The row family of `blueprint` of `tier`: the id without its tier digit (`OR_CommandPost_T`);
/// `None` for a row whose id does not end with its tier.
fn tier_family(blueprint: &str, tier: u8) -> Option<&str> {
    blueprint.strip_suffix(&tier.to_string())
}

/// The build values of one `OR_*` blueprint row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutpostBlueprint {
    /// `HP`: the health of the built structure.
    pub health: f32,
    /// `BuildingTime`: the seconds a build from nothing to whole takes.
    pub build_time_s: f32,
}

/// The `OR_*` rows of `BlueprintItemDefinitions` (their costs are in
/// [`crate::crafting::StructureCosts`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutpostBlueprints {
    by_id: HashMap<String, OutpostBlueprint>,
}

impl OutpostBlueprints {
    /// The `OR_*` rows of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .blueprint_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter(|row| row.blueprint_id.starts_with(BLUEPRINT_PREFIX))
                .map(|row| {
                    (
                        row.blueprint_id.clone(),
                        OutpostBlueprint {
                            health: row.hp.unwrap_or_default() as f32,
                            build_time_s: row.building_time.unwrap_or_default() as f32,
                        },
                    )
                }),
        )
    }

    /// The rows `rows`: (blueprint id, values).
    pub fn build(rows: impl IntoIterator<Item = (String, OutpostBlueprint)>) -> Self {
        Self {
            by_id: rows
                .into_iter()
                .map(|(id, row)| (id.to_lowercase(), row))
                .collect(),
        }
    }

    /// The row `id` (any case).
    pub fn of(&self, id: &str) -> Option<OutpostBlueprint> {
        self.by_id.get(&id.to_lowercase()).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = r#"{"kinds": [
        {"asset": "acd041b7225f5076a4fdcd59d9934084", "slice": "slices/outpostrush/structures/or_commandpost_t1.dynamicslice",
         "blueprint": "OR_CommandPost_T1", "tier": 1,
         "foundation": {"card_component": 0, "place_component": 6, "ownership_component": 3, "members": ["0a0b"]},
         "built": null},
        {"asset": "b8cc8695516f5a7c94337190beeb6ccc", "slice": "slices/outpostrush/structures/or_commandpost_t2.dynamicslice",
         "blueprint": "OR_CommandPost_T2", "tier": 2, "foundation": null, "built": null,
         "template_inferred": true},
        {"asset": "a4276efd80865c7f958b74539ef54a4a", "slice": "slices/outpostrush/structures/or_storage.dynamicslice",
         "blueprint": null, "tier": 1, "foundation": null, "built": null}],
      "maps": [{"folder": "nw_opr_005_vertical", "spawners": [
        {"name": "CommandPostA", "outpost": 0, "role": "command_post", "number": 1,
         "asset": "acd041b7225f5076a4fdcd59d9934084", "place": [-58.476, 64.192, 107.155], "yaw": 0.5}]}]}"#;

    #[test]
    fn the_table_reads_and_a_tier_leads_to_the_next() {
        let table = OutpostStructures::parse(TABLE).expect("table");
        let first = [
            0xac, 0xd0, 0x41, 0xb7, 0x22, 0x5f, 0x50, 0x76, 0xa4, 0xfd, 0xcd, 0x59, 0xd9, 0x93,
            0x40, 0x84,
        ];
        let template = table
            .slice(&first)
            .and_then(|slice| slice.foundation.as_ref())
            .expect("template");
        assert_eq!(template.members, vec![vec![0x0a, 0x0b]]);
        assert_eq!(template.ownership_component, Some(3));
        assert!(!table.slice(&first).expect("tier 1").template_inferred, "captured");
        let next = table.next_tier(&first).expect("tier 2");
        assert_eq!(next.blueprint.as_deref(), Some("OR_CommandPost_T2"));
        assert!(next.template_inferred, "the miner's mark");
        assert!(table.next_tier(&next.asset).is_none());
        let map = table.map("NW_OPR_005_Vertical").expect("map");
        assert_eq!(map.spawners[0].role, StructureRole::CommandPost);
        assert!(OutpostStructures::parse(&TABLE.replace("acd041b7", "acd0")).is_err());
    }

    #[test]
    fn the_status_slots_follow_the_client_structure_order() {
        assert_eq!(StructureRole::Armory.status_slot(1), Some(0));
        assert_eq!(StructureRole::CommandPost.status_slot(1), Some(1));
        assert_eq!(StructureRole::DefensiveStructure.status_slot(1), Some(2));
        assert_eq!(StructureRole::DefensiveStructure.status_slot(4), Some(5));
        assert_eq!(StructureRole::Gate.status_slot(1), Some(6));
        assert_eq!(StructureRole::Gate.status_slot(4), Some(9));
        assert_eq!(StructureRole::Gate.status_slot(5), None);
        assert_eq!(StructureRole::ProtectionWard.status_slot(1), Some(10));
        assert_eq!(StructureRole::StorageShed.status_slot(1), Some(11));
        assert_eq!(StructureRole::AiObjectiveManager.status_slot(1), Some(12));
    }

    #[test]
    fn the_retail_or_rows_give_health_and_build_time() {
        let rows = OutpostBlueprints::build([(
            "OR_Outpost_Gate_T1".to_owned(),
            OutpostBlueprint {
                health: 45_000.0,
                build_time_s: 4.0,
            },
        )]);
        assert_eq!(
            rows.of("or_outpost_gate_t1").map(|row| row.build_time_s),
            Some(4.0)
        );
        assert!(rows.of("OR_Armory_T1").is_none());
    }

    /// The installed table (local data): every map has its structures and the slices of its
    /// spawners, and the gates lead to tier 3 (Observed slices `or_outpost_gate_t1..t3`).
    #[test]
    #[ignore = "needs local-data/content"]
    fn the_installed_table_holds_the_three_maps() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local-data/content");
        let table = OutpostStructures::load(&dir, "25810808").expect("table");
        for folder in ["nw_opr_004_trench", "nw_opr_005_vertical", "newworld_vitaeeterna"] {
            let map = table.map(folder).expect("map");
            assert!(map.spawners.len() >= 30, "{folder}");
            assert!(map.spawners.iter().all(|spawner| table.slice(&spawner.asset).is_some()));
        }
        let gate = table
            .slices
            .iter()
            .find(|slice| slice.blueprint.as_deref() == Some("OR_Outpost_Gate_T1"))
            .expect("gate");
        let second = table.next_tier(&gate.asset).expect("t2");
        assert!(table.next_tier(&second.asset).is_some());
    }

    /// The installed table (local data): every upgrade tier has a template, those no capture
    /// shows marked inferred (B14), so no upgrade is refused for want of one.
    #[test]
    #[ignore = "needs local-data/content"]
    fn every_upgrade_tier_of_the_installed_table_has_a_template() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local-data/content");
        let table = OutpostStructures::load(&dir, "25810808").expect("table");
        let mut inferred = Vec::new();
        for slice in &table.slices {
            let Some(next) = table.next_tier(&slice.asset) else {
                continue;
            };
            assert!(
                next.foundation.is_some() || next.built.is_some(),
                "{} has no template",
                next.slice
            );
            if next.template_inferred {
                inferred.push(next.slice.rsplit('/').next().unwrap_or_default().to_owned());
            }
        }
        inferred.sort();
        assert_eq!(
            inferred,
            [
                "or_armory_t2.dynamicslice",
                "or_armory_t3.dynamicslice",
                "or_outpost_gate_sol_t2.dynamicslice",
                "or_outpost_gate_sol_t3.dynamicslice",
            ]
        );
    }
}
