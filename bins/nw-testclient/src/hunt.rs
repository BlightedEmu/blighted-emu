// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient hunt` (M6-B spec section 9): many bots log in, walk to the nearest mob they
//! see (or, with none in view, to their hunt start), stop in reach and attack it with rising
//! edges of attack bit 9. A bot that gets a 4299 waits, asks to respawn with a 1938, and steps
//! away to show that it lives again. The checks read what each bot saw, and the mob deaths,
//! refused moves and tick times from the metrics of every sim.

use std::sync::Arc;
use std::time::Duration;

use nw_messages::{
    BodyCodec, ClientInputMsg, InputPosition, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX,
    REQUEST_RESPAWN_TYPE_INDEX, RequestRespawnMsg, WorldPosition,
};
use nw_reflect::REGISTRATION_STATUS_AUTHORIZED;
use nw_sim_core::Ground;
use reqwest::Client;

use crate::fleet::{
    BOTS_CONNECTED, BOTS_REGISTERED, BUNDLES_STEADY, BundleClock, FleetBot, FleetRun,
    FleetSettings, NewBot, NewClient, error_kinds, ratio, run_fleet,
};
use crate::pair::{Check, INPUT_CHANNEL, SEND_INTERVAL};
use crate::rep::{RepClient, RepError};
use crate::udp::RepOutcome;
use crate::view::CombatView;

/// Check: every bot got at least one 2071.
pub const BOTS_HIT: &str = "every bot hit a mob";
/// Check: the sims counted at least one mob death per [`BOTS_PER_MOB_DEATH`] bots.
pub const MOBS_DIED: &str = "mobs died";
/// Check: every bot that asked to respawn lived again.
pub const BOTS_LIVED_AGAIN: &str = "every dead bot lived again after its request";
/// Bots per mob death that the run must reach (M6-B spec section 9.3).
pub const BOTS_PER_MOB_DEATH: f64 = 10.0;
/// A bot stops this share of its reach from its mob.
const STOP_SHARE: f32 = 0.5;
/// After its 1938, a bot waits this long before it steps, so that the request is read first.
pub const STEP_DELAY: Duration = Duration::from_secs(1);
/// The step after a respawn, east, in metres.
pub const RESPAWN_STEP_M: f32 = 1.0;
/// The own position must move this far from the death to show that the bot lives again.
const STEP_PROOF_M: f32 = 0.5;
/// A request older than this at the end of the run must have been answered.
pub const RESPAWN_PROOF_WAIT: Duration = Duration::from_secs(3);

/// What the hunt needs besides the clients and the ground.
#[derive(Debug, Clone, PartialEq)]
pub struct HuntSettings {
    /// The bots and how they log in.
    pub fleet: FleetSettings,
    /// The hunt starts, east and north, in metres; bot `n` goes to start `n` modulo their
    /// number while it sees no mob.
    pub starts: Vec<[f32; 2]>,
    /// Walking speed, in metres per second.
    pub speed_mps: f32,
    /// A bot attacks a mob within this distance, in metres.
    pub reach_m: f32,
    /// Time between two attacks of a bot.
    pub attack_interval: Duration,
    /// Time from a 4299 to the bot's 1938.
    pub respawn_wait: Duration,
    /// Longest gap between two own bundles.
    pub max_bundle_gap: Duration,
}

/// What a bot does on one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HuntAction {
    /// A position input.
    Move(InputPosition),
    /// An attack input at the position.
    Attack(InputPosition),
    /// A respawn request.
    Respawn,
}

/// Where a bot is in its life.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Hunting,
    /// Dead since a time, where it died.
    Dead {
        since: Duration,
        at: WorldPosition,
    },
    /// Asked to respawn at a time; steps east of where it died.
    Asked {
        at_time: Duration,
        died: WorldPosition,
    },
}

/// The decisions of one bot.
#[derive(Debug, Clone)]
pub struct Hunter {
    settings: Arc<HuntSettings>,
    start: [f32; 2],
    phase: Phase,
    deaths_seen: u64,
    last_sent: Option<Duration>,
    last_attack: Option<Duration>,
    /// The last position sent; moves go on from there.
    sent: Option<[f32; 3]>,
    lived_again: u64,
}

impl Hunter {
    /// Bot `n` of the hunt.
    pub fn new(settings: Arc<HuntSettings>, n: usize) -> Self {
        let start = settings
            .starts
            .get(n % settings.starts.len().max(1))
            .copied()
            .unwrap_or_default();
        Self {
            settings,
            start,
            phase: Phase::Hunting,
            deaths_seen: 0,
            last_sent: None,
            last_attack: None,
            sent: None,
            lived_again: 0,
        }
    }

    /// What to do `since` the registration, with the own position `own`, the mobs in view and
    /// the 4299 count so far.
    pub fn step(
        &mut self,
        own: WorldPosition,
        mobs: &[(u64, WorldPosition)],
        deaths: u64,
        since: Duration,
        ground: &(dyn Ground + Send + Sync),
    ) -> Option<HuntAction> {
        if deaths > self.deaths_seen {
            self.deaths_seen = deaths;
            if matches!(self.phase, Phase::Asked { .. }) {
                self.lived_again += 1;
            }
            self.phase = Phase::Dead { since, at: own };
            return None;
        }
        match self.phase {
            Phase::Dead { since: died, at } => {
                if since.saturating_sub(died) < self.settings.respawn_wait {
                    return None;
                }
                self.phase = Phase::Asked {
                    at_time: since,
                    died: at,
                };
                Some(HuntAction::Respawn)
            }
            Phase::Asked { at_time, died } => {
                if horizontal(own, [died.x, died.y]) >= STEP_PROOF_M {
                    self.lived_again += 1;
                    self.phase = Phase::Hunting;
                    self.sent = Some([own.x, own.y, own.z()]);
                    return None;
                }
                if since.saturating_sub(at_time) < STEP_DELAY || !self.may_send(since) {
                    return None;
                }
                let step = [died.x + RESPAWN_STEP_M, died.y];
                self.move_toward(own, step, 0.0, since, ground)
            }
            Phase::Hunting => self.hunt(own, mobs, since, ground),
        }
    }

    fn hunt(
        &mut self,
        own: WorldPosition,
        mobs: &[(u64, WorldPosition)],
        since: Duration,
        ground: &(dyn Ground + Send + Sync),
    ) -> Option<HuntAction> {
        if !self.may_send(since) {
            return None;
        }
        let nearest = mobs
            .iter()
            .map(|(_, mob)| (*mob, horizontal(own, [mob.x, mob.y])))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let reach_m = self.settings.reach_m;
        match nearest {
            Some((_, distance)) if distance <= reach_m => {
                let rested = self
                    .last_attack
                    .is_none_or(|last| since.saturating_sub(last) >= self.settings.attack_interval);
                if !rested {
                    return None;
                }
                self.last_attack = Some(since);
                self.last_sent = Some(since);
                let here = self.sent.unwrap_or([own.x, own.y, own.z()]);
                InputPosition::from_metres(here[0], here[1], here[2]).map(HuntAction::Attack)
            }
            Some((mob, _)) => {
                self.move_toward(own, [mob.x, mob.y], reach_m * STOP_SHARE, since, ground)
            }
            None if horizontal(own, self.start) > reach_m => {
                self.move_toward(own, self.start, 0.0, since, ground)
            }
            None => None,
        }
    }

    /// Whether a send interval passed since the last input.
    fn may_send(&self, since: Duration) -> bool {
        self.last_sent
            .is_none_or(|sent| since.saturating_sub(sent) >= SEND_INTERVAL)
    }

    /// One walk step toward `goal`, stopping `stop_m` short of it, on the ground.
    fn move_toward(
        &mut self,
        own: WorldPosition,
        goal: [f32; 2],
        stop_m: f32,
        since: Duration,
        ground: &(dyn Ground + Send + Sync),
    ) -> Option<HuntAction> {
        let from = self.sent.unwrap_or([own.x, own.y, own.z()]);
        let (east, north) = (goal[0] - from[0], goal[1] - from[1]);
        let distance = east.hypot(north);
        let elapsed = self
            .last_sent
            .map_or(SEND_INTERVAL, |sent| since.saturating_sub(sent));
        let length = (self.settings.speed_mps * elapsed.as_secs_f32()).min(distance - stop_m);
        if length <= 0.0 {
            return None;
        }
        let (x, y) = (
            from[0] + east / distance * length,
            from[1] + north / distance * length,
        );
        let z = ground.height_at(x, y).unwrap_or(from[2]);
        let next = InputPosition::from_metres(x, y, z)?;
        self.sent = Some([x, y, z]);
        self.last_sent = Some(since);
        Some(HuntAction::Move(next))
    }

    /// Respawns seen to work: a step after the request, or a later death.
    pub const fn lived_again(&self) -> u64 {
        self.lived_again
    }

    /// The time of a request not answered yet.
    pub const fn waiting_since(&self) -> Option<Duration> {
        match self.phase {
            Phase::Asked { at_time, .. } => Some(at_time),
            Phase::Hunting | Phase::Dead { .. } => None,
        }
    }
}

fn horizontal(own: WorldPosition, to: [f32; 2]) -> f32 {
    (own.x - to[0]).hypot(own.y - to[1])
}

/// What one bot saw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HuntBotReport {
    /// Registration status 0.
    pub registered: bool,
    /// The server closed the connection.
    pub closed: bool,
    /// Longest gap between two own bundles.
    pub max_gap: Duration,
    /// 2071 messages for the bot's hits.
    pub hits_dealt: u64,
    /// 4299 messages for the bot.
    pub deaths: u64,
    /// Respawns seen to work.
    pub lived_again: u64,
    /// A request that was not answered within [`RESPAWN_PROOF_WAIT`] before the end.
    pub unanswered_request: bool,
    /// Why the bot could not run, if it could not.
    pub error: Option<String>,
}

/// Every bot and the sim totals of the run.
pub type HuntReport = FleetRun<HuntBotReport>;

impl HuntReport {
    /// Every check, in order (M6-B spec section 9.3).
    pub fn checks(&self, settings: &HuntSettings) -> Vec<Check> {
        let all = |rule: &dyn Fn(&HuntBotReport) -> bool| {
            !self.bots.is_empty() && self.bots.iter().all(rule)
        };
        let bots = settings.fleet.bots;
        let mut checks = vec![
            Check {
                name: BOTS_REGISTERED,
                passed: self.bots.len() == bots && all(&|bot| bot.registered),
            },
            Check {
                name: BOTS_CONNECTED,
                passed: all(&|bot| !bot.closed),
            },
            Check {
                name: BOTS_HIT,
                passed: all(&|bot| bot.hits_dealt > 0),
            },
            Check {
                name: MOBS_DIED,
                passed: self.sims.mob_deaths >= bots as f64 / BOTS_PER_MOB_DEATH,
            },
            Check {
                name: BOTS_LIVED_AGAIN,
                passed: all(&|bot| !bot.unanswered_request),
            },
            Check {
                name: BUNDLES_STEADY,
                passed: all(&|bot| bot.max_gap <= settings.max_bundle_gap),
            },
        ];
        checks.extend(
            self.sims
                .checks(self.unread_metrics, &settings.fleet.metrics_urls),
        );
        checks
    }

    /// Whether every check passed.
    pub fn passed(&self, settings: &HuntSettings) -> bool {
        self.checks(settings).iter().all(|check| check.passed)
    }

    /// How many bots stopped with each error, by error text.
    pub fn error_kinds(&self) -> std::collections::BTreeMap<&str, usize> {
        error_kinds(self.bots.iter().filter_map(|bot| bot.error.as_deref()))
    }

    /// One line of numbers for the operator.
    pub fn summary(&self) -> String {
        let sum = |count: fn(&HuntBotReport) -> u64| self.bots.iter().map(count).sum::<u64>();
        let worst_gap = self.bots.iter().map(|bot| bot.max_gap).max();
        let errors = self.bots.iter().filter(|bot| bot.error.is_some()).count();
        format!(
            "bots {}, failed to start {errors}, hits {}, deaths {}, lived again {}, mob deaths \
             {}, moves after a death {}, worst bundle gap {} ms, ticks within one tick {:.4}",
            self.bots.len(),
            sum(|bot| bot.hits_dealt),
            sum(|bot| bot.deaths),
            sum(|bot| bot.lived_again),
            self.sims.mob_deaths,
            self.sims.dead_moves,
            worst_gap.unwrap_or_default().as_millis(),
            ratio(self.sims.ticks_within_tick, self.sims.ticks),
        )
    }
}

/// One bot during its run.
struct HuntBot {
    hunter: Hunter,
    ground: Arc<dyn Ground + Send + Sync>,
    view: CombatView,
    own: Option<WorldPosition>,
    clock: BundleClock,
}

impl FleetBot for HuntBot {
    type Report = HuntBotReport;

    fn tick(
        &mut self,
        client: &mut RepClient,
        outcome: &mut RepOutcome,
        since: Duration,
    ) -> Result<(), RepError> {
        let mut seen = 0;
        for received in outcome.messages.drain(..) {
            // A message that does not decode is not a position; the checks miss it.
            if let Ok(Some(position)) = self.view.read(&received) {
                self.own = Some(position);
                seen += 1;
            }
        }
        self.clock.saw(seen, since);
        let Some(own) = self.own else {
            return Ok(());
        };
        let mobs: Vec<(u64, WorldPosition)> = self.view.mobs().collect();
        let action = self
            .hunter
            .step(own, &mobs, self.view.deaths(), since, &*self.ground);
        match action {
            None => Ok(()),
            Some(HuntAction::Move(position)) => {
                send_input(client, &ClientInputMsg::position(position))
            }
            Some(HuntAction::Attack(position)) => {
                send_input(client, &ClientInputMsg::attack(position))
            }
            Some(HuntAction::Respawn) => client.send_message(
                INPUT_CHANNEL,
                true,
                REQUEST_RESPAWN_TYPE_INDEX,
                RequestRespawnMsg::default().encode()?,
            ),
        }
    }

    fn report(self, outcome: &RepOutcome) -> HuntBotReport {
        let unanswered_request = self
            .hunter
            .waiting_since()
            .is_some_and(|asked| outcome.held.saturating_sub(asked) > RESPAWN_PROOF_WAIT);
        HuntBotReport {
            registered: outcome.status == Some(REGISTRATION_STATUS_AUTHORIZED),
            closed: outcome.closed_by_server,
            max_gap: self.clock.max_gap(outcome.held),
            hits_dealt: self.view.hits_dealt(),
            deaths: self.view.deaths(),
            lived_again: self.hunter.lived_again(),
            unanswered_request,
            error: None,
        }
    }

    fn failed(error: String) -> HuntBotReport {
        HuntBotReport {
            error: Some(error),
            ..HuntBotReport::default()
        }
    }
}

/// Sends `input` as the swarm does: unreliable, on the input channel.
fn send_input(client: &mut RepClient, input: &ClientInputMsg) -> Result<(), RepError> {
    client.send_message(
        INPUT_CHANNEL,
        false,
        RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX,
        input.encode()?,
    )
}

/// Runs every bot and reads the sim metrics before and after.
pub async fn run_hunt(
    http: &Client,
    settings: Arc<HuntSettings>,
    ground: Arc<dyn Ground + Send + Sync>,
    new_client: NewClient,
    report: impl FnMut(String),
) -> HuntReport {
    let fleet = Arc::new(settings.fleet.clone());
    let new_bot: NewBot<HuntBot> = Arc::new(move |n| HuntBot {
        hunter: Hunter::new(Arc::clone(&settings), n),
        ground: Arc::clone(&ground),
        view: CombatView::default(),
        own: None,
        clock: BundleClock::new(),
    });
    run_fleet(http, fleet, new_client, new_bot, report).await
}

#[cfg(test)]
mod tests {
    use nw_sim_core::test_support::NoGround;

    use super::*;
    use crate::fleet::test_support::test_fleet;
    use crate::fleet::{METRICS_READ, NO_REFUSED_MOVES, SimTotals, TICKS_FAST};

    const ORIGIN: f32 = 1000.0;
    const HEIGHT: f32 = 50.0;
    const SPEED_MPS: f32 = 4.0;
    const REACH_M: f32 = 2.5;
    const ATTACK_INTERVAL: Duration = Duration::from_millis(600);
    const RESPAWN_WAIT: Duration = Duration::from_millis(2500);
    const DURATION: Duration = Duration::from_secs(120);
    const MOB: u64 = (1 << 34) + 1;
    const FAR_MOB: u64 = MOB + 1;

    fn settings() -> Arc<HuntSettings> {
        Arc::new(HuntSettings {
            fleet: test_fleet(DURATION),
            starts: vec![[ORIGIN, ORIGIN]],
            speed_mps: SPEED_MPS,
            reach_m: REACH_M,
            attack_interval: ATTACK_INTERVAL,
            respawn_wait: RESPAWN_WAIT,
            max_bundle_gap: Duration::from_millis(250),
        })
    }

    fn at(x: f32) -> WorldPosition {
        WorldPosition::from_metres(x, ORIGIN, HEIGHT).expect("finite")
    }

    fn x_of(action: Option<HuntAction>) -> f32 {
        match action {
            Some(HuntAction::Move(position) | HuntAction::Attack(position)) => position.x,
            other => panic!("not a position: {other:?}"),
        }
    }

    #[test]
    fn a_bot_walks_to_the_nearest_mob_and_stops_in_reach() {
        let mut hunter = Hunter::new(settings(), 0);
        let mobs = [(FAR_MOB, at(ORIGIN + 20.0)), (MOB, at(ORIGIN - 10.0))];
        let mut own = at(ORIGIN);
        let mut since = SEND_INTERVAL;
        let first = hunter.step(own, &mobs, 0, since, &NoGround);
        assert!(matches!(first, Some(HuntAction::Move(_))));
        assert!(x_of(first) < ORIGIN, "toward the nearer mob, west");
        let step_m = SPEED_MPS * SEND_INTERVAL.as_secs_f32();
        assert!(
            (ORIGIN - x_of(first) - step_m).abs() < 0.01,
            "at walking speed"
        );
        let mut attacked_at = None;
        for _ in 0..100 {
            since += SEND_INTERVAL;
            match hunter.step(own, &mobs, 0, since, &NoGround) {
                Some(HuntAction::Move(position)) => own = at(position.x),
                Some(HuntAction::Attack(position)) => {
                    attacked_at = Some(position.x);
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        let attacked_at = attacked_at.expect("an attack in reach");
        assert!((attacked_at - (ORIGIN - 10.0)).abs() <= REACH_M);
        assert_eq!(
            hunter.step(own, &mobs, 0, since + SEND_INTERVAL, &NoGround),
            None,
            "the next attack waits for the interval"
        );
        assert!(matches!(
            hunter.step(own, &mobs, 0, since + ATTACK_INTERVAL, &NoGround),
            Some(HuntAction::Attack(_))
        ));
    }

    #[test]
    fn a_bot_without_a_mob_walks_to_its_start() {
        let mut hunter = Hunter::new(settings(), 0);
        let away = at(ORIGIN + 30.0);
        assert!(x_of(hunter.step(away, &[], 0, SEND_INTERVAL, &NoGround)) < ORIGIN + 30.0);
        let mut home = Hunter::new(settings(), 0);
        assert_eq!(
            home.step(at(ORIGIN), &[], 0, SEND_INTERVAL, &NoGround),
            None
        );
    }

    #[test]
    fn a_dead_bot_asks_to_respawn_after_its_wait() {
        let mut hunter = Hunter::new(settings(), 0);
        let mobs = [(MOB, at(ORIGIN + 1.0))];
        let died = Duration::from_secs(10);
        let own = at(ORIGIN);
        assert_eq!(hunter.step(own, &mobs, 1, died, &NoGround), None);
        assert_eq!(
            hunter.step(
                own,
                &mobs,
                1,
                died + RESPAWN_WAIT - SEND_INTERVAL,
                &NoGround
            ),
            None,
            "dead: no move, no attack"
        );
        assert_eq!(
            hunter.step(own, &mobs, 1, died + RESPAWN_WAIT, &NoGround),
            Some(HuntAction::Respawn)
        );
        let asked = died + RESPAWN_WAIT;
        assert_eq!(hunter.waiting_since(), Some(asked));
        assert_eq!(
            hunter.step(own, &mobs, 1, asked + SEND_INTERVAL, &NoGround),
            None,
            "the request is read before the step"
        );
        let step = hunter.step(own, &mobs, 1, asked + STEP_DELAY, &NoGround);
        assert!(x_of(step) > ORIGIN, "a step east");
        let stepped = at(ORIGIN + RESPAWN_STEP_M);
        assert_eq!(
            hunter.step(
                stepped,
                &mobs,
                1,
                asked + STEP_DELAY + SEND_INTERVAL,
                &NoGround
            ),
            None
        );
        assert_eq!((hunter.lived_again(), hunter.waiting_since()), (1, None));
    }

    /// A check and the fault that must fail it alone.
    type Fault = (&'static str, fn(&mut HuntReport));

    fn good_bot() -> HuntBotReport {
        HuntBotReport {
            registered: true,
            closed: false,
            max_gap: Duration::from_millis(100),
            hits_dealt: 5,
            deaths: 1,
            lived_again: 1,
            unanswered_request: false,
            error: None,
        }
    }

    fn good_report() -> HuntReport {
        HuntReport {
            bots: vec![good_bot(), good_bot()],
            sims: SimTotals {
                ticks_within_tick: 999.0,
                ticks: 1000.0,
                mob_deaths: 1.0,
                ..SimTotals::default()
            },
            unread_metrics: 0,
        }
    }

    #[test]
    fn the_hunt_checks_fail_on_each_fault() {
        let settings = settings();
        assert!(good_report().passed(&settings));
        let faults: [Fault; 9] = [
            (BOTS_REGISTERED, |report| report.bots[0].registered = false),
            (BOTS_CONNECTED, |report| report.bots[1].closed = true),
            (BOTS_HIT, |report| report.bots[0].hits_dealt = 0),
            (MOBS_DIED, |report| report.sims.mob_deaths = 0.0),
            (BOTS_LIVED_AGAIN, |report| {
                report.bots[1].unanswered_request = true;
            }),
            (BUNDLES_STEADY, |report| {
                report.bots[0].max_gap = Duration::from_secs(1);
            }),
            (TICKS_FAST, |report| report.sims.ticks_within_tick = 900.0),
            (NO_REFUSED_MOVES, |report| report.sims.refused_moves = 1.0),
            (METRICS_READ, |report| report.unread_metrics = 1),
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
        assert!(!HuntReport::default().passed(&settings));
    }
}
