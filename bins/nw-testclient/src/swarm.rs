// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient swarm` (M5 spec section 11): many bots log in, walk from where they appear to
//! their own lane and then back and forth across the start line, which the operator puts on a
//! `sim` border. Each bot follows the ground of the world file. The checks read what each bot saw
//! of itself, and the handoffs, refused moves and tick times from the metrics of every sim.

use std::sync::Arc;
use std::time::Duration;

use nw_messages::WorldPosition;
use nw_messages::{BodyCodec, ClientInputMsg, InputPosition, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX};
use nw_reflect::REGISTRATION_STATUS_AUTHORIZED;
use nw_sim_core::Ground;
use reqwest::Client;

use crate::fleet::{
    BOTS_CONNECTED, BOTS_REGISTERED, BUNDLES_STEADY, BundleClock, FleetBot, FleetRun,
    FleetSettings, NewBot, NewClient, error_kinds, p99, ratio, run_fleet,
};
use crate::pair::{Check, INPUT_CHANNEL, SEND_INTERVAL};
use crate::rep::{RepClient, RepError};
use crate::route::{Route, RouteError};
use crate::udp::RepOutcome;
use crate::view::OwnView;

/// Check: every bot crossed the start line at least `min_crossings` times.
pub const BOTS_CROSSED: &str = "every bot crossed the sim border often enough";
/// Check: every own position of every bot was on its route.
pub const BOTS_ON_ROUTE: &str = "every bot stayed on its route";
/// Check: enough handoffs, and none refused or late.
pub const HANDOFFS_DONE: &str = "enough handoffs and none failed";
/// Check: 99 % of the handoffs took at most one tick.
pub const HANDOFFS_FAST: &str = "handoff p99 within one tick";
/// Check: every bot got at least [`MIN_BUNDLE_SHARE`] of its bundles.
pub const BUNDLES_ARRIVED: &str = "every bot got 99 % of its bundles";
/// Share of the own bundles (one per tick) that must arrive (M5 load spec section 7.3).
pub const MIN_BUNDLE_SHARE: f64 = 0.99;

/// What the swarm needs besides the clients and the ground.
#[derive(Debug, Clone, PartialEq)]
pub struct SwarmSettings {
    /// The bots and how they log in.
    pub fleet: FleetSettings,
    /// Lanes are spread this far north and south of the start, in metres.
    pub lane_spread_m: f32,
    /// Bots walk this far east and west of the start line, in metres.
    pub swing_m: f32,
    /// Walking speed, in metres per second.
    pub speed_mps: f32,
    /// Fewest crossings of the start line per bot.
    pub min_crossings: u32,
    /// Longest gap between two own bundles.
    pub max_bundle_gap: Duration,
    /// Largest distance of an own position from the route, in metres.
    pub path_tolerance_m: f32,
    /// The director's start positions, east and north, in metres; their x are sim borders.
    pub starts: Vec<[f32; 2]>,
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
    /// Own bundles that arrived.
    pub bundles_received: u64,
    /// Own bundles that the ticks from the first one to the end of the run hold.
    pub bundles_expected: u64,
    /// Why the bot could not run, if it could not.
    pub error: Option<String>,
}

/// Every bot and the sim totals of the run.
pub type SwarmReport = FleetRun<BotReport>;

impl SwarmReport {
    /// Every check, in order (M5 spec section 11.3).
    pub fn checks(&self, settings: &SwarmSettings) -> Vec<Check> {
        let all =
            |rule: &dyn Fn(&BotReport) -> bool| !self.bots.is_empty() && self.bots.iter().all(rule);
        let wanted = settings.fleet.bots as f64 * f64::from(settings.min_crossings);
        let mut checks = vec![
            Check {
                name: BOTS_REGISTERED,
                passed: self.bots.len() == settings.fleet.bots && all(&|bot| bot.registered),
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
                passed: p99(self.sims.handoffs_within_tick, self.sims.handoffs),
            },
            Check {
                name: BUNDLES_ARRIVED,
                passed: all(&|bot| {
                    bot.bundles_expected > 0
                        && bot.bundles_received as f64
                            >= bot.bundles_expected as f64 * MIN_BUNDLE_SHARE
                }),
            },
        ];
        checks.extend(
            self.sims
                .checks(self.unread_metrics, &settings.fleet.metrics_urls),
        );
        checks
    }

    /// Whether every check passed.
    pub fn passed(&self, settings: &SwarmSettings) -> bool {
        self.checks(settings).iter().all(|check| check.passed)
    }

    /// How many bots stopped with each error, by error text.
    pub fn error_kinds(&self) -> std::collections::BTreeMap<&str, usize> {
        error_kinds(self.bots.iter().filter_map(|bot| bot.error.as_deref()))
    }

    /// One line of numbers for the operator.
    pub fn summary(&self) -> String {
        let worst_gap = self.bots.iter().map(|bot| bot.max_gap).max();
        let worst_share = self
            .bots
            .iter()
            .map(|bot| ratio(bot.bundles_received as f64, bot.bundles_expected as f64))
            .fold(1.0, f64::min);
        let fewest = self.bots.iter().map(|bot| bot.crossings).min();
        let errors = self.bots.iter().filter(|bot| bot.error.is_some()).count();
        format!(
            "bots {}, failed to start {errors}, fewest crossings {}, worst bundle gap {} ms, \
             fewest bundles {worst_share:.4}, handoffs {} (failed {}), within one tick: handoffs \
             {:.4}, ticks {:.4}",
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

/// The route of bot `n` of `bots`, east and north of the start: from `from`, where the bot
/// appears, to its lane north or south of the start, then west and east of the start line, long
/// enough for `duration` at `speed_mps`.
///
/// # Errors
///
/// Returns [`RouteError`] for numbers that cannot be walked.
pub fn bot_route(
    from: [f32; 2],
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
    let mut points = vec![from, [0.0, lane]];
    for leg in 0..legs {
        let side = if leg % 2 == 0 { -swing_m } else { swing_m };
        points.push([side, lane]);
    }
    Route::new(points, speed_mps)
}

/// Where a bot sends itself: the route offset from `start` after `walked`, on the ground where
/// the level has a height, else at `fallback_z`.
pub fn target(
    route: &Route,
    start: [f32; 2],
    fallback_z: f32,
    walked: Duration,
    ground: &(impl Ground + ?Sized),
) -> Option<InputPosition> {
    let [east, north] = route.offset_at(walked);
    let (x, y) = (start[0] + east, start[1] + north);
    let z = ground.height_at(x, y).unwrap_or(fallback_z);
    InputPosition::from_metres(x, y, z)
}

/// The start of `starts` nearest to `position`.
pub fn nearest_start(starts: &[[f32; 2]], position: WorldPosition) -> Option<[f32; 2]> {
    let distance = |start: &[f32; 2]| (start[0] - position.x).hypot(start[1] - position.y);
    starts
        .iter()
        .copied()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
}

/// The numbers one bot collects from its own positions, against the start.
#[derive(Debug, Clone, PartialEq)]
pub struct BotTrack {
    start: [f32; 2],
    east_of_start: Option<bool>,
    clock: BundleClock,
    crossings: u32,
    worst_off_route_m: f32,
}

impl BotTrack {
    /// A bot that saw nothing yet; crossings count against the x of `start`.
    pub const fn new(start: [f32; 2]) -> Self {
        Self {
            start,
            east_of_start: None,
            clock: BundleClock::new(),
            crossings: 0,
            worst_off_route_m: 0.0,
        }
    }

    /// New own positions that arrived `since` the registration.
    pub fn saw(&mut self, route: &Route, positions: &[WorldPosition], since: Duration) {
        self.clock.saw(positions.len(), since);
        for position in positions {
            let offset = [position.x - self.start[0], position.y - self.start[1]];
            self.worst_off_route_m = self.worst_off_route_m.max(route.distance_to(offset));
            let east = position.x > self.start[0];
            if self.east_of_start.is_some_and(|before| before != east) {
                self.crossings += 1;
            }
            self.east_of_start = Some(east);
        }
    }

    /// When the own bundles came.
    pub const fn clock(&self) -> &BundleClock {
        &self.clock
    }
}

/// One bot during its run. Its route and its start line come from where it first sees itself.
struct Bot {
    settings: Arc<SwarmSettings>,
    n: usize,
    ground: Arc<dyn Ground + Send + Sync>,
    view: OwnView,
    walk: Option<Walk>,
    last_sent: Option<Duration>,
}

/// The route of a bot that has seen itself.
struct Walk {
    route: Route,
    start: [f32; 2],
    track: BotTrack,
    started: Duration,
    height: f32,
}

impl FleetBot for Bot {
    type Report = BotReport;

    fn tick(
        &mut self,
        client: &mut RepClient,
        outcome: &mut RepOutcome,
        since: Duration,
    ) -> Result<(), RepError> {
        let mut seen = Vec::new();
        for received in outcome.messages.drain(..) {
            // A message that does not decode is not a position; the checks miss it.
            if let Ok(Some(position)) = self.view.read(&received) {
                seen.push(position);
            }
        }
        if self.walk.is_none() {
            let Some(first) = seen.first() else {
                return Ok(());
            };
            let settings = &*self.settings;
            let Some(start) = nearest_start(&settings.starts, *first) else {
                return Ok(());
            };
            let route = bot_route(
                [first.x - start[0], first.y - start[1]],
                self.n,
                settings.fleet.bots,
                settings.lane_spread_m,
                settings.swing_m,
                settings.speed_mps,
                settings.fleet.duration,
            )?;
            self.walk = Some(Walk {
                route,
                start,
                track: BotTrack::new(start),
                started: since,
                height: first.z(),
            });
        }
        let Some(walk) = &mut self.walk else {
            return Ok(());
        };
        walk.track.saw(&walk.route, &seen, since);
        if self
            .last_sent
            .is_some_and(|sent| since.saturating_sub(sent) < SEND_INTERVAL)
        {
            return Ok(());
        }
        self.last_sent = Some(since);
        let walked = since.saturating_sub(walk.started);
        let Some(position) = target(&walk.route, walk.start, walk.height, walked, &*self.ground)
        else {
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

    fn report(self, outcome: &RepOutcome) -> BotReport {
        let track = self
            .walk
            .map_or_else(|| BotTrack::new(NO_START), |walk| walk.track);
        let (bundles_received, bundles_expected) = track.clock.bundles(outcome.held);
        BotReport {
            registered: outcome.status == Some(REGISTRATION_STATUS_AUTHORIZED),
            closed: outcome.closed_by_server,
            crossings: track.crossings,
            max_gap: track.clock.max_gap(outcome.held),
            worst_off_route_m: track.worst_off_route_m,
            bundles_received,
            bundles_expected,
            error: None,
        }
    }

    fn failed(error: String) -> BotReport {
        BotReport {
            error: Some(error),
            ..BotReport::default()
        }
    }
}

/// Runs every bot and reads the sim metrics before and after.
pub async fn run_swarm(
    http: &Client,
    settings: Arc<SwarmSettings>,
    ground: Arc<dyn Ground + Send + Sync>,
    new_client: NewClient,
    report: impl FnMut(String),
) -> SwarmReport {
    let fleet = Arc::new(settings.fleet.clone());
    let new_bot: NewBot<Bot> = Arc::new(move |n| Bot {
        settings: Arc::clone(&settings),
        n,
        ground: Arc::clone(&ground),
        view: OwnView::default(),
        walk: None,
        last_sent: None,
    });
    run_fleet(http, fleet, new_client, new_bot, report).await
}

/// The start of a bot that never saw itself: its report has no bundle and fails.
const NO_START: [f32; 2] = [0.0, 0.0];

#[cfg(test)]
mod tests {
    use nw_sim_core::test_support::NoGround;

    use super::*;
    use crate::fleet::test_support::test_fleet;
    use crate::fleet::{METRICS_READ, NO_REFUSED_MOVES, SimTotals, TICKS_FAST};

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
            fleet: test_fleet(DURATION),
            lane_spread_m: SPREAD_M,
            swing_m: SWING_M,
            speed_mps: SPEED_MPS,
            min_crossings: MIN_CROSSINGS,
            max_bundle_gap: Duration::from_millis(250),
            path_tolerance_m: 1.0,
            starts: vec![[ORIGIN, ORIGIN]],
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

    fn lanes(n: usize) -> Route {
        bot_route([0.0, 0.0], n, BOTS, SPREAD_M, SWING_M, SPEED_MPS, DURATION).expect("route")
    }

    #[test]
    fn bot_routes_cross_the_start_line() {
        let first = lanes(0);
        let last = lanes(BOTS - 1);
        assert!(first.duration() >= DURATION);
        assert_eq!(first.offset_at(DURATION * 2)[1], -SPREAD_M);
        assert_eq!(last.offset_at(DURATION * 2)[1], SPREAD_M);
        let elsewhere = bot_route(
            [SWING_M, 3.0],
            0,
            BOTS,
            SPREAD_M,
            SWING_M,
            SPEED_MPS,
            DURATION,
        )
        .expect("route");
        assert_eq!(
            elsewhere.offset_at(Duration::ZERO),
            [SWING_M, 3.0],
            "starts where it is"
        );
        let mut track = BotTrack::new([ORIGIN, ORIGIN]);
        let steps = (DURATION.as_millis() / TICK.as_millis()) as u32;
        for step in 0..steps {
            let walked = TICK * step;
            let [east, north] = first.offset_at(walked);
            track.saw(&first, &[at(ORIGIN + east, ORIGIN + north)], walked);
        }
        assert!(track.crossings >= MIN_CROSSINGS, "{}", track.crossings);
        assert!(track.worst_off_route_m < 0.01);
        assert!(track.clock().max_gap(TICK * (steps - 1)) <= TICK);
    }

    fn single() -> Route {
        bot_route([0.0, 0.0], 0, 1, SPREAD_M, SWING_M, SPEED_MPS, DURATION).expect("route")
    }

    #[test]
    fn a_route_follows_the_ground() {
        let route = single();
        let walked = Duration::from_secs(1);
        let on_ground = target(&route, [ORIGIN, ORIGIN], ORIGIN, walked, &Flat).expect("finite");
        assert!((on_ground.z() - GROUND_HEIGHT).abs() < 0.01);
        assert!((on_ground.x - (ORIGIN - SPEED_MPS)).abs() < 0.01);
        let no_ground =
            target(&route, [ORIGIN, ORIGIN], ORIGIN, walked, &NoGround).expect("finite");
        assert!((no_ground.z() - ORIGIN).abs() < 0.01);
    }

    #[test]
    fn crossings_count_against_the_start_line_and_gaps_are_counted() {
        let route = single();
        let mut track = BotTrack::new([ORIGIN, ORIGIN]);
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
        assert_eq!(track.clock().max_gap(TICK * 2 + LONG_GAP), LONG_GAP);
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
            bundles_received: 299,
            bundles_expected: 300,
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
                refused_moves: 0.0,
                mob_deaths: 0.0,
            },
            unread_metrics: 0,
        }
    }

    #[test]
    fn the_swarm_checks_fail_on_each_fault() {
        let settings = settings();
        assert!(good_report().passed(&settings));
        let faults: [Fault; 11] = [
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
            (NO_REFUSED_MOVES, |report| report.sims.refused_moves = 1.0),
            (METRICS_READ, |report| report.unread_metrics = 1),
            (BUNDLES_ARRIVED, |report| {
                report.bots[1].bundles_received = 290
            }),
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
        let mut no_metrics = settings;
        no_metrics.fleet.metrics_urls.clear();
        assert!(
            !good_report().passed(&no_metrics),
            "no metrics read is no pass"
        );
    }

    #[test]
    fn bot_errors_are_counted_by_kind() {
        const REFUSED: &str = "no registration response";
        const QUEUE: &str = "login queue timed out";
        let mut report = good_report();
        for error in [REFUSED, QUEUE, REFUSED] {
            report.bots.push(BotReport {
                error: Some(error.to_owned()),
                ..BotReport::default()
            });
        }
        let kinds: Vec<(&str, usize)> = report.error_kinds().into_iter().collect();
        assert_eq!(kinds, vec![(QUEUE, 1), (REFUSED, 2)]);
        assert!(good_report().error_kinds().is_empty());
    }

    #[test]
    fn the_nearest_start_is_the_line() {
        let starts = [[0.0, 0.0], [1000.0, 0.0], [0.0, 1000.0]];
        assert_eq!(
            nearest_start(&starts, at(900.0, 100.0)),
            Some([1000.0, 0.0])
        );
        assert_eq!(nearest_start(&starts, at(10.0, 700.0)), Some([0.0, 1000.0]));
        assert_eq!(nearest_start(&[], at(0.0, 0.0)), None);
    }
}
