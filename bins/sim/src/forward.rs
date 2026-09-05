// SPDX-License-Identifier: AGPL-3.0-or-later
//! Where the inputs of handed-off connections go (M5 spec section 3.3 step 5), with no I/O. The
//! gateway sends a connection's inputs here until it hears the new route; an entry keeps them
//! going to the new sim for a while.

use std::collections::HashMap;

use nw_bus::{BusId, ConnectionRef};

/// The new sim of each handed-off connection, with the time its entry ends.
#[derive(Debug)]
pub struct ForwardTable {
    ttl_ms: u64,
    entries: HashMap<ConnectionRef, (BusId, u64)>,
}

impl ForwardTable {
    /// Entries live `ttl_ms`.
    pub fn new(ttl_ms: u64) -> Self {
        Self {
            ttl_ms,
            entries: HashMap::new(),
        }
    }

    /// `connection` moved to `sim` at `now_ms`.
    pub fn insert(&mut self, connection: ConnectionRef, sim: BusId, now_ms: u64) {
        let ends_ms = now_ms.saturating_add(self.ttl_ms);
        self.entries.insert(connection, (sim, ends_ms));
    }

    /// The sim that inputs of `connection` go to at `now_ms`.
    pub fn target(&self, connection: &ConnectionRef, now_ms: u64) -> Option<&BusId> {
        self.entries
            .get(connection)
            .filter(|(_, ends_ms)| now_ms <= *ends_ms)
            .map(|(sim, _)| sim)
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
    use super::*;

    const TTL_MS: u64 = 10_000;
    const NOW: u64 = 5_000;

    fn connection(number: u64) -> ConnectionRef {
        ConnectionRef {
            gateway: BusId::try_from("gw-1".to_owned()).expect("id"),
            connection: number,
        }
    }

    fn sim(name: &str) -> BusId {
        BusId::try_from(name.to_owned()).expect("id")
    }

    #[test]
    fn forward_entries_expire() {
        let mut forwards = ForwardTable::new(TTL_MS);
        forwards.insert(connection(1), sim("sim-b"), NOW);
        forwards.insert(connection(2), sim("sim-c"), NOW + 1);
        assert_eq!(
            forwards.target(&connection(1), NOW + TTL_MS),
            Some(&sim("sim-b"))
        );
        assert_eq!(forwards.target(&connection(1), NOW + TTL_MS + 1), None);
        assert_eq!(forwards.target(&connection(9), NOW), None);
        forwards.expire(NOW + TTL_MS + 1);
        assert_eq!(
            forwards.target(&connection(1), NOW),
            None,
            "expired entries go"
        );
        assert_eq!(forwards.target(&connection(2), NOW), Some(&sim("sim-c")));
        forwards.remove(&connection(2));
        assert_eq!(forwards.target(&connection(2), NOW), None);
    }

    #[test]
    fn a_new_handoff_replaces_the_entry() {
        let mut forwards = ForwardTable::new(TTL_MS);
        forwards.insert(connection(1), sim("sim-b"), NOW);
        forwards.insert(connection(1), sim("sim-c"), NOW);
        assert_eq!(forwards.target(&connection(1), NOW), Some(&sim("sim-c")));
    }
}
