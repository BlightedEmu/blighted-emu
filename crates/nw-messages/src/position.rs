// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::PositionInTheWorldReplicatedState` on the bundle path (replication note section 2).
//! Reader `FUN_1417b4110`: a slot byte, a child bitmap, then the present children `position`
//! (`FUN_145cea190`), `rotation` (`FUN_14087b250`) and `scale` (`FUN_14087b0b0`).

use crate::wire::{MessageError, Reader, Writer};

/// Type index of the class.
pub const POSITION_IN_THE_WORLD_TYPE_INDEX: u64 = 13;
/// Height of a raw `z` of 0, in metres.
pub const POSITION_Z_MIN: f32 = -100.0;
/// Height of a raw `z` of `u16::MAX`, in metres.
pub const POSITION_Z_MAX: f32 = 2000.0;
/// Slot byte bit: slot 0 (the child bitmap) follows.
const SLOT_0: u8 = 0x01;
const CHILD_POSITION: u8 = 0x01;
const CHILD_ROTATION: u8 = 0x02;
const CHILD_SCALE: u8 = 0x04;
const KNOWN_CHILDREN: u8 = CHILD_POSITION | CHILD_ROTATION | CHILD_SCALE;
/// Rotation mask bit: `w` is negated.
const ROTATION_W_NEGATIVE: u8 = 0x40;
/// Rotation mask: the bit of component `c` (0, 1, 2) that means 0; the bit three higher means 1.
const ROTATION_ONE_SHIFT: u8 = 3;
const ROTATION_COMPONENTS: u8 = 3;
/// Rotation mask bit 7 has no read in the binary and was clear on every captured member.
const ROTATION_UNKNOWN: u8 = 0x80;
/// Scale tag: four values of 1, nothing read.
const SCALE_ONE: u8 = 0;
/// Scale tag: one binary16 for every axis.
const SCALE_UNIFORM: u8 = 1;

/// The `position` child: big-endian `f32` x and y, and a big-endian `u16` height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldPosition {
    /// East, in metres.
    pub x: f32,
    /// North, in metres.
    pub y: f32,
    /// Height, from [`POSITION_Z_MIN`] (0) to [`POSITION_Z_MAX`] (`u16::MAX`).
    pub z_raw: u16,
}

impl WorldPosition {
    /// A position from metres; the height is clamped to the encoded range and rounded.
    pub fn from_metres(x: f32, y: f32, z: f32) -> Self {
        let share = ((z - POSITION_Z_MIN) / (POSITION_Z_MAX - POSITION_Z_MIN)).clamp(0.0, 1.0);
        Self {
            x,
            y,
            z_raw: (share * f32::from(u16::MAX)).round() as u16,
        }
    }

    /// The height in metres: `raw / 65535 * (max - min) + min`.
    pub fn z(&self) -> f32 {
        f32::from(self.z_raw) / f32::from(u16::MAX) * (POSITION_Z_MAX - POSITION_Z_MIN)
            + POSITION_Z_MIN
    }
}

/// One rotation component on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationComponent {
    /// The component is 0; no bytes.
    Zero,
    /// The component is 1; no bytes.
    One,
    /// A big-endian `u16` mapped to `[-1, 1]`.
    Value(u16),
}

/// The `rotation` child: x, y and z of a unit quaternion; `w` is not on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rotation {
    /// x, y and z.
    pub components: [RotationComponent; 3],
    /// Whether `w` is negative.
    pub w_negative: bool,
}

/// The `scale` child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// Tag 0: scale 1, nothing read.
    One,
    /// Tag 1: one binary16 for every axis.
    Uniform(u16),
    /// Any other tag: three binary16 values.
    PerAxis {
        /// The tag byte.
        tag: u8,
        /// x, y and z as binary16.
        values: [u16; 3],
    },
}

/// The replicated position, rotation and scale of one entity.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PositionInTheWorldReplicatedState {
    /// The position, when it changed.
    pub position: Option<WorldPosition>,
    /// The rotation, when it changed.
    pub rotation: Option<Rotation>,
    /// The scale, when it changed.
    pub scale: Option<Scale>,
}

impl PositionInTheWorldReplicatedState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or a bit that the note does not describe
    /// is set.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slot = reader.u8()?;
        if slot & !SLOT_0 != 0 {
            return Err(MessageError::UnknownBits {
                field: "position slot",
                bits: slot & !SLOT_0,
            });
        }
        if slot & SLOT_0 == 0 {
            return Ok(Self::default());
        }
        let children = reader.u8()?;
        if children & !KNOWN_CHILDREN != 0 {
            return Err(MessageError::UnknownBits {
                field: "position children",
                bits: children & !KNOWN_CHILDREN,
            });
        }
        let position = if children & CHILD_POSITION != 0 {
            Some(WorldPosition {
                x: reader.f32()?,
                y: reader.f32()?,
                z_raw: reader.u16()?,
            })
        } else {
            None
        };
        let rotation = if children & CHILD_ROTATION != 0 {
            Some(read_rotation(reader)?)
        } else {
            None
        };
        let scale = if children & CHILD_SCALE != 0 {
            Some(match reader.u8()? {
                SCALE_ONE => Scale::One,
                SCALE_UNIFORM => Scale::Uniform(reader.u16()?),
                tag => Scale::PerAxis {
                    tag,
                    values: [reader.u16()?, reader.u16()?, reader.u16()?],
                },
            })
        } else {
            None
        };
        Ok(Self {
            position,
            rotation,
            scale,
        })
    }

    /// Writes the state for the bundle path. A state with no child writes only the slot byte 0.
    pub fn write(&self, writer: &mut Writer) {
        let children = [
            (self.position.is_some(), CHILD_POSITION),
            (self.rotation.is_some(), CHILD_ROTATION),
            (self.scale.is_some(), CHILD_SCALE),
        ]
        .iter()
        .filter(|(present, _)| *present)
        .fold(0, |bits, (_, bit)| bits | bit);
        if children == 0 {
            writer.u8(0);
            return;
        }
        writer.u8(SLOT_0).u8(children);
        if let Some(position) = self.position {
            writer.f32(position.x).f32(position.y).u16(position.z_raw);
        }
        if let Some(rotation) = self.rotation {
            write_rotation(&rotation, writer);
        }
        match self.scale {
            None => {}
            Some(Scale::One) => {
                writer.u8(SCALE_ONE);
            }
            Some(Scale::Uniform(value)) => {
                writer.u8(SCALE_UNIFORM).u16(value);
            }
            Some(Scale::PerAxis { tag, values }) => {
                writer.u8(tag);
                for value in values {
                    writer.u16(value);
                }
            }
        }
    }
}

fn read_rotation(reader: &mut Reader<'_>) -> Result<Rotation, MessageError> {
    let mask = reader.u8()?;
    if mask & ROTATION_UNKNOWN != 0 {
        return Err(MessageError::UnknownBits {
            field: "rotation mask",
            bits: ROTATION_UNKNOWN,
        });
    }
    let mut components = [RotationComponent::Zero; 3];
    for (index, component) in (0..ROTATION_COMPONENTS).zip(components.iter_mut()) {
        let zero = mask & (1 << index) != 0;
        let one = mask & (1 << (index + ROTATION_ONE_SHIFT)) != 0;
        if zero && one {
            return Err(MessageError::UnknownBits {
                field: "rotation mask",
                bits: mask,
            });
        }
        *component = if zero {
            RotationComponent::Zero
        } else if one {
            RotationComponent::One
        } else {
            RotationComponent::Value(reader.u16()?)
        };
    }
    Ok(Rotation {
        components,
        w_negative: mask & ROTATION_W_NEGATIVE != 0,
    })
}

fn write_rotation(rotation: &Rotation, writer: &mut Writer) {
    let mut mask = if rotation.w_negative {
        ROTATION_W_NEGATIVE
    } else {
        0
    };
    for (index, component) in (0..ROTATION_COMPONENTS).zip(rotation.components) {
        match component {
            RotationComponent::Zero => mask |= 1 << index,
            RotationComponent::One => mask |= 1 << (index + ROTATION_ONE_SHIFT),
            RotationComponent::Value(_) => {}
        }
    }
    writer.u8(mask);
    for component in rotation.components {
        if let RotationComponent::Value(value) = component {
            writer.u16(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: f32 = 8000.5;
    const Y: f32 = 3000.25;
    const HEIGHT: f32 = 120.0;
    const TOLERANCE_M: f32 = 0.05;

    fn full() -> PositionInTheWorldReplicatedState {
        PositionInTheWorldReplicatedState {
            position: Some(WorldPosition::from_metres(X, Y, HEIGHT)),
            rotation: Some(Rotation {
                components: [
                    RotationComponent::Zero,
                    RotationComponent::One,
                    RotationComponent::Value(0x1234),
                ],
                w_negative: true,
            }),
            scale: Some(Scale::PerAxis {
                tag: 2,
                values: [1, 2, 3],
            }),
        }
    }

    fn encode(state: &PositionInTheWorldReplicatedState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer);
        writer.finish()
    }

    fn decode(bytes: &[u8]) -> Result<PositionInTheWorldReplicatedState, MessageError> {
        let mut reader = Reader::new(bytes);
        let state = PositionInTheWorldReplicatedState::read(&mut reader)?;
        reader.finish()?;
        Ok(state)
    }

    #[test]
    fn every_child_round_trips() {
        let bytes = encode(&full());
        assert_eq!(&bytes[..2], &[SLOT_0, KNOWN_CHILDREN]);
        assert_eq!(decode(&bytes), Ok(full()));
    }

    #[test]
    fn a_position_alone_is_twelve_bytes() {
        let state = PositionInTheWorldReplicatedState {
            position: Some(WorldPosition::from_metres(X, Y, HEIGHT)),
            ..PositionInTheWorldReplicatedState::default()
        };
        let bytes = encode(&state);
        assert_eq!(bytes.len(), 12);
        assert_eq!(decode(&bytes), Ok(state));
        let empty = encode(&PositionInTheWorldReplicatedState::default());
        assert_eq!(empty, [0]);
    }

    #[test]
    fn the_height_maps_from_minus_100_to_2000() {
        assert_eq!(
            WorldPosition::from_metres(0.0, 0.0, POSITION_Z_MIN).z_raw,
            0
        );
        assert_eq!(
            WorldPosition::from_metres(0.0, 0.0, POSITION_Z_MAX).z_raw,
            u16::MAX
        );
        assert_eq!(WorldPosition::from_metres(0.0, 0.0, 9999.0).z_raw, u16::MAX);
        let back = WorldPosition::from_metres(X, Y, HEIGHT).z();
        assert!((back - HEIGHT).abs() < TOLERANCE_M);
    }

    #[test]
    fn unknown_bits_are_errors() {
        assert!(matches!(
            decode(&[SLOT_0 | 0x02]),
            Err(MessageError::UnknownBits { .. })
        ));
        assert!(matches!(
            decode(&[SLOT_0, 0x80]),
            Err(MessageError::UnknownBits { .. })
        ));
        assert!(matches!(
            decode(&[SLOT_0, CHILD_ROTATION, ROTATION_UNKNOWN]),
            Err(MessageError::UnknownBits { .. })
        ));
        assert!(decode(&[SLOT_0, CHILD_POSITION, 0]).is_err());
    }
}
