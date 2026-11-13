// SPDX-License-Identifier: AGPL-3.0-or-later
//! What every game mode the sim runs (a duel, an arena match, an Outpost Rush match) shares: its
//! game mode entity (a card and the game mode state 2343 on component 2), the change of that
//! state, the participant lists of a match, the participant state (3312) and the temporary
//! affiliations (3563) of its players, and what a player who leaves a match gets (game mode
//! study 2026-11-11, arena flow study 2026-11-11, Outpost Rush study 2026-11-12).
//!
//! Observed (retail `duel_capture`, `3v3`, `20261010-125035-658`): a participant of the game mode
//! state names the player's entity raw id, its facet entity and its `GameModeParticipantComponent`
//! id (the first half of the facet header of the player's own 3312 calls); the game mode
//! entity's facet entity is the first half of its raw id, low byte first; a player who leaves
//! gets the empty mode list, zero flags, the empty mutation context and team 255.

use bevy_ecs::entity::Entity;
use nw_messages::NS_PER_MS;
use nw_messages::facet::{FacetHeader, component, facet_entity};
use nw_messages::game_mode_participant::{
    ActiveGameMode, GAME_MODE_PARTICIPANT_COMPONENT, GameModeParticipantState,
    MutationContextWire, NO_TEAM, PARTICIPANT_FLAGS_LEN,
};
use nw_messages::game_mode_state::{GameModeState, GeneratedMap, ParticipantRef};
use nw_messages::items::FullList;
use nw_messages::list_delta::ReplicatedList;
use nw_messages::social::groups::REMOVED_LEFT;
use nw_messages::social::{CharacterRef, GroupPush};
use nw_messages::temporary_affiliation::{
    Affiliation, TEMPORARY_AFFILIATION_COMPONENT, TemporaryAffiliationState,
};
use nw_messages::wire::{RAW_ID_LEN, raw_id_of_entity};
use nw_messages::{
    AssetId, BundleMember, DYNAMIC_SLICE_SUB_ID, GdeMetadataReplicatedState, MemberState,
    ObjectivesState,
};
use nw_sim_core::Position;

use super::crafting::{CARD_COMPONENT, Station, dynamic_ref};
use super::{Player, SimWorld};

/// The component of the game mode state on the game mode entity.
pub(super) const GAME_MODE_COMPONENT: u64 = 2;
/// The component constant of the game mode component: its id in a participant state is the
/// game mode entity's facet entity plus it (Observed in both retail duels).
pub(super) const GAME_MODE_FACET: u64 = 0x8b87_8583_82e7_9dab;
/// The status of a participant that plays (Observed in every retail duel and arena).
pub(super) const PLAYING_STATUS: u8 = 3;
/// The first version of the lists of a game mode state (Observed: 2 in a duel).
pub(super) const FIRST_LIST_VERSION: u64 = 2;
/// The kind of the other team's affiliation (Observed: 4 in every retail duel and arena).
pub(super) const OTHER_TEAM_KIND: u32 = 4;
/// The kind of the own team's affiliation in an arena and a capture the flag match (Observed:
/// 0).
const OWN_TEAM_KIND: u32 = 0;
/// The 3455 value of every retail return from a match (Observed: 0 in the arena and in Outpost
/// Rush, `OPR-coral-divide2` 674.307 s, `OPR_TEST3` 1862.622 s).
const MODE_COMPLETED_VALUE: u32 = 0;

/// One player of a match on an instance sim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MatchParticipant {
    pub(super) number: u64,
    pub(super) entity: Entity,
    pub(super) character: [u8; RAW_ID_LEN],
    pub(super) team: u8,
    pub(super) place: u8,
    pub(super) status: u8,
}

/// The participant lists of a match's game mode state in full at `version`; the character ids
/// only when `characters` (the row's `ReplicateParticipantCharacterIds`; Observed: the arena's
/// list holds them, Outpost Rush's is empty, `opr_que_and_lost` 578.634 s).
pub(super) fn participant_lists(
    participants: &[MatchParticipant],
    version: u64,
    characters: bool,
) -> GameModeState {
    let refs = participants
        .iter()
        .map(|participant| participant_of(participant.number))
        .collect();
    let statuses = participants
        .iter()
        .map(|participant| participant.status)
        .collect();
    let teams = participants
        .iter()
        .map(|participant| participant.team)
        .collect();
    let characters = participants
        .iter()
        .filter(|_| characters)
        .map(|participant| participant.character)
        .collect();
    GameModeState {
        participants: Some(ReplicatedList::Full(full(version, refs))),
        statuses: Some(ReplicatedList::Full(full(version, statuses))),
        teams: Some(ReplicatedList::Full(full(version, teams))),
        characters: Some(ReplicatedList::Full(full(version, characters))),
        ..GameModeState::default()
    }
}

/// What a player's participant state names when it joins a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MatchEntry {
    /// The raw id of the game mode entity.
    pub(super) gde_ref: [u8; RAW_ID_LEN],
    /// The game mode's and the map's CRC.
    pub(super) mode: u32,
    pub(super) map: u32,
    /// The instance's phase id.
    pub(super) phase: u64,
    /// The participant flags.
    pub(super) flags: [u8; PARTICIPANT_FLAGS_LEN],
    /// The player's team.
    pub(super) team: u8,
    /// The version of the lists.
    pub(super) list_version: u64,
    /// The player's queue bars.
    pub(super) eligible: Vec<(u32, u64)>,
}
/// A list in its full form with `version`.
pub(super) fn full<T>(version: u64, elements: Vec<T>) -> FullList<T> {
    FullList {
        version: Some(version),
        elements,
    }
}

/// The list `held` with the list child `change`: a full list replaces it; changed elements go
/// into a held full list (an element one past its end is appended), which takes the highest
/// version of the changes (retail: the first change carries the version, the others 0).
fn merged_list<T: Clone>(
    held: Option<&ReplicatedList<T>>,
    change: Option<&ReplicatedList<T>>,
) -> Option<ReplicatedList<T>> {
    match (held, change) {
        (Some(ReplicatedList::Full(list)), Some(ReplicatedList::Changes(changes))) => {
            let mut list = list.clone();
            for change in changes {
                let Ok(index) = usize::try_from(change.index) else {
                    continue;
                };
                if let Some(element) = list.elements.get_mut(index) {
                    *element = change.value.clone();
                } else if index == list.elements.len() {
                    list.elements.push(change.value.clone());
                }
                list.version = list.version.max(Some(change.version));
            }
            Some(ReplicatedList::Full(list))
        }
        (held, None) => held.cloned(),
        (_, new) => new.cloned(),
    }
}

/// The game mode state `state` with the children `change` sets: a child of the change replaces
/// the held one; changed elements of the statuses and of the map's lists go into the held list;
/// an event of the change takes its slot of the feed.
pub(super) fn merged(state: &GameModeState, change: &GameModeState) -> GameModeState {
    let statuses = merged_list(state.statuses.as_ref(), change.statuses.as_ref());
    let mut events: Vec<_> = state
        .events
        .iter()
        .filter(|(slot, _)| change.events.iter().all(|(new, _)| new != slot))
        .chain(&change.events)
        .copied()
        .collect();
    events.sort_by_key(|(slot, _)| *slot);
    let (held, new) = (&state.level_map, &change.level_map);
    let level_map = GeneratedMap {
        origin_m: new.origin_m.or(held.origin_m),
        tile_size_m: new.tile_size_m.or(held.tile_size_m),
        size_in_tiles: new.size_in_tiles.or(held.size_in_tiles),
        tiles: merged_list(held.tiles.as_ref(), new.tiles.as_ref()),
        visited: merged_list(held.visited.as_ref(), new.visited.as_ref()),
        icons: merged_list(held.icons.as_ref(), new.icons.as_ref()),
    };
    GameModeState {
        script_state: change.script_state.or(state.script_state),
        spawned: change.spawned.clone().or_else(|| state.spawned.clone()),
        mode: change.mode.or(state.mode),
        map: change.map.or(state.map),
        participants: change
            .participants
            .clone()
            .or_else(|| state.participants.clone()),
        statuses,
        teams: change.teams.clone().or_else(|| state.teams.clone()),
        characters: change
            .characters
            .clone()
            .or_else(|| state.characters.clone()),
        raids: change.raids.clone().or_else(|| state.raids.clone()),
        values: change.values.clone().or_else(|| state.values.clone()),
        timers: change.timers.clone().or_else(|| state.timers.clone()),
        events,
        no_affliction: state.no_affliction,
        affliction: change.affliction.or(state.affliction),
        level_map,
        linked_mode: change.linked_mode.or(state.linked_mode),
    }
}

/// The affiliations of a player of team `team` of a two-team match: its own team, then the
/// other (Observed: every retail arena and capture the flag join).
pub(super) fn team_affiliations(team: u8) -> Vec<Affiliation> {
    vec![
        Affiliation {
            id: u32::from(team),
            kind: OWN_TEAM_KIND,
        },
        Affiliation {
            id: u32::from(1 - team.min(1)),
            kind: OTHER_TEAM_KIND,
        },
    ]
}

/// The participant reference of player `number`.
pub(super) fn participant_of(number: u64) -> ParticipantRef {
    ParticipantRef {
        raw_id: raw_id_of_entity(number),
        facet: facet_entity(number),
        second: FacetHeader::of(component::GAME_MODE_PARTICIPANT, number).target,
    }
}

/// The facet entity of the game mode entity of raw id `gde_ref`: the first half of the raw id,
/// low byte first.
pub(super) fn mode_facet(gde_ref: &[u8; RAW_ID_LEN]) -> u64 {
    let mut half = [0; size_of::<u64>()];
    half.copy_from_slice(&gde_ref[..size_of::<u64>()]);
    u64::from_le_bytes(half)
}

/// The participant state of a player that joined the match `entry` at server ms `server_ms`
/// ([`SimWorld::put_match_joined`]).
pub(super) fn match_joined_state(entry: MatchEntry, server_ms: u64) -> GameModeParticipantState {
    let facet = mode_facet(&entry.gde_ref);
    let version = entry.list_version;
    GameModeParticipantState {
        active: Some(full(
            version,
            vec![ActiveGameMode {
                raw_id: entry.gde_ref,
                facet,
                second: facet.wrapping_add(GAME_MODE_FACET),
                mode: entry.mode,
                map: entry.map,
                joined_ns: server_ms.saturating_mul(NS_PER_MS),
                phase: entry.phase,
            }],
        )),
        flags: Some(entry.flags),
        eligible: (!entry.eligible.is_empty()).then(|| full(version, entry.eligible)),
        last_team: Some(entry.team),
        ..GameModeParticipantState::default()
    }
}

/// The objectives state `state` with the children `change` sets: a child of the change replaces
/// the held one.
fn merged_objectives(state: &ObjectivesState, change: &ObjectivesState) -> ObjectivesState {
    ObjectivesState {
        grace_period_end: change.grace_period_end.or(state.grace_period_end),
        task_start_times: change
            .task_start_times
            .clone()
            .or_else(|| state.task_start_times.clone()),
        tracked: change.tracked.clone().or_else(|| state.tracked.clone()),
        completed: change.completed.clone().or_else(|| state.completed.clone()),
        active: change.active.clone().or_else(|| state.active.clone()),
        tasks: change.tasks.clone().or_else(|| state.tasks.clone()),
        poi_entities: change
            .poi_entities
            .clone()
            .or_else(|| state.poi_entities.clone()),
        dynamic_poi_version: change.dynamic_poi_version.or(state.dynamic_poi_version),
    }
}

/// The participant state of a player who left its game mode, at list version `version`.
pub(super) fn left_state(version: u64) -> GameModeParticipantState {
    GameModeParticipantState {
        active: Some(full(version, Vec::new())),
        flags: Some([0; PARTICIPANT_FLAGS_LEN]),
        mutation_context: Some(MutationContextWire::NONE),
        last_team: Some(NO_TEAM),
        ..GameModeParticipantState::default()
    }
}

impl SimWorld {
    /// Queues the participant state of player `entity` that joined the match `entry` at server
    /// ms `server_ms`: the active mode (the game mode entity, its mode, map and phase), the
    /// flags, the queue bars and the team (Observed: the first 3312 on every retail arena and
    /// Outpost Rush instance).
    pub(super) fn put_match_joined(&mut self, entity: Entity, entry: MatchEntry, server_ms: u64) {
        self.put_participant_state(entity, match_joined_state(entry, server_ms));
    }

    /// What player `entity` (`participant`) gets when it leaves its match: 3455 and 5283 (its
    /// own member removed), the participant state of no mode and no affiliations, at list
    /// version `list_version` (Observed: every retail arena and Outpost Rush return).
    pub(super) fn send_match_left(
        &mut self,
        entity: Entity,
        participant: &MatchParticipant,
        list_version: u64,
    ) {
        self.send_group_push(
            entity,
            participant.number,
            GroupPush::SiegeWarfareCompleted {
                value: MODE_COMPLETED_VALUE,
            },
        );
        self.send_group_push(
            entity,
            participant.number,
            GroupPush::MemberRemoved {
                character: CharacterRef(Some(participant.character)),
                reason: REMOVED_LEFT,
            },
        );
        self.put_participant_state(entity, left_state(list_version));
        self.put_affiliations(entity, list_version, Vec::new());
    }

    /// Places a game mode entity with the state `state` at `position`: its `GdeRef` is made in
    /// `namespace` (this sim's generation mixed in), its card names the slice `asset` (none for an
    /// arena match). Returns its slot key and raw id.
    pub(super) fn place_game_mode_entity(
        &mut self,
        namespace: u64,
        asset: Option<[u8; RAW_ID_LEN]>,
        position: Position,
        state: GameModeState,
    ) -> (u64, [u8; RAW_ID_LEN]) {
        self.place_game_mode_entity_on(GAME_MODE_COMPONENT, namespace, asset, position, state)
    }

    /// [`Self::place_game_mode_entity`] with the game mode state on component `component`
    /// (Observed: 1 in capture the flag, `CTF_QueueAndJoin_PickUpSpeedHealthAndFlagThenDied`
    /// 46.961 s).
    pub(super) fn place_game_mode_entity_on(
        &mut self,
        component: u64,
        namespace: u64,
        asset: Option<[u8; RAW_ID_LEN]>,
        position: Position,
        state: GameModeState,
    ) -> (u64, [u8; RAW_ID_LEN]) {
        self.place_game_mode_entity_with(component, namespace, asset, position, state, Vec::new())
    }

    /// [`Self::place_game_mode_entity_on`] with the members `extra` after the game mode state
    /// (an expedition's objectives, [`super::expedition_mode`]).
    pub(super) fn place_game_mode_entity_with(
        &mut self,
        component: u64,
        namespace: u64,
        asset: Option<[u8; RAW_ID_LEN]>,
        position: Position,
        state: GameModeState,
        extra: Vec<BundleMember>,
    ) -> (u64, [u8; RAW_ID_LEN]) {
        let namespace = namespace ^ self.me.generation;
        let mut gde_ref = [0; RAW_ID_LEN];
        let entity = self.stations.place_camp(|number| {
            // A slot key is far below `usize::MAX` on the 64-bit targets the server runs on.
            gde_ref = dynamic_ref(namespace, number as usize);
            Station {
                gde_ref,
                position,
                station_type: String::new(),
                name: Vec::new(),
                members: [
                    BundleMember {
                        component: CARD_COMPONENT,
                        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                            asset: asset.map(|id| AssetId {
                                id,
                                sub_id: DYNAMIC_SLICE_SUB_ID,
                            }),
                            gde_ref: Some(gde_ref),
                        }),
                    },
                    BundleMember {
                        component,
                        state: MemberState::GameMode(Box::new(state)),
                    },
                ]
                .into_iter()
                .chain(extra)
                .collect(),
            }
        });
        (entity, gde_ref)
    }

    /// Sends the change `change` of the game mode state of the game mode entity `entity`, and
    /// keeps it for the viewers that get the entity later.
    pub(super) fn change_game_mode_state(&mut self, entity: u64, change: GameModeState) {
        let Some(component) = self.stations.members(entity).and_then(|members| {
            members
                .iter()
                .find(|member| matches!(member.state, MemberState::GameMode(_)))
                .map(|member| member.component)
        }) else {
            return;
        };
        let merge = change.clone();
        self.stations.change_camp(
            entity,
            |members| {
                for member in members {
                    if let MemberState::GameMode(state) = &mut member.state {
                        **state = merged(state, &merge);
                    }
                }
            },
            vec![BundleMember {
                component,
                state: MemberState::GameMode(Box::new(change)),
            }],
        );
    }

    /// Sends the change `change` of the objectives member of the game mode entity `entity`, and
    /// keeps it for the viewers that get the entity later.
    pub(super) fn change_game_mode_objectives(&mut self, entity: u64, change: ObjectivesState) {
        let Some(component) = self.stations.members(entity).and_then(|members| {
            members
                .iter()
                .find(|member| matches!(member.state, MemberState::Objectives(_)))
                .map(|member| member.component)
        }) else {
            return;
        };
        let merge = change.clone();
        self.stations.change_camp(
            entity,
            |members| {
                for member in members {
                    if let MemberState::Objectives(state) = &mut member.state {
                        *state = merged_objectives(state, &merge);
                    }
                }
            },
            vec![BundleMember {
                component,
                state: MemberState::Objectives(change),
            }],
        );
    }

    /// Queues the participant state `state` of player `entity`.
    pub(super) fn put_participant_state(
        &mut self,
        entity: Entity,
        state: GameModeParticipantState,
    ) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.extra_members.push(BundleMember {
                component: GAME_MODE_PARTICIPANT_COMPONENT,
                state: MemberState::GameModeParticipant(state),
            });
        }
    }

    /// Queues the temporary affiliations `affiliations` of player `entity`, at list version
    /// `version`.
    pub(super) fn put_affiliations(
        &mut self,
        entity: Entity,
        version: u64,
        affiliations: Vec<Affiliation>,
    ) {
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.extra_members.push(BundleMember {
                component: TEMPORARY_AFFILIATION_COMPONENT,
                state: MemberState::TemporaryAffiliation(TemporaryAffiliationState {
                    affiliations: Some(ReplicatedList::Full(full(version, affiliations))),
                }),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::list_delta::ListChange;

    use super::*;

    #[test]
    fn a_change_replaces_its_children_and_sets_changed_statuses() {
        let held = GameModeState {
            script_state: Some(0),
            statuses: Some(ReplicatedList::Full(full(4, vec![3, 3]))),
            teams: Some(ReplicatedList::Full(full(4, vec![0, 1]))),
            timers: Some(full(4, vec![(1, 2)])),
            ..GameModeState::default()
        };
        let change = GameModeState {
            script_state: Some(3),
            statuses: Some(ReplicatedList::Changes(vec![ListChange {
                index: 1,
                version: 6,
                value: 7,
            }])),
            timers: Some(full(5, vec![(1, 9)])),
            ..GameModeState::default()
        };
        let state = merged(&held, &change);
        assert_eq!(state.script_state, Some(3));
        assert_eq!(
            state.statuses,
            Some(ReplicatedList::Full(full(6, vec![3, 7])))
        );
        assert_eq!(
            state.teams, held.teams,
            "a child the change does not set stays"
        );
        assert_eq!(state.timers, change.timers);
    }

    /// A later viewer gets every filled slot of the event feed: a new event takes its slot, the
    /// other slots stay (retail `opr_que_and_lost` 2016.046 s: ten slots after the ring turned).
    #[test]
    fn an_event_of_a_change_takes_its_slot_of_the_held_feed() {
        use nw_messages::game_mode_state::GameModeEvent;
        let event = |name, at_ms| GameModeEvent {
            name,
            at_ms,
            number: 0,
        };
        let held = GameModeState {
            events: vec![(0, event(1, 10)), (1, event(2, 20))],
            ..GameModeState::default()
        };
        let change = GameModeState {
            events: vec![(0, event(3, 30))],
            ..GameModeState::default()
        };
        let state = merged(&held, &change);
        assert_eq!(state.events, vec![(0, event(3, 30)), (1, event(2, 20))]);
        assert_eq!(
            merged(&state, &GameModeState::default()).events,
            state.events
        );
    }

    /// A later viewer of a catacombs run gets the map as it stands: a changed visited byte goes
    /// into the held list and an icon one past the list's end is appended (retail `CATACOMBS5`
    /// 338.115 s: icons 1 to 3 appended to a list of one).
    #[test]
    fn a_change_of_the_map_lists_goes_into_the_held_map() {
        use nw_messages::game_mode_state::MapIcon;
        let icon = |key| MapIcon {
            key,
            position_m: [64, 128],
        };
        let held = GameModeState {
            level_map: GeneratedMap {
                size_in_tiles: Some([2, 1]),
                visited: Some(ReplicatedList::Full(full(4, vec![0b01]))),
                icons: Some(ReplicatedList::Full(full(4, vec![icon(1)]))),
                ..GeneratedMap::default()
            },
            ..GameModeState::default()
        };
        let change = GameModeState {
            level_map: GeneratedMap {
                visited: Some(ReplicatedList::Changes(vec![ListChange {
                    index: 0,
                    version: 5,
                    value: 0b11,
                }])),
                icons: Some(ReplicatedList::Changes(vec![
                    ListChange {
                        index: 1,
                        version: 6,
                        value: icon(2),
                    },
                    ListChange {
                        index: 2,
                        version: 0,
                        value: icon(3),
                    },
                ])),
                ..GeneratedMap::default()
            },
            ..GameModeState::default()
        };
        let state = merged(&held, &change);
        assert_eq!(state.level_map.size_in_tiles, Some([2, 1]));
        assert_eq!(
            state.level_map.visited,
            Some(ReplicatedList::Full(full(5, vec![0b11])))
        );
        assert_eq!(
            state.level_map.icons,
            Some(ReplicatedList::Full(full(
                6,
                vec![icon(1), icon(2), icon(3)]
            )))
        );
    }

    #[test]
    fn the_mode_facet_is_the_first_half_of_the_raw_id_low_byte_first() {
        let mut gde_ref = [0; RAW_ID_LEN];
        gde_ref[0] = 0x43;
        gde_ref[7] = 0xf3;
        assert_eq!(mode_facet(&gde_ref), 0xf300_0000_0000_0043);
        let left = left_state(9);
        assert_eq!(left.last_team, Some(NO_TEAM));
        assert_eq!(left.mutation_context, Some(MutationContextWire::NONE));
    }
}
