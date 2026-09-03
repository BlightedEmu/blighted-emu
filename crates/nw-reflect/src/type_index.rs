// SPDX-License-Identifier: AGPL-3.0-or-later
//! Type-index prefix from `GetTypeIdForIndex`.
//!
//! The first byte selects the width. This is not LEB128.
//!
//! | First byte | Extra bytes | Value |
//! |---|---|---|
//! | `0x00`–`0x7F` | 0 | the byte |
//! | `0x80`–`0xBF` | 1 | low 6 bits, then the extra byte shifted left 6 |
//! | `0xC0`–`0xDF` | 2 | low 5 bits, then the extra bytes shifted left 5 |
//! | `0xE0`–`0xEF` | 3 | low 4 bits, then the extra bytes shifted left 4 |
//! | `0xF0`–`0xFF` | 4 | low 3 bits, then the extra bytes shifted left 3 |
//!
//! The first extra byte is the least significant extra byte.
//! Each following extra byte is shifted left 8 more bits.
//! On a five-byte value, only the low 3 bits of the first byte are part of the value.
//!
//! Index [`INLINE_UUID_INDEX`] means the next [`INLINE_UUID_LEN`] bytes are a UUID.
//! An index greater than 0 is a file slot. This module does not store the UUID table.

use thiserror::Error;

/// Wire index that means the next [`INLINE_UUID_LEN`] bytes are a UUID.
pub const INLINE_UUID_INDEX: u64 = 0;

/// Byte length of the UUID that follows index 0.
pub const INLINE_UUID_LEN: usize = 16;

/// File slot of `RegistrationRequestMsg`.
pub const REGISTRATION_REQUEST_MSG_TYPE_INDEX: u64 = 1;

/// File slot of `RegistrationRequestV2Msg`.
pub const REGISTRATION_REQUEST_V2_MSG_TYPE_INDEX: u64 = 2;

/// File slot of `RegistrationResponseMsg`.
pub const REGISTRATION_RESPONSE_MSG_TYPE_INDEX: u64 = 3;

/// File slot of `RegistrationRequestV3Msg`.
pub const REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX: u64 = 19;

/// File slot of `ProcessDeferredMovementRequestsMsg`.
pub const PROCESS_DEFERRED_MOVEMENT_REQUESTS_MSG_TYPE_INDEX: u64 = 723;

/// File slot of `MoveActorMsg`.
pub const MOVE_ACTOR_MSG_TYPE_INDEX: u64 = 710;

/// File slot of `StartMovementCommunicationMsg`.
pub const START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX: u64 = 726;

/// File slot of `REPClient::PingMsg`: an 8-byte body that the server sends about every second
/// and the client sends back (carrier captures note section 8).
pub const PING_MSG_TYPE_INDEX: u64 = 349;

/// File slot of `MoveActorToHubMsg`. The note shows a registrar only. This crate does not encode it.
pub const MOVE_ACTOR_TO_HUB_MSG_TYPE_INDEX: u64 = 718;

/// File slot of `AckMovementMsg`. The note names a reader and does not show a byte list.
pub const ACK_MOVEMENT_MSG_TYPE_INDEX: u64 = 739;

/// File slot of `AddActorMessage`.
///
/// Section 5 shows two big-endian `uint32` values (`FUN_146167960` calls `WS2_32` ordinal 14).
/// When the 0-or-1 byte is 1, virtual calls at offsets `0x10` and `0x28` have no byte list.
/// This crate does not encode the message.
pub const ADD_ACTOR_MESSAGE_TYPE_INDEX: u64 = 121;

/// File slot of `ReceiveActorMsg`. Section 5 leaves helpers unopened. This crate does not encode it.
pub const RECEIVE_ACTOR_MSG_TYPE_INDEX: u64 = 921;

/// File slot of `DeleteActorMessage`. The reader was not opened. This crate does not encode it.
pub const DELETE_ACTOR_MESSAGE_TYPE_INDEX: u64 = 919;

/// File slot of `TimeSynchMsg`. The reader was not opened. This crate does not encode it.
pub const TIME_SYNCH_MSG_TYPE_INDEX: u64 = 335;

/// File slot of `PingRequestMsg`. The reader was not opened. This crate does not encode it.
pub const PING_REQUEST_MSG_TYPE_INDEX: u64 = 209;

const ONE_BYTE_LEN: usize = 1;
const ONE_BYTE_MAX: u8 = 0x7F;
const TWO_BYTE_FIRST: u8 = 0x80;
const THREE_BYTE_FIRST: u8 = 0xC0;
const FOUR_BYTE_FIRST: u8 = 0xE0;
const FIVE_BYTE_FIRST: u8 = 0xF0;

const TWO_BYTE_SHIFT: u32 = 6;
const THREE_BYTE_SHIFT: u32 = 5;
const FOUR_BYTE_SHIFT: u32 = 4;
const FIVE_BYTE_SHIFT: u32 = 3;
const EXTRA_BYTE_BITS: u32 = 8;
const BYTE_MASK: u64 = 0xFF;

const TWO_BYTE_EXTRA: usize = 1;
const THREE_BYTE_EXTRA: usize = 2;
const FOUR_BYTE_EXTRA: usize = 3;
const FIVE_BYTE_EXTRA: usize = 4;

const fn low_mask(shift: u32) -> u8 {
    (1_u8 << shift) - 1
}

const fn form_max(shift: u32, extra_len: u32) -> u64 {
    (1_u64 << (shift + extra_len * EXTRA_BYTE_BITS)) - 1
}

const TWO_BYTE_MAX: u64 = form_max(TWO_BYTE_SHIFT, TWO_BYTE_EXTRA as u32);
const THREE_BYTE_MAX: u64 = form_max(THREE_BYTE_SHIFT, THREE_BYTE_EXTRA as u32);
const FOUR_BYTE_MAX: u64 = form_max(FOUR_BYTE_SHIFT, FOUR_BYTE_EXTRA as u32);
const FIVE_BYTE_MAX: u64 = form_max(FIVE_BYTE_SHIFT, FIVE_BYTE_EXTRA as u32);

struct PrefixForm {
    first: u8,
    shift: u32,
    extra_len: usize,
    max_value: u64,
}

const TWO_BYTE_FORM: PrefixForm = PrefixForm {
    first: TWO_BYTE_FIRST,
    shift: TWO_BYTE_SHIFT,
    extra_len: TWO_BYTE_EXTRA,
    max_value: TWO_BYTE_MAX,
};
const THREE_BYTE_FORM: PrefixForm = PrefixForm {
    first: THREE_BYTE_FIRST,
    shift: THREE_BYTE_SHIFT,
    extra_len: THREE_BYTE_EXTRA,
    max_value: THREE_BYTE_MAX,
};
const FOUR_BYTE_FORM: PrefixForm = PrefixForm {
    first: FOUR_BYTE_FIRST,
    shift: FOUR_BYTE_SHIFT,
    extra_len: FOUR_BYTE_EXTRA,
    max_value: FOUR_BYTE_MAX,
};
const FIVE_BYTE_FORM: PrefixForm = PrefixForm {
    first: FIVE_BYTE_FIRST,
    shift: FIVE_BYTE_SHIFT,
    extra_len: FIVE_BYTE_EXTRA,
    max_value: FIVE_BYTE_MAX,
};

/// Shortest multi-byte form first.
const MULTI_BYTE_FORMS: [PrefixForm; 4] = [
    TWO_BYTE_FORM,
    THREE_BYTE_FORM,
    FOUR_BYTE_FORM,
    FIVE_BYTE_FORM,
];

/// Failure from the type-index prefix codec.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TypeIndexError {
    /// The buffer ended before the prefix, or before the inline UUID, was complete.
    #[error("truncated type-index buffer: need {needed} bytes, have {available}")]
    Truncated {
        /// Bytes required to finish this read.
        needed: usize,
        /// Bytes available in the buffer.
        available: usize,
    },
    /// `index` is larger than the five-byte form can store.
    #[error("type index {index} does not fit in the prefix codec (max {max})")]
    IndexOutOfRange {
        /// Index that was not encoded.
        index: u64,
        /// Largest index the codec can store.
        max: u64,
    },
    /// Index 0 is the inline-UUID form, so it is not a file slot.
    #[error("index 0 is an inline UUID, not a file slot")]
    IndexZeroIsInlineUuid,
}

/// One decoded type-index prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeIndexPrefix {
    /// Decoded index.
    pub index: u64,
    /// Bytes consumed from the front of the buffer.
    pub len: usize,
}

/// A type id after the prefix rule for index 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeId {
    /// Index 0. `uuid` is the next 16 bytes. It is not looked up in the file.
    InlineUuid([u8; INLINE_UUID_LEN]),
    /// Index greater than 0. This is a slot in `typeindex.json`.
    FileSlot(u64),
}

/// Decodes one type-index prefix from the front of `input`.
///
/// # Errors
///
/// Returns [`TypeIndexError::Truncated`] when `input` does not hold the extra bytes
/// selected by the first byte.
pub fn decode_type_index_prefix(input: &[u8]) -> Result<TypeIndexPrefix, TypeIndexError> {
    let Some(&first) = input.first() else {
        return Err(TypeIndexError::Truncated {
            needed: ONE_BYTE_LEN,
            available: 0,
        });
    };
    if first <= ONE_BYTE_MAX {
        return Ok(TypeIndexPrefix {
            index: u64::from(first),
            len: ONE_BYTE_LEN,
        });
    }
    read_form(input, form_for_first_byte(first))
}

/// Encodes `index` in the shortest prefix form.
///
/// # Errors
///
/// Returns [`TypeIndexError::IndexOutOfRange`] when `index` does not fit in five bytes.
pub fn encode_type_index_prefix(index: u64) -> Result<Vec<u8>, TypeIndexError> {
    if index <= u64::from(ONE_BYTE_MAX) {
        // The index is at most ONE_BYTE_MAX here, so it fits in one byte.
        return Ok(vec![index as u8]);
    }
    let form = MULTI_BYTE_FORMS
        .iter()
        .find(|form| index <= form.max_value)
        .ok_or(TypeIndexError::IndexOutOfRange {
            index,
            max: FIVE_BYTE_MAX,
        })?;
    Ok(write_form(index, form))
}

/// Decodes a type id: an inline UUID when the index is 0, otherwise a file slot.
///
/// # Errors
///
/// Returns [`TypeIndexError::Truncated`] when the prefix or the inline UUID is cut short.
pub fn decode_type_id(input: &[u8]) -> Result<(TypeId, usize), TypeIndexError> {
    let prefix = decode_type_index_prefix(input)?;
    if prefix.index != INLINE_UUID_INDEX {
        return Ok((TypeId::FileSlot(prefix.index), prefix.len));
    }
    let needed = prefix.len + INLINE_UUID_LEN;
    if input.len() < needed {
        return Err(TypeIndexError::Truncated {
            needed,
            available: input.len(),
        });
    }
    let mut uuid = [0_u8; INLINE_UUID_LEN];
    uuid.copy_from_slice(&input[prefix.len..needed]);
    Ok((TypeId::InlineUuid(uuid), needed))
}

/// Encodes a type id. A file slot of 0 is rejected because that index is the inline UUID.
///
/// # Errors
///
/// Returns [`TypeIndexError::IndexZeroIsInlineUuid`] for [`TypeId::FileSlot`]`(0)`.
/// Returns [`TypeIndexError::IndexOutOfRange`] when a file slot does not fit in the prefix.
pub fn encode_type_id(type_id: &TypeId) -> Result<Vec<u8>, TypeIndexError> {
    match type_id {
        TypeId::InlineUuid(uuid) => {
            let mut out = encode_type_index_prefix(INLINE_UUID_INDEX)?;
            out.extend(uuid);
            Ok(out)
        }
        TypeId::FileSlot(index) if *index == INLINE_UUID_INDEX => {
            Err(TypeIndexError::IndexZeroIsInlineUuid)
        }
        TypeId::FileSlot(index) => encode_type_index_prefix(*index),
    }
}

fn form_for_first_byte(first: u8) -> &'static PrefixForm {
    if first >= FIVE_BYTE_FIRST {
        &FIVE_BYTE_FORM
    } else if first >= FOUR_BYTE_FIRST {
        &FOUR_BYTE_FORM
    } else if first >= THREE_BYTE_FIRST {
        &THREE_BYTE_FORM
    } else {
        &TWO_BYTE_FORM
    }
}

fn read_form(input: &[u8], form: &PrefixForm) -> Result<TypeIndexPrefix, TypeIndexError> {
    let needed = ONE_BYTE_LEN + form.extra_len;
    if input.len() < needed {
        return Err(TypeIndexError::Truncated {
            needed,
            available: input.len(),
        });
    }
    let mask = u64::from(low_mask(form.shift));
    let mut index = u64::from(input[0]) & mask;
    let mut shift = form.shift;
    for &byte in &input[ONE_BYTE_LEN..needed] {
        index |= u64::from(byte) << shift;
        shift += EXTRA_BYTE_BITS;
    }
    Ok(TypeIndexPrefix { index, len: needed })
}

fn write_form(index: u64, form: &PrefixForm) -> Vec<u8> {
    // The mask keeps fewer than 8 bits.
    let low = (index & u64::from(low_mask(form.shift))) as u8;
    let mut out = Vec::with_capacity(ONE_BYTE_LEN + form.extra_len);
    out.push(form.first | low);
    let mut rest = index >> form.shift;
    for _ in 0..form.extra_len {
        // BYTE_MASK keeps 8 bits.
        out.push((rest & BYTE_MASK) as u8);
        rest >>= EXTRA_BYTE_BITS;
    }
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn ping_is_the_slot_the_captures_show() {
        // Carrier captures note sections 6 and 8: `9d 05` on the wire, `REPClient::PingMsg`.
        assert_eq!(PING_MSG_TYPE_INDEX, 349);
        assert_eq!(
            encode_type_index_prefix(PING_MSG_TYPE_INDEX),
            Ok(vec![0x9d, 0x05])
        );
    }

    const KNOWN_FILE_SLOTS: [u64; 15] = [
        REGISTRATION_REQUEST_MSG_TYPE_INDEX,
        REGISTRATION_REQUEST_V2_MSG_TYPE_INDEX,
        REGISTRATION_RESPONSE_MSG_TYPE_INDEX,
        REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX,
        ADD_ACTOR_MESSAGE_TYPE_INDEX,
        PING_REQUEST_MSG_TYPE_INDEX,
        TIME_SYNCH_MSG_TYPE_INDEX,
        MOVE_ACTOR_MSG_TYPE_INDEX,
        MOVE_ACTOR_TO_HUB_MSG_TYPE_INDEX,
        PROCESS_DEFERRED_MOVEMENT_REQUESTS_MSG_TYPE_INDEX,
        START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX,
        ACK_MOVEMENT_MSG_TYPE_INDEX,
        DELETE_ACTOR_MESSAGE_TYPE_INDEX,
        RECEIVE_ACTOR_MSG_TYPE_INDEX,
        PING_MSG_TYPE_INDEX,
    ];

    /// Shortest encoding of index 0.
    const ENCODED_ZERO: [u8; 1] = [0x00];
    /// Shortest encoding of index 0x7F.
    const ENCODED_0X7F: [u8; 1] = [0x7F];
    /// Index 0x80. Low 6 bits are 0. The extra byte is 0x80 >> 6 = 2.
    const ENCODED_0X80: [u8; 2] = [0x80, 0x02];
    /// Index 0xC0. Low 6 bits are 0. The extra byte is 0xC0 >> 6 = 3.
    const ENCODED_0XC0: [u8; 2] = [0x80, 0x03];
    /// Index 0xE0. Low 6 bits are 0x20. The extra byte is 3. The first byte is 0xA0.
    const ENCODED_0XE0: [u8; 2] = [0xA0, 0x03];
    /// Index 0xF0. Low 6 bits are 0x30. The extra byte is 3. The first byte is 0xB0.
    const ENCODED_0XF0: [u8; 2] = [0xB0, 0x03];

    /// `0xC0 0x01 0x02`: low 5 bits are 0, then `0x01 << 5` and `0x02 << 13`.
    const VALUE_0XC0_01_02: u64 = 0x4020;
    /// `0xE0 0x01 0x00 0x00`: `0x01 << 4`.
    const VALUE_0XE0_01: u64 = 0x10;
    /// `0xF0 0x01 0x00 0x00 0x00`: `0x01 << 3`.
    const VALUE_0XF0_01: u64 = 0x08;

    /// Shortest encoding of `MoveActorMsg` (710 = 11 * 64 + 6).
    const ENCODED_MOVE_ACTOR: [u8; 2] = [0x86, 0x0B];
    /// Shortest encoding of `PingMsg` (349 = 5 * 64 + 29), as the captures show it.
    const ENCODED_PING: [u8; 2] = [0x9D, 0x05];
    /// Five-byte maximum. Low 3 bits are 7. Four extra bytes are `0xFF`.
    const ENCODED_FIVE_BYTE_MAX: [u8; 5] = [0xF7, 0xFF, 0xFF, 0xFF, 0xFF];

    const SAMPLE_UUID: [u8; INLINE_UUID_LEN] = [
        0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD,
        0xEF,
    ];
    const UUID_TAIL_MARKER: u8 = 0xA5;

    fn assert_shortest(index: u64, expected: &[u8]) {
        let encoded = encode_type_index_prefix(index).expect("index fits");
        assert_eq!(encoded, expected);
        let decoded = decode_type_index_prefix(&encoded).expect("encoded prefix");
        assert_eq!(decoded.index, index);
        assert_eq!(decoded.len, expected.len());
    }

    #[test]
    fn shortest_form_round_trips_class_boundary_values() {
        assert_shortest(u64::from(ENCODED_ZERO[0]), &ENCODED_ZERO);
        assert_shortest(u64::from(ENCODED_0X7F[0]), &ENCODED_0X7F);
        assert_shortest(0x80, &ENCODED_0X80);
        assert_shortest(0xC0, &ENCODED_0XC0);
        assert_shortest(0xE0, &ENCODED_0XE0);
        assert_shortest(0xF0, &ENCODED_0XF0);
        assert_shortest(FIVE_BYTE_MAX, &ENCODED_FIVE_BYTE_MAX);
    }

    #[test]
    fn decodes_first_byte_class_boundaries() {
        let zero = decode_type_index_prefix(&ENCODED_ZERO).expect("zero");
        assert_eq!(zero, TypeIndexPrefix { index: 0, len: 1 });

        let one_byte_max = decode_type_index_prefix(&ENCODED_0X7F).expect("0x7F");
        assert_eq!(
            one_byte_max,
            TypeIndexPrefix {
                index: u64::from(ONE_BYTE_MAX),
                len: 1
            }
        );

        let two = decode_type_index_prefix(&[TWO_BYTE_FIRST, 0x00]).expect("0x80");
        assert_eq!(two, TypeIndexPrefix { index: 0, len: 2 });
        let two_value = decode_type_index_prefix(&ENCODED_0X80).expect("0x80 value");
        assert_eq!(
            two_value,
            TypeIndexPrefix {
                index: 0x80,
                len: 2
            }
        );

        let three = decode_type_index_prefix(&[THREE_BYTE_FIRST, 0x00, 0x00]).expect("0xC0");
        assert_eq!(three, TypeIndexPrefix { index: 0, len: 3 });
        let three_value =
            decode_type_index_prefix(&[THREE_BYTE_FIRST, 0x01, 0x02]).expect("0xC0 value");
        assert_eq!(
            three_value,
            TypeIndexPrefix {
                index: VALUE_0XC0_01_02,
                len: 3
            }
        );

        let four = decode_type_index_prefix(&[FOUR_BYTE_FIRST, 0x00, 0x00, 0x00]).expect("0xE0");
        assert_eq!(four, TypeIndexPrefix { index: 0, len: 4 });
        let four_value =
            decode_type_index_prefix(&[FOUR_BYTE_FIRST, 0x01, 0x00, 0x00]).expect("0xE0 value");
        assert_eq!(
            four_value,
            TypeIndexPrefix {
                index: VALUE_0XE0_01,
                len: 4
            }
        );

        let five =
            decode_type_index_prefix(&[FIVE_BYTE_FIRST, 0x00, 0x00, 0x00, 0x00]).expect("0xF0");
        assert_eq!(five, TypeIndexPrefix { index: 0, len: 5 });
        let five_value = decode_type_index_prefix(&[FIVE_BYTE_FIRST, 0x01, 0x00, 0x00, 0x00])
            .expect("0xF0 value");
        assert_eq!(
            five_value,
            TypeIndexPrefix {
                index: VALUE_0XF0_01,
                len: 5
            }
        );
    }

    #[test]
    fn five_byte_first_byte_uses_low_3_bits() {
        let from_tag = decode_type_index_prefix(&ENCODED_FIVE_BYTE_MAX).expect("canonical");
        let from_high_tag =
            decode_type_index_prefix(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF]).expect("0xFF tag");
        assert_eq!(from_tag, from_high_tag);
        assert_eq!(from_tag.index, FIVE_BYTE_MAX);
    }

    #[test]
    fn truncated_prefix_buffer_is_an_error() {
        let cases: [&[u8]; 5] = [
            &[],
            &[TWO_BYTE_FIRST],
            &[THREE_BYTE_FIRST, 0x00],
            &[FOUR_BYTE_FIRST, 0x00, 0x00],
            &[FIVE_BYTE_FIRST, 0x00, 0x00, 0x00],
        ];
        let expected_needed = [
            ONE_BYTE_LEN,
            ONE_BYTE_LEN + TWO_BYTE_EXTRA,
            ONE_BYTE_LEN + THREE_BYTE_EXTRA,
            ONE_BYTE_LEN + FOUR_BYTE_EXTRA,
            ONE_BYTE_LEN + FIVE_BYTE_EXTRA,
        ];
        for (input, needed) in cases.into_iter().zip(expected_needed) {
            let error = decode_type_index_prefix(input).expect_err("truncated prefix");
            assert_eq!(
                error,
                TypeIndexError::Truncated {
                    needed,
                    available: input.len(),
                }
            );
        }
    }

    #[test]
    fn index_zero_reads_the_next_16_bytes_as_a_uuid() {
        let mut input = Vec::with_capacity(ONE_BYTE_LEN + INLINE_UUID_LEN + 1);
        input.extend(ENCODED_ZERO);
        input.extend(SAMPLE_UUID);
        input.push(UUID_TAIL_MARKER);

        let (type_id, len) = decode_type_id(&input).expect("inline uuid");
        assert_eq!(type_id, TypeId::InlineUuid(SAMPLE_UUID));
        assert_eq!(len, ONE_BYTE_LEN + INLINE_UUID_LEN);
        assert_eq!(input[len], UUID_TAIL_MARKER);

        let encoded = encode_type_id(&TypeId::InlineUuid(SAMPLE_UUID)).expect("encode uuid");
        assert_eq!(encoded, &input[..len]);
    }

    #[test]
    fn truncated_inline_uuid_is_an_error() {
        let mut input = Vec::from(ENCODED_ZERO);
        input.extend([0_u8; INLINE_UUID_LEN - 1]);
        let error = decode_type_id(&input).expect_err("short uuid");
        assert_eq!(
            error,
            TypeIndexError::Truncated {
                needed: ONE_BYTE_LEN + INLINE_UUID_LEN,
                available: input.len(),
            }
        );
    }

    #[test]
    fn index_above_zero_is_a_file_slot() {
        let mut input = Vec::from(ENCODED_MOVE_ACTOR);
        input.extend(SAMPLE_UUID);
        let (type_id, len) = decode_type_id(&input).expect("file slot");
        assert_eq!(type_id, TypeId::FileSlot(MOVE_ACTOR_MSG_TYPE_INDEX));
        assert_eq!(len, ENCODED_MOVE_ACTOR.len());
        assert_eq!(&input[len..], SAMPLE_UUID);

        let encoded =
            encode_type_id(&TypeId::FileSlot(MOVE_ACTOR_MSG_TYPE_INDEX)).expect("encode file slot");
        assert_eq!(encoded, ENCODED_MOVE_ACTOR);

        let one_byte =
            decode_type_id(&[u8::try_from(REGISTRATION_REQUEST_MSG_TYPE_INDEX)
                .expect("index 1 fits in one byte")])
            .expect("registration request");
        assert_eq!(
            one_byte,
            (TypeId::FileSlot(REGISTRATION_REQUEST_MSG_TYPE_INDEX), 1)
        );
    }

    #[test]
    fn known_type_indexes_round_trip_as_file_slots() {
        assert_eq!(
            encode_type_index_prefix(MOVE_ACTOR_MSG_TYPE_INDEX).expect("move actor"),
            ENCODED_MOVE_ACTOR
        );
        assert_eq!(
            encode_type_index_prefix(PING_MSG_TYPE_INDEX).expect("ping"),
            ENCODED_PING
        );
        for index in KNOWN_FILE_SLOTS {
            assert_ne!(index, INLINE_UUID_INDEX);
            let encoded = encode_type_id(&TypeId::FileSlot(index)).expect("encode slot");
            let (decoded, len) = decode_type_id(&encoded).expect("decode slot");
            assert_eq!(decoded, TypeId::FileSlot(index));
            assert_eq!(len, encoded.len());
        }
    }

    #[test]
    fn file_slot_zero_is_rejected() {
        let error = encode_type_id(&TypeId::FileSlot(INLINE_UUID_INDEX)).expect_err("slot 0");
        assert_eq!(error, TypeIndexError::IndexZeroIsInlineUuid);
    }

    #[test]
    fn index_above_five_byte_max_is_rejected() {
        let index = FIVE_BYTE_MAX + 1;
        let error = encode_type_index_prefix(index).expect_err("too large");
        assert_eq!(
            error,
            TypeIndexError::IndexOutOfRange {
                index,
                max: FIVE_BYTE_MAX,
            }
        );
    }

    /// Longest input the decoder can look at: one first byte and four extra bytes.
    const LONGEST_PREFIX: usize = 5;

    proptest! {
        #[test]
        fn every_index_round_trips(index in 0..=FIVE_BYTE_MAX) {
            let encoded = encode_type_index_prefix(index)?;
            let decoded = decode_type_index_prefix(&encoded)?;
            prop_assert_eq!(decoded.index, index);
            prop_assert_eq!(decoded.len, encoded.len());
        }

        #[test]
        fn decoding_any_bytes_never_panics(
            bytes in proptest::collection::vec(any::<u8>(), 0..=LONGEST_PREFIX)
        ) {
            if let Ok(prefix) = decode_type_index_prefix(&bytes) {
                prop_assert!(prefix.len <= bytes.len());
            }
        }
    }
}
