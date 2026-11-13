// SPDX-License-Identifier: AGPL-3.0-or-later
//! The encounters of the instance an instance sim runs (dungeon2 study, run 15): the stages of
//! each encounter of its game mode's level script ([`nw_world::dungeon_scripts`]), their spawn
//! timelines, objectives and actions, the encounter's reset at a wipe, and its end: the loot
//! of the loot containers it opens, and the instance's completion after the last one.
//!
//! The level script's own layout is data (encounter slices, `EncounterManagerComponent`,
//! `EncounterComponent`); how the server runs it is not in the client. The rules here follow
//! the class names, the fields and their values (best guesses, dungeon2 REPORT):
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
//! - its success fires its difficulty's reward event ([`Encounter::rewards`]) and the global
//!   success event for each player within the rewards' radius (Observed: each won retail breach
//!   gave the own player `Darkness_<Minor|Major>_DT<n>_T<k>` at the manager's succeeded status,
//!   16 to 50 ms after it; the radius rule is Inferred from the slice's 500 m field).

use nw_bus::Point;
use nw_sim_core::Position;
use nw_world::dungeon_scripts::{
    Encounter, LevelScript, OPEN_WORLD_LEVEL, ObjectiveSpec, Spawn, StageAction, Target, TargetMob,
    TargetSlice,
};
use nw_world::records::PatrolPath;
use uuid::Uuid;

use super::encounter_roots::EncounterObjects;
use super::loot_bag::LimitsSent;
use super::{Body, Player, SimWorld, Vitals};
use crate::mob_ai::script_mob_ai;
use crate::objectives::Event;

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
/// encounter slices' detection volumes (best guess).
const DEFAULT_VOLUME_RADIUS_M: f32 = 30.0;
/// How long a dead encounter mob's body stays before it goes (best guess: the retail bodies of
/// the tutorial stayed 5.1 to 7.1 s after their death, encounter note).
const CORPSE_MS: u64 = 7_000;
/// The blackboard key of a boss's phase (behavior-tree study, engine key table).
const BOSS_PHASE_KEY: &str = "Boss_Phase";
/// Spawns of one GM spawn call.
const ONE: u32 = 1;

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
    /// The health of mob `entity` (a standing start object has [`OBJECT_HEALTH`]); `None` when
    /// it is gone.
    fn health(&self, entity: u64) -> Option<u32>;
    /// The boss phase of mob `entity`.
    fn boss_phase(&self, entity: u64) -> Option<u32>;
    /// A random number below `below`.
    fn pick(&mut self, below: u32) -> u32;
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

/// The run of one encounter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EncounterRun {
    status: EncounterStatus,
    /// The top stage running.
    top: usize,
    stages: Vec<StageRun>,
    mobs: Vec<EncounterMob>,
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

    /// The rewards of encounter `index` at its difficulty: its place, the radius, and its
    /// difficulty's game event and the global success event (those it names).
    pub(super) fn rewards_of(&self, index: usize) -> Option<([f32; 3], f32, Vec<&str>)> {
        let encounter = self.script.encounters.get(index)?;
        let rewards = encounter.rewards.as_ref()?;
        let difficulty = usize::try_from(encounter.difficulty?).ok()?;
        let events = difficulty
            .checked_sub(1)
            .and_then(|at| rewards.events.get(at))
            .into_iter()
            .chain(std::iter::once(&rewards.global_event))
            .map(String::as_str)
            .filter(|event| !event.is_empty())
            .collect();
        Some((encounter.position, rewards.radius_m, events))
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

    /// Whether a living player is within encounter `index`'s volume.
    pub(super) fn engaged(&self, index: usize) -> bool {
        self.encounters.get(index).is_some_and(|run| run.engaged)
    }

    /// The entity events its stages sent since the last call: (encounter, entity, event).
    pub(super) fn take_entity_events(&mut self) -> Vec<(usize, u64, u32)> {
        std::mem::take(&mut self.sent_events)
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

    /// The live mobs of every encounter.
    pub(super) fn mobs(&self) -> impl Iterator<Item = u64> + '_ {
        self.encounters
            .iter()
            .flat_map(|run| &run.mobs)
            .filter(|mob| !mob.gone)
            .map(|mob| mob.entity)
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
            if !owned.get(index).copied().unwrap_or(true) || !chosen {
                if run.status != EncounterStatus::Idle {
                    reset(run, host);
                }
                continue;
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
                    continue;
                }
            }
            let before = run.entity_events.len();
            tick_encounter(encounter, run, host, view, now_ms);
            for (entity, event, _) in run.entity_events.get(before..).unwrap_or_default() {
                self.sent_events.push((index, *entity, *event));
            }
            if run.status == EncounterStatus::Done && !run.reported {
                run.reported = true;
                outcomes.push(ScriptOutcome::EncounterDone(index));
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

    /// The loot containers whose nearest encounter is encounter `index` (best guess: an
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
fn detects(encounter: &Encounter, at: Position, radius_m: f32) -> bool {
    match &encounter.detector {
        Some(detector) => detector.contains([at.x, at.y, at.z]),
        None => at.horizontal_distance(position(encounter.position)) <= radius_m,
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
                reset(run, host);
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
                reset(run, host);
            }
            !left
        }
    }
}

/// Whether an open world encounter starts by itself when a player comes near: its manager
/// activates it, or its spawner spawns when it is enabled (or names no rules).
fn starts_when_enabled(encounter: &Encounter) -> bool {
    encounter.auto_activate
        || encounter
            .spawner
            .is_none_or(|spawner| spawner.spawn_on_enable)
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
        reset(run, host);
        return;
    }
    run_tops(encounter, run, host, now_ms);
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
                reset(run, host);
                return;
            }
            StageStatus::Pending | StageStatus::Running => return,
        }
    }
    run.status = EncounterStatus::Done;
}

/// The encounter starts again: its mobs go, its stages wait, it is idle.
fn reset(run: &mut EncounterRun, host: &mut impl ScriptHost) {
    for mob in run.mobs.iter_mut().filter(|mob| !mob.gone) {
        host.remove(mob.entity);
        mob.gone = true;
    }
    let stages = run.stages.len();
    *run = EncounterRun {
        stages: vec![StageRun::default(); stages],
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
    run_actions(encounter, run, host, &def.end_actions, stage, now_ms);
}

/// Makes the spawns of `stage` that are due.
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
            spawn_one(encounter, run, host, stage, spawn);
        }
    }
}

/// Spawns `spawn.count` mobs of its target: each picks a slice of the target by its weight
/// (an alias) and takes the next location (the encounter's place without one).
fn spawn_one(
    encounter: &Encounter,
    run: &mut EncounterRun,
    host: &mut impl ScriptHost,
    stage: usize,
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
    let per_spawn = |slice: usize| target.slices[slice].mobs_per_spawn.max(ONE);
    let total: u32 = mobs.iter().map(|(_, _, weight)| weight).sum();
    if mobs.is_empty() {
        // A target of no mob: its slices that are start objects stand at the locations.
        for number in 0..spawn.count {
            for (slice, target_slice) in target
                .slices
                .iter()
                .enumerate()
                .filter(|(_, slice)| of_difficulty(target, slice, difficulty))
            {
                match host.spawn_object(&target_slice.path, at_of(number), yaw_of(number)) {
                    Some(entity) => run.mobs.push(EncounterMob {
                        entity,
                        target: spawn.target,
                        slice,
                        stage,
                        died_ms: None,
                        gone: false,
                    }),
                    None => {
                        tracing::debug!(slice = %target_slice.path, "encounter spawn of no mob")
                    }
                }
            }
        }
        return;
    }
    for number in 0..spawn.count {
        let mut roll = host.pick(total);
        let Some(&(slice, mob, _)) = mobs.iter().find(|(_, _, weight)| {
            let hit = roll < *weight;
            roll = roll.saturating_sub(*weight);
            hit
        }) else {
            continue;
        };
        let at = at_of(number);
        for _ in 0..per_spawn(slice) {
            match host.spawn(mob, at, target.slices[slice].waypoints.as_ref()) {
                Some(entity) => run.mobs.push(EncounterMob {
                    entity,
                    target: spawn.target,
                    slice,
                    stage,
                    died_ms: None,
                    gone: false,
                }),
                None => tracing::warn!(
                    vitals = %mob.vitals_id,
                    level = mob.level,
                    "an encounter mob cannot spawn"
                ),
            }
        }
    }
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
                .any(|slice| mob_at(slice, difficulty).is_some() || is_object(&slice.path))
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
/// difficulty, or the encounter has none, or the slice is of it.
fn of_difficulty(target: &Target, slice: &TargetSlice, difficulty: Option<u32>) -> bool {
    let by_difficulty = target.slices.iter().any(|slice| slice.difficulty.is_some());
    match difficulty {
        Some(difficulty) if by_difficulty => slice.difficulty == Some(difficulty),
        _ => true,
    }
}

/// The mob `slice` spawns at `difficulty`: its nested slices' mob of that difficulty, else its
/// own.
fn mob_at(slice: &TargetSlice, difficulty: Option<u32>) -> Option<&TargetMob> {
    difficulty
        .and_then(|difficulty| slice.difficulty_mobs.get(&difficulty))
        .or(slice.mob.as_ref())
}

/// Whether encounter mob `mob` is one of target `target`: its slice or its vitals row is one of
/// the target's.
fn is_of(encounter: &Encounter, mob: &EncounterMob, target: usize) -> bool {
    let Some(own) = encounter
        .targets
        .get(mob.target)
        .and_then(|own| own.slices.get(mob.slice))
    else {
        return false;
    };
    encounter.targets.get(target).is_some_and(|wanted| {
        wanted.slices.iter().any(|slice| {
            slice.path == own.path
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

/// The mobs an objective of stage `stage` tracks: those of `target` (best guess: when no mob
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

fn objective_met(
    encounter: &Encounter,
    run: &EncounterRun,
    host: &impl ScriptHost,
    stage: usize,
    kind: &ObjectiveSpec,
    started_ms: u64,
) -> bool {
    match kind {
        ObjectiveSpec::SliceDestroyed { target, required } => {
            let deaths = tracked(encounter, run, stage, *target)
                .into_iter()
                .filter(|mob| mob.died_ms.is_some_and(|died| died >= started_ms))
                .count();
            u32::try_from(deaths).unwrap_or(u32::MAX) >= (*required).max(ONE)
        }
        ObjectiveSpec::BossPhase { target, phase } => {
            let candidates: Vec<&EncounterMob> = match target {
                Some(target) => tracked(encounter, run, stage, Some(*target)),
                None => run.mobs.iter().collect(),
            };
            // Best guess: the boss's death also ends its phases.
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
            } else {
                // Best guess: the entity is a timeline that the encounter's trigger area starts
                // when a player comes in.
                sent(EVENT_TRIGGER) || run.engaged
            }
        }
        ObjectiveSpec::WaitForParent => false,
        ObjectiveSpec::InvasionWave => {
            let spawned_all = run.stages[stage].due.is_empty();
            let own: Vec<&EncounterMob> =
                run.mobs.iter().filter(|mob| mob.stage == stage).collect();
            spawned_all && !own.is_empty() && own.iter().all(|mob| mob.died_ms.is_some())
        }
    }
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
                    if !mob.gone && is_under(encounter, mob.stage, stage) {
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
        match self.world.spawn_gm(
            &mob.vitals_id,
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

    fn spawn_object(&mut self, slice: &str, at: [f32; 3], yaw_deg: f32) -> Option<u64> {
        self.world.encounter_objects.spawn(slice, at, yaw_deg)
    }

    fn remove(&mut self, entity: u64) {
        if EncounterObjects::is_object(entity) {
            self.world.encounter_objects.remove(entity);
        } else {
            self.world.remove_gm(&[entity]);
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

    fn pick(&mut self, below: u32) -> u32 {
        self.world.dice.below(below.max(ONE))
    }
}

impl SimWorld {
    /// The encounters part of the tick at `now_ms`: the script of the running instance starts
    /// with it and ends with it; its encounters run; an ended one opens its loot containers,
    /// and the last one completes the instance.
    pub(super) fn tick_dungeon_script(&mut self, now_ms: u64) {
        let events = self.mobs.take_encounter_events();
        if !self.instance_sim {
            self.tick_world_events(events, now_ms);
            return;
        }
        let running = self
            .instance_run
            .as_ref()
            .map(|run| (run.id(), run.game_mode().to_owned()));
        let current = self.dungeon_run.as_ref().map(|run| run.instance);
        if current != running.as_ref().map(|(instance, _)| *instance) {
            if let Some(old) = self.dungeon_run.take() {
                let mobs: Vec<u64> = old.mobs().collect();
                self.remove_gm(&mobs);
            }
            let salt = running
                .as_ref()
                .map_or(0, |(instance, _)| instance.as_u64_pair().0);
            self.start_gates(running.as_ref().map(|(_, mode)| mode.as_str()), salt);
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
                Some(DungeonRun::new(instance, script))
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
        let host = &mut SimHost {
            world: self,
            mutated: true,
        };
        let outcomes = run.tick(host, &view, &[], now_ms);
        let owned = vec![true; run.script.encounters.len()];
        self.show_encounters(&mut run, &owned, now_ms);
        self.tick_gates(&run, now_ms);
        for outcome in outcomes {
            match outcome {
                ScriptOutcome::EncounterDone(index) => {
                    self.catacomb_encounter_done(index);
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
                ScriptOutcome::Completed => {
                    tracing::info!(instance = %run.instance, "every encounter done");
                    self.complete_mutated_instance(now_ms);
                    self.game_mode_completed(now_ms);
                }
            }
        }
        self.dungeon_run = Some(run);
    }

    /// The places of the players, for a script's view.
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
                if let Some((at, radius_m, events)) = run.rewards_of(index) {
                    let events: Vec<String> = events.into_iter().map(str::to_owned).collect();
                    self.reward_encounter(position(at), radius_m, &events, now_ms);
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

    /// Each player within `radius_m` of `at` gets the game events `events` (a won breach's
    /// rewards, see the module text).
    fn reward_encounter(&mut self, at: Position, radius_m: f32, events: &[String], now_ms: u64) {
        let numbers: Vec<u64> = {
            let mut query = self.world.query::<(&Player, &Body)>();
            query
                .iter(&self.world)
                .filter(|(_, body)| body.motion.position().horizontal_distance(at) <= radius_m)
                .map(|(player, _)| player.number)
                .collect()
        };
        for number in numbers {
            for event in events {
                self.fire_game_event(number, event, now_ms);
            }
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

    use nw_world::dungeon_scripts::{Stage, StageObjective, Target, TargetSlice};

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
    const ADD_VITALS: &str = "Risen_DG_Windsward_00";
    const LEVEL: u32 = 25;
    const T0: u64 = 1_000;

    /// A test host: spawned mobs by entity with their health and phase.
    #[derive(Default)]
    struct Host {
        next: u64,
        health: HashMap<u64, u32>,
        phase: HashMap<u64, u32>,
        spawned: Vec<(String, [f32; 3])>,
        /// The yaw of each start object spawned, in order.
        object_yaws: Vec<f32>,
        removed: Vec<u64>,
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
            slice.contains(START_OBJECT).then(|| {
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
        fn health(&self, entity: u64) -> Option<u32> {
            self.health.get(&entity).copied()
        }
        fn boss_phase(&self, entity: u64) -> Option<u32> {
            self.phase.get(&entity).copied()
        }
        fn pick(&mut self, _below: u32) -> u32 {
            0
        }
    }

    impl Host {
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
        let (_, radius_m, events) = run.rewards_of(done).expect("rewards");
        assert!((radius_m - 500.0).abs() < f32::EPSILON);
        assert_eq!(
            events,
            [DT2_EVENT, GLOBAL_EVENT],
            "its difficulty's event, then any's"
        );
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
}
