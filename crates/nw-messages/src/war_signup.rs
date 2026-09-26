// SPDX-License-Identifier: AGPL-3.0-or-later
//! Territory war sign-up calls of the player's `TerritoryInteractorComponent` (retail
//! `banned_from_warleader_to_join_war`, `trying_to_join_war_daily_limit` ledgers).
//!
//! Every body starts with a facet header. After the header:
//!
//! - 2767 and 2771: one big-endian `u32` (the war; 1 and 2 in retail).
//! - 2772: the header alone.
//! - 2791 and 2784: one big-endian `u16` territory (11 and 13 in retail).
//! - 2762: the war `u32`, then the territory `u16`.
//! - 2729: the war `u32` of the 2771 it answers, then the sign-up count `u32`.
//! - 2169: one byte (0 in retail; meaning Unknown), then a `u32` (12 after a sign-up for
//!   territory 11; meaning Unknown).
//! - 2726: the war `u32`, a second `u32` (1 in retail; meaning Unknown), a count byte, then
//!   per signed-up player a character id as text, a player name as text, one byte (2 in
//!   retail; meaning Unknown) and a 16-byte id (meaning Unknown), then two bytes (3, 13 in
//!   the one-entry retail body; meaning Unknown).

use crate::combat::{read_string, write_string};
use crate::facet::FacetHeader;
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `TerritoryInteractorComponentServerFacet_RequestSignupList`.
pub const REQUEST_SIGNUP_LIST_TYPE_INDEX: u64 = 2767;
/// Type index of `TerritoryInteractorComponentClientFacet_OnSignupListReceived`.
pub const SIGNUP_LIST_RECEIVED_TYPE_INDEX: u64 = 2726;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestRoster`.
pub const REQUEST_ROSTER_TYPE_INDEX: u64 = 2772;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestRemoteInteract`.
pub const REQUEST_REMOTE_INTERACT_TYPE_INDEX: u64 = 2791;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestSignupCount`.
pub const REQUEST_SIGNUP_COUNT_TYPE_INDEX: u64 = 2771;
/// Type index of `TerritoryInteractorComponentClientFacet_OnSignupCountReceived`.
pub const SIGNUP_COUNT_RECEIVED_TYPE_INDEX: u64 = 2729;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestSignup`.
pub const REQUEST_SIGNUP_TYPE_INDEX: u64 = 2762;
/// Type index of `TerritoryInteractorComponentClientFacet_OnSignupResponseReceived`.
pub const SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX: u64 = 2169;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestLeave`.
pub const REQUEST_LEAVE_TYPE_INDEX: u64 = 2784;

/// The most sign-ups of one 2726 (an entry is at least 19 bytes).
const MAX_SIGNUPS: usize = 0x1_0000;
/// The trailing bytes of the one-entry retail 2726 (meaning Unknown).
pub const SIGNUP_LIST_TRAILER: [u8; 2] = [3, 13];

/// One signed-up player of a 2726.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupEntry {
    /// The character id as text (a UUID in retail).
    pub character: String,
    /// The player name.
    pub name: String,
    /// A byte after the name (2 in retail; meaning Unknown).
    pub flag: u8,
    /// A 16-byte id after it (meaning Unknown).
    pub id: [u8; RAW_ID_LEN],
}

/// A war sign-up call of the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarSignupClientMsg {
    /// 2767: send the sign-up list of this war.
    RequestSignupList {
        /// The header.
        header: FacetHeader,
        /// The war.
        war: u32,
    },
    /// 2772: send the current war roster.
    RequestRoster {
        /// The header.
        header: FacetHeader,
    },
    /// 2791: use a territory board from far away.
    RequestRemoteInteract {
        /// The header.
        header: FacetHeader,
        /// The territory.
        territory: u16,
    },
    /// 2771: send how many signed up for this war.
    RequestSignupCount {
        /// The header.
        header: FacetHeader,
        /// The war.
        war: u32,
    },
    /// 2762: sign the player up for this war of this territory.
    RequestSignup {
        /// The header.
        header: FacetHeader,
        /// The war.
        war: u32,
        /// The territory.
        territory: u16,
    },
    /// 2784: take the player off this territory's war sign-up.
    RequestLeave {
        /// The header.
        header: FacetHeader,
        /// The territory.
        territory: u16,
    },
}

impl WarSignupClientMsg {
    /// Decodes `body` when `type_index` is a war sign-up call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            REQUEST_SIGNUP_LIST_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestSignupList {
                    header,
                    war: reader.u32()?,
                })
            }),
            REQUEST_ROSTER_TYPE_INDEX => read(|header, _| Ok(Self::RequestRoster { header })),
            REQUEST_REMOTE_INTERACT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestRemoteInteract {
                    header,
                    territory: reader.u16()?,
                })
            }),
            REQUEST_SIGNUP_COUNT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestSignupCount {
                    header,
                    war: reader.u32()?,
                })
            }),
            REQUEST_SIGNUP_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestSignup {
                    header,
                    war: reader.u32()?,
                    territory: reader.u16()?,
                })
            }),
            REQUEST_LEAVE_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestLeave {
                    header,
                    territory: reader.u16()?,
                })
            }),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::RequestSignupList { .. } => REQUEST_SIGNUP_LIST_TYPE_INDEX,
            Self::RequestRoster { .. } => REQUEST_ROSTER_TYPE_INDEX,
            Self::RequestRemoteInteract { .. } => REQUEST_REMOTE_INTERACT_TYPE_INDEX,
            Self::RequestSignupCount { .. } => REQUEST_SIGNUP_COUNT_TYPE_INDEX,
            Self::RequestSignup { .. } => REQUEST_SIGNUP_TYPE_INDEX,
            Self::RequestLeave { .. } => REQUEST_LEAVE_TYPE_INDEX,
        }
    }

    /// The header of the call.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::RequestSignupList { header, .. }
            | Self::RequestRoster { header }
            | Self::RequestRemoteInteract { header, .. }
            | Self::RequestSignupCount { header, .. }
            | Self::RequestSignup { header, .. }
            | Self::RequestLeave { header, .. } => *header,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::RequestSignupList { war, .. } | Self::RequestSignupCount { war, .. } => {
                writer.u32(*war);
            }
            Self::RequestRoster { .. } => {}
            Self::RequestRemoteInteract { territory, .. }
            | Self::RequestLeave { territory, .. } => {
                writer.u16(*territory);
            }
            Self::RequestSignup { war, territory, .. } => {
                writer.u32(*war).u16(*territory);
            }
        }
        writer.finish()
    }
}

/// A war sign-up answer of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarSignupServerMsg {
    /// 2726: the players who signed up for the war.
    SignupListReceived {
        /// The header.
        header: FacetHeader,
        /// The war of the 2767 it answers.
        war: u32,
        /// A second value (1 in retail; meaning Unknown).
        extra: u32,
        /// The signed-up players.
        entries: Vec<SignupEntry>,
        /// The last two bytes (meaning Unknown; [`SIGNUP_LIST_TRAILER`] in retail).
        trailer: [u8; 2],
    },
    /// 2729: how many signed up for the war.
    SignupCountReceived {
        /// The header.
        header: FacetHeader,
        /// The war of the 2771 it answers.
        war: u32,
        /// How many signed up.
        count: u32,
    },
    /// 2169: the answer to a 2762 sign-up.
    SignupResponseReceived {
        /// The header.
        header: FacetHeader,
        /// The first byte (0 in retail; meaning Unknown).
        result: u8,
        /// A value after it (meaning Unknown).
        value: u32,
    },
}

impl WarSignupServerMsg {
    /// Whether `type_index` is a war sign-up answer.
    pub const fn is_type(type_index: u64) -> bool {
        matches!(
            type_index,
            SIGNUP_LIST_RECEIVED_TYPE_INDEX
                | SIGNUP_COUNT_RECEIVED_TYPE_INDEX
                | SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX
        )
    }

    /// The type index of the answer.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::SignupListReceived { .. } => SIGNUP_LIST_RECEIVED_TYPE_INDEX,
            Self::SignupCountReceived { .. } => SIGNUP_COUNT_RECEIVED_TYPE_INDEX,
            Self::SignupResponseReceived { .. } => SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX,
        }
    }

    /// The header of the answer.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::SignupListReceived { header, .. }
            | Self::SignupCountReceived { header, .. }
            | Self::SignupResponseReceived { header, .. } => *header,
        }
    }

    /// Decodes the body of the answer of `type_index`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match its layout or `type_index` is not
    /// a war sign-up answer.
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let message = match type_index {
            SIGNUP_LIST_RECEIVED_TYPE_INDEX => {
                let war = reader.u32()?;
                let extra = reader.u32()?;
                let count = usize::from(reader.u8()?);
                if count > MAX_SIGNUPS {
                    return Err(MessageError::CountTooLarge {
                        count: count as u64,
                        limit: MAX_SIGNUPS as u64,
                    });
                }
                let entries = (0..count)
                    .map(|_| {
                        Ok(SignupEntry {
                            character: String::from_utf8_lossy(&read_string(&mut reader)?)
                                .into_owned(),
                            name: String::from_utf8_lossy(&read_string(&mut reader)?).into_owned(),
                            flag: reader.u8()?,
                            id: reader.raw_id()?,
                        })
                    })
                    .collect::<Result<_, MessageError>>()?;
                Self::SignupListReceived {
                    header,
                    war,
                    extra,
                    entries,
                    trailer: reader.array()?,
                }
            }
            SIGNUP_COUNT_RECEIVED_TYPE_INDEX => Self::SignupCountReceived {
                header,
                war: reader.u32()?,
                count: reader.u32()?,
            },
            SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX => Self::SignupResponseReceived {
                header,
                result: reader.u8()?,
                value: reader.u32()?,
            },
            _ => {
                return Err(MessageError::BadTag {
                    field: "war sign-up",
                    tag: u8::try_from(type_index).unwrap_or(u8::MAX),
                });
            }
        };
        reader.finish()?;
        Ok(message)
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text is too long or the list is too large.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::SignupListReceived {
                war,
                extra,
                entries,
                trailer,
                ..
            } => {
                let count =
                    u8::try_from(entries.len()).map_err(|_| MessageError::CountTooLarge {
                        count: entries.len() as u64,
                        limit: u64::from(u8::MAX),
                    })?;
                writer.u32(*war).u32(*extra).u8(count);
                for entry in entries {
                    write_string(&mut writer, entry.character.as_bytes())?;
                    write_string(&mut writer, entry.name.as_bytes())?;
                    writer.u8(entry.flag).bytes(&entry.id);
                }
                writer.bytes(trailer);
            }
            Self::SignupCountReceived { war, count, .. } => {
                writer.u32(*war).u32(*count);
            }
            Self::SignupResponseReceived { result, value, .. } => {
                writer.u8(*result).u32(*value);
            }
        }
        Ok(writer.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// Retail `banned_from_warleader_to_join_war`: the territory interactor header and the
    /// sign-up calls and answers of war 1, territory 11.
    const HEADER: &str = "acf1940f06f669642896c8c14210296e";
    /// The one-entry sign-up list of `trying_to_join_war_daily_limit`.
    const LIST_ONE: &str = "acf1940f06f669642896c8c14210296e0000000100000001012465353834363233612d396662392d343061622d623361652d37646138653465383334333007417276617973610291d633fdaa89428dbc86d08f5219abc5030d";

    fn client(type_index: u64, args: &str) -> WarSignupClientMsg {
        WarSignupClientMsg::decode(type_index, &hex(&format!("{HEADER}{args}")))
            .expect("a sign-up call")
            .expect("reads")
    }

    #[test]
    fn the_retail_client_calls_read_and_write_back() {
        for (type_index, args) in [
            (REQUEST_SIGNUP_LIST_TYPE_INDEX, "00000001"),
            (REQUEST_ROSTER_TYPE_INDEX, ""),
            (REQUEST_REMOTE_INTERACT_TYPE_INDEX, "000b"),
            (REQUEST_SIGNUP_COUNT_TYPE_INDEX, "00000002"),
            (REQUEST_SIGNUP_TYPE_INDEX, "00000001000b"),
            (REQUEST_LEAVE_TYPE_INDEX, "000d"),
        ] {
            let call = client(type_index, args);
            assert_eq!(call.type_index(), type_index);
            assert_eq!(call.encode(), hex(&format!("{HEADER}{args}")));
        }
    }

    #[test]
    fn the_retail_answers_read_and_write_back() {
        let list = WarSignupServerMsg::decode(SIGNUP_LIST_RECEIVED_TYPE_INDEX, &hex(LIST_ONE))
            .expect("reads");
        let WarSignupServerMsg::SignupListReceived {
            war,
            entries,
            trailer,
            ..
        } = &list
        else {
            panic!("a list");
        };
        assert_eq!(*war, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Arvaysa");
        assert_eq!(entries[0].flag, 2);
        assert_eq!(*trailer, SIGNUP_LIST_TRAILER);
        assert_eq!(list.encode().expect("writes"), hex(LIST_ONE));
        for (type_index, args) in [
            (SIGNUP_COUNT_RECEIVED_TYPE_INDEX, "0000000100000003"),
            (SIGNUP_RESPONSE_RECEIVED_TYPE_INDEX, "000000000c"),
        ] {
            let body = format!("{HEADER}{args}");
            let answer = WarSignupServerMsg::decode(type_index, &hex(&body)).expect("reads");
            assert_eq!(answer.type_index(), type_index);
            assert!(WarSignupServerMsg::is_type(type_index));
            assert_eq!(answer.encode().expect("writes"), hex(&body));
        }
    }

    #[test]
    fn other_types_are_refused() {
        assert!(WarSignupClientMsg::decode(1, &[]).is_none());
        assert!(matches!(
            WarSignupServerMsg::decode(1, &hex(HEADER)),
            Err(MessageError::BadTag { .. })
        ));
    }
}
