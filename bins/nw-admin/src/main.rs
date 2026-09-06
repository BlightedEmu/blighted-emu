// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-admin`: local admin web tools.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// The committed example configuration.
const DEFAULT_CONFIG: &str = "config/nw-admin.toml";

#[derive(Parser)]
#[command(
    name = "nw-admin",
    about = "Local admin web tools of the Blighted New World emulator."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serves the admin pages on the configured loopback address until Ctrl-C.
    Serve {
        /// Configuration file.
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
}

fn main() -> ExitCode {
    let Command::Serve { config } = Cli::parse().command;
    match nw_admin::run::serve(&config) {
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
