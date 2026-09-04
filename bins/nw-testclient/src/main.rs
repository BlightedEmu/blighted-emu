// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient`: logs in over HTTPS like the game, then registers with `rep-gateway` and stays
//! connected. Prints one line per step; exits with 0 only when every step passed.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use nw_dtls::DtlsClientConfig;
use nw_testclient::config::{PASSWORD_ENV, load_testclient_config};
use nw_testclient::rep::RepClient;
use nw_testclient::udp::run_rep;
use nw_testclient::walk::{WalkSettings, login_token, walk};

/// The committed example configuration.
const DEFAULT_CONFIG: &str = "config/nw-testclient.toml";
/// Registration status of an accepted login.
const STATUS_AUTHORIZED: u32 = 0;

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
    let trusted = std::fs::read(&config.certificate_file)?;
    let client = RepClient::new(
        &DtlsClientConfig::new(&trusted)?,
        config.mtu,
        login_token(&admission.token),
        0,
    )?;
    let outcome = run_rep(
        admission.rep_address,
        client,
        Duration::from_secs(config.hold_seconds),
        Duration::from_secs(config.registration_timeout_seconds),
    )
    .await?;
    let Some(status) = outcome.status else {
        println!("no registration response");
        return Ok(false);
    };
    println!(
        "registration status {status}, server version {}",
        outcome.server_version.unwrap_or_default()
    );
    println!(
        "held {} s: {} pings echoed, closed by server: {}",
        config.hold_seconds, outcome.pings, outcome.closed_by_server
    );
    Ok(status == STATUS_AUTHORIZED && outcome.pings > 0 && !outcome.closed_by_server)
}
