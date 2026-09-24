// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads the sim heartbeats, the cell table and the sim snapshots from the bus (admin tools spec
//! section 5.2). NATS that cannot be reached is tried again; bad messages are logged and skipped.

use std::time::Duration;

use nw_bus::{Bus, BusError, CellTable, ScriptLog, SimHeartbeat, SimSnapshot, subject};
use nw_common::time::unix_ms;
use uuid::Uuid;

use crate::live::state::LiveState;
use crate::live::{SharedState, lock};

/// Wait before the bus is tried again.
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

enum Message {
    Heartbeat(SimHeartbeat),
    Table(CellTable),
    Snapshot(SimSnapshot),
    ScriptLog(ScriptLog),
}

impl Message {
    fn apply(self, state: &mut LiveState, now_ms: u64) {
        match self {
            Self::Heartbeat(beat) => state.heartbeat(beat, now_ms),
            Self::Table(table) => state.table(table, now_ms),
            Self::Snapshot(part) => state.snapshot_part(part),
            Self::ScriptLog(log) => state.script_log(log),
        }
    }
}

/// Reads the bus of `world` at `nats_url` into `state`; tries again after a failure. Runs until
/// the task is dropped.
pub async fn run(nats_url: String, world: Uuid, state: SharedState) {
    let mut failures: u32 = 0;
    loop {
        match read(&nats_url, world, &state).await {
            Ok(()) => tracing::warn!("bus subscription ended"),
            Err(error) if failures == 0 => tracing::warn!(%error, "bus not read; trying again"),
            Err(error) => tracing::debug!(%error, "bus still not read"),
        }
        failures = failures.saturating_add(1);
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

async fn read(nats_url: &str, world: Uuid, state: &SharedState) -> Result<(), BusError> {
    let bus = Bus::connect(nats_url).await?;
    let mut beats = bus
        .subscribe::<SimHeartbeat>(subject::all_sim_heartbeats(world))
        .await?;
    let mut tables = bus
        .subscribe::<CellTable>(subject::director_table(world))
        .await?;
    let mut snapshots = bus
        .subscribe::<SimSnapshot>(subject::all_sim_snapshots(world))
        .await?;
    let mut script_logs = bus
        .subscribe::<ScriptLog>(subject::all_sim_script_logs(world))
        .await?;
    bus.flush().await?;
    tracing::info!("reading the bus");
    loop {
        let next = tokio::select! {
            next = beats.next() => next.map(|got| got.map(|got| Message::Heartbeat(got.message))),
            next = tables.next() => next.map(|got| got.map(|got| Message::Table(got.message))),
            next = snapshots.next() => next.map(|got| got.map(|got| Message::Snapshot(got.message))),
            next = script_logs.next() => next.map(|got| got.map(|got| Message::ScriptLog(got.message))),
        };
        match next {
            None => return Ok(()),
            Some(Err(error)) => tracing::warn!(%error, "bad bus message"),
            Some(Ok(message)) => message.apply(&mut lock(state), unix_ms()),
        }
    }
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use std::time::Duration;

    use nw_bus::test_support::test_nats_url;
    use nw_bus::{BusId, SimHeartbeat, SimSnapshot, SnapPlayer, subject};
    use uuid::Uuid;

    use super::*;
    use crate::live::{lock, shared};

    const STEP: Duration = Duration::from_millis(50);
    const WAIT: Duration = Duration::from_secs(5);
    const ENTITY: u64 = 77;
    const MAX_PLAYERS: u32 = 500;

    #[tokio::test]
    async fn a_published_heartbeat_and_snapshot_reach_the_view() {
        let world = Uuid::new_v4();
        let sim = BusId::try_from("sim-live".to_owned()).expect("id");
        let state = shared();
        let task = tokio::spawn(run(test_nats_url(), world, state.clone()));
        let publisher = nw_bus::test_support::test_bus().await;
        let beat = SimHeartbeat {
            id: sim.clone(),
            started_at_ms: 1,
            players: 1,
            max_players: MAX_PLAYERS,
            level: "newworld_vitaeeterna".to_owned(),
            instance: None,
        };
        let part = SimSnapshot {
            sim: sim.clone(),
            time_ms: 1,
            part: 0,
            parts: 1,
            players: vec![SnapPlayer {
                entity: ENTITY,
                name: "Ada".to_owned(),
                x: 1.0,
                y: 2.0,
                z: 3.0,
                dead: false,
                frozen: false,
            }],
            awake_mobs: Vec::new(),
            mob_counts: Some([0; nw_bus::MOB_STATES]),
        };
        let view = tokio::time::timeout(WAIT, async {
            loop {
                publisher
                    .publish(subject::sim_heartbeat(world, &sim), &beat)
                    .await
                    .expect("publish");
                publisher
                    .publish(subject::sim_snapshot(world, &sim), &part)
                    .await
                    .expect("publish");
                tokio::time::sleep(STEP).await;
                let view = lock(&state).view();
                if view.tiles.sims == 1 && !view.players.is_empty() {
                    return view;
                }
            }
        })
        .await
        .expect("the view shows the sim");
        task.abort();
        assert_eq!(view.players[0].entity, ENTITY);
        assert_eq!(view.sims[0].max_players, MAX_PLAYERS);
    }
}
