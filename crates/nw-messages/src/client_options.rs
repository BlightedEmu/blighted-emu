// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client's option calls and its phasing acknowledgement (run-15 core study; layouts from
//! the retail captures `newchar-retail-2026-09-18` and `newchar-linux-2026-09-12`, names from the
//! Azoth catalog). Each body is the facet header and one value:
//!
//! | Call | Value | Retail |
//! |---|---|---|
//! | 2515 `PlayerSettingsComponentServerFacet_ReceiveSettingsFromClient` | settings bits, `u32` | once per spawn |
//! | 4272 `TraversalComponentServerFacet_SyncIsAutoTraversalEnabled` | boolean | once per spawn |
//! | 4122 `SpectatedPlayerComponentServerFacet_SetIsPublic` | boolean | once per spawn |
//! | 3967 `PlayerComponentServerFacet_OnCullLimitPctChangeRequest` | percent, `u8` | at spawn, then as the frame rate changes (100, 96, 90, 83, 76, 70) |
//! | 1094 `PlayerComponentServerFacet_OnSetBandwidthModeRequest` | mode, `u32` | 12 times in the first second, then now and then |
//! | 4854 `PlayerComponentServerFacet_OnTelemetryOptionsDataChanged` | options block | once per spawn |
//! | 6745 `GridTrackingComponentServerFacet_ClientAckPhasingUpdate` | phasing sequence, `u8` | after each 6310, with its sequence |
//!
//! None of them has an answer in the captures.

use crate::facet::FacetHeader;
use crate::wire::{MessageError, Reader, Writer};

/// `PlayerSettingsComponentServerFacet_ReceiveSettingsFromClient`.
pub const RECEIVE_SETTINGS_TYPE_INDEX: u64 = 2515;
/// `TraversalComponentServerFacet_SyncIsAutoTraversalEnabled`.
pub const SYNC_AUTO_TRAVERSAL_TYPE_INDEX: u64 = 4272;
/// `SpectatedPlayerComponentServerFacet_SetIsPublic`.
pub const SET_IS_PUBLIC_TYPE_INDEX: u64 = 4122;
/// `PlayerComponentServerFacet_OnCullLimitPctChangeRequest`.
pub const CULL_LIMIT_TYPE_INDEX: u64 = 3967;
/// `PlayerComponentServerFacet_OnSetBandwidthModeRequest`.
pub const BANDWIDTH_MODE_TYPE_INDEX: u64 = 1094;
/// `PlayerComponentServerFacet_OnTelemetryOptionsDataChanged`.
pub const TELEMETRY_OPTIONS_TYPE_INDEX: u64 = 4854;
/// `GridTrackingComponentServerFacet_ClientAckPhasingUpdate`.
pub const ACK_PHASING_TYPE_INDEX: u64 = 6745;
/// The highest cull limit: the client draws every player it may (retail sends 100 at spawn).
pub const FULL_CULL_LIMIT_PERCENT: u8 = 100;

/// One option call of the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientOptionMsg {
    /// 2515: the player settings as one bit set (retail `0x97c9` on both logins of one
    /// character; the meaning of each bit is Unknown).
    Settings {
        /// The header (the player's `PlayerSettingsComponent`).
        header: FacetHeader,
        /// The settings bits.
        bits: u32,
    },
    /// 4272: whether the client climbs and vaults on its own.
    AutoTraversal {
        /// The header (the player's `TraversalComponent`).
        header: FacetHeader,
        /// The option is on.
        enabled: bool,
    },
    /// 4122: whether other players may spectate this player.
    SpectatePublic {
        /// The header (the player's `SpectatedPlayerComponent`).
        header: FacetHeader,
        /// The option is on.
        public: bool,
    },
    /// 3967: the share of other players the client draws, in percent.
    CullLimit {
        /// The header (the player's `PlayerComponent`).
        header: FacetHeader,
        /// The percent, at most [`FULL_CULL_LIMIT_PERCENT`].
        percent: u8,
    },
    /// 1094: the network mode the client asks for (retail 1 and 2; their meaning is Unknown).
    BandwidthMode {
        /// The header (the player's `PlayerComponent`).
        header: FacetHeader,
        /// The mode.
        mode: u32,
    },
    /// 4854: the client's telemetry options. The block holds option flags and the key names
    /// of its key bindings (`@cc_f3`, `@cc_tab`, ...); its layout is Unknown, so it stays raw.
    TelemetryOptions {
        /// The header (the player's `PlayerComponent`).
        header: FacetHeader,
        /// The options block.
        options: Vec<u8>,
    },
    /// 6745: the client applied the phasing update with this sequence (6310's last byte).
    PhasingAck {
        /// The header (the player's `GridTrackingComponent`).
        header: FacetHeader,
        /// The sequence of the update.
        sequence: u8,
    },
}

impl ClientOptionMsg {
    /// Decodes `body` when `type_index` is an option call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            RECEIVE_SETTINGS_TYPE_INDEX => read(|header, reader| {
                Ok(Self::Settings {
                    header,
                    bits: reader.u32()?,
                })
            }),
            SYNC_AUTO_TRAVERSAL_TYPE_INDEX => read(|header, reader| {
                Ok(Self::AutoTraversal {
                    header,
                    enabled: reader.boolean()?,
                })
            }),
            SET_IS_PUBLIC_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SpectatePublic {
                    header,
                    public: reader.boolean()?,
                })
            }),
            CULL_LIMIT_TYPE_INDEX => read(|header, reader| {
                let percent = reader.u8()?;
                if percent > FULL_CULL_LIMIT_PERCENT {
                    return Err(MessageError::CountTooLarge {
                        count: u64::from(percent),
                        limit: u64::from(FULL_CULL_LIMIT_PERCENT),
                    });
                }
                Ok(Self::CullLimit { header, percent })
            }),
            BANDWIDTH_MODE_TYPE_INDEX => read(|header, reader| {
                Ok(Self::BandwidthMode {
                    header,
                    mode: reader.u32()?,
                })
            }),
            TELEMETRY_OPTIONS_TYPE_INDEX => read(|header, reader| {
                let options = reader.rest().to_vec();
                reader.bytes(options.len())?;
                Ok(Self::TelemetryOptions { header, options })
            }),
            ACK_PHASING_TYPE_INDEX => read(|header, reader| {
                Ok(Self::PhasingAck {
                    header,
                    sequence: reader.u8()?,
                })
            }),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::Settings { .. } => RECEIVE_SETTINGS_TYPE_INDEX,
            Self::AutoTraversal { .. } => SYNC_AUTO_TRAVERSAL_TYPE_INDEX,
            Self::SpectatePublic { .. } => SET_IS_PUBLIC_TYPE_INDEX,
            Self::CullLimit { .. } => CULL_LIMIT_TYPE_INDEX,
            Self::BandwidthMode { .. } => BANDWIDTH_MODE_TYPE_INDEX,
            Self::TelemetryOptions { .. } => TELEMETRY_OPTIONS_TYPE_INDEX,
            Self::PhasingAck { .. } => ACK_PHASING_TYPE_INDEX,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::Settings { header, bits } => {
                header.write(&mut writer);
                writer.u32(*bits);
            }
            Self::AutoTraversal {
                header,
                enabled: flag,
            }
            | Self::SpectatePublic {
                header,
                public: flag,
            } => {
                header.write(&mut writer);
                writer.boolean(*flag);
            }
            Self::CullLimit {
                header,
                percent: value,
            }
            | Self::PhasingAck {
                header,
                sequence: value,
            } => {
                header.write(&mut writer);
                writer.u8(*value);
            }
            Self::BandwidthMode { header, mode } => {
                header.write(&mut writer);
                writer.u32(*mode);
            }
            Self::TelemetryOptions { header, options } => {
                header.write(&mut writer);
                writer.bytes(options);
            }
        }
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// Retail newchar 2026-09-18 bodies at the first spawn (header of each component, then the
    /// value), and the phasing acknowledgement of the first open-world 6310.
    const RETAIL: [(u64, &str); 6] = [
        (
            RECEIVE_SETTINGS_TYPE_INDEX,
            "393a153d6c4baa642715f9bca18fdece000097c9",
        ),
        (
            SYNC_AUTO_TRAVERSAL_TYPE_INDEX,
            "901cf0228c03bb802715f9bca18fdece00",
        ),
        (
            SET_IS_PUBLIC_TYPE_INDEX,
            "fd36707b8a87d2852715f9bca18fdece00",
        ),
        (CULL_LIMIT_TYPE_INDEX, "0622e2862eb7406b2715f9bca18fdece64"),
        (
            BANDWIDTH_MODE_TYPE_INDEX,
            "0622e2862eb7406b2715f9bca18fdece00000001",
        ),
        (ACK_PHASING_TYPE_INDEX, "605cb5c8abd062a82715f9bca18fdece01"),
    ];
    /// The settings bits of the retail 2515.
    const RETAIL_SETTINGS: u32 = 0x97c9;
    /// The bandwidth mode of the retail 1094.
    const RETAIL_BANDWIDTH_MODE: u32 = 1;
    /// The first bytes of the retail 4854 options block.
    const TELEMETRY_START: &str = "0622e2862eb7406b2715f9bca18fdece01000101010001000100000000";

    fn decode(type_index: u64, body: &[u8]) -> ClientOptionMsg {
        ClientOptionMsg::decode(type_index, body)
            .expect("an option call")
            .expect("decodes")
    }

    #[test]
    fn retail_option_calls_decode_and_encode_byte_exact() {
        for (type_index, body) in RETAIL {
            let body = hex(body);
            let message = decode(type_index, &body);
            assert_eq!(message.type_index(), type_index);
            assert_eq!(message.encode(), body, "type {type_index}");
        }
        let settings = decode(RECEIVE_SETTINGS_TYPE_INDEX, &hex(RETAIL[0].1));
        assert!(matches!(
            settings,
            ClientOptionMsg::Settings {
                bits: RETAIL_SETTINGS,
                ..
            }
        ));
        let cull = decode(CULL_LIMIT_TYPE_INDEX, &hex(RETAIL[3].1));
        assert!(matches!(
            cull,
            ClientOptionMsg::CullLimit {
                percent: FULL_CULL_LIMIT_PERCENT,
                ..
            }
        ));
        let mode = decode(BANDWIDTH_MODE_TYPE_INDEX, &hex(RETAIL[4].1));
        assert!(matches!(
            mode,
            ClientOptionMsg::BandwidthMode {
                mode: RETAIL_BANDWIDTH_MODE,
                ..
            }
        ));
        let ack = decode(ACK_PHASING_TYPE_INDEX, &hex(RETAIL[5].1));
        assert!(matches!(
            ack,
            ClientOptionMsg::PhasingAck { sequence: 1, .. }
        ));
    }

    #[test]
    fn the_telemetry_block_stays_raw() {
        let body = hex(TELEMETRY_START);
        let message = decode(TELEMETRY_OPTIONS_TYPE_INDEX, &body);
        let ClientOptionMsg::TelemetryOptions { options, .. } = &message else {
            panic!("telemetry options: {message:?}");
        };
        assert_eq!(options.as_slice(), &body[RAW_HEADER_LEN..]);
        assert_eq!(message.encode(), body);
    }

    /// Bytes of a facet header.
    const RAW_HEADER_LEN: usize = 16;

    #[test]
    fn bad_bodies_are_refused_and_other_types_are_not_options() {
        let mut long = hex(RETAIL[0].1);
        long.push(0);
        assert!(matches!(
            ClientOptionMsg::decode(RECEIVE_SETTINGS_TYPE_INDEX, &long),
            Some(Err(MessageError::TrailingBytes { .. }))
        ));
        let mut over_full = hex(RETAIL[3].1);
        *over_full.last_mut().expect("value") = FULL_CULL_LIMIT_PERCENT + 1;
        assert!(matches!(
            ClientOptionMsg::decode(CULL_LIMIT_TYPE_INDEX, &over_full),
            Some(Err(MessageError::CountTooLarge { .. }))
        ));
        let mut not_bool = hex(RETAIL[1].1);
        *not_bool.last_mut().expect("value") = 2;
        assert!(matches!(
            ClientOptionMsg::decode(SYNC_AUTO_TRAVERSAL_TYPE_INDEX, &not_bool),
            Some(Err(_))
        ));
        assert_eq!(
            ClientOptionMsg::decode(crate::RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX, &[]),
            None
        );
    }
}
