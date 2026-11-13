// SPDX-License-Identifier: AGPL-3.0-or-later
//! The world's day and night: one time of day for every sim of a world, from the world clock
//! anchor (world migration 0035, [`nw_db::world_clock_start`]), on the retail cycle
//! ([`nw_messages::DayCycle`]: an hour of day and half an hour of night, Observed in 607 retail
//! ledgers).
//!
//! What the client gets (Observed, ledger `01-starting-zone/12-1_getoffbeach-Soul-Survivor`):
//! the client asks for the time (4248, at its spawn and again later, 1790514473110 ms) and gets
//! `SyncTimeChange` (4246) with the anchor, then `SyncDayPhase` (4247) with the phase, both on
//! its time component; at each change of the phase it gets `SyncDayPhase` alone on that
//! component (1790513972453 ms, 3,575.6 s into the cycle). The client draws its sky and light
//! from the anchor of 4246 (its environment reader `FUN_146bda230` reads `DawnTime`,
//! `DawnDuration`, `DuskTime` and `DuskDuration`, the first four `u32`s of 4246: 21,300, 0,
//! 65,100 and 0).
//!
//! What the server reads from it: the bait of a cast and the 18 fishing perks whose `DayPhases`
//! cell is `Day` or `Night` (`FishLuckDay1` to `FishCatchSizeNight3`, [`super::fishing`]). No
//! behaviour tree reads the AI key `Spawn_Request_TimeOfDay` (Observed: 0 of the Server.pak
//! trees).
//!
//! The spawns, gatherable points and encounter slices with a time of day ([`TimedPlaces`], from
//! the world file's [`nw_world::time_of_day`] points) exist only in the phases their flags name:
//! - a mob of such a spawn goes when its time ends and a new one spawns when its time comes
//!   (Observed, ledger `Playthrough_Part_18`: at the night's `SyncDayPhase`, 2,040,836 ms, the
//!   mobs of day-only spawners sent their last groups 15 to 284 ms before it and the mobs of
//!   night-only spawners came 18 and 35 ms after it; over the 28 retail ledgers with such mobs,
//!   411 of the 414 mob lives seen wholly in one known phase fell in their spawner's phase, and
//!   why the other 3 stood out of it is Unknown);
//! - an open world encounter whose slice has a time of day starts only in it and resets (its mobs
//!   go) when the time ends (Inferred from `m_despawnOnTimeEnd`: no retail ledger holds such an
//!   encounter at a phase change);
//! - a gatherable point with a time of day takes no gather out of it. The client draws these
//!   nodes itself from its level files (its `TimeOfDayConstraintComponentClientFacet`,
//!   `FUN_143c60380`) and retail cards a node only after a gather ([`super::gathering`]), so
//!   nothing goes to the client at the change (Inferred: the server refuses a gather of a node
//!   the client should not show);
//! - the props and spawners with a time of day that the client loads with its level and the
//!   server does not run (pirate flags, glyph chest force fields, the lucky rabbit spawner of
//!   the shrines) need nothing from the server.
//!
//! Retail's dawn and dusk last no time (`SyncTimeChange`'s `DawnDuration` and `DuskDuration` are 0,
//! read by `FUN_146bda230`) and retail sends only the night and the day phase, so a time of day
//! is in force by its `m_night` at night and by its `m_day` by day; `m_dawn` and `m_dusk` are
//! carried and never in force.
//!
//! Every sim reads the anchor again each [`CLOCK_READ_MS`] (Player's design choice: the
//! interval of the claim owners), so a game master's `!time` on one sim reaches the others. A
//! changed anchor goes to each client as the answer to its time request does, 4246 then 4247
//! (Inferred: no retail capture holds a time change by a game master).

use std::collections::HashMap;
use std::sync::Arc;

use nw_bus::ConnectionRef;
use nw_messages::{DAY_PHASE, DayCycle, FacetHeader, ServerMessage};
use nw_world::records::{Position, VitalsSpawn, WorldFile};
use nw_world::time_of_day::TimeOfDay;
use sqlx::PgPool;

use super::{Player, SimWorld, send_now};
use crate::login::{WorldIds, time_answers};

/// How often a sim reads the world clock anchor again, ms (Player's design choice).
pub(super) const CLOCK_READ_MS: u64 = 30_000;
/// Game seconds per game minute and per game hour (`!time`).
const GAME_MINUTE_S: u64 = 60;
const GAME_HOUR_S: u64 = 3_600;
/// The names of the phases in `!time` (the perk table's `DayPhases` names, Observed: `Day` and
/// `Night`).
const NIGHT_NAME: &str = "Night";
const DAY_NAME: &str = "Day";

/// The day phase of the world at server time `server_ms` (ms since 2000-01-01).
pub(super) fn phase_now(ids: &WorldIds, server_ms: u64) -> u32 {
    DayCycle::retail().phase(ids.time_change_time, server_ms)
}

/// The time of day of the sim level's spawns, gatherable points and slices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimedPlaces {
    /// The spawns' times, by the bits of their position (a sim's spawn is a copy of its level
    /// record, so the bits are the same).
    spawns: HashMap<[u32; 3], TimeOfDay>,
    /// The gatherable points' times, by index in the level's list.
    gather_points: HashMap<usize, TimeOfDay>,
    /// The slices' times, by pak path (lower case, as the dungeon scripts name them).
    slices: HashMap<String, TimeOfDay>,
}

/// The key of a spawn's position.
fn position_key(position: Position) -> [u32; 3] {
    position.map(f32::to_bits)
}

/// Whether a time of day is in force in day phase `phase` (see the module text).
fn in_force(window: TimeOfDay, phase: u32) -> bool {
    if phase == DAY_PHASE {
        window.day
    } else {
        window.night
    }
}

/// Whether a thing with time of day `window` (none: always) exists in phase `phase`; `live`
/// when it exists now, so that one whose time ends without `m_despawnOnTimeEnd` stays.
fn holds(window: Option<&TimeOfDay>, phase: u32, live: bool) -> bool {
    window.is_none_or(|window| in_force(*window, phase) || (live && !window.despawn_on_end))
}

impl TimedPlaces {
    /// The times of the level `level`.
    pub fn of(level: &WorldFile) -> Self {
        let points = &level.time_of_day;
        Self {
            spawns: points
                .vitals
                .iter()
                .filter_map(|timed| {
                    let spawn = level.vitals.get(usize::try_from(timed.index).ok()?)?;
                    Some((position_key(spawn.position), timed.window))
                })
                .collect(),
            gather_points: points
                .gatherables
                .iter()
                .filter_map(|timed| Some((usize::try_from(timed.index).ok()?, timed.window)))
                .collect(),
            slices: points
                .slices
                .iter()
                .map(|timed| (timed.slice.clone(), timed.window))
                .collect(),
        }
    }

    /// Whether the mob of `spawn` exists in phase `phase` (`live`: it exists now).
    pub fn spawn_holds(&self, spawn: &VitalsSpawn, phase: u32, live: bool) -> bool {
        holds(self.spawns.get(&position_key(spawn.position)), phase, live)
    }

    /// Whether the gatherable point of index `point` takes a gather in phase `phase`.
    pub fn gather_point_shows(&self, point: usize, phase: u32) -> bool {
        holds(self.gather_points.get(&point), phase, false)
    }

    /// Whether an encounter of slice `slice` runs in phase `phase` (`live`: it runs now).
    pub fn slice_holds(&self, slice: &str, phase: u32, live: bool) -> bool {
        holds(self.slices.get(slice), phase, live)
    }
}

/// The anchor reads and sets of one sim.
#[derive(Debug, Default)]
pub(crate) struct WorldClock {
    /// The phase in which the timed spawns were last held; `None` when they must be held again.
    spawns_held: Option<u32>,
    /// When the next read is due, sim ms.
    next_read_ms: u64,
    /// A job runs.
    running: bool,
    /// A game master's set that waits for its job, with who asked.
    asked: Option<(u64, ConnectionRef)>,
}

/// A world clock job for the server loop.
#[derive(Debug)]
pub(crate) enum ClockJob {
    /// Read the anchor; `current_ns` is kept when the table is empty.
    Read { current_ns: u64 },
    /// Store a game master's anchor, then read it.
    Set {
        anchor_ns: u64,
        asker: ConnectionRef,
    },
}

/// The result of a [`ClockJob`].
#[derive(Debug)]
pub(crate) struct ClockDone {
    anchor_ns: Result<u64, nw_db::DbError>,
    asker: Option<ConnectionRef>,
}

impl ClockJob {
    /// Runs the job against the world database.
    pub(crate) async fn run(self, world: &PgPool) -> ClockDone {
        match self {
            Self::Read { current_ns } => ClockDone {
                anchor_ns: nw_db::world_clock_start(world, current_ns).await,
                asker: None,
            },
            Self::Set { anchor_ns, asker } => ClockDone {
                anchor_ns: nw_db::set_world_clock(world, anchor_ns)
                    .await
                    .map(|()| anchor_ns),
                asker: Some(asker),
            },
        }
    }
}

impl SimWorld {
    /// The world's day phase now.
    pub(super) fn day_phase(&self) -> u32 {
        phase_now(&self.ids, self.server_ms)
    }

    /// Takes out the mobs of the spawns whose time of day ended and spawns those whose time came
    /// ([`TimedPlaces`]), once per phase and again after the mob cells changed; at server time
    /// `server_ms`, before the mobs think.
    pub(super) fn hold_timed_spawns(&mut self, server_ms: u64) {
        let phase = phase_now(&self.ids, server_ms);
        if self.world_clock.spawns_held == Some(phase) {
            return;
        }
        self.world_clock.spawns_held = Some(phase);
        let content = Arc::clone(&self.content);
        self.mobs
            .hold_time_of_day(|spawn, live| content.time_of_day.spawn_holds(spawn, phase, live));
    }

    /// The mob cells changed: the next tick holds the timed spawns again, since the new cells
    /// spawned every one of their spawns.
    pub(super) fn recheck_timed_spawns(&mut self) {
        self.world_clock.spawns_held = None;
    }

    /// The answer to a client's time request on its time component `header`: the anchor and
    /// the phase now; the client's component is kept for the phase changes.
    pub(super) fn time_request_answers(
        ids: &WorldIds,
        player: &mut Player,
        header: FacetHeader,
        server_ms: u64,
    ) -> Vec<ServerMessage> {
        let phase = phase_now(ids, server_ms);
        player.day_phase = Some((header, Some(phase)));
        time_answers(ids, header, phase).to_vec()
    }

    /// Sends `SyncDayPhase` alone to each client whose phase changed, as retail does at each
    /// change, and to a client that came by a handoff (its first one on this sim).
    pub(super) fn tick_day_phase(&mut self) {
        let phase = self.day_phase();
        let mut query = self.world.query::<&mut Player>();
        for mut player in query.iter_mut(&mut self.world) {
            let Some((header, sent)) = player.day_phase else {
                continue;
            };
            if sent == Some(phase) {
                continue;
            }
            player.day_phase = Some((header, Some(phase)));
            let [_, change] = time_answers(&self.ids, header, phase);
            send_now(&mut self.combat_messages, &player.connection, &[change]);
        }
    }

    /// Puts a new anchor in force: each client with a time component gets the anchor and the
    /// phase again.
    fn set_anchor(&mut self, anchor_ns: u64) {
        if anchor_ns == self.ids.time_change_time {
            return;
        }
        tracing::info!(anchor_ns, "world clock anchor changed");
        self.ids.time_change_time = anchor_ns;
        let phase = self.day_phase();
        let mut query = self.world.query::<&mut Player>();
        for mut player in query.iter_mut(&mut self.world) {
            let Some((header, _)) = player.day_phase else {
                continue;
            };
            player.day_phase = Some((header, Some(phase)));
            send_now(
                &mut self.combat_messages,
                &player.connection,
                &time_answers(&self.ids, header, phase),
            );
        }
    }

    /// The world clock jobs due at sim ms `now_ms`: a waiting set, else a read each
    /// [`CLOCK_READ_MS`].
    pub(crate) fn take_clock_jobs(&mut self, now_ms: u64) -> Option<ClockJob> {
        if self.world_clock.running {
            return None;
        }
        let job = if let Some((anchor_ns, asker)) = self.world_clock.asked.take() {
            ClockJob::Set { anchor_ns, asker }
        } else if now_ms >= self.world_clock.next_read_ms {
            ClockJob::Read {
                current_ns: self.ids.time_change_time,
            }
        } else {
            return None;
        };
        self.world_clock.running = true;
        self.world_clock.next_read_ms = now_ms.saturating_add(CLOCK_READ_MS);
        Some(job)
    }

    /// Takes the result of a world clock job.
    pub(crate) fn clock_done(&mut self, done: ClockDone) {
        self.world_clock.running = false;
        match done.anchor_ns {
            Ok(anchor_ns) => {
                self.set_anchor(anchor_ns);
                if let Some(asker) = done.asker {
                    let line = self.time_line();
                    self.answer_gm_chat(&asker, vec![line]);
                }
            }
            Err(error) => {
                tracing::error!(%error, "world clock not read or set");
                if let Some(asker) = done.asker {
                    self.answer_gm_chat(&asker, vec![error.to_string()]);
                }
            }
        }
    }

    /// A world clock job whose task failed: the next read comes at its time.
    pub(crate) fn clock_job_lost(&mut self) {
        self.world_clock.running = false;
    }

    /// The `!time` line: the game time of day and the phase.
    pub(super) fn time_line(&self) -> String {
        let second = DayCycle::retail().game_second(self.ids.time_change_time, self.server_ms);
        let phase = if self.day_phase() == DAY_PHASE {
            DAY_NAME
        } else {
            NIGHT_NAME
        };
        format!(
            "{:02}:{:02} {phase}",
            second / GAME_HOUR_S,
            second % GAME_HOUR_S / GAME_MINUTE_S
        )
    }

    /// A game master sets the time of day to `hour:minute`; the answer comes once the world
    /// database took it.
    pub(super) fn ask_time(&mut self, hour: u64, minute: u64, asker: ConnectionRef) {
        let game_s = hour * GAME_HOUR_S + minute * GAME_MINUTE_S;
        let anchor_ns = DayCycle::retail().anchor_for(game_s, self.server_ms);
        self.world_clock.asked = Some((anchor_ns, asker));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use nw_messages::NIGHT_PHASE;
    use nw_messages::facet::{ClientFacetMsg, component};
    use nw_reflect::Envelope;
    use nw_world::time_of_day::{TimeOfDayPoints, TimedPoint, TimedSlicePath};

    use super::*;
    use crate::mobs::mob_entity;
    use crate::world::tests::{
        NOW, SERVER_TIME, admit_at, connection, table, test_spawn, world_with_spawns,
    };

    const PLAYER: u64 = 42;
    /// The anchor of the test world, ns since 2000-01-01.
    const ANCHOR_NS: u64 = 0;
    /// A thousand cycles of 5,400 s after the anchor, ms.
    const CYCLES_MS: u64 = 5_400_000_000;
    /// Server ms 1,000 s into a cycle (day) and 3,700 s into it (night: the night starts
    /// 3,575.34 s after the anchor), for a world whose anchor is 0.
    pub(crate) const DAY_MS: u64 = CYCLES_MS + 1_000_000;
    pub(crate) const NIGHT_MS: u64 = CYCLES_MS + 3_700_000;
    /// The retail flags of a night-only spawner (`m_night` and `m_despawnOnTimeEnd`).
    pub(crate) const NIGHT_ONLY: TimeOfDay = TimeOfDay {
        night: true,
        dawn: false,
        day: false,
        dusk: false,
        despawn_on_end: true,
    };
    /// A day-only time of day that does not go when its time ends.
    const DAY_STAYS: TimeOfDay = TimeOfDay {
        night: false,
        dawn: true,
        day: true,
        dusk: true,
        despawn_on_end: false,
    };
    const MOB_A_X: f32 = 20.0;
    const MOB_B_X: f32 = 40.0;

    fn world_at(server_ms: u64) -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("player");
        world.ids.time_change_time = ANCHOR_NS;
        world.server_ms = server_ms;
        world.take_messages();
        world
    }

    /// The time messages queued for the player.
    fn time_messages(world: &mut SimWorld) -> Vec<ServerMessage> {
        world
            .take_messages()
            .into_iter()
            .filter(|outgoing| outgoing.connection == connection(PLAYER))
            .flat_map(|outgoing| outgoing.envelopes)
            .filter_map(|bytes| {
                let envelope = Envelope::decode(&bytes).expect("envelope");
                match ServerMessage::from_envelope(&envelope) {
                    Some(Ok(
                        message @ (ServerMessage::TimeChange(_) | ServerMessage::DayPhase(_)),
                    )) => Some(message),
                    _ => None,
                }
            })
            .collect()
    }

    fn ask_time(world: &mut SimWorld) {
        let header = FacetHeader::of(component::TIME, PLAYER);
        world
            .facet_message(
                &connection(PLAYER),
                ClientFacetMsg::TimeSyncRequest { header },
                NOW,
            )
            .expect("answer");
    }

    #[test]
    fn the_time_answer_carries_the_phase_and_a_change_sends_the_phase_alone() {
        let mut world = world_at(DAY_MS);
        ask_time(&mut world);
        let answer = time_messages(&mut world);
        assert!(
            matches!(answer.as_slice(), [ServerMessage::TimeChange(_), ServerMessage::DayPhase(phase)] if phase.u32_at_16 == DAY_PHASE),
            "{answer:?}"
        );
        world.tick_day_phase();
        assert!(time_messages(&mut world).is_empty(), "no change, nothing");
        world.server_ms = NIGHT_MS;
        world.tick_day_phase();
        let change = time_messages(&mut world);
        assert!(
            matches!(change.as_slice(), [ServerMessage::DayPhase(phase)] if phase.u32_at_16 == NIGHT_PHASE),
            "{change:?}"
        );
    }

    #[test]
    fn a_game_master_s_time_is_stored_then_sent_to_every_client_again() {
        const MIDNIGHT: (u64, u64) = (0, 0);
        let mut world = world_at(DAY_MS);
        ask_time(&mut world);
        time_messages(&mut world);
        world.ask_time(MIDNIGHT.0, MIDNIGHT.1, connection(PLAYER));
        let Some(ClockJob::Set { anchor_ns, asker }) = world.take_clock_jobs(NOW) else {
            panic!("the set goes first");
        };
        assert!(world.take_clock_jobs(NOW).is_none(), "one job at a time");
        world.clock_done(ClockDone {
            anchor_ns: Ok(anchor_ns),
            asker: Some(asker),
        });
        assert_eq!(world.ids.time_change_time, anchor_ns);
        let again = time_messages(&mut world);
        assert!(
            matches!(again.as_slice(), [ServerMessage::TimeChange(change), ServerMessage::DayPhase(phase)] if change.time == anchor_ns && phase.u32_at_16 == NIGHT_PHASE),
            "{again:?}"
        );
        assert!(
            world.time_line().starts_with("00:0"),
            "{}",
            world.time_line()
        );
    }

    #[test]
    fn a_client_that_comes_by_a_handoff_gets_the_phase_on_its_time_component() {
        let mut world = world_with_spawns(Vec::new());
        world
            .admit(&admit_at(PLAYER, 0.0, false), NOW, SERVER_TIME)
            .expect("handoff");
        world.ids.time_change_time = ANCHOR_NS;
        world.server_ms = NIGHT_MS;
        world.take_messages();
        world.tick_day_phase();
        let sent = time_messages(&mut world);
        let time = FacetHeader::of(component::TIME, PLAYER);
        assert!(
            matches!(sent.as_slice(), [ServerMessage::DayPhase(phase)] if (phase.component, phase.u64_at_8, phase.u32_at_16) == (time.target, time.entity, NIGHT_PHASE)),
            "{sent:?}"
        );
        world.tick_day_phase();
        assert!(time_messages(&mut world).is_empty(), "once");
    }

    #[test]
    fn the_anchor_is_read_again_each_interval() {
        let mut world = world_at(DAY_MS);
        assert!(matches!(
            world.take_clock_jobs(NOW),
            Some(ClockJob::Read { .. })
        ));
        world.clock_job_lost();
        assert!(
            world.take_clock_jobs(NOW).is_none(),
            "not before the interval"
        );
        assert!(world.take_clock_jobs(NOW + CLOCK_READ_MS).is_some());
    }

    /// An encounter slice that runs by day and does not go when its time ends.
    const ENCOUNTER: &str = "slices/worldevents/x/enc_worldevent_x.dynamicslice";

    /// A level whose second spawn and first gatherable point exist only at night, with the
    /// encounter slice [`ENCOUNTER`].
    fn timed_level() -> WorldFile {
        WorldFile {
            vitals: vec![test_spawn(MOB_A_X), test_spawn(MOB_B_X)],
            time_of_day: TimeOfDayPoints {
                vitals: vec![TimedPoint {
                    index: 1,
                    window: NIGHT_ONLY,
                }],
                gatherables: vec![TimedPoint {
                    index: 0,
                    window: NIGHT_ONLY,
                }],
                slices: vec![TimedSlicePath {
                    slice: ENCOUNTER.to_owned(),
                    window: DAY_STAYS,
                }],
            },
            ..WorldFile::default()
        }
    }

    #[test]
    fn a_time_of_day_is_in_force_by_its_night_or_day_flag() {
        let places = TimedPlaces::of(&timed_level());
        let [always, night_only] = [test_spawn(MOB_A_X), test_spawn(MOB_B_X)];
        assert!(places.spawn_holds(&always, DAY_PHASE, false));
        assert!(places.spawn_holds(&always, NIGHT_PHASE, true));
        assert!(!places.spawn_holds(&night_only, DAY_PHASE, false));
        assert!(
            !places.spawn_holds(&night_only, DAY_PHASE, true),
            "it goes at its end"
        );
        assert!(places.spawn_holds(&night_only, NIGHT_PHASE, false));
        assert!(!places.gather_point_shows(0, DAY_PHASE));
        assert!(places.gather_point_shows(0, NIGHT_PHASE));
        assert!(
            places.gather_point_shows(1, DAY_PHASE),
            "a point without a time"
        );
        assert!(
            !places.slice_holds(ENCOUNTER, NIGHT_PHASE, false),
            "it does not start"
        );
        assert!(
            places.slice_holds(ENCOUNTER, NIGHT_PHASE, true),
            "without m_despawnOnTimeEnd it runs on"
        );
        assert!(places.slice_holds("slices/other.dynamicslice", NIGHT_PHASE, false));
    }

    #[test]
    fn a_night_spawns_mob_goes_by_day_and_comes_back_as_a_new_spawn_at_night() {
        let mut world = world_with_spawns(timed_level().vitals);
        let mut content = (*world.content).clone();
        content.time_of_day = TimedPlaces::of(&timed_level());
        world.content = Arc::new(content);
        world.ids.time_change_time = ANCHOR_NS;
        world.set_table(&table(&["sim-a"])).expect("table");
        let (always, night_only) = (mob_entity(0), mob_entity(1));
        world.hold_timed_spawns(DAY_MS);
        assert_eq!(world.mobs.live_entities(), [always]);
        assert_eq!(world.mobs.take_left(), [night_only]);
        world.hold_timed_spawns(DAY_MS);
        assert!(world.mobs.take_left().is_empty(), "once per phase");
        world.hold_timed_spawns(NIGHT_MS);
        let mut live = world.mobs.live_entities();
        live.sort_unstable();
        assert_eq!(live, [always, night_only]);
        assert_eq!(
            world.mobs.take_respawned(),
            [night_only],
            "a new spawn, new card"
        );
        world.recheck_timed_spawns();
        world.hold_timed_spawns(NIGHT_MS);
        assert!(world.mobs.take_respawned().is_empty(), "already there");
        world.hold_timed_spawns(DAY_MS + CYCLES_MS);
        assert_eq!(world.mobs.take_left(), [night_only]);
    }
}
