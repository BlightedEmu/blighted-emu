// SPDX-License-Identifier: AGPL-3.0-or-later
//! Which gateways are alive (M4 spec section 11), with no I/O. A gateway that restarts or stops
//! beating loses its players. A gateway known only from a join is watched too, so a player whose
//! join finished after its gateway was given up cannot stay forever.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use nw_bus::{BusId, Heartbeat, heartbeat_silence};

struct Seen {
    started_at_ms: Option<u64>,
    last_seen: Instant,
}

/// The gateways of one sim.
pub struct GatewayTracker {
    silence: Duration,
    gateways: BTreeMap<BusId, Seen>,
}

impl GatewayTracker {
    /// A gateway is silent after `misses` missed heartbeats of `interval`.
    pub fn new(interval: Duration, misses: u32) -> Self {
        Self {
            silence: heartbeat_silence(interval, misses),
            gateways: BTreeMap::new(),
        }
    }

    /// A heartbeat at `now`. Returns `true` when the gateway restarted: its players must leave.
    pub fn beat(&mut self, beat: &Heartbeat, now: Instant) -> bool {
        let seen = self.gateways.entry(beat.id.clone()).or_insert(Seen {
            started_at_ms: None,
            last_seen: now,
        });
        let restarted = seen
            .started_at_ms
            .is_some_and(|known| known != beat.started_at_ms);
        seen.started_at_ms = Some(beat.started_at_ms);
        seen.last_seen = now;
        restarted
    }

    /// A join from `gateway` finished at `now`; an unknown gateway is watched from now on.
    pub fn joined(&mut self, gateway: &BusId, now: Instant) {
        self.gateways.entry(gateway.clone()).or_insert(Seen {
            started_at_ms: None,
            last_seen: now,
        });
    }

    /// The gateways silent for longer than the limit at `now`, in id order; they are forgotten.
    pub fn silent(&mut self, now: Instant) -> Vec<BusId> {
        let silent: Vec<BusId> = self
            .gateways
            .iter()
            .filter(|(_, seen)| now.saturating_duration_since(seen.last_seen) > self.silence)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &silent {
            self.gateways.remove(id);
        }
        silent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_secs(1);
    const MISSES: u32 = 3;
    const STARTED_AT_MS: u64 = 100;

    fn id(name: &str) -> BusId {
        BusId::try_from(name.to_owned()).expect("id")
    }

    fn beat(name: &str, started_at_ms: u64) -> Heartbeat {
        Heartbeat {
            id: id(name),
            started_at_ms,
        }
    }

    #[test]
    fn a_new_start_time_is_a_restart() {
        let start = Instant::now();
        let mut gateways = GatewayTracker::new(INTERVAL, MISSES);
        assert!(!gateways.beat(&beat("gw-1", STARTED_AT_MS), start));
        assert!(!gateways.beat(&beat("gw-1", STARTED_AT_MS), start + INTERVAL));
        assert!(gateways.beat(&beat("gw-1", STARTED_AT_MS + 1), start + INTERVAL));
    }

    #[test]
    fn a_gateway_that_stops_beating_is_silent_once() {
        let start = Instant::now();
        let mut gateways = GatewayTracker::new(INTERVAL, MISSES);
        gateways.beat(&beat("gw-1", STARTED_AT_MS), start);
        gateways.beat(&beat("gw-2", STARTED_AT_MS), start);
        let limit = INTERVAL * MISSES;
        gateways.beat(&beat("gw-2", STARTED_AT_MS), start + limit);
        assert!(gateways.silent(start + limit).is_empty());
        assert_eq!(gateways.silent(start + limit + INTERVAL), vec![id("gw-1")]);
        assert!(gateways.silent(start + limit + INTERVAL).is_empty());
    }

    #[test]
    fn a_gateway_known_only_from_a_join_is_watched() {
        let start = Instant::now();
        let mut gateways = GatewayTracker::new(INTERVAL, MISSES);
        gateways.joined(&id("gw-1"), start);
        assert!(!gateways.beat(&beat("gw-1", STARTED_AT_MS), start));
        gateways.joined(&id("gw-2"), start);
        let after = start + INTERVAL * MISSES + INTERVAL;
        assert_eq!(gateways.silent(after), vec![id("gw-1"), id("gw-2")]);
    }
}
