// SPDX-License-Identifier: AGPL-3.0-or-later
//! The loop of `sim` (M4 spec sections 6, 7.1, 7.5, 11; M5 spec sections 3, 7): admits, inputs,
//! the cell table, handoffs, ghosts, the 30 Hz tick, position saves, and heartbeats. One task
//! owns the world. Database work and handoff requests run in their own tasks and send the result
//! back, and bus messages go out through queues, so neither a slow database, a slow sim nor a lost
//! NATS connection stops the tick.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nw_bus::{
    Admit, AdmitReply, Bus, BusError, BusId, CellTable, ConnectionRef, GatewayOutput, Ghosts,
    HandoffMessage, HandoffReply, Heartbeat, Incoming, Publisher, SimHeartbeat, SimInput, SimSlot,
    subject,
};
use nw_common::time::unix_ms;
use nw_db::{PgPool, SaveOutcome, SavedPosition, save_character_position};
use nw_messages::{BodyCodec, ClientInputMsg, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX};
use nw_reflect::Envelope;
use nw_sim_core::{Ground, GroundCheck, Position, TickClock};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::forward::ForwardTable;
use crate::gateways::GatewayTracker;
use crate::login::WorldIds;
use crate::saves::SaveQueue;
use crate::world::{
    Departure, HandoffEnd, HandoffStart, MoveError, Outgoing, SimWorld, WorldSettings,
};

/// Time between checks of the tick clock.
pub const POLL: Duration = Duration::from_millis(5);
/// Unix milliseconds of 2000-01-01, the epoch of the server clock in time messages.
pub const SERVER_EPOCH_UNIX_MS: u64 = 946_684_800_000;
/// Handoff answers that may wait for the loop.
const RESULT_QUEUE: usize = 1024;
/// Gateway messages that may wait for NATS; more are dropped and counted.
pub const OUTPUT_QUEUE: usize = 4096;
/// Ghost lists, forwarded inputs and cancels that may wait for NATS.
const SIM_QUEUE: usize = 4096;
/// Heartbeats that may wait for NATS.
const HEARTBEAT_QUEUE: usize = 16;
/// Longest wait at shutdown for the last saves.
pub const SHUTDOWN_SAVE_WAIT: Duration = Duration::from_secs(10);
/// One tick at 30 Hz, rounded down to whole milliseconds: the M5 limit of a handoff and a tick.
pub const ONE_TICK_SECONDS: f64 = 0.033;
/// Buckets of the handoff and tick time histograms, in seconds (M5 spec section 7.6).
pub const LATENCY_BUCKETS_SECONDS: [f64; 9] = [
    0.001,
    0.002,
    0.005,
    0.010,
    0.020,
    ONE_TICK_SECONDS,
    0.050,
    0.100,
    0.250,
];
/// Metric: players in the world.
pub const METRIC_PLAYERS: &str = "sim_players";
/// Metric: ghosts of other sims.
pub const METRIC_GHOSTS: &str = "sim_ghosts";
/// Metric: ticks that were due but not run on time.
pub const METRIC_LATE_TICKS: &str = "sim_late_ticks_total";
/// Metric: time of one world tick.
pub const METRIC_TICK_SECONDS: &str = "sim_tick_seconds";
/// Metric: admits by `result`.
pub const METRIC_ADMITS: &str = "sim_admits_total";
/// Metric: handoffs by `result`.
pub const METRIC_HANDOFFS: &str = "sim_handoffs_total";
/// Metric: time from the freeze to the answer of a handoff.
pub const METRIC_HANDOFF_SECONDS: &str = "sim_handoff_seconds";
/// Metric: inputs sent on to the sim that took their player.
pub const METRIC_FORWARDED_INPUTS: &str = "sim_forwarded_inputs_total";
/// Metric: failed saves.
pub const METRIC_FAILED_SAVES: &str = "sim_failed_saves_total";
/// Metric: client messages of a type that `sim` does not read yet.
pub const METRIC_IGNORED_MESSAGES: &str = "sim_ignored_client_messages_total";
/// Metric: client messages that do not decode.
pub const METRIC_BAD_MESSAGES: &str = "sim_bad_client_messages_total";
/// Metric: refused moves by `reason`.
pub const METRIC_REFUSED_MOVES: &str = "sim_moves_refused_total";
/// Metric: moves of a player frozen for its handoff.
pub const METRIC_FROZEN_MOVES: &str = "sim_moves_while_frozen_total";
/// Metric: accepted moves where the level has no height data.
pub const METRIC_MOVES_WITHOUT_GROUND: &str = "sim_moves_without_ground_total";
/// Metric: bus messages dropped because NATS is behind.
pub const METRIC_DROPPED_OUTPUTS: &str = "sim_dropped_outputs_total";
const REASON_LABEL: &str = "reason";
/// Label of the handoff result.
pub const RESULT_LABEL: &str = "result";
const ADMITTED: &str = "admitted";
const REFUSED: &str = "refused";
/// Handoff result: the other sim took the player.
pub const HANDOFF_MOVED: &str = "moved";
/// Handoff result: the other sim refused the player.
pub const HANDOFF_REFUSED: &str = "refused";
/// Handoff result: the other sim did not answer in time.
pub const HANDOFF_TIMEOUT: &str = "timeout";
const HANDOFF_GONE: &str = "gone";
/// Yaw of a saved position; the sim has no facing yet.
const NO_YAW: f32 = 0.0;

/// What the loop needs from the configuration.
#[derive(Debug, Clone)]
pub struct LoopSettings {
    /// This sim.
    pub sim_id: BusId,
    /// The world.
    pub world_id: Uuid,
    /// The open map level.
    pub level: String,
    /// The world state settings.
    pub world: WorldSettings,
    /// Longest wait for the answer to a handoff.
    pub handoff_timeout: Duration,
    /// How long inputs of a handed-off connection go on to the new sim, in milliseconds.
    pub forward_ttl_ms: u64,
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

type SaveResult = (Uuid, bool);

/// The answer to one handoff offer.
struct HandoffResult {
    connection: ConnectionRef,
    target: SimSlot,
    sequence: u64,
    reply: Result<HandoffReply, BusError>,
    elapsed: Duration,
}

/// The subscriptions of one sim.
struct Inbox {
    admits: nw_bus::Subscription<Admit>,
    inputs: nw_bus::Subscription<SimInput>,
    handoffs: nw_bus::Subscription<HandoffMessage>,
    ghosts: nw_bus::Subscription<Ghosts>,
    tables: nw_bus::Subscription<CellTable>,
    beats: nw_bus::Subscription<Heartbeat>,
}

impl Inbox {
    async fn subscribe(bus: &Bus, world_id: Uuid, sim: &BusId) -> Result<Self, BusError> {
        let inbox = Self {
            admits: bus.subscribe(subject::sim_admit(world_id, sim)).await?,
            inputs: bus.subscribe(subject::sim_input(world_id, sim)).await?,
            handoffs: bus.subscribe(subject::sim_handoff(world_id, sim)).await?,
            ghosts: bus.subscribe(subject::sim_ghosts(world_id, sim)).await?,
            tables: bus.subscribe(subject::director_table(world_id)).await?,
            beats: bus
                .subscribe(subject::all_gateway_heartbeats(world_id))
                .await?,
        };
        bus.flush().await?;
        Ok(inbox)
    }
}

struct Loop {
    settings: LoopSettings,
    bus: Bus,
    outputs: Publisher<GatewayOutput>,
    sims: Publisher<SimInput>,
    ghosts: Publisher<Ghosts>,
    cancels: Publisher<HandoffMessage>,
    beats: Publisher<SimHeartbeat>,
    db: PgPool,
    world: SimWorld,
    gateways: GatewayTracker,
    forwards: ForwardTable,
    queue: SaveQueue,
    saves: JoinSet<SaveResult>,
    handoff_results: mpsc::Sender<HandoffResult>,
    started: Instant,
    started_at_ms: u64,
}

/// Serves admits, inputs, handoffs, ghosts, the cell table, ticks, saves and heartbeats on the
/// ground of `ground` until `shutdown` finishes; then saves every player once more and waits at
/// most [`SHUTDOWN_SAVE_WAIT`] for the saves.
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
    let mut inbox = Inbox::subscribe(&bus, settings.world_id, &settings.sim_id).await?;
    let started_at_ms = unix_ms();
    let me = SimSlot {
        id: settings.sim_id.clone(),
        generation: started_at_ms,
    };
    let (handoff_results, mut answers) = mpsc::channel(RESULT_QUEUE);
    let mut state = Loop {
        world: SimWorld::new(settings.world, WorldIds::random(started_at_ms)?, ground, me),
        gateways: GatewayTracker::new(settings.heartbeat_interval, settings.heartbeat_misses),
        forwards: ForwardTable::new(settings.forward_ttl_ms),
        outputs: Publisher::start(bus.clone(), OUTPUT_QUEUE),
        sims: Publisher::start(bus.clone(), SIM_QUEUE),
        ghosts: Publisher::start(bus.clone(), SIM_QUEUE),
        cancels: Publisher::start(bus.clone(), SIM_QUEUE),
        beats: Publisher::start(bus.clone(), HEARTBEAT_QUEUE),
        settings,
        bus,
        db,
        queue: SaveQueue::default(),
        saves: JoinSet::new(),
        handoff_results,
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
            Some(admit) = inbox.admits.next() => match admit {
                Ok(admit) => state.admit(admit).await,
                Err(error) => tracing::warn!(%error, "bad admit message"),
            },
            Some(input) = inbox.inputs.next() => match input {
                Ok(input) => state.input(input.message),
                Err(error) => tracing::warn!(%error, "bad input message"),
            },
            Some(handoff) = inbox.handoffs.next() => match handoff {
                Ok(handoff) => state.handoff(handoff).await,
                Err(error) => tracing::warn!(%error, "bad handoff message"),
            },
            Some(ghosts) = inbox.ghosts.next() => match ghosts {
                Ok(ghosts) => state.ghosts_in(ghosts.message),
                Err(error) => tracing::warn!(%error, "bad ghost message"),
            },
            Some(table) = inbox.tables.next() => match table {
                Ok(table) => state.table(&table.message),
                Err(error) => tracing::warn!(%error, "bad cell table"),
            },
            Some(beat) = inbox.beats.next() => match beat {
                Ok(beat) => state.gateway_beat(&beat.message),
                Err(error) => tracing::warn!(%error, "bad heartbeat"),
            },
            Some(answer) = answers.recv() => state.handoff_done(answer),
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

    /// Adds the player of an admit. A position of this sim that is still waiting to be saved is
    /// newer than the database, so it wins.
    async fn admit(&mut self, admit: Incoming<Admit>) {
        let mut message = admit.message.clone();
        if let Some(unsaved) = self.queue.latest(message.character) {
            // f64 to f32: world positions are below 2^24 metres.
            message.position.x = unsaved.x as f32;
            message.position.y = unsaved.y as f32;
            message.position.z = unsaved.z as f32;
        }
        let reply = match self.world.admit(&message, self.now_ms(), server_time()) {
            Ok(login) => {
                metrics::counter!(METRIC_ADMITS, RESULT_LABEL => ADMITTED).increment(1);
                tracing::info!(character = %message.character, resumed = !message.login, "joined");
                self.gateways
                    .joined(&message.connection.gateway, Instant::now());
                self.forwards.remove(&message.connection);
                if let Some(login) = login {
                    self.send(login);
                }
                AdmitReply::Admitted
            }
            Err(error) => {
                metrics::counter!(METRIC_ADMITS, RESULT_LABEL => REFUSED).increment(1);
                tracing::warn!(character = %message.character, %error, "admit refused");
                AdmitReply::Refused {
                    reason: error.to_string(),
                }
            }
        };
        if let Err(error) = self.bus.reply(&admit, &reply).await {
            tracing::warn!(%error, "admit reply failed");
        }
        self.count_players();
    }

    /// A client message or a departure. Those of a handed-off connection go on to its new sim.
    fn input(&mut self, input: SimInput) {
        let connection = match &input {
            SimInput::ClientMessage { connection, .. } | SimInput::Left { connection } => {
                connection.clone()
            }
        };
        if let Some(sim) = self.forwards.target(&connection, self.now_ms()).cloned() {
            if matches!(input, SimInput::Left { .. }) {
                self.forwards.remove(&connection);
            }
            metrics::counter!(METRIC_FORWARDED_INPUTS).increment(1);
            let subject = subject::sim_input(self.settings.world_id, &sim);
            if !self.sims.send(subject, input) {
                metrics::counter!(METRIC_DROPPED_OUTPUTS).increment(1);
            }
            return;
        }
        match input {
            SimInput::ClientMessage {
                connection,
                envelope,
            } => self.client_message(&connection, &envelope),
            SimInput::Left { connection } => {
                if let Some(departure) = self.world.leave(&connection) {
                    self.save(&departure);
                }
                self.count_players();
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
            Err(MoveError::Frozen) => metrics::counter!(METRIC_FROZEN_MOVES).increment(1),
            Err(MoveError::Refused(refusal)) => {
                metrics::counter!(METRIC_REFUSED_MOVES, REASON_LABEL => refusal.reason())
                    .increment(1);
            }
        }
    }

    /// A handoff offer from another sim, or the cancel of one that this sim took too late.
    async fn handoff(&mut self, handoff: Incoming<HandoffMessage>) {
        match &handoff.message {
            HandoffMessage::Offer(offer) => {
                let reply = match self.world.take(offer, self.now_ms()) {
                    Ok(()) => {
                        self.forwards.remove(&offer.connection);
                        self.gateways
                            .joined(&offer.connection.gateway, Instant::now());
                        HandoffReply::Accepted
                    }
                    Err(error) => {
                        tracing::warn!(character = %offer.character, %error, "handoff refused");
                        HandoffReply::Refused {
                            reason: error.to_string(),
                        }
                    }
                };
                if let Err(error) = self.bus.reply(&handoff, &reply).await {
                    tracing::warn!(%error, "handoff reply failed");
                }
            }
            HandoffMessage::Cancel { connection } => {
                if self.world.cancel(connection) {
                    tracing::info!(?connection, "late handoff undone");
                }
            }
        }
        self.count_players();
    }

    fn ghosts_in(&mut self, ghosts: Ghosts) {
        self.world
            .set_ghosts(&ghosts.from, ghosts.entities, self.now_ms());
    }

    /// A new cell table. A table without this sim makes its players leave without `Close`: the
    /// gateways resume them on other sims (M5 spec section 3.6).
    fn table(&mut self, table: &CellTable) {
        match self.world.set_table(table) {
            Ok(departures) => {
                if !departures.is_empty() {
                    tracing::warn!(
                        players = departures.len(),
                        "this sim is not in the cell table; its players leave"
                    );
                }
                for departure in departures {
                    self.save(&departure);
                }
                self.count_players();
            }
            Err(error) => tracing::warn!(%error, version = table.version, "cell table ignored"),
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
        self.count_players();
    }

    /// Runs the world once when ticks are due. The world changes only on admits, inputs, ghosts
    /// and departures, so a late call sends the current state once instead of every missed tick.
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
        let started = Instant::now();
        let output = match self.world.tick(now_ms, server_time()) {
            Ok(output) => output,
            Err(error) => {
                tracing::error!(%error, "tick failed");
                return;
            }
        };
        metrics::histogram!(METRIC_TICK_SECONDS).record(started.elapsed().as_secs_f64());
        for message in output.messages {
            self.send(message);
        }
        for start in output.handoffs {
            self.offer(start);
        }
        for (target, entities) in output.ghosts {
            let subject = subject::sim_ghosts(self.settings.world_id, &target);
            let ghosts = Ghosts {
                from: self.settings.sim_id.clone(),
                entities,
            };
            if !self.ghosts.send(subject, ghosts) {
                metrics::counter!(METRIC_DROPPED_OUTPUTS).increment(1);
            }
        }
    }

    /// Asks the target of a handoff in its own task.
    fn offer(&self, start: HandoffStart) {
        let bus = self.bus.clone();
        let results = self.handoff_results.clone();
        let subject = subject::sim_handoff(self.settings.world_id, &start.target.id);
        let timeout = self.settings.handoff_timeout;
        tokio::spawn(async move {
            let asked = Instant::now();
            let connection = start.offer.connection.clone();
            let sequence = start.offer.handoffs;
            let reply = bus
                .request::<HandoffMessage, HandoffReply>(
                    subject,
                    &HandoffMessage::Offer(start.offer),
                    timeout,
                )
                .await;
            let result = HandoffResult {
                connection,
                target: start.target,
                sequence,
                reply,
                elapsed: asked.elapsed(),
            };
            // The loop may have stopped; then nobody waits for the answer.
            let _ = results.send(result).await;
        });
    }

    /// Ends a handoff: the gateway gets the new route, or the player stays. A target that may
    /// hold the player although this sim keeps it, or although it left, gets a cancel.
    fn handoff_done(&mut self, result: HandoffResult) {
        metrics::histogram!(METRIC_HANDOFF_SECONDS).record(result.elapsed.as_secs_f64());
        let accepted = matches!(result.reply, Ok(HandoffReply::Accepted));
        let late = result.reply.is_err();
        let end = self
            .world
            .handoff_done(&result.connection, accepted, self.now_ms());
        let label = match end {
            HandoffEnd::Moved => {
                self.forwards.insert(
                    result.connection.clone(),
                    result.target.id.clone(),
                    self.now_ms(),
                );
                self.publish(
                    &result.connection.gateway,
                    GatewayOutput::Route {
                        connection: result.connection.connection,
                        sim: result.target.id.clone(),
                        generation: result.target.generation,
                        sequence: result.sequence,
                    },
                );
                HANDOFF_MOVED
            }
            HandoffEnd::Kept if late => HANDOFF_TIMEOUT,
            HandoffEnd::Kept => HANDOFF_REFUSED,
            HandoffEnd::Gone => HANDOFF_GONE,
        };
        let taken_elsewhere = matches!(end, HandoffEnd::Gone) && accepted;
        if late || taken_elsewhere {
            let subject = subject::sim_handoff(self.settings.world_id, &result.target.id);
            let cancel = HandoffMessage::Cancel {
                connection: result.connection,
            };
            if !self.cancels.send(subject, cancel) {
                metrics::counter!(METRIC_DROPPED_OUTPUTS).increment(1);
            }
        }
        metrics::counter!(METRIC_HANDOFFS, RESULT_LABEL => label).increment(1);
        self.count_players();
    }

    fn heartbeat(&mut self) {
        let beat = SimHeartbeat {
            id: self.settings.sim_id.clone(),
            started_at_ms: self.started_at_ms,
            players: u32::try_from(self.world.players()).unwrap_or(u32::MAX),
            max_players: self.settings.world.max_players,
        };
        let subject = subject::sim_heartbeat(self.settings.world_id, &self.settings.sim_id);
        if !self.beats.send(subject, beat) {
            tracing::warn!("heartbeat dropped; NATS is behind");
        }
        for gateway in self.gateways.silent(Instant::now()) {
            tracing::warn!(%gateway, "gateway silent; its players leave");
            self.drop_gateway(&gateway);
        }
        self.forwards.expire(self.now_ms());
        metrics::gauge!(METRIC_GHOSTS).set(self.world.ghosts() as f64);
    }

    fn count_players(&self) {
        metrics::gauge!(METRIC_PLAYERS).set(self.world.players() as f64);
    }

    fn send(&self, outgoing: Outgoing) {
        self.publish(
            &outgoing.connection.gateway,
            GatewayOutput::ServerMessages {
                sim: self.settings.sim_id.clone(),
                connection: outgoing.connection.connection,
                channel: outgoing.channel,
                reliable: outgoing.reliable,
                envelopes: outgoing.envelopes,
            },
        );
    }

    fn publish(&self, gateway: &BusId, message: GatewayOutput) {
        let subject = subject::gateway_output(self.settings.world_id, gateway);
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

/// Milliseconds since 2000-01-01. The unit of the retail `TimeSynchMsg` is Unknown.
fn server_time() -> u64 {
    unix_ms().saturating_sub(SERVER_EPOCH_UNIX_MS)
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use std::collections::BTreeMap;

    use nw_bus::test_support::test_bus;
    use nw_bus::{Point, Subscription};
    use nw_db::test_db::with_test_schema;
    use nw_db::{NewCharacter, WORLD_MIGRATOR, character_position, insert_character};
    use nw_messages::{
        BundlePayload, InputPosition, LOGIN_MESSAGE_TYPES, MemberState, ServerMessage,
    };
    use nw_reflect::TypeId;
    use nw_sim_core::test_support::NoGround;
    use nw_sim_core::{CellBounds, CellGrid, CellOwners};
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;

    use super::*;
    use crate::config::tests::committed;
    use crate::world::{MESSAGE_CHANNEL, REPLICATION_CHANNEL};

    const WAIT: Duration = Duration::from_secs(5);
    const LEVEL: &str = "newworld_vitaeeterna";
    const LEVEL_M: f32 = 14_336.0;
    const CELL_M: f32 = 2048.0;
    /// The first column of the second of two sims.
    const BORDER_X: f32 = 3.0 * CELL_M;
    const Y: f32 = 7000.0;
    const Z: f32 = 100.0;
    const CONNECTION: u64 = 9;
    const SAVE_INTERVAL: Duration = Duration::from_secs(60);
    /// Every test process beats fast, so silence shows in 0.3 s.
    const HEARTBEAT: Duration = Duration::from_millis(100);
    const MISSES: u32 = 3;
    /// The sim subscribes at start; an admit may come before that.
    const TRIES: usize = 50;
    const PAUSE: Duration = Duration::from_millis(100);
    const SAVE_TOLERANCE_M: f64 = 0.01;
    const CHARACTER_NAME: &str = "Ada";
    /// A step the limits allow at once.
    const STEP_M: f32 = 2.0;
    const MOVE_TOLERANCE_M: f32 = 0.01;
    /// A crossing the full budget allows (21.45 m): from 10 m west to 10 m east of the border.
    const CROSSING_M: f32 = 10.0;
    /// Time to watch for a message that must not come.
    const QUIET: Duration = Duration::from_millis(500);
    const FIRST_HANDOFF: u64 = 1;
    const FAKE_GENERATION: u64 = 1;

    fn bounds() -> CellBounds {
        CellBounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: LEVEL_M,
            max_y: LEVEL_M,
        }
    }

    fn id(prefix: &str) -> BusId {
        BusId::try_from(format!("{prefix}-{}", Uuid::new_v4().simple())).expect("id")
    }

    fn settings(world_id: Uuid, name: &str) -> LoopSettings {
        let config = committed();
        LoopSettings {
            sim_id: id(name),
            world_id,
            level: LEVEL.to_owned(),
            world: config.world_settings(bounds()).expect("world settings"),
            handoff_timeout: Duration::from_millis(config.handoff_timeout_ms),
            forward_ttl_ms: config.forward_ttl_ms,
            save_interval: SAVE_INTERVAL,
            heartbeat_interval: HEARTBEAT,
            heartbeat_misses: MISSES,
        }
    }

    fn at(x: f32) -> Point {
        Point { x, y: Y, z: Z }
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
        settings: LoopSettings,
        stop: oneshot::Sender<()>,
        server: JoinHandle<Result<(), ServeError>>,
    }

    impl Running {
        async fn start(bus: &Bus, settings: LoopSettings, db: &PgPool) -> Self {
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
            Self {
                settings,
                stop,
                server,
            }
        }

        fn id(&self) -> &BusId {
            &self.settings.sim_id
        }

        async fn stop(self) {
            let _ = self.stop.send(());
            self.server.await.expect("join").expect("serve");
        }
    }

    /// The test's gateway: its outputs, and a task that beats for it.
    struct TestGateway {
        id: BusId,
        outputs: Subscription<GatewayOutput>,
        beats: JoinHandle<()>,
    }

    impl TestGateway {
        async fn start(bus: &Bus, world_id: Uuid, beating: bool) -> Self {
            let id = id("gw");
            let outputs = bus
                .subscribe::<GatewayOutput>(subject::gateway_output(world_id, &id))
                .await
                .expect("outputs");
            bus.flush().await.expect("flush");
            let beater = bus.clone();
            let beat = Heartbeat {
                id: id.clone(),
                started_at_ms: unix_ms(),
            };
            let beats = tokio::spawn(async move {
                if !beating {
                    return;
                }
                loop {
                    let subject = subject::gateway_heartbeat(world_id, &beat.id);
                    beater.publish(subject, &beat).await.expect("beat");
                    tokio::time::sleep(HEARTBEAT).await;
                }
            });
            Self { id, outputs, beats }
        }

        fn connection(&self, number: u64) -> ConnectionRef {
            ConnectionRef {
                gateway: self.id.clone(),
                connection: number,
            }
        }

        async fn next(&mut self) -> GatewayOutput {
            tokio::time::timeout(WAIT, self.outputs.next())
                .await
                .expect("output in time")
                .expect("open")
                .expect("decode")
                .message
        }

        /// The next output that `wanted` picks.
        async fn until<T>(&mut self, mut wanted: impl FnMut(GatewayOutput) -> Option<T>) -> T {
            loop {
                if let Some(found) = wanted(self.next().await) {
                    return found;
                }
            }
        }

        /// The entity numbers of the next bundle that `sim` sends to `connection`.
        async fn bundle_from(&mut self, sim: &BusId, connection: u64) -> Vec<u64> {
            self.until(|output| match output {
                GatewayOutput::ServerMessages {
                    sim: from,
                    connection: to,
                    channel: REPLICATION_CHANNEL,
                    envelopes,
                    ..
                } if from == *sim && to == connection => Some(members(&envelopes[0])),
                _ => None,
            })
            .await
        }
    }

    impl Drop for TestGateway {
        fn drop(&mut self) {
            self.beats.abort();
        }
    }

    /// The entity numbers and x positions of a bundle envelope.
    fn member_positions(envelope: &[u8]) -> Vec<(u64, f32)> {
        let envelope = Envelope::decode(envelope).expect("envelope");
        let Some(Ok(ServerMessage::Bundle(bundle))) = ServerMessage::from_envelope(&envelope)
        else {
            panic!("expected a bundle");
        };
        BundlePayload::decode(&bundle.payload)
            .expect("payload")
            .groups
            .iter()
            .flat_map(|group| {
                group.members.iter().map(|member| {
                    let MemberState::Position(state) = &member.state;
                    (member.u64_at_0, state.position.expect("position").x)
                })
            })
            .collect()
    }

    fn members(envelope: &[u8]) -> Vec<u64> {
        member_positions(envelope)
            .into_iter()
            .map(|(number, _)| number)
            .collect()
    }

    /// The slots of `sims` from their heartbeats.
    async fn slots(bus: &Bus, world_id: Uuid, sims: &[&BusId]) -> Vec<SimSlot> {
        let mut beats = bus
            .subscribe::<SimHeartbeat>(subject::all_sim_heartbeats(world_id))
            .await
            .expect("beats");
        let mut found: BTreeMap<BusId, u64> = BTreeMap::new();
        let heard = async {
            while found.len() < sims.len() {
                let beat = beats.next().await.expect("open").expect("decode").message;
                if sims.contains(&&beat.id) {
                    found.insert(beat.id, beat.started_at_ms);
                }
            }
        };
        tokio::time::timeout(WAIT, heard)
            .await
            .expect("beats in time");
        sims.iter()
            .map(|sim| SimSlot {
                id: (*sim).clone(),
                generation: found[*sim],
            })
            .collect()
    }

    /// The column stripes of `slots`, published every heartbeat like the director does.
    fn publish_tables(bus: &Bus, world_id: Uuid, slots: Vec<SimSlot>) -> JoinHandle<()> {
        let grid = CellGrid::new(LEVEL_M, LEVEL_M, CELL_M).expect("grid");
        let indexes: Vec<u16> = (0..).take(slots.len()).collect();
        let table = CellTable {
            version: 1,
            cell_size_m: CELL_M,
            columns: grid.columns(),
            rows: grid.rows(),
            owners: CellOwners::columns(grid, &indexes).cells().to_vec(),
            sims: slots,
        };
        let bus = bus.clone();
        tokio::spawn(async move {
            loop {
                let subject = subject::director_table(world_id);
                bus.publish(subject, &table).await.expect("table");
                tokio::time::sleep(HEARTBEAT).await;
            }
        })
    }

    /// Sends `admit` to `sim` until it answers.
    async fn admit(bus: &Bus, world_id: Uuid, sim: &BusId, admit: &Admit) -> AdmitReply {
        let subject = subject::sim_admit(world_id, sim);
        let mut reply: Result<AdmitReply, BusError> = Err(BusError::NoReplySubject);
        for _ in 0..TRIES {
            reply = bus.request(subject.clone(), admit, WAIT).await;
            if reply.is_ok() {
                break;
            }
            tokio::time::sleep(PAUSE).await;
        }
        reply.expect("admit reply")
    }

    fn admit_at(connection: ConnectionRef, character: (Uuid, Uuid), x: f32) -> Admit {
        Admit {
            connection,
            account: character.0,
            character: character.1,
            entity: CONNECTION,
            position: at(x),
            login: true,
        }
    }

    /// A position-only input to `x`.
    fn move_to(connection: ConnectionRef, x: f32) -> SimInput {
        let target = InputPosition::from_metres(x, Y, Z).expect("finite");
        let body = ClientInputMsg::position(target).encode().expect("input");
        let envelope =
            Envelope::message(TypeId::FileSlot(RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX), body)
                .encode()
                .expect("envelope");
        SimInput::ClientMessage {
            connection,
            envelope,
        }
    }

    async fn input(bus: &Bus, world_id: Uuid, sim: &BusId, input: &SimInput) {
        bus.publish(subject::sim_input(world_id, sim), input)
            .await
            .expect("input");
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
    async fn an_admit_gets_login_messages_bundles_and_a_saved_position() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let ada = character(db).await;
            let bus = test_bus().await;
            let world_id = Uuid::new_v4();
            let mut gateway = TestGateway::start(&bus, world_id, true).await;
            let sim = Running::start(&bus, settings(world_id, "sim"), db).await;
            let first = admit_at(gateway.connection(CONNECTION), ada, BORDER_X);
            assert_eq!(
                admit(&bus, world_id, sim.id(), &first).await,
                AdmitReply::Admitted
            );
            let login = gateway
                .until(|output| match output {
                    GatewayOutput::ServerMessages {
                        channel: MESSAGE_CHANNEL,
                        reliable: true,
                        envelopes,
                        ..
                    } => Some(envelopes.len()),
                    _ => None,
                })
                .await;
            assert_eq!(login, LOGIN_MESSAGE_TYPES.len());
            assert_eq!(
                gateway.bundle_from(sim.id(), CONNECTION).await,
                vec![CONNECTION]
            );
            let again = Admit {
                connection: gateway.connection(CONNECTION + 1),
                ..first
            };
            assert!(matches!(
                admit(&bus, world_id, sim.id(), &again).await,
                AdmitReply::Refused { .. }
            ));
            let left = SimInput::Left {
                connection: gateway.connection(CONNECTION),
            };
            input(&bus, world_id, sim.id(), &left).await;
            let saved = saved_position(db, ada.1).await;
            assert_eq!(saved.level, LEVEL);
            assert!((saved.x - f64::from(BORDER_X)).abs() < SAVE_TOLERANCE_M);
            sim.stop().await;
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_client_input_moves_the_player() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let ada = character(db).await;
            let bus = test_bus().await;
            let world_id = Uuid::new_v4();
            let mut gateway = TestGateway::start(&bus, world_id, true).await;
            let sim = Running::start(&bus, settings(world_id, "sim"), db).await;
            let connection = gateway.connection(CONNECTION);
            let start = admit_at(connection.clone(), ada, BORDER_X);
            admit(&bus, world_id, sim.id(), &start).await;
            let target = BORDER_X + STEP_M;
            input(&bus, world_id, sim.id(), &move_to(connection, target)).await;
            gateway
                .until(|output| match output {
                    GatewayOutput::ServerMessages {
                        channel: REPLICATION_CHANNEL,
                        envelopes,
                        ..
                    } => {
                        let own_x = member_positions(&envelopes[0])[0].1;
                        ((own_x - target).abs() < MOVE_TOLERANCE_M).then_some(())
                    }
                    _ => None,
                })
                .await;
            sim.stop().await;
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_last_saves_finish_before_serve_returns() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let ada = character(db).await;
            let bus = test_bus().await;
            let world_id = Uuid::new_v4();
            let gateway = TestGateway::start(&bus, world_id, true).await;
            let sim = Running::start(&bus, settings(world_id, "sim"), db).await;
            let first = admit_at(gateway.connection(CONNECTION), ada, BORDER_X);
            admit(&bus, world_id, sim.id(), &first).await;
            sim.stop().await;
            let saved = character_position(db, ada.1).await.expect("load");
            assert!(saved.is_some(), "the shutdown save is missing");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_silent_gateway_loses_its_players_and_hears_close() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let ada = character(db).await;
            let bus = test_bus().await;
            let world_id = Uuid::new_v4();
            let mut gateway = TestGateway::start(&bus, world_id, false).await;
            let sim = Running::start(&bus, settings(world_id, "sim"), db).await;
            let first = admit_at(gateway.connection(CONNECTION), ada, BORDER_X);
            admit(&bus, world_id, sim.id(), &first).await;
            let closed = gateway
                .until(|output| match output {
                    GatewayOutput::Close { connection } => Some(connection),
                    _ => None,
                })
                .await;
            assert_eq!(closed, CONNECTION);
            saved_position(db, ada.1).await;
            sim.stop().await;
        })
        .await;
    }

    /// Two sims sharing the level in two stripes, and the test's gateway.
    struct Pair {
        bus: Bus,
        world_id: Uuid,
        gateway: TestGateway,
        west: Running,
        east: Running,
        tables: JoinHandle<()>,
    }

    impl Pair {
        async fn start(db: &PgPool) -> Self {
            let bus = test_bus().await;
            let world_id = Uuid::new_v4();
            let gateway = TestGateway::start(&bus, world_id, true).await;
            // Sorted ids give the stripes: "sim-a" is west, "sim-b" east.
            let west = Running::start(&bus, settings(world_id, "sim-a"), db).await;
            let east = Running::start(&bus, settings(world_id, "sim-b"), db).await;
            let slots = slots(&bus, world_id, &[west.id(), east.id()]).await;
            let tables = publish_tables(&bus, world_id, slots);
            Self {
                bus,
                world_id,
                gateway,
                west,
                east,
                tables,
            }
        }

        async fn stop(self) {
            self.tables.abort();
            self.west.stop().await;
            self.east.stop().await;
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_player_crosses_to_the_other_sim_and_its_inputs_follow() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let ada = character(db).await;
            let mut pair = Pair::start(db).await;
            let (bus, world_id) = (pair.bus.clone(), pair.world_id);
            let connection = pair.gateway.connection(CONNECTION);
            let first = admit_at(connection.clone(), ada, BORDER_X - CROSSING_M);
            admit(&bus, world_id, pair.west.id(), &first).await;
            pair.gateway.bundle_from(pair.west.id(), CONNECTION).await;
            let east = pair.east.id().clone();
            let crossing = move_to(connection.clone(), BORDER_X + CROSSING_M);
            input(&bus, world_id, pair.west.id(), &crossing).await;
            let route = pair
                .gateway
                .until(|output| match output {
                    GatewayOutput::Route { sim, sequence, .. } => Some((sim, sequence)),
                    _ => None,
                })
                .await;
            assert_eq!(route, (east.clone(), FIRST_HANDOFF));
            assert_eq!(
                pair.gateway.bundle_from(&east, CONNECTION).await,
                vec![CONNECTION]
            );
            assert!(character_position(db, ada.1).await.expect("load").is_none());
            let left = SimInput::Left { connection };
            input(&bus, world_id, pair.west.id(), &left).await;
            let saved = saved_position(db, ada.1).await;
            assert!((saved.x - f64::from(BORDER_X + CROSSING_M)).abs() < SAVE_TOLERANCE_M);
            pair.stop().await;
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ghosts_cross_the_border() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let mut pair = Pair::start(db).await;
            let (bus, world_id) = (pair.bus.clone(), pair.world_id);
            let west_player = admit_at(
                pair.gateway.connection(CONNECTION),
                character(db).await,
                BORDER_X - CROSSING_M,
            );
            let east_player = Admit {
                connection: pair.gateway.connection(CONNECTION + 1),
                entity: CONNECTION + 1,
                ..admit_at(
                    pair.gateway.connection(CONNECTION + 1),
                    character(db).await,
                    BORDER_X + CROSSING_M,
                )
            };
            admit(&bus, world_id, pair.west.id(), &west_player).await;
            admit(&bus, world_id, pair.east.id(), &east_player).await;
            let east = pair.east.id().clone();
            let seen = async {
                loop {
                    let numbers = pair.gateway.bundle_from(&east, CONNECTION + 1).await;
                    if numbers == vec![CONNECTION + 1, CONNECTION] {
                        return;
                    }
                }
            };
            tokio::time::timeout(WAIT, seen)
                .await
                .expect("ghost in time");
            pair.stop().await;
        })
        .await;
    }

    /// A sim that takes the place of the east sim in the table: it records every handoff
    /// message and answers offers with `answer`, or not at all.
    async fn fake_east(
        bus: &Bus,
        world_id: Uuid,
        answer: Option<HandoffReply>,
    ) -> (SimSlot, Subscription<HandoffMessage>) {
        let fake = SimSlot {
            id: id("sim-b"),
            generation: FAKE_GENERATION,
        };
        let messages = bus
            .subscribe::<HandoffMessage>(subject::sim_handoff(world_id, &fake.id))
            .await
            .expect("handoffs");
        let mut offers = bus
            .subscribe::<HandoffMessage>(subject::sim_handoff(world_id, &fake.id))
            .await
            .expect("handoffs");
        bus.flush().await.expect("flush");
        if let Some(answer) = answer {
            let answerer = bus.clone();
            tokio::spawn(async move {
                while let Some(Ok(message)) = offers.next().await {
                    if matches!(message.message, HandoffMessage::Offer(_)) {
                        answerer.reply(&message, &answer).await.expect("reply");
                    }
                }
            });
        }
        (fake, messages)
    }

    async fn next_handoff(messages: &mut Subscription<HandoffMessage>) -> HandoffMessage {
        tokio::time::timeout(WAIT, messages.next())
            .await
            .expect("handoff message in time")
            .expect("open")
            .expect("decode")
            .message
    }

    /// One real west sim, a fake east sim in the table, and a player 10 m inside the east.
    async fn west_with_fake_east(
        db: &PgPool,
        answer: Option<HandoffReply>,
    ) -> (
        Running,
        Subscription<HandoffMessage>,
        TestGateway,
        JoinHandle<()>,
    ) {
        let bus = test_bus().await;
        let world_id = Uuid::new_v4();
        let gateway = TestGateway::start(&bus, world_id, true).await;
        let west = Running::start(&bus, settings(world_id, "sim-a"), db).await;
        let (fake, messages) = fake_east(&bus, world_id, answer).await;
        let west_slot = slots(&bus, world_id, &[west.id()]).await.remove(0);
        let tables = publish_tables(&bus, world_id, vec![west_slot, fake]);
        let player = admit_at(
            gateway.connection(CONNECTION),
            character(db).await,
            BORDER_X + CROSSING_M,
        );
        admit(&bus, world_id, west.id(), &player).await;
        (west, messages, gateway, tables)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_refused_handoff_keeps_the_player() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let refusal = HandoffReply::Refused {
                reason: "full".to_owned(),
            };
            let (west, mut messages, mut gateway, tables) =
                west_with_fake_east(db, Some(refusal)).await;
            assert!(matches!(
                next_handoff(&mut messages).await,
                HandoffMessage::Offer(_)
            ));
            assert_eq!(
                gateway.bundle_from(west.id(), CONNECTION).await,
                vec![CONNECTION]
            );
            assert!(matches!(
                next_handoff(&mut messages).await,
                HandoffMessage::Offer(_)
            ));
            let route = tokio::time::timeout(
                QUIET,
                gateway.until(|output| matches!(output, GatewayOutput::Route { .. }).then_some(())),
            )
            .await;
            assert!(route.is_err(), "a refused handoff sends no route");
            tables.abort();
            west.stop().await;
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_late_handoff_is_cancelled() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let (west, mut messages, mut gateway, tables) = west_with_fake_east(db, None).await;
            let HandoffMessage::Offer(offer) = next_handoff(&mut messages).await else {
                panic!("expected an offer first");
            };
            assert_eq!(
                next_handoff(&mut messages).await,
                HandoffMessage::Cancel {
                    connection: offer.connection
                }
            );
            assert_eq!(
                gateway.bundle_from(west.id(), CONNECTION).await,
                vec![CONNECTION]
            );
            tables.abort();
            west.stop().await;
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_sim_dropped_from_the_table_saves_and_removes_its_players() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let ada = character(db).await;
            let bus = test_bus().await;
            let world_id = Uuid::new_v4();
            let mut gateway = TestGateway::start(&bus, world_id, true).await;
            let sim = Running::start(&bus, settings(world_id, "sim"), db).await;
            let first = admit_at(gateway.connection(CONNECTION), ada, BORDER_X);
            admit(&bus, world_id, sim.id(), &first).await;
            gateway.bundle_from(sim.id(), CONNECTION).await;
            let other = SimSlot {
                id: id("sim-other"),
                generation: FAKE_GENERATION,
            };
            let tables = publish_tables(&bus, world_id, vec![other]);
            saved_position(db, ada.1).await;
            let close = tokio::time::timeout(
                QUIET,
                gateway.until(|output| matches!(output, GatewayOutput::Close { .. }).then_some(())),
            )
            .await;
            assert!(close.is_err(), "the gateways resume the players; no close");
            tables.abort();
            sim.stop().await;
        })
        .await;
    }
}
