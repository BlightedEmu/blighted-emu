// SPDX-License-Identifier: AGPL-3.0-or-later
//! Registration bodies from protocol note section 3.
//!
//! `RegistrationRequestMsg`, `RegistrationRequestV2Msg`, and `RegistrationRequestV3Msg` are all
//! registered. This module encodes the V3 body and `RegistrationResponseMsg` only.
//! V1 and V2 have no byte list. Nothing here chooses a version for the socket.

mod request_v3;
mod response;
mod token;
mod wire;

pub use request_v3::{
    BLOCK_1407D4B50_STRING_COUNT, BLOCK_1407D42C0_UINT32_COUNT, BLOCK_1407D4490_STRING_COUNT,
    BLOCK_1407D4540_STRING_PAIR_COUNT, BLOCK_1407D4920_STRING_COUNT, Block1407d3f70,
    Block1407d4b50, Block1407d5f30, Block1407d42c0, Block1407d46a0, Block1407d4490, Block1407d4540,
    Block1407d4920, FieldsAt0x2c8, FlagBit0Field, FlagByte140876fa0, ListNode1407d62b0,
    REGISTRATION_REQUEST_V3_BLOCK_0X3E0_OFFSET, REGISTRATION_REQUEST_V3_BLOCK_0XC0_OFFSET,
    REGISTRATION_REQUEST_V3_BYTE_OFFSET, REGISTRATION_REQUEST_V3_FIELDS_OFFSET,
    REGISTRATION_REQUEST_V3_LIST_OFFSET, REGISTRATION_REQUEST_V3_STRING_OFFSET,
    REGISTRATION_REQUEST_V3_UINT32_OFFSET, RegistrationRequestV3Msg,
};
pub use response::{
    REGISTRATION_RESPONSE_FLAG_COUNT, REGISTRATION_RESPONSE_FLAGS_LAST_OFFSET,
    REGISTRATION_RESPONSE_FLAGS_OFFSET, REGISTRATION_RESPONSE_OBSERVED_FLAGS,
    REGISTRATION_RESPONSE_STRING_0_OFFSET, REGISTRATION_RESPONSE_STRING_1_OFFSET,
    REGISTRATION_RESPONSE_UINT32_OFFSET, REGISTRATION_STATUS_AUTHORIZED,
    REGISTRATION_STATUS_REFUSED, RegistrationResponseMsg,
};
pub use token::LoginToken;
pub use wire::{
    FLAG_BIT_0_MASK, LIST_COUNT_ABOVE, LIST_STRING_LONGER_THAN, RegistrationError,
    SIXTEEN_RAW_OCTETS, STRING_LENGTH_OMIT,
};

/// UUID string that follows `RegistrationRequestMsg`. No body codec: the note has no byte list.
pub const REGISTRATION_REQUEST_MSG_UUID: &str = "8673a3cc-2848-4c87-aa72-cc860589d1b5";

/// UUID string that follows `RegistrationRequestV2Msg`. No body codec: the note has no byte list.
pub const REGISTRATION_REQUEST_V2_MSG_UUID: &str = "DA4E5889-A65C-4480-8642-0278160125A7";

/// UUID string that follows `RegistrationRequestV3Msg`.
pub const REGISTRATION_REQUEST_V3_MSG_UUID: &str = "0B826B33-89F5-49E0-B8CB-FE4433427778";

/// UUID string that follows `RegistrationResponseMsg`.
pub const REGISTRATION_RESPONSE_MSG_UUID: &str = "104145a7-ff95-44f1-9468-21fb41c8ac2b";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_index::{
        REGISTRATION_REQUEST_MSG_TYPE_INDEX, REGISTRATION_REQUEST_V2_MSG_TYPE_INDEX,
        REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX, REGISTRATION_RESPONSE_MSG_TYPE_INDEX,
    };

    #[test]
    fn three_request_versions_stay_registered() {
        assert_eq!(REGISTRATION_REQUEST_MSG_TYPE_INDEX, 1);
        assert_eq!(REGISTRATION_REQUEST_V2_MSG_TYPE_INDEX, 2);
        assert_eq!(REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX, 19);
        assert_eq!(REGISTRATION_RESPONSE_MSG_TYPE_INDEX, 3);
        assert_ne!(
            REGISTRATION_REQUEST_MSG_TYPE_INDEX,
            REGISTRATION_REQUEST_V2_MSG_TYPE_INDEX
        );
        assert_ne!(
            REGISTRATION_REQUEST_V2_MSG_TYPE_INDEX,
            REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX
        );
        assert_ne!(
            REGISTRATION_REQUEST_MSG_TYPE_INDEX,
            REGISTRATION_REQUEST_V3_MSG_TYPE_INDEX
        );
        assert_eq!(
            REGISTRATION_REQUEST_MSG_UUID,
            "8673a3cc-2848-4c87-aa72-cc860589d1b5"
        );
        assert_eq!(
            REGISTRATION_REQUEST_V2_MSG_UUID,
            "DA4E5889-A65C-4480-8642-0278160125A7"
        );
        assert_eq!(
            REGISTRATION_REQUEST_V3_MSG_UUID,
            "0B826B33-89F5-49E0-B8CB-FE4433427778"
        );
        assert_eq!(
            REGISTRATION_RESPONSE_MSG_UUID,
            "104145a7-ff95-44f1-9468-21fb41c8ac2b"
        );
    }
}
