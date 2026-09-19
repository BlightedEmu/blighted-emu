// SPDX-License-Identifier: AGPL-3.0-or-later
//! Body codecs of the game messages of M4 (M4 spec section 5).
//!
//! A body is the part of an envelope after the type index. Layouts come only from the RE notes:
//! `docs/re/2026-09-04-bootstrap-message-bodies.md` for the messages and
//! `docs/re/2026-09-02-replication-and-movement.md` for the replicated states. A field without a
//! name in the notes keeps a neutral name. `decode` fails on missing bytes and on bytes left over.

pub mod achievements;
pub mod afflictions;
pub mod alc;
pub mod appearance;
pub mod bundle;
pub mod client_add_entry;
pub mod client_input;
pub mod combat;
pub mod components;
pub mod connected;
pub mod containers;
pub mod cooldowns;
pub mod day_phase;
pub mod entity_slots;
pub mod facet;
pub mod gathering;
pub mod gde;
pub mod gear;
pub mod interactor;
pub mod items;
pub mod level_info;
pub mod objectives;
pub mod paperdoll;
pub mod payload;
pub mod player_components;
pub mod position;
pub mod progression;
pub mod self_ident;
pub mod server;
mod slots;
pub mod spawn_point;
pub mod spreadshot;
#[cfg(any(test, feature = "test-util"))]
pub mod test_support;
pub mod time_change;
pub mod time_synch;
pub mod vitals;
pub mod wire;

pub use achievements::{
    ACHIEVEMENT_COMPONENT, ACHIEVEMENT_TYPE_INDEX, AchievementBits, AchievementState, ByteChange,
};
pub use afflictions::{Affliction, AfflictionId, AfflictionList, ColdLong, HotLong, LongForm};
pub use alc::{
    ALC_TYPE_INDEX, AlcChild, AlcStamp, AlcState, DATA_BITS_ACTIVE, FRAG_TAGS_LEN, LayerEntry,
    MobAnimation, MobStep, POS10_CHILD, SLOT0_CHILDREN, SLOT1_CHILDREN, SPAWN_STATE_ID,
    SPAWN_STATE_MS, SPAWN_TELEPORT, ScopeEntry, StepScope, TeleportStep,
};
pub use appearance::{
    LOOK_CHOICES, PLAYER_APPEARANCE_TYPE_INDEX, PlayerAppearanceState, PlayerIconData, PlayerLook,
};
pub use bundle::{
    IndexBlock, LOCAL_PLAYER_INTEREST, MAX_BUNDLE_INDEXES, OBSERVED_BUNDLE_BYTE_4,
    REPLICATED_STATE_BUNDLE_TYPE_INDEX, ReplicatedStateBundle,
};
pub use client_add_entry::{CLIENT_ADD_ENTRY_MSG_TYPE_INDEX, ClientAddEntryMsg};
pub use client_input::{
    Bit8, ClientInputMsg, HeadStep9, INPUT_Z_MAX, INPUT_Z_MIN, InputHead, InputPosition,
    InputRecord, InputSlots, InteractionRequest, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX,
};
pub use combat::{
    DAMAGE_DEALT_TYPE_INDEX, DEATH_RECAP_TYPE_INDEX, DamageDealtMsg, DamageItem, DeathRecapMsg,
    HIT_TAG, MAX_RESPAWN_PAYLOAD, MELEE_HIT_TYPE_INDEX, MeleeDamage, MeleeHitMsg, NEUTRAL_SCALE,
    ON_DAMAGE_TYPE_INDEX, OnDamageMsg, REQUEST_RESPAWN_TYPE_INDEX, RequestRespawnMsg,
};
pub use components::{
    BOSS_PHASE_TYPE_INDEX, COMBAT_STATUS_TYPE_INDEX, CombatStatusState, DAMAGE_RECEIVER_TYPE_INDEX,
    DELAYED_EVENT_TYPE_INDEX, DamageReceiverState, DelayedEventState,
    GATHERABLE_CONTROLLER_TYPE_INDEX, GatherableControllerState, INTERACT_TYPE_INDEX,
    InteractState, LOOK_TARGETING_TYPE_INDEX, LookTargetingState, MANA_TYPE_INDEX, ManaState,
    STAMINA_TYPE_INDEX, StaminaState, boss_phase_change,
};
pub use connected::{
    Block1415acc30, CALENDAR_CONNECTED_MSG_TYPE_INDEX, CalendarConnectedMsg, ConnectedBody,
    TIMING_CONFIGURATION_CONNECTED_MSG_TYPE_INDEX, TimingConfigurationConnectedMsg,
};
pub use containers::{
    BAG_COMPONENT, CONTAINER_TYPE_INDEX, ContainerState, ITEM_MANAGEMENT_TYPE_INDEX,
    ItemManagementState, OwnedItem,
};
pub use cooldowns::{
    COOLDOWNS_COMPONENT, COOLDOWNS_TYPE_INDEX, ConsumableCooldown, CooldownsState,
};
pub use day_phase::{OBSERVED_DAY_PHASE, SYNC_DAY_PHASE_MSG_TYPE_INDEX, SyncDayPhaseMsg};
pub use entity_slots::{EntitySlots, SlotRead, SlotUpdate};
pub use facet::{
    AdjustFxScriptsMsg, ClientFacetMsg, CutsceneEndFinalizedMsg, CutsceneGrantedMsg,
    DisableCollisionMsg, FacetHeader, GatheringInterruptedMsg, InteractionEndedMsg,
    InteractionValidatedMsg, PlayerSeenByAiMsg, SelectedAsTargetMsg, TargetMsg,
    TeleportWithLoadingScreenMsg, UnselectedAsTargetMsg, UpdatePhasingMsg, facet_entity,
};
pub use gathering::{GATHERING_TYPE_INDEX, GatheringState, NO_GATHERABLE};
pub use gde::{
    AssetId, DYNAMIC_SLICE_SUB_ID, GDE_ID_LEN, GDE_METADATA_COMPONENT, GDE_METADATA_TYPE_INDEX,
    GdeMetadataReplicatedState, PLAYER_SLICE_ASSET,
};
pub use gear::{EquippedItem, GearItem, Hands, PlayerGear, STARTER_BAG_SLOTS, is_visible_slot};
pub use interactor::{INTERACTOR_TYPE_INDEX, InteractorState};
pub use items::{FullList, ItemRecord};
pub use level_info::{
    LEVEL_INFO_CHANGED_MSG_TYPE_INDEX, LEVELS, LOGIN_CLIENT_CONTEXT, LevelInfoChangedMsg,
    LevelSpec, OPEN_WORLD_LEVEL, PhasingEntry, TUTORIAL_LEVEL,
};
pub use objectives::{
    OBJECTIVES_COMPONENT, OBJECTIVES_TYPE_INDEX, ObjectiveRecord, ObjectivesState, TaskEntry,
    TaskWireState,
};
pub use paperdoll::{
    ItemVisual, LoadedAmmo, Loadout, PAPERDOLL_TYPE_INDEX, PaperdollOwner, PaperdollPublic,
    PaperdollState, SlotBits,
};
pub use payload::{BundleGroup, BundleMember, BundlePayload, KnownMembers, MemberState};
pub use player_components::{
    EmptyState, HealthChange, PLAYER_COMPONENT_TYPE_INDEX, PLAYER_EMPTY_COMPONENTS,
    PLAYER_VITALS_COMPONENT, PlayerComponentState, PlayerStates, change_members, empty_state,
    gear_members, player_members,
};
pub use position::{
    POSITION_IN_THE_WORLD_TYPE_INDEX, POSITION_Z_MAX, POSITION_Z_MIN,
    PositionInTheWorldReplicatedState, Rotation, RotationComponent, Scale, WorldPosition,
};
pub use progression::{PROGRESSION_COMPONENT, PROGRESSION_TYPE_INDEX, ProgressionState};
pub use self_ident::{
    PLAYER_MANAGER_SELF_IDENTIFICATION_MSG_TYPE_INDEX, PlayerManagerSelfIdentificationMsg,
};
pub use server::{LOGIN_MESSAGE_TYPES, SERVER_MESSAGE_TYPES, ServerMessage};
pub use spawn_point::{RECEIVE_PLAYER_SPAWN_POINT_MSG_TYPE_INDEX, ReceivePlayerSpawnPointMsg};
pub use spreadshot::{
    SPREADSHOT_PROCESSED_TYPE_INDEX, SPREADSHOT_REQUEST_TYPE_INDEX, SpreadHit,
    SpreadshotProcessedMsg, SpreadshotRequestMsg,
};
pub use time_change::{
    OBSERVED_TIME_CHANGE_F32S_AT_16, OBSERVED_TIME_CHANGE_F32S_AT_60,
    OBSERVED_TIME_CHANGE_U32S_AT_28, SYNC_TIME_CHANGE_MSG_TYPE_INDEX, SyncTimeChangeMsg,
};
pub use time_synch::{NS_PER_MS, TimeSynchMsg};
pub use vitals::{
    FIRST_DAMAGE_FLAGS, PLAYER_VITALS_ID, VITALS_TYPE_INDEX, VitalsData, VitalsSlot0, VitalsSlot1,
    VitalsState,
};
pub use wire::{
    BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer, entity_of_raw_id, raw_id_of_entity,
};
