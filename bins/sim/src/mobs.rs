// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mobs of one sim (M6-A spec section 6). A sim spawns the vitals spawns of the world file
//! that lie in its cells; a mob belongs to the sim of its spawn and never hands off, because its
//! leash keeps it near the spawn.
//!
//! Only awake mobs run their brain. Each tick, the mobs within the wake radius of a player wake;
//! an idle mob without a player near falls asleep again (its brain decides when). A grid of the
//! spawn points, in buckets of the wake radius plus the leash radius, finds the mobs near a
//! player, because a mob is never farther than the leash from its spawn. A player scans again
//! only after it moved [`WAKE_RESCAN_M`] since its last scan, or when the cells change: a player
//! who stands still keeps its awake mobs awake without a scan.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use nw_bus::{MOB_ENTITY_BASE, MOB_STATES, MobGhost, SnapMob};
use nw_content::combat::CombatData;
use nw_sim_core::{
    Brain, BrainAction, BrainSettings, Dice, Ground, Health, MobAttack, MobState, Position, Seen,
    SpatialGrid, brain_step, forget, mob_max_health,
};
pub use nw_world::records::VitalsSpawn;

use crate::combat::CombatSettings;

/// A player scans for mobs to wake again after moving this far (about 2 s of running); far less
/// than the wake radius, so a mob wakes before the player can reach it.
pub const WAKE_RESCAN_M: f32 = 10.0;

/// The entity number of the mob of spawn `index`.
pub const fn mob_entity(index: u32) -> u64 {
    MOB_ENTITY_BASE + index as u64
}

/// The spawn index of a mob's entity number; `None` for a player's number.
pub fn spawn_index(entity: u64) -> Option<u32> {
    entity
        .checked_sub(MOB_ENTITY_BASE)
        .and_then(|index| u32::try_from(index).ok())
}

/// One mob.
#[derive(Debug, Clone)]
pub struct Mob {
    /// Its mind and position.
    pub brain: Brain,
    /// Its health.
    pub health: Health,
    /// Its attack.
    pub attack: MobAttack,
}

/// A mob that hits a target this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobAttackIntent {
    /// The mob's entity number.
    pub mob: u64,
    /// The target's entity number.
    pub target: u64,
}

/// What changes of cells did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MobChanges {
    /// Mobs that spawned.
    pub spawned: usize,
    /// Mobs that left with their cell.
    pub removed: usize,
    /// Newly owned spawns without a mob type or level row.
    pub unspawned: usize,
}

/// The mobs of one sim.
pub struct Mobs {
    spawns: Arc<[VitalsSpawn]>,
    data: Arc<CombatData>,
    settings: BrainSettings,
    mob_attack_factor: f64,
    live: HashMap<u32, Mob>,
    owned: HashSet<u32>,
    awake: BTreeSet<u32>,
    spawn_grid: Option<SpatialGrid<u32>>,
    dice: Dice,
    brain_runs: u64,
    /// Where each player last scanned for mobs to wake.
    scanned: HashMap<u64, Position>,
    wake_scans: u64,
}

fn position_of(spawn: &VitalsSpawn) -> Position {
    let [x, y, z] = spawn.position;
    Position { x, y, z }
}

impl MobChanges {
    /// Adds the counts of `other`.
    pub const fn add(&mut self, other: Self) {
        self.spawned += other.spawned;
        self.removed += other.removed;
        self.unspawned += other.unspawned;
    }
}

impl Mobs {
    /// No mob yet; the spawns of the cells come with [`Mobs::set_cells`].
    pub fn new(spawns: Arc<[VitalsSpawn]>, combat: &CombatSettings) -> Self {
        Self {
            spawns,
            data: Arc::clone(&combat.data),
            settings: combat.brain,
            mob_attack_factor: combat.mob_attack_factor,
            live: HashMap::new(),
            owned: HashSet::new(),
            awake: BTreeSet::new(),
            spawn_grid: None,
            dice: Dice::new(combat.seed),
            brain_runs: 0,
            scanned: HashMap::new(),
            wake_scans: 0,
        }
    }

    /// The mob of spawn `index`, or `None` without its rows.
    fn make(&self, spawn: &VitalsSpawn) -> Option<Mob> {
        let kind = self.data.mob_type(spawn.vitals_id)?;
        let level = self.data.level(spawn.level)?;
        let sight_m =
            self.settings.base_sight_m + self.data.vision_offset(kind.creature_type.as_deref());
        Some(Mob {
            brain: Brain::new(position_of(spawn), sight_m),
            health: Health::full(mob_max_health(level.base_max_health, kind.health_mod)),
            attack: MobAttack {
                base_damage: level.base_damage,
                damage_mod: kind.damage_mod,
                factor: self.mob_attack_factor,
            },
        })
    }

    /// Spawns the mobs of the spawns that `owns`, and removes the others.
    pub fn set_cells(&mut self, owns: impl Fn(Position) -> bool) -> MobChanges {
        let mut changes = MobChanges::default();
        self.scanned.clear();
        let spawns = Arc::clone(&self.spawns);
        for (index, spawn) in (0_u32..).zip(spawns.iter()) {
            if owns(position_of(spawn)) {
                if !self.owned.insert(index) {
                    continue;
                }
                match self.make(spawn) {
                    Some(mob) => {
                        self.live.insert(index, mob);
                        changes.spawned += 1;
                    }
                    None => changes.unspawned += 1,
                }
            } else if self.owned.remove(&index) {
                self.awake.remove(&index);
                if self.live.remove(&index).is_some() {
                    changes.removed += 1;
                }
            }
        }
        self.spawn_grid = SpatialGrid::new(
            self.settings.wake_radius_m + self.settings.leash_radius_m,
        )
        .map(|mut grid| {
            for (index, mob) in &self.live {
                grid.insert(*index, mob.brain.spawn());
            }
            grid
        });
        changes
    }

    /// One tick at `now_ms` with the living players and player ghosts near this sim's mobs.
    /// Returns the hits the mobs make.
    pub fn tick(
        &mut self,
        players: &[Seen],
        now_ms: u64,
        ground: &dyn Ground,
    ) -> Vec<MobAttackIntent> {
        let wake_m = self.settings.wake_radius_m;
        let Some(mut player_grid) = SpatialGrid::new(wake_m) else {
            return Vec::new();
        };
        for player in players {
            player_grid.insert(player.entity, player.position);
        }
        let mut run: BTreeSet<u32> = self.awake.clone();
        let mut scanned = HashMap::with_capacity(players.len());
        if let Some(spawn_grid) = &self.spawn_grid {
            for player in players {
                if let Some(last) = self
                    .scanned
                    .get(&player.entity)
                    .filter(|last| last.horizontal_distance(player.position) < WAKE_RESCAN_M)
                {
                    scanned.insert(player.entity, *last);
                    continue;
                }
                scanned.insert(player.entity, player.position);
                self.wake_scans += 1;
                for (index, _) in spawn_grid.near(player.position) {
                    let near = self.live.get(&index).is_some_and(|mob| {
                        mob.brain.position().horizontal_distance(player.position) <= wake_m
                    });
                    if near {
                        run.insert(index);
                    }
                }
            }
        }
        self.scanned = scanned;
        let mut intents = Vec::new();
        for index in run {
            let Some(mob) = self.live.get_mut(&index) else {
                continue;
            };
            let at = mob.brain.position();
            let seen: Vec<Seen> = player_grid
                .near(at)
                .filter(|(_, position)| position.horizontal_distance(at) <= wake_m)
                .map(|(entity, position)| Seen { entity, position })
                .collect();
            self.brain_runs += 1;
            match brain_step(
                &mut mob.brain,
                &seen,
                now_ms,
                &self.settings,
                &mut self.dice,
            ) {
                BrainAction::MoveTo(to) => {
                    mob.brain
                        .set_height(ground.height_at(to.x, to.y).unwrap_or(to.z));
                }
                BrainAction::Attack(target) => intents.push(MobAttackIntent {
                    mob: mob_entity(index),
                    target,
                }),
                BrainAction::Respawned | BrainAction::Home => mob.health.refill(),
                BrainAction::None => {}
            }
            if mob.brain.state() == MobState::Asleep {
                self.awake.remove(&index);
            } else {
                self.awake.insert(index);
            }
        }
        intents
    }

    /// Every mob forgets `player` (it left, died or handed off).
    pub fn forget(&mut self, player: u64) {
        for index in &self.awake {
            if let Some(mob) = self.live.get_mut(index) {
                forget(&mut mob.brain, player);
            }
        }
    }

    /// Every awake mob: entity number, position and state (M6-A spec section 6.3, M6-B spec
    /// section 5); the ghosts for other sims and the mobs of the bundles both come from here.
    pub fn awake_states(&self) -> impl Iterator<Item = (u64, Position, MobGhost)> + '_ {
        self.awake.iter().filter_map(|index| {
            let mob = self.live.get(index)?;
            let spawn = self.spawns.get(*index as usize)?;
            Some((
                mob_entity(*index),
                mob.brain.position(),
                MobGhost {
                    health: mob.health.current(),
                    max_health: mob.health.max(),
                    returning: mob.brain.state() == MobState::Return,
                    vitals_id: spawn.vitals_id,
                    category_id: spawn.category_id,
                    level: spawn.level,
                },
            ))
        })
    }

    /// The mob with entity number `entity`.
    pub fn get_mut(&mut self, entity: u64) -> Option<&mut Mob> {
        self.live.get_mut(&spawn_index(entity)?)
    }

    /// Marks the mob `entity` awake (a hit woke it).
    pub fn wake(&mut self, entity: u64) {
        if let Some(index) = spawn_index(entity).filter(|index| self.live.contains_key(index)) {
            self.awake.insert(index);
        }
    }

    /// Live mobs.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Whether this sim has no mob.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// The admin snapshot: every awake mob, and mobs by state (admin tools spec section 5).
    pub fn snapshot(&self) -> (Vec<SnapMob>, [u32; MOB_STATES]) {
        let mobs = self
            .awake
            .iter()
            .filter_map(|index| {
                let mob = self.live.get(index)?;
                let at = mob.brain.position();
                Some(SnapMob {
                    entity: mob_entity(*index),
                    x: at.x,
                    y: at.y,
                    state: state_index(mob.brain.state()),
                    health: mob.health.current(),
                    max_health: mob.health.max(),
                })
            })
            .collect();
        let counts = self
            .counts()
            .map(|(_, count)| u32::try_from(count).unwrap_or(u32::MAX));
        (mobs, counts)
    }

    /// Mobs by state.
    pub fn counts(&self) -> [(MobState, usize); MobState::ALL.len()] {
        MobState::ALL.map(|state| {
            let count = if state == MobState::Asleep {
                self.live.len() - self.awake.len()
            } else {
                self.awake
                    .iter()
                    .filter(|index| {
                        self.live
                            .get(index)
                            .is_some_and(|mob| mob.brain.state() == state)
                    })
                    .count()
            };
            (state, count)
        })
    }

    /// Brain steps run since the start.
    pub const fn brain_runs(&self) -> u64 {
        self.brain_runs
    }

    /// Wake scans of players since the start.
    pub const fn wake_scans(&self) -> u64 {
        self.wake_scans
    }
}

/// The position of `state` in [`MobState::ALL`].
fn state_index(state: MobState) -> u8 {
    MobState::ALL
        .iter()
        .position(|each| *each == state)
        .and_then(|index| u8::try_from(index).ok())
        .unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_formats::objectstream::lowercase_crc32;
    use nw_sim_core::{Ground, MobState, hurt};

    use super::*;
    use crate::combat::test_support::{TEST_LEVEL, TEST_MOB};
    use crate::combat::tests::test_combat;

    const HEIGHT: f32 = 37.5;
    const PLAYER: u64 = 3;
    const TICK_MS: u64 = 33;
    const FAR_M: f32 = 5000.0;

    struct Flat;

    impl Ground for Flat {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(HEIGHT)
        }
    }

    fn spawn_at(x: f32, id: &str, level: u32) -> VitalsSpawn {
        VitalsSpawn {
            vitals_id: lowercase_crc32(id),
            level,
            position: [x, 1000.0, HEIGHT],
            ..VitalsSpawn::default()
        }
    }

    fn mobs(spawns: Vec<VitalsSpawn>) -> Mobs {
        Mobs::new(spawns.into(), &test_combat())
    }

    fn player_at(x: f32) -> Seen {
        Seen {
            entity: PLAYER,
            position: Position {
                x,
                y: 1000.0,
                z: HEIGHT,
            },
        }
    }

    fn every(_: Position) -> bool {
        true
    }

    #[test]
    fn mobs_of_owned_cells_spawn_and_lost_cells_remove_them() {
        let mut mobs = mobs(vec![
            spawn_at(100.0, TEST_MOB, TEST_LEVEL),
            spawn_at(900.0, TEST_MOB, TEST_LEVEL),
        ]);
        let west = mobs.set_cells(|position| position.x < 500.0);
        assert_eq!((west.spawned, west.removed), (1, 0));
        assert!(mobs.get_mut(mob_entity(0)).is_some());
        let east = mobs.set_cells(|position| position.x >= 500.0);
        assert_eq!((east.spawned, east.removed), (1, 1));
        assert!(mobs.get_mut(mob_entity(0)).is_none());
        assert!(mobs.get_mut(mob_entity(1)).is_some());
        assert_eq!(mobs.len(), 1);
    }

    #[test]
    fn a_spawn_without_rows_is_not_spawned_and_counted() {
        let mut mobs = mobs(vec![
            spawn_at(100.0, "Nobody", TEST_LEVEL),
            spawn_at(200.0, TEST_MOB, 0),
            spawn_at(300.0, TEST_MOB, TEST_LEVEL),
        ]);
        let first = mobs.set_cells(every);
        assert_eq!((first.spawned, first.unspawned), (1, 2));
        let again = mobs.set_cells(every);
        assert_eq!((again.spawned, again.unspawned), (0, 0), "counted once");
    }

    #[test]
    fn mob_numbers_hold_the_spawn_index() {
        assert_eq!(mob_entity(5), MOB_ENTITY_BASE + 5);
        assert_eq!(spawn_index(mob_entity(5)), Some(5));
        assert_eq!(spawn_index(42), None);
        /// The most bits of an entity number in a bundle member (the 5-byte prefix code).
        const BUNDLE_NUMBER_BITS: u32 = 35;
        /// Far more spawns than any level has.
        const MANY_SPAWNS: u32 = 10_000_000;
        assert!(mob_entity(MANY_SPAWNS) < 1 << BUNDLE_NUMBER_BITS);
        let mut mobs = mobs(vec![spawn_at(100.0, TEST_MOB, TEST_LEVEL)]);
        mobs.set_cells(every);
        let mob = mobs.get_mut(mob_entity(0)).expect("mob");
        assert_eq!(mob.health.max(), 1000, "level health times the plain mod");
    }

    #[test]
    fn only_awake_mobs_run_their_brain() {
        let mut mobs = mobs(vec![
            spawn_at(100.0, TEST_MOB, TEST_LEVEL),
            spawn_at(100.0 + FAR_M, TEST_MOB, TEST_LEVEL),
            spawn_at(100.0 + 2.0 * FAR_M, TEST_MOB, TEST_LEVEL),
        ]);
        mobs.set_cells(every);
        mobs.tick(&[], 0, &Flat);
        assert_eq!(mobs.brain_runs(), 0, "no player, no brain");
        mobs.tick(&[player_at(150.0)], TICK_MS, &Flat);
        assert_eq!(mobs.brain_runs(), 1, "only the mob near the player");
        mobs.tick(&[player_at(FAR_M * 4.0)], 2 * TICK_MS, &Flat);
        assert_eq!(mobs.brain_runs(), 2, "the awake mob runs until it sleeps");
        let awake = mobs
            .counts()
            .into_iter()
            .find(|(state, _)| *state == MobState::Idle);
        assert_eq!(awake, Some((MobState::Idle, 1)));
    }

    #[test]
    fn mobs_walk_on_the_ground() {
        let mut spawn = spawn_at(100.0, TEST_MOB, TEST_LEVEL);
        spawn.position[2] = 0.0;
        let mut mobs = mobs(vec![spawn]);
        mobs.set_cells(every);
        for tick in 0..10 {
            mobs.tick(&[player_at(110.0)], tick * TICK_MS, &Flat);
        }
        let mob = mobs.get_mut(mob_entity(0)).expect("mob");
        assert_eq!(mob.brain.state(), MobState::Chase);
        assert_eq!(mob.brain.position().z, HEIGHT);
    }

    #[test]
    fn mob_attack_intents_name_the_target() {
        let mut mobs = mobs(vec![spawn_at(100.0, TEST_MOB, TEST_LEVEL)]);
        mobs.set_cells(every);
        assert!(
            mobs.tick(&[player_at(101.0)], 0, &Flat).is_empty(),
            "it wakes first"
        );
        let intents = mobs.tick(&[player_at(101.0)], TICK_MS, &Flat);
        assert_eq!(
            intents,
            vec![MobAttackIntent {
                mob: mob_entity(0),
                target: PLAYER
            }]
        );
    }

    #[test]
    fn a_player_that_stands_still_does_not_rescan() {
        let mut mobs = mobs(vec![spawn_at(100.0, TEST_MOB, TEST_LEVEL)]);
        mobs.set_cells(every);
        mobs.tick(&[player_at(150.0)], 0, &Flat);
        assert_eq!(mobs.wake_scans(), 1);
        mobs.tick(&[player_at(150.0 + WAKE_RESCAN_M - 1.0)], TICK_MS, &Flat);
        assert_eq!(mobs.wake_scans(), 1, "still near its last scan");
        mobs.tick(
            &[player_at(150.0 + WAKE_RESCAN_M + 1.0)],
            2 * TICK_MS,
            &Flat,
        );
        assert_eq!(mobs.wake_scans(), 2, "moved far enough");
        mobs.set_cells(|_| false);
        mobs.set_cells(every);
        mobs.tick(
            &[player_at(150.0 + WAKE_RESCAN_M + 1.0)],
            3 * TICK_MS,
            &Flat,
        );
        assert_eq!(mobs.wake_scans(), 3, "new mobs: everyone scans again");
    }

    #[test]
    fn a_mob_forgets_a_player_who_left() {
        let mut mobs = mobs(vec![spawn_at(100.0, TEST_MOB, TEST_LEVEL)]);
        mobs.set_cells(every);
        mobs.tick(&[player_at(115.0)], 0, &Flat);
        let mob = mobs.get_mut(mob_entity(0)).expect("mob");
        assert!(hurt(&mut mob.brain, PLAYER, 1, 0));
        mobs.forget(PLAYER);
        mobs.tick(&[], TICK_MS, &Flat);
        let mob = mobs.get_mut(mob_entity(0)).expect("mob");
        assert_eq!(mob.brain.state(), MobState::Return);
    }

    #[test]
    fn the_spawn_list_is_shared() {
        let spawns: Arc<[VitalsSpawn]> = vec![spawn_at(100.0, TEST_MOB, TEST_LEVEL)].into();
        let mobs = Mobs::new(Arc::clone(&spawns), &test_combat());
        assert_eq!(Arc::strong_count(&spawns), 2);
        assert_eq!(mobs.len(), 0, "nothing spawns before the cells are known");
    }
}
