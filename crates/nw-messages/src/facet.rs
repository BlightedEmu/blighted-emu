// SPDX-License-Identifier: AGPL-3.0-or-later
//! Facet calls: messages to or from one component of the player's entity (tutorial-intro note,
//! sections 0 and 2).
//!
//! Every facet body starts with two big-endian `u64`s: the target id, then the entity. The entity
//! is the first 8 bytes of the `ClientAddEntryMsg` entry id in reverse byte order. The target id
//! is the entity plus the `Id` of the component in the player prefab; for a component on a child
//! entity of the prefab, the entity is first XORed with (main entity `Id` XOR child entity `Id`)
//! (vitals-facet note). The client looks the target id up and drops a call whose target it does
//! not know.

use crate::level_info::PhasingEntry;
use crate::wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};

/// The target-id constant of each component of the player entity (Soul Survivor session).
pub mod component {
    /// `TimeComponent`.
    pub const TIME: u64 = 0xd41e_b32c_0f15_4004;
    /// `PlayerComponent`.
    pub const PLAYER: u64 = 0xdf0c_e8c9_8d27_619d;
    /// `PlayerCutsceneComponent`.
    pub const PLAYER_CUTSCENE: u64 = 0xd499_3151_0e1d_d23f;
    /// `GridTrackingComponent`.
    pub const GRID_TRACKING: u64 = 0x3946_bc0c_0a40_83da;
    /// `DamageReceiverComponent` (mob-cards note, section 4.1).
    pub const DAMAGE_RECEIVER: u64 = 0xccc4_7f92_daf6_bfc1;
    /// `StatusEffectsComponent`, on the child entity "Stats" ([`STATS_CHILD`]): 62,354 retail
    /// calls of 606 players (facets note). On the tutorial capture's player it equals the main
    /// entity plus `0x198b_1950_2047_20ba`, which live 2026-09-16 the client could not find.
    pub const STATUS_EFFECTS: u64 = 0x5630_dc38_1f27_432b;
    /// `VitalsComponent`, on the child entity "Stats" ([`STATS_CHILD`]).
    pub const VITALS: u64 = 0xeeef_fb82_abaf_e4b1;
    /// The XOR of the player prefab's main entity `RootPlayer` and its child `Stats`.
    pub const STATS_CHILD: u64 = 0x3caa_c368_06e0_2db3;
    /// `InteractorComponent` (live-tutorial note, section 7).
    pub const INTERACTOR: u64 = 0x6c57_18db_fe6d_f612;
    /// `GatherableControllerComponent` of the tutorial's ruin chest (live-tutorial note,
    /// section 7).
    pub const CHEST_GATHERABLE_CONTROLLER: u64 = 0x45d5_7d32_25aa_2566;
    /// `AITargetableComponent` of the player.
    pub const AI_TARGETABLE: u64 = 0x5207_d209_ba7d_7dd8;
    /// `TutorialAIComponent` of a tutorial sailor, on its child [`TUTORIAL_AI_CHILD`].
    pub const TUTORIAL_AI: u64 = 0xd128_17b1_c795_98a5;
    /// The XOR of a tutorial sailor's main entity and the child that holds its tutorial AI.
    pub const TUTORIAL_AI_CHILD: u64 = 0x37 << 32;
    /// `CraftingComponent` (beach-quests note: the 2993 and 2997 target less the Soul entity).
    pub const CRAFTING: u64 = 0xf7e4_6d7c_2327_3ef2;
    /// `WhisperPlayerComponent` (retail 2026-09-18: the 5070 target less the player entity).
    pub const WHISPER_PLAYER: u64 = 0x5442_5e1a_b303_2637;
    /// `GameModeParticipantComponent` (retail 2026-09-18: the 6426 target less the player
    /// entity).
    pub const GAME_MODE_PARTICIPANT: u64 = 0x8de6_3359_0b8f_bee0;
}

/// Type index of `PlayerComponentServerFacet_OnAckLevelInfoChanged`.
pub const ACK_LEVEL_INFO_TYPE_INDEX: u64 = 5181;
/// Type index of `PlayerCutsceneComponentServerFacet_NotifyCinematicStarted`.
pub const CINEMATIC_STARTED_TYPE_INDEX: u64 = 6606;
/// Type index of `PlayerCutsceneComponentServerFacet_NotifyCinematicEnded`.
pub const CINEMATIC_ENDED_TYPE_INDEX: u64 = 6542;
/// Type index of `PlayerCutsceneComponentServerFacet_OnClientRequestStartCutscene`.
pub const REQUEST_CUTSCENE_TYPE_INDEX: u64 = 6703;
/// Type index of `PlayerCutsceneComponentServerFacet_OnClientCutsceneEnded`.
pub const CUTSCENE_ENDED_TYPE_INDEX: u64 = 6705;
/// Type index of `PaperdollComponentServerFacet_ServerUnequipItem`.
pub const UNEQUIP_ITEM_TYPE_INDEX: u64 = 3896;
/// Type index of `InteractorComponentServerFacet_RequestCancelCommittedInteractionFromClient`.
pub const CANCEL_INTERACTION_TYPE_INDEX: u64 = 2473;
/// Type index of `PaperdollComponentServerFacet_ServerSwapItems`.
pub const SWAP_ITEMS_TYPE_INDEX: u64 = 3900;
/// Type index of `TimeComponentServerFacet_SyncToClient`: the client asks for the time of day.
pub const TIME_SYNC_REQUEST_TYPE_INDEX: u64 = 4248;
/// Type index of `PlayerArenaComponentServerFacet_NotifyPlayerOfFtueQTECompletion`.
pub const QTE_COMPLETED_TYPE_INDEX: u64 = 6638;
/// Type index of `PlayerComponentServerFacet_TriggerAreaEntityPassthrough`.
pub const TRIGGER_AREA_PASSTHROUGH_TYPE_INDEX: u64 = 560;
/// Type index of `ObjectivesComponentServerFacet_NotifyOpenUiScreen`.
pub const UI_SCREEN_OPENED_TYPE_INDEX: u64 = 2617;
/// Type index of `PlayerTradeComponentServerFacet_TakeAllItems`.
pub const TAKE_ALL_ITEMS_TYPE_INDEX: u64 = 2939;
/// Type index of `PaperdollComponentServerFacet_ServerEquipItem`.
pub const EQUIP_ITEM_TYPE_INDEX: u64 = 3895;
/// Type index of `PlayerComponentServerFacet_ReturnToMainWorld`.
pub const RETURN_TO_MAIN_WORLD_TYPE_INDEX: u64 = 502;
/// Type index of `GridTrackingComponentClientFacet_UpdatePhasing`.
pub const UPDATE_PHASING_TYPE_INDEX: u64 = 6310;
/// Type index of `PlayerCutsceneComponentClientFacet_OnServerCutsceneRequestGranted`.
pub const CUTSCENE_GRANTED_TYPE_INDEX: u64 = 6707;
/// Type index of `PlayerCutsceneComponentClientFacet_OnServerCutsceneEndRequestFinalized`.
pub const CUTSCENE_END_FINALIZED_TYPE_INDEX: u64 = 6709;
/// Type index of `InteractorComponentClientFacet_SyncInteractionValidationResult`.
pub const INTERACTION_VALIDATED_TYPE_INDEX: u64 = 3743;
/// Type index of `InteractorComponentClientFacet_CancelCurrentInteraction`.
pub const INTERACTION_ENDED_TYPE_INDEX: u64 = 3746;
/// Type index of `GatherableControllerComponentClientFacet_GatheringInterrupted`.
pub const GATHERING_INTERRUPTED_TYPE_INDEX: u64 = 2724;
/// Type index of `TutorialAIComponentClientFacet_OnPlayerSeenByAI`.
pub const PLAYER_SEEN_BY_AI_TYPE_INDEX: u64 = 955;
/// Type index of `AITargetableComponentClientFacet_OnSelectedAsTarget`.
pub const SELECTED_AS_TARGET_TYPE_INDEX: u64 = 2415;
/// Type index of `AITargetableComponentClientFacet_OnUnSelectedAsTarget`.
pub const UNSELECTED_AS_TARGET_TYPE_INDEX: u64 = 3916;
/// Type index of `StatusEffectsComponentClientFacet_AdjustFXScripts`.
pub const ADJUST_FX_SCRIPTS_TYPE_INDEX: u64 = 608;
/// Type index of `CharacterComponentClientFacet_RequestDisableCollision`.
pub const DISABLE_COLLISION_TYPE_INDEX: u64 = 1793;
/// Type index of `PlayerComponentClientFacet_OnTeleportWithLoadingScreen`.
pub const TELEPORT_WITH_LOADING_SCREEN_TYPE_INDEX: u64 = 3951;
/// Type index of `ConversationComponentServerFacet_RequestAcceptObjective`.
pub const ACCEPT_OBJECTIVE_TYPE_INDEX: u64 = 3772;
/// Type index of `ConversationComponentServerFacet_RequestCompleteObjective`.
pub const COMPLETE_OBJECTIVE_TYPE_INDEX: u64 = 794;
/// Type index of `CraftingComponentServerFacet_SetCraftingDetails`.
pub const SET_CRAFTING_DETAILS_TYPE_INDEX: u64 = 2997;
/// Type index of `CraftingComponentServerFacet_CancelCrafting`.
pub const CANCEL_CRAFTING_TYPE_INDEX: u64 = 2999;
/// Type index of `CraftingComponentClientFacet_OnCraftingProgressUpdate`.
pub const CRAFTING_PROGRESS_TYPE_INDEX: u64 = 2993;
/// Type index of `TerritoryDetectorComponentServerFacet_AttemptTerritoryUnlock`.
pub const TERRITORY_ENTERED_TYPE_INDEX: u64 = 2297;
/// Type index of `TerritoryDetectorComponentServerFacet_AttemptTerritoryExit`.
pub const TERRITORY_EXITED_TYPE_INDEX: u64 = 6416;
/// Type index of `WhisperPlayerComponentClientFacet_RequestStartWhisper`.
pub const START_WHISPER_TYPE_INDEX: u64 = 5070;
/// Type index of `GameModeParticipantComponentClientFacet_OnGameModeSchedulerRefreshed`.
pub const GAME_MODE_SCHEDULER_REFRESHED_TYPE_INDEX: u64 = 6426;
/// Type index of `PlayerComponentServerFacet_RequestSetIsLookingThroughLoadout`.
pub const LOOKING_THROUGH_LOADOUT_TYPE_INDEX: u64 = 3970;
/// The ingredient kind of an item named by its id (beach-quests note: every captured ingredient).
pub const INGREDIENT_ITEM: u8 = 1;

/// The facet entity of the entity whose entry id names entity number `entity_number`
/// (`ClientAddEntryMsg::for_entity`): the entry id starts with the big-endian entity number, and
/// the facet entity is those bytes in reverse order.
pub const fn facet_entity(entity_number: u64) -> u64 {
    entity_number.swap_bytes()
}

/// The two ids that start a facet body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FacetHeader {
    /// The target id: the entity plus the component constant.
    pub target: u64,
    /// The entity.
    pub entity: u64,
}

impl FacetHeader {
    /// The header of a call to `component` of the player whose entry id names entity number
    /// `entity_number` (`ClientAddEntryMsg::for_entity`).
    pub const fn of(component: u64, entity_number: u64) -> Self {
        let entity = facet_entity(entity_number);
        Self {
            target: entity.wrapping_add(component),
            entity,
        }
    }

    /// The header of a call to `component` on the child entity `child` (the XOR of its `Id` with
    /// the main entity's) of the player of entity number `entity_number`.
    pub const fn of_child(component: u64, child: u64, entity_number: u64) -> Self {
        let entity = entity_number.swap_bytes();
        Self {
            target: (entity ^ child).wrapping_add(component),
            entity,
        }
    }

    /// The entity number of the header's entity (the inverse of [`FacetHeader::of`]).
    pub const fn entity_number(&self) -> u64 {
        self.entity.swap_bytes()
    }

    /// Reads the header.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Truncated`] when the body is shorter than the header.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            target: reader.u64()?,
            entity: reader.u64()?,
        })
    }

    /// Writes the header.
    pub fn write(&self, writer: &mut Writer) {
        writer.u64(self.target).u64(self.entity);
    }
}

/// One ingredient of a craft request (2997).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CraftIngredient {
    /// Its kind ([`INGREDIENT_ITEM`] for an item).
    pub kind: u8,
    /// How many.
    pub quantity: u32,
    /// CRC-32 of the lowercase item id.
    pub item: u32,
}

/// A facet call from the client that the server answers or acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientFacetMsg {
    /// 5181: the client loaded the level of a `LevelInfoChangedMsg` with this client context.
    AckLevelInfo {
        /// The header.
        header: FacetHeader,
        /// The acknowledged client context.
        client_context: u8,
    },
    /// 6606: a cinematic video started; its id is the CRC-32 of its lowercase entry id.
    CinematicStarted {
        /// The header.
        header: FacetHeader,
        /// The cinematic.
        cinematic: u32,
    },
    /// 6542: a cinematic video ended; its id is the CRC-32 of its lowercase entry id.
    CinematicEnded {
        /// The header.
        header: FacetHeader,
        /// The cinematic.
        cinematic: u32,
    },
    /// 6703: the client asks to start a cutscene; its id is the CRC-32 of its lowercase name.
    RequestCutscene {
        /// The header.
        header: FacetHeader,
        /// The cutscene.
        cutscene: u32,
    },
    /// 6705: the client's cutscene ended.
    CutsceneEnded {
        /// The header.
        header: FacetHeader,
    },
    /// 4248: the client's time component asks for the time of day (facets note: retail answers
    /// with 4246 and 4247 at entry and after the exit, and sends them at no other time).
    TimeSyncRequest {
        /// The header: the player's `TimeComponent`.
        header: FacetHeader,
    },
    /// 6638: the client finished a tutorial quick-time sequence; its id is the CRC-32 of its
    /// lowercase name.
    QteCompleted {
        /// The header.
        header: FacetHeader,
        /// The sequence.
        sequence: u32,
    },
    /// 560: the player passed through the client-side trigger area of a static level entity
    /// (ftue-static-entities note, section 3).
    TriggerAreaPassthrough {
        /// The header.
        header: FacetHeader,
        /// The entity id of the trigger area.
        entity: [u8; RAW_ID_LEN],
        /// Its facet entity.
        entity_facet: u64,
        /// An id of unknown meaning.
        extra: u64,
    },
    /// 2617: the client opened a UI screen; its id is the CRC-32 of its lowercase name (`hud`,
    /// `inventory`, ...; live-tutorial note, section 2).
    UiScreenOpened {
        /// The header.
        header: FacetHeader,
        /// The screen.
        screen: u32,
    },
    /// 2939: the Take All button of a loot window: the client asks for every item of the
    /// container (live-tutorial note, section 5.2).
    TakeAllItems {
        /// The header.
        header: FacetHeader,
        /// The container's entity id.
        container: [u8; RAW_ID_LEN],
        /// The container's facet entity.
        container_entity: u64,
        /// An id of unknown meaning.
        extra: u64,
    },
    /// 3895: the client asks to equip the item in bag slot `bag_slot` into paperdoll slot
    /// `equip_slot` (live-tutorial note, section 5.3).
    EquipItem {
        /// The header.
        header: FacetHeader,
        /// The bag slot of the item.
        bag_slot: u32,
        /// The paperdoll slot.
        equip_slot: u32,
        /// How many to equip (a stack).
        quantity: u32,
        /// The item instance id.
        instance: [u8; RAW_ID_LEN],
        /// A byte of unknown meaning (0 in the capture).
        flag: u8,
    },
    /// 3900: the client asks to swap the items of two paperdoll slots (ranged note; an all-zero
    /// instance is an empty slot).
    SwapItems {
        /// The header.
        header: FacetHeader,
        /// The first slot.
        slot_a: u32,
        /// The second slot.
        slot_b: u32,
        /// The instance id of the first slot's item.
        instance_a: [u8; RAW_ID_LEN],
        /// The instance id of the second slot's item.
        instance_b: [u8; RAW_ID_LEN],
        /// A byte of unknown meaning (0 in the captures).
        flag: u8,
    },
    /// 3896: the client asks to take off the item of a paperdoll slot (ranged note).
    UnequipItem {
        /// The header.
        header: FacetHeader,
        /// The paperdoll slot.
        slot: u32,
        /// The bag slot to put it in; `u32::MAX` for the first free one (Inferred).
        bag_slot: u32,
        /// How many (Inferred; 0 and 1 in the captures).
        quantity: u32,
        /// A byte of unknown meaning (1 in the captures).
        flag: u8,
    },
    /// 2473: the client ends its interaction: its acknowledgement of the server's end (15 to 25
    /// ms after each 3746), or its cancel when it gives up or walks away (gather note).
    CancelInteraction {
        /// The header.
        header: FacetHeader,
        /// The object's entity id.
        target: [u8; RAW_ID_LEN],
        /// Its facet entity.
        target_entity: u64,
        /// The interaction id.
        interaction: u64,
    },
    /// 3970: the inventory (loadout) opened or closed. While it is open the client camera
    /// glides to the inventory view, with the look frozen (retail 2026-09-18: camera z 24.41 to
    /// 22.80 at pitch 0.766 while the server held the player still).
    LookingThroughLoadout {
        /// The header (the player's `PlayerComponent`).
        header: FacetHeader,
        /// Whether the inventory is open.
        looking: bool,
    },
    /// 502: the client asks to leave the tutorial for the open world.
    ReturnToMainWorld {
        /// The header.
        header: FacetHeader,
        /// A byte of unknown meaning (0 in the capture).
        flag: u8,
    },
    /// 3772: the player accepts the quest an NPC offers (beach-quests note, section 2).
    AcceptObjective {
        /// The header.
        header: FacetHeader,
        /// CRC-32 of the lowercase objective id.
        objective: u32,
    },
    /// 794: the player turns a finished quest in at its NPC.
    CompleteObjective {
        /// The header.
        header: FacetHeader,
        /// CRC-32 of the lowercase objective id.
        objective: u32,
    },
    /// 2997: the player crafts at the station it uses.
    SetCraftingDetails {
        /// The header (the player's `CraftingComponent`).
        header: FacetHeader,
        /// CRC-32 of the lowercase recipe id.
        recipe: u32,
        /// How many to craft.
        quantity: u32,
        /// A number of unknown meaning (0 in the captures).
        unknown: u32,
        /// A number of unknown meaning: `0x0064_0000` for the tools, 0 for food (Inferred: its
        /// high half is the tools' gear score 100).
        details: u32,
        /// The ingredients the player picked.
        ingredients: Vec<CraftIngredient>,
    },
    /// 2999: the player closes the crafting window.
    CancelCrafting {
        /// The header.
        header: FacetHeader,
    },
    /// 2297: the player came into a territory area: a point of interest or a larger area
    /// (retail 2026-09-18: one call per area entered, three at the level load; retail sends no
    /// answer).
    TerritoryEntered {
        /// The header (the player's `TerritoryDetectorComponent`).
        header: FacetHeader,
        /// The entity id of the area.
        area: [u8; RAW_ID_LEN],
        /// Its facet entity.
        area_entity: u64,
        /// An id of unknown meaning.
        extra: u64,
        /// The `TerritoryID` of the area (retail: 40148, the start beach's watchtower).
        territory: u16,
        /// A number of unknown meaning (0 at the first entry, other values later).
        unknown: u32,
    },
    /// 6416: the player left a territory area (retail sends no answer).
    TerritoryExited {
        /// The header (the player's `TerritoryDetectorComponent`).
        header: FacetHeader,
        /// The entity id of the area.
        area: [u8; RAW_ID_LEN],
        /// Its facet entity.
        area_entity: u64,
        /// An id of unknown meaning.
        extra: u64,
        /// The `TerritoryID` of the area.
        territory: u16,
    },
}

impl ClientFacetMsg {
    /// Decodes `body` when `type_index` is a call this server reads; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            ACK_LEVEL_INFO_TYPE_INDEX => read(|header, reader| {
                Ok(Self::AckLevelInfo {
                    header,
                    client_context: reader.u8()?,
                })
            }),
            CINEMATIC_STARTED_TYPE_INDEX => read(|header, reader| {
                Ok(Self::CinematicStarted {
                    header,
                    cinematic: reader.u32()?,
                })
            }),
            TRIGGER_AREA_PASSTHROUGH_TYPE_INDEX => read(|header, reader| {
                Ok(Self::TriggerAreaPassthrough {
                    header,
                    entity: reader.raw_id()?,
                    entity_facet: reader.u64()?,
                    extra: reader.u64()?,
                })
            }),
            UI_SCREEN_OPENED_TYPE_INDEX => read(|header, reader| {
                Ok(Self::UiScreenOpened {
                    header,
                    screen: reader.u32()?,
                })
            }),
            TAKE_ALL_ITEMS_TYPE_INDEX => read(|header, reader| {
                Ok(Self::TakeAllItems {
                    header,
                    container: reader.raw_id()?,
                    container_entity: reader.u64()?,
                    extra: reader.u64()?,
                })
            }),
            EQUIP_ITEM_TYPE_INDEX => read(|header, reader| {
                Ok(Self::EquipItem {
                    header,
                    bag_slot: reader.u32()?,
                    equip_slot: reader.u32()?,
                    quantity: reader.u32()?,
                    instance: reader.raw_id()?,
                    flag: reader.u8()?,
                })
            }),
            SWAP_ITEMS_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SwapItems {
                    header,
                    slot_a: reader.u32()?,
                    slot_b: reader.u32()?,
                    instance_a: reader.raw_id()?,
                    instance_b: reader.raw_id()?,
                    flag: reader.u8()?,
                })
            }),
            UNEQUIP_ITEM_TYPE_INDEX => read(|header, reader| {
                Ok(Self::UnequipItem {
                    header,
                    slot: reader.u32()?,
                    bag_slot: reader.u32()?,
                    quantity: reader.u32()?,
                    flag: reader.u8()?,
                })
            }),
            CANCEL_INTERACTION_TYPE_INDEX => read(|header, reader| {
                Ok(Self::CancelInteraction {
                    header,
                    target: reader.raw_id()?,
                    target_entity: reader.u64()?,
                    interaction: reader.u64()?,
                })
            }),
            CINEMATIC_ENDED_TYPE_INDEX => read(|header, reader| {
                Ok(Self::CinematicEnded {
                    header,
                    cinematic: reader.u32()?,
                })
            }),
            REQUEST_CUTSCENE_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestCutscene {
                    header,
                    cutscene: reader.u32()?,
                })
            }),
            CUTSCENE_ENDED_TYPE_INDEX => read(|header, _| Ok(Self::CutsceneEnded { header })),
            TIME_SYNC_REQUEST_TYPE_INDEX => read(|header, _| Ok(Self::TimeSyncRequest { header })),
            QTE_COMPLETED_TYPE_INDEX => read(|header, reader| {
                Ok(Self::QteCompleted {
                    header,
                    sequence: reader.u32()?,
                })
            }),
            LOOKING_THROUGH_LOADOUT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::LookingThroughLoadout {
                    header,
                    looking: reader.u8()? != 0,
                })
            }),
            RETURN_TO_MAIN_WORLD_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ReturnToMainWorld {
                    header,
                    flag: reader.u8()?,
                })
            }),
            ACCEPT_OBJECTIVE_TYPE_INDEX => read(|header, reader| {
                Ok(Self::AcceptObjective {
                    header,
                    objective: reader.u32()?,
                })
            }),
            COMPLETE_OBJECTIVE_TYPE_INDEX => read(|header, reader| {
                Ok(Self::CompleteObjective {
                    header,
                    objective: reader.u32()?,
                })
            }),
            SET_CRAFTING_DETAILS_TYPE_INDEX => read(|header, reader| {
                let recipe = reader.u32()?;
                let quantity = reader.u32()?;
                let unknown = reader.u32()?;
                let details = reader.u32()?;
                let count = reader.u8()?;
                let ingredients = (0..count)
                    .map(|_| {
                        Ok(CraftIngredient {
                            kind: reader.u8()?,
                            quantity: reader.u32()?,
                            item: reader.u32()?,
                        })
                    })
                    .collect::<Result<_, MessageError>>()?;
                Ok(Self::SetCraftingDetails {
                    header,
                    recipe,
                    quantity,
                    unknown,
                    details,
                    ingredients,
                })
            }),
            CANCEL_CRAFTING_TYPE_INDEX => read(|header, _| Ok(Self::CancelCrafting { header })),
            TERRITORY_ENTERED_TYPE_INDEX => read(|header, reader| {
                Ok(Self::TerritoryEntered {
                    header,
                    area: reader.raw_id()?,
                    area_entity: reader.u64()?,
                    extra: reader.u64()?,
                    territory: reader.u16()?,
                    unknown: reader.u32()?,
                })
            }),
            TERRITORY_EXITED_TYPE_INDEX => read(|header, reader| {
                Ok(Self::TerritoryExited {
                    header,
                    area: reader.raw_id()?,
                    area_entity: reader.u64()?,
                    extra: reader.u64()?,
                    territory: reader.u16()?,
                })
            }),
            _ => return None,
        })
    }
}

/// 6310: the phases of the player's sectors (the entries of its `LevelInfoChangedMsg`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePhasingMsg {
    /// The header.
    pub header: FacetHeader,
    /// The entries.
    pub entries: Vec<PhasingEntry>,
    /// A byte of unknown meaning: 3 in the tutorial, 2 in the open world, 0 in an empty form.
    pub mode: u8,
    /// Rises with each update; the client acknowledges it (6745).
    pub sequence: u8,
}

impl BodyCodec for UpdatePhasingMsg {
    const TYPE_INDEX: u64 = UPDATE_PHASING_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        // A `usize` count fits 64 bits.
        writer.wide_prefixed(self.entries.len() as u64);
        for entry in &self.entries {
            entry.write(&mut writer);
        }
        Ok(writer.u8(self.mode).u8(self.sequence).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let count = reader.wide_prefixed()?;
        let mut entries = Vec::new();
        for _ in 0..count {
            entries.push(PhasingEntry::read(&mut reader)?);
        }
        let message = Self {
            header,
            entries,
            mode: reader.u8()?,
            sequence: reader.u8()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 6707: the server allows the cutscene the client asked for; it echoes the asked id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutsceneGrantedMsg {
    /// The header.
    pub header: FacetHeader,
    /// The cutscene id of the request.
    pub cutscene: u32,
}

impl BodyCodec for CutsceneGrantedMsg {
    const TYPE_INDEX: u64 = CUTSCENE_GRANTED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.u32(self.cutscene).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            cutscene: reader.u32()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 6709: the server closes the cutscene that the client ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutsceneEndFinalizedMsg {
    /// The header.
    pub header: FacetHeader,
}

impl BodyCodec for CutsceneEndFinalizedMsg {
    const TYPE_INDEX: u64 = CUTSCENE_END_FINALIZED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 3951: the client shows its loading screen for a level change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeleportWithLoadingScreenMsg {
    /// The header.
    pub header: FacetHeader,
    /// A flag (1 in the capture).
    pub flag: bool,
}

impl BodyCodec for TeleportWithLoadingScreenMsg {
    const TYPE_INDEX: u64 = TELEPORT_WITH_LOADING_SCREEN_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.boolean(self.flag).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            flag: reader.boolean()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 3743: the server's answer to an interaction the client started (ftue-static-entities note,
/// section 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionValidatedMsg {
    /// The header (the player's `InteractorComponent`).
    pub header: FacetHeader,
    /// The entity id of the used object.
    pub target: [u8; RAW_ID_LEN],
    /// Its facet entity.
    pub target_entity: u64,
    /// The interaction id the client sent.
    pub interaction: u64,
    /// The interaction kind (14 to gather).
    pub kind: u8,
    /// The result (1 accepted).
    pub status: u8,
    /// The display name string id of the object.
    pub name: Vec<u8>,
    /// A number of unknown meaning (0 in the capture).
    pub extra: u32,
    /// An id of unknown meaning.
    pub id: [u8; RAW_ID_LEN],
}

impl BodyCodec for InteractionValidatedMsg {
    const TYPE_INDEX: u64 = INTERACTION_VALIDATED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer
            .bytes(&self.target)
            .u64(self.target_entity)
            .u64(self.interaction)
            .u8(self.kind)
            .u8(self.status);
        // A `usize` length fits 64 bits.
        writer.wide_prefixed(self.name.len() as u64);
        Ok(writer
            .bytes(&self.name)
            .u32(self.extra)
            .bytes(&self.id)
            .finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let target = reader.raw_id()?;
        let target_entity = reader.u64()?;
        let interaction = reader.u64()?;
        let kind = reader.u8()?;
        let status = reader.u8()?;
        // A length past the body fails the read below.
        let length = usize::try_from(reader.wide_prefixed()?).unwrap_or(usize::MAX);
        let name = reader.bytes(length)?.to_vec();
        let message = Self {
            header,
            target,
            target_entity,
            interaction,
            kind,
            status,
            name,
            extra: reader.u32()?,
            id: reader.raw_id()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 2993: the progress of the player's craft: the recipe crafted and how many, then an empty
/// update when the station is idle again (beach-quests note: retail sends both 1,057 ms after the
/// request, and one more empty update after the close).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraftProgressMsg {
    /// The header (the player's `CraftingComponent`).
    pub header: FacetHeader,
    /// The recipe id; empty when idle.
    pub recipe: Vec<u8>,
    /// How many were crafted.
    pub quantity: u32,
    /// A number of unknown meaning: 1 with a recipe, 0 when idle.
    pub extra: u32,
}

/// The [`CraftProgressMsg::extra`] of a finished craft.
const CRAFTED_EXTRA: u32 = 1;

impl CraftProgressMsg {
    /// Player `player` crafted `quantity` of recipe `recipe`.
    pub fn crafted(player: u64, recipe: &str, quantity: u32) -> Self {
        Self {
            header: FacetHeader::of(component::CRAFTING, player),
            recipe: recipe.as_bytes().to_vec(),
            quantity,
            extra: CRAFTED_EXTRA,
        }
    }

    /// Player `player`'s station is idle.
    pub const fn idle(player: u64) -> Self {
        Self {
            header: FacetHeader::of(component::CRAFTING, player),
            recipe: Vec::new(),
            quantity: 0,
            extra: 0,
        }
    }
}

impl BodyCodec for CraftProgressMsg {
    const TYPE_INDEX: u64 = CRAFTING_PROGRESS_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        // A `usize` length fits 64 bits.
        writer.wide_prefixed(self.recipe.len() as u64);
        Ok(writer
            .bytes(&self.recipe)
            .u32(self.quantity)
            .u32(self.extra)
            .finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        // A length past the body fails the read below.
        let length = usize::try_from(reader.wide_prefixed()?).unwrap_or(usize::MAX);
        let message = Self {
            header,
            recipe: reader.bytes(length)?.to_vec(),
            quantity: reader.u32()?,
            extra: reader.u32()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// A facet call that is its header only.
macro_rules! header_only_message {
    ($(#[$doc:meta])* $name:ident, $type_index:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name {
            /// The header.
            pub header: FacetHeader,
        }

        impl BodyCodec for $name {
            const TYPE_INDEX: u64 = $type_index;

            fn encode(&self) -> Result<Vec<u8>, MessageError> {
                let mut writer = Writer::new();
                self.header.write(&mut writer);
                Ok(writer.finish())
            }

            fn decode(body: &[u8]) -> Result<Self, MessageError> {
                let mut reader = Reader::new(body);
                let message = Self {
                    header: FacetHeader::read(&mut reader)?,
                };
                reader.finish()?;
                Ok(message)
            }
        }
    };
}

header_only_message!(
    /// 955: a tutorial sailor saw the player; its header names the sailor's tutorial AI.
    PlayerSeenByAiMsg,
    PLAYER_SEEN_BY_AI_TYPE_INDEX
);
header_only_message!(
    /// 1793: a dead creature stops colliding; its header names the creature's
    /// `CharacterComponent`.
    DisableCollisionMsg,
    DISABLE_COLLISION_TYPE_INDEX
);

/// 608: a status effect's visual script on the player turns on or off (retail: the potion's
/// `potionheal` after the drink; xp-potion-exit note, section 2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdjustFxScriptsMsg {
    /// The header (the player's `StatusEffectsComponent`).
    pub header: FacetHeader,
    /// CRC-32 of the lowercase script name.
    pub script: u32,
    /// Whether the script turns on.
    pub on: bool,
}

impl AdjustFxScriptsMsg {
    /// The script of CRC `script` turns on for player `player`.
    pub const fn on(player: u64, script: u32) -> Self {
        Self {
            header: FacetHeader::of_child(
                component::STATUS_EFFECTS,
                component::STATS_CHILD,
                player,
            ),
            script,
            on: true,
        }
    }
}

impl BodyCodec for AdjustFxScriptsMsg {
    const TYPE_INDEX: u64 = ADJUST_FX_SCRIPTS_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.u32(self.script).boolean(self.on).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            script: reader.u32()?,
            on: reader.boolean()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 2415 and 3916: a creature picks the player as its target, or drops it (live-tutorial note,
/// section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetMsg {
    /// The header (the player's `AITargetableComponent`).
    pub header: FacetHeader,
    /// The creature's facet entity.
    pub creature: u64,
}

impl TargetMsg {
    fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.u64(self.creature).finish()
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            creature: reader.u64()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 2415: a creature picked the player as its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedAsTargetMsg(pub TargetMsg);

/// 3916: a creature no longer targets the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnselectedAsTargetMsg(pub TargetMsg);

impl BodyCodec for SelectedAsTargetMsg {
    const TYPE_INDEX: u64 = SELECTED_AS_TARGET_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        Ok(self.0.encode())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        TargetMsg::decode(body).map(Self)
    }
}

impl BodyCodec for UnselectedAsTargetMsg {
    const TYPE_INDEX: u64 = UNSELECTED_AS_TARGET_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        Ok(self.0.encode())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        TargetMsg::decode(body).map(Self)
    }
}

header_only_message!(
    /// 6426: the player's game mode schedule changed (retail 2026-09-18: 27 ms after each quest
    /// turn-in, and twice at the level load).
    GameModeSchedulerRefreshedMsg,
    GAME_MODE_SCHEDULER_REFRESHED_TYPE_INDEX
);

impl GameModeSchedulerRefreshedMsg {
    /// The call to player `player`.
    pub const fn to(player: u64) -> Self {
        Self {
            header: FacetHeader::of(component::GAME_MODE_PARTICIPANT, player),
        }
    }
}

/// 5070: the client plays a whisper, the voice of a game event (retail 2026-09-18: the first
/// whisper of Amidst the Wreckage; the client answers with 5235 and the same id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartWhisperMsg {
    /// The header (the player's `WhisperPlayerComponent`).
    pub header: FacetHeader,
    /// CRC-32 of the lowercase whisper id.
    pub whisper: u32,
}

impl StartWhisperMsg {
    /// Player `player` hears the whisper of CRC `whisper`.
    pub const fn to(player: u64, whisper: u32) -> Self {
        Self {
            header: FacetHeader::of(component::WHISPER_PLAYER, player),
            whisper,
        }
    }
}

impl BodyCodec for StartWhisperMsg {
    const TYPE_INDEX: u64 = START_WHISPER_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        Ok(writer.u32(self.whisper).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let message = Self {
            header: FacetHeader::read(&mut reader)?,
            whisper: reader.u32()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

header_only_message!(
    /// 3746: the interaction is over (the gather time ran out).
    InteractionEndedMsg,
    INTERACTION_ENDED_TYPE_INDEX
);
header_only_message!(
    /// 2724: the gatherable's gathering ends; its header names the gatherable.
    GatheringInterruptedMsg,
    GATHERING_INTERRUPTED_TYPE_INDEX
);

#[cfg(test)]
mod tests {
    use super::*;

    /// The Soul Survivor session: its facet entity and its 6310 body (tutorial phase).
    const SOUL_ENTITY: u64 = 0x650f_7a09_cf50_b550;
    const SOUL_ACTION_LIST_TARGET: u64 = 0xa168_9458_7313_62c5;
    const ACTION_LIST: u64 = 0x3c59_1a4e_a3c2_ad75;
    const SOUL_PHASING: &str = concat!(
        "9e563615d991392a650f7a09cf50b550",
        "01",
        "03",
        "00000000",
        "8b2cbbb86aa99537",
        "00000002",
        "03",
        "01"
    );
    const SOUL_GRANT: &str = "39a8ab5add6e878f650f7a09cf50b550a81bd1af";
    /// The facet entity of the retail new character of 2026-09-18.
    const RETAIL_ENTITY: u64 = 0x2715_f9bc_a18f_dece;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    fn soul(component: u64) -> FacetHeader {
        // The entity number whose entry id reverses to the Soul entity.
        FacetHeader::of(component, SOUL_ENTITY.swap_bytes())
    }

    #[test]
    fn the_quest_calls_read_the_retail_bodies() {
        // Retail 3772 and 794 of 12_skinningknife (`f52332f7`), beach-quests note.
        const ACCEPT: &str = "8120e72f6a93d58d650f7a09cf50b550f52332f7";
        const SKINNING_KNIFE: u32 = 0xf523_32f7;
        let header = FacetHeader {
            target: 0x8120_e72f_6a93_d58d,
            entity: SOUL_ENTITY,
        };
        assert_eq!(
            ClientFacetMsg::decode(ACCEPT_OBJECTIVE_TYPE_INDEX, &hex(ACCEPT)),
            Some(Ok(ClientFacetMsg::AcceptObjective {
                header,
                objective: SKINNING_KNIFE
            }))
        );
        assert_eq!(
            ClientFacetMsg::decode(COMPLETE_OBJECTIVE_TYPE_INDEX, &hex(ACCEPT)),
            Some(Ok(ClientFacetMsg::CompleteObjective {
                header,
                objective: SKINNING_KNIFE
            }))
        );
    }

    #[test]
    fn the_loadout_call_reads_the_live_bodies() {
        // Live 2026-09-18: the inventory opened at the tutorial bag, then closed.
        const OPENED: &str = "8e03e9c98d27619daef701000000000001";
        const CLOSED: &str = "8e03e9c98d27619daef701000000000000";
        let header = FacetHeader {
            target: 0x8e03_e9c9_8d27_619d,
            entity: 0xaef7_0100_0000_0000,
        };
        assert_eq!(
            ClientFacetMsg::decode(LOOKING_THROUGH_LOADOUT_TYPE_INDEX, &hex(OPENED)),
            Some(Ok(ClientFacetMsg::LookingThroughLoadout {
                header,
                looking: true
            }))
        );
        assert_eq!(
            ClientFacetMsg::decode(LOOKING_THROUGH_LOADOUT_TYPE_INDEX, &hex(CLOSED)),
            Some(Ok(ClientFacetMsg::LookingThroughLoadout {
                header,
                looking: false
            }))
        );
    }

    #[test]
    fn the_territory_calls_read_the_retail_bodies() {
        // Retail 2026-09-18: the watchtower (40148) entered at the accept of Crewmates No More,
        // and left on the way to the wreck.
        const ENTERED: &str = concat!(
            "dd2cb35d4a60bfb02715f9bca18fdece",
            "33cb5ec44ca08ecf0570d26a6694afc5cf8ea04cc45ecb331f3ec1f5325679ad",
            "9cd4",
            "00000000"
        );
        const WATCHTOWER: u16 = 40_148;
        let header = FacetHeader {
            target: 0xdd2c_b35d_4a60_bfb0,
            entity: RETAIL_ENTITY,
        };
        let area = hex("33cb5ec44ca08ecf0570d26a6694afc5")
            .try_into()
            .expect("an id");
        assert_eq!(
            ClientFacetMsg::decode(TERRITORY_ENTERED_TYPE_INDEX, &hex(ENTERED)),
            Some(Ok(ClientFacetMsg::TerritoryEntered {
                header,
                area,
                area_entity: 0xcf8e_a04c_c45e_cb33,
                extra: 0x1f3e_c1f5_3256_79ad,
                territory: WATCHTOWER,
                unknown: 0,
            }))
        );
        assert_eq!(
            ClientFacetMsg::decode(TERRITORY_EXITED_TYPE_INDEX, &hex(&ENTERED[..100])),
            Some(Ok(ClientFacetMsg::TerritoryExited {
                header,
                area,
                area_entity: 0xcf8e_a04c_c45e_cb33,
                extra: 0x1f3e_c1f5_3256_79ad,
                territory: WATCHTOWER,
            }))
        );
    }

    #[test]
    fn the_whisper_and_the_schedule_refresh_are_retail() {
        // Retail 2026-09-18: 5070 of `01_IsabellasFirstWhisper_Isabella_01` and 6426 after a
        // turn-in.
        const WHISPER: &str = "7b5857d7549305052715f9bca18fdece52f36e05";
        const REFRESHED: &str = "b4fc2d15ad1f9dae2715f9bca18fdece";
        let player = RETAIL_ENTITY.swap_bytes();
        let whisper = StartWhisperMsg::to(player, 0x52f3_6e05);
        assert_eq!(whisper.encode().expect("encode"), hex(WHISPER));
        assert_eq!(StartWhisperMsg::decode(&hex(WHISPER)), Ok(whisper));
        assert_eq!(
            GameModeSchedulerRefreshedMsg::to(player)
                .encode()
                .expect("encode"),
            hex(REFRESHED)
        );
    }

    #[test]
    fn a_craft_request_reads_its_ingredients_and_the_answer_is_retail() {
        // Retail 2997 of the skinning knife: FlintT1 x1 and WoodT1 x1 (beach-quests note).
        const REQUEST: &str = concat!(
            "5cf3e785f277f442650f7a09cf50b550",
            "a546f7c5",
            "00000001",
            "00000000",
            "00640000",
            "02",
            "01000000010c9280c8",
            "01000000018a914427"
        );
        const CRAFTED: &str = concat!(
            "5cf3e785f277f442650f7a09cf50b550",
            "11316853",
            "6b696e6e696e674b6e69666554310000000100000001"
        );
        const IDLE: &str = "5cf3e785f277f442650f7a09cf50b550000000000000000000";
        let header = soul(component::CRAFTING);
        assert_eq!(
            ClientFacetMsg::decode(SET_CRAFTING_DETAILS_TYPE_INDEX, &hex(REQUEST)),
            Some(Ok(ClientFacetMsg::SetCraftingDetails {
                header,
                recipe: 0xa546_f7c5,
                quantity: 1,
                unknown: 0,
                details: 0x0064_0000,
                ingredients: vec![
                    CraftIngredient {
                        kind: INGREDIENT_ITEM,
                        quantity: 1,
                        item: 0x0c92_80c8,
                    },
                    CraftIngredient {
                        kind: INGREDIENT_ITEM,
                        quantity: 1,
                        item: 0x8a91_4427,
                    },
                ],
            }))
        );
        assert_eq!(
            ClientFacetMsg::decode(CANCEL_CRAFTING_TYPE_INDEX, &hex(&IDLE[..32])),
            Some(Ok(ClientFacetMsg::CancelCrafting { header }))
        );
        let player = SOUL_ENTITY.swap_bytes();
        let crafted = CraftProgressMsg::crafted(player, "1hSkinningKnifeT1", 1);
        assert_eq!(crafted.encode().expect("encode"), hex(CRAFTED));
        assert_eq!(
            CraftProgressMsg::idle(player).encode().expect("encode"),
            hex(IDLE)
        );
        assert_eq!(
            CraftProgressMsg::decode(&hex(CRAFTED)).expect("decode"),
            crafted
        );
    }

    #[test]
    fn the_target_is_the_entity_plus_the_component() {
        let header = soul(ACTION_LIST);
        assert_eq!(header.entity, SOUL_ENTITY);
        assert_eq!(header.target, SOUL_ACTION_LIST_TARGET);
    }

    #[test]
    fn a_vitals_call_goes_to_the_stats_child() {
        // Retail sessions (vitals-facet note): player facet entity and 3601 target.
        const SESSIONS: [(u64, u64); 3] = [
            (0xe34e_14f3_44a9_74cc, 0xced4_d31d_edf9_3e30),
            (0x2896_c8c1_4210_296e, 0x032c_072b_f09f_e98e),
            (0x67e7_d282_99ff_40aa, 0x4a3d_0d6d_4acf_51ca),
        ];
        for (player, target) in SESSIONS {
            let header = FacetHeader::of_child(
                component::VITALS,
                component::STATS_CHILD,
                player.swap_bytes(),
            );
            assert_eq!((header.target, header.entity), (target, player));
        }
    }

    #[test]
    fn phasing_and_grant_are_the_retail_bytes() {
        let phasing = UpdatePhasingMsg {
            header: soul(component::GRID_TRACKING),
            entries: vec![PhasingEntry {
                key_type: 3,
                key_id: 0,
                phase_id: 0x8b2c_bbb8_6aa9_9537,
                extra: 2,
            }],
            mode: 3,
            sequence: 1,
        };
        let body = phasing.encode().expect("encode");
        assert_eq!(body, hex(SOUL_PHASING));
        assert_eq!(UpdatePhasingMsg::decode(&body), Ok(phasing));
        let grant = CutsceneGrantedMsg {
            header: soul(component::PLAYER_CUTSCENE),
            cutscene: 0xa81b_d1af,
        };
        assert_eq!(grant.encode().expect("encode"), hex(SOUL_GRANT));
    }

    #[test]
    fn the_ai_messages_are_the_retail_bytes() {
        // Live tutorial (live-tutorial note, section 7): player facet, sailor 1, and the 2415
        // and 955 bodies.
        const PLAYER: u64 = 0xbfa9_6f3e_a2e3_6a5a;
        const SAILOR: u64 = 0xe5be_db8e_4d15_10aa;
        const SELECTED: &str = "11b141485d60e832bfa96f3ea2e36a5ae5bedb8e4d1510aa";
        const SEEN: &str = "b6e6f36b14aaa94fe5bedb8e4d1510aa";
        let selected = SelectedAsTargetMsg(TargetMsg {
            header: FacetHeader::of(component::AI_TARGETABLE, PLAYER.swap_bytes()),
            creature: SAILOR,
        });
        assert_eq!(selected.encode().expect("encode"), hex(SELECTED));
        assert_eq!(SelectedAsTargetMsg::decode(&hex(SELECTED)), Ok(selected));
        let seen = PlayerSeenByAiMsg {
            header: FacetHeader::of_child(
                component::TUTORIAL_AI,
                component::TUTORIAL_AI_CHILD,
                SAILOR.swap_bytes(),
            ),
        };
        assert_eq!(seen.encode().expect("encode"), hex(SEEN));
    }

    #[test]
    fn the_interaction_answer_round_trips() {
        const GATHER: u8 = 14;
        let answer = InteractionValidatedMsg {
            header: soul(component::INTERACTOR),
            target: [0xef; RAW_ID_LEN],
            target_entity: 0x5870_3581_9fa0_a5ef,
            interaction: 0xd296_06d2_31fe_d9f2,
            kind: GATHER,
            status: 1,
            name: b"@LootContainer_AncientSmall_DisplayName".to_vec(),
            extra: 0,
            id: [0x1b; RAW_ID_LEN],
        };
        let body = answer.encode().expect("encode");
        // Header 16, target 16, entity 8, interaction 8, kind and status: the retail name
        // length 0x27 follows at byte 50.
        const NAME_LENGTH_AT: usize = 50;
        assert_eq!(
            body[NAME_LENGTH_AT],
            u8::try_from(answer.name.len()).expect("len")
        );
        assert_eq!(InteractionValidatedMsg::decode(&body), Ok(answer));
        let ended = InteractionEndedMsg {
            header: soul(component::INTERACTOR),
        };
        assert_eq!(
            InteractionEndedMsg::decode(&ended.encode().expect("encode")),
            Ok(ended)
        );
    }

    #[test]
    fn bag_and_equip_calls_decode() {
        const BAG_SLOT: u32 = 2;
        const MAIN_HAND_OPTION_2: u32 = 25;
        let instance = [0xaf; RAW_ID_LEN];
        let mut writer = Writer::new();
        soul(component::PLAYER).write(&mut writer);
        let equip = writer
            .u32(BAG_SLOT)
            .u32(MAIN_HAND_OPTION_2)
            .u32(1)
            .bytes(&instance)
            .u8(0)
            .finish();
        assert_eq!(
            ClientFacetMsg::decode(EQUIP_ITEM_TYPE_INDEX, &equip),
            Some(Ok(ClientFacetMsg::EquipItem {
                header: soul(component::PLAYER),
                bag_slot: BAG_SLOT,
                equip_slot: MAIN_HAND_OPTION_2,
                quantity: 1,
                instance,
                flag: 0,
            }))
        );
        let mut writer = Writer::new();
        soul(component::PLAYER).write(&mut writer);
        let area = [0xff; RAW_ID_LEN];
        let passthrough = writer.bytes(&area).u64(3).u64(4).finish();
        assert_eq!(
            ClientFacetMsg::decode(TRIGGER_AREA_PASSTHROUGH_TYPE_INDEX, &passthrough),
            Some(Ok(ClientFacetMsg::TriggerAreaPassthrough {
                header: soul(component::PLAYER),
                entity: area,
                entity_facet: 3,
                extra: 4,
            }))
        );
        let container = [0x2f; RAW_ID_LEN];
        let mut writer = Writer::new();
        soul(component::PLAYER).write(&mut writer);
        let take = writer.bytes(&container).u64(1).u64(2).finish();
        assert_eq!(
            ClientFacetMsg::decode(TAKE_ALL_ITEMS_TYPE_INDEX, &take),
            Some(Ok(ClientFacetMsg::TakeAllItems {
                header: soul(component::PLAYER),
                container,
                container_entity: 1,
                extra: 2,
            }))
        );
    }

    #[test]
    fn client_calls_decode_and_others_are_left() {
        let mut body = Vec::new();
        let mut writer = Writer::new();
        soul(component::PLAYER).write(&mut writer);
        body.extend(writer.u8(1).finish());
        assert_eq!(
            ClientFacetMsg::decode(ACK_LEVEL_INFO_TYPE_INDEX, &body),
            Some(Ok(ClientFacetMsg::AckLevelInfo {
                header: soul(component::PLAYER),
                client_context: 1,
            }))
        );
        assert!(matches!(
            ClientFacetMsg::decode(CUTSCENE_ENDED_TYPE_INDEX, &body),
            Some(Err(MessageError::TrailingBytes { .. }))
        ));
        assert_eq!(
            ClientFacetMsg::decode(UPDATE_PHASING_TYPE_INDEX, &body),
            None
        );
        // 4248 is the header alone (facets note: retail 1432 ms).
        let mut writer = Writer::new();
        soul(component::TIME).write(&mut writer);
        assert_eq!(
            ClientFacetMsg::decode(TIME_SYNC_REQUEST_TYPE_INDEX, &writer.finish()),
            Some(Ok(ClientFacetMsg::TimeSyncRequest {
                header: soul(component::TIME),
            }))
        );
    }
}
