// SPDX-License-Identifier: AGPL-3.0-or-later
//! The character level, the weapon masteries and the tradeskills of the game data
//! (docs/re/2026-09-18-progression-and-gm-mode.md sections 2 and 3).
//!
//! The level comes from the total experience: the highest level whose `XPToLevel` (the total at
//! which the level starts) is at most the experience. A weapon mastery (a
//! `CategoricalProgressionData` row on the `WeaponMastery` rank table) has a rank and the points
//! inside it; this module keeps the total experience of a mastery and splits it into the two.
//!
//! A tradeskill is a `CategoricalProgressionData` row whose `RankTableId` names a
//! `TradeskillRankData` table (`javelindata_tradeskill<name>`, unique id `Mining`, `Cooking`,
//! ...). Retail lists the tradeskills in the same type-911 list as the weapon masteries (newchar
//! 2026-09-18 login: Cooking, Leatherworking, Fishing; the first skinning added `Skinning` at the
//! list's end), so a tradeskill is a [`Mastery`] of kind [`MasteryKind::Tradeskill`]. Its rank
//! caps are the table's `MaximumInfluence` (Observed: the retail skinning ranks 1 to 5 follow
//! the caps 90, 110, 120, 130, 140), and each rank carries the rank's gathering efficiency and
//! recipe level ([`RankPerks`]).

use std::collections::{BTreeMap, HashMap};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;
use crate::character_points::CharacterPoints;

/// The level table (`javelindata_xpamountsbylevel`).
pub const LEVEL_TABLE: &str = "XPLevels";
/// The rank table of the weapon masteries (`javelindata_weaponmastery`).
pub const WEAPON_MASTERY_RANK_TABLE: &str = "WeaponMastery";

/// What a mastery measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasteryKind {
    /// A weapon's mastery: each rank gives a point of the weapon's pool.
    Weapon,
    /// A tradeskill: its ranks give gathering efficiency and recipe levels, no points.
    Tradeskill,
    /// Another progression whose rank rises with its experience (`AutoRankUp` true; territory
    /// standing, camping).
    Ranked,
    /// A currency (`AutoRankUp` false: `Azoth_Currency`, the faction tokens): its points are
    /// the balance, at most its rank 0 `MaximumInfluence` (Observed, content bundle 25810808:
    /// 2,500 for `AzothCurrency`), and no gain ranks it up.
    Currency,
}

/// The categorical progression of the azoth balance (`CategoricalProgressionData`, rank table
/// `AzothCurrency`; retail newchar 2026-09-18 added it to the type-911 list with rank 0 and 0
/// points). A weapon tree respec costs `AzothRespecCost` of it.
pub const AZOTH_PROGRESSION: &str = "Azoth_Currency";

/// What one rank of a tradeskill gives (`TradeskillRankData`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RankPerks {
    /// `GatheringEfficiency`: the gathering efficiency the rank adds (0 for a crafting skill).
    pub gathering_efficiency: f64,
    /// `RecipeLevel`: the highest `RecipeLevel` of the recipes the rank may craft.
    pub recipe_level: u32,
    /// The (lowest, highest) gear score bonus a craft of each recipe tier gets at this rank,
    /// tier 1 first (`MinGearScoreBonus`/`MaxGearScoreBonus` for tier 1, the `T2`..`T5` columns
    /// for tiers 2 to 5; the loader `FUN_143ca9210` holds the lowest at most the highest).
    pub gear_score_bonus: [(u32, u32); CRAFT_TIERS],
}

/// Recipe tiers with a rank gear score bonus (1 to 5).
pub const CRAFT_TIERS: usize = 5;

impl RankPerks {
    /// The (lowest, highest) gear score bonus of a craft of recipe tier `tier` (1 to 5); none
    /// for another tier.
    pub fn gear_score_bonus_of(&self, tier: u32) -> (u32, u32) {
        usize::try_from(tier)
            .ok()
            .and_then(|tier| tier.checked_sub(1))
            .and_then(|index| self.gear_score_bonus.get(index))
            .copied()
            .unwrap_or_default()
    }
}

/// The rank gear score bonuses of a `TradeskillRankData` row's (lowest, highest) cells by tier,
/// each lowest held at most its highest.
fn rank_gear_score_bonus(
    cells: [(Option<i64>, Option<i64>); CRAFT_TIERS],
) -> [(u32, u32); CRAFT_TIERS] {
    let value = |cell: Option<i64>| {
        cell.and_then(|value| u32::try_from(value).ok())
            .unwrap_or_default()
    };
    cells.map(|(low, high)| {
        let high = value(high);
        (value(low).min(high), high)
    })
}

/// One weapon mastery or tradeskill.
#[derive(Debug, Clone, PartialEq)]
pub struct Mastery {
    /// `CategoricalProgressionId`, for example `GreatSwordAbilityTable` or `Mining`.
    pub id: String,
    /// The CRC-32 of the lowercase id: its id on the wire (type 911).
    pub crc: u32,
    /// The experience of each rank before the next one (`MaximumInfluence`), rank 0 first.
    rank_caps: Vec<u32>,
    /// The top rank, zero-based (`MaxLevel`; for a tradeskill the highest of `MaxLevel`,
    /// `Expansion01MaxLevel` and `Expansion02MaxLevel`).
    pub max_rank: u16,
    /// What it measures.
    pub kind: MasteryKind,
    /// What each rank gives, rank 0 first; empty for a weapon mastery.
    perks: Vec<RankPerks>,
}

impl Mastery {
    /// The rank (zero-based) and the points inside it of a total of `experience`. At the top
    /// rank the points stay 0, as retail sends them.
    pub fn rank_of(&self, experience: u32) -> (u16, u64) {
        if self.kind == MasteryKind::Currency {
            return (0, u64::from(experience.min(self.cap(0))));
        }
        let mut left = experience;
        for rank in 0..self.max_rank {
            let cap = self.cap(rank);
            if left < cap {
                return (rank, u64::from(left));
            }
            left -= cap;
        }
        (self.max_rank, 0)
    }

    /// The total experience at which `rank` starts; the top rank's for a higher one.
    pub fn experience_for_rank(&self, rank: u16) -> u32 {
        (0..rank.min(self.max_rank))
            .map(|below| self.cap(below))
            .fold(0, u32::saturating_add)
    }

    /// The total experience at the top rank: more gives nothing. A currency's is its cap.
    pub fn max_experience(&self) -> u32 {
        if self.kind == MasteryKind::Currency {
            return self.cap(0);
        }
        self.experience_for_rank(self.max_rank)
    }

    fn cap(&self, rank: u16) -> u32 {
        self.rank_caps
            .get(usize::from(rank))
            .copied()
            .unwrap_or(u32::MAX)
    }

    /// What the rank of a total of `experience` gives; the default for a mastery without
    /// perks.
    pub fn perks(&self, experience: u32) -> RankPerks {
        let (rank, _) = self.rank_of(experience);
        self.perks
            .get(usize::from(rank))
            .copied()
            .unwrap_or_default()
    }
}

/// One `TradeskillRankData` row: its rank, cap and perks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TradeskillRankRow {
    /// `Level`.
    pub rank: i64,
    /// `MaximumInfluence`.
    pub cap: f64,
    /// `GatheringEfficiency` and `RecipeLevel`.
    pub perks: RankPerks,
}

/// A tradeskill as the tables give it: its id, its top ranks (`MaxLevel`,
/// `Expansion01MaxLevel`, `Expansion02MaxLevel`) and its rank table's rows.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeskillSource {
    /// `CategoricalProgressionId`.
    pub id: String,
    /// The top rank columns that are set.
    pub max_levels: Vec<i64>,
    /// Its rank table's rows, in any order.
    pub ranks: Vec<TradeskillRankRow>,
}

/// Another categorical progression as the tables give it: a `CategoricalProgressionData` row
/// whose `RankTableId` names a `CategoricalProgressionRankData` table other than the weapon
/// masteries' ([`MasteryKind::Ranked`] or [`MasteryKind::Currency`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressionSource {
    /// `CategoricalProgressionId`.
    pub id: String,
    /// `MaxLevel`.
    pub max_level: i64,
    /// `AutoRankUp`: false makes it a currency.
    pub auto_rank_up: bool,
    /// Its rank table's rows (`Rank`, `MaximumInfluence`), in any order.
    pub ranks: Vec<(i64, i64)>,
}

/// The level table, the weapon masteries and the tradeskills.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progression {
    /// The total experience at which each level starts, level 0 first.
    level_starts: Vec<u32>,
    /// The weapon masteries, in table order.
    masteries: Vec<Mastery>,
    /// The tradeskills, in table order.
    tradeskills: Vec<Mastery>,
    /// The other ranked progressions and the currencies, in table order.
    others: Vec<Mastery>,
    /// The azoth a weapon tree respec costs at each weapon rank (`AzothRespecCost` of the
    /// `WeaponMastery` rank table), by rank.
    respec_costs: BTreeMap<u16, u32>,
    /// The rules of the attribute and weapon points.
    pub points: CharacterPoints,
}

impl Progression {
    /// The level table, the weapon masteries and the tradeskills of `tables`; empty when a
    /// table is missing.
    pub fn of_tables(tables: &Tables) -> Self {
        let levels = tables
            .experience_data
            .iter()
            .filter(|table| table.unique_id.eq_ignore_ascii_case(LEVEL_TABLE))
            .flat_map(|table| &table.rows)
            .map(|row| (row.level_number, row.xp_to_level));
        let ranks = tables
            .categorical_progression_rank_data
            .iter()
            .filter(|table| {
                table
                    .unique_id
                    .eq_ignore_ascii_case(WEAPON_MASTERY_RANK_TABLE)
            })
            .flat_map(|table| &table.rows)
            .map(|row| (row.rank, row.maximum_influence));
        let masteries = tables
            .categorical_progression_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter(|row| {
                row.rank_table_id
                    .as_deref()
                    .is_some_and(|table| table.eq_ignore_ascii_case(WEAPON_MASTERY_RANK_TABLE))
            })
            .map(|row| (row.categorical_progression_id.as_str(), row.max_level));
        let respec_costs = tables
            .categorical_progression_rank_data
            .iter()
            .filter(|table| {
                table
                    .unique_id
                    .eq_ignore_ascii_case(WEAPON_MASTERY_RANK_TABLE)
            })
            .flat_map(|table| &table.rows)
            .map(|row| (row.rank, row.azoth_respec_cost.unwrap_or_default()));
        Self {
            points: CharacterPoints::of_tables(tables),
            ..Self::build(levels, ranks, masteries)
        }
        .with_tradeskills(tradeskill_sources(tables))
        .with_progressions(progression_sources(tables))
        .with_respec_costs(respec_costs)
    }

    /// This progression with the other ranked progressions and currencies of `sources`; a
    /// source without ranks, or with the id of a weapon mastery or tradeskill, is left out.
    #[must_use]
    pub fn with_progressions(
        mut self,
        sources: impl IntoIterator<Item = ProgressionSource>,
    ) -> Self {
        let others: Vec<Mastery> = sources
            .into_iter()
            .filter(|source| !source.ranks.is_empty() && self.mastery(&source.id).is_none())
            .map(|mut source| {
                source.ranks.sort_unstable();
                let kind = if source.auto_rank_up {
                    MasteryKind::Ranked
                } else {
                    MasteryKind::Currency
                };
                Mastery {
                    crc: lowercase_crc32(&source.id),
                    id: source.id,
                    rank_caps: source.ranks.iter().map(|(_, cap)| clamp(*cap)).collect(),
                    // A currency never ranks up.
                    max_rank: match kind {
                        MasteryKind::Currency => 0,
                        _ => u16::try_from(source.max_level.max(0)).unwrap_or(u16::MAX),
                    },
                    kind,
                    perks: Vec::new(),
                }
            })
            .collect();
        self.others = others;
        self
    }

    /// This progression with the weapon tree respec costs `costs` (rank, `AzothRespecCost`),
    /// in any order.
    #[must_use]
    pub fn with_respec_costs(mut self, costs: impl IntoIterator<Item = (i64, i64)>) -> Self {
        self.respec_costs = costs
            .into_iter()
            .filter_map(|(rank, cost)| Some((u16::try_from(rank).ok()?, clamp(cost))))
            .collect();
        self
    }

    /// The azoth a respec of a weapon tree costs at weapon rank `rank` (zero-based); 0 for a
    /// rank the table does not list.
    pub fn respec_cost(&self, rank: u16) -> u32 {
        self.respec_costs.get(&rank).copied().unwrap_or_default()
    }

    /// The categorical progression `id` (any case): a weapon mastery, a tradeskill, another
    /// ranked progression or a currency.
    pub fn categorical(&self, id: &str) -> Option<&Mastery> {
        self.mastery(id).or_else(|| {
            self.others
                .iter()
                .find(|other| other.id.eq_ignore_ascii_case(id))
        })
    }

    /// Every categorical progression whose wire id is `crc`.
    pub fn categorical_by_crc(&self, crc: u32) -> Option<&Mastery> {
        self.masteries
            .iter()
            .chain(&self.tradeskills)
            .chain(&self.others)
            .find(|mastery| mastery.crc == crc)
    }

    /// Every categorical progression on the rank table of the weapon masteries or another:
    /// weapon masteries, tradeskills, other ranked progressions and currencies.
    pub fn categoricals(&self) -> impl Iterator<Item = &Mastery> {
        self.masteries
            .iter()
            .chain(&self.tradeskills)
            .chain(&self.others)
    }

    /// This progression with the tradeskills of `sources` (any order of ranks); a source
    /// without ranks is left out.
    #[must_use]
    pub fn with_tradeskills(mut self, sources: impl IntoIterator<Item = TradeskillSource>) -> Self {
        self.tradeskills = sources
            .into_iter()
            .filter(|source| !source.ranks.is_empty())
            .map(|mut source| {
                source.ranks.sort_by_key(|row| row.rank);
                let max_level = source.max_levels.iter().copied().max().unwrap_or_default();
                Mastery {
                    crc: lowercase_crc32(&source.id),
                    id: source.id,
                    rank_caps: source
                        .ranks
                        .iter()
                        .map(|row| clamp_float(row.cap))
                        .collect(),
                    max_rank: u16::try_from(max_level.max(0)).unwrap_or(u16::MAX),
                    kind: MasteryKind::Tradeskill,
                    perks: source.ranks.iter().map(|row| row.perks).collect(),
                }
            })
            .collect();
        self
    }

    /// The progression of `levels` (level number, `XPToLevel`), the weapon `ranks` (rank,
    /// `MaximumInfluence`) and the weapon `masteries` (id, `MaxLevel`), in any order.
    pub fn build<'a>(
        levels: impl IntoIterator<Item = (i64, i64)>,
        ranks: impl IntoIterator<Item = (i64, i64)>,
        masteries: impl IntoIterator<Item = (&'a str, i64)>,
    ) -> Self {
        let mut levels: Vec<_> = levels.into_iter().collect();
        levels.sort_unstable();
        let mut ranks: Vec<_> = ranks.into_iter().collect();
        ranks.sort_unstable();
        let rank_caps: Vec<u32> = ranks.into_iter().map(|(_, cap)| clamp(cap)).collect();
        Self {
            level_starts: levels.into_iter().map(|(_, start)| clamp(start)).collect(),
            masteries: masteries
                .into_iter()
                .map(|(id, max_level)| Mastery {
                    id: id.to_owned(),
                    crc: lowercase_crc32(id),
                    rank_caps: rank_caps.clone(),
                    max_rank: u16::try_from(max_level.max(0)).unwrap_or(u16::MAX),
                    kind: MasteryKind::Weapon,
                    perks: Vec::new(),
                })
                .collect(),
            tradeskills: Vec::new(),
            others: Vec::new(),
            respec_costs: BTreeMap::new(),
            points: CharacterPoints::default(),
        }
    }

    /// The level (zero-based) of a total of `experience`.
    pub fn level_of(&self, experience: u32) -> u32 {
        let reached = self
            .level_starts
            .iter()
            .take_while(|start| **start <= experience)
            .count();
        u32::try_from(reached.saturating_sub(1)).unwrap_or(u32::MAX)
    }

    /// The total experience at which `level` (zero-based) starts; `None` above the top level.
    pub fn experience_for_level(&self, level: u32) -> Option<u32> {
        self.level_starts.get(usize::try_from(level).ok()?).copied()
    }

    /// The top level, zero-based.
    pub fn max_level(&self) -> u32 {
        u32::try_from(self.level_starts.len().saturating_sub(1)).unwrap_or(u32::MAX)
    }

    /// The weapon mastery or tradeskill `id` (any case).
    pub fn mastery(&self, id: &str) -> Option<&Mastery> {
        self.masteries
            .iter()
            .chain(&self.tradeskills)
            .find(|mastery| mastery.id.eq_ignore_ascii_case(id))
    }

    /// The tradeskill `id` (any case).
    pub fn tradeskill(&self, id: &str) -> Option<&Mastery> {
        self.tradeskills
            .iter()
            .find(|mastery| mastery.id.eq_ignore_ascii_case(id))
    }

    /// The weapon mastery whose wire id is `crc`.
    pub fn mastery_by_crc(&self, crc: u32) -> Option<&Mastery> {
        self.masteries.iter().find(|mastery| mastery.crc == crc)
    }

    /// Every weapon mastery, in table order.
    pub fn masteries(&self) -> &[Mastery] {
        &self.masteries
    }
}

/// A table number as experience: negative is 0, above `u32::MAX` is `u32::MAX`.
fn clamp(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

/// A table float as experience, rounded (the tradeskill caps are whole numbers in the data).
fn clamp_float(value: f64) -> u32 {
    let rounded = value.round();
    if rounded >= f64::from(u32::MAX) {
        u32::MAX
    } else if rounded > 0.0 {
        // In range: checked above.
        rounded as u32
    } else {
        0
    }
}

/// The tradeskills of `tables`: the `CategoricalProgressionData` rows whose `RankTableId` names
/// a `TradeskillRankData` table, with that table's rows.
fn tradeskill_sources(tables: &Tables) -> Vec<TradeskillSource> {
    let rank_tables: HashMap<String, Vec<TradeskillRankRow>> = tables
        .tradeskill_rank_data
        .iter()
        .map(|table| {
            let rows = table
                .rows
                .iter()
                .map(|row| TradeskillRankRow {
                    rank: row.level,
                    cap: row.maximum_influence,
                    perks: RankPerks {
                        gathering_efficiency: row.gathering_efficiency.unwrap_or_default(),
                        recipe_level: row
                            .recipe_level
                            .and_then(|level| u32::try_from(level).ok())
                            .unwrap_or_default(),
                        gear_score_bonus: rank_gear_score_bonus([
                            (row.min_gear_score_bonus, row.max_gear_score_bonus),
                            (row.min_gear_score_bonus_t2, row.max_gear_score_bonus_t2),
                            (row.min_gear_score_bonus_t3, row.max_gear_score_bonus_t3),
                            (row.min_gear_score_bonus_t4, row.max_gear_score_bonus_t4),
                            (row.min_gear_score_bonus_t5, row.max_gear_score_bonus_t5),
                        ]),
                    },
                })
                .collect();
            (table.unique_id.to_lowercase(), rows)
        })
        .collect();
    tables
        .categorical_progression_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let ranks = rank_tables.get(&row.rank_table_id.as_deref()?.to_lowercase())?;
            Some(TradeskillSource {
                id: row.categorical_progression_id.clone(),
                max_levels: [
                    Some(row.max_level),
                    row.expansion01_max_level,
                    row.expansion02_max_level,
                ]
                .into_iter()
                .flatten()
                .collect(),
                ranks: ranks.clone(),
            })
        })
        .collect()
}

/// The other categorical progressions of `tables`: the `CategoricalProgressionData` rows whose
/// `RankTableId` names a `CategoricalProgressionRankData` table other than `WeaponMastery`,
/// with that table's rows.
fn progression_sources(tables: &Tables) -> Vec<ProgressionSource> {
    let rank_tables: HashMap<String, Vec<(i64, i64)>> = tables
        .categorical_progression_rank_data
        .iter()
        .filter(|table| {
            !table
                .unique_id
                .eq_ignore_ascii_case(WEAPON_MASTERY_RANK_TABLE)
        })
        .map(|table| {
            let rows = table
                .rows
                .iter()
                .map(|row| (row.rank, row.maximum_influence))
                .collect();
            (table.unique_id.to_lowercase(), rows)
        })
        .collect();
    tables
        .categorical_progression_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let ranks = rank_tables.get(&row.rank_table_id.as_deref()?.to_lowercase())?;
            Some(ProgressionSource {
                id: row.categorical_progression_id.clone(),
                max_level: row.max_level,
                auto_rank_up: row.auto_rank_up.unwrap_or_default(),
                ranks: ranks.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Retail `XPToLevel` of levels 0 to 5 and the weapon rank caps of ranks 0 to 4.
    const LEVEL_STARTS: [i64; 6] = [0, 150, 480, 1080, 1970, 3070];
    const RANK_CAPS: [i64; 5] = [10, 50, 70, 105, 155];
    const TOP_RANK: i64 = 4;
    const GREATSWORD: &str = "GreatSwordAbilityTable";
    /// Its retail wire id.
    const GREATSWORD_CRC: u32 = 0x180b_9a9f;

    /// The retail level starts and one weapon mastery, both given out of order.
    pub(crate) fn progression() -> Progression {
        let numbered = |values: &[i64]| -> Vec<(i64, i64)> {
            let mut rows: Vec<_> = (0..).zip(values.iter().copied()).collect();
            rows.reverse();
            rows
        };
        Progression::build(
            numbered(&LEVEL_STARTS),
            numbered(&RANK_CAPS),
            [(GREATSWORD, TOP_RANK)],
        )
    }

    #[test]
    fn the_level_follows_the_total_experience() {
        let progression = progression();
        assert_eq!(progression.level_of(0), 0);
        assert_eq!(progression.level_of(149), 0);
        assert_eq!(progression.level_of(150), 1);
        // Retail: one gain from level 0 to 2.
        assert_eq!(progression.level_of(670), 2);
        assert_eq!(progression.level_of(u32::MAX), 5);
        assert_eq!(progression.max_level(), 5);
        assert_eq!(progression.experience_for_level(3), Some(1080));
        assert_eq!(progression.experience_for_level(6), None);
    }

    #[test]
    fn a_mastery_rank_follows_the_caps() {
        let progression = progression();
        let sword = progression
            .mastery("greatswordabilitytable")
            .expect("mastery");
        assert_eq!(sword.crc, GREATSWORD_CRC);
        assert_eq!(sword.rank_of(0), (0, 0));
        // Retail first kill: 20 experience gives rank 1 with 10 points.
        assert_eq!(sword.rank_of(20), (1, 10));
        assert_eq!(sword.experience_for_rank(2), 60);
        // The top rank keeps no points.
        assert_eq!(sword.max_experience(), 235);
        assert_eq!(sword.rank_of(sword.max_experience()), (4, 0));
        assert_eq!(sword.rank_of(u32::MAX), (4, 0));
        assert_eq!(sword.experience_for_rank(u16::MAX), sword.max_experience());
        assert!(progression.mastery("Cooking").is_none());
        assert_eq!(
            progression
                .mastery_by_crc(GREATSWORD_CRC)
                .map(|mastery| mastery.id.as_str()),
            Some(GREATSWORD)
        );
        assert!(progression.mastery_by_crc(0).is_none());
    }

    #[test]
    fn tables_without_the_rows_give_no_progression() {
        let progression = Progression::of_tables(&Tables::default());
        assert!(progression.masteries().is_empty());
        assert!(progression.tradeskill("Skinning").is_none());
        assert_eq!(progression.level_of(1_000), 0);
    }

    /// The first ranks of `javelindata_tradeskillskinning` (build 25810808): caps and gathering
    /// efficiency; skinning recipes have no levels.
    const SKINNING_RANKS: [(i64, f64, f64); 6] = [
        (0, 90.0, 0.0),
        (1, 110.0, 0.05),
        (2, 120.0, 0.1),
        (3, 130.0, 0.15),
        (4, 140.0, 0.2),
        (5, 150.0, 0.25),
    ];
    /// Skinning's `MaxLevel` and `Expansion01MaxLevel`.
    const SKINNING_MAX_LEVELS: [i64; 2] = [200, 250];

    fn skinning() -> TradeskillSource {
        TradeskillSource {
            id: "Skinning".to_owned(),
            max_levels: SKINNING_MAX_LEVELS.to_vec(),
            ranks: SKINNING_RANKS
                .iter()
                .rev()
                .map(|(rank, cap, efficiency)| TradeskillRankRow {
                    rank: *rank,
                    cap: *cap,
                    perks: RankPerks {
                        gathering_efficiency: *efficiency,
                        recipe_level: u32::try_from(*rank).expect("rank"),
                        ..RankPerks::default()
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn a_tradeskill_follows_its_rank_table_and_is_no_weapon() {
        let progression = progression().with_tradeskills([
            skinning(),
            TradeskillSource {
                id: "Empty".to_owned(),
                max_levels: vec![1],
                ranks: Vec::new(),
            },
        ]);
        let skinning = progression.mastery("skinning").expect("tradeskill");
        assert_eq!(skinning.kind, MasteryKind::Tradeskill);
        assert_eq!(skinning.max_rank, 250, "the expansion's top rank");
        // Retail newchar 2026-09-18: the first skinning showed rank 1 with 108 points, the
        // second rank 3 with 86, the third rank 5 with 24.
        assert_eq!(skinning.rank_of(198), (1, 108));
        assert_eq!(skinning.rank_of(198 + 208), (3, 86));
        assert_eq!(skinning.rank_of(198 + 208 + 208), (5, 24));
        let perks = skinning.perks(198 + 208);
        assert!((perks.gathering_efficiency - 0.15).abs() < f64::EPSILON);
        assert_eq!(perks.recipe_level, 3);
        let bonus = rank_gear_score_bonus([
            (Some(0), Some(5)),
            (Some(20), Some(10)),
            (None, Some(75)),
            (Some(-1), None),
            (None, None),
        ]);
        assert_eq!(bonus, [(0, 5), (10, 10), (0, 75), (0, 0), (0, 0)]);
        let ranked = RankPerks {
            gear_score_bonus: bonus,
            ..RankPerks::default()
        };
        assert_eq!(ranked.gear_score_bonus_of(3), (0, 75));
        assert_eq!(ranked.gear_score_bonus_of(0), (0, 0));
        assert_eq!(ranked.gear_score_bonus_of(6), (0, 0));
        assert_eq!(
            progression
                .tradeskill("SKINNING")
                .map(|mastery| mastery.crc),
            Some(lowercase_crc32("skinning"))
        );
        assert!(progression.tradeskill("Empty").is_none(), "no rank table");
        assert!(
            progression.mastery_by_crc(skinning.crc).is_none(),
            "a tradeskill has no weapon pool"
        );
        assert!(
            progression
                .masteries()
                .iter()
                .all(|mastery| mastery.kind == MasteryKind::Weapon)
        );
        assert_eq!(
            progression
                .mastery(GREATSWORD)
                .map(|mastery| mastery.perks(0)),
            Some(RankPerks::default())
        );
    }

    /// `AzothCurrency` rank 0 `MaximumInfluence` (content bundle 25810808).
    const AZOTH_CAP: i64 = 2_500;
    /// `Camping` (`AutoRankUp` true, `MaxLevel` 5) and the first caps of its rank table.
    const CAMPING_CAPS: [(i64, i64); 2] = [(0, 100), (1, 200)];
    const CAMPING_TOP: i64 = 5;
    /// `AzothRespecCost` of the weapon ranks 9 to 11 (content bundle 25810808).
    const RESPEC_COSTS: [(i64, i64); 3] = [(11, 6), (9, 0), (10, 6)];

    #[test]
    fn currencies_keep_a_capped_balance_and_respecs_cost_by_rank() {
        let progression = progression()
            .with_progressions([
                ProgressionSource {
                    id: AZOTH_PROGRESSION.to_owned(),
                    max_level: 0,
                    auto_rank_up: false,
                    ranks: vec![(0, AZOTH_CAP)],
                },
                ProgressionSource {
                    id: "Camping".to_owned(),
                    max_level: CAMPING_TOP,
                    auto_rank_up: true,
                    ranks: CAMPING_CAPS.to_vec(),
                },
                ProgressionSource {
                    id: GREATSWORD.to_owned(),
                    max_level: 1,
                    auto_rank_up: false,
                    ranks: vec![(0, 1)],
                },
                ProgressionSource {
                    id: "NoRanks".to_owned(),
                    max_level: 1,
                    auto_rank_up: true,
                    ranks: Vec::new(),
                },
            ])
            .with_respec_costs(RESPEC_COSTS);
        let azoth = progression.categorical("azoth_currency").expect("currency");
        assert_eq!(azoth.kind, MasteryKind::Currency);
        assert!(
            progression.mastery(AZOTH_PROGRESSION).is_none(),
            "no mastery"
        );
        assert_eq!(azoth.rank_of(1_234), (0, 1_234));
        assert_eq!(
            azoth.rank_of(u32::MAX),
            (0, 2_500),
            "the balance stops at the cap"
        );
        assert_eq!(azoth.max_experience(), 2_500);
        assert_eq!(azoth.experience_for_rank(3), 0);
        let camping = progression.categorical("Camping").expect("ranked");
        assert_eq!(camping.kind, MasteryKind::Ranked);
        assert_eq!(camping.rank_of(150), (1, 50));
        assert_eq!(
            progression
                .categorical(GREATSWORD)
                .map(|mastery| mastery.kind),
            Some(MasteryKind::Weapon),
            "a weapon mastery's id is not taken again"
        );
        assert!(progression.categorical("NoRanks").is_none());
        assert_eq!(
            progression
                .categorical_by_crc(azoth.crc)
                .map(|found| found.id.as_str()),
            Some(AZOTH_PROGRESSION)
        );
        assert_eq!(progression.categoricals().count(), 3);
        assert_eq!(progression.respec_cost(9), 0);
        assert_eq!(progression.respec_cost(10), 6);
        assert_eq!(progression.respec_cost(11), 6);
        assert_eq!(
            progression.respec_cost(2),
            0,
            "a rank the table does not list"
        );
    }
}
