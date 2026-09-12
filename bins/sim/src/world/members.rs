// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bundle groups of the entities a client sees (world-entry note, "Spawn condition traced").
//! Each entity is one group in its own slot ([`super::slots`]); the own player is slot
//! [`LOCAL_PLAYER_INTEREST`]. A group holds the entity's spawn card (type 10, component
//! [`GDE_METADATA_COMPONENT`]) when the entity comes into view (and again once per
//! [`CARD_REFRESH_BUNDLES`] bundles while it stays in view), then its position in an
//! `ALCReplicatedState` (type 11) carrying the `pos10` child, as the retail bundles do. A player's
//! card names the player prefab ([`PLAYER_SLICE_ASSET`]): the client registers its local player
//! only when that prefab is portrayed in slot 1. A mob's card ids are our own values: the mob
//! prefab is Unknown.
//!
//! Bundles split by bytes: an unreliable carrier message must fit one datagram (it is never
//! chunked), so a bundle's groups and stops take at most the bytes of [`MAX_BUNDLE_MEMBERS`]
//! position members, as before M6-B.

use nw_messages::{
    AlcState, AssetId, BundleGroup, BundleMember, BundlePayload, DYNAMIC_SLICE_SUB_ID,
    GDE_METADATA_COMPONENT, GdeMetadataReplicatedState, InputPosition, LOCAL_PLAYER_INTEREST,
    MAX_BUNDLE_INDEXES, MemberState, MessageError, PLAYER_SLICE_ASSET, PlayerStates,
    ReplicatedStateBundle, ServerMessage, player_members, raw_id_of_entity,
};
use nw_sim_core::Position;

use super::MAX_BUNDLE_MEMBERS;
use super::slots::Slots;

/// Most bytes of a position member: a 5-byte entity number, a 1-byte class, a 12-byte body.
const MAX_POSITION_MEMBER_BYTES: usize = 18;
/// The group and stop bytes of one bundle.
pub(super) const MAX_BUNDLE_PAYLOAD_BYTES: usize = MAX_BUNDLE_MEMBERS * MAX_POSITION_MEMBER_BYTES;
/// Bytes of one stopped slot in the index block (a big-endian `u16`).
const STOP_BYTES: usize = 2;
/// Most stops in one bundle: an eighth of the bundle bytes.
const MAX_STOPS_PER_BUNDLE: usize = MAX_BUNDLE_PAYLOAD_BYTES / STOP_BYTES / 8;
const _: () = assert!(MAX_STOPS_PER_BUNDLE as u64 <= MAX_BUNDLE_INDEXES);
/// An entity in view sends its card again once per this many bundles of the player (about one
/// second at one bundle per tick): a bundle is unreliable, and a lost card would hide the entity
/// for as long as it stays in view.
pub(super) const CARD_REFRESH_BUNDLES: u64 = 30;
/// The component of a player's `ALCReplicatedState`: 16 on every own-player member of the
/// captures.
pub(super) const PLAYER_ALC_COMPONENT: u64 = 16;
/// The component of a mob's `ALCReplicatedState`: the most common mob value in the captures
/// (4 and 5 both occur; which mob prefab uses which is Unknown).
pub(super) const MOB_ALC_COMPONENT: u64 = 5;

/// What a client needs to show one mob: its entity number and where it is.
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

/// The prefab and the `ALCReplicatedState` component of one kind of entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EntityKind {
    asset: AssetId,
    alc_component: u64,
    /// Whether the card comes with a state for every other replicated component of the prefab
    /// ([`player_members`]).
    empty_components: bool,
}

impl EntityKind {
    /// A player.
    pub(super) const PLAYER: Self = Self {
        asset: PLAYER_SLICE_ASSET,
        alc_component: PLAYER_ALC_COMPONENT,
        empty_components: true,
    };

    /// A mob whose card carries a unique asset id derived from `entity` (local-player-check
    /// note). The real mob prefab GUID is Unknown; an all-zero id is not used, so the client's
    /// offline cache does not share one key across mobs.
    pub(super) fn mob(entity: u64) -> Self {
        Self {
            asset: AssetId {
                id: raw_id_of_entity(entity),
                sub_id: DYNAMIC_SLICE_SUB_ID,
            },
            alc_component: MOB_ALC_COMPONENT,
            empty_components: false,
        }
    }
}

/// The group of `entity` of `kind` in slot `interest`: its card (with the other component states
/// of a player) when `carded`, then its position. A player's states are `states` when known (the
/// client's own player: its login character, look and vitals), else a connected 3935 state with
/// the entity raw id as `characterId` and empty states.
/// `None` for a position that is not finite.
pub(super) fn entity_group(
    interest: u16,
    entity: u64,
    kind: EntityKind,
    position: Position,
    carded: bool,
    states: Option<PlayerStates>,
) -> Option<BundleGroup> {
    let pos = InputPosition::from_metres(position.x, position.y, position.z)?;
    let mut members = Vec::new();
    if carded {
        members.push(BundleMember {
            component: GDE_METADATA_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState::for_entity(
                kind.asset, entity,
            )),
        });
        if kind.empty_components {
            members.extend(player_members(states.unwrap_or(PlayerStates {
                character_id: raw_id_of_entity(entity),
                look: None,
                health: None,
            })));
        }
    }
    members.push(BundleMember {
        component: kind.alc_component,
        state: MemberState::Alc(AlcState::at_position(pos)),
    });
    Some(BundleGroup {
        interest: u64::from(interest),
        members,
    })
}

/// Which card goes with the client's own player in one bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OwnCard {
    /// No card: only the position.
    Without,
    /// The card again, with the position.
    Refresh,
    /// The first card, with the full first state that sets the client's start position
    /// ([`AlcState::spawn`]).
    Spawn,
}

/// The group of the client's own player with its `states`; `None` for a position that is not
/// finite.
///
/// # Errors
///
/// Returns [`MessageError`] when the spawn state cannot be encoded.
pub(super) fn own_group(
    entity: u64,
    states: PlayerStates,
    position: Position,
    card: OwnCard,
) -> Result<Option<BundleGroup>, MessageError> {
    let Some(mut group) = entity_group(
        LOCAL_PLAYER_INTEREST,
        entity,
        EntityKind::PLAYER,
        position,
        card != OwnCard::Without,
        Some(states),
    ) else {
        return Ok(None);
    };
    if card == OwnCard::Spawn
        && let Some(pos) = InputPosition::from_metres(position.x, position.y, position.z)
        && let Some(alc) = group.members.last_mut()
    {
        alc.state = MemberState::Alc(AlcState::spawn(pos)?);
    }
    Ok(Some(group))
}

/// The bundle envelopes of `groups`, in order, split by bytes, with the pending stops of `slots`
/// in their index blocks (in extra bundles when the groups end first); the first has sequence
/// `sequence` and each next one the next; each carries `client_context`.
pub(super) fn bundles(
    sequence: u64,
    client_context: u8,
    slots: &mut Slots,
    groups: Vec<BundleGroup>,
) -> Result<Vec<Vec<u8>>, MessageError> {
    let mut envelopes = Vec::new();
    let mut groups = groups.into_iter().peekable();
    while groups.peek().is_some() || !slots.pending_stops().is_empty() {
        let stops = slots.take_stops(MAX_STOPS_PER_BUNDLE);
        let mut used = stops.len() * STOP_BYTES;
        let mut part = Vec::new();
        while let Some(group) = groups.peek() {
            let bytes = group_bytes(group)?;
            if !part.is_empty() && used + bytes > MAX_BUNDLE_PAYLOAD_BYTES {
                break;
            }
            used += bytes;
            part.extend(groups.next());
        }
        let next = sequence + envelopes.len() as u64;
        envelopes.push(
            ServerMessage::Bundle(ReplicatedStateBundle::of_groups(
                next,
                client_context,
                stops,
                part,
            )?)
            .envelope()?,
        );
    }
    Ok(envelopes)
}

fn group_bytes(group: &BundleGroup) -> Result<usize, MessageError> {
    Ok(BundlePayload {
        groups: vec![group.clone()],
    }
    .encode()?
    .len())
}

#[cfg(test)]
mod tests {
    use nw_messages::{
        EntitySlots, GDE_ID_LEN, LOGIN_CLIENT_CONTEXT, PLAYER_COMPONENT_TYPE_INDEX, PlayerLook,
        SlotRead,
    };
    use nw_reflect::Envelope;
    use uuid::Uuid;

    use super::super::slots::FIRST_REMOTE_INTEREST;
    use super::*;

    const ENTITY: u64 = (1 << 34) + 7;
    const OWN: u64 = 9;
    const CHARACTER: Uuid = Uuid::from_u128(0x2b87_684d_c82c_46a4_90e4_c764_0247_7507);
    const HEALTH: (u32, u32) = (700, 856);

    fn own_states() -> PlayerStates {
        PlayerStates {
            character_id: *CHARACTER.as_bytes(),
            look: Some(PlayerLook::default()),
            health: Some(HEALTH),
        }
    }
    const SEQUENCE: u64 = 4;
    const MANY: u64 = 60;
    const ALL_STOPS: usize = 1000;

    fn at() -> Position {
        Position {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        }
    }

    fn reads(envelopes: &[Vec<u8>]) -> Vec<SlotRead> {
        envelopes
            .iter()
            .map(|bytes| {
                let envelope = Envelope::decode(bytes).expect("envelope");
                let Some(Ok(ServerMessage::Bundle(bundle))) =
                    ServerMessage::from_envelope(&envelope)
                else {
                    panic!("a bundle");
                };
                SlotRead::of(&bundle).expect("read")
            })
            .collect()
    }

    #[test]
    fn the_own_player_is_the_player_prefab_in_slot_1() {
        let group = own_group(OWN, own_states(), at(), OwnCard::Spawn)
            .expect("encode")
            .expect("finite");
        assert_eq!(group.interest, u64::from(LOCAL_PLAYER_INTEREST));
        let MemberState::GdeMetadata(card) = &group.members[0].state else {
            panic!("the card comes first: {group:?}");
        };
        assert_eq!(group.members[0].component, GDE_METADATA_COMPONENT);
        assert_eq!(card.asset, Some(PLAYER_SLICE_ASSET));
        assert_eq!(card.entity_number(), Some(OWN));
        let states: Vec<BundleMember> = player_members(own_states()).collect();
        assert_eq!(
            group.members[1..=states.len()],
            states[..],
            "every other player component gets a state with the card: the own states"
        );
        let player = states
            .iter()
            .find(|member| member.state.type_index() == PLAYER_COMPONENT_TYPE_INDEX)
            .expect("player component");
        let MemberState::PlayerComponent(state) = &player.state else {
            panic!("3935 is the player component: {player:?}");
        };
        assert_eq!(
            state.character_id,
            Some(*CHARACTER.as_bytes()),
            "the own characterId is the login character, so the client sees the local player"
        );
        assert_eq!(state.player_connected, Some(true));
        let last = group.members.last().expect("position");
        assert_eq!(last.component, PLAYER_ALC_COMPONENT);
        let MemberState::Alc(alc) = &last.state else {
            panic!("then the ALC position: {group:?}");
        };
        assert_eq!(alc.pos10, InputPosition::from_metres(10.0, 20.0, 30.0));
        let later = own_group(OWN, own_states(), at(), OwnCard::Without)
            .expect("encode")
            .expect("finite");
        assert_eq!(later.members.len(), 1, "later only the ALC position");
        let payload = BundlePayload {
            groups: vec![group.clone()],
        };
        let read = BundlePayload::decode(&payload.encode().expect("payload")).expect("read");
        assert_eq!(read.groups[0].members, group.members);
    }

    #[test]
    fn only_the_spawn_card_carries_the_full_first_state() {
        let pos = InputPosition::from_metres(10.0, 20.0, 30.0).expect("finite");
        let alc_of = |card: OwnCard| {
            let group = own_group(OWN, own_states(), at(), card)
                .expect("encode")
                .expect("finite");
            group.members.last().expect("alc").state.clone()
        };
        assert_eq!(
            alc_of(OwnCard::Spawn),
            MemberState::Alc(AlcState::spawn(pos).expect("spawn")),
            "the first card sets the start position with the full state"
        );
        assert_eq!(
            alc_of(OwnCard::Refresh),
            MemberState::Alc(AlcState::at_position(pos)),
            "a refresh moves nothing: the plain position"
        );
        assert_eq!(
            alc_of(OwnCard::Without),
            MemberState::Alc(AlcState::at_position(pos))
        );
    }

    #[test]
    fn a_mob_group_carries_its_own_card() {
        let group = entity_group(
            FIRST_REMOTE_INTEREST,
            ENTITY,
            EntityKind::mob(ENTITY),
            at(),
            true,
            None,
        )
        .expect("finite");
        let MemberState::GdeMetadata(card) = &group.members[0].state else {
            panic!("the card comes first: {group:?}");
        };
        let asset = card.asset.expect("asset");
        assert_eq!(asset.id, raw_id_of_entity(ENTITY));
        assert_eq!(asset.sub_id, DYNAMIC_SLICE_SUB_ID);
        assert_ne!(asset.id, [0; GDE_ID_LEN]);
        assert_eq!(card.entity_number(), Some(ENTITY));
        assert_eq!(group.members[1].component, MOB_ALC_COMPONENT);
    }

    #[test]
    fn bundles_split_by_bytes_and_carry_the_stops() {
        let mut slots = Slots::new();
        let groups: Vec<BundleGroup> = (0..MANY)
            .map(|index| {
                let interest = slots.slot(ENTITY + index).expect("slot").interest;
                entity_group(
                    interest,
                    ENTITY + index,
                    EntityKind::mob(ENTITY + index),
                    at(),
                    true,
                    None,
                )
                .expect("finite")
            })
            .collect();
        slots.keep_only(&[]);
        let envelopes =
            bundles(SEQUENCE, LOGIN_CLIENT_CONTEXT, &mut slots, groups).expect("bundles");
        assert!(envelopes.len() > 1);
        let reads = reads(&envelopes);
        let mut entities = EntitySlots::default();
        let mut seen = Vec::new();
        let mut stopped = 0;
        for mut read in reads {
            stopped += read.stops.len();
            entities.resolve(&mut read);
            seen.extend(read.updates.iter().filter_map(|update| update.entity));
        }
        let expected: Vec<u64> = (0..MANY).map(|index| ENTITY + index).collect();
        assert_eq!(seen, expected, "every group once, in order");
        assert_eq!(stopped, MANY as usize, "every stop goes out");
        assert!(slots.take_stops(ALL_STOPS).is_empty());
    }

    #[test]
    fn stops_without_groups_still_go_out() {
        let mut slots = Slots::new();
        slots.slot(ENTITY);
        slots.keep_only(&[]);
        let envelopes =
            bundles(SEQUENCE, LOGIN_CLIENT_CONTEXT, &mut slots, Vec::new()).expect("bundles");
        let reads = reads(&envelopes);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].stops, vec![FIRST_REMOTE_INTEREST]);
        assert!(
            bundles(SEQUENCE, LOGIN_CLIENT_CONTEXT, &mut slots, Vec::new())
                .expect("bundles")
                .is_empty()
        );
    }
}
