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

use std::path::PathBuf;

use serde::Deserialize;

use crate::instances::InstanceMode;
use crate::layout::{ContentLayout, DUNGEON_ENTRANCES};

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
    /// The table of `layout`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonEntrancesError`] when the file cannot be read or is not the table.
    pub fn load(layout: &ContentLayout) -> Result<Self, DungeonEntrancesError> {
        let path = layout.file(DUNGEON_ENTRANCES);
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

/// The lower-case words an instance entrance's slice name starts with, after its stage number
/// and a `Trial_` (Observed in the open world's capitals: `DungeonEntrance_Amrine`,
/// `DungeonEntranceTrial_Season_01`, `RaidEntrance_MonarchsBluffs00`,
/// `4_1_Trial_Entrance_QuestMedea`, `Entrance_QuestApophis_V2`).
pub const ENTRANCE_WORDS: [&str; 3] = ["dungeonentrance", "raidentrance", "entrance"];
/// The word before the entrance word of a story instance's entrance (`3_2_Trial_Entrance_...`).
const TRIAL_WORD: &str = "trial_";

/// What follows the entrance word in the lower-case slice file name `name`
/// (`_questmedea` of `4_1_trial_entrance_questmedea`); `None` for a slice that is no entrance.
pub fn entrance_rest(name: &str) -> Option<&str> {
    let name = name.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_');
    let name = name.strip_prefix(TRIAL_WORD).unwrap_or(name);
    ENTRANCE_WORDS
        .iter()
        .find_map(|word| name.strip_prefix(word))
}

/// The infix of a locked entrance (`DungeonEntrance_QuestAdiana_Locked_V2`): the same game mode.
const LOCKED_INFIX: &str = "_locked";
/// The version suffix of an entrance (`DungeonEntrance_QuestMammoth_V2`).
const VERSION_SUFFIX: &str = "_v2";
/// The `Dungeon` prefix of an expedition's game mode id (`DungeonAmrine` for `..._Amrine`).
const DUNGEON_PREFIX: &str = "dungeon";
/// The level folder part of an expedition map (`NW_Dungeon_Everfall_00` for `..._Everfall00`).
const MAP_PREFIXES: [&str; 2] = ["nw_dungeon_", "nw_"];

/// The game mode whose entrance slice is `slice_name`: the name after `DungeonEntrance`,
/// without `_Locked`, `_V2` and a trailing hash number, is the game mode id, the id less its
/// `Dungeon` prefix, or the map name less `NW_Dungeon_` or `NW_` (Inferred from the 40 entrance
/// slices of the open world: 35 name one game mode this way, e.g. the QuestMorgaine entrance
/// that retail answered with QuestMorgaine's 5038; the others name none or two and give none).
pub fn entrance_mode<'a>(
    modes: impl Iterator<Item = &'a InstanceMode>,
    slice_name: &str,
) -> Option<&'a InstanceMode> {
    // The last path part, without an extension (`.dynamicslice`).
    let name = slice_name
        .rsplit('/')
        .next()?
        .split('.')
        .next()?
        .to_lowercase();
    let rest = entrance_rest(&name)?
        .replace(LOCKED_INFIX, "")
        .replace(VERSION_SUFFIX, "");
    // A trailing number of more digits than a map number is a hash (`_1970794647`).
    let rest = match rest.rsplit_once('_') {
        Some((head, tail))
            if tail.len() > MAP_NUMBER_DIGITS && tail.chars().all(|c| c.is_ascii_digit()) =>
        {
            head.to_owned()
        }
        _ => rest,
    };
    let names: Vec<(&InstanceMode, [String; 3])> = modes
        .map(|mode| {
            let id = mode.id.to_lowercase();
            let map = mode.map.map_name.to_lowercase();
            let map = MAP_PREFIXES
                .iter()
                .find_map(|prefix| map.strip_prefix(prefix))
                .unwrap_or(&map);
            let names = [
                letters_and_digits(&id),
                letters_and_digits(id.strip_prefix(DUNGEON_PREFIX).unwrap_or(&id)),
                letters_and_digits(map),
            ];
            (mode, names)
        })
        .collect();
    // The name's words, longest first, with and without their number-only words: a word after
    // the mode's name is a variant (`Entrance_QuestIsabella_Simple`), a number-only word a stage
    // (`DungeonEntranceTrial_00_BrimstoneSandworm`). The first key that names any mode decides:
    // two modes for it give none.
    let words: Vec<&str> = rest.split('_').filter(|word| !word.is_empty()).collect();
    for count in (1..=words.len()).rev() {
        let head = &words[..count];
        let without_numbers: Vec<&str> = head
            .iter()
            .copied()
            .filter(|word| !word.chars().all(|c| c.is_ascii_digit()))
            .collect();
        for key in [
            letters_and_digits(&head.concat()),
            letters_and_digits(&without_numbers.concat()),
        ] {
            let mut found = names
                .iter()
                .filter(|(_, names)| names.contains(&key))
                .map(|(mode, _)| *mode);
            if let Some(mode) = found.next() {
                return found.next().is_none().then_some(mode);
            }
        }
    }
    None
}

/// Digits of a map number at the end of an entrance name (`brimstone00`).
const MAP_NUMBER_DIGITS: usize = 2;

fn letters_and_digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphanumeric).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{EntryCounter, InstanceKind, InstanceMap, NO_INDEX};

    /// A game mode of `id` on the map `map_name`, of `kind`; the entrances read only these.
    fn mode(id: &str, map_name: &str, kind: InstanceKind) -> InstanceMode {
        InstanceMode {
            id: id.to_owned(),
            crc: 0,
            kind,
            map: InstanceMap {
                folder: map_name.to_lowercase(),
                map_name: map_name.to_owned(),
                coatlicue_path: format!("coatlicue/{map_name}"),
                frame: [0.0, 0.0, 1.0, 1.0],
                teleports: Vec::new(),
                formation: Vec::new(),
                map_crc: 0,
            },
            capacity: 5,
            min_group: 3,
            required_level: None,
            remote_entry: true,
            respawns: true,
            mutable: false,
            timeout_seconds: None,
            victory_delay_seconds: None,
            items_removed_on_exit: Vec::new(),
            required_item_classes: Vec::new(),
            entry_achievement: None,
            loot_tags: Vec::new(),
            container_level: None,
            counter: EntryCounter::BaseDungeon,
            game_mode_index: NO_INDEX,
            generated: false,
            rotation: Vec::new(),
            rotation_span_seconds: None,
        }
    }

    fn modes() -> Vec<InstanceMode> {
        vec![
            mode(
                "QuestMedea",
                "NewWorld_VitaeEterna",
                InstanceKind::SoloTrial,
            ),
            mode(
                "QuestApophis",
                "NewWorld_VitaeEterna",
                InstanceKind::SoloTrial,
            ),
            mode(
                "QuestIsabella",
                "NW_ORI_SM_QuestIsabella",
                InstanceKind::SoloTrial,
            ),
            mode(
                "TrialBrimstoneSandworm",
                "NewWorld_VitaeEterna",
                InstanceKind::RaidTrial,
            ),
            mode(
                "DungeonAmrine",
                "NW_Dungeon_Windsward_00",
                InstanceKind::Expedition,
            ),
            mode(
                "DungeonShatteredObelisk",
                "NW_Dungeon_Everfall_00",
                InstanceKind::Expedition,
            ),
            mode(
                "QuestMorgaine",
                "NewWorld_VitaeEterna",
                InstanceKind::SoloTrial,
            ),
            mode(
                "TrialSeason04Q02",
                "NW_Trial_Season_04_DaichiDojo",
                InstanceKind::SoloTrial,
            ),
            mode(
                "TrialSeason01",
                "NW_Trial_Season_02",
                InstanceKind::RaidTrial,
            ),
            mode(
                "TrialSeason04",
                "NW_Trial_Season_04",
                InstanceKind::RaidTrial,
            ),
            mode(
                "TrialSeason04Q05",
                "NW_Trial_Season_04",
                InstanceKind::SoloTrial,
            ),
        ]
    }

    fn found(slice: &str) -> Option<String> {
        let modes = modes();
        entrance_mode(modes.iter(), slice).map(|mode| mode.id.clone())
    }

    #[test]
    fn entrance_slices_name_their_game_modes() {
        assert_eq!(
            found("Dungeon/Entrance/dungeonentrance_questmorgaine").as_deref(),
            Some("QuestMorgaine")
        );
        assert_eq!(
            found("dungeon/entrance/dungeonentrance_amrine").as_deref(),
            Some("DungeonAmrine")
        );
        assert_eq!(
            found("Dungeon/Entrance/DungeonEntrance_Everfall00").as_deref(),
            Some("DungeonShatteredObelisk"),
            "by the map name"
        );
        assert_eq!(
            found("Dungeon/Entrance/DungeonEntrance_TrialSeason04Q02_Locked_V2_1970794647")
                .as_deref(),
            Some("TrialSeason04Q02")
        );
        assert_eq!(
            found("Dungeon/Entrance/DungeonEntranceTrial_Season_01").as_deref(),
            Some("TrialSeason01")
        );
        // Two modes share the map NW_Trial_Season_04: no game mode.
        assert_eq!(
            found("Dungeon/Entrance/DungeonEntranceTrial_Season_04"),
            None
        );
        assert_eq!(found("Dungeon/Entrance/DungeonEntrance_FirstLight02"), None);
        // The story instance forms of the open world's capitals.
        assert_eq!(
            found("Dungeon/MedeaWindsward/4_1_Trial_Entrance_QuestMedea").as_deref(),
            Some("QuestMedea")
        );
        assert_eq!(
            found("Dungeon/MedeaWindsward/4__Trial_Entrance_QuestMedea_Locked").as_deref(),
            Some("QuestMedea")
        );
        assert_eq!(
            found("Dungeon/ApophisBrimstone/Entrance_QuestApophis_V2").as_deref(),
            Some("QuestApophis")
        );
        assert_eq!(
            found("Dungeon/IsabellaShattered/Entrance_QuestIsabella_Persisting_Collision_V2")
                .as_deref(),
            Some("QuestIsabella"),
            "a variant word after the name"
        );
        assert_eq!(
            found("Dungeon/Entrance/DungeonEntranceTrial_00_BrimstoneSandworm").as_deref(),
            Some("TrialBrimstoneSandworm"),
            "a stage number before the name"
        );
        assert_eq!(found("IO_Season_04_Q03_Boat_Entrance"), None);
        assert_eq!(found("POIs/Settlement/Townboard"), None);
    }

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
