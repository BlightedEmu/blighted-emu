// SPDX-License-Identifier: AGPL-3.0-or-later
//! The store and account rewards: the store flag of the player, the entitlement calls and the
//! PvP reward track (economy note `local-data/scratch-work/run15/economy/REPORT.md`, section 6).
//!
//! - 4908 `PlayerComponentServerFacet_RequestSetIsInStore` (a flag): retail answers with the
//!   `isInStore` child of the player's 3935 state (`crate::player_components::put_is_in_store`),
//!   no message (`unlock_store_content`, `menus-linux-2026-09-12`, both new characters).
//! - 898 `EntitlementComponentServerFacet_ClientInitializeTransaction`: a store purchase (retail
//!   `unlock_store_content`: three purchases of `Marks of Fortune`).
//! - 5107 `EntitlementComponentServerFacet_RequestClaimEntitlement`: the claim of an external
//!   reward (retail `ExternalRewards`, `menus-linux-2026-09-12`: a Twitch drop container).
//! - 4932 `RewardTrackComponentServerFacet_RequestPurchaseReward` (slot, item, cost) and 4910
//!   `RequestResetRewardTrack`: the PvP reward track (`pvp_track_unlocked`); retail answers
//!   through the track's replicated state and the bag, no message.

use crate::combat::{read_string, write_string};
use crate::facet::FacetHeader;
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// `PlayerComponentServerFacet_RequestSetIsInStore`.
pub const SET_IS_IN_STORE_TYPE_INDEX: u64 = 4908;
/// `EntitlementComponentServerFacet_ClientInitializeTransaction`.
pub const INITIALIZE_TRANSACTION_TYPE_INDEX: u64 = 898;
/// `EntitlementComponentServerFacet_RequestClaimEntitlement`.
pub const CLAIM_ENTITLEMENT_TYPE_INDEX: u64 = 5107;
/// `RewardTrackComponentServerFacet_RequestPurchaseReward`.
pub const PURCHASE_REWARD_TYPE_INDEX: u64 = 4932;
/// `RewardTrackComponentServerFacet_RequestResetRewardTrack`.
pub const RESET_REWARD_TRACK_TYPE_INDEX: u64 = 4910;

/// A store purchase (898): the client's locale, the offer, and its price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePurchase {
    /// The language (retail `en-US`).
    pub language: Vec<u8>,
    /// The currency (retail `EUR`).
    pub currency: Vec<u8>,
    /// The country (retail `IT`).
    pub country: Vec<u8>,
    /// A `u32` (retail 1).
    pub word_a: u32,
    /// The transaction id.
    pub transaction: [u8; RAW_ID_LEN],
    /// The offer id.
    pub offer: Vec<u8>,
    /// The offer id again (retail: the same text).
    pub offer_again: Vec<u8>,
    /// A byte (retail 0).
    pub byte_b: u8,
    /// The product name (retail `Marks of Fortune`).
    pub product: Vec<u8>,
    /// The price (retail 0 and 5000).
    pub price: u32,
    /// A byte (retail 1).
    pub byte_c: u8,
}

/// One store call of the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreCall {
    /// 4908: the player opened (true) or closed the store.
    SetIsInStore {
        /// The header (the player's `PlayerComponent`).
        header: FacetHeader,
        /// Whether the store is open.
        in_store: bool,
    },
    /// 898: a store purchase.
    Purchase {
        /// The header (the player's `EntitlementComponent`).
        header: FacetHeader,
        /// The purchase.
        purchase: StorePurchase,
    },
    /// 5107: the claim of an entitlement.
    ClaimEntitlement {
        /// The header.
        header: FacetHeader,
        /// The entitlement: the CRC-32 of its lowercase id.
        entitlement: u32,
    },
    /// 4932: buy one reward of the PvP track.
    PurchaseReward {
        /// The header (the player's `RewardTrackComponent`).
        header: FacetHeader,
        /// The reward slot (retail 1 and 7).
        slot: u8,
        /// The reward item: the CRC-32 of its lowercase id.
        item: u32,
        /// The cost the client shows.
        cost: u32,
    },
    /// 4910: reroll the PvP track.
    ResetRewardTrack {
        /// The header.
        header: FacetHeader,
    },
}

impl StoreCall {
    /// Decodes `body` when `type_index` is a store call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            let header = FacetHeader::read(reader)?;
            Ok(match type_index {
                SET_IS_IN_STORE_TYPE_INDEX => Self::SetIsInStore {
                    header,
                    in_store: reader.boolean()?,
                },
                INITIALIZE_TRANSACTION_TYPE_INDEX => Self::Purchase {
                    header,
                    purchase: StorePurchase {
                        language: read_string(reader)?,
                        currency: read_string(reader)?,
                        country: read_string(reader)?,
                        word_a: reader.u32()?,
                        transaction: reader.raw_id()?,
                        offer: read_string(reader)?,
                        offer_again: read_string(reader)?,
                        byte_b: reader.u8()?,
                        product: read_string(reader)?,
                        price: reader.u32()?,
                        byte_c: reader.u8()?,
                    },
                },
                CLAIM_ENTITLEMENT_TYPE_INDEX => Self::ClaimEntitlement {
                    header,
                    entitlement: reader.u32()?,
                },
                PURCHASE_REWARD_TYPE_INDEX => Self::PurchaseReward {
                    header,
                    slot: reader.u8()?,
                    item: reader.u32()?,
                    cost: reader.u32()?,
                },
                _ => Self::ResetRewardTrack { header },
            })
        };
        match type_index {
            SET_IS_IN_STORE_TYPE_INDEX
            | INITIALIZE_TRANSACTION_TYPE_INDEX
            | CLAIM_ENTITLEMENT_TYPE_INDEX
            | PURCHASE_REWARD_TYPE_INDEX
            | RESET_REWARD_TRACK_TYPE_INDEX => {
                let mut reader = Reader::new(body);
                Some(read(&mut reader).and_then(|call| {
                    reader.finish()?;
                    Ok(call)
                }))
            }
            _ => None,
        }
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a text that is too long.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        match self {
            Self::SetIsInStore { header, in_store } => {
                header.write(&mut writer);
                writer.boolean(*in_store);
            }
            Self::Purchase { header, purchase } => {
                header.write(&mut writer);
                write_string(&mut writer, &purchase.language)?;
                write_string(&mut writer, &purchase.currency)?;
                write_string(&mut writer, &purchase.country)?;
                writer.u32(purchase.word_a).bytes(&purchase.transaction);
                write_string(&mut writer, &purchase.offer)?;
                write_string(&mut writer, &purchase.offer_again)?;
                writer.u8(purchase.byte_b);
                write_string(&mut writer, &purchase.product)?;
                writer.u32(purchase.price).u8(purchase.byte_c);
            }
            Self::ClaimEntitlement {
                header,
                entitlement,
            } => {
                header.write(&mut writer);
                writer.u32(*entitlement);
            }
            Self::PurchaseReward {
                header,
                slot,
                item,
                cost,
            } => {
                header.write(&mut writer);
                writer.u8(*slot).u32(*item).u32(*cost);
            }
            Self::ResetRewardTrack { header } => header.write(&mut writer),
        }
        Ok(writer.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::hex_bytes;

    /// Retail calls: `unlock_store_content` 12633 ms (4908), 23700 ms (898), `ExternalRewards`
    /// 66150 ms (5107), `pvp_track_unlocked` 31868 ms (4932) and 56565 ms (4910).
    const CALLS: [(u64, &str); 5] = [
        (
            SET_IS_IN_STORE_TYPE_INDEX,
            "07a3b18acf378b0b2896c8c14210296e01",
        ),
        (
            INITIALIZE_TRANSACTION_TYPE_INDEX,
            "eb5c1ce8c5cdbff72896c8c14210296e05656e2d55530345555202495400000001e58f4711d7e74ac483462fb23806808c16366745374b715a53724251564c31724c4c42785a6e4116366745374b715a53724251564c31724c4c42785a6e4100104d61726b73206f6620466f7274756e650000000001",
        ),
        (
            CLAIM_ENTITLEMENT_TYPE_INDEX,
            "a3f72a79438587bbe131d651bfc7f132f938e156",
        ),
        (
            PURCHASE_REWARD_TYPE_INDEX,
            "67e3d016f476590a2896c8c14210296e07fe1498030000c350",
        ),
        (
            RESET_REWARD_TRACK_TYPE_INDEX,
            "67e3d016f476590a2896c8c14210296e",
        ),
    ];

    #[test]
    fn retail_calls_read_and_write_back() {
        for (type_index, hex) in CALLS {
            let call = StoreCall::decode(type_index, &hex_bytes(hex))
                .expect("a call")
                .expect("decodes");
            assert_eq!(
                call.encode().expect("encode"),
                hex_bytes(hex),
                "{type_index}"
            );
        }
        let Some(Ok(StoreCall::Purchase { purchase, .. })) =
            StoreCall::decode(CALLS[1].0, &hex_bytes(CALLS[1].1))
        else {
            panic!("purchase");
        };
        assert_eq!(purchase.product, b"Marks of Fortune");
        let Some(Ok(StoreCall::PurchaseReward { slot, cost, .. })) =
            StoreCall::decode(CALLS[3].0, &hex_bytes(CALLS[3].1))
        else {
            panic!("reward");
        };
        assert_eq!((slot, cost), (7, 50_000));
    }

    #[test]
    fn other_types_are_not_store_calls() {
        assert!(StoreCall::decode(SET_IS_IN_STORE_TYPE_INDEX + 1, &[]).is_none());
    }
}
