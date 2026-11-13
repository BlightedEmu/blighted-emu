// SPDX-License-Identifier: AGPL-3.0-or-later
//! The game data of the afflictions (findings `2026-11-13-catacomb-erosion.md`, sections C6 to
//! C9): the meters of `javelindata_afflictions` (poison, frostbite, disease, bleed, curse,
//! drowning, immobilized, blight, spores, stunned, fatigue, erosion), the status effects that
//! fill or empty them (their `Afflictions` cell), the attack rows that add to them at a hit
//! (`Affliction`, `AddAffliction`), the consumables that change them at their use
//! (`OnUseAffliction`), the effects that absorb part of a gain (`AFA<Kind>`) or raise the meter's
//! top (`Res<Kind>`), and the abilities that the catacombs' erosion effects equip (their
//! `EquipAbility` rows), which move the erosion level on.
//!
//! **Observed** (client `FUN_1467dd9d0`, the server's add of an affliction, and `FUN_145c27160`,
//! the meter change; 871 retail ledgers with an own affliction list): a meter's item key in the
//! vitals' lists is its `IntId` (`CATACOMBS6` 1310.842 s: blight 8; 1325.741 s: erosion 15); its
//! top is the holder's `Res<Kind>` plus the row's `Max` (poison and bleed 50, curse and blight 60
//! from the `Player` vitals row; drowning 20, stunned 4 and erosion 40 from `Max`). A gain is cut
//! by the share of the holder's `AFA<Kind>` effects: the catacombs' equip load effects take part
//! of each erosion gain (`EquipLoad_CritDmgReduction_Medium` 0.25, `_Heavy` 0.6), so a hit's
//! `Catacombs_ApplyErosion` (2) adds 2.0 on a light load (`CATACOMBS5` 133.048 s) and 1.5 on a
//! medium one (`CATACOMBS2` 36.440 s), a mob row's `AddAffliction` 3 adds 3.0 or 2.25
//! (`CATACOMBS6` 916.723 s). `AI_Fulgoris_Gas` (`AfflictionBlight=5`, 1 s ticks) fills the
//! blight 5 a second (`CATACOMBS6` 1310.842 s to 1322.842 s).
//!
//! The erosion's own chain (Observed, the ten catacomb ledgers): every run player holds the
//! effects that [`EROSION_ROOT_EFFECT`]'s ability equips from the arrival (`CATACOMBS2`
//! 17.274 s: `Catacombs_Erosion_Remove_CheckLv2_v2`, `Erosion_lv0_Indicator`); each hit taken
//! adds `Catacombs_Erosion_OnHitTaken` and `Catacombs_ApplyErosion`; a full erosion meter adds
//! `Erosion`, whose application moves the level indicator one step up with that level's
//! `Erosion_lvN_Temp` (`CATACOMBS3` 947.871 s); an erosion shrine (`Catacombs_HealthChest`) adds
//! `CAT_PowerUp_Health` (`AfflictionErosion=-100`) and moves the indicator down (976.819 s).
//!
//! **Worn perks** (Observed, retail `MSQ6-lv28` 1335.582 s, the only one of 1,194 ledgers with
//! another curse top than 60): a player that wears `QuestReward_EF_Corruption_Pouch` (gear
//! score 220, perk `PerkID_Armor_ResistCorrupt`, affix `Affix_Armor_ResistCorrupt`: `RESCurse` 5,
//! `AFACurse` 0.05, `ScalingPerGearScore` `0.00381818181818182,550:0.00`) has a curse top of
//! 67.3125 (a half float: 60 + 5 x 1.4582 = 67.291) and each `ApplyCurseL1` tick (+1) adds
//! 0.927246 (1 - 0.05 x 1.4582 = 0.92709). So a worn perk's affix `RES<Kind>` raises the top and
//! its `AFA<Kind>` takes a share of each gain, both times the perk's potency at its item's gear
//! score ([`crate::perk_effects::GearScoreScale`]); [`AfflictionRules::worn_part`]. The other
//! affixes with such cells are not worn: no table names the affix definitions `RarityLevel*`,
//! `TypeLevel*`, `TypeLevel*Shield` and `WornItem` that hold `AffixArmorRarityLevel*`,
//! `AffixTypeLevel*` (small `RES` shares), `AffixWornItem` and the only `ABA<Kind>` (block
//! absorb) cells, and 156 ledgers of geared players show the plain top 60; no perk names
//! `CorruptionResistance1` to `3` or `BlightResistance1` to `3`. **Inferred**: two worn perks
//! with parts of one kind add up.
//!
//! **Global afflictions** (`IsGlobalAffliction`: the mutation curses `AfflictionCurse2` to `4`;
//! [`AfflictionRules::global_kinds`]): the client keeps them in the game mode's
//! `globalAfflictionData` (`FUN_1467dd9d0` takes another path for them), not in the holder's
//! vitals (Observed: retail `20261002-184508-050`, child 22 of the game mode; the own vitals list
//! only the plain curse). An effect whose `Afflictions` cell names one (`Mut_Curse2_Affliction_Tick`
//! `AfflictionCurse2=-1`) changes the run's one meter. The curse effects' abilities that fire
//! when their holder performs an ability (`AbilityTrigger` `OnPerform`, [`EffectTrigger::Performed`])
//! are reached from each curse's `CurseMinor` and `CurseMajor` of `javelindata_cursemutations`,
//! the `Wildcard` replaced by each elemental `CategoryWildcard` (`Mut_Curse2_Minor` self-applies
//! `Mut_Curse2_Affliction_Tick`; `Mut_Curse2_Fir_Major` `Mut_Curse2_Fir_Major_Damage`, the Blood
//! Offering; `Mut_Curse4_Major` `Mut_Curse4_Major_Exhaust`).
//!
//! Not in these rules: the attack rows
//! whose `Affliction` is empty (nine carry an `AfflictionPercent` and a text `AddAffliction`,
//! shifted cells); the abilities of the first erosion version (`AbilityID_Catacombs_ErosionLv0`
//! to `Lv2`, no effect equips them) and every ability trigger other than `OnHitTaken` and
//! `OnStatusEffectApplied`.

use std::collections::{BTreeSet, HashMap};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;
use crate::combat::DEFAULT_PLAYER_VITALS_ID;
use crate::generated::{AbilityData, AffixStatData, StatusEffectData, VitalsBaseData};
use crate::passive_abilities::list_of;
use crate::perk_effects::GearScoreScale;
use crate::status_effects::RowKey;

/// The erosion affliction's `AfflictionID`.
pub const EROSION_AFFLICTION: &str = "AfflictionErosion";
/// The effect a catacombs run gives each player at its arrival (module text; Inferred: the only
/// effect whose equipped ability equips what retail players hold from the arrival).
pub const EROSION_ROOT_EFFECT: &str = "Catacombs_Erosion_Prototype";
/// The head of every `AfflictionID`; the rest, lowercase, names the kind's `AFA`, `Res`
/// columns (`AfflictionBlight`: `AFABlight`, `ResBlight`).
const AFFLICTION_PREFIX: &str = "Affliction";
/// The separators of the entries of an affliction cell: `+` in the consumables'
/// `OnUseAffliction` (Observed: `AfflictionCurse=-1000+AfflictionPoison=-1000`), `,` as the other
/// list cells (Inferred: no status effect cell holds two entries).
const ENTRY_SEPARATORS: [char; 2] = ['+', ','];
/// The separator of an affliction entry (`AfflictionErosion=2`).
const AFFLICTION_VALUE_SEPARATOR: char = '=';
/// The `RemoveStatusEffects` value that removes the row's `StatusEffectsList`.
const REMOVE_LISTED: &str = "AbilitySpecific";
/// Milliseconds in a second.
const MS_PER_S: f64 = 1_000.0;
/// The `AbilityTrigger` of an ability that fires when its holder performs an ability.
const ON_PERFORM: &str = "OnPerform";
/// The placeholder of a curse effect id that an elemental category replaces (as
/// [`crate::mutations`]).
const CURSE_WILDCARD: &str = "Wildcard";

/// One affliction meter kind (`javelindata_afflictions`).
#[derive(Debug, Clone, PartialEq)]
pub struct AfflictionKind {
    /// CRC-32 of the lowercase `AfflictionID`: the key the gains name.
    pub crc: u32,
    /// `AfflictionID`.
    pub id: String,
    /// `IntId`: the meter's item key in the vitals' affliction lists.
    pub int_id: u8,
    /// `Max`: what the row adds to the holder's resistance for the meter's top.
    pub max: f32,
    /// The `Player` vitals row's `Res<Kind>`: the players' resistance before their effects.
    pub resistance: f32,
    /// `DrainDelay`: how long a meter holds its value after a change, ms.
    pub drain_delay_ms: u64,
    /// `DrainRate`: what a meter loses a second after its hold; 0 never drains.
    pub drain_per_s: f32,
    /// `AfflictedDrainTime`: how long a full meter takes to fall from its top to 0, ms.
    pub afflicted_ms: u64,
    /// `IgnoreWhenAfflicted`: a full meter takes no gain.
    pub ignore_when_afflicted: bool,
    /// `StatusEffects`: the effects a full meter adds to its holder.
    pub afflicted_effects: Vec<String>,
    /// `IsTimerAffliction`: the meter of a global affliction rises with time (the curse
    /// tooltips: "Periodically ..."); `false` for the vitals kinds.
    pub timer: bool,
}

/// A change of one affliction meter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AfflictionGain {
    /// The kind ([`AfflictionKind::crc`]).
    pub kind: u32,
    /// What it adds (a fall below 0).
    pub amount: f32,
}

/// A status effect that changes meters while it is held.
#[derive(Debug, Clone, PartialEq)]
pub struct AfflictionTick {
    /// The effect (CRC-32 of its lowercase `StatusID`).
    pub effect: u32,
    /// What one tick adds: the `Afflictions` entries.
    pub gains: Vec<AfflictionGain>,
    /// When the first tick falls after the application (`DelayInitialTick`), ms.
    pub first_ms: u64,
    /// The time between two ticks (`TickRate`), ms; 0 for an effect without a rate, which
    /// ticks once.
    pub period_ms: u64,
}

/// A held status effect's part of one kind: an absorbed share of each gain (`AFA<Kind>`) or a
/// resistance that raises the meter's top (`Res<Kind>`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectShare {
    /// The effect (CRC-32 of its lowercase `StatusID`).
    pub effect: u32,
    /// The kind ([`AfflictionKind::crc`]).
    pub kind: u32,
    /// The share or the resistance.
    pub value: f32,
}

/// One kind's part of a perk's affix at potency 1: what it adds to the meter's top
/// (`RES<Kind>`) and the share of each gain it takes (`AFA<Kind>`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AffixShare {
    /// What it adds to the top.
    pub resistance: f32,
    /// The share of each gain it takes.
    pub absorption: f32,
}

/// The affliction parts of one perk's affix, by kind ([`AfflictionKind::crc`]), and the perk's
/// gear score scale.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerkShares {
    /// `ScalingPerGearScore`.
    pub scale: GearScoreScale,
    /// The parts, by kind.
    pub shares: Vec<(u32, AffixShare)>,
}

/// What fires an ability of an affliction effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectTrigger {
    /// The holder took a hit (`OnHitTaken`).
    HitTaken,
    /// An effect was applied to the holder (`OnStatusEffectApplied`): the one named by
    /// `StatusEffectBeingApplied`, else the effect that equips the ability.
    Applied(Option<u32>),
    /// The holder performed an ability (`AbilityTrigger` `OnPerform`).
    Performed,
}

/// One ability an affliction effect equips.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectAbility {
    /// Its `AbilityID`.
    pub id: String,
    /// The effect that equips it.
    pub holder: u32,
    /// What fires it.
    pub trigger: EffectTrigger,
    /// The effect the holder must own when it fires (`CheckOwnedStatusEffects`, `StatusEffect`).
    pub owned: Option<u32>,
    /// The effect the holder must not hold when it fires (`DontHaveStatusEffect`).
    pub not_owned: Option<u32>,
    /// The effects it removes (`RemoveStatusEffects` `AbilitySpecific`, `StatusEffectsList`).
    pub remove: Vec<u32>,
    /// The effects it adds to the holder (`SelfApplyStatusEffect`).
    pub apply: Vec<String>,
}

/// The affliction rules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AfflictionRules {
    /// The meter kinds of the holder's vitals, by `IntId`.
    pub kinds: Vec<AfflictionKind>,
    /// The global meter kinds (`IsGlobalAffliction`), by `IntId`: a game mode holds them.
    pub global_kinds: Vec<AfflictionKind>,
    /// The effects that change meters.
    pub ticks: Vec<AfflictionTick>,
    /// The effects that absorb a share of the gains (`AFA<Kind>`).
    pub absorptions: Vec<EffectShare>,
    /// The effects that raise a meter's top (`Res<Kind>`).
    pub resistances: Vec<EffectShare>,
    /// The perks whose affix raises a meter's top or absorbs a share of its gains, by lowercase
    /// `PerkID` (module text).
    pub perks: HashMap<String, PerkShares>,
    /// The effects each affliction effect adds with it (its ability's `OnEquipStatusEffect`).
    pub equips: Vec<(u32, Vec<String>)>,
    /// The abilities of the affliction effects, in the order the effects are reached.
    pub abilities: Vec<EffectAbility>,
    /// The gatherables that add an affliction effect (`AddedStatusEffect`), by CRC-32 of the
    /// lowercase `GatherableID`.
    pub gathered: Vec<(u32, String)>,
    /// What a hit of each attack row adds (`Affliction`, `AddAffliction`).
    pub hits: HashMap<RowKey, AfflictionGain>,
    /// What the use of each consumable changes (`OnUseAffliction`), by lowercase
    /// `ConsumableID`.
    pub uses: HashMap<String, Vec<AfflictionGain>>,
}

impl AfflictionRules {
    /// The rules of `tables`: every meter kind (the vitals' and the global ones apart), the
    /// effects, rows and consumables that change them, the absorbing and resisting effects, and
    /// the abilities of the effects reached from [`EROSION_ROOT_EFFECT`], the kinds' effects, the
    /// effects that change meters and the curse effects ([`curse_roots`]), through their
    /// equipped abilities' added effects and their `OnEndStatusEffect`.
    pub fn of_tables(tables: &Tables) -> Self {
        let player = tables
            .vitals_base_data
            .iter()
            .flat_map(|table| &table.rows)
            .find(|row| row.vitals_id.eq_ignore_ascii_case(DEFAULT_PLAYER_VITALS_ID));
        let (global_kinds, kinds): (Vec<AfflictionKind>, Vec<AfflictionKind>) = tables
            .affliction_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let suffix = kind_suffix(&row.affliction_id);
                Some(AfflictionKind {
                    crc: lowercase_crc32(&row.affliction_id),
                    id: row.affliction_id.clone(),
                    int_id: u8::try_from(row.int_id).ok()?,
                    max: row.max as f32,
                    resistance: player
                        .and_then(|player| vitals_resistance(player, &suffix))
                        .unwrap_or_default() as f32,
                    drain_delay_ms: ms(row.drain_delay),
                    drain_per_s: row.drain_rate.max(0.0) as f32,
                    afflicted_ms: ms(row.afflicted_drain_time),
                    ignore_when_afflicted: row.ignore_when_afflicted,
                    afflicted_effects: list_of(row.status_effects.as_deref()),
                    timer: row.is_timer_affliction == Some(true),
                })
                .map(|kind| (row.is_global_affliction == Some(true), kind))
            })
            .fold(
                (Vec::new(), Vec::new()),
                |(mut global, mut vitals), (is_global, kind)| {
                    if is_global {
                        global.push(kind);
                    } else {
                        vitals.push(kind);
                    }
                    (global, vitals)
                },
            );
        let known = |kind: u32| {
            kinds
                .iter()
                .chain(&global_kinds)
                .any(|known| known.crc == kind)
        };
        let effects: HashMap<u32, &StatusEffectData> = tables
            .status_effect_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (lowercase_crc32(&row.status_id), row))
            .collect();
        let abilities: HashMap<u32, &AbilityData> = tables
            .ability_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (lowercase_crc32(&row.ability_id), row))
            .collect();
        let ticks: Vec<AfflictionTick> = effects
            .iter()
            .filter_map(|(crc, row)| {
                let gains: Vec<AfflictionGain> = gains_of(row.afflictions.as_deref())
                    .into_iter()
                    .filter(|gain| known(gain.kind))
                    .collect();
                (!gains.is_empty()).then(|| AfflictionTick {
                    effect: *crc,
                    gains,
                    first_ms: ms(row.delay_initial_tick.unwrap_or_default()),
                    period_ms: ms(row.tick_rate.unwrap_or_default()),
                })
            })
            .collect();
        let shares = |value_of: fn(&StatusEffectData, &str) -> Option<f64>| -> Vec<EffectShare> {
            effects
                .iter()
                .flat_map(|(crc, row)| {
                    kinds.iter().filter_map(move |kind| {
                        let value = value_of(row, &kind_suffix(&kind.id))?;
                        (value != 0.0).then_some(EffectShare {
                            effect: *crc,
                            kind: kind.crc,
                            value: value as f32,
                        })
                    })
                })
                .collect()
        };
        let mut rules = Self {
            absorptions: shares(effect_absorption),
            resistances: shares(effect_resistance),
            ..Self::default()
        };
        let mut queue: Vec<String> = [EROSION_ROOT_EFFECT.to_owned()]
            .into_iter()
            .chain(kinds.iter().flat_map(|kind| kind.afflicted_effects.clone()))
            .chain(
                ticks
                    .iter()
                    .filter_map(|tick| effects.get(&tick.effect))
                    .map(|row| row.status_id.clone()),
            )
            .chain(curse_roots(tables))
            .collect();
        let mut reached = BTreeSet::new();
        while let Some(id) = queue.pop() {
            let crc = lowercase_crc32(&id);
            let Some(row) = effects.get(&crc) else {
                continue;
            };
            if !reached.insert(crc) {
                continue;
            }
            queue.extend(list_of(row.on_end_status_effect.as_deref()));
            let Some(ability) = row
                .equip_ability
                .as_deref()
                .and_then(|id| abilities.get(&lowercase_crc32(id.trim())))
            else {
                continue;
            };
            let equipped = list_of(ability.on_equip_status_effect.as_deref());
            queue.extend(equipped.iter().cloned());
            if !equipped.is_empty() {
                rules.equips.push((crc, equipped));
            }
            if let Some(ability) = effect_ability(crc, ability) {
                queue.extend(ability.apply.iter().cloned());
                rules.abilities.push(ability);
            }
        }
        rules.gathered = tables
            .gatherable_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let effect = row.added_status_effect.as_deref()?.trim();
                reached
                    .contains(&lowercase_crc32(effect))
                    .then(|| (lowercase_crc32(&row.gatherable_id), effect.to_owned()))
            })
            .collect();
        rules.hits = tables
            .damage_data
            .iter()
            .flat_map(|table| table.rows.iter().map(move |row| (table, row)))
            .filter_map(|(table, row)| {
                let kind = lowercase_crc32(row.affliction.as_deref()?.trim());
                let amount: f32 = row.add_affliction.as_deref()?.trim().parse().ok()?;
                (known(kind) && amount != 0.0).then(|| {
                    (
                        RowKey::of(&table.unique_id, &row.damage_id),
                        AfflictionGain { kind, amount },
                    )
                })
            })
            .collect();
        rules.uses = tables
            .consumable_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let gains: Vec<AfflictionGain> = gains_of(row.on_use_affliction.as_deref())
                    .into_iter()
                    .filter(|gain| known(gain.kind))
                    .collect();
                (!gains.is_empty()).then(|| (row.consumable_id.to_lowercase(), gains))
            })
            .collect();
        rules.perks = perk_shares(tables, &kinds);
        rules.ticks = ticks;
        rules.kinds = kinds;
        rules.global_kinds = global_kinds;
        rules
    }

    /// The global meter kind `kind` (CRC-32 of its lowercase `AfflictionID`).
    pub fn global_kind(&self, kind: u32) -> Option<&AfflictionKind> {
        self.global_kinds.iter().find(|known| known.crc == kind)
    }

    /// The meter kind `kind`, of the vitals or global.
    pub fn meter_kind(&self, kind: u32) -> Option<&AfflictionKind> {
        self.kind(kind).or_else(|| self.global_kind(kind))
    }

    /// The `OnPerform` abilities whose holder is effect `holder`.
    pub fn performed_by(&self, holder: u32) -> impl Iterator<Item = &EffectAbility> {
        self.abilities
            .iter()
            .filter(move |ability| ability.trigger == EffectTrigger::Performed)
            .filter(move |ability| ability.holder == holder)
    }

    /// The part of kind `kind` that the worn `perks` (perk id, its item's gear score) give: each
    /// perk's affix part times the perk's potency at its item's gear score, summed (module
    /// text).
    pub fn worn_part(&self, perks: &[(&str, u32)], kind: u32) -> AffixShare {
        let mut part = AffixShare::default();
        for (perk, gear_score) in perks {
            let Some(held) = self.perks.get(&perk.to_lowercase()) else {
                continue;
            };
            // A potency of a few at most.
            let potency = held.scale.potency(*gear_score) as f32;
            for (_, share) in held.shares.iter().filter(|(of, _)| *of == kind) {
                part.resistance += share.resistance * potency;
                part.absorption += share.absorption * potency;
            }
        }
        part
    }

    /// The meter kind `kind` (CRC-32 of its lowercase `AfflictionID`).
    pub fn kind(&self, kind: u32) -> Option<&AfflictionKind> {
        self.kinds.iter().find(|known| known.crc == kind)
    }

    /// What a hit of attack row `row` adds, before the absorption; `None` for a row that adds
    /// no affliction.
    pub fn hit_gain(&self, row: RowKey) -> Option<AfflictionGain> {
        self.hits.get(&row).copied()
    }

    /// What the use of consumable `consumable` (any case) changes.
    pub fn use_gains(&self, consumable: &str) -> &[AfflictionGain] {
        self.uses
            .get(&consumable.to_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// The tick of effect `effect`, when it changes meters.
    pub fn tick(&self, effect: u32) -> Option<&AfflictionTick> {
        self.ticks.iter().find(|tick| tick.effect == effect)
    }

    /// The effects that effect `effect` adds with it.
    pub fn equipped_by(&self, effect: u32) -> &[String] {
        self.equips
            .iter()
            .find(|(holder, _)| *holder == effect)
            .map_or(&[], |(_, equipped)| equipped.as_slice())
    }

    /// The affliction effect that gathering `gatherable` (CRC-32 of its lowercase id) adds.
    pub fn gathered_effect(&self, gatherable: u32) -> Option<&str> {
        self.gathered
            .iter()
            .find(|(known, _)| *known == gatherable)
            .map(|(_, effect)| effect.as_str())
    }
}

/// Seconds `seconds` of a cell in whole milliseconds; 0 for a negative cell.
fn ms(seconds: f64) -> u64 {
    // Cells hold a few thousand seconds at most.
    (seconds.max(0.0) * MS_PER_S).round() as u64
}

/// The lowercase rest of `affliction_id` after [`AFFLICTION_PREFIX`] (`blight`).
fn kind_suffix(affliction_id: &str) -> String {
    affliction_id
        .strip_prefix(AFFLICTION_PREFIX)
        .unwrap_or(affliction_id)
        .to_lowercase()
}

/// The entries of an affliction cell (`AfflictionBlight=5`, `AfflictionCurse=-1000+...`); an
/// entry without a number is left out.
fn gains_of(cell: Option<&str>) -> Vec<AfflictionGain> {
    cell.map_or_else(Vec::new, |cell| {
        cell.split(ENTRY_SEPARATORS)
            .filter_map(|entry| {
                let (name, value) = entry.split_once(AFFLICTION_VALUE_SEPARATOR)?;
                Some(AfflictionGain {
                    kind: lowercase_crc32(name.trim()),
                    amount: value.trim().parse().ok()?,
                })
            })
            .collect()
    })
}

/// The `AFA<Kind>` cell of `row` for the kind of column suffix `suffix`.
fn effect_absorption(row: &StatusEffectData, suffix: &str) -> Option<f64> {
    match suffix {
        "bleed" => row.afa_bleed,
        "blight" => row.afa_blight,
        "curse" => row.afa_curse,
        "disease" => row.afa_disease,
        "erosion" => row.afa_erosion,
        "frostbite" => row.afa_frostbite,
        "poison" => row.afa_poison,
        "spores" => row.afa_spores,
        "stunned" => row.afa_stunned,
        _ => None,
    }
}

/// The `Res<Kind>` cell of status effect `row` for the kind of column suffix `suffix`.
fn effect_resistance(row: &StatusEffectData, suffix: &str) -> Option<f64> {
    let cell = match suffix {
        "bleed" => row.res_bleed,
        "blight" => row.res_blight,
        "curse" => row.res_curse,
        "disease" => row.res_disease,
        "frostbite" => row.res_frostbite,
        "poison" => row.res_poison,
        "spores" => row.res_spores,
        _ => None,
    };
    // Resistance cells hold a few hundred at most.
    cell.map(|value| value as f64)
}

/// The `Res<Kind>` cell of vitals `row` for the kind of column suffix `suffix`.
fn vitals_resistance(row: &VitalsBaseData, suffix: &str) -> Option<f64> {
    // Resistance cells hold a few hundred at most.
    match suffix {
        "bleed" => row.res_bleed,
        "blight" => row.res_blight,
        "curse" => row.res_curse,
        "disease" => row.res_disease,
        "erosion" => row.res_erosion.map(|value| value as f64),
        "frostbite" => row.res_frostbite.map(|value| value as f64),
        "poison" => row.res_poison,
        "spores" => row.res_spores.map(|value| value as f64),
        _ => None,
    }
}

/// The perks of `tables` whose `Affix` has a `RES<Kind>` or `AFA<Kind>` cell of one of `kinds`
/// (module text); the first row of a perk id wins.
fn perk_shares(tables: &Tables, kinds: &[AfflictionKind]) -> HashMap<String, PerkShares> {
    let affixes: HashMap<String, &AffixStatData> = tables
        .affix_stat_data
        .iter()
        .flat_map(|table| &table.rows)
        .map(|row| (row.status_id.to_lowercase(), row))
        .collect();
    let mut perks = HashMap::new();
    for row in tables.perk_data.iter().flat_map(|table| &table.rows) {
        let Some(affix) = row
            .affix
            .as_deref()
            .and_then(|affix| affixes.get(&affix.trim().to_lowercase()))
        else {
            continue;
        };
        let shares: Vec<(u32, AffixShare)> = kinds
            .iter()
            .filter_map(|kind| {
                let suffix = kind_suffix(&kind.id);
                let share = AffixShare {
                    resistance: affix_resistance(affix, &suffix).unwrap_or_default() as f32,
                    absorption: affix_absorption(affix, &suffix).unwrap_or_default() as f32,
                };
                (share != AffixShare::default()).then_some((kind.crc, share))
            })
            .collect();
        if !shares.is_empty() {
            perks
                .entry(row.perk_id.to_lowercase())
                .or_insert_with(|| PerkShares {
                    scale: GearScoreScale::of_cell(row.scaling_per_gear_score.as_deref()),
                    shares,
                });
        }
    }
    perks
}

/// The `RES<Kind>` cell of affix `row` for the kind of column suffix `suffix` (the cells of
/// the other kinds hold nothing in build 25810808).
fn affix_resistance(row: &AffixStatData, suffix: &str) -> Option<f64> {
    match suffix {
        "blight" => row.res_blight,
        "curse" => row.res_curse,
        // Resistance cells hold a few hundred at most.
        "poison" => row.res_poison.map(|value| value as f64),
        _ => None,
    }
}

/// The `AFA<Kind>` cell of affix `row` for the kind of column suffix `suffix` (as
/// [`affix_resistance`]).
fn affix_absorption(row: &AffixStatData, suffix: &str) -> Option<f64> {
    match suffix {
        "blight" => row.afa_blight,
        "curse" => row.afa_curse,
        "poison" => row.afa_poison,
        _ => None,
    }
}

/// The ability of `row` equipped by effect `holder`; `None` for a row of another trigger
/// (module text).
/// The curse effects of `javelindata_cursemutations`: each curse's `CurseMinor` and `CurseMajor`,
/// a `Wildcard` id once per elemental `CategoryWildcard` (module text).
fn curse_roots(tables: &Tables) -> Vec<String> {
    let categories: BTreeSet<&str> = tables
        .elemental_mutation_static_data
        .iter()
        .flat_map(|table| &table.rows)
        .map(|row| row.category_wildcard.as_str())
        .collect();
    tables
        .curse_mutation_static_data
        .iter()
        .flat_map(|table| &table.rows)
        .flat_map(|row| [row.curse_minor.trim(), row.curse_major.trim()])
        .filter(|id| !id.is_empty())
        .flat_map(|id| {
            if id.contains(CURSE_WILDCARD) {
                categories
                    .iter()
                    .map(|category| id.replace(CURSE_WILDCARD, category))
                    .collect()
            } else {
                vec![id.to_owned()]
            }
        })
        .collect()
}

fn effect_ability(holder: u32, row: &AbilityData) -> Option<EffectAbility> {
    let crc_of = |cell: Option<&str>| {
        cell.map(str::trim)
            .filter(|cell| !cell.is_empty())
            .map(lowercase_crc32)
    };
    let trigger = if row.on_hit_taken == Some(true) {
        EffectTrigger::HitTaken
    } else if row.on_status_effect_applied == Some(true) {
        EffectTrigger::Applied(crc_of(row.status_effect_being_applied.as_deref()))
    } else if row.ability_trigger.as_deref().map(str::trim) == Some(ON_PERFORM) {
        EffectTrigger::Performed
    } else {
        return None;
    };
    let removes = row.remove_status_effects.as_deref().map(str::trim) == Some(REMOVE_LISTED);
    Some(EffectAbility {
        id: row.ability_id.clone(),
        holder,
        trigger,
        owned: if row.check_owned_status_effects == Some(true) {
            crc_of(row.status_effect.as_deref())
        } else {
            None
        },
        not_owned: crc_of(row.dont_have_status_effect.as_deref()),
        remove: if removes {
            list_of(row.status_effects_list.as_deref())
                .iter()
                .map(|id| lowercase_crc32(id))
                .collect()
        } else {
            Vec::new()
        },
        apply: list_of(row.self_apply_status_effect.as_deref()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{AfflictionData, ConsumableItemDefinitions, DamageData, GatherableData};
    use crate::table::Table;

    const APPLY: &str = "Catacombs_ApplyErosion";
    const HIT_ABILITY: &str = "AbilityID_Catacombs_OnHitTaken";
    const LEVEL_ABILITY: &str = "AbilityID_Catacombs_ErosionLv0_v2";
    const LEVEL_EFFECT: &str = "Catacombs_Erosion_Lv1_Ability_v2";
    const LV0: &str = "Erosion_lv0_Indicator";
    const LV1: &str = "Erosion_lv1_Indicator";
    const LV1_TEMP: &str = "Erosion_lv1_Temp";
    const SHRINE: &str = "Catacombs_HealthChest";
    const SHRINE_EFFECT: &str = "CAT_PowerUp_Health";
    const BLIGHT: &str = "AfflictionBlight";
    const GAS: &str = "AI_Fulgoris_Gas";
    const TINCTURE: &str = "TinctureBlightT2";
    /// The `Player` vitals row's `ResBlight` (build 25810808).
    const PLAYER_BLIGHT_RESISTANCE: f32 = 60.0;

    fn table<T>(rows: Vec<T>) -> Vec<Table<T>> {
        vec![Table {
            source_path: String::new(),
            unique_id: String::new(),
            rows,
        }]
    }

    fn effect(id: &str) -> StatusEffectData {
        serde_json::from_value(serde_json::json!({ "status_id": id })).expect("effect row")
    }

    fn ability(value: serde_json::Value) -> AbilityData {
        serde_json::from_value(value).expect("ability row")
    }

    fn affliction(value: serde_json::Value) -> AfflictionData {
        let mut row = serde_json::json!({
            "afflicted_tooltip_text": "", "affliction_tooltip_text": "", "color_hex": "",
            "filling_color_hex": "", "icon": "", "localized_text": "",
            "localized_text_afflicted": "", "ignore_when_afflicted": false
        });
        row.as_object_mut()
            .expect("row")
            .extend(value.as_object().expect("cells").clone());
        serde_json::from_value(row).expect("affliction row")
    }

    /// The retail rows of build 25810808 (`javelindata_statuseffects_catacombs`, `_ai`,
    /// `javelindata_ability_global`, `javelindata_afflictions`, `javelindata_basevitals_player`,
    /// `javelindata_itemdefinitions_consumables`), cut to their affliction cells.
    fn tables() -> Tables {
        let mut prototype = effect(EROSION_ROOT_EFFECT);
        prototype.equip_ability = Some(HIT_ABILITY.to_owned());
        let mut level = effect(LEVEL_EFFECT);
        level.equip_ability = Some(LEVEL_ABILITY.to_owned());
        let mut apply = effect(APPLY);
        apply.afflictions = Some("AfflictionErosion=2".to_owned());
        apply.tick_rate = Some(0.75);
        apply.delay_initial_tick = Some(0.1);
        let mut erosion = effect("Erosion");
        erosion.afa_erosion = Some(1.0);
        let mut shrine = effect(SHRINE_EFFECT);
        shrine.afflictions = Some("AfflictionErosion=-100".to_owned());
        shrine.tick_rate = Some(1.0);
        let mut gas = effect(GAS);
        gas.afflictions = Some("AfflictionBlight=5".to_owned());
        gas.tick_rate = Some(1.0);
        let mut ward = effect("MinorBlightAbsorb");
        ward.afa_blight = Some(0.1);
        let mut cloak = effect("AI_PalatineGuard_Sacrifice_AbyssCloak");
        cloak.res_blight = Some(1);
        let mut player: VitalsBaseData = serde_json::from_value(serde_json::json!({
            "vitals_id": DEFAULT_PLAYER_VITALS_ID, "can_drop_loot": false,
            "health_initial": "", "health_min": 0
        }))
        .expect("vitals row");
        player.res_blight = Some(f64::from(PLAYER_BLIGHT_RESISTANCE));
        let mut gatherable: GatherableData = serde_json::from_value(serde_json::json!({
            "gatherable_id": SHRINE, "base_gather_time": 1.5, "max_respawn_rate": 64002.0,
            "min_respawn_rate": 64000.0
        }))
        .expect("gatherable row");
        gatherable.added_status_effect = Some(SHRINE_EFFECT.to_owned());
        let mut tincture: ConsumableItemDefinitions =
            serde_json::from_value(serde_json::json!({ "consumable_id": TINCTURE }))
                .expect("consumable row");
        tincture.on_use_affliction = Some("AfflictionBlight=-9999+AfflictionMissing=-1".to_owned());
        Tables {
            status_effect_data: table(vec![
                prototype,
                level,
                apply,
                erosion,
                shrine,
                gas,
                ward,
                cloak,
                effect(LV0),
                effect(LV1),
                effect(LV1_TEMP),
                effect("Catacombs_Erosion_OnHitTaken"),
            ]),
            ability_data: table(vec![
                ability(serde_json::json!({
                    "ability_id": HIT_ABILITY, "on_hit_taken": true,
                    "on_equip_status_effect": format!("{LEVEL_EFFECT},{LV0}"),
                    "self_apply_status_effect": format!("Catacombs_Erosion_OnHitTaken,{APPLY}")
                })),
                ability(serde_json::json!({
                    "ability_id": LEVEL_ABILITY, "on_status_effect_applied": true,
                    "check_owned_status_effects": true, "status_effect": LV0,
                    "status_effect_being_applied": "Erosion",
                    "remove_status_effects": REMOVE_LISTED, "status_effects_list": LV0,
                    "self_apply_status_effect": format!("{LV1_TEMP},{LV1}")
                })),
            ]),
            affliction_data: table(vec![
                affliction(serde_json::json!({
                    "affliction_id": EROSION_AFFLICTION, "int_id": 15, "max": 40,
                    "afflicted_drain_time": 10.0, "drain_delay": 9999.0, "drain_rate": 0.0,
                    "status_effects": "Erosion"
                })),
                affliction(serde_json::json!({
                    "affliction_id": BLIGHT, "int_id": 8, "max": 0,
                    "afflicted_drain_time": 180.0, "drain_delay": 1.1, "drain_rate": 1.0,
                    "status_effects": "Blight"
                })),
                affliction(serde_json::json!({
                    "affliction_id": "AfflictionCurse2", "int_id": 11, "max": 45,
                    "afflicted_drain_time": 8.0, "drain_delay": 9999.0, "drain_rate": 9999.0,
                    "ignore_when_afflicted": true, "is_global_affliction": true,
                    "is_timer_affliction": true
                })),
            ]),
            vitals_base_data: table(vec![player]),
            gatherable_data: table(vec![gatherable]),
            consumable_item_definitions: table(vec![tincture]),
            ..Tables::default()
        }
    }

    #[test]
    fn every_meter_kind_but_the_global_ones_comes_from_its_row_and_the_player_vitals() {
        let rules = AfflictionRules::of_tables(&tables());
        assert_eq!(
            rules.kinds.len(),
            2,
            "the global curse is not a vitals meter"
        );
        assert_eq!(
            rules.kind(lowercase_crc32(EROSION_AFFLICTION)),
            Some(&AfflictionKind {
                crc: lowercase_crc32(EROSION_AFFLICTION),
                id: EROSION_AFFLICTION.to_owned(),
                int_id: 15,
                max: 40.0,
                resistance: 0.0,
                drain_delay_ms: 9_999_000,
                drain_per_s: 0.0,
                afflicted_ms: 10_000,
                ignore_when_afflicted: false,
                afflicted_effects: vec!["Erosion".to_owned()],
                timer: false,
            })
        );
        let blight = rules.kind(lowercase_crc32(BLIGHT)).expect("blight");
        assert_eq!(
            (blight.int_id, blight.max + blight.resistance),
            (8, PLAYER_BLIGHT_RESISTANCE),
            "retail cold max 60 (`CATACOMBS6` 1310.842 s)"
        );
        assert_eq!(
            (
                blight.drain_delay_ms,
                blight.drain_per_s,
                blight.afflicted_ms
            ),
            (1_100, 1.0, 180_000)
        );
    }

    #[test]
    fn the_effects_that_tick_absorb_and_resist_name_their_kind() {
        let rules = AfflictionRules::of_tables(&tables());
        let erosion = lowercase_crc32(EROSION_AFFLICTION);
        let blight = lowercase_crc32(BLIGHT);
        // 2 a tick: a light load's 2.0 a hit (`CATACOMBS5`).
        let apply = rules.tick(lowercase_crc32(APPLY)).expect("apply tick");
        assert_eq!(
            (apply.gains.clone(), apply.first_ms, apply.period_ms),
            (
                vec![AfflictionGain {
                    kind: erosion,
                    amount: 2.0
                }],
                100,
                750
            )
        );
        let gas = rules.tick(lowercase_crc32(GAS)).expect("gas tick");
        assert_eq!(
            (gas.gains.clone(), gas.first_ms, gas.period_ms),
            (
                vec![AfflictionGain {
                    kind: blight,
                    amount: 5.0
                }],
                0,
                1_000
            )
        );
        let mut absorptions = rules.absorptions.clone();
        absorptions.sort_by(|a, b| a.value.total_cmp(&b.value));
        assert_eq!(
            absorptions,
            [
                EffectShare {
                    effect: lowercase_crc32("MinorBlightAbsorb"),
                    kind: blight,
                    value: 0.1
                },
                EffectShare {
                    effect: lowercase_crc32("Erosion"),
                    kind: erosion,
                    value: 1.0
                },
            ]
        );
        assert_eq!(
            rules.resistances,
            [EffectShare {
                effect: lowercase_crc32("AI_PalatineGuard_Sacrifice_AbyssCloak"),
                kind: blight,
                value: 1.0
            }]
        );
    }

    #[test]
    fn a_consumable_changes_the_kinds_its_cell_names_that_are_meters() {
        let rules = AfflictionRules::of_tables(&tables());
        assert_eq!(
            rules.use_gains("tinctureblightt2"),
            [AfflictionGain {
                kind: lowercase_crc32(BLIGHT),
                amount: -9_999.0
            }]
        );
        assert!(rules.use_gains("PotionHeal").is_empty());
    }

    /// The tables with the Fiendish and Censored curse rows of build 25810808
    /// (`javelindata_cursemutations`, `javelindata_elementalmutations` cut to the fire row,
    /// `javelindata_statuseffects_common`, `javelindata_ability_global`).
    #[allow(
        clippy::field_reassign_with_default,
        reason = "the test sets only the sheets it reads"
    )]
    fn curse_tables() -> Tables {
        use crate::generated::{CurseMutationStaticData, ElementalMutationStaticData};
        let mut tables = tables();
        tables.elemental_mutation_static_data = table(vec![
            serde_json::from_value::<ElementalMutationStaticData>(serde_json::json!({
                "background_color": "", "category_wildcard": "Fir", "description": "",
                "dungeon": "", "dungeon_plus": "", "dungeon_minus": "", "dungeon_boss": "",
                "dungeon_mini_boss": "", "elemental_difficulty_tier": 3,
                "elemental_mutation_id": "Fir_3", "icon_path": "", "name": "", "text_color": ""
            }))
            .expect("elemental row"),
        ]);
        tables.curse_mutation_static_data = table(
            [
                (
                    "Fiendish",
                    "Mut_Curse2_Minor_Buff",
                    "Mut_Curse2_Wildcard_Major_Buff",
                ),
                (
                    "Censored",
                    "Mut_Wildcard_Curse4_Minor_Buff",
                    "Mut_Curse4_Major_Buff",
                ),
            ]
            .into_iter()
            .map(|(id, minor, major)| {
                serde_json::from_value::<CurseMutationStaticData>(serde_json::json!({
                    "curse_major": major, "curse_minor": minor, "curse_mutation_id": id,
                    "description": "", "global_affliction": null, "icon_path": "", "name": ""
                }))
                .expect("curse row")
            })
            .collect(),
        );
        let equipping = |id: &str, ability: &str| {
            let mut row = effect(id);
            row.equip_ability = Some(ability.to_owned());
            row
        };
        let mut curse_tick = effect(CURSE_TICK);
        curse_tick.afflictions = Some(format!("{CURSE2}=-1"));
        curse_tick.tick_rate = Some(1.0);
        curse_tick.base_duration = Some(0.9);
        let rows = &mut tables.status_effect_data[0].rows;
        rows.extend([
            equipping("Mut_Curse2_Minor_Buff", "Mut_Curse2_Minor"),
            equipping("Mut_Curse2_Fir_Major_Buff", "Mut_Curse2_Fir_Major"),
            equipping("Mut_Curse4_Major_Buff", "Mut_Curse4_Major"),
            curse_tick,
            effect(BLOOD_OFFERING),
            effect(EXHAUST),
        ]);
        let rows = &mut tables.ability_data[0].rows;
        rows.extend([
            ability(serde_json::json!({
                "ability_id": "Mut_Curse2_Minor", "ability_trigger": ON_PERFORM,
                "self_apply_status_effect": CURSE_TICK
            })),
            ability(serde_json::json!({
                "ability_id": "Mut_Curse2_Fir_Major", "ability_trigger": ON_PERFORM,
                "dont_have_status_effect": MORTAR_FIRED,
                "self_apply_status_effect": BLOOD_OFFERING
            })),
            ability(serde_json::json!({
                "ability_id": "Mut_Curse4_Major", "ability_trigger": ON_PERFORM,
                "self_apply_status_effect": EXHAUST
            })),
        ]);
        tables
    }

    const CURSE2: &str = "AfflictionCurse2";
    const CURSE_TICK: &str = "Mut_Curse2_Affliction_Tick";
    const BLOOD_OFFERING: &str = "Mut_Curse2_Fir_Major_Damage";
    const EXHAUST: &str = "Mut_Curse4_Major_Exhaust";
    const MORTAR_FIRED: &str = "Blunderbuss_MortarShotFired";

    /// The global curse is a meter of the game mode, not of the vitals; its tick effect changes
    /// it; the curses' `OnPerform` abilities are reached from the minor and the majors with the
    /// category (Blood Offering, Exhaust).
    #[test]
    fn the_global_curse_and_the_curses_on_perform_abilities_come_from_the_curse_rows() {
        let rules = AfflictionRules::of_tables(&curse_tables());
        let curse = rules
            .global_kind(lowercase_crc32(CURSE2))
            .expect("global kind");
        assert_eq!(
            (curse.int_id, curse.max, curse.afflicted_ms, curse.timer),
            (11, 45.0, 8_000, true)
        );
        assert!(curse.ignore_when_afflicted);
        assert!(rules.kind(curse.crc).is_none(), "not a vitals meter");
        assert_eq!(rules.meter_kind(curse.crc), Some(curse));
        let tick = rules.tick(lowercase_crc32(CURSE_TICK)).expect("tick");
        assert_eq!(
            tick.gains,
            [AfflictionGain {
                kind: curse.crc,
                amount: -1.0
            }]
        );
        let performed = |holder: &str| -> Vec<(&str, Vec<String>, Option<u32>)> {
            rules
                .performed_by(lowercase_crc32(holder))
                .map(|ability| {
                    (
                        ability.id.as_str(),
                        ability.apply.clone(),
                        ability.not_owned,
                    )
                })
                .collect()
        };
        assert_eq!(
            performed("Mut_Curse2_Minor_Buff"),
            [("Mut_Curse2_Minor", vec![CURSE_TICK.to_owned()], None)]
        );
        assert_eq!(
            performed("Mut_Curse2_Fir_Major_Buff"),
            [(
                "Mut_Curse2_Fir_Major",
                vec![BLOOD_OFFERING.to_owned()],
                Some(lowercase_crc32(MORTAR_FIRED))
            )]
        );
        assert_eq!(
            performed("Mut_Curse4_Major_Buff"),
            [("Mut_Curse4_Major", vec![EXHAUST.to_owned()], None)]
        );
    }

    #[test]
    fn the_root_equips_the_level_abilities_and_the_shrine_gather_names_its_effect() {
        let rules = AfflictionRules::of_tables(&tables());
        assert_eq!(
            rules.equipped_by(lowercase_crc32(EROSION_ROOT_EFFECT)),
            [LEVEL_EFFECT.to_owned(), LV0.to_owned()]
        );
        let hit = rules
            .abilities
            .iter()
            .find(|ability| ability.id == HIT_ABILITY)
            .expect("hit ability");
        assert_eq!(hit.trigger, EffectTrigger::HitTaken);
        assert_eq!(hit.apply, ["Catacombs_Erosion_OnHitTaken", APPLY]);
        let level = rules
            .abilities
            .iter()
            .find(|ability| ability.id == LEVEL_ABILITY)
            .expect("level ability");
        assert_eq!(level.holder, lowercase_crc32(LEVEL_EFFECT));
        assert_eq!(
            level.trigger,
            EffectTrigger::Applied(Some(lowercase_crc32("Erosion")))
        );
        assert_eq!(level.owned, Some(lowercase_crc32(LV0)));
        assert_eq!(level.remove, [lowercase_crc32(LV0)]);
        assert_eq!(level.apply, [LV1_TEMP, LV1]);
        assert_eq!(
            rules.gathered_effect(lowercase_crc32(SHRINE)),
            Some(SHRINE_EFFECT)
        );
        assert_eq!(
            rules.gathered_effect(lowercase_crc32("Catacombs_Other")),
            None
        );
    }

    #[test]
    fn a_worn_resist_perk_raises_the_curse_top_and_takes_a_share_of_each_gain_by_gear_score() {
        // Build 25810808 rows; retail `MSQ6-lv28` 1335.582 s (module text).
        const CURSE: &str = "AfflictionCurse";
        const PERK: &str = "PerkID_Armor_ResistCorrupt";
        const AFFIX: &str = "Affix_Armor_ResistCorrupt";
        const SCALING: &str = "0.00381818181818182,550:0.00";
        const POUCH_GEAR_SCORE: u32 = 220;
        const PLAYER_CURSE_RESISTANCE: f64 = 60.0;
        /// The half floats retail sent: the cold top and the first `ApplyCurseL1` tick (+1).
        const RETAIL_TOP: f32 = 67.3125;
        const RETAIL_TICK: f32 = 0.927_246_1;
        /// Half float steps near 67 and near 0.93.
        const TOP_STEP: f32 = 0.0625;
        const TICK_STEP: f32 = 0.000_488_3;
        let mut tables = tables();
        tables.affliction_data[0]
            .rows
            .push(affliction(serde_json::json!({
                "affliction_id": CURSE, "int_id": 5, "max": 0,
                "afflicted_drain_time": 0.0, "drain_delay": 3.1, "drain_rate": 0.4,
                "ignore_when_afflicted": true, "status_effects": "Curse"
            })));
        tables.vitals_base_data[0].rows[0].res_curse = Some(PLAYER_CURSE_RESISTANCE);
        let mut affix: AffixStatData =
            serde_json::from_value(serde_json::json!({ "status_id": AFFIX })).expect("affix row");
        affix.res_curse = Some(5.0);
        affix.afa_curse = Some(0.05);
        let mut perk: crate::generated::PerkData = serde_json::from_value(serde_json::json!({
            "perk_id": PERK, "perk_type": "Generated", "condition_event": "OnEquip",
            "channel": 1, "affix": AFFIX, "scaling_per_gear_score": SCALING
        }))
        .expect("perk row");
        let plain: crate::generated::PerkData = serde_json::from_value(serde_json::json!({
            "perk_id": "PerkID_StatSwap_HP", "perk_type": "Generated",
            "condition_event": "OnEquip", "channel": 1, "affix": "Affix_StatSwap_HP"
        }))
        .expect("plain perk row");
        tables.affix_stat_data = table(vec![affix]);
        tables.perk_data = table(vec![perk.clone(), plain]);
        let rules = AfflictionRules::of_tables(&tables);
        let curse = rules.kind(lowercase_crc32(CURSE)).expect("curse");
        assert_eq!(
            rules.perks.len(),
            1,
            "a perk whose affix has no part holds none"
        );
        let worn = [
            (PERK, POUCH_GEAR_SCORE),
            ("PerkID_StatSwap_HP", POUCH_GEAR_SCORE),
        ];
        let part = rules.worn_part(&worn, curse.crc);
        let top = curse.resistance + curse.max + part.resistance;
        assert!((top - RETAIL_TOP).abs() <= TOP_STEP / 2.0, "top {top}");
        let tick = 1.0 - part.absorption;
        assert!((tick - RETAIL_TICK).abs() <= TICK_STEP / 2.0, "tick {tick}");
        assert_eq!(
            rules.worn_part(&worn, lowercase_crc32(BLIGHT)),
            AffixShare::default(),
            "the curse affix has no blight part"
        );
        let twice = rules.worn_part(&[(PERK, 100), (PERK, 100)], curse.crc);
        assert_eq!(
            (twice.resistance, twice.absorption),
            (10.0, 0.1),
            "two worn perks add up; at gear score 100 the potency is 1"
        );
        let capped = rules.worn_part(&[(PERK, 700)], curse.crc);
        let at_cap = rules.worn_part(&[(PERK, 550)], curse.crc);
        assert_eq!(capped, at_cap, "the scale stops at 550");
        perk.affix = None;
        tables.perk_data = table(vec![perk]);
        assert!(AfflictionRules::of_tables(&tables).perks.is_empty());
    }

    #[test]
    fn tables_without_afflictions_give_no_kind() {
        let rules = AfflictionRules::of_tables(&Tables::default());
        assert!(rules.kinds.is_empty());
        assert!(rules.abilities.is_empty());
        assert!(rules.hits.is_empty());
        assert!(rules.uses.is_empty());
    }

    #[test]
    fn the_equip_load_effects_absorb_and_the_mob_rows_add_their_affliction() {
        /// The retail rows of `javelindata_statuseffects_common`,
        /// `javelindata_damagetable_catacombs_agheavyboss`, `_risen_ftw` and
        /// `_naga_corrupted` (build 25810808).
        const MEDIUM: &str = "EquipLoad_CritDmgReduction_Medium";
        const HEAVY: &str = "EquipLoad_CritDmgReduction_Heavy";
        const BOSS_TABLE: &str = "DamageTable_Catacombs_AGHeavyBoss";
        const SLASH: &str = "Attack_Elite_Double_Slash_1st_Hit";
        const OTHER: &str = "Attack_Elite_Kick";
        const SHIFTED: &str = "CorruptionCore_Explode";
        const SLASH_GAIN: f32 = 3.0;
        let mut medium = effect(MEDIUM);
        medium.afa_erosion = Some(0.25);
        let mut heavy = effect(HEAVY);
        heavy.afa_erosion = Some(0.6);
        let damage = |id: &str, affliction: Option<&str>, add: Option<&str>| {
            let mut row: DamageData =
                serde_json::from_value(serde_json::json!({ "damage_id": id })).expect("damage row");
            row.affliction = affliction.map(str::to_owned);
            row.add_affliction = add.map(str::to_owned);
            row
        };
        let mut tables = tables();
        tables.status_effect_data[0].rows.extend([medium, heavy]);
        tables.damage_data = vec![Table {
            source_path: String::new(),
            unique_id: BOSS_TABLE.to_owned(),
            rows: vec![
                damage(SLASH, Some(EROSION_AFFLICTION), Some("3")),
                damage(OTHER, Some("AfflictionPoison"), Some("8")),
                damage(SHIFTED, None, Some("Corruption")),
            ],
        }];
        let rules = AfflictionRules::of_tables(&tables);
        let erosion = lowercase_crc32(EROSION_AFFLICTION);
        let mut shares: Vec<(u32, f32)> = rules
            .absorptions
            .iter()
            .filter(|share| share.kind == erosion)
            .map(|share| (share.effect, share.value))
            .collect();
        shares.sort_by(|a, b| a.1.total_cmp(&b.1));
        assert_eq!(
            shares,
            [
                (lowercase_crc32(MEDIUM), 0.25),
                (lowercase_crc32(HEAVY), 0.6),
                (lowercase_crc32("Erosion"), 1.0),
            ]
        );
        assert_eq!(
            rules.hit_gain(RowKey::of(BOSS_TABLE, SLASH)),
            Some(AfflictionGain {
                kind: erosion,
                amount: SLASH_GAIN
            })
        );
        assert_eq!(
            rules.hit_gain(RowKey::of(BOSS_TABLE, OTHER)),
            None,
            "poison is no meter of these tables"
        );
        assert_eq!(rules.hit_gain(RowKey::of(BOSS_TABLE, SHIFTED)), None);
    }
}
