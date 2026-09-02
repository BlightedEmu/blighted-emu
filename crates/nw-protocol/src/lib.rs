// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared protocol types for the Blighted New World emulator.
//!
//! `launcher_api` is the JSON between our launcher and `edge-http`.
//!
//! The login-queue body is the JSON object `LoginQueueResponse`.
//! `RepAddress` and `Signature` are strings on the nested `Token` object.
//! Absent keys are left out of the JSON. The client keeps a field only when the key is present.
//!
//! The type-index prefix is the integer read by `GetTypeIdForIndex`.
//! It is not LEB128. Index 0 means the next 16 bytes are a UUID.
//! An index greater than 0 is a file slot.
//!
//! `RegistrationRequestV3Msg` and `RegistrationResponseMsg` use the byte lists in protocol note
//! section 3. V1 and V2 stay registered and have no byte list, so this crate does not encode them
//! and does not choose a version.
//!
//! This crate does not define the GridMate datagram header.

pub mod launcher_api;
pub mod login_queue;
pub mod registration;
pub mod type_index;

pub use login_queue::{
    LoginQueueReply, LoginQueueRequest, LoginQueueRequestBody, LoginQueueResponse, LoginQueueToken,
    parse_login_queue_request,
};
pub use registration::{
    LIST_COUNT_ABOVE, REGISTRATION_REQUEST_MSG_UUID, REGISTRATION_REQUEST_V2_MSG_UUID,
    REGISTRATION_REQUEST_V3_MSG_UUID, REGISTRATION_RESPONSE_MSG_UUID, RegistrationError,
    RegistrationRequestV3Msg, RegistrationResponseMsg, STRING_LENGTH_OMIT,
};
pub use type_index::{
    ACK_MOVEMENT_MSG_TYPE_INDEX, ADD_ACTOR_MESSAGE_TYPE_INDEX, DELETE_ACTOR_MESSAGE_TYPE_INDEX,
    INLINE_UUID_INDEX, INLINE_UUID_LEN, MOVE_ACTOR_MSG_TYPE_INDEX,
    MOVE_ACTOR_TO_HUB_MSG_TYPE_INDEX, PING_MSG_TYPE_INDEX, PING_REQUEST_MSG_TYPE_INDEX,
    PROCESS_DEFERRED_MOVEMENT_REQUESTS_MSG_TYPE_INDEX, RECEIVE_ACTOR_MSG_TYPE_INDEX,
    REGISTRATION_REQUEST_MSG_TYPE_INDEX, REGISTRATION_REQUEST_V2_MSG_TYPE_INDEX,
    REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX, REGISTRATION_RESPONSE_MSG_TYPE_INDEX,
    START_MOVEMENT_COMMUNICATION_MSG_TYPE_INDEX, TIME_SYNCH_MSG_TYPE_INDEX, TypeId, TypeIndexError,
    TypeIndexPrefix, decode_type_id, decode_type_index_prefix, encode_type_id,
    encode_type_index_prefix,
};
