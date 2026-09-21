// SPDX-License-Identifier: AGPL-3.0-or-later
//! The gameplay values of our remote config layers (study note
//! `docs/re/2026-09-18-remote-config-layers.md`), as the server reads them.
//!
//! The client reads the layers as one key stack: each layer is a JSON object, and a key is the
//! dotted path of a value (`javelin.xp-multipliers.progression`; the client registers its
//! variables by these names, for example `FUN_1401a4630` for that key in build 25810808). A key
//! may also be written flat inside its object (`"storage-search.enabled"` in the `javelin`
//! object): both give the same dotted key. A later layer replaces a key of an earlier one as a
//! whole, arrays included (Inferred: retail `CognitoId` gives `Musician:2` where `ProductId`
//! gives `Music:2`).
//!
//! [`GameplayConfig`] holds every gameplay key the server is responsible for. A key that no
//! layer sets is `None` (or an empty list): the system then keeps its table value.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value};

use crate::datatable_overrides::{DatatableOverride, DatatableOverrideError};

/// Separates the parts of a dotted key.
const KEY_SEPARATOR: char = '.';
/// Separates the id and the value of a `<id>:<value>` list entry (`UmbralCurrency:3.0`,
/// `Trial:3`; client parser `FUN_143cad9b0` splits on `0x3a`).
const ENTRY_SEPARATOR: char = ':';
/// Separates the parts of a `javelin.schedule-overrides` entry (`Mammoth;<start>;<end>`).
const SCHEDULE_SEPARATOR: char = ';';
/// The multiplier that changes nothing: the client's default for a progression without an entry
/// (`FUN_143cad9b0` stores 1.0 for it).
pub const NEUTRAL_MULTIPLIER: f64 = 1.0;

/// The keys, as the client names them.
pub mod keys {
    /// Character experience multiplier (server only: no client reader in build 25810808).
    pub const XP_CHARACTER: &str = "javelin.xp-multipliers.character";
    /// Categorical progression multipliers, `<CategoricalProgressionId>:<multiplier>`.
    pub const XP_PROGRESSION: &str = "javelin.xp-multipliers.progression";
    /// Item classes that are disabled.
    pub const DISABLED_ITEM_CLASSES: &str = "javelin.disabled-item-classes";
    /// The most coins a player may hold.
    pub const WALLET_CAP_PLAYER: &str = "javelin.wallet-cap-player";
    /// Paid gear set storage loadouts.
    pub const PAID_LOADOUTS: &str = "javelin.gear-set-storage-max-num-paid-loadouts";
    /// Whether a territory's entry requirement applies.
    pub const TERRITORY_ENTRY_REQUIREMENT: &str = "javelin.territory-entry-requirement-enabled";
    /// Storage search switches.
    pub const STORAGE_SEARCH: &str = "javelin.storage-search.enabled";
    /// Storage search in houses.
    pub const STORAGE_SEARCH_HOUSING: &str = "javelin.storage-search.housing-enabled";
    /// Storage search at crafting stations.
    pub const STORAGE_SEARCH_CRAFTING: &str = "javelin.storage-search.crafting-enabled";
    /// Character level needed for each house, in order.
    pub const HOUSE_LEVEL_REQS: &str = "javelin.housing.house-level-reqs";
    /// Every player owns every appearance for transmog.
    pub const TRANSMOG_OWN_ALL: &str = "javelin.transmog-own-all-appearances";
    /// Datasheet cell overrides.
    pub const DYNAMIC_DATATABLE_CONFIG: &str = "javelin.dynamic-datatable-config";
    /// Spawn slices that do not spawn.
    pub const RED_BUTTON_SPAWN_SLICES: &str = "javelin.red-button-disable-spawn-slice-list";
    /// Achievement corrections.
    pub const RED_BUTTON_ACHIEVEMENT_CORRECTION: &str =
        "javelin.red-button-catch-all-achievement-correction";
    /// NPC override lines.
    pub const RED_BUTTON_NPA_OVERRIDE: &str = "javelin.red-button-npa-override";
    /// Traffic mitigation switch (`public` application).
    pub const RED_BUTTON_TRAFFIC_MITIGATION: &str = "javelin.red-button-disable-traffic-mitigation";
    /// World event schedule overrides, `<id>;<start>;<end>`.
    pub const SCHEDULE_OVERRIDES: &str = "javelin.schedule-overrides";
    /// End of the season pass season.
    pub const SEASON_END: &str = "javelin.seasons-rewards.season-end-datetime";
    /// End of the leaderboard season.
    pub const LEADERBOARD_SEASON_END: &str = "javelin.leaderboard-season-end-datetime";
    /// Most mutated expedition runs.
    pub const DUNGEON_MUTATED_MAX_RUNS: &str = "javelin.gamemode-dungeon-mutated-max-runs";
    /// Longest refresh of a group trial.
    pub const GROUP_TRIAL_MAX_REFRESH: &str = "javelin.gamemode-group-trial-max-refresh-seconds";
    /// Seed modifier of the mutations.
    pub const MUTATIONS_SEED_MODIFIER: &str = "javelin.gamemode-mutations.seedModifier";
    /// Forced refresh of the mutations.
    pub const MUTATIONS_FORCE_REFRESH: &str = "javelin.gamemode-mutations.forceRefresh";
    /// Most game modes scheduled at a time, `<mode>:<count>`.
    pub const SCHEDULER_MAX: &str = "javelin.gamemode-scheduler.maxToSchedule";
    /// Scheduler cadence, `<mode>:<seconds>`.
    pub const SCHEDULER_CADENCE: &str = "javelin.gamemode-scheduler.cadenceInSec";
    /// Activity penalties, `<activity>:<seconds>`.
    pub const ACTIVITY_PENALTIES: &str = "javelin.gamemode-activity-type-penalty-overrides";
    /// Game modes the matchmaking service runs.
    pub const MATCHMAKING_MODES: &str = "javelin.game-mode-matchmaking-service-enabled-modes";
    /// Company-only war slots.
    pub const COMPANY_ONLY_WAR_SLOTS: &str = "javelin.social.enable-company-only-war-slots";
    /// The server's Twitch system.
    pub const TWITCH_SYSTEM: &str = "javelin-server.enable-twitch-system";
}

/// A layer value that the server cannot read.
#[derive(Debug, thiserror::Error)]
pub enum GameplayConfigError {
    /// A key has a value of another type.
    #[error("remote config key {key}: expected {expected}")]
    WrongType {
        /// The dotted key.
        key: &'static str,
        /// What the key holds.
        expected: &'static str,
    },
    /// A list entry is not in its key's format.
    #[error("remote config key {key}: entry {entry:?} {reason}")]
    BadEntry {
        /// The dotted key.
        key: &'static str,
        /// The entry.
        entry: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// A `javelin.dynamic-datatable-config` line is not valid.
    #[error(transparent)]
    Datatable(#[from] DatatableOverrideError),
}

/// The layers merged into one key stack: every leaf by its dotted key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyStack {
    values: BTreeMap<String, Value>,
}

impl KeyStack {
    /// The stack of `layers`, lowest first: a later layer's key replaces an earlier one's.
    pub fn merge<'a>(layers: impl IntoIterator<Item = &'a Map<String, Value>>) -> Self {
        let mut values = BTreeMap::new();
        for layer in layers {
            flatten_into(&mut values, None, layer);
        }
        Self { values }
    }

    /// The value of `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    fn bool(&self, key: &'static str) -> Result<Option<bool>, GameplayConfigError> {
        self.typed(key, "true or false", Value::as_bool)
    }

    fn unsigned(&self, key: &'static str) -> Result<Option<u64>, GameplayConfigError> {
        self.typed(key, "a whole number of at least 0", Value::as_u64)
    }

    fn signed(&self, key: &'static str) -> Result<Option<i64>, GameplayConfigError> {
        self.typed(key, "a whole number", Value::as_i64)
    }

    fn number(&self, key: &'static str) -> Result<Option<f64>, GameplayConfigError> {
        self.typed(key, "a number", Value::as_f64)
    }

    fn text(&self, key: &'static str) -> Result<Option<String>, GameplayConfigError> {
        self.typed(key, "a string", |value| value.as_str().map(str::to_owned))
    }

    /// A list of strings; empty when no layer sets it.
    fn texts(&self, key: &'static str) -> Result<Vec<String>, GameplayConfigError> {
        let wrong = GameplayConfigError::WrongType {
            key,
            expected: "a list of strings",
        };
        let Some(value) = self.values.get(key) else {
            return Ok(Vec::new());
        };
        value
            .as_array()
            .ok_or(wrong)?
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(GameplayConfigError::WrongType {
                        key,
                        expected: "a list of strings",
                    })
            })
            .collect()
    }

    fn typed<T>(
        &self,
        key: &'static str,
        expected: &'static str,
        read: impl Fn(&Value) -> Option<T>,
    ) -> Result<Option<T>, GameplayConfigError> {
        self.values
            .get(key)
            .map(|value| read(value).ok_or(GameplayConfigError::WrongType { key, expected }))
            .transpose()
    }
}

/// Puts every leaf of `object` into `values` under its dotted key below `prefix`.
fn flatten_into(
    values: &mut BTreeMap<String, Value>,
    prefix: Option<&str>,
    object: &Map<String, Value>,
) {
    for (name, value) in object {
        let key = match prefix {
            Some(prefix) => format!("{prefix}{KEY_SEPARATOR}{name}"),
            None => name.clone(),
        };
        match value {
            Value::Object(inner) => flatten_into(values, Some(&key), inner),
            leaf => {
                values.insert(key, leaf.clone());
            }
        }
    }
}

/// `<id>:<value>` entries of `key`, each value read by `parse`.
fn entries<T>(
    stack: &KeyStack,
    key: &'static str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<Vec<(String, T)>, GameplayConfigError> {
    stack
        .texts(key)?
        .into_iter()
        .map(|entry| {
            let bad = |reason| GameplayConfigError::BadEntry {
                key,
                entry: entry.clone(),
                reason,
            };
            let (id, value) = entry
                .split_once(ENTRY_SEPARATOR)
                .ok_or_else(|| bad("has no ':'"))?;
            let value = parse(value.trim()).ok_or_else(|| bad("has a value of another type"))?;
            Ok((id.trim().to_owned(), value))
        })
        .collect()
}

/// `javelin.xp-multipliers`.
#[derive(Debug, Clone, PartialEq)]
pub struct XpMultipliers {
    /// `character`: the multiplier of every character experience gain (quests, kills).
    /// Inferred from retail: a level 1 kill gave 20 points (tutorial note, section 8) where
    /// `RewardData` level 1 `KillExperience` is 10 and this key is 2.
    pub character: f64,
    /// `progression`: the multiplier of each categorical progression by lowercase
    /// `CategoricalProgressionId` (Observed, content bundle 25810808: the tradeskills such as
    /// `Weaponsmithing` and `Musician`, the weapon masteries `<Weapon>AbilityTable`,
    /// `UmbralCurrency`, and the numeric ids `2`, `4`-`6`, `8`-`17`, which are the
    /// territory standing rows `@Brightwood` ... `@Dunwood`; `1`, `3`, `7` and `Music` name no
    /// row). The client applies each to the progression with that id (`FUN_143cad9b0`).
    pub progression: HashMap<String, f64>,
}

impl Default for XpMultipliers {
    fn default() -> Self {
        Self {
            character: NEUTRAL_MULTIPLIER,
            progression: HashMap::new(),
        }
    }
}

impl XpMultipliers {
    /// The character experience that a gain of `points` gives. The rounding of a fractional
    /// result is Unknown (retail values are whole); it rounds to the nearest point here.
    pub fn character_gain(&self, points: u32) -> u32 {
        let gained = (f64::from(points) * self.character).round();
        if gained >= f64::from(u32::MAX) {
            u32::MAX
        } else if gained > 0.0 {
            // In range: checked above.
            gained as u32
        } else {
            0
        }
    }

    /// The multiplier of categorical progression `id` (any case); neutral without an entry.
    pub fn progression(&self, id: &str) -> f64 {
        self.progression
            .get(&id.to_lowercase())
            .copied()
            .unwrap_or(NEUTRAL_MULTIPLIER)
    }
}

/// `javelin.storage-search.*`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageSearch {
    /// `enabled`.
    pub enabled: Option<bool>,
    /// `housing-enabled`.
    pub housing_enabled: Option<bool>,
    /// `crafting-enabled`.
    pub crafting_enabled: Option<bool>,
}

/// The red button lists: emergency switches of live content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RedButton {
    /// `red-button-disable-spawn-slice-list`: spawn slices that do not spawn (client reader
    /// `FUN_143c14700`).
    pub disabled_spawn_slices: Vec<String>,
    /// `red-button-catch-all-achievement-correction` lines, as written.
    pub achievement_corrections: Vec<String>,
    /// `red-button-npa-override` lines, as written.
    pub npa_overrides: Vec<String>,
    /// `red-button-disable-traffic-mitigation`.
    pub traffic_mitigation_disabled: Option<bool>,
}

/// One `javelin.schedule-overrides` entry. The times are kept as written: one retail entry
/// (`FireNaga2;2026-06-30-T17:00:0;...`) is not an ISO time, and how the client reads it is
/// Unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleOverride {
    /// The schedule id (`Mammoth`).
    pub id: String,
    /// The start time text.
    pub start: String,
    /// The end time text.
    pub end: String,
}

/// The season end times, as written (`2027-01-31T00:00:00`; the time zone is Unknown).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seasons {
    /// `seasons-rewards.season-end-datetime`.
    pub season_end: Option<String>,
    /// `leaderboard-season-end-datetime`.
    pub leaderboard_season_end: Option<String>,
}

/// The game mode keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameModes {
    /// `gamemode-dungeon-mutated-max-runs`.
    pub dungeon_mutated_max_runs: Option<u64>,
    /// `gamemode-group-trial-max-refresh-seconds`.
    pub group_trial_max_refresh_seconds: Option<u64>,
    /// `gamemode-mutations.seedModifier`.
    pub mutations_seed_modifier: Option<i64>,
    /// `gamemode-mutations.forceRefresh`.
    pub mutations_force_refresh: Option<i64>,
    /// `gamemode-scheduler.maxToSchedule`: mode and count.
    pub scheduler_max: Vec<(String, u64)>,
    /// `gamemode-scheduler.cadenceInSec`: mode and seconds.
    pub scheduler_cadence_seconds: Vec<(String, u64)>,
    /// `gamemode-activity-type-penalty-overrides`: activity type and seconds.
    pub activity_penalty_seconds: Vec<(String, u64)>,
    /// `game-mode-matchmaking-service-enabled-modes`.
    pub matchmaking_modes: Vec<String>,
}

/// Every gameplay key of the layers that the server applies or keeps for a system.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameplayConfig {
    /// `javelin.xp-multipliers`.
    pub xp: XpMultipliers,
    /// `javelin.disabled-item-classes`: an item with one of these in its `ItemClass` list is
    /// disabled (client check `FUN_14443ea80`).
    pub disabled_item_classes: Vec<String>,
    /// `javelin.wallet-cap-player`.
    pub wallet_cap: Option<u64>,
    /// `javelin.gear-set-storage-max-num-paid-loadouts`.
    pub paid_loadouts: Option<u64>,
    /// `javelin.territory-entry-requirement-enabled`.
    pub territory_entry_requirement: Option<bool>,
    /// `javelin.storage-search.*`.
    pub storage_search: StorageSearch,
    /// `javelin.housing.house-level-reqs`: the character level for each house, in order.
    pub house_level_reqs: Vec<u32>,
    /// `javelin.transmog-own-all-appearances`.
    pub transmog_own_all: Option<bool>,
    /// `javelin.dynamic-datatable-config`: the datasheet cell overrides, applied to the
    /// content tables at load ([`crate::datatable_overrides::apply`]).
    pub datatable_overrides: Vec<DatatableOverride>,
    /// The red button lists.
    pub red_button: RedButton,
    /// `javelin.schedule-overrides`.
    pub schedule_overrides: Vec<ScheduleOverride>,
    /// The season end times.
    pub seasons: Seasons,
    /// The game mode keys.
    pub game_modes: GameModes,
    /// `javelin.social.enable-company-only-war-slots`.
    pub company_only_war_slots: Option<bool>,
    /// `javelin-server.enable-twitch-system`.
    pub twitch_system: Option<bool>,
}

impl GameplayConfig {
    /// The gameplay keys of `stack`.
    pub fn from_stack(stack: &KeyStack) -> Result<Self, GameplayConfigError> {
        let number = |text: &str| text.parse::<f64>().ok();
        let whole = |text: &str| text.parse::<u64>().ok();
        let progression = entries(stack, keys::XP_PROGRESSION, number)?
            .into_iter()
            .map(|(id, multiplier)| (id.to_lowercase(), multiplier))
            .collect();
        let house_level_reqs = stack
            .texts(keys::HOUSE_LEVEL_REQS)?
            .into_iter()
            .map(|level| {
                level
                    .trim()
                    .parse()
                    .map_err(|_| GameplayConfigError::BadEntry {
                        key: keys::HOUSE_LEVEL_REQS,
                        entry: level.clone(),
                        reason: "is not a level",
                    })
            })
            .collect::<Result<_, _>>()?;
        let schedule_overrides = stack
            .texts(keys::SCHEDULE_OVERRIDES)?
            .into_iter()
            .map(|entry| {
                let parts: Vec<&str> = entry.split(SCHEDULE_SEPARATOR).map(str::trim).collect();
                match parts.as_slice() {
                    [id, start, end] => Ok(ScheduleOverride {
                        id: (*id).to_owned(),
                        start: (*start).to_owned(),
                        end: (*end).to_owned(),
                    }),
                    _ => Err(GameplayConfigError::BadEntry {
                        key: keys::SCHEDULE_OVERRIDES,
                        entry: entry.clone(),
                        reason: "is not <id>;<start>;<end>",
                    }),
                }
            })
            .collect::<Result<_, _>>()?;
        let datatable_overrides = stack
            .texts(keys::DYNAMIC_DATATABLE_CONFIG)?
            .iter()
            .map(|line| DatatableOverride::parse(line))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            xp: XpMultipliers {
                character: stack
                    .number(keys::XP_CHARACTER)?
                    .unwrap_or(NEUTRAL_MULTIPLIER),
                progression,
            },
            disabled_item_classes: stack.texts(keys::DISABLED_ITEM_CLASSES)?,
            wallet_cap: stack.unsigned(keys::WALLET_CAP_PLAYER)?,
            paid_loadouts: stack.unsigned(keys::PAID_LOADOUTS)?,
            territory_entry_requirement: stack.bool(keys::TERRITORY_ENTRY_REQUIREMENT)?,
            storage_search: StorageSearch {
                enabled: stack.bool(keys::STORAGE_SEARCH)?,
                housing_enabled: stack.bool(keys::STORAGE_SEARCH_HOUSING)?,
                crafting_enabled: stack.bool(keys::STORAGE_SEARCH_CRAFTING)?,
            },
            house_level_reqs,
            transmog_own_all: stack.bool(keys::TRANSMOG_OWN_ALL)?,
            datatable_overrides,
            red_button: RedButton {
                disabled_spawn_slices: stack.texts(keys::RED_BUTTON_SPAWN_SLICES)?,
                achievement_corrections: stack.texts(keys::RED_BUTTON_ACHIEVEMENT_CORRECTION)?,
                npa_overrides: stack.texts(keys::RED_BUTTON_NPA_OVERRIDE)?,
                traffic_mitigation_disabled: stack.bool(keys::RED_BUTTON_TRAFFIC_MITIGATION)?,
            },
            schedule_overrides,
            seasons: Seasons {
                season_end: stack.text(keys::SEASON_END)?,
                leaderboard_season_end: stack.text(keys::LEADERBOARD_SEASON_END)?,
            },
            game_modes: GameModes {
                dungeon_mutated_max_runs: stack.unsigned(keys::DUNGEON_MUTATED_MAX_RUNS)?,
                group_trial_max_refresh_seconds: stack.unsigned(keys::GROUP_TRIAL_MAX_REFRESH)?,
                mutations_seed_modifier: stack.signed(keys::MUTATIONS_SEED_MODIFIER)?,
                mutations_force_refresh: stack.signed(keys::MUTATIONS_FORCE_REFRESH)?,
                scheduler_max: entries(stack, keys::SCHEDULER_MAX, whole)?,
                scheduler_cadence_seconds: entries(stack, keys::SCHEDULER_CADENCE, whole)?,
                activity_penalty_seconds: entries(stack, keys::ACTIVITY_PENALTIES, whole)?,
                matchmaking_modes: stack.texts(keys::MATCHMAKING_MODES)?,
            },
            company_only_war_slots: stack.bool(keys::COMPANY_ONLY_WAR_SLOTS)?,
            twitch_system: stack.bool(keys::TWITCH_SYSTEM)?,
        })
    }

    /// Whether an item of `item_class` (an `ItemClass` cell, `+`-separated, for example
    /// `Weapon+EquippableMainHand+Dagger+Melee`) is disabled: one of its classes is in
    /// [`GameplayConfig::disabled_item_classes`] (any case).
    pub fn is_class_disabled(&self, item_class: &str) -> bool {
        item_class.split(ITEM_CLASS_SEPARATOR).any(|class| {
            self.disabled_item_classes
                .iter()
                .any(|disabled| disabled.eq_ignore_ascii_case(class.trim()))
        })
    }
}

/// Separates the classes of an `ItemClass` cell.
pub const ITEM_CLASS_SEPARATOR: char = '+';

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn layer(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(object) => object,
            _ => Map::new(),
        }
    }

    fn config(layers: &[Value]) -> Result<GameplayConfig, GameplayConfigError> {
        let layers: Vec<Map<String, Value>> = layers.iter().cloned().map(layer).collect();
        GameplayConfig::from_stack(&KeyStack::merge(&layers))
    }

    #[test]
    fn a_later_layer_replaces_a_key_as_a_whole() {
        let product = json!({"javelin": {"xp-multipliers": {
            "character": 2,
            "progression": ["Music:2", "Mining:2"]
        }}});
        let cognito = json!({"javelin": {"xp-multipliers": {"progression": ["Musician:2"]}}});
        let config = config(&[product, cognito]).expect("config");
        assert!((config.xp.character - 2.0).abs() < f64::EPSILON);
        assert!((config.xp.progression("musician") - 2.0).abs() < f64::EPSILON);
        assert!(
            (config.xp.progression("Music") - NEUTRAL_MULTIPLIER).abs() < f64::EPSILON,
            "the array of the later layer replaces the whole list"
        );
        assert!((config.xp.progression("Mining") - NEUTRAL_MULTIPLIER).abs() < f64::EPSILON);
    }

    #[test]
    fn nested_and_flat_keys_are_the_same_key() {
        let flat = json!({"javelin": {"storage-search.enabled": true}});
        let nested = json!({"javelin": {"storage-search": {"housing-enabled": false}}});
        let config = config(&[flat, nested]).expect("config");
        assert_eq!(
            config.storage_search,
            StorageSearch {
                enabled: Some(true),
                housing_enabled: Some(false),
                crafting_enabled: None,
            }
        );
    }

    #[test]
    fn the_character_multiplier_scales_every_gain() {
        let neutral = XpMultipliers::default();
        assert_eq!(neutral.character_gain(10), 10);
        let retail = XpMultipliers {
            character: 2.0,
            progression: HashMap::new(),
        };
        assert_eq!(retail.character_gain(10), 20, "the retail kill");
        assert_eq!(retail.character_gain(u32::MAX), u32::MAX);
        let half = XpMultipliers {
            character: 0.5,
            progression: HashMap::new(),
        };
        assert_eq!(half.character_gain(5), 3, "rounds to the nearest point");
    }

    #[test]
    fn every_listed_key_is_read() {
        let gameplay = json!({
            "javelin": {
                "disabled-item-classes": ["Dagger"],
                "red-button-disable-spawn-slice-list": ["enc_winterconv_activity_gleamite_00"],
                "social": {"enable-company-only-war-slots": false},
                "gamemode-dungeon-mutated-max-runs": 100,
                "gamemode-group-trial-max-refresh-seconds": 86400,
                "gamemode-mutations": {"seedModifier": 23, "forceRefresh": 10},
                "gamemode-scheduler": {"maxToSchedule": ["Trial:3"], "cadenceInSec": ["Trial:21600"]},
                "territory-entry-requirement-enabled": false,
                "housing": {"house-level-reqs": ["15", "35", "55"]},
                "wallet-cap-player": 1_000_000_000_u64,
                "seasons-rewards.season-end-datetime": "2027-01-31T00:00:00",
                "leaderboard-season-end-datetime": "2027-01-31T00:00:00",
                "red-button-catch-all-achievement-correction": ["!A:B"],
                "storage-search.crafting-enabled": true,
                "gamemode-activity-type-penalty-overrides": ["Dungeon:1200", "Catacombs:300"],
                "gear-set-storage-max-num-paid-loadouts": 27,
                "transmog-own-all-appearances": true,
                "dynamic-datatable-config": ["JavelinData_AfkModes, AfkModeData, Default, WarnSeconds:82800"],
                "red-button-npa-override": ["*SwapActive_18034_Morgaine: ReqAchievementConditional=!debug_1"],
                "schedule-overrides": ["FireNaga2;2026-06-30-T17:00:0;2026-07-07T17:00:00"],
                "game-mode-matchmaking-service-enabled-modes": ["OutpostRush", "Dungeon"],
                "red-button-disable-traffic-mitigation": false
            },
            "javelin-server": {"enable-twitch-system": false}
        });
        let config = config(&[gameplay]).expect("config");
        assert!(config.is_class_disabled("Weapon+EquippableMainHand+Dagger+Melee"));
        assert!(config.is_class_disabled("dagger"));
        assert!(!config.is_class_disabled("Weapon+EquippableMainHand+Sword+Melee"));
        assert_eq!(config.wallet_cap, Some(1_000_000_000));
        assert_eq!(config.paid_loadouts, Some(27));
        assert_eq!(config.territory_entry_requirement, Some(false));
        assert_eq!(config.storage_search.crafting_enabled, Some(true));
        assert_eq!(config.house_level_reqs, [15, 35, 55]);
        assert_eq!(config.transmog_own_all, Some(true));
        assert_eq!(config.datatable_overrides.len(), 1);
        assert_eq!(
            config.red_button,
            RedButton {
                disabled_spawn_slices: vec!["enc_winterconv_activity_gleamite_00".to_owned()],
                achievement_corrections: vec!["!A:B".to_owned()],
                npa_overrides: vec![
                    "*SwapActive_18034_Morgaine: ReqAchievementConditional=!debug_1".to_owned()
                ],
                traffic_mitigation_disabled: Some(false),
            }
        );
        assert_eq!(
            config.schedule_overrides,
            [ScheduleOverride {
                id: "FireNaga2".to_owned(),
                start: "2026-06-30-T17:00:0".to_owned(),
                end: "2026-07-07T17:00:00".to_owned(),
            }],
            "a malformed retail time is kept as written"
        );
        assert_eq!(
            config.seasons.season_end.as_deref(),
            Some("2027-01-31T00:00:00")
        );
        assert_eq!(
            config.seasons.leaderboard_season_end.as_deref(),
            Some("2027-01-31T00:00:00")
        );
        assert_eq!(
            config.game_modes,
            GameModes {
                dungeon_mutated_max_runs: Some(100),
                group_trial_max_refresh_seconds: Some(86_400),
                mutations_seed_modifier: Some(23),
                mutations_force_refresh: Some(10),
                scheduler_max: vec![("Trial".to_owned(), 3)],
                scheduler_cadence_seconds: vec![("Trial".to_owned(), 21_600)],
                activity_penalty_seconds: vec![
                    ("Dungeon".to_owned(), 1_200),
                    ("Catacombs".to_owned(), 300)
                ],
                matchmaking_modes: vec!["OutpostRush".to_owned(), "Dungeon".to_owned()],
            }
        );
        assert_eq!(config.company_only_war_slots, Some(false));
        assert_eq!(config.twitch_system, Some(false));
    }

    #[test]
    fn a_key_no_layer_sets_keeps_the_table_value() {
        let config = config(&[json!({})]).expect("config");
        assert_eq!(config, GameplayConfig::default());
        assert!((config.xp.character - NEUTRAL_MULTIPLIER).abs() < f64::EPSILON);
        assert!(!config.is_class_disabled("Dagger"));
    }

    #[test]
    fn a_value_of_another_type_is_refused() {
        let refused = |value: Value| config(&[value]).expect_err("refused").to_string();
        assert!(
            refused(json!({"javelin": {"wallet-cap-player": "many"}}))
                .contains(keys::WALLET_CAP_PLAYER)
        );
        assert!(
            refused(json!({"javelin": {"xp-multipliers": {"progression": ["Mining"]}}}))
                .contains("has no ':'")
        );
        assert!(
            refused(json!({"javelin": {"xp-multipliers": {"progression": ["Mining:fast"]}}}))
                .contains("another type")
        );
        assert!(
            refused(json!({"javelin": {"housing": {"house-level-reqs": ["high"]}}}))
                .contains("is not a level")
        );
        assert!(
            refused(json!({"javelin": {"schedule-overrides": ["Mammoth;2026-02-17T17:00:00"]}}))
                .contains("<id>;<start>;<end>")
        );
        assert!(
            refused(json!({"javelin": {"dynamic-datatable-config": ["JavelinData_AfkModes"]}}))
                .contains("at least 4 entries")
        );
    }
}
