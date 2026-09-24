// SPDX-License-Identifier: AGPL-3.0-or-later
//! The title a player shows (progression note, section 10): the client asks to show one with
//! 2474 `SocialComponentServerFacet_RequestSetTitle` (the facet header and the title, the CRC-32
//! of its lowercase `TitleID`; 0 shows none), and the server sends no facet answer: the title
//! goes out in the player's `MB::SocialReplicatedState` (type 4176, player component 55), slot
//! 0 child 0, a big-endian `u32` (Observed, Hive_Social: each of 4 calls was followed 50 ms
//! later by an own 4176 change `01 01` and the title).

use crate::facet::FacetHeader;
use crate::payload::MemberState;
use crate::social::states::SocialState;
use crate::wire::{MessageError, Reader, Writer};

/// `SocialComponentServerFacet_RequestSetTitle`.
pub const SET_TITLE_TYPE_INDEX: u64 = 2474;
/// Type index of `MB::SocialReplicatedState`.
pub const SOCIAL_TYPE_INDEX: u64 = 4176;
/// The player component of the social state.
pub const SOCIAL_COMPONENT: u64 = 55;

/// 2474: show a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetTitleRequest {
    /// The header (the player's `SocialComponent`).
    pub header: FacetHeader,
    /// The title; 0 for none.
    pub title: u32,
}

impl SetTitleRequest {
    /// Decodes `body` when `type_index` is 2474; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        if type_index != SET_TITLE_TYPE_INDEX {
            return None;
        }
        let mut reader = Reader::new(body);
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            Ok(Self {
                header: FacetHeader::read(reader)?,
                title: reader.u32()?,
            })
        };
        Some(read(&mut reader).and_then(|request| {
            reader.finish()?;
            Ok(request)
        }))
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.u32(self.title).finish()
    }
}

/// The own social state change that shows `title` (the retail change after a 2474).
pub fn title_state(title: u32) -> MemberState {
    MemberState::Social(SocialState::with_title(title))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: FacetHeader = FacetHeader {
        target: 0x0102_0304_0506_0708,
        entity: 0x1112_1314_1516_1718,
    };
    /// `PlayerTitle_Stoic`, the first title of Hive_Social's 2474 calls.
    const STOIC: u32 = 0x431d_f4ea;
    /// The retail own 4176 change after it.
    const RETAIL_CHANGE: [u8; 6] = [0x01, 0x01, 0x43, 0x1d, 0xf4, 0xea];

    #[test]
    fn a_set_title_call_reads_and_its_state_is_the_retail_change() {
        let call = SetTitleRequest {
            header: HEADER,
            title: STOIC,
        };
        let body = call.encode();
        assert_eq!(
            SetTitleRequest::decode(SET_TITLE_TYPE_INDEX, &body)
                .expect("a title call")
                .expect("reads"),
            call
        );
        assert!(SetTitleRequest::decode(SET_TITLE_TYPE_INDEX + 1, &body).is_none());
        assert!(matches!(
            SetTitleRequest::decode(SET_TITLE_TYPE_INDEX, &body[..body.len() - 1]),
            Some(Err(_))
        ));
        let state = title_state(STOIC);
        assert_eq!(state.type_index(), SOCIAL_TYPE_INDEX);
        let mut writer = Writer::new();
        let MemberState::Social(social) = state else {
            panic!("a social state: {state:?}");
        };
        social.write(&mut writer).expect("writes");
        assert_eq!(writer.finish(), RETAIL_CHANGE);
    }
}
