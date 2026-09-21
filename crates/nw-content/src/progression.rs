// SPDX-License-Identifier: AGPL-3.0-or-later
//! The character level and the weapon masteries of the game data
//! (docs/re/2026-09-18-progression-and-gm-mode.md sections 2 and 3).
//!
//! The level comes from the total experience: the highest level whose `XPToLevel` (the total at
//! which the level starts) is at most the experience. A weapon mastery (a
//! `CategoricalProgressionData` row on the `WeaponMastery` rank table) has a rank and the points
//! inside it; this module keeps the total experience of a mastery and splits it into the two.

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The level table (`javelindata_xpamountsbylevel`).
pub const LEVEL_TABLE: &str = "XPLevels";
/// The rank table of the weapon masteries (`javelindata_weaponmastery`).
pub const WEAPON_MASTERY_RANK_TABLE: &str = "WeaponMastery";

/// One weapon mastery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mastery {
    /// `CategoricalProgressionId`, for example `GreatSwordAbilityTable`.
    pub id: String,
    /// The CRC-32 of the lowercase id: its id on the wire (type 911).
    pub crc: u32,
    /// The experience of each rank before the next one (`MaximumInfluence`), rank 0 first.
    rank_caps: Vec<u32>,
    /// The top rank, zero-based (`MaxLevel`).
    pub max_rank: u16,
}

impl Mastery {
    /// The rank (zero-based) and the points inside it of a total of `experience`. At the top
    /// rank the points stay 0, as retail sends them.
    pub fn rank_of(&self, experience: u32) -> (u16, u64) {
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

    /// The total experience at the top rank: more gives nothing.
    pub fn max_experience(&self) -> u32 {
        self.experience_for_rank(self.max_rank)
    }

    fn cap(&self, rank: u16) -> u32 {
        self.rank_caps
            .get(usize::from(rank))
            .copied()
            .unwrap_or(u32::MAX)
    }
}

/// The level table and the weapon masteries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progression {
    /// The total experience at which each level starts, level 0 first.
    level_starts: Vec<u32>,
    /// The weapon masteries, in table order.
    masteries: Vec<Mastery>,
}

impl Progression {
    /// The level table and the weapon masteries of `tables`; empty when a table is missing.
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
        Self::build(levels, ranks, masteries)
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
                })
                .collect(),
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

    /// The weapon mastery `id` (any case).
    pub fn mastery(&self, id: &str) -> Option<&Mastery> {
        self.masteries
            .iter()
            .find(|mastery| mastery.id.eq_ignore_ascii_case(id))
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
    }

    #[test]
    fn tables_without_the_rows_give_no_progression() {
        let progression = Progression::of_tables(&Tables::default());
        assert!(progression.masteries().is_empty());
        assert_eq!(progression.level_of(1_000), 0);
    }
}
