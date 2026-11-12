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
use crate::items::FullList;
use crate::list_delta::{ReplicatedList, list_change, value_change, write_child};
use crate::payload::{BundleMember, MemberState};
use crate::slots::{
    SLOT_0, children_mask, read_slot_0, read_slot_byte, read_wide_children, write_children,
    write_wide_children,
};
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

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::StampSquare { .. } => STAMP_SQUARE_TYPE_INDEX,
            Self::NewCard { .. } => NEW_CARD_TYPE_INDEX,
            Self::Reroll { .. } => REROLL_TYPE_INDEX,
            Self::Redeem { .. } => REDEEM_REWARD_TYPE_INDEX,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::StampSquare {
                header,
                card,
                square,
            } => {
                header.write(&mut writer);
                writer.u32(*card).bytes(square);
            }
            Self::NewCard { header } | Self::Reroll { header } => header.write(&mut writer),
            Self::Redeem { header, reward } => {
                header.write(&mut writer);
                writer.u64(*reward);
            }
        }
        writer.finish()
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

/// The component of the player's `Javelin::SeasonsRewardsReplicatedState`.
pub const SEASONS_REWARDS_COMPONENT: u64 = 72;
/// Type index of `Javelin::SeasonsRewardsReplicatedState`.
pub const SEASONS_REWARDS_TYPE_INDEX: u64 = 5485;
/// The component of the player's `Javelin::SeasonsRewardsTrackedStatReplicatedState`.
pub const SEASONS_TRACKED_STATS_COMPONENT: u64 = 73;
/// Type index of `Javelin::SeasonsRewardsTrackedStatReplicatedState`.
pub const SEASONS_TRACKED_STATS_TYPE_INDEX: u64 = 5606;

/// The one slot of both season states.
const SLOTS: u8 = 1;
/// Children of the season rewards state's slot 0 (member layout table).
const REWARDS_CHILDREN: u8 = 20;
/// Children of the tracked stats state's slot 0.
const STATS_CHILDREN: u8 = 5;
const REWARDS_FIELD: &str = "season rewards";
const STATS_FIELD: &str = "season tracked stats";
/// The value of `IsInitialized` and `initialized` in every first state (Observed).
const INITIALIZED: u8 = 1;
/// `rewardClaimed`: 0 in each of the 3,970 retail bodies that carry it; its meaning is Unknown.
const REWARD_NOT_CLAIMED: u8 = 0;

/// Children of `Javelin::SeasonsRewardsReplicatedState` slot 0, in layout order.
mod rewards_child {
    pub const CARD_TEMPLATE: u8 = 0;
    pub const CLAIMED_TASKS: u8 = 1;
    pub const STAMPED_SQUARES: u8 = 2;
    pub const WILD_STAMPED_SQUARES: u8 = 3;
    pub const REWARD_CLAIMED: u8 = 4;
    pub const REROLL_COUNT: u8 = 5;
    pub const ACTIVITIES_TASKS: u8 = 6;
    pub const CARD_COUNT: u8 = 7;
    pub const WILD_STAMP_COUNT: u8 = 8;
    pub const WILD_STAMP_AWARD_BOUND: u8 = 9;
    pub const WILD_STAMP_AWARDS_THIS_SESSION: u8 = 10;
    pub const WILD_STAMP_AWARD_REMAINING: u8 = 11;
    pub const IS_INITIALIZED: u8 = 12;
    pub const SEASON_IDS: u8 = 13;
    pub const SEASON_BITMASK_COUNT: u8 = 14;
    pub const SEASON_XP_BY_SEASON: u8 = 15;
    pub const REDEEM_BITMASK: u8 = 16;
    pub const ESCROW_BITMASK: u8 = 17;
    pub const FOREIGN_ESCROW_BITMASK: u8 = 18;
}

/// Children of `Javelin::SeasonsRewardsTrackedStatReplicatedState` slot 0, in layout order.
mod stats_child {
    pub const INITIALIZED: u8 = 0;
    pub const GROUP_LIST: u8 = 1;
    pub const GROUP_COUNT_LIST: u8 = 2;
    pub const GROUP_STAT_INDEX: u8 = 3;
}

/// The season rewards state (`Javelin::SeasonsRewardsReplicatedState`, class 5485, own
/// component 72), every child of the member layout; a child that is `None` is not sent.
///
/// **Observed** (all 4,389 retail bodies of the capture index read and write back the same
/// bytes): the activity card is `cardTemplate` (the card's index, 0 to 9, among the season's
/// ten `SeasonsRewardsCardTemplates` cards), `claimedTasks` (16-bit words of a bit per task of
/// `activitiesTasks` that stamped a square), `stampedSquares` and `wildStampedSquares` (one
/// word: bit `row * 4 + column` of the 4 x 4 card; the wild bits are the squares a wild stamp
/// filled), `rerollCount`, `activitiesTasks` (the CRC-32 of the lowercase
/// `ActivitiesTaskID` of the card's 21 tasks), `cardCount` (the cards done) and
/// `wildStampCount` (at most 3). The season pass is per season: `SeasonIds` (the CRC-32 of the
/// lowercase `SeasonId`), `SeasonXpBySeason` (the season's total experience), `RedeemBitmask`
/// (64-bit words of a bit per redeemed `RewardIndex`), `EscrowBitmask` (a reward on its way) and
/// `SeasonBitmaskCount` (three word counts per season). `firstCharacterConnectTime` is Unix
/// nanoseconds. Unknown: `rewardClaimed` (always 0) and the three `wildStampAward*` children.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeasonsRewardsState {
    /// `cardTemplate`.
    pub card_template: Option<u8>,
    /// `claimedTasks`.
    pub claimed_tasks: Option<ReplicatedList<u16>>,
    /// `stampedSquares`.
    pub stamped_squares: Option<ReplicatedList<u16>>,
    /// `wildStampedSquares`.
    pub wild_stamped_squares: Option<ReplicatedList<u16>>,
    /// `rewardClaimed`.
    pub reward_claimed: Option<u8>,
    /// `rerollCount`.
    pub reroll_count: Option<u8>,
    /// `activitiesTasks`.
    pub activities_tasks: Option<Vec<u32>>,
    /// `cardCount`.
    pub card_count: Option<u16>,
    /// `wildStampCount`.
    pub wild_stamp_count: Option<u16>,
    /// `wildStampAwardBound`: Unix nanoseconds (Observed: always a midnight UTC).
    pub wild_stamp_award_bound: Option<u64>,
    /// `wildStampAwardsThisSession`.
    pub wild_stamp_awards_this_session: Option<u8>,
    /// `wildStampAwardRemaining`.
    pub wild_stamp_award_remaining: Option<u16>,
    /// `IsInitialized`.
    pub is_initialized: Option<u8>,
    /// `SeasonIds`.
    pub season_ids: Option<ReplicatedList<u32>>,
    /// `SeasonBitmaskCount`.
    pub season_bitmask_count: Option<ReplicatedList<u8>>,
    /// `SeasonXpBySeason`.
    pub season_xp: Option<ReplicatedList<u64>>,
    /// `RedeemBitmask`.
    pub redeem_bitmask: Option<ReplicatedList<u64>>,
    /// `EscrowBitmask`.
    pub escrow_bitmask: Option<ReplicatedList<u64>>,
    /// `ForeignEscrowBitmask`.
    pub foreign_escrow_bitmask: Option<ReplicatedList<u64>>,
    /// `firstCharacterConnectTime`: Unix nanoseconds.
    pub first_character_connect_time: Option<u64>,
}

/// Reads a list child of fixed-size elements.
fn list<T>(
    reader: &mut Reader<'_>,
    field: &'static str,
    read: impl FnMut(&mut Reader<'_>) -> Result<T, MessageError>,
) -> Result<Option<ReplicatedList<T>>, MessageError> {
    ReplicatedList::read(reader, field, read).map(Some)
}

impl SeasonsRewardsState {
    /// The present children, in child order.
    fn present(&self) -> [bool; REWARDS_CHILDREN as usize] {
        [
            self.card_template.is_some(),
            self.claimed_tasks.is_some(),
            self.stamped_squares.is_some(),
            self.wild_stamped_squares.is_some(),
            self.reward_claimed.is_some(),
            self.reroll_count.is_some(),
            self.activities_tasks.is_some(),
            self.card_count.is_some(),
            self.wild_stamp_count.is_some(),
            self.wild_stamp_award_bound.is_some(),
            self.wild_stamp_awards_this_session.is_some(),
            self.wild_stamp_award_remaining.is_some(),
            self.is_initialized.is_some(),
            self.season_ids.is_some(),
            self.season_bitmask_count.is_some(),
            self.season_xp.is_some(),
            self.redeem_bitmask.is_some(),
            self.escrow_bitmask.is_some(),
            self.foreign_escrow_bitmask.is_some(),
            self.first_character_connect_time.is_some(),
        ]
    }

    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another slot or child, or a list
    /// delta removes an element.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        use rewards_child as child;
        let mut state = Self::default();
        if read_slot_byte(reader, REWARDS_FIELD, SLOTS)? == 0 {
            return Ok(state);
        }
        let field = REWARDS_FIELD;
        read_wide_children(reader, field, REWARDS_CHILDREN, |reader, index| {
            match index {
                child::CARD_TEMPLATE => state.card_template = Some(reader.u8()?),
                child::CLAIMED_TASKS => {
                    state.claimed_tasks = list(reader, field, |reader| reader.u16())?
                }
                child::STAMPED_SQUARES => {
                    state.stamped_squares = list(reader, field, |reader| reader.u16())?;
                }
                child::WILD_STAMPED_SQUARES => {
                    state.wild_stamped_squares = list(reader, field, |reader| reader.u16())?;
                }
                child::REWARD_CLAIMED => state.reward_claimed = Some(reader.u8()?),
                child::REROLL_COUNT => state.reroll_count = Some(reader.u8()?),
                child::ACTIVITIES_TASKS => {
                    let count = reader.prefixed()?;
                    if count > reader.remaining() as u64 {
                        return Err(MessageError::LengthMismatch { field });
                    }
                    state.activities_tasks =
                        Some((0..count).map(|_| reader.u32()).collect::<Result<_, _>>()?);
                }
                child::CARD_COUNT => state.card_count = Some(reader.u16()?),
                child::WILD_STAMP_COUNT => state.wild_stamp_count = Some(reader.u16()?),
                child::WILD_STAMP_AWARD_BOUND => state.wild_stamp_award_bound = Some(reader.u64()?),
                child::WILD_STAMP_AWARDS_THIS_SESSION => {
                    state.wild_stamp_awards_this_session = Some(reader.u8()?);
                }
                child::WILD_STAMP_AWARD_REMAINING => {
                    state.wild_stamp_award_remaining = Some(reader.u16()?);
                }
                child::IS_INITIALIZED => state.is_initialized = Some(reader.u8()?),
                child::SEASON_IDS => state.season_ids = list(reader, field, |reader| reader.u32())?,
                child::SEASON_BITMASK_COUNT => {
                    state.season_bitmask_count = list(reader, field, |reader| reader.u8())?;
                }
                child::SEASON_XP_BY_SEASON => {
                    state.season_xp = list(reader, field, |reader| reader.u64())?
                }
                child::REDEEM_BITMASK => {
                    state.redeem_bitmask = list(reader, field, |reader| reader.u64())?
                }
                child::ESCROW_BITMASK => {
                    state.escrow_bitmask = list(reader, field, |reader| reader.u64())?
                }
                child::FOREIGN_ESCROW_BITMASK => {
                    state.foreign_escrow_bitmask = list(reader, field, |reader| reader.u64())?;
                }
                // The reader passes no child above the class's last.
                _ => state.first_character_connect_time = Some(reader.u64()?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state; a state without children writes an empty slot byte.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a task list longer than a prefix code holds.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        use rewards_child as child;
        let children = self
            .present()
            .iter()
            .enumerate()
            .filter(|(_, present)| **present)
            .fold(0_u32, |mask, (index, _)| mask | 1 << index);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_wide_children(writer, children, REWARDS_CHILDREN, |writer, index| {
            match index {
                child::CARD_TEMPLATE => writer.u8(self.card_template.unwrap_or_default()),
                child::CLAIMED_TASKS => return words_16(writer, self.claimed_tasks.as_ref()),
                child::STAMPED_SQUARES => return words_16(writer, self.stamped_squares.as_ref()),
                child::WILD_STAMPED_SQUARES => {
                    return words_16(writer, self.wild_stamped_squares.as_ref());
                }
                child::REWARD_CLAIMED => writer.u8(self.reward_claimed.unwrap_or_default()),
                child::REROLL_COUNT => writer.u8(self.reroll_count.unwrap_or_default()),
                child::ACTIVITIES_TASKS => {
                    let tasks = self.activities_tasks.as_deref().unwrap_or_default();
                    writer.prefixed(tasks.len() as u64)?;
                    for task in tasks {
                        writer.u32(*task);
                    }
                    writer
                }
                child::CARD_COUNT => writer.u16(self.card_count.unwrap_or_default()),
                child::WILD_STAMP_COUNT => writer.u16(self.wild_stamp_count.unwrap_or_default()),
                child::WILD_STAMP_AWARD_BOUND => {
                    writer.u64(self.wild_stamp_award_bound.unwrap_or_default())
                }
                child::WILD_STAMP_AWARDS_THIS_SESSION => {
                    writer.u8(self.wild_stamp_awards_this_session.unwrap_or_default())
                }
                child::WILD_STAMP_AWARD_REMAINING => {
                    writer.u16(self.wild_stamp_award_remaining.unwrap_or_default())
                }
                child::IS_INITIALIZED => writer.u8(self.is_initialized.unwrap_or_default()),
                child::SEASON_IDS => {
                    return write_child(writer, self.season_ids.as_ref(), |writer, id| {
                        writer.u32(*id);
                    });
                }
                child::SEASON_BITMASK_COUNT => {
                    return write_child(writer, self.season_bitmask_count.as_ref(), |writer, n| {
                        writer.u8(*n);
                    });
                }
                child::SEASON_XP_BY_SEASON => return words_64(writer, self.season_xp.as_ref()),
                child::REDEEM_BITMASK => return words_64(writer, self.redeem_bitmask.as_ref()),
                child::ESCROW_BITMASK => return words_64(writer, self.escrow_bitmask.as_ref()),
                child::FOREIGN_ESCROW_BITMASK => {
                    return words_64(writer, self.foreign_escrow_bitmask.as_ref());
                }
                // The mask holds no child above the class's last.
                _ => writer.u64(self.first_character_connect_time.unwrap_or_default()),
            };
            Ok(())
        })
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// As [`Self::write`].
    pub fn body(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.write(&mut writer)?;
        Ok(writer.finish())
    }

    /// The own member of the state (a recorded body: the bundle walk passes the class by its
    /// layout).
    ///
    /// # Errors
    ///
    /// As [`Self::write`].
    pub fn member(&self) -> Result<BundleMember, MessageError> {
        Ok(BundleMember {
            component: SEASONS_REWARDS_COMPONENT,
            state: MemberState::Recorded {
                class: SEASONS_REWARDS_TYPE_INDEX,
                body: self.body()?,
            },
        })
    }
}

/// Writes a list child of 16-bit words.
fn words_16(writer: &mut Writer, list: Option<&ReplicatedList<u16>>) -> Result<(), MessageError> {
    write_child(writer, list, |writer, word| {
        writer.u16(*word);
    })
}

/// Writes a list child of 64-bit words.
fn words_64(writer: &mut Writer, list: Option<&ReplicatedList<u64>>) -> Result<(), MessageError> {
    write_child(writer, list, |writer, word| {
        writer.u64(*word);
    })
}

/// The activity card's wild stamp award (`wildStampAwardBound`, `wildStampAwardsThisSession`,
/// `wildStampAwardRemaining`). Their rule is Unknown (Observed: the bound is the next midnight UTC
/// in every retail body; the session count is `0` to `2` or falls by one about every 150 s from
/// 255; the remaining count is 0 or 1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WildStampAward {
    /// `wildStampAwardBound`, Unix nanoseconds.
    pub bound_ns: u64,
    /// `wildStampAwardsThisSession`.
    pub this_session: u8,
    /// `wildStampAwardRemaining`.
    pub remaining: u16,
}

/// The season pass of one season, as the season rewards state holds it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeasonPassEntry {
    /// The CRC-32 of the lowercase `SeasonId`.
    pub season: u32,
    /// The season's total experience.
    pub experience: u64,
    /// The redeemed rewards: bit `n` of word `n / 64` is `RewardIndex` `n`.
    pub redeemed: Vec<u64>,
    /// The rewards on their way, in the same form.
    pub escrow: Vec<u64>,
}

/// What the client holds of the season rewards state, as plain values: the server's whole state
/// ([`Self::full`]) or the children that differ from what the client has ([`Self::change_from`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeasonRewardsValues {
    /// `cardTemplate`.
    pub card_template: u8,
    /// `claimedTasks`.
    pub claimed_tasks: Vec<u16>,
    /// `stampedSquares` (one word).
    pub stamped_squares: u16,
    /// `wildStampedSquares` (one word).
    pub wild_stamped_squares: u16,
    /// `rerollCount`.
    pub reroll_count: u8,
    /// `activitiesTasks`.
    pub activities_tasks: Vec<u32>,
    /// `cardCount`.
    pub card_count: u16,
    /// `wildStampCount`.
    pub wild_stamp_count: u16,
    /// The wild stamp award children.
    pub award: WildStampAward,
    /// The season passes, in `SeasonIds` order.
    pub seasons: Vec<SeasonPassEntry>,
    /// `firstCharacterConnectTime`, Unix nanoseconds.
    pub first_character_connect_ns: u64,
}

impl SeasonRewardsValues {
    fn season_ids(&self) -> Vec<u32> {
        self.seasons.iter().map(|entry| entry.season).collect()
    }

    /// `SeasonBitmaskCount`: three counts per season, redeemed words, escrow words and foreign
    /// escrow words (Observed: their sums are the lengths of `RedeemBitmask`, `EscrowBitmask` and
    /// `ForeignEscrowBitmask` in all 3,924 full retail lists; the foreign count is the redeemed
    /// count in 3,894 of them).
    fn bitmask_counts(&self) -> Vec<u8> {
        let count = |words: &[u64]| u8::try_from(words.len()).unwrap_or(u8::MAX);
        self.seasons
            .iter()
            .flat_map(|entry| {
                let redeemed = count(&entry.redeemed);
                [redeemed, count(&entry.escrow), redeemed]
            })
            .collect()
    }

    fn season_xp(&self) -> Vec<u64> {
        self.seasons.iter().map(|entry| entry.experience).collect()
    }

    fn redeemed(&self) -> Vec<u64> {
        self.seasons
            .iter()
            .flat_map(|entry| entry.redeemed.iter().copied())
            .collect()
    }

    fn escrow(&self) -> Vec<u64> {
        self.seasons
            .iter()
            .flat_map(|entry| entry.escrow.iter().copied())
            .collect()
    }

    /// Foreign escrow words: none is ever set here (Observed: all 0 in every retail body).
    fn foreign_escrow(&self) -> Vec<u64> {
        vec![0; self.redeemed().len()]
    }

    /// The whole state at list version `version`, as retail's first state of a player in game:
    /// every child but `EscrowBitmask`, which goes only while it has words (Observed: 3,674 of
    /// 3,924 full retail lists had no escrow words and no escrow child).
    pub fn full(&self, version: u64) -> SeasonsRewardsState {
        let escrow = self.escrow();
        SeasonsRewardsState {
            card_template: Some(self.card_template),
            claimed_tasks: whole_list(version, self.claimed_tasks.clone()),
            stamped_squares: whole_list(version, vec![self.stamped_squares]),
            wild_stamped_squares: whole_list(version, vec![self.wild_stamped_squares]),
            reward_claimed: Some(REWARD_NOT_CLAIMED),
            reroll_count: Some(self.reroll_count),
            activities_tasks: Some(self.activities_tasks.clone()),
            card_count: Some(self.card_count),
            wild_stamp_count: Some(self.wild_stamp_count),
            wild_stamp_award_bound: Some(self.award.bound_ns),
            wild_stamp_awards_this_session: Some(self.award.this_session),
            wild_stamp_award_remaining: Some(self.award.remaining),
            is_initialized: Some(INITIALIZED),
            season_ids: whole_list(version, self.season_ids()),
            season_bitmask_count: whole_list(version, self.bitmask_counts()),
            season_xp: whole_list(version, self.season_xp()),
            redeem_bitmask: whole_list(version, self.redeemed()),
            escrow_bitmask: if escrow.is_empty() {
                None
            } else {
                whole_list(version, escrow)
            },
            foreign_escrow_bitmask: whole_list(version, self.foreign_escrow()),
            first_character_connect_time: Some(self.first_character_connect_ns),
        }
    }

    /// The children that differ from `before`, what the client has, at list version `version`;
    /// `None` when none does. The card's word lists and the escrow words go whole, the lists by
    /// season as their changed entries (Observed: `Hive_Seasons_Rewards`, a stamp sent
    /// `stampedSquares` whole, a redeem `SeasonBitmaskCount` and `SeasonXpBySeason` as one
    /// changed entry and `EscrowBitmask` whole).
    pub fn change_from(&self, before: &Self, version: u64) -> Option<SeasonsRewardsState> {
        let whole_if = |now: Vec<u16>, then: Vec<u16>| {
            if now == then {
                None
            } else {
                whole_list(version, now)
            }
        };
        let differs = |now: u64, then: u64| (now != then).then_some(now);
        let (escrow, escrow_before) = (self.escrow(), before.escrow());
        let state = SeasonsRewardsState {
            card_template: value_change(Some(&before.card_template), Some(&self.card_template)),
            claimed_tasks: whole_if(self.claimed_tasks.clone(), before.claimed_tasks.clone()),
            stamped_squares: whole_if(vec![self.stamped_squares], vec![before.stamped_squares]),
            wild_stamped_squares: whole_if(
                vec![self.wild_stamped_squares],
                vec![before.wild_stamped_squares],
            ),
            reroll_count: value_change(Some(&before.reroll_count), Some(&self.reroll_count)),
            activities_tasks: (self.activities_tasks != before.activities_tasks)
                .then(|| self.activities_tasks.clone()),
            card_count: value_change(Some(&before.card_count), Some(&self.card_count)),
            wild_stamp_count: value_change(
                Some(&before.wild_stamp_count),
                Some(&self.wild_stamp_count),
            ),
            wild_stamp_award_bound: differs(self.award.bound_ns, before.award.bound_ns),
            wild_stamp_awards_this_session: value_change(
                Some(&before.award.this_session),
                Some(&self.award.this_session),
            ),
            wild_stamp_award_remaining: value_change(
                Some(&before.award.remaining),
                Some(&self.award.remaining),
            ),
            season_ids: list_change(
                Some(before.season_ids().as_slice()),
                &self.season_ids(),
                version,
            ),
            season_bitmask_count: list_change(
                Some(before.bitmask_counts().as_slice()),
                &self.bitmask_counts(),
                version,
            ),
            season_xp: list_change(
                Some(before.season_xp().as_slice()),
                &self.season_xp(),
                version,
            ),
            redeem_bitmask: list_change(
                Some(before.redeemed().as_slice()),
                &self.redeemed(),
                version,
            ),
            escrow_bitmask: if escrow == escrow_before {
                None
            } else {
                whole_list(version, escrow)
            },
            foreign_escrow_bitmask: list_change(
                Some(before.foreign_escrow().as_slice()),
                &self.foreign_escrow(),
                version,
            ),
            first_character_connect_time: differs(
                self.first_character_connect_ns,
                before.first_character_connect_ns,
            ),
            ..SeasonsRewardsState::default()
        };
        (state != SeasonsRewardsState::default()).then_some(state)
    }
}

/// The whole list `elements` at list version `version`.
fn whole_list<T>(version: u64, elements: Vec<T>) -> Option<ReplicatedList<T>> {
    Some(ReplicatedList::Full(FullList {
        version: Some(version),
        elements,
    }))
}

/// The tracked stats state (`Javelin::SeasonsRewardsTrackedStatReplicatedState`, class 5606,
/// own component 73): every child of the member layout; a child that is `None` is not sent.
///
/// **Observed** (all 21,919 retail bodies of the capture index read and write back the same
/// bytes): `groupList` names each group of tracked stats (the CRC-32 of the lowercase `SeasonId`
/// for the season's journey, `ActivitiesCurrentTasks` for the activity card's tasks),
/// `groupCountList` the stats of each group, and `groupStatIndex` and `groupStatValue` the stats
/// of all groups in group order: a stat's index is its place among every `SeasonsRewardsStats`
/// row sorted by the CRC-32 of the lowercase `TrackedStatId` (Observed: the 30 journey stats and
/// the 21 card task stats of three ledgers are those of the content's season 10 tasks, ascending;
/// `CompleteActivityCard`, index 189, rose by one at each new card).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeasonsTrackedStatsState {
    /// `initialized`.
    pub initialized: Option<u8>,
    /// `groupList`.
    pub group_list: Option<ReplicatedList<u32>>,
    /// `groupCountList`.
    pub group_count_list: Option<ReplicatedList<u16>>,
    /// `groupStatIndex`.
    pub group_stat_index: Option<ReplicatedList<u16>>,
    /// `groupStatValue`.
    pub group_stat_value: Option<ReplicatedList<u32>>,
}

impl SeasonsTrackedStatsState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another slot or child, or a list
    /// delta removes an element.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        use stats_child as child;
        let mut state = Self::default();
        let field = STATS_FIELD;
        read_slot_0(reader, field, STATS_CHILDREN, |reader, index| {
            match index {
                child::INITIALIZED => state.initialized = Some(reader.u8()?),
                child::GROUP_LIST => state.group_list = list(reader, field, |reader| reader.u32())?,
                child::GROUP_COUNT_LIST => {
                    state.group_count_list = list(reader, field, |reader| reader.u16())?;
                }
                child::GROUP_STAT_INDEX => {
                    state.group_stat_index = list(reader, field, |reader| reader.u16())?;
                }
                // The reader passes no child above the class's last.
                _ => state.group_stat_value = list(reader, field, |reader| reader.u32())?,
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state; a state without children writes an empty slot byte.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] from the list writer.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        use stats_child as child;
        let children = children_mask(&[
            self.initialized.is_some(),
            self.group_list.is_some(),
            self.group_count_list.is_some(),
            self.group_stat_index.is_some(),
            self.group_stat_value.is_some(),
        ]);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_children(
            writer,
            children,
            STATS_CHILDREN,
            |writer, index| match index {
                child::INITIALIZED => {
                    writer.u8(self.initialized.unwrap_or_default());
                    Ok(())
                }
                child::GROUP_LIST => write_child(writer, self.group_list.as_ref(), |writer, id| {
                    writer.u32(*id);
                }),
                child::GROUP_COUNT_LIST => words_16(writer, self.group_count_list.as_ref()),
                child::GROUP_STAT_INDEX => words_16(writer, self.group_stat_index.as_ref()),
                // The mask holds no child above the class's last.
                _ => write_child(writer, self.group_stat_value.as_ref(), |writer, value| {
                    writer.u32(*value);
                }),
            },
        )
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// As [`Self::write`].
    pub fn body(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.write(&mut writer)?;
        Ok(writer.finish())
    }

    /// The own member of the state (a recorded body, as [`SeasonsRewardsState::member`]).
    ///
    /// # Errors
    ///
    /// As [`Self::write`].
    pub fn member(&self) -> Result<BundleMember, MessageError> {
        Ok(BundleMember {
            component: SEASONS_TRACKED_STATS_COMPONENT,
            state: MemberState::Recorded {
                class: SEASONS_TRACKED_STATS_TYPE_INDEX,
                body: self.body()?,
            },
        })
    }
}

/// One group of tracked stats: its id and each stat's index and value, by ascending index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatGroup {
    /// The group id (module doc of [`SeasonsTrackedStatsState`]).
    pub group: u32,
    /// Each stat's index and value.
    pub stats: Vec<(u16, u32)>,
}

/// What the client holds of the tracked stats state, as plain values ([`Self::full`],
/// [`Self::change_from`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackedStatsValues {
    /// The groups, in `groupList` order.
    pub groups: Vec<StatGroup>,
}

impl TrackedStatsValues {
    fn group_ids(&self) -> Vec<u32> {
        self.groups.iter().map(|group| group.group).collect()
    }

    fn group_counts(&self) -> Vec<u16> {
        self.groups
            .iter()
            .map(|group| u16::try_from(group.stats.len()).unwrap_or(u16::MAX))
            .collect()
    }

    fn indexes(&self) -> Vec<u16> {
        self.groups
            .iter()
            .flat_map(|group| group.stats.iter().map(|(index, _)| *index))
            .collect()
    }

    fn values(&self) -> Vec<u32> {
        self.groups
            .iter()
            .flat_map(|group| group.stats.iter().map(|(_, value)| *value))
            .collect()
    }

    /// The whole state at list version `version` (retail's first state of a player in game).
    pub fn full(&self, version: u64) -> SeasonsTrackedStatsState {
        SeasonsTrackedStatsState {
            initialized: Some(INITIALIZED),
            group_list: whole_list(version, self.group_ids()),
            group_count_list: whole_list(version, self.group_counts()),
            group_stat_index: whole_list(version, self.indexes()),
            group_stat_value: whole_list(version, self.values()),
        }
    }

    /// The lists that differ from `before`, what the client has, as their changed entries at
    /// list version `version`; `None` when none does (Observed: `Hive_Seasons_Rewards` 214.986,
    /// a new card sent the card group's changed indexes and the values it zeroed).
    pub fn change_from(&self, before: &Self, version: u64) -> Option<SeasonsTrackedStatsState> {
        let state = SeasonsTrackedStatsState {
            initialized: None,
            group_list: list_change(
                Some(before.group_ids().as_slice()),
                &self.group_ids(),
                version,
            ),
            group_count_list: list_change(
                Some(before.group_counts().as_slice()),
                &self.group_counts(),
                version,
            ),
            group_stat_index: list_change(
                Some(before.indexes().as_slice()),
                &self.indexes(),
                version,
            ),
            group_stat_value: list_change(
                Some(before.values().as_slice()),
                &self.values(),
                version,
            ),
        };
        (state != SeasonsTrackedStatsState::default()).then_some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::hex_bytes;

    const HEADER: &str = "e5fe5c07dc18f1c3e34e14f344a974cc";

    /// Retail `Hive_Seasons_Rewards` 23847 ms and 23885 ms: a stamp and its answer.
    /// Decodes the call of `type_index` in `body` and checks that it names that index and
    /// writes back `body`.
    fn decode_call(type_index: u64, body: &[u8]) -> SeasonsCall {
        let call = SeasonsCall::decode(type_index, body)
            .expect("a call")
            .expect("decodes");
        assert_eq!(call.type_index(), type_index);
        assert_eq!(call.encode(), body);
        call
    }

    #[test]
    fn a_retail_stamp_gets_the_retail_answer_form() {
        let call = decode_call(
            STAMP_SQUARE_TYPE_INDEX,
            &hex_bytes(&format!("{HEADER}0000001a00020001")),
        );
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
        let call = decode_call(
            REDEEM_REWARD_TYPE_INDEX,
            &hex_bytes(&format!("{HEADER}a17b8b9a22215ad9")),
        );
        let answer = call.answer(response::SUCCESS);
        let body = hex_bytes(&format!("{HEADER}a17b8b9a22215ad900000001"));
        assert_eq!(answer.encode(), body);
        assert_eq!(
            SeasonsMessage::decode(REDEEM_RESPONSE_TYPE_INDEX, &body),
            Some(Ok(answer))
        );
    }

    #[test]
    fn every_season_call_writes_what_it_reads() {
        let header = FacetHeader {
            target: 0xe5fe_5c07_dc18_f1c3,
            entity: 0xe34e_14f3_44a9_74cc,
        };
        let calls = [
            SeasonsCall::StampSquare {
                header,
                card: 29,
                square: [0x11, 1, 0, 0],
            },
            SeasonsCall::NewCard { header },
            SeasonsCall::Reroll { header },
            SeasonsCall::Redeem {
                header,
                reward: 0xa17b_8b9a_2221_5ad9,
            },
        ];
        for call in calls {
            assert_eq!(
                SeasonsCall::decode(call.type_index(), &call.encode()),
                Some(Ok(call))
            );
        }
    }

    #[test]
    fn calls_without_arguments_have_no_body_after_the_header() {
        for type_index in [NEW_CARD_TYPE_INDEX, REROLL_TYPE_INDEX] {
            decode_call(type_index, &hex_bytes(HEADER));
            assert!(matches!(
                SeasonsCall::decode(type_index, &hex_bytes(&format!("{HEADER}00"))),
                Some(Err(_))
            ));
        }
    }

    /// Retail newchar 2026-09-18 at 9.06 s: the first full season rewards state (list version 6).
    const RETAIL_REWARDS_LOGIN: &str = "01ff01000106010001000106010002000106010000000015abb91fb735dd8a146993160a64eae98b199d1dcef33830039475c5d294927dffe372f544acd4dbae4fc7ba4817226ab751b14f10843f0095e4f889708e2b3b0d0d7c94686e9a2d5838c08adedbd3eb38f92c0b9bff000a000318e0ff8560030000ff0001010001060222215ad9c228718c370001060604000400000000010602000000000001a4a3000000000000000000010604000000ffffffffffffffffe0000000000000000000000fff00001100df006e0000010604000000000000000000000000000000000000000000000000000000000000000018703ee352008884";
    /// Its tracked stats state, in the same bundle.
    const RETAIL_STATS_LOGIN: &str = "011f010001060222215ad98d0024e900010602001e001500010633001400220027004f00530069006e0075007e0089008e009f00a200aa00b300b900bd00d700df00fd0102010d01230132014701480150015c015f0175001f00400048004e0058005c006a008c009400bf00da00ee00f100fc0109011a014001420151015b01750001063300000000000000000000000000000000000000b400000000000000000000000000000000000000000000000c000000b7000000050000000000000002000000000000000a0000000000000001000001ba000000000000000a000000160000018a0000000100000000000000000000000000000000000000000000028a0000001200000000000000060000000600000000000000000000000000000000000000000000000000000000000000cf0000000000000000000000190000000000000000000000000000000000000000";
    /// `Hive_Seasons_Rewards` 202.288: a wild stamp of square 7 (list version 503).
    const RETAIL_WILD_STAMP: &str = "010c0001b707019fe90001b707018fc9";
    /// 214.986: a new card (template 5, the card's tasks, `cardCount` 6).
    const RETAIL_NEW_CARD: &str = "01cf050001b50b000001b50b0100000001b50b01000015199d1dce9475c5d2843f0095abb91fb74fc7ba48f7937ce2f3383003dcbe2f21d6ceebf235dd8a14e372f54451b14f1080944c7432b04e0d38c08adeacd4dbae70ecd800f948f4d014801d040d7c9468a5629785010006";
    /// 215.535: the card group's stats of that new card.
    const RETAIL_NEW_CARD_STATS: &str = "011812ff2701800c0048290100006a2a010000bf2b010000d12c010000da2d010000e12e010000ee2f010000f1ff30010000fa31010000fc3201000109330100010a3401000123350100012e36010001393701000142033801000150390100015b063f1001800c0000000628010000000000290100000000002d0100000000003601000000000037010000000000";
    /// 222.934: a reroll.
    const RETAIL_REROLL: &str = "0160011535dd8a148e4fc446843f00951ef0d9d78e2b3b0d8946ff14e372f54469f7e9416993160a8d894cbe64eae98b38c08ade0d7c9468abb91fb7f33830037a9c1cd34fc7ba48f7937ce293ffb9e6f0feb8fb70ecd800";
    /// 226.286, 226.336 and 226.384: the redeem of reward index 26.
    const RETAIL_REDEEM: [&str; 3] = [
        "0180800b01010101970f0101010001970f00000000000119090001970f010000000004000000",
        "0180800f01010101970f0101010001970f000000000001190901010001980f0000000007ffffff0001980f010000000000000000",
        "0180800c01010001980f0000000007ffffff0001980f010000000000000000",
    ];
    const LOGIN_VERSION: u64 = 6;
    /// `Season10` and `Season3`.
    const SEASON_10: u32 = 0x2221_5ad9;
    const SEASON_3: u32 = 0xc228_718c;
    /// `ActivitiesCurrentTasks`.
    const CARD_GROUP: u32 = 0x8d00_24e9;

    fn rewards(hex: &str) -> SeasonsRewardsState {
        let body = hex_bytes(hex);
        let mut reader = Reader::new(&body);
        let state = SeasonsRewardsState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        assert_eq!(state.body().expect("write"), body, "{hex}");
        state
    }

    fn stats(hex: &str) -> SeasonsTrackedStatsState {
        let body = hex_bytes(hex);
        let mut reader = Reader::new(&body);
        let state = SeasonsTrackedStatsState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        assert_eq!(state.body().expect("write"), body, "{hex}");
        state
    }

    /// The values of [`RETAIL_REWARDS_LOGIN`].
    fn login_values() -> SeasonRewardsValues {
        SeasonRewardsValues {
            card_template: 1,
            claimed_tasks: vec![1],
            stamped_squares: 2,
            wild_stamped_squares: 0,
            reroll_count: 0,
            activities_tasks: vec![
                0xabb9_1fb7,
                0x35dd_8a14,
                0x6993_160a,
                0x64ea_e98b,
                0x199d_1dce,
                0xf338_3003,
                0x9475_c5d2,
                0x9492_7dff,
                0xe372_f544,
                0xacd4_dbae,
                0x4fc7_ba48,
                0x1722_6ab7,
                0x51b1_4f10,
                0x843f_0095,
                0xe4f8_8970,
                0x8e2b_3b0d,
                0x0d7c_9468,
                0x6e9a_2d58,
                0x38c0_8ade,
                0xdbd3_eb38,
                0xf92c_0b9b,
            ],
            card_count: 10,
            wild_stamp_count: 3,
            award: WildStampAward {
                bound_ns: 0x18e0_ff85_6003_0000,
                this_session: 0xff,
                remaining: 1,
            },
            seasons: vec![
                SeasonPassEntry {
                    season: SEASON_10,
                    experience: 0x0001_a4a3,
                    redeemed: vec![
                        0x0000_00ff_ffff_ffff,
                        0xffff_ffe0_0000_0000,
                        0x0000_0000_0000_0fff,
                        0x0000_1100_df00_6e00,
                    ],
                    escrow: Vec::new(),
                },
                SeasonPassEntry {
                    season: SEASON_3,
                    ..SeasonPassEntry::default()
                },
            ],
            first_character_connect_ns: 0x1870_3ee3_5200_8884,
        }
    }

    #[test]
    fn the_retail_season_rewards_bodies_write_back() {
        rewards(RETAIL_REWARDS_LOGIN);
        let stamp = rewards(RETAIL_WILD_STAMP);
        assert!(stamp.card_template.is_none() && stamp.stamped_squares.is_some());
        let new_card = rewards(RETAIL_NEW_CARD);
        assert_eq!(new_card.card_count, Some(6));
        assert_eq!(new_card.activities_tasks.map(|tasks| tasks.len()), Some(21));
        assert_eq!(rewards(RETAIL_REROLL).reroll_count, Some(1));
        for body in RETAIL_REDEEM {
            rewards(body);
        }
        assert_eq!(rewards("00"), SeasonsRewardsState::default());
    }

    #[test]
    fn the_retail_tracked_stats_bodies_write_back() {
        let login = stats(RETAIL_STATS_LOGIN);
        assert_eq!(login.initialized, Some(INITIALIZED));
        let new_card = stats(RETAIL_NEW_CARD_STATS);
        assert!(new_card.group_list.is_none() && new_card.group_stat_index.is_some());
    }

    #[test]
    fn the_whole_state_of_the_retail_values_is_the_retail_body() {
        assert_eq!(
            login_values().full(LOGIN_VERSION).body().expect("body"),
            hex_bytes(RETAIL_REWARDS_LOGIN)
        );
        let member = login_values().full(LOGIN_VERSION).member().expect("member");
        assert_eq!(member.component, SEASONS_REWARDS_COMPONENT);
        assert_eq!(member.state.type_index(), SEASONS_REWARDS_TYPE_INDEX);
    }

    #[test]
    fn a_stamp_and_a_redeem_change_what_retail_changed() {
        // The wild stamp of square 7 at 202.288 (before it: 40809 and 36681).
        let mut before = login_values();
        before.stamped_squares = 40_809;
        before.wild_stamped_squares = 36_681;
        let mut after = before.clone();
        after.stamped_squares = 40_937;
        after.wild_stamped_squares = 36_809;
        let change = after.change_from(&before, 503).expect("a change");
        assert_eq!(change.body().expect("body"), hex_bytes(RETAIL_WILD_STAMP));
        assert_eq!(after.change_from(&after, 504), None);
        // The redeem of index 26 at 226.384: the word with bit 26 and the escrow word cleared.
        let one_season = |redeemed: u64, escrow: u64| SeasonRewardsValues {
            seasons: vec![SeasonPassEntry {
                season: SEASON_10,
                experience: 71_945,
                redeemed: vec![redeemed],
                escrow: vec![escrow],
            }],
            ..SeasonRewardsValues::default()
        };
        let change = one_season(0x07ff_ffff, 0)
            .change_from(&one_season(0x03ff_ffff, 1 << 26), 984)
            .expect("a change");
        assert_eq!(change.body().expect("body"), hex_bytes(RETAIL_REDEEM[2]));
    }

    #[test]
    fn the_tracked_stats_values_send_whole_then_their_changes() {
        let values = TrackedStatsValues {
            groups: vec![
                StatGroup {
                    group: SEASON_10,
                    stats: vec![(20, 0), (189, 5)],
                },
                StatGroup {
                    group: CARD_GROUP,
                    stats: vec![(31, 0)],
                },
            ],
        };
        let full = values.full(LOGIN_VERSION);
        assert_eq!(
            stats(&hex(&full.body().expect("body"))),
            full,
            "the whole state reads back"
        );
        let mut after = values.clone();
        after.groups[0].stats[1].1 = 6;
        after.groups[1].stats[0] = (72, 0);
        let change = after.change_from(&values, 768).expect("a change");
        assert_eq!(change.group_list, None);
        assert_eq!(
            change.group_stat_index,
            crate::list_delta::child_changes(768, [(2, 72)])
        );
        assert_eq!(
            change.group_stat_value,
            crate::list_delta::child_changes(768, [(1, 6)])
        );
        assert_eq!(after.change_from(&after, 769), None);
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
