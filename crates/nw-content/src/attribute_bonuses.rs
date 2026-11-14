// SPDX-License-Identifier: AGPL-3.0-or-later
//! The abilities an attribute value turns on (abilities design note, 2026-10-31): each
//! `AttributeDefinition` row names, in `EquipAbilities`, the `AttributeThresholdAbilityTable`
//! abilities a value of its `Level` or more holds (Focus 100: `Foc_Bonus_100_1,Foc_Bonus_100_2`).
//! Their `AbilityData` cells change the mana and the cooldowns: `MaxMana` (Focus 100: 0.1),
//! `ManaRate` (Focus 100: 0.2) and `CooldownTimer` (Focus 25: -0.03), each a share added to 1.
//! **Observed** (retail `general-other-01`, Focus 250): a mana max of 110, the 100 of
//! `ManaBaseMax` and the 0.1 of `Foc_Bonus_100_1`.
//!
//! A threshold ability's `OnEquipStatusEffect` is the status effect its holder has while it
//! holds the ability ([`AttributeBonuses::equip_effects`]); Observed (build 25810808): the second
//! ability of each row names one (`Foc_Bonus_50_2`: `ATB_Foc_Bonus_50_2_Effect`), and some first
//! abilities too (`Foc_Bonus_50_1`: `ATB_Foc_Bonus_50_1_Effect`, `HealMod` 0.05). Those of the
//! second abilities hold lasting (`BaseDuration` -1) fishing and gathering stats: gathering efficiency (`EFF_*`, the 25 and 200
//! rows), yield (`MULT_*`, 250), luck (`ROL_*`, Constitution 50 and the 350 rows), the fish size
//! roll (Focus 25, 200, 250) and cast distance (Focus 50, [`crate::fishing`]).

use std::collections::HashMap;

use crate::Tables;
use crate::character_points::ATTRIBUTE_NAMES;

/// Separates the abilities of an `EquipAbilities` cell.
const ABILITY_SEPARATOR: char = ',';

/// What the held abilities add to the mana and the cooldowns, as shares of the base.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AbilityStats {
    /// `MaxMana`.
    pub max_mana: f64,
    /// `ManaRate`.
    pub mana_rate: f64,
    /// `CooldownTimer` (below 0: shorter cooldowns).
    pub cooldown: f64,
}

impl AbilityStats {
    fn plus(self, other: Self) -> Self {
        Self {
            max_mana: self.max_mana + other.max_mana,
            mana_rate: self.mana_rate + other.mana_rate,
            cooldown: self.cooldown + other.cooldown,
        }
    }
}

/// The share that a held ability's `CooldownTimer` adds to every cooldown it starts: none for a
/// row with `CDRImmediatelyOptions`, whose timer cuts running cooldowns once at its event
/// ([`crate::passive_abilities`]). Build 25810808: 131 rows with a timer have the option (the
/// flail's `Upgrade_Flail_Slam_Upgrade1`, `AbilitySpecific` for its Slam on a hit;
/// `Foc_Bonus_350_1`, `All` when a heart rune is used); `Foc_Bonus_25_1` (-0.03) has none.
/// Ours added the 131 to every cooldown: the server's flail and life staff cooldowns ended
/// before the client's, and the client played no animation for a press the server took
/// (live 2026-11-14).
fn lasting_cooldown(timer: Option<f64>, immediately: Option<&str>) -> f64 {
    if immediately.is_some_and(|options| !options.trim().is_empty()) {
        return 0.0;
    }
    timer.unwrap_or_default()
}

/// The threshold abilities of every attribute and their stats.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttributeBonuses {
    /// Attribute id, the `Level` from which they hold, and the abilities.
    thresholds: Vec<(u32, u32, Vec<String>)>,
    /// The stats of the abilities, by lowercase `AbilityID`.
    stats: HashMap<String, AbilityStats>,
    /// The `OnEquipStatusEffect` of the threshold abilities that have one, by lowercase
    /// `AbilityID`.
    equip_effects: HashMap<String, String>,
}

impl AttributeBonuses {
    /// The bonuses of `tables`: the `EquipAbilities` of each attribute table's rows, and the
    /// `MaxMana`, `ManaRate` and lasting `CooldownTimer` ([`lasting_cooldown`]) of every
    /// `AbilityData` row.
    pub fn from_tables(tables: &Tables) -> Self {
        let thresholds = tables.attribute_definition.iter().flat_map(|table| {
            let attribute = ATTRIBUTE_NAMES
                .iter()
                .position(|name| name.eq_ignore_ascii_case(&table.unique_id));
            table.rows.iter().filter_map(move |row| {
                let attribute = u32::try_from(attribute?).ok()?;
                Some((
                    attribute,
                    u32::try_from(row.level).ok()?,
                    row.equip_abilities.as_deref()?.to_owned(),
                ))
            })
        });
        let stats = tables
            .ability_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                (
                    row.ability_id.clone(),
                    AbilityStats {
                        max_mana: row.max_mana.unwrap_or_default(),
                        mana_rate: row.mana_rate.unwrap_or_default(),
                        cooldown: lasting_cooldown(
                            row.cooldown_timer,
                            row.cdr_immediately_options.as_deref(),
                        ),
                    },
                )
            })
            .filter(|(_, stats)| *stats != AbilityStats::default());
        let equip_effects = tables
            .ability_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let effect = row.on_equip_status_effect.as_deref()?.trim();
                (!effect.is_empty()).then(|| (row.ability_id.clone(), effect.to_owned()))
            });
        Self::build(thresholds, stats).with_equip_effects(equip_effects)
    }

    /// These bonuses with the `OnEquipStatusEffect` of their threshold abilities among
    /// `effects` (ability id, status id); an ability of no threshold is left out.
    #[must_use]
    pub fn with_equip_effects(
        mut self,
        effects: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let threshold: std::collections::HashSet<String> = self
            .thresholds
            .iter()
            .flat_map(|(_, _, abilities)| abilities.iter().map(|ability| ability.to_lowercase()))
            .collect();
        self.equip_effects = effects
            .into_iter()
            .map(|(ability, effect)| (ability.to_lowercase(), effect))
            .filter(|(ability, _)| threshold.contains(ability))
            .collect();
        self
    }

    /// The bonuses of the `thresholds` (attribute id, level, `EquipAbilities` cell) and the
    /// ability `stats` (id, stats).
    pub fn build(
        thresholds: impl IntoIterator<Item = (u32, u32, String)>,
        stats: impl IntoIterator<Item = (String, AbilityStats)>,
    ) -> Self {
        Self {
            thresholds: thresholds
                .into_iter()
                .map(|(attribute, level, cell)| {
                    let abilities = cell
                        .split(ABILITY_SEPARATOR)
                        .map(str::trim)
                        .filter(|ability| !ability.is_empty())
                        .map(str::to_owned)
                        .collect();
                    (attribute, level, abilities)
                })
                .collect(),
            stats: stats
                .into_iter()
                .map(|(id, stats)| (id.to_lowercase(), stats))
                .collect(),
            equip_effects: HashMap::new(),
        }
    }

    /// The abilities that the attribute values `value_of` (attribute id to its value) hold.
    pub fn abilities(&self, value_of: impl Fn(u32) -> u32) -> Vec<&str> {
        self.thresholds
            .iter()
            .filter(|(attribute, level, _)| value_of(*attribute) >= *level)
            .flat_map(|(_, _, abilities)| abilities.iter().map(String::as_str))
            .collect()
    }

    /// The status effects that the threshold abilities among `abilities` (any case) hold.
    pub fn equip_effects<'a>(
        &'a self,
        abilities: impl IntoIterator<Item = &'a str>,
    ) -> impl Iterator<Item = &'a str> {
        abilities
            .into_iter()
            .filter_map(|ability| self.equip_effects.get(&ability.to_lowercase()))
            .map(String::as_str)
    }

    /// The summed stats of `abilities` (any case; one without stats adds nothing).
    pub fn stats_of<'a>(&self, abilities: impl IntoIterator<Item = &'a str>) -> AbilityStats {
        abilities
            .into_iter()
            .filter_map(|ability| self.stats.get(&ability.to_lowercase()))
            .fold(AbilityStats::default(), |sum, stats| sum.plus(*stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOCUS: u32 = 1;
    const STRENGTH: u32 = 2;

    /// The Focus rows 25 and 100 and their abilities (build 25810808).
    fn bonuses() -> AttributeBonuses {
        AttributeBonuses::build(
            [
                (FOCUS, 25, "Foc_Bonus_25_1,Foc_Bonus_25_2".to_owned()),
                (FOCUS, 100, "Foc_Bonus_100_1, Foc_Bonus_100_2".to_owned()),
                (STRENGTH, 25, "Str_Bonus_25_1".to_owned()),
            ],
            [
                (
                    "Foc_Bonus_25_1".to_owned(),
                    AbilityStats {
                        cooldown: -0.03,
                        ..AbilityStats::default()
                    },
                ),
                (
                    "Foc_Bonus_100_1".to_owned(),
                    AbilityStats {
                        max_mana: 0.1,
                        mana_rate: 0.2,
                        ..AbilityStats::default()
                    },
                ),
            ],
        )
    }

    #[test]
    fn an_attribute_value_holds_the_abilities_of_its_rows_and_their_stats_add_up() {
        let bonuses = bonuses();
        let focus = |value: u32| move |attribute: u32| if attribute == FOCUS { value } else { 5 };
        assert!(bonuses.abilities(focus(24)).is_empty());
        assert_eq!(
            bonuses.abilities(focus(250)),
            [
                "Foc_Bonus_25_1",
                "Foc_Bonus_25_2",
                "Foc_Bonus_100_1",
                "Foc_Bonus_100_2"
            ]
        );
        let stats = bonuses.stats_of(bonuses.abilities(focus(250)));
        assert!((stats.max_mana - 0.1).abs() < 1e-9);
        assert!((stats.mana_rate - 0.2).abs() < 1e-9);
        assert!((stats.cooldown + 0.03).abs() < 1e-9);
        assert_eq!(
            bonuses.stats_of(["foc_bonus_25_2"]),
            AbilityStats::default()
        );
    }

    #[test]
    fn a_cooldown_cut_at_an_event_is_no_lasting_share() {
        assert!((lasting_cooldown(Some(-0.03), None) + 0.03).abs() < 1e-9);
        assert!((lasting_cooldown(Some(-0.03), Some(" ")) + 0.03).abs() < 1e-9);
        assert_eq!(lasting_cooldown(Some(-0.05), Some("AbilitySpecific")), 0.0);
        assert_eq!(lasting_cooldown(Some(-0.07), Some("All")), 0.0);
        assert_eq!(lasting_cooldown(None, None), 0.0);
    }

    /// The real rows: Focus 25 cuts every cooldown by 3 %; Focus 350 and the Slam's first
    /// upgrade cut only at their events (build 25810808).
    #[test]
    #[ignore = "needs the build 25810808 content bundle in NW_CONTENT_DIR"]
    fn the_real_event_cuts_are_no_lasting_share() {
        const BUILD: &str = "25810808";
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let path = crate::bundle::bundle_path(std::path::Path::new(&dir), BUILD);
        let bundle = crate::ContentBundle::load(&path, BUILD).expect("bundle");
        let bonuses = AttributeBonuses::from_tables(&bundle.tables);
        assert!((bonuses.stats_of(["Foc_Bonus_25_1"]).cooldown + 0.03).abs() < 1e-9);
        for event_cut in [
            "Foc_Bonus_350_1",
            "Upgrade_Flail_Slam_Upgrade1",
            "Passive_Flail_Eldritch_Passive2",
            "GlobalPerk_Ability_Flail_DefensiveStrike",
        ] {
            assert_eq!(bonuses.stats_of([event_cut]).cooldown, 0.0, "{event_cut}");
        }
    }

    #[test]
    fn a_held_threshold_ability_holds_its_equip_effect() {
        let bonuses = bonuses().with_equip_effects([
            (
                "Foc_Bonus_25_2".to_owned(),
                "ATB_Foc_Bonus_25_2_Effect".to_owned(),
            ),
            ("Not_A_Threshold".to_owned(), "Some_Effect".to_owned()),
        ]);
        let effects: Vec<&str> = bonuses
            .equip_effects(["foc_bonus_25_2", "Foc_Bonus_100_1", "Not_A_Threshold"])
            .collect();
        assert_eq!(effects, ["ATB_Foc_Bonus_25_2_Effect"]);
    }

    /// Focus 50 holds `Foc_Bonus_50_2`, whose effect makes the cast 3 % farther (build 25810808).
    #[test]
    #[ignore = "needs the build 25810808 content bundle in NW_CONTENT_DIR"]
    fn the_real_focus_50_bonus_holds_the_cast_distance_effect() {
        const BUILD: &str = "25810808";
        const FOCUS_50: u32 = 50;
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let path = crate::bundle::bundle_path(std::path::Path::new(&dir), BUILD);
        let bundle = crate::ContentBundle::load(&path, BUILD).expect("bundle");
        let bonuses = AttributeBonuses::from_tables(&bundle.tables);
        let abilities =
            bonuses.abilities(|attribute| if attribute == FOCUS { FOCUS_50 } else { 0 });
        let effects: Vec<&str> = bonuses.equip_effects(abilities).collect();
        assert_eq!(
            effects,
            [
                "ATB_Foc_Bonus_25_2_Effect",
                "ATB_Foc_Bonus_50_1_Effect",
                "ATB_Foc_Bonus_50_2_Effect"
            ]
        );
        let fishing = crate::fishing::FishingRules::from_tables(&bundle.tables);
        let stats = fishing.effect_stats(effects);
        assert!((stats.cast_distance - 0.03).abs() < 1e-9);
        assert!((stats.modifiers.size - 0.5).abs() < 1e-9);
    }
}
