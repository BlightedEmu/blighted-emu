// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sim of each connection (M5 spec sections 3.3, 3.6 and 8), with no I/O.
//!
//! A join sets the route. A handoff changes it only with a higher sequence, so two route messages
//! that arrive out of order cannot leave an old sim. A connection whose sim and generation leave
//! the cell table is resumed through the director; until the answer it has no route, and its
//! inputs are dropped. A resumed player starts a new handoff count on its new sim.

use std::collections::HashMap;

use nw_bus::{BusId, CellTable};
use nw_login::queue::EntryRecord;
use uuid::Uuid;

#[derive(Debug)]
struct Route {
    sim: BusId,
    generation: u64,
    sequence: u64,
    account: Uuid,
    character: Uuid,
    entity: u64,
    resuming: bool,
}

/// A connection to resume through the director.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resume {
    /// The connection id inside the gateway.
    pub connection: u64,
    /// The account.
    pub account: Uuid,
    /// The character.
    pub character: Uuid,
    /// The entity number it keeps.
    pub entity: u64,
}

/// The routes of one gateway.
#[derive(Debug, Default)]
pub struct Routes {
    routes: HashMap<u64, Route>,
}

impl Routes {
    /// The director put the character of `record` on `sim` with `entity`.
    pub fn joined(
        &mut self,
        connection: u64,
        record: &EntryRecord,
        sim: BusId,
        generation: u64,
        entity: u64,
    ) {
        self.routes.insert(
            connection,
            Route {
                sim,
                generation,
                sequence: 0,
                account: record.account_id,
                character: record.character_id,
                entity,
                resuming: false,
            },
        );
    }

    /// The sim that gets the inputs of `connection`; `None` for an unknown or resuming one.
    pub fn target(&self, connection: u64) -> Option<&BusId> {
        self.routes
            .get(&connection)
            .filter(|route| !route.resuming)
            .map(|route| &route.sim)
    }

    /// A handoff moved `connection` to `sim`. Returns whether the route changed: only a higher
    /// `sequence` of a connection that is not resuming does.
    pub fn apply_route(
        &mut self,
        connection: u64,
        sim: BusId,
        generation: u64,
        sequence: u64,
    ) -> bool {
        let Some(route) = self
            .routes
            .get_mut(&connection)
            .filter(|route| !route.resuming && sequence > route.sequence)
        else {
            return false;
        };
        route.sim = sim;
        route.generation = generation;
        route.sequence = sequence;
        true
    }

    /// The connections whose sim and generation are not in `table`; they are marked resuming.
    pub fn lost(&mut self, table: &CellTable) -> Vec<Resume> {
        let live = |route: &Route| {
            table
                .sims
                .iter()
                .any(|slot| slot.id == route.sim && slot.generation == route.generation)
        };
        let mut jobs: Vec<Resume> = self
            .routes
            .iter_mut()
            .filter(|(_, route)| !route.resuming && !live(route))
            .map(|(connection, route)| {
                route.resuming = true;
                Resume {
                    connection: *connection,
                    account: route.account,
                    character: route.character,
                    entity: route.entity,
                }
            })
            .collect();
        jobs.sort_by_key(|job| job.connection);
        jobs
    }

    /// The director put the resumed `connection` on `sim`. Returns `false` when the connection
    /// left during the resume.
    pub fn resumed(&mut self, connection: u64, sim: BusId, generation: u64) -> bool {
        let Some(route) = self.routes.get_mut(&connection) else {
            return false;
        };
        route.sim = sim;
        route.generation = generation;
        route.sequence = 0;
        route.resuming = false;
        true
    }

    /// Forgets `connection`; returns the sim it had.
    pub fn left(&mut self, connection: u64) -> Option<BusId> {
        self.routes.remove(&connection).map(|route| route.sim)
    }
}

#[cfg(test)]
mod tests {
    use nw_bus::{CellTable, SimSlot};

    use super::*;

    const GENERATION: u64 = 5;
    const ENTITY: u64 = 40;

    fn sim(name: &str) -> BusId {
        BusId::try_from(name.to_owned()).expect("id")
    }

    fn record() -> EntryRecord {
        EntryRecord {
            ticket_id: "ticket".to_owned(),
            account_id: Uuid::new_v4(),
            world_id: Uuid::new_v4(),
            character_id: Uuid::new_v4(),
            expires_at: 0,
        }
    }

    fn table(slots: &[(&str, u64)]) -> CellTable {
        CellTable {
            version: 1,
            cell_size_m: 2048.0,
            columns: 0,
            rows: 0,
            sims: slots
                .iter()
                .map(|(name, generation)| SimSlot {
                    id: sim(name),
                    generation: *generation,
                })
                .collect(),
            owners: Vec::new(),
        }
    }

    #[test]
    fn an_older_route_is_ignored() {
        let mut routes = Routes::default();
        routes.joined(1, &record(), sim("sim-a"), GENERATION, ENTITY);
        assert_eq!(routes.target(1), Some(&sim("sim-a")));
        assert!(routes.apply_route(1, sim("sim-b"), GENERATION, 2));
        assert!(!routes.apply_route(1, sim("sim-a"), GENERATION, 1));
        assert!(!routes.apply_route(1, sim("sim-c"), GENERATION, 2));
        assert_eq!(routes.target(1), Some(&sim("sim-b")));
        assert!(!routes.apply_route(9, sim("sim-a"), GENERATION, 3));
    }

    #[test]
    fn connections_of_a_missing_sim_are_resumed_once() {
        let mut routes = Routes::default();
        let first = record();
        routes.joined(1, &first, sim("sim-a"), GENERATION, ENTITY);
        routes.joined(2, &record(), sim("sim-b"), GENERATION, ENTITY + 1);
        let jobs = routes.lost(&table(&[("sim-a", GENERATION + 1), ("sim-b", GENERATION)]));
        assert_eq!(
            jobs,
            vec![Resume {
                connection: 1,
                account: first.account_id,
                character: first.character_id,
                entity: ENTITY,
            }]
        );
        assert!(routes.lost(&table(&[("sim-b", GENERATION)])).is_empty());
        assert_eq!(routes.target(2), Some(&sim("sim-b")));
    }

    #[test]
    fn inputs_during_a_resume_are_dropped_and_a_resume_starts_a_new_sequence() {
        let mut routes = Routes::default();
        routes.joined(1, &record(), sim("sim-a"), GENERATION, ENTITY);
        routes.apply_route(1, sim("sim-a"), GENERATION, 3);
        routes.lost(&table(&[]));
        assert_eq!(routes.target(1), None);
        assert!(!routes.apply_route(1, sim("sim-b"), GENERATION, 4));
        assert!(routes.resumed(1, sim("sim-c"), GENERATION));
        assert_eq!(routes.target(1), Some(&sim("sim-c")));
        assert!(routes.apply_route(1, sim("sim-d"), GENERATION, 1));
        assert_eq!(routes.left(1), Some(sim("sim-d")));
        assert_eq!(routes.left(1), None);
        assert!(!routes.resumed(1, sim("sim-c"), GENERATION));
    }
}
