// SPDX-License-Identifier: MIT OR Apache-2.0
//! Whether a static mesh (`.cgf`, a [`crate::chunk_file`] of version `0x746`) carries physics
//! geometry (level-collision note 2026-09-20, layout Observed in build 25810808): a
//! `MeshPhysicsData` chunk, or a mesh chunk that names one. The asset pipeline cooks exactly
//! these meshes into the client's physics asset (`.rnr`): in a sample of 112 meshes every one
//! with physics geometry had a `.rnr` and none without had one.

use crate::chunk_file::{ChunkFileError, chunks, u32_at};

/// The chunk type of a mesh.
const MESH_CHUNK: u16 = 0x1000;
/// The mesh chunk version this reader knows.
const MESH_VERSION: u16 = 0x802;
/// The chunk type of the physics geometry of a mesh.
const MESH_PHYSICS_DATA_CHUNK: u16 = 0x1018;
/// Where a version-`0x802` mesh chunk holds its four physics-data chunk ids: after seven `u32`
/// counts and ids and sixteen stream slots of eight `u32` chunk ids.
const PHYSICS_IDS_AT: usize = 7 * 4 + 16 * 8 * 4;
/// The physics-data chunk ids of a mesh chunk.
const PHYSICS_IDS: usize = 4;
const U32_LEN: usize = 4;

/// Whether the mesh file `bytes` carries physics geometry.
///
/// # Errors
///
/// Returns [`ChunkFileError`] when the bytes are not a chunk file or a mesh chunk is cut short.
pub fn has_physics_geometry(bytes: &[u8]) -> Result<bool, ChunkFileError> {
    for chunk in chunks(bytes)? {
        if chunk.kind == MESH_PHYSICS_DATA_CHUNK {
            return Ok(true);
        }
        if chunk.kind == MESH_CHUNK && chunk.version == MESH_VERSION {
            for slot in 0..PHYSICS_IDS {
                if u32_at(chunk.data, PHYSICS_IDS_AT + slot * U32_LEN)? != 0 {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_file::chunk_file;

    /// A version-`0x802` mesh chunk whose first physics-data id is `id`.
    fn mesh(id: u32) -> Vec<u8> {
        let mut data = vec![0; PHYSICS_IDS_AT + PHYSICS_IDS * U32_LEN];
        data[PHYSICS_IDS_AT..PHYSICS_IDS_AT + U32_LEN].copy_from_slice(&id.to_le_bytes());
        data
    }

    #[test]
    fn a_mesh_that_names_physics_data_has_physics_geometry() {
        let with = chunk_file(&[(MESH_CHUNK, MESH_VERSION, &mesh(28))]);
        assert_eq!(has_physics_geometry(&with), Ok(true));
        let without = chunk_file(&[(MESH_CHUNK, MESH_VERSION, &mesh(0))]);
        assert_eq!(has_physics_geometry(&without), Ok(false));
        let data_chunk = chunk_file(&[(MESH_PHYSICS_DATA_CHUNK, 0x800, &[0; 16])]);
        assert_eq!(has_physics_geometry(&data_chunk), Ok(true));
    }

    #[test]
    fn a_cut_mesh_chunk_is_an_error() {
        let cut = chunk_file(&[(MESH_CHUNK, MESH_VERSION, &[0; 8])]);
        assert!(matches!(
            has_physics_geometry(&cut),
            Err(ChunkFileError::Truncated { .. })
        ));
    }
}
