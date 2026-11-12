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
    ActiveGameMode, GAME_MODE_PARTICIPANT_COMPONENT, GameModeParticipantState, NO_TEAM,
    PARTICIPANT_FLAGS_LEN,
};
use nw_messages::game_mode_state::{GameModeState, ParticipantRef};
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

/// The game mode state `state` with the children `change` sets: a child of the change replaces
/// the held one; changed elements of the statuses go into the held list.
pub(super) fn merged(state: &GameModeState, change: &GameModeState) -> GameModeState {
    let statuses = match (&state.statuses, &change.statuses) {
        (Some(ReplicatedList::Full(list)), Some(ReplicatedList::Changes(changes))) => {
            let mut list = list.clone();
            for change in changes {
                if let Some(status) = usize::try_from(change.index)
                    .ok()
                    .and_then(|index| list.elements.get_mut(index))
                {
                    *status = change.value;
                }
                list.version = Some(change.version);
            }
            Some(ReplicatedList::Full(list))
        }
        (held, None) => held.clone(),
        (_, new) => new.clone(),
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
        no_affliction: state.no_affliction,
        linked_mode: change.linked_mode.or(state.linked_mode),
    }
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

/// The participant state of a player who left its game mode, at list version `version`.
pub(super) fn left_state(version: u64) -> GameModeParticipantState {
    GameModeParticipantState {
        active: Some(full(version, Vec::new())),
        flags: Some([0; PARTICIPANT_FLAGS_LEN]),
        no_mutation_context: true,
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
        let facet = mode_facet(&entry.gde_ref);
        let version = entry.list_version;
        self.put_participant_state(
            entity,
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
            },
        );
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
                members: vec![
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
                        component: GAME_MODE_COMPONENT,
                        state: MemberState::GameMode(state),
                    },
                ],
            }
        });
        (entity, gde_ref)
    }

    /// Sends the change `change` of the game mode state of the game mode entity `entity`, and
    /// keeps it for the viewers that get the entity later.
    pub(super) fn change_game_mode_state(&mut self, entity: u64, change: GameModeState) {
        let merge = change.clone();
        self.stations.change_camp(
            entity,
            |members| {
                for member in members {
                    if let MemberState::GameMode(state) = &mut member.state {
                        *state = merged(state, &merge);
                    }
                }
            },
            vec![BundleMember {
                component: GAME_MODE_COMPONENT,
                state: MemberState::GameMode(change),
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

    #[test]
    fn the_mode_facet_is_the_first_half_of_the_raw_id_low_byte_first() {
        let mut gde_ref = [0; RAW_ID_LEN];
        gde_ref[0] = 0x43;
        gde_ref[7] = 0xf3;
        assert_eq!(mode_facet(&gde_ref), 0xf300_0000_0000_0043);
        let left = left_state(9);
        assert_eq!(left.last_team, Some(NO_TEAM));
        assert!(left.no_mutation_context);
    }
}
