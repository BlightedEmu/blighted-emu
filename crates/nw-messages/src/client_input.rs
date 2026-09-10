// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ActionListComponentServerFacet_ReceiveInputFromClient` (type 1589): the client's input
//! (replication note section 3, movement message note sections 1 to 4, both checked on every
//! captured body). Unmarshal `FUN_144867c70`, head `FUN_142a2e6b0`, record `FUN_142a25d90`.
//!
//! A record starts with a 24-bit mask. Mask bits 0 to 8 gate fixed fields. The later rows depend
//! on the record's prefix integer (mask bit 0) and on a gate byte (row 4); a record that does not
//! set them keeps the previous record's values. Most rows are gated by a mask bit at a moving
//! cursor. Parts whose meaning is Unknown are kept as raw bytes, so a body encodes back to the
//! same bytes; `encode` refuses a record whose fixed fields do not match its mask.

use crate::position::{height_from_raw, height_to_raw};
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// File slot of `ReceiveInputFromClient`.
pub const RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX: u64 = 1589;
/// Height of a raw `z` of 0 in an input position, in metres.
pub const INPUT_Z_MIN: f32 = -100.0;
/// Height of a raw `z` of `u16::MAX` in an input position, in metres.
pub const INPUT_Z_MAX: f32 = 1000.0;
/// Bytes of the record mask.
const MASK_LEN: usize = 3;
/// The mask bits with a fixed field (0 to 8).
const FIXED_BITS: u32 = 9;
/// Head step 15: most pairs.
const MAX_HEAD_PAIRS: u64 = 16;
/// Most elements in one element list (`0x2000000`).
const MAX_ELEMENTS: u64 = 0x200_0000;
/// Longest string in the bit 8 body (`FUN_14087a3d0`).
const MAX_STRING: u64 = 0x2_FFFF;
/// The tag of the group after the position that ends the group at once.
pub const AFTER_POSITION_END: u8 = 3;
/// Bytes of the group after the position, after its tag: a `u8`, two `f32`, four `u16`.
pub const AFTER_POSITION_LEN: usize = 17;
/// Raw bytes before the `u64` of the row 3 id.
const ROW_3_RAW_LEN: usize = 16;
/// Bytes of the row 5 block (`FUN_1415acf00`).
const ROW_5_BLOCK_LEN: usize = 32;
/// Bytes of row 6.
const ROW_6_LEN: usize = 8;
/// Scaled bytes after the slots (`FUN_142a15ae0`, four calls).
const SLOT_BYTES: usize = 4;
/// Bytes of one element of the three element lists (`FUN_142a1a310`, `FUN_142a1a0d0`,
/// `FUN_142a19e60`).
pub const ELEMENT_LENS: [usize; 3] = [104, 192, 96];
/// Binary16 values of row 11.
const ROW_11_VALUES: usize = 3;
/// The cursor of the rows after bit 8.
const FIRST_CURSOR: u32 = 9;
/// The cursor after row 2.
const ROW_2_CURSOR: u32 = 10;
/// Prefix bits that turn rows on.
const PREFIX_ROW_1: u32 = 0;
const PREFIX_ROW_2: u32 = 11;
const PREFIX_ROW_3: u32 = 8;
const PREFIX_ROW_6: u32 = 14;
const PREFIX_ROW_10: u32 = 15;
const PREFIX_ROW_12: u32 = 16;
const PREFIX_ROW_13: u32 = 18;
const PREFIX_ROW_16: u32 = 0;
/// Mask bits of the fixed fields.
const BIT_0: u32 = 0;
const BIT_1: u32 = 1;
const BIT_2: u32 = 2;
const BIT_3: u32 = 3;
const BIT_4: u32 = 4;
const BIT_5: u32 = 5;
const BIT_6: u32 = 6;
const BIT_7: u32 = 7;
const BIT_8: u32 = 8;

/// A position in an input record: big-endian `f32` world x and y, and a `u16` height from
/// [`INPUT_Z_MIN`] to [`INPUT_Z_MAX`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputPosition {
    /// East, in metres.
    pub x: f32,
    /// North, in metres.
    pub y: f32,
    /// Height, from [`INPUT_Z_MIN`] (0) to [`INPUT_Z_MAX`] (`u16::MAX`).
    pub z_raw: u16,
}

impl InputPosition {
    /// A position from metres; the height is clamped to the encoded range. `None` when a value
    /// is not finite.
    pub fn from_metres(x: f32, y: f32, z: f32) -> Option<Self> {
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return None;
        }
        Some(Self {
            x,
            y,
            z_raw: height_to_raw(z, INPUT_Z_MIN, INPUT_Z_MAX),
        })
    }

    /// The height in metres.
    pub fn z(&self) -> f32 {
        height_from_raw(self.z_raw, INPUT_Z_MIN, INPUT_Z_MAX)
    }

    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            x: reader.f32()?,
            y: reader.f32()?,
            z_raw: reader.u16()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.f32(self.x).f32(self.y).u16(self.z_raw);
    }
}

/// Head step 9 (`FUN_1461ac530`): a flag, a prefix-coded integer when the flag is 1, a `u64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeadStep9 {
    /// The integer after a flag of 1.
    pub prefixed: Option<u64>,
    /// The big-endian `u64` at the end.
    pub u64_at_end: u64,
}

/// The fixed head (`FUN_142a2e6b0`), steps numbered as in the replication note.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputHead {
    /// Steps 1 to 4.
    pub bytes_1_to_4: [u8; 4],
    /// Step 5, a wide prefix-coded integer.
    pub step_5: u64,
    /// Steps 6 and 7.
    pub steps_6_7: [u64; 2],
    /// Step 8.
    pub step_8: u8,
    /// Step 9.
    pub step_9: HeadStep9,
    /// Step 10.
    pub step_10: u8,
    /// Steps 11 to 13.
    pub steps_11_to_13: [u64; 3],
    /// Step 14.
    pub step_14: u8,
    /// Step 15: at most 16 pairs.
    pub step_15_pairs: Vec<[u64; 2]>,
    /// Step 15: the two integers after the pairs.
    pub step_15_tail: [u64; 2],
    /// Steps 16 and 17.
    pub steps_16_17: [u64; 2],
}

/// The slots after mask bit 4 (`FUN_142a2ea20`): a count, the packed values, four scaled bytes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputSlots {
    /// Slots.
    pub count: u8,
    /// One wide integer per two slots, then one narrow integer for an odd last slot.
    pub values: Vec<u64>,
    /// The four scaled bytes.
    pub bytes: [u8; SLOT_BYTES],
}

/// Mask bit 8 (`FUN_1435ebc30`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bit8 {
    /// Flag 0.
    Empty,
    /// Flag 1: the long body, as raw bytes.
    Body(Vec<u8>),
}

/// The mask bits that are 0 in every movement-only record and set in fights (attack input
/// note); which one is a light attack, a heavy attack or an ability is Unknown.
pub const ATTACK_MASK_BITS: [u32; 4] = [9, 10, 13, 15];

/// One record (`FUN_142a25d90`). `None` means the field is not in the record.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InputRecord {
    /// The 24-bit mask.
    pub mask: u32,
    /// The big-endian `u16` after the mask.
    pub u16_after_mask: u16,
    /// Bit 0: the record's prefix integer (wide form).
    pub bit_0: Option<u64>,
    /// Bit 1.
    pub bit_1: Option<u8>,
    /// Bit 2.
    pub bit_2: Option<bool>,
    /// Bit 3.
    pub bit_3: Option<u8>,
    /// Bit 4.
    pub bit_4: Option<u16>,
    /// The slots, always present.
    pub slots: InputSlots,
    /// Bit 5.
    pub bit_5: Option<f32>,
    /// Bit 6.
    pub bit_6: Option<[f32; 2]>,
    /// Bit 7: the position.
    pub bit_7: Option<InputPosition>,
    /// The group after the position (`FUN_14416eb00`): `None` for the end tag 3, else the tag and
    /// its 17 bytes.
    pub after_position: Option<(u8, [u8; AFTER_POSITION_LEN])>,
    /// Bit 8.
    pub bit_8: Option<Bit8>,
    /// Row 1: three integers (prefix bit 0).
    pub row_1: Option<[u64; 3]>,
    /// Row 2: a position (prefix bit 11, mask bit 9).
    pub row_2: Option<InputPosition>,
    /// Row 3: a position (prefix bit 8, cursor bit).
    pub row_3_position: Option<InputPosition>,
    /// Row 3: 16 raw bytes and a `u64` (prefix bit 8, next cursor bit).
    pub row_3_id: Option<([u8; ROW_3_RAW_LEN], u64)>,
    /// Row 4: the gate byte (cursor bit).
    pub row_4: Option<u8>,
    /// Row 5: a position (gate byte not 0, cursor bit).
    pub row_5_position: Option<InputPosition>,
    /// Row 5: the 32-byte block (gate byte not 0, next cursor bit).
    pub row_5_block: Option<[u8; ROW_5_BLOCK_LEN]>,
    /// Row 6: 8 raw bytes (prefix bit 14, cursor bit).
    pub row_6: Option<[u8; ROW_6_LEN]>,
    /// Row 7: elements of 104 bytes, raw.
    pub elements_104: Vec<u8>,
    /// Row 8: elements of 192 bytes, raw.
    pub elements_192: Vec<u8>,
    /// Row 9: elements of 96 bytes, raw.
    pub elements_96: Vec<u8>,
    /// Row 10: a position (prefix bit 15, cursor bit).
    pub row_10: Option<InputPosition>,
    /// Row 11: three binary16 values (cursor bit).
    pub row_11: Option<[u16; ROW_11_VALUES]>,
    /// Row 12: a `u64` (prefix bit 16).
    pub row_12: Option<u64>,
    /// Row 13: a `u8` (prefix bit 18).
    pub row_13: Option<u8>,
    /// Row 14: a `u8` (cursor bit + 1).
    pub row_14: Option<u8>,
    /// Row 15: a `u32` (cursor bit + 2).
    pub row_15: Option<u32>,
    /// Row 16: a binary16 (prefix bit 0, cursor bit + 3).
    pub row_16: Option<u16>,
}

/// The whole message.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClientInputMsg {
    /// The first big-endian `u64`.
    pub component: u64,
    /// The second big-endian `u64`.
    pub u64_at_8: u64,
    /// The fixed head.
    pub head: InputHead,
    /// The records; at most 255 (a `u8` count).
    pub records: Vec<InputRecord>,
}

/// What a record leaves to the next one.
#[derive(Debug, Clone, Copy, Default)]
struct Carried {
    prefix: u64,
    gate: u8,
}

impl Carried {
    fn prefix_bit(self, bit: u32) -> bool {
        // The reader tests the low 32 bits.
        (self.prefix >> bit) & 1 == 1
    }
}

const fn mask_bit(mask: u32, bit: u32) -> bool {
    (mask >> bit) & 1 == 1
}

/// Which rows after bit 8 a record holds (`FUN_142a25d90`, the bit cursor): from its mask, its
/// prefix integer and the gate byte after row 4. `decode` reads by it and `encode` checks by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct RowLayout {
    row_1: bool,
    row_2: bool,
    row_3_position: bool,
    row_3_id: bool,
    row_4: bool,
    row_5_position: bool,
    row_5_block: bool,
    row_6: bool,
    row_10: bool,
    row_11: bool,
    row_12: bool,
    row_13: bool,
    row_14: bool,
    row_15: bool,
    row_16: bool,
}

impl RowLayout {
    /// The rows of a record with `mask`, the prefix integer `prefix` and the gate byte `gate`
    /// that row 5 sees. Rows 1 to 4 do not depend on `gate`.
    fn new(mask: u32, prefix: u64, gate: u8) -> Self {
        let bit = |n: u32| mask_bit(mask, n);
        let carried = Carried { prefix, gate };
        let mut layout = Self {
            row_1: carried.prefix_bit(PREFIX_ROW_1),
            ..Self::default()
        };
        let mut cursor = FIRST_CURSOR;
        if carried.prefix_bit(PREFIX_ROW_2) {
            cursor = ROW_2_CURSOR;
            layout.row_2 = bit(FIRST_CURSOR);
        }
        if carried.prefix_bit(PREFIX_ROW_3) {
            layout.row_3_position = bit(cursor);
            layout.row_3_id = bit(cursor + 1);
            cursor += 2;
        }
        layout.row_4 = bit(cursor);
        let mut next = cursor + 1;
        if gate != 0 {
            layout.row_5_position = bit(cursor + 1);
            layout.row_5_block = bit(cursor + 2);
            next = cursor + 3;
        }
        let mut cursor = next;
        if carried.prefix_bit(PREFIX_ROW_6) {
            layout.row_6 = bit(cursor);
            cursor += 1;
        }
        if carried.prefix_bit(PREFIX_ROW_10) {
            layout.row_10 = bit(cursor);
            cursor += 1;
        }
        layout.row_11 = bit(cursor);
        layout.row_12 = carried.prefix_bit(PREFIX_ROW_12);
        layout.row_13 = carried.prefix_bit(PREFIX_ROW_13);
        layout.row_14 = bit(cursor + 1);
        layout.row_15 = bit(cursor + 2);
        layout.row_16 = carried.prefix_bit(PREFIX_ROW_16) && bit(cursor + 3);
        layout
    }
}

impl InputRecord {
    /// The attack bits of the mask: bit `n` of the result is [`ATTACK_MASK_BITS`]`[n]`.
    pub fn attack_bits(&self) -> u8 {
        ATTACK_MASK_BITS
            .iter()
            .enumerate()
            .filter(|(_, bit)| mask_bit(self.mask, **bit))
            .fold(0, |bits, (index, _)| bits | 1 << index)
    }
}

impl ClientInputMsg {
    /// An input with an empty head and one record that holds only `position`, as the test client
    /// sends it.
    pub fn position(position: InputPosition) -> Self {
        Self {
            records: vec![InputRecord {
                mask: 1 << BIT_7,
                bit_7: Some(position),
                ..InputRecord::default()
            }],
            ..Self::default()
        }
    }

    /// An input with a record that holds `position` and attack bit 9, then a record with only
    /// `position` that releases the bit, so that the next attack input is a rising edge again
    /// (M6-B spec sections 6.1 and 9.2). With no prefix integer, bit 9 is the cursor bit of row
    /// 4, so the first record carries a gate byte of 0: the light attack.
    pub fn attack(position: InputPosition) -> Self {
        let press = InputRecord {
            mask: 1 << BIT_7 | 1 << ATTACK_MASK_BITS[0],
            bit_7: Some(position),
            row_4: Some(0),
            ..InputRecord::default()
        };
        let release = Self::position(position).records;
        Self {
            records: [vec![press], release].concat(),
            ..Self::default()
        }
    }

    /// The position of the last record that has one.
    pub fn last_position(&self) -> Option<InputPosition> {
        self.records.iter().rev().find_map(|record| record.bit_7)
    }
}

impl BodyCodec for ClientInputMsg {
    const TYPE_INDEX: u64 = RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.u64(self.component).u64(self.u64_at_8);
        self.head.write(&mut writer)?;
        let count = u8::try_from(self.records.len()).map_err(|_| MessageError::CountTooLarge {
            count: u64::try_from(self.records.len()).unwrap_or(u64::MAX),
            limit: u64::from(u8::MAX),
        })?;
        writer.u8(count);
        let mut carried = Carried::default();
        for record in &self.records {
            record.write(&mut writer, &mut carried)?;
        }
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let component = reader.u64()?;
        let u64_at_8 = reader.u64()?;
        let head = InputHead::read(&mut reader)?;
        let count = reader.u8()?;
        let mut carried = Carried::default();
        let records = (0..count)
            .map(|_| InputRecord::read(&mut reader, &mut carried))
            .collect::<Result<Vec<_>, _>>()?;
        reader.finish()?;
        Ok(Self {
            component,
            u64_at_8,
            head,
            records,
        })
    }
}

impl InputHead {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let bytes_1_to_4 = reader.array()?;
        let step_5 = reader.wide_prefixed()?;
        let steps_6_7 = [reader.prefixed()?, reader.prefixed()?];
        let step_8 = reader.u8()?;
        let prefixed = if reader.boolean()? {
            Some(reader.prefixed()?)
        } else {
            None
        };
        let step_9 = HeadStep9 {
            prefixed,
            u64_at_end: reader.u64()?,
        };
        let step_10 = reader.u8()?;
        let steps_11_to_13 = [reader.prefixed()?, reader.prefixed()?, reader.prefixed()?];
        let step_14 = reader.u8()?;
        let pairs = reader.prefixed()?;
        if pairs > MAX_HEAD_PAIRS {
            return Err(MessageError::CountTooLarge {
                count: pairs,
                limit: MAX_HEAD_PAIRS,
            });
        }
        let step_15_pairs = (0..pairs)
            .map(|_| Ok([reader.prefixed()?, reader.prefixed()?]))
            .collect::<Result<Vec<_>, MessageError>>()?;
        let step_15_tail = [reader.prefixed()?, reader.prefixed()?];
        let steps_16_17 = [reader.prefixed()?, reader.prefixed()?];
        Ok(Self {
            bytes_1_to_4,
            step_5,
            steps_6_7,
            step_8,
            step_9,
            step_10,
            steps_11_to_13,
            step_14,
            step_15_pairs,
            step_15_tail,
            steps_16_17,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let pairs = u64::try_from(self.step_15_pairs.len()).unwrap_or(u64::MAX);
        if pairs > MAX_HEAD_PAIRS {
            return Err(MessageError::CountTooLarge {
                count: pairs,
                limit: MAX_HEAD_PAIRS,
            });
        }
        writer.bytes(&self.bytes_1_to_4).wide_prefixed(self.step_5);
        for value in self.steps_6_7 {
            writer.prefixed(value)?;
        }
        writer
            .u8(self.step_8)
            .boolean(self.step_9.prefixed.is_some());
        if let Some(value) = self.step_9.prefixed {
            writer.prefixed(value)?;
        }
        writer.u64(self.step_9.u64_at_end).u8(self.step_10);
        for value in self.steps_11_to_13 {
            writer.prefixed(value)?;
        }
        writer.u8(self.step_14).prefixed(pairs)?;
        for value in self.step_15_pairs.iter().flatten() {
            writer.prefixed(*value)?;
        }
        for value in self.step_15_tail.iter().chain(&self.steps_16_17) {
            writer.prefixed(*value)?;
        }
        Ok(())
    }
}

impl InputSlots {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let count = reader.u8()?;
        let mut values = Vec::new();
        let mut left = count;
        while left > 0 {
            if left == 1 {
                values.push(reader.prefixed()?);
                left = 0;
            } else {
                values.push(reader.wide_prefixed()?);
                left -= 2;
            }
        }
        Ok(Self {
            count,
            values,
            bytes: reader.array()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let expected = usize::from(self.count).div_ceil(2);
        if self.values.len() != expected {
            return Err(MessageError::LengthMismatch { field: "slots" });
        }
        writer.u8(self.count);
        let odd = self.count % 2 == 1;
        for (index, value) in self.values.iter().enumerate() {
            if odd && index + 1 == self.values.len() {
                writer.prefixed(*value)?;
            } else {
                writer.wide_prefixed(*value);
            }
        }
        writer.bytes(&self.bytes);
        Ok(())
    }
}

impl InputRecord {
    fn read(reader: &mut Reader<'_>, carried: &mut Carried) -> Result<Self, MessageError> {
        let mask_bytes: [u8; MASK_LEN] = reader.array()?;
        let mask = u32::from_le_bytes([mask_bytes[0], mask_bytes[1], mask_bytes[2], 0]);
        let bit = |n| mask_bit(mask, n);
        let mut record = Self {
            mask,
            u16_after_mask: reader.u16()?,
            ..Self::default()
        };
        if bit(BIT_0) {
            let prefix = reader.wide_prefixed()?;
            record.bit_0 = Some(prefix);
            carried.prefix = prefix & u64::from(u32::MAX);
        }
        record.bit_1 = bit(BIT_1).then(|| reader.u8()).transpose()?;
        record.bit_2 = bit(BIT_2).then(|| reader.boolean()).transpose()?;
        record.bit_3 = bit(BIT_3).then(|| reader.u8()).transpose()?;
        record.bit_4 = bit(BIT_4).then(|| reader.u16()).transpose()?;
        record.slots = InputSlots::read(reader)?;
        record.bit_5 = bit(BIT_5).then(|| reader.f32()).transpose()?;
        record.bit_6 = bit(BIT_6)
            .then(|| Ok::<_, MessageError>([reader.f32()?, reader.f32()?]))
            .transpose()?;
        record.bit_7 = read_position(reader, bit(BIT_7))?;
        let tag = reader.u8()?;
        if tag != AFTER_POSITION_END {
            record.after_position = Some((tag, reader.array()?));
        }
        if bit(BIT_8) {
            record.bit_8 = Some(if reader.boolean()? {
                Bit8::Body(read_long_body(reader)?.to_vec())
            } else {
                Bit8::Empty
            });
        }
        record.read_rows(reader, carried)?;
        Ok(record)
    }

    /// The rows after bit 8, as [`RowLayout`] gives them.
    fn read_rows(
        &mut self,
        reader: &mut Reader<'_>,
        carried: &mut Carried,
    ) -> Result<(), MessageError> {
        let first = RowLayout::new(self.mask, carried.prefix, carried.gate);
        if first.row_1 {
            self.row_1 = Some([reader.prefixed()?, reader.prefixed()?, reader.prefixed()?]);
        }
        self.row_2 = read_position(reader, first.row_2)?;
        self.row_3_position = read_position(reader, first.row_3_position)?;
        self.row_3_id = first
            .row_3_id
            .then(|| Ok::<_, MessageError>((reader.array()?, reader.u64()?)))
            .transpose()?;
        if first.row_4 {
            let gate = reader.u8()?;
            self.row_4 = Some(gate);
            carried.gate = gate;
        }
        let layout = RowLayout::new(self.mask, carried.prefix, carried.gate);
        self.row_5_position = read_position(reader, layout.row_5_position)?;
        self.row_5_block = layout.row_5_block.then(|| reader.array()).transpose()?;
        self.row_6 = layout.row_6.then(|| reader.array()).transpose()?;
        let [len_104, len_192, len_96] = ELEMENT_LENS;
        self.elements_104 = read_elements(reader, len_104)?;
        self.elements_192 = read_elements(reader, len_192)?;
        self.elements_96 = read_elements(reader, len_96)?;
        self.row_10 = read_position(reader, layout.row_10)?;
        self.row_11 = layout
            .row_11
            .then(|| Ok::<_, MessageError>([reader.u16()?, reader.u16()?, reader.u16()?]))
            .transpose()?;
        self.row_12 = layout.row_12.then(|| reader.u64()).transpose()?;
        self.row_13 = layout.row_13.then(|| reader.u8()).transpose()?;
        self.row_14 = layout.row_14.then(|| reader.u8()).transpose()?;
        self.row_15 = layout.row_15.then(|| reader.u32()).transpose()?;
        self.row_16 = layout.row_16.then(|| reader.u16()).transpose()?;
        Ok(())
    }

    /// The rows this record holds, in the shape of [`RowLayout`].
    fn present_rows(&self) -> RowLayout {
        RowLayout {
            row_1: self.row_1.is_some(),
            row_2: self.row_2.is_some(),
            row_3_position: self.row_3_position.is_some(),
            row_3_id: self.row_3_id.is_some(),
            row_4: self.row_4.is_some(),
            row_5_position: self.row_5_position.is_some(),
            row_5_block: self.row_5_block.is_some(),
            row_6: self.row_6.is_some(),
            row_10: self.row_10.is_some(),
            row_11: self.row_11.is_some(),
            row_12: self.row_12.is_some(),
            row_13: self.row_13.is_some(),
            row_14: self.row_14.is_some(),
            row_15: self.row_15.is_some(),
            row_16: self.row_16.is_some(),
        }
    }

    /// Writes the record after the records that left `carried`. Refuses a record whose fields
    /// do not match its mask, or whose rows are not the ones `decode` would read.
    fn write(&self, writer: &mut Writer, carried: &mut Carried) -> Result<(), MessageError> {
        self.check_mask()?;
        if let Some(prefix) = self.bit_0 {
            carried.prefix = prefix & u64::from(u32::MAX);
        }
        if let Some(gate) = self.row_4 {
            carried.gate = gate;
        }
        if self.present_rows() != RowLayout::new(self.mask, carried.prefix, carried.gate) {
            return Err(MessageError::RowsMismatch);
        }
        let mask_bytes = self.mask.to_le_bytes();
        writer
            .bytes(&mask_bytes[..MASK_LEN])
            .u16(self.u16_after_mask);
        if let Some(prefix) = self.bit_0 {
            writer.wide_prefixed(prefix);
        }
        if let Some(value) = self.bit_1 {
            writer.u8(value);
        }
        if let Some(value) = self.bit_2 {
            writer.boolean(value);
        }
        if let Some(value) = self.bit_3 {
            writer.u8(value);
        }
        if let Some(value) = self.bit_4 {
            writer.u16(value);
        }
        self.slots.write(writer)?;
        if let Some(value) = self.bit_5 {
            writer.f32(value);
        }
        if let Some([first, second]) = self.bit_6 {
            writer.f32(first).f32(second);
        }
        if let Some(position) = &self.bit_7 {
            position.write(writer);
        }
        match &self.after_position {
            None => {
                writer.u8(AFTER_POSITION_END);
            }
            Some((AFTER_POSITION_END, _)) => {
                return Err(MessageError::BadTag {
                    field: "after_position",
                    tag: AFTER_POSITION_END,
                });
            }
            Some((tag, bytes)) => {
                writer.u8(*tag).bytes(bytes);
            }
        }
        match &self.bit_8 {
            None => {}
            Some(Bit8::Empty) => {
                writer.boolean(false);
            }
            Some(Bit8::Body(body)) => {
                let mut check = Reader::new(body);
                read_long_body(&mut check)?;
                check.finish()?;
                writer.boolean(true).bytes(body);
            }
        }
        self.write_rows(writer)
    }

    fn write_rows(&self, writer: &mut Writer) -> Result<(), MessageError> {
        if let Some(values) = self.row_1 {
            for value in values {
                writer.prefixed(value)?;
            }
        }
        for position in [&self.row_2, &self.row_3_position].into_iter().flatten() {
            position.write(writer);
        }
        if let Some((raw, value)) = &self.row_3_id {
            writer.bytes(raw).u64(*value);
        }
        if let Some(gate) = self.row_4 {
            writer.u8(gate);
        }
        if let Some(position) = &self.row_5_position {
            position.write(writer);
        }
        if let Some(block) = &self.row_5_block {
            writer.bytes(block);
        }
        if let Some(raw) = &self.row_6 {
            writer.bytes(raw);
        }
        for (elements, len) in [&self.elements_104, &self.elements_192, &self.elements_96]
            .into_iter()
            .zip(ELEMENT_LENS)
        {
            write_elements(writer, elements, len)?;
        }
        if let Some(position) = &self.row_10 {
            position.write(writer);
        }
        if let Some(values) = self.row_11 {
            for value in values {
                writer.u16(value);
            }
        }
        if let Some(value) = self.row_12 {
            writer.u64(value);
        }
        for value in [self.row_13, self.row_14].into_iter().flatten() {
            writer.u8(value);
        }
        if let Some(value) = self.row_15 {
            writer.u32(value);
        }
        if let Some(value) = self.row_16 {
            writer.u16(value);
        }
        Ok(())
    }

    /// Each fixed field is present exactly when its mask bit is set.
    fn check_mask(&self) -> Result<(), MessageError> {
        let present = [
            self.bit_0.is_some(),
            self.bit_1.is_some(),
            self.bit_2.is_some(),
            self.bit_3.is_some(),
            self.bit_4.is_some(),
            self.bit_5.is_some(),
            self.bit_6.is_some(),
            self.bit_7.is_some(),
            self.bit_8.is_some(),
        ];
        match (0..FIXED_BITS).find(|&bit| mask_bit(self.mask, bit) != present[bit as usize]) {
            Some(bit) => Err(MessageError::MaskMismatch {
                // `bit` is below 9.
                bit: bit as u8,
            }),
            None => Ok(()),
        }
    }
}

fn read_position(
    reader: &mut Reader<'_>,
    present: bool,
) -> Result<Option<InputPosition>, MessageError> {
    present.then(|| InputPosition::read(reader)).transpose()
}

fn read_elements(reader: &mut Reader<'_>, len: usize) -> Result<Vec<u8>, MessageError> {
    let count = reader.prefixed()?;
    if count > MAX_ELEMENTS {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_ELEMENTS,
        });
    }
    let total = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(len))
        .ok_or(MessageError::CountTooLarge {
            count,
            limit: MAX_ELEMENTS,
        })?;
    Ok(reader.bytes(total)?.to_vec())
}

fn write_elements(writer: &mut Writer, elements: &[u8], len: usize) -> Result<(), MessageError> {
    if !elements.len().is_multiple_of(len) {
        return Err(MessageError::LengthMismatch { field: "elements" });
    }
    writer.prefixed(u64::try_from(elements.len() / len).unwrap_or(u64::MAX))?;
    writer.bytes(elements);
    Ok(())
}

/// A string of the bit 8 body (`FUN_14087a3d0`): a prefix-coded length and the bytes.
fn skip_string(reader: &mut Reader<'_>) -> Result<(), MessageError> {
    let len = reader.prefixed()?;
    if len > MAX_STRING {
        return Err(MessageError::CountTooLarge {
            count: len,
            limit: MAX_STRING,
        });
    }
    // `len` is at most `MAX_STRING`.
    reader.bytes(len as usize)?;
    Ok(())
}

/// Long body steps 1 to 7: 16 raw bytes, `u64`, `u64`, `u8`, `u8`, `u64`, `u32`.
const LONG_START_LEN: usize = 46;
/// Steps 8 (after its string) and 9: two `u32`, then an `f32`.
const LONG_AFTER_NAME_LEN: usize = 12;
/// Step 17: two groups of three `f32`, when step 16 is 1.
const LONG_VECTORS_LEN: usize = 24;
/// Steps 19 to 25: a `u32`, a `u8`, five `u32`.
const LONG_COUNTERS_LEN: usize = 25;
/// Steps 30 and 31: a `u32` and a `u8`.
const LONG_STEPS_30_31_LEN: usize = 5;
/// Step 33 after its string, and step 34: a `u32`, a `u64`, a `u32`.
const LONG_TAIL_LEN: usize = 16;
/// Steps 11 to 14 are strings.
const LONG_STRINGS_11_TO_14: usize = 4;

/// Reads the long body of bit 8 (the `FUN_1435ebc30` table of the widths that the movement
/// message note cites) and returns its bytes.
fn read_long_body<'a>(reader: &mut Reader<'a>) -> Result<&'a [u8], MessageError> {
    let start = reader.rest();
    let before = reader.remaining();
    reader.bytes(LONG_START_LEN)?;
    skip_string(reader)?;
    reader.bytes(LONG_AFTER_NAME_LEN)?;
    reader.boolean()?;
    for _ in 0..LONG_STRINGS_11_TO_14 {
        skip_string(reader)?;
    }
    reader.f32()?;
    if reader.boolean()? {
        reader.bytes(LONG_VECTORS_LEN)?;
    }
    reader.boolean()?;
    reader.bytes(LONG_COUNTERS_LEN)?;
    reader.prefixed()?;
    reader.u32()?;
    reader.prefixed()?;
    skip_string(reader)?;
    reader.bytes(LONG_STEPS_30_31_LEN)?;
    reader.boolean()?;
    reader.u32()?;
    skip_string(reader)?;
    reader.bytes(LONG_TAIL_LEN)?;
    reader.boolean()?;
    let used = before - reader.remaining();
    Ok(&start[..used])
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const X: f32 = 11_132.5;
    const Y: f32 = 7_305.9;
    const Z: f32 = 170.0;
    const HEIGHT_STEP_M: f32 = 0.02;
    const PREFIX_MASK_ROW_1: u64 = 1;
    const PREFIX_MASK_ROW_2: u64 = 1 << 11;
    const GATE: u8 = 5;
    const ELEMENT_COUNT: usize = 2;

    fn position() -> InputPosition {
        InputPosition::from_metres(X, Y, Z).expect("finite")
    }

    /// A record with every mask bit 0 to 8 set and the rows of prefix bits 0 and 11.
    fn full_record() -> InputRecord {
        InputRecord {
            mask: 0x1FF | (1 << 9) | (1 << 10),
            u16_after_mask: 7,
            bit_0: Some(PREFIX_MASK_ROW_1 | PREFIX_MASK_ROW_2),
            bit_1: Some(1),
            bit_2: Some(true),
            bit_3: Some(3),
            bit_4: Some(4),
            slots: InputSlots {
                count: 3,
                values: vec![u64::MAX, 9],
                bytes: [1, 2, 3, 4],
            },
            bit_5: Some(1.5),
            bit_6: Some([2.5, 3.5]),
            bit_7: Some(position()),
            after_position: Some((0, [6; AFTER_POSITION_LEN])),
            bit_8: Some(Bit8::Empty),
            row_1: Some([1, 2, 3]),
            row_2: Some(position()),
            row_4: Some(GATE),
            elements_104: vec![8; ELEMENT_LENS[0] * ELEMENT_COUNT],
            ..InputRecord::default()
        }
    }

    fn message(records: Vec<InputRecord>) -> ClientInputMsg {
        ClientInputMsg {
            component: 1,
            u64_at_8: 2,
            head: InputHead {
                bytes_1_to_4: [1, 2, 3, 4],
                step_5: u64::from(u32::MAX),
                steps_6_7: [6, 7],
                step_8: 8,
                step_9: HeadStep9 {
                    prefixed: Some(9),
                    u64_at_end: 10,
                },
                step_10: 10,
                steps_11_to_13: [11, 12, 13],
                step_14: 14,
                step_15_pairs: vec![[15, 16]],
                step_15_tail: [17, 18],
                steps_16_17: [19, 20],
            },
            records,
        }
    }

    #[test]
    fn a_position_only_input_round_trips_and_gives_its_position() {
        let input = ClientInputMsg::position(position());
        let body = input.encode().expect("encode");
        let decoded = ClientInputMsg::decode(&body).expect("decode");
        assert_eq!(decoded, input);
        assert_eq!(decoded.last_position(), Some(position()));
        assert_eq!(decoded.encode().expect("again"), body);
    }

    #[test]
    fn attack_bits_read_bits_9_10_13_15() {
        let record = |mask| InputRecord {
            mask,
            ..InputRecord::default()
        };
        assert_eq!(record(0).attack_bits(), 0);
        assert_eq!(record(1 << 9).attack_bits(), 0b0001);
        assert_eq!(record(1 << 10).attack_bits(), 0b0010);
        assert_eq!(record(1 << 13).attack_bits(), 0b0100);
        assert_eq!(record(1 << 15).attack_bits(), 0b1000);
        assert_eq!(
            record(1 << 7 | 1 << 8 | 1 << 11).attack_bits(),
            0,
            "not attack bits"
        );
    }

    #[test]
    fn an_attack_input_sets_bit_9_and_the_position() {
        let input = ClientInputMsg::attack(position());
        let body = input.encode().expect("encode");
        let decoded = ClientInputMsg::decode(&body).expect("decode");
        assert_eq!(decoded, input);
        assert_eq!(decoded.last_position(), Some(position()));
        assert_eq!(decoded.records[0].attack_bits(), 0b0001);
    }

    #[test]
    fn an_attack_input_ends_with_a_release() {
        let decoded =
            ClientInputMsg::decode(&ClientInputMsg::attack(position()).encode().expect("encode"))
                .expect("decode");
        let bits: Vec<u8> = decoded
            .records
            .iter()
            .map(InputRecord::attack_bits)
            .collect();
        assert_eq!(bits, vec![0b0001, 0], "the next attack input rises again");
        assert_eq!(decoded.records[1].bit_7, Some(position()));
    }

    #[test]
    fn the_height_maps_from_minus_100_to_1000() {
        let low = InputPosition::from_metres(0.0, 0.0, INPUT_Z_MIN).expect("finite");
        let high = InputPosition::from_metres(0.0, 0.0, INPUT_Z_MAX).expect("finite");
        assert_eq!((low.z_raw, high.z_raw), (0, u16::MAX));
        assert!((position().z() - Z).abs() < HEIGHT_STEP_M);
        assert_eq!(InputPosition::from_metres(f32::NAN, 0.0, 0.0), None);
    }

    #[test]
    fn a_full_record_round_trips() {
        let input = message(vec![full_record()]);
        let body = input.encode().expect("encode");
        assert_eq!(ClientInputMsg::decode(&body), Ok(input));
    }

    #[test]
    fn the_prefix_and_the_gate_carry_to_the_next_record() {
        // The second record sets neither mask bit 0 nor the gate bit: rows 1 and 2 still run
        // (carried prefix), and row 5 runs (carried gate; its position is mask bit 11).
        let carried = InputRecord {
            mask: (1 << 9) | (1 << 11),
            row_1: Some([4, 5, 6]),
            row_2: Some(position()),
            row_5_position: Some(position()),
            ..InputRecord::default()
        };
        let input = message(vec![full_record(), carried]);
        let body = input.encode().expect("encode");
        assert_eq!(ClientInputMsg::decode(&body), Ok(input));
    }

    #[test]
    fn a_long_bit_8_body_keeps_its_bytes() {
        let mut long = Writer::new();
        long.bytes(&[1; 16]).u64(2).u64(3).u8(4).u8(5).u64(6).u32(7);
        long.prefixed(2).expect("string").bytes(b"ab").u32(8).u32(9);
        long.f32(1.0).boolean(true);
        for _ in 0..4 {
            long.prefixed(0).expect("string");
        }
        long.f32(2.0)
            .boolean(true)
            .bytes(&[0; 24])
            .boolean(false)
            .u32(10)
            .u8(11);
        for value in 0..5 {
            long.u32(value);
        }
        long.prefixed(12)
            .expect("prefix")
            .u32(13)
            .prefixed(14)
            .expect("prefix");
        long.prefixed(0)
            .expect("string")
            .u32(15)
            .u8(16)
            .boolean(false);
        long.u32(17)
            .prefixed(1)
            .expect("string")
            .bytes(b"c")
            .u32(18)
            .u64(19);
        long.u32(20).boolean(true);
        let raw = long.finish();
        let record = InputRecord {
            mask: 1 << 8,
            bit_8: Some(Bit8::Body(raw)),
            ..InputRecord::default()
        };
        let input = message(vec![record]);
        let body = input.encode().expect("encode");
        assert_eq!(ClientInputMsg::decode(&body), Ok(input));
    }

    #[test]
    fn a_mask_that_does_not_match_the_fields_is_refused() {
        let record = InputRecord {
            mask: 1 << 7,
            ..InputRecord::default()
        };
        assert!(matches!(
            message(vec![record]).encode(),
            Err(MessageError::MaskMismatch { bit: 7 })
        ));
        let bad_end = InputRecord {
            after_position: Some((AFTER_POSITION_END, [0; AFTER_POSITION_LEN])),
            ..InputRecord::default()
        };
        assert!(message(vec![bad_end]).encode().is_err());
    }

    #[test]
    fn rows_that_the_mask_prefix_and_gate_do_not_hold_are_refused() {
        let position_only = || InputRecord {
            mask: 1 << 7,
            bit_7: Some(position()),
            ..InputRecord::default()
        };
        let gate_without_bit = InputRecord {
            row_4: Some(1),
            ..position_only()
        };
        assert_eq!(
            message(vec![gate_without_bit]).encode(),
            Err(MessageError::RowsMismatch)
        );
        let row_1_without_prefix = InputRecord {
            row_1: Some([1, 2, 3]),
            ..position_only()
        };
        assert_eq!(
            message(vec![row_1_without_prefix]).encode(),
            Err(MessageError::RowsMismatch)
        );
        let short_slots = InputRecord {
            slots: InputSlots {
                count: 3,
                values: vec![1],
                bytes: [0; 4],
            },
            ..position_only()
        };
        assert_eq!(
            message(vec![short_slots]).encode(),
            Err(MessageError::LengthMismatch { field: "slots" })
        );
        let odd_elements = InputRecord {
            elements_96: vec![0; ELEMENT_LENS[2] + 1],
            ..position_only()
        };
        assert_eq!(
            message(vec![odd_elements]).encode(),
            Err(MessageError::LengthMismatch { field: "elements" })
        );
    }

    #[test]
    fn a_short_body_or_bytes_left_over_are_errors() {
        let body = ClientInputMsg::position(position())
            .encode()
            .expect("encode");
        assert!(ClientInputMsg::decode(&body[..body.len() - 1]).is_err());
        let mut longer = body.clone();
        longer.push(0);
        assert!(matches!(
            ClientInputMsg::decode(&longer),
            Err(MessageError::TrailingBytes { left: 1 })
        ));
    }

    proptest! {
        #[test]
        fn random_bodies_never_panic(body in proptest::collection::vec(any::<u8>(), 0..400)) {
            let _ = ClientInputMsg::decode(&body);
        }
    }
}
