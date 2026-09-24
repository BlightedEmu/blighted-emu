// SPDX-License-Identifier: AGPL-3.0-or-later
//! The player titles (run-15 core study). The client asks for its titles at each spawn and when
//! it opens the title list (4143, the facet header alone); retail answered each call 25 to 30 ms
//! later with 4147 on the same header: the titles the account may use, then the titles still
//! marked new, each list a count and big-endian CRC-32s of the lowercase `TitleID` of
//! `PlayerTitleData` (all 36 ids of the retail newchar answer are in the sheet). The client
//! clears the new marks with 4144 (the header alone).

use crate::facet::FacetHeader;
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// `SocialComponentServerFacet_SendAvailableTitlesToClient`.
pub const SEND_AVAILABLE_TITLES_TYPE_INDEX: u64 = 4143;
/// `SocialComponentServerFacet_ClearNewTitles`.
pub const CLEAR_NEW_TITLES_TYPE_INDEX: u64 = 4144;
/// `SocialComponentClientFacet_ReceiveAvailableTitles`.
pub const RECEIVE_AVAILABLE_TITLES_TYPE_INDEX: u64 = 4147;
/// `SocialComponentClientFacet_OnTitlesUnlock`.
pub const TITLES_UNLOCK_TYPE_INDEX: u64 = 3585;
/// Bytes of one title id.
const TITLE_ID_LEN: usize = 4;

/// One title call of the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleRequest {
    /// 4143: send the titles.
    Available {
        /// The header (the player's `SocialComponent`).
        header: FacetHeader,
    },
    /// 4144: the player saw the new titles.
    ClearNew {
        /// The header (the player's `SocialComponent`).
        header: FacetHeader,
    },
}

impl TitleRequest {
    /// Decodes `body` when `type_index` is a title call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |make: fn(FacetHeader) -> Self| {
            let mut reader = Reader::new(body);
            let header = FacetHeader::read(&mut reader)?;
            reader.finish()?;
            Ok(make(header))
        };
        Some(match type_index {
            SEND_AVAILABLE_TITLES_TYPE_INDEX => read(|header| Self::Available { header }),
            CLEAR_NEW_TITLES_TYPE_INDEX => read(|header| Self::ClearNew { header }),
            _ => return None,
        })
    }

    /// The header of the call.
    pub const fn header(&self) -> FacetHeader {
        match *self {
            Self::Available { header } | Self::ClearNew { header } => header,
        }
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::Available { .. } => SEND_AVAILABLE_TITLES_TYPE_INDEX,
            Self::ClearNew { .. } => CLEAR_NEW_TITLES_TYPE_INDEX,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        writer.finish()
    }
}

/// 4147: the titles of the player's account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableTitlesMsg {
    /// The header of the call.
    pub header: FacetHeader,
    /// The titles the player may use.
    pub available: Vec<u32>,
    /// The titles still marked new.
    pub new: Vec<u32>,
}

impl BodyCodec for AvailableTitlesMsg {
    const TYPE_INDEX: u64 = RECEIVE_AVAILABLE_TITLES_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        write_titles(&mut writer, &self.available);
        write_titles(&mut writer, &self.new);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let available = read_titles(&mut reader)?;
        let new = read_titles(&mut reader)?;
        reader.finish()?;
        Ok(Self {
            header,
            available,
            new,
        })
    }
}

/// 3585: the player unlocked these titles at once (retail `Hive Transmog` and
/// `Hive_catacomes_test_2`: a count byte, then that many title CRCs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitlesUnlockMsg {
    /// The header of the call.
    pub header: FacetHeader,
    /// The unlocked titles (CRC-32 of each lowercase `TitleID`).
    pub titles: Vec<u32>,
}

impl BodyCodec for TitlesUnlockMsg {
    const TYPE_INDEX: u64 = TITLES_UNLOCK_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        write_titles(&mut writer, &self.titles);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let titles = read_titles(&mut reader)?;
        reader.finish()?;
        Ok(Self { header, titles })
    }
}

/// A count (retail `aa02` for 170 titles: the wide prefix code), then the ids.
fn write_titles(writer: &mut Writer, titles: &[u32]) {
    // A `usize` count fits 64 bits.
    writer.wide_prefixed(titles.len() as u64);
    for title in titles {
        writer.u32(*title);
    }
}

fn read_titles(reader: &mut Reader<'_>) -> Result<Vec<u32>, MessageError> {
    let count = reader.wide_prefixed()?;
    let limit = u64::try_from(reader.remaining() / TITLE_ID_LEN).unwrap_or(u64::MAX);
    if count > limit {
        return Err(MessageError::CountTooLarge { count, limit });
    }
    (0..count).map(|_| reader.u32()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// Retail newchar 2026-09-18 at 1792695698095: the call, and its answer 29 ms later (36
    /// titles, 3 new: `PlayerTitle_FreshStart`, `PlayerTitle_Preorder`, `PlayerTitle_Stoic`).
    const REQUEST: &str = "56115c5326b60cf22715f9bca18fdece";
    const ANSWER: &str = "56115c5326b60cf22715f9bca18fdece24431df4ea8a9cfcacb1873b4922e1be3939225d961bb38d5fa946d8abb4fd71e5bd0e3fb4283d2a4e88f9b0498ffc89e45956d27eadc2c5462f2e981924772c9e16f5d85e28ae31c045599eeeac1d222585d718fc6209a840fe8863a753701c0895e22da35fa44488236074856cb4bdc9755f154848aef855fcd8aea81a454c5779ab85e90cfdbb36ebcf934d4dc6207c03b1873b498a9cfcac431df4ea";
    const RETAIL_TITLES: usize = 36;
    /// CRC-32 of `playertitle_stoic`.
    const STOIC: u32 = 0x431d_f4ea;

    #[test]
    fn the_retail_answer_decodes_and_encodes_byte_exact() {
        let request = TitleRequest::decode(SEND_AVAILABLE_TITLES_TYPE_INDEX, &hex(REQUEST))
            .expect("a title call")
            .expect("decodes");
        assert_eq!(request.encode(), hex(REQUEST));
        let answer = AvailableTitlesMsg::decode(&hex(ANSWER)).expect("decodes");
        assert_eq!(answer.header, request.header());
        assert_eq!(answer.available.len(), RETAIL_TITLES);
        assert_eq!(answer.available[0], STOIC);
        assert_eq!(answer.new.last(), Some(&STOIC));
        assert_eq!(answer.encode().expect("encodes"), hex(ANSWER));
    }

    /// More titles than one count byte holds take the two-byte count (retail `aa02`).
    const MANY_TITLES: u32 = 170;
    const TWO_BYTE_COUNT: [u8; 2] = [0xaa, 0x02];

    #[test]
    fn a_long_list_has_the_retail_two_byte_count() {
        let header = TitleRequest::decode(CLEAR_NEW_TITLES_TYPE_INDEX, &hex(REQUEST))
            .expect("a title call")
            .expect("decodes")
            .header();
        let message = AvailableTitlesMsg {
            header,
            available: (0..MANY_TITLES).collect(),
            new: Vec::new(),
        };
        let body = message.encode().expect("encodes");
        assert_eq!(body[REQUEST.len() / 2..][..2], TWO_BYTE_COUNT);
        assert_eq!(AvailableTitlesMsg::decode(&body), Ok(message));
    }

    /// Retail `Hive Transmog`: three titles unlocked at once.
    const RETAIL_UNLOCK: &str = "11c62f11c397b2f4e34e14f344a974cc03fe9b923ce9494937cee4ca9d";

    #[test]
    fn the_retail_unlock_reads_and_writes_back() {
        let unlock = TitlesUnlockMsg::decode(&hex(RETAIL_UNLOCK)).expect("decodes");
        assert_eq!(unlock.titles.len(), 3);
        assert_eq!(unlock.encode().expect("encodes"), hex(RETAIL_UNLOCK));
    }

    #[test]
    fn a_count_beyond_the_body_is_refused() {
        let mut body = hex(REQUEST);
        body.push(1);
        assert!(matches!(
            AvailableTitlesMsg::decode(&body),
            Err(MessageError::CountTooLarge { .. })
        ));
        assert_eq!(
            TitleRequest::decode(crate::lookups::LOBBY_COUNTS_TYPE_INDEX, &body),
            None
        );
    }
}
