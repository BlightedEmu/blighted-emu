// SPDX-License-Identifier: AGPL-3.0-or-later
//! The war declaration calls and answers of `SocialComponent` (wars and sieges study
//! 2026-11-12). No capture holds a declaration; the layouts are the client's writers and
//! readers.
//!
//! - 2920 `SocialComponentServerFacet_RequestDeclareWar` (writer `FUN_143ebaf80`): the defender
//!   company (16 bytes), the war cost the popup showed (u64, hundredths of a coin), the siege
//!   window it showed (4 bytes: the defender's `siegeWindow` start hour, an f32), the territory
//!   (u16), the war camp tier (u8, 0 to 2) and whether the company treasury pays (bool). Lua
//!   `wardeclarationpopup.luac` proto28 sends `RequestDeclareWar(owningGuildId, expectedWarCost,
//!   expectedSiegeWindow, territoryId, selectedCampId, walletSource == COMPANY)`.
//! - 941 `SocialComponentClientFacet_OnWarDeclarationFailed` (writer `FUN_143eae100`): 16 bytes,
//!   u16, 16 bytes, the cost (u64) and the siege window (4 bytes). The popup (proto29) compares the
//!   cost and the window with what it sent, shows `warcost_changed`, `siegewindow_changed`, both
//!   or `unknown`, and keeps the new values. Inferred: the ids are the defender and the declaring
//!   company, the u16 the territory.
//! - 1737 `SocialComponentClientFacet_OnWarRequestPending` (reader `FUN_143eabec0`): 16 bytes
//!   and a u32 (`ntohl`). Inferred: the defender and the client's `WarRequestStatus` (1 Pending,
//!   `FUN_145cc2d20`).
//! - 193 `SocialComponentClientFacet_OnWarRequestSuccessful` (`FUN_142959860`): 16 bytes.
//!   Inferred: the war id.

use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// `SocialComponentServerFacet_RequestDeclareWar`.
pub const DECLARE_WAR_TYPE_INDEX: u64 = 2920;
/// `SocialComponentClientFacet_OnWarDeclarationFailed`.
pub const DECLARATION_FAILED_TYPE_INDEX: u64 = 941;
/// `SocialComponentClientFacet_OnWarRequestPending`.
pub const REQUEST_PENDING_TYPE_INDEX: u64 = 1737;
/// `SocialComponentClientFacet_OnWarRequestSuccessful`.
pub const REQUEST_SUCCESSFUL_TYPE_INDEX: u64 = 193;
/// The client's `WarRequestStatus` `Pending` (`FUN_145cc2d20`).
pub const WAR_REQUEST_PENDING: u32 = 1;

/// The war calls of the client.
pub(crate) const CALL_TYPES: [u64; 1] = [DECLARE_WAR_TYPE_INDEX];

/// A war call of the client.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WarCall {
    /// 2920: declare war on the owner of a territory.
    Declare {
        /// The defending company (the territory owner the popup showed).
        defender: [u8; RAW_ID_LEN],
        /// The cost the popup showed, hundredths of a coin.
        cost: u64,
        /// The defender's siege window start hour the popup showed.
        siege_window: f32,
        /// The territory id.
        territory: u16,
        /// The war camp tier, 0 to 2.
        camp: u8,
        /// The company treasury pays; else the player's wallet.
        from_treasury: bool,
    },
}

impl WarCall {
    pub(crate) fn read(
        type_index: u64,
        reader: &mut Reader<'_>,
    ) -> Option<Result<Self, MessageError>> {
        (type_index == DECLARE_WAR_TYPE_INDEX).then(|| {
            Ok(Self::Declare {
                defender: reader.raw_id()?,
                cost: reader.u64()?,
                siege_window: reader.f32()?,
                territory: reader.u16()?,
                camp: reader.u8()?,
                from_treasury: reader.boolean()?,
            })
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        DECLARE_WAR_TYPE_INDEX
    }

    /// The call's arguments after the facet header, as the client writes them.
    pub(crate) fn write(&self, writer: &mut Writer) {
        let Self::Declare {
            defender,
            cost,
            siege_window,
            territory,
            camp,
            from_treasury,
        } = self;
        writer
            .bytes(defender)
            .u64(*cost)
            .f32(*siege_window)
            .u16(*territory)
            .u8(*camp)
            .boolean(*from_treasury);
    }
}

/// A war answer of the service.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WarPush {
    /// 941: the declaration failed; the popup shows the cost and window the service holds.
    DeclarationFailed {
        /// The defending company.
        defender: [u8; RAW_ID_LEN],
        /// The territory id.
        territory: u16,
        /// The declaring company.
        company: [u8; RAW_ID_LEN],
        /// The cost, hundredths of a coin.
        cost: u64,
        /// The defender's siege window start hour.
        siege_window: f32,
    },
    /// 1737: the declaration is in the lottery.
    Pending {
        /// The defending company.
        defender: [u8; RAW_ID_LEN],
        /// The `WarRequestStatus`.
        status: u32,
    },
    /// 193: the declaration won its lottery.
    Successful {
        /// The war id.
        war: [u8; RAW_ID_LEN],
    },
}

impl WarPush {
    /// The type index of the push.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::DeclarationFailed { .. } => DECLARATION_FAILED_TYPE_INDEX,
            Self::Pending { .. } => REQUEST_PENDING_TYPE_INDEX,
            Self::Successful { .. } => REQUEST_SUCCESSFUL_TYPE_INDEX,
        }
    }

    pub(crate) fn write(&self, writer: &mut Writer) {
        match self {
            Self::DeclarationFailed {
                defender,
                territory,
                company,
                cost,
                siege_window,
            } => {
                writer
                    .bytes(defender)
                    .u16(*territory)
                    .bytes(company)
                    .u64(*cost)
                    .f32(*siege_window);
            }
            Self::Pending { defender, status } => {
                writer.bytes(defender).u32(*status);
            }
            Self::Successful { war } => {
                writer.bytes(war);
            }
        }
    }

    pub(crate) fn read(
        type_index: u64,
        reader: &mut Reader<'_>,
    ) -> Option<Result<Self, MessageError>> {
        let push = match type_index {
            DECLARATION_FAILED_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::DeclarationFailed {
                    defender: reader.raw_id()?,
                    territory: reader.u16()?,
                    company: reader.raw_id()?,
                    cost: reader.u64()?,
                    siege_window: reader.f32()?,
                })
            })(),
            REQUEST_PENDING_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::Pending {
                    defender: reader.raw_id()?,
                    status: reader.u32()?,
                })
            })(),
            REQUEST_SUCCESSFUL_TYPE_INDEX => reader.raw_id().map(|war| Self::Successful { war }),
            _ => return None,
        };
        Some(push)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facet::FacetHeader;
    use crate::social::{SocialCall, SocialPush};

    const DEFENDER: [u8; RAW_ID_LEN] = [7; RAW_ID_LEN];
    const COMPANY: [u8; RAW_ID_LEN] = [9; RAW_ID_LEN];
    const COST: u64 = 100_000;
    const SIEGE_HOUR: f32 = 18.0;
    const TERRITORY: u16 = 6;

    fn header() -> FacetHeader {
        crate::social::SocialFacet::Social.header(1)
    }

    #[test]
    fn a_declaration_reads_in_the_client_writer_order() {
        let call = WarCall::Declare {
            defender: DEFENDER,
            cost: COST,
            siege_window: SIEGE_HOUR,
            territory: TERRITORY,
            camp: 2,
            from_treasury: true,
        };
        let body = SocialCall::War(call).encode(header()).expect("encode");
        let args = &body[body.len() - (RAW_ID_LEN + 8 + 4 + 2 + 1 + 1)..];
        assert_eq!(&args[..RAW_ID_LEN], &DEFENDER);
        assert_eq!(&args[RAW_ID_LEN..RAW_ID_LEN + 8], &COST.to_be_bytes());
        assert_eq!(
            args[args.len() - 2..],
            [2, 1],
            "camp, then the treasury flag"
        );
        let (_, read) = SocialCall::decode(DECLARE_WAR_TYPE_INDEX, &body)
            .expect("a social call")
            .expect("decode");
        assert_eq!(read, SocialCall::War(call));
    }

    #[test]
    fn every_answer_writes_and_reads_back() {
        for push in [
            WarPush::DeclarationFailed {
                defender: DEFENDER,
                territory: TERRITORY,
                company: COMPANY,
                cost: COST,
                siege_window: SIEGE_HOUR,
            },
            WarPush::Pending {
                defender: DEFENDER,
                status: WAR_REQUEST_PENDING,
            },
            WarPush::Successful { war: COMPANY },
        ] {
            let social = SocialPush::War(push);
            assert_eq!(social.facet(), crate::social::SocialFacet::Social);
            let args = social.encode_args().expect("encode");
            let read = SocialPush::decode_args(push.type_index(), &args)
                .expect("a war push")
                .expect("decode");
            assert_eq!(read, social);
        }
        assert_eq!(
            WarPush::Pending {
                defender: DEFENDER,
                status: WAR_REQUEST_PENDING
            }
            .type_index(),
            REQUEST_PENDING_TYPE_INDEX
        );
    }
}
