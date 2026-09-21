// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mob state summary, and the chase brain of the mobs whose behavior tree Server.pak does not
//! have (behavior-tree study, section 4.1: the FTUE club sailor `drowned_qte_basebehavior`, the
//! FTUE legionnaire `legion_ftue_basebehavior` and the tutorial Thorpe
//! `ftue_thorpe_v2_basebehavior` are in no Server.pak). Every other mob runs its own tree
//! (`nw_ai`).
//!
//! The chase brain is a pure state machine with the paces measured from the retail tutorial
//! capture: the caller passes the time and the players near the mob, applies the returned action
//! and puts the mob on the ground. A step first changes the state, then acts in the new state, so
//! a mob that sees a player in reach attacks at once. Without a target it stands at its spawn
//! (retail: the tutorial legionnaire stood at its spawn until it saw the player; live
//! 2026-09-18, a wandering legionnaire walked into the ruin wall). Every move is capped by the
//! mob's speed and kept inside the leash circle around the spawn.

use crate::combat::Threat;
use crate::movement::Position;

const MILLIS_PER_SECOND: f32 = 1000.0;
/// Longest time one step may move a mob, so a stalled tick never makes it jump (our engineering
/// limit).
const MAX_STEP_MS: u64 = 1000;
/// Distance at which a walking mob counts as arrived (our engineering limit: well under a
/// step).
const ARRIVED_M: f32 = 0.05;

/// How a chasing mob moves and when it swings (tutorial-ai-behavior note, section 8; ship-mobs
/// note, section 2: retail sailors started swings at 0.89-1.81 m, ran only to close a gap of 4-12 m
/// and walked or stood nearer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gait {
    /// It starts a swing at a target this near.
    pub swing_start_m: f32,
    /// A fighting mob farther than this from its target runs at its run speed; nearer, it walks.
    pub run_beyond_m: f32,
    /// Its speed while it walks to its target.
    pub walk_speed_mps: f32,
}

/// The numbers of a chasing mob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChaseSettings {
    /// It never goes farther from its spawn: its slice's `CombatLeashDistance`.
    pub leash_radius_m: f32,
    /// Time between two of its swings.
    pub attack_interval_ms: u64,
    /// Its run speed.
    pub run_speed_mps: f32,
    /// How it moves and when it swings.
    pub gait: Gait,
}

/// The state of a mob, for the admin snapshot and the metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MobState {
    /// No player near; costs nothing.
    Asleep,
    /// Awake without a target.
    Idle,
    /// Moves to its target or fights it between attacks.
    Chase,
    /// Plays an attack.
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
    /// The mob is back at its spawn after a return: it stands there; refill its health.
    Home,
}

/// The state of a chasing mob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChaseState {
    /// It stands at its spawn.
    Idle,
    /// It walks to its target.
    Chase,
    /// It hits its target.
    Attack,
    /// It walks back to its spawn.
    Return,
    /// It died.
    Dead,
}

/// One chasing mob's mind.
#[derive(Debug, Clone, PartialEq)]
pub struct ChaseBrain {
    state: ChaseState,
    spawn: Position,
    sight_m: f32,
    target: Option<u64>,
    threat: Threat,
    last_step_ms: Option<u64>,
    last_attack_ms: Option<u64>,
}

/// Walks from `from` up to `max_m` toward `to`, inside the circle of `leash_m` around `spawn`;
/// the point reached, at the height of `from`.
pub fn walk_within(
    from: Position,
    to: Position,
    max_m: f32,
    spawn: Position,
    leash_m: f32,
) -> Position {
    let distance = from.horizontal_distance(to);
    let mut next = if distance <= max_m {
        Position { z: from.z, ..to }
    } else {
        let share = max_m / distance;
        Position {
            x: from.x + (to.x - from.x) * share,
            y: from.y + (to.y - from.y) * share,
            z: from.z,
        }
    };
    let from_spawn = next.horizontal_distance(spawn);
    if from_spawn > leash_m {
        let share = leash_m / from_spawn;
        next.x = spawn.x + (next.x - spawn.x) * share;
        next.y = spawn.y + (next.y - spawn.y) * share;
    }
    next
}

impl ChaseBrain {
    /// A mob that stands at `spawn` and sees `sight_m` far.
    pub fn new(spawn: Position, sight_m: f32) -> Self {
        Self {
            state: ChaseState::Idle,
            spawn,
            sight_m,
            target: None,
            threat: Threat::default(),
            last_step_ms: None,
            last_attack_ms: None,
        }
    }

    /// The mob stood still until `now_ms` (held by a script): its next step moves only the time
    /// after it, not the time it stood.
    pub fn stood_until(&mut self, now_ms: u64) {
        if self.last_step_ms.is_some() {
            self.last_step_ms = Some(now_ms);
        }
    }

    /// The state, as the snapshot shows it.
    pub const fn state(&self) -> MobState {
        match self.state {
            ChaseState::Idle => MobState::Idle,
            ChaseState::Chase => MobState::Chase,
            ChaseState::Attack => MobState::Attack,
            ChaseState::Return => MobState::Return,
            ChaseState::Dead => MobState::Dead,
        }
    }

    /// Whether it walks back to its spawn.
    pub const fn returning(&self) -> bool {
        matches!(self.state, ChaseState::Return)
    }

    /// Whether it chases or hits a target.
    pub const fn fighting(&self) -> bool {
        matches!(self.state, ChaseState::Chase | ChaseState::Attack)
    }

    /// The entity it chases or hits.
    pub const fn target(&self) -> Option<u64> {
        self.target
    }

    /// Its next hit comes one attack interval after `now_ms` at the earliest.
    pub const fn wait_to_attack(&mut self, now_ms: u64) {
        self.last_attack_ms = Some(now_ms);
    }

    fn enter(&mut self, state: ChaseState) {
        self.state = state;
    }

    /// The target to chase now: the top threat that is still near, else the nearest player in
    /// sight. Threat of an attacker that is not near is dropped.
    fn choose_target(&mut self, at: Position, seen: &[Seen]) -> Option<Seen> {
        while let Some(top) = self.threat.top() {
            if let Some(found) = seen.iter().find(|player| player.entity == top) {
                return Some(*found);
            }
            self.threat.forget(top);
        }
        seen.iter()
            .filter(|player| player.position.horizontal_distance(at) <= self.sight_m)
            .min_by(|a, b| {
                let da = a.position.horizontal_distance(at);
                let db = b.position.horizontal_distance(at);
                da.total_cmp(&db).then(a.entity.cmp(&b.entity))
            })
            .copied()
    }

    /// One step at `now_ms` of the mob at `at`, with the living players near it.
    pub fn step(
        &mut self,
        at: Position,
        seen: &[Seen],
        now_ms: u64,
        settings: &ChaseSettings,
    ) -> BrainAction {
        let elapsed_ms = self
            .last_step_ms
            .map_or(0, |last| now_ms.saturating_sub(last).min(MAX_STEP_MS));
        self.last_step_ms = Some(now_ms);
        // A step of a few metres at most.
        #[allow(clippy::cast_precision_loss, reason = "at most MAX_STEP_MS")]
        let reach = |speed_mps: f32| speed_mps * elapsed_ms as f32 / MILLIS_PER_SECOND;
        let leash_m = settings.leash_radius_m;
        match self.state {
            ChaseState::Dead => BrainAction::None,
            ChaseState::Return => {
                let to = walk_within(
                    at,
                    self.spawn,
                    reach(settings.run_speed_mps),
                    self.spawn,
                    leash_m,
                );
                if to.horizontal_distance(self.spawn) > ARRIVED_M {
                    return BrainAction::MoveTo(to);
                }
                self.threat.clear();
                self.target = None;
                self.enter(ChaseState::Idle);
                BrainAction::Home
            }
            ChaseState::Idle | ChaseState::Chase | ChaseState::Attack => {
                self.fight(at, seen, now_ms, &reach, settings)
            }
        }
    }

    /// The awake states. `reach` gives the metres of this step at a speed.
    fn fight(
        &mut self,
        at: Position,
        seen: &[Seen],
        now_ms: u64,
        reach: &dyn Fn(f32) -> f32,
        settings: &ChaseSettings,
    ) -> BrainAction {
        let Some(target) = self.choose_target(at, seen) else {
            self.target = None;
            if self.state != ChaseState::Idle {
                self.enter(ChaseState::Return);
            }
            return BrainAction::None;
        };
        self.target = Some(target.entity);
        let gait = settings.gait;
        if target.position.horizontal_distance(self.spawn)
            > settings.leash_radius_m + gait.swing_start_m
        {
            self.threat.forget(target.entity);
            self.target = None;
            self.enter(ChaseState::Return);
            return BrainAction::None;
        }
        let distance = target.position.horizontal_distance(at);
        if distance > gait.swing_start_m {
            self.enter(ChaseState::Chase);
            let speed_mps = if distance > gait.run_beyond_m {
                settings.run_speed_mps
            } else {
                gait.walk_speed_mps
            };
            // It stops just inside its swing distance, not on the target.
            let short_m = (distance - gait.swing_start_m + ARRIVED_M).max(0.0);
            let max_m = reach(speed_mps).min(short_m);
            return BrainAction::MoveTo(walk_within(
                at,
                target.position,
                max_m,
                self.spawn,
                settings.leash_radius_m,
            ));
        }
        self.enter(ChaseState::Attack);
        let ready = self
            .last_attack_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= settings.attack_interval_ms);
        if !ready {
            return BrainAction::None;
        }
        self.last_attack_ms = Some(now_ms);
        BrainAction::Attack(target.entity)
    }

    /// `attacker` hit the mob, adding `threat`. A mob that returns or is dead ignores it and
    /// returns `false`; any other mob chases.
    pub fn hurt(&mut self, attacker: u64, threat: u64, now_ms: u64) -> bool {
        if matches!(self.state, ChaseState::Return | ChaseState::Dead) {
            return false;
        }
        self.threat.add(attacker, threat);
        if self.state == ChaseState::Idle {
            self.target = Some(attacker);
            self.last_step_ms.get_or_insert(now_ms);
            self.enter(ChaseState::Chase);
        }
        true
    }

    /// The mob died.
    pub fn died(&mut self) {
        self.threat.clear();
        self.target = None;
        self.enter(ChaseState::Dead);
    }

    /// The mob forgets `entity` (it left, died or handed off).
    pub fn forget(&mut self, entity: u64) {
        self.threat.forget(entity);
        if self.target == Some(entity) {
            self.target = None;
        }
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
    const TICK_MS: u64 = 33;
    const SIGHT_M: f32 = 20.0;
    const NEAR_ZERO_M: f32 = 0.01;
    const RUN_MPS: f32 = 3.25;
    const REACH_M: f32 = 3.0;

    fn settings() -> ChaseSettings {
        ChaseSettings {
            leash_radius_m: 50.0,
            attack_interval_ms: 2_000,
            run_speed_mps: RUN_MPS,
            gait: Gait {
                swing_start_m: REACH_M,
                run_beyond_m: REACH_M,
                walk_speed_mps: RUN_MPS,
            },
        }
    }

    /// A sailor-like gait: swings at 1.8 m, runs beyond 4 m, walks 1.25 m/s.
    fn paced() -> ChaseSettings {
        ChaseSettings {
            gait: Gait {
                swing_start_m: 1.8,
                run_beyond_m: 4.0,
                walk_speed_mps: 1.25,
            },
            ..settings()
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

    /// A mob and where it stands.
    struct Mob {
        brain: ChaseBrain,
        at: Position,
    }

    impl Mob {
        fn new() -> Self {
            Self {
                brain: ChaseBrain::new(SPAWN, SIGHT_M),
                at: SPAWN,
            }
        }

        fn step(&mut self, view: &[Seen], now: u64, settings: &ChaseSettings) -> BrainAction {
            let action = self.brain.step(self.at, view, now, settings);
            match action {
                BrainAction::MoveTo(to) => self.at = to,
                BrainAction::Home => self.at = SPAWN,
                BrainAction::None | BrainAction::Attack(_) => {}
            }
            action
        }

        fn run(&mut self, view: &[Seen], from: u64, to: u64) -> Vec<BrainAction> {
            (from..=to)
                .step_by(TICK_MS as usize)
                .map(|now| self.step(view, now, &settings()))
                .collect()
        }
    }

    #[test]
    fn a_held_mob_steps_only_the_time_after_it_stood() {
        let mut brain = ChaseBrain::new(SPAWN, SIGHT_M);
        brain.stood_until(5_000);
        assert_eq!(
            brain.last_step_ms, None,
            "a mob that never stepped has no clock"
        );
        brain.last_step_ms = Some(1_000);
        brain.stood_until(5_000);
        assert_eq!(brain.last_step_ms, Some(5_000));
    }

    #[test]
    fn a_mob_without_a_target_stands_at_its_spawn() {
        let mut mob = Mob::new();
        let actions = mob.run(&[seen(PLAYER, east(90.0))], 0, 60_000);
        assert!(actions.iter().all(|action| *action == BrainAction::None));
        assert_eq!(mob.at, SPAWN);
        assert_eq!(mob.brain.state(), MobState::Idle);
    }

    #[test]
    fn a_paced_mob_runs_far_walks_near_and_swings_close() {
        let mut mob = Mob::new();
        let step_m = |speed_mps: f32| speed_mps * TICK_MS as f32 / 1000.0;
        let moved = |mob: &mut Mob, player: Position, now: u64| {
            let before = mob.at;
            match mob.step(&[seen(PLAYER, player)], now, &paced()) {
                BrainAction::MoveTo(to) => to.horizontal_distance(before),
                other => panic!("moves: {other:?}"),
            }
        };
        mob.step(&[seen(PLAYER, east(10.0))], 0, &paced());
        let far = moved(&mut mob, east(10.0), TICK_MS);
        assert!((far - step_m(RUN_MPS)).abs() < NEAR_ZERO_M, "runs: {far}");
        let player = east(mob.at.x - SPAWN.x + 3.0);
        let walked = moved(&mut mob, player, 2 * TICK_MS);
        assert!(
            (walked - step_m(1.25)).abs() < NEAR_ZERO_M,
            "walks: {walked}"
        );
        let mut now = 3 * TICK_MS;
        let mut swung = false;
        while now < 10_000 && !swung {
            swung = mob.step(&[seen(PLAYER, player)], now, &paced()) == BrainAction::Attack(PLAYER);
            now += TICK_MS;
        }
        assert!(swung, "swings once in reach");
        let gap = mob.at.horizontal_distance(player);
        assert!(
            gap <= 1.8 && gap > 1.6,
            "stops inside its swing distance: {gap}"
        );
    }

    #[test]
    fn a_mob_chases_a_player_in_sight() {
        let mut mob = Mob::new();
        mob.step(&[seen(PLAYER, east(20.5))], 0, &settings());
        assert_eq!(mob.brain.state(), MobState::Idle);
        mob.step(&[seen(PLAYER, east(19.5))], TICK_MS, &settings());
        assert_eq!(mob.brain.state(), MobState::Chase);
        assert_eq!(mob.brain.target(), Some(PLAYER));
        let before = mob.at;
        let next = mob.step(&[seen(PLAYER, east(19.5))], 2 * TICK_MS, &settings());
        let BrainAction::MoveTo(to) = next else {
            panic!("chases: {next:?}");
        };
        let step_m = RUN_MPS * TICK_MS as f32 / 1000.0;
        assert!(
            (to.x - before.x - step_m).abs() < NEAR_ZERO_M,
            "{before:?} to {to:?}"
        );
    }

    #[test]
    fn a_mob_attacks_in_reach_at_its_interval() {
        let mut mob = Mob::new();
        let actions = mob.run(&[seen(PLAYER, east(2.9))], 0, 4_100);
        let attacks = actions
            .iter()
            .filter(|action| **action == BrainAction::Attack(PLAYER))
            .count();
        assert_eq!(attacks, 3, "at 0 s, 2 s and 4 s of the fight");
        assert_eq!(mob.brain.state(), MobState::Attack);
        mob.step(&[seen(PLAYER, east(3.5))], 4_200, &settings());
        assert_eq!(mob.brain.state(), MobState::Chase);
    }

    #[test]
    fn a_mob_past_its_leash_returns_and_heals() {
        let mut mob = Mob::new();
        assert!(mob.brain.hurt(PLAYER, 10, TICK_MS));
        let reach = settings().leash_radius_m + REACH_M;
        mob.step(&[seen(PLAYER, east(reach - 0.5))], 2 * TICK_MS, &settings());
        assert_eq!(mob.brain.state(), MobState::Chase);
        mob.step(&[seen(PLAYER, east(reach + 0.5))], 3 * TICK_MS, &settings());
        assert_eq!(mob.brain.state(), MobState::Return);
        assert!(mob.brain.returning());
        let actions = mob.run(&[], 4 * TICK_MS, 4_000);
        assert!(actions.contains(&BrainAction::Home));
        assert_eq!(mob.brain.state(), MobState::Idle);
        assert!(mob.at.horizontal_distance(SPAWN) < NEAR_ZERO_M);
        assert_eq!(mob.brain.target(), None);
    }

    #[test]
    fn a_returning_or_dead_mob_takes_no_hurt() {
        let mut mob = Mob::new();
        mob.brain.hurt(PLAYER, 10, TICK_MS);
        mob.step(&[], 2 * TICK_MS, &settings());
        assert_eq!(mob.brain.state(), MobState::Return);
        assert!(!mob.brain.hurt(PLAYER, 10, 3 * TICK_MS));
        mob.brain.died();
        assert_eq!(mob.brain.state(), MobState::Dead);
        assert!(!mob.brain.hurt(PLAYER, 10, 4 * TICK_MS));
        assert_eq!(
            mob.step(&[seen(PLAYER, east(1.0))], 5 * TICK_MS, &settings()),
            BrainAction::None
        );
    }

    #[test]
    fn a_hurt_mob_chases_its_attacker() {
        let mut mob = Mob::new();
        assert!(mob.brain.hurt(OTHER, 5, 0));
        assert_eq!(mob.brain.state(), MobState::Chase);
        let view = [seen(PLAYER, east(5.0)), seen(OTHER, east(30.0))];
        mob.step(&view, TICK_MS, &settings());
        assert_eq!(mob.brain.target(), Some(OTHER), "threat beats distance");
    }

    #[test]
    fn a_mob_forgets_a_player_who_left() {
        let mut mob = Mob::new();
        mob.brain.hurt(PLAYER, 10, TICK_MS);
        mob.step(&[seen(PLAYER, east(10.0))], 2 * TICK_MS, &settings());
        assert_eq!(mob.brain.target(), Some(PLAYER));
        mob.brain.forget(PLAYER);
        mob.step(&[], 3 * TICK_MS, &settings());
        assert_eq!(mob.brain.state(), MobState::Return);
        assert_eq!(mob.brain.target(), None);
    }

    proptest! {
        #[test]
        fn a_mob_never_leaves_its_leash_circle(
            path in prop::collection::vec((-200.0_f32..200.0, -200.0_f32..200.0, any::<bool>()), 1..120),
        ) {
            let mut mob = Mob::new();
            for (tick, (dx, dy, hit)) in path.into_iter().enumerate() {
                let now = tick as u64 * TICK_MS;
                let player = Position { x: SPAWN.x + dx, y: SPAWN.y + dy, z: 0.0 };
                if hit {
                    mob.brain.hurt(PLAYER, 1, now);
                }
                mob.step(&[seen(PLAYER, player)], now, &settings());
                prop_assert!(
                    mob.at.horizontal_distance(SPAWN) <= settings().leash_radius_m + NEAR_ZERO_M
                );
            }
        }
    }
}
