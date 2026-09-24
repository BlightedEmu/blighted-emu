// SPDX-License-Identifier: AGPL-3.0-or-later
//! Item perks (run-15 items2 study): the client's wire perk index, the per-item perk swaps, the
//! slot order of an item's perks, the perk buckets that rolls pick from, and the perk count of
//! an item by its tier and gear score.
//!
//! **Wire index (Observed, client `FUN_14448f450`).** The client numbers perks from 1 (0 is an
//! empty slot): it walks the `PerkData` sheets in a fixed order ([`PERK_SHEET_ORDER`]) and, for
//! each, the list order of the hash map it loaded the sheet's rows into. A key is the CRC-32 of
//! the lowercase `PerkID`; the map (`FUN_1446b3080`, `FUN_1446e0480`) hashes a key to a bucket
//! as the sum of the two halves of its 128-bit product with [`HASH_MULTIPLIER`], keeps one
//! linked list where each bucket's keys stay together (a new key goes at the front, or after the
//! last key of its bucket), and grows to the next power of two when the keys outnumber the
//! buckets. The stored map is a copy (`FUN_14477e030`) that lists the buckets' groups from the
//! last bucket to the first. The model gives the retail perks of 26 current-build captures
//! (100 % of the fixed perks).
//!
//! **Swaps and order (Observed).** An item's `Perk1..5` go through `ItemPerkSwapData` until no
//! swap applies (`PerkID_Stat_TwoHandSentry` on a weapon is sent as
//! `PerkID_StatSwap_BaseDamage`). Retail fills the perk slots from slot 0 with no gaps, sorted by
//! descending CRC-32 of the lowercase perk id (140 of 141 multi-perk records).
//!
//! **Perk count (Observed, `playerbaseattributes.pbadb` `PerkGenerationData` → `Perk Data Per
//! Tier`, lookup `FUN_145c4e860`).** The count of an item is the highest count whose gear score
//! range holds the item's gear score; "General" and "Crafting" hold the same table.

use std::collections::{HashMap, HashSet};

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The `PerkData` sheets (their unique ids) in the order the client numbers their perks
/// (Observed: the only order that fits the perks of the current-build captures and the perk
/// `ItemClass` of 1950 of 1954 records).
pub const PERK_SHEET_ORDER: [&str; 7] = [
    "ItemPerks_Gems",
    "ItemPerks_Infix",
    "ItemPerks_Deprecated",
    "ItemPerks_Artifacts",
    "ItemPerks_2025",
    "ItemPerks_EquipmentSetBonuses",
    "ItemPerks",
];
/// Perk slots of an item.
pub const PERK_SLOTS: usize = 5;
/// The multiplier of the client's key hash (`FUN_1414a1220`).
const HASH_MULTIPLIER: u64 = 0xde5f_b9d2_6304_58e9;
/// Bits of the hash's halves.
const HASH_HALF_BITS: u32 = 64;
/// The smallest bucket count the client map grows to.
const MIN_GROWN_BUCKETS: u64 = 2;
/// Separates the parts of an `ItemClass` cell and of a perk's class and label cells.
const CLASS_SEPARATOR: char = '+';
/// A swap keyed by item id (`ItemPerkSwapData.KeyType`).
const KEY_TYPE_ITEM_ID: &str = "ItemId";
/// A swap keyed by item classes.
const KEY_TYPE_ITEM_CLASS: &str = "ItemClass";
/// A bucket entry that names another bucket: this prefix and its id.
const NESTED_BUCKET_PREFIX: &str = "[PBID]";
/// The suffix of the row that holds a bucket's entry weights.
const WEIGHTS_SUFFIX: &str = "_Weights";
/// The weight of a bucket entry without a weights row.
const PLAIN_WEIGHT: f64 = 1.0;
/// The chance of a bucket without `PerkChance`.
const PLAIN_CHANCE: f64 = 1.0;
/// Most nested buckets a pick follows (a cycle in the data stops there).
const MAX_BUCKET_DEPTH: usize = 8;
/// The steps of a random fraction drawn with `below`.
const FRACTION_STEPS: u32 = 1 << 24;
/// Rarity of an item by its perk count (`ItemRarityData` of `playerbaseattributes.pbadb`):
/// 0 perks common, 1-2 uncommon, 3 rare, 4 epic, 5 legendary.
const RARITY_BY_PERKS: [u32; PERK_SLOTS + 1] = [0, 1, 1, 2, 3, 4];

/// One gear score range of a perk count: inclusive bounds.
type PerkCountRange = (u32, u32, u32);

/// The perk count ranges of tiers 2 to 5 (`Perk Data Per Tier` elements 2 to 5): (lowest gear
/// score, highest gear score, perk count). Tiers 0 and 1 have none: their items roll no perks.
const PERK_COUNT_RANGES: [&[PerkCountRange]; 4] = [
    &[(0, 169, 0), (0, 169, 1), (170, 700, 2)],
    &[(0, 269, 0), (0, 269, 1), (270, 324, 2), (325, 700, 3)],
    &[
        (0, 369, 0),
        (0, 369, 1),
        (0, 369, 2),
        (370, 449, 3),
        (450, 700, 4),
    ],
    &[
        (0, 469, 0),
        (0, 469, 1),
        (0, 469, 2),
        (0, 469, 3),
        (470, 599, 4),
        (626, 674, 4),
        (600, 625, 5),
        (675, 1000, 5),
    ],
];
/// The first tier with a perk count table.
const FIRST_PERK_TIER: u32 = 2;

/// The perk count of an item of `tier` at `gear_score`: the highest count whose range holds
/// it; a gear score past every range takes the range nearest to it (`javelin.choose-next-valid-
/// range-for-perk-count`, best guess that it is set); 0 for a tier without a table.
pub fn perk_count(tier: u32, gear_score: u32) -> u32 {
    let Some(ranges) = tier
        .checked_sub(FIRST_PERK_TIER)
        .and_then(|index| PERK_COUNT_RANGES.get(usize::try_from(index).ok()?))
    else {
        return 0;
    };
    let held = ranges
        .iter()
        .filter(|(low, high, _)| (*low..=*high).contains(&gear_score))
        .map(|(_, _, count)| *count)
        .max();
    held.or_else(|| {
        ranges
            .iter()
            .min_by_key(|(low, high, _)| low.abs_diff(gear_score).min(high.abs_diff(gear_score)))
            .map(|(_, _, count)| *count)
    })
    .unwrap_or_default()
}

/// The rarity of an item with `perks` perks: 0 common to 4 legendary.
pub fn rarity(perks: usize) -> u32 {
    RARITY_BY_PERKS[perks.min(PERK_SLOTS)]
}

/// The client's bucket hash of `key`.
fn bucket_hash(key: u32) -> u64 {
    let product = u128::from(key) * u128::from(HASH_MULTIPLIER);
    // The low half: truncation intended.
    (product as u64).wrapping_add((product >> HASH_HALF_BITS) as u64)
}

/// The client's hash map, as the order of its key list.
struct ClientMap {
    keys: Vec<u32>,
    buckets: u64,
    held: HashSet<u32>,
}

impl ClientMap {
    fn new() -> Self {
        Self {
            keys: Vec::new(),
            buckets: 1,
            held: HashSet::new(),
        }
    }

    /// Puts `key` into `list` as a map of `buckets` buckets: after the last key of its bucket,
    /// else at the front.
    fn place(list: &mut Vec<u32>, key: u32, buckets: u64) {
        let bucket = bucket_hash(key) & (buckets - 1);
        match list
            .iter()
            .rposition(|other| bucket_hash(*other) & (buckets - 1) == bucket)
        {
            Some(last) => list.insert(last + 1, key),
            None => list.insert(0, key),
        }
    }

    /// Inserts `key`; a held key changes nothing.
    fn insert(&mut self, key: u32) {
        if !self.held.insert(key) {
            return;
        }
        Self::place(&mut self.keys, key, self.buckets);
        if self.keys.len() as u64 > self.buckets {
            self.rehash(self.buckets + 1);
        }
    }

    /// Grows to the next power of two of at least `wanted` buckets and the key count.
    fn rehash(&mut self, wanted: u64) {
        let grown = wanted
            .max(self.keys.len() as u64)
            .next_power_of_two()
            .max(MIN_GROWN_BUCKETS);
        if grown == self.buckets {
            return;
        }
        let mut list = Vec::with_capacity(self.keys.len());
        for key in &self.keys {
            Self::place(&mut list, *key, grown);
        }
        self.keys = list;
        self.buckets = grown;
    }

    /// The key order of the map's copy: the buckets' groups from the last bucket to the first.
    fn copied_order(&self) -> Vec<u32> {
        let mut order = Vec::with_capacity(self.keys.len());
        for bucket in (0..self.buckets).rev() {
            order.extend(
                self.keys
                    .iter()
                    .filter(|key| bucket_hash(**key) & (self.buckets - 1) == bucket),
            );
        }
        order
    }
}

/// The perk ids of one `PerkData` sheet in the client's order.
fn sheet_order(perk_ids: &[&str]) -> Vec<String> {
    let mut map = ClientMap::new();
    let mut by_key = HashMap::new();
    for perk_id in perk_ids {
        let key = lowercase_crc32(perk_id);
        by_key.entry(key).or_insert(*perk_id);
        map.insert(key);
    }
    map.copied_order()
        .into_iter()
        .filter_map(|key| by_key.get(&key).map(|perk_id| (*perk_id).to_owned()))
        .collect()
}

/// The parts of a `+`-separated cell, trimmed, without empty ones.
pub fn parts(cell: Option<&str>) -> Vec<String> {
    cell.unwrap_or_default()
        .split(CLASS_SEPARATOR)
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether one of `parts` is in `classes` (any case).
fn any_in(parts: &[String], classes: &[String]) -> bool {
    parts
        .iter()
        .any(|part| classes.iter().any(|class| class.eq_ignore_ascii_case(part)))
}

/// One perk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Perk {
    /// `PerkID`.
    pub perk_id: String,
    /// `PerkType`: `Inherent`, `Generated`, `Gem`, `OffensiveCharm`, `DefensiveCharm`,
    /// `SkillCharm`, ...
    pub perk_type: String,
    /// The item classes it goes on (`ItemClass`).
    pub item_classes: Vec<String>,
    /// The item classes it never goes on (`ExcludeItemClass`).
    pub excluded_classes: Vec<String>,
    /// Its exclusive labels: two perks with a label in common never share an item.
    pub labels: Vec<String>,
    /// `DisplayName`, the text key the client shows.
    pub display_name: Option<String>,
}

impl Perk {
    /// Whether it goes on an item of `classes`.
    pub fn fits(&self, classes: &[String]) -> bool {
        any_in(&self.item_classes, classes) && !any_in(&self.excluded_classes, classes)
    }

    /// Whether it shares an exclusive label with `other`.
    pub fn clashes(&self, other: &Self) -> bool {
        any_in(&self.labels, &other.labels)
    }
}

/// One `ItemPerkSwapData` row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PerkSwap {
    key: SwapKey,
    old_perk: String,
    new_perk: String,
}

/// What a swap applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SwapKey {
    /// The item of this id (lowercase).
    Item(String),
    /// Items that hold every one of these classes.
    Classes(Vec<String>),
}

impl SwapKey {
    fn applies(&self, item_id: &str, classes: &[String]) -> bool {
        match self {
            Self::Item(id) => id.eq_ignore_ascii_case(item_id),
            Self::Classes(wanted) => wanted
                .iter()
                .all(|part| classes.iter().any(|class| class.eq_ignore_ascii_case(part))),
        }
    }
}

/// One entry of a perk bucket.
#[derive(Debug, Clone, PartialEq)]
enum BucketEntry {
    /// A perk id.
    Perk(String),
    /// Another bucket (lowercase id).
    Nested(String),
}

/// One `PerkBucketData` bucket.
#[derive(Debug, Clone, PartialEq)]
pub struct PerkBucket {
    /// `PerkType`.
    pub perk_type: Option<String>,
    /// `PerkChance`: the chance its slot gets a perk.
    pub chance: f64,
    /// Its entries and their weights (its `_Weights` row, else 1 each).
    entries: Vec<(BucketEntry, f64)>,
}

/// Every perk, swap and bucket, and the client's wire index of each perk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerkTable {
    /// The perk id of each wire index; index 0 is empty.
    wire: Vec<String>,
    /// The wire index of each lowercase perk id.
    indexes: HashMap<String, u32>,
    /// Each perk by lowercase id.
    perks: HashMap<String, Perk>,
    swaps: Vec<PerkSwap>,
    /// Each bucket by lowercase id.
    buckets: HashMap<String, PerkBucket>,
}

/// One `PerkData` sheet as the table is built from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerkSheet {
    /// Its unique id.
    pub unique_id: String,
    /// Its rows in file order.
    pub perks: Vec<Perk>,
}

/// One `ItemPerkSwapData` row as the table is built from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerkSwapRow {
    /// `Key`.
    pub key: String,
    /// `KeyType`: `ItemId` or `ItemClass`.
    pub key_type: String,
    /// `OldPerk`.
    pub old_perk: String,
    /// `NewPerk`.
    pub new_perk: String,
}

/// One `PerkBucketData` row as the table is built from it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerkBucketRow {
    /// `PerkBucketID`.
    pub id: String,
    /// `PerkType`.
    pub perk_type: Option<String>,
    /// `PerkChance`.
    pub chance: Option<f64>,
    /// The `PerkN` cells in column order.
    pub entries: Vec<String>,
}

/// What a [`PerkTable`] is built from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerkSource {
    /// The `PerkData` sheets.
    pub sheets: Vec<PerkSheet>,
    /// The `ItemPerkSwapData` rows in file order.
    pub swaps: Vec<PerkSwapRow>,
    /// The `PerkBucketData` rows.
    pub buckets: Vec<PerkBucketRow>,
}

impl PerkSource {
    /// The perk rows of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self {
            sheets: tables
                .perk_data
                .iter()
                .map(|table| PerkSheet {
                    unique_id: table.unique_id.clone(),
                    perks: table
                        .rows
                        .iter()
                        .map(|row| Perk {
                            perk_id: row.perk_id.clone(),
                            perk_type: row.perk_type.clone(),
                            item_classes: parts(row.item_class.as_deref()),
                            excluded_classes: parts(row.exclude_item_class.as_deref()),
                            labels: parts(row.exclusive_labels.as_deref()),
                            display_name: row.display_name.clone(),
                        })
                        .collect(),
                })
                .collect(),
            swaps: tables
                .item_perk_swap_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| PerkSwapRow {
                    key: row.key.clone(),
                    key_type: row.key_type.clone(),
                    old_perk: row.old_perk.clone(),
                    new_perk: row.new_perk.clone(),
                })
                .collect(),
            buckets: tables
                .perk_bucket_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| PerkBucketRow {
                    id: row.perk_bucket_id.clone(),
                    perk_type: row.perk_type.clone(),
                    chance: row.perk_chance,
                    entries: row.perk.values().cloned().collect(),
                })
                .collect(),
        }
    }
}

impl PerkTable {
    /// The table of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(PerkSource::from_tables(tables))
    }

    /// The table of `source`. Sheets go in [`PERK_SHEET_ORDER`]; a sheet the order does not name
    /// goes after them, by unique id (best guess: no such sheet in build 25810808).
    pub fn build(mut source: PerkSource) -> Self {
        let rank = |unique_id: &str| {
            PERK_SHEET_ORDER
                .iter()
                .position(|known| known.eq_ignore_ascii_case(unique_id))
                .unwrap_or(PERK_SHEET_ORDER.len())
        };
        source.sheets.sort_by(|left, right| {
            rank(&left.unique_id)
                .cmp(&rank(&right.unique_id))
                .then_with(|| left.unique_id.cmp(&right.unique_id))
        });
        let mut table = Self {
            wire: vec![String::new()],
            ..Self::default()
        };
        for sheet in &source.sheets {
            let ids: Vec<&str> = sheet.perks.iter().map(|perk| perk.perk_id.as_str()).collect();
            for perk_id in sheet_order(&ids) {
                let index = u32::try_from(table.wire.len()).unwrap_or(u32::MAX);
                table.indexes.entry(perk_id.to_lowercase()).or_insert(index);
                table.wire.push(perk_id);
            }
            for perk in &sheet.perks {
                table
                    .perks
                    .entry(perk.perk_id.to_lowercase())
                    .or_insert_with(|| perk.clone());
            }
        }
        table.swaps = source
            .swaps
            .iter()
            .filter_map(|row| {
                let key = if row.key_type.eq_ignore_ascii_case(KEY_TYPE_ITEM_ID) {
                    SwapKey::Item(row.key.to_lowercase())
                } else if row.key_type.eq_ignore_ascii_case(KEY_TYPE_ITEM_CLASS) {
                    SwapKey::Classes(parts(Some(&row.key)))
                } else {
                    return None;
                };
                Some(PerkSwap {
                    key,
                    old_perk: row.old_perk.clone(),
                    new_perk: row.new_perk.clone(),
                })
            })
            .collect();
        let weights: HashMap<String, Vec<f64>> = source
            .buckets
            .iter()
            .filter_map(|row| {
                let lower = row.id.to_lowercase();
                let owner = lower.strip_suffix(&WEIGHTS_SUFFIX.to_lowercase())?;
                Some((
                    owner.to_owned(),
                    row.entries
                        .iter()
                        .map(|cell| cell.trim().parse().unwrap_or(PLAIN_WEIGHT))
                        .collect(),
                ))
            })
            .collect();
        table.buckets = source
            .buckets
            .iter()
            .filter(|row| {
                !row.id
                    .to_lowercase()
                    .ends_with(&WEIGHTS_SUFFIX.to_lowercase())
            })
            .map(|row| {
                let lower = row.id.to_lowercase();
                let row_weights = weights.get(&lower);
                let entries = row
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, cell)| !cell.trim().is_empty())
                    .map(|(at, cell)| {
                        let cell = cell.trim();
                        let entry = match cell.strip_prefix(NESTED_BUCKET_PREFIX) {
                            Some(nested) => BucketEntry::Nested(nested.trim().to_lowercase()),
                            None => BucketEntry::Perk(cell.to_owned()),
                        };
                        let weight = row_weights
                            .and_then(|weights| weights.get(at))
                            .copied()
                            .unwrap_or(PLAIN_WEIGHT);
                        (entry, weight)
                    })
                    .collect();
                (
                    lower,
                    PerkBucket {
                        perk_type: row.perk_type.clone().filter(|kind| !kind.is_empty()),
                        chance: row.chance.unwrap_or(PLAIN_CHANCE),
                        entries,
                    },
                )
            })
            .collect();
        table
    }

    /// The wire index of each lowercase perk id.
    pub fn indexes(&self) -> HashMap<String, u32> {
        self.indexes.clone()
    }

    /// The wire index of `perk_id` (any case); `None` for a perk the client does not know.
    pub fn wire_index(&self, perk_id: &str) -> Option<u32> {
        self.indexes.get(&perk_id.to_lowercase()).copied()
    }

    /// The perk id of wire index `index`; `None` for 0 and an index past the table.
    pub fn perk_id(&self, index: u32) -> Option<&str> {
        let index = usize::try_from(index).ok().filter(|index| *index > 0)?;
        self.wire.get(index).map(String::as_str)
    }

    /// The perk `perk_id` (any case).
    pub fn perk(&self, perk_id: &str) -> Option<&Perk> {
        self.perks.get(&perk_id.to_lowercase())
    }

    /// The bucket `bucket_id` (any case).
    pub fn bucket(&self, bucket_id: &str) -> Option<&PerkBucket> {
        self.buckets.get(&bucket_id.to_lowercase())
    }

    /// The perk an item `item_id` of `classes` carries for `perk_id`: the swaps applied until
    /// none applies (a chain stops after as many steps as there are swaps).
    pub fn swapped(&self, item_id: &str, classes: &[String], perk_id: &str) -> String {
        let mut current = perk_id.to_owned();
        for _ in 0..=self.swaps.len() {
            let next = self.swaps.iter().find(|swap| {
                swap.old_perk.eq_ignore_ascii_case(&current) && swap.key.applies(item_id, classes)
            });
            match next {
                Some(swap) => current.clone_from(&swap.new_perk),
                None => break,
            }
        }
        current
    }

    /// The fixed perks of an item `item_id` of `classes` whose `Perk1..5` cells are `cells`:
    /// each swapped, in slot order ([`Self::ordered`]); an empty cell is left out.
    pub fn fixed_perks<'a>(
        &self,
        item_id: &str,
        classes: &[String],
        cells: impl IntoIterator<Item = Option<&'a str>>,
    ) -> Vec<String> {
        Self::ordered(
            cells
                .into_iter()
                .flatten()
                .map(str::trim)
                .filter(|cell| !cell.is_empty())
                .map(|cell| self.swapped(item_id, classes, cell))
                .collect(),
        )
    }

    /// `perks` in the slot order retail sends: descending CRC-32 of the lowercase perk id.
    pub fn ordered(mut perks: Vec<String>) -> Vec<String> {
        perks.sort_by_key(|perk_id| std::cmp::Reverse(lowercase_crc32(perk_id)));
        perks
    }

    /// The wire slots of `perks` (slot order kept; a perk the client does not know and a perk
    /// past the fifth are left out).
    pub fn wire_slots(&self, perks: &[String]) -> [u32; PERK_SLOTS] {
        let mut slots = [0; PERK_SLOTS];
        let known = perks.iter().filter_map(|perk_id| self.wire_index(perk_id));
        for (slot, index) in slots.iter_mut().zip(known) {
            *slot = index;
        }
        slots
    }

    /// The perk ids of wire `slots` in slot order; empty and unknown slots are left out.
    pub fn perk_ids(&self, slots: &[u32; PERK_SLOTS]) -> Vec<String> {
        slots
            .iter()
            .filter_map(|index| self.perk_id(*index).map(str::to_owned))
            .collect()
    }

    /// Picks one perk of bucket `bucket_id` for an item `item_id` of `classes` that holds
    /// `held`: by the entry weights, among the perks (after swaps) that fit the item, that it
    /// does not hold and that share no exclusive label with a held perk; nested buckets weigh
    /// their perks by their entry's weight. `below(n)` is a random number below `n`. `None`
    /// when no perk qualifies.
    pub fn pick(
        &self,
        bucket_id: &str,
        item_id: &str,
        classes: &[String],
        held: &[String],
        below: &mut impl FnMut(u32) -> u32,
    ) -> Option<String> {
        let mut candidates = Vec::new();
        self.candidates(bucket_id, item_id, classes, held, 1.0, 0, &mut candidates);
        let total: f64 = candidates.iter().map(|(_, weight)| weight).sum();
        if total <= 0.0 {
            return None;
        }
        let mut roll = fraction(below) * total;
        for (perk_id, weight) in &candidates {
            if roll < *weight {
                return Some(perk_id.clone());
            }
            roll -= weight;
        }
        candidates.last().map(|(perk_id, _)| perk_id.clone())
    }

    /// Adds the qualifying perks of `bucket_id` with their weights times `scale`.
    #[allow(clippy::too_many_arguments, reason = "one recursive walk")]
    fn candidates(
        &self,
        bucket_id: &str,
        item_id: &str,
        classes: &[String],
        held: &[String],
        scale: f64,
        depth: usize,
        out: &mut Vec<(String, f64)>,
    ) {
        let Some(bucket) = self.bucket(bucket_id).filter(|_| depth < MAX_BUCKET_DEPTH) else {
            return;
        };
        let total: f64 = bucket.entries.iter().map(|(_, weight)| weight).sum();
        if total <= 0.0 {
            return;
        }
        for (entry, weight) in &bucket.entries {
            let share = scale * weight / total;
            match entry {
                BucketEntry::Nested(nested) => {
                    self.candidates(nested, item_id, classes, held, share, depth + 1, out);
                }
                BucketEntry::Perk(perk_id) => {
                    let perk_id = self.swapped(item_id, classes, perk_id);
                    if share > 0.0 && self.qualifies(&perk_id, classes, held) {
                        out.push((perk_id, share));
                    }
                }
            }
        }
    }

    /// Whether `perk_id` may join `held` on an item of `classes`.
    fn qualifies(&self, perk_id: &str, classes: &[String], held: &[String]) -> bool {
        let Some(perk) = self.perk(perk_id) else {
            return false;
        };
        perk.fits(classes)
            && held.iter().all(|other| {
                !other.eq_ignore_ascii_case(perk_id)
                    && self.perk(other).is_none_or(|other| !perk.clashes(other))
            })
    }
}

/// A random fraction in [0, 1) from `below`.
fn fraction(below: &mut impl FnMut(u32) -> u32) -> f64 {
    f64::from(below(FRACTION_STEPS)) / f64::from(FRACTION_STEPS)
}

/// Whether a draw of `below` falls under `chance` (0 to 1).
pub fn chance_hit(chance: f64, below: &mut impl FnMut(u32) -> u32) -> bool {
    fraction(below) < chance
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEM: &str = "2hGreatSwordT2";
    const SENTRY: &str = "PerkID_Stat_TwoHandSentry";
    const BASE_DAMAGE: &str = "PerkID_StatSwap_BaseDamage";

    fn perk(perk_id: &str, classes: &str, labels: &str) -> Perk {
        Perk {
            perk_id: perk_id.to_owned(),
            perk_type: "Generated".to_owned(),
            item_classes: parts(Some(classes)),
            excluded_classes: parts(Some("EquippableTool")),
            labels: parts(Some(labels)),
            display_name: None,
        }
    }

    fn classes() -> Vec<String> {
        parts(Some("EquippableTwoHand+GreatSword+Melee"))
    }

    fn source() -> PerkSource {
        PerkSource {
            sheets: vec![
                PerkSheet {
                    unique_id: "ItemPerks".to_owned(),
                    perks: vec![
                        perk(SENTRY, "GreatSword", "Attribute"),
                        perk(BASE_DAMAGE, "GreatSword", "Attribute"),
                        perk("PerkID_Weapon_DmgBasic", "EquippableTwoHand", "DmgCon"),
                        perk("PerkID_Weapon_DmgCrit", "EquippableTwoHand", "DmgCon"),
                        perk("PerkID_Armor_DurCC", "Armor", "DebuffDur"),
                    ],
                },
                PerkSheet {
                    unique_id: "ItemPerks_Gems".to_owned(),
                    perks: vec![perk("PerkID_Gem_EmptyGemSlot", "EquippableTwoHand", "")],
                },
            ],
            swaps: vec![PerkSwapRow {
                key: "EquippableTwoHand+GreatSword".to_owned(),
                key_type: KEY_TYPE_ITEM_CLASS.to_owned(),
                old_perk: SENTRY.to_owned(),
                new_perk: BASE_DAMAGE.to_owned(),
            }],
            buckets: vec![
                PerkBucketRow {
                    id: "Generated".to_owned(),
                    perk_type: Some("Generated".to_owned()),
                    chance: Some(0.5),
                    entries: vec![
                        "PerkID_Weapon_DmgBasic".to_owned(),
                        "PerkID_Weapon_DmgCrit".to_owned(),
                        "PerkID_Armor_DurCC".to_owned(),
                        "[PBID]Attributes".to_owned(),
                    ],
                },
                PerkBucketRow {
                    id: "Generated_Weights".to_owned(),
                    entries: vec!["0".to_owned(), "3".to_owned(), "5".to_owned(), "0".to_owned()],
                    ..PerkBucketRow::default()
                },
                PerkBucketRow {
                    id: "Attributes".to_owned(),
                    perk_type: Some("Inherent".to_owned()),
                    chance: None,
                    entries: vec![SENTRY.to_owned()],
                },
            ],
        }
    }

    #[test]
    fn the_client_map_orders_keys_by_its_buckets() {
        // Four keys in one map: every key is listed once, and the copy lists the last bucket's
        // group first.
        let ids = ["a", "b", "c", "d"];
        let order = sheet_order(&ids);
        assert_eq!(order.len(), ids.len());
        let mut map = ClientMap::new();
        for id in ids {
            map.insert(lowercase_crc32(id));
        }
        assert_eq!(map.buckets, 4);
        let buckets: Vec<u64> = order
            .iter()
            .map(|id| bucket_hash(lowercase_crc32(id)) & (map.buckets - 1))
            .collect();
        assert!(buckets.windows(2).all(|pair| pair[0] >= pair[1]));
    }

    #[test]
    fn the_wire_index_walks_the_sheets_in_the_client_order() {
        let table = PerkTable::build(source());
        assert_eq!(table.perk_id(0), None);
        assert_eq!(table.perk_id(1), Some("PerkID_Gem_EmptyGemSlot"));
        assert_eq!(table.wire_index("perkid_gem_emptygemslot"), Some(1));
        let swords: Vec<u32> = [SENTRY, BASE_DAMAGE]
            .iter()
            .map(|id| table.wire_index(id).expect("known"))
            .collect();
        assert!(swords.iter().all(|index| (2..=6).contains(index)));
        let slots = table.wire_slots(&[BASE_DAMAGE.to_owned(), "Unknown".to_owned()]);
        assert_eq!(slots[1..], [0; PERK_SLOTS - 1]);
        assert_eq!(table.perk_ids(&slots), [BASE_DAMAGE]);
    }

    #[test]
    fn a_swap_follows_its_item_classes() {
        let table = PerkTable::build(source());
        assert_eq!(table.swapped(ITEM, &classes(), SENTRY), BASE_DAMAGE);
        let armor = parts(Some("EquippableChest+Armor"));
        assert_eq!(table.swapped("Chest", &armor, SENTRY), SENTRY);
    }

    #[test]
    fn fixed_perks_are_swapped_and_ordered() {
        let table = PerkTable::build(source());
        let fixed = table.fixed_perks(
            ITEM,
            &classes(),
            [Some(SENTRY), None, Some(" "), Some("PerkID_Weapon_DmgCrit")],
        );
        assert_eq!(
            fixed,
            PerkTable::ordered(vec![
                BASE_DAMAGE.to_owned(),
                "PerkID_Weapon_DmgCrit".to_owned()
            ])
        );
    }

    #[test]
    fn perks_go_in_descending_crc_order() {
        let ordered = PerkTable::ordered(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let crcs: Vec<u32> = ordered.iter().map(|id| lowercase_crc32(id)).collect();
        assert!(crcs.windows(2).all(|pair| pair[0] >= pair[1]));
    }

    #[test]
    fn a_pick_takes_a_fitting_weighted_perk_without_a_label_clash() {
        let table = PerkTable::build(source());
        let mut zero = |_: u32| 0_u32;
        // Weights 0, 3, 5 (armor only), 0: only the crit perk fits a greatsword.
        let picked = table.pick("generated", ITEM, &classes(), &[], &mut zero);
        assert_eq!(picked.as_deref(), Some("PerkID_Weapon_DmgCrit"));
        let held = ["PerkID_Weapon_DmgBasic".to_owned()];
        assert_eq!(table.pick("Generated", ITEM, &classes(), &held, &mut zero), None);
        let attribute = table.pick("Attributes", ITEM, &classes(), &[], &mut zero);
        assert_eq!(attribute.as_deref(), Some(BASE_DAMAGE));
        assert!((table.bucket("GENERATED").expect("bucket").chance - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn the_perk_count_follows_the_tier_table() {
        assert_eq!(perk_count(1, 500), 0);
        assert_eq!(perk_count(2, 100), 1);
        assert_eq!(perk_count(2, 170), 2);
        assert_eq!(perk_count(3, 300), 2);
        assert_eq!(perk_count(4, 420), 3);
        assert_eq!(perk_count(4, 450), 4);
        assert_eq!(perk_count(5, 610), 5);
        assert_eq!(perk_count(5, 650), 4);
        assert_eq!(perk_count(5, 700), 5);
        assert_eq!(perk_count(4, 900), 4);
        assert_eq!(rarity(0), 0);
        assert_eq!(rarity(2), 1);
        assert_eq!(rarity(5), 4);
        assert!(chance_hit(0.5, &mut |_: u32| 0_u32));
        assert!(!chance_hit(0.0, &mut |_: u32| 0_u32));
    }
}
