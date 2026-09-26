// SPDX-License-Identifier: AGPL-3.0-or-later
//! The trading post: the client's calls of the `ContractsComponent` and the server's answers
//! (economy note `local-data/scratch-work/run15/economy/REPORT.md`; 6,224 contracts of 13 retail
//! captures parse with these layouts and write back byte for byte).
//!
//! | Call | Answer |
//! |---|---|
//! | 832 `SearchContracts` | 2538 `OnSearchContracts`, or 1298 `OnContractActionFailed` |
//! | 2975 `LookupContracts` (contract ids) | 223 `OnLookupContracts` |
//! | 2556 `LookupContractsByParticipant` ([`crate::lookups`]) | 2278 `OnLookupContractsByParticipant` |
//! | 5255 `InitiateServerTransferCheck` | 2278 |
//! | 2972 `CreateContractRequest` | 634 `OnCreateContract`, or 1298 |
//! | 2212 `CompleteContractRequest` | 1117 `OnResolveContract` (a cancel also 2965 `OnCancelContract`), or 1298 |
//!
//! The server also tells a creator that its contract was fully completed (2964
//! `OnFullyCompleteContract`, then 1117 when the proceeds arrive) and that it expired (917
//! `OnExpireContract`). Every answer goes on the call's facet header; a notice on the player's
//! contracts component ([`CONTRACTS_COMPONENT`]).
//!
//! A contract on the wire (`Contract` with its base `ContractBase` and the class `BuyContract`
//! or `SellContract`, reflection `FUN_142d8f9f0`): a presence byte, the class tag, the fields of
//! the reflection in order, then the class's own fields. Times are Unix nanoseconds (retail
//! keeps milliseconds). The participants of a contract's events are never named (retail sends
//! the empty participant on every event of 6,224 contracts).

use crate::combat::{read_string, write_string};
use crate::facet::FacetHeader;
use crate::item_descriptor::{ItemDescriptor, ShortItemDescriptor};
use crate::wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};

/// `ContractsComponentServerFacet_SearchContracts`.
pub const SEARCH_CONTRACTS_TYPE_INDEX: u64 = 832;
/// `ContractsComponentClientFacet_OnSearchContracts`.
pub const SEARCH_RESULTS_TYPE_INDEX: u64 = 2538;
/// `ContractsComponentServerFacet_LookupContracts`.
pub const LOOKUP_CONTRACTS_TYPE_INDEX: u64 = 2975;
/// `ContractsComponentClientFacet_OnLookupContracts`.
pub const CONTRACTS_FOUND_TYPE_INDEX: u64 = 223;
/// `ContractsComponentServerFacet_CreateContractRequest`.
pub const CREATE_CONTRACT_TYPE_INDEX: u64 = 2972;
/// `ContractsComponentClientFacet_OnCreateContract`.
pub const CONTRACT_CREATED_TYPE_INDEX: u64 = 634;
/// `ContractsComponentServerFacet_CompleteContractRequest`.
pub const COMPLETE_CONTRACT_TYPE_INDEX: u64 = 2212;
/// `ContractsComponentClientFacet_OnResolveContract`.
pub const CONTRACT_RESOLVED_TYPE_INDEX: u64 = 1117;
/// `ContractsComponentClientFacet_OnFullyCompleteContract`.
pub const CONTRACT_FULLY_COMPLETED_TYPE_INDEX: u64 = 2964;
/// `ContractsComponentClientFacet_OnCancelContract`.
pub const CONTRACT_CANCELED_TYPE_INDEX: u64 = 2965;
/// `ContractsComponentClientFacet_OnExpireContract`.
pub const CONTRACT_EXPIRED_TYPE_INDEX: u64 = 917;
/// `ContractsComponentClientFacet_OnContractActionFailed`.
pub const CONTRACT_ACTION_FAILED_TYPE_INDEX: u64 = 1298;
/// `ContractsComponentServerFacet_InitiateServerTransferCheck`.
pub const SERVER_TRANSFER_CHECK_TYPE_INDEX: u64 = 5255;
/// The target-id constant of the player's `ContractsComponent` (Observed: the target less the
/// entity of the 832, 2556, 2972, 2212 and 5255 headers of 6 retail characters).
pub const CONTRACTS_COMPONENT: u64 = 0x8c86_dd8a_8b3f_d4c0;

/// The byte before each contract: the pointer is set (all 6,224 retail contracts).
const CONTRACT_PRESENT: u8 = 1;
/// The class tag of a `BuyContract` (642 retail contracts, each without an item descriptor).
const BUY_TAG: u16 = 0x8d2e;
/// The class tag of a `SellContract` (5,582 retail contracts, each with an item descriptor).
const SELL_TAG: u16 = 0xaf2e;
/// The entity ids of a contract (root player, container, currency, interactor, global storage,
/// contracts component).
pub const CONTRACT_ENTITY_IDS: usize = 6;
/// An invalid `AZ::EntityId`, as retail sends each of the six.
pub const INVALID_ENTITY_ID: u64 = 0x0000_0000_ffff_ffff;
/// The participant type of no participant (`-1`; every retail event participant).
pub const NO_PARTICIPANT_TYPE: u32 = u32::MAX;
/// The status and role of the invalid event (`-1`, retail).
pub const INVALID_EVENT_FIELD: u32 = u32::MAX;
/// The marker of a set participant event (`01 00` on every retail event of a contract; the
/// invalid event has `00` alone).
pub const EVENT_MARKER: u8 = 0;
/// Longest perk attribute list: the same limit as the perk lists (Inferred: retail sends it empty
/// on every contract).
const PERK_ATTRIBUTE_LIMIT: usize = 6;
/// The bytes after the contracts of a 223 or 2278 answer (retail: five zero bytes on all 30).
pub const LOOKUP_TAIL: [u8; LOOKUP_TAIL_LEN] = [0; LOOKUP_TAIL_LEN];
const LOOKUP_TAIL_LEN: usize = 5;
/// The bytes after the total of a 2538 answer (retail: nine zero bytes on all 172).
pub const SEARCH_TAIL: [u8; SEARCH_TAIL_LEN] = [0; SEARCH_TAIL_LEN];
const SEARCH_TAIL_LEN: usize = 9;
/// The last `u32` of a create or complete call (retail: 5 on all 46 calls of 6 characters).
pub const RETAIL_REQUEST_TAIL: u32 = 5;
/// Bytes of a create call's item block after the item id (gear score, rarity, perks, perk counts
/// and the item instance id; all zero on the 13 retail resource listings).
pub const CREATE_ITEM_BLOCK_LEN: usize = 34;
/// Longest list of contract ids or outpost names a client sends.
const MAX_REQUEST_LIST: u64 = 255;

/// `ContractType` (enum strings in address order: `Buy`, `Sell`, `MakeGood`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContractKind {
    /// A buy order: the creator pays now, sellers bring items.
    Buy,
    /// A sell order: the creator lists items, buyers pay.
    Sell,
}

impl ContractKind {
    /// The `ContractType` value of a create call.
    pub const fn value(self) -> u32 {
        match self {
            Self::Buy => 0,
            Self::Sell => 1,
        }
    }

    /// The kind of a `ContractType` value; `None` for `MakeGood` or another value.
    pub const fn of_value(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Buy),
            1 => Some(Self::Sell),
            _ => None,
        }
    }

    const fn tag(self) -> u16 {
        match self {
            Self::Buy => BUY_TAG,
            Self::Sell => SELL_TAG,
        }
    }
}

/// `ContractStatus` (Inferred from the enum strings: `Unknown` is `-1`, then `Available`,
/// `Unavailable`, `Accepted`, `Completed`, `Resolved`; retail listings carry 0, a fully
/// completed contract 3, a canceled or expired one 4).
pub mod status {
    /// Open for completions.
    pub const AVAILABLE: u32 = 0;
    /// Every unit is done; the creator's proceeds wait.
    pub const COMPLETED: u32 = 3;
    /// Closed: proceeds paid, or canceled or expired.
    pub const RESOLVED: u32 = 4;
}

/// `ContractCompletionReason` (enum strings in address order: `None`, `Succeeded`, `Failed`,
/// `Canceled`, `Expired`; retail completions carry 1, cancels 3, expiries 4).
pub mod reason {
    /// No completion yet.
    pub const NONE: u32 = 0;
    /// Units were bought or sold.
    pub const SUCCEEDED: u32 = 1;
    /// The creator canceled.
    pub const CANCELED: u32 = 3;
    /// The posting time ran out.
    pub const EXPIRED: u32 = 4;
}

/// `ContractFailureReason` (Inferred from the enum strings: `Unknown` is `-1`, then the others in
/// address order; retail answers a double search with 7, `Throttled`).
pub mod failure {
    /// `InvalidOutpostId`.
    pub const INVALID_OUTPOST: u32 = 0;
    /// `MaximumContractsCreated`.
    pub const MAXIMUM_CONTRACTS: u32 = 1;
    /// `InsufficientGold`.
    pub const INSUFFICIENT_GOLD: u32 = 2;
    /// `InsufficientItems`.
    pub const INSUFFICIENT_ITEMS: u32 = 3;
    /// `ServerError`.
    pub const SERVER_ERROR: u32 = 5;
    /// `CreatorCannotComplete`.
    pub const CREATOR_CANNOT_COMPLETE: u32 = 6;
    /// `Throttled`.
    pub const THROTTLED: u32 = 7;
    /// `InvalidRequest`.
    pub const INVALID_REQUEST: u32 = 8;
    /// `InventoryFull`.
    pub const INVENTORY_FULL: u32 = 9;
    /// `CoinTransferFailed`.
    pub const COIN_TRANSFER_FAILED: u32 = 10;
    /// `InvalidDuration`.
    pub const INVALID_DURATION: u32 = 17;
    /// `AlreadyResolved`.
    pub const ALREADY_RESOLVED: u32 = 18;
}

/// A contract participant: a character (type 0) or nobody ([`Participant::NONE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Participant {
    /// The participant id.
    pub id: [u8; RAW_ID_LEN],
    /// `ContractParticipantType`.
    pub kind: u32,
}

impl Participant {
    /// No participant, as retail sends every one.
    pub const NONE: Self = Self {
        id: [0; RAW_ID_LEN],
        kind: NO_PARTICIPANT_TYPE,
    };

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            id: reader.raw_id()?,
            kind: reader.u32()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.bytes(&self.id).u32(self.kind);
    }
}

/// `ContractParticipantEvent`: what one participant did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantEvent {
    /// The byte after the presence byte of a set event ([`EVENT_MARKER`]); `None` for the
    /// invalid event, whose presence byte is 0.
    pub marker: Option<u8>,
    /// `m_status` (retail 0; the invalid event `-1`).
    pub status: u32,
    /// `m_role` (retail 0; the invalid event `-1`).
    pub role: u32,
    /// `m_reason`: a [`reason`].
    pub reason: u32,
    /// `m_completionCount`: units done.
    pub completion_count: u32,
    /// `m_itemDescriptor` (retail: no item, quantity 1).
    pub item: ItemDescriptor,
}

impl ParticipantEvent {
    /// A set event with `reason` and `completion_count`, as retail sends it.
    pub fn of(reason: u32, completion_count: u32) -> Self {
        Self {
            marker: Some(EVENT_MARKER),
            status: 0,
            role: 0,
            reason,
            completion_count,
            item: ItemDescriptor::of(0, 1),
        }
    }

    /// The invalid event every retail contract ends with.
    pub fn invalid() -> Self {
        Self {
            marker: None,
            status: INVALID_EVENT_FIELD,
            role: INVALID_EVENT_FIELD,
            reason: reason::NONE,
            completion_count: 0,
            item: ItemDescriptor::of(0, 1),
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let marker = if reader.boolean()? {
            Some(reader.u8()?)
        } else {
            None
        };
        Ok(Self {
            marker,
            status: reader.u32()?,
            role: reader.u32()?,
            reason: reader.u32()?,
            completion_count: reader.u32()?,
            item: ItemDescriptor::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.boolean(self.marker.is_some());
        if let Some(marker) = self.marker {
            writer.u8(marker);
        }
        writer
            .u32(self.status)
            .u32(self.role)
            .u32(self.reason)
            .u32(self.completion_count);
        self.item.write(writer)
    }
}

/// `ContractItemData`: the traded item, as the search and the list show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractItemData {
    /// `TradingCategory` of the item (`Resources`, `Weapons`, ...).
    pub category: Vec<u8>,
    /// `TradingFamily`.
    pub family: Vec<u8>,
    /// `TradingGroup`.
    pub group: Vec<u8>,
    /// The `ItemID`.
    pub name: Vec<u8>,
    /// The gear score (0 on resources).
    pub gear_score: u32,
    /// The tier.
    pub tier: u32,
    /// The perks (CRC-32 of each lowercase perk id).
    pub perks: Vec<u32>,
    /// The perk attributes (retail: always empty).
    pub perk_attributes: Vec<u32>,
    /// The rarity level (retail: 0 on every contract, even on gear with perks).
    pub rarity_level: u32,
    /// The required level (`-1` for none).
    pub required_level: u32,
    /// The perk count.
    pub perk_count: u32,
    /// The gem perk count.
    pub gem_perk_count: u32,
    /// The item instance id (retail: zero on every contract).
    pub instance: [u8; RAW_ID_LEN],
}

impl ContractItemData {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            category: read_string(reader)?,
            family: read_string(reader)?,
            group: read_string(reader)?,
            name: read_string(reader)?,
            gear_score: reader.u32()?,
            tier: reader.u32()?,
            perks: crate::combat::read_list(reader, crate::combat::RECAP_LIST_LIMIT)?,
            perk_attributes: crate::combat::read_list(reader, PERK_ATTRIBUTE_LIMIT)?,
            rarity_level: reader.u32()?,
            required_level: reader.u32()?,
            perk_count: reader.u32()?,
            gem_perk_count: reader.u32()?,
            instance: reader.raw_id()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        write_string(writer, &self.category)?;
        write_string(writer, &self.family)?;
        write_string(writer, &self.group)?;
        write_string(writer, &self.name)?;
        writer.u32(self.gear_score).u32(self.tier);
        crate::combat::write_list(writer, &self.perks, crate::combat::RECAP_LIST_LIMIT)?;
        crate::combat::write_list(writer, &self.perk_attributes, PERK_ATTRIBUTE_LIMIT)?;
        writer
            .u32(self.rarity_level)
            .u32(self.required_level)
            .u32(self.perk_count)
            .u32(self.gem_perk_count)
            .bytes(&self.instance);
        Ok(())
    }
}

/// One contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    /// Buy or sell order.
    pub kind: ContractKind,
    /// The contract id.
    pub id: [u8; RAW_ID_LEN],
    /// `m_version`: grows with each change (1 at creation).
    pub version: u32,
    /// `m_status`: a [`status`].
    pub status: u32,
    /// `m_creationTime`, Unix ns.
    pub created_ns: u64,
    /// `m_completionTime`, Unix ns; 0 while open.
    pub completed_ns: u64,
    /// `m_lastUpdateTime`, Unix ns.
    pub updated_ns: u64,
    /// `m_postingDuration`, ns (retail: 1, 3, 7 or 14 days).
    pub duration_ns: u64,
    /// `m_visibilityCriteria` ([`Participant::NONE`] on every retail contract).
    pub visibility: Participant,
    /// `m_isLocalPlayerCreator`: the receiving player made it.
    pub local_creator: bool,
    /// `m_participantEventsMap`.
    pub events: Vec<(Participant, ParticipantEvent)>,
    /// `m_itemData`.
    pub item: ContractItemData,
    /// The six entity ids ([`INVALID_ENTITY_ID`] on every retail contract).
    pub entity_ids: [u64; CONTRACT_ENTITY_IDS],
    /// `m_invalidParticipant`.
    pub invalid_participant: Participant,
    /// `m_invalidParticipantEvent`.
    pub invalid_event: ParticipantEvent,
    /// `m_invalidReason` (retail: varying uninitialised bytes; this server sends 0).
    pub invalid_reason: u32,
    /// The settlement the contract is posted at.
    pub outpost: u64,
    /// A `u64` before the price (name Unknown; retail 0 on every contract).
    pub reserved: u64,
    /// The price of one unit, in hundredths of a coin.
    pub price: u64,
    /// The units still open.
    pub quantity: u32,
    /// A sell order's item ([`ContractKind::Sell`] only): the descriptor of the units still open.
    pub sold_item: Option<ItemDescriptor>,
}

impl Contract {
    /// Reads one contract with its presence byte.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or the class tag is not a buy or sell
    /// order.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let present = reader.u8()?;
        if present != CONTRACT_PRESENT {
            return Err(MessageError::BadTag {
                field: "contract presence",
                tag: present,
            });
        }
        let tag = reader.u16()?;
        let kind = match tag {
            BUY_TAG => ContractKind::Buy,
            SELL_TAG => ContractKind::Sell,
            other => {
                return Err(MessageError::BadTag {
                    field: "contract class",
                    tag: other.to_be_bytes()[0],
                });
            }
        };
        let id = reader.raw_id()?;
        let version = reader.u32()?;
        let status = reader.u32()?;
        let created_ns = reader.u64()?;
        let completed_ns = reader.u64()?;
        let updated_ns = reader.u64()?;
        let duration_ns = reader.u64()?;
        let visibility = Participant::read(reader)?;
        let local_creator = reader.boolean()?;
        let count = reader.wide_prefixed()?;
        let events = (0..count)
            .map(|_| Ok((Participant::read(reader)?, ParticipantEvent::read(reader)?)))
            .collect::<Result<Vec<_>, MessageError>>()?;
        let item = ContractItemData::read(reader)?;
        let mut entity_ids = [0; CONTRACT_ENTITY_IDS];
        for entity in &mut entity_ids {
            *entity = reader.u64()?;
        }
        let invalid_participant = Participant::read(reader)?;
        let invalid_event = ParticipantEvent::read(reader)?;
        let invalid_reason = reader.u32()?;
        let outpost = reader.u64()?;
        let reserved = reader.u64()?;
        let price = reader.u64()?;
        let quantity = reader.u32()?;
        let sold_item = match kind {
            ContractKind::Sell => Some(ItemDescriptor::read(reader)?),
            ContractKind::Buy => None,
        };
        Ok(Self {
            kind,
            id,
            version,
            status,
            created_ns,
            completed_ns,
            updated_ns,
            duration_ns,
            visibility,
            local_creator,
            events,
            item,
            entity_ids,
            invalid_participant,
            invalid_event,
            invalid_reason,
            outpost,
            reserved,
            price,
            quantity,
            sold_item,
        })
    }

    /// Writes the contract with its presence byte.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a list or string that is too long, and
    /// [`MessageError::MaskMismatch`] for a sell order without its item or a buy order with one.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .u8(CONTRACT_PRESENT)
            .u16(self.kind.tag())
            .bytes(&self.id)
            .u32(self.version)
            .u32(self.status)
            .u64(self.created_ns)
            .u64(self.completed_ns)
            .u64(self.updated_ns)
            .u64(self.duration_ns);
        self.visibility.write(writer);
        writer.boolean(self.local_creator);
        writer.wide_prefixed(u64::try_from(self.events.len()).unwrap_or(u64::MAX));
        for (participant, event) in &self.events {
            participant.write(writer);
            event.write(writer)?;
        }
        self.item.write(writer)?;
        for entity in self.entity_ids {
            writer.u64(entity);
        }
        self.invalid_participant.write(writer);
        self.invalid_event.write(writer)?;
        writer
            .u32(self.invalid_reason)
            .u64(self.outpost)
            .u64(self.reserved)
            .u64(self.price)
            .u32(self.quantity);
        match (self.kind, &self.sold_item) {
            (ContractKind::Sell, Some(item)) => item.write(writer),
            (ContractKind::Buy, None) => Ok(()),
            _ => Err(MessageError::MaskMismatch { bit: 0 }),
        }
    }
}

fn read_contracts(reader: &mut Reader<'_>) -> Result<Vec<Contract>, MessageError> {
    let count = reader.wide_prefixed()?;
    (0..count).map(|_| Contract::read(reader)).collect()
}

fn write_contracts(writer: &mut Writer, contracts: &[Contract]) -> Result<(), MessageError> {
    writer.wide_prefixed(u64::try_from(contracts.len()).unwrap_or(u64::MAX));
    contracts
        .iter()
        .try_for_each(|contract| contract.write(writer))
}

fn read_count(reader: &mut Reader<'_>, limit: u64) -> Result<u64, MessageError> {
    let count = u64::from(reader.u8()?);
    if count > limit {
        return Err(MessageError::CountTooLarge { count, limit });
    }
    Ok(count)
}

/// A filter pair of the search (name Unknown; retail `(slot, 777)` five times, from the
/// advanced filters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchPair {
    /// The first `u32`.
    pub first: u32,
    /// The second `u32`.
    pub second: u32,
}

/// The query of a search (`SearchContractsRequestPayload`): 120 retail calls of 13 captures
/// parse with it. Names are Inferred from the values unless marked Observed.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchQuery {
    /// The first `u32` (retail 1 on every call).
    pub first: u32,
    /// The first result to send (Inferred; retail 0 on every call).
    pub start: u32,
    /// Most contracts per answer (Observed: 100 or 5, and the answers hold at most that many).
    pub page_size: u16,
    /// The action the player looks for: 0 buys (the answers hold sell orders), 1 sells (the
    /// answers hold buy orders) (Observed on 59 answered retail searches).
    pub action: u8,
    /// A `u32` (retail 0).
    pub word_a: u32,
    /// A byte (retail 1 when [`SearchQuery::action`] is 0, mostly 0 when it is 1).
    pub byte_b: u8,
    /// A `u32` (retail 0).
    pub word_c: u32,
    /// `TradingCategory` filter; empty for any.
    pub category: Vec<u8>,
    /// `TradingFamily` filter; empty for any.
    pub family: Vec<u8>,
    /// `TradingGroup` filter; empty for any.
    pub group: Vec<u8>,
    /// `ItemID` filter; empty for any.
    pub item: Vec<u8>,
    /// A byte (retail 0).
    pub byte_d: u8,
    /// The tier filter; `u32::MAX` for any.
    pub tier: u32,
    /// Settlement filter: the outposts as 32 hex digits (the 16 of the outpost id, then zeros).
    pub outposts: Vec<Vec<u8>>,
    /// A float (retail -1.0).
    pub float_e: f32,
    /// Filter pairs (meaning Unknown).
    pub pairs: Vec<SearchPair>,
    /// Five range bounds (retail `-1` each, or 100, 900000, 69, 50, -1 with advanced filters).
    pub bounds: [u32; SEARCH_BOUNDS],
    /// Perk filter: perk ids.
    pub perks: Vec<Vec<u8>>,
    /// Two more string lists (retail empty).
    pub lists: [Vec<Vec<u8>>; SEARCH_EXTRA_LISTS],
    /// Four more bounds (retail `-1` each).
    pub more_bounds: [u32; SEARCH_MORE_BOUNDS],
    /// The perk logic (retail 0, or 1 and 2 with a perk filter).
    pub perk_logic: u32,
    /// A `u32` (retail 1 on the 100-contract searches, 0 on the 5-contract ones).
    pub word_f: u32,
    /// A `u32` (retail 0).
    pub word_g: u32,
}

/// Range bounds of a search query.
pub const SEARCH_BOUNDS: usize = 5;
/// String lists of a search query after the perks.
pub const SEARCH_EXTRA_LISTS: usize = 2;
/// Bounds of a search query after the lists.
pub const SEARCH_MORE_BOUNDS: usize = 4;
/// The value of a bound or the tier that sets no limit.
pub const NO_LIMIT: u32 = u32::MAX;

impl SearchQuery {
    /// The kind of contract the search answers with.
    pub const fn wanted_kind(&self) -> ContractKind {
        if self.action == 0 {
            ContractKind::Sell
        } else {
            ContractKind::Buy
        }
    }

    /// The outpost ids of the settlement filter (the first 16 hex digits of each); names that
    /// are not hex are left out.
    pub fn outpost_ids(&self) -> Vec<u64> {
        self.outposts
            .iter()
            .filter_map(|name| {
                let digits = std::str::from_utf8(name.get(..OUTPOST_HEX_DIGITS)?).ok()?;
                u64::from_str_radix(digits, HEX_RADIX).ok()
            })
            .collect()
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let first = reader.u32()?;
        let start = reader.u32()?;
        let page_size = reader.u16()?;
        let action = reader.u8()?;
        let word_a = reader.u32()?;
        let byte_b = reader.u8()?;
        let word_c = reader.u32()?;
        let category = read_string(reader)?;
        let family = read_string(reader)?;
        let group = read_string(reader)?;
        let item = read_string(reader)?;
        let byte_d = reader.u8()?;
        let tier = reader.u32()?;
        let outpost_count = reader.u16()?;
        if u64::from(outpost_count) > MAX_REQUEST_LIST {
            return Err(MessageError::CountTooLarge {
                count: u64::from(outpost_count),
                limit: MAX_REQUEST_LIST,
            });
        }
        let outposts = (0..outpost_count)
            .map(|_| read_string(reader))
            .collect::<Result<_, _>>()?;
        let float_e = reader.f32()?;
        let pair_count = read_count(reader, MAX_REQUEST_LIST)?;
        let pairs = (0..pair_count)
            .map(|_| {
                Ok(SearchPair {
                    first: reader.u32()?,
                    second: reader.u32()?,
                })
            })
            .collect::<Result<_, MessageError>>()?;
        let mut bounds = [0; SEARCH_BOUNDS];
        for bound in &mut bounds {
            *bound = reader.u32()?;
        }
        let perks = read_strings(reader)?;
        let lists = [read_strings(reader)?, read_strings(reader)?];
        let mut more_bounds = [0; SEARCH_MORE_BOUNDS];
        for bound in &mut more_bounds {
            *bound = reader.u32()?;
        }
        Ok(Self {
            first,
            start,
            page_size,
            action,
            word_a,
            byte_b,
            word_c,
            category,
            family,
            group,
            item,
            byte_d,
            tier,
            outposts,
            float_e,
            pairs,
            bounds,
            perks,
            lists,
            more_bounds,
            perk_logic: reader.u32()?,
            word_f: reader.u32()?,
            word_g: reader.u32()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .u32(self.first)
            .u32(self.start)
            .u16(self.page_size)
            .u8(self.action)
            .u32(self.word_a)
            .u8(self.byte_b)
            .u32(self.word_c);
        for text in [&self.category, &self.family, &self.group, &self.item] {
            write_string(writer, text)?;
        }
        writer.u8(self.byte_d).u32(self.tier);
        let outposts =
            u16::try_from(self.outposts.len()).map_err(|_| too_many(self.outposts.len()))?;
        writer.u16(outposts);
        for outpost in &self.outposts {
            write_string(writer, outpost)?;
        }
        writer.f32(self.float_e);
        writer.u8(u8::try_from(self.pairs.len()).map_err(|_| too_many(self.pairs.len()))?);
        for pair in &self.pairs {
            writer.u32(pair.first).u32(pair.second);
        }
        for bound in self.bounds {
            writer.u32(bound);
        }
        write_strings(writer, &self.perks)?;
        for list in &self.lists {
            write_strings(writer, list)?;
        }
        for bound in self.more_bounds {
            writer.u32(bound);
        }
        writer
            .u32(self.perk_logic)
            .u32(self.word_f)
            .u32(self.word_g);
        Ok(())
    }
}

/// Hex digits of an outpost id in a settlement filter name.
const OUTPOST_HEX_DIGITS: usize = 16;
/// Radix of the outpost names.
const HEX_RADIX: u32 = 16;

fn too_many(len: usize) -> MessageError {
    MessageError::CountTooLarge {
        count: u64::try_from(len).unwrap_or(u64::MAX),
        limit: MAX_REQUEST_LIST,
    }
}

fn read_strings(reader: &mut Reader<'_>) -> Result<Vec<Vec<u8>>, MessageError> {
    let count = read_count(reader, MAX_REQUEST_LIST)?;
    (0..count).map(|_| read_string(reader)).collect()
}

fn write_strings(writer: &mut Writer, strings: &[Vec<u8>]) -> Result<(), MessageError> {
    writer.u8(u8::try_from(strings.len()).map_err(|_| too_many(strings.len()))?);
    strings
        .iter()
        .try_for_each(|text| write_string(writer, text))
}

/// The settlement filter name of outpost `outpost` (16 hex digits, then 16 zeros, as retail).
pub fn outpost_name(outpost: u64) -> Vec<u8> {
    format!("{outpost:016X}{:016X}", 0).into_bytes()
}

/// The paging of a participant lookup (2556 after its participant; Inferred names from 51
/// retail calls of 4 forms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantQuery {
    /// The participant.
    pub participant: Participant,
    /// `ContractEventOpenFilter`: 0 all, 1 open, 2 closed.
    pub open_filter: u32,
    /// `ContractEventRoleFilter`: 0 all, 1 creator, 2 acceptor, 3 completer.
    pub role_filter: u32,
    /// The page index.
    pub page: u32,
    /// Most contracts per answer.
    pub page_size: u16,
    /// The `ContractType` values wanted.
    pub kinds: Vec<u32>,
    /// A last list of `u16` (meaning Unknown; retail empty, or one value 9).
    pub tail: Vec<u16>,
}

/// `ContractEventOpenFilter` values.
pub mod open_filter {
    /// Open and closed contracts.
    pub const ALL: u32 = 0;
    /// Contracts still open (available, or completed with proceeds waiting).
    pub const OPEN: u32 = 1;
    /// Resolved contracts.
    pub const CLOSED: u32 = 2;
}

/// `ContractEventRoleFilter` values.
pub mod role_filter {
    /// Any role.
    pub const ALL: u32 = 0;
    /// The player made the contract.
    pub const CREATOR: u32 = 1;
}

impl ParticipantQuery {
    /// Reads the query.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or a list is too long.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let participant = Participant::read(reader)?;
        let open_filter = reader.u32()?;
        let role_filter = reader.u32()?;
        let page = reader.u32()?;
        let page_size = reader.u16()?;
        let kind_count = read_count(reader, MAX_REQUEST_LIST)?;
        let kinds = (0..kind_count)
            .map(|_| reader.u32())
            .collect::<Result<_, _>>()?;
        let tail_count = read_count(reader, MAX_REQUEST_LIST)?;
        let tail = (0..tail_count)
            .map(|_| reader.u16())
            .collect::<Result<_, _>>()?;
        Ok(Self {
            participant,
            open_filter,
            role_filter,
            page,
            page_size,
            kinds,
            tail,
        })
    }

    /// Writes the query.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a list that is too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        self.participant.write(writer);
        writer
            .u32(self.open_filter)
            .u32(self.role_filter)
            .u32(self.page)
            .u16(self.page_size)
            .u8(u8::try_from(self.kinds.len()).map_err(|_| too_many(self.kinds.len()))?);
        for kind in &self.kinds {
            writer.u32(*kind);
        }
        writer.u8(u8::try_from(self.tail.len()).map_err(|_| too_many(self.tail.len()))?);
        for value in &self.tail {
            writer.u16(*value);
        }
        Ok(())
    }
}

/// One trading post call of the client.
#[derive(Debug, Clone, PartialEq)]
pub enum ContractCall {
    /// 832: a search.
    Search {
        /// The header.
        header: FacetHeader,
        /// The query (boxed: it is far larger than the other calls).
        query: Box<SearchQuery>,
        /// The lookup id the answer repeats.
        lookup: [u8; RAW_ID_LEN],
    },
    /// 2975: contracts by id.
    Lookup {
        /// The header.
        header: FacetHeader,
        /// The contract ids.
        ids: Vec<[u8; RAW_ID_LEN]>,
        /// The `u32` after the ids (retail 3).
        word_a: u32,
        /// The next `u64` (retail 0).
        word_b: u64,
        /// The page size (retail 100).
        page_size: u16,
        /// The lookup id the answer repeats.
        lookup: [u8; RAW_ID_LEN],
    },
    /// 2972: a new contract.
    Create {
        /// The header.
        header: FacetHeader,
        /// The order.
        order: CreateOrder,
        /// The request id the answer repeats.
        request: [u8; RAW_ID_LEN],
        /// The last `u32` ([`RETAIL_REQUEST_TAIL`]).
        tail: u32,
    },
    /// 2212: a completion of another player's contract, or a cancel of the player's own.
    Complete {
        /// The header.
        header: FacetHeader,
        /// The contract.
        contract: [u8; RAW_ID_LEN],
        /// The request id the answer repeats.
        request: [u8; RAW_ID_LEN],
        /// `m_type`: the `ContractType` of the contract.
        kind: u32,
        /// `m_count`: units.
        count: u32,
        /// `m_reason`: [`reason::SUCCEEDED`] completes, [`reason::CANCELED`] cancels.
        reason: u32,
        /// `m_itemDescriptor`: the item the player brings or takes.
        item: ShortItemDescriptor,
        /// The last `u32` ([`RETAIL_REQUEST_TAIL`]).
        tail: u32,
    },
    /// 5255: may the player transfer to another world (its contracts would block it).
    TransferCheck {
        /// The header.
        header: FacetHeader,
        /// The lookup id the 2278 answer repeats.
        lookup: [u8; RAW_ID_LEN],
    },
}

/// `ContractCreationParams` of a create call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOrder {
    /// `m_type`: a `ContractType` value.
    pub kind: u32,
    /// `m_gold`: the price of one unit, hundredths of a coin.
    pub price: u64,
    /// `m_count`: units.
    pub count: u32,
    /// `m_postingDurationDays`.
    pub days: u32,
    /// `m_itemId`: the CRC-32 of the lowercase item id.
    pub item_crc: u32,
    /// The rest of the item fields (gear score, rarity, perks, perk counts, instance id).
    pub item_block: [u8; CREATE_ITEM_BLOCK_LEN],
}

impl CreateOrder {
    /// `m_itemInstanceId`, the last 16 bytes of the item block (Inferred from the reflection
    /// order; zero for a stack).
    pub fn instance(&self) -> [u8; RAW_ID_LEN] {
        let mut instance = [0; RAW_ID_LEN];
        instance.copy_from_slice(&self.item_block[CREATE_ITEM_BLOCK_LEN - RAW_ID_LEN..]);
        instance
    }
}

impl ContractCall {
    /// Decodes `body` when `type_index` is a trading post call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            let header = FacetHeader::read(reader)?;
            match type_index {
                SEARCH_CONTRACTS_TYPE_INDEX => Ok(Self::Search {
                    header,
                    query: Box::new(SearchQuery::read(reader)?),
                    lookup: reader.raw_id()?,
                }),
                LOOKUP_CONTRACTS_TYPE_INDEX => {
                    let count = read_count(reader, MAX_REQUEST_LIST)?;
                    let ids = (0..count)
                        .map(|_| reader.raw_id())
                        .collect::<Result<_, _>>()?;
                    Ok(Self::Lookup {
                        header,
                        ids,
                        word_a: reader.u32()?,
                        word_b: reader.u64()?,
                        page_size: reader.u16()?,
                        lookup: reader.raw_id()?,
                    })
                }
                CREATE_CONTRACT_TYPE_INDEX => Ok(Self::Create {
                    header,
                    order: CreateOrder {
                        kind: reader.u32()?,
                        price: reader.u64()?,
                        count: reader.u32()?,
                        days: reader.u32()?,
                        item_crc: reader.u32()?,
                        item_block: reader.array()?,
                    },
                    request: reader.raw_id()?,
                    tail: reader.u32()?,
                }),
                COMPLETE_CONTRACT_TYPE_INDEX => Ok(Self::Complete {
                    header,
                    contract: reader.raw_id()?,
                    request: reader.raw_id()?,
                    kind: reader.u32()?,
                    count: reader.u32()?,
                    reason: reader.u32()?,
                    item: ShortItemDescriptor::read(reader)?,
                    tail: reader.u32()?,
                }),
                _ => Ok(Self::TransferCheck {
                    header,
                    lookup: reader.raw_id()?,
                }),
            }
        };
        match type_index {
            SEARCH_CONTRACTS_TYPE_INDEX
            | LOOKUP_CONTRACTS_TYPE_INDEX
            | CREATE_CONTRACT_TYPE_INDEX
            | COMPLETE_CONTRACT_TYPE_INDEX
            | SERVER_TRANSFER_CHECK_TYPE_INDEX => {
                let mut reader = Reader::new(body);
                Some(read(&mut reader).and_then(|call| {
                    reader.finish()?;
                    Ok(call)
                }))
            }
            _ => None,
        }
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::Search { .. } => SEARCH_CONTRACTS_TYPE_INDEX,
            Self::Lookup { .. } => LOOKUP_CONTRACTS_TYPE_INDEX,
            Self::Create { .. } => CREATE_CONTRACT_TYPE_INDEX,
            Self::Complete { .. } => COMPLETE_CONTRACT_TYPE_INDEX,
            Self::TransferCheck { .. } => SERVER_TRANSFER_CHECK_TYPE_INDEX,
        }
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a list that is too long.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        match self {
            Self::Search {
                header,
                query,
                lookup,
            } => {
                header.write(&mut writer);
                query.write(&mut writer)?;
                writer.bytes(lookup);
            }
            Self::Lookup {
                header,
                ids,
                word_a,
                word_b,
                page_size,
                lookup,
            } => {
                header.write(&mut writer);
                writer.u8(u8::try_from(ids.len()).map_err(|_| too_many(ids.len()))?);
                for id in ids {
                    writer.bytes(id);
                }
                writer
                    .u32(*word_a)
                    .u64(*word_b)
                    .u16(*page_size)
                    .bytes(lookup);
            }
            Self::Create {
                header,
                order,
                request,
                tail,
            } => {
                header.write(&mut writer);
                writer
                    .u32(order.kind)
                    .u64(order.price)
                    .u32(order.count)
                    .u32(order.days)
                    .u32(order.item_crc)
                    .bytes(&order.item_block)
                    .bytes(request)
                    .u32(*tail);
            }
            Self::Complete {
                header,
                contract,
                request,
                kind,
                count,
                reason,
                item,
                tail,
            } => {
                header.write(&mut writer);
                writer
                    .bytes(contract)
                    .bytes(request)
                    .u32(*kind)
                    .u32(*count)
                    .u32(*reason);
                item.write(&mut writer)?;
                writer.u32(*tail);
            }
            Self::TransferCheck { header, lookup } => {
                header.write(&mut writer);
                writer.bytes(lookup);
            }
        }
        Ok(writer.finish())
    }
}

/// 2538: the contracts of one search page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResultsMsg {
    /// The header of the call.
    pub header: FacetHeader,
    /// The lookup id of the call.
    pub lookup: [u8; RAW_ID_LEN],
    /// The contracts of the page.
    pub contracts: Vec<Contract>,
    /// The token of the next page (retail: base64 of `[price, expiry ms, "cn-<id>"]`); empty on
    /// the last page.
    pub next_page: Vec<u8>,
    /// Contracts that match in all (retail caps it at 10,000).
    pub total: u32,
}

impl BodyCodec for SearchResultsMsg {
    const TYPE_INDEX: u64 = SEARCH_RESULTS_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.bytes(&self.lookup);
        write_contracts(&mut writer, &self.contracts)?;
        write_string(&mut writer, &self.next_page)?;
        writer.u32(self.total).bytes(&SEARCH_TAIL);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let lookup = reader.raw_id()?;
        let contracts = read_contracts(&mut reader)?;
        let next_page = read_string(&mut reader)?;
        let total = reader.u32()?;
        let tail: [u8; SEARCH_TAIL_LEN] = reader.array()?;
        if tail != SEARCH_TAIL {
            return Err(MessageError::LengthMismatch {
                field: "search answer tail",
            });
        }
        reader.finish()?;
        Ok(Self {
            header,
            lookup,
            contracts,
            next_page,
            total,
        })
    }
}

/// 223 (by id) or 2278 (by participant): a contract list and the five-byte tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractListMsg {
    /// The header of the call.
    pub header: FacetHeader,
    /// The lookup id of the call.
    pub lookup: [u8; RAW_ID_LEN],
    /// The contracts.
    pub contracts: Vec<Contract>,
}

impl ContractListMsg {
    fn encode_list(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.bytes(&self.lookup);
        write_contracts(&mut writer, &self.contracts)?;
        writer.bytes(&LOOKUP_TAIL);
        Ok(writer.finish())
    }

    fn decode_list(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let lookup = reader.raw_id()?;
        let contracts = read_contracts(&mut reader)?;
        let tail: [u8; LOOKUP_TAIL_LEN] = reader.array()?;
        if tail != LOOKUP_TAIL {
            return Err(MessageError::LengthMismatch {
                field: "contract list tail",
            });
        }
        reader.finish()?;
        Ok(Self {
            header,
            lookup,
            contracts,
        })
    }
}

/// 223: contracts by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractsFoundMsg(pub ContractListMsg);

impl BodyCodec for ContractsFoundMsg {
    const TYPE_INDEX: u64 = CONTRACTS_FOUND_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        self.0.encode_list()
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        ContractListMsg::decode_list(body).map(Self)
    }
}

/// 2278: the contracts of a participant (`crate::lookups` decodes its call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractsLookedUpMsg(pub ContractListMsg);

impl BodyCodec for ContractsLookedUpMsg {
    const TYPE_INDEX: u64 = crate::lookups::CONTRACTS_LOOKED_UP_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        self.0.encode_list()
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        ContractListMsg::decode_list(body).map(Self)
    }
}

/// 634, 1117, 2965 or 917: one contract after an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractNoticeMsg {
    /// The type index: [`CONTRACT_CREATED_TYPE_INDEX`], [`CONTRACT_RESOLVED_TYPE_INDEX`],
    /// [`CONTRACT_CANCELED_TYPE_INDEX`] or [`CONTRACT_EXPIRED_TYPE_INDEX`].
    pub type_index: u64,
    /// The header.
    pub header: FacetHeader,
    /// The request id of the call it answers (a notice: a fresh id).
    pub request: [u8; RAW_ID_LEN],
    /// The contract.
    pub contract: Contract,
}

impl ContractNoticeMsg {
    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the contract does not encode.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.bytes(&self.request);
        self.contract.write(&mut writer)?;
        Ok(writer.finish())
    }

    /// Reads a notice of type `type_index`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match.
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let request = reader.raw_id()?;
        let contract = Contract::read(&mut reader)?;
        reader.finish()?;
        Ok(Self {
            type_index,
            header,
            request,
            contract,
        })
    }
}

/// 2964: a contract of the player is fully completed by another player's request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullyCompletedMsg {
    /// The header (the creator's contracts component).
    pub header: FacetHeader,
    /// The request id of the completing call.
    pub request: [u8; RAW_ID_LEN],
    /// The contract.
    pub contract: Contract,
    /// Units the completing call took.
    pub count: u32,
}

impl BodyCodec for FullyCompletedMsg {
    const TYPE_INDEX: u64 = CONTRACT_FULLY_COMPLETED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.bytes(&self.request);
        self.contract.write(&mut writer)?;
        writer.u32(self.count);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let request = reader.raw_id()?;
        let contract = Contract::read(&mut reader)?;
        let count = reader.u32()?;
        reader.finish()?;
        Ok(Self {
            header,
            request,
            contract,
            count,
        })
    }
}

/// 1298: a call failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractActionFailedMsg {
    /// The header of the call.
    pub header: FacetHeader,
    /// The request or lookup id of the call.
    pub request: [u8; RAW_ID_LEN],
    /// A [`failure`] reason.
    pub reason: u32,
}

impl BodyCodec for ContractActionFailedMsg {
    const TYPE_INDEX: u64 = CONTRACT_ACTION_FAILED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.bytes(&self.request).u32(self.reason).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            request: reader.raw_id()?,
            reason: reader.u32()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// One server message of the trading post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractMessage {
    /// 2538.
    SearchResults(SearchResultsMsg),
    /// 223.
    Found(ContractsFoundMsg),
    /// 2278.
    LookedUp(ContractsLookedUpMsg),
    /// 634, 1117, 2965, 917.
    Notice(ContractNoticeMsg),
    /// 2964.
    FullyCompleted(FullyCompletedMsg),
    /// 1298.
    Failed(ContractActionFailedMsg),
}

impl ContractMessage {
    /// Decodes `body` when `type_index` is a trading post answer; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let message = match type_index {
            SEARCH_RESULTS_TYPE_INDEX => SearchResultsMsg::decode(body).map(Self::SearchResults),
            CONTRACTS_FOUND_TYPE_INDEX => ContractsFoundMsg::decode(body).map(Self::Found),
            crate::lookups::CONTRACTS_LOOKED_UP_TYPE_INDEX => {
                ContractsLookedUpMsg::decode(body).map(Self::LookedUp)
            }
            CONTRACT_CREATED_TYPE_INDEX
            | CONTRACT_RESOLVED_TYPE_INDEX
            | CONTRACT_CANCELED_TYPE_INDEX
            | CONTRACT_EXPIRED_TYPE_INDEX => {
                ContractNoticeMsg::decode(type_index, body).map(Self::Notice)
            }
            CONTRACT_FULLY_COMPLETED_TYPE_INDEX => {
                FullyCompletedMsg::decode(body).map(Self::FullyCompleted)
            }
            CONTRACT_ACTION_FAILED_TYPE_INDEX => {
                ContractActionFailedMsg::decode(body).map(Self::Failed)
            }
            _ => return None,
        };
        Some(message)
    }

    /// The type index.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::SearchResults(_) => SEARCH_RESULTS_TYPE_INDEX,
            Self::Found(_) => CONTRACTS_FOUND_TYPE_INDEX,
            Self::LookedUp(_) => crate::lookups::CONTRACTS_LOOKED_UP_TYPE_INDEX,
            Self::Notice(notice) => notice.type_index,
            Self::FullyCompleted(_) => CONTRACT_FULLY_COMPLETED_TYPE_INDEX,
            Self::Failed(_) => CONTRACT_ACTION_FAILED_TYPE_INDEX,
        }
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a contract does not encode.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        match self {
            Self::SearchResults(message) => message.encode(),
            Self::Found(message) => message.encode(),
            Self::LookedUp(message) => message.encode(),
            Self::Notice(message) => message.encode(),
            Self::FullyCompleted(message) => message.encode(),
            Self::Failed(message) => message.encode(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::hex_bytes;

    /// Retail `buy-sell-TP` 358458 ms: a one-contract search answer (a buy order of one
    /// `BeeswaxT1` at 100.00, 3 days).
    const ONE_RESULT: &str = "f46eb00d253f156a67e7d28299ff40aa3b0d75a7a84f46428b86cd36bbaea35101018d2e8233f5b0d35142a3b43310684f88a957000000010000000018b901e1cfd73a00000000000000000018b901e1cfd73a000000ebbdb3ed000000000000000000000000000000000000ffffffff000100000000000000000000000000000000ffffffff0100000000000000000000000000000000010000000000000000000100ffffffff00000033000000000000095265736f75726365730c5261775265736f757263657307426565737761780942656573776178543100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000ffffffff00000000ffffffff00000000ffffffff00000000ffffffff00000000ffffffff00000000ffffffff00000000000000000000000000000000ffffffff00ffffffffffffffff00000000000000000000000000000000000100ffffffff0000003300000000000000000000b325909b3cc6434f00000000000000000000000000002710000000010000000001000000000000000000";
    /// Retail `buy-sell-TP` 204164 ms: a sell order of one `BeeswaxT1` at 100.00, 3 days.
    const CREATE: &str = "f46eb00d253f156a67e7d28299ff40aa0000000100000000000027100000000100000003d0a826e70000000000000000000000000000000000000000000000000000000000000000000023bda5b379164fd3aefd6033d006c0d400000005";
    /// Retail `tradepost_post_sell_cancel` 86206 ms: a cancel of the player's own sell order.
    const CANCEL: &str = "403321b7af70654cb3ac442d2430908c57a0e164dcd64407a07a3599e1e7d1c088bd84fc70594257a4aed1fe43e539ac000000010000000100000003000000000000000100000000000005";
    /// Retail `buy-sell-TP` 66308 ms: a search of every sell order, 100 per page.
    const SEARCH: &str = "f46eb00d253f156a67e7d28299ff40aa00000001000000000064000000000001000000000000000000ffffffff0000bf80000000ffffffffffffffffffffffffffffffffffffffff000000ffffffffffffffffffffffffffffffff000000000000000100000000";
    /// The lookup id of [`SEARCH`].
    const SEARCH_LOOKUP: &str = "83cd1ea1b68e42819df0414042bd537e";
    /// Retail `buy-sell-TP` 106592 ms: a throttled double search.
    const THROTTLED: &str =
        "f46eb00d253f156a67e7d28299ff40aaa8021b10d7874164a9d2d798bb1e266500000007";

    #[test]
    fn a_retail_search_answer_reads_and_writes_back() {
        let body = hex_bytes(ONE_RESULT);
        let answer = SearchResultsMsg::decode(&body).expect("decode");
        assert_eq!(answer.contracts.len(), 1);
        let contract = &answer.contracts[0];
        assert_eq!(contract.kind, ContractKind::Buy);
        assert_eq!(contract.status, status::AVAILABLE);
        assert_eq!(contract.item.name, b"BeeswaxT1");
        assert_eq!(contract.price, 10_000);
        assert_eq!(contract.quantity, 1);
        assert_eq!(contract.outpost, 0xb325_909b_3cc6_434f);
        assert_eq!(contract.duration_ns, 3 * 86_400 * 1_000_000_000);
        assert_eq!(contract.events.len(), 1);
        assert_eq!(contract.events[0].0, Participant::NONE);
        assert_eq!(
            contract.entity_ids,
            [INVALID_ENTITY_ID; CONTRACT_ENTITY_IDS]
        );
        assert_eq!(answer.total, 1);
        assert!(answer.next_page.is_empty());
        assert_eq!(answer.encode().expect("encode"), body);
    }

    #[test]
    fn retail_calls_read_and_write_back() {
        let create = ContractCall::decode(CREATE_CONTRACT_TYPE_INDEX, &hex_bytes(CREATE))
            .expect("a call")
            .expect("decodes");
        let ContractCall::Create { order, tail, .. } = &create else {
            panic!("create");
        };
        assert_eq!(ContractKind::of_value(order.kind), Some(ContractKind::Sell));
        assert_eq!((order.price, order.count, order.days), (10_000, 1, 3));
        assert_eq!(order.item_crc, 0xd0a8_26e7);
        assert_eq!(order.instance(), [0; RAW_ID_LEN]);
        assert_eq!(*tail, RETAIL_REQUEST_TAIL);
        assert_eq!(create.encode().expect("encode"), hex_bytes(CREATE));
        let cancel = ContractCall::decode(COMPLETE_CONTRACT_TYPE_INDEX, &hex_bytes(CANCEL))
            .expect("a call")
            .expect("decodes");
        let ContractCall::Complete {
            reason: cancel_reason,
            count,
            item,
            ..
        } = &cancel
        else {
            panic!("complete");
        };
        assert_eq!(*cancel_reason, reason::CANCELED);
        assert_eq!(*count, 1);
        assert_eq!(item.item_crc, 0);
        assert_eq!(cancel.encode().expect("encode"), hex_bytes(CANCEL));
        let search_hex = format!("{SEARCH}{SEARCH_LOOKUP}");
        let search = ContractCall::decode(SEARCH_CONTRACTS_TYPE_INDEX, &hex_bytes(&search_hex))
            .expect("a call")
            .expect("decodes");
        let ContractCall::Search { query, lookup, .. } = &search else {
            panic!("search");
        };
        assert_eq!(query.page_size, 100);
        assert_eq!(query.wanted_kind(), ContractKind::Sell);
        assert_eq!(query.tier, NO_LIMIT);
        assert!(query.outposts.is_empty());
        assert_eq!(lookup.to_vec(), hex_bytes(SEARCH_LOOKUP));
        assert_eq!(search.encode().expect("encode"), hex_bytes(&search_hex));
    }

    #[test]
    fn a_failure_answer_round_trips() {
        let failed = ContractActionFailedMsg::decode(&hex_bytes(THROTTLED)).expect("decode");
        assert_eq!(failed.reason, failure::THROTTLED);
        assert_eq!(failed.encode().expect("encode"), hex_bytes(THROTTLED));
    }

    #[test]
    fn a_settlement_filter_names_its_outpost() {
        let name = outpost_name(0xeb0c_6984_ea58_bdef);
        assert_eq!(name, b"EB0C6984EA58BDEF0000000000000000".to_vec());
        let query = SearchQuery {
            outposts: vec![name, b"not hex at all, sixteen+".to_vec()],
            ..search_query()
        };
        assert_eq!(query.outpost_ids(), vec![0xeb0c_6984_ea58_bdef]);
    }

    #[test]
    fn a_sell_order_needs_its_item() {
        let mut contract = SearchResultsMsg::decode(&hex_bytes(ONE_RESULT))
            .expect("decode")
            .contracts
            .remove(0);
        contract.kind = ContractKind::Sell;
        assert!(contract.write(&mut Writer::new()).is_err());
        contract.sold_item = Some(ItemDescriptor::of(0xd0a8_26e7, 1));
        let mut writer = Writer::new();
        contract.write(&mut writer).expect("write");
        let bytes = writer.finish();
        assert_eq!(Contract::read(&mut Reader::new(&bytes)), Ok(contract));
    }

    fn search_query() -> SearchQuery {
        let ContractCall::Search { query, .. } = ContractCall::decode(
            SEARCH_CONTRACTS_TYPE_INDEX,
            &hex_bytes(&format!("{SEARCH}{SEARCH_LOOKUP}")),
        )
        .expect("a call")
        .expect("decodes") else {
            panic!("search");
        };
        *query
    }
}
