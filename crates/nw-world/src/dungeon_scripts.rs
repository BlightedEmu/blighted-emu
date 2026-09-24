// SPDX-License-Identifier: AGPL-3.0-or-later
//! The dungeon script file (`dungeon-scripts-<build>.json`, written by `content-tool
//! dungeon-scripts`, read by the instance sims): per instance game mode, its level's encounters (from the
//! encounter slices that the game mode's level script slice spawns, [`crate::level_script`]) in
//! world coordinates, with every spawn and tracked target resolved to the slices it may spawn
//! and the mob each one is, and its loot containers.
//!
//! An encounter is a tree of stages. Its manager's stages run in order; a stage runs its spawn
//! timeline, waits for its objectives and its child stages (in order), then runs its end
//! actions. Indexes in the file point into the encounter's own `stages` and `targets`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::slice_data::GUID_LEN;

/// The file name prefix; the client build and [`DUNGEON_SCRIPTS_EXTENSION`] follow.
pub const DUNGEON_SCRIPTS_PREFIX: &str = "dungeon-scripts-";
/// The file extension.
pub const DUNGEON_SCRIPTS_EXTENSION: &str = ".json";

/// The mob a spawned slice is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetMob {
    /// The vitals row id.
    pub vitals_id: String,
    /// The vitals category row id.
    pub category_id: String,
    /// The creature level (the spawner's, else the vitals row's).
    pub level: u32,
}

/// One slice a spawn or a tracked target may be.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetSlice {
    /// The pak path of the slice.
    pub path: String,
    /// Its asset GUID as stored.
    pub guid: [u8; GUID_LEN],
    /// Its weight in its alias (`Percent`); 0 for a slice named directly.
    pub percent: u32,
    /// The mob its spawner slice names; `None` for a creature slice (the sim looks its GUID up
    /// in the mob prefab table) or a slice that is no mob.
    pub mob: Option<TargetMob>,
}

/// The slices of one `m_sliceAsset` / `m_aliasAsset` pair.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    /// The slice's path or the alias asset's path.
    pub name: String,
    /// Its slices (one for a slice, the alias's entries for an alias).
    pub slices: Vec<TargetSlice>,
}

/// One spawn of a stage's timeline.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    /// Index of its target in [`Encounter::targets`].
    pub target: usize,
    /// Seconds after the stage started.
    pub delay_s: u32,
    /// How many.
    pub count: u32,
    /// Where (world metres), in order; none spawns at the encounter's place.
    pub locations: Vec<[f32; 3]>,
}

/// What an objective waits for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectiveSpec {
    /// `required` deaths of the target (`None`: of the stage's own spawns).
    SliceDestroyed {
        /// Index in [`Encounter::targets`].
        target: Option<usize>,
        /// Deaths needed.
        required: u32,
    },
    /// The target (`None`: a spawn of the encounter) reaches boss phase `phase`.
    BossPhase {
        /// Index in [`Encounter::targets`].
        target: Option<usize>,
        /// The phase.
        phase: u32,
    },
    /// A mob tree of the encounter sends `event`.
    EncounterEvent {
        /// The event id.
        event: String,
    },
    /// An entity of the encounter slice is activated or deactivated by an action (both flags
    /// unset: triggered).
    EntityActiveState {
        /// The entity id.
        entity: u64,
        /// Completes on `Activate`.
        on_activated: bool,
        /// Completes on `Deactivate`.
        on_deactivated: bool,
    },
    /// Completes with its stage's parent.
    WaitForParent,
    /// Every spawn of the stage's timeline spawned and died.
    InvasionWave,
}

/// One objective of a stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageObjective {
    /// Seconds; 0 for no limit.
    pub time_limit_s: u32,
    /// The limit completes it (else the limit fails the stage).
    pub succeed_on_timeout: bool,
    /// Its completion fails the stage.
    pub failure: bool,
    /// What it waits for.
    pub kind: ObjectiveSpec,
}

/// One start or end action of a stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StageAction {
    /// An entity event (`EventType`: 0 trigger, 1 untrigger, 2 activate, 3 deactivate).
    EntityEvent {
        /// The entity id.
        entity: u64,
        /// The `EventType` value.
        event: u32,
        /// Also its child entities.
        recursive: bool,
        /// Only for the local player.
        local_player_only: bool,
    },
    /// The encounter's spawns of the target go (`None`: every spawn of the encounter).
    DestroySlice {
        /// Index in [`Encounter::targets`].
        target: Option<usize>,
    },
    /// Every spawn of the stage dies.
    KillSpawns,
}

/// One stage of an encounter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    /// The stage entity's name.
    pub name: String,
    /// The stage entity's id.
    pub entity: u64,
    /// Its children may stay unfinished.
    pub child_stages_optional: bool,
    /// Indexes of its child stages in [`Encounter::stages`], in order.
    pub children: Vec<usize>,
    /// Its spawn timeline.
    pub spawns: Vec<Spawn>,
    /// Its objectives.
    pub objectives: Vec<StageObjective>,
    /// Its start actions.
    pub start_actions: Vec<StageAction>,
    /// Its end actions.
    pub end_actions: Vec<StageAction>,
}

/// One encounter of a level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Encounter {
    /// The level script entity that spawns it.
    pub name: String,
    /// The encounter slice's pak path.
    pub slice: String,
    /// Its place (world metres).
    pub position: [f32; 3],
    /// Its yaw, degrees.
    pub yaw_deg: f32,
    /// It runs from the instance's start.
    pub auto_activate: bool,
    /// Milliseconds from its activation to its first stage.
    pub auto_activate_delay_ms: u32,
    /// Seconds a stage waits with no player in the volume before the encounter resets.
    pub grace_period_s: u32,
    /// The tree event that resets it; empty for none.
    pub reset_event: String,
    /// The radius of its player volume, metres.
    pub detection_radius_m: Option<f32>,
    /// Indexes of its top stages in [`Encounter::stages`], in order.
    pub top_stages: Vec<usize>,
    /// Every stage.
    pub stages: Vec<Stage>,
    /// Every spawn or tracked target.
    pub targets: Vec<Target>,
}

/// A loot container of a level script.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Chest {
    /// The level script entity that spawns it.
    pub name: String,
    /// The container slice's pak path.
    pub slice: String,
    /// Its place (world metres).
    pub position: [f32; 3],
    /// Its loot table.
    pub loot_table: String,
}

/// The script of one instance game mode.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LevelScript {
    /// The level folder.
    pub level: String,
    /// The level script slice's pak path.
    pub script_slice: String,
    /// Its encounters.
    pub encounters: Vec<Encounter>,
    /// Its loot containers.
    pub chests: Vec<Chest>,
}

/// The scripts of every instance game mode, by game mode id (several modes share a level,
/// each with its own script: the soul trials of the open world map, the season trials).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DungeonScripts {
    /// The scripts by game mode id.
    pub game_modes: BTreeMap<String, LevelScript>,
}

/// Why the file cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum DungeonScriptsError {
    /// The file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not dungeon scripts.
    #[error("{path}: {source}")]
    Json {
        /// The file.
        path: PathBuf,
        /// Why.
        source: serde_json::Error,
    },
}

/// The dungeon script file of `client_build` in `dir`.
pub fn dungeon_scripts_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!(
        "{DUNGEON_SCRIPTS_PREFIX}{client_build}{DUNGEON_SCRIPTS_EXTENSION}"
    ))
}

impl DungeonScripts {
    /// Reads `path`; a missing file has no scripts (content from before this file).
    ///
    /// # Errors
    ///
    /// Returns [`DungeonScriptsError`] when the file cannot be read or is not dungeon scripts.
    pub fn load(path: &Path) -> Result<Self, DungeonScriptsError> {
        match std::fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|source| DungeonScriptsError::Json {
                    path: path.to_path_buf(),
                    source,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(DungeonScriptsError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Writes the file to `path`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonScriptsError`] when a value cannot be written or the file cannot be.
    pub fn write(&self, path: &Path) -> Result<(), DungeonScriptsError> {
        let text =
            serde_json::to_string_pretty(self).map_err(|source| DungeonScriptsError::Json {
                path: path.to_path_buf(),
                source,
            })?;
        std::fs::write(path, text).map_err(|source| DungeonScriptsError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// The script of game mode `id`.
    pub fn game_mode(&self, id: &str) -> Option<&LevelScript> {
        self.game_modes.get(id)
    }

    /// Whether a game mode on level `folder` has a script.
    pub fn has_level(&self, folder: &str) -> bool {
        self.game_modes.values().any(|script| script.level == folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: &str = "25810808";
    const LEVEL: &str = "nw_dungeon_windsward_00";
    const MODE: &str = "DungeonAmrine";

    fn scripts() -> DungeonScripts {
        let encounter = Encounter {
            name: "EncounterBoss".to_owned(),
            slice: "slices/dungeon_eventmanager/encounters/dg_enc_windsward_00_boss.dynamicslice"
                .to_owned(),
            position: [886.1, 906.0, 141.0],
            auto_activate: true,
            grace_period_s: 20,
            detection_radius_m: Some(30.0),
            top_stages: vec![0],
            stages: vec![Stage {
                name: "Wave_01".to_owned(),
                spawns: vec![Spawn {
                    target: 0,
                    count: 1,
                    ..Spawn::default()
                }],
                objectives: vec![StageObjective {
                    time_limit_s: 0,
                    succeed_on_timeout: false,
                    failure: false,
                    kind: ObjectiveSpec::SliceDestroyed {
                        target: Some(0),
                        required: 1,
                    },
                }],
                end_actions: vec![StageAction::KillSpawns],
                ..Stage::default()
            }],
            targets: vec![Target {
                name: "boss".to_owned(),
                slices: vec![TargetSlice {
                    path: "boss".to_owned(),
                    guid: [1; GUID_LEN],
                    percent: 0,
                    mob: Some(TargetMob {
                        vitals_id: "Undead_Brute_Boss".to_owned(),
                        category_id: String::new(),
                        level: 25,
                    }),
                }],
            }],
            ..Encounter::default()
        };
        DungeonScripts {
            game_modes: BTreeMap::from([(
                MODE.to_owned(),
                LevelScript {
                    level: LEVEL.to_owned(),
                    script_slice: "slices/dungeon/levelscripts/amrinedungeon.dynamicslice"
                        .to_owned(),
                    encounters: vec![encounter],
                    chests: vec![Chest {
                        name: "LockedChest_Simon".to_owned(),
                        loot_table: "ChestLootElite".to_owned(),
                        ..Chest::default()
                    }],
                },
            )]),
        }
    }

    #[test]
    fn the_file_writes_and_reads_back() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dungeon_scripts_path(dir.path(), BUILD);
        assert!(path.ends_with(format!("dungeon-scripts-{BUILD}.json")));
        let written = scripts();
        written.write(&path).expect("write");
        let read = DungeonScripts::load(&path).expect("load");
        assert_eq!(read, written);
        assert_eq!(read.game_mode(MODE).map(|script| script.encounters.len()), Some(1));
        assert!(read.game_mode("QuestCaerDun").is_none());
        assert!(read.has_level(LEVEL) && !read.has_level("newworld_vitaeeterna"));
    }

    #[test]
    fn a_missing_file_has_no_scripts_and_a_bad_one_is_an_error() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dungeon_scripts_path(dir.path(), BUILD);
        assert_eq!(DungeonScripts::load(&path).expect("missing"), DungeonScripts::default());
        std::fs::write(&path, "not json").expect("write");
        assert!(matches!(
            DungeonScripts::load(&path),
            Err(DungeonScriptsError::Json { .. })
        ));
    }
}
