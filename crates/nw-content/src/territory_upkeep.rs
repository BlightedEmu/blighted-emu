// SPDX-License-Identifier: AGPL-3.0-or-later
//! The upkeep cost of a territory claim: the `UpkeepCost` of its level
//! (`javelindata_territorygovernance`, `TerritoryUpkeepDefinition`) times the territory's
//! `UpkeepModifier` (`TerritoryDefinition`).
//!
//! Observed (build 25810808 tables, territory ownership study 2026-11-11): `UpkeepCost`
//! 1,000,000 at every level 0 to 45, `UpkeepModifier` 0.1 on the 13 claims; the one retail
//! payment (2738 `pay-territory-tax` 1783335287110) asked 100,000 for territory 6 at level 31.

use std::collections::{BTreeMap, HashMap};

use crate::Tables;

/// What the upkeep of every claim costs, by level and territory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpkeepCosts {
    /// `UpkeepCost` by `Level`, hundredths of a coin.
    by_level: BTreeMap<u32, u64>,
    /// `UpkeepModifier` by `TerritoryID`.
    modifiers: HashMap<u16, f64>,
}

impl UpkeepCosts {
    /// The upkeep rows and territory modifiers of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .territory_upkeep_definition
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    Some((
                        u32::try_from(row.level).ok()?,
                        u64::try_from(row.upkeep_cost).ok()?,
                    ))
                }),
            tables
                .territory_definition
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    Some((u16::try_from(row.territory_id).ok()?, row.upkeep_modifier?))
                }),
        )
    }

    /// The costs of `levels` (level, `UpkeepCost`) and `modifiers` (territory, `UpkeepModifier`);
    /// the first row of a level or territory wins.
    pub fn build(
        levels: impl IntoIterator<Item = (u32, u64)>,
        modifiers: impl IntoIterator<Item = (u16, f64)>,
    ) -> Self {
        let mut costs = Self::default();
        for (level, cost) in levels {
            costs.by_level.entry(level).or_insert(cost);
        }
        for (territory, modifier) in modifiers {
            costs.modifiers.entry(territory).or_insert(modifier);
        }
        costs
    }

    /// The upkeep of `territory` at `level`, hundredths of a coin, rounded to the nearest coin
    /// hundredth (Inferred: 1,000,000 x 0.1 is 100,000 in `f64` only after rounding); `None` for a
    /// level or territory without a row.
    pub fn cost(&self, territory: u16, level: u32) -> Option<u64> {
        let cost = *self.by_level.get(&level)?;
        let modifier = *self.modifiers.get(&territory)?;
        // A cost of u64 times a modifier near 1 fits a u64; a negative product is no cost.
        Some((cost as f64 * modifier).round().max(0.0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build 25810808 values: the `UpkeepCost` of every level and the claims' modifier.
    const UPKEEP_COST: u64 = 1_000_000;
    const MODIFIER: f64 = 0.1;
    const TERRITORY: u16 = 6;
    const LEVEL: u32 = 31;
    /// The cost of the one retail payment.
    const RETAIL_COST: u64 = 100_000;

    #[test]
    fn the_cost_is_the_level_cost_times_the_territory_modifier() {
        let costs = UpkeepCosts::build(
            [(0, UPKEEP_COST), (LEVEL, UPKEEP_COST), (LEVEL, 1)],
            [(TERRITORY, MODIFIER), (TERRITORY, 1.0)],
        );
        assert_eq!(costs.cost(TERRITORY, LEVEL), Some(RETAIL_COST));
        assert_eq!(costs.cost(TERRITORY, 0), Some(RETAIL_COST));
        assert_eq!(costs.cost(TERRITORY + 1, LEVEL), None, "no territory row");
        assert_eq!(costs.cost(TERRITORY, LEVEL + 1), None, "no level row");
    }
}
