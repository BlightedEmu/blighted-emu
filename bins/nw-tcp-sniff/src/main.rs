// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-tcp-sniff`: a diagnostic TCP listener that records the raw bytes a client sends.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use nw_tcp_sniff::{SniffConfig, serve};

/// Default listen address: the `rep-default-port` of the client.
const DEFAULT_LISTEN: &str = "127.0.0.1:33435";
/// Default directory for the dump files.
const DEFAULT_OUT_DIR: &str = "local-data/tcp-sniff";
/// Default `tracing` filter.
const DEFAULT_LOG_FILTER: &str = "info";

#[derive(Parser)]
#[command(
    name = "nw-tcp-sniff",
    about = "Records the raw bytes a client sends over TCP, to learn the transport framing."
)]
struct Cli {
    /// Address to listen on.
    #[arg(long, default_value = DEFAULT_LISTEN)]
    listen: SocketAddr,
    /// Directory for the per-connection dump files.
    #[arg(long, default_value = DEFAULT_OUT_DIR)]
    out_dir: PathBuf,
    /// `tracing` filter, for example `info`.
    #[arg(long, default_value = DEFAULT_LOG_FILTER)]
    log_filter: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config = SniffConfig {
        listen: cli.listen,
        out_dir: cli.out_dir,
        log_filter: cli.log_filter,
    };
    match serve(config) {
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
