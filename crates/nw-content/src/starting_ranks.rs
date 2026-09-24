// SPDX-License-Identifier: AGPL-3.0-or-later
//! The categorical progression ranks a new character of an archetype pick starts with (craft2
//! report, run 15).
//!
//! **Observed.** A retail new character of `Archetype_Swordsman` (newchar 2026-09-18) had Cooking,
//! Leatherworking and Fishing at rank 50 with 0 points in its first type-911 list; one of
//! `Archetype_Ranger` (`gather_node_20260912_233618`) had Engineering, Woodworking and Logging at
//! rank 50. Its `BackstoryDefinition` row gives them: one column per tradeskill (`Cooking` 50,
//! `Leatherworking` 50) and the `CategoricalProgression` cell (`Fishing:50`); the column name is
//! the `CategoricalProgressionId`. The `WeaponMasteries` cell (`SwordAbilityTable:19`) holds
//! weapon mastery ranks the same way.

use std::collections::HashMap;

use crate::Tables;
use crate::generated::BackstoryDefinition;

/// Separates the pairs of a `CategoricalProgression` or `WeaponMasteries` cell.
const PAIR_SEPARATOR: char = ',';
/// Separates a progression id from its rank in one pair.
const RANK_SEPARATOR: char = ':';

/// The starting ranks of every backstory, by lowercase backstory id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartingRanks {
    by_backstory: HashMap<String, Vec<(String, u16)>>,
}

/// The tradeskill columns of a backstory row, by their `CategoricalProgressionId`.
fn tradeskill_columns(row: &BackstoryDefinition) -> [(&'static str, Option<i64>); 18] {
    [
        ("Weaponsmithing", row.weaponsmithing),
        ("Armoring", row.armoring),
        ("Jewelcrafting", row.jewelcrafting),
        ("Arcana", row.arcana),
        ("Cooking", row.cooking),
        ("Furnishing", row.furnishing),
        ("Engineering", row.engineering),
        ("Smelting", row.smelting),
        ("Woodworking", row.woodworking),
        ("Leatherworking", row.leatherworking),
        ("Weaving", row.weaving),
        ("Stonecutting", row.stonecutting),
        ("Skinning", row.skinning),
        ("Mining", row.mining),
        ("Logging", row.logging),
        ("Harvesting", row.harvesting),
        ("Musician", row.musician),
        ("AzothStaff", row.azoth_staff),
    ]
}

/// The `id:rank` pairs of a cell; a pair without a whole rank is left out.
fn pairs(cell: Option<&str>) -> impl Iterator<Item = (String, u16)> + '_ {
    cell.into_iter()
        .flat_map(|cell| cell.split(PAIR_SEPARATOR))
        .filter_map(|pair| {
            let (id, rank) = pair.split_once(RANK_SEPARATOR)?;
            let id = id.trim();
            let rank = rank.trim().parse().ok()?;
            (!id.is_empty()).then(|| (id.to_owned(), rank))
        })
}

impl StartingRanks {
    /// The starting ranks of the `BackstoryDefinition` rows in `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .backstory_definition
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    let columns = tradeskill_columns(row).into_iter().filter_map(|(id, rank)| {
                        let rank = u16::try_from(rank?).ok().filter(|rank| *rank > 0)?;
                        Some((id.to_owned(), rank))
                    });
                    let ranks = columns
                        .chain(pairs(row.categorical_progression.as_deref()))
                        .chain(pairs(row.weapon_masteries.as_deref()))
                        .collect();
                    (row.backstory_id.clone(), ranks)
                }),
        )
    }

    /// The starting ranks of `backstories` (backstory id, (progression id, rank) in row order);
    /// the first row of an id wins.
    pub fn build(backstories: impl IntoIterator<Item = (String, Vec<(String, u16)>)>) -> Self {
        let mut by_backstory = HashMap::new();
        for (id, ranks) in backstories {
            by_backstory.entry(id.to_lowercase()).or_insert(ranks);
        }
        Self { by_backstory }
    }

    /// The (progression id, rank) pairs a new character of `backstory` (any case) starts with,
    /// in row order; empty for an unknown backstory.
    pub fn of(&self, backstory: &str) -> &[(String, u16)] {
        self.by_backstory
            .get(&backstory.to_lowercase())
            .map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_lists_its_ranks() {
        let ranks: Vec<_> = pairs(Some("Fishing:50, SwordAbilityTable:19,Broken,:4,X:y")).collect();
        assert_eq!(
            ranks,
            [
                ("Fishing".to_owned(), 50),
                ("SwordAbilityTable".to_owned(), 19)
            ]
        );
        assert_eq!(pairs(None).count(), 0);
    }

    #[test]
    fn an_archetype_starts_with_its_ranks() {
        // Retail `Archetype_Swordsman`: Cooking and Leatherworking columns 50, `Fishing:50`.
        let swordsman = vec![
            ("Cooking".to_owned(), 50),
            ("Leatherworking".to_owned(), 50),
            ("Fishing".to_owned(), 50),
        ];
        let ranks = StartingRanks::build([
            ("Archetype_Swordsman".to_owned(), swordsman.clone()),
            ("archetype_swordsman".to_owned(), Vec::new()),
        ]);
        assert_eq!(ranks.of("ARCHETYPE_SWORDSMAN"), swordsman.as_slice());
        assert!(ranks.of("Archetype_Mystic").is_empty());
    }
}
