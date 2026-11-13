// SPDX-License-Identifier: AGPL-3.0-or-later
//! The 3v3 arena match on its instance sim (arena flow study 2026-11-11,
//! `local-data/findings/2026-11-11-arena-3v3-flow.md` sections 2 to 4): the game mode entity
//! (2343), the players' participant states (3312) and temporary affiliations (3563), the rounds,
//! the deaths, the end and the return.
//!
//! Observed (retail `3v3` and `20261010-125035-658` on `NW_Arena01`, `arena-4165c8761c29c499` on
//! `NW_Arena_3v3_04`, `3v3_arena_group_2` with six players):
//!
//! - The game mode entity: a card with no asset and the game mode state: script state 0, the
//!   mode's and the map's CRC, each participant with status 3, its team and its character, one
//!   raid id per team (two in a row), the values (the round wins and round flags of the two teams
//!   and a random number) and the timers `GameModeTimeout` (+3 h) and `PregameTimer` (+90 s).
//! - A player's participant state names the mode with flags `03c0e7080000` and its team; its
//!   affiliations name its own team (kind 0) and the other (kind 4), and stay for the whole match.
//! - Script states: 0 the pregame, 3 a round (`HealingDebuffTimer` +30 s, `RoundTimer`), 5 the
//!   overtime (`OvertimeTimer` +10 s, then no timer until a death), 4 a round's end
//!   (`PostRoundTimer` +3 s), 1 between rounds (`InterRoundTimer` +15 s), 2 the end
//!   (`WinningTeamIdx`, `Cleanup` +60 s). Round 1 starts from state 0.
//! - Flags: `03e0e7080000` in states 3 and 5, `03e0e7090000` in state 4, `03c0e7080000` else.
//! - A ready-up (6824) sets status bit `0x20`; the round's first body sends every status 3 again;
//!   a ready-up does not shorten a timer.
//! - A death: status 7 (no death's door) and 4977 to every participant; the team with no living
//!   player loses the round: its foe's win count and round flag rise; the next state clears the
//!   flag and the dead stand again with status 3, every player at the other team's spot (the
//!   teams change sides each round, 3 of 3 matches).
//! - The first team with 3 round wins wins (2 matches: 3-0); a team with no participant left
//!   loses (`3v3` 638.099 s: the other player's entry went, state 2 and `WinningTeamIdx` 1).
//! - `HealingDebuffTimer` spawns the healing debuff slice, `RoundTimer` the overtime slice (and on
//!   `NW_Arena_3v3_04` its sulfur ring); their damage is not on the wire (Unknown) and this sim
//!   deals none.
//! - The client's 1980 `Leave` (CRC `9bb080d0`, 3 of 3 returns) is answered with 3455 (`0`), 5283
//!   (the own character), 3951, the removal of the participant (full lists) and the move back to
//!   where the player queued; on the open world 4970 follows. A player who leaves a running match
//!   may not queue again for 30 minutes (2 leaves).
//!
//! Inferred: the players still in the match at the end of `Cleanup` go back (no capture waited
//! that long); PvP between the two teams only in states 3 and 5; the round time of a map
//! (Observed: 120 s on `NW_Arena01` in 6 rounds, 30 s on `NW_Arena_3v3_04` in 3 rounds; 120 s on
//! the other maps).

use bevy_ecs::entity::Entity;
use nw_bus::ArenaSeat;
use nw_content::instances::{InstanceKind, InstanceMap};
use nw_messages::components::DelayedEventState;
use nw_messages::game_mode_participant::{GameModeParticipantState, PARTICIPANT_FLAGS_LEN};
use nw_messages::game_mode_state::{GameModeState, Keyed};
use nw_messages::list_delta::{ListChange, ReplicatedList};
use nw_messages::position::{PositionInTheWorldReplicatedState, WorldPosition};
use nw_messages::social::{GroupPush, SocialFacet, SocialPush, push_envelope};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    AssetId, BundleMember, DYNAMIC_SLICE_SUB_ID, DeathRecapForAllMsg, GdeMetadataReplicatedState,
    MemberState, ServerMessage,
};
use nw_sim_core::Position;

use super::attack::Vitals;
use super::capture_the_flag::CaptureTheFlagMatch;
use super::crafting::{CARD_COMPONENT, Station, dynamic_ref};
use super::game_mode_entity::{
    FIRST_LIST_VERSION, MatchEntry, MatchParticipant as Participant, PLAYING_STATUS, full,
    participant_lists, team_affiliations,
};
use super::outpost_rush::OutpostRushMatch;
use super::queue::team_spot;
use super::{MESSAGE_CHANNEL, Outgoing, Placement, Player, SimWorld, unix_ns};

/// The script states (Observed).
const PREGAME: u8 = 0;
const BETWEEN_ROUNDS: u8 = 1;
const ENDED: u8 = 2;
const ROUND: u8 = 3;
const ROUND_ENDED: u8 = 4;
const OVERTIME: u8 = 5;
/// The timer keys (CRC-32 of the lowercase names; arena flow study, section 3.1).
pub(super) const GAME_MODE_TIMEOUT: u32 = 0xd815_d620;
pub(super) const PREGAME_TIMER: u32 = 0xf4f6_b51e;
const INTER_ROUND_TIMER: u32 = 0xb9e3_ba2d;
const ROUND_TIMER: u32 = 0x9242_bfa1;
const HEALING_DEBUFF_TIMER: u32 = 0xd1a3_6145;
const POST_ROUND_TIMER: u32 = 0x7eb0_604b;
pub(super) const OVERTIME_TIMER: u32 = 0x42fe_b1a0;
pub(super) const CLEANUP_TIMER: u32 = 0xca48_34d9;
/// The timer lengths (Observed).
pub(super) const GAME_MODE_TIMEOUT_MS: u64 = 10_800_000;
pub(super) const PREGAME_MS: u64 = 90_000;
const INTER_ROUND_MS: u64 = 15_000;
const HEALING_DEBUFF_MS: u64 = 30_000;
const POST_ROUND_MS: u64 = 3_000;
const OVERTIME_MS: u64 = 10_000;
const CLEANUP_MS: u64 = 60_000;
/// The round time of a map with no observed one, and the observed ones by level folder.
const DEFAULT_ROUND_MS: u64 = 120_000;
const ROUND_MS_BY_MAP: [(&str, u64); 2] = [("nw_arena01", 120_000), ("nw_arena_3v3_04", 30_000)];
/// The value keys in their retail order: team 1's round wins and round flag, the random number,
/// team 0's round wins and round flag; then `WinningTeamIdx` at the end (names Unknown but the
/// last; their meaning Inferred from the values).
const TEAM_1_WINS: u32 = 0x97d3_993b;
const TEAM_1_WON_ROUND: u32 = 0xb08a_6e7b;
const MATCH_NUMBER: u32 = 0x11f0_067d;
const TEAM_0_WINS: u32 = 0x0eda_c881;
const TEAM_0_WON_ROUND: u32 = 0x2983_3fc1;
pub(super) const WINNING_TEAM: u32 = 0x428a_d202;
/// Round wins that win the match.
pub(super) const ROUNDS_TO_WIN: u32 = 3;
/// The end events by the rounds a team won (Observed: the winners of `arena` 159.741 s and `3v3`
/// 638.083 s got `Arena3v3_Win_3_Round`; no ledger holds a loser's end). Inferred from the
/// content rows: a team that lost with 1 or 2 round wins gets `Arena3v3_Win_1_Round` or
/// `_Win_2_Round` (rewards between the loss and the win), one with none `Arena3v3_Loss`.
const ARENA_END_EVENTS: [&str; ROUNDS_TO_WIN as usize + 1] = [
    "Arena3v3_Loss",
    "Arena3v3_Win_1_Round",
    "Arena3v3_Win_2_Round",
    "Arena3v3_Win_3_Round",
];

/// The end event of a team that won `wins` rounds.
fn arena_end_event(wins: u32) -> &'static str {
    let index = usize::try_from(wins.min(ROUNDS_TO_WIN)).unwrap_or_default();
    ARENA_END_EVENTS[index]
}
/// The status bits: ready, and dead in the round (Observed `23` and `07`).
pub(super) const READY_BIT: u8 = 0x20;
const DEAD_STATUS: u8 = 0x07;
/// The participant flags of each phase (Observed).
const MATCH_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x03, 0xc0, 0xe7, 0x08, 0x00, 0x00];
const FIGHT_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x03, 0xe0, 0xe7, 0x08, 0x00, 0x00];
const ROUND_OVER_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x03, 0xe0, 0xe7, 0x09, 0x00, 0x00];
/// The teams of a match.
const TEAMS: usize = 2;
/// The CRC-32 of `leave`: the 1980 event of the end screen's leave button (exe strings).
pub(super) const LEAVE_EVENT: u32 = 0x9bb0_80d0;
/// A leave of a running match bars the queue this long (Observed: 1800.5 s and 1799 s).
pub(super) const LEAVER_PENALTY_NS: u64 = 1_800_000_000_000;
/// The namespaces of the `GdeRef`s of the match entity and the hazard entities ("arenamat",
/// "arenahaz").
const MATCH_NAMESPACE: u64 = 0x6172_656e_616d_6174;
const HAZARD_NAMESPACE: u64 = 0x6172_656e_6168_617a;
/// The first raid id of a sim's matches; each match takes two in a row (Observed: `20b` and
/// `20c`, `433` and `434`; the base is our choice).
pub(super) const RAID_ID_BASE: u64 = 0x200;
/// The component of a hazard entity's position (Observed: 2 for the healing debuff and the
/// overtime slices, 1 for the sulfur ring).
const HAZARD_POSITION_COMPONENT: u64 = 2;
const RING_POSITION_COMPONENT: u64 = 1;
/// The overtime slice's components besides its card and position (Observed `3v3` 616.665 s and
/// `arena-4165c8761c29c499` 12.722 s): class 1647 with the same bytes, and a delayed event of
/// 3 s on component 3.
const OVERTIME_RECORDED_COMPONENT: u64 = 4;
const OVERTIME_RECORDED_CLASS: u64 = 1647;
const OVERTIME_RECORDED_BODY: [u8; 15] = [
    0x01, 0x03, 0x00, 0x01, 0x02, 0x01, 0x00, 0x00, 0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00,
];
const OVERTIME_DELAY_COMPONENT: u64 = 3;
const OVERTIME_DELAY_S: f32 = 3.0;

/// When a hazard spawns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HazardTime {
    /// At `HealingDebuffTimer`.
    HealingDebuff,
    /// At `RoundTimer`.
    Overtime,
}

/// The kind of a hazard entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HazardShape {
    /// The healing debuff slice: its card and position.
    Debuff,
    /// The overtime slice: its card, position, class 1647 and a delayed event.
    Overtime,
    /// The sulfur ring: its card and position on component 1.
    Ring,
}

/// A hazard entity of a map: its level folder, when it spawns, its slice asset and where
/// (Observed; the other maps' hazards are Unknown and spawn nothing).
struct Hazard {
    folder: &'static str,
    time: HazardTime,
    shape: HazardShape,
    asset: [u8; RAW_ID_LEN],
    position: [f32; 3],
}

/// The hazards of the retail ledgers: `NW_Arena01` (`arena01_scalinghealingdebuff_damage`,
/// `arena01_3v3_overtime_damage`, `3v3` 424.437 s and 616.665 s) and `NW_Arena_3v3_04`
/// (`arena04_3v3_overtime_damage` and `sulfurring_arena_04`, `arena-4165c8761c29c499` 75.552 s).
const HAZARDS: [Hazard; 4] = [
    Hazard {
        folder: "nw_arena01",
        time: HazardTime::HealingDebuff,
        shape: HazardShape::Debuff,
        asset: [
            0xc0, 0x7e, 0x13, 0xfd, 0x95, 0x13, 0x57, 0xdb, 0xae, 0xfa, 0x8d, 0x93, 0x16, 0x7a,
            0x0e, 0x1a,
        ],
        position: [939.364, 900.433_2, 226.49],
    },
    Hazard {
        folder: "nw_arena01",
        time: HazardTime::Overtime,
        shape: HazardShape::Overtime,
        asset: [
            0x3c, 0xf0, 0xbc, 0x9e, 0xbe, 0x3c, 0x50, 0x3c, 0xa0, 0xc4, 0x17, 0x69, 0x8a, 0x74,
            0x57, 0xe7,
        ],
        position: [939.364, 900.433_2, 225.0],
    },
    Hazard {
        folder: "nw_arena_3v3_04",
        time: HazardTime::Overtime,
        shape: HazardShape::Overtime,
        asset: [
            0xad, 0x21, 0xb4, 0x59, 0xf7, 0x1f, 0x51, 0x12, 0x8c, 0x92, 0x5a, 0xd1, 0xaf, 0x41,
            0xc8, 0xa2,
        ],
        position: [1232.0, 1668.0, 99.0],
    },
    Hazard {
        folder: "nw_arena_3v3_04",
        time: HazardTime::Overtime,
        shape: HazardShape::Ring,
        asset: [
            0xed, 0x95, 0x3c, 0x1b, 0x8c, 0x7c, 0x52, 0xd5, 0x99, 0xf2, 0x1a, 0x04, 0x69, 0x93,
            0x8a, 0xf9,
        ],
        position: [1232.0, 1668.0, 99.0],
    },
];

/// The running arena match of this instance sim.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ArenaMatch {
    /// The slot key of the game mode entity.
    entity: u64,
    gde_ref: [u8; RAW_ID_LEN],
    crc: u32,
    map: InstanceMap,
    round_ms: u64,
    participants: Vec<Participant>,
    state: u8,
    /// The server ms when the current state's timer ends (the round's `RoundTimer`, the
    /// overtime's `OvertimeTimer` until it went, ...); `None` in an overtime after its timer.
    ends_ms: Option<u64>,
    /// The server ms of the round's `HealingDebuffTimer`, until it went.
    debuff_ms: Option<u64>,
    /// The server ms of `GameModeTimeout`.
    timeout_ms: u64,
    wins: [u32; TEAMS],
    /// The team that won the last round, while its flag is up.
    round_winner: Option<u8>,
    winner: Option<u8>,
    number: u64,
    raids: [u64; TEAMS],
    /// The version of the newest list of the game mode state.
    version: u64,
    /// The version of the players' participant and affiliation lists.
    list_version: u64,
    /// The teams stand at each other's spots (every other round).
    swapped: bool,
    /// The slot keys of the hazard entities of the round.
    hazards: Vec<u64>,
    /// Both teams had a participant here: from then on a team with none loses.
    both_teams_seen: bool,
}

/// The matches of this instance sim: the running arena match, how many ran, and the running
/// Outpost Rush match ([`super::outpost_rush`]) and the running capture the flag match
/// ([`super::capture_the_flag`]).
#[derive(Debug, Default)]
pub(super) struct ArenaMatches {
    running: Option<ArenaMatch>,
    pub(super) started: u64,
    pub(super) outpost_rush: Option<OutpostRushMatch>,
    pub(super) capture_the_flag: Option<CaptureTheFlagMatch>,
}

impl ArenaMatches {
    /// The team raid id and the team place of player `number` in the arena, Outpost Rush or
    /// capture the flag match this sim runs ([`super::raids`]); `None` for a player in none.
    pub(super) fn raid_seat_of(&self, number: u64) -> Option<(u64, u8)> {
        self.running
            .as_ref()
            .and_then(|running| running.raid_seat_of(number))
            .or_else(|| {
                self.outpost_rush
                    .as_ref()
                    .and_then(|running| running.raid_seat_of(number))
            })
            .or_else(|| {
                self.capture_the_flag
                    .as_ref()
                    .and_then(|running| running.raid_seat_of(number))
            })
    }
}

impl ArenaMatch {
    fn of(&self, number: u64) -> Option<&Participant> {
        self.participants
            .iter()
            .find(|participant| participant.number == number)
    }

    /// The team raid id and the team place of player `number`.
    fn raid_seat_of(&self, number: u64) -> Option<(u64, u8)> {
        let participant = self.of(number)?;
        let raid = *self.raids.get(usize::from(participant.team))?;
        Some((raid, participant.place))
    }

    fn next_version(&mut self) -> u64 {
        self.version += 1;
        self.version
    }

    fn alive(&self, team: u8) -> bool {
        self.participants.iter().any(|participant| {
            participant.team == team && participant.status & DEAD_STATUS != DEAD_STATUS
        })
    }

    fn present(&self, team: u8) -> bool {
        self.participants
            .iter()
            .any(|participant| participant.team == team)
    }

    /// The values list: the round wins and flags of both teams, the match number, and
    /// `WinningTeamIdx` once a team won.
    fn values(&self) -> Vec<Keyed> {
        let flag = |team: u8| u64::from(self.round_winner == Some(team));
        let mut values = vec![
            (TEAM_1_WINS, u64::from(self.wins[1])),
            (TEAM_1_WON_ROUND, flag(1)),
            (MATCH_NUMBER, self.number),
            (TEAM_0_WINS, u64::from(self.wins[0])),
            (TEAM_0_WON_ROUND, flag(0)),
        ];
        if let Some(winner) = self.winner {
            values.push((WINNING_TEAM, u64::from(winner)));
        }
        values
    }

    /// The participant lists of the game mode state, in full at a new version.
    fn participant_lists(&mut self) -> GameModeState {
        let version = self.next_version();
        participant_lists(&self.participants, version, true)
    }

    /// The change of a new script state with `timers` (the game mode timeout first, but in the
    /// end state) and the values.
    fn script_change(&mut self, state: u8, timers: Vec<Keyed>) -> GameModeState {
        self.state = state;
        let version = self.next_version();
        GameModeState {
            script_state: Some(state),
            values: Some(full(version, self.values())),
            timers: Some(full(version, timers)),
            ..GameModeState::default()
        }
    }
}

/// The round time of the map of level folder `folder`.
fn round_ms_of(folder: &str) -> u64 {
    ROUND_MS_BY_MAP
        .iter()
        .find(|(map, _)| *map == folder)
        .map_or(DEFAULT_ROUND_MS, |(_, ms)| *ms)
}

impl SimWorld {
    /// Whether players `first` and `second` are foes in the arena match: participants of two
    /// teams, both alive, in a round or its overtime.
    pub(super) fn arena_foes(&self, first: u64, second: u64) -> bool {
        let Some(running) = &self.arena_matches.running else {
            return false;
        };
        if !matches!(running.state, ROUND | OVERTIME) {
            return false;
        }
        match (running.of(first), running.of(second)) {
            (Some(first), Some(second)) => {
                first.team != second.team
                    && first.status & DEAD_STATUS != DEAD_STATUS
                    && second.status & DEAD_STATUS != DEAD_STATUS
            }
            _ => false,
        }
    }

    /// Whether this sim runs an arena match.
    pub(super) const fn in_arena_match(&self) -> bool {
        self.arena_matches.running.is_some()
    }

    /// Whether the arena round rule holds now: a round or its overtime runs (Observed: the
    /// four retail 2139 warnings of an arena, 15 s left, came in states 3 and 5).
    pub(super) fn in_arena_round(&self) -> bool {
        self.arena_matches
            .running
            .as_ref()
            .is_some_and(|running| matches!(running.state, ROUND | OVERTIME))
    }

    /// Player `entity` arrived on this instance sim with its seat `seat`: it joins the match
    /// (the first one opens it) with its participant state and affiliations; on an Outpost Rush
    /// instance it joins that match ([`super::outpost_rush`]), in a catacombs run that run
    /// ([`super::catacombs`]).
    pub(super) fn join_arena_match(&mut self, entity: Entity, seat: ArenaSeat) {
        if self.runs_capture_the_flag() {
            self.join_capture_the_flag(entity, seat);
            return;
        }
        if self.runs_outpost_rush() {
            self.join_outpost_rush(entity, seat);
            return;
        }
        if self.catacomb_run().is_some() {
            self.join_catacombs(entity);
            return;
        }
        let Some((number, character, eligible)) = self.world.get::<Player>(entity).map(|player| {
            (
                player.number,
                *player.character.as_bytes(),
                player.arena.queue_eligible().to_vec(),
            )
        }) else {
            return;
        };
        if self.arena_matches.running.is_none() && !self.open_arena_match() {
            return;
        }
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        if running.of(number).is_some() || running.state == ENDED {
            return;
        }
        running.participants.push(Participant {
            number,
            entity,
            character,
            team: seat.team,
            place: seat.place,
            status: PLAYING_STATUS,
        });
        running.both_teams_seen |= (0..TEAMS as u8).all(|team| running.present(team));
        let lists = running.participant_lists();
        let (game_mode_entity, gde_ref, crc, map_crc) = (
            running.entity,
            running.gde_ref,
            running.crc,
            running.map.map_crc,
        );
        running.list_version += 1;
        let list_version = running.list_version;
        let phase = self.instance_run.as_ref().map_or(0, |run| run.phase());
        self.change_game_mode_state(game_mode_entity, lists);
        self.put_match_joined(
            entity,
            MatchEntry {
                gde_ref,
                mode: crc,
                map: map_crc,
                phase,
                flags: MATCH_FLAGS,
                team: seat.team,
                list_version,
                eligible,
            },
            server_ms,
        );
        self.put_affiliations(entity, list_version, team_affiliations(seat.team));
        tracing::info!(
            player = number,
            team = seat.team,
            "player joined the arena match"
        );
    }

    /// Opens the match of this sim's arena instance: its game mode entity in state 0. `false`
    /// when this sim runs no arena.
    fn open_arena_match(&mut self) -> bool {
        let content = std::sync::Arc::clone(&self.content);
        let Some(mode) = self
            .instance_run
            .as_ref()
            .and_then(|run| content.instances.by_id(run.game_mode()))
            .filter(|mode| mode.kind == InstanceKind::Arena)
        else {
            return false;
        };
        let Some(map) = mode.map_of(self.settings.level.folder).cloned() else {
            return false;
        };
        let server_ms = self.server_ms;
        let started = self.arena_matches.started;
        self.arena_matches.started += 1;
        let raid = RAID_ID_BASE + started * TEAMS as u64;
        let number = unix_ns(server_ms) ^ started.rotate_left(32);
        let mut running = ArenaMatch {
            entity: 0,
            gde_ref: [0; RAW_ID_LEN],
            crc: mode.crc,
            round_ms: round_ms_of(&map.folder),
            map,
            participants: Vec::new(),
            state: PREGAME,
            ends_ms: Some(server_ms.saturating_add(PREGAME_MS)),
            debuff_ms: None,
            timeout_ms: server_ms.saturating_add(GAME_MODE_TIMEOUT_MS),
            wins: [0; TEAMS],
            round_winner: None,
            winner: None,
            number,
            raids: [raid, raid + 1],
            version: FIRST_LIST_VERSION,
            list_version: FIRST_LIST_VERSION,
            swapped: false,
            hazards: Vec::new(),
            both_teams_seen: false,
        };
        let version = running.version;
        let state = GameModeState {
            script_state: Some(PREGAME),
            spawned: Some(full(version, Vec::new())),
            mode: Some(running.crc),
            map: Some(running.map.map_crc),
            participants: Some(ReplicatedList::Full(full(version, Vec::new()))),
            statuses: Some(ReplicatedList::Full(full(version, Vec::new()))),
            teams: Some(ReplicatedList::Full(full(version, Vec::new()))),
            characters: Some(ReplicatedList::Full(full(version, Vec::new()))),
            raids: Some(ReplicatedList::Full(full(version, running.raids.to_vec()))),
            values: Some(full(version, running.values())),
            timers: Some(full(
                version,
                vec![
                    (GAME_MODE_TIMEOUT, running.timeout_ms),
                    (PREGAME_TIMER, server_ms.saturating_add(PREGAME_MS)),
                ],
            )),
            no_affliction: true,
            linked_mode: Some(0),
            ..GameModeState::default()
        };
        let middle = middle_of(&running.map);
        let (entity, gde_ref) = self.place_game_mode_entity(MATCH_NAMESPACE, None, middle, state);
        running.entity = entity;
        running.gde_ref = gde_ref;
        tracing::info!(game_mode = %mode.id, level = %running.map.folder, "arena match opened");
        self.arena_matches.running = Some(running);
        true
    }

    /// 6824: player `number` readied up (or not) in the pregame or between rounds (or in an
    /// Outpost Rush pregame).
    pub(super) fn arena_ready(&mut self, number: u64, ready: bool) {
        if self.arena_matches.capture_the_flag.is_some() {
            self.capture_the_flag_ready(number, ready);
            return;
        }
        if self.arena_matches.outpost_rush.is_some() {
            self.outpost_rush_ready(number, ready);
            return;
        }
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        if !matches!(running.state, PREGAME | BETWEEN_ROUNDS) {
            return;
        }
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.number == number)
        else {
            return;
        };
        let status = if ready {
            running.participants[index].status | READY_BIT
        } else {
            running.participants[index].status & !READY_BIT
        };
        running.participants[index].status = status;
        let version = running.next_version();
        let entity = running.entity;
        self.change_game_mode_state(
            entity,
            GameModeState {
                statuses: Some(ReplicatedList::Changes(vec![ListChange {
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                    version,
                    value: status,
                }])),
                ..GameModeState::default()
            },
        );
    }

    /// Player `target` died to `attacker`: in the match, its status is dead, every participant
    /// gets the 4977, and a team with no living player loses the round (in an Outpost Rush
    /// match: its rule, [`super::outpost_rush`]).
    pub(super) fn arena_death(&mut self, target: u64, attacker: u64) {
        if self.arena_matches.capture_the_flag.is_some() {
            self.capture_the_flag_death(target, attacker);
            return;
        }
        if self.arena_matches.outpost_rush.is_some() {
            self.outpost_rush_death(target, attacker);
            return;
        }
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.number == target)
        else {
            return;
        };
        running.participants[index].status = DEAD_STATUS;
        let dead = running.participants[index].clone();
        let killer = running
            .of(attacker)
            .map_or(dead.character, |killer| killer.character);
        let version = running.next_version();
        let entity = running.entity;
        let watchers: Vec<Entity> = running
            .participants
            .iter()
            .map(|participant| participant.entity)
            .collect();
        let lost = matches!(running.state, ROUND | OVERTIME) && !running.alive(dead.team);
        self.change_game_mode_state(
            entity,
            GameModeState {
                statuses: Some(ReplicatedList::Changes(vec![ListChange {
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                    version,
                    value: DEAD_STATUS,
                }])),
                ..GameModeState::default()
            },
        );
        let recap = ServerMessage::DeathRecapForAll(DeathRecapForAllMsg::of(
            dead.number,
            dead.character,
            killer,
        ));
        for watcher in watchers {
            self.send_to(watcher, std::slice::from_ref(&recap));
        }
        if lost {
            self.end_arena_round(1 - dead.team.min(1));
        }
    }

    /// Team `winner` won the round: state 4 with its win and flag, the round flags of every
    /// player, and the hazards go.
    fn end_arena_round(&mut self, winner: u8) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        running.wins[usize::from(winner)] += 1;
        running.round_winner = Some(winner);
        running.ends_ms = Some(server_ms.saturating_add(POST_ROUND_MS));
        running.debuff_ms = None;
        let timers = vec![
            (GAME_MODE_TIMEOUT, running.timeout_ms),
            (POST_ROUND_TIMER, server_ms.saturating_add(POST_ROUND_MS)),
        ];
        let change = running.script_change(ROUND_ENDED, timers);
        let entity = running.entity;
        let hazards = std::mem::take(&mut running.hazards);
        let players: Vec<Entity> = running
            .participants
            .iter()
            .map(|participant| participant.entity)
            .collect();
        self.change_game_mode_state(entity, change);
        for hazard in hazards {
            self.stations.remove_camp(hazard);
        }
        self.put_flags(&players, ROUND_OVER_FLAGS);
        tracing::info!(winner, "arena round ended");
    }

    /// Queues the participant flags `flags` of `players`.
    fn put_flags(&mut self, players: &[Entity], flags: [u8; PARTICIPANT_FLAGS_LEN]) {
        for player in players {
            self.put_participant_state(
                *player,
                GameModeParticipantState {
                    flags: Some(flags),
                    ..GameModeParticipantState::default()
                },
            );
        }
    }

    /// Runs the match: the timers that ended move the script on; a team with no participant
    /// left loses (or runs the Outpost Rush match).
    pub(super) fn tick_arena(&mut self) {
        if self.arena_matches.capture_the_flag.is_some() {
            self.tick_capture_the_flag();
            return;
        }
        if self.arena_matches.outpost_rush.is_some() {
            self.tick_outpost_rush();
            return;
        }
        let now_ms = self.server_ms;
        let Some(running) = self.arena_matches.running.as_ref() else {
            return;
        };
        let (state, ends_ms, debuff_ms) = (running.state, running.ends_ms, running.debuff_ms);
        let absent = (0..TEAMS as u8).find(|team| !running.present(*team));
        // A team that left loses; at the pregame's end a team that never came loses too
        // (Inferred).
        let pregame_over = state == PREGAME && ends_ms.is_some_and(|end| now_ms >= end);
        if state != ENDED
            && let Some(team) = absent
            && (running.both_teams_seen || pregame_over)
        {
            self.end_arena_match(1 - team);
            return;
        }
        if state == ROUND && debuff_ms.is_some_and(|at| now_ms >= at) {
            self.healing_debuff();
        }
        if ends_ms.is_none_or(|end| now_ms < end) {
            return;
        }
        match state {
            PREGAME | BETWEEN_ROUNDS => self.start_arena_round(),
            ROUND => self.start_overtime(),
            OVERTIME => self.overtime_timer_ended(),
            ROUND_ENDED => self.after_arena_round(),
            _ => self.cleanup_arena_match(),
        }
    }

    /// A round starts: state 3, every status playing, the fight flags.
    fn start_arena_round(&mut self) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        let round_end = server_ms.saturating_add(running.round_ms);
        let debuff = server_ms.saturating_add(HEALING_DEBUFF_MS);
        running.ends_ms = Some(round_end);
        running.debuff_ms = Some(debuff);
        for participant in &mut running.participants {
            participant.status = PLAYING_STATUS;
        }
        let timers = vec![
            (GAME_MODE_TIMEOUT, running.timeout_ms),
            (HEALING_DEBUFF_TIMER, debuff),
            (ROUND_TIMER, round_end),
        ];
        let mut change = running.script_change(ROUND, timers);
        let version = running.version;
        change.statuses = Some(ReplicatedList::Full(full(
            version,
            running
                .participants
                .iter()
                .map(|participant| participant.status)
                .collect(),
        )));
        let entity = running.entity;
        let players: Vec<Entity> = running
            .participants
            .iter()
            .map(|participant| participant.entity)
            .collect();
        self.change_game_mode_state(entity, change);
        self.put_flags(&players, FIGHT_FLAGS);
    }

    /// `HealingDebuffTimer` ended: the timer goes and the healing debuff slice spawns.
    fn healing_debuff(&mut self) {
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        running.debuff_ms = None;
        let timers = vec![
            (GAME_MODE_TIMEOUT, running.timeout_ms),
            (ROUND_TIMER, running.ends_ms.unwrap_or(running.timeout_ms)),
        ];
        let version = running.next_version();
        let entity = running.entity;
        self.change_game_mode_state(
            entity,
            GameModeState {
                timers: Some(full(version, timers)),
                ..GameModeState::default()
            },
        );
        self.spawn_hazards(HazardTime::HealingDebuff);
    }

    /// `RoundTimer` ended with both teams alive: the overtime (state 5) and its slices.
    fn start_overtime(&mut self) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        let overtime_end = server_ms.saturating_add(OVERTIME_MS);
        running.ends_ms = Some(overtime_end);
        running.debuff_ms = None;
        let timers = vec![
            (GAME_MODE_TIMEOUT, running.timeout_ms),
            (OVERTIME_TIMER, overtime_end),
        ];
        let change = running.script_change(OVERTIME, timers);
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
        self.spawn_hazards(HazardTime::Overtime);
    }

    /// `OvertimeTimer` ended: the timer goes, the overtime goes on until a death.
    fn overtime_timer_ended(&mut self) {
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        running.ends_ms = None;
        let timers = vec![(GAME_MODE_TIMEOUT, running.timeout_ms)];
        let version = running.next_version();
        let entity = running.entity;
        self.change_game_mode_state(
            entity,
            GameModeState {
                timers: Some(full(version, timers)),
                ..GameModeState::default()
            },
        );
    }

    /// `PostRoundTimer` ended: a team with [`ROUNDS_TO_WIN`] wins ends the match, else the
    /// players stand again at the other side for the next round.
    fn after_arena_round(&mut self) {
        let Some(running) = self.arena_matches.running.as_ref() else {
            return;
        };
        if let Some(winner) =
            (0..TEAMS as u8).find(|team| running.wins[usize::from(*team)] >= ROUNDS_TO_WIN)
        {
            self.end_arena_match(winner);
            return;
        }
        let server_ms = self.server_ms;
        let now_ms = self.last_now_ms;
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        running.round_winner = None;
        running.swapped = !running.swapped;
        running.ends_ms = Some(server_ms.saturating_add(INTER_ROUND_MS));
        let timers = vec![
            (GAME_MODE_TIMEOUT, running.timeout_ms),
            (INTER_ROUND_TIMER, server_ms.saturating_add(INTER_ROUND_MS)),
        ];
        for participant in &mut running.participants {
            participant.status = PLAYING_STATUS;
        }
        let mut change = running.script_change(BETWEEN_ROUNDS, timers);
        let version = running.version;
        change.statuses = Some(ReplicatedList::Full(full(
            version,
            running
                .participants
                .iter()
                .map(|participant| participant.status)
                .collect(),
        )));
        let entity = running.entity;
        let spots: Vec<(Entity, Option<Position>)> = running
            .participants
            .iter()
            .map(|participant| {
                let side = if running.swapped {
                    1 - participant.team.min(1)
                } else {
                    participant.team
                };
                (
                    participant.entity,
                    team_spot(&running.map, side, participant.place),
                )
            })
            .collect();
        self.change_game_mode_state(entity, change);
        for (player, spot) in spots {
            self.stand_arena_player(player);
            if let Some(spot) = spot {
                self.place_player(player, spot, None, Placement::Drop, now_ms);
            }
        }
        let players: Vec<Entity> = self
            .arena_matches
            .running
            .iter()
            .flat_map(|running| {
                running
                    .participants
                    .iter()
                    .map(|participant| participant.entity)
            })
            .collect();
        self.put_flags(&players, MATCH_FLAGS);
    }

    /// Player `entity` stands again with its whole health (a death of the round ends).
    pub(super) fn stand_arena_player(&mut self, entity: Entity) {
        let number = self.world.get::<Player>(entity).map(|player| player.number);
        if let Some(mut vitals) = self.world.get_mut::<Vitals>(entity) {
            vitals.health.refill();
            vitals.rise();
        }
        if let Some(number) = number {
            self.mobs.forget(number);
        }
    }

    /// Team `winner` won the match: state 2 with `WinningTeamIdx` and `Cleanup`, and each
    /// participant gets the end event of its team's round wins ([`arena_end_event`]).
    fn end_arena_match(&mut self, winner: u8) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        running.winner = Some(winner);
        running.round_winner = None;
        running.ends_ms = Some(server_ms.saturating_add(CLEANUP_MS));
        running.debuff_ms = None;
        for participant in &mut running.participants {
            participant.status = PLAYING_STATUS;
        }
        let timers = vec![(CLEANUP_TIMER, server_ms.saturating_add(CLEANUP_MS))];
        let mut change = running.script_change(ENDED, timers);
        let version = running.version;
        change.statuses = Some(ReplicatedList::Full(full(
            version,
            running
                .participants
                .iter()
                .map(|participant| participant.status)
                .collect(),
        )));
        let entity = running.entity;
        let hazards = std::mem::take(&mut running.hazards);
        let players: Vec<Entity> = running
            .participants
            .iter()
            .map(|participant| participant.entity)
            .collect();
        let events: Vec<(Entity, u64, &'static str)> = running
            .participants
            .iter()
            .map(|participant| {
                let wins = running.wins[usize::from(participant.team.min(1))];
                (participant.entity, participant.number, arena_end_event(wins))
            })
            .collect();
        self.change_game_mode_state(entity, change);
        for hazard in hazards {
            self.stations.remove_camp(hazard);
        }
        for player in &players {
            self.stand_arena_player(*player);
        }
        self.put_flags(&players, MATCH_FLAGS);
        for (player, number, event) in events {
            self.give_mode_event(player, number, event);
        }
        tracing::info!(winner, "arena match ended");
    }

    /// `Cleanup` ended: the players still in the match go back (Inferred), and the instance
    /// closes so the sim is free once they left.
    fn cleanup_arena_match(&mut self) {
        let players: Vec<Entity> = self
            .arena_matches
            .running
            .iter()
            .flat_map(|running| {
                running
                    .participants
                    .iter()
                    .map(|participant| participant.entity)
            })
            .collect();
        for player in players {
            self.leave_arena(player);
        }
        if let Some(running) = self.arena_matches.running.take() {
            self.stations.remove_camp(running.entity);
        }
        if let Some(instance) = self.instance_run.as_ref().map(|run| run.id()) {
            self.close_instance(instance);
        }
    }

    /// Spawns the hazard entities of the map that come at `time`.
    fn spawn_hazards(&mut self, time: HazardTime) {
        let Some(folder) = self
            .arena_matches
            .running
            .as_ref()
            .map(|running| running.map.folder.clone())
        else {
            return;
        };
        let namespace = HAZARD_NAMESPACE ^ self.me.generation;
        let mut placed = Vec::new();
        for hazard in HAZARDS
            .iter()
            .filter(|hazard| hazard.folder == folder && hazard.time == time)
        {
            let [x, y, z] = hazard.position;
            let position = Position { x, y, z };
            let number = self.stations.place_camp(|number| {
                // A slot key is far below `usize::MAX` on the 64-bit targets the server runs on.
                let gde_ref = dynamic_ref(namespace, number as usize);
                Station {
                    gde_ref,
                    position,
                    station_type: String::new(),
                    name: Vec::new(),
                    members: hazard_members(hazard, gde_ref),
                }
            });
            placed.push(number);
        }
        if let Some(running) = self.arena_matches.running.as_mut() {
            running.hazards.extend(placed);
        }
    }

    /// Player `entity` leaves the match (1980 `Leave`, 3309, or the end of `Cleanup`; a catacombs
    /// run's own leave: [`super::catacombs`]): 3455 and
    /// 5283, its participant state names no mode, the others' lists lose it, a leave of a running
    /// match bars its queue, and it goes back to where it came from.
    pub(super) fn leave_arena(&mut self, entity: Entity) {
        if self.catacomb_run().is_some() {
            self.leave_catacombs(entity);
            return;
        }
        if self.arena_matches.capture_the_flag.is_some() {
            self.leave_capture_the_flag(entity);
            return;
        }
        if self.arena_matches.outpost_rush.is_some() {
            self.leave_outpost_rush(entity);
            return;
        }
        let now_ns = unix_ns(self.server_ms);
        let Some(running) = self.arena_matches.running.as_mut() else {
            self.exit_instance(entity);
            return;
        };
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.entity == entity)
        else {
            self.exit_instance(entity);
            return;
        };
        let participant = running.participants.remove(index);
        let running_match = running.state != ENDED;
        let crc = running.crc;
        running.list_version += 1;
        let list_version = running.list_version;
        let lists = running.participant_lists();
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, lists);
        if running_match && let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player
                .arena
                .bar_queue(crc, now_ns.saturating_add(LEAVER_PENALTY_NS));
        }
        self.send_match_left(entity, &participant, list_version);
        self.stand_arena_player(entity);
        self.exit_instance(entity);
    }

    /// A player of the match leaves the sim (a logout or a lost connection): a running match bars
    /// its queue (Observed: the kicked player of `3v3_arena_group_2` got the 30 minutes), and its
    /// entry goes from the lists.
    pub(super) fn arena_left_sim(&mut self, entity: Entity) {
        if self.catacomb_run().is_some() {
            self.catacombs_left_sim(entity);
            return;
        }
        if self.arena_matches.capture_the_flag.is_some() {
            self.capture_the_flag_left_sim(entity);
            return;
        }
        if self.arena_matches.outpost_rush.is_some() {
            self.outpost_rush_left_sim(entity);
            return;
        }
        let now_ns = unix_ns(self.server_ms);
        let Some(running) = self.arena_matches.running.as_mut() else {
            return;
        };
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.entity == entity)
        else {
            return;
        };
        running.participants.remove(index);
        let crc = running.crc;
        let running_match = running.state != ENDED;
        let lists = running.participant_lists();
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, lists);
        if running_match && let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player
                .arena
                .bar_queue(crc, now_ns.saturating_add(LEAVER_PENALTY_NS));
        }
    }

    /// Sends the group push `push` to player `entity` (`number`).
    pub(super) fn send_group_push(&mut self, entity: Entity, number: u64, push: GroupPush) {
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return;
        };
        let push = SocialPush::Group(push);
        let envelope = push.encode_args().and_then(|args| {
            push_envelope(push.type_index(), SocialFacet::Groups.header(number), &args)
        });
        match envelope {
            Ok(envelope) => self.combat_messages.push(Outgoing {
                connection,
                channel: MESSAGE_CHANNEL,
                reliable: true,
                envelopes: vec![envelope],
            }),
            Err(error) => tracing::warn!(player = number, %error, "group push not sent"),
        }
    }
}

/// The game mode entity's place: midway between the two team spots.
pub(super) fn middle_of(map: &InstanceMap) -> Position {
    let spot = |index: usize| {
        map.teleports
            .get(index)
            .map_or([0.0; 3], |teleport| teleport.position)
    };
    let (first, second) = (spot(0), spot(1));
    Position {
        x: (first[0] + second[0]) / 2.0,
        y: (first[1] + second[1]) / 2.0,
        z: (first[2] + second[2]) / 2.0,
    }
}

/// The spawn members of hazard `hazard` with raw id `gde_ref`.
fn hazard_members(hazard: &Hazard, gde_ref: [u8; RAW_ID_LEN]) -> Vec<BundleMember> {
    let [x, y, z] = hazard.position;
    let position_component = match hazard.shape {
        HazardShape::Ring => RING_POSITION_COMPONENT,
        HazardShape::Debuff | HazardShape::Overtime => HAZARD_POSITION_COMPONENT,
    };
    let mut members = vec![
        BundleMember {
            component: CARD_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(AssetId {
                    id: hazard.asset,
                    sub_id: DYNAMIC_SLICE_SUB_ID,
                }),
                gde_ref: Some(gde_ref),
            }),
        },
        BundleMember {
            component: position_component,
            state: MemberState::Position(PositionInTheWorldReplicatedState {
                position: WorldPosition::from_metres(x, y, z),
                rotation: None,
                scale: None,
            }),
        },
    ];
    if hazard.shape == HazardShape::Overtime {
        members.push(BundleMember {
            component: OVERTIME_RECORDED_COMPONENT,
            state: MemberState::Recorded {
                class: OVERTIME_RECORDED_CLASS,
                body: OVERTIME_RECORDED_BODY.to_vec(),
            },
        });
        members.push(BundleMember {
            component: OVERTIME_DELAY_COMPONENT,
            state: MemberState::DelayedEvent(DelayedEventState {
                total_delay_s: Some(OVERTIME_DELAY_S),
                completed: Some(false),
            }),
        });
    }
    members
}

#[cfg(test)]
pub(super) mod tests {
    use nw_formats::objectstream::lowercase_crc32;

    use super::*;

    #[test]
    fn each_team_gets_the_end_event_of_its_round_wins() {
        assert_eq!(arena_end_event(ROUNDS_TO_WIN), "Arena3v3_Win_3_Round");
        assert_eq!(arena_end_event(2), "Arena3v3_Win_2_Round");
        assert_eq!(arena_end_event(1), "Arena3v3_Win_1_Round");
        assert_eq!(arena_end_event(0), "Arena3v3_Loss");
    }

    #[test]
    fn the_keys_are_the_crcs_of_their_names() {
        for (key, name) in [
            (GAME_MODE_TIMEOUT, "GameModeTimeout"),
            (PREGAME_TIMER, "PregameTimer"),
            (INTER_ROUND_TIMER, "InterRoundTimer"),
            (ROUND_TIMER, "RoundTimer"),
            (HEALING_DEBUFF_TIMER, "HealingDebuffTimer"),
            (POST_ROUND_TIMER, "PostRoundTimer"),
            (OVERTIME_TIMER, "OvertimeTimer"),
            (CLEANUP_TIMER, "Cleanup"),
            (WINNING_TEAM, "WinningTeamIdx"),
            (LEAVE_EVENT, "Leave"),
        ] {
            assert_eq!(key, lowercase_crc32(name), "{name}");
        }
    }

    #[test]
    fn each_map_has_its_round_time_and_its_hazards() {
        assert_eq!(round_ms_of("nw_arena01"), 120_000);
        assert_eq!(round_ms_of("nw_arena_3v3_04"), 30_000);
        assert_eq!(round_ms_of("nw_arena02"), DEFAULT_ROUND_MS);
        let overtime = HAZARDS
            .iter()
            .find(|hazard| hazard.folder == "nw_arena01" && hazard.time == HazardTime::Overtime)
            .expect("the overtime slice");
        let members = hazard_members(overtime, [7; RAW_ID_LEN]);
        let components: Vec<u64> = members.iter().map(|member| member.component).collect();
        assert_eq!(components, [0, 2, 4, 3]);
        let ring = HAZARDS
            .iter()
            .find(|hazard| hazard.shape == HazardShape::Ring)
            .expect("the ring");
        assert_eq!(hazard_members(ring, [7; RAW_ID_LEN])[1].component, 1);
    }

    use nw_bus::{InstanceOpened, InstanceStay, StoredPoint};
    use nw_content::instances::{EntryCounter, InstanceCatalog, InstanceMode, NO_INDEX, Teleport};
    use nw_messages::LevelSpec;
    use uuid::Uuid;

    use crate::world::tests::{NOW, admit_at, slot};

    /// Players of the test sim.
    const MANY_PLAYERS: usize = 10;
    /// The two players, one per team.
    const FIRST: u64 = 41;
    const SECOND: u64 = 42;
    /// The test instance and its phase.
    const INSTANCE: u128 = 7;
    const PHASE: u64 = 0x8b2c_bbb8_6ac5_c889;
    /// The server time of the opening.
    const START_MS: u64 = 1_000_000;

    /// The retail `Arena3v3` mode on `NW_Arena01` (made from its rows).
    pub(in crate::world) fn arena() -> InstanceMode {
        let map = InstanceMap {
            folder: "nw_arena01".to_owned(),
            map_name: "NW_Arena01".to_owned(),
            coatlicue_path: "coatlicue/NW_Arena01".to_owned(),
            frame: [864.0, 832.0, 992.0, 960.0],
            teleports: vec![
                Teleport {
                    position: [939.0, 935.0, 226.5],
                    yaw_deg: 180.0,
                },
                Teleport {
                    position: [939.0, 873.0, 227.0],
                    yaw_deg: 0.0,
                },
            ],
            formation: vec![[1.5, 0.0], [-1.5, 0.0]],
            map_crc: lowercase_crc32("Arena3v3"),
        };
        InstanceMode {
            id: "Arena3v3".to_owned(),
            crc: lowercase_crc32("Arena3v3"),
            kind: InstanceKind::Arena,
            map: map.clone(),
            capacity: 6,
            min_group: 0,
            required_level: Some(20),
            remote_entry: false,
            respawns: false,
            mutable: false,
            timeout_seconds: None,
            victory_delay_seconds: Some(4),
            items_removed_on_exit: vec!["ArenaConsumable".to_owned()],
            required_item_classes: Vec::new(),
            entry_achievement: None,
            loot_tags: vec!["CasualArenas".to_owned()],
            container_level: None,
            counter: EntryCounter::Uncounted,
            game_mode_index: NO_INDEX,
            generated: false,
            rotation: vec![map],
            rotation_span_seconds: Some(3600),
        }
    }

    /// The arena instance sim with the match open and two players, one per team.
    fn arena_sim() -> SimWorld {
        let mode = arena();
        let mut settings = crate::world::tests::settings(MANY_PLAYERS, MANY_PLAYERS as u32);
        settings.instance_role = true;
        settings.level = LevelSpec::instance(
            &mode.map.folder,
            &mode.map.map_name,
            &mode.map.coatlicue_path,
            mode.map.frame,
        );
        let mut world = SimWorld::new(
            settings,
            crate::login::WorldIds::random(0).expect("ids"),
            std::sync::Arc::new(nw_sim_core::test_support::NoGround),
            slot("inst-1"),
            crate::combat::tests::test_combat(),
            std::sync::Arc::new(crate::player_content::PlayerContent {
                instances: InstanceCatalog::build([mode.clone()]),
                ..crate::player_content::PlayerContent::default()
            }),
        );
        let admits: Vec<_> = [(FIRST, 0), (SECOND, 1)]
            .into_iter()
            .map(|(number, team)| {
                let mut admit = admit_at(number, 940.0, false);
                admit.progression.arena.stay = Some(InstanceStay {
                    instance: Uuid::from_u128(INSTANCE),
                    game_mode: mode.id.clone(),
                    phase: PHASE,
                    return_level: "newworld_vitaeeterna".to_owned(),
                    return_position: StoredPoint::default(),
                    activated_ns: 1,
                    mutation_difficulty: None,
                    seat: Some(ArenaSeat { team, place: 0 }),
                });
                admit
            })
            .collect();
        let members = admits.iter().map(|admit| admit.character).collect();
        assert_eq!(
            world.open_instance(
                Uuid::from_u128(INSTANCE),
                &mode.id,
                PHASE,
                members,
                None,
                NOW
            ),
            InstanceOpened::Opened
        );
        for admit in &admits {
            world.admit(admit, NOW, START_MS).expect("admit");
        }
        world
    }

    fn state(world: &SimWorld) -> Option<u8> {
        world
            .arena_matches
            .running
            .as_ref()
            .map(|running| running.state)
    }

    /// Ticks the sim at server time `server_ms`.
    fn tick_at(world: &mut SimWorld, server_ms: u64) {
        world.tick(NOW, server_ms).expect("tick");
    }

    #[test]
    fn a_match_plays_its_rounds_until_a_team_has_three_wins() {
        let mut world = arena_sim();
        let running = world.arena_matches.running.as_ref().expect("the match");
        assert_eq!(running.participants.len(), 2);
        assert_eq!(running.state, PREGAME);
        assert!(!world.arena_foes(FIRST, SECOND), "no fight in the pregame");
        world.arena_ready(FIRST, true);
        assert_eq!(
            world
                .arena_matches
                .running
                .as_ref()
                .expect("the match")
                .participants[0]
                .status,
            PLAYING_STATUS | READY_BIT
        );
        let mut now_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, now_ms);
        assert_eq!(state(&world), Some(ROUND));
        assert!(world.arena_foes(FIRST, SECOND));
        assert!(world.in_arena_round());
        tick_at(&mut world, now_ms + HEALING_DEBUFF_MS);
        assert_eq!(
            world
                .arena_matches
                .running
                .as_ref()
                .expect("the match")
                .hazards
                .len(),
            1,
            "the healing debuff slice"
        );
        for round in 1..=ROUNDS_TO_WIN {
            world.arena_death(SECOND, FIRST);
            let running = world.arena_matches.running.as_ref().expect("the match");
            assert_eq!(running.state, ROUND_ENDED);
            assert_eq!(running.wins, [round, 0]);
            assert_eq!(running.round_winner, Some(0));
            assert!(
                running.hazards.is_empty(),
                "the hazards go at the round's end"
            );
            now_ms = now_ms.max(START_MS) + HEALING_DEBUFF_MS + POST_ROUND_MS;
            tick_at(&mut world, now_ms);
            if round == ROUNDS_TO_WIN {
                break;
            }
            let running = world.arena_matches.running.as_ref().expect("the match");
            assert_eq!(running.state, BETWEEN_ROUNDS);
            assert_eq!(running.round_winner, None);
            assert_eq!(running.swapped, round % 2 == 1, "the teams change sides");
            assert!(
                running
                    .participants
                    .iter()
                    .all(|p| p.status == PLAYING_STATUS)
            );
            now_ms += INTER_ROUND_MS;
            tick_at(&mut world, now_ms);
            assert_eq!(state(&world), Some(ROUND));
        }
        let running = world.arena_matches.running.as_ref().expect("the match");
        assert_eq!((running.state, running.winner), (ENDED, Some(0)));
        assert!(running.values().contains(&(WINNING_TEAM, 0)));
        tick_at(&mut world, now_ms + CLEANUP_MS);
        assert!(world.arena_matches.running.is_none(), "the players go back");
    }

    #[test]
    fn a_round_with_no_death_goes_to_overtime() {
        let mut world = arena_sim();
        let round_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, round_ms);
        tick_at(&mut world, round_ms + DEFAULT_ROUND_MS);
        let running = world.arena_matches.running.as_ref().expect("the match");
        assert_eq!(running.state, OVERTIME);
        // The healing debuff slice of the round stays until the round ends (`solo` 526.667 to
        // 616.665: the overtime slice took another slot), so the overtime slice is the second.
        assert_eq!(
            running.hazards.len(),
            2,
            "the healing debuff and the overtime slices of NW_Arena01"
        );
        tick_at(&mut world, round_ms + DEFAULT_ROUND_MS + OVERTIME_MS);
        let running = world.arena_matches.running.as_ref().expect("the match");
        assert_eq!((running.state, running.ends_ms), (OVERTIME, None));
        assert!(world.arena_foes(FIRST, SECOND), "the fight goes on");
    }

    #[test]
    fn a_leave_of_a_running_match_bars_the_queue_and_the_other_team_wins() {
        let mut world = arena_sim();
        tick_at(&mut world, START_MS + PREGAME_MS);
        let second = world.player_by_number(SECOND).expect("player");
        world.leave_arena(second);
        let barred = world
            .world
            .get::<Player>(second)
            .expect("player")
            .arena
            .queue_eligible()
            .to_vec();
        assert_eq!(barred.len(), 1);
        assert_eq!(barred[0].0, lowercase_crc32("Arena3v3"));
        tick_at(&mut world, START_MS + PREGAME_MS + 1);
        let running = world.arena_matches.running.as_ref().expect("the match");
        assert_eq!((running.state, running.winner), (ENDED, Some(0)));
        assert_eq!(running.participants.len(), 1);
    }
}
