// SPDX-License-Identifier: AGPL-3.0-or-later
//! Expeditions, raids and trials (instances) from `GameModeData` and `GameModeMapData`: who may
//! enter, how many, the level each one loads, where the group appears, how long it lasts and what
//! the player's arena state counts (run 15 instances study, REPORT section 3).
//!
//! A game mode is an instance when its row has `IsDungeon` (expeditions, raids, soul and season
//! trials) or `ActivityType` `Catacombs`; the PvP modes (arenas, Outpost Rush, battle royale,
//! capture the flag) and the matchmaking groups `Dungeon` and `Mutation` (no map) are not.
//!
//! Rules from the retail captures (29 ledgers):
//!
//! - The level a player loads is the map's `CoatlicueName` (`coatlicue\NW_Dungeon_Windsward_00`):
//!   its last part is the map name of the 1635 and, in lower case, our level folder; the frame of
//!   the 1635 is `WorldBounds` (Observed in all 7 kinds but the generated catacombs, whose frame is
//!   not in the table: [`InstanceMode::generated`]).
//! - A group appears at `TeamTeleportData` (`x,y,z,yaw` entries joined by `+`).
//! - The arena state's `dungeonCooldownTime` is set at the teleport: 1800 s later for an
//!   expedition or raid (5 captures, 1794 to 1801 s after the 946), 300 s later for a solo trial,
//!   whose `enterSoloTrialCooldownTime` is the teleport time (15 captures).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;
use crate::generated::{GameModeData, GameModeMapData};

/// The `ActivityType` of a catacombs run.
const CATACOMBS_ACTIVITY: &str = "Catacombs";
/// The `ActivityType` of an expedition: its entries count as base dungeon entries.
const DUNGEON_ACTIVITY: &str = "Dungeon";
/// The folder part of a `CoatlicueName` (`coatlicue\NW_Dungeon_Windsward_00`).
const COATLICUE_FOLDER: &str = "coatlicue";
/// The separator of a `CoatlicueName` in the datasheet.
const DATASHEET_PATH_SEPARATOR: char = '\\';
/// The separator of the terrain path in the 1635 (`coatlicue/NW_Dungeon_Windsward_00`).
const WIRE_PATH_SEPARATOR: char = '/';
/// The separator of the numbers of `WorldBounds` and of one teleport entry.
const NUMBER_SEPARATOR: char = ',';
/// The separator of the teleport entries of `TeamTeleportData`.
const ENTRY_SEPARATOR: char = '+';
/// The separator of the item class lists of a game mode.
const LIST_SEPARATOR: char = ',';
/// The separator of an item's `ItemClass` list (`DungeonItem+CatacombsOnly`).
const ITEM_CLASS_SEPARATOR: char = '+';
/// Numbers of a `WorldBounds`: x, y, width, height.
const FRAME_NUMBERS: usize = 4;
/// Numbers of a teleport entry: x, y, z, yaw in degrees.
const TELEPORT_NUMBERS: usize = 4;
/// Players of a solo trial (`IsSoloTrial`, no `TeamCapacity`).
const SOLO_CAPACITY: u32 = 1;
/// Seconds from the teleport to `dungeonCooldownTime` for an expedition or a raid (Observed: 5
/// captures, 1794 to 1801 s, the spread being the capture clocks).
pub const GROUP_COOLDOWN_SECONDS: u64 = 1800;
/// Seconds from the teleport to `dungeonCooldownTime` for a solo trial (Observed: 15 captures).
pub const SOLO_COOLDOWN_SECONDS: u64 = 300;

/// The multiplier of the client's 64-bit key hash (`FUN_1414a1220`: the 128-bit product of the
/// key and this constant, its low and high halves added).
const KEY_HASH_MULTIPLIER: u128 = 0xde5f_b9d2_6304_58e9;
/// Bits of one half of the key hash product.
const HASH_HALF_BITS: u32 = u64::BITS;
/// Fewest buckets of the client's game mode table (an empty table has one).
const MIN_BUCKETS: usize = 1;
/// `m_gameModeIdx` of no game mode: the state of a player who is not in a soul trial (every
/// expedition, raid and catacombs capture), and of a row past the last index a byte holds.
pub const NO_INDEX: u8 = 0xff;

/// The kind of an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceKind {
    /// An expedition (`ActivityType` `Dungeon`).
    Expedition,
    /// A raid or group trial (`IsRaidTrial`).
    RaidTrial,
    /// A solo soul trial (`IsSoloTrial`).
    SoloTrial,
    /// A catacombs run.
    Catacombs,
}

/// Which of the arena state's counters an entry raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryCounter {
    /// `m_numBaseDungeonsEnteredSinceLastRefresh`.
    BaseDungeon,
    /// `m_numGroupTrialsEnteredSinceLastRefresh`.
    GroupTrial,
    /// None.
    Uncounted,
}

/// One spot where a group appears.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Teleport {
    /// Metres: east, north, up.
    pub position: [f32; 3],
    /// Yaw, degrees.
    pub yaw_deg: f32,
}

/// The level of an instance.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceMap {
    /// Our level folder: the lower-case map name (world files, sim config).
    pub folder: String,
    /// The map name of the 1635.
    pub map_name: String,
    /// The terrain path of the 1635.
    pub coatlicue_path: String,
    /// `WorldBounds`: x, y, width, height.
    pub frame: [f32; FRAME_NUMBERS],
    /// Where the group appears; the first is the default.
    pub teleports: Vec<Teleport>,
}

/// One instance game mode.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceMode {
    /// `GameModeID`.
    pub id: String,
    /// CRC-32 of the lower-case id: what 5039 and 5038 name.
    pub crc: u32,
    /// The kind.
    pub kind: InstanceKind,
    /// The level.
    pub map: InstanceMap,
    /// Most players (`TeamCapacity`; 1 for a solo trial).
    pub capacity: u32,
    /// Fewest group members (`MinGroupSize`; 0 when absent).
    pub min_group: u32,
    /// `RequiredLevel`.
    pub required_level: Option<u32>,
    /// `IsRemoteEntryAllowed`: 5039 and 5037 may start it from afar.
    pub remote_entry: bool,
    /// `DungeonRespawnsPermitted`.
    pub respawns: bool,
    /// `IsMutable`.
    pub mutable: bool,
    /// `OverrideDungeonTimeout`: the instance ends this many seconds after it opened.
    pub timeout_seconds: Option<u64>,
    /// `VictoryDelaySec`.
    pub victory_delay_seconds: Option<u64>,
    /// `ItemClassesToRemoveOnExit`.
    pub items_removed_on_exit: Vec<String>,
    /// `RequiredConsumableItemClasses`: an entry needs an item of one of them.
    pub required_item_classes: Vec<String>,
    /// `EntryAchievementConditional`: an entry needs this achievement.
    pub entry_achievement: Option<String>,
    /// `LootTags`.
    pub loot_tags: Vec<String>,
    /// `ContainerLevel`.
    pub container_level: Option<u32>,
    /// The counter an entry raises.
    pub counter: EntryCounter,
    /// `m_gameModeIdx`: the place of the mode in the client's game mode table
    /// ([`game_mode_indexes`]); the arena state sends it for a solo trial.
    pub game_mode_index: u8,
    /// The level is generated per run (`LevelGeneratorType`, the catacombs): its tiles and frame
    /// are not in the tables.
    pub generated: bool,
}

impl InstanceMode {
    /// Seconds from the teleport to the arena state's `dungeonCooldownTime`.
    pub const fn cooldown_seconds(&self) -> u64 {
        match self.kind {
            InstanceKind::SoloTrial => SOLO_COOLDOWN_SECONDS,
            _ => GROUP_COOLDOWN_SECONDS,
        }
    }
}

/// Every instance game mode, by CRC.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstanceCatalog {
    modes: BTreeMap<u32, InstanceMode>,
    /// The `ItemClass` entries (lower case) of each item (lower-case id) that has a class an
    /// instance names (`ItemClassesToRemoveOnExit`, `RequiredConsumableItemClasses`).
    item_classes: HashMap<String, Vec<String>>,
}

impl InstanceCatalog {
    /// The instances of `tables`. A game mode without a map row, without a `CoatlicueName`, or with
    /// a `WorldBounds` or `TeamTeleportData` that does not read is left out.
    pub fn from_tables(tables: &Tables) -> Self {
        let maps: BTreeMap<&str, &GameModeMapData> = tables
            .game_mode_map_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.game_mode_id.as_str(), row))
            .collect();
        let rows: Vec<&GameModeData> = tables
            .game_mode_data
            .iter()
            .flat_map(|table| &table.rows)
            .collect();
        let indexes = game_mode_indexes(rows.iter().map(|row| row.game_mode_id.as_str()));
        let modes = rows
            .iter()
            .filter_map(|row| {
                let map = maps.get(row.game_mode_id.as_str())?;
                let index = indexes
                    .get(row.game_mode_id.as_str())
                    .copied()
                    .unwrap_or(NO_INDEX);
                instance_mode(row, map, index)
            })
            .map(|mode| (mode.crc, mode))
            .collect::<BTreeMap<_, _>>();
        let named: BTreeSet<String> = modes
            .values()
            .flat_map(|mode| {
                mode.items_removed_on_exit
                    .iter()
                    .chain(&mode.required_item_classes)
            })
            .map(|class| class.to_lowercase())
            .collect();
        let item_classes = tables
            .master_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let classes: Vec<String> = row
                    .item_class
                    .as_deref()?
                    .split(ITEM_CLASS_SEPARATOR)
                    .map(|class| class.trim().to_lowercase())
                    .filter(|class| named.contains(class))
                    .collect();
                (!classes.is_empty()).then(|| (row.item_id.to_lowercase(), classes))
            })
            .collect();
        Self {
            item_classes,
            ..Self::build(modes.into_values())
        }
    }

    /// The catalog of `modes`, with no item classes.
    pub fn build(modes: impl IntoIterator<Item = InstanceMode>) -> Self {
        Self {
            modes: modes.into_iter().map(|mode| (mode.crc, mode)).collect(),
            item_classes: HashMap::new(),
        }
    }

    /// Whether item `item_id` has one of `classes` (`ItemClass` entries, any case).
    pub fn item_has_class(&self, item_id: &str, classes: &[String]) -> bool {
        self.item_classes
            .get(&item_id.to_lowercase())
            .is_some_and(|own| {
                classes
                    .iter()
                    .any(|class| own.iter().any(|own| own.eq_ignore_ascii_case(class)))
            })
    }

    /// The mode whose lower-case id has CRC-32 `crc`.
    pub fn by_crc(&self, crc: u32) -> Option<&InstanceMode> {
        self.modes.get(&crc)
    }

    /// The mode `id`. A retail mode's CRC is the CRC-32 of its lower-case id. A mode stored
    /// under another CRC (a test catalog) is found by its id.
    pub fn by_id(&self, id: &str) -> Option<&InstanceMode> {
        self.by_crc(lowercase_crc32(id)).or_else(|| {
            self.modes
                .values()
                .find(|mode| mode.id.eq_ignore_ascii_case(id))
        })
    }

    /// The modes whose level is `folder`.
    pub fn of_level<'a>(&'a self, folder: &'a str) -> impl Iterator<Item = &'a InstanceMode> {
        self.modes
            .values()
            .filter(move |mode| mode.map.folder == folder)
    }

    /// Every mode.
    pub fn modes(&self) -> impl Iterator<Item = &InstanceMode> {
        self.modes.values()
    }

    /// Number of modes.
    pub fn len(&self) -> usize {
        self.modes.len()
    }

    /// Whether there is no mode.
    pub fn is_empty(&self) -> bool {
        self.modes.is_empty()
    }
}

/// The client's key hash of a game mode CRC (`FUN_1414a1220`, applied to the zero-extended
/// CRC).
fn key_hash(crc: u32) -> u64 {
    let product = u128::from(crc) * KEY_HASH_MULTIPLIER;
    // Truncation is the rule: the low and high halves of the product are added, wrapping.
    let low = product as u64;
    let high = (product >> HASH_HALF_BITS) as u64;
    low.wrapping_add(high)
}

/// `m_gameModeIdx` of every game mode id of `ids` (the `javelindata_gamemodes` rows, in
/// datasheet order): its place in the client's game mode table (run 15 dungeon2 study,
/// `REPORT-gamemodeidx.md`).
///
/// `FUN_142789c50` turns the index into a game mode by walking the `GameModeDataManager` table
/// (`FUN_14324c120` inserts each row under the lower-case CRC of its id, `FUN_14322d840`,
/// rehash `FUN_14322f810`) and counting its entries. That walk visits the buckets from the last
/// to the first, and the rows of one bucket in datasheet order; the bucket of a row is its
/// [`key_hash`] masked by the bucket count, the smallest power of two that holds every row
/// (Observed: 64 buckets for the 53 rows give all 21 captured trial indexes; the growth rule
/// for other row counts is a best guess). A place past the last a byte holds is [`NO_INDEX`].
pub fn game_mode_indexes<'a>(ids: impl IntoIterator<Item = &'a str>) -> HashMap<&'a str, u8> {
    let ids: Vec<&str> = ids.into_iter().collect();
    let mask = (ids.len().max(MIN_BUCKETS).next_power_of_two() - MIN_BUCKETS) as u64;
    let mut order: Vec<(u64, usize, &str)> = ids
        .iter()
        .enumerate()
        .map(|(row, id)| (key_hash(lowercase_crc32(id)) & mask, row, *id))
        .collect();
    order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    order
        .into_iter()
        .enumerate()
        .map(|(place, (_, _, id))| (id, u8::try_from(place).unwrap_or(NO_INDEX)))
        .collect()
}

fn instance_mode(
    row: &GameModeData,
    map: &GameModeMapData,
    game_mode_index: u8,
) -> Option<InstanceMode> {
    let catacombs = row.activity_type.as_deref() == Some(CATACOMBS_ACTIVITY);
    if !row.is_dungeon.unwrap_or_default() && !catacombs {
        return None;
    }
    let kind = if catacombs {
        InstanceKind::Catacombs
    } else if row.is_solo_trial.unwrap_or_default() {
        InstanceKind::SoloTrial
    } else if row.is_raid_trial.unwrap_or_default() {
        InstanceKind::RaidTrial
    } else {
        InstanceKind::Expedition
    };
    let counter = match kind {
        InstanceKind::Expedition if row.activity_type.as_deref() == Some(DUNGEON_ACTIVITY) => {
            EntryCounter::BaseDungeon
        }
        InstanceKind::RaidTrial => EntryCounter::GroupTrial,
        _ => EntryCounter::Uncounted,
    };
    let capacity = match (row.team_capacity, kind) {
        (Some(capacity), _) => u32::try_from(capacity).ok()?,
        (None, InstanceKind::SoloTrial) => SOLO_CAPACITY,
        (None, _) => return None,
    };
    Some(InstanceMode {
        id: row.game_mode_id.clone(),
        crc: lowercase_crc32(&row.game_mode_id),
        kind,
        map: instance_map(map)?,
        capacity,
        min_group: unsigned(row.min_group_size).unwrap_or_default(),
        required_level: unsigned(row.required_level),
        remote_entry: row.is_remote_entry_allowed.unwrap_or_default(),
        respawns: row.dungeon_respawns_permitted.unwrap_or_default(),
        mutable: row.is_mutable.unwrap_or_default(),
        timeout_seconds: row
            .override_dungeon_timeout
            .and_then(|v| u64::try_from(v).ok()),
        victory_delay_seconds: row.victory_delay_sec.and_then(|v| u64::try_from(v).ok()),
        items_removed_on_exit: list(row.item_classes_to_remove_on_exit.as_deref()),
        required_item_classes: list(row.required_consumable_item_classes.as_deref()),
        entry_achievement: row.entry_achievement_conditional.clone(),
        loot_tags: list(row.loot_tags.as_deref()),
        container_level: unsigned(row.container_level),
        counter,
        game_mode_index,
        generated: row.level_generator_type.is_some(),
    })
}

fn instance_map(map: &GameModeMapData) -> Option<InstanceMap> {
    let coatlicue = map.coatlicue_name.as_deref()?;
    let map_name = coatlicue
        .rsplit(DATASHEET_PATH_SEPARATOR)
        .next()?
        .to_owned();
    let frame: [f32; FRAME_NUMBERS] = numbers(map.world_bounds.as_deref()?)?.try_into().ok()?;
    let teleports = map
        .team_teleport_data
        .as_deref()?
        .split(ENTRY_SEPARATOR)
        .map(|entry| {
            let [x, y, z, yaw_deg]: [f32; TELEPORT_NUMBERS] = numbers(entry)?.try_into().ok()?;
            Some(Teleport {
                position: [x, y, z],
                yaw_deg,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if teleports.is_empty() {
        return None;
    }
    Some(InstanceMap {
        folder: map_name.to_lowercase(),
        coatlicue_path: format!("{COATLICUE_FOLDER}{WIRE_PATH_SEPARATOR}{map_name}"),
        map_name,
        frame,
        teleports,
    })
}

/// The numbers of `text`, comma separated; `None` when one does not read.
fn numbers(text: &str) -> Option<Vec<f32>> {
    text.split(NUMBER_SEPARATOR)
        .map(|number| number.trim().parse::<f32>().ok())
        .collect()
}

fn list(text: Option<&str>) -> Vec<String> {
    text.into_iter()
        .flat_map(|text| text.split(LIST_SEPARATOR))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn unsigned(value: Option<i64>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;

    /// A game mode row with only the required columns set (every `Option` column is absent).
    fn mode_row(id: &str) -> GameModeData {
        serde_json::from_value(serde_json::json!({
            "allow_camping": false,
            "feature_flag": "",
            "game_mode_id": id,
            "raid_invite_delay_sec": 0,
            "raid_invite_duration_sec": 0,
            "script_name": id,
        }))
        .expect("game mode row")
    }

    fn map_row(id: &str, coatlicue: &str, bounds: &str, teleport: &str) -> GameModeMapData {
        serde_json::from_value(serde_json::json!({
            "game_mode_id": id,
            "game_mode_map_id": id,
            "coatlicue_name": coatlicue,
            "world_bounds": bounds,
            "team_teleport_data": teleport,
        }))
        .expect("game mode map row")
    }

    fn tables(modes: Vec<GameModeData>, maps: Vec<GameModeMapData>) -> Tables {
        let mut tables = Tables::default();
        tables.game_mode_data.push(Table {
            source_path: "javelindata_gamemodes.datasheet".to_owned(),
            unique_id: "GameModes".to_owned(),
            rows: modes,
        });
        tables.game_mode_map_data.push(Table {
            source_path: "javelindata_gamemodemaps.datasheet".to_owned(),
            unique_id: "GameModeMaps".to_owned(),
            rows: maps,
        });
        tables
    }

    /// The retail Amrine rows (javelindata_gamemodes and javelindata_gamemodemaps, build
    /// 25810808).
    fn amrine() -> (GameModeData, GameModeMapData) {
        let mut row = mode_row("DungeonAmrine");
        row.activity_type = Some(DUNGEON_ACTIVITY.to_owned());
        row.is_dungeon = Some(true);
        row.team_capacity = Some(5);
        row.min_group_size = Some(3);
        row.is_remote_entry_allowed = Some(true);
        row.dungeon_respawns_permitted = Some(true);
        row.item_classes_to_remove_on_exit = Some("DungeonItem".to_owned());
        row.loot_tags = Some("Amrine,Lost,Dungeon".to_owned());
        row.container_level = Some(25);
        let map = map_row(
            "DungeonAmrine",
            "coatlicue\\NW_Dungeon_Windsward_00",
            "610.0,560.0,352.0,512.0",
            "645.0,663.0,153.0,0",
        );
        (row, map)
    }

    #[test]
    fn an_expedition_reads_its_map_rules_and_counter() {
        let (row, map) = amrine();
        let catalog = InstanceCatalog::from_tables(&tables(vec![row], vec![map]));
        assert_eq!(catalog.len(), 1);
        let mode = catalog.by_id("DungeonAmrine").expect("mode");
        assert_eq!(catalog.by_crc(lowercase_crc32("dungeonamrine")), Some(mode));
        assert_eq!(mode.kind, InstanceKind::Expedition);
        assert_eq!(mode.counter, EntryCounter::BaseDungeon);
        assert_eq!((mode.capacity, mode.min_group), (5, 3));
        assert!(mode.remote_entry && mode.respawns && !mode.generated);
        assert_eq!(mode.items_removed_on_exit, ["DungeonItem"]);
        assert_eq!(mode.loot_tags, ["Amrine", "Lost", "Dungeon"]);
        assert_eq!(mode.map.folder, "nw_dungeon_windsward_00");
        assert_eq!(mode.map.map_name, "NW_Dungeon_Windsward_00");
        assert_eq!(mode.map.coatlicue_path, "coatlicue/NW_Dungeon_Windsward_00");
        assert_eq!(mode.map.frame, [610.0, 560.0, 352.0, 512.0]);
        assert_eq!(
            mode.map.teleports,
            [Teleport {
                position: [645.0, 663.0, 153.0],
                yaw_deg: 0.0,
            }]
        );
        assert_eq!(mode.game_mode_index, 0, "the only row of its table");
        assert_eq!(mode.cooldown_seconds(), GROUP_COOLDOWN_SECONDS);
        assert_eq!(catalog.of_level("nw_dungeon_windsward_00").count(), 1);
    }

    #[test]
    fn a_solo_trial_has_room_for_one() {
        let mut row = mode_row("TrialSeason04Q06");
        row.is_dungeon = Some(true);
        row.is_solo_trial = Some(true);
        row.override_dungeon_timeout = Some(1800);
        row.required_level = Some(61);
        let map = map_row(
            "TrialSeason04Q06",
            "coatlicue\\NW_Trial_Season_04_DeviceRoom",
            "121.0,128.0,256.0,256.0",
            "172.82,254.97,260.34,270",
        );
        let catalog = InstanceCatalog::from_tables(&tables(vec![row], vec![map]));
        let mode = catalog.by_id("TrialSeason04Q06").expect("mode");
        assert_eq!(mode.kind, InstanceKind::SoloTrial);
        assert_eq!(mode.capacity, SOLO_CAPACITY);
        assert_eq!(mode.counter, EntryCounter::Uncounted);
        assert_eq!(mode.game_mode_index, 0, "the only row of its table");
        assert_eq!(mode.timeout_seconds, Some(1800));
        assert_eq!(mode.required_level, Some(61));
        assert_eq!(mode.cooldown_seconds(), SOLO_COOLDOWN_SECONDS);
    }

    #[test]
    fn pvp_modes_rows_without_a_map_and_bad_frames_are_left_out() {
        let mut arena = mode_row("Arena3v3");
        arena.team_capacity = Some(3);
        let arena_map = map_row(
            "Arena3v3",
            "coatlicue\\NW_Arena01",
            "864,832,992,960",
            "939,935,226.5,180",
        );
        let mut lost = mode_row("QuestLost");
        lost.is_dungeon = Some(true);
        lost.is_solo_trial = Some(true);
        let (amrine, mut amrine_map) = amrine();
        amrine_map.world_bounds = Some("610.0,560.0".to_owned());
        let catalog = InstanceCatalog::from_tables(&tables(
            vec![arena, lost, amrine],
            vec![arena_map, amrine_map],
        ));
        assert!(catalog.is_empty());
    }

    #[test]
    fn the_items_of_an_exit_class_are_known_by_id() {
        let (row, map) = amrine();
        let mut tables = tables(vec![row], vec![map]);
        let items: Vec<crate::generated::MasterItemDefinitions> = [
            ("Catacombs_Currency_Key", Some("DungeonItem+CatacombsOnly")),
            ("PotionHealthT1", Some("Consumable+Potion")),
            ("Unclassed", None),
        ]
        .into_iter()
        .map(|(id, class)| {
            serde_json::from_value(serde_json::json!({
                "item_id": id,
                "item_class": class,
            }))
            .expect("item row")
        })
        .collect();
        tables.master_item_definitions.push(Table {
            source_path: "javelindata_itemdefinitions_master.datasheet".to_owned(),
            unique_id: "MasterItemDefinitions".to_owned(),
            rows: items,
        });
        let catalog = InstanceCatalog::from_tables(&tables);
        let mode = catalog.by_id("DungeonAmrine").expect("mode");
        assert!(catalog.item_has_class("catacombs_currency_key", &mode.items_removed_on_exit));
        assert!(!catalog.item_has_class("PotionHealthT1", &mode.items_removed_on_exit));
        assert!(!catalog.item_has_class("Unclassed", &mode.items_removed_on_exit));
    }

    #[test]
    fn a_raid_counts_as_a_group_trial_with_two_teleport_spots() {
        let mut row = mode_row("RaidCutlassKeys00");
        row.is_dungeon = Some(true);
        row.is_raid_trial = Some(true);
        row.team_capacity = Some(10);
        row.min_group_size = Some(5);
        let map = map_row(
            "RaidCutlassKeys00",
            "coatlicue\\NW_Raid_CutlassKeys_00",
            "545.0,574.0,800.0,768.0",
            "569.0,1237.0,120.0,260+570.0,1238.0,120.0,250",
        );
        let catalog = InstanceCatalog::from_tables(&tables(vec![row], vec![map]));
        let mode = catalog.by_id("RaidCutlassKeys00").expect("mode");
        assert_eq!(mode.kind, InstanceKind::RaidTrial);
        assert_eq!(mode.counter, EntryCounter::GroupTrial);
        assert_eq!(mode.map.teleports.len(), 2);
    }

    /// The `GameModeID` column of `javelindata_gamemodes` (build 25810808), in datasheet order.
    const RETAIL_GAME_MODE_IDS: [&str; 53] = [
        "TestMode1",
        "TestMode2",
        "BattleRoyale",
        "CaptureTheFlag",
        "Duel",
        "OutpostRush",
        "OutpostRush_NoPerks",
        "Arena3v3",
        "Dungeon",
        "Mutation",
        "DungeonAmrine",
        "DungeonShatteredObelisk",
        "DungeonRestlessShores01",
        "DungeonEbonscale00",
        "DungeonEdengrove00",
        "DungeonReekwater00",
        "DungeonCutlassKeys00",
        "DungeonGreatCleave01",
        "DungeonShatterMtn00",
        "DungeonBrimstoneSands00",
        "DungeonFirstLight01",
        "DungeonGreatCleave00",
        "DungeonDunwood02",
        "TrialBrimstoneSandworm",
        "RaidCutlassKeys00",
        "RaidMonarchsBluffs00",
        "TrialSeason01",
        "TrialSeason04",
        "QuestAdiana",
        "QuestApophis",
        "QuestCaerDun",
        "QuestEmpressZhou",
        "QuestIsabella",
        "QuestMammoth",
        "QuestMedusa",
        "QuestMotherwell",
        "QuestMedea",
        "QuestNihilo",
        "QuestYonas",
        "TrialSeason02Q13",
        "TrialSeason02Q12",
        "TrialSeason04Q02",
        "TrialSeason04Q06",
        "TrialSeason04Q05",
        "QuestHeartforge",
        "QuestLiang",
        "QuestGawain",
        "Catacombs",
        "QuestDracula",
        "QuestWolf",
        "QuestMorgaine",
        "QuestVarlaam",
        "QuestSeraphim",
    ];

    /// `m_gameModeIdx` as retail sent it after the 6776 of each trial (17 soul trial ledgers and
    /// the 26 ledgers of the dungeon2 scan; one value per trial in every ledger).
    const CAPTURED_INDEXES: [(&str, u8); 21] = [
        ("QuestMedea", 0),
        ("TrialSeason02Q12", 1),
        ("TrialSeason04Q02", 2),
        ("QuestYonas", 5),
        ("QuestAdiana", 10),
        ("QuestDracula", 12),
        ("QuestVarlaam", 14),
        ("QuestCaerDun", 15),
        ("QuestLiang", 22),
        ("QuestIsabella", 26),
        ("QuestNihilo", 29),
        ("QuestMorgaine", 32),
        ("QuestHeartforge", 38),
        ("QuestEmpressZhou", 39),
        ("TrialSeason04Q05", 41),
        ("QuestMotherwell", 44),
        ("QuestSeraphim", 45),
        ("QuestMammoth", 47),
        ("TrialSeason04Q06", 48),
        ("QuestGawain", 49),
        ("QuestWolf", 50),
    ];

    #[test]
    fn the_retail_rows_give_every_captured_trial_index() {
        let indexes = game_mode_indexes(RETAIL_GAME_MODE_IDS);
        assert_eq!(indexes.len(), RETAIL_GAME_MODE_IDS.len());
        for (id, index) in CAPTURED_INDEXES {
            assert_eq!(indexes[id], index, "{id}");
        }
        // Every row has its own place.
        let places: BTreeSet<u8> = indexes.values().copied().collect();
        assert_eq!(places.len(), RETAIL_GAME_MODE_IDS.len());
        assert!(
            places
                .iter()
                .all(|place| usize::from(*place) < RETAIL_GAME_MODE_IDS.len())
        );
    }

    #[test]
    fn rows_of_one_bucket_keep_their_datasheet_order() {
        // Buckets 62, 10 and 2 of the retail table hold two captured trials each.
        let indexes = game_mode_indexes(RETAIL_GAME_MODE_IDS);
        for (first, second) in [
            ("TrialSeason02Q12", "TrialSeason04Q02"),
            ("QuestMotherwell", "QuestSeraphim"),
            ("QuestGawain", "QuestWolf"),
        ] {
            assert_eq!(indexes[first] + 1, indexes[second]);
        }
    }

    #[test]
    fn the_catalog_takes_the_index_of_the_whole_table() {
        let mut modes = Vec::new();
        let mut maps = Vec::new();
        for id in RETAIL_GAME_MODE_IDS {
            let mut row = mode_row(id);
            if id == "TrialSeason04Q06" {
                row.is_dungeon = Some(true);
                row.is_solo_trial = Some(true);
                maps.push(map_row(
                    id,
                    "coatlicue\\NW_Trial_Season_04_DeviceRoom",
                    "121.0,128.0,256.0,256.0",
                    "172.82,254.97,260.34,270",
                ));
            }
            modes.push(row);
        }
        let catalog = InstanceCatalog::from_tables(&tables(modes, maps));
        assert_eq!(catalog.len(), 1);
        assert_eq!(
            catalog
                .by_id("TrialSeason04Q06")
                .expect("mode")
                .game_mode_index,
            48
        );
    }

    #[test]
    fn the_key_hash_adds_the_halves_of_the_product() {
        assert_eq!(key_hash(0), 0);
        assert_eq!(key_hash(1), 0xde5f_b9d2_6304_58e9);
        // 2 * multiplier carries one bit into the high half.
        assert_eq!(key_hash(2), 0xbcbf_73a4_c608_b1d2 + 1);
    }
}
