// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::SpellComponentReplicatedState` (class 2912): the state of a spell entity, the slice a
//! cast spawns (Sacred Ground's circle, an impact, a detection volume). Layout from the client's
//! member table; the children Observed in the retail spawn groups (175,954 of 198,914 spell
//! states hold exactly the id, the start place and rotation, the attachment, the caster and
//! the radius scale).

use crate::position::{Rotation, WorldPosition, read_rotation, write_rotation};
use crate::slots::{children_mask, read_slot_0, write_known};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `MB::SpellComponentReplicatedState`.
pub const SPELL_TYPE_INDEX: u64 = 2912;
/// The bytes of an attachment or of the caster's id: a raw id, then an entity id.
pub const SPELL_REF_LEN: usize = 24;
/// The attachment of a spell that hangs on nothing (retail: 20 zero bytes, then `ff ff ff ff`).
pub const NO_ATTACHMENT: [u8; SPELL_REF_LEN] = {
    let mut bytes = [0; SPELL_REF_LEN];
    let mut index = SPELL_REF_LEN - 4;
    while index < SPELL_REF_LEN {
        bytes[index] = 0xff;
        index += 1;
    }
    bytes
};
const FIELD: &str = "spell";
const CHILDREN: u8 = 11;

/// The state of a spell entity. A `None` child is not sent.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpellState {
    /// Child 0, `spllDtId`: CRC-32 of the lowercase `SpellData` id.
    pub spell: Option<u32>,
    /// Child 1, `strtpos`: where it starts.
    pub start: Option<WorldPosition>,
    /// Child 2, `strtrot`.
    pub start_rotation: Option<Rotation>,
    /// Child 3, `chldpos`.
    pub child: Option<WorldPosition>,
    /// Child 4, `chldrot`.
    pub child_rotation: Option<Rotation>,
    /// Child 5, `prevAttch`.
    pub previous_attachment: Option<[u8; SPELL_REF_LEN]>,
    /// Child 6, `attch`: what it hangs on ([`NO_ATTACHMENT`] for nothing).
    pub attachment: Option<[u8; SPELL_REF_LEN]>,
    /// Child 7, `casterId`: the caster's raw id, then its entity id big-endian (Observed: the
    /// last 8 bytes are the first 8 reversed).
    pub caster: Option<[u8; SPELL_REF_LEN]>,
    /// Child 8, `spawnTime`.
    pub spawn_time: Option<u64>,
    /// Child 9, `numSpwn`.
    pub spawns: Option<u32>,
    /// Child 10, `m_aoeRadiusScaling`.
    pub radius_scale: Option<f32>,
}

fn read_place(reader: &mut Reader<'_>) -> Result<WorldPosition, MessageError> {
    Ok(WorldPosition {
        x: reader.f32()?,
        y: reader.f32()?,
        z_raw: reader.u16()?,
    })
}

fn write_place(place: WorldPosition, writer: &mut Writer) {
    writer.f32(place.x).f32(place.y).u16(place.z_raw);
}

fn read_ref(reader: &mut Reader<'_>) -> Result<[u8; SPELL_REF_LEN], MessageError> {
    let mut bytes = [0; SPELL_REF_LEN];
    bytes.copy_from_slice(reader.bytes(SPELL_REF_LEN)?);
    Ok(bytes)
}

impl SpellState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sets an unsized child.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CHILDREN, |reader, child| {
            match child {
                0 => state.spell = Some(reader.u32()?),
                1 => state.start = Some(read_place(reader)?),
                2 => state.start_rotation = Some(read_rotation(reader)?),
                3 => state.child = Some(read_place(reader)?),
                4 => state.child_rotation = Some(read_rotation(reader)?),
                5 => state.previous_attachment = Some(read_ref(reader)?),
                6 => state.attachment = Some(read_ref(reader)?),
                7 => state.caster = Some(read_ref(reader)?),
                8 => state.spawn_time = Some(reader.u64()?),
                9 => state.spawns = Some(reader.u32()?),
                _ => state.radius_scale = Some(reader.f32()?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let mask = children_mask(&[
            self.spell.is_some(),
            self.start.is_some(),
            self.start_rotation.is_some(),
            self.child.is_some(),
            self.child_rotation.is_some(),
            self.previous_attachment.is_some(),
            self.attachment.is_some(),
            self.caster.is_some(),
            self.spawn_time.is_some(),
            self.spawns.is_some(),
            self.radius_scale.is_some(),
        ]);
        write_known(writer, mask, CHILDREN, |writer, child| match child {
            0 => drop(self.spell.map(|spell| writer.u32(spell))),
            1 => drop(self.start.map(|place| write_place(place, writer))),
            2 => drop(
                self.start_rotation
                    .map(|turn| write_rotation(&turn, writer)),
            ),
            3 => drop(self.child.map(|place| write_place(place, writer))),
            4 => drop(
                self.child_rotation
                    .map(|turn| write_rotation(&turn, writer)),
            ),
            5 => drop(self.previous_attachment.map(|bytes| writer.bytes(&bytes))),
            6 => drop(self.attachment.map(|bytes| writer.bytes(&bytes))),
            7 => drop(self.caster.map(|bytes| writer.bytes(&bytes))),
            8 => drop(self.spawn_time.map(|time| writer.u64(time))),
            9 => drop(self.spawns.map(|spawns| writer.u32(spawns))),
            _ => drop(self.radius_scale.map(|scale| writer.f32(scale))),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Sacred Ground of retail `general-other-01` at 79.810 s (the body after the
    /// component and class).
    const SACRED_GROUND: &str = "01c74b414d7946325cd445a497eb111a03fa86\
        0000000000000000000000000000000000000000ffffffff\
        09c86f8015f81f3a191aa8805f5dc5f279193a1ff815806fc83f800000";

    #[test]
    fn the_retail_sacred_ground_reads_and_writes_back_the_same() {
        let body = hex::decode(SACRED_GROUND).expect("hex");
        let mut reader = Reader::new(&body);
        let state = SpellState::read(&mut reader).expect("state");
        reader.finish().expect("the whole body");
        assert_eq!(state.spell, Some(0x4b41_4d79));
        let start = state.start.expect("start");
        assert!((start.x - 11415.207).abs() < 0.01 && (start.y - 5266.99).abs() < 0.01);
        assert_eq!(state.attachment, Some(NO_ATTACHMENT));
        let caster = state.caster.expect("caster");
        let mut entity: [u8; 8] = caster[..8].try_into().expect("8 bytes");
        entity.reverse();
        assert_eq!(
            caster[16..],
            entity,
            "the entity id is the raw id's first half, reversed"
        );
        assert_eq!(state.radius_scale, Some(1.0));
        assert_eq!(
            (
                state.child,
                state.previous_attachment,
                state.spawn_time,
                state.spawns
            ),
            (None, None, None, None)
        );
        let mut writer = Writer::new();
        state.write(&mut writer);
        assert_eq!(writer.finish(), body);
    }
}
