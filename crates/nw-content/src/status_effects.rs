// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rules of every status effect (`StatusEffectData`, all 36 tables of build 25810808, 4,517
//! rows) and the effects that attack rows put on their target (`DamageData.StatusEffect`).
//!
//! The wire names an effect by its key: the position of the CRC-32 of its lowercase id in the
//! ascending list of the CRC-32 of every row of every `StatusEffectData` table, repeated ids
//! kept, a repeated id at its first position (run-15 combat study, Observed on seven retail
//! captures: `Sanctuary` is in two tables at 1316 and 1317 and always sent as 1316; with the
//! repeats left out every key above 1316 names an unrelated effect). The key is a big-endian
//! `u16` on the wire (`ntohs` in the client's reader).
//!
//! An effect lasts its `BaseDuration` (none: -1 or 0), longer or shorter by the
//! `EffectDurationMods` of the effects the target has whose categories it shares, at most its
//! `DurationMax`. A new application of an effect the target has adds a stack up to `StackMax`
//! (retail `Status_Passive_Lifestaff_Healing_HealingBuffonBasic`: stacks 1, 2, 3, 3, 3 and the
//! application count 1 to 6) and moves its end when `RefreshDuration` is not false. Its potency
//! is one stack's potency times the stacks, at most `PotencyMax`, times one plus the
//! `EffectPotencyMods` of the target's other effects for its categories (retail `FoodRecoveryT5`
//! potency 9 rose to 27 while `Status_OutOfCombat_PassiveRegen`, `PassiveFoodRegen=2`, ran).

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;
use crate::character_points::ATTRIBUTE_COUNT;
use crate::generated::StatusEffectData;

/// Separates the entries of a list cell (`EffectCategories`, `AddStatusEffects`, mods).
const LIST_SEPARATOR: char = '+';
/// Separates the effects of a `DamageData.StatusEffect` cell (a repeated id adds a stack).
const HIT_EFFECT_SEPARATOR: char = ',';
/// Separates a category from its value in a mod cell (`Root=-0.2`).
const MOD_SEPARATOR: char = '=';
/// The `HitCondition` of an effect that counts the hits its holder takes.
const HIT_TAKEN_CONDITION: &str = "OnHitTaken";
/// Milliseconds per second.
const MS_PER_SECOND: f64 = 1_000.0;
/// The stack limit of an effect whose `StackMax` is empty (567 rows): one stack.
const DEFAULT_STACK_MAX: u8 = 1;

/// The damage types that have an `Abs*` and a `Dmg*` column.
const DAMAGE_TYPE_COUNT: usize = 12;
/// The `TypeID` of the damage type of each `Abs*` and `Dmg*` column, in column order.
const DAMAGE_TYPE_COLUMNS: [&str; DAMAGE_TYPE_COUNT] = [
    "Acid",
    "Arcane",
    "Corruption",
    "Fire",
    "Ice",
    "Lightning",
    "Nature",
    "Siege",
    "Slash",
    "Standard",
    "Strike",
    "Thrust",
];

/// The gathering tradeskill of each `EFF_*` column, in column order.
const GATHER_EFFICIENCY_COLUMNS: [&str; 4] = ["Harvesting", "Logging", "Mining", "Skinning"];
/// The crafting tradeskill of each `MGS_*` and `MaxGS_*` column pair, in column order.
const CRAFT_GEAR_SCORE_COLUMNS: [&str; 5] = [
    "Arcana",
    "Armoring",
    "Engineering",
    "Jewelcrafting",
    "Weaponsmithing",
];

/// The tradeskill of each `EXP_*` column, in column order (`EXP_Woodworking` is a text column).
const EXPERIENCE_COLUMNS: [&str; 10] = [
    "Fishing",
    "Harvesting",
    "Leatherworking",
    "Logging",
    "Mining",
    "Skinning",
    "Smelting",
    "Stonecutting",
    "Weaving",
    "Woodworking",
];
/// The gathering tradeskill of each `MULT_*` column, in column order.
const GATHER_YIELD_COLUMNS: [&str; 5] = ["Fishing", "Harvesting", "Logging", "Mining", "Skinning"];
/// The tradeskill of each `ROL_*` column, in column order.
const LUCK_COLUMNS: [&str; 12] = [
    "Cooking",
    "Fishing",
    "Harvesting",
    "Jewelcrafting",
    "Leatherworking",
    "Logging",
    "Mining",
    "Skinning",
    "Smelting",
    "Stonecutting",
    "Weaving",
    "Woodworking",
];
/// The gathering tradeskill of each `INS*` column, in column order.
const INSTANT_GATHER_COLUMNS: [&str; 4] = ["Harvesting", "Logging", "Mining", "Skinning"];
/// Separates the entries of an `EXPCategoricalProgression` cell (`2:1,4:1`).
const PROGRESSION_SEPARATOR: char = ',';
/// Separates a progression from its share in an `EXPCategoricalProgression` entry.
const PROGRESSION_SHARE_SEPARATOR: char = ':';
/// Separates the tags of a `LootTags` cell (Inferred: as the loot tables' `Conditions`; every
/// cell of build 25810808 holds one tag).
const LOOT_TAG_SEPARATOR: char = ',';

/// The reward and tradeskill bonus columns of a status effect (status effect bonuses study
/// 2026-11-11). Each value is a share added to 1 of the reward it names, or a bonus added to a
/// loot roll; every held effect adds its value times its potency.
///
/// Observed (`Playthrough_Part_02`, five token uses at 1792686628775 to 1792686639126, the
/// tokens held all through the ledger): `Token_Gather_1` (`EXP_*` 1.0) made a level-modified
/// `Gather_SkinningT1` (32 x 2.2030358, 70 rounded down) give Skinning 280 = 70 x 2 (remote
/// multiplier) x (1 + 1) at 3492.2 s; `Token_Weapon_1` (`EXPCategoricalProgression` 1.0 for every
/// weapon mastery) made a kill of raw 16 give the held sword 64 = 16 x 2 x 2 and the other
/// weapon 12 = ceil(16 x 0.15) x 2 x 2 at 1505.5 s; `Token_Territory_1` (1.0 for every territory
/// standing) made the kill's standing 5 give 30 = 5 x 3 x 2 and a chart's 50 give 300 at
/// 1626.6 s. The other columns keep this form (Inferred: no capture holds a reward with them).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RewardMods {
    /// The `EXP_*` and `EXPCategoricalProgression` columns: (lowercase
    /// `CategoricalProgressionId`, share added to its gains).
    pub progression: Vec<(String, f64)>,
    /// The `MULT_*` columns: (lowercase gathering tradeskill, share added to its yield).
    pub gather_yield: Vec<(String, f64)>,
    /// The `ROL_*` columns: (lowercase tradeskill, bonus added to its loot rolls).
    pub tradeskill_luck: Vec<(String, f64)>,
    /// `GlobalRollMod`: the bonus added to the other loot rolls.
    pub global_luck: f64,
    /// The `INS*` columns: (lowercase gathering tradeskill, chance that a gather of it is done
    /// in a single swing). Observed (texts of build 25810808): `ATB_Str_Bonus_300_2_Effect`
    /// `INSMining` 0.25 is the Strength 300 bonus "25% chance to mine an ore with a single swing"
    /// (`Str_Bonus_300_active`), `ATB_Con_Bonus_300_2_Effect` `INSLogging` 0.25 the Constitution
    /// 300 bonus "25% chance to chop down a tree in a single swing" (`Con_Bonus_300_active`).
    pub instant_gather: Vec<(String, f64)>,
    /// `TerritoryStandingMod`: the share added to territory standing gains.
    pub territory_standing: f64,
    /// `FactionReputationMod`: the share added to faction reputation gains.
    pub faction_reputation: f64,
    /// `FactionTokensMod`: the share added to faction token gains.
    pub faction_tokens: f64,
    /// `AzothMod`: the share added to azoth gains.
    pub azoth: f64,
    /// `LootTags`: the tags its holder's loot rolls hold (`ToolMoteFire`, `EtherealMining`).
    pub loot_tags: Vec<String>,
}

impl RewardMods {
    /// The bonus columns of datasheet row `row`.
    fn of(row: &StatusEffectData) -> Self {
        let by_skill = |skills: &[&str], values: &[Option<f64>]| -> Vec<(String, f64)> {
            skills
                .iter()
                .zip(values)
                .filter_map(|(skill, value)| {
                    value
                        .filter(|value| *value != 0.0)
                        .map(|value| (skill.to_lowercase(), value))
                })
                .collect()
        };
        // Luck bonuses of a few thousand.
        let luck = |value: Option<i64>| value.map(|value| value as f64);
        // `EXP_Woodworking`, `INSHarvesting` and `INSSkinning` are text columns: a number in
        // them counts, another text does not.
        let number = |cell: Option<&str>| cell.and_then(|cell| cell.trim().parse().ok());
        let woodworking = number(row.exp_woodworking.as_deref());
        let mut progression = by_skill(
            &EXPERIENCE_COLUMNS,
            &[
                row.exp_fishing,
                row.exp_harvesting,
                row.exp_leatherworking,
                row.exp_logging,
                row.exp_mining,
                row.exp_skinning,
                row.exp_smelting,
                row.exp_stonecutting,
                row.exp_weaving,
                woodworking,
            ],
        );
        progression.extend(progression_shares(
            row.exp_categorical_progression.as_deref(),
        ));
        Self {
            progression,
            gather_yield: by_skill(
                &GATHER_YIELD_COLUMNS,
                &[
                    row.mult_fishing,
                    row.mult_harvesting,
                    row.mult_logging,
                    row.mult_mining,
                    row.mult_skinning,
                ],
            ),
            tradeskill_luck: by_skill(
                &LUCK_COLUMNS,
                &[
                    luck(row.rol_cooking),
                    luck(row.rol_fishing),
                    luck(row.rol_harvesting),
                    luck(row.rol_jewelcrafting),
                    luck(row.rol_leatherworking),
                    luck(row.rol_logging),
                    luck(row.rol_mining),
                    luck(row.rol_skinning),
                    luck(row.rol_smelting),
                    luck(row.rol_stonecutting),
                    luck(row.rol_weaving),
                    luck(row.rol_woodworking),
                ],
            ),
            global_luck: row.global_roll_mod.unwrap_or(0.0),
            instant_gather: by_skill(
                &INSTANT_GATHER_COLUMNS,
                &[
                    number(row.ins_harvesting.as_deref()),
                    row.ins_logging,
                    row.ins_mining,
                    number(row.ins_skinning.as_deref()),
                ],
            ),
            territory_standing: row.territory_standing_mod.unwrap_or(0.0),
            faction_reputation: row.faction_reputation_mod.unwrap_or(0.0),
            faction_tokens: row.faction_tokens_mod.unwrap_or(0.0),
            azoth: row.azoth_mod.unwrap_or(0.0),
            loot_tags: row
                .loot_tags
                .as_deref()
                .unwrap_or_default()
                .split(LOOT_TAG_SEPARATOR)
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }

    /// The share it adds to the gains of progression `id` (any case).
    pub fn progression_share(&self, id: &str) -> f64 {
        share_of(&self.progression, id)
    }

    /// The share it adds to the yield of gathering tradeskill `tradeskill` (any case).
    pub fn yield_share(&self, tradeskill: &str) -> f64 {
        share_of(&self.gather_yield, tradeskill)
    }

    /// The bonus it adds to the loot rolls of tradeskill `tradeskill` (any case).
    pub fn luck_of(&self, tradeskill: &str) -> f64 {
        share_of(&self.tradeskill_luck, tradeskill)
    }

    /// The chance it gives a gather of gathering tradeskill `tradeskill` (any case) to be done
    /// in a single swing.
    pub fn instant_gather_chance(&self, tradeskill: &str) -> f64 {
        share_of(&self.instant_gather, tradeskill)
    }
}

/// The sum of the values of `values` (lowercase keys) for `key` (any case).
fn share_of(values: &[(String, f64)], key: &str) -> f64 {
    values
        .iter()
        .filter(|(own, _)| own.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
        .sum()
}

/// The (lowercase progression id, share) entries of an `EXPCategoricalProgression` cell
/// (`HatchetAbilityTable:1,SwordAbilityTable:1`, `2:0.15`); an entry without a number is left
/// out.
fn progression_shares(cell: Option<&str>) -> Vec<(String, f64)> {
    cell.unwrap_or_default()
        .split(PROGRESSION_SEPARATOR)
        .filter_map(|entry| {
            let (id, share) = entry.split_once(PROGRESSION_SHARE_SEPARATOR)?;
            let id = id.trim();
            let share: f64 = share.trim().parse().ok()?;
            (!id.is_empty() && share != 0.0).then(|| (id.to_lowercase(), share))
        })
        .collect()
}

/// The rules of one status effect.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusEffectRule {
    /// `StatusID`.
    pub id: String,
    /// The CRC-32 of the lowercase id.
    pub crc: u32,
    /// The wire key ([`StatusEffectRules::key`]).
    pub key: u16,
    /// How long it lasts; `None`: until something removes it.
    pub duration_ms: Option<u64>,
    /// `DurationMax`: the longest it may last after the mods.
    pub duration_max_ms: Option<u64>,
    /// `TickRate`: the time between two of its vitals changes; `None`: it does not tick.
    pub tick_ms: Option<u64>,
    /// `DelayInitialTick`: the first change comes this long after the application.
    pub first_tick_ms: u64,
    /// `StackMax`, at least 1.
    pub stack_max: u8,
    /// A new application moves its end (`RefreshDuration` not false).
    pub refresh_duration: bool,
    /// `PotencyMax`: the most its stacks give.
    pub potency_max: Option<f64>,
    /// `EffectCategories`, lowercase.
    pub categories: Vec<String>,
    /// `RemoveStatusEffectCategories`: its application ends the effects of these categories.
    pub remove_categories: Vec<String>,
    /// `RemoveStatusEffects`: its application ends these effects (CRC-32 of the lowercase ids).
    pub remove_effects: Vec<u32>,
    /// `AntirequisiteEffectCategories`: it does not apply while the holder has an effect of one
    /// of these categories.
    pub blocked_by: Vec<String>,
    /// `Health`: health per change.
    pub health: f64,
    /// `HealthModifierPercent`: the share of the max health per change.
    pub health_share: f64,
    /// `HealthModifierDamageBased`: the share of the applying hit per change.
    pub hit_share: f64,
    /// `Stamina`: stamina per change.
    pub stamina: f64,
    /// The `DamageTypeData` `IntID` of its `DamageType`.
    pub damage_type: Option<u8>,
    /// `RemoveOnDeath`.
    pub remove_on_death: bool,
    /// `RemoveOnRespawn`.
    pub remove_on_respawn: bool,
    /// `DisablePauseOnPersist`: its time runs on while its holder is away; without it the time
    /// pauses.
    pub runs_offline: bool,
    /// `AllowAddIfDead`.
    pub allow_add_if_dead: bool,
    /// `UseLightweightReplication`: it goes in the lightweight map of the 4236 state.
    pub lightweight: bool,
    /// `IsClientPredicted`: its lightweight value is a 30 Hz tick, not a time.
    pub client_predicted: bool,
    /// `StopOnHitCount` with the `OnHitTaken` condition: it ends after this many hits taken.
    pub stop_on_hits_taken: Option<u32>,
    /// `OnEndStatusEffect`: added when it ends (CRC-32 of the lowercase ids).
    pub on_end: Vec<u32>,
    /// `OnTickStatusEffect`: added at each change (CRC-32 of the lowercase ids).
    pub on_tick: Vec<u32>,
    /// `EffectPotencyMods`: (lowercase category, mod) for the holder's other effects.
    pub potency_mods: Vec<(String, f64)>,
    /// `EffectDurationMods`: (lowercase category, mod) for the effects the holder gets.
    pub duration_mods: Vec<(String, f64)>,
    /// The `Abs*` columns: (damage type `IntID`, share of the hit taken off).
    pub absorption: Vec<(u8, f64)>,
    /// The `Dmg*` columns: (damage type `IntID`, share added to the holder's hits).
    pub damage_mods: Vec<(u8, f64)>,
    /// The `EFF_*` columns: (gathering tradeskill, gathering efficiency added).
    pub gather_efficiency: Vec<(String, f64)>,
    /// `ItemClassWeightMods`: (lowercase item class, share added to the weight its items have
    /// in the holder's bag; `ATB_Str_Bonus_150_2_Effect` `Ore=-0.1+Stone=-0.1+Gem=-0.1`, the
    /// Strength 150 bonus "-10% weight of mined items").
    pub item_weight_mods: Vec<(String, f64)>,
    /// `Encumbrance`: what it adds to the holder's encumbrance limit (`ATB_Str_Bonus_50_2_Effect`
    /// 250, the Strength 50 bonus "+25 encumbrance": the limit's units are a tenth of the shown
    /// weight, as `EncumbranceBaseMax` 2000 of the player).
    pub encumbrance: f64,
    /// The `MGS_*` and `MaxGS_*` columns: (crafting tradeskill, gear score added to the least
    /// and to the most gear score of its crafts).
    pub craft_gear_score: Vec<(String, i64, i64)>,
    /// The `WKN*` columns: (damage type `IntID`, share added to the hits the holder takes;
    /// `Greatsword_Stance_Offense` 0.15, `Greatsword_Stance_Defense` -0.15).
    pub weakness: Vec<(u8, f64)>,
    /// `StaminaRate`: the share added to the holder's stamina regen (the `*_Exhaust` effects
    /// -0.1 to -0.2).
    pub stamina_rate: f64,
    /// `StaminaDamageModifier`: the share added to the stamina the holder's hits take from a
    /// blocker (the `AI_*_Empower` mob buffs 0.15 to 0.3).
    pub block_stamina_mod: f64,
    /// The `MOD*` columns: the points added to each attribute, in
    /// [`crate::character_points::ATTRIBUTE_NAMES`] order (the `Food*` meals 10 to 24).
    pub attribute_mods: [i64; ATTRIBUTE_COUNT],
    /// `CritChanceModifier`: the chance added to the holder's crits.
    pub crit_chance: f64,
    /// `CritDamageModifier`: the share added to the holder's crit damage.
    pub crit_damage: f64,
    /// `HealMod`: the share added to the heals the holder gets (the disease effects -0.01 to
    /// -0.3).
    pub heal_mod: f64,
    /// `ConsumableHealMod`: the share added to the heals of the holder's consumables.
    pub consumable_heal_mod: f64,
    /// `PhysicalArmor`: the share added to the holder's physical armor (the rend effects -0.02
    /// to -0.1).
    pub physical_armor: f64,
    /// `ElementalArmor`: the share added to the holder's elemental armor.
    pub elemental_armor: f64,
    /// `HealthRate`: the share added to the holder's health regen (`Erosion_lv1` -0.5).
    pub health_rate: f64,
    /// What it keeps its holder from doing.
    pub control: Control,
    /// `RemoveOnDeathsDoor`: it ends when its holder goes to death's door.
    pub remove_on_deaths_door: bool,
    /// `OnDeathStatusEffect`: added when its holder dies (CRC-32 of the lowercase ids).
    pub on_death: Vec<u32>,
    /// `InitialStackSize`: the stacks of its first application, when more than one.
    pub initial_stacks: Option<u8>,
    /// `GameEventID`: the game event its application fires (the mount foods'
    /// `Mount_GrantRidingXP_*`: riding experience, and the quest step `9401_TutorialFood`).
    pub game_event: Option<String>,
    /// `ShowTextInDamageNumbers`: its application by a consumable plays its visual script
    /// (608). Observed: retail sent 608 with `Status_Mount_GrantRidingXP_Small` 730 ms after the
    /// mount food press of `9401_tutorialfood-Hungry-as-a-horse` (67.85 s), and none at the
    /// `FoodRecoveryT2` use of `general-other-01` (502.111 s), an effect without it. Inferred:
    /// the column is what sends it.
    pub shows_damage_text: bool,
    /// `StackDuration` is set: a new application adds its duration to the time left
    /// (the `Token_*` boosts; Observed, `Playthrough_Part_02`: a `Token_Proficiency_1` use at
    /// 1792686639126 ended at 1792695508531, its 5,400 s after the 3,469 s the token held from an
    /// earlier session; the other four tokens, not held before, ended 7,200 s after their use).
    pub stack_duration: bool,
    /// Its reward and tradeskill bonuses.
    pub rewards: RewardMods,
}

/// What a status effect keeps its holder from doing (the flag columns of `StatusEffectData`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Control {
    /// `NoDodge`: no dodge (79 effects, the roots and tethers).
    pub no_dodge: bool,
    /// `Stunned`: no attack, dodge, block or ability (48 effects, `AI_Stun_2sec`).
    pub stunned: bool,
    /// `Silenced`: no ability (40 effects, `AI_Silence_6sec`).
    pub silenced: bool,
    /// `NoSprint`: no sprint (130 effects).
    pub no_sprint: bool,
    /// `NoHealthRegen`: no health regen (`IceMagic_Entombed`).
    pub no_health_regen: bool,
}

impl Control {
    /// Whether it keeps its holder from anything.
    pub const fn any(self) -> bool {
        self.no_dodge || self.stunned || self.silenced || self.no_sprint || self.no_health_regen
    }

    /// What `self` and `other` keep the holder from together.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self {
            no_dodge: self.no_dodge || other.no_dodge,
            stunned: self.stunned || other.stunned,
            silenced: self.silenced || other.silenced,
            no_sprint: self.no_sprint || other.no_sprint,
            no_health_regen: self.no_health_regen || other.no_health_regen,
        }
    }

    /// Whether the holder may dodge.
    pub const fn may_dodge(self) -> bool {
        !self.no_dodge && !self.stunned
    }

    /// Whether the holder may use an ability.
    pub const fn may_cast(self) -> bool {
        !self.silenced && !self.stunned
    }

    /// Whether the holder may attack or block.
    pub const fn may_fight(self) -> bool {
        !self.stunned
    }
}

impl StatusEffectRule {
    /// Whether one of its categories is `category` (lowercase).
    pub fn has_category(&self, category: &str) -> bool {
        self.categories.iter().any(|own| own == category)
    }

    /// Whether it changes the vitals at its changes.
    pub fn changes_vitals(&self) -> bool {
        self.health != 0.0
            || self.health_share != 0.0
            || self.hit_share != 0.0
            || self.stamina != 0.0
    }
}

/// One `StatusEffectData` row, as the rules need it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EffectSourceRow {
    /// `StatusID`.
    pub status_id: String,
    /// `BaseDuration`, seconds.
    pub base_duration_s: Option<f64>,
    /// `DurationMax`, seconds.
    pub duration_max_s: Option<f64>,
    /// `TickRate`, seconds.
    pub tick_s: Option<f64>,
    /// `DelayInitialTick`, seconds.
    pub delay_s: Option<f64>,
    /// `StackMax`.
    pub stack_max: Option<i64>,
    /// `RefreshDuration`.
    pub refresh_duration: Option<bool>,
    /// `PotencyMax`.
    pub potency_max: Option<f64>,
    /// `EffectCategories`.
    pub categories: Option<String>,
    /// `RemoveStatusEffectCategories`.
    pub remove_categories: Option<String>,
    /// `RemoveStatusEffects`.
    pub remove_effects: Option<String>,
    /// `AntirequisiteEffectCategories`.
    pub antirequisite_categories: Option<String>,
    /// `Health`.
    pub health: Option<f64>,
    /// `HealthModifierPercent`.
    pub health_share: Option<f64>,
    /// `HealthModifierDamageBased`.
    pub hit_share: Option<f64>,
    /// `Stamina`.
    pub stamina: Option<f64>,
    /// `DamageType`.
    pub damage_type: Option<String>,
    /// `RemoveOnDeath` is set.
    pub remove_on_death: bool,
    /// `RemoveOnRespawn` is set.
    pub remove_on_respawn: bool,
    /// `DisablePauseOnPersist`.
    pub runs_offline: bool,
    /// `AllowAddIfDead`.
    pub allow_add_if_dead: bool,
    /// `UseLightweightReplication`.
    pub lightweight: bool,
    /// `IsClientPredicted`.
    pub client_predicted: bool,
    /// `StopOnHitCount`.
    pub stop_on_hit_count: Option<i64>,
    /// `HitCondition`.
    pub hit_condition: Option<String>,
    /// `OnEndStatusEffect`.
    pub on_end: Option<String>,
    /// `OnTickStatusEffect`.
    pub on_tick: Option<String>,
    /// `EffectPotencyMods`.
    pub potency_mods: Option<String>,
    /// `EffectDurationMods`.
    pub duration_mods: Option<String>,
    /// The `Abs*` columns: (damage type `TypeID`, value).
    pub absorption: Vec<(String, f64)>,
    /// The `Dmg*` columns: (damage type `TypeID`, value).
    pub damage_mods: Vec<(String, f64)>,
    /// The `EFF_*` columns: (gathering tradeskill, value).
    pub gather_efficiency: Vec<(String, f64)>,
    /// `ItemClassWeightMods`.
    pub item_weight_mods: Option<String>,
    /// `Encumbrance`.
    pub encumbrance: Option<i64>,
    /// The `MGS_*` and `MaxGS_*` columns: (crafting tradeskill, least, most).
    pub craft_gear_score: Vec<(String, i64, i64)>,
    /// The `WKN*` columns: (damage type `TypeID`, value).
    pub weakness: Vec<(String, f64)>,
    /// `StaminaRate`.
    pub stamina_rate: Option<f64>,
    /// `StaminaDamageModifier`.
    pub block_stamina_mod: Option<f64>,
    /// The `MOD*` columns, in [`crate::character_points::ATTRIBUTE_NAMES`] order.
    pub attribute_mods: [Option<i64>; ATTRIBUTE_COUNT],
    /// `CritChanceModifier`.
    pub crit_chance: Option<f64>,
    /// `CritDamageModifier`.
    pub crit_damage: Option<f64>,
    /// `HealMod`.
    pub heal_mod: Option<f64>,
    /// `ConsumableHealMod`.
    pub consumable_heal_mod: Option<f64>,
    /// `PhysicalArmor`.
    pub physical_armor: Option<f64>,
    /// `ElementalArmor`.
    pub elemental_armor: Option<f64>,
    /// `HealthRate`.
    pub health_rate: Option<f64>,
    /// The control flag columns.
    pub control: Control,
    /// `RemoveOnDeathsDoor` is set.
    pub remove_on_deaths_door: bool,
    /// `OnDeathStatusEffect`.
    pub on_death: Option<String>,
    /// `InitialStackSize`.
    pub initial_stacks: Option<i64>,
    /// `GameEventID`.
    pub game_event: Option<String>,
    /// `ShowTextInDamageNumbers`.
    pub shows_damage_text: bool,
    /// `StackDuration` is above 0.
    pub stack_duration: bool,
    /// The reward and tradeskill bonus columns.
    pub rewards: RewardMods,
}

impl EffectSourceRow {
    /// The rule content of the datasheet row `row`.
    fn of(row: &StatusEffectData) -> Self {
        let set = |value: Option<i64>| value.is_some_and(|value| value != 0);
        let absorption = [
            row.abs_acid,
            row.abs_arcane,
            row.abs_corruption,
            row.abs_fire,
            row.abs_ice,
            row.abs_lightning,
            row.abs_nature,
            row.abs_siege,
            row.abs_slash,
            row.abs_standard,
            row.abs_strike,
            row.abs_thrust,
        ];
        let damage = [
            row.dmg_acid,
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
        ];
        // The weakness columns: every damage type but acid has one.
        let weakness = [
            None,
            row.wkn_arcane,
            row.wkn_corruption,
            row.wkn_fire,
            row.wkn_ice,
            row.wkn_lightning,
            row.wkn_nature,
            row.wkn_siege,
            row.wkn_slash,
            row.wkn_standard,
            row.wkn_strike,
            row.wkn_thrust,
        ];
        let by_type = |values: [Option<f64>; DAMAGE_TYPE_COUNT]| -> Vec<(String, f64)> {
            DAMAGE_TYPE_COLUMNS
                .iter()
                .zip(values)
                .filter_map(|(name, value)| {
                    value
                        .filter(|value| *value != 0.0)
                        .map(|value| ((*name).to_owned(), value))
                })
                .collect()
        };
        Self {
            status_id: row.status_id.clone(),
            base_duration_s: row.base_duration,
            duration_max_s: row.duration_max,
            tick_s: row.tick_rate,
            delay_s: row.delay_initial_tick,
            stack_max: row.stack_max,
            refresh_duration: row.refresh_duration,
            potency_max: row.potency_max,
            categories: row.effect_categories.clone(),
            remove_categories: row.remove_status_effect_categories.clone(),
            remove_effects: row.remove_status_effects.clone(),
            antirequisite_categories: row.antirequisite_effect_categories.clone(),
            // Health values of a few thousand.
            health: row.health.map(|health| health as f64),
            health_share: row.health_modifier_percent,
            hit_share: row.health_modifier_damage_based,
            stamina: row.stamina,
            damage_type: row.damage_type.clone(),
            remove_on_death: set(row.remove_on_death),
            remove_on_respawn: set(row.remove_on_respawn),
            runs_offline: row.disable_pause_on_persist.unwrap_or(false),
            allow_add_if_dead: row.allow_add_if_dead.unwrap_or(false),
            lightweight: row.use_lightweight_replication.unwrap_or(false),
            client_predicted: row.is_client_predicted.unwrap_or(false),
            stop_on_hit_count: row.stop_on_hit_count,
            hit_condition: row.hit_condition.clone(),
            on_end: row.on_end_status_effect.clone(),
            on_tick: row.on_tick_status_effect.clone(),
            potency_mods: row.effect_potency_mods.clone(),
            duration_mods: row.effect_duration_mods.clone(),
            absorption: by_type(absorption),
            damage_mods: by_type(damage),
            item_weight_mods: row.item_class_weight_mods.clone(),
            encumbrance: row.encumbrance,
            gather_efficiency: GATHER_EFFICIENCY_COLUMNS
                .iter()
                .zip([
                    row.eff_harvesting,
                    row.eff_logging,
                    row.eff_mining,
                    row.eff_skinning,
                ])
                .filter_map(|(skill, value)| {
                    value
                        .filter(|value| *value != 0.0)
                        .map(|value| ((*skill).to_owned(), value))
                })
                .collect(),
            craft_gear_score: CRAFT_GEAR_SCORE_COLUMNS
                .iter()
                .zip([
                    (row.mgs_arcana, row.max_gs_arcana),
                    (row.mgs_armoring, row.max_gs_armoring),
                    (row.mgs_engineering, row.max_gs_engineering),
                    (row.mgs_jewelcrafting, row.max_gs_jewelcrafting),
                    (row.mgs_weaponsmithing, row.max_gs_weaponsmithing),
                ])
                .filter(|(_, (least, most))| least.is_some() || most.is_some())
                .map(|(skill, (least, most))| {
                    (
                        (*skill).to_owned(),
                        least.unwrap_or_default(),
                        most.unwrap_or_default(),
                    )
                })
                .collect(),
            weakness: by_type(weakness),
            stamina_rate: row.stamina_rate,
            block_stamina_mod: row.stamina_damage_modifier,
            attribute_mods: [
                row.mod_constitution,
                row.mod_focus,
                row.mod_strength,
                row.mod_dexterity,
                row.mod_intelligence,
            ],
            crit_chance: row.crit_chance_modifier,
            crit_damage: row.crit_damage_modifier,
            heal_mod: row.heal_mod,
            consumable_heal_mod: row.consumable_heal_mod,
            physical_armor: row.physical_armor,
            elemental_armor: row.elemental_armor,
            health_rate: row.health_rate,
            control: Control {
                no_dodge: set(row.no_dodge),
                stunned: set(row.stunned),
                silenced: set(row.silenced),
                no_sprint: set(row.no_sprint),
                no_health_regen: set(row.no_health_regen),
            },
            remove_on_deaths_door: set(row.remove_on_deaths_door),
            on_death: row.on_death_status_effect.clone(),
            initial_stacks: row.initial_stack_size,
            game_event: row.game_event_id.clone().filter(|event| !event.is_empty()),
            shows_damage_text: row.show_text_in_damage_numbers.unwrap_or(false),
            stack_duration: row.stack_duration.is_some_and(|stack| stack > 0.0),
            rewards: RewardMods::of(row),
        }
    }
}

/// One attack row that puts status effects on its target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitEffectRow {
    /// The unique id of its damage table.
    pub table: String,
    /// `DamageID`.
    pub damage_id: String,
    /// `StatusEffect`: the effect ids, `,`-separated; a repeated id adds a stack.
    pub status_effects: String,
}

/// The plain content of the status effect sheets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusEffectSource {
    /// Every `StatusEffectData` row of every table, in table order.
    pub effects: Vec<EffectSourceRow>,
    /// The `DamageTypeData` rows: (`TypeID`, `IntID`).
    pub damage_types: Vec<(String, u8)>,
    /// The attack rows that put effects on their target.
    pub hit_effects: Vec<HitEffectRow>,
}

impl StatusEffectSource {
    /// The status effect content of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self {
            effects: tables
                .status_effect_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(EffectSourceRow::of)
                .collect(),
            damage_types: tables
                .damage_type_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| Some((row.type_id.clone(), u8::try_from(row.int_id).ok()?)))
                .collect(),
            hit_effects: tables
                .damage_data
                .iter()
                .flat_map(|table| table.rows.iter().map(move |row| (table, row)))
                .filter_map(|(table, row)| {
                    Some(HitEffectRow {
                        table: table.unique_id.clone(),
                        damage_id: row.damage_id.clone(),
                        status_effects: row
                            .status_effect
                            .clone()
                            .filter(|cell| !cell.trim().is_empty())?,
                    })
                })
                .collect(),
        }
    }
}

/// An attack row by the CRC-32 of its lowercase table and row ids: a small key that hit records
/// can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowKey {
    /// CRC-32 of the lowercase unique id of its damage table.
    pub table: u32,
    /// CRC-32 of the lowercase `DamageID`.
    pub row: u32,
}

impl RowKey {
    /// The key of row `damage_id` of table `table` (any case).
    pub fn of(table: &str, damage_id: &str) -> Self {
        Self {
            table: lowercase_crc32(table),
            row: lowercase_crc32(damage_id),
        }
    }
}

/// The rules of every status effect.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusEffectRules {
    /// Each effect, by the CRC-32 of its lowercase id (the first row of a repeated id).
    by_crc: HashMap<u32, StatusEffectRule>,
    /// The effects each attack row puts on its target.
    on_hit: HashMap<RowKey, Vec<u32>>,
}

impl StatusEffectRules {
    /// The rules of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(StatusEffectSource::from_tables(tables))
    }

    /// The rules of `source`.
    pub fn build(source: StatusEffectSource) -> Self {
        let mut sorted: Vec<u32> = source
            .effects
            .iter()
            .map(|row| lowercase_crc32(&row.status_id))
            .collect();
        sorted.sort_unstable();
        let damage_types: HashMap<String, u8> = source
            .damage_types
            .iter()
            .map(|(name, id)| (name.to_lowercase(), *id))
            .collect();
        let mut by_crc = HashMap::new();
        for row in &source.effects {
            let crc = lowercase_crc32(&row.status_id);
            if by_crc.contains_key(&crc) {
                continue;
            }
            // The first position of the CRC among the 4,517 sorted rows: below 2^16.
            let Ok(key) = u16::try_from(sorted.partition_point(|other| *other < crc)) else {
                continue;
            };
            by_crc.insert(crc, rule(row, crc, key, &damage_types));
        }
        let on_hit = source
            .hit_effects
            .iter()
            .map(|row| {
                (
                    RowKey::of(&row.table, &row.damage_id),
                    row.status_effects
                        .split(HIT_EFFECT_SEPARATOR)
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                        .map(lowercase_crc32)
                        .collect(),
                )
            })
            .collect();
        Self { by_crc, on_hit }
    }

    /// The effect of CRC-32 `crc` (of its lowercase id).
    pub fn get(&self, crc: u32) -> Option<&StatusEffectRule> {
        self.by_crc.get(&crc)
    }

    /// The effect of id `id` (any case).
    pub fn by_id(&self, id: &str) -> Option<&StatusEffectRule> {
        self.get(lowercase_crc32(id))
    }

    /// The effects the attack row `row` puts on its target, one entry per stack.
    pub fn on_hit(&self, row: RowKey) -> &[u32] {
        self.on_hit.get(&row).map_or(&[], Vec::as_slice)
    }

    /// The number of effects.
    pub fn len(&self) -> usize {
        self.by_crc.len()
    }

    /// Whether there is no effect.
    pub fn is_empty(&self) -> bool {
        self.by_crc.is_empty()
    }
}

/// The rule of `row`, the effect of CRC-32 `crc` and wire key `key`.
fn rule(
    row: &EffectSourceRow,
    crc: u32,
    key: u16,
    damage_types: &HashMap<String, u8>,
) -> StatusEffectRule {
    let ms = |seconds: f64| (seconds * MS_PER_SECOND).round() as u64;
    let positive_ms = |seconds: Option<f64>| seconds.filter(|seconds| *seconds > 0.0).map(ms);
    let type_of = |name: &str| damage_types.get(&name.trim().to_lowercase()).copied();
    let typed = |values: &[(String, f64)]| {
        values
            .iter()
            .filter_map(|(name, value)| Some((type_of(name)?, *value)))
            .collect()
    };
    StatusEffectRule {
        id: row.status_id.clone(),
        crc,
        key,
        duration_ms: positive_ms(row.base_duration_s),
        duration_max_ms: positive_ms(row.duration_max_s),
        tick_ms: positive_ms(row.tick_s),
        first_tick_ms: positive_ms(row.delay_s).unwrap_or(0),
        stack_max: row
            .stack_max
            .and_then(|max| u8::try_from(max).ok())
            .filter(|max| *max > 0)
            .unwrap_or(DEFAULT_STACK_MAX),
        refresh_duration: row.refresh_duration != Some(false),
        potency_max: row.potency_max.filter(|max| *max > 0.0),
        categories: names(row.categories.as_deref()),
        remove_categories: names(row.remove_categories.as_deref()),
        remove_effects: ids(row.remove_effects.as_deref()),
        blocked_by: names(row.antirequisite_categories.as_deref()),
        health: row.health.unwrap_or(0.0),
        health_share: row.health_share.unwrap_or(0.0),
        hit_share: row.hit_share.unwrap_or(0.0),
        stamina: row.stamina.unwrap_or(0.0),
        damage_type: row.damage_type.as_deref().and_then(type_of),
        remove_on_death: row.remove_on_death,
        remove_on_respawn: row.remove_on_respawn,
        runs_offline: row.runs_offline,
        allow_add_if_dead: row.allow_add_if_dead,
        lightweight: row.lightweight,
        client_predicted: row.client_predicted,
        stop_on_hits_taken: (row.hit_condition.as_deref() == Some(HIT_TAKEN_CONDITION))
            .then_some(row.stop_on_hit_count)
            .flatten()
            .and_then(|count| u32::try_from(count).ok())
            .filter(|count| *count > 0),
        on_end: ids(row.on_end.as_deref()),
        on_tick: ids(row.on_tick.as_deref()),
        potency_mods: mods(row.potency_mods.as_deref()),
        duration_mods: mods(row.duration_mods.as_deref()),
        absorption: typed(&row.absorption),
        damage_mods: typed(&row.damage_mods),
        gather_efficiency: row.gather_efficiency.clone(),
        item_weight_mods: mods(row.item_weight_mods.as_deref()),
        // Encumbrance bonuses of a few thousand.
        encumbrance: row.encumbrance.map_or(0.0, |bonus| bonus as f64),
        craft_gear_score: row.craft_gear_score.clone(),
        weakness: typed(&row.weakness),
        stamina_rate: row.stamina_rate.unwrap_or(0.0),
        block_stamina_mod: row.block_stamina_mod.unwrap_or(0.0),
        attribute_mods: row.attribute_mods.map(Option::unwrap_or_default),
        crit_chance: row.crit_chance.unwrap_or(0.0),
        crit_damage: row.crit_damage.unwrap_or(0.0),
        heal_mod: row.heal_mod.unwrap_or(0.0),
        consumable_heal_mod: row.consumable_heal_mod.unwrap_or(0.0),
        physical_armor: row.physical_armor.unwrap_or(0.0),
        elemental_armor: row.elemental_armor.unwrap_or(0.0),
        health_rate: row.health_rate.unwrap_or(0.0),
        control: row.control,
        remove_on_deaths_door: row.remove_on_deaths_door,
        on_death: ids(row.on_death.as_deref()),
        initial_stacks: row
            .initial_stacks
            .and_then(|stacks| u8::try_from(stacks).ok())
            .filter(|stacks| *stacks > 1),
        game_event: row.game_event.clone(),
        shows_damage_text: row.shows_damage_text,
        stack_duration: row.stack_duration,
        rewards: row.rewards.clone(),
    }
}

/// The lowercase names of a `+`-separated cell.
pub(crate) fn names(cell: Option<&str>) -> Vec<String> {
    cell.unwrap_or_default()
        .split(LIST_SEPARATOR)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The CRC-32 of the lowercase ids of a `+`-separated cell.
fn ids(cell: Option<&str>) -> Vec<u32> {
    names(cell).iter().map(|id| lowercase_crc32(id)).collect()
}

/// The (lowercase category, value) pairs of a mod cell (`Root=-0.2+Stun=-0.2`); an entry without
/// a number is left out.
fn mods(cell: Option<&str>) -> Vec<(String, f64)> {
    names(cell)
        .iter()
        .filter_map(|entry| {
            let (category, value) = entry.split_once(MOD_SEPARATOR)?;
            Some((category.trim().to_owned(), value.trim().parse().ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DamageTypeData` `Slash` (newchar retail capture: light sword items tag 4).
    const SLASH: u8 = 4;
    /// `DamageTypeData` `Standard`.
    const STANDARD: u8 = 1;

    fn row(id: &str) -> EffectSourceRow {
        EffectSourceRow {
            status_id: id.to_owned(),
            ..EffectSourceRow::default()
        }
    }

    fn source() -> StatusEffectSource {
        StatusEffectSource {
            effects: vec![
                // The retail `AI_Bleed_9` row.
                EffectSourceRow {
                    base_duration_s: Some(10.0),
                    tick_s: Some(2.0),
                    stack_max: Some(1),
                    categories: Some("Debuff+Bleed+Dot".to_owned()),
                    health_share: Some(-0.015),
                    hit_share: Some(-0.05),
                    damage_type: Some("Standard".to_owned()),
                    remove_on_death: true,
                    ..row("AI_Bleed_9")
                },
                // The retail `EquipLoad_CCDurReduction_Heavy` row.
                EffectSourceRow {
                    base_duration_s: Some(-1.0),
                    duration_mods: Some("Root=-0.2+Stun=-0.2+Slow=-0.2".to_owned()),
                    lightweight: true,
                    client_predicted: true,
                    ..row("EquipLoad_CCDurReduction_Heavy")
                },
                // The retail `Status_OutOfCombat_PassiveRegen` row.
                EffectSourceRow {
                    base_duration_s: Some(180.0),
                    delay_s: Some(1.0),
                    tick_s: Some(1.0),
                    health: Some(40.0),
                    potency_mods: Some("PassiveFoodRegen=2".to_owned()),
                    stop_on_hit_count: Some(1),
                    hit_condition: Some("OnHitTaken".to_owned()),
                    remove_on_respawn: true,
                    absorption: vec![("Slash".to_owned(), 0.5)],
                    ..row("Status_OutOfCombat_PassiveRegen")
                },
                // A repeated id, as retail `Sanctuary` in two tables.
                row("Sanctuary"),
                EffectSourceRow {
                    stack_max: Some(3),
                    ..row("Sanctuary")
                },
            ],
            damage_types: vec![
                ("Slash".to_owned(), SLASH),
                ("Standard".to_owned(), STANDARD),
            ],
            hit_effects: vec![HitEffectRow {
                table: "WolfDamageTable".to_owned(),
                damage_id: "Bite".to_owned(),
                status_effects: "AI_Bleed_9, AI_Bleed_9".to_owned(),
            }],
        }
    }

    #[test]
    fn a_row_gives_its_weakness_stamina_attribute_heal_armor_and_control_columns() {
        let mut source = source();
        source.effects.push(EffectSourceRow {
            status_id: "Greatsword_Stance_Offense".to_owned(),
            weakness: vec![("Slash".to_owned(), 0.15), ("Nowhere".to_owned(), 1.0)],
            stamina_rate: Some(-0.15),
            block_stamina_mod: Some(0.2),
            attribute_mods: [Some(18), None, Some(12), None, None],
            heal_mod: Some(-0.05),
            physical_armor: Some(-0.1),
            control: Control {
                stunned: true,
                ..Control::default()
            },
            remove_on_deaths_door: true,
            on_death: Some("AI_Bleed_9".to_owned()),
            initial_stacks: Some(5),
            game_event: Some("Mount_GrantRidingXP_Small".to_owned()),
            ..EffectSourceRow::default()
        });
        let rules = StatusEffectRules::build(source);
        let rule = rules.by_id("Greatsword_Stance_Offense").expect("rule");
        assert_eq!(
            rule.weakness,
            vec![(SLASH, 0.15)],
            "an unknown type is left out"
        );
        assert_eq!(rule.stamina_rate, -0.15);
        assert_eq!(rule.block_stamina_mod, 0.2);
        assert_eq!(rule.attribute_mods, [18, 0, 12, 0, 0]);
        assert_eq!((rule.heal_mod, rule.physical_armor), (-0.05, -0.1));
        assert!(!rule.control.may_dodge() && !rule.control.may_cast() && !rule.control.may_fight());
        assert!(rule.remove_on_deaths_door);
        assert_eq!(rule.on_death, vec![lowercase_crc32("ai_bleed_9")]);
        assert_eq!(rule.initial_stacks, Some(5));
        assert_eq!(
            rule.game_event.as_deref(),
            Some("Mount_GrantRidingXP_Small")
        );
        let bleed = rules.by_id("AI_Bleed_9").expect("bleed");
        assert!(!bleed.control.any() && bleed.initial_stacks.is_none());
    }

    #[test]
    fn a_key_is_the_first_place_of_the_crc_among_every_row() {
        let rules = StatusEffectRules::build(source());
        let mut crcs: Vec<u32> = source()
            .effects
            .iter()
            .map(|row| lowercase_crc32(&row.status_id))
            .collect();
        crcs.sort_unstable();
        for id in ["AI_Bleed_9", "Sanctuary", "Status_OutOfCombat_PassiveRegen"] {
            let rule = rules.by_id(id).expect("rule");
            let first = crcs
                .iter()
                .position(|crc| *crc == rule.crc)
                .expect("listed");
            assert_eq!(usize::from(rule.key), first, "{id}");
        }
        assert_eq!(rules.len(), 4, "a repeated id is one effect");
        assert_eq!(
            rules.by_id("sanctuary").map(|rule| rule.stack_max),
            Some(DEFAULT_STACK_MAX),
            "the first row of a repeated id"
        );
    }

    #[test]
    fn the_rows_give_durations_ticks_mods_and_hit_effects() {
        let rules = StatusEffectRules::build(source());
        let bleed = rules.by_id("ai_bleed_9").expect("bleed");
        assert_eq!(bleed.duration_ms, Some(10_000));
        assert_eq!(bleed.tick_ms, Some(2_000));
        assert_eq!(bleed.damage_type, Some(STANDARD));
        assert!(bleed.has_category("dot") && bleed.changes_vitals());
        assert!(bleed.refresh_duration, "an empty RefreshDuration refreshes");
        let load = rules.by_id("EquipLoad_CCDurReduction_Heavy").expect("load");
        assert_eq!(load.duration_ms, None);
        assert!(load.lightweight && load.client_predicted);
        assert_eq!(
            load.duration_mods,
            vec![
                ("root".to_owned(), -0.2),
                ("stun".to_owned(), -0.2),
                ("slow".to_owned(), -0.2)
            ]
        );
        let regen = rules
            .by_id("Status_OutOfCombat_PassiveRegen")
            .expect("regen");
        assert_eq!(regen.first_tick_ms, 1_000);
        assert_eq!(regen.stop_on_hits_taken, Some(1));
        assert_eq!(
            regen.potency_mods,
            vec![("passivefoodregen".to_owned(), 2.0)]
        );
        assert_eq!(regen.absorption, vec![(SLASH, 0.5)]);
        assert_eq!(
            rules.on_hit(RowKey::of("wolfdamagetable", "BITE")),
            [bleed.crc, bleed.crc].as_slice(),
            "each repeat is one stack"
        );
        assert!(
            rules
                .on_hit(RowKey::of("WolfDamageTable", "Claw"))
                .is_empty()
        );
    }

    /// A `StatusEffectData` row of the cells `cells` (JSON names), the others empty.
    fn data_row(cells: serde_json::Value) -> StatusEffectData {
        serde_json::from_value(cells).expect("a status effect row")
    }

    #[test]
    fn the_bonus_columns_give_progression_yield_luck_and_reward_shares() {
        // Build 25810808 `Token_Weapon_1` (cut to two masteries), `Token_Territory_1` (two
        // standings), `Token_Gather_1`, `Token_Proficiency_1`, `PvP_Flag_Bonus` and
        // `Status_Perk_Bags_FactionGain`, as one made-up row.
        let row = data_row(serde_json::json!({
            "status_id": "Test_AllBonuses",
            "exp_categorical_progression": "SwordAbilityTable:1,HatchetAbilityTable:1,2:1,x,4:",
            "exp_mining": 1.0,
            "exp_woodworking": "1.4999999999999999E-2",
            "mult_mining": 0.1,
            "rol_mining": 3000,
            "ins_mining": 0.25,
            "ins_skinning": "0.5",
            "global_roll_mod": 1000.0,
            "faction_reputation_mod": 0.03,
            "faction_tokens_mod": 0.03,
            "territory_standing_mod": 0.05,
            "azoth_mod": 0.03,
            "loot_tags": "ToolMoteFire",
            "stack_duration": 1.0,
        }));
        let source = EffectSourceRow::of(&row);
        assert!(source.stack_duration);
        let mods = &source.rewards;
        assert_eq!(mods.progression_share("swordabilitytable"), 1.0);
        assert_eq!(mods.progression_share("2"), 1.0);
        assert_eq!(mods.progression_share("4"), 0.0, "an entry without a share");
        assert_eq!(mods.progression_share("Mining"), 1.0);
        assert!((mods.progression_share("Woodworking") - 0.015).abs() < 1e-12);
        assert_eq!(mods.progression_share("Skinning"), 0.0);
        assert_eq!(mods.yield_share("mining"), 0.1);
        assert_eq!(mods.luck_of("Mining"), 3_000.0);
        assert_eq!(mods.luck_of("Logging"), 0.0);
        assert_eq!(mods.global_luck, 1_000.0);
        assert_eq!(mods.instant_gather_chance("Mining"), 0.25);
        assert_eq!(
            mods.instant_gather_chance("skinning"),
            0.5,
            "a number in a text column"
        );
        assert_eq!(mods.instant_gather_chance("Logging"), 0.0);
        assert_eq!(
            (
                mods.faction_reputation,
                mods.faction_tokens,
                mods.territory_standing,
                mods.azoth
            ),
            (0.03, 0.03, 0.05, 0.03)
        );
        assert_eq!(mods.loot_tags, ["ToolMoteFire"]);
        // A text that is no number in `EXP_Woodworking` (`Mut_Base_Curse4_Minor_Silence`).
        let silence = data_row(serde_json::json!({
            "status_id": "Test_Silence",
            "exp_woodworking": "StatFX_Silence_ON",
        }));
        assert_eq!(EffectSourceRow::of(&silence).rewards, RewardMods::default());
    }
}
