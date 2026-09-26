// SPDX-License-Identifier: AGPL-3.0-or-later
//! War board, capture-point siege and PvP spectator calls (retail
//! `arvaysa_ES_attacker_war_45vs45`, `gorgon_arva_1` ledgers).
//!
//! Every body starts with a facet header. The war board calls (3206, 5953) and the spectator
//! stop (6845) carry the header alone. The siege calls (4062, 4063) carry the header of the
//! capture point alone.
//!
//! The war board pushes (839, 3530, 2968) carry a variable list part after the header whose
//! inner layout is not fully traced (Azoth catalog: a big-endian `u16`, then nested
//! structures). This crate keeps those bytes whole: a body reads and writes back byte-exact,
//! and [`WarboardManifest::of_participants`] builds the 3530 form this server sends, from the
//! pattern of the retail bodies (a participant count, the value 2, then one character
//! reference and one zero byte per participant, then the bytes 1, 0, 0, 0; the retail bodies
//! of 3 and 6 participants end with those bytes or with participant indexes, so the exact
//! tail is Unknown).

use crate::facet::FacetHeader;
use crate::social::CharacterRef;
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `WarboardComponentClientFacet_OnUpdateWarboardStats`.
pub const UPDATE_WARBOARD_STATS_TYPE_INDEX: u64 = 839;
/// Type index of `WarboardComponentClientFacet_OnUpdateFullWarboardManifest`.
pub const UPDATE_FULL_WARBOARD_MANIFEST_TYPE_INDEX: u64 = 3530;
/// Type index of `PlayerDataBroadcastComponentServerFacet_OnWarboardEnabled`.
pub const WARBOARD_ENABLED_TYPE_INDEX: u64 = 3206;
/// Type index of `WarboardComponentClientFacet_OnUpdateWarboardManifest`.
pub const UPDATE_WARBOARD_MANIFEST_TYPE_INDEX: u64 = 2968;
/// Type index of `PlayerDataBroadcastComponentServerFacet_OnUpdateWarboardManifestReceived`.
pub const WARBOARD_MANIFEST_RECEIVED_TYPE_INDEX: u64 = 5953;
/// Type index of `CapturePointComponentClientFacet_OnSiegeBegan`.
pub const SIEGE_BEGAN_TYPE_INDEX: u64 = 4062;
/// Type index of `CapturePointComponentClientFacet_OnSiegeEnded`.
pub const SIEGE_ENDED_TYPE_INDEX: u64 = 4063;
/// Type index of `PvPSpectatorCamControllerComponentServerFacet_RequestStopSpectating`.
pub const STOP_SPECTATING_TYPE_INDEX: u64 = 6845;

/// The second `u16` of every retail 3530 after its participant count (meaning Unknown).
const MANIFEST_SECOND: u16 = 2;
/// The bytes after the participants of the retail 3530 of 3 participants (meaning Unknown).
const MANIFEST_TAIL: [u8; 4] = [1, 0, 0, 0];
/// The byte after each participant reference of a 3530 (0 in the retail bodies read).
const PARTICIPANT_BYTE: u8 = 0;

/// A war call of the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarClientMsg {
    /// 3206: the player turned the war board on.
    WarboardEnabled {
        /// The header.
        header: FacetHeader,
    },
    /// 5953: the client received an updated war board list.
    WarboardManifestReceived {
        /// The header.
        header: FacetHeader,
    },
    /// 6845: stop spectating and go back to the player's own character.
    StopSpectating {
        /// The header.
        header: FacetHeader,
    },
}

impl WarClientMsg {
    /// Decodes `body` when `type_index` is a war call of the client; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let make = match type_index {
            WARBOARD_ENABLED_TYPE_INDEX => {
                fn make(header: FacetHeader) -> WarClientMsg {
                    WarClientMsg::WarboardEnabled { header }
                }
                make
            }
            WARBOARD_MANIFEST_RECEIVED_TYPE_INDEX => {
                fn make(header: FacetHeader) -> WarClientMsg {
                    WarClientMsg::WarboardManifestReceived { header }
                }
                make
            }
            STOP_SPECTATING_TYPE_INDEX => {
                fn make(header: FacetHeader) -> WarClientMsg {
                    WarClientMsg::StopSpectating { header }
                }
                make
            }
            _ => return None,
        };
        let mut reader = Reader::new(body);
        let header = match FacetHeader::read(&mut reader) {
            Ok(header) => header,
            Err(error) => return Some(Err(error)),
        };
        Some(reader.finish().map(|()| make(header)))
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::WarboardEnabled { .. } => WARBOARD_ENABLED_TYPE_INDEX,
            Self::WarboardManifestReceived { .. } => WARBOARD_MANIFEST_RECEIVED_TYPE_INDEX,
            Self::StopSpectating { .. } => STOP_SPECTATING_TYPE_INDEX,
        }
    }

    /// The header of the call.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::WarboardEnabled { header }
            | Self::WarboardManifestReceived { header }
            | Self::StopSpectating { header } => *header,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        writer.finish()
    }
}

/// The list part of a 3530 full war board manifest: the participants of a war.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarboardManifest {
    /// The participants, in list order.
    pub participants: Vec<CharacterRef>,
}

impl WarboardManifest {
    /// The manifest of these participants.
    pub fn of_participants(participants: Vec<CharacterRef>) -> Self {
        Self { participants }
    }

    /// The list bytes after the header, in the form this server sends.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for more participants than a `u16` holds.
    pub fn encode_payload(&self) -> Result<Vec<u8>, MessageError> {
        let count =
            u16::try_from(self.participants.len()).map_err(|_| MessageError::CountTooLarge {
                count: self.participants.len() as u64,
                limit: u64::from(u16::MAX),
            })?;
        let mut writer = Writer::new();
        writer.u16(count).u16(MANIFEST_SECOND);
        for participant in &self.participants {
            participant.write(&mut writer);
            writer.u8(PARTICIPANT_BYTE);
        }
        writer.bytes(&MANIFEST_TAIL);
        Ok(writer.finish())
    }
}

/// A war push of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarServerMsg {
    /// 839: the numbers of the war board. The list part after the header is kept whole (its
    /// inner layout is not fully traced).
    UpdateWarboardStats {
        /// The header.
        header: FacetHeader,
        /// The bytes after the header.
        payload: Vec<u8>,
    },
    /// 3530: the full list of the war board.
    UpdateFullWarboardManifest {
        /// The header.
        header: FacetHeader,
        /// The bytes after the header.
        payload: Vec<u8>,
    },
    /// 2968: an update of the war board list. The list part is kept whole.
    UpdateWarboardManifest {
        /// The header.
        header: FacetHeader,
        /// The bytes after the header.
        payload: Vec<u8>,
    },
    /// 4062: a siege began at this capture point.
    SiegeBegan {
        /// The header (the capture point).
        header: FacetHeader,
    },
    /// 4063: a siege ended at this capture point.
    SiegeEnded {
        /// The header (the capture point).
        header: FacetHeader,
    },
}

impl WarServerMsg {
    /// Whether `type_index` is a war push of the server.
    pub const fn is_type(type_index: u64) -> bool {
        matches!(
            type_index,
            UPDATE_WARBOARD_STATS_TYPE_INDEX
                | UPDATE_FULL_WARBOARD_MANIFEST_TYPE_INDEX
                | UPDATE_WARBOARD_MANIFEST_TYPE_INDEX
                | SIEGE_BEGAN_TYPE_INDEX
                | SIEGE_ENDED_TYPE_INDEX
        )
    }

    /// The type index of the push.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::UpdateWarboardStats { .. } => UPDATE_WARBOARD_STATS_TYPE_INDEX,
            Self::UpdateFullWarboardManifest { .. } => UPDATE_FULL_WARBOARD_MANIFEST_TYPE_INDEX,
            Self::UpdateWarboardManifest { .. } => UPDATE_WARBOARD_MANIFEST_TYPE_INDEX,
            Self::SiegeBegan { .. } => SIEGE_BEGAN_TYPE_INDEX,
            Self::SiegeEnded { .. } => SIEGE_ENDED_TYPE_INDEX,
        }
    }

    /// The header of the push.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::UpdateWarboardStats { header, .. }
            | Self::UpdateFullWarboardManifest { header, .. }
            | Self::UpdateWarboardManifest { header, .. }
            | Self::SiegeBegan { header }
            | Self::SiegeEnded { header } => *header,
        }
    }

    /// A 3530 of `manifest` to the player of `header`.
    pub fn full_manifest(
        header: FacetHeader,
        manifest: &WarboardManifest,
    ) -> Result<Self, MessageError> {
        Ok(Self::UpdateFullWarboardManifest {
            header,
            payload: manifest.encode_payload()?,
        })
    }

    /// Decodes the body of the push of `type_index`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or `type_index` is not a war push.
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let message = match type_index {
            UPDATE_WARBOARD_STATS_TYPE_INDEX => Self::UpdateWarboardStats {
                header,
                payload: reader.rest().to_vec(),
            },
            UPDATE_FULL_WARBOARD_MANIFEST_TYPE_INDEX => Self::UpdateFullWarboardManifest {
                header,
                payload: reader.rest().to_vec(),
            },
            UPDATE_WARBOARD_MANIFEST_TYPE_INDEX => Self::UpdateWarboardManifest {
                header,
                payload: reader.rest().to_vec(),
            },
            SIEGE_BEGAN_TYPE_INDEX => {
                reader.finish()?;
                Self::SiegeBegan { header }
            }
            SIEGE_ENDED_TYPE_INDEX => {
                reader.finish()?;
                Self::SiegeEnded { header }
            }
            _ => {
                return Err(MessageError::BadTag {
                    field: "war",
                    tag: u8::try_from(type_index).unwrap_or(u8::MAX),
                });
            }
        };
        Ok(message)
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::UpdateWarboardStats { payload, .. }
            | Self::UpdateFullWarboardManifest { payload, .. }
            | Self::UpdateWarboardManifest { payload, .. } => {
                writer.bytes(payload);
            }
            Self::SiegeBegan { .. } | Self::SiegeEnded { .. } => {}
        }
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// Retail `arvaysa_ES_attacker_war_45vs45`: the war board header of the player and one
    /// body of each war push and call.
    const WARBOARD: &str = "e5e42e8ff2cfbbdeb8341e5e44440536";
    const STATS: &str = "e5e42e8ff2cfbbdeb8341e5e44440536000300000100000201000000";
    const MANIFEST_UPDATE: &str =
        "e5e42e8ff2cfbbdeb8341e5e44440536000400000000000105cb8ed912b72c48a2b8a17767825b42f5010000";
    const ENABLED: &str = "30bf9374ec0960172896c8c14210296e";
    /// A capture point header of a retail 4062.
    const POINT: &str = "7f82af304ad40acde2e14b4e02d4ed56";

    #[test]
    fn the_retail_bodies_read_and_write_back() {
        for (type_index, body) in [
            (UPDATE_WARBOARD_STATS_TYPE_INDEX, STATS),
            (UPDATE_WARBOARD_MANIFEST_TYPE_INDEX, MANIFEST_UPDATE),
            (SIEGE_BEGAN_TYPE_INDEX, POINT),
            (SIEGE_ENDED_TYPE_INDEX, POINT),
        ] {
            let push = WarServerMsg::decode(type_index, &hex(body)).expect("reads");
            assert_eq!(push.type_index(), type_index);
            assert!(WarServerMsg::is_type(type_index));
            assert_eq!(push.encode(), hex(body));
        }
        for (type_index, body) in [
            (WARBOARD_ENABLED_TYPE_INDEX, ENABLED),
            (WARBOARD_MANIFEST_RECEIVED_TYPE_INDEX, ENABLED),
            (STOP_SPECTATING_TYPE_INDEX, ENABLED),
        ] {
            let call = WarClientMsg::decode(type_index, &hex(body))
                .expect("a war call")
                .expect("reads");
            assert_eq!(call.type_index(), type_index);
            assert_eq!(call.encode(), hex(body));
        }
        let _ = WARBOARD;
    }

    #[test]
    fn a_full_manifest_lists_its_participants() {
        let header = FacetHeader {
            target: 1,
            entity: 2,
        };
        let id = [7; 16];
        let manifest = WarboardManifest::of_participants(vec![CharacterRef(Some(id))]);
        let push = WarServerMsg::full_manifest(header, &manifest).expect("builds");
        let WarServerMsg::UpdateFullWarboardManifest { payload, .. } = &push else {
            panic!("a full manifest");
        };
        assert_eq!(payload[..4], [0, 1, 0, MANIFEST_SECOND as u8]);
        assert_eq!(payload[4], 5, "a set character reference");
        assert_eq!(&payload[5..21], &id);
        let body = push.encode();
        assert_eq!(
            WarServerMsg::decode(UPDATE_FULL_WARBOARD_MANIFEST_TYPE_INDEX, &body),
            Ok(push)
        );
    }

    #[test]
    fn other_types_and_long_siege_bodies_are_refused() {
        assert!(WarClientMsg::decode(1, &[]).is_none());
        let mut long = hex(POINT);
        long.push(0);
        assert!(matches!(
            WarServerMsg::decode(SIEGE_BEGAN_TYPE_INDEX, &long),
            Err(MessageError::TrailingBytes { .. })
        ));
        assert!(matches!(
            WarServerMsg::decode(1, &hex(POINT)),
            Err(MessageError::BadTag { .. })
        ));
    }
}
