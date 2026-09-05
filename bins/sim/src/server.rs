// SPDX-License-Identifier: AGPL-3.0-or-later
//! The loop of `sim` (M4 spec sections 3.1, 6, 7.1, 7.5, 11): bus joins and inputs, the 30 Hz
//! tick, position saves, and heartbeats. One task owns the world; database work runs in its own
//! tasks and sends the result back, so a slow database never stops the tick.

use std::collections::HashMap;
use std::future::Future;
use std::time::{Duration, Instant};

use nw_bus::{Bus, BusError, BusId, GatewayOutput, Heartbeat, Incoming, Join, JoinReply, SimInput};
use nw_common::time::unix_ms;
use nw_db::{PgPool, SavedPosition, character_by_id, character_position, save_character_position};
use nw_sim_core::{Position, TickClock};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::login::WorldIds;
use crate::world::{Departure, Outgoing, SimWorld, WorldSettings};

/// Time between checks of the tick clock.
pub const POLL: Duration = Duration::from_millis(5);
/// Unix milliseconds of 2000-01-01, the epoch of the server clock in time messages.
pub const SERVER_EPOCH_UNIX_MS: u64 = 946_684_800_000;
/// Results of database work that may wait for the loop.
const RESULT_QUEUE: usize = 1024;
/// Metric: players in the world.
pub const METRIC_PLAYERS: &str = "sim_players";
/// Metric: ticks that started late.
pub const METRIC_LATE_TICKS: &str = "sim_late_ticks_total";
/// Metric: joins by `result`.
pub const METRIC_JOINS: &str = "sim_joins_total";
/// Metric: failed saves.
pub const METRIC_FAILED_SAVES: &str = "sim_failed_saves_total";
/// Metric: client messages that `sim` does not read yet.
pub const METRIC_IGNORED_MESSAGES: &str = "sim_ignored_client_messages_total";
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

struct GatewayState {
    started_at_ms: u64,
    last_seen: Instant,
}

type JoinResult = (Incoming<Join>, Result<Position, String>);
type SaveResult = (Uuid, SavedPosition, bool);

struct Loop {
    settings: LoopSettings,
    bus: Bus,
    db: PgPool,
    world: SimWorld,
    gateways: HashMap<BusId, GatewayState>,
    unsaved: HashMap<Uuid, SavedPosition>,
    joins: mpsc::Sender<JoinResult>,
    saves: mpsc::Sender<SaveResult>,
    started: Instant,
    started_at_ms: u64,
}

/// Serves joins, inputs, ticks, saves and heartbeats until `shutdown` finishes; then saves every
/// player once more.
///
/// # Errors
///
/// Returns [`ServeError`] when a subscription cannot start.
pub async fn serve(
    settings: LoopSettings,
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
    let (save_results, mut saved) = mpsc::channel(RESULT_QUEUE);
    let mut state = Loop {
        world: SimWorld::new(settings.world, WorldIds::random(started_at_ms)?),
        settings,
        bus,
        db,
        gateways: HashMap::new(),
        unsaved: HashMap::new(),
        joins: join_results,
        saves: save_results,
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
                Ok(beat) => state.gateway_beat(beat.message),
                Err(error) => tracing::warn!(%error, "bad heartbeat"),
            },
            Some((character, position, ok)) = saved.recv() => state.saved(character, position, ok),
            _ = poll.tick() => state.tick(&mut clock).await,
            _ = save_timer.tick() => state.save_all(),
            _ = beat_timer.tick() => state.heartbeat().await,
        }
    }
    state.save_all();
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

    async fn complete_join(&mut self, join: Incoming<Join>, result: Result<Position, String>) {
        let message = &join.message;
        let joined = result.and_then(|position| {
            self.world
                .join(
                    message.connection.clone(),
                    message.character,
                    position,
                    self.now_ms(),
                    server_time(),
                )
                .map_err(|error| error.to_string())
        });
        let reply = match joined {
            Ok(login) => {
                metrics::counter!(METRIC_JOINS, RESULT_LABEL => JOINED).increment(1);
                tracing::info!(character = %message.character, "joined");
                self.send(login).await;
                JoinReply::Joined
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
            SimInput::ClientMessage { .. } => {
                metrics::counter!(METRIC_IGNORED_MESSAGES).increment(1);
            }
            SimInput::Left { connection } => {
                if let Some(departure) = self.world.leave(&connection) {
                    self.save(departure);
                }
                metrics::gauge!(METRIC_PLAYERS).set(self.world.players() as f64);
            }
        }
    }

    fn gateway_beat(&mut self, beat: Heartbeat) {
        let restarted = self
            .gateways
            .get(&beat.id)
            .is_some_and(|known| known.started_at_ms != beat.started_at_ms);
        if restarted {
            self.drop_gateway(&beat.id);
        }
        self.gateways.insert(
            beat.id,
            GatewayState {
                started_at_ms: beat.started_at_ms,
                last_seen: Instant::now(),
            },
        );
    }

    fn drop_gateway(&mut self, gateway: &BusId) {
        for departure in self.world.leave_gateway(gateway) {
            self.save(departure);
        }
        metrics::gauge!(METRIC_PLAYERS).set(self.world.players() as f64);
    }

    async fn tick(&mut self, clock: &mut TickClock) {
        let now_ms = self.now_ms();
        let now_us = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        let due = clock.due(now_us);
        if due.count > 1 {
            metrics::counter!(METRIC_LATE_TICKS).increment(due.count - 1);
        }
        for _ in 0..due.count {
            match self.world.tick(now_ms, server_time()) {
                Ok(outgoing) => {
                    for message in outgoing {
                        self.send(message).await;
                    }
                }
                Err(error) => tracing::error!(%error, "tick failed"),
            }
        }
    }

    async fn heartbeat(&mut self) {
        let beat = Heartbeat {
            id: self.settings.sim_id.clone(),
            started_at_ms: self.started_at_ms,
        };
        let subject = nw_bus::subject::sim_heartbeat(self.settings.world_id, &self.settings.sim_id);
        if let Err(error) = self.bus.publish(subject, &beat).await {
            tracing::warn!(%error, "heartbeat failed");
        }
        let limit = self.settings.heartbeat_interval * self.settings.heartbeat_misses;
        let silent: Vec<BusId> = self
            .gateways
            .iter()
            .filter(|(_, state)| state.last_seen.elapsed() > limit)
            .map(|(id, _)| id.clone())
            .collect();
        for gateway in silent {
            tracing::warn!(%gateway, "gateway silent; its players leave");
            self.gateways.remove(&gateway);
            self.drop_gateway(&gateway);
        }
    }

    async fn send(&self, outgoing: Outgoing) {
        let subject =
            nw_bus::subject::gateway_output(self.settings.world_id, &outgoing.connection.gateway);
        let message = GatewayOutput::ServerMessages {
            connection: outgoing.connection.connection,
            channel: outgoing.channel,
            reliable: outgoing.reliable,
            envelopes: outgoing.envelopes,
        };
        if let Err(error) = self.bus.publish(subject, &message).await {
            tracing::warn!(%error, "output publish failed");
        }
    }

    /// Saves every player and every save that failed before.
    fn save_all(&mut self) {
        let players: Vec<Departure> = self.world.positions();
        for departure in players {
            self.save(departure);
        }
        let retries: Vec<(Uuid, SavedPosition)> = self.unsaved.drain().collect();
        for (character, position) in retries {
            self.spawn_save(character, position);
        }
    }

    fn save(&self, departure: Departure) {
        let position = SavedPosition {
            level: self.settings.level.clone(),
            x: f64::from(departure.position.x),
            y: f64::from(departure.position.y),
            z: f64::from(departure.position.z),
            yaw: NO_YAW,
        };
        self.spawn_save(departure.character, position);
    }

    fn spawn_save(&self, character: Uuid, position: SavedPosition) {
        let db = self.db.clone();
        let results = self.saves.clone();
        tokio::spawn(async move {
            let ok = match save_character_position(&db, character, &position).await {
                Ok(()) => true,
                Err(error) => {
                    tracing::error!(%error, %character, "position save failed");
                    false
                }
            };
            // The loop may have stopped; then nobody retries.
            let _ = results.send((character, position, ok)).await;
        });
    }

    /// Keeps a failed save for the next save round.
    fn saved(&mut self, character: Uuid, position: SavedPosition, ok: bool) {
        if !ok {
            metrics::counter!(METRIC_FAILED_SAVES).increment(1);
            self.unsaved.insert(character, position);
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
    use nw_bus::{ConnectionRef, GatewayOutput};
    use nw_db::test_db::with_test_schema;
    use nw_db::{NewCharacter, WORLD_MIGRATOR, insert_character};
    use nw_messages::{BundlePayload, ServerMessage};
    use nw_reflect::Envelope;

    use super::*;
    use crate::world::{MESSAGE_CHANNEL, REPLICATION_CHANNEL};

    const WAIT: Duration = Duration::from_secs(5);
    const LEVEL: &str = "newworld_vitaeeterna";
    const LOGIN_MESSAGES: usize = 7;
    const CONNECTION: u64 = 9;
    const START: Position = Position {
        x: 11132.5,
        y: 7305.9,
        z: 170.0,
    };
    const RADIUS: f32 = 100.0;
    const SAVE_INTERVAL: Duration = Duration::from_secs(10);
    const HEARTBEAT: Duration = Duration::from_secs(1);
    const MISSES: u32 = 3;
    /// The sim subscribes at start; a join may come before that.
    const TRIES: usize = 50;
    const PAUSE: Duration = Duration::from_millis(100);
    const SAVE_TOLERANCE_M: f64 = 0.01;

    fn settings(world_id: Uuid) -> LoopSettings {
        LoopSettings {
            sim_id: BusId::try_from(format!("sim-{}", Uuid::new_v4().simple())).expect("id"),
            world_id,
            level: LEVEL.to_owned(),
            start: START,
            world: WorldSettings {
                interest_radius_m: RADIUS,
            },
            save_interval: SAVE_INTERVAL,
            heartbeat_interval: HEARTBEAT,
            heartbeat_misses: MISSES,
        }
    }

    async fn next_output(
        outputs: &mut nw_bus::Subscription<GatewayOutput>,
    ) -> (u8, bool, Vec<Vec<u8>>) {
        let incoming = tokio::time::timeout(WAIT, outputs.next())
            .await
            .expect("output in time")
            .expect("open")
            .expect("decode");
        match incoming.message {
            GatewayOutput::ServerMessages {
                channel,
                reliable,
                envelopes,
                ..
            } => (channel, reliable, envelopes),
            GatewayOutput::Close { .. } => panic!("unexpected close"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_gets_login_messages_bundles_and_a_saved_position() {
        with_test_schema(async |db| {
            WORLD_MIGRATOR.run(db).await.expect("migrate");
            let account = Uuid::new_v4();
            let character = insert_character(
                db,
                &NewCharacter {
                    id: Uuid::new_v4(),
                    account_id: account,
                    name: "Ada".to_owned(),
                    appearance: serde_json::json!({}),
                },
            )
            .await
            .expect("character");
            let bus = test_bus().await;
            let world_id = Uuid::new_v4();
            let settings = settings(world_id);
            let sim_id = settings.sim_id.clone();
            let gateway = BusId::try_from("gw-test".to_owned()).expect("id");
            let mut outputs = bus
                .subscribe::<GatewayOutput>(nw_bus::subject::gateway_output(world_id, &gateway))
                .await
                .expect("outputs");
            bus.flush().await.expect("flush");
            let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
            let server = tokio::spawn(serve(settings, bus.clone(), db.clone(), async {
                let _ = stopped.await;
            }));
            let connection = ConnectionRef {
                gateway,
                connection: CONNECTION,
            };
            let join = |account| Join {
                connection: connection.clone(),
                account,
                character: character.id,
            };
            let subject = nw_bus::subject::sim_join(world_id, &sim_id);
            let mut reply: Result<JoinReply, BusError> = Err(BusError::NoReplySubject);
            for _ in 0..TRIES {
                reply = bus.request(subject.clone(), &join(account), WAIT).await;
                if reply.is_ok() {
                    break;
                }
                tokio::time::sleep(PAUSE).await;
            }
            assert_eq!(reply.expect("joined"), JoinReply::Joined);
            let (channel, reliable, login) = next_output(&mut outputs).await;
            assert_eq!(
                (channel, reliable, login.len()),
                (MESSAGE_CHANNEL, true, LOGIN_MESSAGES)
            );
            let (channel, reliable, bundle) = next_output(&mut outputs).await;
            assert_eq!((channel, reliable), (REPLICATION_CHANNEL, false));
            let envelope = Envelope::decode(&bundle[0]).expect("envelope");
            let Some(Ok(ServerMessage::Bundle(bundle))) = ServerMessage::from_envelope(&envelope)
            else {
                panic!("expected a bundle");
            };
            let payload = BundlePayload::decode(&bundle.payload).expect("payload");
            assert_eq!(payload.groups[0].members.len(), 1);
            let other_account: JoinReply = bus
                .request(
                    subject.clone(),
                    &Join {
                        connection: ConnectionRef {
                            gateway: connection.gateway.clone(),
                            connection: CONNECTION + 1,
                        },
                        ..join(Uuid::new_v4())
                    },
                    WAIT,
                )
                .await
                .expect("reply");
            assert!(matches!(other_account, JoinReply::Refused { .. }));
            bus.publish(
                nw_bus::subject::sim_input(world_id, &sim_id),
                &SimInput::Left {
                    connection: connection.clone(),
                },
            )
            .await
            .expect("left");
            let mut saved = None;
            for _ in 0..TRIES {
                saved = character_position(db, character.id).await.expect("load");
                if saved.is_some() {
                    break;
                }
                tokio::time::sleep(PAUSE).await;
            }
            let saved = saved.expect("saved position");
            assert_eq!(saved.level, LEVEL);
            assert!((saved.x - f64::from(START.x)).abs() < SAVE_TOLERANCE_M);
            let _ = stop.send(());
            server.await.expect("join").expect("serve");
        })
        .await;
    }
}
