// SPDX-License-Identifier: AGPL-3.0-or-later
//! Starts the login HTTPS listener.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use edge_http::run;

/// Repo-relative edge file from `nw-edge-config`.
const DEFAULT_EDGE_CONFIG: &str = nw_edge_config::server::EXAMPLE_EDGE_CONFIG_PATH;

#[derive(Parser)]
#[command(name = "edge-http", about = "Preservation server login queue.")]
struct Cli {
    /// Edge file with the HTTPS listen address and `RepAddress`.
    #[arg(long, default_value = DEFAULT_EDGE_CONFIG)]
    edge_config: PathBuf,
    /// Login file with the database, Valkey, TLS certificate paths, and queue limits.
    #[arg(long)]
    config: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli.edge_config, &cli.config) {
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
