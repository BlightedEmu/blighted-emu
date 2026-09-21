// SPDX-License-Identifier: AGPL-3.0-or-later
//! The slayer state of a player (slayer note, 2026-09-18; movement study 2026-09-20): the action
//! it plays, which the client and the other players see in the ALC children 13 to 18, and the time
//! into that action's sequence (children 2 and 3). The client starts its actions itself and names
//! each one in row 1 of its input ([`SlayerInput`]); the server echoes it in the next own state,
//! started in the state numbered with that record's counter, then adds its own follow-up for some
//! actions. In the retail newchar capture 73 changes came from the client and 481 from the server:
//! the follow-ups ([`FOLLOW_UPS`]) and the movement states ([`Locomotion`]).
//!
//! The sequence time is the sum of the input records' frame times since the record that started
//! the state, that record's own included ([`SequenceClock`] has the wire form).

use nw_messages::{SecondSlayerState, SequenceClock, SequenceTime, SlayerInput, SlayerState};
use nw_sim_core::Intent;

/// How many own states carry one slayer change (Observed: retail sent a follow-up in the state
/// that started it and the next one, 15c1 and 15c2 at 206528 to 206577 ms).
const ECHO_STATES: u8 = 2;
/// The follow-up the server starts one own state after an action the client started: (the
/// client's sequence, the server's). Observed in the retail newchar capture, each pair every time
/// it came (5 to 13 times), 0 to 132 ms after the client's action: a light attack's sequences
/// and their next part. Other sequences have no follow-up here (Unknown: more need captures of
/// the other weapons).
const FOLLOW_UPS: [(u64, u64); 7] = [
    (5047, 487),
    (5055, 4603),
    (5051, 484),
    (5056, 468),
    (5050, 485),
    (3120, 2030),
    (1221, 463),
];

/// The follow-up sequence of the client's sequence `sequence`.
fn follow_up_of(sequence: u64) -> Option<u64> {
    FOLLOW_UPS
        .iter()
        .find(|(started, _)| *started == sequence)
        .map(|(_, follow_up)| *follow_up)
}

/// The slayer states the server starts for the player's own movement, with the sequence each
/// transition took most often in the retail newchar capture (the server's slayer changes against
/// the input keys of the records before and at them). The sequence of one transition varies
/// with what the capture does not show (the foot in front, the turn of the camera); Unknown: the
/// client's slayer script that picks it.
mod movement {
    /// `slayerStateId` of a move that starts from standing still (`13=1a`).
    pub(super) const START_STATE: u64 = 0x1a;
    /// `slayerStateId` of the moves that follow and of a stop (`13=1b`).
    pub(super) const MOVE_STATE: u64 = 0x1b;
    /// `slayerStateId` of standing still (`13=1f`, the first own state's).
    pub(super) const IDLE_STATE: u64 = 0x1f;
    /// Standing still, a key forward or to the side: `aa2b`, 29 of the 57 forward starts and 11
    /// of the 21 side starts.
    pub(super) const START_SEQUENCE: u64 = 2794;
    /// Standing still, the back key: state `1b` with `9814`, 6 of the 16 back starts.
    pub(super) const BACK_START_SEQUENCE: u64 = 1304;
    /// Moving, the keys let go: `bd15`, 29 of the 96 stops of a forward move.
    pub(super) const STOP_SEQUENCE: u64 = 1405;
    /// Stopping, a key again: `b625`, 21 of the 51 forward presses during a stop.
    pub(super) const RESTART_SEQUENCE: u64 = 2422;
    /// Stopped: `b910`, 34 of the 106 idle states after a stop.
    pub(super) const IDLE_SEQUENCE: u64 = 1081;
    /// Input records from the stop to the idle state: the length of the stop sequence (9 of the
    /// 14 `bd15` stops that ended in an idle state did after 39 records).
    pub(super) const STOP_RECORDS: u16 = 39;
}

/// Where the player's own movement is, for its movement slayer states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Locomotion {
    /// Standing still (the first own state's idle state).
    #[default]
    Idle,
    /// Moving by its keys.
    Moving,
    /// Its keys let go: records left until it stands still.
    Stopping(u16),
}

impl Locomotion {
    /// The next movement after a record with `intent`, and the slayer state and sequence the
    /// server starts in that record, when it starts one.
    fn step(self, intent: Intent) -> (Self, Option<(u64, u64)>) {
        let moving = intent.forward != 0 || intent.right != 0;
        match (self, moving) {
            (Self::Idle, true) => {
                let start = if intent.forward < 0 {
                    (movement::MOVE_STATE, movement::BACK_START_SEQUENCE)
                } else {
                    (movement::START_STATE, movement::START_SEQUENCE)
                };
                (Self::Moving, Some(start))
            }
            (Self::Moving, false) => (
                Self::Stopping(movement::STOP_RECORDS),
                Some((movement::MOVE_STATE, movement::STOP_SEQUENCE)),
            ),
            (Self::Stopping(_), true) => (
                Self::Moving,
                Some((movement::MOVE_STATE, movement::RESTART_SEQUENCE)),
            ),
            (Self::Stopping(left), false) => match left.saturating_sub(1) {
                0 => (
                    Self::Idle,
                    Some((movement::IDLE_STATE, movement::IDLE_SEQUENCE)),
                ),
                left => (Self::Stopping(left), None),
            },
            (Self::Idle, false) | (Self::Moving, true) => (self, None),
        }
    }
}

/// A slayer state that started in an input record and that no own state carried yet.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Pending {
    /// An action the client started.
    Client(SlayerInput),
    /// A movement state the server started: its state and sequence.
    Movement(u64, u64),
}

/// One input record for the slayer state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SlayerRecord {
    /// The record's counter.
    pub(super) counter: u16,
    /// Its frame time, s.
    pub(super) frame_s: f32,
    /// The action its client starts in it.
    pub(super) action: Option<SlayerInput>,
    /// Its movement keys, when the server moves the body by them.
    pub(super) intent: Option<Intent>,
}

/// What one own state carries of the slayer state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct SlayerDue {
    /// The slayer state, when it changed or echoes.
    pub(super) state: Option<SlayerState>,
    /// The time into its sequence.
    pub(super) time: Option<SequenceTime>,
}

/// One player's slayer state.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SlayerTrack {
    /// A state that started in a record (its counter, and that record's frame time) and that no
    /// own state carried yet.
    pending: Option<(Pending, u16, f32)>,
    /// The follow-up that starts in the next own state.
    follow_up: Option<u64>,
    /// The player's own movement.
    locomotion: Locomotion,
    /// The newest slayer state, and how many more own states carry it.
    current: Option<SlayerState>,
    repeats: u8,
    /// Seconds into the newest state's sequence.
    sequence_s: f32,
    /// The frame time of the newest record.
    newest_frame_s: f32,
    /// The sequence time the client has; `None` before its first own state.
    clock: Option<SequenceClock>,
    /// What this tick's own state carries, until that state takes it.
    due: SlayerDue,
}

impl SlayerTrack {
    /// The player's client took its first own state ([`nw_messages::AlcState::spawn`]): its
    /// slots and sequence time start there, standing still.
    pub(super) fn spawned(&mut self) {
        *self = Self {
            sequence_s: SequenceClock::spawn_seconds(),
            clock: Some(SequenceClock::after_spawn()),
            ..Self::default()
        };
    }

    /// Reads one input record not read before (oldest first): its frame time goes on the
    /// sequence, an action its client starts goes out with the next own state, and so does a
    /// change of the player's movement.
    pub(super) fn record(&mut self, record: SlayerRecord) {
        self.newest_frame_s = record.frame_s;
        self.sequence_s += record.frame_s;
        let movement = record.intent.and_then(|intent| {
            let (next, started) = self.locomotion.step(intent);
            self.locomotion = next;
            started
        });
        let started = record
            .action
            .map(Pending::Client)
            .or_else(|| movement.map(|(state, sequence)| Pending::Movement(state, sequence)));
        if let Some(started) = started {
            self.pending = Some((started, record.counter, record.frame_s));
            self.sequence_s = record.frame_s;
            self.follow_up = None;
        }
    }

    /// Moves on to the tick whose own state is numbered `number` at state time `state_ms`: its
    /// slayer state is a newly started one, else the server's follow-up, else the last change once
    /// more ([`ECHO_STATES`]); its sequence time goes with every own state. The players who see
    /// this one get the newest state at once ([`SlayerTrack::current`]).
    pub(super) fn advance(&mut self, number: u16, state_ms: u64) {
        let (state, started) = self.next(number);
        let sequence_s = self.sequence_s;
        let time = self
            .clock
            .as_mut()
            .map(|clock| clock.time(sequence_s, started, state_ms));
        self.due = SlayerDue { state, time };
    }

    /// What this tick's own state carries, once.
    pub(super) fn take_due(&mut self) -> SlayerDue {
        std::mem::take(&mut self.due)
    }

    /// The slayer state of the own state numbered `number`, and the frame time of the record that
    /// started it when it starts there.
    fn next(&mut self, number: u16) -> (Option<SlayerState>, Option<f32>) {
        if let Some((pending, counter, frame_s)) = self.pending.take() {
            let state = match pending {
                Pending::Client(input) => {
                    self.follow_up = follow_up_of(input.sequence);
                    SlayerState::of_input(input, counter)
                }
                Pending::Movement(state, sequence) => SlayerState {
                    state: Some(state),
                    started: counter,
                    sequence,
                    second: None,
                },
            };
            return (self.start(state), Some(frame_s));
        }
        if let Some(sequence) = self.follow_up.take() {
            self.sequence_s = self.newest_frame_s;
            let state = SlayerState {
                state: None,
                started: number,
                sequence,
                second: Some(SecondSlayerState {
                    started: number,
                    playing: None,
                }),
            };
            return (self.start(state), Some(self.newest_frame_s));
        }
        if self.repeats == 0 {
            return (None, None);
        }
        self.repeats -= 1;
        (self.current, None)
    }

    /// The newest slayer state, for the players who see this one.
    pub(super) const fn current(&self) -> Option<SlayerState> {
        self.current
    }

    fn start(&mut self, state: SlayerState) -> Option<SlayerState> {
        self.current = Some(state);
        self.repeats = ECHO_STATES - 1;
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 30 Hz frame time.
    const FRAME_S: f32 = 1.0 / 30.0;
    const TICK_MS: u64 = 33;

    /// Retail newchar capture: record 5568 started sequence 5055; the server followed with 4603.
    fn attack_record(counter: u16) -> SlayerRecord {
        SlayerRecord {
            counter,
            frame_s: FRAME_S,
            action: Some(SlayerInput {
                sequence: 5055,
                state: 0x000a,
                second_state: 0x002c,
                second_sequence: 0,
            }),
            intent: None,
        }
    }

    fn quiet_record(counter: u16, intent: Intent) -> SlayerRecord {
        SlayerRecord {
            counter,
            frame_s: FRAME_S,
            action: None,
            intent: Some(intent),
        }
    }

    /// The own state of the tick numbered `number`.
    fn own_state(track: &mut SlayerTrack, number: u16) -> SlayerDue {
        track.advance(number, u64::from(number) * TICK_MS);
        track.take_due()
    }

    fn spawned() -> SlayerTrack {
        let mut track = SlayerTrack::default();
        track.spawned();
        track
    }

    #[test]
    fn an_action_goes_out_started_in_its_record_then_its_follow_up() {
        const ACTION_RECORD: u16 = 0x15c0;
        const NEWEST: u16 = ACTION_RECORD + 1;
        let mut track = spawned();
        track.record(attack_record(ACTION_RECORD));
        track.record(quiet_record(NEWEST, Intent::default()));
        let started = own_state(&mut track, NEWEST).state.expect("the action");
        assert_eq!(
            (started.state, started.started, started.sequence),
            (None, ACTION_RECORD, 5055),
            "started in the record that named it; retail echoes no state without a second-slot \
             action"
        );
        let follow_up = own_state(&mut track, NEWEST).state.expect("the follow-up");
        assert_eq!(
            (follow_up.state, follow_up.started, follow_up.sequence),
            (None, NEWEST, 4603)
        );
        assert_eq!(
            own_state(&mut track, NEWEST).state,
            Some(follow_up),
            "once more"
        );
        assert_eq!(own_state(&mut track, NEWEST).state, None);
        assert_eq!(track.current(), Some(follow_up), "the others still see it");
    }

    #[test]
    fn the_sequence_time_counts_the_records_since_the_action() {
        // Two records after the action's record: 3 frames in, from an absolute part of one.
        let mut track = spawned();
        track.record(attack_record(10));
        track.record(quiet_record(11, Intent::default()));
        track.record(quiet_record(12, Intent::default()));
        let time = own_state(&mut track, 12).time.expect("time");
        let mut clock = SequenceClock::default();
        assert_eq!(
            time,
            clock.time(3.0 * FRAME_S, Some(FRAME_S), 12 * TICK_MS),
            "{time:?}"
        );
        assert!(time.abs.is_some(), "a start sends its absolute part");
    }

    #[test]
    fn no_time_goes_before_the_first_own_state() {
        let mut track = SlayerTrack::default();
        track.record(quiet_record(1, Intent::default()));
        assert_eq!(own_state(&mut track, 1), SlayerDue::default());
        let mut track = spawned();
        track.record(quiet_record(3, Intent::default()));
        assert_eq!(
            own_state(&mut track, 3).time.map(|time| time.rel),
            Some(0x83),
            "the first state's two steps and one more record"
        );
    }

    #[test]
    fn moving_starts_stops_and_comes_to_rest_with_the_retail_sequences() {
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut counter = 100;
        let mut step = |track: &mut SlayerTrack, intent: Intent| {
            counter += 1;
            track.record(quiet_record(counter, intent));
            own_state(track, counter).state
        };
        let start = step(&mut track, forward).expect("a start");
        assert_eq!(
            (start.state, start.sequence),
            (Some(movement::START_STATE), movement::START_SEQUENCE)
        );
        // The echo, then nothing while running.
        assert_eq!(step(&mut track, forward), Some(start));
        assert_eq!(step(&mut track, forward), None);
        let stop = step(&mut track, Intent::default()).expect("a stop");
        assert_eq!(
            (stop.state, stop.sequence),
            (Some(movement::MOVE_STATE), movement::STOP_SEQUENCE)
        );
        let idle = (0..movement::STOP_RECORDS)
            .find_map(|_| {
                step(&mut track, Intent::default())
                    .filter(|state| state.state == Some(movement::IDLE_STATE))
            })
            .expect("standing still");
        assert_eq!(idle.sequence, movement::IDLE_SEQUENCE);
        assert_eq!(
            idle.started.wrapping_sub(stop.started),
            movement::STOP_RECORDS
        );
        let back = step(&mut track, Intent::of_keys(false, true, false, false)).expect("back");
        assert_eq!(
            (back.state, back.sequence),
            (Some(movement::MOVE_STATE), movement::BACK_START_SEQUENCE)
        );
        step(&mut track, Intent::default());
        let again = step(&mut track, forward).expect("moving again");
        assert_eq!(again.sequence, movement::RESTART_SEQUENCE);
    }

    #[test]
    fn the_movement_sequences_are_the_retail_bytes() {
        let bytes = |sequence: u64| {
            nw_messages::wire::Writer::new()
                .prefixed(sequence)
                .expect("prefix")
                .finish()
        };
        for (sequence, retail) in [
            (movement::START_SEQUENCE, [0xaa, 0x2b]),
            (movement::BACK_START_SEQUENCE, [0x98, 0x14]),
            (movement::STOP_SEQUENCE, [0xbd, 0x15]),
            (movement::RESTART_SEQUENCE, [0xb6, 0x25]),
            (movement::IDLE_SEQUENCE, [0xb9, 0x10]),
        ] {
            assert_eq!(bytes(sequence), retail, "{sequence}");
        }
    }
}
