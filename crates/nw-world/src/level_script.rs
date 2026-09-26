// SPDX-License-Identifier: AGPL-3.0-or-later
//! The script slices of an instance level (dungeon2 study, run 15): the game mode's `SlicePath`
//! slice (`slices/dungeon/levelscripts/amrinedungeon.dynamicslice`), placed by a capital of the
//! level's `dungeon_script` layer, and the encounter slices its spawners spawn
//! (`slices/dungeon_eventmanager/encounters/dg_enc_windsward_00_boss.dynamicslice`).
//!
//! Every slice is an ObjectStream whose root holds a slice component with the slice's entities.
//! An entity has an `Id`, a `Name`, a transform component (`m_localTM` relative to `m_parentId`)
//! and the components this reader keeps (type UUIDs from the client's registrations, names from
//! the strings of the functions that use them, `local-data/re-sweep`):
//!
//! - spawners (`PrefabSpawnerComponent`, `PointSpawnerComponent`, `SpawnerComponent`): the slice
//!   or alias asset they spawn and when;
//! - `AIVariantProviderComponent`: the vitals row of a spawner slice's mob;
//! - `EncounterManagerComponent`: an encounter's top stages and its activation;
//! - `EncounterComponent`: one stage (a wave or a phase): its child stages, its spawn timeline
//!   (`SpawnDefinition`), its objectives and its start and end actions;
//! - `DetectionVolumeComponent`: the radius of an encounter's player volume;
//! - `GatherableControllerComponent`: a loot container's loot table.
//!
//! An asset reference (`m_sliceAsset`, `m_aliasAsset`, `m_trackedSlice`) is the asset GUID
//! (16 bytes), its sub id and type, then the hint path as a big-endian `u64` length at byte 48
//! and the text (Observed: 9,821 of 9,821 references of the dungeon slices).

use nw_formats::objectstream::{Element, ObjectStreamError, UUID_SIZE, lowercase_crc32};
use serde::{Deserialize, Serialize};

use crate::slice_data::{Basis, GUID_LEN, IDENTITY, SliceDataError, transform_from_bytes};
use crate::slice_triggers::TriggerEvent;

/// `{48866F1B-791B-4606-A20C-22153D1750FA}` `EncounterManagerComponent`.
pub const ENCOUNTER_MANAGER_TYPE: [u8; UUID_SIZE] = [
    0x48, 0x86, 0x6f, 0x1b, 0x79, 0x1b, 0x46, 0x06, 0xa2, 0x0c, 0x22, 0x15, 0x3d, 0x17, 0x50, 0xfa,
];
/// `{D1BC08CE-E244-4F31-84F4-DBD4D4BED1C8}` `EncounterComponent`.
pub const ENCOUNTER_STAGE_TYPE: [u8; UUID_SIZE] = [
    0xd1, 0xbc, 0x08, 0xce, 0xe2, 0x44, 0x4f, 0x31, 0x84, 0xf4, 0xdb, 0xd4, 0xd4, 0xbe, 0xd1, 0xc8,
];
/// `{B52D0D52-99CD-41BE-B6F7-D2519C15BF14}` `PrefabSpawnerComponent`.
pub const PREFAB_SPAWNER_TYPE: [u8; UUID_SIZE] = [
    0xb5, 0x2d, 0x0d, 0x52, 0x99, 0xcd, 0x41, 0xbe, 0xb6, 0xf7, 0xd2, 0x51, 0x9c, 0x15, 0xbf, 0x14,
];
/// `{D407E673-6F0B-4283-8C30-AE9A396F5822}` `PointSpawnerComponent`.
pub const POINT_SPAWNER_TYPE: [u8; UUID_SIZE] = [
    0xd4, 0x07, 0xe6, 0x73, 0x6f, 0x0b, 0x42, 0x83, 0x8c, 0x30, 0xae, 0x9a, 0x39, 0x6f, 0x58, 0x22,
];
/// `{8B86F1ED-3CA0-4E28-B816-4F5BA3267C13}` `AIVariantProviderComponent`.
pub const VARIANT_PROVIDER_TYPE: [u8; UUID_SIZE] = [
    0x8b, 0x86, 0xf1, 0xed, 0x3c, 0xa0, 0x4e, 0x28, 0xb8, 0x16, 0x4f, 0x5b, 0xa3, 0x26, 0x7c, 0x13,
];
/// `{41BC3BB3-EF30-41E4-B751-12C4EAB97C0E}` `DetectionVolumeComponent`.
pub const DETECTION_VOLUME_TYPE: [u8; UUID_SIZE] = [
    0x41, 0xbc, 0x3b, 0xb3, 0xef, 0x30, 0x41, 0xe4, 0xb7, 0x51, 0x12, 0xc4, 0xea, 0xb9, 0x7c, 0x0e,
];
/// `{0B382AF4-7946-4698-9223-206D1187B612}` `GatherableControllerComponent`.
pub const GATHERABLE_CONTROLLER_TYPE: [u8; UUID_SIZE] = [
    0x0b, 0x38, 0x2a, 0xf4, 0x79, 0x46, 0x46, 0x98, 0x92, 0x23, 0x20, 0x6d, 0x11, 0x87, 0xb6, 0x12,
];
/// Objective classes (`m_objectives` entries).
pub mod objective_type {
    use super::UUID_SIZE;
    /// `{632E9ECE-F80C-48C0-9922-711537743873}` `SliceDestroyedObjective`.
    pub const SLICE_DESTROYED: [u8; UUID_SIZE] = [
        0x63, 0x2e, 0x9e, 0xce, 0xf8, 0x0c, 0x48, 0xc0, 0x99, 0x22, 0x71, 0x15, 0x37, 0x74, 0x38,
        0x73,
    ];
    /// `{EE96E3A8-FA5E-4264-BB76-066EAF740A7E}` `BossPhaseObjective`.
    pub const BOSS_PHASE: [u8; UUID_SIZE] = [
        0xee, 0x96, 0xe3, 0xa8, 0xfa, 0x5e, 0x42, 0x64, 0xbb, 0x76, 0x06, 0x6e, 0xaf, 0x74, 0x0a,
        0x7e,
    ];
    /// `{EAD831AA-836B-4BA5-8E24-4E5BB8119559}` `EncounterEventObjective`.
    pub const ENCOUNTER_EVENT: [u8; UUID_SIZE] = [
        0xea, 0xd8, 0x31, 0xaa, 0x83, 0x6b, 0x4b, 0xa5, 0x8e, 0x24, 0x4e, 0x5b, 0xb8, 0x11, 0x95,
        0x59,
    ];
    /// `{40ACDA82-8B8C-4BCE-83D4-AB03140CA04A}` `EntityActiveStateChangedObjective`.
    pub const ENTITY_ACTIVE_STATE: [u8; UUID_SIZE] = [
        0x40, 0xac, 0xda, 0x82, 0x8b, 0x8c, 0x4b, 0xce, 0x83, 0xd4, 0xab, 0x03, 0x14, 0x0c, 0xa0,
        0x4a,
    ];
    /// `{BFBD6466-7738-4378-B22D-402C00F27AB0}` `WaitForParentObjective`.
    pub const WAIT_FOR_PARENT: [u8; UUID_SIZE] = [
        0xbf, 0xbd, 0x64, 0x66, 0x77, 0x38, 0x43, 0x78, 0xb2, 0x2d, 0x40, 0x2c, 0x00, 0xf2, 0x7a,
        0xb0,
    ];
    /// `{CDDD161A-9A85-4B81-9F7F-18845C6560C9}` `InvasionWaveObjective`.
    pub const INVASION_WAVE: [u8; UUID_SIZE] = [
        0xcd, 0xdd, 0x16, 0x1a, 0x9a, 0x85, 0x4b, 0x81, 0x9f, 0x7f, 0x18, 0x84, 0x5c, 0x65, 0x60,
        0xc9,
    ];
}
/// Action classes (`m_startActions`, `m_endActions` entries).
pub mod action_type {
    use super::UUID_SIZE;
    /// `{1C5389F4-C99E-4362-B225-65AB588A8984}` `EntityEventAction`.
    pub const ENTITY_EVENT: [u8; UUID_SIZE] = [
        0x1c, 0x53, 0x89, 0xf4, 0xc9, 0x9e, 0x43, 0x62, 0xb2, 0x25, 0x65, 0xab, 0x58, 0x8a, 0x89,
        0x84,
    ];
    /// `{8BE430F9-E0E5-4453-80AA-4CFDF64E78F4}` `DestroySliceAction`.
    pub const DESTROY_SLICE: [u8; UUID_SIZE] = [
        0x8b, 0xe4, 0x30, 0xf9, 0xe0, 0xe5, 0x44, 0x53, 0x80, 0xaa, 0x4c, 0xfd, 0xf6, 0x4e, 0x78,
        0xf4,
    ];
    /// `{BA5688E3-0802-428A-B94C-D726B8564ADD}` `KillSpawnsAction`.
    pub const KILL_SPAWNS: [u8; UUID_SIZE] = [
        0xba, 0x56, 0x88, 0xe3, 0x08, 0x02, 0x42, 0x8a, 0xb9, 0x4c, 0xd7, 0x26, 0xb8, 0x56, 0x4a,
        0xdd,
    ];
}

/// Byte of an asset reference where the hint path's length starts (a big-endian `u64`).
const ASSET_HINT_LENGTH_AT: usize = 48;
/// Byte of an asset reference where the hint path starts.
const ASSET_HINT_AT: usize = ASSET_HINT_LENGTH_AT + size_of::<u64>();
/// The suffix of a dynamic slice; an editor slice (`.slice`) is shipped as one.
pub const DYNAMIC_SLICE_SUFFIX: &str = ".dynamicslice";
const EDITOR_SLICE_SUFFIX: &str = ".slice";
/// The path separator of a hint written on Windows.
const WINDOWS_SEPARATOR: char = '\\';
/// The path separator of a pak path.
const PAK_SEPARATOR: char = '/';
/// An entity id that names no entity (`m_parentId` of a root entity).
const NO_ENTITY: u64 = 0xffff_ffff;
/// Deepest descendant searched for a component field: a component keeps its fields in itself,
/// its server facet or its base class (Observed: at most 3 levels).
const FIELD_DEPTH: usize = 3;
/// Deepest descendant searched for a loot table id (`GatherableController` → instanced loot
/// entry → `m_lootTableId`).
const LOOT_TABLE_DEPTH: usize = 4;

/// A script slice that cannot be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LevelScriptError {
    /// A value has another size or text form than its field needs.
    #[error("level script: {0}")]
    Value(#[from] ObjectStreamError),
    /// A transform is not 12 floats.
    #[error("level script: {0}")]
    Transform(#[from] SliceDataError),
    /// The root has no slice component with an entity list.
    #[error("level script: the root holds no slice entities")]
    NoEntities,
    /// An entity or component lacks a field the reader needs.
    #[error("level script: a {0} has no {1}")]
    Missing(&'static str, &'static str),
    /// An asset reference is shorter than its hint length says.
    #[error("level script: an asset reference of {0} bytes")]
    Asset(usize),
}

/// An asset a slice names: its GUID and its pak path (lower case, `/`, `.dynamicslice` for a
/// slice).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AssetRef {
    /// The asset GUID as stored.
    pub guid: [u8; GUID_LEN],
    /// The pak path.
    pub path: String,
}

/// When a spawner spawns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spawner {
    /// `m_sliceAsset`.
    pub slice: Option<AssetRef>,
    /// `m_aliasAsset`.
    pub alias: Option<AssetRef>,
    /// `m_spawnOnInitialize`.
    pub on_initialize: bool,
    /// `m_spawnOnTrigger`.
    pub on_trigger: bool,
}

/// The vitals row of a spawner slice's mob (`AIVariantProviderComponent`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VitalsVariant {
    /// `m_vitalsTableRowId`.
    pub vitals_id: String,
    /// `m_vitalsCategoryTableRowId`.
    pub category_id: String,
    /// `m_vitalsLevel`; 0 takes the level of the vitals row.
    pub level: u32,
}

/// An encounter's activation (`EncounterManagerComponent`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncounterManager {
    /// `m_autoActivate`.
    pub auto_activate: bool,
    /// `m_autoActivateDelayMS`.
    pub auto_activate_delay_ms: u32,
    /// `m_gracePeriod`, seconds.
    pub grace_period_s: u32,
    /// `m_resetEncounterEventId`; empty for none.
    pub reset_event: String,
    /// `m_stages`: its top stages, in order.
    pub stages: Vec<u64>,
}

/// One `SpawnDefinition` of a stage's spawn timeline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpawnDefinition {
    /// `m_sliceAsset`.
    pub slice: Option<AssetRef>,
    /// `m_aliasAsset`.
    pub alias: Option<AssetRef>,
    /// `m_spawnDelay`, seconds after the stage starts.
    pub delay_s: u32,
    /// `m_count`.
    pub count: u32,
    /// `m_spawnLocations`: entities of the slice.
    pub locations: Vec<u64>,
}

/// What an objective waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectiveKind {
    /// `SliceDestroyedObjective`: `required` deaths of the tracked slice or alias.
    SliceDestroyed {
        /// `m_trackedSlice`.
        slice: Option<AssetRef>,
        /// `m_trackedAlias`.
        alias: Option<AssetRef>,
        /// `m_requiredCount`.
        required: u32,
    },
    /// `BossPhaseObjective`: the tracked boss reaches `phase`.
    BossPhase {
        /// `m_trackedSlice`.
        slice: Option<AssetRef>,
        /// `m_trackedAlias`.
        alias: Option<AssetRef>,
        /// `m_trackedPhase`.
        phase: u32,
    },
    /// `EncounterEventObjective`: a tree of the encounter sends `event`.
    EncounterEvent {
        /// `m_eventId`.
        event: String,
    },
    /// `EntityActiveStateChangedObjective`: an entity of the slice changes state.
    EntityActiveState {
        /// `m_targetEntityId`.
        entity: u64,
        /// `m_completeOnActivated`.
        on_activated: bool,
        /// `m_completeOnDeactivated`.
        on_deactivated: bool,
    },
    /// `WaitForParentObjective`.
    WaitForParent,
    /// `InvasionWaveObjective`.
    InvasionWave,
}

/// One objective of a stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objective {
    /// `m_timeLimit`, seconds; 0 for none.
    pub time_limit_s: u32,
    /// `m_succeedOnTimeout`.
    pub succeed_on_timeout: bool,
    /// `m_failureCondition`: its completion fails the stage.
    pub failure: bool,
    /// What it waits for.
    pub kind: ObjectiveKind,
}

/// One start or end action of a stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `EntityEventAction`: an event on an entity of the slice.
    EntityEvent {
        /// `m_entityRef`.
        entity: u64,
        /// `m_type`.
        event: TriggerEvent,
        /// `m_applyRecursively`.
        recursive: bool,
        /// `m_localPlayerOnly`.
        local_player_only: bool,
    },
    /// `DestroySliceAction`: the encounter's spawns of a slice or alias go.
    DestroySlice {
        /// `m_sliceAsset`.
        slice: Option<AssetRef>,
        /// `m_aliasAsset`.
        alias: Option<AssetRef>,
    },
    /// `KillSpawnsAction`: every spawn of the stage dies.
    KillSpawns,
}

/// One stage of an encounter (`EncounterComponent`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncounterStage {
    /// `m_childStagesOptional`.
    pub child_stages_optional: bool,
    /// `m_userWave`.
    pub user_wave: bool,
    /// `m_stages`: its child stages, in order.
    pub stages: Vec<u64>,
    /// `m_spawnTimeline`.
    pub spawns: Vec<SpawnDefinition>,
    /// `m_objectives`.
    pub objectives: Vec<Objective>,
    /// `m_startActions`.
    pub start_actions: Vec<Action>,
    /// `m_endActions`.
    pub end_actions: Vec<Action>,
}

/// A rigid transform: a basis by rows and a translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// The basis, by rows.
    pub basis: Basis,
    /// The translation.
    pub translation: [f64; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            basis: IDENTITY,
            translation: [0.0; 3],
        }
    }
}

impl Transform {
    /// `self` after `child`: `child` is relative to `self`.
    pub fn then(&self, child: &Self) -> Self {
        let offset = crate::slice_data::apply(&self.basis, &child.translation);
        Self {
            basis: crate::slice_data::multiply(&self.basis, &child.basis),
            translation: [
                self.translation[0] + offset[0],
                self.translation[1] + offset[1],
                self.translation[2] + offset[2],
            ],
        }
    }

    /// The transform of a placement at `position` turned by quaternion `rotation` and scaled
    /// by `scale`.
    pub fn placed(position: [f32; 3], rotation: [f32; 4], scale: f32) -> Self {
        Self {
            basis: crate::slice_data::scaled(&crate::slice_data::quaternion_basis(rotation), scale),
            translation: position.map(f64::from),
        }
    }

    /// The yaw, degrees counter-clockwise from the x axis.
    pub fn yaw_degrees(&self) -> f64 {
        self.basis[1][0].atan2(self.basis[0][0]).to_degrees()
    }
}

/// One entity of a script slice with the components this reader keeps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptEntity {
    /// `Id`.
    pub id: u64,
    /// `Name`.
    pub name: String,
    /// `m_parentId`; `None` for a root entity.
    pub parent: Option<u64>,
    /// `m_localTM`.
    pub local: Transform,
    /// Its spawner.
    pub spawner: Option<Spawner>,
    /// Its vitals row.
    pub variant: Option<VitalsVariant>,
    /// Its encounter activation.
    pub manager: Option<EncounterManager>,
    /// Its stage.
    pub stage: Option<EncounterStage>,
    /// The radius of its detection volume's sphere, metres.
    pub detection_radius_m: Option<f32>,
    /// Its loot container's loot table (`m_lootTableId`).
    pub loot_table: Option<String>,
}

/// The entities of one script slice, in stream order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SliceScript {
    /// Its entities.
    pub entities: Vec<ScriptEntity>,
}

impl SliceScript {
    /// The entity `id`.
    pub fn entity(&self, id: u64) -> Option<&ScriptEntity> {
        self.entities.iter().find(|entity| entity.id == id)
    }

    /// The transform of entity `id` relative to the slice's placement: its local transform
    /// after its parents' (a parent chain longer than the slice is cut).
    pub fn placement_of(&self, id: u64) -> Transform {
        let mut chain = Vec::new();
        let mut at = self.entity(id);
        while let Some(entity) = at {
            if chain.len() > self.entities.len() {
                break;
            }
            chain.push(entity.local);
            at = entity.parent.and_then(|parent| self.entity(parent));
        }
        chain
            .iter()
            .rev()
            .fold(Transform::default(), |placed, local| placed.then(local))
    }

    /// The entity with the encounter manager, when the slice is an encounter.
    pub fn manager(&self) -> Option<(&ScriptEntity, &EncounterManager)> {
        self.entities
            .iter()
            .find_map(|entity| entity.manager.as_ref().map(|manager| (entity, manager)))
    }

    /// The vitals row of the slice's mob, when the slice is a spawner slice.
    pub fn variant(&self) -> Option<&VitalsVariant> {
        self.entities
            .iter()
            .find_map(|entity| entity.variant.as_ref())
    }

    /// The loot table of the slice's loot container, when it is one.
    pub fn loot_table(&self) -> Option<&str> {
        self.entities
            .iter()
            .find_map(|entity| entity.loot_table.as_deref())
    }
}

/// The names of the fields read, as their name CRCs.
struct Names {
    id: u32,
    name: u32,
    components: u32,
    entities: u32,
    local_tm: u32,
    parent_id: u32,
    entity_id: u32,
    entity_ref: u32,
    base_class: u32,
    slice_asset: u32,
    alias_asset: u32,
    on_initialize: u32,
    on_trigger: u32,
    vitals_row: u32,
    category_row: u32,
    vitals_level: u32,
    auto_activate: u32,
    auto_activate_delay: u32,
    grace_period: u32,
    reset_event: u32,
    stages: u32,
    child_optional: u32,
    user_wave: u32,
    spawn_timeline: u32,
    spawn_delay: u32,
    count: u32,
    spawn_locations: u32,
    objectives: u32,
    start_actions: u32,
    end_actions: u32,
    time_limit: u32,
    succeed_on_timeout: u32,
    failure: u32,
    tracked_slice: u32,
    tracked_alias: u32,
    required: u32,
    tracked_phase: u32,
    event_id: u32,
    target_entity: u32,
    on_activated: u32,
    on_deactivated: u32,
    event_data: u32,
    event_type: u32,
    recursive: u32,
    local_player_only: u32,
    shape: u32,
    radius: u32,
    loot_table: u32,
}

impl Names {
    fn new() -> Self {
        Self {
            id: lowercase_crc32("Id"),
            name: lowercase_crc32("Name"),
            components: lowercase_crc32("Components"),
            entities: lowercase_crc32("Entities"),
            local_tm: lowercase_crc32("m_localTM"),
            parent_id: lowercase_crc32("m_parentId"),
            entity_id: lowercase_crc32("EntityId"),
            entity_ref: lowercase_crc32("m_entityRef"),
            base_class: lowercase_crc32("BaseClass1"),
            slice_asset: lowercase_crc32("m_sliceAsset"),
            alias_asset: lowercase_crc32("m_aliasAsset"),
            on_initialize: lowercase_crc32("m_spawnOnInitialize"),
            on_trigger: lowercase_crc32("m_spawnOnTrigger"),
            vitals_row: lowercase_crc32("m_vitalsTableRowId"),
            category_row: lowercase_crc32("m_vitalsCategoryTableRowId"),
            vitals_level: lowercase_crc32("m_vitalsLevel"),
            auto_activate: lowercase_crc32("m_autoActivate"),
            auto_activate_delay: lowercase_crc32("m_autoActivateDelayMS"),
            grace_period: lowercase_crc32("m_gracePeriod"),
            reset_event: lowercase_crc32("m_resetEncounterEventId"),
            stages: lowercase_crc32("m_stages"),
            child_optional: lowercase_crc32("m_childStagesOptional"),
            user_wave: lowercase_crc32("m_userWave"),
            spawn_timeline: lowercase_crc32("m_spawnTimeline"),
            spawn_delay: lowercase_crc32("m_spawnDelay"),
            count: lowercase_crc32("m_count"),
            spawn_locations: lowercase_crc32("m_spawnLocations"),
            objectives: lowercase_crc32("m_objectives"),
            start_actions: lowercase_crc32("m_startActions"),
            end_actions: lowercase_crc32("m_endActions"),
            time_limit: lowercase_crc32("m_timeLimit"),
            succeed_on_timeout: lowercase_crc32("m_succeedOnTimeout"),
            failure: lowercase_crc32("m_failureCondition"),
            tracked_slice: lowercase_crc32("m_trackedSlice"),
            tracked_alias: lowercase_crc32("m_trackedAlias"),
            required: lowercase_crc32("m_requiredCount"),
            tracked_phase: lowercase_crc32("m_trackedPhase"),
            event_id: lowercase_crc32("m_eventId"),
            target_entity: lowercase_crc32("m_targetEntityId"),
            on_activated: lowercase_crc32("m_completeOnActivated"),
            on_deactivated: lowercase_crc32("m_completeOnDeactivated"),
            event_data: lowercase_crc32("m_eventData"),
            event_type: lowercase_crc32("m_type"),
            recursive: lowercase_crc32("m_applyRecursively"),
            local_player_only: lowercase_crc32("m_localPlayerOnly"),
            shape: lowercase_crc32("m_shape"),
            radius: lowercase_crc32("m_radius"),
            loot_table: lowercase_crc32("m_lootTableId"),
        }
    }
}

/// The first descendant of `element` named `name_crc`, breadth first, at most `depth` levels
/// down.
fn find(element: &Element, name_crc: u32, depth: usize) -> Option<&Element> {
    let mut level: Vec<&Element> = vec![element];
    for _ in 0..depth {
        let mut next = Vec::new();
        for parent in level {
            if let Some(found) = parent.child(name_crc) {
                return Some(found);
            }
            next.extend(parent.children.iter());
        }
        level = next;
    }
    None
}

/// The pak path of hint `hint`: lower case, `/`, an editor slice as its dynamic slice.
pub fn pak_path(hint: &str) -> String {
    let path = hint
        .to_lowercase()
        .replace(WINDOWS_SEPARATOR, &PAK_SEPARATOR.to_string());
    match path.strip_suffix(EDITOR_SLICE_SUFFIX) {
        Some(bare) => format!("{bare}{DYNAMIC_SLICE_SUFFIX}"),
        None => path,
    }
}

/// The asset of reference `element`; `None` when it names none (no GUID and no path).
fn asset(element: Option<&Element>) -> Result<Option<AssetRef>, LevelScriptError> {
    let Some(value) = element.map(|element| element.value.as_slice()) else {
        return Ok(None);
    };
    let length = value
        .get(ASSET_HINT_LENGTH_AT..ASSET_HINT_AT)
        .and_then(|bytes| <[u8; size_of::<u64>()]>::try_from(bytes).ok())
        .map(u64::from_be_bytes)
        .and_then(|length| usize::try_from(length).ok())
        .ok_or(LevelScriptError::Asset(value.len()))?;
    let hint = value
        .get(ASSET_HINT_AT..ASSET_HINT_AT.saturating_add(length))
        .ok_or(LevelScriptError::Asset(value.len()))?;
    let mut guid = [0; GUID_LEN];
    guid.copy_from_slice(&value[..GUID_LEN]);
    let path = pak_path(&String::from_utf8_lossy(hint));
    Ok((guid != [0; GUID_LEN] || !path.is_empty()).then_some(AssetRef { guid, path }))
}

fn flag(element: &Element, name_crc: u32) -> Result<bool, LevelScriptError> {
    find(element, name_crc, FIELD_DEPTH)
        .map_or(Ok(false), Element::as_bool)
        .map_err(LevelScriptError::from)
}

fn number(element: &Element, name_crc: u32) -> Result<u32, LevelScriptError> {
    find(element, name_crc, FIELD_DEPTH)
        .map_or(Ok(0), Element::as_u32)
        .map_err(LevelScriptError::from)
}

fn text(element: &Element, name_crc: u32, depth: usize) -> Result<String, LevelScriptError> {
    find(element, name_crc, depth)
        .map_or(Ok(""), Element::as_string)
        .map(str::to_owned)
        .map_err(LevelScriptError::from)
}

/// The id an entity reference holds: `EntityId` → `Id`, or the `Id` right in it.
fn reference(element: &Element, names: &Names) -> Result<Option<u64>, LevelScriptError> {
    let holder = element.child(names.entity_id).unwrap_or(element);
    let id = holder
        .child(names.id)
        .ok_or(LevelScriptError::Missing("entity reference", "Id"))?
        .as_u64()?;
    Ok((id != NO_ENTITY).then_some(id))
}

/// The ids of the entity references of list `list`.
fn references(list: Option<&Element>, names: &Names) -> Result<Vec<u64>, LevelScriptError> {
    let mut ids = Vec::new();
    for entry in list
        .map(|list| list.children.as_slice())
        .unwrap_or_default()
    {
        ids.extend(reference(entry, names)?);
    }
    Ok(ids)
}

/// The class entries of a list of pointers (`m_objectives`, `m_startActions`): each entry
/// holds its class as its one child.
fn pointed(list: Option<&Element>) -> impl Iterator<Item = &Element> {
    list.into_iter()
        .flat_map(|list| &list.children)
        .filter_map(|entry| entry.children.first())
}

fn objective(class: &Element, names: &Names) -> Result<Option<Objective>, LevelScriptError> {
    let kind = match class.type_id {
        objective_type::SLICE_DESTROYED => ObjectiveKind::SliceDestroyed {
            slice: asset(class.child(names.tracked_slice))?,
            alias: asset(class.child(names.tracked_alias))?,
            required: number(class, names.required)?,
        },
        objective_type::BOSS_PHASE => ObjectiveKind::BossPhase {
            slice: asset(class.child(names.tracked_slice))?,
            alias: asset(class.child(names.tracked_alias))?,
            phase: number(class, names.tracked_phase)?,
        },
        objective_type::ENCOUNTER_EVENT => ObjectiveKind::EncounterEvent {
            event: text(class, names.event_id, FIELD_DEPTH)?,
        },
        objective_type::ENTITY_ACTIVE_STATE => ObjectiveKind::EntityActiveState {
            entity: class
                .child(names.target_entity)
                .map(|target| reference(target, names))
                .transpose()?
                .flatten()
                .ok_or(LevelScriptError::Missing("objective", "m_targetEntityId"))?,
            on_activated: flag(class, names.on_activated)?,
            on_deactivated: flag(class, names.on_deactivated)?,
        },
        objective_type::WAIT_FOR_PARENT => ObjectiveKind::WaitForParent,
        objective_type::INVASION_WAVE => ObjectiveKind::InvasionWave,
        _ => return Ok(None),
    };
    let base = class.child(names.base_class).unwrap_or(class);
    Ok(Some(Objective {
        time_limit_s: number(base, names.time_limit)?,
        succeed_on_timeout: flag(base, names.succeed_on_timeout)?,
        failure: flag(base, names.failure)?,
        kind,
    }))
}

fn action(class: &Element, names: &Names) -> Result<Option<Action>, LevelScriptError> {
    Ok(Some(match class.type_id {
        action_type::ENTITY_EVENT => {
            let data = class
                .child(names.event_data)
                .ok_or(LevelScriptError::Missing(
                    "entity event action",
                    "m_eventData",
                ))?;
            let Some(entity) = data
                .child(names.entity_ref)
                .map(|holder| reference(holder, names))
                .transpose()?
                .flatten()
            else {
                return Ok(None);
            };
            Action::EntityEvent {
                entity,
                event: TriggerEvent::from(number(data, names.event_type)?),
                recursive: flag(data, names.recursive)?,
                local_player_only: flag(data, names.local_player_only)?,
            }
        }
        action_type::DESTROY_SLICE => Action::DestroySlice {
            slice: asset(class.child(names.slice_asset))?,
            alias: asset(class.child(names.alias_asset))?,
        },
        action_type::KILL_SPAWNS => Action::KillSpawns,
        _ => return Ok(None),
    }))
}

fn actions(list: Option<&Element>, names: &Names) -> Result<Vec<Action>, LevelScriptError> {
    let mut read = Vec::new();
    for class in pointed(list) {
        read.extend(action(class, names)?);
    }
    Ok(read)
}

fn stage(component: &Element, names: &Names) -> Result<EncounterStage, LevelScriptError> {
    let mut spawns = Vec::new();
    for definition in component
        .child(names.spawn_timeline)
        .map(|list| list.children.as_slice())
        .unwrap_or_default()
    {
        spawns.push(SpawnDefinition {
            slice: asset(definition.child(names.slice_asset))?,
            alias: asset(definition.child(names.alias_asset))?,
            delay_s: number(definition, names.spawn_delay)?,
            count: number(definition, names.count)?,
            locations: references(definition.child(names.spawn_locations), names)?,
        });
    }
    let mut objectives = Vec::new();
    for class in pointed(component.child(names.objectives)) {
        objectives.extend(objective(class, names)?);
    }
    Ok(EncounterStage {
        child_stages_optional: flag(component, names.child_optional)?,
        user_wave: flag(component, names.user_wave)?,
        stages: references(component.child(names.stages), names)?,
        spawns,
        objectives,
        start_actions: actions(component.child(names.start_actions), names)?,
        end_actions: actions(component.child(names.end_actions), names)?,
    })
}

fn manager(component: &Element, names: &Names) -> Result<EncounterManager, LevelScriptError> {
    Ok(EncounterManager {
        auto_activate: flag(component, names.auto_activate)?,
        auto_activate_delay_ms: number(component, names.auto_activate_delay)?,
        grace_period_s: number(component, names.grace_period)?,
        reset_event: text(component, names.reset_event, FIELD_DEPTH)?,
        stages: references(component.child(names.stages), names)?,
    })
}

fn spawner(component: &Element, names: &Names) -> Result<Spawner, LevelScriptError> {
    Ok(Spawner {
        slice: asset(find(component, names.slice_asset, FIELD_DEPTH))?,
        alias: asset(find(component, names.alias_asset, FIELD_DEPTH))?,
        on_initialize: flag(component, names.on_initialize)?,
        on_trigger: flag(component, names.on_trigger)?,
    })
}

fn entity(element: &Element, names: &Names) -> Result<ScriptEntity, LevelScriptError> {
    let id = element
        .child(names.id)
        .and_then(|id| id.child(names.id))
        .ok_or(LevelScriptError::Missing("slice entity", "Id"))?
        .as_u64()?;
    let mut read = ScriptEntity {
        id,
        name: element
            .child(names.name)
            .map_or(Ok(""), Element::as_string)?
            .trim()
            .to_owned(),
        ..ScriptEntity::default()
    };
    for component in element
        .child(names.components)
        .map(|list| list.children.as_slice())
        .unwrap_or_default()
    {
        if let Some(local) = component.child(names.local_tm) {
            let (basis, translation) = transform_from_bytes(&local.value)?;
            read.local = Transform { basis, translation };
            read.parent = component
                .child(names.parent_id)
                .map(|parent| reference(parent, names))
                .transpose()?
                .flatten();
            continue;
        }
        match component.type_id {
            ENCOUNTER_MANAGER_TYPE => read.manager = Some(manager(component, names)?),
            ENCOUNTER_STAGE_TYPE => read.stage = Some(stage(component, names)?),
            PREFAB_SPAWNER_TYPE | POINT_SPAWNER_TYPE => {
                read.spawner = Some(spawner(component, names)?);
            }
            VARIANT_PROVIDER_TYPE => {
                read.variant = Some(VitalsVariant {
                    vitals_id: text(component, names.vitals_row, FIELD_DEPTH)?,
                    category_id: text(component, names.category_row, FIELD_DEPTH)?,
                    level: number(component, names.vitals_level)?,
                });
            }
            DETECTION_VOLUME_TYPE => {
                read.detection_radius_m = component
                    .child(names.shape)
                    .and_then(|shape| find(shape, names.radius, FIELD_DEPTH))
                    .map(Element::as_f32)
                    .transpose()?;
            }
            GATHERABLE_CONTROLLER_TYPE => {
                let table = text(component, names.loot_table, LOOT_TABLE_DEPTH)?;
                read.loot_table = (!table.is_empty()).then_some(table);
            }
            _ => {}
        }
    }
    Ok(read)
}

/// The entities of the script slice whose ObjectStream root is `root`.
///
/// # Errors
///
/// Returns [`LevelScriptError`] when the root holds no slice entities, or an entity, transform,
/// asset reference or kept component has the wrong form.
pub fn slice_script(root: &Element) -> Result<SliceScript, LevelScriptError> {
    let names = Names::new();
    let entities = root
        .child(names.components)
        .and_then(|components| {
            components
                .children
                .iter()
                .find_map(|component| component.child(names.entities))
        })
        .ok_or(LevelScriptError::NoEntities)?;
    Ok(SliceScript {
        entities: entities
            .children
            .iter()
            .map(|element| entity(element, &names))
            .collect::<Result<_, _>>()?,
    })
}

/// One slice of an alias asset with its weight (`Percent`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasSlice {
    /// The slice.
    pub slice: AssetRef,
    /// `Percent`.
    pub percent: u32,
}

/// The slices of the alias asset whose ObjectStream root is `root` (an `.aliasasset`, AZCS
/// compressed in the paks): every `slices` entry's `slice` and `Percent`.
///
/// # Errors
///
/// Returns [`LevelScriptError`] when a reference or weight has the wrong form.
pub fn alias_slices(root: &Element) -> Result<Vec<AliasSlice>, LevelScriptError> {
    let slices_crc = lowercase_crc32("slices");
    let slice_crc = lowercase_crc32("slice");
    let percent_crc = lowercase_crc32("Percent");
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(element) = stack.pop() {
        if element.name_crc == Some(slices_crc) {
            for entry in &element.children {
                if let Some(slice) = asset(entry.child(slice_crc))? {
                    let percent = entry.child(percent_crc).map_or(Ok(0), Element::as_u32)?;
                    found.push(AliasSlice { slice, percent });
                }
            }
            continue;
        }
        stack.extend(element.children.iter().rev());
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCOUNTER: u64 = 0xa6;
    const WAVE: u64 = 0x8e;
    const PHASE: u64 = 0x8f;
    const TIMELINE: u64 = 0xa8;
    const LOCATION: u64 = 0xb0;
    const BOSS_PATH: &str = "Slices/Dungeon_EventManager/Spawners/Boss.slice";
    const BOSS_PAK_PATH: &str = "slices/dungeon_eventmanager/spawners/boss.dynamicslice";
    const BOSS_GUID: [u8; GUID_LEN] = [7; GUID_LEN];
    const EVENT: &str = "Intro2_Complete";
    const TIME_LIMIT_S: u32 = 20;
    const GRACE_S: u32 = 20;
    const RADIUS_M: f32 = 30.0;
    const OFFSET: [f32; 3] = [106.1, 232.0, 7.0];
    const VITALS: &str = "Undead_Brute_DG_Windsward_00";
    const LOOT: &str = "ChestLootElite";

    fn named(name: &str, children: Vec<Element>) -> Element {
        Element::with_children(lowercase_crc32(name), children)
    }

    fn value(name: &str, bytes: &[u8]) -> Element {
        Element::with_value(lowercase_crc32(name), bytes.to_vec())
    }

    fn typed(mut element: Element, type_id: [u8; UUID_SIZE]) -> Element {
        element.type_id = type_id;
        element
    }

    fn id_holder(name: &str, id: u64) -> Element {
        named(name, vec![value("Id", &id.to_be_bytes())])
    }

    fn reference(id: u64) -> Element {
        named("element", vec![id_holder("EntityId", id)])
    }

    fn asset_value(name: &str, guid: [u8; GUID_LEN], hint: &str) -> Element {
        let mut bytes = guid.to_vec();
        bytes.resize(ASSET_HINT_LENGTH_AT, 0);
        bytes.extend(
            u64::try_from(hint.len())
                .expect("hint length")
                .to_be_bytes(),
        );
        bytes.extend(hint.as_bytes());
        value(name, &bytes)
    }

    fn transform(parent: u64, translation: [f32; 3]) -> Element {
        let mut floats = Vec::new();
        for column in [
            [1.0_f32, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            translation,
        ] {
            for cell in column {
                floats.extend(cell.to_be_bytes());
            }
        }
        named(
            "element",
            vec![value("m_localTM", &floats), id_holder("m_parentId", parent)],
        )
    }

    fn entity(id: u64, name: &str, components: Vec<Element>) -> Element {
        named(
            "element",
            vec![
                id_holder("Id", id),
                value("Name", name.as_bytes()),
                named("Components", components),
            ],
        )
    }

    fn base(time_limit_s: u32, succeed: bool) -> Element {
        named(
            "BaseClass1",
            vec![
                value("m_timeLimit", &time_limit_s.to_be_bytes()),
                value("m_succeedOnTimeout", &[u8::from(succeed)]),
                value("m_failureCondition", &[0]),
            ],
        )
    }

    fn pointer(class: Element) -> Element {
        named("element", vec![class])
    }

    fn slice(entities: Vec<Element>) -> Element {
        named(
            "root",
            vec![named(
                "Components",
                vec![named("element", vec![named("Entities", entities)])],
            )],
        )
    }

    fn encounter_slice() -> Element {
        let manager = typed(
            named(
                "element",
                vec![
                    named(
                        "m_serverFacetPtr",
                        vec![
                            value("m_autoActivate", &[1]),
                            value("m_autoActivateDelayMS", &0_u32.to_be_bytes()),
                            value("m_gracePeriod", &GRACE_S.to_be_bytes()),
                            value("m_resetEncounterEventId", b"CleanupReset"),
                        ],
                    ),
                    named("m_stages", vec![reference(WAVE)]),
                ],
            ),
            ENCOUNTER_MANAGER_TYPE,
        );
        let volume = typed(
            named(
                "element",
                vec![named(
                    "m_shape",
                    vec![named(
                        "element",
                        vec![value("m_radius", &RADIUS_M.to_be_bytes())],
                    )],
                )],
            ),
            DETECTION_VOLUME_TYPE,
        );
        let wave = typed(
            named(
                "element",
                vec![
                    named(
                        "m_serverFacetPtr",
                        vec![value("m_childStagesOptional", &[1])],
                    ),
                    named("m_stages", vec![reference(PHASE)]),
                    named(
                        "m_spawnTimeline",
                        vec![named(
                            "element",
                            vec![
                                asset_value("m_sliceAsset", BOSS_GUID, BOSS_PATH),
                                asset_value("m_aliasAsset", [0; GUID_LEN], ""),
                                value("m_spawnDelay", &0_u32.to_be_bytes()),
                                value("m_count", &1_u32.to_be_bytes()),
                                named("m_spawnLocations", vec![reference(LOCATION)]),
                            ],
                        )],
                    ),
                    named(
                        "m_objectives",
                        vec![pointer(typed(
                            named(
                                "element",
                                vec![
                                    base(0, false),
                                    asset_value("m_trackedSlice", BOSS_GUID, BOSS_PATH),
                                    asset_value("m_trackedAlias", [0; GUID_LEN], ""),
                                    value("m_requiredCount", &1_u32.to_be_bytes()),
                                ],
                            ),
                            objective_type::SLICE_DESTROYED,
                        ))],
                    ),
                    named("m_startActions", Vec::new()),
                    named(
                        "m_endActions",
                        vec![pointer(typed(
                            named("element", vec![value("m_localOnly", &[0])]),
                            action_type::KILL_SPAWNS,
                        ))],
                    ),
                    value("m_userWave", &[0]),
                ],
            ),
            ENCOUNTER_STAGE_TYPE,
        );
        let phase = typed(
            named(
                "element",
                vec![
                    named("m_stages", Vec::new()),
                    named(
                        "m_objectives",
                        vec![pointer(typed(
                            named(
                                "element",
                                vec![
                                    base(TIME_LIMIT_S, true),
                                    value("m_eventId", EVENT.as_bytes()),
                                ],
                            ),
                            objective_type::ENCOUNTER_EVENT,
                        ))],
                    ),
                    named(
                        "m_startActions",
                        vec![pointer(typed(
                            named(
                                "element",
                                vec![named(
                                    "m_eventData",
                                    vec![
                                        named("m_entityRef", vec![id_holder("EntityId", TIMELINE)]),
                                        value("m_type", &1_u32.to_be_bytes()),
                                        value("m_applyRecursively", &[1]),
                                        value("m_localPlayerOnly", &[0]),
                                    ],
                                )],
                            ),
                            action_type::ENTITY_EVENT,
                        ))],
                    ),
                    value("m_userWave", &[1]),
                ],
            ),
            ENCOUNTER_STAGE_TYPE,
        );
        slice(vec![
            entity(
                ENCOUNTER,
                "DG_Enc_Test_Boss",
                vec![transform(NO_ENTITY, [0.0; 3]), manager, volume],
            ),
            entity(WAVE, "Wave_01", vec![transform(ENCOUNTER, [0.0; 3]), wave]),
            entity(
                PHASE,
                "Phase 1 Intro",
                vec![transform(WAVE, [0.0; 3]), phase],
            ),
            entity(LOCATION, "Location", vec![transform(ENCOUNTER, OFFSET)]),
        ])
    }

    #[test]
    fn an_encounter_reads_its_manager_stages_spawns_objectives_and_actions() {
        let script = slice_script(&encounter_slice()).expect("script");
        let (root, manager) = script.manager().expect("manager");
        assert_eq!(root.id, ENCOUNTER);
        assert_eq!(root.detection_radius_m, Some(RADIUS_M));
        assert_eq!(
            *manager,
            EncounterManager {
                auto_activate: true,
                auto_activate_delay_ms: 0,
                grace_period_s: GRACE_S,
                reset_event: "CleanupReset".to_owned(),
                stages: vec![WAVE],
            }
        );
        let wave = script
            .entity(WAVE)
            .and_then(|e| e.stage.clone())
            .expect("wave");
        assert!(wave.child_stages_optional && !wave.user_wave);
        assert_eq!(wave.stages, [PHASE]);
        let boss = AssetRef {
            guid: BOSS_GUID,
            path: BOSS_PAK_PATH.to_owned(),
        };
        assert_eq!(
            wave.spawns,
            [SpawnDefinition {
                slice: Some(boss.clone()),
                alias: None,
                delay_s: 0,
                count: 1,
                locations: vec![LOCATION],
            }]
        );
        assert_eq!(
            wave.objectives,
            [Objective {
                time_limit_s: 0,
                succeed_on_timeout: false,
                failure: false,
                kind: ObjectiveKind::SliceDestroyed {
                    slice: Some(boss),
                    alias: None,
                    required: 1,
                },
            }]
        );
        assert_eq!(wave.end_actions, [Action::KillSpawns]);
        let phase = script
            .entity(PHASE)
            .and_then(|e| e.stage.clone())
            .expect("phase");
        assert_eq!(
            phase.objectives[0],
            Objective {
                time_limit_s: TIME_LIMIT_S,
                succeed_on_timeout: true,
                failure: false,
                kind: ObjectiveKind::EncounterEvent {
                    event: EVENT.to_owned()
                },
            }
        );
        assert_eq!(
            phase.start_actions,
            [Action::EntityEvent {
                entity: TIMELINE,
                event: TriggerEvent::UnTrigger,
                recursive: true,
                local_player_only: false,
            }]
        );
    }

    #[test]
    fn placements_follow_the_parent_chain() {
        let script = slice_script(&encounter_slice()).expect("script");
        let placed = Transform::placed([780.0, 674.0, 134.0], [0.0, 0.0, 0.0, 1.0], 1.0)
            .then(&script.placement_of(LOCATION));
        let expected = [780.0 + OFFSET[0], 674.0 + OFFSET[1], 134.0 + OFFSET[2]];
        for (got, want) in placed.translation.iter().zip(expected) {
            assert!((got - f64::from(want)).abs() < 1e-3, "{got} {want}");
        }
        assert!(script.entity(ENCOUNTER).is_some_and(|e| e.parent.is_none()));
    }

    #[test]
    fn a_spawner_slice_names_its_vitals_and_a_chest_its_loot_table() {
        let variant = typed(
            named(
                "element",
                vec![named(
                    "m_serverFacetPtr",
                    vec![
                        value("m_vitalsTableRowId", VITALS.as_bytes()),
                        value("m_vitalsCategoryTableRowId", VITALS.as_bytes()),
                        value("m_vitalsLevel", &0_u32.to_be_bytes()),
                    ],
                )],
            ),
            VARIANT_PROVIDER_TYPE,
        );
        let chest = typed(
            named(
                "element",
                vec![named(
                    "m_instancedLootEntityRef",
                    vec![named("loot", vec![value("m_lootTableId", LOOT.as_bytes())])],
                )],
            ),
            GATHERABLE_CONTROLLER_TYPE,
        );
        let script =
            slice_script(&slice(vec![entity(1, "Spawner", vec![variant, chest])])).expect("script");
        assert_eq!(
            script.variant(),
            Some(&VitalsVariant {
                vitals_id: VITALS.to_owned(),
                category_id: VITALS.to_owned(),
                level: 0,
            })
        );
        assert_eq!(script.loot_table(), Some(LOOT));
        assert!(script.manager().is_none());
    }

    #[test]
    fn hints_become_pak_paths_and_an_empty_reference_is_none() {
        assert_eq!(pak_path(BOSS_PATH), BOSS_PAK_PATH);
        assert_eq!(
            pak_path("Slices\\Characters\\Boss.dynamicslice"),
            "slices/characters/boss.dynamicslice"
        );
        let empty = asset_value("m_sliceAsset", [0; GUID_LEN], "");
        assert_eq!(asset(Some(&empty)), Ok(None));
        let short = value("m_sliceAsset", &[1; ASSET_HINT_LENGTH_AT]);
        assert_eq!(
            asset(Some(&short)),
            Err(LevelScriptError::Asset(ASSET_HINT_LENGTH_AT))
        );
    }

    #[test]
    fn an_alias_lists_its_slices_with_their_weights() {
        let entry = |guid: u8, percent: u32| {
            named(
                "element",
                vec![
                    asset_value("slice", [guid; GUID_LEN], "slices/a.dynamicslice"),
                    value("Percent", &percent.to_be_bytes()),
                ],
            )
        };
        let root = named(
            "root",
            vec![named(
                "list",
                vec![named(
                    "element",
                    vec![named("slices", vec![entry(1, 100), entry(2, 50)])],
                )],
            )],
        );
        let slices = alias_slices(&root).expect("alias");
        assert_eq!(slices.len(), 2);
        assert_eq!((slices[0].slice.guid[0], slices[0].percent), (1, 100));
        assert_eq!((slices[1].slice.guid[0], slices[1].percent), (2, 50));
    }

    #[test]
    fn a_slice_without_entities_is_an_error() {
        assert_eq!(
            slice_script(&named("root", Vec::new())),
            Err(LevelScriptError::NoEntities)
        );
    }
}
