// SPDX-License-Identifier: AGPL-3.0-or-later
//! GridMate carrier pieces that the protocol note already shows.
//!
//! The note is `docs/re/2026-08-29-client-protocol-login-gridmate.md`.
//! This crate stores the four send sequence counters and the four reliable sequence counters.
//! It copies a caller buffer into the `0x600` byte send slot. That copy writes no header.
//! It encodes `MoveActorMsg` and `StartMovementCommunicationMsg`.
//! LZ4 wrapper slots are recorded here. Those wrappers have no direct call, so this crate
//! does not compress a datagram.
//! It records the cipher list `ECDHE-RSA-AES256-GCM-SHA384`.
//! A UDP send returns [`DATAGRAM_HEADER_UNKNOWN`] because the datagram header was not found.

pub mod error;
pub mod lz4;
pub mod message;
pub mod send_slot;
pub mod sequence;
pub mod transport;

pub use error::{CarrierError, DATAGRAM_HEADER_UNKNOWN};
pub use lz4::{
    LZ4_COMPRESS_BOUND_SLOT_ADDRESS, LZ4_COMPRESS_BOUND_WRAPPER, LZ4_CONFIG_USE, LZ4_FACTORY,
    LZ4_FUNCTION_AT_PLUS_0X28, LZ4_FUNCTION_AT_PLUS_0X30, LZ4_PLUS_0X28_SLOT_ADDRESS,
    LZ4_PLUS_0X30_SLOT_ADDRESS, LZ4_SLOT_COMPRESS_BOUND, LZ4_SLOT_PLUS_0X28, LZ4_SLOT_PLUS_0X30,
    LZ4_VTABLE, LZ4_WRAPPERS_HAVE_DIRECT_CALL,
};
pub use message::{
    MOVE_ACTOR_MSG_TYPE_INDEX, MOVE_ACTOR_MSG_UUID, MessageBody, MoveActorMsg, RAW_OCTETS,
    START_MOVEMENT_BODY_OCTETS, START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX,
    START_MOVEMENT_COMMUNICATION_MSG_UUID, START_MOVEMENT_INTEGER_OBJECT_OFFSET,
    START_MOVEMENT_INTEGER_OCTETS, START_MOVEMENT_RAW_OBJECT_OFFSET, StartMovementCommunicationMsg,
};
pub use send_slot::{
    SEND_FILL_BYTES_OFFSET, SEND_FILL_CONDVAR_OFFSET, SEND_FILL_COUNTER_OFFSET, SEND_FILL_FUNCTION,
    SEND_FILL_LENGTH_OFFSET, SEND_FILL_SECOND_SLOT_ADDRESS, SEND_FILL_SECOND_VTABLE,
    SEND_FILL_SIDE_OFFSET, SEND_FILL_SLOT_ADDRESS, SEND_FILL_SLOT_INDEX_OFFSET,
    SEND_FILL_SLOT_OFFSET, SEND_FILL_VTABLE, SEND_SLOT_CAPACITY, SEND_SLOT_SIDE_OCTETS,
    SEND_SLOT_TOO_LONG, SendSlot, send_slot_bytes_offset, send_slot_side_offset,
};
pub use sequence::{
    ConnectionSequences, SEND_RELIABLE_SEQ_OFFSET_0, SEND_RELIABLE_SEQ_OFFSET_1,
    SEND_RELIABLE_SEQ_OFFSET_2, SEND_RELIABLE_SEQ_OFFSET_3, SEND_SEQ_OFFSET_0, SEND_SEQ_OFFSET_1,
    SEND_SEQ_OFFSET_2, SEND_SEQ_OFFSET_3, SEQUENCE_HALF_RANGE, SEQUENCE_SLOT_COUNT, SEQUENCE_STEP,
    SequenceOrder, SequenceSlot, UINT16_OCTETS, compare_u16_sequence, wrapping_next,
};
pub use transport::{
    CIPHER_ECDHE_RSA_AES256_GCM_SHA384, CarrierSocket, STOCK_CLIENT_TRUSTS_GENERATED_CERTIFICATE,
    send_udp_datagram,
};
