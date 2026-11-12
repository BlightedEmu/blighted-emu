// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Outpost Rush match on its instance sim (Outpost Rush study 2026-11-12,
//! `local-data/findings/2026-11-12-outpost-rush.md`): the game mode entity (2343), the players'
//! participant states (3312), the three outposts, the score, the kills, the respawn, the end and
//! the return.
//!
//! Observed (retail `opr_que_and_lost` on `NW_OPR_005_Vertical`, the whole match from the
//! queue; `OPR-coral-divide2` on `NW_OPR_004_Trench` and `OPR_TEST3` on the open world map,
//! both joined into a running match; times in seconds from the ledger's first record):
//!
//! - The game mode entity: a card with no asset and the game mode state: the mode `91b99a18`, the
//!   map's CRC, each participant with status 3 and its team, an empty character list, one raid id
//!   per team (two in a row), the values and the timers `GameModeTimeout` (+3 h) and
//!   `PregameTimer` (+90 s): script state 2 (`opr_que_and_lost` 578.634).
//! - At `PregameTimer` the script state turns to 0 (665.446) with `TimeLimit` 2,700 s on (3 of 3
//!   ledgers); 3 s later `TeamScores` 0 and the value `e78c48f6` 1 (668.478).
//! - The outposts are the values `CapturePointA_Status`, `CapturePointB_Status` and
//!   `CapturePointC_Status` (the names of the client slice `GameMode_OutpostRush_005_Vertical`;
//!   their CRC-32 are the keys): the owner team in the low byte, the capturing team in the
//!   second, the progress in the third, `ff` for no team; `ffff` at the start. The progress moves
//!   about 2 a second (683.712 to 733.667: 0 to 98), falls when the other team pushes it back,
//!   and at its end the capturing team owns the outpost with no capturer and progress 0 (734.177);
//!   an owned outpost goes straight to the other team (`06a8de5f` 1108.472 to 1151.305).
//! - `TeamScores` holds team 1's score in the high half and team 0's in the low half. Every 3 s
//!   each team gains one point per outpost it owns (734.879 to 751.113: 1 a tick with one
//!   outpost, 2 with two; 1109.403 to 1148.538: 3 with all three); a kill gives the killer's
//!   team 1 point (the foe team's score rose by 1 within 60 ms of 3 of 3 own deaths).
//! - While one team owns all three outposts the value `e78c48f6` rises: 2 after 60 s, then 1 more
//!   every 30 s (`OPR-coral-divide2` 249.857 to 309.856, 340.036, 370.203, ...; and 532.366 to
//!   593.549), and a tick gives that team its outposts times the value (309.856: 6 a tick); it is
//!   1 again when the team loses an outpost (451.736).
//! - The first team with 1,000 points wins (`opr_que_and_lost` 2105.783: team 1 at `3e8`,
//!   `OPR_TEST3` 1851.453): script state 1, `WinningTeamIdx`, `Cleanup` 180 s on.
//! - Statuses: 3 alive, `0x20` set by a ready-up (6824) in the pregame and kept (636.312), `0x04`
//!   while dead (`07`, `27`).
//! - The participant flags are `13ca61080000` for the whole match (3 of 3 ledgers); the player
//!   gets no temporary affiliation in the match.
//! - The client's 1980 `Leave` is answered as in the arena: 3455 (`0`), 5283 (own character),
//!   3951 and the move back (`OPR-coral-divide2` 674.171, `OPR_TEST3` 1862.570).
//! - A dead player asks for its respawn (1938) 8 to 30 s after its death, at one respawn point
//!   per match (the same point id in every request of a ledger).
//!
//! Inferred: the capture rule (each second the progress moves 2 toward the team with more living
//! players in the outpost; an equal count holds it; an empty outpost falls back 2 a second; the
//! end at 100); the outpost places on a map (the slice's places from the map's middle, 1024,
//! 1024: Observed exactly for the B outposts of both maps); the capture area (the slice's
//! `radius` 21.35 m around the outpost, its `height` above and below); PvP between the two teams
//! while the match runs; the respawn at the team's spot as soon as the client asks; at
//! `TimeLimit` the team with more points wins (no capture ran that long), and a tie has no
//! winner; a team with no participant left loses; the players still in the match at the end of
//! `Cleanup` go back; a leave of a running match bars the queue as the arena's does.
//!
//! Not built (no capture shows them in full, or they need entities this sim does not make): the
//! capture point entities (333), the gates and structures (2270, 2646), the turrets (4276),
//! resources and the armory, the summons, the boss (`BossSpawnTimer`, `BossKilled`,
//! `BossFreezeTimer`, `FrozenStatus`) and its score freeze, the event feed (`Event1` to `Event3`),
//! the end statistics, the score board (839) and the rewards, and a backfill into a running
//! match.

use bevy_ecs::entity::Entity;
use nw_bus::ArenaSeat;
use nw_content::instances::{InstanceKind, InstanceMap};
use nw_messages::game_mode_participant::PARTICIPANT_FLAGS_LEN;
use nw_messages::game_mode_state::{GameModeState, Keyed};
use nw_messages::list_delta::{ListChange, ReplicatedList};
use nw_messages::wire::RAW_ID_LEN;
use nw_sim_core::Position;

use super::arena_match::{
    CLEANUP_TIMER, GAME_MODE_TIMEOUT, GAME_MODE_TIMEOUT_MS, LEAVER_PENALTY_NS, PREGAME_MS,
    PREGAME_TIMER, RAID_ID_BASE, READY_BIT, WINNING_TEAM, middle_of,
};
use super::game_mode_entity::{
    FIRST_LIST_VERSION, MatchEntry, MatchParticipant, PLAYING_STATUS, full, participant_lists,
};
use super::queue::team_spot;
use super::{Body, Player, SimWorld, unix_ns};

/// The script states (Observed).
const RUNNING: u8 = 0;
const ENDED: u8 = 1;
const PREGAME: u8 = 2;
/// `TimeLimit`: the match's end, from the start of state 0 (Observed: 2,700 s in 3 of 3).
const TIME_LIMIT_TIMER: u32 = 0x8f0e_9156;
const TIME_LIMIT_MS: u64 = 2_700_000;
/// `Cleanup` after the end (Observed: 180 s, `opr_que_and_lost` 2105.783 s).
const CLEANUP_MS: u64 = 180_000;
/// The value keys: `TeamScores`, the three outposts (CRC-32 of the lowercase names), and the
/// score multiplier of the team that owns every outpost (name Unknown; meaning Inferred from the
/// values).
const TEAM_SCORES: u32 = 0x4a6e_9282;
const OUTPOST_A_STATUS: u32 = 0x448d_d922;
const OUTPOST_B_STATUS: u32 = 0xca02_dec1;
const OUTPOST_C_STATUS: u32 = 0x06a8_de5f;
const DOMINATION_MULTIPLIER: u32 = 0xe78c_48f6;
/// The value of no team in an outpost status (Observed `ff`).
const NO_TEAM: u8 = 0xff;
/// The bits of a team's score in `TeamScores` (Observed: team 1 high, team 0 low).
const SCORE_BITS: u32 = 16;
/// The shifts of an outpost status: the capturing team, the progress (Observed).
const CAPTURER_SHIFT: u32 = 8;
const PROGRESS_SHIFT: u32 = 16;
/// Points that win the match (Observed: `3e8` at the end of 2 matches).
const SCORE_TO_WIN: u32 = 1_000;
/// The score tick, and the points an owned outpost gives a tick (Observed).
const SCORE_TICK_MS: u64 = 3_000;
const OUTPOST_POINTS: u32 = 1;
/// The points of a kill for the killer's team (Observed).
const KILL_POINTS: u32 = 1;
/// The multiplier with no team owning every outpost, its first rise after the third outpost and
/// each later rise (Observed: 2 runs of `OPR-coral-divide2`).
const BASE_MULTIPLIER: u32 = 1;
const DOMINATION_FIRST_STEP_MS: u64 = 60_000;
const DOMINATION_STEP_MS: u64 = 30_000;
/// The capture tick and the progress step of a tick (Observed: about one change a second, steps
/// of about 2), and the progress that ends a capture (Inferred: the last retail value was 98).
const CAPTURE_TICK_MS: u64 = 1_000;
const CAPTURE_STEP: u8 = 2;
const CAPTURE_DONE: u8 = 100;
/// The dead bit of a participant status (Observed `07` and `27`).
const DEAD_BIT: u8 = 0x04;
/// The participant flags of the whole match (Observed).
const MATCH_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x13, 0xca, 0x61, 0x08, 0x00, 0x00];
/// The teams of a match.
const TEAMS: usize = 2;
/// The namespace of the match entity's `GdeRef` ("oprmatch").
const MATCH_NAMESPACE: u64 = 0x6f70_726d_6174_6368;
/// The capture radius of an outpost (Observed: the field `radius` 21.35 of every outpost of the
/// client slices of both maps).
const CAPTURE_RADIUS_M: f32 = 21.35;

/// One outpost of a map: its status key, its place and the `height` of its capture area.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Outpost {
    key: u32,
    position: [f32; 3],
    height_m: f32,
}

/// The outposts of one map (Observed: the entities `OR_Outpost_A_LUNA`, `OR_Outpost_B_SOL` and
/// `OR_Outpost_C_STAR` of the client slices `GameMode_OutpostRush_005_Vertical` and
/// `OutpostRush_Layout_004_Trench`, their slice places plus the map's middle 1024, 1024; the
/// retail capture point of B stood at that place in both maps).
#[derive(Debug, Clone, Copy, PartialEq)]
struct OutpostMap {
    folder: &'static str,
    outposts: [Outpost; 3],
}

const OUTPOST_MAPS: [OutpostMap; 2] = [
    OutpostMap {
        folder: "nw_opr_004_trench",
        outposts: [
            Outpost {
                key: OUTPOST_A_STATUS,
                position: [981.46, 887.39, 13.5],
                height_m: 18.0,
            },
            Outpost {
                key: OUTPOST_B_STATUS,
                position: [1042.11, 1026.18, 16.0],
                height_m: 16.5,
            },
            Outpost {
                key: OUTPOST_C_STATUS,
                position: [1101.98, 887.4, 13.61],
                height_m: 16.0,
            },
        ],
    },
    OutpostMap {
        folder: "nw_opr_005_vertical",
        outposts: [
            Outpost {
                key: OUTPOST_A_STATUS,
                position: [962.38, 1077.42, 107.0],
                height_m: 18.0,
            },
            Outpost {
                key: OUTPOST_B_STATUS,
                position: [1023.75, 1024.1, 119.6],
                height_m: 16.5,
            },
            Outpost {
                key: OUTPOST_C_STATUS,
                position: [1086.71, 971.59, 107.0],
                height_m: 16.0,
            },
        ],
    },
];

/// The state of one outpost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OutpostState {
    owner: u8,
    capturer: u8,
    progress: u8,
}

impl OutpostState {
    const NEUTRAL: Self = Self {
        owner: NO_TEAM,
        capturer: NO_TEAM,
        progress: 0,
    };

    /// The status value of the outpost.
    fn value(self) -> u64 {
        u64::from(self.owner)
            | u64::from(self.capturer) << CAPTURER_SHIFT
            | u64::from(self.progress) << PROGRESS_SHIFT
    }

    /// One capture tick with `present` living players of each team in the outpost.
    fn step(self, present: [usize; TEAMS]) -> Self {
        let pusher = match present[0].cmp(&present[1]) {
            std::cmp::Ordering::Greater => Some(0),
            std::cmp::Ordering::Less => Some(1),
            std::cmp::Ordering::Equal => None,
        };
        let back = |state: Self, idle_capturer: u8| {
            let progress = state.progress.saturating_sub(CAPTURE_STEP);
            Self {
                progress,
                capturer: if progress == 0 {
                    idle_capturer
                } else {
                    state.capturer
                },
                ..state
            }
        };
        match pusher {
            // An equal count of players holds the progress.
            None if present[0] > 0 => self,
            None => back(self, NO_TEAM),
            Some(team) if team == self.owner => back(self, NO_TEAM),
            Some(team) if self.capturer == team || self.capturer == NO_TEAM => {
                let progress = self.progress.saturating_add(CAPTURE_STEP);
                if progress >= CAPTURE_DONE {
                    Self {
                        owner: team,
                        ..Self::NEUTRAL
                    }
                } else {
                    Self {
                        capturer: team,
                        progress,
                        ..self
                    }
                }
            }
            Some(team) => back(self, team),
        }
    }
}

/// `TeamScores` of `scores`.
fn team_scores(scores: [u32; TEAMS]) -> u64 {
    u64::from(scores[1]) << SCORE_BITS | u64::from(scores[0])
}

/// The multiplier of a team that owns every outpost since `held_ms` ms.
const fn domination_multiplier(held_ms: u64) -> u32 {
    if held_ms < DOMINATION_FIRST_STEP_MS {
        return BASE_MULTIPLIER;
    }
    let steps = (held_ms - DOMINATION_FIRST_STEP_MS) / DOMINATION_STEP_MS;
    // A match lasts at most `TIME_LIMIT_MS`: the step count is far below `u32::MAX`.
    BASE_MULTIPLIER + 1 + steps as u32
}

/// The running Outpost Rush match of this instance sim.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct OutpostRushMatch {
    /// The slot key of the game mode entity, and its raw id.
    entity: u64,
    gde_ref: [u8; RAW_ID_LEN],
    crc: u32,
    map: InstanceMap,
    outposts: [Outpost; 3],
    participants: Vec<MatchParticipant>,
    state: u8,
    /// The server ms of the current state's timer: `PregameTimer`, `TimeLimit` or `Cleanup`.
    ends_ms: u64,
    timeout_ms: u64,
    points: [OutpostState; 3],
    scores: [u32; TEAMS],
    /// The team that owns every outpost and since when (server ms).
    dominating: Option<(u8, u64)>,
    multiplier: u32,
    next_score_ms: u64,
    next_capture_ms: u64,
    raids: [u64; TEAMS],
    winner: Option<u8>,
    version: u64,
    list_version: u64,
    both_teams_seen: bool,
}

impl OutpostRushMatch {
    fn next_version(&mut self) -> u64 {
        self.version += 1;
        self.version
    }

    fn of(&self, number: u64) -> Option<&MatchParticipant> {
        self.participants
            .iter()
            .find(|participant| participant.number == number)
    }

    fn present(&self, team: u8) -> bool {
        self.participants
            .iter()
            .any(|participant| participant.team == team)
    }

    /// The values: the outposts, and from the start the scores and the multiplier, and
    /// `WinningTeamIdx` once a team won.
    fn values(&self) -> Vec<Keyed> {
        let mut values: Vec<Keyed> = self
            .outposts
            .iter()
            .zip(self.points)
            .map(|(outpost, point)| (outpost.key, point.value()))
            .collect();
        if self.state != PREGAME {
            values.push((TEAM_SCORES, team_scores(self.scores)));
            values.push((DOMINATION_MULTIPLIER, u64::from(self.multiplier)));
        }
        if let Some(winner) = self.winner {
            values.push((WINNING_TEAM, u64::from(winner)));
        }
        values
    }

    /// The timers of the current state.
    fn timers(&self) -> Vec<Keyed> {
        let own = match self.state {
            PREGAME => PREGAME_TIMER,
            RUNNING => TIME_LIMIT_TIMER,
            _ => CLEANUP_TIMER,
        };
        vec![(own, self.ends_ms), (GAME_MODE_TIMEOUT, self.timeout_ms)]
    }

    /// The change of the values at a new version.
    fn values_change(&mut self) -> GameModeState {
        let version = self.next_version();
        GameModeState {
            values: Some(full(version, self.values())),
            ..GameModeState::default()
        }
    }

    /// The change to script state `state` whose timer ends at `ends_ms`.
    fn script_change(&mut self, state: u8, ends_ms: u64) -> GameModeState {
        self.state = state;
        self.ends_ms = ends_ms;
        let version = self.next_version();
        GameModeState {
            script_state: Some(state),
            values: Some(full(version, self.values())),
            timers: Some(full(version, self.timers())),
            ..GameModeState::default()
        }
    }

    /// The participant lists at a new version (no character ids).
    fn participant_lists(&mut self) -> GameModeState {
        let version = self.next_version();
        participant_lists(&self.participants, version, false)
    }

    /// The change of participant `index`'s status to `status`.
    fn status_change(&mut self, index: usize, status: u8) -> GameModeState {
        self.participants[index].status = status;
        let version = self.next_version();
        GameModeState {
            statuses: Some(ReplicatedList::Changes(vec![ListChange {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                version,
                value: status,
            }])),
            ..GameModeState::default()
        }
    }

    /// Team `team` scores `points`; `true` when it reached the win.
    fn score(&mut self, team: u8, points: u32) -> bool {
        let score = &mut self.scores[usize::from(team.min(1))];
        *score = score.saturating_add(points);
        *score >= SCORE_TO_WIN
    }

    /// The team that owns every outpost.
    fn owner_of_all(&self) -> Option<u8> {
        let owner = self.points[0].owner;
        (owner != NO_TEAM && self.points.iter().all(|point| point.owner == owner)).then_some(owner)
    }

    /// The team with more points; `None` on a tie.
    fn leader(&self) -> Option<u8> {
        match self.scores[0].cmp(&self.scores[1]) {
            std::cmp::Ordering::Greater => Some(0),
            std::cmp::Ordering::Less => Some(1),
            std::cmp::Ordering::Equal => None,
        }
    }
}

impl SimWorld {
    /// Whether this instance sim's game mode is Outpost Rush.
    pub(super) fn runs_outpost_rush(&self) -> bool {
        self.instance_run
            .as_ref()
            .and_then(|run| self.content.instances.by_id(run.game_mode()))
            .is_some_and(|mode| mode.kind == InstanceKind::OutpostRush)
    }

    /// Player `entity` arrived with its seat `seat`: it joins the match (the first one opens it)
    /// with its participant state.
    pub(super) fn join_outpost_rush(&mut self, entity: Entity, seat: ArenaSeat) {
        let Some((number, character, eligible)) = self.world.get::<Player>(entity).map(|player| {
            (
                player.number,
                *player.character.as_bytes(),
                player.arena.queue_eligible().to_vec(),
            )
        }) else {
            return;
        };
        if self.arena_matches.outpost_rush.is_none() && !self.open_outpost_rush() {
            return;
        }
        let server_ms = self.server_ms;
        let phase = self.instance_run.as_ref().map_or(0, |run| run.phase());
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        if running.of(number).is_some() || running.state == ENDED {
            return;
        }
        running.participants.push(MatchParticipant {
            number,
            entity,
            character,
            team: seat.team,
            place: seat.place,
            status: PLAYING_STATUS,
        });
        running.both_teams_seen |= (0..TEAMS as u8).all(|team| running.present(team));
        let lists = running.participant_lists();
        running.list_version += 1;
        let entry = MatchEntry {
            gde_ref: running.gde_ref,
            mode: running.crc,
            map: running.map.map_crc,
            phase,
            flags: MATCH_FLAGS,
            team: seat.team,
            list_version: running.list_version,
            eligible,
        };
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, lists);
        self.put_match_joined(entity, entry, server_ms);
        tracing::info!(
            player = number,
            team = seat.team,
            "player joined the Outpost Rush match"
        );
    }

    /// Opens the match of this sim's Outpost Rush instance: its game mode entity in the
    /// pregame. `false` when this sim runs no Outpost Rush or its map has no outposts.
    fn open_outpost_rush(&mut self) -> bool {
        let content = std::sync::Arc::clone(&self.content);
        let Some(mode) = self
            .instance_run
            .as_ref()
            .and_then(|run| content.instances.by_id(run.game_mode()))
            .filter(|mode| mode.kind == InstanceKind::OutpostRush)
        else {
            return false;
        };
        let folder = self.settings.level.folder;
        let (Some(map), Some(outposts)) = (
            mode.map_of(folder).cloned(),
            OUTPOST_MAPS.iter().find(|map| map.folder == folder),
        ) else {
            tracing::warn!(level = folder, "Outpost Rush map without outposts");
            return false;
        };
        let server_ms = self.server_ms;
        let started = self.arena_matches.started;
        self.arena_matches.started += 1;
        let raid = RAID_ID_BASE + started * TEAMS as u64;
        let pregame_end = server_ms.saturating_add(PREGAME_MS);
        let mut running = OutpostRushMatch {
            entity: 0,
            gde_ref: [0; RAW_ID_LEN],
            crc: mode.crc,
            map,
            outposts: outposts.outposts,
            participants: Vec::new(),
            state: PREGAME,
            ends_ms: pregame_end,
            timeout_ms: server_ms.saturating_add(GAME_MODE_TIMEOUT_MS),
            points: [OutpostState::NEUTRAL; 3],
            scores: [0; TEAMS],
            dominating: None,
            multiplier: BASE_MULTIPLIER,
            next_score_ms: pregame_end.saturating_add(SCORE_TICK_MS),
            next_capture_ms: pregame_end.saturating_add(CAPTURE_TICK_MS),
            raids: [raid, raid + 1],
            winner: None,
            version: FIRST_LIST_VERSION,
            list_version: FIRST_LIST_VERSION,
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
            timers: Some(full(version, running.timers())),
            no_affliction: true,
            linked_mode: Some(0),
        };
        let middle = middle_of(&running.map);
        let (entity, gde_ref) = self.place_game_mode_entity(MATCH_NAMESPACE, None, middle, state);
        running.entity = entity;
        running.gde_ref = gde_ref;
        tracing::info!(level = %running.map.folder, "Outpost Rush match opened");
        self.arena_matches.outpost_rush = Some(running);
        true
    }

    /// 6824: player `number` readied up (or not) in the pregame.
    pub(super) fn outpost_rush_ready(&mut self, number: u64, ready: bool) {
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.number == number)
        else {
            return;
        };
        if running.state != PREGAME {
            return;
        }
        let status = running.participants[index].status;
        let status = if ready {
            status | READY_BIT
        } else {
            status & !READY_BIT
        };
        let change = running.status_change(index, status);
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
    }

    /// Player `target` died to `attacker`: its status is dead and, in the running match, a
    /// participant of the other team scores the kill for its team.
    pub(super) fn outpost_rush_death(&mut self, target: u64, attacker: u64) {
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.number == target)
        else {
            return;
        };
        let dead = running.participants[index].clone();
        let mut change = running.status_change(index, dead.status | DEAD_BIT);
        let killer = running
            .of(attacker)
            .map(|killer| killer.team)
            .filter(|team| *team != dead.team && running.state == RUNNING);
        let won = killer.map(|team| (team, running.score(team, KILL_POINTS)));
        if won.is_some() {
            change.values = running.values_change().values;
        }
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
        if let Some((team, true)) = won {
            self.end_outpost_rush(Some(team));
        }
    }

    /// Where dead player `entity` of the match respawns: its team's spot, with its status alive
    /// again. `None` when it plays no Outpost Rush match here.
    pub(super) fn outpost_rush_respawn(&mut self, entity: Entity) -> Option<Position> {
        let running = self.arena_matches.outpost_rush.as_mut()?;
        let index = running
            .participants
            .iter()
            .position(|participant| participant.entity == entity)?;
        let participant = running.participants[index].clone();
        let spot = team_spot(&running.map, participant.team, participant.place)?;
        let change = running.status_change(index, participant.status & !DEAD_BIT);
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, change);
        Some(spot)
    }

    /// Whether players `first` and `second` are foes in the match: participants of two teams
    /// while it runs. `None` when this sim runs no Outpost Rush match.
    pub(super) fn outpost_rush_foes(&self, first: Entity, second: Entity) -> Option<bool> {
        let running = self.arena_matches.outpost_rush.as_ref()?;
        let team = |entity: Entity| {
            running
                .participants
                .iter()
                .find(|participant| participant.entity == entity)
                .map(|participant| participant.team)
        };
        Some(
            running.state == RUNNING
                && team(first)
                    .zip(team(second))
                    .is_some_and(|(first, second)| first != second),
        )
    }

    /// Runs the match: the pregame's end, the captures, the scores, the time limit, a team with
    /// no participant left, and the end of `Cleanup`.
    pub(super) fn tick_outpost_rush(&mut self) {
        let now_ms = self.server_ms;
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return;
        };
        let (state, ends_ms) = (running.state, running.ends_ms);
        let absent = (0..TEAMS as u8).find(|team| !running.present(*team));
        if state != ENDED
            && let Some(team) = absent
            && (running.both_teams_seen || (state == PREGAME && now_ms >= ends_ms))
        {
            self.end_outpost_rush(Some(1 - team));
            return;
        }
        match state {
            PREGAME if now_ms >= ends_ms => self.start_outpost_rush(),
            RUNNING => self.run_outpost_rush(now_ms),
            ENDED if now_ms >= ends_ms => self.cleanup_outpost_rush(),
            _ => {}
        }
    }

    /// `PregameTimer` ended: state 0 with `TimeLimit`.
    fn start_outpost_rush(&mut self) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        running.next_score_ms = server_ms.saturating_add(SCORE_TICK_MS);
        running.next_capture_ms = server_ms.saturating_add(CAPTURE_TICK_MS);
        let change = running.script_change(RUNNING, server_ms.saturating_add(TIME_LIMIT_MS));
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
        tracing::info!("Outpost Rush match started");
    }

    /// The running match at server ms `now_ms`: the capture ticks and the score ticks that are
    /// due, then the time limit.
    fn run_outpost_rush(&mut self, now_ms: u64) {
        let Some(running) = self.arena_matches.outpost_rush.as_ref() else {
            return;
        };
        let outposts = running.outposts;
        let players: Vec<(Entity, u8)> = running
            .participants
            .iter()
            .filter(|participant| participant.status & DEAD_BIT == 0)
            .map(|participant| (participant.entity, participant.team))
            .collect();
        let places: Vec<(Position, u8)> = players
            .iter()
            .filter_map(|(entity, team)| {
                self.world
                    .get::<Body>(*entity)
                    .map(|body| (body.motion.position(), *team))
            })
            .collect();
        let present: Vec<[usize; TEAMS]> = outposts
            .iter()
            .map(|outpost| {
                let mut count = [0; TEAMS];
                for (at, team) in &places {
                    let [x, y, z] = outpost.position;
                    let (east, north) = (at.x - x, at.y - y);
                    if (east * east + north * north).sqrt() <= CAPTURE_RADIUS_M
                        && (at.z - z).abs() <= outpost.height_m
                    {
                        count[usize::from((*team).min(1))] += 1;
                    }
                }
                count
            })
            .collect();
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        let mut changed = false;
        let mut won = None;
        while now_ms >= running.next_capture_ms {
            running.next_capture_ms = running.next_capture_ms.saturating_add(CAPTURE_TICK_MS);
            for (point, present) in running.points.iter_mut().zip(&present) {
                let next = point.step(*present);
                changed |= next != *point;
                *point = next;
            }
        }
        let dominating = running.owner_of_all();
        if dominating != running.dominating.map(|(team, _)| team) {
            running.dominating = dominating.map(|team| (team, now_ms));
        }
        let multiplier = running.dominating.map_or(BASE_MULTIPLIER, |(_, since)| {
            domination_multiplier(now_ms.saturating_sub(since))
        });
        changed |= multiplier != running.multiplier;
        running.multiplier = multiplier;
        while now_ms >= running.next_score_ms && won.is_none() {
            running.next_score_ms = running.next_score_ms.saturating_add(SCORE_TICK_MS);
            for team in 0..TEAMS as u8 {
                let owned = running
                    .points
                    .iter()
                    .filter(|point| point.owner == team)
                    .count();
                let factor = if running.dominating.is_some_and(|(owner, _)| owner == team) {
                    running.multiplier
                } else {
                    BASE_MULTIPLIER
                };
                let points = u32::try_from(owned).unwrap_or(u32::MAX) * OUTPOST_POINTS * factor;
                if points > 0 {
                    changed = true;
                    if running.score(team, points) {
                        won = Some(team);
                    }
                }
            }
        }
        let timed_out = now_ms >= running.ends_ms;
        let leader = running.leader();
        let change = changed.then(|| running.values_change());
        let entity = running.entity;
        if let Some(change) = change {
            self.change_game_mode_state(entity, change);
        }
        if let Some(team) = won {
            self.end_outpost_rush(Some(team));
        } else if timed_out {
            self.end_outpost_rush(leader);
        }
    }

    /// The match ended with `winner` (`None`: a tie at the time limit): state 1 with
    /// `WinningTeamIdx` and `Cleanup`.
    fn end_outpost_rush(&mut self, winner: Option<u8>) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return;
        };
        running.winner = winner;
        let change = running.script_change(ENDED, server_ms.saturating_add(CLEANUP_MS));
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
        tracing::info!(?winner, "Outpost Rush match ended");
    }

    /// `Cleanup` ended: the players still in the match go back, and the instance closes.
    fn cleanup_outpost_rush(&mut self) {
        let players: Vec<Entity> = self
            .arena_matches
            .outpost_rush
            .iter()
            .flat_map(|running| {
                running
                    .participants
                    .iter()
                    .map(|participant| participant.entity)
            })
            .collect();
        for player in players {
            self.leave_outpost_rush(player);
        }
        if let Some(running) = self.arena_matches.outpost_rush.take() {
            self.stations.remove_camp(running.entity);
        }
        if let Some(instance) = self.instance_run.as_ref().map(|run| run.id()) {
            self.close_instance(instance);
        }
    }

    /// Takes player `entity` out of the match's lists; a leave of a running match bars its
    /// queue. The participant and the list version, when it played.
    fn drop_outpost_rush_participant(&mut self, entity: Entity) -> Option<(MatchParticipant, u64)> {
        let now_ns = unix_ns(self.server_ms);
        let running = self.arena_matches.outpost_rush.as_mut()?;
        let index = running
            .participants
            .iter()
            .position(|participant| participant.entity == entity)?;
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
        Some((participant, list_version))
    }

    /// Player `entity` leaves the match (1980 `Leave`, 3309, or the end of `Cleanup`): what a
    /// player who leaves gets, and it goes back to where it came from.
    pub(super) fn leave_outpost_rush(&mut self, entity: Entity) {
        if let Some((participant, list_version)) = self.drop_outpost_rush_participant(entity) {
            self.send_match_left(entity, &participant, list_version);
            self.stand_arena_player(entity);
        }
        self.exit_instance(entity);
    }

    /// A player of the match leaves the sim (a logout or a lost connection).
    pub(super) fn outpost_rush_left_sim(&mut self, entity: Entity) {
        self.drop_outpost_rush_participant(entity);
    }
}

#[cfg(test)]
pub(super) mod tests {
    use nw_bus::{InstanceOpened, InstanceStay, StoredPoint};
    use nw_content::instances::{
        EntryCounter, InstanceCatalog, InstanceMode, NO_INDEX, OUTPOST_RUSH_GAME_MODE, Teleport,
    };
    use nw_formats::objectstream::lowercase_crc32;
    use nw_messages::LevelSpec;
    use uuid::Uuid;

    use super::*;
    use crate::world::Placement;
    use crate::world::tests::{NOW, admit_at, slot};

    /// Players of the test sim.
    const MANY_PLAYERS: usize = 10;
    /// The two players, one per team.
    const FIRST: u64 = 41;
    const SECOND: u64 = 42;
    /// The test instance and its phase.
    const INSTANCE: u128 = 8;
    const PHASE: u64 = 0x486f_26c0_6aa9_8117;
    /// The server time of the opening.
    const START_MS: u64 = 1_000_000;

    #[test]
    fn the_keys_are_the_crcs_of_their_names() {
        for (key, name) in [
            (TIME_LIMIT_TIMER, "TimeLimit"),
            (TEAM_SCORES, "TeamScores"),
            (OUTPOST_A_STATUS, "CapturePointA_Status"),
            (OUTPOST_B_STATUS, "CapturePointB_Status"),
            (OUTPOST_C_STATUS, "CapturePointC_Status"),
        ] {
            assert_eq!(key, lowercase_crc32(name), "{name}");
        }
        assert_eq!(lowercase_crc32(OUTPOST_RUSH_GAME_MODE), 0x91b9_9a18);
    }

    #[test]
    fn the_values_pack_as_retail() {
        // `opr_que_and_lost`: 683.712 `0000ff`, 733.667 `6200ff`, 734.177 `00ff00`.
        let capturing = OutpostState {
            owner: NO_TEAM,
            capturer: 0,
            progress: 0x62,
        };
        assert_eq!(OutpostState::NEUTRAL.value(), 0xffff);
        assert_eq!(capturing.value(), 0x62_00ff);
        let owned = OutpostState {
            owner: 0,
            ..OutpostState::NEUTRAL
        };
        assert_eq!(owned.value(), 0xff00);
        // 2105.783: team 1 at 1,000 and team 0 at 0x217.
        assert_eq!(team_scores([0x217, 1_000]), 0x03e8_0217);
    }

    #[test]
    fn a_capture_rises_toward_the_bigger_team_and_ends_at_one_hundred() {
        let mut point = OutpostState::NEUTRAL;
        point = point.step([1, 0]);
        assert_eq!((point.capturer, point.progress), (0, CAPTURE_STEP));
        assert_eq!(point.step([1, 1]), point, "an equal count holds it");
        assert_eq!(
            point.step([0, 0]),
            OutpostState::NEUTRAL,
            "an empty outpost falls back"
        );
        let pushed = point.step([0, 2]);
        assert_eq!(
            (pushed.capturer, pushed.progress),
            (1, 0),
            "the other team pushes it back"
        );
        for _ in 0..CAPTURE_DONE / CAPTURE_STEP {
            point = point.step([3, 1]);
        }
        assert_eq!(
            point,
            OutpostState {
                owner: 0,
                ..OutpostState::NEUTRAL
            }
        );
        assert_eq!(point.step([2, 0]), point, "the owner holds its outpost");
        let taken = (0..CAPTURE_DONE / CAPTURE_STEP).fold(point, |point, _| point.step([0, 1]));
        assert_eq!(
            taken.owner, 1,
            "an owned outpost goes straight to the other team"
        );
    }

    #[test]
    fn the_multiplier_rises_after_a_minute_then_every_thirty_seconds() {
        assert_eq!(domination_multiplier(59_999), 1);
        assert_eq!(domination_multiplier(60_000), 2);
        assert_eq!(domination_multiplier(89_999), 2);
        assert_eq!(domination_multiplier(90_000), 3);
        assert_eq!(
            domination_multiplier(180_000),
            6,
            "OPR-coral-divide2 430.568 s"
        );
    }

    /// The retail `OutpostRush` mode on `NW_OPR_004_Trench` (made from its rows).
    pub(in crate::world) fn outpost_rush() -> InstanceMode {
        let map = InstanceMap {
            folder: "nw_opr_004_trench".to_owned(),
            map_name: "NW_OPR_004_Trench".to_owned(),
            coatlicue_path: "coatlicue/NW_OPR_004_Trench".to_owned(),
            frame: [832.0, 768.0, 512.0, 512.0],
            teleports: vec![
                Teleport {
                    position: [931.2498, 983.3751, 49.75],
                    yaw_deg: 270.0,
                },
                Teleport {
                    position: [1170.75, 987.75, 49.3],
                    yaw_deg: 90.0,
                },
            ],
            formation: Vec::new(),
            map_crc: lowercase_crc32("OutpostRush_004_Trench"),
        };
        InstanceMode {
            id: OUTPOST_RUSH_GAME_MODE.to_owned(),
            crc: lowercase_crc32(OUTPOST_RUSH_GAME_MODE),
            kind: InstanceKind::OutpostRush,
            map: map.clone(),
            capacity: 40,
            min_group: 0,
            required_level: Some(60),
            remote_entry: false,
            respawns: false,
            mutable: false,
            timeout_seconds: None,
            victory_delay_seconds: None,
            items_removed_on_exit: vec!["OutpostRushOnly".to_owned()],
            required_item_classes: Vec::new(),
            entry_achievement: None,
            loot_tags: vec!["OutpostRush".to_owned()],
            container_level: None,
            counter: EntryCounter::Uncounted,
            game_mode_index: NO_INDEX,
            generated: false,
            rotation: vec![map],
            rotation_span_seconds: Some(3600),
        }
    }

    /// The Outpost Rush instance sim with the match open and two players, one per team.
    fn rush_sim() -> SimWorld {
        let mode = outpost_rush();
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

    fn running(world: &SimWorld) -> &OutpostRushMatch {
        world
            .arena_matches
            .outpost_rush
            .as_ref()
            .expect("the match")
    }

    fn tick_at(world: &mut SimWorld, server_ms: u64) {
        world.tick(NOW, server_ms).expect("tick");
    }

    /// Puts player `number` at outpost `index` of the Trench map.
    fn stand_in_outpost(world: &mut SimWorld, number: u64, index: usize) {
        let [x, y, z] = OUTPOST_MAPS[0].outposts[index].position;
        let entity = world.player_by_number(number).expect("player");
        world.place_player(entity, Position { x, y, z }, None, Placement::Drop, NOW);
    }

    #[test]
    fn a_held_outpost_scores_until_the_owner_wins() {
        let mut world = rush_sim();
        let first = world.player_by_number(FIRST).expect("player");
        let second = world.player_by_number(SECOND).expect("player");
        let match_state = running(&world);
        assert_eq!(
            (match_state.state, match_state.participants.len()),
            (PREGAME, 2)
        );
        assert_eq!(
            match_state.values().len(),
            3,
            "the outposts only in the pregame"
        );
        assert_eq!(
            world.outpost_rush_foes(first, second),
            Some(false),
            "no fight in the pregame"
        );
        world.outpost_rush_ready(FIRST, true);
        assert_eq!(
            running(&world).participants[0].status,
            PLAYING_STATUS | READY_BIT
        );
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        assert_eq!(running(&world).state, RUNNING);
        assert_eq!(world.outpost_rush_foes(first, second), Some(true));
        stand_in_outpost(&mut world, FIRST, 0);
        let captured_ms = start_ms + u64::from(CAPTURE_DONE / CAPTURE_STEP) * CAPTURE_TICK_MS;
        tick_at(&mut world, captured_ms);
        assert_eq!(running(&world).points[0].owner, 0, "team 0 owns outpost A");
        let score = running(&world).scores[0];
        assert!(score > 0, "a held outpost scores each tick");
        world.outpost_rush_death(SECOND, FIRST);
        let match_state = running(&world);
        assert_eq!(match_state.scores[0], score + KILL_POINTS, "a kill scores");
        assert_eq!(match_state.participants[1].status & DEAD_BIT, DEAD_BIT);
        let spot = world.outpost_rush_respawn(second).expect("a respawn spot");
        assert_eq!((spot.x, spot.y), (1170.75, 987.75), "team 1's spot");
        assert_eq!(running(&world).participants[1].status & DEAD_BIT, 0);
        let ticks_to_win = u64::from(SCORE_TO_WIN);
        tick_at(&mut world, captured_ms + ticks_to_win * SCORE_TICK_MS);
        let match_state = running(&world);
        assert_eq!((match_state.state, match_state.winner), (ENDED, Some(0)));
        assert!(match_state.values().contains(&(WINNING_TEAM, 0)));
        tick_at(
            &mut world,
            captured_ms + ticks_to_win * SCORE_TICK_MS + CLEANUP_MS,
        );
        assert!(
            world.arena_matches.outpost_rush.is_none(),
            "the players go back"
        );
    }

    #[test]
    fn a_leave_of_a_running_match_bars_the_queue_and_the_other_team_wins() {
        let mut world = rush_sim();
        tick_at(&mut world, START_MS + PREGAME_MS);
        let second = world.player_by_number(SECOND).expect("player");
        world.leave_outpost_rush(second);
        let barred = world
            .world
            .get::<Player>(second)
            .expect("player")
            .arena
            .queue_eligible()
            .to_vec();
        assert_eq!(
            barred.first().map(|bar| bar.0),
            Some(lowercase_crc32(OUTPOST_RUSH_GAME_MODE))
        );
        tick_at(&mut world, START_MS + PREGAME_MS + 1);
        let match_state = running(&world);
        assert_eq!((match_state.state, match_state.winner), (ENDED, Some(0)));
    }

    #[test]
    fn the_time_limit_gives_the_match_to_the_leader_and_a_tie_to_no_team() {
        let mut world = rush_sim();
        let start_ms = START_MS + PREGAME_MS;
        tick_at(&mut world, start_ms);
        tick_at(&mut world, start_ms + TIME_LIMIT_MS);
        let match_state = running(&world);
        assert_eq!((match_state.state, match_state.winner), (ENDED, None));
    }
}
