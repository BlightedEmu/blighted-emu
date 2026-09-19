// SPDX-License-Identifier: AGPL-3.0-or-later
//! The gatherables and their loot (gather note, 2026-09-18): a gather request names its
//! gatherable by the CRC-32 of its lowercase `GatherableID`, and the gatherable's
//! `FinalLootTable` gives the items.
//!
//! A loot table `T` is three rows: `T` (the entries, `AND` or `OR`, a condition), `T_Qty` (each
//! entry's quantity, `a` or `a-b`) and `T_Probs` (each entry's threshold, with `MaxRoll`). An
//! `AND` table rolls each entry and drops it when the roll reaches its threshold; an `OR` table
//! rolls once and drops the entry with the highest threshold the roll reaches. A table with the
//! condition `EnemyLevel` or `MinContLevel` uses that level as its roll. An entry `[LTID]X` rolls
//! table `X` quantity times.

use std::collections::{BTreeMap, HashMap};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The suffixes of a table's quantity and threshold rows.
const QUANTITY_SUFFIX: &str = "_Qty";
const THRESHOLD_SUFFIX: &str = "_Probs";
/// An entry that rolls another loot table.
const TABLE_PREFIX: &str = "[LTID]";
/// An entry that names a loot bucket (not read yet: such an entry drops nothing).
const BUCKET_PREFIX: &str = "[LBID]";
/// The quantity range separator (`1-2`).
const RANGE_SEPARATOR: char = '-';
/// The `AndOr` value of a table that drops every entry its roll reaches.
const AND: &str = "AND";
/// The conditions whose level is a table's roll.
const ENEMY_LEVEL: &str = "EnemyLevel";
const CONTENT_LEVEL: &str = "MinContLevel";
/// The tradeskill of a gatherable that is skinned.
const SKINNING: &str = "Skinning";
/// How deep `[LTID]` entries may nest (retail tables nest 3 deep).
const MAX_DEPTH: u8 = 8;
/// Milliseconds per second.
const MS_PER_SECOND: f64 = 1_000.0;

/// One gatherable of `javelindata_gatherables`.
#[derive(Debug, Clone, PartialEq)]
pub struct Gatherable {
    /// `GatherableID`.
    pub id: String,
    /// `BaseGatherTime`, in ms.
    pub gather_ms: u64,
    /// `MinRespawnRate` and `MaxRespawnRate`, in ms.
    pub respawn_ms: (u64, u64),
    /// `FinalLootTable`.
    pub loot_table: Option<String>,
    /// It is a corpse to skin (`Tradeskill` Skinning).
    pub skinning: bool,
}

/// The levels a loot roll reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LootLevels {
    /// The level of the mob skinned (`EnemyLevel`).
    pub enemy: u32,
    /// The level of the gatherer (`MinContLevel`).
    pub content: u32,
}

/// One entry of a loot table: its item or table, quantity and threshold.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LootEntry {
    item: String,
    quantity: (u32, u32),
    threshold: u32,
}

/// What a table rolls on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Roll {
    /// A random roll below this.
    Random(u32),
    /// The enemy's level.
    EnemyLevel,
    /// The gatherer's level.
    ContentLevel,
    /// A condition this server does not read: the table drops nothing.
    Unread,
}

/// One loot table.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LootTable {
    and: bool,
    roll: Roll,
    entries: Vec<LootEntry>,
}

/// The gatherables by the CRC-32 of their lowercase id, and the loot tables.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gatherables {
    by_crc: HashMap<u32, Gatherable>,
    tables: HashMap<String, LootTable>,
}

/// A quantity `a` or `a-b`; `None` for another text.
fn quantity(text: &str) -> Option<(u32, u32)> {
    match text.split_once(RANGE_SEPARATOR) {
        Some((low, high)) => Some((low.trim().parse().ok()?, high.trim().parse().ok()?)),
        None => text.trim().parse().ok().map(|count| (count, count)),
    }
}

/// Seconds of a datasheet as whole ms.
fn ms_of(seconds: f64) -> u64 {
    // Gather and respawn times of a few minutes at most.
    (seconds * MS_PER_SECOND).round().max(0.0) as u64
}

/// One row of `javelindata_loottables`: a table, or its quantity or threshold row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LootRow {
    /// `LootTableID`.
    pub id: String,
    /// `AND-OR`.
    pub and_or: Option<String>,
    /// `Conditions`.
    pub conditions: Option<String>,
    /// `MaxRoll`.
    pub max_roll: Option<i64>,
    /// The `Item` cells by column number.
    pub cells: BTreeMap<u32, String>,
}

/// What [`Gatherables`] are built from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GatheringSource {
    /// The gatherables.
    pub gatherables: Vec<Gatherable>,
    /// The loot table rows.
    pub loot_rows: Vec<LootRow>,
}

impl GatheringSource {
    /// The gatherables and loot rows of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self {
            gatherables: tables
                .gatherable_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| Gatherable {
                    id: row.gatherable_id.clone(),
                    gather_ms: ms_of(row.base_gather_time),
                    respawn_ms: (ms_of(row.min_respawn_rate), ms_of(row.max_respawn_rate)),
                    loot_table: row.final_loot_table.clone(),
                    skinning: row.tradeskill.as_deref() == Some(SKINNING),
                })
                .collect(),
            loot_rows: tables
                .loot_tables_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| LootRow {
                    id: row.loot_table_id.clone(),
                    and_or: row.and_or.clone(),
                    conditions: row.conditions.clone(),
                    max_roll: row.max_roll,
                    cells: row.item.clone(),
                })
                .collect(),
        }
    }
}

impl Gatherables {
    /// The gatherables and loot tables of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(GatheringSource::from_tables(tables))
    }

    /// The gatherables and loot tables of `source`.
    pub fn build(source: GatheringSource) -> Self {
        let by_crc = source
            .gatherables
            .into_iter()
            .map(|gatherable| (lowercase_crc32(&gatherable.id), gatherable))
            .collect();
        let rows: HashMap<&str, &LootRow> = source
            .loot_rows
            .iter()
            .map(|row| (row.id.as_str(), row))
            .collect();
        let tables = rows
            .iter()
            .filter(|(id, _)| !id.ends_with(QUANTITY_SUFFIX) && !id.ends_with(THRESHOLD_SUFFIX))
            .map(|(id, row)| {
                let quantities = rows.get(format!("{id}{QUANTITY_SUFFIX}").as_str());
                let thresholds = rows.get(format!("{id}{THRESHOLD_SUFFIX}").as_str());
                let cell = |row: Option<&&LootRow>, index: &u32| row?.cells.get(index).cloned();
                let entries = row
                    .cells
                    .iter()
                    .map(|(index, item)| LootEntry {
                        item: item.clone(),
                        quantity: cell(quantities, index)
                            .and_then(|text| quantity(&text))
                            .unwrap_or((1, 1)),
                        threshold: cell(thresholds, index)
                            .and_then(|text| text.trim().parse().ok())
                            .unwrap_or(0),
                    })
                    .collect();
                let max_roll = thresholds
                    .and_then(|row| row.max_roll)
                    .and_then(|roll| u32::try_from(roll).ok())
                    .unwrap_or(1);
                let roll = match row.conditions.as_deref() {
                    None => Roll::Random(max_roll),
                    Some(ENEMY_LEVEL) => Roll::EnemyLevel,
                    Some(CONTENT_LEVEL) => Roll::ContentLevel,
                    Some(_) => Roll::Unread,
                };
                (
                    (*id).to_owned(),
                    LootTable {
                        and: row.and_or.as_deref() == Some(AND),
                        roll,
                        entries,
                    },
                )
            })
            .collect();
        Self { by_crc, tables }
    }

    /// The gatherable whose lowercase id has the CRC-32 `crc`.
    pub fn get(&self, crc: u32) -> Option<&Gatherable> {
        self.by_crc.get(&crc)
    }

    /// The items loot table `table` drops at `levels`, merged by item, with `below(n)` a random
    /// number below `n`.
    pub fn roll(
        &self,
        table: &str,
        levels: LootLevels,
        below: &mut impl FnMut(u32) -> u32,
    ) -> Vec<(String, u32)> {
        let mut drops: BTreeMap<String, u32> = BTreeMap::new();
        self.roll_into(table, levels, below, 0, &mut drops);
        drops.into_iter().collect()
    }

    fn roll_into(
        &self,
        table: &str,
        levels: LootLevels,
        below: &mut impl FnMut(u32) -> u32,
        depth: u8,
        drops: &mut BTreeMap<String, u32>,
    ) {
        let Some(table) = self.tables.get(table).filter(|_| depth < MAX_DEPTH) else {
            return;
        };
        let draw = |below: &mut dyn FnMut(u32) -> u32| match table.roll {
            Roll::Random(max) => Some(below(max.max(1))),
            Roll::EnemyLevel => Some(levels.enemy),
            Roll::ContentLevel => Some(levels.content),
            Roll::Unread => None,
        };
        let dropped: Vec<&LootEntry> = if table.and {
            table
                .entries
                .iter()
                .filter(|entry| draw(below).is_some_and(|roll| roll >= entry.threshold))
                .collect()
        } else {
            let Some(roll) = draw(below) else {
                return;
            };
            let reached = table
                .entries
                .iter()
                .filter(|entry| entry.threshold <= roll)
                .map(|entry| entry.threshold)
                .max();
            let best: Vec<&LootEntry> = table
                .entries
                .iter()
                .filter(|entry| Some(entry.threshold) == reached)
                .collect();
            // Entries with the same threshold share the chance.
            let count = u32::try_from(best.len()).unwrap_or(u32::MAX);
            usize::try_from(below(count.max(1)))
                .ok()
                .and_then(|pick| best.get(pick))
                .copied()
                .into_iter()
                .collect()
        };
        for entry in dropped {
            let (low, high) = entry.quantity;
            let count = low + below(high.saturating_sub(low).saturating_add(1));
            if let Some(inner) = entry.item.strip_prefix(TABLE_PREFIX) {
                for _ in 0..count {
                    self.roll_into(inner, levels, below, depth + 1, drops);
                }
            } else if !entry.item.starts_with(BUCKET_PREFIX) && count > 0 {
                *drops.entry(entry.item.clone()).or_default() += count;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loot(
        id: &str,
        cells: &[(u32, &str)],
        and_or: Option<&str>,
        conditions: Option<&str>,
    ) -> LootRow {
        LootRow {
            id: id.to_owned(),
            and_or: and_or.map(str::to_owned),
            conditions: conditions.map(str::to_owned),
            max_roll: None,
            cells: cells
                .iter()
                .map(|(index, text)| (*index, (*text).to_owned()))
                .collect(),
        }
    }

    fn probs(id: &str, cells: &[(u32, &str)], max_roll: i64) -> LootRow {
        LootRow {
            max_roll: Some(max_roll),
            ..loot(id, cells, None, None)
        }
    }

    /// The retail rows of flint and of a boar's skin (javelindata_loottables, build 25810808),
    /// with the bait table left out.
    fn gatherables() -> Gatherables {
        let loot_rows = vec![
            loot(
                "SingleStone",
                &[(1, "FlintT1"), (2, "[LTID]BaitDayNight")],
                Some("AND"),
                None,
            ),
            loot("SingleStone_Qty", &[(1, "1"), (2, "1")], None, None),
            probs("SingleStone_Probs", &[(1, "0"), (2, "87500")], 100_000),
            loot(
                "BoarSkinning",
                &[(1, "[LTID]SkinningSmall"), (2, "[LTID]BoarMeats")],
                Some("AND"),
                None,
            ),
            loot("BoarSkinning_Qty", &[(1, "7"), (2, "1")], None, None),
            probs("BoarSkinning_Probs", &[(1, "0"), (2, "0")], 100_000),
            loot(
                "SkinningSmall",
                &[(1, "RawhideT1"), (2, "RawhideT1"), (3, "RawhideT4")],
                Some("OR"),
                Some("EnemyLevel"),
            ),
            loot(
                "SkinningSmall_Qty",
                &[(1, "1-2"), (2, "1-2"), (3, "1-2")],
                None,
                None,
            ),
            probs("SkinningSmall_Probs", &[(1, "0"), (2, "2"), (3, "32")], 1),
            loot("BoarMeats", &[(1, "PorkT1")], Some("AND"), None),
            loot("BoarMeats_Qty", &[(1, "1-2")], None, None),
            probs("BoarMeats_Probs", &[(1, "0")], 100_000),
        ];
        Gatherables::build(GatheringSource {
            gatherables: vec![Gatherable {
                id: "SingleStone".to_owned(),
                gather_ms: 250,
                respawn_ms: (60_000, 90_000),
                loot_table: Some("SingleStone".to_owned()),
                skinning: false,
            }],
            loot_rows,
        })
    }

    const LEVELS: LootLevels = LootLevels {
        enemy: 3,
        content: 1,
    };

    #[test]
    fn a_gatherable_is_found_by_the_crc_of_its_lowercase_id() {
        // Retail flint request: crc `a417137f`.
        let flint = gatherables().get(0xa417_137f).cloned().expect("flint");
        assert_eq!(flint.id, "SingleStone");
        assert_eq!(ms_of(0.25), 250);
    }

    #[test]
    fn flint_always_gives_one_flint() {
        let gatherables = gatherables();
        for seed in 0..100_u32 {
            let mut below = |n: u32| seed.wrapping_mul(7_919) % n;
            assert_eq!(
                gatherables.roll("SingleStone", LEVELS, &mut below),
                [("FlintT1".to_owned(), 1)],
                "the bait table is not loaded: it drops nothing"
            );
        }
    }

    #[test]
    fn a_boar_gives_seven_level_rolls_of_rawhide_and_pork() {
        let gatherables = gatherables();
        let mut lowest = |_: u32| 0;
        assert_eq!(
            gatherables.roll("BoarSkinning", LEVELS, &mut lowest),
            [("PorkT1".to_owned(), 1), ("RawhideT1".to_owned(), 7)]
        );
        let mut highest = |n: u32| n - 1;
        let drops = gatherables.roll("BoarSkinning", LEVELS, &mut highest);
        assert_eq!(
            drops,
            [("PorkT1".to_owned(), 2), ("RawhideT1".to_owned(), 14)],
            "level 3 reaches the threshold-2 row, never the level-32 RawhideT4"
        );
    }

    #[test]
    fn quantities_read_a_count_or_a_range() {
        assert_eq!(quantity("1"), Some((1, 1)));
        assert_eq!(quantity("2-3"), Some((2, 3)));
        assert_eq!(quantity("x"), None);
    }
}
