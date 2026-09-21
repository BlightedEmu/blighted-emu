// SPDX-License-Identifier: AGPL-3.0-or-later
//! Where each mob stood in the last moments (lag compensation): a client shows a mob where the
//! server had it a little earlier (its interpolation and the network), and a mob's lunge moves it
//! metres in a few ticks. Retail confirms a melee hit on a target up to 0.2 s back
//! (`javelin.server-melee-hit-confirmation-max-lookback-time-sec`), so a swing tests the mob at
//! each of its places of that time.

use std::collections::{HashMap, VecDeque};

use nw_sim_core::Position;

/// How far back a swing looks: the default of
/// `javelin.server-melee-hit-confirmation-max-lookback-time-sec`, 0.2 s (Observed: the client
/// registers it with the `f32` at .rdata 0x147fbe048, `cdcc4c3e`, in `FUN_141082780`).
const TRAIL_MS: u64 = 200;

/// The recent places of the mobs.
#[derive(Debug, Default)]
pub(super) struct MobTrails {
    trails: HashMap<u64, VecDeque<(u64, Position)>>,
}

impl MobTrails {
    /// The mobs `mobs` (entity number, place) stand where they are at `now_ms`; mobs not among
    /// them are forgotten.
    pub(super) fn record(&mut self, now_ms: u64, mobs: impl Iterator<Item = (u64, Position)>) {
        let oldest_ms = now_ms.saturating_sub(TRAIL_MS);
        let mut seen = HashMap::with_capacity(self.trails.len());
        for (mob, at) in mobs {
            let mut trail = self.trails.remove(&mob).unwrap_or_default();
            trail.push_back((now_ms, at));
            while trail.front().is_some_and(|(at_ms, _)| *at_ms < oldest_ms) {
                trail.pop_front();
            }
            seen.insert(mob, trail);
        }
        self.trails = seen;
    }

    /// Where mob `mob` stood in the last [`TRAIL_MS`], oldest first.
    pub(super) fn recent(&self, mob: u64) -> impl Iterator<Item = Position> + '_ {
        self.trails
            .get(&mob)
            .into_iter()
            .flat_map(|trail| trail.iter().map(|(_, at)| *at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOB: u64 = 9;
    const TICK_MS: u64 = 33;

    fn at(x: f32) -> Position {
        Position { x, y: 0.0, z: 0.0 }
    }

    #[test]
    fn a_trail_keeps_the_last_places_and_forgets_gone_mobs() {
        let mut trails = MobTrails::default();
        trails.record(0, [(MOB, at(0.0))].into_iter());
        trails.record(TICK_MS, [(MOB, at(1.0))].into_iter());
        assert_eq!(trails.recent(MOB).collect::<Vec<_>>(), [at(0.0), at(1.0)]);
        trails.record(TRAIL_MS + TICK_MS, [(MOB, at(2.0))].into_iter());
        assert_eq!(
            trails.recent(MOB).collect::<Vec<_>>(),
            [at(1.0), at(2.0)],
            "older places go"
        );
        trails.record(TRAIL_MS + 2 * TICK_MS, std::iter::empty());
        assert_eq!(trails.recent(MOB).count(), 0, "a gone mob is forgotten");
    }
}
