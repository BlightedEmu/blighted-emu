// SPDX-License-Identifier: AGPL-3.0-or-later
//! Loot tables and their rolls (gather note 2026-09-18, admin studio spec section 6.2, loot note
//! 2026-09-18), for gathering, mob kills and the studio's drop simulator.
//!
//! A loot table `T` is three rows: `T` (the entries, `AND` or `OR`, the conditions), `T_Qty`
//! (each entry's quantity, `a` or `a-b`) and `T_Probs` (each entry's threshold, with `MaxRoll`).
//! An `AND` table rolls each entry and drops it when the roll reaches its threshold; an `OR`
//! table rolls once and drops the entry with the highest threshold the roll reaches. An entry
//! `[LTID]X` rolls table `X` quantity times; an entry `[LBID]X` drops quantity items of loot
//! bucket `X`, each picked with equal chance among the bucket items whose tags hold (the bucket
//! odds columns are not read: Unknown).
//!
//! `Conditions` is a comma list. A level name makes the table roll that level instead of a
//! random number: `EnemyLevel` the killed mob's, `MinContLevel` the content level, `Level` the
//! player's, `MinPOIContLevel` the point of interest's (nwdb.info shows them as "Enemy Level",
//! `{!clvl}`, `{!plvl}` and "POI Level", one above the data: `MinContLevel:0-18` is
//! `{!clvl}1-19`, `Level:64` is `{!plvl}65+`, the threshold 0 of an `EnemyLevel` table is
//! "Enemy Level 1+"; so levels are compared 0-based, see [`loot_level`]). `GlobalMod` rolls
//! plainly (its bonus is Unknown). `[LIM]X` names loot limit `X`: the table rolls only while the
//! player's count of `X` is below its limit, and a table with `TriggerLimitOnVisit` counts a
//! visit on `X` each time it rolls (Inferred, see [`crate::loot_limits`]; 38 tables of build
//! 25810808). Any other name is a tag the roll must hold (Inferred: the
//! creature tables `CreatureLootCommon` and `CreatureLootElite` have `Common` and `Elite`, mob
//! `LootTags`). A roll holds a tag when the mob has it or a table on the roll path has that id
//! (7,517 bucket tags of build 25810808 name a loot table).
//!
//! A bucket item's `Tags` are a comma list of plain tags and level ranges `Key:a-b` (from `a` to
//! `b`) or `Key:a` (`a` and up, nwdb `{!plvl}65+` for `Level:64`); a key that is not a level
//! name never holds (Unknown: `HWMLoot*`, `DawnLevel`). An item without tags always drops; with
//! `MatchOne` one tag that holds is enough, else every tag must hold (nwdb.info `bmo`).

use std::collections::{BTreeMap, HashMap};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The suffixes of a table's quantity and threshold rows.
const QUANTITY_SUFFIX: &str = "_Qty";
const THRESHOLD_SUFFIX: &str = "_Probs";
/// An entry that rolls another loot table.
const TABLE_PREFIX: &str = "[LTID]";
/// An entry that drops items of a loot bucket.
const BUCKET_PREFIX: &str = "[LBID]";
/// The quantity and level range separator (`1-2`).
const RANGE_SEPARATOR: char = '-';
/// Separates a level tag's key from its range (`MinContLevel:0-18`).
const KEY_SEPARATOR: char = ':';
/// Separates the names of a `Conditions`, `Tags` or `LootTags` cell.
const LIST_SEPARATOR: char = ',';
/// The `AndOr` value of a table that drops every entry its roll reaches.
const AND: &str = "AND";
/// The condition of a table whose roll gets a global loot bonus (Inferred from its name; the
/// creature master table has it). The server adds no bonus, so the roll is a plain one.
const GLOBAL_MOD: &str = "GlobalMod";
/// The prefix of a condition that names a loot limit.
const LIMIT_PREFIX: &str = "[LIM]";
/// The `TriggerLimitOnVisit` values that count a visit (the build has `TRUE` and `1`).
const TRIGGER_VALUES: [&str; 2] = ["TRUE", "1"];
/// The level names of conditions and bucket tags.
const ENEMY_LEVEL: &str = "EnemyLevel";
const CONTENT_LEVEL: &str = "MinContLevel";
const PLAYER_LEVEL: &str = "Level";
const POI_LEVEL: &str = "MinPOIContLevel";
/// How deep `[LTID]` entries may nest (retail tables nest 3 deep).
const MAX_DEPTH: u8 = 8;
/// Most table rolls and bucket picks of one roll: a design table that rolls itself with a large
/// quantity would otherwise run for ever (quantity to the power of [`MAX_DEPTH`]).
pub const MAX_ROLL_STEPS: u32 = 10_000;
/// The `RowPlaceholders` value of the bucket sheet row that names each column's bucket.
const BUCKET_NAMES_ROW: &str = "FIRSTROW";

/// The loot value of a shown level (a mob's `Level`, 1 and up): the data compares levels from 0
/// (module notes).
pub const fn loot_level(level: u32) -> u32 {
    level.saturating_sub(1)
}

/// What a loot roll reads: the loot values of the levels (0-based, see [`loot_level`]; `None`
/// for a level not known here, whose conditions never hold) and the loot tags of the mob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LootContext<'a> {
    /// The killed or skinned mob's level (`EnemyLevel`).
    pub enemy: Option<u32>,
    /// The content level (`MinContLevel`).
    pub content: Option<u32>,
    /// The player's level (`Level`): its `ExperienceData.LootTagValue`.
    pub player: Option<u32>,
    /// The point of interest's level (`MinPOIContLevel`).
    pub poi: Option<u32>,
    /// The mob's loot tags (`LootTags`); none for a gather.
    pub tags: &'a [String],
    /// The lowercase ids of the player's loot limits at their limit: their `[LIM]` tables do
    /// not roll.
    pub reached_limits: &'a [String],
}

/// What one roll drops, and the loot limits its tables count a visit on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LootRoll {
    /// The items, merged by item.
    pub items: Vec<(String, u32)>,
    /// The ids of the limits of the `TriggerLimitOnVisit` tables it rolled, once per roll of
    /// each such table.
    pub limit_visits: Vec<String>,
}

/// Which level a condition or a bucket tag reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LevelKind {
    Enemy,
    Content,
    Player,
    Poi,
}

impl LevelKind {
    /// The level that `name` names, in any case.
    fn of(name: &str) -> Option<Self> {
        [
            (ENEMY_LEVEL, Self::Enemy),
            (CONTENT_LEVEL, Self::Content),
            (PLAYER_LEVEL, Self::Player),
            (POI_LEVEL, Self::Poi),
        ]
        .into_iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, kind)| kind)
    }
}

impl LootContext<'_> {
    fn level(&self, kind: LevelKind) -> Option<u32> {
        match kind {
            LevelKind::Enemy => self.enemy,
            LevelKind::Content => self.content,
            LevelKind::Player => self.player,
            LevelKind::Poi => self.poi,
        }
    }
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
    /// A level.
    Level(LevelKind),
}

/// One loot table.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LootTable {
    and: bool,
    roll: Roll,
    /// The tags the roll must hold, lowercase.
    required: Vec<String>,
    /// The loot limit of its `[LIM]` condition, lowercase.
    limit: Option<String>,
    /// Whether each roll of it counts a visit on its limit (`TriggerLimitOnVisit`).
    counts_visit: bool,
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
    /// `TriggerLimitOnVisit` is `TRUE` or `1`.
    pub trigger_limit_on_visit: bool,
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
    /// `TagsN`.
    pub tags: Option<String>,
    /// `MatchOneN`.
    pub match_one: bool,
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
                trigger_limit_on_visit: counts_visits(row.trigger_limit_on_visit.as_deref()),
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
                            tags: row.tags.get(column).cloned(),
                            match_one: row.match_one.get(column).copied().unwrap_or_default(),
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

/// Whether a `TriggerLimitOnVisit` cell counts visits.
fn counts_visits(cell: Option<&str>) -> bool {
    cell.is_some_and(|value| {
        TRIGGER_VALUES
            .iter()
            .any(|truth| truth.eq_ignore_ascii_case(value.trim()))
    })
}

/// The names of a comma list cell, trimmed, without empty ones.
fn names(cell: Option<&str>) -> impl Iterator<Item = &str> {
    cell.unwrap_or_default()
        .split(LIST_SEPARATOR)
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

/// One tag of a bucket item.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BucketTag {
    /// A level from the first to the second (no end: and up).
    Level(LevelKind, u32, Option<u32>),
    /// A tag the roll must hold, lowercase.
    Plain(String),
    /// A key this server does not know, or a range it cannot read: it never holds.
    Never,
}

impl BucketTag {
    fn of(text: &str) -> Self {
        let Some((key, range)) = text.split_once(KEY_SEPARATOR) else {
            return Self::Plain(text.to_lowercase());
        };
        let Some(kind) = LevelKind::of(key.trim()) else {
            return Self::Never;
        };
        let bounds = match range.split_once(RANGE_SEPARATOR) {
            Some((low, high)) => low
                .trim()
                .parse()
                .ok()
                .zip(high.trim().parse().ok())
                .map(|(low, high)| (low, Some(high))),
            None => range.trim().parse().ok().map(|low| (low, None)),
        };
        bounds.map_or(Self::Never, |(low, high)| Self::Level(kind, low, high))
    }
}

/// One item of a bucket: its quantity range and its tags.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BucketEntry {
    item: String,
    quantity: (u32, u32),
    tags: Vec<BucketTag>,
    match_one: bool,
}

/// What a roll holds while it walks: the context and the ids of the tables on its path.
struct Holds<'a, 'b> {
    context: LootContext<'a>,
    path: &'b [String],
}

impl Holds<'_, '_> {
    /// Whether the roll holds `tag` (lowercase).
    fn tag(&self, tag: &str) -> bool {
        self.context
            .tags
            .iter()
            .any(|own| own.eq_ignore_ascii_case(tag))
            || self.path.iter().any(|table| table == tag)
    }

    fn bucket_tag(&self, tag: &BucketTag) -> bool {
        match tag {
            BucketTag::Level(kind, low, high) => self
                .context
                .level(*kind)
                .is_some_and(|level| level >= *low && high.is_none_or(|high| level <= high)),
            BucketTag::Plain(tag) => self.tag(tag),
            BucketTag::Never => false,
        }
    }

    /// Whether bucket item `entry` may drop.
    fn bucket_entry(&self, entry: &BucketEntry) -> bool {
        if entry.tags.is_empty() {
            true
        } else if entry.match_one {
            entry.tags.iter().any(|tag| self.bucket_tag(tag))
        } else {
            entry.tags.iter().all(|tag| self.bucket_tag(tag))
        }
    }
}

/// Every loot table and bucket, by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootTables {
    tables: HashMap<String, LootTable>,
    buckets: HashMap<String, Vec<BucketEntry>>,
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
                let mut roll = Roll::Random(max_roll);
                let mut required = Vec::new();
                let mut limit = None;
                for name in names(row.conditions.as_deref()) {
                    if let Some(id) = name.strip_prefix(LIMIT_PREFIX) {
                        // No retail table names two limits.
                        limit = Some(id.trim().to_lowercase());
                        continue;
                    }
                    match LevelKind::of(name) {
                        // The first level name gives the roll (no retail table has two).
                        Some(kind) if roll == Roll::Random(max_roll) => roll = Roll::Level(kind),
                        Some(_) => {}
                        None if name.eq_ignore_ascii_case(GLOBAL_MOD) => {}
                        None => required.push(name.to_lowercase()),
                    }
                }
                (
                    (*id).to_owned(),
                    LootTable {
                        and: row.and_or.as_deref() == Some(AND),
                        roll,
                        required,
                        counts_visit: row.trigger_limit_on_visit && limit.is_some(),
                        limit,
                        entries,
                    },
                )
            })
            .collect();
        let mut buckets: HashMap<String, Vec<BucketEntry>> = HashMap::new();
        for item in source.bucket_items {
            let entry = BucketEntry {
                quantity: item
                    .quantity
                    .as_deref()
                    .and_then(quantity)
                    .unwrap_or((1, 1)),
                tags: names(item.tags.as_deref()).map(BucketTag::of).collect(),
                match_one: item.match_one,
                item: item.item,
            };
            buckets.entry(item.bucket).or_default().push(entry);
        }
        Self { tables, buckets }
    }

    /// Whether table `id` exists.
    pub fn contains(&self, id: &str) -> bool {
        self.tables.contains_key(id)
    }

    /// What loot table `table` drops in `context` and the limit visits it counts, with
    /// `below(n)` a random number below `n`.
    pub fn roll(
        &self,
        table: &str,
        context: LootContext<'_>,
        below: &mut impl FnMut(u32) -> u32,
    ) -> LootRoll {
        let mut drops: BTreeMap<String, u32> = BTreeMap::new();
        let mut walk = Walk {
            context,
            below,
            steps: 0,
            path: Vec::new(),
            drops: &mut drops,
            limit_visits: Vec::new(),
        };
        walk.table(self, table);
        let limit_visits = walk.limit_visits;
        LootRoll {
            items: drops.into_iter().collect(),
            limit_visits,
        }
    }
}

/// One roll as it walks the tables.
struct Walk<'a, 'b, F> {
    context: LootContext<'a>,
    below: &'b mut F,
    steps: u32,
    /// The lowercase ids of the tables on the path, outermost first.
    path: Vec<String>,
    drops: &'b mut BTreeMap<String, u32>,
    limit_visits: Vec<String>,
}

impl<F: FnMut(u32) -> u32> Walk<'_, '_, F> {
    fn table(&mut self, tables: &LootTables, id: &str) {
        let Some(table) = tables
            .tables
            .get(id)
            .filter(|_| self.path.len() < usize::from(MAX_DEPTH) && self.steps < MAX_ROLL_STEPS)
        else {
            return;
        };
        self.steps += 1;
        self.path.push(id.to_lowercase());
        let holds = Holds {
            context: self.context,
            path: &self.path,
        };
        let below_limit = table
            .limit
            .as_ref()
            .is_none_or(|limit| !self.context.reached_limits.contains(limit));
        if below_limit && table.required.iter().all(|tag| holds.tag(tag)) {
            if let Some(limit) = table.limit.as_ref().filter(|_| table.counts_visit) {
                self.limit_visits.push(limit.clone());
            }
            self.entries(tables, table);
        }
        self.path.pop();
    }

    /// The roll of `table`: `None` when its level is not known.
    fn draw(&mut self, table: &LootTable) -> Option<u32> {
        match table.roll {
            Roll::Random(max) => Some((self.below)(max.max(1))),
            Roll::Level(kind) => self.context.level(kind),
        }
    }

    fn entries(&mut self, tables: &LootTables, table: &LootTable) {
        let dropped: Vec<&LootEntry> = if table.and {
            table
                .entries
                .iter()
                .filter(|entry| self.draw(table).is_some_and(|roll| roll >= entry.threshold))
                .collect()
        } else {
            let Some(roll) = self.draw(table) else {
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
            pick(&best, self.below).copied().into_iter().collect()
        };
        for entry in dropped {
            let count = count_of(entry.quantity, self.below);
            if let Some(inner) = entry.item.strip_prefix(TABLE_PREFIX) {
                for _ in 0..count {
                    if self.steps >= MAX_ROLL_STEPS {
                        break;
                    }
                    self.table(tables, inner);
                }
            } else if let Some(bucket) = entry.item.strip_prefix(BUCKET_PREFIX) {
                self.bucket(tables, bucket, count);
            } else {
                self.drop(&entry.item, count);
            }
        }
    }

    /// `count` picks of bucket `bucket` among its items that may drop.
    fn bucket(&mut self, tables: &LootTables, bucket: &str, count: u32) {
        let holds = Holds {
            context: self.context,
            path: &self.path,
        };
        let items: Vec<&BucketEntry> = tables
            .buckets
            .get(bucket)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter(|entry| holds.bucket_entry(entry))
            .collect();
        for _ in 0..count {
            if self.steps >= MAX_ROLL_STEPS {
                break;
            }
            self.steps += 1;
            if let Some(entry) = pick(&items, self.below) {
                let amount = count_of(entry.quantity, self.below);
                self.drop(&entry.item, amount);
            }
        }
    }

    fn drop(&mut self, item: &str, count: u32) {
        if count > 0 {
            let total = self.drops.entry(item.to_owned()).or_default();
            *total = total.saturating_add(count);
        }
    }
}

/// The loot value of the player's level for its experience (`ExperienceData`): the
/// `LootTagValue` of the highest level whose `XPToLevel` (the total experience it takes, nwdb.info
/// `xp_total`) the experience reaches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerLootLevels {
    /// (`XPToLevel`, `LootTagValue`), by experience.
    levels: Vec<(u64, u32)>,
}

impl PlayerLootLevels {
    /// The levels of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .experience_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    Some((
                        u64::try_from(row.xp_to_level).ok()?,
                        u32::try_from(row.loot_tag_value).ok()?,
                    ))
                }),
        )
    }

    /// The levels of (`XPToLevel`, `LootTagValue`) pairs.
    pub fn build(levels: impl IntoIterator<Item = (u64, u32)>) -> Self {
        let mut levels: Vec<(u64, u32)> = levels.into_iter().collect();
        levels.sort_unstable();
        Self { levels }
    }

    /// The loot value of the level of a player with `experience` points; `None` without levels.
    pub fn of(&self, experience: u64) -> Option<u32> {
        self.levels
            .iter()
            .take_while(|(total, _)| *total <= experience)
            .last()
            .map(|(_, value)| *value)
    }
}

/// What a killed mob type rolls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobDrop {
    /// `LootTableId`.
    pub table: String,
    /// `LootTags`: the conditions of the tables this mob's loot rolls.
    pub tags: Vec<String>,
}

/// One mob type's loot cells.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MobLootRow {
    /// `VitalsID`.
    pub vitals_id: String,
    /// `LootTableId`.
    pub table: Option<String>,
    /// `LootTags`.
    pub tags: Option<String>,
}

/// The drop of every mob type by the CRC-32 of its lowercase `VitalsID`.
///
/// Retail rolls the table of every kill (loot note 2026-09-18, 695 ledgers): own kills of mob
/// types with `CreatureLootMaster` left a loot bag for 1,211 of 1,712 at `LootDropChance` 0.1, 232
/// of 311 at 0.05 and 26 of 46 at 1, so the chance does not gate the drop; types with
/// `CanDropLoot` false and a table left bags too (51 of 74), and types without a table none (0 of
/// 80). What `LootDropChance` and `CanDropLoot` do is Unknown; neither is read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MobLoot {
    by_crc: HashMap<u32, MobDrop>,
}

impl MobLoot {
    /// The mob drops of `tables`: the mob types of `VitalsBaseData`, then the few of
    /// `VitalsData`.
    pub fn from_tables(tables: &Tables) -> Self {
        let base = tables
            .vitals_base_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| MobLootRow {
                vitals_id: row.vitals_id.clone(),
                table: row.loot_table_id.clone(),
                tags: row.loot_tags.clone(),
            });
        let other = tables
            .vitals_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| MobLootRow {
                vitals_id: row.vitals_id.clone(),
                table: row.loot_table_id.clone(),
                tags: None,
            });
        Self::build(base.chain(other))
    }

    /// The drops of `rows`; a row without a table drops nothing, and the first row of a vitals
    /// id wins.
    pub fn build(rows: impl IntoIterator<Item = MobLootRow>) -> Self {
        let mut by_crc = HashMap::new();
        for row in rows {
            let Some(table) = row.table.filter(|table| !table.trim().is_empty()) else {
                continue;
            };
            let tags = names(row.tags.as_deref()).map(str::to_owned).collect();
            by_crc
                .entry(lowercase_crc32(&row.vitals_id))
                .or_insert(MobDrop { table, tags });
        }
        Self { by_crc }
    }

    /// The drop of the mob type whose lowercase id has the CRC-32 `vitals_crc`.
    pub fn of(&self, vitals_crc: u32) -> Option<&MobDrop> {
        self.by_crc.get(&vitals_crc)
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
            trigger_limit_on_visit: false,
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

    /// A table `id` of one always-dropping entry `item` with `conditions`.
    fn single(id: &str, item: &str, and_or: &str, conditions: Option<&str>) -> Vec<LootRow> {
        vec![
            loot(id, &[(1, item)], Some(and_or), conditions),
            loot(&format!("{id}_Qty"), &[(1, "1")], None, None),
            probs(&format!("{id}_Probs"), &[(1, "0")], 100),
        ]
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

    /// A level-3 mob (loot value 2), a level-1 player, no content or POI level, no tags.
    const LEVELS: LootContext<'static> = LootContext {
        enemy: Some(loot_level(3)),
        content: None,
        player: Some(0),
        poi: None,
        tags: &[],
        reached_limits: &[],
    };

    fn bucket_item(bucket: &str, item: &str, tags: &str, match_one: bool) -> BucketItem {
        BucketItem {
            bucket: bucket.to_owned(),
            item: item.to_owned(),
            quantity: Some("1".to_owned()),
            tags: Some(tags.to_owned()).filter(|tags| !tags.is_empty()),
            match_one,
        }
    }

    #[test]
    fn flint_always_gives_one_flint() {
        let tables = tables(Vec::new(), Vec::new());
        for seed in 0..100_u32 {
            let mut below = |n: u32| seed.wrapping_mul(7_919) % n;
            assert_eq!(
                tables.roll("SingleStone", LEVELS, &mut below).items,
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
            tables.roll("BoarSkinning", LEVELS, &mut lowest).items,
            [("PorkT1".to_owned(), 1), ("RawhideT1".to_owned(), 7)]
        );
        let mut highest = |n: u32| n - 1;
        let drops = tables.roll("BoarSkinning", LEVELS, &mut highest).items;
        assert_eq!(
            drops,
            [("PorkT1".to_owned(), 2), ("RawhideT1".to_owned(), 14)],
            "level 3 (value 2) reaches the threshold-2 row, never the threshold-32 RawhideT4"
        );
        let unknown_level = LootContext {
            enemy: None,
            ..LEVELS
        };
        assert_eq!(
            tables
                .roll("BoarSkinning", unknown_level, &mut lowest)
                .items,
            [("PorkT1".to_owned(), 1)],
            "a level table rolls nothing without its level"
        );
    }

    #[test]
    fn levels_are_compared_from_zero() {
        assert_eq!(loot_level(1), 0);
        assert_eq!(loot_level(65), 64);
        assert_eq!(loot_level(0), 0);
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
            quantity: Some(quantity.to_owned()),
            ..bucket_item("Gems", item, "", false)
        };
        let tables = tables(rows, vec![gem("RubyT1", "1"), gem("OpalT1", "3")]);
        let mut first = |_: u32| 0;
        assert_eq!(
            tables.roll("Chest", LEVELS, &mut first).items,
            [("RubyT1".to_owned(), 2)]
        );
        let mut last = |n: u32| n - 1;
        assert_eq!(
            tables.roll("Chest", LEVELS, &mut last).items,
            [("OpalT1".to_owned(), 6)]
        );
    }

    #[test]
    fn conditions_split_on_commas_into_a_level_roll_and_tags() {
        // Retail `EnemyLevel,Elite` (4 tables) and `GlobalMod` (25).
        let mut rows = single("EliteGear", "Sword", "OR", Some("EnemyLevel, Elite"));
        rows.extend(single("Master", "Bone", "AND", Some("GlobalMod")));
        rows.extend(single("Odd", "Bone", "AND", Some("SomethingElse")));
        let tables = tables(rows, Vec::new());
        let mut first = |_: u32| 0;
        assert_eq!(
            tables.roll("Master", LEVELS, &mut first).items,
            [("Bone".to_owned(), 1)]
        );
        assert!(tables.roll("Odd", LEVELS, &mut first).items.is_empty());
        assert!(
            tables
                .roll("EliteGear", LEVELS, &mut first)
                .items
                .is_empty(),
            "a common mob does not hold Elite"
        );
        let elite = [String::from("Corrupted"), String::from("elite")];
        let mob = LootContext {
            tags: &elite,
            ..LEVELS
        };
        assert_eq!(
            tables.roll("EliteGear", mob, &mut first).items,
            [("Sword".to_owned(), 1)]
        );
        let unknown_level = LootContext { enemy: None, ..mob };
        assert!(
            tables
                .roll("EliteGear", unknown_level, &mut first)
                .items
                .is_empty()
        );
    }

    #[test]
    fn each_level_name_rolls_its_own_level() {
        // A threshold-5 entry over a threshold-0 entry: a level of 5 or more reaches the first.
        let level_rows = |id: &str, condition: &str| {
            vec![
                loot(id, &[(1, "Low"), (2, "High")], Some("OR"), Some(condition)),
                loot(&format!("{id}_Qty"), &[(1, "1"), (2, "1")], None, None),
                probs(&format!("{id}_Probs"), &[(1, "0"), (2, "5")], 0),
            ]
        };
        let mut rows = level_rows("ByEnemy", "EnemyLevel");
        rows.extend(level_rows("ByContent", "MinContLevel"));
        rows.extend(level_rows("ByPlayer", "Level"));
        rows.extend(level_rows("ByPoi", "MinPOIContLevel"));
        let tables = tables(rows, Vec::new());
        let context = LootContext {
            enemy: Some(5),
            content: Some(4),
            player: Some(9),
            poi: Some(0),
            tags: &[],
            reached_limits: &[],
        };
        let mut first = |_: u32| 0;
        let roll = |table: &str, below: &mut dyn FnMut(u32) -> u32| {
            tables.roll(table, context, &mut |n| below(n)).items
        };
        assert_eq!(roll("ByEnemy", &mut first), [("High".to_owned(), 1)]);
        assert_eq!(roll("ByContent", &mut first), [("Low".to_owned(), 1)]);
        assert_eq!(roll("ByPlayer", &mut first), [("High".to_owned(), 1)]);
        assert_eq!(roll("ByPoi", &mut first), [("Low".to_owned(), 1)]);
    }

    #[test]
    fn bucket_tags_read_level_ranges_plain_tags_and_match_one() {
        let rows = single("Weapons", "[LBID]ProcWeapons", "AND", None);
        // Retail ProcWeapons and FamilyWeapons cells.
        let items = vec![
            bucket_item("ProcWeapons", "SwordT2", "MinContLevel:0-18", false),
            bucket_item("ProcWeapons", "SwordT3", "MinContLevel:19-38", false),
            bucket_item("ProcWeapons", "SwordT5", "Level:64", false),
            bucket_item(
                "ProcWeapons",
                "BeastSword",
                "Beast,MinContLevel:0-18",
                false,
            ),
            bucket_item("ProcWeapons", "Either", "Beast,Elite", true),
            bucket_item("ProcWeapons", "HighWater", "HWMLootSword:600-600", false),
            bucket_item("ProcWeapons", "BadRange", "MinContLevel:x-y", false),
            bucket_item("ProcWeapons", "PathTagged", "weapons", false),
        ];
        /// The draw of a roll that picks the bucket item.
        const PICK_DRAW: u32 = 3;
        let tables = tables(rows, items);
        let eligible = |context: LootContext<'_>| -> Vec<String> {
            // Each pick index in turn lists the eligible items in bucket order; an index past
            // the eligible items picks none. The third draw of a roll is the pick (the table's
            // roll, the entry's count, the pick, the item's quantity).
            (0..8_u32)
                .map_while(|index| {
                    let mut draws = 0;
                    let mut below = |n: u32| {
                        draws += 1;
                        if draws == PICK_DRAW { index.min(n) } else { 0 }
                    };
                    tables
                        .roll("Weapons", context, &mut below)
                        .items
                        .into_iter()
                        .next()
                        .map(|(item, _)| item)
                })
                .collect()
        };
        let low = LootContext {
            content: Some(18),
            ..LEVELS
        };
        assert_eq!(eligible(low), ["SwordT2", "PathTagged"]);
        let beast = [String::from("Beast")];
        let beast_low = LootContext {
            tags: &beast,
            ..low
        };
        assert_eq!(
            eligible(beast_low),
            ["SwordT2", "BeastSword", "Either", "PathTagged"]
        );
        let capped = LootContext {
            content: Some(19),
            player: Some(64),
            ..LEVELS
        };
        assert_eq!(eligible(capped), ["SwordT3", "SwordT5", "PathTagged"]);
        let no_content = LootContext {
            content: None,
            ..LEVELS
        };
        assert_eq!(eligible(no_content), ["PathTagged"]);
    }

    #[test]
    fn a_table_that_rolls_itself_many_times_stops_at_the_step_limit() {
        let rows = vec![
            loot(
                "Bomb",
                &[(1, "[LTID]Bomb"), (2, "Stick")],
                Some("AND"),
                None,
            ),
            loot("Bomb_Qty", &[(1, "1000"), (2, "4294967295")], None, None),
            probs("Bomb_Probs", &[(1, "0"), (2, "0")], 100),
        ];
        let tables = tables(rows, Vec::new());
        let mut first = |_: u32| 0;
        let drops = tables.roll("Bomb", LEVELS, &mut first).items;
        assert_eq!(drops, [("Stick".to_owned(), u32::MAX)], "the sum saturates");
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
            tables.roll("Loop", LEVELS, &mut first).items,
            [("Stick".to_owned(), u32::from(MAX_DEPTH))]
        );
    }

    #[test]
    fn a_mob_type_drops_its_table() {
        let row = |id: &str, table: Option<&str>| MobLootRow {
            vitals_id: id.to_owned(),
            table: table.map(str::to_owned),
            tags: None,
        };
        let loot = MobLoot::build([
            MobLootRow {
                tags: Some("Common, Wolf,".to_owned()),
                ..row("Wolf_Alpha", Some("WolfLoot"))
            },
            row("Boar", Some("BoarLoot")),
            row("Ghost", None),
            row("Blank", Some(" ")),
            row("Boar", Some("SecondBoarLoot")),
        ]);
        let of = |id: &str| loot.of(lowercase_crc32(id)).cloned();
        assert_eq!(
            of("wolf_alpha"),
            Some(MobDrop {
                table: "WolfLoot".to_owned(),
                tags: vec!["Common".to_owned(), "Wolf".to_owned()],
            })
        );
        assert_eq!(
            of("Boar").map(|drop| drop.table),
            Some("BoarLoot".to_owned())
        );
        assert_eq!(of("Ghost"), None, "no table: no loot");
        assert_eq!(of("Blank"), None);
    }

    #[test]
    fn a_players_loot_level_is_the_highest_level_its_experience_reaches() {
        // Retail XPLevels rows 0 to 3: (XPToLevel, LootTagValue).
        let levels = PlayerLootLevels::build([(480, 2), (0, 0), (150, 1), (1080, 3)]);
        assert_eq!(levels.of(0), Some(0));
        assert_eq!(levels.of(149), Some(0));
        assert_eq!(levels.of(150), Some(1));
        assert_eq!(levels.of(1_079), Some(2));
        assert_eq!(levels.of(u64::MAX), Some(3));
        assert_eq!(PlayerLootLevels::default().of(0), None);
    }

    #[test]
    fn quantities_read_a_count_or_a_range() {
        assert_eq!(quantity("1"), Some((1, 1)));
        assert_eq!(quantity("2-3"), Some((2, 3)));
        assert_eq!(quantity("x"), None);
    }

    #[test]
    fn the_trigger_cell_counts_visits_for_true_and_one() {
        assert!(counts_visits(Some("TRUE")));
        assert!(counts_visits(Some("1")));
        assert!(counts_visits(Some("true ")));
        assert!(!counts_visits(Some("FALSE")));
        assert!(!counts_visits(Some("0")));
        assert!(!counts_visits(None));
    }

    #[test]
    fn a_limit_condition_rolls_below_the_limit_and_counts_its_visits() {
        // The retail rows of `PvPGM725RewardLimit` (`[LIM]PvPGM725Gear_Limit`, trigger
        // `TRUE`) and of `LNY_Tribute_Reward` (`[LIM]LNY_GiftLimit_B`, trigger `1`), the
        // second rolled from a master table; a third without the trigger.
        let triggered = |id: &str, item: &str, limit: &str, trigger: bool| {
            let mut rows = single(id, item, "AND", Some(&format!("[LIM]{limit}")));
            rows[0].trigger_limit_on_visit = trigger;
            rows
        };
        let mut rows = triggered(
            "PvPGM725RewardLimit",
            "PvPGM725RewardContainer",
            "PvPGM725Gear_Limit",
            true,
        );
        rows.extend(triggered(
            "LNY_Tribute_Reward",
            "LNY_GiftSack_Bundle",
            "LNY_GiftLimit_B",
            true,
        ));
        rows.extend(triggered("Quiet", "Pebble", "Quiet_Limit", false));
        rows.extend(single("Master", "[LTID]LNY_Tribute_Reward", "AND", None));
        let tables = tables(rows, Vec::new());
        let mut first = |_: u32| 0;
        assert_eq!(
            tables.roll("PvPGM725RewardLimit", LEVELS, &mut first),
            LootRoll {
                items: vec![("PvPGM725RewardContainer".to_owned(), 1)],
                limit_visits: vec!["pvpgm725gear_limit".to_owned()],
            },
            "below the limit: the table rolls and counts a visit"
        );
        assert_eq!(
            tables.roll("Master", LEVELS, &mut first),
            LootRoll {
                items: vec![("LNY_GiftSack_Bundle".to_owned(), 1)],
                limit_visits: vec!["lny_giftlimit_b".to_owned()],
            },
            "an inner table counts its visit too"
        );
        assert_eq!(
            tables.roll("Quiet", LEVELS, &mut first),
            LootRoll {
                items: vec![("Pebble".to_owned(), 1)],
                limit_visits: Vec::new(),
            },
            "without the trigger a roll counts nothing"
        );
        let reached = [
            "pvpgm725gear_limit".to_owned(),
            "lny_giftlimit_b".to_owned(),
        ];
        let at_limit = LootContext {
            reached_limits: &reached,
            ..LEVELS
        };
        assert_eq!(
            tables.roll("PvPGM725RewardLimit", at_limit, &mut first),
            LootRoll::default(),
            "at the limit the table does not roll"
        );
        assert_eq!(
            tables.roll("Master", at_limit, &mut first),
            LootRoll::default()
        );
        let source = LootSource {
            rows: vec![LootRow {
                trigger_limit_on_visit: true,
                ..loot("NoLimit", &[(1, "Stone")], Some("AND"), None)
            }],
            bucket_items: Vec::new(),
        };
        assert_eq!(
            LootTables::build(source)
                .roll("NoLimit", LEVELS, &mut first)
                .limit_visits,
            Vec::<String>::new(),
            "a trigger without a [LIM] condition names no limit"
        );
    }
}
