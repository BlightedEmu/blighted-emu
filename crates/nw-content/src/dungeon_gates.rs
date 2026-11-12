// SPDX-License-Identifier: AGPL-3.0-or-later
//! The gates, doors, light bridges, portal stones, switches and arena blockers of the instance
//! levels, and the rules that open and close them (dungeon gates note 2026-11-11).
//!
//! Observed (retail captures): each of these is a slice that a spawner entity of the level
//! script (`slices/dungeon/levelscripts/<x>.dynamicslice`) places, and a network entity of its
//! own: a card of the slice asset, its position, its `GatherableControllerReplicatedState`
//! (4 closed, 8 open) and one `TimelineComponentReplicatedState` per timeline entity of the
//! slice, each on the component number its slice gives it (the replica index). An arena blocker
//! (Starstone `BossArenaBlockerInivs`) has a card and a position only: it is spawned when the
//! boss fight starts and its slot stops at the boss encounter's success.
//!
//! The gates file ([`DungeonGates`], `content-tool dungeon-gates`, from the game paks) holds,
//! per level script, every such spawner: its entity name, place, slice asset and component
//! numbers, and each timeline's name and length; and, per encounter slice, the component numbers
//! of its own timelines, delayed events and placement obstructions (the targets of its stages'
//! entity events).
//!
//! The rules file ([`GateRules`]) is mined from the retail captures (`gaterules.py` of the
//! capture tools): per level, what opens and what closes each gate. The retail rule source is
//! Unknown: no client data names the doors (the note, part 1), so a level with no capture has no
//! rule here.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::town_props::ASSET_ID_LEN;

const GATES_PREFIX: &str = "dungeon-gates-";
const RULES_PREFIX: &str = "dungeon-gate-rules-";
const FILE_SUFFIX: &str = ".json";
/// The name parts of a gate's open and close timelines (Observed: `Timeline_Open` and
/// `Timeline_Close` of the ancient gates, `TimelineToDepleted` and `TimelineToHealthy` of the
/// Dynasty boss door; lower case).
const OPEN_TIMELINE_PARTS: [&str; 2] = ["open", "todepleted"];
const CLOSE_TIMELINE_PARTS: [&str; 2] = ["close", "tohealthy"];

/// The gates file of client build `client_build` in content directory `dir`.
pub fn dungeon_gates_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{GATES_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// The gate rules file of client build `client_build` in content directory `dir`.
pub fn dungeon_gate_rules_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{RULES_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad gates or rules file.
#[derive(Debug, thiserror::Error)]
pub enum DungeonGatesError {
    /// The file cannot be read or written.
    #[error("dungeon gates file {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("dungeon gates file: {0}")]
    Json(#[from] serde_json::Error),
}

/// The hex form of an asset id in the files.
mod hex_asset {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use super::ASSET_ID_LEN;

    pub(super) fn serialize<S: Serializer>(
        asset: &[u8; ASSET_ID_LEN],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(asset))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u8; ASSET_ID_LEN], D::Error> {
        let text = String::deserialize(deserializer)?;
        let bytes = hex::decode(&text).map_err(D::Error::custom)?;
        <[u8; ASSET_ID_LEN]>::try_from(bytes.as_slice())
            .map_err(|_| D::Error::custom(format!("an asset id of {} bytes", bytes.len())))
    }
}

/// What a gate's timeline plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineRole {
    /// It plays when the gate opens (gatherable 4 to 8).
    Open,
    /// It plays when the gate closes (8 to 4).
    Close,
    /// Another timeline: it does not play on a state change.
    Other,
}

/// One timeline of a gate slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateTimeline {
    /// The name of its entity in the slice.
    pub name: String,
    /// The component of its 1647 state.
    pub component: u64,
    /// Its length, seconds: the latest clip end of its `.timeline` asset (Inferred; Observed
    /// equal to the retail end bodies: Amrine 8.0, Starstone 7.0, Dynasty 2.125 and 1.5).
    pub length_s: f32,
}

impl GateTimeline {
    /// Whether it opens or closes its gate, by its name.
    pub fn role(&self) -> TimelineRole {
        let name = self.name.to_lowercase();
        if OPEN_TIMELINE_PARTS.iter().any(|part| name.contains(part)) {
            TimelineRole::Open
        } else if CLOSE_TIMELINE_PARTS.iter().any(|part| name.contains(part)) {
            TimelineRole::Close
        } else {
            TimelineRole::Other
        }
    }
}

/// One gate-like object of a level script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    /// The level-script entity that spawns it (`ObeliskEntryDoor`).
    pub name: String,
    /// That entity's id.
    pub entity: u64,
    /// The pak path of the slice it spawns.
    pub slice: String,
    /// The slice asset's id (the card's asset).
    #[serde(with = "hex_asset")]
    pub asset: [u8; ASSET_ID_LEN],
    /// Its world place, metres.
    pub position: [f32; 3],
    /// Its yaw, degrees counter-clockwise from the x axis.
    pub yaw_deg: f32,
    /// The component of its position state.
    pub position_component: Option<u64>,
    /// The component of its gatherable state; `None` for a blocker without one.
    pub gatherable_component: Option<u64>,
    /// Its timelines.
    pub timelines: Vec<GateTimeline>,
}

impl Gate {
    /// Its timeline that plays at `role`.
    pub fn timeline(&self, role: TimelineRole) -> Option<&GateTimeline> {
        self.timelines
            .iter()
            .find(|timeline| timeline.role() == role)
    }
}

/// The gates of one level script.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GateLevel {
    /// The level script's pak path.
    pub script_slice: String,
    /// The level folder.
    pub level: String,
    /// The game modes that run it.
    pub game_modes: Vec<String>,
    /// Its gate-like spawners, in level-script order.
    pub gates: Vec<Gate>,
}

impl GateLevel {
    /// The gate spawned by level-script entity `name`.
    pub fn gate(&self, name: &str) -> Option<(usize, &Gate)> {
        self.gates
            .iter()
            .enumerate()
            .find(|(_, gate)| gate.name == name)
    }
}

/// A replicated component of an encounter slice's own entity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityComponent {
    /// A timeline (1647) of `length_s` seconds.
    Timeline {
        /// Its component.
        component: u64,
        /// Its length, seconds (as [`GateTimeline::length_s`]).
        length_s: f32,
    },
    /// A delayed event (3139) of `delay_s` seconds.
    DelayedEvent {
        /// Its component.
        component: u64,
        /// Its delay, seconds.
        delay_s: f32,
    },
    /// A placement obstruction (2187).
    PlacementObstruction {
        /// Its component.
        component: u64,
    },
}

/// An entity of an encounter slice with replicated components.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SliceEntity {
    /// Its id (the id a stage's entity event names).
    pub entity: u64,
    /// Its name.
    pub name: String,
    /// Its replicated components.
    pub components: Vec<EntityComponent>,
}

#[derive(Serialize, Deserialize)]
struct GatesFile {
    source: String,
    levels: Vec<GateLevel>,
    encounter_entities: BTreeMap<String, Vec<SliceEntity>>,
}

/// The gates file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DungeonGates {
    /// Where it was built from.
    pub source: String,
    /// Each level script's gates.
    pub levels: Vec<GateLevel>,
    /// The replicated entities of each encounter slice, by pak path.
    pub encounter_entities: BTreeMap<String, Vec<SliceEntity>>,
    by_mode: HashMap<String, usize>,
}

impl DungeonGates {
    /// The file of `levels` and `encounter_entities`, built from `source`.
    pub fn new(
        source: String,
        levels: Vec<GateLevel>,
        encounter_entities: BTreeMap<String, Vec<SliceEntity>>,
    ) -> Self {
        let by_mode = levels
            .iter()
            .enumerate()
            .flat_map(|(index, level)| {
                level
                    .game_modes
                    .iter()
                    .map(move |mode| (mode.clone(), index))
            })
            .collect();
        Self {
            source,
            levels,
            encounter_entities,
            by_mode,
        }
    }

    /// The file of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonGatesError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, DungeonGatesError> {
        Self::parse(&read(&dungeon_gates_path(dir, client_build))?)
    }

    /// The file in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonGatesError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, DungeonGatesError> {
        let file: GatesFile = serde_json::from_str(text)?;
        Ok(Self::new(file.source, file.levels, file.encounter_entities))
    }

    /// Writes it to `path`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonGatesError`] when the file cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), DungeonGatesError> {
        let file = GatesFile {
            source: self.source.clone(),
            levels: self.levels.clone(),
            encounter_entities: self.encounter_entities.clone(),
        };
        write(path, &serde_json::to_string_pretty(&file)?)
    }

    /// The gates of game mode `game_mode`'s level script.
    pub fn of_mode(&self, game_mode: &str) -> Option<&GateLevel> {
        self.by_mode
            .get(game_mode)
            .and_then(|index| self.levels.get(*index))
    }

    /// The replicated entities of encounter slice `slice`.
    pub fn slice_entities(&self, slice: &str) -> &[SliceEntity] {
        self.encounter_entities
            .get(slice)
            .map_or(&[], Vec::as_slice)
    }

    /// How many gates all levels hold.
    pub fn gate_count(&self) -> usize {
        self.levels.iter().map(|level| level.gates.len()).sum()
    }
}

/// A gate's state when the instance starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// Not spawned: a rule spawns it (an arena blocker).
    Absent,
    /// Spawned with the level.
    Present {
        /// Its gatherable value (4 closed, 8 open; Observed also 128 on the Amrine pressure
        /// plates, meaning Unknown); `None` for a blocker without one.
        state: Option<u8>,
    },
}

/// What makes a rule fire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// The encounter of that level-script name succeeded (6786 = 3).
    EncounterSucceeded {
        /// Its level-script name.
        encounter: String,
    },
    /// The encounter started (6786 = 2).
    EncounterStarted {
        /// Its level-script name.
        encounter: String,
    },
    /// A living player came into the encounter's volume while it ran (the stand-in for the
    /// level script's boss area: Inferred, the retail close had no event in the stream).
    EncounterEngaged {
        /// Its level-script name.
        encounter: String,
    },
    /// A stage of the encounter started (2133 in progress).
    StageStarted {
        /// Its level-script name.
        encounter: String,
        /// The stage's entity name.
        stage: String,
    },
    /// A player used the switch, the gate of that level-script name (its gather ended).
    SwitchUsed {
        /// The switch's level-script name.
        switch: String,
    },
}

/// What a rule does to its gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateAction {
    /// Its gatherable takes `state` (8 opens, 4 closes) and, in the same tick, `timeline` starts
    /// (the timeline retail played with the change; `None` when it played none).
    Set {
        /// The gatherable value.
        state: u8,
        /// The name of the timeline entity that plays.
        timeline: Option<String>,
    },
    /// The entity comes (card and states).
    Spawn,
    /// The entity's slot stops.
    Remove,
}

/// One rule of a gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateRule {
    /// When it fires.
    pub when: Trigger,
    /// What it does.
    pub then: GateAction,
}

/// The rules of one gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateRuleSet {
    /// The gate's level-script name.
    pub gate: String,
    /// Its state at the instance's start.
    pub entry: Entry,
    /// Its rules.
    pub rules: Vec<GateRule>,
}

/// The mined rules of one level script.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelRules {
    /// The level script's pak path.
    pub script_slice: String,
    /// The retail ledgers the rules come from.
    pub ledgers: Vec<String>,
    /// The rules of each gate the captures show.
    pub gates: Vec<GateRuleSet>,
}

impl LevelRules {
    /// The rules of gate `gate`.
    pub fn of_gate(&self, gate: &str) -> Option<&GateRuleSet> {
        self.gates.iter().find(|rules| rules.gate == gate)
    }
}

#[derive(Serialize, Deserialize)]
struct RulesFile {
    source: String,
    levels: Vec<LevelRules>,
}

/// The rules file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateRules {
    /// Where it was mined from.
    pub source: String,
    /// The rules of each level script the captures show.
    pub levels: Vec<LevelRules>,
}

impl GateRules {
    /// The file of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonGatesError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, DungeonGatesError> {
        Self::parse(&read(&dungeon_gate_rules_path(dir, client_build))?)
    }

    /// The file in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonGatesError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, DungeonGatesError> {
        let file: RulesFile = serde_json::from_str(text)?;
        Ok(Self {
            source: file.source,
            levels: file.levels,
        })
    }

    /// The rules of level script `script_slice`.
    pub fn of_level(&self, script_slice: &str) -> Option<&LevelRules> {
        self.levels
            .iter()
            .find(|level| level.script_slice == script_slice)
    }
}

fn read(path: &Path) -> Result<String, DungeonGatesError> {
    std::fs::read_to_string(path).map_err(|source| DungeonGatesError::Io {
        path: path.to_owned(),
        source,
    })
}

fn write(path: &Path, text: &str) -> Result<(), DungeonGatesError> {
    std::fs::write(path, text).map_err(|source| DungeonGatesError::Io {
        path: path.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up level shaped as Amrine: one tall gate and one encounter slice entity.
    const GATES: &str = r#"{
        "source": "test",
        "levels": [{
            "script_slice": "slices/dungeon/levelscripts/testdungeon.dynamicslice",
            "level": "nw_dungeon_test_00",
            "game_modes": ["DungeonTest", "DungeonTestMutated"],
            "gates": [{
                "name": "ObeliskEntryDoor", "entity": 1324, "slice": "slices/test/gate.dynamicslice",
                "asset": "00112233445566778899aabbccddeeff", "position": [899.7, 861.6, 141.8],
                "yaw_deg": 90.0, "position_component": 1,
                "gatherable_component": 2,
                "timelines": [
                    {"name": "Timeline_Close", "component": 3, "length_s": 8.0},
                    {"name": "Timeline_Open", "component": 4, "length_s": 8.0}
                ]
            }]
        }],
        "encounter_entities": {
            "slices/test/encounter.dynamicslice": [{
                "entity": 3951, "name": "DeathEvent",
                "components": [{"delayed_event": {"component": 5, "delay_s": 3.5}}]
            }]
        }
    }"#;

    const RULES: &str = r#"{
        "source": "test",
        "levels": [{
            "script_slice": "slices/dungeon/levelscripts/testdungeon.dynamicslice",
            "ledgers": ["test ledger"],
            "gates": [{
                "gate": "ObeliskEntryDoor", "entry": {"present": {"state": 4}},
                "rules": [
                    {"when": {"encounter_succeeded": {"encounter": "EncounterChamber"}},
                     "then": {"set": {"state": 8, "timeline": "Timeline_Open"}}},
                    {"when": {"encounter_engaged": {"encounter": "EncounterBoss"}},
                     "then": {"set": {"state": 4, "timeline": "Timeline_Close"}}}
                ]
            }]
        }]
    }"#;

    #[test]
    fn a_game_mode_finds_its_level_s_gates_and_their_timelines() {
        let gates = DungeonGates::parse(GATES).expect("gates");
        assert_eq!(gates.gate_count(), 1);
        let level = gates.of_mode("DungeonTestMutated").expect("level");
        let (index, door) = level.gate("ObeliskEntryDoor").expect("door");
        assert_eq!(index, 0);
        assert_eq!(door.asset[..2], [0x00, 0x11]);
        assert_eq!(
            door.timeline(TimelineRole::Open).map(|t| t.component),
            Some(4)
        );
        assert_eq!(
            door.timeline(TimelineRole::Close).map(|t| t.component),
            Some(3)
        );
        assert!(gates.of_mode("DungeonOther").is_none());
        assert_eq!(
            gates.slice_entities("slices/test/encounter.dynamicslice")[0].components,
            [EntityComponent::DelayedEvent {
                component: 5,
                delay_s: 3.5
            }]
        );
        assert!(gates.slice_entities("slices/other.dynamicslice").is_empty());
    }

    #[test]
    fn the_gates_file_writes_and_reads_back() {
        let gates = DungeonGates::parse(GATES).expect("gates");
        let dir = tempfile::tempdir().expect("dir");
        let path = dungeon_gates_path(dir.path(), "1");
        gates.write(&path).expect("write");
        assert_eq!(DungeonGates::load(dir.path(), "1").expect("load"), gates);
    }

    #[test]
    fn timeline_roles_follow_the_retail_names() {
        let timeline = |name: &str| GateTimeline {
            name: name.to_owned(),
            component: 3,
            length_s: 1.5,
        };
        assert_eq!(timeline("TimelineToDepleted").role(), TimelineRole::Open);
        assert_eq!(timeline("TimelineToHealthy").role(), TimelineRole::Close);
        assert_eq!(timeline("Timeline_Death").role(), TimelineRole::Other);
    }

    #[test]
    fn a_level_s_rules_name_their_triggers() {
        let rules = GateRules::parse(RULES).expect("rules");
        let level = rules
            .of_level("slices/dungeon/levelscripts/testdungeon.dynamicslice")
            .expect("level");
        let door = level.of_gate("ObeliskEntryDoor").expect("door");
        assert_eq!(door.entry, Entry::Present { state: Some(4) });
        assert_eq!(
            door.rules[1],
            GateRule {
                when: Trigger::EncounterEngaged {
                    encounter: "EncounterBoss".to_owned()
                },
                then: GateAction::Set {
                    state: 4,
                    timeline: Some("Timeline_Close".to_owned()),
                },
            }
        );
    }

    #[test]
    fn a_bad_asset_id_is_an_error() {
        let bad = GATES.replace("00112233445566778899aabbccddeeff", "0011");
        assert!(matches!(
            DungeonGates::parse(&bad),
            Err(DungeonGatesError::Json(_))
        ));
    }
}
