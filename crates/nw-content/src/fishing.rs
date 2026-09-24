// SPDX-License-Identifier: AGPL-3.0-or-later
//! The fishing rules of the game data (run 15 fishing study): the poles
//! (`javelindata_fishing_poles`, found through an item's `ItemStatsRef`), the baits, the
//! catchables, the fish behaviors, the water types, the hotspots, the cast distance bonus of the
//! `Fishing` tradeskill ranks (`TradeskillRankData` `Fishing`), the territories' loot tags, and
//! the global fishing values of `playerbaseattributes.pbadb` ([`FISHING_DATA`]).
//!
//! The water at a place is fishable when its water flag bit 1 is set; bit 3 makes it salt water,
//! bit 2 fresh water, and its depth gives the depth class (`FUN_1430e8590`, `FUN_1469f93d0`:
//! Deep from 1.8 m, Shallow from 0.9 m, Very Shallow from 0.2 m). The level water map's sea is
//! `0x0a` (salt), lakes and rivers `0x07` (fresh), ponds `0x04` (not fishable).
//!
//! A hotspot is a spawned slice `fishing_hotspot_<type>_<n>`: its `FishingHotspotComponent`
//! names the hotspot row `Hotspot_<Type>_<n>` (Observed on `fishing_hotspot_secret_64` and
//! `fishing_hotspot_broad_4`), and its physics shape on the `FishingHotspot` layer has a radius
//! of 2 m ([`HOTSPOT_RADIUS_M`]).

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The categorical progression of fishing.
pub const FISHING_TRADESKILL: &str = "Fishing";
/// The item class of a fishing pole (`EquippableTool+FishingPole`).
const FISHING_POLE_CLASS: &str = "FishingPole";
/// Separates the classes of an `ItemClass` cell.
const CLASS_SEPARATOR: char = '+';
/// Separates the ends of a range cell (`5-12`, `1-1.5`).
const RANGE_SEPARATOR: char = '-';
/// Separates the names of a list cell (`Lazy,Sporadic`, `Fresh,Salt`).
const LIST_SEPARATOR: char = ',';
/// The water tags of a bait's `RequiredWaterTags` and of the loot tables' conditions.
pub const FRESH_TAG: &str = "Fresh";
/// See [`FRESH_TAG`].
pub const SALT_TAG: &str = "Salt";
/// The file stem start of a hotspot slice; the rest after `fishing_` is the hotspot row id.
const HOTSPOT_SLICE_START: &str = "fishing_hotspot_";
/// What a hotspot slice's stem has before its hotspot row id.
const HOTSPOT_SLICE_PREFIX: &str = "fishing_";
/// The extension of a slice file.
const SLICE_EXTENSION: &str = ".dynamicslice";
/// The radius of a hotspot's physics shape (`m_radius` of the hotspot slices).
pub const HOTSPOT_RADIUS_M: f32 = 2.0;
/// The water flag bits (`FUN_1430e8590`): fishable, fresh, salt.
const FISHABLE_WATER: u32 = 1 << 1;
const FRESH_WATER: u32 = 1 << 2;
const SALT_WATER: u32 = 1 << 3;
/// The depths of the depth classes (`FUN_1469f93d0`), m.
const DEEP_M: f32 = 1.8;
const SHALLOW_M: f32 = 0.9;
const VERY_SHALLOW_M: f32 = 0.2;
/// Characters of a `FishBehaviorPattern`: one per time block.
pub const BEHAVIOR_BLOCKS: usize = 8;
/// The pattern character of a fighting block.
const FIGHTING_CHARACTER: char = '1';

/// The global fishing values (`FishingData` of `sharedassets/genericassets/
/// playerbaseattributes.pbadb`, build 25810808, class registered at `0x145cb04e0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FishingData {
    /// `Fish Catch Game Event`.
    pub catch_event: &'static str,
    /// `Fish Hook Game Event`.
    pub hook_event: &'static str,
    /// `Bait Loss Chance Cast Hit Land`.
    pub bait_loss_cast_hit_land: f64,
    /// `Bait Loss Chance Hook Miss`.
    pub bait_loss_hook_miss: f64,
    /// `Bait Loss Chance Reeling Miss Distance`.
    pub bait_loss_reeling_distance: f64,
    /// `Bait Loss Chance Reeling Miss Tension`.
    pub bait_loss_reeling_tension: f64,
    /// `Bait Loss Chance Fish Caught`.
    pub bait_loss_fish_caught: f64,
    /// `Bite Window Open Duration Seconds`.
    pub bite_window_s: f64,
    /// `Fish Behavior Time Block Duration Seconds`.
    pub behavior_block_s: f64,
    /// `Fish Fighting Line Tension Multiplier`, `Reel In Multiplier`, `Swim Away Multiplier`.
    pub fighting: FishStateMultipliers,
    /// `Fish Tired Line Tension Multiplier`, `Reel In Multiplier`, `Swim Away Multiplier`.
    pub tired: FishStateMultipliers,
}

/// The multipliers of one fish state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FishStateMultipliers {
    /// Of the line tension gain.
    pub tension: f64,
    /// Of the reel-in speed.
    pub reel: f64,
    /// Of the swim-away speed.
    pub swim: f64,
}

/// The values of build 25810808.
pub const FISHING_DATA: FishingData = FishingData {
    catch_event: "FishCaught",
    hook_event: "FishHooked",
    bait_loss_cast_hit_land: 0.0,
    bait_loss_hook_miss: 0.5,
    bait_loss_reeling_distance: 1.0,
    bait_loss_reeling_tension: 1.0,
    bait_loss_fish_caught: 0.75,
    bite_window_s: 1.0,
    behavior_block_s: 2.0,
    fighting: FishStateMultipliers {
        tension: 2.5,
        reel: 0.75,
        swim: 2.5,
    },
    tired: FishStateMultipliers {
        tension: 1.0,
        reel: 1.0,
        swim: 1.0,
    },
};

/// A range cell: `a-b`, or `a` for both ends.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Range {
    /// The low end.
    pub low: f64,
    /// The high end.
    pub high: f64,
}

impl Range {
    /// The range of `cell`; `None` for a cell that is not one.
    pub fn of(cell: &str) -> Option<Self> {
        let text = cell.trim();
        let (low, high) = match text.split_once(RANGE_SEPARATOR) {
            Some((low, high)) => (low.trim().parse().ok()?, high.trim().parse().ok()?),
            None => {
                let value = text.parse().ok()?;
                (value, value)
            }
        };
        Some(Self {
            low: f64::min(low, high),
            high: f64::max(low, high),
        })
    }

    /// The value at `fraction` (0 to 1) of the way from the low end to the high end.
    pub fn at(self, fraction: f64) -> f64 {
        self.low + (self.high - self.low) * fraction.clamp(0.0, 1.0)
    }
}

/// The client enum `FishingWaterType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WaterType {
    /// 1.
    FreshVeryShallow = 1,
    /// 2.
    FreshShallow = 2,
    /// 3.
    FreshDeep = 3,
    /// 4.
    SaltVeryShallow = 4,
    /// 5.
    SaltShallow = 5,
    /// 6.
    SaltDeep = 6,
}

impl WaterType {
    /// The water type of water with flags `flags` and depth `depth_m`; `None` where nobody can
    /// fish (no fishable flag, neither fresh nor salt, or too shallow).
    pub fn of(flags: u32, depth_m: f32) -> Option<Self> {
        if flags & FISHABLE_WATER == 0 {
            return None;
        }
        // The client takes the lowest of the two kind bits that is set.
        let salt = if flags & FRESH_WATER != 0 {
            false
        } else if flags & SALT_WATER != 0 {
            true
        } else {
            return None;
        };
        let class = if depth_m >= DEEP_M {
            3
        } else if depth_m >= SHALLOW_M {
            2
        } else if depth_m >= VERY_SHALLOW_M {
            1
        } else {
            return None;
        };
        Self::of_class(salt, class)
    }

    fn of_class(salt: bool, class: u8) -> Option<Self> {
        Some(match (salt, class) {
            (false, 1) => Self::FreshVeryShallow,
            (false, 2) => Self::FreshShallow,
            (false, 3) => Self::FreshDeep,
            (true, 1) => Self::SaltVeryShallow,
            (true, 2) => Self::SaltShallow,
            (true, 3) => Self::SaltDeep,
            _ => return None,
        })
    }

    /// Whether it is salt water.
    pub const fn is_salt(self) -> bool {
        matches!(self, Self::SaltVeryShallow | Self::SaltShallow | Self::SaltDeep)
    }

    /// Whether it is deep: nobody may cast from it (`CastStartAction`: "can't cast from here").
    pub const fn is_deep(self) -> bool {
        matches!(self, Self::FreshDeep | Self::SaltDeep)
    }

    /// The water's tag ([`FRESH_TAG`] or [`SALT_TAG`]).
    pub const fn tag(self) -> &'static str {
        if self.is_salt() { SALT_TAG } else { FRESH_TAG }
    }

    /// Its `FishingWaterData` id.
    pub const fn id(self) -> &'static str {
        match self {
            Self::FreshVeryShallow => "FreshWater_VeryShallow",
            Self::FreshShallow => "FreshWater_Shallow",
            Self::FreshDeep => "FreshWater_Deep",
            Self::SaltVeryShallow => "SaltWater_VeryShallow",
            Self::SaltShallow => "SaltWater_Shallow",
            Self::SaltDeep => "SaltWater_Deep",
        }
    }

    const ALL: [Self; 6] = [
        Self::FreshVeryShallow,
        Self::FreshShallow,
        Self::FreshDeep,
        Self::SaltVeryShallow,
        Self::SaltShallow,
        Self::SaltDeep,
    ];
}

/// The day phases of the bait columns (`SyncDayPhase`'s phase: the catch log names 0 `Night`,
/// 1 `Dawn`, 2 `Day`, 3 `Dusk`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayPhase {
    /// 0.
    Night,
    /// 1.
    Dawn,
    /// 2.
    Day,
    /// 3.
    Dusk,
}

impl DayPhase {
    /// The phase of `SyncDayPhase` value `value`; `None` for another value.
    pub const fn of(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Night),
            1 => Some(Self::Dawn),
            2 => Some(Self::Day),
            3 => Some(Self::Dusk),
            _ => None,
        }
    }
}

/// The roll modifiers a pole, a bait or a hotspot gives the catch roll.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RollModifiers {
    /// `FishRarityRollModifier`.
    pub rarity: f64,
    /// `FishSizeRollModifier`.
    pub size: f64,
    /// `SummerFishRarityRollModifier`.
    pub summer_rarity: f64,
}

impl RollModifiers {
    /// The sum of `self` and `other`.
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            rarity: self.rarity + other.rarity,
            size: self.size + other.size,
            summer_rarity: self.summer_rarity + other.summer_rarity,
        }
    }
}

/// One fishing pole (`FishingPolesData`).
#[derive(Debug, Clone, PartialEq)]
pub struct Pole {
    /// `Id`.
    pub id: String,
    /// `CastDistanceRange`, m.
    pub cast_distance: Range,
    /// The roll modifiers.
    pub modifiers: RollModifiers,
    /// `FishReelTimeSeconds`: a tired fish comes in this many seconds of reeling.
    pub reel_time_s: f64,
    /// `LineTensionGainPerSecond`.
    pub tension_gain: f64,
    /// `LineTensionRecoverPerSecond`.
    pub tension_recover: f64,
}

/// One bait (`FishingBaitData`).
#[derive(Debug, Clone, PartialEq)]
pub struct Bait {
    /// `Id`: the bait item's id.
    pub id: String,
    /// The CRC-32 of the lowercase id: its id in 2309 and 557.
    pub crc: u32,
    /// The roll modifiers.
    pub modifiers: RollModifiers,
    /// `RequiredWaterTags`.
    pub water_tags: Vec<String>,
    /// `ActiveAtNight`, `ActiveAtDawn`, `ActiveAtDaytime`, `ActiveAtDusk`.
    active: [bool; 4],
    /// `LootTableId`: the loot table it fishes from, when set.
    pub loot_table: Option<String>,
    /// `TimeToNibbleSecondsMultiplier`.
    pub nibble_multiplier: f64,
}

impl Bait {
    /// Whether it works in water `water` at day phase `phase`.
    pub fn works(&self, water: WaterType, phase: DayPhase) -> bool {
        let phase_index = match phase {
            DayPhase::Night => 0,
            DayPhase::Dawn => 1,
            DayPhase::Day => 2,
            DayPhase::Dusk => 3,
        };
        self.active[phase_index]
            && self
                .water_tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case(water.tag()))
    }
}

/// One catchable (`FishingCatchablesData`): an item a hook may bring in.
#[derive(Debug, Clone, PartialEq)]
pub struct Catchable {
    /// `Id`: the item id.
    pub id: String,
    /// `FishWeightRange`.
    pub weight: Range,
    /// `FishLengthRange`.
    pub length: Range,
    /// `FishingXp`.
    pub fishing_xp: u32,
    /// `FishDistanceSwamAwayPerSecond`, m/s.
    pub swim_away_mps: f64,
    /// `LineTensionGainPerSecondMultiplier`.
    pub tension_multiplier: f64,
    /// `FishBehaviors`: the behavior ids.
    pub behaviors: Vec<String>,
    /// `RewardGameEventId`.
    pub reward_event: Option<String>,
}

/// One fish behavior (`FishingBehaviorsData`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Behavior {
    /// `Id`.
    pub id: String,
    /// The CRC-32 of the lowercase id: its id in 3172.
    pub crc: u32,
    /// `FishBehaviorPattern`: bit `i` is time block `i`, set when the fish fights.
    pub bits: u8,
}

impl Behavior {
    /// Whether the fish fights in time block `block` (from 0); after the pattern it is tired
    /// (Unknown: no retail fight lasted past the pattern's 8 blocks).
    pub const fn fighting(&self, block: u64) -> bool {
        block < BEHAVIOR_BLOCKS as u64 && self.bits >> block & 1 == 1
    }
}

/// The time ranges of a water type (`FishingWaterData`) or a hotspot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaitRanges {
    /// `TimeToNibbleSecondsRange`.
    pub nibble_s: Range,
    /// `TimeToBiteSecondsRange`.
    pub bite_s: Range,
}

/// One hotspot (`FishingHotspotsData`).
#[derive(Debug, Clone, PartialEq)]
pub struct Hotspot {
    /// `Id`.
    pub id: String,
    /// The CRC-32 of the lowercase id: its id in 2444.
    pub crc: u32,
    /// The roll modifiers.
    pub modifiers: RollModifiers,
    /// `LootTableId`.
    pub loot_table: String,
    /// `LootTag`: a tag its loot roll holds.
    pub loot_tag: String,
    /// `RequiredLevel`: the lowest `Fishing` rank (zero-based) that fishes it.
    pub required_rank: u16,
    /// Its wait ranges, in place of the water's.
    pub waits: WaitRanges,
}

/// A hotspot placed in the level.
#[derive(Debug, Clone, PartialEq)]
struct HotspotPlace {
    /// The lowercase hotspot id.
    hotspot: String,
    /// Where it is, m.
    position: [f32; 3],
}

/// The fishing rules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FishingRules {
    /// The poles, by the lowercase id of each item that uses them.
    poles: HashMap<String, Pole>,
    /// The baits, by CRC-32.
    baits: HashMap<u32, Bait>,
    /// The catchables, by lowercase item id.
    catchables: HashMap<String, Catchable>,
    /// The behaviors, by lowercase id.
    behaviors: HashMap<String, Behavior>,
    /// The wait ranges of each water type.
    waters: HashMap<WaterType, WaitRanges>,
    /// The hotspots, by lowercase id.
    hotspots: HashMap<String, Hotspot>,
    /// The hotspots of the level.
    places: Vec<HotspotPlace>,
    /// The `FishingMaxCastDistanceBonus` of each `Fishing` rank, rank 0 first.
    cast_bonus: Vec<f64>,
    /// The `LootTags` of each territory.
    territory_tags: HashMap<u32, Vec<String>>,
}

/// The names of a list cell, trimmed, without empty ones.
fn names(cell: &str) -> Vec<String> {
    cell.split(LIST_SEPARATOR)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The bits of a `FishBehaviorPattern` cell: character `i` is bit `i`.
fn pattern_bits(pattern: &str) -> u8 {
    pattern
        .chars()
        .take(BEHAVIOR_BLOCKS)
        .enumerate()
        .filter(|(_, character)| *character == FIGHTING_CHARACTER)
        .fold(0, |bits, (block, _)| bits | 1 << block)
}

/// A whole non-negative number of a data cell as `u32`.
fn whole(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

impl FishingRules {
    /// The fishing rules of `tables`; no hotspot is placed ([`Self::with_hotspots`]).
    pub fn from_tables(tables: &Tables) -> Self {
        let poles_by_id: HashMap<String, Pole> = tables
            .fishing_poles_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    row.id.to_lowercase(),
                    Pole {
                        id: row.id.clone(),
                        cast_distance: Range::of(&row.cast_distance_range)?,
                        modifiers: RollModifiers {
                            rarity: row.fish_rarity_roll_modifier,
                            // Whole numbers in the data.
                            size: row.fish_size_roll_modifier as f64,
                            summer_rarity: row.summer_fish_rarity_roll_modifier,
                        },
                        reel_time_s: row.fish_reel_time_seconds as f64,
                        tension_gain: row.line_tension_gain_per_second,
                        tension_recover: row.line_tension_recover_per_second,
                    },
                ))
            })
            .collect();
        let poles = tables
            .master_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter(|row| {
                row.item_class.as_deref().is_some_and(|classes| {
                    classes
                        .split(CLASS_SEPARATOR)
                        .any(|class| class.trim().eq_ignore_ascii_case(FISHING_POLE_CLASS))
                })
            })
            .filter_map(|row| {
                let stats = row.item_stats_ref.as_deref().unwrap_or(&row.item_id);
                let pole = poles_by_id.get(&stats.to_lowercase())?;
                Some((row.item_id.to_lowercase(), pole.clone()))
            })
            .collect();
        let baits = tables
            .fishing_bait_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                let crc = lowercase_crc32(&row.id);
                (
                    crc,
                    Bait {
                        id: row.id.clone(),
                        crc,
                        modifiers: RollModifiers {
                            rarity: row.fish_rarity_roll_modifier,
                            size: row.fish_size_roll_modifier,
                            // Whole numbers in the data.
                            summer_rarity: row.summer_fish_rarity_roll_modifier as f64,
                        },
                        water_tags: names(&row.required_water_tags),
                        active: [
                            row.active_at_night,
                            row.active_at_dawn,
                            row.active_at_daytime,
                            row.active_at_dusk,
                        ],
                        loot_table: row.loot_table_id.clone().filter(|table| !table.is_empty()),
                        nibble_multiplier: row.time_to_nibble_seconds_multiplier,
                    },
                )
            })
            .collect();
        let catchables = tables
            .fishing_catchables_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    row.id.to_lowercase(),
                    Catchable {
                        id: row.id.clone(),
                        weight: Range::of(&row.fish_weight_range)?,
                        length: Range::of(&row.fish_length_range)?,
                        fishing_xp: whole(row.fishing_xp),
                        swim_away_mps: row.fish_distance_swam_away_per_second,
                        tension_multiplier: row.line_tension_gain_per_second_multiplier,
                        behaviors: names(&row.fish_behaviors),
                        reward_event: row
                            .reward_game_event_id
                            .clone()
                            .filter(|event| !event.is_empty()),
                    },
                ))
            })
            .collect();
        let behaviors = tables
            .fishing_behaviors_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                (
                    row.id.to_lowercase(),
                    Behavior {
                        id: row.id.clone(),
                        crc: lowercase_crc32(&row.id),
                        bits: pattern_bits(&format!("{:08}", row.fish_behavior_pattern)),
                    },
                )
            })
            .collect();
        let waters = tables
            .fishing_water_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let water = WaterType::ALL
                    .into_iter()
                    .find(|water| water.id().eq_ignore_ascii_case(&row.id))?;
                Some((
                    water,
                    WaitRanges {
                        nibble_s: Range::of(&row.time_to_nibble_seconds_range)?,
                        bite_s: Range::of(&row.time_to_bite_seconds_range)?,
                    },
                ))
            })
            .collect();
        let hotspots = tables
            .fishing_hotspots_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    row.id.to_lowercase(),
                    Hotspot {
                        id: row.id.clone(),
                        crc: lowercase_crc32(&row.id),
                        modifiers: RollModifiers {
                            rarity: row.fish_rarity_roll_modifier,
                            // Whole numbers in the data.
                            size: row.fish_size_roll_modifier as f64,
                            summer_rarity: row.summer_fish_rarity_roll_modifier as f64,
                        },
                        loot_table: row.loot_table_id.clone(),
                        loot_tag: row.loot_tag.clone(),
                        required_rank: u16::try_from(row.required_level.max(0))
                            .unwrap_or(u16::MAX),
                        waits: WaitRanges {
                            nibble_s: Range::of(&row.time_to_nibble_seconds_range)?,
                            bite_s: Range::of(&row.time_to_bite_seconds_range)?,
                        },
                    },
                ))
            })
            .collect();
        let mut ranks: Vec<(i64, f64)> = tables
            .tradeskill_rank_data
            .iter()
            .filter(|table| table.unique_id.eq_ignore_ascii_case(FISHING_TRADESKILL))
            .flat_map(|table| &table.rows)
            .map(|row| (row.level, row.fishing_max_cast_distance_bonus.unwrap_or_default()))
            .collect();
        ranks.sort_by_key(|(rank, _)| *rank);
        let territory_tags = tables
            .territory_definition
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let tags = names(row.loot_tags.as_deref()?);
                Some((u32::try_from(row.territory_id).ok()?, tags))
            })
            .filter(|(_, tags)| !tags.is_empty())
            .collect();
        Self {
            poles,
            baits,
            catchables,
            behaviors,
            waters,
            hotspots,
            places: Vec::new(),
            cast_bonus: ranks.into_iter().map(|(_, bonus)| bonus).collect(),
            territory_tags,
        }
    }

    /// These rules with the hotspots among the level's spawned `slices` (slice path, place):
    /// those whose slice is a hotspot slice of a known hotspot.
    #[must_use]
    pub fn with_hotspots<'a>(
        mut self,
        slices: impl IntoIterator<Item = (&'a str, [f32; 3])>,
    ) -> Self {
        self.places = slices
            .into_iter()
            .filter_map(|(path, position)| {
                let file = path.rsplit(['/', '\\']).next()?.to_lowercase();
                let stem = file.strip_suffix(SLICE_EXTENSION).unwrap_or(&file);
                if !stem.starts_with(HOTSPOT_SLICE_START) {
                    return None;
                }
                let hotspot = stem.strip_prefix(HOTSPOT_SLICE_PREFIX)?.to_owned();
                self.hotspots.contains_key(&hotspot).then_some(HotspotPlace { hotspot, position })
            })
            .collect();
        self
    }

    /// The pole of the item `item_id`.
    pub fn pole(&self, item_id: &str) -> Option<&Pole> {
        self.poles.get(&item_id.to_lowercase())
    }

    /// The bait whose lowercase id has CRC-32 `crc`.
    pub fn bait(&self, crc: u32) -> Option<&Bait> {
        self.baits.get(&crc)
    }

    /// The bait of item `item_id` (a bait's id is its item's).
    pub fn bait_of_item(&self, item_id: &str) -> Option<&Bait> {
        self.bait(lowercase_crc32(item_id))
    }

    /// The catchable of item `item_id`.
    pub fn catchable(&self, item_id: &str) -> Option<&Catchable> {
        self.catchables.get(&item_id.to_lowercase())
    }

    /// The behavior `id`.
    pub fn behavior(&self, id: &str) -> Option<&Behavior> {
        self.behaviors.get(&id.to_lowercase())
    }

    /// The wait ranges of water `water`.
    pub fn waits(&self, water: WaterType) -> Option<WaitRanges> {
        self.waters.get(&water).copied()
    }

    /// The hotspot `id`.
    pub fn hotspot(&self, id: &str) -> Option<&Hotspot> {
        self.hotspots.get(&id.to_lowercase())
    }

    /// The nearest placed hotspot whose shape holds (`x`, `y`) and that a `Fishing` rank of
    /// `rank` fishes.
    pub fn hotspot_at(&self, x: f32, y: f32, rank: u16) -> Option<&Hotspot> {
        self.places
            .iter()
            .map(|place| {
                let [px, py, _] = place.position;
                ((px - x).hypot(py - y), place)
            })
            .filter(|(distance, _)| *distance <= HOTSPOT_RADIUS_M)
            .filter_map(|(distance, place)| {
                let hotspot = self.hotspots.get(&place.hotspot)?;
                (rank >= hotspot.required_rank).then_some((distance, hotspot))
            })
            .min_by(|(near, _), (far, _)| near.total_cmp(far))
            .map(|(_, hotspot)| hotspot)
    }

    /// The `FishingMaxCastDistanceBonus` of `Fishing` rank `rank` (zero-based), m; the top
    /// rank's above it, 0 without the table.
    pub fn cast_bonus(&self, rank: u16) -> f64 {
        self.cast_bonus
            .get(usize::from(rank))
            .or(self.cast_bonus.last())
            .copied()
            .unwrap_or_default()
    }

    /// The loot tags of the territories `territories` (their `LootTags`).
    pub fn territory_tags(&self, territories: &[u16]) -> Vec<String> {
        territories
            .iter()
            .filter_map(|territory| self.territory_tags.get(&u32::from(*territory)))
            .flatten()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{
        FishingBaitData, FishingBehaviorsData, FishingCatchablesData, FishingHotspotsData,
        FishingPolesData, FishingWaterData, MasterItemDefinitions, TradeskillRankData,
    };
    use crate::table::Table;

    fn table<T>(unique_id: &str, rows: Vec<T>) -> Vec<Table<T>> {
        vec![Table {
            source_path: String::new(),
            unique_id: unique_id.to_owned(),
            rows,
        }]
    }

    /// Rows of build 25810808: the T5 pole and the item that uses its stats, a bait, a medium
    /// salmon, two behaviors, the fresh shallow water, a broad hotspot and two fishing ranks.
    fn tables() -> Tables {
        let item = |value: serde_json::Value| -> MasterItemDefinitions {
            serde_json::from_value(value).expect("item row")
        };
        let pole_item = |item_id: &str, stats: &str| {
            item(serde_json::json!({
                "item_id": item_id,
                "item_class": "EquippableTool+FishingPole",
                "item_stats_ref": stats,
            }))
        };
        let rank = |level: i64, bonus: f64| -> TradeskillRankData {
            serde_json::from_value(serde_json::json!({
                "currency_cost": 0,
                "influence_cost": 0,
                "level": level,
                "maximum_influence": 50.0,
                "required_level": 0,
                "fishing_max_cast_distance_bonus": bonus,
            }))
            .expect("rank row")
        };
        Tables {
            fishing_poles_data: table(
                "FishingPolesData",
                vec![FishingPolesData {
                    id: "2hFishingPoleT5".to_owned(),
                    cast_distance_range: "5-20".to_owned(),
                    fish_rarity_roll_modifier: 1.25,
                    fish_size_roll_modifier: 0,
                    summer_fish_rarity_roll_modifier: 0.0,
                    fish_reel_time_seconds: 8,
                    line_tension_gain_per_second: 12.5,
                    line_tension_recover_per_second: 47.0,
                }],
            ),
            master_item_definitions: table(
                "MasterItemDefinitions_Common",
                vec![
                    pole_item("2hFishingPoleT5", "2hFishingPoleT5"),
                    pole_item("2hFishingPoleT52", "2hFishingPoleT5"),
                    item(serde_json::json!({"item_id": "FishingSalmonMediumT1"})),
                ],
            ),
            fishing_bait_data: table(
                "FishingBaitData",
                vec![FishingBaitData {
                    id: "BaitFreshFireflyT3".to_owned(),
                    fish_rarity_roll_modifier: 0.9,
                    fish_size_roll_modifier: 0.0,
                    summer_fish_rarity_roll_modifier: 0,
                    required_water_tags: "Fresh".to_owned(),
                    active_at_dawn: true,
                    active_at_daytime: true,
                    active_at_dusk: true,
                    active_at_night: false,
                    loot_table_id: None,
                    time_to_nibble_seconds_multiplier: 0.2,
                }],
            ),
            fishing_catchables_data: table(
                "FishingCatchablesData",
                vec![FishingCatchablesData {
                    id: "FishingSalmonMediumT1".to_owned(),
                    fish_size: "Medium".to_owned(),
                    fish_weight_range: "1-5".to_owned(),
                    fish_length_range: "9-15".to_owned(),
                    fishing_xp: 110,
                    fish_distance_swam_away_per_second: 0.15,
                    line_tension_gain_per_second_multiplier: 1.2,
                    fish_behaviors: "Lazy, SporadicEasy".to_owned(),
                    zoom_camera_on_success: false,
                    fish_character_definition_path: String::new(),
                    fish_scale: 0.7,
                    reward_game_event_id: None,
                }],
            ),
            fishing_behaviors_data: table(
                "FishingBehaviorsData",
                vec![
                    FishingBehaviorsData {
                        id: "Lazy".to_owned(),
                        fish_behavior_pattern: 0,
                        fish_behavior_localization_key: String::new(),
                    },
                    FishingBehaviorsData {
                        id: "SporadicEasy".to_owned(),
                        fish_behavior_pattern: 1_001_001,
                        fish_behavior_localization_key: String::new(),
                    },
                ],
            ),
            fishing_water_data: table(
                "FishingWaterData",
                vec![FishingWaterData {
                    id: "FreshWater_Shallow".to_owned(),
                    time_to_nibble_seconds_range: "10-20".to_owned(),
                    time_to_bite_seconds_range: "1-1.5".to_owned(),
                }],
            ),
            fishing_hotspots_data: table(
                "FishingHotspotsData",
                vec![FishingHotspotsData {
                    id: "Hotspot_Broad_4".to_owned(),
                    display_name: String::new(),
                    fish_rarity_roll_modifier: 7.66,
                    fish_size_roll_modifier: 0,
                    summer_fish_rarity_roll_modifier: 0,
                    loot_table_id: "FishingHotspotLoot".to_owned(),
                    loot_tag: "Broad".to_owned(),
                    notes: None,
                    required_level: 2,
                    time_to_bite_seconds_range: "1-1.5".to_owned(),
                    time_to_nibble_seconds_range: "3-6".to_owned(),
                }],
            ),
            tradeskill_rank_data: table(
                FISHING_TRADESKILL,
                vec![rank(1, 0.05), rank(0, 0.0)],
            ),
            ..Tables::default()
        }
    }

    #[test]
    fn an_item_finds_its_pole_through_its_stats_and_the_rows_read() {
        let rules = FishingRules::from_tables(&tables());
        let pole = rules.pole("2HFISHINGPOLET52").expect("the T52 uses the T5 stats");
        assert_eq!(pole.id, "2hFishingPoleT5");
        assert_eq!(pole.cast_distance, Range { low: 5.0, high: 20.0 });
        assert!(rules.pole("FishingSalmonMediumT1").is_none(), "no pole class");
        let bait = rules.bait_of_item("BaitFreshFireflyT3").expect("bait");
        assert_eq!(bait.crc, 0xa9d9_f243, "retail 2309");
        assert!(bait.works(WaterType::FreshShallow, DayPhase::Day));
        assert!(!bait.works(WaterType::SaltShallow, DayPhase::Day), "fresh only");
        assert!(!bait.works(WaterType::FreshShallow, DayPhase::Night));
        let salmon = rules.catchable("fishingsalmonmediumt1").expect("salmon");
        assert_eq!(salmon.behaviors, ["Lazy", "SporadicEasy"]);
        assert_eq!(salmon.length, Range { low: 9.0, high: 15.0 });
        let sporadic = rules.behavior("sporadiceasy").expect("behavior");
        assert_eq!(sporadic.bits, 0x92, "retail 3172 bits of 01001001");
        assert_eq!(sporadic.crc, 0xd26d_ad61);
        assert!(sporadic.fighting(1) && !sporadic.fighting(0));
        assert!(!sporadic.fighting(9), "tired after the pattern");
        assert_eq!(rules.behavior("Lazy").map(|lazy| lazy.bits), Some(0));
        let waits = rules.waits(WaterType::FreshShallow).expect("waits");
        assert_eq!(waits.nibble_s, Range { low: 10.0, high: 20.0 });
        assert_eq!(rules.waits(WaterType::SaltDeep), None);
        assert!((rules.cast_bonus(1) - 0.05).abs() < f64::EPSILON);
        assert!((rules.cast_bonus(200) - 0.05).abs() < f64::EPSILON, "the top rank's");
    }

    #[test]
    fn hotspots_come_from_their_slices_and_need_their_rank() {
        let rules = FishingRules::from_tables(&tables()).with_hotspots([
            (
                "slices/coatgen/fishing/fishing_hotspot_broad_4.dynamicslice",
                [100.0, 200.0, 40.0],
            ),
            ("slices/coatgen/fishing/fishing_hotspot_broad_99.dynamicslice", [0.0, 0.0, 0.0]),
            ("slices/pois/other.dynamicslice", [100.0, 200.0, 40.0]),
        ]);
        let hotspot = rules.hotspot_at(101.0, 201.0, 2).expect("inside its 2 m");
        assert_eq!(hotspot.id, "Hotspot_Broad_4");
        assert_eq!(hotspot.loot_table, "FishingHotspotLoot");
        assert_eq!(hotspot.loot_tag, "Broad");
        assert!(rules.hotspot_at(101.0, 201.0, 1).is_none(), "rank too low");
        assert!(rules.hotspot_at(103.0, 200.0, 2).is_none(), "outside");
        assert!(rules.hotspot_at(0.0, 0.0, 9).is_none(), "an unknown hotspot is not placed");
    }

    #[test]
    fn the_water_flags_and_depth_give_the_water_type() {
        const SEA: u32 = 0x0a;
        const LAKE: u32 = 0x07;
        const POND: u32 = 0x04;
        assert_eq!(WaterType::of(SEA, 2.0), Some(WaterType::SaltDeep));
        assert_eq!(WaterType::of(SEA, 1.0), Some(WaterType::SaltShallow));
        assert_eq!(WaterType::of(LAKE, 0.5), Some(WaterType::FreshVeryShallow));
        assert_eq!(WaterType::of(LAKE, 0.1), None, "too shallow");
        assert_eq!(WaterType::of(POND, 3.0), None, "not fishable");
        assert!(WaterType::SaltDeep.is_deep() && WaterType::SaltDeep.is_salt());
        assert_eq!(WaterType::FreshShallow.tag(), FRESH_TAG);
        assert_eq!(WaterType::FreshShallow as u8, 2);
    }

    #[test]
    fn ranges_read_both_forms() {
        assert_eq!(Range::of("1-1.5"), Some(Range { low: 1.0, high: 1.5 }));
        assert_eq!(Range::of(" 25 "), Some(Range { low: 25.0, high: 25.0 }));
        assert_eq!(Range::of("x"), None);
        assert!((Range { low: 5.0, high: 20.0 }.at(0.5) - 12.5).abs() < f64::EPSILON);
        assert!((Range { low: 5.0, high: 20.0 }.at(2.0) - 20.0).abs() < f64::EPSILON);
        assert_eq!(DayPhase::of(2), Some(DayPhase::Day));
        assert_eq!(DayPhase::of(4), None);
    }
}
