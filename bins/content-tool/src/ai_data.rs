// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ai-data`: builds the AI data file (`mobs/ai-data.bin`, read by `nw_ai::data::AiData`)
//! from Server.pak and the game paks (behavior-tree study 2026-09-18, sections 2.4, 3 and 5.1).
//!
//! - Every Server.pak `.behaviortree` goes into one shared node arena (`nw_ai::decode`).
//! - Every creature slice of the creature animation table gives an archetype: its
//!   `BehaviorTreeComponent.m_behaviorTreeAssets`, `m_configParams`, auto-evaluated keys,
//!   vision and aggro values, targeting profile, navigation configs (`m_navConfigs`), body slope,
//!   CAGE grid, animation database, damage table (and its datasheet's unique id) and main-hand
//!   item (field names Observed: each field CRC is the
//!   lower-case CRC32 of the name, study `fieldnames-all.tsv`; `m_actionGridNew` is the
//!   `0x0d5832e9` field of `creature_animations`).
//! - Every grid action list `Action` whose `IfHTNCAGEAction` or `IfBehaviorTreeTask` names a task
//!   becomes a CAGE action: fragment, completion offsets, `SetMannequinTag` activities, motion
//!   multiplier, and the fragment's entries in the creature's animation database with their
//!   length and procedural clips (`CAGE-Damage`, `Homing`, ...).
//! - A fragment's length comes from its base animation layer: each clip plays its `.caf` length
//!   (`MotionParams` chunk: frames x ticks per frame x seconds per tick) from its blend
//!   `StartTime`, divided by its `speed`; a blend space plays the length its example clips share.
//!   A length that cannot be read is Unknown and listed in the report; nothing is filled in.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use nw_ai::data::{
    AiData, ArcTrajectory, Archetype, BlockActivity, BlockEffect, CageAction, CageCondition,
    Completion, Creature, DamageTableRows, DelayedEvent, EntityTrigger, FacingLength, FacingMode,
    FragmentCase, FragmentLength, FragmentVariant, Grid, NavConfig, ProcWindow, ProjectileEntity,
    ProjectileImpact, ProjectileSlice, RangedAttack, RangedAttackStream, SliceTriggerRef,
    TagActivity, TagEffect, TreeRef, Vision,
};
use nw_ai::decode::{DecodeError, ServerPakError, TreeDecoder, decode_entries, server_pak_entries};
use nw_ai::node::NodeId;
use nw_ai::value::{NameId, Param, Value};
use nw_content::creature_animations::CreatureRow;
use nw_content::player_attacks::AttackMotion;
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, slice_assets_in};
use nw_formats::azcs::{decode_azcs, is_azcs};
use nw_formats::caf::Caf;
use nw_formats::datasheet::Datasheet;
use nw_formats::mannequin::{
    ActionList, AnimDatabase, BlockActivity as NwBlockActivity, BlockChange, CageGrid, Condition,
    EntityTrigger as NwEntityTrigger, Fragment, FragmentCase as NwFragmentCase, GridList,
    MannequinError, Node as XmlNode, StaminaActivities, TagCase, TagChange, parse_tree,
};
use nw_formats::objectstream::{Element, lowercase_crc32, parse_object_stream};
use nw_formats::pak::{PakArchive, PakError, find_paks};
use nw_formats::skeleton::Skeleton;
use nw_oodle::KrakenDecompressor;
use nw_world::level_script::{
    AssetRef, DELAYED_EVENT_TYPE, NETWORK_POSITION_TYPE, asset as level_asset,
};
use nw_world::slice_triggers::slice_triggers;
use nw_world::stations::OWNERSHIP;

use crate::creature_animations::{read_databases, read_entries, text_of, weapon_item};
use crate::error::ToolError;
use crate::level_doors::replica_index;
use crate::melee_shapes::root_path_on;
use crate::player_attacks::{
    RANGED_ATTACK_DATABASE, RANGED_CLIP, ranged_attack_entries, ranged_attacks, root_motion,
    slice_flight,
};
use crate::source::GameSource;
use crate::stand_in_trees::{StandInError, stand_in_trees};

/// Where the user keeps the retrieved Server.pak (server-pak note).
pub const DEFAULT_SERVER_PAK: &str = "local-data/server-pak/Server.pak";
/// The environment variable that names the Server.pak file of `content-tool`.
pub const SERVER_PAK_ENV: &str = "NW_SERVER_PAK";
/// The `CAGE-RangedAttack` parameter that names the projectile slice.
const SLICE_OVERRIDE_PARAM: &str = "SliceOverride";
/// The folder of the dynamic slices in the asset catalog.
const SLICES_FOLDER: &str = "slices/";
/// The column of a damage table's row ids.
const DAMAGE_ID_COLUMN: &str = "DamageID";
/// `{5637158A-9AE1-41A7-A5C0-D07A76116EAF}`: the projectile component (Observed: its
/// `m_baseSpeed`, `m_accel` and `m_spawnOnHitAsset`; its replica index is the component of the
/// class 16 member, 2 in `corrupted_miner_bomb.dynamicslice`, 1 in
/// `corrupted_firstmate_anchor_projectile.dynamicslice`, as newchar-retail-2026-09-18 sent them
/// at 802.637 s and 1247.540 s).
const PROJECTILE_COMPONENT_TYPE: [u8; 16] = [
    0x56, 0x37, 0x15, 0x8a, 0x9a, 0xe1, 0x41, 0xa7, 0xa5, 0xc0, 0xd0, 0x7a, 0x76, 0x11, 0x6e, 0xaf,
];
/// The projectile component's slice to spawn where it hits.
const SPAWN_ON_HIT: &str = "m_spawnOnHitAsset";
/// The projectile component's stay after a hit.
const STUCK_LIFETIME: &str = "m_stuckLifetime";
/// A damage component's hit (Observed in the grenadier bomb's explosion and the anchor's impact:
/// `m_damageData` > `m_damageDataTable`, `m_damageDataEntry`; the radius of its `m_shape`).
const DAMAGE_DATA: &str = "m_damageData";
const DAMAGE_DATA_TABLE: &str = "m_damageDataTable";
const DAMAGE_DATA_ENTRY: &str = "m_damageDataEntry";
const DAMAGE_SHAPE: &str = "m_shape";
const SHAPE_RADIUS: &str = "m_radius";
/// A delayed event component's delay.
const DELAY_SECONDS: &str = "m_timeDelaySeconds";
/// A ranged attack's arc settings (field names from the client's reflection strings).
const ARC_SETTINGS: &str = "m_arcTrajectorySettings";
const ARC_ENABLED: &str = "m_useArcTrajectories";
const ARC_LOB_MIN: &str = "m_lobHeightMin";
const ARC_LOB_MAX: &str = "m_lobHeightMax";
const ARC_GRAVITY_MIN: &str = "m_gravityOverrideMin";
const ARC_GRAVITY_MAX: &str = "m_gravityOverrideMax";
const ARC_DISTANCE_MIN: &str = "m_distanceLerpMin";
const ARC_DISTANCE_MAX: &str = "m_distanceLerpMax";

/// Slice fields (Observed names).
const TREE_ASSETS: &str = "m_behaviorTreeAssets";
const ASSET_REF: &str = "m_assetRef";
const CONFIG_PARAMS: &str = "m_configParams";
const CONFIG_NAME: &str = "m_name";
const CONFIG_VALUE: &str = "m_value";
const AUTO_EVALUATE: &str = "m_autoEvaluateBlackboardVariables";
const AUTO_EVALUATION_FREQUENCY: &str = "m_autoEvaluationFrequency";
const AMBIENT_VISION: &str = "m_ambientVisionProfile";
const COMBAT_VISION: &str = "m_combatVisionProfile";
const VISION_ANGLE: &str = "m_visionAngle";
const VISION_DISTANCE: &str = "m_visionDistance";
const GROUP_AGGRO_DISTANCE: &str = "m_groupAggroDistance";
const GROUP_AGGRO_CAP: &str = "m_groupAggroShareCap";
const TARGETING_PROFILE: &str = "m_targetingProfile";
const OVERRIDE_AGGRO_DELAY: &str = "m_overrideAggroDelay";
const AGGRO_DELAY: &str = "m_aggroDelayOverrideSec";
const NAV_CONFIGS: &str = "m_navConfigs";
const NAV_CONFIG_TYPE: &str = "m_navConfigType";
const NAV_MAX_SPEED: &str = "m_maxSpeed";
const NAV_MIN_SPEED: &str = "m_minSpeed";
const NAV_TURN_RATE: &str = "m_turnRateDegs";
const NAV_ACCELERATION: &str = "m_linearAcceleration";
const NAV_DECELERATION: &str = "m_linearDeceleration";
const NAV_ARRIVAL: &str = "m_arrivalThreshold";
const NAV_REPATH: &str = "m_targetRepathDistance";
const NAV_SELECT_POSITION: &str = "m_selectPositionParams";
const NAV_ACTION_PARAMS: &str = "m_navActionParams";
const NAV_STRAFE_PARAMS: &str = "m_strafeActionParams";
const NAV_TELEPORT: &str = "m_teleport";
const NAV_TURN_ACTION: &str = "m_turnActionName";
const NAV_START_ACTION: &str = "m_startActionName";
const NAV_START_RULE: &str = "m_navStartRule";
const MAX_SLOPE: &str = "m_maxSlope";
/// The `StatusEffectsComponent` field of the effects the creature spawns with.
const INITIAL_STATUS_EFFECTS: &str = "m_initialStatusEffects";
/// The `GritComponent` field (client reflection `FUN_1468e90f0`) that makes the grit value the
/// stamina.
const GRIT_USES_STAMINA: &str = "m_useStaminaAsValue";
/// The `FxScriptComponent` field of the FX lists it names, and the name of each entry (client
/// reflection strings beside `FxScriptComponent`; Observed in the bear slice:
/// `m_scriptDataContainer` > element > `m_scriptName`).
const FX_SCRIPT_CONTAINER: &str = "m_scriptDataContainer";
const FX_SCRIPT_NAME: &str = "m_scriptName";
/// The FX list of the large bodies ([`nw_ai::data::Archetype::large_status_effects_fx`]).
const LARGE_STATUS_EFFECTS_FX: &str = "fxscriptlist_npc_statuseffects_large";
const ACTION_GRID: &str = "m_actionGridNew";
const ANIMATION_DATABASE: &str = "m_animationDatabase";
const DAMAGE_TABLE: &str = "m_damageTable";
const ASSET_PATH: &str = "AssetPath";
const BASE_CLASS: &str = "BaseClass1";
const DAMAGE_TABLE_ASSET: &str = "Asset";
/// The file endings of a damage table in the slice and in the paks.
const DAMAGE_TABLE_SUFFIX: &str = ".xml";
const DATASHEET_SUFFIX: &str = ".datasheet";

/// Grid and action list XML (Observed in `predator/cage/boar/boar.grid`, `boar_combat.actionlist`).
const ACTION_ELEMENT: &str = "Action";
const NAME_ATTRIBUTE: &str = "Name";
const FRAGMENT_NAME_ATTRIBUTE: &str = "FragmentName";
const ACTIVATION_ELEMENT: &str = "ActivationCondition";
const TASK_TESTS: [&str; 2] = ["IfHTNCAGEAction", "IfBehaviorTreeTask"];
const TASK_ATTRIBUTE: &str = "ActionName";
const COMPLETION_ACTIVITIES: [&str; 2] = ["HTNPlanDoneActivity", "BehaviorTreeTask"];
const DEFAULT_ELEMENT: &str = "Default";
const CONDITIONAL_ELEMENT: &str = "Conditional";
const COMPLETION_OFFSET: &str = "CompletionFragmentOffset";
const MOTION_ACTIVITY: &str = "AnimationDrivenMotion";
const MOTION_MULTIPLIER: &str = "MotionMultiplier";
/// The facing activity (Observed in `baseai_turninplace.actionlist`).
const FACING_ACTIVITY: &str = "SetDesiredFacingBlendParam";
const FACING_MODE: &str = "FacingMode";

/// Animation files by extension.
const CAF_EXTENSION: &str = ".caf";
/// Skeleton files.
const SKELETON_EXTENSION: &str = ".chr";
const BLEND_SPACE_EXTENSION: &str = ".bspace";
const COMBINED_BLEND_SPACE_EXTENSION: &str = ".comb";
const CLIP_EXTENSIONS: [&str; 3] = [
    CAF_EXTENSION,
    BLEND_SPACE_EXTENSION,
    COMBINED_BLEND_SPACE_EXTENSION,
];
/// Blend space XML (Observed in `boar_combat_turn_blend_l.bspace` and `_comb.comb`).
const EXAMPLE_ELEMENT: &str = "Example";
const BLEND_SPACE_ELEMENT: &str = "BlendSpace";
const CLIP_NAME_ATTRIBUTE: &str = "AName";
/// The dimensions of a blend space and the place of each example in them (Observed in
/// `mc_broken_combat_turn_blend_l.bspace`: `<Dimensions><Param Name="DesiredFacing" Min="0"
/// Max="3.1400001"/>`, `<Example SetPara0="1.575" AName="mc_broken_combat_idleturn90_l"/>`; the
/// `.comb` files write the dimension's attribute `name`).
const DIMENSIONS_ELEMENT: &str = "Dimensions";
const PARAM_ELEMENT: &str = "Param";
const PARAM_NAME_ATTRIBUTE: &str = "Name";
const EXAMPLE_PARAM_ATTRIBUTE: &str = "SetPara0";
/// The blend parameter of a turn: the signed angle the mob turns by, radians
/// (`SetDesiredFacingBlendParam` sets it).
const FACING_PARAM: &str = "DesiredFacing";
/// Two clip lengths closer than this are the same (one 30 fps frame is 33 ms).
const SAME_LENGTH_S: f64 = 1e-4;

/// CAF chunk file (Observed: `CrCh` magic; chunk count and table offset at bytes 8 and 12; a
/// 16-byte table entry of type `u16`, version `u16`, id, size and offset `u32`; the
/// `MotionParams` chunk `0x3002` holds flags, compression, ticks per frame, seconds per tick,
/// start and end frame; boar_gore: 46 frames at 1/30 s, read by the audit script
/// `cafdur.py`).
const CAF_MAGIC: &[u8; 4] = b"CrCh";
const CAF_CHUNK_COUNT_AT: usize = 8;
const CAF_CHUNK_TABLE_AT: usize = 12;
const CAF_CHUNK_ENTRY_LEN: usize = 16;
const CAF_CHUNK_TYPE_LEN: usize = 2;
const CAF_CHUNK_OFFSET_AT: usize = 12;
const CAF_MOTION_PARAMS: u16 = 0x3002;
const MOTION_TICKS_PER_FRAME_AT: usize = 8;
const MOTION_SECONDS_PER_TICK_AT: usize = 12;
const MOTION_START_AT: usize = 16;
const MOTION_END_AT: usize = 20;

/// A failure of the AI data build.
#[derive(Debug, thiserror::Error)]
pub enum AiDataError {
    /// Server.pak cannot be read.
    #[error(transparent)]
    ServerPak(#[from] ServerPakError),
    /// A pak of the install cannot be listed.
    #[error(transparent)]
    Pak(#[from] PakError),
    /// A slice value does not decode.
    #[error("{path}: {source}")]
    Decode {
        /// The slice.
        path: String,
        /// Why.
        source: DecodeError,
    },
    /// A stand-in tree cannot be built.
    #[error(transparent)]
    StandIn(#[from] StandInError),
    /// The creature animation table has a bad asset id.
    #[error("creature animation table: bad asset {0}")]
    Asset(String),
}

/// What the build did, and every value it could not find.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiDataReport {
    /// Server.pak trees.
    pub trees: usize,
    /// Shared nodes.
    pub nodes: usize,
    /// Nodes with every sub-tree copy counted.
    pub expanded_nodes: u64,
    /// Creature slice assets.
    pub creatures: usize,
    /// Archetypes (distinct slices).
    pub archetypes: usize,
    /// Grid and animation database pairs.
    pub grids: usize,
    /// Distinct CAGE actions.
    pub cage_actions: usize,
    /// Archetypes with a Server.pak tree, with a grid, and with an animation database.
    pub with_tree: usize,
    /// Archetypes with a grid.
    pub with_grid: usize,
    /// Archetypes with an animation database.
    pub with_animation_database: usize,
    /// Fragment entries over all CAGE actions, and those with a known length.
    pub variants: usize,
    /// Fragment entries with a length in seconds (one length, or one per turn).
    pub timed_variants: usize,
    /// Trees a slice names that Server.pak does not have, with the slices that name them.
    pub missing_trees: BTreeMap<String, BTreeSet<String>>,
    /// CAGE tasks a slice's tree runs that its grid has no action for, by slice.
    pub missing_tasks: BTreeMap<String, BTreeSet<String>>,
    /// Clips whose length is Unknown, with the reason.
    pub unknown_clips: BTreeMap<String, String>,
    /// Fragment entries whose length is Unknown, by animation database.
    pub unknown_fragments: BTreeMap<String, usize>,
    /// Attacks of the ranged attack database.
    pub ranged_attacks: usize,
    /// Projectile slices the ranged clips name, with a flight.
    pub projectile_slices: usize,
    /// Projectile slices the ranged clips name, with a replicated entity.
    pub projectile_entities: usize,
    /// Files that could not be used, with the reason.
    pub skipped: BTreeMap<String, String>,
}

/// The AI of one slice before the arena exists.
#[derive(Debug, Clone, Default)]
struct SliceAi {
    trees: Vec<String>,
    config: Vec<(String, Value)>,
    auto_evaluate: Vec<String>,
    auto_evaluation_s: Option<f32>,
    ambient_vision: Option<Vision>,
    combat_vision: Option<Vision>,
    group_aggro_distance: Option<f32>,
    group_aggro_cap: Option<u32>,
    targeting_profile: Option<String>,
    aggro_delay_override_s: Option<f32>,
    nav_configs: Vec<NavConfig>,
    max_slope_deg: Option<f32>,
    grid: Option<String>,
    animation_database: Option<String>,
    damage_table: Option<String>,
    weapon: Option<String>,
    /// The slice's `TriggerEntityComponent`s: name, child (main `Id` XOR owner `Id`), component.
    triggers: Vec<(String, u64, u64)>,
    /// `m_initialStatusEffects`, in stream order.
    initial_effects: Vec<String>,
    /// `GritComponent.m_useStaminaAsValue`.
    grit_uses_stamina: bool,
    /// An `FxScriptComponent` names [`LARGE_STATUS_EFFECTS_FX`].
    large_status_effects_fx: bool,
}

fn crc(name: &str) -> u32 {
    lowercase_crc32(name)
}

/// The first text value at or below `element`, depth first.
fn text_leaf(element: &Element) -> Option<String> {
    if !element.value.is_empty() {
        return element.as_string().ok().map(str::to_owned);
    }
    element.children.iter().find_map(text_leaf)
}

/// The first element named `name` below `element`, depth first.
fn first<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    every(element, name).next()
}

/// Every element named `name` below `element`, depth first.
fn every<'a>(element: &'a Element, name: &str) -> impl Iterator<Item = &'a Element> + use<'a> {
    let wanted = crc(name);
    let mut stack: Vec<&Element> = element.children.iter().rev().collect();
    std::iter::from_fn(move || {
        while let Some(next) = stack.pop() {
            stack.extend(next.children.iter().rev());
            if next.name_crc == Some(wanted) {
                return Some(next);
            }
        }
        None
    })
}

/// The field `name` of a class element: a direct child, or one in its `BaseClass1` chain
/// (Observed: a config param's `m_name` sits in `BaseClass1`, its `m_value` beside it).
fn field<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    child(element, name).or_else(|| field(child(element, BASE_CLASS)?, name))
}

/// The direct child named `name`.
fn child<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    element.child(crc(name))
}

fn decode_err(path: &str) -> impl Fn(DecodeError) -> AiDataError + '_ {
    move |source| AiDataError::Decode {
        path: path.to_owned(),
        source,
    }
}

fn float_value(value: Value) -> Option<f32> {
    match value {
        Value::Float(value) => Some(value),
        // Small whole numbers only; a slice stores counts and degrees this way.
        Value::Int(value) => Some(value as f32),
        Value::Bool(_) | Value::Text(_) | Value::Vec3(_) => None,
    }
}

fn text_value(decoder: &TreeDecoder, value: Value) -> Option<String> {
    match value {
        Value::Text(text) => Some(decoder.name(text).to_owned()),
        _ => None,
    }
}

impl SliceAi {
    fn read(root: &Element, decoder: &mut TreeDecoder, path: &str) -> Result<Self, AiDataError> {
        let err = decode_err(path);
        let mut slice = Self::default();
        let value = |decoder: &mut TreeDecoder, element: &Element, field: &'static str| {
            decoder.value_of(field, element).map_err(&err)
        };
        if let Some(assets) = first(root, TREE_ASSETS) {
            for entry in &assets.children {
                if let Some(reference) = child(entry, ASSET_REF)
                    && let Some(asset) = decoder.asset_of(ASSET_REF, reference).map_err(&err)?
                    && let Some(hint) = asset.hint
                {
                    slice.trees.push(decoder.name(hint).to_owned());
                }
            }
        }
        if let Some(params) = first(root, CONFIG_PARAMS) {
            let mut stack = vec![params];
            while let Some(next) = stack.pop() {
                match (field(next, CONFIG_NAME), child(next, CONFIG_VALUE)) {
                    (Some(name), Some(stored)) => {
                        let name = value(decoder, name, CONFIG_NAME)?;
                        let name = text_value(decoder, name).ok_or_else(|| {
                            err(DecodeError::MissingField {
                                class: nw_ai::classes::Class::String,
                                field: CONFIG_NAME,
                            })
                        })?;
                        slice
                            .config
                            .push((name, value(decoder, stored, CONFIG_VALUE)?));
                    }
                    _ => stack.extend(next.children.iter().rev()),
                }
            }
        }
        if let Some(keys) = first(root, AUTO_EVALUATE) {
            for key in &keys.children {
                let key = value(decoder, key, AUTO_EVALUATE)?;
                slice.auto_evaluate.extend(text_value(decoder, key));
            }
        }
        let float = |decoder: &mut TreeDecoder,
                     within: &Element,
                     field: &'static str|
         -> Result<Option<f32>, AiDataError> {
            match first(within, field) {
                Some(element) => Ok(float_value(value(decoder, element, field)?)),
                None => Ok(None),
            }
        };
        slice.auto_evaluation_s = float(decoder, root, AUTO_EVALUATION_FREQUENCY)?;
        for (field, vision) in [
            (AMBIENT_VISION, &mut slice.ambient_vision),
            (COMBAT_VISION, &mut slice.combat_vision),
        ] {
            if let Some(profile) = first(root, field) {
                *vision = match (
                    float(decoder, profile, VISION_ANGLE)?,
                    float(decoder, profile, VISION_DISTANCE)?,
                ) {
                    (Some(angle_deg), Some(distance_m)) => Some(Vision {
                        angle_deg,
                        distance_m,
                    }),
                    _ => None,
                };
            }
        }
        slice.group_aggro_distance = float(decoder, root, GROUP_AGGRO_DISTANCE)?;
        slice.group_aggro_cap = float(decoder, root, GROUP_AGGRO_CAP)?.map(|cap| cap as u32);
        slice.max_slope_deg = float(decoder, root, MAX_SLOPE)?;
        if let Some(effects) = first(root, INITIAL_STATUS_EFFECTS) {
            // Each entry holds the effect id as its one text value (Observed in the bear slice).
            slice.initial_effects = effects.children.iter().filter_map(text_leaf).collect();
        }
        slice.grit_uses_stamina = first(root, GRIT_USES_STAMINA)
            .and_then(|element| element.as_bool().ok())
            .unwrap_or(false);
        slice.large_status_effects_fx = every(root, FX_SCRIPT_CONTAINER)
            .flat_map(|container| &container.children)
            .filter_map(|entry| child(entry, FX_SCRIPT_NAME))
            .any(|name| {
                name.as_string()
                    .is_ok_and(|name| name.eq_ignore_ascii_case(LARGE_STATUS_EFFECTS_FX))
            });
        let overridden = match first(root, OVERRIDE_AGGRO_DELAY) {
            Some(element) => value(decoder, element, OVERRIDE_AGGRO_DELAY)?.as_bool(),
            None => None,
        };
        if overridden == Some(true) {
            slice.aggro_delay_override_s = float(decoder, root, AGGRO_DELAY)?;
        }
        let asset_path = |decoder: &mut TreeDecoder,
                          field: &'static str|
         -> Result<Option<String>, AiDataError> {
            let Some(element) = first(root, field) else {
                return Ok(None);
            };
            Ok(decoder
                .asset_of(field, element)
                .map_err(&err)?
                .and_then(|asset| asset.hint)
                .map(|hint| decoder.name(hint).to_owned()))
        };
        slice.targeting_profile = asset_path(decoder, TARGETING_PROFILE)?;
        slice.grid = asset_path(decoder, ACTION_GRID)?;
        let text_at = |decoder: &mut TreeDecoder,
                       element: Option<&Element>|
         -> Result<Option<String>, AiDataError> {
            // `AssetPath` sits in the field's `BaseClass1` (Observed in the boar slice).
            match element.and_then(|element| first(element, ASSET_PATH)) {
                Some(path) => {
                    let stored = value(decoder, path, ASSET_PATH)?;
                    Ok(text_value(decoder, stored).map(|path| path.to_lowercase()))
                }
                None => Ok(None),
            }
        };
        slice.animation_database = text_at(decoder, first(root, ANIMATION_DATABASE))?;
        slice.damage_table = text_at(
            decoder,
            first(root, DAMAGE_TABLE).and_then(|table| child(table, DAMAGE_TABLE_ASSET)),
        )?;
        if let Some(configs) = first(root, NAV_CONFIGS) {
            for entry in &configs.children {
                slice.nav_configs.push(nav_config(entry, decoder, path)?);
            }
        }
        Ok(slice)
    }
}

fn nav_config(
    entry: &Element,
    decoder: &mut TreeDecoder,
    path: &str,
) -> Result<NavConfig, AiDataError> {
    let err = decode_err(path);
    let missing = |field: &'static str| {
        err(DecodeError::MissingField {
            class: nw_ai::classes::Class::Float,
            field,
        })
    };
    let value = |decoder: &mut TreeDecoder, field: &'static str| -> Result<Value, AiDataError> {
        let element = child(entry, field).ok_or_else(|| missing(field))?;
        decoder.value_of(field, element).map_err(&err)
    };
    let number = |decoder: &mut TreeDecoder, field: &'static str| -> Result<f32, AiDataError> {
        float_value(value(decoder, field)?).ok_or_else(|| missing(field))
    };
    let stored = value(decoder, NAV_CONFIG_TYPE)?;
    let kind_text = text_value(decoder, stored).ok_or_else(|| missing(NAV_CONFIG_TYPE))?;
    let kind = decoder.intern(&kind_text);
    let max_speed = number(decoder, NAV_MAX_SPEED)?;
    let min_speed = number(decoder, NAV_MIN_SPEED)?;
    let turn_rate_deg = number(decoder, NAV_TURN_RATE)?;
    let linear_acceleration = number(decoder, NAV_ACCELERATION)?;
    let linear_deceleration = number(decoder, NAV_DECELERATION)?;
    let arrival_threshold = number(decoder, NAV_ARRIVAL)?;
    let target_repath_distance = number(decoder, NAV_REPATH)?;
    let select_position = match child(entry, NAV_SELECT_POSITION) {
        Some(params) => Some(decoder.select_position_of(params).map_err(&err)?),
        None => None,
    };
    let teleport = match child(entry, NAV_TELEPORT) {
        Some(element) => decoder
            .value_of(NAV_TELEPORT, element)
            .map_err(&err)?
            .as_bool()
            .unwrap_or(false),
        None => false,
    };
    let task =
        |decoder: &mut TreeDecoder, field: &'static str| -> Result<Option<NameId>, AiDataError> {
            let Some(element) = child(entry, field) else {
                return Ok(None);
            };
            let stored = decoder.value_of(field, element).map_err(&err)?;
            let text = text_value(decoder, stored);
            Ok(text
                .filter(|text| !text.is_empty())
                .map(|text| decoder.intern(&text)))
        };
    let turn_action = task(decoder, NAV_TURN_ACTION)?;
    let start_action = task(decoder, NAV_START_ACTION)?;
    let start_rule = match child(entry, NAV_START_RULE) {
        Some(element) => match decoder.value_of(NAV_START_RULE, element).map_err(&err)? {
            Value::Int(rule) => rule,
            _ => return Err(missing(NAV_START_RULE)),
        },
        None => return Err(missing(NAV_START_RULE)),
    };
    let mut action_params = Vec::new();
    for group in [NAV_ACTION_PARAMS, NAV_STRAFE_PARAMS] {
        if let Some(params) = child(entry, group) {
            for field in &params.children {
                if let Some(name_crc) = field.name_crc {
                    action_params.push((name_crc, decoder.value_of(group, field).map_err(&err)?));
                }
            }
        }
    }
    Ok(NavConfig {
        kind,
        max_speed,
        min_speed,
        turn_rate_deg,
        linear_acceleration,
        linear_deceleration,
        arrival_threshold,
        target_repath_distance,
        select_position,
        teleport,
        turn_action,
        start_action,
        start_rule,
        action_params,
    })
}

/// One action of an action list, before interning.
#[derive(Debug, Clone, Default, PartialEq)]
struct ListAction {
    name: String,
    fragment: String,
    tasks: Vec<String>,
    completion: Option<(f32, Vec<(String, f32)>)>,
    /// The `<Default>` case of each `SetMannequinTag` activity, with the conditions of the
    /// branches it is in (read by `nw_formats::mannequin`, which knows `Unset`: the mob lists
    /// write it, never `Clear`).
    tags: Vec<(Condition, TagCase)>,
    triggers: Vec<NwEntityTrigger>,
    motion_multiplier: Option<f32>,
    facing: Option<FacingMode>,
    activation: Option<Condition>,
    deactivation: Option<Condition>,
    blocks: Vec<NwBlockActivity>,
    stamina: StaminaActivities,
    /// Its `ChangeActionFragment` cases (read by `nw_formats::mannequin`).
    fragment_cases: Vec<NwFragmentCase>,
}

fn parse_number(text: Option<&str>) -> Option<f32> {
    text?.trim().parse().ok()
}

/// The runtime's effect of a tag change.
const fn tag_effect(change: TagChange) -> TagEffect {
    match change {
        TagChange::Set => TagEffect::Set,
        TagChange::Unset => TagEffect::Clear,
        TagChange::Keep => TagEffect::NoEffect,
    }
}

/// The runtime's effect of a block change.
const fn block_effect(change: BlockChange) -> BlockEffect {
    match change {
        BlockChange::Raise => BlockEffect::Raise,
        BlockChange::Lower => BlockEffect::Lower,
        BlockChange::Keep => BlockEffect::Keep,
    }
}

/// A condition that tests something: an empty `<ActivationCondition/>` reads as an empty `And`.
fn testing(condition: Option<&Condition>) -> Option<&Condition> {
    condition.filter(|condition| !matches!(condition, Condition::All(parts) if parts.is_empty()))
}

/// Milliseconds per second of a tag delay.
const MS_PER_SECOND: f32 = 1000.0;

fn children_named<'a>(node: &'a XmlNode, name: &'a str) -> impl Iterator<Item = &'a XmlNode> {
    node.children.iter().filter(move |child| child.name == name)
}

/// An action list: its actions and its named conditions.
#[derive(Debug, Clone, Default, PartialEq)]
struct ParsedList {
    actions: Vec<ListAction>,
    conditions: Vec<(String, Condition)>,
}

/// The tasks and the action names of a grid with their CAGE actions, each sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GridIndex {
    tasks: Vec<(NameId, u32)>,
    actions: Vec<(NameId, u32)>,
}

/// The actions of an action list and its named conditions.
fn list_actions(text: &str) -> Result<ParsedList, MannequinError> {
    let root = parse_tree(text)?;
    let mut actions = Vec::new();
    root.descendants(ACTION_ELEMENT, &mut actions);
    let mut out = Vec::new();
    for action in actions {
        let Some(name) = action.attribute(NAME_ATTRIBUTE) else {
            continue;
        };
        // An action without a `FragmentName` is kept when the engine starts it by its
        // condition (the `ActivateGrit` action of `baseai_stagger.actionlist`); one a task
        // names needs a fragment to play.
        let engine_started = children_named(action, ACTIVATION_ELEMENT).next().is_some();
        let fragment = action
            .attribute(FRAGMENT_NAME_ATTRIBUTE)
            .unwrap_or_default();
        let mut tasks = Vec::new();
        for activation in children_named(action, ACTIVATION_ELEMENT) {
            for test in TASK_TESTS {
                let mut found = Vec::new();
                activation.descendants(test, &mut found);
                tasks.extend(
                    found
                        .iter()
                        .filter_map(|node| node.attribute(TASK_ATTRIBUTE))
                        .map(str::to_owned),
                );
            }
        }
        if action.attribute(FRAGMENT_NAME_ATTRIBUTE).is_none()
            && (!engine_started || !tasks.is_empty())
        {
            continue;
        }
        let mut completion = None;
        let mut motion_multiplier = None;
        let mut facing = None;
        for activities in action
            .children
            .iter()
            .filter(|node| node.name == "Activities")
        {
            for activity in &activities.children {
                let default = children_named(activity, DEFAULT_ELEMENT).next();
                if COMPLETION_ACTIVITIES.contains(&activity.name.as_str()) && completion.is_none() {
                    if let Some(offset) =
                        default.and_then(|node| parse_number(node.attribute(COMPLETION_OFFSET)))
                    {
                        let conditional = children_named(activity, CONDITIONAL_ELEMENT)
                            .filter_map(|node| {
                                Some((
                                    node.attribute(NAME_ATTRIBUTE)?.to_owned(),
                                    parse_number(node.attribute(COMPLETION_OFFSET))?,
                                ))
                            })
                            .collect();
                        completion = Some((offset, conditional));
                    }
                } else if activity.name == MOTION_ACTIVITY {
                    motion_multiplier =
                        default.and_then(|node| parse_number(node.attribute(MOTION_MULTIPLIER)));
                } else if activity.name == FACING_ACTIVITY {
                    facing = default
                        .and_then(|node| node.attribute(FACING_MODE))
                        .and_then(FacingMode::of);
                }
            }
        }
        out.push(ListAction {
            name: name.to_owned(),
            fragment: fragment.to_owned(),
            tasks,
            completion,
            tags: Vec::new(),
            triggers: Vec::new(),
            motion_multiplier,
            facing,
            activation: None,
            deactivation: None,
            blocks: Vec::new(),
            stamina: StaminaActivities::default(),
            fragment_cases: Vec::new(),
        });
    }
    let parsed = ActionList::parse(text)?;
    for action in &mut out {
        if let Some(list_action) = parsed.action(&action.name) {
            action.triggers.clone_from(&list_action.triggers);
            action.tags = list_action
                .tag_activities
                .iter()
                .filter_map(|activity| {
                    Some((activity.condition.clone(), activity.default.clone()?))
                })
                .collect();
            action.activation.clone_from(&list_action.activation);
            action.deactivation.clone_from(&list_action.deactivation);
            action.blocks.clone_from(&list_action.block_activities);
            action.stamina.clone_from(&list_action.stamina);
            action
                .fragment_cases
                .clone_from(&list_action.fragment_cases);
        }
    }
    Ok(ParsedList {
        actions: out,
        conditions: parsed.conditions,
    })
}

/// The action lists of a grid, in file order: the pak path of each and the alias of each of
/// its actions (`<AliasMap><Action ActionName Alias>`).
fn grid_lists(text: &str) -> Result<GridLists, MannequinError> {
    let lists = CageGrid::lists(text)?;
    Ok(GridLists {
        paths: lists.iter().map(GridList::pak_path).collect(),
        aliases: lists
            .iter()
            .flat_map(|list| &list.aliased)
            .filter_map(|action| Some((action.name.clone(), action.alias.clone()?)))
            .collect(),
    })
}

/// What a grid names: its action lists and the alias of each action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GridLists {
    paths: Vec<String>,
    aliases: Vec<(String, String)>,
}

/// The length of one clip in seconds, or why it is Unknown.
pub(crate) type ClipLength = Result<f64, String>;

/// The length of a `.caf` (its `MotionParams` chunk): its frames times its ticks per frame times
/// its f32 seconds per tick, in f32 as the game holds it (Inferred from the retail scope times:
/// the corruption grunt's `Fidget` of 150 frames at speed 1.25 stopped at tick 120 in 105
/// plays, so its length is past 4 s, as 150 x 0.033333335 rounded to f32 (5.0000005) / 1.25;
/// the same product in f64 rounds to exactly 4 s, a tick short; the damned great axe
/// `POI_Patrol_Fidget` of 540 frames stopped at 539 in 59 plays: 18 s in f32).
fn caf_seconds(bytes: &[u8]) -> Option<f64> {
    if bytes.get(..CAF_MAGIC.len())? != CAF_MAGIC {
        return None;
    }
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    let count = u32_at(CAF_CHUNK_COUNT_AT)? as usize;
    let table = u32_at(CAF_CHUNK_TABLE_AT)? as usize;
    for index in 0..count {
        let entry = table + index * CAF_CHUNK_ENTRY_LEN;
        let kind = u16::from_le_bytes(
            bytes
                .get(entry..entry + CAF_CHUNK_TYPE_LEN)?
                .try_into()
                .ok()?,
        );
        if kind != CAF_MOTION_PARAMS {
            continue;
        }
        let at = u32_at(entry + CAF_CHUNK_OFFSET_AT)? as usize;
        // Frame and tick counts of a few thousand at most.
        #[allow(clippy::cast_precision_loss, reason = "counts of a few thousand")]
        let (ticks_per_frame, start, end) = (
            u32_at(at + MOTION_TICKS_PER_FRAME_AT)? as i32 as f32,
            u32_at(at + MOTION_START_AT)? as i32 as f32,
            u32_at(at + MOTION_END_AT)? as i32 as f32,
        );
        let seconds_per_tick = f32::from_bits(u32_at(at + MOTION_SECONDS_PER_TICK_AT)?);
        return Some(f64::from(
            (end - start) * ticks_per_frame * seconds_per_tick,
        ));
    }
    None
}

/// The clip names a blend space or combined blend space plays (lower-case base names).
fn blend_members(text: &str) -> Result<Vec<String>, MannequinError> {
    let root = parse_tree(text)?;
    let mut members = Vec::new();
    for element in [EXAMPLE_ELEMENT, BLEND_SPACE_ELEMENT] {
        let mut found = Vec::new();
        root.descendants(element, &mut found);
        members.extend(
            found
                .iter()
                .filter_map(|node| node.attribute(CLIP_NAME_ATTRIBUTE))
                .map(base_name),
        );
    }
    Ok(members)
}

/// The `DesiredFacing` of each example of a blend space, in file order; `None` when
/// `DesiredFacing` is not its one dimension or an example has no place in it.
fn facing_params(text: &str) -> Option<Vec<f64>> {
    let root = parse_tree(text).ok()?;
    let mut dimensions = Vec::new();
    root.descendants(DIMENSIONS_ELEMENT, &mut dimensions);
    let mut params = Vec::new();
    for dimension in dimensions {
        dimension.descendants(PARAM_ELEMENT, &mut params);
    }
    let [param] = params.as_slice() else {
        return None;
    };
    if param.attribute_any_case(PARAM_NAME_ATTRIBUTE) != Some(FACING_PARAM) {
        return None;
    }
    let mut examples = Vec::new();
    root.descendants(EXAMPLE_ELEMENT, &mut examples);
    examples
        .iter()
        .map(|example| example.attribute(EXAMPLE_PARAM_ATTRIBUTE)?.parse().ok())
        .collect()
}

/// The lower-case file name of a path without its folder and extension.
fn base_name(path: &str) -> String {
    let file = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_lowercase();
    CLIP_EXTENSIONS
        .iter()
        .find_map(|extension| file.strip_suffix(extension))
        .map_or(file.clone(), str::to_owned)
}

/// The animation files of the install (lower-case paths) by base name.
///
/// # Errors
///
/// Returns [`ToolError`] when a pak cannot be listed.
fn clip_files(game_dir: &Path) -> Result<HashMap<String, Vec<String>>, ToolError> {
    let mut files: HashMap<String, Vec<String>> = HashMap::new();
    for pak in find_paks(game_dir).map_err(AiDataError::from)? {
        let mut archive = PakArchive::open(&pak).map_err(AiDataError::from)?;
        for entry in archive.entries().map_err(AiDataError::from)? {
            let path = entry.path.to_lowercase();
            if CLIP_EXTENSIONS
                .iter()
                .any(|extension| path.ends_with(extension))
            {
                files.entry(base_name(&path)).or_default().push(path);
            }
        }
    }
    Ok(files)
}

/// The clips and skeletons that give the root motion of a CAGE action's entries
/// ([`FragmentVariant::root_motion`]).
///
/// The root motion is the path of the skeleton's root joint, as for the player's attacks
/// ([`root_path_on`], [`root_motion`]). Observed (build 25810808, the 155 `.chr` files of the
/// paks that read, probe `mobroots` of the tools repo's capdecode crate): every skeleton's first
/// joint is its root (`Xform` in 134, `XFORM` in 8, `xform` in 5, an `xform_..._jnt` in 7,
/// `vinesMulti_rootJoint` in 1), with no parent and a bind pose at the origin, and its controller
/// id is the CRC-32 of its name as written. So the root track of a clip is the track
/// of one of these root ids, and any skeleton of that root gives the same root path.
pub(crate) struct RootClips {
    /// The clips by base name; a name that two files share is left out.
    clips: HashMap<String, Caf>,
    /// One skeleton of each root joint, by the root's controller id.
    roots: HashMap<u32, Skeleton>,
}

impl RootClips {
    /// Reads the `.caf` clips `names` (base names) and every skeleton of the paks.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`] when the paks cannot be read.
    fn read(
        source: &GameSource,
        game_dir: &Path,
        names: &BTreeSet<String>,
    ) -> Result<Self, ToolError> {
        let files = clip_files(game_dir)?;
        let wanted: HashSet<String> = names
            .iter()
            .filter_map(|name| match files.get(name).map(Vec::as_slice) {
                Some([path]) if path.ends_with(CAF_EXTENSION) => Some(path.clone()),
                _ => None,
            })
            .collect();
        let clips = read_entries(source, &wanted)?
            .into_iter()
            .filter_map(|(path, bytes)| Some((base_name(&path), Caf::parse(&bytes).ok()?)))
            .collect();
        let mut roots = HashMap::new();
        source.for_each_entry(
            |path| path.ends_with(SKELETON_EXTENSION),
            |_, bytes| {
                if let Ok(skeleton) = Skeleton::parse(&bytes)
                    && let Some(root) = skeleton.joints.first().map(|joint| joint.controller_id)
                {
                    roots.entry(root).or_insert(skeleton);
                }
                Ok::<(), ToolError>(())
            },
        )?;
        Ok(Self { clips, roots })
    }

    /// The root motion of `fragment`, of length `length`: the root path of its main animation
    /// when it moves; `None` for a fragment that loops or whose length is not one time, whose
    /// main animation is not a read `.caf` (a blend space), or whose clip has no root track.
    fn motion(&self, fragment: &Fragment, length: &FragmentLength) -> Option<AttackMotion> {
        let FragmentLength::Seconds(length_s) = length else {
            return None;
        };
        let clip = fragment.main_animation()?;
        let caf = self.clips.get(&clip.name.to_lowercase())?;
        let skeleton = caf.tracks.keys().find_map(|id| self.roots.get(id))?;
        root_path_on(skeleton, caf, clip, f64::from(*length_s))
            .ok()
            .and_then(root_motion)
    }
}

/// The example clips of a blend space on `DesiredFacing`: the facing (radians) and the length
/// (seconds) of each, sorted by facing.
type FacingClips = Vec<(f64, f64)>;

/// Clip lengths by base name, read from the game paks.
pub(crate) struct ClipLengths {
    lengths: HashMap<String, ClipLength>,
    /// The blend spaces on `DesiredFacing`, and the combined blend spaces of such.
    facing: HashMap<String, FacingClips>,
}

impl ClipLengths {
    /// The lengths `entries`, by clip name.
    #[cfg(test)]
    pub(crate) fn of(entries: &[(&str, ClipLength)]) -> Self {
        Self {
            lengths: entries
                .iter()
                .map(|(name, length)| ((*name).to_owned(), length.clone()))
                .collect(),
            facing: HashMap::new(),
        }
    }

    /// Lists the animation files of the install and reads the clips `names` play, and the clips
    /// their blend spaces play.
    pub(crate) fn read(
        source: &GameSource,
        game_dir: &Path,
        names: &BTreeSet<String>,
    ) -> Result<Self, ToolError> {
        let files = clip_files(game_dir)?;
        let mut raw: HashMap<String, Result<Vec<u8>, String>> = HashMap::new();
        let mut pending: BTreeSet<String> = names.clone();
        while !pending.is_empty() {
            let mut wanted = HashSet::new();
            for name in &pending {
                match files.get(name).map(Vec::as_slice) {
                    Some([path]) => {
                        wanted.insert(path.clone());
                    }
                    Some(paths) => {
                        raw.insert(
                            name.clone(),
                            Err(format!("{} files have this name", paths.len())),
                        );
                    }
                    None => {
                        raw.insert(
                            name.clone(),
                            Err("no animation file has this name".to_owned()),
                        );
                    }
                }
            }
            let read = read_entries(source, &wanted)?;
            let mut next = BTreeSet::new();
            for (path, bytes) in read {
                let name = base_name(&path);
                if !path.ends_with(CAF_EXTENSION) {
                    match text_of(Some(&bytes)).map(blend_members) {
                        Some(Ok(members)) => next.extend(
                            members
                                .into_iter()
                                .filter(|member| !raw.contains_key(member)),
                        ),
                        Some(Err(error)) => {
                            raw.insert(name, Err(error.to_string()));
                            continue;
                        }
                        None => {
                            raw.insert(name, Err("blend space is not UTF-8".to_owned()));
                            continue;
                        }
                    }
                }
                raw.insert(name, Ok(bytes));
            }
            pending = next
                .into_iter()
                .filter(|name| !raw.contains_key(name))
                .collect();
        }
        Ok(Self::resolved(&raw))
    }

    /// The lengths of the animation files `raw`, by base name: the bytes of each, or why it
    /// was not read.
    fn resolved(raw: &HashMap<String, Result<Vec<u8>, String>>) -> Self {
        let mut lengths = HashMap::new();
        let mut facing = HashMap::new();
        for name in raw.keys() {
            let length = Self::resolve(name, raw, &mut lengths, &mut facing, &mut Vec::new());
            lengths.insert(name.clone(), length);
        }
        Self { lengths, facing }
    }

    /// The length of `name`: a `.caf` from its chunk, a blend space from the clips it plays when
    /// they all have one length ([`Self::blend_length`]).
    fn resolve(
        name: &str,
        raw: &HashMap<String, Result<Vec<u8>, String>>,
        lengths: &mut HashMap<String, ClipLength>,
        facing: &mut HashMap<String, FacingClips>,
        stack: &mut Vec<String>,
    ) -> ClipLength {
        if let Some(known) = lengths.get(name) {
            return known.clone();
        }
        if stack.iter().any(|open| open == name) {
            return Err("blend space plays itself".to_owned());
        }
        let length = match raw.get(name) {
            None => Err("not read".to_owned()),
            Some(Err(reason)) => Err(reason.clone()),
            Some(Ok(bytes)) => match caf_seconds(bytes) {
                Some(seconds) => Ok(seconds),
                None => match text_of(Some(bytes)).map(|text| (text, blend_members(text))) {
                    Some((text, Ok(members))) if !members.is_empty() => {
                        stack.push(name.to_owned());
                        let parts: Vec<ClipLength> = members
                            .iter()
                            .map(|member| Self::resolve(member, raw, lengths, facing, stack))
                            .collect();
                        stack.pop();
                        Self::blend_length(name, text, &members, &parts, facing)
                    }
                    _ => Err("neither a CAF nor a blend space".to_owned()),
                },
            },
        };
        lengths.insert(name.to_owned(), length.clone());
        length
    }

    /// The length of the blend space `name` (its file `text`), which plays `members` of the
    /// lengths `parts`: their one length; Unknown when they differ. A blend space on
    /// `DesiredFacing`, and a combined blend space of such, also gives `facing` its examples:
    /// its length is then the blend of the examples around the mob's turn
    /// ([`FragmentLength::ByFacing`]).
    fn blend_length(
        name: &str,
        text: &str,
        members: &[String],
        parts: &[ClipLength],
        facing: &mut HashMap<String, FacingClips>,
    ) -> ClipLength {
        let own = facing_params(text)
            .filter(|params| params.len() == parts.len())
            .and_then(|params| {
                params
                    .into_iter()
                    .zip(parts)
                    .map(|(param, part)| Some((param, *part.as_ref().ok()?)))
                    .collect::<Option<FacingClips>>()
            });
        let combined = || {
            members
                .iter()
                .map(|member| facing.get(member).cloned())
                .collect::<Option<Vec<FacingClips>>>()
                .map(|spaces| spaces.concat())
        };
        if let Some(mut examples) = own.or_else(combined) {
            examples.sort_by(|a, b| a.0.total_cmp(&b.0));
            examples.dedup_by(|a, b| a.0 == b.0);
            facing.insert(name.to_owned(), examples);
        }
        if let Some(reason) = parts.iter().find_map(|part| part.as_ref().err()) {
            return Err(format!("a blended clip: {reason}"));
        }
        let mut values = parts.iter().flatten();
        let first = *values.next().ok_or("a blend space of no clips")?;
        if values.all(|value| (value - first).abs() < SAME_LENGTH_S) {
            Ok(first)
        } else {
            Err("blends clips of different lengths".to_owned())
        }
    }

    fn get(&self, name: &str) -> ClipLength {
        self.lengths
            .get(name)
            .cloned()
            .unwrap_or_else(|| Err("not read".to_owned()))
    }

    /// The examples of the blend space `name` on `DesiredFacing`; `None` when it is none.
    fn facing(&self, name: &str) -> Option<&FacingClips> {
        self.facing.get(name)
    }
}

/// How long a fragment entry plays, from its base animation layer (the first; the shared
/// clip timing of [`Fragment::time_chained_clips`]): each clip starts at the sum of the layer's
/// exit times, a chained clip (negative exit time) where the clip before it ends, and plays
/// `(length - StartTime) / speed`; a layer whose last `<Animation>` is empty ends where that
/// clip starts ([`Fragment::layer_ends_s`]; one of negative exit time: at the end of the clip
/// before, [`nw_formats::mannequin::AnimClip::chained_end`]); else the entry ends with its last
/// clip, and a looping last clip never ends. A last clip that is a blend space on
/// `DesiredFacing` of clips of different lengths ends by the mob's turn
/// ([`FragmentLength::ByFacing`]: each example as that clip alone would end). The time base of
/// `ExitTime` and `StartTime` (seconds) is Inferred; it fits the retail boar `Attack_Gore` (one clip; 1,465 and 1,498 ms
/// observed against 1,404 ms + one or two ticks) and `Combat_Turn` (a blend space of 1.8 s
/// clips, 1,632 and 1,683 ms against 1,600 ms). Unknown: an entry without an animation layer or
/// whose base layer has no `<Animation>`, and one whose last clip or a clip before a chained
/// clip has no known length.
pub(crate) fn fragment_length(
    fragment: &Fragment,
    lengths: &ClipLengths,
) -> Result<FragmentLength, String> {
    let Some(layer) = fragment.anim_layers.first() else {
        return Err("no animation layer".to_owned());
    };
    let mut base = Fragment {
        anim_layers: vec![layer.clone()],
        layer_ends_s: vec![fragment.layer_ends_s.first().copied().flatten()],
        ..Fragment::default()
    };
    base.time_chained_clips(&|name| lengths.get(&name.to_lowercase()).ok());
    if base.awaits_clip_lengths {
        return Err("a chained clip follows a clip of no known end".to_owned());
    }
    if let Some(end_s) = base.layer_ends_s[0] {
        return Ok(FragmentLength::Seconds(end_s as f32));
    }
    let Some(last) = base.anim_layers[0].last() else {
        return Err("the base layer has no animation".to_owned());
    };
    if last.looped {
        return Ok(FragmentLength::Loops);
    }
    let name = last.name.to_lowercase();
    match (lengths.get(&name), lengths.facing(&name)) {
        (Ok(length), _) => Ok(FragmentLength::Seconds(last.natural_end_s(length) as f32)),
        (Err(_), Some(examples)) => Ok(FragmentLength::ByFacing(
            examples
                .iter()
                .map(|(facing, length)| FacingLength {
                    facing_rad: *facing as f32,
                    seconds: last.natural_end_s(*length) as f32,
                })
                .collect(),
        )),
        (Err(reason), None) => Err(format!("{}: {reason}", last.name)),
    }
}

/// The lower-case names of the clips the base animation layers of `databases` play: the clips
/// whose lengths [`fragment_length`] reads.
pub(crate) fn base_clip_names<'a>(
    databases: impl IntoIterator<Item = &'a AnimDatabase>,
) -> BTreeSet<String> {
    databases
        .into_iter()
        .flat_map(|database| &database.fragments)
        .filter_map(|fragment| fragment.anim_layers.first())
        .flatten()
        .map(|clip| clip.name.to_lowercase())
        .collect()
}

fn cage_condition(decoder: &mut TreeDecoder, condition: &Condition) -> CageCondition {
    match condition {
        Condition::All(parts) => CageCondition::All(
            parts
                .iter()
                .map(|part| cage_condition(decoder, part))
                .collect(),
        ),
        Condition::Any(parts) => CageCondition::Any(
            parts
                .iter()
                .map(|part| cage_condition(decoder, part))
                .collect(),
        ),
        Condition::Not(part) => CageCondition::Not(Box::new(cage_condition(decoder, part))),
        Condition::True => CageCondition::True,
        Condition::Tag(tag) => CageCondition::Tag(decoder.intern(tag)),
        Condition::Marker(marker) => CageCondition::Marker(decoder.intern(marker)),
        Condition::StatusEffect(effect) => CageCondition::StatusEffect(decoder.intern(effect)),
        Condition::FragmentPlaying(Some(fragment)) => {
            CageCondition::FragmentPlaying(decoder.intern(fragment))
        }
        Condition::DamagedByAngle { min_deg, max_deg } => CageCondition::DamagedByAngle {
            min_deg: *min_deg,
            max_deg: *max_deg,
        },
        // A mob's tree reads tags, markers, status effects and named playing fragments only:
        // every other test stays a test by its element name, as before the parser knew them.
        other => CageCondition::Other(decoder.intern(other.test_element().unwrap_or_default())),
    }
}

/// The tasks a tree can request, by name (literal tasks and tasks the archetype's config gives
/// a keyed `ActionName`).
fn tree_tasks(
    data_nodes: &[nw_ai::node::Node],
    child_ids: &[NodeId],
    root: NodeId,
    arch: &Archetype,
) -> BTreeSet<NameId> {
    let mut tasks = BTreeSet::new();
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let kind = &data_nodes[id as usize].kind;
        if let Some(Param { key, value }) = kind.task() {
            match (key.and_then(|key| arch.config_value(key)), value) {
                (Some(Value::Text(task)), _) | (None, Some(Value::Text(task))) => {
                    tasks.insert(task);
                }
                _ => {}
            }
        }
        stack.extend(kind.child());
        if let Some(children) = kind.children() {
            let first = children.first as usize;
            stack.extend_from_slice(&child_ids[first..first + children.count as usize]);
        }
    }
    tasks
}

/// The pak path of the datasheet of a slice's `m_damageTable` (Observed: the slice names
/// `.../javelindata_damagetable_boar.xml`, the paks hold `.../javelindata_damagetable_boar.datasheet`).
fn datasheet_path(table: &str) -> String {
    table.strip_suffix(DAMAGE_TABLE_SUFFIX).map_or_else(
        || table.to_owned(),
        |stem| format!("{stem}{DATASHEET_SUFFIX}"),
    )
}

/// A damage table datasheet: its unique id and its rows' `DamageID`s in sheet order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DamageSheet {
    unique_id: String,
    rows: Vec<String>,
}

impl DamageSheet {
    fn of(sheet: Datasheet) -> Self {
        let id_column = sheet
            .columns
            .iter()
            .position(|column| column.name == DAMAGE_ID_COLUMN);
        let rows = id_column.map_or_else(Vec::new, |column| {
            sheet
                .rows
                .iter()
                .filter_map(|row| row.get(column).cloned())
                .collect()
        });
        Self {
            unique_id: sheet.unique_id,
            rows,
        }
    }
}

/// The damage table datasheet of each slice that names one, by the slice's table path; a
/// datasheet that is missing or does not parse is listed in the report.
fn damage_tables(
    source: &GameSource,
    slices: &BTreeMap<String, SliceAi>,
    report: &mut AiDataReport,
) -> Result<HashMap<String, DamageSheet>, ToolError> {
    let tables: HashSet<String> = slices
        .values()
        .filter_map(|slice| slice.damage_table.clone())
        // Some slices hold an empty path: they name no table.
        .filter(|table| !table.is_empty())
        .collect();
    let sheets: HashSet<String> = tables.iter().map(|table| datasheet_path(table)).collect();
    let bytes = read_entries(source, &sheets)?;
    let mut ids = HashMap::new();
    for table in tables {
        let sheet = datasheet_path(&table);
        match bytes.get(&sheet).map(|bytes| Datasheet::parse(bytes)) {
            Some(Ok(parsed)) => {
                ids.insert(table, DamageSheet::of(parsed));
            }
            Some(Err(error)) => {
                report.skipped.insert(sheet, error.to_string());
            }
            None => {
                report.skipped.insert(sheet, "not in the paks".to_owned());
            }
        }
    }
    Ok(ids)
}

fn asset_bytes(hex: &str) -> Result<[u8; 16], AiDataError> {
    let bytes = hex::decode(hex).map_err(|_| AiDataError::Asset(hex.to_owned()))?;
    bytes
        .try_into()
        .map_err(|_| AiDataError::Asset(hex.to_owned()))
}

fn slice_root(bytes: &[u8]) -> Result<Element, String> {
    let stream = if is_azcs(bytes) {
        decode_azcs(bytes).map_err(|error| error.to_string())?
    } else {
        bytes.to_vec()
    };
    parse_object_stream(&stream).map_err(|error| error.to_string())
}

/// Builds the AI data from `source` (the install at `game_dir`), the Server.pak at
/// `server_pak` and the creature animation table `creatures`.
///
/// # Errors
///
/// Returns [`ToolError`] when Server.pak or a pak cannot be read, or a tree does not decode.
pub fn build_ai_data(
    source: &GameSource,
    game_dir: &Path,
    server_pak: &Path,
    creatures: &[CreatureRow],
) -> Result<(AiData, AiDataReport), ToolError> {
    let mut report = AiDataReport::default();
    let mut decoder = TreeDecoder::default();
    {
        let entries =
            server_pak_entries(server_pak, &KrakenDecompressor).map_err(AiDataError::from)?;
        decode_entries(&mut decoder, &entries).map_err(AiDataError::from)?;
        for (path, root) in stand_in_trees(&entries).map_err(AiDataError::from)? {
            decoder
                .add_tree(path, &root)
                .map_err(|source| AiDataError::Decode {
                    path: path.to_owned(),
                    source,
                })?;
        }
    }
    let slice_paths: HashSet<String> = creatures
        .iter()
        .map(|row| row.slice.to_lowercase())
        .collect();
    let slice_bytes = read_entries(source, &slice_paths)?;
    let mut slices: BTreeMap<String, SliceAi> = BTreeMap::new();
    for path in &slice_paths {
        let Some(bytes) = slice_bytes.get(path) else {
            report
                .skipped
                .insert(path.clone(), "not in the paks".to_owned());
            continue;
        };
        match slice_root(bytes) {
            Ok(root) => {
                let mut slice = SliceAi::read(&root, &mut decoder, path)?;
                slice.weapon = weapon_item(&root);
                // A slice without triggers, or with an unreadable one, runs none.
                slice.triggers = slice_triggers(&root)
                    .map(|triggers| {
                        triggers
                            .into_iter()
                            .map(|trigger| {
                                (
                                    trigger.name,
                                    trigger.main ^ trigger.owner,
                                    trigger.component,
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                slices.insert(path.clone(), slice);
            }
            Err(reason) => {
                report.skipped.insert(path.clone(), reason);
            }
        }
    }
    drop(slice_bytes);
    let tables = damage_tables(source, &slices, &mut report)?;
    let grid_paths: HashSet<String> = slices
        .values()
        .filter_map(|slice| slice.grid.clone())
        .collect();
    let grid_bytes = read_entries(source, &grid_paths)?;
    let mut grid_lists_by_path: HashMap<String, GridLists> = HashMap::new();
    for path in &grid_paths {
        match text_of(grid_bytes.get(path)).map(grid_lists) {
            Some(Ok(lists)) => {
                grid_lists_by_path.insert(path.clone(), lists);
            }
            Some(Err(error)) => {
                report.skipped.insert(path.clone(), error.to_string());
            }
            None => {
                report
                    .skipped
                    .insert(path.clone(), "grid missing or not UTF-8".to_owned());
            }
        }
    }
    let list_paths: HashSet<String> = grid_lists_by_path
        .values()
        .flat_map(|lists| lists.paths.iter().cloned())
        .collect();
    let list_bytes = read_entries(source, &list_paths)?;
    let mut lists: HashMap<String, ParsedList> = HashMap::new();
    for path in &list_paths {
        match text_of(list_bytes.get(path)).map(list_actions) {
            Some(Ok(parsed)) => {
                lists.insert(path.clone(), parsed);
            }
            Some(Err(error)) => {
                report.skipped.insert(path.clone(), error.to_string());
            }
            None => {
                report
                    .skipped
                    .insert(path.clone(), "action list missing or not UTF-8".to_owned());
            }
        }
    }
    // The animation database of each slice's main-hand weapon (its creature row's).
    let weapon_adbs: HashMap<String, String> = creatures
        .iter()
        .filter_map(|row| {
            Some((
                row.slice.to_lowercase(),
                row.weapon_animation_database.clone()?,
            ))
        })
        .collect();
    let adb_paths: HashSet<String> = slices
        .values()
        .filter_map(|slice| slice.animation_database.clone())
        .chain(weapon_adbs.values().cloned())
        .collect();
    let mut adbs: HashMap<String, AnimDatabase> = HashMap::new();
    for (path, parsed) in read_databases(source, &adb_paths)? {
        match parsed {
            Ok(parsed) => {
                adbs.insert(path, parsed);
            }
            Err(reason) => {
                report.skipped.insert(path, reason);
            }
        }
    }
    let clip_names = base_clip_names(adbs.values());
    let lengths = ClipLengths::read(source, game_dir, &clip_names)?;
    for name in &clip_names {
        if let Err(reason) = lengths.get(name)
            && lengths.facing(name).is_none()
        {
            report.unknown_clips.insert(name.clone(), reason);
        }
    }

    // The main animations of the entries of the actions that the animation's root moves.
    let motion_fragments: HashSet<&str> = lists
        .values()
        .flat_map(|list| &list.actions)
        .filter(|action| action.motion_multiplier.is_some())
        .flat_map(|action| {
            std::iter::once(action.fragment.as_str()).chain(
                action
                    .fragment_cases
                    .iter()
                    .map(|case| case.fragment.as_str()),
            )
        })
        .collect();
    let root_clip_names: BTreeSet<String> = adbs
        .values()
        .flat_map(|database| &database.fragments)
        .filter(|fragment| motion_fragments.contains(fragment.name.as_str()))
        .filter_map(Fragment::main_animation)
        .map(|clip| clip.name.to_lowercase())
        .collect();
    let roots = RootClips::read(source, game_dir, &root_clip_names)?;
    let mut builder = CageBuilder {
        lists: &lists,
        lengths: &lengths,
        roots: &roots,
        actions: Vec::new(),
        index: HashMap::new(),
        unknown_fragments: BTreeMap::new(),
    };
    let mut grids: Vec<Grid> = Vec::new();
    let mut grid_index: HashMap<(String, Option<String>, Option<String>), u32> = HashMap::new();
    let mut archetypes: Vec<Archetype> = Vec::new();
    let mut archetype_of_slice: HashMap<String, u32> = HashMap::new();
    for (path, slice) in &slices {
        let weapon_adb = weapon_adbs.get(path);
        let grid = match &slice.grid {
            Some(grid) if grid_lists_by_path.contains_key(grid) => {
                let key = (
                    grid.clone(),
                    slice.animation_database.clone(),
                    weapon_adb.cloned(),
                );
                match grid_index.get(&key) {
                    Some(index) => Some(*index),
                    None => {
                        let named = &grid_lists_by_path[grid];
                        let GridIndex { tasks, actions } = builder.grid(
                            &mut decoder,
                            &named.paths,
                            database(&adbs, slice.animation_database.as_deref()),
                            database(&adbs, weapon_adb.map(String::as_str)),
                        );
                        let mut first_alias: BTreeMap<NameId, NameId> = BTreeMap::new();
                        for (action, alias) in &named.aliases {
                            let action = decoder.intern(action);
                            let alias = decoder.intern(alias);
                            first_alias.entry(action).or_insert(alias);
                        }
                        let aliases: Vec<(NameId, NameId)> = first_alias.into_iter().collect();
                        let index = u32::try_from(grids.len()).unwrap_or(u32::MAX);
                        grids.push(Grid {
                            path: decoder.intern(grid),
                            animation_database: slice
                                .animation_database
                                .as_ref()
                                .map(|adb| decoder.intern(adb)),
                            tasks,
                            actions,
                            aliases,
                        });
                        grid_index.insert(key, index);
                        Some(index)
                    }
                }
            }
            _ => None,
        };
        let config = slice
            .config
            .iter()
            .map(|(name, value)| (decoder.key(name), *value))
            .collect();
        let auto_evaluate = slice
            .auto_evaluate
            .iter()
            .map(|name| decoder.key(name))
            .collect();
        let row_tags: BTreeSet<&String> = creatures
            .iter()
            .filter(|row| row.slice.to_lowercase() == *path)
            .flat_map(|row| &row.default_tags)
            .collect();
        let archetype = Archetype {
            slice: decoder.intern(path),
            trees: slice
                .trees
                .iter()
                .map(|tree| TreeRef {
                    path: decoder.intern(tree),
                    tree: None,
                })
                .collect(),
            config,
            auto_evaluate,
            auto_evaluation_s: slice.auto_evaluation_s,
            ambient_vision: slice.ambient_vision,
            combat_vision: slice.combat_vision,
            group_aggro_distance: slice.group_aggro_distance,
            group_aggro_cap: slice.group_aggro_cap,
            targeting_profile: slice
                .targeting_profile
                .as_ref()
                .map(|profile| decoder.intern(profile)),
            aggro_delay_override_s: slice.aggro_delay_override_s,
            nav_configs: slice.nav_configs.clone(),
            max_slope_deg: slice.max_slope_deg,
            grid,
            animation_database: slice
                .animation_database
                .as_ref()
                .map(|adb| decoder.intern(adb)),
            damage_table: slice
                .damage_table
                .as_ref()
                .map(|table| decoder.intern(table)),
            damage_table_id: slice
                .damage_table
                .as_ref()
                .and_then(|table| tables.get(table))
                .map(|sheet| decoder.intern(&sheet.unique_id)),
            weapon_item: slice.weapon.as_ref().map(|item| decoder.intern(item)),
            default_tags: row_tags
                .into_iter()
                .map(|tag| decoder.intern(tag))
                .collect(),
            initial_effects: slice
                .initial_effects
                .iter()
                .map(|effect| decoder.intern(effect))
                .collect(),
            grit_uses_stamina: slice.grit_uses_stamina,
            large_status_effects_fx: slice.large_status_effects_fx,
            triggers: slice
                .triggers
                .iter()
                .map(|(name, child, component)| SliceTriggerRef {
                    name: decoder.intern(name),
                    child: *child,
                    component: *component,
                })
                .collect(),
        };
        archetype_of_slice.insert(
            path.clone(),
            u32::try_from(archetypes.len()).unwrap_or(u32::MAX),
        );
        archetypes.push(archetype);
    }
    let shots = ranged_shots(source, &mut decoder, &builder.actions, &mut report)?;
    let damage_table_rows = table_rows(&mut decoder, tables.into_values());
    let trees = decoder.finish();
    let tree_of: HashMap<NameId, u32> = trees
        .trees
        .iter()
        .enumerate()
        .map(|(index, tree)| (tree.path, u32::try_from(index).unwrap_or(u32::MAX)))
        .collect();
    for archetype in &mut archetypes {
        for reference in &mut archetype.trees {
            reference.tree = tree_of.get(&reference.path).copied();
            if reference.tree.is_none() {
                report
                    .missing_trees
                    .entry(trees.names[reference.path as usize].clone())
                    .or_default()
                    .insert(trees.names[archetype.slice as usize].clone());
            }
        }
        let root = archetype
            .trees
            .iter()
            .find_map(|reference| reference.tree)
            .and_then(|tree| trees.trees[tree as usize].root);
        if let Some(root) = root {
            let grid = archetype.grid.and_then(|grid| grids.get(grid as usize));
            for task in tree_tasks(&trees.nodes, &trees.child_ids, root, archetype) {
                if grid.and_then(|grid| grid.action(task)).is_none() {
                    report
                        .missing_tasks
                        .entry(trees.names[archetype.slice as usize].clone())
                        .or_default()
                        .insert(trees.names[task as usize].clone());
                }
            }
        }
    }
    let mut creature_list = Vec::new();
    for row in creatures {
        if let Some(archetype) = archetype_of_slice.get(&row.slice.to_lowercase()) {
            creature_list.push(Creature {
                asset: asset_bytes(&row.asset)?,
                archetype: *archetype,
            });
        }
    }
    creature_list.sort_by_key(|creature| creature.asset);
    creature_list.dedup_by(|left, right| left.asset == right.asset);
    report.trees = trees.trees.len();
    report.nodes = trees.nodes.len();
    report.expanded_nodes = trees
        .trees
        .iter()
        .map(|tree| u64::from(tree.expanded_nodes))
        .sum();
    report.creatures = creature_list.len();
    report.archetypes = archetypes.len();
    report.grids = grids.len();
    report.cage_actions = builder.actions.len();
    report.with_tree = archetypes
        .iter()
        .filter(|arch| arch.trees.iter().any(|tree| tree.tree.is_some()))
        .count();
    report.with_grid = archetypes.iter().filter(|arch| arch.grid.is_some()).count();
    report.with_animation_database = archetypes
        .iter()
        .filter(|arch| arch.animation_database.is_some())
        .count();
    let variants = builder.actions.iter().flat_map(|action| &action.variants);
    report.variants = variants.clone().count();
    report.timed_variants = variants
        .filter(|variant| {
            matches!(
                variant.length,
                FragmentLength::Seconds(_) | FragmentLength::ByFacing(_)
            )
        })
        .count();
    report.unknown_fragments = builder.unknown_fragments;
    let data = AiData {
        names: trees.names,
        keys: trees.keys,
        nodes: trees.nodes,
        child_ids: trees.child_ids,
        trees: trees.trees,
        archetypes,
        grids,
        cage_actions: builder.actions,
        creatures: creature_list,
        ranged_attacks: shots.attacks,
        projectile_slices: shots.flights,
        ranged_attack_streams: shots.streams,
        projectile_entities: shots.entities,
        damage_table_rows,
    };
    Ok((data, report))
}

/// What the ranged clips of the creatures fire: every attack of the ranged attack database (its
/// range and radius, and its stream place and arc), and the flight and replicated entity of each
/// projectile slice they name.
struct RangedShots {
    attacks: Vec<RangedAttack>,
    streams: Vec<RangedAttackStream>,
    flights: Vec<ProjectileSlice>,
    entities: Vec<ProjectileEntity>,
}

/// The attacks of the ranged attack database `root`, sorted by profile and attack, with the
/// stream place and arc of each.
fn database_attacks(
    decoder: &mut TreeDecoder,
    root: &Element,
) -> (Vec<RangedAttack>, Vec<RangedAttackStream>) {
    let mut attacks: Vec<_> = ranged_attacks(root).into_iter().collect();
    attacks.sort_by(|left, right| left.0.cmp(&right.0));
    let ranged = attacks
        .into_iter()
        .map(
            |((profile, attack), (range_m, radius_m, hit_scan))| RangedAttack {
                profile: decoder.intern(&profile),
                attack: decoder.intern(&attack),
                range_m,
                radius_m,
                hit_scan,
            },
        )
        .collect();
    let streams = ranged_attack_entries(root)
        .into_iter()
        .map(|entry| RangedAttackStream {
            profile: decoder.intern(&entry.profile),
            attack: decoder.intern(&entry.attack),
            index: entry.index,
            arc: arc_trajectory(entry.data),
        })
        .collect();
    (ranged, streams)
}

/// The `m_arcTrajectorySettings` of the ranged attack data `data`, when `m_useArcTrajectories`
/// is set and every value is there.
fn arc_trajectory(data: &Element) -> Option<ArcTrajectory> {
    let settings = child(data, ARC_SETTINGS)?;
    if !child(settings, ARC_ENABLED)?.as_bool().ok()? {
        return None;
    }
    let float = |name: &str| child(settings, name)?.as_f32().ok();
    Some(ArcTrajectory {
        lob_height_m: [float(ARC_LOB_MIN)?, float(ARC_LOB_MAX)?],
        gravity_mps2: [float(ARC_GRAVITY_MIN)?, float(ARC_GRAVITY_MAX)?],
        distance_m: [float(ARC_DISTANCE_MIN)?, float(ARC_DISTANCE_MAX)?],
    })
}

/// Every element of type `type_id` at or below `root`, depth first.
fn components_of<'a>(root: &'a Element, type_id: &[u8; 16]) -> Vec<&'a Element> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(next) = stack.pop() {
        if &next.type_id == type_id {
            found.push(next);
        }
        stack.extend(next.children.iter().rev());
    }
    found
}

/// The replica index of the first component of type `type_id` in the slice `root` that has one
/// above 0 (0: not replicated).
fn replicated(root: &Element, type_id: &[u8; 16]) -> Option<u64> {
    components_of(root, type_id)
        .into_iter()
        .find_map(|component| replica_index(component).filter(|index| *index > 0))
}

/// The projectile component of the slice `root`: its replica index, its `m_stuckLifetime` and
/// the impact slice it spawns where it hits (`m_spawnOnHitAsset`), when it names one.
fn projectile_component(root: &Element) -> Option<(u64, f32, Option<AssetRef>)> {
    let component = components_of(root, &PROJECTILE_COMPONENT_TYPE)
        .into_iter()
        .find(|component| replica_index(component).is_some_and(|index| index > 0))?;
    let stuck_s = first(component, STUCK_LIFETIME)?.as_f32().ok()?;
    let impact = level_asset(first(component, SPAWN_ON_HIT)).ok().flatten();
    Some((replica_index(component)?, stuck_s, impact))
}

/// The area hit of the impact slice `root`: the first component whose `m_damageData` names a
/// row, with the radius of its shape; `None` when no component of it hurts.
fn impact_hit(root: &Element) -> Option<(String, String, f32)> {
    let mut stack = vec![root];
    while let Some(next) = stack.pop() {
        let damage = child(next, DAMAGE_DATA);
        let text = |name: &str| {
            damage
                .and_then(|damage| child(damage, name))
                .and_then(|value| value.as_string().ok())
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        if let (Some(table), Some(row)) = (text(DAMAGE_DATA_TABLE), text(DAMAGE_DATA_ENTRY)) {
            let radius_m = first(next, DAMAGE_SHAPE)
                .and_then(|shape| first(shape, SHAPE_RADIUS))
                .and_then(|radius| radius.as_f32().ok())?;
            return Some((table, row, radius_m));
        }
        stack.extend(next.children.iter().rev());
    }
    None
}

/// The impact of the impact slice `root` of asset `asset`; `None` when it hurts no one.
fn projectile_impact(
    decoder: &mut TreeDecoder,
    root: &Element,
    asset: [u8; 16],
) -> Option<ProjectileImpact> {
    let (table, row, radius_m) = impact_hit(root)?;
    let mut delayed_events: Vec<DelayedEvent> = components_of(root, &DELAYED_EVENT_TYPE)
        .into_iter()
        .filter_map(|event| {
            Some(DelayedEvent {
                component: replica_index(event).filter(|index| *index > 0)?,
                delay_s: child(event, DELAY_SECONDS)?.as_f32().ok()?,
            })
        })
        .collect();
    delayed_events.sort_by_key(|event| event.component);
    Some(ProjectileImpact {
        asset,
        table: decoder.intern(&table),
        row: decoder.intern(&row),
        radius_m,
        ownership_component: replicated(root, &OWNERSHIP),
        position_component: replicated(root, &NETWORK_POSITION_TYPE),
        delayed_events,
    })
}

/// The asset GUID of each of the dynamic slices `paths` in the asset catalog of `source`; a
/// catalog that cannot be read is listed in the report and gives none.
fn slice_assets(
    source: &GameSource,
    paths: &BTreeSet<String>,
    report: &mut AiDataReport,
) -> Result<HashMap<String, [u8; 16]>, ToolError> {
    let mut catalog = None;
    source.for_each_entry(
        |path| path == ASSET_CATALOG_ENTRY,
        |_, bytes| {
            catalog = Some(slice_assets_in(&bytes, SLICES_FOLDER));
            Ok::<(), ToolError>(())
        },
    )?;
    let assets = match catalog {
        Some(Ok(assets)) => assets,
        Some(Err(error)) => {
            report
                .skipped
                .insert(ASSET_CATALOG_ENTRY.to_owned(), error.to_string());
            return Ok(HashMap::new());
        }
        None => {
            report
                .skipped
                .insert(ASSET_CATALOG_ENTRY.to_owned(), "not in the paks".to_owned());
            return Ok(HashMap::new());
        }
    };
    Ok(assets
        .into_iter()
        .filter_map(|(path, (guid, _))| {
            let path = path.to_lowercase();
            paths.contains(&path).then_some((path, guid))
        })
        .collect())
}

/// Every attack of the ranged attack database, and the flight and replicated entity of each
/// projectile slice that a `CAGE-RangedAttack` clip of `actions` names (`SliceOverride`). A slice
/// that is missing, has no projectile component or is not in the asset catalog, and an impact
/// slice that is missing, go to the report's skipped files.
fn ranged_shots(
    source: &GameSource,
    decoder: &mut TreeDecoder,
    actions: &[CageAction],
    report: &mut AiDataReport,
) -> Result<RangedShots, ToolError> {
    let ranged_clip = decoder.intern(RANGED_CLIP);
    let slice_param = decoder.intern(SLICE_OVERRIDE_PARAM);
    let slice_paths: BTreeSet<String> = actions
        .iter()
        .flat_map(|action| action.variants.iter().chain(&action.weapon_variants))
        .flat_map(|variant| &variant.windows)
        .filter(|window| window.kind == ranged_clip)
        .filter_map(|window| {
            window
                .params
                .iter()
                .find(|(name, _)| *name == slice_param)
                .map(|(_, slice)| decoder.name(*slice).to_lowercase())
        })
        .filter(|slice| !slice.is_empty())
        .collect();
    let wanted: HashSet<String> = slice_paths
        .iter()
        .cloned()
        .chain([RANGED_ATTACK_DATABASE.to_owned()])
        .collect();
    let bytes = read_entries(source, &wanted)?;
    let (attacks, streams) = match bytes
        .get(RANGED_ATTACK_DATABASE)
        .map(|bytes| slice_root(bytes))
    {
        Some(Ok(root)) => database_attacks(decoder, &root),
        Some(Err(reason)) => {
            report
                .skipped
                .insert(RANGED_ATTACK_DATABASE.to_owned(), reason);
            (Vec::new(), Vec::new())
        }
        None => {
            report.skipped.insert(
                RANGED_ATTACK_DATABASE.to_owned(),
                "not in the paks".to_owned(),
            );
            (Vec::new(), Vec::new())
        }
    };
    let assets = slice_assets(source, &slice_paths, report)?;
    let mut flights = Vec::new();
    let mut projectiles = Vec::new();
    for path in &slice_paths {
        let root = bytes
            .get(path)
            .ok_or_else(|| "not in the paks".to_owned())
            .and_then(|bytes| slice_root(bytes));
        let flight = root.as_ref().map_err(String::clone).and_then(|root| {
            slice_flight(root).ok_or_else(|| "no projectile component".to_owned())
        });
        match flight {
            Ok(flight) => flights.push(ProjectileSlice {
                slice: decoder.intern(path),
                flight,
            }),
            Err(reason) => {
                report.skipped.insert(path.clone(), reason);
            }
        }
        let Ok(root) = root else { continue };
        let Some((component, stuck_s, impact)) = projectile_component(&root) else {
            continue;
        };
        let Some(asset) = assets.get(path) else {
            report
                .skipped
                .insert(path.clone(), "not in the asset catalog".to_owned());
            continue;
        };
        let ownership = replicated(&root, &OWNERSHIP);
        projectiles.push((
            path.clone(),
            *asset,
            (component, stuck_s),
            ownership,
            impact,
        ));
    }
    let impact_paths: HashSet<String> = projectiles
        .iter()
        .filter_map(|(_, _, _, _, impact)| impact.as_ref().map(|impact| impact.path.clone()))
        .collect();
    let impact_bytes = read_entries(source, &impact_paths)?;
    let mut entities = Vec::new();
    for (path, asset, (projectile_component, stuck_lifetime_s), ownership_component, impact) in
        projectiles
    {
        let impact = impact.and_then(|impact| {
            let root = impact_bytes
                .get(&impact.path)
                .ok_or_else(|| "not in the paks".to_owned())
                .and_then(|bytes| slice_root(bytes));
            match root {
                Ok(root) => projectile_impact(decoder, &root, impact.guid),
                Err(reason) => {
                    report.skipped.insert(impact.path, reason);
                    None
                }
            }
        });
        entities.push(ProjectileEntity {
            slice: decoder.intern(&path),
            asset,
            projectile_component,
            stuck_lifetime_s,
            ownership_component,
            impact,
        });
    }
    report.ranged_attacks = attacks.len();
    report.projectile_slices = flights.len();
    report.projectile_entities = entities.len();
    Ok(RangedShots {
        attacks,
        streams,
        flights,
        entities,
    })
}

/// The rows of each of the damage tables `sheets`, once per unique id.
fn table_rows(
    decoder: &mut TreeDecoder,
    sheets: impl IntoIterator<Item = DamageSheet>,
) -> Vec<DamageTableRows> {
    let by_id: BTreeMap<String, Vec<String>> = sheets
        .into_iter()
        .map(|sheet| (sheet.unique_id, sheet.rows))
        .collect();
    by_id
        .into_iter()
        .map(|(table, rows)| DamageTableRows {
            table: decoder.intern(&table),
            rows: rows.iter().map(|row| decoder.intern(row)).collect(),
        })
        .collect()
}

/// The parsed animation database at `path` among `adbs`, with its path.
fn database<'a>(
    adbs: &'a HashMap<String, AnimDatabase>,
    path: Option<&'a str>,
) -> Option<(&'a str, &'a AnimDatabase)> {
    let path = path?;
    Some((path, adbs.get(path)?))
}

/// The CAGE actions built so far, shared by the grids (equal actions are stored once).
struct CageBuilder<'a> {
    lists: &'a HashMap<String, ParsedList>,
    lengths: &'a ClipLengths,
    roots: &'a RootClips,
    actions: Vec<CageAction>,
    index: HashMap<Vec<u8>, u32>,
    unknown_fragments: BTreeMap<String, usize>,
}

impl CageBuilder<'_> {
    /// The tasks and actions of one grid with the CAGE actions they resolve to, each sorted by
    /// name (the first action list that has a task or an action name wins, as the client reads
    /// the grid's lists in order).
    fn grid(
        &mut self,
        decoder: &mut TreeDecoder,
        list_paths: &[String],
        adb: Option<(&str, &AnimDatabase)>,
        weapon_adb: Option<(&str, &AnimDatabase)>,
    ) -> GridIndex {
        let mut tasks: BTreeMap<NameId, u32> = BTreeMap::new();
        let mut names: BTreeMap<NameId, u32> = BTreeMap::new();
        for list_path in list_paths {
            let Some(ParsedList {
                actions,
                conditions,
            }) = self.lists.get(list_path)
            else {
                continue;
            };
            let list = decoder.intern(list_path);
            for action in actions {
                let cage = self.action(decoder, list, action, conditions, adb, weapon_adb);
                let name = cage.action;
                let key = serde_json::to_vec(&cage).unwrap_or_default();
                let index = match self.index.get(&key) {
                    Some(index) => *index,
                    None => {
                        let index = u32::try_from(self.actions.len()).unwrap_or(u32::MAX);
                        self.actions.push(cage);
                        self.index.insert(key, index);
                        index
                    }
                };
                names.entry(name).or_insert(index);
                for task in &action.tasks {
                    tasks.entry(decoder.intern(task)).or_insert(index);
                }
            }
        }
        GridIndex {
            tasks: tasks.into_iter().collect(),
            actions: names.into_iter().collect(),
        }
    }

    fn action(
        &mut self,
        decoder: &mut TreeDecoder,
        list: NameId,
        action: &ListAction,
        conditions: &[(String, Condition)],
        adb: Option<(&str, &AnimDatabase)>,
        weapon_adb: Option<(&str, &AnimDatabase)>,
    ) -> CageAction {
        let completion = action
            .completion
            .as_ref()
            .map(|(default_offset_s, conditional)| Completion {
                default_offset_s: *default_offset_s,
                conditional: conditional
                    .iter()
                    .map(|(name, offset)| {
                        let condition = match conditions.iter().find(|(known, _)| known == name) {
                            Some((_, condition)) => cage_condition(decoder, condition),
                            None => CageCondition::Other(decoder.intern(name)),
                        };
                        (condition, *offset)
                    })
                    .collect(),
            });
        // The animation moves the body (`AnimationDrivenMotion`); a weapon's entry moves none.
        let moves = action.motion_multiplier.is_some();
        let variants = self.variants(decoder, &action.fragment, adb, moves);
        let weapon_variants = self.variants(decoder, &action.fragment, weapon_adb, false);
        let fragment_cases = action
            .fragment_cases
            .iter()
            .map(|case| FragmentCase {
                condition: cage_condition(decoder, &case.condition),
                fragment: decoder.intern(&case.fragment),
                variants: self.variants(decoder, &case.fragment, adb, moves),
            })
            .collect();
        CageAction {
            list,
            action: decoder.intern(&action.name),
            fragment: decoder.intern(&action.fragment),
            completion,
            tags: action
                .tags
                .iter()
                .map(|(condition, case)| TagActivity {
                    condition: cage_condition(decoder, condition),
                    tag: decoder.intern(&case.tag),
                    on_enter: tag_effect(case.on_enter),
                    on_exit: tag_effect(case.on_exit),
                    // A delay of a few seconds.
                    delay_s: case.delay_ms as f32 / MS_PER_SECOND,
                })
                .collect(),
            triggers: action
                .triggers
                .iter()
                .map(|trigger| EntityTrigger {
                    trigger: decoder.intern(&trigger.name),
                    condition: cage_condition(decoder, &trigger.condition),
                })
                .collect(),
            motion_multiplier: action.motion_multiplier,
            facing: action.facing,
            variants,
            weapon_variants,
            // The engine starts an action without a task by its condition; a task action's
            // condition names its task.
            activation: testing(action.activation.as_ref())
                .filter(|_| action.tasks.is_empty())
                .map(|condition| cage_condition(decoder, condition)),
            deactivation: testing(action.deactivation.as_ref())
                .map(|condition| cage_condition(decoder, condition)),
            blocks: action
                .blocks
                .iter()
                .map(|block| BlockActivity {
                    condition: cage_condition(decoder, &block.condition),
                    on_enter: block_effect(block.on_enter),
                    on_exit: block_effect(block.on_exit),
                })
                .collect(),
            regen_off: action
                .stamina
                .regen_off
                .as_ref()
                .map(|condition| cage_condition(decoder, condition)),
            stamina_reset: action
                .stamina
                .reset_on_exit
                .as_ref()
                .map(|condition| cage_condition(decoder, condition)),
            grit_on: action
                .stamina
                .grit_on
                .as_ref()
                .map(|condition| cage_condition(decoder, condition)),
            fragment_cases,
        }
    }

    /// The entries of `fragment` in the animation database `adb`, with their procedural clips,
    /// and their root motion when the action's animation `moves` the body.
    fn variants(
        &mut self,
        decoder: &mut TreeDecoder,
        fragment: &str,
        adb: Option<(&str, &AnimDatabase)>,
        moves: bool,
    ) -> Vec<FragmentVariant> {
        let mut variants = Vec::new();
        if let Some((adb_path, clips)) = adb {
            for procedural in &clips.fragments {
                if procedural.name != fragment {
                    continue;
                }
                let length = fragment_length(procedural, self.lengths).unwrap_or_else(|_| {
                    *self
                        .unknown_fragments
                        .entry(adb_path.to_owned())
                        .or_default() += 1;
                    FragmentLength::Unknown
                });
                let root_motion = moves
                    .then(|| self.roots.motion(procedural, &length))
                    .flatten();
                variants.push(FragmentVariant {
                    tags: procedural
                        .tags
                        .iter()
                        .map(|tag| decoder.intern(tag))
                        .collect(),
                    frag_tags: procedural
                        .frag_tags
                        .iter()
                        .map(|tag| decoder.intern(tag))
                        .collect(),
                    length,
                    windows: procedural
                        .clips
                        .iter()
                        .map(|clip| ProcWindow {
                            kind: decoder.intern(&clip.kind),
                            start_s: clip.start_s as f32,
                            end_s: clip.end_s.map(|end| end as f32),
                            params: clip
                                .params
                                .iter()
                                .map(|(name, value)| (decoder.intern(name), decoder.intern(value)))
                                .collect(),
                        })
                        .collect(),
                    root_motion,
                });
            }
        }
        variants
    }
}

#[cfg(test)]
mod tests {
    use nw_ai::classes::{CLASSES, Class};

    use super::*;

    /// 46 frames at 1/30 s: `boar_gore.caf` (build 25810808).
    const GORE_FRAMES: i32 = 46;
    const SECONDS_PER_FRAME: f32 = 1.0 / 30.0;
    const GORE_SPEED: f64 = 0.85;
    const CAF_HEADER_LEN: usize = 16;
    const MOTION_PARAMS_LEN: usize = 24;

    /// A CAF with one chunk, the `MotionParams` of a clip of `frames` frames.
    fn caf(frames: i32) -> Vec<u8> {
        let mut bytes = CAF_MAGIC.to_vec();
        bytes.extend_from_slice(&0x746_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&(CAF_HEADER_LEN as u32).to_le_bytes());
        let motion_at = (CAF_HEADER_LEN + CAF_CHUNK_ENTRY_LEN) as u32;
        bytes.extend_from_slice(&CAF_MOTION_PARAMS.to_le_bytes());
        bytes.extend_from_slice(&0x925_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&(MOTION_PARAMS_LEN as u32).to_le_bytes());
        bytes.extend_from_slice(&motion_at.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&SECONDS_PER_FRAME.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&frames.to_le_bytes());
        bytes
    }

    fn lengths(entries: &[(&str, ClipLength)]) -> ClipLengths {
        ClipLengths::of(entries)
    }

    /// The one entry of a database whose base animation layer is `layer` (XML).
    fn entry(layer: &str) -> Fragment {
        let adb = format!(
            "<AnimDB><FragmentList><Attack><Fragment Tags=\"\"><AnimLayer>{layer}</AnimLayer>\
             </Fragment></Attack></FragmentList></AnimDB>"
        );
        let mut database = AnimDatabase::parse(&adb).expect("adb");
        database.fragments.remove(0)
    }

    #[test]
    fn a_caf_length_is_its_frames_times_the_frame_time() {
        let seconds = caf_seconds(&caf(GORE_FRAMES)).expect("length");
        assert!((seconds - f64::from(GORE_FRAMES) * f64::from(SECONDS_PER_FRAME)).abs() < 1e-6);
        // The corruption grunt's pain `Fidget`: 150 frames, f32 5.0000005 s (not 5.00000026).
        const PAIN_FRAMES: i32 = 150;
        const PAIN_F32_S: f32 = 5.000_000_5;
        assert_eq!(caf_seconds(&caf(PAIN_FRAMES)), Some(f64::from(PAIN_F32_S)));
        let mut other = caf(GORE_FRAMES);
        other[0] = b'X';
        assert_eq!(caf_seconds(&other), None);
        assert_eq!(caf_seconds(&caf(GORE_FRAMES)[..CAF_HEADER_LEN]), None);
    }

    #[test]
    fn a_fragment_plays_its_clips_by_exit_time_speed_and_loop() {
        let gore_s = f64::from(GORE_FRAMES) * f64::from(SECONDS_PER_FRAME);
        let known = lengths(&[
            ("boar_gore", Ok(gore_s)),
            ("loop", Ok(1.0)),
            ("bad", Err("x".into())),
        ]);
        let one = entry(&format!(
            r#"<Blend ExitTime="0"/><Animation name="Boar_Gore" speed="{GORE_SPEED}"/>"#
        ));
        let FragmentLength::Seconds(seconds) = fragment_length(&one, &known).expect("length")
        else {
            panic!("seconds");
        };
        assert!((f64::from(seconds) - gore_s / GORE_SPEED).abs() < 1e-5);
        let cut = entry(
            r#"<Blend ExitTime="0"/><Animation name="boar_gore"/><Blend ExitTime="2"/><Animation name=""/>"#,
        );
        assert_eq!(
            fragment_length(&cut, &known),
            Ok(FragmentLength::Seconds(2.0)),
            "an empty clip ends it"
        );
        let chained = entry(
            r#"<Blend ExitTime="0"/><Animation name="loop"/><Blend ExitTime="-1"/><Animation name="loop"/>"#,
        );
        assert_eq!(
            fragment_length(&chained, &known),
            Ok(FragmentLength::Seconds(2.0)),
            "a chained clip starts where the one before ends"
        );
        let from_start = entry(
            r#"<Blend ExitTime="0.5" StartTime="0.25"/><Animation name="loop" speed="0.5"/>"#,
        );
        assert_eq!(
            fragment_length(&from_start, &known),
            Ok(FragmentLength::Seconds(2.0)),
            "0.5 s, then 0.75 s of the clip at half speed"
        );
        let outro = entry(
            r#"<Blend ExitTime="0"/><Animation name="loop" flags="Loop"/><Blend ExitTime="0.5"/><Animation name=""/><Blend ExitTime="1"/><Animation name=""/>"#,
        );
        assert_eq!(
            fragment_length(&outro, &known),
            Ok(FragmentLength::Seconds(1.5)),
            "the last empty clip ends it (the Imhotep `Outro` shape)"
        );
        let looping = entry(r#"<Blend ExitTime="0"/><Animation name="loop" flags="Loop"/>"#);
        assert_eq!(fragment_length(&looping, &known), Ok(FragmentLength::Loops));
        let after_loop = entry(
            r#"<Blend ExitTime="0"/><Animation name="loop" flags="Loop"/><Blend ExitTime="-1"/><Animation name="loop"/>"#,
        );
        assert!(
            fragment_length(&after_loop, &known).is_err(),
            "a chained clip after a loop has no start"
        );
        let bad_first = entry(
            r#"<Blend ExitTime="0"/><Animation name="bad"/><Blend ExitTime="1"/><Animation name="loop"/>"#,
        );
        assert_eq!(
            fragment_length(&bad_first, &known),
            Ok(FragmentLength::Seconds(2.0)),
            "a clip that neither ends the entry nor leads a chained one needs no length"
        );
        assert!(
            fragment_length(
                &entry(r#"<Blend ExitTime="0"/><Animation name="bad"/>"#),
                &known
            )
            .is_err()
        );
        assert_eq!(
            fragment_length(
                &entry(r#"<Blend ExitTime="0"/><Animation name=""/>"#),
                &known
            ),
            Ok(FragmentLength::Seconds(0.0)),
            "a base layer of only an empty clip ends at once"
        );
        let silent = entry(
            r#"<Blend ExitTime="0.5"/><Animation name=""/><Blend ExitTime="0.25"/><Animation name=""/>"#,
        );
        assert_eq!(
            fragment_length(&silent, &known),
            Ok(FragmentLength::Seconds(0.75)),
            "it ends where its last empty clip starts"
        );
        assert!(
            fragment_length(&entry(""), &known).is_err(),
            "an empty AnimLayer"
        );
        let mut no_layer = entry(r#"<Blend ExitTime="0"/><Animation name="loop"/>"#);
        no_layer.anim_layers.clear();
        assert!(fragment_length(&no_layer, &known).is_err());
        assert_eq!(
            base_clip_names([&AnimDatabase {
                fragments: vec![one, cut],
            }]),
            BTreeSet::from(["boar_gore".to_owned()])
        );
    }

    /// The skeletons' turn blend space (`mc_broken_combat_turn_blend_comb.comb` and its `_l`
    /// and `_r` blend spaces, build 25810808, trimmed): an idle at 0 and the 90 and 180 degree
    /// turn clips of each side, of different lengths.
    #[test]
    fn a_turn_blend_space_of_clips_of_different_lengths_ends_by_the_turn() {
        const IDLE_FRAMES: i32 = 76;
        const QUARTER_FRAMES: i32 = 40;
        const HALF_LEFT_FRAMES: i32 = 60;
        const HALF_RIGHT_FRAMES: i32 = 66;
        const TURN_SPEED: f64 = 2.0;
        const QUARTER_RAD: f32 = 1.575;
        const HALF_RAD: f32 = 3.150_000_1;
        const COMB: &str = r#"<CombinedBlendSpace>
 <Dimensions><Param name="DesiredFacing" ChooseBlendSpace="1" locked="1"/></Dimensions>
 <AdditionalExtraction><Param name="MoveSpeed"/></AdditionalExtraction>
 <BlendSpaces>
  <BlendSpace AName="animations/Broken/BlendSpaces/turn_l.bspace"/>
  <BlendSpace AName="animations/Broken/BlendSpaces/turn_r.bspace"/>
 </BlendSpaces>
</CombinedBlendSpace>"#;
        const LEFT: &str = r#"<ParaGroup>
 <Dimensions><Param Name="DesiredFacing" Min="0" Max="3.1400001" Cells="27" locked="1"/></Dimensions>
 <AdditionalExtraction><Param name="MoveSpeed"/></AdditionalExtraction>
 <ExampleList>
  <Example SetPara0="0" AName="idle"/>
  <Example SetPara0="1.575" AName="turn90_l"/>
  <Example SetPara0="3.1500001" AName="turn180_l"/>
 </ExampleList>
</ParaGroup>"#;
        const RIGHT: &str = r#"<ParaGroup>
 <Dimensions><Param Name="DesiredFacing" Min="-3.1400001" Max="0" Cells="27" locked="1"/></Dimensions>
 <ExampleList>
  <Example SetPara0="-3.1500001" AName="turn180_r"/>
  <Example SetPara0="-1.575" AName="turn90_r"/>
  <Example SetPara0="0" AName="idle"/>
 </ExampleList>
</ParaGroup>"#;
        /// A blend space on another parameter (the walk blend spaces: `MoveSpeed`).
        const SPEED: &str = r#"<ParaGroup>
 <Dimensions><Param Name="MoveSpeed" Min="0" Max="4"/></Dimensions>
 <ExampleList>
  <Example SetPara0="0" AName="idle"/>
  <Example SetPara0="4" AName="turn90_l"/>
 </ExampleList>
</ParaGroup>"#;
        let file = |name: &str, bytes: Vec<u8>| (name.to_owned(), Ok(bytes));
        let raw: HashMap<String, Result<Vec<u8>, String>> = HashMap::from([
            file("idle", caf(IDLE_FRAMES)),
            file("turn90_l", caf(QUARTER_FRAMES)),
            file("turn90_r", caf(QUARTER_FRAMES)),
            file("turn180_l", caf(HALF_LEFT_FRAMES)),
            file("turn180_r", caf(HALF_RIGHT_FRAMES)),
            file("turn_l", LEFT.as_bytes().to_vec()),
            file("turn_r", RIGHT.as_bytes().to_vec()),
            file("turn_comb", COMB.as_bytes().to_vec()),
            file("speed", SPEED.as_bytes().to_vec()),
        ]);
        let known = ClipLengths::resolved(&raw);
        assert!(known.get("turn_comb").is_err(), "no one length");
        let turn = entry(&format!(
            r#"<Blend ExitTime="0"/><Animation name="Turn_Comb" speed="{TURN_SPEED}"/>"#
        ));
        let FragmentLength::ByFacing(examples) = fragment_length(&turn, &known).expect("length")
        else {
            panic!("by facing");
        };
        let seconds =
            |frames: i32| (f64::from(frames) * f64::from(SECONDS_PER_FRAME) / TURN_SPEED) as f32;
        let expected = [
            (-HALF_RAD, seconds(HALF_RIGHT_FRAMES)),
            (-QUARTER_RAD, seconds(QUARTER_FRAMES)),
            (0.0, seconds(IDLE_FRAMES)),
            (QUARTER_RAD, seconds(QUARTER_FRAMES)),
            (HALF_RAD, seconds(HALF_LEFT_FRAMES)),
        ];
        assert_eq!(examples.len(), expected.len(), "{examples:?}");
        for (example, (facing_rad, length_s)) in examples.iter().zip(expected) {
            assert!(
                (example.facing_rad - facing_rad).abs() < 1e-6,
                "{example:?}"
            );
            assert!((example.seconds - length_s).abs() < 1e-5, "{example:?}");
        }
        let walk = entry(r#"<Blend ExitTime="0"/><Animation name="speed"/>"#);
        assert!(
            fragment_length(&walk, &known).is_err(),
            "a blend on another parameter stays Unknown"
        );
    }

    #[test]
    fn names_and_paths_are_the_paks_form() {
        assert_eq!(base_name("animations/X/Boar_Gore.caf"), "boar_gore");
        assert_eq!(base_name("a\\b\\turn_comb.comb"), "turn_comb");
        assert_eq!(base_name("boar_combat_idle_turn"), "boar_combat_idle_turn");
    }

    /// The boar's `Attack_Gore` action (`boar_combat.actionlist`, build 25810808).
    const GORE_LIST: &str = r#"<ActionList><Actions>
<Action Name="Attack_Gore" FragmentName="Attack_Gore" Predictive="true">
 <ActivationCondition><IfHTNCAGEAction ActionName="Attack_Gore_CAGE_Task"/></ActivationCondition>
 <Activities>
  <HTNPlanDoneActivity>
   <Default InvalidatePlanOnExit="false" CompletionFragmentOffset="0.4"/>
   <Conditional Name="ChainAttack" CompletionFragmentOffset="0.8"/>
  </HTNPlanDoneActivity>
  <AnimationDrivenMotion><Default MotionMultiplier="1.00"/></AnimationDrivenMotion>
  <SetDesiredFacingBlendParam><Default FacingMode="AngleToTarget"/></SetDesiredFacingBlendParam>
  <SetMannequinTag ResetWhenReinstalled="true"><Default TagName="Alerted" OnEnter="Set" OnExit="NoEffect" DelayTimer="0.10"/></SetMannequinTag>
 </Activities>
</Action>
<Action Name="Reaction" FragmentName="r_R2_F"><Activities/></Action>
</Actions>
<Conditions><IfMannequinTag Name="ChainAttack" TagName="ChainAttack"/></Conditions>
</ActionList>"#;

    #[test]
    fn a_mob_block_list_keeps_unset_tags_its_block_and_which_actions_the_engine_starts() {
        // Retail `damned_block.actionlist` (build 25810808), trimmed. The mob lists write
        // `Unset`; the old reader knew only `Clear`, so no mob tag was ever unset.
        let text = r#"<ActionList><Actions>
 <Action Name="Block_On" FragmentName="Block_Start">
  <ActivationCondition/>
  <Activities><ActivateBlock><Default OnEnter="ActivateOffHandWeapon" OnExit="Deactivate"/></ActivateBlock>
   <SetMannequinTag ResetWhenReinstalled="true"><Default TagName="Block" OnEnter="NoEffect" OnExit="Set" DelayTimer="0.0"/></SetMannequinTag></Activities>
 </Action>
 <Action Name="Block" FragmentName="">
  <ActivationCondition><IfMannequinTag TagName="Alerted"/></ActivationCondition>
  <Activities><SetMannequinTag ResetWhenReinstalled="true"><Default TagName="Block" OnEnter="Set" OnExit="Unset" DelayTimer="0.0"/></SetMannequinTag></Activities>
 </Action>
</Actions></ActionList>"#;
        let parsed = list_actions(text).expect("list");
        let [block_on, block] = &parsed.actions[..] else {
            panic!("two actions: {:?}", parsed.actions);
        };
        assert_eq!(tag_effect(block.tags[0].1.on_exit), TagEffect::Clear);
        assert_eq!(block_on.blocks.len(), 1);
        assert_eq!(
            block_effect(block_on.blocks[0].on_enter),
            BlockEffect::Raise
        );
        assert_eq!(
            testing(block_on.activation.as_ref()),
            None,
            "an empty condition"
        );
        assert!(testing(block.activation.as_ref()).is_some());
    }

    #[test]
    fn a_stagger_list_keeps_its_engine_grit_action_without_a_fragment_and_drops_a_task_one() {
        // Retail `baseai_stagger.actionlist` (build 25810808), trimmed, and a made-up task
        // action without a fragment.
        let text = r#"<ActionList><Actions>
 <Action Name="ActivateGrit">
  <ActivationCondition><And><IfHasStatusEffect StatusEffect="AI_Enable_Stagger"/><Not><IfFragmentPlaying UseHostActionFragment="false" FragmentName="r_Stagger"/></Not></And></ActivationCondition>
  <DeactivationCondition><Or><IfFragmentPlaying UseHostActionFragment="false" FragmentName="r_Stagger"/></Or></DeactivationCondition>
  <Activities><ActivateGrit><Default OnEnter="Activate" OnExit="Deactivate" DamageTableRow=""/></ActivateGrit></Activities>
 </Action>
 <Action Name="Fragmentless_Task">
  <ActivationCondition><IfBehaviorTreeTask ActionName="Fragmentless_CAGE_Task"/></ActivationCondition>
 </Action>
</Actions></ActionList>"#;
        let parsed = list_actions(text).expect("list");
        let [grit] = &parsed.actions[..] else {
            panic!("only the engine action: {:?}", parsed.actions);
        };
        assert_eq!(
            (grit.name.as_str(), grit.fragment.as_str()),
            ("ActivateGrit", "")
        );
        assert_eq!(grit.stamina.grit_on, Some(Condition::True));
        let mut decoder = TreeDecoder::default();
        let stagger = decoder.intern("r_Stagger");
        let deactivation = cage_condition(
            &mut decoder,
            grit.deactivation.as_ref().expect("a deactivation"),
        );
        let mob = |playing: bool| {
            deactivation.holds_with(&nw_ai::data::MobCage {
                has_tag: &|_| false,
                has_marker: &|_| false,
                has_effect: &|_| false,
                plays_fragment: &|fragment| playing && fragment == stagger,
                hit_angle_deg: None,
            })
        };
        assert!(mob(true), "the playing stagger stops the grit");
        assert!(!mob(false));
    }

    #[test]
    fn a_stagger_keeps_its_front_and_back_fragments_and_their_hit_sides() {
        // Retail `baseai_stagger.actionlist` (build 25810808), trimmed.
        let text = r#"<ActionList><Actions>
 <Action Name="Reaction_Staggered" FragmentName="r_Stagger" ServerStarted="true">
  <ActivationCondition><IfStaminaValue ComparisonValue="0.0"/></ActivationCondition>
  <Activities><ChangeActionFragment ResetWhenReinstalled="true">
   <Default UseDefaultFragment="true" FragmentName=""/>
   <Conditional Name="damaged_front" UseDefaultFragment="false" FragmentName="r_Stagger_F"/>
   <Conditional Name="damaged_back" UseDefaultFragment="false" FragmentName="r_Stagger_B"/>
  </ChangeActionFragment></Activities>
 </Action>
</Actions><Conditions>
 <Or Name="damaged_front"><IfDamagedByAngle Max="90.000000" Min="0.000000" Tolerance="0.010000"/><IfDamagedByAngle Max="0.000000" Min="-90.000000" Tolerance="0.010000"/></Or>
 <Or Name="damaged_back"><IfDamagedByAngle Max="180.000000" Min="90.000000" Tolerance="0.010000"/><IfDamagedByAngle Max="-90.000000" Min="-180.000000" Tolerance="0.010000"/></Or>
</Conditions></ActionList>"#;
        let parsed = list_actions(text).expect("list");
        let [stagger] = &parsed.actions[..] else {
            panic!("one action: {:?}", parsed.actions);
        };
        let fragments: Vec<&str> = stagger
            .fragment_cases
            .iter()
            .map(|case| case.fragment.as_str())
            .collect();
        assert_eq!(fragments, ["r_Stagger_F", "r_Stagger_B"]);
        let mut decoder = TreeDecoder::default();
        let sides: Vec<CageCondition> = stagger
            .fragment_cases
            .iter()
            .map(|case| cage_condition(&mut decoder, &case.condition))
            .collect();
        let holds = |side: &CageCondition, angle: f32| {
            side.holds_with(&nw_ai::data::MobCage {
                has_tag: &|_| false,
                has_marker: &|_| false,
                has_effect: &|_| false,
                plays_fragment: &|_| false,
                hit_angle_deg: Some(angle),
            })
        };
        assert!(holds(&sides[0], -45.0) && !holds(&sides[1], -45.0));
        assert!(holds(&sides[1], 170.0) && !holds(&sides[0], 170.0));
    }

    /// The test skeleton's clips, `swing` (its root goes 1 m forward over 30 frames of 1/30 s)
    /// and `still` (the same frames, its root stays), on its skeleton of root `Xform`.
    fn root_clips() -> RootClips {
        let bodies = crate::melee_shapes::test_support::bodies();
        let swing = bodies.clips[crate::melee_shapes::test_support::SWING]
            .clone()
            .expect("the swing clip");
        let mut still = swing.clone();
        for track in still.tracks.values_mut() {
            track
                .positions
                .iter_mut()
                .for_each(|(_, at)| *at = [0.0; 3]);
        }
        let root = bodies.player.joints[0].controller_id;
        RootClips {
            clips: HashMap::from([
                (crate::melee_shapes::test_support::SWING.to_owned(), swing),
                (STILL_CLIP.to_owned(), still),
            ]),
            roots: HashMap::from([(root, bodies.player)]),
        }
    }

    /// A clip whose root stays where it starts.
    const STILL_CLIP: &str = "still";
    /// The test clip's length and the frame time of its root path.
    const SWING_S: f64 = 1.0;
    const SWING_STEP_MS: f32 = 1_000.0 / 30.0;

    #[test]
    fn a_moving_actions_entry_gets_the_root_motion_of_its_main_animation() {
        let parsed = list_actions(GORE_LIST).expect("list");
        let adb = AnimDatabase::parse(
            "<AnimDB><FragmentList>\
             <Attack_Gore><Fragment Tags=\"\"><AnimLayer><Blend ExitTime=\"0\"/>\
             <Animation name=\"swing\"/></AnimLayer></Fragment>\
             <Fragment Tags=\"Armadillo\"><AnimLayer><Blend ExitTime=\"0\"/>\
             <Animation name=\"still\"/></AnimLayer></Fragment>\
             <Fragment Tags=\"Loop\"><AnimLayer><Blend ExitTime=\"0\"/>\
             <Animation name=\"swing\" flags=\"Loop\"/></AnimLayer></Fragment></Attack_Gore>\
             <r_R2_F><Fragment Tags=\"\"><AnimLayer><Blend ExitTime=\"0\"/>\
             <Animation name=\"swing\"/></AnimLayer></Fragment></r_R2_F>\
             </FragmentList></AnimDB>",
        )
        .expect("adb");
        let known = lengths(&[
            (crate::melee_shapes::test_support::SWING, Ok(SWING_S)),
            (STILL_CLIP, Ok(SWING_S)),
        ]);
        let roots = root_clips();
        let lists = HashMap::new();
        let mut builder = CageBuilder {
            lists: &lists,
            lengths: &known,
            roots: &roots,
            actions: Vec::new(),
            index: HashMap::new(),
            unknown_fragments: BTreeMap::new(),
        };
        let mut decoder = TreeDecoder::default();
        let list = decoder.intern("gore.actionlist");
        let database = Some(("gore.adb", &adb));
        let gore = builder.action(
            &mut decoder,
            list,
            &parsed.actions[0],
            &parsed.conditions,
            database,
            database,
        );
        assert_eq!(gore.motion_multiplier, Some(1.0), "the multiplier is kept");
        let motion = gore.variants[0]
            .root_motion
            .as_ref()
            .expect("the swing moves");
        assert!(
            (motion.step_ms - SWING_STEP_MS).abs() < 1e-3,
            "{}",
            motion.step_ms
        );
        assert_eq!(motion.offsets.len(), 31, "an offset per frame, both ends");
        let end = motion.offsets.last().expect("an end");
        assert!(
            end[0].abs() < 1e-4 && (end[1] - 1.0).abs() < 1e-4,
            "{end:?}"
        );
        assert_eq!(gore.variants[1].root_motion, None, "a root that stays");
        assert_eq!(gore.variants[2].root_motion, None, "a loop");
        assert!(
            gore.weapon_variants
                .iter()
                .all(|variant| variant.root_motion.is_none()),
            "a weapon's entry moves no body"
        );
        let reaction = builder.action(
            &mut decoder,
            list,
            &parsed.actions[1],
            &parsed.conditions,
            database,
            None,
        );
        assert_eq!(reaction.motion_multiplier, None);
        assert_eq!(
            reaction.variants[0].root_motion, None,
            "no multiplier: no root motion"
        );
    }

    #[test]
    fn an_action_list_gives_tasks_completion_motion_and_tags() {
        let parsed = list_actions(GORE_LIST).expect("list");
        let gore = &parsed.actions[0];
        assert_eq!(gore.tasks, ["Attack_Gore_CAGE_Task"]);
        assert_eq!(
            gore.completion,
            Some((0.4, vec![("ChainAttack".to_owned(), 0.8)]))
        );
        assert_eq!(gore.motion_multiplier, Some(1.0));
        assert_eq!(gore.facing, Some(FacingMode::AngleToTarget));
        assert_eq!(parsed.actions[1].facing, None);
        assert_eq!(
            gore.tags,
            [(
                Condition::True,
                TagCase {
                    tag: "Alerted".to_owned(),
                    on_enter: TagChange::Set,
                    on_exit: TagChange::Keep,
                    delay_ms: 100,
                }
            )]
        );
        assert!(
            parsed.actions[1].tasks.is_empty(),
            "the engine starts reactions"
        );
        assert_eq!(parsed.conditions[0].0, "ChainAttack");
        let mut decoder = TreeDecoder::default();
        let condition = cage_condition(&mut decoder, &parsed.conditions[0].1);
        assert_eq!(condition, CageCondition::Tag(decoder.intern("ChainAttack")));
    }

    #[test]
    fn a_damage_table_of_a_slice_is_a_datasheet_in_the_paks() {
        assert_eq!(
            datasheet_path("sharedassets/datatables/javelindata_damagetable_boar.xml"),
            "sharedassets/datatables/javelindata_damagetable_boar.datasheet"
        );
        assert_eq!(datasheet_path("other.datasheet"), "other.datasheet");
    }

    #[test]
    fn a_grid_names_its_lists_and_aliases() {
        let grid = r#"<Grid><ActionLists>
<ActionList Path="SharedAssets\SpringboardEntitites\Characters\BaseAI\CAGE\BaseAI_Stun.actionlist"><AliasMap><Action ActionName="Stun" Alias="BehaviorTreeTask"/></AliasMap></ActionList>
<ActionList Path="SharedAssets/Boar_Reaction.actionlist"><AliasMap><Action ActionName="Reaction" Alias="Reaction"/></AliasMap></ActionList>
</ActionLists></Grid>"#;
        let lists = grid_lists(grid).expect("grid");
        assert_eq!(
            lists.paths,
            [
                "sharedassets/springboardentitites/characters/baseai/cage/baseai_stun.actionlist",
                "sharedassets/boar_reaction.actionlist"
            ]
        );
        assert_eq!(
            lists.aliases[1],
            ("Reaction".to_owned(), "Reaction".to_owned())
        );
    }

    #[test]
    fn an_animation_database_gives_each_entry_its_base_layer_and_clips() {
        let adb = r#"<AnimDB><FragmentList><Attack_Gore>
<Fragment BlendOutDuration="0.2" Tags=""><AnimLayer><Blend ExitTime="0" StartTime="0" Duration="0.1"/><Animation name="boar_gore" speed="0.85000002"/></AnimLayer>
<ProcLayer><Blend ExitTime="0.69999999" StartTime="0" Duration="0"/><Procedural type="CAGE-Damage"><ProceduralParams><DamageTableRow value="Attack_Gore"/></ProceduralParams></Procedural><Blend ExitTime="0.19999999"/><Procedural type=""/></ProcLayer>
</Fragment>
<Fragment Tags="Armadillo"><AnimLayer><Blend ExitTime="0"/><Animation name="armadillo_idle" flags="Loop"/></AnimLayer></Fragment>
</Attack_Gore></FragmentList></AnimDB>"#;
        let clips = AnimDatabase::parse(adb).expect("clips");
        assert_eq!(clips.fragments.len(), 2);
        let base = &clips.fragments[0].anim_layers[0][0];
        assert_eq!(base.name, "boar_gore");
        assert!((base.speed - GORE_SPEED).abs() < 1e-6);
        assert!(clips.fragments[1].anim_layers[0][0].looped);
        assert_eq!(clips.fragments[1].tags, ["Armadillo"]);
        let damage = &clips.fragments[0].clips[0];
        assert!((damage.start_s - 0.7).abs() < 1e-6);
        assert_eq!(damage.param("DamageTableRow"), Some("Attack_Gore"));
    }

    fn uuid_of(class: Class) -> [u8; 16] {
        CLASSES
            .iter()
            .find(|(known, _)| *known == class)
            .map(|(_, uuid)| *uuid)
            .expect("class")
    }

    fn typed(name: &str, class: Class, value: Vec<u8>, children: Vec<Element>) -> Element {
        Element {
            name_crc: Some(crc(name)),
            version: None,
            type_id: uuid_of(class),
            value,
            children,
        }
    }

    fn float(name: &str, value: f32) -> Element {
        typed(name, Class::Float, value.to_be_bytes().to_vec(), Vec::new())
    }

    fn text(name: &str, value: &str) -> Element {
        typed(name, Class::String, value.as_bytes().to_vec(), Vec::new())
    }

    /// An `FxScriptComponent`'s lists as the bear slice holds them: the component's base class >
    /// `m_scriptDataContainer` > element > `m_scriptName`.
    fn fx_lists(names: &[&str]) -> Element {
        let entries = names
            .iter()
            .map(|name| {
                typed(
                    "element",
                    Class::Node,
                    Vec::new(),
                    vec![text(FX_SCRIPT_NAME, name)],
                )
            })
            .collect();
        typed(
            BASE_CLASS,
            Class::Node,
            Vec::new(),
            vec![typed(FX_SCRIPT_CONTAINER, Class::Node, Vec::new(), entries)],
        )
    }

    /// A slice whose FX lists are the plain status effect list and a kind's own (the Inquisitor
    /// warden's: `fxscriptlist_npc_statuseffects`, `fxscriptlist_npc`) is not a large body; one
    /// that also names the large list in a second component is.
    #[test]
    fn a_slice_is_a_large_body_only_when_an_fx_script_component_names_the_large_list() {
        let read = |components: Vec<Element>| {
            let root = typed("root", Class::Node, Vec::new(), components);
            SliceAi::read(&root, &mut TreeDecoder::default(), "warden")
                .expect("slice")
                .large_status_effects_fx
        };
        const PLAIN: &str = "fxscriptlist_npc_statuseffects";
        const OWN: &str = "fxscriptlist_npc";
        assert!(!read(vec![fx_lists(&[PLAIN, OWN])]));
        assert!(!read(Vec::new()), "no FX script component");
        assert!(read(vec![
            fx_lists(&[OWN]),
            fx_lists(&[PLAIN, LARGE_STATUS_EFFECTS_FX])
        ]));
    }

    fn asset(name: &str, hint: &str) -> Element {
        let mut value = vec![1; nw_ai::decode::ASSET_HINT_LEN_AT];
        value.extend_from_slice(&(hint.len() as u64).to_be_bytes());
        value.extend_from_slice(hint.as_bytes());
        typed(name, Class::Asset, value, Vec::new())
    }

    fn position(name: &str) -> Element {
        typed(
            name,
            Class::PositionType,
            Vec::new(),
            vec![typed(
                "PositionType",
                Class::Int,
                0_i32.to_be_bytes().to_vec(),
                Vec::new(),
            )],
        )
    }

    /// The boar's `RunToTarget` config (slice `boar.dynamicslice`).
    fn run_config() -> Element {
        let mut select = vec![position("Origin"), position("Anchor")];
        for name in [
            "Direction",
            "PositionSelectMode",
            "MaxAttempts",
            "MaxAttemptsParam",
        ] {
            select.push(typed(
                name,
                Class::Int,
                0_i32.to_be_bytes().to_vec(),
                Vec::new(),
            ));
        }
        for name in ["CheckPathToOrigin", "CheckPathToAnchor", "FavorMiddleAngle"] {
            select.push(typed(name, Class::Bool, vec![0], Vec::new()));
        }
        for name in [
            "StartAngle",
            "StopAngle",
            "RangeMin",
            "RangeMax",
            "DistanceFromWall",
            "QueryRadius",
            "StartAngleParam",
            "StopAngleParam",
            "RangeMinParam",
            "RangeMaxParam",
            "DistanceFromWallParam",
            "QueryRadiusParam",
            "GroupRange",
        ] {
            select.push(float(name, 1.5));
        }
        typed(
            "element",
            Class::Node,
            Vec::new(),
            vec![
                text(NAV_CONFIG_TYPE, "RunToTarget"),
                float(NAV_TURN_RATE, 120.0),
                float(NAV_ARRIVAL, 0.5),
                float(NAV_MIN_SPEED, 2.0),
                float(NAV_MAX_SPEED, 7.0),
                float(NAV_ACCELERATION, 30.0),
                float(NAV_DECELERATION, 30.0),
                float(NAV_REPATH, 0.5),
                typed(NAV_SELECT_POSITION, Class::Parameters, Vec::new(), select),
                typed(
                    NAV_ACTION_PARAMS,
                    Class::Node,
                    Vec::new(),
                    vec![float("m_agentRadius", 0.5)],
                ),
                typed(NAV_TELEPORT, Class::Bool, vec![0], Vec::new()),
                text(NAV_TURN_ACTION, "Combat_Turn_ToNav_CAGE_Task"),
                text(NAV_START_ACTION, ""),
                typed(
                    NAV_START_RULE,
                    Class::Int,
                    1_i32.to_be_bytes().to_vec(),
                    Vec::new(),
                ),
            ],
        )
    }

    #[test]
    fn a_slice_gives_its_tree_config_navigation_and_files() {
        let any = |value: Element| typed(CONFIG_VALUE, Class::Any, Vec::new(), vec![value]);
        let config = typed(
            CONFIG_PARAMS,
            Class::Node,
            Vec::new(),
            vec![typed(
                "element",
                Class::Node,
                Vec::new(),
                vec![
                    text(CONFIG_NAME, "CombatLeashDistance"),
                    any(float("m_data", 70.0)),
                ],
            )],
        );
        let trees = typed(
            TREE_ASSETS,
            Class::Node,
            Vec::new(),
            vec![typed(
                "element",
                Class::Node,
                Vec::new(),
                vec![asset(
                    ASSET_REF,
                    "SharedAssets/AI/BehaviorTrees/Predator/Boar/Boar_Base.behaviortree",
                )],
            )],
        );
        let database = typed(
            ANIMATION_DATABASE,
            Class::Node,
            Vec::new(),
            vec![typed(
                "BaseClass1",
                Class::Node,
                Vec::new(),
                vec![text(
                    ASSET_PATH,
                    "Animations/Mannequin/ADB/NPC_Boar_Anims.adb",
                )],
            )],
        );
        let navigation = typed(NAV_CONFIGS, Class::Node, Vec::new(), vec![run_config()]);
        let root = typed(
            "root",
            Class::Node,
            Vec::new(),
            vec![
                config,
                trees,
                database,
                navigation,
                float(MAX_SLOPE, 70.0),
                asset(ACTION_GRID, "SharedAssets/Boar.grid"),
                // As the bear slice holds them: `m_initialStatusEffects` > element > text, and
                // the `GritComponent` flag.
                typed(
                    INITIAL_STATUS_EFFECTS,
                    Class::Node,
                    Vec::new(),
                    vec![typed(
                        "element",
                        Class::Node,
                        Vec::new(),
                        vec![text("m_statusEffectId", "AI_Enable_Stagger")],
                    )],
                ),
                typed(GRIT_USES_STAMINA, Class::Bool, vec![1], Vec::new()),
                fx_lists(&[LARGE_STATUS_EFFECTS_FX]),
            ],
        );
        let mut decoder = TreeDecoder::default();
        let slice = SliceAi::read(&root, &mut decoder, "boar").expect("slice");
        assert_eq!(slice.initial_effects, ["AI_Enable_Stagger"]);
        assert!(slice.grit_uses_stamina);
        assert!(slice.large_status_effects_fx);
        assert_eq!(
            slice.trees,
            ["sharedassets/ai/behaviortrees/predator/boar/boar_base.behaviortree"]
        );
        assert_eq!(
            slice.config,
            [("CombatLeashDistance".to_owned(), Value::Float(70.0))]
        );
        assert_eq!(
            slice.animation_database.as_deref(),
            Some("animations/mannequin/adb/npc_boar_anims.adb")
        );
        assert_eq!(slice.grid.as_deref(), Some("sharedassets/boar.grid"));
        assert_eq!(slice.max_slope_deg, Some(70.0));
        let run = &slice.nav_configs[0];
        assert_eq!(decoder.name(run.kind), "RunToTarget");
        assert_eq!(
            (run.max_speed, run.min_speed, run.turn_rate_deg),
            (7.0, 2.0, 120.0)
        );
        assert_eq!(
            run.turn_action
                .map(|task| decoder.name(task).to_owned())
                .as_deref(),
            Some("Combat_Turn_ToNav_CAGE_Task")
        );
        assert_eq!(run.start_action, None);
        assert_eq!(
            run.action_params,
            [(crc("m_agentRadius"), Value::Float(0.5))]
        );
        assert!(run.select_position.is_some());
    }

    /// A field of no class: its name and its value bytes.
    fn field_of(name: &str, value: Vec<u8>, children: Vec<Element>) -> Element {
        Element {
            name_crc: Some(crc(name)),
            version: None,
            type_id: [0; 16],
            value,
            children,
        }
    }

    /// A component of type `type_id` whose base class holds its replica index `replica`.
    fn component(type_id: [u8; 16], replica: u32, mut fields: Vec<Element>) -> Element {
        let index = Element {
            name_crc: Some(nw_world::level_script::REPLICA_INDEX_NAME),
            ..field_of("", replica.to_be_bytes().to_vec(), Vec::new())
        };
        fields.push(field_of(BASE_CLASS, Vec::new(), vec![index]));
        Element {
            type_id,
            ..field_of("element", Vec::new(), fields)
        }
    }

    /// A damage component as the grenadier bomb's explosion holds it: the radius under its
    /// server facet's `m_shape`, the row in `m_damageData`.
    fn damage_component(table: &str, row: &str, radius_m: f32) -> Element {
        let shape = field_of(
            DAMAGE_SHAPE,
            Vec::new(),
            vec![field_of(
                "element",
                Vec::new(),
                vec![float(SHAPE_RADIUS, radius_m)],
            )],
        );
        component(
            [7; 16],
            0,
            vec![
                field_of("m_serverFacetPtr", Vec::new(), vec![shape]),
                field_of(
                    DAMAGE_DATA,
                    Vec::new(),
                    vec![text(DAMAGE_DATA_TABLE, table), text(DAMAGE_DATA_ENTRY, row)],
                ),
            ],
        )
    }

    /// `drowned_miner_bomb_explosion.dynamicslice` as the paks hold it: ownership on 1, the
    /// position on 2, a delayed event of 5 s on 3, the damage component (not replicated) and an
    /// empty damage component.
    #[test]
    fn an_impact_slice_gives_its_damage_row_radius_and_replicated_states() {
        let root = field_of(
            "root",
            Vec::new(),
            vec![
                component(OWNERSHIP, 1, Vec::new()),
                component(NETWORK_POSITION_TYPE, 2, Vec::new()),
                component(DELAYED_EVENT_TYPE, 3, vec![float(DELAY_SECONDS, 5.0)]),
                damage_component("", "", 0.0),
                damage_component(
                    "UndeadGrenadierDamageTable",
                    "Drowned_Miner_Bomb_Explosion",
                    2.0,
                ),
            ],
        );
        let mut decoder = TreeDecoder::default();
        let impact = projectile_impact(&mut decoder, &root, [9; 16]).expect("an impact");
        assert_eq!(decoder.name(impact.table), "UndeadGrenadierDamageTable");
        assert_eq!(decoder.name(impact.row), "Drowned_Miner_Bomb_Explosion");
        assert_eq!(
            (
                impact.asset,
                impact.radius_m,
                impact.ownership_component,
                impact.position_component
            ),
            ([9; 16], 2.0, Some(1), Some(2))
        );
        assert_eq!(
            impact.delayed_events,
            [DelayedEvent {
                component: 3,
                delay_s: 5.0
            }]
        );
        let harmless = field_of("root", Vec::new(), vec![damage_component("", "", 0.0)]);
        assert!(projectile_impact(&mut decoder, &harmless, [9; 16]).is_none());
    }

    /// An asset reference as `m_spawnOnHitAsset` holds it (Observed in
    /// `drowned_miner_bomb.dynamicslice`): the GUID, then at byte 48 the length of the path hint.
    fn asset_value(name: &str, guid: [u8; 16], hint: &str) -> Element {
        const HINT_LENGTH_AT: usize = 48;
        let mut value = guid.to_vec();
        value.resize(HINT_LENGTH_AT, 0);
        value.extend((hint.len() as u64).to_be_bytes());
        value.extend(hint.as_bytes());
        field_of(name, value, Vec::new())
    }

    /// `drowned_miner_bomb.dynamicslice`: ownership on 1, the projectile on 2, its explosion
    /// named as an editor slice.
    #[test]
    fn a_projectile_slice_gives_its_components_and_its_impact_slice() {
        const EXPLOSION: [u8; 16] = [
            0xf0, 0x65, 0xc8, 0x6b, 0x55, 0xad, 0x5d, 0xd8, 0x87, 0xde, 0x67, 0x0c, 0xec, 0xd7,
            0x70, 0x3f,
        ];
        let root = field_of(
            "root",
            Vec::new(),
            vec![
                component(OWNERSHIP, 1, Vec::new()),
                component(
                    PROJECTILE_COMPONENT_TYPE,
                    2,
                    vec![
                        float(STUCK_LIFETIME, 0.7),
                        asset_value(
                            SPAWN_ON_HIT,
                            EXPLOSION,
                            "slices/items/weapons/drowned_miner_bomb_explosion.slice",
                        ),
                    ],
                ),
            ],
        );
        let (replica, stuck_s, impact) = projectile_component(&root).expect("a projectile");
        assert_eq!((replica, stuck_s), (2, 0.7));
        assert_eq!(replicated(&root, &OWNERSHIP), Some(1));
        assert_eq!(
            impact,
            Some(AssetRef {
                guid: EXPLOSION,
                path: "slices/items/weapons/drowned_miner_bomb_explosion.dynamicslice".to_owned(),
            })
        );
        let plain = field_of(
            "root",
            Vec::new(),
            vec![component(
                PROJECTILE_COMPONENT_TYPE,
                1,
                vec![float(STUCK_LIFETIME, 2.5)],
            )],
        );
        assert_eq!(projectile_component(&plain), Some((1, 2.5, None)));
        assert_eq!(replicated(&plain, &OWNERSHIP), None);
    }

    /// `AI_Undead/Grenadier_Bomb`'s arc settings in the ranged attack database; with
    /// `m_useArcTrajectories` off an attack has none.
    #[test]
    fn a_ranged_attack_arcs_only_with_its_arc_settings_on() {
        let data = |enabled: u8| {
            field_of(
                "value2",
                Vec::new(),
                vec![field_of(
                    ARC_SETTINGS,
                    Vec::new(),
                    vec![
                        field_of(ARC_ENABLED, vec![enabled], Vec::new()),
                        float(ARC_LOB_MIN, 0.0),
                        float(ARC_LOB_MAX, 0.4),
                        float(ARC_GRAVITY_MIN, 10.0),
                        float(ARC_GRAVITY_MAX, 15.0),
                        float(ARC_DISTANCE_MIN, 3.0),
                        float(ARC_DISTANCE_MAX, 4.0),
                    ],
                )],
            )
        };
        assert_eq!(
            arc_trajectory(&data(1)),
            Some(ArcTrajectory {
                lob_height_m: [0.0, 0.4],
                gravity_mps2: [10.0, 15.0],
                distance_m: [3.0, 4.0],
            })
        );
        assert_eq!(arc_trajectory(&data(0)), None);
    }

    #[test]
    fn a_damage_sheet_keeps_its_row_ids_in_sheet_order() {
        let sheet = Datasheet {
            type_name: "DamageData".to_owned(),
            unique_id: "FTUEDamageTableMiner".to_owned(),
            columns: vec![nw_formats::datasheet::Column {
                name: DAMAGE_ID_COLUMN.to_owned(),
                kind: nw_formats::datasheet::ColumnKind::String,
            }],
            rows: vec![
                vec!["Drowned_Miner_Bomb".to_owned()],
                vec!["Drowned_Miner_Bomb_Explosion".to_owned()],
            ],
        };
        let mut decoder = TreeDecoder::default();
        let rows = table_rows(&mut decoder, [DamageSheet::of(sheet)]);
        assert_eq!(rows.len(), 1);
        assert_eq!(decoder.name(rows[0].table), "FTUEDamageTableMiner");
        assert_eq!(
            rows[0]
                .rows
                .iter()
                .map(|row| decoder.name(*row))
                .collect::<Vec<_>>(),
            ["Drowned_Miner_Bomb", "Drowned_Miner_Bomb_Explosion"]
        );
    }
}
