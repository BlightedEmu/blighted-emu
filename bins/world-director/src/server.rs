// SPDX-License-Identifier: AGPL-3.0-or-later
//! The loop of `world-director` (M5 spec sections 3.1, 3.6, 3.7, 6 and 9).
//!
//! One task owns the sim list and the assignment. Each join runs in its own task with a copy of
//! the assignment, so a slow database or sim never stops the heartbeats. A new director waits one
//! silence window to hear every sim before it publishes a table or routes a join.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nw_bus::{
    Bus, BusError, BusId, CellTable, Incoming, Join, JoinReply, Publisher, SimHeartbeat,
    heartbeat_silence, subject,
};
use nw_db::PgPool;
use nw_login::queue::QueueStore;
use nw_sim_core::{CellGrid, Position, TUTORIAL_FOLDER};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::join::{JoinContext, route_join};
use crate::sims::{Assignment, SimRegistry};

/// Tables that may wait for NATS.
const TABLE_QUEUE: usize = 16;
/// Admit reports that may wait for the loop.
const ADMIT_QUEUE: usize = 1024;
/// Metric: live sims.
pub const METRIC_SIMS: &str = "director_sims";
/// Metric: players in the world.
pub const METRIC_PLAYERS: &str = "director_players";
/// Metric: joins by `result`.
pub const METRIC_JOINS: &str = "director_joins_total";
/// Metric: changes of the cell table.
pub const METRIC_TABLE_CHANGES: &str = "director_table_changes_total";
const RESULT_LABEL: &str = "result";
const JOINED: &str = "joined";
/// Join refusal while the director waits for the sims.
const STARTING: &str = "the director is starting";

/// What the loop needs from the configuration.
#[derive(Debug, Clone)]
pub struct DirectorSettings {
    /// The world.
    pub world_id: Uuid,
    /// The open map level.
    pub level: String,
    /// Where characters that were never in the world appear, in turn.
    pub starts: Vec<Position>,
    /// The cells of the level.
    pub grid: CellGrid,
    /// Longest wait for a sim to take a player.
    pub admit_timeout: Duration,
    /// Time between table publications and free-slot reports.
    pub heartbeat_interval: Duration,
    /// Missed sim heartbeats before its cells move.
    pub heartbeat_misses: u32,
}

/// The loop's failure.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The bus failed.
    #[error(transparent)]
    Bus(#[from] BusError),
}

struct Loop<Q> {
    settings: DirectorSettings,
    context: Arc<JoinContext>,
    store: Q,
    sims: SimRegistry,
    assignment: Option<Arc<Assignment>>,
    version: u64,
    tables: Publisher<CellTable>,
    /// Join tasks report each sim that took a player here.
    admits: mpsc::Sender<BusId>,
    started: Instant,
}

/// Serves joins, sim heartbeats, the cell table and the free player slots until `shutdown`
/// finishes.
///
/// # Errors
///
/// Returns [`ServeError`] when a subscription cannot start.
pub async fn serve<Q: QueueStore>(
    settings: DirectorSettings,
    bus: Bus,
    db: PgPool,
    store: Q,
    shutdown: impl Future<Output = ()>,
) -> Result<(), ServeError> {
    let world_id = settings.world_id;
    let mut joins = bus
        .subscribe::<Join>(subject::director_join(world_id))
        .await?;
    let mut beats = bus
        .subscribe::<SimHeartbeat>(subject::all_sim_heartbeats(world_id))
        .await?;
    bus.flush().await?;
    let (admits, mut admitted) = mpsc::channel(ADMIT_QUEUE);
    let mut state = Loop {
        context: Arc::new(JoinContext {
            world_id,
            level: settings.level.clone(),
            starts: settings.starts.clone(),
            admit_timeout: settings.admit_timeout,
            db,
            bus: bus.clone(),
        }),
        sims: SimRegistry::new(settings.heartbeat_interval, settings.heartbeat_misses),
        tables: Publisher::start(bus, TABLE_QUEUE),
        settings,
        store,
        assignment: None,
        version: 0,
        admits,
        started: Instant::now(),
    };
    let mut timer = tokio::time::interval(state.settings.heartbeat_interval);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => return Ok(()),
            Some(join) = joins.next() => match join {
                Ok(join) => state.join(join),
                Err(error) => tracing::warn!(%error, "bad join message"),
            },
            Some(beat) = beats.next() => match beat {
                Ok(beat) => state.beat(&beat.message),
                Err(error) => tracing::warn!(%error, "bad sim heartbeat"),
            },
            Some(sim) = admitted.recv() => state.sims.admitted(&sim),
            _ = timer.tick() => state.timer(),
        }
    }
}

impl<Q: QueueStore> Loop<Q> {
    fn silence(&self) -> Duration {
        heartbeat_silence(
            self.settings.heartbeat_interval,
            self.settings.heartbeat_misses,
        )
    }

    /// Routes a join in its own task; before the first table it is refused.
    fn join(&self, join: Incoming<Join>) {
        let context = Arc::clone(&self.context);
        let assignment = self.assignment.clone();
        let tutorial = self.sims.least_loaded(TUTORIAL_FOLDER);
        let admits = self.admits.clone();
        tokio::spawn(async move {
            let reply = match assignment {
                Some(assignment) => {
                    match route_join(&context, &assignment, tutorial.as_ref(), &join.message).await
                    {
                        Ok((sim, entity)) => {
                            metrics::counter!(METRIC_JOINS, RESULT_LABEL => JOINED).increment(1);
                            tracing::info!(character = %join.message.character, sim = %sim.id, "joined");
                            // The loop may have stopped; then the count no longer matters.
                            let _ = admits.send(sim.id.clone()).await;
                            JoinReply::Joined {
                                sim: sim.id,
                                generation: sim.generation,
                                entity,
                            }
                        }
                        Err(refusal) => {
                            metrics::counter!(METRIC_JOINS, RESULT_LABEL => refusal.label())
                                .increment(1);
                            tracing::warn!(character = %join.message.character, %refusal, "join refused");
                            JoinReply::Refused {
                                reason: refusal.to_string(),
                            }
                        }
                    }
                }
                None => JoinReply::Refused {
                    reason: STARTING.to_owned(),
                },
            };
            if let Err(error) = context.bus.reply(&join, &reply).await {
                tracing::warn!(%error, "join reply failed");
            }
        });
    }

    fn beat(&mut self, beat: &SimHeartbeat) {
        if self.sims.beat(beat, Instant::now()) {
            tracing::info!(sim = %beat.id, generation = beat.started_at_ms, "sim joined the world");
            self.reassign();
        }
    }

    /// Drops silent sims, publishes the table and reports the free slots, once the start window
    /// is over.
    fn timer(&mut self) {
        let silent = self.sims.drop_silent(Instant::now());
        for sim in &silent {
            tracing::warn!(%sim, "sim silent; its cells move");
        }
        if !silent.is_empty() {
            self.reassign();
        }
        if self.assignment.is_none() && self.started.elapsed() >= self.silence() {
            self.reassign_now();
        }
        if self.sims.all_silent(Instant::now()) {
            // Most likely this director lost NATS: the sims keep their last table.
            tracing::warn!("no sim heartbeat in the silence window; the table stays");
            return;
        }
        let Some(assignment) = &self.assignment else {
            return;
        };
        self.publish(assignment.table());
        metrics::gauge!(METRIC_SIMS).set(self.sims.count() as f64);
        metrics::gauge!(METRIC_PLAYERS).set(f64::from(self.sims.players()));
        let store = self.store.clone();
        let (world_id, free, ttl) = (
            self.settings.world_id,
            self.sims.free_slots(),
            self.silence(),
        );
        tokio::spawn(async move {
            let ttl_ms = u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX);
            if let Err(error) = store.set_free_slots(world_id, free, ttl_ms).await {
                tracing::warn!(%error, "free slots not reported");
            }
        });
    }

    /// A new assignment when the director is past its start window.
    fn reassign(&mut self) {
        if self.assignment.is_some() {
            self.reassign_now();
        }
    }

    fn reassign_now(&mut self) {
        self.version += 1;
        let assignment = Assignment::new(
            self.version,
            self.settings.grid,
            self.sims.slots(&self.context.level),
            self.sims.other_slots(&self.context.level),
        );
        for (sim, from_x, to_x) in assignment.x_ranges() {
            tracing::info!(sim = %sim.id, from_x, to_x, version = self.version, "cells assigned");
        }
        metrics::counter!(METRIC_TABLE_CHANGES).increment(1);
        self.publish(assignment.table());
        self.assignment = Some(Arc::new(assignment));
    }

    fn publish(&self, table: CellTable) {
        let subject = subject::director_table(self.settings.world_id);
        if !self.tables.send(subject, table) {
            tracing::warn!("table dropped; NATS is behind");
        }
    }
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use std::sync::{Arc, Mutex};

    use nw_bus::test_support::test_bus;
    use nw_bus::{
        Admit, AdmitReply, BusId, CellTable, ConnectionRef, Join, JoinReply, ResumeOf,
        SimHeartbeat, Subscription, subject,
    };
    use nw_db::test_db::with_test_schema;
    use nw_db::{
        NewCharacter, PgPool, SavedPosition, WORLD_MIGRATOR, insert_character,
        save_character_position,
    };
    use nw_login::queue::{QueueAction, QueueSettings, QueueStatus, QueueStore, QueueTime};
    use nw_login::test_support::test_valkey_url;
    use nw_login::valkey::Valkey;
    use nw_sim_core::{CellGrid, Position};
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;
    use uuid::Uuid;

    use super::*;

    const LEVEL: &str = "newworld_vitaeeterna";
    const LEVEL_M: f32 = 14_336.0;
    const CELL_M: f32 = 2048.0;
    const HEARTBEAT: Duration = Duration::from_millis(100);
    const MISSES: u32 = 2;
    const ADMIT_TIMEOUT: Duration = Duration::from_millis(500);
    const WAIT: Duration = Duration::from_secs(5);
    const MAX_PLAYERS: u32 = 50;
    const RESUMED_ENTITY: u64 = 42;
    const RESUMED_HANDOFFS: u64 = 1 << 32;
    const RESUMED_BUNDLES: u64 = 900;
    const CONNECTION: u64 = 7;
    /// West of the border between two sims (column 3 starts at 6144 m).
    const WEST: Position = Position {
        x: 1000.0,
        y: 7000.0,
        z: 100.0,
    };
    const EAST_X: f64 = 12_000.0;
    const ENTRY_LIMIT: u32 = 10;
    const STALE_AFTER_MS: u64 = 60_000;
    const NO_YAW: f32 = 0.0;

    fn id(prefix: &str) -> BusId {
        BusId::try_from(format!("{prefix}-{}", Uuid::new_v4().simple())).expect("id")
    }

    fn settings(world_id: Uuid) -> DirectorSettings {
        DirectorSettings {
            world_id,
            level: LEVEL.to_owned(),
            starts: vec![WEST],
            grid: CellGrid::new(LEVEL_M, LEVEL_M, CELL_M).expect("grid"),
            admit_timeout: ADMIT_TIMEOUT,
            heartbeat_interval: HEARTBEAT,
            heartbeat_misses: MISSES,
        }
    }

    async fn valkey() -> Valkey {
        Valkey::connect(&test_valkey_url())
            .await
            .expect("connect to Valkey")
    }

    /// A sim that beats and answers every admit; it records the admits.
    struct FakeSim {
        id: BusId,
        admits: Arc<Mutex<Vec<Admit>>>,
        beats: JoinHandle<()>,
    }

    impl FakeSim {
        async fn start(bus: &Bus, world_id: Uuid, name: &str, started_at_ms: u64) -> Self {
            let id = id(name);
            let admits = Arc::new(Mutex::new(Vec::new()));
            let mut requests = bus
                .subscribe::<Admit>(subject::sim_admit(world_id, &id))
                .await
                .expect("admits");
            bus.flush().await.expect("flush");
            let answerer = bus.clone();
            let seen = Arc::clone(&admits);
            tokio::spawn(async move {
                while let Some(Ok(admit)) = requests.next().await {
                    seen.lock().expect("lock").push(admit.message.clone());
                    answerer
                        .reply(&admit, &AdmitReply::Admitted)
                        .await
                        .expect("reply");
                }
            });
            let beater = bus.clone();
            let beat = SimHeartbeat {
                id: id.clone(),
                started_at_ms,
                players: 0,
                max_players: MAX_PLAYERS,
                level: LEVEL.to_owned(),
            };
            let beats = tokio::spawn(async move {
                loop {
                    let subject = subject::sim_heartbeat(world_id, &beat.id);
                    beater.publish(subject, &beat).await.expect("beat");
                    tokio::time::sleep(HEARTBEAT).await;
                }
            });
            Self { id, admits, beats }
        }

        fn admits(&self) -> Vec<Admit> {
            self.admits.lock().expect("lock").clone()
        }
    }

    struct Running {
        bus: Bus,
        world_id: Uuid,
        tables: Subscription<CellTable>,
        stop: oneshot::Sender<()>,
        server: JoinHandle<Result<(), ServeError>>,
    }

    async fn run(db: &PgPool) -> Running {
        let bus = test_bus().await;
        let world_id = Uuid::new_v4();
        let tables = bus
            .subscribe::<CellTable>(subject::director_table(world_id))
            .await
            .expect("tables");
        bus.flush().await.expect("flush");
        let (stop, stopped) = oneshot::channel::<()>();
        let server = tokio::spawn(serve(
            settings(world_id),
            bus.clone(),
            db.clone(),
            valkey().await,
            async {
                let _ = stopped.await;
            },
        ));
        Running {
            bus,
            world_id,
            tables,
            stop,
            server,
        }
    }

    impl Running {
        /// The next table with exactly these sims.
        async fn table_with(&mut self, sims: &[&BusId]) -> CellTable {
            let wanted = async {
                loop {
                    let table = self
                        .tables
                        .next()
                        .await
                        .expect("open")
                        .expect("decode")
                        .message;
                    let ids: Vec<&BusId> = table.sims.iter().map(|slot| &slot.id).collect();
                    let mut expected = sims.to_vec();
                    expected.sort();
                    if ids == expected {
                        return table;
                    }
                }
            };
            tokio::time::timeout(WAIT, wanted)
                .await
                .expect("table in time")
        }

        async fn join(&self, join: &Join) -> JoinReply {
            self.bus
                .request(subject::director_join(self.world_id), join, WAIT)
                .await
                .expect("join reply")
        }

        async fn stop(self) {
            let _ = self.stop.send(());
            self.server.await.expect("join").expect("serve");
        }
    }

    /// A character of a new account: (account, character).
    async fn character(db: &PgPool, name: &str) -> (Uuid, Uuid) {
        let account = Uuid::new_v4();
        let character = insert_character(
            db,
            &NewCharacter {
                id: Uuid::new_v4(),
                account_id: account,
                name: name.to_owned(),
                appearance: serde_json::json!({}),
            },
        )
        .await
        .expect("character");
        (account, character.id)
    }

    fn join(account: Uuid, character: Uuid, resume: Option<ResumeOf>) -> Join {
        Join {
            connection: ConnectionRef {
                gateway: BusId::try_from("gw-1".to_owned()).expect("id"),
                connection: CONNECTION,
            },
            account,
            character,
            resume,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn joins_wait_for_the_start_window_then_go_to_the_owner_of_the_cell() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let mut director = run(db).await;
            let (account, new) = character(db, "Westa").await;
            assert!(matches!(
                director.join(&join(account, new, None)).await,
                JoinReply::Refused { .. }
            ));
            let west = FakeSim::start(&director.bus, director.world_id, "sim-a", 1).await;
            let east = FakeSim::start(&director.bus, director.world_id, "sim-b", 1).await;
            director.table_with(&[&west.id, &east.id]).await;

            let reply = director.join(&join(account, new, None)).await;
            let JoinReply::Joined { sim, entity, .. } = reply else {
                panic!("expected joined, got {reply:?}");
            };
            assert_eq!(sim, west.id);
            let admit = west.admits()[0].clone();
            assert!(admit.login);
            assert_eq!(admit.entity, entity);
            assert_eq!((admit.position.x, admit.position.z), (WEST.x, WEST.z));

            let (account, saved) = character(db, "Easta").await;
            let position = SavedPosition {
                level: LEVEL.to_owned(),
                x: EAST_X,
                y: f64::from(WEST.y),
                z: f64::from(WEST.z),
                yaw: NO_YAW,
                saved_at_ms: 1,
            };
            save_character_position(db, saved, &position)
                .await
                .expect("save");
            let reply = director.join(&join(account, saved, None)).await;
            let JoinReply::Joined {
                sim, entity: next, ..
            } = reply
            else {
                panic!("expected joined, got {reply:?}");
            };
            assert_eq!(sim, east.id);
            assert!(next > entity, "entity numbers rise");
            director.stop().await;
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_resume_sends_no_login_and_keeps_the_number() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let mut director = run(db).await;
            let sim = FakeSim::start(&director.bus, director.world_id, "sim-a", 1).await;
            director.table_with(&[&sim.id]).await;
            let (account, character) = character(db, "Resuma").await;
            let resume = ResumeOf {
                entity: RESUMED_ENTITY,
                handoffs: RESUMED_HANDOFFS,
                bundles_sent: RESUMED_BUNDLES,
            };
            let reply = director.join(&join(account, character, Some(resume))).await;
            assert!(matches!(
                reply,
                JoinReply::Joined {
                    entity: RESUMED_ENTITY,
                    ..
                }
            ));
            let admit = sim.admits()[0].clone();
            assert!(!admit.login);
            assert_eq!(admit.entity, RESUMED_ENTITY);
            assert_eq!(admit.handoffs, RESUMED_HANDOFFS);
            assert_eq!(admit.bundles_sent, RESUMED_BUNDLES);
            director.stop().await;
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_for_another_account_or_character_is_refused() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let mut director = run(db).await;
            let (_, character) = character(db, "Othera").await;
            let sim = FakeSim::start(&director.bus, director.world_id, "sim-a", 1).await;
            director.table_with(&[&sim.id]).await;
            assert!(matches!(
                director.join(&join(Uuid::new_v4(), character, None)).await,
                JoinReply::Refused { .. }
            ));
            assert!(matches!(
                director
                    .join(&join(Uuid::new_v4(), Uuid::new_v4(), None))
                    .await,
                JoinReply::Refused { .. }
            ));
            assert!(sim.admits().is_empty());
            director.stop().await;
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_table_follows_the_sims_and_free_slots_reach_valkey() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let mut director = run(db).await;
            let first = FakeSim::start(&director.bus, director.world_id, "sim-a", 1).await;
            let second = FakeSim::start(&director.bus, director.world_id, "sim-b", 1).await;
            let table = director.table_with(&[&first.id, &second.id]).await;
            assert_eq!(table.owners.len(), 49);

            let queue = QueueSettings {
                world_id: director.world_id,
                capacity: ENTRY_LIMIT,
                stale_after_ms: STALE_AFTER_MS,
            };
            let admitted = async {
                loop {
                    let status = valkey()
                        .await
                        .update(
                            &queue,
                            Uuid::new_v4(),
                            QueueAction::Join {
                                character_id: Uuid::new_v4(),
                                new_ticket_id: Uuid::new_v4().to_string(),
                            },
                            QueueTime::StoreClock,
                        )
                        .await
                        .expect("queue");
                    if matches!(status, Some(QueueStatus::Admitted(_))) {
                        return;
                    }
                    tokio::time::sleep(HEARTBEAT).await;
                }
            };
            tokio::time::timeout(WAIT, admitted)
                .await
                .expect("free slots in time");

            second.beats.abort();
            director.table_with(&[&first.id]).await;
            director.stop().await;
        })
        .await;
    }
}
