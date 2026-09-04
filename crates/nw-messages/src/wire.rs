// SPDX-License-Identifier: AGPL-3.0-or-later
//! Field readers and writers of message bodies (bootstrap bodies note, "Shared readers").
//! Integers and floats are big-endian. A raw id is 16 bytes, not swapped. A prefix-coded integer
//! uses the type-index prefix code of the envelope (not LEB128).

use nw_reflect::{TypeIndexError, decode_type_index_prefix, encode_type_index_prefix};
use thiserror::Error;

/// Bytes of a raw id field.
pub const RAW_ID_LEN: usize = 16;

/// A body that does not match its layout.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MessageError {
    /// The body ends before a field.
    #[error("body ends early: {needed} bytes needed, {available} left")]
    Truncated {
        /// Bytes the field needs.
        needed: usize,
        /// Bytes left in the body.
        available: usize,
    },
    /// Bytes are left after the last field.
    #[error("{left} bytes left after the last field")]
    TrailingBytes {
        /// Bytes left.
        left: usize,
    },
    /// A flag byte is not 0 or 1.
    #[error("flag byte {value} is not 0 or 1")]
    NotBoolean {
        /// The byte.
        value: u8,
    },
    /// A count is above its limit.
    #[error("count {count} is above the limit {limit}")]
    CountTooLarge {
        /// The count.
        count: u64,
        /// The limit.
        limit: u64,
    },
    /// Bits that the RE notes do not describe are set.
    #[error("{field}: unknown bits {bits:#04x}")]
    UnknownBits {
        /// The field.
        field: &'static str,
        /// The unknown bits that are set.
        bits: u8,
    },
    /// A replicated state class has no reader here, and its body has no length.
    #[error("no reader for replicated state class {class:?}")]
    UnknownMemberClass {
        /// The class type id.
        class: nw_reflect::TypeId,
    },
    /// A prefix-coded integer is cut off or too large.
    #[error("prefix-coded integer: {0}")]
    Prefix(#[from] TypeIndexError),
}

/// A message body with a known layout.
pub trait BodyCodec: Sized {
    /// File slot of the message class in `typeindex.json`.
    const TYPE_INDEX: u64;

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a value does not fit its field.
    fn encode(&self) -> Result<Vec<u8>, MessageError>;

    /// The message from its body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the bytes do not match the layout.
    fn decode(body: &[u8]) -> Result<Self, MessageError>;
}

/// Reads fields from the front of a body.
#[derive(Debug)]
pub struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    /// A reader over `body`.
    pub const fn new(body: &'a [u8]) -> Self {
        Self { rest: body }
    }

    /// The next `len` bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] when fewer bytes are left.
    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8], MessageError> {
        if self.rest.len() < len {
            return Err(MessageError::Truncated {
                needed: len,
                available: self.rest.len(),
            });
        }
        let (field, rest) = self.rest.split_at(len);
        self.rest = rest;
        Ok(field)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], MessageError> {
        let Some((field, rest)) = self.rest.split_first_chunk::<N>() else {
            return Err(MessageError::Truncated {
                needed: N,
                available: self.rest.len(),
            });
        };
        self.rest = rest;
        Ok(*field)
    }

    /// One byte.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] at the end of the body.
    pub fn u8(&mut self) -> Result<u8, MessageError> {
        Ok(u8::from_be_bytes(self.array()?))
    }

    /// A flag byte that must be 0 or 1.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::NotBoolean`] for another value.
    pub fn boolean(&mut self) -> Result<bool, MessageError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(MessageError::NotBoolean { value }),
        }
    }

    /// A big-endian `u16`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] when fewer bytes are left.
    pub fn u16(&mut self) -> Result<u16, MessageError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    /// A big-endian `u32`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] when fewer bytes are left.
    pub fn u32(&mut self) -> Result<u32, MessageError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    /// A big-endian `u64`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] when fewer bytes are left.
    pub fn u64(&mut self) -> Result<u64, MessageError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    /// A big-endian `f32`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] when fewer bytes are left.
    pub fn f32(&mut self) -> Result<f32, MessageError> {
        Ok(f32::from_be_bytes(self.array()?))
    }

    /// A 16-byte raw id.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] when fewer bytes are left.
    pub fn raw_id(&mut self) -> Result<[u8; RAW_ID_LEN], MessageError> {
        self.array()
    }

    /// A prefix-coded integer.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Prefix`] when the prefix is cut off.
    pub fn prefixed(&mut self) -> Result<u64, MessageError> {
        let prefix = decode_type_index_prefix(self.rest)?;
        self.bytes(prefix.len)?;
        Ok(prefix.index)
    }

    /// Ends the body.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::TrailingBytes`] when bytes are left.
    pub fn finish(self) -> Result<(), MessageError> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(MessageError::TrailingBytes {
                left: self.rest.len(),
            })
        }
    }
}

/// Writes fields of a body in order.
#[derive(Debug, Default)]
pub struct Writer {
    body: Vec<u8>,
}

impl Writer {
    /// An empty body.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds raw bytes.
    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.body.extend_from_slice(bytes);
        self
    }

    /// Adds one byte.
    pub fn u8(&mut self, value: u8) -> &mut Self {
        self.bytes(&[value])
    }

    /// Adds a flag byte, 0 or 1.
    pub fn boolean(&mut self, value: bool) -> &mut Self {
        self.u8(u8::from(value))
    }

    /// Adds a big-endian `u16`.
    pub fn u16(&mut self, value: u16) -> &mut Self {
        self.bytes(&value.to_be_bytes())
    }

    /// Adds a big-endian `u32`.
    pub fn u32(&mut self, value: u32) -> &mut Self {
        self.bytes(&value.to_be_bytes())
    }

    /// Adds a big-endian `u64`.
    pub fn u64(&mut self, value: u64) -> &mut Self {
        self.bytes(&value.to_be_bytes())
    }

    /// Adds a big-endian `f32`.
    pub fn f32(&mut self, value: f32) -> &mut Self {
        self.bytes(&value.to_be_bytes())
    }

    /// Adds a prefix-coded integer in its shortest form.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Prefix`] when `value` does not fit the prefix code.
    pub fn prefixed(&mut self, value: u64) -> Result<&mut Self, MessageError> {
        let prefix = encode_type_index_prefix(value)?;
        Ok(self.bytes(&prefix))
    }

    /// The finished body.
    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIXED_VALUES: [u64; 4] = [0, 127, 128, 12_702];
    const FLOAT: f32 = 6.0;

    #[test]
    fn fields_are_big_endian_and_round_trip() {
        let id = [7; RAW_ID_LEN];
        let mut writer = Writer::new();
        writer
            .u8(1)
            .boolean(true)
            .u16(0x0102)
            .u32(0x0304_0506)
            .u64(0x0708_090a_0b0c_0d0e)
            .f32(FLOAT)
            .bytes(&id);
        let body = writer.finish();
        assert_eq!(&body[2..4], &[1, 2]);
        assert_eq!(&body[4..8], &[3, 4, 5, 6]);
        let mut reader = Reader::new(&body);
        assert_eq!(reader.u8(), Ok(1));
        assert_eq!(reader.boolean(), Ok(true));
        assert_eq!(reader.u16(), Ok(0x0102));
        assert_eq!(reader.u32(), Ok(0x0304_0506));
        assert_eq!(reader.u64(), Ok(0x0708_090a_0b0c_0d0e));
        assert_eq!(reader.f32(), Ok(FLOAT));
        assert_eq!(reader.raw_id(), Ok(id));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn prefixed_integers_use_the_type_index_code() {
        for value in PREFIXED_VALUES {
            let mut writer = Writer::new();
            writer.prefixed(value).expect("fits");
            let body = writer.finish();
            assert_eq!(body, encode_type_index_prefix(value).expect("fits"));
            let mut reader = Reader::new(&body);
            assert_eq!(reader.prefixed(), Ok(value));
            assert_eq!(reader.finish(), Ok(()));
        }
    }

    #[test]
    fn a_value_beyond_the_prefix_code_is_an_error() {
        assert!(matches!(
            Writer::new().prefixed(u64::MAX),
            Err(MessageError::Prefix(_))
        ));
    }

    #[test]
    fn short_bodies_flags_and_leftovers_are_errors() {
        assert_eq!(
            Reader::new(&[1, 2]).u32(),
            Err(MessageError::Truncated {
                needed: 4,
                available: 2
            })
        );
        assert_eq!(
            Reader::new(&[2]).boolean(),
            Err(MessageError::NotBoolean { value: 2 })
        );
        assert_eq!(
            Reader::new(&[0]).finish(),
            Err(MessageError::TrailingBytes { left: 1 })
        );
    }
}
