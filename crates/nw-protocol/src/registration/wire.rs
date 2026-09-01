// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared integer, prefix-length, and 0-or-1 writers for registration bodies.
//!
//! String lengths use the type-index prefix from section 4. This is not LEB128.
//! Multi-byte integers are big-endian. `FUN_140876cd0` and `FUN_1408770e0` call `WS2_32` ordinal 8.
//! The matching readers call ordinal 14.

use crate::type_index::{decode_type_index_prefix, encode_type_index_prefix};

/// Octets in one raw byte.
pub(crate) const U8_OCTETS: usize = 1;

/// Octets in a big-endian `uint32`.
pub(crate) const UINT32_OCTETS: usize = 4;

/// Octets in a big-endian `uint64`.
pub(crate) const UINT64_OCTETS: usize = 8;

/// Sixteen raw bytes selected when bit 0 of a `FUN_140876fa0` flag is set.
pub const SIXTEEN_RAW_OCTETS: usize = 16;

/// A length-prefixed string at this length or above is written as one 0 byte
/// where the note states that rule.
///
/// `FUN_1407d54a0`, `FUN_1407d42c0`, and `FUN_1407d60d0` use `0x30000` or more.
pub const STRING_LENGTH_OMIT: usize = 0x30000;

/// `FUN_140876d80` replaces a byte string longer than this value with one 0 byte.
///
/// This is one less than [`STRING_LENGTH_OMIT`].
pub const LIST_STRING_LONGER_THAN: usize = 0x2ffff;

/// `FUN_1407d62b0` writes a count of 0 when the container length is above this value.
pub const LIST_COUNT_ABOVE: usize = 0x2000000;

/// Largest byte the registration readers accept for a 0-or-1 field.
pub(crate) const ZERO_OR_ONE_MAX: u8 = 1;

/// Wire byte 0.
pub(crate) const BYTE_ZERO: u8 = 0;

/// Wire byte 1.
pub(crate) const BYTE_ONE: u8 = 1;

/// Bit 0 of the flag byte written by `FUN_140876fa0`.
pub const FLAG_BIT_0_MASK: u8 = 1;

/// Write a length-prefixed string with the `0x30000` omission rule.
pub(crate) const OMIT_AT_STRING_LENGTH_LIMIT: bool = true;

/// Write a length-prefixed string with its bytes kept.
pub(crate) const KEEP_FULL_STRING: bool = false;

const _: () = assert!(STRING_LENGTH_OMIT == LIST_STRING_LONGER_THAN + 1);

/// Failure from a registration body codec.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistrationError {
    /// The type-index prefix codec rejected the length or the buffer.
    #[error(transparent)]
    TypeIndex(#[from] crate::type_index::TypeIndexError),
    /// The buffer ended before a field was complete.
    #[error("truncated registration buffer: need {needed} bytes, have {available}")]
    Truncated {
        /// Bytes this read still needs.
        needed: usize,
        /// Bytes left in the buffer.
        available: usize,
    },
    /// Bytes remain after the body.
    #[error("registration buffer has {remaining} trailing bytes")]
    TrailingBytes {
        /// Bytes left after the body.
        remaining: usize,
    },
    /// A prefix length does not fit in `usize`.
    #[error("prefixed length {length} does not fit in usize")]
    LengthTooLarge {
        /// Length from the prefix.
        length: u64,
    },
    /// A list count is above [`LIST_COUNT_ABOVE`]. The writer does not emit that count.
    #[error("list count {count} is above {limit}")]
    ListCountAboveLimit {
        /// Count read from the prefix.
        count: u64,
        /// Note limit. A count above this value is rejected.
        limit: usize,
    },
    /// A byte that must be 0 or 1 was 2 or more.
    #[error("byte {byte:#x} is not 0 or 1")]
    ZeroOrOneOutOfRange {
        /// Byte from the caller or the buffer.
        byte: u8,
    },
    /// The 0-or-1 byte is 0 and a following byte was set.
    #[error("0 or 1 byte is 0 and a following byte was set")]
    ZeroOrOneUnexpected,
    /// The 0-or-1 byte is 1 and the following byte is absent.
    #[error("0 or 1 byte is 1 and the following byte is absent")]
    ZeroOrOneMissing,
    /// Bit 0 of the flag byte does not match the following field.
    #[error("flag byte {flag:#x} bit 0 does not match the following field")]
    FlagBitMismatch {
        /// Flag byte that did not match.
        flag: u8,
    },
}

/// Cursor over a registration body.
pub(crate) struct Reader<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    pub(crate) const fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.pos)
    }

    fn rest(&self) -> &'a [u8] {
        &self.input[self.pos..]
    }

    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], RegistrationError> {
        let available = self.remaining();
        if len > available {
            return Err(RegistrationError::Truncated {
                needed: len,
                available,
            });
        }
        let start = self.pos;
        self.pos += len;
        Ok(&self.input[start..self.pos])
    }

    pub(crate) fn finish(self) -> Result<(), RegistrationError> {
        let remaining = self.remaining();
        if remaining == BYTE_ZERO as usize {
            Ok(())
        } else {
            Err(RegistrationError::TrailingBytes { remaining })
        }
    }
}

pub(crate) fn write_u8(out: &mut Vec<u8>, byte: u8) {
    out.push(byte);
}

pub(crate) fn read_u8(reader: &mut Reader<'_>) -> Result<u8, RegistrationError> {
    let bytes = reader.take(U8_OCTETS)?;
    Ok(bytes[0])
}

pub(crate) fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn read_u32(reader: &mut Reader<'_>) -> Result<u32, RegistrationError> {
    let mut bytes = [0_u8; UINT32_OCTETS];
    bytes.copy_from_slice(reader.take(UINT32_OCTETS)?);
    Ok(u32::from_be_bytes(bytes))
}

pub(crate) fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn read_u64(reader: &mut Reader<'_>) -> Result<u64, RegistrationError> {
    let mut bytes = [0_u8; UINT64_OCTETS];
    bytes.copy_from_slice(reader.take(UINT64_OCTETS)?);
    Ok(u64::from_be_bytes(bytes))
}

pub(crate) fn write_prefix_len(out: &mut Vec<u8>, len: usize) -> Result<(), RegistrationError> {
    let value = u64::try_from(len).expect("usize fits in u64");
    out.extend(encode_type_index_prefix(value)?);
    Ok(())
}

pub(crate) fn read_prefix_len(reader: &mut Reader<'_>) -> Result<u64, RegistrationError> {
    let prefix = decode_type_index_prefix(reader.rest())?;
    let _prefix_bytes = reader.take(prefix.len)?;
    Ok(prefix.index)
}

pub(crate) fn write_length_prefixed(
    out: &mut Vec<u8>,
    bytes: &[u8],
    omit_at_limit: bool,
) -> Result<(), RegistrationError> {
    if omit_at_limit && bytes.len() >= STRING_LENGTH_OMIT {
        write_prefix_len(out, 0)?;
        return Ok(());
    }
    write_prefix_len(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

pub(crate) fn read_length_prefixed(reader: &mut Reader<'_>) -> Result<Vec<u8>, RegistrationError> {
    let length = read_prefix_len(reader)?;
    let len = usize::try_from(length).map_err(|_| RegistrationError::LengthTooLarge { length })?;
    Ok(reader.take(len)?.to_vec())
}

pub(crate) fn read_array<const N: usize>(
    reader: &mut Reader<'_>,
) -> Result<[u8; N], RegistrationError> {
    let mut raw = [0_u8; N];
    raw.copy_from_slice(reader.take(N)?);
    Ok(raw)
}

pub(crate) fn write_zero_or_one(out: &mut Vec<u8>, byte: u8) -> Result<(), RegistrationError> {
    if byte > ZERO_OR_ONE_MAX {
        return Err(RegistrationError::ZeroOrOneOutOfRange { byte });
    }
    write_u8(out, byte);
    Ok(())
}

pub(crate) fn read_zero_or_one(reader: &mut Reader<'_>) -> Result<u8, RegistrationError> {
    let byte = read_u8(reader)?;
    if byte > ZERO_OR_ONE_MAX {
        return Err(RegistrationError::ZeroOrOneOutOfRange { byte });
    }
    Ok(byte)
}

pub(crate) fn write_zero_or_one_then_optional(
    out: &mut Vec<u8>,
    first: u8,
    second: Option<u8>,
) -> Result<(), RegistrationError> {
    if first > ZERO_OR_ONE_MAX {
        return Err(RegistrationError::ZeroOrOneOutOfRange { byte: first });
    }
    match (first, second) {
        (BYTE_ZERO, None) => {
            write_u8(out, BYTE_ZERO);
            Ok(())
        }
        (BYTE_ONE, Some(second)) => {
            write_u8(out, BYTE_ONE);
            write_zero_or_one(out, second)
        }
        (BYTE_ZERO, Some(_)) => Err(RegistrationError::ZeroOrOneUnexpected),
        (BYTE_ONE, None) => Err(RegistrationError::ZeroOrOneMissing),
        _ => Err(RegistrationError::ZeroOrOneOutOfRange { byte: first }),
    }
}

pub(crate) fn read_zero_or_one_then_optional(
    reader: &mut Reader<'_>,
) -> Result<(u8, Option<u8>), RegistrationError> {
    let first = read_zero_or_one(reader)?;
    if first == BYTE_ZERO {
        Ok((first, None))
    } else {
        Ok((first, Some(read_zero_or_one(reader)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_U32: u32 = 0xA1B2_C3D4;
    const SAMPLE_U32_BE: [u8; UINT32_OCTETS] = [0xA1, 0xB2, 0xC3, 0xD4];
    const SAMPLE_U64: u64 = 0x0102_0304_0506_0708;
    const SAMPLE_U64_BE: [u8; UINT64_OCTETS] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
    const SHORT_BYTES: [u8; 2] = [b'a', b'b'];
    const EMPTY_PREFIX: [u8; 1] = [BYTE_ZERO];
    const OMIT_FILL: u8 = 0xEE;
    const KEPT_FILL: u8 = 0x11;
    const OUT_OF_RANGE_BYTE: u8 = 2;
    const TRAILING_BYTE: u8 = 0xFF;

    #[test]
    fn integers_are_big_endian() {
        let mut out = Vec::new();
        write_u32(&mut out, SAMPLE_U32);
        write_u64(&mut out, SAMPLE_U64);
        assert_eq!(&out[..UINT32_OCTETS], &SAMPLE_U32_BE);
        assert_eq!(&out[UINT32_OCTETS..], &SAMPLE_U64_BE);
        let mut reader = Reader::new(&out);
        assert_eq!(read_u32(&mut reader).expect("u32"), SAMPLE_U32);
        assert_eq!(read_u64(&mut reader).expect("u64"), SAMPLE_U64);
        reader.finish().expect("no trailing bytes");
    }

    #[test]
    fn short_and_empty_strings_use_the_prefix() {
        let mut out = Vec::new();
        write_length_prefixed(&mut out, &SHORT_BYTES, KEEP_FULL_STRING).expect("short");
        write_length_prefixed(&mut out, &[], KEEP_FULL_STRING).expect("empty");
        assert_eq!(out[0], u8::try_from(SHORT_BYTES.len()).expect("short len"));
        assert_eq!(&out[1..3], &SHORT_BYTES);
        assert_eq!(out[3], BYTE_ZERO);
        let mut reader = Reader::new(&out);
        assert_eq!(
            read_length_prefixed(&mut reader).expect("short"),
            SHORT_BYTES
        );
        assert_eq!(
            read_length_prefixed(&mut reader).expect("empty"),
            Vec::<u8>::new()
        );
        reader.finish().expect("done");
    }

    #[test]
    fn string_at_the_omit_limit_becomes_one_zero_byte() {
        let omitted = vec![OMIT_FILL; STRING_LENGTH_OMIT];
        let mut omitted_out = Vec::new();
        write_length_prefixed(&mut omitted_out, &omitted, OMIT_AT_STRING_LENGTH_LIMIT)
            .expect("omit");
        assert_eq!(omitted_out, EMPTY_PREFIX);
        assert!(!omitted_out.contains(&OMIT_FILL));

        let kept_len = STRING_LENGTH_OMIT - 1;
        assert_eq!(kept_len, LIST_STRING_LONGER_THAN);
        let kept = vec![KEPT_FILL; kept_len];
        let mut kept_out = Vec::new();
        write_length_prefixed(&mut kept_out, &kept, OMIT_AT_STRING_LENGTH_LIMIT).expect("keep");
        let mut reader = Reader::new(&kept_out);
        assert_eq!(read_length_prefixed(&mut reader).expect("kept"), kept);
        reader.finish().expect("kept done");

        let mut full = Vec::new();
        write_length_prefixed(&mut full, &omitted, KEEP_FULL_STRING).expect("full");
        assert!(full.len() > STRING_LENGTH_OMIT);
        let mut full_reader = Reader::new(&full);
        assert_eq!(
            read_length_prefixed(&mut full_reader).expect("full"),
            omitted
        );
    }

    #[test]
    fn list_count_limit_fits_in_the_prefix() {
        let mut at_limit = Vec::new();
        write_prefix_len(&mut at_limit, LIST_COUNT_ABOVE).expect("limit");
        let decoded = decode_type_index_prefix(&at_limit).expect("decode limit");
        assert_eq!(decoded.index, u64::try_from(LIST_COUNT_ABOVE).expect("u64"));
        assert_eq!(decoded.len, at_limit.len());

        let above = LIST_COUNT_ABOVE + 1;
        let mut above_out = Vec::new();
        write_prefix_len(&mut above_out, above).expect("above");
        let decoded_above = decode_type_index_prefix(&above_out).expect("decode above");
        assert_eq!(decoded_above.index, u64::try_from(above).expect("u64"));
    }

    #[test]
    fn zero_or_one_rejects_two_or_more() {
        let mut out = Vec::new();
        write_zero_or_one(&mut out, BYTE_ZERO).expect("0");
        write_zero_or_one(&mut out, BYTE_ONE).expect("1");
        assert_eq!(out, [BYTE_ZERO, BYTE_ONE]);
        let error = write_zero_or_one(&mut out, OUT_OF_RANGE_BYTE).expect_err("2");
        assert_eq!(
            error,
            RegistrationError::ZeroOrOneOutOfRange {
                byte: OUT_OF_RANGE_BYTE
            }
        );
        let mut reader = Reader::new(&[OUT_OF_RANGE_BYTE]);
        assert_eq!(
            read_zero_or_one(&mut reader).expect_err("2"),
            RegistrationError::ZeroOrOneOutOfRange {
                byte: OUT_OF_RANGE_BYTE
            }
        );
    }

    #[test]
    fn optional_second_byte_follows_only_when_the_first_is_one() {
        let mut absent = Vec::new();
        write_zero_or_one_then_optional(&mut absent, BYTE_ZERO, None).expect("0");
        assert_eq!(absent, [BYTE_ZERO]);
        let mut reader = Reader::new(&[BYTE_ZERO, TRAILING_BYTE]);
        assert_eq!(
            read_zero_or_one_then_optional(&mut reader).expect("read 0"),
            (BYTE_ZERO, None)
        );
        assert_eq!(reader.remaining(), 1);

        let mut present = Vec::new();
        write_zero_or_one_then_optional(&mut present, BYTE_ONE, Some(BYTE_ZERO)).expect("1");
        assert_eq!(present, [BYTE_ONE, BYTE_ZERO]);
        let mut present_reader = Reader::new(&present);
        assert_eq!(
            read_zero_or_one_then_optional(&mut present_reader).expect("read 1"),
            (BYTE_ONE, Some(BYTE_ZERO))
        );
        present_reader.finish().expect("done");

        assert_eq!(
            write_zero_or_one_then_optional(&mut Vec::new(), BYTE_ZERO, Some(BYTE_ONE))
                .expect_err("extra"),
            RegistrationError::ZeroOrOneUnexpected
        );
        assert_eq!(
            write_zero_or_one_then_optional(&mut Vec::new(), BYTE_ONE, None).expect_err("missing"),
            RegistrationError::ZeroOrOneMissing
        );
        assert_eq!(
            read_zero_or_one_then_optional(&mut Reader::new(&[BYTE_ONE])).expect_err("truncated"),
            RegistrationError::Truncated {
                needed: U8_OCTETS,
                available: 0,
            }
        );
        assert_eq!(
            read_zero_or_one_then_optional(&mut Reader::new(&[BYTE_ONE, OUT_OF_RANGE_BYTE]))
                .expect_err("second"),
            RegistrationError::ZeroOrOneOutOfRange {
                byte: OUT_OF_RANGE_BYTE
            }
        );
    }
}
