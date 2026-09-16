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
    /// Walking speed.
    pub mob_speed_mps: f32,
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

/// One mob's mind.
#[derive(Debug, Clone, PartialEq)]
pub struct Brain {
    state: MobState,
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
}

impl Brain {
    /// An asleep mob at `spawn` that sees `sight_m` far.
    pub fn new(spawn: Position, sight_m: f32) -> Self {
        Self {
            state: MobState::Asleep,
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
        }
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
    /// sight. Threat of an attacker that is not near is dropped.
    fn choose_target(&mut self, seen: &[Seen]) -> Option<Seen> {
        while let Some(top) = self.threat.top() {
            if let Some(found) = seen.iter().find(|player| player.entity == top) {
                return Some(*found);
            }
            self.threat.forget(top);
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
    let max_m = settings.mob_speed_mps * elapsed_ms as f32 / MILLIS_PER_SECOND;
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
            brain.position = brain.spawn;
            brain.threat.clear();
            brain.target = None;
            brain.last_player_ms = now_ms;
            brain.enter(MobState::Idle, now_ms);
            BrainAction::Respawned
        }
        MobState::Return => {
            let at = brain.walk(brain.spawn, max_m, settings.leash_radius_m);
            if at.horizontal_distance(brain.spawn) > ARRIVED_M {
                return BrainAction::MoveTo(at);
            }
            brain.threat.clear();
            brain.target = None;
            brain.enter(MobState::Idle, now_ms);
            BrainAction::Home
        }
        MobState::Asleep => {
            if player_near {
                brain.enter(MobState::Idle, now_ms);
            }
            BrainAction::None
        }
        MobState::Idle | MobState::Chase | MobState::Attack => {
            fight(brain, seen, now_ms, max_m, settings, dice)
        }
    }
}

/// The awake states: Idle, Chase and Attack.
fn fight(
    brain: &mut Brain,
    seen: &[Seen],
    now_ms: u64,
    max_m: f32,
    settings: &BrainSettings,
    dice: &mut Dice,
) -> BrainAction {
    let Some(target) = brain.choose_target(seen) else {
        brain.target = None;
        if brain.state != MobState::Idle {
            brain.enter(MobState::Return, now_ms);
            return BrainAction::None;
        }
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
    if target.position.horizontal_distance(brain.position) > settings.attack_reach_m {
        if brain.state != MobState::Chase {
            brain.enter(MobState::Chase, now_ms);
        }
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
    if now_ms.saturating_sub(brain.last_player_ms) >= settings.sleep_after_ms {
        brain.enter(MobState::Asleep, now_ms);
        return BrainAction::None;
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
            mob_respawn_ms: 360_000,
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
