// SPDX-License-Identifier: AGPL-3.0-or-later
//! The members inside a `ReplicatedStateBundle` payload (replication note section 1). Walk
//! `FUN_14175ccc0`, group `FUN_146af20d0`, member `FUN_146af2340`.
//!
//! A payload is groups until its end. A group is a prefix-coded integer, a `u8` member count, and
//! the members. A member is a prefix-coded integer, a type id (0 adds 16 raw bytes), and the body
//! of its class. A body has no length, so a member of a class without a reader here ends the walk.

use std::ops::Range;

use nw_reflect::{TypeId, decode_type_id, encode_type_id};

use crate::gde::{GDE_METADATA_TYPE_INDEX, GdeMetadataReplicatedState};
use crate::position::{POSITION_IN_THE_WORLD_TYPE_INDEX, PositionInTheWorldReplicatedState};
use crate::vitals::{VITALS_TYPE_INDEX, VitalsSlot0};
use crate::wire::{MessageError, Reader, Writer};

/// The state of one member, by class.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberState {
    /// `MB::PositionInTheWorldReplicatedState`.
    Position(PositionInTheWorldReplicatedState),
    /// `MB::GdeMetadataReplicatedState`: the spawn card.
    GdeMetadata(GdeMetadataReplicatedState),
    /// `MB::VitalsComponentReplicatedState`, slot 0.
    Vitals(VitalsSlot0),
}

impl MemberState {
    /// The type index of this state's class.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::Position(_) => POSITION_IN_THE_WORLD_TYPE_INDEX,
            Self::GdeMetadata(_) => GDE_METADATA_TYPE_INDEX,
            Self::Vitals(_) => VITALS_TYPE_INDEX,
        }
    }

    /// The class type id of this state.
    pub const fn class(&self) -> TypeId {
        TypeId::FileSlot(self.type_index())
    }

    fn read(class: TypeId, reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        match class {
            TypeId::FileSlot(POSITION_IN_THE_WORLD_TYPE_INDEX) => {
                PositionInTheWorldReplicatedState::read(reader).map(Self::Position)
            }
            TypeId::FileSlot(GDE_METADATA_TYPE_INDEX) => {
                GdeMetadataReplicatedState::read(reader).map(Self::GdeMetadata)
            }
            TypeId::FileSlot(VITALS_TYPE_INDEX) => VitalsSlot0::read(reader).map(Self::Vitals),
            _ => Err(MessageError::UnknownMemberClass { class }),
        }
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        match self {
            Self::Position(state) => state.write(writer),
            Self::GdeMetadata(state) => {
                state.write(writer);
                Ok(())
            }
            Self::Vitals(state) => {
                state.write(writer);
                Ok(())
            }
        }
    }
}

/// One member: one replicated state of one entity.
#[derive(Debug, Clone, PartialEq)]
pub struct BundleMember {
    /// The first prefix-coded integer. The code does not name it.
    pub u64_at_0: u64,
    /// The state.
    pub state: MemberState,
}

impl BundleMember {
    /// The member bytes: the integer, the class type id, and the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Prefix`] when the integer does not fit the prefix code.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        writer
            .prefixed(self.u64_at_0)?
            .bytes(&encode_type_id(&self.state.class())?);
        self.state.write(&mut writer)?;
        Ok(writer.finish())
    }
}

/// One group of members.
#[derive(Debug, Clone, PartialEq)]
pub struct BundleGroup {
    /// The group's prefix-coded integer. The client keeps its low 16 bits.
    pub leading: u64,
    /// The members; at most `u8::MAX`.
    pub members: Vec<BundleMember>,
}

/// A whole payload of members whose classes all have a reader here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BundlePayload {
    /// The groups, in order.
    pub groups: Vec<BundleGroup>,
}

/// The members at the front of a payload that have a reader here, with the byte range of each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KnownMembers {
    /// The members and their bytes in the payload.
    pub members: Vec<(BundleMember, Range<usize>)>,
    /// The class of the first member without a reader; `None` when the whole payload was read.
    pub stopped_at: Option<TypeId>,
}

impl BundlePayload {
    /// The payload bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a group of more than `u8::MAX` members, and
    /// [`MessageError::Prefix`] for an integer that does not fit the prefix code.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        for group in &self.groups {
            let count =
                u8::try_from(group.members.len()).map_err(|_| MessageError::CountTooLarge {
                    count: u64::try_from(group.members.len()).unwrap_or(u64::MAX),
                    limit: u64::from(u8::MAX),
                })?;
            writer.prefixed(group.leading)?.u8(count);
            for member in &group.members {
                writer.bytes(&member.encode()?);
            }
        }
        Ok(writer.finish())
    }

    /// Reads a payload whose members all have a reader here.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::UnknownMemberClass`] for a member without a reader, and another
    /// [`MessageError`] when the bytes do not match the layout.
    pub fn decode(payload: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(payload);
        let mut groups = Vec::new();
        while !reader.is_empty() {
            let leading = reader.prefixed()?;
            let count = reader.u8()?;
            let mut members = Vec::new();
            for _ in 0..count {
                members.push(read_member(&mut reader)?);
            }
            groups.push(BundleGroup { leading, members });
        }
        Ok(Self { groups })
    }

    /// Reads the members at the front of a payload until the end or until a member of a class
    /// without a reader.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the bytes before that point do not match the layout.
    pub fn decode_known(payload: &[u8]) -> Result<KnownMembers, MessageError> {
        let mut reader = Reader::new(payload);
        let mut known = KnownMembers::default();
        while !reader.is_empty() {
            reader.prefixed()?;
            let count = reader.u8()?;
            for _ in 0..count {
                let start = payload.len() - reader.remaining();
                let member = match read_member(&mut reader) {
                    Err(MessageError::UnknownMemberClass { class }) => {
                        known.stopped_at = Some(class);
                        return Ok(known);
                    }
                    other => other?,
                };
                let end = payload.len() - reader.remaining();
                known.members.push((member, start..end));
            }
        }
        Ok(known)
    }
}

/// One member: the integer, the class type id, and the state of that class.
fn read_member(reader: &mut Reader<'_>) -> Result<BundleMember, MessageError> {
    let u64_at_0 = reader.prefixed()?;
    let class = read_class(reader)?;
    let state = MemberState::read(class, reader)?;
    Ok(BundleMember { u64_at_0, state })
}

fn read_class(reader: &mut Reader<'_>) -> Result<TypeId, MessageError> {
    let (class, len) = decode_type_id(reader.rest())?;
    reader.bytes(len)?;
    Ok(class)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::gde::{AssetId, GDE_ID_LEN, GdeMetadataReplicatedState};
    use crate::position::WorldPosition;
    use crate::vitals::VitalsSlot0;
    use crate::wire::RAW_ID_LEN;

    const ENTITY: u64 = 300;
    const LEADING: u64 = 7;
    const OTHER_CLASS: u64 = 11;
    /// An entity integer that fits one prefix byte.
    const SMALL_ENTITY: u8 = 5;

    fn position_member(x: f32) -> BundleMember {
        BundleMember {
            u64_at_0: ENTITY,
            state: MemberState::Position(PositionInTheWorldReplicatedState {
                position: Some(WorldPosition::from_metres(x, 2.0, 3.0).expect("finite")),
                ..PositionInTheWorldReplicatedState::default()
            }),
        }
    }

    fn payload() -> BundlePayload {
        BundlePayload {
            groups: vec![
                BundleGroup {
                    leading: LEADING,
                    members: vec![position_member(1.0), position_member(4.0)],
                },
                BundleGroup {
                    leading: LEADING + 1,
                    members: vec![position_member(5.0)],
                },
            ],
        }
    }

    #[test]
    fn members_of_the_new_classes_walk_in_a_payload() {
        let card = BundleMember {
            u64_at_0: ENTITY,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(AssetId {
                    id: [1; GDE_ID_LEN],
                    sub_id: 2,
                }),
                gde_ref: Some([9; GDE_ID_LEN]),
            }),
        };
        let vitals = BundleMember {
            u64_at_0: ENTITY,
            state: MemberState::Vitals(VitalsSlot0 {
                vitals_level: Some(5),
                max_health: Some(1000),
                ..VitalsSlot0::default()
            }),
        };
        let mixed = BundlePayload {
            groups: vec![BundleGroup {
                leading: LEADING,
                members: vec![position_member(1.0), card, vitals],
            }],
        };
        let bytes = mixed.encode().expect("encode");
        assert_eq!(BundlePayload::decode(&bytes), Ok(mixed));
    }

    #[test]
    fn groups_and_members_round_trip() {
        let bytes = payload().encode().expect("encode");
        assert_eq!(&bytes[..2], &[LEADING as u8, 2]);
        assert_eq!(BundlePayload::decode(&bytes), Ok(payload()));
        let known = BundlePayload::decode_known(&bytes).expect("known");
        assert_eq!(known.members.len(), 3);
        assert_eq!(known.stopped_at, None);
        for (member, range) in known.members {
            assert_eq!(member.encode().expect("member"), bytes[range]);
        }
    }

    #[test]
    fn a_member_without_a_reader_ends_the_known_members() {
        let mut bytes = vec![LEADING as u8, 2];
        bytes.extend(position_member(1.0).encode().expect("member"));
        bytes.extend([SMALL_ENTITY, OTHER_CLASS as u8, 9, 9, 9]);
        let known = BundlePayload::decode_known(&bytes).expect("known");
        assert_eq!(known.members.len(), 1);
        assert_eq!(known.stopped_at, Some(TypeId::FileSlot(OTHER_CLASS)));
        assert_eq!(
            BundlePayload::decode(&bytes),
            Err(MessageError::UnknownMemberClass {
                class: TypeId::FileSlot(OTHER_CLASS)
            })
        );
    }

    #[test]
    fn an_inline_uuid_class_takes_sixteen_bytes() {
        let mut bytes = vec![LEADING as u8, 1, 1, 0];
        bytes.extend([4; RAW_ID_LEN]);
        let known = BundlePayload::decode_known(&bytes).expect("known");
        assert_eq!(known.stopped_at, Some(TypeId::InlineUuid([4; RAW_ID_LEN])));
    }

    proptest! {
        #[test]
        fn random_payloads_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            let _ = BundlePayload::decode(&bytes);
            let _ = BundlePayload::decode_known(&bytes);
        }

        #[test]
        fn random_bytes_after_a_position_member_header_never_panic(
            tail in proptest::collection::vec(any::<u8>(), 0..48),
        ) {
            let mut bytes = vec![LEADING as u8, 1, SMALL_ENTITY, POSITION_IN_THE_WORLD_TYPE_INDEX as u8];
            bytes.extend(tail);
            let _ = BundlePayload::decode(&bytes);
            let _ = BundlePayload::decode_known(&bytes);
        }
    }

    #[test]
    fn too_many_members_or_a_short_payload_are_errors() {
        let group = BundleGroup {
            leading: LEADING,
            members: vec![position_member(1.0); usize::from(u8::MAX) + 1],
        };
        assert!(matches!(
            BundlePayload {
                groups: vec![group]
            }
            .encode(),
            Err(MessageError::CountTooLarge { .. })
        ));
        let bytes = payload().encode().expect("encode");
        assert!(BundlePayload::decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(BundlePayload::decode_known(&bytes[..bytes.len() - 1]).is_err());
        assert_eq!(BundlePayload::decode(&[]), Ok(BundlePayload::default()));
    }
}
