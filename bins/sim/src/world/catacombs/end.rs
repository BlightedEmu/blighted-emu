// SPDX-License-Identifier: AGPL-3.0-or-later
//! The end of a catacombs run: the players' statuses and respawns, the extraction of each boss
//! room (and of the start room), the run's end, its payout and the players' leave.
//!
//! Observed (retail `CATACOMBS`, `CATACOMBS3`, `CATACOMBS5`, `CATACOMBS6`, `catacombshigh`;
//! seconds from each ledger's first record; key names are the CRC-32 of the lowercase name):
//!
//! - A participant's status is 3 while it plays, 0x0b in death's door and 7 once dead; each
//!   death takes one respawn from `catacombs_respawns_remaining` (`CATACOMBS5` 1644.054 s: status
//!   7 and the value 5 -> 4; 1646.552 s back to 3).
//! - Every boss tile (`tile1_a` to `tile5_a`) holds the encounter `EncounterExtraction`
//!   (`dg_enc_catacombs_extraction_prototype`) and the exit `EncounterExtractionPortal`
//!   (`catacombs_extraction_exit`). When the boss room is cleared, `dynamicdifficulty` rises by
//!   one and the map shows `extractionicon` 10 m from the room's centre (`CATACOMBS5` 929.415 s
//!   and 929.562 s, 1576.072 s; `CATACOMBS` 257.412 s).
//! - The extraction's top stages are, in order, `Setup`, `Activate core`, `Survive` (60 s),
//!   `Cleanup` (3 s) and `Exit` (60 s). When the core is activated the run goes to state 2 with
//!   the timers of the instance's end and `extractiontimer` 120 s later, and the boss tile goes
//!   to state 5 (`CATACOMBS5` 1593.555 s, timer +119.7 s; `CATACOMBS3` 898.020 s;
//!   `CATACOMBS6` 1392.705 s).
//! - A player in the exit (`ExtractionIGCTriggerVolume`: `m_radius` 1.1 m, `m_height` 50 m from
//!   20 m below the exit) while it is open is extracted: `catacombs_extracted_count` rises, its
//!   3312 flags gain the cutscene bits (byte 4, `07`) and it gets 6969 `ExtractionIGC`, then
//!   1.85 s later 6969 `ExtractionGodEyesView` (`CATACOMBS5` 1658.053 s to 1661.292 s: the
//!   player was 1.07 m from the exit, not yet at 3.1 m).
//! - The run ends (state 3) when every participant is extracted (`CATACOMBS5` 1674.550 s, 1.5 s
//!   after the third; `CATACOMBS6` 1469.702 s), when the extraction timer runs out
//!   (`CATACOMBS3` 1017.017 s, one of three out) or when every participant is dead and no
//!   respawn was left (`CATACOMBS` 947.915 s state 2, 948.417 s state 3; the client names it
//!   `NO_RESPAWN_POINTS`, `FUN_146484c80`, with `NO_PLAYERS_EXTRACTED` and
//!   `CATACOMBS_TIME_OUT` for the other failures). The end sets the values `e0a0e90f` (the
//!   participants; name Unknown), `CatacombsCurrency_Silvers` and `CatacombsCurrency_Crowns`
//!   (each the amount paid) and the timer `5e17d105` 3 s later; at that timer the value
//!   `b6bee0a1` is 1, the timer goes and the timer `dee04f78` is 180 s later.
//! - The payout (`FUN_1464882f0` and the five ledgers above, every value exact): each player's
//!   `CatacombsCurrency_CursedSilvers` and `_CursedCrowns` (a pool every participant shares,
//!   `FUN_146484b80`) go to 0 and `CatacombsCurrency_Silvers` and `_Crowns` gain: with nobody
//!   extracted a fifth of the pool (`PitySubtraction`, at least 1: 224 -> 44), with some
//!   extracted the pool (620 -> 620, 1 -> 1), with everyone extracted the pool and a fifth more
//!   (`BonusAddition`, at least 1: 618 -> 741, 1 -> 2; 1045 -> 1254, 4 -> 5; 4660 -> 5592,
//!   58 -> 69).
//! - A player leaves with the 1980 `Leave` (`CATACOMBS5` 1700.676 s; `CATACOMBS` 966.212 s): it
//!   gets 6970 (empty name), 3455, 5283 (its own member) and the 3312 of no game mode; the
//!   others get 6414 and the lists without it (1687.101 s to 1690.720 s).
//! - The start tile goes from state 0x0e to 3 300 s after the run's start (`CATACOMBS5` 110.631 s
//!   -> 410.637 s; `CATACOMBS2` consistent).
//!
//! Inferred: the start room's extraction opens with that change (its tile holds an extraction
//! like a boss tile's); the extraction icon's 10 m point turns with the tile; the run ends at
//! `catacombstimer` (`CATACOMBS_TIME_OUT`) as at the extraction timer; the players still in the
//! run leave at the timer `dee04f78`; the run's paid values are the pool's (every participant's
//! pool is the same).

use bevy_ecs::entity::Entity;
use nw_messages::game_mode_participant::{GameModeParticipantState, PARTICIPANT_FLAGS_LEN};
use nw_messages::game_mode_state::{GameModeState, Keyed, MapIcon};
use nw_messages::list_delta::{ListChange, ReplicatedList};
use nw_messages::social::CharacterRef;
use nw_messages::{GameModeServerMsg, ServerMessage};
use nw_world::catacomb_tiles::turned_point;

use super::super::attack::Vitals;
use super::board;
use super::super::game_mode_entity::{PLAYING_STATUS, full, participant_lists};
use super::super::queue::participant_header;
use super::super::{Body, Player, SimWorld};
use super::{
    CLEARED_ROOM_STATE, CatacombRun, DYNAMIC_DIFFICULTY_VALUE, RESPAWNS_PER_PARTICIPANT,
    RESPAWNS_REMAINING_VALUE, THIRD_RUN_VALUE, run_timers, with_flag,
};

/// The encounter slots of an extraction (module text).
const EXTRACTION_SLOTS: [&str; 2] = ["EncounterExtraction", "EncounterExtractionStartTile"];
/// The room kind whose clear opens its extraction.
const BOSS_ROOM: &str = "EncounterBoss";
/// The id of a boss room's end event before its two-digit order ([`boss_event`]).
const BOSS_EVENT_PREFIX: &str = "CatacombsKillEvent_Boss_";
/// The room kind of the start room.
const START_ROOM: &str = "EncounterStart";
/// The place in its top order of the extraction's `Survive` stage: the core was activated.
const SURVIVE_TOP: usize = 2;
/// The place in its top order of the extraction's `Exit` stage: the exit is open.
const EXIT_TOP: usize = 4;
/// The exit's trigger volume: radius, height and how far below the exit it starts, metres.
const EXIT_RADIUS_M: f32 = 1.1;
const EXIT_HEIGHT_M: f32 = 50.0;
const EXIT_BELOW_M: f32 = 20.0;
/// How far from the room's centre the extraction icon stands, metres.
const EXTRACTION_ICON_OFFSET_M: f32 = 10.0;
/// `extractionicon`.
const EXTRACTION_POINT_ICON: u32 = 0x4ebf_a0c4;
/// How long the extraction runs from the core's activation, ms.
const EXTRACTION_MS: u64 = 120_000;
/// How long after the run's start the start tile changes, ms.
const START_CLOSE_MS: u64 = 300_000;
/// The start tile's state from then on.
const START_CLOSED_STATE: u8 = 0x03;
/// The state of a boss tile whose extraction runs.
const EXTRACTING_ROOM_STATE: u8 = 0x05;
/// The run's script states of the extraction and of the end.
const RUN_EXTRACTION_STATE: u8 = 2;
const RUN_ENDED_STATE: u8 = 3;
/// The participant statuses of a player in death's door and of a dead one.
const DEATHS_DOOR_STATUS: u8 = 0x0b;
const DEAD_STATUS: u8 = 0x07;
/// `catacombs_extracted_count`.
const EXTRACTED_VALUE: u32 = 0x1cd8_e669;
/// The end's count of participants (name Unknown).
const PARTICIPANTS_VALUE: u32 = 0xe0a0_e90f;
/// `extractiontimer`.
const EXTRACTION_TIMER: u32 = 0xe7f3_f866;
/// The end's first timer and the leave timer (names Unknown).
const VICTORY_TIMER: u32 = 0x5e17_d105;
const LEAVE_TIMER: u32 = 0xdee0_4f78;
/// How long after the end its first timer runs out, and how long the leave timer runs, ms.
const VICTORY_DELAY_MS: u64 = 3_000;
const LEAVE_DELAY_MS: u64 = 180_000;
/// How long after the last extraction the run ends, ms.
const ALL_EXTRACTED_DELAY_MS: u64 = 1_500;
/// How long after the last death the run ends, ms.
const ALL_DEAD_DELAY_MS: u64 = 500;
/// The cutscene bits of the participant flags: byte and bits (`PlayerCutsceneIgnoredByAI`,
/// `PlayerCutsceneInvincibility` and the next one).
const CUTSCENE_FLAGS: (usize, u8) = (4, 0x07);
/// The two extraction cutscenes, their values and the time between them.
const EXTRACTION_CUTSCENE: (&str, f32) = ("ExtractionIGC", 0.0);
const GOD_EYES_CUTSCENE: (&str, f32) = ("ExtractionGodEyesView", 29.494_202);
const GOD_EYES_DELAY_MS: u64 = 1_850;
/// The run's currencies: each cursed pool and what it pays into, with the CRC of the paid one
/// (the run's value of the amount paid).
pub(super) const CURRENCIES: [(&str, &str, u32); 2] = [
    (
        "CatacombsCurrency_CursedSilvers",
        "CatacombsCurrency_Silvers",
        0xc946_10a6,
    ),
    (
        "CatacombsCurrency_CursedCrowns",
        "CatacombsCurrency_Crowns",
        0xd638_d8aa,
    ),
];
/// The places of the silvers and the crowns in [`CURRENCIES`].
const SILVERS: usize = 0;
const CROWNS: usize = 1;
/// The share of the pool of the pity and of the bonus: one fifth.
const PAYOUT_DIVISOR: u32 = 5;
/// The least pity or bonus of a pool that is not empty.
const LEAST_SHARE: u32 = 1;

/// Why a run ended (the client's names of the failures, `FUN_146484c80`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EndReason {
    /// Every participant was extracted.
    Extracted,
    /// The extraction timer ran out with some participants extracted.
    ExtractionOver,
    /// `NO_RESPAWN_POINTS`: every participant was dead and no respawn was left.
    NoRespawnPoints,
    /// `NO_PLAYERS_EXTRACTED`: the extraction ended with no participant extracted.
    NoPlayersExtracted,
    /// `CATACOMBS_TIME_OUT`: `catacombstimer` ran out.
    TimeOut,
}

/// One extraction of a run: its encounter, its room and its exit.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Extraction {
    /// Its index in the run's script.
    encounter: usize,
    /// Its room, by layout index.
    room: usize,
    /// Its exit: east, north, up (metres).
    exit: [f32; 3],
    /// It may run (its room was cleared, or the start room's time came).
    open: bool,
}

/// What the end of a run keeps.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct RunEnd {
    /// The run's extractions.
    extractions: Vec<Extraction>,
    /// The run's script state.
    state: u8,
    /// Server ms (since 2000-01-01) of the run's start.
    started_ms: Option<u64>,
    /// The start tile changed to [`START_CLOSED_STATE`].
    start_closed: bool,
    /// The run's values and timers, in full.
    values: Vec<Keyed>,
    timers: Vec<Keyed>,
    /// A death found no respawn left.
    out_of_respawns: bool,
    /// When the extraction ends, server ms.
    extraction_ends_ms: Option<u64>,
    /// When `catacombstimer` runs out, server ms.
    time_out_ms: Option<u64>,
    /// The extracted players, by number.
    extracted: Vec<u64>,
    /// The boss rooms cleared.
    bosses_cleared: u32,
    /// The second cutscene each extracted player gets, at server ms.
    god_eyes_due: Vec<(Entity, u64, u64)>,
    /// The end that is due: its reason and when, server ms.
    end_due: Option<(EndReason, u64)>,
    /// When the end's first timer and the leave timer run out, server ms.
    victory_ms: Option<u64>,
    leave_ms: Option<u64>,
}

/// One participant as a tick sees it.
struct Seen {
    entity: Entity,
    number: u64,
    at: [f32; 3],
    status: u8,
}

/// The end event of the `boss`-th boss room cleared (1-based): `CatacombsKillEvent_Boss_01` to
/// `_05` (Observed, the content's rows and the ledgers of the module note).
fn boss_event(boss: u32) -> String {
    format!("{BOSS_EVENT_PREFIX}{boss:02}")
}

/// The amount a pool of `pool` pays when `extracted` of `participants` were extracted
/// (module text).
fn payout(pool: u32, extracted: usize, participants: usize) -> u32 {
    if pool == 0 {
        return 0;
    }
    let share = (pool / PAYOUT_DIVISOR).max(LEAST_SHARE);
    match extracted {
        0 => share,
        all if all >= participants => pool.saturating_add(share),
        _ => pool,
    }
}

/// Whether `at` stands in the trigger volume of the exit at `exit`.
fn in_exit(at: [f32; 3], exit: [f32; 3]) -> bool {
    let [east, north] = [at[0] - exit[0], at[1] - exit[1]];
    let above = at[2] - (exit[2] - EXIT_BELOW_M);
    east.hypot(north) <= EXIT_RADIUS_M && (0.0..=EXIT_HEIGHT_M).contains(&above)
}

/// `values` with `key` set to `value` (added at the end when it is new).
fn set_keyed(values: &mut Vec<Keyed>, key: u32, value: u64) {
    match values.iter_mut().find(|(known, _)| *known == key) {
        Some(entry) => entry.1 = value,
        None => values.push((key, value)),
    }
}

/// The status of a player of `vitals`.
fn status_of(vitals: &Vitals) -> u8 {
    if vitals.deaths_door_since_ms.is_some() {
        DEATHS_DOOR_STATUS
    } else if vitals.health.is_dead() {
        DEAD_STATUS
    } else {
        PLAYING_STATUS
    }
}

impl RunEnd {
    /// The end of a run whose script holds `encounters` (by slot name and place) in the rooms
    /// `rooms` (the room of each encounter).
    pub(super) fn of<'a>(
        encounters: impl Iterator<Item = (&'a str, [f32; 3])>,
        rooms: &[usize],
    ) -> Self {
        let extractions = encounters
            .zip(rooms)
            .enumerate()
            .filter(|(_, ((name, _), _))| EXTRACTION_SLOTS.contains(name))
            .map(|(encounter, ((_, exit), room))| Extraction {
                encounter,
                room: *room,
                exit,
                open: false,
            })
            .collect();
        Self {
            extractions,
            ..Self::default()
        }
    }

    /// Whether encounter `index` is an extraction.
    pub(super) fn is_extraction(&self, index: usize) -> bool {
        self.extractions
            .iter()
            .any(|extraction| extraction.encounter == index)
    }

    /// The values and timers of a run of `participants` players that starts at `server_ms` in
    /// an instance that ends at Unix ms `ends_at_ms` (the start's full lists).
    pub(super) fn start(
        &mut self,
        participants: u64,
        server_ms: u64,
        ends_at_ms: Option<u64>,
    ) -> (Vec<Keyed>, Vec<Keyed>) {
        self.state = super::RUN_STARTED_STATE;
        self.started_ms = Some(server_ms);
        self.values = vec![
            (DYNAMIC_DIFFICULTY_VALUE, 0),
            (
                RESPAWNS_REMAINING_VALUE,
                participants * RESPAWNS_PER_PARTICIPANT,
            ),
            (THIRD_RUN_VALUE, 0),
        ];
        self.timers = run_timers(server_ms, ends_at_ms);
        self.time_out_ms = self
            .timers
            .iter()
            .find(|(key, _)| *key == super::CATACOMBS_TIMER)
            .map(|(_, at)| *at);
        (self.values.clone(), self.timers.clone())
    }

    /// The value `key`.
    pub(super) fn value(&self, key: u32) -> u64 {
        self.values
            .iter()
            .find(|(known, _)| *known == key)
            .map_or(0, |(_, value)| *value)
    }
}

impl CatacombRun {
    /// Which encounters of the run's script may run: every one but an extraction that did not
    /// open yet.
    pub(in crate::world) fn owned_encounters(&self) -> Vec<bool> {
        (0..self.script.encounters.len())
            .map(|index| {
                self.end
                    .extractions
                    .iter()
                    .find(|extraction| extraction.encounter == index)
                    .is_none_or(|extraction| extraction.open)
            })
            .collect()
    }

    /// The next version of the run's lists.
    fn next_version(&mut self) -> u64 {
        self.progress.version += 1;
        self.progress.version
    }

    /// The run's own entity's change of its values and timers in full, at a new version.
    fn keyed_change(&mut self) -> GameModeState {
        let version = self.next_version();
        GameModeState {
            values: Some(full(version, self.end.values.clone())),
            timers: Some(full(version, self.end.timers.clone())),
            ..GameModeState::default()
        }
    }

    /// A player joined: when the run started, its respawns join the run's (Observed: +3 45 ms
    /// after the join, `CATACOMBS5` 902.017 s to 902.062 s, `CATACOMBS3` 13.522 s, `CATACOMBS6`
    /// 513.294 s to 513.705 s; a join before the start counts in [`RunEnd::start`]). The run
    /// entity's change.
    pub(super) fn late_join_change(&mut self) -> Option<GameModeState> {
        self.end.started_ms?;
        let respawns = self.end.value(RESPAWNS_REMAINING_VALUE) + RESPAWNS_PER_PARTICIPANT;
        set_keyed(&mut self.end.values, RESPAWNS_REMAINING_VALUE, respawns);
        let mut change = self.keyed_change();
        change.timers = None;
        Some(change)
    }

    /// Room `room` was cleared: a boss room's extraction opens, `dynamicdifficulty` rises and the
    /// map shows the extraction icon (module text). The run entity's change.
    pub(super) fn room_cleared(&mut self, room: usize) -> Option<GameModeState> {
        let placed = self.layout.rooms.get(room)?;
        if placed.room_type != BOSS_ROOM {
            return None;
        }
        let point = turned_point(
            [0.0, EXTRACTION_ICON_OFFSET_M, 0.0],
            placed.position,
            placed.yaw_deg,
        );
        for extraction in &mut self.end.extractions {
            if extraction.room == room {
                extraction.open = true;
            }
        }
        self.end.bosses_cleared += 1;
        let difficulty = self.end.value(DYNAMIC_DIFFICULTY_VALUE) + 1;
        set_keyed(&mut self.end.values, DYNAMIC_DIFFICULTY_VALUE, difficulty);
        let mut change = self.keyed_change();
        change.timers = None;
        let version = self.progress.version;
        let index = self.progress.icons.len();
        let icon = MapIcon {
            key: EXTRACTION_POINT_ICON,
            // A run's map spans a few kilometres at most.
            position_m: [point[0] as u16, point[1] as u16],
        };
        self.progress.icons.push(icon);
        change.level_map.icons = Some(ReplicatedList::Changes(vec![ListChange {
            // A run shows a few dozen icons.
            index: index as u32,
            version,
            value: icon,
        }]));
        Some(change)
    }

    /// The participants' statuses `seen` (in participant order): the changed statuses, each
    /// death's respawn, and whether a death found no respawn left. The run entity's change.
    fn status_changes(&mut self, seen: &[Seen]) -> Option<GameModeState> {
        let mut changed = Vec::new();
        for (index, participant) in self.progress.participants.iter_mut().enumerate() {
            let Some(now) = seen
                .iter()
                .find(|seen| seen.entity == participant.entity)
                .map(|seen| seen.status)
            else {
                continue;
            };
            if now == participant.status {
                continue;
            }
            if now == DEAD_STATUS {
                let left = self.end.value(RESPAWNS_REMAINING_VALUE);
                if left == 0 {
                    self.end.out_of_respawns = true;
                }
                set_keyed(
                    &mut self.end.values,
                    RESPAWNS_REMAINING_VALUE,
                    left.saturating_sub(1),
                );
            } else if now == PLAYING_STATUS {
                self.end.out_of_respawns = false;
            }
            participant.status = now;
            changed.push((index, now));
        }
        if changed.is_empty() {
            return None;
        }
        let mut change = self.keyed_change();
        change.timers = None;
        let version = self.progress.version;
        change.statuses = Some(ReplicatedList::Changes(
            changed
                .iter()
                .enumerate()
                .map(|(position, (index, status))| ListChange {
                    // A run holds three participants.
                    index: *index as u32,
                    version: if position == 0 { version } else { 0 },
                    value: *status,
                })
                .collect(),
        ));
        Some(change)
    }
}

impl SimWorld {
    /// One tick of the end of this sim's catacombs run (module text).
    pub(in crate::world) fn tick_catacomb_end(&mut self) {
        let server_ms = self.server_ms;
        let Some(run) = self.catacomb_run() else {
            return;
        };
        let tops: Vec<(usize, Option<usize>)> = run
            .end
            .extractions
            .iter()
            .map(|extraction| {
                let top = self
                    .dungeon_run
                    .as_ref()
                    .and_then(|dungeon| dungeon.running_top(extraction.encounter));
                (extraction.encounter, top)
            })
            .collect();
        let numbers: Vec<Entity> = run
            .progress
            .participants
            .iter()
            .map(|participant| participant.entity)
            .collect();
        let seen: Vec<Seen> = numbers
            .iter()
            .filter_map(|entity| {
                let player = self.world.get::<Player>(*entity)?;
                let at = self.world.get::<Body>(*entity)?.motion.position();
                let vitals = self.world.get::<Vitals>(*entity)?;
                Some(Seen {
                    entity: *entity,
                    number: player.number,
                    at: [at.x, at.y, at.z],
                    status: status_of(vitals),
                })
            })
            .collect();
        let pools = self.catacomb_pools(&numbers);
        let Some(run) = self.catacomb_run_mut() else {
            return;
        };
        let Some((mode_entity, _)) = run.progress.entity else {
            return;
        };
        let mut states: Vec<(u64, GameModeState)> = Vec::new();
        let mut extracted_now = Vec::new();
        if run.end.started_ms.is_some() && run.end.state < RUN_ENDED_STATE {
            states.extend(run.status_changes(&seen).map(|change| (mode_entity, change)));
            states.extend(run.start_close(server_ms));
            states.extend(run.extraction_changes(&tops, server_ms));
            let (changes, extracted) = run.exits(&tops, &seen, server_ms);
            states.extend(changes.map(|change| (mode_entity, change)));
            extracted_now = extracted;
            run.end_checks(&seen, server_ms);
        }
        let end = run.end_change(server_ms, pools);
        let ended = end.as_ref().map(|(reason, _, _)| *reason);
        let payouts = end.as_ref().map(|(_, _, payouts)| payouts.clone());
        if let Some(payouts) = &payouts {
            let paid = |currency: usize| {
                payouts
                    .iter()
                    .find(|(paid, _)| *paid == currency)
                    .map_or(0, |(_, amount)| *amount)
            };
            let extracted = u32::try_from(run.end.extracted.len()).unwrap_or(u32::MAX);
            let mut totals = vec![
                (board::EXTRACTED, extracted),
                (board::SILVERS_EXTRACTED, paid(SILVERS)),
                (board::CROWNS_EXTRACTED, paid(CROWNS)),
            ];
            totals.extend(run.pool.board_totals());
            run.board.set(&totals, server_ms);
        }
        if let Some((_, change, _)) = end {
            states.push((mode_entity, change));
        }
        states.extend(run.victory_change(server_ms).map(|change| (mode_entity, change)));
        let god_eyes: Vec<(Entity, u64)> = {
            let (due, waiting) = std::mem::take(&mut run.end.god_eyes_due)
                .into_iter()
                .partition(|(_, _, at)| *at <= server_ms);
            run.end.god_eyes_due = waiting;
            due.into_iter().map(|(entity, number, _)| (entity, number)).collect()
        };
        let leave_now = run.end.leave_ms.is_some_and(|at| at <= server_ms);
        let flags: Vec<(Entity, [u8; PARTICIPANT_FLAGS_LEN])> = extracted_now
            .iter()
            .filter_map(|entity| {
                let index = run
                    .progress
                    .participants
                    .iter()
                    .position(|participant| participant.entity == *entity)?;
                let flags = run.progress.flags.get_mut(index)?;
                *flags = with_flag(*flags, CUTSCENE_FLAGS, true);
                Some((*entity, *flags))
            })
            .collect();
        for (entity, change) in states {
            self.change_game_mode_state(entity, change);
        }
        for (entity, flags) in flags {
            self.put_participant_state(
                entity,
                GameModeParticipantState {
                    flags: Some(flags),
                    ..GameModeParticipantState::default()
                },
            );
            self.start_cutscene(entity, EXTRACTION_CUTSCENE);
        }
        for (entity, _) in god_eyes {
            self.start_cutscene(entity, GOD_EYES_CUTSCENE);
        }
        if let (Some(reason), Some(payouts)) = (ended, payouts) {
            tracing::info!(?reason, ?payouts, "catacombs run ended");
            self.pay_catacombs(&payouts);
            self.give_catacomb_boss_events();
        }
        if leave_now {
            let players: Vec<Entity> = self
                .catacomb_run()
                .map(|run| {
                    run.progress
                        .participants
                        .iter()
                        .map(|participant| participant.entity)
                        .collect()
                })
                .unwrap_or_default();
            for player in players {
                self.leave_catacombs(player);
            }
        }
    }

    /// The pool of each run currency ([`CURRENCIES`]) of the participants `players`: the
    /// largest participant's (every participant's pool is the same, module text).
    fn catacomb_pools(&self, players: &[Entity]) -> [u32; CURRENCIES.len()] {
        CURRENCIES.map(|(cursed, _, _)| {
            let Some(mastery) = self.content.progression.categorical(cursed) else {
                return 0;
            };
            players
                .iter()
                .filter_map(|entity| self.world.get::<Player>(*entity))
                .map(|player| player.masteries.experience(mastery))
                .max()
                .unwrap_or_default()
        })
    }

    /// Sends player `entity` the start of the cutscene `(name, value)`.
    fn start_cutscene(&mut self, entity: Entity, (name, value): (&str, f32)) {
        let Some(number) = self.world.get::<Player>(entity).map(|player| player.number) else {
            return;
        };
        let message = ServerMessage::GameMode(GameModeServerMsg::StartCutsceneWithName {
            header: participant_header(number),
            name: name.to_owned(),
            value_bits: value.to_bits(),
        });
        self.send_to(entity, &[message]);
    }

    /// Every participant's cursed currencies go to 0 and pay `payouts` (per paid currency:
    /// its id and amount) into its currencies.
    fn pay_catacombs(&mut self, payouts: &[(usize, u32)]) {
        let content = std::sync::Arc::clone(&self.content);
        let players: Vec<Entity> = self
            .catacomb_run()
            .map(|run| {
                run.progress
                    .participants
                    .iter()
                    .map(|participant| participant.entity)
                    .collect()
            })
            .unwrap_or_default();
        for entity in players {
            let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                continue;
            };
            for (currency, amount) in payouts {
                let Some((cursed, paid, _)) = CURRENCIES.get(*currency) else {
                    continue;
                };
                let (Some(cursed), Some(paid)) = (
                    content.progression.categorical(cursed),
                    content.progression.categorical(paid),
                ) else {
                    tracing::warn!(currency = paid, "catacombs currency not in the content");
                    continue;
                };
                player.masteries.set(cursed, 0);
                let held = player.masteries.experience(paid);
                player.masteries.set(paid, held.saturating_add(*amount));
            }
        }
    }

    /// Each extracted participant gets one end event for each boss room the run cleared, in
    /// the order of the clears ([`boss_event`]; Observed: `CATACOMBS5` 1674.6 s `_01` and `_02`
    /// after 2 boss kills, `CATACOMBS6` 1469.7 s 3, `catacombshigh` 1075.3 s 5, each with its
    /// umbral shards and 100 souls, at the payout; none to a player who was not extracted:
    /// `CATACOMBS3` 1017.0 s, `CATACOMBS` 948.4 s).
    fn give_catacomb_boss_events(&mut self) {
        let Some(run) = self.catacomb_run() else {
            return;
        };
        let bosses = run.end.bosses_cleared;
        let extracted: Vec<(Entity, u64)> = run
            .progress
            .participants
            .iter()
            .filter(|participant| run.end.extracted.contains(&participant.number))
            .map(|participant| (participant.entity, participant.number))
            .collect();
        for (entity, number) in extracted {
            for boss in 1..=bosses {
                self.give_mode_event(entity, number, &boss_event(boss));
            }
        }
    }

    /// Player `entity` leaves the catacombs run (1980 `Leave`, 3309 or the leave timer): what a
    /// player who leaves gets, the others' 6414 and lists, and it goes back.
    pub(in crate::world) fn leave_catacombs(&mut self, entity: Entity) {
        if let Some((participant, version, others, mode, extracted)) =
            self.drop_catacomb_participant(entity)
        {
            if extracted {
                let message = ServerMessage::GameMode(GameModeServerMsg::StopCutsceneWithName {
                    header: participant_header(participant.number),
                    name: String::new(),
                });
                self.send_to(entity, &[message]);
            }
            self.send_match_left(entity, &participant, version);
            for (other, number) in others {
                let message = ServerMessage::GameMode(GameModeServerMsg::GameModePlayerRemoved {
                    header: participant_header(number),
                    game_mode: mode,
                    player: CharacterRef(Some(participant.character)),
                });
                self.send_to(other, &[message]);
            }
        }
        self.exit_instance(entity);
    }

    /// A participant of the run leaves the sim (a logout or a lost connection).
    pub(in crate::world) fn catacombs_left_sim(&mut self, entity: Entity) {
        self.drop_catacomb_participant(entity);
    }

    /// Takes player `entity` out of the run's lists on every entity that holds them. The
    /// participant, the list version, the others (entity, number), the mode's CRC and whether it
    /// was extracted.
    #[allow(clippy::type_complexity, reason = "one private caller pair")]
    fn drop_catacomb_participant(
        &mut self,
        entity: Entity,
    ) -> Option<(
        super::super::game_mode_entity::MatchParticipant,
        u64,
        Vec<(Entity, u64)>,
        u32,
        bool,
    )> {
        let server_ms = self.server_ms;
        let run = self.catacomb_run_mut()?;
        let index = run
            .progress
            .participants
            .iter()
            .position(|participant| participant.entity == entity)?;
        let participant = run.progress.participants.remove(index);
        run.board.leave(participant.number, server_ms);
        run.progress.flags.remove(index);
        let extracted = run.end.extracted.contains(&participant.number);
        let version = run.next_version();
        let lists = participant_lists(&run.progress.participants, version, true);
        let others = run
            .progress
            .participants
            .iter()
            .map(|other| (other.entity, other.number))
            .collect();
        let mode = run.progress.mode_crc;
        let entities = run.state_entities();
        for state_entity in entities {
            self.change_game_mode_state(state_entity, lists.clone());
        }
        Some((participant, version, others, mode, extracted))
    }
}

impl CatacombRun {
    /// The start tile's change 300 s after the run's start; the start room's extraction opens.
    fn start_close(&mut self, server_ms: u64) -> Option<(u64, GameModeState)> {
        let started = self.end.started_ms?;
        if self.end.start_closed || server_ms < started.saturating_add(START_CLOSE_MS) {
            return None;
        }
        self.end.start_closed = true;
        let start = self
            .layout
            .rooms
            .iter()
            .position(|room| room.room_type == START_ROOM)?;
        for extraction in &mut self.end.extractions {
            if extraction.room == start {
                extraction.open = true;
            }
        }
        let room = self.rooms.get_mut(start)?;
        room.state = START_CLOSED_STATE;
        Some((
            room.tile?,
            GameModeState {
                script_state: Some(START_CLOSED_STATE),
                ..GameModeState::default()
            },
        ))
    }

    /// The run's change when an extraction's core was activated (`tops`: each extraction's
    /// running top stage): state 2, the extraction timer, the room's tile in state 5.
    fn extraction_changes(
        &mut self,
        tops: &[(usize, Option<usize>)],
        server_ms: u64,
    ) -> Vec<(u64, GameModeState)> {
        let mut changes = Vec::new();
        if self.end.state >= RUN_EXTRACTION_STATE {
            return changes;
        }
        let Some(extraction) = self.end.extractions.iter().find(|extraction| {
            tops.iter().any(|(encounter, top)| {
                *encounter == extraction.encounter && top.is_some_and(|top| top >= SURVIVE_TOP)
            })
        }) else {
            return changes;
        };
        let room = extraction.room;
        let Some((mode_entity, _)) = self.progress.entity else {
            return changes;
        };
        let ends_ms = server_ms.saturating_add(EXTRACTION_MS);
        self.end.state = RUN_EXTRACTION_STATE;
        self.end.extraction_ends_ms = Some(ends_ms);
        self.end
            .timers
            .retain(|(key, _)| *key == super::INSTANCE_END_TIMER);
        self.end.timers.push((EXTRACTION_TIMER, ends_ms));
        let mut change = self.keyed_change();
        change.values = None;
        change.script_state = Some(RUN_EXTRACTION_STATE);
        changes.push((mode_entity, change));
        if let Some(progress) = self.rooms.get_mut(room)
            && progress.state == CLEARED_ROOM_STATE
        {
            progress.state = EXTRACTING_ROOM_STATE;
            changes.extend(progress.tile.map(|tile| {
                (
                    tile,
                    GameModeState {
                        script_state: Some(EXTRACTING_ROOM_STATE),
                        ..GameModeState::default()
                    },
                )
            }));
        }
        changes
    }

    /// The players `seen` that stand in an open exit (`tops`) and were not extracted yet are
    /// extracted: the run's change of its count and those players.
    fn exits(
        &mut self,
        tops: &[(usize, Option<usize>)],
        seen: &[Seen],
        server_ms: u64,
    ) -> (Option<GameModeState>, Vec<Entity>) {
        let open: Vec<[f32; 3]> = self
            .end
            .extractions
            .iter()
            .filter(|extraction| {
                tops.iter().any(|(encounter, top)| {
                    *encounter == extraction.encounter && *top == Some(EXIT_TOP)
                })
            })
            .map(|extraction| extraction.exit)
            .collect();
        let mut extracted = Vec::new();
        for player in seen {
            if player.status != PLAYING_STATUS
                || self.end.extracted.contains(&player.number)
                || !open.iter().any(|exit| in_exit(player.at, *exit))
            {
                continue;
            }
            self.end.extracted.push(player.number);
            self.end.god_eyes_due.push((
                player.entity,
                player.number,
                server_ms.saturating_add(GOD_EYES_DELAY_MS),
            ));
            extracted.push(player.entity);
        }
        if extracted.is_empty() {
            return (None, extracted);
        }
        set_keyed(
            &mut self.end.values,
            EXTRACTED_VALUE,
            self.end.extracted.len() as u64,
        );
        let mut change = self.keyed_change();
        change.timers = None;
        (Some(change), extracted)
    }

    /// Sets the end that is due, if none is (module text).
    fn end_checks(&mut self, seen: &[Seen], server_ms: u64) {
        if self.end.end_due.is_some() {
            return;
        }
        let participants = self.progress.participants.len();
        let extracted = self.end.extracted.len();
        let due = if participants > 0 && extracted >= participants {
            Some((
                EndReason::Extracted,
                server_ms.saturating_add(ALL_EXTRACTED_DELAY_MS),
            ))
        } else if self.end.out_of_respawns
            && !seen.is_empty()
            && seen.iter().all(|player| player.status == DEAD_STATUS)
        {
            Some((
                EndReason::NoRespawnPoints,
                server_ms.saturating_add(ALL_DEAD_DELAY_MS),
            ))
        } else if self
            .end
            .extraction_ends_ms
            .is_some_and(|ends| server_ms >= ends)
        {
            let reason = if extracted == 0 {
                EndReason::NoPlayersExtracted
            } else {
                EndReason::ExtractionOver
            };
            Some((reason, server_ms))
        } else if self.end.time_out_ms.is_some_and(|ends| server_ms >= ends) {
            Some((EndReason::TimeOut, server_ms))
        } else {
            None
        };
        self.end.end_due = due;
    }

    /// The run's end when it is due, with the run currencies' pools `pools`: the reason, the
    /// run entity's change and the payouts (currency index, amount).
    #[allow(clippy::type_complexity, reason = "one private caller")]
    fn end_change(
        &mut self,
        server_ms: u64,
        pools: [u32; CURRENCIES.len()],
    ) -> Option<(EndReason, GameModeState, Vec<(usize, u32)>)> {
        let (reason, at) = self.end.end_due?;
        if server_ms < at || self.end.state >= RUN_ENDED_STATE {
            return None;
        }
        let mut change = GameModeState::default();
        if reason == EndReason::NoRespawnPoints && self.end.state < RUN_EXTRACTION_STATE {
            // Observed: the run goes through state 2 to 3 (`CATACOMBS` 947.915 s).
            self.end.timers.retain(|(key, _)| *key == super::INSTANCE_END_TIMER);
            self.end
                .timers
                .push((EXTRACTION_TIMER, server_ms.saturating_add(EXTRACTION_MS)));
        }
        self.end.state = RUN_ENDED_STATE;
        let participants = self.progress.participants.len();
        let extracted = self.end.extracted.len();
        let payouts: Vec<(usize, u32)> = pools
            .iter()
            .enumerate()
            .map(|(currency, pool)| (currency, payout(*pool, extracted, participants)))
            .collect();
        set_keyed(&mut self.end.values, PARTICIPANTS_VALUE, participants as u64);
        for (currency, amount) in &payouts {
            if let Some((_, _, key)) = CURRENCIES.get(*currency) {
                set_keyed(&mut self.end.values, *key, u64::from(*amount));
            }
        }
        let victory = server_ms.saturating_add(VICTORY_DELAY_MS);
        set_keyed(&mut self.end.timers, VICTORY_TIMER, victory);
        self.end.victory_ms = Some(victory);
        let keyed = self.keyed_change();
        change.values = keyed.values;
        change.timers = keyed.timers;
        change.script_state = Some(RUN_ENDED_STATE);
        Some((reason, change, payouts))
    }

    /// The change at the end's first timer: `b6bee0a1` is 1 and the leave timer runs.
    fn victory_change(&mut self, server_ms: u64) -> Option<GameModeState> {
        let at = self.end.victory_ms?;
        if server_ms < at {
            return None;
        }
        self.end.victory_ms = None;
        set_keyed(&mut self.end.values, THIRD_RUN_VALUE, 1);
        self.end.timers.retain(|(key, _)| *key != VICTORY_TIMER);
        let leave = server_ms.saturating_add(LEAVE_DELAY_MS);
        self.end.timers.push((LEAVE_TIMER, leave));
        self.end.leave_ms = Some(leave);
        Some(self.keyed_change())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{PLAYER, arrived_run, held_state, stand_in_room};
    use nw_formats::objectstream::lowercase_crc32;
    use nw_messages::MemberState;
    use super::*;

    /// The server ms the run starts at in these tests.
    const START_MS: u64 = 1_000_000;

    /// The retail payouts (module text): nobody, some or everyone extracted.
    #[test]
    fn the_payout_is_the_pity_the_pool_or_the_bonus_of_the_retail_runs() {
        // `CATACOMBS`: two players, nobody out.
        assert_eq!(payout(224, 0, 2), 44);
        assert_eq!(payout(0, 0, 2), 0);
        // `CATACOMBS3`: one of three out.
        assert_eq!(payout(620, 1, 3), 620);
        assert_eq!(payout(1, 1, 3), 1);
        // `CATACOMBS5`, `CATACOMBS6`, `catacombshigh`: all three out.
        assert_eq!(payout(618, 3, 3), 741);
        assert_eq!(payout(1, 3, 3), 2);
        assert_eq!(payout(1045, 3, 3), 1254);
        assert_eq!(payout(4, 3, 3), 5);
        assert_eq!(payout(4660, 3, 3), 5592);
        assert_eq!(payout(58, 3, 3), 69);
    }

    /// Retail `CATACOMBS5` 1659.004 to 1659.336 s: 1.07 m from the exit is in, 3.1 m is not; the
    /// volume reaches from 20 m below the exit to 30 m above it.
    #[test]
    fn the_exit_holds_a_player_within_its_radius_and_height() {
        let exit = [576.0, 448.0, 118.0];
        assert!(in_exit([574.93, 447.96, 118.1], exit));
        assert!(!in_exit([572.91, 447.73, 118.1], exit));
        assert!(in_exit([576.0, 448.0, 98.5], exit));
        assert!(!in_exit([576.0, 448.0, 97.0], exit));
        assert!(!in_exit([576.0, 448.0, 149.0], exit));
    }

    /// A run of the test world that started at [`START_MS`], with the index of its boss room and
    /// of that room's extraction encounter.
    fn started_run() -> (SimWorld, usize, usize) {
        let (mut world, _) = arrived_run();
        let run = world.catacomb_run_mut().expect("run");
        let (values, timers) = run.end.start(1, START_MS, None);
        assert_eq!(values[1], (RESPAWNS_REMAINING_VALUE, 3));
        assert_eq!(timers.len(), 1, "an instance with no end has the run timer alone");
        run.progress.started = true;
        let boss = run
            .layout
            .rooms
            .iter()
            .position(|room| room.room_type == BOSS_ROOM)
            .expect("a boss room");
        let extraction = run
            .end
            .extractions
            .iter()
            .find(|extraction| extraction.room == boss)
            .expect("the boss room's extraction")
            .encounter;
        (world, boss, extraction)
    }

    /// Retail `CATACOMBS5` 1674.6 s: after two boss rooms, the extracted player gets
    /// `CatacombsKillEvent_Boss_01` and `_02` (umbral 10 and 15) in one list; a player who was
    /// not extracted gets none.
    #[test]
    fn the_extracted_players_get_one_end_event_for_each_cleared_boss_room() {
        use nw_content::game_mode_rewards::{ModeReward, ModeRewards};
        assert_eq!(boss_event(1), "CatacombsKillEvent_Boss_01");
        let (mut world, boss, _) = started_run();
        let mut content = (*world.content).clone();
        content.mode_rewards = ModeRewards::build([1_u32, 2].map(|boss| {
            let reward = ModeReward {
                umbral: f64::from(5 + 5 * boss),
                ..ModeReward::default()
            };
            (boss_event(boss), reward)
        }));
        world.content = std::sync::Arc::new(content);
        let entity = world.player_by_number(PLAYER).expect("player");
        let run = world.catacomb_run_mut().expect("run");
        run.room_cleared(boss).expect("the boss room");
        run.room_cleared(boss).expect("the boss room");
        world.give_catacomb_boss_events();
        let log = |world: &SimWorld| {
            world
                .world
                .get::<Player>(entity)
                .expect("player")
                .game_events
                .clone()
        };
        assert!(!log(&world).has_change(), "not extracted");
        world.catacomb_run_mut().expect("run").end.extracted.push(PLAYER);
        world.give_catacomb_boss_events();
        let member = log(&world).take_member(1).expect("two entries");
        let MemberState::GameEvents(state) = member.state else {
            panic!("a game event member");
        };
        let Some(ReplicatedList::Full(list)) = state.game_events else {
            panic!("the first entries go in full");
        };
        let sent: Vec<(u32, u32)> = list
            .elements
            .iter()
            .map(|entry| (entry.event, entry.value_h))
            .collect();
        assert_eq!(
            sent,
            [
                (lowercase_crc32(&boss_event(1)), 10),
                (lowercase_crc32(&boss_event(2)), 15)
            ]
        );
    }

    /// Retail `CATACOMBS5` 902.017 s to 902.062 s: a player who joins the started run adds its
    /// three respawns; before the start the run's start counts every participant.
    #[test]
    fn a_late_joiner_adds_its_respawns_to_the_started_run() {
        let (mut world, _) = arrived_run();
        let run = world.catacomb_run_mut().expect("run");
        assert_eq!(run.late_join_change(), None, "not started");
        let (mut world, _, _) = started_run();
        let run = world.catacomb_run_mut().expect("run");
        let change = run.late_join_change().expect("a change");
        let values = change.values.expect("values").elements;
        assert!(
            values.contains(&(RESPAWNS_REMAINING_VALUE, 2 * RESPAWNS_PER_PARTICIPANT)),
            "{values:x?}"
        );
        assert_eq!(change.timers, None);
    }

    /// Retail `CATACOMBS5` 1576.072 s: the cleared boss room opens its extraction (held until
    /// then), raises `dynamicdifficulty` and shows the extraction icon 10 m from its centre; the
    /// extraction is no encounter of the room's clear.
    #[test]
    fn a_cleared_boss_room_opens_its_extraction_and_raises_the_difficulty() {
        let (mut world, boss, extraction) = started_run();
        let run = world.catacomb_run().expect("run").clone();
        assert!(!run.owned_encounters()[extraction], "held before the clear");
        assert!(!run.rooms[boss].open_encounters.contains(&extraction));
        let (mode_entity, _) = run.progress.entity.expect("run entity");
        for encounter in run.rooms[boss].open_encounters.clone() {
            world.catacomb_encounter_done(encounter);
        }
        let run = world.catacomb_run().expect("run").clone();
        assert!(run.owned_encounters()[extraction], "open after the clear");
        assert_eq!(run.end.value(DYNAMIC_DIFFICULTY_VALUE), 1);
        let held = held_state(&world, mode_entity);
        let values = held.values.expect("values").elements;
        assert!(values.contains(&(DYNAMIC_DIFFICULTY_VALUE, 1)), "{values:x?}");
        let icon = run.progress.icons.last().expect("an icon");
        assert_eq!(icon.key, EXTRACTION_POINT_ICON);
        let room = &run.layout.rooms[boss];
        let centre = [room.position[0], room.position[1]];
        let offset = (f32::from(icon.position_m[0]) - centre[0])
            .hypot(f32::from(icon.position_m[1]) - centre[1]);
        assert!((offset - EXTRACTION_ICON_OFFSET_M).abs() <= 1.0, "{offset}");
    }

    /// Retail `CATACOMBS5` 1593.555 s to 1677.567 s: the activated core starts the extraction
    /// (state 2, 120 s timer, boss tile 5); a player in the open exit is extracted; with every
    /// participant out the run ends 1.5 s later with the bonus payout, and 3 s after the end the
    /// leave timer runs.
    #[test]
    fn an_open_exit_extracts_the_player_and_the_run_ends_with_the_bonus() {
        let (mut world, boss, extraction) = started_run();
        let run = world.catacomb_run_mut().expect("run");
        run.rooms[boss].state = CLEARED_ROOM_STATE;
        let exit = run
            .end
            .extractions
            .iter()
            .find(|known| known.encounter == extraction)
            .expect("extraction")
            .exit;
        let mut now = START_MS + 600_000;
        let changes = run.extraction_changes(&[(extraction, Some(SURVIVE_TOP))], now);
        assert_eq!(run.end.state, RUN_EXTRACTION_STATE);
        assert_eq!(changes[0].1.script_state, Some(RUN_EXTRACTION_STATE));
        assert_eq!(
            changes[0].1.timers.as_ref().expect("timers").elements,
            vec![(EXTRACTION_TIMER, now + EXTRACTION_MS)]
        );
        assert_eq!(changes[1].1.script_state, Some(EXTRACTING_ROOM_STATE));
        assert_eq!(run.rooms[boss].state, EXTRACTING_ROOM_STATE);
        let entity = run.progress.participants[0].entity;
        let player = |at| Seen {
            entity,
            number: PLAYER,
            at,
            status: PLAYING_STATUS,
        };
        let far = [exit[0] + 3.1, exit[1], exit[2]];
        let tops = [(extraction, Some(EXIT_TOP))];
        assert!(run.exits(&tops, &[player(far)], now).1.is_empty());
        let (change, out) = run.exits(&tops, &[player(exit)], now);
        assert_eq!(out, vec![entity]);
        let values = change.expect("count").values.expect("values").elements;
        assert!(values.contains(&(EXTRACTED_VALUE, 1)));
        assert!(run.exits(&tops, &[player(exit)], now).1.is_empty(), "once");
        run.end_checks(&[player(exit)], now);
        assert_eq!(
            run.end.end_due,
            Some((EndReason::Extracted, now + ALL_EXTRACTED_DELAY_MS))
        );
        assert_eq!(run.end_change(now, [618, 1]), None, "not yet");
        now += ALL_EXTRACTED_DELAY_MS;
        let (reason, change, payouts) = run.end_change(now, [618, 1]).expect("the end");
        assert_eq!(reason, EndReason::Extracted);
        assert_eq!(payouts, vec![(0, 741), (1, 2)]);
        assert_eq!(change.script_state, Some(RUN_ENDED_STATE));
        let values = change.values.expect("values").elements;
        for value in [(PARTICIPANTS_VALUE, 1), (0xc946_10a6, 741), (0xd638_d8aa, 2)] {
            assert!(values.contains(&value), "{values:x?}");
        }
        let timers = change.timers.expect("timers").elements;
        assert!(timers.contains(&(VICTORY_TIMER, now + VICTORY_DELAY_MS)));
        assert_eq!(run.victory_change(now), None);
        now += VICTORY_DELAY_MS;
        let victory = run.victory_change(now).expect("the victory");
        assert!(victory.values.expect("values").elements.contains(&(THIRD_RUN_VALUE, 1)));
        let timers = victory.timers.expect("timers").elements;
        assert!(!timers.iter().any(|(key, _)| *key == VICTORY_TIMER));
        assert!(timers.contains(&(LEAVE_TIMER, now + LEAVE_DELAY_MS)));
    }

    /// Retail `CATACOMBS` 849.920 s and 947.915 s: a death takes a respawn (the last one too);
    /// once every participant is dead and a death found no respawn left the run ends 0.5 s later
    /// through state 2 with the pity payout.
    #[test]
    fn every_participant_dead_with_no_respawn_left_ends_the_run_with_the_pity() {
        let (mut world, _, _) = started_run();
        let run = world.catacomb_run_mut().expect("run");
        let entity = run.progress.participants[0].entity;
        let seen = |status| {
            [Seen {
                entity,
                number: PLAYER,
                at: [0.0; 3],
                status,
            }]
        };
        let mut now = START_MS + 60_000;
        for left in [2, 1, 0] {
            let change = run.status_changes(&seen(DEAD_STATUS)).expect("a death");
            assert!(change.values.expect("values").elements.contains(&(RESPAWNS_REMAINING_VALUE, left)));
            assert!(!run.end.out_of_respawns);
            assert!(run.status_changes(&seen(PLAYING_STATUS)).is_some(), "respawned");
        }
        assert_eq!(run.status_changes(&seen(PLAYING_STATUS)), None, "no change");
        run.status_changes(&seen(DEATHS_DOOR_STATUS)).expect("downed");
        run.status_changes(&seen(DEAD_STATUS)).expect("dead");
        assert!(run.end.out_of_respawns);
        run.end_checks(&seen(DEAD_STATUS), now);
        assert_eq!(
            run.end.end_due,
            Some((EndReason::NoRespawnPoints, now + ALL_DEAD_DELAY_MS))
        );
        now += ALL_DEAD_DELAY_MS;
        let (_, change, payouts) = run.end_change(now, [224, 0]).expect("the end");
        assert_eq!(payouts, vec![(0, 44), (1, 0)]);
        assert!(
            change
                .timers
                .expect("timers")
                .elements
                .iter()
                .any(|(key, _)| *key == EXTRACTION_TIMER),
            "through state 2"
        );
    }

    /// Retail `CATACOMBS3` 1017.017 s: the extraction timer's end ends the run; with nobody out
    /// it is `NO_PLAYERS_EXTRACTED`; `catacombstimer` ends a run with no extraction.
    #[test]
    fn the_extraction_timer_and_the_run_timer_end_the_run() {
        let (mut world, _, _) = started_run();
        let run = world.catacomb_run_mut().expect("run");
        let mut timed = run.clone();
        let seen: [Seen; 0] = [];
        timed.end.extraction_ends_ms = Some(START_MS + 10);
        timed.end_checks(&seen, START_MS + 10);
        assert_eq!(
            timed.end.end_due,
            Some((EndReason::NoPlayersExtracted, START_MS + 10))
        );
        timed.end.end_due = None;
        let mut other = timed.progress.participants[0].clone();
        other.number = PLAYER + 1;
        timed.progress.participants.push(other);
        timed.end.extracted.push(PLAYER + 1);
        timed.end_checks(&seen, START_MS + 10);
        assert_eq!(timed.end.end_due.map(|(reason, _)| reason), Some(EndReason::ExtractionOver));
        let ends = START_MS + super::super::RUN_DURATION_MS;
        run.end_checks(&seen, ends - 1);
        assert_eq!(run.end.end_due, None);
        run.end_checks(&seen, ends);
        assert_eq!(run.end.end_due, Some((EndReason::TimeOut, ends)));
    }

    /// Retail `CATACOMBS5` 410.637 s: 300 s after the start the start tile goes to 3 and the
    /// start room's extraction opens.
    #[test]
    fn the_start_tile_closes_300_s_after_the_start_and_opens_its_extraction() {
        let (mut world, _, _) = started_run();
        let run = world.catacomb_run_mut().expect("run");
        let start = run
            .layout
            .rooms
            .iter()
            .position(|room| room.room_type == START_ROOM)
            .expect("start room");
        assert_eq!(run.start_close(START_MS + START_CLOSE_MS - 1), None);
        let (tile, change) = run.start_close(START_MS + START_CLOSE_MS).expect("closed");
        assert_eq!(Some(tile), run.rooms[start].tile);
        assert_eq!(change.script_state, Some(START_CLOSED_STATE));
        assert!(
            run.end
                .extractions
                .iter()
                .filter(|extraction| extraction.room == start)
                .all(|extraction| extraction.open)
        );
        assert_eq!(run.start_close(START_MS + START_CLOSE_MS + 1), None, "once");
    }

    /// Retail `CATACOMBS5` 1700.676 to 1700.783 s: a player who leaves gets the left
    /// participant state and goes out of the run's lists; an extracted one gets the cutscene's
    /// stop first.
    #[test]
    fn a_leaving_player_leaves_the_lists_with_the_left_state() {
        let (mut world, _, _) = started_run();
        let entity = world.player_by_number(PLAYER).expect("player");
        stand_in_room(&mut world, 0);
        if let Some(run) = world.catacomb_run_mut() {
            run.end.extracted.push(PLAYER);
        }
        world.take_messages();
        world.leave_catacombs(entity);
        let run = world.catacomb_run().expect("run");
        assert!(run.progress.participants.is_empty());
        let (mode_entity, _) = run.progress.entity.expect("run entity");
        let held = held_state(&world, mode_entity);
        assert_eq!(
            held.statuses
                .and_then(|list| match list {
                    ReplicatedList::Full(full) => Some(full.elements),
                    ReplicatedList::Changes(_) => None,
                }),
            Some(Vec::new())
        );
        let stops = world
            .take_messages()
            .iter()
            .filter(|out| out.connection == crate::world::tests::connection(PLAYER))
            .flat_map(|out| &out.envelopes)
            .filter_map(|bytes| nw_reflect::Envelope::decode(bytes).ok())
            .filter(|envelope| {
                envelope.type_index()
                    == Some(nw_messages::game_mode::STOP_CUTSCENE_WITH_NAME_TYPE_INDEX)
            })
            .count();
        assert_eq!(stops, 1);
    }

    /// The retail content (build 25810808): the target of every boss tile extraction's
    /// `Activate core` stage (`SliceDestroyed`) and both cursed silver chests are gatherable
    /// start objects of the table, as retail sent them (`CATACOMBS5` 1577.088 s: gatherable
    /// controller 4).
    #[test]
    #[ignore = "needs the build 25810808 content files in NW_CONTENT_DIR"]
    fn the_extraction_core_and_the_chests_are_gatherable_start_objects_of_the_content() {
        use nw_world::dungeon_scripts::ObjectiveSpec;

        const BUILD: &str = "25810808";
        const CORE_STAGE: &str = "Wave_01 - Objective: Activate core";
        let dir = std::path::PathBuf::from(std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR"));
        let table = nw_content::town_props::TownProps::load_file(
            nw_content::town_props::encounter_objects_path(&dir, BUILD),
        )
        .expect("objects table");
        let mut objects = super::super::super::encounter_roots::EncounterObjects::build(&table);
        let tiles = nw_world::catacomb_tiles::CatacombTiles::load(
            &nw_world::catacomb_tiles::catacomb_tiles_path(&dir, BUILD),
        )
        .expect("tiles");
        let mut cores = Vec::new();
        for tile in 1..=5 {
            let variation = format!("catacombs/gameplay/tiles/tile{tile}_a_gameplay");
            let encounter = &tiles
                .get(&variation)
                .expect("boss tile")
                .encounters
                .iter()
                .find(|entry| entry.encounter.name == EXTRACTION_SLOTS[0])
                .expect("its extraction")
                .encounter;
            let stage = encounter
                .stages
                .iter()
                .find(|stage| stage.name == CORE_STAGE)
                .expect("the core stage");
            let ObjectiveSpec::SliceDestroyed {
                target: Some(target),
                ..
            } = stage.objectives[0].kind
            else {
                panic!("the core is destroyed");
            };
            cores.extend(
                encounter.targets[target]
                    .slices
                    .iter()
                    .map(|slice| slice.path.clone()),
            );
        }
        assert_eq!(cores.len(), 5);
        let chests = super::super::pool::CHESTS.map(|(slice, _)| slice.to_owned());
        for slice in cores.iter().chain(&chests) {
            let number = objects.spawn(slice, [0.0; 3], 0.0).expect(slice);
            let group = objects
                .group(
                    number,
                    super::super::super::slots::Slot { interest: 1, new: true },
                    &mut std::collections::HashMap::new(),
                )
                .expect("group");
            assert!(
                group.members.iter().any(|member| matches!(
                    member.state,
                    nw_messages::MemberState::GatherableController(
                        nw_messages::GatherableControllerState {
                            gatherable_state: Some(4)
                        }
                    )
                )),
                "{slice}"
            );
        }
    }
}
