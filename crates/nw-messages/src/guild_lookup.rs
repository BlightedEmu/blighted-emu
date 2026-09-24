// SPDX-License-Identifier: AGPL-3.0-or-later
//! Company (guild) look-ups for name plates (run-15 core study): the client asks for the
//! companies of the players it shows (3639 `GuildsComponentServerFacet_RequestGetGuilds`) and
//! the server answers with 3610 `GuildsComponentClientFacet_OnGetUnorderedGuildsMsg` on the same
//! header and request id. Retail sent each answer twice, the same bytes (newchar
//! 2026-09-18: 12 calls, 24 answers; all ledgers: 68,095 calls, 131,155 answers). The entry is
//! the client's `OtherGuildData` (reflection `FUN_145cabe00`: `m_guildName`,
//! `m_guildMasterCharacterIdString`, `m_faction`, `m_crestData`, `m_numMembers`, `m_numClaims`,
//! `m_numWars`, `m_siegeWindow`) after the guild id; 3,012 retail answers walk to their last
//! byte with it.

use crate::buildables::GuildCrest;
use crate::combat::{read_string, write_string};
use crate::facet::FacetHeader;
use crate::wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};

/// `GuildsComponentServerFacet_RequestGetGuilds`.
pub const REQUEST_GET_GUILDS_TYPE_INDEX: u64 = 3639;
/// `GuildsComponentClientFacet_OnGetUnorderedGuildsMsg`.
pub const UNORDERED_GUILDS_TYPE_INDEX: u64 = 3610;
/// The most guild ids of one call (retail calls hold one).
pub const MAX_REQUESTED_GUILDS: u64 = 100;

/// 3639: the client asks for companies by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildsRequest {
    /// The header (the player's `GuildsComponent`).
    pub header: FacetHeader,
    /// The request id; the answer repeats it.
    pub request: [u8; RAW_ID_LEN],
    /// The guild ids.
    pub guilds: Vec<[u8; RAW_ID_LEN]>,
}

impl GuildsRequest {
    /// Decodes `body` when `type_index` is 3639; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        (type_index == REQUEST_GET_GUILDS_TYPE_INDEX).then(|| {
            let mut reader = Reader::new(body);
            let header = FacetHeader::read(&mut reader)?;
            let request = reader.raw_id()?;
            let count = reader.wide_prefixed()?;
            if count > MAX_REQUESTED_GUILDS {
                return Err(MessageError::CountTooLarge {
                    count,
                    limit: MAX_REQUESTED_GUILDS,
                });
            }
            let guilds = (0..count)
                .map(|_| reader.raw_id())
                .collect::<Result<_, _>>()?;
            reader.finish()?;
            Ok(Self {
                header,
                request,
                guilds,
            })
        })
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.bytes(&self.request);
        // A `usize` count fits 64 bits.
        writer.wide_prefixed(self.guilds.len() as u64);
        for guild in &self.guilds {
            writer.bytes(guild);
        }
        writer.finish()
    }

    /// The answer with `guilds`, the requested companies the server knows.
    pub fn answer(&self, guilds: Vec<OtherGuildData>) -> UnorderedGuildsMsg {
        UnorderedGuildsMsg {
            header: self.header,
            request: self.request,
            guilds,
        }
    }
}

/// `m_siegeWindow`: an `f32` (retail 1.0, 2.0, 3.0, 4.0, 20.0, 22.0, 23.0, and 0.0 in 4 of
/// 3,012: Inferred, the hour the war window starts) and a text: empty in 17,556 of 18,114 retail
/// entries of 34 sessions, 44 base64 characters in the other 558 (meaning Unknown; Inferred, the
/// region rule of `GuildSiegeWindowRegionData`).
#[derive(Debug, Clone, PartialEq)]
pub struct SiegeWindow {
    /// The start hour.
    pub start_hour: f32,
    /// The text after it.
    pub rule: Vec<u8>,
}

/// One company of an answer.
#[derive(Debug, Clone, PartialEq)]
pub struct OtherGuildData {
    /// The guild id.
    pub guild: [u8; RAW_ID_LEN],
    /// `m_guildName`.
    pub name: Vec<u8>,
    /// `m_guildMasterCharacterIdString`, dashed lowercase UUID text.
    pub master: Vec<u8>,
    /// `m_faction` (retail 1 to 3).
    pub faction: u8,
    /// `m_crestData`.
    pub crest: GuildCrest,
    /// `m_numMembers`.
    pub members: u32,
    /// `m_numClaims`.
    pub claims: u32,
    /// `m_numWars`.
    pub wars: u32,
    /// `m_siegeWindow`.
    pub siege_window: SiegeWindow,
}

impl OtherGuildData {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            guild: reader.raw_id()?,
            name: read_string(reader)?,
            master: read_string(reader)?,
            faction: reader.u8()?,
            crest: GuildCrest::read(reader)?,
            members: reader.u32()?,
            claims: reader.u32()?,
            wars: reader.u32()?,
            siege_window: SiegeWindow {
                start_hour: reader.f32()?,
                rule: read_string(reader)?,
            },
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.bytes(&self.guild);
        write_string(writer, &self.name)?;
        write_string(writer, &self.master)?;
        writer.u8(self.faction);
        self.crest.write(writer);
        writer
            .u32(self.members)
            .u32(self.claims)
            .u32(self.wars)
            .f32(self.siege_window.start_hour);
        write_string(writer, &self.siege_window.rule)
    }
}

/// 3610: the companies of a 3639 that the server knows.
#[derive(Debug, Clone, PartialEq)]
pub struct UnorderedGuildsMsg {
    /// The header of the call.
    pub header: FacetHeader,
    /// The request id.
    pub request: [u8; RAW_ID_LEN],
    /// The companies.
    pub guilds: Vec<OtherGuildData>,
}

impl BodyCodec for UnorderedGuildsMsg {
    const TYPE_INDEX: u64 = UNORDERED_GUILDS_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.bytes(&self.request);
        // A `usize` count fits 64 bits.
        writer.wide_prefixed(self.guilds.len() as u64);
        for guild in &self.guilds {
            guild.write(&mut writer)?;
        }
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let request = reader.raw_id()?;
        let count = reader.wide_prefixed()?;
        if count > MAX_REQUESTED_GUILDS {
            return Err(MessageError::CountTooLarge {
                count,
                limit: MAX_REQUESTED_GUILDS,
            });
        }
        let guilds = (0..count)
            .map(|_| OtherGuildData::read(&mut reader))
            .collect::<Result<_, _>>()?;
        reader.finish()?;
        Ok(Self {
            header,
            request,
            guilds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// A call in the retail form (newchar 2026-09-18, 1792695699623) with the ids replaced.
    const REQUEST: &str = "ca8ff742fa9e42cf2715f9bca18fdece0102030405060708090a0b0c0d0e0f1001a1a2a3a4a5a6a7a8a9aaabacadaeafb0";
    /// Its answer in the retail form: name, master and ids replaced; faction, crest, counts
    /// and window are those of a retail answer of that session.
    const ANSWER: &str = concat!(
        "ca8ff742fa9e42cf2715f9bca18fdece0102030405060708090a0b0c0d0e0f1001",
        "a1a2a3a4a5a6a7a8a9aaabacadaeafb0",
        "07436f6d70616e79",
        "2432323232323232322d323232322d343232322d383232322d323232323232323232323232",
        "03",
        "00f63f09898a3ebebebf3d50d0d23f80000000d93f6eeef03f6feff13f6dedef3f800000",
        "00000055",
        "00000002",
        "00000000",
        "4000000000",
    );
    const MEMBERS: u32 = 0x55;
    const CLAIMS: u32 = 2;
    const START_HOUR: f32 = 2.0;
    const BACKGROUND: u16 = 0xf6;
    const FOREGROUND: u16 = 0xd9;

    #[test]
    fn a_call_and_its_answer_walk_in_the_retail_form() {
        let request = GuildsRequest::decode(REQUEST_GET_GUILDS_TYPE_INDEX, &hex(REQUEST))
            .expect("a guild call")
            .expect("decodes");
        assert_eq!(request.encode(), hex(REQUEST));
        let answer = UnorderedGuildsMsg::decode(&hex(ANSWER)).expect("decodes");
        assert_eq!(
            (answer.header, answer.request),
            (request.header, request.request)
        );
        let [guild] = answer.guilds.as_slice() else {
            panic!("one company: {answer:?}");
        };
        assert_eq!(guild.guild, request.guilds[0]);
        assert_eq!(guild.name, b"Company");
        assert_eq!(
            (guild.crest.background, guild.crest.foreground),
            (BACKGROUND, FOREGROUND)
        );
        assert_eq!(
            (guild.members, guild.claims, guild.wars),
            (MEMBERS, CLAIMS, 0)
        );
        assert_eq!(guild.siege_window.start_hour, START_HOUR);
        assert_eq!(answer.encode().expect("encodes"), hex(ANSWER));
        assert_eq!(request.answer(answer.guilds.clone()), answer);
    }

    #[test]
    fn an_entry_with_the_44_character_window_text_walks() {
        // 558 retail entries end with a 44-character text instead of the empty one (a retail
        // text shape; the characters replaced).
        const RULE: &str = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVphYmNkZWY=";
        let mut answer = UnorderedGuildsMsg::decode(&hex(ANSWER)).expect("decodes");
        answer.guilds[0].siege_window.rule = RULE.as_bytes().to_vec();
        let body = answer.encode().expect("encodes");
        assert_eq!(body.len(), hex(ANSWER).len() + RULE.len());
        assert_eq!(UnorderedGuildsMsg::decode(&body).expect("decodes"), answer);
    }

    #[test]
    fn an_answer_with_no_company_is_the_header_the_id_and_a_zero_count() {
        let request = GuildsRequest::decode(REQUEST_GET_GUILDS_TYPE_INDEX, &hex(REQUEST))
            .expect("a guild call")
            .expect("decodes");
        let body = request.answer(Vec::new()).encode().expect("encodes");
        let mut expected = hex(REQUEST)[..2 * RAW_ID_LEN].to_vec();
        expected.push(0);
        assert_eq!(body, expected);
        assert!(GuildsRequest::decode(UNORDERED_GUILDS_TYPE_INDEX, &body).is_none());
    }
}
