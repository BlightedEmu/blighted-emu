// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool`: builds generated table code, the content bundle and the world files from a
//! New World install.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use content_tool::build::{DEFAULT_CONTENT_DIR, default_bundle_path, run_build};
use content_tool::client_build::load_client_build;
use content_tool::codegen::run_codegen;
use content_tool::error::ToolError;
use content_tool::source::GameSource;
use content_tool::world::{LevelReport, run_world};

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
    /// Writes one world file per level and checks each by reloading it.
    World {
        /// Output folder for `world-<build>-<level>.bin`.
        #[arg(long, default_value = DEFAULT_CONTENT_DIR)]
        out_dir: PathBuf,
        /// Content bundle for the spawn ID check (default: `local-data/content/bundle-<build>.bin`).
        #[arg(long)]
        bundle: Option<PathBuf>,
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
        Command::World { out_dir, bundle } => {
            let bundle = bundle.unwrap_or_else(|| default_bundle_path(&client_build));
            print_world(&run_world(&source, &client_build, &bundle, &out_dir)?);
        }
    }
    Ok(())
}

fn print_world(reports: &[LevelReport]) {
    for report in reports {
        let counts = &report.counts;
        println!(
            "world {}: {} regions ({} with terrain), {} npcs, {} vitals, {} gatherables, {} lore, {} landmarks, {} encounters, {} capitals, {} bytes -> {}",
            report.level,
            counts.regions,
            counts.terrain_regions,
            counts.npcs,
            counts.vitals,
            counts.gatherables,
            counts.lore,
            counts.landmarks,
            counts.encounters,
            counts.capitals,
            report.file_bytes,
            report.path.display()
        );
        for (section, records) in &report.skipped.sections {
            println!("  skipped section {section:08x}: {records} records");
        }
        for ((section, field), count) in &report.skipped.fields {
            println!("  skipped field {section:08x}/{field:08x}: {count}");
        }
        let unmatched = &report.unmatched;
        println!(
            "  unmatched ids: {} npcs, {} vitals, {} gatherables, {} lore",
            unmatched.npcs, unmatched.vitals, unmatched.gatherables, unmatched.lore
        );
    }
    let total_bytes: u64 = reports.iter().map(|report| report.file_bytes).sum();
    println!("world: {} levels, {total_bytes} bytes", reports.len());
}
