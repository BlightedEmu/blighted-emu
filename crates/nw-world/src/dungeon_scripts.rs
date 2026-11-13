// SPDX-License-Identifier: AGPL-3.0-or-later
//! The dungeon script file (`dungeon-scripts-<build>.json`, written by `content-tool
//! dungeon-scripts`, read by the instance sims): per instance game mode, its level's encounters (from the
//! encounter slices that the game mode's level script slice spawns, [`crate::level_script`]) in
//! world coordinates, with every spawn and tracked target resolved to the slices it may spawn
//! and the mob each one is, and its loot containers.
//!
//! An encounter is a tree of stages. Its manager's stages run in order; a stage runs its spawn
//! timeline, waits for its objectives and its child stages (in order), then runs its end
//! actions. Indexes in the file point into the encounter's own `stages` and `targets`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use nw_formats::objectstream::lowercase_crc32;

use crate::collision::{Affine, Geometry, shape_contains};
use crate::level_script::{DifficultyRewards, SpawnerRules};
use crate::records::PatrolPath;
use crate::slice_data::GUID_LEN;

/// The level folder of the open world. Its encounters are placed by capitals, not by a game
/// mode's level script; the file holds them under the level's name, beside the game modes.
pub const OPEN_WORLD_LEVEL: &str = "newworld_vitaeeterna";
/// The file name prefix; the client build and [`DUNGEON_SCRIPTS_EXTENSION`] follow.
pub const DUNGEON_SCRIPTS_PREFIX: &str = "dungeon-scripts-";
/// The file extension.
pub const DUNGEON_SCRIPTS_EXTENSION: &str = ".json";
/// The level script entity whose trigger area starts an expedition's clock (one of
/// [`LevelScript::trigger_areas`]).
pub const START_AREA_ENTITY: &str = "AreaMutatorStart";

/// The mob a spawned slice is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetMob {
    /// The vitals row id.
    pub vitals_id: String,
    /// The vitals category row id.
    pub category_id: String,
    /// The creature level (the spawner's, else the vitals row's).
    pub level: u32,
}

/// A mob that a slice's own delayed event spawns: the event triggers a spawner of the slice
/// that waits for a trigger ([`crate::level_script::ScriptEntity::waits_for_trigger`]).
/// Observed: the lightning hazard's `Delay` entity triggers `Single_Tornado` 2, 12 and 22 s
/// after the slice starts, and retail showed its three tornadoes
/// (`AngryEarth_Dryad_Siren_Tornado_Catacombs`) 10 s apart (`CATACOMBS3` 449.6, 459.6 and
/// 469.6 s).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimedMob {
    /// Seconds after the slice spawned.
    pub delay_s: f32,
    /// The spawner's place from the slice's origin before the spawn's turn: east, north, up
    /// (metres).
    pub offset: [f32; 3],
    /// The mob.
    pub mob: TargetMob,
}

/// One slice a spawn or a tracked target may be.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TargetSlice {
    /// The pak path of the slice.
    pub path: String,
    /// Its asset GUID as stored.
    pub guid: [u8; GUID_LEN],
    /// Its weight in its alias (`Percent`); 0 for a slice named directly.
    pub percent: u32,
    /// The mob its spawner slice names; `None` for a creature slice (the sim looks its GUID up
    /// in the mob prefab table) or a slice that is no mob.
    pub mob: Option<TargetMob>,
    /// How many mobs one spawn of the slice makes: 1 for a spawner slice; the spawns of a
    /// nested encounter slice (the storm priest's minion portal spawns two swarmers).
    #[serde(default = "one_mob")]
    pub mobs_per_spawn: u32,
    /// The waypoints of its spawner slice, from the spawn's place (the storm priest's
    /// `wp-nopath` with the task `POI_Ritual_Channeling_LOOP`); `None` for a slice without one.
    #[serde(default)]
    pub waypoints: Option<PatrolPath>,
    /// The difficulty the slice is for, when its alias holds one slice per difficulty (its file
    /// name ends in `_dt<n>` or `_l<n>`: the corrupted breaches' aliases list `_dt1`, `_dt1`,
    /// `_dt2` to `_dt6`); `None` for a slice of any difficulty.
    #[serde(default)]
    pub difficulty: Option<u32>,
    /// The mob of each difficulty, when the slice spawns nested slices of one difficulty each
    /// (a corrupted breach's wave slice `darkness_ai_minor_portal_wave_standardwave_01_a`);
    /// empty for a slice of one mob.
    #[serde(default)]
    pub difficulty_mobs: BTreeMap<u32, TargetMob>,
    /// The refill rule of a slice whose spawner is an area spawner
    /// ([`crate::level_script::AREA_SPAWNER_TYPE`]); `None` for another slice.
    #[serde(default)]
    pub refill: Option<AreaRefill>,
    /// The mobs of the alias its spawner names, each with its weight, when the alias holds more
    /// than one mob and no slice of one difficulty: each mob of a spawn is one of them, rolled
    /// by weight, and [`Self::mob`] is the first. Observed: the extraction's survive stage
    /// (`catacombs_extraction_sustainwave`, alias `catacombs_extraction_sustainwave_fodder`:
    /// bloodbeast, ghoul, meatbag, soul hunter and nurse 80 each, cryptkeeper 60) spawned all
    /// six kinds (`CATACOMBS5` 1593.9 to 1617.7 s); Inferred: one roll per mob and per
    /// comeback. Empty for a slice of one mob.
    #[serde(default)]
    pub alias_mobs: Vec<AliasMob>,
    /// The slices its spawner spawns: the `m_sliceAsset` of its first spawner entity that names
    /// one, else the entries of that spawner's `m_aliasAsset`; empty for a slice without a
    /// spawner. An objective that tracks a creature slice counts the mobs of the
    /// spawner slices that spawn it. Observed: the hallway waves' `Sub-Wave_03` (two deaths of
    /// `catacombs_enemies_slices`, spawned through `catacombs_enemies_pointspawners`) counted
    /// a first wave knight that died after it started: the third wave came 3.0 s after the
    /// second death since then (127.9 and 140.0 s) while a second wave knight lived to 148.7 s
    /// (`CATACOMBS5`).
    ///
    /// A spawner settings component ([`crate::level_script::SPAWNER_SETTINGS_TYPE`]) is no
    /// spawner, even when it names a slice: the mutated nurse's `AI Functionality` entity holds
    /// one that names `lost_mutated_nurse_minion` (the AI's `m_minionSpawnerEntity`), so the
    /// nurse's first spawner is its `InstancedLoot` entity's, as for the other creature slices.
    /// Observed: no nurse minion in the retail catacomb ledgers (12 nurses in `CATACOMBS3`,
    /// `CATACOMBS5` and `CATACOMBS6`, no card of the minion's asset `631c64ab...`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spawned_slices: Vec<String>,
    /// The `Family` of its mob's vitals row (`VitalsBaseData`); empty when it has no mob or the
    /// row names none. An encounter keeps one family ([`Encounter`] runs in the sim).
    /// Observed: each catacomb room's mobs were of one family, though the rooms' aliases
    /// (`catacombs_enemies_slices`, 74 slices) hold six (Human, Lost, Corrupted, Wildlife,
    /// AngryEarth, AncientGuardian): about 100 rooms of the 10 catacomb ledgers, for example
    /// the hallway waves rooms `CATACOMBS3` 303.8 to 348.6 s (nine evil knights) and 52.0 to
    /// 110.5 s (AngryEarth only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family: String,
    /// The mobs its own delayed events spawn ([`TimedMob`]); empty for most slices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timed_mobs: Vec<TimedMob>,
    /// Seconds after its spawn when its own delayed event removes it and what it spawned (a
    /// `DestroyGDEComponent`, [`crate::level_script::DESTROY_GDE_TYPE`]); `None` for a slice that
    /// stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifetime_s: Option<f32>,
    /// The hazard object its area spawner keeps standing ([`HazardObject`]): it makes
    /// [`Self::mobs_per_spawn`] of them at the points of [`Self::refill`] and a new one there
    /// after each ends; `None` for another slice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hazard: Option<HazardObject>,
    /// The spawners its prefab spawn randomizer picks from ([`SpawnRandomizer`]); `None` for
    /// another slice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub randomizer: Option<SpawnRandomizer>,
    /// The radius of its detectable object's sphere, metres
    /// ([`crate::level_script::DETECTABLE_OBJECT_TYPE`]): a picked spawner of its encounter
    /// whose detection volume it reaches stops ([`SpawnRandomizer`]); `None` for a slice that
    /// is no detectable object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detectable_radius_m: Option<f32>,
    /// The mobs its timeline spawns while a player gathers it ([`TimedMob`], `delay_s` = the
    /// time on the timeline): the gatherable's gathering start event triggers the timeline
    /// and its gathering end event untriggers it. Observed: the sun disk shield
    /// (`io_worldevent_1624_diskatumshield`, gatherable `1624_0401_DiskAtumStart`) spawned its
    /// sulfur elementals at `Spawn1`, `Spawn2`, `Spawn3`, `Spawn4`, `Spawn1`, `Spawn2` 3.4,
    /// 3.9, 2.4, 2.9, 1.8 and 2.3 s into a player's gathers of 187.7 to 195.2, 202.6 to 209.0
    /// and 217.4 to 220.0 s (`1624_0401_thedemonscauldron` 191.1 to 219.7 s): the timeline
    /// plays while the player gathers and holds its place between gathers (its clips start
    /// 3.17, 3.68, 9.57, 10.11, 15.40 and 15.94 s on it). Empty for another slice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gather_mobs: Vec<TimedMob>,
}

/// The spawners a prefab spawn randomizer (`Javelin::PrefabSpawnRandomizer`,
/// [`crate::level_script::PREFAB_SPAWN_RANDOMIZER_TYPE`]) of a slice picks when the slice
/// starts: a count in [`Self::picks`] of different ones of [`Self::choices`]. Each picked one
/// keeps its own mob and brings it back after each death, while the stage that spawned the slice
/// runs. Observed: the Reekwater skeleton closet (`dungeon_reekwater_monster_closet_skeleton_
/// randomizer`, counts 1 and 1; three spawners: sword, crawler, archer) spawned only crawlers
/// (`Skeleton_Crawler_DG_Reekwater_00_Spawn`) one at a time, each 8.0 s after the previous one's
/// death, 570.8 to 690.7 s (`EXP-Lazarus-Instrumentality`, seven crawlers): the pick is made once,
/// and the picked spawner's own point spawner (`m_respawnAfterDeath`, `m_minRespawnCooldown`
/// 8 s) brings its mob back. Inferred: a pick of each count and spawner is equally likely.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpawnRandomizer {
    /// The fewest and the most spawners it picks (`m_minSpawnCount`, `m_maxSpawnCount`).
    pub picks: [u32; 2],
    /// The spawners it picks from.
    pub choices: Vec<RandomizerChoice>,
}

/// One spawner a [`SpawnRandomizer`] picks from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RandomizerChoice {
    /// Its mob.
    pub mob: TargetMob,
    /// Its place from the slice's origin before the spawn's turn: east, north, up (metres).
    pub offset: [f32; 3],
    /// Seconds from its mob's death to the mob's comeback (its point spawner's
    /// `m_minRespawnCooldown`, with `m_respawnAfterDeath`); `None` for no comeback.
    pub respawn_s: Option<f32>,
    /// The radius of its detection volume, metres: a detectable object of the encounter that
    /// reaches it turns it off (its enter event `Deactivate`s it), no comeback follows, and its
    /// mob goes when that object ends. Observed: the closet spawners' 5 m volume (filter
    /// `DungeonScriptedDetectionFilter`) and the cleanup stage's `dungeon_reekwater_deactivate_
    /// closet` (a 1 m detectable sphere that removes itself 3 s after it spawns); the closet's
    /// last crawler, alive, went 2.9 s after the closet's stage ended (`EXP-Lazarus-
    /// Instrumentality` 690.7 to 693.6 s). Inferred: its mob goes with the detectable object's
    /// end. `None` for a spawner without a volume.
    pub stop_radius_m: Option<f32>,
}

/// An object with a body and no mob that an area spawner keeps standing, which ends by its own
/// timer and hurts the players in its effect areas. Observed: the extraction's survive stage
/// (`catacombs_extraction_sustainwave_hazard`: live count 3, respawn 0.1 to 7 s) dropped
/// falling debris (`dg_enc_catacombs_hazard_falling_debris_random`, vitals row
/// `Catacombs_Hazard_Debris`) that lived 6.4 to 6.5 s each (`CATACOMBS5` from 1633.8 s, 173
/// sightings); its slice's delayed events turn its stun volume on at 4.0 s and off at 4.6 s
/// and remove it at 6.5 s.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HazardObject {
    /// The object's slice path (the spawner's `m_sliceAsset`).
    pub slice: String,
    /// Seconds from its spawn to its removal (its `DestroyGDEComponent` trigger).
    pub life_s: f32,
    /// Its effect areas.
    pub areas: Vec<EffectArea>,
}

/// A sphere of a hazard object that gives status effects to the players in it while it is on
/// (a `PersistentAreaStatusEffect` component, [`crate::level_script::AREA_STATUS_EFFECT_TYPE`],
/// with the sphere of its entity's detection volume).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectArea {
    /// The status effects it adds (`m_addStatusEffects`).
    pub effects: Vec<String>,
    /// Its sphere radius, metres.
    pub radius_m: f32,
    /// Seconds after the object's spawn when it turns on (its `Activate` delayed event).
    pub on_s: f32,
    /// Seconds after the object's spawn when it turns off (its `Deactivate` delayed event).
    pub off_s: f32,
}

/// One mob of a spawner's alias ([`TargetSlice::alias_mobs`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasMob {
    /// The mob.
    pub mob: TargetMob,
    /// Its weight in the alias (`Percent`).
    pub percent: u32,
}

/// How an area spawner keeps its mobs: [`TargetSlice::mobs_per_spawn`] (its `m_liveCount`) stand
/// at its spawn points, and while the stage that spawned them runs, a dead one comes back after
/// a time in its respawn range. Observed: the extraction's survive stage
/// (`catacombs_extraction_sustainwave`, live count 4, respawn 2 to 7 s, nine points) kept new
/// fodder coming through its 60 s (`CATACOMBS5` 1593.9 to 1617.7 s: ghouls and variant spawns
/// 0.1 to 4 s after a death); Inferred: one comeback per death, at the dead one's point.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AreaRefill {
    /// The shortest and longest time from a death to its comeback, seconds.
    pub respawn_s: [f32; 2],
    /// Its spawn points (`m_locations`), from the slice's origin before the spawn's turn: east,
    /// north, up (metres).
    pub points: Vec<[f32; 3]>,
}

/// The mobs of one spawn of a spawner slice.
const fn one_mob() -> u32 {
    1
}

/// The slices of one `m_sliceAsset` / `m_aliasAsset` pair.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// The slice's path or the alias asset's path.
    pub name: String,
    /// Its slices (one for a slice, the alias's entries for an alias).
    pub slices: Vec<TargetSlice>,
}

/// One spawn of a stage's timeline.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    /// Index of its target in [`Encounter::targets`].
    pub target: usize,
    /// Seconds after the stage started.
    pub delay_s: u32,
    /// How many.
    pub count: u32,
    /// Where (world metres), in order; none spawns at the encounter's place.
    pub locations: Vec<[f32; 3]>,
    /// The turn of each location from the encounter's yaw ([`Encounter::yaw_deg`]), degrees
    /// counter-clockwise, in the order of [`Self::locations`]. A file written before it has
    /// none: a spawn then faces the encounter's way.
    #[serde(default)]
    pub turns_deg: Vec<f32>,
}

/// What an objective waits for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectiveSpec {
    /// `required` deaths of the target (`None`: of the stage's own spawns).
    SliceDestroyed {
        /// Index in [`Encounter::targets`].
        target: Option<usize>,
        /// Deaths needed.
        required: u32,
    },
    /// The target (`None`: a spawn of the encounter) reaches boss phase `phase`.
    BossPhase {
        /// Index in [`Encounter::targets`].
        target: Option<usize>,
        /// The phase.
        phase: u32,
    },
    /// A mob tree of the encounter sends `event`.
    EncounterEvent {
        /// The event id.
        event: String,
    },
    /// An entity of the encounter slice is activated or deactivated by an action (both flags
    /// unset: triggered).
    EntityActiveState {
        /// The entity id.
        entity: u64,
        /// Completes on `Activate`.
        on_activated: bool,
        /// Completes on `Deactivate`.
        on_deactivated: bool,
    },
    /// Completes with its stage's parent.
    WaitForParent,
    /// Every spawn of the stage's timeline spawned and died.
    InvasionWave,
    /// `required` slices spawned by the spawners of the target's spawns (`None`: of the stage's
    /// own spawns) since the stage started. Observed: the stronghold siren's `Wave_02 -
    /// Objective: Stop Cannons` tracks its own three `rw_siren_cannonball_complete_gatherable`
    /// spawns (3 required); its task counted 1, 2 and 3 at the ends of a player's gathers of
    /// them, when each one's spawner spawned its `rw_cannonball_trigger`, and not at their own
    /// spawn (`Playthrough_Part_18` 5183.635 s spawns, 5203.531, 5204.067 and 5217.050 s counts).
    SliceSpawned {
        /// Index in [`Encounter::targets`].
        target: Option<usize>,
        /// Spawns needed.
        required: u32,
    },
}

/// One objective of a stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageObjective {
    /// Seconds; 0 for no limit.
    pub time_limit_s: u32,
    /// The limit completes it (else the limit fails the stage).
    pub succeed_on_timeout: bool,
    /// Its completion fails the stage.
    pub failure: bool,
    /// What it waits for.
    pub kind: ObjectiveSpec,
}

/// One start or end action of a stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StageAction {
    /// An entity event (`EventType`: 0 trigger, 1 untrigger, 2 activate, 3 deactivate).
    EntityEvent {
        /// The entity id.
        entity: u64,
        /// The `EventType` value.
        event: u32,
        /// Also its child entities.
        recursive: bool,
        /// Only for the local player.
        local_player_only: bool,
    },
    /// The encounter's spawns of the target go (`None`: every spawn of the encounter).
    DestroySlice {
        /// Index in [`Encounter::targets`].
        target: Option<usize>,
    },
    /// Every spawn of the stage dies.
    KillSpawns,
}

/// One stage of an encounter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    /// The stage entity's name.
    pub name: String,
    /// The stage entity's id.
    pub entity: u64,
    /// Its children may stay unfinished.
    pub child_stages_optional: bool,
    /// `m_userWave`: its objectives are tasks of the encounter's objective
    /// ([`Encounter::objective_tasks`]).
    #[serde(default)]
    pub user_wave: bool,
    /// Indexes of its child stages in [`Encounter::stages`], in order.
    pub children: Vec<usize>,
    /// Its spawn timeline.
    pub spawns: Vec<Spawn>,
    /// Its objectives.
    pub objectives: Vec<StageObjective>,
    /// Its start actions.
    pub start_actions: Vec<StageAction>,
    /// Its end actions.
    pub end_actions: Vec<StageAction>,
}

/// An encounter's detection volume in the world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detector {
    /// Its shape in its own space.
    pub shape: Geometry,
    /// World from the shape's space.
    pub place: Affine,
}

impl Detector {
    /// Whether the world point `point` is inside it.
    pub fn contains(&self, point: [f32; 3]) -> bool {
        shape_contains(&self.shape, &self.place, point)
    }
}

/// One encounter of a level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Encounter {
    /// The level script entity that spawns it.
    pub name: String,
    /// The encounter slice's pak path.
    pub slice: String,
    /// Its place (world metres).
    pub position: [f32; 3],
    /// Its yaw, degrees.
    pub yaw_deg: f32,
    /// It runs from the instance's start.
    pub auto_activate: bool,
    /// Milliseconds from its activation to its first stage.
    pub auto_activate_delay_ms: u32,
    /// Seconds a stage waits with no player in the volume before the encounter resets.
    pub grace_period_s: u32,
    /// The tree event that resets it; empty for none.
    pub reset_event: String,
    /// The radius of its player volume, metres.
    pub detection_radius_m: Option<f32>,
    /// Its detection volume in the world: the root's first query shape at its place (a shape
    /// takes no entity scale); `None` for a root without one.
    #[serde(default)]
    pub detector: Option<Detector>,
    /// Indexes of its top stages in [`Encounter::stages`], in order.
    pub top_stages: Vec<usize>,
    /// Every stage.
    pub stages: Vec<Stage>,
    /// Every spawn or tracked target.
    pub targets: Vec<Target>,
    /// The spawn rules of the spawner that spawns it (the spawn group's area spawner: its
    /// respawn range and whether it spawns when enabled); `None` when its spawner has none.
    #[serde(default)]
    pub spawner: Option<SpawnerRules>,
    /// The alias spawner that spawns it, when one does (`<level script path>#<entity id>`): it
    /// spawns one slice of its alias at one of its spawn points, so the encounters of one group
    /// are the choices of one spawn (Observed: `Darkness_SpawnGroup_Minor_Master` spawns one of
    /// four breach slices at one of seven points, `m_liveCount` 1).
    #[serde(default)]
    pub spawn_group: Option<String>,
    /// Its difficulty (from 1): its spawns of a slice per difficulty spawn that difficulty's
    /// slice, and its success fires that difficulty's reward; `None` for an encounter of one
    /// difficulty.
    #[serde(default)]
    pub difficulty: Option<u32>,
    /// Its rewards by difficulty (a corrupted breach's root).
    #[serde(default)]
    pub rewards: Option<DifficultyRewards>,
    /// The component number of its root's objectives component (class 3857 on the wire,
    /// `{A42A47DB-4F14-41C6-81D1-19A95BA99F84}` in the slice); `None` for a root without one.
    /// Observed: the replica index of that component is the retail member's component number
    /// (stronghold siren 4, morgaine 4, the remnant 3).
    #[serde(default)]
    pub objectives_component: Option<u64>,
}

/// The name of the root task container (`ConsecutiveTaskContainer`, lower-case CRC-32
/// 0x1004d537) and of each task's container, whose task index follows it
/// (`ConsecutiveTaskContainer2`).
const CONTAINER_NAME: &str = "ConsecutiveTaskContainer";
/// The first task index of an objective.
const FIRST_TASK_INDEX: u8 = 1;
/// The task indexes one objective takes: its own and its container's.
const INDEXES_PER_TASK: u8 = 2;

/// One task of an encounter's objective: one objective of a stage, held alone by its own
/// container ([`Encounter::objective_tasks`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncounterTask {
    /// Index in [`Encounter::stages`].
    pub stage: usize,
    /// Index in that stage's [`Stage::objectives`].
    pub objective: usize,
    /// Its task index; its container's is the next one.
    pub index: u8,
    /// The lower-case CRC-32 of its id, the objective's class name
    /// (`EncounterObjectiveSliceDestroyed` 0x230690d4).
    pub task: u32,
    /// The lower-case CRC-32 of its container's id (`ConsecutiveTaskContainer2` 0xa2781fd7).
    pub container: u32,
}

/// The task tree of an encounter's objective: its tasks in order and the root's index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncounterTasks {
    /// Its tasks, in task index order.
    pub tasks: Vec<EncounterTask>,
    /// The task index of the root container (it holds every container).
    pub root_index: u8,
    /// The lower-case CRC-32 of the root container's id.
    pub root_task: u32,
}

/// The objective class name of objective kind `kind`, when an objective of that kind with
/// `time_limit_s` makes a task. Observed (74 retail encounter roots of 28 slices, encounter
/// objectives note): WaitForParent makes none (2 of 2); EncounterEvent only with a time limit
/// (overseerzane's 1 s `Wave_01_Intro` is task 3, morgaine's untimed `Intro` none); BossPhase
/// only with a tracked slice or alias (8 of 8 with one, morgaine's two without none); every other
/// kind always (SliceDestroyed also without a target, 35 of 35).
const fn task_class(kind: &ObjectiveSpec, time_limit_s: u32) -> Option<&'static str> {
    match kind {
        ObjectiveSpec::SliceDestroyed { .. } => Some("EncounterObjectiveSliceDestroyed"),
        ObjectiveSpec::BossPhase { target: Some(_), .. } => Some("EncounterObjectiveBossPhase"),
        ObjectiveSpec::EncounterEvent { .. } if time_limit_s > 0 => {
            Some("EncounterObjectiveEncounterEvent")
        }
        ObjectiveSpec::EntityActiveState { .. } => {
            Some("EncounterObjectiveEntityActiveStateChanged")
        }
        ObjectiveSpec::InvasionWave => Some("EncounterObjectiveInvasionWave"),
        ObjectiveSpec::SliceSpawned { .. } => Some("EncounterObjectiveSliceSpawned"),
        ObjectiveSpec::BossPhase { target: None, .. }
        | ObjectiveSpec::EncounterEvent { .. }
        | ObjectiveSpec::WaitForParent => None,
    }
}

impl Encounter {
    /// The task tree of its objective (client `FUN_144466230`, encounter objectives note): the
    /// objectives of its [`Stage::user_wave`] stages, the top stages in order and each stage
    /// followed by its child stages, that make a task ([`task_class`]). Each task takes the next
    /// index and its own `ConsecutiveTaskContainer<index>` the one after it; the root container
    /// takes the index after the last. `None` when no objective makes a task or the indexes pass
    /// the wire's one byte (Inferred: the client's counter is a byte).
    pub fn objective_tasks(&self) -> Option<EncounterTasks> {
        let mut order = Vec::new();
        let mut pending: Vec<usize> = self.top_stages.iter().rev().copied().collect();
        while let Some(stage) = pending.pop() {
            if order.contains(&stage) {
                continue;
            }
            order.push(stage);
            if let Some(def) = self.stages.get(stage) {
                pending.extend(def.children.iter().rev());
            }
        }
        let mut tasks = Vec::new();
        let mut index = FIRST_TASK_INDEX;
        for stage in order {
            let Some(def) = self.stages.get(stage).filter(|def| def.user_wave) else {
                continue;
            };
            for (objective, spec) in def.objectives.iter().enumerate() {
                let Some(class) = task_class(&spec.kind, spec.time_limit_s) else {
                    continue;
                };
                let container_index = index.checked_add(1)?;
                tasks.push(EncounterTask {
                    stage,
                    objective,
                    index,
                    task: lowercase_crc32(class),
                    container: lowercase_crc32(&format!("{CONTAINER_NAME}{container_index}")),
                });
                index = index.checked_add(INDEXES_PER_TASK)?;
            }
        }
        (!tasks.is_empty()).then_some(EncounterTasks {
            tasks,
            root_index: index,
            root_task: lowercase_crc32(CONTAINER_NAME),
        })
    }
}

/// A loot container of a level script.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Chest {
    /// The level script entity that spawns it.
    pub name: String,
    /// The container slice's pak path.
    pub slice: String,
    /// Its place (world metres).
    pub position: [f32; 3],
    /// Its loot table.
    pub loot_table: String,
}

/// The script of one instance game mode.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LevelScript {
    /// The level folder.
    pub level: String,
    /// The level script slice's pak path.
    pub script_slice: String,
    /// Its encounters.
    pub encounters: Vec<Encounter>,
    /// Its loot containers.
    pub chests: Vec<Chest>,
    /// Its trigger areas: each entity with a `TriggerAreaComponent` and an engine shape, in
    /// the world. An expedition's script reads them by name (findings
    /// `2026-11-13-catacomb-erosion.md` section C16): the first player in
    /// [`START_AREA_ENTITY`] starts the run's clock, and a player in a task's area ends it.
    #[serde(default)]
    pub trigger_areas: Vec<TriggerArea>,
}

/// A trigger area of a level script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerArea {
    /// The entity's name.
    pub name: String,
    /// Its volume in the world: the entity's engine shape at the entity's place.
    pub volume: Detector,
}

impl LevelScript {
    /// The volume of its trigger area `name`.
    pub fn trigger_area(&self, name: &str) -> Option<&Detector> {
        self.trigger_areas
            .iter()
            .find(|area| area.name == name)
            .map(|area| &area.volume)
    }
}

/// The scripts of every instance game mode, by game mode id (several modes share a level,
/// each with its own script: the soul trials of the open world map, the season trials).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DungeonScripts {
    /// The scripts by game mode id.
    pub game_modes: BTreeMap<String, LevelScript>,
}

/// Why the file cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum DungeonScriptsError {
    /// The file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not dungeon scripts.
    #[error("{path}: {source}")]
    Json {
        /// The file.
        path: PathBuf,
        /// Why.
        source: serde_json::Error,
    },
}

/// The dungeon script file of `client_build` in `dir`.
pub fn dungeon_scripts_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!(
        "{DUNGEON_SCRIPTS_PREFIX}{client_build}{DUNGEON_SCRIPTS_EXTENSION}"
    ))
}

impl DungeonScripts {
    /// Reads `path`; a missing file has no scripts (content from before this file).
    ///
    /// # Errors
    ///
    /// Returns [`DungeonScriptsError`] when the file cannot be read or is not dungeon scripts.
    pub fn load(path: &Path) -> Result<Self, DungeonScriptsError> {
        match std::fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|source| DungeonScriptsError::Json {
                    path: path.to_path_buf(),
                    source,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(DungeonScriptsError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Writes the file to `path`.
    ///
    /// # Errors
    ///
    /// Returns [`DungeonScriptsError`] when a value cannot be written or the file cannot be.
    pub fn write(&self, path: &Path) -> Result<(), DungeonScriptsError> {
        let text =
            serde_json::to_string_pretty(self).map_err(|source| DungeonScriptsError::Json {
                path: path.to_path_buf(),
                source,
            })?;
        std::fs::write(path, text).map_err(|source| DungeonScriptsError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// The script of game mode `id`.
    pub fn game_mode(&self, id: &str) -> Option<&LevelScript> {
        self.game_modes.get(id)
    }

    /// The task names of the waypoints of every spawned slice.
    pub fn waypoint_tasks(&self) -> impl Iterator<Item = &str> {
        self.game_modes
            .values()
            .flat_map(|script| &script.encounters)
            .flat_map(|encounter| &encounter.targets)
            .flat_map(|target| &target.slices)
            .filter_map(|slice| slice.waypoints.as_ref())
            .flat_map(|path| path.tasks.iter().flatten())
            .map(|task| task.task.as_str())
    }

    /// Whether a game mode on level `folder` has a script.
    pub fn has_level(&self, folder: &str) -> bool {
        self.game_modes
            .values()
            .any(|script| script.level == folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: &str = "25810808";
    const LEVEL: &str = "nw_dungeon_windsward_00";
    const MODE: &str = "DungeonAmrine";

    fn scripts() -> DungeonScripts {
        let encounter = Encounter {
            name: "EncounterBoss".to_owned(),
            slice: "slices/dungeon_eventmanager/encounters/dg_enc_windsward_00_boss.dynamicslice"
                .to_owned(),
            position: [886.1, 906.0, 141.0],
            auto_activate: true,
            grace_period_s: 20,
            detection_radius_m: Some(30.0),
            top_stages: vec![0],
            stages: vec![Stage {
                name: "Wave_01".to_owned(),
                spawns: vec![Spawn {
                    target: 0,
                    count: 1,
                    ..Spawn::default()
                }],
                objectives: vec![StageObjective {
                    time_limit_s: 0,
                    succeed_on_timeout: false,
                    failure: false,
                    kind: ObjectiveSpec::SliceDestroyed {
                        target: Some(0),
                        required: 1,
                    },
                }],
                end_actions: vec![StageAction::KillSpawns],
                ..Stage::default()
            }],
            targets: vec![Target {
                name: "boss".to_owned(),
                slices: vec![TargetSlice {
                    path: "boss".to_owned(),
                    guid: [1; GUID_LEN],
                    percent: 0,
                    mob: Some(TargetMob {
                        vitals_id: "Undead_Brute_Boss".to_owned(),
                        category_id: String::new(),
                        level: 25,
                    }),
                    mobs_per_spawn: 1,
                    waypoints: None,
                    difficulty: None,
                    difficulty_mobs: BTreeMap::new(),
                    refill: None,
                    alias_mobs: Vec::new(),
                    spawned_slices: vec!["slices/characters/undead_brute.dynamicslice".to_owned()],
                    family: "Lost".to_owned(),
                    timed_mobs: vec![TimedMob {
                        delay_s: 2.0,
                        offset: [1.0, -2.0, 0.0],
                        mob: TargetMob {
                            vitals_id: "AngryEarth_Dryad_Siren_Tornado_Catacombs".to_owned(),
                            category_id: String::new(),
                            level: 25,
                        },
                    }],
                    lifetime_s: Some(33.0),
                    hazard: Some(HazardObject {
                        slice: "slices/catacombs/test_falling_debris.dynamicslice".to_owned(),
                        life_s: 6.5,
                        areas: vec![EffectArea {
                            effects: vec!["Stunned_Afflict".to_owned()],
                            radius_m: 1.2,
                            on_s: 4.0,
                            off_s: 4.6,
                        }],
                    }),
                    randomizer: Some(SpawnRandomizer {
                        picks: [1, 1],
                        choices: vec![RandomizerChoice {
                            mob: TargetMob {
                                vitals_id: "Skeleton_Crawler_DG_Reekwater_00_Spawn".to_owned(),
                                category_id: "Skeleton_Crawler_DG_Reekwater_00".to_owned(),
                                level: 60,
                            },
                            offset: [0.5, 0.0, 0.0],
                            respawn_s: Some(8.0),
                            stop_radius_m: Some(5.0),
                        }],
                    }),
                    detectable_radius_m: Some(1.0),
                    gather_mobs: vec![TimedMob {
                        delay_s: 3.172,
                        offset: [-2.0, 1.0, 0.0],
                        mob: TargetMob {
                            vitals_id: "Sulfur_Elemental_Entity_Spawn".to_owned(),
                            category_id: String::new(),
                            level: 60,
                        },
                    }],
                }],
            }],
            ..Encounter::default()
        };
        DungeonScripts {
            game_modes: BTreeMap::from([(
                MODE.to_owned(),
                LevelScript {
                    level: LEVEL.to_owned(),
                    script_slice: "slices/dungeon/levelscripts/amrinedungeon.dynamicslice"
                        .to_owned(),
                    encounters: vec![encounter],
                    chests: vec![Chest {
                        name: "LockedChest_Simon".to_owned(),
                        loot_table: "ChestLootElite".to_owned(),
                        ..Chest::default()
                    }],
                    trigger_areas: Vec::new(),
                },
            )]),
        }
    }

    #[test]
    fn the_file_writes_and_reads_back() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dungeon_scripts_path(dir.path(), BUILD);
        assert!(path.ends_with(format!("dungeon-scripts-{BUILD}.json")));
        let written = scripts();
        written.write(&path).expect("write");
        let read = DungeonScripts::load(&path).expect("load");
        assert_eq!(read, written);
        assert_eq!(
            read.game_mode(MODE).map(|script| script.encounters.len()),
            Some(1)
        );
        assert!(read.game_mode("QuestCaerDun").is_none());
        assert!(read.has_level(LEVEL) && !read.has_level("newworld_vitaeeterna"));
    }

    fn objective(kind: ObjectiveSpec, time_limit_s: u32) -> StageObjective {
        StageObjective {
            time_limit_s,
            succeed_on_timeout: time_limit_s > 0,
            failure: false,
            kind,
        }
    }

    fn user_stage(objectives: Vec<StageObjective>) -> Stage {
        Stage {
            user_wave: true,
            objectives,
            ..Stage::default()
        }
    }

    const fn kills(required: u32) -> ObjectiveSpec {
        ObjectiveSpec::SliceDestroyed {
            target: Some(0),
            required,
        }
    }

    #[test]
    fn each_task_has_its_own_container_and_morgaine_s_untracked_phases_make_none() {
        // Retail `NH-MSQ-6.3` 21.707 and 29.839 s: Phase_2's two kill objectives are tasks 1
        // and 3 (containers 2 and 4), the root is 5; the untimed intro event, the wait and the
        // two boss phases without a tracked slice make no task.
        let boss_phase = |phase| ObjectiveSpec::BossPhase {
            target: None,
            phase,
        };
        let event = ObjectiveSpec::EncounterEvent {
            event: "Intro_Complete".to_owned(),
        };
        let encounter = Encounter {
            top_stages: vec![0, 6],
            stages: vec![
                Stage {
                    children: vec![1, 2, 3, 4, 5],
                    objectives: vec![objective(kills(1), 0)],
                    ..Stage::default()
                },
                user_stage(vec![objective(ObjectiveSpec::WaitForParent, 0)]),
                user_stage(vec![objective(event, 0)]),
                user_stage(vec![objective(boss_phase(1), 0)]),
                user_stage(vec![objective(kills(2), 0), objective(kills(2), 0)]),
                user_stage(vec![objective(boss_phase(3), 0)]),
                Stage::default(),
            ],
            ..Encounter::default()
        };
        let tree = encounter.objective_tasks().expect("tasks");
        let slice_destroyed = lowercase_crc32("EncounterObjectiveSliceDestroyed");
        assert_eq!(slice_destroyed, 0x2306_90d4);
        assert_eq!(
            tree.tasks,
            vec![
                EncounterTask {
                    stage: 4,
                    objective: 0,
                    index: 1,
                    task: slice_destroyed,
                    container: 0xa278_1fd7,
                },
                EncounterTask {
                    stage: 4,
                    objective: 1,
                    index: 3,
                    task: slice_destroyed,
                    container: 0x4b1b_bae2,
                },
            ]
        );
        assert_eq!((tree.root_index, tree.root_task), (5, 0x1004_d537));
    }

    #[test]
    fn a_timed_event_and_a_tracked_boss_phase_are_tasks_in_stage_order_with_children_after() {
        // Retail overseerzane: the 1 s intro event is task 3 (0xf96f98f5) between the delay and
        // the kills; the root is 9. A tracked boss phase is a task (0x41a72d8e), and a child
        // stage's task follows its parent's.
        let tracked_phase = ObjectiveSpec::BossPhase {
            target: Some(0),
            phase: 1,
        };
        let event = ObjectiveSpec::EncounterEvent {
            event: "Intro_Complete".to_owned(),
        };
        let encounter = Encounter {
            top_stages: vec![0, 1, 2, 3, 5],
            stages: vec![
                Stage::default(),
                user_stage(vec![objective(kills(1), 0)]),
                user_stage(vec![objective(event, 1)]),
                Stage {
                    children: vec![4],
                    ..user_stage(vec![objective(kills(2), 0)])
                },
                user_stage(vec![objective(tracked_phase, 0)]),
                user_stage(vec![objective(kills(1), 0)]),
            ],
            ..Encounter::default()
        };
        let tree = encounter.objective_tasks().expect("tasks");
        let tasks: Vec<(usize, u8, u32)> = tree
            .tasks
            .iter()
            .map(|task| (task.stage, task.index, task.task))
            .collect();
        assert_eq!(
            tasks,
            vec![
                (1, 1, 0x2306_90d4),
                (2, 3, 0xf96f_98f5),
                (3, 5, 0x2306_90d4),
                (4, 7, 0x41a7_2d8e),
                (5, 9, 0x2306_90d4),
            ]
        );
        assert_eq!(tree.root_index, 11);
    }

    #[test]
    fn an_encounter_without_a_task_has_no_objective() {
        let encounter = Encounter {
            top_stages: vec![0],
            stages: vec![Stage {
                objectives: vec![objective(kills(1), 0)],
                ..Stage::default()
            }],
            ..Encounter::default()
        };
        assert_eq!(encounter.objective_tasks(), None);
    }

    #[test]
    fn a_missing_file_has_no_scripts_and_a_bad_one_is_an_error() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dungeon_scripts_path(dir.path(), BUILD);
        assert_eq!(
            DungeonScripts::load(&path).expect("missing"),
            DungeonScripts::default()
        );
        std::fs::write(&path, "not json").expect("write");
        assert!(matches!(
            DungeonScripts::load(&path),
            Err(DungeonScriptsError::Json { .. })
        ));
    }
}
