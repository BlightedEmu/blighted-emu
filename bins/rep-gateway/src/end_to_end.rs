// SPDX-License-Identifier: AGPL-3.0-or-later
//! The exit tests without the game (`infra` feature), on the real PostgreSQL, Valkey and NATS:
//!
//! - M3 (M3 spec section 12): the test client logs in over HTTP through the `edge-http` routes,
//!   waits in the queue, and registers with the gateway over UDP. The same token is refused the
//!   second time.
//! - M4 (M4 spec section 1.2, criteria 3.1, 3.2 and 3.5): `sim` serves the world over NATS. Two
//!   clients are in the world together and see each other; a client that logs in again appears at
//!   its saved position.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nw_bus::test_support::test_bus;
use nw_bus::{BusId, Heartbeat, subject};
use nw_db::test_db::with_test_schema;
use nw_db::{GLOBAL_MIGRATOR, PgPool, SavedPosition, WORLD_MIGRATOR, save_character_position};
use nw_login::config::load_edge_config;
use nw_login::install_crypto_provider;
use nw_login::test_support::{edge_app, test_valkey_url};
use nw_login::valkey::Valkey;
use nw_messages::WorldPosition;
use nw_reflect::{REGISTRATION_STATUS_AUTHORIZED, REGISTRATION_STATUS_REFUSED};
use nw_sim_core::Position;
use nw_sim_core::test_support::NoGround;
use nw_testclient::pair::{PairSettings, distance, run_pair, session};
use nw_testclient::udp::run_rep;
use nw_testclient::walk::{WalkSettings, login_token, walk};
use sim::config::{SimConfig, load_sim_config};
use sim::server::{LoopSettings, serve as serve_sim};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::server::tests::{HOLD, LIMITS, VERSION, WAIT, accepting, client, spawn_gateway};
use crate::world::{BusWorld, WorldLinkSettings};

const EDGE_CONFIG: &str = "../../config/edge.toml";
const LOOPBACK: &str = "127.0.0.1:0";
const PASSWORD: &str = "password-ok";
const CHARACTER_NAME: &str = "Gateway";
const FIRST_CHARACTER_NAME: &str = "Pairone";
const SECOND_CHARACTER_NAME: &str = "Pairtwo";
/// Each run makes new accounts: a prefix and 12 random hex digits.
const ACCOUNT_PREFIX: &str = "e2e";
const ACCOUNT_RANDOM_DIGITS: usize = 12;
const LEVEL: &str = "newworld_vitaeeterna";
const SIM_CONFIG: &str = "../../config/sim.toml";
/// A saved position away from the start, inside the interest radius.
const SAVED_OFFSET_M: f32 = 25.0;
const SAVE_INTERVAL: Duration = Duration::from_secs(10);
const HEARTBEAT: Duration = Duration::from_millis(200);
const HEARTBEAT_MISSES: u32 = 3;
const JOIN_TIMEOUT: Duration = Duration::from_secs(2);
/// Time in the world per session: many bundles at 30 Hz.
const PAIR_HOLD: Duration = Duration::from_secs(1);
/// M4 spec section 1.2 criterion 3.5.
const TOLERANCE_M: f32 = 0.1;
const NO_YAW: f32 = 0.0;

/// `edge-http` on a loopback port, for one world and one gateway.
struct Edge {
    http: reqwest::Client,
    login_url: String,
    game_url: String,
}

impl Edge {
    async fn start(
        global: &PgPool,
        world_pool: &PgPool,
        valkey: Valkey,
        world_id: Uuid,
        gateway: SocketAddr,
    ) -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(EDGE_CONFIG);
        let mut config = load_edge_config(&path).expect("edge config");
        config.worlds[0].id = world_id;
        config.rep_address = gateway.to_string();
        let config = Arc::new(config);
        let app = edge_app(Arc::clone(&config), global, world_pool, valkey);
        let listener = TcpListener::bind(LOOPBACK).await.expect("bind HTTP");
        let http_address = listener.local_addr().expect("HTTP address");
        tokio::spawn(async move { axum::serve(listener, app).await });
        let login_host = config.login_host();
        let game_host = config.game_host();
        let http = reqwest::Client::builder()
            .resolve(&login_host, http_address)
            .resolve(&game_host, http_address)
            .build()
            .expect("HTTP client");
        let port = http_address.port();
        Self {
            http,
            login_url: format!("http://{login_host}:{port}"),
            game_url: format!("http://{game_host}:{port}"),
        }
    }

    /// The settings of a new account with one character.
    fn new_account(&self, character_name: &str) -> WalkSettings {
        let random = Uuid::new_v4().simple().to_string();
        WalkSettings {
            login_url: self.login_url.clone(),
            game_url: self.game_url.clone(),
            account: format!("{ACCOUNT_PREFIX}{}", &random[..ACCOUNT_RANDOM_DIGITS]),
            password: PASSWORD.to_owned(),
            character_name: character_name.to_owned(),
        }
    }
}

/// Runs `test` with migrated global and world schemas.
async fn with_schemas(test: impl AsyncFnOnce(&PgPool, &PgPool)) {
    install_crypto_provider();
    with_test_schema(async |global| {
        GLOBAL_MIGRATOR.run(global).await.expect("migrate global");
        with_test_schema(async |world| {
            WORLD_MIGRATOR.run(world).await.expect("migrate world");
            test(global, world).await;
        })
        .await;
    })
    .await;
}

async fn valkey() -> Valkey {
    Valkey::connect(&test_valkey_url())
        .await
        .expect("connect to Valkey")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_logs_in_and_registers_with_the_gateway_once() {
    with_schemas(async |global, world_pool| {
        // A new world id keeps this run's queue apart from other runs in the shared Valkey.
        let world_id = Uuid::new_v4();
        let valkey = valkey().await;
        let (world, events) = accepting();
        let gateway = spawn_gateway(valkey.clone(), world_id, LIMITS, world, events).await;
        let edge = Edge::start(global, world_pool, valkey, world_id, gateway).await;
        let admission = walk(&edge.http, &edge.new_account(CHARACTER_NAME), |_| {})
            .await
            .expect("walk");
        assert_eq!(admission.rep_address, gateway);

        let token = login_token(&admission.token);
        let outcome = run_rep(gateway, client(token.clone()), HOLD, WAIT)
            .await
            .expect("register");
        assert_eq!(outcome.status, Some(REGISTRATION_STATUS_AUTHORIZED));
        assert_eq!(outcome.server_version.as_deref(), Some(VERSION));
        assert!(outcome.pings > 0);
        let again = run_rep(gateway, client(token), HOLD, WAIT)
            .await
            .expect("register again");
        assert_eq!(again.status, Some(REGISTRATION_STATUS_REFUSED));
    })
    .await;
}

/// The committed `config/sim.toml`.
fn sim_config() -> SimConfig {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SIM_CONFIG);
    load_sim_config(&path).expect("sim config")
}

/// Its start position.
fn start() -> Position {
    sim_config().start_position.position()
}

/// Starts `sim` for `world_id` on the world database and waits for its first heartbeat.
async fn start_sim(world_id: Uuid, sim_id: &BusId, world_pool: &PgPool) {
    let bus = test_bus().await;
    let mut beats = bus
        .subscribe::<Heartbeat>(subject::sim_heartbeat(world_id, sim_id))
        .await
        .expect("subscribe");
    bus.flush().await.expect("flush");
    let settings = LoopSettings {
        sim_id: sim_id.clone(),
        world_id,
        level: LEVEL.to_owned(),
        start: start(),
        world: sim_config().world_settings().expect("world settings"),
        save_interval: SAVE_INTERVAL,
        heartbeat_interval: HEARTBEAT,
        heartbeat_misses: HEARTBEAT_MISSES,
    };
    tokio::spawn(serve_sim(
        settings,
        Arc::new(NoGround),
        bus,
        world_pool.clone(),
        std::future::pending(),
    ));
    tokio::time::timeout(WAIT, beats.next())
        .await
        .expect("sim heartbeat in time")
        .expect("open")
        .expect("decode");
}

#[tokio::test(flavor = "multi_thread")]
async fn two_clients_see_each_other_and_a_saved_position_returns() {
    with_schemas(async |global, world_pool| {
        let world_id = Uuid::new_v4();
        let unique = Uuid::new_v4().simple().to_string();
        let sim_id = BusId::try_from(format!("sim-{unique}")).expect("id");
        let gateway_id = BusId::try_from(format!("gw-{unique}")).expect("id");
        start_sim(world_id, &sim_id, world_pool).await;
        let (world, events) = BusWorld::start(
            test_bus().await,
            WorldLinkSettings {
                world_id,
                gateway_id,
                sim_id,
                join_timeout: JOIN_TIMEOUT,
                heartbeat_interval: HEARTBEAT,
                heartbeat_misses: HEARTBEAT_MISSES,
            },
        )
        .await
        .expect("world link");
        let valkey = valkey().await;
        let gateway = spawn_gateway(valkey.clone(), world_id, LIMITS, world, events).await;
        let edge = Edge::start(global, world_pool, valkey, world_id, gateway).await;

        let second = edge.new_account(SECOND_CHARACTER_NAME);
        let settings = PairSettings {
            first: edge.new_account(FIRST_CHARACTER_NAME),
            second: second.clone(),
            hold: PAIR_HOLD,
            registration_timeout: WAIT,
        };
        let report = run_pair(&edge.http, &settings, |token| Ok(client(token)), |_| {})
            .await
            .expect("pair");
        assert!(
            report.passed(TOLERANCE_M),
            "{:?}",
            report.checks(TOLERANCE_M)
        );

        // The second client left before the first one's second session (one hold ago), so its
        // last save is done. A position written now is what its next login must show.
        let admission = walk(&edge.http, &second, |_| {}).await.expect("walk");
        let token = login_token(&admission.token);
        let character = String::from_utf8(token.character_id.clone()).expect("text");
        let character = Uuid::parse_str(&character).expect("character id");
        let start = start();
        let saved = SavedPosition {
            level: LEVEL.to_owned(),
            x: f64::from(start.x + SAVED_OFFSET_M),
            y: f64::from(start.y),
            z: f64::from(start.z),
            yaw: NO_YAW,
        };
        save_character_position(world_pool, character, &saved)
            .await
            .expect("save");
        let back = session(gateway, client(token), PAIR_HOLD, WAIT)
            .await
            .expect("session");
        let expected =
            WorldPosition::from_metres(start.x + SAVED_OFFSET_M, start.y, start.z).expect("finite");
        let own = back.view.own_position().expect("own position");
        assert!(distance(own, expected) <= TOLERANCE_M);
    })
    .await;
}
