// SPDX-License-Identifier: AGPL-3.0-or-later
//! The weight of each item for the equip load and the player's equip load limits (dodge study
//! 2026-09-20). An item weighs its stats row's `WeightOverride` (`ArmorItemDefinitions` or
//! `WeaponItemDefinitions`, by the master row's `ItemStatsRef`) where that row has one, else its
//! master row's `Weight`. Retail's new character dodged with its open-world kit, whose master
//! weights (555, over the 500 maximum) would make it overburdened, unable to dodge, and whose
//! armor overrides give 470.9: Inferred, the overrides count. The limits are the
//! `EncumbranceData` row of the player container.

use std::collections::HashMap;

use crate::Tables;

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
}

/// The item weights and the limits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EquipLoads {
    /// The weight of each item, by its lowercase id.
    weights: HashMap<String, f64>,
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
        Self { weights, limits }
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
            });
        Self::new(items, armor.chain(weapons), limits)
    }

    /// The weight of item `item_id` (any case); 0 for an item the tables do not weigh.
    pub fn weight(&self, item_id: &str) -> f64 {
        self.weights
            .get(&item_id.to_lowercase())
            .copied()
            .unwrap_or_default()
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
}
