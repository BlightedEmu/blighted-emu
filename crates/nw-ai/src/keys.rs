// SPDX-License-Identifier: AGPL-3.0-or-later
//! The blackboard keys the engine writes (behavior-tree study, section 3.1).
//!
//! Observed: the client exe holds a table of 80 key names at 0x83777a8 to 0x8377dc8, built at
//! start-up by `FUN_1446fd700` (called from `FUN_144697690`); study `builtin-bbkeys.txt`. Who
//! writes each key and how is **Unknown** (the writers use CRCs computed at run time; a search of
//! the decompile for the constants found nothing): the sim supplies them through
//! [`crate::brain::World::engine_value`]. Meanings are Inferred from the names and their use in the
//! trees (for example `Target_Dist` in metres, compared with 1 to 7; `Target_Angle` in signed
//! degrees, compared with +-30 and +-80).
//!
//! Two key families are not in the table: `SE_<status effect id>` and `SECat_<category>`, true while
//! such a status effect is active (Inferred from their use, for example `SE_Sandworm_Invulnerability`
//! and `SECat_Root`).

/// Prefix of the key that exists while a CAGE task runs (`CAGE_<task>`, for example
/// `CAGE_Move_Combat_CAGE_Task` in Turn.behaviortree and `CAGE_Reset_Idle_CAGE_Task`; Inferred
/// from their use with `VariableExistsCondition`).
pub const CAGE_TASK_PREFIX: &str = "CAGE_";
/// Prefix of a status effect key.
pub const STATUS_EFFECT_PREFIX: &str = "SE_";
/// Prefix of a status effect category key.
pub const STATUS_CATEGORY_PREFIX: &str = "SECat_";

macro_rules! engine_keys {
    ($($variant:ident => $name:literal,)*) => {
        /// A key of the engine table.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum EngineKey {
            $(
                #[doc = concat!("`", $name, "`.")]
                $variant,
            )*
        }

        impl EngineKey {
            /// Every engine key, in table order.
            pub const ALL: &'static [EngineKey] = &[$(EngineKey::$variant,)*];

            /// The key's name in the trees.
            pub const fn name(self) -> &'static str {
                match self {
                    $(EngineKey::$variant => $name,)*
                }
            }
        }
    };
}

engine_keys! {
    ActionTargetAngle => "Action_Target_Angle",
    ActionTargetCount => "Action_Target_Count",
    ActionTargetDist => "Action_Target_Dist",
    ActionTargetGdeId => "Action_Target_GDE_ID",
    ActionTargetIndex => "Action_Target_Index",
    ActionTargetOrientation => "Action_Target_Orientation",
    ActionTargetOrientationDelta => "Action_Target_Orientation_Delta",
    ActionTargetPos => "Action_Target_Pos",
    AiOrder => "AI_Order",
    AttackObjectiveGdeRef => "Attack_Objective_GDERef",
    AttackObjectiveOrientation => "Attack_Objective_Orientation",
    AttackObjectivePos => "Attack_Objective_Pos",
    AttackObjectiveRootPos => "Attack_Objective_Root_Pos",
    CreatureType => "Creature_Type",
    ChainedReactionCount => "Chained_Reaction_Count",
    CombatLeashDistance => "CombatLeashDistance",
    DamageFilter => "Damage_Filter",
    DestinationDist => "Destination_Dist",
    DifficultyTier => "Difficulty_Tier",
    DynamicEntityScalar => "Dynamic_Entity_Scalar",
    FullBodyReaction => "Full_Body_Reaction",
    GuardObjectiveGdeRef => "Guard_Objective_GDERef",
    GuardObjectiveOrientation => "Guard_Objective_Orientation",
    GuardObjectivePos => "Guard_Objective_Pos",
    GuardObjectiveRootPos => "Guard_Objective_Root_Pos",
    HasStealthTarget => "Has_Stealth_Target",
    HealthPercent => "Health_Percent",
    IsMinion => "Is_Minion",
    LeashOptOut => "Leash_Opt_Out",
    LeashTimeLimit => "Leash_Time_Limit",
    ManualTargeting => "Manual_Targeting",
    MinionCount => "Minion_Count",
    MyServerGdeId => "My_Server_GDE_ID",
    NavEngageRange => "Nav_Engage_Range",
    NavKey => "Nav_Key",
    NavLinkStartPos => "Nav_Link_Start_Pos",
    NavLinkStartAlign => "Nav_Link_Start_Align",
    NavLinkEndPos => "Nav_Link_End_Pos",
    NavLinkEndAlign => "Nav_Link_End_Align",
    OrientationDelta => "Orientation_Delta",
    PathDistanceToTarget => "Path_Distance_To_Target",
    PounceFeatureDisabled => "Pounce_FeatureDisabled",
    RangedAccuracy => "Ranged_Accuracy",
    RangedDynamicAccuracy => "Ranged_Dynamic_Accuracy",
    RangedAttackRoll => "Ranged_Attack_Roll",
    SelectedPos => "Selected_Pos",
    ShouldMotionBeStopped => "Should_Motion_Be_Stopped",
    SpawnerGdeRef => "Spawner_GDERef",
    SpawnPos => "Spawn_Pos",
    SpawnRequestServerTime => "Spawn_Request_Server_Time",
    SpawnRequestTimeOfDay => "Spawn_Request_TimeOfDay",
    TargetAngle => "Target_Angle",
    TargetDist => "Target_Dist",
    TargetGdeId => "Target_GDE_ID",
    TargetLastKnownPos => "Target_Last_Known_Pos",
    TargetOrientation => "Target_Orientation",
    TargetPos => "Target_Pos",
    TargetPosInCone => "Target_Pos_In_Cone",
    TargetScore => "Target_Score",
    TargetVisibilityScore => "Target_Visibility_Score",
    TimeSinceTargetReachable => "Time_Since_Target_Reachable",
    UsePathDistanceLeashing => "Use_Path_Distance_Leashing",
    VisionAngle => "Vision_Angle",
    VisionDistance => "Vision_Distance",
    VitalsState => "Vitals_State",
    WaypointAlign => "Waypoint_Align",
    WaypointDuration => "Waypoint_Duration",
    WaypointDurationMin => "Waypoint_Duration_Min",
    WaypointDurationMax => "Waypoint_Duration_Max",
    WaypointPos => "Waypoint_Pos",
    WaypointLookAheadRadius => "Waypoint_LookAhead_Radius",
    HasTarget => "Has_Target",
    LastDamagePos => "Last_Damage_Pos",
    TargetVisible => "Target_Visible",
    TargetVitalsState => "Target_Vitals_State",
    TargetReachable => "Target_Reachable",
    TargetDirectlyReachable => "Target_Directly_Reachable",
    WaypointTask => "Waypoint_Task",
    BossPhase => "Boss_Phase",
    Passive => "Passive",
}

impl EngineKey {
    /// The engine key named `name` (without case: keys match by the lower-case CRC32 of the
    /// name, study section 1.2).
    pub fn of(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|key| key.name().eq_ignore_ascii_case(name))
    }
}

/// Where a key's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// Written by the tree (facts) or set by the slice config; nothing else.
    Tree,
    /// An engine key.
    Engine(EngineKey),
    /// `SE_<id>`: true while a status effect with this id (the name id of the rest) is active.
    StatusEffect(u32),
    /// `SECat_<category>`: true while a status effect of this category is active.
    StatusCategory(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Names in the exe table (study `builtin-bbkeys.txt`).
    const TABLE_LEN: usize = 80;

    #[test]
    fn the_table_has_80_distinct_names() {
        assert_eq!(EngineKey::ALL.len(), TABLE_LEN);
        let mut names: Vec<&str> = EngineKey::ALL.iter().map(|key| key.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TABLE_LEN);
    }

    #[test]
    fn keys_match_without_case() {
        assert_eq!(EngineKey::of("target_dist"), Some(EngineKey::TargetDist));
        assert_eq!(EngineKey::of("Has_Target"), Some(EngineKey::HasTarget));
        assert_eq!(EngineKey::of("AttackCooldown"), None);
    }
}
