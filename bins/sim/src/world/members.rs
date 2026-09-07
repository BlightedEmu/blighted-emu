// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bundle members of players and mobs (M6-B spec section 5). A player is its position. A
//! mob is its position too, and when it comes into a player's view, its spawn card (type 10) and
//! its vitals block (type 15, slot 0: current and max health, ids, level) come first; they come
//! again once per [`CARD_REFRESH_BUNDLES`] bundles while it stays in view. The spawn card's ids are our own values:
//! what the client needs there is Unknown.
//!
//! Bundles split by bytes: an unreliable carrier message must fit one datagram (it is never
//! chunked), so a bundle's members take at most the bytes of [`MAX_BUNDLE_MEMBERS`] position
//! members, as before M6-B.

use nw_bus::MobGhost;
use nw_messages::{
    AssetId, BundleMember, GDE_ID_LEN, GdeMetadataReplicatedState, MemberState, MessageError,
    PositionInTheWorldReplicatedState, ReplicatedStateBundle, ServerMessage, VitalsSlot0,
    WorldPosition,
};
use nw_sim_core::Position;

use super::MAX_BUNDLE_MEMBERS;
use super::attack::amount_of;

/// Most bytes of a position member: a 5-byte entity number, a 1-byte class, a 12-byte body.
const MAX_POSITION_MEMBER_BYTES: usize = 18;
/// The member bytes of one bundle.
pub(super) const MAX_BUNDLE_PAYLOAD_BYTES: usize = MAX_BUNDLE_MEMBERS * MAX_POSITION_MEMBER_BYTES;
/// A mob in view sends its card and vitals again once per this many bundles of the player (about
/// one second at one bundle per tick): a bundle is unreliable, and a lost card would hide the mob
/// for as long as it stays in view.
pub(super) const CARD_REFRESH_BUNDLES: u64 = 30;
/// The entity number fills the last 8 bytes of the 16-byte `GdeRef`.
const ENTITY_IN_REF: usize = GDE_ID_LEN - std::mem::size_of::<u64>();

/// What a client needs to show one mob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MobCard {
    /// The entity number.
    pub(super) entity: u64,
    /// Where it is.
    pub(super) position: Position,
    /// crc32 of the lower-case vitals id.
    pub(super) vitals_id: u32,
    /// crc32 of the lower-case category id.
    pub(super) category_id: u32,
    /// The spawn level.
    pub(super) level: u32,
    /// The current health.
    pub(super) health: u32,
    /// The max health.
    pub(super) max_health: u32,
}

/// The card of a mob from its state.
pub(super) const fn card_of(entity: u64, position: Position, state: MobGhost) -> MobCard {
    MobCard {
        entity,
        position,
        vitals_id: state.vitals_id,
        category_id: state.category_id,
        level: state.level,
        health: state.health,
        max_health: state.max_health,
    }
}

/// The position member of `entity`; `None` for a position that is not finite.
pub(super) fn position_member(entity: u64, position: Position) -> Option<BundleMember> {
    Some(BundleMember {
        u64_at_0: entity,
        state: MemberState::Position(PositionInTheWorldReplicatedState {
            position: Some(WorldPosition::from_metres(
                position.x, position.y, position.z,
            )?),
            ..PositionInTheWorldReplicatedState::default()
        }),
    })
}

/// The members of a mob: its spawn card and vitals block when it is `first` in view, then its
/// position.
pub(super) fn mob_members(card: &MobCard, first: bool, asset_sub_id: u32) -> Vec<BundleMember> {
    let mut members = Vec::new();
    if first {
        let mut gde_ref = [0; GDE_ID_LEN];
        gde_ref[ENTITY_IN_REF..].copy_from_slice(&card.entity.to_be_bytes());
        members.push(BundleMember {
            u64_at_0: card.entity,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(AssetId {
                    id: [0; GDE_ID_LEN],
                    sub_id: asset_sub_id,
                }),
                gde_ref: Some(gde_ref),
            }),
        });
        members.push(BundleMember {
            u64_at_0: card.entity,
            state: MemberState::Vitals(VitalsSlot0 {
                health_amount: Some(amount_of(card.health)),
                health_max: Some(amount_of(card.max_health)),
                vitals_id: Some(card.vitals_id),
                vitals_category_id: Some(card.category_id),
                vitals_level: Some(card.level),
                max_health: Some(u16::try_from(card.max_health).unwrap_or(u16::MAX)),
                ..VitalsSlot0::default()
            }),
        });
    }
    members.extend(position_member(card.entity, card.position));
    members
}

/// The bundle envelopes of `members`, in order, split by bytes; the first has sequence
/// `sequence` and each next one the next.
pub(super) fn bundles(
    sequence: u64,
    members: Vec<BundleMember>,
) -> Result<Vec<Vec<u8>>, MessageError> {
    let mut envelopes = Vec::new();
    let mut part = Vec::new();
    let mut part_bytes = 0;
    for member in members {
        let bytes = member.encode()?.len();
        if !part.is_empty() && part_bytes + bytes > MAX_BUNDLE_PAYLOAD_BYTES {
            let next = sequence + envelopes.len() as u64;
            envelopes.push(bundle(next, std::mem::take(&mut part))?);
            part_bytes = 0;
        }
        part_bytes += bytes;
        part.push(member);
    }
    if !part.is_empty() {
        let next = sequence + envelopes.len() as u64;
        envelopes.push(bundle(next, part)?);
    }
    Ok(envelopes)
}

fn bundle(sequence: u64, members: Vec<BundleMember>) -> Result<Vec<u8>, MessageError> {
    ServerMessage::Bundle(ReplicatedStateBundle::of_members(sequence, members)?).envelope()
}

#[cfg(test)]
mod tests {
    use nw_messages::{BundlePayload, MemberState, ServerMessage};
    use nw_reflect::Envelope;

    use super::*;

    const ENTITY: u64 = (1 << 34) + 7;
    /// The group head before the members: the group integer and the member count.
    const PAYLOAD_HEADER_BYTES: usize = 2;
    const SUB_ID: u32 = 2;

    fn card(max_health: u32) -> MobCard {
        MobCard {
            entity: ENTITY,
            position: Position {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            },
            vitals_id: 11,
            category_id: 12,
            level: 13,
            health: max_health,
            max_health,
        }
    }

    #[test]
    fn max_health_is_capped_at_u16() {
        let members = mob_members(&card(100_000), true, SUB_ID);
        let vitals = members.iter().find_map(|member| match &member.state {
            MemberState::Vitals(vitals) => Some(*vitals),
            _ => None,
        });
        assert_eq!(vitals.and_then(|vitals| vitals.max_health), Some(u16::MAX));
    }

    #[test]
    fn the_spawn_card_carries_the_sub_id_and_the_entity_number() {
        let members = mob_members(&card(500), true, SUB_ID);
        let MemberState::GdeMetadata(spawn) = &members[0].state else {
            panic!("the card comes first: {members:?}");
        };
        let asset = spawn.asset.expect("asset");
        assert_eq!((asset.id, asset.sub_id), ([0; 16], SUB_ID));
        let gde_ref = spawn.gde_ref.expect("ref");
        assert_eq!(&gde_ref[..8], &[0; 8]);
        assert_eq!(
            u64::from_be_bytes(gde_ref[8..].try_into().expect("8")),
            ENTITY
        );
        assert!(members.iter().all(|member| member.u64_at_0 == ENTITY));
        let MemberState::Vitals(vitals) = &members[1].state else {
            panic!("then the vitals: {members:?}");
        };
        assert_eq!(
            (
                vitals.vitals_id,
                vitals.vitals_category_id,
                vitals.vitals_level
            ),
            (Some(11), Some(12), Some(13))
        );
        assert!(matches!(members[2].state, MemberState::Position(_)));
        assert_eq!(
            mob_members(&card(500), false, SUB_ID).len(),
            1,
            "later only the position"
        );
    }

    #[test]
    fn the_vitals_block_carries_current_and_max_health() {
        let wounded = MobCard {
            health: 400,
            ..card(500)
        };
        let members = mob_members(&wounded, true, SUB_ID);
        let MemberState::Vitals(vitals) = &members[1].state else {
            panic!("the vitals: {members:?}");
        };
        assert_eq!(
            (vitals.health_amount, vitals.health_max, vitals.max_health),
            (Some(400.0), Some(500.0), Some(500))
        );
    }

    #[test]
    fn bundles_split_by_bytes() {
        let members: Vec<BundleMember> = (0..60)
            .flat_map(|index| {
                mob_members(
                    &MobCard {
                        entity: ENTITY + index,
                        ..card(500)
                    },
                    true,
                    SUB_ID,
                )
            })
            .collect();
        let envelopes = bundles(0, members.clone()).expect("bundles");
        assert!(envelopes.len() > 1);
        let mut seen = 0;
        for envelope in &envelopes {
            let envelope = Envelope::decode(envelope).expect("envelope");
            let Some(Ok(ServerMessage::Bundle(bundle))) = ServerMessage::from_envelope(&envelope)
            else {
                panic!("a bundle");
            };
            assert!(bundle.payload.len() <= MAX_BUNDLE_PAYLOAD_BYTES + PAYLOAD_HEADER_BYTES);
            seen += BundlePayload::decode(&bundle.payload)
                .expect("payload")
                .groups[0]
                .members
                .len();
        }
        assert_eq!(seen, members.len(), "every member once, in order");
    }
}
