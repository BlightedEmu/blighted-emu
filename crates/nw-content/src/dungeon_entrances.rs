// SPDX-License-Identifier: AGPL-3.0-or-later
//! The open world's dungeon entrances as network entities (dungeon entrance note, 2026-11-12).
//!
//! Observed (117 retail ledgers, 1,469 bodies of `Javelin::DungeonEntranceReplicatedState`,
//! class 3105): each expedition, raid and trial entrance capital of the open world is a network
//! entity that a player near it gets: a card of the level form (no asset, the capital's id with
//! the zero phase), the entrance state and the states of the slice's other replicated
//! components (`DetectionVolumeEventReplicatedState` 366, `SlayerScriptReplicatedState` 3362,
//! `DelayedEventComponentReplicatedState` 3139), each on the component number of its slice.
//! Each component keeps its number in every placement of one slice.
//!
//! The table file holds one row per entrance slice, mined from the retail captures (the
//! `entrancetable` bin of the capture tools): the slice name (lower case, as the level's
//! capitals name it), the component of its entrance state, and the slice's other spawn members
//! as retail sent them (the most common set and body of its cards). An entrance slice that no
//! capture shows has no row.

use std::path::{Path, PathBuf};

use serde::Deserialize;

const FILE_PREFIX: &str = "dungeon-entrances-";
const FILE_SUFFIX: &str = ".json";

/// The table file of client build `client_build` in content directory `dir`.
pub fn dungeon_entrances_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum DungeonEntrancesError {
    /// The file cannot be read.
    #[error("dungeon entrances table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("dungeon entrances table: {0}")]
    Json(#[from] serde_json::Error),
    /// A member's hex does not read.
    #[error("dungeon entrances table: slice {slice}: {source}")]
    Hex {
        /// The slice of the row.
        slice: String,
        /// Why.
        source: hex::FromHexError,
    },
}

#[derive(Deserialize)]
struct TableFile {
    entrances: Vec<EntranceRow>,
}

#[derive(Deserialize)]
struct EntranceRow {
    slice: String,
    entrance_component: u64,
    members: Vec<String>,
}

/// The network entity of one entrance slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntranceSlice {
    /// The slice name, lower case (`dungeon/entrance/dungeonentrance_amrine`).
    pub slice: String,
    /// The component number of its `DungeonEntranceReplicatedState`.
    pub entrance_component: u64,
    /// Its other spawn states, each as the bytes of one bundle member (component, class, body).
    pub members: Vec<Vec<u8>>,
}

/// The entrance slices of a level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DungeonEntrances {
    slices: Vec<EntranceSlice>,
}

impl DungeonEntrances {
    /// The table of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonEntrancesError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, DungeonEntrancesError> {
        let path = dungeon_entrances_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| DungeonEntrancesError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonEntrancesError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, DungeonEntrancesError> {
        let file: TableFile = serde_json::from_str(text)?;
        let slices = file
            .entrances
            .into_iter()
            .map(|row| {
                let members = row
                    .members
                    .iter()
                    .map(hex::decode)
                    .collect::<Result<_, _>>()
                    .map_err(|source| DungeonEntrancesError::Hex {
                        slice: row.slice.clone(),
                        source,
                    })?;
                Ok(EntranceSlice {
                    slice: row.slice.to_lowercase(),
                    entrance_component: row.entrance_component,
                    members,
                })
            })
            .collect::<Result<_, DungeonEntrancesError>>()?;
        Ok(Self { slices })
    }

    /// The row of the entrance slice named `slice` (any case).
    pub fn of_slice(&self, slice: &str) -> Option<&EntranceSlice> {
        let slice = slice.to_lowercase();
        self.slices.iter().find(|row| row.slice == slice)
    }

    /// How many slices it holds.
    pub const fn len(&self) -> usize {
        self.slices.len()
    }

    /// Whether it holds no slice.
    pub const fn is_empty(&self) -> bool {
        self.slices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The row of the Gawain entrance as the capture tools mined it from 6 retail cards: the
    /// slayer script on component 2, the detection volume on 4, the entrance state on 3.
    const TABLE: &str = r#"{"entrances": [{
        "cards": 6,
        "entrance_component": 3,
        "members": ["02a23401040001812000", "04ae05010f0002fe85b7befcd20002fe85b7ecc2eb0002fe85b7befcd20002fe85b7ecc2eb"],
        "slice": "dungeon/gawain/dungeonentrance_questgawain"
    }]}"#;

    #[test]
    fn a_row_reads_with_its_entrance_component_and_states() {
        let table = DungeonEntrances::parse(TABLE).expect("table");
        assert_eq!(table.len(), 1);
        let row = table
            .of_slice("Dungeon/Gawain/DungeonEntrance_QuestGawain")
            .expect("the row, in any case");
        assert_eq!(row.entrance_component, 3);
        assert_eq!(row.members.len(), 2);
        assert_eq!(
            row.members[0],
            hex::decode("02a23401040001812000").expect("hex")
        );
        assert!(table.of_slice("dungeon/entrance/other").is_none());
    }

    #[test]
    fn bad_hex_names_its_slice() {
        let text = r#"{"entrances": [{"slice": "a", "entrance_component": 2, "members": ["zz"]}]}"#;
        let error = DungeonEntrances::parse(text).expect_err("bad hex");
        assert!(error.to_string().contains("slice a"));
    }
}
