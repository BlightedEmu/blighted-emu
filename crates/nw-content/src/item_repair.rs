// SPDX-License-Identifier: AGPL-3.0-or-later
//! Item durability, repair and gear score upgrade rules (run-15 items2 study).
//!
//! **Values (Observed, `sharedassets/genericassets/playerbaseattributes.pbadb`, fields keyed by
//! the CRC-32 of their lowercase display name, all 119 fields of the client list
//! `FUN_145cae240` matched):** "Durability To Coin Rate" 0.01, "Durability Repair Cost Data"
//! (tier, value) pairs (1,0) (2,1) (3,2) (4,2) (5,3), "Repair Resource Modifier" 0.1, "Repair
//! Max Durability Cost" 0 (a repair never lowers the full durability), "Base Gather Durability
//! Cost" 1.0, "Pvp Paperdoll Durability Loss Multiplier" 0.4.
//!
//! **Client check (Observed, `FUN_142eb1190`):** an item of tier 1 or more needs the repair
//! parts of a bus call `(item, 1)` from its `Repair_T1` points and the coins of a bus call
//! `(tier, MaxDurability - Durability)`; an item below tier 1 is repaired for nothing; a repair
//! kit `RepairKitT{tier}` repairs it instead (`FUN_142eb17a0`). The two bus handlers are
//! virtual calls not resolved in the decompile, so the formulas of [`repair_cost`] are a **best
//! guess** from the values above.
//!
//! **Upgrade (Observed, 9 of 9 retail upgrades in 5 ledgers):** raising an item's gear score
//! from `from` to `to` costs the sum of `GearScoreUpgradeDefinition.RequiredCurrencyQuantity`
//! over the levels `from` to `to - 1` (740 to 790 cost 289, 700 to 793 cost 571, 795 to 800
//! cost 279).

use std::collections::BTreeMap;

use crate::Tables;
use crate::item_rolls::ItemRollRow;

/// "Durability To Coin Rate": wire coins (the wallet's hundredths) per point of durability,
/// before the tier's value (best guess that the rate is in wire coins: 0.75 coin for a broken
/// T5 weapon of 2500).
pub const DURABILITY_TO_COIN_RATE: f64 = 0.01;
/// The steps a formula result is rounded to before it is rounded up, so a product such as
/// `300 × 0.4` that floating point puts a hair above a whole number stays that number.
const CLEAN_STEPS: f64 = 1_000_000.0;
/// "Durability Repair Cost Data": the coin value of each tier.
const REPAIR_COST_BY_TIER: [(u32, u32); 5] = [(1, 0), (2, 1), (3, 2), (4, 2), (5, 3)];
/// "Repair Resource Modifier": repair parts per point of durability, before the item's
/// `RepairDustModifier`.
pub const REPAIR_RESOURCE_MODIFIER: f64 = 0.1;
/// The share of the full durability the parts formula counts in (best guess: per percent).
const DURABILITY_PERCENT: f64 = 100.0;
/// The `RepairDustModifier` of an item without one.
const PLAIN_DUST_MODIFIER: f64 = 1.0;
/// The categorical progression that holds the repair parts (rank table `Repair_T1`,
/// `MaximumInfluence` 2000; only T1 has a progression row).
pub const REPAIR_PARTS_PROGRESSION: &str = "Repair_T1";
/// The lowest tier whose repair costs anything (`FUN_142eb1190`: below it the repair is free).
const FIRST_PAID_TIER: u32 = 1;
/// The item id of the repair kit of a tier: this and the tier (`RepairKitT%d`).
pub const REPAIR_KIT_PREFIX: &str = "RepairKitT";
/// "Base Gather Durability Cost": what a tool loses per completed gather.
pub const GATHER_DURABILITY_COST: u32 = 1;
/// What a weapon loses per hit that takes durability (best guess: `DurabilityCostOverride` is
/// empty or 0 in all 4,938 `DamageData` rows; the gather base cost).
pub const HIT_DURABILITY_COST: u32 = 1;
/// "Pvp Paperdoll Durability Loss Multiplier": a PvP death takes this share of the PvE loss.
pub const PVP_DEATH_LOSS_MULTIPLIER: f64 = 0.4;

/// What a repair costs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RepairCost {
    /// Wire coins (hundredths).
    pub coins: u64,
    /// `Repair_T1` points.
    pub parts: u32,
}

/// The repair kit item id of `tier`.
pub fn repair_kit(tier: u32) -> String {
    format!("{REPAIR_KIT_PREFIX}{tier}")
}

/// `value` rounded up to a whole number after [`CLEAN_STEPS`] rounding.
fn ceil_clean(value: f64) -> f64 {
    ((value * CLEAN_STEPS).round() / CLEAN_STEPS).ceil()
}

/// What repairing `item` from `durability` to full costs (module notes; best guess):
/// coins `ceil(missing × rate × tier value)` in wire hundredths, parts
/// `ceil(missing × 0.1 × RepairDustModifier ÷ (full ÷ 100))`; nothing below tier 1.
pub fn repair_cost(item: &ItemRollRow, durability: u32) -> RepairCost {
    let missing = item.durability.saturating_sub(durability);
    if item.tier < FIRST_PAID_TIER || missing == 0 || item.durability == 0 {
        return RepairCost::default();
    }
    let tier_value = REPAIR_COST_BY_TIER
        .iter()
        .find(|(tier, _)| *tier == item.tier)
        .or_else(|| REPAIR_COST_BY_TIER.last())
        .map_or(0, |(_, value)| *value);
    let missing = f64::from(missing);
    let coins = ceil_clean(missing * DURABILITY_TO_COIN_RATE * f64::from(tier_value));
    let dust = item.repair_dust_modifier.unwrap_or(PLAIN_DUST_MODIFIER);
    let parts = ceil_clean(
        missing * REPAIR_RESOURCE_MODIFIER * dust
            / (f64::from(item.durability) / DURABILITY_PERCENT),
    );
    RepairCost {
        // Whole, non-negative and far below 2^53.
        coins: coins.max(0.0) as u64,
        parts: parts.max(0.0) as u32,
    }
}

/// What a death takes from a worn `item` (`ceil(full × DurabilityDmgOnDeath)`, times
/// [`PVP_DEATH_LOSS_MULTIPLIER`] for a PvP death; the rounding is a best guess).
pub fn death_loss(item: &ItemRollRow, pvp: bool) -> u32 {
    let multiplier = if pvp { PVP_DEATH_LOSS_MULTIPLIER } else { 1.0 };
    let loss = ceil_clean(f64::from(item.durability) * item.durability_loss_on_death * multiplier);
    // At most the full durability, a u32.
    loss.clamp(0.0, f64::from(item.durability)) as u32
}

/// The gear score upgrade levels (`GearScoreUpgradeDefinition`): the currency and quantity of
/// each level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GearScoreUpgrades {
    levels: BTreeMap<u32, (String, u64)>,
}

/// Why an upgrade is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeRefusal {
    /// `to` is not above `from`, or a level of the range has no row.
    NoSuchLevel,
    /// The levels of the range ask for different currencies.
    MixedCurrency,
}

impl GearScoreUpgrades {
    /// The upgrade levels of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .gear_score_upgrade_definition
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    Some((
                        u32::try_from(row.level).ok()?,
                        row.required_currency_id.clone(),
                        u64::try_from(row.required_currency_quantity).ok()?,
                    ))
                }),
        )
    }

    /// The levels of `rows`: (level, currency id, quantity).
    pub fn build(rows: impl IntoIterator<Item = (u32, String, u64)>) -> Self {
        Self {
            levels: rows
                .into_iter()
                .map(|(level, currency, quantity)| (level, (currency, quantity)))
                .collect(),
        }
    }

    /// The currency and quantity of raising a gear score from `from` to `to`: the sum over the
    /// levels `from` to `to - 1`, each of which must have a row (so `from` is at least the
    /// first level and `to` at most one past the last).
    pub fn cost(&self, from: u32, to: u32) -> Result<(String, u64), UpgradeRefusal> {
        if to <= from {
            return Err(UpgradeRefusal::NoSuchLevel);
        }
        let mut currency: Option<&str> = None;
        let mut total: u64 = 0;
        for level in from..to {
            let (id, quantity) = self
                .levels
                .get(&level)
                .ok_or(UpgradeRefusal::NoSuchLevel)?;
            match currency {
                Some(known) if !known.eq_ignore_ascii_case(id) => {
                    return Err(UpgradeRefusal::MixedCurrency);
                }
                _ => currency = Some(id),
            }
            total = total.saturating_add(*quantity);
        }
        currency
            .map(|id| (id.to_owned(), total))
            .ok_or(UpgradeRefusal::NoSuchLevel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UMBRAL: &str = "UmbralCurrency";
    /// The retail quantities of levels 785 to 799.
    const TOP_QUANTITIES: [u64; 15] = [7, 10, 12, 15, 20, 24, 27, 31, 36, 40, 44, 49, 54, 62, 70];
    const FIRST_LEVEL: u32 = 700;
    const FIRST_TOP_LEVEL: u32 = 785;
    const PLAIN_QUANTITY: u64 = 5;

    fn upgrades() -> GearScoreUpgrades {
        GearScoreUpgrades::build((FIRST_LEVEL..FIRST_TOP_LEVEL + 15).map(|level| {
            let quantity = level
                .checked_sub(FIRST_TOP_LEVEL)
                .map_or(PLAIN_QUANTITY, |at| TOP_QUANTITIES[at as usize]);
            (level, UMBRAL.to_owned(), quantity)
        }))
    }

    fn item(tier: u32, durability: u32) -> ItemRollRow {
        ItemRollRow {
            tier,
            durability,
            durability_loss_on_death: 0.12,
            repair_dust_modifier: Some(0.25),
            ..ItemRollRow::default()
        }
    }

    #[test]
    fn upgrades_cost_the_retail_amounts() {
        let upgrades = upgrades();
        assert_eq!(upgrades.cost(740, 790), Ok((UMBRAL.to_owned(), 289)));
        assert_eq!(upgrades.cost(700, 793), Ok((UMBRAL.to_owned(), 571)));
        assert_eq!(upgrades.cost(795, 800), Ok((UMBRAL.to_owned(), 279)));
        assert_eq!(upgrades.cost(699, 701), Err(UpgradeRefusal::NoSuchLevel));
        assert_eq!(upgrades.cost(799, 801), Err(UpgradeRefusal::NoSuchLevel));
        assert_eq!(upgrades.cost(750, 750), Err(UpgradeRefusal::NoSuchLevel));
    }

    #[test]
    fn a_repair_costs_coins_and_parts_by_tier_and_nothing_below_tier_one() {
        let cost = repair_cost(&item(5, 2_500), 0);
        assert_eq!(cost.coins, 75);
        assert_eq!(cost.parts, 3);
        assert_eq!(repair_cost(&item(1, 2_500), 0).coins, 0);
        assert_eq!(repair_cost(&item(0, 2_500), 0), RepairCost::default());
        assert_eq!(repair_cost(&item(5, 2_500), 2_500), RepairCost::default());
        assert_eq!(repair_kit(4), "RepairKitT4");
    }

    #[test]
    fn a_death_takes_its_share_and_less_in_pvp() {
        assert_eq!(death_loss(&item(5, 2_500), false), 300);
        assert_eq!(death_loss(&item(5, 2_500), true), 120);
        let unbreakable = ItemRollRow {
            durability_loss_on_death: 0.0,
            ..item(5, 2_500)
        };
        assert_eq!(death_loss(&unbreakable, false), 0);
    }
}
