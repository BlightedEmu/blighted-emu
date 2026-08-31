// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool`: builds generated table code and the content bundle from a New World install.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use content_tool::build::{default_bundle_path, run_build};
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
    /// Writes the content bundle and checks it by reloading it.
    Build {
        /// Bundle file (default: `local-data/content/bundle-<build>.bin`).
        #[arg(long)]
        out: Option<PathBuf>,
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
        Command::Build { out } => {
            let out = out.unwrap_or_else(|| default_bundle_path(&client_build));
            let summary = run_build(&source, &client_build, &out)?;
            println!(
                "build: {} datasheets, {} rows, {} text files, {} text entries, {} duplicate text keys, {} keyless text entries -> {}",
                summary.datasheet_files,
                summary.rows,
                summary.text_files,
                summary.text_entries,
                summary.duplicate_text_keys,
                summary.keyless_text_entries,
                summary.path.display()
            );
        }
    }
    Ok(())
}
