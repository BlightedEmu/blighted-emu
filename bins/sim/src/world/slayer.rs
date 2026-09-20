// SPDX-License-Identifier: AGPL-3.0-or-later
//! The slayer state of a player (slayer note, 2026-09-18): the action it plays, which the client
//! and the other players see in the ALC children 13 to 18. The client starts its actions itself
//! and names each one in row 1 of its input ([`SlayerInput`]); the server echoes it in the next
//! own state, started in that state, then adds its own follow-up for some actions. In the retail
//! newchar capture 73 changes came from the client and 481 from the server: the follow-ups and
//! the movement states (run, walk). The movement states go through the run toggle
//! ([`super::replica`]); the follow-ups this server knows are [`FOLLOW_UPS`].

use nw_messages::{InputRecord, SecondSlayerState, SlayerInput, SlayerState};

use super::attack::counter_is_newer;

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

/// One player's slayer state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SlayerTrack {
    /// The counter of the newest input record read.
    counter: Option<u16>,
    /// An action the client started that no own state carried yet.
    started: Option<SlayerInput>,
    /// The follow-up that starts in the next own state.
    follow_up: Option<u64>,
    /// The newest slayer state, and how many more own states carry it.
    current: Option<SlayerState>,
    repeats: u8,
    /// The slayer state of this tick's own state, until that state takes it.
    due: Option<SlayerState>,
}

impl SlayerTrack {
    /// Reads the records of an input message `records`, newest first, not read before: the
    /// newest action the client started in them goes out with the next own state.
    pub(super) fn input(&mut self, records: &[InputRecord]) {
        for record in records.iter().rev() {
            let counter = record.u16_after_mask;
            if self
                .counter
                .is_some_and(|last| !counter_is_newer(counter, last))
            {
                continue;
            }
            self.counter = Some(counter);
            if let Some(input) = record.slayer_input() {
                self.started = Some(input);
                self.follow_up = None;
            }
        }
    }

    /// Moves on to the tick whose own state is numbered `id`: its slayer state is a new action of
    /// the client, else its server follow-up, else the last change once more ([`ECHO_STATES`]).
    /// The players who see this one get the newest at once ([`SlayerTrack::current`]).
    pub(super) fn advance(&mut self, id: u16) {
        self.due = self.next(id);
    }

    /// The slayer state of this tick's own state, once.
    pub(super) const fn take_due(&mut self) -> Option<SlayerState> {
        self.due.take()
    }

    fn next(&mut self, id: u16) -> Option<SlayerState> {
        if let Some(input) = self.started.take() {
            self.follow_up = follow_up_of(input.sequence);
            return self.start(SlayerState::of_input(input, id));
        }
        if let Some(sequence) = self.follow_up.take() {
            return self.start(SlayerState {
                state: None,
                started: id,
                sequence,
                second: Some(SecondSlayerState {
                    started: id,
                    playing: None,
                }),
            });
        }
        if self.repeats == 0 {
            return None;
        }
        self.repeats -= 1;
        self.current
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

    /// Retail newchar capture: record 5568 started sequence 5055; the server followed with 4603.
    fn attack_record(counter: u16) -> InputRecord {
        InputRecord {
            u16_after_mask: counter,
            row_1: Some([5055, 0x002c_000a, 0]),
            ..InputRecord::default()
        }
    }

    /// The own state of the tick numbered `id`.
    fn own_state(track: &mut SlayerTrack, id: u16) -> Option<SlayerState> {
        track.advance(id);
        track.take_due()
    }

    #[test]
    fn an_action_goes_out_then_its_follow_up() {
        const FIRST_STATE: u16 = 0x15c0;
        let mut track = SlayerTrack::default();
        track.input(&[attack_record(5568)]);
        let started = own_state(&mut track, FIRST_STATE).expect("the action");
        assert_eq!(
            (started.state, started.started, started.sequence),
            (None, FIRST_STATE, 5055),
            "retail echoes no state without a second-slot action"
        );
        let follow_up = own_state(&mut track, FIRST_STATE + 1).expect("the follow-up");
        assert_eq!(
            (follow_up.state, follow_up.started, follow_up.sequence),
            (None, FIRST_STATE + 1, 4603)
        );
        assert_eq!(
            own_state(&mut track, FIRST_STATE + 2),
            Some(follow_up),
            "once more"
        );
        assert_eq!(own_state(&mut track, FIRST_STATE + 3), None);
        assert_eq!(track.current(), Some(follow_up), "the others still see it");
    }

    #[test]
    fn an_older_copy_starts_nothing() {
        let mut track = SlayerTrack::default();
        track.input(&[attack_record(10)]);
        own_state(&mut track, 1);
        // The next message repeats record 10 after the new record 11, which starts nothing.
        let quiet = InputRecord {
            u16_after_mask: 11,
            ..InputRecord::default()
        };
        track.input(&[quiet, attack_record(10)]);
        assert_eq!(
            own_state(&mut track, 2).map(|state| state.sequence),
            Some(4603),
            "the follow-up, not the old action again"
        );
    }
}
