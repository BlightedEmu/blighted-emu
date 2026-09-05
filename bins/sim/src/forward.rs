// SPDX-License-Identifier: AGPL-3.0-or-later
//! Where the inputs of handed-off connections go (M5 spec section 3.3 step 5), with no I/O. The
//! gateway sends a connection's inputs here until it hears the new route; an entry keeps them
//! going to the new sim for a while, and keeps the route, so it can be sent again in case the
//! first route message was lost.

use std::collections::HashMap;

use nw_bus::{ConnectionRef, SimSlot};

/// The new sim of a handed-off connection and the route sequence of that handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward {
    /// The sim that took the player.
    pub sim: SimSlot,
    /// The handoff count: the sequence of the route message.
    pub sequence: u64,
}

/// The forward of each handed-off connection, with the time its entry ends.
#[derive(Debug)]
pub struct ForwardTable {
    ttl_ms: u64,
    entries: HashMap<ConnectionRef, (Forward, u64)>,
}

impl ForwardTable {
    /// Entries live `ttl_ms`.
    pub fn new(ttl_ms: u64) -> Self {
        Self {
            ttl_ms,
            entries: HashMap::new(),
        }
    }

    /// `connection` moved by `forward` at `now_ms`.
    pub fn insert(&mut self, connection: ConnectionRef, forward: Forward, now_ms: u64) {
        let ends_ms = now_ms.saturating_add(self.ttl_ms);
        self.entries.insert(connection, (forward, ends_ms));
    }

    /// Where inputs of `connection` go at `now_ms`.
    pub fn target(&self, connection: &ConnectionRef, now_ms: u64) -> Option<&Forward> {
        self.entries
            .get(connection)
            .filter(|(_, ends_ms)| now_ms <= *ends_ms)
            .map(|(forward, _)| forward)
    }

    /// Forgets `connection`: it came back, or it left.
    pub fn remove(&mut self, connection: &ConnectionRef) {
        self.entries.remove(connection);
    }

    /// Forgets the entries that ended before `now_ms`.
    pub fn expire(&mut self, now_ms: u64) {
        self.entries.retain(|_, (_, ends_ms)| now_ms <= *ends_ms);
    }
}

#[cfg(test)]
mod tests {
    use nw_bus::BusId;

    use super::*;

    const TTL_MS: u64 = 10_000;
    const NOW: u64 = 5_000;
    const GENERATION: u64 = 7;
    const SEQUENCE: u64 = 3;

    fn connection(number: u64) -> ConnectionRef {
        ConnectionRef {
            gateway: BusId::try_from("gw-1".to_owned()).expect("id"),
            connection: number,
        }
    }

    fn forward(name: &str) -> Forward {
        Forward {
            sim: SimSlot {
                id: BusId::try_from(name.to_owned()).expect("id"),
                generation: GENERATION,
            },
            sequence: SEQUENCE,
        }
    }

    #[test]
    fn forward_entries_expire() {
        let mut forwards = ForwardTable::new(TTL_MS);
        forwards.insert(connection(1), forward("sim-b"), NOW);
        forwards.insert(connection(2), forward("sim-c"), NOW + 1);
        assert_eq!(
            forwards.target(&connection(1), NOW + TTL_MS),
            Some(&forward("sim-b"))
        );
        assert_eq!(forwards.target(&connection(1), NOW + TTL_MS + 1), None);
        assert_eq!(forwards.target(&connection(9), NOW), None);
        forwards.expire(NOW + TTL_MS + 1);
        assert_eq!(
            forwards.target(&connection(1), NOW),
            None,
            "expired entries go"
        );
        assert_eq!(
            forwards.target(&connection(2), NOW),
            Some(&forward("sim-c"))
        );
        forwards.remove(&connection(2));
        assert_eq!(forwards.target(&connection(2), NOW), None);
    }

    #[test]
    fn a_new_handoff_replaces_the_entry() {
        let mut forwards = ForwardTable::new(TTL_MS);
        forwards.insert(connection(1), forward("sim-b"), NOW);
        forwards.insert(connection(1), forward("sim-c"), NOW);
        assert_eq!(
            forwards.target(&connection(1), NOW),
            Some(&forward("sim-c"))
        );
    }
}
