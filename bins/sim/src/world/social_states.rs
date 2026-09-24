// SPDX-License-Identifier: AGPL-3.0-or-later
//! The replicated social states the world's social service sends for this sim's players (run 15
//! social2 study): the player's groups, social, company and chat states go into its next own
//! bundle, and its group entity (`GroupDataComponentReplicatedState` with the group's spawn
//! card) is one more entity in its bundles, in an entity slot of its own. This sim also tells
//! the service each change of a player's health and life, which its group shows.

use nw_bus::{
    GroupEntityDelivery, OwnStateDelivery, PlayerVitals, SocialDeliveries, SocialInput,
    SocialStateKind,
};
use nw_messages::payload::{BundleGroup, BundleMember, MemberState};
use nw_messages::social::group_data::{
    FULL_PERCENT, GROUP_DATA_TYPE_INDEX, GroupDataState, group_entity_members,
};
use nw_messages::social::states::{
    CHAT_STATE_COMPONENT, CHAT_STATE_TYPE_INDEX, GROUPS_STATE_COMPONENT, GROUPS_STATE_TYPE_INDEX,
    GUILDS_STATE_COMPONENT, GUILDS_STATE_TYPE_INDEX,
};
use nw_messages::title_choice::{SOCIAL_COMPONENT, SOCIAL_TYPE_INDEX};
use uuid::Uuid;

use super::attack::Vitals;
use super::slots::Slot;
use super::{Player, SimWorld};

/// The entity numbers of the players' group entities: from the top of the mob numbers (below
/// `1 << 35`) to the musical performances.
pub(super) const GROUP_ENTITY_BASE: u64 = 1 << 35;
/// Percent of a full bar.
const PERCENT: u64 = 100;

/// The player component and class of a replicated social state.
const fn member_of(kind: SocialStateKind) -> (u64, u64) {
    match kind {
        SocialStateKind::Groups => (GROUPS_STATE_COMPONENT, GROUPS_STATE_TYPE_INDEX),
        SocialStateKind::Social => (SOCIAL_COMPONENT, SOCIAL_TYPE_INDEX),
        SocialStateKind::Guilds => (GUILDS_STATE_COMPONENT, GUILDS_STATE_TYPE_INDEX),
        SocialStateKind::Chat => (CHAT_STATE_COMPONENT, CHAT_STATE_TYPE_INDEX),
    }
}

/// A player's group entity on this sim.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupEntity {
    /// Its entity number for the player's slots.
    number: u64,
    group: Uuid,
    state: GroupDataState,
    /// The state changed since the client got it.
    changed: bool,
}

/// A player's social side on this sim.
#[derive(Debug, Default)]
pub(super) struct PlayerSocial {
    group_entity: Option<GroupEntity>,
    /// Group entities this player had on this sim: the next one gets a new number, so its client
    /// gets a new slot and card.
    groups_seen: u64,
    /// The vitals last told to the social service.
    vitals_sent: Option<(u8, u8, bool)>,
}

impl PlayerSocial {
    /// The entity number of the player's group entity.
    pub(super) fn group_entity_number(&self) -> Option<u64> {
        self.group_entity.as_ref().map(|entity| entity.number)
    }

    /// The bundle group of the group entity `number` in `slot`: the group data and, in a new
    /// slot, the spawn card; a known slot gets the data after a change only.
    pub(super) fn group_entity_group(&mut self, number: u64, slot: Slot) -> Option<BundleGroup> {
        let entity = self
            .group_entity
            .as_mut()
            .filter(|entity| entity.number == number)?;
        if !slot.new && !entity.changed {
            return None;
        }
        entity.changed = false;
        Some(BundleGroup {
            interest: u64::from(slot.interest),
            members: group_entity_members(*entity.group.as_bytes(), entity.state.clone(), slot.new),
        })
    }

    fn set_group_entity(&mut self, delivery: &GroupEntityDelivery) {
        let Some(view) = &delivery.view else {
            self.group_entity = None;
            return;
        };
        let state = match MemberState::decode(GROUP_DATA_TYPE_INDEX, &view.body) {
            Ok(MemberState::GroupData(state)) => *state,
            Ok(_) | Err(_) => {
                tracing::warn!(character = %delivery.character, "group entity does not read");
                return;
            }
        };
        match &mut self.group_entity {
            Some(entity) if entity.group == view.group => {
                entity.changed = entity.state != state;
                entity.state = state;
            }
            _ => {
                self.groups_seen = self.groups_seen.wrapping_add(1);
                self.group_entity = Some(GroupEntity {
                    number: GROUP_ENTITY_BASE + self.groups_seen,
                    group: view.group,
                    state,
                    changed: true,
                });
            }
        }
    }
}

/// Health in percent of `max`, rounded up: a living player never shows 0.
fn percent(current: u32, max: u32) -> u8 {
    if max == 0 {
        return 0;
    }
    let percent = (u64::from(current) * PERCENT).div_ceil(u64::from(max));
    u8::try_from(percent.min(PERCENT)).unwrap_or(FULL_PERCENT)
}

impl SimWorld {
    /// Puts the social service's states of `deliveries` into their players: the own states into
    /// the next own bundle, the group entities into the players' entities. A player that is not
    /// here is skipped.
    pub fn apply_social_states(&mut self, deliveries: &SocialDeliveries) {
        for delivery in &deliveries.own_states {
            self.apply_own_state(delivery);
        }
        for delivery in &deliveries.group_entities {
            let Some(entity) = self.player_of_character(delivery.character) else {
                continue;
            };
            if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                player.social.set_group_entity(delivery);
            }
        }
    }

    fn apply_own_state(&mut self, delivery: &OwnStateDelivery) {
        let (component, class) = member_of(delivery.kind);
        let state = match MemberState::decode(class, &delivery.body) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(character = %delivery.character, class, %error, "social state does not read");
                return;
            }
        };
        let Some(entity) = self.player_of_character(delivery.character) else {
            return;
        };
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.extra_members.push(BundleMember { component, state });
        }
    }

    fn player_of_character(&self, character: Uuid) -> Option<bevy_ecs::entity::Entity> {
        self.by_connection.values().copied().find(|entity| {
            self.world
                .get::<Player>(*entity)
                .is_some_and(|player| player.character == character)
        })
    }

    /// The health and life changes of this sim's players since the last call, for the social
    /// service. The sim keeps no mana pool: mana is full.
    pub fn social_vitals(&mut self) -> Vec<SocialInput> {
        let entities: Vec<bevy_ecs::entity::Entity> =
            self.by_connection.values().copied().collect();
        let mut inputs = Vec::new();
        for entity in entities {
            let Some(vitals) = self.world.get::<Vitals>(entity).copied() else {
                continue;
            };
            let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                continue;
            };
            let health = vitals.health;
            let now = (
                percent(health.current(), health.max()),
                FULL_PERCENT,
                health.current() == 0,
            );
            if player.social.vitals_sent == Some(now) {
                continue;
            }
            player.social.vitals_sent = Some(now);
            inputs.push(SocialInput::Vitals(PlayerVitals {
                character: player.character,
                health_percent: now.0,
                mana_percent: now.1,
                dead: now.2,
            }));
        }
        inputs
    }
}

#[cfg(test)]
mod tests {
    use nw_bus::GroupEntityView;
    use nw_messages::appearance::PlayerIconData;
    use nw_messages::social::group_data::{GROUP_CARD_COMPONENT, GroupMemberView};
    use nw_messages::social::states::GroupsState;
    use nw_messages::wire::Writer;

    use super::*;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};

    const PLAYER: u64 = 3;
    const GROUP: u128 = 77;
    const INTEREST: u16 = 40;

    fn admitted() -> (SimWorld, Uuid) {
        let mut world = world_with_spawns(Vec::new());
        let admit = admit_at(PLAYER, 0.0, true);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        (world, admit.character)
    }

    fn entity_view(character: Uuid, health: u8) -> GroupEntityView {
        let member = GroupMemberView {
            character: *character.as_bytes(),
            name: "Ann".to_owned(),
            icon: PlayerIconData::default(),
            health,
            mana: FULL_PERCENT,
            connected: true,
            dead: false,
            leader: true,
            faction: 0,
            level: 1,
        };
        let state = GroupDataState::whole(*Uuid::from_u128(GROUP).as_bytes(), &[member]);
        let mut writer = Writer::new();
        state.write(&mut writer).expect("writes");
        GroupEntityView {
            group: Uuid::from_u128(GROUP),
            body: writer.finish(),
        }
    }

    fn side(world: &mut SimWorld) -> &mut PlayerSocial {
        let entity = world.player_by_number(PLAYER).expect("player");
        &mut world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .into_inner()
            .social
    }

    #[test]
    fn an_own_state_goes_into_the_next_own_bundle() {
        let (mut world, character) = admitted();
        let mut writer = Writer::new();
        let state = GroupsState {
            group_id: Some([4; 16]),
            ..GroupsState::default()
        };
        state.write(&mut writer).expect("writes");
        world.apply_social_states(&SocialDeliveries {
            own_states: vec![OwnStateDelivery {
                character,
                kind: SocialStateKind::Groups,
                body: writer.finish(),
            }],
            ..SocialDeliveries::default()
        });
        let entity = world.player_by_number(PLAYER).expect("player");
        let player = world.world.get::<Player>(entity).expect("player");
        assert!(player.extra_members.iter().any(|member| {
            member.component == GROUPS_STATE_COMPONENT
                && member.state == MemberState::Groups(state.clone())
        }));
    }

    #[test]
    fn the_group_entity_gets_its_card_once_then_its_changes() {
        let (mut world, character) = admitted();
        let deliver = |world: &mut SimWorld, health| {
            world.apply_social_states(&SocialDeliveries {
                group_entities: vec![GroupEntityDelivery {
                    character,
                    view: Some(entity_view(character, health)),
                }],
                ..SocialDeliveries::default()
            });
        };
        deliver(&mut world, FULL_PERCENT);
        let number = side(&mut world).group_entity_number().expect("an entity");
        let new = Slot {
            interest: INTEREST,
            new: true,
        };
        let known = Slot {
            interest: INTEREST,
            new: false,
        };
        let card = side(&mut world)
            .group_entity_group(number, new)
            .expect("the card");
        assert_eq!(card.members.len(), 2);
        assert_eq!(card.members[1].component, GROUP_CARD_COMPONENT);
        assert!(side(&mut world).group_entity_group(number, known).is_none());
        deliver(&mut world, 50);
        let change = side(&mut world)
            .group_entity_group(number, known)
            .expect("the change");
        assert_eq!(change.members.len(), 1, "no card again");
        world.apply_social_states(&SocialDeliveries {
            group_entities: vec![GroupEntityDelivery {
                character,
                view: None,
            }],
            ..SocialDeliveries::default()
        });
        assert_eq!(side(&mut world).group_entity_number(), None);
    }

    #[test]
    fn health_changes_go_to_the_social_service_once() {
        let (mut world, character) = admitted();
        let first = world.social_vitals();
        let [SocialInput::Vitals(vitals)] = first.as_slice() else {
            panic!("one vitals input: {first:?}");
        };
        assert_eq!(vitals.character, character);
        assert!(world.social_vitals().is_empty(), "no change");
        assert_eq!(percent(1, 1000), 1);
        assert_eq!(percent(1000, 1000), FULL_PERCENT);
        assert_eq!(percent(0, 0), 0);
    }
}
