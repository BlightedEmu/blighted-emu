// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::ALCReplicatedState` (type 11) on the bundle path (movement message note, "Type 11 pass";
//! instance virtual `+0x90`, `FUN_142a436b0`).
//!
//! The body starts with one byte whose bit 0 and bit 1 select two slots; the client tests no
//! other bit. A selected slot has a wide prefix integer whose low bits are child presence bits,
//! one per child in constructor order; the client tests only the low 54 bits, and only bits of
//! children that exist matter. A set bit reads that child; a clear bit reads nothing.
//!
//! Only `pos10` (slot 0, child 27) is kept as a value. The rest is skipped, and the body keeps
//! its exact bytes, so a read and write gives the same bytes.

use crate::client_input::InputPosition;
use crate::wire::{MessageError, Reader, Writer};

/// Type index of the class.
pub const ALC_TYPE_INDEX: u64 = 11;
/// Slot 0 child that holds the position (`pos10`), counted from 0.
pub const POS10_CHILD: usize = 27;
/// A `blob33` count is below this.
const BLOB33_LIMIT: u8 = 33;
/// A `blob177` count is below this.
const BLOB177_LIMIT: u8 = 177;
/// Bytes of a `frag` child.
const FRAG_LEN: usize = 12;
/// Bytes after a set `bounds` flag, before its `u8` and the next flag.
const BOUNDS_LEN: usize = 16;
/// Bytes of a `pos3` child.
const POS3_LEN: usize = 3;
/// The child presence bits the client tests.
const PRESENCE_BITS: u64 = (1 << 54) - 1;
/// Slot bits of the first byte.
const SLOT_BITS: [u8; 2] = [0b01, 0b10];
/// `quat` tag bits that shorten the child (movement message note, `FUN_14087a8d0`).
const QUAT_BIT_1: u8 = 0b10;
const QUAT_BIT_2: u8 = 0b100;
const QUAT_BIT_3: u8 = 0b1000;
const QUAT_BIT_4: u8 = 0b1_0000;
/// Extra `quat` bytes when no tag bit shortens it.
const QUAT_MAX_EXTRA: usize = 3;

/// The kind of one child, by its size on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlcChild {
    /// 1 byte.
    U8,
    /// 2 bytes.
    U16,
    /// 4 bytes.
    F32,
    /// 8 bytes.
    U64,
    /// 3 bytes.
    Pos3,
    /// `f32`, `f32`, `u16`, big-endian.
    Pos10,
    /// 2 bytes.
    Half,
    /// A short prefix integer.
    Prefix,
    /// A short prefix count below 33, then that many bytes.
    Blob33,
    /// A short prefix count below 177, then that many bytes.
    Blob177,
    /// 12 bytes.
    Frag,
    /// A flag byte; 1 adds 16 bytes, a `u8` and the next flag byte.
    Bounds,
    /// A tag byte, then 0 to 3 bytes.
    Quat,
}

use AlcChild::{Blob33, Blob177, Bounds, F32, Frag, Half, Pos3, Pos10, Prefix, Quat, U8, U16, U64};

/// The 54 children of slot 0, in constructor order.
pub const SLOT0_CHILDREN: [AlcChild; 54] = [
    U8, U8, U8, Half, U8, Half, U8, Half, U8, Half, Pos3, Quat, Quat, Prefix, U16, Prefix, Prefix,
    U16, Prefix, Prefix, U16, Prefix, Prefix, U16, Prefix, U8, U8, Pos10, U8, Blob33, U8, U8,
    Prefix, Blob177, Frag, F32, Half, Half, U8, U8, U64, U8, U8, U8, U8, U8, Prefix, U8, Prefix,
    U8, U8, U8, U8, F32,
];

/// The 9 children of slot 1, in constructor order.
pub const SLOT1_CHILDREN: [AlcChild; 9] = [U8, U8, Blob177, Blob33, U8, U8, U8, Bounds, U8];

/// The extra bytes after a `quat` tag.
const fn quat_extra(tag: u8) -> usize {
    let mut count = if tag & QUAT_BIT_1 == 0 {
        QUAT_MAX_EXTRA
    } else {
        QUAT_MAX_EXTRA - 1
    };
    if tag & QUAT_BIT_2 != 0 {
        count -= 1;
    }
    if tag & QUAT_BIT_3 != 0 {
        count -= 1;
    }
    if tag & QUAT_BIT_4 != 0 && count != 0 {
        count -= 1;
    }
    count
}

/// One type 11 member body: its bytes and its position.
#[derive(Debug, Clone, PartialEq)]
pub struct AlcState {
    /// The body as read.
    pub bytes: Vec<u8>,
    /// `pos10` when slot 0 sends it.
    pub pos10: Option<InputPosition>,
}

impl AlcState {
    /// Reads one body from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a blob count is over its limit, or a
    /// `bounds` flag is not 0 or 1.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let start = reader.rest();
        let mask = reader.u8()?;
        let mut pos10 = None;
        for (slot_bit, children) in SLOT_BITS.iter().zip([&SLOT0_CHILDREN[..], &SLOT1_CHILDREN]) {
            if mask & slot_bit == 0 {
                continue;
            }
            let present = reader.wide_prefixed()? & PRESENCE_BITS;
            for (index, kind) in children.iter().enumerate() {
                if present >> index & 1 == 0 {
                    continue;
                }
                if *kind == Pos10 {
                    pos10 = Some(InputPosition::read(reader)?);
                } else {
                    skip_child(*kind, reader)?;
                }
            }
        }
        let used = start.len() - reader.remaining();
        Ok(Self {
            bytes: start[..used].to_vec(),
            pos10,
        })
    }

    /// Writes the body as read.
    pub fn write(&self, writer: &mut Writer) {
        writer.bytes(&self.bytes);
    }

    /// A minimal body that replicates only `pos` through the slot-0 `pos10` child (child 27):
    /// the slot-0 mask bit, a presence integer with only child 27 set, then the 10-byte position.
    /// This is how entities carry their position in the retail bundles (the dominant member is
    /// type 11); [`read`](Self::read) of the result returns the same bytes and `pos10`.
    pub fn at_position(pos: InputPosition) -> Self {
        let mut writer = Writer::new();
        writer.u8(SLOT_BITS[0]);
        writer.wide_prefixed(1 << POS10_CHILD);
        writer.f32(pos.x).f32(pos.y).u16(pos.z_raw);
        Self {
            bytes: writer.finish(),
            pos10: Some(pos),
        }
    }
}

fn skip_child(kind: AlcChild, reader: &mut Reader<'_>) -> Result<(), MessageError> {
    match kind {
        U8 => {
            reader.u8()?;
        }
        U16 | Half => {
            reader.u16()?;
        }
        F32 => {
            reader.f32()?;
        }
        U64 => {
            reader.u64()?;
        }
        Pos3 => {
            reader.bytes(POS3_LEN)?;
        }
        Pos10 => {
            InputPosition::read(reader)?;
        }
        Prefix => {
            reader.prefixed()?;
        }
        Blob33 => skip_blob(reader, BLOB33_LIMIT)?,
        Blob177 => skip_blob(reader, BLOB177_LIMIT)?,
        Frag => {
            reader.bytes(FRAG_LEN)?;
        }
        Bounds => {
            if reader.boolean()? {
                reader.bytes(BOUNDS_LEN)?;
                reader.u8()?;
                reader.boolean()?;
            }
        }
        Quat => {
            let tag = reader.u8()?;
            reader.bytes(quat_extra(tag))?;
        }
    }
    Ok(())
}

fn skip_blob(reader: &mut Reader<'_>, limit: u8) -> Result<(), MessageError> {
    let count = reader.prefixed()?;
    if count >= u64::from(limit) {
        return Err(MessageError::CountTooLarge {
            count,
            limit: u64::from(limit - 1),
        });
    }
    // The count is below 177.
    reader.bytes(count as usize)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::payload::{BundlePayload, MemberState};
    use crate::position::{
        POSITION_IN_THE_WORLD_TYPE_INDEX, PositionInTheWorldReplicatedState, WorldPosition,
    };
    use crate::wire::Writer;

    const POS10_X: f32 = 100.5;
    const POS10_Y: f32 = 200.25;
    const POS10_Z_RAW: u16 = 1000;
    const SLOT_0: u8 = 0b01;
    const SLOT_1: u8 = 0b10;
    const ENTITY: u64 = 9;
    const GROUP: u64 = 1;
    /// A presence bit above the 54 children of slot 0.
    const STRAY_BIT: u64 = 1 << 60;

    /// Valid bytes of one child of `kind`, from `seed`.
    fn child_bytes(kind: AlcChild, seed: u8) -> Vec<u8> {
        let mut writer = Writer::new();
        match kind {
            AlcChild::U8 => {
                writer.u8(seed);
            }
            AlcChild::U16 | AlcChild::Half => {
                writer.u16(u16::from(seed));
            }
            AlcChild::F32 => {
                writer.f32(f32::from(seed));
            }
            AlcChild::U64 => {
                writer.u64(u64::from(seed));
            }
            AlcChild::Pos3 => {
                writer.bytes(&[seed; 3]);
            }
            AlcChild::Pos10 => {
                writer.f32(POS10_X).f32(POS10_Y).u16(POS10_Z_RAW);
            }
            AlcChild::Prefix => {
                writer.prefixed(u64::from(seed)).expect("prefix");
            }
            AlcChild::Blob33 => {
                let count = seed % BLOB33_LIMIT;
                writer
                    .prefixed(u64::from(count))
                    .expect("count")
                    .bytes(&vec![seed; usize::from(count)]);
            }
            AlcChild::Blob177 => {
                let count = seed % BLOB177_LIMIT;
                writer
                    .prefixed(u64::from(count))
                    .expect("count")
                    .bytes(&vec![seed; usize::from(count)]);
            }
            AlcChild::Frag => {
                writer.bytes(&[seed; FRAG_LEN]);
            }
            AlcChild::Bounds => {
                if seed.is_multiple_of(2) {
                    writer.u8(0);
                } else {
                    writer.u8(1).bytes(&[seed; BOUNDS_LEN]).u8(seed).u8(0);
                }
            }
            AlcChild::Quat => {
                writer.u8(seed).bytes(&vec![seed; quat_extra(seed)]);
            }
        }
        writer.finish()
    }

    /// A body with the given children of each slot set, from `seed`.
    fn body(slot0: &[usize], slot1: &[usize], seed: u8) -> Vec<u8> {
        let mut writer = Writer::new();
        let mut mask = 0;
        if !slot0.is_empty() {
            mask |= SLOT_0;
        }
        if !slot1.is_empty() {
            mask |= SLOT_1;
        }
        writer.u8(mask);
        for (children, table) in [(slot0, &SLOT0_CHILDREN[..]), (slot1, &SLOT1_CHILDREN[..])] {
            if children.is_empty() {
                continue;
            }
            let bits = children.iter().fold(0_u64, |bits, child| bits | 1 << child);
            writer.wide_prefixed(bits);
            for (index, kind) in table.iter().enumerate() {
                if children.contains(&index) {
                    let value = seed.wrapping_add(u8::try_from(index).expect("small"));
                    writer.bytes(&child_bytes(*kind, value));
                }
            }
        }
        writer.finish()
    }

    fn read(bytes: &[u8]) -> Result<(AlcState, usize), MessageError> {
        let mut reader = Reader::new(bytes);
        let state = AlcState::read(&mut reader)?;
        Ok((state, reader.remaining()))
    }

    #[test]
    fn the_tables_have_the_note_widths() {
        assert_eq!(SLOT0_CHILDREN.len(), 54);
        assert_eq!(SLOT1_CHILDREN.len(), 9);
        assert_eq!(SLOT0_CHILDREN[POS10_CHILD], AlcChild::Pos10);
    }

    #[test]
    fn an_alc_body_with_slot_0_reads_pos10() {
        let bytes = body(&[POS10_CHILD], &[], 0);
        let (state, left) = read(&bytes).expect("read");
        assert_eq!(left, 0);
        let position = state.pos10.expect("pos10");
        assert_eq!(
            (position.x, position.y, position.z_raw),
            (POS10_X, POS10_Y, POS10_Z_RAW)
        );
        assert_eq!(state.bytes, bytes);
    }

    #[test]
    fn an_alc_body_with_both_slots_reads_to_its_end() {
        let mut bytes = body(&[0, 10, 11, POS10_CHILD, 53], &[2, 3, 7], 3);
        let end = bytes.len();
        bytes.push(0xAB);
        let (state, left) = read(&bytes).expect("read");
        assert_eq!(left, 1, "the next byte is not read");
        assert_eq!(state.bytes, bytes[..end]);
    }

    #[test]
    fn a_body_without_slots_is_one_byte() {
        let (state, left) = read(&[0, 0xAB]).expect("read");
        assert_eq!((state.bytes, state.pos10, left), (vec![0], None, 1));
    }

    #[test]
    fn alc_presence_bits_above_the_children_are_ignored() {
        let mut writer = Writer::new();
        writer.u8(SLOT_0).wide_prefixed(STRAY_BIT | 1).u8(7);
        let bytes = writer.finish();
        let (state, left) = read(&bytes).expect("read");
        assert_eq!((state.bytes.len(), left), (bytes.len(), 0));
    }

    #[test]
    fn an_alc_blob_over_its_limit_is_an_error() {
        const BLOB33_CHILD: usize = 29;
        assert_eq!(SLOT0_CHILDREN[BLOB33_CHILD], AlcChild::Blob33);
        let mut writer = Writer::new();
        writer
            .u8(SLOT_0)
            .wide_prefixed(1 << BLOB33_CHILD)
            .prefixed(u64::from(BLOB33_LIMIT))
            .expect("count")
            .bytes(&[0; BLOB33_LIMIT as usize]);
        assert!(matches!(
            read(&writer.finish()),
            Err(MessageError::CountTooLarge { .. })
        ));
    }

    #[test]
    fn a_short_alc_body_is_an_error() {
        let bytes = body(&[POS10_CHILD], &[], 0);
        assert!(matches!(
            read(&bytes[..bytes.len() - 1]),
            Err(MessageError::Truncated { .. })
        ));
    }

    #[test]
    fn each_quat_tag_gives_its_byte_count() {
        for tag in 0..=u8::MAX {
            let mut count: usize = if tag & 0b10 == 0 { 3 } else { 2 };
            if tag & 0b100 != 0 {
                count -= 1;
            }
            if tag & 0b1000 != 0 {
                count -= 1;
            }
            if tag & 0b1_0000 != 0 && count != 0 {
                count -= 1;
            }
            assert_eq!(quat_extra(tag), count, "tag {tag:#04x}");
        }
    }

    #[test]
    fn a_bounds_child_reads_its_flag_chain() {
        const BOUNDS_CHILD: usize = 7;
        assert_eq!(SLOT1_CHILDREN[BOUNDS_CHILD], AlcChild::Bounds);
        for seed in [0, 1] {
            let bytes = body(&[], &[BOUNDS_CHILD], seed);
            let (state, left) = read(&bytes).expect("read");
            assert_eq!((state.bytes, left), (bytes, 0));
        }
        let mut writer = Writer::new();
        writer.u8(SLOT_1).wide_prefixed(1 << BOUNDS_CHILD).u8(2);
        assert!(matches!(
            read(&writer.finish()),
            Err(MessageError::NotBoolean { value: 2 })
        ));
    }

    proptest! {
        #[test]
        fn an_alc_body_reads_and_writes_the_same_bytes(
            slot0 in any::<u64>(),
            slot1 in any::<u16>(),
            seed in any::<u8>(),
        ) {
            let slot0: Vec<usize> = (0..SLOT0_CHILDREN.len()).filter(|bit| slot0 >> bit & 1 == 1).collect();
            let slot1: Vec<usize> = (0..SLOT1_CHILDREN.len()).filter(|bit| slot1 >> bit & 1 == 1).collect();
            let bytes = body(&slot0, &slot1, seed);
            let (state, left) = read(&bytes).expect("read");
            prop_assert_eq!(left, 0);
            let mut writer = Writer::new();
            state.write(&mut writer);
            prop_assert_eq!(writer.finish(), bytes);
        }
    }

    #[test]
    fn decode_known_goes_past_alc() {
        let mut writer = Writer::new();
        writer.prefixed(GROUP).expect("group").u8(2);
        writer
            .prefixed(ENTITY)
            .expect("entity")
            .bytes(
                &nw_reflect::encode_type_id(&nw_reflect::TypeId::FileSlot(ALC_TYPE_INDEX))
                    .expect("class"),
            )
            .bytes(&body(&[POS10_CHILD], &[], 0));
        let position = crate::payload::BundleMember {
            u64_at_0: ENTITY,
            state: MemberState::Position(PositionInTheWorldReplicatedState {
                position: WorldPosition::from_metres(1.0, 2.0, 3.0),
                ..PositionInTheWorldReplicatedState::default()
            }),
        };
        writer.bytes(&position.encode().expect("member"));
        let payload = writer.finish();
        let known = BundlePayload::decode_known(&payload).expect("decode");
        assert_eq!(known.stopped_at, None);
        let classes: Vec<u64> = known
            .members
            .iter()
            .map(|(member, _)| member.state.type_index())
            .collect();
        assert_eq!(
            classes,
            vec![ALC_TYPE_INDEX, POSITION_IN_THE_WORLD_TYPE_INDEX]
        );
        let whole = BundlePayload::decode(&payload).expect("whole");
        assert_eq!(whole.encode().expect("encode"), payload);
    }
}
