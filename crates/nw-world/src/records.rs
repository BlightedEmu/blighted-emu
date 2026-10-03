// SPDX-License-Identifier: AGPL-3.0-or-later
//! The typed world data of one level (M1b spec section 4).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::interact_tags::InteractTagged;
use crate::slice_data::SpawnedSlice;
use crate::stations::CraftingStation;
use crate::travel_slices::{HousingPlot, Teleporter};

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
    /// The slices the capitals spawn (`region.slicedata`).
    pub spawned: Vec<SpawnedSlice>,
    /// The crafting stations among the spawned slices.
    pub stations: Vec<CraftingStation>,
    /// Stable design key of each NPC point, in the order of `npcs` (see [`point_keys`]).
    pub npc_keys: Vec<String>,
    /// Stable design key of each vitals spawn, in the order of `vitals` (see [`point_keys`]).
    pub vitals_keys: Vec<String>,
    /// The AI profiles of the design files, by id (admin studio spec section 6.5).
    pub ai_profiles: BTreeMap<String, AiProfile>,
    /// The patrol paths of this level's design files, by id (spec section 6.4).
    pub paths: BTreeMap<String, PatrolPath>,
    /// The profile and path of each vitals spawn, in the order of `vitals`; empty when no spawn
    /// has one.
    pub vitals_ai: Vec<SpawnAi>,
    /// The house plots among the spawned slices.
    pub plots: Vec<HousingPlot>,
    /// The teleporters among the capitals.
    pub teleporters: Vec<Teleporter>,
    /// The capitals and spawned slices whose slices hold interact tags.
    pub interact_tags: Vec<InteractTagged>,
}

/// What an awake mob does without a target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiBehaviour {
    /// Wanders near its spawn; attacks a player in sight.
    #[default]
    Wander,
    /// Stands at its spawn; attacks a player in sight.
    Guard,
    /// Walks its path; attacks a player in sight.
    Patrol,
    /// Stands at its spawn; fights only a player who hit it.
    Idle,
}

/// The brain numbers of a group of spawns; a missing number keeps the sim's `[combat]` value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AiProfile {
    /// What it does without a target.
    pub behaviour: AiBehaviour,
    /// A player this near wakes it, metres.
    pub wake_radius_m: Option<f32>,
    /// It sees a player this far, metres.
    pub sight_m: Option<f32>,
    /// It wanders this far from its spawn, metres.
    pub wander_radius_m: Option<f32>,
    /// It pauses this long between wander walks.
    pub wander_pause_ms: Option<u64>,
    /// It never goes farther than this from its leash centre, metres.
    pub leash_radius_m: Option<f32>,
    /// It hits a target this near, metres.
    pub attack_reach_m: Option<f32>,
    /// Time between its hits.
    pub attack_interval_ms: Option<u64>,
    /// Its run speed, metres per second.
    pub speed_mps: Option<f32>,
    /// Time from its death to its respawn.
    pub respawn_ms: Option<u64>,
}

/// How a path goes on after its last point.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathMode {
    /// Back to the first point.
    #[default]
    Loop,
    /// Back along the points, then forward again.
    BackAndForth,
    /// The mob stays at the last point.
    Once,
}

/// A patrol path.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PatrolPath {
    /// The points, in order.
    pub points: Vec<Position>,
    /// The wait at each point, ms (missing: no wait).
    pub waits_ms: Vec<u64>,
    /// What comes after the last point.
    pub mode: PathMode,
    /// The task at each point, from a spawner slice's waypoints (missing: no task).
    pub tasks: Vec<Option<WaypointTask>>,
}

/// What a mob does at a waypoint (`AIWaypointComponent`): the `Waypoint_Task`,
/// `Waypoint_Duration_Min`, `Waypoint_Duration_Max` and `Waypoint_Align` its tree's Patrol
/// sub-tree reads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WaypointTask {
    /// `m_taskName`: the fragment it plays (`POI_Graze_LOOP`).
    pub task: String,
    /// `m_minDuration` and `m_maxDuration`, seconds; negative in a waypoint without a duration.
    pub duration_s: [f32; 2],
    /// `m_matchAlignment`: it turns to the waypoint's facing.
    pub align: bool,
}

/// The AI profile and path ids of one spawn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnAi {
    /// An id of [`WorldFile::ai_profiles`].
    pub profile: Option<String>,
    /// An id of [`WorldFile::paths`].
    pub path: Option<String>,
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
    /// Spawned slices.
    pub spawned: usize,
    /// Crafting stations.
    pub stations: usize,
}

/// Metres of one step of a point key coordinate (0.1 m).
const KEY_STEPS_PER_METRE: f32 = 10.0;
/// Number of the first repeat of a point key (`_2`).
const FIRST_REPEAT: usize = 2;

/// The design key of each point, in input order: `<type id>_<x>_<y>_<z>`, every coordinate
/// rounded to 0.1 m with one decimal, and `_2`, `_3`, ... on later points with an equal key.
pub fn point_keys(points: impl IntoIterator<Item = (u32, Position)>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    points
        .into_iter()
        .map(|(type_id, [x, y, z])| {
            let key = format!(
                "{type_id}_{}_{}_{}",
                key_coordinate(x),
                key_coordinate(y),
                key_coordinate(z)
            );
            let count = seen.entry(key.clone()).or_insert(0);
            *count += 1;
            if *count < FIRST_REPEAT {
                key
            } else {
                format!("{key}_{count}")
            }
        })
        .collect()
}

fn key_coordinate(value: f32) -> String {
    // Adding 0.0 turns -0.0 into 0.0.
    let rounded = (value * KEY_STEPS_PER_METRE).round() / KEY_STEPS_PER_METRE + 0.0;
    format!("{rounded:.1}")
}

impl WorldFile {
    /// Sets `npc_keys` and `vitals_keys` from the current points.
    pub fn fill_point_keys(&mut self) {
        self.npc_keys = point_keys(self.npcs.iter().map(|npc| (npc.npc_id, npc.position)));
        self.vitals_keys = point_keys(
            self.vitals
                .iter()
                .map(|spawn| (spawn.vitals_id, spawn.position)),
        );
    }

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
            spawned: self.spawned.len(),
            stations: self.stations.len(),
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
        self.spawned += other.spawned;
        self.stations += other.stations;
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
    fn a_point_key_rounds_to_a_tenth() {
        assert_eq!(
            point_keys([(12, [100.04, -3.26, 7.0])]),
            ["12_100.0_-3.3_7.0"]
        );
    }

    #[test]
    fn negative_zero_prints_as_zero() {
        assert_eq!(point_keys([(1, [-0.01, 0.0, 0.0])]), ["1_0.0_0.0_0.0"]);
    }

    #[test]
    fn equal_keys_get_a_count() {
        let position = [5.0, 6.0, 7.0];
        assert_eq!(
            point_keys([(3, position), (3, position), (3, position)]),
            ["3_5.0_6.0_7.0", "3_5.0_6.0_7.0_2", "3_5.0_6.0_7.0_3"]
        );
    }

    #[test]
    fn fill_point_keys_keys_every_vitals_spawn_and_npc() {
        let mut world = WorldFile {
            npcs: vec![NpcPoint::default()],
            vitals: vec![VitalsSpawn::default(), VitalsSpawn::default()],
            ..WorldFile::default()
        };
        world.fill_point_keys();
        assert_eq!(world.npc_keys, ["0_0.0_0.0_0.0"]);
        assert_eq!(world.vitals_keys, ["0_0.0_0.0_0.0", "0_0.0_0.0_0.0_2"]);
    }

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
            spawned: 11,
            stations: 12,
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
                spawned: 11,
                stations: 12,
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
            spawned: vec![SpawnedSlice::default(); 2],
            ..WorldFile::default()
        };
        assert_eq!(
            world.counts(),
            RecordCounts {
                regions: 2,
                terrain_regions: 1,
                vitals: 3,
                capitals: 1,
                spawned: 2,
                ..RecordCounts::default()
            }
        );
    }
}
