// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool`: builds generated table code, the content bundle and the world files from a
//! New World install, and checks capture ledgers.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use content_tool::build::{DEFAULT_CONTENT_DIR, default_bundle_path, run_build};
use content_tool::capture::CaptureReport;
use content_tool::codegen::run_codegen;
use content_tool::combat::{CombatCheck, DEFAULT_LEVEL, DEFAULT_NWDB_DIR, check, read_nwdb};
use content_tool::error::ToolError;
use content_tool::ftue_entities::{FtueScan, Timeline, read_ledger, write_table};
use content_tool::map_tiles::{DEFAULT_MAP_DIR, MapTilesReport, YIndex, run_map_tiles};
use content_tool::mob_prefabs::{
    DEFAULT_CAPTURE_ROOTS, LedgerSource, MobPrefabsError, PrefabTable, SourceList,
    find_ledger_sources, run_workers, scan_ledger, vitals_names, worker_process, write_entries,
};
use content_tool::source::GameSource;
use content_tool::world::{LevelReport, UnmatchedIds, run_world, world_totals};
use nw_capture::{CLIENT_TO_SERVER, CaptureError, SERVER_TO_CLIENT, TypeNames};
use nw_content::ContentBundle;
use nw_content::client_build::load_client_build;
use nw_content::combat::{CombatData, CombatRows};
use nw_content::ftue_entities::ftue_entities_path;
use nw_content::mob_prefabs::mob_prefabs_path;
use nw_world::WorldFile;
use nw_world::metadata::SkipCounts;
use nw_world::records::RecordCounts;
use nw_world::world_file::world_file_path;

const DEFAULT_CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
const DEFAULT_GENERATED_DIR: &str = "crates/nw-content/src/generated";
/// Client type table in the install folder.
const TYPE_INDEX_FILE: &str = "typeindex.json";
/// Message types printed per direction by `capture-check`.
const TOP_TYPES: usize = 30;
/// Decode failures printed by `capture-check`.
const SHOWN_FAILURES: usize = 20;
/// Failed ledgers and bad archives printed by `mob-prefabs`.
const SHOWN_MOB_FAILURES: usize = 20;
/// Classes printed by `mob-prefabs` that cut spawn card groups short.
const SHOWN_CUT_CLASSES: usize = 10;
/// Unread spawn cards of each kind printed by `ftue-entities`.
const SHOWN_UNREAD_CARDS: usize = 20;

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
    /// Writes the world map tiles of a level as PNG files with a `tiles.json` index.
    MapTiles {
        /// Level whose map tiles are written.
        #[arg(long, default_value = DEFAULT_LEVEL)]
        level: String,
        /// Output folder; the level gets a folder in it.
        #[arg(long, default_value = DEFAULT_MAP_DIR)]
        out_dir: PathBuf,
        /// Which edge tile row 0 is (README, "Admin tools").
        #[arg(long, value_enum, default_value_t = YIndex::SouthUp)]
        y_index: YIndex,
        /// Only count the pixel formats of the tiles; write nothing.
        #[arg(long)]
        formats_only: bool,
    },
    /// Checks that every vitals spawn finds its combat rows, and our mob health against nwdb.info.
    CombatCheck {
        /// Level whose world file is checked.
        #[arg(long, default_value = DEFAULT_LEVEL)]
        level: String,
        /// Content bundle (default: `local-data/content/bundle-<build>.bin`).
        #[arg(long)]
        bundle: Option<PathBuf>,
        /// Folder of nwdb.info creature records.
        #[arg(long, default_value = DEFAULT_NWDB_DIR)]
        nwdb_dir: PathBuf,
    },
    /// Builds the mob prefab table from the capture ledgers; each ledger is read by its own
    /// worker process (run it under a memory limit, for example `ulimit -v 4000000`).
    MobPrefabs {
        /// Folders searched for `ledger.bin` files and zip archives that hold them.
        #[arg(default_values = DEFAULT_CAPTURE_ROOTS)]
        roots: Vec<PathBuf>,
        /// Output file (default: `local-data/content/mob-prefabs-<build>.json`).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Content bundle for the vitals names (default: `local-data/content/bundle-<build>.bin`).
        #[arg(long)]
        bundle: Option<PathBuf>,
        /// Worker processes at a time (default: the number of CPU threads).
        #[arg(long)]
        jobs: Option<NonZeroUsize>,
    },
    /// Builds the tutorial entity table (static entities, asset entities and the mobs they name)
    /// from the capture of a new character's tutorial run (run it under a memory limit, for
    /// example `ulimit -v 4000000`).
    FtueEntities {
        /// The `ledger.bin` file.
        ledger: PathBuf,
        /// Output file (default: `local-data/content/ftue-entities-<build>.json`).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Worker of `mob-prefabs`: reads one ledger and prints its tally as JSON.
    #[command(name = "mob-prefabs-ledger", hide = true)]
    MobPrefabsLedger {
        /// The ledger file, or the zip archive that holds it.
        #[arg(long)]
        ledger: PathBuf,
        /// The ledger's entry name in the zip archive.
        #[arg(long)]
        entry: Option<String>,
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
    match cli.command {
        Command::Codegen { out_dir } => {
            let source = GameSource::open(&cli.game_dir)?;
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
            let source = GameSource::open(&cli.game_dir)?;
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
            let source = GameSource::open(&cli.game_dir)?;
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
        Command::MapTiles {
            level,
            out_dir,
            y_index,
            formats_only,
        } => {
            let source = GameSource::open(&cli.game_dir)?;
            print_map_tiles(&run_map_tiles(
                &source,
                &level,
                &out_dir,
                y_index,
                formats_only,
            )?);
        }
        Command::CombatCheck {
            level,
            bundle,
            nwdb_dir,
        } => {
            let bundle = bundle.unwrap_or_else(|| default_bundle_path(&client_build));
            let tables = ContentBundle::load(&bundle, &client_build)?.tables;
            let data = CombatData::from_tables(&tables, &CombatRows::default())?;
            drop(tables);
            let world_path = world_file_path(Path::new(DEFAULT_CONTENT_DIR), &client_build, &level);
            let world = WorldFile::load(&world_path, &client_build)?;
            let nwdb = read_nwdb(&nwdb_dir)?;
            print_combat(
                &check(&data, &world.vitals, &nwdb.creatures),
                nwdb.unreadable,
            );
        }
        Command::MobPrefabs {
            roots,
            out,
            bundle,
            jobs,
        } => {
            let out = out
                .unwrap_or_else(|| mob_prefabs_path(Path::new(DEFAULT_CONTENT_DIR), &client_build));
            let bundle = bundle.unwrap_or_else(|| default_bundle_path(&client_build));
            let global_args = [
                OsString::from("--game-dir"),
                cli.game_dir.into_os_string(),
                OsString::from("--client-build-config"),
                cli.client_build_config.into_os_string(),
            ];
            run_mob_prefabs(&roots, &out, &bundle, &client_build, jobs, &global_args)?;
        }
        Command::FtueEntities { ledger, out } => {
            let out = out.unwrap_or_else(|| {
                ftue_entities_path(Path::new(DEFAULT_CONTENT_DIR), &client_build)
            });
            let scan = content_tool::ftue_entities::scan_ledger(&read_ledger(&ledger)?)?;
            write_table(&out, &scan.table)?;
            print_ftue_entities(&scan, &out);
        }
        Command::MobPrefabsLedger { ledger, entry } => {
            let tally = scan_ledger(
                &LedgerSource {
                    path: ledger,
                    entry,
                }
                .read()?,
            );
            let mut stdout = std::io::stdout().lock();
            serde_json::to_writer(&mut stdout, &tally).map_err(MobPrefabsError::from)?;
            stdout.flush().map_err(|source| MobPrefabsError::Io {
                path: PathBuf::from("stdout"),
                source,
            })?;
        }
    }
    Ok(())
}

fn run_mob_prefabs(
    roots: &[PathBuf],
    out: &Path,
    bundle: &Path,
    client_build: &str,
    jobs: Option<NonZeroUsize>,
    global_args: &[OsString],
) -> Result<(), ToolError> {
    let list = find_ledger_sources(roots)?;
    let exe = std::env::current_exe().map_err(|source| MobPrefabsError::Io {
        path: PathBuf::from("content-tool"),
        source,
    })?;
    let jobs = jobs
        .or_else(|| std::thread::available_parallelism().ok())
        .unwrap_or(NonZeroUsize::MIN);
    let results = run_workers(&list.sources, jobs, |source| {
        worker_process(&exe, global_args, source)
    });
    let mut table = PrefabTable::default();
    let mut failures = Vec::new();
    for result in results {
        match result {
            Ok(tally) => table.add(tally),
            Err(error) => failures.push(error.to_string()),
        }
    }
    let names = vitals_names(&ContentBundle::load(bundle, client_build)?.tables);
    let entries = table.entries(&names);
    write_entries(out, &entries)?;
    let assets: HashSet<&str> = entries.iter().map(|entry| entry.asset.as_str()).collect();
    print_mob_prefabs(&list, &table, &failures);
    println!(
        "entries: {}, distinct assets: {}, with a vitals name: {}, with a death: {} -> {}",
        entries.len(),
        assets.len(),
        entries
            .iter()
            .filter(|entry| entry.vitals_name.is_some())
            .count(),
        entries
            .iter()
            .filter(|entry| !entry.death_members.is_empty())
            .count(),
        out.display()
    );
    Ok(())
}

fn print_mob_prefabs(list: &SourceList, table: &PrefabTable, failures: &[String]) {
    let stats = &table.stats;
    println!(
        "mob-prefabs: {} ledgers found, {} merged, {} duplicates, {} failed; {} rar archives not read, {} bad zip archives",
        list.sources.len(),
        table.ledgers,
        table.duplicates,
        failures.len(),
        list.rar_archives,
        list.bad_archives.len()
    );
    println!(
        "bundles {} ({} bad), mob first groups {}, player cards {}, cards without vitals {}, cut card groups {}, deaths {}, resyncs {}, lost payload tails {}, stream failures {}",
        stats.bundles,
        stats.bad_bundles,
        stats.first_groups,
        stats.player_cards,
        stats.cards_without_vitals,
        stats.cut_cards,
        stats.deaths,
        stats.resyncs,
        stats.lost_tails,
        stats.stream_failures
    );
    let mut cut: Vec<(u64, u64)> = stats.cut_classes.iter().map(|(c, n)| (*c, *n)).collect();
    cut.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    println!(
        "classes without a reader that cut card groups (class, groups): {:?}",
        &cut[..cut.len().min(SHOWN_CUT_CLASSES)]
    );
    for failure in failures.iter().take(SHOWN_MOB_FAILURES) {
        println!("  failed: {failure}");
    }
    for (path, reason) in list.bad_archives.iter().take(SHOWN_MOB_FAILURES) {
        println!("  bad archive: {}: {reason}", path.display());
    }
}

fn print_ftue_entities(scan: &FtueScan, out: &Path) {
    let stats = &scan.stats;
    let table = &scan.table;
    println!(
        "ftue-entities: {} bundles, {} static and {} asset entities, {} mobs ({} mob cards), {} spawns ({} sent again), {} changes ({} sent again), {} static and {} asset cards in unread groups -> {}",
        stats.bundles,
        table.entities.len(),
        table.asset_entities.len(),
        table.mobs.len(),
        stats.mob_cards,
        stats.spawns,
        stats.repeated_spawns,
        stats.changes,
        stats.repeated_changes,
        stats.unread_cards.len(),
        stats.unread_asset_cards.len(),
        out.display()
    );
    for entity in &table.entities {
        print_timeline(&entity.gde_ref_prefix, &entity.timeline);
    }
    for entity in &table.asset_entities {
        print_timeline(&entity.asset, &entity.timeline);
    }
    for mob in &table.mobs {
        println!(
            "  mob {} (vitals {:08x}) first card at {} ms",
            mob.asset, mob.vitals_id, mob.first_card_ms
        );
    }
    for (time_ms, prefix) in stats.unread_cards.iter().take(SHOWN_UNREAD_CARDS) {
        println!("  unread card at {time_ms} ms: {}", hex::encode(prefix));
    }
    for (time_ms, asset) in stats.unread_asset_cards.iter().take(SHOWN_UNREAD_CARDS) {
        println!(
            "  unread asset card at {time_ms} ms: {}",
            hex::encode(asset)
        );
    }
}

fn print_timeline(name: &str, timeline: &Timeline) {
    println!(
        "  {name}: spawns at {:?}, {} changes, despawns at {:?}",
        timeline
            .spawns
            .iter()
            .map(|spawn| (spawn.time_ms, spawn.interest, spawn.members.len()))
            .collect::<Vec<_>>(),
        timeline.changes.len(),
        timeline.despawns
    );
}

fn print_map_tiles(report: &MapTilesReport) {
    println!("map-tiles: tiles per level {:?}", report.per_level);
    println!("formats: {:?}", report.formats);
    println!("unreadable tiles (left out): {}", report.unreadable.len());
    for (path, reason) in &report.unreadable {
        println!("  {path}: {reason}");
    }
}

fn print_combat(report: &CombatCheck, unreadable: usize) {
    println!(
        "combat-check: {} spawns, {} without a mob type row, {} without a level row",
        report.spawns, report.without_type, report.without_level
    );
    println!(
        "nwdb: {} of {} creature records match our max health, {} more match it at another level ({} files unreadable)",
        report.nwdb_matched, report.nwdb_records, report.nwdb_other_level, unreadable
    );
    for mismatch in &report.mismatches {
        let ours = mismatch
            .ours
            .map_or_else(|| "no rows".to_owned(), |ours| ours.to_string());
        println!(
            "  {} level {}: nwdb {}, ours {ours}",
            mismatch.vitals_id, mismatch.level, mismatch.nwdb
        );
    }
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
    println!(
        "new connections inside a capture context: {}",
        report.connections
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
