// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client's GM and debug calls that the server takes from a game master
//! (docs/re/2026-09-18-progression-and-gm-mode.md section 5). The client sends them only in its
//! GM mode, but it checks nothing itself: the server checks the sender's rank.
//!
//! Each body is the facet header and then the fields of the sender's message object in object
//! order (Inferred: no capture holds one). The decoder refuses any other length, so a wrong
//! reading is refused and logged with its body, never used.

use crate::facet::FacetHeader;
use crate::wire::{MessageError, Reader, Writer};

/// `PlayerComponentServerFacet_RequestGmPermissions`: no body; the local player sends it at its
/// start when the client is a GM client.
pub const REQUEST_GM_PERMISSIONS_TYPE_INDEX: u64 = 3956;
/// `ProgressionComponentServerFacet_Debug_SetPlayerLevel`: a `u32` level.
pub const DEBUG_SET_PLAYER_LEVEL_TYPE_INDEX: u64 = 3342;
/// `CurrencyComponentServerFacet_GmSetCurrency`: a `u64` amount.
pub const GM_SET_CURRENCY_TYPE_INDEX: u64 = 3783;
/// `CategoricalProgressionComponentServerFacet_DebugModifyProgression`: a `u8`, a `u64` amount
/// and a `u32` progression id.
pub const DEBUG_MODIFY_PROGRESSION_TYPE_INDEX: u64 = 4032;
/// `CategoricalProgressionComponentServerFacet_DebugSetProgressionRank`: a `u16` rank and a
/// `u32` progression id.
pub const DEBUG_SET_PROGRESSION_RANK_TYPE_INDEX: u64 = 43;
/// `LocalPlayerDebugComponentServerFacet_DebugTeleportPlayer`: 16 bytes (Inferred: x, y, z and
/// one more `f32`).
pub const DEBUG_TELEPORT_PLAYER_TYPE_INDEX: u64 = 3941;
/// `LocalPlayerDebugComponentServerFacet_DebugTeleportPlayerGM`: as 3941.
pub const DEBUG_TELEPORT_PLAYER_GM_TYPE_INDEX: u64 = 1033;

/// One GM or debug call of the client.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GmClientMsg {
    /// 3956: the client is in GM mode.
    RequestGmPermissions {
        /// The header.
        header: FacetHeader,
    },
    /// 3342: set the sender's level.
    SetPlayerLevel {
        /// The header.
        header: FacetHeader,
        /// The level (whether zero-based is Unknown; the server reads it zero-based, as 899).
        level: u32,
    },
    /// 3783: set the sender's coins.
    SetCurrency {
        /// The header.
        header: FacetHeader,
        /// The coins.
        amount: u64,
    },
    /// 4032: add experience to one of the sender's categorical progressions.
    ModifyProgression {
        /// The header.
        header: FacetHeader,
        /// A flag of Unknown meaning.
        flag: u8,
        /// The experience.
        amount: u64,
        /// The progression id (the CRC-32 of its lowercase `CategoricalProgressionId`).
        progression: u32,
    },
    /// 43: set the rank of one of the sender's categorical progressions.
    SetProgressionRank {
        /// The header.
        header: FacetHeader,
        /// The rank, zero-based.
        rank: u16,
        /// The progression id.
        progression: u32,
    },
    /// 3941 or 1033: move the sender.
    Teleport {
        /// The header.
        header: FacetHeader,
        /// East, metres.
        x: f32,
        /// North, metres.
        y: f32,
        /// Up, metres.
        z: f32,
        /// The fourth value (Unknown; perhaps the yaw).
        extra: f32,
    },
}

impl GmClientMsg {
    /// Decodes `body` when `type_index` is a GM call this server reads; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            REQUEST_GM_PERMISSIONS_TYPE_INDEX => {
                read(|header, _| Ok(Self::RequestGmPermissions { header }))
            }
            DEBUG_SET_PLAYER_LEVEL_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SetPlayerLevel {
                    header,
                    level: reader.u32()?,
                })
            }),
            GM_SET_CURRENCY_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SetCurrency {
                    header,
                    amount: reader.u64()?,
                })
            }),
            DEBUG_MODIFY_PROGRESSION_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ModifyProgression {
                    header,
                    flag: reader.u8()?,
                    amount: reader.u64()?,
                    progression: reader.u32()?,
                })
            }),
            DEBUG_SET_PROGRESSION_RANK_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SetProgressionRank {
                    header,
                    rank: reader.u16()?,
                    progression: reader.u32()?,
                })
            }),
            DEBUG_TELEPORT_PLAYER_TYPE_INDEX | DEBUG_TELEPORT_PLAYER_GM_TYPE_INDEX => {
                read(|header, reader| {
                    Ok(Self::Teleport {
                        header,
                        x: reader.f32()?,
                        y: reader.f32()?,
                        z: reader.f32()?,
                        extra: reader.f32()?,
                    })
                })
            }
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::RequestGmPermissions { .. } => REQUEST_GM_PERMISSIONS_TYPE_INDEX,
            Self::SetPlayerLevel { .. } => DEBUG_SET_PLAYER_LEVEL_TYPE_INDEX,
            Self::SetCurrency { .. } => GM_SET_CURRENCY_TYPE_INDEX,
            Self::ModifyProgression { .. } => DEBUG_MODIFY_PROGRESSION_TYPE_INDEX,
            Self::SetProgressionRank { .. } => DEBUG_SET_PROGRESSION_RANK_TYPE_INDEX,
            Self::Teleport { .. } => DEBUG_TELEPORT_PLAYER_TYPE_INDEX,
        }
    }

    /// The body as the client sends it (for tests and tools).
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::RequestGmPermissions { header } => header.write(&mut writer),
            Self::SetPlayerLevel { header, level } => {
                header.write(&mut writer);
                writer.u32(*level);
            }
            Self::SetCurrency { header, amount } => {
                header.write(&mut writer);
                writer.u64(*amount);
            }
            Self::ModifyProgression {
                header,
                flag,
                amount,
                progression,
            } => {
                header.write(&mut writer);
                writer.u8(*flag).u64(*amount).u32(*progression);
            }
            Self::SetProgressionRank {
                header,
                rank,
                progression,
            } => {
                header.write(&mut writer);
                writer.u16(*rank).u32(*progression);
            }
            Self::Teleport {
                header,
                x,
                y,
                z,
                extra,
            } => {
                header.write(&mut writer);
                writer.f32(*x).f32(*y).f32(*z).f32(*extra);
            }
        }
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: FacetHeader = FacetHeader {
        target: 0x0102_0304_0506_0708,
        entity: 0x0102_0304_0506_0700,
    };
    /// `GreatSwordAbilityTable`.
    const GREATSWORD: u32 = 0x180b_9a9f;

    #[test]
    fn every_call_round_trips_and_a_wrong_length_is_refused() {
        let calls = [
            GmClientMsg::RequestGmPermissions { header: HEADER },
            GmClientMsg::SetPlayerLevel {
                header: HEADER,
                level: 59,
            },
            GmClientMsg::SetCurrency {
                header: HEADER,
                amount: 1_000_000,
            },
            GmClientMsg::ModifyProgression {
                header: HEADER,
                flag: 1,
                amount: 500,
                progression: GREATSWORD,
            },
            GmClientMsg::SetProgressionRank {
                header: HEADER,
                rank: 19,
                progression: GREATSWORD,
            },
            GmClientMsg::Teleport {
                header: HEADER,
                x: 1.0,
                y: 2.0,
                z: 3.0,
                extra: 0.5,
            },
        ];
        for call in calls {
            let body = call.encode();
            assert_eq!(
                GmClientMsg::decode(call.type_index(), &body).expect("known"),
                Ok(call)
            );
            let mut longer = body.clone();
            longer.push(0);
            assert!(
                GmClientMsg::decode(call.type_index(), &longer)
                    .expect("known")
                    .is_err()
            );
        }
        assert!(
            GmClientMsg::decode(DEBUG_TELEPORT_PLAYER_GM_TYPE_INDEX, &calls[5].encode()).is_some()
        );
        assert!(GmClientMsg::decode(1, &[]).is_none());
    }
}
