// SPDX-License-Identifier: AGPL-3.0-or-later
//! Trees that neither Server.pak nor the client holds, built from held trees.
//!
//! `damned_firstmate_2h_anchor_basebehavior` (First Mate Janszoon,
//! `Corrupted_Sailor_StarterBeach_Named_00`) is missing. In the retail new-character run he
//! attacks with `Attack_Far_2H_Axe_01`, `Attack_Close_2H_Axe_04` and once with
//! `Officer_AnchorToss` (capture 1246273 ms, the player 8.7 m away, right after
//! `Attack_Close_2H_Axe_04`). No held tree holds all three. His tree is built from two (Inferred,
//! the base chosen by the user 2026-09-26, the toss added on the user's request 2026-10-30):
//!
//! - the damned great axeman's `damned_2h_melee_basebehavior`, which holds the two axe attacks;
//! - with, first in its combat selector, the toss of the undead officer's `FarRange` branch
//!   (target at 6 to 10 m and visible: lock the target and throw the anchor, a 6 to 9 s
//!   cooldown), the only toss of the held trees whose range holds the retail 8.7 m. The branch's
//!   other choices (the officer's ocean shield, anchor dash and run attack) are left out: the
//!   retail Janszoon showed none of them. The officer throws only at difficulty tier 2 or more;
//!   Janszoon's slice sets tier 1 and he threw in retail, so that one condition is left out too;
//! - with, first in the close range selector, the officer's close `SpellCDCheck` branch: the
//!   anchor slam behind its `SpellCD` cooldown (10 to 14 s at his tier). A second retail fight
//!   (the Linux main quest playthrough, part 1, 819480 ms) shows `Officer_AnchorSlam` right after
//!   `Attack_Combo_GreatAxeman_3`. The branch's ocean shield is left out (no retail fight shows
//!   it). Its place in the selector is Inferred. The branch stays inside the officer's own
//!   `CloseRange` condition (target within 3 m): the slam does not move him in.

use nw_ai::classes::{Class, class_of};
use nw_ai::data::{Archetype, CageAction, Grid};
use nw_ai::decode::{DecodeError, TreeDecoder};
use nw_ai::node::{
    ActionTarget, CageParams, CooldownOverride, CooldownSpec, Gates, Motion, NavRayCast,
    NavigationParams, Node, NodeId, NodeKind, SelectionMethod, StrafeParams, TargetLimits,
};
use nw_ai::value::{CompareOp, Condition, ConditionGroup, NameId, Param, Value};
use nw_formats::objectstream::{Element, lowercase_crc32, parse_object_stream};

/// Janszoon's missing tree.
pub const JANSZOON_TREE: &str =
    "sharedassets/ai/behaviortrees/damned/damned_firstmate_2h_anchor_basebehavior.behaviortree";
/// The damned great axeman's base behavior.
const GREAT_AXEMAN_TREE: &str =
    "sharedassets/ai/behaviortrees/damned/damned_2h_melee_basebehavior.behaviortree";
/// The undead officer's combat behavior, which holds the anchor toss.
const OFFICER_COMBAT_TREE: &str =
    "sharedassets/ai/behaviortrees/undead/undead_officer_combatbehavior.behaviortree";
/// The great axeman's combat sub-tree (its `Tree` name, lower case).
const GREAT_AXEMAN_COMBAT: &str = "damnedgreataxeman_combatbehavior";
/// The selector of the combat sub-tree that picks the range branch.
const COMBAT_SELECTOR: &str = "Combat";
/// The officer's branch for a target at 6 to 10 m.
const TOSS_BRANCH: &str = "FarRange";
/// The selector of [`TOSS_BRANCH`].
const TOSS_SELECTOR: &str = "FarSelector";
/// The officer's tier check inside [`TOSS_SELECTOR`], above the toss.
const TIER_CHECK: &str = "MediumDifficultyCheck";
/// The key of the tier condition.
const TIER_KEY: &str = "Difficulty_Tier";
/// The officer's branch for a target within 3 m (`Target_Dist <= 3.0`), which holds
/// [`SLAM_BRANCH`].
const SLAM_RANGE: &str = "CloseRange";
/// The officer's close range branch that slams behind `SpellCD`.
const SLAM_BRANCH: &str = "SpellCDCheck";
/// The selector of [`SLAM_BRANCH`] that picks the shield or the slam.
const SLAM_SELECTOR: &str = "SpellSelector";
/// The slam's CAGE task.
const SLAM_TASK: &str = "Officer_AnchorSlam_CAGE_Task";
/// The great axeman's close range branch of its combat selector.
const CLOSE_RANGE: &str = "CloseMeleeRange";
/// The selector of [`CLOSE_RANGE`].
const CLOSE_SELECTOR: &str = "CloseAttackSel";

const NAME: &str = "Name";
const ROOT: &str = "Root";
const CHILDREN: &str = "Children";
const CHILD: &str = "Child";
const ENTRY_CONDITIONS: &str = "EntryConditions";
const CONDITIONS: &str = "Conditions";
const BASE_CLASS: &str = "BaseClass1";

/// Why a stand-in tree cannot be built.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StandInError {
    /// A tree it is built from is not in Server.pak.
    #[error("stand-in tree {tree}: {from} is not in Server.pak")]
    MissingSource {
        /// The tree to build.
        tree: &'static str,
        /// The missing source tree.
        from: &'static str,
    },
    /// A source tree does not parse.
    #[error("stand-in tree {tree}: {from} does not parse: {reason}")]
    Parse {
        /// The tree to build.
        tree: &'static str,
        /// The source tree.
        from: &'static str,
        /// Why.
        reason: String,
    },
    /// A node it edits is not where the source trees had it.
    #[error("stand-in tree {tree}: {what} not found")]
    Shape {
        /// The tree to build.
        tree: &'static str,
        /// The node or field.
        what: &'static str,
    },
}

/// The stand-in trees (path and `Tree` root) built from the Server.pak tree `entries` (path
/// and ObjectStream bytes).
///
/// # Errors
///
/// Returns [`StandInError`] when a source tree is missing or not in its known shape.
pub fn stand_in_trees(
    entries: &[(String, Vec<u8>)],
) -> Result<Vec<(&'static str, Element)>, StandInError> {
    Ok(vec![(JANSZOON_TREE, janszoon_tree(entries)?)])
}

fn janszoon_tree(entries: &[(String, Vec<u8>)]) -> Result<Element, StandInError> {
    let shape = |what| StandInError::Shape {
        tree: JANSZOON_TREE,
        what,
    };
    let mut root = source(entries, GREAT_AXEMAN_TREE)?;
    let officer = source(entries, OFFICER_COMBAT_TREE)?;
    let mut toss = branch(&officer, TOSS_BRANCH, None).ok_or_else(|| shape(TOSS_BRANCH))?;
    if keep_one_choice(&mut toss, TOSS_SELECTOR, &|node| {
        node_name(node) == Some(TIER_CHECK)
    }) != 1
    {
        return Err(shape(TOSS_SELECTOR));
    }
    let mut slam =
        branch(&officer, SLAM_BRANCH, Some(SLAM_TASK)).ok_or_else(|| shape(SLAM_BRANCH))?;
    if keep_one_choice(&mut slam, SLAM_SELECTOR, &|node| {
        holds_text(node, SLAM_TASK)
    }) != 1
    {
        return Err(shape(SLAM_SELECTOR));
    }
    // The slam keeps the officer's own range: the great axeman's close range is 4 m, and the
    // slam does not move him in (its homing clip: `MoveToTarget` false). Picked at up to 4 m it
    // did not reach a player 2.6 to 3.9 m away (live 2026-11-26 00:21 to 00:23).
    let mut slam_in_range =
        branch(&officer, SLAM_RANGE, Some(SLAM_TASK)).ok_or_else(|| shape(SLAM_RANGE))?;
    slam_in_range
        .children
        .first_mut()
        .and_then(|node| field_mut(node, CHILD))
        .ok_or_else(|| shape(SLAM_RANGE))?
        .children
        .clone_from(&slam.children);
    let slam = slam_in_range;
    let mut tier_conditions = 0;
    for_each_mut(&mut toss, &mut |element| {
        if node_name(element) != Some(TIER_CHECK) {
            return;
        }
        let Some(list) =
            field_mut(element, ENTRY_CONDITIONS).and_then(|group| field_mut(group, CONDITIONS))
        else {
            return;
        };
        let before = list.children.len();
        list.children
            .retain(|condition| !holds_text(condition, TIER_KEY));
        tier_conditions += before - list.children.len();
    });
    if tier_conditions != 1 {
        return Err(shape(TIER_KEY));
    }
    let mut grafts = 0;
    let mut slams = 0;
    for_each_mut(&mut root, &mut |element| {
        let is_combat_tree = class_of(&element.type_id) == Some(Class::Tree)
            && field(element, NAME)
                .and_then(text)
                .is_some_and(|name| name.to_lowercase().contains(GREAT_AXEMAN_COMBAT));
        if !is_combat_tree {
            return;
        }
        let Some(list) = field_mut(element, ROOT)
            .and_then(|pointer| pointer.children.first_mut())
            .filter(|node| node_name(node) == Some(COMBAT_SELECTOR))
            .and_then(|selector| field_mut(selector, CHILDREN))
        else {
            return;
        };
        list.children.insert(0, toss.clone());
        grafts += 1;
        let close_choices = list
            .children
            .iter_mut()
            .filter_map(|pointer| pointer.children.first_mut())
            .filter(|node| node_name(node) == Some(CLOSE_RANGE))
            .filter_map(|node| field_mut(node, CHILD))
            .filter_map(|pointer| pointer.children.first_mut())
            .filter(|node| node_name(node) == Some(CLOSE_SELECTOR))
            .filter_map(|selector| field_mut(selector, CHILDREN));
        for choices in close_choices {
            choices.children.insert(0, slam.clone());
            slams += 1;
        }
    });
    if grafts == 0 {
        return Err(shape(GREAT_AXEMAN_COMBAT));
    }
    if slams != grafts {
        return Err(shape(CLOSE_SELECTOR));
    }
    Ok(root)
}

/// A copy of the first node pointer under `tree` whose node is named `name` and, with `task`,
/// holds that CAGE task.
fn branch(tree: &Element, name: &str, task: Option<&str>) -> Option<Element> {
    find(tree, &|element| {
        class_of(&element.type_id) == Some(Class::NodePtr)
            && element.children.first().is_some_and(|node| {
                node_name(node) == Some(name) && task.is_none_or(|task| holds_text(node, task))
            })
    })
    .cloned()
}

/// Keeps, in every selector named `selector` under `branch`, only the choices whose node `keep`
/// accepts; the number of choices kept.
fn keep_one_choice(branch: &mut Element, selector: &str, keep: &dyn Fn(&Element) -> bool) -> usize {
    let mut kept = 0;
    for_each_mut(branch, &mut |element| {
        if node_name(element) != Some(selector) {
            return;
        }
        if let Some(list) = field_mut(element, CHILDREN) {
            list.children
                .retain(|pointer| pointer.children.first().is_some_and(keep));
            kept += list.children.len();
        }
    });
    kept
}

/// The parsed `Tree` root of the Server.pak tree `path`.
fn source(entries: &[(String, Vec<u8>)], path: &'static str) -> Result<Element, StandInError> {
    let (_, bytes) =
        entries
            .iter()
            .find(|(known, _)| known == path)
            .ok_or(StandInError::MissingSource {
                tree: JANSZOON_TREE,
                from: path,
            })?;
    parse_object_stream(bytes).map_err(|error| StandInError::Parse {
        tree: JANSZOON_TREE,
        from: path,
        reason: error.to_string(),
    })
}

/// The field `name` of `element`, its base classes included.
fn field<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    let crc = lowercase_crc32(name);
    let base = lowercase_crc32(BASE_CLASS);
    element
        .children
        .iter()
        .find_map(|child| match child.name_crc {
            Some(known) if known == crc => Some(child),
            Some(known) if known == base => field(child, name),
            _ => None,
        })
}

/// The field `name` of `element`, its base classes included, to change.
fn field_mut<'a>(element: &'a mut Element, name: &str) -> Option<&'a mut Element> {
    let crc = lowercase_crc32(name);
    let base = lowercase_crc32(BASE_CLASS);
    let index = element.children.iter().position(|child| {
        child.name_crc == Some(crc)
            || (child.name_crc == Some(base) && field(child, name).is_some())
    })?;
    let child = &mut element.children[index];
    if child.name_crc == Some(crc) {
        Some(child)
    } else {
        field_mut(child, name)
    }
}

/// The text of a string element.
fn text(element: &Element) -> Option<&str> {
    (class_of(&element.type_id) == Some(Class::String))
        .then(|| std::str::from_utf8(&element.value).ok())
        .flatten()
}

/// The `Name` of a node class element; `None` for the base class parts inside one.
fn node_name(node: &Element) -> Option<&str> {
    if node.name_crc == Some(lowercase_crc32(BASE_CLASS)) {
        return None;
    }
    field(node, NAME).and_then(text)
}

/// Whether a string element under `element` holds `wanted`.
fn holds_text(element: &Element, wanted: &str) -> bool {
    text(element) == Some(wanted)
        || element
            .children
            .iter()
            .any(|child| holds_text(child, wanted))
}

/// The first element under `element` (itself included, depth first) that `test` accepts.
fn find<'a>(element: &'a Element, test: &dyn Fn(&Element) -> bool) -> Option<&'a Element> {
    if test(element) {
        return Some(element);
    }
    element.children.iter().find_map(|child| find(child, test))
}

/// Calls `visit` on `element` and every element under it, parents first.
fn for_each_mut(element: &mut Element, visit: &mut dyn FnMut(&mut Element)) {
    visit(element);
    for child in &mut element.children {
        for_each_mut(child, visit);
    }
}

/// Tutorial trees whose mobs run their measured retail script (`nw_sim_core::ChaseBrain`):
/// never bind a stand-in tree for them, or the tutorial script would be replaced.
const FTUE_SCRIPTED_TREES: [&str; 3] = [
    "sharedassets/ai/behaviortrees/drowned/ftue/drowned_qte_basebehavior.behaviortree",
    "sharedassets/ai/behaviortrees/legion/legion_ftue/legion_ftue_basebehavior.behaviortree",
    "sharedassets/ai/behaviortrees/quest_npcs/ftue_thorpe_v2/ftue_thorpe_v2_basebehavior.behaviortree",
];

/// Missing trees whose exact sibling base tree exists in `Server.pak` (Inferred: newer
/// parameter-override or solo variants of a creature family whose base tree Server.pak holds).
const SIBLING_TREES: [(&str, &str); 13] = [
    (
        "sharedassets/ai/behaviortrees/rat/rat_plaguerat_base_overrides.behaviortree",
        "sharedassets/ai/behaviortrees/rat/rat_plaguerat_base.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/prey/fox/fox_base.behaviortree",
        "sharedassets/ai/behaviortrees/prey/prey_base.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/predator/lion/lion_diver.behaviortree",
        "sharedassets/ai/behaviortrees/predator/lion/lion_base.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/catacombs/ancientguardianheavy_boss/catacombs_ancientguardianheavyboss_base_overrides.behaviortree",
        "sharedassets/ai/behaviortrees/ancientguardian/ancientguardian_heavyboss_basebehavior.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/catacombs/dryadsiren_boss/catacombs_dryadsirenboss_base_overrides.behaviortree",
        "sharedassets/ai/behaviortrees/dryad_siren/dryadsiren_basebehavior.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/catacombs/gorillaelemental_boss/catacombs_gorillaelemental_base_overrides.behaviortree",
        "sharedassets/ai/behaviortrees/gorilla/gorilla_boss_elemental_basebehavior.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/catacombs/nagacorrupted_boss/catacombs_naga_corrupted_base_overrides.behaviortree",
        "sharedassets/ai/behaviortrees/naga/naga_basebehavior.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/dynasty_empress/soultrialversion/empress_solo_base_behavior.behaviortree",
        "sharedassets/ai/behaviortrees/dynasty_empress/empress_base_behavior.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/isabella/isabella_solo_msq2.3/isabella_solomsq2_base_behavior.behaviortree",
        "sharedassets/ai/behaviortrees/isabella/isabella_base_behavior.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/medea/medea_console_basebehavior.behaviortree",
        "sharedassets/ai/behaviortrees/medea/medea_basebehavior.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/medusa/medusa_maze_basebehavior.behaviortree",
        "sharedassets/ai/behaviortrees/medusa/medusa_basebehavior.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/hercyne/gorilla_h_base_behavior.behaviortree",
        "sharedassets/ai/behaviortrees/gorilla/gorilla_boss_basebehavior.behaviortree",
    ),
    (
        "sharedassets/ai/behaviortrees/hercyne/hercyne_pounce_base_behavior.behaviortree",
        "sharedassets/ai/behaviortrees/hercyne/hercyne_base_behavior.behaviortree",
    ),
];

/// Standard `Server.pak` sub-trees reused by grid-built stand-in trees.
const BASEAI_STUN: &str = "sharedassets/ai/behaviortrees/baseai/stun.behaviortree";
const BASEAI_DISTANCE_LEASH: &str =
    "sharedassets/ai/behaviortrees/baseai/distanceleash.behaviortree";
const BASEAI_UNREACHABLE_LEASH: &str =
    "sharedassets/ai/behaviortrees/baseai/targetisunreachable_leash.behaviortree";
const BASEAI_TARGET_DEATH_LEASH: &str =
    "sharedassets/ai/behaviortrees/baseai/targetdeath_leash.behaviortree";
const BASEAI_PATROL: &str = "sharedassets/ai/behaviortrees/baseai/patrol.behaviortree";
const BASEAI_WANDER: &str = "sharedassets/ai/behaviortrees/baseai/wander.behaviortree";
const BASEAI_DAMAGE_OUTSIDE_AOI: &str =
    "sharedassets/ai/behaviortrees/baseai/reactorresettodamageoutsideaoi.behaviortree";
const BASEAI_TURN: &str = "sharedassets/ai/behaviortrees/turn.behaviortree";

/// CAGE task names checked in a creature's grid when building a stand-in tree.
const TASK_STUN: &str = "Stun_CAGE_Task";
const TASK_RESET_MOVE: &str = "Reset_Move_CAGE_Task";
const TASK_RESET_IDLE: &str = "Reset_Idle_CAGE_Task";
const TASK_MOVE_COMBAT: &str = "Move_Combat_CAGE_Task";
const TASK_MOVE: &str = "Move_CAGE_Task";
const TASK_TURN: &str = "Combat_Turn_CAGE_Task";
const TASK_STRAFE: &str = "Strafe_CAGE_Task";
const TASK_COMBAT_IDLE: &str = "Combat_Idle_CAGE_Task";

/// Nav config names used by grid-built stand-in trees.
const NAV_RUN_TO_TARGET: &str = "RunToTarget";
const NAV_RUN_CLOSE_QUARTERS: &str = "RunToTargetCloseQuarters";
const NAV_WALK_TO_TARGET: &str = "WalkToTarget";
const NAV_STRAFE: &str = "Strafe";

/// Proc window kinds in a CAGE fragment variant that classify an attack action.
const WINDOW_DAMAGE: &str = "CAGE-Damage";
const WINDOW_RANGED: &str = "CAGE-RangedAttack";
const WINDOW_SPELL: &str = "CAGE-CastSpell";
const WINDOW_HOMING: &str = "Homing";
const PARAM_MAX_MOVE_DISTANCE: &str = "MaxMoveDistance";
const PARAM_MOVE_TO_DISTANCE: &str = "MoveToDistance";
const PARAM_SHAPE_RADIUS: &str = "MeleeAttackShapeRadius";

/// Action-list path fragment for shared non-attack baseai lists.
const BASEAI_LIST_MARKER: &str = "baseai_";

/// Task name fragments that mark non-combat or combo-continuation tasks (skipped as standalone
/// combat picks).
const NON_ATTACK_TASK_MARKERS: [&str; 22] = [
    "reaction_",
    "breakout",
    "death",
    "spawn",
    "despawn",
    "idle",
    "move",
    "strafe",
    "turn",
    "reset",
    "alert",
    "fidget",
    "waypoint",
    "poi_",
    "attack_structure",
    "attack_cooldown",
    "block",
    "taunt",
    "stun",
    "stagger",
    "_end_",
    "_success_",
];

/// Blackboard keys read by grid-built stand-in trees.
const BB_FULL_BODY_REACTION: &str = "Full_Body_Reaction";
const BB_HAS_TARGET: &str = "Has_Target";
const BB_TARGET_DIST: &str = "Target_Dist";
const BB_TARGET_VISIBLE: &str = "Target_Visible";
const BB_TARGET_DIRECTLY_REACHABLE: &str = "Target_Directly_Reachable";

/// Combat ranges, cooldowns and utility weights for grid-built stand-in trees (Inferred).
const MELEE_DEFAULT_RANGE_M: f32 = 3.5;
const MELEE_MIN_RANGE_M: f32 = 2.5;
const MELEE_MAX_RANGE_M: f32 = 5.0;
const RANGED_MIN_RANGE_M: f32 = 4.0;
const RANGED_MAX_RANGE_M: f32 = 20.0;
const MELEE_CHASE_STOP_M: f32 = 2.0;
const RANGED_CHASE_STOP_M: f32 = 10.0;
const MELEE_COOLDOWN_MIN_S: f32 = 1.5;
const MELEE_COOLDOWN_MAX_S: f32 = 2.5;
const RANGED_COOLDOWN_MIN_S: f32 = 2.5;
const RANGED_COOLDOWN_MAX_S: f32 = 4.5;
const TURN_INTERVAL_MIN_S: f32 = 0.5;
const TURN_INTERVAL_MAX_S: f32 = 1.0;
const DEFAULT_UTILITY: f32 = 1.0;
const WEIGHTED_RANDOM_TOP_N: u64 = 16;
const TARGET_EVAL_MS: i64 = 250;
const UNSET_LIMIT: i64 = -1;
const MELEE_STRAFE_MIN_M: f32 = 2.0;
const MELEE_STRAFE_MAX_M: f32 = 4.0;
const RANGED_STRAFE_MIN_M: f32 = 8.0;
const RANGED_STRAFE_MAX_M: f32 = 14.0;
const STRAFE_APPROACH_RATIO: f32 = 1.25;
const STRAFE_RETREAT_RATIO: f32 = 0.75;
const STRAFE_FREQ_MIN_S: f32 = 1.0;
const STRAFE_FREQ_MAX_S: f32 = 2.5;
const STRAFE_ANGLE_MIN_DEG: f32 = 45.0;
const STRAFE_ANGLE_MAX_DEG: f32 = 90.0;
const STRAFE_STAND_STILL_MIN_S: f32 = 0.5;
const STRAFE_STAND_STILL_MAX_S: f32 = 1.5;
const STRAFE_KEEP_AWAY_M: f32 = 1.5;
const STRAFE_FACING_RATE: f32 = 0.25;
const STRAFE_DIR_RATE_DEG: f32 = 90.0;

/// Fills missing behavior trees for archetypes whose `.behaviortree` Server.pak does not hold
/// (spiders, plague rats, werebeasts, Upyr, Inquisitors and more):
///
/// 1. Leaves the three FTUE tutorial trees unbound so the tutorial keeps its measured script.
/// 2. Binds known sibling trees from `Server.pak` ([`SIBLING_TREES`]).
/// 3. For any remaining archetype whose grid holds at least one valid attack action for its
///    default tags, builds a stand-in tree from `Server.pak`'s `baseai/` sub-trees and the
///    archetype's own grid actions.
///
/// Returns the number of archetypes that received a tree.
///
/// # Errors
///
/// Returns [`DecodeError`] when the node arena is full.
pub fn fill_missing_trees(
    decoder: &mut TreeDecoder,
    archetypes: &mut [Archetype],
    grids: &[Grid],
    actions: &[CageAction],
) -> Result<usize, DecodeError> {
    let mut filled = 0;
    for archetype in archetypes.iter_mut() {
        if archetype.trees.iter().any(|tree| tree.tree.is_some()) {
            continue;
        }
        let Some(first_path) = archetype
            .trees
            .first()
            .map(|tree| decoder.name(tree.path).to_owned())
        else {
            continue;
        };
        if FTUE_SCRIPTED_TREES.contains(&first_path.as_str()) {
            continue;
        }
        if let Some((_, sibling)) = SIBLING_TREES
            .iter()
            .find(|(missing, _)| *missing == first_path)
            && let Some((tree_index, _)) = decoder.tree_by_path(sibling)
        {
            archetype.trees[0].tree = Some(tree_index);
            filled += 1;
            continue;
        }
        let Some(grid_index) = archetype.grid else {
            continue;
        };
        let Some(grid) = grids.get(grid_index as usize) else {
            continue;
        };
        if let Some(tree_index) = build_grid_tree(decoder, archetype, grid, actions, &first_path)? {
            archetype.trees[0].tree = Some(tree_index);
            filled += 1;
        }
    }
    Ok(filled)
}

/// Attack tasks discovered from one archetype's grid for its default tags.
struct GridAttacks {
    melee: Vec<NameId>,
    ranged_or_spell: Vec<NameId>,
    melee_range_m: f32,
}

fn discover_grid_attacks(
    decoder: &TreeDecoder,
    archetype: &Archetype,
    grid: &Grid,
    actions: &[CageAction],
) -> GridAttacks {
    let has_tag = |tag: NameId| archetype.default_tags.contains(&tag);
    let mut melee = Vec::new();
    let mut ranged_or_spell = Vec::new();
    let mut max_reach: f32 = 0.0;

    for &(task_id, action_index) in &grid.tasks {
        let Some(action) = actions.get(action_index as usize) else {
            continue;
        };
        let list_lower = decoder.name(action.list).to_lowercase();
        if list_lower.contains(BASEAI_LIST_MARKER) {
            continue;
        }
        let task_lower = decoder.name(task_id).to_lowercase();
        if NON_ATTACK_TASK_MARKERS
            .iter()
            .any(|marker| task_lower.contains(marker))
        {
            continue;
        }
        let Some(variant) = action.variant(&has_tag) else {
            continue;
        };
        let weapon_variant = action.weapon_variant(&has_tag);
        let mut has_damage = false;
        let mut has_ranged = false;
        let mut has_spell = false;
        let mut homing_reach: f32 = 0.0;
        let mut shape_radius: f32 = 0.0;
        let all_windows = variant
            .windows
            .iter()
            .chain(weapon_variant.iter().flat_map(|wv| wv.windows.iter()));
        for window in all_windows {
            match decoder.name(window.kind) {
                WINDOW_RANGED => has_ranged = true,
                WINDOW_SPELL => has_spell = true,
                WINDOW_DAMAGE => {
                    has_damage = true;
                    for &(key, value) in &window.params {
                        if decoder.name(key) == PARAM_SHAPE_RADIUS
                            && let Ok(radius) = decoder.name(value).parse::<f32>()
                        {
                            shape_radius = shape_radius.max(radius);
                        }
                    }
                }
                WINDOW_HOMING => {
                    let mut move_to: f32 = 0.0;
                    let mut max_move: f32 = 0.0;
                    for &(key, value) in &window.params {
                        let name = decoder.name(key);
                        if name == PARAM_MOVE_TO_DISTANCE
                            && let Ok(parsed) = decoder.name(value).parse::<f32>()
                        {
                            move_to = parsed;
                        } else if name == PARAM_MAX_MOVE_DISTANCE
                            && let Ok(parsed) = decoder.name(value).parse::<f32>()
                        {
                            max_move = parsed;
                        }
                    }
                    homing_reach = homing_reach.max(move_to + max_move);
                }
                _ => {}
            }
        }
        if has_ranged || has_spell {
            ranged_or_spell.push(task_id);
        } else if has_damage {
            melee.push(task_id);
            let reach = homing_reach.max(shape_radius + MELEE_CHASE_STOP_M);
            max_reach = max_reach.max(reach);
        }
    }

    let melee_range_m = if max_reach > 0.0 {
        max_reach.clamp(MELEE_MIN_RANGE_M, MELEE_MAX_RANGE_M)
    } else {
        MELEE_DEFAULT_RANGE_M
    };

    GridAttacks {
        melee,
        ranged_or_spell,
        melee_range_m,
    }
}

fn grid_has_task(decoder: &TreeDecoder, grid: &Grid, task_name: &str) -> Option<NameId> {
    grid.tasks
        .iter()
        .find(|&&(id, _)| decoder.name(id) == task_name)
        .map(|&(id, _)| id)
}

fn archetype_nav_config(
    decoder: &TreeDecoder,
    archetype: &Archetype,
    wanted: &str,
) -> Option<NameId> {
    archetype
        .nav_configs
        .iter()
        .find(|config| decoder.name(config.kind) == wanted)
        .map(|config| config.kind)
}

fn build_grid_tree(
    decoder: &mut TreeDecoder,
    archetype: &Archetype,
    grid: &Grid,
    actions: &[CageAction],
    missing_path: &str,
) -> Result<Option<u32>, DecodeError> {
    let attacks = discover_grid_attacks(decoder, archetype, grid, actions);
    if attacks.melee.is_empty() && attacks.ranged_or_spell.is_empty() {
        return Ok(None);
    }

    let empty = decoder.intern("");
    let move_combat_task = grid_has_task(decoder, grid, TASK_MOVE_COMBAT)
        .or_else(|| grid_has_task(decoder, grid, TASK_MOVE));
    let mobile = move_combat_task.is_some();
    let has_stun = grid_has_task(decoder, grid, TASK_STUN).is_some();
    let has_reset_move = grid_has_task(decoder, grid, TASK_RESET_MOVE).is_some();
    let has_reset_idle = grid_has_task(decoder, grid, TASK_RESET_IDLE).is_some();
    let has_turn = grid_has_task(decoder, grid, TASK_TURN).is_some();
    let has_move = grid_has_task(decoder, grid, TASK_MOVE).is_some();
    let strafe_task = grid_has_task(decoder, grid, TASK_STRAFE);
    let combat_idle_task = grid_has_task(decoder, grid, TASK_COMBAT_IDLE);
    let chase_nav = archetype_nav_config(decoder, archetype, NAV_RUN_TO_TARGET)
        .or_else(|| archetype_nav_config(decoder, archetype, NAV_RUN_CLOSE_QUARTERS))
        .or_else(|| archetype_nav_config(decoder, archetype, NAV_WALK_TO_TARGET))
        .or_else(|| archetype.nav_configs.first().map(|config| config.kind));
    let strafe_nav = archetype_nav_config(decoder, archetype, NAV_STRAFE);

    let mut expanded: u32 = 0;

    // Combat selector children
    let mut combat_children = Vec::new();
    if mobile && let Some(node) = subtree_node(decoder, BASEAI_TARGET_DEATH_LEASH)? {
        combat_children.push(node);
        expanded += 1;
    }
    if has_turn && let Some(turn_sub) = subtree_node(decoder, BASEAI_TURN)? {
        let turn_name = decoder.intern("LimitTurnFrequency");
        let turn_node = decoder.intern_node(Node {
            name: turn_name,
            utility: 0.0,
            interruptable: true,
            kind: NodeKind::FrequencyLimiter {
                child: Some(turn_sub),
                min: Param::literal(Value::Float(TURN_INTERVAL_MIN_S)),
                max: Param::literal(Value::Float(TURN_INTERVAL_MAX_S)),
                stamp_on_success: true,
                stamp_on_failure: false,
            },
        })?;
        combat_children.push(turn_node);
        expanded += 2;
    }

    let has_melee = !attacks.melee.is_empty();
    let has_ranged = !attacks.ranged_or_spell.is_empty();

    if has_ranged {
        let mut ranged_leaves = Vec::with_capacity(attacks.ranged_or_spell.len());
        for task_id in attacks.ranged_or_spell {
            let leaf = decoder.intern_node(Node {
                name: task_id,
                utility: DEFAULT_UTILITY,
                interruptable: false,
                kind: NodeKind::Cage(Box::new(cage_params(task_id, true, empty))),
            })?;
            ranged_leaves.push(leaf);
            expanded += 1;
        }
        let children = decoder.intern_children(ranged_leaves)?;
        let pick_name = decoder.intern("RangedAttackSel");
        let pick = decoder.intern_node(Node {
            name: pick_name,
            utility: 0.0,
            interruptable: false,
            kind: NodeKind::UtilitySelector {
                children,
                method: SelectionMethod::WeightedRandom,
                top_n: WEIGHTED_RANDOM_TOP_N,
                preserve_if_no_change: true,
                always_preserve: false,
            },
        })?;
        let cd_name = decoder.intern("RangedCooldown");
        let cooled = decoder.intern_node(Node {
            name: cd_name,
            utility: 0.0,
            interruptable: false,
            kind: NodeKind::Cooldown {
                child: Some(pick),
                cooldown: CooldownSpec {
                    name: cd_name,
                    min: Param::literal(Value::Float(RANGED_COOLDOWN_MIN_S)),
                    max: Param::literal(Value::Float(RANGED_COOLDOWN_MAX_S)),
                    override_rule: CooldownOverride::Always,
                },
                omit_on_failure: true,
            },
        })?;
        let dist_key = decoder.key(BB_TARGET_DIST);
        let vis_key = decoder.key(BB_TARGET_VISIBLE);
        let mut conds = Vec::new();
        if has_melee {
            conds.push(cond_float(
                dist_key,
                CompareOp::GreaterThanOrEqual,
                RANGED_MIN_RANGE_M,
            ));
        }
        conds.push(cond_float(
            dist_key,
            CompareOp::LessThanOrEqual,
            RANGED_MAX_RANGE_M,
        ));
        conds.push(cond_bool(vis_key, true));
        let gate_name = decoder.intern("RangedWindow");
        let gated = decoder.intern_node(Node {
            name: gate_name,
            utility: 0.0,
            interruptable: false,
            kind: NodeKind::BlackboardCondition {
                child: Some(cooled),
                gates: Box::new(Gates {
                    entry: ConditionGroup {
                        any: false,
                        conditions: conds,
                    },
                    continuation: ConditionGroup::default(),
                    interrupt: ConditionGroup::default(),
                }),
            },
        })?;
        combat_children.push(gated);
        expanded += 3;
    }

    if has_melee {
        let mut melee_leaves = Vec::with_capacity(attacks.melee.len());
        for task_id in attacks.melee {
            let leaf = decoder.intern_node(Node {
                name: task_id,
                utility: DEFAULT_UTILITY,
                interruptable: false,
                kind: NodeKind::Cage(Box::new(cage_params(task_id, true, empty))),
            })?;
            melee_leaves.push(leaf);
            expanded += 1;
        }
        let children = decoder.intern_children(melee_leaves)?;
        let pick_name = decoder.intern("MeleeAttackSel");
        let pick = decoder.intern_node(Node {
            name: pick_name,
            utility: 0.0,
            interruptable: false,
            kind: NodeKind::UtilitySelector {
                children,
                method: SelectionMethod::WeightedRandom,
                top_n: WEIGHTED_RANDOM_TOP_N,
                preserve_if_no_change: true,
                always_preserve: false,
            },
        })?;
        let cd_name = decoder.intern("AttackCooldown");
        let cooled = decoder.intern_node(Node {
            name: cd_name,
            utility: 0.0,
            interruptable: false,
            kind: NodeKind::Cooldown {
                child: Some(pick),
                cooldown: CooldownSpec {
                    name: cd_name,
                    min: Param::literal(Value::Float(MELEE_COOLDOWN_MIN_S)),
                    max: Param::literal(Value::Float(MELEE_COOLDOWN_MAX_S)),
                    override_rule: CooldownOverride::Always,
                },
                omit_on_failure: true,
            },
        })?;
        let dist_key = decoder.key(BB_TARGET_DIST);
        let reach_key = decoder.key(BB_TARGET_DIRECTLY_REACHABLE);
        let gate_name = decoder.intern("MeleeWindow");
        let gated = decoder.intern_node(Node {
            name: gate_name,
            utility: 0.0,
            interruptable: false,
            kind: NodeKind::BlackboardCondition {
                child: Some(cooled),
                gates: Box::new(Gates {
                    entry: ConditionGroup {
                        any: false,
                        conditions: vec![
                            cond_float(dist_key, CompareOp::LessThanOrEqual, attacks.melee_range_m),
                            cond_bool(reach_key, true),
                        ],
                    },
                    continuation: ConditionGroup::default(),
                    interrupt: ConditionGroup::default(),
                }),
            },
        })?;
        combat_children.push(gated);
        expanded += 3;
    }

    if let Some(move_task) = move_combat_task {
        let stop_m = if has_melee {
            MELEE_CHASE_STOP_M
        } else {
            RANGED_CHASE_STOP_M
        };
        let nav_name = decoder.intern("ChaseTarget");
        let nav_node = decoder.intern_node(Node {
            name: nav_name,
            utility: 0.0,
            interruptable: true,
            kind: NodeKind::Navigation(Box::new(default_navigation(move_task, chase_nav, empty))),
        })?;
        let dist_key = decoder.key(BB_TARGET_DIST);
        let chase_gate_name = decoder.intern("ChaseRange");
        let chase_gated = decoder.intern_node(Node {
            name: chase_gate_name,
            utility: 0.0,
            interruptable: true,
            kind: NodeKind::BlackboardCondition {
                child: Some(nav_node),
                gates: Box::new(Gates {
                    entry: ConditionGroup {
                        any: false,
                        conditions: vec![cond_float(dist_key, CompareOp::GreaterThan, stop_m)],
                    },
                    continuation: ConditionGroup {
                        any: false,
                        conditions: vec![cond_float(dist_key, CompareOp::GreaterThan, stop_m)],
                    },
                    interrupt: ConditionGroup::default(),
                }),
            },
        })?;
        combat_children.push(chase_gated);
        expanded += 2;
    }

    if let (Some(strafe_id), Some(strafe_cfg)) = (strafe_task, strafe_nav) {
        let (min_m, max_m) = if has_melee {
            (MELEE_STRAFE_MIN_M, MELEE_STRAFE_MAX_M)
        } else {
            (RANGED_STRAFE_MIN_M, RANGED_STRAFE_MAX_M)
        };
        let strafe_name = decoder.intern("CombatStrafe");
        let strafe_node = decoder.intern_node(Node {
            name: strafe_name,
            utility: 0.0,
            interruptable: true,
            kind: NodeKind::Strafe(Box::new(default_strafe(
                strafe_id,
                Some(strafe_cfg),
                min_m,
                max_m,
                empty,
            ))),
        })?;
        combat_children.push(strafe_node);
        expanded += 1;
    } else if let Some(idle_id) = combat_idle_task {
        let idle_name = decoder.intern("CombatIdle");
        let idle_node = decoder.intern_node(Node {
            name: idle_name,
            utility: 0.0,
            interruptable: true,
            kind: NodeKind::Cage(Box::new(cage_params(idle_id, false, empty))),
        })?;
        combat_children.push(idle_node);
        expanded += 1;
    }

    let combat_list = decoder.intern_children(combat_children)?;
    let combat_sel_name = decoder.intern("CombatRoot");
    let combat_sel = decoder.intern_node(Node {
        name: combat_sel_name,
        utility: 0.0,
        interruptable: true,
        kind: NodeKind::Selector(combat_list),
    })?;
    let has_target_key = decoder.key(BB_HAS_TARGET);
    let has_target_name = decoder.intern("HasTarget");
    let has_target = decoder.intern_node(Node {
        name: has_target_name,
        utility: 0.0,
        interruptable: true,
        kind: NodeKind::BlackboardCondition {
            child: Some(combat_sel),
            gates: Box::new(Gates {
                entry: ConditionGroup {
                    any: false,
                    conditions: vec![cond_bool(has_target_key, true)],
                },
                continuation: ConditionGroup {
                    any: false,
                    conditions: vec![cond_bool(has_target_key, true)],
                },
                interrupt: ConditionGroup::default(),
            }),
        },
    })?;
    expanded += 2;

    // StandardBehavior selector: leashes, combat, ambient
    let mut standard_children = Vec::new();
    if mobile
        && has_reset_move
        && let Some(node) = subtree_node(decoder, BASEAI_DISTANCE_LEASH)?
    {
        standard_children.push(node);
        expanded += 1;
    }
    if mobile
        && has_reset_idle
        && let Some(node) = subtree_node(decoder, BASEAI_UNREACHABLE_LEASH)?
    {
        standard_children.push(node);
        expanded += 1;
    }
    standard_children.push(has_target);
    if mobile {
        if let Some(node) = subtree_node(decoder, BASEAI_DAMAGE_OUTSIDE_AOI)? {
            standard_children.push(node);
            expanded += 1;
        }
        if has_move {
            if let Some(node) = subtree_node(decoder, BASEAI_PATROL)? {
                standard_children.push(node);
                expanded += 1;
            }
            if let Some(node) = subtree_node(decoder, BASEAI_WANDER)? {
                standard_children.push(node);
                expanded += 1;
            }
        }
    }

    let standard_list = decoder.intern_children(standard_children)?;
    let standard_name = decoder.intern("StandardBehavior");
    let standard_sel = decoder.intern_node(Node {
        name: standard_name,
        utility: 0.0,
        interruptable: true,
        kind: NodeKind::Selector(standard_list),
    })?;
    let reaction_key = decoder.key(BB_FULL_BODY_REACTION);
    let non_reaction_name = decoder.intern("NonReactionBehaviors");
    let non_reaction = decoder.intern_node(Node {
        name: non_reaction_name,
        utility: 0.0,
        interruptable: true,
        kind: NodeKind::BlackboardCondition {
            child: Some(standard_sel),
            gates: Box::new(Gates {
                entry: ConditionGroup {
                    any: false,
                    conditions: vec![cond_bool(reaction_key, false)],
                },
                continuation: ConditionGroup {
                    any: false,
                    conditions: vec![cond_bool(reaction_key, false)],
                },
                interrupt: ConditionGroup::default(),
            }),
        },
    })?;
    expanded += 2;

    let mut root_children = Vec::new();
    if has_stun && let Some(stun_sub) = subtree_node(decoder, BASEAI_STUN)? {
        let reaction_name = decoder.intern("ReactionBehaviors");
        let reaction = decoder.intern_node(Node {
            name: reaction_name,
            utility: 0.0,
            interruptable: true,
            kind: NodeKind::BlackboardCondition {
                child: Some(stun_sub),
                gates: Box::new(Gates {
                    entry: ConditionGroup {
                        any: false,
                        conditions: vec![cond_bool(reaction_key, true)],
                    },
                    continuation: ConditionGroup {
                        any: false,
                        conditions: vec![cond_bool(reaction_key, true)],
                    },
                    interrupt: ConditionGroup::default(),
                }),
            },
        })?;
        root_children.push(reaction);
        expanded += 2;
    }
    root_children.push(non_reaction);

    let root_list = decoder.intern_children(root_children)?;
    let root_name = decoder.intern("Root");
    let root = decoder.intern_node(Node {
        name: root_name,
        utility: 0.0,
        interruptable: true,
        kind: NodeKind::Selector(root_list),
    })?;
    expanded += 1;

    let slice_name = decoder.name(archetype.slice);
    let built_path = format!("{missing_path}#grid:{slice_name}");
    let tree_index = decoder.add_built_tree(&built_path, root, expanded)?;
    Ok(Some(tree_index))
}

fn subtree_node(decoder: &mut TreeDecoder, path: &str) -> Result<Option<NodeId>, DecodeError> {
    let Some((_, Some(root))) = decoder.tree_by_path(path) else {
        return Ok(None);
    };
    let name = decoder.intern(path);
    let id = decoder.intern_node(Node {
        name,
        utility: 0.0,
        interruptable: true,
        kind: NodeKind::SubTree {
            path: name,
            root: Some(root),
        },
    })?;
    Ok(Some(id))
}

fn cond_bool(key: nw_ai::value::KeyId, expected: bool) -> Condition {
    Condition {
        left: Param {
            key: Some(key),
            value: Some(Value::Bool(false)),
        },
        op: CompareOp::Equal,
        right: Param::literal(Value::Bool(expected)),
        auto_plan: false,
        allow_dynamic_scaling: false,
    }
}

fn cond_float(key: nw_ai::value::KeyId, op: CompareOp, expected: f32) -> Condition {
    Condition {
        left: Param::of_key(key),
        op,
        right: Param::literal(Value::Float(expected)),
        auto_plan: false,
        allow_dynamic_scaling: false,
    }
}

fn default_action_target(empty: NameId) -> ActionTarget {
    ActionTarget {
        profile: None,
        tag_filter: empty,
        limits: TargetLimits {
            min_targets: Param::literal(Value::Int(UNSET_LIMIT)),
            max_targets: Param::literal(Value::Int(UNSET_LIMIT)),
            top_exclude: Param::literal(Value::Int(UNSET_LIMIT)),
            range_behavior: Param::literal(Value::Int(0)),
        },
    }
}

fn default_nav_ray_cast() -> NavRayCast {
    NavRayCast {
        direction: Param::literal(Value::Float(0.0)),
        distance: Param::literal(Value::Float(0.0)),
        distance_from_wall: Param::literal(Value::Float(0.0)),
        check_at_execution: false,
        allow_dynamic_scaling: true,
    }
}

fn cage_params(task: NameId, wait_for_completion: bool, empty: NameId) -> CageParams {
    CageParams {
        task: Param::literal(Value::Text(task)),
        wait_for_completion,
        fail_timeout: Param::literal(Value::Float(0.0)),
        nav_ray_cast: default_nav_ray_cast(),
        align_position: Param::UNSET,
        align_orientation: Param::UNSET,
        can_kill_deaths_door: false,
        target: default_action_target(empty),
        target_evaluation_ms: Param::literal(Value::Int(TARGET_EVAL_MS)),
    }
}

fn default_motion(nav_config: Option<NameId>) -> Motion {
    Motion {
        nav_config,
        max_speed: Param::UNSET,
        min_speed: Param::UNSET,
        linear_acceleration: Param::UNSET,
        linear_deceleration: Param::UNSET,
        arrival_threshold: Param::UNSET,
        target_repath_distance: Param::UNSET,
        allow_dynamic_scaling: false,
        allowed_stuck_resolutions: Param::UNSET,
        give_up_behavior: Param::UNSET,
        select_position: None,
    }
}

fn default_navigation(task: NameId, nav_config: Option<NameId>, empty: NameId) -> NavigationParams {
    NavigationParams {
        cage: cage_params(task, false, empty),
        motion: default_motion(nav_config),
        nav_key: None,
        turn_rate: Param::UNSET,
        angular_acceleration: Param::UNSET,
        angular_deceleration: Param::UNSET,
        facing_mode: Param::literal(Value::Int(0)),
        face_movement_smooth_time: Param::UNSET,
        agent_radius: Param::UNSET,
        teleport: false,
        allow_partial_paths: true,
        calculate_path_on_exec: true,
        start_action_override: None,
        turn_action_override: None,
        observe_heat: false,
        apply_heat: false,
        max_corridor_width: Param::UNSET,
        nearby_nav_link_removal_threshold: Param::UNSET,
        waypoint_look_ahead_radius: Param::UNSET,
        max_steering_force: Param::UNSET,
        max_avoidance_force: Param::UNSET,
        max_separation_force: Param::UNSET,
        steering_ramp_down_range_min: Param::UNSET,
        steering_ramp_down_range_max: Param::UNSET,
        enable_avoidance: false,
        avoidance_look_ahead_horizon: Param::UNSET,
        avoidance_force: Param::UNSET,
        enable_separation: false,
        separation_circle_force: Param::UNSET,
        separation_circle_radius: Param::UNSET,
        separation_cone_force: Param::UNSET,
        separation_cone_range: Param::UNSET,
        separation_cone_angle: Param::UNSET,
    }
}

fn default_strafe(
    task: NameId,
    nav_config: Option<NameId>,
    ideal_min: f32,
    ideal_max: f32,
    empty: NameId,
) -> StrafeParams {
    StrafeParams {
        cage: cage_params(task, false, empty),
        motion: default_motion(nav_config),
        ideal_distance_min: Param::literal(Value::Float(ideal_min)),
        ideal_distance_max: Param::literal(Value::Float(ideal_max)),
        approach_distance_ratio: Param::literal(Value::Float(STRAFE_APPROACH_RATIO)),
        retreat_distance_ratio: Param::literal(Value::Float(STRAFE_RETREAT_RATIO)),
        lateral_frequency_min: Param::literal(Value::Float(STRAFE_FREQ_MIN_S)),
        lateral_frequency_max: Param::literal(Value::Float(STRAFE_FREQ_MAX_S)),
        lateral_angle_min: Param::literal(Value::Float(STRAFE_ANGLE_MIN_DEG)),
        lateral_angle_max: Param::literal(Value::Float(STRAFE_ANGLE_MAX_DEG)),
        direct_frequency_min: Param::literal(Value::Float(STRAFE_FREQ_MIN_S)),
        direct_frequency_max: Param::literal(Value::Float(STRAFE_FREQ_MAX_S)),
        allow_standing_still: Param::literal(Value::Bool(true)),
        stand_still_frequency_min: Param::literal(Value::Float(STRAFE_FREQ_MIN_S)),
        stand_still_frequency_max: Param::literal(Value::Float(STRAFE_FREQ_MAX_S)),
        stand_still_duration_min: Param::literal(Value::Float(STRAFE_STAND_STILL_MIN_S)),
        stand_still_duration_max: Param::literal(Value::Float(STRAFE_STAND_STILL_MAX_S)),
        reposition_keep_away_distance: Param::literal(Value::Float(STRAFE_KEEP_AWAY_M)),
        facing_convergence_rate: Param::literal(Value::Float(STRAFE_FACING_RATE)),
        direction_change_rate: Param::literal(Value::Float(STRAFE_DIR_RATE_DEG)),
    }
}

#[cfg(test)]
mod unit_tests {
    use nw_ai::data::{
        Archetype, CageAction, FragmentLength, FragmentVariant, Grid, NavConfig, ProcWindow,
        TreeRef,
    };
    use nw_ai::decode::TreeDecoder;
    use nw_ai::node::{Node, NodeKind};

    use super::*;

    fn empty_archetype(
        decoder: &mut TreeDecoder,
        slice: &str,
        tree_path: &str,
        grid: Option<u32>,
        tags: &[&str],
    ) -> Archetype {
        let nav_kind = decoder.intern(NAV_RUN_TO_TARGET);
        let strafe_kind = decoder.intern(NAV_STRAFE);
        Archetype {
            slice: decoder.intern(slice),
            trees: vec![TreeRef {
                path: decoder.intern(tree_path),
                tree: None,
            }],
            config: Vec::new(),
            auto_evaluate: Vec::new(),
            auto_evaluation_s: None,
            ambient_vision: None,
            combat_vision: None,
            group_aggro_distance: None,
            group_aggro_cap: None,
            targeting_profile: None,
            aggro_delay_override_s: None,
            nav_configs: vec![
                NavConfig {
                    kind: nav_kind,
                    max_speed: 6.0,
                    min_speed: 1.0,
                    turn_rate_deg: 360.0,
                    linear_acceleration: 30.0,
                    linear_deceleration: 30.0,
                    arrival_threshold: 0.2,
                    target_repath_distance: 1.0,
                    select_position: None,
                    teleport: false,
                    turn_action: None,
                    start_action: None,
                    start_rule: 0,
                    action_params: Vec::new(),
                },
                NavConfig {
                    kind: strafe_kind,
                    max_speed: 3.0,
                    min_speed: 1.0,
                    turn_rate_deg: 360.0,
                    linear_acceleration: 30.0,
                    linear_deceleration: 30.0,
                    arrival_threshold: 0.2,
                    target_repath_distance: 1.0,
                    select_position: None,
                    teleport: false,
                    turn_action: None,
                    start_action: None,
                    start_rule: 0,
                    action_params: Vec::new(),
                },
            ],
            max_slope_deg: None,
            grid,
            animation_database: None,
            damage_table: None,
            damage_table_id: None,
            weapon_item: None,
            default_tags: tags.iter().map(|tag| decoder.intern(tag)).collect(),
            triggers: Vec::new(),
            initial_effects: Vec::new(),
            grit_uses_stamina: false,
            large_status_effects_fx: false,
        }
    }

    fn make_action(
        decoder: &mut TreeDecoder,
        list: &str,
        name: &str,
        window_kind: &str,
        tags: &[&str],
    ) -> CageAction {
        let kind_id = decoder.intern(window_kind);
        let radius_key = decoder.intern(PARAM_SHAPE_RADIUS);
        let radius_val = decoder.intern("1.2");
        CageAction {
            list: decoder.intern(list),
            action: decoder.intern(name),
            fragment: decoder.intern(name),
            fragment_cases: Vec::new(),
            completion: None,
            tags: Vec::new(),
            blocks: Vec::new(),
            activation: None,
            deactivation: None,
            regen_off: None,
            stamina_reset: None,
            grit_on: None,
            triggers: Vec::new(),
            motion_multiplier: Some(1.0),
            facing: None,
            variants: vec![FragmentVariant {
                tags: tags.iter().map(|tag| decoder.intern(tag)).collect(),
                frag_tags: Vec::new(),
                length: FragmentLength::Seconds(1.0),
                root_motion: None,
                windows: vec![ProcWindow {
                    kind: kind_id,
                    start_s: 0.3,
                    end_s: Some(0.6),
                    params: vec![(radius_key, radius_val)],
                }],
            }],
            weapon_variants: Vec::new(),
        }
    }

    #[test]
    fn fill_missing_trees_skips_ftue_binds_siblings_and_builds_grid_trees() {
        let mut decoder = TreeDecoder::default();
        let fake_name = decoder.intern("Fake");
        let fake_node = decoder
            .intern_node(Node {
                name: fake_name,
                utility: 0.0,
                interruptable: true,
                kind: NodeKind::Fake {
                    indefinitely: false,
                },
            })
            .expect("fake node");
        for path in [
            BASEAI_STUN,
            BASEAI_DISTANCE_LEASH,
            BASEAI_UNREACHABLE_LEASH,
            BASEAI_TARGET_DEATH_LEASH,
            BASEAI_PATROL,
            BASEAI_WANDER,
            BASEAI_DAMAGE_OUTSIDE_AOI,
            BASEAI_TURN,
            "sharedassets/ai/behaviortrees/rat/rat_plaguerat_base.behaviortree",
        ] {
            decoder
                .add_built_tree(path, fake_node, 1)
                .expect("base tree");
        }

        let bite = make_action(
            &mut decoder,
            "spider/spider_attack.actionlist",
            "Attack_Bite",
            WINDOW_DAMAGE,
            &["Spider"],
        );
        let spit = make_action(
            &mut decoder,
            "spider/spider_attack.actionlist",
            "Attack_WebSpit",
            WINDOW_RANGED,
            &["Spider"],
        );
        let other_tag = make_action(
            &mut decoder,
            "spider/spider_attack.actionlist",
            "Attack_OtherOnly",
            WINDOW_DAMAGE,
            &["OtherTag"],
        );
        let move_act = make_action(
            &mut decoder,
            "baseai/baseai_nav.actionlist",
            "Move_Combat",
            WINDOW_HOMING,
            &[],
        );
        let actions = vec![bite, spit, other_tag, move_act];

        let mut tasks = vec![
            (decoder.intern("Attack_Bite_CAGE_Task"), 0),
            (decoder.intern("Attack_WebSpit_CAGE_Task"), 1),
            (decoder.intern("Attack_OtherOnly_CAGE_Task"), 2),
            (decoder.intern(TASK_MOVE_COMBAT), 3),
            (decoder.intern(TASK_MOVE), 3),
            (decoder.intern(TASK_STRAFE), 3),
            (decoder.intern(TASK_TURN), 3),
            (decoder.intern(TASK_STUN), 3),
            (decoder.intern(TASK_RESET_MOVE), 3),
            (decoder.intern(TASK_RESET_IDLE), 3),
        ];
        tasks.sort_by_key(|(id, _)| *id);
        let grids = vec![Grid {
            path: decoder.intern("spider/spider.grid"),
            animation_database: None,
            tasks,
            actions: Vec::new(),
            aliases: Vec::new(),
        }];

        let mut archetypes = vec![
            empty_archetype(
                &mut decoder,
                "slices/characters/ftue_corrupted/corrupted_sailor_zone3_club.dynamicslice",
                FTUE_SCRIPTED_TREES[0],
                Some(0),
                &["Spider"],
            ),
            empty_archetype(
                &mut decoder,
                "slices/characters/wildlife_rat_plaguerat.dynamicslice",
                "sharedassets/ai/behaviortrees/rat/rat_plaguerat_base_overrides.behaviortree",
                Some(0),
                &["Rat_Plague"],
            ),
            empty_archetype(
                &mut decoder,
                "slices/characters/spider_melee.dynamicslice",
                "sharedassets/ai/behaviortrees/spider/spider_melee_base_overrides.behaviortree",
                Some(0),
                &["Spider"],
            ),
        ];

        let filled = fill_missing_trees(&mut decoder, &mut archetypes, &grids, &actions)
            .expect("fill missing trees");
        assert_eq!(filled, 2);
        assert!(
            archetypes[0].trees[0].tree.is_none(),
            "FTUE tutorial tree must stay unbound"
        );
        assert_eq!(
            archetypes[1].trees[0].tree,
            decoder
                .tree_by_path("sharedassets/ai/behaviortrees/rat/rat_plaguerat_base.behaviortree")
                .map(|(idx, _)| idx),
            "plague rat override binds to rat_plaguerat_base"
        );
        assert!(
            archetypes[2].trees[0].tree.is_some(),
            "spider receives a grid-built stand-in tree"
        );
    }
}

#[cfg(all(test, feature = "local-data"))]
mod tests {
    use std::path::Path;

    use nw_ai::decode::server_pak_entries;
    use nw_oodle::KrakenDecompressor;

    use super::*;
    use crate::ai_data::DEFAULT_SERVER_PAK;

    fn entries() -> Vec<(String, Vec<u8>)> {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        server_pak_entries(&repo.join(DEFAULT_SERVER_PAK), &KrakenDecompressor).expect("Server.pak")
    }

    #[test]
    fn janszoon_s_tree_is_the_great_axeman_s_with_the_officer_s_toss_and_slam() {
        const TOSS_TASK: &str = "Officer_AnchorToss_CAGE_Task";
        let entries = entries();
        let trees = stand_in_trees(&entries).expect("stand-in trees");
        let [(path, root)] = trees.as_slice() else {
            panic!("one stand-in tree");
        };
        assert_eq!(*path, JANSZOON_TREE);
        let combat = find(root, &|element| {
            node_name(element) == Some(COMBAT_SELECTOR)
                && field(element, CHILDREN).is_some_and(|list| {
                    list.children
                        .first()
                        .and_then(|pointer| pointer.children.first())
                        .and_then(node_name)
                        == Some(TOSS_BRANCH)
                })
        })
        .expect("the toss first in the combat selector");
        let branch = &field(combat, CHILDREN).expect("children").children[0];
        assert!(holds_text(branch, TOSS_TASK));
        assert!(!holds_text(branch, TIER_KEY));
        let selector = find(branch, &|element| node_name(element) == Some(TOSS_SELECTOR))
            .expect("toss selector");
        assert_eq!(
            field(selector, CHILDREN).expect("children").children.len(),
            1
        );
        let close = find(root, &|element| {
            node_name(element) == Some(CLOSE_SELECTOR)
                && field(element, CHILDREN).is_some_and(|list| {
                    list.children
                        .first()
                        .and_then(|pointer| pointer.children.first())
                        .and_then(node_name)
                        == Some(SLAM_RANGE)
                })
        })
        .expect("the slam's range first in the close range selector");
        let slam = &field(close, CHILDREN).expect("children").children[0];
        let in_range = slam
            .children
            .first()
            .and_then(|range| field(range, CHILD))
            .and_then(|pointer| pointer.children.first())
            .and_then(node_name);
        assert_eq!(in_range, Some(SLAM_BRANCH), "only the slam in its range");
        assert!(holds_text(slam, SLAM_TASK));
        assert!(!holds_text(slam, "Officer_OceanShield_CAGE_Task"));
        assert!(!holds_text(slam, "Officer_OceanShield_Easy_CAGE_Task"));
        let mut decoder = nw_ai::decode::TreeDecoder::default();
        decoder
            .add_tree(path, root)
            .expect("the built tree decodes");
    }
}
