// SPDX-License-Identifier: AGPL-3.0-or-later
//! Named view of Group 4 of `RegistrationRequestV3Msg`: the login-queue `Token` (protocol note
//! section 3; field order checked on the 43 captured requests, carrier captures note section 7).
//!
//! The wire order follows the `Token` struct: `Signature`, `RepAddress`, `WorldId`,
//! `CharacterId`, `PersonaId`, `SteamAppId`, `SteamUserId`, `ChannelId`, `TicketId`,
//! `LocationGroupId`, `LocationId`, `GenerateTime`, `IssueTime`, `AccountAge`,
//! `ClientCapabilities`, `TokenVersion`, the owner flags, `HostHash`, `AccountIsLocked`,
//! `JwtClaims`, and two more strings.

use super::request_v3::{
    Block1407d3f70, Block1407d4b50, Block1407d5f30, Block1407d42c0, Block1407d46a0, Block1407d4490,
    Block1407d4540, Block1407d4920, FieldsAt0x2c8, FlagBit0Field, FlagByte140876fa0,
    RegistrationRequestV3Msg,
};
use super::wire::BYTE_ZERO;

/// Presence byte of `AccountIsLocked` when the value follows.
const PRESENT: u8 = 1;

/// The login-queue `Token` as the client sends it in V3.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoginToken {
    /// `Signature`.
    pub signature: Vec<u8>,
    /// `RepAddress`.
    pub rep_address: Vec<u8>,
    /// `WorldId`.
    pub world_id: Vec<u8>,
    /// `CharacterId`.
    pub character_id: Vec<u8>,
    /// `PersonaId`.
    pub persona_id: Vec<u8>,
    /// `SteamAppId`.
    pub steam_app_id: u32,
    /// `SteamUserId`.
    pub steam_user_id: Vec<u8>,
    /// `ChannelId`.
    pub channel_id: Vec<u8>,
    /// `TicketId`.
    pub ticket_id: Vec<u8>,
    /// `LocationGroupId`.
    pub location_group_id: Vec<u8>,
    /// `LocationId`.
    pub location_id: Vec<u8>,
    /// `GenerateTime`, Unix seconds.
    pub generate_time: u32,
    /// `IssueTime`, Unix seconds.
    pub issue_time: u32,
    /// `AccountAge`.
    pub account_age: u32,
    /// `ClientCapabilities`.
    pub client_capabilities: Vec<u8>,
    /// `TokenVersion`.
    pub token_version: u32,
    /// `IsPermanentAppOwner` / `IsTrialOwner` bits.
    pub owner_flags: u8,
    /// `HostHash`.
    pub host_hash: Vec<u8>,
    /// `AccountIsLocked` (0 or 1), when present.
    pub account_is_locked: Option<u8>,
    /// `JwtClaims`.
    pub jwt_claims: Vec<u8>,
    /// The two strings after `JwtClaims`.
    pub jwt_strings: [Vec<u8>; 2],
}

impl LoginToken {
    fn from_block(block: &Block1407d5f30) -> Self {
        let [rep_address, world_id, character_id, persona_id] = block.next.strings.clone();
        let first = &block.next.next;
        let [steam_user_id, channel_id, ticket_id, location_group_id] = first.strings.clone();
        let rest = &first.next;
        let [generate_time, issue_time, account_age] = rest.uint32s;
        Self {
            signature: block.bytes.clone(),
            rep_address,
            world_id,
            character_id,
            persona_id,
            steam_app_id: block.next.uint32,
            steam_user_id,
            channel_id,
            ticket_id,
            location_group_id,
            location_id: rest.string_0.clone(),
            generate_time,
            issue_time,
            account_age,
            client_capabilities: rest.string_1.clone(),
            token_version: rest.uint32,
            owner_flags: rest.raw_byte,
            host_hash: rest.string_2.clone(),
            account_is_locked: rest.zero_or_one_when_one,
            jwt_claims: rest.next.bytes.clone(),
            jwt_strings: rest.next.next.strings.clone(),
        }
    }

    fn into_block(self) -> Block1407d5f30 {
        Block1407d5f30 {
            bytes: self.signature,
            next: Block1407d4920 {
                strings: [
                    self.rep_address,
                    self.world_id,
                    self.character_id,
                    self.persona_id,
                ],
                uint32: self.steam_app_id,
                next: Block1407d4b50 {
                    strings: [
                        self.steam_user_id,
                        self.channel_id,
                        self.ticket_id,
                        self.location_group_id,
                    ],
                    next: Block1407d42c0 {
                        string_0: self.location_id,
                        uint32s: [self.generate_time, self.issue_time, self.account_age],
                        string_1: self.client_capabilities,
                        uint32: self.token_version,
                        raw_byte: self.owner_flags,
                        string_2: self.host_hash,
                        zero_or_one: if self.account_is_locked.is_some() {
                            PRESENT
                        } else {
                            BYTE_ZERO
                        },
                        zero_or_one_when_one: self.account_is_locked,
                        next: Block1407d46a0 {
                            bytes: self.jwt_claims,
                            next: Block1407d4490 {
                                strings: self.jwt_strings,
                            },
                        },
                    },
                },
            },
        }
    }
}

fn empty_flagged() -> FlagByte140876fa0 {
    FlagByte140876fa0 {
        flag: BYTE_ZERO,
        field: FlagBit0Field::LengthPrefixed(Vec::new()),
    }
}

impl RegistrationRequestV3Msg {
    /// The login token in Group 4.
    pub fn login_token(&self) -> LoginToken {
        LoginToken::from_block(&self.block_at_0xc0)
    }

    /// A V3 request that carries `token`; every other group is zero or empty.
    pub fn with_login_token(token: LoginToken) -> Self {
        Self {
            uint32_at_0x8: 0,
            list_at_0x10: Vec::new(),
            bytes_at_0xa0: Vec::new(),
            block_at_0xc0: token.into_block(),
            fields_at_0x2c8: FieldsAt0x2c8 {
                uint32_0: 0,
                zero_or_one: BYTE_ZERO,
                uint32_1: 0,
                bytes: Vec::new(),
                next: Block1407d4540 {
                    strings_before: Default::default(),
                    uint32: 0,
                    strings_after: Default::default(),
                    next: Block1407d46a0 {
                        bytes: Vec::new(),
                        next: Block1407d4490 {
                            strings: Default::default(),
                        },
                    },
                },
            },
            block_at_0x3e0: Block1407d3f70 {
                flagged_0: empty_flagged(),
                flagged_1: empty_flagged(),
                zero_or_one: BYTE_ZERO,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEAM_APP_ID: u32 = 1_063_730;
    const TOKEN_VERSION: u32 = 10;
    const GENERATE_TIME: u32 = 1_782_067_831;
    const LOCKED: u8 = 1;

    fn token() -> LoginToken {
        LoginToken {
            signature: b"signature".to_vec(),
            rep_address: b"127.0.0.1:33435".to_vec(),
            world_id: b"8d0f4c2e-0000-4000-8000-000000000001".to_vec(),
            character_id: b"2b87684d-c82c-46a4-90e4-c76402477507".to_vec(),
            persona_id: b"persona".to_vec(),
            steam_app_id: STEAM_APP_ID,
            ticket_id: b"ticket".to_vec(),
            generate_time: GENERATE_TIME,
            issue_time: GENERATE_TIME,
            token_version: TOKEN_VERSION,
            account_is_locked: Some(LOCKED),
            ..LoginToken::default()
        }
    }

    #[test]
    fn a_token_survives_the_v3_encoding() {
        let request = RegistrationRequestV3Msg::with_login_token(token());
        let bytes = request.encode().expect("encode");
        let decoded = RegistrationRequestV3Msg::decode(&bytes).expect("decode");
        assert_eq!(decoded.login_token(), token());
    }

    #[test]
    fn named_fields_sit_in_the_struct_order() {
        let request = RegistrationRequestV3Msg::with_login_token(token());
        let block = &request.block_at_0xc0;
        assert_eq!(block.bytes, b"signature");
        assert_eq!(block.next.strings[0], b"127.0.0.1:33435");
        assert_eq!(block.next.uint32, STEAM_APP_ID);
        assert_eq!(block.next.next.strings[2], b"ticket");
        assert_eq!(block.next.next.next.uint32, TOKEN_VERSION);
        assert_eq!(block.next.next.next.zero_or_one, PRESENT);
    }

    #[test]
    fn a_missing_lock_flag_is_absent() {
        let mut unlocked = token();
        unlocked.account_is_locked = None;
        let bytes = RegistrationRequestV3Msg::with_login_token(unlocked.clone())
            .encode()
            .expect("encode");
        let decoded = RegistrationRequestV3Msg::decode(&bytes).expect("decode");
        assert_eq!(decoded.login_token(), unlocked);
    }
}
