// SPDX-License-Identifier: AGPL-3.0-or-later
//! The combat rows a `sim` keeps (M6-A spec section 3). [`CombatSource`] is the plain content of
//! the datasheets that combat reads; [`CombatData`] is what a `sim` looks up during a tick.
//!
//! A mob type is found by the crc32 of its lower-case vitals id, as the world file stores it: in
//! `VitalsBaseData` first, then `VitalsData` (no mods: 1.0), then `VitalsLevelVariantData`, whose
//! row names the base type it varies. A type's mods are its own times the category mods of its
//! creature type (`VitalsModifierData`, for example `Solo+`: health 2, damage 1.2).

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;
use thiserror::Error;

use crate::Tables;
use crate::character_points::{ATTRIBUTE_COUNT, DEXTERITY, FOCUS, INTELLIGENCE, STRENGTH};
use crate::gameplay_config::ITEM_CLASS_SEPARATOR;
use crate::player_abilities::{AbilityList, PlayerAbilities, SpellRow};
use crate::player_attacks::{
    HitReaction, PlayerAttacks, ReactionFragment, ReactionKind, Stance, stance_key,
};
use crate::player_sequences::PlayerSequences;
use crate::status_effects::RowKey;

/// Separates the status effects of an `AddStatusEffects` cell.
const EFFECT_SEPARATOR: char = '+';
/// Health and damage mod of a type without them.
const PLAIN_MOD: f64 = 1.0;

/// The player row of `VitalsBaseData` (M6-A spec section 9).
pub const DEFAULT_PLAYER_VITALS_ID: &str = "Player";
/// The player sword of `WeaponItemDefinitions`.
pub const DEFAULT_WEAPON_ID: &str = "SwordT1";
/// The `RangedAttackProfile` of a weapon that fires spread shots.
const SPREAD_SHOT_PROFILE: &str = "Blunderbuss";
/// The player's damage table: the unique id of the datasheet that the player slice names
/// (`slices/player.dynamicslice` refers to `datatables/javelindata_damagetable`, whose unique id
/// is `DamageTable`; build 25810808). A damage row id is unique within one table only.
pub const DEFAULT_PLAYER_DAMAGE_TABLE: &str = "DamageTable";
/// The separator of a damage row's `Affixes` cell.
const AFFIX_SEPARATOR: char = ',';
/// A block stability is a percentage.
const PERCENT: f64 = 100.0;
/// The `BalanceCategory` of the open-world PvP balance rows that adjust a weapon class
/// (`javelindata_pvpbalance_openworld`).
const PVP_BALANCE_ITEM_CLASS: &str = "ItemClass";
/// A `TRUE` cell of a text column (`LevelDisparityData` `AdjustPowerLevel`).
const TRUE_CELL: &str = "TRUE";

/// The config rows a `sim` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatRows {
    /// The player row of `VitalsBaseData`.
    pub player_vitals_id: String,
    /// The player weapon row of `WeaponItemDefinitions`.
    pub weapon_id: String,
    /// The unique id of the player's damage table, where its attack rows are.
    pub damage_table: String,
}

impl Default for CombatRows {
    fn default() -> Self {
        Self {
            player_vitals_id: DEFAULT_PLAYER_VITALS_ID.to_owned(),
            weapon_id: DEFAULT_WEAPON_ID.to_owned(),
            damage_table: DEFAULT_PLAYER_DAMAGE_TABLE.to_owned(),
        }
    }
}

/// The `VitalsModifierData` mods of one creature type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CategoryMods {
    /// Health multiplier.
    pub health: f64,
    /// Damage multiplier.
    pub damage: f64,
}

/// A mob type.
#[derive(Debug, Clone, PartialEq)]
pub struct MobType {
    /// Multiplies the level's base health (`HealthMod`).
    pub health_mod: f64,
    /// Multiplies that health again: the creature type's `CategoryHealthMod` (each product
    /// loses its fraction, as on the retail cards).
    pub category_health_mod: f64,
    /// Multiplies the level's base damage (with the creature type's category mod).
    pub damage_mod: f64,
    /// The creature type, for the vision offset.
    pub creature_type: Option<String>,
    /// Its guard, for a type that can block.
    pub guard: Option<MobGuard>,
}

impl MobType {
    /// A type without mods.
    pub const PLAIN: Self = Self {
        health_mod: PLAIN_MOD,
        category_health_mod: PLAIN_MOD,
        damage_mod: PLAIN_MOD,
        creature_type: None,
        guard: None,
    };
}

/// The guard of a mob type that can block (`VitalsBaseData`; Observed in the rows of build
/// 25810808: the shield and guard pose kinds that retail showed blocking, `Damned_1H_Swordsman_*`,
/// `Lost_Knight_Tank_*`, `Dryad_Soldier_*`, `Invasion_Swordsman`, have `StaminaBaseMax` 50,
/// `StaminaBaseTickRate` 2, `StaminaTickDelay` 1, `BlockDamageMitigation` 1 and
/// `BlockStaminaCost` 0.8; the boar has `StaminaBaseMax` 1 and mitigation 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobGuard {
    /// `StaminaBaseMax`.
    pub stamina_max: f32,
    /// `StaminaInitial`.
    pub stamina_initial: f32,
    /// `StaminaBaseTickRate`: the first vitals state of the mob carries it as `StaminaTickRate`
    /// (Observed: a binary16 of it; retail `0x4000` (2) on the damned pistoleer's card, vitals id
    /// `0501e946`, beside `StaminaAmount` and `StaminaMax` 50; the player's 40 is `0x5100`).
    pub tick_rate: f32,
    /// Stamina back per second: `StaminaBaseTickRate` per `StaminaTickDelay` seconds, times
    /// [`MOB_REGEN_SCALE`] (**Inferred**, fitted: 3.0 to 3.3 per second for the common guard's
    /// 2 per 1 put 71 of 81 retail fresh-guard sequences right, 5 per second 69, 2 per second
    /// 69; no retail state carries a mob's stamina after its card).
    pub regen_per_s: f32,
    /// `BlockDamageMitigation`: the share of a blocked hit it stops (1: all; retail sent 3,315
    /// of 4,586 direct hits on a raised mob block as `Resisted` with 0 damage).
    pub mitigation: f32,
    /// `BlockStaminaCost`: multiplies the stamina a blocked hit takes (Observed: the weapon's
    /// `BlockStaminaDamage` x the row's `BlockStaminaDmgMod` x this 0.8 put 71 of 81 retail
    /// fresh-guard sequences of 45 ledgers right; the weapon's value with any factor and no row,
    /// at most 38).
    pub stamina_cost: f32,
}

/// A mob guard's regen per second for each `StaminaBaseTickRate` per `StaminaTickDelay`
/// second ([`MobGuard::regen_per_s`]; **Inferred**: the fitted 3.0 to 3.3 per second for 2 per
/// 1 is 1.5 to 1.65 times the row's value; this is the low end of that range).
const MOB_REGEN_SCALE: f32 = 1.5;
/// A guard needs more stamina than this to block at all (the boar's `StaminaBaseMax` 1).
const MIN_GUARD_STAMINA: f32 = 1.0;

impl MobGuard {
    /// The guard of the vitals cells (max, initial, tick rate, tick delay, mitigation, cost);
    /// `None` for a type whose stamina is too small or whose block stops nothing.
    pub fn of(
        (max, initial, tick_rate, tick_delay): (i64, i64, i64, i64),
        (mitigation, cost): (i64, f64),
    ) -> Option<Self> {
        // Stamina cells of a few dozen points.
        let stamina_max = max as f32;
        let delay = (tick_delay as f32).max(1.0);
        (stamina_max > MIN_GUARD_STAMINA && mitigation > 0).then(|| Self {
            stamina_max,
            stamina_initial: (initial as f32).clamp(0.0, stamina_max),
            tick_rate: tick_rate as f32,
            regen_per_s: tick_rate as f32 / delay * MOB_REGEN_SCALE,
            // `BlockDamageMitigation` is 0 or 1 in every row.
            mitigation: (mitigation as f32).clamp(0.0, 1.0),
            stamina_cost: cost as f32,
        })
    }
}

/// One level of `VitalsLevelData`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelRow {
    /// Base max health.
    pub base_max_health: f64,
    /// Base damage of one hit.
    pub base_damage: f64,
    /// Physical armor rating of a mob of this level.
    pub physical_armor_rating: f64,
    /// Elemental armor rating of a mob of this level.
    pub elemental_armor_rating: f64,
    /// `GearScore`: the gear score of a mob of this level, against which a player's armor
    /// mitigates its hits.
    pub gear_score: u32,
}

/// The armor of an armor item (`ArmorItemDefinitions`): its share of a full set's rating and the
/// scale of each kind (the set's physical and elemental scale factors).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmorRow {
    /// `ArmorRatingScaleFactor`: the item's share of a full set (the five heavy starter pieces:
    /// head 0.15, chest 0.35, hands 0.15, legs 0.2, feet 0.15).
    pub rating_share: f64,
    /// `PhysicalArmorSetScaleFactor`.
    pub physical_scale: f64,
    /// `ElementalArmorSetScaleFactor`.
    pub elemental_scale: f64,
}

/// Milliseconds per second.
const MS_PER_SECOND: f64 = 1_000.0;

/// The `Scaling*` cells `cells` (attribute id, cell) of a weapon row, by attribute id; an empty
/// cell is 0.
fn weapon_scaling(cells: [(u32, Option<f64>); 4]) -> [f64; ATTRIBUTE_COUNT] {
    let mut scaling = [0.0; ATTRIBUTE_COUNT];
    for (attribute, cell) in cells {
        scaling[attribute as usize] = cell.unwrap_or(0.0);
    }
    scaling
}

/// The player weapon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponRow {
    /// Base damage.
    pub base_damage: f64,
    /// Crit chance, 0 to 1.
    pub crit_chance: f64,
    /// Crit damage multiplier.
    pub crit_multiplier: f64,
    /// `ScalingStrength`, `ScalingDexterity`, `ScalingIntelligence` and `ScalingFocus`, by
    /// attribute id ([`crate::character_points::ATTRIBUTE_NAMES`]; Constitution scales no
    /// weapon): the share of its damage each point of the attribute's `ModifierValueSum` adds
    /// ([`crate::attribute_damage`]).
    pub scaling: [f64; ATTRIBUTE_COUNT],
    /// It fires spread shots (a blunderbuss): its attacks come as 2719 requests.
    pub spread_shot: bool,
    /// The shots it holds when loaded (`MaxLoadedAmmo`; the blunderbuss 2, melee weapons 0).
    pub max_loaded_ammo: u8,
    /// How long it takes to load again (`AutoReloadAmmoSeconds`; the blunderbuss 1.46 s).
    pub reload_ms: u64,
    /// The key of its stance: its `MannequinTag` ([`crate::player_attacks::stance_key`]).
    pub stance: u32,
    /// `BlockStaminaDamage`: the stamina its hits take from a blocker, before the attack's mod.
    pub block_stamina_damage: f64,
    /// `BlockStability`: the share (percent) of a blocked hit's stamina it takes off.
    pub block_stability: f64,
    /// How it blocks a projectile; `None` without `CanBlockRanged`: a projectile passes its
    /// block.
    pub ranged_block: Option<RangedBlock>,
    /// `BaseStaggerDamage`: the grit its hits take from a target whose grit is on, before the
    /// attack's `StaggerDmgModifier` ([`AttackRow::stagger`]); 0 for an empty cell.
    pub stagger_damage: f64,
}

/// How a weapon with `CanBlockRanged` blocks a projectile (build 25810808: shields 0 and 1,
/// swords 0.5 and 0.5, great axes 0.3 and 0.7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangedBlock {
    /// `RangedBlockHealthDamageScaling`: the share of the hit's health that passes the block
    /// (Inferred: 0 without the cell, as on the shields that have one).
    pub health_scale: f64,
    /// `RangedBlockStaminaDamageScaling`: the share of the block's stamina the hit takes
    /// (Inferred: 1 without the cell).
    pub stamina_scale: f64,
}

/// The health share of a ranged block without `RangedBlockHealthDamageScaling`.
const RANGED_BLOCK_HEALTH: f64 = 0.0;
/// The stamina share of a ranged block without `RangedBlockStaminaDamageScaling`.
const RANGED_BLOCK_STAMINA: f64 = 1.0;

/// A consumable that heals: a potion.
#[derive(Debug, Clone, PartialEq)]
pub struct HealingConsumable {
    /// `ConsumableItemDefinitions.CooldownDuration`, in seconds.
    pub cooldown_s: u32,
    /// `CooldownId`: consumables of one id share the cooldown.
    pub cooldown_id: String,
    /// The heal of its status effect (`StatusEffectData.Health`).
    pub health: f64,
    /// Its share of max health (`HealthModifierPercent`).
    pub health_share: f64,
    /// Its `MannequinTag`: the item use it plays (`Potion`); empty for none.
    pub use_tag: String,
}

/// One row of `ConsumableItemDefinitions` that has a cooldown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumableRow {
    /// `ConsumableID`.
    pub id: String,
    /// `CooldownDuration`, seconds.
    pub cooldown_s: u32,
    /// `CooldownId`.
    pub cooldown_id: String,
    /// `AddStatusEffects`.
    pub status_effects: String,
    /// `MannequinTag`; empty for none.
    pub use_tag: String,
}

/// The extra damage of an attack's affix (`AffixStatData`): its share of the hit and the
/// `DamageTypeData` `IntID` of its type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AffixDamage {
    /// `DamagePercentage`, 0 to 1.
    pub share: f64,
    /// The `IntID` of its `DamageType`.
    pub damage_type: u8,
}

/// The `Category` of a `DamageTypeData` row: which armor takes a hit of the type off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DamageCategory {
    /// `True` (`True`, `Falling`, `Brimstone`): no armor.
    True,
    /// `Physical` (`Standard`, `Slash`, `Thrust`, `Strike`, `PhysFire`, `Siege`).
    #[default]
    Physical,
    /// `Elemental` (`Arcane`, `Fire`, `Lightning`, `Corruption`, `Ice`, `Nature`, `Acid`).
    Elemental,
}

impl DamageCategory {
    /// The category of a `Category` cell.
    fn of(cell: &str) -> Self {
        match cell {
            "True" => Self::True,
            "Elemental" => Self::Elemental,
            _ => Self::Physical,
        }
    }
}

/// One attack of `DamageData`.
#[derive(Debug, Clone, PartialEq)]
pub struct AttackRow {
    /// Its row id.
    pub damage_id: String,
    /// Damage coefficient.
    pub coefficient: f64,
    /// Whether it can crit (an empty cell allows it).
    pub can_crit: bool,
    /// Threat it adds besides its damage.
    pub threat: u64,
    /// The `IntID` of its `DamageType` in `DamageTypeData`, when the row names a known one.
    pub damage_type: Option<u8>,
    /// The unique id of its table.
    pub table: String,
    /// `BlockStaminaDmgMod`: multiplies the attacker weapon's `BlockStaminaDamage` when a blocker
    /// takes the hit; 0 without a number.
    pub block_stamina_mod: f64,
    /// Whether a block stops it: not `Unblockable` and not `CanBeBlocked` false.
    pub blockable: bool,
    /// The `Category` of its damage type (physical when the row names none).
    pub category: DamageCategory,
    /// The extra damage of its first affix with one (`Affixes`, `AffixStatData`).
    pub affix: Option<AffixDamage>,
    /// A hit of it from behind the target is a crit (not `NoBackstab`; Observed: 510 of 560
    /// retail melee hits that could crit from behind a mob were crits, 857 of 3,772 from its
    /// front, 160 open world ledgers).
    pub backstab: bool,
    /// A ranged hit of it on the head is a head shot (not `NoHeadshot`).
    pub headshot: bool,
    /// The heart rune charge points its hit gives: to the attacker (`AttackRuneCharge`,
    /// `AttackBlockedRuneCharge` when blocked) and to the target (`HitRuneCharge`,
    /// `HitBlockedRuneCharge`); 0 without a number.
    pub rune_charge: RuneChargeRow,
    /// `PowerLevel`: how hard its hit shakes a player (the reaction table's levels); 0 for an
    /// empty cell. Observed (client `FUN_14692b9a0`): the client reads each power column as a
    /// pair, the base column and its `_AI` twin, and a creature's hit on a player takes the base
    /// column (the tutorial: `Damned_1H_Melee_*` rows of level 0 gave no reaction, the club and
    /// axe rows of levels 1 to 3 did).
    pub power_level: u32,
    /// `HitStun`: how long its hit stuns a player it shakes, in frames of 1/30 s
    /// ([`crate::player_attacks::ReactionEntry::stun_ms`]); 0 for an empty cell. A creature's
    /// hit takes the base column, as the power level.
    pub hit_stun: u32,
    /// `StaggerDmgModifier`: multiplies the attacker weapon's `BaseStaggerDamage` when it hits a
    /// target whose grit is on; 0 without a number (Observed cells: `0` to `40`, one bad
    /// `0.0.5`).
    pub stagger_mod: f64,
    /// `StunBreakoutIncrement`: what its hit adds to the reaction count of a player it shakes
    /// (Inferred from the name and `IfCanBreakReaction StunToBreakout`; Observed cells 0, 0.1,
    /// 0.25, 0.5 and 1); 0 for an empty cell. A creature's hit takes the base column, as the
    /// power level.
    pub stun_breakout: f64,
}

/// A `LevelDisparityData` row that changes the power level of a hit: an attacker this many
/// levels above its target turns a hit of `required` into one of `adjusted` that stuns for
/// `adjusted_hit_stun` (`AdjustPowerLevel`; Observed rows of build 25810808: 10 to 20 levels
/// above turn level 0 into level 1 with a stun of 20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerLevelShift {
    /// `LevelDisparity`: the attacker's level minus the target's.
    pub disparity: i64,
    /// `RequiredPowerLevel`.
    pub required: u32,
    /// `AdjustedPowerLevel`.
    pub adjusted: u32,
    /// `AdjustedHitStun`.
    pub adjusted_hit_stun: u32,
}

/// How a hit shakes its target: its power level and its stun in frames
/// ([`CombatData::incoming_hit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IncomingHit {
    /// The power level.
    pub power_level: u32,
    /// The stun, frames of 1/30 s.
    pub hit_stun: u32,
}

/// The heart rune charge points of a `DamageData` row.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RuneChargeRow {
    /// `AttackRuneCharge`.
    pub attack: f64,
    /// `AttackBlockedRuneCharge`.
    pub attack_blocked: f64,
    /// `HitRuneCharge`.
    pub hit: f64,
    /// `HitBlockedRuneCharge`.
    pub hit_blocked: f64,
}

impl AttackRow {
    /// The stamina it takes from a blocker of `block_stability` (percent) when the attacker's
    /// weapon is `weapon` (retail: 14.6 and 25.55 from the legionnaire's and Thorpe's stabs on a
    /// warhammer of stability 27, `100 x 0.2 x 0.73` and `125 x 0.28 x 0.73`).
    pub fn block_stamina(&self, weapon: &WeaponRow, block_stability: f64) -> f64 {
        weapon.block_stamina_damage * self.block_stamina_mod * (1.0 - block_stability / PERCENT)
    }

    /// The grit its hit takes from a target whose grit is on, by an attacker with `weapon`
    /// (Inferred from the column names: `BaseStaggerDamage` times `StaggerDmgModifier`; the
    /// client's `totalStaggerDamage` debug field, `FUN_146701570`, sums such hits).
    pub fn stagger(&self, weapon: &WeaponRow) -> f64 {
        weapon.stagger_damage * self.stagger_mod
    }
}

/// A spell a creature casts: the `SpellData` row a `CAGE-CastSpell` clip names by `SpellName`
/// (Observed: 744 of the 765 names of the creature clips have a row, most in
/// `javelindata_spelltable_ai`).
#[derive(Debug, Clone, PartialEq)]
pub struct MobSpell {
    /// Its row, without a projectile flight (the ranged attack database is not in the tables).
    pub row: SpellRow,
    /// It has a `RangedAttackProfile`: it flies from the creature to where it opens.
    pub flies: bool,
}

/// The plain content of the combat datasheets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CombatSource {
    /// `VitalsBaseData` rows by id.
    pub base_types: Vec<(String, MobType)>,
    /// `VitalsData` ids.
    pub plain_types: Vec<String>,
    /// `VitalsLevelVariantData`: (id, base id).
    pub variants: Vec<(String, String)>,
    /// `VitalsLevelData` by level.
    pub levels: Vec<(u32, LevelRow)>,
    /// `WeaponItemDefinitions` by id.
    pub weapons: Vec<(String, WeaponRow)>,
    /// `WeaponItemDefinitions`: (id, `RangedAttackProfile`) of the weapons that have a profile.
    pub ranged_profiles: Vec<(String, String)>,
    /// `MasterItemDefinitions`: (item id, `ItemStatsRef`) of the items that have stats.
    pub item_stats: Vec<(String, String)>,
    /// `WeaponItemDefinitions`: (id, `EquipAbility`) of the rows that grant an ability while
    /// worn (a heart rune's `Ability_Rune_Detonate`).
    pub equip_abilities: Vec<(String, String)>,
    /// `ArmorItemDefinitions` by id.
    pub armors: Vec<(String, ArmorRow)>,
    /// `SpellData`: every spell row, as a creature casts it.
    pub mob_spells: Vec<MobSpell>,
    /// `ConsumableItemDefinitions`: the consumables with a cooldown.
    pub consumables: Vec<ConsumableRow>,
    /// `StatusEffectData`: (status id, health, health share) of the effects that heal.
    pub heals: Vec<(String, f64, f64)>,
    /// `DamageData` rows.
    pub attacks: Vec<AttackRow>,
    /// `AITargetingData`: out-of-combat vision offset by creature type.
    pub vision_offsets: Vec<(String, f32)>,
    /// `VitalsBaseData` initial health by id (rows whose cell is a whole number).
    pub player_health: Vec<(String, u32)>,
    /// `VitalsModifierData` by category (creature type).
    pub categories: Vec<(String, CategoryMods)>,
    /// `MasterItemDefinitions`: (item id, `ItemClass` cell) of the items that have classes.
    pub item_classes: Vec<(String, String)>,
    /// The open-world PvP balance: (item class, `WeaponBaseDamageAdjustment`) of each weapon
    /// class row.
    pub pvp_weapon_adjustments: Vec<(String, f64)>,
    /// The `LevelDisparityData` rows that change a hit's power level.
    pub power_level_shifts: Vec<PowerLevelShift>,
}

impl CombatSource {
    /// The combat content of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let base_rows = || tables.vitals_base_data.iter().flat_map(|table| &table.rows);
        let damage_types: HashMap<&str, u8> = tables
            .damage_type_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| Some((row.type_id.as_str(), u8::try_from(row.int_id).ok()?)))
            .collect();
        let categories: HashMap<&str, DamageCategory> = tables
            .damage_type_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.type_id.as_str(), DamageCategory::of(&row.category)))
            .collect();
        let affixes: HashMap<&str, AffixDamage> = tables
            .affix_stat_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    row.status_id.as_str(),
                    AffixDamage {
                        share: row.damage_percentage?,
                        damage_type: *damage_types.get(row.damage_type.as_deref()?)?,
                    },
                ))
            })
            .collect();
        Self {
            base_types: base_rows()
                .map(|row| {
                    (
                        row.vitals_id.clone(),
                        MobType {
                            health_mod: row.health_mod.unwrap_or(PLAIN_MOD),
                            category_health_mod: PLAIN_MOD,
                            damage_mod: row.damage_mod.unwrap_or(PLAIN_MOD),
                            creature_type: row.creature_type.clone(),
                            guard: MobGuard::of(
                                (
                                    row.stamina_base_max.unwrap_or_default(),
                                    row.stamina_initial.unwrap_or_default(),
                                    row.stamina_base_tick_rate.unwrap_or_default(),
                                    row.stamina_tick_delay.unwrap_or_default(),
                                ),
                                (
                                    row.block_damage_mitigation.unwrap_or_default(),
                                    row.block_stamina_cost.unwrap_or_default(),
                                ),
                            ),
                        },
                    )
                })
                .collect(),
            plain_types: tables
                .vitals_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| row.vitals_id.clone())
                .collect(),
            variants: tables
                .vitals_level_variant_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| (row.vitals_id.clone(), row.base_vitals_id.clone()))
                .collect(),
            levels: tables
                .vitals_level_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let level = u32::try_from(row.level).ok()?;
                    Some((
                        level,
                        LevelRow {
                            base_max_health: row.base_max_health,
                            base_damage: row.base_damage,
                            physical_armor_rating: row.physical_armor_rating,
                            elemental_armor_rating: row.elemental_armor_rating,
                            gear_score: u32::try_from(row.gear_score).unwrap_or(0),
                        },
                    ))
                })
                .collect(),
            weapons: tables
                .weapon_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    (
                        row.weapon_id.clone(),
                        WeaponRow {
                            base_damage: row.base_damage.map_or(0.0, |damage| damage as f64),
                            crit_chance: row.crit_chance.unwrap_or(0.0),
                            crit_multiplier: row.crit_damage_multiplier.unwrap_or(PLAIN_MOD),
                            scaling: weapon_scaling([
                                (FOCUS, row.scaling_focus),
                                (STRENGTH, row.scaling_strength),
                                (DEXTERITY, row.scaling_dexterity),
                                (INTELLIGENCE, row.scaling_intelligence),
                            ]),
                            spread_shot: row.ranged_attack_profile.as_deref()
                                == Some(SPREAD_SHOT_PROFILE),
                            max_loaded_ammo: row
                                .max_loaded_ammo
                                .and_then(|ammo| u8::try_from(ammo).ok())
                                .unwrap_or(0),
                            // A reload of a few seconds.
                            reload_ms: row
                                .auto_reload_ammo_seconds
                                .map_or(0, |seconds| (seconds * MS_PER_SECOND).round() as u64),
                            stance: stance_key(row.mannequin_tag.as_deref().unwrap_or_default()),
                            block_stamina_damage: row
                                .block_stamina_damage
                                .map_or(0.0, |damage| damage as f64),
                            block_stability: row
                                .block_stability
                                .map_or(0.0, |stability| stability as f64),
                            stagger_damage: row
                                .base_stagger_damage
                                .map_or(0.0, |damage| damage as f64),
                            ranged_block: row.can_block_ranged.unwrap_or_default().then(|| {
                                RangedBlock {
                                    health_scale: row
                                        .ranged_block_health_damage_scaling
                                        .unwrap_or(RANGED_BLOCK_HEALTH),
                                    stamina_scale: row
                                        .ranged_block_stamina_damage_scaling
                                        .unwrap_or(RANGED_BLOCK_STAMINA),
                                }
                            }),
                        },
                    )
                })
                .collect(),
            ranged_profiles: tables
                .weapon_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| Some((row.weapon_id.clone(), row.ranged_attack_profile.clone()?)))
                .collect(),
            consumables: tables
                .consumable_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    Some(ConsumableRow {
                        id: row.consumable_id.clone(),
                        cooldown_s: u32::try_from(row.cooldown_duration?).ok()?,
                        cooldown_id: row.cooldown_id.clone()?,
                        status_effects: row.add_status_effects.clone()?,
                        use_tag: row.mannequin_tag.clone().unwrap_or_default(),
                    })
                })
                .collect(),
            heals: tables
                .status_effect_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let health = row.health?;
                    (health > 0).then(|| {
                        (
                            row.status_id.clone(),
                            health as f64,
                            row.health_modifier_percent.unwrap_or(0.0),
                        )
                    })
                })
                .collect(),
            armors: tables
                .armor_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    (
                        row.weapon_id.clone(),
                        ArmorRow {
                            rating_share: row.armor_rating_scale_factor.unwrap_or(0.0),
                            physical_scale: row.physical_armor_set_scale_factor.unwrap_or(0.0),
                            elemental_scale: row.elemental_armor_set_scale_factor.unwrap_or(0.0),
                        },
                    )
                })
                .collect(),
            mob_spells: tables
                .spell_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| MobSpell {
                    row: SpellRow::of_data(row, None, None, None),
                    flies: row.ranged_attack_profile.is_some(),
                })
                .collect(),
            equip_abilities: tables
                .weapon_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| Some((row.weapon_id.clone(), row.equip_ability.clone()?)))
                .collect(),
            item_stats: tables
                .master_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| Some((row.item_id.clone(), row.item_stats_ref.clone()?)))
                .collect(),
            attacks: tables
                .damage_data
                .iter()
                .flat_map(|table| table.rows.iter().map(move |row| (table, row)))
                .map(|(table, row)| AttackRow {
                    damage_id: row.damage_id.clone(),
                    coefficient: row.dmg_coef.unwrap_or(PLAIN_MOD),
                    can_crit: row.can_crit.unwrap_or(true),
                    threat: row
                        .add_threat
                        .and_then(|threat| u64::try_from(threat).ok())
                        .unwrap_or(0),
                    damage_type: row
                        .damage_type
                        .as_deref()
                        .and_then(|name| damage_types.get(name).copied()),
                    table: table.unique_id.clone(),
                    block_stamina_mod: row
                        .block_stamina_dmg_mod
                        .as_deref()
                        .and_then(|cell| cell.trim().parse().ok())
                        .unwrap_or(0.0),
                    // 145 rows are `Unblockable` (boss wipes, howls, spells) and 5 not
                    // `CanBeBlocked` (the admiral's cannon storm).
                    blockable: row.unblockable != Some(true) && row.can_be_blocked != Some(false),
                    category: row
                        .damage_type
                        .as_deref()
                        .and_then(|name| categories.get(name).copied())
                        .unwrap_or_default(),
                    affix: row.affixes.as_deref().and_then(|cell| {
                        cell.split(AFFIX_SEPARATOR)
                            .find_map(|affix| affixes.get(affix.trim()).copied())
                    }),
                    backstab: row.no_backstab != Some(true),
                    headshot: row.no_headshot != Some(true),
                    rune_charge: RuneChargeRow {
                        attack: row.attack_rune_charge.unwrap_or_default(),
                        attack_blocked: row.attack_blocked_rune_charge.unwrap_or_default(),
                        hit: row.hit_rune_charge.unwrap_or_default(),
                        hit_blocked: row.hit_blocked_rune_charge.unwrap_or_default(),
                    },
                    power_level: row
                        .power_level
                        .and_then(|level| u32::try_from(level).ok())
                        .unwrap_or(0),
                    hit_stun: row
                        .hit_stun
                        .and_then(|frames| u32::try_from(frames).ok())
                        .unwrap_or(0),
                    stun_breakout: row.stun_breakout_increment.unwrap_or(0.0),
                    stagger_mod: row
                        .stagger_dmg_modifier
                        .as_deref()
                        .and_then(|cell| cell.trim().parse().ok())
                        .unwrap_or(0.0),
                })
                .collect(),
            vision_offsets: tables
                .ai_targeting_data
                .iter()
                .flat_map(|table| &table.rows)
                // A vision offset is a few metres; f32 holds it exactly enough.
                .map(|row| {
                    (
                        row.creature_type.clone(),
                        row.out_of_combat_vision_distance_offset as f32,
                    )
                })
                .collect(),
            categories: tables
                .vitals_modifier_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    (
                        row.category_id.clone(),
                        CategoryMods {
                            health: row.category_health_mod,
                            damage: row.category_damage_mod,
                        },
                    )
                })
                .collect(),
            player_health: base_rows()
                .filter_map(|row| {
                    Some((
                        row.vitals_id.clone(),
                        row.health_initial.trim().parse().ok()?,
                    ))
                })
                .collect(),
            item_classes: tables
                .master_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| Some((row.item_id.clone(), row.item_class.clone()?)))
                .collect(),
            pvp_weapon_adjustments: tables
                .open_world_balance_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter(|row| row.balance_category == PVP_BALANCE_ITEM_CLASS)
                .filter_map(|row| {
                    Some((
                        row.balance_target.clone(),
                        row.weapon_base_damage_adjustment?,
                    ))
                })
                .collect(),
            power_level_shifts: tables
                .level_disparity_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter(|row| row.adjust_power_level.eq_ignore_ascii_case(TRUE_CELL))
                .filter_map(|row| {
                    Some(PowerLevelShift {
                        disparity: row.level_disparity,
                        required: u32::try_from(row.required_power_level).ok()?,
                        adjusted: u32::try_from(row.adjusted_power_level).ok()?,
                        adjusted_hit_stun: u32::try_from(row.adjusted_hit_stun).ok()?,
                    })
                })
                .collect(),
        }
    }
}

/// A config row that the datasheets do not have.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CombatDataError {
    /// No `VitalsBaseData` row with a whole initial health.
    #[error("no player vitals row `{0}`")]
    MissingPlayer(String),
    /// No `WeaponItemDefinitions` row.
    #[error("no weapon row `{0}`")]
    MissingWeapon(String),
    /// Two rows of one damage table whose ids differ only in case.
    #[error("damage table `{table}` has two rows `{damage_id}` (ids differ only in case)")]
    DuplicateAttackRow {
        /// The table's unique id.
        table: String,
        /// The row id.
        damage_id: String,
    },
}

/// What a `sim` looks up during a tick.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatData {
    mob_types: HashMap<u32, MobType>,
    levels: HashMap<u32, LevelRow>,
    weapon: WeaponRow,
    /// The weapon row of each item that is a weapon, by lower-case item id.
    item_weapons: HashMap<String, WeaponRow>,
    /// The armor row of each item that is armor, by lower-case item id.
    item_armors: HashMap<String, ArmorRow>,
    /// The ranged attack profile of each weapon item that has one, by lower-case item id.
    item_ranged_profiles: HashMap<String, String>,
    /// The healing consumable of each item that is one, by lower-case item id.
    item_heals: HashMap<String, HealingConsumable>,
    /// The open-world PvP damage factor of each weapon item whose class the balance table
    /// adjusts (1 + `WeaponBaseDamageAdjustment`), by lower-case item id.
    item_pvp_damage: HashMap<String, f64>,
    /// Every attack row, by lower-case table unique id and lower-case row id. The game data names
    /// rows in other cases than their tables do (the flail stance's `1h_Flail_LightAttack1` for
    /// the row `1H_Flail_LightAttack1`), and retail flail hits land, so a row id matches in any
    /// case.
    table_attacks: HashMap<(String, String), AttackRow>,
    /// The heart rune charge of every attack row, by its row key (a hit names its row so).
    rune_charges: HashMap<RowKey, RuneChargeRow>,
    /// The ability each item grants while worn (its stats row's `EquipAbility`), by lower-case
    /// item id.
    item_equip_abilities: HashMap<String, String>,
    /// The spells creatures cast, by lower-case spell id.
    mob_spells: HashMap<String, MobSpell>,
    /// The player's damage table (lower-case unique id).
    player_table: String,
    /// The basic attacks of each weapon stance.
    player_attacks: PlayerAttacks,
    /// The ability lists of the player grid.
    player_abilities: PlayerAbilities,
    /// The item classes of each weapon item (`ItemClass`, split), by lower-case item id.
    item_weapon_classes: HashMap<String, Vec<String>>,
    /// The slayer sequences the server starts for each weapon stance.
    player_sequences: PlayerSequences,
    vision_offsets: HashMap<String, f32>,
    player_base_health: u32,
    /// [`CombatSource::power_level_shifts`].
    power_level_shifts: Vec<PowerLevelShift>,
}

/// The slayer sequence a reaction plays ([`CombatData::reaction_sequence`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReactionSequence {
    /// `slayerSequenceId` (child 15).
    pub sequence: u64,
    /// Whether it is the sequence of the nearest mined entry, not the entry's own (Inferred).
    pub inferred: bool,
}

impl CombatData {
    /// The combat data of `tables` for the config `rows`.
    ///
    /// # Errors
    ///
    /// Returns [`CombatDataError`] when a config row is missing.
    pub fn from_tables(tables: &Tables, rows: &CombatRows) -> Result<Self, CombatDataError> {
        Self::build(CombatSource::from_tables(tables), rows)
    }

    /// The combat data of `source` for the config `rows`.
    ///
    /// # Errors
    ///
    /// Returns [`CombatDataError`] when a config row is missing.
    pub fn build(source: CombatSource, rows: &CombatRows) -> Result<Self, CombatDataError> {
        let player_base_health = source
            .player_health
            .iter()
            .find(|(id, _)| *id == rows.player_vitals_id)
            .map(|(_, health)| *health)
            .ok_or_else(|| CombatDataError::MissingPlayer(rows.player_vitals_id.clone()))?;
        let weapon = source
            .weapons
            .iter()
            .find(|(id, _)| *id == rows.weapon_id)
            .map(|(_, weapon)| *weapon)
            .ok_or_else(|| CombatDataError::MissingWeapon(rows.weapon_id.clone()))?;
        let mut table_attacks: HashMap<(String, String), AttackRow> = HashMap::new();
        for row in &source.attacks {
            let key = (row.table.to_lowercase(), row.damage_id.to_lowercase());
            if table_attacks.insert(key, row.clone()).is_some() {
                return Err(CombatDataError::DuplicateAttackRow {
                    table: row.table.clone(),
                    damage_id: row.damage_id.clone(),
                });
            }
        }
        let equip_abilities: HashMap<&str, &str> = source
            .equip_abilities
            .iter()
            .map(|(weapon, ability)| (weapon.as_str(), ability.as_str()))
            .collect();
        let item_equip_abilities = source
            .item_stats
            .iter()
            .filter_map(|(item, stats)| {
                Some((
                    item.to_lowercase(),
                    (*equip_abilities.get(stats.as_str())?).to_owned(),
                ))
            })
            .collect();
        let rune_charges = source
            .attacks
            .iter()
            .map(|row| (RowKey::of(&row.table, &row.damage_id), row.rune_charge))
            .collect();
        let player_table = rows.damage_table.to_lowercase();
        let weapons: HashMap<String, WeaponRow> = source
            .weapons
            .iter()
            .map(|(id, weapon)| (id.to_lowercase(), *weapon))
            .collect();
        let pvp_adjustments: HashMap<String, f64> = source
            .pvp_weapon_adjustments
            .iter()
            .map(|(class, adjustment)| (class.to_lowercase(), *adjustment))
            .collect();
        let item_pvp_damage = source
            .item_classes
            .iter()
            .filter_map(|(item, classes)| {
                let adjustment = classes
                    .split(ITEM_CLASS_SEPARATOR)
                    .find_map(|class| pvp_adjustments.get(&class.trim().to_lowercase()))?;
                Some((item.to_lowercase(), PLAIN_MOD + adjustment))
            })
            .collect();
        let item_weapons: HashMap<String, WeaponRow> = source
            .item_stats
            .iter()
            .filter_map(|(item, stats)| {
                Some((item.to_lowercase(), *weapons.get(&stats.to_lowercase())?))
            })
            .collect();
        let armors: HashMap<String, ArmorRow> = source
            .armors
            .iter()
            .map(|(id, armor)| (id.to_lowercase(), *armor))
            .collect();
        let item_armors = source
            .item_stats
            .iter()
            .filter_map(|(item, stats)| {
                Some((item.to_lowercase(), *armors.get(&stats.to_lowercase())?))
            })
            .collect();
        let item_weapon_classes = source
            .item_classes
            .iter()
            .filter(|(item, _)| item_weapons.contains_key(&item.to_lowercase()))
            .map(|(item, classes)| {
                (
                    item.to_lowercase(),
                    classes
                        .split(ITEM_CLASS_SEPARATOR)
                        .map(str::trim)
                        .filter(|class| !class.is_empty())
                        .map(str::to_owned)
                        .collect(),
                )
            })
            .collect();
        let profiles: HashMap<String, &String> = source
            .ranged_profiles
            .iter()
            .map(|(id, profile)| (id.to_lowercase(), profile))
            .collect();
        let item_ranged_profiles = source
            .item_stats
            .iter()
            .filter_map(|(item, stats)| {
                Some((
                    item.to_lowercase(),
                    (*profiles.get(&stats.to_lowercase())?).clone(),
                ))
            })
            .collect();
        let heals: HashMap<String, (f64, f64)> = source
            .heals
            .iter()
            .map(|(id, health, share)| (id.to_lowercase(), (*health, *share)))
            .collect();
        let consumables: HashMap<String, HealingConsumable> = source
            .consumables
            .iter()
            .filter_map(|row| {
                let (health, health_share) = row
                    .status_effects
                    .split(EFFECT_SEPARATOR)
                    .find_map(|effect| heals.get(&effect.trim().to_lowercase()))?;
                Some((
                    row.id.to_lowercase(),
                    HealingConsumable {
                        cooldown_s: row.cooldown_s,
                        cooldown_id: row.cooldown_id.clone(),
                        health: *health,
                        health_share: *health_share,
                        use_tag: row.use_tag.clone(),
                    },
                ))
            })
            .collect();
        // A consumable's id is its item id: every retail health potion has no `ItemStatsRef`
        // (live 2026-09-16: `PotionHealthT1` never healed). An item that names a consumable by
        // its stats is kept too; its own consumable row wins.
        let item_heals = source
            .item_stats
            .iter()
            .filter_map(|(item, stats)| {
                Some((
                    item.to_lowercase(),
                    consumables.get(&stats.to_lowercase())?.clone(),
                ))
            })
            .chain(consumables.clone())
            .collect();
        let categories: HashMap<String, CategoryMods> = source.categories.into_iter().collect();
        let base: HashMap<u32, MobType> = source
            .base_types
            .into_iter()
            .map(|(id, mut kind)| {
                if let Some(mods) = kind
                    .creature_type
                    .as_ref()
                    .and_then(|kind| categories.get(kind))
                {
                    kind.category_health_mod = mods.health;
                    kind.damage_mod *= mods.damage;
                }
                (lowercase_crc32(&id), kind)
            })
            .collect();
        let mut mob_types = base.clone();
        for id in source.plain_types {
            mob_types
                .entry(lowercase_crc32(&id))
                .or_insert(MobType::PLAIN);
        }
        for (id, base_id) in source.variants {
            let kind = base
                .get(&lowercase_crc32(&base_id))
                .cloned()
                .unwrap_or(MobType::PLAIN);
            mob_types.entry(lowercase_crc32(&id)).or_insert(kind);
        }
        Ok(Self {
            mob_types,
            levels: source.levels.into_iter().collect(),
            weapon,
            item_weapons,
            item_armors,
            item_ranged_profiles,
            item_heals,
            item_pvp_damage,
            table_attacks,
            rune_charges,
            item_equip_abilities,
            mob_spells: source
                .mob_spells
                .into_iter()
                .map(|spell| (spell.row.id.to_lowercase(), spell))
                .collect(),
            player_table,
            player_attacks: PlayerAttacks::default(),
            player_abilities: PlayerAbilities::default(),
            item_weapon_classes,
            player_sequences: PlayerSequences::default(),
            vision_offsets: source.vision_offsets.into_iter().collect(),
            player_base_health,
            power_level_shifts: source.power_level_shifts,
        })
    }

    /// How a hit of `row` shakes a target `disparity` levels below its attacker
    /// ([`PowerLevelShift`]): the shifted level and stun of the row of that disparity (the
    /// table's last row past its end, Inferred) when it shifts the row's level, else the row's.
    pub fn incoming_hit(&self, row: &AttackRow, disparity: i64) -> IncomingHit {
        let own = IncomingHit {
            power_level: row.power_level,
            hit_stun: row.hit_stun,
        };
        let Some(high) = self
            .power_level_shifts
            .iter()
            .map(|shift| shift.disparity)
            .max()
        else {
            return own;
        };
        let disparity = disparity.min(high);
        self.power_level_shifts
            .iter()
            .find(|shift| shift.disparity == disparity && shift.required == row.power_level)
            .map_or(own, |shift| IncomingHit {
                power_level: shift.adjusted,
                hit_stun: shift.adjusted_hit_stun,
            })
    }

    /// The mob type of a vitals id crc32.
    pub fn mob_type(&self, vitals_crc: u32) -> Option<&MobType> {
        self.mob_types.get(&vitals_crc)
    }

    /// The weapon row of item `item_id` (its `ItemStatsRef`); `None` for an item that is no
    /// weapon. Case does not matter.
    pub fn weapon_of_item(&self, item_id: &str) -> Option<&WeaponRow> {
        self.item_weapons.get(&item_id.to_lowercase())
    }

    /// The armor row of item `item_id` (its `ItemStatsRef`); `None` for an item that is no armor.
    /// Case does not matter.
    pub fn armor_of_item(&self, item_id: &str) -> Option<&ArmorRow> {
        self.item_armors.get(&item_id.to_lowercase())
    }

    /// The ranged attack profile (`RangedAttackProfile`) of weapon item `item_id`; `None` for an
    /// item that has none. Case does not matter.
    pub fn ranged_profile_of_item(&self, item_id: &str) -> Option<&str> {
        self.item_ranged_profiles
            .get(&item_id.to_lowercase())
            .map(String::as_str)
    }

    /// The open-world PvP damage factor of weapon item `item_id`: 1 plus its class's
    /// `WeaponBaseDamageAdjustment`, 1 for a class the balance table does not adjust. Case does
    /// not matter.
    pub fn pvp_damage_factor(&self, item_id: &str) -> f64 {
        self.item_pvp_damage
            .get(&item_id.to_lowercase())
            .copied()
            .unwrap_or(PLAIN_MOD)
    }

    /// The healing consumable of item `item_id`; `None` for an item that is none. Case does not
    /// matter.
    pub fn heal_of_item(&self, item_id: &str) -> Option<&HealingConsumable> {
        self.item_heals.get(&item_id.to_lowercase())
    }

    /// The row of `level`.
    pub fn level(&self, level: u32) -> Option<&LevelRow> {
        self.levels.get(&level)
    }

    /// Every level row.
    pub fn levels(&self) -> impl Iterator<Item = (u32, &LevelRow)> {
        self.levels.iter().map(|(level, row)| (*level, row))
    }

    /// The player weapon.
    pub const fn weapon(&self) -> &WeaponRow {
        &self.weapon
    }

    /// The player attack row `damage_id` of the player's damage table.
    pub fn attack(&self, damage_id: &str) -> Option<&AttackRow> {
        self.table_attack(&self.player_table, damage_id)
    }

    /// The attack row `damage_id` of the table of unique id `table` (both in any case).
    pub fn table_attack(&self, table: &str, damage_id: &str) -> Option<&AttackRow> {
        self.table_attacks
            .get(&(table.to_lowercase(), damage_id.to_lowercase()))
    }

    /// The heart rune charge of the attack row of key `row` (all 0 for a row it does not know).
    pub fn rune_charge(&self, row: RowKey) -> RuneChargeRow {
        self.rune_charges.get(&row).copied().unwrap_or_default()
    }

    /// This data with the basic attacks of the weapon stances, `attacks`.
    #[must_use]
    pub fn with_player_attacks(self, attacks: PlayerAttacks) -> Self {
        Self {
            player_attacks: attacks,
            ..self
        }
    }

    /// This data with the ability lists of the player grid, `abilities`.
    #[must_use]
    pub fn with_player_abilities(self, abilities: PlayerAbilities) -> Self {
        Self {
            player_abilities: abilities,
            ..self
        }
    }

    /// The CAGE input names of the input numbers `on` holds ([`PlayerAbilities::inputs_on`]).
    pub fn ability_inputs_on(&self, on: impl Fn(u32) -> bool) -> Vec<String> {
        self.player_abilities.inputs_on(on)
    }

    /// The ability lists that weapon item `item_id` turns on (any case), in grid order.
    pub fn ability_lists_of_item(&self, item_id: &str) -> Vec<&AbilityList> {
        self.item_weapon_classes
            .get(&item_id.to_lowercase())
            .map(|classes| {
                let classes: Vec<&str> = classes.iter().map(String::as_str).collect();
                self.player_abilities.lists_for(&classes)
            })
            .unwrap_or_default()
    }

    /// The item classes (`ItemClass`, split) of weapon item `item_id` (any case): a shield's
    /// too (`KiteShield`); empty for an item that is no weapon.
    pub fn weapon_item_classes(&self, item_id: &str) -> &[String] {
        self.item_weapon_classes
            .get(&item_id.to_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// The ability item `item_id` (any case) grants while worn (a heart rune's).
    pub fn equip_ability_of_item(&self, item_id: &str) -> Option<&str> {
        self.item_equip_abilities
            .get(&item_id.to_lowercase())
            .map(String::as_str)
    }

    /// The spell `name` (any case) a creature's `CAGE-CastSpell` clip casts.
    pub fn mob_spell(&self, name: &str) -> Option<&MobSpell> {
        self.mob_spells.get(&name.to_lowercase())
    }

    /// The heart rune's ability lists ([`crate::player_abilities::HEARTGEM_CLASS`]): on while a
    /// rune is in its slot, beside the lists of the weapon in hand.
    pub fn rune_lists(&self) -> Vec<&AbilityList> {
        self.player_abilities
            .lists_for(&[crate::player_abilities::HEARTGEM_CLASS])
    }

    /// The same data with the player sequence table `sequences`.
    #[must_use]
    pub fn with_player_sequences(self, sequences: PlayerSequences) -> Self {
        Self {
            player_sequences: sequences,
            ..self
        }
    }

    /// The player sequence table.
    pub const fn player_sequences(&self) -> &PlayerSequences {
        &self.player_sequences
    }

    /// The stance of key `key` ([`WeaponRow::stance`]).
    pub fn stance(&self, key: u32) -> Option<&Stance> {
        self.player_attacks.stance(key)
    }

    /// The slayer sequence of the reaction row `row` of stance `stance` (key `stance_key`): the
    /// mined sequence of its entry's animations (Observed), else the nearest entry with a mined
    /// sequence (Inferred, no retail play of the entry was mined). An upper-body row takes only
    /// an upper-body entry's sequence, a full-body row only a full-body one's. Nearest: of the
    /// same reaction kind (guard break, stagger, knockdown, upper body) first, then of the same
    /// stance, then of the nearest `R` level, then of the nearest side (quarter turns), then of
    /// the same variant, then the lowest sequence (so the pick does not depend on the table's
    /// order). `None` for a row without an entry or of no reaction fragment, or a table with no
    /// mined sequence of its body part.
    pub fn reaction_sequence(
        &self,
        stance_key: u32,
        stance: &Stance,
        row: &HitReaction,
    ) -> Option<ReactionSequence> {
        let entry = row.entry.as_ref()?;
        let wanted = ReactionFragment::parse(&row.fragment)?;
        let upper_body = |kind: ReactionKind| kind == ReactionKind::UpperBody;
        if let Some(sequence) = self.player_sequences.reaction(&entry.animations) {
            return Some(ReactionSequence {
                sequence,
                inferred: false,
            });
        }
        let distance = |key: u32, known: &HitReaction, sequence: u64| {
            let fragment = ReactionFragment::parse(&known.fragment)?;
            (upper_body(fragment.kind) == upper_body(wanted.kind)).then(|| {
                (
                    fragment.kind != wanted.kind,
                    key != stance_key,
                    fragment.level.abs_diff(wanted.level),
                    fragment.side_turns(&wanted),
                    fragment.variant != wanted.variant,
                    sequence,
                )
            })
        };
        std::iter::once((stance_key, stance))
            .chain(self.player_attacks.stances())
            .flat_map(|(key, stance)| stance.reactions.iter().map(move |known| (key, known)))
            .filter_map(|(key, known)| {
                let sequence = self
                    .player_sequences
                    .reaction(&known.entry.as_ref()?.animations)?;
                distance(key, known, sequence)
            })
            .min()
            .map(|(.., sequence)| ReactionSequence {
                sequence,
                inferred: true,
            })
    }

    /// The stance of a character with no weapon.
    pub fn unarmed_stance(&self) -> Option<&Stance> {
        self.stance(stance_key(""))
    }

    /// The out-of-combat vision offset of `creature_type`; 0 without a row.
    pub fn vision_offset(&self, creature_type: Option<&str>) -> f32 {
        creature_type
            .and_then(|kind| self.vision_offsets.get(kind))
            .copied()
            .unwrap_or(0.0)
    }

    /// The player's base health: its max at level 0 without Constitution.
    pub const fn player_base_health(&self) -> u32 {
        self.player_base_health
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;
    use nw_formats::datasheet::{Column, ColumnKind, Datasheet};

    /// A datasheet of `type_name` with `columns` and `rows`.
    fn sheet(type_name: &str, columns: &[(&str, ColumnKind)], rows: &[&[&str]]) -> Datasheet {
        Datasheet {
            type_name: type_name.to_owned(),
            unique_id: format!("{type_name}_1"),
            columns: columns
                .iter()
                .map(|(name, kind)| Column {
                    name: (*name).to_owned(),
                    kind: *kind,
                })
                .collect(),
            rows: rows
                .iter()
                .map(|row| row.iter().map(|value| (*value).to_owned()).collect())
                .collect(),
        }
    }

    #[test]
    fn an_attack_has_the_int_id_of_its_damage_type() {
        use ColumnKind::{Number, String as Text};
        // Retail `DamageTypeData` (build 25810808): Slash 4, Thrust 5.
        let types = sheet(
            "DamageTypeData",
            &[
                ("TypeID", Text),
                ("IntID", Number),
                ("Category", Text),
                ("DamageBonusAgainst", Text),
                ("DisplayName", Text),
                ("AfflictionFrac", Number),
                ("DamageBonusFrac", Number),
                ("DefenseBonusFrac", Number),
                ("MitigationBonusFrac", Number),
            ],
            &[
                &[
                    "Slash", "4", "Physical", "None", "Slash", "0", "0", "0", "0",
                ],
                &[
                    "Thrust", "5", "Physical", "None", "Thrust", "0", "0", "0", "0",
                ],
            ],
        );
        // Retail rows of `javelindata_damagetable_damned_commander_ftue`: the stab's phase-two row
        // has a quarter fire, its phase-one row none.
        let attacks = sheet(
            "DamageData",
            &[
                ("DamageID", Text),
                ("DamageType", Text),
                ("BlockStaminaDmgMod", Text),
                ("Affixes", Text),
            ],
            &[
                &["1H_Sword_Attack1", "Slash", "0.4", ""],
                &[
                    "1H_Sword_Attack2",
                    "Thrust",
                    "0.28000000000000003",
                    "AI_FireDMG_25",
                ],
                &["Unknown_Type", "Mystery", "", "No_Such_Affix"],
                &["No_Type", "", "x", ""],
            ],
        );
        let affixes = sheet(
            "AffixStatData",
            &[
                ("StatusID", Text),
                ("DamagePercentage", Number),
                ("DamageType", Text),
            ],
            &[&["AI_FireDMG_25", "0.25", "Thrust"]],
        );
        let tables = Tables {
            damage_type_data: vec![Table::fill("types", &types).expect("types")],
            damage_data: vec![Table::fill("attacks", &attacks).expect("attacks")],
            affix_stat_data: vec![Table::fill("affixes", &affixes).expect("affixes")],
            ..Tables::default()
        };
        let source = CombatSource::from_tables(&tables);
        let types: Vec<(&str, Option<u8>)> = source
            .attacks
            .iter()
            .map(|row| (row.damage_id.as_str(), row.damage_type))
            .collect();
        assert_eq!(
            types,
            [
                ("1H_Sword_Attack1", Some(4)),
                ("1H_Sword_Attack2", Some(5)),
                ("Unknown_Type", None),
                ("No_Type", None),
            ]
        );
        let stab = &source.attacks[1];
        assert_eq!(stab.table, "DamageData_1", "the sheet's unique id");
        assert!((stab.block_stamina_mod - 0.28).abs() < 1e-9);
        assert_eq!(
            stab.affix,
            Some(AffixDamage {
                share: 0.25,
                damage_type: 5
            })
        );
        assert_eq!(source.attacks[0].affix, None);
        assert_eq!(source.attacks[2].affix, None, "an affix without a row");
        assert_eq!(source.attacks[3].block_stamina_mod, 0.0, "not a number");
    }

    #[test]
    fn an_attacker_far_above_its_target_turns_a_level_0_hit_into_level_1() {
        // `LevelDisparityData` (build 25810808): 10 to 20 levels above, level 0 becomes 1; the
        // test table holds the row of 10.
        let data = data();
        let row = |power_level, hit_stun| AttackRow {
            power_level,
            hit_stun,
            ..source().attacks[0].clone()
        };
        let shifted = IncomingHit {
            power_level: 1,
            hit_stun: 20,
        };
        assert_eq!(data.incoming_hit(&row(0, 10), 10), shifted);
        assert_eq!(
            data.incoming_hit(&row(0, 10), 25),
            shifted,
            "past the last row"
        );
        let own = IncomingHit {
            power_level: 0,
            hit_stun: 10,
        };
        assert_eq!(data.incoming_hit(&row(0, 10), 9), own, "no row of 9");
        assert_eq!(data.incoming_hit(&row(0, 10), -30), own);
        assert_eq!(
            data.incoming_hit(&row(2, 18), 10),
            IncomingHit {
                power_level: 2,
                hit_stun: 18,
            },
            "the row shifts level 0 only"
        );
    }

    #[test]
    fn a_hit_takes_its_weapons_stagger_damage_times_its_rows_modifier() {
        let row = AttackRow {
            stagger_mod: 0.5,
            stun_breakout: 0.0,
            ..source().attacks[0].clone()
        };
        let weapon = WeaponRow {
            stagger_damage: 100.0,
            ..sword_row()
        };
        assert!((row.stagger(&weapon) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_blocked_hit_takes_the_retail_stamina() {
        // Retail tutorial, warhammer of `BlockStability` 27: the legionnaire's
        // `Legionnaire_LightStab` (0.2) with `Legion_1HSpear` (100) took 14.6, Thorpe's
        // `Attack_Stab_02` (0.28) with his sword (125) 25.55.
        let warhammer = 27.0;
        let row = |mod_: f64| AttackRow {
            block_stamina_mod: mod_,
            blockable: true,
            category: DamageCategory::Physical,
            ..source().attacks[0].clone()
        };
        let weapon = |damage: f64| WeaponRow {
            block_stamina_damage: damage,
            ..sword_row()
        };
        assert!((row(0.2).block_stamina(&weapon(100.0), warhammer) - 14.6).abs() < 1e-9);
        assert!((row(0.28).block_stamina(&weapon(125.0), warhammer) - 25.55).abs() < 1e-9);
    }

    const PLAYER_HEALTH: u32 = 856;
    const LEVEL: u32 = 20;
    const LEVEL_HEALTH: f64 = 1872.2;
    const LEVEL_DAMAGE: f64 = 686.1;
    const LEVEL_ARMOR: f64 = 109.336;
    const SWORD_ITEM: &str = "1hSwordT1";
    /// The swordsman's starter chest and its stats row (build 25810808).
    const CHEST_ITEM: &str = "SwordsmanArchetypeChest";
    const CHEST_STATS: &str = "HeavyChest_Balanced";
    const CHEST: ArmorRow = ArmorRow {
        rating_share: 0.35,
        physical_scale: 0.874_996_5,
        elemental_scale: 0.874_996_5,
    };
    /// A sword's open-world PvP adjustment in the test table.
    const SWORD_PVP_ADJUSTMENT: f64 = -0.1;
    /// The bow's (retail -0.253); no test item is a bow.
    const BOW_PVP_ADJUSTMENT: f64 = -0.253;
    const POTION_COOLDOWN_S: u32 = 25;
    const POTION_HEAL: f64 = 400.0;
    const POTION_SHARE: f64 = 0.1;
    const BOAR_HEALTH_MOD: f64 = 0.5;
    const BOAR_DAMAGE_MOD: f64 = 0.8;
    const SWORD_BASE: f64 = 64.0;
    const LIGHT: &str = "1H_Sword_Attack1";
    const HEAVY: &str = "1H_Sword_Attack2";
    const PELLET: &str = "Blunderbuss_Primary_Attack";
    const PLAYER_TABLE: &str = "DamageTable";
    const VISION_OFFSET: f32 = 5.0;

    fn rows() -> CombatRows {
        CombatRows {
            player_vitals_id: "Player".to_owned(),
            weapon_id: "SwordT1".to_owned(),
            damage_table: PLAYER_TABLE.to_owned(),
        }
    }

    fn boar() -> MobType {
        MobType {
            health_mod: BOAR_HEALTH_MOD,
            category_health_mod: 1.0,
            damage_mod: BOAR_DAMAGE_MOD,
            creature_type: Some("Beast".to_owned()),
            guard: None,
        }
    }

    #[test]
    fn a_shield_mobs_vitals_give_its_guard_and_a_boars_none() {
        // Retail rows (build 25810808): `Damned_1H_Swordsman_M-F_42` and `Boar`.
        let guard = MobGuard::of((50, 50, 2, 1), (1, 0.8)).expect("a guard");
        assert!(
            (guard.regen_per_s - 3.0).abs() < 1e-4,
            "{}",
            guard.regen_per_s
        );
        assert!((guard.mitigation - 1.0).abs() < f32::EPSILON);
        assert!(
            (guard.tick_rate - 2.0).abs() < f32::EPSILON,
            "its card's tick rate"
        );
        assert_eq!(MobGuard::of((1, 0, 0, 0), (0, 0.0)), None);
    }

    /// The occultist's starter ice gauntlet, its stats row and the row's `MannequinTag` (retail
    /// rows, build 25810808).
    const ICE_GAUNTLET_ITEM: &str = "OccultistArchetype_1hGauntletIce";
    const ICE_GAUNTLET_STATS: &str = "1hElementalGauntlet_IceT2";
    const ICE_GAUNTLET_TAGS: &str = "MagicGauntlet,Ice";
    /// The ice gauntlet's `RangedAttackProfile`.
    const ICE_PROFILE: &str = "IceMagic";

    fn sword_row() -> WeaponRow {
        WeaponRow {
            base_damage: SWORD_BASE,
            crit_chance: 0.03,
            crit_multiplier: 1.3,
            scaling: [0.0; ATTRIBUTE_COUNT],
            spread_shot: false,
            max_loaded_ammo: 0,
            reload_ms: 0,
            stance: stance_key("1H_Melee,Sword"),
            block_stamina_damage: 0.0,
            block_stability: 0.0,
            ranged_block: None,
            stagger_damage: 0.0,
        }
    }

    /// A heart rune of the build: its stats row grants its ability while worn.
    const RUNE_ITEM: &str = "RuneDetonateT3A";
    const RUNE_STATS: &str = "RuneDetonateT1";
    const RUNE_ABILITY: &str = "Ability_Rune_Detonate";

    /// The dryad shaman's bomb (`javelindata_spelltable_ai`, build 25810808).
    const MOB_SPELL: &str = "Dryad_Bomb";
    const MOB_SPELL_ROW: &str = "Mushroom";
    const MOB_SPELL_RADIUS_M: f32 = 3.0;

    fn mob_bomb() -> MobSpell {
        let row: crate::generated::SpellData = serde_json::from_value(serde_json::json!({
            "spell_id": MOB_SPELL,
            "radius": MOB_SPELL_RADIUS_M,
            "damage_table_row": MOB_SPELL_ROW,
            "status_effects": "Slow, Rooted",
            "target_types": "Enemy",
            "ranged_attack_profile": "AI_Ranged_AOE",
        }))
        .expect("row");
        MobSpell {
            row: SpellRow::of_data(&row, None, None, None),
            flies: row.ranged_attack_profile.is_some(),
        }
    }

    #[test]
    fn a_creature_clip_finds_its_spell_by_name_in_any_case() {
        let data = CombatData::build(source(), &rows()).expect("data");
        let spell = data.mob_spell("dryad_bomb").expect("spell");
        assert!(spell.flies);
        assert_eq!(spell.row.damage_row.as_deref(), Some(MOB_SPELL_ROW));
        assert_eq!(spell.row.radius_m, Some(MOB_SPELL_RADIUS_M));
        assert_eq!(spell.row.status_effects, ["Slow", "Rooted"]);
        assert!(data.mob_spell("Dryad_Unknown").is_none());
    }

    #[test]
    fn a_worn_heart_rune_grants_its_stats_row_ability() {
        let data = CombatData::build(source(), &rows()).expect("data");
        assert_eq!(
            data.equip_ability_of_item("runedetonatet3a"),
            Some(RUNE_ABILITY)
        );
        assert_eq!(data.equip_ability_of_item(SWORD_ITEM), None);
    }

    fn source() -> CombatSource {
        CombatSource {
            base_types: vec![("Boar_20".to_owned(), boar())],
            plain_types: vec!["Grunt".to_owned(), "Boar_20".to_owned()],
            variants: vec![("Boar_20_Elite".to_owned(), "Boar_20".to_owned())],
            levels: vec![(
                LEVEL,
                LevelRow {
                    base_max_health: LEVEL_HEALTH,
                    base_damage: LEVEL_DAMAGE,
                    physical_armor_rating: LEVEL_ARMOR,
                    elemental_armor_rating: LEVEL_ARMOR,
                    gear_score: 0,
                },
            )],
            weapons: vec![
                ("SwordT1".to_owned(), sword_row()),
                (
                    ICE_GAUNTLET_STATS.to_owned(),
                    WeaponRow {
                        stance: stance_key(ICE_GAUNTLET_TAGS),
                        ..sword_row()
                    },
                ),
            ],
            ranged_profiles: vec![(ICE_GAUNTLET_STATS.to_owned(), ICE_PROFILE.to_owned())],
            consumables: vec![ConsumableRow {
                id: "PotionHealthT1".to_owned(),
                cooldown_s: POTION_COOLDOWN_S,
                cooldown_id: "SurvivalPotion".to_owned(),
                status_effects: "PotionHeal".to_owned(),
                use_tag: "Potion".to_owned(),
            }],
            heals: vec![("PotionHeal".to_owned(), POTION_HEAL, POTION_SHARE)],
            // As in the retail tables: the potion has no stats row.
            item_classes: vec![(
                SWORD_ITEM.to_owned(),
                "EquippableMainHand+Sword+Melee".to_owned(),
            )],
            pvp_weapon_adjustments: vec![
                ("Sword".to_owned(), SWORD_PVP_ADJUSTMENT),
                ("Bow".to_owned(), BOW_PVP_ADJUSTMENT),
            ],
            power_level_shifts: vec![PowerLevelShift {
                disparity: 10,
                required: 0,
                adjusted: 1,
                adjusted_hit_stun: 20,
            }],
            equip_abilities: vec![(RUNE_STATS.to_owned(), RUNE_ABILITY.to_owned())],
            mob_spells: vec![mob_bomb()],
            item_stats: vec![
                (SWORD_ITEM.to_owned(), "SwordT1".to_owned()),
                ("Shield".to_owned(), "NoWeaponRow".to_owned()),
                (ICE_GAUNTLET_ITEM.to_owned(), ICE_GAUNTLET_STATS.to_owned()),
                (CHEST_ITEM.to_owned(), CHEST_STATS.to_owned()),
                (RUNE_ITEM.to_owned(), RUNE_STATS.to_owned()),
            ],
            armors: vec![(CHEST_STATS.to_owned(), CHEST)],
            attacks: [(LIGHT, 1.0, 0), (HEAVY, 1.2, 1), (PELLET, 0.19, 0)]
                .map(|(id, coefficient, threat)| AttackRow {
                    damage_id: id.to_owned(),
                    coefficient,
                    can_crit: true,
                    threat,
                    damage_type: None,
                    table: PLAYER_TABLE.to_owned(),
                    block_stamina_mod: 0.0,
                    blockable: true,
                    category: DamageCategory::Physical,
                    affix: None,
                    backstab: true,
                    headshot: true,
                    rune_charge: RuneChargeRow::default(),
                    power_level: 0,
                    hit_stun: 0,
                    stagger_mod: 0.0,
                    stun_breakout: 0.0,
                })
                .to_vec(),
            vision_offsets: vec![("Beast".to_owned(), VISION_OFFSET)],
            player_health: vec![("Player".to_owned(), PLAYER_HEALTH)],
            categories: Vec::new(),
        }
    }

    fn data() -> CombatData {
        CombatData::build(source(), &rows()).expect("data")
    }

    #[test]
    fn a_weapon_item_has_the_row_of_its_stats() {
        let data = data();
        assert_eq!(
            data.weapon_of_item("1HSWORDT1")
                .map(|weapon| weapon.base_damage),
            Some(SWORD_BASE)
        );
        assert_eq!(data.weapon_of_item("Shield"), None, "no weapon row");
        assert_eq!(
            data.heal_of_item("potionhealtht1"),
            Some(&HealingConsumable {
                cooldown_s: POTION_COOLDOWN_S,
                cooldown_id: "SurvivalPotion".to_owned(),
                health: POTION_HEAL,
                health_share: POTION_SHARE,
                use_tag: "Potion".to_owned(),
            })
        );
        assert_eq!(
            data.level(LEVEL).map(|level| level.physical_armor_rating),
            Some(LEVEL_ARMOR)
        );
    }

    #[test]
    fn an_attack_is_found_in_its_own_table() {
        let mut source = source();
        // Retail: `1H_Sword_Attack1` is in the player table (1.0) and in NPC tables (0.15).
        source.attacks.push(AttackRow {
            coefficient: 0.15,
            table: "DamnedDamageTable_Damned_FTUE".to_owned(),
            ..source.attacks[0].clone()
        });
        let data = CombatData::build(source, &rows()).expect("data");
        assert_eq!(data.attack(LIGHT).map(|row| row.coefficient), Some(1.0));
        assert_eq!(
            data.table_attack("damneddamagetable_damned_ftue", LIGHT)
                .map(|row| row.coefficient),
            Some(0.15)
        );
        assert_eq!(data.table_attack("NoTable", LIGHT), None);
    }

    /// The flail stance names `1h_Flail_LightAttack1`; the player table holds
    /// `1H_Flail_LightAttack1` (build 25810808).
    #[test]
    fn an_attack_row_id_matches_in_any_case() {
        let data = CombatData::build(source(), &rows()).expect("data");
        let stance_spelling = LIGHT.to_lowercase();
        assert_eq!(
            data.attack(&stance_spelling)
                .map(|row| row.damage_id.as_str()),
            Some(LIGHT)
        );
    }

    #[test]
    fn two_rows_of_one_table_that_differ_only_in_case_are_refused() {
        let mut source = source();
        source.attacks.push(AttackRow {
            damage_id: LIGHT.to_uppercase(),
            ..source.attacks[0].clone()
        });
        assert!(matches!(
            CombatData::build(source, &rows()),
            Err(CombatDataError::DuplicateAttackRow { .. })
        ));
    }

    /// A stance of tags `tags` with reaction rows of `fragments` (fragment, animations).
    fn reacting(tags: &str, fragments: &[(&str, &str)]) -> Stance {
        use crate::player_attacks::{HitReaction, ReactionEntry};
        use nw_formats::mannequin::Condition;
        Stance {
            tags: tags.to_owned(),
            light: None,
            light_from_ms: 0,
            heavy_from_ms: None,
            heavy: None,
            charged_at_ms: None,
            charged: None,
            dodge_invulnerable_ms: None,
            sheathe_ms: None,
            uses: Vec::new(),
            combo: Vec::new(),
            variants: Vec::new(),
            aimed: None,
            aimed_charged: None,
            reactions: fragments
                .iter()
                .map(|(fragment, animations)| HitReaction {
                    power_level: None,
                    fragment: (*fragment).to_owned(),
                    condition: Condition::True,
                    entry: Some(ReactionEntry {
                        animations: (*animations).to_owned(),
                        length_ms: 0,
                        can_transition_ms: None,
                        motion: None,
                    }),
                })
                .collect(),
        }
    }

    #[test]
    fn a_reaction_without_a_mined_sequence_takes_the_nearest_mined_one() {
        use crate::player_sequences::ReactionPlay;
        const SWORD: &str = "1H_Melee,Sword";
        let sword = reacting(
            SWORD,
            &[
                ("r_R1_F", "sword_r1_f"),
                ("r_R1_L", "sword_r1_l"),
                ("r_R1_B", "sword_r1_b"),
                ("r_R3_F", "sword_r3_f"),
                ("r_R4_F", "sword_r4_f"),
                ("r_B0_Break", "sword_break"),
                ("r_R0_F", "sword_r0_f"),
            ],
        );
        let great_sword = reacting(
            "2H_Melee,GreatSword",
            &[
                ("r_R1_F", "great_r1_f"),
                ("r_B0_Break", "great_break"),
                ("r_R4_F", "great_r4_f"),
                ("r_R0_F", "great_r0_f"),
            ],
        );
        let mined = [
            ("sword_r1_f", 100),
            ("sword_r3_f", 300),
            ("great_r1_f", 900),
            ("great_break", 2125),
            ("great_r4_f", 3486),
            ("great_r0_f", 50),
        ];
        let data = data()
            .with_player_attacks(PlayerAttacks::of_stances(vec![sword.clone(), great_sword]))
            .with_player_sequences(
                PlayerSequences::default().with_reactions(
                    mined
                        .iter()
                        .map(|(animations, sequence)| ReactionPlay {
                            animations: (*animations).to_owned(),
                            sequence: *sequence,
                        })
                        .collect(),
                ),
            );
        let key = stance_key(SWORD);
        let sequence = |fragment: &str| {
            let row = sword
                .reactions
                .iter()
                .find(|row| row.fragment == fragment)
                .expect("row");
            data.reaction_sequence(key, &sword, row)
                .map(|found| (found.sequence, found.inferred))
        };
        assert_eq!(sequence("r_R1_F"), Some((100, false)), "its own mined one");
        assert_eq!(
            sequence("r_R1_L"),
            Some((100, true)),
            "the same stance and level a quarter turn away"
        );
        assert_eq!(
            sequence("r_R1_B"),
            Some((100, true)),
            "the same level before a nearer side of another level"
        );
        assert_eq!(
            sequence("r_B0_Break"),
            Some((2125, true)),
            "a guard break of another stance before a stagger of its own"
        );
        assert_eq!(
            sequence("r_R4_F"),
            Some((3486, true)),
            "a knockdown of another stance"
        );
        assert_eq!(
            sequence("r_R0_F"),
            Some((50, true)),
            "an upper-body row takes an upper-body sequence only"
        );
    }

    #[test]
    fn a_weapon_finds_its_stance() {
        let stance = Stance {
            tags: "1H_Melee,Sword".to_owned(),
            light: None,
            light_from_ms: 0,
            heavy_from_ms: None,
            heavy: None,
            charged_at_ms: None,
            charged: None,
            dodge_invulnerable_ms: Some([0, 500]),
            sheathe_ms: None,
            uses: Vec::new(),
            combo: Vec::new(),
            variants: Vec::new(),
            aimed: None,
            aimed_charged: None,
            reactions: Vec::new(),
        };
        let unarmed = Stance {
            tags: String::new(),
            ..stance.clone()
        };
        let data = data().with_player_attacks(PlayerAttacks::of_stances(vec![stance, unarmed]));
        let sword = data.weapon_of_item(SWORD_ITEM).expect("sword");
        assert_eq!(data.armor_of_item("swordsmanarchetypechest"), Some(&CHEST));
        assert_eq!(data.armor_of_item(SWORD_ITEM), None, "a weapon is no armor");
        assert_eq!(
            data.stance(sword.stance).map(|stance| stance.tags.as_str()),
            Some("1H_Melee,Sword")
        );
        assert_eq!(
            data.unarmed_stance().map(|stance| stance.tags.as_str()),
            Some("")
        );
        // The starter ice gauntlet's stats id names no weapon; its row's tags do.
        assert_eq!(
            data.weapon_of_item(ICE_GAUNTLET_ITEM)
                .map(|weapon| weapon.stance),
            Some(stance_key(ICE_GAUNTLET_TAGS))
        );
    }

    #[test]
    fn mob_types_come_from_vitals_base_first() {
        let data = data();
        assert_eq!(data.mob_type(lowercase_crc32("boar_20")), Some(&boar()));
        assert_eq!(data.mob_type(lowercase_crc32("BOAR_20")), Some(&boar()));
    }

    #[test]
    fn a_creature_type_multiplies_the_mods() {
        let mut source = source();
        source.categories = vec![(
            "Beast".to_owned(),
            CategoryMods {
                health: 2.0,
                damage: 1.5,
            },
        )];
        let data = CombatData::build(source, &rows()).expect("data");
        let boar = data.mob_type(lowercase_crc32("Boar_20")).expect("boar");
        assert_eq!(boar.health_mod, BOAR_HEALTH_MOD);
        assert_eq!(boar.category_health_mod, 2.0);
        assert_eq!(boar.damage_mod, BOAR_DAMAGE_MOD * 1.5);
        let elite = data
            .mob_type(lowercase_crc32("Boar_20_Elite"))
            .expect("variant");
        assert_eq!(elite.category_health_mod, 2.0, "a variant too");
    }

    #[test]
    fn a_type_only_in_vitals_data_gets_mods_of_one() {
        let grunt = data()
            .mob_type(lowercase_crc32("Grunt"))
            .cloned()
            .expect("grunt");
        assert_eq!(grunt, MobType::PLAIN);
    }

    #[test]
    fn a_variant_takes_the_type_of_its_base() {
        assert_eq!(
            data().mob_type(lowercase_crc32("Boar_20_Elite")),
            Some(&boar())
        );
        assert_eq!(data().mob_type(lowercase_crc32("Nobody")), None);
    }

    #[test]
    fn levels_are_read_by_level() {
        let data = data();
        assert_eq!(
            data.level(LEVEL).map(|row| row.base_damage),
            Some(LEVEL_DAMAGE)
        );
        assert_eq!(data.level(LEVEL + 1), None);
    }

    #[test]
    fn missing_config_rows_are_errors() {
        let mut no_player = rows();
        no_player.player_vitals_id = "Ghost".to_owned();
        assert!(matches!(
            CombatData::build(source(), &no_player),
            Err(CombatDataError::MissingPlayer(_))
        ));
        let mut no_weapon = rows();
        no_weapon.weapon_id = "Spoon".to_owned();
        assert!(matches!(
            CombatData::build(source(), &no_weapon),
            Err(CombatDataError::MissingWeapon(_))
        ));
        assert!(matches!(
            CombatData::from_tables(&Tables::default(), &rows()),
            Err(CombatDataError::MissingPlayer(_))
        ));
    }

    #[test]
    fn the_player_health_is_the_initial_of_its_row() {
        assert_eq!(data().player_base_health(), PLAYER_HEALTH);
    }

    #[test]
    fn the_weapon_and_its_attacks_are_kept() {
        let data = data();
        assert_eq!(data.weapon().base_damage, SWORD_BASE);
        assert_eq!(data.attack(HEAVY).map(|row| row.threat), Some(1));
        assert_eq!(data.attack("Other"), None);
    }

    #[test]
    fn an_unknown_creature_type_has_no_vision_offset() {
        let data = data();
        assert_eq!(data.vision_offset(Some("Beast")), VISION_OFFSET);
        assert_eq!(data.vision_offset(Some("Dragon")), 0.0);
        assert_eq!(data.vision_offset(None), 0.0);
    }

    #[test]
    fn a_weapon_item_turns_on_the_ability_lists_of_its_classes() {
        use crate::player_abilities::AbilityList;
        let list = |class: &str| AbilityList {
            weapon_classes: vec![class.to_owned()],
            actions: Vec::new(),
            fragments: Vec::new(),
            cooldowns_ms: std::collections::BTreeMap::new(),
            mana_costs: std::collections::BTreeMap::new(),
        };
        let data = CombatData::build(source(), &rows())
            .expect("data")
            .with_player_abilities(PlayerAbilities {
                lists: vec![list("LifeStaff"), list("Sword")],
                inputs: Vec::new(),
            });
        let lists = data.ability_lists_of_item("1hswordt1");
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].weapon_classes, ["Sword"]);
        assert!(data.ability_lists_of_item("NoSuchItem").is_empty());
    }

    #[test]
    fn a_weapon_class_in_the_pvp_balance_scales_its_pvp_damage() {
        let data = CombatData::build(source(), &rows()).expect("data");
        assert!((data.pvp_damage_factor("1HSWORDT1") - (1.0 + SWORD_PVP_ADJUSTMENT)).abs() < 1e-9);
        assert!((data.pvp_damage_factor(ICE_GAUNTLET_ITEM) - 1.0).abs() < 1e-9);
        assert!((data.pvp_damage_factor("NoSuchItem") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_weapon_item_has_the_ranged_profile_of_its_stats_row() {
        let data = CombatData::build(source(), &rows()).expect("data");
        assert_eq!(
            data.ranged_profile_of_item(&ICE_GAUNTLET_ITEM.to_uppercase()),
            Some(ICE_PROFILE)
        );
        assert_eq!(data.ranged_profile_of_item("1hSwordT1"), None, "no profile");
        assert_eq!(data.ranged_profile_of_item("NoSuchItem"), None);
    }
}
