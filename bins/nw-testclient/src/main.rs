// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient`: logs in over HTTPS like the game, then registers with `rep-gateway` and stays
//! connected, then checks that the same ticket is refused a second time. Prints one line per step;
//! exits with 0 only when every step passed.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use nw_dtls::DtlsClientConfig;
use nw_reflect::REGISTRATION_STATUS_AUTHORIZED;
use nw_testclient::config::{PASSWORD_ENV, load_testclient_config};
use nw_testclient::rep::RepClient;
use nw_testclient::udp::run_rep;
use nw_testclient::walk::{WalkSettings, login_token, walk};

/// The committed example configuration.
const DEFAULT_CONFIG: &str = "config/nw-testclient.toml";

#[derive(Parser)]
#[command(
    name = "nw-testclient",
    about = "Test client of the Blighted New World emulator."
)]
struct Cli {
    /// Configuration file. The password comes from `NW_SECRET_TESTCLIENT_PASSWORD`.
    #[arg(long, default_value = DEFAULT_CONFIG)]
    config: PathBuf,
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
    let password = std::env::var(PASSWORD_ENV).map_err(|_| format!("{PASSWORD_ENV} is not set"))?;
    nw_login::install_crypto_provider();
    let http = reqwest::Client::builder().build()?;
    let settings = WalkSettings {
        login_url: config.login_url.clone(),
        game_url: config.game_url.clone(),
        account: config.account.clone(),
        password,
        character_name: config.character_name.clone(),
    };
    let admission = walk(&http, &settings, |line| println!("{line}")).await?;
    let trusted = DtlsClientConfig::new(&std::fs::read(&config.certificate_file)?)?;
    let token = login_token(&admission.token);
    let registration_timeout = Duration::from_secs(config.registration_timeout_seconds);
    let new_client = || RepClient::new(&trusted, config.mtu, token.clone(), 0);
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
