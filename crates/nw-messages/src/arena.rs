// SPDX-License-Identifier: AGPL-3.0-or-later
//! Expeditions, soul trials and raids (instances): the player's `PlayerArenaComponent`.
//!
//! Run 15 instances study (`local-data/scratch-work/run15/instances/REPORT.md`, 29 retail
//! ledgers of expeditions, raids, soul trials and catacombs):
//!
//! - Every call is a facet call of the player's `PlayerArenaComponent`: the target is the entity
//!   plus [`PLAYER_ARENA_FACET`] (Observed, the same constant on 3 characters).
//! - The client asks about a game mode from afar with 5039 (the CRC-32 of the lowercase game mode
//!   id) and the server answers 5038 with the same CRC and an allowed byte (1 in all 50 retail
//!   answers). At an entrance the interaction 71 `OpenDungeonMenu` gets the same 5038.
//! - The client asks to start with 5037 (one byte, 0 in every capture) or the interaction 72
//!   `EnterDungeon`; the server answers 6776 with an [`ActivateResponse`] (0 = success in every
//!   capture) and sets `isQueuedForDungeon`.
//! - The client then sends 946 (with another 5037 in the same millisecond); the server sets
//!   `isInArena`, shows the loading screen (3951) and moves the player into the instance level.
//! - 926 `ForfeitArena` (one byte, 0) and the interaction 74 `ExitDungeon` leave the instance.
//!
//! The member body of [`PlayerArenaState`] is the generic slot reader (one slot, 17 children; each
//! child byte is followed by the data of the children it sets, `crate::slots`). Child types from
//! the reader functions of the constructor `FUN_14276fed0`: `FUN_1417b4c20` one boolean byte,
//! `FUN_14279f420` a big-endian `u64`, `FUN_14279ec10` a version list, `FUN_14279f200` 16 raw
//! bytes, `FUN_1417b4590` a big-endian `u32`, `FUN_14279f1b0` one byte.

use crate::facet::FacetHeader;
use crate::items::FullList;
use crate::payload::{BundleMember, MemberState};
use crate::slots::{read_slot_byte, read_wide_children, write_wide_children};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `Javelin::PlayerArenaReplicatedState`.
pub const PLAYER_ARENA_TYPE_INDEX: u64 = 2406;
/// The player prefab component that takes the arena state (component-slots note, section 2).
pub const PLAYER_ARENA_COMPONENT: u64 = 4;
/// The target-id constant of the player's `PlayerArenaComponent`: the facet target less the
/// entity, the same in the 5037, 5039, 946, 926, 5038 and 6776 calls of 3 retail characters.
pub const PLAYER_ARENA_FACET: u64 = 0x0adf_8519_a10c_374d;

/// Type index of `PlayerArenaComponentServerFacet_TeleportIntoDungeon`.
pub const TELEPORT_INTO_DUNGEON_TYPE_INDEX: u64 = 946;
/// Type index of `PlayerArenaComponentServerFacet_ForfeitArena`.
pub const FORFEIT_ARENA_TYPE_INDEX: u64 = 926;
/// Type index of `PlayerArenaComponentServerFacet_RequestRemoteEnterDungeon`.
pub const REQUEST_REMOTE_ENTER_DUNGEON_TYPE_INDEX: u64 = 5037;
/// Type index of `PlayerArenaComponentServerFacet_RequestRemoteInteract`.
pub const REQUEST_REMOTE_INTERACT_TYPE_INDEX: u64 = 5039;
/// Type index of `PlayerArenaComponentClientFacet_OnRemoteInteractResult`.
pub const REMOTE_INTERACT_RESULT_TYPE_INDEX: u64 = 5038;
/// Type index of `PlayerArenaComponentClientFacet_OnDungeonActivateRemoteResult`.
pub const DUNGEON_ACTIVATE_RESULT_TYPE_INDEX: u64 = 6776;

/// Children of the arena state's only slot.
const ARENA_CHILDREN: u8 = 17;
/// Slots of the arena state.
const ARENA_SLOTS: u8 = 1;
/// The slot byte with slot 0 set.
const SLOT_0: u8 = 0x01;
/// The slot byte with no slot set.
const NO_SLOT: u8 = 0;
/// Bytes of `m_lastDungeonsEntered` and `m_lastMutatedDungeonEntered`.
const DUNGEON_ID_LEN: usize = RAW_ID_LEN;
/// `m_gameModeIdx` when no single-player game mode is set: every group expedition, raid and
/// catacombs capture sends it (and the client code tests for it before the lookup,
/// `FUN_142789c50`).
pub const NO_GAME_MODE_INDEX: u8 = 0xff;
const FIELD: &str = "player arena";

/// The children of [`PlayerArenaState`], in constructor order.
mod child {
    pub const IN_ARENA: u8 = 0;
    pub const QUEUED: u8 = 1;
    pub const DUNGEON_COOLDOWN: u8 = 2;
    pub const SOLO_TRIAL_COOLDOWN: u8 = 3;
    pub const DUNGEON_RANKS: u8 = 4;
    pub const LAST_DUNGEON: u8 = 5;
    pub const LAST_MUTATED_DUNGEON: u8 = 6;
    pub const BASE_ENTERED: u8 = 7;
    pub const MUTATED_ENTERED: u8 = 8;
    pub const GROUP_TRIALS_ENTERED: u8 = 9;
    pub const NEXT_BASE_REFRESH: u8 = 10;
    pub const NEXT_MUTATED_REFRESH: u8 = 11;
    pub const NEXT_GROUP_TRIAL_REFRESH: u8 = 12;
    pub const MUTATION_AWARD: u8 = 13;
    pub const SINGLE_PLAYER_STATE: u8 = 14;
    pub const SINGLE_PLAYER_TIME: u8 = 15;
    pub const GAME_MODE_INDEX: u8 = 16;
}

/// `m_singlePlayerInstanceState` of a soul trial the server activated: 3 from the 6776 answer
/// until the teleport (17 retail trials).
pub const SINGLE_PLAYER_ACTIVATED: u8 = 3;
/// `m_singlePlayerInstanceState` after the teleport into the trial: 4 (17 retail trials).
pub const SINGLE_PLAYER_ENTERED: u8 = 4;

/// The player's arena state (type 2406): `None` children are not in the body. Times named
/// `_since_2000_ns` count nanoseconds from 2000-01-01 (the cooldowns: retail values sit 26.5
/// years after it); the others are Unix nanoseconds (the refresh times fall on the daily and
/// weekly reset hours).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerArenaState {
    /// `isInArena`: the player is inside an instance.
    pub in_arena: Option<bool>,
    /// `isQueuedForDungeon`: the instance is activated and waits for the teleport.
    pub queued: Option<bool>,
    /// `dungeonCooldownTime`: in two soul trials 240 s after the teleport.
    pub dungeon_cooldown_since_2000_ns: Option<u64>,
    /// `enterSoloTrialCooldownTime`: in two soul trials 60 s before the teleport.
    pub solo_trial_cooldown_since_2000_ns: Option<u64>,
    /// `dungeonRanks`: one byte per element (three elements of 2 or 3 in the captures).
    pub dungeon_ranks: Option<FullList<u8>>,
    /// `m_lastDungeonsEntered`: 16 bytes, new at each expedition entry.
    pub last_dungeon: Option<[u8; DUNGEON_ID_LEN]>,
    /// `m_lastMutatedDungeonEntered`: 16 bytes.
    pub last_mutated_dungeon: Option<[u8; DUNGEON_ID_LEN]>,
    /// `m_numBaseDungeonsEnteredSinceLastRefresh`.
    pub base_entered: Option<u32>,
    /// `m_numMutatedDungeonsEnteredSinceLastRefresh`.
    pub mutated_entered: Option<u32>,
    /// `m_numGroupTrialsEnteredSinceLastRefresh`.
    pub group_trials_entered: Option<u32>,
    /// `m_nextDungeonBaseMaxLimitRefreshTime`, Unix nanoseconds.
    pub next_base_refresh_ns: Option<u64>,
    /// `m_nextDungeonMutatedMaxLimitRefreshTime`, Unix nanoseconds.
    pub next_mutated_refresh_ns: Option<u64>,
    /// `m_nextGroupTrialMaxLimitRefreshTime`, Unix nanoseconds.
    pub next_group_trial_refresh_ns: Option<u64>,
    /// `hasMutationUnlockAwardBeenGranted`.
    pub mutation_award: Option<bool>,
    /// `m_singlePlayerInstanceState` ([`SINGLE_PLAYER_ACTIVATED`], [`SINGLE_PLAYER_ENTERED`]).
    pub single_player_state: Option<u8>,
    /// `m_singlePlayerDungeonTime`, Unix nanoseconds: when the trial was activated.
    pub single_player_time_ns: Option<u64>,
    /// `m_gameModeIdx`: the index of the trial's game mode in the client's game mode list, or
    /// [`NO_GAME_MODE_INDEX`].
    pub game_mode_index: Option<u8>,
}

impl PlayerArenaState {
    fn present(&self) -> [bool; ARENA_CHILDREN as usize] {
        [
            self.in_arena.is_some(),
            self.queued.is_some(),
            self.dungeon_cooldown_since_2000_ns.is_some(),
            self.solo_trial_cooldown_since_2000_ns.is_some(),
            self.dungeon_ranks.is_some(),
            self.last_dungeon.is_some(),
            self.last_mutated_dungeon.is_some(),
            self.base_entered.is_some(),
            self.mutated_entered.is_some(),
            self.group_trials_entered.is_some(),
            self.next_base_refresh_ns.is_some(),
            self.next_mutated_refresh_ns.is_some(),
            self.next_group_trial_refresh_ns.is_some(),
            self.mutation_award.is_some(),
            self.single_player_state.is_some(),
            self.single_player_time_ns.is_some(),
            self.game_mode_index.is_some(),
        ]
    }

    /// Whether no child is set: such a change sends nothing.
    pub fn is_empty(&self) -> bool {
        self.present().iter().all(|present| !present)
    }

    /// The children of `self` that differ from `before` (a change sends only those, as retail:
    /// one child per call for the queue, four at the teleport).
    pub fn changes_from(&self, before: &Self) -> Self {
        fn changed<T: Clone + PartialEq>(now: &Option<T>, before: &Option<T>) -> Option<T> {
            now.as_ref().filter(|now| before.as_ref() != Some(*now)).cloned()
        }
        Self {
            in_arena: changed(&self.in_arena, &before.in_arena),
            queued: changed(&self.queued, &before.queued),
            dungeon_cooldown_since_2000_ns: changed(
                &self.dungeon_cooldown_since_2000_ns,
                &before.dungeon_cooldown_since_2000_ns,
            ),
            solo_trial_cooldown_since_2000_ns: changed(
                &self.solo_trial_cooldown_since_2000_ns,
                &before.solo_trial_cooldown_since_2000_ns,
            ),
            dungeon_ranks: changed(&self.dungeon_ranks, &before.dungeon_ranks),
            last_dungeon: changed(&self.last_dungeon, &before.last_dungeon),
            last_mutated_dungeon: changed(&self.last_mutated_dungeon, &before.last_mutated_dungeon),
            base_entered: changed(&self.base_entered, &before.base_entered),
            mutated_entered: changed(&self.mutated_entered, &before.mutated_entered),
            group_trials_entered: changed(&self.group_trials_entered, &before.group_trials_entered),
            next_base_refresh_ns: changed(&self.next_base_refresh_ns, &before.next_base_refresh_ns),
            next_mutated_refresh_ns: changed(
                &self.next_mutated_refresh_ns,
                &before.next_mutated_refresh_ns,
            ),
            next_group_trial_refresh_ns: changed(
                &self.next_group_trial_refresh_ns,
                &before.next_group_trial_refresh_ns,
            ),
            mutation_award: changed(&self.mutation_award, &before.mutation_award),
            single_player_state: changed(&self.single_player_state, &before.single_player_state),
            single_player_time_ns: changed(
                &self.single_player_time_ns,
                &before.single_player_time_ns,
            ),
            game_mode_index: changed(&self.game_mode_index, &before.game_mode_index),
        }
    }

    /// Writes the body: the slot byte, then the children.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the ranks list does not fit its prefix code.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let mask = self
            .present()
            .iter()
            .enumerate()
            .filter(|(_, present)| **present)
            .fold(0_u32, |mask, (index, _)| mask | 1 << index);
        if mask == 0 {
            writer.u8(NO_SLOT);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_wide_children(writer, mask, ARENA_CHILDREN, |writer, index| {
            self.write_child(writer, index)
        })
    }

    fn write_child(&self, writer: &mut Writer, index: u8) -> Result<(), MessageError> {
        let flag = |writer: &mut Writer, value: Option<bool>| {
            writer.boolean(value.unwrap_or_default());
        };
        let long = |writer: &mut Writer, value: Option<u64>| {
            writer.u64(value.unwrap_or_default());
        };
        let count = |writer: &mut Writer, value: Option<u32>| {
            writer.u32(value.unwrap_or_default());
        };
        match index {
            child::IN_ARENA => flag(writer, self.in_arena),
            child::QUEUED => flag(writer, self.queued),
            child::DUNGEON_COOLDOWN => long(writer, self.dungeon_cooldown_since_2000_ns),
            child::SOLO_TRIAL_COOLDOWN => long(writer, self.solo_trial_cooldown_since_2000_ns),
            child::DUNGEON_RANKS => {
                if let Some(ranks) = &self.dungeon_ranks {
                    ranks.write(writer, |writer, rank| {
                        writer.u8(*rank);
                        Ok(())
                    })?;
                }
            }
            child::LAST_DUNGEON => {
                writer.bytes(&self.last_dungeon.unwrap_or_default());
            }
            child::LAST_MUTATED_DUNGEON => {
                writer.bytes(&self.last_mutated_dungeon.unwrap_or_default());
            }
            child::BASE_ENTERED => count(writer, self.base_entered),
            child::MUTATED_ENTERED => count(writer, self.mutated_entered),
            child::GROUP_TRIALS_ENTERED => count(writer, self.group_trials_entered),
            child::NEXT_BASE_REFRESH => long(writer, self.next_base_refresh_ns),
            child::NEXT_MUTATED_REFRESH => long(writer, self.next_mutated_refresh_ns),
            child::NEXT_GROUP_TRIAL_REFRESH => long(writer, self.next_group_trial_refresh_ns),
            child::MUTATION_AWARD => flag(writer, self.mutation_award),
            child::SINGLE_PLAYER_STATE => {
                writer.u8(self.single_player_state.unwrap_or_default());
            }
            child::SINGLE_PLAYER_TIME => long(writer, self.single_player_time_ns),
            child::GAME_MODE_INDEX => {
                writer.u8(self.game_mode_index.unwrap_or(NO_GAME_MODE_INDEX));
            }
            _ => {
                return Err(MessageError::Unsized {
                    field: FIELD,
                    byte: index,
                });
            }
        }
        Ok(())
    }

    /// Reads a body.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        if read_slot_byte(reader, FIELD, ARENA_SLOTS)? == NO_SLOT {
            return Ok(state);
        }
        read_wide_children(reader, FIELD, ARENA_CHILDREN, |reader, index| {
            match index {
                child::IN_ARENA => state.in_arena = Some(reader.boolean()?),
                child::QUEUED => state.queued = Some(reader.boolean()?),
                child::DUNGEON_COOLDOWN => {
                    state.dungeon_cooldown_since_2000_ns = Some(reader.u64()?);
                }
                child::SOLO_TRIAL_COOLDOWN => {
                    state.solo_trial_cooldown_since_2000_ns = Some(reader.u64()?);
                }
                child::DUNGEON_RANKS => {
                    state.dungeon_ranks = Some(FullList::read(reader, |reader| reader.u8())?);
                }
                child::LAST_DUNGEON => state.last_dungeon = Some(reader.array()?),
                child::LAST_MUTATED_DUNGEON => state.last_mutated_dungeon = Some(reader.array()?),
                child::BASE_ENTERED => state.base_entered = Some(reader.u32()?),
                child::MUTATED_ENTERED => state.mutated_entered = Some(reader.u32()?),
                child::GROUP_TRIALS_ENTERED => state.group_trials_entered = Some(reader.u32()?),
                child::NEXT_BASE_REFRESH => state.next_base_refresh_ns = Some(reader.u64()?),
                child::NEXT_MUTATED_REFRESH => state.next_mutated_refresh_ns = Some(reader.u64()?),
                child::NEXT_GROUP_TRIAL_REFRESH => {
                    state.next_group_trial_refresh_ns = Some(reader.u64()?);
                }
                child::MUTATION_AWARD => state.mutation_award = Some(reader.boolean()?),
                child::SINGLE_PLAYER_STATE => state.single_player_state = Some(reader.u8()?),
                child::SINGLE_PLAYER_TIME => state.single_player_time_ns = Some(reader.u64()?),
                child::GAME_MODE_INDEX => state.game_mode_index = Some(reader.u8()?),
                child => {
                    return Err(MessageError::Unsized {
                        field: FIELD,
                        byte: child,
                    });
                }
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// The body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the ranks list does not fit its prefix code.
    pub fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.write(&mut writer)?;
        Ok(writer.finish())
    }

    /// The state of the body `body`, which it must use up.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not match the layout.
    pub fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let state = Self::read(&mut reader)?;
        reader.finish()?;
        Ok(state)
    }

    /// The bundle member of the player's own entity with this state (component
    /// [`PLAYER_ARENA_COMPONENT`]).
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the state cannot be encoded.
    pub fn member(&self) -> Result<BundleMember, MessageError> {
        Ok(BundleMember {
            component: PLAYER_ARENA_COMPONENT,
            state: MemberState::Recorded {
                class: PLAYER_ARENA_TYPE_INDEX,
                body: self.encode()?,
            },
        })
    }
}

/// `eDungeonActivateResponse` (client script enum, registered in this order in `FUN_1454d3310`;
/// the values are their order, Inferred: `Success` is 0 in every retail 6776).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivateResponse {
    /// The instance is activated.
    Success,
    /// The group has fewer members than the game mode's `MinGroupSize`.
    NotEnoughMembers,
    /// Too few group members are near the entrance.
    NotEnoughMembersInProximity,
    /// The player lacks a required item (`RequiredConsumableItemClasses`).
    MissingReqItem,
    /// The required item is disabled.
    ReqItemDisabled,
    /// The group has more members than the game mode's `TeamCapacity`.
    TooManyMembers,
    /// A raid group may not enter.
    RaidGroupDisallowed,
    /// A solo game mode refused the group.
    SoloGroupFailure,
    /// An activation of the group is already pending.
    PendingActivation,
    /// The player is in its cooldown.
    InCooldown,
    /// The entrance could not activate the instance.
    EntranceActivationFailure,
    /// The group already has an instance of another game mode.
    GroupAlreadyInDungeon,
    /// The group did not answer in time.
    GroupTimeout,
    /// The mutation needs a higher level.
    MutationLevelReq,
    /// The mutation difficulty is not unlocked.
    MutationDifficultyReq,
    /// Items could not be removed for the entry.
    ItemRemovalFailure,
    /// The scheduled game mode is not available now.
    ScheduledNotAvailable,
    /// The entrance is busy.
    EntranceBusy,
}

impl ActivateResponse {
    const ALL: [Self; 18] = [
        Self::Success,
        Self::NotEnoughMembers,
        Self::NotEnoughMembersInProximity,
        Self::MissingReqItem,
        Self::ReqItemDisabled,
        Self::TooManyMembers,
        Self::RaidGroupDisallowed,
        Self::SoloGroupFailure,
        Self::PendingActivation,
        Self::InCooldown,
        Self::EntranceActivationFailure,
        Self::GroupAlreadyInDungeon,
        Self::GroupTimeout,
        Self::MutationLevelReq,
        Self::MutationDifficultyReq,
        Self::ItemRemovalFailure,
        Self::ScheduledNotAvailable,
        Self::EntranceBusy,
    ];

    /// The wire number.
    pub fn number(self) -> u32 {
        Self::ALL
            .iter()
            .position(|response| *response == self)
            .and_then(|position| u32::try_from(position).ok())
            .unwrap_or_default()
    }

    /// The response of wire number `number`.
    pub fn of_number(number: u32) -> Option<Self> {
        usize::try_from(number)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
    }
}

/// An arena call of the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaClientMsg {
    /// 946: teleport into the activated instance.
    TeleportIntoDungeon {
        /// The header.
        header: FacetHeader,
    },
    /// 926: give up the instance.
    ForfeitArena {
        /// The header.
        header: FacetHeader,
        /// One byte (0 in every capture).
        flag: u8,
    },
    /// 5037: activate (or re-enter) the instance of the player's group from afar.
    RequestRemoteEnterDungeon {
        /// The header.
        header: FacetHeader,
        /// One byte (0 in every capture).
        flag: u8,
    },
    /// 5039: may the player use the game mode from afar? Its CRC-32 of the lowercase id.
    RequestRemoteInteract {
        /// The header.
        header: FacetHeader,
        /// CRC-32 of the lowercase `GameModeID`.
        game_mode: u32,
    },
}

impl ArenaClientMsg {
    /// Decodes `body` when `type_index` is an arena call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            TELEPORT_INTO_DUNGEON_TYPE_INDEX => {
                read(|header, _| Ok(Self::TeleportIntoDungeon { header }))
            }
            FORFEIT_ARENA_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ForfeitArena {
                    header,
                    flag: reader.u8()?,
                })
            }),
            REQUEST_REMOTE_ENTER_DUNGEON_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestRemoteEnterDungeon {
                    header,
                    flag: reader.u8()?,
                })
            }),
            REQUEST_REMOTE_INTERACT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RequestRemoteInteract {
                    header,
                    game_mode: reader.u32()?,
                })
            }),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::TeleportIntoDungeon { .. } => TELEPORT_INTO_DUNGEON_TYPE_INDEX,
            Self::ForfeitArena { .. } => FORFEIT_ARENA_TYPE_INDEX,
            Self::RequestRemoteEnterDungeon { .. } => REQUEST_REMOTE_ENTER_DUNGEON_TYPE_INDEX,
            Self::RequestRemoteInteract { .. } => REQUEST_REMOTE_INTERACT_TYPE_INDEX,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::TeleportIntoDungeon { header } => header.write(&mut writer),
            Self::ForfeitArena { header, flag }
            | Self::RequestRemoteEnterDungeon { header, flag } => {
                header.write(&mut writer);
                writer.u8(*flag);
            }
            Self::RequestRemoteInteract { header, game_mode } => {
                header.write(&mut writer);
                writer.u32(*game_mode);
            }
        }
        writer.finish()
    }
}

/// An arena answer of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaServerMsg {
    /// 5038: the answer to 5039 or to an `OpenDungeonMenu` interaction.
    RemoteInteractResult {
        /// The header.
        header: FacetHeader,
        /// The CRC-32 of the lowercase game mode id the call named.
        game_mode: u32,
        /// Whether the player may use the game mode (1 in all 50 retail answers).
        allowed: bool,
    },
    /// 6776: the answer to an activation (5037 or `EnterDungeon`).
    ActivateResult {
        /// The header.
        header: FacetHeader,
        /// The response.
        response: ActivateResponse,
    },
}

impl ArenaServerMsg {
    /// Whether `type_index` is an arena answer.
    pub const fn is_type(type_index: u64) -> bool {
        matches!(
            type_index,
            REMOTE_INTERACT_RESULT_TYPE_INDEX | DUNGEON_ACTIVATE_RESULT_TYPE_INDEX
        )
    }

    /// The type index of the answer.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::RemoteInteractResult { .. } => REMOTE_INTERACT_RESULT_TYPE_INDEX,
            Self::ActivateResult { .. } => DUNGEON_ACTIVATE_RESULT_TYPE_INDEX,
        }
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::RemoteInteractResult {
                header,
                game_mode,
                allowed,
            } => {
                header.write(&mut writer);
                writer.u32(*game_mode).boolean(*allowed);
            }
            Self::ActivateResult { header, response } => {
                header.write(&mut writer);
                writer.u32(response.number());
            }
        }
        writer.finish()
    }

    /// Decodes `body` of arena answer `type_index`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not read, and [`MessageError::BadTag`] for a
    /// type that is not an arena answer or an unknown response number.
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let message = match type_index {
            REMOTE_INTERACT_RESULT_TYPE_INDEX => Self::RemoteInteractResult {
                header,
                game_mode: reader.u32()?,
                allowed: reader.boolean()?,
            },
            DUNGEON_ACTIVATE_RESULT_TYPE_INDEX => {
                let number = reader.u32()?;
                Self::ActivateResult {
                    header,
                    response: ActivateResponse::of_number(number).ok_or(MessageError::BadTag {
                        field: "dungeon activate response",
                        tag: u8::try_from(number).unwrap_or(u8::MAX),
                    })?,
                }
            }
            _ => {
                return Err(MessageError::BadTag {
                    field: "arena answer type",
                    tag: u8::MAX,
                });
            }
        };
        reader.finish()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTITY: u64 = 0x1122_3344_5566_7788;
    const TARGET: u64 = 0x99aa_bbcc_ddee_ff00;
    /// A refresh time on a reset hour, Unix nanoseconds (capture-shaped: 03:00 UTC).
    const REFRESH_NS: u64 = 1_792_746_000_226_611_400;
    const MUTATED_REFRESH_NS: u64 = 1_793_091_600_226_670_700;
    const COOLDOWN_NS: u64 = 0x0bb5_50d9_463d_8651;
    const SOLO_NS: u64 = 0x0bb5_5093_6cd8_ce51;
    const TRIAL_TIME_NS: u64 = 0x18d8_9d70_0b4e_4508;
    const TRIAL_INDEX: u8 = 0x30;
    const RANKS_VERSION: u64 = 2476;

    fn header() -> FacetHeader {
        FacetHeader {
            target: TARGET,
            entity: ENTITY,
        }
    }

    fn header_bytes() -> Vec<u8> {
        let mut bytes = TARGET.to_be_bytes().to_vec();
        bytes.extend(ENTITY.to_be_bytes());
        bytes
    }

    fn round(state: &PlayerArenaState, expected: &[u8]) {
        let body = state.encode().expect("encode");
        assert_eq!(body, expected);
        assert_eq!(&PlayerArenaState::decode(&body).expect("decode"), state);
    }

    #[test]
    fn a_new_character_gets_the_three_refresh_times() {
        // Retail newchar-retail-2026-09-18: slot 0, an empty first child byte, then children
        // 10, 11 and 12.
        let state = PlayerArenaState {
            next_base_refresh_ns: Some(REFRESH_NS),
            next_mutated_refresh_ns: Some(MUTATED_REFRESH_NS),
            next_group_trial_refresh_ns: Some(REFRESH_NS),
            ..PlayerArenaState::default()
        };
        let mut expected = vec![0x01, 0x80, 0x38];
        expected.extend(REFRESH_NS.to_be_bytes());
        expected.extend(MUTATED_REFRESH_NS.to_be_bytes());
        expected.extend(REFRESH_NS.to_be_bytes());
        round(&state, &expected);
    }

    #[test]
    fn a_trial_activation_queues_with_its_state_time_and_index() {
        // Retail soul trials: `01 82 01 80 07 03 <time> <index>`.
        let state = PlayerArenaState {
            queued: Some(true),
            single_player_state: Some(SINGLE_PLAYER_ACTIVATED),
            single_player_time_ns: Some(TRIAL_TIME_NS),
            game_mode_index: Some(TRIAL_INDEX),
            ..PlayerArenaState::default()
        };
        let mut expected = vec![0x01, 0x82, 0x01, 0x80, 0x07, SINGLE_PLAYER_ACTIVATED];
        expected.extend(TRIAL_TIME_NS.to_be_bytes());
        expected.push(TRIAL_INDEX);
        round(&state, &expected);
    }

    #[test]
    fn the_teleport_sets_in_arena_and_the_cooldowns() {
        // Retail: `01 8f 01 00 <cooldown> <solo cooldown> 80 01 04`.
        let state = PlayerArenaState {
            in_arena: Some(true),
            queued: Some(false),
            dungeon_cooldown_since_2000_ns: Some(COOLDOWN_NS),
            solo_trial_cooldown_since_2000_ns: Some(SOLO_NS),
            single_player_state: Some(SINGLE_PLAYER_ENTERED),
            ..PlayerArenaState::default()
        };
        let mut expected = vec![0x01, 0x8f, 0x01, 0x00];
        expected.extend(COOLDOWN_NS.to_be_bytes());
        expected.extend(SOLO_NS.to_be_bytes());
        expected.extend([0x80, 0x01, SINGLE_PLAYER_ENTERED]);
        round(&state, &expected);
        // The exit: `01 01 00`.
        round(
            &PlayerArenaState {
                in_arena: Some(false),
                ..PlayerArenaState::default()
            },
            &[0x01, 0x01, 0x00],
        );
    }

    #[test]
    fn the_full_state_in_an_instance_walks_every_child() {
        let state = PlayerArenaState {
            in_arena: Some(true),
            queued: Some(false),
            dungeon_cooldown_since_2000_ns: Some(COOLDOWN_NS),
            solo_trial_cooldown_since_2000_ns: Some(SOLO_NS),
            dungeon_ranks: Some(FullList {
                version: Some(RANKS_VERSION),
                elements: vec![3, 3, 3],
            }),
            last_dungeon: Some([0x2d; DUNGEON_ID_LEN]),
            last_mutated_dungeon: Some([0x7d; DUNGEON_ID_LEN]),
            base_entered: Some(1),
            mutated_entered: None,
            group_trials_entered: Some(0),
            next_base_refresh_ns: Some(REFRESH_NS),
            next_mutated_refresh_ns: Some(MUTATED_REFRESH_NS),
            next_group_trial_refresh_ns: Some(REFRESH_NS),
            mutation_award: Some(true),
            single_player_state: Some(SINGLE_PLAYER_ENTERED),
            single_player_time_ns: Some(TRIAL_TIME_NS),
            game_mode_index: Some(TRIAL_INDEX),
        };
        let body = state.encode().expect("encode");
        // Slot 0, children 0 to 6, then the ranks list `00 01 <version ac 26> 03 03 03 03`.
        assert_eq!(&body[..4], &[0x01, 0xff, 0x01, 0x00]);
        assert_eq!(&body[20..28], &[0x00, 0x01, 0xac, 0x26, 0x03, 0x03, 0x03, 0x03]);
        assert_eq!(PlayerArenaState::decode(&body).expect("decode"), state);
        assert!(!state.is_empty());
        assert!(PlayerArenaState::default().is_empty());
        assert_eq!(PlayerArenaState::default().encode().expect("encode"), [NO_SLOT]);
    }

    #[test]
    fn a_change_keeps_only_the_children_that_moved() {
        let before = PlayerArenaState {
            in_arena: Some(false),
            queued: Some(true),
            base_entered: Some(1),
            ..PlayerArenaState::default()
        };
        let now = PlayerArenaState {
            in_arena: Some(true),
            queued: Some(false),
            base_entered: Some(1),
            ..PlayerArenaState::default()
        };
        assert_eq!(
            now.changes_from(&before),
            PlayerArenaState {
                in_arena: Some(true),
                queued: Some(false),
                ..PlayerArenaState::default()
            }
        );
        assert!(now.changes_from(&now).is_empty());
    }

    #[test]
    fn the_member_is_component_four_of_class_2406() {
        let member = PlayerArenaState {
            in_arena: Some(false),
            ..PlayerArenaState::default()
        }
        .member()
        .expect("member");
        assert_eq!(member.component, PLAYER_ARENA_COMPONENT);
        assert_eq!(member.state.type_index(), PLAYER_ARENA_TYPE_INDEX);
    }

    #[test]
    fn client_calls_read_with_their_arguments() {
        let header = header();
        let mut remote = header_bytes();
        remote.extend(0x634a_d894_u32.to_be_bytes());
        assert_eq!(
            ArenaClientMsg::decode(REQUEST_REMOTE_INTERACT_TYPE_INDEX, &remote),
            Some(Ok(ArenaClientMsg::RequestRemoteInteract {
                header,
                game_mode: 0x634a_d894,
            }))
        );
        let mut enter = header_bytes();
        enter.push(0);
        assert_eq!(
            ArenaClientMsg::decode(REQUEST_REMOTE_ENTER_DUNGEON_TYPE_INDEX, &enter),
            Some(Ok(ArenaClientMsg::RequestRemoteEnterDungeon { header, flag: 0 }))
        );
        assert_eq!(
            ArenaClientMsg::decode(FORFEIT_ARENA_TYPE_INDEX, &enter),
            Some(Ok(ArenaClientMsg::ForfeitArena { header, flag: 0 }))
        );
        assert_eq!(
            ArenaClientMsg::decode(TELEPORT_INTO_DUNGEON_TYPE_INDEX, &header_bytes()),
            Some(Ok(ArenaClientMsg::TeleportIntoDungeon { header }))
        );
        assert!(matches!(
            ArenaClientMsg::decode(TELEPORT_INTO_DUNGEON_TYPE_INDEX, &enter),
            Some(Err(_))
        ));
        assert_eq!(ArenaClientMsg::decode(ACK_UNRELATED_TYPE_INDEX, &enter), None);
        for call in [
            ArenaClientMsg::TeleportIntoDungeon { header },
            ArenaClientMsg::ForfeitArena { header, flag: 0 },
            ArenaClientMsg::RequestRemoteEnterDungeon { header, flag: 0 },
            ArenaClientMsg::RequestRemoteInteract {
                header,
                game_mode: 7,
            },
        ] {
            assert_eq!(
                ArenaClientMsg::decode(call.type_index(), &call.encode()),
                Some(Ok(call))
            );
        }
    }

    /// A type index that is no arena call (the level acknowledgement).
    const ACK_UNRELATED_TYPE_INDEX: u64 = 5181;

    #[test]
    fn answers_have_the_captured_shapes() {
        let header = header();
        let interact = ArenaServerMsg::RemoteInteractResult {
            header,
            game_mode: 0x2736_ba35,
            allowed: true,
        };
        let mut expected = header_bytes();
        expected.extend([0x27, 0x36, 0xba, 0x35, 0x01]);
        assert_eq!(interact.encode(), expected);
        assert_eq!(
            ArenaServerMsg::decode(interact.type_index(), &expected),
            Ok(interact)
        );
        let activate = ArenaServerMsg::ActivateResult {
            header,
            response: ActivateResponse::Success,
        };
        let mut expected = header_bytes();
        expected.extend([0, 0, 0, 0]);
        assert_eq!(activate.encode(), expected);
        assert_eq!(
            ArenaServerMsg::decode(activate.type_index(), &expected),
            Ok(activate)
        );
        assert!(ArenaServerMsg::is_type(DUNGEON_ACTIVATE_RESULT_TYPE_INDEX));
        assert!(!ArenaServerMsg::is_type(TELEPORT_INTO_DUNGEON_TYPE_INDEX));
    }

    #[test]
    fn responses_number_in_their_declared_order() {
        assert_eq!(ActivateResponse::Success.number(), 0);
        assert_eq!(ActivateResponse::TooManyMembers.number(), 5);
        assert_eq!(ActivateResponse::EntranceBusy.number(), 17);
        for response in ActivateResponse::ALL {
            assert_eq!(ActivateResponse::of_number(response.number()), Some(response));
        }
        assert_eq!(ActivateResponse::of_number(18), None);
    }
}
