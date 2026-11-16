// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool`: builds generated table code, the content bundle and the world files from a
//! New World install, builds the content folder (`content build`), and checks capture ledgers.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use content_tool::ability_instances::build_ability_instances;
use content_tool::ai_data::{AiDataReport, DEFAULT_SERVER_PAK, build_ai_data};
use content_tool::ai_trace::{
    AiTrace, DEFAULT_TRACE_PATH, read_creatures, trace_ledger, write_trace,
};
use content_tool::breach_tiers::build_breach_tiers;
use content_tool::build::{DEFAULT_CONTENT_DIR, run_build};
use content_tool::camp_slices::build_camp_slices;
use content_tool::capture::CaptureReport;
use content_tool::catacomb_tiles::build_catacomb_tiles;
use content_tool::catacombs::{
    CatacombCheck, DEFAULT_GAME_MODE as DEFAULT_CATACOMB_MODE,
    DEFAULT_RUNS as DEFAULT_CATACOMB_RUNS, check as catacomb_check, map_rows as catacomb_map_rows,
};
use content_tool::claim_plots::build_claim_plots;
use content_tool::codegen::run_codegen;
use content_tool::collision::{CollisionError, CollisionReport, PakIndex, build_level_collision};
use content_tool::combat::{CombatCheck, DEFAULT_LEVEL, DEFAULT_NWDB_DIR, check, read_nwdb};
use content_tool::content::{
    BuildLine, BuildOptions, BuildOutcome, MADE_BY, MigratePlan, MoveKind, build_content,
    content_status, made_from, migrate, plan_migration,
};
use content_tool::creature_animations::{CreatureAnimationsReport, build_rows};
use content_tool::dungeon_gates::{DungeonGatesReport, build_dungeon_gates};
use content_tool::dungeon_scripts::{DungeonScriptsReport, build_dungeon_scripts};
use content_tool::effect_areas::{EffectAreasReport, build_effect_areas};
use content_tool::error::ToolError;
use content_tool::ftue_entities::{FtueScan, Timeline, read_ledger, write_table};
use content_tool::leaderboards::write_leaderboard_table;
use content_tool::level_doors::{LevelDoorsReport, build_level_doors};
use content_tool::map_tiles::{MapTilesReport, YIndex, run_map_tiles};
use content_tool::mob_prefabs::{
    DEFAULT_CAPTURE_ROOTS, LedgerSource, MobPrefabsError, PrefabTable, SourceList,
    find_ledger_sources, run_workers, scan_ledger, vitals_names, worker_process, write_entries,
};
use content_tool::music_charts::build_music_charts;
use content_tool::player_abilities::{PlayerAbilitiesReport, build_abilities};
use content_tool::player_attacks::{BundleRows, PlayerAttacksReport, build_stances, read_sources};
use content_tool::quick_courses::build_quick_courses;
use content_tool::realm_pack::install::{TableOutcome, install_pack};
use content_tool::realm_pack::make::make_pack;
use content_tool::roads::{RoadsReport, build_level_roads};
use content_tool::source::GameSource;
use content_tool::store::write_store_table;
use content_tool::water::build_level_water;
use content_tool::world::{LevelReport, UnmatchedIds, run_world, world_totals};
use nw_capture::{CLIENT_TO_SERVER, CaptureError, SERVER_TO_CLIENT, TypeNames};
use nw_content::ContentBundle;
use nw_content::breach_tiers::BreachTiers;
use nw_content::client_build::load_client_build;
use nw_content::combat::{CombatData, CombatRows};
use nw_content::creature_animations::write_rows;
use nw_content::layout::{
    ABILITY_INSTANCES, AI_DATA, BREACH_TIERS, CAMP_SLICES, CATACOMB_LEVEL, CATACOMB_TILES,
    CLAIM_PLOTS, COLLISION, COLLISION_LEVELS, CREATURE_ANIMATIONS, ContentLayout, DUNGEON_GATES,
    DUNGEON_SCRIPTS, EFFECT_AREAS, FTUE_ENTITIES, FileRef, LEADERBOARDS, LEVEL_DOORS,
    LEVEL_DOORS_LEVELS, LevelFile, MOB_PREFABS, MUSIC_CHARTS, OPEN_WORLD_LEVEL, PLAYER_ABILITIES,
    PLAYER_ATTACKS, QUICK_COURSES, RETAIL_BUNDLE, RETAIL_WORLD, ROADS, ROADS_LEVELS, STEPS, STORE,
    Step, StepLevels, StepSpec, WATER, WATER_LEVELS,
};
use nw_content::manifest::{record, record_made_from};
use nw_content::mob_prefabs::MobPrefabs;
use nw_content::player_attacks::write_stances;
use nw_design::{DesignMode, DesignPaths, DesignReport, run_design};
use nw_world::WorldFile;
use nw_world::metadata::SkipCounts;
use nw_world::records::RecordCounts;

const DEFAULT_CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
/// Where `realm-pack make` writes the pack.
const DEFAULT_PACK_DIR: &str = "local-data/realm-pack";
/// The levels whose effect areas `effect-areas` reads by default: the open world, the
/// catacombs and the dungeon whose sulfur pools hold `Env_SulfurPool_DoT`.
const DEFAULT_EFFECT_AREA_LEVELS: [&str; 3] = [
    OPEN_WORLD_LEVEL,
    CATACOMB_LEVEL,
    "nw_dungeon_brimstonesands_00",
];
/// The team's design files (admin studio spec section 5).
const DEFAULT_DESIGN_DIR: &str = "content";
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
/// Members without a reader left out of spawn groups, printed by `ftue-entities`.
const SHOWN_SKIPPED_MEMBERS: usize = 20;
/// What `content status` shows when the manifest has no time for a file.
const NO_TIME: &str = "-";

#[derive(Parser)]
#[command(
    name = "content-tool",
    about = "Builds content data from a New World client install."
)]
struct Cli {
    /// New World install folder (the folder that holds `assets/` and `Bin64/`).
    #[arg(long, env = "NW_GAME_DIR")]
    game_dir: Option<PathBuf>,
    /// File with the pinned client build.
    #[arg(long, default_value = DEFAULT_CLIENT_BUILD_CONFIG)]
    client_build_config: PathBuf,
    /// The content folder: the files of the pinned build go into `<content-dir>/<build>/`
    /// (README, "Content data").
    #[arg(long, default_value = DEFAULT_CONTENT_DIR)]
    content_dir: PathBuf,
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
    /// Builds, checks and migrates the whole content folder.
    Content {
        /// What to do.
        #[command(subcommand)]
        action: ContentAction,
    },
    /// Writes the retail content bundle (`retail/bundle.bin`) and checks it by reloading it.
    Build,
    /// Writes one retail world file per level (`retail/levels/<level>/world.bin`) and checks each
    /// by reloading it.
    World,
    /// Writes the collision file of each level (its solid shapes from the game paks) and checks
    /// it by reloading it.
    Collision {
        /// Levels to build.
        #[arg(default_values = COLLISION_LEVELS)]
        levels: Vec<String>,
    },
    /// Writes the road file of each level (the road cells of its tract maps from the game paks)
    /// and checks it by reloading it.
    Roads {
        /// Levels to build.
        #[arg(default_values = ROADS_LEVELS)]
        levels: Vec<String>,
    },
    /// Writes the water file of each level (the water surfaces of its region water trees from
    /// the game paks) and checks it by reloading it.
    Water {
        /// Levels to build.
        #[arg(default_values = WATER_LEVELS)]
        levels: Vec<String>,
    },
    /// Merges the design files over the retail files and writes the bundle and world files the
    /// servers read. Needs no game install.
    Design {
        /// The design files.
        #[arg(long, default_value = DEFAULT_DESIGN_DIR)]
        design: PathBuf,
        /// Runs every check and writes nothing.
        #[arg(long)]
        check: bool,
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
    /// Writes the world map tiles of a level as PNG files with a `tiles.json` index into the
    /// map folder of the content folder (`<content-dir>/map/<level>/`).
    MapTiles {
        /// Level whose map tiles are written.
        #[arg(long, default_value = DEFAULT_LEVEL)]
        level: String,
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
    },
    /// Builds the creature animation table from the game paks, for the creatures of the mob
    /// prefab table.
    CreatureAnimations,
    /// Builds the AI data file (every Server.pak behavior tree, the AI of every creature slice
    /// and the CAGE task resolution of their grids) from Server.pak and the game paks, for the
    /// creatures of the creature animation table.
    AiData {
        /// The Server.pak file.
        #[arg(long, default_value = DEFAULT_SERVER_PAK)]
        server_pak: PathBuf,
    },
    /// Writes what the retail mobs of a capture ledger did (ALC states, FullBody animations),
    /// the own player's positions and the hits on the player, as JSON (run it under a memory
    /// limit, for example `ulimit -v 4000000`).
    AiTrace {
        /// The `ledger.bin` file.
        ledger: PathBuf,
        /// Output file.
        #[arg(long, default_value = DEFAULT_TRACE_PATH)]
        out: PathBuf,
    },
    /// Builds the player attack table (each weapon stance's basic attacks, dodge and item uses)
    /// from the game paks and the merged content bundle.
    PlayerAttacks,
    /// Builds the player ability table (each weapon ability list's active abilities: hit,
    /// markers, cooldowns, mana cost, spells) from the game paks and the merged content bundle.
    PlayerAbilities,
    /// Makes the layouts of many runs of a generated game mode (the catacombs) and checks their
    /// tile slices against the paks; prints the counts and the first run's map.
    CatacombCheck {
        /// The game mode.
        #[arg(long, default_value = DEFAULT_CATACOMB_MODE)]
        game_mode: String,
        /// Runs to make (seeds 0 and up).
        #[arg(long, default_value_t = DEFAULT_CATACOMB_RUNS)]
        runs: u64,
    },
    /// Writes the breach tiers file: the difficulty of each corrupted breach place, from the
    /// world map points of the retail ledgers (read by `dungeon-scripts`).
    BreachTiers {
        /// Retail `ledger.bin` files.
        #[arg(required = true)]
        ledgers: Vec<PathBuf>,
    },
    /// Writes the dungeon script file: the encounters and loot containers of every instance
    /// level's script slice, from the game paks and the merged content bundle.
    DungeonScripts,
    /// Writes the gates file: the gates, doors, switches and arena blockers of every instance
    /// level's script slice, and the replicated entities of its encounter slices, from the game
    /// paks and the merged content bundle.
    DungeonGates,
    /// Writes the effect areas file: the status effect areas of the levels' placed slices and
    /// of the spell slices whose effects change an affliction meter, from the game paks and the
    /// merged content bundle.
    EffectAreas {
        /// Levels whose status effect areas are read.
        #[arg(default_values = DEFAULT_EFFECT_AREA_LEVELS)]
        levels: Vec<String>,
    },
    /// Writes the doors file of each level: every capital of the level whose slice has a door,
    /// with the replicated components retail sends at its spawn, from the level's merged world
    /// file and the game paks.
    LevelDoors {
        /// Levels to build.
        #[arg(default_values = LEVEL_DOORS_LEVELS)]
        levels: Vec<String>,
    },
    /// Writes the music chart file: the notes of every music sheet, from the game paks.
    MusicCharts,
    /// Writes the camp slice file: the spawn group components, asset id and respawn point of
    /// every camp slice, from the game paks.
    CampSlices,
    /// Writes the catacombs tile file: the gameplay and art slice, the slots and the encounters
    /// of every tile variation of the catacombs generation rules, and the power-up slices, from
    /// the game paks (reads the mob prefab table), and the catacombs level's collision file.
    CatacombTiles,
    /// Writes the claim plot file: the spawn group components of every territory's land claim
    /// slice, from the game paks.
    ClaimPlots,
    /// Writes the ability instance file: the instance type of every slice with an ability
    /// instance component and the player's instance limits, from the game paks.
    AbilityInstances,
    /// Writes the quick course path file: the nodes of every mount race, rally, tour and time
    /// trial, from the game paks.
    QuickCourses,
    /// Writes the store table (`services/store.json`): the store mounts and the entitlements
    /// every account holds, from the merged content bundle, for `edge-http`'s store. Needs no
    /// game install.
    Store,
    /// Writes the leaderboard table (`services/leaderboards.json`): every board of a world with
    /// its English name and group, from the merged content bundle, for `status-reporter`. Needs
    /// no game install.
    Leaderboards,
    /// Makes or installs the realm data pack: the capture-made tables, scrubbed of player data
    /// (README, "Content data").
    RealmPack {
        /// What to do.
        #[command(subcommand)]
        action: RealmPackAction,
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

impl Command {
    /// False for the commands that read no content file (the map tiles go beside the build
    /// folders).
    const fn reads_content(&self) -> bool {
        !matches!(
            self,
            Self::Codegen { .. }
                | Self::CaptureCheck { .. }
                | Self::MapTiles { .. }
                | Self::MobPrefabsLedger { .. }
        )
    }
}

#[derive(Subcommand)]
enum ContentAction {
    /// Builds every content file in registry order. A file that exists, is in the manifest and
    /// is newer than its inputs is kept. The capture-made files come from the realm data pack
    /// only (`just realm-data`).
    Build {
        /// Builds every file again.
        #[arg(long)]
        rebuild: bool,
        /// Builds only the tables made from the capture-made ones (`just realm-data`).
        #[arg(long)]
        derived: bool,
        /// The design files.
        #[arg(long, default_value = DEFAULT_DESIGN_DIR)]
        design: PathBuf,
        /// The Server.pak file.
        #[arg(long, default_value = DEFAULT_SERVER_PAK)]
        server_pak: PathBuf,
    },
    /// Prints every content file with its topic, source, state (ok, old, missing) and when it
    /// was made.
    Status {
        /// The design files.
        #[arg(long, default_value = DEFAULT_DESIGN_DIR)]
        design: PathBuf,
        /// The Server.pak file.
        #[arg(long, default_value = DEFAULT_SERVER_PAK)]
        server_pak: PathBuf,
    },
    /// Moves an old flat content folder (`<table>-<build>.json`, `retail/`) into the layout:
    /// renames only, a merged file equal to its retail file becomes a hard link to it.
    Migrate {
        /// Prints the moves only.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum RealmPackAction {
    /// Scrubs and scans a copy of the pack tables of the content folder and writes
    /// `realm-data-<build>.zip`, `SHA256SUMS` and `scan-report.txt` (read the report before
    /// publishing). The content folder is only read.
    Make {
        /// Where the pack files go.
        #[arg(long, default_value = DEFAULT_PACK_DIR)]
        out_dir: PathBuf,
        /// The pack version (one up per published pack).
        #[arg(long)]
        pack_version: u32,
        /// The server commit whose loaders read these tables (`git rev-parse HEAD`).
        #[arg(long)]
        source_commit: String,
    },
    /// Checks a pack zip in full and writes its tables into the content folder.
    Install {
        /// The pack zip.
        zip: PathBuf,
        /// Replace tables of the folder that differ from the pack's.
        #[arg(long)]
        replace: bool,
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

fn print_roads(report: &RoadsReport) {
    println!(
        "roads {}: {} tract maps, {} regions with roads, {} road cells -> {}",
        report.level,
        report.tractmaps,
        report.road_regions,
        report.road_cells,
        report.path.display()
    );
}

fn print_collision(report: &CollisionReport, took: std::time::Duration) {
    println!(
        "collision {}: {} placements ({} variant capitals, {} of an unknown variant, {} from region slice data of which {} without collision), {} slices, {} solid entities as the level loads (no shape named {}, query shape kind not read {}, inactive {}, mesh without physics geometry {}, mesh with physics geometry but no .rnr {}, asset missing {}), {} physics assets ({} shapes of kinds not read), {} shapes, {} bodies, {} deferred slices with {} bodies, {} quest volumes, {} placed triangles, {} file bytes, {} index bytes, {:.1} s -> {}",
        report.level,
        report.placements,
        report.variant_placements,
        report.unknown_variants,
        report.slice_data_variants,
        report.slice_data_without_collision,
        report.slices,
        report.solid_entities,
        report.shapeless_entities,
        report.unread_entities,
        report.inactive_entities,
        report.meshes_without_physics,
        report.meshes_with_physics_without_rnr,
        report.missing_assets,
        report.assets,
        report.unsupported_shapes,
        report.geometries,
        report.bodies,
        report.deferred_slices,
        report.deferred_bodies,
        report.volumes,
        report.placed_triangles,
        report.file_bytes,
        report.index_bytes,
        took.as_secs_f64(),
        report.path.display()
    );
    println!(
        "collision {}: {} distribution rows ({} not placed), {} engine triggers, {} mob-only bodies, {} player-only bodies, {} navigation marks ({} marks not read)",
        report.level,
        report.distribution_rows,
        report.distribution_unplaced,
        report.triggers,
        report.ai_bodies,
        report.player_only,
        report.nav_marks,
        report.unread_marks
    );
}

fn print_design(report: &DesignReport, layout: &ContentLayout) {
    let target = if report.written {
        layout.build_dir().display().to_string()
    } else {
        "nothing written".to_owned()
    };
    println!(
        "design: {} files, {} levels, {} errors -> {target}",
        report.files,
        report.levels,
        report.errors.len()
    );
    for error in &report.errors {
        println!("  {error}");
    }
}

/// The install folder, for the commands that read it.
fn game_dir(game_dir: Option<&Path>) -> Result<&Path, ToolError> {
    game_dir.ok_or(ToolError::MissingGameDir)
}

/// What the content steps read besides the content folder.
#[derive(Clone, Copy)]
struct StepContext<'a> {
    /// The content folder.
    layout: &'a ContentLayout,
    /// The install folder, when given.
    game_dir: Option<&'a Path>,
    /// The design files.
    design_dir: &'a Path,
    /// The Server.pak file.
    server_pak: &'a Path,
}

/// The registry entry of `step`.
fn spec_of(step: Step) -> &'static StepSpec {
    STEPS
        .iter()
        .find(|spec| spec.step == step)
        .expect("every step is in the registry")
}

/// The levels `content build` runs `spec` at: its named levels, the levels `effect-areas`
/// reads, else none (the world steps take every level of the install).
fn build_levels(spec: &StepSpec) -> Vec<String> {
    let levels: &[&str] = match (spec.step, spec.levels) {
        (_, StepLevels::Named(levels)) => levels,
        (Step::EffectAreas, _) => &DEFAULT_EFFECT_AREA_LEVELS,
        _ => &[],
    };
    levels.iter().map(|&level| level.to_owned()).collect()
}

/// Runs `step` at `levels` and records the files it wrote in the manifest, with the other files
/// they were made from.
fn run_recorded(context: &StepContext<'_>, step: Step, levels: &[String]) -> Result<(), ToolError> {
    let written = run_step(context, step, levels)?;
    let others = made_from(spec_of(step), context.design_dir, context.server_pak);
    if others.is_empty() {
        record(context.layout, &written, MADE_BY)?;
    } else {
        record_made_from(context.layout, &written, MADE_BY, &others)?;
    }
    Ok(())
}

/// Runs one step of the registry at `levels` (the levels of its level files, or the levels
/// `effect-areas` reads); prints its report and returns the files it wrote.
fn run_step(
    context: &StepContext<'_>,
    step: Step,
    levels: &[String],
) -> Result<Vec<FileRef>, ToolError> {
    let layout = context.layout;
    let paks = || {
        PakIndex::open(game_dir(context.game_dir)?)
            .map_err(CollisionError::from)
            .map_err(ToolError::from)
    };
    let source = || {
        game_dir(context.game_dir).and_then(|dir| GameSource::open(dir).map_err(ToolError::from))
    };
    let at_levels = |file: LevelFile| -> Vec<FileRef> {
        levels
            .iter()
            .map(|level| FileRef::level(file, level))
            .collect()
    };
    Ok(match step {
        Step::RetailBundle => {
            let summary = run_build(&source()?, layout)?;
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
            vec![FileRef::build(RETAIL_BUNDLE)]
        }
        Step::RetailWorld => {
            let reports = run_world(&source()?, layout)?;
            print_world(&reports);
            reports
                .iter()
                .map(|report| FileRef::level(RETAIL_WORLD, &report.level))
                .collect()
        }
        Step::Design => {
            run_design_step(layout, context.design_dir, DesignMode::Write)?;
            // `run_design` records the merged files itself.
            Vec::new()
        }
        Step::Water => {
            let index = paks()?;
            for level in levels {
                let report = build_level_water(&index, level, layout)?;
                println!(
                    "water {}: {} regions, {} nodes -> {}",
                    report.level,
                    report.regions,
                    report.nodes,
                    report.path.display()
                );
            }
            at_levels(WATER)
        }
        Step::Collision => {
            let index = paks()?;
            for level in levels {
                let started = std::time::Instant::now();
                let report = build_level_collision(&index, level, layout)?;
                print_collision(&report, started.elapsed());
            }
            at_levels(COLLISION)
        }
        Step::Roads => {
            let index = paks()?;
            for level in levels {
                print_roads(&build_level_roads(&index, level, layout)?);
            }
            at_levels(ROADS)
        }
        Step::LevelDoors => {
            let index = paks()?;
            for level in levels {
                let world = WorldFile::load(layout, level)?;
                print_level_doors(&build_level_doors(&index, &world, level, layout)?);
            }
            at_levels(LEVEL_DOORS)
        }
        Step::PlayerAttacks => {
            let out = layout.file(PLAYER_ATTACKS);
            let rows = BundleRows::of(&ContentBundle::load(layout)?.tables);
            let (stances, report) = build_stances(&read_sources(&source()?, &rows)?, &rows);
            write_stances(&out, &stances)?;
            print_player_attacks(&report, &out);
            vec![FileRef::build(PLAYER_ATTACKS)]
        }
        Step::PlayerAbilities => {
            let out = layout.file(PLAYER_ABILITIES);
            let tables = ContentBundle::load(layout)?.tables;
            let rows = BundleRows::of(&tables);
            let (table, report) =
                build_abilities(&read_sources(&source()?, &rows)?, &rows, &tables);
            table.write(&out)?;
            print_player_abilities(&report, &out);
            vec![FileRef::build(PLAYER_ABILITIES)]
        }
        Step::DungeonGates => {
            let tables = ContentBundle::load(layout)?.tables;
            print_dungeon_gates(&build_dungeon_gates(&paks()?, &tables, layout)?);
            vec![FileRef::build(DUNGEON_GATES)]
        }
        Step::AbilityInstances => {
            let report = build_ability_instances(&source()?, layout)?;
            println!(
                "{}: {} typed slices of {} slices, {} limits",
                report.path.display(),
                report.typed,
                report.slices,
                report.limits
            );
            vec![FileRef::build(ABILITY_INSTANCES)]
        }
        Step::ClaimPlots => {
            let report = build_claim_plots(&source()?, layout)?;
            println!(
                "{}: {} claim plots of {} claim plot folder slices",
                report.path.display(),
                report.plots,
                report.slices
            );
            vec![FileRef::build(CLAIM_PLOTS)]
        }
        Step::MusicCharts => {
            let report = build_music_charts(&source()?, layout)?;
            println!(
                "{}: {} charts, {} notes",
                report.path.display(),
                report.charts,
                report.notes
            );
            vec![FileRef::build(MUSIC_CHARTS)]
        }
        Step::CampSlices => {
            let report = build_camp_slices(&source()?, layout)?;
            println!(
                "{}: {} camps ({} PvP) of {} camp folder slices",
                report.path.display(),
                report.camps,
                report.pvp_camps,
                report.slices
            );
            vec![FileRef::build(CAMP_SLICES)]
        }
        Step::QuickCourses => {
            let report = build_quick_courses(&source()?, layout)?;
            println!(
                "{}: {} courses of {} course folder slices",
                report.path.display(),
                report.courses,
                report.slices
            );
            vec![FileRef::build(QUICK_COURSES)]
        }
        Step::Store => {
            let report = write_store_table(layout)?;
            println!(
                "{}: {} store mounts, {} token packs, {} entitlements every account holds",
                report.path.display(),
                report.mounts,
                report.tokens,
                report.held_by_all
            );
            vec![FileRef::build(STORE)]
        }
        Step::Leaderboards => {
            let report = write_leaderboard_table(layout)?;
            println!("{}: {} leaderboards", report.path.display(), report.boards);
            vec![FileRef::build(LEADERBOARDS)]
        }
        Step::CreatureAnimations => {
            let out = layout.file(CREATURE_ANIMATIONS);
            let tables = ContentBundle::load(layout)?.tables;
            let prefabs = MobPrefabs::load(layout)
                .map_err(content_tool::creature_animations::CreatureAnimationsError::from)?;
            let dir = game_dir(context.game_dir)?;
            let (rows, report) = build_rows(&GameSource::open(dir)?, dir, &prefabs, &tables)?;
            write_rows(&out, &rows)
                .map_err(content_tool::creature_animations::CreatureAnimationsError::from)?;
            print_creature_animations(&report, &out);
            vec![FileRef::build(CREATURE_ANIMATIONS)]
        }
        Step::CatacombTiles => {
            let tables = ContentBundle::load(layout)?.tables;
            let report = build_catacomb_tiles(&paks()?, &tables, layout)?;
            println!(
                "{}: {} tile variations, {} art tiles, {} plain slots, {} encounters, {} power-ups",
                report.path.display(),
                report.variations,
                report.art_tiles,
                report.props,
                report.encounters,
                report.power_ups
            );
            println!(
                "{}: {} art tiles with solids, {} shapes",
                report.collision_path.display(),
                report.solid_art_tiles,
                report.solid_bodies
            );
            vec![
                FileRef::build(CATACOMB_TILES),
                FileRef::level(COLLISION, CATACOMB_LEVEL),
            ]
        }
        Step::DungeonScripts => {
            let tables = ContentBundle::load(layout)?.tables;
            print_dungeon_scripts(&build_dungeon_scripts(&paks()?, &tables, layout)?);
            vec![FileRef::build(DUNGEON_SCRIPTS)]
        }
        Step::EffectAreas => {
            let tables = ContentBundle::load(layout)?.tables;
            print_effect_areas(&build_effect_areas(&paks()?, &tables, levels, layout)?);
            vec![FileRef::build(EFFECT_AREAS)]
        }
        Step::AiData => {
            let creatures = read_creatures(&layout.file(CREATURE_ANIMATIONS))?;
            let dir = game_dir(context.game_dir)?;
            let (data, report) =
                build_ai_data(&GameSource::open(dir)?, dir, context.server_pak, &creatures)?;
            let path = data.save(layout)?;
            let reloaded = nw_ai::AiData::load(layout)?;
            if reloaded != data {
                return Err(ToolError::CountMismatch {
                    what: "AI data nodes",
                    expected: data.nodes.len(),
                    found: reloaded.nodes.len(),
                });
            }
            print_ai_data(&report, &path);
            vec![FileRef::build(AI_DATA)]
        }
    })
}

/// Runs the design step on `layout` in `mode`; an error when a design file has a problem.
fn run_design_step(
    layout: &ContentLayout,
    design_dir: &Path,
    mode: DesignMode,
) -> Result<(), ToolError> {
    let paths = DesignPaths {
        design_dir: design_dir.to_path_buf(),
        layout: layout.clone(),
    };
    let report = run_design(&paths, mode, MADE_BY)?;
    print_design(&report, layout);
    if report.errors.is_empty() {
        Ok(())
    } else {
        Err(ToolError::DesignErrors {
            count: report.errors.len(),
        })
    }
}

fn run(cli: Cli) -> Result<(), ToolError> {
    let client_build = load_client_build(&cli.client_build_config)?;
    // A command that reads no content file does not read the manifest either, so a bad one
    // stops only the commands that need it.
    let layout = if cli.command.reads_content() {
        ContentLayout::open(&cli.content_dir, &client_build)?
    } else {
        ContentLayout::new(&cli.content_dir, &client_build)
    };
    let design_dir = Path::new(DEFAULT_DESIGN_DIR);
    let server_pak = Path::new(DEFAULT_SERVER_PAK);
    let context = StepContext {
        layout: &layout,
        game_dir: cli.game_dir.as_deref(),
        design_dir,
        server_pak,
    };
    let at_levels = |step: Step, levels: &[String]| run_recorded(&context, step, levels);
    let with_build_levels = |step: Step| at_levels(step, &build_levels(spec_of(step)));
    match cli.command {
        Command::Codegen { out_dir } => {
            let source = GameSource::open(game_dir(cli.game_dir.as_deref())?)?;
            let summary = run_codegen(&source, &client_build, &out_dir)?;
            println!(
                "codegen: {} datasheets, {} table types, {} files written, {} stale files removed",
                summary.datasheet_files,
                summary.table_types,
                summary.files_written,
                summary.stale_files_removed
            );
        }
        Command::Content { action } => run_content(action, &context)?,
        Command::Build => with_build_levels(Step::RetailBundle)?,
        Command::World => with_build_levels(Step::RetailWorld)?,
        Command::Collision { levels } => at_levels(Step::Collision, &levels)?,
        Command::Roads { levels } => at_levels(Step::Roads, &levels)?,
        Command::Water { levels } => at_levels(Step::Water, &levels)?,
        Command::LevelDoors { levels } => at_levels(Step::LevelDoors, &levels)?,
        Command::EffectAreas { levels } => at_levels(Step::EffectAreas, &levels)?,
        Command::Design { design, check } => {
            let mode = if check {
                DesignMode::Check
            } else {
                DesignMode::Write
            };
            run_design_step(&layout, &design, mode)?;
        }
        Command::CreatureAnimations => with_build_levels(Step::CreatureAnimations)?,
        Command::AiData { server_pak } => run_recorded(
            &StepContext {
                server_pak: &server_pak,
                ..context
            },
            Step::AiData,
            &[],
        )?,
        Command::PlayerAttacks => with_build_levels(Step::PlayerAttacks)?,
        Command::PlayerAbilities => with_build_levels(Step::PlayerAbilities)?,
        Command::DungeonScripts => with_build_levels(Step::DungeonScripts)?,
        Command::DungeonGates => with_build_levels(Step::DungeonGates)?,
        Command::MusicCharts => with_build_levels(Step::MusicCharts)?,
        Command::CatacombTiles => with_build_levels(Step::CatacombTiles)?,
        Command::CampSlices => with_build_levels(Step::CampSlices)?,
        Command::ClaimPlots => with_build_levels(Step::ClaimPlots)?,
        Command::AbilityInstances => with_build_levels(Step::AbilityInstances)?,
        Command::QuickCourses => with_build_levels(Step::QuickCourses)?,
        Command::Store => with_build_levels(Step::Store)?,
        Command::Leaderboards => with_build_levels(Step::Leaderboards)?,
        Command::CaptureCheck { ledgers, names } => {
            let names = match names {
                Some(names) => TypeNames::load(
                    &game_dir(cli.game_dir.as_deref())?.join(TYPE_INDEX_FILE),
                    &names,
                )?,
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
            y_index,
            formats_only,
        } => {
            let source = GameSource::open(game_dir(cli.game_dir.as_deref())?)?;
            print_map_tiles(&run_map_tiles(
                &source,
                &level,
                &layout.map_dir(),
                y_index,
                formats_only,
            )?);
        }
        Command::CombatCheck { level, nwdb_dir } => {
            let tables = ContentBundle::load(&layout)?.tables;
            let data = CombatData::from_tables(&tables, &CombatRows::default())?;
            drop(tables);
            let world = WorldFile::load(&layout, &level)?;
            let nwdb = read_nwdb(&nwdb_dir)?;
            print_combat(
                &check(&data, &world.vitals, &nwdb.creatures),
                nwdb.unreadable,
            );
        }
        Command::MobPrefabs { roots, jobs } => {
            let global_args = [
                OsString::from("--game-dir"),
                game_dir(cli.game_dir.as_deref())?.as_os_str().to_owned(),
                OsString::from("--client-build-config"),
                cli.client_build_config.into_os_string(),
            ];
            run_mob_prefabs(&roots, &layout, jobs, &global_args)?;
            record(&layout, &[FileRef::build(MOB_PREFABS)], MADE_BY)?;
        }
        Command::FtueEntities { ledger } => {
            let out = layout.file(FTUE_ENTITIES);
            let scan = content_tool::ftue_entities::scan_ledger(&read_ledger(&ledger)?)?;
            write_table(&out, &scan.table)?;
            record(&layout, &[FileRef::build(FTUE_ENTITIES)], MADE_BY)?;
            print_ftue_entities(&scan, &out);
        }
        Command::AiTrace { ledger, out } => {
            let creatures = read_creatures(&layout.file(CREATURE_ANIMATIONS))?;
            let trace = trace_ledger(&read_ledger(&ledger)?, &creatures);
            write_trace(&out, &trace)?;
            print_ai_trace(&trace, &out);
        }
        Command::CatacombCheck { game_mode, runs } => {
            let tables = ContentBundle::load(&layout)?.tables;
            let index =
                PakIndex::open(game_dir(cli.game_dir.as_deref())?).map_err(CollisionError::from)?;
            let report = catacomb_check(&tables, &game_mode, runs, |wanted| {
                index
                    .paths(|path| wanted.contains(path))
                    .into_iter()
                    .collect()
            })?;
            print_catacombs(&report);
        }
        Command::BreachTiers { ledgers } => {
            let tiers = BreachTiers {
                tiers: build_breach_tiers(&ledgers)?,
            };
            let path = tiers.write(&layout)?;
            record(&layout, &[FileRef::build(BREACH_TIERS)], MADE_BY)?;
            println!("{}: {} breach places", path.display(), tiers.tiers.len());
        }
        Command::RealmPack { action } => run_realm_pack(action, &layout)?,
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

/// `content build`, `content status` and `content migrate`.
fn run_content(action: ContentAction, context: &StepContext<'_>) -> Result<(), ToolError> {
    let layout = context.layout;
    let options = |rebuild, derived_only, design: PathBuf, server_pak: PathBuf| BuildOptions {
        rebuild,
        derived_only,
        has_game_dir: context.game_dir.is_some(),
        design_dir: design,
        server_pak,
    };
    match action {
        ContentAction::Build {
            rebuild,
            derived,
            design,
            server_pak,
        } => {
            let options = options(rebuild, derived, design, server_pak);
            let context = StepContext {
                design_dir: &options.design_dir,
                server_pak: &options.server_pak,
                ..*context
            };
            build_content(
                layout,
                &options,
                |spec| run_recorded(&context, spec.step, &build_levels(spec)),
                |line| print_build_line(line, layout, &options.server_pak),
            )?;
        }
        ContentAction::Status { design, server_pak } => {
            let rows = content_status(layout, &options(false, false, design, server_pak))?;
            let key_width = rows.iter().map(|row| row.key.len()).max().unwrap_or(0);
            println!("{}:", layout.build_dir().display());
            for row in rows {
                println!(
                    "  {:key_width$}  {:12}  {:8}  {:8}  {}",
                    row.key,
                    row.topic,
                    row.source.label(),
                    row.state.label(),
                    row.made_at.as_deref().unwrap_or(NO_TIME)
                );
            }
        }
        ContentAction::Migrate { dry_run } => {
            let plan = plan_migration(layout)?;
            print_migration(&plan);
            if dry_run {
                println!("dry run: nothing was moved");
            } else {
                let retail_removed = migrate(layout, &plan)?;
                println!(
                    "{} files moved into {}{}",
                    plan.moves.len(),
                    layout.build_dir().display(),
                    if retail_removed {
                        "; the empty old retail folder is removed"
                    } else {
                        ""
                    }
                );
            }
        }
    }
    Ok(())
}

/// Prints what `content build` did with a step or found of a pack file.
fn print_build_line(line: &BuildLine, layout: &ContentLayout, server_pak: &Path) {
    let what = line.what;
    match &line.outcome {
        BuildOutcome::Built => println!("{what}: built"),
        BuildOutcome::Fresh => println!(
            "{what}: kept, its files are newer than their inputs (--rebuild builds it again)"
        ),
        BuildOutcome::NoGameDir => {
            println!("{what}: kept, it reads the game install and NW_GAME_DIR is not set")
        }
        BuildOutcome::NoServerPak => println!(
            "{what}: skipped: Server.pak not found: get it with a depot download of build {} into {}, then build again",
            layout.client_build(),
            server_pak.display()
        ),
        BuildOutcome::NoPack(files) => println!(
            "{what}: skipped, it reads the capture-made {}: install the realm data pack (just realm-data <pack.zip>)",
            files.join(", ")
        ),
        BuildOutcome::PackMissing => println!(
            "{what}: missing; it is capture-made: install the realm data pack (just realm-data <pack.zip>)"
        ),
    }
}

/// Prints every move of `plan` and the entries it leaves in place.
fn print_migration(plan: &MigratePlan) {
    for done in &plan.moves {
        match &done.kind {
            MoveKind::Rename => {
                println!("{} -> {}", done.from.display(), done.to.display());
            }
            MoveKind::RemoveDuplicate => println!(
                "{} removed: {} holds the same bytes",
                done.from.display(),
                done.to.display()
            ),
            MoveKind::LinkToRetail(retail) => println!(
                "{} -> {} (equal to its retail file: a hard link to {}; the old file is removed)",
                done.from.display(),
                done.to.display(),
                retail.display()
            ),
        }
    }
    for path in &plan.left {
        println!("left in place (no content file): {}", path.display());
    }
    for file in &plan.unrecorded {
        println!(
            "recorded in the manifest (already in place): {}",
            file.key()
        );
    }
}

fn print_effect_areas(report: &EffectAreasReport) {
    println!(
        "{}: {} slices read, level areas {:?}, stage areas {:?}, {} spells with areas, {} areas whose volume does not read",
        report.path.display(),
        report.slices,
        report.level_areas,
        report.stage_areas,
        report.spells,
        report.unread_volumes
    );
}

fn print_level_doors(report: &LevelDoorsReport) {
    println!(
        "{}: {} capitals ({} slices not in the catalog), {} slices read, {} door slices, {} door capitals",
        report.path.display(),
        report.capitals,
        report.unresolved,
        report.slices,
        report.door_slices,
        report.doors
    );
}

fn print_dungeon_gates(report: &DungeonGatesReport) {
    println!(
        "{}: {} levels, {} gates ({} blockers), {} timelines ({} without their asset), {} encounter slices with {} replicated entities",
        report.path.display(),
        report.levels,
        report.gates,
        report.blockers,
        report.timelines,
        report.unread_timelines,
        report.encounter_slices,
        report.encounter_entities
    );
    if !report.skipped_modes.is_empty() {
        println!(
            "game modes without a script: {}",
            report.skipped_modes.join(", ")
        );
    }
    for path in report.missing_slices.iter().take(SHOWN_FAILURES) {
        println!("not in the paks: {path}");
    }
}

fn print_dungeon_scripts(report: &DungeonScriptsReport) {
    println!(
        "{}: {} levels, {} encounters, {} stages, {} spawns, {} chests",
        report.path.display(),
        report.levels,
        report.encounters,
        report.stages,
        report.spawns,
        report.chests
    );
    println!(
        "open world spawn groups: {} encounters; {} of alias spawners; {} encounters with spawner rules; {} detection volumes placed, {} of a shape not read",
        report.open_world_encounters,
        report.grouped_encounters,
        report.encounters_with_spawner_rules,
        report.detectors,
        report.unread_detectors
    );
    println!(
        "target slices: {} spawner mobs ({} without a level), {} other slices",
        report.spawner_mobs, report.levelless_mobs, report.other_slices
    );
    if !report.skipped_modes.is_empty() {
        println!(
            "game modes without a script: {}",
            report.skipped_modes.join(", ")
        );
    }
    for path in report.missing_slices.iter().take(SHOWN_FAILURES) {
        println!("not in the paks: {path}");
    }
}

fn print_catacombs(report: &CatacombCheck) {
    println!(
        "{}: {} runs from cell {:?}: {} to {} tiles, {} to {} connectors",
        report.game_mode,
        report.runs,
        report.start_cell,
        report.tiles.0,
        report.tiles.1,
        report.connectors.0,
        report.connectors.1
    );
    println!(
        "run 0: cluster {}, theme {}, 1635 frame {:?}",
        report.first.cluster,
        report.first.theme.as_deref().unwrap_or("-"),
        report.first.frame()
    );
    for row in catacomb_map_rows(&report.first) {
        println!("  {row}");
    }
    if report.missing_slices.is_empty() {
        println!("every tile variation slice is in the paks");
    }
    for slice in &report.missing_slices {
        println!("missing tile slice: {slice}");
    }
}

/// Missing values printed per kind by `ai-data`.
const SHOWN_AI_GAPS: usize = 40;

fn print_ai_data(report: &AiDataReport, out: &Path) {
    println!(
        "ai-data: {} trees, {} shared nodes ({} expanded), {} creatures, {} archetypes, {} grids, {} CAGE actions -> {}",
        report.trees,
        report.nodes,
        report.expanded_nodes,
        report.creatures,
        report.archetypes,
        report.grids,
        report.cage_actions,
        out.display()
    );
    println!(
        "archetypes with a Server.pak tree {}, a grid {}, an animation database {}; fragment entries {}, with a length {}",
        report.with_tree,
        report.with_grid,
        report.with_animation_database,
        report.variants,
        report.timed_variants
    );
    println!(
        "trees named by slices and not in Server.pak: {}",
        report.missing_trees.len()
    );
    for (tree, slices) in report.missing_trees.iter().take(SHOWN_AI_GAPS) {
        println!("  {tree}: {} slices", slices.len());
    }
    println!(
        "slices whose tree runs a task their grid lacks: {}",
        report.missing_tasks.len()
    );
    for (slice, tasks) in report.missing_tasks.iter().take(SHOWN_AI_GAPS) {
        println!("  {slice}: {tasks:?}");
    }
    println!("clips of Unknown length: {}", report.unknown_clips.len());
    for (clip, reason) in report.unknown_clips.iter().take(SHOWN_AI_GAPS) {
        println!("  {clip}: {reason}");
    }
    println!(
        "fragment entries of Unknown length: {} in {} animation databases",
        report.unknown_fragments.values().sum::<usize>(),
        report.unknown_fragments.len()
    );
    println!(
        "ranged attacks: {}, projectile slices of the mob ranged clips: {}",
        report.ranged_attacks, report.projectile_slices
    );
    println!("files not used: {}", report.skipped.len());
    for (file, reason) in report.skipped.iter().take(SHOWN_AI_GAPS) {
        println!("  {file}: {reason}");
    }
}

fn print_ai_trace(trace: &AiTrace, out: &Path) {
    let stats = &trace.stats;
    println!(
        "ai-trace: {} mob lifetimes, {} mob states, {} player positions, {} player hits ({} by an unknown attacker) -> {}",
        trace.mobs.len(),
        stats.mob_states,
        trace.player.len(),
        trace.player_hits.len(),
        stats.unknown_attackers,
        out.display()
    );
    println!(
        "  {} bundles, {} groups, {} resyncs, {} other-component states, {} bad scope blobs",
        stats.bundles,
        stats.groups,
        stats.resyncs,
        stats.other_component_states,
        stats.bad_scope_blobs
    );
}

fn print_player_attacks(report: &PlayerAttacksReport, out: &Path) {
    println!(
        "player-attacks: {} stances -> {}",
        report.stances,
        out.display()
    );
    for (stance, why) in &report.skipped {
        println!("  skipped {stance}: {why}");
    }
}

fn print_player_abilities(report: &PlayerAbilitiesReport, out: &Path) {
    println!(
        "player-abilities: {} lists, {} actions, {} input actions -> {}",
        report.lists,
        report.actions,
        report.inputs,
        out.display()
    );
    for (part, why) in &report.skipped {
        println!("  skipped {part}: {why}");
    }
}

fn print_creature_animations(report: &CreatureAnimationsReport, out: &Path) {
    println!(
        "creature-animations: {} of {} creature assets -> {}",
        report.creatures,
        report.assets,
        out.display()
    );
    for (asset, why) in &report.skipped {
        println!("  skipped {asset}: {why}");
    }
}

fn run_mob_prefabs(
    roots: &[PathBuf],
    layout: &ContentLayout,
    jobs: Option<NonZeroUsize>,
    global_args: &[OsString],
) -> Result<(), ToolError> {
    let out = layout.file(MOB_PREFABS);
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
    let names = vitals_names(&ContentBundle::load(layout)?.tables);
    let entries = table.entries(&names);
    write_entries(&out, &entries)?;
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
        "ftue-entities: {} bundles, {} static and {} asset entities, {} mobs ({} mob cards), {} spawns ({} sent again), {} changes ({} sent again), {} static and {} asset cards in unread groups, {} members without a reader left out -> {}",
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
        stats.skipped_members.len(),
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
    for (time_ms, class) in stats.skipped_members.iter().take(SHOWN_SKIPPED_MEMBERS) {
        println!("  member of class {class} without a reader left out at {time_ms} ms");
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

fn run_realm_pack(action: RealmPackAction, layout: &ContentLayout) -> Result<(), ToolError> {
    match action {
        RealmPackAction::Make {
            out_dir,
            pack_version,
            source_commit,
        } => {
            let report = make_pack(layout, &out_dir, pack_version, &source_commit)?;
            println!(
                "{}: {} owner, turret and roster states looked at and set to no one, {} player ids zeroed, {} words in the review report (read it before publishing)",
                report.zip.display(),
                report.identity_states,
                report.player_ids,
                report.review_words
            );
        }
        RealmPackAction::Install { zip, replace } => {
            let report = install_pack(&zip, layout, replace, MADE_BY)?;
            for (table, outcome) in &report.tables {
                println!("{table}: {outcome:?}");
            }
            if report
                .tables
                .iter()
                .any(|(_, outcome)| *outcome == TableOutcome::Kept)
            {
                println!(
                    "kept tables differ from the pack's: run again with --replace to take the pack's"
                );
            }
            println!(
                "{} replicated members of a class with no reader were not checked",
                report.unknown_class_members
            );
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
        "{label}: {} regions ({} with terrain), {} npcs, {} vitals, {} gatherables, {} lore, {} landmarks, {} encounters, {} capitals, {} spawned slices, {} stations, {file_bytes} bytes",
        counts.regions,
        counts.terrain_regions,
        counts.npcs,
        counts.vitals,
        counts.gatherables,
        counts.lore,
        counts.landmarks,
        counts.encounters,
        counts.capitals,
        counts.spawned,
        counts.stations,
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
