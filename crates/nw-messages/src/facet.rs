// SPDX-License-Identifier: AGPL-3.0-or-later
//! Facet calls: messages to or from one component of the player's entity (tutorial-intro note,
//! sections 0 and 2).
//!
//! Every facet body starts with two big-endian `u64`s: the target id, then the entity. The entity
//! is the first 8 bytes of the `ClientAddEntryMsg` entry id in reverse byte order. The target id
//! is the entity plus a constant of the component (the same in separate retail sessions); the
//! client looks the target id up and drops a call whose target it does not know.

use crate::level_info::PhasingEntry;
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// The target-id constant of each component of the player entity (Soul Survivor session).
pub mod component {
    /// `TimeComponent`.
    pub const TIME: u64 = 0xd41e_b32c_0f15_4004;
    /// `PlayerComponent`.
    pub const PLAYER: u64 = 0xdf0c_e8c9_8d27_619d;
    /// `PlayerCutsceneComponent`.
    pub const PLAYER_CUTSCENE: u64 = 0xd499_3151_0e1d_d23f;
    /// `GridTrackingComponent`.
    pub const GRID_TRACKING: u64 = 0x3946_bc0c_0a40_83da;
}

/// Type index of `PlayerComponentServerFacet_OnAckLevelInfoChanged`.
pub const ACK_LEVEL_INFO_TYPE_INDEX: u64 = 5181;
/// Type index of `PlayerCutsceneComponentServerFacet_NotifyCinematicEnded`.
pub const CINEMATIC_ENDED_TYPE_INDEX: u64 = 6542;
/// Type index of `PlayerCutsceneComponentServerFacet_OnClientRequestStartCutscene`.
pub const REQUEST_CUTSCENE_TYPE_INDEX: u64 = 6703;
/// Type index of `PlayerCutsceneComponentServerFacet_OnClientCutsceneEnded`.
pub const CUTSCENE_ENDED_TYPE_INDEX: u64 = 6705;
/// Type index of `PlayerComponentServerFacet_ReturnToMainWorld`.
pub const RETURN_TO_MAIN_WORLD_TYPE_INDEX: u64 = 502;
/// Type index of `GridTrackingComponentClientFacet_UpdatePhasing`.
pub const UPDATE_PHASING_TYPE_INDEX: u64 = 6310;
/// Type index of `PlayerCutsceneComponentClientFacet_OnServerCutsceneRequestGranted`.
pub const CUTSCENE_GRANTED_TYPE_INDEX: u64 = 6707;
/// Type index of `PlayerCutsceneComponentClientFacet_OnServerCutsceneEndRequestFinalized`.
pub const CUTSCENE_END_FINALIZED_TYPE_INDEX: u64 = 6709;
/// Type index of `PlayerComponentClientFacet_OnTeleportWithLoadingScreen`.
pub const TELEPORT_WITH_LOADING_SCREEN_TYPE_INDEX: u64 = 3951;

/// The two ids that start a facet body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FacetHeader {
    /// The target id: the entity plus the component constant.
    pub target: u64,
    /// The entity.
    pub entity: u64,
}

impl FacetHeader {
    /// The header of a call to `component` of the player whose entry id names entity number
    /// `entity_number` (`ClientAddEntryMsg::for_entity`).
    pub const fn of(component: u64, entity_number: u64) -> Self {
        // The entry id starts with the big-endian entity number; the facet entity is those
        // bytes in reverse order.
        let entity = entity_number.swap_bytes();
        Self {
            target: entity.wrapping_add(component),
            entity,
        }
    }

    /// Reads the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] when the body is shorter than the header.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            target: reader.u64()?,
            entity: reader.u64()?,
        })
    }

    /// Writes the header.
    pub fn write(&self, writer: &mut Writer) {
        writer.u64(self.target).u64(self.entity);
    }
}

/// A facet call from the client that the server answers or acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientFacetMsg {
    /// 5181: the client loaded the level of a `LevelInfoChangedMsg` with this client context.
    AckLevelInfo {
        /// The header.
        header: FacetHeader,
        /// The acknowledged client context.
        client_context: u8,
    },
    /// 6542: a cinematic video ended; its id is the CRC-32 of its lowercase entry id.
    CinematicEnded {
        /// The header.
        header: FacetHeader,
        /// The cinematic.
        cinematic: u32,
    },
    /// 6703: the client asks to start a cutscene; its id is the CRC-32 of its lowercase name.
    RequestCutscene {
        /// The header.
        header: FacetHeader,
        /// The cutscene.
        cutscene: u32,
    },
    /// 6705: the client's cutscene ended.
    CutsceneEnded {
        /// The header.
        header: FacetHeader,
    },
    /// 502: the client asks to leave the tutorial for the open world.
    ReturnToMainWorld {
        /// The header.
        header: FacetHeader,
        /// A byte of unknown meaning (0 in the capture).
        flag: u8,
    },
}

impl ClientFacetMsg {
    /// Decodes `body` when `type_index` is a call this server reads; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            ACK_LEVEL_INFO_TYPE_INDEX => read(|header, reader| {
                Ok(Self::AckLevelInfo {
                    header,
                    client_context: reader.u8()?,
                })
            }),
            CINEMATIC_ENDED_TYPE_INDEX => read(|header, reader| {
                Ok(Self::CinematicEnded {
                    header,
                    cinematic: reader.u32()?,
                })
            }),
            REQUEST_CUTSCENE_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestCutscene {
                    header,
                    cutscene: reader.u32()?,
                })
            }),
            CUTSCENE_ENDED_TYPE_INDEX => read(|header, _| Ok(Self::CutsceneEnded { header })),
            RETURN_TO_MAIN_WORLD_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ReturnToMainWorld {
                    header,
                    flag: reader.u8()?,
                })
            }),
            _ => return None,
        })
    }
}

/// 6310: the phases of the player's sectors (the entries of its `LevelInfoChangedMsg`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePhasingMsg {
    /// The header.
    pub header: FacetHeader,
    /// The entries.
    pub entries: Vec<PhasingEntry>,
    /// A byte of unknown meaning: 3 in the tutorial, 2 in the open world, 0 in an empty form.
    pub mode: u8,
    /// Rises with each update; the client acknowledges it (6745).
    pub sequence: u8,
}

impl BodyCodec for UpdatePhasingMsg {
    const TYPE_INDEX: u64 = UPDATE_PHASING_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        // A `usize` count fits 64 bits.
        writer.wide_prefixed(self.entries.len() as u64);
        for entry in &self.entries {
            entry.write(&mut writer);
        }
        Ok(writer.u8(self.mode).u8(self.sequence).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let count = reader.wide_prefixed()?;
        let mut entries = Vec::new();
        for _ in 0..count {
            entries.push(PhasingEntry::read(&mut reader)?);
        }
        let message = Self {
            header,
            entries,
            mode: reader.u8()?,
            sequence: reader.u8()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 6707: the server allows the cutscene the client asked for; it echoes the asked id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutsceneGrantedMsg {
    /// The header.
    pub header: FacetHeader,
    /// The cutscene id of the request.
    pub cutscene: u32,
}

impl BodyCodec for CutsceneGrantedMsg {
    const TYPE_INDEX: u64 = CUTSCENE_GRANTED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.u32(self.cutscene).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            cutscene: reader.u32()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 6709: the server closes the cutscene that the client ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutsceneEndFinalizedMsg {
    /// The header.
    pub header: FacetHeader,
}

impl BodyCodec for CutsceneEndFinalizedMsg {
    const TYPE_INDEX: u64 = CUTSCENE_END_FINALIZED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 3951: the client shows its loading screen for a level change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeleportWithLoadingScreenMsg {
    /// The header.
    pub header: FacetHeader,
    /// A flag (1 in the capture).
    pub flag: bool,
}

impl BodyCodec for TeleportWithLoadingScreenMsg {
    const TYPE_INDEX: u64 = TELEPORT_WITH_LOADING_SCREEN_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.boolean(self.flag).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            flag: reader.boolean()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Soul Survivor session: its facet entity and its 6310 body (tutorial phase).
    const SOUL_ENTITY: u64 = 0x650f_7a09_cf50_b550;
    const SOUL_ACTION_LIST_TARGET: u64 = 0xa168_9458_7313_62c5;
    const ACTION_LIST: u64 = 0x3c59_1a4e_a3c2_ad75;
    const SOUL_PHASING: &str = concat!(
        "9e563615d991392a650f7a09cf50b550",
        "01",
        "03",
        "00000000",
        "8b2cbbb86aa99537",
        "00000002",
        "03",
        "01"
    );
    const SOUL_GRANT: &str = "39a8ab5add6e878f650f7a09cf50b550a81bd1af";

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    fn soul(component: u64) -> FacetHeader {
        // The entity number whose entry id reverses to the Soul entity.
        FacetHeader::of(component, SOUL_ENTITY.swap_bytes())
    }

    #[test]
    fn the_target_is_the_entity_plus_the_component() {
        let header = soul(ACTION_LIST);
        assert_eq!(header.entity, SOUL_ENTITY);
        assert_eq!(header.target, SOUL_ACTION_LIST_TARGET);
    }

    #[test]
    fn phasing_and_grant_are_the_retail_bytes() {
        let phasing = UpdatePhasingMsg {
            header: soul(component::GRID_TRACKING),
            entries: vec![PhasingEntry {
                key_type: 3,
                key_id: 0,
                phase_id: 0x8b2c_bbb8_6aa9_9537,
                extra: 2,
            }],
            mode: 3,
            sequence: 1,
        };
        let body = phasing.encode().expect("encode");
        assert_eq!(body, hex(SOUL_PHASING));
        assert_eq!(UpdatePhasingMsg::decode(&body), Ok(phasing));
        let grant = CutsceneGrantedMsg {
            header: soul(component::PLAYER_CUTSCENE),
            cutscene: 0xa81b_d1af,
        };
        assert_eq!(grant.encode().expect("encode"), hex(SOUL_GRANT));
    }

    #[test]
    fn client_calls_decode_and_others_are_left() {
        let mut body = Vec::new();
        let mut writer = Writer::new();
        soul(component::PLAYER).write(&mut writer);
        body.extend(writer.u8(1).finish());
        assert_eq!(
            ClientFacetMsg::decode(ACK_LEVEL_INFO_TYPE_INDEX, &body),
            Some(Ok(ClientFacetMsg::AckLevelInfo {
                header: soul(component::PLAYER),
                client_context: 1,
            }))
        );
        assert!(matches!(
            ClientFacetMsg::decode(CUTSCENE_ENDED_TYPE_INDEX, &body),
            Some(Err(MessageError::TrailingBytes { .. }))
        ));
        assert_eq!(
            ClientFacetMsg::decode(UPDATE_PHASING_TYPE_INDEX, &body),
            None
        );
    }
}
