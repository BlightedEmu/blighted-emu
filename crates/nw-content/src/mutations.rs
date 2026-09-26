// SPDX-License-Identifier: AGPL-3.0-or-later
//! Mutated expeditions (run 15 dungeon study, `local-data/scratch-work/run15/dungeon2/
//! REPORT-mutations.md`): the weekly mutation of the mutable expeditions, the three mutation
//! difficulties, what a difficulty does to each mob and player of the instance, and what it adds
//! to the loot and the rewards.
//!
//! Data (content bundle 25810808): `MutationDifficultyStaticData` (tiers 1 to 3),
//! `ElementalMutationStaticData` (4 categories, a row per tier), `PromotionMutationStaticData`,
//! `CurseMutationStaticData`, `MutationPerksStaticData` (the loot of each category),
//! `BuffBucketData`, `StatusEffectData`, `GameModeData.IsMutable` and `MutLootTagsOverride`.
//!
//! Rules:
//!
//! - The week (Observed, 19 retail weeks of `GameModeMutationSchedulerReplicatedState` in 549
//!   ledgers): it starts on Tuesday 07:00 UTC ([`CADENCE_ANCHOR_UNIX_S`], [`WEEK_SECONDS`]);
//!   3 mutable expeditions are mutated, all with one elemental category, one promotion and one
//!   curse. The observed weeks are kept as they were ([`OBSERVED_WEEKS`]); the server's pick for
//!   the other weeks is not in the client (best guess, [`MutationCatalog::week_at`]: a seeded
//!   pick that keeps what all 19 weeks show: no category and no curse two weeks in a row, and a
//!   category's promotion one of those seen with it).
//! - A difficulty scales a mob by its creature type (`VitalsBaseData.CreatureType`: `Dungeon`,
//!   `Dungeon+`, `Dungeon-`, `DungeonBoss`, `DungeonMiniBoss`, the client's
//!   `buffBucketForCreatureType`): its `HealthIncreaseMod` and `DamageIncreaseMod` status effects
//!   at the `HealthPotency*` and `DamagePotency*` of that type. A status effect's stat mod times
//!   its potency is the change (best guess: `Mut_HealthScale` `MaxHealthMod` 0.01 at potency 250
//!   is +250 %).
//! - The elemental category's row of the difficulty's tier gives a buff bucket per creature
//!   type (`Dungeon`, `DungeonPlus`, ...), resolved through `BuffBucketData` (`AND` takes every
//!   entry, `OR` one).
//! - The promotion's `Promotion<tier>` status effect goes to the strong creature types (best
//!   guess: `Dungeon+`, `DungeonMiniBoss`, `DungeonBoss`; the client names no target).
//! - A difficulty with `HasMinorCurse` / `HasMajorCurse` gives every mutated creature the curse's
//!   `CurseMinor` / `CurseMajor`, its `Wildcard` replaced by the week's category
//!   (`Mut_Curse1_Wildcard_Major_Buff` is `Mut_Curse1_Fir_Major_Buff` in a fire week: Observed,
//!   every such id exists for the 4 categories), at potency `BaseCurseDamageMod` (best guess);
//!   the players get the curse's `GlobalAffliction`.

use std::collections::{BTreeMap, HashMap, HashSet};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;
use crate::generated::{BuffBucketData, MutationDifficultyStaticData};

/// Seconds in a week: the mutation cadence.
pub const WEEK_SECONDS: u64 = 604_800;
/// The start of the first observed mutation week, 2026-06-09 07:00:00 UTC (a Tuesday): every
/// observed week starts a whole number of weeks from it.
pub const CADENCE_ANCHOR_UNIX_S: u64 = 1_780_988_400;
/// Nanoseconds in a second.
const NS_PER_SECOND: u64 = 1_000_000_000;
/// Mutated expeditions per week (Observed: 3 in all 19 weeks).
pub const SETS_PER_WEEK: usize = 3;
/// The placeholder of a curse id that the week's elemental category replaces.
const WILDCARD: &str = "Wildcard";
/// A `TRUE` cell of the datasheets.
const TRUE_CELL: &str = "true";
/// A buff bucket entry that is a status effect.
const ENTRY_STATUS_EFFECT: &str = "statuseffect";
/// A buff bucket entry that is another buff bucket.
const ENTRY_BUFF_BUCKET: &str = "buffbucket";
/// A buff bucket that takes one of its entries.
const TABLE_OR: &str = "or";
/// The suffix of the row that holds an `OR` bucket's roll thresholds.
const PROBS_SUFFIX: &str = "_probs";
/// Deepest chain of buff buckets followed (the data nests 3 deep).
const MAX_BUCKET_DEPTH: usize = 8;
/// The separator of a list cell (`MutDiff,MutDiff1`).
const LIST_SEPARATOR: char = ',';
/// The separator of a gear score range (`725-730`).
const RANGE_SEPARATOR: char = '-';
/// The potency of a bucket entry without one.
const DEFAULT_POTENCY: f64 = 1.0;
/// The factor of a mob stat without a mutation.
const UNCHANGED: f64 = 1.0;
/// Perk roll multipliers of a difficulty (`PerkRollMult1` to `PerkRollMult5`).
pub const PERK_ROLL_MULTS: usize = 5;
/// Seed of the pick of the weeks without an observation (any constant: it only has to be the
/// same on every sim).
const WEEK_SEED: u64 = 0x6d75_7461_7469_6f6e;
/// Constants of the splitmix64 mix.
const MIX_ADD: u64 = 0x9e37_79b9_7f4a_7c15;
const MIX_FIRST: u64 = 0xbf58_476d_1ce4_e5b9;
const MIX_SECOND: u64 = 0x94d0_49bb_1331_11eb;
const MIX_SHIFT_FIRST: u32 = 30;
const MIX_SHIFT_SECOND: u32 = 27;
const MIX_SHIFT_LAST: u32 = 31;
/// The draws of one week's pick: the category, the promotion, the curse, then one per game mode.
const CATEGORY_DRAW: u64 = 0;
const PROMOTION_DRAW: u64 = 1;
const CURSE_DRAW: u64 = 2;
const MODE_DRAWS_FROM: u64 = 3;

/// One observed mutation week.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedWeek {
    /// `mutationCadenceStartTime`, Unix seconds.
    pub start_unix_s: u64,
    /// The mutated game modes, in the state's order.
    pub game_modes: [&'static str; SETS_PER_WEEK],
    /// The elemental category (`CategoryWildcard`).
    pub elemental: &'static str,
    /// The promotion (`PromotionMutationID`).
    pub promotion: &'static str,
    /// The curse (`CurseMutationID`).
    pub curse: &'static str,
}

/// The 19 weeks of the retail captures (Observed: the first `GameModeMutationSchedulerReplicated
/// State` of each of 549 ledgers; the weeks of 2026-08-04 and 2026-08-18 to 09-29 partly missing
/// are not in any ledger).
pub const OBSERVED_WEEKS: [ObservedWeek; 19] = [
    ObservedWeek {
        start_unix_s: 1_780_988_400,
        game_modes: [
            "DungeonCutlassKeys00",
            "DungeonBrimstoneSands00",
            "DungeonShatteredObelisk",
        ],
        elemental: "Fir",
        promotion: "Indomitable",
        curse: "Frenzied",
    },
    ObservedWeek {
        start_unix_s: 1_781_593_200,
        game_modes: [
            "DungeonEbonscale00",
            "DungeonEdengrove00",
            "DungeonFirstLight01",
        ],
        elemental: "Ice",
        promotion: "Barbaric",
        curse: "Censored",
    },
    ObservedWeek {
        start_unix_s: 1_782_198_000,
        game_modes: [
            "DungeonGreatCleave00",
            "DungeonReekwater00",
            "DungeonShatteredObelisk",
        ],
        elemental: "Nat",
        promotion: "Savage",
        curse: "Fiendish",
    },
    ObservedWeek {
        start_unix_s: 1_782_802_800,
        game_modes: [
            "DungeonBrimstoneSands00",
            "DungeonRestlessShores01",
            "DungeonGreatCleave01",
        ],
        elemental: "Voi",
        promotion: "Oppressive",
        curse: "Frenzied",
    },
    ObservedWeek {
        start_unix_s: 1_783_407_600,
        game_modes: [
            "DungeonCutlassKeys00",
            "DungeonFirstLight01",
            "DungeonShatterMtn00",
        ],
        elemental: "Fir",
        promotion: "Indomitable",
        curse: "Censored",
    },
    ObservedWeek {
        start_unix_s: 1_784_012_400,
        game_modes: [
            "DungeonEbonscale00",
            "DungeonCutlassKeys00",
            "DungeonShatteredObelisk",
        ],
        elemental: "Ice",
        promotion: "Barbaric",
        curse: "Fiendish",
    },
    ObservedWeek {
        start_unix_s: 1_784_617_200,
        game_modes: [
            "DungeonGreatCleave00",
            "DungeonReekwater00",
            "DungeonRestlessShores01",
        ],
        elemental: "Nat",
        promotion: "Savage",
        curse: "Desiccated",
    },
    ObservedWeek {
        start_unix_s: 1_785_222_000,
        game_modes: [
            "DungeonEdengrove00",
            "DungeonFirstLight01",
            "DungeonGreatCleave01",
        ],
        elemental: "Voi",
        promotion: "Oppressive",
        curse: "Censored",
    },
    ObservedWeek {
        start_unix_s: 1_786_431_600,
        game_modes: [
            "DungeonEbonscale00",
            "DungeonShatteredObelisk",
            "DungeonGreatCleave01",
        ],
        elemental: "Ice",
        promotion: "Barbaric",
        curse: "Desiccated",
    },
    ObservedWeek {
        start_unix_s: 1_787_036_400,
        game_modes: [
            "DungeonBrimstoneSands00",
            "DungeonReekwater00",
            "DungeonRestlessShores01",
        ],
        elemental: "Nat",
        promotion: "Savage",
        curse: "Frenzied",
    },
    ObservedWeek {
        start_unix_s: 1_787_641_200,
        game_modes: [
            "DungeonEdengrove00",
            "DungeonFirstLight01",
            "DungeonCutlassKeys00",
        ],
        elemental: "Ice",
        promotion: "Barbaric",
        curse: "Censored",
    },
    ObservedWeek {
        start_unix_s: 1_788_246_000,
        game_modes: [
            "DungeonCutlassKeys00",
            "DungeonGreatCleave00",
            "DungeonBrimstoneSands00",
        ],
        elemental: "Fir",
        promotion: "Savage",
        curse: "Desiccated",
    },
    ObservedWeek {
        start_unix_s: 1_788_850_800,
        game_modes: [
            "DungeonFirstLight01",
            "DungeonReekwater00",
            "DungeonGreatCleave01",
        ],
        elemental: "Nat",
        promotion: "Oppressive",
        curse: "Fiendish",
    },
    ObservedWeek {
        start_unix_s: 1_789_455_600,
        game_modes: [
            "DungeonEdengrove00",
            "DungeonShatteredObelisk",
            "DungeonRestlessShores01",
        ],
        elemental: "Voi",
        promotion: "Indomitable",
        curse: "Frenzied",
    },
    ObservedWeek {
        start_unix_s: 1_790_060_400,
        game_modes: [
            "DungeonEbonscale00",
            "DungeonCutlassKeys00",
            "DungeonShatterMtn00",
        ],
        elemental: "Ice",
        promotion: "Barbaric",
        curse: "Desiccated",
    },
    ObservedWeek {
        start_unix_s: 1_790_665_200,
        game_modes: [
            "DungeonFirstLight01",
            "DungeonGreatCleave00",
            "DungeonShatterMtn00",
        ],
        elemental: "Fir",
        promotion: "Savage",
        curse: "Fiendish",
    },
    ObservedWeek {
        start_unix_s: 1_791_270_000,
        game_modes: [
            "DungeonEdengrove00",
            "DungeonCutlassKeys00",
            "DungeonShatteredObelisk",
        ],
        elemental: "Nat",
        promotion: "Oppressive",
        curse: "Frenzied",
    },
    ObservedWeek {
        start_unix_s: 1_791_874_800,
        game_modes: [
            "DungeonBrimstoneSands00",
            "DungeonGreatCleave01",
            "DungeonReekwater00",
        ],
        elemental: "Voi",
        promotion: "Indomitable",
        curse: "Censored",
    },
    ObservedWeek {
        start_unix_s: 1_792_479_600,
        game_modes: [
            "DungeonCutlassKeys00",
            "DungeonBrimstoneSands00",
            "DungeonShatterMtn00",
        ],
        elemental: "Ice",
        promotion: "Barbaric",
        curse: "Fiendish",
    },
];

/// The creature types a mutation changes (`VitalsBaseData.CreatureType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MutatedCreature {
    /// `Dungeon`.
    Dungeon,
    /// `Dungeon+`.
    DungeonPlus,
    /// `Dungeon-`.
    DungeonMinus,
    /// `DungeonBoss`.
    DungeonBoss,
    /// `DungeonMiniBoss`.
    DungeonMiniBoss,
}

impl MutatedCreature {
    /// The mutated creature of `creature_type` (any case); `None` for another type.
    pub fn of(creature_type: &str) -> Option<Self> {
        Some(match creature_type.to_lowercase().as_str() {
            "dungeon" => Self::Dungeon,
            "dungeon+" => Self::DungeonPlus,
            "dungeon-" => Self::DungeonMinus,
            "dungeonboss" => Self::DungeonBoss,
            "dungeonminiboss" => Self::DungeonMiniBoss,
            _ => return None,
        })
    }

    /// Whether it gets the week's promotion (best guess: the strong creature types).
    const fn promoted(self) -> bool {
        matches!(
            self,
            Self::DungeonPlus | Self::DungeonBoss | Self::DungeonMiniBoss
        )
    }
}

/// A value for each mutated creature type.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerCreature<T> {
    /// `Dungeon`.
    pub dungeon: T,
    /// `Dungeon+`.
    pub plus: T,
    /// `Dungeon-`.
    pub minus: T,
    /// `DungeonBoss`.
    pub boss: T,
    /// `DungeonMiniBoss`.
    pub mini_boss: T,
}

impl<T> PerCreature<T> {
    /// The value of `creature`.
    pub const fn of(&self, creature: MutatedCreature) -> &T {
        match creature {
            MutatedCreature::Dungeon => &self.dungeon,
            MutatedCreature::DungeonPlus => &self.plus,
            MutatedCreature::DungeonMinus => &self.minus,
            MutatedCreature::DungeonBoss => &self.boss,
            MutatedCreature::DungeonMiniBoss => &self.mini_boss,
        }
    }
}

/// One mutation difficulty (`MutationDifficultyStaticData`).
#[derive(Debug, Clone, PartialEq)]
pub struct Difficulty {
    /// `MutationDifficulty`: 1 to 3.
    pub tier: u8,
    /// `DifficultyTier`: the elemental row of this difficulty.
    pub elemental_tier: u8,
    /// `HealthIncreaseMod`: the status effect that scales health.
    pub health_effect: String,
    /// `DamageIncreaseMod`: the status effect that scales damage.
    pub damage_effect: String,
    /// `HealthPotency*`.
    pub health_potency: PerCreature<f64>,
    /// `DamagePotency*`.
    pub damage_potency: PerCreature<f64>,
    /// `HasMinorCurse`.
    pub minor_curse: bool,
    /// `HasMajorCurse`.
    pub major_curse: bool,
    /// `BaseCurseDamageMod`.
    pub curse_potency: f64,
    /// `InjectedLootTags`.
    pub loot_tags: Vec<String>,
    /// `InjectedCreatureLoot`.
    pub creature_loot: Option<String>,
    /// `InjectedContainerLoot`.
    pub container_loot: Option<String>,
    /// `LootGSRangeOverride`: the low and high gear score.
    pub gear_score_range: Option<(u32, u32)>,
    /// `PerkRollMult1` to `PerkRollMult5`.
    pub perk_roll_mults: [f64; PERK_ROLL_MULTS],
    /// `CompletionEvent1` to `CompletionEvent3`.
    pub completion_events: Vec<String>,
    /// `MatchmakingMinLevel`: the lowest character level that may enter.
    pub min_level: u32,
    /// `RecommendedGearScore`.
    pub recommended_gear_score: u32,
}

/// One elemental row (`ElementalMutationStaticData`).
#[derive(Debug, Clone, PartialEq)]
struct Elemental {
    /// `CategoryWildcard` (`Fir`).
    category: String,
    /// `ElementalDifficultyTier`.
    tier: u8,
    /// The buff bucket of each creature type.
    buckets: PerCreature<String>,
}

/// One promotion (`PromotionMutationStaticData`).
#[derive(Debug, Clone, PartialEq)]
struct Promotion {
    /// `PromotionMutationID`.
    id: String,
    /// `Promotion1` to `Promotion3`.
    effects: Vec<String>,
}

/// One curse (`CurseMutationStaticData`).
#[derive(Debug, Clone, PartialEq)]
struct Curse {
    /// `CurseMutationID`.
    id: String,
    /// `CurseMinor`.
    minor: String,
    /// `CurseMajor`.
    major: String,
    /// `GlobalAffliction`.
    affliction: Option<String>,
}

/// The loot of one elemental category (`MutationPerksStaticData`).
#[derive(Debug, Clone, PartialEq)]
struct Perks {
    /// `ElementalMutationTypeID`.
    category: String,
    /// `InjectedLootTags`.
    loot_tags: Vec<String>,
    /// `InjectedCreatureLoot`.
    creature_loot: String,
    /// `InjectedContainerLoot`.
    container_loot: String,
    /// `InjectedPerkBucket1` to `5`.
    perk_buckets: Vec<String>,
}

/// One entry of a buff bucket.
#[derive(Debug, Clone, PartialEq)]
enum BucketEntry {
    /// A status effect at a potency.
    Effect(String, f64),
    /// Another bucket at a potency.
    Bucket(String, f64),
}

/// One buff bucket.
#[derive(Debug, Clone, PartialEq)]
struct Bucket {
    /// `TableType` `OR`: one entry, else every entry.
    one_of: bool,
    /// The entries.
    entries: Vec<BucketEntry>,
    /// The roll thresholds of an `OR` bucket (`<id>_Probs`) and its `MaxRoll`.
    thresholds: Option<(Vec<u32>, u32)>,
}

/// One mutable expedition.
#[derive(Debug, Clone, PartialEq)]
pub struct MutableMode {
    /// `GameModeID`.
    pub id: String,
    /// CRC-32 of the lowercase id.
    pub crc: u32,
    /// `MutLootTagsOverride`, else `LootTags`: the game mode's tags in a mutated run.
    pub loot_tags: Vec<String>,
}

/// One mutated expedition of a week (`GameModeMutationSet` with its game mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationSet {
    /// `GameModeID`.
    pub game_mode: String,
    /// The elemental category (`Fir`).
    pub elemental: String,
    /// `PromotionMutationID`.
    pub promotion: String,
    /// `CurseMutationID`.
    pub curse: String,
}

impl MutationSet {
    /// The wire entry: the CRC-32 of each lowercase id.
    pub fn crcs(&self) -> [u32; 4] {
        [
            lowercase_crc32(&self.game_mode),
            lowercase_crc32(&self.elemental),
            lowercase_crc32(&self.promotion),
            lowercase_crc32(&self.curse),
        ]
    }
}

/// One mutation week.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationWeek {
    /// Its start, Unix seconds.
    pub start_unix_s: u64,
    /// The mutated expeditions.
    pub sets: Vec<MutationSet>,
}

impl MutationWeek {
    /// Its start, Unix nanoseconds (`mutationCadenceStartTime`).
    pub const fn start_ns(&self) -> u64 {
        self.start_unix_s.saturating_mul(NS_PER_SECOND)
    }

    /// The set of game mode `game_mode` (any case).
    pub fn set_of(&self, game_mode: &str) -> Option<&MutationSet> {
        self.sets
            .iter()
            .find(|set| set.game_mode.eq_ignore_ascii_case(game_mode))
    }
}

/// The mutation of one instance: the week's set of its game mode and the chosen difficulty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationContext {
    /// The week's set.
    pub set: MutationSet,
    /// `MutationDifficulty`: 1 to 3.
    pub difficulty: u8,
}

/// A status effect a mutation puts on a mob or a player.
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedEffect {
    /// `StatusID`.
    pub status_id: String,
    /// The potency.
    pub potency: f64,
}

/// What a mutation does to one mob.
#[derive(Debug, Clone, PartialEq)]
pub struct MobMutation {
    /// Its maximum health times this.
    pub health_factor: f64,
    /// Its damage times this.
    pub damage_factor: f64,
    /// The status effects it gets (elemental, promotion, curse).
    pub effects: Vec<AppliedEffect>,
}

impl Default for MobMutation {
    fn default() -> Self {
        Self {
            health_factor: UNCHANGED,
            damage_factor: UNCHANGED,
            effects: Vec::new(),
        }
    }
}

/// What a mutation adds to the loot of the instance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MutatedLoot {
    /// The loot tags of the run: the game mode's mutated tags, the difficulty's and the
    /// category's injected tags.
    pub tags: Vec<String>,
    /// The loot tables a mob kill rolls besides its own (`InjectedCreatureLoot`).
    pub creature_tables: Vec<String>,
    /// The loot tables a container rolls besides its own (`InjectedContainerLoot`).
    pub container_tables: Vec<String>,
    /// `LootGSRangeOverride`.
    pub gear_score_range: Option<(u32, u32)>,
    /// `PerkRollMult1` to `PerkRollMult5`.
    pub perk_roll_mults: [f64; PERK_ROLL_MULTS],
    /// The category's perk buckets.
    pub perk_buckets: Vec<String>,
}

/// Every mutation rule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MutationCatalog {
    difficulties: BTreeMap<u8, Difficulty>,
    elementals: Vec<Elemental>,
    promotions: Vec<Promotion>,
    curses: Vec<Curse>,
    perks: Vec<Perks>,
    /// Buff buckets by lowercase id.
    buckets: HashMap<String, Bucket>,
    /// Every status effect id, by lowercase id.
    effects: HashMap<String, String>,
    /// The stat mods of the difficulties' scale effects, by lowercase id: (`MaxHealthMod`,
    /// the largest `Dmg*` mod).
    scale_mods: HashMap<String, (f64, f64)>,
    /// The mutable expeditions, in datasheet order.
    modes: Vec<MutableMode>,
}

fn list(cell: Option<&str>) -> Vec<String> {
    cell.into_iter()
        .flat_map(|cell| cell.split(LIST_SEPARATOR))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn tier(value: i64) -> Option<u8> {
    u8::try_from(value).ok()
}

/// A potency cell as a factor; a negative cell is none.
#[allow(
    clippy::cast_precision_loss,
    reason = "potencies of a few hundred are exact in f64"
)]
fn potency(value: i64) -> f64 {
    value.max(0) as f64
}

fn non_empty(cell: &str) -> Option<String> {
    let cell = cell.trim();
    (!cell.is_empty()).then(|| cell.to_owned())
}

fn gear_score_range(cell: &str) -> Option<(u32, u32)> {
    let (low, high) = cell.split_once(RANGE_SEPARATOR)?;
    Some((low.trim().parse().ok()?, high.trim().parse().ok()?))
}

fn difficulty(row: &MutationDifficultyStaticData) -> Option<Difficulty> {
    Some(Difficulty {
        tier: tier(row.mutation_difficulty)?,
        elemental_tier: tier(row.difficulty_tier)?,
        health_effect: row.health_increase_mod.clone(),
        damage_effect: row.damage_increase_mod.clone(),
        health_potency: PerCreature {
            dungeon: potency(row.health_potency_dungeon),
            plus: potency(row.health_potency_dungeon_plus),
            minus: potency(row.health_potency_dungeon_minus),
            boss: potency(row.health_potency_dungeon_boss),
            mini_boss: potency(row.health_potency_dungeon_mini_boss),
        },
        damage_potency: PerCreature {
            dungeon: potency(row.damage_potency_dungeon),
            plus: potency(row.damage_potency_dungeon_plus),
            minus: potency(row.damage_potency_dungeon_minus),
            boss: potency(row.damage_potency_dungeon_boss),
            mini_boss: potency(row.damage_potency_dungeon_mini_boss),
        },
        minor_curse: row.has_minor_curse.eq_ignore_ascii_case(TRUE_CELL),
        major_curse: row.has_major_curse.eq_ignore_ascii_case(TRUE_CELL),
        curse_potency: row.base_curse_damage_mod,
        loot_tags: list(Some(&row.injected_loot_tags)),
        creature_loot: non_empty(&row.injected_creature_loot),
        container_loot: non_empty(&row.injected_container_loot),
        gear_score_range: gear_score_range(&row.loot_gs_range_override),
        perk_roll_mults: [
            row.perk_roll_mult1,
            row.perk_roll_mult2,
            row.perk_roll_mult3,
            row.perk_roll_mult4,
            row.perk_roll_mult5,
        ],
        completion_events: [
            &row.completion_event1,
            &row.completion_event2,
            &row.completion_event3,
        ]
        .into_iter()
        .filter_map(|event| non_empty(event))
        .collect(),
        min_level: u32::try_from(row.matchmaking_min_level).unwrap_or_default(),
        recommended_gear_score: u32::try_from(row.recommended_gear_score).unwrap_or_default(),
    })
}

fn bucket(row: &BuffBucketData, probs: Option<&BuffBucketData>) -> Bucket {
    let buffs = [
        (&row.buff1, &row.buff_type1, row.buff_potency1),
        (&row.buff2, &row.buff_type2, row.buff_potency2),
        (&row.buff3, &row.buff_type3, row.buff_potency3),
        (&row.buff4, &row.buff_type4, row.buff_potency4),
        (&row.buff5, &row.buff_type5, row.buff_potency5),
        (&row.buff6, &row.buff_type6, row.buff_potency6),
    ];
    let entries = buffs
        .into_iter()
        .filter_map(|(id, kind, value)| {
            let id = id.as_deref().and_then(non_empty)?;
            let value = value.map_or(DEFAULT_POTENCY, potency);
            match kind.as_deref()?.to_lowercase().as_str() {
                ENTRY_STATUS_EFFECT => Some(BucketEntry::Effect(id, value)),
                ENTRY_BUFF_BUCKET => Some(BucketEntry::Bucket(id, value)),
                _ => None,
            }
        })
        .collect();
    let thresholds = probs.map(|probs| {
        let cells = [
            &probs.buff1,
            &probs.buff2,
            &probs.buff3,
            &probs.buff4,
            &probs.buff5,
            &probs.buff6,
        ];
        let values = cells
            .into_iter()
            .filter_map(|cell| cell.as_deref()?.trim().parse::<u32>().ok())
            .collect();
        let max = probs
            .max_roll
            .and_then(|max| u32::try_from(max).ok())
            .unwrap_or_default();
        (values, max)
    });
    Bucket {
        one_of: row
            .table_type
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case(TABLE_OR)),
        entries,
        thresholds,
    }
}

/// splitmix64 of `value`.
const fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(MIX_ADD);
    z = (z ^ (z >> MIX_SHIFT_FIRST)).wrapping_mul(MIX_FIRST);
    z = (z ^ (z >> MIX_SHIFT_SECOND)).wrapping_mul(MIX_SECOND);
    z ^ (z >> MIX_SHIFT_LAST)
}

/// The element of `items` at `hash` modulo their count; `None` for none.
fn pick<T>(items: &[T], hash: u64) -> Option<&T> {
    let count = u64::try_from(items.len()).ok().filter(|count| *count > 0)?;
    items.get(usize::try_from(hash % count).ok()?)
}

/// The week number of Unix second `unix_s`: whole weeks from [`CADENCE_ANCHOR_UNIX_S`],
/// negative before it.
fn week_number(unix_s: u64) -> i64 {
    let anchor = i64::try_from(CADENCE_ANCHOR_UNIX_S).unwrap_or(i64::MAX);
    let at = i64::try_from(unix_s).unwrap_or(i64::MAX);
    let week = i64::try_from(WEEK_SECONDS).unwrap_or(i64::MAX);
    (at - anchor).div_euclid(week)
}

/// The start of week `number`, Unix seconds (0 for a week before the epoch).
fn week_start(number: i64) -> u64 {
    let week = i64::try_from(WEEK_SECONDS).unwrap_or(i64::MAX);
    let anchor = i64::try_from(CADENCE_ANCHOR_UNIX_S).unwrap_or(i64::MAX);
    u64::try_from(anchor.saturating_add(number.saturating_mul(week))).unwrap_or_default()
}

impl MutationCatalog {
    /// The rules of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let difficulties = tables
            .mutation_difficulty_static_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(difficulty)
            .map(|difficulty| (difficulty.tier, difficulty))
            .collect::<BTreeMap<_, _>>();
        let elementals = tables
            .elemental_mutation_static_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some(Elemental {
                    category: row.category_wildcard.clone(),
                    tier: tier(row.elemental_difficulty_tier)?,
                    buckets: PerCreature {
                        dungeon: row.dungeon.clone(),
                        plus: row.dungeon_plus.clone(),
                        minus: row.dungeon_minus.clone(),
                        boss: row.dungeon_boss.clone(),
                        mini_boss: row.dungeon_mini_boss.clone(),
                    },
                })
            })
            .collect();
        let promotions = tables
            .promotion_mutation_static_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| Promotion {
                id: row.promotion_mutation_id.clone(),
                effects: [&row.promotion1, &row.promotion2, &row.promotion3]
                    .into_iter()
                    .filter_map(|effect| non_empty(effect))
                    .collect(),
            })
            .collect();
        let curses = tables
            .curse_mutation_static_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| Curse {
                id: row.curse_mutation_id.clone(),
                minor: row.curse_minor.clone(),
                major: row.curse_major.clone(),
                affliction: row.global_affliction.as_deref().and_then(non_empty),
            })
            .collect();
        let perks = tables
            .mutation_perks_static_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| Perks {
                category: row.elemental_mutation_type_id.clone(),
                loot_tags: list(Some(&row.injected_loot_tags)),
                creature_loot: row.injected_creature_loot.clone(),
                container_loot: row.injected_container_loot.clone(),
                perk_buckets: [
                    row.injected_perk_bucket1.as_deref(),
                    Some(row.injected_perk_bucket2.as_str()),
                    row.injected_perk_bucket3.as_deref(),
                    row.injected_perk_bucket4.as_deref(),
                    row.injected_perk_bucket5.as_deref(),
                ]
                .into_iter()
                .flatten()
                .filter_map(non_empty)
                .collect(),
            })
            .collect();
        let bucket_rows: HashMap<String, &BuffBucketData> = tables
            .buff_bucket_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.buff_bucket_id.to_lowercase(), row))
            .collect();
        let buckets = bucket_rows
            .iter()
            .filter(|(id, _)| !id.ends_with(PROBS_SUFFIX))
            .map(|(id, row)| {
                let probs = bucket_rows.get(&format!("{id}{PROBS_SUFFIX}")).copied();
                (id.clone(), bucket(row, probs))
            })
            .collect();
        let status_rows = || {
            tables
                .status_effect_data
                .iter()
                .flat_map(|table| &table.rows)
        };
        let effects = status_rows()
            .map(|row| (row.status_id.to_lowercase(), row.status_id.clone()))
            .collect();
        let scaling: HashSet<String> = difficulties
            .values()
            .flat_map(|difficulty: &Difficulty| {
                [
                    difficulty.health_effect.to_lowercase(),
                    difficulty.damage_effect.to_lowercase(),
                ]
            })
            .collect();
        let scale_mods = status_rows()
            .filter(|row| scaling.contains(&row.status_id.to_lowercase()))
            .map(|row| {
                let damage = [
                    row.dmg_arcane,
                    row.dmg_corruption,
                    row.dmg_fire,
                    row.dmg_ice,
                    row.dmg_lightning,
                    row.dmg_nature,
                    row.dmg_siege,
                    row.dmg_slash,
                    row.dmg_standard,
                    row.dmg_strike,
                    row.dmg_thrust,
                ]
                .into_iter()
                .flatten()
                .fold(0.0_f64, f64::max);
                (
                    row.status_id.to_lowercase(),
                    (row.max_health_mod.unwrap_or_default(), damage),
                )
            })
            .collect();
        let modes = tables
            .game_mode_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter(|row| row.is_mutable.unwrap_or_default())
            .map(|row| {
                let mutated = list(row.mut_loot_tags_override.as_deref());
                MutableMode {
                    id: row.game_mode_id.clone(),
                    crc: lowercase_crc32(&row.game_mode_id),
                    loot_tags: if mutated.is_empty() {
                        list(row.loot_tags.as_deref())
                    } else {
                        mutated
                    },
                }
            })
            .collect();
        Self {
            difficulties,
            elementals,
            promotions,
            curses,
            perks,
            buckets,
            effects,
            scale_mods,
            modes,
        }
    }

    /// Difficulty `tier`.
    pub fn difficulty(&self, tier: u8) -> Option<&Difficulty> {
        self.difficulties.get(&tier)
    }

    /// The highest difficulty.
    pub fn highest_difficulty(&self) -> Option<u8> {
        self.difficulties.keys().next_back().copied()
    }

    /// The mutable expeditions.
    pub fn modes(&self) -> &[MutableMode] {
        &self.modes
    }

    /// Whether game mode `id` (any case) can be mutated.
    pub fn is_mutable(&self, id: &str) -> bool {
        self.mode(id).is_some()
    }

    fn mode(&self, id: &str) -> Option<&MutableMode> {
        self.modes
            .iter()
            .find(|mode| mode.id.eq_ignore_ascii_case(id))
    }

    /// The elemental categories, in datasheet order.
    fn categories(&self) -> Vec<&str> {
        let mut categories: Vec<&str> = Vec::new();
        for elemental in &self.elementals {
            if !categories.contains(&elemental.category.as_str()) {
                categories.push(&elemental.category);
            }
        }
        categories
    }

    /// The mutation week that holds Unix second `unix_s`: the observed one, else the seeded
    /// pick (best guess) that follows the last observed week before it.
    pub fn week_at(&self, unix_s: u64) -> MutationWeek {
        let number = week_number(unix_s);
        if let Some(week) = Self::observed(number) {
            return week;
        }
        let last_seen = OBSERVED_WEEKS
            .iter()
            .map(|week| week_number(week.start_unix_s))
            .filter(|seen| *seen < number)
            .max();
        let mut previous = last_seen.and_then(Self::observed);
        let first = last_seen.map_or(number, |seen| seen + 1);
        let mut week = self.picked(first, previous.as_ref());
        for later in first + 1..=number {
            previous = Some(week);
            week = self.picked(later, previous.as_ref());
        }
        week
    }

    /// The observed week `number`.
    fn observed(number: i64) -> Option<MutationWeek> {
        OBSERVED_WEEKS
            .iter()
            .find(|week| week_number(week.start_unix_s) == number)
            .map(|week| MutationWeek {
                start_unix_s: week.start_unix_s,
                sets: week
                    .game_modes
                    .iter()
                    .map(|game_mode| MutationSet {
                        game_mode: (*game_mode).to_owned(),
                        elemental: week.elemental.to_owned(),
                        promotion: week.promotion.to_owned(),
                        curse: week.curse.to_owned(),
                    })
                    .collect(),
            })
    }

    /// The seeded pick of week `number` after `previous` (best guess): a category other than
    /// the previous week's, one of the promotions seen with it (any when none was seen) other
    /// than the previous week's when it can, a curse other than the previous week's, and
    /// [`SETS_PER_WEEK`] distinct mutable expeditions.
    fn picked(&self, number: i64, previous: Option<&MutationWeek>) -> MutationWeek {
        let before = previous.and_then(|week| week.sets.first());
        let seed = mix(WEEK_SEED ^ number.cast_unsigned());
        let categories: Vec<&str> = self
            .categories()
            .into_iter()
            .filter(|category| before.is_none_or(|set| set.elemental != *category))
            .collect();
        let elemental = pick(&categories, mix(seed.wrapping_add(CATEGORY_DRAW)))
            .copied()
            .unwrap_or_default();
        let seen: Vec<&str> = OBSERVED_WEEKS
            .iter()
            .filter(|week| week.elemental == elemental)
            .map(|week| week.promotion)
            .collect();
        let mut promotions: Vec<&str> = self
            .promotions
            .iter()
            .map(|promotion| promotion.id.as_str())
            .filter(|promotion| seen.is_empty() || seen.contains(promotion))
            .collect();
        if promotions.len() > 1 {
            promotions.retain(|promotion| before.is_none_or(|set| set.promotion != *promotion));
        }
        let promotion = pick(&promotions, mix(seed.wrapping_add(PROMOTION_DRAW)))
            .copied()
            .unwrap_or_default();
        let curses: Vec<&str> = self
            .curses
            .iter()
            .map(|curse| curse.id.as_str())
            .filter(|curse| before.is_none_or(|set| set.curse != *curse))
            .collect();
        let curse = pick(&curses, mix(seed.wrapping_add(CURSE_DRAW)))
            .copied()
            .unwrap_or_default();
        let mut left: Vec<&MutableMode> = self.modes.iter().collect();
        let mut sets = Vec::new();
        for draw_number in (MODE_DRAWS_FROM..).take(SETS_PER_WEEK) {
            let draw = mix(seed.wrapping_add(draw_number));
            let Some(index) = u64::try_from(left.len())
                .ok()
                .filter(|count| *count > 0)
                .and_then(|count| usize::try_from(draw % count).ok())
            else {
                break;
            };
            let mode = left.remove(index);
            sets.push(MutationSet {
                game_mode: mode.id.clone(),
                elemental: elemental.to_owned(),
                promotion: promotion.to_owned(),
                curse: curse.to_owned(),
            });
        }
        MutationWeek {
            start_unix_s: week_start(number),
            sets,
        }
    }

    /// The mutation of game mode `game_mode` activated at Unix second `activated_unix_s` at
    /// difficulty `difficulty`; `None` when the game mode is not mutated that week or the
    /// difficulty does not exist.
    pub fn context(
        &self,
        game_mode: &str,
        activated_unix_s: u64,
        difficulty: u8,
    ) -> Option<MutationContext> {
        self.difficulty(difficulty)?;
        let set = self.week_at(activated_unix_s).set_of(game_mode)?.clone();
        Some(MutationContext { set, difficulty })
    }

    /// A curse id with its `Wildcard` replaced by `category`.
    fn with_category(id: &str, category: &str) -> String {
        id.replace(WILDCARD, category)
    }

    /// The status effects of buff bucket or status effect `id` at `potency`, into `out`; `below`
    /// rolls an `OR` bucket. An id that is neither is left out.
    fn resolve(
        &self,
        id: &str,
        potency: f64,
        depth: usize,
        below: &mut impl FnMut(u32) -> u32,
        out: &mut Vec<AppliedEffect>,
    ) {
        if depth > MAX_BUCKET_DEPTH {
            return;
        }
        let key = id.to_lowercase();
        if let Some(status_id) = self.effects.get(&key) {
            out.push(AppliedEffect {
                status_id: status_id.clone(),
                potency,
            });
            return;
        }
        let Some(bucket) = self.buckets.get(&key) else {
            return;
        };
        let chosen: Vec<&BucketEntry> = if bucket.one_of {
            Self::one_entry(bucket, below).into_iter().collect()
        } else {
            bucket.entries.iter().collect()
        };
        for entry in chosen {
            match entry {
                BucketEntry::Effect(id, entry_potency) | BucketEntry::Bucket(id, entry_potency) => {
                    self.resolve(id, potency * entry_potency, depth + 1, below, out);
                }
            }
        }
    }

    /// The entry an `OR` bucket takes: with roll thresholds and a `MaxRoll`, the last entry whose
    /// threshold the roll in `0..=MaxRoll` reaches; else one entry at random (best guess: every
    /// retail `_Probs` row of a mutation bucket is all 0 with `MaxRoll` 0).
    fn one_entry<'a>(
        bucket: &'a Bucket,
        below: &mut impl FnMut(u32) -> u32,
    ) -> Option<&'a BucketEntry> {
        let count = u32::try_from(bucket.entries.len())
            .ok()
            .filter(|n| *n > 0)?;
        if let Some((thresholds, max)) = &bucket.thresholds
            && *max > 0
        {
            let roll = below(max.saturating_add(1));
            let index = thresholds
                .iter()
                .take(bucket.entries.len())
                .rposition(|threshold| roll >= *threshold)?;
            return bucket.entries.get(index);
        }
        bucket
            .entries
            .get(usize::try_from(below(count)).unwrap_or_default())
    }

    /// The stat factor of scale effect `effect` at `potency`, with `mod_of` its mod.
    fn scale(&self, effect: &str, potency: f64, mod_of: impl Fn(&(f64, f64)) -> f64) -> f64 {
        self.scale_mods
            .get(&effect.to_lowercase())
            .map_or(UNCHANGED, |mods| UNCHANGED + mod_of(mods) * potency)
    }

    /// What mutation `context` does to a mob of creature type `creature_type`; `below` rolls the
    /// `OR` buckets. A mob of another creature type is unchanged.
    pub fn mob_mutation(
        &self,
        context: &MutationContext,
        creature_type: Option<&str>,
        below: &mut impl FnMut(u32) -> u32,
    ) -> MobMutation {
        let (Some(creature), Some(difficulty)) = (
            creature_type.and_then(MutatedCreature::of),
            self.difficulty(context.difficulty),
        ) else {
            return MobMutation::default();
        };
        let set = &context.set;
        let mut effects = Vec::new();
        if let Some(elemental) = self.elementals.iter().find(|elemental| {
            elemental.category.eq_ignore_ascii_case(&set.elemental)
                && elemental.tier == difficulty.elemental_tier
        }) {
            self.resolve(
                elemental.buckets.of(creature),
                DEFAULT_POTENCY,
                0,
                below,
                &mut effects,
            );
        }
        if creature.promoted()
            && let Some(effect) = self
                .promotions
                .iter()
                .find(|promotion| promotion.id.eq_ignore_ascii_case(&set.promotion))
                .and_then(|promotion| {
                    promotion
                        .effects
                        .get(usize::from(difficulty.tier.saturating_sub(1)))
                })
        {
            self.resolve(effect, DEFAULT_POTENCY, 0, below, &mut effects);
        }
        if let Some(curse) = self.curse(&set.curse) {
            let curse_potency = if difficulty.curse_potency > 0.0 {
                difficulty.curse_potency
            } else {
                DEFAULT_POTENCY
            };
            let parts = [
                (difficulty.minor_curse, &curse.minor),
                (difficulty.major_curse, &curse.major),
            ];
            for (_, id) in parts.into_iter().filter(|(has, _)| *has) {
                let id = Self::with_category(id, &set.elemental);
                self.resolve(&id, curse_potency, 0, below, &mut effects);
            }
        }
        MobMutation {
            health_factor: self.scale(
                &difficulty.health_effect,
                *difficulty.health_potency.of(creature),
                |(health, _)| *health,
            ),
            damage_factor: self.scale(
                &difficulty.damage_effect,
                *difficulty.damage_potency.of(creature),
                |(_, damage)| *damage,
            ),
            effects,
        }
    }

    fn curse(&self, id: &str) -> Option<&Curse> {
        self.curses
            .iter()
            .find(|curse| curse.id.eq_ignore_ascii_case(id))
    }

    /// The affliction every player of an instance of `context` gets: the curse's
    /// `GlobalAffliction` when the difficulty has a curse.
    pub fn player_affliction(&self, context: &MutationContext) -> Option<&str> {
        let difficulty = self.difficulty(context.difficulty)?;
        if !(difficulty.minor_curse || difficulty.major_curse) {
            return None;
        }
        self.curse(&context.set.curse)?.affliction.as_deref()
    }

    /// What mutation `context` adds to the loot of its instance.
    pub fn loot(&self, context: &MutationContext) -> MutatedLoot {
        let Some(difficulty) = self.difficulty(context.difficulty) else {
            return MutatedLoot::default();
        };
        let perks = self
            .perks
            .iter()
            .find(|perks| perks.category.eq_ignore_ascii_case(&context.set.elemental));
        let mut tags: Vec<String> = self
            .mode(&context.set.game_mode)
            .map(|mode| mode.loot_tags.clone())
            .unwrap_or_default();
        tags.extend(difficulty.loot_tags.iter().cloned());
        tags.extend(
            perks
                .iter()
                .flat_map(|perks| perks.loot_tags.iter().cloned()),
        );
        MutatedLoot {
            tags,
            creature_tables: difficulty
                .creature_loot
                .iter()
                .cloned()
                .chain(perks.map(|perks| perks.creature_loot.clone()))
                .collect(),
            container_tables: difficulty
                .container_loot
                .iter()
                .cloned()
                .chain(perks.map(|perks| perks.container_loot.clone()))
                .collect(),
            gear_score_range: difficulty.gear_score_range,
            perk_roll_mults: difficulty.perk_roll_mults,
            perk_buckets: perks
                .map(|perks| perks.perk_buckets.clone())
                .unwrap_or_default(),
        }
    }

    /// The game event a completed instance of `context` grants each player (best guess: the
    /// difficulty's `CompletionEvent1`; what earns `2` and `3` is not in the client data).
    pub fn completion_event(&self, context: &MutationContext) -> Option<&str> {
        self.difficulty(context.difficulty)?
            .completion_events
            .first()
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{
        BuffBucketData, CurseMutationStaticData, ElementalMutationStaticData, GameModeData,
        MutationPerksStaticData, PromotionMutationStaticData, StatusEffectData,
    };
    use crate::table::Table;

    /// 2026-10-20 07:00 UTC plus a day: inside the last observed week.
    const LAST_OBSERVED_DAY: u64 = 1_792_479_600 + 86_400;
    /// One roll: always the first.
    fn first(_: u32) -> u32 {
        0
    }

    fn table<T>(rows: Vec<T>) -> Vec<Table<T>> {
        vec![Table {
            source_path: String::new(),
            unique_id: String::new(),
            rows,
        }]
    }

    fn row<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).expect("row")
    }

    /// The retail difficulty 3 row (javelindata_mutationdifficulty, build 25810808), less its
    /// matchmaking cells.
    fn difficulty_three() -> MutationDifficultyStaticData {
        // A `json!` value this wide exceeds the macro recursion limit.
        row(serde_json::from_str(
            r#"{
            "base_curse_damage_mod": 2.0,
            "completion_event1": "MutDiff3T1",
            "completion_event2": "MutDiff3T2",
            "completion_event3": "MutDiff3T3",
            "damage_increase_mod": "Mut_DamageScale",
            "damage_potency_dungeon": 175,
            "damage_potency_dungeon_plus": 175,
            "damage_potency_dungeon_minus": 250,
            "damage_potency_dungeon_boss": 160,
            "damage_potency_dungeon_mini_boss": 160,
            "difficulty_tier": 3,
            "gs_clamp_level_req_delta": 2,
            "gear_score_delta_modifier": 0.2,
            "has_major_curse": "TRUE",
            "has_minor_curse": "TRUE",
            "health_increase_mod": "Mut_HealthScale",
            "health_potency_dungeon": 250,
            "health_potency_dungeon_plus": 250,
            "health_potency_dungeon_minus": 175,
            "health_potency_dungeon_boss": 220,
            "health_potency_dungeon_mini_boss": 220,
            "injected_container_loot": "MutatorLoot_Difficulty",
            "injected_creature_loot": "MutatorLoot_Difficulty",
            "injected_loot_tags": "MutDiff,MutDiff1,MutDiff2,MutDiff3",
            "loot_gs_range_override": "730-730",
            "matchmaking_max_dps": 3,
            "matchmaking_max_healers": 1,
            "matchmaking_max_level": 65,
            "matchmaking_max_tanks": 1,
            "matchmaking_min_dps": 3,
            "matchmaking_min_gs": 695,
            "matchmaking_min_healers": 1,
            "matchmaking_min_level": 65,
            "matchmaking_min_tanks": 1,
            "matchmaking_required_healing_focus": 350,
            "matchmaking_required_healing_mastery": 19,
            "matchmaking_requires_taunt_gem": true,
            "mutation_difficulty": 3,
            "perk_roll_mult1": 5.0,
            "perk_roll_mult2": 5.0,
            "perk_roll_mult3": 5.0,
            "perk_roll_mult4": 5.0,
            "perk_roll_mult5": 0.5,
            "recommended_gear_score": 695,
            "use_mm_buff": false,
            "uses_role_selection": false
        }"#,
        )
        .expect("difficulty row"))
    }

    fn status(id: &str, extra: serde_json::Value) -> StatusEffectData {
        let mut value = serde_json::json!({ "status_id": id });
        if let (Some(object), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            object.extend(extra.clone());
        }
        row(value)
    }

    fn mode(id: &str, tags: &str, mutated: Option<&str>) -> GameModeData {
        row(serde_json::json!({
            "allow_camping": false,
            "feature_flag": "",
            "game_mode_id": id,
            "raid_invite_delay_sec": 0,
            "raid_invite_duration_sec": 0,
            "script_name": id,
            "is_mutable": true,
            "loot_tags": tags,
            "mut_loot_tags_override": mutated,
        }))
    }

    /// The retail rows of the ice week (build 25810808) that the tests read.
    #[allow(
        clippy::field_reassign_with_default,
        reason = "Tables has hundreds of sheets; the test sets only the ones it reads"
    )]
    fn catalog() -> MutationCatalog {
        let mut tables = Tables::default();
        tables.mutation_difficulty_static_data = table(vec![difficulty_three()]);
        tables.elemental_mutation_static_data = table(
            ["Fir", "Ice", "Nat", "Voi"]
                .into_iter()
                .map(|category| -> ElementalMutationStaticData {
                    row(serde_json::json!({
                        "background_color": "",
                        "category_wildcard": category,
                        "description": "",
                        "dungeon": format!("Mutation_{category}_Dungeon_Tier3"),
                        "dungeon_plus": format!("Mutation_{category}_DungeonPlus_Tier3"),
                        "dungeon_minus": format!("Mutation_{category}_DungeonMinus_Tier3"),
                        "dungeon_boss": "Mutation_DungeonBoss_Tier3_Buff",
                        "dungeon_mini_boss": format!("Mutation_{category}_DungeonPlus_Tier3"),
                        "elemental_difficulty_tier": 3,
                        "elemental_mutation_id": format!("{category}_3"),
                        "icon_path": "",
                        "name": "",
                        "text_color": ""
                    }))
                })
                .collect(),
        );
        tables.promotion_mutation_static_data = table(
            [
                (
                    "Savage",
                    "Mut_Disease_1_Buff",
                    "Mut_Disease_2_Buff",
                    "Mut_RotatingBeam_Buff",
                ),
                (
                    "Barbaric",
                    "Mut_StaminaDamage_1_Buff",
                    "Mut_StaminaDamage_2_Buff",
                    "Mut_Enrage_Buff",
                ),
                (
                    "Oppressive",
                    "Mut_ManaThorns_1_Buff",
                    "Mut_ManaThorns_2_Buff",
                    "Mut_SilenceArea_Buff",
                ),
                (
                    "Indomitable",
                    "Mut_LifeSteal_1_Buff",
                    "Mut_LifeSteal_2_Buff",
                    "Mut_ProjectileShield_Buff",
                ),
            ]
            .into_iter()
            .map(|(id, one, two, three)| -> PromotionMutationStaticData {
                row(serde_json::json!({
                    "background_color": "",
                    "description": "",
                    "icon_path": "",
                    "name": "",
                    "promotion1": one,
                    "promotion2": two,
                    "promotion3": three,
                    "promotion_mutation_id": id,
                    "text_color": ""
                }))
            })
            .collect(),
        );
        tables.curse_mutation_static_data = table(
            [
                (
                    "Frenzied",
                    "Mut_Curse1_Wildcard_Minor_Buff",
                    "Mut_Curse1_Wildcard_Major_Buff",
                    None,
                ),
                (
                    "Fiendish",
                    "Mut_Curse2_Minor_Buff",
                    "Mut_Curse2_Wildcard_Major_Buff",
                    Some("AfflictionCurse2"),
                ),
                (
                    "Desiccated",
                    "Mut_Wildcard_Curse3_Minor_Buff",
                    "Mut_Wildcard_Curse3_Major_Buff",
                    Some("AfflictionCurse3"),
                ),
                (
                    "Censored",
                    "Mut_Wildcard_Curse4_Minor_Buff",
                    "Mut_Curse4_Major_Buff",
                    Some("AfflictionCurse4"),
                ),
            ]
            .into_iter()
            .map(
                |(id, minor, major, affliction)| -> CurseMutationStaticData {
                    row(serde_json::json!({
                        "curse_major": major,
                        "curse_minor": minor,
                        "curse_mutation_id": id,
                        "description": "",
                        "global_affliction": affliction,
                        "icon_path": "",
                        "name": ""
                    }))
                },
            )
            .collect(),
        );
        tables.mutation_perks_static_data =
            table(vec![row::<MutationPerksStaticData>(serde_json::json!({
                "description": "",
                "elemental_mutation_type_id": "Ice",
                "injected_container_loot": "MutatorLoot_Ice",
                "injected_creature_loot": "MutatorLoot_Ice",
                "injected_loot_tags": "Ice",
                "injected_perk_bucket2": "PerkBucket_MutatorIce",
                "name": ""
            }))]);
        tables.buff_bucket_data = table(vec![
            row::<BuffBucketData>(serde_json::json!({
                "buff1": "Mut_Ice_Damage_Buff",
                "buff2": "Mutation_Ice_DungeonPlus_Tier3_Promo",
                "buff_bucket_id": "Mutation_Ice_DungeonPlus_Tier3",
                "buff_potency1": 1,
                "buff_potency2": 1,
                "buff_type1": "StatusEffect",
                "buff_type2": "BuffBucket",
                "table_type": "AND"
            })),
            row::<BuffBucketData>(serde_json::json!({
                "buff1": "Mut_Ice_Promo1",
                "buff2": "Mut_Ice_Promo2",
                "buff_bucket_id": "Mutation_Ice_DungeonPlus_Tier3_Promo",
                "buff_potency1": 1,
                "buff_potency2": 1,
                "buff_type1": "StatusEffect",
                "buff_type2": "StatusEffect",
                "table_type": "OR"
            })),
            row::<BuffBucketData>(serde_json::json!({
                "buff1": "0",
                "buff2": "0",
                "buff_bucket_id": "Mutation_Ice_DungeonPlus_Tier3_Promo_Probs",
                "max_roll": 0
            })),
        ]);
        tables.status_effect_data = table(vec![
            status(
                "Mut_HealthScale",
                serde_json::json!({ "max_health_mod": 0.01 }),
            ),
            status(
                "Mut_DamageScale",
                serde_json::json!({ "dmg_standard": 0.01, "dmg_fire": 0.01 }),
            ),
            status("Mut_Ice_Damage_Buff", serde_json::json!({})),
            status("Mut_Ice_Promo1", serde_json::json!({})),
            status("Mut_Ice_Promo2", serde_json::json!({})),
            status("Mut_Enrage_Buff", serde_json::json!({})),
            status("Mut_Curse2_Minor_Buff", serde_json::json!({})),
            status("Mut_Curse2_Ice_Major_Buff", serde_json::json!({})),
        ]);
        tables.game_mode_data = table(
            [
                "DungeonCutlassKeys00",
                "DungeonBrimstoneSands00",
                "DungeonShatterMtn00",
                "DungeonEbonscale00",
                "DungeonEdengrove00",
            ]
            .into_iter()
            .map(|id| mode(id, "Lost,Dungeon", Some("CutlassKeys00_Mut,GypsumBlue")))
            .collect(),
        );
        MutationCatalog::from_tables(&tables)
    }

    fn context(difficulty: u8) -> MutationContext {
        catalog()
            .context("dungeoncutlasskeys00", LAST_OBSERVED_DAY, difficulty)
            .expect("mutated this week")
    }

    #[test]
    fn an_observed_week_is_the_retail_one() {
        let week = catalog().week_at(LAST_OBSERVED_DAY);
        assert_eq!(week.start_unix_s, 1_792_479_600);
        assert_eq!(week.start_ns(), 1_792_479_600_000_000_000);
        let set = week.set_of("DungeonCutlassKeys00").expect("set");
        assert_eq!(
            (
                set.elemental.as_str(),
                set.promotion.as_str(),
                set.curse.as_str()
            ),
            ("Ice", "Barbaric", "Fiendish")
        );
        // The wire entry of the retail newchar state (`mutation_scheduler` sample).
        assert_eq!(
            set.crcs(),
            [0x2348_25a2, 0xcb4f_840e, 0xcb5e_7b4d, 0xa78a_7b02]
        );
        assert!(week.set_of("DungeonEdengrove00").is_none());
    }

    #[test]
    fn every_observed_week_starts_on_a_whole_week() {
        for week in OBSERVED_WEEKS {
            assert_eq!(
                (week.start_unix_s - CADENCE_ANCHOR_UNIX_S) % WEEK_SECONDS,
                0
            );
        }
    }

    #[test]
    fn a_later_week_keeps_the_observed_rules() {
        let catalog = catalog();
        let mut previous = catalog.week_at(LAST_OBSERVED_DAY);
        for weeks in 1..=12 {
            let week = catalog.week_at(LAST_OBSERVED_DAY + weeks * WEEK_SECONDS);
            assert_eq!(week.start_unix_s, previous.start_unix_s + WEEK_SECONDS);
            assert_eq!(week.sets.len(), SETS_PER_WEEK);
            let set = &week.sets[0];
            let before = &previous.sets[0];
            assert_ne!(set.elemental, before.elemental);
            assert_ne!(set.curse, before.curse);
            let mut modes: Vec<&str> = week.sets.iter().map(|set| set.game_mode.as_str()).collect();
            modes.dedup();
            assert_eq!(modes.len(), SETS_PER_WEEK);
            assert!(
                week.sets
                    .iter()
                    .all(|other| other.elemental == set.elemental)
            );
            // The same week again is the same pick.
            assert_eq!(
                catalog.week_at(LAST_OBSERVED_DAY + weeks * WEEK_SECONDS),
                week
            );
            previous = week;
        }
    }

    #[test]
    fn a_boss_is_scaled_by_its_potencies_and_gets_its_buckets() {
        let catalog = catalog();
        let mutation = catalog.mob_mutation(&context(3), Some("DungeonBoss"), &mut first);
        // Health potency 220 of +1 % each, damage potency 160.
        assert!((mutation.health_factor - 3.2).abs() < 1e-9);
        assert!((mutation.damage_factor - 2.6).abs() < 1e-9);
        let ids: Vec<&str> = mutation
            .effects
            .iter()
            .map(|effect| effect.status_id.as_str())
            .collect();
        // The boss bucket is not in this test's buckets; the promotion of tier 3 and both curse
        // parts (the major with its wildcard replaced) are.
        assert_eq!(
            ids,
            [
                "Mut_Enrage_Buff",
                "Mut_Curse2_Minor_Buff",
                "Mut_Curse2_Ice_Major_Buff"
            ]
        );
        assert!((mutation.effects[1].potency - 2.0).abs() < 1e-9);
    }

    #[test]
    fn an_or_bucket_takes_one_entry_and_an_and_bucket_all() {
        let catalog = catalog();
        let mutation = catalog.mob_mutation(&context(3), Some("Dungeon+"), &mut |count| count - 1);
        let ids: Vec<&str> = mutation
            .effects
            .iter()
            .map(|effect| effect.status_id.as_str())
            .collect();
        assert_eq!(ids[..2], ["Mut_Ice_Damage_Buff", "Mut_Ice_Promo2"]);
        assert!((mutation.health_factor - 3.5).abs() < 1e-9);
    }

    #[test]
    fn other_mobs_and_unknown_difficulties_are_unchanged() {
        let catalog = catalog();
        assert_eq!(
            catalog.mob_mutation(&context(3), Some("Solo"), &mut first),
            MobMutation::default()
        );
        assert_eq!(
            catalog.mob_mutation(&context(3), None, &mut first),
            MobMutation::default()
        );
        assert!(
            catalog
                .context("DungeonCutlassKeys00", LAST_OBSERVED_DAY, 9)
                .is_none()
        );
        assert!(
            catalog
                .context("DungeonEdengrove00", LAST_OBSERVED_DAY, 3)
                .is_none()
        );
    }

    #[test]
    fn the_loot_adds_the_mutated_tags_and_tables() {
        let catalog = catalog();
        let loot = catalog.loot(&context(3));
        assert_eq!(
            loot.tags,
            [
                "CutlassKeys00_Mut",
                "GypsumBlue",
                "MutDiff",
                "MutDiff1",
                "MutDiff2",
                "MutDiff3",
                "Ice"
            ]
        );
        assert_eq!(
            loot.creature_tables,
            ["MutatorLoot_Difficulty", "MutatorLoot_Ice"]
        );
        assert_eq!(loot.gear_score_range, Some((730, 730)));
        assert_eq!(loot.perk_buckets, ["PerkBucket_MutatorIce"]);
        assert_eq!(catalog.completion_event(&context(3)), Some("MutDiff3T1"));
        assert_eq!(
            catalog.player_affliction(&context(3)),
            Some("AfflictionCurse2")
        );
        assert!(catalog.is_mutable("dungeonshattermtn00"));
        assert_eq!(catalog.highest_difficulty(), Some(3));
    }
}
