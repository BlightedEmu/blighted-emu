// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Amazon::Hub::ReplicatedStateBundle` (bodies note, section 8): the outer header only. Reader
//! `FUN_14646c2b0`, header `FUN_146b07870`. The payload bytes after the header are kept as they
//! are; [`crate::payload`] reads their members.
//!
//! The walk (`FUN_14175ccc0`) first applies the index block as `ReplicationControl` commands, then
//! hands each payload group to the entity state handler (`FUN_141717fc0`) with the low 16 bits of
//! the group integer as its `interestId`. One interest id is one entity slot of the client;
//! interest [`LOCAL_PLAYER_INTEREST`] is the client's own player (world-entry note, "Spawn
//! condition traced").

use crate::payload::{BundleGroup, BundlePayload};
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// File slot of `ReplicatedStateBundle`.
pub const REPLICATED_STATE_BUNDLE_TYPE_INDEX: u64 = 8;
/// Most `u16` values in the index block (`FUN_146a6ddb0`).
pub const MAX_BUNDLE_INDEXES: u64 = 100;
/// The interest id of the client's own player: `FUN_141717fc0` takes its "new local player
/// metadata" branch only for interest 1, and the own entity of every capture is in group 1.
pub const LOCAL_PLAYER_INTEREST: u16 = 1;
/// The most common header byte 4 in the captures; its meaning is Unknown.
pub const OBSERVED_BUNDLE_BYTE_4: u8 = 1;

/// The optional block of step 7: the `ReplicationControl` commands. A prefix-coded stop count,
/// then a list of big-endian `u16` interest ids: the first `stop_count` are stopped (the client
/// drops that entity slot), the rest are paused. In the captures the stop count always equals the
/// list length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexBlock {
    /// How many of [`IndexBlock::indexes`], from the front, are stops.
    pub stop_count: u64,
    /// The interest ids; at most [`MAX_BUNDLE_INDEXES`].
    pub indexes: Vec<u16>,
}

/// The header and the raw payload of one bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicatedStateBundle {
    /// Steps 1 and 2: a prefix-coded integer, present when the first byte is 1.
    pub leading: Option<u64>,
    /// Step 3: one byte.
    pub byte_3: u8,
    /// Step 4: one byte.
    pub byte_4: u8,
    /// Step 5: a 0 or 1 byte.
    pub flag_5: bool,
    /// Steps 6 and 7: present when the step 6 byte is 1.
    pub index_block: Option<IndexBlock>,
    /// Steps 8 and 9: the payload, after its prefix-coded length.
    pub payload: Vec<u8>,
}

impl ReplicatedStateBundle {
    /// A bundle with the captured header values, `sequence` as the leading integer,
    /// `client_context` as header byte 3 (the client-context id of the player's current level), the
    /// interest ids in `stops` stopped, and `groups` (one group per entity slot), in order.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a member cannot be encoded.
    pub fn of_groups(
        sequence: u64,
        client_context: u8,
        stops: Vec<u16>,
        groups: Vec<BundleGroup>,
    ) -> Result<Self, MessageError> {
        Ok(Self {
            leading: Some(sequence),
            byte_3: client_context,
            byte_4: OBSERVED_BUNDLE_BYTE_4,
            flag_5: true,
            index_block: Some(IndexBlock {
                stop_count: u64::try_from(stops.len()).unwrap_or(u64::MAX),
                indexes: stops,
            }),
            payload: BundlePayload { groups }.encode()?,
        })
    }
}

impl BodyCodec for ReplicatedStateBundle {
    const TYPE_INDEX: u64 = REPLICATED_STATE_BUNDLE_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer.boolean(self.leading.is_some());
        if let Some(leading) = self.leading {
            writer.prefixed(leading)?;
        }
        writer
            .u8(self.byte_3)
            .u8(self.byte_4)
            .boolean(self.flag_5)
            .boolean(self.index_block.is_some());
        if let Some(block) = &self.index_block {
            let count = u64::try_from(block.indexes.len()).unwrap_or(u64::MAX);
            if count > MAX_BUNDLE_INDEXES {
                return Err(MessageError::CountTooLarge {
                    count,
                    limit: MAX_BUNDLE_INDEXES,
                });
            }
            writer.prefixed(block.stop_count)?.prefixed(count)?;
            for index in &block.indexes {
                writer.u16(*index);
            }
        }
        let length = u64::try_from(self.payload.len()).unwrap_or(u64::MAX);
        writer.prefixed(length)?.bytes(&self.payload);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let leading = if reader.boolean()? {
            Some(reader.prefixed()?)
        } else {
            None
        };
        let byte_3 = reader.u8()?;
        let byte_4 = reader.u8()?;
        let flag_5 = reader.boolean()?;
        let index_block = if reader.boolean()? {
            let stop_count = reader.prefixed()?;
            let len = reader.prefixed()?;
            if len > MAX_BUNDLE_INDEXES {
                return Err(MessageError::CountTooLarge {
                    count: len,
                    limit: MAX_BUNDLE_INDEXES,
                });
            }
            let mut indexes = Vec::new();
            for _ in 0..len {
                indexes.push(reader.u16()?);
            }
            Some(IndexBlock {
                stop_count,
                indexes,
            })
        } else {
            None
        };
        // A length above the address space is cut off by the reader as a short body.
        let length = usize::try_from(reader.prefixed()?).unwrap_or(usize::MAX);
        let payload = reader.bytes(length)?.to_vec();
        reader.finish()?;
        Ok(Self {
            leading,
            byte_3,
            byte_4,
            flag_5,
            index_block,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::level_info::LOGIN_CLIENT_CONTEXT;
    use crate::payload::{BundleMember, MemberState};
    use crate::position::{PositionInTheWorldReplicatedState, WorldPosition};

    const PAYLOAD_LEN: usize = 300;
    const LEADING: u64 = 1000;
    /// One more than [`MAX_BUNDLE_INDEXES`], in one prefix byte.
    const TOO_MANY_INDEXES: u8 = 101;

    const SEQUENCE: u64 = 7;
    const COMPONENT: u64 = 16;
    const OTHER_INTEREST: u16 = 3;
    const STOPPED: [u16; 2] = [4, 9];

    fn group(interest: u16, x: f32) -> BundleGroup {
        BundleGroup {
            interest: u64::from(interest),
            members: vec![BundleMember {
                component: COMPONENT,
                state: MemberState::Position(PositionInTheWorldReplicatedState {
                    position: WorldPosition::from_metres(x, 0.0, 0.0),
                    ..PositionInTheWorldReplicatedState::default()
                }),
            }],
        }
    }

    #[test]
    fn a_bundle_of_groups_has_the_captured_header_the_stops_and_one_group_per_slot() {
        let groups = vec![
            group(LOCAL_PLAYER_INTEREST, 1.0),
            group(OTHER_INTEREST, 2.0),
        ];
        let bundle = ReplicatedStateBundle::of_groups(
            SEQUENCE,
            LOGIN_CLIENT_CONTEXT,
            STOPPED.to_vec(),
            groups.clone(),
        )
        .expect("bundle");
        assert_eq!(bundle.leading, Some(SEQUENCE));
        assert_eq!(
            (bundle.byte_3, bundle.byte_4, bundle.flag_5),
            (LOGIN_CLIENT_CONTEXT, OBSERVED_BUNDLE_BYTE_4, true)
        );
        assert_eq!(
            bundle.index_block,
            Some(IndexBlock {
                stop_count: STOPPED.len() as u64,
                indexes: STOPPED.to_vec()
            })
        );
        let payload = BundlePayload::decode(&bundle.payload).expect("payload");
        assert_eq!(payload.groups, groups);
    }

    fn full() -> ReplicatedStateBundle {
        ReplicatedStateBundle {
            leading: Some(LEADING),
            byte_3: 3,
            byte_4: 4,
            flag_5: true,
            index_block: Some(IndexBlock {
                stop_count: 2,
                indexes: vec![7, 0x0102],
            }),
            payload: vec![5; PAYLOAD_LEN],
        }
    }

    #[test]
    fn every_optional_part_round_trips() {
        let bundle = full();
        let body = bundle.encode().expect("encode");
        assert_eq!(ReplicatedStateBundle::decode(&body), Ok(bundle));
        let bare = ReplicatedStateBundle {
            leading: None,
            index_block: None,
            payload: Vec::new(),
            ..full()
        };
        let body = bare.encode().expect("encode");
        assert_eq!(body, [0, 3, 4, 1, 0, 0]);
        assert_eq!(ReplicatedStateBundle::decode(&body), Ok(bare));
    }

    #[test]
    fn too_many_indexes_and_short_payloads_are_errors() {
        let mut bundle = full();
        bundle.index_block = Some(IndexBlock {
            stop_count: 0,
            indexes: vec![0; MAX_BUNDLE_INDEXES as usize + 1],
        });
        assert!(matches!(
            bundle.encode(),
            Err(MessageError::CountTooLarge { .. })
        ));
        let body = full().encode().expect("encode");
        assert!(ReplicatedStateBundle::decode(&body[..body.len() - 1]).is_err());
        let mut flag = body.clone();
        flag[0] = 2;
        assert_eq!(
            ReplicatedStateBundle::decode(&flag),
            Err(MessageError::NotBoolean { value: 2 })
        );
    }

    #[test]
    fn a_huge_payload_length_is_a_short_body_not_an_allocation() {
        let body = [0, 3, 4, 0, 0, 0xF7, 0xFF, 0xFF, 0xFF, 0xFF];
        assert!(matches!(
            ReplicatedStateBundle::decode(&body),
            Err(MessageError::Truncated { .. })
        ));
    }

    #[test]
    fn an_index_count_above_the_limit_is_refused_on_decode() {
        let body = [0, 3, 4, 0, 1, 0, TOO_MANY_INDEXES];
        assert_eq!(
            ReplicatedStateBundle::decode(&body),
            Err(MessageError::CountTooLarge {
                count: u64::from(TOO_MANY_INDEXES),
                limit: MAX_BUNDLE_INDEXES
            })
        );
    }

    proptest! {
        #[test]
        fn random_bodies_never_panic(body in proptest::collection::vec(any::<u8>(), 0..64)) {
            let _ = ReplicatedStateBundle::decode(&body);
        }

        #[test]
        fn random_bytes_after_valid_flags_never_panic(
            leading in any::<bool>(),
            flag in any::<bool>(),
            has_block in any::<bool>(),
            tail in proptest::collection::vec(any::<u8>(), 0..256),
        ) {
            let mut body = vec![u8::from(leading)];
            body.extend_from_slice(&tail[..tail.len().min(1)]);
            body.extend_from_slice(&[3, 4, u8::from(flag), u8::from(has_block)]);
            body.extend_from_slice(&tail);
            let _ = ReplicatedStateBundle::decode(&body);
        }
    }
}
