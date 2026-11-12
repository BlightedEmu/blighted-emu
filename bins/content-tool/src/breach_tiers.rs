// SPDX-License-Identifier: AGPL-3.0-or-later
//! `breach-tiers`: the difficulty of each corrupted breach place, from the world map points of
//! the retail ledgers (`breach-tiers-<build>.json`, read by `content-tool dungeon-scripts`).
//!
//! Observed (breaches note 2026-11-11): the world entity's map data state
//! (`Javelin::GlobalMapDataManagerComponentReplicatedState`, class 3780) lists the world map
//! points; a full list is `01 01` (slot and child mask), a list head, then one entry per point:
//! key u64, x f32, y f32, type u16, number u32, flag u8 (all big-endian, 23 bytes). Types 8001
//! to 8009 are the breaches (the minor monolith slice holds the text `8001`); the number is the
//! breach's difficulty: in the 24 breach ledgers the place of every won breach showed the number
//! of its reward event's `DT<n>` (`Darkness_Minor_DT1_T1` at a number 1 place, `_DT2_` at 2,
//! `_DT3_` at 3, `_DT4_` at 4; `Darkness_Major_DT4_T2` at a number 4 place). A place keeps its
//! number for weeks (a sample of 146 retail ledgers: 32 of 230 places showed two neighbouring
//! numbers, none showed numbers further apart).
//!
//! The file keeps, per place (x, y rounded to [`PLACE_GRID_M`]), the number seen in most
//! ledgers. The server's own rule for the number is Unknown (`javelindata_darkness`'s only row is
//! named `test`; `javelindata_difficulty_darkness` maps 40 steps of an Unknown input to
//! difficulties).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nw_capture::{Decoder, SERVER_TO_CLIENT, ledger_records};
use nw_messages::{
    BodyCodec, BundlePayload, MemberState, REPLICATED_STATE_BUNDLE_TYPE_INDEX,
    ReplicatedStateBundle,
};
use serde::{Deserialize, Serialize};

/// `Javelin::GlobalMapDataManagerComponentReplicatedState`.
const MAP_DATA_CLASS: u64 = 3780;
/// The bytes of one map point.
const ENTRY_LEN: usize = 23;
/// Where a point's fields start in its bytes.
const X_AT: usize = 8;
const Y_AT: usize = 12;
const TYPE_AT: usize = 16;
const NUMBER_AT: usize = 18;
/// The bytes before the list head of a full list: slot and child mask.
const STATE_HEAD_LEN: usize = 2;
/// The longest list head before the entry count (form byte and version).
const MAX_LIST_HEAD_LEN: usize = 12;
/// The map point types of the corrupted breaches.
const FIRST_BREACH_TYPE: u16 = 8001;
const LAST_BREACH_TYPE: u16 = 8009;
/// The grid the places are rounded to, metres (map points of one breach place lie within a few
/// metres of each other; our choice).
pub const PLACE_GRID_M: f32 = 5.0;
/// The file name prefix and extension.
const FILE_PREFIX: &str = "breach-tiers-";
const FILE_EXTENSION: &str = ".json";

/// The places of one ledger by grid cell: the first place seen and its numbers.
type LedgerPlaces = BTreeMap<(i32, i32), ([f32; 2], Vec<u32>)>;
/// The places of every ledger by grid cell: the first place seen and in how many ledgers each
/// number showed.
type PlaceCounts = BTreeMap<(i32, i32), ([f32; 2], BTreeMap<u32, u32>)>;

/// The difficulty of one breach place.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BreachTier {
    /// The place's x, metres.
    pub x: f32,
    /// The place's y, metres.
    pub y: f32,
    /// The difficulty (the map point's number).
    pub difficulty: u32,
    /// In how many ledgers the place showed that difficulty.
    pub ledgers: u32,
}

/// Why the file cannot be made or read.
#[derive(Debug, thiserror::Error)]
pub enum BreachTiersError {
    /// A file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table's JSON.
    #[error("breach tiers: {0}")]
    Json(#[from] serde_json::Error),
}

/// The path of the file of `client_build` in `dir`.
pub fn breach_tiers_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_EXTENSION}"))
}

/// The breach points (place, type, number) of one map data state body; none for a body that is
/// no full list.
fn breach_points(body: &[u8]) -> Vec<([f32; 2], u32)> {
    let field = |entry: &[u8], at: usize, len: usize| entry.get(at..at + len).map(<[u8]>::to_vec);
    // The entry count is the byte before the entries; the list head before it has no fixed
    // length here, so the count is found where the rest is exactly that many entries.
    let Some(start) = (STATE_HEAD_LEN + 1..body.len().min(STATE_HEAD_LEN + MAX_LIST_HEAD_LEN))
        .find(|&start| {
            let count = usize::from(body[start - 1]);
            count > 0 && body.len() - start == count * ENTRY_LEN
        })
    else {
        return Vec::new();
    };
    body[start..]
        .chunks(ENTRY_LEN)
        .filter_map(|entry| {
            let x = f32::from_be_bytes(field(entry, X_AT, 4)?.try_into().ok()?);
            let y = f32::from_be_bytes(field(entry, Y_AT, 4)?.try_into().ok()?);
            let kind = u16::from_be_bytes(field(entry, TYPE_AT, 2)?.try_into().ok()?);
            let number = u32::from_be_bytes(field(entry, NUMBER_AT, 4)?.try_into().ok()?);
            (FIRST_BREACH_TYPE..=LAST_BREACH_TYPE)
                .contains(&kind)
                .then_some(([x, y], number))
        })
        .collect()
}

/// The grid cell of a place.
#[allow(clippy::cast_possible_truncation, reason = "a world grid cell")]
fn cell([x, y]: [f32; 2]) -> (i32, i32) {
    (
        (x / PLACE_GRID_M).round() as i32,
        (y / PLACE_GRID_M).round() as i32,
    )
}

/// The breach places of the ledger `bytes`: each place's numbers, once per ledger.
fn ledger_places(bytes: &[u8]) -> LedgerPlaces {
    let mut places: LedgerPlaces = BTreeMap::new();
    let mut decoder = Decoder::new();
    for record in ledger_records(bytes) {
        let Ok(record) = record else { break };
        let output = decoder.record(&record);
        if record.direction != SERVER_TO_CLIENT {
            continue;
        }
        for decoded in &output.envelopes {
            if decoded.envelope.type_index() != Some(REPLICATED_STATE_BUNDLE_TYPE_INDEX) {
                continue;
            }
            let Ok(bundle) = ReplicatedStateBundle::decode(&decoded.envelope.body) else {
                continue;
            };
            let Ok(payload) = BundlePayload::decode(&bundle.payload) else {
                continue;
            };
            for member in payload.groups.iter().flat_map(|group| &group.members) {
                let MemberState::Recorded { class, body } = &member.state else {
                    continue;
                };
                if *class != MAP_DATA_CLASS {
                    continue;
                }
                for (place, number) in breach_points(body) {
                    let entry = places.entry(cell(place)).or_insert((place, Vec::new()));
                    if !entry.1.contains(&number) {
                        entry.1.push(number);
                    }
                }
            }
        }
    }
    places
}

/// The breach tiers of the ledgers `ledgers`: per place, the number most ledgers showed (the
/// lower one on a tie).
///
/// # Errors
///
/// Returns [`BreachTiersError::Io`] when a ledger cannot be read.
pub fn build_breach_tiers(ledgers: &[PathBuf]) -> Result<Vec<BreachTier>, BreachTiersError> {
    let mut counts: PlaceCounts = BTreeMap::new();
    for path in ledgers {
        let bytes = std::fs::read(path).map_err(|source| BreachTiersError::Io {
            path: path.clone(),
            source,
        })?;
        merge(&mut counts, ledger_places(&bytes));
    }
    Ok(tiers(&counts))
}

fn merge(counts: &mut PlaceCounts, places: LedgerPlaces) {
    for (key, (place, numbers)) in places {
        let entry = counts.entry(key).or_insert((place, BTreeMap::new()));
        for number in numbers {
            *entry.1.entry(number).or_default() += 1;
        }
    }
}

fn tiers(counts: &PlaceCounts) -> Vec<BreachTier> {
    counts
        .values()
        .filter_map(|([x, y], numbers)| {
            // The most seen; `max_by_key` keeps the last of equals, so walk from the highest.
            let (difficulty, ledgers) = numbers
                .iter()
                .rev()
                .max_by_key(|(_, seen)| **seen)
                .map(|(number, seen)| (*number, *seen))?;
            Some(BreachTier {
                x: *x,
                y: *y,
                difficulty,
                ledgers,
            })
        })
        .collect()
}

/// Writes `tiers` to the file of `client_build` in `dir`; its path.
///
/// # Errors
///
/// Returns [`BreachTiersError`] when the file cannot be written.
pub fn write_breach_tiers(
    dir: &Path,
    client_build: &str,
    tiers: &[BreachTier],
) -> Result<PathBuf, BreachTiersError> {
    let path = breach_tiers_path(dir, client_build);
    std::fs::write(&path, serde_json::to_vec_pretty(tiers)?).map_err(|source| {
        BreachTiersError::Io {
            path: path.clone(),
            source,
        }
    })?;
    Ok(path)
}

/// The tiers of the file of `client_build` in `dir`; none when there is no file.
///
/// # Errors
///
/// Returns [`BreachTiersError`] when the file is there but cannot be read.
pub fn load_breach_tiers(
    dir: &Path,
    client_build: &str,
) -> Result<Vec<BreachTier>, BreachTiersError> {
    let path = breach_tiers_path(dir, client_build);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(BreachTiersError::Io { path, source }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A retail map data body (Observed form, findings 2026-11-11 player states), keys made up:
    /// head `01 01 00 01`, version `ca 64 04`, count 2, then a minor monolith of number 2 at
    /// (9158.6, 5684.1) and a town point of type 8030.
    fn body() -> Vec<u8> {
        let mut body = vec![0x01, 0x01, 0x00, 0x01, 0xca, 0x64, 0x04, 0x02];
        for (key, x, y, kind, number) in [
            (
                0x1111_2222_3333_4444_u64,
                9158.6_f32,
                5684.1_f32,
                8001_u16,
                2_u32,
            ),
            (0x5555_6666_7777_8888, 7000.0, 3000.0, 8030, 0),
        ] {
            body.extend_from_slice(&key.to_be_bytes());
            body.extend_from_slice(&x.to_be_bytes());
            body.extend_from_slice(&y.to_be_bytes());
            body.extend_from_slice(&kind.to_be_bytes());
            body.extend_from_slice(&number.to_be_bytes());
            body.push(0);
        }
        body
    }

    #[test]
    fn a_full_list_gives_its_breach_points_only() {
        let points = breach_points(&body());
        assert_eq!(points.len(), 1, "the town point is no breach");
        let ([x, y], number) = points[0];
        assert!((x - 9158.6).abs() < 0.01 && (y - 5684.1).abs() < 0.01);
        assert_eq!(number, 2);
        assert!(
            breach_points(&body()[..body().len() - 1]).is_empty(),
            "no full list"
        );
    }

    #[test]
    fn a_place_keeps_the_number_most_ledgers_showed() {
        let mut counts = BTreeMap::new();
        let place = [9118.7, 4769.1];
        for numbers in [vec![3], vec![3, 4], vec![4], vec![3]] {
            merge(
                &mut counts,
                BTreeMap::from([(cell(place), (place, numbers))]),
            );
        }
        let tiers = tiers(&counts);
        assert_eq!(tiers.len(), 1);
        assert_eq!((tiers[0].difficulty, tiers[0].ledgers), (3, 3));
        let mut tie = BTreeMap::new();
        merge(
            &mut tie,
            BTreeMap::from([(cell(place), (place, vec![3, 4]))]),
        );
        assert_eq!(super::tiers(&tie)[0].difficulty, 3, "the lower on a tie");
    }
}
