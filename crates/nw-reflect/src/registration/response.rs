// SPDX-License-Identifier: AGPL-3.0-or-later
//! `RegistrationResponseMsg` body from protocol note section 3.
//!
//! Writer `FUN_1407cceb0`. Reader `FUN_1407cd040`.
//! Field names are absent from the binary.

use super::wire::{
    KEEP_FULL_STRING, Reader, RegistrationError, read_length_prefixed, read_u32, read_u64,
    read_zero_or_one, write_length_prefixed, write_u32, write_u64, write_zero_or_one,
};

/// Big-endian `uint32` at object +0x8.
pub const REGISTRATION_RESPONSE_UINT32_OFFSET: usize = 0x8;

/// Length-prefixed string at object +0x18. `FUN_1407fc670`.
pub const REGISTRATION_RESPONSE_STRING_0_OFFSET: usize = 0x18;

/// Length-prefixed string at object +0x38.
pub const REGISTRATION_RESPONSE_STRING_1_OFFSET: usize = 0x38;

/// First of the four 0-or-1 bytes. Object +0x58.
pub const REGISTRATION_RESPONSE_FLAGS_OFFSET: usize = 0x58;

/// Last of the four 0-or-1 bytes. Object +0x5b.
pub const REGISTRATION_RESPONSE_FLAGS_LAST_OFFSET: usize = 0x5b;

/// Four bytes at object +0x58 through +0x5b. Each is 0 or 1.
pub const REGISTRATION_RESPONSE_FLAG_COUNT: usize = 4;

/// Status of an accepted registration (all 43 captured responses).
pub const REGISTRATION_STATUS_AUTHORIZED: u32 = 0;

/// Status of a refused registration. Any value other than 0 refuses; the emulator sends 1.
pub const REGISTRATION_STATUS_REFUSED: u32 = 1;

/// The four bytes of all 43 captured responses (meanings Unknown).
pub const REGISTRATION_RESPONSE_OBSERVED_FLAGS: [u8; REGISTRATION_RESPONSE_FLAG_COUNT] =
    [1, 0, 0, 1];

const _: () = {
    assert!(
        REGISTRATION_RESPONSE_FLAGS_OFFSET + REGISTRATION_RESPONSE_FLAG_COUNT - 1
            == REGISTRATION_RESPONSE_FLAGS_LAST_OFFSET
    );
    assert!(REGISTRATION_RESPONSE_UINT32_OFFSET < REGISTRATION_RESPONSE_STRING_0_OFFSET);
    assert!(REGISTRATION_RESPONSE_STRING_0_OFFSET < REGISTRATION_RESPONSE_STRING_1_OFFSET);
    assert!(REGISTRATION_RESPONSE_STRING_1_OFFSET < REGISTRATION_RESPONSE_FLAGS_OFFSET);
};

/// `RegistrationResponseMsg` body. The type index is not part of this byte list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationResponseMsg {
    /// Big-endian `uint32` at object +0x8. The note does not name this field.
    pub uint32_at_0x8: u32,
    /// Big-endian `uint64`. `FUN_1408770e0` calls `htonl` on each 32-bit half.
    /// The note does not name this field.
    pub uint64: u64,
    /// Length-prefixed string at object +0x18. The note does not name this field.
    pub bytes_at_0x18: Vec<u8>,
    /// Length-prefixed string at object +0x38. The note does not name this field.
    pub bytes_at_0x38: Vec<u8>,
    /// Four bytes at object +0x58 through +0x5b. Each is 0 or 1.
    pub bytes_at_0x58: [u8; REGISTRATION_RESPONSE_FLAG_COUNT],
}

impl RegistrationResponseMsg {
    /// Encode the response body. The bytes do not include a type index or a datagram header.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::ZeroOrOneOutOfRange`] when one of the four flag bytes is 2 or more.
    pub fn encode(&self) -> Result<Vec<u8>, RegistrationError> {
        let mut out = Vec::new();
        write_u32(&mut out, self.uint32_at_0x8);
        write_u64(&mut out, self.uint64);
        write_length_prefixed(&mut out, &self.bytes_at_0x18, KEEP_FULL_STRING)?;
        write_length_prefixed(&mut out, &self.bytes_at_0x38, KEEP_FULL_STRING)?;
        for byte in self.bytes_at_0x58 {
            write_zero_or_one(&mut out, byte)?;
        }
        Ok(out)
    }

    /// Decode a response body. `bytes` is the body only.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] when the buffer is short, a flag byte is 2 or more,
    /// or bytes remain after the body.
    pub fn decode(bytes: &[u8]) -> Result<Self, RegistrationError> {
        let mut reader = Reader::new(bytes);
        let uint32_at_0x8 = read_u32(&mut reader)?;
        let uint64 = read_u64(&mut reader)?;
        let bytes_at_0x18 = read_length_prefixed(&mut reader)?;
        let bytes_at_0x38 = read_length_prefixed(&mut reader)?;
        let mut bytes_at_0x58 = [0_u8; REGISTRATION_RESPONSE_FLAG_COUNT];
        for slot in &mut bytes_at_0x58 {
            *slot = read_zero_or_one(&mut reader)?;
        }
        reader.finish()?;
        Ok(Self {
            uint32_at_0x8,
            uint64,
            bytes_at_0x18,
            bytes_at_0x38,
            bytes_at_0x58,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UINT32_AT_0X8: u32 = 0x1122_3344;
    const UINT64: u64 = 0x0102_0304_0506_0708;
    const BYTES_AT_0X18: [u8; 2] = [b'h', b'i'];
    const FLAG_0: u8 = 0;
    const FLAG_1: u8 = 1;
    const FLAGS: [u8; REGISTRATION_RESPONSE_FLAG_COUNT] = [FLAG_0, FLAG_1, FLAG_0, FLAG_1];
    const OUT_OF_RANGE_BYTE: u8 = 2;
    const LONG_FILL: u8 = 0x7E;

    const EXPECTED: [u8; 20] = [
        0x11, 0x22, 0x33, 0x44, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x02, b'h', b'i',
        0x00, 0x00, 0x01, 0x00, 0x01,
    ];

    fn sample() -> RegistrationResponseMsg {
        RegistrationResponseMsg {
            uint32_at_0x8: UINT32_AT_0X8,
            uint64: UINT64,
            bytes_at_0x18: BYTES_AT_0X18.to_vec(),
            bytes_at_0x38: Vec::new(),
            bytes_at_0x58: FLAGS,
        }
    }

    #[test]
    fn response_body_matches_the_write_order() {
        let message = sample();
        let encoded = message.encode().expect("encode");
        assert_eq!(encoded, EXPECTED);
        let decoded = RegistrationResponseMsg::decode(&encoded).expect("decode");
        assert_eq!(decoded, message);
    }

    #[test]
    fn response_strings_at_the_omit_limit_are_refused() {
        let mut message = sample();
        message.bytes_at_0x38 = vec![LONG_FILL; super::super::wire::STRING_LENGTH_OMIT];
        assert_eq!(
            message.encode(),
            Err(RegistrationError::StringTooLong {
                length: super::super::wire::STRING_LENGTH_OMIT
            })
        );
    }

    #[test]
    fn a_flag_of_two_or_more_is_rejected() {
        let mut message = sample();
        message.bytes_at_0x58[0] = OUT_OF_RANGE_BYTE;
        assert_eq!(
            message.encode().expect_err("encode"),
            RegistrationError::ZeroOrOneOutOfRange {
                byte: OUT_OF_RANGE_BYTE
            }
        );
        let mut patched = EXPECTED;
        let last = patched.len() - 1;
        patched[last] = OUT_OF_RANGE_BYTE;
        assert_eq!(
            RegistrationResponseMsg::decode(&patched).expect_err("decode"),
            RegistrationError::ZeroOrOneOutOfRange {
                byte: OUT_OF_RANGE_BYTE
            }
        );
        let mut trailing = EXPECTED.to_vec();
        trailing.push(FLAG_0);
        assert_eq!(
            RegistrationResponseMsg::decode(&trailing).expect_err("trailing"),
            RegistrationError::TrailingBytes { remaining: 1 }
        );
        assert_eq!(
            RegistrationResponseMsg::decode(&EXPECTED[..EXPECTED.len() - 1]).expect_err("short"),
            RegistrationError::Truncated {
                needed: super::super::wire::U8_OCTETS,
                available: 0,
            }
        );
    }

    /// Byte changes applied to an encoded sample.
    const MAX_MUTATIONS: usize = 8;

    proptest::proptest! {
        #[test]
        fn mutated_bodies_never_panic_and_round_trip(
            mutations in proptest::collection::vec(
                (proptest::prelude::any::<usize>(), proptest::prelude::any::<u8>()),
                0..=MAX_MUTATIONS,
            ),
            cut in proptest::option::of(proptest::prelude::any::<usize>()),
        ) {
            let mut bytes = sample().encode()?;
            for (position, value) in mutations {
                let index = position % bytes.len();
                bytes[index] = value;
            }
            if let Some(cut) = cut {
                bytes.truncate(cut % (bytes.len() + 1));
            }
            if let Ok(message) = RegistrationResponseMsg::decode(&bytes) {
                let again = RegistrationResponseMsg::decode(&message.encode()?)?;
                proptest::prop_assert_eq!(again, message);
            }
        }
    }
}
