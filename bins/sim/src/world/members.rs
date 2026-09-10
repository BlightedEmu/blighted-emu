// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bundle members of players and mobs (M6-B spec section 5). A player is its own position
//! member (type 13). A mob replicates the way the retail bundles do: its spawn card (type 10)
//! when it comes into a player's view (and again once per [`CARD_REFRESH_BUNDLES`] bundles while
//! it stays in view), then its position every bundle through an `ALCReplicatedState` (type 11,
//! the member that dominates the real bundles) carrying the `pos10` child. The real client cannot
//! decode a `VitalsComponentReplicatedState` (type 15) member — that class is not in the captured
//! bundle members and its wire layout was never pinned — so the mob no longer sends one. The
//! spawn card's ids are our own values: what the client needs there is Unknown.
//!
//! Bundles split by bytes: an unreliable carrier message must fit one datagram (it is never
//! chunked), so a bundle's members take at most the bytes of [`MAX_BUNDLE_MEMBERS`] position
//! members, as before M6-B.

use nw_messages::{
    AlcState, AssetId, BundleMember, GDE_ID_LEN, GdeMetadataReplicatedState, InputPosition,
    MemberState, MessageError, PositionInTheWorldReplicatedState, ReplicatedStateBundle,
    ServerMessage, WorldPosition,
};
use nw_sim_core::Position;

use super::MAX_BUNDLE_MEMBERS;

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

/// What a client needs to show one mob: its entity number and where it is. Health and ids used
/// to go out in a type-15 vitals member, which the real client cannot decode, so they are no
/// longer part of the card.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MobCard {
    /// The entity number.
    pub(super) entity: u64,
    /// Where it is.
    pub(super) position: Position,
}

/// The card of a mob.
pub(super) const fn card_of(entity: u64, position: Position) -> MobCard {
    MobCard { entity, position }
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
    }
    if let Some(pos) = InputPosition::from_metres(card.position.x, card.position.y, card.position.z)
    {
        members.push(BundleMember {
            u64_at_0: card.entity,
            state: MemberState::Alc(AlcState::at_position(pos)),
        });
    }
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

    fn card() -> MobCard {
        MobCard {
            entity: ENTITY,
            position: Position {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            },
        }
    }

    #[test]
    fn a_mob_sends_its_spawn_card_then_an_alc_position() {
        let members = mob_members(&card(), true, SUB_ID);
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
        let MemberState::Alc(alc) = &members[1].state else {
            panic!("then the ALC position: {members:?}");
        };
        assert_eq!(alc.pos10, InputPosition::from_metres(10.0, 20.0, 30.0));
        assert!(members.iter().all(|member| member.u64_at_0 == ENTITY));
        assert_eq!(
            mob_members(&card(), false, SUB_ID).len(),
            1,
            "later only the ALC position, not the spawn card"
        );
    }

    #[test]
    fn the_alc_position_member_round_trips_in_a_bundle() {
        let members = mob_members(&card(), true, SUB_ID);
        let envelope = bundle(0, members).expect("bundle");
        let envelope = Envelope::decode(&envelope).expect("envelope");
        let Some(Ok(ServerMessage::Bundle(decoded))) = ServerMessage::from_envelope(&envelope)
        else {
            panic!("a bundle");
        };
        let payload = BundlePayload::decode(&decoded.payload).expect("payload");
        let alc = payload.groups[0].members.iter().find_map(|member| match &member.state {
            MemberState::Alc(alc) => Some(alc.clone()),
            _ => None,
        });
        assert_eq!(
            alc.and_then(|alc| alc.pos10),
            InputPosition::from_metres(10.0, 20.0, 30.0)
        );
    }

    #[test]
    fn bundles_split_by_bytes() {
        let members: Vec<BundleMember> = (0..60)
            .flat_map(|index| {
                mob_members(
                    &MobCard {
                        entity: ENTITY + index,
                        ..card()
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
