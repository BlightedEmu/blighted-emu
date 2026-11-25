// SPDX-License-Identifier: AGPL-3.0-or-later
//! The encounters of the instance an instance sim runs (dungeon2 study, run 15): the stages of
//! each encounter of its game mode's level script ([`nw_world::dungeon_scripts`]), their spawn
//! timelines, objectives and actions, the encounter's reset at a wipe, and its end: the loot
//! of the loot containers it opens, and the instance's completion after the last one.
//!
//! The level script's own layout is data (encounter slices, `EncounterManagerComponent`,
//! `EncounterComponent`); how the server runs it is not in the client. The rules here follow
//! the class names, the fields and their values (Inferred, dungeon2 REPORT):
//!
//! - an encounter with `m_autoActivate` starts with the instance (after its delay); another
//!   starts when a living player comes within its volume's radius;
//! - its top stages run in order; a stage runs its spawn timeline and its start actions, then
//!   its child stages in order; it ends when its own objectives are done and (unless its
//!   children are optional) its children ended; a stage without objectives of its own ends with
//!   its children; a child whose objectives all wait for the parent runs beside the others and
//!   ends with the parent;
//! - an objective's time limit ends it (`m_succeedOnTimeout`) or fails its stage; a failed
//!   stage or a wipe (every player in the instance dead for the encounter's grace period)
//!   resets the encounter: its mobs go and it starts again.
//!
//! The open world's encounters (its spawn groups: `WorldEvent_SpawnGroup_Single_Master` and
//! the others of [`nw_world::dungeon_scripts::OPEN_WORLD_LEVEL`]) run by the same stages, with
//! the open world's own start and end ([`OpenWorldRules`]; retail `12-8_demonstorm` and
//! `12-9_demonstorm2`: the storm priest shows `Spawn` when the player comes within 84 m, and
//! his swarmers come two at a time, 15 s apart, as his `Sub-Phase_01 - Spawn Swarmer Portals`
//! stage spawns them):
//!
//! - an encounter starts when a player comes within the sim's wake radius of it, not with the
//!   sim (Inferred: 329 encounters hold 1,875 mobs that would stand awake with no player near);
//! - when no player is that near for its grace period, it resets and waits again;
//! - an ended encounter starts again after its spawner's respawn time: a time in the spawn
//!   group's area spawner's `m_minRespawnRange` to `m_maxRespawnRange` (seconds), or the mob
//!   respawn time when its spawner has no range (Inferred);
//! - an encounter whose spawner does not spawn when it is enabled (`m_spawnOnEnable` 0: it
//!   waits for its trigger) does not start when a player comes near;
//! - a sim runs the encounters of its own cells.
//!
//! The corrupted breaches are the open world encounters of an alias spawner (breaches note
//! 2026-11-11): each `Darkness_SpawnGroup_Minor_Master` spawns one of four minor breach slices
//! (monolith, portal, grove, hive) at one of its seven spawn points, each
//! `Darkness_SpawnGroup_Major_Master` one of two major ones (portal, monolith) at its own place
//! (Observed: the slices, and the retail breaches stood at those places). The script holds one
//! encounter per slice and point, of one spawn group ([`Encounter::spawn_group`]):
//!
//! - one encounter of a group is its choice; the others wait idle and show no root. When the
//!   choice comes back after its respawn time, the group picks again (Inferred: `m_liveCount` 1,
//!   the alias' equal `Percent` weights; the retail pick is Unknown);
//! - a group chooses only encounters whose every objective can be met at their difficulty: each
//!   tracked target, and each spawn of a stage that counts its own spawns, has a slice of that
//!   difficulty that is a mob or a start object ([`completable`]). The carapaces, membranes and
//!   difficulty 5 and 6 cores have no retail entity table yet, so the breaches that need them
//!   never start (an encounter that cannot end would hold its place's mobs for nothing);
//! - a breach runs at its place's difficulty ([`Encounter::difficulty`], Observed per place from
//!   the retail world map): its spawns of a target with one slice per difficulty spawn that
//!   difficulty's slice and mob ([`TargetSlice::difficulty`], [`TargetSlice::difficulty_mobs`]);
//! - its success fires its difficulty's reward event ([`Encounter::rewards`]) for each player
//!   within the rewards' radius (Observed: each won retail breach gave the own player
//!   `Darkness_<Minor|Major>_DT<n>_T<k>` at the manager's succeeded status, 16 to 50 ms after
//!   it; the radius rule is Inferred from the slice's 500 m field). The list's
//!   `Darkness_Any_GlobalSuccess` goes to no player (Observed: none in the own game events of
//!   28 won breaches and 3 won Myrkgard events, batch E2 `breach_events.txt`).
//!
//! The darkness events of Myrkgard (findings `2026-11-13-catacomb-erosion.md` section E2) are
//! open world encounters of their spawn slices (`Enc_Darkness_Major_Portal_SM_01_Spawn` and four
//! more: two portals, three monoliths), at difficulty 6 (Observed: their portals' cores were
//! `darkness_core_major_portal_dt6`). Each one's reward list names one event, its key fragment,
//! given at any difficulty (Observed, `Playthrough_Part_18`: `Event_MyrkgardKeyFragment5` at the
//! north portal's success, unix 1793539616.726 s, 21 ms before its manager's succeeded status
//! reached the player; `Event_MyrkgardKeyFragment2` at the east monolith's, 1793539736.349 s).
//!
//! - A target of no mob (a hazard) of several difficulties spawns the slice of the highest one
//!   not above the encounter's (Inferred: the hazard aliases hold `_l1` to `_l4` only, and the
//!   difficulty 6 Myrkgard events gave `ApplyCurseL4`); a mob target keeps the slice of the
//!   encounter's own difficulty (above); a target of no mob spawns each of its slices once,
//!   though its alias names one more than once (Observed data: the minor hazard alias names
//!   `_l4` three times; retail players held one `ApplyCurseL4`, application 1).
//! - The effect areas of an encounter's slice and of the slices its stages spawn
//!   ([`nw_world::effect_areas::EffectAreas::slices`]) stand while their slice does
//!   ([`ScriptHost::open_slice_areas`]): an encounter's own from the time it is shown, a stage
//!   spawn's from its spawn; a reset takes the spawns' areas away, and both go
//!   [`AREAS_AFTER_SUCCESS_MS`] after the success (Observed: the own player lost the north
//!   portal's `ApplyCurseL4` 10.2 s after its manager's success, `Playthrough_Part_18`
//!   1427.3 s). Inferred: an encounter whose root another sim shows, or that its group did not
//!   choose, shows no areas here.

use bevy_ecs::entity::Entity;
use nw_bus::Point;
use nw_sim_core::Position;
use nw_world::catacomb_tiles::turned_point;
use nw_world::collision::Geometry;
use nw_world::dungeon_scripts::{
    Encounter, EncounterTasks, HazardObject, LevelScript, OPEN_WORLD_LEVEL, ObjectiveSpec,
    RandomizerChoice, Spawn, SpawnRandomizer, StageAction, Target, TargetMob, TargetSlice,
};
use nw_world::records::PatrolPath;
use uuid::Uuid;

use super::encounter_roots::EncounterObjects;
use super::loot_bag::LimitsSent;
use super::{Body, Player, SimWorld, Vitals};
use crate::mob_ai::script_mob_ai;
use crate::objectives::Event;

/// What the name of an encounter's intro stage holds, in lower case (`Intro 1 - Play`,
/// `01 - Intro - Brute Goliath`, `Stage1_Intro`; [`DungeonRun::boss_intro_over`]).
pub(super) const INTRO_STAGE_MARK: &str = "intro";

/// Milliseconds in a second.
const MS_PER_S: u64 = 1_000;
const MS_PER_S_F64: f64 = 1_000.0;
/// The SplitMix64 steps (Steele, Lea and Flood) that pick a respawn time in its range.
const SPLITMIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const SPLITMIX_MUL_A: u64 = 0xBF58_476D_1CE4_E5B9;
const SPLITMIX_MUL_B: u64 = 0x94D0_49BB_1331_11EB;
/// The high bits of a mixed value that make the share: 53, a double's mantissa.
const SHARE_BITS: u32 = 53;
const SHARE_SHIFT: u32 = u64::BITS - SHARE_BITS;
/// `EventType` values of an entity event (`slice_triggers`).
pub(super) const EVENT_TRIGGER: u32 = 0;
pub(super) const EVENT_ACTIVATE: u32 = 2;
pub(super) const EVENT_DEACTIVATE: u32 = 3;
/// The volume radius of an encounter whose slice has none: the most common sphere radius of the
/// encounter slices' detection volumes (Inferred).
const DEFAULT_VOLUME_RADIUS_M: f32 = 30.0;
/// How long a dead encounter mob's body stays before it goes (Inferred: the retail bodies of
/// the tutorial stayed 5.1 to 7.1 s after their death, encounter note).
pub(super) const CORPSE_MS: u64 = 7_000;
/// The blackboard key of a boss's phase (behavior-tree study, engine key table).
const BOSS_PHASE_KEY: &str = "Boss_Phase";
/// The potency of a hazard object's area effects (Inferred: the effect's own potency; the
/// persistent area status effect names none).
const HAZARD_EFFECT_POTENCY: f64 = 1.0;
/// Spawns of one GM spawn call.
const ONE: u32 = 1;
/// How long an encounter's areas stand after its success (module text), ms.
const AREAS_AFTER_SUCCESS_MS: u64 = 10_200;
/// A reward list of one event gives it at every difficulty (module text).
const ONE_EVENT: usize = 1;
/// The radius around an encounter within which its success counts for a player's quests when
/// the encounter names no player volume, metres (Inferred: the 30 m volume of the retail quest
/// encounters `Enc_Quest_04A_Side02_ObeliskDefenders_01` and `WorldEvent_Enc_99B_MSQ_YonasRetreat`).
const DEFAULT_SUCCESS_RADIUS_M: f32 = 30.0;

/// What the script asks of the sim.
pub(super) trait ScriptHost {
    /// Spawns mob `mob` at `at` with the waypoints `waypoints` of its spawner slice; its
    /// entity, or `None` when it cannot spawn.
    fn spawn(
        &mut self,
        mob: &TargetMob,
        at: [f32; 3],
        waypoints: Option<&PatrolPath>,
    ) -> Option<u64>;
    /// Spawns a start object of slice `slice` at `at`, turned `yaw_deg` (a target slice
    /// without a mob); its entity, or `None` when the slice is no start object.
    fn spawn_object(&mut self, slice: &str, at: [f32; 3], yaw_deg: f32) -> Option<u64>;
    /// Removes encounter mob or start object `entity`.
    fn remove(&mut self, entity: u64);
    /// Gives status effects `effects`, from `source`, to the living players within `radius_m`
    /// of `at` but those of `skip`, at server time `now_ms`; the players it gave them to.
    fn afflict(
        &mut self,
        sphere: ([f32; 3], f32),
        effects: &[String],
        source_at: (u64, u64),
        skip: &[u64],
    ) -> Vec<u64>;
    /// Ends status effects `effects` of `source` on `players` at server time `now_ms`.
    fn end_effects(&mut self, players: &[u64], effects: &[String], source: u64, now_ms: u64);
    /// The health of mob `entity` (a standing start object has [`OBJECT_HEALTH`]); `None` when
    /// it is gone.
    fn health(&self, entity: u64) -> Option<u32>;
    /// The boss phase of mob `entity`.
    fn boss_phase(&self, entity: u64) -> Option<u32>;
    /// Whether a player gathers start object `entity` now; a host where no one gathers says
    /// no.
    fn being_gathered(&self, _entity: u64) -> bool {
        false
    }
    /// A random number below `below`.
    fn pick(&mut self, below: u32) -> u32;
    /// Opens the effect areas of slice `slice` standing at `at`, turned `yaw_deg`: a handle for
    /// their close, or `None` when the slice has none. A host without effect areas opens none.
    fn open_slice_areas(&mut self, _slice: &str, _at: [f32; 3], _yaw_deg: f32) -> Option<u64> {
        None
    }
    /// Closes the effect areas of handle `handle`.
    fn close_slice_areas(&mut self, _handle: u64) {}
    /// Mob `mob`, just spawned by an open world encounter, takes the nearest living player for
    /// whom `in_volume` holds as its target. A host without players does nothing.
    fn engage(&mut self, _mob: u64, _in_volume: &dyn Fn(Position) -> bool) {}
}

/// The health a standing start object reports: it is not dead until it is gathered.
pub(super) const OBJECT_HEALTH: u32 = 1;

/// What the players of the instance are doing in one tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct ScriptView {
    /// The places of the living players.
    pub(super) living: Vec<Position>,
    /// The places of every player, living or dead.
    pub(super) everyone: Vec<Position>,
    /// Some player is in the instance (living or dead).
    pub(super) present: bool,
    /// The encounter events the mobs' trees sent: (mob, event id).
    pub(super) events: Vec<(u64, String)>,
}

/// Where a stage is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum StageStatus {
    #[default]
    Pending,
    Running,
    Done,
    Failed,
}

/// One task of an encounter's objective as the encounter's run stands
/// ([`DungeonRun::objective_phase`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TaskRun {
    /// When its stage started; `None` before it started.
    pub(super) started_ms: Option<u64>,
    /// The count it reached (deaths, spawns; [`objective_count`]); 0 for an objective that does
    /// not count.
    pub(super) count: u32,
    /// It counts (SliceDestroyed, SliceSpawned).
    pub(super) counts: bool,
    /// Its objective has a time limit.
    pub(super) timed: bool,
    /// It is done.
    pub(super) done: bool,
}

/// Where the objective of an encounter's root stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ObjectivePhase {
    /// The encounter runs: each task's run, in task order.
    Running(Vec<TaskRun>),
    /// The encounter ended.
    Ended,
}

/// The run of one stage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StageRun {
    status: StageStatus,
    started_ms: u64,
    /// Spawns of its timeline not made yet: (due time, index in the stage's spawns).
    due: Vec<(u64, usize)>,
    /// The next child stage to run.
    child: usize,
    /// Which of its objectives are done.
    done: Vec<bool>,
}

/// Where an encounter is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum EncounterStatus {
    #[default]
    Idle,
    /// Its first stage starts at this time.
    Due(u64),
    Running,
    Done,
}

/// One mob an encounter spawned.
#[derive(Debug, Clone, PartialEq)]
struct EncounterMob {
    entity: u64,
    /// Its target and the slice of it.
    target: usize,
    slice: usize,
    /// The stage that spawned it.
    stage: usize,
    /// When it died.
    died_ms: Option<u64>,
    /// Its body went.
    gone: bool,
    /// The point of its area spawner it stands at, while its death has no comeback yet
    /// ([`nw_world::dungeon_scripts::AreaRefill`]); `None` for another mob.
    refill_at: Option<[f32; 3]>,
    /// When the slice spawn it came from ends and takes it along
    /// ([`nw_world::dungeon_scripts::TargetSlice::lifetime_s`]); `None` for a mob that stays.
    /// Such a mob is its slice's, not its stage's: the stage's `KillSpawns` leaves it. Observed:
    /// the lightning hazard's stage `Wave_02 - Begin Hazard` ends with `KillSpawns` at its 30 s
    /// limit, and its tornadoes went at the slice's 33 s `Destroy` (`CATACOMBS3` 480.6 s, 31 s
    /// after the first tornado, which came 2 s after the slice).
    ends_ms: Option<u64>,
    /// A start object's gathering timeline made it ([`GatherClock`]): it is its stage's mob,
    /// not one of its slice. Observed: the sun disk shield's stage (`Wave_02 - Objective: Kill
    /// AI`, its shield's destruction) went on after its first sulfur elemental died
    /// (`1624_0401_thedemonscauldron` 200.2 s) until the shield's gather ended (220.0 s).
    of_object: bool,
    /// The member of its spawner slice it is ([`TargetSlice::members`]); `None` for a mob of a
    /// slice of one mob kind.
    member: Option<usize>,
}

/// A mob that a slice spawn's own delayed event makes later
/// ([`nw_world::dungeon_scripts::TimedMob`]).
#[derive(Debug, Clone, PartialEq)]
struct TimedDue {
    /// When it spawns.
    due_ms: u64,
    mob: TargetMob,
    /// Its place in the world.
    at: [f32; 3],
    /// The target and the slice of the slice spawn that makes it, and that spawn's stage.
    target: usize,
    slice: usize,
    stage: usize,
    /// When its slice spawn ends ([`EncounterMob::ends_ms`]).
    ends_ms: Option<u64>,
}

/// An effect area of a standing hazard object ([`nw_world::dungeon_scripts::EffectArea`]).
#[derive(Debug, Clone, PartialEq)]
struct HazardArea {
    /// The hazard object, the source of its effects.
    object: u64,
    /// Its centre.
    at: [f32; 3],
    effects: Vec<String>,
    radius_m: f32,
    /// When it turns on and off.
    on_ms: u64,
    off_ms: u64,
    /// The players it gave its effects to. Observed: `m_reapplicationDelaySeconds` 10 s, longer
    /// than the debris's life, so each player takes them once per object.
    given: Vec<u64>,
}

/// The comeback of a dead mob of an area spawner ([`nw_world::dungeon_scripts::AreaRefill`]).
#[derive(Debug, Clone, PartialEq)]
struct Refill {
    /// When it comes back.
    due_ms: u64,
    /// Its target and the slice of it.
    target: usize,
    slice: usize,
    /// The stage that spawned it; it comes back only while that stage runs.
    stage: usize,
    /// Its point.
    at: [f32; 3],
}

/// A spawner a prefab spawn randomizer picked ([`SpawnRandomizer`]): it keeps one mob of its
/// choice and brings it back after each death, while the stage that spawned it runs, until a
/// detectable object reaches it ([`Deactivator`]).
#[derive(Debug, Clone, PartialEq)]
struct PickedSpawner {
    /// The target and the slice of the randomizer, and the stage that spawned it.
    target: usize,
    slice: usize,
    stage: usize,
    /// The index of its choice in [`SpawnRandomizer::choices`].
    choice: usize,
    /// Its place in the world.
    at: [f32; 3],
    /// Its mob, while one stands.
    mob: Option<u64>,
    /// When its next mob comes, after a death.
    due_ms: Option<u64>,
    /// A detectable object reached it: it is off and brings no mob back.
    off: bool,
    /// When its mob goes: the end of the detectable object that turned it off.
    leaves_ms: Option<u64>,
}

/// An area trigger slice that a stage spawned: a stage objective that tracks it (`SliceDestroyed`)
/// counts it once a living player came into it.
#[derive(Debug, Clone, PartialEq)]
struct AreaTrigger {
    /// Its target.
    target: usize,
    /// Its place.
    at: [f32; 3],
    /// When a player came into it.
    fired_ms: Option<u64>,
}

/// The file name start of an encounter's area trigger slice
/// (`slices/encounters/ai_encounter_area_trigger` and its named kinds `_20m`, `_sol`, `_yonas`):
/// a trigger volume with no mob, which a stage that waits for players spawns and tracks as
/// destroyed (`QuestMedusa`'s and `QuestDracula`'s `Wave 0 - Setup`, and 18 more encounters of
/// trials and the open world). Live 2026-11-24 05:44-05:52Z: the Medusa trial never left its
/// setup, as no mob of that target could die; 08:23Z: Dracula's `_20m` trigger was taken for a
/// slice of no mob, and Cecilia never came.
const AREA_TRIGGER_STEM: &str = "ai_encounter_area_trigger";

/// Whether slice `path` is an encounter's area trigger ([`AREA_TRIGGER_STEM`]).
pub(super) fn is_area_trigger(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|file| file.starts_with(AREA_TRIGGER_STEM) && file.ends_with(SLICE_END))
}

/// The radius of an area trigger slice, m (**Inferred**: its volume is in the slice, which the
/// content does not read; the retail trial starts came at 10 to 27 m: Isabella's `AreaBossStart`
/// box, Adiana's 27 m `AreaBossFightStart` sphere).
const AREA_TRIGGER_RADIUS_M: f32 = 20.0;

/// A detectable object an encounter spawned ([`TargetSlice::detectable_radius_m`]).
#[derive(Debug, Clone, PartialEq)]
struct Deactivator {
    /// Its place and the radius of its sphere.
    at: [f32; 3],
    radius_m: f32,
    /// When it ends ([`TargetSlice::lifetime_s`]); `None` for one that stays.
    ends_ms: Option<u64>,
}

/// The timeline of a standing start object that spawns mobs while a player gathers it
/// ([`TargetSlice::gather_mobs`]): it plays while the object is gathered and holds its place
/// between gathers.
#[derive(Debug, Clone, PartialEq)]
struct GatherClock {
    /// The start object.
    object: u64,
    /// Its target and slice, and the stage that spawned it.
    target: usize,
    slice: usize,
    stage: usize,
    /// Its place and turn in the world.
    at: [f32; 3],
    yaw_deg: f32,
    /// How far the timeline played, milliseconds.
    played_ms: u64,
    /// The server time of its last tick.
    last_ms: u64,
    /// The next of its mobs to spawn.
    next: usize,
}

/// The run of one encounter.
#[derive(Debug, Clone, Default, PartialEq)]
struct EncounterRun {
    status: EncounterStatus,
    /// The top stage running.
    top: usize,
    stages: Vec<StageRun>,
    mobs: Vec<EncounterMob>,
    /// The comebacks of its area spawners' dead mobs not made yet.
    refills: Vec<Refill>,
    /// The mobs its slice spawns' delayed events make later.
    timed: Vec<TimedDue>,
    /// The effect areas of its standing hazard objects ([`HazardObject`]).
    hazard_areas: Vec<HazardArea>,
    /// The spawners its slice spawns' randomizers picked.
    picked: Vec<PickedSpawner>,
    /// The detectable objects it spawned.
    deactivators: Vec<Deactivator>,
    /// The area trigger slices it spawned ([`is_area_trigger`]).
    area_triggers: Vec<AreaTrigger>,
    /// The gathering timelines of its standing start objects.
    gather_clocks: Vec<GatherClock>,
    /// The `Family` its mobs keep, once a spawn picked one
    /// ([`nw_world::dungeon_scripts::TargetSlice::family`]); `None` before its first spawn.
    family: Option<String>,
    /// Entity events its actions sent: (entity, event, time).
    entity_events: Vec<(u64, u32, u64)>,
    /// Tree events of its mobs: (event id, time).
    events: Vec<(String, u64)>,
    /// A living player is within its volume.
    engaged: bool,
    /// Since when every player is dead.
    wiped_since_ms: Option<u64>,
    /// Its end went out as an outcome.
    reported: bool,
    /// Open world: since when no player is near.
    left_since_ms: Option<u64>,
    /// Open world: when it ended.
    done_ms: Option<u64>,
    /// When its last top stage ended.
    ended_ms: Option<u64>,
    /// The handles of the effect areas its stage spawns opened.
    spawn_areas: Vec<u64>,
    /// The handle of its own slice's effect areas while they stand.
    own_areas: Option<u64>,
    /// Its slice has no effect areas of its own (a host gave no handle).
    no_own_areas: bool,
    /// It runs in the open world (set at each of its ticks there).
    open_world: bool,
}

/// How the encounters of the open world start and end (see the module text).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct OpenWorldRules {
    /// A player this near starts an encounter and keeps it.
    pub(super) near_m: f32,
    /// An ended encounter starts again after this time.
    pub(super) respawn_ms: u64,
}

/// What a tick of the script did that the sim acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScriptOutcome {
    /// The encounter of this index ended.
    EncounterDone(usize),
    /// Every encounter ended.
    Completed,
}

/// The script of the running instance.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DungeonRun {
    instance: Uuid,
    script: LevelScript,
    encounters: Vec<EncounterRun>,
    completed: bool,
    /// The rules of the open world; `None` for an instance.
    open_world: Option<OpenWorldRules>,
    /// The entity events the stages sent since the last take: (encounter, entity, event).
    sent_events: Vec<(usize, u64, u32)>,
    /// The spawn groups of the encounters ([`Encounter::spawn_group`]).
    groups: Vec<SpawnGroup>,
    /// The group of each encounter, by index.
    group_of: Vec<Option<usize>>,
    /// Whether it runs the daily twin of each encounter that has one ([`trial_variant`]).
    daily: bool,
}

/// The encounters of one alias spawner: one of them is its choice at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpawnGroup {
    /// Its encounters, by index.
    members: Vec<usize>,
    /// The member that is its choice (an index into `members`).
    chosen: usize,
}

impl SpawnGroup {
    /// The member picked by `seed` (a share of the members by [`mixed`]).
    fn pick(&mut self, seed: u64) {
        let count = u64::try_from(self.members.len()).unwrap_or(u64::MAX).max(1);
        self.chosen = usize::try_from(mixed(seed) % count).unwrap_or_default();
    }

    fn choice(&self) -> Option<usize> {
        self.members.get(self.chosen).copied()
    }
}

/// The end of the slice path of a daily solo trial encounter; the slice path without it (and
/// with [`SLICE_END`]) is its quest twin (`enc_quest_abominableliangboss_daily` and
/// `enc_quest_abominableliangboss` in the `QuestLiang` script, 12 trial game modes).
const DAILY_SLICE_END: &str = "_daily.dynamicslice";
/// The end of a slice path.
const SLICE_END: &str = ".dynamicslice";
/// The end of the name of a daily solo trial encounter (lowercase): `EncounterBossDaily` is the
/// daily twin of `EncounterBoss`.
const DAILY_NAME_END: &str = "daily";
/// The start of a game mode's daily trial flag (achievement `DailyTrialLiang`).
const DAILY_TRIAL_FLAG: &str = "DailyTrial";
/// The end of the flag that the daily trial was done (`DailyTrialLiangComplete`).
const DAILY_DONE_END: &str = "Complete";
/// The game mode id starts that the daily trial flag leaves out (`QuestLiang` ->
/// `DailyTrialLiang`, `TrialSeason04Q02` -> `DailyTrialSeason04Q02`).
const TRIAL_MODE_STARTS: [&str; 2] = ["Quest", "Trial"];

/// The daily trial flag of game mode `game_mode` and its done flag.
pub(super) fn daily_trial_flags(game_mode: &str) -> (String, String) {
    let name = TRIAL_MODE_STARTS
        .iter()
        .find_map(|start| game_mode.strip_prefix(start))
        .unwrap_or(game_mode);
    let flag = format!("{DAILY_TRIAL_FLAG}{name}");
    let done = format!("{flag}{DAILY_DONE_END}");
    (flag, done)
}

/// `script` with one encounter of each daily twin pair: the daily one when `daily`, else the
/// quest one; encounters with no twin stay. Observed: retail ran one of the two per run, the
/// quest one in the main story runs (`99f_msq_er_fightliang`, `MSQ19-lv65`,
/// `Playthrough_Part_07`: asset `5e64fa12...` of `enc_quest_abominableliangboss`;
/// `99g_msq_sm_destroyenemy`: `db197547...` of `enc_quest_isabella_msq2`) and the daily one in
/// the soul trial repeats (`soultrialhealervg`, `Soul-Trial---Tempest-Mind`: `2e8415ee...` of
/// `enc_quest_isabella_msq2_daily`). Both encounters' entities in the script slice are alike, so
/// the rule that picks one is Inferred from the trial doors' conditions
/// (`99F_Sun_Greet_FightLiang_00 || (DailyTrialLiang && !DailyTrialLiangComplete)`).
fn trial_variant(mut script: LevelScript, daily: bool) -> LevelScript {
    let slices: std::collections::HashSet<String> = script
        .encounters
        .iter()
        .map(|encounter| encounter.slice.to_lowercase())
        .collect();
    let names: std::collections::HashSet<String> = script
        .encounters
        .iter()
        .map(|encounter| encounter.name.to_lowercase())
        .collect();
    script.encounters.retain(|encounter| {
        // The twins by name (`EncounterBoss` and `EncounterBossDaily`): `QuestMorgaine`'s two
        // name one slice, `enc_morgaine` (live 2026-11-24 06:41:18Z and 06:41:22Z: both ran and
        // the bot fought two Morgaines).
        let name = encounter.name.to_lowercase();
        if let Some(stem) = name.strip_suffix(DAILY_NAME_END)
            && names.contains(stem)
        {
            return daily;
        }
        if names.contains(&format!("{name}{DAILY_NAME_END}")) {
            return !daily;
        }
        let slice = encounter.slice.to_lowercase();
        if let Some(stem) = slice.strip_suffix(DAILY_SLICE_END) {
            return daily || !slices.contains(&format!("{stem}{SLICE_END}"));
        }
        let has_daily = slice
            .strip_suffix(SLICE_END)
            .is_some_and(|stem| slices.contains(&format!("{stem}{DAILY_SLICE_END}")));
        !(daily && has_daily)
    });
    script
}

/// The spawn groups of `script`'s encounters and the group of each encounter whose objectives can
/// be met (`can_end`, by index); each group's first choice is picked by its index.
fn spawn_groups(script: &LevelScript, can_end: &[bool]) -> (Vec<SpawnGroup>, Vec<Option<usize>>) {
    let mut by_name: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut groups: Vec<SpawnGroup> = Vec::new();
    let group_of = script
        .encounters
        .iter()
        .enumerate()
        .map(|(index, encounter)| {
            let name = encounter.spawn_group.as_deref()?;
            let group = *by_name.entry(name).or_insert_with(|| {
                groups.push(SpawnGroup {
                    members: Vec::new(),
                    chosen: 0,
                });
                groups.len() - 1
            });
            if can_end.get(index).copied().unwrap_or(true) {
                groups[group].members.push(index);
            }
            Some(group)
        })
        .collect();
    for (index, group) in (0_u64..).zip(groups.iter_mut()) {
        group.pick(index);
    }
    (groups, group_of)
}

impl DungeonRun {
    /// The run of `script` for instance `instance`: every encounter idle.
    pub(super) fn new(instance: Uuid, script: LevelScript) -> Self {
        let encounters = script
            .encounters
            .iter()
            .map(|encounter| EncounterRun {
                stages: vec![StageRun::default(); encounter.stages.len()],
                ..EncounterRun::default()
            })
            .collect();
        let (groups, group_of) = spawn_groups(&script, &[]);
        Self {
            instance,
            script,
            encounters,
            completed: false,
            open_world: None,
            sent_events: Vec::new(),
            groups,
            group_of,
            daily: false,
        }
    }

    /// The run of solo trial `script` for instance `instance`: of each encounter with a daily
    /// twin, the daily one when `daily`, else the other ([`trial_variant`]).
    pub(super) fn of_trial(instance: Uuid, script: LevelScript, daily: bool) -> Self {
        Self {
            daily,
            ..Self::new(instance, trial_variant(script, daily))
        }
    }

    /// Whether encounter `index` may run: it is of no spawn group, or its group's choice.
    pub(super) fn chosen(&self, index: usize) -> bool {
        self.group_of
            .get(index)
            .copied()
            .flatten()
            .and_then(|group| self.groups.get(group))
            .is_none_or(|group| group.choice() == Some(index))
    }

    /// The reward of encounter `index` at its difficulty: its place, the radius and its
    /// difficulty's game event, or the list's one event (module text).
    pub(super) fn rewards_of(&self, index: usize) -> Option<([f32; 3], f32, &str)> {
        let encounter = self.script.encounters.get(index)?;
        let rewards = encounter.rewards.as_ref()?;
        let difficulty = usize::try_from(encounter.difficulty?).ok()?;
        let event = difficulty
            .checked_sub(1)
            .and_then(|at| rewards.events.get(at))
            .or_else(|| {
                rewards
                    .events
                    .first()
                    .filter(|_| rewards.events.len() == ONE_EVENT)
            })
            .filter(|event| !event.is_empty())?;
        Some((encounter.position, rewards.radius_m, event.as_str()))
    }

    /// The success event of encounter `index` (its `m_universalSuccessGameEventId`), with its
    /// place and the radius of its player volume ([`DEFAULT_SUCCESS_RADIUS_M`] when it names
    /// none); `None` for an encounter without one.
    pub(super) fn success_event_of(&self, index: usize) -> Option<([f32; 3], f32, &str)> {
        let encounter = self.script.encounters.get(index)?;
        let event = encounter
            .rewards
            .as_ref()
            .map(|rewards| rewards.global_event.as_str())
            .filter(|event| !event.is_empty())?;
        let radius_m = encounter
            .detection_radius_m
            .unwrap_or(DEFAULT_SUCCESS_RADIUS_M);
        Some((encounter.position, radius_m, event))
    }

    /// The run of the open world's `script` by `rules`: every encounter idle; a spawn group
    /// chooses among its encounters that [`completable`] finds can end with the start object
    /// slices `is_object`.
    pub(super) fn of_open_world(
        script: LevelScript,
        rules: OpenWorldRules,
        is_object: &impl Fn(&str) -> bool,
    ) -> Self {
        let can_end: Vec<bool> = script
            .encounters
            .iter()
            .map(|encounter| encounter.spawn_group.is_none() || completable(encounter, is_object))
            .collect();
        let (groups, group_of) = spawn_groups(&script, &can_end);
        Self {
            open_world: Some(rules),
            groups,
            group_of,
            ..Self::new(Uuid::nil(), script)
        }
    }

    /// Its level script.
    pub(super) fn script(&self) -> &LevelScript {
        &self.script
    }

    /// Whether its run ended (every encounter done, or an expedition's last boss task).
    pub(super) const fn completed(&self) -> bool {
        self.completed
    }

    /// Whether encounter `index` is done.
    pub(super) fn encounter_done(&self, index: usize) -> bool {
        self.encounters
            .get(index)
            .is_some_and(|run| run.status == EncounterStatus::Done)
    }

    /// The place in its top order of the top stage encounter `index` runs; `None` when it does
    /// not run.
    pub(super) fn running_top(&self, index: usize) -> Option<usize> {
        let run = self.encounters.get(index)?;
        (run.status == EncounterStatus::Running).then_some(run.top)
    }

    /// Whether a living player is within encounter `index`'s volume.
    pub(super) fn engaged(&self, index: usize) -> bool {
        self.encounters.get(index).is_some_and(|run| run.engaged)
    }

    /// The entity events its stages sent since the last call: (encounter, entity, event).
    pub(super) fn take_entity_events(&mut self) -> Vec<(usize, u64, u32)> {
        std::mem::take(&mut self.sent_events)
    }

    /// Where the objective of encounter `index` with task tree `tasks` stands: `None` while it
    /// is idle or waits, or runs before any task's stage started (Observed: the objective is
    /// made when its first task's stage starts, encounter objectives note).
    pub(super) fn objective_phase(
        &self,
        index: usize,
        tasks: &EncounterTasks,
    ) -> Option<ObjectivePhase> {
        let encounter = self.script.encounters.get(index)?;
        let run = self.encounters.get(index)?;
        match run.status {
            EncounterStatus::Idle | EncounterStatus::Due(_) => return None,
            EncounterStatus::Done => return Some(ObjectivePhase::Ended),
            EncounterStatus::Running => {}
        }
        let runs: Vec<TaskRun> = tasks
            .tasks
            .iter()
            .map(|task| {
                let stage = run.stages.get(task.stage);
                let objective = encounter
                    .stages
                    .get(task.stage)
                    .and_then(|def| def.objectives.get(task.objective));
                let started_ms = stage
                    .filter(|stage| stage.status != StageStatus::Pending)
                    .map(|stage| stage.started_ms);
                let count = objective.zip(started_ms).and_then(|(objective, started)| {
                    objective_count(encounter, run, task.stage, &objective.kind, started)
                });
                let required = objective.and_then(|objective| match objective.kind {
                    ObjectiveSpec::SliceDestroyed { required, .. }
                    | ObjectiveSpec::SliceSpawned { required, .. } => Some(required.max(ONE)),
                    _ => None,
                });
                TaskRun {
                    started_ms,
                    count: count.unwrap_or_default().min(required.unwrap_or(u32::MAX)),
                    counts: required.is_some(),
                    timed: objective.is_some_and(|objective| objective.time_limit_s > 0),
                    done: stage.is_some_and(|stage| {
                        stage.done.get(task.objective).copied().unwrap_or_default()
                    }),
                }
            })
            .collect();
        runs.iter()
            .any(|task| task.started_ms.is_some())
            .then_some(ObjectivePhase::Running(runs))
    }

    /// The client statuses of encounter `index` ([`super::encounter_roots`]): its manager's,
    /// and each stage's in stage order. An idle or due encounter has not started; a done one
    /// succeeded.
    pub(super) fn client_status(&self, index: usize) -> Option<(u32, Vec<u32>)> {
        use super::encounter_roots::{
            STATUS_FAILED, STATUS_IN_PROGRESS, STATUS_NOT_STARTED, STATUS_SUCCEEDED,
        };
        let run = self.encounters.get(index)?;
        let manager = match run.status {
            EncounterStatus::Idle | EncounterStatus::Due(_) => STATUS_NOT_STARTED,
            EncounterStatus::Running => STATUS_IN_PROGRESS,
            EncounterStatus::Done => STATUS_SUCCEEDED,
        };
        let stages = run
            .stages
            .iter()
            .map(|stage| match stage.status {
                StageStatus::Pending => STATUS_NOT_STARTED,
                StageStatus::Running => STATUS_IN_PROGRESS,
                StageStatus::Done => STATUS_SUCCEEDED,
                StageStatus::Failed => STATUS_FAILED,
            })
            .collect();
        Some((manager, stages))
    }

    /// The living mobs of encounter `index`: each one's entity and its target's index.
    pub(super) fn living_mobs_of(&self, index: usize) -> Vec<(u64, usize)> {
        self.encounters
            .get(index)
            .map(|run| {
                run.mobs
                    .iter()
                    .filter(|mob| !mob.gone && mob.died_ms.is_none())
                    .map(|mob| (mob.entity, mob.target))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The live mobs of every encounter.
    pub(super) fn mobs(&self) -> impl Iterator<Item = u64> + '_ {
        self.encounters
            .iter()
            .flat_map(|run| &run.mobs)
            .filter(|mob| !mob.gone)
            .map(|mob| mob.entity)
    }

    /// Where the intro of the encounter that spawned mob `mob` is, when `mob` is a boss of it
    /// (the mob of a target that an objective of one of its top stages tracks) and the
    /// encounter has intro stages (a stage whose name holds [`INTRO_STAGE_MARK`]): `false` while
    /// none of them has started or one runs, `true` once one started and none runs. `None` for
    /// another mob.
    pub(super) fn boss_intro_over(&self, mob: u64) -> Option<bool> {
        let (encounter, run, own) = self
            .script
            .encounters
            .iter()
            .zip(&self.encounters)
            .find_map(|(encounter, run)| {
                let own = run.mobs.iter().find(|known| known.entity == mob)?;
                Some((encounter, run, own))
            })?;
        let boss = encounter.top_stages.iter().any(|&top| {
            encounter.stages.get(top).is_some_and(|stage| {
                stage
                    .objectives
                    .iter()
                    .any(|objective| match objective.kind {
                        ObjectiveSpec::SliceDestroyed {
                            target: Some(target),
                            ..
                        } => is_of(encounter, own, target),
                        _ => false,
                    })
            })
        });
        let intros: Vec<StageStatus> = encounter
            .stages
            .iter()
            .zip(&run.stages)
            .filter(|(stage, _)| stage.name.to_lowercase().contains(INTRO_STAGE_MARK))
            .map(|(_, stage)| stage.status)
            .collect();
        if !boss || intros.is_empty() {
            return None;
        }
        let started = intros.iter().any(|status| *status != StageStatus::Pending);
        let running = intros.contains(&StageStatus::Running);
        Some(started && !running)
    }

    /// When the stage named `stage` of the encounter that spawned mob `mob` started, while it
    /// runs or after it ended; `None` before it started, or for a mob of no encounter.
    pub(super) fn stage_started_ms(&self, mob: u64, stage: &str) -> Option<u64> {
        let (encounter, run) = self.encounter_of_mob(mob)?;
        let index = encounter
            .stages
            .iter()
            .position(|known| known.name == stage)?;
        let stage = run.stages.get(index)?;
        matches!(stage.status, StageStatus::Running | StageStatus::Done).then_some(stage.started_ms)
    }

    /// The encounter events that the running intro stages (a name that holds
    /// [`INTRO_STAGE_MARK`]) of the encounter that spawned mob `mob` wait for, each with its
    /// stage's start: what the boss sends at the end of its intro.
    pub(super) fn awaited_intro_events(&self, mob: u64) -> Vec<(String, u64)> {
        let Some((encounter, run)) = self.encounter_of_mob(mob) else {
            return Vec::new();
        };
        encounter
            .stages
            .iter()
            .zip(&run.stages)
            .filter(|(stage, state)| {
                state.status == StageStatus::Running
                    && stage.name.to_lowercase().contains(INTRO_STAGE_MARK)
            })
            .flat_map(|(stage, state)| {
                stage
                    .objectives
                    .iter()
                    .filter_map(move |objective| match &objective.kind {
                        ObjectiveSpec::EncounterEvent { event } => {
                            Some((event.clone(), state.started_ms))
                        }
                        _ => None,
                    })
            })
            .collect()
    }

    /// The encounter that spawned mob `mob`, with its run.
    fn encounter_of_mob(&self, mob: u64) -> Option<(&Encounter, &EncounterRun)> {
        self.script
            .encounters
            .iter()
            .zip(&self.encounters)
            .find(|(_, run)| run.mobs.iter().any(|known| known.entity == mob))
    }

    /// One tick at `now_ms`. `owned` names the encounters this sim runs, by index (every one
    /// when it is empty): an encounter of another sim's cell resets here.
    pub(super) fn tick(
        &mut self,
        host: &mut impl ScriptHost,
        view: &ScriptView,
        owned: &[bool],
        now_ms: u64,
    ) -> Vec<ScriptOutcome> {
        let mut outcomes = Vec::new();
        for (index, encounter) in self.script.encounters.iter().enumerate() {
            let group = self.group_of.get(index).copied().flatten();
            let chosen = group
                .and_then(|group| self.groups.get(group))
                .is_none_or(|group| group.choice() == Some(index));
            let run = &mut self.encounters[index];
            let shown = owned.get(index).copied().unwrap_or(true) && chosen;
            // The state before the tick, for the debug lines of its changes (only when they
            // are written).
            let before = (self.open_world.is_some() && tracing::enabled!(tracing::Level::DEBUG))
                .then(|| EncounterState::of(run));
            'tick: {
                sync_own_areas(encounter, run, host, shown, now_ms);
                if !shown {
                    if run.status != EncounterStatus::Idle {
                        reset(run, host, now_ms);
                    }
                    break 'tick;
                }
                if let Some(rules) = self.open_world {
                    let was_done = run.status == EncounterStatus::Done;
                    let runs = open_world_step(encounter, run, host, view, rules, now_ms);
                    if was_done
                        && run.status == EncounterStatus::Idle
                        && let Some(group) = group.and_then(|group| self.groups.get_mut(group))
                    {
                        // It came back after its respawn time: the group picks its next choice.
                        group.pick(now_ms);
                    }
                    if !runs {
                        break 'tick;
                    }
                }
                let events_before = run.entity_events.len();
                let mobs_before = run.mobs.len();
                run.open_world = self.open_world.is_some();
                tick_encounter(encounter, run, host, view, now_ms);
                if let Some(rules) = self.open_world {
                    // Observed (retail `09a_s17_establishdefenses`, 116.8 s): the two prowlers of
                    // `Wave_03` spawned 34 and 37 m from the player, 31 and 32 m from the centre (a
                    // 30 m sphere), and from 1.5 s on ran straight at the player at about 6 m/s.
                    // Inferred: every mob an open world encounter spawns takes the nearest player in
                    // its volume at once (the retail pick among several players is Unknown).
                    let in_volume = |at: Position| detects(encounter, at, rules.near_m);
                    for mob in run.mobs.get(mobs_before..).unwrap_or_default() {
                        if !mob.of_object {
                            host.engage(mob.entity, &in_volume);
                        }
                    }
                }
                for (entity, event, _) in run.entity_events.get(events_before..).unwrap_or_default()
                {
                    self.sent_events.push((index, *entity, *event));
                }
                if run.status == EncounterStatus::Done && !run.reported {
                    run.reported = true;
                    outcomes.push(ScriptOutcome::EncounterDone(index));
                }
            }
            if let Some(before) = before {
                before.log_changes(&EncounterState::of(&self.encounters[index]), encounter);
            }
        }
        let all_done = !self.encounters.is_empty()
            && self
                .encounters
                .iter()
                .all(|run| run.status == EncounterStatus::Done);
        if all_done && !self.completed {
            self.completed = true;
            outcomes.push(ScriptOutcome::Completed);
        }
        outcomes
    }

    /// The loot containers whose nearest encounter is encounter `index` (Inferred: an
    /// encounter's end opens the containers beside it; the level script's own key and gate
    /// logic is server script that the client does not have).
    pub(super) fn chests_of(&self, index: usize) -> impl Iterator<Item = (&str, [f32; 3])> + '_ {
        self.script.chests.iter().filter_map(move |chest| {
            let nearest = self
                .script
                .encounters
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    distance(a.position, chest.position)
                        .total_cmp(&distance(b.position, chest.position))
                })
                .map(|(nearest, _)| nearest)?;
            (nearest == index).then_some((chest.loot_table.as_str(), chest.position))
        })
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f32>()
        .sqrt()
}

fn position([x, y, z]: [f32; 3]) -> Position {
    Position { x, y, z }
}

/// Whether a player at `at` is in `encounter`'s detection volume; an encounter without one
/// (or of a shape not read) detects within `radius_m` of its place.
///
/// A player is in a volume with its feet or with the middle of its body
/// ([`BODY_MIDDLE_M`]; **Inferred**: a volume detects a body, not a point. Retail
/// `99a_msq_03_climbarcturus` fits both: the cylinder of `99A_Enc_MSQ_09_Torment` has its
/// bottom at the floor, z 153.84, and started with the feet at 154.06, not at 152.48).
fn detects(encounter: &Encounter, at: Position, radius_m: f32) -> bool {
    match &encounter.detector {
        Some(detector) => {
            detector.contains([at.x, at.y, at.z])
                || detector.contains([at.x, at.y, at.z + BODY_MIDDLE_M])
        }
        None => at.horizontal_distance(position(encounter.position)) <= radius_m,
    }
}

/// An open world encounter's status and the status of each stage, for its debug lines.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EncounterState {
    status: EncounterStatus,
    stages: Vec<StageStatus>,
}

impl EncounterState {
    /// The state of `run`; an idle encounter's stages all wait, so it holds none (no copy for
    /// the many idle encounters of the open world).
    fn of(run: &EncounterRun) -> Self {
        Self {
            status: run.status,
            stages: if run.status == EncounterStatus::Idle {
                Vec::new()
            } else {
                run.stages.iter().map(|stage| stage.status).collect()
            },
        }
    }

    /// The status of stage `index`: a stage it holds none of waits.
    fn stage(&self, index: usize) -> StageStatus {
        self.stages.get(index).copied().unwrap_or_default()
    }

    /// One debug line for each change from `self` to `now` of `encounter` (live 2026-11-22
    /// 19:18:21: no start object stood at `Enc_WorldEvent_10A_Q3_SpectralTyrant` 90 s after
    /// its success, and no line told how the encounter stood).
    fn log_changes(&self, now: &Self, encounter: &Encounter) {
        if self.status != now.status {
            tracing::debug!(
                encounter = %encounter.name,
                from = ?self.status,
                to = ?now.status,
                "open world encounter status"
            );
        }
        for index in 0..self.stages.len().max(now.stages.len()) {
            let (before, after) = (self.stage(index), now.stage(index));
            if before != after {
                tracing::debug!(
                    encounter = %encounter.name,
                    stage = %encounter.stages.get(index).map_or("", |stage| stage.name.as_str()),
                    from = ?before,
                    to = ?after,
                    "open world encounter stage"
                );
            }
        }
    }
}

/// The open world's part of an encounter's tick: whether the encounter runs its stages in this
/// tick. It waits idle until a player is near, resets when every player left for its grace
/// period, and starts again [`OpenWorldRules::respawn_ms`] after its end.
fn open_world_step(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    view: &ScriptView,
    rules: OpenWorldRules,
    now_ms: u64,
) -> bool {
    let near = view
        .everyone
        .iter()
        .any(|player| detects(encounter, *player, rules.near_m));
    match run.status {
        EncounterStatus::Idle => near && starts_when_enabled(encounter),
        EncounterStatus::Done => {
            bury(run, host, now_ms);
            let done = *run.done_ms.get_or_insert(now_ms);
            if now_ms.saturating_sub(done) >= respawn_ms(encounter, rules, done) {
                reset(run, host, now_ms);
            }
            false
        }
        EncounterStatus::Due(_) | EncounterStatus::Running => {
            run.left_since_ms = (!near).then(|| run.left_since_ms.unwrap_or(now_ms));
            let grace_ms = u64::from(encounter.grace_period_s).saturating_mul(MS_PER_S);
            let left = run
                .left_since_ms
                .is_some_and(|since| now_ms.saturating_sub(since) >= grace_ms);
            if left {
                reset(run, host, now_ms);
            }
            !left
        }
    }
}

/// Whether an open world encounter starts by itself when a player comes near: its manager
/// activates it, or its spawner spawns when it is enabled (or names no rules).
pub(super) fn starts_when_enabled(encounter: &Encounter) -> bool {
    encounter.auto_activate
        || encounter
            .spawner
            .is_none_or(|spawner| spawner.spawn_on_enable)
}

/// The radius around `encounter`'s place within which a player stays in its volume: the radius
/// of its detection sphere, the smaller horizontal half size of its detection box, else its
/// detection radius ([`DEFAULT_VOLUME_RADIUS_M`] without one). An open world encounter resets
/// when no player is in its volume for its grace period ([`open_world_step`]):
/// `WorldEvent_Enc_99B_MSQ_YonasRetreat` (a sphere of 30 m, a grace of 1 s) reset whenever a
/// fight took the player 50 m north to the fort's placed mobs.
pub(super) fn volume_radius_m(encounter: &Encounter) -> f32 {
    match encounter.detector.as_ref().map(|detector| &detector.shape) {
        Some(Geometry::Sphere { radius }) => *radius,
        Some(Geometry::Box { half_extents }) => half_extents[0].min(half_extents[1]),
        _ => encounter
            .detection_radius_m
            .unwrap_or(DEFAULT_VOLUME_RADIUS_M),
    }
}

/// The radius around `encounter`'s place that lies in its volume all around: its volume's
/// radius ([`volume_radius_m`]) less the distance of the volume's middle from that place. A
/// fight plan names it. Live 2026-11-25 14:46:48Z: the sphere of `WorldEvent_09A_M5_WaveEvent`
/// (45 m, grace 0 s) has its middle 10 m from the encounter's place; a player 45 m from the
/// place was out of it, and the encounter reset in its last wave.
pub(super) fn fight_radius_m(encounter: &Encounter) -> f32 {
    const PLACE: usize = 3;
    let off_middle = encounter.detector.as_ref().map_or(0.0, |detector| {
        let middle = Position {
            x: detector.place[0][PLACE],
            y: detector.place[1][PLACE],
            z: encounter.position[2],
        };
        middle.horizontal_distance(position(encounter.position))
    });
    (volume_radius_m(encounter) - off_middle).max(0.0)
}

/// The height of the middle of a standing player body over its feet, m (half the standing
/// capsule of `slices/player.dynamicslice`, about 1.8 m high).
const BODY_MIDDLE_M: f32 = 0.9;

/// The lowest and the highest height of `encounter`'s detection volume when it is an upright
/// cylinder (its axis along z); `None` for another volume.
pub(super) fn volume_levels(encounter: &Encounter) -> Option<(f32, f32)> {
    const UP: usize = 2;
    const PLACE: usize = 3;
    let detector = encounter.detector.as_ref()?;
    let Geometry::Cylinder { from, to, .. } = &detector.shape else {
        return None;
    };
    let upright = from[0] == to[0] && from[1] == to[1] && detector.place[UP][UP] > 0.0;
    let base = detector.place[UP][PLACE];
    upright.then(|| (base + from[UP].min(to[UP]), base + from[UP].max(to[UP])))
}

/// How long after its end at `done_ms` an open world encounter starts again: a time in its
/// spawner's respawn range (picked by the end time, so that a run holds one time), else the
/// rules' mob respawn time. A range whose end is below its start is its start: the holdout's
/// spawner (`WorldEvent_Holdout`, range 10 to 0 s) took its one-way door away 10.05 s after the
/// end its stage times give (`99a_msq_01b_learnsoulwardens` 378.650 s; Inferred: the end
/// itself is out of the player's view).
fn respawn_ms(encounter: &Encounter, rules: OpenWorldRules, done_ms: u64) -> u64 {
    let Some([low_s, high_s]) = encounter
        .spawner
        .map(|spawner| spawner.respawn_s)
        .filter(|[low, high]| *high > 0.0 || *low > 0.0)
        .map(|[low, high]| [low, high.max(low)])
    else {
        return rules.respawn_ms;
    };
    // SplitMix64 of the end time: a share of the range.
    let mixed = mixed(done_ms);
    // A share of a range of minutes.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share of a range of minutes"
    )]
    let ms = {
        let share = (mixed >> SHARE_SHIFT) as f64 / (1_u64 << SHARE_BITS) as f64;
        let seconds = f64::from(low_s) + share * f64::from(high_s - low_s);
        (seconds * MS_PER_S_F64).round() as u64
    };
    ms
}

/// The SplitMix64 mix (Steele, Lea and Flood) of `seed`.
pub(super) const fn mixed(seed: u64) -> u64 {
    let mut mixed = seed.wrapping_add(SPLITMIX_GAMMA);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(SPLITMIX_MUL_A);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(SPLITMIX_MUL_B);
    mixed ^ (mixed >> 31)
}

fn tick_encounter(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    view: &ScriptView,
    now_ms: u64,
) {
    for (mob, event) in &view.events {
        if run.mobs.iter().any(|known| known.entity == *mob) {
            run.events.push((event.clone(), now_ms));
        }
    }
    bury(run, host, now_ms);
    // A slice spawn's own timer runs on after its stage and its encounter ended.
    timed_due(run, host, now_ms);
    hazard_areas_due(run, host, now_ms);
    gather_clocks_due(encounter, run, host, now_ms);
    deactivators_due(encounter, run, host, now_ms);
    for trigger in run
        .area_triggers
        .iter_mut()
        .filter(|trigger| trigger.fired_ms.is_none())
    {
        if view.living.iter().any(|player| {
            distance([player.x, player.y, player.z], trigger.at) <= AREA_TRIGGER_RADIUS_M
        }) {
            trigger.fired_ms = Some(now_ms);
        }
    }
    let radius = encounter
        .detection_radius_m
        .unwrap_or(DEFAULT_VOLUME_RADIUS_M);
    run.engaged = view
        .living
        .iter()
        .any(|player| detects(encounter, *player, radius));
    let reset_asked = !encounter.reset_event.is_empty()
        && run
            .events
            .iter()
            .any(|(event, _)| event.eq_ignore_ascii_case(&encounter.reset_event));
    if run.status == EncounterStatus::Idle {
        if encounter.auto_activate {
            run.status = EncounterStatus::Due(
                now_ms.saturating_add(u64::from(encounter.auto_activate_delay_ms)),
            );
        } else if run.engaged {
            run.status = EncounterStatus::Due(now_ms);
        }
    }
    if let EncounterStatus::Due(at_ms) = run.status
        && now_ms >= at_ms
    {
        run.status = EncounterStatus::Running;
        run.top = 0;
    }
    if run.status != EncounterStatus::Running {
        return;
    }
    let wiped = view.present && view.living.is_empty();
    run.wiped_since_ms = wiped.then(|| run.wiped_since_ms.unwrap_or(now_ms));
    let grace_ms = u64::from(encounter.grace_period_s).saturating_mul(MS_PER_S);
    let wipe_over = run
        .wiped_since_ms
        .is_some_and(|since| now_ms.saturating_sub(since) >= grace_ms);
    if wipe_over || reset_asked {
        reset(run, host, now_ms);
        return;
    }
    run_tops(encounter, run, host, now_ms);
}

/// Spawns the mobs of the slice spawns' delayed events that are due ([`TimedDue`]), and removes
/// every mob whose slice spawn ended ([`EncounterMob::ends_ms`]): it goes without a death of
/// its own and counts as destroyed.
fn timed_due(run: &mut EncounterRun, host: &mut impl ScriptHost, now_ms: u64) {
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut run.timed)
        .into_iter()
        .partition(|timed| timed.due_ms <= now_ms);
    run.timed = later;
    for timed in due {
        if timed.ends_ms.is_some_and(|ends| ends <= now_ms) {
            continue;
        }
        match host.spawn(&timed.mob, timed.at, None) {
            Some(entity) => run.mobs.push(EncounterMob {
                entity,
                target: timed.target,
                slice: timed.slice,
                stage: timed.stage,
                died_ms: None,
                gone: false,
                refill_at: None,
                ends_ms: timed.ends_ms,
                of_object: false,
                member: None,
            }),
            None => tracing::warn!(vitals = %timed.mob.vitals_id, "a timed spawn cannot spawn"),
        }
    }
    for mob in run.mobs.iter_mut().filter(|mob| !mob.gone) {
        if mob.ends_ms.is_some_and(|ends| ends <= now_ms) {
            host.remove(mob.entity);
            mob.gone = true;
            mob.died_ms.get_or_insert(now_ms);
        }
    }
}

/// Runs the effect areas of the standing hazard objects: while one is on, each living player
/// within it takes its effects once; when it turns off, the players it gave them to lose them
/// (`m_removeOnExit`; Inferred: turning off ends them as leaving the volume would) and it goes.
fn hazard_areas_due(run: &mut EncounterRun, host: &mut impl ScriptHost, now_ms: u64) {
    let mut kept = Vec::with_capacity(run.hazard_areas.len());
    for mut area in std::mem::take(&mut run.hazard_areas) {
        if now_ms >= area.off_ms {
            host.end_effects(&area.given, &area.effects, area.object, now_ms);
            continue;
        }
        if now_ms >= area.on_ms {
            let given = host.afflict(
                (area.at, area.radius_m),
                &area.effects,
                (area.object, now_ms),
                &area.given,
            );
            area.given.extend(given);
        }
        kept.push(area);
    }
    run.hazard_areas = kept;
}

/// Spawns hazard object `hazard` of slice `slice` of target `target` for stage `stage` at `at`,
/// turned `yaw_deg`, at server time `now_ms`: it stands for its life, comes back at its point
/// after its end like a dead area spawner mob ([`refill_due`]), and its effect areas run from
/// their on to their off times ([`hazard_areas_due`]).
fn spawn_hazard(
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    hazard: &HazardObject,
    (target, slice, stage): (usize, usize, usize),
    (at, yaw_deg, now_ms): ([f32; 3], f32, u64),
) {
    let Some(entity) = host.spawn_object(&hazard.slice, at, yaw_deg) else {
        tracing::debug!(slice = %hazard.slice, "a hazard object that is no start object");
        return;
    };
    run.mobs.push(EncounterMob {
        entity,
        target,
        slice,
        stage,
        died_ms: None,
        gone: false,
        refill_at: Some(at),
        ends_ms: Some(now_ms.saturating_add(seconds_to_ms(hazard.life_s))),
        of_object: false,
        member: None,
    });
    run.hazard_areas
        .extend(hazard.areas.iter().map(|area| HazardArea {
            object: entity,
            at,
            effects: area.effects.clone(),
            radius_m: area.radius_m,
            on_ms: now_ms.saturating_add(seconds_to_ms(area.on_s)),
            off_ms: now_ms.saturating_add(seconds_to_ms(area.off_s)),
            given: Vec::new(),
        }));
}

/// Runs the top stages in order; a failed one resets the encounter.
fn run_tops(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    now_ms: u64,
) {
    while let Some(&stage) = encounter.top_stages.get(run.top) {
        if run.stages[stage].status == StageStatus::Pending {
            start_stage(encounter, run, host, stage, now_ms);
        }
        match step_stage(encounter, run, host, stage, now_ms) {
            StageStatus::Done => run.top += 1,
            StageStatus::Failed => {
                reset(run, host, now_ms);
                return;
            }
            StageStatus::Pending | StageStatus::Running => return,
        }
    }
    run.status = EncounterStatus::Done;
    run.ended_ms = Some(now_ms);
}

/// Whether the areas of `run`'s encounter and of its spawns still stand at `now_ms`: not
/// [`AREAS_AFTER_SUCCESS_MS`] after its success.
fn areas_stand(run: &EncounterRun, now_ms: u64) -> bool {
    run.status != EncounterStatus::Done
        || run
            .ended_ms
            .is_none_or(|ended| now_ms.saturating_sub(ended) < AREAS_AFTER_SUCCESS_MS)
}

/// The effect areas of `encounter`'s own slice stand while it is `shown` and its areas stand
/// ([`areas_stand`]), and those of its stage spawns go with its success's (module text).
fn sync_own_areas(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    shown: bool,
    now_ms: u64,
) {
    let stand = areas_stand(run, now_ms);
    if !stand {
        for handle in std::mem::take(&mut run.spawn_areas) {
            host.close_slice_areas(handle);
        }
    }
    match run.own_areas {
        Some(handle) if !(shown && stand) => {
            host.close_slice_areas(handle);
            run.own_areas = None;
        }
        None if shown && stand && !run.no_own_areas => {
            run.own_areas =
                host.open_slice_areas(&encounter.slice, encounter.position, encounter.yaw_deg);
            run.no_own_areas = run.own_areas.is_none();
        }
        _ => {}
    }
}

/// The encounter starts again: its mobs go, its stages wait, it is idle.
fn reset(run: &mut EncounterRun, host: &mut impl ScriptHost, now_ms: u64) {
    for mob in run.mobs.iter_mut().filter(|mob| !mob.gone) {
        host.remove(mob.entity);
        mob.gone = true;
    }
    for area in &run.hazard_areas {
        host.end_effects(&area.given, &area.effects, area.object, now_ms);
    }
    for handle in std::mem::take(&mut run.spawn_areas) {
        host.close_slice_areas(handle);
    }
    let stages = run.stages.len();
    *run = EncounterRun {
        stages: vec![StageRun::default(); stages],
        own_areas: run.own_areas,
        no_own_areas: run.no_own_areas,
        ..EncounterRun::default()
    };
}

/// Notes the deaths of the encounter's mobs; a body goes [`CORPSE_MS`] after its death.
fn bury(run: &mut EncounterRun, host: &mut impl ScriptHost, now_ms: u64) {
    for mob in run.mobs.iter_mut().filter(|mob| !mob.gone) {
        match host.health(mob.entity) {
            None => {
                mob.gone = true;
                mob.died_ms.get_or_insert(now_ms);
            }
            Some(0) => {
                let died = *mob.died_ms.get_or_insert(now_ms);
                if now_ms.saturating_sub(died) >= CORPSE_MS {
                    host.remove(mob.entity);
                    mob.gone = true;
                }
            }
            Some(_) => {}
        }
    }
}

fn start_stage(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    stage: usize,
    now_ms: u64,
) {
    let Some(def) = encounter.stages.get(stage) else {
        return;
    };
    run.stages[stage] = StageRun {
        status: StageStatus::Running,
        started_ms: now_ms,
        due: def
            .spawns
            .iter()
            .enumerate()
            .map(|(index, spawn)| {
                (
                    now_ms.saturating_add(u64::from(spawn.delay_s).saturating_mul(MS_PER_S)),
                    index,
                )
            })
            .collect(),
        child: 0,
        done: vec![false; def.objectives.len()],
    };
    run_actions(encounter, run, host, &def.start_actions, stage, now_ms);
}

/// Whether every objective of `stage` waits for its parent.
fn waits_for_parent(encounter: &Encounter, stage: usize) -> bool {
    encounter.stages.get(stage).is_some_and(|def| {
        !def.objectives.is_empty()
            && def
                .objectives
                .iter()
                .all(|objective| objective.kind == ObjectiveSpec::WaitForParent)
    })
}

/// One step of running stage `stage`; its status after it.
fn step_stage(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    stage: usize,
    now_ms: u64,
) -> StageStatus {
    let Some(def) = encounter.stages.get(stage) else {
        return StageStatus::Done;
    };
    if run.stages[stage].status != StageStatus::Running {
        return run.stages[stage].status;
    }
    spawn_due(encounter, run, host, stage, now_ms);
    // Its children, in order; a child that waits for its parent runs beside the next ones.
    while let Some(&child) = def.children.get(run.stages[stage].child) {
        if run.stages[child].status == StageStatus::Pending {
            start_stage(encounter, run, host, child, now_ms);
        }
        if waits_for_parent(encounter, child) {
            spawn_due(encounter, run, host, child, now_ms);
            run.stages[stage].child += 1;
            continue;
        }
        match step_stage(encounter, run, host, child, now_ms) {
            StageStatus::Done => run.stages[stage].child += 1,
            StageStatus::Failed => {
                run.stages[stage].status = StageStatus::Failed;
                return StageStatus::Failed;
            }
            StageStatus::Pending | StageStatus::Running => break,
        }
    }
    let children_ended = run.stages[stage].child >= def.children.len();
    let mut own = false;
    for (index, objective) in def.objectives.iter().enumerate() {
        if objective.kind == ObjectiveSpec::WaitForParent {
            continue;
        }
        own = true;
        if run.stages[stage].done[index] {
            continue;
        }
        let started = run.stages[stage].started_ms;
        let limit_ms = u64::from(objective.time_limit_s).saturating_mul(MS_PER_S);
        let timed_out = objective.time_limit_s > 0 && now_ms.saturating_sub(started) >= limit_ms;
        let met = objective_met(encounter, run, &*host, stage, &objective.kind, started);
        if (met && objective.failure) || (timed_out && !objective.succeed_on_timeout && !met) {
            run.stages[stage].status = StageStatus::Failed;
            return StageStatus::Failed;
        }
        if met || timed_out {
            run.stages[stage].done[index] = true;
        }
    }
    let own_done = def
        .objectives
        .iter()
        .zip(&run.stages[stage].done)
        .all(|(objective, done)| {
            *done || objective.failure || objective.kind == ObjectiveSpec::WaitForParent
        });
    let ended = if own {
        own_done && (children_ended || def.child_stages_optional)
    } else {
        children_ended
    };
    if ended {
        end_stage(encounter, run, host, stage, now_ms);
        return StageStatus::Done;
    }
    StageStatus::Running
}

/// Ends stage `stage` and every stage under it that still runs, with their end actions.
fn end_stage(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    stage: usize,
    now_ms: u64,
) {
    let Some(def) = encounter.stages.get(stage) else {
        return;
    };
    for child in &def.children {
        if run.stages[*child].status == StageStatus::Running {
            end_stage(encounter, run, host, *child, now_ms);
        }
    }
    run.stages[stage].status = StageStatus::Done;
    run.stages[stage].due.clear();
    run.refills.retain(|refill| refill.stage != stage);
    for picked in run.picked.iter_mut().filter(|picked| picked.stage == stage) {
        picked.due_ms = None;
    }
    run_actions(encounter, run, host, &def.end_actions, stage, now_ms);
}

/// Makes the spawns of `stage` that are due, and the comebacks of its area spawners' mobs.
fn spawn_due(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    stage: usize,
    now_ms: u64,
) {
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut run.stages[stage].due)
        .into_iter()
        .partition(|(at_ms, _)| *at_ms <= now_ms);
    run.stages[stage].due = later;
    for (_, index) in due {
        if let Some(spawn) = encounter.stages[stage].spawns.get(index) {
            spawn_one(encounter, run, host, (stage, now_ms), spawn);
        }
    }
    refill_due(encounter, run, host, stage, now_ms);
    picked_due(encounter, run, host, stage, now_ms);
}

/// Spawns `spawn.count` mobs of its target: each picks a slice of the target by its weight
/// (an alias) among those of the encounter's family ([`in_family`]) and takes the next location
/// (the encounter's place without one). A target of no mob spawns its slices as start objects,
/// and the mobs their own delayed events make come later ([`TimedDue`]).
fn spawn_one(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    (stage, now_ms): (usize, u64),
    spawn: &Spawn,
) {
    let Some(target) = encounter.targets.get(spawn.target) else {
        return;
    };
    let location_of = |number: u32| {
        usize::try_from(number)
            .ok()
            .map(|number| number % spawn.locations.len().max(1))
    };
    let at_of = |number: u32| {
        location_of(number)
            .and_then(|location| spawn.locations.get(location))
            .copied()
            .unwrap_or(encounter.position)
    };
    // The encounter's yaw and its location's turn ([`Spawn::turns_deg`]).
    let yaw_of = |number: u32| {
        encounter.yaw_deg
            + location_of(number)
                .and_then(|location| spawn.turns_deg.get(location))
                .copied()
                .unwrap_or_default()
    };
    let difficulty = encounter.difficulty;
    let mobs: Vec<(usize, &TargetMob, u32)> = target
        .slices
        .iter()
        .enumerate()
        .filter(|(_, slice)| of_difficulty(target, slice, difficulty))
        .filter_map(|(index, slice)| {
            Some((index, mob_at(slice, difficulty)?, slice.percent.max(ONE)))
        })
        .collect();
    let mobs = in_family(run, host, target, mobs);
    let per_spawn = |slice: usize| target.slices[slice].mobs_per_spawn.max(ONE);
    if mobs.is_empty() {
        for number in 0..spawn.count {
            // An alias that names one slice more than once (`darkness_hazard_corruption_minor_l4`
            // twice in `alias_darkness_corruption_hazard_minor`) spawns it once.
            let mut placed: Vec<&str> = Vec::new();
            for (slice, target_slice) in target
                .slices
                .iter()
                .enumerate()
                .filter(|(_, slice)| of_difficulty(target, slice, difficulty))
            {
                if placed.contains(&target_slice.path.as_str()) {
                    continue;
                }
                placed.push(&target_slice.path);
                let (at, yaw_deg) = (at_of(number), yaw_of(number));
                if let Some(randomizer) = &target_slice.randomizer {
                    let place = (spawn.target, slice, stage);
                    pick_spawners(encounter, run, host, randomizer, place, (at, yaw_deg));
                    continue;
                }
                if let Some(hazard) = &target_slice.hazard {
                    // An area spawner's hazard objects stand at its points, turned with the
                    // spawn.
                    let points = target_slice
                        .refill
                        .as_ref()
                        .map(|refill| refill.points.as_slice())
                        .unwrap_or_default();
                    for index in 0..target_slice.mobs_per_spawn.max(ONE) {
                        let point = usize::try_from(index)
                            .ok()
                            .and_then(|index| points.get(index % points.len().max(1)))
                            .map_or(at, |point| turned_point(*point, at, yaw_deg));
                        spawn_hazard(
                            run,
                            host,
                            hazard,
                            (spawn.target, slice, stage),
                            (point, yaw_deg, now_ms),
                        );
                    }
                    continue;
                }
                if let Some(handle) = host.open_slice_areas(&target_slice.path, at, yaw_deg) {
                    run.spawn_areas.push(handle);
                }
                let ends_ms = target_slice
                    .lifetime_s
                    .map(|lifetime_s| now_ms.saturating_add(seconds_to_ms(lifetime_s)));
                run.timed
                    .extend(target_slice.timed_mobs.iter().map(|timed| TimedDue {
                        due_ms: now_ms.saturating_add(seconds_to_ms(timed.delay_s)),
                        mob: timed.mob.clone(),
                        at: turned_point(timed.offset, at, yaw_deg),
                        target: spawn.target,
                        slice,
                        stage,
                        ends_ms,
                    }));
                if let Some(radius_m) = target_slice.detectable_radius_m {
                    run.deactivators.push(Deactivator {
                        at,
                        radius_m,
                        ends_ms,
                    });
                }
                // An area trigger counts by the players in it, also where the level knows its
                // slice as an object (the open world's encounter objects hold
                // `ai_encounter_area_trigger`; live 2026-11-24 09:19-09:29Z and 2026-11-25
                // 14:00Z a player stood 1 m from the trigger of
                // `99C_WorldEvent_SpawnGroup_Brytmar` and its stage never ended).
                let area_trigger = is_area_trigger(&target_slice.path);
                if area_trigger {
                    run.area_triggers.push(AreaTrigger {
                        target: spawn.target,
                        at,
                        fired_ms: None,
                    });
                }
                match host.spawn_object(&target_slice.path, at, yaw_deg) {
                    Some(entity) => {
                        run.mobs.push(EncounterMob {
                            entity,
                            target: spawn.target,
                            slice,
                            stage,
                            died_ms: None,
                            gone: false,
                            refill_at: None,
                            ends_ms,
                            of_object: false,
                            member: None,
                        });
                        if !target_slice.gather_mobs.is_empty() {
                            run.gather_clocks.push(GatherClock {
                                object: entity,
                                target: spawn.target,
                                slice,
                                stage,
                                at,
                                yaw_deg,
                                played_ms: 0,
                                last_ms: now_ms,
                                next: 0,
                            });
                        }
                    }
                    None if area_trigger => {}
                    None => {
                        tracing::debug!(slice = %target_slice.path, "encounter spawn of no mob")
                    }
                }
            }
        }
        return;
    }
    for number in 0..spawn.count {
        let Some(&(slice, mob, _)) = weighted(host, &mobs, |(_, _, weight)| *weight) else {
            continue;
        };
        let at = at_of(number);
        let members = &target.slices[slice].members;
        if !members.is_empty() {
            // A spawner slice of mixed mobs makes each point spawner's mob at its own place.
            for (index, member) in members.iter().enumerate() {
                let place = turned_point(member.offset, at, yaw_of(number));
                spawn_mob(
                    run,
                    host,
                    &member.mob,
                    (spawn.target, slice, stage),
                    (place, None),
                    target,
                    Some(index),
                );
            }
            continue;
        }
        let refill = target.slices[slice].refill.as_ref();
        for index in 0..per_spawn(slice) {
            // An area spawner's mobs stand at its points, turned with the spawn.
            let point = refill.and_then(|refill| {
                let points = &refill.points;
                let index = usize::try_from(index).ok()? % points.len().max(1);
                points
                    .get(index)
                    .map(|point| turned_point(*point, at, yaw_of(number)))
            });
            let place = point.unwrap_or(at);
            let refill_at = refill.map(|_| place);
            let mob = alias_rolled(host, &target.slices[slice], mob, difficulty);
            spawn_mob(
                run,
                host,
                mob,
                (spawn.target, slice, stage),
                (place, refill_at),
                target,
                None,
            );
        }
    }
}

/// The mobs of `mobs` of the encounter's family ([`EncounterRun::family`]). A target whose mobs
/// are of more than one family keeps the encounter's family when it has mobs of it, else it
/// picks one by the summed weights of its mobs of each family and the encounter keeps it; the
/// first target of one family gives the encounter that family. A target with no mob of the
/// encounter's family spawns from all its mobs (a room's named mob or boss of another family).
/// Observed: each catacomb room showed mobs of one family (about 100 rooms of the 10 catacomb
/// ledgers; [`TargetSlice::family`]). Inferred: the pick by summed weights (the retail rule is
/// a server script no capture or client function shows) and one pick per run of the encounter.
fn in_family<'a>(
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    target: &'a Target,
    mobs: Vec<(usize, &'a TargetMob, u32)>,
) -> Vec<(usize, &'a TargetMob, u32)> {
    let mut families: Vec<(&str, u32)> = Vec::new();
    for (slice, _, weight) in &mobs {
        let family = target.slices[*slice].family.as_str();
        if family.is_empty() {
            continue;
        }
        match families.iter_mut().find(|(known, _)| *known == family) {
            Some((_, total)) => *total = total.saturating_add(*weight),
            None => families.push((family, *weight)),
        }
    }
    if families.is_empty() {
        return mobs;
    }
    let family = match run.family.as_deref() {
        Some(kept) if families.iter().any(|(known, _)| *known == kept) => kept.to_owned(),
        // A target only of another family than the encounter's: all its mobs.
        Some(_) if families.len() == 1 => return mobs,
        kept => {
            let Some((picked, _)) = weighted(host, &families, |(_, weight)| *weight) else {
                return mobs;
            };
            let picked = (*picked).to_owned();
            if kept.is_none() {
                run.family = Some(picked.clone());
            }
            picked
        }
    };
    mobs.into_iter()
        .filter(|(slice, _, _)| target.slices[*slice].family == family)
        .collect()
}

/// Spawns mob `mob` of slice `slice` of target `target_index` for stage `stage` at `at`; an
/// area spawner's mob keeps `refill_at`, the point it comes back at; a member of a spawner
/// slice of mixed mobs (`member`, [`TargetSlice::members`]) walks its own waypoints. Its entity,
/// or `None` when it cannot spawn.
fn spawn_mob(
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    mob: &TargetMob,
    (target_index, slice, stage): (usize, usize, usize),
    (at, refill_at): ([f32; 3], Option<[f32; 3]>),
    target: &Target,
    member: Option<usize>,
) -> Option<u64> {
    let own = &target.slices[slice];
    let waypoints = match member {
        Some(index) => own
            .members
            .get(index)
            .and_then(|member| member.waypoints.as_ref()),
        None => own.waypoints.as_ref(),
    };
    let entity = host.spawn(mob, at, waypoints);
    match entity {
        Some(entity) => run.mobs.push(EncounterMob {
            entity,
            target: target_index,
            slice,
            stage,
            died_ms: None,
            gone: false,
            refill_at,
            ends_ms: None,
            of_object: false,
            member,
        }),
        None => tracing::warn!(
            vitals = %mob.vitals_id,
            level = mob.level,
            "an encounter mob cannot spawn"
        ),
    }
    entity
}

/// Picks the spawners of randomizer `randomizer` (slice `slice` of target `target`, spawned by
/// stage `stage` at `at`, turned `yaw_deg`): a count in its range of different choices, each
/// count and choice equally likely (Inferred), and spawns each picked one's mob at its place
/// ([`PickedSpawner`]).
fn pick_spawners(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    randomizer: &SpawnRandomizer,
    (target, slice, stage): (usize, usize, usize),
    (at, yaw_deg): ([f32; 3], f32),
) {
    let [fewest, most] = randomizer.picks;
    let count = fewest.saturating_add(host.pick(most.saturating_sub(fewest).saturating_add(ONE)));
    let mut left: Vec<usize> = (0..randomizer.choices.len()).collect();
    for _ in 0..count {
        let Some(last) = left.len().checked_sub(1) else {
            break;
        };
        let below = u32::try_from(left.len()).unwrap_or(u32::MAX);
        let index = usize::try_from(host.pick(below)).map_or(last, |index| index.min(last));
        let choice = left.remove(index);
        let mut picked = PickedSpawner {
            target,
            slice,
            stage,
            choice,
            at: turned_point(randomizer.choices[choice].offset, at, yaw_deg),
            mob: None,
            due_ms: None,
            off: false,
            leaves_ms: None,
        };
        picked.mob = spawn_picked(encounter, run, host, &picked);
        run.picked.push(picked);
    }
}

/// The choice of randomizer that picked spawner `picked` is.
fn choice_of<'a>(encounter: &'a Encounter, picked: &PickedSpawner) -> Option<&'a RandomizerChoice> {
    encounter
        .targets
        .get(picked.target)?
        .slices
        .get(picked.slice)?
        .randomizer
        .as_ref()?
        .choices
        .get(picked.choice)
}

/// Spawns the mob of picked spawner `picked` at its place; its entity.
fn spawn_picked(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    picked: &PickedSpawner,
) -> Option<u64> {
    let target = encounter.targets.get(picked.target)?;
    let mob = &choice_of(encounter, picked)?.mob;
    let place = (picked.target, picked.slice, picked.stage);
    spawn_mob(run, host, mob, place, (picked.at, None), target, None)
}

/// The comebacks of the spawners stage `stage` picked ([`PickedSpawner`]): the dead mob of a
/// spawner that is on comes back its choice's respawn time after its death, as the same mob
/// (Observed: the Reekwater closet's seven crawlers, [`SpawnRandomizer`]); a choice without a
/// respawn time brings none back.
fn picked_due(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    stage: usize,
    now_ms: u64,
) {
    for index in 0..run.picked.len() {
        let picked = &run.picked[index];
        if picked.stage != stage || picked.off {
            continue;
        }
        if let Some(entity) = picked.mob {
            let Some(died_ms) = run
                .mobs
                .iter()
                .find(|mob| mob.entity == entity)
                .and_then(|mob| mob.died_ms)
            else {
                continue;
            };
            let respawn_s = choice_of(encounter, picked).and_then(|choice| choice.respawn_s);
            run.picked[index].mob = None;
            run.picked[index].due_ms =
                respawn_s.map(|respawn_s| died_ms.saturating_add(seconds_to_ms(respawn_s)));
        }
        if run.picked[index]
            .due_ms
            .is_some_and(|due_ms| due_ms <= now_ms)
        {
            run.picked[index].due_ms = None;
            let picked = run.picked[index].clone();
            run.picked[index].mob = spawn_picked(encounter, run, host, &picked);
        }
    }
}

/// Turns off each picked spawner that a detectable object of the encounter reaches (the
/// spawner's detection volume and the object's sphere meet): it brings no mob back, and its mob
/// goes at the object's end ([`RandomizerChoice::stop_radius_m`]); the objects that ended are
/// dropped.
fn deactivators_due(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    now_ms: u64,
) {
    for deactivator in &run.deactivators {
        for picked in run.picked.iter_mut().filter(|picked| !picked.off) {
            let Some(volume_m) =
                choice_of(encounter, picked).and_then(|choice| choice.stop_radius_m)
            else {
                continue;
            };
            if distance(picked.at, deactivator.at) <= volume_m + deactivator.radius_m {
                picked.off = true;
                picked.due_ms = None;
                picked.leaves_ms = deactivator.ends_ms;
            }
        }
    }
    run.deactivators
        .retain(|deactivator| deactivator.ends_ms.is_none_or(|ends_ms| ends_ms > now_ms));
    for picked in &mut run.picked {
        if picked.leaves_ms.is_none_or(|leaves_ms| leaves_ms > now_ms) {
            continue;
        }
        picked.leaves_ms = None;
        let Some(entity) = picked.mob.take() else {
            continue;
        };
        if let Some(mob) = run
            .mobs
            .iter_mut()
            .find(|mob| mob.entity == entity && !mob.gone)
        {
            host.remove(entity);
            mob.gone = true;
            mob.died_ms.get_or_insert(now_ms);
        }
    }
}

/// Plays the gathering timelines of the standing start objects ([`GatherClock`]): while a player
/// gathers its object, a timeline runs on and spawns each of its mobs whose time on it came, at
/// the mob's spawner turned with the object; between gathers it holds its place; it ends with
/// its object.
fn gather_clocks_due(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    now_ms: u64,
) {
    let mut kept = Vec::with_capacity(run.gather_clocks.len());
    for mut clock in std::mem::take(&mut run.gather_clocks) {
        if host.health(clock.object).is_none() {
            continue;
        }
        if host.being_gathered(clock.object) {
            clock.played_ms = clock
                .played_ms
                .saturating_add(now_ms.saturating_sub(clock.last_ms));
        }
        clock.last_ms = now_ms;
        let mobs = encounter
            .targets
            .get(clock.target)
            .and_then(|target| target.slices.get(clock.slice))
            .map(|slice| slice.gather_mobs.as_slice())
            .unwrap_or_default();
        while let Some(timed) = mobs
            .get(clock.next)
            .filter(|timed| seconds_to_ms(timed.delay_s) <= clock.played_ms)
        {
            clock.next += 1;
            let at = turned_point(timed.offset, clock.at, clock.yaw_deg);
            match host.spawn(&timed.mob, at, None) {
                Some(entity) => run.mobs.push(EncounterMob {
                    entity,
                    target: clock.target,
                    slice: clock.slice,
                    stage: clock.stage,
                    died_ms: None,
                    gone: false,
                    refill_at: None,
                    ends_ms: None,
                    of_object: true,
                    member: None,
                }),
                None => {
                    tracing::warn!(vitals = %timed.mob.vitals_id, "a gather spawn cannot spawn")
                }
            }
        }
        kept.push(clock);
    }
    run.gather_clocks = kept;
}

/// The comebacks of stage `stage`: each dead area spawner mob of the stage gets one, a time in
/// its spawner's respawn range after its death ([`nw_world::dungeon_scripts::AreaRefill`]);
/// those that are due come back at their points.
fn refill_due(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    stage: usize,
    now_ms: u64,
) {
    for index in 0..run.mobs.len() {
        let mob = &run.mobs[index];
        let (Some(died_ms), Some(at)) = (mob.died_ms, mob.refill_at) else {
            continue;
        };
        if mob.stage != stage {
            continue;
        }
        let (target, slice) = (mob.target, mob.slice);
        let Some(refill) = encounter
            .targets
            .get(target)
            .and_then(|target| target.slices.get(slice))
            .and_then(|slice| slice.refill.as_ref())
        else {
            continue;
        };
        let [shortest, longest] = refill.respawn_s.map(seconds_to_ms);
        let spread = u32::try_from(longest.saturating_sub(shortest)).unwrap_or(u32::MAX);
        let wait_ms = shortest + u64::from(host.pick(spread.saturating_add(ONE)));
        run.mobs[index].refill_at = None;
        run.refills.push(Refill {
            due_ms: died_ms.saturating_add(wait_ms),
            target,
            slice,
            stage,
            at,
        });
    }
    let (due, later): (Vec<Refill>, Vec<Refill>) = std::mem::take(&mut run.refills)
        .into_iter()
        .partition(|refill| refill.stage == stage && refill.due_ms <= now_ms);
    run.refills = later;
    let difficulty = encounter.difficulty;
    for refill in due {
        let Some(target) = encounter.targets.get(refill.target) else {
            continue;
        };
        if let Some(hazard) = target
            .slices
            .get(refill.slice)
            .and_then(|slice| slice.hazard.as_ref())
        {
            let place = (refill.target, refill.slice, refill.stage);
            spawn_hazard(
                run,
                host,
                hazard,
                place,
                (refill.at, encounter.yaw_deg, now_ms),
            );
            continue;
        }
        let Some((slice, mob)) = target
            .slices
            .get(refill.slice)
            .and_then(|slice| Some((slice, mob_at(slice, difficulty)?)))
        else {
            continue;
        };
        let mob = alias_rolled(host, slice, mob, difficulty);
        let place = (refill.target, refill.slice, refill.stage);
        spawn_mob(
            run,
            host,
            mob,
            place,
            (refill.at, Some(refill.at)),
            target,
            None,
        );
    }
}

/// Whole milliseconds of `seconds` (0 for a negative time).
fn seconds_to_ms(seconds: f32) -> u64 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a respawn time of seconds"
    )]
    let ms = (f64::from(seconds.max(0.0)) * MS_PER_S_F64).round() as u64;
    ms
}

/// Whether every objective of `encounter` can be met at its difficulty: each target a stage
/// objective tracks, and each spawn under a stage that counts its own spawns, has a slice of the
/// difficulty that spawns a mob or a start object (`is_object`).
fn completable(encounter: &Encounter, is_object: &impl Fn(&str) -> bool) -> bool {
    let difficulty = encounter.difficulty;
    let spawnable = |target: usize| {
        encounter.targets.get(target).is_some_and(|target| {
            target
                .slices
                .iter()
                .filter(|slice| of_difficulty(target, slice, difficulty))
                .any(|slice| {
                    mob_at(slice, difficulty).is_some()
                        || slice.randomizer.is_some()
                        || is_object(&slice.path)
                })
        })
    };
    encounter.stages.iter().enumerate().all(|(index, stage)| {
        stage
            .objectives
            .iter()
            .all(|objective| match objective.kind {
                ObjectiveSpec::SliceDestroyed {
                    target: Some(target),
                    ..
                } => spawnable(target),
                ObjectiveSpec::SliceDestroyed { target: None, .. } => encounter
                    .stages
                    .iter()
                    .enumerate()
                    .filter(|(under, _)| is_under(encounter, *under, index))
                    .flat_map(|(_, stage)| &stage.spawns)
                    .all(|spawn| spawnable(spawn.target)),
                _ => true,
            })
    })
}

/// Whether `slice` of `target` spawns at `difficulty`: the target holds no slice of one
/// difficulty, or the encounter has none, or the slice is of it; for a target of no mob (a
/// hazard), the slice of the highest difficulty of the target's slices not above it (module
/// text).
fn of_difficulty(target: &Target, slice: &TargetSlice, difficulty: Option<u32>) -> bool {
    let by_difficulty = target.slices.iter().any(|slice| slice.difficulty.is_some());
    let of_mobs = target.slices.iter().any(|slice| slice.mob.is_some());
    match difficulty {
        Some(difficulty) if by_difficulty && of_mobs => slice.difficulty == Some(difficulty),
        Some(difficulty) if by_difficulty => {
            let nearest = target
                .slices
                .iter()
                .filter_map(|slice| slice.difficulty)
                .filter(|of| *of <= difficulty)
                .max();
            nearest.is_some() && slice.difficulty == nearest
        }
        _ => true,
    }
}

/// One of `items`, rolled by the weight `weight_of` gives each (the alias `Percent`); `None`
/// when `items` is empty.
fn weighted<'a, T>(
    host: &mut impl ScriptHost,
    items: &'a [T],
    weight_of: impl Fn(&T) -> u32,
) -> Option<&'a T> {
    let total: u32 = items.iter().map(&weight_of).sum();
    let mut roll = host.pick(total);
    items.iter().find(|item| {
        let weight = weight_of(item);
        let hit = roll < weight;
        roll = roll.saturating_sub(weight);
        hit
    })
}

/// The mob one spawned mob of `slice` is: one of its spawner alias's mobs rolled by weight
/// ([`nw_world::dungeon_scripts::TargetSlice::alias_mobs`]), else `mob` (also for a slice with
/// a mob of its own at `difficulty`).
fn alias_rolled<'a>(
    host: &mut impl ScriptHost,
    slice: &'a TargetSlice,
    mob: &'a TargetMob,
    difficulty: Option<u32>,
) -> &'a TargetMob {
    let own_difficulty =
        difficulty.is_some_and(|difficulty| slice.difficulty_mobs.contains_key(&difficulty));
    if own_difficulty {
        return mob;
    }
    weighted(host, &slice.alias_mobs, |entry| entry.percent.max(ONE))
        .map_or(mob, |entry| &entry.mob)
}

/// The mob `slice` spawns at `difficulty`: its nested slices' mob of that difficulty, else its
/// own.
fn mob_at(slice: &TargetSlice, difficulty: Option<u32>) -> Option<&TargetMob> {
    difficulty
        .and_then(|difficulty| slice.difficulty_mobs.get(&difficulty))
        .or(slice.mob.as_ref())
}

/// Whether encounter mob `mob` is one of target `target`: its slice, a slice its spawner slice
/// spawns ([`TargetSlice::spawned_slices`]) or its vitals row is one of the target's.
fn is_of(encounter: &Encounter, mob: &EncounterMob, target: usize) -> bool {
    if mob.of_object {
        return false;
    }
    let Some(own) = encounter
        .targets
        .get(mob.target)
        .and_then(|own| own.slices.get(mob.slice))
    else {
        return false;
    };
    if let Some(member) = mob.member.and_then(|index| own.members.get(index)) {
        // A member of a spawner slice of mixed mobs is its spawner slice's, its own creature
        // slice's and its own row's mob: the cave summoner ends the wave that tracks it.
        return encounter.targets.get(target).is_some_and(|wanted| {
            wanted.slices.iter().any(|slice| {
                slice.path == own.path
                    || member.slices.contains(&slice.path)
                    || slice.mob.as_ref().is_some_and(|wanted| {
                        wanted.vitals_id.eq_ignore_ascii_case(&member.mob.vitals_id)
                    })
            })
        });
    }
    encounter.targets.get(target).is_some_and(|wanted| {
        wanted.slices.iter().any(|slice| {
            slice.path == own.path
                || own.spawned_slices.contains(&slice.path)
                || slice
                    .mob
                    .as_ref()
                    .zip(own.mob.as_ref())
                    .is_some_and(|(a, b)| a.vitals_id.eq_ignore_ascii_case(&b.vitals_id))
        })
    })
}

/// Whether stage `stage` is `ancestor` or under it.
fn is_under(encounter: &Encounter, stage: usize, ancestor: usize) -> bool {
    stage == ancestor
        || encounter.stages.get(ancestor).is_some_and(|def| {
            def.children
                .iter()
                .any(|child| is_under(encounter, stage, *child))
        })
}

/// The mobs an objective of stage `stage` tracks: those of `target` (Inferred: when no mob
/// of the encounter is of it, the target is a creature the stage's spawner slices pick by
/// their own alias, so the stage's own mobs), or the stage's own mobs without a target.
fn tracked<'a>(
    encounter: &'a Encounter,
    run: &'a EncounterRun,
    stage: usize,
    target: Option<usize>,
) -> Vec<&'a EncounterMob> {
    let own: Vec<&EncounterMob> = run
        .mobs
        .iter()
        .filter(|mob| is_under(encounter, mob.stage, stage))
        .collect();
    match target {
        Some(target) => {
            let of_target: Vec<&EncounterMob> = run
                .mobs
                .iter()
                .filter(|mob| is_of(encounter, mob, target))
                .collect();
            if of_target.is_empty() { own } else { of_target }
        }
        None => own,
    }
}

/// Whether encounter mob `mob` is a spawn of `target` (by its slice's path), or of stage
/// `stage` when the objective names no target, whose own spawner spawns a slice
/// ([`TargetSlice::spawned_slices`]).
fn spawns_for(
    encounter: &Encounter,
    mob: &EncounterMob,
    stage: usize,
    target: Option<usize>,
) -> bool {
    let Some(own) = encounter
        .targets
        .get(mob.target)
        .and_then(|own| own.slices.get(mob.slice))
    else {
        return false;
    };
    if mob.of_object {
        return false;
    }
    let same_slice = |target: usize| {
        encounter
            .targets
            .get(target)
            .is_some_and(|wanted| wanted.slices.iter().any(|slice| slice.path == own.path))
    };
    // A creature slice that the target names and the stage spawns itself, with no spawner in
    // it, is its own spawn. Observed: `QuestAdiana`'s `00 - Setup` (SliceSpawned of target 0,
    // `ai_fl_adiana_archer`) ended at 264.542 s, 12 ms after Adiana's card (264.530 s,
    // `09a_m15_fightartemis`).
    if own.spawned_slices.is_empty() {
        return own.mob.is_some() && target.is_some_and(same_slice);
    }
    match target {
        Some(target) => same_slice(target),
        None => is_under(encounter, mob.stage, stage),
    }
}

/// The count a counting objective of stage `stage` reached since `started_ms`: the deaths of
/// its tracked mobs (SliceDestroyed), or the slices the spawners of its tracked spawns made
/// (SliceSpawned); `None` for an objective that does not count.
///
/// A spawn whose slice has no mob is a start object: its spawner spawns its slice when a
/// player's gather of it ends (Observed: the stronghold siren's cannonball gatherables, see
/// [`ObjectiveSpec::SliceSpawned`]). A spawn of a spawner slice with a mob is the mob its
/// spawner made: it counts when a stage under the objective's stage spawned it (Inferred: the
/// setup stages that track the boss slice they spawn, `Wave_00 - Setup` of overseerzane, count
/// their own spawn; no capture shows the moment).
fn objective_count(
    encounter: &Encounter,
    run: &EncounterRun,
    stage: usize,
    kind: &ObjectiveSpec,
    started_ms: u64,
) -> Option<u32> {
    let count = match kind {
        ObjectiveSpec::SliceDestroyed { target, .. } => {
            let triggers = run
                .area_triggers
                .iter()
                .filter(|trigger| {
                    Some(trigger.target) == *target
                        && trigger.fired_ms.is_some_and(|fired| fired >= started_ms)
                })
                .count();
            tracked(encounter, run, stage, *target)
                .into_iter()
                .filter(|mob| mob.died_ms.is_some_and(|died| died >= started_ms))
                .count()
                + triggers
        }
        ObjectiveSpec::SliceSpawned { target, .. } => run
            .mobs
            .iter()
            .filter(|mob| spawns_for(encounter, mob, stage, *target))
            .filter(|mob| {
                let object = encounter
                    .targets
                    .get(mob.target)
                    .and_then(|own| own.slices.get(mob.slice))
                    .is_some_and(|own| mob_at(own, encounter.difficulty).is_none());
                if object {
                    mob.died_ms.is_some_and(|died| died >= started_ms)
                } else {
                    is_under(encounter, mob.stage, stage)
                }
            })
            .count(),
        _ => return None,
    };
    Some(u32::try_from(count).unwrap_or(u32::MAX))
}

fn objective_met(
    encounter: &Encounter,
    run: &EncounterRun,
    host: &impl ScriptHost,
    stage: usize,
    kind: &ObjectiveSpec,
    started_ms: u64,
) -> bool {
    match kind {
        ObjectiveSpec::SliceDestroyed { required, .. }
        | ObjectiveSpec::SliceSpawned { required, .. } => {
            objective_count(encounter, run, stage, kind, started_ms)
                .is_some_and(|count| count >= (*required).max(ONE))
        }
        ObjectiveSpec::BossPhase { target, phase } => {
            let candidates: Vec<&EncounterMob> = match target {
                Some(target) => tracked(encounter, run, stage, Some(*target)),
                None => run.mobs.iter().collect(),
            };
            // Inferred: the boss's death also ends its phases.
            candidates.into_iter().any(|mob| {
                mob.died_ms.is_some()
                    || host
                        .boss_phase(mob.entity)
                        .is_some_and(|reached| reached >= *phase)
            })
        }
        ObjectiveSpec::EncounterEvent { event } => run
            .events
            .iter()
            .any(|(sent, at)| *at >= started_ms && sent.eq_ignore_ascii_case(event)),
        ObjectiveSpec::EntityActiveState {
            entity,
            on_activated,
            on_deactivated,
        } => {
            let sent = |wanted: u32| {
                run.entity_events
                    .iter()
                    .any(|(to, event, at)| to == entity && *event == wanted && *at >= started_ms)
            };
            if *on_activated || *on_deactivated {
                (*on_activated && sent(EVENT_ACTIVATE))
                    || (*on_deactivated && sent(EVENT_DEACTIVATE))
                    || (run.open_world
                        && run.engaged
                        && !changed_by_a_stage(encounter, *entity, *on_activated, *on_deactivated))
            } else {
                // Inferred: the entity is a timeline that the encounter's trigger area starts
                // when a player comes in; a player's arrival ends the stage only once its own
                // spawns are made (retail `quest_18018-The-Wolf-s-Den`: `_Wave 0 - Wait` ran
                // 19.4 s and spawned Asena 5 s in; live 2026-11-25 15:52Z the stage ended at
                // once and dropped her spawn, so the trial had no boss).
                sent(EVENT_TRIGGER) || (run.engaged && run.stages[stage].due.is_empty())
            }
        }
        ObjectiveSpec::WaitForParent => false,
        ObjectiveSpec::InvasionWave => {
            let spawned_all = run.stages[stage].due.is_empty()
                && !run.refills.iter().any(|refill| refill.stage == stage)
                && !run
                    .picked
                    .iter()
                    .any(|picked| picked.stage == stage && picked.due_ms.is_some());
            let own: Vec<&EncounterMob> =
                run.mobs.iter().filter(|mob| mob.stage == stage).collect();
            spawned_all && !own.is_empty() && own.iter().all(|mob| mob.died_ms.is_some())
        }
    }
}

/// Whether an action of a stage of `encounter` activates (`activated`) or deactivates
/// (`deactivated`) its slice entity `entity`.
///
/// In build 25810808, 203 of the 207 open world stage objectives that wait for such a change
/// wait for an entity that no stage action changes: a trigger volume of the encounter's slice
/// (`Wave_00 - Setup - Wait for Enter Area`, `Trigger the Statue`) or an object the player uses
/// (the horn of `Enc_Defense_09A_M5_WaveEvent`: its `Start_Trigger_Event` entity goes off when
/// the horn is blown; a console). The content reads neither those volumes nor those objects.
/// **Inferred**: in the open world such an objective is met while a living player is in the
/// encounter's volume ([`objective_met`]). Not built: the trigger's own volume and the use of
/// the horn. Live 2026-11-25 14:24-14:28Z: `WorldEvent_09A_M5_WaveEvent` stood in
/// `Wave_00 - Wait for Players` with a player in it until that player left.
fn changed_by_a_stage(
    encounter: &Encounter,
    entity: u64,
    activated: bool,
    deactivated: bool,
) -> bool {
    encounter
        .stages
        .iter()
        .flat_map(|stage| stage.start_actions.iter().chain(&stage.end_actions))
        .any(|action| {
            matches!(action, StageAction::EntityEvent { entity: to, event, .. }
                if *to == entity
                    && ((activated && *event == EVENT_ACTIVATE)
                        || (deactivated && *event == EVENT_DEACTIVATE)))
        })
}

/// Runs `actions` of stage `stage`.
fn run_actions(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    actions: &[StageAction],
    stage: usize,
    now_ms: u64,
) {
    for action in actions {
        match action {
            StageAction::EntityEvent { entity, event, .. } => {
                // The event counts for the stage objectives that wait for it
                // (`EntityActiveStateChanged`), and the sim sends its effect on the entity of the
                // encounter slice it names on the encounter's root group
                // ([`super::encounter_roots::EncounterRoots::entity_event`]; dungeon gates note,
                // Observed). The gates and doors are no targets of these events: they are entities
                // of the level script ([`super::gates`]).
                run.entity_events.push((*entity, *event, now_ms));
            }
            StageAction::DestroySlice { target } => {
                for index in 0..run.mobs.len() {
                    let matches =
                        target.is_none_or(|target| is_of(encounter, &run.mobs[index], target));
                    if matches && !run.mobs[index].gone {
                        host.remove(run.mobs[index].entity);
                        run.mobs[index].gone = true;
                    }
                }
            }
            StageAction::KillSpawns => {
                for index in 0..run.mobs.len() {
                    let mob = &run.mobs[index];
                    if !mob.gone && mob.ends_ms.is_none() && is_under(encounter, mob.stage, stage) {
                        host.remove(mob.entity);
                        run.mobs[index].gone = true;
                    }
                }
            }
        }
    }
}

/// The sim as the host of its instance's script.
struct SimHost<'a> {
    world: &'a mut SimWorld,
    /// The mobs take the instance's mutation (an instance's script).
    mutated: bool,
}

impl ScriptHost for SimHost<'_> {
    fn spawn(
        &mut self,
        mob: &TargetMob,
        at: [f32; 3],
        waypoints: Option<&PatrolPath>,
    ) -> Option<u64> {
        let [x, y, z] = at;
        let ai =
            waypoints.map(|path| script_mob_ai(path, Position { x, y, z }, self.world.mobs.ai()));
        let category = Some(mob.category_id.as_str()).filter(|category| !category.is_empty());
        match self.world.spawn_gm_of_category(
            &mob.vitals_id,
            category,
            mob.level,
            Point { x, y, z },
            ONE,
            ai.as_ref(),
        ) {
            Ok(entities) => {
                let entity = entities.first().copied()?;
                if self.mutated {
                    self.world.mutate_mob(entity);
                }
                Some(entity)
            }
            Err(reason) => {
                tracing::warn!(vitals = %mob.vitals_id, %reason, "encounter spawn");
                None
            }
        }
    }

    fn engage(&mut self, mob: u64, in_volume: &dyn Fn(Position) -> bool) {
        let Some((at, _)) = self.world.mobs.places(mob) else {
            return;
        };
        let mut query = self.world.world.query::<(&Player, &Body, &Vitals)>();
        let nearest = query
            .iter(&self.world.world)
            .filter(|(_, body, vitals)| {
                !vitals.health.is_dead() && in_volume(body.motion.position())
            })
            .map(|(player, body, _)| (player.number, body.motion.position().distance(at)))
            .min_by(|(_, a), (_, b)| a.total_cmp(b));
        if let Some((number, _)) = nearest {
            let now_ms = self.world.last_now_ms;
            self.world.mobs.provoke(mob, number, now_ms);
        }
    }

    fn spawn_object(&mut self, slice: &str, at: [f32; 3], yaw_deg: f32) -> Option<u64> {
        // An encounter turret (the Isle of Night ballista) stands on the level turret holder.
        if let Some(number) = self.world.spawn_encounter_turret(slice, at, yaw_deg) {
            return Some(number);
        }
        self.world.encounter_objects.spawn(slice, at, yaw_deg)
    }

    fn remove(&mut self, entity: u64) {
        if self.world.remove_encounter_turret(entity) {
            return;
        }
        if EncounterObjects::is_object(entity) {
            self.world.encounter_objects.remove(entity);
        } else {
            self.world.remove_gm(&[entity]);
        }
    }

    fn afflict(
        &mut self,
        (at, radius_m): ([f32; 3], f32),
        effects: &[String],
        (source, now_ms): (u64, u64),
        skip: &[u64],
    ) -> Vec<u64> {
        let content = std::sync::Arc::clone(&self.world.content);
        let [x, y, z] = at;
        let mut given = Vec::new();
        for entity in self.world.players_within(Position { x, y, z }, radius_m) {
            let number = entity.to_bits();
            if skip.contains(&number) {
                continue;
            }
            let Some(mut player) = self.world.world.get_mut::<Player>(entity) else {
                continue;
            };
            for effect in effects {
                player.status_effects.add_by_id(
                    &content.status_effects,
                    effect,
                    HAZARD_EFFECT_POTENCY,
                    source,
                    now_ms,
                );
            }
            given.push(number);
        }
        given
    }

    fn end_effects(&mut self, players: &[u64], effects: &[String], source: u64, now_ms: u64) {
        let content = std::sync::Arc::clone(&self.world.content);
        for number in players {
            let Some(mut player) = self
                .world
                .world
                .get_mut::<Player>(Entity::from_bits(*number))
            else {
                continue;
            };
            for effect in effects {
                player.status_effects.end(
                    &content.status_effects,
                    nw_formats::objectstream::lowercase_crc32(effect),
                    source,
                    now_ms,
                );
            }
        }
    }

    fn health(&self, entity: u64) -> Option<u32> {
        if EncounterObjects::is_object(entity) {
            return self
                .world
                .encounter_objects
                .stands(entity)
                .then_some(OBJECT_HEALTH);
        }
        self.world.mobs.health(entity)
    }

    fn boss_phase(&self, entity: u64) -> Option<u32> {
        if let Some(phase) = self.world.boss_phases.get(&entity) {
            return Some(*phase);
        }
        let key = self.world.mobs.ai().data().key(BOSS_PHASE_KEY)?;
        let value = self
            .world
            .mobs
            .tree(entity)?
            .brain()
            .fact(key)?
            .as_number()?;
        // A phase is a small whole number.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a phase"
        )]
        Some(value.max(0.0) as u32)
    }

    fn being_gathered(&self, entity: u64) -> bool {
        self.world
            .open_gathers
            .iter()
            .any(|gather| gather.object() == Some(entity))
    }

    fn pick(&mut self, below: u32) -> u32 {
        self.world.dice.below(below.max(ONE))
    }

    fn open_slice_areas(&mut self, slice: &str, at: [f32; 3], yaw_deg: f32) -> Option<u64> {
        self.world.open_slice_areas(slice, at, yaw_deg)
    }

    fn close_slice_areas(&mut self, handle: u64) {
        self.world.close_slice_areas(handle);
    }
}

impl SimWorld {
    /// The encounters part of the tick at `now_ms`: the script of the running instance starts
    /// with it and ends with it; its encounters run; an ended one opens its loot containers,
    /// and the last one completes the instance.
    pub(super) fn tick_dungeon_script(&mut self, now_ms: u64) {
        let mut events = self.mobs.take_encounter_events();
        // The scripted bosses' events ([`super::boss_scripts`]).
        events.extend(self.boss_fights.take_events());
        // The intro ends of the expedition bosses ([`super::linked_bosses`]).
        events.extend(self.linked_bosses.take_events());
        if !self.instance_sim {
            self.tick_world_events(events, now_ms);
            return;
        }
        let running = self
            .instance_run
            .as_ref()
            .map(|run| (run.id(), run.game_mode().to_owned()));
        let daily = running
            .as_ref()
            .is_some_and(|(_, game_mode)| self.daily_trial_player(game_mode));
        let current = self
            .dungeon_run
            .as_ref()
            .map(|run| (run.instance, run.daily));
        if current != running.as_ref().map(|(instance, _)| (*instance, daily)) {
            if let Some(old) = self.dungeon_run.take() {
                let mobs: Vec<u64> = old.mobs().collect();
                self.remove_gm(&mobs);
            }
            let salt = running
                .as_ref()
                .map_or(0, |(instance, _)| instance.as_u64_pair().0);
            self.start_gates(running.as_ref().map(|(_, mode)| mode.as_str()), salt);
            self.fresh_instance_nodes(running.as_ref().map(|(instance, _)| *instance));
            // The script's start objects (the talisman piles of `QuestLiang`) are slices of the
            // start objects table; a catacombs run builds its own ([`super::catacombs`]), and an
            // instance sim runs no open world encounters that hold one.
            if running.is_some() && self.catacomb_run().is_none() && self.world_events.is_none() {
                self.encounter_objects = EncounterObjects::build(&self.content.encounter_objects);
            }
            self.dungeon_run = running.and_then(|(instance, game_mode)| {
                // A catacombs run's script is made from its layout ([`super::catacombs`]).
                let script = match self.catacomb_run() {
                    Some(catacomb) => catacomb.script().clone(),
                    None => self.content.dungeon_scripts.game_mode(&game_mode)?.clone(),
                };
                tracing::info!(
                    %instance,
                    game_mode = %game_mode,
                    encounters = script.encounters.len(),
                    chests = script.chests.len(),
                    "dungeon script"
                );
                Some(DungeonRun::of_trial(instance, script, daily))
            });
            self.encounter_roots = self.dungeon_run.as_ref().map_or_else(
                super::encounter_roots::EncounterRoots::default,
                |run| {
                    super::encounter_roots::EncounterRoots::build(
                        &self.content.encounter_roots,
                        &run.script.encounters,
                    )
                },
            );
        }
        let Some(mut run) = self.dungeon_run.take() else {
            return;
        };
        let mut view = ScriptView {
            events,
            ..ScriptView::default()
        };
        self.see_players(&mut view);
        // A catacombs run holds its extractions until they open ([`super::catacombs`]).
        let owned = self
            .catacomb_run()
            .map(super::catacombs::CatacombRun::owned_encounters)
            .unwrap_or_default();
        let host = &mut SimHost {
            world: self,
            mutated: true,
        };
        let outcomes = run.tick(host, &view, &owned, now_ms);
        let owned = vec![true; run.script.encounters.len()];
        self.show_encounters(&mut run, &owned, now_ms);
        self.tick_gates(&run, now_ms);
        self.expedition_encounter_events(&run);
        // An expedition ends at its last boss's task, not when every encounter of its level
        // script is done ([`SimWorld::expedition_reached_end`]).
        if !run.completed && self.expedition_reached_end() {
            run.completed = true;
            self.instance_run_ended(&run, "the last boss task ended", now_ms);
        }
        for outcome in outcomes {
            match outcome {
                ScriptOutcome::EncounterDone(index) => {
                    self.catacomb_encounter_done(index);
                    if let Some((at, radius_m, event)) = run.success_event_of(index) {
                        let event = event.to_owned();
                        self.encounter_succeeded(position(at), radius_m, &event, now_ms);
                    }
                    let chests: Vec<(String, [f32; 3])> = run
                        .chests_of(index)
                        .map(|(table, at)| (table.to_owned(), at))
                        .collect();
                    tracing::info!(
                        encounter = %run.script.encounters[index].name,
                        chests = chests.len(),
                        "encounter done"
                    );
                    for (table, _) in chests {
                        self.open_dungeon_chest(&table, now_ms);
                    }
                }
                // A catacombs run ends by its extraction, not by its encounters.
                ScriptOutcome::Completed if self.catacomb_run().is_some() => {}
                ScriptOutcome::Completed => {
                    self.instance_run_ended(&run, "every encounter done", now_ms);
                }
            }
        }
        self.dungeon_run = Some(run);
    }

    /// The instance's run ended for `why`: a mutated run's rewards, the expedition's end time
    /// and score, every player's finish of the game mode (`TaskCompleteDungeon`) and a soul
    /// trial's daily end ([`super::soul_trials`]).
    fn instance_run_ended(&mut self, run: &DungeonRun, why: &str, now_ms: u64) {
        tracing::info!(instance = %run.instance, why, "instance run ended");
        self.complete_mutated_instance(now_ms);
        self.complete_expedition_run(now_ms);
        self.game_mode_completed(now_ms);
        self.soul_trial_ended(now_ms);
    }

    /// The places of the players, for a script's view.
    /// Whether a player in this sim plays game mode `game_mode`'s daily trial: it holds the
    /// mode's daily trial flag and not its done flag ([`daily_trial_flags`]).
    fn daily_trial_player(&mut self, game_mode: &str) -> bool {
        let (flag, done) = daily_trial_flags(game_mode);
        let rules = &self.achievement_rules;
        let mut query = self.world.query::<&Player>();
        query.iter(&self.world).any(|player| {
            player.flow.has_achievement(rules, &flag) && !player.flow.has_achievement(rules, &done)
        })
    }

    fn see_players(&mut self, view: &mut ScriptView) {
        let mut query = self.world.query::<(&Player, &Body, &Vitals)>();
        for (_, body, vitals) in query.iter(&self.world) {
            let at = body.motion.position();
            view.present = true;
            view.everyone.push(at);
            if !vitals.health.is_dead() {
                view.living.push(at);
            }
        }
    }

    /// The open world's encounters at `now_ms`, with the encounter events `events` of the mobs'
    /// trees: the script of this sim's level, when the dungeon script file holds one.
    fn tick_world_events(&mut self, events: Vec<(u64, String)>, now_ms: u64) {
        if self.world_events.is_none() {
            let Some(script) = self
                .settings
                .world_events
                .then(|| self.content.dungeon_scripts.game_mode(OPEN_WORLD_LEVEL))
                .flatten()
                .cloned()
            else {
                return;
            };
            tracing::info!(
                encounters = script.encounters.len(),
                "open world encounters"
            );
            let rules = OpenWorldRules {
                near_m: self.combat.wake_radius_m,
                respawn_ms: self.combat.mob_respawn_ms,
            };
            self.encounter_roots = super::encounter_roots::EncounterRoots::build(
                &self.content.encounter_roots,
                &script.encounters,
            );
            tracing::info!(
                roots = self.encounter_roots.len(),
                "open world encounter roots"
            );
            self.encounter_objects = EncounterObjects::build(&self.content.encounter_objects);
            let objects = &self.encounter_objects;
            let run = DungeonRun::of_open_world(script, rules, &|slice| objects.is_kind(slice));
            self.world_events = Some(run);
        }
        let Some(mut run) = self.world_events.take() else {
            return;
        };
        let mut view = ScriptView {
            events,
            ..ScriptView::default()
        };
        self.see_players(&mut view);
        let owned = self.runs_here(&run);
        let host = &mut SimHost {
            world: self,
            mutated: false,
        };
        for outcome in run.tick(host, &view, &owned, now_ms) {
            if let ScriptOutcome::EncounterDone(index) = outcome {
                tracing::debug!(
                    encounter = %run.script.encounters[index].name,
                    "open world encounter done"
                );
                if let Some((at, radius_m, event)) = run.rewards_of(index) {
                    let event = event.to_owned();
                    self.reward_encounter(position(at), radius_m, &event, now_ms);
                }
                if let Some((at, radius_m, event)) = run.success_event_of(index) {
                    let event = event.to_owned();
                    self.encounter_succeeded(position(at), radius_m, &event, now_ms);
                }
            }
        }
        self.show_encounters(&mut run, &owned, now_ms);
        self.world_events = Some(run);
    }

    /// Which open world encounters of `run` this sim runs now, by index: those of its own cells
    /// in their time of day ([`super::day_night::TimedPlaces`]). An encounter out of its time
    /// resets as one of another sim's cell does.
    fn runs_here(&self, run: &DungeonRun) -> Vec<bool> {
        let phase = self.day_phase();
        run.script
            .encounters
            .iter()
            .zip(&run.encounters)
            .map(|(encounter, state)| {
                let [x, y, z] = encounter.position;
                self.owns(Point { x, y, z })
                    && self.content.time_of_day.slice_holds(
                        &encounter.slice,
                        phase,
                        state.status != EncounterStatus::Idle,
                    )
            })
            .collect()
    }

    /// The encounter roots after a step of `run` at `now_ms`: each root shows its encounter's
    /// statuses when this sim owns it (`owned`, by encounter), and the stages' entity events go
    /// out on it.
    fn show_encounters(&mut self, run: &mut DungeonRun, owned: &[bool], now_ms: u64) {
        for (index, owned) in owned.iter().enumerate() {
            if let Some((manager, stages)) = run.client_status(index) {
                let shown = *owned && run.chosen(index);
                self.encounter_roots.set(index, shown, manager, &stages);
                self.encounter_roots
                    .set_objective(index, |tasks| run.objective_phase(index, tasks));
            }
        }
        for (index, entity, event) in run.take_entity_events() {
            let Some(encounter) = run.script.encounters.get(index) else {
                continue;
            };
            let components = self
                .content
                .dungeon_gates
                .slice_entities(&encounter.slice)
                .iter()
                .find(|known| known.entity == entity)
                .map_or(&[][..], |known| known.components.as_slice());
            self.encounter_roots
                .entity_event(index, components, event, now_ms);
        }
        self.encounter_roots.tick(now_ms);
    }

    /// Each player within `radius_m` of `at` counts the encounter's success event `event` for
    /// its quests (`TaskGameEvent`): the step `04A_Side02_02_01a_Task_Encounter_01` waits for
    /// `04A_Side02_02_Encounter_01_Success`, the success event of its encounter's slice. It is no
    /// game event entry of the player (module text: retail sent no `Darkness_Any_GlobalSuccess`).
    fn encounter_succeeded(&mut self, at: Position, radius_m: f32, event: &str, now_ms: u64) {
        for number in self.player_numbers_within(at, radius_m) {
            self.quest_event(number, crate::objectives::Event::GameEvent(event), now_ms);
        }
    }

    /// The numbers of the players within `radius_m` of `at`, horizontally, the dead too.
    fn player_numbers_within(&mut self, at: Position, radius_m: f32) -> Vec<u64> {
        let mut query = self.world.query::<(&Player, &Body)>();
        query
            .iter(&self.world)
            .filter(|(_, body)| body.motion.position().horizontal_distance(at) <= radius_m)
            .map(|(player, _)| player.number)
            .collect()
    }

    /// Each player within `radius_m` of `at` gets the game event `event` (a won breach's
    /// reward, see the module text).
    fn reward_encounter(&mut self, at: Position, radius_m: f32, event: &str, now_ms: u64) {
        for number in self.player_numbers_within(at, radius_m) {
            self.fire_game_event(number, event, now_ms);
        }
    }

    /// Every player of the instance finished its game mode (`TaskCompleteDungeon`).
    fn game_mode_completed(&mut self, now_ms: u64) {
        let Some(mode) = self
            .instance_run
            .as_ref()
            .map(|run| run.game_mode().to_owned())
        else {
            return;
        };
        let numbers: Vec<u64> = {
            let mut query = self.world.query::<&Player>();
            query
                .iter(&self.world)
                .map(|player| player.number)
                .collect()
        };
        for number in numbers {
            self.quest_event(number, Event::GameModeCompleted(&mode), now_ms);
            self.update_quest_tracker(number);
        }
    }

    /// Every player of the instance gets the loot of loot table `table` (each its own roll, as
    /// an instanced loot container; needs live check: retail players open the container).
    fn open_dungeon_chest(&mut self, table: &str, now_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let numbers: Vec<u64> = {
            let mut query = self.world.query::<&Player>();
            query
                .iter(&self.world)
                .map(|player| player.number)
                .collect()
        };
        for number in numbers {
            let reached = self.reached_loot_limits(number);
            let context = self.loot_context(number, None, &reached);
            let dice = &mut self.dice;
            let rolled = content
                .loot
                .roll(table, context, &mut |below| dice.below(below));
            let hints = rolled.gear_scores.clone();
            let loot = self.limit_loot(number, rolled);
            self.send_loot_limits(number, LimitsSent::Changed);
            let (items, rolls) = self.roll_drops(number, None, loot, &hints);
            self.give_drops(
                number,
                items.into_iter().zip(rolls).collect(),
                nw_messages::items::SYNC_MOVED,
                now_ms,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nw_world::dungeon_scripts::{SpawnerMember, Stage, StageObjective, Target, TargetSlice};

    use super::*;

    const BOSS: usize = 0;
    const ADDS: usize = 1;
    const PLACE: [f32; 3] = [886.0, 906.0, 141.0];
    const RADIUS_M: f32 = 30.0;
    const INTRO_S: u32 = 20;
    const GRACE_S: u32 = 20;
    const BOSS_VITALS: &str = "Withered_Brute_Named_08";
    /// The name part of the test host's start object slices.
    const START_OBJECT: &str = "io_worldevent_";
    /// The part of a hazard object's slice path that the test host spawns as an object.
    const HAZARD_OBJECT: &str = "falling_debris";
    const ADD_VITALS: &str = "Risen_DG_Windsward_00";
    const LEVEL: u32 = 25;
    const T0: u64 = 1_000;

    /// A test host: spawned mobs by entity with their health and phase.
    #[derive(Default)]
    struct Host {
        next: u64,
        /// The last slice area handle: apart from the entity numbers, which `kill` reads as the
        /// order of `spawned`.
        next_area: u64,
        health: HashMap<u64, u32>,
        phase: HashMap<u64, u32>,
        spawned: Vec<(String, [f32; 3])>,
        /// The yaw of each start object spawned, in order.
        object_yaws: Vec<f32>,
        removed: Vec<u64>,
        /// The rolls `pick` gives, in order; 0 once they are used up.
        rolls: std::collections::VecDeque<u32>,
        /// The living players and their places, for `afflict`.
        players: Vec<(u64, [f32; 3])>,
        /// Each player `afflict` gave effects to, with the time.
        afflicted: Vec<(u64, u64)>,
        /// Each player whose effects `end_effects` ended, with the time.
        ended: Vec<(u64, u64)>,
        /// The start objects a player gathers now.
        gathered: std::collections::HashSet<u64>,
        /// The slices that have effect areas ([`ScriptHost::open_slice_areas`]).
        area_slices: Vec<String>,
        /// The open slice areas: handle and slice.
        open_areas: Vec<(u64, String)>,
        /// Each mob `engage` gave a target, with that player.
        engaged: Vec<(u64, u64)>,
    }

    impl ScriptHost for Host {
        fn spawn(
            &mut self,
            mob: &TargetMob,
            at: [f32; 3],
            _waypoints: Option<&PatrolPath>,
        ) -> Option<u64> {
            self.next += 1;
            self.health.insert(self.next, 100);
            self.spawned.push((mob.vitals_id.clone(), at));
            Some(self.next)
        }
        fn spawn_object(&mut self, slice: &str, at: [f32; 3], yaw_deg: f32) -> Option<u64> {
            (slice.contains(START_OBJECT) || slice.contains(HAZARD_OBJECT)).then(|| {
                self.next += 1;
                self.health.insert(self.next, OBJECT_HEALTH);
                self.spawned.push((slice.to_owned(), at));
                self.object_yaws.push(yaw_deg);
                self.next
            })
        }
        fn remove(&mut self, entity: u64) {
            self.health.remove(&entity);
            self.removed.push(entity);
        }
        fn afflict(
            &mut self,
            (at, radius_m): ([f32; 3], f32),
            _effects: &[String],
            (_source, now_ms): (u64, u64),
            skip: &[u64],
        ) -> Vec<u64> {
            let given: Vec<u64> = self
                .players
                .iter()
                .filter(|(player, place)| {
                    !skip.contains(player) && (place[0] - at[0]).hypot(place[1] - at[1]) <= radius_m
                })
                .map(|(player, _)| *player)
                .collect();
            self.afflicted
                .extend(given.iter().map(|player| (*player, now_ms)));
            given
        }
        fn end_effects(&mut self, players: &[u64], _effects: &[String], _source: u64, now_ms: u64) {
            self.ended
                .extend(players.iter().map(|player| (*player, now_ms)));
        }
        fn health(&self, entity: u64) -> Option<u32> {
            self.health.get(&entity).copied()
        }
        fn boss_phase(&self, entity: u64) -> Option<u32> {
            self.phase.get(&entity).copied()
        }
        fn being_gathered(&self, entity: u64) -> bool {
            self.gathered.contains(&entity)
        }
        fn pick(&mut self, _below: u32) -> u32 {
            self.rolls.pop_front().unwrap_or_default()
        }
        fn open_slice_areas(&mut self, slice: &str, _at: [f32; 3], _yaw_deg: f32) -> Option<u64> {
            self.area_slices
                .iter()
                .any(|known| known == slice)
                .then(|| {
                    self.next_area += 1;
                    self.open_areas.push((self.next_area, slice.to_owned()));
                    self.next_area
                })
        }
        fn close_slice_areas(&mut self, handle: u64) {
            self.open_areas.retain(|(open, _)| *open != handle);
        }

        fn engage(&mut self, mob: u64, in_volume: &dyn Fn(Position) -> bool) {
            if let Some((number, _)) = self.players.iter().find(|(_, at)| in_volume(position(*at)))
            {
                self.engaged.push((mob, *number));
            }
        }
    }

    impl Host {
        /// The slices whose areas stand now, in their opening order.
        fn open_area_slices(&self) -> Vec<&str> {
            self.open_areas
                .iter()
                .map(|(_, slice)| slice.as_str())
                .collect()
        }

        fn kill(&mut self, vitals: &str) {
            let entities: Vec<u64> = self
                .spawned
                .iter()
                .enumerate()
                .filter(|(_, (id, _))| id == vitals)
                .map(|(index, _)| u64::try_from(index).expect("index") + 1)
                .collect();
            for entity in entities {
                if let Some(health) = self.health.get_mut(&entity) {
                    *health = 0;
                }
            }
        }
    }

    fn target(vitals: &str) -> Target {
        Target {
            name: vitals.to_owned(),
            slices: vec![TargetSlice {
                path: format!("slices/{vitals}.dynamicslice"),
                mob: Some(TargetMob {
                    vitals_id: vitals.to_owned(),
                    category_id: String::new(),
                    level: LEVEL,
                }),
                ..TargetSlice::default()
            }],
        }
    }

    fn objective(kind: ObjectiveSpec, time_limit_s: u32, succeed: bool) -> StageObjective {
        StageObjective {
            time_limit_s,
            succeed_on_timeout: succeed,
            failure: false,
            kind,
        }
    }

    /// The Amrine boss encounter's shape: a wave that spawns the boss and ends with its death,
    /// with optional children: wait for a player, a 20 s intro, the fight.
    fn boss_encounter() -> Encounter {
        let wave = Stage {
            name: "Wave_01".to_owned(),
            child_stages_optional: true,
            children: vec![1, 2, 3],
            spawns: vec![Spawn {
                target: BOSS,
                count: 1,
                ..Spawn::default()
            }],
            objectives: vec![objective(
                ObjectiveSpec::SliceDestroyed {
                    target: Some(BOSS),
                    required: 1,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let wait = Stage {
            name: "Phase 0 Wait".to_owned(),
            objectives: vec![objective(
                ObjectiveSpec::EntityActiveState {
                    entity: 0xa8,
                    on_activated: false,
                    on_deactivated: false,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let intro = Stage {
            name: "Phase 1 Intro".to_owned(),
            objectives: vec![objective(
                ObjectiveSpec::EncounterEvent {
                    event: "Intro2_Complete".to_owned(),
                },
                INTRO_S,
                true,
            )],
            ..Stage::default()
        };
        let fight = Stage {
            name: "Phase 2 Fight".to_owned(),
            objectives: vec![objective(
                ObjectiveSpec::BossPhase {
                    target: None,
                    phase: 3,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        Encounter {
            name: "EncounterBoss".to_owned(),
            position: PLACE,
            auto_activate: true,
            grace_period_s: GRACE_S,
            detection_radius_m: Some(RADIUS_M),
            top_stages: vec![0],
            stages: vec![wave, wait, intro, fight],
            targets: vec![target(BOSS_VITALS), target(ADD_VITALS)],
            ..Encounter::default()
        }
    }

    /// `QuestMorgaine`: `EncounterBoss` and `EncounterBossDaily` name one slice; the run keeps
    /// the twin of its kind by the encounters' names.
    #[test]
    fn twins_by_name_of_one_slice_run_one_of_them() {
        const SLICE: &str = "slices/dunwood/soultrial/morgaine/enc_morgaine.dynamicslice";
        let named = |name: &str| Encounter {
            name: name.to_owned(),
            slice: SLICE.to_owned(),
            ..boss_encounter()
        };
        let both = script(vec![named("EncounterBoss"), named("EncounterBossDaily")]);
        let names = |daily: bool| -> Vec<String> {
            trial_variant(both.clone(), daily)
                .encounters
                .into_iter()
                .map(|encounter| encounter.name)
                .collect()
        };
        assert_eq!(names(false), ["EncounterBoss"]);
        assert_eq!(names(true), ["EncounterBossDaily"]);
    }

    #[test]
    fn a_solo_trial_runs_the_quest_or_the_daily_twin_of_an_encounter_never_both() {
        const QUEST: &str = "slices/dungeon/liang/enc_quest_boss.dynamicslice";
        const DAILY: &str = "slices/dungeon/liang/enc_quest_boss_daily.dynamicslice";
        const LONE: &str = "slices/dungeon/liang/enc_side.dynamicslice";
        let with_slice = |slice: &str| Encounter {
            slice: slice.to_owned(),
            ..boss_encounter()
        };
        let both = script(vec![with_slice(QUEST), with_slice(DAILY), with_slice(LONE)]);
        let slices = |daily: bool| -> Vec<String> {
            trial_variant(both.clone(), daily)
                .encounters
                .into_iter()
                .map(|encounter| encounter.slice)
                .collect()
        };
        assert_eq!(slices(false), [QUEST, LONE], "the quest's run");
        assert_eq!(slices(true), [DAILY, LONE], "the daily trial's run");
        assert_eq!(
            daily_trial_flags("QuestLiang"),
            (
                "DailyTrialLiang".to_owned(),
                "DailyTrialLiangComplete".to_owned()
            )
        );
        assert_eq!(
            daily_trial_flags("TrialSeason04Q02").0,
            "DailyTrialSeason04Q02"
        );
        let run = DungeonRun::of_trial(Uuid::nil(), both.clone(), false);
        assert_eq!(run.script().encounters.len(), 2);
        assert!(!run.daily);
    }

    fn script(encounters: Vec<Encounter>) -> LevelScript {
        LevelScript {
            level: "nw_dungeon_windsward_00".to_owned(),
            encounters,
            chests: vec![nw_world::dungeon_scripts::Chest {
                name: "LockedChest_Simon".to_owned(),
                position: [PLACE[0] - 16.0, PLACE[1] - 21.0, PLACE[2]],
                loot_table: "ChestLootElite".to_owned(),
                ..Default::default()
            }],
            ..LevelScript::default()
        }
    }

    fn view(players: &[[f32; 3]]) -> ScriptView {
        ScriptView {
            living: players.iter().map(|at| position(*at)).collect(),
            everyone: players.iter().map(|at| position(*at)).collect(),
            present: true,
            events: Vec::new(),
        }
    }

    fn stage_status(run: &DungeonRun, stage: usize) -> StageStatus {
        run.encounters[0].stages[stage].status
    }

    #[test]
    fn a_detection_volume_detects_inside_its_shape_and_no_volume_within_the_radius() {
        // A long box along x: 40 m by 4 m, at the encounter's place.
        let boxed = Encounter {
            position: PLACE,
            detector: Some(nw_world::dungeon_scripts::Detector {
                shape: nw_world::collision::Geometry::Box {
                    half_extents: [20.0, 2.0, 10.0],
                },
                place: nw_world::collision::place_of([0.0, 0.0, 0.0, 1.0], 1.0, PLACE),
            }),
            ..Encounter::default()
        };
        let at = |dx: f32, dy: f32| position([PLACE[0] + dx, PLACE[1] + dy, PLACE[2]]);
        assert!(
            detects(&boxed, at(18.0, 0.0), RADIUS_M),
            "far along the box"
        );
        assert!(
            !detects(&boxed, at(0.0, 5.0), RADIUS_M),
            "beside the box, near its middle"
        );
        let plain = Encounter {
            position: PLACE,
            ..Encounter::default()
        };
        assert!(detects(&plain, at(0.0, 5.0), RADIUS_M));
        assert!(!detects(&plain, at(RADIUS_M + 1.0, 0.0), RADIUS_M));
    }

    /// The fight radius of an encounter: its sphere's radius (`YonasRetreat`: 30 m), its box's
    /// smaller horizontal half size, else its detection radius.
    #[test]
    fn the_volume_radius_is_the_sphere_radius_or_the_narrow_half_of_the_box() {
        const SPHERE_RADIUS_M: f32 = 15.0;
        const DETECTION_RADIUS_M: f32 = 12.0;
        let detector = |shape| nw_world::dungeon_scripts::Detector {
            shape,
            place: nw_world::collision::place_of([0.0, 0.0, 0.0, 1.0], 1.0, PLACE),
        };
        let sphere = Encounter {
            detector: Some(detector(Geometry::Sphere {
                radius: SPHERE_RADIUS_M,
            })),
            detection_radius_m: Some(DETECTION_RADIUS_M),
            ..Encounter::default()
        };
        assert!((volume_radius_m(&sphere) - SPHERE_RADIUS_M).abs() < f32::EPSILON);
        let boxed = Encounter {
            detector: Some(detector(Geometry::Box {
                half_extents: [20.0, 2.0, 10.0],
            })),
            ..Encounter::default()
        };
        assert!((volume_radius_m(&boxed) - 2.0).abs() < f32::EPSILON);
        let plain = Encounter {
            detection_radius_m: Some(DETECTION_RADIUS_M),
            ..Encounter::default()
        };
        assert!((volume_radius_m(&plain) - DETECTION_RADIUS_M).abs() < f32::EPSILON);
        assert!(
            (volume_radius_m(&Encounter::default()) - DEFAULT_VOLUME_RADIUS_M).abs() < f32::EPSILON
        );
    }

    /// The debug lines of an open world encounter's state: an idle encounter holds no stage
    /// copy, and its stages read as waiting, so its start shows each stage that began.
    #[test]
    fn an_idle_encounter_s_state_holds_no_stages_and_reads_them_as_waiting() {
        let mut run =
            DungeonRun::of_open_world(script(vec![boss_encounter()]), open_rules(), &|_| false);
        let idle = EncounterState::of(&run.encounters[0]);
        assert!(idle.stages.is_empty());
        assert_eq!(idle.stage(0), StageStatus::Pending);
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        let running = EncounterState::of(&run.encounters[0]);
        assert_eq!(running.status, EncounterStatus::Running);
        assert_eq!(running.stage(0), StageStatus::Running);
        assert_ne!(idle, running);
        idle.log_changes(&running, &boss_encounter());
    }

    #[test]
    fn a_start_object_stands_until_it_is_gathered_and_then_its_stage_ends() {
        // The encounter's yaw and its location's turn: the object faces their sum.
        const ENCOUNTER_YAW_DEG: f32 = 90.0;
        const TURN_DEG: f32 = -52.0;
        let object = Target {
            name: "StartObject".to_owned(),
            slices: vec![TargetSlice {
                path: format!("slices/quest/{START_OBJECT}encstart.dynamicslice"),
                ..TargetSlice::default()
            }],
        };
        let interact = Stage {
            name: "Wave_01 - Objective: Interact IO".to_owned(),
            spawns: vec![Spawn {
                target: 0,
                count: 1,
                locations: vec![PLACE],
                turns_deg: vec![TURN_DEG],
                ..Spawn::default()
            }],
            objectives: vec![objective(
                ObjectiveSpec::SliceDestroyed {
                    target: Some(0),
                    required: 1,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let encounter = Encounter {
            position: PLACE,
            auto_activate: true,
            top_stages: vec![0],
            stages: vec![interact],
            targets: vec![object],
            yaw_deg: ENCOUNTER_YAW_DEG,
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        assert_eq!(host.spawned.len(), 1, "the start object stands");
        assert!(host.spawned[0].0.contains(START_OBJECT));
        assert_eq!(host.object_yaws, [ENCOUNTER_YAW_DEG + TURN_DEG]);
        assert_eq!(stage_status(&run, 0), StageStatus::Running);
        assert_eq!(
            run.client_status(0),
            Some((
                super::super::encounter_roots::STATUS_IN_PROGRESS,
                vec![super::super::encounter_roots::STATUS_IN_PROGRESS]
            ))
        );
        // The gather ends the object.
        host.health.clear();
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 3);
        assert_eq!(stage_status(&run, 0), StageStatus::Done);
    }

    /// The stronghold siren's `Wave_02 - Objective: Stop Cannons` (`Playthrough_Part_18`
    /// 5183.635 to 5217.065 s): the stage spawns three cannonball gatherables and its slice
    /// spawned objective counts each gather's end (the gatherable's spawner spawns its trigger
    /// then), not the spawns; the third ends the stage. The objective's task shows the count
    /// from the stage's start.
    #[test]
    fn a_slice_spawned_objective_counts_the_ends_of_its_gatherables_gathers() {
        const CANNONS: u32 = 3;
        let gatherable = Target {
            name: "Cannonball".to_owned(),
            slices: vec![TargetSlice {
                path: format!("slices/worldevents/{START_OBJECT}cannonball.dynamicslice"),
                spawned_slices: vec![
                    "slices/worldevents/rw_cannonball_trigger.dynamicslice".into(),
                ],
                ..TargetSlice::default()
            }],
        };
        let wave = Stage {
            name: "Wave_02 - Objective: Stop Cannons".to_owned(),
            user_wave: true,
            spawns: vec![Spawn {
                target: 0,
                count: CANNONS,
                locations: vec![PLACE],
                ..Spawn::default()
            }],
            objectives: vec![objective(
                ObjectiveSpec::SliceSpawned {
                    target: Some(0),
                    required: CANNONS,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let encounter = Encounter {
            position: PLACE,
            auto_activate: true,
            top_stages: vec![0],
            stages: vec![wave],
            targets: vec![gatherable],
            ..Encounter::default()
        };
        let tasks = encounter.objective_tasks().expect("tasks");
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        assert_eq!(run.objective_phase(0, &tasks), None, "idle");
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        assert_eq!(host.spawned.len(), 3, "the three gatherables stand");
        let running = |count, done| {
            Some(ObjectivePhase::Running(vec![TaskRun {
                started_ms: Some(T0),
                count,
                counts: true,
                timed: false,
                done,
            }]))
        };
        assert_eq!(
            run.objective_phase(0, &tasks),
            running(0, false),
            "spawns do not count"
        );
        let mut objects: Vec<u64> = host.health.keys().copied().collect();
        objects.sort_unstable();
        let mut now_ms = T0 + 1;
        for (count, object) in (1..CANNONS).zip(&objects) {
            host.health.remove(object);
            now_ms += 1;
            run.tick(&mut host, &view(&[PLACE]), &[], now_ms);
            assert_eq!(run.objective_phase(0, &tasks), running(count, false));
            assert_eq!(stage_status(&run, 0), StageStatus::Running);
        }
        host.health.remove(&objects[2]);
        run.tick(&mut host, &view(&[PLACE]), &[], now_ms + 1);
        run.tick(&mut host, &view(&[PLACE]), &[], now_ms + 2);
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Done,
            "the third gather ends it"
        );
        assert_eq!(run.objective_phase(0, &tasks), Some(ObjectivePhase::Ended));
    }

    #[test]
    fn the_boss_spawns_waits_for_a_player_and_its_death_ends_the_encounter() {
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![boss_encounter()]));
        let mut host = Host::default();
        let far = [0.0, 0.0, 0.0];
        run.tick(&mut host, &view(&[far]), &[], T0);
        run.tick(&mut host, &view(&[far]), &[], T0 + 1);
        assert_eq!(host.spawned, [(BOSS_VITALS.to_owned(), PLACE)]);
        assert_eq!(
            stage_status(&run, 1),
            StageStatus::Running,
            "waits for a player"
        );
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        assert_eq!(stage_status(&run, 1), StageStatus::Done);
        assert_eq!(stage_status(&run, 2), StageStatus::Running, "the intro");
        let mut events = view(&[PLACE]);
        events.events.push((1, "Intro2_Complete".to_owned()));
        run.tick(&mut host, &events, &[], T0 + 3);
        assert_eq!(stage_status(&run, 2), StageStatus::Done);
        assert_eq!(stage_status(&run, 3), StageStatus::Running, "the fight");
        host.kill(BOSS_VITALS);
        let outcomes = run.tick(&mut host, &view(&[PLACE]), &[], T0 + 4);
        assert_eq!(
            outcomes,
            [ScriptOutcome::EncounterDone(0), ScriptOutcome::Completed]
        );
        assert_eq!(run.chests_of(0).count(), 1);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 4 + CORPSE_MS);
        assert_eq!(host.removed, [1], "the body goes after its corpse time");
    }

    /// Retail `quest_18018-The-Wolf-s-Den`: the trial's wait spawns Asena 5 s in; a player who
    /// is there at the start ends the wait only once she is spawned.
    #[test]
    fn a_wait_for_a_player_ends_once_its_own_spawns_are_made() {
        const DELAY_S: u32 = 5;
        let mut encounter = boss_encounter();
        encounter.stages[1].spawns = vec![Spawn {
            target: ADDS,
            delay_s: DELAY_S,
            count: 1,
            ..Spawn::default()
        }];
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        assert_eq!(
            stage_status(&run, 1),
            StageStatus::Running,
            "its spawn is not made yet"
        );
        let spawned_at = T0 + 1 + u64::from(DELAY_S) * MS_PER_S;
        run.tick(&mut host, &view(&[PLACE]), &[], spawned_at);
        assert!(host.spawned.iter().any(|(vitals, _)| vitals == ADD_VITALS));
        run.tick(&mut host, &view(&[PLACE]), &[], spawned_at + 1);
        assert_eq!(stage_status(&run, 1), StageStatus::Done);
    }

    /// Retail `09a_m15_fightartemis`: Adiana spawns 1 s into her setup (263.141 -> 264.542 s),
    /// and her intro stage, beside the wait that waits for its parent, starts with her fight.
    #[test]
    fn the_adiana_trial_s_intro_starts_after_her_spawn() {
        use nw_content::layout::repo_layout;
        use nw_world::dungeon_scripts::DungeonScripts;
        let scripts = DungeonScripts::load(&repo_layout()).expect("dungeon scripts file");
        let script = scripts
            .game_mode("QuestAdiana")
            .expect("QuestAdiana")
            .clone();
        let at = script.encounters[0].position;
        let mut run = DungeonRun::of_trial(Uuid::nil(), script, false);
        let mut host = Host::default();
        for tick in 0..30 {
            run.tick(&mut host, &view(&[at]), &[], T0 + tick * 100);
        }
        let boss = host
            .spawned
            .iter()
            .position(|(vitals, _)| vitals == "FL_AI_Adiana_Bow");
        let boss = u64::try_from(boss.expect("the boss spawned") + 1).expect("entity");
        assert!(run.stage_started_ms(boss, "01 - Intro").is_some());
    }

    /// A boss script times its events from its encounter's stages ([`super::boss_scripts`]).
    #[test]
    fn a_boss_s_stage_start_is_known_while_it_runs_and_after_it() {
        const BOSS: u64 = 1;
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![boss_encounter()]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[[0.0, 0.0, 0.0]]), &[], T0);
        run.tick(&mut host, &view(&[[0.0, 0.0, 0.0]]), &[], T0 + 1);
        assert_eq!(run.stage_started_ms(BOSS, "Phase 1 Intro"), None, "not yet");
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        assert_eq!(run.stage_started_ms(BOSS, "Phase 1 Intro"), Some(T0 + 2));
        let mut events = view(&[PLACE]);
        events.events.push((BOSS, "Intro2_Complete".to_owned()));
        run.tick(&mut host, &events, &[], T0 + 3);
        assert_eq!(
            run.stage_started_ms(BOSS, "Phase 1 Intro"),
            Some(T0 + 2),
            "done"
        );
        assert_eq!(run.stage_started_ms(BOSS, "No such stage"), None);
        assert_eq!(
            run.stage_started_ms(BOSS + 100, "Phase 1 Intro"),
            None,
            "no mob of it"
        );
    }

    /// A boss's intro is over once an intro stage started and none runs; a mob that no top
    /// stage tracks has none.
    #[test]
    fn a_boss_intro_is_over_once_its_intro_stages_ran() {
        const BOSS_MOB: u64 = 1;
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![boss_encounter()]));
        let mut host = Host::default();
        let far = [0.0, 0.0, 0.0];
        run.tick(&mut host, &view(&[far]), &[], T0);
        run.tick(&mut host, &view(&[far]), &[], T0 + 1);
        assert_eq!(run.boss_intro_over(BOSS_MOB), Some(false), "no intro yet");
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        assert_eq!(run.boss_intro_over(BOSS_MOB), Some(false), "the intro runs");
        let mut events = view(&[PLACE]);
        events.events.push((BOSS_MOB, "Intro2_Complete".to_owned()));
        run.tick(&mut host, &events, &[], T0 + 3);
        assert_eq!(run.boss_intro_over(BOSS_MOB), Some(true));
        assert_eq!(run.boss_intro_over(BOSS_MOB + 100), None, "no mob of it");
    }

    /// Retail `DungeonFirstLight01` `Encounter_SecondBoss`: the main wave ends with the
    /// mammoth's death only after its children ran: the wait, the intro that waits for
    /// `Intro_Complete` with no time limit (2.7 s in both retail runs), and the fight. The
    /// running intro names the event it waits for, and the event ends it.
    #[test]
    fn a_running_intro_names_the_event_it_waits_for_and_the_wave_waits_for_it() {
        const BOSS_MOB: u64 = 1;
        const INTRO_EVENT: &str = "Intro_Complete";
        let mut encounter = boss_encounter();
        encounter.stages[0].child_stages_optional = false;
        encounter.stages[2].objectives = vec![objective(
            ObjectiveSpec::EncounterEvent {
                event: INTRO_EVENT.to_owned(),
            },
            0,
            false,
        )];
        encounter.stages[3].objectives.clear();
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        assert_eq!(
            run.awaited_intro_events(BOSS_MOB),
            [(INTRO_EVENT.to_owned(), T0)],
            "a player at the encounter ended the wait at its start"
        );
        host.kill(BOSS_VITALS);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 3);
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Running,
            "the dead boss's wave waits for its intro"
        );
        let mut events = view(&[PLACE]);
        events.events.push((BOSS_MOB, INTRO_EVENT.to_owned()));
        run.tick(&mut host, &events, &[], T0 + 4);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 5);
        assert!(run.awaited_intro_events(BOSS_MOB).is_empty());
        assert_eq!(stage_status(&run, 0), StageStatus::Done);
    }

    /// A spawner slice of mixed mobs makes each member at its own place, and a wave that tracks
    /// one member's creature slice ends with that member's death only (the Shipyard cave's
    /// `Wave_01` ends when its summoner dies, not its swarmers).
    /// Retail `QuestMedusa`: its `Wave 0 - Setup` spawns an area trigger slice at the arena and
    /// waits for it to be destroyed; a player who comes into it ends the setup, one far from it
    /// does not.
    #[test]
    fn a_player_in_a_spawned_area_trigger_ends_the_stage_that_tracks_it() {
        // The second slice is one the level knows as an object (the open world's encounter
        // objects hold the trigger slice): the host spawns it, and the trigger counts all the
        // same.
        for trigger in [
            "slices/encounters/ai_encounter_area_trigger_20m.dynamicslice".to_owned(),
            format!("slices/encounters/ai_encounter_area_trigger_{START_OBJECT}.dynamicslice"),
        ] {
            a_player_in_the_trigger_ends_its_stage(&trigger);
        }
    }

    fn a_player_in_the_trigger_ends_its_stage(trigger: &str) {
        let trigger_at = [PLACE[0] + 10.0, PLACE[1], PLACE[2]];
        let setup = Stage {
            name: "Wave 0 - Setup".to_owned(),
            spawns: vec![Spawn {
                target: 0,
                count: 1,
                locations: vec![trigger_at],
                ..Spawn::default()
            }],
            objectives: vec![objective(
                ObjectiveSpec::SliceDestroyed {
                    target: Some(0),
                    required: 1,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let encounter = Encounter {
            name: "EncounterBoss".to_owned(),
            position: PLACE,
            auto_activate: true,
            grace_period_s: GRACE_S,
            detection_radius_m: Some(RADIUS_M),
            top_stages: vec![0],
            stages: vec![setup],
            targets: vec![Target {
                name: trigger.to_owned(),
                slices: vec![TargetSlice {
                    path: trigger.to_owned(),
                    ..TargetSlice::default()
                }],
            }],
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        let far = [PLACE[0] - RADIUS_M + 1.0, PLACE[1], PLACE[2]];
        run.tick(&mut host, &view(&[far]), &[], T0);
        run.tick(&mut host, &view(&[far]), &[], T0 + 1);
        run.tick(&mut host, &view(&[far]), &[], T0 + 2);
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Running,
            "no player in it"
        );
        run.tick(&mut host, &view(&[trigger_at]), &[], T0 + 3);
        run.tick(&mut host, &view(&[trigger_at]), &[], T0 + 4);
        assert_eq!(stage_status(&run, 0), StageStatus::Done);
    }

    #[test]
    fn a_mixed_spawner_slice_spawns_each_member_and_only_its_tracked_member_ends_the_wave() {
        const SWARMER: &str = "Corruption_Swarmer_DG_Ebonscale_00";
        const SUMMONER: &str = "Dynasty_Summoner_Named_DG_Ebonscale_00";
        const SUMMONER_SLICE: &str = "slices/characters/corrupted_dynasty_summoner.dynamicslice";
        const SUMMONER_AT: [f32; 3] = [-1.0, 9.0, 0.0];
        const SPAWNER: usize = 0;
        const TRACKED: usize = 1;
        let member = |vitals: &str, offset: [f32; 3], slice: &str| SpawnerMember {
            mob: TargetMob {
                vitals_id: vitals.to_owned(),
                category_id: String::new(),
                level: LEVEL,
            },
            offset,
            slices: vec![slice.to_owned()],
            waypoints: None,
        };
        let mut spawner = target(SWARMER);
        spawner.slices[0].members = vec![
            member(
                SWARMER,
                [0.0; 3],
                "slices/characters/corruption_swarmer.dynamicslice",
            ),
            member(SUMMONER, SUMMONER_AT, SUMMONER_SLICE),
        ];
        let tracked = Target {
            name: SUMMONER_SLICE.to_owned(),
            slices: vec![TargetSlice {
                path: SUMMONER_SLICE.to_owned(),
                ..TargetSlice::default()
            }],
        };
        let wave = Stage {
            name: "Wave_01 - Objective: Kill Enemies".to_owned(),
            spawns: vec![Spawn {
                target: SPAWNER,
                count: 1,
                ..Spawn::default()
            }],
            objectives: vec![objective(
                ObjectiveSpec::SliceDestroyed {
                    target: Some(TRACKED),
                    required: 1,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let cave = Encounter {
            name: "EncounterCave".to_owned(),
            position: PLACE,
            auto_activate: true,
            grace_period_s: GRACE_S,
            detection_radius_m: Some(RADIUS_M),
            top_stages: vec![0],
            stages: vec![wave],
            targets: vec![spawner, tracked],
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![cave]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        let summoner_at = [
            PLACE[0] + SUMMONER_AT[0],
            PLACE[1] + SUMMONER_AT[1],
            PLACE[2] + SUMMONER_AT[2],
        ];
        assert_eq!(
            host.spawned,
            [
                (SWARMER.to_owned(), PLACE),
                (SUMMONER.to_owned(), summoner_at)
            ]
        );
        host.kill(SWARMER);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Running,
            "a swarmer died"
        );
        host.kill(SUMMONER);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 3);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 4);
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Done,
            "the summoner died"
        );
    }

    /// A spawn group's area spawner gives the respawn range of its encounter, and one that does
    /// not spawn when enabled keeps its encounter from starting by itself.
    #[test]
    fn an_encounter_respawns_in_its_spawner_s_range_and_starts_by_its_spawner() {
        use nw_world::level_script::{SpawnerKind, SpawnerRules};
        const RESPAWN_MS: u64 = 60_000;
        let rules = OpenWorldRules {
            near_m: 100.0,
            respawn_ms: RESPAWN_MS,
        };
        let area = SpawnerRules {
            kind: SpawnerKind::Area,
            live_count: 1,
            respawn_s: [900.0, 1_200.0],
            spawn_on_enable: true,
            respawn_after_death: true,
            maintain_population: false,
        };
        let plain = boss_encounter();
        assert_eq!(respawn_ms(&plain, rules, T0), RESPAWN_MS, "no rules");
        let grouped = Encounter {
            spawner: Some(area),
            ..boss_encounter()
        };
        for done in [T0, T0 + 1, T0 + 77_777] {
            let ms = respawn_ms(&grouped, rules, done);
            assert!((900_000..=1_200_000).contains(&ms), "{ms}");
            assert_eq!(ms, respawn_ms(&grouped, rules, done), "one time per end");
        }
        // The holdout's spawner range (Observed, `WorldEvent_Holdout`).
        const HOLDOUT_RANGE_S: [f32; 2] = [10.0, 0.0];
        const HOLDOUT_RESPAWN_MS: u64 = 10_000;
        let holdout = Encounter {
            spawner: Some(SpawnerRules {
                respawn_s: HOLDOUT_RANGE_S,
                ..area
            }),
            ..boss_encounter()
        };
        assert_eq!(
            respawn_ms(&holdout, rules, T0),
            HOLDOUT_RESPAWN_MS,
            "an end below the start"
        );
        assert!(starts_when_enabled(&grouped));
        let triggered = Encounter {
            spawner: Some(SpawnerRules {
                spawn_on_enable: false,
                ..area
            }),
            auto_activate: false,
            ..boss_encounter()
        };
        assert!(!starts_when_enabled(&triggered), "it waits for its trigger");
    }

    /// Retail `09a_s17_establishdefenses` (116.8 s): the prowlers spawned 31 and 32 m from the
    /// centre and ran at the player. Each mob an open world encounter spawns takes a player in
    /// its volume; a player out of it is no target.
    #[test]
    fn an_open_world_encounter_s_mobs_engage_a_player_in_its_volume() {
        const PLAYER: u64 = 42;
        const OUTSIDE: u64 = 43;
        let rules = OpenWorldRules {
            near_m: RADIUS_M,
            respawn_ms: open_rules().respawn_ms,
        };
        let far = [PLACE[0] + RADIUS_M * 3.0, PLACE[1], PLACE[2]];
        let mut run = DungeonRun::of_open_world(script(vec![boss_encounter()]), rules, &|_| false);
        let mut host = Host {
            players: vec![(OUTSIDE, far), (PLAYER, PLACE)],
            ..Host::default()
        };
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        assert_eq!(host.spawned.len(), 1);
        assert_eq!(host.engaged, [(1, PLAYER)]);
        let mut instance = DungeonRun::new(Uuid::nil(), script(vec![boss_encounter()]));
        let mut inside = Host {
            players: vec![(PLAYER, PLACE)],
            ..Host::default()
        };
        instance.tick(&mut inside, &view(&[PLACE]), &[], T0);
        instance.tick(&mut inside, &view(&[PLACE]), &[], T0 + 1);
        assert!(
            inside.engaged.is_empty(),
            "an instance's encounter picks by its own script"
        );
    }

    /// Build 25810808 `99A_Enc_MSQ_09_Torment`: an upright cylinder from the tower top's floor
    /// (z 153.84) 10 m up. A player on that floor is in it by its body; one on the ring 2.3 m
    /// lower is not. The plan of its fight names its heights.
    #[test]
    fn an_upright_cylinder_volume_has_levels_and_detects_a_body_on_its_floor() {
        const FLOOR_M: f32 = 153.844;
        const HALF_HEIGHT_M: f32 = 5.0;
        const RING_M: f32 = 151.5;
        let at = |z: f32| Position {
            x: PLACE[0],
            y: PLACE[1],
            z,
        };
        let mut place = nw_world::collision::IDENTITY_AFFINE;
        place[0][3] = PLACE[0];
        place[1][3] = PLACE[1];
        place[2][3] = FLOOR_M + HALF_HEIGHT_M;
        let encounter = Encounter {
            position: [PLACE[0], PLACE[1], FLOOR_M],
            detector: Some(nw_world::dungeon_scripts::Detector {
                shape: Geometry::Cylinder {
                    from: [0.0, 0.0, -HALF_HEIGHT_M],
                    to: [0.0, 0.0, HALF_HEIGHT_M],
                    radius: RADIUS_M,
                },
                place,
            }),
            ..Encounter::default()
        };
        let (low, high) = volume_levels(&encounter).expect("levels");
        assert!(
            (low - FLOOR_M).abs() < 1e-3 && (high - FLOOR_M - 2.0 * HALF_HEIGHT_M).abs() < 1e-3
        );
        assert!(
            detects(&encounter, at(FLOOR_M - 0.01), RADIUS_M),
            "feet on the floor"
        );
        assert!(
            !detects(&encounter, at(RING_M), RADIUS_M),
            "on the ring under it"
        );
        assert_eq!(volume_levels(&boss_encounter()), None, "no cylinder");
    }

    #[test]
    fn a_fight_radius_is_the_volume_s_radius_less_its_middle_s_distance_from_the_place() {
        const OFF_MIDDLE_M: f32 = 10.0;
        let mut place = nw_world::collision::IDENTITY_AFFINE;
        place[0][3] = PLACE[0] + OFF_MIDDLE_M;
        place[1][3] = PLACE[1];
        place[2][3] = PLACE[2];
        let encounter = Encounter {
            position: PLACE,
            detector: Some(nw_world::dungeon_scripts::Detector {
                shape: Geometry::Sphere { radius: RADIUS_M },
                place,
            }),
            ..Encounter::default()
        };
        assert!((fight_radius_m(&encounter) - (RADIUS_M - OFF_MIDDLE_M)).abs() < 1e-3);
        let centred = Encounter {
            position: PLACE,
            detection_radius_m: Some(RADIUS_M),
            ..Encounter::default()
        };
        assert!((fight_radius_m(&centred) - RADIUS_M).abs() < 1e-3);
    }

    /// Build 25810808 `WorldEvent_09A_M5_WaveEvent`: `Wave_00 - Wait for Players` waits for
    /// a slice entity to go off, which no stage action does. In the open world a player in the
    /// volume ends the wait; an instance's script still waits.
    #[test]
    fn an_open_world_wait_for_an_entity_no_stage_changes_ends_with_a_player_in_the_volume() {
        const START_TRIGGER: u64 = 23_997_448_385_935;
        let wait = Stage {
            name: "Wave_00 - Wait for Players".to_owned(),
            objectives: vec![objective(
                ObjectiveSpec::EntityActiveState {
                    entity: START_TRIGGER,
                    on_activated: false,
                    on_deactivated: true,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let encounter = Encounter {
            name: "WorldEvent_WaveEvent".to_owned(),
            position: PLACE,
            auto_activate: true,
            grace_period_s: GRACE_S,
            detection_radius_m: Some(RADIUS_M),
            top_stages: vec![0],
            stages: vec![wait],
            ..Encounter::default()
        };
        let mut host = Host::default();
        let mut instance = DungeonRun::new(Uuid::nil(), script(vec![encounter.clone()]));
        let mut open = DungeonRun::of_open_world(script(vec![encounter]), open_rules(), &|_| false);
        for tick in 0..4 {
            instance.tick(&mut host, &view(&[PLACE]), &[], T0 + tick);
            open.tick(&mut host, &view(&[PLACE]), &[], T0 + tick);
        }
        assert_eq!(stage_status(&instance, 0), StageStatus::Running);
        assert_eq!(stage_status(&open, 0), StageStatus::Done);
    }

    #[test]
    fn an_open_world_encounter_runs_while_a_player_is_near_and_comes_back_after_its_end() {
        // Retail `12-8_demonstorm`: the storm priest shows `Spawn` when the player comes
        // within 84 m of him.
        const NEAR_M: f32 = 100.0;
        const RESPAWN_MS: u64 = 60_000;
        let rules = OpenWorldRules {
            near_m: NEAR_M,
            respawn_ms: RESPAWN_MS,
        };
        let far = [PLACE[0] + NEAR_M * 3.0, PLACE[1], PLACE[2]];
        let mut run = DungeonRun::of_open_world(script(vec![boss_encounter()]), rules, &|_| false);
        let mut host = Host::default();
        run.tick(&mut host, &view(&[far]), &[], T0);
        run.tick(&mut host, &view(&[far]), &[], T0 + 1);
        assert!(host.spawned.is_empty(), "no player is near");
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 3);
        assert_eq!(host.spawned, [(BOSS_VITALS.to_owned(), PLACE)]);
        // Every player leaves for the grace period: its mobs go and it waits again.
        let left = T0 + 4;
        run.tick(&mut host, &view(&[far]), &[], left);
        assert!(host.removed.is_empty(), "the grace period runs");
        run.tick(
            &mut host,
            &view(&[far]),
            &[],
            left + u64::from(GRACE_S) * MS_PER_S,
        );
        assert_eq!(host.removed, [1]);
        // A player comes back: it starts again; another sim's cell takes it away.
        let back = left + u64::from(GRACE_S) * MS_PER_S + 1;
        run.tick(&mut host, &view(&[PLACE]), &[], back);
        run.tick(&mut host, &view(&[PLACE]), &[], back + 1);
        assert_eq!(host.spawned.len(), 2);
        run.tick(&mut host, &view(&[PLACE]), &[false], back + 2);
        assert_eq!(host.removed, [1, 2], "an encounter of another sim's cell");
        // It runs to its end, and starts again after the respawn time.
        run.tick(&mut host, &view(&[PLACE]), &[], back + 3);
        run.tick(&mut host, &view(&[PLACE]), &[], back + 4);
        let mut events = view(&[PLACE]);
        events.events.push((3, "Intro2_Complete".to_owned()));
        run.tick(&mut host, &events, &[], back + 5);
        host.kill(BOSS_VITALS);
        let outcomes = run.tick(&mut host, &view(&[PLACE]), &[], back + 6);
        assert!(outcomes.contains(&ScriptOutcome::EncounterDone(0)));
        let spawned = host.spawned.len();
        run.tick(&mut host, &view(&[PLACE]), &[], back + 7);
        assert_eq!(host.spawned.len(), spawned, "it waits for its respawn time");
        run.tick(&mut host, &view(&[PLACE]), &[], back + 7 + RESPAWN_MS);
        run.tick(&mut host, &view(&[PLACE]), &[], back + 8 + RESPAWN_MS);
        run.tick(&mut host, &view(&[PLACE]), &[], back + 9 + RESPAWN_MS);
        assert_eq!(host.spawned.len(), spawned + 1, "it starts again");
    }

    #[test]
    fn a_spawn_of_a_nested_encounter_slice_makes_each_of_its_mobs() {
        // The storm priest's minion portal: one spawn of the nested encounter slice makes its
        // two swarmers (retail `12-9_demonstorm2`: two `Corruption_Swarmer_Chaplain_Minion` at
        // 121.0 s and 122.3 s, two more 15 s later).
        const PER_PORTAL: u32 = 2;
        let mut encounter = boss_encounter();
        for target in &mut encounter.targets {
            for slice in &mut target.slices {
                slice.mobs_per_spawn = PER_PORTAL;
            }
        }
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        assert_eq!(host.spawned.len(), PER_PORTAL as usize);
    }

    /// The extraction's survive stage: its area spawner's mobs stand at its points turned with
    /// the encounter, a dead one comes back at its point after the shortest respawn time while
    /// the stage runs (its wave is never "all dead"), and none comes back after the stage's 60 s.
    #[test]
    fn an_area_spawner_refills_its_points_while_its_stage_runs() {
        const LIVE: u32 = 2;
        const SURVIVE_S: u32 = 60;
        const SHORTEST_S: f32 = 2.0;
        const TURN_DEG: f32 = 90.0;
        let survive = Stage {
            name: "Wave_02 - Survive".to_owned(),
            spawns: vec![Spawn {
                target: 0,
                count: 1,
                locations: vec![PLACE],
                ..Spawn::default()
            }],
            objectives: vec![objective(ObjectiveSpec::InvasionWave, SURVIVE_S, true)],
            ..Stage::default()
        };
        let mut fodder = target(ADD_VITALS);
        fodder.slices[0].mobs_per_spawn = LIVE;
        fodder.slices[0].refill = Some(nw_world::dungeon_scripts::AreaRefill {
            respawn_s: [SHORTEST_S, 7.0],
            points: vec![[1.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
        });
        let encounter = Encounter {
            name: "EncounterExtraction".to_owned(),
            position: PLACE,
            yaw_deg: TURN_DEG,
            auto_activate: true,
            top_stages: vec![0],
            stages: vec![survive],
            targets: vec![fodder],
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        assert_eq!(host.spawned.len(), LIVE as usize);
        let first = host.spawned[0].1;
        let expected = turned_point([1.0, 0.0, 0.0], PLACE, TURN_DEG);
        assert!(
            (first[0] - expected[0]).abs() < 1e-3 && (first[1] - expected[1]).abs() < 1e-3,
            "a point turns with the encounter: {first:?} against {expected:?}"
        );
        host.kill(ADD_VITALS);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Running,
            "never all dead"
        );
        let comeback = T0 + 2 + seconds_to_ms(SHORTEST_S);
        run.tick(&mut host, &view(&[PLACE]), &[], comeback - 1);
        assert_eq!(host.spawned.len(), LIVE as usize, "not before its time");
        run.tick(&mut host, &view(&[PLACE]), &[], comeback);
        assert_eq!(host.spawned.len(), 2 * LIVE as usize, "both come back");
        assert_eq!(host.spawned[LIVE as usize].1, first, "at their points");
        let end = T0 + 1 + u64::from(SURVIVE_S) * MS_PER_S;
        host.kill(ADD_VITALS);
        run.tick(&mut host, &view(&[PLACE]), &[], end);
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Done,
            "it ends at its time"
        );
        run.tick(&mut host, &view(&[PLACE]), &[], end + 10 * MS_PER_S);
        assert_eq!(
            host.spawned.len(),
            2 * LIVE as usize,
            "no comeback after the stage"
        );
    }

    /// The extraction's falling debris: the survive stage's hazard spawner keeps three debris
    /// objects at its points; each gives its stun to a player in its 1.2 m sphere once between
    /// 4.0 and 4.6 s, ends it at 4.6 s, goes at 6.5 s and comes back after its respawn time.
    #[test]
    fn a_hazard_spawner_keeps_its_objects_and_stuns_once_while_their_area_is_on() {
        use nw_world::dungeon_scripts::{AreaRefill, EffectArea, HazardObject};
        const LIVE: u32 = 3;
        const SURVIVE_S: u32 = 60;
        const SHORTEST_S: f32 = 0.1;
        const PLAYER: u64 = 7_000;
        const ON_MS: u64 = 4_000;
        const OFF_MS: u64 = 4_600;
        const LIFE_MS: u64 = 6_500;
        let survive = Stage {
            name: "Wave_02 - Survive".to_owned(),
            spawns: vec![Spawn {
                target: 0,
                count: 1,
                locations: vec![PLACE],
                ..Spawn::default()
            }],
            objectives: vec![objective(ObjectiveSpec::InvasionWave, SURVIVE_S, true)],
            ..Stage::default()
        };
        let points = vec![[1.0, 0.0, 0.0], [0.0, 5.0, 0.0], [-5.0, 0.0, 0.0]];
        let spawner = Target {
            name: "slices/catacombs/test_sustainwave_hazard.dynamicslice".to_owned(),
            slices: vec![TargetSlice {
                path: "slices/catacombs/test_sustainwave_hazard.dynamicslice".to_owned(),
                mobs_per_spawn: LIVE,
                refill: Some(AreaRefill {
                    respawn_s: [SHORTEST_S, 7.0],
                    points: points.clone(),
                }),
                hazard: Some(HazardObject {
                    slice: format!("slices/catacombs/test_{HAZARD_OBJECT}.dynamicslice"),
                    life_s: 6.5,
                    areas: vec![EffectArea {
                        effects: vec!["Stunned_Afflict".to_owned()],
                        radius_m: 1.2,
                        on_s: 4.0,
                        off_s: 4.6,
                    }],
                }),
                ..TargetSlice::default()
            }],
        };
        let encounter = Encounter {
            name: "EncounterExtraction".to_owned(),
            position: PLACE,
            auto_activate: true,
            top_stages: vec![0],
            stages: vec![survive],
            targets: vec![spawner],
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        let first_point = turned_point(points[0], PLACE, 0.0);
        host.players = vec![(PLAYER, first_point)];
        let mut now = T0;
        while host.spawned.is_empty() && now < T0 + MS_PER_S {
            run.tick(&mut host, &view(&[PLACE]), &[], now);
            now += 1;
        }
        let spawned_ms = now - 1;
        assert_eq!(host.spawned.len(), LIVE as usize, "three debris at once");
        assert!(
            host.spawned
                .iter()
                .all(|(slice, _)| slice.contains(HAZARD_OBJECT)),
            "{:?}",
            host.spawned
        );
        let at = |offset_ms: u64| spawned_ms + offset_ms;
        run.tick(&mut host, &view(&[PLACE]), &[], at(ON_MS - 1));
        assert!(host.afflicted.is_empty(), "not before the area is on");
        run.tick(&mut host, &view(&[PLACE]), &[], at(ON_MS));
        run.tick(&mut host, &view(&[PLACE]), &[], at(ON_MS + 300));
        assert_eq!(
            host.afflicted,
            [(PLAYER, at(ON_MS))],
            "once, by the one debris over the player"
        );
        run.tick(&mut host, &view(&[PLACE]), &[], at(OFF_MS));
        assert_eq!(
            host.ended,
            [(PLAYER, at(OFF_MS))],
            "the stun ends with the area"
        );
        run.tick(&mut host, &view(&[PLACE]), &[], at(LIFE_MS));
        assert_eq!(
            host.removed.len(),
            LIVE as usize,
            "the debris goes at its end"
        );
        let comeback = at(LIFE_MS) + seconds_to_ms(SHORTEST_S);
        run.tick(&mut host, &view(&[PLACE]), &[], comeback - 1);
        assert_eq!(
            host.spawned.len(),
            LIVE as usize,
            "not before its respawn time"
        );
        run.tick(&mut host, &view(&[PLACE]), &[], comeback);
        assert_eq!(
            host.spawned.len(),
            2 * LIVE as usize,
            "each comes back at its point"
        );
        assert_eq!(host.spawned[LIVE as usize].1, host.spawned[0].1);
    }

    /// A slice whose area spawner names a mixed alias rolls each of its mobs, and each comeback,
    /// over the alias's mobs by their weights (the survive wave's six kinds).
    #[test]
    fn an_area_spawner_rolls_each_mob_over_its_alias_by_weight() {
        use nw_world::dungeon_scripts::AliasMob;
        const LIVE: u32 = 2;
        const GHOUL: &str = "Lost_FeralGhoul_Catacombs";
        const BLOODBEAST_PERCENT: u32 = 80;
        const GHOUL_PERCENT: u32 = 60;
        let alias_mob = |vitals: &str, percent: u32| AliasMob {
            mob: TargetMob {
                vitals_id: vitals.to_owned(),
                category_id: String::new(),
                level: LEVEL,
            },
            percent,
        };
        let mut fodder = target(ADD_VITALS);
        fodder.slices[0].mobs_per_spawn = LIVE;
        fodder.slices[0].refill = Some(nw_world::dungeon_scripts::AreaRefill {
            respawn_s: [2.0, 2.0],
            points: vec![[1.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
        });
        fodder.slices[0].alias_mobs = vec![
            alias_mob(ADD_VITALS, BLOODBEAST_PERCENT),
            alias_mob(GHOUL, GHOUL_PERCENT),
        ];
        let encounter = Encounter {
            name: "EncounterExtraction".to_owned(),
            position: PLACE,
            auto_activate: true,
            top_stages: vec![0],
            stages: vec![Stage {
                spawns: vec![Spawn {
                    target: 0,
                    count: 1,
                    locations: vec![PLACE],
                    ..Spawn::default()
                }],
                objectives: vec![objective(ObjectiveSpec::InvasionWave, 60, true)],
                ..Stage::default()
            }],
            targets: vec![fodder],
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        // The slice roll, then one alias roll per mob: below 80 the first kind, else the second.
        let mut host = Host {
            rolls: [0, BLOODBEAST_PERCENT - 1, BLOODBEAST_PERCENT].into(),
            ..Host::default()
        };
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        let kinds: Vec<&str> = host.spawned.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(kinds, [ADD_VITALS, GHOUL]);
        host.kill(ADD_VITALS);
        host.kill(GHOUL);
        // Two waits, then the comebacks' alias rolls.
        host.rolls = [0, 0, BLOODBEAST_PERCENT + GHOUL_PERCENT - 1, 0].into();
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2 + seconds_to_ms(2.0));
        let comebacks: Vec<&str> = host.spawned[LIVE as usize..]
            .iter()
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(
            comebacks,
            [GHOUL, ADD_VITALS],
            "each comeback rolls its own kind"
        );
    }

    #[test]
    fn the_intro_ends_at_its_time_limit_without_its_event() {
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![boss_encounter()]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        assert_eq!(stage_status(&run, 2), StageStatus::Running);
        let limit = u64::from(INTRO_S) * MS_PER_S;
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2 + limit);
        assert_eq!(stage_status(&run, 2), StageStatus::Done);
        host.phase.insert(1, 3);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 3 + limit);
        assert_eq!(
            stage_status(&run, 3),
            StageStatus::Done,
            "phase 3 ends the fight"
        );
    }

    #[test]
    fn a_wipe_past_the_grace_period_resets_the_encounter() {
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![boss_encounter()]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        assert_eq!(host.spawned.len(), 1);
        run.tick(&mut host, &view(&[]), &[], T0 + 2);
        let grace = u64::from(GRACE_S) * MS_PER_S;
        run.tick(&mut host, &view(&[]), &[], T0 + 2 + grace);
        assert_eq!(host.removed, [1], "the boss goes");
        assert_eq!(run.encounters[0].status, EncounterStatus::Idle);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 3 + grace);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 4 + grace);
        assert_eq!(host.spawned.len(), 2, "it starts again");
    }

    #[test]
    fn a_kill_wave_counts_its_own_mobs_when_its_target_names_other_slices() {
        let wave = Stage {
            name: "Wave_01 - Objective: Kill Wave".to_owned(),
            spawns: vec![Spawn {
                target: ADDS,
                count: 2,
                delay_s: 1,
                locations: vec![[1.0, 2.0, 3.0]],
                ..Spawn::default()
            }],
            objectives: vec![objective(
                ObjectiveSpec::SliceDestroyed {
                    target: Some(BOSS),
                    required: 2,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let cleanup = Stage {
            name: "Wave_03 - Cleanup".to_owned(),
            objectives: vec![objective(ObjectiveSpec::InvasionWave, 3, true)],
            ..Stage::default()
        };
        let encounter = Encounter {
            auto_activate: false,
            top_stages: vec![0, 1],
            stages: vec![wave, cleanup],
            ..boss_encounter()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[[0.0; 3]]), &[], T0);
        assert_eq!(
            run.encounters[0].status,
            EncounterStatus::Idle,
            "no player near"
        );
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        assert!(host.spawned.is_empty(), "the spawn waits for its delay");
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2 + MS_PER_S);
        assert_eq!(host.spawned.len(), 2);
        assert!(host.spawned.iter().all(|(_, at)| *at == [1.0, 2.0, 3.0]));
        host.kill(ADD_VITALS);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 3 + MS_PER_S);
        assert_eq!(stage_status(&run, 0), StageStatus::Done);
        assert_eq!(stage_status(&run, 1), StageStatus::Running);
        let outcomes = run.tick(&mut host, &view(&[PLACE]), &[], T0 + 4 + 4 * MS_PER_S);
        assert!(outcomes.contains(&ScriptOutcome::Completed));
    }

    #[test]
    fn a_failure_objective_fails_its_stage_and_resets_the_encounter() {
        let mut encounter = boss_encounter();
        encounter.stages[0].objectives.push(StageObjective {
            time_limit_s: 0,
            succeed_on_timeout: false,
            failure: true,
            kind: ObjectiveSpec::EncounterEvent {
                event: "Defeat".to_owned(),
            },
        });
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        let mut events = view(&[PLACE]);
        events.events.push((1, "Defeat".to_owned()));
        run.tick(&mut host, &events, &[], T0 + 2);
        assert_eq!(run.encounters[0].status, EncounterStatus::Idle);
        assert_eq!(host.removed, [1]);
    }

    #[test]
    fn end_actions_remove_the_stages_mobs_and_note_entity_events() {
        let mut encounter = boss_encounter();
        encounter.stages[0].end_actions = vec![
            StageAction::KillSpawns,
            StageAction::EntityEvent {
                entity: 7,
                event: EVENT_ACTIVATE,
                recursive: false,
                local_player_only: false,
            },
        ];
        encounter.stages[0].objectives = vec![objective(
            ObjectiveSpec::EncounterEvent {
                event: "Fight_Complete".to_owned(),
            },
            0,
            false,
        )];
        encounter.stages[0].child_stages_optional = true;
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        let mut events = view(&[PLACE]);
        events.events.push((1, "Fight_Complete".to_owned()));
        run.tick(&mut host, &events, &[], T0 + 2);
        assert_eq!(host.removed, [1], "the living boss goes with its stage");
        assert!(
            run.encounters[0]
                .entity_events
                .contains(&(7, EVENT_ACTIVATE, T0 + 2))
        );
    }

    /// The mobs of a breach-like target: one slice per difficulty (`_dt1` twice, `_dt2`), as the
    /// corrupted breaches' aliases list them, and a wave slice with a mob per difficulty.
    fn breach_targets() -> Vec<Target> {
        let tier = |difficulty: u32, vitals: &str| TargetSlice {
            path: format!("slices/characters/corruption_swarmer_dt{difficulty}.dynamicslice"),
            percent: 100,
            difficulty: Some(difficulty),
            mob: Some(TargetMob {
                vitals_id: vitals.to_owned(),
                category_id: String::new(),
                level: LEVEL,
            }),
            ..TargetSlice::default()
        };
        let wave = TargetSlice {
            path: "slices/darkness/events/general/darkness_ai_swarmer_wander_00.dynamicslice"
                .to_owned(),
            mob: Some(TargetMob {
                vitals_id: SWARMER_DT1.to_owned(),
                category_id: String::new(),
                level: LEVEL,
            }),
            difficulty_mobs: [(2, SWARMER_DT2), (3, SWARMER_DT3)]
                .into_iter()
                .map(|(difficulty, vitals)| {
                    (
                        difficulty,
                        TargetMob {
                            vitals_id: vitals.to_owned(),
                            category_id: String::new(),
                            level: LEVEL,
                        },
                    )
                })
                .collect(),
            ..TargetSlice::default()
        };
        vec![
            Target {
                name: "alias_darkness_corruption_swarmer.aliasasset".to_owned(),
                slices: vec![
                    tier(1, SWARMER_DT1),
                    tier(1, SWARMER_DT1),
                    tier(2, SWARMER_DT2),
                ],
            },
            Target {
                name: "darkness_ai_swarmer_wander_00.dynamicslice".to_owned(),
                slices: vec![wave],
            },
        ]
    }

    const SWARMER_DT1: &str = "Corruption_Swarmer_DT1";
    const SWARMER_DT2: &str = "Corruption_Swarmer_DT2";
    const SWARMER_DT3: &str = "Corruption_Swarmer_DT3";
    const GROUP: &str = "coatgen/test/darkness_spawngroup_minor_master_1.dynamicslice#7";
    const DT2_EVENT: &str = "Darkness_Minor_DT2_T1";
    const GLOBAL_EVENT: &str = "Darkness_Any_GlobalSuccess";

    /// A breach of the group at `place` and `difficulty`: its one stage spawns one of each
    /// target and ends when the first target's mob dies.
    fn breach(place: [f32; 3], difficulty: u32) -> Encounter {
        Encounter {
            name: "Darkness_SpawnGroup_Minor_Master".to_owned(),
            position: place,
            auto_activate: true,
            grace_period_s: GRACE_S,
            detection_radius_m: Some(RADIUS_M),
            top_stages: vec![0],
            stages: vec![Stage {
                name: "Wave_01 - Objective".to_owned(),
                spawns: [0, 1]
                    .into_iter()
                    .map(|target| Spawn {
                        target,
                        count: 1,
                        ..Spawn::default()
                    })
                    .collect(),
                objectives: vec![objective(
                    ObjectiveSpec::SliceDestroyed {
                        target: Some(0),
                        required: 1,
                    },
                    0,
                    false,
                )],
                ..Stage::default()
            }],
            targets: breach_targets(),
            spawn_group: Some(GROUP.to_owned()),
            difficulty: Some(difficulty),
            rewards: Some(nw_world::level_script::DifficultyRewards {
                radius_m: 500.0,
                events: vec!["Darkness_Minor_DT1_T1".to_owned(), DT2_EVENT.to_owned()],
                global_event: GLOBAL_EVENT.to_owned(),
            }),
            ..Encounter::default()
        }
    }

    fn open_rules() -> OpenWorldRules {
        OpenWorldRules {
            near_m: 100.0,
            respawn_ms: 60_000,
        }
    }

    #[test]
    fn an_encounter_runs_only_in_its_time_of_day() {
        use nw_world::time_of_day::{TimeOfDayPoints, TimedSlicePath};

        use crate::world::day_night::TimedPlaces;
        use crate::world::day_night::tests::{DAY_MS, NIGHT_MS, NIGHT_ONLY};
        use crate::world::tests::{table, world_with_spawns};
        const HAUNT: &str =
            "slices/worldevents/brightwood/enc_worldevent_brightwood_walshamhaunt_00.dynamicslice";
        let haunt = Encounter {
            slice: HAUNT.to_owned(),
            ..boss_encounter()
        };
        let run =
            DungeonRun::of_open_world(script(vec![boss_encounter(), haunt]), open_rules(), &|_| {
                false
            });
        let mut world = world_with_spawns(Vec::new());
        world.set_table(&table(&["sim-a"])).expect("table");
        world.ids.time_change_time = 0;
        let level = nw_world::WorldFile {
            time_of_day: TimeOfDayPoints {
                slices: vec![TimedSlicePath {
                    slice: HAUNT.to_owned(),
                    window: NIGHT_ONLY,
                }],
                ..TimeOfDayPoints::default()
            },
            ..nw_world::WorldFile::default()
        };
        world.content = std::sync::Arc::new(crate::player_content::PlayerContent {
            time_of_day: TimedPlaces::of(&level),
            ..(*world.content).clone()
        });
        world.server_ms = DAY_MS;
        assert_eq!(
            world.runs_here(&run),
            [true, false],
            "the haunt waits for the night"
        );
        world.server_ms = NIGHT_MS;
        assert_eq!(world.runs_here(&run), [true, true]);
    }

    /// An encounter mob keeps its spawner slice's vitals category as its spawn category, which a
    /// kill task counts (the C10A_Q3 tyrant: category `CK_Spectral_Lost_Fire_Q3_WaveEvent`); a
    /// slice without one gives none.
    #[test]
    fn an_encounter_mob_keeps_its_slice_s_vitals_category() {
        use crate::combat::test_support::{TEST_LEVEL, TEST_MOB};
        use crate::world::tests::world_with_spawns;
        const CATEGORY: &str = "CK_Spectral_Lost_Fire_Q3_WaveEvent";
        let mut world = world_with_spawns(Vec::new());
        let mob = |category: &str| TargetMob {
            vitals_id: TEST_MOB.to_owned(),
            category_id: category.to_owned(),
            level: TEST_LEVEL,
        };
        let mut host = SimHost {
            world: &mut world,
            mutated: false,
        };
        let tyrant = host.spawn(&mob(CATEGORY), PLACE, None).expect("tyrant");
        let plain = host.spawn(&mob(""), PLACE, None).expect("plain");
        assert_eq!(
            world.mobs.category_of(tyrant),
            Some(nw_formats::objectstream::lowercase_crc32(CATEGORY))
        );
        assert_eq!(world.mobs.category_of(plain), None);
    }

    #[test]
    fn a_breach_spawns_its_difficulty_s_slice_and_nested_mob() {
        let mut run =
            DungeonRun::of_open_world(script(vec![breach(PLACE, 2)]), open_rules(), &|_| false);
        let mut host = Host::default();
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        let spawned: Vec<&str> = host.spawned.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            spawned,
            [SWARMER_DT2, SWARMER_DT2],
            "the alias slice, the wave's mob"
        );
        let mut low =
            DungeonRun::of_open_world(script(vec![breach(PLACE, 1)]), open_rules(), &|_| false);
        let mut host = Host::default();
        low.tick(&mut host, &view(&[PLACE]), &[], T0);
        low.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        let spawned: Vec<&str> = host.spawned.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            spawned,
            [SWARMER_DT1, SWARMER_DT1],
            "a wave without a mob of the difficulty spawns its own"
        );
    }

    /// Live 2026-11-24 04:22:32Z: `FM_corruption_breach` waits for the global success of any
    /// breach; the plan named the first breach of the list, which its group had not chosen, and
    /// the bot saw no mob there. The plan names the group's choice.
    #[test]
    fn a_breach_event_step_is_the_fight_at_the_breach_its_group_runs() {
        let other = [PLACE[0] + 150.0, PLACE[1], PLACE[2]];
        let script = script(vec![breach(PLACE, 2), breach(other, 2)]);
        let run = DungeonRun::of_open_world(script.clone(), open_rules(), &|_| false);
        let chosen = if run.chosen(0) { PLACE } else { other };
        let mut world = crate::world::tests::world_with_spawns(Vec::new());
        world.settings.world_events = true;
        world.content = std::sync::Arc::new(crate::player_content::PlayerContent {
            dungeon_scripts: nw_world::dungeon_scripts::DungeonScripts {
                game_modes: [(OPEN_WORLD_LEVEL.to_owned(), script)]
                    .into_iter()
                    .collect(),
            },
            ..crate::player_content::PlayerContent::default()
        });
        world.world_events = Some(run);
        let encounter = world.encounter_firing(GLOBAL_EVENT).expect("a breach");
        assert_eq!(encounter.position, chosen);
        assert_eq!(
            world.encounter_mob_now(usize::from(chosen != PLACE)),
            None,
            "no wave lives yet: the plan takes its first mob at the breach"
        );
    }

    #[test]
    fn one_breach_of_a_group_runs_and_the_group_picks_again_after_its_respawn() {
        let other = [PLACE[0] + 150.0, PLACE[1], PLACE[2]];
        let mut run = DungeonRun::of_open_world(
            script(vec![breach(PLACE, 2), breach(other, 2)]),
            open_rules(),
            &|_| false,
        );
        let chosen: Vec<bool> = (0..2).map(|index| run.chosen(index)).collect();
        assert_eq!(chosen.iter().filter(|chosen| **chosen).count(), 1);
        let (at, away) = if chosen[0] {
            (PLACE, other)
        } else {
            (other, PLACE)
        };
        let mut host = Host::default();
        run.tick(&mut host, &view(&[away]), &[], T0);
        run.tick(&mut host, &view(&[away]), &[], T0 + 1);
        assert!(host.spawned.is_empty(), "the other point waits");
        run.tick(&mut host, &view(&[at]), &[], T0 + 2);
        run.tick(&mut host, &view(&[at]), &[], T0 + 3);
        assert_eq!(host.spawned.len(), 2);
        host.kill(SWARMER_DT2);
        let outcomes = run.tick(&mut host, &view(&[at]), &[], T0 + 4);
        let done = usize::from(!chosen[0]);
        assert!(outcomes.contains(&ScriptOutcome::EncounterDone(done)));
        let (_, radius_m, event) = run.rewards_of(done).expect("rewards");
        assert!((radius_m - 500.0).abs() < f32::EPSILON);
        assert_eq!(event, DT2_EVENT, "its difficulty's event, no global one");
        // The area spawner's respawn time (no rules here: the mob respawn time).
        let back = T0 + 5 + open_rules().respawn_ms;
        run.tick(&mut host, &view(&[at]), &[], T0 + 5);
        run.tick(&mut host, &view(&[at]), &[], back);
        assert_eq!(
            (0..2).filter(|index| run.chosen(*index)).count(),
            1,
            "the group picked its next choice"
        );
    }

    #[test]
    fn a_breach_that_cannot_end_at_its_difficulty_is_never_chosen() {
        // Difficulty 3: the tracked alias has no slice of it, so its objective cannot be met.
        let run =
            DungeonRun::of_open_world(script(vec![breach(PLACE, 3)]), open_rules(), &|_| false);
        assert!(!run.chosen(0));
        let object = Encounter {
            targets: vec![Target {
                name: "alias_darkness_minor_portal_core.aliasasset".to_owned(),
                slices: vec![TargetSlice {
                    path: "slices/darkness/events/general/core/darkness_core_minor_portal_dt3.dynamicslice"
                        .to_owned(),
                    difficulty: Some(3),
                    ..TargetSlice::default()
                }],
            }],
            ..breach(PLACE, 3)
        };
        let is_core = |slice: &str| slice.contains("darkness_core_");
        assert!(
            completable(&object, &is_core),
            "a start object of the difficulty"
        );
        assert!(!completable(&object, &|_| false));
        let plain =
            DungeonRun::of_open_world(script(vec![boss_encounter()]), open_rules(), &|_| false);
        assert!(plain.chosen(0), "an encounter of no group always may run");
    }

    /// A target slice of `family` whose spawner slice spawns `spawned` (the point spawner of a
    /// catacomb alias).
    fn family_slice(vitals: &str, family: &str, spawned: &str) -> TargetSlice {
        TargetSlice {
            path: format!("slices/spawners/{vitals}.dynamicslice"),
            percent: 100,
            mob: Some(TargetMob {
                vitals_id: vitals.to_owned(),
                category_id: String::new(),
                level: LEVEL,
            }),
            spawned_slices: vec![spawned.to_owned()],
            family: family.to_owned(),
            ..TargetSlice::default()
        }
    }

    /// A room of two waves of two families' point spawners: every mob is of one family, and the
    /// second wave's objective, which names the creature slices the spawners spawn (with other
    /// vitals rows), counts a first wave mob that died after it started.
    #[test]
    fn an_encounter_keeps_one_family_and_counts_the_creatures_its_spawners_spawn() {
        const KNIGHT: &str = "slices/characters/evil_knight_spear.dynamicslice";
        const GHOUL: &str = "slices/characters/lost_feral_ghoul.dynamicslice";
        let spawners = Target {
            name: "catacombs_enemies_pointspawners".to_owned(),
            slices: vec![
                family_slice("Lost_FeralGhoul_Catacombs", "Lost", GHOUL),
                family_slice("Evil_Knight_Spear_Catacombs", "Human", KNIGHT),
            ],
        };
        let creatures = Target {
            name: "catacombs_enemies_slices".to_owned(),
            slices: [GHOUL, KNIGHT]
                .iter()
                .map(|path| TargetSlice {
                    path: (*path).to_owned(),
                    percent: 100,
                    ..TargetSlice::default()
                })
                .collect(),
        };
        let wave = |name: &str, count: u32, required: u32| Stage {
            name: name.to_owned(),
            spawns: vec![Spawn {
                target: 0,
                count,
                ..Spawn::default()
            }],
            objectives: vec![objective(
                ObjectiveSpec::SliceDestroyed {
                    target: Some(1),
                    required,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let encounter = Encounter {
            name: "Hallway".to_owned(),
            position: PLACE,
            auto_activate: true,
            top_stages: vec![0, 1],
            stages: vec![wave("Wave 1", 2, 1), wave("Wave 2", 2, 2)],
            targets: vec![spawners, creatures],
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        // The family roll lands on the second family's weight (Human).
        let mut host = Host {
            rolls: [150].into(),
            ..Host::default()
        };
        run.tick(&mut host, &view(&[PLACE]), &[], T0);
        assert_eq!(host.spawned.len(), 2);
        let kill = |host: &mut Host, entity: u64| {
            host.health.insert(entity, 0);
        };
        kill(&mut host, 1);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 1);
        assert_eq!(
            stage_status(&run, 1),
            StageStatus::Running,
            "wave 2 after one death"
        );
        assert_eq!(host.spawned.len(), 4);
        // The first wave's second mob and one of the second wave: two deaths since wave 2 began.
        kill(&mut host, 2);
        kill(&mut host, 3);
        run.tick(&mut host, &view(&[PLACE]), &[], T0 + 2);
        assert_eq!(stage_status(&run, 1), StageStatus::Done);
        assert!(
            host.spawned
                .iter()
                .all(|(vitals, _)| vitals == "Evil_Knight_Spear_Catacombs"),
            "{:?}",
            host.spawned
        );
        assert_eq!(run.encounters[0].family.as_deref(), Some("Human"));
    }

    /// The lightning hazard's slice spawn: its delayed events bring its tornadoes 2, 12 and 22 s
    /// after it, its stage's `KillSpawns` at the 30 s limit leaves them, and its own 33 s end
    /// takes them (`CATACOMBS3` 449.6, 459.6, 469.6 and 480.6 s).
    #[test]
    fn a_slice_spawn_brings_its_timed_mobs_and_its_end_takes_them() {
        const TORNADO: &str = "AngryEarth_Dryad_Siren_Tornado_Catacombs";
        const HAZARD_S: u32 = 30;
        const LIFETIME_S: f32 = 33.0;
        let tornado = |delay_s: f32| nw_world::dungeon_scripts::TimedMob {
            delay_s,
            offset: [1.0, 0.0, 0.0],
            mob: TargetMob {
                vitals_id: TORNADO.to_owned(),
                category_id: String::new(),
                level: LEVEL,
            },
        };
        let hazard = Target {
            name: "dg_enc_catacombs_hazard_lightning".to_owned(),
            slices: vec![TargetSlice {
                path: "slices/catacombs/hazard/dg_enc_catacombs_hazard_lightning.dynamicslice"
                    .to_owned(),
                timed_mobs: vec![tornado(2.0), tornado(12.0), tornado(22.0)],
                lifetime_s: Some(LIFETIME_S),
                ..TargetSlice::default()
            }],
        };
        let stage = Stage {
            name: "Wave_02 - Begin Hazard".to_owned(),
            spawns: vec![Spawn {
                target: 0,
                count: 1,
                ..Spawn::default()
            }],
            objectives: vec![objective(ObjectiveSpec::InvasionWave, HAZARD_S, true)],
            end_actions: vec![StageAction::KillSpawns],
            ..Stage::default()
        };
        let encounter = Encounter {
            name: "EncounterHazard".to_owned(),
            position: PLACE,
            auto_activate: true,
            top_stages: vec![0],
            stages: vec![stage],
            targets: vec![hazard],
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        let at = |seconds: u64| T0 + seconds * MS_PER_S;
        let mut tick = |host: &mut Host, seconds: u64| {
            run.tick(host, &view(&[PLACE]), &[], at(seconds));
        };
        tick(&mut host, 0);
        tick(&mut host, 1);
        assert!(host.spawned.is_empty());
        tick(&mut host, 2);
        assert_eq!(host.spawned.len(), 1);
        assert_eq!(host.spawned[0].1, [PLACE[0] + 1.0, PLACE[1], PLACE[2]]);
        tick(&mut host, 12);
        tick(&mut host, 22);
        assert_eq!(host.spawned.len(), 3);
        tick(&mut host, u64::from(HAZARD_S));
        assert!(
            host.removed.is_empty(),
            "the stage's KillSpawns leaves them"
        );
        tick(&mut host, 32);
        assert!(host.removed.is_empty());
        tick(&mut host, 33);
        assert_eq!(host.removed, [1, 2, 3], "the slice's end takes them");
    }

    /// The retail hallway waves room (`dg_enc_catacombs_hallway_waves` of the tile file): four
    /// waves of 2, 2, 2 and 3 of one family, each 3 s after the kill that ends the one before:
    /// one death since the first wave, then two since each later wave began, as in `CATACOMBS5`
    /// (waves 81.0, 127.5, 143.0, 155.4 s after the deaths at 124.5, 127.9 and 140.0, 148.7 and
    /// 152.3 s) and `CATACOMBS3` (303.8, 311.0, 327.1, 335.6 s); the room's portal came at the
    /// last wave's end (`CATACOMBS3` 342.579 s at 448.00, 448.56).
    #[cfg(feature = "local-data")]
    #[test]
    fn the_hallway_waves_room_spawns_its_retail_waves() {
        use nw_world::catacomb_tiles::CatacombTiles;
        const WAVES: &str =
            "slices/catacombs/gameplay/hallwaygameplay/dg_enc_catacombs_hallway_waves.dynamicslice";
        const TRANSITION_MS: u64 = 3_000;
        let tiles =
            CatacombTiles::load(&nw_content::layout::repo_layout()).expect("catacomb tile file");
        let encounter = tiles
            .by_variation
            .values()
            .flat_map(|tile| &tile.encounters)
            .map(|entry| entry.encounter.clone())
            .find(|encounter| encounter.slice == WAVES)
            .expect("the hallway waves room");
        let families: HashMap<String, String> = encounter
            .targets
            .iter()
            .flat_map(|target| &target.slices)
            .filter_map(|slice| Some((slice.mob.as_ref()?.vitals_id.clone(), slice.family.clone())))
            .collect();
        let at = encounter.position;
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        let mut now = T0;
        let mut waves = Vec::new();
        let mut seen = 0;
        // Each step: kill the mobs it names (oldest first), then wait out the transition.
        for kills in [1_usize, 2, 2, 3] {
            run.tick(&mut host, &view(&[at]), &[], now);
            waves.push(host.spawned.len() - seen);
            seen = host.spawned.len();
            let alive: Vec<u64> = (1..=u64::try_from(seen).expect("count"))
                .filter(|entity| host.health.get(entity).is_some_and(|health| *health > 0))
                .take(kills)
                .collect();
            for entity in alive {
                host.health.insert(entity, 0);
            }
            now += 1;
            run.tick(&mut host, &view(&[at]), &[], now);
            assert_eq!(
                host.spawned.len(),
                seen,
                "no wave before its 3 s transition"
            );
            now += TRANSITION_MS;
        }
        assert_eq!(waves, [2, 2, 2, 3]);
        let kinds: std::collections::BTreeSet<&str> = host
            .spawned
            .iter()
            .filter_map(|(vitals, _)| families.get(vitals).map(String::as_str))
            .collect();
        assert_eq!(kinds.len(), 1, "one family: {kinds:?}");
    }

    /// Ticks `run` every 100 ms from `from_ms` to `to_ms` (both included) with a player at
    /// [`PLACE`].
    fn tick_through(run: &mut DungeonRun, host: &mut Host, from_ms: u64, to_ms: u64) {
        const TICK_MS: usize = 100;
        for now_ms in (from_ms..=to_ms).step_by(TICK_MS) {
            run.tick(host, &view(&[PLACE]), &[], now_ms);
        }
    }

    /// The Reekwater skeleton closet: its randomizer picks one spawner once, the picked crawler
    /// comes back 8 s after each death and keeps the wave open while it does, the wave's 120 s
    /// limit ends the comebacks, and the cleanup stage's deactivator turns the closet off and
    /// takes its living crawler at its 3 s end (`EXP-Lazarus-Instrumentality` 570.8 to 693.6 s).
    #[test]
    fn a_randomizer_keeps_its_pick_brings_it_back_and_a_deactivator_ends_it() {
        use nw_world::dungeon_scripts::{RandomizerChoice, SpawnRandomizer};
        const CRAWLER: &str = "Skeleton_Crawler_DG_Reekwater_00_Spawn";
        const ARCHER: &str = "Skeleton_Archer_DG_Reekwater_00_Spawn";
        const COMEBACK_S: f32 = 8.0;
        const COMEBACK_MS: u64 = 8_000;
        const VOLUME_M: f32 = 5.0;
        const WAVE_S: u32 = 120;
        const WAVE_MS: u64 = 120_000;
        const DEACTIVATOR_LIFE_S: f32 = 3.0;
        const DEACTIVATOR_LIFE_MS: u64 = 3_000;
        const DEATH_MS: u64 = 10_000;
        const TICK_MS: u64 = 100;
        let choice = |vitals: &str, respawn_s: Option<f32>| RandomizerChoice {
            mob: TargetMob {
                vitals_id: vitals.to_owned(),
                category_id: String::new(),
                level: 60,
            },
            offset: [0.0; 3],
            respawn_s,
            stop_radius_m: Some(VOLUME_M),
        };
        let closet = Target {
            name: "closet".to_owned(),
            slices: vec![TargetSlice {
                path: "slices/dungeon/reekwater/dungeon_reekwater_monster_closet_skeleton_randomizer.dynamicslice"
                    .to_owned(),
                randomizer: Some(SpawnRandomizer {
                    picks: [1, 1],
                    choices: vec![choice(CRAWLER, Some(COMEBACK_S)), choice(ARCHER, None)],
                }),
                ..TargetSlice::default()
            }],
        };
        let deactivator = Target {
            name: "deactivator".to_owned(),
            slices: vec![TargetSlice {
                path: "slices/dungeon/reekwater/dungeon_reekwater_deactivate_closet.dynamicslice"
                    .to_owned(),
                detectable_radius_m: Some(1.0),
                lifetime_s: Some(DEACTIVATOR_LIFE_S),
                ..TargetSlice::default()
            }],
        };
        let spawn = |target: usize| Spawn {
            target,
            count: 1,
            locations: vec![PLACE],
            ..Spawn::default()
        };
        let wave = Stage {
            name: "Wave_01".to_owned(),
            spawns: vec![spawn(0)],
            objectives: vec![objective(ObjectiveSpec::InvasionWave, WAVE_S, true)],
            ..Stage::default()
        };
        let cleanup = Stage {
            name: "Cleanup".to_owned(),
            spawns: vec![spawn(1)],
            objectives: vec![objective(ObjectiveSpec::WaitForParent, 0, false)],
            ..Stage::default()
        };
        let encounter = Encounter {
            position: PLACE,
            auto_activate: true,
            top_stages: vec![0, 1],
            stages: vec![wave, cleanup],
            targets: vec![closet, deactivator],
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        // The count roll, then the choice roll: the crawler.
        let mut host = Host {
            rolls: [0, 0].into(),
            ..Host::default()
        };
        tick_through(&mut run, &mut host, T0, T0 + DEATH_MS - TICK_MS);
        assert_eq!(host.spawned.len(), 1, "one spawner picked, one mob");
        assert_eq!(host.spawned[0].0, CRAWLER);
        host.kill(CRAWLER);
        let died_ms = T0 + DEATH_MS;
        tick_through(
            &mut run,
            &mut host,
            died_ms,
            died_ms + COMEBACK_MS - TICK_MS,
        );
        assert_eq!(host.spawned.len(), 1, "no comeback before its 8 s");
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Running,
            "a comeback keeps the wave"
        );
        tick_through(
            &mut run,
            &mut host,
            died_ms + COMEBACK_MS,
            died_ms + COMEBACK_MS,
        );
        assert_eq!(
            host.spawned.len(),
            2,
            "the crawler comes back 8 s after its death"
        );
        assert_eq!(host.spawned[1].0, CRAWLER, "the pick is kept");
        let second = 2;
        tick_through(
            &mut run,
            &mut host,
            died_ms + COMEBACK_MS + TICK_MS,
            T0 + WAVE_MS,
        );
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Done,
            "the wave's limit ends it"
        );
        let cleanup_ms = T0 + WAVE_MS;
        tick_through(
            &mut run,
            &mut host,
            cleanup_ms + TICK_MS,
            cleanup_ms + DEACTIVATOR_LIFE_MS - TICK_MS,
        );
        assert!(
            !host.removed.contains(&second),
            "the crawler stands until the deactivator ends"
        );
        tick_through(
            &mut run,
            &mut host,
            cleanup_ms + DEACTIVATOR_LIFE_MS,
            cleanup_ms + DEACTIVATOR_LIFE_MS + TICK_MS,
        );
        assert!(
            host.removed.contains(&second),
            "the deactivator's end takes the crawler"
        );
        assert_eq!(
            host.spawned.len(),
            2,
            "the closet brings no crawler back once it is off"
        );
    }

    /// The sun disk shield: its timeline plays while a player gathers it and holds its place
    /// between gathers, so gathers of 7.5, 6.4 and 2.4 s (retail: 7.5, 6.4 and 2.6 s, the first
    /// sights 0.2 to 0.3 s after the clip times) spawn two, then two, then two elementals at
    /// their spawners; an elemental's death does not end the shield's stage, the shield's
    /// gather does (`1624_0401_thedemonscauldron` 187.7 to 220.0 s).
    #[test]
    fn a_gathered_start_object_plays_its_timeline_and_holds_it_between_gathers() {
        use nw_world::dungeon_scripts::TimedMob;
        const ELEMENTAL: &str = "Sulfur_Elemental_Entity_Spawn";
        const CLIPS: [(f32, [f32; 3]); 8] = [
            (3.172, [-2.0, 0.0, 0.0]),
            (3.683, [2.0, 0.0, 0.0]),
            (9.570, [-2.0, -2.0, 0.0]),
            (10.108, [2.0, -2.0, 0.0]),
            (15.403, [-2.0, 0.0, 0.0]),
            (15.941, [2.0, 0.0, 0.0]),
            (16.425, [-2.0, -2.0, 0.0]),
            (16.989, [2.0, -2.0, 0.0]),
        ];
        // The retail gathers: (start, end) from the shield's spawn, milliseconds.
        const GATHERS: [(u64, u64); 3] = [(1_000, 8_500), (15_900, 22_300), (30_600, 33_000)];
        const SPAWNED_AFTER: [usize; 3] = [2, 4, 6];
        let shield = Target {
            name: "shield".to_owned(),
            slices: vec![TargetSlice {
                path: format!(
                    "slices/worldevents/brimstonesands/interact/{START_OBJECT}1624_diskatumshield.dynamicslice"
                ),
                gather_mobs: CLIPS
                    .iter()
                    .map(|(delay_s, offset)| TimedMob {
                        delay_s: *delay_s,
                        offset: *offset,
                        mob: TargetMob {
                            vitals_id: ELEMENTAL.to_owned(),
                            category_id: String::new(),
                            level: 60,
                        },
                    })
                    .collect(),
                ..TargetSlice::default()
            }],
        };
        let kill_ai = Stage {
            name: "Wave_02 - Objective: Kill AI".to_owned(),
            spawns: vec![Spawn {
                target: 0,
                count: 1,
                locations: vec![PLACE],
                ..Spawn::default()
            }],
            objectives: vec![objective(
                ObjectiveSpec::SliceDestroyed {
                    target: Some(0),
                    required: 1,
                },
                0,
                false,
            )],
            ..Stage::default()
        };
        let encounter = Encounter {
            position: PLACE,
            auto_activate: true,
            top_stages: vec![0],
            stages: vec![kill_ai],
            targets: vec![shield],
            ..Encounter::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![encounter]));
        let mut host = Host::default();
        tick_through(&mut run, &mut host, T0, T0);
        assert_eq!(host.spawned.len(), 1, "the shield stands");
        let shield_entity = 1;
        let elementals = |host: &Host| {
            host.spawned
                .iter()
                .filter(|(id, _)| id == ELEMENTAL)
                .count()
        };
        let mut idle_from = T0;
        for ((start, end), spawned) in GATHERS.into_iter().zip(SPAWNED_AFTER) {
            tick_through(&mut run, &mut host, idle_from, T0 + start);
            host.gathered.insert(shield_entity);
            tick_through(&mut run, &mut host, T0 + start, T0 + end);
            host.gathered.remove(&shield_entity);
            assert_eq!(elementals(&host), spawned, "the timeline held its place");
            host.kill(ELEMENTAL);
            idle_from = T0 + end;
        }
        let places: Vec<[f32; 3]> = host.spawned[1..].iter().map(|(_, at)| *at).collect();
        let expected: Vec<[f32; 3]> = CLIPS[..6]
            .iter()
            .map(|(_, offset)| turned_point(*offset, PLACE, 0.0))
            .collect();
        assert_eq!(places, expected, "each elemental at its spawner");
        assert_eq!(
            stage_status(&run, 0),
            StageStatus::Running,
            "the elementals' deaths do not end it"
        );
        // The gather's end takes the shield away.
        host.health.remove(&shield_entity);
        tick_through(&mut run, &mut host, idle_from, idle_from + 200);
        assert_eq!(stage_status(&run, 0), StageStatus::Done);
        assert_eq!(elementals(&host), 6, "no timeline without its shield");
    }

    /// The open world trigger chains in the content files: the Reekwater closet's randomizer
    /// (one of three closet spawners, the crawler's 8 s comeback and 5 m volume) and its
    /// deactivator (a 1 m detectable sphere that ends after 3 s); the sun disk shield's eight
    /// timeline spawns; and the start object rows of the lighthouse lure, both powder kegs and
    /// the shield, so their `Interact with IO` and `Kill AI` stages can end.
    #[cfg(feature = "local-data")]
    #[test]
    fn the_trigger_chains_hold_their_retail_sources_in_the_content_files() {
        use nw_content::layout::{ENCOUNTER_OBJECTS, repo_layout};
        use nw_content::town_props::TownProps;
        use nw_world::dungeon_scripts::DungeonScripts;
        const CLOSET: &str = "/dungeon_reekwater_monster_closet_skeleton_randomizer.dynamicslice";
        const DEACTIVATOR: &str = "/dungeon_reekwater_deactivate_closet.dynamicslice";
        const SHIELD: &str = "/io_worldevent_1624_diskatumshield.dynamicslice";
        const CRAWLER: &str = "Skeleton_Crawler_DG_Reekwater_00_Spawn";
        const ELEMENTAL: &str = "Sulfur_Elemental_Entity_Spawn";
        const CLOSET_SPAWNERS: usize = 3;
        const SHIELD_SPAWNS: usize = 8;
        const FIRST_CLIP_S: f32 = 3.172;
        const COMEBACK_S: f32 = 8.0;
        const VOLUME_M: f32 = 5.0;
        const DETECTABLE_M: f32 = 1.0;
        const DEACTIVATOR_LIFE_S: f32 = 3.0;
        const START_OBJECTS: [&str; 4] = [
            "slices/worldevents/reekwater/interact/lighthouselure/io_worldevent_0513_lighthouselure_01.dynamicslice",
            "slices/worldevents/reekwater/interact/powderkega/io_worldevent_0511_powderkega_01.dynamicslice",
            "slices/worldevents/reekwater/interact/powderkegb/io_worldevent_0511_powderkegb_01.dynamicslice",
            "slices/worldevents/brimstonesands/interact/io_worldevent_1624_diskatumshield.dynamicslice",
        ];
        let layout = repo_layout();
        let scripts = DungeonScripts::load(&layout).expect("dungeon scripts file");
        let slices_of = |path: &str| -> Vec<TargetSlice> {
            scripts
                .game_modes
                .values()
                .flat_map(|mode| &mode.encounters)
                .flat_map(|encounter| &encounter.targets)
                .flat_map(|target| &target.slices)
                .filter(|slice| slice.path.ends_with(path))
                .cloned()
                .collect()
        };
        let closets = slices_of(CLOSET);
        assert!(!closets.is_empty());
        for closet in &closets {
            let randomizer = closet.randomizer.as_ref().expect("the closet's randomizer");
            assert_eq!(randomizer.picks, [1, 1]);
            assert_eq!(randomizer.choices.len(), CLOSET_SPAWNERS);
            let crawler = randomizer
                .choices
                .iter()
                .find(|choice| choice.mob.vitals_id == CRAWLER)
                .expect("the crawler spawner");
            assert_eq!(crawler.respawn_s, Some(COMEBACK_S));
            assert_eq!(crawler.stop_radius_m, Some(VOLUME_M));
        }
        let deactivators = slices_of(DEACTIVATOR);
        assert!(!deactivators.is_empty());
        for deactivator in &deactivators {
            assert_eq!(deactivator.detectable_radius_m, Some(DETECTABLE_M));
            assert_eq!(deactivator.lifetime_s, Some(DEACTIVATOR_LIFE_S));
        }
        let shields = slices_of(SHIELD);
        assert!(!shields.is_empty());
        for shield in &shields {
            assert_eq!(shield.gather_mobs.len(), SHIELD_SPAWNS);
            assert!(
                shield
                    .gather_mobs
                    .iter()
                    .all(|timed| timed.mob.vitals_id == ELEMENTAL)
            );
            assert!((shield.gather_mobs[0].delay_s - FIRST_CLIP_S).abs() < 1e-3);
        }
        let objects = EncounterObjects::build(
            &TownProps::load(&layout, ENCOUNTER_OBJECTS).expect("objects table"),
        );
        for slice in START_OBJECTS {
            assert!(objects.is_kind(slice), "{slice} stands as a start object");
        }
    }

    /// The open world script file keeps the spawn counts and comebacks the retail ledgers show:
    /// the festering hive's sustain swarmer is one that comes back 7 s after its death
    /// (`Corrupted-Breaches-Festering-Hive-lvl45` 58.0 -> 65.1 s, `-lvl55` 30.4 -> 37.5 s); the
    /// infested grove's sustain spawner makes three at once (`Corrupted-Breaches-Infested-Grove-
    /// lvl25` 23.7 to 24.1 s); the holdout's swordsman spawner makes three that never come back
    /// (`99a_msq_01b_learnsoulwardens` 246.0, 260.8 and 275.8 s, nine in all); a place marker
    /// (`stag_spawn_loc`) spawns no mob.
    #[cfg(feature = "local-data")]
    #[test]
    fn the_open_world_scripts_keep_the_retail_spawn_counts_and_comebacks() {
        use nw_world::dungeon_scripts::DungeonScripts;
        const OPEN_WORLD: &str = "newworld_vitaeeterna";
        const HIVE_SUSTAIN: &str = "/darkness_ai_minor_hive_sustain_00.dynamicslice";
        const GROVE_SUSTAIN: &str = "/darkness_ai_swarmers_sustain_00.dynamicslice";
        const HOLDOUT_SWORDSMEN: &str = "/holdout_areaspawner_swordsman.dynamicslice";
        const STAG_MARKER: &str = "/stag_spawn_loc.dynamicslice";
        const HIVE_RESPAWN_S: f32 = 7.0;
        const GROVE_LIVE: u32 = 3;
        const HOLDOUT_LIVE: u32 = 3;
        let scripts =
            DungeonScripts::load(&nw_content::layout::repo_layout()).expect("dungeon scripts file");

        let slices_of = |level: Option<&str>, path: &str| -> Vec<TargetSlice> {
            scripts
                .game_modes
                .iter()
                .filter(|(name, _)| level.is_none_or(|level| level == name.as_str()))
                .flat_map(|(_, mode)| &mode.encounters)
                .flat_map(|encounter| &encounter.targets)
                .flat_map(|target| &target.slices)
                .filter(|slice| slice.path.ends_with(path))
                .cloned()
                .collect()
        };
        let hive = slices_of(Some(OPEN_WORLD), HIVE_SUSTAIN);
        assert!(!hive.is_empty());
        for slice in &hive {
            assert_eq!(slice.mobs_per_spawn, 1);
            let refill = slice.refill.as_ref().expect("the hive swarmer comes back");
            assert!((refill.respawn_s[0] - HIVE_RESPAWN_S).abs() < f32::EPSILON);
        }
        let grove = slices_of(Some(OPEN_WORLD), GROVE_SUSTAIN);
        assert!(!grove.is_empty());
        assert!(grove.iter().all(|slice| slice.mobs_per_spawn == GROVE_LIVE));
        let holdout = slices_of(Some(OPEN_WORLD), HOLDOUT_SWORDSMEN);
        assert!(!holdout.is_empty());
        for slice in &holdout {
            assert_eq!(slice.mobs_per_spawn, HOLDOUT_LIVE);
            assert!(slice.refill.is_none(), "no holdout swordsman came back");
        }
        let markers = slices_of(None, STAG_MARKER);
        assert!(!markers.is_empty());
        assert!(markers.iter().all(|slice| slice.mob.is_none()));
    }

    /// The Myrkgard north portal (`enc_darkness_major_portal_sm_01`) and its hazard.
    const PORTAL_SLICE: &str =
        "slices/darkness/events/pois/enc_darkness_major_portal_sm_01.dynamicslice";
    const HAZARD_SLICE: &str =
        "slices/darkness/events/general/hazards/darkness_hazard_corruption_major_l4.dynamicslice";

    /// A Myrkgard event: its stage spawns the hazard and a mob and ends with the mob's death;
    /// difficulty 6 and its one reward event (`Playthrough_Part_18`).
    fn myrkgard_event() -> Encounter {
        Encounter {
            name: "Enc_Darkness_Major_Portal_SM_01_Spawn.slice".to_owned(),
            slice: PORTAL_SLICE.to_owned(),
            position: PLACE,
            auto_activate: true,
            grace_period_s: GRACE_S,
            detection_radius_m: Some(RADIUS_M),
            top_stages: vec![0],
            stages: vec![Stage {
                name: "Wave_00 - Setup".to_owned(),
                spawns: [0, 1]
                    .into_iter()
                    .map(|target| Spawn {
                        target,
                        count: 1,
                        ..Spawn::default()
                    })
                    .collect(),
                objectives: vec![objective(
                    ObjectiveSpec::SliceDestroyed {
                        target: Some(0),
                        required: 1,
                    },
                    0,
                    false,
                )],
                ..Stage::default()
            }],
            targets: vec![
                target(ADD_VITALS),
                Target {
                    name: HAZARD_SLICE.to_owned(),
                    slices: vec![TargetSlice {
                        path: HAZARD_SLICE.to_owned(),
                        difficulty: Some(4),
                        ..TargetSlice::default()
                    }],
                },
            ],
            difficulty: Some(6),
            rewards: Some(nw_world::level_script::DifficultyRewards {
                radius_m: 500.0,
                events: vec!["Event_MyrkgardKeyFragment5".to_owned()],
                global_event: GLOBAL_EVENT.to_owned(),
            }),
            ..Encounter::default()
        }
    }

    /// The quest encounter `Enc_Quest_04A_Side02_ObeliskDefenders_01`: no reward list, its
    /// success event `04A_Side02_02_Encounter_01_Success` for the players in its 30 m volume.
    #[test]
    fn an_encounter_s_success_event_counts_within_its_volume() {
        const SUCCESS: &str = "04A_Side02_02_Encounter_01_Success";
        let quest_encounter = Encounter {
            rewards: Some(nw_world::level_script::DifficultyRewards {
                radius_m: 0.0,
                events: Vec::new(),
                global_event: SUCCESS.to_owned(),
            }),
            detection_radius_m: Some(DEFAULT_SUCCESS_RADIUS_M),
            ..myrkgard_event()
        };
        let plain = Encounter {
            rewards: None,
            ..myrkgard_event()
        };
        let no_volume = Encounter {
            detection_radius_m: None,
            ..quest_encounter.clone()
        };
        let run = DungeonRun::new(Uuid::nil(), script(vec![quest_encounter, plain, no_volume]));
        assert_eq!(
            run.success_event_of(0),
            Some((PLACE, DEFAULT_SUCCESS_RADIUS_M, SUCCESS))
        );
        assert_eq!(run.rewards_of(0), None, "no reward event");
        assert_eq!(run.success_event_of(1), None);
        assert_eq!(
            run.success_event_of(2).map(|(_, radius_m, _)| radius_m),
            Some(DEFAULT_SUCCESS_RADIUS_M)
        );
    }

    /// Retail `Playthrough_Part_18` (the north Myrkgard portal): its success gives its one
    /// event, the key fragment, at difficulty 6, and no global success event.
    #[test]
    fn a_myrkgard_event_gives_its_one_event_at_its_difficulty() {
        let run = DungeonRun::new(Uuid::nil(), script(vec![myrkgard_event()]));
        let (_, _, event) = run.rewards_of(0).expect("reward");
        assert_eq!(event, "Event_MyrkgardKeyFragment5");
    }

    /// A hazard target of several difficulties spawns its highest one not above the
    /// encounter's: the hazard alias (`_l1` to `_l4`) its `_l4` at difficulty 6 and its `_l2`
    /// at 2, a lone `_l4` hazard at 6, nothing of `_l2` to `_l4` at 1.
    #[test]
    fn a_target_spawns_its_highest_difficulty_not_above_the_encounters() {
        let slice = |difficulty: u32| TargetSlice {
            path: format!("darkness_hazard_corruption_major_l{difficulty}.dynamicslice"),
            difficulty: Some(difficulty),
            ..TargetSlice::default()
        };
        let alias = Target {
            name: "alias_darkness_corruption_hazard_major.aliasasset".to_owned(),
            slices: (1..=4).map(slice).collect(),
        };
        let spawned = |target: &Target, difficulty: u32| -> Vec<u32> {
            target
                .slices
                .iter()
                .filter(|slice| of_difficulty(target, slice, Some(difficulty)))
                .filter_map(|slice| slice.difficulty)
                .collect()
        };
        assert_eq!(spawned(&alias, 6), [4]);
        assert_eq!(spawned(&alias, 2), [2]);
        let lone = Target {
            name: HAZARD_SLICE.to_owned(),
            slices: vec![slice(4)],
        };
        assert_eq!(spawned(&lone, 6), [4]);
        let high = Target {
            name: "high".to_owned(),
            slices: (2..=4).map(slice).collect(),
        };
        assert!(spawned(&high, 1).is_empty());
    }

    /// Retail `Playthrough_Part_18`: the north portal's `ApplyCurseL4` stood while the portal
    /// ran and went 10.2 s after its success. The event's own areas open as it is shown, its
    /// hazard's at the stage's spawn; a reset takes the spawn's away and keeps its own; both go
    /// 10.2 s after the success.
    #[test]
    fn an_events_areas_stand_until_shortly_after_its_success() {
        let mut host = Host {
            area_slices: vec![PORTAL_SLICE.to_owned(), HAZARD_SLICE.to_owned()],
            ..Host::default()
        };
        let mut run = DungeonRun::new(Uuid::nil(), script(vec![myrkgard_event()]));
        let here = view(&[PLACE]);
        run.tick(&mut host, &here, &[], T0);
        assert_eq!(host.open_area_slices(), [PORTAL_SLICE, HAZARD_SLICE]);
        let wiped = ScriptView {
            living: Vec::new(),
            ..here.clone()
        };
        let grace_ms = u64::from(GRACE_S) * MS_PER_S;
        run.tick(&mut host, &wiped, &[], T0 + 1);
        run.tick(&mut host, &wiped, &[], T0 + 1 + grace_ms);
        assert_eq!(
            host.open_area_slices(),
            [PORTAL_SLICE],
            "the reset keeps the event's own"
        );
        let start = T0 + 2 + grace_ms;
        run.tick(&mut host, &here, &[], start);
        assert_eq!(host.open_area_slices(), [PORTAL_SLICE, HAZARD_SLICE]);
        host.kill(ADD_VITALS);
        let outcomes = run.tick(&mut host, &here, &[], start + 1);
        assert!(outcomes.contains(&ScriptOutcome::EncounterDone(0)));
        run.tick(&mut host, &here, &[], start + AREAS_AFTER_SUCCESS_MS);
        assert_eq!(host.open_area_slices().len(), 2, "they stand 10.2 s");
        run.tick(&mut host, &here, &[], start + 1 + AREAS_AFTER_SUCCESS_MS);
        assert!(host.open_area_slices().is_empty());
    }
}
