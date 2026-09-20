// SPDX-License-Identifier: AGPL-3.0-or-later
//! How fast the other players see a player's action (replication note, 2026-09-18): the bots of
//! one swarm run in one process, so a bot that changes its block notes when on a shared
//! [`SyncProbe`], and every bot that then sees that block state of it in its bundles
//! ([`crate::view::SyncView`]) takes the time from the change to the sight as one sample.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The block changes of the bots of one run, by entity number: when and to which state.
#[derive(Debug, Default)]
pub struct SyncProbe {
    changes: Mutex<HashMap<u64, (Instant, bool)>>,
}

impl SyncProbe {
    /// Bot `entity` changed its block to `blocking` at `at`.
    pub fn changed(&self, entity: u64, blocking: bool, at: Instant) {
        if let Ok(mut changes) = self.changes.lock() {
            changes.insert(entity, (at, blocking));
        }
    }

    /// When bot `entity` changed its block to `blocking`, when that is its last change.
    fn change_to(&self, entity: u64, blocking: bool) -> Option<Instant> {
        let changes = self.changes.lock().ok()?;
        changes
            .get(&entity)
            .filter(|(_, state)| *state == blocking)
            .map(|(at, _)| *at)
    }
}

/// What one bot saw of the others' blocks, and its samples.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncTrack {
    seen: HashMap<u64, bool>,
    samples: Vec<Duration>,
}

impl SyncTrack {
    /// The bot saw entity `entity` with its block `blocking` at `at`: a new state that is the
    /// last change on `probe` is one sample.
    pub fn saw(&mut self, probe: &SyncProbe, entity: u64, blocking: bool, at: Instant) {
        if self.seen.insert(entity, blocking) == Some(blocking) {
            return;
        }
        if let Some(changed) = probe.change_to(entity, blocking) {
            self.samples.push(at.saturating_duration_since(changed));
        }
    }

    /// The samples, in the order they came.
    pub fn samples(&self) -> &[Duration] {
        &self.samples
    }
}

/// The `share` quantile (0 to 1) of `samples`; `None` without samples.
pub fn quantile(samples: &[Duration], share: f64) -> Option<Duration> {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "an index into a list of samples"
    )]
    let index = ((sorted.len() as f64 - 1.0) * share).round() as usize;
    sorted.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTOR: u64 = 7;
    const DELAY: Duration = Duration::from_millis(40);

    #[test]
    fn a_change_seen_once_is_one_sample() {
        let probe = SyncProbe::default();
        let mut track = SyncTrack::default();
        let start = Instant::now();
        track.saw(&probe, ACTOR, false, start);
        assert!(track.samples().is_empty(), "no change yet");
        probe.changed(ACTOR, true, start);
        track.saw(&probe, ACTOR, true, start + DELAY);
        track.saw(&probe, ACTOR, true, start + DELAY * 2);
        assert_eq!(
            track.samples(),
            [DELAY],
            "the same state again is no sample"
        );
        track.saw(&probe, ACTOR, false, start + DELAY * 3);
        assert_eq!(
            track.samples().len(),
            1,
            "a state that is not the last change"
        );
    }

    #[test]
    fn quantiles_pick_from_the_sorted_samples() {
        let samples: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        assert_eq!(quantile(&samples, 0.5), Some(Duration::from_millis(51)));
        assert_eq!(quantile(&samples, 0.99), Some(Duration::from_millis(99)));
        assert_eq!(quantile(&[], 0.5), None);
    }
}
