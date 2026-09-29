// SPDX-License-Identifier: AGPL-3.0-or-later
//! The world file (M1b spec section 3.3): the shared header of `nw_content::container` with
//! magic `NWWF`, then a postcard body holding one [`WorldFile`]. Errors are [`ContainerError`]s.

use std::path::{Path, PathBuf};

use nw_content::container::{
    ContainerError, ContainerSpec, decode_value, encode_value, read_file, write_file,
};

use crate::records::WorldFile;

/// First 4 bytes of every world file.
pub const WORLD_MAGIC: [u8; 4] = *b"NWWF";
/// Current world file format version.
pub const WORLD_FORMAT_VERSION: u16 = 1;
/// Version of the record types. Raise it when any type in `records` changes; the golden test
/// in this module fails until it is raised.
pub const WORLD_SCHEMA_VERSION: u32 = 6;
/// Header fields of the world file.
pub const WORLD_CONTAINER: ContainerSpec = ContainerSpec {
    magic: WORLD_MAGIC,
    format_version: WORLD_FORMAT_VERSION,
    schema_hash: WORLD_SCHEMA_VERSION,
};

/// The world file path of one level: `<dir>/world-<client_build>-<level>.bin`.
pub fn world_file_path(dir: &Path, client_build: &str, level: &str) -> PathBuf {
    dir.join(format!("world-{client_build}-{level}.bin"))
}

impl WorldFile {
    /// Encodes the world with its header.
    pub fn to_bytes(&self, client_build: &str) -> Result<Vec<u8>, ContainerError> {
        encode_value(&WORLD_CONTAINER, client_build, self)
    }

    /// Checks the header, then decodes the world.
    pub fn from_bytes(bytes: &[u8], expected_build: &str) -> Result<Self, ContainerError> {
        decode_value(&WORLD_CONTAINER, bytes, expected_build)
    }

    /// Writes the world file to `path`, creating parent folders.
    pub fn save(&self, path: &Path, client_build: &str) -> Result<(), ContainerError> {
        write_file(path, &self.to_bytes(client_build)?)
    }

    /// Reads and checks the world file at `path`.
    pub fn load(path: &Path, expected_build: &str) -> Result<Self, ContainerError> {
        Self::from_bytes(&read_file(path)?, expected_build)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nw_content::container::encode_container;

    use super::*;
    use crate::records::*;
    use crate::slice_data::SpawnedSlice;
    use crate::stations::{CraftingStation, StationGridSide, StationSlice};
    use crate::travel_slices::{HousingPlot, Teleporter};

    const BUILD: &str = "22469132";
    /// A spawned slice id with every byte set.
    const SPAWNED_ID: u64 = 0x8181_8181_8181_8187;
    const OTHER_BUILD: &str = "1";
    /// postcard body of [`sample`]. If this test fails, the record types changed: raise
    /// `WORLD_SCHEMA_VERSION` and replace these bytes with the new body.
    const GOLDEN_BODY: &[u8] = &[
        2, 108, 118, 128, 16, 140, 1, 141, 1, 0, 0, 32, 66, 1, 1, 130, 1, 131, 1, 1, 4, 144, 1,
        145, 1, 1, 129, 128, 128, 128, 8, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 200, 1, 172, 2,
        1, 130, 128, 128, 128, 8, 131, 128, 128, 128, 8, 150, 1, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0,
        64, 64, 1, 1, 172, 2, 1, 132, 128, 128, 128, 8, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64,
        0, 1, 172, 2, 1, 133, 128, 128, 128, 8, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 1, 172,
        2, 1, 201, 1, 100, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 0, 0, 192, 63, 1, 172, 2, 1,
        1, 101, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 1, 172, 2, 1, 1, 99, 0, 0, 128, 63, 0, 0,
        0, 64, 0, 0, 64, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 1, 0, 0, 0, 63, 1,
        115, 1, 1, 118, 1, 97, 1, 135, 131, 134, 140, 152, 176, 224, 192, 129, 1, 1, 120, 1, 119,
        0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 1, 135, 131, 134, 140, 152, 176, 224, 192, 129,
        1, 1, 120, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 1, 116, 1, 110, 1, 1, 5, 1, 115, 1, 9,
        1, 1, 109, 1, 8, 1, 1, 3, 1, 7, 1, 1, 1, 1, 1, 6, 1, 1, 110, 1, 1, 107, 1, 1, 112, 1, 0, 1,
        0, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0, 1, 1, 114, 1, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64,
        1, 5, 1, 1, 1, 14, 80, 79, 73, 95, 71, 114, 97, 122, 101, 95, 76, 79, 79, 80, 0, 0, 112,
        65, 0, 0, 52, 66, 0, 1, 1, 1, 112, 1, 1, 114, 1, 135, 131, 134, 140, 152, 176, 224, 192,
        129, 1, 1, 120, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 1, 104, 1, 1, 99, 0, 0, 128, 63,
        0, 0, 0, 64, 0, 0, 64, 64, 0, 0, 192, 63, 200, 1,
    ];

    /// One record of every kind. Every integer is 128 or more and every optional field is set,
    /// so that a change of integer width or of an `Option` changes the encoded bytes.
    fn sample() -> WorldFile {
        let position = [1.0, 2.0, 3.0];
        let territory_ids = vec![300];
        WorldFile {
            level: LevelInfo {
                name: "lv".to_owned(),
                region_size: 2048,
                grid_width: 140,
                grid_height: 141,
                ocean_level: 40.0,
                has_terrain: true,
            },
            regions: vec![Region {
                x: 130,
                y: 131,
                heights: Some(HeightGrid {
                    samples: vec![400, 401],
                }),
            }],
            npcs: vec![NpcPoint {
                npc_id: 0x8000_0001,
                position,
                faction_type: 200,
                territory_ids: territory_ids.clone(),
            }],
            vitals: vec![VitalsSpawn {
                vitals_id: 0x8000_0002,
                category_id: 0x8000_0003,
                level: 150,
                position,
                is_encounter: true,
                territory_ids: territory_ids.clone(),
            }],
            gatherables: vec![GatherablePoint {
                gatherable_id: 0x8000_0004,
                position,
                is_encounter: false,
                territory_ids: territory_ids.clone(),
            }],
            lore: vec![LorePoint {
                lore_id: 0x8000_0005,
                position,
                territory_ids: territory_ids.clone(),
            }],
            landmarks: vec![Landmark {
                landmark_type: 201,
                landmark_data: "d".to_owned(),
                position,
                radius: 1.5,
                territory_ids: territory_ids.clone(),
            }],
            encounters: vec![EncounterPoint {
                encounter_id: "e".to_owned(),
                position,
                territory_ids,
            }],
            capitals: vec![Capital {
                id: "c".to_owned(),
                position,
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: Some(0.5),
                slice_name: "s".to_owned(),
                variant_name: Some("v".to_owned()),
                slice_asset_id: "a".to_owned(),
            }],
            spawned: vec![SpawnedSlice {
                id: SPAWNED_ID,
                slice: "x".to_owned(),
                variant: "w".to_owned(),
                position,
            }],
            stations: vec![CraftingStation {
                id: SPAWNED_ID,
                slice: "x".to_owned(),
                position,
                components: StationSlice {
                    station_type: "t".to_owned(),
                    name: "n".to_owned(),
                    buildable: Some((1, 5)),
                    structure: "s".to_owned(),
                    ownership: Some((9, 1)),
                    structure_name: "m".to_owned(),
                    obstruction: Some((8, true)),
                    territory: Some(3),
                    grid: Some((
                        7,
                        vec![StationGridSide {
                            side: 1,
                            points: 1,
                            active: true,
                        }],
                    )),
                    delayed_event: Some(6),
                },
            }],
            npc_keys: vec!["n".to_owned()],
            vitals_keys: vec!["k".to_owned()],
            ai_profiles: BTreeMap::from([(
                "p".to_owned(),
                AiProfile {
                    behaviour: AiBehaviour::Guard,
                    sight_m: Some(2.0),
                    ..AiProfile::default()
                },
            )]),
            paths: BTreeMap::from([(
                "r".to_owned(),
                PatrolPath {
                    points: vec![position],
                    waits_ms: vec![5],
                    mode: PathMode::BackAndForth,
                    tasks: vec![Some(WaypointTask {
                        task: "POI_Graze_LOOP".to_owned(),
                        duration_s: [15.0, 45.0],
                        align: false,
                    })],
                },
            )]),
            vitals_ai: vec![SpawnAi {
                profile: Some("p".to_owned()),
                path: Some("r".to_owned()),
            }],
            plots: vec![HousingPlot {
                id: SPAWNED_ID,
                slice: "x".to_owned(),
                position,
                house_type: "h".to_owned(),
            }],
            teleporters: vec![Teleporter {
                capital: "c".to_owned(),
                destination: position,
                yaw_rad: 1.5,
                channel: 200,
            }],
        }
    }

    #[test]
    fn body_matches_the_golden_bytes() {
        assert_eq!(postcard::to_allocvec(&sample()).unwrap(), GOLDEN_BODY);
    }

    #[test]
    fn round_trip_keeps_everything() {
        let bytes = sample().to_bytes(BUILD).unwrap();
        assert_eq!(&bytes[..WORLD_MAGIC.len()], &WORLD_MAGIC);
        assert_eq!(WorldFile::from_bytes(&bytes, BUILD).unwrap(), sample());
    }

    #[test]
    fn other_build_is_refused() {
        let bytes = sample().to_bytes(BUILD).unwrap();
        assert!(matches!(
            WorldFile::from_bytes(&bytes, OTHER_BUILD),
            Err(ContainerError::BuildMismatch { .. })
        ));
    }

    #[test]
    fn bundle_is_not_a_world_file() {
        let spec = ContainerSpec {
            magic: *b"NWCB",
            ..WORLD_CONTAINER
        };
        let bytes = encode_container(&spec, BUILD, &[]).unwrap();
        assert!(matches!(
            WorldFile::from_bytes(&bytes, BUILD),
            Err(ContainerError::BadMagic { .. })
        ));
    }

    #[test]
    fn save_and_load_use_the_level_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = world_file_path(dir.path(), BUILD, "lv");
        assert!(path.ends_with("world-22469132-lv.bin"));
        sample().save(&path, BUILD).unwrap();
        assert_eq!(WorldFile::load(&path, BUILD).unwrap(), sample());
    }
}
