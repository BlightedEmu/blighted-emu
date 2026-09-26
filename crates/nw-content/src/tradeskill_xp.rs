// SPDX-License-Identifier: AGPL-3.0-or-later
//! How much tradeskill experience a gather or a craft gives, and the bonus items of a craft
//! (craft2 report, run 15).
//!
//! **Gathers (Observed).** A gather's game event gives its `CategoricalProgressionReward` times
//! the `GatherableLevelModifier` of the gathered creature's level in the tradeskill's rank
//! table, rounded down, then times the remote config's multiplier. Only the skinning table has
//! the column; a node (ore, plant, tree) has no level and gives the plain reward. Retail
//! newchar 2026-09-18: boars of level 2 and 3 (`Gather_SkinningT2`, 95) gave 198 = 99 x 2 and
//! 208 = 104 x 2 (modifiers 1.05 and 1.1027); `Playthrough_Part_01` (multiplier 4 there) wolves of
//! level 7, 14 and 15 gave 508, 720 and 756, a level-8 churro (`Gather_SkinningT1`, 32) 180.
//! Herbs, hemp and alchemy motes gave exactly reward x multiplier.
//!
//! **Crafts (Observed).** A craft's game event gives its reward times the sum of the recipe's
//! ingredient quantities, for each craft (`IngotT2`: 11 x 4 ore; `IngotT3`: 46 x 6; `IngotT4`:
//! 222 x 11; `IngotT5`: 924 x 13, over 6,000 retail crafts of `MSQ16-lv62`; nwdb.info's
//! `recipeExp` and `recipeStanding` are the same products). The territory standing of the event
//! counts the same way (retail newchar: two Light Rations, standing 1 x 1 x 2 under multiplier 3
//! = 6).
//!
//! **Bonus items (Inferred, 37 retail refining batches, 7,565 crafts, 1,179 bonus items).** Each
//! craft rolls the recipe's `BonusItemChance` plus the rank's `RollBonus` over 10,000
//! (`IngotT2` at Smelting 60: 0.114 observed, 0.110 predicted; at 103: 0.157 and 0.153). Every
//! item row's `ExtraBonusItemChance` is empty in the data, so it adds nothing.

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The modifier of a level the table does not raise: the plain reward.
pub const NEUTRAL_MODIFIER: f64 = 1.0;
/// `RollBonus` is in hundredths of a percent (Inferred: rank 60's 1100 is a chance of 0.11).
pub const ROLL_BONUS_SCALE: f64 = 10_000.0;
/// The resolution of a bonus roll: a chance is rolled in ten-thousandths.
const CHANCE_STEPS: u32 = 10_000;

/// The rank-table curves of one tradeskill.
#[derive(Debug, Clone, Default, PartialEq)]
struct Curve {
    /// `GatherableLevelModifier` by `Level` (the gathered creature's level).
    level_modifiers: Vec<f64>,
    /// `RollBonus` over [`ROLL_BONUS_SCALE`] by `Level` (the tradeskill's rank).
    roll_bonuses: Vec<f64>,
}

/// One `TradeskillRankData` row as the curves read it.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveRow {
    /// The `CategoricalProgressionId` whose rank table holds the row.
    pub tradeskill: String,
    /// `Level`.
    pub level: i64,
    /// `GatherableLevelModifier`.
    pub level_modifier: Option<f64>,
    /// `RollBonus`.
    pub roll_bonus: Option<i64>,
}

/// The gather level modifiers and craft roll bonuses of every tradeskill.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TradeskillCurves {
    by_id: HashMap<String, Curve>,
}

impl TradeskillCurves {
    /// The curves of the `TradeskillRankData` tables in `tables` that a
    /// `CategoricalProgressionData` row names as its `RankTableId`.
    pub fn from_tables(tables: &Tables) -> Self {
        let owners: HashMap<String, String> = tables
            .categorical_progression_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    row.rank_table_id.as_deref()?.to_lowercase(),
                    row.categorical_progression_id.clone(),
                ))
            })
            .collect();
        Self::build(tables.tradeskill_rank_data.iter().flat_map(|table| {
            let owner = owners.get(&table.unique_id.to_lowercase()).cloned();
            table.rows.iter().filter_map(move |row| {
                Some(CurveRow {
                    tradeskill: owner.clone()?,
                    level: row.level,
                    level_modifier: row.gatherable_level_modifier,
                    roll_bonus: row.roll_bonus,
                })
            })
        }))
    }

    /// The curves of `rows`, in any order; a row of a negative level is left out.
    pub fn build(rows: impl IntoIterator<Item = CurveRow>) -> Self {
        let mut by_id: HashMap<String, Curve> = HashMap::new();
        for row in rows {
            let Ok(level) = usize::try_from(row.level) else {
                continue;
            };
            let curve = by_id.entry(row.tradeskill.to_lowercase()).or_default();
            set_at(
                &mut curve.level_modifiers,
                level,
                row.level_modifier.unwrap_or_default(),
            );
            #[allow(clippy::cast_precision_loss, reason = "roll bonuses of a few thousand")]
            let bonus = row.roll_bonus.unwrap_or_default() as f64 / ROLL_BONUS_SCALE;
            set_at(&mut curve.roll_bonuses, level, bonus);
        }
        Self { by_id }
    }

    /// The `GatherableLevelModifier` of tradeskill `id` (any case) for a gathered creature of
    /// `level`; [`NEUTRAL_MODIFIER`] for a tradeskill or level without a positive one.
    pub fn level_modifier(&self, id: &str, level: u32) -> f64 {
        self.by_id
            .get(&id.to_lowercase())
            .and_then(|curve| curve.level_modifiers.get(usize::try_from(level).ok()?))
            .copied()
            .filter(|modifier| *modifier > 0.0)
            .unwrap_or(NEUTRAL_MODIFIER)
    }

    /// The bonus item chance rank `rank` of tradeskill `id` (any case) adds; 0 for none.
    pub fn roll_bonus(&self, id: &str, rank: u16) -> f64 {
        self.by_id
            .get(&id.to_lowercase())
            .and_then(|curve| curve.roll_bonuses.get(usize::from(rank)))
            .copied()
            .unwrap_or_default()
    }
}

/// Sets `values[index]`, growing the list with zeros.
fn set_at(values: &mut Vec<f64>, index: usize, value: f64) {
    if values.len() <= index {
        values.resize(index + 1, 0.0);
    }
    values[index] = value;
}

/// The `BonusItemChance` of every crafting recipe, by the CRC-32 of its lowercase id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BonusChances {
    by_recipe: HashMap<u32, f64>,
}

impl BonusChances {
    /// The chances of the `CraftingRecipeData` rows in `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .crafting_recipe_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| Some((row.recipe_id.as_str(), row.bonus_item_chance?))),
        )
    }

    /// The chances of `recipes` (recipe id, `BonusItemChance`); the first row of an id wins.
    pub fn build<'a>(recipes: impl IntoIterator<Item = (&'a str, f64)>) -> Self {
        let mut by_recipe = HashMap::new();
        for (id, chance) in recipes {
            by_recipe.entry(lowercase_crc32(id)).or_insert(chance);
        }
        Self { by_recipe }
    }

    /// The `BonusItemChance` of the recipe whose wire id is `crc`; `None` for a recipe without
    /// one (it makes no bonus items).
    pub fn of(&self, crc: u32) -> Option<f64> {
        self.by_recipe.get(&crc).copied()
    }
}

/// A gather event's reward `reward` under level modifier `modifier`: rounded down before the
/// multiplier (Observed: 95 x 1.9955 = 189.6 gave 189 x 4 = 756, not 758).
pub fn gather_reward(reward: f64, modifier: f64) -> f64 {
    (reward * modifier).floor()
}

/// How many of `crafts` crafts, each rolling `chance` with `below(n)` a random number below `n`,
/// make a bonus item.
pub fn bonus_crafts(chance: f64, crafts: u32, below: &mut impl FnMut(u32) -> u32) -> u32 {
    let steps = (chance.clamp(0.0, 1.0) * f64::from(CHANCE_STEPS)).round();
    // In 0..=CHANCE_STEPS: clamped above.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped"
    )]
    let steps = steps as u32;
    (0..crafts)
        .filter(|_| below(CHANCE_STEPS) < steps)
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail skinning `GatherableLevelModifier` of levels 0 to 3 and 14, 15.
    fn skinning() -> TradeskillCurves {
        let row = |level, level_modifier, roll_bonus| CurveRow {
            tradeskill: "Skinning".to_owned(),
            level,
            level_modifier: Some(level_modifier),
            roll_bonus: Some(roll_bonus),
        };
        TradeskillCurves::build([
            row(3, 1.102_657_5, 30),
            row(0, 0.0, 0),
            row(1, 1.0, 10),
            row(2, 1.05, 20),
            row(14, 1.899_077_253_842_609, 140),
            row(15, 1.995_522_321_081_751_4, 150),
        ])
    }

    #[test]
    fn a_skin_gives_the_reward_of_the_creature_level() {
        let curves = skinning();
        // Retail newchar: boars of level 2 and 3, `Gather_SkinningT2` 95, multiplier 2.
        let boar = |level| gather_reward(95.0, curves.level_modifier("skinning", level)) * 2.0;
        assert_eq!(boar(2), 198.0);
        assert_eq!(boar(3), 208.0);
        // Playthrough_Part_01 (multiplier 4): wolves of level 14 and 15.
        let wolf = |level| gather_reward(95.0, curves.level_modifier("Skinning", level)) * 4.0;
        assert_eq!(wolf(14), 720.0);
        assert_eq!(wolf(15), 756.0);
        // Level 0's modifier is 0 in the data, an unknown level or tradeskill has none.
        assert_eq!(curves.level_modifier("Skinning", 0), NEUTRAL_MODIFIER);
        assert_eq!(curves.level_modifier("Skinning", 99), NEUTRAL_MODIFIER);
        assert_eq!(curves.level_modifier("Mining", 3), NEUTRAL_MODIFIER);
    }

    #[test]
    fn a_rank_adds_its_roll_bonus() {
        let curves = skinning();
        assert!((curves.roll_bonus("Skinning", 15) - 0.015).abs() < f64::EPSILON);
        assert_eq!(curves.roll_bonus("Skinning", 200), 0.0);
        assert_eq!(curves.roll_bonus("Mining", 1), 0.0);
    }

    #[test]
    fn a_recipe_keeps_its_bonus_chance() {
        let chances = BonusChances::build([("IngotT3", -0.02), ("IngotT3", 0.5)]);
        assert_eq!(chances.of(lowercase_crc32("ingott3")), Some(-0.02));
        assert_eq!(chances.of(lowercase_crc32("other")), None);
    }

    #[test]
    fn each_craft_rolls_its_bonus() {
        // A roll of 0 is below any positive chance, the top roll below none but a sure one.
        assert_eq!(bonus_crafts(0.11, 5, &mut |_| 0), 5);
        assert_eq!(bonus_crafts(0.11, 5, &mut |below| below - 1), 0);
        assert_eq!(bonus_crafts(1.5, 3, &mut |below| below - 1), 3);
        assert_eq!(bonus_crafts(-0.02, 3, &mut |_| 0), 0);
        // 1,100 of 10,000 steps: rolls 0..1,099 win.
        assert_eq!(bonus_crafts(0.11, 1, &mut |_| 1_099), 1);
        assert_eq!(bonus_crafts(0.11, 1, &mut |_| 1_100), 0);
    }
}
