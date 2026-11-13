// SPDX-License-Identifier: AGPL-3.0-or-later
//! The forts of the territory invasions (invasion study 2026-11-12, plan
//! `local-data/notes/2026-11-12-big-systems-plan.md` item 6): for each fort a retail invasion
//! capture shows, the invasion encounter entity, its waves with their mobs, the mob level, the
//! fort's structures and the reward event.
//!
//! Observed (the `invasionforts` bin of the capture tools over `invasion` (territory 16, won),
//! `Reekwater_Invasion` (territory 5, lost) and `Playthrough_Part_21` (territory 2, lost)):
//!
//! - The invasion runs one level entity of the fort (a card with a `GdeRef` and no asset, the
//!   defender company's ownership) that holds the encounter manager (6786) and nine encounter
//!   stages (2133): one defense stage that runs from the start to the end, and eight waves
//!   ([`InvasionFort::waves`]); each wave runs from its start to the next wave's start.
//! - The mobs of a wave are the corrupted the player saw, each with the time of its first sight
//!   after its wave's start, its vitals id and its first place ([`WaveSpawn`]); the siege mobs
//!   are the grunts and bombers with the cow launcher slice. The first sight is when the player
//!   saw the mob, not when it spawned (a mob the player did not see is not in the table).
//! - The fort's structures (4103 child 2, six entries of kinds 4 to 9) and the reward event the
//!   player got at the end (`Invasion_T4` at territory 16, `Invasion_T3` at 5 and 2).
//! - The lanes (the `invasionlanes` bin over the same ledgers): every invasion mob's positions
//!   over time, the mobs put together by their first place (within 8 m: 69 places at territory
//!   16, 81 at 5, 55 at 2); a place's lane is the track of its mob that walked the longest way
//!   (up to 415 m), cut to points 4 m apart at least ([`WaveSpawn::path`]; 975 of 978 spawns of
//!   territory 5 and 1,316 of 1,320 of territory 2 have one). Inferred: the retail lanes are
//!   server data no capture holds, and the longest walk of a place is the most of its lane the
//!   captures show (a mob that met a player fought it on the way).
//!
//! The table file is mined from the captures (no tracked file holds capture data): a fort that
//! no invasion capture shows has no entry, and gets no invasion.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The start of the table file name.
const FILE_PREFIX: &str = "invasion-forts-";
/// The end of the table file name.
const FILE_SUFFIX: &str = ".json";
/// The length of a `GdeRef`.
const ID_LEN: usize = 16;

/// The table file of client build `client_build` in content directory `dir`.
pub fn invasion_forts_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum InvasionFortsError {
    /// The file cannot be read.
    #[error("invasion forts table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("invasion forts table: {0}")]
    Json(#[from] serde_json::Error),
    /// A hex field does not read.
    #[error("invasion forts table: territory {territory}: {source}")]
    Hex {
        /// The fort's territory.
        territory: u16,
        /// Why.
        source: hex::FromHexError,
    },
    /// A fort lacks a part every invasion needs.
    #[error("invasion forts table: territory {territory}: {what}")]
    Missing {
        /// The fort's territory.
        territory: u16,
        /// What it lacks.
        what: &'static str,
    },
}

#[derive(Deserialize)]
struct TableFile {
    forts: Vec<FortRow>,
}

#[derive(Deserialize)]
struct FortRow {
    territory: u16,
    encounter: EncounterRow,
    #[serde(default)]
    structures: Option<Vec<StructureRow>>,
    mob_level: Option<u32>,
    reward_event: Option<String>,
    waves: Vec<WaveRow>,
}

#[derive(Deserialize)]
struct EncounterRow {
    gde_ref: String,
    card_component: u64,
    defense_component: Option<u64>,
    members: Vec<String>,
}

#[derive(Deserialize)]
struct StructureRow {
    key: u64,
    place: [f32; 3],
    kind: u32,
    health: u8,
    flag: u8,
}

#[derive(Deserialize)]
struct WaveRow {
    component: u64,
    start_after_ms: u64,
    spawns: Vec<SpawnRow>,
}

#[derive(Deserialize)]
struct SpawnRow {
    after_ms: u64,
    vitals: u32,
    place: [f32; 3],
    siege: bool,
    #[serde(default)]
    path: Vec<[f32; 3]>,
}

/// The invasion encounter entity of a fort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvasionEncounter {
    /// Its card's `GdeRef` (a level entity: the card has no asset).
    pub gde_ref: [u8; ID_LEN],
    /// The component of its card.
    pub card_component: u64,
    /// The component of its defense stage.
    pub defense_component: u64,
    /// Every other member of its first state (ownership, manager, stages, delayed events), each
    /// as one bundle member's bytes, as retail sent it.
    pub members: Vec<Vec<u8>>,
}

/// A major structure of a fort (the 4103 child 2 entry).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FortWall {
    /// Its key in the siege data.
    pub key: u64,
    /// Its place.
    pub place: [f32; 3],
    /// Its kind (Observed: 4 to 9).
    pub kind: u32,
    /// Its health in percent at the start (Observed: 100).
    pub health: u8,
    /// Its byte (Observed: 5).
    pub flag: u8,
}

/// One mob of a wave.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveSpawn {
    /// Milliseconds from its wave's start.
    pub after_ms: u64,
    /// Its vitals id (the CRC-32 of the lowercase `VitalsData` row id).
    pub vitals: u32,
    /// Where it was first seen.
    pub place: [f32; 3],
    /// It carries a cow launcher: a siege mob.
    pub siege: bool,
    /// Its lane after [`Self::place`]: the world positions it walks through, in order (module
    /// doc); empty when no retail mob of its place walked.
    pub path: Vec<[f32; 3]>,
}

/// The vitals id of `Invasion_Grunt` (the CRC-32 of the lowercase row id).
const INVASION_GRUNT_VITALS: u32 = 0x8b24_1eb8;
/// The vitals id of `Invasion_Bomber`.
const INVASION_BOMBER_VITALS: u32 = 0xaeeb_87b9;
/// `slices/characters/invasion/invasion_grunt_cowlauncher.dynamicslice`.
const GRUNT_COW_LAUNCHER_SLICE: [u8; ID_LEN] = [
    0x31, 0x2c, 0xc0, 0x27, 0xe5, 0x7a, 0x5b, 0x7a, 0x94, 0x84, 0xd5, 0x4b, 0x0a, 0x1e, 0x28, 0xb4,
];
/// `slices/characters/invasion/invasion_bomber_cowlauncher.dynamicslice`.
const BOMBER_COW_LAUNCHER_SLICE: [u8; ID_LEN] = [
    0xb8, 0xad, 0x52, 0xae, 0xd8, 0xd5, 0x52, 0x12, 0x81, 0xa3, 0x75, 0x28, 0xdc, 0xdf, 0x71, 0xd1,
];

impl WaveSpawn {
    /// The slice asset it shows when that is not its vitals id's most seen look: the cow
    /// launcher slice of a siege grunt or bomber.
    ///
    /// Observed (mob prefab table, build 25810808, from the invasion ledgers): the siege mobs
    /// share the vitals of the plain grunt (`Invasion_Grunt`, 37 siege spawns in the fort
    /// table) and bomber (`Invasion_Bomber`, 37) and show their own slice; no other vitals id
    /// carries a cow launcher.
    pub const fn slice(&self) -> Option<[u8; ID_LEN]> {
        match (self.siege, self.vitals) {
            (true, INVASION_GRUNT_VITALS) => Some(GRUNT_COW_LAUNCHER_SLICE),
            (true, INVASION_BOMBER_VITALS) => Some(BOMBER_COW_LAUNCHER_SLICE),
            _ => None,
        }
    }
}

/// One wave of an invasion.
#[derive(Debug, Clone, PartialEq)]
pub struct InvasionWave {
    /// The component of its stage on the encounter entity.
    pub component: u64,
    /// When it started in the capture, milliseconds from the invasion's start.
    pub start_after_ms: u64,
    /// Its mobs, by time.
    pub spawns: Vec<WaveSpawn>,
}

/// The invasion of one fort.
#[derive(Debug, Clone, PartialEq)]
pub struct InvasionFort {
    /// The territory id.
    pub territory: u16,
    /// Its encounter entity.
    pub encounter: InvasionEncounter,
    /// Its structures.
    pub walls: Vec<FortWall>,
    /// The level of its mobs.
    pub mob_level: u32,
    /// The game event every player of the instance gets at the end.
    pub reward_event: String,
    /// Its waves, in order.
    pub waves: Vec<InvasionWave>,
}

/// The invasion forts, by territory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InvasionForts {
    forts: Vec<InvasionFort>,
}

impl InvasionForts {
    /// The table of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`InvasionFortsError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, InvasionFortsError> {
        let path = invasion_forts_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| InvasionFortsError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`InvasionFortsError`] when the text is not the table, or a fort lacks its
    /// defense stage, its mob level, its reward event or its waves.
    pub fn parse(text: &str) -> Result<Self, InvasionFortsError> {
        let file: TableFile = serde_json::from_str(text)?;
        let forts = file
            .forts
            .into_iter()
            .map(fort_of)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { forts })
    }

    /// The invasion of the fort of `territory`.
    pub fn fort(&self, territory: u16) -> Option<&InvasionFort> {
        self.forts.iter().find(|fort| fort.territory == territory)
    }

    /// The territories that have an invasion.
    pub fn territories(&self) -> impl Iterator<Item = u16> + '_ {
        self.forts.iter().map(|fort| fort.territory)
    }

    /// How many forts the table holds.
    pub fn len(&self) -> usize {
        self.forts.len()
    }

    /// Whether the table has no fort.
    pub fn is_empty(&self) -> bool {
        self.forts.is_empty()
    }
}

fn fort_of(row: FortRow) -> Result<InvasionFort, InvasionFortsError> {
    let territory = row.territory;
    let missing = |what| InvasionFortsError::Missing { territory, what };
    let hex = |text: &str| {
        hex::decode(text).map_err(|source| InvasionFortsError::Hex { territory, source })
    };
    let gde_ref: [u8; ID_LEN] = hex(&row.encounter.gde_ref)?
        .try_into()
        .map_err(|_| missing("a 16-byte encounter id"))?;
    if row.waves.is_empty() {
        return Err(missing("its waves"));
    }
    let mut waves: Vec<InvasionWave> = row
        .waves
        .into_iter()
        .map(|wave| {
            let mut spawns: Vec<WaveSpawn> = wave
                .spawns
                .into_iter()
                .map(|spawn| WaveSpawn {
                    after_ms: spawn.after_ms,
                    vitals: spawn.vitals,
                    place: spawn.place,
                    siege: spawn.siege,
                    path: spawn.path,
                })
                .collect();
            spawns.sort_by_key(|spawn| spawn.after_ms);
            InvasionWave {
                component: wave.component,
                start_after_ms: wave.start_after_ms,
                spawns,
            }
        })
        .collect();
    waves.sort_by_key(|wave| wave.start_after_ms);
    Ok(InvasionFort {
        territory,
        encounter: InvasionEncounter {
            gde_ref,
            card_component: row.encounter.card_component,
            defense_component: row
                .encounter
                .defense_component
                .ok_or_else(|| missing("its defense stage"))?,
            members: row
                .encounter
                .members
                .iter()
                .map(|member| hex(member))
                .collect::<Result<Vec<_>, _>>()?,
        },
        walls: row
            .structures
            .unwrap_or_default()
            .into_iter()
            .map(|wall| FortWall {
                key: wall.key,
                place: wall.place,
                kind: wall.kind,
                health: wall.health,
                flag: wall.flag,
            })
            .collect(),
        mob_level: row.mob_level.ok_or_else(|| missing("its mob level"))?,
        reward_event: row
            .reward_event
            .ok_or_else(|| missing("its reward event"))?,
        waves,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = r#"{"forts": [{
        "territory": 16,
        "encounter": {"gde_ref": "1b36e3993bd273d89a2e1f6aa2b56ebd", "card_component": 0,
                      "defense_component": 7, "members": ["0203"]},
        "structures": [{"key": 7, "place": [5805.5, 7387.5, 71.1], "kind": 4, "health": 100, "flag": 5}],
        "mob_level": 65, "reward_event": "Invasion_T4",
        "waves": [
          {"component": 8, "start_after_ms": 170027, "spawns": [
            {"after_ms": 9000, "vitals": 2334400185, "place": [5805.0, 7283.0, 71.0], "siege": false}]},
          {"component": 6, "start_after_ms": 0, "spawns": [
            {"after_ms": 8000, "vitals": 2334400185, "place": [5805.0, 7283.0, 71.0], "siege": false},
            {"after_ms": 4100, "vitals": 2934671289, "place": [5801.0, 7281.0, 71.0], "siege": true,
             "path": [[5806.0, 7290.0, 71.0], [5807.0, 7297.0, 71.0]]}]}]}]}"#;

    #[test]
    fn the_table_reads_with_its_waves_and_spawns_in_time_order() {
        let forts = InvasionForts::parse(TABLE).expect("table");
        let fort = forts.fort(16).expect("fort");
        assert_eq!(fort.encounter.defense_component, 7);
        assert_eq!(fort.encounter.members, vec![vec![2, 3]]);
        assert_eq!(fort.walls.len(), 1);
        assert_eq!(
            (fort.mob_level, fort.reward_event.as_str()),
            (65, "Invasion_T4")
        );
        let starts: Vec<u64> = fort.waves.iter().map(|wave| wave.start_after_ms).collect();
        assert_eq!(starts, vec![0, 170_027]);
        let firsts: Vec<u64> = fort.waves[0]
            .spawns
            .iter()
            .map(|spawn| spawn.after_ms)
            .collect();
        assert_eq!(firsts, vec![4100, 8000]);
        assert!(fort.waves[0].spawns[0].siege);
        assert_eq!(
            fort.waves[0].spawns[0].path,
            vec![[5806.0, 7290.0, 71.0], [5807.0, 7297.0, 71.0]],
            "its lane in order"
        );
        assert!(
            fort.waves[0].spawns[1].path.is_empty(),
            "a spawn without a lane"
        );
        assert_eq!(forts.territories().collect::<Vec<_>>(), vec![16]);
        assert!(forts.fort(5).is_none());
    }

    #[test]
    fn a_fort_without_its_defense_stage_or_waves_is_refused() {
        let no_defense = TABLE.replace(r#""defense_component": 7"#, r#""defense_component": null"#);
        assert!(matches!(
            InvasionForts::parse(&no_defense),
            Err(InvasionFortsError::Missing { territory: 16, .. })
        ));
        let no_reward = TABLE.replace(
            r#""reward_event": "Invasion_T4""#,
            r#""reward_event": null"#,
        );
        assert!(InvasionForts::parse(&no_reward).is_err());
    }

    #[test]
    fn a_siege_grunt_or_bomber_shows_its_cow_launcher_slice() {
        let spawn = |vitals, siege| WaveSpawn {
            after_ms: 0,
            vitals,
            place: [0.0; 3],
            siege,
            path: Vec::new(),
        };
        assert_eq!(
            spawn(INVASION_GRUNT_VITALS, true).slice(),
            Some(GRUNT_COW_LAUNCHER_SLICE)
        );
        assert_eq!(
            spawn(INVASION_BOMBER_VITALS, true).slice(),
            Some(BOMBER_COW_LAUNCHER_SLICE)
        );
        assert_eq!(
            spawn(INVASION_GRUNT_VITALS, false).slice(),
            None,
            "plain look"
        );
        assert_eq!(spawn(INVASION_BOMBER_VITALS, false).slice(), None);
        assert_eq!(spawn(1, true).slice(), None, "no other cow launcher slice");
    }
}
