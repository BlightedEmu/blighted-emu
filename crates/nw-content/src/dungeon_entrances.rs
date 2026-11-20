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
/// and a `Trial_`, each with the game mode id prefix of its kind (Observed in the open world's
/// capitals: `DungeonEntrance_Amrine` of `DungeonAmrine`, `RaidEntrance_MonarchsBluffs00` of
/// `RaidMonarchsBluffs00`, `DungeonEntranceTrial_Season_01`, `4_1_Trial_Entrance_QuestMedea`,
/// `Entrance_QuestApophis_V2`).
const ENTRANCE_WORDS: [(&str, &str); 3] = [
    ("dungeonentrance", DUNGEON_PREFIX),
    ("raidentrance", RAID_PREFIX),
    ("entrance", DUNGEON_PREFIX),
];
/// The word before the entrance word of a story instance's entrance (`3_2_Trial_Entrance_...`).
const TRIAL_WORD: &str = "trial_";

/// The game mode id prefix of the entrance word and what follows that word in the lower-case
/// slice file name `name` (`_questmedea` of `4_1_trial_entrance_questmedea`); `None` for a slice
/// that is no entrance.
fn entrance_parts(name: &str) -> Option<(&'static str, &str)> {
    let name = name.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_');
    let name = name.strip_prefix(TRIAL_WORD).unwrap_or(name);
    ENTRANCE_WORDS
        .iter()
        .find_map(|(word, prefix)| Some((*prefix, name.strip_prefix(word)?)))
}

/// What follows the entrance word in the lower-case slice file name `name`
/// (`_questmedea` of `4_1_trial_entrance_questmedea`); `None` for a slice that is no entrance.
pub fn entrance_rest(name: &str) -> Option<&str> {
    entrance_parts(name).map(|(_, rest)| rest)
}

/// The infix of a locked entrance (`DungeonEntrance_QuestAdiana_Locked_V2`): the same game mode.
const LOCKED_INFIX: &str = "_locked";
/// The version suffix of an entrance (`DungeonEntrance_QuestMammoth_V2`).
const VERSION_SUFFIX: &str = "_v2";
/// The `Dungeon` prefix of an expedition's game mode id (`DungeonAmrine` for `..._Amrine`).
const DUNGEON_PREFIX: &str = "dungeon";
/// The `Raid` prefix of a raid's game mode id (`RaidMonarchsBluffs00` for `RaidEntrance_...`).
const RAID_PREFIX: &str = "raid";
/// The level folder part of an expedition map (`NW_Dungeon_Everfall_00` for `..._Everfall00`).
const MAP_PREFIXES: [&str; 2] = ["nw_dungeon_", "nw_"];

/// An instance entrance of a level: its slice and the dungeon area it stands in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntranceDoor {
    /// The capital's slice name (`Dungeon/Entrance/DungeonEntrance_Amrine`).
    pub slice: String,
    /// The game mode id of the dungeon area that holds the door, when one does.
    pub area_mode: Option<String>,
}

impl EntranceDoor {
    /// The game mode among `modes` that the door opens ([`entrance_mode`]).
    pub fn mode<'a>(
        &self,
        modes: impl Iterator<Item = &'a InstanceMode>,
    ) -> Option<&'a InstanceMode> {
        entrance_mode(modes, &self.slice, self.area_mode.as_deref())
    }
}

/// The game mode whose entrance slice is `slice_name`, standing in the dungeon area of game mode
/// `area_mode` (the territory definition's `GameMode` of the door's place, when it has one).
///
/// The name decides first: the name after the entrance word, without `_Locked`, `_V2` and a
/// trailing hash number, is the game mode id or the id less the prefix of the entrance word
/// (`Dungeon`, `Raid`); else the map name less `NW_Dungeon_` or `NW_`; else, for a name with no
/// map number, the id less its number (`DungeonEntrance_CutlassKeys` of `DungeonCutlassKeys00`).
/// A name that gives none or two modes leaves the area's mode (Inferred from the 60 entrance
/// slices of the open world: the name gives the mode of 58, among them the QuestMorgaine
/// entrance that retail answered with QuestMorgaine's 5038; `DungeonEntrance_Brimstone00` and
/// `DungeonEntrance_FirstLight02` name none and stand 9 and 10 m from the points of the areas
/// of The Ennead and the Hive of Gorgons).
pub fn entrance_mode<'a>(
    modes: impl Iterator<Item = &'a InstanceMode>,
    slice_name: &str,
    area_mode: Option<&str>,
) -> Option<&'a InstanceMode> {
    // The last path part, without an extension (`.dynamicslice`).
    let name = slice_name
        .rsplit(['/', '\\'])
        .next()?
        .split('.')
        .next()?
        .to_lowercase();
    let (prefix, rest) = entrance_parts(&name)?;
    let modes: Vec<&InstanceMode> = modes.collect();
    mode_of_name(&modes, prefix, rest).or_else(|| {
        let area_mode = area_mode?;
        modes.into_iter().find(|mode| mode.id == area_mode)
    })
}

/// The game mode that names the entrance whose name after the entrance word is `rest`, for an
/// entrance word of the game mode id prefix `prefix` ([`entrance_mode`]).
fn mode_of_name<'a>(
    modes: &[&'a InstanceMode],
    prefix: &str,
    rest: &str,
) -> Option<&'a InstanceMode> {
    let rest = rest.replace(LOCKED_INFIX, "").replace(VERSION_SUFFIX, "");
    // A trailing number of more digits than a map number is a hash (`_1970794647`).
    let rest = match rest.rsplit_once('_') {
        Some((head, tail))
            if tail.len() > MAP_NUMBER_DIGITS && tail.chars().all(|c| c.is_ascii_digit()) =>
        {
            head.to_owned()
        }
        _ => rest,
    };
    let names: Vec<ModeNames> = modes
        .iter()
        .map(|mode| ModeNames::of(mode, prefix))
        .collect();
    // The name's words, longest first, with and without their number-only words: a word after
    // the mode's name is a variant (`Entrance_QuestIsabella_Simple`), a number-only word a stage
    // (`DungeonEntranceTrial_00_BrimstoneSandworm`). The first key that names any mode by its id,
    // else by its map, decides: two modes for it give none.
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
            for named in [ModeNames::by_id, ModeNames::by_map] {
                if let Some(found) = only_one(&names, |names| named(names, &key)) {
                    return found;
                }
            }
        }
    }
    // A name with no map number names the mode of its id and a map number.
    let key = letters_and_digits(&words.concat());
    if key.ends_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    only_one(&names, |names| names.by_id_less_number(&key)).flatten()
}

/// `Some` when any mode of `names` is `named`: that mode when it is the only one, `None` for two.
fn only_one<'a>(
    names: &[ModeNames<'a>],
    named: impl Fn(&ModeNames<'a>) -> bool,
) -> Option<Option<&'a InstanceMode>> {
    let mut found = names
        .iter()
        .filter(|names| named(names))
        .map(|names| names.mode);
    let first = found.next()?;
    Some(found.next().is_none().then_some(first))
}

/// The names an entrance may call a game mode by, in letters and digits.
struct ModeNames<'a> {
    mode: &'a InstanceMode,
    /// The id and the id less the prefix of the entrance word.
    ids: [String; 2],
    /// The map name less its level folder part.
    map: String,
}

impl<'a> ModeNames<'a> {
    fn of(mode: &'a InstanceMode, prefix: &str) -> Self {
        let id = mode.id.to_lowercase();
        let map = mode.map.map_name.to_lowercase();
        let map = MAP_PREFIXES
            .iter()
            .find_map(|folder| map.strip_prefix(folder))
            .unwrap_or(&map);
        Self {
            mode,
            ids: [
                letters_and_digits(&id),
                letters_and_digits(id.strip_prefix(prefix).unwrap_or(&id)),
            ],
            map: letters_and_digits(map),
        }
    }

    fn by_id(&self, key: &str) -> bool {
        self.ids.iter().any(|id| id == key)
    }

    fn by_map(&self, key: &str) -> bool {
        self.map == key
    }

    fn by_id_less_number(&self, key: &str) -> bool {
        self.ids
            .iter()
            .any(|id| id.trim_end_matches(|c: char| c.is_ascii_digit()) == key)
    }
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
            mode(
                "RaidMonarchsBluffs00",
                "NW_Raid_MonarchBluff_00",
                InstanceKind::RaidTrial,
            ),
            mode(
                "DungeonCutlassKeys00",
                "NW_Dungeon_CutlassKeys_00",
                InstanceKind::Expedition,
            ),
            mode(
                "RaidCutlassKeys00",
                "NW_Raid_CutlassKeys_00",
                InstanceKind::RaidTrial,
            ),
            mode(
                "DungeonBrimstoneSands00",
                "NW_Dungeon_BrimstoneSands_00",
                InstanceKind::Expedition,
            ),
            mode(
                "DungeonFirstLight01",
                "NW_Dungeon_FirstLight_01",
                InstanceKind::Expedition,
            ),
        ]
    }

    fn found_in(slice: &str, area_mode: Option<&str>) -> Option<String> {
        let modes = modes();
        entrance_mode(modes.iter(), slice, area_mode).map(|mode| mode.id.clone())
    }

    fn found(slice: &str) -> Option<String> {
        found_in(slice, None)
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
        assert_eq!(
            found("Dungeon/Entrance/DungeonEntranceTrial_Season_04").as_deref(),
            Some("TrialSeason04"),
            "the id wins over the map NW_Trial_Season_04 that two modes share"
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

    #[test]
    fn a_raid_door_names_its_mode_less_the_raid_prefix() {
        assert_eq!(
            found("Dungeon/Raids/Entrances/RaidEntrance_MonarchsBluffs00").as_deref(),
            Some("RaidMonarchsBluffs00")
        );
    }

    #[test]
    fn a_door_with_no_map_number_names_the_mode_of_its_kind() {
        assert_eq!(
            found("Dungeon/Entrance/DungeonEntrance_CutlassKeys").as_deref(),
            Some("DungeonCutlassKeys00"),
            "the expedition, not the raid of the same map name"
        );
        assert_eq!(
            found("Dungeon/Entrance/DungeonEntrance_FirstLight02"),
            None,
            "a map number that no mode has names no other number's mode"
        );
    }

    #[test]
    fn a_door_that_names_no_mode_opens_the_mode_of_its_area() {
        assert_eq!(
            found_in(
                "Dungeon/Entrance/DungeonEntrance_FirstLight02",
                Some("RaidCutlassKeys00")
            )
            .as_deref(),
            Some("RaidCutlassKeys00")
        );
        assert_eq!(
            found_in(
                "Dungeon/Entrance/DungeonEntrance_Brimstone00",
                Some("DungeonBrimstoneSands00")
            )
            .as_deref(),
            Some("DungeonBrimstoneSands00")
        );
        assert_eq!(
            found_in(
                "Dungeon/Entrance/DungeonEntrance_Amrine",
                Some("DungeonBrimstoneSands00")
            )
            .as_deref(),
            Some("DungeonAmrine"),
            "the name wins over the area"
        );
        assert_eq!(
            found_in(
                "Dungeon/Entrance/DungeonEntrance_Brimstone00",
                Some("NoSuchMode")
            ),
            None
        );
        assert_eq!(
            found_in("POIs/Settlement/Townboard", Some("DungeonAmrine")),
            None,
            "a slice that is no entrance has no mode"
        );
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
