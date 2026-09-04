// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool`: builds generated table code, the content bundle and the world files from a
//! New World install, and checks capture ledgers.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use content_tool::build::{DEFAULT_CONTENT_DIR, default_bundle_path, run_build};
use content_tool::capture::{
    CLIENT_TO_SERVER, CaptureError, CaptureReport, SERVER_TO_CLIENT, TypeNames,
};
use content_tool::client_build::load_client_build;
use content_tool::codegen::run_codegen;
use content_tool::error::ToolError;
use content_tool::source::GameSource;
use content_tool::world::{LevelReport, UnmatchedIds, run_world, world_totals};
use nw_world::metadata::SkipCounts;
use nw_world::records::RecordCounts;

const DEFAULT_CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
const DEFAULT_GENERATED_DIR: &str = "crates/nw-content/src/generated";
/// Client type table in the install folder.
const TYPE_INDEX_FILE: &str = "typeindex.json";
/// Message types printed per direction by `capture-check`.
const TOP_TYPES: usize = 30;
/// Decode failures printed by `capture-check`.
const SHOWN_FAILURES: usize = 20;

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
    /// Decodes capture ledgers with the carrier and message codecs; prints counts only.
    CaptureCheck {
        /// `ledger.bin` files (unpacked into `local-data/`).
        #[arg(required = true)]
        ledgers: Vec<PathBuf>,
        /// Aeternum-World `Catalog/uuid_to_class.json`, to print class names next to type indexes.
        #[arg(long)]
        names: Option<PathBuf>,
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
        Command::CaptureCheck { ledgers, names } => {
            let names = match names {
                Some(names) => TypeNames::load(&cli.game_dir.join(TYPE_INDEX_FILE), &names)?,
                None => TypeNames::default(),
            };
            let report = run_capture_check(&ledgers)?;
            print_capture(&report, &names);
            if !report.failures.is_empty() {
                return Err(ToolError::CaptureFailures {
                    count: report.failures.len(),
                });
            }
        }
    }
    Ok(())
}

fn run_capture_check(ledgers: &[PathBuf]) -> Result<CaptureReport, ToolError> {
    let mut report = CaptureReport::default();
    for path in ledgers {
        let bytes = std::fs::read(path).map_err(|source| CaptureError::Io {
            path: path.clone(),
            source,
        })?;
        report.check_ledger(path, &bytes)?;
    }
    Ok(report)
}

fn print_capture(report: &CaptureReport, names: &TypeNames) {
    println!(
        "capture-check: {} files, {} + {} datagrams (server, client), {} + {} messages, {} inline-UUID types, {} decode failures",
        report.files,
        report.datagrams[SERVER_TO_CLIENT as usize],
        report.datagrams[CLIENT_TO_SERVER as usize],
        report.messages[SERVER_TO_CLIENT as usize],
        report.messages[CLIENT_TO_SERVER as usize],
        report.inline_uuid_messages,
        report.failures.len()
    );
    let registration = &report.registration;
    println!(
        "registration: {} responses; status {:?}; four bytes {:?}; versions {:?}; token lengths {:?}; clock within {} ms",
        registration.responses,
        registration.statuses,
        registration.flags,
        registration.versions,
        registration.token_lengths,
        registration.max_clock_offset_ms
    );
    println!(
        "M4 bodies decoded and encoded to the same bytes, by type: {:?}",
        report.bodies
    );
    println!(
        "bundle members decoded and encoded to the same bytes, by class: {:?}; payloads read to the end: {}",
        report.members, report.complete_payloads
    );
    for (direction, label) in [
        (SERVER_TO_CLIENT, "server to client"),
        (CLIENT_TO_SERVER, "client to server"),
    ] {
        let mut types: Vec<(u64, u64)> = report
            .types
            .iter()
            .filter(|((from, _), _)| *from == direction)
            .map(|((_, index), count)| (*index, *count))
            .collect();
        types.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
        println!("types {label}: {} kinds", types.len());
        for (index, count) in types.iter().take(TOP_TYPES) {
            println!(
                "  {index:>5} {count:>9} {}",
                names.name(*index).unwrap_or("")
            );
        }
    }
    for failure in report.failures.iter().take(SHOWN_FAILURES) {
        println!(
            "  failure: {} record {}: {}",
            failure.file.display(),
            failure.record,
            failure.reason
        );
    }
}

fn print_world(reports: &[LevelReport]) {
    for report in reports {
        print_level_summary(
            &format!("world {}", report.level),
            &report.counts,
            report.file_bytes,
            &report.skipped,
            &report.unmatched,
        );
        println!("  file: {}", report.path.display());
    }
    let totals = world_totals(reports);
    print_level_summary(
        &format!("world total ({} levels)", totals.levels),
        &totals.counts,
        totals.file_bytes,
        &totals.skipped,
        &totals.unmatched,
    );
}

fn print_level_summary(
    label: &str,
    counts: &RecordCounts,
    file_bytes: u64,
    skipped: &SkipCounts,
    unmatched: &UnmatchedIds,
) {
    println!(
        "{label}: {} regions ({} with terrain), {} npcs, {} vitals, {} gatherables, {} lore, {} landmarks, {} encounters, {} capitals, {file_bytes} bytes",
        counts.regions,
        counts.terrain_regions,
        counts.npcs,
        counts.vitals,
        counts.gatherables,
        counts.lore,
        counts.landmarks,
        counts.encounters,
        counts.capitals,
    );
    for (section, records) in &skipped.sections {
        println!("  skipped section {section:08x}: {records} records");
    }
    for ((section, field), count) in &skipped.fields {
        println!("  skipped field {section:08x}/{field:08x}: {count}");
    }
    println!(
        "  unmatched ids: {} npcs, {} vitals, {} gatherables, {} lore",
        unmatched.npcs, unmatched.vitals, unmatched.gatherables, unmatched.lore
    );
}
