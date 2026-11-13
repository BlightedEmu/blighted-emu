// SPDX-License-Identifier: AGPL-3.0-or-later
//! Mob AI from the game's own data (behavior-tree study 2026-09-18, sections 3 and 5): each mob
//! of a creature slice whose tree Server.pak has runs that tree (`nw_ai::Brain`), with the slice's
//! config values, vision, navigation configs and CAGE grid from the AI data file
//! (`ai-data-<build>.bin`, `content-tool ai-data`). This module holds the shared data, the design
//! files' overrides of a spawn, and one mob's mind: what it perceives, what the engine keys of
//! its tree read, and how its tree's commands move and animate it.
//!
//! Marks: **Observed** (read in the data or the capture), **Inferred** (follows from them, not
//! seen run), **Unknown** (not found).
//!
//! What the engine keys are (the writers in the client are Unknown, study section 3.1):
//! - `Has_Target`, `Target_Pos`: the mob's target (see [`TreeMind::perceive`]).
//! - `Target_Dist`: horizontal metres (Inferred: the boar's 3 m and 7 m bands fit the retail
//!   attack starts at 1.69 to 5.30 m; `content-tool` `ai_replay`).
//! - `Target_Angle`: signed degrees from the facing to the target, positive on the mob's left
//!   (the trace rule; Inferred from the Turn sub-tree's `< -30 Or > 30`).
//! - `Target_Visible`: the target is within the combat vision distance (Inferred: the retail
//!   boar turned to a player at 77 to 107 degrees, so its combat cone of 180 degrees to each
//!   side does not hide the target in a fight; trace finding a). It does not end the chase
//!   (see [`MindState::perceive`]).
//! - `Target_Reachable`, `Target_Directly_Reachable`: the straight line to the target stands on
//!   floor within the slice's `m_maxSlope` (Inferred: the server has no navigation mesh, see
//!   [`floor_line`]).
//! - `Time_Since_Target_Reachable`: seconds since the target was last reachable, or since the
//!   mob took it (Inferred from the name and `TargetUnreachableResetTime` 5 s: a mob gets that
//!   long to reach a new target before `UnreachableTargetLeash` sends it home).
//! - `Target_Score`: [`HELD_TARGET_SCORE`] while it has a target.
//! - `Target_Vitals_State`, `Vitals_State`: 4 while alive (Inferred: the trees name the test
//!   `$Target_Vitals_State Equal 4` "HasAliveTarget", 154 times; a dead player is no target).
//! - `Health_Percent`: 0 to 100 (Observed: the trees compare it with 5 to 100, and 33.33 and
//!   66.67 for thirds).
//! - `Spawn_Pos`, `Last_Damage_Pos`, `Full_Body_Reaction`, `Waypoint_Pos`, `Waypoint_Duration`.
//! - `Passive`: the slice's `Is_Passive` (Inferred: the boar's `IsPassive?` branch reads
//!   `Passive`, its slice sets `Is_Passive`).
//! - Every other engine key is not set: its value is Unknown, and the trees then read their
//!   fallback or the slice config (for example `Leash_Time_Limit`: the cvar values are Unknown,
//!   so the leash takes the branch without a time limit).

use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use nw_ai::brain::{Brain, Command, Destination, MotionOutcome, Runtime, Slot, World};
use nw_ai::data::{
    AiData, Archetype, CageAction, FacingMode, FragmentLength, FragmentVariant, MobCage,
    NavConfig, ProcWindow, SliceTriggerRef,
};
use nw_ai::keys::EngineKey;
use nw_ai::node::{
    ActionTarget, EntityQueryParams, NodeId, NodeKind, PositionType, SelectPositionParams,
    TagOperation, TrackerParams,
};
use nw_ai::rng::Rng48;
use nw_ai::value::{Blackboard, KeyId, NameId, Param, Value};
use nw_content::combat::MobGuard;
use nw_content::container::ContainerError;
use nw_content::mob_prefabs::ASSET_ID_LEN;
use nw_content::status_effects::Control;
use nw_sim_core::character::STEP_UP_M;
use nw_sim_core::{Ground, Mover, Position, Seen, Steering, Threat, yaw_gap, yaw_toward};
use nw_world::WorldFile;
use nw_world::records::{AiBehaviour, AiProfile, PathMode, PatrolPath};
use serde::Deserialize;

use crate::mob_homing::{self, Body, HOMING_CLIP, HomingClip, HomingRun, PARAM_TRUE};
use crate::mob_path::{PathPlan, find_path};
use crate::mobs::MobReach;

/// A moving mob keeps its own height when the level's ground lies more than this far below it
/// (our engineering limit): it stands on a floor the height map does not hold, such as the
/// tutorial ship's deck high above the sea bed (live 2026-09-12: the sailor dropped under the
/// ship and vanished).
pub const MAX_STEP_DOWN_M: f32 = 3.0;
/// A mob that sees notices a player this near at any bearing: the perception profile's
/// `m_radiusOfImmediateVicinity` (Inferred value, mobs note 2026-10-31: three beach sailors
/// alerted 0.3 to 0.65 s after the player stood 3.2 to 3.8 m away beside or behind them, two of
/// them at -113 and -115 deg, out of their cone of 90 deg to each side; the client default is
/// 2.5 m and the server profile is Unknown).
pub(crate) const IMMEDIATE_VICINITY_M: f32 = 3.8;
/// Longest time one AI step may move a mob, so a stalled tick never makes it jump (our
/// engineering limit).
const MAX_STEP_MS: u64 = 1_000;
/// The step of a floor check along a line (our engineering limit: half a body).
const FLOOR_STEP_M: f32 = 0.5;
/// A body this near the floor under it stands on that floor (our engineering limit: a floor
/// check's step).
const ON_FLOOR_M: f32 = 0.5;
/// The most a mob's feet rise in one floor check step (Observed: the highest rise of a retail
/// mob between two of its places was 0.58 m over the beach captures; ours climbed 1.36 m in
/// 0.21 m onto a solid, live 2026-10-29 18:29:39.797, mob ground height note).
const MAX_STEP_RISE_M: f32 = 0.6;
/// The turns, nearest first, a mob tries when its straight way meets a wall, a climb or an edge:
/// it walks around the solid (Inferred: retail mobs follow the level's navigation mesh around
/// rocks and crates; ours has none, live 2026-10-29: Janszoon stood blocked by a rock 2 m from
/// his spawn and never reached the player).
const DETOUR_TURNS_DEG: [f64; 6] = [30.0, -30.0, 60.0, -60.0, 90.0, -90.0];
/// The longest first leg of a way around a solid (our engineering limit: about a rock's size).
const DETOUR_LEG_M: f32 = 4.0;
/// A reach check's answer holds while the mob and its target each stay this near where it ran
/// (our engineering limit: the check's own floor step). Live 2026-11-01: with a check on every
/// step, half of the mob time of a sim of 366 players went to the checks (mob part 11.1 ms, 5.5
/// ms without them).
const REACH_RECHECK_M: f32 = FLOOR_STEP_M;
/// A reach answer that took a way search holds while the mob and its target stay this near
/// where it ran (our engineering limit: a search costs as much as hundreds of checks).
const PATH_RECHECK_M: f32 = 3.0;
/// The least time between two way searches of one mob, ms (our engineering limit).
const PATH_RETRY_MS: u64 = 1_000;
/// A way serves a goal this near the goal it was searched for, m (our engineering limit: about a
/// navigation config's pick around its target).
const PATH_GOAL_SLACK_M: f32 = 3.0;
/// A place of a way is reached this near, m (our engineering limit: half a floor step; the way
/// was checked from that place, so a place left farther away can put a corner of a solid on the
/// line to the next one).
const WAYPOINT_REACHED_M: f32 = FLOOR_STEP_M * HALF;
/// A mob in a fight keeps a place of its way out each this far, for its way home when no search
/// finds one, m (our engineering limit).
const TRAIL_STEP_M: f32 = 2.0;
/// The most places of a trail; a longer trail keeps every second place (our engineering limit).
const TRAIL_MAX: usize = 256;
/// The slice config param that gives the engine key `Difficulty_Tier` (Inferred: Janszoon's
/// slice, `corrupted_sailor_starterbeach_named_00`, sets `DifficultyTier` 1 and no
/// `Difficulty_Tier`; his great axeman trees attack only at a tier, and tier 1 picks the
/// `Damned_GreatAxeman_Medium_AttackTree`, which holds the two attacks he used in the retail
/// newchar run, `Attack_Close_2H_Axe_04` and `Attack_Far_2H_Axe_01`. Without it he only strafed,
/// live 2026-10-29).
const DIFFICULTY_TIER_PARAM: &str = "DifficultyTier";
/// The tree fact that turns on `DistanceLeash` (with `Leash_Opt_Out` 0). Neither the client's
/// engine key table nor a slice sets it; the server engine does (Inferred: every retail reset of
/// the beach and Steal the Boat captures plays `DistanceLeash`'s `Combat_Fidget` then
/// `Reset_Move`, the dynasty mobs 61 to 73 m from where they started, past their
/// `CombatLeashDistance`). A mob starts with it true unless its slice config sets it. Without it
/// no tree mob ever walked home (live 2026-10-29: Janszoon stood 51 m from his spawn, past his
/// 45 m leash).
const DOES_LEASH_KEY: &str = "DoesLeash";
/// The slice config param that gives the engine key `Leash_Opt_Out` (a dungeon mob's opt-out of
/// the leash); 0 without one.
const LEASH_OPT_OUT_PARAM: &str = "Leash_Opt_Out";
/// `Target_Vitals_State` and `Vitals_State` of a living entity (see the module notes).
const ALIVE_VITALS_STATE: i64 = 4;
/// The vitals state filter of an `EntityQueryCondition` that filters nothing (Inferred: "Check
/// For Players" holds it in `MatchingVitalsState` and in `NotMatchingVitalsState` at once).
const NO_VITALS_STATE_FILTER: i64 = 6;
/// The largest `Radius` of an `EntityQueryCondition` (Observed: the client logs "Invalid (> 200)
/// radius specified in EntityQueryCondition").
const ENTITY_QUERY_MAX_RADIUS_M: f32 = 200.0;
/// `Health_Percent` is 0 to 100 (module notes).
const PERCENT: f32 = 100.0;
const MS_PER_S: f32 = 1_000.0;
const MS_PER_S_F64: f64 = 1_000.0;
const HALF: f32 = 0.5;
const FULL_TURN_DEG: f64 = 360.0;
/// `SelectPositionParams.Direction` (study section 1.4: `AnchorToOrigin` 1, `OriginOrientation`
/// 2 Observed; `OriginToAnchor` 0 Inferred).
const ANCHOR_TO_ORIGIN: i64 = 1;
const ORIGIN_ORIENTATION: i64 = 2;
/// The grid aliases of the actions the engine starts (Observed: `<AliasMap>` of the creature
/// grids, `Reaction` and `Death`).
const REACTION_ALIAS: &str = "Reaction";
const DEATH_ALIAS: &str = "Death";
/// The procedural clip of a fragment marker and its name param (`npc_damned_anims.adb`:
/// `<Procedural type="CAGE-Named"><ProceduralParams><Name value="DisableBlock"/>`).
const NAMED_CLIP: &str = "CAGE-Named";
const NAMED_CLIP_NAME: &str = "Name";
/// The reaction action of a broken guard (`baseai_reaction.actionlist` and the creature
/// reaction lists: fragment `BrokenGuard` or `r_BlockBroken`, tags `Alerted` and `BlockBroken`).
const BROKEN_GUARD_ACTION: &str = "BrokenGuard";
/// How long a broken guard's reaction lasts when its fragment's length is Unknown (Inferred:
/// about a hit reaction's length).
const BROKEN_GUARD_MS: u64 = 1_000;
/// The stagger of a mob whose grit took its last stamina (`baseai_stagger.actionlist` and the
/// other `*_stagger` lists: fragment `r_Stagger`, started by the server).
const STAGGER_ACTION: &str = "Reaction_Staggered";
/// The status effect a mob's slice gives it to have the stagger (`m_initialStatusEffects`; the
/// list: "Add this into your character's initial status effects to enable the Stagger
/// mechanic on them").
const ENABLE_STAGGER_EFFECT: &str = "AI_Enable_Stagger";
/// The markers that keep the stagger from starting (`DisableStagger`: "Fully disables Stagger
/// during specific fragments"; `DisableStaggerReaction`: grit stays on, the stagger waits for
/// the next hit after the marker).
const DISABLE_STAGGER_MARKERS: [&str; 2] = ["DisableStagger", "DisableStaggerReaction"];
/// The effect the stagger holds while it plays (`ModifyStatusEffects` of `Reaction_Staggered`:
/// applied on enter, removed on exit; `StaminaRate` -100 holds the regen, and its
/// `OnEndStatusEffect` `AI_Staggered_StaggerDamageResist` follows).
const STAGGER_TRACKER_EFFECT: &str = "AI_Staggered_Tracker";
/// How long before its fragment's end the stagger ends and its `ResetStamina` fills the
/// stamina: its deactivation `IfFragmentDone ExitOffsetSeconds="0.1"` (Observed: the ledgers'
/// child 44 was full 5.80 s after the zero state on the dynasty, corruption and sand heavies,
/// whose `r_Stagger` lasts 6.0 s, and 5.13 s after it on the Motherwell stag, 5.33 s; the other
/// 0.1 s lie within the state delay, `zero44` 2026-11-13).
const STAGGER_EXIT_OFFSET_MS: u64 = 100;
/// Milliseconds per second of a fragment length.
const MS_PER_SECOND_F32: f32 = 1_000.0;
/// The procedural clips of a fragment and their parameters (Observed in `npc_boar_anims.adb`).
const DAMAGE_CLIP: &str = "CAGE-Damage";
const DAMAGE_ROW_PARAM: &str = "DamageTableRow";
const SHAPE_RADIUS_PARAM: &str = "MeleeAttackShapeRadius";
const SHAPE_HALF_HEIGHT_PARAM: &str = "MeleeAttackCapsuleHalfHeight";
/// A ranged attack clip and its parameters (Observed: Janszoon's `Officer_AnchorToss` in
/// `npc_damned_anims.adb`).
const RANGED_CLIP: &str = "CAGE-RangedAttack";
const RANGED_ATTACK_PARAM: &str = "RangedAttackName";
const SLICE_OVERRIDE_PARAM: &str = "SliceOverride";
const LEAD_TARGET_PARAM: &str = "AILeadTarget";
const LEAD_MAX_ANGLE_PARAM: &str = "AILeadTargetMaxAngle";
/// A spell clip and its parameters (Observed: 2,667 `CAGE-CastSpell` clips of the creature
/// databases, every one with `SpellName`, `AIAimAtTarget` and `ProjectileSpeed`).
const CAST_CLIP: &str = "CAGE-CastSpell";
const SPELL_NAME_PARAM: &str = "SpellName";
const AIM_AT_TARGET_PARAM: &str = "AIAimAtTarget";
const PROJECTILE_SPEED_PARAM: &str = "ProjectileSpeed";
/// The navigation config of a run (Observed: the boar's `Run` nodes name `RunToTarget`; retail
/// boar runs reached 6.96 m/s, its maximum 7.0).
const RUN_CONFIG: &str = "RunToTarget";
/// The navigation config of a flight (Observed: the prey trees' `FleeFromTarget`,
/// `FleeAnyTowardHome`, `FleeAnyDirection` and `FleeFromDamage` name `Flee`; no slice has a
/// navigation config of that kind). A flight without a picked point runs from its target
/// ([`MotionKind::Away`]).
const FLEE_CONFIG: &str = "Flee";
/// How far ahead of a fleeing mob it steers, straight away from what it flees (our engineering
/// limit: the first leg of a way around a solid).
const FLEE_AHEAD_M: f32 = DETOUR_LEG_M;
/// `Target_Score` of the target a mob holds. The targeting profile's curves are Unknown (study
/// section 3.2), so the score itself is Unknown; Inferred: a mob holds only a target it picked,
/// whose score is above 0, and 1 is the top of a utility. Only the prey and goat trees read
/// the key (Observed: `Target_Score > 0` and `>= 0.05`, four times each). Without it no prey
/// fled and no goat fought: the live goat `sheep_churro_8` (archetype 748) held a player 35 s
/// with no move (sim log 2026-11-12 14:32:52 to 14:33:27): its `HighTargetScore` (node 18123)
/// failed, so the tree fell to its ambient wander.
const HELD_TARGET_SCORE: f32 = 1.0;
/// The status effects of a leash reset (Observed: `ResetToHome` and `DistanceLeash` apply
/// `ResetAI` or `ResetAINoHeal` while the mob walks home).
const RESET_EFFECT: &str = "ResetAI";
const RESET_NO_HEAL_EFFECT: &str = "ResetAINoHeal";
/// Slice config keys the design overrides set (Observed names in the slices and the trees).
const IS_PASSIVE_KEY: &str = "Is_Passive";
const ENABLE_WANDER_KEY: &str = "Enable_Wander";
const WANDER_MAX_KEY: &str = "WanderDistanceMax";
const AMBIENT_LEASH_KEY: &str = "AmbientLeashDistance";
/// The combat leash (Observed: DistanceLeash reads it).
pub(crate) const COMBAT_LEASH_KEY: &str = "CombatLeashDistance";
/// The attack cooldown of the base trees (Observed: `AttackCooldown` in boar_base and the
/// legion and damned trees).
const ATTACK_COOLDOWN: &str = "AttackCooldown";
/// The networked tag of the players (Observed: 12 `TargetTagFilter`s of the Server.pak trees
/// name it; Inferred: every player carries it, so a filter of it takes the players the mob sees).
const PLAYER_TAG: &str = "Player";

/// `[mob_ai]`: how often a mob's tree runs, by what it does and how near the nearest player is
/// (trace finding c, `local-data/scratch-work/btree/trace/findings.txt`: the retail ALC state
/// rate, Inferred equal to the AI tick rate; the cvars `javelin-server.ai-update-rate-*` hold the
/// retail values, which are Unknown).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiRates {
    /// A mob with a target, at every distance (retail 33 to 34 ms).
    pub combat_interval_ms: u64,
    /// A moving mob without a target: near, middle and far (retail 98, 218 and 333 ms).
    pub moving_interval_ms: [u64; 3],
    /// Where the middle and far bands of a moving mob start (retail 20 m and 40 m).
    pub moving_range_m: [f32; 2],
    /// A mob that stands without a target, at every distance (retail 333 ms).
    pub idle_interval_ms: u64,
}

/// The retail rates of trace finding c.
pub const RETAIL_AI_RATES: AiRates = AiRates {
    combat_interval_ms: 33,
    moving_interval_ms: [98, 218, 333],
    moving_range_m: [20.0, 40.0],
    idle_interval_ms: 333,
};

impl Default for AiRates {
    fn default() -> Self {
        RETAIL_AI_RATES
    }
}

/// What a mob does, for its tick rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// It has a target.
    Combat,
    /// It moves without a target.
    Moving,
    /// It stands without a target.
    Idle,
}

impl AiRates {
    /// Whether every interval is above 0 and the bands rise.
    pub fn is_valid(&self) -> bool {
        self.combat_interval_ms > 0
            && self.idle_interval_ms > 0
            && self.moving_interval_ms.iter().all(|ms| *ms > 0)
            && self.moving_range_m[0].is_finite()
            && self.moving_range_m[0] > 0.0
            && self.moving_range_m[1].is_finite()
            && self.moving_range_m[1] > self.moving_range_m[0]
    }

    /// The time to the next tick of a mob doing `activity` with the nearest player
    /// `nearest_m` away.
    pub fn interval_ms(&self, activity: Activity, nearest_m: f32) -> u64 {
        match activity {
            Activity::Combat => self.combat_interval_ms,
            Activity::Idle => self.idle_interval_ms,
            Activity::Moving => {
                let band = self
                    .moving_range_m
                    .iter()
                    .take_while(|start| nearest_m >= **start)
                    .count();
                self.moving_interval_ms[band]
            }
        }
    }
}

/// The names the sim looks for in the data, resolved once.
#[derive(Debug, Clone, Copy, Default)]
struct Names {
    reaction: Option<NameId>,
    death: Option<NameId>,
    named_clip: Option<NameId>,
    named_clip_name: Option<NameId>,
    broken_guard: Option<NameId>,
    stagger: Option<NameId>,
    enable_stagger: Option<NameId>,
    disable_stagger: [Option<NameId>; 2],
    stagger_tracker: Option<NameId>,
    damage_clip: Option<NameId>,
    homing_clip: Option<NameId>,
    damage_row: Option<NameId>,
    shape_radius: Option<NameId>,
    shape_half_height: Option<NameId>,
    ranged_clip: Option<NameId>,
    ranged_attack: Option<NameId>,
    slice_override: Option<NameId>,
    lead_target: Option<NameId>,
    lead_max_angle: Option<NameId>,
    cast_clip: Option<NameId>,
    spell_name: Option<NameId>,
    aim_at_target: Option<NameId>,
    projectile_speed: Option<NameId>,
    run_config: Option<NameId>,
    flee_config: Option<NameId>,
    reset: Option<NameId>,
    reset_no_heal: Option<NameId>,
    attack_cooldown: Option<NameId>,
    is_passive: Option<KeyId>,
    enable_wander: Option<KeyId>,
    wander_max: Option<KeyId>,
    ambient_leash: Option<KeyId>,
    combat_leash: Option<KeyId>,
}

/// The slice asset of each vitals id's most seen look (the mob prefab table's).
pub type LookAssets = HashMap<u32, [u8; ASSET_ID_LEN]>;
/// The vitals id of each creature slice asset of the mob prefab table: what a tree's
/// `ExecuteSpawn` of that slice spawns.
pub type SliceVitals = HashMap<[u8; ASSET_ID_LEN], u32>;

/// The shared AI of a sim: the data, its runtime, the tick rates, and the slice of each vitals
/// id's look.
#[derive(Debug)]
pub struct MobAi {
    runtime: Runtime,
    names: Names,
    rates: AiRates,
    looks: LookAssets,
    slice_vitals: SliceVitals,
    /// For each archetype, whether the data gives its chase distance: its slice's
    /// `CombatLeashDistance`, or a literal of its trees' conditions on that key
    /// ([`chase_from_data`]).
    chase_data: Vec<bool>,
}

impl MobAi {
    /// The AI of `data` with tick rates `rates`; a mob of a vitals id shows the slice `looks`
    /// gives, unless its spawn names its own.
    pub fn new(data: impl Into<Arc<AiData>>, rates: AiRates, looks: LookAssets) -> Self {
        let runtime = Runtime::new(data);
        let name = |text: &str| runtime.name_id(text);
        let key = |text: &str| runtime.data().key(text);
        let names = Names {
            reaction: name(REACTION_ALIAS),
            death: name(DEATH_ALIAS),
            named_clip: name(NAMED_CLIP),
            named_clip_name: name(NAMED_CLIP_NAME),
            broken_guard: name(BROKEN_GUARD_ACTION),
            stagger: name(STAGGER_ACTION),
            enable_stagger: name(ENABLE_STAGGER_EFFECT),
            disable_stagger: DISABLE_STAGGER_MARKERS.map(name),
            stagger_tracker: name(STAGGER_TRACKER_EFFECT),
            damage_clip: name(DAMAGE_CLIP),
            homing_clip: name(HOMING_CLIP),
            damage_row: name(DAMAGE_ROW_PARAM),
            shape_radius: name(SHAPE_RADIUS_PARAM),
            shape_half_height: name(SHAPE_HALF_HEIGHT_PARAM),
            ranged_clip: name(RANGED_CLIP),
            ranged_attack: name(RANGED_ATTACK_PARAM),
            slice_override: name(SLICE_OVERRIDE_PARAM),
            lead_target: name(LEAD_TARGET_PARAM),
            lead_max_angle: name(LEAD_MAX_ANGLE_PARAM),
            cast_clip: name(CAST_CLIP),
            spell_name: name(SPELL_NAME_PARAM),
            aim_at_target: name(AIM_AT_TARGET_PARAM),
            projectile_speed: name(PROJECTILE_SPEED_PARAM),
            run_config: name(RUN_CONFIG),
            flee_config: name(FLEE_CONFIG),
            reset: name(RESET_EFFECT),
            reset_no_heal: name(RESET_NO_HEAL_EFFECT),
            attack_cooldown: name(ATTACK_COOLDOWN),
            is_passive: key(IS_PASSIVE_KEY),
            enable_wander: key(ENABLE_WANDER_KEY),
            wander_max: key(WANDER_MAX_KEY),
            ambient_leash: key(AMBIENT_LEASH_KEY),
            combat_leash: key(COMBAT_LEASH_KEY),
        };
        let chase_data = runtime
            .data()
            .archetypes
            .iter()
            .map(|arch| chase_from_data(runtime.data(), arch, names.combat_leash))
            .collect();
        Self {
            runtime,
            names,
            rates,
            looks,
            slice_vitals: SliceVitals::new(),
            chase_data,
        }
    }

    /// How far a mob of archetype `archetype` and vitals `CreatureType` `creature_type` chases
    /// from its spawn when the data gives no value ([`chase_fallback_m`]); `None` when its slice
    /// or its trees give one.
    pub fn chase_fallback(&self, archetype: u32, creature_type: Option<&str>) -> Option<f32> {
        (!self
            .chase_data
            .get(archetype as usize)
            .copied()
            .unwrap_or(false))
        .then(|| chase_fallback_m(creature_type))
    }

    /// The AI whose trees' `ExecuteSpawn` nodes spawn the creatures of `slice_vitals`.
    #[must_use]
    pub fn with_slice_vitals(mut self, slice_vitals: SliceVitals) -> Self {
        self.slice_vitals = slice_vitals;
        self
    }

    /// The vitals id of creature slice asset `asset`: what a summon of that slice spawns.
    pub fn summoned_vitals(&self, asset: &[u8; ASSET_ID_LEN]) -> Option<u32> {
        self.slice_vitals.get(asset).copied()
    }

    /// The AI data file of `client_build` in `dir` with the waypoint task names `tasks` (the
    /// level's and the scripts': [`with_task_names`]), tick rates `rates` and the looks `looks`.
    ///
    /// # Errors
    ///
    /// Returns [`ContainerError`] when the file is missing, from another build or bad.
    pub fn load<'a>(
        dir: &Path,
        client_build: &str,
        tasks: impl IntoIterator<Item = &'a str>,
        rates: AiRates,
        looks: LookAssets,
    ) -> Result<Self, ContainerError> {
        let data = with_task_names(AiData::load(dir, client_build)?, tasks);
        Ok(Self::new(data, rates, looks))
    }

    /// No data: every mob is without a tree.
    pub fn empty() -> Self {
        Self::new(AiData::default(), AiRates::default(), LookAssets::new())
    }

    /// The archetype of a mob of `vitals_id` shown with slice asset `asset`, else with its
    /// vitals id's look.
    pub fn archetype_for(&self, vitals_id: u32, asset: Option<[u8; ASSET_ID_LEN]>) -> Option<u32> {
        self.archetype_of(&asset.or_else(|| self.looks.get(&vitals_id).copied())?)
    }

    /// The data.
    pub fn data(&self) -> &AiData {
        self.runtime.data()
    }

    /// The tick rates.
    pub const fn rates(&self) -> &AiRates {
        &self.rates
    }

    /// The archetype index of the creature slice asset `asset`.
    pub fn archetype_of(&self, asset: &[u8; ASSET_ID_LEN]) -> Option<u32> {
        let data = self.data();
        let index = data
            .creatures
            .binary_search_by(|creature| creature.asset.cmp(asset))
            .ok()?;
        Some(data.creatures[index].archetype)
    }

    /// Whether archetype `archetype` has a tree Server.pak holds.
    pub fn has_tree(&self, archetype: u32) -> bool {
        self.arch(archetype)
            .is_some_and(|arch| arch.trees.iter().any(|tree| tree.tree.is_some()))
    }

    /// Archetype `archetype`.
    pub fn arch(&self, archetype: u32) -> Option<&Archetype> {
        self.data().archetypes.get(archetype as usize)
    }

    /// The config value of `key` of archetype `archetype` as a number.
    pub fn config_number(&self, archetype: u32, key: &str) -> Option<f64> {
        let key = self.data().key(key)?;
        self.arch(archetype)?.config_value(key)?.as_number()
    }

    /// The action of archetype `archetype`'s grid named `name`.
    fn named(&self, archetype: u32, name: Option<NameId>) -> Option<u32> {
        let data = self.data();
        data.grids
            .get(self.arch(archetype)?.grid? as usize)?
            .named(name?)
    }

    /// The actions of archetype `archetype`'s grid that the engine starts by their condition.
    fn engine_started(&self, archetype: u32) -> Vec<u32> {
        let data = self.data();
        self.arch(archetype)
            .and_then(|arch| data.grids.get(arch.grid? as usize))
            .map_or_else(Vec::new, |grid| {
                grid.actions
                    .iter()
                    .map(|(_, action)| *action)
                    .filter(|action| {
                        data.cage_actions
                            .get(*action as usize)
                            .is_some_and(|cage| cage.activation.is_some())
                    })
                    .collect()
            })
    }

    /// The action of archetype `archetype`'s grid whose alias is `alias`.
    fn aliased(&self, archetype: u32, alias: Option<NameId>) -> Option<u32> {
        let data = self.data();
        let alias = alias?;
        let grid = data.grids.get(self.arch(archetype)?.grid? as usize)?;
        grid.actions
            .iter()
            .find(|(name, _)| grid.alias(*name) == Some(alias))
            .map(|(_, action)| *action)
    }

    /// The CAGE action `action`.
    pub fn cage_action(&self, action: u32) -> Option<&CageAction> {
        self.data().cage_actions.get(action as usize)
    }

    /// The text of name `id`.
    pub fn name(&self, id: NameId) -> &str {
        self.data().name(id)
    }
}

/// How a patrol goes on after its last point.
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

/// The waypoints a patrolling mob walks (admin studio spec section 6.4): the `Waypoint_Pos` and
/// `Waypoint_Duration` its tree's Patrol sub-tree reads, advanced by its `SelectWaypointAction`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Route {
    /// The points, in order.
    pub points: Vec<Position>,
    /// The wait at each point (missing: no wait).
    pub waits_ms: Vec<u64>,
    /// What comes after the last point.
    pub mode: RouteMode,
    /// The task at each point (missing: none).
    pub tasks: Vec<Option<RouteTask>>,
}

/// What a mob does at a point of its route: a spawner waypoint's `AIWaypointComponent`
/// ([`nw_world::records::WaypointTask`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteTask {
    /// `Waypoint_Task`: the fragment it plays there.
    pub task: NameId,
    /// `Waypoint_Duration_Min` and `Waypoint_Duration_Max`, seconds (negative: none).
    pub duration_s: [f32; 2],
    /// `Waypoint_Align`.
    pub align: bool,
}

impl RouteTask {
    /// The `Waypoint_Duration` of one visit for the random draw `unit` in `[0, 1)`: uniform
    /// between the minimum and the maximum (Inferred: the engine's rule is Unknown; retail boars
    /// grazed for times between them), the minimum as it is when that is not above 0 (the
    /// Patrol sub-tree then plays the task once, `POI_Waypoint`).
    fn duration_s(&self, unit: f32) -> f32 {
        let [min, max] = self.duration_s;
        if min <= 0.0 {
            min
        } else {
            min + unit * (max - min).max(0.0)
        }
    }
}

/// Where a mob is on its route.
#[derive(Debug, Clone, PartialEq)]
struct RouteWalk {
    route: Arc<Route>,
    next: usize,
    backwards: bool,
    /// The `Waypoint_Duration` of this visit of a point with a task.
    task_s: f32,
}

impl RouteWalk {
    fn point(&self) -> Option<Position> {
        self.route.points.get(self.next).copied()
    }

    fn task(&self) -> Option<&RouteTask> {
        self.route.tasks.get(self.next)?.as_ref()
    }

    fn wait_s(&self) -> f32 {
        if self.task().is_some() {
            return self.task_s;
        }
        // A wait of minutes at most.
        #[allow(clippy::cast_precision_loss, reason = "a wait of minutes")]
        let ms = self.route.waits_ms.get(self.next).copied().unwrap_or(0) as f32;
        ms / MS_PER_S
    }

    /// Draws the duration of the visit of its point from `unit`.
    fn visit(&mut self, unit: f32) {
        self.task_s = self.task().map_or(0.0, |task| task.duration_s(unit));
    }

    /// Goes on to the point after `next`, as the mode says.
    fn advance(&mut self) {
        let last = self.route.points.len().saturating_sub(1);
        match self.route.mode {
            RouteMode::Loop => self.next = if self.next >= last { 0 } else { self.next + 1 },
            RouteMode::Once => self.next = (self.next + 1).min(last),
            RouteMode::BackAndForth => {
                if last == 0 {
                    return;
                }
                if (self.backwards && self.next == 0) || (!self.backwards && self.next >= last) {
                    self.backwards = !self.backwards;
                }
                self.next = if self.backwards {
                    self.next - 1
                } else {
                    self.next + 1
                };
            }
        }
    }
}

/// What a design file changes of a spawn (admin studio spec section 6.5): the slice config
/// values it overrides (they become the mob's own blackboard values), its attack cooldown, its
/// run speed, its sight, its route and its respawn time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpawnAi {
    /// Blackboard values over the slice's.
    pub facts: Vec<(KeyId, Value)>,
    /// The attack cooldown, seconds.
    pub attack_cooldown_s: Option<f64>,
    /// The top speed of its run (`RunToTarget`).
    pub run_speed_mps: Option<f32>,
    /// The distance of its ambient vision.
    pub sight_m: Option<f32>,
    /// The route it patrols.
    pub route: Option<Arc<Route>>,
    /// Its respawn time.
    pub respawn_ms: Option<u64>,
    /// The place it guards: the engine key `Guard_Objective_Pos` and the position type
    /// `GuardObjectivePosition` of its trees (an Outpost Rush summon guards its summoning
    /// circle, [`crate::world`]'s `outpost_rush_summons`); `None` for a mob with no objective.
    pub guard_objective: Option<[f32; 3]>,
}

/// The overrides of each vitals spawn of `world`, in spawn order; `None` keeps the slice's AI.
///
/// The design fields map onto the trees' own values:
/// - `behaviour`: `guard` sets `Enable_Wander` false (the Wander sub-tree runs only while it is
///   true, Observed); `idle` also sets `Is_Passive` true (a passive mob takes only its attackers
///   as targets, see [`TreeMind::perceive`]); `patrol` walks the spawn's path.
/// - `leash_radius_m`: `AmbientLeashDistance` and `CombatLeashDistance` (DistanceLeash reads
///   them, Observed).
/// - `wander_radius_m`: `WanderDistanceMax` (the Wander sub-tree picks points up to it).
/// - `attack_interval_ms`: the times of the `AttackCooldown` cooldown.
/// - `speed_mps`: the top speed of the `RunToTarget` navigation config.
/// - `sight_m`: the distance of the ambient vision profile.
/// - `respawn_ms`: the respawn time.
///
/// `wake_radius_m`, `wander_pause_ms` and `attack_reach_m` have no value in the trees: the wake
/// radius is the sim's, the wander pause and the attack bands are node literals (the boar's
/// `LimitFidgetAndWander` 2-7 s, its 3 m and 7 m bands). They are left out with a warning.
pub fn resolve_ai(world: &WorldFile, ai: &MobAi) -> Arc<[Option<SpawnAi>]> {
    let names = &ai.names;
    world
        .vitals_ai
        .iter()
        .map(|spawn| {
            if spawn.profile.is_none() && spawn.path.is_none() {
                return None;
            }
            let id = spawn.profile.as_deref().unwrap_or_default();
            let profile = spawn
                .profile
                .as_ref()
                .and_then(|id| world.ai_profiles.get(id))
                .cloned()
                .unwrap_or_default();
            warn_unmapped(id, &profile);
            let route = spawn
                .path
                .as_ref()
                .and_then(|id| world.paths.get(id))
                .map(|path| Arc::new(route_of(path, ai)));
            let mut facts = Vec::new();
            let mut set = |key: Option<KeyId>, value: Value| {
                if let Some(key) = key {
                    facts.push((key, value));
                }
            };
            match profile.behaviour {
                AiBehaviour::Wander | AiBehaviour::Patrol => {}
                AiBehaviour::Guard => set(names.enable_wander, Value::Bool(false)),
                AiBehaviour::Idle => {
                    set(names.enable_wander, Value::Bool(false));
                    set(names.is_passive, Value::Bool(true));
                }
            }
            if let Some(leash) = profile.leash_radius_m {
                set(names.ambient_leash, Value::Float(leash));
                set(names.combat_leash, Value::Float(leash));
            }
            if let Some(wander) = profile.wander_radius_m {
                set(names.wander_max, Value::Float(wander));
            }
            Some(SpawnAi {
                facts,
                // A cooldown of seconds.
                #[allow(clippy::cast_precision_loss, reason = "a cooldown of seconds")]
                attack_cooldown_s: profile
                    .attack_interval_ms
                    .map(|ms| ms as f64 / MS_PER_S_F64),
                run_speed_mps: profile.speed_mps,
                sight_m: profile.sight_m,
                route: route.filter(|_| {
                    matches!(profile.behaviour, AiBehaviour::Patrol | AiBehaviour::Wander)
                }),
                respawn_ms: profile.respawn_ms,
                guard_objective: None,
            })
        })
        .collect()
}

fn warn_unmapped(id: &str, profile: &AiProfile) {
    let unmapped = [
        ("wake_radius_m", profile.wake_radius_m.is_some()),
        ("wander_pause_ms", profile.wander_pause_ms.is_some()),
        ("attack_reach_m", profile.attack_reach_m.is_some()),
    ];
    for (field, set) in unmapped {
        if set {
            tracing::warn!(
                profile = id,
                field,
                "AI profile field has no value in the behavior trees: left out"
            );
        }
    }
}

/// `data` with the waypoint task names `tasks` (a level's: [`WorldFile::waypoint_tasks`]; the
/// scripts': `DungeonScripts::waypoint_tasks`). A waypoint task names the fragment that the
/// mob plays at its waypoint; the trees name only some of them (the open world: 34 of its 148
/// task names, 6,764 of its 18,264 waypoint tasks), and a route keeps a task only by its name id
/// ([`route_of`]). Retail `12-6_pillageship`: three beach boars play
/// `POI_Lay_Down_On_Side_LOOP`, a name of no tree.
pub fn with_task_names<'a>(mut data: AiData, tasks: impl IntoIterator<Item = &'a str>) -> AiData {
    for task in tasks {
        data.intern(task);
    }
    data
}

/// The overrides of a script mob that spawns at `at` with the waypoints `path` of its spawner
/// slice (points from the spawner's own place).
pub fn script_mob_ai(path: &PatrolPath, at: Position, ai: &MobAi) -> SpawnAi {
    let placed = PatrolPath {
        points: path
            .points
            .iter()
            .map(|point| [point[0] + at.x, point[1] + at.y, point[2] + at.z])
            .collect(),
        ..path.clone()
    };
    SpawnAi {
        route: Some(Arc::new(route_of(&placed, ai))),
        ..SpawnAi::default()
    }
}

fn route_of(path: &PatrolPath, ai: &MobAi) -> Route {
    Route {
        points: path
            .points
            .iter()
            .map(|&[x, y, z]| Position { x, y, z })
            .collect(),
        waits_ms: path.waits_ms.clone(),
        mode: match path.mode {
            PathMode::Loop => RouteMode::Loop,
            PathMode::BackAndForth => RouteMode::BackAndForth,
            PathMode::Once => RouteMode::Once,
        },
        tasks: path
            .tasks
            .iter()
            .map(|task| {
                let task = task.as_ref()?;
                Some(RouteTask {
                    task: ai.data().name_id(&task.task)?,
                    duration_s: task.duration_s,
                    align: task.align,
                })
            })
            .collect(),
    }
}

/// The height of a mob that moved to `to` on `ground`: the floor's under it, unless that lies
/// more than [`MAX_STEP_DOWN_M`] below the mob.
pub fn height_after_move(ground: &dyn Ground, to: Position) -> f32 {
    height_over(ground.floor_at(to.x, to.y, to.z), to.z)
}

/// [`height_after_move`] of a mob at height `z` over `floor`.
fn height_over(floor: Option<f32>, z: f32) -> f32 {
    match floor {
        Some(height) if z - height <= MAX_STEP_DOWN_M => height,
        _ => z,
    }
}

/// The end of the straight line from `from` to `to` on floor: at each step of [`FLOOR_STEP_M`]
/// the body stands as [`height_after_move`] puts it (on the floor near its height; where the
/// level has no height or its ground lies far below, it keeps its height), and the rise between
/// two steps must stay within
/// `max_slope_deg` (the slice's `m_maxSlope`; no limit without one) and below
/// [`MAX_STEP_RISE_M`], no wall may stand at knee height between two steps, and no step may go
/// from a place where a mob may stand to one where it may not ([`Ground::no_stand`]: a solid, a
/// navigation mark, deep water; a mob already in such a place walks out). `None` when the
/// line is too steep, climbs too high, meets a wall or a closed place, or when a body standing
/// on a floor would step off an edge more than
/// [`MAX_STEP_DOWN_M`] high: kept at its height it walked on in the air (live
/// login-world-entry-01: boars off the wreck's deck 3.7 to 12.1 m above the floor for up to 84
/// s; retail newchar: no body more than 3 m over the floor for longer than a jump). The server has no navigation mesh (the level `.bai` files are not read, their format
/// is Unknown): the level's floors (the terrain and the level collision's structures) stand in
/// for it (Inferred).
pub fn floor_line(
    ground: &dyn Ground,
    from: Position,
    to: Position,
    max_slope_deg: Option<f32>,
) -> Option<Position> {
    let length = from.horizontal_distance(to);
    // A line of tens of metres.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a line of tens of metres"
    )]
    let steps = (length / FLOOR_STEP_M).ceil().max(1.0) as u32;
    let max_rise = max_slope_deg.map(|slope| slope.to_radians().tan());
    let mut last = Position {
        z: height_after_move(ground, from),
        ..from
    };
    let mut free = !ground.no_stand(last);
    for step in 1..=steps {
        // A few hundred steps.
        #[allow(clippy::cast_precision_loss, reason = "a few hundred steps")]
        let share = step as f32 / steps as f32;
        let x = from.x + (to.x - from.x) * share;
        let y = from.y + (to.y - from.y) * share;
        let on_floor = ground
            .floor_at(last.x, last.y, last.z)
            .is_some_and(|floor| (last.z - floor).abs() <= ON_FLOOR_M);
        let below = ground.floor_at(x, y, last.z);
        if on_floor && below.is_some_and(|floor| last.z - floor > MAX_STEP_DOWN_M) {
            return None;
        }
        let z = height_over(below, last.z);
        let run = last.horizontal_distance(Position { x, y, z });
        if max_rise.is_some_and(|rise| run > 0.0 && (z - last.z).abs() / run > rise) {
            return None;
        }
        if z - last.z > MAX_STEP_RISE_M {
            return None;
        }
        // A wall at knee height stops the line, as it stops a pushed body (mob ground height
        // note: 13 of 14,548 live mob moves went into a solid, one 4 times next to a crate).
        let knee = |at: Position| Position {
            z: at.z + STEP_UP_M,
            ..at
        };
        let stand = Position { x, y, z };
        if ground.blocked(knee(last), knee(stand)) {
            return None;
        }
        let closed = ground.no_stand(stand);
        if free && closed {
            return None;
        }
        free = !closed;
        last = stand;
    }
    Some(last)
}

/// The end of a move from `from` toward `to` over the floors ([`floor_line`]); when the straight
/// line is blocked, the move of the same length along the nearest turn of [`DETOUR_TURNS_DEG`]
/// that is free.
fn floor_move(
    ground: &dyn Ground,
    from: Position,
    to: Position,
    max_slope_deg: Option<f32>,
) -> Option<Position> {
    floor_line(ground, from, to, max_slope_deg).or_else(|| {
        let length = from.horizontal_distance(to);
        let toward = yaw_toward(from, to);
        DETOUR_TURNS_DEG.iter().find_map(|degrees| {
            let side = ahead(from, turned(toward, *degrees), length);
            floor_line(ground, from, side, max_slope_deg)
        })
    })
}

/// Whether a mob at `from` reaches `to` over the floors: straight, or around one solid through a
/// side point along a turn of [`DETOUR_TURNS_DEG`] at most [`DETOUR_LEG_M`] away.
fn reaches(ground: &dyn Ground, from: Position, to: Position, max_slope_deg: Option<f32>) -> bool {
    floor_line(ground, from, to, max_slope_deg).is_some() || {
        let leg = from.horizontal_distance(to).min(DETOUR_LEG_M);
        let toward = yaw_toward(from, to);
        DETOUR_TURNS_DEG.iter().any(|degrees| {
            let side = ahead(from, turned(toward, *degrees), leg);
            floor_line(ground, from, side, max_slope_deg)
                .and_then(|side| floor_line(ground, side, to, max_slope_deg))
                .is_some()
        })
    }
}

/// Steers `mover` toward `goal` over `dt_s` along the way `path` when it serves that goal (a
/// way searched for a goal more than [`PATH_GOAL_SLACK_M`] away is dropped): to the way's next
/// place, then on. Whether it arrived at `goal`. At the end of a way that does not reach its
/// goal (the place nearest it) the mob stands.
fn steer_along(
    path: &mut Option<PathPlan>,
    mover: &mut Mover,
    goal: Position,
    steering: &Steering,
    dt_s: f32,
) -> bool {
    if path
        .as_ref()
        .is_some_and(|plan| plan.goal.horizontal_distance(goal) > PATH_GOAL_SLACK_M)
    {
        *path = None;
    }
    let Some(plan) = path.as_mut() else {
        return mover.steer_to(goal, steering, dt_s);
    };
    match plan.next_point() {
        Some(point) => {
            // The move's own arrival distance is for its goal, not for the places on the way.
            let to_point = Steering {
                arrival_m: 0.0,
                ..*steering
            };
            if mover.steer_to(point, &to_point, dt_s)
                || mover.position.horizontal_distance(point) <= WAYPOINT_REACHED_M
            {
                plan.advance();
            }
            false
        }
        None if plan.complete => {
            *path = None;
            mover.steer_to(goal, steering, dt_s)
        }
        None => {
            mover.speed_mps = 0.0;
            false
        }
    }
}

/// The yaw of `facing` turned by `degrees` (a positive angle turns west, as the yaw does).
fn turned(facing: f32, degrees: f64) -> f32 {
    // An angle of a turn.
    #[allow(clippy::cast_possible_truncation, reason = "an angle of a turn")]
    let radians = degrees.to_radians() as f32;
    facing + radians
}

/// The clips of `windows`, each written once: a clip written again in the same entry, with the
/// same times and parameters, is the same hit or shot (Observed: the damned bowman's
/// `Fire_Bow_SpreadShot` holds three equal `CAGE-RangedAttack` clips at 1.47 s and hit the
/// retail player once, MSQ13-lv56 ledger 770,029 ms; the damned bear's `Attack_Lunge_L_Quad`
/// holds two equal `CAGE-Damage` clips and hit once, 1,342,703 ms). The undead pistoleer's
/// `Shoot_Pistol_BreakGuard` holds five equal shots at 0.93 s, which hit a live player five times
/// in one millisecond (sim log 2026-11-12 14:32:02.032).
fn once_each(windows: &[ProcWindow]) -> impl Iterator<Item = &ProcWindow> {
    windows
        .iter()
        .enumerate()
        .filter(|(index, window)| !windows[..*index].contains(window))
        .map(|(_, window)| window)
}

/// The point `distance` from `from` along yaw `yaw`.
fn ahead(from: Position, yaw: f32, distance: f32) -> Position {
    Position {
        x: from.x - yaw.sin() * distance,
        y: from.y + yaw.cos() * distance,
        z: from.z,
    }
}

/// How a mob moves now: its tree's navigation or strafe.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Motion {
    slot: Slot,
    kind: MotionKind,
    steering: Steering,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MotionKind {
    /// To a point.
    To(Position),
    /// To the target: to a point the archetype's navigation config `config` picks around it,
    /// picked again when the target moved more than the config's repath distance from where it
    /// was.
    Follow {
        goal: Option<(Position, Position)>,
        config: Option<usize>,
    },
    /// Around the target until `until_ms`.
    Strafe {
        ideal_m: (f32, f32),
        left: bool,
        until_ms: u64,
    },
    /// Away from its target, else from where it was last hit, until that point lies beyond the
    /// distance of its ambient vision: a `Flee` navigation without a picked point (Observed:
    /// MSQ13-lv56 ledger, 1,388,400 to 1,694,300 ms: nine retail turkeys ran `Move_Combat` from
    /// a player 3.4 to 15.3 m away, for 8.5 to 53.5 m at 1.7 to 4.2 m/s, six of them within 70
    /// degrees of straight away, and eight ended with the player beyond the turkey's 15 m
    /// ambient vision; Inferred: the end rule, as the engine's rule is Unknown).
    Away,
}

/// The CAGE action a mob plays, as its replicated animation shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    /// The CAGE action (index in the data's `cage_actions`).
    pub action: u32,
    /// When it started (sim ms).
    pub started_ms: u64,
    /// Counts the starts, so that a start of the same action again is a new entry.
    pub serial: u32,
}

/// The time of a damage window that waits for its run to get to the target: no time comes to
/// it until the run arrives (the boar's `Attack_Charge`: a `CAGE-Damage` clip from 0 s without
/// an end on a looping fragment, played by a `NavigationAction`; live 2026-11-10 and before, a
/// charge struck nobody).
const ON_CONTACT_MS: u64 = u64::MAX;

/// A moment of the action a mob plays: a damage window (`CAGE-Damage` of its fragment entry),
/// the moment its `CAGE-RangedAttack` clip fires, or the moment its `CAGE-CastSpell` clip casts.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Window {
    at_ms: u64,
    act: WindowAct,
}

/// What a [`Window`] does when it comes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum WindowAct {
    /// It strikes the target with its damage row.
    Strike {
        row: NameId,
        reach: MobReach,
        ranged: Option<RangedWindow>,
    },
    /// It casts its spell.
    Cast(CastWindow),
}

/// The parameters of a `CAGE-CastSpell` clip.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CastWindow {
    spell: NameId,
    /// `AIAimAtTarget`.
    aim: bool,
    /// `ProjectileSpeed`, m/s.
    speed_mps: f32,
}

/// A spell a mob's `CAGE-CastSpell` clip casts ([`nw_content::combat::MobSpell`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SpellCast {
    /// `SpellName`.
    pub spell: String,
    /// The mob's target and where it was, when it has one.
    pub target: Option<(u64, Position)>,
    /// `AIAimAtTarget`: it opens on the target, not on the mob.
    pub aim: bool,
    /// `ProjectileSpeed`: how fast a spell that flies gets there, m/s.
    pub speed_mps: f32,
    /// The unique id of the creature's damage table, for the spell's damage row.
    pub table: Option<String>,
}

/// The parameters of a `CAGE-RangedAttack` clip.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RangedWindow {
    attack: NameId,
    slice: Option<NameId>,
    lead: bool,
    lead_max_rad: f32,
    speed_mps: Option<f32>,
}

/// The projectile of a `CAGE-RangedAttack` clip, which the mob fires at its target.
#[derive(Debug, Clone, PartialEq)]
pub struct RangedClip {
    /// `RangedAttackName`: the attack of the main-hand weapon's ranged profile.
    pub attack: String,
    /// `SliceOverride`: the projectile slice (lower-case pak path); `None` when the clip names
    /// none.
    pub slice: Option<String>,
    /// `AILeadTarget`: it aims where the target will be when it gets there.
    pub lead: bool,
    /// `AILeadTargetMaxAngle`: how far from the target the lead may turn the aim (radians).
    pub lead_max_rad: f32,
    /// `ProjectileSpeed` (m/s): the speed its aim leads with; `None` when the clip names none.
    pub speed_mps: Option<f32>,
}

/// The hit of a damage window: its damage table row and how far it reaches.
#[derive(Debug, Clone, PartialEq)]
pub struct Strike {
    /// The unique id of the creature's damage table (`#DamageData` header).
    pub table: String,
    /// The `DamageTableRow` of the window.
    pub row: String,
    /// The item of the creature's main hand, whose weapon row a block's stamina comes from.
    pub weapon: Option<String>,
    /// How the swing reaches its target (a ranged strike lands by its projectile instead).
    pub reach: MobReach,
    /// The projectile of a ranged clip; `None` for a damage window.
    pub ranged: Option<RangedClip>,
}

/// A hit of a damage window on the mob's target.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowHit {
    /// The target.
    pub target: u64,
    /// Where the target was.
    pub target_at: Position,
    /// The hit.
    pub strike: Strike,
}

/// What a mob's think step did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Thought {
    /// The hits whose damage windows came.
    pub hits: Vec<WindowHit>,
    /// The spells whose clips came.
    pub casts: Vec<SpellCast>,
    /// The target it took this step, when it had none.
    pub acquired: Option<u64>,
    /// A reset that heals ended: refill its health (Inferred: the `ResetAI` row of
    /// `javelindata_statuseffects_ai` has health columns that `ResetAINoHeal` lacks).
    pub healed: bool,
    /// Its tree ran this step (its tick was due).
    pub ticked: bool,
    /// The encounter events its tree sent (`EncounterEventDecorator`), for the encounter that
    /// spawned it (dungeon scripts).
    pub encounter_events: Vec<NameId>,
    /// The mobs its tree spawned (`ExecuteSpawn`).
    pub summons: Vec<Summon>,
    /// The slice triggers its tree and its CAGE actions ran (`TriggerEntity`): each goes to
    /// its clients as a 3135 (oquests triggers note, section 1.3).
    pub triggers: Vec<SliceTriggerRef>,
    /// The triggers its tree ran on other mobs (`TriggerRemoteEntity`): (mob, trigger name).
    pub remote_triggers: Vec<(u64, NameId)>,
    /// The facts its tree set on other mobs (`SetBlackboardFactOnRemoteEntityAction`).
    pub remote_facts: Vec<RemoteFact>,
    /// The orders its tree gave other mobs (`AssignOrder`): (mob, order CRC32).
    pub orders: Vec<(u64, u32)>,
}

/// A fact a mob's tree sets on another mob's tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteFact {
    /// The mob.
    pub mob: u64,
    /// The fact.
    pub key: KeyId,
    /// Its value.
    pub value: Value,
    /// `OverwriteTargetFact`: it replaces a value the mob already has (else only a fact the mob
    /// does not have is set; Inferred from the name).
    pub overwrite: bool,
}

/// A mob whose tree gave it networked tags (`UpdateNWTags`), as the other trees see it in one
/// tick. The slices' own `NWTags` are not in the AI data (Unknown), so only these tags exist.
#[derive(Debug, Clone, PartialEq)]
pub struct TaggedMob {
    /// The mob.
    pub entity: u64,
    /// Where it is.
    pub position: Position,
    /// Its tags.
    pub tags: Vec<NameId>,
}

/// The status effects the sim holds on a mob (put there by players, by its tree and by the
/// instance's mutation), as its tree and its body read them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeldEffects {
    /// The effect ids.
    pub effects: Vec<String>,
    /// Their `EffectCategories` (lower case).
    pub categories: Vec<String>,
    /// What they keep the mob from doing.
    pub control: Control,
    /// The sum of their `StaminaRate`: its guard's regen is `1 +` this times the plain one
    /// ([`MobGuard::regen_with`]).
    pub stamina_rate: f32,
    /// The sum of the `StaggerDamageReduction` of their equip abilities: the share of a hit's
    /// stagger its grit does not take ([`MobGuard::grit_loss`]).
    pub stagger_reduction: f32,
}

/// The other mobs a tree's remote actions reach in one step: the mob's own entity and the
/// tagged mobs.
#[derive(Debug, Clone, Copy)]
struct Others<'a> {
    entity: u64,
    tagged: &'a [TaggedMob],
}

impl Others<'_> {
    /// No other mob (a step of the engine's own actions, the reaction and the death).
    const NONE: Others<'static> = Others {
        entity: 0,
        tagged: &[],
    };
}

/// A tree's `ExecuteSpawn`: `count` mobs of creature slice `asset` at `at` (the node's
/// `TargetPosBlackboardKey`), else at the summoner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summon {
    /// The creature slice asset (`Slice`).
    pub asset: [u8; ASSET_ID_LEN],
    /// `Count`.
    pub count: u32,
    /// Where they spawn; `None`: at the summoner.
    pub at: Option<Position>,
}

/// The surroundings of a mob in one step.
pub struct Surroundings<'a> {
    /// The sim time.
    pub now_ms: u64,
    /// The players it may take as targets.
    pub seen: &'a [Seen],
    /// The level's ground: the terrain and the level collision's structures.
    pub ground: &'a (dyn Ground + Sync),
    /// Its health, 0 to 1.
    pub health_share: f32,
    /// Its own entity.
    pub entity: u64,
    /// The mobs of the sim with networked tags.
    pub tagged: &'a [TaggedMob],
    /// The status effects the sim holds on it.
    pub held: Option<&'a HeldEffects>,
}

/// One mob's mind: its tree's brain and what the engine keeps for it.
#[derive(Debug, Clone)]
pub struct TreeMind {
    brain: Brain,
    state: MindState,
}

/// What the engine keeps for a mob's tree: the parts its world view reads, apart from the brain
/// so that the brain can think while the view reads them.
#[derive(Debug, Clone)]
struct MindState {
    archetype: u32,
    spawn: Position,
    yaw: f32,
    /// Its facing is its own: it turned or moved since its spawn.
    turned: bool,
    speed_mps: f32,
    motion: Option<Motion>,
    shown: Option<Shown>,
    serial: u32,
    task_slot: Option<Slot>,
    /// The turn of a facing task: its start, its end and its start yaw.
    turn: Option<(u64, u64, f32)>,
    windows: Vec<Window>,
    /// The `Homing` clips of the attack it plays.
    homing: Vec<HomingRun>,
    /// The way it walks around solids to its motion's goal ([`crate::mob_path`]).
    path: Option<PathPlan>,
    /// When it may search for a way again.
    next_path_ms: u64,
    /// Its places of the way out of a fight, for its way home ([`TRAIL_STEP_M`]).
    trail: Vec<Position>,
    target: Option<u64>,
    target_at: Option<Position>,
    threat: Threat,
    locked: bool,
    manual: Option<Position>,
    last_damage: Option<Position>,
    reachable_ms: Option<u64>,
    reach: Option<ReachCheck>,
    statuses: Vec<NameId>,
    /// While a hit reaction plays: until when.
    reaction_until_ms: Option<u64>,
    route: Option<RouteWalk>,
    next_tick_ms: u64,
    last_step_ms: Option<u64>,
    picks: Rng48,
    run_speed_mps: Option<f32>,
    sight_m: Option<f32>,
    /// Its networked tags (`UpdateNWTags`).
    nw_tags: Vec<NameId>,
    /// The order another mob gave it (`AssignOrder`), the CRC32 the engine key `AI_Order`
    /// holds until it completes it (`CompleteOrder`; Inferred from the trees, which test
    /// `AI_Order` against the CRC of the order an `AssignOrder` gives).
    order: Option<u32>,
    /// The action target trackers it runs, the innermost last (`ActionTargetTrackerDecorator`).
    trackers: Vec<NodeId>,
    /// The status effects its tree put on it (`true`) or took off it since the last
    /// [`TreeMind::take_status_changes`] (`StatusEffectDecorator`).
    status_changes: Vec<(NameId, bool)>,
    /// The actions the engine runs by their condition.
    engine_runs: Vec<EngineRun>,
    /// The action of the reaction that plays (the grid's `Reaction`, or a broken guard's).
    reaction_action: Option<u32>,
    /// The stagger that plays, as its replicated animation shows it, and the fragment its
    /// `ChangeActionFragment` chose by the side of the hit ([`TreeMind::stagger`]).
    staggered: Option<(Shown, NameId)>,
    /// Its guard, for a mob type that can block.
    guard: Option<GuardState>,
    /// Its block is up (after its last step).
    blocking: bool,
    /// The node and CAGE task of the action or move it started last ([`TreeMind::log_decision`]).
    decision: Option<(NodeId, NameId)>,
    /// Why its target changed in its last step ([`TargetReason`]).
    target_reason: Option<TargetReason>,
    /// Its chase distance where the data gives none ([`MobAi::chase_fallback`]): the engine key
    /// `CombatLeashDistance`.
    chase_fallback_m: Option<f32>,
    /// The place it guards ([`SpawnAi::guard_objective`]).
    guard_objective: Option<[f32; 3]>,
    /// The players it never takes as targets ([`TreeMind::spare`]).
    spared: Vec<u64>,
}

/// Why a mob's target changed, for the log of its decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetReason {
    /// The target left the players the mob may see (the wake radius), or the mob walks home in
    /// a leash reset.
    OutOfSight,
    /// It took the player that put the most threat on it.
    Threat,
    /// It saw a player in its ambient vision.
    Sight,
}

/// An action the engine runs by its activation condition (`Block`, `BlockBroken`,
/// `Attack_Cooldown` of the mob block lists), and which of its tag activities are on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EngineRun {
    action: u32,
    tags_on: Vec<bool>,
}

/// A mob's guard stamina.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GuardState {
    rules: MobGuard,
    stamina: f32,
    /// When its regen last ran.
    regen_ms: Option<u64>,
}

/// What a hit did to a mob whose grit is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GritHit {
    /// The stamina took the hit.
    Held,
    /// The hit left no stamina: the stagger started.
    Staggered,
}

/// What a blocked hit did to a mob's guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardHit {
    /// The guard holds.
    Held,
    /// The hit took the last stamina: the guard broke.
    Broken,
}

/// One change to the engine runs of a mob, found before any is made.
enum EngineStep {
    /// The action starts with these tag activities on.
    Start(u32, Vec<bool>),
    /// The action ends with these tag activities on; `true`: its stamina reset runs.
    Stop(u32, Vec<bool>, bool),
    /// A tag activity of a running action turns on or off.
    Toggle(u32, usize, bool),
}

/// The last reach check of a mob: where it and its target stood, and the answer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ReachCheck {
    from: Position,
    to: Position,
    reachable: bool,
    /// How far the mob and its target may move before the answer is checked again
    /// ([`REACH_RECHECK_M`], or [`PATH_RECHECK_M`] for an answer of a way search).
    recheck_m: f32,
}

impl ReachCheck {
    /// The answer for a mob at `from` and its target at `to`, while both stay within the
    /// check's `recheck_m` of the places of the check.
    fn answer(self, from: Position, to: Position) -> Option<bool> {
        let near = |checked: Position, now: Position| {
            checked.horizontal_distance(now) < self.recheck_m
                && (checked.z - now.z).abs() < self.recheck_m
        };
        (near(self.from, from) && near(self.to, to)).then_some(self.reachable)
    }
}

/// What a mob perceives in one step.
struct Perceived {
    target: Option<TargetView>,
    nearest_m: f32,
    candidates: u32,
}

#[derive(Debug, Clone, Copy)]
struct TargetView {
    position: Position,
    distance_m: f32,
    angle_deg: f32,
    visible: bool,
    reachable: bool,
}

impl MindState {
    /// The vision of the archetype: ambient without a target, combat with one (the slice's
    /// `m_ambientVisionProfile` and `m_combatVisionProfile`; `-1` is none). A design override of
    /// the sight replaces the ambient distance.
    fn vision(&self, ai: &MobAi, combat: bool) -> Option<(f32, f32)> {
        let arch = ai.arch(self.archetype)?;
        let profile = if combat {
            arch.combat_vision.or(arch.ambient_vision)
        } else {
            arch.ambient_vision
        }?;
        let distance = if combat {
            profile.distance_m
        } else {
            self.sight_m.unwrap_or(profile.distance_m)
        };
        (profile.angle_deg >= 0.0 && distance >= 0.0).then_some((profile.angle_deg, distance))
    }

    fn stop_moving(&mut self) {
        self.motion = None;
        self.speed_mps = 0.0;
    }

    /// Whether a way search reaches `to` from `at` (its answer and how long it holds): a way
    /// that ends at the target serves the mob's next move to it. A mob whose last search is not
    /// [`PATH_RETRY_MS`] old keeps its last answer.
    fn search_reach(
        &mut self,
        ground: &dyn Ground,
        at: Position,
        to: Position,
        max_slope: Option<f32>,
        now_ms: u64,
    ) -> (bool, f32) {
        if now_ms < self.next_path_ms {
            return (
                self.reach.is_some_and(|check| check.reachable),
                REACH_RECHECK_M,
            );
        }
        self.next_path_ms = now_ms + PATH_RETRY_MS;
        let plan = find_path(ground, at, to, max_slope).filter(|plan| plan.complete);
        let reachable = plan.is_some();
        if reachable {
            self.path = plan;
        }
        (reachable, PATH_RECHECK_M)
    }

    /// A way from `at` to `goal` when the straight move failed: a way search, or for a mob on
    /// its way home with no way found, its trail of the way out walked back. `false` when no
    /// way is found or the last search is not [`PATH_RETRY_MS`] old.
    fn plan_way(
        &mut self,
        ai: &MobAi,
        ground: &dyn Ground,
        at: Position,
        goal: Position,
        now_ms: u64,
    ) -> bool {
        if now_ms < self.next_path_ms {
            return false;
        }
        self.next_path_ms = now_ms + PATH_RETRY_MS;
        let max_slope = ai.arch(self.archetype).and_then(|arch| arch.max_slope_deg);
        let plan = find_path(ground, at, goal, max_slope);
        let plan = if plan.as_ref().is_some_and(|plan| plan.complete)
            || !self.resetting(ai)
            || self.trail.is_empty()
        {
            plan
        } else {
            let mut points: Vec<Position> = self.trail.iter().rev().copied().collect();
            points.push(goal);
            Some(PathPlan {
                goal,
                points,
                next: 0,
                complete: true,
            })
        };
        self.path = plan;
        self.path.is_some()
    }

    /// Keeps the mob's place `at` in its trail while it fights (each [`TRAIL_STEP_M`]).
    fn keep_trail(&mut self, ai: &MobAi, at: Position) {
        if self.target.is_none() || self.resetting(ai) {
            return;
        }
        if self
            .trail
            .last()
            .is_some_and(|last| last.horizontal_distance(at) < TRAIL_STEP_M)
        {
            return;
        }
        self.trail.push(at);
        if self.trail.len() > TRAIL_MAX {
            let mut keep = false;
            self.trail.retain(|_| {
                keep = !keep;
                keep
            });
        }
    }

    /// The attack it plays ends: its damage windows and its homing clips with it.
    fn end_attack(&mut self) {
        self.windows.clear();
        self.homing.clear();
    }

    /// One step at `now_ms` of the homing clips of its attack, for a mob at `at` whose target
    /// stands at `target`; a `rooted` mob only turns. A mob that its tree moves is not homed
    /// (the move steers it).
    fn home_in(
        &mut self,
        ai: &MobAi,
        at: &mut Position,
        target: Option<Position>,
        ground: &dyn Ground,
        now_ms: u64,
        rooted: bool,
    ) {
        if self.homing.is_empty() || self.motion.is_some() {
            return;
        }
        let max_slope = ai.arch(self.archetype).and_then(|arch| arch.max_slope_deg);
        let mut body = Body {
            position: *at,
            yaw: self.yaw,
        };
        mob_homing::step(
            &mut self.homing,
            now_ms,
            &mut body,
            target,
            rooted,
            |from, to| {
                floor_line(ground, from, to, max_slope).map(|end| Position {
                    z: height_after_move(ground, end),
                    ..end
                })
            },
        );
        if body
            != (Body {
                position: *at,
                yaw: self.yaw,
            })
        {
            *at = body.position;
            self.yaw = body.yaw;
            self.turned = true;
        }
    }

    fn motion_goal(&self) -> Option<Position> {
        match self.motion?.kind {
            MotionKind::To(goal) => Some(goal),
            MotionKind::Follow { goal, .. } => goal.map(|(point, _)| point),
            MotionKind::Strafe { .. } | MotionKind::Away => None,
        }
    }

    /// Where a mob at `at` that flees its target `target`, else where it was last hit, steers
    /// to ([`MotionKind::Away`]): [`FLEE_AHEAD_M`] on, straight away from it. `None` when it has
    /// fled past the distance of its ambient vision (none: at once), or has nothing to flee.
    fn flight_goal(
        &self,
        ai: &MobAi,
        at: Position,
        target: Option<TargetView>,
    ) -> Option<Position> {
        let from = target.map(|target| target.position).or(self.last_damage)?;
        let far_m = self.vision(ai, false).map_or(0.0, |(_, distance)| distance);
        (at.horizontal_distance(from) < far_m)
            .then(|| ahead(at, yaw_toward(from, at), FLEE_AHEAD_M))
    }

    /// Whether the mob is in a leash reset (it walks home and sees no player).
    fn resetting(&self, ai: &MobAi) -> bool {
        self.statuses.iter().any(|status| {
            Some(*status) == ai.names.reset || Some(*status) == ai.names.reset_no_heal
        })
    }

    /// Picks the target (the targeting profile's curve formula is Unknown, study section 3.2;
    /// this rule is Inferred): a mob in a leash reset has none (it walks home); a target is kept
    /// while the mob may see it (the sim's wake radius), and while locked
    /// (`LockTargetSelection`); else the top threat that it may see; else, for a mob that is not
    /// `passive`, the nearest player in its ambient vision cone (the slice profile: the angle
    /// from its facing to each side, and its distance).
    ///
    /// The combat vision distance does not end a chase (Observed, `aggro/chase.py` over the 19
    /// MSQ ledgers, 2026-11-12: of 147 retail mobs that walked home alive, half still ran at the
    /// player when it was 28.6 m or more away, a quarter at 40.1 m or more, for example a
    /// skeleton at 70.5 m in MSQ13-lv56 before its walk home at 1,211,901 ms and a boar at 56 m
    /// in MSQ5-lv25 before 1,343,598 ms; both slices have a 30 m combat vision). The walk home
    /// came 60 to 80 m from the mob's first place in 76 of the 147 (median 66.7 m): the tree's
    /// `DistanceLeash` on `CombatLeashDistance` (70 m on most slices) ends the chase, so the
    /// held target is kept to it.
    ///
    /// The cone's angle is the slice's `m_visionAngle` from the facing to each side, not the
    /// cone's full width (Observed, `aggro/hazard.py`, 2026-11-12: the mobs of a 75 deg / 12 m
    /// profile alerted 3.8 to 12 m away at 37.5 to 75 deg from their facing 7 times in 193 s of
    /// exposure there, at 75 to 120 deg once in 269 s and at 120 to 180 deg never in 318 s (with
    /// the full-width reading, a cone of 37.5 deg to each side, the 7 could not be); the
    /// 50 deg / 18 m mobs alerted
    /// at 37.5 to 60 deg 3 times and never at 60 to 180 deg within 18 m).
    fn perceive(
        &mut self,
        ai: &MobAi,
        at: Position,
        passive: bool,
        around: &Surroundings<'_>,
    ) -> Perceived {
        let combat = self.vision(ai, true);
        let open: Vec<Seen> = around
            .seen
            .iter()
            .filter(|player| !self.spared.contains(&player.entity))
            .copied()
            .collect();
        let seen: &[Seen] = if self.resetting(ai) { &[] } else { &open };
        let near = |entity: u64| seen.iter().find(|player| player.entity == entity).copied();
        let had = self.target;
        if self.target.is_some_and(|target| near(target).is_none()) {
            self.target = None;
            self.target_reason = Some(TargetReason::OutOfSight);
        }
        if !(self.locked && self.target.is_some()) {
            while let Some(top) = self.threat.top() {
                if near(top).is_some() {
                    if self.target != Some(top) {
                        self.target_reason = Some(TargetReason::Threat);
                    }
                    self.target = Some(top);
                    break;
                }
                self.threat.forget(top);
            }
        }
        if self.target.is_none()
            && !passive
            && let Some((angle_deg, distance_m)) = self.vision(ai, false)
        {
            self.target = seen
                .iter()
                .filter(|player| {
                    let distance = player.position.horizontal_distance(at);
                    let bearing = yaw_gap(self.yaw, yaw_toward(at, player.position)).abs();
                    distance <= IMMEDIATE_VICINITY_M
                        || (distance <= distance_m && bearing.to_degrees() <= angle_deg)
                })
                .min_by(|a, b| {
                    let da = a.position.horizontal_distance(at);
                    let db = b.position.horizontal_distance(at);
                    da.total_cmp(&db).then(a.entity.cmp(&b.entity))
                })
                .map(|player| player.entity);
            if self.target.is_some() {
                self.target_reason = Some(TargetReason::Sight);
            }
        }
        if self.target.is_some() && had.is_none() {
            // The fight takes over from the damage reaction (Inferred).
            self.last_damage = None;
        }
        if self.target != had {
            // The time since the target was reachable counts from its pick (module notes).
            // Counted from the sim's start, a target out of reach at its first sight sent the
            // mob home at once: `UnreachableTargetLeash` (node 239, `Time_Since_Target_Reachable
            // > 6`) took the risen of archetype 530 home 4.47 m from its spawn by `LeashOrDie`
            // (node 219) again and again, `since_reachable_ms=None` (live 2026-11-12 13:23:58).
            self.reachable_ms = self.target.map(|_| around.now_ms);
        }
        let max_slope = ai.arch(self.archetype).and_then(|arch| arch.max_slope_deg);
        let target = self.target.and_then(near).map(|player| {
            let position = self.manual.unwrap_or(player.position);
            let distance_m = position.horizontal_distance(at);
            let gap = yaw_gap(self.yaw, yaw_toward(at, position));
            let reachable = self
                .reach
                .and_then(|check| check.answer(at, position))
                .unwrap_or_else(|| {
                    let (reachable, recheck_m) = if reaches(around.ground, at, position, max_slope)
                    {
                        (true, REACH_RECHECK_M)
                    } else {
                        self.search_reach(around.ground, at, position, max_slope, around.now_ms)
                    };
                    self.reach = Some(ReachCheck {
                        from: at,
                        to: position,
                        reachable,
                        recheck_m,
                    });
                    reachable
                });
            if reachable {
                self.reachable_ms = Some(around.now_ms);
            }
            TargetView {
                position,
                distance_m,
                angle_deg: gap.to_degrees(),
                visible: combat.is_none_or(|(_, distance)| distance_m <= distance),
                reachable,
            }
        });
        self.target_at = target.map(|target| target.position);
        if target.is_none() {
            self.reachable_ms = None;
        }
        let nearest_m = around
            .seen
            .iter()
            .map(|player| player.position.horizontal_distance(at))
            .fold(f32::INFINITY, f32::min);
        let candidates = seen
            .iter()
            .filter(|player| {
                combat
                    .is_none_or(|(_, distance)| player.position.horizontal_distance(at) <= distance)
            })
            .count();
        Perceived {
            target,
            nearest_m,
            candidates: u32::try_from(candidates).unwrap_or(u32::MAX),
        }
    }

    /// The world view of this state.
    fn view<'a>(&'a self, ai: &'a MobAi, at: Position, now_ms: u64, passive: bool) -> View<'a> {
        View {
            ai,
            state: self,
            at,
            now_ms,
            passive,
            target: None,
            candidates: 0,
            health_share: 1.0,
            ground: None,
            picks: Cell::new(self.picks),
            others: Others::NONE,
            seen: &[],
            held: None,
        }
    }

    /// The tag filter of the innermost action target tracker it runs: the tracker's
    /// `TargetTagFilter`, else that of its `ActionTarget`.
    fn tracker_filter(&self, ai: &MobAi) -> Option<NameId> {
        let node = *self.trackers.last()?;
        let NodeKind::ActionTargetTracker { params, .. } = &ai.data().node(node).kind else {
            return None;
        };
        [params.tag_filter, params.target.tag_filter]
            .into_iter()
            .find(|tag| !ai.name(*tag).is_empty())
    }

    /// The other mobs a remote action of the mob at `at` acts on: the tagged mobs of `others`
    /// (but itself) with the tag of `target`'s `TargetTagFilter`, else of the innermost tracker's,
    /// nearest first and at most `m_maxTargets` of them when set; target `index` of them inside a
    /// `ForEachTargetDecorator`, else all of them with `all`, else the nearest. Inferred: the
    /// targeting profile's curves are Unknown, so the nearest go first; an action with no tag
    /// takes the mob's players, on which the sim runs no remote action (a player's slice
    /// triggers and blackboard are Unknown), so it has none.
    fn remote_targets(
        &self,
        ai: &MobAi,
        at: Position,
        others: Others<'_>,
        target: Option<&ActionTarget>,
        all: bool,
        index: Option<u32>,
    ) -> Vec<u64> {
        let own = target
            .map(|target| target.tag_filter)
            .filter(|tag| !ai.name(*tag).is_empty());
        let Some(tag) = own.or_else(|| self.tracker_filter(ai)) else {
            return Vec::new();
        };
        let mut found: Vec<&TaggedMob> = others
            .tagged
            .iter()
            .filter(|mob| mob.entity != others.entity && mob.tags.contains(&tag))
            .collect();
        found.sort_by(|a, b| {
            let (da, db) = (
                a.position.horizontal_distance(at),
                b.position.horizontal_distance(at),
            );
            da.total_cmp(&db).then(a.entity.cmp(&b.entity))
        });
        let max = target
            .and_then(|target| target.limits.max_targets.number(&NoBoard))
            .filter(|max| *max > 0.0);
        if let Some(max) = max {
            // A handful of targets.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a handful of targets"
            )]
            found.truncate(max as usize);
        }
        let entities = found.into_iter().map(|mob| mob.entity);
        match index {
            Some(index) => entities.skip(index as usize).take(1).collect(),
            None if all => entities.collect(),
            None => entities.take(1).collect(),
        }
    }

    /// The tags after `operations` (`UpdateNWTags`: add or remove each tag); `None` when a
    /// strict operation changes nothing (Inferred from `PerformStrictOperation`: a strict add of a
    /// tag the mob has, or a strict remove of one it lacks, fails the node).
    fn tags_after<'a>(
        &self,
        operations: impl IntoIterator<Item = &'a TagOperation>,
    ) -> Option<Vec<NameId>> {
        let mut tags = self.nw_tags.clone();
        for operation in operations {
            let has = tags.contains(&operation.tag);
            if operation.strict && has == operation.add {
                return None;
            }
            if operation.add && !has {
                tags.push(operation.tag);
            } else if !operation.add {
                tags.retain(|tag| *tag != operation.tag);
            }
        }
        Some(tags)
    }

    /// Fires the damage, ranged and spell windows of mob `entity` that are due at `now_ms`.
    fn fire_windows(&mut self, ai: &MobAi, entity: u64, now_ms: u64, thought: &mut Thought) {
        let (due, later): (Vec<Window>, Vec<Window>) = self
            .windows
            .iter()
            .partition(|window| now_ms >= window.at_ms);
        self.windows = later;
        let Some(arch) = ai.arch(self.archetype) else {
            return;
        };
        let table = arch.damage_table_id.map(|table| ai.name(table).to_owned());
        let target = self.target.zip(self.target_at);
        for window in due {
            match window.act {
                WindowAct::Cast(cast) => thought.casts.push(SpellCast {
                    spell: ai.name(cast.spell).to_owned(),
                    target,
                    aim: cast.aim,
                    speed_mps: cast.speed_mps,
                    table: table.clone(),
                }),
                WindowAct::Strike { row, reach, ranged } => {
                    let (Some((target, target_at)), Some(table)) = (target, &table) else {
                        tracing::debug!(
                            mob = entity,
                            attack = ai.name(row),
                            reason = if target.is_none() {
                                "no target"
                            } else {
                                "no damage table"
                            },
                            "mob attack not started"
                        );
                        continue;
                    };
                    thought.hits.push(WindowHit {
                        target,
                        target_at,
                        strike: Strike {
                            table: table.clone(),
                            row: ai.name(row).to_owned(),
                            weapon: arch.main_hand_item.map(|item| ai.name(item).to_owned()),
                            reach,
                            ranged: ranged.map(|ranged| RangedClip {
                                attack: ai.name(ranged.attack).to_owned(),
                                slice: ranged.slice.map(|slice| ai.name(slice).to_lowercase()),
                                lead: ranged.lead,
                                lead_max_rad: ranged.lead_max_rad,
                                speed_mps: ranged.speed_mps,
                            }),
                        },
                    });
                }
            }
        }
    }

    /// The task `task` of `slot` starts playing.
    fn show(&mut self, ai: &MobAi, slot: Slot, task: NameId, now_ms: u64) {
        let data = ai.data();
        let Some(action) = ai
            .arch(self.archetype)
            .and_then(|arch| data.grids.get(arch.grid? as usize))
            .and_then(|grid| grid.action(task))
        else {
            return;
        };
        self.serial = self.serial.wrapping_add(1);
        self.shown = Some(Shown {
            action,
            started_ms: now_ms,
            serial: self.serial,
        });
        self.task_slot = Some(slot);
    }

    /// A CAGE task starts: what it shows, its turn, and the damage windows of the fragment entries
    /// the mob plays, its body's and its weapon's (their `CAGE-Damage` clips: start and `DamageTableRow`; the reach comes from
    /// the clip's shape and the entry's `Homing`, see [`window_reach`]).
    #[allow(
        clippy::too_many_arguments,
        reason = "the fields of a PlayTask command"
    )]
    fn play(
        &mut self,
        ai: &MobAi,
        tags: &[NameId],
        slot: Slot,
        task: NameId,
        action: u32,
        done_at_ms: Option<u64>,
        now_ms: u64,
    ) {
        self.show(ai, slot, task, now_ms);
        self.end_attack();
        self.turn = None;
        let Some(cage) = ai.cage_action(action) else {
            return;
        };
        if cage.facing.is_some() {
            // Inferred: the animation turns the body over the action (retail `Combat_Turn` 1632
            // and 1683 ms for 77 to 107 degrees, trace finding a).
            self.turn = done_at_ms.map(|end| (now_ms, end, self.yaw));
        }
        self.open_windows(ai, tags, cage, now_ms, done_at_ms, false);
    }

    /// The damage windows, ranged clips and homing clips of `cage`, which starts at `now_ms`
    /// and ends at `ends_ms` (`None`: Unknown), for a mob with the tags `tags`. An action that a
    /// move plays (`moving`, a `NavigationAction`: the boar's `Attack_Charge`) keeps a damage
    /// clip without an end open for the whole run: it strikes when the run gets to its target
    /// ([`ON_CONTACT_MS`]).
    #[allow(
        clippy::too_many_arguments,
        reason = "the action, its times and how it plays"
    )]
    fn open_windows(
        &mut self,
        ai: &MobAi,
        tags: &[NameId],
        cage: &CageAction,
        now_ms: u64,
        ends_ms: Option<u64>,
        moving: bool,
    ) {
        let has_tag = |tag| tags.contains(&tag);
        // The weapon's `WeaponContext` scope plays the fragment from the weapon's database
        // beside the body. Its windows count only when the body's entry has no `CAGE-Damage`
        // clip (Inferred: 449 tasks hold damage clips in both, mostly the same count, as one hit
        // written twice; Janszoon's axe hits are only in the weapon's database).
        let body = cage
            .variant(&has_tag)
            .map_or(&[][..], |variant| &variant.windows[..]);
        let weapon = cage
            .weapon_variant(&has_tag)
            .map_or(&[][..], |variant| &variant.windows[..]);
        let is_damage = |window: &&ProcWindow| Some(window.kind) == ai.names.damage_clip;
        let windows = if body.iter().any(|window| is_damage(&window)) {
            body
        } else {
            weapon
        };
        // The homing clips of one database, as the damage clips (the body's first).
        let is_homing = |window: &&ProcWindow| Some(window.kind) == ai.names.homing_clip;
        let homing_windows = if body.iter().any(|window| is_homing(&window)) {
            body
        } else {
            weapon
        };
        let clips: Vec<HomingClip> = homing_windows
            .iter()
            .filter(is_homing)
            .map(|window| {
                HomingClip::from_params(
                    window.start_s,
                    window.end_s,
                    window
                        .params
                        .iter()
                        .map(|(name, value)| (ai.name(*name), ai.name(*value))),
                )
            })
            .collect();
        self.homing = clips
            .iter()
            .map(|clip| HomingRun::new(*clip, now_ms, ends_ms))
            .collect();
        let homing = clips.first();
        for window in once_each(windows).filter(is_damage) {
            let Some(row) = param_of(window, ai.names.damage_row) else {
                continue;
            };
            // Seconds of a fragment.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "seconds of a fragment"
            )]
            let starts_ms = now_ms + (window.start_s * MS_PER_S).round() as u64;
            let at_ms = if moving && window.end_s.is_none() {
                ON_CONTACT_MS
            } else {
                starts_ms
            };
            self.windows.push(Window {
                at_ms,
                act: WindowAct::Strike {
                    row,
                    reach: window_reach(ai, shaped(ai, window, weapon), homing),
                    ranged: None,
                },
            });
        }
        // A ranged clip written in both databases is one shot, as a damage clip.
        let is_ranged = |window: &&ProcWindow| Some(window.kind) == ai.names.ranged_clip;
        let ranged_windows = if body.iter().any(|window| is_ranged(&window)) {
            body
        } else {
            weapon
        };
        for window in once_each(ranged_windows).filter(is_ranged) {
            let (Some(row), Some(attack)) = (
                param_of(window, ai.names.damage_row),
                param_of(window, ai.names.ranged_attack),
            ) else {
                continue;
            };
            let text = |name| param_of(window, name).map(|value| ai.name(value).trim());
            // Seconds of a fragment.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "seconds of a fragment"
            )]
            let at_ms = now_ms + (window.start_s * MS_PER_S).round() as u64;
            self.windows.push(Window {
                at_ms,
                act: WindowAct::Strike {
                    row,
                    reach: window_reach(ai, window, homing),
                    ranged: Some(RangedWindow {
                        attack,
                        slice: param_of(window, ai.names.slice_override)
                            .filter(|slice| !ai.name(*slice).trim().is_empty()),
                        lead: text(ai.names.lead_target) == Some(PARAM_TRUE),
                        lead_max_rad: text(ai.names.lead_max_angle)
                            .and_then(|degrees| degrees.parse::<f32>().ok())
                            .unwrap_or(0.0)
                            .to_radians(),
                        speed_mps: text(ai.names.projectile_speed)
                            .and_then(|speed| speed.parse::<f32>().ok())
                            .filter(|speed| *speed > 0.0),
                    }),
                },
            });
        }
        // A spell clip written in both databases is one cast, as a damage clip.
        let is_cast = |window: &&ProcWindow| Some(window.kind) == ai.names.cast_clip;
        let cast_windows = if body.iter().any(|window| is_cast(&window)) {
            body
        } else {
            weapon
        };
        for window in cast_windows.iter().filter(is_cast) {
            let Some(spell) = param_of(window, ai.names.spell_name) else {
                continue;
            };
            let text = |name| param_of(window, name).map(|value| ai.name(value).trim());
            // Seconds of a fragment.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "seconds of a fragment"
            )]
            let at_ms = now_ms + (window.start_s * MS_PER_S).round() as u64;
            self.windows.push(Window {
                at_ms,
                act: WindowAct::Cast(CastWindow {
                    spell,
                    aim: text(ai.names.aim_at_target) == Some(PARAM_TRUE),
                    speed_mps: text(ai.names.projectile_speed)
                        .and_then(|speed| speed.parse::<f32>().ok())
                        .unwrap_or(0.0),
                }),
            });
        }
    }

    /// The moves of the motion and the turn of a facing task over `dt_s`; the end of a motion
    /// goes to `brain`.
    fn move_and_turn(
        &mut self,
        ai: &MobAi,
        brain: &mut Brain,
        at: &mut Position,
        target: Option<TargetView>,
        around: &Surroundings<'_>,
        dt_s: f32,
    ) {
        let now_ms = around.now_ms;
        let ground = around.ground;
        if let Some((start_ms, end_ms, from_yaw)) = self.turn {
            let facing = self
                .shown
                .and_then(|shown| ai.cage_action(shown.action))
                .and_then(|action| action.facing);
            let toward = match (facing, target) {
                (Some(FacingMode::AngleToTarget), Some(target)) => {
                    Some(yaw_toward(*at, target.position))
                }
                _ => self.motion_goal().map(|goal| yaw_toward(*at, goal)),
            };
            if let Some(toward) = toward {
                // A share of a turn of a second or two.
                #[allow(clippy::cast_precision_loss, reason = "a turn of a second or two")]
                let share = (now_ms.saturating_sub(start_ms) as f32
                    / end_ms.saturating_sub(start_ms).max(1) as f32)
                    .min(1.0);
                self.yaw = from_yaw + yaw_gap(from_yaw, toward) * share;
                self.turned = true;
            }
        }
        let Some(mut motion) = self.motion else {
            return;
        };
        let mut mover = Mover {
            position: *at,
            yaw: self.yaw,
            speed_mps: self.speed_mps,
        };
        let max_slope = ai.arch(self.archetype).and_then(|arch| arch.max_slope_deg);
        let flight = self.flight_goal(ai, *at, target);
        let outcome = match &mut motion.kind {
            MotionKind::To(goal) => {
                steer_along(&mut self.path, &mut mover, *goal, &motion.steering, dt_s)
                    .then_some(MotionOutcome::Arrived)
            }
            MotionKind::Follow { goal, config } => match target {
                None => Some(MotionOutcome::Failed),
                Some(target) => {
                    let config =
                        config.and_then(|index| ai.arch(self.archetype)?.nav_configs.get(index));
                    let repath_m = config.map_or(0.0, |config| config.target_repath_distance);
                    let stale = goal.is_none_or(|(_, picked_at)| {
                        picked_at.horizontal_distance(target.position) > repath_m
                    });
                    if stale {
                        let picked = config
                            .and_then(|config| config.select_position.as_ref())
                            .and_then(|params| {
                                self.pick_around(ai, *at, target.position, params, around.ground)
                            })
                            .unwrap_or(target.position);
                        *goal = Some((picked, target.position));
                    }
                    goal.and_then(|(point, _)| {
                        steer_along(&mut self.path, &mut mover, point, &motion.steering, dt_s)
                            .then_some(MotionOutcome::Arrived)
                    })
                }
            },
            MotionKind::Strafe {
                ideal_m,
                left,
                until_ms,
            } => match target {
                None => Some(MotionOutcome::Failed),
                Some(_) if now_ms >= *until_ms => Some(MotionOutcome::Arrived),
                Some(target) => {
                    mover.strafe_around(target.position, *ideal_m, *left, &motion.steering, dt_s);
                    None
                }
            },
            MotionKind::Away => match (target, self.last_damage) {
                (None, None) => Some(MotionOutcome::Failed),
                _ => match flight {
                    None => Some(MotionOutcome::Arrived),
                    Some(point) => {
                        steer_along(&mut self.path, &mut mover, point, &motion.steering, dt_s);
                        None
                    }
                },
            },
        };
        let goal = match motion.kind {
            MotionKind::To(goal) => Some(goal),
            MotionKind::Follow { goal, .. } => goal.map(|(point, _)| point),
            MotionKind::Strafe { .. } => None,
            MotionKind::Away => flight,
        };
        let outcome = match floor_move(ground, *at, mover.position, max_slope) {
            Some(on_floor) => {
                *at = Position {
                    z: height_after_move(ground, on_floor),
                    ..on_floor
                };
                self.keep_trail(ai, *at);
                outcome
            }
            // The straight move is closed: it searches a way around and walks it from its next
            // step.
            None if goal.is_some_and(|goal| self.plan_way(ai, ground, *at, goal, now_ms)) => {
                mover.speed_mps = 0.0;
                outcome
            }
            // It cannot go on: no floor ahead within its slope, and no way around.
            None => {
                tracing::debug!(
                    x = at.x,
                    y = at.y,
                    z = at.z,
                    step_x = mover.position.x,
                    step_y = mover.position.y,
                    "mob move failed: no floor ahead"
                );
                Some(MotionOutcome::Failed)
            }
        };
        self.yaw = mover.yaw;
        self.turned = true;
        self.speed_mps = mover.speed_mps;
        match outcome {
            Some(outcome) => {
                if outcome == MotionOutcome::Arrived {
                    for window in &mut self.windows {
                        if window.at_ms == ON_CONTACT_MS {
                            window.at_ms = now_ms;
                        }
                    }
                }
                brain.report_motion(motion.slot, outcome);
                self.stop_moving();
                // The tree learns of it at once.
                self.next_tick_ms = 0;
            }
            None => self.motion = Some(motion),
        }
    }

    /// A point around `anchor` that `params` pick (a navigation config's
    /// `m_selectPositionParams`: the boar's `RunToTarget` picks 1.5 to 2 m from its target), with
    /// the mob at `at`; the pick reads only the params' own values.
    fn pick_around(
        &mut self,
        ai: &MobAi,
        at: Position,
        anchor: Position,
        params: &SelectPositionParams,
        ground: &(dyn Ground + Sync),
    ) -> Option<Position> {
        let (picked, picks) = {
            let mut view = self.view(ai, at, 0, false);
            view.ground = Some(ground);
            view.target = Some(TargetView {
                position: anchor,
                distance_m: anchor.horizontal_distance(at),
                angle_deg: 0.0,
                visible: true,
                reachable: true,
            });
            let picked = view.select_position(params, &NoBoard).map(position_of);
            (picked, view.picks.get())
        };
        self.picks = picks;
        picked
    }
}

impl TreeMind {
    /// The mind of a mob of archetype `archetype` at `spawn` facing `yaw`, with random numbers
    /// from `seed` and the design overrides `spawn_ai`; `None` without a tree.
    pub fn new(
        ai: &MobAi,
        archetype: u32,
        spawn: Position,
        yaw: f32,
        seed: u64,
        spawn_ai: Option<&SpawnAi>,
    ) -> Option<Self> {
        if !ai.has_tree(archetype) {
            return None;
        }
        let mut brain = Brain::new(&ai.runtime, archetype, seed).ok()?;
        if let Some(key) = ai.data().key(DOES_LEASH_KEY)
            && ai
                .arch(archetype)
                .is_none_or(|arch| arch.config_value(key).is_none())
        {
            brain.set_fact(key, Value::Bool(true));
        }
        if let Some(overrides) = spawn_ai {
            for (key, value) in &overrides.facts {
                brain.set_fact(*key, *value);
            }
            if let (Some(seconds), Some(name)) =
                (overrides.attack_cooldown_s, ai.names.attack_cooldown)
            {
                brain.set_cooldown_times(name, seconds, seconds);
            }
        }
        Some(Self {
            brain,
            state: MindState {
                archetype,
                spawn,
                yaw,
                turned: false,
                speed_mps: 0.0,
                motion: None,
                shown: None,
                serial: 0,
                task_slot: None,
                turn: None,
                windows: Vec::new(),
                homing: Vec::new(),
                path: None,
                next_path_ms: 0,
                trail: Vec::new(),
                target: None,
                target_at: None,
                threat: Threat::default(),
                locked: false,
                manual: None,
                last_damage: None,
                reachable_ms: None,
                reach: None,
                // The slice's `m_initialStatusEffects` (`AI_Enable_Stagger`); the cards do
                // not show them (Observed: no such effect on the heavies' and Seraphim's cards).
                statuses: ai
                    .arch(archetype)
                    .map_or_else(Vec::new, |arch| arch.initial_effects.clone()),
                reaction_until_ms: None,
                route: spawn_ai
                    .and_then(|overrides| overrides.route.clone())
                    .filter(|route| !route.points.is_empty())
                    .map(|route| {
                        let mut walk = RouteWalk {
                            route,
                            next: 0,
                            backwards: false,
                            task_s: 0.0,
                        };
                        walk.visit(Rng48::new(seed).unit());
                        walk
                    }),
                next_tick_ms: 0,
                last_step_ms: None,
                picks: Rng48::new(seed.rotate_left(1)),
                run_speed_mps: spawn_ai.and_then(|overrides| overrides.run_speed_mps),
                sight_m: spawn_ai.and_then(|overrides| overrides.sight_m),
                nw_tags: Vec::new(),
                order: None,
                trackers: Vec::new(),
                status_changes: Vec::new(),
                engine_runs: Vec::new(),
                reaction_action: None,
                staggered: None,
                guard: None,
                blocking: false,
                decision: None,
                target_reason: None,
                chase_fallback_m: None,
                guard_objective: spawn_ai.and_then(|overrides| overrides.guard_objective),
                spared: Vec::new(),
            },
        })
    }

    /// It chases `chase_m` from its spawn: the chase distance of a mob whose data gives none.
    pub const fn set_chase_fallback(&mut self, chase_m: f32) {
        self.state.chase_fallback_m = Some(chase_m);
    }

    /// It can block with guard `guard` (its mob type's): its stamina starts at the guard's
    /// initial stamina.
    pub fn set_guard(&mut self, guard: MobGuard) {
        self.state.guard = Some(GuardState {
            rules: guard,
            stamina: guard.stamina_initial,
            regen_ms: None,
        });
    }

    /// Its guard, when it can block.
    pub fn guard(&self) -> Option<MobGuard> {
        self.state.guard.map(|guard| guard.rules)
    }

    /// Its guard stamina now, when it can block: blocked hits lower it, its regen and the end of
    /// `BlockBroken` raise it.
    pub fn guard_stamina(&self) -> Option<f32> {
        self.state.guard.map(|guard| guard.stamina)
    }

    /// Its block is up: a task action it plays raises one (`Block_On`), or an action the engine
    /// runs does (`Block`, while no `DisableBlock` marker of its fragment is on), as of its last
    /// step. A mob without a guard never blocks.
    pub const fn blocking(&self) -> bool {
        self.state.blocking
    }

    /// A blocked hit of block stamina `loss` takes its guard share ([`MobGuard::blocked_loss`])
    /// from the mob at `at` at `now_ms`; the hit that takes the last breaks its guard: the
    /// `BrokenGuard`
    /// reaction plays and sets `BlockBroken`, whose engine action holds the regen and fills the
    /// stamina at its end. `None` for a mob without a guard.
    pub fn guard_hit(
        &mut self,
        ai: &MobAi,
        at: Position,
        now_ms: u64,
        loss: f32,
    ) -> Option<GuardHit> {
        let guard = self.state.guard.as_mut()?;
        guard.stamina = (guard.stamina - MobGuard::blocked_loss(loss)).max(0.0);
        if guard.stamina > 0.0 {
            return Some(GuardHit::Held);
        }
        self.break_guard(ai, at, now_ms);
        Some(GuardHit::Broken)
    }

    /// The length of the entry of reaction action `action` the mob plays with its tags now;
    /// `None` when it loops or is Unknown.
    fn reaction_length_ms(&self, ai: &MobAi, action: u32) -> Option<u64> {
        self.entry_length_ms(&ai.data().cage_actions.get(action as usize)?.variants)
    }

    /// The length of the entry of `variants` the mob plays with its tags now; `None` when it
    /// loops or is Unknown.
    fn entry_length_ms(&self, variants: &[FragmentVariant]) -> Option<u64> {
        let tags = self.brain.tags();
        let has_tag = |tag: NameId| tags.contains(&tag);
        CageAction::variant_of(variants, &has_tag)
            .and_then(|variant| match variant.length {
                // A reaction of a few seconds.
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a reaction of a few seconds"
                )]
                FragmentLength::Seconds(seconds) => {
                    Some((seconds * MS_PER_SECOND_F32).round() as u64)
                }
                FragmentLength::Loops | FragmentLength::Unknown => None,
            })
    }

    /// Its grit is on at `now_ms` while it holds the effects `held`: its slice's grit is its
    /// stamina (`m_useStaminaAsValue`) and a running engine action's `ActivateGrit` branch
    /// holds (the stagger lists' `ActivateGrit`: while it holds `AI_Enable_Stagger`, no
    /// `DisableStagger` marker is on and `r_Stagger` does not play).
    fn grit_on(&self, ai: &MobAi, now_ms: u64, held: Option<&HeldEffects>) -> bool {
        if !ai
            .arch(self.state.archetype)
            .is_some_and(|arch| arch.grit_uses_stamina)
        {
            return false;
        }
        let data = ai.data();
        self.with_cage(ai, now_ms, held, |mob| {
            self.state
                .engine_runs
                .iter()
                .filter_map(|run| data.cage_actions.get(run.action as usize))
                .any(|cage| {
                    cage.grit_on
                        .as_ref()
                        .is_some_and(|condition| condition.holds_with(mob))
                })
        })
    }

    /// An unblocked hit of stagger `stagger` from `from` (the attacker's position, when known)
    /// lands on the mob at `at` at `now_ms` while it holds the effects `held`: with its grit
    /// on, its stamina loses [`MobGuard::grit_loss`] of it (Observed: 3,427 of 40,765 own
    /// direct hits of the ledgers lowered a mob's child 44, on the slices that hold
    /// `AI_Enable_Stagger`), and a hit that leaves none starts its stagger
    /// ([`Self::stagger`]). `None`: it has no stamina, or its grit is off.
    pub fn grit_hit(
        &mut self,
        ai: &MobAi,
        (at, from): (Position, Option<Position>),
        now_ms: u64,
        stagger: f32,
        held: Option<&HeldEffects>,
    ) -> Option<GritHit> {
        if self.state.guard.is_none() || !self.grit_on(ai, now_ms, held) {
            return None;
        }
        let reduction = held.map_or(0.0, |held| held.stagger_reduction);
        let side = from.map(|from| self.hit_angle_deg(at, from));
        let large_status_effects_fx = ai
            .arch(self.state.archetype)
            .is_some_and(|arch| arch.large_status_effects_fx);
        let guard = self.state.guard.as_mut()?;
        let body = guard.rules.grit_body(large_status_effects_fx);
        guard.stamina = (guard.stamina - MobGuard::grit_loss(stagger, reduction, body)).max(0.0);
        if guard.stamina > 0.0 || !self.stagger(ai, at, now_ms, held, side) {
            return Some(GritHit::Held);
        }
        Some(GritHit::Staggered)
    }

    /// The side a hit from `from` comes from for the mob at `at`, in degrees from its facing
    /// (-180 to 180), positive to its right (Inferred: the lists name 45 to 135 degrees
    /// `damaged_right`; the stagger's front and back need only the size).
    fn hit_angle_deg(&self, at: Position, from: Position) -> f32 {
        -yaw_gap(self.state.yaw, yaw_toward(at, from)).to_degrees()
    }

    /// The stagger (`Reaction_Staggered`) of the mob at `at` starts at `now_ms` after a hit
    /// from `side` degrees of its facing (`None`: Unknown), when its grid has one whose
    /// fragment length is known and its activation holds: the mob holds `AI_Enable_Stagger`
    /// (and not `ResetAI`), no `DisableStagger` or `DisableStaggerReaction` marker is on, it is
    /// not stunned and no stagger plays. The fragment its `ChangeActionFragment` picks by the
    /// side plays (`r_Stagger_F` from ahead, `r_Stagger_B` from behind, else `r_Stagger`) and
    /// shows (Observed: retail Blind Seraph cards played `r_Stagger_F`), with its `Stunned`
    /// tag; the tree's plan stops, it holds `AI_Staggered_Tracker`, and its end
    /// ([`STAGGER_EXIT_OFFSET_MS`] before the fragment's) fills the stamina. Whether it started.
    fn stagger(
        &mut self,
        ai: &MobAi,
        at: Position,
        now_ms: u64,
        held: Option<&HeldEffects>,
        side: Option<f32>,
    ) -> bool {
        let names = ai.names;
        let Some(action) = ai.named(self.state.archetype, names.stagger) else {
            return false;
        };
        let Some(cage) = ai.data().cage_actions.get(action as usize) else {
            return false;
        };
        let (fragment, variants) =
            self.with_cage_hit(ai, now_ms, held, side, |mob| cage.starting_fragment(mob));
        let Some(length_ms) = self.entry_length_ms(variants) else {
            return false;
        };
        let free = self.with_cage(ai, now_ms, held, |mob| {
            names.enable_stagger.is_some_and(|effect| (mob.has_effect)(effect))
                && !names.reset.is_some_and(|effect| (mob.has_effect)(effect))
                && !names
                    .disable_stagger
                    .iter()
                    .flatten()
                    .any(|marker| (mob.has_marker)(*marker))
        });
        let stunned = held.is_some_and(|held| !held.control.may_fight());
        if !free || stunned || self.state.reaction_action == Some(action) {
            return false;
        }
        self.state.reaction_until_ms =
            Some(now_ms + length_ms.saturating_sub(STAGGER_EXIT_OFFSET_MS));
        self.reaction(ai, at, now_ms, action, true);
        self.state.serial = self.state.serial.wrapping_add(1);
        self.state.staggered = Some((
            Shown {
                action,
                started_ms: now_ms,
                serial: self.state.serial,
            },
            fragment,
        ));
        self.state.stop_moving();
        self.state.end_attack();
        true
    }

    /// The guard of the mob at `at` breaks at `now_ms`: its `BrokenGuard` reaction plays.
    fn break_guard(&mut self, ai: &MobAi, at: Position, now_ms: u64) {
        self.state.blocking = false;
        let Some(action) = ai.named(self.state.archetype, ai.names.broken_guard) else {
            return;
        };
        let length_ms = self
            .reaction_length_ms(ai, action)
            .unwrap_or(BROKEN_GUARD_MS);
        self.state.reaction_until_ms = Some(now_ms + length_ms);
        self.reaction(ai, at, now_ms, action, true);
        self.state.stop_moving();
        self.state.end_attack();
    }

    /// The tags, markers and status effects its CAGE conditions read at `now_ms`, with the
    /// effects `held` the world holds on it, given to `with`; no hit is tested.
    fn with_cage<R>(
        &self,
        ai: &MobAi,
        now_ms: u64,
        held: Option<&HeldEffects>,
        with: impl FnOnce(&MobCage<'_>) -> R,
    ) -> R {
        self.with_cage_hit(ai, now_ms, held, None, with)
    }

    /// [`Self::with_cage`] testing a hit from `hit_angle_deg` degrees of its facing.
    fn with_cage_hit<R>(
        &self,
        ai: &MobAi,
        now_ms: u64,
        held: Option<&HeldEffects>,
        hit_angle_deg: Option<f32>,
        with: impl FnOnce(&MobCage<'_>) -> R,
    ) -> R {
        let data = ai.data();
        let tags = self.brain.tags();
        let has_tag = |tag: NameId| tags.contains(&tag);
        let shown = self
            .state
            .shown
            .and_then(|shown| Some((data.cage_actions.get(shown.action as usize)?, shown)));
        let has_marker = |marker: NameId| {
            let (Some((cage, shown)), Some(clip), Some(param)) =
                (shown, ai.names.named_clip, ai.names.named_clip_name)
            else {
                return false;
            };
            // Seconds into a fragment of a few seconds.
            #[allow(clippy::cast_precision_loss, reason = "seconds into a fragment")]
            let elapsed_s = now_ms.saturating_sub(shown.started_ms) as f32 / MS_PER_SECOND_F32;
            data.sets_marker(cage, &has_tag, (elapsed_s, marker), (clip, param))
        };
        let has_effect = |effect: NameId| {
            let name = ai.name(effect);
            self.state
                .statuses
                .iter()
                .any(|status| ai.name(*status).eq_ignore_ascii_case(name))
                || held.is_some_and(|held| {
                    held.effects
                        .iter()
                        .any(|held| held.eq_ignore_ascii_case(name))
                })
        };
        let playing = [
            self.state.shown.map(|shown| shown.action),
            self.state.reaction_action,
        ];
        let plays_fragment = |fragment: NameId| {
            playing
                .iter()
                .flatten()
                .filter_map(|action| data.cage_actions.get(*action as usize))
                .any(|cage| cage.fragment == fragment)
        };
        with(&MobCage {
            has_tag: &has_tag,
            has_marker: &has_marker,
            has_effect: &has_effect,
            plays_fragment: &plays_fragment,
            hit_angle_deg,
        })
    }

    /// Runs the actions the engine starts by their condition, for the mob at `at` at `now_ms`
    /// with the effects `held`: an action starts while its activation holds and ends when its
    /// deactivation holds (or, without one, when its activation stops holding); each of its tag
    /// activities is on while its branch holds. Then its regen runs unless a running action
    /// holds it, and its block state follows ([`Self::blocking`]).
    fn run_engine_actions(
        &mut self,
        ai: &MobAi,
        at: Position,
        now_ms: u64,
        held: Option<&HeldEffects>,
    ) {
        let engine = ai.engine_started(self.state.archetype);
        let data = ai.data();
        let steps: Vec<EngineStep> = self.with_cage(ai, now_ms, held, |mob| {
            let mut steps = Vec::new();
            for action in &engine {
                let Some(cage) = data.cage_actions.get(*action as usize) else {
                    continue;
                };
                let active = cage
                    .activation
                    .as_ref()
                    .is_some_and(|condition| condition.holds_with(mob));
                let on: Vec<bool> = cage
                    .tags
                    .iter()
                    .map(|tag| tag.condition.holds_with(mob))
                    .collect();
                match self
                    .state
                    .engine_runs
                    .iter()
                    .find(|run| run.action == *action)
                {
                    None if active => steps.push(EngineStep::Start(*action, on)),
                    None => {}
                    Some(run) => {
                        let stop = cage
                            .deactivation
                            .as_ref()
                            .map_or(!active, |condition| condition.holds_with(mob));
                        if stop {
                            let reset = cage
                                .stamina_reset
                                .as_ref()
                                .is_some_and(|condition| condition.holds_with(mob));
                            steps.push(EngineStep::Stop(*action, run.tags_on.clone(), reset));
                        } else {
                            for (index, (was, now)) in run.tags_on.iter().zip(&on).enumerate() {
                                if was != now {
                                    steps.push(EngineStep::Toggle(*action, index, *now));
                                }
                            }
                        }
                    }
                }
            }
            steps
        });
        if !steps.is_empty() {
            let passive = self.passive(ai);
            let mut out = Vec::new();
            {
                let view = self.state.view(ai, at, now_ms, passive);
                for step in &steps {
                    match step {
                        EngineStep::Start(action, on) | EngineStep::Stop(action, on, _) => {
                            let enter = matches!(step, EngineStep::Start(..));
                            for (index, _) in on.iter().enumerate().filter(|(_, on)| **on) {
                                self.brain.engine_activity(
                                    &ai.runtime,
                                    &view,
                                    (*action, index),
                                    enter,
                                    &mut out,
                                );
                            }
                        }
                        EngineStep::Toggle(action, index, on) => self.brain.engine_activity(
                            &ai.runtime,
                            &view,
                            (*action, *index),
                            *on,
                            &mut out,
                        ),
                    }
                }
            }
            for step in steps {
                match step {
                    EngineStep::Start(action, tags_on) => {
                        self.state.engine_runs.push(EngineRun { action, tags_on });
                    }
                    EngineStep::Stop(action, _, reset) => {
                        self.state.engine_runs.retain(|run| run.action != action);
                        if let Some(guard) = self.state.guard.as_mut().filter(|_| reset) {
                            guard.stamina = guard.rules.stamina_max;
                        }
                    }
                    EngineStep::Toggle(action, index, on) => {
                        if let Some(flag) = self
                            .state
                            .engine_runs
                            .iter_mut()
                            .find(|run| run.action == action)
                            .and_then(|run| run.tags_on.get_mut(index))
                        {
                            *flag = on;
                        }
                    }
                }
            }
            self.apply(ai, at, now_ms, out, &mut Thought::default(), Others::NONE);
        }
        self.guard_step(ai, now_ms, held);
    }

    /// The guard's regen at `now_ms` (none while a running engine action holds it; faster by
    /// the `StaminaRate` of the status effects `held`, [`MobGuard::regen_with`]) and the block
    /// state.
    fn guard_step(&mut self, ai: &MobAi, now_ms: u64, held: Option<&HeldEffects>) {
        if self.state.guard.is_none() {
            return;
        }
        let data = ai.data();
        let (regen_off, blocking) = self.with_cage(ai, now_ms, held, |mob| {
            let runs = || {
                self.state
                    .engine_runs
                    .iter()
                    .filter_map(|run| data.cage_actions.get(run.action as usize))
            };
            let regen_off = runs().any(|cage| {
                cage.regen_off
                    .as_ref()
                    .is_some_and(|condition| condition.holds_with(mob))
            });
            let shown = self
                .state
                .shown
                .and_then(|shown| data.cage_actions.get(shown.action as usize))
                .is_some_and(|cage| cage.raises_block(mob));
            (
                regen_off,
                shown || runs().any(|cage| cage.raises_block(mob)),
            )
        });
        self.state.blocking = blocking;
        if let Some(guard) = self.state.guard.as_mut() {
            let from = guard.regen_ms.unwrap_or(now_ms);
            guard.regen_ms = Some(now_ms);
            if !regen_off {
                // A step of a second or less.
                #[allow(clippy::cast_precision_loss, reason = "a step of a second or less")]
                let seconds = now_ms.saturating_sub(from) as f32 / MS_PER_SECOND_F32;
                let rate = held.map_or(0.0, |held| held.stamina_rate);
                guard.stamina = (guard.stamina + guard.rules.regen_with(rate) * seconds)
                    .min(guard.rules.stamina_max);
            }
        }
    }

    /// Its archetype.
    pub const fn archetype(&self) -> u32 {
        self.state.archetype
    }

    /// Where it faces.
    pub const fn yaw(&self) -> f32 {
        self.state.yaw
    }

    /// Whether its facing is its own: it turned or moved since its spawn (the spawn's facing is
    /// the prefab's, which the world file does not keep).
    pub const fn turned(&self) -> bool {
        self.state.turned
    }

    /// Its target.
    pub const fn target(&self) -> Option<u64> {
        self.state.target
    }

    /// The action it plays: its stagger while one plays, else its tree's action.
    pub fn shown(&self) -> Option<Shown> {
        self.state
            .staggered
            .map(|(shown, _)| shown)
            .or(self.state.shown)
    }

    /// The fragment its stagger plays, while one plays ([`Self::stagger`]).
    pub fn stagger_fragment(&self) -> Option<NameId> {
        self.state.staggered.map(|(_, fragment)| fragment)
    }

    /// The task of the route point it is at, when the point has one.
    pub fn waypoint_task(&self) -> Option<NameId> {
        Some(self.state.route.as_ref()?.task()?.task)
    }

    /// Its Mannequin tags.
    pub fn tags(&self) -> &[NameId] {
        self.brain.tags()
    }

    /// The brain.
    pub const fn brain(&self) -> &Brain {
        &self.brain
    }

    /// Its networked tags (`UpdateNWTags`).
    pub fn nw_tags(&self) -> &[NameId] {
        &self.state.nw_tags
    }

    /// Another mob's tree sets `fact` on its tree ([`RemoteFact`]); its tree runs at once.
    pub fn set_remote_fact(&mut self, fact: &RemoteFact) {
        if fact.overwrite || self.brain.fact(fact.key).is_none() {
            self.brain.set_fact(fact.key, fact.value);
            self.state.next_tick_ms = 0;
        }
    }

    /// The status effects its tree put on it (`true`) or took off it since the last call, by
    /// id: the sim holds them with the effects players put on it, so that they change its
    /// damage and show to the clients.
    pub fn take_status_changes(&mut self) -> Vec<(NameId, bool)> {
        std::mem::take(&mut self.state.status_changes)
    }

    /// Another mob's tree gives it `order` (`AssignOrder`); its tree runs at once.
    pub const fn assign_order(&mut self, order: u32) {
        self.state.order = Some(order);
        self.state.next_tick_ms = 0;
    }

    /// Whether it walks home in a leash reset: the tree applied `ResetAI` or `ResetAINoHeal`
    /// (it takes no damage then; Inferred, as the old return state).
    pub fn returning(&self, ai: &MobAi) -> bool {
        self.state.statuses.iter().any(|status| {
            Some(*status) == ai.names.reset || Some(*status) == ai.names.reset_no_heal
        })
    }

    /// Whether it plays an action with a damage window still to come.
    pub fn attacking(&self) -> bool {
        !self.state.windows.is_empty()
    }

    /// Whether its slice (or its spawn's override) makes it passive.
    fn passive(&self, ai: &MobAi) -> bool {
        ai.names
            .is_passive
            .and_then(|key| {
                self.brain
                    .fact(key)
                    .or_else(|| ai.arch(self.state.archetype)?.config_value(key))
            })
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// `attacker` at `from` hit the mob, adding `threat`: whether the hit counts (a mob in a
    /// leash reset takes none).
    pub fn hurt(&mut self, ai: &MobAi, attacker: u64, from: Position, threat: u64) -> bool {
        if self.returning(ai) {
            return false;
        }
        self.state.threat.add(attacker, threat);
        self.state.last_damage = Some(from);
        // A hit wakes a waiting tree at once.
        self.state.next_tick_ms = 0;
        true
    }

    /// The mob never takes the players `players` as targets, and drops its target and their
    /// threat when they are among them (an Outpost Rush summon spares its own team; Inferred
    /// from the summons' cards, which carry the summoning team's temporary affiliations).
    pub fn spare(&mut self, players: Vec<u64>) {
        for player in &players {
            self.forget(*player);
        }
        self.state.spared = players;
    }

    /// Whether it spares player `player` ([`Self::spare`]): the player's hits on it are
    /// refused (Inferred: the summon's card carries the temporary affiliations of the player's
    /// team, which make it a friend of that team; no capture shows a hit on an own summon).
    pub fn spares(&self, player: u64) -> bool {
        self.state.spared.contains(&player)
    }

    /// The players it spares ([`Self::spare`]).
    #[cfg(test)]
    pub fn spared(&self) -> &[u64] {
        &self.state.spared
    }

    /// The place it guards ([`SpawnAi::guard_objective`]).
    #[cfg(test)]
    pub const fn guard_objective(&self) -> Option<[f32; 3]> {
        self.state.guard_objective
    }

    /// The mob takes `target` as its target when it has none (group aggro).
    pub fn share_target(&mut self, target: u64) {
        if self.state.target.is_none() {
            self.state.threat.add(target, 0);
            self.state.next_tick_ms = 0;
        }
    }

    /// The mob forgets `entity`.
    pub fn forget(&mut self, entity: u64) {
        self.state.threat.forget(entity);
        if self.state.target == Some(entity) {
            self.state.target = None;
        }
    }

    /// A hit reaction of `length_ms` starts at `now_ms` on the mob at `at`: the engine's
    /// `Reaction` action reaches the tree (it cancels the plan and sets the action's tags, for
    /// example the boar's `Alerted`; Observed in `boar_reaction.actionlist`), and
    /// `Full_Body_Reaction` is true while it plays. A swing in progress is lost (retail dropped a
    /// creature's swing when a hit made it reel; swing note).
    pub fn react(&mut self, ai: &MobAi, at: Position, now_ms: u64, length_ms: u64) {
        self.state.reaction_until_ms = Some(now_ms + length_ms);
        if let Some(action) = ai.aliased(self.state.archetype, ai.names.reaction) {
            self.reaction(ai, at, now_ms, action, true);
        }
        self.state.stop_moving();
        self.state.end_attack();
    }

    /// The engine's reaction action `action` (the grid's `Reaction`, or a broken guard's)
    /// starts (`playing`) or ends.
    fn reaction(&mut self, ai: &MobAi, at: Position, now_ms: u64, action: u32, playing: bool) {
        self.state.next_tick_ms = 0;
        self.state.reaction_action = playing.then_some(action);
        if Some(action) == ai.named(self.state.archetype, ai.names.stagger) {
            if !playing {
                self.state.staggered = None;
            }
            // `ModifyStatusEffects`: the tracker on enter, off on exit; `ResetStamina` on exit.
            if let Some(tracker) = ai.names.stagger_tracker {
                self.state.status_changes.push((tracker, playing));
            }
            if let Some(guard) = self.state.guard.as_mut().filter(|_| !playing) {
                guard.stamina = guard.rules.stamina_max;
            }
        }
        let passive = self.passive(ai);
        let mut out = Vec::new();
        {
            let view = self.state.view(ai, at, now_ms, passive);
            if playing {
                self.brain
                    .engine_action(&ai.runtime, &view, action, &mut out);
            } else {
                self.brain
                    .engine_action_end(&ai.runtime, &view, action, &mut out);
            }
        }
        self.apply(ai, at, now_ms, out, &mut Thought::default(), Others::NONE);
    }

    /// The mob died at `at`: the engine's `Death` action cancels the plan.
    pub fn died(&mut self, ai: &MobAi, at: Position, now_ms: u64) {
        if let Some(action) = ai.aliased(self.state.archetype, ai.names.death) {
            let passive = self.passive(ai);
            let mut out = Vec::new();
            let view = self.state.view(ai, at, now_ms, passive);
            self.brain
                .engine_action(&ai.runtime, &view, action, &mut out);
        }
        let state = &mut self.state;
        state.stop_moving();
        state.end_attack();
        state.shown = None;
        state.engine_runs.clear();
        state.blocking = false;
        state.target = None;
        state.threat.clear();
        state.trackers.clear();
    }

    /// Whether the mind has work at `now_ms`: its tick, a damage window or the end of a
    /// reaction is due (between its ticks a mob neither perceives nor moves: the retail state
    /// rate, trace finding c).
    pub fn due(&self, now_ms: u64) -> bool {
        let state = &self.state;
        now_ms >= state.next_tick_ms
            || state.windows.iter().any(|window| now_ms >= window.at_ms)
            || state.reaction_until_ms.is_some_and(|until| now_ms >= until)
    }

    /// One step at `around.now_ms` of the mob at `at`: it perceives, its damage windows hit,
    /// and when its tick is due it moves, turns and its tree thinks.
    pub fn step(&mut self, ai: &MobAi, at: &mut Position, around: &Surroundings<'_>) -> Thought {
        let now_ms = around.now_ms;
        let mut thought = Thought::default();
        if self
            .state
            .reaction_until_ms
            .is_some_and(|until| now_ms >= until)
        {
            self.state.reaction_until_ms = None;
            if let Some(action) = self.state.reaction_action {
                self.reaction(ai, *at, now_ms, action, false);
            }
        }
        self.run_engine_actions(ai, *at, now_ms, around.held);
        let control = around
            .held
            .map_or_else(Control::default, |held| held.control);
        if !control.may_fight() {
            // Inferred: a stunned mob (`Stunned`, `AI_Stun_2sec`) stands, its swing is lost as in
            // a hit reaction, and its tree waits until the stun ends, as a stunned player can
            // neither move nor attack.
            self.state.stop_moving();
            self.state.end_attack();
            return thought;
        }
        let had = self.state.target;
        let passive = self.passive(ai);
        let perceived = self.state.perceive(ai, *at, passive, around);
        self.log_target_change(ai, had, *at);
        let perceived_target = self.state.target;
        // Out of the fight and home: the trail of its way out is done with.
        if self.state.target.is_none() && !self.state.resetting(ai) {
            self.state.trail.clear();
        }
        if had.is_none() {
            thought.acquired = self.state.target;
        }
        self.state
            .fire_windows(ai, around.entity, now_ms, &mut thought);
        if now_ms < self.state.next_tick_ms {
            return thought;
        }
        let dt_ms = self
            .state
            .last_step_ms
            .map_or(0, |last| now_ms.saturating_sub(last).min(MAX_STEP_MS));
        self.state.last_step_ms = Some(now_ms);
        // A step below a second.
        #[allow(clippy::cast_precision_loss, reason = "a step below a second")]
        let dt_s = dt_ms as f32 / MS_PER_S;
        // Inferred: a root (`NoDodge`: the roots and tethers) holds the mob where it stands; it
        // still turns and attacks.
        let target_at = perceived.target.map(|target| target.position);
        if control.may_dodge() {
            self.state
                .move_and_turn(ai, &mut self.brain, at, perceived.target, around, dt_s);
            self.state
                .home_in(ai, at, target_at, around.ground, now_ms, false);
            // A run that got to its target strikes now: the tree's next action starts in this
            // step and takes the windows of the run away.
            self.state
                .fire_windows(ai, around.entity, now_ms, &mut thought);
        } else {
            self.state.speed_mps = 0.0;
            self.state
                .home_in(ai, at, target_at, around.ground, now_ms, true);
        }
        let others = Others {
            entity: around.entity,
            tagged: around.tagged,
        };
        let mut out = Vec::new();
        let picks = {
            let mut view = self.state.view(ai, *at, now_ms, passive);
            view.target = perceived.target;
            view.candidates = perceived.candidates;
            view.health_share = around.health_share;
            view.ground = Some(around.ground);
            view.others = others;
            view.seen = around.seen;
            view.held = around.held;
            self.brain.tick(&ai.runtime, &view, &mut out);
            view.picks.get()
        };
        self.state.picks = picks;
        thought.ticked = true;
        self.apply(ai, *at, now_ms, out, &mut thought, others);
        self.log_target_change(ai, perceived_target, *at);
        let activity = if self.state.target.is_some() {
            Activity::Combat
        } else if self.state.motion.is_some() {
            Activity::Moving
        } else {
            Activity::Idle
        };
        self.state.next_tick_ms = now_ms + ai.rates.interval_ms(activity, perceived.nearest_m);
        thought
    }

    /// Logs the start of an action or move by node `node` playing `task`, when it differs from
    /// the one the mob at `at` started last (the live check of a mob's decisions: one line per
    /// change, not per tick).
    fn log_decision(&mut self, ai: &MobAi, entity: u64, at: Position, node: NodeId, task: NameId) {
        if self.state.decision == Some((node, task)) {
            return;
        }
        self.state.decision = Some((node, task));
        let data = ai.data();
        tracing::debug!(
            mob = entity,
            archetype = self.state.archetype,
            node,
            node_name = ai.name(data.node(node).name),
            task = ai.name(task),
            target = ?self.state.target,
            target_m = ?self.state.target_at.map(|target| target.horizontal_distance(at)),
            reachable = ?self.state.reach.map(|check| check.reachable),
            "mob decision"
        );
    }

    /// Logs a change of the target from `from` and its reason (the live check of aggro and
    /// leash).
    fn log_target_change(&self, ai: &MobAi, from: Option<u64>, at: Position) {
        if self.state.target != from {
            tracing::debug!(
                archetype = self.state.archetype,
                from = ?from,
                to = ?self.state.target,
                reason = ?self.state.target_reason,
                resetting = self.state.resetting(ai),
                x = at.x,
                y = at.y,
                z = at.z,
                "mob target changed"
            );
        }
    }

    /// The steering of a navigation config of the archetype, or of the node's own fields.
    fn steering(
        &self,
        ai: &MobAi,
        node: NodeId,
        config: Option<&NavConfig>,
        view: &View<'_>,
    ) -> Steering {
        if let Some(config) = config {
            let run = Some(config.kind) == ai.names.run_config;
            return Steering {
                max_speed_mps: self
                    .state
                    .run_speed_mps
                    .filter(|_| run)
                    .unwrap_or(config.max_speed),
                min_speed_mps: config.min_speed,
                acceleration_mps2: config.linear_acceleration,
                deceleration_mps2: config.linear_deceleration,
                turn_rate_deg_s: config.turn_rate_deg,
                arrival_m: config.arrival_threshold,
            };
        }
        let number = |param: &Param| {
            self.brain
                .resolve(&ai.runtime, view, param)
                .and_then(Value::as_number)
                // Node fields of metres and seconds.
                .map_or(0.0, |value| value as f32)
        };
        let (motion, turn_rate) = match &ai.data().node(node).kind {
            NodeKind::Navigation(nav) => (&nav.motion, &nav.turn_rate),
            // Inferred: a strafe's facing follows its target at this rate.
            NodeKind::Strafe(strafe) => (&strafe.motion, &strafe.facing_convergence_rate),
            _ => return STANDING,
        };
        Steering {
            max_speed_mps: number(&motion.max_speed),
            min_speed_mps: number(&motion.min_speed),
            acceleration_mps2: number(&motion.linear_acceleration),
            deceleration_mps2: number(&motion.linear_deceleration),
            turn_rate_deg_s: number(turn_rate),
            arrival_m: number(&motion.arrival_threshold),
        }
    }

    /// Applies the tree's commands at `now_ms` to the mob at `at`; its remote actions reach
    /// `others`.
    fn apply(
        &mut self,
        ai: &MobAi,
        at: Position,
        now_ms: u64,
        out: Vec<Command>,
        thought: &mut Thought,
        others: Others<'_>,
    ) {
        let passive = self.passive(ai);
        for command in out {
            match command {
                Command::PlayTask {
                    slot,
                    node,
                    task,
                    action,
                    done_at_ms,
                } => {
                    self.log_decision(ai, others.entity, at, node, task);
                    let tags = self.brain.tags();
                    self.state
                        .play(ai, tags, slot, task, action, done_at_ms, now_ms);
                    // The action's `TriggerEntity` activities run as it starts, each when the
                    // conditions of its branches hold.
                    if let Some(cage) = ai.data().cage_actions.get(action as usize) {
                        let has_tag = |tag| tags.contains(&tag);
                        for trigger in &cage.triggers {
                            if trigger.condition.holds(&has_tag) {
                                self.trigger(ai, trigger.trigger, thought);
                            }
                        }
                    }
                }
                Command::EndTask { slot } => {
                    if self.state.task_slot == Some(slot) {
                        self.state.end_attack();
                        self.state.turn = None;
                    }
                }
                Command::Navigate {
                    slot,
                    node,
                    task,
                    destination,
                    config,
                } => {
                    let config_index = config;
                    let config = config
                        .and_then(|index| ai.arch(self.state.archetype)?.nav_configs.get(index));
                    let steering = {
                        let view = self.state.view(ai, at, now_ms, passive);
                        self.steering(ai, node, config, &view)
                    };
                    // A move to where the mob stands is done at once and shows no move: retail
                    // ends a navigation at its arrival (Inferred), and the live pistoleer of
                    // archetype 678 played `Move_CAGE_Task` 1,190 times on its own waypoint at
                    // (7773.8, 3565.8), its Patrol sub-tree's `PatrolMove` (node 1173) before
                    // each waypoint task: it walked in place (sim log 2026-11-12 14:28 to 14:35).
                    if let Destination::Point(point) = destination
                        && at.horizontal_distance(position_of(point)) <= steering.arrival_m
                    {
                        self.state.stop_moving();
                        self.brain.report_motion(slot, MotionOutcome::Arrived);
                        continue;
                    }
                    let flees = match &ai.data().node(node).kind {
                        NodeKind::Navigation(nav) => {
                            nav.motion.nav_config.is_some()
                                && nav.motion.nav_config == ai.names.flee_config
                        }
                        _ => false,
                    };
                    let kind = match destination {
                        Destination::Point(point) => MotionKind::To(position_of(point)),
                        Destination::Target if flees => MotionKind::Away,
                        Destination::Target => MotionKind::Follow {
                            goal: None,
                            config: config_index,
                        },
                    };
                    self.state.motion = Some(Motion {
                        slot,
                        kind,
                        steering,
                    });
                    self.log_decision(ai, others.entity, at, node, task);
                    self.state.show(ai, slot, task, now_ms);
                    let moved = self
                        .state
                        .shown
                        .and_then(|shown| ai.cage_action(shown.action));
                    if let Some(cage) = moved {
                        self.state
                            .open_windows(ai, self.brain.tags(), cage, now_ms, None, true);
                    }
                }
                Command::Strafe {
                    slot,
                    node,
                    task,
                    config,
                } => {
                    let NodeKind::Strafe(strafe) = &ai.data().node(node).kind else {
                        continue;
                    };
                    self.log_decision(ai, others.entity, at, node, task);
                    let config = config
                        .and_then(|index| ai.arch(self.state.archetype)?.nav_configs.get(index));
                    let (steering, ideal, lateral) = {
                        let view = self.state.view(ai, at, now_ms, passive);
                        // A field the node leaves unset comes from the slice's strafe config
                        // ([`strafe_config_number`]).
                        let number = |param: &Param, field: &str| {
                            self.brain
                                .resolve(&ai.runtime, &view, param)
                                .and_then(Value::as_number)
                                .or_else(|| strafe_config_number(config, field))
                        };
                        (
                            self.steering(ai, node, config, &view),
                            (
                                number(&strafe.ideal_distance_min, IDEAL_MIN_FIELD).unwrap_or(0.0),
                                number(&strafe.ideal_distance_max, IDEAL_MAX_FIELD).unwrap_or(0.0),
                            ),
                            number(&strafe.lateral_frequency_min, LATERAL_MIN_FIELD)
                                .zip(number(&strafe.lateral_frequency_max, LATERAL_MAX_FIELD)),
                        )
                    };
                    // Inferred: one sideways move lasts a random time of its lateral move
                    // frequency (seconds); retail strafes of 31 to 1303 ms were cut earlier by the
                    // next plan (trace finding b).
                    let lateral_s =
                        lateral.map_or(0.0, |(min, max)| self.state.picks.range(min, max));
                    let left = self.state.picks.unit() < HALF;
                    // Metres and seconds of the node.
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "metres and seconds of the node"
                    )]
                    let (ideal_m, until_ms) = (
                        (ideal.0 as f32, ideal.1 as f32),
                        now_ms + (lateral_s * MS_PER_S_F64) as u64,
                    );
                    self.state.motion = Some(Motion {
                        slot,
                        kind: MotionKind::Strafe {
                            ideal_m,
                            left,
                            until_ms,
                        },
                        steering,
                    });
                    self.state.show(ai, slot, task, now_ms);
                }
                Command::StopMotion { slot } => {
                    if self.state.motion.is_some_and(|motion| motion.slot == slot) {
                        self.state.stop_moving();
                    }
                }
                Command::StatusEffect {
                    effect,
                    apply,
                    node,
                } => {
                    self.status_effect(ai, effect, apply, thought);
                    self.log_reset(ai, effect, apply, node, at, now_ms);
                }
                Command::LockTarget(locked) => self.state.locked = locked,
                Command::ManualTarget(point) => self.state.manual = point.map(position_of),
                Command::Action { node, target_index } => match &ai.data().node(node).kind {
                    NodeKind::ExecuteSpawn {
                        slice,
                        count,
                        position_key,
                        ..
                    } => {
                        let passive = self.passive(ai);
                        let view = self.state.view(ai, at, now_ms, passive);
                        let count = self
                            .brain
                            .resolve(&ai.runtime, &view, count)
                            .and_then(Value::as_number)
                            // A handful of adds.
                            .map_or(0, |count| count.max(0.0).round() as u32);
                        let at = position_key.and_then(|key| match self.brain.fact(key) {
                            Some(Value::Vec3(point)) => Some(position_of(point)),
                            _ => None,
                        });
                        thought.summons.push(Summon {
                            asset: slice.guid,
                            count,
                            at,
                        });
                    }
                    NodeKind::SelectWaypoint => {
                        if let Some(route) = &mut self.state.route {
                            route.advance();
                            route.visit(self.state.picks.unit());
                        }
                    }
                    NodeKind::TriggerEntity { trigger } => self.trigger(ai, *trigger, thought),
                    NodeKind::TriggerRemoteEntity {
                        trigger,
                        all_targets,
                        target,
                    } => {
                        let targets = self.state.remote_targets(
                            ai,
                            at,
                            others,
                            Some(target),
                            *all_targets,
                            target_index,
                        );
                        thought
                            .remote_triggers
                            .extend(targets.into_iter().map(|mob| (mob, *trigger)));
                    }
                    NodeKind::SetRemoteFact {
                        fact,
                        value,
                        overwrite,
                        target,
                    } => {
                        let value = {
                            let view = self.state.view(ai, at, now_ms, passive);
                            self.brain.resolve(&ai.runtime, &view, value)
                        };
                        let (Some(key), Some(value)) = (ai.data().key(ai.name(*fact)), value)
                        else {
                            continue;
                        };
                        // Inferred: the node names no `UseAllAvailableTargets`; it sets the fact
                        // on every target.
                        let targets = self.state.remote_targets(
                            ai,
                            at,
                            others,
                            Some(target),
                            true,
                            target_index,
                        );
                        thought
                            .remote_facts
                            .extend(targets.into_iter().map(|mob| RemoteFact {
                                mob,
                                key,
                                value,
                                overwrite: *overwrite,
                            }));
                    }
                    NodeKind::AssignOrder { order } => {
                        // Inferred: the node names no target; the order goes to every action
                        // target of its tracker.
                        let targets =
                            self.state
                                .remote_targets(ai, at, others, None, true, target_index);
                        thought
                            .orders
                            .extend(targets.into_iter().map(|mob| (mob, *order)));
                    }
                    NodeKind::CompleteOrder => self.state.order = None,
                    NodeKind::UpdateNwTags { main, operations } => {
                        if let Some(tags) = self
                            .state
                            .tags_after(std::iter::once(main).chain(operations))
                        {
                            self.state.nw_tags = tags;
                        }
                    }
                    other => tracing::warn!(
                        archetype = self.state.archetype,
                        node = ?std::mem::discriminant(other),
                        "tree action the sim does not run went out"
                    ),
                },
                Command::EncounterEvent(event) => {
                    tracing::debug!(
                        archetype = self.state.archetype,
                        event = ai.name(event),
                        "tree encounter event"
                    );
                    thought.encounter_events.push(event);
                }
                Command::TrackActionTargets(Some(tracker)) => self.state.trackers.push(tracker),
                Command::TrackActionTargets(None) => {
                    self.state.trackers.pop();
                }
                // The tags stay in the brain (the replicated tag state reads them).
                Command::Tag { .. } => {}
            }
        }
    }

    /// Runs the slice triggers named `trigger` of the mob's archetype: each joins `thought`.
    fn trigger(&self, ai: &MobAi, trigger: NameId, thought: &mut Thought) {
        let name = ai.name(trigger);
        let Some(arch) = ai.arch(self.state.archetype) else {
            return;
        };
        let before = thought.triggers.len();
        thought
            .triggers
            .extend(arch.triggers_named(name, |id| ai.name(id)).copied());
        if thought.triggers.len() == before {
            tracing::debug!(
                archetype = self.state.archetype,
                trigger = name,
                "trigger of no slice component"
            );
        }
    }

    /// Logs the start of a leash reset by tree node `node` (the live check of the leash): what
    /// the tree's leash conditions read then.
    fn log_reset(
        &self,
        ai: &MobAi,
        effect: NameId,
        apply: bool,
        node: NodeId,
        at: Position,
        now_ms: u64,
    ) {
        if !apply || (Some(effect) != ai.names.reset && Some(effect) != ai.names.reset_no_heal) {
            return;
        }
        let state = &self.state;
        tracing::debug!(
            archetype = state.archetype,
            node,
            since_reachable_ms = ?state.reachable_ms.map(|at_ms| now_ms.saturating_sub(at_ms)),
            from_spawn_m = at.horizontal_distance(state.spawn),
            from_waypoint_m = ?state.route.as_ref().and_then(RouteWalk::point).map(|point| at.horizontal_distance(point)),
            target_at = ?state.target_at,
            x = at.x,
            y = at.y,
            z = at.z,
            "mob leash reset"
        );
    }

    /// The tree applies (`apply`) or removes status effect `effect`. The sim keeps the effects
    /// for the `SE_` keys; a leash reset (`ResetAI`, `ResetAINoHeal`) ends the fight, and the
    /// end of `ResetAI` heals (see [`Thought::healed`]).
    fn status_effect(&mut self, ai: &MobAi, effect: NameId, apply: bool, thought: &mut Thought) {
        let state = &mut self.state;
        state.status_changes.push((effect, apply));
        if apply {
            if !state.statuses.contains(&effect) {
                state.statuses.push(effect);
            }
            if Some(effect) == ai.names.reset || Some(effect) == ai.names.reset_no_heal {
                // Inferred: the reset walks home and forgets the fight.
                state.target = None;
                state.threat.clear();
                state.last_damage = None;
            }
        } else {
            state.statuses.retain(|known| *known != effect);
            thought.healed |= Some(effect) == ai.names.reset;
        }
    }
}

/// The steering of a node that does not move.
const STANDING: Steering = Steering {
    max_speed_mps: 0.0,
    min_speed_mps: 0.0,
    acceleration_mps2: 0.0,
    deceleration_mps2: 0.0,
    turn_rate_deg_s: 0.0,
    arrival_m: 0.0,
};

/// The text value of parameter `name` of `window`.
fn param_of(window: &ProcWindow, name: Option<NameId>) -> Option<NameId> {
    let name = name?;
    window
        .params
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, value)| *value)
}

/// How the swing of a damage window reaches (Inferred: the joint the shape hangs on is Unknown
/// without the creature's skeleton): the shape reaches its radius plus its length (twice the
/// capsule's half height) in front of the body. The body itself moves in by the entry's
/// `Homing` clips ([`mob_homing`]), so the swing does not lunge at the hit; it faces a target
/// within the clip's `MaxAngle` (Observed: the boar's gore homing angle 180). Without a homing
/// clip it faces where it aimed.
fn window_reach(ai: &MobAi, window: &ProcWindow, homing: Option<&HomingClip>) -> MobReach {
    let number = |name: Option<NameId>| {
        param_of(window, name).and_then(|value| ai.name(value).trim().parse::<f32>().ok())
    };
    let radius = number(ai.names.shape_radius).unwrap_or(0.0);
    let half_height = number(ai.names.shape_half_height).unwrap_or(0.0);
    MobReach {
        lunge_max_m: 0.0,
        lunge_turn_rad: homing.map_or(0.0, |clip| clip.max_angle_rad),
        reach_m: radius + CAPSULE_ENDS * half_height,
    }
}

/// The `m_strafeActionParams` fields of a slice's navigation config that a strafe node leaves
/// unset (Observed: the undead pistoleer's `Strafe_Close` sets `IdealDistanceMin` 2.5 and
/// `IdealDistanceMax` 5.5 and no `LateralMovementFrequency`; its slice's `Strafe` config holds
/// `m_lateralMovementFrequencyMin` 1 and `Max` 3). Without them its strafe ended in one tick and
/// it stood in `Combat_Idle` between its shots beside a player (sim test 2026-11-12); retail it
/// shot a player 1 m away every 1 to 2 s (MSQ5-lv25 ledger, 423,090 to 426,024 ms).
const IDEAL_MIN_FIELD: &str = "m_idealDistanceMin";
const IDEAL_MAX_FIELD: &str = "m_idealDistanceMax";
const LATERAL_MIN_FIELD: &str = "m_lateralMovementFrequencyMin";
const LATERAL_MAX_FIELD: &str = "m_lateralMovementFrequencyMax";

/// The number of strafe field `field` in the slice navigation config `config` (its
/// `action_params`, by the lower-case CRC32 of the field name).
fn strafe_config_number(config: Option<&NavConfig>, field: &str) -> Option<f64> {
    let crc = nw_formats::objectstream::lowercase_crc32(field);
    config?
        .action_params
        .iter()
        .find(|(known, _)| *known == crc)
        .and_then(|(_, value)| value.as_number())
}

/// How far a normal mob chases from its spawn where the game data and the captures give no
/// value: 50 yards. Player's design choice 2026-11-12, used only where the game data and the
/// captures give no value (898 of the 922 archetypes take their slice's `CombatLeashDistance` or
/// their trees' literal; the other 24 are spell slices, hazards, catacomb bosses and gorillas).
const NORMAL_CHASE_M: f32 = 45.72;
/// How far a named mob chases: 75 yards. Player's design choice 2026-11-12, used only where the
/// game data and the captures give no value.
const NAMED_CHASE_M: f32 = 68.58;
/// How far an elite or boss mob chases: 100 yards. Player's design choice 2026-11-12, used only
/// where the game data and the captures give no value.
const ELITE_CHASE_M: f32 = 91.44;
/// The starts of the `CreatureType`s of the vitals rows of named mobs (Observed values:
/// `Named_Solo`, `Named_Solo+`).
const NAMED_TYPE: &str = "Named";
/// The `CreatureType`s of the vitals rows of elite and boss mobs (Observed values: `Elite`,
/// `Elite+`, `EliteBoss`, `Dungeon`, `DungeonBoss`, `Raid10`, `Catacombs`, `SoloBoss`, ...): the
/// group content and every boss.
const ELITE_TYPES: [&str; 4] = ["Elite", "Dungeon", "Raid", "Catacombs"];
const BOSS_TYPE: &str = "Boss";

/// The chase distance of a mob of vitals `CreatureType` `creature_type` where the data gives
/// none: elite and boss mobs [`ELITE_CHASE_M`], named mobs [`NAMED_CHASE_M`], every other
/// [`NORMAL_CHASE_M`] (player's design choice 2026-11-12).
pub fn chase_fallback_m(creature_type: Option<&str>) -> f32 {
    let kind = creature_type.unwrap_or_default();
    if kind.contains(BOSS_TYPE) || ELITE_TYPES.iter().any(|elite| kind.starts_with(elite)) {
        ELITE_CHASE_M
    } else if kind.starts_with(NAMED_TYPE) {
        NAMED_CHASE_M
    } else {
        NORMAL_CHASE_M
    }
}

/// Whether the data gives archetype `arch` its chase distance (`combat_leash`, the key
/// `CombatLeashDistance`): its slice's config holds the key, or a condition of its trees reads
/// the key with a literal beside it (`$CombatLeashDistance|70`, the base trees' leashes).
fn chase_from_data(data: &AiData, arch: &Archetype, combat_leash: Option<KeyId>) -> bool {
    let Some(key) = combat_leash else {
        return false;
    };
    if arch.config_value(key).is_some() {
        return true;
    }
    let mut open: Vec<NodeId> = arch
        .trees
        .iter()
        .filter_map(|tree| data.trees.get(tree.tree? as usize)?.root)
        .collect();
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = open.pop() {
        if !seen.insert(id) {
            continue;
        }
        let kind = &data.node(id).kind;
        let gates = match kind {
            NodeKind::BlackboardCondition { gates, .. }
            | NodeKind::DistanceCondition { gates, .. } => Some(&**gates),
            NodeKind::EntityQuery { params, .. } => Some(&params.gates),
            _ => None,
        };
        let literal = gates.is_some_and(|gates| {
            [&gates.entry, &gates.continuation, &gates.interrupt]
                .into_iter()
                .flat_map(|group| &group.conditions)
                .flat_map(|condition| [&condition.left, &condition.right])
                .any(|param| param.key == Some(key) && param.value.is_some())
        });
        if literal {
            return true;
        }
        open.extend(kind.child());
        open.extend(data.children(id).iter().copied());
    }
    false
}

/// The clip whose shape the hit of damage clip `window` swings: its own, or, when it names no
/// shape, the clip of the weapon's database `weapon` with the same `DamageTableRow` (Inferred:
/// the hit written in both databases, as [`MindState::open_windows`] takes it; the weapon's
/// clip holds the shape of its hit volume). Observed: the undead berserker's
/// `Berserk_Sword_Swing_Right` holds `Undead_1H_Sword_Slash` at 0.41 s without a shape in its
/// body's entry, and at 0.40 s with radius 0.25 and half height 0.35 in its weapon's. Without
/// the shape the Quartermaster of "A Most Mysterious Tablet" (archetype 662) missed a live
/// player 0.96 to 2.06 m away "out of reach" (VM sim log 2026-11-12 16:53:26 to 16:53:48);
/// retail, its hits landed 0.99 to 1.31 m away (`12a_msq_08_destroyrune` ledger, 108,777 to
/// 130,076 ms).
fn shaped<'a>(ai: &MobAi, window: &'a ProcWindow, weapon: &'a [ProcWindow]) -> &'a ProcWindow {
    if param_of(window, ai.names.shape_radius).is_some() {
        return window;
    }
    let row = param_of(window, ai.names.damage_row);
    weapon
        .iter()
        .find(|clip| {
            clip.kind == window.kind
                && param_of(clip, ai.names.damage_row) == row
                && param_of(clip, ai.names.shape_radius).is_some()
        })
        .unwrap_or(window)
}

/// A capsule's length is twice its half height.
const CAPSULE_ENDS: f32 = 2.0;

fn position_of(point: [f32; 3]) -> Position {
    Position {
        x: point[0],
        y: point[1],
        z: point[2],
    }
}

fn vec3(at: Position) -> Value {
    Value::Vec3([at.x, at.y, at.z])
}

/// A blackboard with nothing set (a navigation config's own pick reads only its literals).
struct NoBoard;

impl Blackboard for NoBoard {
    fn value(&self, _key: KeyId) -> Option<Value> {
        None
    }

    fn dynamic_scale(&self) -> Option<f64> {
        None
    }
}

/// The world as one mob's tree reads it.
struct View<'a> {
    ai: &'a MobAi,
    state: &'a MindState,
    at: Position,
    now_ms: u64,
    passive: bool,
    target: Option<TargetView>,
    candidates: u32,
    health_share: f32,
    ground: Option<&'a (dyn Ground + Sync)>,
    /// The random numbers of position picks (the tree's own draws are in its brain).
    picks: Cell<Rng48>,
    /// The other mobs its remote actions reach.
    others: Others<'a>,
    /// The living players near it ([`Surroundings::seen`]).
    seen: &'a [Seen],
    /// The status effects the sim holds on it.
    held: Option<&'a HeldEffects>,
}

impl View<'_> {
    fn draw(&self) -> f32 {
        let mut rng = self.picks.get();
        let unit = rng.unit();
        self.picks.set(rng);
        unit
    }

    fn max_slope(&self) -> Option<f32> {
        self.ai.arch(self.state.archetype)?.max_slope_deg
    }
}

impl World for View<'_> {
    fn now_ms(&self) -> u64 {
        self.now_ms
    }

    fn engine_value(&self, key: EngineKey) -> Option<Value> {
        let state = self.state;
        let target = self.target;
        Some(match key {
            EngineKey::HasTarget => Value::Bool(target.is_some()),
            EngineKey::TargetScore => {
                target?;
                Value::Float(HELD_TARGET_SCORE)
            }
            EngineKey::TargetDist => Value::Float(target?.distance_m),
            EngineKey::TargetAngle => Value::Float(target?.angle_deg),
            EngineKey::TargetPos | EngineKey::TargetLastKnownPos => vec3(target?.position),
            EngineKey::TargetVisible => Value::Bool(target.is_some_and(|target| target.visible)),
            EngineKey::TargetReachable | EngineKey::TargetDirectlyReachable => {
                Value::Bool(target.is_some_and(|target| target.reachable))
            }
            EngineKey::TargetVitalsState => {
                target?;
                Value::Int(ALIVE_VITALS_STATE)
            }
            EngineKey::VitalsState => Value::Int(ALIVE_VITALS_STATE),
            EngineKey::TimeSinceTargetReachable => {
                target?;
                let since = state
                    .reachable_ms
                    .map_or(self.now_ms, |at| self.now_ms.saturating_sub(at));
                // Seconds of a fight.
                #[allow(clippy::cast_precision_loss, reason = "seconds of a fight")]
                Value::Float(since as f32 / MS_PER_S)
            }
            EngineKey::HealthPercent => Value::Float(self.health_share * PERCENT),
            EngineKey::SpawnPos => vec3(state.spawn),
            EngineKey::LastDamagePos => vec3(state.last_damage?),
            EngineKey::FullBodyReaction => Value::Bool(state.reaction_until_ms.is_some()),
            EngineKey::Passive => Value::Int(i64::from(self.passive)),
            EngineKey::WaypointPos => vec3(state.route.as_ref()?.point()?),
            EngineKey::WaypointDuration => Value::Float(state.route.as_ref()?.wait_s()),
            EngineKey::WaypointTask => Value::Text(state.route.as_ref()?.task()?.task),
            EngineKey::WaypointDurationMin => {
                Value::Float(state.route.as_ref()?.task()?.duration_s[0])
            }
            EngineKey::WaypointDurationMax => {
                Value::Float(state.route.as_ref()?.task()?.duration_s[1])
            }
            EngineKey::WaypointAlign => Value::Bool(state.route.as_ref()?.task()?.align),
            EngineKey::LeashOptOut => {
                let opt_out = self
                    .ai
                    .config_number(state.archetype, LEASH_OPT_OUT_PARAM)
                    .unwrap_or(0.0);
                // An opt-out of 0 or 1.
                #[allow(clippy::cast_possible_truncation, reason = "an opt-out of 0 or 1")]
                Value::Int(opt_out.round() as i64)
            }
            EngineKey::DifficultyTier => {
                let tier = self
                    .ai
                    .config_number(state.archetype, DIFFICULTY_TIER_PARAM)?;
                // A tier of 0 to 3.
                #[allow(clippy::cast_possible_truncation, reason = "a tier of 0 to 3")]
                Value::Int(tier.round() as i64)
            }
            EngineKey::AiOrder => Value::Int(i64::from(state.order?)),
            EngineKey::GuardObjectivePos => Value::Vec3(state.guard_objective?),
            // The engine's leash: the fallback of a mob whose data gives none; else the slice
            // config and the trees' literals hold it.
            EngineKey::CombatLeashDistance => Value::Float(state.chase_fallback_m?),
            _ => return None,
        })
    }

    fn has_status_effect(&self, effect: &str) -> bool {
        self.state
            .statuses
            .iter()
            .any(|status| self.ai.name(*status) == effect)
            || self.held.is_some_and(|held| {
                held.effects
                    .iter()
                    .any(|held| held.eq_ignore_ascii_case(effect))
            })
    }

    fn has_status_category(&self, category: &str) -> bool {
        self.held.is_some_and(|held| {
            held.categories
                .iter()
                .any(|held| held.eq_ignore_ascii_case(category))
        })
    }

    fn position(&self, of: PositionType) -> Option<[f32; 3]> {
        let state = self.state;
        let at = match of {
            PositionType::Own => self.at,
            PositionType::Target | PositionType::LastKnownTarget | PositionType::ActionTarget => {
                self.target?.position
            }
            PositionType::SpawnPosition => state.spawn,
            PositionType::LastDamagePosition => state.last_damage?,
            PositionType::WaypointPosition => state.route.as_ref()?.point()?,
            PositionType::GuardObjectivePosition => return state.guard_objective,
            // Group, attack objective, nav link and above-start positions: the sim has no such
            // data (a selected position is a tree fact, read through the blackboard).
            PositionType::SelectedPosition
            | PositionType::GroupCenter
            | PositionType::AttackObjectivePosition
            | PositionType::AboveStartPosition
            | PositionType::NavLinkEndPosition => return None,
        };
        Some([at.x, at.y, at.z])
    }

    fn action_targets(&self, params: &TrackerParams) -> u32 {
        // Inferred: without a tag (or with `Player`), the players the mob could take as targets
        // within its combat vision (the targeting profile's curves are Unknown); with another
        // tag, the other mobs that carry it.
        let tag = [params.tag_filter, params.target.tag_filter]
            .into_iter()
            .find(|tag| !self.ai.name(*tag).is_empty());
        match tag {
            Some(tag) if self.ai.name(tag) != PLAYER_TAG => {
                let others = self.others;
                let tagged = others
                    .tagged
                    .iter()
                    .filter(|mob| mob.entity != others.entity && mob.tags.contains(&tag))
                    .count();
                u32::try_from(tagged).unwrap_or(u32::MAX)
            }
            _ => self.candidates,
        }
    }

    fn entity_query(&self, params: &EntityQueryParams, board: &dyn Blackboard) -> bool {
        // Inferred from the three query nodes of the data ("Check Enemies Dead": `Player`,
        // vitals state 4, radius 0 and 200; "Check For Players": `Player`, radius 100; all with
        // the entry condition `Unused Equal 0`): the node counts the entities with `TagFilter`
        // within `Radius` of `Center` (the mob without one) in `MatchingVitalsState` and not in
        // `NotMatchingVitalsState`, and its entry conditions compare that count (their left
        // operand is the placeholder text `Unused`) with their right operand. A state of 6
        // (past the last state: both fields of "Check For Players" hold it) is no filter, and a
        // radius of 0 is the node's limit of 200 m. The threat filter is 2 with an empty text
        // in every node (Inferred: no filter). Only living entities are known here, so a query
        // for another state than alive finds none (Unknown: the states of the dead).
        let state = |param: &Param| match param.resolve(&board) {
            Some(Value::Int(state)) if state != NO_VITALS_STATE_FILTER => Some(state),
            _ => None,
        };
        let wants_alive = state(&params.matching_vitals_state)
            .is_none_or(|state| state == ALIVE_VITALS_STATE)
            && state(&params.not_matching_vitals_state)
                .is_none_or(|state| state != ALIVE_VITALS_STATE);
        let centre = match params.center.resolve(&board) {
            Some(Value::Vec3([x, y, z])) => Position { x, y, z },
            _ => self.at,
        };
        // A radius of a few hundred metres.
        #[allow(clippy::cast_possible_truncation, reason = "a radius of at most 200 m")]
        let radius_m = params
            .radius
            .number(&board)
            .map(|radius| radius as f32)
            .filter(|radius| *radius > 0.0)
            .map_or(ENTITY_QUERY_MAX_RADIUS_M, |radius| {
                radius.min(ENTITY_QUERY_MAX_RADIUS_M)
            });
        let within = |at: Position| at.horizontal_distance(centre) <= radius_m;
        let tag = self.ai.name(params.tag_filter);
        let found = if !wants_alive {
            0
        } else if tag.is_empty() || tag == PLAYER_TAG {
            self.seen
                .iter()
                .filter(|player| within(player.position))
                .count()
        } else {
            self.others
                .tagged
                .iter()
                .filter(|mob| {
                    mob.entity != self.others.entity
                        && within(mob.position)
                        && mob.tags.iter().any(|own| self.ai.name(*own) == tag)
                })
                .count()
        };
        let found = Value::Int(i64::try_from(found).unwrap_or(i64::MAX));
        let entry = &params.gates.entry;
        if entry.is_empty() {
            // Inferred: a query with no condition passes when it finds an entity.
            return found != Value::Int(0);
        }
        entry.holds_for(found, &board)
    }

    fn entities_with_tag(&self, tag: &str) -> u32 {
        // Only the tags trees gave (`UpdateNWTags`) are known: the slices' own `NWTags` are not
        // in the AI data (Unknown).
        let tagged = self
            .others
            .tagged
            .iter()
            .filter(|mob| mob.tags.iter().any(|own| self.ai.name(*own) == tag))
            .count();
        u32::try_from(tagged).unwrap_or(u32::MAX)
    }

    fn can_act(&self, kind: &NodeKind, target_index: Option<u32>) -> bool {
        let state = self.state;
        let reaches = |target: Option<&ActionTarget>| {
            !state
                .remote_targets(self.ai, self.at, self.others, target, true, target_index)
                .is_empty()
        };
        match kind {
            NodeKind::ExecuteSpawn { .. }
            | NodeKind::TriggerEntity { .. }
            | NodeKind::SelectWaypoint => true,
            NodeKind::TriggerRemoteEntity { target, .. } => reaches(Some(target)),
            NodeKind::SetRemoteFact { fact, target, .. } => {
                self.ai.data().key(self.ai.name(*fact)).is_some() && reaches(Some(target))
            }
            NodeKind::AssignOrder { .. } => reaches(None),
            NodeKind::CompleteOrder => state.order.is_some(),
            NodeKind::UpdateNwTags { main, operations } => state
                .tags_after(std::iter::once(main).chain(operations))
                .is_some(),
            // Unknown: the node names no projectile and no beam; they are components of the
            // mob's slice (`ProjectileSpawner`, the beam attack of `BeamName`), which the AI data
            // does not hold, so the sim fires neither and the node fails.
            NodeKind::SpawnProjectile(_) | NodeKind::SetBeam { .. } => false,
            _ => false,
        }
    }

    fn has_patrol(&self) -> bool {
        self.state.route.is_some()
    }

    fn line_of_sight(&self, _from: [f32; 3], _to: [f32; 3]) -> bool {
        // The server has no level geometry to block a line (Inferred: nothing blocks).
        true
    }

    fn nav_clearance(&self, direction_deg: f64, distance: f64, _from_wall: f64) -> bool {
        let Some(ground) = self.ground else {
            return true;
        };
        // Metres of a node.
        #[allow(clippy::cast_possible_truncation, reason = "metres of a node")]
        let end = ahead(
            self.at,
            turned(self.state.yaw, direction_deg),
            distance as f32,
        );
        floor_line(ground, self.at, end, self.max_slope()).is_some()
    }

    fn select_position(
        &self,
        params: &SelectPositionParams,
        board: &dyn Blackboard,
    ) -> Option<[f32; 3]> {
        let (origin, anchor) = (
            position_of(self.position(params.origin)?),
            position_of(self.position(params.anchor)?),
        );
        let number = |plain: &Param, keyed: &Param| {
            if keyed.is_unset() {
                plain.number(&board)
            } else {
                keyed.number(&board)
            }
        };
        let direction = params.direction.number(&board).unwrap_or(0.0);
        // A small whole number.
        #[allow(clippy::cast_possible_truncation, reason = "a small whole number")]
        let base = match direction as i64 {
            ANCHOR_TO_ORIGIN => yaw_toward(anchor, origin),
            ORIGIN_ORIENTATION => self.state.yaw,
            _ => yaw_toward(origin, anchor),
        };
        let start = number(&params.start_angle, &params.start_angle_param).unwrap_or(0.0);
        let stop = number(&params.stop_angle, &params.stop_angle_param).unwrap_or(0.0);
        let span = (stop - start).rem_euclid(FULL_TURN_DEG);
        let range_min = number(&params.range_min, &params.range_min_param).unwrap_or(0.0);
        let range_max = number(&params.range_max, &params.range_max_param).unwrap_or(0.0);
        let attempts = number(&params.max_attempts, &params.max_attempts_param)
            .unwrap_or(1.0)
            .max(1.0);
        // A few attempts.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a few attempts"
        )]
        let attempts = attempts as u32;
        for _ in 0..attempts {
            let angle = start + f64::from(self.draw()) * span;
            let range = range_min + f64::from(self.draw()) * (range_max - range_min);
            // Metres of a pick.
            #[allow(clippy::cast_possible_truncation, reason = "metres of a pick")]
            let point = ahead(origin, turned(base, angle), range as f32);
            let Some(ground) = self.ground else {
                return Some([point.x, point.y, point.z]);
            };
            // A pick that checks no path takes any floor point (Observed: `CheckPathToOrigin`
            // and `CheckPathToAnchor`); the move then walks a way to it. Requiring a straight
            // floor line from the mob made the beach sword sailor's `ResetToWaypoint` (node
            // 152, origin its waypoint, no path check) fail 25 m out: it played
            // `Combat_Idle` and `Combat_Fidget` again and again and never walked home (retail
            // test `the_beach_sword_sailor_walks_home_from_every_side`, bearing 180 degrees).
            // The point stands as a body moved there stands ([`height_after_move`]): on the
            // floor near its height, else at its own height where the level has no height data,
            // as the floor line keeps it. Requiring a floor there made every pick fail where the
            // level has no height data: a leash reset's walk home to the spawn failed at once and
            // the mob stood past its leash (sim test `a_returning_mob_is_refused`, 2026-11-12).
            if !(params.check_path_to_origin || params.check_path_to_anchor) {
                let z = height_after_move(ground, point);
                return Some([point.x, point.y, z]);
            }
            if let Some(reached) = floor_line(ground, self.at, point, self.max_slope())
                .filter(|reached| reached.horizontal_distance(point) < FLOOR_STEP_M)
            {
                return Some([reached.x, reached.y, reached.z]);
            }
        }
        None
    }
}

/// A small test AI (feature `test-util`): one archetype whose tree walks home in a leash reset
/// when it is farther than its leash from its spawn, fights a target in reach, runs to a target
/// out of reach, and else stands.
#[cfg(any(test, feature = "test-util"))]
pub mod test_support {
    use nw_ai::build::{Build, run_config};
    use nw_ai::data::{
        AiData, BlockActivity, BlockEffect, CageAction, CageCondition, Creature, FragmentLength,
        FragmentVariant, ProcWindow, ProjectileSlice, RangedAttack, TagActivity, TagEffect, Vision,
    };
    use nw_ai::node::PositionType;
    use nw_ai::value::{CompareOp, Value};
    use nw_content::player_attacks::Flight;
    use nw_formats::objectstream::lowercase_crc32;

    use super::{
        AiRates, COMBAT_LEASH_KEY, DAMAGE_CLIP, DAMAGE_ROW_PARAM, DEATH_ALIAS, LookAssets, MobAi,
        REACTION_ALIAS, RUN_CONFIG, SHAPE_HALF_HEIGHT_PARAM, SHAPE_RADIUS_PARAM,
    };
    use crate::mob_homing::{
        HOMING_CLIP, MAX_ANGLE_PARAM, MOVE_TO_DISTANCE_PARAM, MOVE_TO_DURATION_PARAM,
        MOVE_TO_TARGET_PARAM, RADIUS_PARAM, TURN_RATE_PARAM,
    };

    /// The slice asset of the test archetype.
    pub const TEST_ASSET: [u8; 16] = [0x7e; 16];
    /// The test tree attacks a target this near.
    pub const TEST_REACH_M: f32 = 3.0;
    /// It sees a player this far, all around.
    pub const TEST_SIGHT_M: f32 = 20.0;
    const FULL_CIRCLE_DEG: f32 = 360.0;
    /// Its combat leash.
    pub const TEST_LEASH_M: f32 = 50.0;
    /// It shares a target it takes with mobs this near, at most this many.
    pub const TEST_GROUP_AGGRO_M: f32 = 10.0;
    /// The most mobs it shares a target with.
    pub const TEST_GROUP_AGGRO_CAP: u32 = 2;
    /// Its attack: the task, the length, the cooldown after it and its damage window.
    pub const TEST_ATTACK_TASK: &str = "Attack_Task";
    /// The length of its attack.
    pub const TEST_ATTACK_S: f32 = 1.5;
    /// The cooldown after its attack.
    pub const TEST_COOLDOWN_S: f32 = 2.0;
    /// When its damage window comes after the attack starts.
    pub const TEST_HIT_S: f32 = 0.5;
    /// The damage table and row of its hits.
    pub const TEST_MOB_TABLE: &str = "TestMobTable";
    /// The row of its hits.
    pub const TEST_MOB_ROW: &str = "Test_Mob_Attack";
    /// Its homing: radius, angle, turn rate (degrees per second) and its move to 1 m from the
    /// target in 0.3 s.
    const TEST_HOMING_RADIUS_M: &str = "4";
    const TEST_HOMING_ANGLE_DEG: &str = "180";
    const TEST_HOMING_TURN_DEG_S: &str = "1080";
    /// How near its homing draws it to its target.
    pub const TEST_HOMING_DISTANCE_M: f32 = 1.0;
    const TEST_HOMING_DURATION_S: &str = "0.3";
    const TEST_SHAPE_RADIUS_M: &str = "0.4";
    const TEST_SHAPE_HALF_HEIGHT_M: &str = "0.2";
    const TEST_HIT_LENGTH_S: f32 = 0.2;
    /// Its run, its combat idle and its idle tasks.
    pub const TEST_RUN_TASK: &str = "Move_Combat_Task";
    /// Its combat idle task.
    pub const TEST_COMBAT_IDLE_TASK: &str = "Combat_Idle_Task";
    /// Its idle task.
    pub const TEST_IDLE_TASK: &str = "Idle_Task";
    /// Its walk home in a leash reset.
    pub const TEST_RESET_TASK: &str = "Reset_Move_Task";
    const TEST_REACTION_S: f32 = 0.8;
    const TEST_DEATH_S: f32 = 2.0;
    const TRUE: f32 = 1.0;
    /// A ranged attack of the data: its profile, its name and its projectile slice.
    pub const TEST_RANGED_PROFILE: &str = "AI_Test";
    /// The name of the test ranged attack.
    pub const TEST_RANGED_ATTACK: &str = "Test_Shot";
    /// The projectile slice of the test ranged attack.
    pub const TEST_SHOT_SLICE: &str = "slices/characters/spellslices/test_shot.dynamicslice";
    /// The test ranged attack's range and radius.
    pub const TEST_SHOT_RANGE_M: f32 = 10.0;
    /// The radius of the test projectile.
    pub const TEST_SHOT_RADIUS_M: f32 = 0.5;
    /// The test projectile flies straight at this speed.
    pub const TEST_SHOT_SPEED_MPS: f32 = 20.0;
    const TEST_SHOT_LIFETIME_S: f32 = 5.0;

    /// The data of the test archetype.
    pub fn test_ai_data() -> AiData {
        let mut build = Build::default();
        let damage = build.name(DAMAGE_CLIP);
        let homing = build.name(HOMING_CLIP);
        let param =
            |build: &mut Build, name: &str, value: &str| (build.name(name), build.name(value));
        let windows = vec![
            ProcWindow {
                kind: homing,
                start_s: 0.0,
                end_s: Some(TEST_HIT_S),
                params: vec![
                    param(&mut build, RADIUS_PARAM, TEST_HOMING_RADIUS_M),
                    param(&mut build, MAX_ANGLE_PARAM, TEST_HOMING_ANGLE_DEG),
                    param(&mut build, TURN_RATE_PARAM, TEST_HOMING_TURN_DEG_S),
                    param(
                        &mut build,
                        MOVE_TO_TARGET_PARAM,
                        crate::mob_homing::PARAM_TRUE,
                    ),
                    param(
                        &mut build,
                        MOVE_TO_DISTANCE_PARAM,
                        &TEST_HOMING_DISTANCE_M.to_string(),
                    ),
                    param(&mut build, MOVE_TO_DURATION_PARAM, TEST_HOMING_DURATION_S),
                ],
            },
            ProcWindow {
                kind: damage,
                start_s: TEST_HIT_S,
                end_s: Some(TEST_HIT_S + TEST_HIT_LENGTH_S),
                params: vec![
                    param(&mut build, DAMAGE_ROW_PARAM, TEST_MOB_ROW),
                    param(&mut build, SHAPE_RADIUS_PARAM, TEST_SHAPE_RADIUS_M),
                    param(
                        &mut build,
                        SHAPE_HALF_HEIGHT_PARAM,
                        TEST_SHAPE_HALF_HEIGHT_M,
                    ),
                ],
            },
        ];
        build.action(
            TEST_ATTACK_TASK,
            FragmentLength::Seconds(TEST_ATTACK_S + nw_ai::build::COMPLETION_OFFSET_S),
            Vec::new(),
            windows,
        );
        let attack_params = build.cage_params(TEST_ATTACK_TASK);
        let attack = build.node(nw_ai::node::NodeKind::Cage(Box::new(attack_params)));
        let cooled = build.cooldown(
            super::ATTACK_COOLDOWN,
            TEST_COOLDOWN_S,
            TEST_COOLDOWN_S,
            attack,
        );
        let near = build.gate(
            "Target_Dist",
            CompareOp::LessThanOrEqual,
            TEST_REACH_M,
            cooled,
        );
        let run = build.navigation(TEST_RUN_TASK, Some(RUN_CONFIG));
        let far = build.gate("Target_Dist", CompareOp::GreaterThan, TEST_REACH_M, run);
        let combat_idle = build.idle(TEST_COMBAT_IDLE_TASK);
        let fight = build.selector(&[near, far, combat_idle]);
        let combat = build.gate("Has_Target", CompareOp::Equal, TRUE, fight);
        let leash_key = build.key(COMBAT_LEASH_KEY);
        let home = build.navigation_to(
            TEST_RESET_TASK,
            Some(RUN_CONFIG),
            PositionType::SpawnPosition,
        );
        // The walk home finishes before the tree plans again (as DistanceLeash's).
        build.data.nodes[home as usize].interruptable = false;
        let reset = build.status_effect(super::RESET_EFFECT, home);
        let leash_param = nw_ai::value::Param {
            key: Some(leash_key),
            value: Some(Value::Float(TEST_LEASH_M)),
        };
        let leash = build.farther_than("Spawn_Pos", leash_param, reset);
        let idle = build.idle(TEST_IDLE_TASK);
        let root = build.selector(&[leash, combat, idle]);
        build.action(
            REACTION_ALIAS,
            FragmentLength::Seconds(TEST_REACTION_S),
            Vec::new(),
            Vec::new(),
        );
        build.action(
            DEATH_ALIAS,
            FragmentLength::Seconds(TEST_DEATH_S),
            Vec::new(),
            Vec::new(),
        );
        let run_kind = build.name(RUN_CONFIG);
        // The keys the design overrides set, as the retail trees read them.
        for key in [
            super::IS_PASSIVE_KEY,
            super::ENABLE_WANDER_KEY,
            super::WANDER_MAX_KEY,
            super::AMBIENT_LEASH_KEY,
        ] {
            build.key(key);
        }

        let table = build.name(TEST_MOB_TABLE);
        let ranged_attacks = vec![RangedAttack {
            profile: build.name(TEST_RANGED_PROFILE),
            attack: build.name(TEST_RANGED_ATTACK),
            range_m: TEST_SHOT_RANGE_M,
            radius_m: TEST_SHOT_RADIUS_M,
            hit_scan: false,
        }];
        let projectile_slices = vec![ProjectileSlice {
            slice: build.name(TEST_SHOT_SLICE),
            flight: Flight {
                base_speed_mps: TEST_SHOT_SPEED_MPS,
                extra_speed_mps: 0.0,
                time_for_max_speed_s: 0.0,
                gravity_mps2: 0.0,
                lifetime_s: TEST_SHOT_LIFETIME_S,
            },
        }];
        let mut data = build.finish(root);
        data.ranged_attacks = ranged_attacks;
        data.projectile_slices = projectile_slices;
        let reaction = data.name_id(REACTION_ALIAS).expect("reaction");
        let death = data.name_id(DEATH_ALIAS).expect("death");
        let mut aliases = vec![(reaction, reaction), (death, death)];
        aliases.sort_unstable();
        data.grids[0].aliases = aliases;
        add_guard_actions(&mut data);
        let arch = &mut data.archetypes[0];
        let vision = Vision {
            angle_deg: FULL_CIRCLE_DEG,
            distance_m: TEST_SIGHT_M,
        };
        arch.ambient_vision = Some(vision);
        arch.nav_configs.push(run_config(run_kind));
        arch.config.push((leash_key, Value::Float(TEST_LEASH_M)));
        arch.damage_table_id = Some(table);
        arch.group_aggro_distance = Some(TEST_GROUP_AGGRO_M);
        arch.group_aggro_cap = Some(TEST_GROUP_AGGRO_CAP);
        data.creatures.push(Creature {
            asset: TEST_ASSET,
            archetype: 0,
        });
        data
    }

    /// Picks that give their origin exactly (every field a test does not set).
    pub fn exact_pick() -> nw_ai::node::SelectPositionParams {
        let zero = nw_ai::value::Param::literal(Value::Float(0.0));
        let unset = nw_ai::value::Param::UNSET;
        nw_ai::node::SelectPositionParams {
            origin: PositionType::Own,
            anchor: PositionType::Own,
            direction: unset,
            mode: unset,
            check_path_to_origin: false,
            check_path_to_anchor: false,
            start_angle: zero,
            stop_angle: zero,
            favor_middle_angle: false,
            range_min: zero,
            range_max: zero,
            distance_from_wall: unset,
            query_radius: unset,
            max_attempts: unset,
            start_angle_param: unset,
            stop_angle_param: unset,
            range_min_param: unset,
            range_max_param: unset,
            distance_from_wall_param: unset,
            query_radius_param: unset,
            max_attempts_param: unset,
            group_detector: unset,
            group_range: unset,
        }
    }

    /// How long the test `BlockBroken` holds its tag (retail `damned_block.actionlist`: 8 s).
    pub const TEST_BLOCK_BROKEN_S: f32 = 8.0;
    /// How long the test `BrokenGuard` reaction plays.
    pub const TEST_BROKEN_GUARD_S: f32 = 1.0;

    /// The block actions of the retail `damned_block.actionlist`, trimmed: the engine runs
    /// `Block` (raises the block and sets `Block` while `BlockBroken` is off; a mob without a
    /// guard never blocks) and `BlockBroken` (unsets its tag after 8 s, holds the regen and
    /// fills the stamina at its end); `BrokenGuard` is the reaction that sets `BlockBroken`.
    fn add_guard_actions(data: &mut AiData) {
        let mut name = |text: &str| {
            data.names.push(text.to_owned());
            u32::try_from(data.names.len() - 1).expect("name id")
        };
        let (block, broken, broken_guard) = (
            name("Block"),
            name("BlockBroken"),
            name(super::BROKEN_GUARD_ACTION),
        );
        let list = name("damned_block.actionlist");
        let not_broken = CageCondition::Not(Box::new(CageCondition::Tag(broken)));
        let action = |name: u32, length: FragmentLength| CageAction {
            list,
            action: name,
            fragment: name,
            completion: None,
            tags: Vec::new(),
            triggers: Vec::new(),
            motion_multiplier: None,
            facing: None,
            variants: vec![FragmentVariant {
                tags: Vec::new(),
                frag_tags: Vec::new(),
                length,
                windows: Vec::new(),
            }],
            weapon_variants: Vec::new(),
            activation: None,
            deactivation: None,
            blocks: Vec::new(),
            regen_off: None,
            stamina_reset: None,
            grit_on: None,
            fragment_cases: Vec::new(),
        };
        let tag = |tag: u32, on_enter, on_exit, delay_s| TagActivity {
            condition: CageCondition::True,
            tag,
            on_enter,
            on_exit,
            delay_s,
        };
        let actions = [
            (
                block,
                CageAction {
                    activation: Some(not_broken),
                    deactivation: Some(CageCondition::Tag(broken)),
                    blocks: vec![BlockActivity {
                        condition: CageCondition::True,
                        on_enter: BlockEffect::Raise,
                        on_exit: BlockEffect::Lower,
                    }],
                    tags: vec![tag(block, TagEffect::Set, TagEffect::Clear, 0.0)],
                    ..action(block, FragmentLength::Loops)
                },
            ),
            (
                broken,
                CageAction {
                    activation: Some(CageCondition::Tag(broken)),
                    tags: vec![tag(
                        broken,
                        TagEffect::Clear,
                        TagEffect::NoEffect,
                        TEST_BLOCK_BROKEN_S,
                    )],
                    regen_off: Some(CageCondition::True),
                    stamina_reset: Some(CageCondition::True),
                    ..action(broken, FragmentLength::Loops)
                },
            ),
            (
                broken_guard,
                CageAction {
                    tags: vec![tag(broken, TagEffect::Set, TagEffect::NoEffect, 0.0)],
                    ..action(broken_guard, FragmentLength::Seconds(TEST_BROKEN_GUARD_S))
                },
            ),
        ];
        for (name, cage) in actions {
            let index = u32::try_from(data.cage_actions.len()).expect("action index");
            data.cage_actions.push(cage);
            data.grids[0].actions.push((name, index));
        }
        data.grids[0].actions.sort_unstable();
    }

    /// How long the test `r_Stagger` plays.
    pub const TEST_STAGGER_S: f32 = 6.0;

    /// The test AI data whose slice has the stagger of the retail `baseai_stagger.actionlist`,
    /// trimmed: its grit is its stamina and it spawns with `AI_Enable_Stagger`; the engine runs
    /// `ActivateGrit` (grit on while it holds the effect and `r_Stagger` does not play), and
    /// `Reaction_Staggered` plays `r_Stagger` with the `Stunned` tag.
    pub fn test_stagger_ai_data() -> AiData {
        let mut data = test_ai_data();
        let mut name = |text: &str| {
            data.names.push(text.to_owned());
            u32::try_from(data.names.len() - 1).expect("name id")
        };
        let (grit, stagger, fragment, enable, stunned) = (
            name("ActivateGrit"),
            name(super::STAGGER_ACTION),
            name("r_Stagger"),
            name(super::ENABLE_STAGGER_EFFECT),
            name("Stunned"),
        );
        name(super::STAGGER_TRACKER_EFFECT);
        let list = name("baseai_stagger.actionlist");
        let playing = CageCondition::FragmentPlaying(fragment);
        let plain = |name: u32, fragment: u32, length: FragmentLength| CageAction {
            list,
            action: name,
            fragment,
            completion: None,
            tags: Vec::new(),
            triggers: Vec::new(),
            motion_multiplier: None,
            facing: None,
            variants: vec![FragmentVariant {
                tags: Vec::new(),
                frag_tags: Vec::new(),
                length,
                windows: Vec::new(),
            }],
            weapon_variants: Vec::new(),
            activation: None,
            deactivation: None,
            blocks: Vec::new(),
            regen_off: None,
            stamina_reset: None,
            grit_on: None,
            fragment_cases: Vec::new(),
        };
        let actions = [
            (
                grit,
                CageAction {
                    activation: Some(CageCondition::All(vec![
                        CageCondition::StatusEffect(enable),
                        CageCondition::Not(Box::new(playing.clone())),
                    ])),
                    deactivation: Some(playing),
                    grit_on: Some(CageCondition::True),
                    variants: Vec::new(),
                    ..plain(grit, grit, FragmentLength::Unknown)
                },
            ),
            (
                stagger,
                CageAction {
                    tags: vec![TagActivity {
                        condition: CageCondition::True,
                        tag: stunned,
                        on_enter: TagEffect::Set,
                        on_exit: TagEffect::Clear,
                        delay_s: 0.0,
                    }],
                    ..plain(stagger, fragment, FragmentLength::Seconds(TEST_STAGGER_S))
                },
            ),
        ];
        for (name, cage) in actions {
            let index = u32::try_from(data.cage_actions.len()).expect("action index");
            data.cage_actions.push(cage);
            data.grids[0].actions.push((name, index));
        }
        data.grids[0].actions.sort_unstable();
        for arch in &mut data.archetypes {
            arch.grit_uses_stamina = true;
            arch.initial_effects = vec![enable];
        }
        data
    }

    /// The test AI with the retail tick rates; the mobs of `vitals_ids` show the test slice.
    pub fn test_ai(vitals_ids: &[&str]) -> MobAi {
        test_ai_with(test_ai_data(), vitals_ids)
    }

    /// The AI of `data` with the retail tick rates; the mobs of `vitals_ids` show the test
    /// slice.
    pub fn test_ai_with(data: AiData, vitals_ids: &[&str]) -> MobAi {
        let looks: LookAssets = vitals_ids
            .iter()
            .map(|id| (lowercase_crc32(id), TEST_ASSET))
            .collect();
        MobAi::new(data, AiRates::default(), looks)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nw_ai::data::ProcWindow;
    use nw_content::combat::{GritBody, LARGE_BODY_GRIT_SHARE};
    use nw_sim_core::test_support::FlatGround;
    use nw_world::records::SpawnAi as DesignSpawn;

    use super::test_support::{TEST_ASSET, test_ai, test_ai_data};
    use super::*;

    const GROUND_Z: f32 = 10.0;
    const START: Position = Position {
        x: 100.0,
        y: 100.0,
        z: GROUND_Z,
    };
    const TEST_MOB: &str = "Test_Mob";

    /// A ground that rises `rise` metres per metre east of `START`.
    struct Ramp {
        rise: f32,
    }

    impl Ground for Ramp {
        fn height_at(&self, x: f32, _: f32) -> Option<f32> {
            Some(GROUND_Z + (x - START.x).max(0.0) * self.rise)
        }
    }

    fn east(metres: f32) -> Position {
        Position {
            x: START.x + metres,
            ..START
        }
    }

    #[test]
    fn a_reach_answer_holds_until_the_mob_or_its_target_moves_a_floor_step() {
        let check = ReachCheck {
            from: START,
            to: east(20.0),
            reachable: false,
            recheck_m: REACH_RECHECK_M,
        };
        let short = REACH_RECHECK_M * 0.9;
        assert_eq!(check.answer(east(short), east(20.0 - short)), Some(false));
        assert_eq!(check.answer(east(REACH_RECHECK_M), east(20.0)), None);
        assert_eq!(check.answer(START, east(20.0 + REACH_RECHECK_M)), None);
        let climbed = Position {
            z: GROUND_Z + REACH_RECHECK_M,
            ..START
        };
        assert_eq!(check.answer(climbed, east(20.0)), None);
    }

    #[test]
    fn the_retail_rates_pick_the_band_of_the_nearest_player() {
        let rates = AiRates::default();
        assert!(rates.is_valid());
        assert_eq!(rates.interval_ms(Activity::Combat, 100.0), 33);
        assert_eq!(rates.interval_ms(Activity::Idle, 1.0), 333);
        assert_eq!(rates.interval_ms(Activity::Moving, 19.9), 98);
        assert_eq!(rates.interval_ms(Activity::Moving, 20.0), 218);
        assert_eq!(rates.interval_ms(Activity::Moving, f32::INFINITY), 333);
        let falling = AiRates {
            moving_range_m: [40.0, 20.0],
            ..rates
        };
        assert!(!falling.is_valid());
        assert!(
            !AiRates {
                combat_interval_ms: 0,
                ..rates
            }
            .is_valid()
        );
    }

    #[test]
    fn a_mob_shows_its_spawns_slice_or_its_vitals_look() {
        let ai = test_ai(&[TEST_MOB]);
        let vitals = nw_formats::objectstream::lowercase_crc32(TEST_MOB);
        assert_eq!(ai.archetype_for(vitals, None), Some(0));
        assert_eq!(
            ai.archetype_for(vitals, Some([1; 16])),
            None,
            "an unknown slice"
        );
        assert_eq!(ai.archetype_for(vitals + 1, Some(TEST_ASSET)), Some(0));
        assert_eq!(ai.archetype_for(vitals + 1, None), None);
        assert!(ai.has_tree(0));
        assert!(!MobAi::empty().has_tree(0));
    }

    #[test]
    fn a_line_stands_on_the_floor_within_the_slope() {
        let gentle = Ramp { rise: 0.5 };
        let end = floor_line(&gentle, START, east(10.0), Some(45.0)).expect("a 27 degree ramp");
        assert!((end.z - (GROUND_Z + 5.0)).abs() < 1e-3, "{end:?}");
        assert_eq!(
            floor_line(&Ramp { rise: 2.0 }, START, east(10.0), Some(45.0)),
            None,
            "a 63 degree ramp is too steep"
        );
        assert!(
            floor_line(&Ramp { rise: 1.0 }, START, east(10.0), None).is_some(),
            "without a slope limit a 45 degree ramp rises 0.5 m a step"
        );
        assert_eq!(
            floor_line(&Ramp { rise: 2.0 }, START, east(10.0), None),
            None,
            "1 m a step is over the retail climb"
        );
        let deck = Position {
            z: GROUND_Z + MAX_STEP_DOWN_M + 1.0,
            ..START
        };
        let on_deck =
            floor_line(&FlatGround(GROUND_Z), deck, east(5.0), Some(45.0)).map(|end| end.z);
        assert_eq!(
            on_deck,
            Some(deck.z),
            "a deck above the height map keeps its height"
        );
    }

    /// A deck the floor holds east of `START` up to [`DECK_END_M`], [`DECK_HEIGHT_M`] above the
    /// ground; the ground lies under it.
    struct Deck;

    const DECK_END_M: f32 = 3.0;
    const DECK_HEIGHT_M: f32 = MAX_STEP_DOWN_M + 4.0;

    impl Ground for Deck {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_Z)
        }

        fn floor_at(&self, x: f32, _: f32, z: f32) -> Option<f32> {
            let deck = GROUND_Z + DECK_HEIGHT_M;
            Some(if x - START.x <= DECK_END_M && z >= deck - ON_FLOOR_M {
                deck
            } else {
                GROUND_Z
            })
        }
    }

    /// Flat ground with a wall across the line from 2 m to 3 m east of [`START`].
    struct Wall;

    impl Ground for Wall {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_Z)
        }

        fn blocked(&self, from: Position, to: Position) -> bool {
            let near = from.x.min(to.x) - START.x;
            let far = from.x.max(to.x) - START.x;
            far >= 2.0 && near <= 3.0
        }
    }

    #[test]
    fn a_line_stops_at_a_wall() {
        // Live 2026-10-29: mobs walked into rock and a crate; the line tested the floor only.
        assert!(
            floor_line(&Wall, START, east(1.5), None).is_some(),
            "short of it"
        );
        assert_eq!(
            floor_line(&Wall, START, east(5.0), None),
            None,
            "through it"
        );
    }

    /// Flat ground with a rock from 2 m to 3 m east of [`START`], [`ROCK_HALF_WIDTH_M`] to each
    /// side of the line.
    struct Rock;

    const ROCK_HALF_WIDTH_M: f32 = 1.5;

    impl Ground for Rock {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_Z)
        }

        fn blocked(&self, from: Position, to: Position) -> bool {
            let near = from.x.min(to.x) - START.x;
            let far = from.x.max(to.x) - START.x;
            let beside = |at: Position| (at.y - START.y).abs() <= ROCK_HALF_WIDTH_M;
            far >= 2.0 && near <= 3.0 && beside(from) && beside(to)
        }
    }

    #[test]
    fn a_target_behind_a_rock_is_reached_around_it() {
        // Live 2026-10-29: Janszoon stood blocked by a rock 2 m from his spawn.
        assert_eq!(floor_line(&Rock, START, east(5.0), None), None, "straight");
        assert!(reaches(&Rock, START, east(5.0), None), "around it");
        assert!(
            !reaches(&Wall, START, east(5.0), None),
            "a long wall stays a wall"
        );
    }

    /// Flat ground with two walls across the way east of [`START`], their gaps at opposite
    /// ends: no one turn passes them.
    struct TwoWalls;

    impl Ground for TwoWalls {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_Z)
        }

        fn no_stand(&self, feet: Position) -> bool {
            let (x, y) = (feet.x - START.x, feet.y - START.y);
            ((4.0..=5.0).contains(&x) && (-20.0..=6.0).contains(&y))
                || ((9.0..=10.0).contains(&x) && (-6.0..=20.0).contains(&y))
        }
    }

    #[test]
    fn a_mob_walks_a_way_around_two_walls_to_its_target() {
        use super::test_support::TEST_REACH_M;
        const STEP_MS: usize = 33;
        const UNTIL_MS: u64 = 30_000;
        let ai = test_ai(&[]);
        let player = east(14.0);
        assert!(
            !reaches(&TwoWalls, START, player, None),
            "no one turn passes"
        );
        let seen = [Seen {
            entity: 7,
            position: player,
        }];
        let around = |now_ms| Surroundings {
            now_ms,
            seen: &seen,
            ground: &TwoWalls,
            health_share: 1.0,
            entity: 1,
            tagged: &[],
            held: None,
        };
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut at = START;
        let mut nearest = f32::MAX;
        for now_ms in (0..UNTIL_MS).step_by(STEP_MS) {
            mind.step(&ai, &mut at, &around(now_ms));
            assert!(!TwoWalls.no_stand(at), "never in a wall: {at:?}");
            nearest = nearest.min(at.horizontal_distance(player));
        }
        assert!(nearest <= TEST_REACH_M, "it got to its target: {nearest}");
    }

    #[test]
    fn a_move_into_a_rock_turns_along_it() {
        const STEP_M: f32 = 0.5;
        let before = east(1.75);
        let into = east(1.75 + STEP_M);
        let moved = floor_move(&Rock, before, into, None).expect("a free turn");
        assert!(moved.x - START.x < 2.0, "not into the rock");
        assert!(
            (moved.horizontal_distance(before) - STEP_M).abs() < 0.01,
            "the same step length"
        );
        assert_eq!(
            floor_move(&Rock, START, east(1.0), None),
            Some(east(1.0)),
            "straight"
        );
    }

    #[test]
    fn a_mob_on_a_floor_does_not_walk_off_a_high_edge() {
        let on_deck = Position {
            z: GROUND_Z + DECK_HEIGHT_M,
            ..START
        };
        assert!(
            floor_line(&Deck, on_deck, east(DECK_END_M - 1.0), None).is_some(),
            "along the deck"
        );
        assert_eq!(
            floor_line(&Deck, on_deck, east(DECK_END_M + 2.0), None),
            None,
            "off its edge, 7 m above the ground"
        );
    }

    #[test]
    fn a_design_profile_becomes_overrides_of_the_slice_values() {
        const LEASH_M: f32 = 30.0;
        const WANDER_M: f32 = 4.0;
        const INTERVAL_MS: u64 = 2_500;
        const RESPAWN_MS: u64 = 9_000;
        let ai = test_ai(&[]);
        let key = |name: &str| ai.data().key(name).expect("key");
        let world = WorldFile {
            ai_profiles: BTreeMap::from([(
                "guards".to_owned(),
                AiProfile {
                    behaviour: AiBehaviour::Guard,
                    leash_radius_m: Some(LEASH_M),
                    wander_radius_m: Some(WANDER_M),
                    attack_interval_ms: Some(INTERVAL_MS),
                    respawn_ms: Some(RESPAWN_MS),
                    ..AiProfile::default()
                },
            )]),
            paths: BTreeMap::from([(
                "walk".to_owned(),
                PatrolPath {
                    points: vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
                    waits_ms: vec![1_500],
                    mode: PathMode::BackAndForth,
                    tasks: Vec::new(),
                },
            )]),
            vitals_ai: vec![
                DesignSpawn {
                    profile: Some("guards".to_owned()),
                    path: None,
                },
                DesignSpawn::default(),
                DesignSpawn {
                    profile: None,
                    path: Some("walk".to_owned()),
                },
            ],
            ..WorldFile::default()
        };
        let resolved = resolve_ai(&world, &ai);
        let guard = resolved[0].as_ref().expect("a profile");
        assert!(
            guard
                .facts
                .contains(&(key(ENABLE_WANDER_KEY), Value::Bool(false)))
        );
        assert!(
            guard
                .facts
                .contains(&(key(AMBIENT_LEASH_KEY), Value::Float(LEASH_M)))
        );
        assert!(
            guard
                .facts
                .contains(&(key(COMBAT_LEASH_KEY), Value::Float(LEASH_M)))
        );
        assert!(
            guard
                .facts
                .contains(&(key(WANDER_MAX_KEY), Value::Float(WANDER_M)))
        );
        assert_eq!(guard.attack_cooldown_s, Some(2.5));
        assert_eq!(guard.respawn_ms, Some(RESPAWN_MS));
        assert_eq!(resolved[1], None, "no profile, no path");
        let route = resolved[2]
            .as_ref()
            .and_then(|spawn| spawn.route.clone())
            .expect("a patrol");
        assert_eq!(route.points.len(), 2);
        assert_eq!(route.mode, RouteMode::BackAndForth);
    }

    #[test]
    fn a_route_goes_on_as_its_mode_says() {
        let walk = |mode| RouteWalk {
            route: Arc::new(Route {
                points: vec![START, east(1.0), east(2.0)],
                waits_ms: vec![2_000],
                mode,
                tasks: Vec::new(),
            }),
            next: 0,
            backwards: false,
            task_s: 0.0,
        };
        let visits = |mut walk: RouteWalk| {
            (0..5)
                .map(|_| {
                    walk.advance();
                    walk.next
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(visits(walk(RouteMode::Loop)), [1, 2, 0, 1, 2]);
        assert_eq!(visits(walk(RouteMode::BackAndForth)), [1, 2, 1, 0, 1]);
        assert_eq!(visits(walk(RouteMode::Once)), [1, 2, 2, 2, 2]);
        assert!((walk(RouteMode::Loop).wait_s() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_waypoint_task_stays_its_drawn_time_or_plays_once_without_one() {
        // The retail beach patrol spawner's graze waypoint (15 to 45 s) and the graze
        // spawner's (-1: no duration).
        const GRAZE: NameId = 7;
        let task = |duration_s| RouteTask {
            task: GRAZE,
            duration_s,
            align: false,
        };
        let mut walk = RouteWalk {
            route: Arc::new(Route {
                points: vec![START, east(1.0)],
                waits_ms: vec![2_000, 2_000],
                mode: RouteMode::Loop,
                tasks: vec![Some(task([15.0, 45.0])), Some(task([-1.0, -1.0]))],
            }),
            next: 0,
            backwards: false,
            task_s: 0.0,
        };
        walk.visit(0.5);
        assert!(
            (walk.wait_s() - 30.0).abs() < 1e-4,
            "the task's time, not the wait"
        );
        assert_eq!(walk.task().map(|task| task.task), Some(GRAZE));
        walk.advance();
        walk.visit(0.5);
        assert!(walk.wait_s() < 0.0, "no duration: the single task");
    }

    #[test]
    fn a_damage_window_reaches_by_its_shape_and_faces_by_its_homing() {
        let mut data = test_ai_data();
        let mut name = |text: &str| {
            data.name_id(text).unwrap_or_else(|| {
                data.names.push(text.to_owned());
                (data.names.len() - 1) as NameId
            })
        };
        let damage = ProcWindow {
            kind: name(DAMAGE_CLIP),
            start_s: 0.7,
            end_s: Some(0.9),
            params: vec![
                (name(SHAPE_RADIUS_PARAM), name("0.40000001")),
                (name(SHAPE_HALF_HEIGHT_PARAM), name("0.2")),
            ],
        };
        let homing = crate::mob_homing::HomingClip::from_params(
            0.0,
            None,
            [
                (crate::mob_homing::RADIUS_PARAM, "5"),
                (crate::mob_homing::MAX_ANGLE_PARAM, "180"),
            ],
        );
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        // The boar gore's clips (`npc_boar_anims.adb`): radius 0.4, half height 0.2, homing
        // radius 5 and angle 180. Its body moves in by the homing, so the hit does not lunge.
        let reach = window_reach(&ai, &damage, Some(&homing));
        assert!((reach.reach_m - 0.8).abs() < 1e-6);
        assert!(reach.lunge_max_m.abs() < f32::EPSILON);
        assert!((reach.lunge_turn_rad - std::f32::consts::PI).abs() < 1e-6);
        let still = window_reach(&ai, &damage, None);
        assert_eq!((still.lunge_max_m, still.lunge_turn_rad), (0.0, 0.0));
    }

    /// The test attack's homing clip draws the mob in to 1 m of its target before its damage
    /// window, and the mob turns to face it.
    #[test]
    fn an_attack_draws_the_mob_in_by_its_homing_before_its_hit() {
        use super::test_support::TEST_HOMING_DISTANCE_M;
        const STEP_MS: usize = 33;
        const UNTIL_MS: u64 = 5_000;
        let ai = test_ai(&[]);
        let ground = FlatGround(GROUND_Z);
        let player = east(2.8);
        let seen = [Seen {
            entity: 7,
            position: player,
        }];
        let around = |now_ms| Surroundings {
            now_ms,
            seen: &seen,
            ground: &ground,
            health_share: 1.0,
            entity: 1,
            tagged: &[],
            held: None,
        };
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut at = START;
        let hit_at = (0..UNTIL_MS).step_by(STEP_MS).find_map(|now_ms| {
            let thought = mind.step(&ai, &mut at, &around(now_ms));
            (!thought.hits.is_empty()).then_some(at)
        });
        let hit_at = hit_at.expect("it attacks");
        let left = hit_at.horizontal_distance(player);
        assert!(
            (left - TEST_HOMING_DISTANCE_M).abs() < 0.1,
            "drawn in to 1 m: {left}"
        );
        let facing = yaw_gap(mind.yaw(), yaw_toward(hit_at, player));
        assert!(facing.abs() < 0.1, "faces the player: {facing}");
    }

    /// A tree's `TriggerEntity` node runs the slice components of that trigger name (any case),
    /// and a name the slice lacks runs none (oquests triggers note: the relic spear's
    /// `DeactivateVisibility`).
    #[test]
    fn a_trigger_entity_node_runs_the_slice_components_of_its_name() {
        const CHILD: u64 = 0x0000_0174_1dcf_e452;
        const COMPONENT: u64 = 0x1f2e_3d4c_5b6a_7988;
        let mut data = test_ai_data();
        let name = u32::try_from(data.names.len()).expect("name id");
        data.names.push("DeactivateVisibility".into());
        let other = name + 1;
        data.names.push("NoSuchTrigger".into());
        data.archetypes[0].triggers.push(SliceTriggerRef {
            name,
            child: CHILD,
            component: COMPONENT,
        });
        let node = u32::try_from(data.nodes.len()).expect("node id");
        for trigger in [name, other] {
            data.nodes.push(nw_ai::node::Node {
                name: 0,
                utility: 1.0,
                interruptable: true,
                kind: NodeKind::TriggerEntity { trigger },
            });
        }
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut thought = Thought::default();
        mind.apply(
            &ai,
            START,
            0,
            vec![
                Command::Action {
                    node,
                    target_index: None,
                },
                Command::Action {
                    node: node + 1,
                    target_index: None,
                },
            ],
            &mut thought,
            Others::NONE,
        );
        assert_eq!(
            thought.triggers,
            [SliceTriggerRef {
                name,
                child: CHILD,
                component: COMPONENT,
            }]
        );
    }

    /// A name id of `data`, added when it lacks the text.
    fn name_in(data: &mut AiData, text: &str) -> NameId {
        data.name_id(text).unwrap_or_else(|| {
            data.names.push(text.to_owned());
            NameId::try_from(data.names.len() - 1).expect("name id")
        })
    }

    /// A node of `kind` added to `data`.
    fn node_in(data: &mut AiData, kind: NodeKind) -> NodeId {
        data.nodes.push(nw_ai::node::Node {
            name: 0,
            utility: 1.0,
            interruptable: true,
            kind,
        });
        NodeId::try_from(data.nodes.len() - 1).expect("node id")
    }

    /// An `ActionTarget` of tag `tag` and no limits.
    fn tagged_target(tag: NameId) -> ActionTarget {
        let unset = Param::literal(Value::Int(-1));
        ActionTarget {
            profile: None,
            tag_filter: tag,
            limits: nw_ai::node::TargetLimits {
                min_targets: unset,
                max_targets: unset,
                top_exclude: unset,
                range_behavior: Param::literal(Value::Int(0)),
            },
        }
    }

    /// The mob of entity `entity` at `metres` east of `START` with tag `tag`.
    fn tagged_mob(entity: u64, metres: f32, tag: NameId) -> TaggedMob {
        TaggedMob {
            entity,
            position: east(metres),
            tags: vec![tag],
        }
    }

    /// A `TriggerRemoteEntity` (retail `tendril_leviathan_base` `TurnOnMarker`) runs the slice
    /// trigger on the tagged mobs, the nearest alone without `UseAllAvailableTargets`; with no
    /// tagged mob the sim cannot run it, so the node fails.
    #[test]
    fn a_remote_trigger_goes_to_the_tagged_mobs_and_fails_without_one() {
        const OWN: u64 = 1;
        let mut data = test_ai_data();
        let trigger = name_in(&mut data, "TurnOnMarker");
        let tag = name_in(&mut data, "Tendril");
        let kind = |all_targets| NodeKind::TriggerRemoteEntity {
            trigger,
            all_targets,
            target: tagged_target(tag),
        };
        let one = node_in(&mut data, kind(false));
        let all = node_in(&mut data, kind(true));
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let tagged = [
            tagged_mob(OWN, 0.0, tag),
            tagged_mob(8, 9.0, tag),
            tagged_mob(9, 3.0, tag),
        ];
        let others = Others {
            entity: OWN,
            tagged: &tagged,
        };
        {
            let mut view = mind.state.view(&ai, START, 0, false);
            assert!(
                !view.can_act(&ai.data().node(one).kind, None),
                "no mob near"
            );
            view.others = others;
            assert!(view.can_act(&ai.data().node(one).kind, None));
        }
        let mut thought = Thought::default();
        let action = |node| Command::Action {
            node,
            target_index: None,
        };
        mind.apply(
            &ai,
            START,
            0,
            vec![action(one), action(all)],
            &mut thought,
            others,
        );
        assert_eq!(
            thought.remote_triggers,
            [(9, trigger), (9, trigger), (8, trigger)],
            "the nearest, then every tagged mob but itself"
        );
    }

    /// The data's "Check Enemies Dead" query: `Player`, vitals state 4, not 6, `Unused Equal 0`.
    /// It passes when no living player is within its radius (0: the node's limit of 200 m).
    #[test]
    fn an_entity_query_holds_its_count_against_its_entry_condition() {
        const QUERY_RADIUS_M: f32 = 100.0;
        let mut data = test_ai_data();
        let player = name_in(&mut data, PLAYER_TAG);
        let unused = name_in(&mut data, "Unused");
        let none = name_in(&mut data, "");
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let query = |radius: f32| EntityQueryParams {
            gates: nw_ai::node::Gates {
                entry: nw_ai::value::ConditionGroup {
                    any: false,
                    conditions: vec![nw_ai::value::Condition {
                        left: Param::literal(Value::Text(unused)),
                        op: nw_ai::value::CompareOp::Equal,
                        right: Param::literal(Value::Int(0)),
                        auto_plan: false,
                        allow_dynamic_scaling: true,
                    }],
                },
                ..nw_ai::node::Gates::default()
            },
            center: Param {
                key: None,
                value: None,
            },
            radius: Param::literal(Value::Float(radius)),
            tag_filter: player,
            matching_vitals_state: Param::literal(Value::Int(ALIVE_VITALS_STATE)),
            not_matching_vitals_state: Param::literal(Value::Int(NO_VITALS_STATE_FILTER)),
            in_threat_filter: Param::literal(Value::Int(2)),
            in_threat_filter_string: none,
        };
        let player_at = |distance_m: f32| {
            [Seen {
                entity: 1,
                position: Position {
                    x: START.x + distance_m,
                    ..START
                },
            }]
        };
        let passes = |seen: &[Seen], radius: f32| {
            let mut view = mind.state.view(&ai, START, 0, false);
            view.seen = seen;
            view.entity_query(&query(radius), &NoBoard)
        };
        assert!(
            passes(&[], QUERY_RADIUS_M),
            "no player: the enemies are dead"
        );
        assert!(
            !passes(&player_at(50.0), QUERY_RADIUS_M),
            "a living player inside"
        );
        assert!(
            passes(&player_at(150.0), QUERY_RADIUS_M),
            "a player beyond the radius"
        );
        assert!(
            !passes(&player_at(150.0), 0.0),
            "a radius of 0 is the limit of 200 m"
        );
    }

    /// `UpdateNWTags` gives the mob its tags, which a tracker's tag filter and
    /// `CountEntitiesWithTag` of another mob then see (retail `hercyne_reindeer_turret`
    /// `BurstReady`); a strict add of a tag it has fails.
    #[test]
    fn tags_a_tree_gives_are_seen_by_other_trees() {
        let mut data = test_ai_data();
        let ready = name_in(&mut data, "BurstReady");
        let add = |strict| nw_ai::node::TagOperation {
            tag: ready,
            add: true,
            strict,
        };
        let update = node_in(
            &mut data,
            NodeKind::UpdateNwTags {
                main: add(false),
                operations: Vec::new(),
            },
        );
        let strict_kind = NodeKind::UpdateNwTags {
            main: add(true),
            operations: Vec::new(),
        };
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        mind.apply(
            &ai,
            START,
            0,
            vec![Command::Action {
                node: update,
                target_index: None,
            }],
            &mut Thought::default(),
            Others::NONE,
        );
        assert_eq!(mind.nw_tags(), [ready]);
        let view = mind.state.view(&ai, START, 0, false);
        assert!(
            !view.can_act(&strict_kind, None),
            "a strict add of a held tag"
        );
        let tagged = [tagged_mob(5, 2.0, ready)];
        let other = TreeMind::new(&ai, 0, START, 0.0, 2, None).expect("a tree");
        let mut view = other.state.view(&ai, START, 0, false);
        view.others = Others {
            entity: 6,
            tagged: &tagged,
        };
        assert_eq!(view.entities_with_tag("BurstReady"), 1);
        let unset = Param::literal(Value::Int(-1));
        let tracker = TrackerParams {
            target: tagged_target(0),
            evaluation_ms: unset,
            profile: None,
            tag_filter: ready,
            min_targets: unset,
            max_targets: unset,
            top_exclude: unset,
            all_valid_targets: false,
            range_behavior: unset,
        };
        assert_eq!(view.action_targets(&tracker), 1);
    }

    /// An order another tree gives (`AssignOrder`) is the `AI_Order` engine key until the mob
    /// completes it (`CompleteOrder`, which fails without one); a remote fact replaces a value
    /// only with `OverwriteTargetFact`.
    #[test]
    fn orders_and_remote_facts_reach_the_tree() {
        const DESPAWN: u32 = 3_967_816_091;
        let mut data = test_ai_data();
        let complete = node_in(&mut data, NodeKind::CompleteOrder);
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let order = |mind: &TreeMind| {
            mind.state
                .view(&ai, START, 0, false)
                .engine_value(EngineKey::AiOrder)
        };
        assert_eq!(order(&mind), None);
        assert!(
            !mind
                .state
                .view(&ai, START, 0, false)
                .can_act(&NodeKind::CompleteOrder, None)
        );
        mind.assign_order(DESPAWN);
        assert_eq!(order(&mind), Some(Value::Int(i64::from(DESPAWN))));
        mind.apply(
            &ai,
            START,
            0,
            vec![Command::Action {
                node: complete,
                target_index: None,
            }],
            &mut Thought::default(),
            Others::NONE,
        );
        assert_eq!(order(&mind), None);
        let key = ai.data().key(COMBAT_LEASH_KEY).expect("key");
        let fact = |value: f32, overwrite| RemoteFact {
            mob: 1,
            key,
            value: Value::Float(value),
            overwrite,
        };
        mind.set_remote_fact(&fact(1.0, false));
        mind.set_remote_fact(&fact(2.0, false));
        assert_eq!(mind.brain().fact(key), Some(Value::Float(1.0)), "kept");
        mind.set_remote_fact(&fact(3.0, true));
        assert_eq!(
            mind.brain().fact(key),
            Some(Value::Float(3.0)),
            "overwritten"
        );
    }

    /// The sim has no projectile or beam of a tree node (Unknown in the AI data): those nodes
    /// fail.
    #[test]
    fn a_tree_projectile_or_beam_fails() {
        let ai = test_ai(&[]);
        let mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let view = mind.state.view(&ai, START, 0, false);
        let unset = Param::literal(Value::Int(-1));
        let projectile = NodeKind::SpawnProjectile(Box::new(nw_ai::node::ProjectileParams {
            target: tagged_target(0),
            target_pos: unset,
            use_all_available_targets: true,
            min_targets_per_shot: unset,
            max_targets_per_shot: unset,
            min_seconds_between_shots: unset,
            max_seconds_between_shots: unset,
        }));
        let beam = NodeKind::SetBeam {
            beam: 0,
            active: true,
            aim_mode: unset,
            lock_target: false,
        };
        assert!(!view.can_act(&projectile, None));
        assert!(!view.can_act(&beam, None));
    }

    /// The tree reads the effects the sim holds on the mob (a player's stun), and the effects
    /// the tree puts on the mob go to the sim.
    #[test]
    fn the_tree_sees_held_effects_and_hands_its_own_to_the_sim() {
        const STUN: &str = "AI_Stun_2sec";
        let mut data = test_ai_data();
        let empower = name_in(&mut data, "Empower");
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let held = HeldEffects {
            effects: vec![STUN.to_owned()],
            categories: vec!["stun".to_owned()],
            control: Control {
                stunned: true,
                ..Control::default()
            },
            ..HeldEffects::default()
        };
        {
            let mut view = mind.state.view(&ai, START, 0, false);
            assert!(!view.has_status_effect(STUN));
            view.held = Some(&held);
            assert!(view.has_status_effect(STUN));
            assert!(view.has_status_category("Stun"));
        }
        let mut thought = Thought::default();
        mind.status_effect(&ai, empower, true, &mut thought);
        mind.status_effect(&ai, empower, false, &mut thought);
        assert_eq!(
            mind.take_status_changes(),
            [(empower, true), (empower, false)]
        );
        assert!(mind.take_status_changes().is_empty());
    }

    /// A mob with a guard holds its block up, loses stamina to blocked hits, and the hit that
    /// takes the last breaks the guard: `BlockBroken` takes the block down for 8 s, holds the
    /// regen, then fills the stamina, and the block rises again (retail `damned_block`).
    /// The guard of nearly every blocking mob type (`StaminaBaseMax` 50, `StaminaBaseTickRate`
    /// 2 per `StaminaTickDelay` 1: 130 of the 180 vitals rows of the mobs that blocked in the
    /// retail ledgers, the damned pistoleer's too), with its regen of [`MobGuard::of`].
    const GUARD: MobGuard = MobGuard {
        stamina_max: 50.0,
        stamina_initial: 50.0,
        row_stamina_max: 50.0,
        row_stamina_initial: 50.0,
        tick_rate: 2.0,
        regen_per_s: 2.0,
        mitigation: 1.0,
        beast: false,
    };

    #[test]
    fn a_guard_breaks_at_no_stamina_and_rises_again_after_block_broken() {
        use super::test_support::{TEST_BLOCK_BROKEN_S, TEST_BROKEN_GUARD_S};
        const STEP_MS: u64 = 100;
        let ai = test_ai(&[]);
        let ground = FlatGround(GROUND_Z);
        let around = |now_ms| Surroundings {
            now_ms,
            seen: &[],
            ground: &ground,
            health_share: 1.0,
            entity: 1,
            tagged: &[],
            held: None,
        };
        let mut plain = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut at = START;
        plain.step(&ai, &mut at, &around(0));
        assert!(!plain.blocking(), "no guard, no block");
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        mind.set_guard(GUARD);
        mind.step(&ai, &mut at, &around(0));
        assert!(mind.blocking(), "its block rises");
        assert_eq!(
            mind.guard_stamina(),
            Some(GUARD.stamina_max),
            "a fresh guard is full"
        );
        assert_eq!(mind.guard_hit(&ai, at, 0, 20.0), Some(GuardHit::Held));
        assert_eq!(
            mind.guard_stamina(),
            Some(GUARD.stamina_max - MobGuard::blocked_loss(20.0)),
            "a blocked hit lowers it"
        );
        assert_eq!(mind.guard_hit(&ai, at, 0, 50.0), Some(GuardHit::Broken));
        assert!(!mind.blocking());
        assert_eq!(
            mind.guard_stamina(),
            Some(0.0),
            "a broken guard shows empty"
        );
        let broken_ms = (TEST_BLOCK_BROKEN_S * 1000.0) as u64;
        let mut rose_ms = None;
        for now_ms in (STEP_MS..broken_ms * 2).step_by(STEP_MS as usize) {
            mind.step(&ai, &mut at, &around(now_ms));
            if mind.blocking() && rose_ms.is_none() {
                rose_ms = Some(now_ms);
            }
        }
        let rose_ms = rose_ms.expect("the block rises again");
        assert_eq!(
            mind.guard_stamina(),
            Some(GUARD.stamina_max),
            "the end of BlockBroken fills it"
        );
        assert!(
            rose_ms >= broken_ms
                && rose_ms <= broken_ms + (TEST_BROKEN_GUARD_S * 1000.0) as u64 + 2_000,
            "after BlockBroken: {rose_ms}"
        );
        assert_eq!(
            mind.guard_hit(
                &ai,
                at,
                broken_ms * 2,
                GUARD.stamina_max / nw_content::combat::MOB_GUARD_STAMINA_SHARE - 1.0
            ),
            Some(GuardHit::Held),
            "the stamina is full again"
        );
    }

    /// A heavy mob (its slice's grit is its stamina, `AI_Enable_Stagger` from its spawn) loses
    /// stamina to unblocked hits while its grit is on, less under a stagger resist; the hit that
    /// leaves none staggers it: `r_Stagger` plays with the tracker on, its grit is off and the
    /// stamina stays empty, and the stagger's end fills the stamina, takes the tracker off and
    /// turns the grit on again. A mob without the stagger loses none.
    #[test]
    fn a_heavy_mobs_grit_takes_unblocked_hits_from_its_stamina_and_staggers_it_at_none() {
        use super::test_support::{TEST_STAGGER_S, test_ai_with, test_stagger_ai_data};
        /// `Bear_Grizzly_Named_00`: 1,000 stamina, `BlockDamageMitigation` 0, a `Beast`.
        const BEAR: MobGuard = MobGuard {
            stamina_max: 1_000.0,
            stamina_initial: 1_000.0,
            row_stamina_max: 1_000.0,
            row_stamina_initial: 1_000.0,
            tick_rate: 0.0,
            regen_per_s: 0.0,
            mitigation: 0.0,
            beast: true,
        };
        const BEAR_BODY: GritBody = GritBody::Large;
        /// A great sword heavy: 60 x 1.85.
        const HEAVY_STAGGER: f32 = 111.0;
        const RESIST: f32 = 0.5;
        const MOST_HITS: u64 = 20;
        let ai = test_ai_with(test_stagger_ai_data(), &[]);
        let tracker = ai.data().name_id(super::STAGGER_TRACKER_EFFECT);
        let ground = FlatGround(GROUND_Z);
        let around = |now_ms| Surroundings {
            now_ms,
            seen: &[],
            ground: &ground,
            health_share: 1.0,
            entity: 1,
            tagged: &[],
            held: None,
        };
        let trackers = |mind: &mut TreeMind| -> Vec<bool> {
            mind.take_status_changes()
                .into_iter()
                .filter(|(effect, _)| Some(*effect) == tracker)
                .map(|(_, on)| on)
                .collect()
        };
        let mut at = START;
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        mind.set_guard(BEAR);
        assert_eq!(
            mind.grit_hit(&ai, (at, None), 0, HEAVY_STAGGER, None),
            None,
            "before its first step its grit is off"
        );
        mind.step(&ai, &mut at, &around(0));
        assert_eq!(
            mind.grit_hit(&ai, (at, None), 0, HEAVY_STAGGER, None),
            Some(GritHit::Held)
        );
        let after_one = BEAR.stamina_max - MobGuard::grit_loss(HEAVY_STAGGER, 0.0, BEAR_BODY);
        assert_eq!(mind.guard_stamina(), Some(after_one));
        let resisting = HeldEffects {
            stagger_reduction: RESIST,
            ..HeldEffects::default()
        };
        mind.grit_hit(&ai, (at, None), 0, HEAVY_STAGGER, Some(&resisting));
        assert_eq!(
            mind.guard_stamina(),
            Some(after_one - MobGuard::grit_loss(HEAVY_STAGGER, RESIST, BEAR_BODY)),
            "the stagger resist takes half off"
        );
        let staggered_ms = (1..=MOST_HITS)
            .find(|now_ms| {
                mind.grit_hit(&ai, (at, None), *now_ms, HEAVY_STAGGER, None) == Some(GritHit::Staggered)
            })
            .expect("the hits stagger it");
        assert_eq!(mind.guard_stamina(), Some(0.0));
        assert_eq!(trackers(&mut mind), [true], "the tracker holds its regen");
        mind.step(&ai, &mut at, &around(staggered_ms + 1));
        assert_eq!(
            mind.grit_hit(&ai, (at, None), staggered_ms + 1, HEAVY_STAGGER, None),
            None,
            "r_Stagger plays: its grit is off"
        );
        let end_ms =
            staggered_ms + (TEST_STAGGER_S * 1_000.0) as u64 - super::STAGGER_EXIT_OFFSET_MS;
        mind.step(&ai, &mut at, &around(end_ms - 1));
        assert_eq!(mind.guard_stamina(), Some(0.0), "empty while it plays");
        mind.step(&ai, &mut at, &around(end_ms));
        assert_eq!(
            mind.guard_stamina(),
            Some(BEAR.stamina_max),
            "its end fills it"
        );
        assert_eq!(trackers(&mut mind), [false]);
        assert_eq!(
            mind.grit_hit(&ai, (at, None), end_ms, HEAVY_STAGGER, None),
            Some(GritHit::Held),
            "its grit is on again"
        );
        let plain_ai = test_ai(&[]);
        let mut plain = TreeMind::new(&plain_ai, 0, START, 0.0, 1, None).expect("a tree");
        plain.set_guard(BEAR);
        plain.step(&plain_ai, &mut at, &around(0));
        assert_eq!(plain.grit_hit(&plain_ai, (at, None), 0, HEAVY_STAGGER, None), None);
        assert_eq!(plain.guard_stamina(), Some(BEAR.stamina_max));
    }

    /// A hit takes 0.75 of the grit share from a large body: a mob whose slice holds the large
    /// status effect FX list (an ogre, a brute) or whose vitals row is a `Beast` (the Motherwell
    /// stag), and all of it from the others (a dynasty heavy, a Seraph boss).
    #[test]
    fn a_large_bodys_grit_takes_three_quarters_of_the_share_from_a_hit() {
        use super::test_support::{test_ai_with, test_stagger_ai_data};
        /// A great sword heavy: 60 x 1.85.
        const HEAVY_STAGGER: f32 = 111.0;
        /// `Dynasty_Heavy`: 500 stamina, its block stops all.
        const HEAVY: MobGuard = MobGuard {
            stamina_max: 500.0,
            stamina_initial: 500.0,
            row_stamina_max: 500.0,
            row_stamina_initial: 500.0,
            tick_rate: 0.0,
            regen_per_s: 0.0,
            mitigation: 1.0,
            beast: false,
        };
        const STAG: MobGuard = MobGuard {
            beast: true,
            ..HEAVY
        };
        let ground = FlatGround(GROUND_Z);
        let around = Surroundings {
            now_ms: 0,
            seen: &[],
            ground: &ground,
            health_share: 1.0,
            entity: 1,
            tagged: &[],
            held: None,
        };
        let first_loss = |large_status_effects_fx: bool, guard: MobGuard| {
            let mut data = test_stagger_ai_data();
            for arch in &mut data.archetypes {
                arch.large_status_effects_fx = large_status_effects_fx;
            }
            let ai = test_ai_with(data, &[]);
            let mut at = START;
            let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
            mind.set_guard(guard);
            mind.step(&ai, &mut at, &around);
            assert_eq!(
                mind.grit_hit(&ai, (at, None), 0, HEAVY_STAGGER, None),
                Some(GritHit::Held)
            );
            guard.stamina_max - mind.guard_stamina().expect("a stamina")
        };
        let full = MobGuard::grit_loss(HEAVY_STAGGER, 0.0, GritBody::Standard);
        let large = full * LARGE_BODY_GRIT_SHARE;
        assert!(
            (first_loss(false, HEAVY) - full).abs() < 1e-3,
            "a humanoid heavy"
        );
        assert!(
            (first_loss(true, HEAVY) - large).abs() < 1e-3,
            "the large FX list"
        );
        assert!((first_loss(false, STAG) - large).abs() < 1e-3, "a beast");
        assert!((first_loss(true, STAG) - large).abs() < 1e-3, "both: once");
    }

    /// A staggered mob plays the stagger its `ChangeActionFragment` picks by the side it was
    /// hit from (`r_Stagger_F` ahead, `r_Stagger_B` behind), shows it while it plays, and the
    /// picked fragment's length ends it; a hit from an Unknown side plays its own `r_Stagger`.
    #[test]
    fn a_staggered_mob_plays_the_front_or_back_stagger_of_the_side_it_was_hit_from() {
        use nw_ai::data::{CageCondition, FragmentCase};

        use super::test_support::{TEST_STAGGER_S, test_ai_with, test_stagger_ai_data};
        const FRONT_S: f32 = 5.0;
        const BACK_S: f32 = 4.0;
        /// A great sword heavy: 60 x 1.85.
        const HEAVY_STAGGER: f32 = 111.0;
        const MOST_HITS: u64 = 20;
        const AWAY_M: f32 = 5.0;
        const GUARD: MobGuard = MobGuard {
            stamina_max: 250.0,
            stamina_initial: 250.0,
            row_stamina_max: 250.0,
            row_stamina_initial: 250.0,
            tick_rate: 0.0,
            regen_per_s: 0.0,
            mitigation: 0.0,
            beast: false,
        };
        let mut data = test_stagger_ai_data();
        let mut name = |text: &str| {
            data.names.push(text.to_owned());
            u32::try_from(data.names.len() - 1).expect("name id")
        };
        let (front, back) = (name("r_Stagger_F"), name("r_Stagger_B"));
        let entry = |seconds| {
            vec![FragmentVariant {
                tags: Vec::new(),
                frag_tags: Vec::new(),
                length: FragmentLength::Seconds(seconds),
                windows: Vec::new(),
            }]
        };
        let side = |min_deg, max_deg| CageCondition::DamagedByAngle { min_deg, max_deg };
        let stagger_name = data.name_id(super::STAGGER_ACTION).expect("stagger name");
        let own_fragment = {
            let stagger = data
                .cage_actions
                .iter_mut()
                .find(|cage| cage.action == stagger_name)
                .expect("stagger action");
            stagger.fragment_cases = vec![
                FragmentCase {
                    condition: CageCondition::Any(vec![side(0, 90), side(-90, 0)]),
                    fragment: front,
                    variants: entry(FRONT_S),
                },
                FragmentCase {
                    condition: CageCondition::Any(vec![side(90, 180), side(-180, -90)]),
                    fragment: back,
                    variants: entry(BACK_S),
                },
            ];
            stagger.fragment
        };
        let ai = test_ai_with(data, &[]);
        let ground = FlatGround(GROUND_Z);
        let around = |now_ms| Surroundings {
            now_ms,
            seen: &[],
            ground: &ground,
            health_share: 1.0,
            entity: 1,
            tagged: &[],
            held: None,
        };
        // The mob faces yaw 0 (+y): ahead is +y, behind -y.
        let staggered_by = |from: Option<Position>, seconds: f32| {
            let mut at = START;
            let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
            mind.set_guard(GUARD);
            mind.step(&ai, &mut at, &around(0));
            let staggered_ms = (1..=MOST_HITS)
                .find(|now_ms| {
                    mind.grit_hit(&ai, (at, from), *now_ms, HEAVY_STAGGER, None)
                        == Some(GritHit::Staggered)
                })
                .expect("the hits stagger it");
            let fragment = mind.stagger_fragment();
            assert_eq!(
                mind.shown().map(|shown| shown.action),
                ai.named(0, ai.names.stagger),
                "the stagger shows"
            );
            let end_ms = staggered_ms + (seconds * 1_000.0) as u64 - super::STAGGER_EXIT_OFFSET_MS;
            mind.step(&ai, &mut at, &around(end_ms - 1));
            let empty_before = mind.guard_stamina() == Some(0.0);
            mind.step(&ai, &mut at, &around(end_ms));
            let full_at_end = mind.guard_stamina() == Some(GUARD.stamina_max);
            assert!(empty_before && full_at_end, "its fragment's length ends it");
            assert_eq!(mind.stagger_fragment(), None, "it shows no more");
            fragment
        };
        let ahead = Position {
            y: START.y + AWAY_M,
            ..START
        };
        let behind = Position {
            y: START.y - AWAY_M,
            ..START
        };
        assert_eq!(staggered_by(Some(ahead), FRONT_S), Some(front));
        assert_eq!(staggered_by(Some(behind), BACK_S), Some(back));
        assert_eq!(staggered_by(None, TEST_STAGGER_S), Some(own_fragment));
    }

    /// One retail guard sequence: the attacker's weapon, its blocked hits (seconds after the
    /// first, raw stamina loss) and the hit that broke the guard.
    type GuardSequence = (&'static str, &'static [(f32, f32)], Option<usize>);

    /// Retail blocked hits on a fresh guard, each as (seconds after the first, the attacker's
    /// weapon `BlockStaminaDamage` x the attack row's `BlockStaminaDmgMod`), and the hit whose
    /// fall of the block was a guard break (the block fell with it and stayed down 7 s or more,
    /// or the mob died first), `None` when the block stayed up 0.3 s or more after the last hit.
    /// Observed (blocksweep and attackseq over the 45 ledgers with breaks; the hit's attack is
    /// the attacker's last slayer start within 1.5 s before it: a release start is a light, a
    /// hold start a charged attack).
    const RETAIL_GUARD_SEQUENCES: [GuardSequence; 6] = [
        // Playthrough_Part_07 interest 110, a rapier (28): six lights (0.4), then the charged
        // attack (2.4) at 1753.22 s.
        (
            "rapier",
            &[
                (0.0, 28.0 * 0.4),
                (0.22, 28.0 * 0.4),
                (0.75, 28.0 * 0.4),
                (2.42, 28.0 * 0.4),
                (2.97, 28.0 * 0.4),
                (3.19, 28.0 * 0.4),
                (4.18, 28.0 * 2.4),
            ],
            Some(6),
        ),
        // Playthrough_Part_04 interest 62, a sword (36): four lights (`1H_Sword_Attack1` of the
        // player damage table, 0.4), then the charged attack (2.4) at 104.76 s.
        (
            "sword",
            &[
                (0.0, 36.0 * 0.4),
                (3.86, 36.0 * 0.4),
                (4.29, 36.0 * 0.4),
                (4.76, 36.0 * 0.4),
                (7.15, 36.0 * 2.4),
            ],
            Some(4),
        ),
        // Playthrough_Part_16 interest 57, a flail (39): four lights (0.4), then the charged
        // attack (2.0) at 153.37 s.
        (
            "flail",
            &[
                (0.0, 39.0 * 0.4),
                (0.6, 39.0 * 0.4),
                (4.79, 39.0 * 0.4),
                (5.43, 39.0 * 0.4),
                (8.16, 39.0 * 2.0),
            ],
            Some(4),
        ),
        // Playthrough_Part_06 interest 118, a great sword (60): its charged attack (1.85) at
        // 3749.52 s.
        ("great sword", &[(0.0, 60.0 * 1.85)], Some(0)),
        // Playthrough_Part_01 interest 58, a hatchet (30): seven lights (0.4) from 4040.47 s; the
        // regen between them holds the guard until the seventh.
        (
            "hatchet run",
            &[
                (0.0, 30.0 * 0.4),
                (0.46, 30.0 * 0.4),
                (0.89, 30.0 * 0.4),
                (1.43, 30.0 * 0.4),
                (2.79, 30.0 * 0.4),
                (3.36, 30.0 * 0.4),
                (5.03, 30.0 * 0.4),
            ],
            Some(6),
        ),
        // Playthrough_Part_01 interest 76, a hatchet (30): three lights (0.4) from 2390.95 s;
        // the block stayed up.
        (
            "hatchet",
            &[(0.0, 30.0 * 0.4), (0.47, 30.0 * 0.4), (0.91, 30.0 * 0.4)],
            None,
        ),
    ];

    /// A blocked hit takes [`nw_content::combat::MOB_GUARD_STAMINA_SHARE`] of the attacker's
    /// weapon `BlockStaminaDamage` x the row's `BlockStaminaDmgMod`, and the guard regains its
    /// stamina at 2 per second between them: the six retail sequences break where retail broke,
    /// the hatchet run at its seventh light (Observed: the guard's ALC child 44 of each run gives
    /// the same loss and regen, [`MobGuard::regen_per_s`]).
    #[test]
    fn the_retail_blocked_hits_break_the_guard_where_retail_broke() {
        const STEP_MS: u64 = 50;
        const MS_PER_S: f32 = 1_000.0;
        let ai = test_ai(&[]);
        let ground = FlatGround(GROUND_Z);
        let around = |now_ms| Surroundings {
            now_ms,
            seen: &[],
            ground: &ground,
            health_share: 1.0,
            entity: 1,
            tagged: &[],
            held: None,
        };
        for (weapon, hits, broken_at) in RETAIL_GUARD_SEQUENCES {
            let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
            mind.set_guard(GUARD);
            let mut at = START;
            let mut now_ms = 0;
            mind.step(&ai, &mut at, &around(now_ms));
            let mut broken = None;
            for (index, (after_s, loss)) in hits.iter().enumerate() {
                let hit_ms = (after_s * MS_PER_S).round() as u64;
                while now_ms + STEP_MS <= hit_ms {
                    now_ms += STEP_MS;
                    mind.step(&ai, &mut at, &around(now_ms));
                }
                if mind.guard_hit(&ai, at, hit_ms, *loss) == Some(GuardHit::Broken) {
                    broken = Some(index);
                    break;
                }
            }
            assert_eq!(broken, broken_at, "the {weapon}'s sequence");
        }
    }

    /// A guard regains its stamina at its plain regen (2 per second for 2 per 1 s and for the
    /// `Inquisitor_Warden` 2 per 0 s, a `Dungeon` guard of 200 too) and `1 +` the held
    /// `StaminaRate` times it while the world holds status effects on the mob
    /// (`AI_EvilKnight_Rally` 5: 12 per second, a segment of 5 points in 0.417 s).
    #[test]
    fn a_guard_regains_its_stamina_at_its_plain_or_held_rate() {
        const STEP_MS: u64 = 100;
        const STEPS: u64 = 10;
        const SPENT: f32 = 40.0;
        const RALLY_RATE: f32 = 5.0;
        const DUNGEON_STAMINA_MOD: f64 = 4.0;
        let ai = test_ai(&[]);
        let ground = FlatGround(GROUND_Z);
        let rally = HeldEffects {
            stamina_rate: RALLY_RATE,
            ..HeldEffects::default()
        };
        let warden = MobGuard::of((75, 75, 2, 0), 1).expect("a guard");
        let dungeon = GUARD.in_category(DUNGEON_STAMINA_MOD);
        for (guard, held, per_s) in [
            (GUARD, None, 2.0),
            (warden, None, 2.0),
            (dungeon, None, 2.0),
            (GUARD, Some(&rally), 12.0),
        ] {
            let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
            mind.set_guard(guard);
            let mut at = START;
            let around = |now_ms| Surroundings {
                now_ms,
                seen: &[],
                ground: &ground,
                health_share: 1.0,
                entity: 1,
                tagged: &[],
                held,
            };
            mind.step(&ai, &mut at, &around(0));
            assert_eq!(mind.guard_hit(&ai, at, 0, SPENT), Some(GuardHit::Held));
            let after_hit = mind.guard_stamina().expect("a guard");
            for step in 1..=STEPS {
                mind.step(&ai, &mut at, &around(step * STEP_MS));
            }
            let regained = mind.guard_stamina().expect("a guard") - after_hit;
            assert!(
                (regained - per_s).abs() < 1e-3,
                "{guard:?} {held:?}: {regained} in 1 s"
            );
        }
    }

    /// A stunned mob neither perceives, moves nor thinks; a root holds it in place while it
    /// thinks.
    #[test]
    fn a_stun_holds_the_tree_and_a_root_holds_the_body() {
        let ai = test_ai(&[]);
        let ground = FlatGround(GROUND_Z);
        let seen = [Seen {
            entity: 7,
            position: east(1.0),
        }];
        let around = |held| Surroundings {
            now_ms: 0,
            seen: &seen,
            ground: &ground,
            health_share: 1.0,
            entity: 1,
            tagged: &[],
            held,
        };
        let stunned = HeldEffects {
            control: Control {
                stunned: true,
                ..Control::default()
            },
            ..HeldEffects::default()
        };
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut at = START;
        let thought = mind.step(&ai, &mut at, &around(Some(&stunned)));
        assert!(!thought.ticked && thought.acquired.is_none(), "{thought:?}");
        let thought = mind.step(&ai, &mut at, &around(None));
        assert!(thought.ticked && thought.acquired == Some(7));
    }

    /// A tree's `ExecuteSpawn` (retail `medusa_minionsummon_attackbehavior` `Spawn_Boar`:
    /// `Count` 1, `TargetPosBlackboardKey` unset) gives the sim a summon of its slice at the
    /// mob.
    #[test]
    fn an_execute_spawn_node_summons_its_slice() {
        const SLICE: [u8; ASSET_ID_LEN] = [3; ASSET_ID_LEN];
        const COUNT: i64 = 2;
        let mut data = test_ai_data();
        let node = u32::try_from(data.nodes.len()).expect("node id");
        data.nodes.push(nw_ai::node::Node {
            name: 0,
            utility: 1.0,
            interruptable: true,
            kind: NodeKind::ExecuteSpawn {
                slice: nw_ai::node::AssetRef {
                    guid: SLICE,
                    hint: None,
                },
                alias: None,
                count: Param::literal(Value::Int(COUNT)),
                position_key: None,
                use_owner_rotation: true,
            },
        });
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut thought = Thought::default();
        mind.apply(
            &ai,
            START,
            0,
            vec![Command::Action {
                node,
                target_index: None,
            }],
            &mut thought,
            Others::NONE,
        );
        assert_eq!(
            thought.summons,
            [Summon {
                asset: SLICE,
                count: 2,
                at: None,
            }]
        );
    }

    #[test]
    fn a_ring_pick_lands_within_its_range_and_angles() {
        const RANGE: (f32, f32) = (3.0, 6.0);
        const PICKS: u64 = 200;
        let ai = test_ai(&[]);
        let mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let ground = FlatGround(GROUND_Z);
        let value = |metres: f32| Param::literal(Value::Float(metres));
        // The wander's straight pick: from the spawn, away from the mob, 310 to 50 degrees.
        let params = SelectPositionParams {
            origin: PositionType::SpawnPosition,
            anchor: PositionType::Own,
            direction: Param::literal(Value::Int(ANCHOR_TO_ORIGIN)),
            start_angle_param: value(310.0),
            stop_angle_param: value(50.0),
            range_min_param: value(RANGE.0),
            range_max_param: value(RANGE.1),
            max_attempts_param: Param::literal(Value::Int(5)),
            ..test_support::exact_pick()
        };
        let at = Position {
            y: START.y - 1.0,
            ..START
        };
        for seed in 0..PICKS {
            let mut view = mind.state.view(&ai, at, 0, false);
            view.ground = Some(&ground);
            view.picks = Cell::new(Rng48::new(seed));
            let point = view
                .select_position(&params, &NoBoard)
                .map(position_of)
                .expect("a point");
            let distance = point.horizontal_distance(START);
            assert!((RANGE.0..=RANGE.1 + 1e-3).contains(&distance), "{distance}");
            // Away from the mob (south of the spawn): north of it, within 50 degrees.
            let bearing = yaw_toward(START, point).to_degrees();
            assert!(bearing.abs() <= 50.0 + 1e-3, "{bearing}");
            assert_eq!(point.z, GROUND_Z);
        }
    }

    /// The surroundings of one step: player 7 at `player` on `ground`.
    fn alone_with<'a>(
        now_ms: u64,
        seen: &'a [Seen],
        ground: &'a (dyn Ground + Sync),
    ) -> Surroundings<'a> {
        Surroundings {
            now_ms,
            seen,
            ground,
            health_share: 1.0,
            entity: 1,
            tagged: &[],
            held: None,
        }
    }

    /// A held target has a `Target_Score` above the prey trees' bars (`> 0`, `>= 0.05`); a mob
    /// without a target has none.
    #[test]
    fn a_held_target_has_a_score_and_no_target_none() {
        const PREY_BAR: f32 = 0.05;
        let ai = test_ai(&[]);
        let mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut view = mind.state.view(&ai, START, 0, false);
        assert_eq!(view.engine_value(EngineKey::TargetScore), None);
        view.target = Some(TargetView {
            position: east(5.0),
            distance_m: 5.0,
            angle_deg: 0.0,
            visible: true,
            reachable: true,
        });
        let score = view
            .engine_value(EngineKey::TargetScore)
            .and_then(Value::as_number)
            .expect("a score");
        assert!(score >= f64::from(PREY_BAR), "{score}");
    }

    /// A target out of reach at its first sight is unreachable since its pick, not since the
    /// sim's start (live 2026-11-12: the risen of archetype 530 went home at once).
    #[test]
    fn the_unreachable_time_of_a_new_target_counts_from_its_pick() {
        const LATE_MS: u64 = 3_600_000;
        let ai = test_ai(&[]);
        let seen = [Seen {
            entity: 7,
            position: east(5.0),
        }];
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut at = START;
        mind.step(&ai, &mut at, &alone_with(LATE_MS, &seen, &Wall));
        assert_eq!(
            mind.target(),
            Some(7),
            "it takes the player behind the wall"
        );
        assert_eq!(mind.state.reachable_ms, Some(LATE_MS));
    }

    /// An Outpost Rush summon never takes a player of its own team, and drops one it held.
    #[test]
    fn a_spared_player_is_never_its_target() {
        const NOW_MS: u64 = 1_000;
        const OWN: u64 = 7;
        const FOE: u64 = 8;
        let ai = test_ai(&[]);
        let own = [Seen {
            entity: OWN,
            position: east(5.0),
        }];
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut at = START;
        mind.step(&ai, &mut at, &alone_with(NOW_MS, &own, &Wall));
        assert_eq!(mind.target(), Some(OWN));
        mind.spare(vec![OWN]);
        assert_eq!(mind.target(), None, "the held target is dropped");
        assert!(mind.spares(OWN), "its team's hits on it are refused");
        assert!(!mind.spares(FOE), "a foe's hits land");
        mind.step(&ai, &mut at, &alone_with(2 * NOW_MS, &own, &Wall));
        assert_eq!(mind.target(), None, "a spared player is never taken");
        let both = [
            own[0],
            Seen {
                entity: FOE,
                position: east(6.0),
            },
        ];
        mind.step(&ai, &mut at, &alone_with(3 * NOW_MS, &both, &Wall));
        assert_eq!(mind.target(), Some(FOE));
    }

    /// The guard objective of the spawn's overrides is the engine's `Guard_Objective_Pos` and
    /// the trees' `GuardObjectivePosition`.
    #[test]
    fn a_guard_objective_is_its_engine_key_and_position() {
        const GUARD: [f32; 3] = [3.0, 4.0, 5.0];
        let ai = test_ai(&[]);
        let overrides = SpawnAi {
            guard_objective: Some(GUARD),
            ..SpawnAi::default()
        };
        let guarding = TreeMind::new(&ai, 0, START, 0.0, 1, Some(&overrides)).expect("a tree");
        let view = guarding.state.view(&ai, START, 0, false);
        assert_eq!(
            view.engine_value(EngineKey::GuardObjectivePos),
            Some(Value::Vec3(GUARD))
        );
        assert_eq!(view.position(PositionType::GuardObjectivePosition), Some(GUARD));
        let plain = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let view = plain.state.view(&ai, START, 0, false);
        assert_eq!(view.engine_value(EngineKey::GuardObjectivePos), None);
        assert_eq!(view.position(PositionType::GuardObjectivePosition), None);
    }

    /// A damage clip written twice in one entry is one hit (retail damned bear
    /// `Attack_Lunge_L_Quad`: two equal clips, one hit).
    #[test]
    fn a_clip_written_twice_is_one_hit() {
        use super::test_support::TEST_ATTACK_TASK;
        let mut data = test_ai_data();
        let task = data.name_id(TEST_ATTACK_TASK).expect("the attack task");
        let action = data.grids[0].action(task).expect("its action") as usize;
        let windows = &mut data.cage_actions[action].variants[0].windows;
        let damage = windows
            .iter()
            .find(|window| data.names[window.kind as usize] == DAMAGE_CLIP)
            .cloned()
            .expect("a damage clip");
        windows.push(damage);
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let cage = ai.cage_action(action as u32).expect("the attack");
        mind.state.open_windows(&ai, &[], cage, 0, None, false);
        let strikes = mind
            .state
            .windows
            .iter()
            .filter(|window| matches!(window.act, WindowAct::Strike { .. }))
            .count();
        assert_eq!(strikes, 1);
    }

    /// A move to where the mob stands is done at once and shows no move (live 2026-11-12: a
    /// pistoleer walked in place on its waypoint).
    #[test]
    fn a_move_to_where_the_mob_stands_shows_no_move() {
        const WALK_TASK: &str = "Walk_Task";
        const WAIT_TASK: &str = "Wait_Task";
        let mut build = nw_ai::build::Build::default();
        let walk = build.navigation_to(WALK_TASK, None, PositionType::SpawnPosition);
        let wait = build.idle(WAIT_TASK);
        let root = build.sequence(&[walk, wait]);
        let data = build.finish(root);
        let action_of = |task: &str| {
            data.grids[0]
                .action(data.name_id(task).expect("task"))
                .expect("action")
        };
        let (walk_action, wait_action) = (action_of(WALK_TASK), action_of(WAIT_TASK));
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let ground = FlatGround(GROUND_Z);
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut at = START;
        mind.step(&ai, &mut at, &alone_with(0, &[], &ground));
        assert!(mind.state.motion.is_none(), "no move");
        assert_ne!(
            mind.state.shown.map(|shown| shown.action),
            Some(walk_action)
        );
        // The tree learns of the arrival at its next tick.
        mind.step(
            &ai,
            &mut at,
            &alone_with(RETAIL_AI_RATES.idle_interval_ms, &[], &ground),
        );
        assert_eq!(
            mind.state.shown.map(|shown| shown.action),
            Some(wait_action)
        );
        assert_eq!(at, START);
    }

    /// A `Flee` navigation to the target runs straight away from it until the target lies
    /// beyond the mob's ambient vision (the prey trees' `FleeFromTarget`: the node's own speeds
    /// 5 and 2 m/s, no slice config of that kind; retail turkeys, MSQ13-lv56 ledger).
    #[test]
    fn a_fleeing_mob_runs_from_its_target_past_its_sight() {
        use super::test_support::TEST_SIGHT_M;
        const FLEE_TASK: &str = "Move_Combat_CAGE_Task";
        const STEP_MS: u64 = 33;
        const UNTIL_MS: u64 = 20_000;
        const NODE_MAX_SPEED: f32 = 5.0;
        const NODE_MIN_SPEED: f32 = 2.0;
        const NODE_ACCELERATION: f32 = 2.0;
        const NODE_ARRIVAL_M: f32 = 0.2;
        const NODE_TURN_RATE_DEG: f32 = 720.0;
        const PLAYER_M: f32 = 5.0;
        let mut build = nw_ai::build::Build::default();
        let flee = build.navigation(FLEE_TASK, Some(FLEE_CONFIG));
        let fight = build.gate("Has_Target", nw_ai::value::CompareOp::Equal, 1.0, flee);
        let stand = build.idle("Idle_Task");
        let root = build.selector(&[fight, stand]);
        let mut data = build.finish(root);
        if let NodeKind::Navigation(nav) = &mut data.nodes[flee as usize].kind {
            let literal = |value: f32| Param::literal(Value::Float(value));
            nav.motion.max_speed = literal(NODE_MAX_SPEED);
            nav.motion.min_speed = literal(NODE_MIN_SPEED);
            nav.motion.linear_acceleration = literal(NODE_ACCELERATION);
            nav.motion.linear_deceleration = literal(NODE_ACCELERATION);
            nav.motion.arrival_threshold = literal(NODE_ARRIVAL_M);
            nav.turn_rate = literal(NODE_TURN_RATE_DEG);
        }
        data.archetypes[0].ambient_vision = Some(nw_ai::data::Vision {
            angle_deg: 360.0,
            distance_m: TEST_SIGHT_M,
        });
        let ai = MobAi::new(data, AiRates::default(), LookAssets::new());
        let ground = FlatGround(GROUND_Z);
        let player = east(PLAYER_M);
        let seen = [Seen {
            entity: 7,
            position: player,
        }];
        let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let mut at = START;
        let mut fled = false;
        for now_ms in (0..UNTIL_MS).step_by(STEP_MS as usize) {
            let before = at;
            mind.step(&ai, &mut at, &alone_with(now_ms, &seen, &ground));
            // It keeps its target (the chase rule of `perceive`), so its tree runs the flight
            // again: the flight has ended when a step no longer moves it.
            if now_ms > 0 && at == before {
                fled = true;
                break;
            }
        }
        assert!(fled, "the flight ends");
        assert!(at.horizontal_distance(player) >= TEST_SIGHT_M, "{at:?}");
        assert!(
            at.x < START.x && (at.y - START.y).abs() < 1.0,
            "straight away: {at:?}"
        );
    }

    /// The test data without a chase distance: no `CombatLeashDistance` in the slice config and
    /// none beside the key in the leash condition of its tree.
    fn data_without_a_chase_distance() -> AiData {
        let mut data = test_ai_data();
        let leash = data.key(COMBAT_LEASH_KEY).expect("the leash key");
        data.archetypes[0].config.retain(|(key, _)| *key != leash);
        for node in &mut data.nodes {
            if let NodeKind::DistanceCondition { gates, .. } = &mut node.kind {
                for condition in &mut gates.entry.conditions {
                    if condition.right.key == Some(leash) {
                        condition.right.value = None;
                    }
                }
            }
        }
        data
    }

    /// Where the data gives no chase distance, a normal mob chases 50 yards and a named one 75
    /// yards (player's design choice 2026-11-12); the data's own value goes first.
    #[test]
    fn a_mob_without_a_chase_distance_in_its_data_takes_the_fallback_of_its_kind() {
        const NAMED_TYPE_ROW: &str = "Named_Solo+";
        const BOSS_TYPE_ROW: &str = "DungeonBoss";
        let with_data = MobAi::new(test_ai_data(), AiRates::default(), LookAssets::new());
        assert_eq!(with_data.chase_fallback(0, None), None, "its slice's leash");
        let ai = MobAi::new(
            data_without_a_chase_distance(),
            AiRates::default(),
            LookAssets::new(),
        );
        assert_eq!(ai.chase_fallback(0, Some("Solo")), Some(NORMAL_CHASE_M));
        assert_eq!(
            ai.chase_fallback(0, Some(NAMED_TYPE_ROW)),
            Some(NAMED_CHASE_M)
        );
        assert_eq!(
            ai.chase_fallback(0, Some(BOSS_TYPE_ROW)),
            Some(ELITE_CHASE_M)
        );
        assert_eq!(ai.chase_fallback(0, None), Some(NORMAL_CHASE_M));
    }

    /// A normal mob keeps chasing a player 40 m from its spawn and a named one at 60 m, when no
    /// data value exists; past the fallback they walk home.
    #[test]
    fn a_mob_chases_its_fallback_distance_from_its_spawn() {
        const STEP_MS: u64 = 33;
        const STEPS: u64 = 10;
        const PLAYER_AHEAD_M: f32 = 2.0;
        let ai = MobAi::new(
            data_without_a_chase_distance(),
            AiRates::default(),
            LookAssets::new(),
        );
        let ground = FlatGround(GROUND_Z);
        let goes_home = |chase_m: f32, out_m: f32| {
            let mut mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
            mind.set_chase_fallback(chase_m);
            let mut at = east(out_m);
            let seen = [Seen {
                entity: 7,
                position: east(out_m + PLAYER_AHEAD_M),
            }];
            (0..STEPS).any(|step| {
                mind.step(&ai, &mut at, &alone_with(step * STEP_MS, &seen, &ground));
                mind.returning(&ai)
            })
        };
        assert!(!goes_home(NORMAL_CHASE_M, 40.0), "a normal mob at 40 m");
        assert!(
            goes_home(NORMAL_CHASE_M, 50.0),
            "a normal mob past 50 yards"
        );
        assert!(!goes_home(NAMED_CHASE_M, 60.0), "a named mob at 60 m");
        assert!(goes_home(NAMED_CHASE_M, 70.0), "a named mob past 75 yards");
    }

    /// A strafe field the node leaves unset comes from the slice's strafe config by its CRC (the
    /// undead pistoleer's `m_lateralMovementFrequencyMin` 1 and `Max` 3).
    #[test]
    fn a_strafe_takes_its_lateral_frequency_from_the_slice_config() {
        const LATERAL_MIN_S: f64 = 1.0;
        let kind = name_in(&mut test_ai_data(), "Strafe");
        let config = NavConfig {
            action_params: vec![(
                nw_formats::objectstream::lowercase_crc32(LATERAL_MIN_FIELD),
                Value::Float(LATERAL_MIN_S as f32),
            )],
            ..nw_ai::build::run_config(kind)
        };
        assert_eq!(
            strafe_config_number(Some(&config), LATERAL_MIN_FIELD),
            Some(LATERAL_MIN_S)
        );
        assert_eq!(strafe_config_number(Some(&config), LATERAL_MAX_FIELD), None);
        assert_eq!(strafe_config_number(None, LATERAL_MIN_FIELD), None);
    }

    /// A pick that checks no path takes a floor point behind a wall; one that checks the path
    /// to its anchor (the mob) does not (`CheckPathToAnchor`; the beach sword sailor's
    /// `ResetToWaypoint` checks none).
    #[test]
    fn a_pick_checks_the_path_only_when_its_node_asks() {
        const BEHIND_WALL_M: f32 = 5.0;
        let ai = test_ai(&[]);
        let mind = TreeMind::new(&ai, 0, east(BEHIND_WALL_M), 0.0, 1, None).expect("a tree");
        let params = SelectPositionParams {
            origin: PositionType::SpawnPosition,
            anchor: PositionType::Own,
            ..test_support::exact_pick()
        };
        let mut view = mind.state.view(&ai, START, 0, false);
        view.ground = Some(&Wall);
        assert_eq!(
            view.select_position(&params, &NoBoard).map(position_of),
            Some(east(BEHIND_WALL_M)),
            "no path check"
        );
        let checked = SelectPositionParams {
            check_path_to_anchor: true,
            ..params
        };
        assert_eq!(
            view.select_position(&checked, &NoBoard),
            None,
            "through the wall"
        );
    }

    /// Ground without height data anywhere.
    struct NoHeights;

    impl Ground for NoHeights {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            None
        }
    }

    /// A pick that checks no path takes its point at the point's own height where the level has
    /// no height data, as the floor line keeps a body's height there (sim test 2026-11-12: the
    /// walk home to the spawn failed at once and the mob stood past its leash).
    #[test]
    fn a_pick_without_a_path_check_keeps_its_height_where_the_level_has_none() {
        const OUT_M: f32 = 55.0;
        let ai = test_ai(&[]);
        let mind = TreeMind::new(&ai, 0, START, 0.0, 1, None).expect("a tree");
        let params = SelectPositionParams {
            origin: PositionType::SpawnPosition,
            anchor: PositionType::Own,
            ..test_support::exact_pick()
        };
        let mut view = mind.state.view(&ai, east(OUT_M), 0, false);
        view.ground = Some(&NoHeights);
        assert_eq!(
            view.select_position(&params, &NoBoard).map(position_of),
            Some(START)
        );
    }
}
