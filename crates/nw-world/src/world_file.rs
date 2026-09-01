// SPDX-License-Identifier: AGPL-3.0-or-later
//! The world file (M1b spec section 3.3): the shared header of `nw_content::container` with
//! magic `NWWF`, then a postcard body holding one [`WorldFile`].

use std::path::{Path, PathBuf};

use nw_content::container::{
    ContainerError, ContainerSpec, decode_container, encode_container, read_file, write_file,
};

use crate::records::WorldFile;

/// First 4 bytes of every world file.
pub const WORLD_MAGIC: [u8; 4] = *b"NWWF";
/// Current world file format version.
pub const WORLD_FORMAT_VERSION: u16 = 1;
/// Version of the record types. Raise it when any type in `records` changes; the golden test
/// in this module fails until it is raised.
pub const WORLD_SCHEMA_VERSION: u32 = 1;
/// Header fields of the world file.
pub const WORLD_CONTAINER: ContainerSpec = ContainerSpec {
    magic: WORLD_MAGIC,
    format_version: WORLD_FORMAT_VERSION,
    schema_hash: WORLD_SCHEMA_VERSION,
};

/// Error from world file reading or writing.
#[derive(Debug, thiserror::Error)]
pub enum WorldError {
    /// The header is not valid or the file cannot be accessed.
    #[error("world file: {0}")]
    Container(#[from] ContainerError),
    /// The world cannot be encoded.
    #[error("cannot encode world file: {0}")]
    Encode(postcard::Error),
    /// The body cannot be decoded.
    #[error("cannot decode world file: {0}")]
    Decode(postcard::Error),
    /// The body has bytes after the encoded world.
    #[error("world file body has {count} bytes after the content")]
    TrailingBytes {
        /// Number of extra bytes.
        count: usize,
    },
}

/// The world file path of one level: `<dir>/world-<client_build>-<level>.bin`.
pub fn world_file_path(dir: &Path, client_build: &str, level: &str) -> PathBuf {
    dir.join(format!("world-{client_build}-{level}.bin"))
}

impl WorldFile {
    /// Encodes the world with its header.
    pub fn to_bytes(&self, client_build: &str) -> Result<Vec<u8>, WorldError> {
        let body = postcard::to_allocvec(self).map_err(WorldError::Encode)?;
        Ok(encode_container(&WORLD_CONTAINER, client_build, &body)?)
    }

    /// Checks the header, then decodes the world.
    pub fn from_bytes(bytes: &[u8], expected_build: &str) -> Result<Self, WorldError> {
        let body = decode_container(&WORLD_CONTAINER, bytes, expected_build)?;
        let (world, trailing): (Self, _) =
            postcard::take_from_bytes(body).map_err(WorldError::Decode)?;
        if !trailing.is_empty() {
            return Err(WorldError::TrailingBytes {
                count: trailing.len(),
            });
        }
        Ok(world)
    }

    /// Writes the world file to `path`, creating parent folders.
    pub fn save(&self, path: &Path, client_build: &str) -> Result<(), WorldError> {
        Ok(write_file(path, &self.to_bytes(client_build)?)?)
    }

    /// Reads and checks the world file at `path`.
    pub fn load(path: &Path, expected_build: &str) -> Result<Self, WorldError> {
        Self::from_bytes(&read_file(path)?, expected_build)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::*;

    const BUILD: &str = "22469132";
    const OTHER_BUILD: &str = "1";
    const EXTRA_BYTE: u8 = 0;
    /// postcard body of [`sample`]. If this test fails, the record types changed: raise
    /// `WORLD_SCHEMA_VERSION` and replace these bytes with the new body.
    const GOLDEN_BODY: &[u8] = &[
        2, 108, 118, 128, 16, 1, 1, 0, 0, 32, 66, 1, 1, 0, 0, 1, 4, 1, 0, 2, 0, 1, 1, 0, 0, 128,
        63, 0, 0, 0, 64, 0, 0, 64, 64, 2, 1, 7, 1, 3, 4, 5, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64,
        64, 1, 1, 7, 1, 6, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 0, 1, 7, 1, 8, 0, 0, 128, 63,
        0, 0, 0, 64, 0, 0, 64, 64, 1, 7, 1, 9, 1, 100, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 0,
        0, 192, 63, 1, 7, 1, 1, 101, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 1, 7, 1, 1, 99, 0,
        0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63,
        1, 0, 0, 0, 63, 1, 115, 0, 1, 97,
    ];

    /// One record of every kind, with small values.
    fn sample() -> WorldFile {
        let position = [1.0, 2.0, 3.0];
        let territory_ids = vec![7];
        WorldFile {
            level: LevelInfo {
                name: "lv".to_owned(),
                region_size: 2048,
                grid_width: 1,
                grid_height: 1,
                ocean_level: 40.0,
                has_terrain: true,
            },
            regions: vec![Region {
                x: 0,
                y: 0,
                heights: Some(HeightGrid {
                    samples: vec![1, 2],
                }),
            }],
            npcs: vec![NpcPoint {
                npc_id: 1,
                position,
                faction_type: 2,
                territory_ids: territory_ids.clone(),
            }],
            vitals: vec![VitalsSpawn {
                vitals_id: 3,
                category_id: 4,
                level: 5,
                position,
                is_encounter: true,
                territory_ids: territory_ids.clone(),
            }],
            gatherables: vec![GatherablePoint {
                gatherable_id: 6,
                position,
                is_encounter: false,
                territory_ids: territory_ids.clone(),
            }],
            lore: vec![LorePoint {
                lore_id: 8,
                position,
                territory_ids: territory_ids.clone(),
            }],
            landmarks: vec![Landmark {
                landmark_type: 9,
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
                variant_name: None,
                slice_asset_id: "a".to_owned(),
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
    fn header_errors_come_from_the_container() {
        let bytes = sample().to_bytes(BUILD).unwrap();
        assert!(matches!(
            WorldFile::from_bytes(&bytes, OTHER_BUILD),
            Err(WorldError::Container(ContainerError::BuildMismatch { .. }))
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
            Err(WorldError::Container(ContainerError::BadMagic { .. }))
        ));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut body = postcard::to_allocvec(&sample()).unwrap();
        body.push(EXTRA_BYTE);
        let bytes = encode_container(&WORLD_CONTAINER, BUILD, &body).unwrap();
        assert!(matches!(
            WorldFile::from_bytes(&bytes, BUILD),
            Err(WorldError::TrailingBytes { count: 1 })
        ));
    }

    #[test]
    fn body_that_is_not_a_world_is_refused() {
        let bytes = encode_container(&WORLD_CONTAINER, BUILD, &[]).unwrap();
        assert!(matches!(
            WorldFile::from_bytes(&bytes, BUILD),
            Err(WorldError::Decode(_))
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
