// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient`: logs in over HTTPS like the game, then registers with `rep-gateway`.
//! `registration` (the default) stays connected, then checks that the same ticket is refused a
//! second time. `pair` runs two clients in the world (M4 spec section 9). `swarm` runs many bots
//! across a sim border (M5 spec section 11). `hunt` runs many bots that fight mobs (M6-B spec
//! section 9). `chat` says one line in area chat and waits for the server's answer (system lines
//! for a GM chat command such as `!help` of a game master account, else the line itself). `voice`
//! sets up the voice chat and checks the retail answers (it needs a `[voice]` service in the
//! social service's configuration). `play`
//! plays quests with a game master's bot and writes a report. `expedition` runs game master bots
//! that form a group, enter an expedition and walk its start area. Prints one line per step;
//! exits with 0 only when every step passed.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::sync::watch;

use clap::{Parser, Subcommand};
use nw_content::bundle::ContentBundle;
use nw_content::client_build::load_client_build;
use nw_content::gathering::{GatherRules, Gatherables};
use nw_content::instances::InstanceCatalog;
use nw_content::layout::{ContentLayout, content_dir};
use nw_content::poi_tags::PoiTags;
use nw_content::quick_courses::QuickCourseRules;
use nw_content::town_stations::TownStations;
use nw_dtls::DtlsClientConfig;
use nw_messages::WorldPosition;
use nw_reflect::REGISTRATION_STATUS_AUTHORIZED;
use nw_testclient::activity::{
    Activity, ActivityData, ActivityKind, CourseRoutes, mount_reward_box,
};
use nw_testclient::chat::{ChatSettings, run_chat};
use nw_testclient::config::{
    PASSWORD_ENV, SECOND_PASSWORD_ENV, SWARM_PASSWORD_ENV, TestClientConfig, load_testclient_config,
};
use nw_testclient::fleet::{NewClient, udp_receive_errors};
use nw_testclient::gather::{GatherNode, run_gather};
use nw_testclient::group::{
    GroupBoard, GroupSeat, LEADER, expedition_steps, floor_lines, quest_group_steps,
};
use nw_testclient::hunt::run_hunt;
use nw_testclient::pair::{Check, LATE_STATE_GAP, PairReport, PairSettings, RecordWalk, run_pair};
use nw_testclient::play::{PlayData, PlayReport, PlaySettings, Verdict, run_play};
use nw_testclient::record_walk::InputDevice;
use nw_testclient::rep::{LedgerSlot, RepClient};
use nw_testclient::swarm::run_swarm;
use nw_testclient::udp::{Hold, run_rep};
use nw_testclient::voice::{VoiceSettings, run_voice};
use nw_testclient::walk::{CHARACTER_CREATION_PARAMS, WalkSettings, login_token, walk};
use nw_world::WorldFile;
use nw_world::collision::LevelCollision;
use nw_world::collision_index::CollisionIndex;
use nw_world::quick_courses::QuickCoursePaths;

/// The committed example configuration.
const DEFAULT_CONFIG: &str = "config/nw-testclient.toml";
/// The line of `chat` without `--line`: the GM command list.
const DEFAULT_CHAT_LINE: &str = "!help";
/// The longest play without `--minutes`.
const DEFAULT_PLAY_MINUTES: u64 = 30;
/// The expedition of `expedition` without `--game-mode`: Amrine Excavation.
const DEFAULT_EXPEDITION: &str = "DungeonAmrine";
/// The longest expedition run without `--minutes`.
const DEFAULT_EXPEDITION_MINUTES: u64 = 10;
/// Seconds per minute.
const SECONDS_PER_MINUTE: u64 = 60;
/// The end of a play bot's character name after its account name.
const PLAY_CHARACTER_MARK: &str = "Char";
/// Bytes read from standard input at a time while waiting for its end.
const INPUT_BUFFER_BYTES: usize = 256;
/// The kernel's network counters (Linux).
const SNMP_PATH: &str = "/proc/net/snmp";

#[derive(Parser)]
#[command(
    name = "nw-testclient",
    about = "Test client of the Blighted New World emulator."
)]
struct Cli {
    /// Configuration file. The password comes from `NW_SECRET_TESTCLIENT_PASSWORD`.
    #[arg(long, default_value = DEFAULT_CONFIG)]
    config: PathBuf,
    /// Writes the plain carrier datagrams of the first client to this capture ledger, which
    /// `blighted-admin` opens like a recorded session.
    #[arg(long, global = true)]
    ledger: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// One client: registration, a hold with pings, and the refused second registration
    /// (the default).
    Registration,
    /// Two clients in the world: each sees the other, and the first logs in again at its saved
    /// position. The second password comes from `NW_SECRET_TESTCLIENT_PASSWORD_B`.
    Pair {
        /// Walk with this device's input records, as the game client does, over the `[swarm]`
        /// level's ground, instead of with the route's points.
        #[arg(long, value_enum)]
        input: Option<InputDevice>,
    },
    /// Many bots across a sim border (`[swarm]`). The bot password comes from
    /// `NW_SECRET_TESTCLIENT_SWARM_PASSWORD`.
    Swarm {
        /// Bots of this step instead of `[swarm] bots`.
        #[arg(long)]
        bots: Option<usize>,
    },
    /// Many bots that fight mobs (`[hunt]`, with accounts of their own and the sockets of
    /// `[swarm]`). The bot password comes from `NW_SECRET_TESTCLIENT_SWARM_PASSWORD`.
    Hunt {
        /// Bots of this step instead of `[hunt] bots`.
        #[arg(long)]
        bots: Option<usize>,
    },
    /// Many bots that gather the level's nodes near where they appear (`[gather]`, with
    /// accounts of their own and the sockets of `[swarm]`). The bot password comes from
    /// `NW_SECRET_TESTCLIENT_SWARM_PASSWORD`.
    Gather {
        /// Bots of this step instead of `[gather] bots`.
        #[arg(long)]
        bots: Option<usize>,
    },
    /// One client (the first account) says a line in area chat and waits for the server's
    /// answer: system lines for a GM chat command of a game master, else the line itself.
    Chat {
        /// The line, as typed in the chat box.
        #[arg(long, default_value = DEFAULT_CHAT_LINE)]
        line: String,
    },
    /// One client (the first account) sets up its voice chat: the service, a login token, the
    /// positional channels and a join token for the first one, each in the retail form.
    Voice,
    /// One client (the first account, which needs the game master rank) plays quests on the
    /// live servers through the client's calls and writes which quests end and where each other
    /// one stops.
    Play {
        /// The quest ids, comma-separated, in the order to try them.
        #[arg(long, value_delimiter = ',')]
        quests: Vec<String>,
        /// The activities to play after the quests, comma-separated.
        #[arg(long, value_delimiter = ',', value_enum)]
        activities: Vec<ActivityKind>,
        /// The report file (Markdown).
        #[arg(long)]
        report: PathBuf,
        /// Longest time in the world, in minutes.
        #[arg(long, default_value_t = DEFAULT_PLAY_MINUTES)]
        minutes: u64,
        /// The bot's account instead of the first account (the same password); a new one is
        /// registered. It needs the game master rank.
        #[arg(long)]
        account: Option<String>,
        /// The archetype pick of a new character (`Archetype_Soldier`): it wears that starter
        /// kit and has a bag.
        #[arg(long)]
        backstory: Option<String>,
        /// Quests a game master line starts at the setup, past their requirements, comma-
        /// separated: the start of a test after a chain a player already played live.
        #[arg(long, value_delimiter = ',')]
        gm_start: Vec<String>,
        /// The character of the foe bot of the PvP quest steps (a bot that plays the
        /// `pvp-foe` activity at the same time): the bot kills it for their player kills.
        #[arg(long)]
        pvp_foe: Option<String>,
    },
    /// Bots of game master accounts form a group of at least the game mode's `MinGroupSize`,
    /// enter its expedition from afar, walk its start area, and write where the server put each
    /// of them against the level's floor (`nw_testclient::group`). The first account leads; all
    /// take the first account's password.
    Expedition {
        /// The game mode (`GameModeID`).
        #[arg(long, default_value = DEFAULT_EXPEDITION)]
        game_mode: String,
        /// The bots' accounts, comma-separated, the leader first; each needs the game master
        /// rank. A new one is registered.
        #[arg(long, value_delimiter = ',', required = true)]
        accounts: Vec<String>,
        /// The report file (Markdown).
        #[arg(long)]
        report: PathBuf,
        /// Longest time in the world, in minutes.
        #[arg(long, default_value_t = DEFAULT_EXPEDITION_MINUTES)]
        minutes: u64,
        /// Quests the leader plays, comma-separated, once the group is formed (in place of the
        /// walk of the start area): a quest step that needs the expedition enters it with the
        /// group, and the others stay in the group until the leader ends.
        #[arg(long, value_delimiter = ',')]
        quests: Vec<String>,
        /// Quests a game master line starts at the leader's setup, past their requirements.
        #[arg(long, value_delimiter = ',')]
        gm_start: Vec<String>,
    },
}

fn main() -> ExitCode {
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => finish(runtime, run(Cli::parse())),
        Err(error) => {
            eprintln!("error: runtime failed: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs `future` to its end on `runtime`, then drops the runtime without waiting for its
/// blocking tasks: the read of standard input of an until-stopped run ends only with the input,
/// and the program must end after its report.
fn finish<F: Future>(runtime: tokio::runtime::Runtime, future: F) -> F::Output {
    let output = runtime.block_on(future);
    runtime.shutdown_background();
    output
}

async fn run(cli: Cli) -> ExitCode {
    match steps(cli).await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn steps(cli: Cli) -> Result<bool, Box<dyn std::error::Error>> {
    let config = load_testclient_config(&cli.config)?;
    nw_login::install_crypto_provider();
    let http = config
        .host_overrides()?
        .into_iter()
        .fold(reqwest::Client::builder(), |builder, (host, address)| {
            builder.resolve(&host, address)
        })
        .build()?;
    let trusted = DtlsClientConfig::new(&std::fs::read(&config.certificate_file)?)?;
    let ledger = Arc::new(LedgerSlot::new(match &cli.ledger {
        Some(path) => Some(nw_capture::LedgerWriter::new(
            Box::new(std::io::BufWriter::new(std::fs::File::create(path)?))
                as Box<dyn std::io::Write + Send>,
        )),
        None => None,
    }));
    match cli.command.unwrap_or(Command::Registration) {
        Command::Registration => {
            registration(&config, &http, &trusted, &own(&config)?, &ledger).await
        }
        Command::Pair { input } => {
            pair(&config, &http, &trusted, own(&config)?, &ledger, input).await
        }
        Command::Swarm { bots } => {
            let mut config = config;
            if let Some(bots) = bots {
                config.swarm.bots = bots;
            }
            swarm(&config, &http, trusted, ledger).await
        }
        Command::Hunt { bots } => {
            let mut config = config;
            if let Some(bots) = bots {
                config.hunt.bots = bots;
            }
            hunt(&config, &http, trusted, ledger).await
        }
        Command::Gather { bots } => {
            let mut config = config;
            if let Some(bots) = bots {
                config.gather.bots = bots;
            }
            gather(&config, &http, trusted, ledger).await
        }
        Command::Voice => voice(&config, &http, &trusted, own(&config)?, &ledger).await,
        Command::Chat { line } => {
            chat(&config, &http, &trusted, own(&config)?, line, &ledger).await
        }
        Command::Play {
            quests,
            activities,
            report,
            minutes,
            account,
            backstory,
            gm_start,
            pvp_foe,
        } => {
            let mut bot = own(&config)?;
            if let Some(account) = account {
                bot.character_name = format!("{}{PLAY_CHARACTER_MARK}", account);
                bot.account = account;
            }
            if let Some(backstory) = backstory {
                bot.creation_params = nw_login::look::bot_creation_params(&backstory)
                    .ok_or("the creation settings of the backstory do not write")?;
            }
            let settings = PlaySettings {
                account: bot,
                quests,
                gm_start,
                pvp_foe,
                hold: Duration::from_secs(minutes * SECONDS_PER_MINUTE),
                registration_timeout: Duration::from_secs(config.registration_timeout_seconds),
            };
            play(
                &config,
                &http,
                &trusted,
                &settings,
                &activities,
                &report,
                &ledger,
            )
            .await
        }
        Command::Expedition {
            game_mode,
            accounts,
            report,
            minutes,
            quests,
            gm_start,
        } => {
            let first = own(&config)?;
            let bots = accounts
                .iter()
                .enumerate()
                .map(|(seat, account)| PlaySettings {
                    account: WalkSettings {
                        account: account.clone(),
                        character_name: format!("{account}{PLAY_CHARACTER_MARK}"),
                        ..first.clone()
                    },
                    quests: if seat == LEADER {
                        quests.clone()
                    } else {
                        Vec::new()
                    },
                    gm_start: if seat == LEADER {
                        gm_start.clone()
                    } else {
                        Vec::new()
                    },
                    pvp_foe: None,
                    hold: Duration::from_secs(minutes * SECONDS_PER_MINUTE),
                    registration_timeout: Duration::from_secs(config.registration_timeout_seconds),
                })
                .collect::<Vec<_>>();
            expedition(
                &config, &http, &trusted, &game_mode, &bots, &report, &ledger,
            )
            .await
        }
    }
}

/// The test account of `registration` and `pair`.
fn own(config: &TestClientConfig) -> Result<WalkSettings, String> {
    let password = secret(PASSWORD_ENV)?;
    Ok(walk_settings(
        config,
        &config.account,
        &config.character_name,
        password,
    ))
}

async fn swarm(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: DtlsClientConfig,
    ledger: Arc<LedgerSlot>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let settings = Arc::new(config.swarm_settings(secret(SWARM_PASSWORD_ENV)?)?);
    let (ground, new_client) = fleet_parts(config, trusted, ledger)?;
    let errors_before = receive_errors();
    let stop = stop_signal(settings.fleet.duration.is_none());
    let report = run_swarm(
        http,
        Arc::clone(&settings),
        ground,
        new_client,
        stop,
        |line| {
            println!("{line}");
        },
    )
    .await;
    println!("{}", report.summary());
    Ok(print_end(
        report.error_kinds(),
        errors_before,
        &report.checks(&settings),
    ))
}

async fn hunt(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: DtlsClientConfig,
    ledger: Arc<LedgerSlot>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let settings = Arc::new(config.hunt_settings(secret(SWARM_PASSWORD_ENV)?)?);
    let (ground, new_client) = fleet_parts(config, trusted, ledger)?;
    let errors_before = receive_errors();
    let stop = stop_signal(settings.fleet.duration.is_none());
    let report = run_hunt(
        http,
        Arc::clone(&settings),
        ground,
        new_client,
        stop,
        |line| {
            println!("{line}");
        },
    )
    .await;
    println!("{}", report.summary());
    Ok(print_end(
        report.error_kinds(),
        errors_before,
        &report.checks(&settings),
    ))
}

async fn gather(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: DtlsClientConfig,
    ledger: Arc<LedgerSlot>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let (ground, new_client) = fleet_parts(config, trusted, ledger)?;
    let (nodes, gatherables) = gather_nodes(config, &ground)?;
    println!("gather nodes in the level: {}", nodes.len());
    let mut by_tradeskill = std::collections::BTreeMap::new();
    for node in nodes.iter() {
        let tradeskill = gatherables
            .get(node.gatherable)
            .and_then(|row| row.tradeskill.clone())
            .unwrap_or_default();
        *by_tradeskill.entry(tradeskill).or_insert(0_usize) += 1;
    }
    println!("gather nodes by tradeskill: {by_tradeskill:?}");
    let settings = Arc::new(config.gather_settings(secret(SWARM_PASSWORD_ENV)?, nodes)?);
    let errors_before = receive_errors();
    let stop = stop_signal(settings.fleet.duration.is_none());
    let report = run_gather(
        http,
        Arc::clone(&settings),
        ground,
        new_client,
        stop,
        |line| {
            println!("{line}");
        },
    )
    .await;
    println!("{}", report.summary());
    let name = |gatherable: u32| {
        gatherables.get(gatherable).map_or(("unknown", "?"), |row| {
            (row.id.as_str(), row.tradeskill.as_deref().unwrap_or("none"))
        })
    };
    for (gatherable, count) in report.kinds() {
        let (id, tradeskill) = name(gatherable);
        println!("gathered {count} x {id} ({tradeskill})");
    }
    for (gatherable, count) in report.refused_kinds() {
        let (id, tradeskill) = name(gatherable);
        println!("refused {count} x {id} ({tradeskill})");
    }
    Ok(print_end(
        report.error_kinds(),
        errors_before,
        &report.checks(&settings),
    ))
}

/// The gather nodes of a level and the gatherables that name their kinds.
type LevelNodes = (Arc<[GatherNode]>, Gatherables);

/// The gather nodes of the `[swarm]` level that a new character with the tools of the
/// `[gather] tradeskills` may gather: the gatherables of the content bundle of one of those
/// tradeskills that are no loot container and ask for no tradeskill rank or item (the level's
/// other gatherables are corpses, doors and quest objects); with the gatherables, which name
/// each node's kind in the report.
fn gather_nodes(
    config: &TestClientConfig,
    level: &WorldFile,
) -> Result<LevelNodes, Box<dyn std::error::Error>> {
    let tables = ContentBundle::load(&level_layout(config)?)?.tables;
    let gatherables = Gatherables::from_tables(&tables);
    let rules = GatherRules::from_tables(&tables);
    let tradeskills = &config.gather.tradeskills;
    let workable = |crc: u32| {
        let Some(gatherable) = gatherables.get(crc) else {
            return false;
        };
        let rule = rules.of(crc);
        let tool = gatherable.tradeskill.as_ref().is_some_and(|tradeskill| {
            tradeskills
                .iter()
                .any(|own| own.eq_ignore_ascii_case(tradeskill))
        });
        !gatherable.loot_container
            && rule.required_rank == 0
            && rule.required_item.is_none()
            && tool
    };
    let nodes = level
        .gatherables
        .iter()
        .filter(|point| workable(point.gatherable_id))
        .map(|point| GatherNode {
            gatherable: point.gatherable_id,
            at: point.position,
        })
        .collect();
    Ok((nodes, gatherables))
}

/// The stop of a run without a duration: it turns `true` on Ctrl-C or when standard input
/// reaches its end (the admin panel closes it to stop a batch). A timed run never stops early.
fn stop_signal(until_stopped: bool) -> watch::Receiver<bool> {
    let (stop, stopped) = watch::channel(false);
    if until_stopped {
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                () = input_end() => {}
            }
            println!("stopping: every bot closes");
            // The receivers live as long as the run.
            let _ = stop.send(true);
        });
    }
    stopped
}

/// Waits until standard input reaches its end or fails.
async fn input_end() {
    let mut input = tokio::io::stdin();
    let mut buffer = [0_u8; INPUT_BUFFER_BYTES];
    while matches!(input.read(&mut buffer).await, Ok(read) if read > 0) {}
}

/// The content of the `[swarm]` level's client build.
fn level_layout(config: &TestClientConfig) -> Result<ContentLayout, Box<dyn std::error::Error>> {
    let build = load_client_build(&config.swarm.client_build_config)?;
    Ok(ContentLayout::open(content_dir(), &build)?)
}

/// The ground of the `[swarm]` level and the maker of bot clients.
fn fleet_parts(
    config: &TestClientConfig,
    trusted: DtlsClientConfig,
    ledger: Arc<LedgerSlot>,
) -> Result<(Arc<WorldFile>, NewClient), Box<dyn std::error::Error>> {
    let ground = Arc::new(WorldFile::load(
        &level_layout(config)?,
        &config.swarm.level,
    )?);
    let mtu = config.mtu;
    let trusted = Arc::new(trusted);
    let new_client: NewClient = Arc::new(move |token| {
        RepClient::new(&trusted, mtu, token, 0).map(|client| ledger.give(client))
    });
    Ok((ground, new_client))
}

/// Prints the bot errors, the PC's UDP drops since `errors_before` and the checks; returns
/// whether every check passed.
fn print_end(
    error_kinds: std::collections::BTreeMap<&str, usize>,
    errors_before: Option<u64>,
    checks: &[Check],
) -> bool {
    for (error, bots) in error_kinds {
        println!("{bots} bots failed to start: {error}");
    }
    match (errors_before, receive_errors()) {
        (Some(before), Some(after)) => println!(
            "UDP receive buffer errors of the PC during the step: {}",
            after.saturating_sub(before)
        ),
        _ => println!("UDP receive buffer errors of the PC: Unknown"),
    }
    print_checks(checks)
}

/// Prints each check; returns whether every check passed.
fn print_checks(checks: &[Check]) -> bool {
    for check in checks {
        let result = if check.passed { "pass" } else { "FAIL" };
        println!("{}: {result}", check.name);
    }
    checks.iter().all(|check| check.passed)
}

/// The PC's count of UDP datagrams dropped for a full receive buffer (Linux).
fn receive_errors() -> Option<u64> {
    let snmp = std::fs::read_to_string(SNMP_PATH).ok()?;
    udp_receive_errors(&snmp)
}

fn secret(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is not set"))
}

fn walk_settings(
    config: &TestClientConfig,
    account: &str,
    character_name: &str,
    password: String,
) -> WalkSettings {
    WalkSettings {
        login_url: config.login_url.clone(),
        game_url: config.game_url.clone(),
        account: account.to_owned(),
        password,
        character_name: character_name.to_owned(),
        creation_params: CHARACTER_CREATION_PARAMS.to_owned(),
    }
}

async fn pair(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: &DtlsClientConfig,
    first: WalkSettings,
    ledger: &LedgerSlot,
    input: Option<InputDevice>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let records = match input {
        Some(device) => {
            let (ground, sea_level_m) =
                nw_sim_core::player_level_ground(&level_layout(config)?, &config.swarm.level)?;
            Some(RecordWalk {
                device,
                ground: Arc::new(ground),
                sea_level_m,
            })
        }
        None => None,
    };
    let second = walk_settings(
        config,
        &config.second_account,
        &config.second_character_name,
        secret(SECOND_PASSWORD_ENV)?,
    );
    let settings = PairSettings {
        first,
        second,
        route: config.route()?,
        jump_m: config.jump_m,
        hold: Duration::from_secs(config.hold_seconds),
        registration_timeout: Duration::from_secs(config.registration_timeout_seconds),
    };
    let report = run_pair(
        http,
        &settings,
        records.as_ref(),
        |token| RepClient::new(trusted, config.mtu, token, 0).map(|client| ledger.give(client)),
        |line| println!("{line}"),
    )
    .await?;
    print_walk_states(&report);
    print_saved_position(&report);
    let tolerances = (config.path_tolerance_m, config.position_tolerance_m);
    Ok(print_checks(&report.checks(tolerances.0, tolerances.1)))
}

/// Prints where the first session of the first client ended (as it saw itself and as the second
/// client saw it) and where its next login started.
fn print_saved_position(report: &PairReport) {
    let place = |position: Option<WorldPosition>| {
        position.map_or_else(
            || "none".to_owned(),
            |at| format!("{:.2} {:.2} {:.2}", at.x, at.y, at.z()),
        )
    };
    let seen = report
        .first
        .view
        .own_number()
        .and_then(|number| report.second.view.position(number));
    println!(
        "first session ended at: {} (seen by the second client at: {}); next login at: {}",
        place(report.first.view.own_position()),
        place(seen),
        place(report.again.view.own_position()),
    );
}

/// Prints how often the second client got the first one's state during the walk.
fn print_walk_states(report: &PairReport) {
    let gaps = report.walk_state_gaps();
    let Some(largest) = gaps.iter().max() else {
        println!("walker states seen during the walk: fewer than two");
        return;
    };
    let total: Duration = gaps.iter().sum();
    let late = gaps.iter().filter(|gap| **gap > LATE_STATE_GAP).count();
    let count = u32::try_from(gaps.len()).unwrap_or(u32::MAX);
    println!(
        "walker states seen during the walk: {}, mean gap {} ms, largest gap {} ms, gaps over {} ms: {late}",
        gaps.len() + 1,
        (total / count).as_millis(),
        largest.as_millis(),
        LATE_STATE_GAP.as_millis(),
    );
}

async fn chat(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: &DtlsClientConfig,
    account: WalkSettings,
    line: String,
    ledger: &LedgerSlot,
) -> Result<bool, Box<dyn std::error::Error>> {
    let settings = ChatSettings {
        account,
        line,
        hold: Duration::from_secs(config.hold_seconds),
        registration_timeout: Duration::from_secs(config.registration_timeout_seconds),
    };
    let report = run_chat(
        http,
        &settings,
        |token| RepClient::new(trusted, config.mtu, token, 0).map(|client| ledger.give(client)),
        |line| println!("{line}"),
    )
    .await?;
    for answer in &report.answers {
        println!("answer: {answer}");
    }
    if report.echoed {
        println!("the line came back as area chat");
    }
    Ok(print_checks(&report.checks()))
}

async fn voice(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: &DtlsClientConfig,
    account: WalkSettings,
    ledger: &LedgerSlot,
) -> Result<bool, Box<dyn std::error::Error>> {
    let settings = VoiceSettings {
        account,
        hold: Duration::from_secs(config.hold_seconds),
        registration_timeout: Duration::from_secs(config.registration_timeout_seconds),
    };
    let report = run_voice(
        http,
        &settings,
        |token| RepClient::new(trusted, config.mtu, token, 0).map(|client| ledger.give(client)),
        |line| println!("{line}"),
    )
    .await?;
    if let Some(service) = &report.service {
        println!("voice service: {service}");
    }
    for (name, uri) in &report.channels {
        println!("channel {name}: {uri}");
    }
    Ok(print_checks(&report.checks()))
}

async fn play(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: &DtlsClientConfig,
    settings: &PlaySettings,
    kinds: &[ActivityKind],
    report_path: &std::path::Path,
    ledger: &LedgerSlot,
) -> Result<bool, Box<dyn std::error::Error>> {
    let content = PlayContent::load(config)?;
    let data = ActivityData {
        world: &content.world,
        tables: &content.tables,
        town_stations: &content.town_stations,
    };
    let activities = kinds
        .iter()
        .map(|kind| Activity::of(*kind, &data))
        .collect();
    let data = content.play_data(activities, None);
    let report = run_play(
        http,
        settings,
        data,
        |token| RepClient::new(trusted, config.mtu, token, 0).map(|client| ledger.give(client)),
        |line| println!("{line}"),
    )
    .await?;
    std::fs::write(report_path, report.markdown())?;
    for play in report.quests.iter().chain(&report.activities) {
        println!("{}: {:?}", play.quest, play.verdict);
    }
    println!("report in {}", report_path.display());
    Ok(report.quests.len() == settings.quests.len() && report.activities.len() == kinds.len())
}

/// The sim's own gather nodes of a level's gather `points`, in its order (with the cyclic
/// stones' points).
fn gather_nodes_of(points: &[nw_world::records::GatherablePoint]) -> Arc<[GatherNode]> {
    points
        .iter()
        .map(|point| GatherNode {
            gatherable: point.gatherable_id,
            at: point.position,
        })
        .collect()
}

/// The gather nodes of each instance game mode of `tables` whose map is a level other than
/// `open_level`, by the lowercase game mode id: a trial's quest objects stand on its own level
/// (`99E_Motherwell_MSQ_PlaceStaff` in `nw_ori_eg_questmotherwell`). A level with no world file
/// (the generated catacombs) has no nodes.
fn trial_nodes(
    layout: &ContentLayout,
    tables: &nw_content::Tables,
    open_level: &str,
) -> HashMap<String, Arc<[GatherNode]>> {
    let cyclic = nw_content::gathering::CyclicGatherables::from_tables(tables);
    let mut by_level: HashMap<String, Option<Arc<[GatherNode]>>> = HashMap::new();
    InstanceCatalog::from_tables(tables)
        .modes()
        .filter(|mode| mode.map.folder != open_level)
        .filter_map(|mode| {
            let nodes = by_level
                .entry(mode.map.folder.clone())
                .or_insert_with(|| {
                    WorldFile::load(layout, &mode.map.folder).ok().map(|world| {
                        gather_nodes_of(&sim::player_content::level_gather_points(&world, &cyclic))
                    })
                })
                .clone()?;
            Some((mode.id.to_lowercase(), nodes))
        })
        .collect()
}

/// What a play bot reads of the content: the `[swarm]` level's world file, the tables and the
/// settlements' station slots.
struct PlayContent {
    layout: ContentLayout,
    world: WorldFile,
    tables: nw_content::Tables,
    town_stations: TownStations,
    courses: Arc<CourseRoutes>,
    /// The gather nodes of each instance game mode whose map is another level than the
    /// `[swarm]` level, by the lowercase game mode id.
    trial_nodes: Arc<HashMap<String, Arc<[GatherNode]>>>,
}

impl PlayContent {
    fn load(config: &TestClientConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let layout = level_layout(config)?;
        let tables = ContentBundle::load(&layout)?.tables;
        let trial_nodes = Arc::new(trial_nodes(&layout, &tables, &config.swarm.level));
        Ok(Self {
            world: WorldFile::load(&layout, &config.swarm.level)?,
            trial_nodes,
            courses: Arc::new(CourseRoutes {
                rules: QuickCourseRules::from_tables(&tables),
                paths: QuickCoursePaths::load(&layout)?,
            }),
            tables,
            town_stations: TownStations::load(&layout)?,
            layout,
        })
    }

    /// The play data of a bot that plays `activities`, in the seat `group` of a group of bots.
    fn play_data(&self, activities: Vec<Activity>, group: Option<GroupSeat>) -> PlayData {
        let points = sim::player_content::level_gather_points(
            &self.world,
            &nw_content::gathering::CyclicGatherables::from_tables(&self.tables),
        );
        let nodes = gather_nodes_of(&points);
        let gatherables = Gatherables::from_tables(&self.tables);
        let containers = points
            .iter()
            .map(|point| point.gatherable_id)
            .filter(|crc| {
                gatherables
                    .get(*crc)
                    .is_some_and(|kind| kind.loot_container)
            })
            .collect();
        PlayData {
            activities,
            activities_first: false,
            nodes,
            trial_nodes: Arc::clone(&self.trial_nodes),
            containers,
            poi_tags: PoiTags::from_tables(&self.tables),
            group,
            courses: Arc::clone(&self.courses),
            mount_reward_box: mount_reward_box(&self.tables),
        }
    }
}

/// Runs `futures` at the same time; their outputs in order.
async fn all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut running: Vec<Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
    let mut outputs: Vec<Option<F::Output>> = running.iter().map(|_| None).collect();
    std::future::poll_fn(|context| {
        let mut pending = false;
        for (future, output) in running.iter_mut().zip(outputs.iter_mut()) {
            if output.is_none() {
                match future.as_mut().poll(context) {
                    Poll::Ready(done) => *output = Some(done),
                    Poll::Pending => pending = true,
                }
            }
        }
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    outputs.into_iter().flatten().collect()
}

/// The `expedition` run: the bots of `bots` (the leader first) form a group, enter the
/// expedition of `game_mode` and walk its start area; the report goes to `report_path`.
async fn expedition(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: &DtlsClientConfig,
    game_mode: &str,
    bots: &[PlaySettings],
    report_path: &std::path::Path,
    ledger: &LedgerSlot,
) -> Result<bool, Box<dyn std::error::Error>> {
    let content = PlayContent::load(config)?;
    let catalog = InstanceCatalog::from_tables(&content.tables);
    let mode = catalog
        .by_id(game_mode)
        .ok_or_else(|| format!("no game mode {game_mode}"))?;
    let fewest = usize::try_from(mode.min_group.max(1))?;
    let most = usize::try_from(mode.capacity)?;
    if !(fewest..=most).contains(&bots.len()) {
        return Err(format!(
            "{game_mode} takes a group of {fewest} to {most} players; {} accounts given",
            bots.len()
        )
        .into());
    }
    let level = &mode.map.folder;
    let ground = CollisionIndex::new(LevelCollision::load(&content.layout, level)?);
    let (walk_ground, sea_level_m) = nw_sim_core::player_level_ground(&content.layout, level)?;
    let walk_ground: Arc<dyn nw_sim_core::Ground + Send + Sync> = Arc::new(walk_ground);
    let seats = GroupBoard::seats(bots.len());
    // With quests, the leader plays them once the group is formed; else every bot walks.
    let questing = bots.first().is_some_and(|leader| !leader.quests.is_empty());
    let runs = bots.iter().zip(&seats).map(|(settings, seat)| {
        let records = RecordWalk {
            device: seat_device(seat),
            ground: Arc::clone(&walk_ground),
            sea_level_m,
        };
        let steps = if questing {
            quest_group_steps(seat, settings.hold)
        } else {
            expedition_steps(seat, &mode.id, mode.crc, &records)
        };
        let activity = Activity {
            kind: ActivityKind::Group,
            steps: Ok(steps),
        };
        let mut data = content.play_data(vec![activity], Some(seat.clone()));
        data.activities_first = questing;
        let bot = seat.number();
        let seat = seat.clone();
        async move {
            let report = run_play(
                http,
                settings,
                data,
                |token| {
                    RepClient::new(trusted, config.mtu, token, 0).map(|client| ledger.give(client))
                },
                move |line| println!("bot {bot}: {line}"),
            )
            .await;
            if seat.leads() {
                seat.set_leader_done();
            }
            report
        }
    });
    let reports = all(runs.collect()).await;
    let mut markdown = format!(
        "# Expedition run: {game_mode} ({level}), {} bots\n",
        bots.len()
    );
    let mut passed = true;
    for ((settings, seat), report) in bots.iter().zip(&seats).zip(reports) {
        let role = if seat.leads() { "leader" } else { "member" };
        let _ = write!(
            markdown,
            "\n## Bot {} ({role}, account {}, {:?})\n\n",
            seat.number(),
            settings.account.account,
            seat_device(seat)
        );
        passed &= expedition_section(&mut markdown, report, seat, &ground, questing);
    }
    std::fs::write(report_path, &markdown)?;
    println!("{markdown}");
    println!("report in {}", report_path.display());
    Ok(passed)
}

/// The device a group bot walks with: the keyboard on even seats, a game pad on odd ones, so a
/// run walks with both.
const fn seat_device(seat: &GroupSeat) -> InputDevice {
    /// The devices the seats take in turn.
    const DEVICES: usize = 2;
    if seat.number().is_multiple_of(DEVICES) {
        InputDevice::Keys
    } else {
        InputDevice::Pad
    }
}

/// Writes one bot's part of the expedition report into `markdown`: how its steps ended and its
/// server places against the floor of `ground`; whether its steps all passed.
fn expedition_section(
    markdown: &mut String,
    report: Result<PlayReport, nw_testclient::pair::PairError>,
    seat: &GroupSeat,
    ground: &CollisionIndex,
    questing: bool,
) -> bool {
    let report = match report {
        Ok(report) => report,
        Err(error) => {
            let _ = writeln!(markdown, "- the bot did not run: {error}");
            return false;
        }
    };
    let mut passed = !report.activities.is_empty();
    for play in &report.activities {
        let _ = writeln!(markdown, "- verdict: {:?}", play.verdict);
        passed &= play.verdict == Verdict::Completed;
        for action in &play.actions {
            let _ = writeln!(markdown, "  - {action}");
        }
    }
    if questing {
        if seat.leads() {
            passed &= report
                .quests
                .iter()
                .all(|play| play.verdict == Verdict::Completed);
            let _ = writeln!(markdown, "\n{}", report.markdown());
        }
        return passed;
    }
    let _ = writeln!(
        markdown,
        "\nServer places in the instance against the floor:\n"
    );
    for line in floor_lines(&seat.places(), ground) {
        let _ = writeln!(markdown, "{line}");
    }
    passed
}

async fn registration(
    config: &TestClientConfig,
    http: &reqwest::Client,
    trusted: &DtlsClientConfig,
    settings: &WalkSettings,
    ledger: &LedgerSlot,
) -> Result<bool, Box<dyn std::error::Error>> {
    let admission = walk(http, settings, |line| println!("{line}")).await?;
    let token = login_token(&admission.token);
    let registration_timeout = Duration::from_secs(config.registration_timeout_seconds);
    let new_client =
        || RepClient::new(trusted, config.mtu, token.clone(), 0).map(|client| ledger.give(client));
    let outcome = run_rep(
        admission.rep_address,
        new_client()?,
        Hold::For(Duration::from_secs(config.hold_seconds)),
        registration_timeout,
    )
    .await?;
    let Some(status) = outcome.status else {
        println!("no registration response");
        return Ok(false);
    };
    println!(
        "registration status {status}, server version {}",
        outcome.server_version.as_deref().unwrap_or_default()
    );
    println!(
        "held {:.1} s: {} pings echoed, closed by server: {}",
        outcome.held.as_secs_f64(),
        outcome.pings,
        outcome.closed_by_server
    );
    // The ticket is single-use: the same token must be refused now.
    let again = run_rep(
        admission.rep_address,
        new_client()?,
        Hold::For(Duration::ZERO),
        registration_timeout,
    )
    .await?;
    let Some(second_status) = again.status else {
        println!("no response to the second registration");
        return Ok(false);
    };
    println!("second registration with the same ticket: status {second_status}");
    Ok(status == REGISTRATION_STATUS_AUTHORIZED
        && outcome.pings_kept_up()
        && !outcome.closed_by_server
        && second_status != REGISTRATION_STATUS_AUTHORIZED)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// Longer than any end of a run may wait.
    const BLOCKED: Duration = Duration::from_secs(10);
    const QUICK: Duration = Duration::from_secs(2);

    #[test]
    fn a_waiting_input_read_does_not_hold_the_end() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .build()
            .expect("runtime");
        let started = Instant::now();
        let code = finish(runtime, async {
            // Stands for the read of standard input, which waits until the input ends.
            tokio::task::spawn_blocking(|| std::thread::sleep(BLOCKED));
            ExitCode::SUCCESS
        });
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(started.elapsed() < QUICK, "{:?}", started.elapsed());
    }
}
