// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client's conversation and board calls (conversation-notify note, sections 1 and 3). Each
//! body is the facet header and the arguments; a body of another length is refused.
//!
//! Conversation calls go to the player's `ConversationComponent` (header target less entity
//! `0x1c11_6d25_9b43_203d`, as 3772 and 794): the client asks for a topic when the player picks
//! it (3158; retail sends no answer) and names the topics it finished when the conversation
//! closes (5397; a topic that is the `TaskID` of an active `TaskConversationTopic` task completes
//! it, and retail shows that in the objectives state about 35 ms later).
//!
//! Board calls go to the player's `ObjectiveInteractorComponent` (target less entity
//! `0xb8d6_73af_8594_a7cc`) while a board is open (interaction kind 51).

use crate::facet::FacetHeader;
use crate::wire::{MessageError, Reader, Writer};

/// `ConversationComponentServerFacet_RequestConversationTopicFromClient`.
pub const REQUEST_TOPIC_TYPE_INDEX: u64 = 3158;
/// `ConversationComponentServerFacet_OnFinishedConversationTopicsFromClient`.
pub const FINISHED_TOPICS_TYPE_INDEX: u64 = 5397;
/// `ObjectiveInteractorComponentServerFacet_OnPurchaseItemRequest`.
pub const PURCHASE_ITEM_TYPE_INDEX: u64 = 3741;
/// `ObjectiveInteractorComponentServerFacet_OnObjectiveCompletionRequest`.
pub const COMPLETE_OBJECTIVE_TYPE_INDEX: u64 = 2461;
/// `ObjectiveInteractorComponentServerFacet_OnObjectiveSelectionRequest`.
pub const SELECT_OBJECTIVE_TYPE_INDEX: u64 = 3816;
/// `ObjectiveInteractorComponentServerFacet_OnPvpMissionSelectionRequest`.
pub const SELECT_PVP_MISSION_TYPE_INDEX: u64 = 3818;
/// `ObjectiveInteractorComponentServerFacet_OnCommunityGoalSelectionRequest`.
pub const SELECT_COMMUNITY_GOAL_TYPE_INDEX: u64 = 3819;

/// Bytes of one topic id.
const TOPIC_LEN: usize = 4;
const FIELD: &str = "finished topics";

/// One conversation call of the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationClientMsg {
    /// 3158: the player picked a topic.
    RequestTopic {
        /// The header.
        header: FacetHeader,
        /// CRC-32 of the lowercase `ConversationTopicID` (`conversation_topic_data`; 356 of 356
        /// retail ids found).
        topic: u32,
    },
    /// 5397: the conversation closed with these topics finished (retail: one topic in each of
    /// 354 bodies, the one of the 3158 before; sent with the 6705 cutscene end and the 2473
    /// interaction cancel).
    FinishedTopics {
        /// The header.
        header: FacetHeader,
        /// CRC-32 of each lowercase `ConversationTopicID`.
        topics: Vec<u32>,
    },
}

impl ConversationClientMsg {
    /// Decodes `body` when `type_index` is a conversation call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        Some(match type_index {
            REQUEST_TOPIC_TYPE_INDEX => read_call(body, |header, reader| {
                Ok(Self::RequestTopic {
                    header,
                    topic: reader.u32()?,
                })
            }),
            FINISHED_TOPICS_TYPE_INDEX => read_call(body, |header, reader| {
                let count = reader.wide_prefixed()?;
                // Each topic takes `TOPIC_LEN` bytes.
                if count != (reader.remaining() / TOPIC_LEN) as u64 {
                    return Err(MessageError::LengthMismatch { field: FIELD });
                }
                let topics = (0..count).map(|_| reader.u32()).collect::<Result<_, _>>()?;
                Ok(Self::FinishedTopics { header, topics })
            }),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::RequestTopic { .. } => REQUEST_TOPIC_TYPE_INDEX,
            Self::FinishedTopics { .. } => FINISHED_TOPICS_TYPE_INDEX,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::RequestTopic { header, topic } => {
                header.write(&mut writer);
                writer.u32(*topic);
            }
            Self::FinishedTopics { header, topics } => {
                header.write(&mut writer);
                // A `usize` count fits 64 bits.
                writer.wide_prefixed(topics.len() as u64);
                for topic in topics {
                    writer.u32(*topic);
                }
            }
        }
        writer.finish()
    }
}

/// One board call of the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectiveInteractorClientMsg {
    /// 2461: turn in a mission (retail 15 calls; the objectives state changes within 1 s).
    CompleteObjective {
        /// The header.
        header: FacetHeader,
        /// The mission instance (`MissionParams::instance` of the 3829 state).
        instance: u64,
    },
    /// 3816: take the offered mission at this index of the 3829 list (retail 0 to 2).
    SelectObjective {
        /// The header.
        header: FacetHeader,
        /// The index in `missionParams`.
        index: u8,
    },
    /// 3818: take the offered PvP mission (retail `FM_06_PVP_*` and `FM_10_PVP_*`).
    SelectPvpMission {
        /// The header.
        header: FacetHeader,
        /// CRC-32 of the lowercase `MissionID`.
        mission: u32,
    },
    /// 3819: take a town project mission of a community goal.
    SelectCommunityGoal {
        /// The header.
        header: FacetHeader,
        /// CRC-32 of the lowercase `MissionID`.
        mission: u32,
        /// CRC-32 of the lowercase `TerritoryProjectID` of that mission.
        project: u32,
    },
}

impl ObjectiveInteractorClientMsg {
    /// Decodes `body` when `type_index` is a board call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        Some(match type_index {
            COMPLETE_OBJECTIVE_TYPE_INDEX => read_call(body, |header, reader| {
                Ok(Self::CompleteObjective {
                    header,
                    instance: reader.u64()?,
                })
            }),
            SELECT_OBJECTIVE_TYPE_INDEX => read_call(body, |header, reader| {
                Ok(Self::SelectObjective {
                    header,
                    index: reader.u8()?,
                })
            }),
            SELECT_PVP_MISSION_TYPE_INDEX => read_call(body, |header, reader| {
                Ok(Self::SelectPvpMission {
                    header,
                    mission: reader.u32()?,
                })
            }),
            SELECT_COMMUNITY_GOAL_TYPE_INDEX => read_call(body, |header, reader| {
                Ok(Self::SelectCommunityGoal {
                    header,
                    mission: reader.u32()?,
                    project: reader.u32()?,
                })
            }),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::CompleteObjective { .. } => COMPLETE_OBJECTIVE_TYPE_INDEX,
            Self::SelectObjective { .. } => SELECT_OBJECTIVE_TYPE_INDEX,
            Self::SelectPvpMission { .. } => SELECT_PVP_MISSION_TYPE_INDEX,
            Self::SelectCommunityGoal { .. } => SELECT_COMMUNITY_GOAL_TYPE_INDEX,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::CompleteObjective { header, instance } => {
                header.write(&mut writer);
                writer.u64(*instance);
            }
            Self::SelectObjective { header, index } => {
                header.write(&mut writer);
                writer.u8(*index);
            }
            Self::SelectPvpMission { header, mission } => {
                header.write(&mut writer);
                writer.u32(*mission);
            }
            Self::SelectCommunityGoal {
                header,
                mission,
                project,
            } => {
                header.write(&mut writer);
                writer.u32(*mission).u32(*project);
            }
        }
        writer.finish()
    }
}

/// Reads the header, then the arguments with `read_rest`; the body must end there.
fn read_call<T>(
    body: &[u8],
    read_rest: impl FnOnce(FacetHeader, &mut Reader<'_>) -> Result<T, MessageError>,
) -> Result<T, MessageError> {
    let mut reader = Reader::new(body);
    let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
    reader.finish()?;
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header (synthetic ids).
    const HEADER: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    /// CRC-32 of `99a_grace_tasktopic_usestaff`.
    const USE_STAFF_TOPIC: u32 = 0xef2c_70f9;
    /// CRC-32 of `fm_c_hunt_0607`.
    const HUNT_MISSION: u32 = 0x66f9_81bb;
    /// CRC-32 of `cookingprojectid3`.
    const COOKING_PROJECT: u32 = 0x2f22_9df9;
    /// CRC-32 of `cookingprojectid_1_fishingsalmonlarget1_1`.
    const COOKING_MISSION: u32 = 0x469e_c58c;

    fn header() -> FacetHeader {
        FacetHeader::read(&mut Reader::new(&HEADER)).expect("header")
    }

    fn body(arguments: &[u8]) -> Vec<u8> {
        [&HEADER[..], arguments].concat()
    }

    #[test]
    fn the_conversation_calls_read_the_retail_layouts() {
        let request = body(&USE_STAFF_TOPIC.to_be_bytes());
        let message = ConversationClientMsg::decode(REQUEST_TOPIC_TYPE_INDEX, &request)
            .expect("a conversation call")
            .expect("reads");
        assert_eq!(
            message,
            ConversationClientMsg::RequestTopic {
                header: header(),
                topic: USE_STAFF_TOPIC,
            }
        );
        assert_eq!(message.encode(), request);
        // Retail 5397: a count of 1, then the topic (21 bytes).
        let finished = body(&[0x01, 0xef, 0x2c, 0x70, 0xf9]);
        let message = ConversationClientMsg::decode(FINISHED_TOPICS_TYPE_INDEX, &finished)
            .expect("a conversation call")
            .expect("reads");
        assert_eq!(
            message,
            ConversationClientMsg::FinishedTopics {
                header: header(),
                topics: vec![USE_STAFF_TOPIC],
            }
        );
        assert_eq!(message.type_index(), FINISHED_TOPICS_TYPE_INDEX);
        assert_eq!(message.encode(), finished);
        // A count that does not match the topics.
        assert!(matches!(
            ConversationClientMsg::decode(
                FINISHED_TOPICS_TYPE_INDEX,
                &body(&[0x02, 0xef, 0x2c, 0x70, 0xf9])
            ),
            Some(Err(MessageError::LengthMismatch { .. }))
        ));
        assert!(
            ConversationClientMsg::decode(crate::shop::PURCHASE_ITEM_TYPE_INDEX, &request)
                .is_none()
        );
    }

    #[test]
    fn the_board_calls_read_the_retail_layouts_and_write_them_back() {
        let calls = [
            ObjectiveInteractorClientMsg::CompleteObjective {
                header: header(),
                instance: 0x1122_3344_5566_7788,
            },
            ObjectiveInteractorClientMsg::SelectObjective {
                header: header(),
                index: 2,
            },
            ObjectiveInteractorClientMsg::SelectPvpMission {
                header: header(),
                mission: HUNT_MISSION,
            },
            ObjectiveInteractorClientMsg::SelectCommunityGoal {
                header: header(),
                mission: COOKING_MISSION,
                project: COOKING_PROJECT,
            },
        ];
        // Retail body lengths: 24, 17, 20 and 24 bytes.
        let lengths = [24, 17, 20, 24];
        for (call, length) in calls.into_iter().zip(lengths) {
            let bytes = call.encode();
            assert_eq!(bytes.len(), length);
            assert_eq!(
                ObjectiveInteractorClientMsg::decode(call.type_index(), &bytes),
                Some(Ok(call.clone()))
            );
            let mut long = bytes;
            long.push(0);
            assert!(matches!(
                ObjectiveInteractorClientMsg::decode(call.type_index(), &long),
                Some(Err(MessageError::TrailingBytes { .. }))
            ));
        }
        assert!(ObjectiveInteractorClientMsg::decode(REQUEST_TOPIC_TYPE_INDEX, &HEADER).is_none());
    }
}
