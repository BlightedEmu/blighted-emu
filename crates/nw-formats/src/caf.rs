// SPDX-License-Identifier: MIT OR Apache-2.0
//! The client's animation clips (`.caf`, a [`crate::chunk_file`]; layout Observed in
//! build 25810808, 2026-09-18).
//!
//! - Motion parameters (chunk `0x3002`, version `0x925`): asset flags, compression, ticks per
//!   frame, seconds per tick, first and last frame. All 5,948 player clips have 1 tick per frame
//!   at 1/30, 1/24 or 1/60 s.
//! - One controller per joint (chunk `0x100d`, version `0x831`): a 20-byte head (the joint's
//!   controller id, the CRC-32 of its name as written; flags; rotation and position key counts;
//!   rotation format, rotation time format, position format, whether positions have their own
//!   times, position time format, whether tracks are 4-byte aligned; two pad bytes), then the
//!   rotation keys, their times, the position keys and their own times. Sizes Observed: the parts
//!   fill all 380,786 controllers of the player clips.
//! - Rotation formats Observed: `5`, three 15-bit parts and a 2-bit index of the left-out part
//!   (48 bits), and `8`, two 21-bit and one 20-bit parts and a 2-bit index (64 bits). Each kept
//!   part is `value / ((2^bits - 1) / sqrt 2) - 1 / sqrt 2`, the parts fill x, y, z, w from the
//!   low bits up skipping the left-out one, which is `sqrt(1 - sum of squares)` (Inferred from
//!   smoothness: consecutive keys of the greatsword and sword clips agree to a mean |dot| of 0.997
//!   in this order, 0.978 to 0.987 in the others; the format-8 widths fit with a worst |dot| of
//!   0.976, the other widths fall to 0.004).
//! - Time formats Observed: `1`, `u16` frames, and `2`, `u8` frames; `0` (`f32` frames) is the
//!   engine's plain form. Position format Observed: `2`, three `f32`.

use std::collections::HashMap;

use crate::chunk_file::{ChunkFileError, chunks, f32_at, u16_at, u32_at};
use crate::rigid::{Quat, Vec3, lerp, slerp};

/// The motion parameters chunk.
const MOTION_PARAMS: u16 = 0x3002;
const MOTION_PARAMS_VERSION: u16 = 0x925;
const TICKS_PER_FRAME_AT: usize = 8;
const SECONDS_PER_TICK_AT: usize = 12;
const START_AT: usize = 16;
const END_AT: usize = 20;
/// The controller chunk.
const CONTROLLER: u16 = 0x100d;
const CONTROLLER_VERSION: u16 = 0x831;
const CONTROLLER_HEAD_LEN: usize = 20;
const ROTATION_COUNT_AT: usize = 8;
const POSITION_COUNT_AT: usize = 10;
const ROTATION_FORMAT_AT: usize = 12;
const ROTATION_TIME_AT: usize = 13;
const POSITION_FORMAT_AT: usize = 14;
const POSITION_KEYS_INFO_AT: usize = 15;
const POSITION_TIME_AT: usize = 16;
const ALIGNED_AT: usize = 17;
/// Positions with their own times (else they share the rotation times).
const OWN_POSITION_TIMES: u8 = 1;
/// Rotation formats.
const ROTATION_48_BIT: u8 = 5;
const ROTATION_64_BIT_EXT: u8 = 8;
const ROTATION_48_BIT_LEN: usize = 6;
const ROTATION_64_BIT_LEN: usize = 8;
const WIDTHS_48_BIT: [u32; 3] = [15, 15, 15];
const WIDTHS_64_BIT_EXT: [u32; 3] = [21, 21, 20];
const INDEX_SHIFT_48_BIT: u32 = 46;
const INDEX_SHIFT_64_BIT: u32 = 62;
const INDEX_MASK: u64 = 0b11;
/// Time formats.
const TIME_F32: u8 = 0;
const TIME_U16: u8 = 1;
const TIME_U8: u8 = 2;
/// The position format.
const POSITION_VEC3: u8 = 2;
const VEC3_LEN: usize = 12;
const F32_LEN: usize = 4;
const U16_LEN: usize = 2;
/// Track alignment.
const ALIGN: usize = 4;
/// Parts of a quaternion.
const QUAT_PARTS: usize = 4;

/// A clip that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CafError {
    /// The chunk file cannot be read.
    #[error(transparent)]
    ChunkFile(#[from] ChunkFileError),
    /// The clip has no motion parameters of version `0x925`.
    #[error("clip has no motion parameters")]
    NoMotionParams,
    /// A controller uses a format this reader does not know.
    #[error("controller {controller:#010x} uses unknown {what} format {format}")]
    UnknownFormat {
        /// The controller id.
        controller: u32,
        /// Which format.
        what: &'static str,
        /// The format number.
        format: u8,
    },
}

/// One joint's keys.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    /// Rotation keys: frame and rotation, by frame.
    pub rotations: Vec<(f32, Quat)>,
    /// Position keys: frame and position, by frame.
    pub positions: Vec<(f32, Vec3)>,
}

/// The value at `frame` of `keys`: the first before them, the last after them, blended between.
fn sample<T: Copy>(keys: &[(f32, T)], frame: f32, blend: impl Fn(T, T, f32) -> T) -> Option<T> {
    let after = keys.partition_point(|(at, _)| *at <= frame);
    match (
        after.checked_sub(1).and_then(|i| keys.get(i)),
        keys.get(after),
    ) {
        (Some(&(from_at, from)), Some(&(to_at, to))) if to_at > from_at => {
            Some(blend(from, to, (frame - from_at) / (to_at - from_at)))
        }
        (Some(&(_, from)), _) => Some(from),
        (None, next) => next.map(|(_, value)| *value),
    }
}

impl Track {
    /// The rotation at `frame`; `None` when the track has no rotation keys.
    pub fn rotation_at(&self, frame: f32) -> Option<Quat> {
        sample(&self.rotations, frame, slerp)
    }

    /// The position at `frame`; `None` when the track has no position keys.
    pub fn position_at(&self, frame: f32) -> Option<Vec3> {
        sample(&self.positions, frame, lerp)
    }
}

/// An animation clip.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Caf {
    /// Ticks per frame.
    pub ticks_per_frame: i32,
    /// Seconds per tick.
    pub seconds_per_tick: f32,
    /// The first frame.
    pub start_frame: i32,
    /// The last frame.
    pub end_frame: i32,
    /// The joints' tracks, by controller id.
    pub tracks: HashMap<u32, Track>,
}

/// Reads key parts from a controller chunk.
struct Keys<'a> {
    data: &'a [u8],
    at: usize,
    aligned: bool,
}

impl Keys<'_> {
    fn skip_to_alignment(&mut self) {
        if self.aligned {
            self.at = self.at.div_ceil(ALIGN) * ALIGN;
        }
    }

    fn take(&mut self, len: usize) -> Result<&[u8], CafError> {
        let slice = self
            .data
            .get(self.at..self.at + len)
            .ok_or(ChunkFileError::Truncated { at: self.at })?;
        self.at += len;
        Ok(slice)
    }

    fn times(&mut self, controller: u32, format: u8, count: usize) -> Result<Vec<f32>, CafError> {
        let times = match format {
            TIME_U8 => self
                .take(count)?
                .iter()
                .map(|frame| f32::from(*frame))
                .collect(),
            TIME_U16 => {
                let bytes = self.take(count * U16_LEN)?;
                (0..count)
                    .map(|i| u16_at(bytes, i * U16_LEN).map(f32::from))
                    .collect::<Result<_, _>>()?
            }
            TIME_F32 => {
                let bytes = self.take(count * F32_LEN)?;
                (0..count)
                    .map(|i| f32_at(bytes, i * F32_LEN))
                    .collect::<Result<_, _>>()?
            }
            format => {
                return Err(CafError::UnknownFormat {
                    controller,
                    what: "time",
                    format,
                });
            }
        };
        self.skip_to_alignment();
        Ok(times)
    }
}

/// The quaternion of a smallest-three key `packed`: parts of `widths` bits from the low bits up,
/// the index of the left-out part at `index_shift`.
fn smallest_three(packed: u64, widths: [u32; 3], index_shift: u32) -> Quat {
    let left_out = usize::try_from((packed >> index_shift) & INDEX_MASK).unwrap_or_default();
    let half_range = std::f32::consts::FRAC_1_SQRT_2;
    let mut quat = [0.0_f32; QUAT_PARTS];
    let mut shift = 0;
    let mut widths = widths.iter();
    let mut squares = 0.0;
    for (part, slot) in quat.iter_mut().enumerate() {
        if part == left_out {
            continue;
        }
        let width = widths.next().copied().unwrap_or_default();
        let mask = (1_u64 << width) - 1;
        // A part of 21 bits or fewer is exact in an f64, and its share fits an f32.
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let share =
            (((packed >> shift) & mask) as f64 / (mask as f64 * f64::from(half_range))) as f32;
        *slot = share - half_range;
        squares += *slot * *slot;
        shift += width;
    }
    quat[left_out] = (1.0 - squares).max(0.0).sqrt();
    quat
}

fn rotations(
    keys: &mut Keys<'_>,
    controller: u32,
    format: u8,
    count: usize,
) -> Result<Vec<Quat>, CafError> {
    let (len, widths, index_shift) = match format {
        ROTATION_48_BIT => (ROTATION_48_BIT_LEN, WIDTHS_48_BIT, INDEX_SHIFT_48_BIT),
        ROTATION_64_BIT_EXT => (ROTATION_64_BIT_LEN, WIDTHS_64_BIT_EXT, INDEX_SHIFT_64_BIT),
        format => {
            return Err(CafError::UnknownFormat {
                controller,
                what: "rotation",
                format,
            });
        }
    };
    let bytes = keys.take(count * len)?;
    let found = bytes
        .chunks_exact(len)
        .map(|key| {
            let packed = key
                .iter()
                .rev()
                .fold(0_u64, |packed, byte| packed << u8::BITS | u64::from(*byte));
            smallest_three(packed, widths, index_shift)
        })
        .collect();
    keys.skip_to_alignment();
    Ok(found)
}

fn track(data: &[u8]) -> Result<(u32, Track), CafError> {
    let controller = u32_at(data, 0)?;
    let byte = |at: usize| {
        data.get(at)
            .copied()
            .ok_or(CafError::ChunkFile(ChunkFileError::Truncated { at }))
    };
    let rotation_count = usize::from(u16_at(data, ROTATION_COUNT_AT)?);
    let position_count = usize::from(u16_at(data, POSITION_COUNT_AT)?);
    let mut keys = Keys {
        data,
        at: CONTROLLER_HEAD_LEN,
        aligned: byte(ALIGNED_AT)? != 0,
    };
    let mut found = Track::default();
    let mut rotation_times = Vec::new();
    if rotation_count > 0 {
        let values = rotations(
            &mut keys,
            controller,
            byte(ROTATION_FORMAT_AT)?,
            rotation_count,
        )?;
        rotation_times = keys.times(controller, byte(ROTATION_TIME_AT)?, rotation_count)?;
        found.rotations = rotation_times.iter().copied().zip(values).collect();
    }
    if position_count > 0 {
        let format = byte(POSITION_FORMAT_AT)?;
        if format != POSITION_VEC3 {
            return Err(CafError::UnknownFormat {
                controller,
                what: "position",
                format,
            });
        }
        let bytes = keys.take(position_count * VEC3_LEN)?;
        let values: Vec<Vec3> = (0..position_count)
            .map(|i| {
                let at = i * VEC3_LEN;
                Ok([
                    f32_at(bytes, at)?,
                    f32_at(bytes, at + F32_LEN)?,
                    f32_at(bytes, at + 2 * F32_LEN)?,
                ])
            })
            .collect::<Result<_, ChunkFileError>>()?;
        keys.skip_to_alignment();
        let times = if byte(POSITION_KEYS_INFO_AT)? == OWN_POSITION_TIMES {
            keys.times(controller, byte(POSITION_TIME_AT)?, position_count)?
        } else {
            rotation_times
        };
        found.positions = times.into_iter().zip(values).collect();
    }
    Ok((controller, found))
}

impl Caf {
    /// Reads a `.caf` clip.
    ///
    /// # Errors
    ///
    /// Returns [`CafError`] when the chunk file, its motion parameters or a controller cannot be
    /// read.
    pub fn parse(bytes: &[u8]) -> Result<Self, CafError> {
        let mut clip = None;
        let mut tracks = HashMap::new();
        for chunk in chunks(bytes)? {
            match (chunk.kind, chunk.version) {
                (MOTION_PARAMS, MOTION_PARAMS_VERSION) => {
                    let data = chunk.data;
                    let int =
                        |at| u32_at(data, at).map(|value| i32::from_le_bytes(value.to_le_bytes()));
                    clip = Some(Self {
                        ticks_per_frame: int(TICKS_PER_FRAME_AT)?,
                        seconds_per_tick: f32_at(data, SECONDS_PER_TICK_AT)?,
                        start_frame: int(START_AT)?,
                        end_frame: int(END_AT)?,
                        tracks: HashMap::new(),
                    });
                }
                (CONTROLLER, CONTROLLER_VERSION) => {
                    let (controller, found) = track(chunk.data)?;
                    tracks.insert(controller, found);
                }
                _ => {}
            }
        }
        let mut clip = clip.ok_or(CafError::NoMotionParams)?;
        clip.tracks = tracks;
        Ok(clip)
    }

    /// Seconds per frame.
    pub fn frame_seconds(&self) -> f32 {
        // A tick count of a few frames is exact in an f32.
        #[allow(clippy::cast_precision_loss)]
        let ticks = self.ticks_per_frame as f32;
        ticks * self.seconds_per_tick
    }

    /// The clip's length in seconds: its frames from the first to the last.
    pub fn duration_s(&self) -> f32 {
        // A frame count of a clip is exact in an f32.
        #[allow(clippy::cast_precision_loss)]
        let frames = (self.end_frame - self.start_frame) as f32;
        frames * self.frame_seconds()
    }

    /// The frame `seconds` after the clip's start.
    pub fn frame_at(&self, seconds: f32) -> f32 {
        // As in `duration_s`.
        #[allow(clippy::cast_precision_loss)]
        let start = self.start_frame as f32;
        start + seconds / self.frame_seconds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_file::chunk_file;
    use crate::rigid::{dot, length};

    /// The motion parameters of `grtswrd_prim_c1.caf` (build 25810808): 1 tick per frame of 1/30
    /// s, frames 0 to 73.
    fn motion_params() -> Vec<u8> {
        let mut data = Vec::new();
        for word in [2_u32, 500, 1] {
            data.extend_from_slice(&word.to_le_bytes());
        }
        data.extend_from_slice(&(1.0_f32 / 30.0).to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&73_u32.to_le_bytes());
        data
    }

    /// The `Hips` controller head of `grtswrd_prim_c1.caf` with 2 rotation keys of format 5, byte
    /// times, 2 positions sharing the rotation times.
    fn controller(id: u32, rotations: &[[u8; 6]], positions: &[Vec3], own_times: bool) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&id.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&u16::try_from(rotations.len()).expect("n").to_le_bytes());
        data.extend_from_slice(&u16::try_from(positions.len()).expect("n").to_le_bytes());
        data.extend_from_slice(&[
            ROTATION_48_BIT,
            TIME_U8,
            POSITION_VEC3,
            u8::from(own_times),
            TIME_U8,
            1,
            0,
            0,
        ]);
        for key in rotations {
            data.extend_from_slice(key);
        }
        data.resize(data.len().div_ceil(ALIGN) * ALIGN, 0);
        let frames: Vec<u8> = (0..rotations.len())
            .map(|i| u8::try_from(i * 10).expect("frame"))
            .collect();
        data.extend_from_slice(&frames);
        data.resize(data.len().div_ceil(ALIGN) * ALIGN, 0);
        for position in positions {
            for part in position {
                data.extend_from_slice(&part.to_le_bytes());
            }
        }
        if own_times {
            data.extend_from_slice(&[0, 5]);
        }
        data
    }

    /// A 48-bit key: parts x, y, z (index 3, w left out) of `value` each.
    fn key_48(parts: [u16; 3], left_out: u64) -> [u8; 6] {
        let packed = u64::from(parts[0])
            | u64::from(parts[1]) << 15
            | u64::from(parts[2]) << 30
            | left_out << INDEX_SHIFT_48_BIT;
        let bytes = packed.to_le_bytes();
        [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]]
    }

    /// The part value of 0 in 15 bits: `(1 / sqrt 2) * (2^15 - 1) / sqrt 2`, rounded.
    const ZERO_15: u16 = 16384;

    #[test]
    fn a_clip_has_its_frames_and_tracks() {
        let identity = key_48([ZERO_15; 3], 3);
        let hips = controller(
            0xded1_0611,
            &[identity, identity],
            &[[0.0, 0.0, 1.0], [0.0, 1.0, 1.0]],
            false,
        );
        let root = controller(0xbd8d_5144, &[identity], &[[0.0; 3], [0.0, 1.6, 0.0]], true);
        let bytes = chunk_file(&[
            (MOTION_PARAMS, MOTION_PARAMS_VERSION, &motion_params()),
            (CONTROLLER, CONTROLLER_VERSION, &hips),
            (CONTROLLER, CONTROLLER_VERSION, &root),
        ]);
        let clip = Caf::parse(&bytes).expect("clip");
        assert!((clip.duration_s() - 73.0 / 30.0).abs() < 1e-5);
        assert!((clip.frame_at(0.5) - 15.0).abs() < 1e-4);
        let hips = &clip.tracks[&0xded1_0611];
        let q = hips.rotation_at(3.0).expect("rotation");
        assert!((q[3] - 1.0).abs() < 1e-4, "{q:?}");
        let halfway = hips.position_at(5.0).expect("position");
        assert!(
            (halfway[1] - 0.5).abs() < 1e-5,
            "times shared with the rotations"
        );
        let root = &clip.tracks[&0xbd8d_5144];
        assert_eq!(
            root.position_at(9.0),
            Some([0.0, 1.6, 0.0]),
            "own times 0 and 5"
        );
        assert_eq!(
            root.position_at(-1.0),
            Some([0.0; 3]),
            "before the first key"
        );
    }

    #[test]
    fn a_smallest_three_key_rebuilds_the_left_out_part() {
        // x at its largest stored value, y and z 0, w left out.
        let q = smallest_three(
            u64::from(u16::MAX >> 1)
                | u64::from(ZERO_15) << 15
                | u64::from(ZERO_15) << 30
                | 3 << 46,
            WIDTHS_48_BIT,
            INDEX_SHIFT_48_BIT,
        );
        assert!(
            (q[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-4,
            "{q:?}"
        );
        assert!(
            (q[3] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3,
            "{q:?}"
        );
        assert!((length([q[0], q[1], q[2]]).powi(2) + q[3] * q[3] - 1.0).abs() < 1e-4);
        // Index 0 left out: the first stored part is y.
        let y = smallest_three(
            u64::from(u16::MAX >> 1) | u64::from(ZERO_15) << 15 | u64::from(ZERO_15) << 30,
            WIDTHS_48_BIT,
            INDEX_SHIFT_48_BIT,
        );
        assert!(dot([y[1], 0.0, 0.0], [1.0, 0.0, 0.0]) > 0.7);
    }

    #[test]
    fn an_unknown_format_is_an_error() {
        let mut data = controller(7, &[key_48([ZERO_15; 3], 3)], &[], false);
        data[ROTATION_FORMAT_AT] = 3;
        let bytes = chunk_file(&[
            (MOTION_PARAMS, MOTION_PARAMS_VERSION, &motion_params()),
            (CONTROLLER, CONTROLLER_VERSION, &data),
        ]);
        assert_eq!(
            Caf::parse(&bytes),
            Err(CafError::UnknownFormat {
                controller: 7,
                what: "rotation",
                format: 3
            })
        );
        let bytes = chunk_file(&[(
            CONTROLLER,
            CONTROLLER_VERSION,
            &controller(7, &[], &[], false),
        )]);
        assert_eq!(Caf::parse(&bytes), Err(CafError::NoMotionParams));
    }
}
