// SPDX-License-Identifier: AGPL-3.0-or-later
//! The node classes of the Server.pak behavior trees and their parameters (behavior-tree study,
//! sections 2.1 to 2.3; field names Observed: each field CRC is the lower-case CRC32 of a name in
//! the client exe, study `fieldnames.tsv`). Every field a class has in the 1,075 trees is kept;
//! a parameter that can name a blackboard key is a [`Param`].

use serde::{Deserialize, Serialize};

use crate::value::{ConditionGroup, KeyId, NameId, Param};

/// Index of a node in [`crate::data::AiData::nodes`].
pub type NodeId = u32;

/// The children of a composite: a run of [`crate::data::AiData::child_ids`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Children {
    /// First index in the child list.
    pub first: u32,
    /// Number of children.
    pub count: u32,
}

/// One node: the `Node` base fields and its class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// `Name`.
    pub name: NameId,
    /// `BaseUtility` (default `1.19e-7`, `FLT_EPSILON`).
    pub utility: f32,
    /// `Interruptable`.
    pub interruptable: bool,
    /// The class and its fields.
    pub kind: NodeKind,
}

/// `PositionType` (Observed: EditContext enum at `FUN_1426ad480`, parser `FUN_1426a69d0`; 4 and
/// 10 Inferred from the XML parse order; study section 1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionType {
    /// 0 `Self`.
    Own,
    /// 1 `Target`.
    Target,
    /// 2 `LastKnownTarget`.
    LastKnownTarget,
    /// 3 `SelectedPosition`.
    SelectedPosition,
    /// 4 `LastDamagePosition`.
    LastDamagePosition,
    /// 5 `SpawnPosition`.
    SpawnPosition,
    /// 6 `WaypointPosition`.
    WaypointPosition,
    /// 7 `GroupCenter`.
    GroupCenter,
    /// 8 `AttackObjectivePosition`.
    AttackObjectivePosition,
    /// 9 `GuardObjectivePosition`.
    GuardObjectivePosition,
    /// 10 `AboveStartPosition`.
    AboveStartPosition,
    /// 11 `NavLinkEndPosition`.
    NavLinkEndPosition,
    /// 12 `ActionTarget`.
    ActionTarget,
}

impl PositionType {
    /// The position type of a stored number.
    pub fn from_stored(value: i64) -> Option<Self> {
        Some(match value {
            0 => Self::Own,
            1 => Self::Target,
            2 => Self::LastKnownTarget,
            3 => Self::SelectedPosition,
            4 => Self::LastDamagePosition,
            5 => Self::SpawnPosition,
            6 => Self::WaypointPosition,
            7 => Self::GroupCenter,
            8 => Self::AttackObjectivePosition,
            9 => Self::GuardObjectivePosition,
            10 => Self::AboveStartPosition,
            11 => Self::NavLinkEndPosition,
            12 => Self::ActionTarget,
            _ => return None,
        })
    }
}

/// `UtilitySelector.SelectionMethod` (Observed in `FUN_1426355b0`: 0 takes the child with the
/// highest utility, 1 draws among the top `WeightedRandomN` children by utility weight).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionMethod {
    /// 0 `Top1`.
    Top,
    /// 1 `WeightedRandom`.
    WeightedRandom,
}

/// `Override` of a cooldown set (Observed: the XML parser `FUN_14264a8e0` stores `Always` 0,
/// `Never` 1, `KeepLongest` 2, `KeepShortest` 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CooldownOverride {
    /// 0: the new time replaces a running cooldown.
    Always,
    /// 1: a running cooldown is kept.
    Never,
    /// 2: the later end is kept.
    KeepLongest,
    /// 3: the earlier end is kept.
    KeepShortest,
}

impl CooldownOverride {
    /// The override of a stored number.
    pub fn from_stored(value: i64) -> Option<Self> {
        Some(match value {
            0 => Self::Always,
            1 => Self::Never,
            2 => Self::KeepLongest,
            3 => Self::KeepShortest,
            _ => return None,
        })
    }
}

/// `SetBlackboardFactAction.Operator` (Observed: `FUN_1426038b0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactOp {
    /// 1 `Assign`.
    Assign,
    /// 2 `Add`.
    Add,
    /// 3 `Subtract`.
    Subtract,
}

impl FactOp {
    /// The operator of a stored number.
    pub fn from_stored(value: i64) -> Option<Self> {
        Some(match value {
            1 => Self::Assign,
            2 => Self::Add,
            3 => Self::Subtract,
            _ => return None,
        })
    }
}

/// A cooldown name with its random time range and override rule.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CooldownSpec {
    /// `CooldownName`.
    pub name: NameId,
    /// `TimeMin` (seconds).
    pub min: Param,
    /// `TimeMax` (seconds).
    pub max: Param,
    /// `Override`.
    pub override_rule: CooldownOverride,
}

/// `AZ::Data::Asset`: the asset GUID (the SHA-1 name UUID of the lower-case path, study section
/// 1.2) and the path hint when one is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetRef {
    /// The asset GUID.
    pub guid: [u8; 16],
    /// The path hint; `None` when empty.
    pub hint: Option<NameId>,
}

/// `TargetSelectionLimits`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TargetLimits {
    /// `m_minTargets` (-1: not set).
    pub min_targets: Param,
    /// `m_maxTargets` (-1: not set).
    pub max_targets: Param,
    /// `m_topTargetsToExclude` (-1: not set).
    pub top_exclude: Param,
    /// `m_targetSelectionLimitsRangeBehavior`.
    pub range_behavior: Param,
}

/// `ActionTargetHelper`: who an action is for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ActionTarget {
    /// `TargetingProfile` (`.curveconfiglist`); `None` for the AI's own target.
    pub profile: Option<AssetRef>,
    /// `TargetTagFilter`.
    pub tag_filter: NameId,
    /// `LimitsOverride`.
    pub limits: TargetLimits,
}

/// `NavRayCastHelper`: the navmesh clearance an action needs ("Not enough clearance on the
/// navmesh for this action", study section 2.3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NavRayCast {
    /// `Direction` (degrees from the facing).
    pub direction: Param,
    /// `Distance` (metres; 0: no check).
    pub distance: Param,
    /// `DistanceFromWall` (metres).
    pub distance_from_wall: Param,
    /// `CheckAtExecution`: check when the action starts, not when it is planned.
    pub check_at_execution: bool,
    /// `AllowDynamicScaling`.
    pub allow_dynamic_scaling: bool,
}

/// The `CAGEAction` fields, also the base of `NavigationAction` and `StrafeAction`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CageParams {
    /// `ActionName`: the CAGE task, as text or as a key that holds it (42 nodes read it from a
    /// key, for example `CombatMoveCAGETaskName`).
    pub task: Param,
    /// `WaitForCAGECompletion`.
    pub wait_for_completion: bool,
    /// `FailTimeoutSeconds` (0: none).
    pub fail_timeout: Param,
    /// `NavRayCast`.
    pub nav_ray_cast: NavRayCast,
    /// `AlignToObjectPosition`.
    pub align_position: Param,
    /// `AlignToObjectOrientation`.
    pub align_orientation: Param,
    /// `CanKillDeathsDoor`.
    pub can_kill_deaths_door: bool,
    /// `ActionTarget`.
    pub target: ActionTarget,
    /// `ActionTargetEvaluationFrequencyMS`.
    pub target_evaluation_ms: Param,
}

/// `Parameters` (`SelectPositionParams`): how a navmesh point is picked (study section 1.4:
/// `PositionSelectMode` CirclePick 1, RingPick 4, Ticketing 3; `Direction` AnchorToOrigin 1,
/// OriginOrientation 2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SelectPositionParams {
    /// `Origin`.
    pub origin: PositionType,
    /// `Anchor`.
    pub anchor: PositionType,
    /// `Direction`.
    pub direction: Param,
    /// `PositionSelectMode`.
    pub mode: Param,
    /// `CheckPathToOrigin`.
    pub check_path_to_origin: bool,
    /// `CheckPathToAnchor`.
    pub check_path_to_anchor: bool,
    /// `StartAngle`.
    pub start_angle: Param,
    /// `StopAngle`.
    pub stop_angle: Param,
    /// `FavorMiddleAngle`.
    pub favor_middle_angle: bool,
    /// `RangeMin`.
    pub range_min: Param,
    /// `RangeMax`.
    pub range_max: Param,
    /// `DistanceFromWall`.
    pub distance_from_wall: Param,
    /// `QueryRadius`.
    pub query_radius: Param,
    /// `MaxAttempts`.
    pub max_attempts: Param,
    /// `StartAngleParam`.
    pub start_angle_param: Param,
    /// `StopAngleParam`.
    pub stop_angle_param: Param,
    /// `RangeMinParam`.
    pub range_min_param: Param,
    /// `RangeMaxParam`.
    pub range_max_param: Param,
    /// `DistanceFromWallParam`.
    pub distance_from_wall_param: Param,
    /// `QueryRadiusParam`.
    pub query_radius_param: Param,
    /// `MaxAttemptsParam`.
    pub max_attempts_param: Param,
    /// `GroupDetector`.
    pub group_detector: Param,
    /// `GroupRange`.
    pub group_range: Param,
}

/// The motion fields that `NavigationAction` and `StrafeAction` share.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    /// `NavConfigType`: the slice navigation config it uses.
    pub nav_config: Option<NameId>,
    /// `MaxSpeed`.
    pub max_speed: Param,
    /// `MinSpeed`.
    pub min_speed: Param,
    /// `LinearAcceleration`.
    pub linear_acceleration: Param,
    /// `LinearDeceleration`.
    pub linear_deceleration: Param,
    /// `ArrivalThreshold`.
    pub arrival_threshold: Param,
    /// `TargetRepathDistance`.
    pub target_repath_distance: Param,
    /// `AllowDynamicScaling`.
    pub allow_dynamic_scaling: bool,
    /// `AllowedStuckResolutions`.
    pub allowed_stuck_resolutions: Param,
    /// `GiveUpBehavior`.
    pub give_up_behavior: Param,
    /// `SelectPositionParams` when `HasSelectPositionParams` is set.
    pub select_position: Option<SelectPositionParams>,
}

/// `NavigationAction`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NavigationParams {
    /// The CAGE base (the move task).
    pub cage: CageParams,
    /// The shared motion fields.
    pub motion: Motion,
    /// `NavKey`: the blackboard position it moves to; `None`: the target or the selected
    /// position.
    pub nav_key: Option<KeyId>,
    /// `TurnRate`.
    pub turn_rate: Param,
    /// `AngularAcceleration`.
    pub angular_acceleration: Param,
    /// `AngularDeceleration`.
    pub angular_deceleration: Param,
    /// `FacingMode`.
    pub facing_mode: Param,
    /// `FaceMovementSmoothTime`.
    pub face_movement_smooth_time: Param,
    /// `AgentRadius`.
    pub agent_radius: Param,
    /// `Teleport`.
    pub teleport: bool,
    /// `AllowPartialPaths`.
    pub allow_partial_paths: bool,
    /// `CalculatePathOnExec`.
    pub calculate_path_on_exec: bool,
    /// `StartActionOverride`: a CAGE task to start the move with.
    pub start_action_override: Option<NameId>,
    /// `TurnActionOverride`: a CAGE task to turn with.
    pub turn_action_override: Option<NameId>,
    /// `ObserveHeat`.
    pub observe_heat: bool,
    /// `ApplyHeat`.
    pub apply_heat: bool,
    /// `MaxCorridorWidth`.
    pub max_corridor_width: Param,
    /// `NearbyNavLinkRemovalThreshold`.
    pub nearby_nav_link_removal_threshold: Param,
    /// `WaypointLookAheadRadius`.
    pub waypoint_look_ahead_radius: Param,
    /// `MaxSteeringForce`.
    pub max_steering_force: Param,
    /// `MaxAvoidanceForce`.
    pub max_avoidance_force: Param,
    /// `MaxSeparationForce`.
    pub max_separation_force: Param,
    /// `SteeringRampDownRangeMin`.
    pub steering_ramp_down_range_min: Param,
    /// `SteeringRampDownRangeMax`.
    pub steering_ramp_down_range_max: Param,
    /// `EnableAvoidance`.
    pub enable_avoidance: bool,
    /// `AvoidanceLookAheadHorizon`.
    pub avoidance_look_ahead_horizon: Param,
    /// `AvoidanceForce`.
    pub avoidance_force: Param,
    /// `EnableSeparation`.
    pub enable_separation: bool,
    /// `SeparationCircleForce`.
    pub separation_circle_force: Param,
    /// `SeparationCircleRadius`.
    pub separation_circle_radius: Param,
    /// `SeparationConeForce`.
    pub separation_cone_force: Param,
    /// `SeparationConeRange`.
    pub separation_cone_range: Param,
    /// `SeparationConeAngle`.
    pub separation_cone_angle: Param,
}

/// `StrafeAction`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrafeParams {
    /// The CAGE base (the strafe task).
    pub cage: CageParams,
    /// The shared motion fields.
    pub motion: Motion,
    /// `IdealDistanceMin`.
    pub ideal_distance_min: Param,
    /// `IdealDistanceMax`.
    pub ideal_distance_max: Param,
    /// `ApproachDistanceRatio`.
    pub approach_distance_ratio: Param,
    /// `RetreatDistanceRatio`.
    pub retreat_distance_ratio: Param,
    /// `LateralMovementFrequencyMin`.
    pub lateral_frequency_min: Param,
    /// `LateralMovementFrequencyMax`.
    pub lateral_frequency_max: Param,
    /// `LateralMovementAngleMin`.
    pub lateral_angle_min: Param,
    /// `LateralMovementAngleMax`.
    pub lateral_angle_max: Param,
    /// `DirectMovementFrequencyMin`.
    pub direct_frequency_min: Param,
    /// `DirectMovementFrequencyMax`.
    pub direct_frequency_max: Param,
    /// `AllowStandingStill`.
    pub allow_standing_still: Param,
    /// `StandStillFrequencyMin`.
    pub stand_still_frequency_min: Param,
    /// `StandStillFrequencyMax`.
    pub stand_still_frequency_max: Param,
    /// `StandStillDurationMin`.
    pub stand_still_duration_min: Param,
    /// `StandStillDurationMax`.
    pub stand_still_duration_max: Param,
    /// `RepositionKeepAwayDistance`.
    pub reposition_keep_away_distance: Param,
    /// `FacingConvergenceRate`.
    pub facing_convergence_rate: Param,
    /// `DirectionChangeRateDegs`.
    pub direction_change_rate: Param,
}

/// `ActionTargetTrackerDecorator`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackerParams {
    /// `ActionTarget`.
    pub target: ActionTarget,
    /// `ActionTargetEvaluationFrequencyMS`.
    pub evaluation_ms: Param,
    /// `TargetingProfile`.
    pub profile: Option<AssetRef>,
    /// `TargetTagFilter`.
    pub tag_filter: NameId,
    /// `MinTargets` (-1: not set).
    pub min_targets: Param,
    /// `MaxTargets`.
    pub max_targets: Param,
    /// `TopExclude` (-1: not set).
    pub top_exclude: Param,
    /// `TargetAllValidTargets`.
    pub all_valid_targets: bool,
    /// `TargetSelectionLimitsRangeBehavior`.
    pub range_behavior: Param,
}

/// `EntityQueryCondition`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityQueryParams {
    /// Entry, continuation and interrupt conditions over the query result.
    pub gates: Gates,
    /// `Center` (`None`: the mob).
    pub center: Param,
    /// `Radius` (at most 200 m: "Invalid (> 200) radius specified in EntityQueryCondition").
    pub radius: Param,
    /// `TagFilter`.
    pub tag_filter: NameId,
    /// `MatchingVitalsState`.
    pub matching_vitals_state: Param,
    /// `NotMatchingVitalsState`.
    pub not_matching_vitals_state: Param,
    /// `InThreatFilter`.
    pub in_threat_filter: Param,
    /// `InThreatFilterString`.
    pub in_threat_filter_string: NameId,
}

/// `SpawnProjectile`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectileParams {
    /// `ActionTarget`.
    pub target: ActionTarget,
    /// `TargetPos`.
    pub target_pos: Param,
    /// `UseAllAvailableTargets`.
    pub use_all_available_targets: bool,
    /// `MinTargetsPerShot`.
    pub min_targets_per_shot: Param,
    /// `MaxTargetsPerShot`.
    pub max_targets_per_shot: Param,
    /// `MinSecondsBetweenShots`.
    pub min_seconds_between_shots: Param,
    /// `MaxSecondsBetweenShots`.
    pub max_seconds_between_shots: Param,
}

/// `TagOperation` of `UpdateNWTags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagOperation {
    /// `Tag`.
    pub tag: NameId,
    /// `IsOperation`: add (`true`, `AddTag`) or remove (`false`, `RemoveTag`).
    pub add: bool,
    /// `PerformStrictOperation`.
    pub strict: bool,
}

/// The entry, continuation and interrupt conditions of a condition decorator (study section
/// 2.2: entry is tested to start, continuation while it runs, interrupt stops it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Gates {
    /// `EntryConditions`.
    pub entry: ConditionGroup,
    /// `ContinuationConditions`.
    pub continuation: ConditionGroup,
    /// `InterruptConditions`.
    pub interrupt: ConditionGroup,
}

/// The class of a node and its fields. Decorators hold `child: Option<NodeId>` (the client logs
/// "No child" and fails a decorator without one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    /// `Sequencer`: children in order; fails on the first failure.
    Sequencer(Children),
    /// `PermissiveSequencer`: children in order; a failed child counts as a success.
    PermissiveSequencer(Children),
    /// `Selector`: the first child that plans.
    Selector(Children),
    /// `UtilitySelector`.
    UtilitySelector {
        /// The children.
        children: Children,
        /// `SelectionMethod`.
        method: SelectionMethod,
        /// `WeightedRandomN`.
        top_n: u64,
        /// `PreserveExecutingPlanIfNoChange`.
        preserve_if_no_change: bool,
        /// `AlwaysPreserveExecutingPlan`.
        always_preserve: bool,
    },
    /// `Parallel`.
    Parallel {
        /// The children.
        children: Children,
        /// `CompletionMode`.
        completion_mode: Param,
    },
    /// `ForEachTargetDecorator`.
    ForEachTarget {
        /// The children.
        children: Children,
        /// `ExecutionIterationBehavior`.
        execution: Param,
        /// `EvaluationIterationBehavior`.
        evaluation: Param,
        /// `MaxNumberOfTargets`.
        max_targets: Param,
    },
    /// `TreeReference`: an inlined sub-tree.
    SubTree {
        /// The sub-tree's path (`Tree.Name`).
        path: NameId,
        /// Its root.
        root: Option<NodeId>,
    },
    /// `BlackboardCondition`.
    BlackboardCondition {
        /// The child.
        child: Option<NodeId>,
        /// The conditions.
        gates: Box<Gates>,
    },
    /// `DistanceCondition`: left operand a position, right a distance.
    DistanceCondition {
        /// The child.
        child: Option<NodeId>,
        /// The conditions.
        gates: Box<Gates>,
        /// `VerticalOffset`.
        vertical_offset: Param,
        /// `VerticalThreshold`.
        vertical_threshold: Param,
        /// `AllowDynamicScaling`.
        allow_dynamic_scaling: bool,
    },
    /// `CooldownDecorator`: fails while the cooldown runs; sets it after the child.
    Cooldown {
        /// The child.
        child: Option<NodeId>,
        /// The cooldown.
        cooldown: CooldownSpec,
        /// `OmitOnFailure`: no cooldown after a failed child.
        omit_on_failure: bool,
    },
    /// `IsCooldownActiveCondition`.
    IsCooldownActive {
        /// The child.
        child: Option<NodeId>,
        /// `CooldownName`.
        name: NameId,
        /// `InvertCheck`.
        invert: bool,
    },
    /// `MannequinTagCondition`.
    MannequinTag {
        /// The child.
        child: Option<NodeId>,
        /// `Tag`.
        tag: NameId,
        /// `InvertCheck`.
        invert: bool,
    },
    /// `SetMannequinTag`: the tag is set while the child runs.
    SetMannequinTag {
        /// The child.
        child: Option<NodeId>,
        /// `Tag`.
        tag: NameId,
    },
    /// `ActionTargetTrackerDecorator`.
    ActionTargetTracker {
        /// The child.
        child: Option<NodeId>,
        /// The fields.
        params: Box<TrackerParams>,
    },
    /// `Timer`: a time limit on the child.
    Timer {
        /// The child.
        child: Option<NodeId>,
        /// `TimeMin`.
        min: Param,
        /// `TimeMax`.
        max: Param,
        /// `SucceedOnTimeOut`.
        succeed_on_timeout: bool,
    },
    /// `FrequencyLimiter`: the child plans once per random interval.
    FrequencyLimiter {
        /// The child.
        child: Option<NodeId>,
        /// `IntervalMin`.
        min: Param,
        /// `IntervalMax`.
        max: Param,
        /// `StampOnSuccess`.
        stamp_on_success: bool,
        /// `StampOnFailure`.
        stamp_on_failure: bool,
    },
    /// `VariableExistsCondition`.
    VariableExists {
        /// The child.
        child: Option<NodeId>,
        /// `VariableName`.
        key: KeyId,
        /// `InvertCheck`.
        invert: bool,
    },
    /// `CountLimiter`.
    CountLimiter {
        /// The child.
        child: Option<NodeId>,
        /// `MaxCount`.
        max: Param,
        /// `CountOnSuccess`.
        count_on_success: bool,
        /// `CountOnFailure`.
        count_on_failure: bool,
    },
    /// `LockTargetSelection`: the target stays while the child runs.
    LockTargetSelection {
        /// The child.
        child: Option<NodeId>,
    },
    /// `RandomChanceCondition`.
    RandomChance {
        /// The child.
        child: Option<NodeId>,
        /// `SuccessRate` (0 to 1).
        rate: Param,
    },
    /// `StatusEffectDecorator`: a status effect while the child runs.
    StatusEffect {
        /// The child.
        child: Option<NodeId>,
        /// `StatusEffectId`.
        effect: NameId,
        /// `RemoveOnEnd`.
        remove_on_end: bool,
    },
    /// `EncounterEventDecorator`.
    EncounterEvent {
        /// The child.
        child: Option<NodeId>,
        /// `EventId`.
        event: NameId,
        /// `MaxCount`.
        max_count: Param,
        /// `SendOnSuccess`.
        on_success: bool,
        /// `SendOnFailure`.
        on_failure: bool,
        /// `SendOnCancelled`.
        on_cancelled: bool,
    },
    /// `AlwaysSucceed`: "Child failed, forced to success".
    AlwaysSucceed {
        /// The child.
        child: Option<NodeId>,
        /// `ApplyDuringEvaluation`.
        during_evaluation: bool,
        /// `ApplyDuringExecution`.
        during_execution: bool,
    },
    /// `VariableMonitor`.
    VariableMonitor {
        /// The child.
        child: Option<NodeId>,
        /// `VariableName`.
        key: KeyId,
    },
    /// `VisibilityRaycast`.
    VisibilityRaycast {
        /// The child.
        child: Option<NodeId>,
        /// `StartPosition`.
        start: PositionType,
        /// `EndPosition`.
        end: PositionType,
        /// `OffsetLength`.
        offset: Param,
    },
    /// `EntityQueryCondition`.
    EntityQuery {
        /// The child.
        child: Option<NodeId>,
        /// The fields.
        params: Box<EntityQueryParams>,
    },
    /// `Repeater`.
    Repeater {
        /// The child.
        child: Option<NodeId>,
        /// `RepeatCountMin`.
        count_min: Param,
        /// `RepeatCountMax`.
        count_max: Param,
        /// `RepeatSecondsMin`.
        seconds_min: Param,
        /// `RepeatSecondsMax`.
        seconds_max: Param,
    },
    /// `ManualTargeting`.
    ManualTargeting {
        /// The child.
        child: Option<NodeId>,
        /// `TargetVariable`.
        key: KeyId,
        /// `AssumeTargetVisible`.
        assume_visible: bool,
        /// `TargetHeightOffset`.
        height_offset: Param,
    },
    /// `CAGEAction`.
    Cage(Box<CageParams>),
    /// `NavigationAction`.
    Navigation(Box<NavigationParams>),
    /// `StrafeAction`.
    Strafe(Box<StrafeParams>),
    /// `SetCooldownAction`.
    SetCooldown(CooldownSpec),
    /// `SetBlackboardFactAction`.
    SetFact {
        /// `FactName`.
        key: KeyId,
        /// `Operator`.
        op: FactOp,
        /// `RightOperand`.
        value: Param,
    },
    /// `SetBlackboardFactOnRemoteEntityAction`.
    SetRemoteFact {
        /// `FactName`.
        fact: NameId,
        /// `Value`.
        value: Param,
        /// `OverwriteTargetFact`.
        overwrite: bool,
        /// `ActionTarget`.
        target: ActionTarget,
    },
    /// `TriggerEntity`.
    TriggerEntity {
        /// `TriggerName`.
        trigger: NameId,
    },
    /// `TriggerRemoteEntity`.
    TriggerRemoteEntity {
        /// `TriggerName`.
        trigger: NameId,
        /// `UseAllAvailableTargets`.
        all_targets: bool,
        /// `ActionTarget`.
        target: ActionTarget,
    },
    /// `SetBeamAttackActive`.
    SetBeam {
        /// `BeamName`.
        beam: NameId,
        /// `Active`.
        active: bool,
        /// `AimMode`.
        aim_mode: Param,
        /// `LockTarget`.
        lock_target: bool,
    },
    /// `ExecuteSpawn`.
    ExecuteSpawn {
        /// `Slice`.
        slice: AssetRef,
        /// `SliceAlias`.
        alias: Option<AssetRef>,
        /// `Count`.
        count: Param,
        /// `TargetPosBlackboardKey` (`None`: the mob's position).
        position_key: Option<KeyId>,
        /// `UseOwnerRotation`.
        use_owner_rotation: bool,
    },
    /// `SelectPositionAction`.
    SelectPosition {
        /// `Params`.
        params: Box<SelectPositionParams>,
        /// `StorageFact` (`None`: `Selected_Pos`).
        storage: Option<KeyId>,
        /// `AllowDynamicScaling`.
        allow_dynamic_scaling: bool,
    },
    /// `FakeAction`.
    Fake {
        /// `ExecuteIndefinitely`.
        indefinitely: bool,
    },
    /// `CompleteOrder`.
    CompleteOrder,
    /// `AssignOrder`.
    AssignOrder {
        /// `Order` (CRC32 of the order name).
        order: u32,
    },
    /// `CountEntitiesWithTag`.
    CountEntitiesWithTag {
        /// `TagName`.
        tag: NameId,
        /// `BlackboardKey`: where the count goes.
        key: KeyId,
    },
    /// `Delay`.
    Delay {
        /// `TimeMin`.
        min: Param,
        /// `TimeMax`.
        max: Param,
    },
    /// `SelectWaypointAction`.
    SelectWaypoint,
    /// `SpawnProjectile`.
    SpawnProjectile(Box<ProjectileParams>),
    /// `UpdateNWTags`.
    UpdateNwTags {
        /// `MainTagOperation`.
        main: TagOperation,
        /// `TagOperations`.
        operations: Vec<TagOperation>,
    },
}

impl NodeKind {
    /// The child of a decorator; `None` for other classes and a decorator without a child.
    pub fn child(&self) -> Option<NodeId> {
        match self {
            Self::SubTree { root: child, .. }
            | Self::BlackboardCondition { child, .. }
            | Self::DistanceCondition { child, .. }
            | Self::Cooldown { child, .. }
            | Self::IsCooldownActive { child, .. }
            | Self::MannequinTag { child, .. }
            | Self::SetMannequinTag { child, .. }
            | Self::ActionTargetTracker { child, .. }
            | Self::Timer { child, .. }
            | Self::FrequencyLimiter { child, .. }
            | Self::VariableExists { child, .. }
            | Self::CountLimiter { child, .. }
            | Self::LockTargetSelection { child }
            | Self::RandomChance { child, .. }
            | Self::StatusEffect { child, .. }
            | Self::EncounterEvent { child, .. }
            | Self::AlwaysSucceed { child, .. }
            | Self::VariableMonitor { child, .. }
            | Self::VisibilityRaycast { child, .. }
            | Self::EntityQuery { child, .. }
            | Self::Repeater { child, .. }
            | Self::ManualTargeting { child, .. } => *child,
            _ => None,
        }
    }

    /// Whether the class is a decorator (it has `Child`; a sub-tree reference is not one).
    pub fn is_decorator(&self) -> bool {
        matches!(
            self,
            Self::BlackboardCondition { .. }
                | Self::DistanceCondition { .. }
                | Self::Cooldown { .. }
                | Self::IsCooldownActive { .. }
                | Self::MannequinTag { .. }
                | Self::SetMannequinTag { .. }
                | Self::ActionTargetTracker { .. }
                | Self::Timer { .. }
                | Self::FrequencyLimiter { .. }
                | Self::VariableExists { .. }
                | Self::CountLimiter { .. }
                | Self::LockTargetSelection { .. }
                | Self::RandomChance { .. }
                | Self::StatusEffect { .. }
                | Self::EncounterEvent { .. }
                | Self::AlwaysSucceed { .. }
                | Self::VariableMonitor { .. }
                | Self::VisibilityRaycast { .. }
                | Self::EntityQuery { .. }
                | Self::Repeater { .. }
                | Self::ManualTargeting { .. }
        )
    }

    /// The children of a composite.
    pub fn children(&self) -> Option<Children> {
        match self {
            Self::Sequencer(children)
            | Self::PermissiveSequencer(children)
            | Self::Selector(children)
            | Self::UtilitySelector { children, .. }
            | Self::Parallel { children, .. }
            | Self::ForEachTarget { children, .. } => Some(*children),
            _ => None,
        }
    }

    /// The CAGE task a leaf requests, when it is a CAGE, navigation or strafe action.
    pub fn task(&self) -> Option<Param> {
        match self {
            Self::Cage(cage) => Some(cage.task),
            Self::Navigation(nav) => Some(nav.cage.task),
            Self::Strafe(strafe) => Some(strafe.cage.task),
            _ => None,
        }
    }
}
