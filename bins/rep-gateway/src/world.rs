// SPDX-License-Identifier: AGPL-3.0-or-later
//! The link from the gateway to the world (M4 spec sections 3.1, 6 and 11; M5 spec sections 3 and
//! 8): joins go to `world-director`; client messages and departures go to the sim of each
//! connection; server messages and closes come in.
//!
//! [`World`] is what the server loop uses; [`BusWorld`] is the NATS version. It keeps the route of
//! each connection: a join sets it, a route message of a handoff changes it, and a connection
//! whose sim leaves the cell table is resumed through the director. Inputs and heartbeats go
//! through publishing queues ([`Publisher`]), so each sim gets the inputs in the order the clients
//! sent them, and a lost NATS connection never blocks the loop.

use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use nw_bus::{
    Bus, BusError, BusId, CellTable, ConnectionRef, GatewayOutput, Heartbeat, Join, JoinReply,
    Publisher, ResumeOf, SimInput, Subscription, subject,
};
use nw_common::time::unix_ms;
use nw_login::queue::EntryRecord;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::registration::Refusal;
use crate::routes::{Resume, Routes, Stray};

/// Client messages and departures that may wait for the publishing task.
pub const INPUT_QUEUE: usize = 4096;
/// World events that may wait for the server loop.
pub const EVENT_QUEUE: usize = 4096;
/// Heartbeats that may wait for NATS.
const HEARTBEAT_QUEUE: usize = 16;
/// Metric: inputs dropped because the publishing task is behind.
pub const METRIC_INPUTS_DROPPED: &str = "rep_gateway_world_inputs_dropped_total";
/// Metric: inputs of a connection without a route (resuming, or gone).
pub const METRIC_INPUTS_WITHOUT_ROUTE: &str = "rep_gateway_inputs_without_route_total";
/// Metric: sims told to drop a copy that nobody routes to.
pub const METRIC_STRAYS_EVICTED: &str = "rep_gateway_strays_evicted_total";
/// Metric: resumes by `result`.
pub const METRIC_RESUMES: &str = "rep_gateway_resumes_total";
const RESULT_LABEL: &str = "result";
const RESUMED: &str = "resumed";
const CLOSED: &str = "closed";

/// What the world tells the server loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldEvent {
    /// Server message envelopes for one connection, on one carrier channel.
    Messages {
        /// The sim that sent them.
        sim: BusId,
        /// The connection id.
        connection: u64,
        /// The carrier channel.
        channel: u8,
        /// Whether the carrier sends them reliably.
        reliable: bool,
        /// The envelopes, in order.
        envelopes: Vec<Vec<u8>>,
    },
    /// Close the connection.
    Close {
        /// The connection id.
        connection: u64,
    },
}

/// The world as the server loop sees it.
pub trait World: Clone + Send + Sync + 'static {
    /// Asks the world to take the character of `record` on `connection`.
    ///
    /// # Errors
    ///
    /// Returns [`Refusal::WorldRefused`] or [`Refusal::WorldUnavailable`].
    fn join(
        &self,
        connection: u64,
        record: &EntryRecord,
    ) -> impl Future<Output = Result<(), Refusal>> + Send;

    /// Forwards one client message envelope, unchanged, without waiting.
    fn client_message(&self, connection: u64, envelope: Vec<u8>);

    /// Tells the world that a connection is gone, without waiting.
    fn left(&self, connection: u64);

    /// `sim` sent messages for a connection this gateway does not have (for example after a
    /// join that timed out); it hears that the connection left.
    fn unknown(&self, connection: u64, sim: &BusId);
}

/// The link settings (config keys `gateway_id`, `join_timeout_ms`, `heartbeat_interval_ms`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldLinkSettings {
    /// The world.
    pub world_id: Uuid,
    /// This gateway in bus subjects.
    pub gateway_id: BusId,
    /// Longest wait for a join or resume answer.
    pub join_timeout: Duration,
    /// Time between heartbeats.
    pub heartbeat_interval: Duration,
}

/// The world over NATS.
#[derive(Debug, Clone)]
pub struct BusWorld {
    bus: Bus,
    settings: Arc<WorldLinkSettings>,
    inputs: Publisher<SimInput>,
    routes: Arc<Mutex<Routes>>,
    events: mpsc::Sender<WorldEvent>,
}

impl BusWorld {
    /// Subscribes to this gateway's output and the cell table, and starts the tasks that publish
    /// inputs and heartbeats. Returns the link and its events for the server loop.
    ///
    /// # Errors
    ///
    /// Returns [`BusError`] when a subscription cannot start.
    pub async fn start(
        bus: Bus,
        settings: WorldLinkSettings,
    ) -> Result<(Self, mpsc::Receiver<WorldEvent>), BusError> {
        let world_id = settings.world_id;
        let outputs = bus
            .subscribe::<GatewayOutput>(subject::gateway_output(world_id, &settings.gateway_id))
            .await?;
        let tables = bus
            .subscribe::<CellTable>(subject::director_table(world_id))
            .await?;
        bus.flush().await?;
        let settings = Arc::new(settings);
        let (events, receiver) = mpsc::channel(EVENT_QUEUE);
        let world = Self {
            inputs: Publisher::start(bus.clone(), INPUT_QUEUE),
            routes: Arc::default(),
            bus: bus.clone(),
            settings: Arc::clone(&settings),
            events,
        };
        tokio::spawn(world.clone().forward_outputs(outputs));
        tokio::spawn(world.clone().follow_tables(tables));
        tokio::spawn(heartbeats(
            Publisher::start(bus, HEARTBEAT_QUEUE),
            settings,
            world.events.clone(),
        ));
        Ok((world, receiver))
    }

    fn routes(&self) -> MutexGuard<'_, Routes> {
        // A panic while the lock was held cannot leave a route half written.
        self.routes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn connection(&self, connection: u64) -> ConnectionRef {
        ConnectionRef {
            gateway: self.settings.gateway_id.clone(),
            connection,
        }
    }

    fn input(&self, sim: &BusId, input: SimInput) {
        let subject = subject::sim_input(self.settings.world_id, sim);
        if !self.inputs.send(subject, input) {
            metrics::counter!(METRIC_INPUTS_DROPPED).increment(1);
        }
    }

    fn send_left(&self, connection: u64, sim: &BusId) {
        self.input(
            sim,
            SimInput::Left {
                connection: self.connection(connection),
            },
        );
    }

    /// Asks the director; `resume` is set for a resumed player.
    async fn ask_director(
        &self,
        connection: u64,
        account: Uuid,
        character: Uuid,
        resume: Option<ResumeOf>,
    ) -> Result<JoinReply, BusError> {
        let join = Join {
            connection: self.connection(connection),
            account,
            character,
            resume,
        };
        self.bus
            .request::<Join, JoinReply>(
                subject::director_join(self.settings.world_id),
                &join,
                self.settings.join_timeout,
            )
            .await
    }

    /// Server messages and closes go to the loop; route messages change the routes here. A
    /// stray copy past its grace is told that the connection left.
    async fn forward_outputs(self, mut outputs: Subscription<GatewayOutput>) {
        while let Some(output) = outputs.next().await {
            let event = match output.map(|output| output.message) {
                Ok(GatewayOutput::ServerMessages {
                    sim,
                    connection,
                    channel,
                    reliable,
                    envelopes,
                }) => {
                    let stray = self.routes().stray(connection, &sim, Instant::now());
                    if stray == Stray::Evict {
                        metrics::counter!(METRIC_STRAYS_EVICTED).increment(1);
                        tracing::warn!(connection, %sim, "a sim keeps a copy; it drops it");
                        self.send_left(connection, &sim);
                        continue;
                    }
                    WorldEvent::Messages {
                        sim,
                        connection,
                        channel,
                        reliable,
                        envelopes,
                    }
                }
                Ok(GatewayOutput::Close { connection }) => WorldEvent::Close { connection },
                Ok(GatewayOutput::Route {
                    connection,
                    sim,
                    generation,
                    sequence,
                }) => {
                    self.routes()
                        .apply_route(connection, sim, generation, sequence);
                    continue;
                }
                Err(error) => {
                    tracing::warn!(%error, "bad world output");
                    continue;
                }
            };
            if self.events.send(event).await.is_err() {
                return;
            }
        }
    }

    /// Resumes the connections whose sim left the cell table.
    async fn follow_tables(self, mut tables: Subscription<CellTable>) {
        while let Some(table) = tables.next().await {
            let table = match table {
                Ok(table) => table.message,
                Err(error) => {
                    tracing::warn!(%error, "bad cell table");
                    continue;
                }
            };
            let jobs = self.routes().lost(&table);
            for job in jobs {
                tokio::spawn(self.clone().resume(job));
            }
        }
    }

    /// Puts a player of a lost sim on the owner of its cell (M5 spec section 3.6); a refusal
    /// closes the connection.
    async fn resume(self, job: Resume) {
        let resume = ResumeOf {
            entity: job.entity,
            handoffs: job.handoffs,
        };
        let reply = self
            .ask_director(job.connection, job.account, job.character, Some(resume))
            .await;
        match reply {
            Ok(JoinReply::Joined {
                sim, generation, ..
            }) => {
                metrics::counter!(METRIC_RESUMES, RESULT_LABEL => RESUMED).increment(1);
                tracing::info!(connection = job.connection, %sim, "player resumed");
                let still_here = self.routes().resumed(&job, sim.clone(), generation);
                if !still_here {
                    self.send_left(job.connection, &sim);
                }
            }
            Ok(JoinReply::Refused { reason }) => self.close(job.connection, &reason).await,
            Err(error) => self.close(job.connection, &error.to_string()).await,
        }
    }

    async fn close(&self, connection: u64, reason: &str) {
        metrics::counter!(METRIC_RESUMES, RESULT_LABEL => CLOSED).increment(1);
        tracing::warn!(connection, reason, "resume failed; the connection closes");
        self.routes().left(connection);
        // The loop may have stopped; then nobody needs the close.
        let _ = self.events.send(WorldEvent::Close { connection }).await;
    }
}

impl World for BusWorld {
    async fn join(&self, connection: u64, record: &EntryRecord) -> Result<(), Refusal> {
        let reply = self
            .ask_director(connection, record.account_id, record.character_id, None)
            .await;
        match reply {
            Ok(JoinReply::Joined {
                sim,
                generation,
                entity,
            }) => {
                self.routes()
                    .joined(connection, record, sim, generation, entity);
                Ok(())
            }
            Ok(JoinReply::Refused { reason }) => {
                tracing::warn!(%reason, "world refused the character");
                Err(Refusal::WorldRefused)
            }
            Err(error) => {
                tracing::warn!(%error, "world join failed");
                Err(Refusal::WorldUnavailable)
            }
        }
    }

    fn client_message(&self, connection: u64, envelope: Vec<u8>) {
        let Some(sim) = self.routes().target(connection).cloned() else {
            metrics::counter!(METRIC_INPUTS_WITHOUT_ROUTE).increment(1);
            return;
        };
        self.input(
            &sim,
            SimInput::ClientMessage {
                connection: self.connection(connection),
                envelope,
            },
        );
    }

    fn left(&self, connection: u64) {
        let sim = self.routes().left(connection);
        if let Some(sim) = sim {
            self.send_left(connection, &sim);
        }
    }

    fn unknown(&self, connection: u64, sim: &BusId) {
        self.routes().left(connection);
        self.send_left(connection, sim);
    }
}

/// Queues this gateway's heartbeat every interval. Ends when the server loop is gone.
async fn heartbeats(
    beats: Publisher<Heartbeat>,
    settings: Arc<WorldLinkSettings>,
    events: mpsc::Sender<WorldEvent>,
) {
    let subject = subject::gateway_heartbeat(settings.world_id, &settings.gateway_id);
    let own = Heartbeat {
        id: settings.gateway_id.clone(),
        started_at_ms: unix_ms(),
    };
    let mut timer = tokio::time::interval(settings.heartbeat_interval);
    loop {
        timer.tick().await;
        if events.is_closed() {
            return;
        }
        if !beats.send(subject.clone(), own.clone()) {
            tracing::warn!("heartbeat dropped; NATS is behind");
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// The sim of a [`TestWorld`].
    pub(crate) fn test_sim() -> BusId {
        BusId::try_from("sim-test".to_owned()).expect("id")
    }

    /// What a [`TestWorld`] was sent.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Sent {
        ClientMessage(u64, Vec<u8>),
        Left(u64),
    }

    /// A world in memory: it accepts or refuses every join, sends `login` to each joined
    /// connection before it answers (as `sim` does), and records what the gateway sends.
    #[derive(Clone)]
    pub(crate) struct TestWorld {
        refuse: bool,
        login: Vec<Vec<u8>>,
        events: mpsc::Sender<WorldEvent>,
        pub(crate) joins: Arc<Mutex<Vec<u64>>>,
        pub(crate) sent: Arc<Mutex<Vec<Sent>>>,
    }

    impl TestWorld {
        /// A world and the events it sends to the gateway loop.
        pub(crate) fn new(refuse: bool, login: Vec<Vec<u8>>) -> (Self, mpsc::Receiver<WorldEvent>) {
            let (events, receiver) = mpsc::channel(EVENT_QUEUE);
            let world = Self {
                refuse,
                login,
                events,
                joins: Arc::default(),
                sent: Arc::default(),
            };
            (world, receiver)
        }

        /// Sends `event` to the gateway loop.
        pub(crate) async fn emit(&self, event: WorldEvent) {
            self.events.send(event).await.expect("gateway loop runs");
        }
    }

    impl World for TestWorld {
        async fn join(&self, connection: u64, _: &EntryRecord) -> Result<(), Refusal> {
            self.joins.lock().expect("lock").push(connection);
            if self.refuse {
                return Err(Refusal::WorldRefused);
            }
            self.emit(WorldEvent::Messages {
                sim: test_sim(),
                connection,
                channel: LOGIN_CHANNEL,
                reliable: true,
                envelopes: self.login.clone(),
            })
            .await;
            Ok(())
        }

        fn client_message(&self, connection: u64, envelope: Vec<u8>) {
            self.sent
                .lock()
                .expect("lock")
                .push(Sent::ClientMessage(connection, envelope));
        }

        fn left(&self, connection: u64) {
            self.sent.lock().expect("lock").push(Sent::Left(connection));
        }

        fn unknown(&self, connection: u64, _: &BusId) {
            self.sent.lock().expect("lock").push(Sent::Left(connection));
        }
    }

    /// The carrier channel of the login messages.
    const LOGIN_CHANNEL: u8 = 0;
}

#[cfg(all(test, feature = "infra"))]
mod bus_tests {
    use nw_bus::SimSlot;
    use nw_bus::test_support::test_bus;

    use super::*;

    const JOIN_TIMEOUT: Duration = Duration::from_millis(500);
    const HEARTBEAT: Duration = Duration::from_millis(50);
    const WAIT: Duration = Duration::from_secs(5);
    const PAUSE: Duration = Duration::from_millis(50);
    const CONNECTION: u64 = 3;
    const UNKNOWN_CONNECTION: u64 = 77;
    const GENERATION: u64 = 1_800_000_000_000;
    const ENTITY: u64 = 12;
    const ENVELOPE: [u8; 2] = [4, 5];

    fn id(prefix: &str) -> BusId {
        BusId::try_from(format!("{prefix}-{}", Uuid::new_v4().simple())).expect("id")
    }

    fn settings() -> WorldLinkSettings {
        WorldLinkSettings {
            world_id: Uuid::new_v4(),
            gateway_id: id("gw"),
            join_timeout: JOIN_TIMEOUT,
            heartbeat_interval: HEARTBEAT,
        }
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

    fn joined(sim: &BusId) -> JoinReply {
        JoinReply::Joined {
            sim: sim.clone(),
            generation: GENERATION,
            entity: ENTITY,
        }
    }

    async fn next<T>(receiver: &mut mpsc::Receiver<T>) -> T {
        tokio::time::timeout(WAIT, receiver.recv())
            .await
            .expect("in time")
            .expect("open")
    }

    /// A director that answers new joins with `first` and resumes with `resume`.
    async fn director(bus: &Bus, world_id: Uuid, first: JoinReply, resume: JoinReply) {
        let mut joins = bus
            .subscribe::<Join>(subject::director_join(world_id))
            .await
            .expect("subscribe");
        bus.flush().await.expect("flush");
        let answerer = bus.clone();
        tokio::spawn(async move {
            while let Some(Ok(join)) = joins.next().await {
                let reply = if join.message.resume.is_some() {
                    &resume
                } else {
                    &first
                };
                answerer.reply(&join, reply).await.expect("reply");
            }
        });
    }

    async fn inputs_of(bus: &Bus, world_id: Uuid, sim: &BusId) -> Subscription<SimInput> {
        let inputs = bus
            .subscribe::<SimInput>(subject::sim_input(world_id, sim))
            .await
            .expect("subscribe");
        bus.flush().await.expect("flush");
        inputs
    }

    async fn next_input(inputs: &mut Subscription<SimInput>) -> SimInput {
        tokio::time::timeout(WAIT, inputs.next())
            .await
            .expect("in time")
            .expect("open")
            .expect("decode")
            .message
    }

    /// Sends client messages until one reaches `inputs`; the route changes in another task.
    async fn message_reaches(world: &BusWorld, inputs: &mut Subscription<SimInput>) {
        let reached = async {
            loop {
                world.client_message(CONNECTION, ENVELOPE.to_vec());
                if let Ok(Some(Ok(_))) = tokio::time::timeout(PAUSE, inputs.next()).await {
                    return;
                }
            }
        };
        tokio::time::timeout(WAIT, reached)
            .await
            .expect("routed in time");
    }

    fn table_of(sims: &[&BusId]) -> CellTable {
        CellTable {
            version: 1,
            cell_size_m: 2048.0,
            columns: 0,
            rows: 0,
            sims: sims
                .iter()
                .map(|sim| SimSlot {
                    id: (*sim).clone(),
                    generation: GENERATION,
                })
                .collect(),
            owners: Vec::new(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_goes_through_the_director_or_times_out() {
        let bus = test_bus().await;
        let settings = settings();
        let (world, _events) = BusWorld::start(bus.clone(), settings.clone())
            .await
            .expect("start");
        assert_eq!(
            world.join(CONNECTION, &record()).await,
            Err(Refusal::WorldUnavailable)
        );
        let sim = id("sim");
        let refused = JoinReply::Refused {
            reason: "no".to_owned(),
        };
        director(&bus, settings.world_id, joined(&sim), refused).await;
        assert_eq!(world.join(CONNECTION, &record()).await, Ok(()));

        let refusing = self::settings();
        let (world, _events) = BusWorld::start(bus.clone(), refusing.clone())
            .await
            .expect("start");
        let reason = "no".to_owned();
        director(
            &bus,
            refusing.world_id,
            JoinReply::Refused {
                reason: reason.clone(),
            },
            JoinReply::Refused { reason },
        )
        .await;
        assert_eq!(
            world.join(CONNECTION, &record()).await,
            Err(Refusal::WorldRefused)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn inputs_follow_the_route_in_order_and_outputs_come_back() {
        let bus = test_bus().await;
        let settings = settings();
        let sim = id("sim");
        director(&bus, settings.world_id, joined(&sim), joined(&sim)).await;
        let mut inputs = inputs_of(&bus, settings.world_id, &sim).await;
        let (world, mut events) = BusWorld::start(bus.clone(), settings.clone())
            .await
            .expect("start");
        world.client_message(CONNECTION, ENVELOPE.to_vec());
        world.join(CONNECTION, &record()).await.expect("join");
        world.client_message(CONNECTION, ENVELOPE.to_vec());
        world.left(CONNECTION);
        world.unknown(UNKNOWN_CONNECTION, &sim);
        let connection = |number| ConnectionRef {
            gateway: settings.gateway_id.clone(),
            connection: number,
        };
        assert_eq!(
            next_input(&mut inputs).await,
            SimInput::ClientMessage {
                connection: connection(CONNECTION),
                envelope: ENVELOPE.to_vec(),
            }
        );
        assert_eq!(
            next_input(&mut inputs).await,
            SimInput::Left {
                connection: connection(CONNECTION)
            }
        );
        assert_eq!(
            next_input(&mut inputs).await,
            SimInput::Left {
                connection: connection(UNKNOWN_CONNECTION)
            }
        );
        let output = subject::gateway_output(settings.world_id, &settings.gateway_id);
        bus.publish(
            output,
            &GatewayOutput::Close {
                connection: CONNECTION,
            },
        )
        .await
        .expect("publish");
        assert_eq!(
            next(&mut events).await,
            WorldEvent::Close {
                connection: CONNECTION
            }
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_route_moves_the_inputs() {
        let bus = test_bus().await;
        let settings = settings();
        let (first, second) = (id("sim-a"), id("sim-b"));
        director(&bus, settings.world_id, joined(&first), joined(&first)).await;
        let mut inputs = inputs_of(&bus, settings.world_id, &second).await;
        let (world, _events) = BusWorld::start(bus.clone(), settings.clone())
            .await
            .expect("start");
        world.join(CONNECTION, &record()).await.expect("join");
        let route = GatewayOutput::Route {
            connection: CONNECTION,
            sim: second.clone(),
            generation: GENERATION,
            sequence: 1,
        };
        let output = subject::gateway_output(settings.world_id, &settings.gateway_id);
        bus.publish(output, &route).await.expect("publish");
        message_reaches(&world, &mut inputs).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_resume_moves_the_inputs_and_a_refused_one_closes() {
        let bus = test_bus().await;
        let settings = settings();
        let (lost, other) = (id("sim-a"), id("sim-b"));
        director(&bus, settings.world_id, joined(&lost), joined(&other)).await;
        let mut inputs = inputs_of(&bus, settings.world_id, &other).await;
        let (world, _events) = BusWorld::start(bus.clone(), settings.clone())
            .await
            .expect("start");
        world.join(CONNECTION, &record()).await.expect("join");
        let tables = subject::director_table(settings.world_id);
        bus.publish(tables, &table_of(&[&other]))
            .await
            .expect("table");
        message_reaches(&world, &mut inputs).await;

        let refusing = self::settings();
        let refused = JoinReply::Refused {
            reason: "no cell".to_owned(),
        };
        director(&bus, refusing.world_id, joined(&lost), refused).await;
        let (world, mut events) = BusWorld::start(bus.clone(), refusing.clone())
            .await
            .expect("start");
        world.join(CONNECTION, &record()).await.expect("join");
        let tables = subject::director_table(refusing.world_id);
        bus.publish(tables, &table_of(&[])).await.expect("table");
        assert_eq!(
            next(&mut events).await,
            WorldEvent::Close {
                connection: CONNECTION
            }
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_gateway_beats() {
        let bus = test_bus().await;
        let settings = settings();
        let mut beats = bus
            .subscribe::<Heartbeat>(subject::gateway_heartbeat(
                settings.world_id,
                &settings.gateway_id,
            ))
            .await
            .expect("subscribe");
        bus.flush().await.expect("flush");
        let (_world, _events) = BusWorld::start(bus.clone(), settings.clone())
            .await
            .expect("start");
        let beat = tokio::time::timeout(WAIT, beats.next())
            .await
            .expect("in time");
        assert_eq!(
            beat.expect("open").expect("decode").message.id,
            settings.gateway_id
        );
    }
}
