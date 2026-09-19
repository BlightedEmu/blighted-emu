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
//!
//! A personal mob (a tutorial encounter's) belongs to one player: only that player sees it and is
//! its target, it may be held idle until its fight starts, it stays awake, and it does not
//! respawn: its encounter removes it.
//!
//! A GM mob (admin studio spec section 8.2) is spawned by a game master at any point: every player
//! sees and fights it, it stays awake, and it respawns at its spawn until the GM removes it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use nw_bus::{MOB_ENTITY_BASE, MOB_STATES, MobGhost, SnapMob};
use nw_content::combat::CombatData;
use nw_content::mob_prefabs::ASSET_ID_LEN;
use nw_sim_core::{
    Behaviour, Brain, BrainAction, BrainSettings, Dice, Gait, Ground, Health, MobAttack, MobState,
    Position, Seen, SpatialGrid, TUTORIAL_FOLDER, brain_step, forget, mob_max_health,
};
pub use nw_world::records::VitalsSpawn;

use crate::combat::CombatSettings;
use crate::mob_ai::MobAi;

/// A player scans for mobs to wake again after moving this far (about 2 s of running); far less
/// than the wake radius, so a mob wakes before the player can reach it.
pub const WAKE_RESCAN_M: f32 = 10.0;

/// A moving mob keeps its own height when the level's ground lies more than this far below it
/// (our choice): it stands on a floor the height map does not hold, such as the tutorial ship's
/// deck high above the sea bed (live 2026-09-12: the sailor dropped under the ship and vanished).
pub const MAX_STEP_DOWN_M: f32 = 3.0;

/// The height of a mob that moves to `to` on `ground`: the ground's, unless the ground lies more
/// than [`MAX_STEP_DOWN_M`] below the mob.
fn height_after_move(ground: &dyn Ground, to: Position) -> f32 {
    match ground.height_at(to.x, to.y) {
        Some(height) if to.z - height <= MAX_STEP_DOWN_M => height,
        _ => to.z,
    }
}

/// The first mob's number after [`MOB_ENTITY_BASE`]. A number whose low 32 bits are all zero
/// gives a `GdeRef` that the client takes but never shows: in every live run the first spawned
/// mob (`MOB_ENTITY_BASE` itself, the tutorial's sailor 1) stayed invisible while it fought,
/// and the mobs after it, with the same card bytes, showed (2026-09-14).
const FIRST_MOB_OFFSET: u64 = 1;

/// The entity number of the mob of spawn `index`.
pub const fn mob_entity(index: u32) -> u64 {
    MOB_ENTITY_BASE + FIRST_MOB_OFFSET + index as u64
}

/// The spawn index of a mob's entity number; `None` for a player's number.
pub fn spawn_index(entity: u64) -> Option<u32> {
    entity
        .checked_sub(MOB_ENTITY_BASE + FIRST_MOB_OFFSET)
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobAttackIntent {
    /// The mob's entity number.
    pub mob: u64,
    /// The target's entity number.
    pub target: u64,
    /// Where the target was when the swing started: the mob aims there.
    pub target_at: Position,
    /// The move it swings, when its pace has moves.
    pub mob_move: Option<MobMove>,
}

/// One of `moves` at random; `None` when there are none.
fn pick(moves: &[MobMove], dice: &mut Dice) -> Option<MobMove> {
    let count = u32::try_from(moves.len()).ok().filter(|count| *count > 0)?;
    moves.get(usize::try_from(dice.below(count)).ok()?).copied()
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

/// How far past the distance where a mob starts its swing the swing still lands (our choice:
/// retail hits landed 0.77 to 2.19 m from the player's body; sailors start at 1.8 m).
pub const SWING_HIT_MARGIN_M: f32 = 1.0;

/// The item tag of the hits of a mob without a pace (our choice: the retail sailors' and risen's
/// tag; the open world's are Unknown).
pub const DEFAULT_HIT_TAG: u8 = 6;

/// The stamina a blocked swing of a mob without a pace takes (Inferred: the retail signifier's
/// light stab, a level 1 mob, took 14.6; the swings of other mobs are Unknown).
pub const DEFAULT_BLOCK_STAMINA: f32 = 14.6;

/// The upright capsule of a body: its radius and height (collision note: the `CharacterComponent`
/// capsules of the creature slices).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    /// Its radius.
    pub radius_m: f32,
    /// Its height, from its feet.
    pub height_m: f32,
}

/// The body of a mob without a pace (our choice: the legionnaire's, risen's and swarmer's
/// capsule).
pub const DEFAULT_MOB_CAPSULE: Capsule = Capsule {
    radius_m: 0.5,
    height_m: 1.8,
};

/// How a mob's swing reaches its target (collision note, section 3: each retail creature lunged
/// forward in its swing until the bodies touched, so every retail mob hit landed 0.79 to 1.65 m
/// from the player).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobReach {
    /// The lunge stops this near the target, or where the two bodies touch.
    pub lunge_stop_m: f32,
    /// The longest lunge.
    pub lunge_max_m: f32,
    /// It lunges only at a target within this angle of where it aimed at the swing start
    /// (radians).
    pub lunge_turn_rad: f32,
    /// How far in front of its body the hit shape reaches.
    pub reach_m: f32,
}

/// One full-body fragment of a creature: its fragment and action numbers (its controller
/// definitions) and the last byte of a retail entry of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveFragment {
    /// The fragment number.
    pub fragment: u16,
    /// The action number.
    pub action: u16,
    /// The last byte of the retail entry.
    pub tail: u8,
}

/// One hit of a mob move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveHit {
    /// When it lands, after the move starts.
    pub at_ms: u64,
    /// Its share of the pace's hit: its damage row's coefficient over the coefficient of the row
    /// the pace's attack factor was fitted to.
    pub damage_scale: f64,
    /// The share of its damage that is fire in a second phase (its row's `AI_FireDMG` affix).
    pub fire_share: f64,
    /// The stamina it takes from a player who blocks it.
    pub block_stamina: f32,
}

/// An attack that a mob picks from its pace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobMove {
    /// The fragment it plays.
    pub fragment: MoveFragment,
    /// How long the fragment plays.
    pub length_ms: u64,
    /// Its hits.
    pub hits: &'static [MoveHit],
    /// The 3601 item tag of its hits.
    pub tag: u8,
}

/// The second phase of a boss.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhaseTwo {
    /// The moves it picks from then.
    pub moves: &'static [MobMove],
    /// Its damage then, as a multiple of the first phase's.
    pub damage_mult: f64,
    /// The 3601 item tag of the fire part of its hits.
    pub fire_tag: u8,
}

/// How a mob hits with one hit of a move: its damage multiple, its item tags and the stamina
/// a block costs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveStrike {
    /// The hit.
    pub hit: MoveHit,
    /// The item tag of the hit.
    pub tag: u8,
    /// In a second phase: its damage multiple and the item tag of the fire part.
    pub phase_two: Option<(f64, u8)>,
}

/// How a personal mob fights when not as its rows and the config say. A paced mob stands on the
/// floor height of its spawn point; the height map does not move it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pace {
    /// Its attack factor, in place of the config's `mob_attack_factor`.
    pub attack_factor: f64,
    /// Time between two swings: a random time in this range, rolled after each swing. Its first
    /// swing also waits the shortest gap after its fight starts.
    pub swing_gap_ms: (u64, u64),
    /// Its chase speed, in place of the config's `mob_speed_mps`.
    pub speed_mps: f32,
    /// When a swing hits, after it starts.
    pub hit_delay_ms: u64,
    /// The stamina a swing takes from a player who blocks it.
    pub block_stamina: f32,
    /// How it moves and when it swings.
    pub gait: Gait,
    /// How far it sees, in place of the config's sight.
    pub sight_m: f32,
    /// The item tag of its hits on a player (3601; meaning Unknown).
    pub hit_tag: u8,
    /// The item scale of its hits on a player: the share of the raw damage that the player's
    /// armor let through.
    pub hit_scale: f32,
    /// Its body.
    pub capsule: Capsule,
    /// How its swing reaches; `None`: it lands within its gait's swing start plus
    /// [`SWING_HIT_MARGIN_M`].
    pub reach: Option<MobReach>,
    /// The attacks it picks from, at random; empty: one swing of [`Pace::hit_delay_ms`]. With
    /// moves, the swing gap is the time after a move's end.
    pub moves: &'static [MobMove],
    /// Its second phase, for a boss.
    pub phase_two: Option<PhaseTwo>,
}

/// A mob spawned for one player.
#[derive(Debug, Clone)]
struct Personal {
    spawn: VitalsSpawn,
    owner: u64,
    /// The mob sees no player until its fight starts.
    held: bool,
    pace: Option<Pace>,
    /// The gap before its next swing, when paced.
    swing_gap_ms: u64,
    /// Until this time the mob does nothing (a scripted pause).
    frozen_until_ms: u64,
    /// Hits do no damage until this time.
    absorb_until_ms: u64,
    /// It fights in its pace's second phase.
    in_phase_two: bool,
    /// The slice asset of its retail card, when its vitals id has more than one look.
    asset: Option<[u8; ASSET_ID_LEN]>,
}

/// The mobs of one sim.
pub struct Mobs {
    spawns: Arc<[VitalsSpawn]>,
    /// Personal mobs by index; their indexes follow the spawn list.
    personal: HashMap<u32, Personal>,
    /// GM mobs by index, with their spawns; their indexes follow the spawn list too.
    gm: BTreeMap<u32, VitalsSpawn>,
    /// The design AI of each spawn of the spawn list (empty: none).
    ai: Arc<[Option<MobAi>]>,
    next_personal: u32,
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

/// The spawns that a sim of `level` spawns. Every mob of the tutorial is a fight of its quest,
/// which the tutorial script spawns for each player (its level file also lists the island's
/// fights as plain spawns); every other level keeps all its spawns.
pub fn spawns_of_level(level: &str, spawns: &[VitalsSpawn]) -> Arc<[VitalsSpawn]> {
    if level == TUTORIAL_FOLDER {
        return Arc::from([]);
    }
    spawns.iter().cloned().collect()
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
        let next_personal = u32::try_from(spawns.len()).unwrap_or(u32::MAX);
        Self {
            spawns,
            personal: HashMap::new(),
            gm: BTreeMap::new(),
            ai: Arc::from([]),
            next_personal,
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

    /// Spawns a personal mob of `spawn` for player `owner` at `now_ms`, held idle when `held`,
    /// hitting at `pace` when given, shown with slice asset `asset` when given; its entity
    /// number, or `None` without its rows. A paced mob that fights at once waits one interval
    /// before its first hit.
    pub fn spawn_personal(
        &mut self,
        spawn: VitalsSpawn,
        owner: u64,
        held: bool,
        pace: Option<Pace>,
        asset: Option<[u8; ASSET_ID_LEN]>,
        now_ms: u64,
    ) -> Option<u64> {
        let mut mob = self.make(&spawn)?;
        if let Some(pace) = pace {
            mob.attack.factor = pace.attack_factor;
            mob.brain.set_sight(pace.sight_m);
            if !held {
                mob.brain.wait_to_attack(now_ms);
            }
        }
        let swing_gap_ms = pace.map_or(0, |pace| pace.swing_gap_ms.0);
        let index = self.next_personal;
        self.next_personal = index.checked_add(1)?;
        self.live.insert(index, mob);
        self.awake.insert(index);
        self.personal.insert(
            index,
            Personal {
                spawn,
                owner,
                held,
                pace,
                swing_gap_ms,
                frozen_until_ms: 0,
                absorb_until_ms: 0,
                in_phase_two: false,
                asset,
            },
        );
        Some(mob_entity(index))
    }

    /// The design AI of the spawn list, in spawn order; set before the first cells.
    pub fn set_ai(&mut self, ai: Arc<[Option<MobAi>]>) {
        self.ai = ai;
    }

    /// The design AI of spawn `index`, when a design file gave it one.
    fn ai_of(&self, index: u32) -> Option<&MobAi> {
        self.ai.get(index as usize)?.as_ref()
    }

    /// Spawns a GM mob of `spawn`; its entity number, or `None` without its rows.
    pub fn spawn_gm(&mut self, spawn: VitalsSpawn) -> Option<u64> {
        let mob = self.make(&spawn)?;
        let index = self.next_personal;
        self.next_personal = index.checked_add(1)?;
        self.live.insert(index, mob);
        self.awake.insert(index);
        self.gm.insert(index, spawn);
        Some(mob_entity(index))
    }

    /// Removes the GM mob `entity`; `false` when it is not one.
    pub fn remove_gm(&mut self, entity: u64) -> bool {
        let Some(index) = spawn_index(entity).filter(|index| self.gm.remove(index).is_some())
        else {
            return false;
        };
        self.live.remove(&index);
        self.awake.remove(&index);
        true
    }

    /// The GM mobs, by entity number.
    pub fn gm_mobs(&self) -> Vec<u64> {
        self.gm.keys().map(|index| mob_entity(*index)).collect()
    }

    /// Lets the held personal mobs of `owner` see it at `now_ms`; a paced one waits one interval
    /// before its first hit.
    pub fn release(&mut self, owner: u64, now_ms: u64) {
        for (index, personal) in self
            .personal
            .iter_mut()
            .filter(|(_, mob)| mob.owner == owner && mob.held)
        {
            personal.held = false;
            if personal.pace.is_some()
                && let Some(mob) = self.live.get_mut(index)
            {
                mob.brain.wait_to_attack(now_ms);
            }
        }
    }

    /// Removes the personal mob `entity`.
    pub fn remove_personal(&mut self, entity: u64) {
        if let Some(index) = spawn_index(entity)
            && self.personal.remove(&index).is_some()
        {
            self.live.remove(&index);
            self.awake.remove(&index);
        }
    }

    /// The owner of mob `entity` when it is personal.
    pub fn owner(&self, entity: u64) -> Option<u64> {
        self.personal
            .get(&spawn_index(entity)?)
            .map(|personal| personal.owner)
    }

    /// The slice asset of personal mob `entity`'s retail card, when it has one.
    pub fn asset(&self, entity: u64) -> Option<[u8; ASSET_ID_LEN]> {
        self.personal.get(&spawn_index(entity)?)?.asset
    }

    /// The brain state of mob `entity`, when it lives here.
    pub fn brain_state(&self, entity: u64) -> Option<MobState> {
        self.live
            .get(&spawn_index(entity)?)
            .map(|mob| mob.brain.state())
    }

    /// Hits on personal mob `entity` do no damage until `until_ms`.
    pub fn absorb(&mut self, entity: u64, until_ms: u64) {
        if let Some(personal) = spawn_index(entity).and_then(|index| self.personal.get_mut(&index))
        {
            personal.absorb_until_ms = until_ms;
        }
    }

    /// Whether hits on mob `entity` do no damage at `now_ms`.
    pub fn absorbing(&self, entity: u64, now_ms: u64) -> bool {
        spawn_index(entity)
            .and_then(|index| self.personal.get(&index))
            .is_some_and(|personal| now_ms < personal.absorb_until_ms)
    }

    /// Personal mob `entity` stands still and does nothing until `until_ms`.
    pub fn freeze(&mut self, entity: u64, until_ms: u64) {
        if let Some(personal) = spawn_index(entity).and_then(|index| self.personal.get_mut(&index))
        {
            personal.frozen_until_ms = until_ms;
        }
    }

    /// When a swing of mob `entity` hits, after the swing starts: its pace's delay, else 0.
    pub fn hit_delay_ms(&self, entity: u64) -> u64 {
        spawn_index(entity)
            .and_then(|index| self.personal.get(&index)?.pace)
            .map_or(0, |pace| pace.hit_delay_ms)
    }

    /// The stamina a swing of mob `entity` takes from a player who blocks it: its pace's, or
    /// [`DEFAULT_BLOCK_STAMINA`].
    pub fn block_stamina(&self, entity: u64) -> f32 {
        spawn_index(entity)
            .and_then(|index| self.personal.get(&index)?.pace)
            .map_or(DEFAULT_BLOCK_STAMINA, |pace| pace.block_stamina)
    }

    /// Personal mob `entity` fights in its pace's second phase from now on.
    pub fn enter_phase_two(&mut self, entity: u64) {
        if let Some(personal) = spawn_index(entity).and_then(|index| self.personal.get_mut(&index))
        {
            personal.in_phase_two = true;
        }
    }

    /// The second phase of mob `entity`, while it fights in it.
    pub fn phase_two(&self, entity: u64) -> Option<PhaseTwo> {
        let personal = self.personal.get(&spawn_index(entity)?)?;
        personal.pace?.phase_two.filter(|_| personal.in_phase_two)
    }

    /// The body of mob `entity`: its pace's, else [`DEFAULT_MOB_CAPSULE`].
    pub fn capsule(&self, entity: u64) -> Capsule {
        spawn_index(entity)
            .and_then(|index| self.personal.get(&index)?.pace)
            .map_or(DEFAULT_MOB_CAPSULE, |pace| pace.capsule)
    }

    /// How the swing of mob `entity` reaches, when its pace says.
    pub fn reach(&self, entity: u64) -> Option<MobReach> {
        spawn_index(entity)
            .and_then(|index| self.personal.get(&index)?.pace)
            .and_then(|pace| pace.reach)
    }

    /// Mob `entity` lunges up to `max_m` toward `to` in its swing, inside its leash; where it is
    /// then.
    pub fn lunge(&mut self, entity: u64, to: Position, max_m: f32) -> Option<Position> {
        let leash_m = self.settings.leash_radius_m;
        let mob = self.get_mut(entity)?;
        Some(mob.brain.lunge(to, max_m, leash_m))
    }

    /// How far from mob `entity` its swing lands on a player: where its gait starts a swing, plus
    /// [`SWING_HIT_MARGIN_M`].
    pub fn hit_reach_m(&self, entity: u64) -> f32 {
        let gait = spawn_index(entity)
            .and_then(|index| self.personal.get(&index)?.pace)
            .map_or(self.settings.gait, |pace| pace.gait);
        gait.swing_start_m + SWING_HIT_MARGIN_M
    }

    /// The item tag and scale of the hits of mob `entity`: its pace's, else [`DEFAULT_HIT_TAG`]
    /// and `default_scale`.
    pub fn hit_style(&self, entity: u64, default_scale: f32) -> (u8, f32) {
        spawn_index(entity)
            .and_then(|index| self.personal.get(&index)?.pace)
            .map_or((DEFAULT_HIT_TAG, default_scale), |pace| {
                (pace.hit_tag, pace.hit_scale)
            })
    }

    /// The level of mob `entity`, when it lives here.
    pub fn level(&self, entity: u64) -> Option<u32> {
        self.spawn_of(spawn_index(entity)?).map(|spawn| spawn.level)
    }

    /// The `vitals_id` (CRC-32 of the lowercase `VitalsID`) of mob `entity`, when it lives here.
    pub fn vitals_of(&self, entity: u64) -> Option<u32> {
        self.spawn_of(spawn_index(entity)?)
            .map(|spawn| spawn.vitals_id)
    }

    /// The entity mob `entity` chases or hits, when it lives here.
    pub fn target(&self, entity: u64) -> Option<u64> {
        self.live.get(&spawn_index(entity)?)?.brain.target()
    }

    /// The health of mob `entity`, when it lives here.
    pub fn health(&self, entity: u64) -> Option<u32> {
        self.live
            .get(&spawn_index(entity)?)
            .map(|mob| mob.health.current())
    }

    /// The spawn of mob `index`: a world spawn or a personal one.
    fn spawn_of(&self, index: u32) -> Option<&VitalsSpawn> {
        self.spawns
            .get(index as usize)
            .or_else(|| self.personal.get(&index).map(|personal| &personal.spawn))
            .or_else(|| self.gm.get(&index))
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
                    Some(mut mob) => {
                        if let Some(ai) = self.ai_of(index) {
                            mob.brain.set_behaviour(ai.behaviour);
                            if let Some(sight_m) = ai.sight_m {
                                mob.brain.set_sight(sight_m);
                            }
                            if let Some(route) = &ai.route {
                                mob.brain.set_route(route.clone());
                            }
                            if ai.behaviour == Behaviour::Patrol {
                                self.awake.insert(index);
                            }
                        }
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
            if self
                .personal
                .get(&index)
                .is_some_and(|personal| now_ms < personal.frozen_until_ms)
            {
                // A frozen mob's step clock waits with it (live 2026-09-14: each sailor jumped
                // 5.5 m in one state after its alert, a second of run at once).
                mob.brain.stood_until(now_ms);
                continue;
            }
            let at = mob.brain.position();
            let personal = self.personal.get(&index);
            let seen: Vec<Seen> = player_grid
                .near(at)
                .filter(|(_, position)| position.horizontal_distance(at) <= wake_m)
                .filter(|(entity, _)| personal.is_none_or(|mob| !mob.held && mob.owner == *entity))
                .map(|(entity, position)| Seen { entity, position })
                .collect();
            self.brain_runs += 1;
            let designed = self.ai.get(index as usize).and_then(Option::as_ref);
            let base = designed.map_or(self.settings, |ai| ai.settings);
            let settings = personal
                .and_then(|personal| Some((personal.swing_gap_ms, personal.pace?)))
                .map_or(base, |(swing_gap_ms, pace)| BrainSettings {
                    mob_attack_interval_ms: swing_gap_ms,
                    mob_speed_mps: pace.speed_mps,
                    gait: pace.gait,
                    ..self.settings
                });
            // A patrolling mob walks away from its spawn, where the wake scan looks: it stays
            // awake.
            let patrols = designed.is_some_and(|ai| ai.behaviour == Behaviour::Patrol);
            let is_personal = personal.is_some();
            let paced = personal.is_some_and(|personal| personal.pace.is_some());
            let action = brain_step(&mut mob.brain, &seen, now_ms, &settings, &mut self.dice);
            match action {
                // A paced mob keeps the floor height of its spawn: the retail tutorial creatures
                // stayed on one floor per fight, a ruin or a deck that the height map does not
                // hold (height note: 0.3 to 6 m above it, Thorpe's arena with no terrain).
                BrainAction::MoveTo(to) if !paced => {
                    mob.brain.set_height(height_after_move(ground, to));
                }
                BrainAction::MoveTo(_) => {}
                BrainAction::Attack(target) => {
                    let target_at = seen
                        .iter()
                        .find(|player| player.entity == target)
                        .map_or(at, |player| player.position);
                    let mut mob_move = None;
                    if let Some(personal) = self.personal.get_mut(&index)
                        && let Some(pace) = personal.pace
                    {
                        let moves = pace
                            .phase_two
                            .filter(|_| personal.in_phase_two)
                            .map_or(pace.moves, |phase| phase.moves);
                        mob_move = pick(moves, &mut self.dice);
                        let (shortest, longest) = pace.swing_gap_ms;
                        let spread =
                            u32::try_from(longest.saturating_sub(shortest)).unwrap_or(u32::MAX);
                        let length_ms = mob_move.map_or(0, |picked: MobMove| picked.length_ms);
                        personal.swing_gap_ms = length_ms
                            + shortest
                            + u64::from(self.dice.below(spread.saturating_add(1)));
                    }
                    intents.push(MobAttackIntent {
                        mob: mob_entity(index),
                        target,
                        target_at,
                        mob_move,
                    });
                }
                BrainAction::Respawned | BrainAction::Home => mob.health.refill(),
                BrainAction::None => {}
            }
            let stays_awake = is_personal || patrols || self.gm.contains_key(&index);
            if mob.brain.state() == MobState::Asleep && !stays_awake {
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
            let spawn = self.spawn_of(*index)?;
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

    /// The admin snapshot: every awake mob, and mobs by state (admin tools spec section 5), in
    /// one walk over the awake mobs.
    pub fn snapshot(&self) -> (Vec<SnapMob>, [u32; MOB_STATES]) {
        let mut counts = self.asleep_counts();
        let mobs = self
            .awake_mobs()
            .map(|(index, mob)| {
                tally(&mut counts, mob.brain.state());
                let at = mob.brain.position();
                SnapMob {
                    entity: mob_entity(index),
                    x: at.x,
                    y: at.y,
                    state: state_index(mob.brain.state()),
                    health: mob.health.current(),
                    max_health: mob.health.max(),
                }
            })
            .collect();
        let counts = counts.map(|(_, count)| u32::try_from(count).unwrap_or(u32::MAX));
        (mobs, counts)
    }

    /// Mobs by state, in one walk over the awake mobs.
    pub fn counts(&self) -> [(MobState, usize); MobState::ALL.len()] {
        let mut counts = self.asleep_counts();
        for (_, mob) in self.awake_mobs() {
            tally(&mut counts, mob.brain.state());
        }
        counts
    }

    /// Every state at 0 but the asleep mobs: every live mob that is not awake.
    fn asleep_counts(&self) -> [(MobState, usize); MobState::ALL.len()] {
        let asleep = self.live.len() - self.awake.len();
        MobState::ALL.map(|state| (state, if state == MobState::Asleep { asleep } else { 0 }))
    }

    fn awake_mobs(&self) -> impl Iterator<Item = (u32, &Mob)> + '_ {
        self.awake
            .iter()
            .filter_map(|index| self.live.get(index).map(|mob| (*index, mob)))
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
/// Counts one awake mob in `state`; asleep mobs are counted as the mobs that are not awake.
fn tally(counts: &mut [(MobState, usize); MobState::ALL.len()], state: MobState) {
    if state == MobState::Asleep {
        return;
    }
    if let Some((_, count)) = counts.iter_mut().find(|(each, _)| *each == state) {
        *count += 1;
    }
}

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
    use std::time::{Duration, Instant};

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

    #[test]
    fn the_tutorial_spawns_only_its_script_and_other_levels_keep_their_spawns() {
        let encounter = VitalsSpawn {
            is_encounter: true,
            ..spawn_at(0.0, TEST_MOB, TEST_LEVEL)
        };
        let standing = spawn_at(1.0, TEST_MOB, TEST_LEVEL);
        let spawns = [encounter.clone(), standing.clone()];
        assert!(spawns_of_level(TUTORIAL_FOLDER, &spawns).is_empty());
        assert_eq!(
            &*spawns_of_level("newworld_vitaeeterna", &spawns),
            &spawns[..]
        );
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
    fn a_gm_mob_is_everyones_stays_awake_and_can_be_removed() {
        const LATER_MS: u64 = 600_000;
        let mut mobs = mobs(Vec::new());
        let entity = mobs
            .spawn_gm(spawn_at(10.0, TEST_MOB, TEST_LEVEL))
            .expect("gm mob");
        assert_eq!(mobs.owner(entity), None, "every player sees it");
        assert_eq!(mobs.gm_mobs(), [entity]);
        assert!(mobs.health(entity).is_some());
        mobs.tick(&[], 0, &Flat);
        let runs = mobs.brain_runs;
        mobs.tick(&[], LATER_MS, &Flat);
        assert!(mobs.brain_runs > runs, "it stays awake with no player near");
        assert!(mobs.remove_gm(entity));
        assert!(!mobs.remove_gm(entity));
        assert_eq!(mobs.health(entity), None);
        assert!(mobs.gm_mobs().is_empty());
    }

    #[test]
    fn a_gm_mob_without_rows_is_refused() {
        let mut mobs = mobs(Vec::new());
        assert_eq!(
            mobs.spawn_gm(spawn_at(10.0, "no_such_mob", TEST_LEVEL)),
            None
        );
    }

    fn designed(behaviour: nw_sim_core::Behaviour, route: Option<nw_sim_core::Route>) -> MobAi {
        MobAi {
            behaviour,
            settings: test_combat().brain,
            sight_m: None,
            route,
        }
    }

    /// The position of mob `entity` after ticking from `from_ms` to `to_ms` with a player near
    /// enough to wake the mobs and too far to be seen.
    fn walk(mobs: &mut Mobs, entity: u64, from_ms: u64, to_ms: u64) -> Position {
        let player = Seen {
            entity: PLAYER,
            position: Position {
                x: 50.0,
                y: 1060.0,
                z: HEIGHT,
            },
        };
        for now in (from_ms..=to_ms).step_by(TICK_MS as usize) {
            mobs.tick(&[player], now, &Flat);
        }
        mobs.get_mut(entity).expect("mob").brain.position()
    }

    #[test]
    fn a_guard_spawn_stands_and_a_patrol_spawn_walks_its_route_awake() {
        const ROUTE_END_X: f32 = 90.0;
        let mut mobs = mobs(vec![
            spawn_at(10.0, TEST_MOB, TEST_LEVEL),
            spawn_at(10.0, TEST_MOB, TEST_LEVEL),
        ]);
        let route = nw_sim_core::Route {
            points: vec![Position {
                x: ROUTE_END_X,
                y: 1000.0,
                z: HEIGHT,
            }],
            waits_ms: Vec::new(),
            mode: nw_sim_core::RouteMode::Once,
        };
        mobs.set_ai(Arc::from([
            Some(designed(nw_sim_core::Behaviour::Guard, None)),
            Some(designed(nw_sim_core::Behaviour::Patrol, Some(route))),
        ]));
        mobs.set_cells(every);
        let guard = walk(&mut mobs, mob_entity(0), 0, 40_000);
        assert!(
            (guard.x - 10.0).abs() < 0.01,
            "the guard stands at its spawn: {guard:?}"
        );
        let patrol = mobs.get_mut(mob_entity(1)).expect("mob").brain.position();
        assert!(
            (patrol.x - ROUTE_END_X).abs() < 0.1,
            "the patrol reached its route end: {patrol:?}"
        );
        let later = walk(&mut mobs, mob_entity(1), 40_033, 200_000);
        assert!(
            (later.x - ROUTE_END_X).abs() < 0.1,
            "it stays awake and at its end"
        );
    }

    #[test]
    fn a_patrol_spawn_walks_its_route_with_no_player_in_the_world() {
        const ROUTE_END_X: f32 = 40.0;
        let mut mobs = mobs(vec![spawn_at(10.0, TEST_MOB, TEST_LEVEL)]);
        let route = nw_sim_core::Route {
            points: vec![Position {
                x: ROUTE_END_X,
                y: 1000.0,
                z: HEIGHT,
            }],
            waits_ms: Vec::new(),
            mode: nw_sim_core::RouteMode::Once,
        };
        mobs.set_ai(Arc::from([Some(designed(
            nw_sim_core::Behaviour::Patrol,
            Some(route),
        ))]));
        mobs.set_cells(every);
        for now in (0..=100_000).step_by(TICK_MS as usize) {
            mobs.tick(&[], now, &Flat);
        }
        let patrol = mobs.get_mut(mob_entity(0)).expect("mob").brain.position();
        assert!(
            (patrol.x - ROUTE_END_X).abs() < 0.1,
            "the patrol walked alone: {patrol:?}"
        );
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
        assert_eq!(mob_entity(5), MOB_ENTITY_BASE + FIRST_MOB_OFFSET + 5);
        // No mob number has its low 32 bits all zero (the client does not show such a mob).
        for index in [0, 1, 5, u32::MAX - 1] {
            assert_ne!(mob_entity(index) & u64::from(u32::MAX), 0, "index {index}");
        }
        assert_eq!(spawn_index(MOB_ENTITY_BASE), None, "below the first mob");
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
    fn the_snapshot_lists_awake_mobs_and_counts_every_state() {
        let mut mobs = mobs(vec![
            spawn_at(100.0, TEST_MOB, TEST_LEVEL),
            spawn_at(100.0 + FAR_M, TEST_MOB, TEST_LEVEL),
            spawn_at(100.0 + 2.0 * FAR_M, TEST_MOB, TEST_LEVEL),
        ]);
        mobs.set_cells(every);
        mobs.tick(&[player_at(150.0)], TICK_MS, &Flat);
        let (awake, counts) = mobs.snapshot();
        assert_eq!(
            awake.iter().map(|mob| mob.entity).collect::<Vec<_>>(),
            vec![mob_entity(0)]
        );
        let expected = mobs
            .counts()
            .map(|(_, count)| u32::try_from(count).expect("count"));
        assert_eq!(counts, expected);
        assert_eq!(counts.iter().sum::<u32>(), 3);
        assert_eq!(counts[usize::from(state_index(MobState::Asleep))], 2);
    }

    /// Admin tools spec 1.3 B3: run with
    /// `cargo test --release -p sim snapshot_speed -- --ignored --nocapture`.
    #[test]
    #[ignore = "release-mode timing of the admin snapshot"]
    fn snapshot_speed_of_many_awake_mobs() {
        /// Awake mobs of the busiest sim in the live panel exit test (one sim, 1,000 bots).
        const AWAKE: u32 = 23_000;
        const SPACING_M: f32 = 0.004;
        const ROUNDS: u32 = 100;
        const LIMIT: Duration = Duration::from_micros(500);
        let spawns = (0..AWAKE)
            .map(|index| spawn_at(100.0 + index as f32 * SPACING_M, TEST_MOB, TEST_LEVEL))
            .collect();
        let mut mobs = mobs(spawns);
        mobs.set_cells(every);
        mobs.tick(&[player_at(150.0)], TICK_MS, &Flat);
        assert_eq!(
            mobs.snapshot().0.len(),
            AWAKE as usize,
            "every mob is awake"
        );
        let started = Instant::now();
        for _ in 0..ROUNDS {
            std::hint::black_box(mobs.snapshot());
        }
        let average = started.elapsed() / ROUNDS;
        println!("snapshot of {AWAKE} awake mobs: {average:?}");
        assert!(average < LIMIT, "{average:?}");
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
    fn a_mob_on_a_deck_keeps_its_height() {
        const DECK: f32 = 90.0;
        let mut spawn = spawn_at(100.0, TEST_MOB, TEST_LEVEL);
        spawn.position[2] = DECK;
        let mut mobs = mobs(vec![spawn]);
        mobs.set_cells(every);
        for tick in 0..10 {
            mobs.tick(&[player_at(110.0)], tick * TICK_MS, &Flat);
        }
        let mob = mobs.get_mut(mob_entity(0)).expect("mob");
        assert_eq!(mob.brain.state(), MobState::Chase);
        assert_eq!(
            mob.brain.position().z,
            DECK,
            "the ground is far below the deck"
        );
        let near = Position {
            x: 0.0,
            y: 0.0,
            z: HEIGHT + MAX_STEP_DOWN_M,
        };
        assert_eq!(
            height_after_move(&Flat, near),
            HEIGHT,
            "a step down onto the ground"
        );
    }

    #[test]
    fn a_mob_with_moves_swings_them_and_waits_for_their_end() {
        const COOLDOWN_MS: u64 = 1_000;
        const FIRST: MobMove = MobMove {
            fragment: MoveFragment {
                fragment: 58,
                action: 28,
                tail: 0xa5,
            },
            length_ms: 2_000,
            hits: &[],
            tag: 5,
        };
        const SECOND: MobMove = MobMove {
            length_ms: 500,
            ..FIRST
        };
        const FIRST_MOVES: [MobMove; 1] = [FIRST];
        const SECOND_MOVES: [MobMove; 1] = [SECOND];
        const PACE: Pace = Pace {
            attack_factor: 0.25,
            swing_gap_ms: (COOLDOWN_MS, COOLDOWN_MS),
            speed_mps: 5.5,
            hit_delay_ms: 0,
            block_stamina: DEFAULT_BLOCK_STAMINA,
            gait: Gait::uniform(3.0, 5.5),
            sight_m: 20.0,
            hit_tag: DEFAULT_HIT_TAG,
            hit_scale: 1.0,
            capsule: DEFAULT_MOB_CAPSULE,
            reach: None,
            moves: &FIRST_MOVES,
            phase_two: Some(PhaseTwo {
                moves: &SECOND_MOVES,
                damage_mult: 2.2,
                fire_tag: 9,
            }),
        };
        const START: u64 = 1_000;
        let mut mobs = mobs(Vec::new());
        let boss = mobs
            .spawn_personal(
                spawn_at(100.0, TEST_MOB, TEST_LEVEL),
                PLAYER,
                false,
                Some(PACE),
                None,
                START,
            )
            .expect("rows");
        let mut swings = Vec::new();
        let mut now = START;
        while swings.len() < 2 {
            for intent in mobs.tick(&[player_at(101.0)], now, &Flat) {
                swings.push((now, intent.mob_move));
            }
            now += TICK_MS;
        }
        assert_eq!(swings[0].1, Some(FIRST));
        assert!(swings[1].0 - swings[0].0 >= FIRST.length_ms + COOLDOWN_MS);
        assert_eq!(mobs.phase_two(boss), None);
        mobs.enter_phase_two(boss);
        assert!(mobs.phase_two(boss).is_some());
        let next = (0..)
            .map(|tick| now + tick * TICK_MS)
            .find_map(|at| mobs.tick(&[player_at(101.0)], at, &Flat).first().copied())
            .expect("a swing");
        assert_eq!(next.mob_move, Some(SECOND));
    }

    #[test]
    fn a_paced_mob_waits_one_interval_after_its_fight_starts_and_between_hits() {
        const GAP_MS: u64 = 6_000;
        const SPREAD_MS: u64 = 1_000;
        const PACE: Pace = Pace {
            attack_factor: 0.25,
            swing_gap_ms: (GAP_MS, GAP_MS + SPREAD_MS),
            speed_mps: 5.5,
            hit_delay_ms: 456,
            block_stamina: DEFAULT_BLOCK_STAMINA,
            gait: Gait::uniform(3.0, 5.5),
            sight_m: 20.0,
            hit_tag: DEFAULT_HIT_TAG,
            hit_scale: 1.0,
            capsule: DEFAULT_MOB_CAPSULE,
            reach: None,
            moves: &[],
            phase_two: None,
        };
        const START: u64 = 1_000;
        let mut mobs = mobs(Vec::new());
        let held = mobs
            .spawn_personal(
                spawn_at(100.0, TEST_MOB, TEST_LEVEL),
                PLAYER,
                true,
                Some(PACE),
                None,
                0,
            )
            .expect("rows");
        assert_eq!(mobs.get_mut(held).map(|mob| mob.attack.factor), Some(0.25));
        mobs.release(PLAYER, START);
        let mut hits = Vec::new();
        let mut now = START;
        while now < START + 3 * PACE.swing_gap_ms.0 {
            if !mobs.tick(&[player_at(101.0)], now, &Flat).is_empty() {
                hits.push(now);
            }
            now += TICK_MS;
        }
        assert!(hits.len() >= 2, "{hits:?}");
        assert!(hits[0] >= START + PACE.swing_gap_ms.0, "{hits:?}");
        assert!(hits[1] - hits[0] >= PACE.swing_gap_ms.0, "{hits:?}");
        let at_once = mobs
            .spawn_personal(
                spawn_at(100.0, TEST_MOB, TEST_LEVEL),
                PLAYER,
                false,
                Some(PACE),
                None,
                now,
            )
            .expect("rows");
        let ticks_to_try = 3 * PACE.swing_gap_ms.0 / TICK_MS;
        let first = (0..ticks_to_try)
            .map(|tick| now + tick * TICK_MS)
            .find(|at| {
                mobs.tick(&[player_at(101.0)], *at, &Flat)
                    .iter()
                    .any(|intent| intent.mob == at_once)
            })
            .expect("it hits");
        assert!(first >= now + PACE.swing_gap_ms.0);
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
                target: PLAYER,
                target_at: player_at(101.0).position,
                mob_move: None,
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
