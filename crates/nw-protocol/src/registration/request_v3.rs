// SPDX-License-Identifier: AGPL-3.0-or-later
//! `RegistrationRequestV3Msg` body from protocol note section 3.
//!
//! Writer `FUN_1407ce860`. Reader `FUN_1407ce8e0` calls `FUN_1407d9620`.
//! Field names are absent from the binary. The names below are the note's offsets and functions.
//! This body is not a choice of V1, V2, or V3. V1 and V2 have no byte list.

use super::wire::{
    BYTE_ZERO, FLAG_BIT_0_MASK, KEEP_FULL_STRING, LIST_COUNT_ABOVE, OMIT_AT_STRING_LENGTH_LIMIT,
    Reader, RegistrationError, SIXTEEN_RAW_OCTETS, read_array, read_length_prefixed,
    read_prefix_len, read_u8, read_u32, read_zero_or_one, read_zero_or_one_then_optional,
    write_length_prefixed, write_prefix_len, write_u8, write_u32, write_zero_or_one,
    write_zero_or_one_then_optional,
};

/// Big-endian `uint32` at object +0x8.
pub const REGISTRATION_REQUEST_V3_UINT32_OFFSET: usize = 0x8;

/// `FUN_1407d62b0` list at object +0x10.
pub const REGISTRATION_REQUEST_V3_LIST_OFFSET: usize = 0x10;

/// Length-prefixed string at object +0xa0. `FUN_1407d54a0`.
pub const REGISTRATION_REQUEST_V3_STRING_OFFSET: usize = 0xa0;

/// `FUN_1407d5f30` at object +0xc0.
pub const REGISTRATION_REQUEST_V3_BLOCK_0XC0_OFFSET: usize = 0xc0;

/// Fields at object +0x2c8.
pub const REGISTRATION_REQUEST_V3_FIELDS_OFFSET: usize = 0x2c8;

/// `FUN_1407d3f70` object argument +0x3e0.
pub const REGISTRATION_REQUEST_V3_BLOCK_0X3E0_OFFSET: usize = 0x3e0;

/// Byte argument of `FUN_1407d3f70` at object +0x460.
pub const REGISTRATION_REQUEST_V3_BYTE_OFFSET: usize = 0x460;

const _: () = {
    assert!(REGISTRATION_REQUEST_V3_UINT32_OFFSET < REGISTRATION_REQUEST_V3_LIST_OFFSET);
    assert!(REGISTRATION_REQUEST_V3_LIST_OFFSET < REGISTRATION_REQUEST_V3_STRING_OFFSET);
    assert!(REGISTRATION_REQUEST_V3_STRING_OFFSET < REGISTRATION_REQUEST_V3_BLOCK_0XC0_OFFSET);
    assert!(REGISTRATION_REQUEST_V3_BLOCK_0XC0_OFFSET < REGISTRATION_REQUEST_V3_FIELDS_OFFSET);
    assert!(REGISTRATION_REQUEST_V3_FIELDS_OFFSET < REGISTRATION_REQUEST_V3_BLOCK_0X3E0_OFFSET);
    assert!(REGISTRATION_REQUEST_V3_BLOCK_0X3E0_OFFSET < REGISTRATION_REQUEST_V3_BYTE_OFFSET);
};

/// Four length-prefixed strings in `FUN_1407d4920`.
pub const BLOCK_1407D4920_STRING_COUNT: usize = 4;

/// Four length-prefixed strings in `FUN_1407d4b50`.
pub const BLOCK_1407D4B50_STRING_COUNT: usize = 4;

/// Three big-endian `uint32` values in `FUN_1407d42c0`.
pub const BLOCK_1407D42C0_UINT32_COUNT: usize = 3;

/// Two length-prefixed strings in `FUN_1407d4490`.
pub const BLOCK_1407D4490_STRING_COUNT: usize = 2;

/// Two length-prefixed strings on each side of the `uint32` in `FUN_1407d4540`.
pub const BLOCK_1407D4540_STRING_PAIR_COUNT: usize = 2;

/// One list node from `FUN_1407d62b0`: a big-endian `uint32`, then a length-prefixed byte string.
///
/// `FUN_140876d80` writes one 0 byte when the byte string is longer than `0x2ffff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListNode1407d62b0 {
    /// Big-endian `uint32`. The note does not name this field.
    pub uint32: u32,
    /// Length-prefixed byte string. The note does not name this field.
    pub bytes: Vec<u8>,
}

/// `FUN_1407d4490`: two length-prefixed strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block1407d4490 {
    /// Two length-prefixed strings. The note does not name them.
    pub strings: [Vec<u8>; BLOCK_1407D4490_STRING_COUNT],
}

/// `FUN_1407d46a0`: one length-prefixed string, then `FUN_1407d4490`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block1407d46a0 {
    /// Length-prefixed string. The note does not name this field.
    pub bytes: Vec<u8>,
    /// `FUN_1407d4490`.
    pub next: Block1407d4490,
}

/// `FUN_1407d42c0` byte list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block1407d42c0 {
    /// Length-prefixed string. Length `STRING_LENGTH_OMIT` or more is written as 0.
    pub string_0: Vec<u8>,
    /// Three big-endian `uint32` values. `FUN_140876cd0`.
    pub uint32s: [u32; BLOCK_1407D42C0_UINT32_COUNT],
    /// Length-prefixed string after the three `uint32` values.
    pub string_1: Vec<u8>,
    /// Big-endian `uint32` after `string_1`.
    pub uint32: u32,
    /// One raw byte. The note does not limit this byte to 0 or 1.
    pub raw_byte: u8,
    /// Length-prefixed string after `raw_byte`.
    pub string_2: Vec<u8>,
    /// One byte that is 0 or 1.
    pub zero_or_one: u8,
    /// Written when `zero_or_one` is 1. Absent when `zero_or_one` is 0.
    pub zero_or_one_when_one: Option<u8>,
    /// `FUN_1407d46a0`.
    pub next: Block1407d46a0,
}

/// `FUN_1407d4b50`: four length-prefixed strings, then `FUN_1407d42c0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block1407d4b50 {
    /// Four length-prefixed strings. The note does not name them.
    pub strings: [Vec<u8>; BLOCK_1407D4B50_STRING_COUNT],
    /// `FUN_1407d42c0`.
    pub next: Block1407d42c0,
}

/// `FUN_1407d4920`: four length-prefixed strings, one big-endian `uint32`, then `FUN_1407d4b50`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block1407d4920 {
    /// Four length-prefixed strings. The note does not name them.
    pub strings: [Vec<u8>; BLOCK_1407D4920_STRING_COUNT],
    /// Big-endian `uint32`. The note does not name this field.
    pub uint32: u32,
    /// `FUN_1407d4b50`.
    pub next: Block1407d4b50,
}

/// `FUN_1407d5f30` on object +0xc0: one length-prefixed string, then `FUN_1407d4920`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block1407d5f30 {
    /// Length-prefixed string. The note does not name this field.
    pub bytes: Vec<u8>,
    /// `FUN_1407d4920`.
    pub next: Block1407d4920,
}

/// `FUN_1407d4540`: two length-prefixed strings, one big-endian `uint32`, two length-prefixed strings, then `FUN_1407d46a0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block1407d4540 {
    /// Two length-prefixed strings before the `uint32`.
    pub strings_before: [Vec<u8>; BLOCK_1407D4540_STRING_PAIR_COUNT],
    /// Big-endian `uint32`. The note does not name this field.
    pub uint32: u32,
    /// Two length-prefixed strings after the `uint32`.
    pub strings_after: [Vec<u8>; BLOCK_1407D4540_STRING_PAIR_COUNT],
    /// `FUN_1407d46a0`.
    pub next: Block1407d46a0,
}

/// Object +0x2c8: big-endian `uint32`, 0 or 1, big-endian `uint32`, length-prefixed string, then `FUN_1407d4540`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldsAt0x2c8 {
    /// First big-endian `uint32`. The note does not name this field.
    pub uint32_0: u32,
    /// One byte that is 0 or 1.
    pub zero_or_one: u8,
    /// Second big-endian `uint32`. The note does not name this field.
    pub uint32_1: u32,
    /// Length-prefixed string. The note does not name this field.
    pub bytes: Vec<u8>,
    /// `FUN_1407d4540`.
    pub next: Block1407d4540,
}

/// Field that follows the flag byte from `FUN_140876fa0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlagBit0Field {
    /// Bit 0 is set. The next field is 16 raw bytes.
    SixteenRaw([u8; SIXTEEN_RAW_OCTETS]),
    /// Bit 0 is clear. `FUN_1407d60d0` writes one length-prefixed string.
    LengthPrefixed(Vec<u8>),
}

/// One `FUN_140876fa0` flag byte and the field bit 0 selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagByte140876fa0 {
    /// Flag byte. The note does not name this field. Bit 0 selects `field`.
    pub flag: u8,
    /// Field selected by bit 0 of `flag`.
    pub field: FlagBit0Field,
}

/// `FUN_1407d3f70` on object +0x3e0 and the byte at +0x460.
///
/// Two `FUN_140876fa0` calls, then one byte that is 0 or 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block1407d3f70 {
    /// First `FUN_140876fa0` call.
    pub flagged_0: FlagByte140876fa0,
    /// Second `FUN_140876fa0` call.
    pub flagged_1: FlagByte140876fa0,
    /// One byte that is 0 or 1, after the two flag calls.
    pub zero_or_one: u8,
}

/// `RegistrationRequestV3Msg` body. The type index is not part of this byte list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationRequestV3Msg {
    /// Big-endian `uint32` from object +0x8. The note does not name this field.
    pub uint32_at_0x8: u32,
    /// `FUN_1407d62b0` on object +0x10.
    pub list_at_0x10: Vec<ListNode1407d62b0>,
    /// Length-prefixed string at object +0xa0.
    pub bytes_at_0xa0: Vec<u8>,
    /// `FUN_1407d5f30` on object +0xc0.
    pub block_at_0xc0: Block1407d5f30,
    /// Fields at object +0x2c8, then `FUN_1407d4540`.
    pub fields_at_0x2c8: FieldsAt0x2c8,
    /// `FUN_1407d3f70`.
    pub block_at_0x3e0: Block1407d3f70,
}

fn encoded_list_count(len: usize) -> usize {
    if len > LIST_COUNT_ABOVE { 0 } else { len }
}

fn write_list(out: &mut Vec<u8>, nodes: &[ListNode1407d62b0]) -> Result<(), RegistrationError> {
    let count = encoded_list_count(nodes.len());
    write_prefix_len(out, count)?;
    for node in nodes.iter().take(count) {
        write_u32(out, node.uint32);
        write_length_prefixed(out, &node.bytes, OMIT_AT_STRING_LENGTH_LIMIT)?;
    }
    Ok(())
}

fn read_list(reader: &mut Reader<'_>) -> Result<Vec<ListNode1407d62b0>, RegistrationError> {
    let count = read_prefix_len(reader)?;
    let limit = u64::try_from(LIST_COUNT_ABOVE).expect("list limit fits in u64");
    if count > limit {
        return Err(RegistrationError::ListCountAboveLimit {
            count,
            limit: LIST_COUNT_ABOVE,
        });
    }
    let count = usize::try_from(count).expect("list count fits in usize");
    let mut nodes = Vec::new();
    for _ in 0..count {
        nodes.push(ListNode1407d62b0 {
            uint32: read_u32(reader)?,
            bytes: read_length_prefixed(reader)?,
        });
    }
    Ok(nodes)
}

fn write_strings(out: &mut Vec<u8>, strings: &[Vec<u8>]) -> Result<(), RegistrationError> {
    for bytes in strings {
        write_length_prefixed(out, bytes, KEEP_FULL_STRING)?;
    }
    Ok(())
}

fn read_string_array<const N: usize>(
    reader: &mut Reader<'_>,
) -> Result<[Vec<u8>; N], RegistrationError> {
    let mut strings = core::array::from_fn(|_| Vec::new());
    for slot in &mut strings {
        *slot = read_length_prefixed(reader)?;
    }
    Ok(strings)
}

impl Block1407d4490 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        write_strings(out, &self.strings)
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        Ok(Self {
            strings: read_string_array(reader)?,
        })
    }
}

impl Block1407d46a0 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        write_length_prefixed(out, &self.bytes, KEEP_FULL_STRING)?;
        self.next.encode(out)
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        Ok(Self {
            bytes: read_length_prefixed(reader)?,
            next: Block1407d4490::decode(reader)?,
        })
    }
}

impl Block1407d42c0 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        write_length_prefixed(out, &self.string_0, OMIT_AT_STRING_LENGTH_LIMIT)?;
        for value in self.uint32s {
            write_u32(out, value);
        }
        write_length_prefixed(out, &self.string_1, KEEP_FULL_STRING)?;
        write_u32(out, self.uint32);
        write_u8(out, self.raw_byte);
        write_length_prefixed(out, &self.string_2, KEEP_FULL_STRING)?;
        write_zero_or_one_then_optional(out, self.zero_or_one, self.zero_or_one_when_one)?;
        self.next.encode(out)
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        let string_0 = read_length_prefixed(reader)?;
        let mut uint32s = [0_u32; BLOCK_1407D42C0_UINT32_COUNT];
        for slot in &mut uint32s {
            *slot = read_u32(reader)?;
        }
        let string_1 = read_length_prefixed(reader)?;
        let uint32 = read_u32(reader)?;
        let raw_byte = read_u8(reader)?;
        let string_2 = read_length_prefixed(reader)?;
        let (zero_or_one, zero_or_one_when_one) = read_zero_or_one_then_optional(reader)?;
        Ok(Self {
            string_0,
            uint32s,
            string_1,
            uint32,
            raw_byte,
            string_2,
            zero_or_one,
            zero_or_one_when_one,
            next: Block1407d46a0::decode(reader)?,
        })
    }
}

impl Block1407d4b50 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        write_strings(out, &self.strings)?;
        self.next.encode(out)
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        Ok(Self {
            strings: read_string_array(reader)?,
            next: Block1407d42c0::decode(reader)?,
        })
    }
}

impl Block1407d4920 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        write_strings(out, &self.strings)?;
        write_u32(out, self.uint32);
        self.next.encode(out)
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        Ok(Self {
            strings: read_string_array(reader)?,
            uint32: read_u32(reader)?,
            next: Block1407d4b50::decode(reader)?,
        })
    }
}

impl Block1407d5f30 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        write_length_prefixed(out, &self.bytes, KEEP_FULL_STRING)?;
        self.next.encode(out)
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        Ok(Self {
            bytes: read_length_prefixed(reader)?,
            next: Block1407d4920::decode(reader)?,
        })
    }
}

impl Block1407d4540 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        write_strings(out, &self.strings_before)?;
        write_u32(out, self.uint32);
        write_strings(out, &self.strings_after)?;
        self.next.encode(out)
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        Ok(Self {
            strings_before: read_string_array(reader)?,
            uint32: read_u32(reader)?,
            strings_after: read_string_array(reader)?,
            next: Block1407d46a0::decode(reader)?,
        })
    }
}

impl FieldsAt0x2c8 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        write_u32(out, self.uint32_0);
        write_zero_or_one(out, self.zero_or_one)?;
        write_u32(out, self.uint32_1);
        write_length_prefixed(out, &self.bytes, KEEP_FULL_STRING)?;
        self.next.encode(out)
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        Ok(Self {
            uint32_0: read_u32(reader)?,
            zero_or_one: read_zero_or_one(reader)?,
            uint32_1: read_u32(reader)?,
            bytes: read_length_prefixed(reader)?,
            next: Block1407d4540::decode(reader)?,
        })
    }
}

impl FlagByte140876fa0 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        let bit0_set = self.flag & FLAG_BIT_0_MASK != BYTE_ZERO;
        match (&self.field, bit0_set) {
            (FlagBit0Field::SixteenRaw(raw), true) => {
                write_u8(out, self.flag);
                out.extend_from_slice(raw);
                Ok(())
            }
            (FlagBit0Field::LengthPrefixed(bytes), false) => {
                write_u8(out, self.flag);
                write_length_prefixed(out, bytes, OMIT_AT_STRING_LENGTH_LIMIT)
            }
            _ => Err(RegistrationError::FlagBitMismatch { flag: self.flag }),
        }
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        let flag = super::wire::read_u8(reader)?;
        let field = if flag & FLAG_BIT_0_MASK != BYTE_ZERO {
            FlagBit0Field::SixteenRaw(read_array(reader)?)
        } else {
            FlagBit0Field::LengthPrefixed(read_length_prefixed(reader)?)
        };
        Ok(Self { flag, field })
    }
}

impl Block1407d3f70 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), RegistrationError> {
        self.flagged_0.encode(out)?;
        self.flagged_1.encode(out)?;
        write_zero_or_one(out, self.zero_or_one)
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, RegistrationError> {
        Ok(Self {
            flagged_0: FlagByte140876fa0::decode(reader)?,
            flagged_1: FlagByte140876fa0::decode(reader)?,
            zero_or_one: read_zero_or_one(reader)?,
        })
    }
}

impl RegistrationRequestV3Msg {
    /// Encode the V3 body. The bytes do not include a type index or a datagram header.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] when a 0-or-1 byte is out of range, a 0-or-1 pair disagrees
    /// with its extra byte, or a flag bit does not match the following field.
    pub fn encode(&self) -> Result<Vec<u8>, RegistrationError> {
        let mut out = Vec::new();
        write_u32(&mut out, self.uint32_at_0x8);
        write_list(&mut out, &self.list_at_0x10)?;
        write_length_prefixed(&mut out, &self.bytes_at_0xa0, OMIT_AT_STRING_LENGTH_LIMIT)?;
        self.block_at_0xc0.encode(&mut out)?;
        self.fields_at_0x2c8.encode(&mut out)?;
        self.block_at_0x3e0.encode(&mut out)?;
        Ok(out)
    }

    /// Decode a V3 body. `bytes` is the body only.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] when the buffer is short, a 0-or-1 byte is 2 or more,
    /// a list count is above [`LIST_COUNT_ABOVE`], or bytes remain after the body.
    pub fn decode(bytes: &[u8]) -> Result<Self, RegistrationError> {
        let mut reader = Reader::new(bytes);
        let message = Self {
            uint32_at_0x8: read_u32(&mut reader)?,
            list_at_0x10: read_list(&mut reader)?,
            bytes_at_0xa0: read_length_prefixed(&mut reader)?,
            block_at_0xc0: Block1407d5f30::decode(&mut reader)?,
            fields_at_0x2c8: FieldsAt0x2c8::decode(&mut reader)?,
            block_at_0x3e0: Block1407d3f70::decode(&mut reader)?,
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_index::{REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX, encode_type_index_prefix};

    const UINT32_AT_0X8: u32 = 0x0102_0304;
    const NODE_UINT32: u32 = 0x0000_007B;
    const NODE_BYTE: u8 = b'n';
    const BYTES_AT_0XA0: &[u8] = b"a0";
    const RAW_BYTE: u8 = 0xAB;
    const FLAG_STRING: u8 = 0x02;
    const FLAG_RAW: u8 = 0x03;
    const RAW_FILL: u8 = 0x10;
    const FLAG_STRING_BYTE: u8 = b'y';
    const FINAL_ZERO_OR_ONE: u8 = 1;
    const OUT_OF_RANGE_BYTE: u8 = 2;
    const OMIT_FILL: u8 = 0xEE;
    const LONG_STRING_FILL: u8 = 0x5A;
    const V3_TYPE_INDEX: u64 = 19;

    const V3_PREFIX: [u8; 14] = [
        0x01, 0x02, 0x03, 0x04, 0x01, 0x00, 0x00, 0x00, 0x7B, 0x01, NODE_BYTE, 0x02, b'a', b'0',
    ];

    fn two(first: &[u8], second: &[u8]) -> [Vec<u8>; BLOCK_1407D4490_STRING_COUNT] {
        [first.to_vec(), second.to_vec()]
    }

    fn four(a: &[u8], b: &[u8], c: &[u8], d: &[u8]) -> [Vec<u8>; BLOCK_1407D4920_STRING_COUNT] {
        [a.to_vec(), b.to_vec(), c.to_vec(), d.to_vec()]
    }

    fn block_4490(first: &[u8], second: &[u8]) -> Block1407d4490 {
        Block1407d4490 {
            strings: two(first, second),
        }
    }

    fn block_46a0(bytes: &[u8], first: &[u8], second: &[u8]) -> Block1407d46a0 {
        Block1407d46a0 {
            bytes: bytes.to_vec(),
            next: block_4490(first, second),
        }
    }

    fn sample() -> RegistrationRequestV3Msg {
        RegistrationRequestV3Msg {
            uint32_at_0x8: UINT32_AT_0X8,
            list_at_0x10: vec![ListNode1407d62b0 {
                uint32: NODE_UINT32,
                bytes: vec![NODE_BYTE],
            }],
            bytes_at_0xa0: BYTES_AT_0XA0.to_vec(),
            block_at_0xc0: Block1407d5f30 {
                bytes: b"c0".to_vec(),
                next: Block1407d4920 {
                    strings: four(b"r0", b"r1", b"r2", b"r3"),
                    uint32: 0x1111_1111,
                    next: Block1407d4b50 {
                        strings: four(b"b0", b"b1", b"b2", b"b3"),
                        next: Block1407d42c0 {
                            string_0: b"s0".to_vec(),
                            uint32s: [0x0A0A_0A0A, 0x0B0B_0B0B, 0x0C0C_0C0C],
                            string_1: b"s1".to_vec(),
                            uint32: 0x0D0D_0D0D,
                            raw_byte: RAW_BYTE,
                            string_2: b"s2".to_vec(),
                            zero_or_one: BYTE_ONE_PAIR,
                            zero_or_one_when_one: Some(BYTE_ZERO),
                            next: block_46a0(b"p", b"q0", b"q1"),
                        },
                    },
                },
            },
            fields_at_0x2c8: FieldsAt0x2c8 {
                uint32_0: 0x2222_2222,
                zero_or_one: BYTE_ZERO,
                uint32_1: 0x3333_3333,
                bytes: b"t".to_vec(),
                next: Block1407d4540 {
                    strings_before: two(b"u0", b"u1"),
                    uint32: 0x4444_4444,
                    strings_after: two(b"v0", b"v1"),
                    next: block_46a0(b"w", b"x0", b"x1"),
                },
            },
            block_at_0x3e0: Block1407d3f70 {
                flagged_0: FlagByte140876fa0 {
                    flag: FLAG_STRING,
                    field: FlagBit0Field::LengthPrefixed(vec![FLAG_STRING_BYTE]),
                },
                flagged_1: FlagByte140876fa0 {
                    flag: FLAG_RAW,
                    field: FlagBit0Field::SixteenRaw([RAW_FILL; SIXTEEN_RAW_OCTETS]),
                },
                zero_or_one: FINAL_ZERO_OR_ONE,
            },
        }
    }

    const BYTE_ZERO: u8 = 0;
    const BYTE_ONE_PAIR: u8 = 1;

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn push_short(out: &mut Vec<u8>, bytes: &[u8]) {
        assert!(bytes.len() <= usize::from(0x7F_u8));
        let len = u8::try_from(bytes.len()).expect("short string");
        out.push(len);
        out.extend_from_slice(bytes);
    }

    fn push_expected(message: &RegistrationRequestV3Msg) -> Vec<u8> {
        let mut out = Vec::new();
        push_u32(&mut out, message.uint32_at_0x8);
        let count = u8::try_from(message.list_at_0x10.len()).expect("sample list is short");
        out.push(count);
        for node in &message.list_at_0x10 {
            push_u32(&mut out, node.uint32);
            push_short(&mut out, &node.bytes);
        }
        push_short(&mut out, &message.bytes_at_0xa0);
        push_short(&mut out, &message.block_at_0xc0.bytes);
        let block_4920 = &message.block_at_0xc0.next;
        for bytes in &block_4920.strings {
            push_short(&mut out, bytes);
        }
        push_u32(&mut out, block_4920.uint32);
        let block_4b50 = &block_4920.next;
        for bytes in &block_4b50.strings {
            push_short(&mut out, bytes);
        }
        let block_42c0 = &block_4b50.next;
        push_short(&mut out, &block_42c0.string_0);
        for value in block_42c0.uint32s {
            push_u32(&mut out, value);
        }
        push_short(&mut out, &block_42c0.string_1);
        push_u32(&mut out, block_42c0.uint32);
        out.push(block_42c0.raw_byte);
        push_short(&mut out, &block_42c0.string_2);
        out.push(block_42c0.zero_or_one);
        if let Some(second) = block_42c0.zero_or_one_when_one {
            out.push(second);
        }
        push_short(&mut out, &block_42c0.next.bytes);
        for bytes in &block_42c0.next.next.strings {
            push_short(&mut out, bytes);
        }
        let fields = &message.fields_at_0x2c8;
        push_u32(&mut out, fields.uint32_0);
        out.push(fields.zero_or_one);
        push_u32(&mut out, fields.uint32_1);
        push_short(&mut out, &fields.bytes);
        for bytes in &fields.next.strings_before {
            push_short(&mut out, bytes);
        }
        push_u32(&mut out, fields.next.uint32);
        for bytes in &fields.next.strings_after {
            push_short(&mut out, bytes);
        }
        push_short(&mut out, &fields.next.next.bytes);
        for bytes in &fields.next.next.next.strings {
            push_short(&mut out, bytes);
        }
        let tail = &message.block_at_0x3e0;
        out.push(tail.flagged_0.flag);
        match &tail.flagged_0.field {
            FlagBit0Field::LengthPrefixed(bytes) => push_short(&mut out, bytes),
            FlagBit0Field::SixteenRaw(raw) => out.extend_from_slice(raw),
        }
        out.push(tail.flagged_1.flag);
        match &tail.flagged_1.field {
            FlagBit0Field::SixteenRaw(raw) => out.extend_from_slice(raw),
            FlagBit0Field::LengthPrefixed(bytes) => push_short(&mut out, bytes),
        }
        out.push(tail.zero_or_one);
        out
    }

    #[test]
    fn v3_type_index_is_the_file_slot() {
        assert_eq!(REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX, V3_TYPE_INDEX);
    }

    #[test]
    fn list_count_above_the_limit_encodes_as_zero() {
        assert_eq!(encoded_list_count(0), 0);
        assert_eq!(encoded_list_count(LIST_COUNT_ABOVE), LIST_COUNT_ABOVE);
        assert_eq!(encoded_list_count(LIST_COUNT_ABOVE + 1), 0);
    }

    #[test]
    fn v3_body_matches_the_write_order() {
        let message = sample();
        let encoded = message.encode().expect("encode");
        let expected = push_expected(&message);
        assert_eq!(encoded, expected);
        assert_eq!(&encoded[..V3_PREFIX.len()], &V3_PREFIX);
        assert_eq!(encoded.last().copied(), Some(FINAL_ZERO_OR_ONE));
        assert!(encoded.contains(&RAW_BYTE));
        let decoded = RegistrationRequestV3Msg::decode(&encoded).expect("decode");
        assert_eq!(decoded, message);
    }

    #[test]
    fn omit_rules_drop_long_byte_strings_and_keep_the_others() {
        let mut omitted_a0 = sample();
        omitted_a0.bytes_at_0xa0 = vec![OMIT_FILL; super::super::wire::STRING_LENGTH_OMIT];
        let mut empty_a0 = sample();
        empty_a0.bytes_at_0xa0.clear();
        assert_eq!(
            omitted_a0.encode().expect("omit a0"),
            empty_a0.encode().expect("empty a0")
        );
        let decoded = RegistrationRequestV3Msg::decode(&omitted_a0.encode().expect("decode omit"))
            .expect("decode");
        assert!(decoded.bytes_at_0xa0.is_empty());

        let mut omitted_node = sample();
        omitted_node.list_at_0x10[0].bytes =
            vec![OMIT_FILL; super::super::wire::STRING_LENGTH_OMIT];
        let mut empty_node = sample();
        empty_node.list_at_0x10[0].bytes.clear();
        assert_eq!(
            omitted_node.encode().expect("omit node"),
            empty_node.encode().expect("empty node")
        );

        let mut omitted_s0 = sample();
        omitted_s0.block_at_0xc0.next.next.next.string_0 =
            vec![OMIT_FILL; super::super::wire::STRING_LENGTH_OMIT];
        let mut empty_s0 = sample();
        empty_s0.block_at_0xc0.next.next.next.string_0.clear();
        assert_eq!(
            omitted_s0.encode().expect("omit s0"),
            empty_s0.encode().expect("empty s0")
        );

        let mut omitted_flag = sample();
        match &mut omitted_flag.block_at_0x3e0.flagged_0.field {
            FlagBit0Field::LengthPrefixed(bytes) => {
                *bytes = vec![OMIT_FILL; super::super::wire::STRING_LENGTH_OMIT];
            }
            FlagBit0Field::SixteenRaw(_) => panic!("flag 0 is a length-prefixed string"),
        }
        let mut empty_flag = sample();
        match &mut empty_flag.block_at_0x3e0.flagged_0.field {
            FlagBit0Field::LengthPrefixed(bytes) => bytes.clear(),
            FlagBit0Field::SixteenRaw(_) => panic!("flag 0 is a length-prefixed string"),
        }
        assert_eq!(
            omitted_flag.encode().expect("omit flag"),
            empty_flag.encode().expect("empty flag")
        );

        let mut kept = sample();
        let long = vec![LONG_STRING_FILL; super::super::wire::STRING_LENGTH_OMIT];
        kept.block_at_0xc0.next.next.next.string_1 = long.clone();
        let encoded = kept.encode().expect("keep string_1");
        assert!(encoded.len() > super::super::wire::STRING_LENGTH_OMIT);
        let prefix = encode_type_index_prefix(
            u64::try_from(super::super::wire::STRING_LENGTH_OMIT).expect("len"),
        )
        .expect("prefix");
        assert!(encoded.windows(prefix.len()).any(|window| window == prefix));
        let decoded = RegistrationRequestV3Msg::decode(&encoded).expect("decode kept");
        assert_eq!(decoded.block_at_0xc0.next.next.next.string_1, long);
    }

    #[test]
    fn invalid_flags_and_zero_or_one_bytes_fail() {
        let mut missing = sample();
        missing.block_at_0xc0.next.next.next.zero_or_one = BYTE_ONE_PAIR;
        missing.block_at_0xc0.next.next.next.zero_or_one_when_one = None;
        assert_eq!(
            missing.encode().expect_err("missing second"),
            RegistrationError::ZeroOrOneMissing
        );

        let mut extra = sample();
        extra.block_at_0xc0.next.next.next.zero_or_one = BYTE_ZERO;
        extra.block_at_0xc0.next.next.next.zero_or_one_when_one = Some(BYTE_ZERO);
        assert_eq!(
            extra.encode().expect_err("unexpected second"),
            RegistrationError::ZeroOrOneUnexpected
        );

        let mut range = sample();
        range.block_at_0x3e0.zero_or_one = OUT_OF_RANGE_BYTE;
        assert_eq!(
            range.encode().expect_err("range"),
            RegistrationError::ZeroOrOneOutOfRange {
                byte: OUT_OF_RANGE_BYTE
            }
        );

        let mismatch = FlagByte140876fa0 {
            flag: FLAG_RAW,
            field: FlagBit0Field::LengthPrefixed(Vec::new()),
        };
        let mut out = Vec::new();
        assert_eq!(
            mismatch.encode(&mut out).expect_err("mismatch"),
            RegistrationError::FlagBitMismatch { flag: FLAG_RAW }
        );
        assert!(out.is_empty());

        let encoded = sample().encode().expect("sample");
        let mut patched = encoded.clone();
        let last = patched.len() - 1;
        patched[last] = OUT_OF_RANGE_BYTE;
        assert_eq!(
            RegistrationRequestV3Msg::decode(&patched).expect_err("patched"),
            RegistrationError::ZeroOrOneOutOfRange {
                byte: OUT_OF_RANGE_BYTE
            }
        );
        let mut trailing = encoded;
        trailing.push(BYTE_ZERO);
        assert_eq!(
            RegistrationRequestV3Msg::decode(&trailing).expect_err("trailing"),
            RegistrationError::TrailingBytes { remaining: 1 }
        );
    }

    #[test]
    fn list_count_above_the_limit_is_rejected_on_decode() {
        let above = LIST_COUNT_ABOVE + 1;
        let count = encode_type_index_prefix(u64::try_from(above).expect("count")).expect("prefix");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&UINT32_AT_0X8.to_be_bytes());
        bytes.extend(count);
        assert_eq!(
            RegistrationRequestV3Msg::decode(&bytes).expect_err("count"),
            RegistrationError::ListCountAboveLimit {
                count: u64::try_from(above).expect("count"),
                limit: LIST_COUNT_ABOVE,
            }
        );
    }
}
