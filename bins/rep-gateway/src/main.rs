// SPDX-License-Identifier: AGPL-3.0-or-later
//! `rep-gateway`: the UDP entry point of a world. `serve` runs it; `generate-certificate` makes
//! its own certificate and key.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rep_gateway::run::{generate_certificate, serve};

/// The committed example configuration.
const DEFAULT_CONFIG: &str = "config/rep-gateway.toml";
/// Validity of a generated certificate: 10 years.
const DEFAULT_VALID_DAYS: u32 = 3650;

#[derive(Parser)]
#[command(
    name = "rep-gateway",
    about = "World entry point of the Blighted New World emulator."
)]
struct Cli {
    /// Configuration file. The signing key comes from `NW_SECRET_SIGNING_KEY`.
    #[arg(long, default_value = DEFAULT_CONFIG)]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serves UDP.
    Serve,
    /// Writes a new self-signed certificate and key to the configured files.
    GenerateCertificate {
        /// Days the certificate is valid.
        #[arg(long, default_value_t = DEFAULT_VALID_DAYS)]
        days: u32,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Serve => serve(&cli.config),
        Command::GenerateCertificate { days } => generate_certificate(&cli.config, days),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            let mut cause = std::error::Error::source(&error);
            while let Some(inner) = cause {
                eprintln!("  caused by: {inner}");
                cause = inner.source();
            }
            ExitCode::FAILURE
        }
    }
}
