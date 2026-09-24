// SPDX-License-Identifier: AGPL-3.0-or-later
//! Other players' data (run-15 core study): the client asks for the players it shows by name
//! (4150 `SocialComponentServerFacet_RequestGetPlayers`: nearby players, friends, groups) and
//! the server answers with 4140 `SocialComponentClientFacet_PlayerDataResponse` on the same
//! header and request id. The layout comes from 350 retail answers (`newchar-retail-2026-09-18`,
//! `nightaven`, `Check_Leaderboard`): every one walks to its last byte with the fields below.
//!
//! - A request is a request id (16 bytes, reused by the client for later requests) and a list
//!   of character ids as dashed lowercase UUID text.
//! - An answer is the request id and one entry per player the server found; retail left out
//!   ids it did not know (`Check_Leaderboard`: requests of 2 to 100 ids, answers with fewer)
//!   and the client asks again for a missing one (`FUN_143f056f0`: "Received PlayerDataResponse
//!   without requested data for characterId").

use crate::appearance::PlayerIconData;
use crate::combat::{read_string, write_string};
use crate::facet::FacetHeader;
use crate::wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};

/// `SocialComponentServerFacet_RequestGetPlayers`.
pub const REQUEST_GET_PLAYERS_TYPE_INDEX: u64 = 4150;
/// `SocialComponentClientFacet_PlayerDataResponse`.
pub const PLAYER_DATA_RESPONSE_TYPE_INDEX: u64 = 4140;
/// The most ids of one request: 100, the largest retail request (`Check_Leaderboard`).
pub const MAX_REQUESTED_PLAYERS: u64 = 100;
/// The guild id of a player in no guild: 16 zero bytes (272 of 350 retail entries).
pub const NO_GUILD: [u8; RAW_ID_LEN] = [0; RAW_ID_LEN];
/// The platform of an entry without platform data: -1, with an empty platform user id and a
/// presence byte of 0 (113 of 350 retail entries).
pub const NO_PLATFORM: i32 = -1;
/// The presence byte of an entry without platform data.
pub const NO_PLATFORM_PRESENCE: u8 = 0;
/// The byte after the faction: 0 in all 350 retail entries (meaning Unknown).
pub const FACTION_TAIL: u8 = 0;
/// The `u32` after the presence byte: 0 in all 350 retail entries (meaning Unknown).
pub const PRESENCE_TAIL: u32 = 0;
/// The last byte of an entry and the `u64` after it: 1 and 0 in all 350 retail entries
/// (meaning Unknown).
pub const ENTRY_TAIL_FLAG: u8 = 1;
/// See [`ENTRY_TAIL_FLAG`].
pub const ENTRY_TAIL_VALUE: u64 = 0;

/// 4150: the client asks for the data of players by character id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerDataRequest {
    /// The header (the player's `SocialComponent`).
    pub header: FacetHeader,
    /// The request id; the answer repeats it.
    pub request: [u8; RAW_ID_LEN],
    /// The character ids, as dashed lowercase UUID text.
    pub characters: Vec<Vec<u8>>,
}

impl PlayerDataRequest {
    /// Decodes `body` when `type_index` is 4150; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        (type_index == REQUEST_GET_PLAYERS_TYPE_INDEX).then(|| {
            let mut reader = Reader::new(body);
            let header = FacetHeader::read(&mut reader)?;
            let request = reader.raw_id()?;
            let count = reader.wide_prefixed()?;
            if count > MAX_REQUESTED_PLAYERS {
                return Err(MessageError::CountTooLarge {
                    count,
                    limit: MAX_REQUESTED_PLAYERS,
                });
            }
            let characters = (0..count)
                .map(|_| read_string(&mut reader))
                .collect::<Result<_, _>>()?;
            reader.finish()?;
            Ok(Self {
                header,
                request,
                characters,
            })
        })
    }

    /// The body bytes, as the client sends them.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when an id is too long for its prefix.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.bytes(&self.request);
        // A `usize` count fits 64 bits.
        writer.wide_prefixed(self.characters.len() as u64);
        for character in &self.characters {
            write_string(&mut writer, character)?;
        }
        Ok(writer.finish())
    }

    /// The answer with `players`, the requested players the server found.
    pub fn answer(&self, players: Vec<PlayerDataView>) -> PlayerDataResponseMsg {
        PlayerDataResponseMsg {
            header: self.header,
            request: self.request,
            players,
        }
    }
}

/// One player of an answer. Field names are Inferred from the values: the pronoun byte takes
/// 0, 1 and 2 (`ePronounType`), the level is zero-based (69 for the top level 70 in the
/// 2026-09 data; one character rose 0x13, 0x14, 0x15 over a session), the faction 0 to 3, and
/// the platform 1 comes with a 17-digit Steam id while -1 comes with no id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerDataView {
    /// The character id, as dashed lowercase UUID text.
    pub character: Vec<u8>,
    /// The character name.
    pub name: Vec<u8>,
    /// The pronoun type.
    pub pronoun: u8,
    /// The character icon.
    pub icon: PlayerIconData,
    /// The guild id ([`NO_GUILD`] for none).
    pub guild: [u8; RAW_ID_LEN],
    /// The level, zero-based.
    pub level: u32,
    /// The faction (0 for none).
    pub faction: u8,
    /// The world id, as dashed lowercase UUID text.
    pub world: Vec<u8>,
    /// The presence byte ([`NO_PLATFORM_PRESENCE`] without platform data).
    pub presence: u8,
    /// The platform ([`NO_PLATFORM`] for none).
    pub platform: i32,
    /// The platform user id (empty for none).
    pub platform_user: Vec<u8>,
}

impl PlayerDataView {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let character = read_string(reader)?;
        let name = read_string(reader)?;
        let pronoun = reader.u8()?;
        let icon = PlayerIconData::read(reader)?;
        let guild = reader.raw_id()?;
        let level = reader.u32()?;
        let faction = reader.u8()?;
        expect_u64(reader.u8()?.into(), FACTION_TAIL.into())?;
        let world = read_string(reader)?;
        let presence = reader.u8()?;
        expect_u64(reader.u32()?.into(), PRESENCE_TAIL.into())?;
        let platform = i32::from_be_bytes(reader.u32()?.to_be_bytes());
        let platform_user = read_string(reader)?;
        expect_u64(reader.u8()?.into(), ENTRY_TAIL_FLAG.into())?;
        expect_u64(reader.u64()?, ENTRY_TAIL_VALUE)?;
        Ok(Self {
            character,
            name,
            pronoun,
            icon,
            guild,
            level,
            faction,
            world,
            presence,
            platform,
            platform_user,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        write_string(writer, &self.character)?;
        write_string(writer, &self.name)?;
        writer.u8(self.pronoun);
        self.icon.write(writer);
        writer
            .bytes(&self.guild)
            .u32(self.level)
            .u8(self.faction)
            .u8(FACTION_TAIL);
        write_string(writer, &self.world)?;
        writer
            .u8(self.presence)
            .u32(PRESENCE_TAIL)
            .u32(u32::from_be_bytes(self.platform.to_be_bytes()));
        write_string(writer, &self.platform_user)?;
        writer.u8(ENTRY_TAIL_FLAG).u64(ENTRY_TAIL_VALUE);
        Ok(())
    }
}

/// A field the retail answers always held at one value: another value is a layout this reader
/// does not know.
fn expect_u64(value: u64, retail: u64) -> Result<(), MessageError> {
    if value == retail {
        Ok(())
    } else {
        Err(MessageError::RowsMismatch)
    }
}

/// 4140: the players of a 4150 that the server found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerDataResponseMsg {
    /// The header of the request.
    pub header: FacetHeader,
    /// The request id.
    pub request: [u8; RAW_ID_LEN],
    /// The players.
    pub players: Vec<PlayerDataView>,
}

impl BodyCodec for PlayerDataResponseMsg {
    const TYPE_INDEX: u64 = PLAYER_DATA_RESPONSE_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.bytes(&self.request);
        // A `usize` count fits 64 bits.
        writer.wide_prefixed(self.players.len() as u64);
        for player in &self.players {
            player.write(&mut writer)?;
        }
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let request = reader.raw_id()?;
        let count = reader.wide_prefixed()?;
        if count > MAX_REQUESTED_PLAYERS {
            return Err(MessageError::CountTooLarge {
                count,
                limit: MAX_REQUESTED_PLAYERS,
            });
        }
        let players = (0..count)
            .map(|_| PlayerDataView::read(&mut reader))
            .collect::<Result<_, _>>()?;
        reader.finish()?;
        Ok(Self {
            header,
            request,
            players,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// A request in the retail form (newchar 2026-09-18, 1792696148389) with the requested id
    /// replaced: header, request id, one id.
    const REQUEST: &str = "56115c5326b60cf22715f9bca18fdecebd12f11776934bb88653f774d8328b9c012430303030303030302d303030302d343030302d383030302d303030303030303030303031";
    /// An answer in the retail form of that request, with the name, ids and platform user
    /// replaced (a 17-digit Steam id); the icon, level, faction and tails are retail.
    const ANSWER: &str = concat!(
        "56115c5326b60cf22715f9bca18fdecebd12f11776934bb88653f774d8328b9c01",
        "2430303030303030302d303030302d343030302d383030302d303030303030303030303031",
        "07506c6179657231",
        "00",
        "000008060600070000",
        "00000000000000000000000000000000",
        "00000045",
        "0300",
        "2431313131313131312d313131312d343131312d383131312d313131313131313131313131",
        "01",
        "00000000",
        "00000001",
        "113130303030303030303030303030303031",
        "010000000000000000",
    );
    /// The zero-based retail level of the entry (the top level, 70).
    const TOP_LEVEL: u32 = 69;
    const FACTION: u8 = 3;
    const STEAM: i32 = 1;

    #[test]
    fn a_request_and_its_answer_walk_in_the_retail_form() {
        let request = PlayerDataRequest::decode(REQUEST_GET_PLAYERS_TYPE_INDEX, &hex(REQUEST))
            .expect("a player data request")
            .expect("decodes");
        assert_eq!(request.characters.len(), 1);
        assert_eq!(request.encode().expect("encodes"), hex(REQUEST));
        let answer = PlayerDataResponseMsg::decode(&hex(ANSWER)).expect("decodes");
        assert_eq!(
            (answer.header, answer.request),
            (request.header, request.request)
        );
        let [player] = answer.players.as_slice() else {
            panic!("one player: {answer:?}");
        };
        assert_eq!(player.character, request.characters[0]);
        assert_eq!(player.name, b"Player1");
        assert_eq!(
            (player.level, player.faction, player.platform),
            (TOP_LEVEL, FACTION, STEAM)
        );
        assert_eq!(player.guild, NO_GUILD);
        assert_eq!(answer.encode().expect("encodes"), hex(ANSWER));
        assert_eq!(request.answer(answer.players.clone()), answer);
    }

    #[test]
    fn an_entry_without_platform_data_has_the_retail_form() {
        let mut answer = PlayerDataResponseMsg::decode(&hex(ANSWER)).expect("decodes");
        let player = &mut answer.players[0];
        player.presence = NO_PLATFORM_PRESENCE;
        player.platform = NO_PLATFORM;
        player.platform_user.clear();
        let body = answer.encode().expect("encodes");
        // Retail: `00 00000000 ffffffff 00` after the world id.
        let tail = hex("0000000000ffffffff00010000000000000000");
        assert!(body.ends_with(&tail));
        assert_eq!(PlayerDataResponseMsg::decode(&body), Ok(answer));
    }

    #[test]
    fn an_unknown_tail_value_and_a_long_request_are_refused() {
        let mut body = hex(ANSWER);
        let last = body.len() - 1;
        body[last] = 1;
        assert_eq!(
            PlayerDataResponseMsg::decode(&body),
            Err(MessageError::RowsMismatch)
        );
        let mut writer = Writer::new();
        writer.bytes(&hex(REQUEST)[..2 * RAW_ID_LEN]);
        writer.wide_prefixed(MAX_REQUESTED_PLAYERS + 1);
        assert!(matches!(
            PlayerDataRequest::decode(REQUEST_GET_PLAYERS_TYPE_INDEX, &writer.finish()),
            Some(Err(MessageError::CountTooLarge { .. }))
        ));
        assert_eq!(
            PlayerDataRequest::decode(PLAYER_DATA_RESPONSE_TYPE_INDEX, &[]),
            None
        );
    }
}
