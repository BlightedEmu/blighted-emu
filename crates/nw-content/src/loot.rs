// SPDX-License-Identifier: AGPL-3.0-or-later
//! Loot tables and their rolls (gather note 2026-09-18, admin studio spec section 6.2), for
//! gathering, mob kills and the studio's drop simulator.
//!
//! A loot table `T` is three rows: `T` (the entries, `AND` or `OR`, a condition), `T_Qty` (each
//! entry's quantity, `a` or `a-b`) and `T_Probs` (each entry's threshold, with `MaxRoll`). An
//! `AND` table rolls each entry and drops it when the roll reaches its threshold; an `OR` table
//! rolls once and drops the entry with the highest threshold the roll reaches. A table with the
//! condition `EnemyLevel` or `MinContLevel` uses that level as its roll. An entry `[LTID]X` rolls
//! table `X` quantity times; an entry `[LBID]X` drops quantity items of loot bucket `X`, each
//! picked with equal chance (the bucket's tags and odds are not read: Unknown).

use std::collections::{BTreeMap, HashMap};

use crate::Tables;

/// The suffixes of a table's quantity and threshold rows.
const QUANTITY_SUFFIX: &str = "_Qty";
const THRESHOLD_SUFFIX: &str = "_Probs";
/// An entry that rolls another loot table.
const TABLE_PREFIX: &str = "[LTID]";
/// An entry that drops items of a loot bucket.
const BUCKET_PREFIX: &str = "[LBID]";
/// The quantity range separator (`1-2`).
const RANGE_SEPARATOR: char = '-';
/// The `AndOr` value of a table that drops every entry its roll reaches.
const AND: &str = "AND";
/// The conditions whose level is a table's roll.
const ENEMY_LEVEL: &str = "EnemyLevel";
const CONTENT_LEVEL: &str = "MinContLevel";
/// How deep `[LTID]` entries may nest (retail tables nest 3 deep).
const MAX_DEPTH: u8 = 8;
/// The `RowPlaceholders` value of the bucket sheet row that names each column's bucket.
const BUCKET_NAMES_ROW: &str = "FIRSTROW";

/// The levels a loot roll reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LootLevels {
    /// The level of the mob killed or skinned (`EnemyLevel`).
    pub enemy: u32,
    /// The level of the player (`MinContLevel`).
    pub content: u32,
}

/// One entry of a loot table: its item, table or bucket, quantity and threshold.
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
    /// The player's level.
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

/// One item of a loot bucket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BucketItem {
    /// The bucket name (`LootBucketN` of the sheet's first row).
    pub bucket: String,
    /// `ItemN`.
    pub item: String,
    /// `QuantityN`: `a` or `a-b`.
    pub quantity: Option<String>,
}

/// What [`LootTables`] are built from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootSource {
    /// The loot table rows.
    pub rows: Vec<LootRow>,
    /// The bucket items.
    pub bucket_items: Vec<BucketItem>,
}

impl LootSource {
    /// The loot rows and bucket items of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let rows = tables
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
            .collect();
        // Each bucket sheet names its columns' buckets in its own first row.
        let mut bucket_items = Vec::new();
        for table in &tables.loot_bucket_data {
            let names: BTreeMap<u32, &str> = table
                .rows
                .iter()
                .filter(|row| row.row_placeholders == BUCKET_NAMES_ROW)
                .flat_map(|row| &row.loot_bucket)
                .map(|(column, name)| (*column, name.as_str()))
                .collect();
            for row in table
                .rows
                .iter()
                .filter(|row| row.row_placeholders != BUCKET_NAMES_ROW)
            {
                for (column, item) in &row.item {
                    if let Some(bucket) = names.get(column) {
                        bucket_items.push(BucketItem {
                            bucket: (*bucket).to_owned(),
                            item: item.clone(),
                            quantity: row.quantity.get(column).cloned(),
                        });
                    }
                }
            }
        }
        Self { rows, bucket_items }
    }
}

/// A quantity `a` or `a-b`; `None` for another text.
fn quantity(text: &str) -> Option<(u32, u32)> {
    match text.split_once(RANGE_SEPARATOR) {
        Some((low, high)) => Some((low.trim().parse().ok()?, high.trim().parse().ok()?)),
        None => text.trim().parse().ok().map(|count| (count, count)),
    }
}

/// The items of one bucket, each with its quantity range.
type Bucket = Vec<(String, (u32, u32))>;

/// Every loot table and bucket, by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootTables {
    tables: HashMap<String, LootTable>,
    buckets: HashMap<String, Bucket>,
}

impl LootTables {
    /// The loot tables and buckets of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(LootSource::from_tables(tables))
    }

    /// The loot tables and buckets of `source`.
    pub fn build(source: LootSource) -> Self {
        let rows: HashMap<&str, &LootRow> = source
            .rows
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
        let mut buckets: HashMap<String, Bucket> = HashMap::new();
        for item in source.bucket_items {
            let count = item
                .quantity
                .as_deref()
                .and_then(quantity)
                .unwrap_or((1, 1));
            buckets
                .entry(item.bucket)
                .or_default()
                .push((item.item, count));
        }
        Self { tables, buckets }
    }

    /// Whether table `id` exists.
    pub fn contains(&self, id: &str) -> bool {
        self.tables.contains_key(id)
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
            pick(&best, below).copied().into_iter().collect()
        };
        for entry in dropped {
            let count = count_of(entry.quantity, below);
            if let Some(inner) = entry.item.strip_prefix(TABLE_PREFIX) {
                for _ in 0..count {
                    self.roll_into(inner, levels, below, depth + 1, drops);
                }
            } else if let Some(bucket) = entry.item.strip_prefix(BUCKET_PREFIX) {
                let items = self.buckets.get(bucket).map_or(&[][..], Vec::as_slice);
                for _ in 0..count {
                    if let Some((item, range)) = pick(items, below) {
                        let amount = count_of(*range, below);
                        if amount > 0 {
                            *drops.entry(item.clone()).or_default() += amount;
                        }
                    }
                }
            } else if count > 0 {
                *drops.entry(entry.item.clone()).or_default() += count;
            }
        }
    }
}

/// A number from `low` to `high` of `(low, high)`.
fn count_of((low, high): (u32, u32), below: &mut impl FnMut(u32) -> u32) -> u32 {
    low + below(high.saturating_sub(low).saturating_add(1))
}

/// One of `choices` with equal chance; `None` for none.
fn pick<'a, T>(choices: &'a [T], below: &mut impl FnMut(u32) -> u32) -> Option<&'a T> {
    let count = u32::try_from(choices.len()).unwrap_or(u32::MAX);
    if count == 0 {
        return None;
    }
    usize::try_from(below(count))
        .ok()
        .and_then(|index| choices.get(index))
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
    fn retail_rows() -> Vec<LootRow> {
        vec![
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
        ]
    }

    fn tables(mut rows: Vec<LootRow>, bucket_items: Vec<BucketItem>) -> LootTables {
        rows.extend(retail_rows());
        LootTables::build(LootSource { rows, bucket_items })
    }

    const LEVELS: LootLevels = LootLevels {
        enemy: 3,
        content: 1,
    };

    #[test]
    fn flint_always_gives_one_flint() {
        let tables = tables(Vec::new(), Vec::new());
        for seed in 0..100_u32 {
            let mut below = |n: u32| seed.wrapping_mul(7_919) % n;
            assert_eq!(
                tables.roll("SingleStone", LEVELS, &mut below),
                [("FlintT1".to_owned(), 1)],
                "the bait table is not loaded: it drops nothing"
            );
        }
    }

    #[test]
    fn a_boar_gives_seven_level_rolls_of_rawhide_and_pork() {
        let tables = tables(Vec::new(), Vec::new());
        let mut lowest = |_: u32| 0;
        assert_eq!(
            tables.roll("BoarSkinning", LEVELS, &mut lowest),
            [("PorkT1".to_owned(), 1), ("RawhideT1".to_owned(), 7)]
        );
        let mut highest = |n: u32| n - 1;
        let drops = tables.roll("BoarSkinning", LEVELS, &mut highest);
        assert_eq!(
            drops,
            [("PorkT1".to_owned(), 2), ("RawhideT1".to_owned(), 14)],
            "level 3 reaches the threshold-2 row, never the level-32 RawhideT4"
        );
    }

    #[test]
    fn a_bucket_entry_drops_one_of_its_items_per_count() {
        let rows = vec![
            loot(
                "Chest",
                &[(1, "[LBID]Gems"), (2, "[LBID]Empty")],
                Some("AND"),
                None,
            ),
            loot("Chest_Qty", &[(1, "2"), (2, "1")], None, None),
            probs("Chest_Probs", &[(1, "0"), (2, "0")], 100),
        ];
        let gem = |item: &str, quantity: &str| BucketItem {
            bucket: "Gems".to_owned(),
            item: item.to_owned(),
            quantity: Some(quantity.to_owned()),
        };
        let tables = tables(rows, vec![gem("RubyT1", "1"), gem("OpalT1", "3")]);
        let mut first = |_: u32| 0;
        assert_eq!(
            tables.roll("Chest", LEVELS, &mut first),
            [("RubyT1".to_owned(), 2)]
        );
        let mut last = |n: u32| n - 1;
        assert_eq!(
            tables.roll("Chest", LEVELS, &mut last),
            [("OpalT1".to_owned(), 6)]
        );
    }

    #[test]
    fn a_table_that_rolls_itself_stops() {
        let rows = vec![
            loot(
                "Loop",
                &[(1, "[LTID]Loop"), (2, "Stick")],
                Some("AND"),
                None,
            ),
            loot("Loop_Qty", &[(1, "1"), (2, "1")], None, None),
            probs("Loop_Probs", &[(1, "0"), (2, "0")], 100),
        ];
        let tables = tables(rows, Vec::new());
        let mut first = |_: u32| 0;
        assert_eq!(
            tables.roll("Loop", LEVELS, &mut first),
            [("Stick".to_owned(), u32::from(MAX_DEPTH))]
        );
    }

    #[test]
    fn quantities_read_a_count_or_a_range() {
        assert_eq!(quantity("1"), Some((1, 1)));
        assert_eq!(quantity("2-3"), Some((2, 3)));
        assert_eq!(quantity("x"), None);
    }
}
