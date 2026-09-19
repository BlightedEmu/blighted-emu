// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mob brain (M6-A spec section 5): a pure state machine. The caller passes the time and the
//! players near the mob, applies the returned action, and puts the mob on the ground.
//!
//! A step first changes the state as the table says, then acts in the new state, so a mob that
//! sees a player in reach attacks at once. Every move is capped by the mob's speed and kept inside
//! the leash circle around the spawn, so a mob never leaves it.

use crate::combat::{Dice, Threat};
use crate::movement::Position;

const MILLIS_PER_SECOND: f32 = 1000.0;
/// Longest time one step may move a mob, so a stalled tick never makes it jump.
const MAX_STEP_MS: u64 = 1000;
/// Distance at which a walking mob counts as arrived.
const ARRIVED_M: f32 = 0.05;
/// Resolution of the random wander points.
const WANDER_STEPS: u32 = 1_000_000;

/// How a mob moves and when it swings (tutorial-ai-behavior note, section 8; ship-mobs note,
/// section 2: retail sailors started swings at 0.89-1.81 m, ran only to close a gap of 4-12 m and
/// walked or stood nearer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gait {
    /// It starts a swing at a target this near.
    pub swing_start_m: f32,
    /// A fighting mob farther than this from its target runs at the mob speed; nearer, it walks.
    pub run_beyond_m: f32,
    /// Its speed while it walks to its target.
    pub walk_speed_mps: f32,
    /// Its speed while it wanders near its spawn.
    pub wander_speed_mps: f32,
}

impl Gait {
    /// A gait with one reach and one speed for every move: it swings at `reach_m` and else runs
    /// at `speed_mps`.
    pub const fn uniform(reach_m: f32, speed_mps: f32) -> Self {
        Self {
            swing_start_m: reach_m,
            run_beyond_m: reach_m,
            walk_speed_mps: speed_mps,
            wander_speed_mps: speed_mps,
        }
    }
}

/// The numbers of the brain (`sim.toml` section `[combat]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrainSettings {
    /// A player this near wakes the mob.
    pub wake_radius_m: f32,
    /// An idle mob without a player within the wake radius for this long falls asleep.
    pub sleep_after_ms: u64,
    /// Sight before the creature type's offset.
    pub base_sight_m: f32,
    /// An idle mob walks to points this near its spawn.
    pub wander_radius_m: f32,
    /// Wait between two wander walks.
    pub wander_pause_ms: u64,
    /// A mob never goes farther from its spawn.
    pub leash_radius_m: f32,
    /// A mob hits a target this near.
    pub attack_reach_m: f32,
    /// Time between two hits of a mob.
    pub mob_attack_interval_ms: u64,
    /// Running speed.
    pub mob_speed_mps: f32,
    /// How it moves and when it swings.
    pub gait: Gait,
    /// Time from death to respawn.
    pub mob_respawn_ms: u64,
}

/// The state of a mob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MobState {
    /// No player near; costs nothing.
    Asleep,
    /// Wanders near its spawn.
    Idle,
    /// Walks to its target.
    Chase,
    /// Hits its target.
    Attack,
    /// Walks back to its spawn; takes no damage.
    Return,
    /// Waits for its respawn.
    Dead,
}

impl MobState {
    /// Every state, in order.
    pub const ALL: [Self; 6] = [
        Self::Asleep,
        Self::Idle,
        Self::Chase,
        Self::Attack,
        Self::Return,
        Self::Dead,
    ];

    /// The metric label of the state.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Asleep => "asleep",
            Self::Idle => "idle",
            Self::Chase => "chase",
            Self::Attack => "attack",
            Self::Return => "return",
            Self::Dead => "dead",
        }
    }
}

/// A living player (or player ghost) near the mob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seen {
    /// Its entity number.
    pub entity: u64,
    /// Where it is.
    pub position: Position,
}

/// What the caller does after a step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BrainAction {
    /// Nothing.
    None,
    /// The mob walked to this point.
    MoveTo(Position),
    /// The mob hits this entity.
    Attack(u64),
    /// The mob came back to life at its spawn: refill its health.
    Respawned,
    /// The mob is back at its spawn after a return: refill its health.
    Home,
}

/// What an awake mob without a target does (admin studio spec section 6.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Behaviour {
    /// It wanders near its spawn and attacks a player in sight.
    #[default]
    Wander,
    /// It stands at its spawn and attacks a player in sight.
    Guard,
    /// It walks its route and attacks a player in sight; after a fight it goes back to where it
    /// left the route.
    Patrol,
    /// It stands at its spawn and fights only a player who hit it.
    Idle,
}

/// How a route goes on after its last point.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RouteMode {
    /// Back to the first point.
    #[default]
    Loop,
    /// Back along the points, then forward again.
    BackAndForth,
    /// It stays at the last point.
    Once,
}

/// The points a patrolling mob walks (admin studio spec section 6.4).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Route {
    /// The points, in order.
    pub points: Vec<Position>,
    /// The wait at each point (missing: no wait).
    pub waits_ms: Vec<u64>,
    /// What comes after the last point.
    pub mode: RouteMode,
}

/// Where a mob is on its route.
#[derive(Debug, Clone, Default, PartialEq)]
struct RouteWalk {
    route: Route,
    next: usize,
    backwards: bool,
    wait_until_ms: u64,
}

impl RouteWalk {
    /// Goes on to the point after `next`, as the mode says; `false` at the end of a once route.
    fn advance(&mut self) -> bool {
        let last = self.route.points.len().saturating_sub(1);
        match self.route.mode {
            RouteMode::Loop => self.next = if self.next >= last { 0 } else { self.next + 1 },
            RouteMode::Once if self.next >= last => return false,
            RouteMode::Once => self.next += 1,
            RouteMode::BackAndForth => {
                if last == 0 {
                    return true;
                }
                if self.backwards && self.next == 0 || !self.backwards && self.next >= last {
                    self.backwards = !self.backwards;
                }
                self.next = if self.backwards {
                    self.next - 1
                } else {
                    self.next + 1
                };
            }
        }
        true
    }
}

/// One mob's mind.
#[derive(Debug, Clone, PartialEq)]
pub struct Brain {
    state: MobState,
    /// Where it respawns.
    home: Position,
    /// The centre of its leash: its home, or for a patrol the point where it left its route.
    spawn: Position,
    position: Position,
    sight_m: f32,
    target: Option<u64>,
    threat: Threat,
    /// When the state last changed or the last wander walk ended.
    since_ms: u64,
    last_player_ms: u64,
    last_step_ms: Option<u64>,
    last_attack_ms: Option<u64>,
    wander_to: Option<Position>,
    behaviour: Behaviour,
    route: Option<RouteWalk>,
}

impl Brain {
    /// The mob stood still until `now_ms` (held by a script): its next step moves only the time
    /// after it, not the time it stood.
    pub fn stood_until(&mut self, now_ms: u64) {
        if self.last_step_ms.is_some() {
            self.last_step_ms = Some(now_ms);
        }
    }

    /// An asleep mob at `spawn` that sees `sight_m` far.
    pub fn new(spawn: Position, sight_m: f32) -> Self {
        Self {
            state: MobState::Asleep,
            home: spawn,
            spawn,
            position: spawn,
            sight_m,
            target: None,
            threat: Threat::default(),
            since_ms: 0,
            last_player_ms: 0,
            last_step_ms: None,
            last_attack_ms: None,
            wander_to: None,
            behaviour: Behaviour::Wander,
            route: None,
        }
    }

    /// What it does without a target from now on.
    pub const fn set_behaviour(&mut self, behaviour: Behaviour) {
        self.behaviour = behaviour;
    }

    /// The route it walks with [`Behaviour::Patrol`], from its first point.
    pub fn set_route(&mut self, route: Route) {
        self.route = (!route.points.is_empty()).then_some(RouteWalk {
            route,
            ..RouteWalk::default()
        });
    }

    /// It sees players `sight_m` far from now on.
    pub const fn set_sight(&mut self, sight_m: f32) {
        self.sight_m = sight_m;
    }

    /// The state.
    pub const fn state(&self) -> MobState {
        self.state
    }

    /// The entity it chases or hits.
    pub const fn target(&self) -> Option<u64> {
        self.target
    }

    /// Where it is.
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Where it spawns.
    pub const fn spawn(&self) -> Position {
        self.spawn
    }

    /// Its next hit comes one attack interval after `now_ms` at the earliest.
    pub const fn wait_to_attack(&mut self, now_ms: u64) {
        self.last_attack_ms = Some(now_ms);
    }

    /// Moves the mob up to `max_m` toward `to`, inside the leash circle of `leash_m` around its
    /// spawn (the lunge of its swing); returns where it is then.
    pub fn lunge(&mut self, to: Position, max_m: f32, leash_m: f32) -> Position {
        self.walk(to, max_m, leash_m)
    }

    /// Puts the mob on the ground at height `z`.
    pub const fn set_height(&mut self, z: f32) {
        self.position.z = z;
    }

    fn enter(&mut self, state: MobState, now_ms: u64) {
        self.state = state;
        self.since_ms = now_ms;
        self.wander_to = None;
    }

    /// The target to chase now: the top threat that is still near, else the nearest player in
    /// sight (not for [`Behaviour::Idle`]). Threat of an attacker that is not near is dropped.
    fn choose_target(&mut self, seen: &[Seen]) -> Option<Seen> {
        while let Some(top) = self.threat.top() {
            if let Some(found) = seen.iter().find(|player| player.entity == top) {
                return Some(*found);
            }
            self.threat.forget(top);
        }
        if self.behaviour == Behaviour::Idle {
            return None;
        }
        seen.iter()
            .filter(|player| player.position.horizontal_distance(self.position) <= self.sight_m)
            .min_by(|a, b| {
                let da = a.position.horizontal_distance(self.position);
                let db = b.position.horizontal_distance(self.position);
                da.total_cmp(&db).then(a.entity.cmp(&b.entity))
            })
            .copied()
    }

    /// Walks up to `max_m` towards `to`, inside the leash circle; returns the new position.
    fn walk(&mut self, to: Position, max_m: f32, leash_m: f32) -> Position {
        let distance = self.position.horizontal_distance(to);
        let mut next = if distance <= max_m {
            Position {
                z: self.position.z,
                ..to
            }
        } else {
            let share = max_m / distance;
            Position {
                x: self.position.x + (to.x - self.position.x) * share,
                y: self.position.y + (to.y - self.position.y) * share,
                z: self.position.z,
            }
        };
        let from_spawn = next.horizontal_distance(self.spawn);
        if from_spawn > leash_m {
            let share = leash_m / from_spawn;
            next.x = self.spawn.x + (next.x - self.spawn.x) * share;
            next.y = self.spawn.y + (next.y - self.spawn.y) * share;
        }
        self.position = next;
        next
    }

    fn wander_point(&self, radius_m: f32, dice: &mut Dice) -> Position {
        let fraction = |dice: &mut Dice| dice.below(WANDER_STEPS) as f32 / WANDER_STEPS as f32;
        let angle = fraction(dice) * std::f32::consts::TAU;
        let distance = radius_m * fraction(dice).sqrt();
        Position {
            x: self.spawn.x + distance * angle.cos(),
            y: self.spawn.y + distance * angle.sin(),
            z: self.spawn.z,
        }
    }
}

/// One step of `brain` at `now_ms`, with the living players near it.
pub fn step(
    brain: &mut Brain,
    seen: &[Seen],
    now_ms: u64,
    settings: &BrainSettings,
    dice: &mut Dice,
) -> BrainAction {
    let elapsed_ms = brain
        .last_step_ms
        .map_or(0, |last| now_ms.saturating_sub(last).min(MAX_STEP_MS));
    brain.last_step_ms = Some(now_ms);
    let reach = |speed_mps: f32| speed_mps * elapsed_ms as f32 / MILLIS_PER_SECOND;
    let player_near = seen.iter().any(|player| {
        player.position.horizontal_distance(brain.position) <= settings.wake_radius_m
    });
    if player_near {
        brain.last_player_ms = now_ms;
    }
    match brain.state {
        MobState::Dead => {
            if now_ms.saturating_sub(brain.since_ms) < settings.mob_respawn_ms {
                return BrainAction::None;
            }
            brain.position = brain.home;
            brain.spawn = brain.home;
            if let Some(walk) = &mut brain.route {
                *walk = RouteWalk {
                    route: std::mem::take(&mut walk.route),
                    ..RouteWalk::default()
                };
            }
            brain.threat.clear();
            brain.target = None;
            brain.last_player_ms = now_ms;
            brain.enter(MobState::Idle, now_ms);
            BrainAction::Respawned
        }
        MobState::Return => {
            let at = brain.walk(
                brain.spawn,
                reach(settings.mob_speed_mps),
                settings.leash_radius_m,
            );
            if at.horizontal_distance(brain.spawn) > ARRIVED_M {
                return BrainAction::MoveTo(at);
            }
            brain.threat.clear();
            brain.target = None;
            brain.enter(MobState::Idle, now_ms);
            BrainAction::Home
        }
        MobState::Asleep => {
            // A patrol walks its route with no player near: it never sleeps.
            if player_near || brain.behaviour == Behaviour::Patrol {
                brain.enter(MobState::Idle, now_ms);
            }
            BrainAction::None
        }
        MobState::Idle | MobState::Chase | MobState::Attack => {
            fight(brain, seen, now_ms, &reach, settings, dice)
        }
    }
}

/// The awake states: Idle, Chase and Attack. `reach` gives the metres of this step at a speed.
fn fight(
    brain: &mut Brain,
    seen: &[Seen],
    now_ms: u64,
    reach: &dyn Fn(f32) -> f32,
    settings: &BrainSettings,
    dice: &mut Dice,
) -> BrainAction {
    let Some(target) = brain.choose_target(seen) else {
        brain.target = None;
        if brain.state != MobState::Idle {
            brain.enter(MobState::Return, now_ms);
            return BrainAction::None;
        }
        let max_m = reach(settings.gait.wander_speed_mps);
        return idle(brain, now_ms, max_m, settings, dice);
    };
    brain.target = Some(target.entity);
    if target.position.horizontal_distance(brain.spawn)
        > settings.leash_radius_m + settings.attack_reach_m
    {
        brain.threat.forget(target.entity);
        brain.target = None;
        brain.enter(MobState::Return, now_ms);
        return BrainAction::None;
    }
    let gait = settings.gait;
    let distance = target.position.horizontal_distance(brain.position);
    if distance > gait.swing_start_m {
        if brain.state != MobState::Chase {
            brain.enter(MobState::Chase, now_ms);
        }
        let speed_mps = if distance > gait.run_beyond_m {
            settings.mob_speed_mps
        } else {
            gait.walk_speed_mps
        };
        // It stops just inside its swing distance, not on the target.
        let short_m = (distance - gait.swing_start_m + ARRIVED_M).max(0.0);
        let max_m = reach(speed_mps).min(short_m);
        return BrainAction::MoveTo(brain.walk(target.position, max_m, settings.leash_radius_m));
    }
    if brain.state != MobState::Attack {
        brain.enter(MobState::Attack, now_ms);
    }
    let ready = brain
        .last_attack_ms
        .is_none_or(|last| now_ms.saturating_sub(last) >= settings.mob_attack_interval_ms);
    if !ready {
        return BrainAction::None;
    }
    brain.last_attack_ms = Some(now_ms);
    BrainAction::Attack(target.entity)
}

/// An idle mob with no target: wanders, or falls asleep after quiet time.
fn idle(
    brain: &mut Brain,
    now_ms: u64,
    max_m: f32,
    settings: &BrainSettings,
    dice: &mut Dice,
) -> BrainAction {
    if brain.behaviour != Behaviour::Patrol
        && now_ms.saturating_sub(brain.last_player_ms) >= settings.sleep_after_ms
    {
        brain.enter(MobState::Asleep, now_ms);
        return BrainAction::None;
    }
    match brain.behaviour {
        Behaviour::Wander => {}
        Behaviour::Guard | Behaviour::Idle => return BrainAction::None,
        Behaviour::Patrol => return patrol(brain, now_ms, max_m),
    }
    let to = match brain.wander_to {
        Some(to) => to,
        None if now_ms.saturating_sub(brain.since_ms) >= settings.wander_pause_ms => {
            let to = brain.wander_point(settings.wander_radius_m, dice);
            brain.wander_to = Some(to);
            to
        }
        None => return BrainAction::None,
    };
    let at = brain.walk(to, max_m, settings.leash_radius_m);
    if at.horizontal_distance(to) <= ARRIVED_M {
        brain.wander_to = None;
        brain.since_ms = now_ms;
    }
    BrainAction::MoveTo(at)
}

/// A patrolling mob with no target: walks to its next route point, waits there, and goes on.
/// Its leash centre follows it, so a fight starts where it left the route.
fn patrol(brain: &mut Brain, now_ms: u64, max_m: f32) -> BrainAction {
    let Some(walk) = &mut brain.route else {
        return BrainAction::None;
    };
    if now_ms < walk.wait_until_ms {
        return BrainAction::None;
    }
    let Some(&to) = walk.route.points.get(walk.next) else {
        return BrainAction::None;
    };
    let distance = brain.position.horizontal_distance(to);
    let at = if distance <= max_m {
        Position {
            z: brain.position.z,
            ..to
        }
    } else {
        let share = max_m / distance;
        Position {
            x: brain.position.x + (to.x - brain.position.x) * share,
            y: brain.position.y + (to.y - brain.position.y) * share,
            z: brain.position.z,
        }
    };
    if at.horizontal_distance(to) <= ARRIVED_M {
        let wait_ms = walk
            .route
            .waits_ms
            .get(walk.next)
            .copied()
            .unwrap_or_default();
        if walk.advance() {
            walk.wait_until_ms = now_ms.saturating_add(wait_ms);
        } else {
            walk.wait_until_ms = u64::MAX;
        }
    }
    brain.position = at;
    brain.spawn = at;
    if distance <= ARRIVED_M {
        return BrainAction::None;
    }
    BrainAction::MoveTo(at)
}

/// `attacker` hit the mob, adding `threat`. A mob that returns or is dead ignores it and returns
/// `false`; any other mob wakes and chases.
pub fn hurt(brain: &mut Brain, attacker: u64, threat: u64, now_ms: u64) -> bool {
    if matches!(brain.state, MobState::Return | MobState::Dead) {
        return false;
    }
    brain.threat.add(attacker, threat);
    brain.last_player_ms = now_ms;
    if matches!(brain.state, MobState::Asleep | MobState::Idle) {
        brain.target = Some(attacker);
        brain.last_step_ms.get_or_insert(now_ms);
        brain.enter(MobState::Chase, now_ms);
    }
    true
}

/// The mob died at `now_ms`.
pub fn died(brain: &mut Brain, now_ms: u64) {
    brain.threat.clear();
    brain.target = None;
    brain.enter(MobState::Dead, now_ms);
}

/// The mob forgets `entity` (it left, died or handed off).
pub fn forget(brain: &mut Brain, entity: u64) {
    brain.threat.forget(entity);
    if brain.target == Some(entity) {
        brain.target = None;
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_held_mob_steps_only_the_time_after_it_stood() {
        let mut brain = Brain::new(
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            20.0,
        );
        brain.stood_until(5_000);
        assert_eq!(
            brain.last_step_ms, None,
            "a mob that never stepped has no clock"
        );
        brain.last_step_ms = Some(1_000);
        brain.stood_until(5_000);
        assert_eq!(brain.last_step_ms, Some(5_000));
    }

    const SPAWN: Position = Position {
        x: 1000.0,
        y: 1000.0,
        z: 0.0,
    };
    const PLAYER: u64 = 7;
    const OTHER: u64 = 8;
    const SEED: u64 = 3;
    const TICK_MS: u64 = 33;
    const SIGHT_M: f32 = 20.0;
    const NEAR_ZERO_M: f32 = 0.01;

    fn settings() -> BrainSettings {
        BrainSettings {
            wake_radius_m: 100.0,
            sleep_after_ms: 30_000,
            base_sight_m: SIGHT_M,
            wander_radius_m: 10.0,
            wander_pause_ms: 5_000,
            leash_radius_m: 50.0,
            attack_reach_m: 3.0,
            mob_attack_interval_ms: 2_000,
            mob_speed_mps: 3.25,
            gait: Gait::uniform(3.0, 3.25),
            mob_respawn_ms: 360_000,
        }
    }

    /// A sailor-like gait: swings at 1.8 m, runs beyond 4 m, walks 1.25 m/s, wanders 0.6 m/s.
    fn paced() -> BrainSettings {
        BrainSettings {
            gait: Gait {
                swing_start_m: 1.8,
                run_beyond_m: 4.0,
                walk_speed_mps: 1.25,
                wander_speed_mps: 0.6,
            },
            ..settings()
        }
    }

    #[test]
    fn a_paced_mob_runs_far_walks_near_and_swings_close() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        let step_m = |speed_mps: f32| speed_mps * TICK_MS as f32 / 1000.0;
        let moved = |brain: &mut Brain, player: Position, now: u64, dice: &mut Dice| {
            let before = brain.position();
            match step(brain, &[seen(PLAYER, player)], now, &paced(), dice) {
                BrainAction::MoveTo(to) => to.horizontal_distance(before),
                other => panic!("moves: {other:?}"),
            }
        };
        let far = moved(&mut brain, east(10.0), TICK_MS, &mut dice);
        assert!(
            (far - step_m(settings().mob_speed_mps)).abs() < NEAR_ZERO_M,
            "runs: {far}"
        );
        let near = brain.position().x + 3.0;
        let player = Position { x: near, ..SPAWN };
        let walked = moved(&mut brain, player, 2 * TICK_MS, &mut dice);
        assert!(
            (walked - step_m(1.25)).abs() < NEAR_ZERO_M,
            "walks: {walked}"
        );
        let mut now = 3 * TICK_MS;
        let mut swung = false;
        while now < 10_000 && !swung {
            swung = step(
                &mut brain,
                &[seen(PLAYER, player)],
                now,
                &paced(),
                &mut dice,
            ) == BrainAction::Attack(PLAYER);
            now += TICK_MS;
        }
        assert!(swung, "swings once in reach");
        let gap = brain.position().horizontal_distance(player);
        assert!(
            gap <= 1.8 && gap > 1.6,
            "stops inside its swing distance: {gap}"
        );
    }

    #[test]
    fn a_paced_mob_wanders_slowly() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        let view = [seen(PLAYER, east(90.0))];
        let mut now = TICK_MS;
        while now <= 20_000 {
            let before = brain.position();
            if let BrainAction::MoveTo(to) = step(&mut brain, &view, now, &paced(), &mut dice) {
                let speed_mps = to.horizontal_distance(before) * 1000.0 / TICK_MS as f32;
                assert!(speed_mps <= 0.6 + NEAR_ZERO_M, "wanders at {speed_mps} m/s");
            }
            now += TICK_MS;
        }
    }

    fn east(metres: f32) -> Position {
        Position {
            x: SPAWN.x + metres,
            ..SPAWN
        }
    }

    fn seen(entity: u64, position: Position) -> Seen {
        Seen { entity, position }
    }

    /// Steps every tick from `from` to `to` with the same view; returns the actions.
    fn run(
        brain: &mut Brain,
        view: &[Seen],
        from: u64,
        to: u64,
        dice: &mut Dice,
    ) -> Vec<BrainAction> {
        (from..=to)
            .step_by(TICK_MS as usize)
            .map(|now| step(brain, view, now, &settings(), dice))
            .collect()
    }

    fn awake(now: u64) -> Brain {
        let mut brain = Brain::new(SPAWN, SIGHT_M);
        step(
            &mut brain,
            &[seen(PLAYER, east(99.0))],
            now,
            &settings(),
            &mut Dice::new(SEED),
        );
        assert_eq!(brain.state(), MobState::Idle);
        brain
    }

    #[test]
    fn an_asleep_mob_wakes_when_a_player_is_near() {
        let mut dice = Dice::new(SEED);
        let mut brain = Brain::new(SPAWN, SIGHT_M);
        assert_eq!(brain.state(), MobState::Asleep);
        step(
            &mut brain,
            &[seen(PLAYER, east(100.5))],
            0,
            &settings(),
            &mut dice,
        );
        assert_eq!(brain.state(), MobState::Asleep);
        step(
            &mut brain,
            &[seen(PLAYER, east(99.5))],
            TICK_MS,
            &settings(),
            &mut dice,
        );
        assert_eq!(brain.state(), MobState::Idle);
    }

    #[test]
    fn an_idle_mob_wanders_inside_its_radius_and_pauses() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        let view = [seen(PLAYER, east(90.0))];
        let early = run(&mut brain, &view, TICK_MS, 4_900, &mut dice);
        assert!(
            early.iter().all(|action| *action == BrainAction::None),
            "pauses first"
        );
        let later = run(&mut brain, &view, 5_000, 20_000, &mut dice);
        assert!(
            later
                .iter()
                .any(|action| matches!(action, BrainAction::MoveTo(_)))
        );
        for action in later {
            if let BrainAction::MoveTo(to) = action {
                assert!(to.horizontal_distance(SPAWN) <= settings().wander_radius_m + NEAR_ZERO_M);
            }
        }
    }

    fn north(metres: f32) -> Position {
        Position {
            y: SPAWN.y + metres,
            ..SPAWN
        }
    }

    /// Steps every tick from `from` to `to` with the same view and `settings`.
    fn run_with(
        brain: &mut Brain,
        view: &[Seen],
        from: u64,
        to: u64,
        settings: &BrainSettings,
    ) -> Vec<BrainAction> {
        let mut dice = Dice::new(SEED);
        (from..=to)
            .step_by(TICK_MS as usize)
            .map(|now| step(brain, view, now, settings, &mut dice))
            .collect()
    }

    #[test]
    fn a_guard_stands_still_and_attacks_on_sight() {
        let mut brain = awake(0);
        brain.set_behaviour(Behaviour::Guard);
        let far = [seen(PLAYER, east(90.0))];
        let actions = run_with(&mut brain, &far, TICK_MS, 20_000, &settings());
        assert!(
            actions.iter().all(|action| *action == BrainAction::None),
            "no wander"
        );
        assert_eq!(brain.position(), SPAWN);
        let near = [seen(PLAYER, east(10.0))];
        run_with(&mut brain, &near, 20_033, 25_000, &settings());
        assert!(matches!(brain.state(), MobState::Chase | MobState::Attack));
    }

    #[test]
    fn an_idle_mob_ignores_a_player_in_sight_but_fights_back() {
        let mut brain = awake(0);
        brain.set_behaviour(Behaviour::Idle);
        let near = [seen(PLAYER, east(5.0))];
        let actions = run_with(&mut brain, &near, TICK_MS, 10_000, &settings());
        assert!(actions.iter().all(|action| *action == BrainAction::None));
        assert_eq!(brain.state(), MobState::Idle);
        assert!(hurt(&mut brain, PLAYER, 1, 10_033));
        run_with(&mut brain, &near, 10_066, 12_000, &settings());
        assert!(matches!(brain.state(), MobState::Chase | MobState::Attack));
    }

    #[test]
    fn a_patrol_walks_a_route_longer_than_its_leash_and_loops() {
        const LEG_M: f32 = 120.0;
        let mut brain = awake(0);
        brain.set_behaviour(Behaviour::Patrol);
        brain.set_route(Route {
            points: vec![east(LEG_M), SPAWN],
            waits_ms: vec![1_000, 0],
            mode: RouteMode::Loop,
        });
        // A player near enough all the way, so the mob stays awake.
        let wide = BrainSettings {
            wake_radius_m: 1_000.0,
            ..settings()
        };
        let view = [seen(PLAYER, north(95.0))];
        let mut far_m: f32 = 0.0;
        let mut back = false;
        let mut dice = Dice::new(SEED);
        let mut now = TICK_MS;
        while now < 200_000 {
            step(&mut brain, &view, now, &wide, &mut dice);
            let from_spawn = brain.position().horizontal_distance(SPAWN);
            far_m = far_m.max(from_spawn);
            if far_m > LEG_M - 1.0 && from_spawn < 1.0 {
                back = true;
                break;
            }
            now += TICK_MS;
        }
        assert!(
            far_m > settings().leash_radius_m,
            "it walked past the leash radius"
        );
        assert!(back, "it looped back to the first point");
    }

    #[test]
    fn a_patrol_wakes_and_walks_with_no_player_near() {
        const LEG_M: f32 = 30.0;
        let mut brain = Brain::new(SPAWN, SIGHT_M);
        brain.set_behaviour(Behaviour::Patrol);
        brain.set_route(Route {
            points: vec![east(LEG_M)],
            waits_ms: Vec::new(),
            mode: RouteMode::Once,
        });
        let mut dice = Dice::new(SEED);
        let quiet_ms = settings().sleep_after_ms * 3;
        run(&mut brain, &[], 0, quiet_ms, &mut dice);
        assert_ne!(brain.state(), MobState::Asleep, "a patrol does not sleep");
        assert!(
            brain.position().horizontal_distance(east(LEG_M)) < ARRIVED_M,
            "it walked its route alone: {:?}",
            brain.position()
        );
    }

    #[test]
    fn a_patrol_back_and_forth_and_once() {
        let mut brain = awake(0);
        brain.set_behaviour(Behaviour::Patrol);
        brain.set_route(Route {
            points: vec![east(10.0), east(20.0)],
            waits_ms: Vec::new(),
            mode: RouteMode::Once,
        });
        let view = [seen(PLAYER, north(95.0))];
        run_with(&mut brain, &view, TICK_MS, 30_000, &settings());
        assert!(
            brain.position().horizontal_distance(east(20.0)) < 0.1,
            "once: stays at the end"
        );
        let mut brain = awake(0);
        brain.set_behaviour(Behaviour::Patrol);
        brain.set_route(Route {
            points: vec![east(10.0), east(20.0)],
            waits_ms: Vec::new(),
            mode: RouteMode::BackAndForth,
        });
        let mut dice = Dice::new(SEED);
        let mut visits_first_again = false;
        let mut reached_end = false;
        for tick in 1..2_000 {
            step(&mut brain, &view, tick * TICK_MS, &settings(), &mut dice);
            let at = brain.position();
            reached_end |= at.horizontal_distance(east(20.0)) < 0.1;
            if reached_end && at.horizontal_distance(east(10.0)) < 0.1 {
                visits_first_again = true;
                break;
            }
        }
        assert!(
            visits_first_again,
            "back and forth: it comes back to the first point"
        );
    }

    #[test]
    fn a_patrol_returns_to_where_it_left_and_respawns_at_home() {
        let mut brain = awake(0);
        brain.set_behaviour(Behaviour::Patrol);
        brain.set_route(Route {
            points: vec![east(100.0)],
            waits_ms: Vec::new(),
            mode: RouteMode::Once,
        });
        let quiet = [seen(PLAYER, north(95.0))];
        run_with(&mut brain, &quiet, TICK_MS, 20_000, &settings());
        let left_at = brain.position();
        assert!(left_at.horizontal_distance(SPAWN) > 50.0);
        let near = Position {
            y: left_at.y + 8.0,
            ..left_at
        };
        run_with(
            &mut brain,
            &[seen(PLAYER, near)],
            20_033,
            21_000,
            &settings(),
        );
        assert!(matches!(brain.state(), MobState::Chase | MobState::Attack));
        run_with(&mut brain, &quiet, 21_033, 60_000, &settings());
        assert!(
            brain.position().horizontal_distance(left_at) < 30.0
                || brain.position().horizontal_distance(east(100.0)) < 0.1
        );
        died(&mut brain, 60_033);
        run_with(
            &mut brain,
            &quiet,
            60_066,
            60_066 + settings().mob_respawn_ms,
            &settings(),
        );
        assert_eq!(brain.position(), SPAWN, "it respawns at its spawn");
    }

    #[test]
    fn an_idle_mob_sleeps_after_quiet_time() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        step(&mut brain, &[], 29_999, &settings(), &mut dice);
        assert_eq!(brain.state(), MobState::Idle);
        step(&mut brain, &[], 30_000, &settings(), &mut dice);
        assert_eq!(brain.state(), MobState::Asleep);
    }

    #[test]
    fn a_mob_chases_a_player_in_sight() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        step(
            &mut brain,
            &[seen(PLAYER, east(20.5))],
            TICK_MS,
            &settings(),
            &mut dice,
        );
        assert_eq!(brain.state(), MobState::Idle);
        step(
            &mut brain,
            &[seen(PLAYER, east(19.5))],
            2 * TICK_MS,
            &settings(),
            &mut dice,
        );
        assert_eq!(brain.state(), MobState::Chase);
        assert_eq!(brain.target(), Some(PLAYER));
        let before = brain.position();
        let next = step(
            &mut brain,
            &[seen(PLAYER, east(19.5))],
            3 * TICK_MS,
            &settings(),
            &mut dice,
        );
        let BrainAction::MoveTo(to) = next else {
            panic!("chases: {next:?}");
        };
        let step_m = settings().mob_speed_mps * TICK_MS as f32 / 1000.0;
        assert!(
            (to.x - before.x - step_m).abs() < NEAR_ZERO_M,
            "{before:?} to {to:?}"
        );
    }

    #[test]
    fn a_mob_attacks_in_reach_at_its_interval() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        let view = [seen(PLAYER, east(2.9))];
        let actions = run(&mut brain, &view, TICK_MS, 4_100, &mut dice);
        let attacks = actions
            .iter()
            .filter(|action| **action == BrainAction::Attack(PLAYER))
            .count();
        assert_eq!(attacks, 3, "at 0 s, 2 s and 4 s of the fight");
        assert_eq!(brain.state(), MobState::Attack);
        step(
            &mut brain,
            &[seen(PLAYER, east(3.5))],
            4_200,
            &settings(),
            &mut dice,
        );
        assert_eq!(brain.state(), MobState::Chase);
    }

    #[test]
    fn a_mob_past_its_leash_returns_and_heals() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        assert!(hurt(&mut brain, PLAYER, 10, TICK_MS));
        let reach = settings().leash_radius_m + settings().attack_reach_m;
        step(
            &mut brain,
            &[seen(PLAYER, east(reach - 0.5))],
            2 * TICK_MS,
            &settings(),
            &mut dice,
        );
        assert_eq!(brain.state(), MobState::Chase);
        step(
            &mut brain,
            &[seen(PLAYER, east(reach + 0.5))],
            3 * TICK_MS,
            &settings(),
            &mut dice,
        );
        assert_eq!(brain.state(), MobState::Return);
        let actions = run(&mut brain, &[], 4 * TICK_MS, 4_000, &mut dice);
        assert!(actions.contains(&BrainAction::Home));
        assert_eq!(brain.state(), MobState::Idle);
        assert!(brain.position().horizontal_distance(SPAWN) < NEAR_ZERO_M);
        assert_eq!(brain.target(), None);
    }

    #[test]
    fn a_returning_mob_takes_no_hurt() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        hurt(&mut brain, PLAYER, 10, TICK_MS);
        step(&mut brain, &[], 2 * TICK_MS, &settings(), &mut dice);
        assert_eq!(brain.state(), MobState::Return);
        assert!(!hurt(&mut brain, PLAYER, 10, 3 * TICK_MS));
        assert_eq!(brain.state(), MobState::Return);
    }

    #[test]
    fn a_dead_mob_respawns_after_its_timer() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        hurt(&mut brain, PLAYER, 10, TICK_MS);
        died(&mut brain, 1_000);
        assert_eq!(brain.state(), MobState::Dead);
        assert!(!hurt(&mut brain, PLAYER, 10, 1_100));
        let view = [seen(PLAYER, east(1.0))];
        assert_eq!(
            step(&mut brain, &view, 360_999, &settings(), &mut dice),
            BrainAction::None
        );
        assert_eq!(brain.state(), MobState::Dead);
        assert_eq!(
            step(&mut brain, &view, 361_000, &settings(), &mut dice),
            BrainAction::Respawned
        );
        assert_eq!(brain.state(), MobState::Idle);
        assert!(brain.position().horizontal_distance(SPAWN) < NEAR_ZERO_M);
    }

    #[test]
    fn a_hurt_mob_chases_its_attacker() {
        let mut dice = Dice::new(SEED);
        let mut brain = Brain::new(SPAWN, SIGHT_M);
        assert!(hurt(&mut brain, OTHER, 5, 0));
        assert_eq!(brain.state(), MobState::Chase);
        let view = [seen(PLAYER, east(5.0)), seen(OTHER, east(30.0))];
        step(&mut brain, &view, TICK_MS, &settings(), &mut dice);
        assert_eq!(brain.target(), Some(OTHER), "threat beats distance");
    }

    #[test]
    fn a_mob_forgets_a_player_who_left() {
        let mut dice = Dice::new(SEED);
        let mut brain = awake(0);
        hurt(&mut brain, PLAYER, 10, TICK_MS);
        step(
            &mut brain,
            &[seen(PLAYER, east(10.0))],
            2 * TICK_MS,
            &settings(),
            &mut dice,
        );
        assert_eq!(brain.target(), Some(PLAYER));
        forget(&mut brain, PLAYER);
        step(&mut brain, &[], 3 * TICK_MS, &settings(), &mut dice);
        assert_eq!(brain.state(), MobState::Return);
        assert_eq!(brain.target(), None);
    }

    proptest! {
        #[test]
        fn a_mob_never_leaves_its_leash_circle(
            path in prop::collection::vec((-200.0_f32..200.0, -200.0_f32..200.0, any::<bool>()), 1..120),
        ) {
            let mut dice = Dice::new(SEED);
            let mut brain = Brain::new(SPAWN, SIGHT_M);
            for (tick, (dx, dy, hit)) in path.into_iter().enumerate() {
                let now = tick as u64 * TICK_MS;
                let player = Position { x: SPAWN.x + dx, y: SPAWN.y + dy, z: 0.0 };
                if hit {
                    hurt(&mut brain, PLAYER, 1, now);
                }
                step(&mut brain, &[seen(PLAYER, player)], now, &settings(), &mut dice);
                prop_assert!(
                    brain.position().horizontal_distance(SPAWN)
                        <= settings().leash_radius_m + NEAR_ZERO_M
                );
            }
        }
    }
}
