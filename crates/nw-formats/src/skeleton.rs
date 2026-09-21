// SPDX-License-Identifier: MIT OR Apache-2.0
//! A character's skeleton (`.chr`, a [`crate::chunk_file`]) and the pose of its joints in a clip
//! (layout Observed in build 25810808, 2026-09-18).
//!
//! The compiled bones chunk (`0x2000`, version `0x800`) holds 32 reserved bytes, then one 584-byte
//! record per joint: the joint's controller id (the CRC-32 of its name as written, the id of its
//! track in a clip), two physics blocks, its mass, its world-to-bone and bone-to-world matrices in
//! the bind pose (row-major 3 x 4, the translation in the last column), its name (256 bytes), its
//! limb id, the record offset of its parent (0 for the root), its child count and the record
//! offset of its first child. Observed: the player's 130 joints from `Xform` down, each parent
//! before its children.
//!
//! A clip's tracks are local to the parent joint (Observed: `grtswrd_prim_c1` puts the player's
//! head 1.47 m and its feet 0.14 m above the root, and moves `Xform` 1.586 m forward, the clip's
//! own `m_fDistance`). A joint without a track in a clip keeps its bind pose relative to its
//! parent.

use crate::caf::Caf;
use crate::chunk_file::{ChunkFileError, chunks, f32_at, u32_at};
use crate::rigid::{Rigid, quat_from_rows};

/// The compiled bones chunk.
const COMPILED_BONES: u16 = 0x2000;
const COMPILED_BONES_VERSION: u16 = 0x800;
const RESERVED_LEN: usize = 32;
const BONE_LEN: usize = 584;
const BONE_TO_WORLD_AT: usize = 264;
const NAME_AT: usize = 312;
const NAME_LEN: usize = 256;
const PARENT_AT: usize = 572;
const MATRIX_ROWS: usize = 3;
const MATRIX_COLUMNS: usize = 4;
const F32_LEN: usize = 4;
/// The translation column of a 3 x 4 matrix.
const TRANSLATION_COLUMN: usize = 3;

/// A skeleton that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkeletonError {
    /// The chunk file cannot be read.
    #[error(transparent)]
    ChunkFile(#[from] ChunkFileError),
    /// The file has no compiled bones of version `0x800`.
    #[error("skeleton has no compiled bones")]
    NoBones,
    /// A joint's parent is not a joint before it.
    #[error("joint {joint} has no parent before it")]
    BadParent {
        /// The joint's index.
        joint: usize,
    },
}

/// One joint.
#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    /// Its name.
    pub name: String,
    /// Its controller id: the id of its track in a clip.
    pub controller_id: u32,
    /// Its parent's index; `None` for the root.
    pub parent: Option<usize>,
    /// Its bind pose in model space.
    pub bind_model: Rigid,
    /// Its bind pose relative to its parent (model space for the root).
    pub bind_local: Rigid,
}

/// A character's joints, each parent before its children.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Skeleton {
    /// The joints.
    pub joints: Vec<Joint>,
}

fn bind_model(record: &[u8]) -> Result<Rigid, ChunkFileError> {
    let mut matrix = [[0.0_f32; MATRIX_COLUMNS]; MATRIX_ROWS];
    for (row, values) in matrix.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = f32_at(
                record,
                BONE_TO_WORLD_AT + (row * MATRIX_COLUMNS + column) * F32_LEN,
            )?;
        }
    }
    let rows = matrix.map(|row| [row[0], row[1], row[2]]);
    Ok(Rigid {
        rotation: quat_from_rows(rows),
        translation: matrix.map(|row| row[TRANSLATION_COLUMN]),
    })
}

impl Skeleton {
    /// Reads a `.chr` skeleton.
    ///
    /// # Errors
    ///
    /// Returns [`SkeletonError`] when the chunk file or its bones cannot be read.
    pub fn parse(bytes: &[u8]) -> Result<Self, SkeletonError> {
        let data = chunks(bytes)?
            .into_iter()
            .find(|chunk| chunk.kind == COMPILED_BONES && chunk.version == COMPILED_BONES_VERSION)
            .ok_or(SkeletonError::NoBones)?
            .data;
        let records = data.get(RESERVED_LEN..).unwrap_or_default();
        let mut joints: Vec<Joint> = Vec::new();
        for (index, record) in records.chunks_exact(BONE_LEN).enumerate() {
            let name_bytes = &record[NAME_AT..NAME_AT + NAME_LEN];
            let end = name_bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(NAME_LEN);
            let name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();
            let offset = i32::from_le_bytes(u32_at(record, PARENT_AT)?.to_le_bytes());
            let parent = if offset == 0 {
                None
            } else {
                let parent = isize::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_add(isize::try_from(offset).ok()?))
                    .and_then(|parent| usize::try_from(parent).ok())
                    .filter(|parent| *parent < index)
                    .ok_or(SkeletonError::BadParent { joint: index })?;
                Some(parent)
            };
            let model = bind_model(record)?;
            let local = parent.map_or(model, |parent| {
                joints[parent].bind_model.inverse().then_local(&model)
            });
            joints.push(Joint {
                name,
                controller_id: u32_at(record, 0)?,
                parent,
                bind_model: model,
                bind_local: local,
            });
        }
        Ok(Self { joints })
    }

    /// The index of the joint named `name`, any case (the client finds joints by the lower-case
    /// CRC-32 of the name: `FUN_146dc0770` hashes `right_hand_attach` lower case).
    pub fn joint(&self, name: &str) -> Option<usize> {
        self.joints
            .iter()
            .position(|joint| joint.name.eq_ignore_ascii_case(name))
    }

    /// Every joint's model-space pose at `frame` of `clip`: a joint's track where it has one, else
    /// its bind pose, relative to its parent.
    pub fn pose(&self, clip: &Caf, frame: f32) -> Vec<Rigid> {
        let mut model: Vec<Rigid> = Vec::with_capacity(self.joints.len());
        for joint in &self.joints {
            let track = clip.tracks.get(&joint.controller_id);
            let local = Rigid {
                rotation: track
                    .and_then(|track| track.rotation_at(frame))
                    .unwrap_or(joint.bind_local.rotation),
                translation: track
                    .and_then(|track| track.position_at(frame))
                    .unwrap_or(joint.bind_local.translation),
            };
            let pose = joint
                .parent
                .and_then(|parent| model.get(parent))
                .map_or(local, |parent| parent.then_local(&local));
            model.push(pose);
        }
        model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caf::Track;
    use crate::chunk_file::chunk_file;
    use crate::rigid::{IDENTITY_QUAT, length, quat_about_z, sub};

    fn record(id: u32, name: &str, parent_offset: i32, rows: [[f32; 4]; 3]) -> Vec<u8> {
        let mut record = vec![0_u8; BONE_LEN];
        record[..4].copy_from_slice(&id.to_le_bytes());
        for (row, values) in rows.iter().enumerate() {
            for (column, value) in values.iter().enumerate() {
                let at = BONE_TO_WORLD_AT + (row * MATRIX_COLUMNS + column) * F32_LEN;
                record[at..at + F32_LEN].copy_from_slice(&value.to_le_bytes());
            }
        }
        record[NAME_AT..NAME_AT + name.len()].copy_from_slice(name.as_bytes());
        record[PARENT_AT..PARENT_AT + 4].copy_from_slice(&parent_offset.to_le_bytes());
        record
    }

    /// `Xform`, `Pelvis` 1.073 m up and `Hips` turned, as in `player_male.chr`.
    fn skeleton_bytes() -> Vec<u8> {
        let mut data = vec![0_u8; RESERVED_LEN];
        data.extend(record(
            0xbd8d_5144,
            "Xform",
            0,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
        ));
        data.extend(record(
            0xb021_fd58,
            "Pelvis",
            -1,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, -0.023],
                [0.0, 0.0, 1.0, 1.073],
            ],
        ));
        data.extend(record(
            0xded1_0611,
            "Hips",
            -1,
            [
                [0.0, 0.0, -1.0, 0.0],
                [0.0, 1.0, 0.0, -0.023],
                [1.0, 0.0, 0.0, 1.071],
            ],
        ));
        chunk_file(&[(COMPILED_BONES, COMPILED_BONES_VERSION, &data)])
    }

    #[test]
    fn a_skeleton_has_its_joints_and_parents() {
        let skeleton = Skeleton::parse(&skeleton_bytes()).expect("skeleton");
        assert_eq!(skeleton.joints.len(), 3);
        assert_eq!(skeleton.joints[1].parent, Some(0));
        assert_eq!(skeleton.joints[2].parent, Some(1));
        assert_eq!(skeleton.joint("xform"), Some(0), "any case");
        let hips = &skeleton.joints[2].bind_local;
        assert!(length(sub(hips.translation, [0.0, 0.0, -0.002])) < 1e-5);
    }

    #[test]
    fn a_pose_moves_the_children_with_the_root() {
        let skeleton = Skeleton::parse(&skeleton_bytes()).expect("skeleton");
        let mut clip = Caf {
            ticks_per_frame: 1,
            seconds_per_tick: 1.0 / 30.0,
            start_frame: 0,
            end_frame: 10,
            ..Caf::default()
        };
        clip.tracks.insert(
            0xbd8d_5144,
            Track {
                rotations: vec![
                    (0.0, IDENTITY_QUAT),
                    (10.0, quat_about_z(std::f32::consts::PI)),
                ],
                positions: vec![(0.0, [0.0; 3]), (10.0, [0.0, 2.0, 0.0])],
            },
        );
        let start = skeleton.pose(&clip, 0.0);
        assert!(length(sub(start[2].translation, [0.0, -0.023, 1.071])) < 1e-4);
        let end = skeleton.pose(&clip, 10.0);
        // The root turned half a turn and moved 2 m: the pelvis' -0.023 offset is now +0.023.
        assert!(
            length(sub(end[1].translation, [0.0, 2.023, 1.073])) < 1e-4,
            "{:?}",
            end[1]
        );
    }

    #[test]
    fn a_file_without_bones_is_an_error() {
        let bytes = chunk_file(&[(1, 1, &[0; 4])]);
        assert_eq!(Skeleton::parse(&bytes), Err(SkeletonError::NoBones));
        let mut data = vec![0_u8; RESERVED_LEN];
        data.extend(record(
            1,
            "a",
            5,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
        ));
        let bytes = chunk_file(&[(COMPILED_BONES, COMPILED_BONES_VERSION, &data)]);
        assert_eq!(
            Skeleton::parse(&bytes),
            Err(SkeletonError::BadParent { joint: 0 })
        );
    }
}
