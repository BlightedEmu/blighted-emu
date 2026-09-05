// SPDX-License-Identifier: AGPL-3.0-or-later
//! The loop of `sim` (M4 spec sections 3.1, 6, 7.1, 7.5, 11): bus joins and inputs, the 30 Hz
//! tick, position saves, and heartbeats. One task owns the world. Database work runs in its own
//! tasks and sends the result back, and bus messages go out through queues, so neither a slow
//! database nor a lost NATS connection stops the tick.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nw_bus::{
    Bus, BusError, BusId, ConnectionRef, GatewayOutput, Heartbeat, Incoming, Join, JoinReply,
    Publisher, SimInput,
};
use nw_common::time::unix_ms;
use nw_db::{
    PgPool, SaveOutcome, SavedPosition, character_by_id, character_position,
    save_character_position,
};
use nw_messages::{BodyCodec, ClientInputMsg, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX};
use nw_reflect::Envelope;
use nw_sim_core::{Ground, GroundCheck, Position, TickClock};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::gateways::GatewayTracker;
use crate::login::WorldIds;
use crate::saves::SaveQueue;
use crate::world::{Departure, MoveError, Outgoing, SimWorld, WorldSettings};

/// Time between checks of the tick clock.
pub const POLL: Duration = Duration::from_millis(5);
/// Unix milliseconds of 2000-01-01, the epoch of the server clock in time messages.
pub const SERVER_EPOCH_UNIX_MS: u64 = 946_684_800_000;
/// Results of database loads that may wait for the loop.
const RESULT_QUEUE: usize = 1024;
/// Gateway messages that may wait for NATS; more are dropped and counted.
pub const OUTPUT_QUEUE: usize = 4096;
/// Heartbeats that may wait for NATS.
const HEARTBEAT_QUEUE: usize = 16;
/// Longest wait at shutdown for the last saves.
pub const SHUTDOWN_SAVE_WAIT: Duration = Duration::from_secs(10);
/// Metric: players in the world.
pub const METRIC_PLAYERS: &str = "sim_players";
/// Metric: ticks that were due but not run on time.
pub const METRIC_LATE_TICKS: &str = "sim_late_ticks_total";
/// Metric: joins by `result`.
pub const METRIC_JOINS: &str = "sim_joins_total";
/// Metric: failed saves.
pub const METRIC_FAILED_SAVES: &str = "sim_failed_saves_total";
/// Metric: client messages of a type that `sim` does not read yet.
pub const METRIC_IGNORED_MESSAGES: &str = "sim_ignored_client_messages_total";
/// Metric: client messages that do not decode.
pub const METRIC_BAD_MESSAGES: &str = "sim_bad_client_messages_total";
/// Metric: refused moves by `reason`.
pub const METRIC_REFUSED_MOVES: &str = "sim_moves_refused_total";
/// Metric: accepted moves where the level has no height data.
pub const METRIC_MOVES_WITHOUT_GROUND: &str = "sim_moves_without_ground_total";
const REASON_LABEL: &str = "reason";
/// Metric: gateway messages dropped because NATS is behind.
pub const METRIC_DROPPED_OUTPUTS: &str = "sim_dropped_outputs_total";
const RESULT_LABEL: &str = "result";
const JOINED: &str = "joined";
const REFUSED: &str = "refused";
/// Yaw of a saved position; M4 has no facing yet.
const NO_YAW: f32 = 0.0;

/// What the loop needs from the configuration.
#[derive(Debug, Clone)]
pub struct LoopSettings {
    /// This sim.
    pub sim_id: BusId,
    /// The world.
    pub world_id: Uuid,
    /// The level of this cell.
    pub level: String,
    /// Where a character that was never in the world appears.
    pub start: Position,
    /// The world state settings.
    pub world: WorldSettings,
    /// Time between position saves.
    pub save_interval: Duration,
    /// Time between heartbeats.
    pub heartbeat_interval: Duration,
    /// Missed gateway heartbeats before its players leave.
    pub heartbeat_misses: u32,
}

/// The loop's failure.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The bus failed.
    #[error(transparent)]
    Bus(#[from] BusError),
    /// No random bytes for the world ids.
    #[error("no random bytes: {0}")]
    Random(#[from] getrandom::Error),
}

type JoinResult = (Incoming<Join>, Result<Position, String>);
type SaveResult = (Uuid, bool);

struct Loop {
    settings: LoopSettings,
    bus: Bus,
    outputs: Publisher<GatewayOutput>,
    beats: Publisher<Heartbeat>,
    db: PgPool,
    world: SimWorld,
    gateways: GatewayTracker,
    queue: SaveQueue,
    saves: JoinSet<SaveResult>,
    joins: mpsc::Sender<JoinResult>,
    started: Instant,
    started_at_ms: u64,
}

/// Serves joins, inputs, ticks, saves and heartbeats on the ground of `ground` until `shutdown`
/// finishes; then saves every player once more and waits at most [`SHUTDOWN_SAVE_WAIT`] for the
/// saves.
///
/// # Errors
///
/// Returns [`ServeError`] when a subscription cannot start.
pub async fn serve(
    settings: LoopSettings,
    ground: Arc<dyn Ground + Send + Sync>,
    bus: Bus,
    db: PgPool,
    shutdown: impl Future<Output = ()>,
) -> Result<(), ServeError> {
    let world_id = settings.world_id;
    let mut joins = bus
        .subscribe::<Join>(nw_bus::subject::sim_join(world_id, &settings.sim_id))
        .await?;
    let mut inputs = bus
        .subscribe::<SimInput>(nw_bus::subject::sim_input(world_id, &settings.sim_id))
        .await?;
    let mut beats = bus
        .subscribe::<Heartbeat>(nw_bus::subject::all_gateway_heartbeats(world_id))
        .await?;
    bus.flush().await?;
    let started_at_ms = unix_ms();
    let (join_results, mut joined) = mpsc::channel(RESULT_QUEUE);
    let mut state = Loop {
        world: SimWorld::new(settings.world, WorldIds::random(started_at_ms)?, ground),
        gateways: GatewayTracker::new(settings.heartbeat_interval, settings.heartbeat_misses),
        outputs: Publisher::start(bus.clone(), OUTPUT_QUEUE),
        beats: Publisher::start(bus.clone(), HEARTBEAT_QUEUE),
        settings,
        bus,
        db,
        queue: SaveQueue::default(),
        saves: JoinSet::new(),
        joins: join_results,
        started: Instant::now(),
        started_at_ms,
    };
    let mut clock = TickClock::new(0);
    let mut poll = tokio::time::interval(POLL);
    let mut save_timer = tokio::time::interval(state.settings.save_interval);
    let mut beat_timer = tokio::time::interval(state.settings.heartbeat_interval);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            Some(join) = joins.next() => match join {
                Ok(join) => state.load(join),
                Err(error) => tracing::warn!(%error, "bad join message"),
            },
            Some((join, result)) = joined.recv() => state.complete_join(join, result).await,
            Some(input) = inputs.next() => match input {
                Ok(input) => state.input(input.message),
                Err(error) => tracing::warn!(%error, "bad input message"),
            },
            Some(beat) = beats.next() => match beat {
                Ok(beat) => state.gateway_beat(&beat.message),
                Err(error) => tracing::warn!(%error, "bad heartbeat"),
            },
            Some(saved) = state.saves.join_next() => state.saved(saved),
            _ = poll.tick() => state.tick(&mut clock),
            _ = save_timer.tick() => state.save_all(),
            _ = beat_timer.tick() => state.heartbeat(),
        }
    }
    state.finish().await;
    Ok(())
}

impl Loop {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Loads the character in its own task.
    fn load(&self, join: Incoming<Join>) {
        let db = self.db.clone();
        let level = self.settings.level.clone();
        let start = self.settings.start;
        let results = self.joins.clone();
        tokio::spawn(async move {
            let result = load_position(&db, &join.message, &level, start).await;
            // The loop may have stopped; then nobody waits for the result.
            let _ = results.send((join, result)).await;
        });
    }

    /// Adds the loaded character. A position that is still waiting to be saved is newer than the
    /// database, so it wins.
    async fn complete_join(&mut self, join: Incoming<Join>, result: Result<Position, String>) {
        let message = &join.message;
        let unsaved = self.queue.latest(message.character).map(|saved| Position {
            // f64 to f32: world positions are below 2^24 metres.
            x: saved.x as f32,
            y: saved.y as f32,
            z: saved.z as f32,
        });
        let joined = result.and_then(|loaded| {
            self.world
                .join(
                    message.connection.clone(),
                    message.character,
                    unsaved.unwrap_or(loaded),
                    self.now_ms(),
                    server_time(),
                )
                .map_err(|error| error.to_string())
        });
        let reply = match joined {
            Ok((entity, login)) => {
                metrics::counter!(METRIC_JOINS, RESULT_LABEL => JOINED).increment(1);
                tracing::info!(character = %message.character, "joined");
                self.gateways
                    .joined(&message.connection.gateway, Instant::now());
                self.send(login);
                JoinReply::Joined {
                    sim: self.settings.sim_id.clone(),
                    generation: self.started_at_ms,
                    entity,
                }
            }
            Err(reason) => {
                metrics::counter!(METRIC_JOINS, RESULT_LABEL => REFUSED).increment(1);
                tracing::warn!(character = %message.character, %reason, "join refused");
                JoinReply::Refused { reason }
            }
        };
        if let Err(error) = self.bus.reply(&join, &reply).await {
            tracing::warn!(%error, "join reply failed");
        }
        metrics::gauge!(METRIC_PLAYERS).set(self.world.players() as f64);
    }

    fn input(&mut self, input: SimInput) {
        match input {
            SimInput::ClientMessage {
                connection,
                envelope,
            } => self.client_message(&connection, &envelope),
            SimInput::Left { connection } => {
                if let Some(departure) = self.world.leave(&connection) {
                    self.save(&departure);
                }
                metrics::gauge!(METRIC_PLAYERS).set(self.world.players() as f64);
            }
        }
    }

    /// Reads one client message. Type 1589 moves the player to the position of its last record
    /// that has one (M4 spec section 5.3); every other type is counted and ignored. Bad bytes
    /// are counted and never close the connection.
    fn client_message(&mut self, connection: &ConnectionRef, bytes: &[u8]) {
        let Ok(envelope) = Envelope::decode(bytes) else {
            metrics::counter!(METRIC_BAD_MESSAGES).increment(1);
            return;
        };
        if envelope.type_index() != Some(RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX) {
            metrics::counter!(METRIC_IGNORED_MESSAGES).increment(1);
            return;
        }
        let Ok(input) = ClientInputMsg::decode(&envelope.body) else {
            metrics::counter!(METRIC_BAD_MESSAGES).increment(1);
            return;
        };
        let Some(position) = input.last_position() else {
            return;
        };
        let next = Position {
            x: position.x,
            y: position.y,
            z: position.z(),
        };
        match self.world.move_to(connection, next, self.now_ms()) {
            Ok(GroundCheck::Checked) | Err(MoveError::NotInWorld) => {}
            Ok(GroundCheck::NoGround) => {
                metrics::counter!(METRIC_MOVES_WITHOUT_GROUND).increment(1);
            }
            Err(MoveError::Refused(refusal)) => {
                metrics::counter!(METRIC_REFUSED_MOVES, REASON_LABEL => refusal.reason())
                    .increment(1);
            }
        }
    }

    fn gateway_beat(&mut self, beat: &Heartbeat) {
        if self.gateways.beat(beat, Instant::now()) {
            tracing::warn!(gateway = %beat.id, "gateway restarted; its players leave");
            self.drop_gateway(&beat.id);
        }
    }

    /// Saves and removes every player of `gateway`, and tells the gateway to close them.
    fn drop_gateway(&mut self, gateway: &BusId) {
        for departure in self.world.leave_gateway(gateway) {
            self.save(&departure);
            self.publish(
                gateway,
                GatewayOutput::Close {
                    connection: departure.connection.connection,
                },
            );
        }
        metrics::gauge!(METRIC_PLAYERS).set(self.world.players() as f64);
    }

    /// Runs the world once when ticks are due. The world changes only on joins, inputs and
    /// departures, so a late call sends the current state once instead of every missed tick.
    fn tick(&mut self, clock: &mut TickClock) {
        let now_ms = self.now_ms();
        let now_us = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        let due = clock.due(now_us);
        if due.count == 0 {
            return;
        }
        let missed = due.count - 1;
        if missed > 0 {
            metrics::counter!(METRIC_LATE_TICKS).increment(missed);
            tracing::warn!(missed, "ticks ran late");
        }
        match self.world.tick(now_ms, server_time()) {
            Ok(outgoing) => {
                for message in outgoing {
                    self.send(message);
                }
            }
            Err(error) => tracing::error!(%error, "tick failed"),
        }
    }

    fn heartbeat(&mut self) {
        let beat = Heartbeat {
            id: self.settings.sim_id.clone(),
            started_at_ms: self.started_at_ms,
        };
        let subject = nw_bus::subject::sim_heartbeat(self.settings.world_id, &self.settings.sim_id);
        if !self.beats.send(subject, beat) {
            tracing::warn!("heartbeat dropped; NATS is behind");
        }
        for gateway in self.gateways.silent(Instant::now()) {
            tracing::warn!(%gateway, "gateway silent; its players leave");
            self.drop_gateway(&gateway);
        }
    }

    fn send(&self, outgoing: Outgoing) {
        self.publish(
            &outgoing.connection.gateway,
            GatewayOutput::ServerMessages {
                connection: outgoing.connection.connection,
                channel: outgoing.channel,
                reliable: outgoing.reliable,
                envelopes: outgoing.envelopes,
            },
        );
    }

    fn publish(&self, gateway: &BusId, message: GatewayOutput) {
        let subject = nw_bus::subject::gateway_output(self.settings.world_id, gateway);
        if !self.outputs.send(subject, message) {
            metrics::counter!(METRIC_DROPPED_OUTPUTS).increment(1);
        }
    }

    /// Saves every player and retries every save that failed before.
    fn save_all(&mut self) {
        for departure in self.world.positions() {
            self.save(&departure);
        }
        for (character, position) in self.queue.retry() {
            self.spawn_save(character, position);
        }
    }

    fn save(&mut self, departure: &Departure) {
        let position = SavedPosition {
            level: self.settings.level.clone(),
            x: f64::from(departure.position.x),
            y: f64::from(departure.position.y),
            z: f64::from(departure.position.z),
            yaw: NO_YAW,
            saved_at_ms: unix_ms(),
        };
        if let Some(position) = self.queue.want(departure.character, position) {
            self.spawn_save(departure.character, position);
        }
    }

    fn spawn_save(&mut self, character: Uuid, position: SavedPosition) {
        let db = self.db.clone();
        self.saves.spawn(async move {
            // A newer stored position is a finished save too.
            let ok = match save_character_position(&db, character, &position).await {
                Ok(SaveOutcome::Saved | SaveOutcome::Older) => true,
                Err(error) => {
                    tracing::error!(%error, %character, "position save failed");
                    false
                }
            };
            (character, ok)
        });
    }

    /// A save ended; the next wanted position of that character starts now.
    fn saved(&mut self, result: Result<SaveResult, tokio::task::JoinError>) {
        let (character, ok) = match result {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(%error, "save task failed");
                return;
            }
        };
        if !ok {
            metrics::counter!(METRIC_FAILED_SAVES).increment(1);
        }
        if let Some(next) = self.queue.finished(character, ok) {
            self.spawn_save(character, next);
        }
    }

    /// Saves every player once more and waits at most [`SHUTDOWN_SAVE_WAIT`] for every save.
    async fn finish(&mut self) {
        self.save_all();
        let drained = tokio::time::timeout(SHUTDOWN_SAVE_WAIT, async {
            while let Some(saved) = self.saves.join_next().await {
                self.saved(saved);
            }
        })
        .await;
        if drained.is_err() || self.queue.waiting() > 0 {
            tracing::error!(
                waiting = self.queue.waiting(),
                "positions not saved at shutdown"
            );
        }
    }
}

/// The position a joining character appears at: its saved position on this level, or `start`.
async fn load_position(
    db: &PgPool,
    join: &Join,
    level: &str,
    start: Position,
) -> Result<Position, String> {
    let character = character_by_id(db, join.character)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("no such character")?;
    if character.account_id != join.account {
        return Err("the character belongs to another account".to_owned());
    }
    let saved = character_position(db, join.character)
        .await
        .map_err(|error| error.to_string())?;
    Ok(match saved {
        // f64 to f32: world positions are below 2^24 metres.
        Some(saved) if saved.level == level => Position {
            x: saved.x as f32,
            y: saved.y as f32,
            z: saved.z as f32,
        },
        _ => start,
    })
}

/// Milliseconds since 2000-01-01. The unit of the retail `TimeSynchMsg` is Unknown.
fn server_time() -> u64 {
    unix_ms().saturating_sub(SERVER_EPOCH_UNIX_MS)
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use nw_bus::test_support::test_bus;
    use nw_bus::{ConnectionRef, GatewayOutput, Subscription};
    use nw_db::test_db::with_test_schema;
    use nw_db::{NewCharacter, WORLD_MIGRATOR, insert_character};
    use nw_messages::{
        BundlePayload, InputPosition, LOGIN_MESSAGE_TYPES, MemberState, ServerMessage,
    };
    use nw_reflect::TypeId;
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;

    use super::*;
    use nw_sim_core::test_support::NoGround;

    use crate::config::tests::committed;
    use crate::world::{MESSAGE_CHANNEL, REPLICATION_CHANNEL};

    const WAIT: Duration = Duration::from_secs(5);
    const LEVEL: &str = "newworld_vitaeeterna";
    const CONNECTION: u64 = 9;
    const SAVE_INTERVAL: Duration = Duration::from_secs(10);
    const HEARTBEAT: Duration = Duration::from_secs(1);
    const MISSES: u32 = 3;
    /// A gateway that never beats is silent after 0.2 s.
    const SHORT_HEARTBEAT: Duration = Duration::from_millis(100);
    const SHORT_MISSES: u32 = 2;
    /// The sim subscribes at start; a join may come before that.
    const TRIES: usize = 50;
    const PAUSE: Duration = Duration::from_millis(100);
    const SAVE_TOLERANCE_M: f64 = 0.01;
    const CHARACTER_NAME: &str = "Ada";
    /// A step the limits allow after [`STEP_WAIT`].
    const STEP_M: f32 = 2.0;
    const STEP_WAIT: Duration = Duration::from_millis(500);
    const MOVE_TOLERANCE_M: f32 = 0.01;

    fn start() -> Position {
        committed().start_position.position()
    }

    fn settings(world_id: Uuid) -> LoopSettings {
        LoopSettings {
            sim_id: BusId::try_from(format!("sim-{}", Uuid::new_v4().simple())).expect("id"),
            world_id,
            level: LEVEL.to_owned(),
            start: start(),
            world: committed().world_settings().expect("world settings"),
            save_interval: SAVE_INTERVAL,
            heartbeat_interval: HEARTBEAT,
            heartbeat_misses: MISSES,
        }
    }

    /// A character of a new account: (account, character).
    async fn character(db: &PgPool) -> (Uuid, Uuid) {
        let account = Uuid::new_v4();
        let character = insert_character(
            db,
            &NewCharacter {
                id: Uuid::new_v4(),
                account_id: account,
                name: CHARACTER_NAME.to_owned(),
                appearance: serde_json::json!({}),
            },
        )
        .await
        .expect("character");
        (account, character.id)
    }

    /// A running sim and the sender that stops it.
    struct Running {
        bus: Bus,
        settings: LoopSettings,
        outputs: Subscription<GatewayOutput>,
        stop: oneshot::Sender<()>,
        server: JoinHandle<Result<(), ServeError>>,
    }

    async fn run(settings: LoopSettings, gateway: &BusId, db: &PgPool) -> Running {
        let bus = test_bus().await;
        let outputs = bus
            .subscribe::<GatewayOutput>(nw_bus::subject::gateway_output(settings.world_id, gateway))
            .await
            .expect("outputs");
        bus.flush().await.expect("flush");
        let (stop, stopped) = oneshot::channel::<()>();
        let server = tokio::spawn(serve(
            settings.clone(),
            Arc::new(NoGround),
            bus.clone(),
            db.clone(),
            async {
                let _ = stopped.await;
            },
        ));
        Running {
            bus,
            settings,
            outputs,
            stop,
            server,
        }
    }

    impl Running {
        /// Sends `join` until the sim answers.
        async fn join(&self, join: &Join) -> JoinReply {
            let subject = nw_bus::subject::sim_join(self.settings.world_id, &self.settings.sim_id);
            let mut reply: Result<JoinReply, BusError> = Err(BusError::NoReplySubject);
            for _ in 0..TRIES {
                reply = self.bus.request(subject.clone(), join, WAIT).await;
                if reply.is_ok() {
                    break;
                }
                tokio::time::sleep(PAUSE).await;
            }
            reply.expect("join reply")
        }

        async fn next_output(&mut self) -> GatewayOutput {
            tokio::time::timeout(WAIT, self.outputs.next())
                .await
                .expect("output in time")
                .expect("open")
                .expect("decode")
                .message
        }

        async fn next_messages(&mut self) -> (u8, bool, Vec<Vec<u8>>) {
            match self.next_output().await {
                GatewayOutput::ServerMessages {
                    channel,
                    reliable,
                    envelopes,
                    ..
                } => (channel, reliable, envelopes),
                GatewayOutput::Close { .. } => panic!("unexpected close"),
            }
        }

        async fn stop(self) {
            let _ = self.stop.send(());
            self.server.await.expect("join").expect("serve");
        }
    }

    fn gateway() -> BusId {
        BusId::try_from(format!("gw-{}", Uuid::new_v4().simple())).expect("id")
    }

    async fn saved_position(db: &PgPool, character: Uuid) -> SavedPosition {
        for _ in 0..TRIES {
            if let Some(saved) = character_position(db, character).await.expect("load") {
                return saved;
            }
            tokio::time::sleep(PAUSE).await;
        }
        panic!("no saved position");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_gets_login_messages_bundles_and_a_saved_position() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let (account, character) = character(db).await;
            let gateway = gateway();
            let mut sim = run(settings(Uuid::new_v4()), &gateway, db).await;
            let connection = ConnectionRef {
                gateway,
                connection: CONNECTION,
            };
            let join = |account| Join {
                connection: connection.clone(),
                account,
                character,
                resume: None,
            };
            assert!(matches!(
                sim.join(&join(account)).await,
                JoinReply::Joined { .. }
            ));
            let (channel, reliable, login) = sim.next_messages().await;
            assert_eq!(
                (channel, reliable, login.len()),
                (MESSAGE_CHANNEL, true, LOGIN_MESSAGE_TYPES.len())
            );
            let (channel, reliable, bundle) = sim.next_messages().await;
            assert_eq!((channel, reliable), (REPLICATION_CHANNEL, false));
            let envelope = Envelope::decode(&bundle[0]).expect("envelope");
            let Some(Ok(ServerMessage::Bundle(bundle))) = ServerMessage::from_envelope(&envelope)
            else {
                panic!("expected a bundle");
            };
            let payload = BundlePayload::decode(&bundle.payload).expect("payload");
            assert_eq!(payload.groups[0].members.len(), 1);
            let other_account = Join {
                connection: ConnectionRef {
                    gateway: connection.gateway.clone(),
                    connection: CONNECTION + 1,
                },
                ..join(Uuid::new_v4())
            };
            assert!(matches!(
                sim.join(&other_account).await,
                JoinReply::Refused { .. }
            ));
            sim.bus
                .publish(
                    nw_bus::subject::sim_input(sim.settings.world_id, &sim.settings.sim_id),
                    &SimInput::Left {
                        connection: connection.clone(),
                    },
                )
                .await
                .expect("left");
            let saved = saved_position(db, character).await;
            assert_eq!(saved.level, LEVEL);
            assert!((saved.x - f64::from(start().x)).abs() < SAVE_TOLERANCE_M);
            sim.stop().await;
        })
        .await;
    }

    /// The own x of the next bundle on the replication channel.
    async fn next_own_x(sim: &mut Running) -> f32 {
        loop {
            let (channel, _, envelopes) = sim.next_messages().await;
            if channel != REPLICATION_CHANNEL {
                continue;
            }
            let envelope = Envelope::decode(&envelopes[0]).expect("envelope");
            let Some(Ok(ServerMessage::Bundle(bundle))) = ServerMessage::from_envelope(&envelope)
            else {
                panic!("expected a bundle");
            };
            let payload = BundlePayload::decode(&bundle.payload).expect("payload");
            let MemberState::Position(state) = &payload.groups[0].members[0].state;
            return state.position.expect("position").x;
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_client_input_moves_the_player() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let (account, character) = character(db).await;
            let gateway = gateway();
            let mut sim = run(settings(Uuid::new_v4()), &gateway, db).await;
            let connection = ConnectionRef {
                gateway,
                connection: CONNECTION,
            };
            let join = Join {
                connection: connection.clone(),
                account,
                character,
                resume: None,
            };
            assert!(matches!(sim.join(&join).await, JoinReply::Joined { .. }));
            let start = start();
            let target =
                InputPosition::from_metres(start.x + STEP_M, start.y, start.z).expect("finite");
            let body = ClientInputMsg::position(target).encode().expect("input");
            let envelope =
                Envelope::message(TypeId::FileSlot(RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX), body)
                    .encode()
                    .expect("envelope");
            tokio::time::sleep(STEP_WAIT).await;
            sim.bus
                .publish(
                    nw_bus::subject::sim_input(sim.settings.world_id, &sim.settings.sim_id),
                    &SimInput::ClientMessage {
                        connection,
                        envelope,
                    },
                )
                .await
                .expect("input");
            let moved =
                async { while (next_own_x(&mut sim).await - target.x).abs() > MOVE_TOLERANCE_M {} };
            tokio::time::timeout(WAIT, moved)
                .await
                .expect("moved in time");
            sim.stop().await;
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_last_saves_finish_before_serve_returns() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let (account, character) = character(db).await;
            let gateway = gateway();
            let sim = run(settings(Uuid::new_v4()), &gateway, db).await;
            let join = Join {
                connection: ConnectionRef {
                    gateway,
                    connection: CONNECTION,
                },
                account,
                character,
                resume: None,
            };
            assert!(matches!(sim.join(&join).await, JoinReply::Joined { .. }));
            sim.stop().await;
            let saved = character_position(db, character).await.expect("load");
            assert!(saved.is_some(), "the shutdown save is missing");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_silent_gateway_loses_its_players_and_hears_close() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let (account, character) = character(db).await;
            let gateway = gateway();
            let settings = LoopSettings {
                heartbeat_interval: SHORT_HEARTBEAT,
                heartbeat_misses: SHORT_MISSES,
                ..settings(Uuid::new_v4())
            };
            let mut sim = run(settings, &gateway, db).await;
            let join = Join {
                connection: ConnectionRef {
                    gateway,
                    connection: CONNECTION,
                },
                account,
                character,
                resume: None,
            };
            assert!(matches!(sim.join(&join).await, JoinReply::Joined { .. }));
            let close = async {
                loop {
                    if let GatewayOutput::Close { connection } = sim.next_output().await {
                        return connection;
                    }
                }
            };
            let closed = tokio::time::timeout(WAIT, close)
                .await
                .expect("close in time");
            assert_eq!(closed, CONNECTION);
            saved_position(db, character).await;
            sim.stop().await;
        })
        .await;
    }
}
