// SPDX-License-Identifier: AGPL-3.0-or-later
//! Player-to-player trade: the client's calls of the `P2PTradeComponent`, the server's answers
//! and the trade state (retail `trading_between_players` 1 to 8: 16 trades of 3 characters;
//! economy note `local-data/scratch-work/run15/economy/REPORT.md`, section 4).
//!
//! | Call | Answer |
//! |---|---|
//! | 1437 `UpdateOfferedItems` (item instances and quantities) | 688 `OnUpdateOfferedItemsResponse` |
//! | 122 `UpdateOfferedCoin` | 4253 `OnUpdateOfferedCoinResponse` |
//! | 4202 `LockInOffer` | 3364 `OnLockInOfferResponse` |
//! | 5263 `CancelOfferLockIn` | 5264 `OnCancelOfferLockInResponse` |
//! | 495 `ConfirmTrade` | 4254 `OnConfirmTradeResponse`; the other player gets 3506 `OnOtherPlayerConfirmed` |
//! | 4252 `CancelTrade` (a reason) | 2179 `OnTradeSessionEnded` to both |
//!
//! A trade starts with 2652 `OnTradeSessionStarted` to both players (after the generic invite
//! of type `trade` is accepted) and ends with 2179 to both: reason 0 after the swap. Each answer
//! is a `P2PTradeFailure` `u32` (0 on all 98 retail answers). The offers show in the
//! `P2PTradeReplicatedState` of each player (type 4251, player component [`P2P_TRADE_STATE_KEY`];
//! constructor `FUN_1440c9720`): the player's own offer, the other's offer, and whether each is
//! locked in. No retail capture has 5263 or 5264; they are read like 4202 and 3364, their
//! siblings (Inferred).

use crate::facet::FacetHeader;
use crate::items::{FullList, ItemRecord};
use crate::slots::{SLOT_0, children_mask, read_slot_0, write_children};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// `P2PTradeComponentServerFacet_UpdateOfferedItems`.
pub const UPDATE_OFFERED_ITEMS_TYPE_INDEX: u64 = 1437;
/// `P2PTradeComponentServerFacet_UpdateOfferedCoin`.
pub const UPDATE_OFFERED_COIN_TYPE_INDEX: u64 = 122;
/// `P2PTradeComponentServerFacet_LockInOffer`.
pub const LOCK_IN_OFFER_TYPE_INDEX: u64 = 4202;
/// `P2PTradeComponentServerFacet_CancelOfferLockIn`.
pub const CANCEL_LOCK_IN_TYPE_INDEX: u64 = 5263;
/// `P2PTradeComponentServerFacet_ConfirmTrade`.
pub const CONFIRM_TRADE_TYPE_INDEX: u64 = 495;
/// `P2PTradeComponentServerFacet_CancelTrade`.
pub const CANCEL_TRADE_TYPE_INDEX: u64 = 4252;
/// `P2PTradeComponentClientFacet_OnTradeSessionStarted`.
pub const SESSION_STARTED_TYPE_INDEX: u64 = 2652;
/// `P2PTradeComponentClientFacet_OnTradeSessionEnded`.
pub const SESSION_ENDED_TYPE_INDEX: u64 = 2179;
/// `P2PTradeComponentClientFacet_OnUpdateOfferedItemsResponse`.
pub const ITEMS_RESPONSE_TYPE_INDEX: u64 = 688;
/// `P2PTradeComponentClientFacet_OnUpdateOfferedCoinResponse`.
pub const COIN_RESPONSE_TYPE_INDEX: u64 = 4253;
/// `P2PTradeComponentClientFacet_OnLockInOfferResponse`.
pub const LOCK_IN_RESPONSE_TYPE_INDEX: u64 = 3364;
/// `P2PTradeComponentClientFacet_OnCancelOfferLockInResponse`.
pub const CANCEL_LOCK_IN_RESPONSE_TYPE_INDEX: u64 = 5264;
/// `P2PTradeComponentClientFacet_OnConfirmTradeResponse`.
pub const CONFIRM_RESPONSE_TYPE_INDEX: u64 = 4254;
/// `P2PTradeComponentClientFacet_OnOtherPlayerConfirmed`.
pub const OTHER_CONFIRMED_TYPE_INDEX: u64 = 3506;
/// Type index of `Javelin::P2PTradeReplicatedState`.
pub const P2P_TRADE_STATE_TYPE_INDEX: u64 = 4251;
/// The target-id constant of the player's `P2PTradeComponent` (Observed: the target less the
/// entity of every 1437, 122, 4202, 495 and 4252 header of the trade captures).
pub const P2P_TRADE_COMPONENT: u64 = 0x3d36_7958_16f1_b0e8;
/// The player component that holds the trade state (Observed: member key 59 of the 52 retail
/// own bundles with a trade state).
pub const P2P_TRADE_STATE_KEY: u64 = 59;
/// The byte before the other player's character id in 2652 (Observed: 5 in all 26 retail
/// starts; the social lookups 6453 carry the same byte before a character id).
pub const PLAYER_REF_KIND: u8 = 5;
/// Most offered stacks in one call.
const MAX_OFFERED_STACKS: u8 = u8::MAX;
/// Children of the trade state: `ownOfferedItems`, `ownOfferedCoin`, `ownOfferIsLockedIn`,
/// `otherOfferedItems`, `otherOfferedCoin`, `otherOfferIsLockedIn`, `canReceiveOtherItems`.
const STATE_CHILDREN: u8 = 7;

/// `P2PTradeFailure` values (the enum strings in address order; the client sends 17 with its
/// cancel and retail ends a canceled trade with 17 to both players).
pub mod failure {
    /// No failure.
    pub const NONE: u32 = 0;
    /// `InvalidItems`.
    pub const INVALID_ITEMS: u32 = 4;
    /// `InvalidCoinAmount`.
    pub const INVALID_COIN_AMOUNT: u32 = 6;
    /// `TradeAlreadyConfirmed`.
    pub const TRADE_ALREADY_CONFIRMED: u32 = 7;
    /// `CannotReceiveItems`.
    pub const CANNOT_RECEIVE_ITEMS: u32 = 8;
    /// `BothPlayersNotLockedIn`.
    pub const BOTH_PLAYERS_NOT_LOCKED_IN: u32 = 9;
    /// `TradeStateMismatch`.
    pub const TRADE_STATE_MISMATCH: u32 = 11;
    /// `FailedToRemoveItemsOrCoin`.
    pub const FAILED_TO_REMOVE: u32 = 14;
    /// The reason of a canceled trade (Observed: every retail 4252 and the 2179 after it).
    pub const TRADE_CANCELED: u32 = 17;
}

/// One offered stack: the item instance and how many of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfferedStack {
    /// The item instance id.
    pub instance: [u8; RAW_ID_LEN],
    /// How many.
    pub quantity: u64,
}

/// One trade call of the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeCall {
    /// 1437: the whole offer of items, as it is now.
    UpdateItems {
        /// The header.
        header: FacetHeader,
        /// The stacks.
        stacks: Vec<OfferedStack>,
    },
    /// 122: the offered coin, in hundredths.
    UpdateCoin {
        /// The header.
        header: FacetHeader,
        /// The coin.
        coin: u64,
    },
    /// 4202: lock the offer.
    LockIn {
        /// The header.
        header: FacetHeader,
    },
    /// 5263: unlock the offer.
    CancelLockIn {
        /// The header.
        header: FacetHeader,
    },
    /// 495: confirm the locked offers.
    Confirm {
        /// The header.
        header: FacetHeader,
    },
    /// 4252: end the trade.
    Cancel {
        /// The header.
        header: FacetHeader,
        /// The reason ([`failure::TRADE_CANCELED`]).
        reason: u32,
    },
}

impl TradeCall {
    /// Decodes `body` when `type_index` is a trade call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            let header = FacetHeader::read(reader)?;
            Ok(match type_index {
                UPDATE_OFFERED_ITEMS_TYPE_INDEX => {
                    let count = reader.u8()?;
                    let stacks = (0..count)
                        .map(|_| {
                            Ok(OfferedStack {
                                instance: reader.raw_id()?,
                                quantity: reader.u64()?,
                            })
                        })
                        .collect::<Result<_, MessageError>>()?;
                    Self::UpdateItems { header, stacks }
                }
                UPDATE_OFFERED_COIN_TYPE_INDEX => Self::UpdateCoin {
                    header,
                    coin: reader.u64()?,
                },
                LOCK_IN_OFFER_TYPE_INDEX => Self::LockIn { header },
                CANCEL_LOCK_IN_TYPE_INDEX => Self::CancelLockIn { header },
                CONFIRM_TRADE_TYPE_INDEX => Self::Confirm { header },
                _ => Self::Cancel {
                    header,
                    reason: reader.u32()?,
                },
            })
        };
        match type_index {
            UPDATE_OFFERED_ITEMS_TYPE_INDEX
            | UPDATE_OFFERED_COIN_TYPE_INDEX
            | LOCK_IN_OFFER_TYPE_INDEX
            | CANCEL_LOCK_IN_TYPE_INDEX
            | CONFIRM_TRADE_TYPE_INDEX
            | CANCEL_TRADE_TYPE_INDEX => {
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
            Self::UpdateItems { .. } => UPDATE_OFFERED_ITEMS_TYPE_INDEX,
            Self::UpdateCoin { .. } => UPDATE_OFFERED_COIN_TYPE_INDEX,
            Self::LockIn { .. } => LOCK_IN_OFFER_TYPE_INDEX,
            Self::CancelLockIn { .. } => CANCEL_LOCK_IN_TYPE_INDEX,
            Self::Confirm { .. } => CONFIRM_TRADE_TYPE_INDEX,
            Self::Cancel { .. } => CANCEL_TRADE_TYPE_INDEX,
        }
    }

    /// The header of the call.
    pub const fn header(&self) -> FacetHeader {
        match self {
            Self::UpdateItems { header, .. }
            | Self::UpdateCoin { header, .. }
            | Self::LockIn { header }
            | Self::CancelLockIn { header }
            | Self::Confirm { header }
            | Self::Cancel { header, .. } => *header,
        }
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for more than 255 stacks.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::UpdateItems { stacks, .. } => {
                let count =
                    u8::try_from(stacks.len()).map_err(|_| MessageError::CountTooLarge {
                        count: u64::try_from(stacks.len()).unwrap_or(u64::MAX),
                        limit: u64::from(MAX_OFFERED_STACKS),
                    })?;
                writer.u8(count);
                for stack in stacks {
                    writer.bytes(&stack.instance).u64(stack.quantity);
                }
            }
            Self::UpdateCoin { coin, .. } => {
                writer.u64(*coin);
            }
            Self::Cancel { reason, .. } => {
                writer.u32(*reason);
            }
            Self::LockIn { .. } | Self::CancelLockIn { .. } | Self::Confirm { .. } => {}
        }
        Ok(writer.finish())
    }
}

/// One trade message of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeMessage {
    /// 2652: a trade with the character `other` started.
    Started {
        /// The header (the player's trade component).
        header: FacetHeader,
        /// The other player's character id.
        other: [u8; RAW_ID_LEN],
    },
    /// 2179: the trade ended.
    Ended {
        /// The header.
        header: FacetHeader,
        /// [`failure::NONE`] after the swap, else why.
        reason: u32,
    },
    /// 688, 4253, 3364, 5264 or 4254: the answer to a call.
    Response {
        /// The type index of the answer.
        type_index: u64,
        /// The header of the call.
        header: FacetHeader,
        /// A [`failure`].
        failure: u32,
    },
    /// 3506: the other player confirmed.
    OtherConfirmed {
        /// The header.
        header: FacetHeader,
    },
}

impl TradeMessage {
    /// The answer of `type_index` to a call of type `call`; `None` for a call without one.
    pub const fn response_type(call: u64) -> Option<u64> {
        match call {
            UPDATE_OFFERED_ITEMS_TYPE_INDEX => Some(ITEMS_RESPONSE_TYPE_INDEX),
            UPDATE_OFFERED_COIN_TYPE_INDEX => Some(COIN_RESPONSE_TYPE_INDEX),
            LOCK_IN_OFFER_TYPE_INDEX => Some(LOCK_IN_RESPONSE_TYPE_INDEX),
            CANCEL_LOCK_IN_TYPE_INDEX => Some(CANCEL_LOCK_IN_RESPONSE_TYPE_INDEX),
            CONFIRM_TRADE_TYPE_INDEX => Some(CONFIRM_RESPONSE_TYPE_INDEX),
            _ => None,
        }
    }

    /// Decodes `body` when `type_index` is a trade message; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            let header = FacetHeader::read(reader)?;
            Ok(match type_index {
                SESSION_STARTED_TYPE_INDEX => {
                    let kind = reader.u8()?;
                    if kind != PLAYER_REF_KIND {
                        return Err(MessageError::BadTag {
                            field: "trade partner",
                            tag: kind,
                        });
                    }
                    Self::Started {
                        header,
                        other: reader.raw_id()?,
                    }
                }
                SESSION_ENDED_TYPE_INDEX => Self::Ended {
                    header,
                    reason: reader.u32()?,
                },
                OTHER_CONFIRMED_TYPE_INDEX => Self::OtherConfirmed { header },
                _ => Self::Response {
                    type_index,
                    header,
                    failure: reader.u32()?,
                },
            })
        };
        match type_index {
            SESSION_STARTED_TYPE_INDEX
            | SESSION_ENDED_TYPE_INDEX
            | OTHER_CONFIRMED_TYPE_INDEX
            | ITEMS_RESPONSE_TYPE_INDEX
            | COIN_RESPONSE_TYPE_INDEX
            | LOCK_IN_RESPONSE_TYPE_INDEX
            | CANCEL_LOCK_IN_RESPONSE_TYPE_INDEX
            | CONFIRM_RESPONSE_TYPE_INDEX => {
                let mut reader = Reader::new(body);
                Some(read(&mut reader).and_then(|message| {
                    reader.finish()?;
                    Ok(message)
                }))
            }
            _ => None,
        }
    }

    /// The type index.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::Started { .. } => SESSION_STARTED_TYPE_INDEX,
            Self::Ended { .. } => SESSION_ENDED_TYPE_INDEX,
            Self::Response { type_index, .. } => *type_index,
            Self::OtherConfirmed { .. } => OTHER_CONFIRMED_TYPE_INDEX,
        }
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::Started { header, other } => {
                header.write(&mut writer);
                writer.u8(PLAYER_REF_KIND).bytes(other);
            }
            Self::Ended { header, reason } => {
                header.write(&mut writer);
                writer.u32(*reason);
            }
            Self::Response {
                header, failure, ..
            } => {
                header.write(&mut writer);
                writer.u32(*failure);
            }
            Self::OtherConfirmed { header } => header.write(&mut writer),
        }
        writer.finish()
    }
}

/// `Javelin::P2PTradeReplicatedState`: the children that changed, `None` for the others.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct P2PTradeState {
    /// `ownOfferedItems`.
    pub own_items: Option<FullList<ItemRecord>>,
    /// `ownOfferedCoin`.
    pub own_coin: Option<u64>,
    /// `ownOfferIsLockedIn`.
    pub own_locked: Option<bool>,
    /// `otherOfferedItems`.
    pub other_items: Option<FullList<ItemRecord>>,
    /// `otherOfferedCoin`.
    pub other_coin: Option<u64>,
    /// `otherOfferIsLockedIn`.
    pub other_locked: Option<bool>,
    /// `canReceiveOtherItems`.
    pub can_receive: Option<bool>,
}

impl P2PTradeState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, an item list is a delta, or a child
    /// the class has not is set.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, "p2p trade", STATE_CHILDREN, |reader, child| {
            match child {
                0 => state.own_items = Some(FullList::read(reader, ItemRecord::read)?),
                1 => state.own_coin = Some(reader.u64()?),
                2 => state.own_locked = Some(reader.boolean()?),
                3 => state.other_items = Some(FullList::read(reader, ItemRecord::read)?),
                4 => state.other_coin = Some(reader.u64()?),
                5 => state.other_locked = Some(reader.boolean()?),
                _ => state.can_receive = Some(reader.boolean()?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for an item that does not fit its record.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let present = [
            self.own_items.is_some(),
            self.own_coin.is_some(),
            self.own_locked.is_some(),
            self.other_items.is_some(),
            self.other_coin.is_some(),
            self.other_locked.is_some(),
            self.can_receive.is_some(),
        ];
        let children = children_mask(&present);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_children(writer, children, STATE_CHILDREN, |writer, child| {
            let write_items = |writer: &mut Writer, items: &FullList<ItemRecord>| {
                items.write(writer, |writer, item| item.write(writer))
            };
            match child {
                0 => self
                    .own_items
                    .as_ref()
                    .map_or(Ok(()), |items| write_items(writer, items)),
                1 => {
                    self.own_coin.map(|coin| writer.u64(coin));
                    Ok(())
                }
                2 => {
                    self.own_locked.map(|locked| writer.boolean(locked));
                    Ok(())
                }
                3 => self
                    .other_items
                    .as_ref()
                    .map_or(Ok(()), |items| write_items(writer, items)),
                4 => {
                    self.other_coin.map(|coin| writer.u64(coin));
                    Ok(())
                }
                5 => {
                    self.other_locked.map(|locked| writer.boolean(locked));
                    Ok(())
                }
                _ => {
                    self.can_receive.map(|can| writer.boolean(can));
                    Ok(())
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::hex_bytes;

    /// Retail `trading_between_players` 107327 ms and 108309 ms: one stack, then two.
    const ONE_STACK: &str =
        "65cd42195901da562896c8c14210296e0146d9854bdd624cc9b1a92854bcd1b0ee000000000000000b";
    const TWO_STACKS: &str = "65cd42195901da562896c8c14210296e0246d9854bdd624cc9b1a92854bcd1b0ee000000000000000bac2cd0ecbac449d384a214879e5422ce0000000000000009";
    /// Retail `trading_between_players_3` 37194 ms: a cancel.
    const CANCEL: &str = "65cd42195901da562896c8c14210296e00000011";
    /// Retail `trading_between_players` 21425 ms: the start.
    const STARTED: &str = "65cd42195901da562896c8c14210296e0568ae0505138045c88f9460ed13aa849c";
    /// The own trade states of retail `trading_between_players` (body after member key 59 and
    /// class 4251): 100 coin offered (27976 ms), the other locked in (29130 ms), the reset at the
    /// end (31478 ms), and one stack of 48 offered (70059 ms).
    const COIN: &str = "01020000000000000064";
    const OTHER_LOCKED: &str = "012001";
    const RESET: &str = "012f0001a641000000000000000000000001a6410000";
    const ITEMS: &str = "01010001a95301002e38a9ced2dcad00c6c2a600000000000000000000dcee93748d4045f98f1c96f892676978000000000000000000";

    #[test]
    fn retail_calls_read_and_write_back() {
        for (type_index, hex) in [
            (UPDATE_OFFERED_ITEMS_TYPE_INDEX, ONE_STACK),
            (UPDATE_OFFERED_ITEMS_TYPE_INDEX, TWO_STACKS),
            (CANCEL_TRADE_TYPE_INDEX, CANCEL),
        ] {
            let call = TradeCall::decode(type_index, &hex_bytes(hex))
                .expect("a call")
                .expect("decodes");
            assert_eq!(call.encode().expect("encode"), hex_bytes(hex));
        }
        let Some(Ok(TradeCall::UpdateItems { stacks, .. })) =
            TradeCall::decode(UPDATE_OFFERED_ITEMS_TYPE_INDEX, &hex_bytes(TWO_STACKS))
        else {
            panic!("items");
        };
        assert_eq!(
            stacks
                .iter()
                .map(|stack| stack.quantity)
                .collect::<Vec<_>>(),
            [11, 9]
        );
    }

    #[test]
    fn the_retail_start_names_the_other_character() {
        let message = TradeMessage::decode(SESSION_STARTED_TYPE_INDEX, &hex_bytes(STARTED))
            .expect("a message")
            .expect("decodes");
        let TradeMessage::Started { other, .. } = message else {
            panic!("start");
        };
        assert_eq!(other[0], 0x68);
        assert_eq!(message.encode(), hex_bytes(STARTED));
    }

    #[test]
    fn retail_trade_states_read_and_write_back() {
        for hex in [COIN, OTHER_LOCKED, RESET, ITEMS] {
            let bytes = hex_bytes(hex);
            let mut reader = Reader::new(&bytes);
            let state = P2PTradeState::read(&mut reader).expect("read");
            assert!(reader.is_empty(), "{hex}");
            let mut writer = Writer::new();
            state.write(&mut writer).expect("write");
            assert_eq!(writer.finish(), bytes);
        }
        let item_bytes = hex_bytes(ITEMS);
        let mut reader = Reader::new(&item_bytes);
        let items = P2PTradeState::read(&mut reader).expect("read");
        let own = items.own_items.expect("own items");
        assert_eq!(own.elements.len(), 1);
        assert_eq!(own.elements[0].quantity, 48);
        let reset_bytes = hex_bytes(RESET);
        let mut reader = Reader::new(&reset_bytes);
        let reset = P2PTradeState::read(&mut reader).expect("read");
        assert_eq!(reset.own_coin, Some(0));
        assert_eq!(reset.other_locked, Some(false));
    }

    #[test]
    fn each_call_with_an_answer_names_it() {
        assert_eq!(
            TradeMessage::response_type(LOCK_IN_OFFER_TYPE_INDEX),
            Some(LOCK_IN_RESPONSE_TYPE_INDEX)
        );
        assert_eq!(TradeMessage::response_type(CANCEL_TRADE_TYPE_INDEX), None);
    }
}
