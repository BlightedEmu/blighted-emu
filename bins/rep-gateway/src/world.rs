// SPDX-License-Identifier: AGPL-3.0-or-later
//! The link from the gateway to the world, `sim` (M4 spec sections 3.1, 6 and 11): joins, client
//! messages and departures go out; server messages, closes and the loss of the world come in.
//!
//! [`World`] is what the server loop uses; [`BusWorld`] is the NATS version. Inputs and heartbeats
//! go through publishing queues ([`Publisher`]), so `sim` gets the inputs in the order the clients
//! sent them, and a lost NATS connection never blocks the loop or the silence check.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nw_bus::{
    Bus, BusError, BusId, ConnectionRef, GatewayOutput, Heartbeat, Join, JoinReply, Publisher,
    SimInput, Subscription, heartbeat_silence, subject,
};
use nw_common::time::unix_ms;
use nw_login::queue::EntryRecord;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::registration::Refusal;

/// Client messages and departures that may wait for the publishing task.
pub const INPUT_QUEUE: usize = 4096;
/// World events that may wait for the server loop.
pub const EVENT_QUEUE: usize = 4096;
/// Heartbeats that may wait for NATS.
const HEARTBEAT_QUEUE: usize = 16;
/// Metric: inputs dropped because the publishing task is behind.
pub const METRIC_INPUTS_DROPPED: &str = "rep_gateway_world_inputs_dropped_total";

/// What the world tells the server loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldEvent {
    /// Server messages for a connection, or a close.
    Output(GatewayOutput),
    /// The world is gone (silent or restarted); its players must be closed.
    Lost,
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
}

/// The link settings (config keys `gateway_id`, `sim_id`, `join_timeout_ms`,
/// `heartbeat_interval_ms`, `heartbeat_misses`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldLinkSettings {
    /// The world.
    pub world_id: Uuid,
    /// This gateway in bus subjects.
    pub gateway_id: BusId,
    /// The sim of the world.
    pub sim_id: BusId,
    /// Longest wait for a join answer.
    pub join_timeout: Duration,
    /// Time between heartbeats.
    pub heartbeat_interval: Duration,
    /// Missed sim heartbeats before the world is lost.
    pub heartbeat_misses: u32,
}

/// The world over NATS.
#[derive(Debug, Clone)]
pub struct BusWorld {
    bus: Bus,
    settings: Arc<WorldLinkSettings>,
    input_subject: String,
    inputs: Publisher<SimInput>,
}

impl BusWorld {
    /// Subscribes to this gateway's output and the sim heartbeat, and starts the tasks that
    /// publish inputs and heartbeats. Returns the link and its events for the server loop.
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
        let sim_beats = bus
            .subscribe::<Heartbeat>(subject::sim_heartbeat(world_id, &settings.sim_id))
            .await?;
        bus.flush().await?;
        let settings = Arc::new(settings);
        let (events, receiver) = mpsc::channel(EVENT_QUEUE);
        tokio::spawn(forward_outputs(outputs, events.clone()));
        tokio::spawn(heartbeats(
            Publisher::start(bus.clone(), HEARTBEAT_QUEUE),
            Arc::clone(&settings),
            sim_beats,
            events,
        ));
        Ok((
            Self {
                input_subject: subject::sim_input(world_id, &settings.sim_id),
                inputs: Publisher::start(bus.clone(), INPUT_QUEUE),
                bus,
                settings,
            },
            receiver,
        ))
    }

    fn connection(&self, connection: u64) -> ConnectionRef {
        ConnectionRef {
            gateway: self.settings.gateway_id.clone(),
            connection,
        }
    }

    fn input(&self, input: SimInput) {
        if !self.inputs.send(self.input_subject.clone(), input) {
            metrics::counter!(METRIC_INPUTS_DROPPED).increment(1);
        }
    }
}

impl World for BusWorld {
    async fn join(&self, connection: u64, record: &EntryRecord) -> Result<(), Refusal> {
        let join = Join {
            connection: self.connection(connection),
            account: record.account_id,
            character: record.character_id,
        };
        let subject = subject::sim_join(self.settings.world_id, &self.settings.sim_id);
        match self
            .bus
            .request::<Join, JoinReply>(subject, &join, self.settings.join_timeout)
            .await
        {
            Ok(JoinReply::Joined) => Ok(()),
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
        self.input(SimInput::ClientMessage {
            connection: self.connection(connection),
            envelope,
        });
    }

    fn left(&self, connection: u64) {
        self.input(SimInput::Left {
            connection: self.connection(connection),
        });
    }
}

async fn forward_outputs(
    mut outputs: Subscription<GatewayOutput>,
    events: mpsc::Sender<WorldEvent>,
) {
    while let Some(output) = outputs.next().await {
        match output {
            Ok(output) => {
                if events
                    .send(WorldEvent::Output(output.message))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => tracing::warn!(%error, "bad world output"),
        }
    }
}

/// Reports [`WorldEvent::Lost`] once when the sim misses `heartbeat_misses` beats, and when it
/// restarts, and queues this gateway's heartbeat. Ends when the server loop is gone.
async fn heartbeats(
    beats: Publisher<Heartbeat>,
    settings: Arc<WorldLinkSettings>,
    mut sim_beats: Subscription<Heartbeat>,
    events: mpsc::Sender<WorldEvent>,
) {
    let beat_subject = subject::gateway_heartbeat(settings.world_id, &settings.gateway_id);
    let own = Heartbeat {
        id: settings.gateway_id.clone(),
        started_at_ms: unix_ms(),
    };
    let silence = heartbeat_silence(settings.heartbeat_interval, settings.heartbeat_misses);
    let mut sim_started_at_ms: Option<u64> = None;
    let mut last_seen = Instant::now();
    let mut lost = false;
    let mut timer = tokio::time::interval(settings.heartbeat_interval);
    loop {
        let lost_now = tokio::select! {
            _ = timer.tick() => {
                if events.is_closed() {
                    return;
                }
                let silent = !lost && last_seen.elapsed() > silence;
                if silent {
                    tracing::warn!(sim = %settings.sim_id, "world silent; its players close");
                    lost = true;
                }
                if !beats.send(beat_subject.clone(), own.clone()) {
                    tracing::warn!("heartbeat dropped; NATS is behind");
                }
                silent
            }
            beat = sim_beats.next() => match beat {
                None => return,
                Some(Ok(beat)) => {
                    let started_at_ms = beat.message.started_at_ms;
                    let restarted = sim_started_at_ms.is_some_and(|known| known != started_at_ms);
                    if restarted {
                        tracing::warn!(sim = %settings.sim_id, "world restarted; its players close");
                    }
                    sim_started_at_ms = Some(started_at_ms);
                    last_seen = Instant::now();
                    lost = false;
                    restarted
                }
                Some(Err(error)) => {
                    tracing::warn!(%error, "bad sim heartbeat");
                    false
                }
            },
        };
        if lost_now && events.send(WorldEvent::Lost).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

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
            self.emit(WorldEvent::Output(GatewayOutput::ServerMessages {
                connection,
                channel: LOGIN_CHANNEL,
                reliable: true,
                envelopes: self.login.clone(),
            }))
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
    }

    /// The carrier channel of the login messages.
    const LOGIN_CHANNEL: u8 = 0;
}

#[cfg(all(test, feature = "infra"))]
mod bus_tests {
    use std::time::Duration;

    use nw_bus::test_support::test_bus;
    use nw_bus::{Heartbeat, Join, JoinReply, SimInput};
    use uuid::Uuid;

    use super::*;

    const JOIN_TIMEOUT: Duration = Duration::from_millis(500);
    const HEARTBEAT: Duration = Duration::from_millis(50);
    const MISSES: u32 = 2;
    /// Silence of 50 s: longer than any wait of these tests.
    const PATIENT_MISSES: u32 = 1000;
    /// Time for the link to handle the beats before the check.
    const SETTLE: Duration = Duration::from_millis(200);
    const WAIT: Duration = Duration::from_secs(5);
    const CONNECTION: u64 = 3;
    const SIM_STARTED_AT_MS: u64 = 10;
    const ENVELOPE: [u8; 2] = [4, 5];

    fn settings() -> WorldLinkSettings {
        let unique = Uuid::new_v4().simple().to_string();
        WorldLinkSettings {
            world_id: Uuid::new_v4(),
            gateway_id: BusId::try_from(format!("gw-{unique}")).expect("id"),
            sim_id: BusId::try_from(format!("sim-{unique}")).expect("id"),
            join_timeout: JOIN_TIMEOUT,
            heartbeat_interval: HEARTBEAT,
            heartbeat_misses: MISSES,
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

    async fn next<T>(receiver: &mut mpsc::Receiver<T>) -> T {
        tokio::time::timeout(WAIT, receiver.recv())
            .await
            .expect("in time")
            .expect("open")
    }

    /// A sim that answers every join with `reply`.
    async fn answer_joins(bus: &Bus, settings: &WorldLinkSettings, reply: JoinReply) {
        let mut joins = bus
            .subscribe::<Join>(subject::sim_join(settings.world_id, &settings.sim_id))
            .await
            .expect("subscribe");
        bus.flush().await.expect("flush");
        let answerer = bus.clone();
        tokio::spawn(async move {
            while let Some(Ok(join)) = joins.next().await {
                answerer.reply(&join, &reply).await.expect("reply");
            }
        });
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn joins_get_the_sim_answer_or_time_out() {
        let bus = test_bus().await;
        let settings = settings();
        let (world, _events) = BusWorld::start(bus.clone(), settings.clone())
            .await
            .expect("start");
        assert_eq!(
            world.join(CONNECTION, &record()).await,
            Err(Refusal::WorldUnavailable)
        );
        answer_joins(&bus, &settings, JoinReply::Joined).await;
        assert_eq!(world.join(CONNECTION, &record()).await, Ok(()));

        let refusing = self::settings();
        let (world, _events) = BusWorld::start(bus.clone(), refusing.clone())
            .await
            .expect("start");
        let reason = "no".to_owned();
        answer_joins(&bus, &refusing, JoinReply::Refused { reason }).await;
        assert_eq!(
            world.join(CONNECTION, &record()).await,
            Err(Refusal::WorldRefused)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_that_gets_no_answer_times_out() {
        let bus = test_bus().await;
        let settings = settings();
        let mut joins = bus
            .subscribe::<Join>(subject::sim_join(settings.world_id, &settings.sim_id))
            .await
            .expect("subscribe");
        bus.flush().await.expect("flush");
        tokio::spawn(async move { while joins.next().await.is_some() {} });
        let (world, _events) = BusWorld::start(bus, settings).await.expect("start");
        let started = std::time::Instant::now();
        assert_eq!(
            world.join(CONNECTION, &record()).await,
            Err(Refusal::WorldUnavailable)
        );
        assert!(started.elapsed() >= JOIN_TIMEOUT);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn inputs_go_to_the_sim_in_order_and_outputs_come_back() {
        let bus = test_bus().await;
        let settings = settings();
        let mut inputs = bus
            .subscribe::<SimInput>(subject::sim_input(settings.world_id, &settings.sim_id))
            .await
            .expect("subscribe");
        bus.flush().await.expect("flush");
        let (world, mut events) = BusWorld::start(bus.clone(), settings.clone())
            .await
            .expect("start");
        world.client_message(CONNECTION, ENVELOPE.to_vec());
        world.left(CONNECTION);
        let connection = ConnectionRef {
            gateway: settings.gateway_id.clone(),
            connection: CONNECTION,
        };
        let first = tokio::time::timeout(WAIT, inputs.next())
            .await
            .expect("in time");
        assert_eq!(
            first.expect("open").expect("decode").message,
            SimInput::ClientMessage {
                connection: connection.clone(),
                envelope: ENVELOPE.to_vec(),
            }
        );
        let second = tokio::time::timeout(WAIT, inputs.next())
            .await
            .expect("in time");
        assert_eq!(
            second.expect("open").expect("decode").message,
            SimInput::Left { connection }
        );
        let output = GatewayOutput::Close {
            connection: CONNECTION,
        };
        bus.publish(
            subject::gateway_output(settings.world_id, &settings.gateway_id),
            &output,
        )
        .await
        .expect("publish");
        assert_eq!(next(&mut events).await, WorldEvent::Output(output));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_gateway_beats_and_a_silent_sim_is_lost() {
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
        let (_world, mut events) = BusWorld::start(bus.clone(), settings.clone())
            .await
            .expect("start");
        let beat = tokio::time::timeout(WAIT, beats.next())
            .await
            .expect("in time");
        assert_eq!(
            beat.expect("open").expect("decode").message.id,
            settings.gateway_id
        );
        assert_eq!(next(&mut events).await, WorldEvent::Lost);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_restarted_sim_is_lost() {
        let bus = test_bus().await;
        let settings = WorldLinkSettings {
            heartbeat_misses: PATIENT_MISSES,
            ..settings()
        };
        let (_world, mut events) = BusWorld::start(bus.clone(), settings.clone())
            .await
            .expect("start");
        let sim_beat = |started_at_ms| Heartbeat {
            id: settings.sim_id.clone(),
            started_at_ms,
        };
        let subject = subject::sim_heartbeat(settings.world_id, &settings.sim_id);
        for started_at_ms in [SIM_STARTED_AT_MS, SIM_STARTED_AT_MS] {
            bus.publish(subject.clone(), &sim_beat(started_at_ms))
                .await
                .expect("publish");
        }
        tokio::time::sleep(SETTLE).await;
        assert!(events.try_recv().is_err());
        bus.publish(subject, &sim_beat(SIM_STARTED_AT_MS + 1))
            .await
            .expect("publish");
        assert_eq!(next(&mut events).await, WorldEvent::Lost);
    }
}
