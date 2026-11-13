// SPDX-License-Identifier: AGPL-3.0-or-later
//! The weight of each item for the equip load and the player's equip load limits (dodge study
//! 2026-09-20). An item weighs its stats row's `WeightOverride` (`ArmorItemDefinitions` or
//! `WeaponItemDefinitions`, by the master row's `ItemStatsRef`) where that row has one, else its
//! master row's `Weight`. Retail's new character dodged with its open-world kit, whose master
//! weights (555, over the 500 maximum) would make it overburdened, unable to dodge, and whose
//! armor overrides give 470.9: Inferred, the overrides count. The limits are the
//! `EncumbranceData` row of the player container.
//!
//! In the bag an item weighs its weight times one plus the `ItemClassWeightMods` shares of the
//! holder's effects for the item's classes (`ItemClass` of its master row): the Strength 150
//! effect's `Ore=-0.1` makes `OreT1` (`Resource+Ore`) weigh 0.9 of its weight ("-10% weight of
//! mined items"). Inferred: the shares of all matched classes add up (no item of build 25810808
//! has two classes with a share of one effect), and a weight is never below 0.

use std::collections::HashMap;

use crate::Tables;

/// Separates the classes of an `ItemClass` cell (`Resource+Ore`).
const CLASS_SEPARATOR: char = '+';
/// The weight multiple of an item without a weight share.
const FULL_WEIGHT: f64 = 1.0;

/// The `ContainerTypeID` of the player's row of `EncumbranceData`.
pub const PLAYER_CONTAINER: &str = "Player";

/// The player's equip load maximum and the category ratios, percent of the maximum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EquipLoadRatios {
    /// `EquipLoadBaseMax`.
    pub base_max: f64,
    /// `EquipLoadRatioNormal`.
    pub normal_percent: f64,
    /// `EquipLoadRatioSlow`.
    pub slow_percent: f64,
    /// `EquipLoadRatioOverburdened`.
    pub overburdened_percent: f64,
    /// `EquipLoadDamageMultFast`, `...Normal`, `...Slow` and `EquipLoadDamageOverburdened`: the
    /// share each category adds to the damage of the player's hits (build 25810808: 0.15, 0,
    /// -0.15; Observed: the retail hits of two characters with a slow load hold -0.15 in their
    /// additive sum).
    pub damage_mods: EquipLoadDamage,
}

/// The share each equip load category adds to the damage of the player's hits.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EquipLoadDamage {
    /// Under the normal ratio.
    pub fast: f64,
    /// From the normal ratio.
    pub normal: f64,
    /// From the slow ratio.
    pub slow: f64,
    /// From the overburdened ratio.
    pub overburdened: f64,
}

/// The item weights and the limits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EquipLoads {
    /// The weight of each item, by its lowercase id.
    weights: HashMap<String, f64>,
    /// The lowercase `ItemClass` entries of each item, by its lowercase id.
    classes: HashMap<String, Vec<String>>,
    /// The player's limits, when the tables hold its row.
    pub limits: Option<EquipLoadRatios>,
}

impl EquipLoads {
    /// The weights of `items` (item id, master `Weight`, `ItemStatsRef`) with the stats rows'
    /// `overrides` (stats id, `WeightOverride`), and `limits`.
    pub fn new<'a>(
        items: impl IntoIterator<Item = (&'a str, Option<f64>, Option<&'a str>)>,
        overrides: impl IntoIterator<Item = (&'a str, Option<f64>)>,
        limits: Option<EquipLoadRatios>,
    ) -> Self {
        let overrides: HashMap<String, f64> = overrides
            .into_iter()
            .filter_map(|(id, weight)| Some((id.to_lowercase(), weight?)))
            .collect();
        let weights = items
            .into_iter()
            .filter_map(|(id, weight, stats)| {
                let weight = stats
                    .and_then(|stats| overrides.get(&stats.to_lowercase()).copied())
                    .or(weight)?;
                Some((id.to_lowercase(), weight))
            })
            .collect();
        Self {
            weights,
            classes: HashMap::new(),
            limits,
        }
    }

    /// These weights with the `ItemClass` cells of `items` (item id, cell).
    #[must_use]
    pub fn with_classes<'a>(
        mut self,
        items: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    ) -> Self {
        self.classes = items
            .into_iter()
            .filter_map(|(id, cell)| {
                let classes: Vec<String> = cell?
                    .split(CLASS_SEPARATOR)
                    .map(str::trim)
                    .filter(|class| !class.is_empty())
                    .map(str::to_lowercase)
                    .collect();
                Some((id.to_lowercase(), classes))
            })
            .collect();
        self
    }

    /// The weights and limits of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let items = tables
            .master_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                (
                    row.item_id.as_str(),
                    row.weight,
                    row.item_stats_ref.as_deref(),
                )
            });
        let armor = tables
            .armor_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.weapon_id.as_str(), row.weight_override));
        let weapons = tables
            .weapon_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.weapon_id.as_str(), row.weight_override));
        #[allow(
            clippy::cast_precision_loss,
            reason = "the equip load maximum and ratios are a few hundred"
        )]
        let limits = tables
            .encumbrance_data
            .iter()
            .flat_map(|table| &table.rows)
            .find(|row| row.container_type_id == PLAYER_CONTAINER)
            .map(|row| EquipLoadRatios {
                base_max: row.equip_load_base_max as f64,
                normal_percent: row.equip_load_ratio_normal as f64,
                slow_percent: row.equip_load_ratio_slow as f64,
                overburdened_percent: row.equip_load_ratio_overburdened as f64,
                damage_mods: EquipLoadDamage {
                    fast: row.equip_load_damage_mult_fast.unwrap_or(0.0),
                    normal: row
                        .equip_load_damage_mult_normal
                        .map_or(0.0, |share| share as f64),
                    slow: row.equip_load_damage_mult_slow.unwrap_or(0.0),
                    overburdened: row
                        .equip_load_damage_overburdened
                        .map_or(0.0, |share| share as f64),
                },
            });
        let classes = tables
            .master_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.item_id.as_str(), row.item_class.as_deref()));
        Self::new(items, armor.chain(weapons), limits).with_classes(classes)
    }

    /// The weight of item `item_id` (any case); 0 for an item the tables do not weigh.
    pub fn weight(&self, item_id: &str) -> f64 {
        self.weights
            .get(&item_id.to_lowercase())
            .copied()
            .unwrap_or_default()
    }

    /// The weight of one item `item_id` (any case) in a bag whose holder's effects give the
    /// weight shares `weight_mods` (lowercase item class, share; see the module text).
    pub fn bag_weight(&self, item_id: &str, weight_mods: &[(String, f64)]) -> f64 {
        let id = item_id.to_lowercase();
        let share: f64 = self.classes.get(&id).map_or(0.0, |classes| {
            weight_mods
                .iter()
                .filter(|(class, _)| classes.contains(class))
                .map(|(_, share)| share)
                .sum()
        });
        self.weight(&id) * (FULL_WEIGHT + share).max(0.0)
    }

    /// The equip load of the equipped `items`: the sum of their weights.
    pub fn load<'a>(&self, items: impl IntoIterator<Item = &'a str>) -> f64 {
        items.into_iter().map(|item| self.weight(item)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOSE: f64 = 1e-9;

    /// The Swordsman kit's tutorial gear: master rows and their stats rows' overrides.
    fn swordsman() -> EquipLoads {
        EquipLoads::new(
            [
                (
                    "SwordsmanArchetypeChest",
                    Some(120.0),
                    Some("HeavyChest_Balanced"),
                ),
                (
                    "SwordsmanArchetypeHands",
                    Some(60.0),
                    Some("HeavyHands_Balanced"),
                ),
                ("1hSwordT2_FTUE", Some(36.0), Some("1hSwordT2_FTUE")),
                ("NoWeight", None, None),
            ],
            [
                ("HeavyChest_Balanced", Some(110.555_807_775)),
                ("HeavyHands_Balanced", Some(47.381_060_475)),
                ("1hSwordT2_FTUE", None),
            ],
            None,
        )
    }

    #[test]
    fn an_item_weighs_its_override_else_its_master_weight() {
        let loads = swordsman();
        assert!((loads.weight("SwordsmanArchetypeChest") - 110.555_807_775).abs() < CLOSE);
        assert!((loads.weight("swordsmanarchetypehands") - 47.381_060_475).abs() < CLOSE);
        assert!((loads.weight("1hSwordT2_FTUE") - 36.0).abs() < CLOSE);
        assert!(loads.weight("NoWeight").abs() < CLOSE);
        assert!(loads.weight("Unknown").abs() < CLOSE);
        let load = loads.load(["SwordsmanArchetypeChest", "1hSwordT2_FTUE"]);
        assert!((load - 146.555_807_775).abs() < CLOSE);
    }

    /// The Strength 150 shares (`ATB_Str_Bonus_150_2_Effect`) lighten the classes they name in
    /// the bag, a share of another effect adds to them, and a weight never goes below 0.
    #[test]
    fn a_bag_item_weighs_less_by_the_shares_of_its_classes() {
        let loads = EquipLoads::new(
            [
                ("OreT1", Some(2.0), None),
                ("WoodT1", Some(2.0), None),
                ("NoClass", Some(2.0), None),
            ],
            [],
            None,
        )
        .with_classes([
            ("OreT1", Some("Resource+Ore")),
            ("WoodT1", Some("Resource+Wood")),
            ("NoClass", None),
        ]);
        let strength = [
            ("ore".to_owned(), -0.1),
            ("stone".to_owned(), -0.1),
            ("gem".to_owned(), -0.1),
        ];
        assert!((loads.bag_weight("oret1", &strength) - 1.8).abs() < CLOSE);
        assert!((loads.bag_weight("WoodT1", &strength) - 2.0).abs() < CLOSE);
        assert!((loads.bag_weight("NoClass", &strength) - 2.0).abs() < CLOSE);
        let with_prospector = [("ore".to_owned(), -0.1), ("ore".to_owned(), -0.05)];
        assert!((loads.bag_weight("OreT1", &with_prospector) - 1.7).abs() < CLOSE);
        assert!(loads.bag_weight("OreT1", &[("ore".to_owned(), -2.0)]).abs() < CLOSE);
    }
}
