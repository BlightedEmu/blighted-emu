// SPDX-License-Identifier: AGPL-3.0-or-later
//! The game data of the influence towers (findings `2026-11-12-wars-and-sieges.md`, "Control
//! points and towers"): each tower's entry of the claims' faction control point list, its
//! territory, place and capture times.
//!
//! Observed: the level's influence towers are its landmarks of type [`TOWER_LANDMARK_TYPE`]
//! (39, three per territory), whose data is the tower's `TowerID` of
//! `javelindata_factioncontrol_towerdefinitions` (`CaptureTimeInSeconds` 120,
//! `CaptureTimeMinimum` 75, `CaptureMaxPlayerDelta` 5 on every row), and whose first territory
//! id is the territory.
//!
//! **Entries** ([`TOWER_ENTRIES`], Observed): the client's map gives a tower the key
//! `GetInfluenceTowerClaimKey(GetLandmarkIndex(actor))` (`factioncontrolpointhud.luac`).
//! `GetLandmarkIndex` (`LandClaimRequestBus` slot +0xc8, `FUN_142f83190`) parses the tower
//! actor's id and returns its place in the landmark manager's tower list (`FUN_1444d1be0`, a
//! list of 16-byte actor ids). The landmark loader (`FUN_1444d5030`) puts the actor id of each
//! type 39 landmark into that list once, at its place by `memcmp` of the 16 bytes, so the list
//! holds the tower actor ids in rising byte order. `GetInfluenceTowerClaimKey` (slot +0xb8,
//! `FUN_142f82eb0`) is that index plus the highest territory id plus one: the key that the
//! claims' list reader (`FUN_142f820d0`) gives entry `index + 18` (13 claims and 5 territories
//! come first). The actor ids are field `0xf84f8d10` of the tower landmark records in the open
//! world's `region.metadata` files. The world file keeps no actor id, so the table holds the
//! entries that this rule gives. The captures agree: the 39 entries fall into the 13 groups of
//! three that went inactive together at each territory's conflict start, and the recorder stood
//! in the capture radius of the tower whose entry changed (`raid_group_influence_race`: 8 m from
//! `Cutlass_Keys_Tower_01_CKRV` when entry 34 was taken, within 25 m of `Cutlass_Keys_Tower_03`
//! at three changes of entry 42; `Playthrough_Part_17`: 18 m from `Edengrove_Tower_03` when
//! entry 21 went contested at Unix ms 1793475342397).

use std::collections::BTreeMap;

use nw_content::Tables;
use nw_world::WorldFile;

/// The landmark type of an influence tower (Observed: 39 landmarks of the open world, each
/// naming a `TowerID`).
pub const TOWER_LANDMARK_TYPE: u8 = 39;

/// The claims' faction control point entry of each influence tower, by `TowerID` (Observed:
/// the client's rule of the module doc over the actor ids of build 25810808).
pub const TOWER_ENTRIES: [(&str, u8); 39] = [
    ("Cutlass_Keys_Tower_02", 18),
    ("Reekwater_Tower_02", 19),
    ("Brightwood_Tower_01", 20),
    ("Edengrove_Tower_03", 21),
    ("Reekwater_Tower_03", 22),
    ("Windsward_Tower_02", 23),
    ("Restless_Shore_Tower_01", 24),
    ("Edengrove_Tower_02", 25),
    ("Brightwood_Tower_03", 26),
    ("Dunwood_Tower_03", 27),
    ("Brightwood_Tower_02", 28),
    ("Restless_Shore_Tower_03", 29),
    ("Reekwater_Tower_01", 30),
    ("Ebonscale_Reach_Tower_03", 31),
    ("Mourningdale_Tower_02", 32),
    ("Mourningdale_Tower_03", 33),
    ("Cutlass_Keys_Tower_01_CKRV", 34),
    ("Ebonscale_Reach_Tower_02", 35),
    ("Dunwood_Tower_02", 36),
    ("Monarchs_Bluff_Tower_02", 37),
    ("Windsward_Tower_03", 38),
    ("Brimstone_Sands_Tower_02", 39),
    ("Monarchs_Bluff_Tower_03", 40),
    ("Weavers_Fen_Tower_01", 41),
    ("Cutlass_Keys_Tower_03", 42),
    ("Brimstone_Sands_Tower_03", 43),
    ("Weavers_Fen_Tower_02", 44),
    ("Ebonscale_Reach_Tower_01", 45),
    ("Dunwood_Tower_01", 46),
    ("Everfall_Tower_01", 47),
    ("Mourningdale_Tower_01", 48),
    ("Edengrove_Tower_01", 49),
    ("Windsward_Tower_01", 50),
    ("Everfall_Tower_02", 51),
    ("Restless_Shore_Tower_02", 52),
    ("Everfall_Tower_03", 53),
    ("Monarchs_Bluff_Tower_01", 54),
    ("Weavers_Fen_Tower_03", 55),
    ("Brimstone_Sands_Tower_01", 56),
];

/// The entry of tower `tower_id`; `None` for a `TowerID` the table does not name.
fn tower_entry(tower_id: &str) -> Option<u8> {
    TOWER_ENTRIES
        .iter()
        .find(|(known, _)| *known == tower_id)
        .map(|(_, entry)| *entry)
}

/// One influence tower.
#[derive(Debug, Clone, PartialEq)]
pub struct TowerPoint {
    /// Its entry of the claims' faction control point list.
    pub entry: u8,
    /// Its territory id.
    pub territory: u16,
    /// Its `TowerID`.
    pub tower_id: String,
    /// Its place, metres.
    pub position: [f32; 3],
    /// The capture time of one capturer more than the defenders, seconds.
    pub capture_s: u32,
    /// The capture time of the most capturers more, seconds.
    pub capture_min_s: u32,
    /// From how many capturers more the capture time is the shortest.
    pub max_player_delta: u32,
}

/// The influence towers of the level.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ControlPointRules {
    /// The towers, by entry.
    pub towers: Vec<TowerPoint>,
}

impl ControlPointRules {
    /// The towers of `level` with their capture times from `tables`; a tower landmark without
    /// a territory, whose `TowerID` has no row or no entry in [`TOWER_ENTRIES`], has no tower;
    /// a tower that comes twice (a region file and its capital file) keeps its first landmark,
    /// as the client keeps each actor id once.
    pub fn of(tables: &Tables, level: &WorldFile) -> Self {
        let rows: BTreeMap<&str, (u32, u32, u32)> = tables
            .influence_tower_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    row.tower_id.as_str(),
                    (
                        u32::try_from(row.capture_time_in_seconds).ok()?,
                        u32::try_from(row.capture_time_minimum).ok()?,
                        u32::try_from(row.capture_max_player_delta).ok()?,
                    ),
                ))
            })
            .collect();
        let mut towers = Vec::new();
        for landmark in &level.landmarks {
            if landmark.landmark_type != TOWER_LANDMARK_TYPE {
                continue;
            }
            let tower_id = landmark.landmark_data.as_str();
            let Some(territory) = landmark.territory_ids.first() else {
                continue;
            };
            let Some(entry) = tower_entry(tower_id) else {
                tracing::warn!(tower_id, "influence tower without a list entry");
                continue;
            };
            let Some((capture_s, capture_min_s, max_player_delta)) = rows.get(tower_id) else {
                tracing::warn!(tower_id, "influence tower without a definition");
                continue;
            };
            if towers.iter().any(|tower: &TowerPoint| tower.entry == entry) {
                continue;
            }
            towers.push(TowerPoint {
                entry,
                territory: *territory,
                tower_id: tower_id.to_owned(),
                position: landmark.position,
                capture_s: *capture_s,
                capture_min_s: *capture_min_s,
                max_player_delta: *max_player_delta,
            });
        }
        towers.sort_by_key(|tower| tower.entry);
        Self { towers }
    }
}

#[cfg(test)]
mod tests {
    use nw_content::generated::InfluenceTowerDefinitions;
    use nw_content::table::Table;
    use nw_world::records::Landmark;

    use super::*;

    const CAPTURE_S: i64 = 120;
    const CAPTURE_MIN_S: i64 = 75;
    const MAX_DELTA: i64 = 5;

    fn landmark(tower_id: &str, territory: u16, x: f32) -> Landmark {
        Landmark {
            landmark_type: TOWER_LANDMARK_TYPE,
            landmark_data: tower_id.to_owned(),
            position: [x, 0.0, 0.0],
            radius: 0.0,
            territory_ids: vec![territory],
        }
    }

    fn row(tower_id: &str) -> InfluenceTowerDefinitions {
        InfluenceTowerDefinitions {
            capture_max_player_delta: MAX_DELTA,
            capture_time_in_seconds: CAPTURE_S,
            capture_time_minimum: CAPTURE_MIN_S,
            description: String::new(),
            name: String::new(),
            tower_id: tower_id.to_owned(),
        }
    }

    fn tables_of(names: &[&str]) -> Tables {
        Tables {
            influence_tower_definitions: vec![Table {
                source_path: String::new(),
                unique_id: String::new(),
                rows: names.iter().map(|name| row(name)).collect(),
            }],
            ..Tables::default()
        }
    }

    #[test]
    fn each_tower_takes_the_entry_of_its_actor_id_place() {
        const WINDSWARD: u16 = 6;
        let names = [
            "Windsward_Tower_03",
            "Windsward_Tower_01",
            "Windsward_Tower_02",
        ];
        let level = WorldFile {
            landmarks: names
                .iter()
                .zip([2.0, 0.0, 1.0])
                .map(|(name, x)| landmark(name, WINDSWARD, x))
                .collect(),
            ..WorldFile::default()
        };
        let rules = ControlPointRules::of(&tables_of(&names), &level);
        let entries: Vec<(u8, &str)> = rules
            .towers
            .iter()
            .map(|tower| (tower.entry, tower.tower_id.as_str()))
            .collect();
        assert_eq!(
            entries,
            [
                (23, "Windsward_Tower_02"),
                (38, "Windsward_Tower_03"),
                (50, "Windsward_Tower_01")
            ]
        );
        assert!(rules.towers.iter().all(|tower| tower.territory == WINDSWARD));
        assert_eq!(rules.towers[0].position, [1.0, 0.0, 0.0]);
        assert_eq!(i64::from(rules.towers[0].capture_s), CAPTURE_S);
        assert_eq!(i64::from(rules.towers[0].capture_min_s), CAPTURE_MIN_S);
        assert_eq!(i64::from(rules.towers[0].max_player_delta), MAX_DELTA);
    }

    #[test]
    fn a_tower_without_an_entry_a_row_or_a_territory_or_seen_twice_adds_none() {
        const CUTLASS_KEYS: u16 = 10;
        const FIRST_X: f32 = 1.0;
        const SECOND_X: f32 = 2.0;
        let mut unplaced = landmark("Cutlass_Keys_Tower_03", CUTLASS_KEYS, 0.0);
        unplaced.territory_ids.clear();
        let level = WorldFile {
            landmarks: vec![
                landmark("Made_Up_Tower_01", CUTLASS_KEYS, 0.0),
                landmark("Cutlass_Keys_Tower_01_CKRV", CUTLASS_KEYS, 0.0),
                unplaced,
                landmark("Cutlass_Keys_Tower_02", CUTLASS_KEYS, FIRST_X),
                landmark("Cutlass_Keys_Tower_02", CUTLASS_KEYS, SECOND_X),
            ],
            ..WorldFile::default()
        };
        let tables = tables_of(&[
            "Made_Up_Tower_01",
            "Cutlass_Keys_Tower_03",
            "Cutlass_Keys_Tower_02",
        ]);
        let rules = ControlPointRules::of(&tables, &level);
        let towers: Vec<(u8, &str, f32)> = rules
            .towers
            .iter()
            .map(|tower| (tower.entry, tower.tower_id.as_str(), tower.position[0]))
            .collect();
        assert_eq!(towers, [(18, "Cutlass_Keys_Tower_02", FIRST_X)]);
    }

    /// The entries of each territory's towers that went inactive together at its conflict
    /// start (Observed, findings "Control points and towers").
    const CONFLICT_GROUPS: [(&str, [u8; 3]); 13] = [
        ("Cutlass_Keys", [18, 34, 42]),
        ("Reekwater", [19, 22, 30]),
        ("Brightwood", [20, 26, 28]),
        ("Edengrove", [21, 25, 49]),
        ("Windsward", [23, 38, 50]),
        ("Restless_Shore", [24, 29, 52]),
        ("Dunwood", [27, 36, 46]),
        ("Ebonscale_Reach", [31, 35, 45]),
        ("Mourningdale", [32, 33, 48]),
        ("Monarchs_Bluff", [37, 40, 54]),
        ("Brimstone_Sands", [39, 43, 56]),
        ("Weavers_Fen", [41, 44, 55]),
        ("Everfall", [47, 51, 53]),
    ];

    #[test]
    fn the_table_holds_the_39_entries_once_in_the_conflict_groups() {
        let mut entries: Vec<u8> = TOWER_ENTRIES.iter().map(|(_, entry)| *entry).collect();
        entries.sort_unstable();
        assert_eq!(entries, (18..57).collect::<Vec<u8>>());
        for (territory, group) in CONFLICT_GROUPS {
            let mut found: Vec<u8> = TOWER_ENTRIES
                .iter()
                .filter(|(tower_id, _)| {
                    tower_id
                        .strip_prefix(territory)
                        .is_some_and(|rest| rest.starts_with("_Tower_"))
                })
                .map(|(_, entry)| *entry)
                .collect();
            found.sort_unstable();
            assert_eq!(found, group, "{territory}");
        }
    }
}

#[cfg(all(test, feature = "local-data"))]
mod retail_tests {
    use std::path::Path;

    use nw_content::ContentBundle;
    use nw_content::bundle::bundle_path;
    use nw_content::client_build::load_client_build;
    use nw_world::dungeon_scripts::OPEN_WORLD_LEVEL;
    use nw_world::world_file::world_file_path;

    use super::*;

    const CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
    /// The towers of the open world: three in each of 13 territories.
    const TOWERS: usize = 39;
    const TOWERS_PER_TERRITORY: usize = 3;
    /// Cutlass Keys and the entries of its conflict group (Observed).
    const CUTLASS_KEYS: u16 = 10;
    const CUTLASS_KEYS_ENTRIES: [u8; 3] = [18, 34, 42];

    #[test]
    fn the_open_world_gives_each_tower_its_entry_in_its_territory() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let build = load_client_build(&repo.join(CLIENT_BUILD_CONFIG)).expect("client build");
        let dir = repo.join("local-data/content");
        let bundle = ContentBundle::load(&bundle_path(&dir, &build), &build).expect("bundle");
        let level = WorldFile::load(&world_file_path(&dir, &build, OPEN_WORLD_LEVEL), &build)
            .expect("world");
        let rules = ControlPointRules::of(&bundle.tables, &level);
        assert_eq!(rules.towers.len(), TOWERS);
        for tower in &rules.towers {
            assert_eq!(tower_entry(&tower.tower_id), Some(tower.entry));
        }
        let mut territories: BTreeMap<u16, Vec<u8>> = BTreeMap::new();
        for tower in &rules.towers {
            territories.entry(tower.territory).or_default().push(tower.entry);
        }
        assert!(
            territories
                .values()
                .all(|entries| entries.len() == TOWERS_PER_TERRITORY)
        );
        assert_eq!(
            territories.get(&CUTLASS_KEYS),
            Some(&CUTLASS_KEYS_ENTRIES.to_vec())
        );
    }
}
