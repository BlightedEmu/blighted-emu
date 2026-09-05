// SPDX-License-Identifier: AGPL-3.0-or-later
//! `world-director`: the cells, joins and admission slots of one world.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// The committed example configuration.
const DEFAULT_CONFIG: &str = "config/world-director.toml";

#[derive(Parser)]
#[command(
    name = "world-director",
    about = "World director of the Blighted New World emulator."
)]
struct Cli {
    /// Configuration file. The world database password comes from the variable it names.
    #[arg(long, default_value = DEFAULT_CONFIG)]
    config: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match world_director::run::serve(&cli.config) {
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
