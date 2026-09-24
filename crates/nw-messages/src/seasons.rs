// SPDX-License-Identifier: AGPL-3.0-or-later
//! The season activities and rewards: the client's calls of the `SeasonsRewardsComponent` and
//! the server's answers (retail `Hive_Seasons_Rewards` and `reroll_activities`: 14 calls; economy
//! note `local-data/scratch-work/run15/economy/REPORT.md`, section 5).
//!
//! | Call | Answer |
//! |---|---|
//! | 5645 `ActivitiesRequestStampSquare` (card, square) | 5686 `OnActivitiesStampResponse` |
//! | 5647 `ActivitiesRequestNewCard` | 5687 `OnActivitiesNewCardResponse` |
//! | 5648 `ActivitiesRequestReroll` | 5688 `OnActivitiesRerollResponse` |
//! | 5611 `RequestRedeemSeasonsReward` (reward id) | 5629 `OnRedeemSeasonsRewardResponse` (reward id) |
//!
//! Each answer is a `SeasonsRewardsActivitiesResponse` `u32` on the call's header (retail 1,
//! `Success`, on all 14).

use crate::facet::FacetHeader;
use crate::wire::{MessageError, Reader, Writer};

/// `SeasonsRewardsComponentServerFacet_ActivitiesRequestStampSquare`.
pub const STAMP_SQUARE_TYPE_INDEX: u64 = 5645;
/// `SeasonsRewardsComponentServerFacet_ActivitiesRequestNewCard`.
pub const NEW_CARD_TYPE_INDEX: u64 = 5647;
/// `SeasonsRewardsComponentServerFacet_ActivitiesRequestReroll`.
pub const REROLL_TYPE_INDEX: u64 = 5648;
/// `SeasonsRewardsComponentServerFacet_RequestRedeemSeasonsReward`.
pub const REDEEM_REWARD_TYPE_INDEX: u64 = 5611;
/// `SeasonsRewardsComponentClientFacet_OnActivitiesStampResponse`.
pub const STAMP_RESPONSE_TYPE_INDEX: u64 = 5686;
/// `SeasonsRewardsComponentClientFacet_OnActivitiesNewCardResponse`.
pub const NEW_CARD_RESPONSE_TYPE_INDEX: u64 = 5687;
/// `SeasonsRewardsComponentClientFacet_OnActivitiesRerollResponse`.
pub const REROLL_RESPONSE_TYPE_INDEX: u64 = 5688;
/// `SeasonsRewardsComponentClientFacet_OnRedeemSeasonsRewardResponse`.
pub const REDEEM_RESPONSE_TYPE_INDEX: u64 = 5629;
/// Bytes of a stamp call's square (Observed values: row, column and flag bytes, `00 02 00 01`
/// and `11 01 00 00`; their names are Unknown).
pub const SQUARE_LEN: usize = 4;

/// `SeasonsRewardsActivitiesResponse` (enum strings in address order: `Unknown`, `Success`,
/// `Failure`, `Failure_MaxTransactions`, `InsufficientAzoth`, `Failure_Throttled`,
/// `Failure_CrossWorld`).
pub mod response {
    /// The call was carried out.
    pub const SUCCESS: u32 = 1;
    /// The call was refused.
    pub const FAILURE: u32 = 2;
}

/// One season call of the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeasonsCall {
    /// 5645: stamp one square of an activity card.
    StampSquare {
        /// The header.
        header: FacetHeader,
        /// The card index (retail 26 and 29).
        card: u32,
        /// The square.
        square: [u8; SQUARE_LEN],
    },
    /// 5647: a new activity card.
    NewCard {
        /// The header.
        header: FacetHeader,
    },
    /// 5648: reroll the card's tasks.
    Reroll {
        /// The header.
        header: FacetHeader,
    },
    /// 5611: redeem one season reward.
    Redeem {
        /// The header.
        header: FacetHeader,
        /// The reward id.
        reward: u64,
    },
}

impl SeasonsCall {
    /// Decodes `body` when `type_index` is a season call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            let header = FacetHeader::read(reader)?;
            Ok(match type_index {
                STAMP_SQUARE_TYPE_INDEX => Self::StampSquare {
                    header,
                    card: reader.u32()?,
                    square: reader.array()?,
                },
                NEW_CARD_TYPE_INDEX => Self::NewCard { header },
                REROLL_TYPE_INDEX => Self::Reroll { header },
                _ => Self::Redeem {
                    header,
                    reward: reader.u64()?,
                },
            })
        };
        match type_index {
            STAMP_SQUARE_TYPE_INDEX
            | NEW_CARD_TYPE_INDEX
            | REROLL_TYPE_INDEX
            | REDEEM_REWARD_TYPE_INDEX => {
                let mut reader = Reader::new(body);
                Some(read(&mut reader).and_then(|call| {
                    reader.finish()?;
                    Ok(call)
                }))
            }
            _ => None,
        }
    }

    /// The answer to the call with `result` (a [`response`] value).
    pub const fn answer(&self, result: u32) -> SeasonsMessage {
        match *self {
            Self::StampSquare { header, .. } => SeasonsMessage::Response {
                type_index: STAMP_RESPONSE_TYPE_INDEX,
                header,
                result,
            },
            Self::NewCard { header } => SeasonsMessage::Response {
                type_index: NEW_CARD_RESPONSE_TYPE_INDEX,
                header,
                result,
            },
            Self::Reroll { header } => SeasonsMessage::Response {
                type_index: REROLL_RESPONSE_TYPE_INDEX,
                header,
                result,
            },
            Self::Redeem { header, reward } => SeasonsMessage::Redeemed {
                header,
                reward,
                result,
            },
        }
    }
}

/// One season answer of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeasonsMessage {
    /// 5686, 5687 or 5688.
    Response {
        /// The type index.
        type_index: u64,
        /// The header of the call.
        header: FacetHeader,
        /// A [`response`] value.
        result: u32,
    },
    /// 5629.
    Redeemed {
        /// The header of the call.
        header: FacetHeader,
        /// The reward id of the call.
        reward: u64,
        /// A [`response`] value.
        result: u32,
    },
}

impl SeasonsMessage {
    /// Decodes `body` when `type_index` is a season answer; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |reader: &mut Reader<'_>| -> Result<Self, MessageError> {
            let header = FacetHeader::read(reader)?;
            Ok(if type_index == REDEEM_RESPONSE_TYPE_INDEX {
                Self::Redeemed {
                    header,
                    reward: reader.u64()?,
                    result: reader.u32()?,
                }
            } else {
                Self::Response {
                    type_index,
                    header,
                    result: reader.u32()?,
                }
            })
        };
        match type_index {
            STAMP_RESPONSE_TYPE_INDEX
            | NEW_CARD_RESPONSE_TYPE_INDEX
            | REROLL_RESPONSE_TYPE_INDEX
            | REDEEM_RESPONSE_TYPE_INDEX => {
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
            Self::Response { type_index, .. } => *type_index,
            Self::Redeemed { .. } => REDEEM_RESPONSE_TYPE_INDEX,
        }
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::Response { header, result, .. } => {
                header.write(&mut writer);
                writer.u32(*result);
            }
            Self::Redeemed {
                header,
                reward,
                result,
            } => {
                header.write(&mut writer);
                writer.u64(*reward).u32(*result);
            }
        }
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::hex_bytes;

    const HEADER: &str = "e5fe5c07dc18f1c3e34e14f344a974cc";

    /// Retail `Hive_Seasons_Rewards` 23847 ms and 23885 ms: a stamp and its answer.
    #[test]
    fn a_retail_stamp_gets_the_retail_answer_form() {
        let call = SeasonsCall::decode(
            STAMP_SQUARE_TYPE_INDEX,
            &hex_bytes(&format!("{HEADER}0000001a00020001")),
        )
        .expect("a call")
        .expect("decodes");
        let SeasonsCall::StampSquare { card, square, .. } = call else {
            panic!("stamp");
        };
        assert_eq!((card, square), (26, [0, 2, 0, 1]));
        let answer = call.answer(response::SUCCESS);
        assert_eq!(answer.type_index(), STAMP_RESPONSE_TYPE_INDEX);
        assert_eq!(answer.encode(), hex_bytes(&format!("{HEADER}00000001")));
    }

    /// Retail `Hive_Seasons_Rewards` 49498 ms and 49636 ms: a redeem and its answer.
    #[test]
    fn a_retail_redeem_repeats_its_reward() {
        let call = SeasonsCall::decode(
            REDEEM_REWARD_TYPE_INDEX,
            &hex_bytes(&format!("{HEADER}a17b8b9a22215ad9")),
        )
        .expect("a call")
        .expect("decodes");
        let answer = call.answer(response::SUCCESS);
        let body = hex_bytes(&format!("{HEADER}a17b8b9a22215ad900000001"));
        assert_eq!(answer.encode(), body);
        assert_eq!(
            SeasonsMessage::decode(REDEEM_RESPONSE_TYPE_INDEX, &body),
            Some(Ok(answer))
        );
    }

    #[test]
    fn calls_without_arguments_have_no_body_after_the_header() {
        for type_index in [NEW_CARD_TYPE_INDEX, REROLL_TYPE_INDEX] {
            assert!(matches!(
                SeasonsCall::decode(type_index, &hex_bytes(HEADER)),
                Some(Ok(_))
            ));
            assert!(matches!(
                SeasonsCall::decode(type_index, &hex_bytes(&format!("{HEADER}00"))),
                Some(Err(_))
            ));
        }
    }
}
