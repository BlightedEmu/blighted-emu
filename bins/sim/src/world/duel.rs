// SPDX-License-Identifier: AGPL-3.0-or-later
//! Duels (game mode study 2026-11-11: two duels of retail `duel_capture`). A duel is a small
//! game mode. It starts when the invited player accepts a `Duel` invite ([`super::invites`]).
//!
//! Observed, with the times of the first retail duel (seconds from the capture's start):
//!
//! - 67.299: the server spawns the game mode entity (a card of the duel mode's slice and the
//!   game mode state 2343 on component 2): script state 1, both players as participants with
//!   status 3 and teams 0 and 1, and a timer that ends 6 s later. Each player's participant
//!   state (3312) names the mode, with the join flags and its team.
//! - 73.315: script state 3 and a new timer 90 s later; each player's participant flags change
//!   and its temporary affiliation (3563) names its own team (kind 1) and the other (kind 4).
//!   From here the two can hurt each other.
//! - each hit between the two: the timer again, 90 s after the hit.
//! - 83.896: one player's health reaches 0. It goes down (the incapacitated state), it does
//!   not die. Script state 0, its status 11, an end value 0, the timer again; each player's
//!   affiliation is empty again and its participant flags change.
//! - 86.931: the loser's status is 3 again; it stands up with its whole health.
//! - 91.966: each player's participant state names no mode; the game mode entity goes.
//!
//! Inferred: a duel whose timer runs out ends with no loser; a duel ends when one of its
//! players leaves the sim. Unknown: the meaning of the participant flag bits and of the
//! status values, the names of the timers and of the end value (their CRC-32 keys are sent as
//! retail sent them), and the rule of the team ids (a pair of numbers one apart; this server
//! takes them from the game mode entity's slot key).

use bevy_ecs::entity::Entity;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::NS_PER_MS;
use nw_messages::game_mode_participant::{
    ActiveGameMode, GameModeParticipantState, PARTICIPANT_FLAGS_LEN,
};
use nw_messages::game_mode_state::GameModeState;
use nw_messages::items::FullList;
use nw_messages::list_delta::{ListChange, ReplicatedList};
use nw_messages::temporary_affiliation::Affiliation;
use nw_messages::wire::RAW_ID_LEN;

use super::attack::Vitals;
use super::game_mode_entity::{
    FIRST_LIST_VERSION, GAME_MODE_FACET, OTHER_TEAM_KIND, PLAYING_STATUS, full, left_state,
    mode_facet, participant_of,
};
use super::{Body, Player, SimWorld};

/// The `GameModeId` of a duel, and the activity of its invite.
pub(super) const DUEL_MODE: &str = "Duel";
/// The slice asset of the duel's game mode entity (Observed: the card of both retail duels).
const DUEL_MODE_ASSET: [u8; RAW_ID_LEN] = [
    0x96, 0xa9, 0xa5, 0xa7, 0xc9, 0x9c, 0x5a, 0x5c, 0xb0, 0x67, 0xf8, 0xa2, 0xb8, 0x72, 0xaa, 0xd6,
];
/// The namespace of the `GdeRef`s of duel entities ("duelmode").
const DUEL_NAMESPACE: u64 = 0x6475_656c_6d6f_6465;
/// The script states of a duel: the countdown, the fight, the end.
const COUNTDOWN_STATE: u8 = 1;
const FIGHT_STATE: u8 = 3;
const ENDED_STATE: u8 = 0;
/// The status of the loser from the duel's end until it stands up.
const LOST_STATUS: u8 = 11;
/// The keys of the duel's timers: the countdown's end and the fight's end.
const COUNTDOWN_TIMER: u32 = 0xf920_f724;
const FIGHT_TIMER: u32 = 0x47d6_e652;
/// The key of the value the duel's end sets to 0.
const END_VALUE: u32 = 0x428a_d202;
/// The countdown before the fight.
const COUNTDOWN_MS: u64 = 6_000;
/// The fight ends this long after its start or its last hit.
const FIGHT_MS: u64 = 90_000;
/// The loser stands up this long after the duel's end (Observed: 3.03 s).
const STAND_UP_MS: u64 = 3_000;
/// The players leave the mode this long after the duel's end (Observed: 8.05 s).
const LEAVE_MS: u64 = 8_000;
/// The participant flags of each phase.
const JOIN_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x00, 0x00, 0x17, 0x10, 0x00, 0x00];
const FIGHT_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0xef, 0x17, 0x17, 0x30, 0x00, 0x00];
const ENDED_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0xec, 0x17, 0x17, 0x70, 0x00, 0x00];
/// The kind of the own team's temporary affiliation in a duel (Observed: 1).
const OWN_TEAM_KIND: u32 = 1;
/// The players of a duel.
const DUELISTS: usize = 2;
/// The phase of a duel's mode in the participant state: none (Observed: 0).
const NO_PHASE: u64 = 0;

/// Where a duel is; its times are server ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The countdown: the fight starts at this sim ms.
    Countdown { fight_ms: u64 },
    /// The fight: it ends at this sim ms without a hit.
    Fight { ends_ms: u64 },
    /// The duel ended at `ended_ms`; `down` is the loser's index while it lies down.
    Ended { ended_ms: u64, down: Option<usize> },
}

/// One duel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Duel {
    /// The slot key of its game mode entity.
    entity: u64,
    /// Its players: number and entity, by team index.
    players: [(u64, Entity); DUELISTS],
    phase: Phase,
    /// The version of the newest list change of its game mode state.
    version: u64,
    /// The version of its players' participant lists.
    list_version: u64,
}

/// The duels of this sim.
#[derive(Debug, Default)]
pub(super) struct Duels {
    running: Vec<Duel>,
    /// The duels started so far: each takes two versions of its players' participant lists.
    started: u64,
}

impl Duels {
    /// Whether players `first` and `second` fight each other in a duel.
    pub(super) fn fighting(&self, first: u64, second: u64) -> bool {
        self.running.iter().any(|duel| {
            matches!(duel.phase, Phase::Fight { .. })
                && duel.index_of(first).is_some()
                && duel.index_of(second).is_some()
        })
    }

    /// Whether player `number` is in a duel.
    pub(super) fn dueling(&self, number: u64) -> bool {
        self.running
            .iter()
            .any(|duel| duel.index_of(number).is_some())
    }
}

impl Duel {
    fn index_of(&self, number: u64) -> Option<usize> {
        self.players
            .iter()
            .position(|(player, _)| *player == number)
    }
}

/// The team id of team `team` of the duel whose game mode entity has slot key `entity`.
fn team_id(entity: u64, team: usize) -> u32 {
    // The low bits of a slot key and a team index of 0 or 1.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the low bits name the team"
    )]
    let id = (entity as u32)
        .wrapping_mul(DUELISTS as u32)
        .wrapping_add(team as u32);
    id
}

impl SimWorld {
    /// Starts the duel of players `inviter` and `invitee`; `false` when one of them is not
    /// here, is dead or duels already. A duel's times are server times.
    pub(super) fn start_duel(&mut self, inviter: Entity, invitee: Entity) -> bool {
        let facts = |world: &Self, entity: Entity| {
            let player = world.world.get::<Player>(entity)?;
            let vitals = world.world.get::<Vitals>(entity)?;
            let at = world.world.get::<Body>(entity)?.motion.position();
            (!vitals.fallen() && !world.duels.dueling(player.number)).then_some((
                player.number,
                *player.character.as_bytes(),
                at,
            ))
        };
        let (Some(first), Some(second)) = (facts(self, inviter), facts(self, invitee)) else {
            return false;
        };
        let server_ms = self.server_ms;
        let mode = lowercase_crc32(DUEL_MODE);
        let state = GameModeState {
            script_state: Some(COUNTDOWN_STATE),
            spawned: Some(full(FIRST_LIST_VERSION, Vec::new())),
            mode: Some(mode),
            map: Some(mode),
            participants: Some(ReplicatedList::Full(full(
                FIRST_LIST_VERSION,
                vec![participant_of(first.0), participant_of(second.0)],
            ))),
            statuses: Some(ReplicatedList::Full(full(
                FIRST_LIST_VERSION,
                vec![PLAYING_STATUS; DUELISTS],
            ))),
            teams: Some(ReplicatedList::Full(full(FIRST_LIST_VERSION, vec![0, 1]))),
            characters: Some(ReplicatedList::Full(full(
                FIRST_LIST_VERSION,
                vec![first.1, second.1],
            ))),
            raids: Some(ReplicatedList::Full(full(FIRST_LIST_VERSION, Vec::new()))),
            values: Some(full(FIRST_LIST_VERSION, Vec::new())),
            timers: Some(full(
                FIRST_LIST_VERSION,
                vec![(COUNTDOWN_TIMER, server_ms.saturating_add(COUNTDOWN_MS))],
            )),
            no_affliction: true,
            linked_mode: Some(0),
            ..GameModeState::default()
        };
        let middle = nw_sim_core::Position {
            x: (first.2.x + second.2.x) / 2.0,
            y: (first.2.y + second.2.y) / 2.0,
            z: (first.2.z + second.2.z) / 2.0,
        };
        let (entity, gde_ref) =
            self.place_game_mode_entity(DUEL_NAMESPACE, Some(DUEL_MODE_ASSET), middle, state);
        self.duels.started += 1;
        let list_version = self.duels.started.saturating_mul(DUELISTS as u64);
        let mode_facet = mode_facet(&gde_ref);
        let joined = ActiveGameMode {
            raw_id: gde_ref,
            facet: mode_facet,
            second: mode_facet.wrapping_add(GAME_MODE_FACET),
            mode,
            map: mode,
            joined_ns: server_ms.saturating_mul(NS_PER_MS),
            phase: NO_PHASE,
        };
        for (team, entity) in [inviter, invitee].into_iter().enumerate() {
            self.put_participant_state(
                entity,
                GameModeParticipantState {
                    active: Some(full(list_version, vec![joined])),
                    flags: Some(JOIN_FLAGS),
                    // Two teams.
                    last_team: u8::try_from(team).ok(),
                    ..GameModeParticipantState::default()
                },
            );
        }
        self.duels.running.push(Duel {
            entity,
            players: [(first.0, inviter), (second.0, invitee)],
            phase: Phase::Countdown {
                fight_ms: server_ms.saturating_add(COUNTDOWN_MS),
            },
            version: FIRST_LIST_VERSION,
            list_version,
        });
        tracing::info!(first = first.0, second = second.0, "duel started");
        true
    }

    /// Sends the change `change` of the game mode state of the duel at `index`.
    fn change_duel_state(&mut self, index: usize, change: GameModeState) {
        let entity = self.duels.running[index].entity;
        self.change_game_mode_state(entity, change);
    }

    /// The fight timer of the duel at `index` from now, as a full list with a new version.
    fn fight_timer(&mut self, index: usize) -> FullList<(u32, u64)> {
        let duel = &mut self.duels.running[index];
        duel.version += 1;
        full(
            duel.version,
            vec![(FIGHT_TIMER, self.server_ms.saturating_add(FIGHT_MS))],
        )
    }

    /// A hit of player `attacker` on player `target` landed: the fight of their duel goes on
    /// [`FIGHT_MS`] more; a target that went down lost.
    pub(super) fn duel_hit(&mut self, attacker: u64, target: u64) {
        let now_ms = self.server_ms;
        let Some(index) = self.duels.running.iter().position(|duel| {
            matches!(duel.phase, Phase::Fight { .. })
                && duel.index_of(attacker).is_some()
                && duel.index_of(target).is_some()
        }) else {
            return;
        };
        let loser = self.duels.running[index].index_of(target).filter(|loser| {
            let entity = self.duels.running[index].players[*loser].1;
            self.world.get::<Vitals>(entity).is_some_and(Vitals::fallen)
        });
        if loser.is_some() {
            self.end_duel(index, loser, now_ms);
            return;
        }
        self.duels.running[index].phase = Phase::Fight {
            ends_ms: now_ms.saturating_add(FIGHT_MS),
        };
        let timers = self.fight_timer(index);
        self.change_duel_state(
            index,
            GameModeState {
                timers: Some(timers),
                ..GameModeState::default()
            },
        );
    }

    /// Ends the duel at `index` at `now_ms`; `loser` is the index of the player that went down.
    fn end_duel(&mut self, index: usize, loser: Option<usize>, now_ms: u64) {
        let timers = self.fight_timer(index);
        let duel = &mut self.duels.running[index];
        duel.phase = Phase::Ended {
            ended_ms: now_ms,
            down: loser,
        };
        let version = duel.version;
        let players = duel.players;
        let list_version = duel.list_version;
        self.change_duel_state(
            index,
            GameModeState {
                script_state: Some(ENDED_STATE),
                statuses: loser.map(|loser| {
                    ReplicatedList::Changes(vec![ListChange {
                        // A team index of 0 or 1.
                        index: loser as u32,
                        version,
                        value: LOST_STATUS,
                    }])
                }),
                values: Some(full(version, vec![(END_VALUE, 0)])),
                timers: Some(timers),
                ..GameModeState::default()
            },
        );
        for (_, entity) in players {
            self.put_affiliations(entity, list_version.saturating_add(1), Vec::new());
            self.put_participant_state(
                entity,
                GameModeParticipantState {
                    flags: Some(ENDED_FLAGS),
                    ..GameModeParticipantState::default()
                },
            );
        }
        tracing::info!(?players, ?loser, "duel ended");
    }

    /// Runs the duels: a countdown that ended starts its fight, a fight whose timer ran out
    /// ends with no loser, a loser stands up, and the players leave an ended duel. A duel one
    /// of whose players is gone ends at once.
    pub(super) fn tick_duels(&mut self) {
        let now_ms = self.server_ms;
        let mut index = 0;
        while index < self.duels.running.len() {
            let duel = self.duels.running[index].clone();
            let here = duel.players.iter().all(|(number, entity)| {
                self.world
                    .get::<Player>(*entity)
                    .is_some_and(|player| player.number == *number)
            });
            match duel.phase {
                _ if !here => {
                    self.leave_duel(index);
                    continue;
                }
                Phase::Countdown { fight_ms } if now_ms >= fight_ms => {
                    self.start_fight(index, now_ms)
                }
                Phase::Fight { ends_ms } if now_ms >= ends_ms => self.end_duel(index, None, now_ms),
                Phase::Ended { ended_ms, down } => {
                    if let Some(loser) = down
                        && now_ms >= ended_ms.saturating_add(STAND_UP_MS)
                    {
                        self.stand_loser_up(index, loser);
                    }
                    if now_ms >= ended_ms.saturating_add(LEAVE_MS) {
                        self.leave_duel(index);
                        continue;
                    }
                }
                Phase::Countdown { .. } | Phase::Fight { .. } => {}
            }
            index += 1;
        }
    }

    /// The fight of the duel at `index` starts at `now_ms`.
    fn start_fight(&mut self, index: usize, now_ms: u64) {
        self.duels.running[index].phase = Phase::Fight {
            ends_ms: now_ms.saturating_add(FIGHT_MS),
        };
        let timers = self.fight_timer(index);
        self.change_duel_state(
            index,
            GameModeState {
                script_state: Some(FIGHT_STATE),
                timers: Some(timers),
                ..GameModeState::default()
            },
        );
        let duel = self.duels.running[index].clone();
        for (team, (_, entity)) in duel.players.into_iter().enumerate() {
            self.put_participant_state(
                entity,
                GameModeParticipantState {
                    flags: Some(FIGHT_FLAGS),
                    ..GameModeParticipantState::default()
                },
            );
            self.put_affiliations(
                entity,
                duel.list_version,
                vec![
                    Affiliation {
                        id: team_id(duel.entity, team),
                        kind: OWN_TEAM_KIND,
                    },
                    Affiliation {
                        id: team_id(duel.entity, DUELISTS - 1 - team),
                        kind: OTHER_TEAM_KIND,
                    },
                ],
            );
        }
    }

    /// The loser (team `loser`) of the duel at `index` stands up with its whole health, and
    /// its status is the playing one again.
    fn stand_loser_up(&mut self, index: usize, loser: usize) {
        let duel = &mut self.duels.running[index];
        if let Phase::Ended { down, .. } = &mut duel.phase {
            *down = None;
        }
        duel.version += 1;
        let version = duel.version;
        let (number, entity) = duel.players[loser];
        if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) {
            vitals.health.refill();
            vitals.rise();
        }
        self.mobs.forget(number);
        self.change_duel_state(
            index,
            GameModeState {
                statuses: Some(ReplicatedList::Changes(vec![ListChange {
                    // A team index of 0 or 1.
                    index: loser as u32,
                    version,
                    value: PLAYING_STATUS,
                }])),
                ..GameModeState::default()
            },
        );
    }

    /// The players of the duel at `index` leave it: their participant states name no mode, a
    /// loser that still lies down stands up, and the game mode entity goes.
    fn leave_duel(&mut self, index: usize) {
        let duel = self.duels.running.remove(index);
        self.stations.remove_camp(duel.entity);
        for (_, entity) in duel.players {
            if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity)
                && vitals.downed()
            {
                vitals.health.refill();
                vitals.rise();
            }
            self.put_participant_state(entity, left_state(duel.list_version.saturating_add(1)));
        }
    }
}
