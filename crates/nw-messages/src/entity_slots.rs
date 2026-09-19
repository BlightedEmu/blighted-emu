// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client side of the entity slots of the replication bundles (world-entry note, "Spawn
//! condition traced"). A bundle first stops slots (its index block), then sends one group per
//! slot. A slot is tied to an entity by the spawn card in it ([`GdeMetadataReplicatedState`]) and
//! stays tied until it is stopped; slot [`LOCAL_PLAYER_INTEREST`] is the client's own player.

use std::collections::BTreeMap;

use crate::bundle::{LOCAL_PLAYER_INTEREST, ReplicatedStateBundle};
use crate::gde::GdeMetadataReplicatedState;
use crate::payload::{BundleMember, BundlePayload, MemberState};
use crate::position::WorldPosition;
use crate::wire::MessageError;

/// What one bundle group says about one slot.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotUpdate {
    /// The slot (`interestId`): the low 16 bits of the group integer.
    pub interest: u16,
    /// The entity number of the slot: from a spawn card in this group, else from an earlier one
    /// (filled by [`EntitySlots::resolve`]).
    pub entity: Option<u64>,
    /// The spawn card in the group, when it holds one.
    pub card: Option<GdeMetadataReplicatedState>,
    /// The last position in the group (a position member or an `ALCReplicatedState` position).
    pub position: Option<WorldPosition>,
    /// The members.
    pub members: Vec<BundleMember>,
}

impl SlotUpdate {
    /// Whether this is the client's own player.
    pub const fn is_local_player(&self) -> bool {
        self.interest == LOCAL_PLAYER_INTEREST
    }
}

/// What one bundle says, before the slots are resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotRead {
    /// The stopped slots, in order.
    pub stops: Vec<u16>,
    /// One update per group, in order.
    pub updates: Vec<SlotUpdate>,
}

impl SlotRead {
    /// Reads `bundle`; every member must have a reader here.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the payload does not decode.
    pub fn of(bundle: &ReplicatedStateBundle) -> Result<Self, MessageError> {
        let block = bundle.index_block.as_ref();
        let stop_count = block.map_or(0, |block| {
            usize::try_from(block.stop_count).unwrap_or(usize::MAX)
        });
        let stops = block.map_or_else(Vec::new, |block| {
            block.indexes.iter().take(stop_count).copied().collect()
        });
        let updates = BundlePayload::decode(&bundle.payload)?
            .groups
            .into_iter()
            .map(|group| {
                let card = group.members.iter().find_map(|member| match &member.state {
                    MemberState::GdeMetadata(card) => Some(*card),
                    _ => None,
                });
                let position = group.members.iter().rev().find_map(position_of);
                SlotUpdate {
                    interest: low_16_bits(group.interest),
                    entity: card.and_then(|card| card.entity_number()),
                    card,
                    position,
                    members: group.members,
                }
            })
            .collect();
        Ok(Self { stops, updates })
    }

    /// The own player's last position in this bundle.
    pub fn local_player_position(&self) -> Option<WorldPosition> {
        self.updates
            .iter()
            .rev()
            .filter(|update| update.is_local_player())
            .find_map(|update| update.position)
    }
}

/// The entity of each slot, as one client keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntitySlots {
    entities: BTreeMap<u16, u64>,
}

impl EntitySlots {
    /// Applies the stops and cards of `read`, in bundle order, and fills the entity of each
    /// update from them. Returns the entities of the stopped slots.
    pub fn resolve(&mut self, read: &mut SlotRead) -> Vec<u64> {
        let stopped = read
            .stops
            .iter()
            .filter_map(|slot| self.entities.remove(slot))
            .collect();
        for update in &mut read.updates {
            match update.entity {
                Some(entity) => {
                    self.entities.insert(update.interest, entity);
                }
                None => update.entity = self.entities.get(&update.interest).copied(),
            }
        }
        stopped
    }

    /// Ties `interest` to `entity` without a spawn card: type 164 names the local player's
    /// entity, and slot [`LOCAL_PLAYER_INTEREST`] holds that player even when the card knob is
    /// off (local-player-check note).
    pub fn bind(&mut self, interest: u16, entity: u64) {
        self.entities.insert(interest, entity);
    }

    /// The entity of `interest`, when a card or [`Self::bind`] tied it.
    pub fn entity(&self, interest: u16) -> Option<u64> {
        self.entities.get(&interest).copied()
    }
}

fn position_of(member: &BundleMember) -> Option<WorldPosition> {
    match &member.state {
        MemberState::Position(state) => state.position,
        MemberState::Alc(alc) => alc
            .pos10
            .and_then(|at| WorldPosition::from_metres(at.x, at.y, at.z())),
        MemberState::GdeMetadata(_)
        | MemberState::Vitals(_)
        | MemberState::Stamina(_)
        | MemberState::CombatStatus(_)
        | MemberState::DelayedEvent(_)
        | MemberState::LookTargeting(_)
        | MemberState::Gathering(_)
        | MemberState::Interactor(_)
        | MemberState::PlayerComponent(_)
        | MemberState::Appearance(_)
        | MemberState::Mana(_)
        | MemberState::Paperdoll(_)
        | MemberState::Container(_)
        | MemberState::Achievement(_)
        | MemberState::DamageReceiver(_)
        | MemberState::GatherableController(_)
        | MemberState::Interact(_)
        | MemberState::Currency(_)
        | MemberState::ItemManagement(_)
        | MemberState::Objectives(_)
        | MemberState::Progression(_)
        | MemberState::Cooldowns(_)
        | MemberState::Empty(_)
        | MemberState::Recorded { .. } => None,
    }
}

/// The client keeps the low 16 bits of the group integer.
fn low_16_bits(interest: u64) -> u16 {
    u16::try_from(interest & u64::from(u16::MAX)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alc::AlcState;
    use crate::client_input::InputPosition;
    use crate::gde::{GDE_METADATA_COMPONENT, PLAYER_SLICE_ASSET};
    use crate::payload::BundleGroup;

    const SEQUENCE: u64 = 3;
    const OWN: u64 = 77;
    const MOB: u64 = (1 << 34) + 5;
    const MOB_SLOT: u16 = 3;
    const ALC_COMPONENT: u64 = 16;

    fn card(number: u64) -> BundleMember {
        BundleMember {
            component: GDE_METADATA_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState::for_entity(
                PLAYER_SLICE_ASSET,
                number,
            )),
        }
    }

    fn alc_at(x: f32) -> BundleMember {
        BundleMember {
            component: ALC_COMPONENT,
            state: MemberState::Alc(AlcState::at_position(
                InputPosition::from_metres(x, 0.0, 0.0).expect("finite"),
            )),
        }
    }

    fn group(interest: u16, members: Vec<BundleMember>) -> BundleGroup {
        BundleGroup {
            interest: u64::from(interest),
            members,
        }
    }

    fn read(stops: Vec<u16>, groups: Vec<BundleGroup>) -> SlotRead {
        let bundle =
            ReplicatedStateBundle::of_groups(SEQUENCE, crate::LOGIN_CLIENT_CONTEXT, stops, groups)
                .expect("bundle");
        SlotRead::of(&bundle).expect("read")
    }

    #[test]
    fn a_card_ties_a_slot_until_it_is_stopped() {
        let mut slots = EntitySlots::default();
        let mut first = read(
            Vec::new(),
            vec![
                group(LOCAL_PLAYER_INTEREST, vec![card(OWN), alc_at(1.0)]),
                group(MOB_SLOT, vec![card(MOB), alc_at(2.0)]),
            ],
        );
        assert!(slots.resolve(&mut first).is_empty());
        assert!(first.updates[0].is_local_player());
        assert_eq!(
            first.updates[0].card.and_then(|card| card.asset),
            Some(PLAYER_SLICE_ASSET)
        );
        assert_eq!(first.updates[1].entity, Some(MOB));
        assert_eq!(
            first.local_player_position(),
            WorldPosition::from_metres(1.0, 0.0, 0.0)
        );
        let mut later = read(Vec::new(), vec![group(MOB_SLOT, vec![alc_at(3.0)])]);
        slots.resolve(&mut later);
        assert_eq!(later.updates[0].entity, Some(MOB));
        assert_eq!(later.updates[0].card, None);
        let mut stopped = read(vec![MOB_SLOT], Vec::new());
        assert_eq!(slots.resolve(&mut stopped), vec![MOB]);
        assert_eq!(slots.entity(MOB_SLOT), None);
        assert_eq!(slots.entity(LOCAL_PLAYER_INTEREST), Some(OWN));
    }

    #[test]
    fn a_bind_ties_the_local_slot_without_a_card() {
        let mut slots = EntitySlots::default();
        slots.bind(LOCAL_PLAYER_INTEREST, OWN);
        let mut later = read(
            Vec::new(),
            vec![group(LOCAL_PLAYER_INTEREST, vec![alc_at(1.0)])],
        );
        slots.resolve(&mut later);
        assert_eq!(later.updates[0].entity, Some(OWN));
        assert_eq!(later.updates[0].card, None);
    }
}
