// SPDX-License-Identifier: AGPL-3.0-or-later
//! The M3 exit test without the game (M3 spec section 12, `infra` feature): the test client logs
//! in over HTTP through the `edge-http` routes on the real PostgreSQL and Valkey, waits in the
//! queue, and registers with the gateway over UDP. The same token is refused the second time.

use std::path::Path;
use std::sync::Arc;

use nw_db::test_db::with_test_schema;
use nw_db::{GLOBAL_MIGRATOR, PgPool, WORLD_MIGRATOR};
use nw_login::config::load_edge_config;
use nw_login::install_crypto_provider;
use nw_login::test_support::{edge_app, test_valkey_url};
use nw_login::valkey::Valkey;
use nw_testclient::udp::run_rep;
use nw_testclient::walk::{WalkSettings, login_token, walk};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::server::tests::{HOLD, LIMITS, VERSION, WAIT, client, spawn_gateway};
use nw_reflect::{REGISTRATION_STATUS_AUTHORIZED, REGISTRATION_STATUS_REFUSED};

const EDGE_CONFIG: &str = "../../config/edge.toml";
const LOOPBACK: &str = "127.0.0.1:0";
const PASSWORD: &str = "password-ok";
const CHARACTER_NAME: &str = "Gateway";
/// Each run makes a new account: a prefix and 12 random hex digits.
const ACCOUNT_PREFIX: &str = "e2e";
const ACCOUNT_RANDOM_DIGITS: usize = 12;

#[tokio::test(flavor = "multi_thread")]
async fn a_client_logs_in_and_registers_with_the_gateway_once() {
    install_crypto_provider();
    with_test_schema(async |global| {
        GLOBAL_MIGRATOR.run(global).await.expect("migrate global");
        with_test_schema(async |world| {
            WORLD_MIGRATOR.run(world).await.expect("migrate world");
            log_in_and_register(global, world).await;
        })
        .await;
    })
    .await;
}

async fn log_in_and_register(global: &PgPool, world_pool: &PgPool) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(EDGE_CONFIG);
    let mut config = load_edge_config(&path).expect("edge config");
    // A new world id keeps this run's queue apart from other runs in the shared Valkey.
    let world_id = Uuid::new_v4();
    config.worlds[0].id = world_id;
    let valkey = Valkey::connect(&test_valkey_url())
        .await
        .expect("connect to Valkey");
    let gateway = spawn_gateway(valkey.clone(), world_id, LIMITS).await;
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
    let random = Uuid::new_v4().simple().to_string();
    let settings = WalkSettings {
        login_url: format!("http://{login_host}:{port}"),
        game_url: format!("http://{game_host}:{port}"),
        account: format!("{ACCOUNT_PREFIX}{}", &random[..ACCOUNT_RANDOM_DIGITS]),
        password: PASSWORD.to_owned(),
        character_name: CHARACTER_NAME.to_owned(),
    };
    let admission = walk(&http, &settings, |_| {}).await.expect("walk");
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
}
