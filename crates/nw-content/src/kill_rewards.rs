// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a mob kill gives its killer besides loot (progression note, section 7): character
//! experience and weapon mastery experience.
//!
//! The kill's raw experience ([`KillRewards::raw_experience`]) is `RewardData.KillExperience`
//! of the mob's level times the `RewardModifierData.Experience` of the mob's creature type
//! (`VitalsBaseData.CreatureType`, a level variant takes its base's) and one plus the
//! `LevelDisparityData.KillExpModifier` of the level gap, in 32-bit floats. The gap is the mob's
//! level minus the player's level as the client shows it (one-based). A mob whose
//! `NoExpNoCoin` is set gives none (Observed, content bundle 25810808: the tutorial ship's
//! sailors `Corrupted_Sailor_1H_Club_FTUE`; retail gave no experience for them and 20 for each
//! island foe, which are `Solo`).
//!
//! Observed over 2,213 retail kills of 258 open world ledgers (findings
//! `2026-11-13-kill-xp-rounding.md`):
//! - The gap uses the one-based player level: `Playthrough_Part_03` 5863.2 s, a level 28 mob
//!   killed at the shown level 38 got the -10 modifier (held weapon +92 = ceil(37 x 0.6) x 4,
//!   character +44 = floor(22.2) x 2).
//! - The arithmetic is 32-bit: `Playthrough_Part_02` 1505.5 s, 25 x 0.6 = 15.0000006 gave the
//!   held sword 16 points (x 4 = +64); `Playthrough_Part_01` 3894.2 s, raw 20 gave the other
//!   weapon 20 x 0.15 = 3.0 points (x 4 = +12).
//! - The character gains the raw experience rounded down ([`character_points`]), at least
//!   [`LEAST_KILL_POINTS`]: a `Critter` (`Experience` 0) gives 1 (`Turkey`, `Sheep_Churro_8`,
//!   `Explosive_Barrel_*`: +2 under multiplier 2), and newchar 2026-09-18 632 s raw 5.5 gave
//!   +10. The remote config's multiplier and the territory's experience share then apply with
//!   one rounding down (`XpMultipliers::character_gain`).
//!
//! The weapon that made the kill trains its mastery (`WeaponItemDefinitions.WeaponMasteryCategoryId`
//! of the item's `ItemStatsRef`) by the raw experience rounded up ([`held_weapon_points`]); the
//! other worn weapon trains its mastery by the raw experience times [`OTHER_WEAPON_SHARE`],
//! rounded up ([`other_weapon_points`]). Each gain is then multiplied by the remote config's
//! multiplier of its progression (Observed, retail newchar 2026-09-18: character +20, the
//! killing greatsword +20 and the worn blunderbuss +4 at a level 1 kill, multipliers 2; a kill of
//! raw 5.5 gave character +10, mastery +12 and +2).
//!
//! Each kill also fires a game event ([`KillRewards::kill_event`]): the mob's
//! `VitalsBaseData.KillEventIdOverride`, else `KillEvent`. Observed (retail 497 entries,
//! `MSQ5-lv25` and `CTF01`; content bundle 25810808):
//! - Coins: `RewardData.KillCurrency` of the mob's level times the creature type's
//!   `RewardModifierData.Currency`, rounded (`Solo` 470, `Solo+` 540, `Solo-` 235 of one level;
//!   `CTF01` 82.1 s 960 = level 66 times 1.0); the wallet rose by it (`MSQ5-lv25` 1758.286 s
//!   +470).
//! - Territory standing: the creature type's `TerritoryStanding` (`Solo` 5, `Solo+` 15, `Solo-`
//!   2), before the territory's multiplier (the standing rose by 15 for 5 under multiplier 3).
//! - Azoth: `RewardData.KillAzoth` times the creature type's `Azoth` (10 at level 25, 15 at level
//!   66) for an event row with an `AzothRewardChance` (`KillEvent`); 0 for one without
//!   (`CatacombsKillEvent_*`, every catacombs ledger). Inferred: it is given, as the coins and the
//!   standing are (no ledger shows the azoth wallet move: the players stood at its cap).
//! - The event row's `UmbralCurrency` and `ItemReward` (`CatacombsKillEvent_Fodder` 4 souls).
//! - A mob of `NoExpNoCoin` fires the event with nothing (Inferred from the 7 empty `KillEvent`
//!   entries of `MSQ5-lv25`).
//!
//! **Kill credit** ([`event_credit`], findings `2026-11-13-kill-xp-rounding.md`, section C8): a
//! kill that more players hit gives each credited player its rewards times the "Event Credit
//! Modifiers" entry of their count (`playerbaseattributes.pbadb` "Event Credit Data", Observed
//! values: two players 0.7 of the experience, 0.8 of the coins, 0.7 of the standing). The raw
//! experience takes the share in 32-bit floats before the points are cut, so the character and
//! both weapons gain less. Observed, `Playthrough_Part_04` 2543.7 s (raw 23.4): character +32 =
//! floor(16.38) x 2, held sword +68 = ceil(16.38) x 4, other weapon +12 = ceil(2.457) x 4, and
//! the entry's coins 480 and standing 3 (600 and 5 alone); `Playthrough_Part_08` 5204.5 s (raw
//! 69): +96, +196 = 49 x 4, +32; 22 entries of level 60 kills with 720 coins and standing 3
//! (900 and 5 alone). The credit's time (120 s), health share (0.01) and range (200 m) are the
//! same section's values; Inferred: a player is credited when it hit the mob in that time for
//! at least that share of its health and stands in that range at the kill.

use std::collections::{BTreeMap, HashMap, HashSet};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The share of a kill's raw experience that the worn weapon which did not make the kill gains.
/// Fitted to retail (Observed): 19 kills of newchar 2026-09-18 and levelling_weapon_up (raw 5.5,
/// 10, 11, 12, 22, 24, 26, 36, 73 gave 1, 2, 2, 2, 4, 4, 4, 6, 11 before the multiplier) fit
/// `ceil(raw x share)` for every share above 0.1389 and up to 0.1507; no table holds the value.
pub const OTHER_WEAPON_SHARE: f32 = 0.15;
/// The fewest character experience points of a kill of a mob with a reward (module note,
/// Observed: critters).
pub const LEAST_KILL_POINTS: u32 = 1;
/// The `NoExpNoCoin` cell of a mob that gives no reward.
const NO_REWARD: &str = "1";
/// The factor of a kill without a level gap modifier.
const PLAIN: f32 = 1.0;
/// The quantity of an item reward without an `ItemRewardQty`.
const ONE_ITEM: u32 = 1;

/// An `i64` table cell as a float (table amounts are far below 2^53).
#[allow(
    clippy::cast_precision_loss,
    reason = "table amounts are far below 2^53"
)]
fn amount(cell: i64) -> f64 {
    cell as f64
}

/// A table factor as the 32-bit float the kill arithmetic uses (module note).
#[allow(
    clippy::cast_possible_truncation,
    reason = "the kill arithmetic is 32-bit (module note); table factors are small"
)]
fn single(factor: f64) -> f32 {
    factor as f32
}

/// A kill experience amount as a 32-bit float (table amounts are far below 2^24).
#[allow(
    clippy::cast_precision_loss,
    reason = "table amounts are far below 2^24"
)]
fn single_amount(points: u32) -> f32 {
    points as f32
}

/// The reward kind of one mob type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MobKind {
    /// `CreatureType`.
    creature_type: Option<String>,
    /// Whether `NoExpNoCoin` is set.
    no_reward: bool,
    /// `KillEventIdOverride`: the game event of its kill in place of [`KILL_EVENT`].
    kill_event: Option<String>,
}

/// The game event of a kill of a mob without a `KillEventIdOverride` (Observed: retail 497
/// entries of open-world kills).
pub const KILL_EVENT: &str = "KillEvent";

/// The `RewardModifierData` factors of one creature type that a kill uses (an empty cell is 0).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CreatureMods {
    /// `Experience`.
    pub experience: f64,
    /// `Currency`.
    pub currency: f64,
    /// `TerritoryStanding`.
    pub standing: f64,
    /// `Azoth`.
    pub azoth: f64,
}

/// The kill amounts of one `RewardData` level.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KillLevel {
    /// `KillCurrency`.
    pub currency: f64,
    /// `KillAzoth`.
    pub azoth: f64,
}

/// What a kill's game event row gives besides the level amounts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KillEventRow {
    /// `ItemReward` and `ItemRewardQty` (a `[LTID]` loot table or an item; quantity 1 when empty).
    pub item: Option<(String, u32)>,
    /// `UmbralCurrency`.
    pub umbral: f64,
    /// Whether it has an `AzothRewardChance` (the kill's azoth goes only with one).
    pub azoth: bool,
}

/// The game event one kill fires and what it gives.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KillGameEvent {
    /// The event id.
    pub event: String,
    /// Coins (rounded).
    pub coins: u64,
    /// Territory standing before the territory's multiplier.
    pub standing: f64,
    /// Azoth.
    pub azoth: f64,
    /// Umbral shards.
    pub umbral: f64,
    /// The row's item reward.
    pub item: Option<(String, u32)>,
}

/// The rules of the kill rewards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KillRewards {
    /// The kind of each mob type, by the CRC-32 of its lowercase `VitalsID`.
    mobs: HashMap<u32, MobKind>,
    /// The `RewardModifierData` factors by lowercase `Modifiers` (the creature type).
    creature_mods: HashMap<String, CreatureMods>,
    /// The kill amounts by `RewardData` level.
    levels: HashMap<u32, KillLevel>,
    /// The kill game event rows by lowercase event id.
    events: HashMap<String, KillEventRow>,
    /// `LevelDisparityData.KillExpModifier` by `LevelDisparity`.
    disparity: BTreeMap<i64, f64>,
    /// The weapon mastery of each weapon item, by lowercase item id.
    weapon_masteries: HashMap<String, String>,
}

/// One `VitalsBaseData` row as [`KillRewards::build`] takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobRow<'a> {
    /// `VitalsID`.
    pub vitals_id: &'a str,
    /// `CreatureType`.
    pub creature_type: Option<&'a str>,
    /// `NoExpNoCoin`.
    pub no_exp_no_coin: Option<&'a str>,
    /// `KillEventIdOverride`.
    pub kill_event_override: Option<&'a str>,
}

impl KillRewards {
    /// The rules of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let mobs = tables
            .vitals_base_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| MobRow {
                vitals_id: &row.vitals_id,
                creature_type: row.creature_type.as_deref(),
                no_exp_no_coin: row.no_exp_no_coin.as_deref(),
                kill_event_override: row.kill_event_id_override.as_deref(),
            });
        let variants = tables
            .vitals_level_variant_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.vitals_id.as_str(), row.base_vitals_id.as_str()));
        let factor = |cell: Option<f64>| cell.unwrap_or_default();
        let creature_mods = tables
            .reward_modifier_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                let mods = CreatureMods {
                    experience: row.experience,
                    currency: factor(row.currency),
                    standing: factor(row.territory_standing),
                    azoth: factor(row.azoth),
                };
                (row.modifiers.as_str(), mods)
            });
        let disparity = tables
            .level_disparity_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.level_disparity, row.kill_exp_modifier));
        let weapon_masteries: HashMap<String, &str> = tables
            .weapon_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    row.weapon_id.to_lowercase(),
                    row.weapon_mastery_category_id.as_deref()?,
                ))
            })
            .collect();
        let items = tables
            .master_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let stats = row.item_stats_ref.as_deref()?.to_lowercase();
                Some((row.item_id.as_str(), *weapon_masteries.get(&stats)?))
            });
        let levels = tables
            .reward_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let level = KillLevel {
                    currency: row.kill_currency,
                    azoth: amount(row.kill_azoth),
                };
                Some((u32::try_from(row.level).ok()?, level))
            });
        let kill_events: HashSet<String> = tables
            .vitals_base_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| row.kill_event_id_override.as_deref())
            .chain([KILL_EVENT])
            .map(str::to_lowercase)
            .collect();
        let events = tables
            .game_event_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter(|row| kill_events.contains(&row.event_id.to_lowercase()))
            .map(|row| {
                let item = row
                    .item_reward
                    .clone()
                    .filter(|item| !item.is_empty())
                    .map(|item| {
                        let quantity = row
                            .item_reward_qty
                            .and_then(|quantity| u32::try_from(quantity).ok())
                            .unwrap_or(ONE_ITEM);
                        (item, quantity)
                    });
                let event = KillEventRow {
                    item,
                    umbral: row.umbral_currency.map_or(0.0, amount),
                    azoth: row.azoth_reward_chance.is_some(),
                };
                (row.event_id.as_str(), event)
            });
        Self::build(
            mobs,
            variants,
            creature_mods,
            disparity,
            items,
            levels,
            events,
        )
    }

    /// The rules of the mob rows `mobs`, the level variants `variants` (variant id, base id),
    /// the creature type factors `creature_mods`, the level gap modifiers `disparity` (gap,
    /// `KillExpModifier`), the weapon `items` (item id, mastery id), the kill amounts of each
    /// level `levels` and the kill game event rows `events` (event id, row). The first row of
    /// an id wins.
    pub fn build<'a>(
        mobs: impl IntoIterator<Item = MobRow<'a>>,
        variants: impl IntoIterator<Item = (&'a str, &'a str)>,
        creature_mods: impl IntoIterator<Item = (&'a str, CreatureMods)>,
        disparity: impl IntoIterator<Item = (i64, f64)>,
        items: impl IntoIterator<Item = (&'a str, &'a str)>,
        levels: impl IntoIterator<Item = (u32, KillLevel)>,
        events: impl IntoIterator<Item = (&'a str, KillEventRow)>,
    ) -> Self {
        let mut by_id: HashMap<String, MobKind> = HashMap::new();
        for row in mobs {
            by_id
                .entry(row.vitals_id.to_lowercase())
                .or_insert_with(|| MobKind {
                    creature_type: row.creature_type.map(str::to_owned),
                    no_reward: row
                        .no_exp_no_coin
                        .is_some_and(|cell| cell.trim() == NO_REWARD),
                    kill_event: row
                        .kill_event_override
                        .filter(|event| !event.is_empty())
                        .map(str::to_owned),
                });
        }
        let mut mobs: HashMap<u32, MobKind> = by_id
            .iter()
            .map(|(id, kind)| (lowercase_crc32(id), kind.clone()))
            .collect();
        for (variant, base) in variants {
            if let Some(kind) = by_id.get(&base.to_lowercase()) {
                mobs.entry(lowercase_crc32(variant))
                    .or_insert_with(|| kind.clone());
            }
        }
        let mut mods = HashMap::new();
        for (kind, factors) in creature_mods {
            mods.entry(kind.to_lowercase()).or_insert(factors);
        }
        let mut by_level = HashMap::new();
        for (level, amounts) in levels {
            by_level.entry(level).or_insert(amounts);
        }
        let mut event_rows = HashMap::new();
        for (event, row) in events {
            event_rows.entry(event.to_lowercase()).or_insert(row);
        }
        let mut gaps = BTreeMap::new();
        for (gap, modifier) in disparity {
            gaps.entry(gap).or_insert(modifier);
        }
        let mut weapons = HashMap::new();
        for (item, mastery) in items {
            weapons
                .entry(item.to_lowercase())
                .or_insert_with(|| mastery.to_owned());
        }
        Self {
            mobs,
            creature_mods: mods,
            levels: by_level,
            events: event_rows,
            disparity: gaps,
            weapon_masteries: weapons,
        }
    }

    /// The raw experience of a kill of the mob type `vitals_crc` of level `mob_level` by a
    /// player of the one-based level `player_level` (the level the client shows: the zero-based
    /// progression level plus one), whose `RewardData` kill experience is `base` (module note):
    /// `base` x the creature type's experience modifier x (1 + the level gap's modifier, the
    /// gap clamped to the table's rows), in 32-bit floats, never below 0. `None` for a mob
    /// without a reward, an unknown mob type or a creature type without a modifier row.
    pub fn raw_experience(
        &self,
        vitals_crc: u32,
        base: u32,
        mob_level: u32,
        player_level: u32,
    ) -> Option<f32> {
        let kind = self.mobs.get(&vitals_crc).filter(|kind| !kind.no_reward)?;
        let creature = kind
            .creature_type
            .as_ref()
            .and_then(|kind| self.creature_mods.get(&kind.to_lowercase()))?;
        let gap = i64::from(mob_level) - i64::from(player_level);
        let scaled = single_amount(base) * single(creature.experience);
        Some((scaled * (PLAIN + single(self.gap_modifier(gap)))).max(0.0))
    }

    /// The game event a kill of the mob type `vitals_crc` of level `mob_level` fires and what it
    /// gives (module note); `None` for an unknown mob type. A mob without a reward, a creature
    /// type without a factor row, a level without a `RewardData` row or an event without a row
    /// gives none of that part.
    pub fn kill_event(&self, vitals_crc: u32, mob_level: u32) -> Option<KillGameEvent> {
        let kind = self.mobs.get(&vitals_crc)?;
        let event = kind.kill_event.as_deref().unwrap_or(KILL_EVENT);
        let mut fired = KillGameEvent {
            event: event.to_owned(),
            ..KillGameEvent::default()
        };
        if kind.no_reward {
            return Some(fired);
        }
        let row = self.events.get(&event.to_lowercase());
        let mods = kind
            .creature_type
            .as_ref()
            .and_then(|kind| self.creature_mods.get(&kind.to_lowercase()))
            .copied()
            .unwrap_or_default();
        let level = self.levels.get(&mob_level).copied().unwrap_or_default();
        fired.coins = whole_coins(level.currency * mods.currency);
        fired.standing = mods.standing;
        if row.is_some_and(|row| row.azoth) {
            fired.azoth = level.azoth * mods.azoth;
        }
        if let Some(row) = row {
            fired.umbral = row.umbral;
            fired.item.clone_from(&row.item);
        }
        Some(fired)
    }

    /// The `CreatureType` of the mob type `vitals_crc` (a level variant has its base's); `None`
    /// for an unknown mob type or one without a type.
    pub fn creature_type(&self, vitals_crc: u32) -> Option<&str> {
        self.mobs.get(&vitals_crc)?.creature_type.as_deref()
    }

    /// The `KillExpModifier` of level gap `gap`, clamped to the table's rows; 0 between rows
    /// or without a table.
    fn gap_modifier(&self, gap: i64) -> f64 {
        let (Some((low, _)), Some((high, _))) = (
            self.disparity.first_key_value(),
            self.disparity.last_key_value(),
        ) else {
            return 0.0;
        };
        let gap = gap.clamp(*low, *high);
        self.disparity.get(&gap).copied().unwrap_or_default()
    }

    /// The weapon mastery (`CategoricalProgressionId`) that item `item_id` trains; `None` for
    /// an item that is no player weapon. Case does not matter.
    pub fn weapon_mastery(&self, item_id: &str) -> Option<&str> {
        self.weapon_masteries
            .get(&item_id.to_lowercase())
            .map(String::as_str)
    }
}

/// The coins of `raw`, rounded to the nearest; 0 for none.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to the target range first"
)]
fn whole_coins(raw: f64) -> u64 {
    raw.round().clamp(0.0, u64::MAX as f64) as u64
}

/// The character experience points of a kill of raw experience `raw` (module note): `raw`
/// rounded down, at least [`LEAST_KILL_POINTS`].
pub fn character_points(raw: f32) -> u32 {
    whole(raw.floor()).max(LEAST_KILL_POINTS)
}

/// The mastery points of the weapon that made a kill of raw experience `raw`: `raw` rounded up.
pub fn held_weapon_points(raw: f32) -> u32 {
    whole(raw.ceil())
}

/// The mastery points of another worn weapon at a kill of raw experience `raw`: `raw` x
/// [`OTHER_WEAPON_SHARE`] in 32-bit floats, rounded up.
pub fn other_weapon_points(raw: f32) -> u32 {
    whole((raw * OTHER_WEAPON_SHARE).ceil())
}

/// The whole points of the whole number `points`; 0 for none, [`u32::MAX`] above it.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to the target range first"
)]
fn whole(points: f32) -> u32 {
    if points >= single_amount(u32::MAX) {
        u32::MAX
    } else if points > 0.0 {
        points as u32
    } else {
        0
    }
}

/// "Credit Time Limit Seconds" (`playerbaseattributes.pbadb` "Event Credit Data"): how long
/// after its last hit a player keeps its credit for a kill, ms.
pub const CREDIT_TIME_LIMIT_MS: u64 = 120_000;
/// "Credit Health Threshold Percentage": the share of the mob's health a player's hits must
/// take for its credit.
pub const CREDIT_HEALTH_SHARE: f32 = 0.01;
/// "Credit Range Limit Meters": how far from the mob a credited player may stand at the kill.
pub const CREDIT_RANGE_M: f32 = 200.0;

/// The share of a kill's rewards that each credited player gets ("Event Credit Modifiers"; its
/// "Loot Modifier" is 1.0 for every count).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CreditModifier {
    /// "Xp Modifier": of the raw experience.
    pub experience: f32,
    /// "Currency Modifier": of the coins.
    pub currency: f32,
    /// "Territory Standing Modifier": of the territory standing.
    pub standing: f32,
}

/// "Event Credit Modifiers", one entry for each count of credited players from one; a larger
/// count takes the last.
pub const EVENT_CREDIT_MODIFIERS: [CreditModifier; 10] = [
    CreditModifier {
        experience: 1.0,
        currency: 1.0,
        standing: 1.0,
    },
    CreditModifier {
        experience: 0.7,
        currency: 0.8,
        standing: 0.7,
    },
    CreditModifier {
        experience: 0.55,
        currency: 0.6,
        standing: 0.55,
    },
    CreditModifier {
        experience: 0.45,
        currency: 0.4,
        standing: 0.45,
    },
    CreditModifier {
        experience: 0.4,
        currency: 0.2,
        standing: 0.4,
    },
    CreditModifier {
        experience: 0.35,
        currency: 0.1,
        standing: 0.35,
    },
    CreditModifier {
        experience: 0.3,
        currency: 0.1,
        standing: 0.3,
    },
    CreditModifier {
        experience: 0.25,
        currency: 0.1,
        standing: 0.25,
    },
    CreditModifier {
        experience: 0.2,
        currency: 0.1,
        standing: 0.2,
    },
    CreditModifier {
        experience: 0.15,
        currency: 0.1,
        standing: 0.15,
    },
];

/// The modifier of a kill with `credited` credited players (module note); a kill always credits
/// its killer, so 0 counts as one.
pub fn event_credit(credited: usize) -> CreditModifier {
    EVENT_CREDIT_MODIFIERS[credited
        .saturating_sub(1)
        .min(EVENT_CREDIT_MODIFIERS.len() - 1)]
}

impl CreditModifier {
    /// The raw experience `raw` of one credited player, in 32-bit floats (module note).
    pub fn raw_experience(&self, raw: f32) -> f32 {
        raw * self.experience
    }

    /// The game event `fired` of one credited player: its coins times the currency share,
    /// rounded, and its standing times the standing share in 32-bit floats, rounded down
    /// (Observed: 900 coins gave 720 and standing 5 gave 3; module note). The rest is whole.
    pub fn event(&self, fired: &KillGameEvent) -> KillGameEvent {
        KillGameEvent {
            coins: whole_coins(amount_of_coins(fired.coins) * f64::from(self.currency)),
            standing: f64::from((single(fired.standing) * self.standing).floor()),
            ..fired.clone()
        }
    }
}

/// Coins as a float (a wallet holds far below 2^53).
#[allow(clippy::cast_precision_loss, reason = "a wallet holds far below 2^53")]
fn amount_of_coins(coins: u64) -> f64 {
    coins as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGION: &str = "Legion_Legionnaire_FTUE";
    const SAILOR: &str = "Corrupted_Sailor_1H_Club_FTUE";
    const VARIANT: &str = "Legion_Legionnaire_FTUE_L3";
    const ELITE: &str = "Elite_Boar";
    /// Retail `RewardModifierData` rows (content bundle 25810808).
    const SOLO_MOD: f64 = 1.0;
    const ELITE_MOD: f64 = 3.0;
    const SOLO: CreatureMods = CreatureMods {
        experience: SOLO_MOD,
        currency: 1.0,
        standing: 5.0,
        azoth: 1.0,
    };
    const CATACOMBS: CreatureMods = CreatureMods {
        experience: 0.0,
        currency: 0.0,
        standing: 0.0,
        azoth: 1.0,
    };
    /// Retail `RewardData` kill amounts of levels 25 and 66.
    const LEVEL_25: u32 = 25;
    const LEVEL_25_KILL: KillLevel = KillLevel {
        currency: 550.0,
        azoth: 10.0,
    };
    const LEVEL_66: u32 = 66;
    const LEVEL_66_KILL: KillLevel = KillLevel {
        currency: 960.000_000_000_000_1,
        azoth: 15.0,
    };
    const CATACOMB_FODDER: &str = "Catacombs_Fodder_Test";
    const FODDER_EVENT: &str = "CatacombsKillEvent_Fodder";
    const SOULS: &str = "[LTID]Catacombs_Currency_Souls_Fodder";
    /// Retail `LevelDisparityData`: -20 and -19 to -10 cut kill experience.
    const GAPS: [(i64, f64); 3] = [(-20, -0.8), (-10, -0.4), (3, 0.0)];
    const SWORD_ITEM: &str = "2hGreatSword_StarterT1";
    const SWORD_MASTERY: &str = "GreatSwordAbilityTable";

    fn rewards() -> KillRewards {
        KillRewards::build(
            [
                MobRow {
                    vitals_id: LEGION,
                    creature_type: Some("Solo"),
                    no_exp_no_coin: None,
                    kill_event_override: None,
                },
                MobRow {
                    vitals_id: SAILOR,
                    creature_type: Some("Solo_StarterBeach"),
                    no_exp_no_coin: Some("1"),
                    kill_event_override: None,
                },
                MobRow {
                    vitals_id: CATACOMB_FODDER,
                    creature_type: Some("Catacombs"),
                    no_exp_no_coin: None,
                    kill_event_override: Some(FODDER_EVENT),
                },
                MobRow {
                    vitals_id: ELITE,
                    creature_type: Some("Elite"),
                    no_exp_no_coin: None,
                    kill_event_override: None,
                },
                MobRow {
                    vitals_id: "Unmodified",
                    creature_type: Some("NoRow"),
                    no_exp_no_coin: None,
                    kill_event_override: None,
                },
            ],
            [(VARIANT, LEGION), ("Orphan", "Missing")],
            [
                ("Solo", SOLO),
                ("Elite", mods(ELITE_MOD)),
                ("Catacombs", CATACOMBS),
            ],
            GAPS,
            [(SWORD_ITEM, SWORD_MASTERY)],
            [(LEVEL_25, LEVEL_25_KILL), (LEVEL_66, LEVEL_66_KILL)],
            [
                (
                    KILL_EVENT,
                    KillEventRow {
                        azoth: true,
                        ..KillEventRow::default()
                    },
                ),
                (
                    FODDER_EVENT,
                    KillEventRow {
                        item: Some((SOULS.to_owned(), 1)),
                        ..KillEventRow::default()
                    },
                ),
            ],
        )
    }

    fn mods(experience: f64) -> CreatureMods {
        CreatureMods {
            experience,
            ..CreatureMods::default()
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// `RewardData` kill experience of a level 1 and a level 16 mob.
    const BASE_1: u32 = 10;
    const BASE_16: u32 = 25;

    fn raw(rewards: &KillRewards, vitals: &str, base: u32, mob: u32, player: u32) -> Option<f32> {
        rewards.raw_experience(lowercase_crc32(vitals), base, mob, player)
    }

    #[test]
    fn the_creature_type_and_the_level_gap_scale_a_kill() {
        let rewards = rewards();
        assert_eq!(raw(&rewards, LEGION, BASE_1, 1, 1), Some(10.0));
        assert_eq!(raw(&rewards, VARIANT, BASE_1, 3, 3), Some(10.0));
        assert_eq!(raw(&rewards, ELITE, BASE_1, 5, 5), Some(30.0));
        // Retail: the ship's sailors give nothing; a type without a row or an unknown one
        // neither.
        assert_eq!(raw(&rewards, SAILOR, BASE_1, 1, 1), None);
        assert_eq!(raw(&rewards, "Unmodified", BASE_1, 1, 1), None);
        assert_eq!(raw(&rewards, "Orphan", BASE_1, 1, 1), None);
        // A critter has a row of experience 0: it gives a raw of 0 (and so the least points).
        assert_eq!(raw(&rewards, CATACOMB_FODDER, BASE_1, 1, 1), Some(0.0));
        // A mob 10 levels below the player's shown level gives 60 %, 30 below (clamped to
        // -20) 20 %; above the player it gives the whole.
        let near =
            |kill: Option<f32>, want: f32| kill.is_some_and(|kill| (kill - want).abs() < 1e-5);
        assert!(near(raw(&rewards, LEGION, BASE_1, 1, 11), 6.0));
        assert!(near(raw(&rewards, LEGION, BASE_1, 1, 31), 2.0));
        assert_eq!(raw(&rewards, LEGION, BASE_1, 40, 1), Some(10.0));
    }

    #[test]
    fn the_gap_is_to_the_shown_level_and_the_arithmetic_is_32_bit() {
        let rewards = rewards();
        // Retail `Playthrough_Part_02` 1505.5 s: a level 16 `Solo` (base 25) 10 or more levels
        // below the shown level (the fixture's row -10) is 25 x 0.6 in f32, above 15: the held
        // sword gained 16 (x 4 = +64), the other weapon ceil(2.25) = 3 (+12), the character
        // floor = 15.
        let kill = raw(&rewards, LEGION, BASE_16, 16, 26).expect("a reward");
        assert!(kill > 15.0);
        assert_eq!(held_weapon_points(kill), 16);
        assert_eq!(other_weapon_points(kill), 3);
        assert_eq!(character_points(kill), 15);
        // The shown level 25 is a gap of -9: no modifier.
        assert_eq!(raw(&rewards, LEGION, BASE_16, 16, 25), Some(25.0));
    }

    #[test]
    fn a_kill_fires_its_event_with_the_level_amounts_of_its_creature_type() {
        let rewards = rewards();
        // Retail `CTF01` 82.1 s: a level 66 `Solo` kill, `KillEvent` 960 coins, azoth 15.
        let open_world = rewards
            .kill_event(lowercase_crc32(LEGION), LEVEL_66)
            .expect("a known mob");
        assert_eq!(open_world.event, KILL_EVENT);
        assert_eq!(open_world.coins, 960);
        assert!(close(open_world.standing, 5.0));
        assert!(close(open_world.azoth, 15.0));
        // A level variant takes its base's event; a level without a row gives no coins.
        let variant = rewards
            .kill_event(lowercase_crc32(VARIANT), LEVEL_25)
            .expect("a variant");
        assert_eq!(variant.coins, 550);
        assert!(close(variant.azoth, 10.0));
        let unknown_level = rewards
            .kill_event(lowercase_crc32(LEGION), 1)
            .expect("a known mob");
        assert_eq!(unknown_level.coins, 0);
        // Retail catacombs: the override event, its souls, no coins, standing or azoth.
        let fodder = rewards
            .kill_event(lowercase_crc32(CATACOMB_FODDER), LEVEL_66)
            .expect("a catacomb mob");
        assert_eq!(fodder.event, FODDER_EVENT);
        assert_eq!(fodder.item, Some((SOULS.to_owned(), 1)));
        assert_eq!(fodder.coins, 0);
        assert!(close(fodder.azoth, 0.0) && close(fodder.standing, 0.0));
        // A mob without a reward fires the plain event with nothing; an unknown one none.
        let sailor = rewards
            .kill_event(lowercase_crc32(SAILOR), LEVEL_25)
            .expect("a known mob");
        assert_eq!(
            sailor,
            KillGameEvent {
                event: KILL_EVENT.to_owned(),
                ..KillGameEvent::default()
            }
        );
        assert_eq!(rewards.kill_event(lowercase_crc32("Orphan"), 1), None);
    }

    #[test]
    fn a_mob_type_and_its_level_variant_name_their_creature_type() {
        let rewards = rewards();
        assert_eq!(rewards.creature_type(lowercase_crc32(ELITE)), Some("Elite"));
        assert_eq!(
            rewards.creature_type(lowercase_crc32(VARIANT)),
            Some("Solo")
        );
        assert_eq!(rewards.creature_type(lowercase_crc32("Orphan")), None);
    }

    #[test]
    fn weapons_name_their_mastery_and_gains_round_as_retail() {
        let rewards = rewards();
        assert_eq!(
            rewards.weapon_mastery("2HGREATSWORD_STARTERT1"),
            Some(SWORD_MASTERY)
        );
        assert_eq!(rewards.weapon_mastery("Flint"), None);
        // Retail raw 5.5: character 5, mastery 6, the other weapon 1 (before the multiplier).
        assert_eq!(character_points(5.5), 5);
        assert_eq!(held_weapon_points(5.5), 6);
        assert_eq!(other_weapon_points(5.5), 1);
        // Retail `Playthrough_Part_01` 3894.2 s: raw 20 gave the other weapon 3 (f32 20 x 0.15
        // is 3.0; f64 is above 3).
        for (raw, other) in [
            (10.0, 2),
            (20.0, 3),
            (22.0, 4),
            (24.0, 4),
            (36.0, 6),
            (73.0, 11),
        ] {
            assert_eq!(other_weapon_points(raw), other, "raw {raw}");
        }
        // A critter's raw 0 gives the least character points and no mastery.
        assert_eq!(character_points(0.0), LEAST_KILL_POINTS);
        assert_eq!(held_weapon_points(0.0), 0);
        assert_eq!(held_weapon_points(-1.0), 0);
        assert_eq!(held_weapon_points(f32::MAX), u32::MAX);
    }

    #[test]
    fn a_kill_of_two_players_gives_each_the_second_credit_entry() {
        // Retail `Playthrough_Part_04` 2543.7 s: raw 23.4 hit by two players.
        let two = event_credit(2);
        let raw = two.raw_experience(23.4);
        assert_eq!(character_points(raw), 16);
        assert_eq!(held_weapon_points(raw), 17);
        assert_eq!(other_weapon_points(raw), 3);
        // `Playthrough_Part_08` 5204.5 s: raw 69 gave +96, +196 and +32 under multipliers 2, 4.
        let raw = two.raw_experience(69.0);
        assert_eq!(
            (
                character_points(raw),
                held_weapon_points(raw),
                other_weapon_points(raw)
            ),
            (48, 49, 8)
        );
        // A level 60 `Solo` kill alone: 900 coins and 5 standing; shared: 720 and 3.
        let alone = KillGameEvent {
            event: KILL_EVENT.to_owned(),
            coins: 900,
            standing: 5.0,
            azoth: 15.0,
            ..KillGameEvent::default()
        };
        assert_eq!(
            two.event(&alone),
            KillGameEvent {
                coins: 720,
                standing: 3.0,
                ..alone.clone()
            }
        );
        assert_eq!(event_credit(1).event(&alone), alone);
    }

    #[test]
    fn the_credit_entries_follow_the_count_and_stop_at_the_last() {
        assert_eq!(event_credit(0), EVENT_CREDIT_MODIFIERS[0]);
        assert_eq!(event_credit(1).experience, 1.0);
        assert_eq!(event_credit(3).experience, 0.55);
        assert_eq!(event_credit(10).experience, 0.15);
        assert_eq!(event_credit(25), EVENT_CREDIT_MODIFIERS[9]);
    }
}
