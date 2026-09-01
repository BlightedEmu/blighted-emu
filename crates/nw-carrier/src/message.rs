// SPDX-License-Identifier: AGPL-3.0-or-later
//! Message bodies whose field lists are complete in the protocol note, section 5.
//!
//! The type-index prefix codec is not implemented here. The caller passes a `u32` type index.
//! Registration bodies are in `nw-protocol`. This crate does not choose V1, V2, or V3.
//!
//! These section 5 bodies stay unimplemented because the byte list is still incomplete:
//! `MoveActorToHubMsg` (registrar only), `AckMovementMsg` (reader named, no byte list),
//! `AddActorMessage`, `ReceiveActorMsg`, `DeleteActorMessage`, `TimeSynchMsg`,
//! `PingRequestMsg`, `PingMsg`, and `ProcessDeferredMovementRequestsMsg`.
//! `AddActorMessage` does show two big-endian `uint32` values: `FUN_146167960` calls
//! `WS2_32` ordinal 14. When its 0-or-1 byte is 1, `FUN_142d67640` reads a type id and then
//! virtual calls at offsets `0x10` and `0x28`. Those calls have no byte list.
//! `ReceiveActorMsg` still names unopened helpers, including `FUN_146abe4b0`, `FUN_146ae35e0`,
//! `FUN_146a923f0`, `FUN_1415bc630`, `FUN_146a6b260`, `FUN_1461ac530`, `FUN_14087a820`,
//! and `FUN_146a6b5b0`.
//! Spawn and Player names in section 5 are state logs or RTTI. This crate has no spawn message.

use crate::error::CarrierError;

/// Length of the raw byte field on `MoveActorMsg` and `StartMovementCommunicationMsg`.
pub const RAW_OCTETS: usize = 16;

/// File index of `MoveActorMsg`. Protocol note section 5.
pub const MOVE_ACTOR_MSG_TYPE_INDEX: u32 = 710;

/// UUID string that the note places immediately after `MoveActorMsg`.
pub const MOVE_ACTOR_MSG_UUID: &str = "5570cfb7-0c5c-475e-bb19-d8fbc3fd8d32";

/// File index of `StartMovementCommunicationMsg`. Protocol note section 5.
pub const START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX: u32 = 726;

/// UUID string that the note places immediately after `StartMovementCommunicationMsg`.
pub const START_MOVEMENT_COMMUNICATION_MSG_UUID: &str = "767fc9bf-1c8d-417f-ad19-c51e363d4df0";

/// Object offset of the 16 raw bytes written by `FUN_146a546d0`. Protocol note section 5, object +0x8.
pub const START_MOVEMENT_RAW_OBJECT_OFFSET: usize = 0x8;

/// Octet count of the big-endian `uint32` on `StartMovementCommunicationMsg`.
pub const START_MOVEMENT_INTEGER_OCTETS: usize = 4;

/// Object offset of that `uint32`. Protocol note section 5, object +0x18.
pub const START_MOVEMENT_INTEGER_OBJECT_OFFSET: usize =
    START_MOVEMENT_RAW_OBJECT_OFFSET + RAW_OCTETS;

/// Wire length of `StartMovementCommunicationMsg`: 16 raw bytes, then one big-endian `uint32`.
pub const START_MOVEMENT_BODY_OCTETS: usize = RAW_OCTETS + START_MOVEMENT_INTEGER_OCTETS;

/// Body bytes plus the caller-supplied type index.
///
/// `bytes` does not include a type-index prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageBody {
    /// Type index the caller supplied. This crate checks it and does not encode it.
    pub type_index: u32,
    /// Observed message body.
    pub bytes: Vec<u8>,
}

/// `MoveActorMsg` body. The reader `FUN_146a54050` copies 16 raw bytes and then stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveActorMsg {
    /// Sixteen raw bytes. The note does not name this field.
    pub raw: [u8; RAW_OCTETS],
}

impl MoveActorMsg {
    /// Encode the 16 raw bytes. The type index is checked and is not written.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::UnexpectedTypeIndex`] when `type_index` is not [`MOVE_ACTOR_MSG_TYPE_INDEX`].
    pub fn encode_body(&self, type_index: u32) -> Result<MessageBody, CarrierError> {
        require_type_index(MOVE_ACTOR_MSG_TYPE_INDEX, type_index)?;
        Ok(MessageBody {
            type_index,
            bytes: self.raw.to_vec(),
        })
    }

    /// Decode 16 raw bytes. The type index is not read from `bytes`.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::UnexpectedTypeIndex`] or [`CarrierError::UnexpectedBodyLength`].
    pub fn decode_body(type_index: u32, bytes: &[u8]) -> Result<Self, CarrierError> {
        require_type_index(MOVE_ACTOR_MSG_TYPE_INDEX, type_index)?;
        require_len(RAW_OCTETS, bytes.len())?;
        let mut raw = [0_u8; RAW_OCTETS];
        raw.copy_from_slice(bytes);
        Ok(Self { raw })
    }
}

/// `StartMovementCommunicationMsg` body.
///
/// Writer `FUN_146a546d0` writes 16 raw bytes from object +0x8, then a big-endian `uint32` from +0x18.
/// Reader `FUN_146a6b1b0` reads those same two fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartMovementCommunicationMsg {
    /// Sixteen raw bytes from object +0x8. The note does not name this field.
    pub raw: [u8; RAW_OCTETS],
    /// Big-endian `uint32` from object +0x18. The note does not name this field.
    pub integer: u32,
}

impl StartMovementCommunicationMsg {
    /// Encode 16 raw bytes and one big-endian `uint32`. The type index is checked and is not written.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::UnexpectedTypeIndex`] when `type_index` is not
    /// [`START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX`].
    pub fn encode_body(&self, type_index: u32) -> Result<MessageBody, CarrierError> {
        require_type_index(START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX, type_index)?;
        let mut bytes = Vec::with_capacity(START_MOVEMENT_BODY_OCTETS);
        bytes.extend_from_slice(&self.raw);
        bytes.extend_from_slice(&self.integer.to_be_bytes());
        Ok(MessageBody { type_index, bytes })
    }

    /// Decode 16 raw bytes and one big-endian `uint32`.
    ///
    /// # Errors
    ///
    /// Returns [`CarrierError::UnexpectedTypeIndex`] or [`CarrierError::UnexpectedBodyLength`].
    pub fn decode_body(type_index: u32, bytes: &[u8]) -> Result<Self, CarrierError> {
        require_type_index(START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX, type_index)?;
        require_len(START_MOVEMENT_BODY_OCTETS, bytes.len())?;
        let mut raw = [0_u8; RAW_OCTETS];
        raw.copy_from_slice(&bytes[..RAW_OCTETS]);
        let mut integer_bytes = [0_u8; START_MOVEMENT_INTEGER_OCTETS];
        integer_bytes.copy_from_slice(&bytes[RAW_OCTETS..START_MOVEMENT_BODY_OCTETS]);
        Ok(Self {
            raw,
            integer: u32::from_be_bytes(integer_bytes),
        })
    }
}

fn require_type_index(expected: u32, actual: u32) -> Result<(), CarrierError> {
    if actual == expected {
        Ok(())
    } else {
        Err(CarrierError::UnexpectedTypeIndex { expected, actual })
    }
}

fn require_len(expected: usize, actual: usize) -> Result<(), CarrierError> {
    if actual == expected {
        Ok(())
    } else {
        Err(CarrierError::UnexpectedBodyLength { expected, actual })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RAW: [u8; RAW_OCTETS] = [
        0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd,
        0xef,
    ];
    const SAMPLE_INTEGER: u32 = 0xA1B2_C3D4;
    const SAMPLE_INTEGER_BE: [u8; START_MOVEMENT_INTEGER_OCTETS] = [0xA1, 0xB2, 0xC3, 0xD4];
    const WRONG_MOVE_INDEX: u32 = MOVE_ACTOR_MSG_TYPE_INDEX + 1;
    const WRONG_START_INDEX: u32 = START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX + 1;
    const OBSERVED_MOVE_UUID: &str = "5570cfb7-0c5c-475e-bb19-d8fbc3fd8d32";
    const OBSERVED_START_UUID: &str = "767fc9bf-1c8d-417f-ad19-c51e363d4df0";

    #[test]
    fn move_actor_body_is_sixteen_raw_bytes() {
        assert_eq!(MOVE_ACTOR_MSG_UUID, OBSERVED_MOVE_UUID);
        let message = MoveActorMsg { raw: SAMPLE_RAW };
        let encoded = message
            .encode_body(MOVE_ACTOR_MSG_TYPE_INDEX)
            .expect("encode");
        assert_eq!(encoded.type_index, MOVE_ACTOR_MSG_TYPE_INDEX);
        assert_eq!(encoded.bytes, SAMPLE_RAW);
        assert_eq!(encoded.bytes.len(), RAW_OCTETS);
        let decoded =
            MoveActorMsg::decode_body(encoded.type_index, &encoded.bytes).expect("decode");
        assert_eq!(decoded, message);

        let wrong_index = message.encode_body(WRONG_MOVE_INDEX).expect_err("index");
        assert_eq!(
            wrong_index,
            CarrierError::UnexpectedTypeIndex {
                expected: MOVE_ACTOR_MSG_TYPE_INDEX,
                actual: WRONG_MOVE_INDEX,
            }
        );
        let short =
            MoveActorMsg::decode_body(MOVE_ACTOR_MSG_TYPE_INDEX, &SAMPLE_RAW[..RAW_OCTETS - 1])
                .expect_err("short");
        assert_eq!(
            short,
            CarrierError::UnexpectedBodyLength {
                expected: RAW_OCTETS,
                actual: RAW_OCTETS - 1,
            }
        );
    }

    #[test]
    fn start_movement_body_is_raw_bytes_plus_big_endian_u32() {
        assert_eq!(START_MOVEMENT_COMMUNICATION_MSG_UUID, OBSERVED_START_UUID);
        assert_eq!(
            START_MOVEMENT_INTEGER_OBJECT_OFFSET,
            START_MOVEMENT_RAW_OBJECT_OFFSET + RAW_OCTETS
        );
        let message = StartMovementCommunicationMsg {
            raw: SAMPLE_RAW,
            integer: SAMPLE_INTEGER,
        };
        let encoded = message
            .encode_body(START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX)
            .expect("encode");
        assert_eq!(
            encoded.type_index,
            START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX
        );
        assert_eq!(encoded.bytes.len(), START_MOVEMENT_BODY_OCTETS);
        assert_eq!(&encoded.bytes[..RAW_OCTETS], &SAMPLE_RAW);
        assert_eq!(&encoded.bytes[RAW_OCTETS..], &SAMPLE_INTEGER_BE);
        let decoded =
            StartMovementCommunicationMsg::decode_body(encoded.type_index, &encoded.bytes)
                .expect("decode");
        assert_eq!(decoded, message);

        let wrong_index = message.encode_body(WRONG_START_INDEX).expect_err("index");
        assert_eq!(
            wrong_index,
            CarrierError::UnexpectedTypeIndex {
                expected: START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX,
                actual: WRONG_START_INDEX,
            }
        );
        let long_len = START_MOVEMENT_BODY_OCTETS + 1;
        let mut long_bytes = encoded.bytes;
        long_bytes.push(0);
        let long = StartMovementCommunicationMsg::decode_body(
            START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX,
            &long_bytes,
        )
        .expect_err("long");
        assert_eq!(
            long,
            CarrierError::UnexpectedBodyLength {
                expected: START_MOVEMENT_BODY_OCTETS,
                actual: long_len,
            }
        );
    }
}
