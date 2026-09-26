// SPDX-License-Identifier: AGPL-3.0-or-later
//! The gear score of a dropped item (run-15 items2 study, R3). The loot tables' columns are
//! Observed (`LootTablesData`, read by `FUN_143685650`); the server roll is not in the client, so
//! the rule is a best guess built from them:
//!
//! - the range is the dropping entry's `GearScoreRange{n}`; else, for a table with
//!   `UseLevelGS`, the player level's `GSLimitT{tier}` (`ExperienceData`, `XPLevels`); else the
//!   item's `MinGearScore..MaxGearScore`, the top held to the killed mob level's
//!   `AILootGSCeiling` (`VitalsLevelData`);
//! - the gear score is drawn evenly in the range, then the table's `GSBonus` is added, and for
//!   a mob drop its level's `LootGSBonus` at `LootGSBonusChance`;
//! - the item roll ([`crate::item_rolls::ItemRolls::roll`]) holds it to the item's range.

use crate::Tables;
use crate::item_rolls::ItemRollRow;
use crate::loot::DropGearScore;
use crate::perks::chance_hit;
use crate::progression::LEVEL_TABLE;

/// Separates the ends of a `GSLimitT{n}` cell (`160-200`).
const RANGE_SEPARATOR: char = '-';
/// Tiers with a `GSLimitT{n}` column.
const LIMIT_TIERS: usize = 5;

/// What a mob level adds to its drops (`VitalsLevelData`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MobLootGearScore {
    /// `Level`.
    pub level: u32,
    /// `AILootGSCeiling`: the highest gear score its drops reach.
    pub ceiling: u32,
    /// `LootGSBonus`.
    pub bonus: u32,
    /// `LootGSBonusChance`.
    pub bonus_chance: f64,
}

/// The gear score rules of drops: each player level's limits by tier and each mob level's
/// ceiling and bonus.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DropRolls {
    /// The `GSLimitT1..5` ranges of each zero-based player level.
    level_limits: Vec<[Option<(u32, u32)>; LIMIT_TIERS]>,
    /// The mob levels, by level.
    mobs: Vec<MobLootGearScore>,
}

/// A `GSLimitT{n}` cell as its range.
fn limit(cell: &str) -> Option<(u32, u32)> {
    let (low, high) = cell.split_once(RANGE_SEPARATOR)?;
    Some((low.trim().parse().ok()?, high.trim().parse().ok()?))
}

/// A non-negative number of a cell.
fn number(value: i64) -> u32 {
    u32::try_from(value).unwrap_or_default()
}

impl DropRolls {
    /// The rules of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let levels = tables
            .experience_data
            .iter()
            .filter(|table| table.unique_id.eq_ignore_ascii_case(LEVEL_TABLE))
            .flat_map(|table| &table.rows)
            .map(|row| {
                (
                    row.level_number,
                    [
                        limit(&row.gs_limit_t1),
                        limit(&row.gs_limit_t2),
                        limit(&row.gs_limit_t3),
                        limit(&row.gs_limit_t4),
                        limit(&row.gs_limit_t5),
                    ],
                )
            });
        let mobs = tables
            .vitals_level_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| MobLootGearScore {
                level: number(row.level),
                ceiling: number(row.ai_loot_gs_ceiling),
                bonus: number(row.loot_gs_bonus),
                bonus_chance: row.loot_gs_bonus_chance,
            });
        Self::build(levels, mobs)
    }

    /// The rules of the player `levels` (level number, limits by tier; sorted here, the first
    /// is zero-based level 0) and the `mobs` levels.
    pub fn build(
        levels: impl IntoIterator<Item = (i64, [Option<(u32, u32)>; LIMIT_TIERS])>,
        mobs: impl IntoIterator<Item = MobLootGearScore>,
    ) -> Self {
        let mut levels: Vec<_> = levels.into_iter().collect();
        levels.sort_by_key(|(level, _)| *level);
        let mut mobs: Vec<_> = mobs.into_iter().collect();
        mobs.sort_by_key(|mob| mob.level);
        Self {
            level_limits: levels.into_iter().map(|(_, limits)| limits).collect(),
            mobs,
        }
    }

    /// The row of mob level `level`: the highest row at most it.
    fn mob(&self, level: u32) -> Option<&MobLootGearScore> {
        self.mobs.iter().rev().find(|mob| mob.level <= level)
    }

    /// The range a drop of `item` with `hint` draws from, for a player of zero-based level
    /// `player_level` and a mob of level `mob_level` (module notes).
    pub fn range(
        &self,
        hint: &DropGearScore,
        item: &ItemRollRow,
        player_level: Option<u32>,
        mob_level: Option<u32>,
    ) -> (u32, u32) {
        if let Some(range) = hint.range {
            return range;
        }
        let level_limit = player_level
            .filter(|_| hint.use_level)
            .and_then(|level| self.level_limits.get(usize::try_from(level).ok()?))
            .and_then(|limits| {
                let tier = usize::try_from(item.tier).ok()?.checked_sub(1)?;
                *limits.get(tier)?
            });
        if let Some(range) = level_limit {
            return range;
        }
        let top = item.max_gear_score.max(item.min_gear_score);
        let ceiling = mob_level
            .and_then(|level| self.mob(level))
            .map_or(top, |mob| mob.ceiling);
        (
            item.min_gear_score,
            top.min(ceiling).max(item.min_gear_score),
        )
    }

    /// The gear score of a drop of `item` with `hint` (module notes); `below(n)` is a random
    /// number below `n`.
    pub fn gear_score(
        &self,
        hint: &DropGearScore,
        item: &ItemRollRow,
        player_level: Option<u32>,
        mob_level: Option<u32>,
        below: &mut impl FnMut(u32) -> u32,
    ) -> u32 {
        let (low, high) = self.range(hint, item, player_level, mob_level);
        let (low, high) = (low.min(high), high.max(low));
        let drawn = low.saturating_add(below(high - low + 1));
        let mob_bonus = mob_level
            .and_then(|level| self.mob(level))
            .filter(|mob| mob.bonus > 0 && chance_hit(mob.bonus_chance, below))
            .map_or(0, |mob| mob.bonus);
        drawn
            .saturating_add_signed(hint.bonus)
            .saturating_add(mob_bonus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sword() -> ItemRollRow {
        ItemRollRow {
            item_id: "1hSwordT5".to_owned(),
            tier: 5,
            min_gear_score: 500,
            max_gear_score: 625,
            ..ItemRollRow::default()
        }
    }

    fn rolls() -> DropRolls {
        let mut limits = [None; LIMIT_TIERS];
        limits[4] = limit("626 - 695");
        DropRolls::build(
            [(2, [None; LIMIT_TIERS]), (1, limits)],
            [
                MobLootGearScore {
                    level: 77,
                    ceiling: 730,
                    bonus: 20,
                    bonus_chance: 0.2,
                },
                MobLootGearScore {
                    level: 1,
                    ceiling: 100,
                    ..MobLootGearScore::default()
                },
            ],
        )
    }

    #[test]
    fn the_range_comes_from_the_entry_the_level_or_the_mob_ceiling() {
        let rolls = rolls();
        let entry = DropGearScore {
            range: Some((650, 700)),
            ..DropGearScore::default()
        };
        assert_eq!(rolls.range(&entry, &sword(), Some(0), Some(80)), (650, 700));
        let level = DropGearScore {
            use_level: true,
            ..DropGearScore::default()
        };
        // Level number 1 sorts first: zero-based level 0.
        assert_eq!(rolls.range(&level, &sword(), Some(0), None), (626, 695));
        assert_eq!(
            rolls.range(&level, &sword(), Some(1), Some(3)),
            (500, 500),
            "no limit at that level: the item's range under the mob's ceiling"
        );
        let plain = DropGearScore::default();
        assert_eq!(rolls.range(&plain, &sword(), None, Some(80)), (500, 625));
    }

    #[test]
    fn a_drop_adds_the_table_and_mob_bonuses() {
        let rolls = rolls();
        let hint = DropGearScore {
            range: Some((600, 610)),
            bonus: -10,
            ..DropGearScore::default()
        };
        // The lowest draw, the mob bonus hit (0 is below every chance above 0).
        assert_eq!(
            rolls.gear_score(&hint, &sword(), None, Some(80), &mut |_: u32| 0_u32),
            610
        );
        assert_eq!(
            rolls.gear_score(&hint, &sword(), None, None, &mut |n: u32| n - 1),
            600
        );
    }
}
