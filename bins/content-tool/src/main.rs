// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool`: builds generated table code and the content bundle from a New World install.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use content_tool::client_build::load_client_build;
use content_tool::codegen::run_codegen;
use content_tool::error::ToolError;
use content_tool::source::GameSource;

const DEFAULT_CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
const DEFAULT_GENERATED_DIR: &str = "crates/nw-content/src/generated";

#[derive(Parser)]
#[command(
    name = "content-tool",
    about = "Builds content data from a New World client install."
)]
struct Cli {
    /// New World install folder (the folder that holds `assets/` and `Bin64/`).
    #[arg(long, env = "NW_GAME_DIR")]
    game_dir: PathBuf,
    /// File with the pinned client build.
    #[arg(long, default_value = DEFAULT_CLIENT_BUILD_CONFIG)]
    client_build_config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Writes the generated table structs.
    Codegen {
        /// Output folder.
        #[arg(long, default_value = DEFAULT_GENERATED_DIR)]
        out_dir: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
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

fn run(cli: Cli) -> Result<(), ToolError> {
    let client_build = load_client_build(&cli.client_build_config)?;
    let source = GameSource::open(&cli.game_dir)?;
    match cli.command {
        Command::Codegen { out_dir } => {
            let summary = run_codegen(&source, &client_build, &out_dir)?;
            println!(
                "codegen: {} datasheets, {} table types, {} files written, {} stale files removed",
                summary.datasheet_files,
                summary.table_types,
                summary.files_written,
                summary.stale_files_removed
            );
        }
    }
    Ok(())
}
