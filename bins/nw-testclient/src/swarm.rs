// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient swarm` (M5 spec section 11): many bots log in, walk to their own lane and then
//! back and forth across the start line, which the operator puts on a `sim` border. Each bot
//! follows the ground of the world file. The checks read what each bot saw of itself, and the
//! handoff and tick times from the metrics of every sim.

use std::sync::Arc;
use std::time::{Duration, Instant};

use nw_messages::WorldPosition;
use nw_messages::{BodyCodec, ClientInputMsg, InputPosition, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX};
use nw_reflect::{LoginToken, REGISTRATION_STATUS_AUTHORIZED};
use nw_sim_core::Ground;
use reqwest::Client;
use sim::server::{
    HANDOFF_MOVED, HANDOFF_REFUSED, HANDOFF_TIMEOUT, METRIC_HANDOFF_SECONDS, METRIC_HANDOFFS,
    METRIC_TICK_SECONDS, ONE_TICK_SECONDS, RESULT_LABEL,
};
use tokio::task::JoinSet;

use crate::metrics::Scrape;
use crate::pair::{Check, INPUT_CHANNEL, SEND_INTERVAL};
use crate::rep::{RepClient, RepError};
use crate::route::{Route, RouteError};
use crate::udp::{RepOutcome, run_rep_with};
use crate::view::WorldView;
use crate::walk::{WalkSettings, login_token, walk};

/// Check: every bot got registration status 0.
pub const BOTS_REGISTERED: &str = "every bot registered";
/// Check: every bot crossed the start line at least `min_crossings` times.
pub const BOTS_CROSSED: &str = "every bot crossed the sim border often enough";
/// Check: every own position of every bot was on its route.
pub const BOTS_ON_ROUTE: &str = "every bot stayed on its route";
/// Check: no bot was closed by the server.
pub const BOTS_CONNECTED: &str = "no bot was closed";
/// Check: no gap between two own bundles was longer than the limit.
pub const BUNDLES_STEADY: &str = "the bundles never stopped";
/// Check: enough handoffs, and none refused or late.
pub const HANDOFFS_DONE: &str = "enough handoffs and none failed";
/// Check: 99 % of the handoffs took at most one tick.
pub const HANDOFFS_FAST: &str = "handoff p99 within one tick";
/// Check: 99 % of the ticks took at most one tick.
pub const TICKS_FAST: &str = "tick p99 within one tick";
/// The share of the p99 checks.
pub const P99_SHARE: f64 = 0.99;
/// Letters of a character name suffix: `aaa` to `zzz`.
pub const NAME_SUFFIX_LETTERS: u32 = 3;
const ALPHABET: u32 = 26;
/// Most bots: one name suffix each.
pub const MAX_BOTS: usize = 26 * 26 * 26;
/// Digits of the account number.
const ACCOUNT_DIGITS: usize = 5;
/// Metrics count of a histogram.
const COUNT_SUFFIX: &str = "_count";

/// What the swarm needs besides the clients and the ground.
#[derive(Debug, Clone, PartialEq)]
pub struct SwarmSettings {
    /// Bots to run.
    pub bots: usize,
    /// Account names are this and a number.
    pub account_prefix: String,
    /// Character names are this and three letters.
    pub character_prefix: String,
    /// The password of every bot account.
    pub password: String,
    /// `https://login.<domain>`.
    pub login_url: String,
    /// `https://game.<domain>`.
    pub game_url: String,
    /// New bots per second.
    pub bots_per_second: u32,
    /// Lanes are spread this far north and south of the start, in metres.
    pub lane_spread_m: f32,
    /// Bots walk this far east and west of the start line, in metres.
    pub swing_m: f32,
    /// Walking speed, in metres per second.
    pub speed_mps: f32,
    /// Time in the world per bot.
    pub duration: Duration,
    /// Longest wait for a registration response.
    pub registration_timeout: Duration,
    /// Fewest crossings of the start line per bot.
    pub min_crossings: u32,
    /// Longest gap between two own bundles.
    pub max_bundle_gap: Duration,
    /// Largest distance of an own position from the route, in metres.
    pub path_tolerance_m: f32,
    /// The metrics URL of every sim.
    pub metrics_urls: Vec<String>,
}

/// What one bot saw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BotReport {
    /// Registration status 0.
    pub registered: bool,
    /// The server closed the connection.
    pub closed: bool,
    /// Crossings of the start line.
    pub crossings: u32,
    /// Longest gap between two own bundles.
    pub max_gap: Duration,
    /// Largest distance of an own position from the route, in metres.
    pub worst_off_route_m: f32,
    /// Why the bot could not run, if it could not.
    pub error: Option<String>,
}

/// The sums of the sim metrics over the run.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SimTotals {
    /// Handoffs that moved a player.
    pub moved: f64,
    /// Handoffs refused or not answered in time.
    pub failed: f64,
    /// Handoffs within one tick.
    pub handoffs_within_tick: f64,
    /// Every handoff.
    pub handoffs: f64,
    /// Ticks within one tick.
    pub ticks_within_tick: f64,
    /// Every tick.
    pub ticks: f64,
}

impl SimTotals {
    /// The totals of a scrape of every sim.
    pub fn of(scrape: &Scrape) -> Self {
        let result = |value| Some((RESULT_LABEL, value));
        let count = |histogram: &str| scrape.total(&format!("{histogram}{COUNT_SUFFIX}"), None);
        Self {
            moved: scrape.total(METRIC_HANDOFFS, result(HANDOFF_MOVED)),
            failed: scrape.total(METRIC_HANDOFFS, result(HANDOFF_REFUSED))
                + scrape.total(METRIC_HANDOFFS, result(HANDOFF_TIMEOUT)),
            handoffs_within_tick: scrape.bucket(METRIC_HANDOFF_SECONDS, ONE_TICK_SECONDS),
            handoffs: count(METRIC_HANDOFF_SECONDS),
            ticks_within_tick: scrape.bucket(METRIC_TICK_SECONDS, ONE_TICK_SECONDS),
            ticks: count(METRIC_TICK_SECONDS),
        }
    }

    /// What happened between `before` and `self`.
    pub fn since(&self, before: &Self) -> Self {
        Self {
            moved: self.moved - before.moved,
            failed: self.failed - before.failed,
            handoffs_within_tick: self.handoffs_within_tick - before.handoffs_within_tick,
            handoffs: self.handoffs - before.handoffs,
            ticks_within_tick: self.ticks_within_tick - before.ticks_within_tick,
            ticks: self.ticks - before.ticks,
        }
    }
}

/// Every bot and the sim totals of the run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SwarmReport {
    /// One report per bot.
    pub bots: Vec<BotReport>,
    /// The sim metrics of the run.
    pub sims: SimTotals,
}

impl SwarmReport {
    /// Every check, in order (M5 spec section 11.3).
    pub fn checks(&self, settings: &SwarmSettings) -> Vec<Check> {
        let all =
            |rule: &dyn Fn(&BotReport) -> bool| !self.bots.is_empty() && self.bots.iter().all(rule);
        let share = |within: f64, total: f64| total > 0.0 && within / total >= P99_SHARE;
        let wanted = settings.bots as f64 * f64::from(settings.min_crossings);
        vec![
            Check {
                name: BOTS_REGISTERED,
                passed: self.bots.len() == settings.bots && all(&|bot| bot.registered),
            },
            Check {
                name: BOTS_CROSSED,
                passed: all(&|bot| bot.crossings >= settings.min_crossings),
            },
            Check {
                name: BOTS_ON_ROUTE,
                passed: all(&|bot| bot.worst_off_route_m <= settings.path_tolerance_m),
            },
            Check {
                name: BOTS_CONNECTED,
                passed: all(&|bot| !bot.closed),
            },
            Check {
                name: BUNDLES_STEADY,
                passed: all(&|bot| bot.max_gap <= settings.max_bundle_gap),
            },
            Check {
                name: HANDOFFS_DONE,
                passed: self.sims.moved >= wanted && self.sims.failed == 0.0,
            },
            Check {
                name: HANDOFFS_FAST,
                passed: share(self.sims.handoffs_within_tick, self.sims.handoffs),
            },
            Check {
                name: TICKS_FAST,
                passed: share(self.sims.ticks_within_tick, self.sims.ticks),
            },
        ]
    }

    /// Whether every check passed.
    pub fn passed(&self, settings: &SwarmSettings) -> bool {
        self.checks(settings).iter().all(|check| check.passed)
    }

    /// One line of numbers for the operator.
    pub fn summary(&self) -> String {
        let worst_gap = self.bots.iter().map(|bot| bot.max_gap).max();
        let fewest = self.bots.iter().map(|bot| bot.crossings).min();
        let errors = self.bots.iter().filter(|bot| bot.error.is_some()).count();
        format!(
            "bots {}, failed to start {errors}, fewest crossings {}, worst bundle gap {} ms, \
             handoffs {} (failed {}), within one tick: handoffs {:.4}, ticks {:.4}",
            self.bots.len(),
            fewest.unwrap_or_default(),
            worst_gap.unwrap_or_default().as_millis(),
            self.sims.moved,
            self.sims.failed,
            ratio(self.sims.handoffs_within_tick, self.sims.handoffs),
            ratio(self.sims.ticks_within_tick, self.sims.ticks),
        )
    }
}

fn ratio(part: f64, whole: f64) -> f64 {
    if whole > 0.0 { part / whole } else { 0.0 }
}

/// The account and character names of bot `n`: the account prefix and five digits, the
/// character prefix and three letters (character names hold letters only).
pub fn bot_names(account_prefix: &str, character_prefix: &str, n: usize) -> (String, String) {
    let account = format!("{account_prefix}{n:0ACCOUNT_DIGITS$}");
    let mut suffix = String::new();
    let mut rest = u32::try_from(n).unwrap_or(u32::MAX);
    for _ in 0..NAME_SUFFIX_LETTERS {
        suffix.insert(0, char::from(b'a' + (rest % ALPHABET) as u8));
        rest /= ALPHABET;
    }
    (account, format!("{character_prefix}{suffix}"))
}

/// The route of bot `n` of `bots`: north or south to its lane, then west and east of the start
/// line, long enough for `duration` at `speed_mps`.
///
/// # Errors
///
/// Returns [`RouteError`] for numbers that cannot be walked.
pub fn bot_route(
    n: usize,
    bots: usize,
    lane_spread_m: f32,
    swing_m: f32,
    speed_mps: f32,
    duration: Duration,
) -> Result<Route, RouteError> {
    let lane = if bots > 1 {
        -lane_spread_m + 2.0 * lane_spread_m * n as f32 / (bots - 1) as f32
    } else {
        0.0
    };
    let walk_m = speed_mps * duration.as_secs_f32();
    let legs = (walk_m / (2.0 * swing_m)).ceil() as usize + 1;
    let mut points = vec![[0.0, 0.0], [0.0, lane]];
    for leg in 0..legs {
        let side = if leg % 2 == 0 { -swing_m } else { swing_m };
        points.push([side, lane]);
    }
    Route::new(points, speed_mps)
}

/// Where a bot sends itself: the route offset from `origin` after `walked`, on the ground where
/// the level has a height, else at the origin's height.
pub fn target(
    route: &Route,
    origin: WorldPosition,
    walked: Duration,
    ground: &(impl Ground + ?Sized),
) -> Option<InputPosition> {
    let [east, north] = route.offset_at(walked);
    let (x, y) = (origin.x + east, origin.y + north);
    let z = ground.height_at(x, y).unwrap_or(origin.z());
    InputPosition::from_metres(x, y, z)
}

/// The numbers one bot collects from its own positions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BotTrack {
    origin: Option<WorldPosition>,
    east_of_start: Option<bool>,
    last_bundle: Option<Duration>,
    crossings: u32,
    max_gap: Duration,
    worst_off_route_m: f32,
}

impl BotTrack {
    /// New own positions that arrived `since` the registration.
    pub fn saw(&mut self, route: &Route, positions: &[WorldPosition], since: Duration) {
        if positions.is_empty() {
            return;
        }
        if let Some(last) = self.last_bundle {
            self.max_gap = self.max_gap.max(since.saturating_sub(last));
        }
        self.last_bundle = Some(since);
        for position in positions {
            let origin = *self.origin.get_or_insert(*position);
            let offset = [position.x - origin.x, position.y - origin.y];
            self.worst_off_route_m = self.worst_off_route_m.max(route.distance_to(offset));
            let east = position.x > origin.x;
            if self.east_of_start.is_some_and(|before| before != east) {
                self.crossings += 1;
            }
            self.east_of_start = Some(east);
        }
    }

    /// The start position: the first own position.
    pub const fn origin(&self) -> Option<WorldPosition> {
        self.origin
    }
}

/// One bot during its run.
struct Bot<'a> {
    route: &'a Route,
    ground: &'a (dyn Ground + Send + Sync),
    view: WorldView,
    own_seen: usize,
    track: BotTrack,
    started: Option<Duration>,
    last_sent: Option<Duration>,
}

impl Bot<'_> {
    fn tick(
        &mut self,
        client: &mut RepClient,
        outcome: &mut RepOutcome,
        since: Duration,
    ) -> Result<(), RepError> {
        for received in outcome.messages.drain(..) {
            // A message that does not decode is not a position; the checks miss it.
            let _ = self.view.read(&received);
        }
        if let Some(own) = self.view.own_number() {
            let track = self.view.track(own);
            self.track.saw(self.route, &track[self.own_seen..], since);
            self.own_seen = track.len();
        }
        let Some(origin) = self.track.origin() else {
            return Ok(());
        };
        let started = *self.started.get_or_insert(since);
        if self
            .last_sent
            .is_some_and(|sent| since.saturating_sub(sent) < SEND_INTERVAL)
        {
            return Ok(());
        }
        self.last_sent = Some(since);
        let walked = since.saturating_sub(started);
        let Some(position) = target(self.route, origin, walked, self.ground) else {
            return Ok(());
        };
        let body = ClientInputMsg::position(position).encode()?;
        client.send_message(
            INPUT_CHANNEL,
            false,
            RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX,
            body,
        )
    }
}

/// Makes the REP client of a bot.
pub type NewClient = Arc<dyn Fn(LoginToken) -> Result<RepClient, RepError> + Send + Sync>;

/// Runs every bot and reads the sim metrics before and after.
pub async fn run_swarm(
    http: &Client,
    settings: Arc<SwarmSettings>,
    ground: Arc<dyn Ground + Send + Sync>,
    new_client: NewClient,
    mut report: impl FnMut(String),
) -> SwarmReport {
    let before = scrape(http, &settings.metrics_urls, &mut report).await;
    let mut bots = JoinSet::new();
    let pause = Duration::from_secs(1) / settings.bots_per_second.max(1);
    let started = Instant::now();
    for n in 0..settings.bots {
        let (http, settings, ground, new_client) = (
            http.clone(),
            Arc::clone(&settings),
            Arc::clone(&ground),
            Arc::clone(&new_client),
        );
        bots.spawn(async move { run_bot(&http, &settings, n, &*ground, &*new_client).await });
        tokio::time::sleep(pause).await;
    }
    report(format!(
        "{} bots started in {:.1} s",
        settings.bots,
        started.elapsed().as_secs_f64()
    ));
    let mut reports = Vec::with_capacity(settings.bots);
    while let Some(done) = bots.join_next().await {
        reports.push(done.unwrap_or_else(|error| BotReport {
            error: Some(error.to_string()),
            ..BotReport::default()
        }));
    }
    let after = scrape(http, &settings.metrics_urls, &mut report).await;
    SwarmReport {
        bots: reports,
        sims: after.since(&before),
    }
}

async fn run_bot(
    http: &Client,
    settings: &SwarmSettings,
    n: usize,
    ground: &(dyn Ground + Send + Sync),
    new_client: &(dyn Fn(LoginToken) -> Result<RepClient, RepError> + Send + Sync),
) -> BotReport {
    let failed = |error: String| BotReport {
        error: Some(error),
        ..BotReport::default()
    };
    let (account, character_name) =
        bot_names(&settings.account_prefix, &settings.character_prefix, n);
    let walk_settings = WalkSettings {
        login_url: settings.login_url.clone(),
        game_url: settings.game_url.clone(),
        account,
        password: settings.password.clone(),
        character_name,
    };
    let route = match bot_route(
        n,
        settings.bots,
        settings.lane_spread_m,
        settings.swing_m,
        settings.speed_mps,
        settings.duration,
    ) {
        Ok(route) => route,
        Err(error) => return failed(error.to_string()),
    };
    let admission = match walk(http, &walk_settings, |_| {}).await {
        Ok(admission) => admission,
        Err(error) => return failed(error.to_string()),
    };
    let client = match new_client(login_token(&admission.token)) {
        Ok(client) => client,
        Err(error) => return failed(error.to_string()),
    };
    let mut bot = Bot {
        route: &route,
        ground,
        view: WorldView::default(),
        own_seen: 0,
        track: BotTrack::default(),
        started: None,
        last_sent: None,
    };
    let ran = run_rep_with(
        admission.rep_address,
        client,
        settings.duration,
        settings.registration_timeout,
        |client, outcome, since| bot.tick(client, outcome, since),
    )
    .await;
    match ran {
        Ok(outcome) => BotReport {
            registered: outcome.status == Some(REGISTRATION_STATUS_AUTHORIZED),
            closed: outcome.closed_by_server,
            crossings: bot.track.crossings,
            max_gap: bot.track.max_gap,
            worst_off_route_m: bot.track.worst_off_route_m,
            error: None,
        },
        Err(error) => failed(error.to_string()),
    }
}

/// The totals of every sim's metrics; a sim that cannot be read is reported and counts as 0.
async fn scrape(http: &Client, urls: &[String], report: &mut impl FnMut(String)) -> SimTotals {
    let mut sum = Scrape::default();
    for url in urls {
        let text = match http.get(url).send().await {
            Ok(response) => response.text().await,
            Err(error) => Err(error),
        };
        match text {
            Ok(text) => sum.add(&Scrape::parse(&text)),
            Err(error) => report(format!("metrics of {url} not read: {error}")),
        }
    }
    SimTotals::of(&sum)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use nw_sim_core::test_support::NoGround;

    use super::*;

    const BOTS: usize = 1000;
    const SPREAD_M: f32 = 150.0;
    const SWING_M: f32 = 20.0;
    const SPEED_MPS: f32 = 4.0;
    const DURATION: Duration = Duration::from_secs(120);
    const MIN_CROSSINGS: u32 = 4;
    const ORIGIN: f32 = 1000.0;
    const GROUND_HEIGHT: f32 = 37.5;
    const TICK: Duration = Duration::from_millis(33);
    const LONG_GAP: Duration = Duration::from_millis(400);

    fn settings() -> SwarmSettings {
        SwarmSettings {
            bots: 2,
            account_prefix: "bot".to_owned(),
            character_prefix: "Swarm".to_owned(),
            password: "password-ok".to_owned(),
            login_url: String::new(),
            game_url: String::new(),
            bots_per_second: 50,
            lane_spread_m: SPREAD_M,
            swing_m: SWING_M,
            speed_mps: SPEED_MPS,
            duration: DURATION,
            registration_timeout: DURATION,
            min_crossings: MIN_CROSSINGS,
            max_bundle_gap: Duration::from_millis(250),
            path_tolerance_m: 1.0,
            metrics_urls: Vec::new(),
        }
    }

    fn at(x: f32, y: f32) -> WorldPosition {
        WorldPosition::from_metres(x, y, ORIGIN).expect("finite")
    }

    struct Flat;

    impl Ground for Flat {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_HEIGHT)
        }
    }

    #[test]
    fn bot_names_are_letters_and_unique() {
        let names: Vec<(String, String)> = (0..MAX_BOTS)
            .map(|n| bot_names("bot", "Swarm", n))
            .collect();
        assert_eq!(names[0], ("bot00000".to_owned(), "Swarmaaa".to_owned()));
        assert_eq!(names[27].1, "Swarmabb");
        let characters: BTreeSet<&String> = names.iter().map(|(_, character)| character).collect();
        let accounts: BTreeSet<&String> = names.iter().map(|(account, _)| account).collect();
        assert_eq!((characters.len(), accounts.len()), (MAX_BOTS, MAX_BOTS));
        assert!(
            characters
                .iter()
                .all(|name| name.bytes().all(|byte| byte.is_ascii_alphabetic()))
        );
    }

    #[test]
    fn bot_routes_cross_the_start_line() {
        let first = bot_route(0, BOTS, SPREAD_M, SWING_M, SPEED_MPS, DURATION).expect("route");
        let last =
            bot_route(BOTS - 1, BOTS, SPREAD_M, SWING_M, SPEED_MPS, DURATION).expect("route");
        assert!(first.duration() >= DURATION);
        assert_eq!(first.offset_at(DURATION * 2)[1], -SPREAD_M);
        assert_eq!(last.offset_at(DURATION * 2)[1], SPREAD_M);
        let mut track = BotTrack::default();
        let steps = (DURATION.as_millis() / TICK.as_millis()) as u32;
        for step in 0..steps {
            let walked = TICK * step;
            let [east, north] = first.offset_at(walked);
            track.saw(&first, &[at(ORIGIN + east, ORIGIN + north)], walked);
        }
        assert!(track.crossings >= MIN_CROSSINGS, "{}", track.crossings);
        assert!(track.worst_off_route_m < 0.01);
        assert!(track.max_gap <= TICK);
    }

    #[test]
    fn a_route_follows_the_ground() {
        let route = bot_route(0, 1, SPREAD_M, SWING_M, SPEED_MPS, DURATION).expect("route");
        let origin = at(ORIGIN, ORIGIN);
        let walked = Duration::from_secs(1);
        let on_ground = target(&route, origin, walked, &Flat).expect("finite");
        assert!((on_ground.z() - GROUND_HEIGHT).abs() < 0.01);
        assert!((on_ground.x - (ORIGIN - SPEED_MPS)).abs() < 0.01);
        let no_ground = target(&route, origin, walked, &NoGround).expect("finite");
        assert!((no_ground.z() - origin.z()).abs() < 0.01);
    }

    #[test]
    fn crossings_and_gaps_are_counted() {
        let route = bot_route(0, 1, SPREAD_M, SWING_M, SPEED_MPS, DURATION).expect("route");
        let mut track = BotTrack::default();
        track.saw(&route, &[at(ORIGIN, ORIGIN)], TICK);
        track.saw(
            &route,
            &[at(ORIGIN + 1.0, ORIGIN), at(ORIGIN - 1.0, ORIGIN)],
            TICK * 2,
        );
        track.saw(&route, &[], TICK * 3);
        track.saw(
            &route,
            &[at(ORIGIN + 2.0, ORIGIN + 5.0)],
            TICK * 2 + LONG_GAP,
        );
        assert_eq!(track.crossings, 3);
        assert_eq!(track.max_gap, LONG_GAP);
        assert!((track.worst_off_route_m - 5.0).abs() < 0.01);
    }

    /// A check and the fault that must fail it alone.
    type Fault = (&'static str, fn(&mut SwarmReport));

    fn good_bot() -> BotReport {
        BotReport {
            registered: true,
            closed: false,
            crossings: MIN_CROSSINGS,
            max_gap: Duration::from_millis(100),
            worst_off_route_m: 0.5,
            error: None,
        }
    }

    fn good_report() -> SwarmReport {
        SwarmReport {
            bots: vec![good_bot(), good_bot()],
            sims: SimTotals {
                moved: 8.0,
                failed: 0.0,
                handoffs_within_tick: 100.0,
                handoffs: 100.0,
                ticks_within_tick: 999.0,
                ticks: 1000.0,
            },
        }
    }

    #[test]
    fn the_swarm_checks_fail_on_each_fault() {
        let settings = settings();
        assert!(good_report().passed(&settings));
        let faults: [Fault; 8] = [
            (BOTS_REGISTERED, |report| report.bots[0].registered = false),
            (BOTS_CROSSED, |report| report.bots[1].crossings = 1),
            (BOTS_ON_ROUTE, |report| {
                report.bots[0].worst_off_route_m = 3.0
            }),
            (BOTS_CONNECTED, |report| report.bots[0].closed = true),
            (BUNDLES_STEADY, |report| {
                report.bots[1].max_gap = Duration::from_secs(1);
            }),
            (HANDOFFS_DONE, |report| report.sims.failed = 1.0),
            (HANDOFFS_FAST, |report| {
                report.sims.handoffs_within_tick = 90.0
            }),
            (TICKS_FAST, |report| report.sims.ticks_within_tick = 900.0),
        ];
        for (name, fault) in faults {
            let mut report = good_report();
            fault(&mut report);
            let failed: Vec<&str> = report
                .checks(&settings)
                .iter()
                .filter(|check| !check.passed)
                .map(|check| check.name)
                .collect();
            assert_eq!(failed, vec![name]);
        }
        let mut missing = good_report();
        missing.bots.pop();
        assert!(!missing.passed(&settings));
        assert!(!SwarmReport::default().passed(&settings));
    }

    #[test]
    fn sim_totals_come_from_the_metrics_and_their_change() {
        let text = format!(
            "{METRIC_HANDOFFS}{{{RESULT_LABEL}=\"{HANDOFF_MOVED}\"}} 10\n\
             {METRIC_HANDOFFS}{{{RESULT_LABEL}=\"{HANDOFF_TIMEOUT}\"}} 1\n\
             {METRIC_HANDOFF_SECONDS}_bucket{{le=\"0.033\"}} 9\n\
             {METRIC_HANDOFF_SECONDS}_count 11\n"
        );
        let totals = SimTotals::of(&Scrape::parse(&text));
        assert_eq!((totals.moved, totals.failed), (10.0, 1.0));
        assert_eq!((totals.handoffs_within_tick, totals.handoffs), (9.0, 11.0));
        assert_eq!(totals.since(&totals), SimTotals::default());
    }
}
