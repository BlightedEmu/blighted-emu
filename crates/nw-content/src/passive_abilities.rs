// SPDX-License-Identifier: AGPL-3.0-or-later
//! The passive abilities that combat events trigger: the `AbilityData` rows with `OnHit`,
//! `OnCrit`, `OnKill`, `OnHitTaken`, `OnBlockedHit`, `OnBlockedHitTaken`, `OnBlockBreak`,
//! `OnTargetBlockBreak`, `OnHeadShot` or `OnHitBehind` set (1,249 rows of the player tables of build 25810808: weapon mastery
//! passives and upgrades, item, gem, artifact and set perks, attribute thresholds).
//!
//! A row is a trigger, the conditions of the event it needs and the effects it has. The engine
//! reads these columns (column names in the data, **Inferred** meaning from the names and the
//! values of the 1,249 rows, no retail capture of a passive):
//!
//! - Conditions: `ActivationCooldown` (seconds between two firings), `WeaponTag` (a tag of the
//!   held weapon's stance, `Rifle`, `Axe`), `AttackType` and `IgnoreDisabledAttackTypes` (the
//!   hit's `DamageData.AttackType`), `DamageTableRow` (the hit's damage row), `DamageIsMelee`,
//!   `DamageIsRanged` (`DamageData.IsRanged`), `DamageTypes`, `TargetHealthPercent`,
//!   `MyHealthPercent`, `MyStaminaPercent`, `MyManaPercent` with their comparisons,
//!   `TargetStatusEffect` / `TargetStatusEffectCategory` with `TargetStatusEffectComparison`
//!   and `TargetStatusEffectStackSize` (`NotEqual`: the other side holds none; a count: at
//!   least or exactly that many stacks; no comparison: holds one), `StatusEffect` /
//!   `StatusEffectCategories` the same on the own side (`CheckOwnedStatusEffects`,
//!   `CheckStatusEffectsOnTargetOwned`: only the effects the holder applied), `DontHaveStatusEffect`,
//!   `EquipLoadCategory`,
//!   `TargetVitalsCategory`, `AttackerVitalsCategory`, `DistFromDefender`,
//!   `RequiredAbilityID` and `RequiredEquippedAbilityID` (held abilities),
//!   `ExcludeFromGameModes`, and `RequireReaction` (the hit dealt damage).
//! - Effects: `SelfApplyStatusEffect`, `OtherApplyStatusEffect`, `RemoveStatusEffects`,
//!   `BaseDamage` (a share added to the hit's damage), `BaseDamageReduction` (a share taken from
//!   a hit taken), `CritChance`, `CritDamage`, `HeadshotDamage` and `HitFromBehindDamage` (a
//!   share added to a head shot, to a hit from behind), `DmgPctToHealth` (a share of the hit healed),
//!   `Mana` (a share of the mana max), `Stamina` (points), `Knockback` (metres the target goes
//!   back), and `CooldownTimer` seconds for
//!   `CooldownID` under `CDRImmediatelyOptions`.
//!
//! A weapon tree node without a trigger whose effects all change a hit (`BaseDamage`,
//! `BaseDamageReduction`, `CritChance`, `CritDamage`: the fire staff's
//! `Passive_Firestaff_FireMage_CritDmg`, the flame thrower's `Upgrade_Firestaff_FlameThrower_Dmg`)
//! holds while its conditions hold: every hit of the holder and every hit on it fire it
//! ([`ALWAYS_ON_TRIGGERS`]; **Inferred**: the row names no event, and its effects only change a
//! hit). An active ability's own row (`IsActiveAbility`) is no passive.
//!
//! A row with any other condition column ([`PassiveAbility::unread_conditions`]) never fires:
//! the engine cannot tell whether it holds. Effect columns the engine does not carry out are
//! listed in [`PassiveAbility::unread_effects`].

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;
use crate::generated::AbilityData;
use crate::status_effects::RowKey;

/// Separates the entries of a list cell.
const LIST_SEPARATOR: char = ',';
/// A health, stamina or mana cell is a percent.
const PERCENT: f64 = 100.0;
/// Milliseconds per second of an `ActivationCooldown`.
const MS_PER_SECOND: f64 = 1000.0;
/// The copies of an ability that count when its row does not stack.
const ONE_COPY: u32 = 1;
/// The `DamageData.IsRanged` cell of a ranged row (the others write `0` or nothing).
const RANGED_CELL: &str = "1";
/// The `CDRImmediatelyOptions` that shortens every ability of the held weapon.
const ACTIVE_WEAPON_SCOPE: &str = "ActiveWeapon";
/// The `CDRImmediatelyOptions` that shortens every ability but the named one.
const ALL_EXCEPT_SCOPE: &str = "AllExcept";

/// The events that fire a node that names none: the holder's hits and the hits on it.
pub const ALWAYS_ON_TRIGGERS: [Trigger; 2] = [Trigger::Hit, Trigger::HitTaken];

/// A combat event that fires passive abilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// The holder hit a target (`OnHit`).
    Hit,
    /// The holder's hit was a crit (`OnCrit`).
    Crit,
    /// The holder's hit killed its target (`OnKill`).
    Kill,
    /// The holder took a hit (`OnHitTaken`).
    HitTaken,
    /// The target blocked the holder's hit (`OnBlockedHit`).
    BlockedHit,
    /// The holder blocked a hit (`OnBlockedHitTaken`).
    BlockedHitTaken,
    /// The holder's guard broke (`OnBlockBreak`).
    BlockBreak,
    /// The holder broke the target's guard (`OnTargetBlockBreak`).
    TargetBlockBreak,
    /// The holder's ranged hit was a head shot (`OnHeadShot`).
    HeadShot,
    /// The holder hit the target from behind (`OnHitBehind`).
    HitBehind,
}

impl Trigger {
    /// Whether the holder made the hit of this event (else it took it).
    pub const fn by_holder(self) -> bool {
        matches!(
            self,
            Self::Hit
                | Self::Crit
                | Self::Kill
                | Self::BlockedHit
                | Self::TargetBlockBreak
                | Self::HeadShot
                | Self::HitBehind
        )
    }
}

/// A comparison of a condition cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    /// `LessThan`.
    Less,
    /// `LessThanOrEqual`.
    LessOrEqual,
    /// `Equal`.
    Equal,
    /// `NotEqual`.
    NotEqual,
    /// `GreaterThanOrEqual`.
    GreaterOrEqual,
    /// `GreaterThan`.
    Greater,
}

impl Compare {
    /// The comparison a cell names; the case varies and two spellings are wrong in the data
    /// (`GreatherThanOrEqual`, `GreaterThanorEqual`). `None` for another word.
    pub fn of(cell: &str) -> Option<Self> {
        match cell.trim().to_ascii_lowercase().as_str() {
            "lessthan" => Some(Self::Less),
            "lessthanorequal" => Some(Self::LessOrEqual),
            "equal" => Some(Self::Equal),
            "notequal" => Some(Self::NotEqual),
            "greaterthanorequal" | "greatherthanorequal" => Some(Self::GreaterOrEqual),
            "greaterthan" => Some(Self::Greater),
            _ => None,
        }
    }

    /// Whether `value` compares to `limit` so.
    pub fn holds(self, value: f64, limit: f64) -> bool {
        match self {
            Self::Less => value < limit,
            Self::LessOrEqual => value <= limit,
            Self::Equal => (value - limit).abs() < f64::EPSILON,
            Self::NotEqual => (value - limit).abs() >= f64::EPSILON,
            Self::GreaterOrEqual => value >= limit,
            Self::Greater => value > limit,
        }
    }
}

/// The side of an event a condition or an effect is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The ability's holder.
    Holder,
    /// The other one: the target of the holder's hit, or the attacker of a hit it took.
    Other,
}

/// A vital of a side, as a share of its max.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vital {
    /// Health.
    Health,
    /// Stamina.
    Stamina,
    /// Mana.
    Mana,
}

/// What status effects a condition looks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectMatch {
    /// Effects of these `StatusID`s.
    Ids(Vec<String>),
    /// Effects of these categories.
    Categories(Vec<String>),
}

/// How many stacks of the matched effects a condition needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StackRule {
    /// At least one.
    Any,
    /// None (`NotEqual` without a count).
    None,
    /// The stack count compares to the count.
    Count(Compare, u32),
}

impl StackRule {
    /// The rule of a comparison cell and a stack count cell.
    fn of(compare: Option<&str>, stacks: Option<i64>) -> Self {
        let count = stacks.and_then(|stacks| u32::try_from(stacks).ok());
        match (compare.and_then(Compare::of), count) {
            (Some(Compare::NotEqual), None) => Self::None,
            (Some(compare), Some(count)) => Self::Count(compare, count),
            _ => Self::Any,
        }
    }

    /// Whether `stacks` stacks meet it.
    pub fn holds(self, stacks: u32) -> bool {
        match self {
            Self::Any => stacks > 0,
            Self::None => stacks == 0,
            Self::Count(compare, count) => compare.holds(f64::from(stacks), f64::from(count)),
        }
    }
}

/// One condition of an event.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// A stance tag of the held weapon (`WeaponTag`).
    WeaponTag(String),
    /// The hit's attack type is one of these (`AttackType`).
    AttackTypes(Vec<String>),
    /// The hit's attack type is none of these (`IgnoreDisabledAttackTypes`).
    NotAttackTypes(Vec<String>),
    /// The hit's damage row is one of these (`DamageTableRow`), by CRC-32 of the lowercase
    /// `DamageID` ([`RowKey::row`]).
    DamageRows(Vec<u32>),
    /// The hit is melee (`DamageIsMelee`).
    Melee,
    /// The hit is ranged (`DamageIsRanged`).
    Ranged,
    /// The hit's damage type is one of these (`DamageTypes`).
    DamageTypes(Vec<String>),
    /// A vital of a side compares to a share of its max.
    Vital {
        /// Whose vital.
        side: Side,
        /// Which vital.
        vital: Vital,
        /// How it compares.
        compare: Compare,
        /// The share of the max it compares to.
        share: f64,
    },
    /// A side holds the matched effects by the stack rule.
    Effects {
        /// Whose effects.
        side: Side,
        /// Which effects.
        matched: EffectMatch,
        /// How many stacks.
        stacks: StackRule,
        /// Only the effects the holder applied count (`CheckOwnedStatusEffects` on the own
        /// side, `CheckStatusEffectsOnTargetOwned` on the other).
        owned: bool,
    },
    /// The holder's equip load is one of these (`EquipLoadCategory`: `Fast`, `Normal`,
    /// `Slow`).
    EquipLoads(Vec<String>),
    /// The holder holds none of these effects (`DontHaveStatusEffect`).
    LacksEffects(Vec<String>),
    /// A side's vitals category is this one (`TargetVitalsCategory`, `AttackerVitalsCategory`).
    Category {
        /// Whose category.
        side: Side,
        /// The `VitalsCategory` (`Player`, `Hostile`).
        category: String,
    },
    /// The distance between the two compares to these metres (`DistFromDefender`).
    Distance {
        /// How it compares.
        compare: Compare,
        /// The metres it compares to.
        metres: f64,
    },
    /// The hit dealt damage (`RequireReaction`).
    Reaction,
    /// The world runs none of these game modes (`ExcludeFromGameModes`).
    NotInGameModes(Vec<String>),
}

/// Which cooldowns a cooldown effect changes (`CDRImmediatelyOptions`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CooldownScope {
    /// The ability of this id (`AbilitySpecific`).
    Ability(String),
    /// Every ability of the held weapon (`ActiveWeapon`).
    ActiveWeapon,
    /// Every ability of the held weapon but this one (`AllExcept`).
    AllExcept(String),
}

/// One effect of a fired ability.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Status effects on a side (`SelfApplyStatusEffect`, `OtherApplyStatusEffect`).
    Apply {
        /// On whom.
        side: Side,
        /// The `StatusID`s.
        ids: Vec<String>,
    },
    /// Ends these effects on the holder (`RemoveStatusEffects`).
    RemoveOwn(Vec<String>),
    /// A share added to the holder's hit (`BaseDamage`).
    DamageShare(f64),
    /// A share taken from a hit on the holder (`BaseDamageReduction`).
    TakenShare(f64),
    /// Added to the holder's crit chance for the hit (`CritChance`).
    CritChance(f64),
    /// Added to the holder's crit multiplier for the hit (`CritDamage`).
    CritDamage(f64),
    /// A share added to the holder's head shot (`HeadshotDamage`).
    HeadshotShare(f64),
    /// A share added to the holder's hit from behind (`HitFromBehindDamage`).
    BehindShare(f64),
    /// A share of the hit's damage healed (`DmgPctToHealth`).
    LifeSteal(f64),
    /// A share of the mana max given (`Mana`; below 0 taken).
    ManaShare(f64),
    /// Stamina points given (`Stamina`).
    Stamina(f64),
    /// Metres the holder's hit knocks its target back (`Knockback`; Inferred unit: the player
    /// rows hold 1 to 4, the warhammer's 4).
    Knockback(f64),
    /// Seconds added to cooldowns (`CooldownTimer`; below 0 shorter).
    Cooldown {
        /// Which cooldowns.
        scope: CooldownScope,
        /// The seconds added.
        seconds: f64,
    },
}

impl Effect {
    /// Whether it changes the hit itself, so it must be known before the hit lands.
    pub const fn changes_hit(&self) -> bool {
        matches!(
            self,
            Self::DamageShare(_)
                | Self::TakenShare(_)
                | Self::CritChance(_)
                | Self::CritDamage(_)
                | Self::HeadshotShare(_)
                | Self::BehindShare(_)
        )
    }
}

/// One passive ability that combat events fire.
#[derive(Debug, Clone, PartialEq)]
pub struct PassiveAbility {
    /// `AbilityID`.
    pub id: String,
    /// The events that fire it.
    pub triggers: Vec<Trigger>,
    /// Abilities the holder must also hold (`RequiredAbilityID`, `RequiredEquippedAbilityID`).
    pub requires: Vec<String>,
    /// `RequiredEquippedAbilityID`: a helper row, held with that ability (**Inferred**: 59
    /// combat rows, `Passive_Flail_Crusader_Passive5_Helper1` among them, are no mastery point,
    /// perk or attribute ability, and name the ability they help here).
    pub helps: Option<String>,
    /// What the event must meet.
    pub conditions: Vec<Condition>,
    /// What it does.
    pub effects: Vec<Effect>,
    /// The time between two firings (`ActivationCooldown`).
    pub cooldown_ms: u64,
    /// How many held copies count (`IsStackableAbility`, `IsStackableMax`): numeric effects
    /// scale by the copies.
    pub stack_max: u32,
    /// The condition columns its row fills that the engine does not check.
    pub unread_conditions: Vec<&'static str>,
    /// The effect columns its row fills that the engine does not carry out.
    pub unread_effects: Vec<&'static str>,
}

impl PassiveAbility {
    /// The ability of `row`; `None` for a row no combat event fires.
    pub fn of_row(row: &AbilityData) -> Option<Self> {
        let triggers: Vec<Trigger> = [
            (row.on_hit, Trigger::Hit),
            (row.on_crit, Trigger::Crit),
            (row.on_kill, Trigger::Kill),
            (row.on_hit_taken, Trigger::HitTaken),
            (row.on_blocked_hit, Trigger::BlockedHit),
            (row.on_blocked_hit_taken, Trigger::BlockedHitTaken),
            (row.on_block_break, Trigger::BlockBreak),
            (row.on_target_block_break, Trigger::TargetBlockBreak),
            (row.on_head_shot, Trigger::HeadShot),
            (row.on_hit_behind, Trigger::HitBehind),
        ]
        .into_iter()
        .filter(|(set, _)| *set == Some(true))
        .map(|(_, trigger)| trigger)
        .collect();
        let effects = effects_of(row);
        let triggers = if !triggers.is_empty() {
            triggers
        } else if row.is_active_ability != Some(true)
            && !effects.is_empty()
            && effects.iter().all(Effect::changes_hit)
        {
            ALWAYS_ON_TRIGGERS.to_vec()
        } else {
            return None;
        };
        let stackable = row.is_stackable_ability == Some(true);
        Some(Self {
            id: row.ability_id.clone(),
            triggers,
            requires: [&row.required_ability_id, &row.required_equipped_ability_id]
                .into_iter()
                .filter_map(|cell| filled(cell.as_deref()))
                .map(str::to_owned)
                .collect(),
            helps: filled(row.required_equipped_ability_id.as_deref()).map(str::to_owned),
            conditions: conditions_of(row),
            effects,
            // An activation cooldown is a few seconds.
            cooldown_ms: (row.activation_cooldown.unwrap_or_default().max(0.0) * MS_PER_SECOND)
                .round() as u64,
            stack_max: if stackable {
                row.is_stackable_max
                    .and_then(|max| u32::try_from(max).ok())
                    .unwrap_or(ONE_COPY)
                    .max(ONE_COPY)
            } else {
                ONE_COPY
            },
            unread_conditions: unread(row, UNREAD_CONDITIONS),
            unread_effects: unread(row, UNREAD_EFFECTS),
        })
    }

    /// Whether the engine can fire it: it checks every condition and has an effect.
    pub fn fires(&self) -> bool {
        self.unread_conditions.is_empty() && !self.effects.is_empty()
    }

    /// Whether `trigger` fires it.
    pub fn fired_by(&self, trigger: Trigger) -> bool {
        self.triggers.contains(&trigger)
    }
}

/// A non-empty, trimmed cell.
fn filled(cell: Option<&str>) -> Option<&str> {
    cell.map(str::trim).filter(|cell| !cell.is_empty())
}

/// The entries of a list cell, trimmed; empty for an empty cell.
pub(crate) fn list_of(cell: Option<&str>) -> Vec<String> {
    filled(cell).map_or_else(Vec::new, |cell| {
        cell.split(LIST_SEPARATOR)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

/// A number cell that holds something.
fn nonzero(value: Option<f64>) -> Option<f64> {
    value.filter(|value| *value != 0.0)
}

/// The conditions of `row`.
fn conditions_of(row: &AbilityData) -> Vec<Condition> {
    let mut conditions = Vec::new();
    if let Some(tag) = filled(row.weapon_tag.as_deref()) {
        conditions.push(Condition::WeaponTag(tag.to_owned()));
    }
    let attack_types = list_of(row.attack_type.as_deref());
    if !attack_types.is_empty() {
        conditions.push(Condition::AttackTypes(attack_types));
    }
    let ignored = list_of(row.ignore_disabled_attack_types.as_deref());
    if !ignored.is_empty() {
        conditions.push(Condition::NotAttackTypes(ignored));
    }
    let rows: Vec<u32> = list_of(row.damage_table_row.as_deref())
        .iter()
        .map(|id| lowercase_crc32(id))
        .collect();
    if !rows.is_empty() {
        conditions.push(Condition::DamageRows(rows));
    }
    if row.damage_is_melee == Some(true) {
        conditions.push(Condition::Melee);
    }
    if row.damage_is_ranged == Some(true) {
        conditions.push(Condition::Ranged);
    }
    let damage_types = list_of(row.damage_types.as_deref());
    if !damage_types.is_empty() {
        conditions.push(Condition::DamageTypes(damage_types));
    }
    let vitals = [
        (
            Side::Other,
            Vital::Health,
            &row.target_comparison_type,
            row.target_health_percent,
        ),
        (
            Side::Holder,
            Vital::Health,
            &row.my_comparison_type,
            row.my_health_percent,
        ),
        (
            Side::Holder,
            Vital::Stamina,
            &row.my_stamina_comparison_type,
            row.my_stamina_percent,
        ),
        (
            Side::Holder,
            Vital::Mana,
            &row.my_mana_comparison_type,
            row.my_mana_percent,
        ),
    ];
    for (side, vital, compare, percent) in vitals {
        if let (Some(compare), Some(percent)) = (compare.as_deref().and_then(Compare::of), percent)
        {
            conditions.push(Condition::Vital {
                side,
                vital,
                compare,
                // A percent cell is 0 to 100.
                share: percent as f64 / PERCENT,
            });
        }
    }
    let target_stacks = StackRule::of(
        row.target_status_effect_comparison.as_deref(),
        row.target_status_effect_stack_size,
    );
    let own_stacks = StackRule::of(
        row.status_effect_comparison.as_deref(),
        row.status_effect_stack_size,
    );
    let target_owned = row.check_status_effects_on_target_owned == Some(true);
    let own_owned = row.check_owned_status_effects == Some(true);
    let effect_conditions = [
        (
            Side::Other,
            list_of(row.target_status_effect.as_deref()),
            EffectMatch::Ids as fn(Vec<String>) -> EffectMatch,
            target_stacks,
            target_owned,
        ),
        (
            Side::Other,
            list_of(row.target_status_effect_category.as_deref()),
            EffectMatch::Categories,
            target_stacks,
            target_owned,
        ),
        (
            Side::Holder,
            list_of(row.status_effect.as_deref()),
            EffectMatch::Ids,
            own_stacks,
            own_owned,
        ),
        (
            Side::Holder,
            list_of(row.status_effect_categories.as_deref()),
            EffectMatch::Categories,
            own_stacks,
            own_owned,
        ),
    ];
    for (side, entries, matched, stacks, owned) in effect_conditions {
        if !entries.is_empty() {
            conditions.push(Condition::Effects {
                side,
                matched: matched(entries),
                stacks,
                owned,
            });
        }
    }
    let loads = list_of(row.equip_load_category.as_deref());
    if !loads.is_empty() {
        conditions.push(Condition::EquipLoads(loads));
    }
    let lacks = list_of(row.dont_have_status_effect.as_deref());
    if !lacks.is_empty() {
        conditions.push(Condition::LacksEffects(lacks));
    }
    for (side, cell) in [
        (Side::Other, &row.target_vitals_category),
        (Side::Holder, &row.attacker_vitals_category),
    ] {
        if let Some(category) = filled(cell.as_deref()) {
            conditions.push(Condition::Category {
                side,
                category: category.to_owned(),
            });
        }
    }
    if let (Some(compare), Some(metres)) = (
        row.dist_comparison_type.as_deref().and_then(Compare::of),
        row.dist_from_defender,
    ) {
        conditions.push(Condition::Distance { compare, metres });
    }
    if row.require_reaction == Some(true) {
        conditions.push(Condition::Reaction);
    }
    let modes = list_of(row.exclude_from_game_modes.as_deref());
    if !modes.is_empty() {
        conditions.push(Condition::NotInGameModes(modes));
    }
    conditions
}

/// The effects of `row`.
fn effects_of(row: &AbilityData) -> Vec<Effect> {
    let mut effects = Vec::new();
    for (side, cell) in [
        (Side::Holder, &row.self_apply_status_effect),
        (Side::Other, &row.other_apply_status_effect),
    ] {
        let ids = list_of(cell.as_deref());
        if !ids.is_empty() {
            effects.push(Effect::Apply { side, ids });
        }
    }
    let removed = list_of(row.remove_status_effects.as_deref());
    if !removed.is_empty() {
        effects.push(Effect::RemoveOwn(removed));
    }
    let shares = [
        (row.base_damage, Effect::DamageShare as fn(f64) -> Effect),
        (row.base_damage_reduction, Effect::TakenShare),
        (row.crit_chance, Effect::CritChance),
        (row.crit_damage, Effect::CritDamage),
        (row.headshot_damage, Effect::HeadshotShare),
        (row.hit_from_behind_damage, Effect::BehindShare),
        (row.dmg_pct_to_health, Effect::LifeSteal),
        (row.mana, Effect::ManaShare),
        (row.stamina.map(|points| points as f64), Effect::Stamina),
        (row.knockback, Effect::Knockback),
    ];
    effects.extend(
        shares
            .into_iter()
            .filter_map(|(value, effect)| nonzero(value).map(effect)),
    );
    if let Some(seconds) = nonzero(row.cooldown_timer) {
        let id = filled(row.cooldown_id.as_deref()).map(str::to_owned);
        let scope = match (filled(row.cdr_immediately_options.as_deref()), id) {
            (Some(ACTIVE_WEAPON_SCOPE), _) => Some(CooldownScope::ActiveWeapon),
            (Some(ALL_EXCEPT_SCOPE), Some(id)) => Some(CooldownScope::AllExcept(id)),
            (_, Some(id)) => Some(CooldownScope::Ability(id)),
            (_, None) => None,
        };
        if let Some(scope) = scope {
            effects.push(Effect::Cooldown { scope, seconds });
        }
    }
    effects
}

/// A column and whether a row fills it.
type Filled = (&'static str, fn(&AbilityData) -> bool);

/// The columns of `row` among `columns` that it fills.
fn unread(row: &AbilityData, columns: &[Filled]) -> Vec<&'static str> {
    columns
        .iter()
        .filter(|(_, fills)| fills(row))
        .map(|(column, _)| *column)
        .collect()
}

fn text(cell: &Option<String>) -> bool {
    filled(cell.as_deref()).is_some()
}

fn number(cell: Option<f64>) -> bool {
    nonzero(cell).is_some()
}

fn count(cell: Option<i64>) -> bool {
    cell.is_some_and(|count| count != 0)
}

/// The condition columns of the combat rows that the engine does not check: action and
/// sequence windows of the CAGE actions, slayer script conditions, hit counters, grit, markers,
/// crowds, ammo and cooldown checks.
const UNREAD_CONDITIONS: &[Filled] = &[
    ("InAction", |row| text(&row.in_action)),
    ("InSequence", |row| text(&row.in_sequence)),
    ("SlayerScriptCondition", |row| {
        text(&row.slayer_script_condition)
    }),
    ("TrackHitCount", |row| row.track_hit_count == Some(true)),
    ("NumberOfTrackedHits", |row| {
        count(row.number_of_tracked_hits)
    }),
    ("NumSuccessfulHits", |row| count(row.num_successful_hits)),
    ("NumConsecutiveHits", |row| count(row.num_consecutive_hits)),
    ("MaxConsecutiveHits", |row| count(row.max_consecutive_hits)),
    ("HasGritActive", |row| row.has_grit_active == Some(true)),
    ("TargetHasGritActive", |row| {
        row.target_has_grit_active == Some(true)
    }),
    ("AbilityOnCooldownOptions", |row| {
        text(&row.ability_on_cooldown_options)
    }),
    ("NumAroundMe", |row| count(row.num_around_me)),
    ("MaxNumAroundMe", |row| count(row.max_num_around_me)),
    ("MyMarker", |row| text(&row.my_marker)),
    ("MySequenceMarker", |row| text(&row.my_sequence_marker)),
    ("TargetMarker", |row| text(&row.target_marker)),
    ("TargetSequenceMarker", |row| {
        text(&row.target_sequence_marker)
    }),
    ("RequiredEquipment", |row| text(&row.required_equipment)),
    ("TargetCollisionFilters", |row| {
        text(&row.target_collision_filters)
    }),
    ("UISpellForManaCheck", |row| {
        text(&row.ui_spell_for_mana_check)
    }),
    ("LoadedAmmoCount", |row| count(row.loaded_ammo_count)),
    ("OnEventConditionalActivationChance", |row| {
        row.on_event_conditional_activation_chance == Some(true)
    }),
    ("IsInCombatState", |row| {
        row.is_in_combat_state == Some(true)
    }),
    ("IsNotInCombatState", |row| {
        row.is_not_in_combat_state == Some(true)
    }),
    ("MyAfflictionType", |row| text(&row.my_affliction_type)),
];

/// The effect columns of the combat rows that the engine does not carry out.
const UNREAD_EFFECTS: &[Filled] = &[
    ("RemoteDamageTableRow", |row| {
        text(&row.remote_damage_table_row)
    }),
    ("StatusEffectDamageTableRowOverride", |row| {
        text(&row.status_effect_damage_table_row_override)
    }),
    ("DamageTableRowOverride", |row| {
        text(&row.damage_table_row_override)
    }),
    ("ThreatDamage", |row| number(row.threat_damage)),
    ("ArmorPenetration", |row| number(row.armor_penetration)),
    ("CritDamageReduction", |row| {
        number(row.crit_damage_reduction)
    }),
    ("BlockDamage", |row| number(row.block_damage)),
    ("BlockDamageReduction", |row| {
        number(row.block_damage_reduction)
    }),
    ("PhysicalArmor", |row| number(row.physical_armor)),
    ("ElementalArmor", |row| number(row.elemental_armor)),
    ("HealthDamageMitigation", |row| {
        text(&row.health_damage_mitigation)
    }),
    ("HealScalingValueMultiplier", |row| {
        number(row.heal_scaling_value_multiplier)
    }),
    ("CastSpell", |row| text(&row.cast_spell)),
    ("StatusEffectDurationMult", |row| {
        number(row.status_effect_duration_mult)
    }),
    ("TargetStatusEffectDurationMult", |row| {
        number(row.target_status_effect_duration_mult)
    }),
    ("TargetStatusEffectDurationMod", |row| {
        count(row.target_status_effect_duration_mod)
    }),
    ("RemoveTargetStatusEffectsList", |row| {
        text(&row.remove_target_status_effects_list)
    }),
    ("RemoveTargetStatusEffectCats", |row| {
        text(&row.remove_target_status_effect_cats)
    }),
    ("NumStatusEffectsToRemove", |row| {
        count(row.num_status_effects_to_remove)
    }),
    ("PerStatusEffectOnSelf", |row| {
        row.per_status_effect_on_self == Some(true)
    }),
    ("PerStatusEffectOnTarget", |row| {
        row.per_status_effect_on_target == Some(true)
    }),
    ("PerStatusEffectStackOnSelf", |row| {
        row.per_status_effect_stack_on_self == Some(true)
    }),
];

/// What a hit's damage row tells the engine (`DamageData`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HitTraits {
    /// `AttackType` (`Light`, `Heavy`, `Ability`, `Heartgem`, ...).
    pub attack_type: Option<String>,
    /// `IsRanged`.
    pub ranged: bool,
    /// `DamageType` (`Slash`, `Fire`, ...).
    pub damage_type: Option<String>,
}

/// Every passive ability that combat events fire, and the traits of every damage row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PassiveAbilities {
    abilities: Vec<PassiveAbility>,
    /// The abilities of each lowercase `AbilityID`, by index.
    by_id: HashMap<String, usize>,
    /// The helper abilities of each lowercase `AbilityID`, by index.
    helpers: HashMap<String, Vec<usize>>,
    /// The traits of each damage row.
    hits: HashMap<RowKey, HitTraits>,
}

impl PassiveAbilities {
    /// The abilities of the `AbilityData` rows of `tables` and the hit traits of its
    /// `DamageData` rows.
    pub fn from_tables(tables: &Tables) -> Self {
        let hits = tables.damage_data.iter().flat_map(|table| {
            table.rows.iter().map(move |row| {
                (
                    table.unique_id.as_str(),
                    row.damage_id.as_str(),
                    HitTraits {
                        attack_type: filled(row.attack_type.as_deref()).map(str::to_owned),
                        ranged: filled(row.is_ranged.as_deref()) == Some(RANGED_CELL),
                        damage_type: filled(row.damage_type.as_deref()).map(str::to_owned),
                    },
                )
            })
        });
        Self::build(
            tables.ability_data.iter().flat_map(|table| &table.rows),
            hits,
        )
    }

    /// The abilities of `rows` and the hit traits `hits` (table, damage id, traits).
    pub fn build<'a>(
        rows: impl IntoIterator<Item = &'a AbilityData>,
        hits: impl IntoIterator<Item = (&'a str, &'a str, HitTraits)>,
    ) -> Self {
        let mut abilities = Vec::new();
        let mut by_id = HashMap::new();
        let mut helpers: HashMap<String, Vec<usize>> = HashMap::new();
        for ability in rows.into_iter().filter_map(PassiveAbility::of_row) {
            by_id
                .entry(ability.id.to_lowercase())
                .or_insert(abilities.len());
            if let Some(helped) = &ability.helps {
                helpers
                    .entry(helped.to_lowercase())
                    .or_default()
                    .push(abilities.len());
            }
            abilities.push(ability);
        }
        let hits = hits
            .into_iter()
            .map(|(table, id, traits)| (RowKey::of(table, id), traits))
            .collect();
        Self {
            abilities,
            by_id,
            helpers,
            hits,
        }
    }

    /// The ability of id `id` (case does not matter).
    pub fn get(&self, id: &str) -> Option<&PassiveAbility> {
        self.by_id
            .get(&id.to_lowercase())
            .and_then(|index| self.abilities.get(*index))
    }

    /// The helper abilities of ability `id` (case does not matter).
    pub fn helpers_of(&self, id: &str) -> impl Iterator<Item = &PassiveAbility> {
        self.helpers
            .get(&id.to_lowercase())
            .into_iter()
            .flatten()
            .filter_map(|index| self.abilities.get(*index))
    }

    /// The traits of damage row `row`.
    pub fn hit(&self, row: RowKey) -> Option<&HitTraits> {
        self.hits.get(&row)
    }

    /// Every ability.
    pub fn all(&self) -> &[PassiveAbility] {
        &self.abilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(fields: serde_json::Value) -> AbilityData {
        serde_json::from_value(fields).expect("row")
    }

    #[test]
    fn a_row_without_a_combat_trigger_is_no_passive() {
        assert_eq!(
            PassiveAbility::of_row(&row(serde_json::json!({
                "ability_id": "Foc_Bonus_100_1",
                "max_mana": 0.1
            }))),
            None
        );
    }

    #[test]
    fn a_head_shot_and_a_hit_from_behind_fire_their_rows_and_raise_their_hits() {
        let head = PassiveAbility::of_row(&row(serde_json::json!({
            "ability_id": "Test_HeadshotDamage",
            "on_head_shot": true,
            "headshot_damage": 0.1
        })))
        .expect("passive");
        assert_eq!(head.triggers, [Trigger::HeadShot]);
        assert_eq!(head.effects, [Effect::HeadshotShare(0.1)]);
        assert!(head.fires(), "{:?}", head.unread_effects);
        let behind = PassiveAbility::of_row(&row(serde_json::json!({
            "ability_id": "Test_BackstabDamage",
            "hit_from_behind_damage": 0.2
        })))
        .expect("an always-on node");
        assert_eq!(behind.effects, [Effect::BehindShare(0.2)]);
        assert!(Trigger::HitBehind.by_holder() && Trigger::HeadShot.by_holder());
    }

    #[test]
    fn a_tree_node_without_a_trigger_that_changes_hits_fires_on_every_hit() {
        // Retail rows of build 25810808.
        let node = PassiveAbility::of_row(&row(serde_json::json!({
            "ability_id": "Passive_Firestaff_FireMage_CritDmg",
            "crit_damage": 0.15,
            "tree_id": 1,
            "weapon_tag": "FireMagic"
        })))
        .expect("passive");
        assert_eq!(node.triggers, ALWAYS_ON_TRIGGERS);
        assert_eq!(node.effects, [Effect::CritDamage(0.15)]);
        assert!(
            node.conditions
                .contains(&Condition::WeaponTag("FireMagic".to_owned()))
        );
        assert!(node.fires());
        let active = PassiveAbility::of_row(&row(serde_json::json!({
            "ability_id": "Ability_Firestaff_FlameThrower",
            "base_damage": 0.1,
            "is_active_ability": true
        })));
        assert_eq!(active, None, "an active ability's own row");
    }

    #[test]
    fn a_weapon_passive_reads_its_trigger_conditions_and_effects() {
        // Retail row of build 25810808.
        let ability = PassiveAbility::of_row(&row(serde_json::json!({
            "ability_id": "Upgrade_Dagger_Stealth_1_SurprisedSlow",
            "equip_when_unsheathed": true,
            "on_hit": true,
            "other_apply_status_effect": "Status_Dagger_Stealth_Surprised_Slow",
            "require_reaction": true,
            "required_ability_id": "Ability_Dagger_Stealth",
            "required_equipped_ability_id": "Upgrade_Dagger_Stealth_1",
            "target_status_effect": "Status_Dagger_Stealth_Surprised",
            "weapon_tag": "Dagger"
        })))
        .expect("passive");
        assert_eq!(ability.triggers, [Trigger::Hit]);
        assert_eq!(
            ability.requires,
            ["Ability_Dagger_Stealth", "Upgrade_Dagger_Stealth_1"]
        );
        assert_eq!(
            ability.conditions,
            [
                Condition::WeaponTag("Dagger".to_owned()),
                Condition::Effects {
                    side: Side::Other,
                    matched: EffectMatch::Ids(vec!["Status_Dagger_Stealth_Surprised".to_owned()]),
                    stacks: StackRule::Any,
                    owned: false,
                },
                Condition::Reaction,
            ]
        );
        assert_eq!(
            ability.effects,
            [Effect::Apply {
                side: Side::Other,
                ids: vec!["Status_Dagger_Stealth_Surprised_Slow".to_owned()],
            }]
        );
        assert!(ability.fires());
    }

    #[test]
    fn a_row_with_an_unread_condition_never_fires() {
        // Retail row: the cooldown cut needs the Clear Out action window.
        let ability = PassiveAbility::of_row(&row(serde_json::json!({
            "ability_id": "Upgrade_WarHammer_ClearOut_CDRonHit",
            "cdr_immediately_options": "AbilitySpecific",
            "cooldown_id": "Ability_WarHammer_ClearOut",
            "cooldown_timer": -0.05,
            "in_action": "Ability_WarHammer_ClearOut",
            "in_action_time": 0.1,
            "on_hit": true
        })))
        .expect("passive");
        assert_eq!(
            ability.effects,
            [Effect::Cooldown {
                scope: CooldownScope::Ability("Ability_WarHammer_ClearOut".to_owned()),
                seconds: -0.05,
            }]
        );
        assert_eq!(ability.unread_conditions, ["InAction"]);
        assert!(!ability.fires());
    }

    #[test]
    fn comparisons_read_every_spelling_and_stack_rules_follow_them() {
        assert_eq!(
            Compare::of("GreatherThanOrEqual"),
            Some(Compare::GreaterOrEqual)
        );
        assert_eq!(Compare::of("EQUAL"), Some(Compare::Equal));
        assert_eq!(Compare::of("Sometimes"), None);
        assert!(Compare::Less.holds(0.29, 0.3));
        assert!(!Compare::Less.holds(0.3, 0.3));
        assert!(StackRule::of(Some("NotEqual"), None).holds(0));
        assert!(!StackRule::of(Some("NotEqual"), None).holds(1));
        assert!(StackRule::of(Some("GreaterThanOrEqual"), Some(3)).holds(3));
        assert!(!StackRule::of(Some("GreaterThanOrEqual"), Some(3)).holds(2));
        assert!(StackRule::of(None, None).holds(1));
    }

    #[test]
    fn a_stackable_perk_counts_its_copies_and_a_health_cell_is_a_share() {
        let ability = PassiveAbility::of_row(&row(serde_json::json!({
            "ability_id": "Ability_Perk_Executioner",
            "on_hit": true,
            "base_damage": 0.1,
            "target_comparison_type": "LessThan",
            "target_health_percent": 30,
            "is_stackable_ability": true,
            "is_stackable_max": 2
        })))
        .expect("passive");
        assert_eq!(ability.stack_max, 2);
        assert_eq!(
            ability.conditions,
            [Condition::Vital {
                side: Side::Other,
                vital: Vital::Health,
                compare: Compare::Less,
                share: 0.3,
            }]
        );
        assert_eq!(ability.effects, [Effect::DamageShare(0.1)]);
        assert!(ability.effects[0].changes_hit());
    }

    #[test]
    fn the_book_finds_abilities_and_hit_traits_without_case() {
        let rows = [
            row(serde_json::json!({
                "ability_id": "Passive_Flail_Crusader_Passive5",
                "on_blocked_hit_taken": true,
                "self_apply_status_effect": "Status_Flail_Passive_BlockedHit_DurStacking"
            })),
            row(serde_json::json!({
                "ability_id": "Passive_Flail_Crusader_Passive5_Helper1",
                "on_hit_taken": true,
                "required_ability_id": "Passive_Flail_Crusader_Passive5",
                "required_equipped_ability_id": "Passive_Flail_Crusader_Passive5",
                "status_effect": "Status_Flail_Passive_BlockedHit_DurStacking"
            })),
        ];
        let book = PassiveAbilities::build(
            &rows,
            [(
                "DamageTable",
                "Flail_Light1",
                HitTraits {
                    attack_type: Some("Light".to_owned()),
                    ranged: false,
                    damage_type: Some("Strike".to_owned()),
                },
            )],
        );
        assert!(
            book.get("passive_flail_crusader_passive5")
                .is_some_and(|ability| ability.fired_by(Trigger::BlockedHitTaken))
        );
        assert_eq!(
            book.helpers_of("Passive_Flail_Crusader_Passive5")
                .map(|helper| helper.id.as_str())
                .collect::<Vec<_>>(),
            ["Passive_Flail_Crusader_Passive5_Helper1"]
        );
        assert_eq!(
            book.hit(RowKey::of("damagetable", "flail_light1"))
                .and_then(|traits| traits.attack_type.as_deref()),
            Some("Light")
        );
        assert!(!Trigger::BlockedHitTaken.by_holder());
        assert!(Trigger::TargetBlockBreak.by_holder());
    }
}
