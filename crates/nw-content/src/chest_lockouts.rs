// SPDX-License-Identifier: AGPL-3.0-or-later
//! How long a player may not open a loot container (a chest) again (craft2 report, run 15; gather
//! helpers D8 and D9).
//!
//! **Observed.** Retail changes no chest state when a player opens one: the player's own loot
//! tracker (982) gets an entry for the chest, looted (state 2), with a reset time, at the gather's
//! end. While the entry lives a new gather of that chest is refused (3743 kind 14 status 2). The
//! reset time of an elite chest (`GatherableData.IsElite`) is the server's next daily reset (94 of
//! 96 entries in 24 ledgers; [`crate::resets`]); its `MaxRespawnRate` (82,800 s) is not its
//! lockout. Another container's is a roll of its respawn time, or 3,600 s without one (638 of 638
//! entries; no data column holds it), capped at the daily reset (the sim's gather lock).

use std::collections::HashSet;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The lockout of a container without a respawn rate (Observed, 638 of 638 entries).
pub const CONTAINER_LOCKOUT_MS: u64 = 3_600_000;

/// The elite chests: the loot containers whose lock lasts until the next daily reset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EliteChests {
    crcs: HashSet<u32>,
}

/// One `GatherableData` row as [`EliteChests`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerRow<'a> {
    /// `GatherableID`.
    pub id: &'a str,
    /// `IsLootContainer`.
    pub container: bool,
    /// `IsElite`.
    pub elite: bool,
}

impl EliteChests {
    /// The elite chests of `GatherableData` in `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .gatherable_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| ContainerRow {
                    id: &row.gatherable_id,
                    container: row.is_loot_container.unwrap_or_default(),
                    elite: row.is_elite.unwrap_or_default(),
                }),
        )
    }

    /// The elite chests of `rows`: the rows that are a loot container and elite.
    pub fn build<'a>(rows: impl IntoIterator<Item = ContainerRow<'a>>) -> Self {
        Self {
            crcs: rows
                .into_iter()
                .filter(|row| row.container && row.elite)
                .map(|row| lowercase_crc32(row.id))
                .collect(),
        }
    }

    /// Whether the gatherable whose wire id is `crc` is an elite chest.
    pub fn contains(&self, crc: u32) -> bool {
        self.crcs.contains(&crc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_elite_loot_container_is_an_elite_chest() {
        let row = |id, container, elite| ContainerRow {
            id,
            container,
            elite,
        };
        let chests = EliteChests::build([
            row("EliteChest", true, true),
            row("SupplyCrate", true, false),
            row("EliteVein", false, true),
        ]);
        assert!(chests.contains(lowercase_crc32("elitechest")));
        assert!(!chests.contains(lowercase_crc32("supplycrate")));
        assert!(!chests.contains(lowercase_crc32("elitevein")));
    }
}
