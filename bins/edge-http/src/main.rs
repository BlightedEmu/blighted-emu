// SPDX-License-Identifier: AGPL-3.0-or-later
//! `edge-http`: the HTTPS login server. `serve` runs it; `migrate` updates the databases.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use edge_http::{migrate, serve};

/// The committed example configuration.
const DEFAULT_CONFIG: &str = "config/edge.toml";

#[derive(Parser)]
#[command(
    name = "edge-http",
    about = "Login server of the Blighted New World emulator."
)]
struct Cli {
    /// Configuration file. Secrets come from `NW_SECRET_*` environment variables.
    #[arg(long, default_value = DEFAULT_CONFIG)]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serves HTTPS.
    Serve,
    /// Applies the database migrations of the global and every world database.
    Migrate,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Serve => serve(&cli.config),
        Command::Migrate => migrate(&cli.config),
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
