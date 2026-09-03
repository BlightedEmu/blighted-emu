// SPDX-License-Identifier: AGPL-3.0-or-later
//! Messages inside the GridMate carrier (M3 spec section 3.2).
//!
//! The type-index prefix is the integer read by `GetTypeIdForIndex`.
//! It is not LEB128. Index 0 means the next 16 bytes are a UUID.
//! An index greater than 0 is a slot in the client file `typeindex.json`.
//!
//! `RegistrationRequestV3Msg` and `RegistrationResponseMsg` use the byte lists in protocol note
//! section 3. V1 and V2 stay registered and have no byte list, so this crate does not encode them
//! and does not choose a version.
//!
//! `framing` places messages in a delivered carrier payload; `envelope` reads the header of each
//! message (carrier captures note section 5).

pub mod envelope;
pub mod framing;
pub mod registration;
pub mod type_index;

pub use envelope::{Envelope, EnvelopeError};
pub use framing::{
    CLIENT_HEADER_LEN, CLIENT_SESSION_LEN, ClientMessage, FramingError, decode_client_message,
    decode_server_messages, encode_client_message, encode_server_messages,
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
