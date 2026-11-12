// SPDX-License-Identifier: AGPL-3.0-or-later
//! The forts of the territory wars (wars and sieges study 2026-11-12, findings
//! `2026-11-12-wars-and-sieges.md`, section "The framed instance and the war fight"): for each
//! fort that a retail war capture shows, the frame of its war instance, where the players of
//! each side arrived, the siege data entity and the four capture points with their spawn cards.
//!
//! Observed (the `warforts` bin of the capture tools over every retail ledger with the siege
//! warfare data, 4103): a war runs in an instance of the open world level whose 1635 names
//! `NewWorld_VitaeEterna` with a 512 m frame and a private phase (`arvaysa_ES_attacker_war_45vs45`
//! 1783536302144, frame 5632, 5120, 512, 512). Each of the six forts of the war captures
//! (territories 4, 8, 10, 11, 15, 17) has its frame at the 512 m grid square that holds its claim
//! point, and its claim point (number [`CLAIM_POINT_NUMBER`]) at the territory's claim position.
//! Inside the instance retail spawns a group with the siege data (card and 4103) and, for each
//! capture point, a group of its card, its place, its ownership and its 333 state; the 4103 keys
//! a point by the first eight bytes of its `GdeRef` read as a little-endian `u64`
//! ([`FortPoint::key`]), and the point's 4062 names the point's facet id and that key.
//!
//! The table file is mined from the captures (no tracked file holds capture data): a fort that no
//! capture shows has no entry, and it gets no war instance. The invasion forts (territories 2, 5
//! and 16) showed a frame but no capture point in the instance: they have no entry either.

use std::path::{Path, PathBuf};

use serde::Deserialize;

const FILE_PREFIX: &str = "war-forts-";
const FILE_SUFFIX: &str = ".json";
/// The bytes of an asset id and of a `GdeRef`.
pub const ID_LEN: usize = 16;
/// The bytes of a capture point's key: the first part of its `GdeRef`.
const KEY_LEN: usize = 8;
/// The number of a fort's claim point (Observed: point 4 is at the claim position in all six
/// forts, and its capture ended the ES war).
pub const CLAIM_POINT_NUMBER: u32 = 4;
/// The war side of an attacker and of a defender (the 2762 side).
pub const SIDE_ATTACKER: u8 = 1;
/// The war side of a defender.
pub const SIDE_DEFENDER: u8 = 2;

/// The table file of client build `client_build` in content directory `dir`.
pub fn war_forts_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum WarFortsError {
    /// The file cannot be read.
    #[error("war forts table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("war forts table: {0}")]
    Json(#[from] serde_json::Error),
    /// A hex field does not read.
    #[error("war forts table: territory {territory}: {source}")]
    Hex {
        /// The fort's territory.
        territory: u16,
        /// Why.
        source: hex::FromHexError,
    },
    /// An id or a facet has the wrong length.
    #[error("war forts table: territory {territory}: a field of {len} bytes")]
    Len {
        /// The fort's territory.
        territory: u16,
        /// The bytes it has.
        len: usize,
    },
    /// A fort has no claim point.
    #[error("war forts table: territory {territory} has no claim point")]
    NoClaimPoint {
        /// The fort's territory.
        territory: u16,
    },
}

#[derive(Deserialize)]
struct TableFile {
    forts: Vec<FortRow>,
}

#[derive(Deserialize)]
struct FortRow {
    territory: u16,
    frame: [f32; 4],
    arrivals: Vec<ArrivalRow>,
    siege: SiegeRow,
    points: Vec<PointRow>,
}

#[derive(Deserialize)]
struct ArrivalRow {
    side: u8,
    position: [f32; 3],
}

#[derive(Deserialize)]
struct SiegeRow {
    asset: String,
    gde_ref: String,
    card_component: u64,
    state_component: u64,
}

#[derive(Deserialize)]
struct PointRow {
    number: u32,
    asset: String,
    gde_ref: String,
    facet: String,
    place: [f32; 3],
    kind: u8,
    position: String,
    card_component: u64,
    position_component: u64,
    ownership_component: u64,
    point_component: u64,
}

/// Where a player of a side arrived in a war instance (Observed: the capture player's first place
/// inside the frame after the instance's 1635).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrival {
    /// [`SIDE_ATTACKER`] or [`SIDE_DEFENDER`].
    pub side: u8,
    /// Metres: east, north, up.
    pub position: [f32; 3],
}

/// The entity of a fort's siege warfare data (4103).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiegeEntity {
    /// Its slice asset.
    pub asset: [u8; ID_LEN],
    /// Its `GdeRef`.
    pub gde_ref: [u8; ID_LEN],
    /// The component of its card.
    pub card_component: u64,
    /// The component of its 4103 state.
    pub state_component: u64,
}

/// One capture point of a fort.
#[derive(Debug, Clone, PartialEq)]
pub struct FortPoint {
    /// Its number: 1 to 3 the outer points, [`CLAIM_POINT_NUMBER`] the claim point.
    pub number: u32,
    /// Its slice asset.
    pub asset: [u8; ID_LEN],
    /// Its `GdeRef`.
    pub gde_ref: [u8; ID_LEN],
    /// The facet id of its 4062 and 4063.
    pub facet: u64,
    /// Its place (the 333 cold data's).
    pub place: [f32; 3],
    /// Its kind in its first 333 state of the instance.
    pub kind: u8,
    /// Its place state as one bundle member's bytes (component, class, body), as retail sent it.
    pub position: Vec<u8>,
    /// The component of its card.
    pub card_component: u64,
    /// The component of its place state (in `position`).
    pub position_component: u64,
    /// The component of its ownership state.
    pub ownership_component: u64,
    /// The component of its 333 state.
    pub point_component: u64,
}

impl FortPoint {
    /// The key of the point in the siege warfare data and in its 4062: the first eight bytes of
    /// its `GdeRef` as a little-endian `u64` (Observed in the six forts).
    pub fn key(&self) -> u64 {
        let mut key = [0; KEY_LEN];
        key.copy_from_slice(&self.gde_ref[..KEY_LEN]);
        u64::from_le_bytes(key)
    }

    /// Whether this is the fort's claim point.
    pub const fn is_claim(&self) -> bool {
        self.number == CLAIM_POINT_NUMBER
    }
}

/// One fort.
#[derive(Debug, Clone, PartialEq)]
pub struct WarFort {
    /// The territory id.
    pub territory: u16,
    /// The frame of its war instance: x, y, width, height.
    pub frame: [f32; 4],
    /// Where the players arrived, by side.
    pub arrivals: Vec<Arrival>,
    /// Its siege warfare data entity.
    pub siege: SiegeEntity,
    /// Its capture points, by number.
    pub points: Vec<FortPoint>,
}

impl WarFort {
    /// Where a player of `side` arrives: the first Observed arrival of its side, else the first
    /// Observed arrival of the fort (Inferred: the spawn camps of the sides are Unknown).
    pub fn arrival(&self, side: u8) -> Option<[f32; 3]> {
        self.arrivals
            .iter()
            .find(|arrival| arrival.side == side)
            .or_else(|| self.arrivals.first())
            .map(|arrival| arrival.position)
    }

    /// Whether `[x, y]` is inside the frame.
    pub fn frames(&self, x: f32, y: f32) -> bool {
        let [left, bottom, width, height] = self.frame;
        (left..left + width).contains(&x) && (bottom..bottom + height).contains(&y)
    }
}

/// The forts of the wars, by territory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WarForts {
    forts: Vec<WarFort>,
}

impl WarForts {
    /// The table of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`WarFortsError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, WarFortsError> {
        let path = war_forts_path(dir, client_build);
        let text =
            std::fs::read_to_string(&path).map_err(|source| WarFortsError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`WarFortsError`] when the text is not the table, or a fort has no claim point or
    /// no arrival.
    pub fn parse(text: &str) -> Result<Self, WarFortsError> {
        let file: TableFile = serde_json::from_str(text)?;
        let forts = file
            .forts
            .into_iter()
            .map(fort_of)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { forts })
    }

    /// The fort of `territory`, when a capture shows it.
    pub fn fort(&self, territory: u16) -> Option<&WarFort> {
        self.forts.iter().find(|fort| fort.territory == territory)
    }

    /// Every fort.
    pub fn iter(&self) -> impl Iterator<Item = &WarFort> {
        self.forts.iter()
    }

    /// The number of forts.
    pub fn len(&self) -> usize {
        self.forts.len()
    }

    /// Whether the table has no fort.
    pub fn is_empty(&self) -> bool {
        self.forts.is_empty()
    }
}

fn fort_of(row: FortRow) -> Result<WarFort, WarFortsError> {
    let territory = row.territory;
    let bytes =
        |text: &str| hex::decode(text).map_err(|source| WarFortsError::Hex { territory, source });
    let id = |text: &str| -> Result<[u8; ID_LEN], WarFortsError> {
        let bytes = bytes(text)?;
        let len = bytes.len();
        bytes
            .try_into()
            .map_err(|_| WarFortsError::Len { territory, len })
    };
    let facet = |text: &str| -> Result<u64, WarFortsError> {
        let bytes = bytes(text)?;
        let len = bytes.len();
        let bytes: [u8; KEY_LEN] = bytes
            .try_into()
            .map_err(|_| WarFortsError::Len { territory, len })?;
        Ok(u64::from_be_bytes(bytes))
    };
    let mut points = row
        .points
        .into_iter()
        .map(|point| {
            Ok(FortPoint {
                number: point.number,
                asset: id(&point.asset)?,
                gde_ref: id(&point.gde_ref)?,
                facet: facet(&point.facet)?,
                place: point.place,
                kind: point.kind,
                position: bytes(&point.position)?,
                card_component: point.card_component,
                position_component: point.position_component,
                ownership_component: point.ownership_component,
                point_component: point.point_component,
            })
        })
        .collect::<Result<Vec<_>, WarFortsError>>()?;
    points.sort_by_key(|point| point.number);
    if !points.iter().any(FortPoint::is_claim) || row.arrivals.is_empty() {
        return Err(WarFortsError::NoClaimPoint { territory });
    }
    Ok(WarFort {
        territory,
        frame: row.frame,
        arrivals: row
            .arrivals
            .into_iter()
            .map(|arrival| Arrival {
                side: arrival.side,
                position: arrival.position,
            })
            .collect(),
        siege: SiegeEntity {
            asset: id(&row.siege.asset)?,
            gde_ref: id(&row.siege.gde_ref)?,
            card_component: row.siege.card_component,
            state_component: row.siege.state_component,
        },
        points,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A made-up fort in the table form: made-up ids, an outer point and the claim point.
    pub(crate) const TABLE: &str = r#"{"forts": [{
        "territory": 8,
        "ledger": "made-up",
        "frame": [5632.0, 5120.0, 512.0, 512.0],
        "arrivals": [{"side": 1, "position": [6001.5, 5291.0, 56.6], "ledger": "made-up", "unix_ms": 1}],
        "siege": {"asset": "11111111111111111111111111111111", "gde_ref": "22222222222222222222222222222222",
                  "card_component": 0, "state_component": 2},
        "points": [
          {"number": 4, "asset": "33333333333333333333333333333333",
           "gde_ref": "08070605040302011111111111111111", "facet": "0a0b0c0d0e0f0001",
           "place": [5902.48, 5428.12, 57.7], "kind": 6, "position": "020d010345b873d645a9a0f2",
           "card_component": 0, "position_component": 2, "ownership_component": 1, "point_component": 4},
          {"number": 1, "asset": "33333333333333333333333333333333",
           "gde_ref": "aabbccddeeff00112222222222222222", "facet": "0a0b0c0d0e0f0002",
           "place": [5972.2, 5363.1, 56.9], "kind": 4, "position": "020d0103",
           "card_component": 0, "position_component": 2, "ownership_component": 1, "point_component": 3}
        ]}]}"#;

    /// The retail key rule with made-up ids (`arvaysa_ES_attacker_war_45vs45` 1783537199924: the
    /// 4103 key and the 4062 entity of each point are its `GdeRef`'s first eight bytes reversed).
    #[test]
    fn a_fort_reads_with_its_points_in_number_order_and_their_keys() {
        let forts = WarForts::parse(TABLE).expect("table");
        assert_eq!(forts.len(), 1);
        let fort = forts.fort(8).expect("territory 8");
        assert_eq!(fort.points[0].number, 1, "sorted by number");
        let claim = &fort.points[1];
        assert!(claim.is_claim());
        assert_eq!(claim.key(), 0x0102_0304_0506_0708);
        assert_eq!(claim.facet, 0x0a0b_0c0d_0e0f_0001);
        assert_eq!(fort.points[0].key(), 0x1100_ffee_ddcc_bbaa);
        assert!(fort.frames(5902.48, 5428.12));
        assert!(!fort.frames(5631.0, 5428.12));
        assert_eq!(fort.arrival(SIDE_ATTACKER), Some([6001.5, 5291.0, 56.6]));
        assert_eq!(
            fort.arrival(SIDE_DEFENDER),
            Some([6001.5, 5291.0, 56.6]),
            "no defender arrival: the fort's first"
        );
        assert!(forts.fort(12).is_none(), "no capture shows that fort");
    }

    #[test]
    fn a_fort_without_a_claim_point_is_refused() {
        let table = TABLE.replace("\"number\": 4", "\"number\": 2");
        assert!(matches!(
            WarForts::parse(&table),
            Err(WarFortsError::NoClaimPoint { territory: 8 })
        ));
    }
}
