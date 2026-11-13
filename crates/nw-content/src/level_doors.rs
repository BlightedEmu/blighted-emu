// SPDX-License-Identifier: AGPL-3.0-or-later
//! The doors of a level's capitals (level doors finding 2026-11-13): every capital whose slice
//! has a replicated door component, with the replicated components of that slice whose states
//! retail sends when a player comes near.
//!
//! Observed (148 retail ledgers, 328 door lives whose card names no asset): each such card's
//! `GdeRef` is a level capital's id (`nw_world::capitals::capital_id`, its first 8 bytes) and a
//! phase id (its last 8): 36 `GdeRef`s, 21 capitals of `newworld_vitaeeterna`, no miss. The client
//! has the slice in its level data, so the card names no asset, and the server sends the states
//! of the slice's replicated components that differ from the client's start: the door, and on
//! some slices a slayer script, timelines, delayed events, a placement obstruction or a
//! gatherable (the table of the finding).
//!
//! The file ([`LevelDoors`], `content-tool level-doors`, from the game paks and the level's world
//! file) holds one row per door capital: its id, its place, its slice and the component numbers
//! (`m_replicationIndex`) of the kinds of [`DoorComponentKind`]. Other replicated components (the
//! position, interact states, map markers) are left out: retail sent none of them at a door's
//! spawn (Observed on all 21 capitals).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE_PREFIX: &str = "level-doors-";
const FILE_SUFFIX: &str = ".json";

/// The level doors file of client build `client_build` and level `level` in content directory
/// `dir`.
pub fn level_doors_path(dir: &Path, client_build: &str, level: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}-{level}{FILE_SUFFIX}"))
}

/// A bad level doors file.
#[derive(Debug, thiserror::Error)]
pub enum LevelDoorsError {
    /// The file cannot be read or written.
    #[error("level doors file {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("level doors file: {0}")]
    Json(#[from] serde_json::Error),
}

/// What a replicated component of a door slice is (by the component's type in the slice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoorComponentKind {
    /// `{0FF532C2-E078-4F5F-A559-3473A4AEB5B5}` (entity `DoorFunctionality`): its replica is the
    /// `DoorComponentReplicatedState` (class 2330).
    Door,
    /// `{3D939617-3F24-41DC-A05F-2F298E8F4CB8}`: its replica is the `SlayerScriptReplicatedState`
    /// (class 3362).
    SlayerScript,
    /// A timeline (class 1647).
    Timeline,
    /// A delayed event (class 3139).
    DelayedEvent,
    /// A placement obstruction (class 2187).
    PlacementObstruction,
    /// A `GatherableControllerComponent` (class 12).
    Gatherable,
}

/// One replicated component of a door slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoorComponent {
    /// Its component number on the wire (`m_replicationIndex`).
    pub component: u64,
    /// What it is.
    pub kind: DoorComponentKind,
}

/// One capital of the level whose slice has a door.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelDoor {
    /// The capital's entity id (`capital_id` of its GUID): the first 8 bytes of its `GdeRef`.
    pub capital: u64,
    /// Where it stands (`worldPosition`).
    pub position: [f32; 3],
    /// The capital's slice pak path, lower case with `/`.
    pub slice: String,
    /// The slice's replicated components of the kinds kept, in component order.
    pub components: Vec<DoorComponent>,
}

impl LevelDoor {
    /// The component numbers of its components of `kind`, in order.
    pub fn components_of(&self, kind: DoorComponentKind) -> impl Iterator<Item = u64> + '_ {
        self.components
            .iter()
            .filter(move |component| component.kind == kind)
            .map(|component| component.component)
    }
}

/// The door capitals of one level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LevelDoors {
    /// What made the file.
    pub source: String,
    /// The level folder.
    pub level: String,
    /// The door capitals, in the capitals' file order.
    pub doors: Vec<LevelDoor>,
}

impl LevelDoors {
    /// The file of client build `client_build` and level `level` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`LevelDoorsError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str, level: &str) -> Result<Self, LevelDoorsError> {
        let path = level_doors_path(dir, client_build, level);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| LevelDoorsError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The file in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`LevelDoorsError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, LevelDoorsError> {
        Ok(serde_json::from_str(text)?)
    }

    /// Writes it to `path`.
    ///
    /// # Errors
    ///
    /// Returns [`LevelDoorsError`] when the file cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), LevelDoorsError> {
        std::fs::write(path, serde_json::to_string_pretty(self)?).map_err(|source| {
            LevelDoorsError::Io {
                path: path.to_owned(),
                source,
            }
        })
    }

    /// How many door capitals the level has.
    pub fn len(&self) -> usize {
        self.doors.len()
    }

    /// Whether the level has no door capital.
    pub fn is_empty(&self) -> bool {
        self.doors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up level with one door capital shaped as the 17DW gate (slayer script on 2, door
    /// on 5).
    const DOORS: &str = r#"{
        "source": "test",
        "level": "newworld_vitaeeterna",
        "doors": [
            {
                "capital": 1732609537381872043,
                "position": [3402.029, 5808.956, 240.55014],
                "slice": "coatgen/f46c8fa3/master_simpledoor_teleport_a_17dw_msq_gate_3362621797.dynamicslice",
                "components": [
                    {"component": 2, "kind": "slayer_script"},
                    {"component": 5, "kind": "door"}
                ]
            }
        ]
    }"#;

    #[test]
    fn the_file_reads_its_door_capitals_and_their_components() {
        let doors = LevelDoors::parse(DOORS).expect("parse");
        assert_eq!(doors.len(), 1);
        let door = &doors.doors[0];
        assert_eq!(
            door.components_of(DoorComponentKind::Door)
                .collect::<Vec<_>>(),
            [5]
        );
        assert_eq!(
            door.components_of(DoorComponentKind::SlayerScript)
                .collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(door.components_of(DoorComponentKind::Timeline).count(), 0);
    }

    #[test]
    fn a_written_file_reads_back_the_same() {
        let doors = LevelDoors::parse(DOORS).expect("parse");
        let dir = std::env::temp_dir().join(format!("nw-level-doors-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = level_doors_path(&dir, "1", "newworld_vitaeeterna");
        doors.write(&path).expect("write");
        let read = LevelDoors::load(&dir, "1", "newworld_vitaeeterna").expect("load");
        std::fs::remove_dir_all(&dir).expect("clean");
        assert_eq!(read, doors);
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let dir = std::env::temp_dir().join("nw-level-doors-none");
        assert!(matches!(
            LevelDoors::load(&dir, "1", "nowhere"),
            Err(LevelDoorsError::Io { .. })
        ));
    }
}
