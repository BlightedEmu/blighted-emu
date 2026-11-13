// SPDX-License-Identifier: AGPL-3.0-or-later
//! The governor's desk calls of the player's `TerritoryInteractorComponent` (territory upkeep
//! study 2026-11-11, ledgers `pay-territory-tax`, `territory-status`, `MSQ6-lv28`).
//!
//! - 2722 `TerritoryInteractorComponentClientFacet_OnServerFacetReady` (server): the facet
//!   header, a big-endian `u16` territory and a `u32`. Observed: retail sends it 3 to 6 ms after
//!   it accepted the use of a governor's desk (3743, kind 54 `@ui_governorsdesk`) or of a
//!   territory planning board (kind 55), with the territory of the desk (6 at 1783335279058,
//!   2 at 1782906884743 and 1782906921588, 4 at 1789491648861). The `u32` was 2 for a governor
//!   and a consul of a company (their `guildRank` 0 and 1 in the 3147 of those ledgers) and 0 for
//!   a player in no company ([`ACCESS_OF_A_MEMBER`], [`ACCESS_OF_NO_MEMBER`]; meaning Inferred).
//! - 2738 `TerritoryInteractorComponentServerFacet_RequestPayUpkeepCost` (client): the header, a
//!   `u16` territory, a flag byte and a `u64` cost (client signature `RequestPayUpkeepCost(
//!   ranged_int<u16>, bool, ranged_int<u64, 0, 10000000000>)`, `FUN_14481ace0`). Observed once:
//!   `0006 01 00000000000186a0` at 1783335287110: territory 6, flag 1 (meaning Unknown), cost
//!   100,000 (`UpkeepCost` 1,000,000 x `UpkeepModifier` 0.1). Retail sent no answer message.
//! - 453 `TerritoryInteractorComponentServerFacet_RequestStartProject` (client): the header, a
//!   `u32` project (`Crc32`), a flag byte and a second `u32` (client signature
//!   `RequestStartProject(Crc32 const&, bool, Crc32 const&)`, reader `FUN_1447e35f0`). The Lua
//!   (`territoryplanning.luac` proto21) sends `RequestStartProject(projectId, not
//!   useCompanyWallet)`: the flag is "pay from the own wallet". It never sets the second `u32`
//!   (meaning Unknown). No capture holds a 453.
//! - 2739 `TerritoryInteractorComponentServerFacet_RequestCancelProject` (client): the header and
//!   a `u32` project (reader `FUN_1426ffc00`; Lua proto20 `RequestCancelProject(toCancelProjectId)`).
//!   No capture holds a 2739.
//!
//! The header's target is the player's main entity plus [`TERRITORY_INTERACTOR_COMPONENT`]
//! (Observed: the same difference for the two players of these ledgers).

use crate::facet::FacetHeader;
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// The component constant of the player's `TerritoryInteractorComponent` (target less entity in
/// every 2722 and 2738 header of the captures).
pub const TERRITORY_INTERACTOR_COMPONENT: u64 = 0x845a_cb4d_c4e6_3ff6;
/// Type index of `TerritoryInteractorComponentClientFacet_OnServerFacetReady`.
pub const TERRITORY_FACET_READY_TYPE_INDEX: u64 = 2722;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestPayUpkeepCost`.
pub const PAY_UPKEEP_COST_TYPE_INDEX: u64 = 2738;
/// The 2722 value for a player of a company (Observed: a governor and a consul).
pub const ACCESS_OF_A_MEMBER: u32 = 2;
/// The 2722 value for a player in no company (Observed).
pub const ACCESS_OF_NO_MEMBER: u32 = 0;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestStartProject`.
pub const START_PROJECT_TYPE_INDEX: u64 = 453;
/// Type index of `TerritoryInteractorComponentServerFacet_RequestCancelProject`.
pub const CANCEL_PROJECT_TYPE_INDEX: u64 = 2739;
/// The highest cost the client sends (its `ranged_int<u64, 0, 10000000000>`).
pub const MAX_UPKEEP_COST: u64 = 10_000_000_000;

/// 2722: the desk's territory facet is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerritoryFacetReadyMsg {
    /// The header (the player's `TerritoryInteractorComponent`).
    pub header: FacetHeader,
    /// The territory of the desk.
    pub territory: u16,
    /// [`ACCESS_OF_A_MEMBER`] or [`ACCESS_OF_NO_MEMBER`].
    pub access: u32,
}

impl TerritoryFacetReadyMsg {
    /// The 2722 of player entity number `number` for `territory`.
    pub fn of(number: u64, territory: u16, access: u32) -> Self {
        Self {
            header: FacetHeader::of(TERRITORY_INTERACTOR_COMPONENT, number),
            territory,
            access,
        }
    }
}

impl BodyCodec for TerritoryFacetReadyMsg {
    const TYPE_INDEX: u64 = TERRITORY_FACET_READY_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.u16(self.territory).u32(self.access).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            territory: reader.u16()?,
            access: reader.u32()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 2738: pay the upkeep of a territory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayUpkeepCostMsg {
    /// The header (the player's `TerritoryInteractorComponent`).
    pub header: FacetHeader,
    /// The territory.
    pub territory: u16,
    /// The flag (1 in the one retail call; meaning Unknown).
    pub flag: bool,
    /// The cost the client shows, hundredths of a coin.
    pub cost: u64,
}

impl PayUpkeepCostMsg {
    /// Decodes `body` when `type_index` is 2738; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        (type_index == PAY_UPKEEP_COST_TYPE_INDEX).then(|| {
            let mut reader = Reader::new(body);
            let message = Self {
                header: FacetHeader::read(&mut reader)?,
                territory: reader.u16()?,
                flag: reader.boolean()?,
                cost: reader.u64()?,
            };
            reader.finish()?;
            if message.cost > MAX_UPKEEP_COST {
                return Err(MessageError::CountTooLarge {
                    count: message.cost,
                    limit: MAX_UPKEEP_COST,
                });
            }
            Ok(message)
        })
    }

    /// The body (the client's form).
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer
            .u16(self.territory)
            .boolean(self.flag)
            .u64(self.cost)
            .finish()
    }
}

/// A town project call of the territory planning board (453, 2739).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TownProjectCallMsg {
    /// 453: start a project.
    Start {
        /// The header (the player's `TerritoryInteractorComponent`).
        header: FacetHeader,
        /// The CRC-32 of the lowercase `ProjectId`.
        project: u32,
        /// Pay from the player's own wallet (else from the company treasury).
        own_wallet: bool,
        /// The second `Crc32` (meaning Unknown; the Lua never sets it).
        second: u32,
    },
    /// 2739: cancel the running project.
    Cancel {
        /// The header (the player's `TerritoryInteractorComponent`).
        header: FacetHeader,
        /// The CRC-32 of the lowercase `ProjectId`.
        project: u32,
    },
}

impl TownProjectCallMsg {
    /// Decodes `body` when `type_index` is 453 or 2739; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            let header = FacetHeader::read(reader)?;
            let project = reader.u32()?;
            if type_index == CANCEL_PROJECT_TYPE_INDEX {
                return Ok(Self::Cancel { header, project });
            }
            Ok(Self::Start {
                header,
                project,
                own_wallet: reader.boolean()?,
                second: reader.u32()?,
            })
        };
        matches!(
            type_index,
            START_PROJECT_TYPE_INDEX | CANCEL_PROJECT_TYPE_INDEX
        )
        .then(|| {
            let mut reader = Reader::new(body);
            let message = read(&mut reader)?;
            reader.finish()?;
            Ok(message)
        })
    }

    /// The body (the client's form).
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::Start {
                header,
                project,
                own_wallet,
                second,
            } => {
                header.write(&mut writer);
                writer.u32(*project).boolean(*own_wallet).u32(*second);
            }
            Self::Cancel { header, project } => {
                header.write(&mut writer);
                writer.u32(*project);
            }
        }
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up player entity number.
    const PLAYER: u64 = 0x1234;
    /// The retail 2738 after its header (`pay-territory-tax` 1783335287110).
    const PAY_ARGS: &str = "00060100000000000186a0";
    /// The retail 2722 after its header (`pay-territory-tax` 1783335279058).
    const READY_ARGS: &str = "000600000002";
    const TERRITORY: u16 = 6;
    const RETAIL_COST: u64 = 100_000;

    fn header_bytes() -> Vec<u8> {
        let mut writer = Writer::new();
        FacetHeader::of(TERRITORY_INTERACTOR_COMPONENT, PLAYER).write(&mut writer);
        writer.finish()
    }

    fn body(args: &str) -> Vec<u8> {
        let mut body = header_bytes();
        body.extend(hex::decode(args).expect("hex"));
        body
    }

    #[test]
    fn the_retail_upkeep_payment_reads_and_writes_back() {
        let body = body(PAY_ARGS);
        let call = PayUpkeepCostMsg::decode(PAY_UPKEEP_COST_TYPE_INDEX, &body)
            .expect("2738")
            .expect("reads");
        assert_eq!(
            (call.territory, call.flag, call.cost),
            (TERRITORY, true, RETAIL_COST)
        );
        assert_eq!(
            call.header,
            FacetHeader::of(TERRITORY_INTERACTOR_COMPONENT, PLAYER)
        );
        assert_eq!(call.encode(), body);
        assert!(PayUpkeepCostMsg::decode(TERRITORY_FACET_READY_TYPE_INDEX, &body).is_none());
    }

    #[test]
    fn a_cost_above_the_clients_range_or_a_short_body_is_refused() {
        let mut call = PayUpkeepCostMsg::decode(PAY_UPKEEP_COST_TYPE_INDEX, &body(PAY_ARGS))
            .expect("2738")
            .expect("reads");
        call.cost = MAX_UPKEEP_COST + 1;
        assert!(
            PayUpkeepCostMsg::decode(PAY_UPKEEP_COST_TYPE_INDEX, &call.encode())
                .expect("2738")
                .is_err()
        );
        assert!(
            PayUpkeepCostMsg::decode(PAY_UPKEEP_COST_TYPE_INDEX, &header_bytes())
                .expect("2738")
                .is_err()
        );
    }

    #[test]
    fn the_retail_facet_ready_writes_back() {
        let message = TerritoryFacetReadyMsg::of(PLAYER, TERRITORY, ACCESS_OF_A_MEMBER);
        assert_eq!(message.encode().expect("encode"), body(READY_ARGS));
        assert_eq!(
            TerritoryFacetReadyMsg::decode(&body(READY_ARGS)).expect("decode"),
            message
        );
    }

    #[test]
    fn a_town_project_start_and_cancel_read_and_write_back() {
        const PROJECT: u32 = 0x1234_5678;
        let header = FacetHeader::of(TERRITORY_INTERACTOR_COMPONENT, PLAYER);
        let start = TownProjectCallMsg::Start {
            header,
            project: PROJECT,
            own_wallet: true,
            second: 0,
        };
        let body = start.encode();
        assert_eq!(
            body,
            [
                header_bytes(),
                hex::decode("123456780100000000").expect("hex")
            ]
            .concat()
        );
        assert_eq!(
            TownProjectCallMsg::decode(START_PROJECT_TYPE_INDEX, &body),
            Some(Ok(start))
        );
        let cancel = TownProjectCallMsg::Cancel {
            header,
            project: PROJECT,
        };
        assert_eq!(
            TownProjectCallMsg::decode(CANCEL_PROJECT_TYPE_INDEX, &cancel.encode()),
            Some(Ok(cancel))
        );
        assert!(TownProjectCallMsg::decode(PAY_UPKEEP_COST_TYPE_INDEX, &body).is_none());
        assert!(
            TownProjectCallMsg::decode(CANCEL_PROJECT_TYPE_INDEX, &body)
                .expect("2739")
                .is_err(),
            "a start body is too long for a cancel"
        );
    }
}
