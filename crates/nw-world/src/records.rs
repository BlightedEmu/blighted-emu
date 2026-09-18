// SPDX-License-Identifier: AGPL-3.0-or-later
//! The typed world data of one level (M1b spec section 4).

use serde::{Deserialize, Serialize};

/// A world position in metres: x, y, z (z up).
pub type Position = [f32; 3];

/// Everything M1b reads for one level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldFile {
    /// Level frame.
    pub level: LevelInfo,
    /// Regions, sorted by (y, x).
    pub regions: Vec<Region>,
    /// NPC points.
    pub npcs: Vec<NpcPoint>,
    /// Creature (vitals) spawn points.
    pub vitals: Vec<VitalsSpawn>,
    /// Gatherable points.
    pub gatherables: Vec<GatherablePoint>,
    /// Lore points.
    pub lore: Vec<LorePoint>,
    /// Landmarks.
    pub landmarks: Vec<Landmark>,
    /// Encounter points.
    pub encounters: Vec<EncounterPoint>,
    /// Placement records from `*.capitals.json`.
    pub capitals: Vec<Capital>,
}

/// Level frame from `tracts.json` and `terrain.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LevelInfo {
    /// Level folder name, for example `newworld_vitaeeterna`.
    pub name: String,
    /// Region edge length in metres (`tracts.json` `regionSize`).
    pub region_size: u32,
    /// Highest region x index + 1.
    pub grid_width: u32,
    /// Highest region y index + 1.
    pub grid_height: u32,
    /// `terrain.json` `oceanLevel`, metres.
    pub ocean_level: f32,
    /// True when the level has terrain with the verified height scale (M1b spec section 5).
    pub has_terrain: bool,
}

/// One region of the level grid.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// Region x index (`r_+XX_+YY`).
    pub x: u32,
    /// Region y index.
    pub y: u32,
    /// Ground height samples; `None` when the level has no terrain.
    pub heights: Option<HeightGrid>,
}

/// Raw heightmap samples of one region.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeightGrid {
    /// A square of raw samples (2048 a side in the open world, 1024 in the FTUE levels), row 0 =
    /// north edge, at [`crate::height::HEIGHT_UNITS_PER_METRE`]. Stored as little-endian bytes.
    #[serde(with = "le_u16_bytes")]
    pub samples: Vec<u16>,
}

/// An NPC point (`npcData` section).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NpcPoint {
    /// crc32 of the lower-case NPC id.
    pub npc_id: u32,
    /// World position.
    pub position: Position,
    /// Faction type number.
    pub faction_type: u8,
    /// Territory ids that contain the point.
    pub territory_ids: Vec<u16>,
}

/// A creature spawn point (vitals section).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VitalsSpawn {
    /// crc32 of the lower-case vitals id.
    pub vitals_id: u32,
    /// crc32 of the lower-case vitals category id.
    pub category_id: u32,
    /// Creature level.
    pub level: u32,
    /// World position.
    pub position: Position,
    /// True when the spawn belongs to an encounter.
    pub is_encounter: bool,
    /// Territory ids that contain the point.
    pub territory_ids: Vec<u16>,
}

/// A gatherable point (gatherables section).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GatherablePoint {
    /// crc32 of the lower-case gatherable id.
    pub gatherable_id: u32,
    /// World position.
    pub position: Position,
    /// True when the point belongs to an encounter.
    pub is_encounter: bool,
    /// Territory ids that contain the point.
    pub territory_ids: Vec<u16>,
}

/// A lore point (`loreData` section).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LorePoint {
    /// crc32 of the lower-case lore id.
    pub lore_id: u32,
    /// World position.
    pub position: Position,
    /// Territory ids that contain the point.
    pub territory_ids: Vec<u16>,
}

/// A landmark (landmarks section).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Landmark {
    /// Landmark type number.
    pub landmark_type: u8,
    /// Landmark data text.
    pub landmark_data: String,
    /// World position.
    pub position: Position,
    /// Radius in metres.
    pub radius: f32,
    /// Territory ids that contain the point.
    pub territory_ids: Vec<u16>,
}

/// An encounter point (encounters section).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EncounterPoint {
    /// Encounter id text.
    pub encounter_id: String,
    /// World position.
    pub position: Position,
    /// Territory ids that contain the point.
    pub territory_ids: Vec<u16>,
}

/// A placement record from `*.capitals.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capital {
    /// GUID text.
    pub id: String,
    /// World position (`worldPosition`).
    pub position: Position,
    /// Rotation quaternion x, y, z, w.
    pub rotation: [f32; 4],
    /// Uniform scale, when present.
    pub scale: Option<f32>,
    /// Slice path, or `<PLOT>`.
    pub slice_name: String,
    /// Variant name, when present.
    pub variant_name: Option<String>,
    /// Slice asset id text, `{GUID}:subIdHex`.
    pub slice_asset_id: String,
}

/// Number of records of each kind in a [`WorldFile`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecordCounts {
    /// Regions.
    pub regions: usize,
    /// Regions with a height grid.
    pub terrain_regions: usize,
    /// NPC points.
    pub npcs: usize,
    /// Creature spawn points.
    pub vitals: usize,
    /// Gatherable points.
    pub gatherables: usize,
    /// Lore points.
    pub lore: usize,
    /// Landmarks.
    pub landmarks: usize,
    /// Encounter points.
    pub encounters: usize,
    /// Capitals.
    pub capitals: usize,
}

impl WorldFile {
    /// Counts the records of each kind.
    pub fn counts(&self) -> RecordCounts {
        RecordCounts {
            regions: self.regions.len(),
            terrain_regions: self
                .regions
                .iter()
                .filter(|region| region.heights.is_some())
                .count(),
            npcs: self.npcs.len(),
            vitals: self.vitals.len(),
            gatherables: self.gatherables.len(),
            lore: self.lore.len(),
            landmarks: self.landmarks.len(),
            encounters: self.encounters.len(),
            capitals: self.capitals.len(),
        }
    }
}

impl std::ops::AddAssign for RecordCounts {
    fn add_assign(&mut self, other: Self) {
        self.regions += other.regions;
        self.terrain_regions += other.terrain_regions;
        self.npcs += other.npcs;
        self.vitals += other.vitals;
        self.gatherables += other.gatherables;
        self.lore += other.lore;
        self.landmarks += other.landmarks;
        self.encounters += other.encounters;
        self.capitals += other.capitals;
    }
}

/// Serializes `Vec<u16>` as one byte string of little-endian pairs, so a 2048 x 2048 grid is
/// 8 MiB in the file instead of up to 12 MiB of varints.
mod le_u16_bytes {
    use std::fmt;

    use serde::de::{self, Visitor};
    use serde::{Deserializer, Serializer};

    const PAIR_SIZE: usize = size_of::<u16>();

    pub fn serialize<S: Serializer>(samples: &[u16], serializer: S) -> Result<S::Ok, S::Error> {
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        serializer.serialize_bytes(&bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u16>, D::Error> {
        deserializer.deserialize_bytes(PairVisitor)
    }

    struct PairVisitor;

    impl Visitor<'_> for PairVisitor {
        type Value = Vec<u16>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("little-endian u16 pairs")
        }

        fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Vec<u16>, E> {
            if !bytes.len().is_multiple_of(PAIR_SIZE) {
                return Err(E::invalid_length(bytes.len(), &self));
            }
            Ok(bytes
                .chunks_exact(PAIR_SIZE)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ODD_LENGTH_BYTES: [u8; 2] = [1, 0xab];

    #[test]
    fn height_grid_is_stored_as_little_endian_pairs() {
        let grid = HeightGrid {
            samples: vec![0x0102, 0xa0b0],
        };
        let bytes = postcard::to_allocvec(&grid).unwrap();
        // postcard writes the byte length as a varint, then the bytes.
        assert_eq!(bytes, [4, 0x02, 0x01, 0xb0, 0xa0]);
        assert_eq!(postcard::from_bytes::<HeightGrid>(&bytes).unwrap(), grid);
    }

    #[test]
    fn odd_height_bytes_are_refused() {
        assert!(postcard::from_bytes::<HeightGrid>(&ODD_LENGTH_BYTES).is_err());
    }

    #[test]
    fn counts_add_field_by_field() {
        let mut total = RecordCounts {
            regions: 1,
            vitals: 2,
            ..RecordCounts::default()
        };
        total += RecordCounts {
            regions: 3,
            terrain_regions: 1,
            npcs: 4,
            vitals: 5,
            gatherables: 6,
            lore: 7,
            landmarks: 8,
            encounters: 9,
            capitals: 10,
        };
        assert_eq!(
            total,
            RecordCounts {
                regions: 4,
                terrain_regions: 1,
                npcs: 4,
                vitals: 7,
                gatherables: 6,
                lore: 7,
                landmarks: 8,
                encounters: 9,
                capitals: 10,
            }
        );
    }

    #[test]
    fn counts_cover_every_list() {
        let world = WorldFile {
            regions: vec![
                Region::default(),
                Region {
                    heights: Some(HeightGrid::default()),
                    ..Region::default()
                },
            ],
            vitals: vec![VitalsSpawn::default(); 3],
            capitals: vec![Capital::default()],
            ..WorldFile::default()
        };
        assert_eq!(
            world.counts(),
            RecordCounts {
                regions: 2,
                terrain_regions: 1,
                vitals: 3,
                capitals: 1,
                ..RecordCounts::default()
            }
        );
    }
}
