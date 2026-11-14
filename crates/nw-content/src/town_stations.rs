// SPDX-License-Identifier: AGPL-3.0-or-later
//! The crafting and refining stations of the settlements that the retail server spawns (town
//! crafting stations study, 2026-11-14). None is a level object for the client and none is in
//! the world data: retail sends each one a spawn card of its tier's slice
//! (`slices/structures/settlements/stations/settlement_station_<craft>_t<n>`), its place and
//! the states of the slice's components, and a client with no card offers no use there (live
//! 2026-11-14: no station use in a town reached the server).
//!
//! The table file holds, mined from the retail captures (13,635 station groups of 415 ledgers,
//! the `townstations`, `stationslices` and `claimtiers` probes and `townstations.py` of the
//! capture tools):
//!
//! - one slot per craft and settlement: its territory, its upgrade (the claim progression's
//!   `SettlementProgressionCategory`, Blacksmithing 0 to Smelting 9), its place state, and
//!   whether it shows the `_alt` look at tier 5 (13 settlements x 10 crafts; every tier of a slot
//!   stands at the same place);
//! - one slice per upgrade, station type and look: its station type and name from the slice's
//!   components, its asset, and the states of its components as retail sent them.
//!
//! Which tier a slot shows (Observed): a crafting station (upgrades 0 to 4) is of station type
//! claim tier + 2, 6,598 of 6,785 sightings; a refining station (5 to 9) of claim tier + 3 at
//! most 5, 93 to 98 % per craft (its slices `_t2`, `_t3`, `_t5` are types 3, 4, 5). A slot whose
//! territory is found by that fit: every settlement fits one territory at 100 %.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::town_props::ASSET_ID_LEN;

const FILE_PREFIX: &str = "town-stations-";
const FILE_SUFFIX: &str = ".json";
/// The first refining upgrade (Carpentry 5 to Smelting 9).
const FIRST_REFINING_UPGRADE: u8 = 5;
/// A crafting station's type above its claim tier.
const CRAFTING_TYPE_ABOVE_TIER: u8 = 2;
/// A refining station's type above its claim tier.
const REFINING_TYPE_ABOVE_TIER: u8 = 3;
/// The highest station type of a settlement.
const TOP_STATION_TYPE: u8 = 5;

/// The table file of client build `client_build` in content directory `dir`.
pub fn town_stations_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// The station type a slot of upgrade `upgrade` shows at the claim tier `tier` (module doc).
pub fn station_tier(upgrade: u8, tier: u8) -> u8 {
    let above = if upgrade < FIRST_REFINING_UPGRADE {
        CRAFTING_TYPE_ABOVE_TIER
    } else {
        REFINING_TYPE_ABOVE_TIER
    };
    tier.saturating_add(above).min(TOP_STATION_TYPE)
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum TownStationsError {
    /// The file cannot be read.
    #[error("town stations table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table.
    #[error("town stations table: {0}")]
    Json(#[from] serde_json::Error),
    /// A hex cell does not read.
    #[error("town stations table: {what} {index}: {source}")]
    Hex {
        /// A slot or a slice.
        what: &'static str,
        /// Its place in the file.
        index: usize,
        /// Why.
        source: hex::FromHexError,
    },
    /// A slice's asset id is not 16 bytes.
    #[error("town stations table: slice {index}: an asset id of {len} bytes")]
    AssetLen {
        /// The slice's place in the file.
        index: usize,
        /// The bytes it has.
        len: usize,
    },
    /// A slice's station type does not end in its tier.
    #[error("town stations table: slice {index}: station type {station_type} names no tier")]
    Tier {
        /// The slice's place in the file.
        index: usize,
        /// Its station type.
        station_type: String,
    },
}

#[derive(Deserialize)]
struct TableFile {
    slots: Vec<SlotRow>,
    slices: Vec<SliceRow>,
}

#[derive(Deserialize)]
struct SlotRow {
    territory: u16,
    upgrade: u8,
    place: String,
    alt: bool,
}

#[derive(Deserialize)]
struct SliceRow {
    upgrade: u8,
    station_type: String,
    name: String,
    alt: bool,
    asset: String,
    members: Vec<String>,
}

/// One station place of a settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TownStationSlot {
    /// Its settlement's territory id.
    pub territory: u16,
    /// Its upgrade: the claim progression category that sets its tier.
    pub upgrade: u8,
    /// Its place state, as the bytes of one bundle member.
    pub place: Vec<u8>,
    /// It shows the `_alt` look at tier 5.
    pub alt: bool,
}

/// One station slice: what a slot of its upgrade shows at its tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TownStationSlice {
    /// Its upgrade.
    pub upgrade: u8,
    /// Its station type, as recipes name it (`Smelting4`).
    pub station_type: String,
    /// The tier its station type names.
    pub tier: u8,
    /// The name its interaction's answer carries (`@SmeltingT3_MasterName`).
    pub name: String,
    /// It is the `_alt` look.
    pub alt: bool,
    /// The slice asset's id.
    pub asset: [u8; ASSET_ID_LEN],
    /// The states of its components, each as the bytes of one bundle member.
    pub members: Vec<Vec<u8>>,
}

/// The settlements' station slots and slices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TownStations {
    slots: Vec<TownStationSlot>,
    slices: Vec<TownStationSlice>,
}

impl TownStations {
    /// The table of client build `client_build` in content directory `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`TownStationsError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, TownStationsError> {
        let path = town_stations_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| TownStationsError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`TownStationsError`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, TownStationsError> {
        let file: TableFile = serde_json::from_str(text)?;
        let slots = file
            .slots
            .iter()
            .enumerate()
            .map(|(index, row)| {
                Ok(TownStationSlot {
                    territory: row.territory,
                    upgrade: row.upgrade,
                    place: hex::decode(&row.place).map_err(|source| TownStationsError::Hex {
                        what: "slot",
                        index,
                        source,
                    })?,
                    alt: row.alt,
                })
            })
            .collect::<Result<_, TownStationsError>>()?;
        let slices = file
            .slices
            .iter()
            .enumerate()
            .map(|(index, row)| slice(index, row))
            .collect::<Result<_, TownStationsError>>()?;
        Ok(Self { slots, slices })
    }

    /// The slots.
    pub fn slots(&self) -> &[TownStationSlot] {
        &self.slots
    }

    /// The slices of upgrade `upgrade`, every tier and look.
    pub fn slices_of(&self, upgrade: u8) -> impl Iterator<Item = &TownStationSlice> {
        self.slices
            .iter()
            .filter(move |slice| slice.upgrade == upgrade)
    }

    /// The slice slot `slot` shows at the claim tier `tier`: the slice of its upgrade and
    /// station type ([`station_tier`]), in its look when the table has it, else in the plain one.
    pub fn slice_of(&self, slot: &TownStationSlot, tier: u8) -> Option<&TownStationSlice> {
        let wanted = station_tier(slot.upgrade, tier);
        let of_look = |alt: bool| {
            self.slices.iter().find(|slice| {
                slice.upgrade == slot.upgrade && slice.tier == wanted && slice.alt == alt
            })
        };
        of_look(slot.alt && wanted == TOP_STATION_TYPE).or_else(|| of_look(false))
    }
}

fn slice(index: usize, row: &SliceRow) -> Result<TownStationSlice, TownStationsError> {
    let bytes = |text: &str| {
        hex::decode(text).map_err(|source| TownStationsError::Hex {
            what: "slice",
            index,
            source,
        })
    };
    let asset = bytes(&row.asset)?;
    let asset = <[u8; ASSET_ID_LEN]>::try_from(asset.as_slice()).map_err(|_| {
        TownStationsError::AssetLen {
            index,
            len: asset.len(),
        }
    })?;
    let digits = row
        .station_type
        .trim_start_matches(|letter: char| !letter.is_ascii_digit());
    let tier = digits.parse().map_err(|_| TownStationsError::Tier {
        index,
        station_type: row.station_type.clone(),
    })?;
    Ok(TownStationSlice {
        upgrade: row.upgrade,
        station_type: row.station_type.clone(),
        tier,
        name: row.name.clone(),
        alt: row.alt,
        asset,
        members: row
            .members
            .iter()
            .map(|member| bytes(member))
            .collect::<Result<_, _>>()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Windsward smelter slot and two of its slices as the retail captures show them.
    const TABLE: &str = r#"{"slots": [
        {"territory": 6, "upgrade": 9, "place": "020d01034611c11f45284f5b164443f4ee", "alt": true}
      ], "slices": [
        {"upgrade": 9, "station_type": "Smelting4", "name": "@SmeltingT3_MasterName", "alt": false,
         "slice": "slices/structures/settlements/stations/settlement_station_smelting_t3.dynamicslice",
         "asset": "1170a97191f759ac97698f128ab0609a",
         "members": ["01ba180107050000017a00", "038b22010100", "04b22d010200000000"]},
        {"upgrade": 9, "station_type": "Smelting5", "name": "@SmeltingT5_MasterName", "alt": false,
         "asset": "569156eeb52b580fb63019368b00b4c1",
         "members": ["038b22010100"]}
    ]}"#;

    #[test]
    fn a_crafting_station_is_two_types_above_its_tier_and_a_refining_one_three() {
        assert_eq!(station_tier(0, 0), 2, "a new town's forge");
        assert_eq!(station_tier(0, 3), 5);
        assert_eq!(station_tier(9, 0), 3, "a new town's smelter");
        assert_eq!(station_tier(9, 2), 5);
        assert_eq!(station_tier(9, 3), 5, "no type above 5");
    }

    #[test]
    fn a_slot_shows_the_slice_of_its_tier_in_the_plain_look_when_no_alt_is_known() {
        let table = TownStations::parse(TABLE).expect("table");
        let slot = &table.slots()[0];
        assert_eq!(slot.place.len(), 17);
        let at_one = table.slice_of(slot, 1).expect("tier 1");
        assert_eq!(
            (at_one.station_type.as_str(), at_one.tier, at_one.members.len()),
            ("Smelting4", 4, 3)
        );
        assert_eq!(at_one.asset[0], 0x11);
        let top = table.slice_of(slot, 3).expect("tier 3");
        assert_eq!(top.station_type, "Smelting5", "no alt slice: the plain one");
        assert!(table.slice_of(slot, 0).is_none(), "no type-3 slice in this table");
        assert_eq!(table.slices_of(9).count(), 2);
        assert_eq!(table.slices_of(0).count(), 0);
    }

    #[test]
    fn a_station_type_without_a_tier_is_refused() {
        let table = TABLE.replace("Smelting4", "Smelting");
        assert!(matches!(
            TownStations::parse(&table),
            Err(TownStationsError::Tier { index: 0, .. })
        ));
    }
}
