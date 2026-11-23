// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bot's fishing: a player's keys, driven by the own fishing steps the server sends
//! (`FishingComponentState`, the sim's `world/fishing.rs`). Retail `16_side_11_feedifrit`
//! (Brimstone Sands, The Insatiable Flame): the fishing key took the pole out
//! (`StartedEquippingFishing` 87.7 s, `FishingEquipped` 88.3 s); the held attack key charged a
//! cast (`CastStart`) and its release cast it (`CastEnd`); two casts hit land
//! (`FailedCastHitLand` 94.5 s and 100.1 s) before the player turned and the third hit water
//! (`CastHitWater`, `FishingStarted` 107.9 s); a fish nibbled 12.0 s later, bit 1.3 s after that,
//! the press set the hook (`HookHit`), and the player held and rested the key in turns
//! (`ReelingActive`, `ReelingRecover`) until `SucceededFishCaught` (132.6 s), then
//! `FishingEnded` and `FishingEquipped` again. The client acknowledges each step it read (5339).
//!
//! The angler holds the key for a cast of [`CAST_HOLD`] and reels in turns of [`REEL_HOLD`] and
//! [`REEL_REST`]: the sim's line gains up to 37.5 tension a second on a fighting fish and loses
//! 47 a second at rest (its test values of a T5 pole), so such a turn never reaches the top of
//! 100. A cast on land turns the view by [`TURN_RAD`] for the next cast, as the retail player
//! turned.

use std::time::Duration;

use nw_messages::fishing::{FishingState, StateTransition};

/// How long the attack key charges a cast (retail held it 1.2 to 2.0 s; the cast bar fills in
/// 2 s, then falls back).
pub const CAST_HOLD: Duration = Duration::from_millis(1_500);
/// How long the key reels in one turn.
pub const REEL_HOLD: Duration = Duration::from_millis(1_200);
/// How long the key rests after each reel.
pub const REEL_REST: Duration = Duration::from_millis(1_000);
/// The turn of the view after a cast on land: an eighth of a circle.
pub const TURN_RAD: f32 = std::f32::consts::FRAC_PI_4;
/// Casts on land before the angler gives up: one for each eighth of a circle.
const MAX_LAND_CASTS: u32 = 8;
/// How long the angler waits for a step the server owes it (the pole out, a landing, a fish).
const STEP_WAIT: Duration = Duration::from_secs(10);
/// How long a cast in water waits for a bite (retail: 13.3 s to the bite, 9 to 15 s nibbles).
const BITE_WAIT: Duration = Duration::from_secs(60);
/// Casts the angler makes before it gives up (misses, breaks and fish that swam away).
const MAX_CASTS: u32 = 12;

/// What the angler does with the keys and calls of its client.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FishAct {
    /// Press the fishing key.
    FishingKey,
    /// Hold the attack key.
    Hold {
        /// The first record of the hold: the key's press is in it too.
        pressed: bool,
    },
    /// Let the attack key go.
    LetGo,
    /// Turn the view by this many radians.
    Turn(f32),
    /// Acknowledge every step before this sequence (5339).
    Acknowledge(u16),
}

/// How a fishing run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FishEnd {
    /// A fish was caught.
    Caught,
    /// The angler gave up, and why.
    GaveUp(String),
}

/// Where the angler is in its run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// The fishing key went out; the pole comes out.
    Equipping,
    /// The key charges a cast since `since`.
    Charging,
    /// The cast flies.
    Flying,
    /// The bobber waits in water for a bite.
    Waiting,
    /// The hook is set; the key reels since `since` (`holding`) or rests.
    Reeling { holding: bool },
    /// A cast ended without a fish: the pole is out again for the next cast.
    Recasting,
    /// The fish is caught.
    Caught,
}

/// A bot's fishing run.
#[derive(Debug, Clone)]
pub struct Angler {
    stage: Stage,
    /// When the stage started.
    since: Duration,
    /// The steps read so far (an index into the view's steps).
    read: usize,
    /// The next sequence to acknowledge.
    acknowledged: Option<u16>,
    casts: u32,
    land_casts: u32,
    /// Whether the fishing key went out.
    started: bool,
}

impl Angler {
    /// A run that starts at `now` after the `seen` own fishing steps the bot read before it: a
    /// run reads only the steps of its own casts (live 2026-11-22 15:23: a second run read the
    /// first run's catch and ended at once).
    pub const fn new(now: Duration, seen: usize) -> Self {
        Self {
            stage: Stage::Equipping,
            since: now,
            read: seen,
            acknowledged: None,
            casts: 0,
            land_casts: 0,
            started: false,
        }
    }

    /// One tick at `now` with the own fishing steps `steps` seen so far: what the client sends,
    /// or how the run ended.
    pub fn tick(
        &mut self,
        steps: &[StateTransition],
        now: Duration,
    ) -> Result<Vec<FishAct>, FishEnd> {
        let mut acts = Vec::new();
        if !self.started {
            self.started = true;
            self.since = now;
            return Ok(vec![FishAct::FishingKey]);
        }
        let new = steps.get(self.read..).unwrap_or_default();
        self.read = steps.len();
        if let Some(last) = new.last() {
            let next = last.sequence.wrapping_add(1);
            if self.acknowledged != Some(next) {
                self.acknowledged = Some(next);
                acts.push(FishAct::Acknowledge(next));
            }
        }
        for step in new {
            self.on_step(step.state, now, &mut acts)?;
        }
        let waited = now.saturating_sub(self.since);
        match self.stage {
            Stage::Equipping | Stage::Flying if waited > STEP_WAIT => {
                return Err(FishEnd::GaveUp(format!(
                    "no fishing step within {} s ({:?})",
                    STEP_WAIT.as_secs(),
                    self.stage
                )));
            }
            Stage::Waiting if waited > BITE_WAIT => {
                return Err(FishEnd::GaveUp(format!(
                    "no bite within {} s",
                    BITE_WAIT.as_secs()
                )));
            }
            Stage::Recasting if waited > STEP_WAIT => self.cast(now, &mut acts)?,
            Stage::Charging if waited >= CAST_HOLD => {
                acts.push(FishAct::LetGo);
                self.enter(Stage::Flying, now);
            }
            Stage::Charging => acts.push(FishAct::Hold { pressed: false }),
            Stage::Reeling { holding: true } if waited >= REEL_HOLD => {
                acts.push(FishAct::LetGo);
                self.enter(Stage::Reeling { holding: false }, now);
            }
            Stage::Reeling { holding: true } => acts.push(FishAct::Hold { pressed: false }),
            Stage::Reeling { holding: false } if waited >= REEL_REST => {
                acts.push(FishAct::Hold { pressed: true });
                self.enter(Stage::Reeling { holding: true }, now);
            }
            Stage::Caught => return Err(FishEnd::Caught),
            _ => {}
        }
        Ok(acts)
    }

    /// The server entered `state` at `now`.
    fn on_step(
        &mut self,
        state: FishingState,
        now: Duration,
        acts: &mut Vec<FishAct>,
    ) -> Result<(), FishEnd> {
        match state {
            FishingState::FishingEquipped
                if matches!(self.stage, Stage::Equipping | Stage::Recasting) =>
            {
                self.cast(now, acts)?;
            }
            FishingState::FishingStarted => self.enter(Stage::Waiting, now),
            FishingState::FishBiteWindowOpen => acts.push(FishAct::Hold { pressed: true }),
            FishingState::HookHit => {
                // The hook's press ends; the line rests until the first reel.
                acts.push(FishAct::LetGo);
                self.enter(Stage::Reeling { holding: false }, now);
            }
            FishingState::FailedCastHitLand => {
                self.land_casts += 1;
                if self.land_casts >= MAX_LAND_CASTS {
                    return Err(FishEnd::GaveUp(format!(
                        "{MAX_LAND_CASTS} casts hit land: no water around this place"
                    )));
                }
                acts.push(FishAct::Turn(TURN_RAD));
                self.enter(Stage::Recasting, now);
            }
            FishingState::FailedHookMissedTooEarly
            | FishingState::FailedHookMissedTooLate
            | FishingState::FailedLineBreak
            | FishingState::FailedFishSwamAway => {
                if matches!(self.stage, Stage::Reeling { holding: true }) {
                    acts.push(FishAct::LetGo);
                }
                self.enter(Stage::Recasting, now);
            }
            FishingState::SucceededFishCaught => {
                if matches!(self.stage, Stage::Reeling { holding: true }) {
                    acts.push(FishAct::LetGo);
                }
                self.enter(Stage::Caught, now);
            }
            _ => {}
        }
        Ok(())
    }

    /// Starts the next cast's charge at `now`.
    fn cast(&mut self, now: Duration, acts: &mut Vec<FishAct>) -> Result<(), FishEnd> {
        if self.casts >= MAX_CASTS {
            return Err(FishEnd::GaveUp(format!("{MAX_CASTS} casts caught nothing")));
        }
        self.casts += 1;
        acts.push(FishAct::Hold { pressed: true });
        self.enter(Stage::Charging, now);
        Ok(())
    }

    const fn enter(&mut self, stage: Stage, now: Duration) {
        self.stage = stage;
        self.since = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// The steps of the retail run (`16_side_11_feedifrit`), by sequence.
    fn steps(states: &[FishingState]) -> Vec<StateTransition> {
        (0_u16..)
            .zip(states)
            .map(|(sequence, state)| StateTransition {
                sequence,
                state: *state,
            })
            .collect()
    }

    #[test]
    fn the_angler_takes_the_pole_out_casts_turns_off_land_hooks_reels_and_catches() {
        use FishingState::*;
        let mut angler = Angler::new(at(0), 0);
        assert_eq!(angler.tick(&[], at(0)), Ok(vec![FishAct::FishingKey]));
        let mut seen = steps(&[StartedEquippingFishing, FishingEquipped]);
        assert_eq!(
            angler.tick(&seen, at(600)),
            Ok(vec![
                FishAct::Acknowledge(2),
                FishAct::Hold { pressed: true },
                FishAct::Hold { pressed: false }
            ])
        );
        assert_eq!(
            angler.tick(&seen, at(600) + CAST_HOLD),
            Ok(vec![FishAct::LetGo])
        );
        // The first cast hits land: the view turns, and the next cast charges.
        seen.extend(
            steps(&[CastStart, CastEnd, FailedCastHitLand])
                .iter()
                .map(|step| StateTransition {
                    sequence: step.sequence + 2,
                    ..*step
                }),
        );
        let acts = angler.tick(&seen, at(3_000)).expect("on");
        assert_eq!(acts, vec![FishAct::Acknowledge(5), FishAct::Turn(TURN_RAD)]);
        let next = |seen: &mut Vec<StateTransition>, state| {
            let sequence = seen.last().map_or(0, |last| last.sequence + 1);
            seen.push(StateTransition { sequence, state });
        };
        next(&mut seen, FishingEnded);
        next(&mut seen, FishingEquipped);
        let acts = angler.tick(&seen, at(3_800)).expect("on");
        assert!(acts.contains(&FishAct::Hold { pressed: true }));
        angler.tick(&seen, at(3_800) + CAST_HOLD).expect("cast");
        for state in [
            CastStart,
            CastEnd,
            CastHitWater,
            FishingStarted,
            FishNibbleWindowOpen,
        ] {
            next(&mut seen, state);
        }
        angler.tick(&seen, at(7_000)).expect("waits");
        next(&mut seen, FishBiteWindowOpen);
        let acts = angler.tick(&seen, at(19_000)).expect("bite");
        assert!(acts.contains(&FishAct::Hold { pressed: true }), "the hook");
        next(&mut seen, HookHit);
        let acts = angler.tick(&seen, at(19_500)).expect("hooked");
        assert!(acts.contains(&FishAct::LetGo));
        assert_eq!(
            angler.tick(&seen, at(19_500) + REEL_REST),
            Ok(vec![FishAct::Hold { pressed: true }]),
            "the first reel after the rest"
        );
        assert_eq!(
            angler.tick(&seen, at(19_600) + REEL_REST),
            Ok(vec![FishAct::Hold { pressed: false }])
        );
        assert_eq!(
            angler.tick(&seen, at(19_500) + REEL_REST + REEL_HOLD),
            Ok(vec![FishAct::LetGo]),
            "a rest after each reel"
        );
        next(&mut seen, SucceededFishCaught);
        assert_eq!(angler.tick(&seen, at(25_000)), Err(FishEnd::Caught));
    }

    #[test]
    fn a_new_run_reads_only_the_steps_after_the_earlier_runs() {
        use FishingState::*;
        let earlier = steps(&[
            StartedEquippingFishing,
            FishingEquipped,
            SucceededFishCaught,
        ]);
        let mut angler = Angler::new(at(0), earlier.len());
        assert_eq!(angler.tick(&earlier, at(0)), Ok(vec![FishAct::FishingKey]));
        assert!(
            angler.tick(&earlier, at(500)).is_ok(),
            "the earlier catch is not this run's"
        );
    }

    #[test]
    fn the_angler_gives_up_when_every_direction_hits_land() {
        use FishingState::*;
        let mut angler = Angler::new(at(0), 0);
        angler.tick(&[], at(0)).expect("key");
        let mut seen = steps(&[StartedEquippingFishing, FishingEquipped]);
        let mut now = at(500);
        angler.tick(&seen, now).expect("charge");
        let mut ended = None;
        for _ in 0..MAX_LAND_CASTS {
            now += CAST_HOLD;
            angler.tick(&seen, now).expect("cast");
            for state in [
                CastStart,
                CastEnd,
                FailedCastHitLand,
                FishingEnded,
                FishingEquipped,
            ] {
                let sequence = seen.last().map_or(0, |last| last.sequence + 1);
                seen.push(StateTransition { sequence, state });
            }
            now += at(1_000);
            if let Err(end) = angler.tick(&seen, now) {
                ended = Some(end);
                break;
            }
        }
        assert!(matches!(ended, Some(FishEnd::GaveUp(why)) if why.contains("land")));
    }
}
