// SPDX-License-Identifier: AGPL-3.0-or-later
//! Subjects, pattern `nw.<world>.<component>.<id>.<message>` (M4 spec section 6.1).

use uuid::Uuid;

use crate::id::BusId;

/// Gateway to sim: client messages and departures.
pub fn sim_input(world: Uuid, sim: &BusId) -> String {
    format!("nw.{world}.sim.{sim}.input")
}

/// Admin tool to one sim, request/reply: a GM command for a target the sim owns.
pub fn sim_admin(world: Uuid, sim: &BusId) -> String {
    format!("nw.{world}.sim.{sim}.admin")
}

/// Sim, every second.
pub fn sim_heartbeat(world: Uuid, sim: &BusId) -> String {
    format!("nw.{world}.sim.{sim}.heartbeat")
}

/// Sim to gateway: server messages and closes.
pub fn gateway_output(world: Uuid, gateway: &BusId) -> String {
    format!("nw.{world}.gateway.{gateway}.output")
}

/// Gateway, every second.
pub fn gateway_heartbeat(world: Uuid, gateway: &BusId) -> String {
    format!("nw.{world}.gateway.{gateway}.heartbeat")
}

/// Every gateway heartbeat of a world (a NATS wildcard), for `sim`.
pub fn all_gateway_heartbeats(world: Uuid) -> String {
    format!("nw.{world}.gateway.*.heartbeat")
}

/// Gateway to director, request/reply: a player joins or resumes.
pub fn director_join(world: Uuid) -> String {
    format!("nw.{world}.director.join")
}

/// Director, every heartbeat and on change: the cell table.
pub fn director_table(world: Uuid) -> String {
    format!("nw.{world}.director.table")
}

/// Director to sim, request/reply: take a player.
pub fn sim_admit(world: Uuid, sim: &BusId) -> String {
    format!("nw.{world}.sim.{sim}.admit")
}

/// Sim to sim: handoff offers and cancels.
pub fn sim_handoff(world: Uuid, sim: &BusId) -> String {
    format!("nw.{world}.sim.{sim}.handoff")
}

/// Sim to sim, every tick: ghosts.
pub fn sim_ghosts(world: Uuid, sim: &BusId) -> String {
    format!("nw.{world}.sim.{sim}.ghosts")
}

/// Sim to sim: hits on the receiver's entities.
pub fn sim_hit(world: Uuid, sim: &BusId) -> String {
    format!("nw.{world}.sim.{sim}.hit")
}

/// Every sim heartbeat of a world (a NATS wildcard), for the director.
pub fn all_sim_heartbeats(world: Uuid) -> String {
    format!("nw.{world}.sim.*.heartbeat")
}

/// Sim to the admin tool: the admin snapshot, at the sim's set interval.
pub fn sim_snapshot(world: Uuid, sim: &BusId) -> String {
    format!("nw.{world}.sim.{sim}.snapshot")
}

/// Sim to admin: the run log lines of its scripts.
pub fn sim_script_log(world: Uuid, sim: &BusId) -> String {
    format!("nw.{world}.sim.{sim}.script")
}

/// Every sim's script log of a world (a NATS wildcard), for the admin tool.
pub fn all_sim_script_logs(world: Uuid) -> String {
    format!("nw.{world}.sim.*.script")
}

/// Every sim snapshot of a world (a NATS wildcard), for the admin tool.
pub fn all_sim_snapshots(world: Uuid) -> String {
    format!("nw.{world}.sim.*.snapshot")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_follow_the_pattern() {
        let world = Uuid::nil();
        let sim = BusId::try_from("sim-1".to_owned()).expect("id");
        let gateway = BusId::try_from("gw-1".to_owned()).expect("id");
        let prefix = "nw.00000000-0000-0000-0000-000000000000";
        assert_eq!(sim_input(world, &sim), format!("{prefix}.sim.sim-1.input"));
        assert_eq!(sim_admin(world, &sim), format!("{prefix}.sim.sim-1.admin"));
        assert_eq!(
            sim_heartbeat(world, &sim),
            format!("{prefix}.sim.sim-1.heartbeat")
        );
        assert_eq!(
            gateway_output(world, &gateway),
            format!("{prefix}.gateway.gw-1.output")
        );
        assert_eq!(
            gateway_heartbeat(world, &gateway),
            format!("{prefix}.gateway.gw-1.heartbeat")
        );
        assert_eq!(
            all_gateway_heartbeats(world),
            format!("{prefix}.gateway.*.heartbeat")
        );
        assert_eq!(director_join(world), format!("{prefix}.director.join"));
        assert_eq!(director_table(world), format!("{prefix}.director.table"));
        assert_eq!(sim_admit(world, &sim), format!("{prefix}.sim.sim-1.admit"));
        assert_eq!(
            sim_handoff(world, &sim),
            format!("{prefix}.sim.sim-1.handoff")
        );
        assert_eq!(
            sim_ghosts(world, &sim),
            format!("{prefix}.sim.sim-1.ghosts")
        );
        assert_eq!(sim_hit(world, &sim), format!("{prefix}.sim.sim-1.hit"));
        assert_eq!(
            all_sim_heartbeats(world),
            format!("{prefix}.sim.*.heartbeat")
        );
        assert_eq!(
            sim_snapshot(world, &sim),
            format!("{prefix}.sim.sim-1.snapshot")
        );
        assert_eq!(all_sim_snapshots(world), format!("{prefix}.sim.*.snapshot"));
    }
}
