// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient`: logs in over HTTPS like the game, then registers with `rep-gateway`.
//! `registration` (the default) stays connected, then checks that the same ticket is refused a
//! second time. `pair` runs two clients in the world (M4 spec section 9). `swarm` runs many bots
//! across a sim border (M5 spec section 11). Prints one line per step; exits with 0 only when
//! every step passed.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use nw_content::client_build::load_client_build;
use nw_dtls::DtlsClientConfig;
use nw_reflect::REGISTRATION_STATUS_AUTHORIZED;
use nw_testclient::config::{
    PASSWORD_ENV, SECOND_PASSWORD_ENV, SWARM_PASSWORD_ENV, TestClientConfig, load_testclient_config,
};
use nw_testclient::pair::{PairSettings, run_pair};
use nw_testclient::rep::RepClient;
use nw_testclient::swarm::{NewClient, run_swarm, udp_receive_errors};
use nw_testclient::udp::run_rep;
use nw_testclient::walk::{WalkSettings, login_token, walk};
use nw_world::WorldFile;
use nw_world::world_file::world_file_path;

/// The committed example configuration.
const DEFAULT_CONFIG: &str = "config/nw-testclient.toml";
/// The kernel's network counters (Linux).
const SNMP_PATH: &str = "/proc/net/snmp";

#[derive(Parser)]
#[command(
    name = "nw-testclient",
    about = "Test client of the Blighted New World emulator."
)]
struct Cli {
    /// Configuration file. The password comes from `NW_SECRET_TESTCLIENT_PASSWORD`.
    #[arg(long, default_value = DEFAULT_CONFIG)]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// One client: registration, a hold with pings, and the refused second registration
    /// (the default).
    Registration,
    /// Two clients in the world: each sees the other, and the first logs in again at its saved
    /// position. The second password comes from `NW_SECRET_TESTCLIENT_PASSWORD_B`.
    Pair,
    /// Many bots across a sim border (`[swarm]`). The bot password comes from
    /// `NW_SECRET_TESTCLIENT_SWARM_PASSWORD`.
    Swarm {
        /// Bots of this step instead of `[swarm] bots`.
        #[arg(long)]
        bots: Option<usize>,
    },
}

fn main() -> ExitCode {
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(run(Cli::parse())),
        Err(error) => {
            eprintln!("error: runtime failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> ExitCode {
    match steps(cli).await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn steps(cli: Cli) -> Result<bool, Box<dyn std::error::Error>> {
    let config = load_testclient_config(&cli.config)?;
    nw_login::install_crypto_provider();
    let http = reqwest::Client::builder().build()?;
    let trusted = DtlsClientConfig::new(&std::fs::read(&config.certificate_file)?)?;
    match cli.command.unwrap_or(Command::Registration) {
        Command::Registration => registration(&config, &http, &trusted, &own(&config)?).await,
        Command::Pair => pair(&config, &http, &trusted, own(&config)?).await,
        Command::Swarm { bots } => {
            let mut config = config;
            if let Some(bots) = bots {
                config.swarm.bots = bots;
            }
            swarm(&config, &http, trusted).await
        }
    }
}

/// The test account of `registration` and `pair`.
fn own(config: &TestClientConfig) -> Result<WalkSettings, String> {
    let password = secret(PASSWORD_ENV)?;
    Ok(walk_settings(
        config,
        &config.account,
        &config.character_name,
        password,
    ))
}

async fn swarm(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: DtlsClientConfig,
) -> Result<bool, Box<dyn std::error::Error>> {
    let settings = Arc::new(config.swarm_settings(secret(SWARM_PASSWORD_ENV)?)?);
    let swarm = &config.swarm;
    let build = load_client_build(&swarm.client_build_config)?;
    let path = world_file_path(&swarm.content_dir, &build, &swarm.level);
    let ground = Arc::new(WorldFile::load(&path, &build)?);
    let mtu = config.mtu;
    let trusted = Arc::new(trusted);
    let new_client: NewClient = Arc::new(move |token| RepClient::new(&trusted, mtu, token, 0));
    let errors_before = receive_errors();
    let report = run_swarm(http, Arc::clone(&settings), ground, new_client, |line| {
        println!("{line}");
    })
    .await;
    println!("{}", report.summary());
    match (errors_before, receive_errors()) {
        (Some(before), Some(after)) => println!(
            "UDP receive buffer errors of the PC during the step: {}",
            after.saturating_sub(before)
        ),
        _ => println!("UDP receive buffer errors of the PC: Unknown"),
    }
    for check in report.checks(&settings) {
        let result = if check.passed { "pass" } else { "FAIL" };
        println!("{}: {result}", check.name);
    }
    Ok(report.passed(&settings))
}

/// The PC's count of UDP datagrams dropped for a full receive buffer (Linux).
fn receive_errors() -> Option<u64> {
    let snmp = std::fs::read_to_string(SNMP_PATH).ok()?;
    udp_receive_errors(&snmp)
}

fn secret(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is not set"))
}

fn walk_settings(
    config: &TestClientConfig,
    account: &str,
    character_name: &str,
    password: String,
) -> WalkSettings {
    WalkSettings {
        login_url: config.login_url.clone(),
        game_url: config.game_url.clone(),
        account: account.to_owned(),
        password,
        character_name: character_name.to_owned(),
    }
}

async fn pair(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: &DtlsClientConfig,
    first: WalkSettings,
) -> Result<bool, Box<dyn std::error::Error>> {
    let second = walk_settings(
        config,
        &config.second_account,
        &config.second_character_name,
        secret(SECOND_PASSWORD_ENV)?,
    );
    let settings = PairSettings {
        first,
        second,
        route: config.route()?,
        jump_m: config.jump_m,
        hold: Duration::from_secs(config.hold_seconds),
        registration_timeout: Duration::from_secs(config.registration_timeout_seconds),
    };
    let report = run_pair(
        http,
        &settings,
        |token| RepClient::new(trusted, config.mtu, token, 0),
        |line| println!("{line}"),
    )
    .await?;
    let tolerances = (config.path_tolerance_m, config.position_tolerance_m);
    for check in report.checks(tolerances.0, tolerances.1) {
        let result = if check.passed { "pass" } else { "FAIL" };
        println!("{}: {result}", check.name);
    }
    Ok(report.passed(tolerances.0, tolerances.1))
}

async fn registration(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: &DtlsClientConfig,
    settings: &WalkSettings,
) -> Result<bool, Box<dyn std::error::Error>> {
    let admission = walk(http, settings, |line| println!("{line}")).await?;
    let token = login_token(&admission.token);
    let registration_timeout = Duration::from_secs(config.registration_timeout_seconds);
    let new_client = || RepClient::new(trusted, config.mtu, token.clone(), 0);
    let outcome = run_rep(
        admission.rep_address,
        new_client()?,
        Duration::from_secs(config.hold_seconds),
        registration_timeout,
    )
    .await?;
    let Some(status) = outcome.status else {
        println!("no registration response");
        return Ok(false);
    };
    println!(
        "registration status {status}, server version {}",
        outcome.server_version.as_deref().unwrap_or_default()
    );
    println!(
        "held {:.1} s: {} pings echoed, closed by server: {}",
        outcome.held.as_secs_f64(),
        outcome.pings,
        outcome.closed_by_server
    );
    // The ticket is single-use: the same token must be refused now.
    let again = run_rep(
        admission.rep_address,
        new_client()?,
        Duration::ZERO,
        registration_timeout,
    )
    .await?;
    let Some(second_status) = again.status else {
        println!("no response to the second registration");
        return Ok(false);
    };
    println!("second registration with the same ticket: status {second_status}");
    Ok(status == REGISTRATION_STATUS_AUTHORIZED
        && outcome.pings_kept_up()
        && !outcome.closed_by_server
        && second_status != REGISTRATION_STATUS_AUTHORIZED)
}
