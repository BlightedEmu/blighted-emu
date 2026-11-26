// SPDX-License-Identifier: AGPL-3.0-or-later
//! Group Finder calls, pushes and replicated state (`GroupsComponent` and
//! `Javelin::GroupFinderGroupDataComponentReplicatedState`, type 1091).
//!
//! Layouts verified from retail captures (`Playthrough_Part_01` 3032.773 s to 3071.193 s,
//! `07-Tempests_Heart` 1.906 s to 9.866 s, `09-The_Ennead`) and the client's replicated-state
//! table ([`crate::member_layouts::table`] class 1091):
//!
//! - 5355 `RequestModifyGroupFinderApplicationSubscription`: header, `subscribed: bool`.
//! - 1960 `RequestGetGroupFinderGroupList`: header, `filter_flag: u8`, `game_mode_crc: u32`,
//!   `page: u16`.
//! - 3461 `OnGroupFinderGetGroupListSuccess`: header, empty list observed as `00 00 01`
//!   (`u8` count `0`, `u8` page token `0`, `bool` `is_last_page = true`). The wire shape of a
//!   non-empty entry is Inferred (no retail ledger holds one).
//! - 1147 `RequestGetGroupFinderGroup`: header, `group_id: [u8; 16]`.
//! - 2010 `OnGroupFinderGetGroupSuccess`: header, `group_id: [u8; 16]`,
//!   [`GroupFinderGroupMetadata`], `u8` count + `GroupFinderMemberBlob`s (282-byte retail sample
//!   in `Playthrough_Part_01` 3032.841 s).
//! - 3485 `OnGroupFinderApplicationAccepted`: header, `group_id: [u8; 16]`,
//!   `application_id: Text`.
//! - 3245 `RequestDeclineGroupFinderApplicationInvite`: header, `group_id: [u8; 16]`,
//!   `application_id: Text`, `reason: u8`.
//! - 3475 `OnGroupFinderDeclineApplicationInviteSuccess`: header only.
//! - 4739 `RequestConfirmConvertGroupFinderGroupToGroup`: header only.

use crate::facet::FacetHeader;
use crate::items::FullList;
use crate::slots::{read_children_64, read_slot_byte, write_children_64};
use crate::social::players::ICON_LEN;
use crate::social::text::{read_text, write_text};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `Javelin::GroupFinderGroupDataComponentReplicatedState`.
pub const GROUP_FINDER_DATA_TYPE_INDEX: u64 = 1091;
/// `GroupsComponentServerFacet_RequestGetGroupFinderGroupList`.
pub const GET_GROUP_LIST_TYPE_INDEX: u64 = 1960;
/// `GroupsComponentClientFacet_OnGroupFinderGetGroupListSuccess`.
pub const GET_GROUP_LIST_SUCCESS_TYPE_INDEX: u64 = 3461;
/// `GroupsComponentServerFacet_RequestGetGroupFinderGroup`.
pub const GET_GROUP_TYPE_INDEX: u64 = 1147;
/// `GroupsComponentClientFacet_OnGroupFinderGetGroupSuccess`.
pub const GET_GROUP_SUCCESS_TYPE_INDEX: u64 = 2010;
/// `GroupsComponentClientFacet_OnGroupFinderApplicationAccepted`.
pub const APPLICATION_ACCEPTED_TYPE_INDEX: u64 = 3485;
/// `GroupsComponentServerFacet_RequestDeclineGroupFinderApplicationInvite`.
pub const DECLINE_APPLICATION_INVITE_TYPE_INDEX: u64 = 3245;
/// `GroupsComponentClientFacet_OnGroupFinderDeclineApplicationInviteSuccess`.
pub const DECLINE_APPLICATION_INVITE_SUCCESS_TYPE_INDEX: u64 = 3475;
/// `GroupsComponentServerFacet_RequestModifyGroupFinderApplicationSubscription`.
pub const MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX: u64 = 5355;
/// `GroupsComponentServerFacet_RequestConfirmConvertGroupFinderGroupToGroup`.
pub const CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX: u64 = 4739;

/// The `createSource` byte of `GroupsState` (`1994`) when a group was formed from Group Finder
/// (Observed: `07-Tempests_Heart`, `09-The_Ennead`).
pub const GROUP_FINDER_CREATE_SOURCE: u8 = 1;
/// `platform_id_kind` when a member has no platform id (Observed: `0xffff_ffff` in
/// `Playthrough_Part_01` 3032.841 s).
pub const NO_PLATFORM_ID_KIND: u32 = u32::MAX;

/// The observed Group Finder client calls.
pub(crate) const CALL_TYPES: [u64; 5] = [
    GET_GROUP_LIST_TYPE_INDEX,
    GET_GROUP_TYPE_INDEX,
    DECLINE_APPLICATION_INVITE_TYPE_INDEX,
    MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX,
    CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX,
];

/// Slots of `GroupFinderGroupDataComponentReplicatedState` (class 1091).
const SLOTS: u8 = 5;
const SLOT_1_BIT: u8 = 1 << 1;
const SLOT_2_BIT: u8 = 1 << 2;
const SLOT_3_BIT: u8 = 1 << 3;
const SLOT_1_CHILDREN: u8 = 5;
const SLOT_2_CHILDREN: u8 = 1;
const SLOT_3_CHILDREN: u8 = 3;

const CHILD_GROUP_ID: u8 = 0;
const CHILD_METADATA: u8 = 1;
const CHILD_MEMBER_BLOBS: u8 = 2;
const CHILD_PENDING_CONVERSION: u8 = 3;
const CHILD_CONVERSION_CONFIRMATIONS: u8 = 4;
const CHILD_HAS_PENDING_APPLICATIONS: u8 = 0;
const CHILD_APPLICATION_IDS: u8 = 0;
const CHILD_APPLICATION_MEMBER_BLOBS: u8 = 1;
const CHILD_APPLICATION_STATES: u8 = 2;

const FIELD_STATE: &str = "group finder state";

/// Metadata of a Group Finder listing (`GroupFinderGroupMetadata`, verified against class 1091
/// and the 282-byte `2010` sample of `Playthrough_Part_01` 3032.841 s).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupFinderGroupMetadata {
    /// Leading flags (Observed: `[1, 0, 0]` in `Playthrough_Part_01`).
    pub flags: [u8; 3],
    /// Lowercase CRC-32 of the activity `GameModeId`.
    pub game_mode_crc: u32,
    /// Mutator CRC-32 (`0` for regular).
    pub mutator_crc: u32,
    /// Minimum character level.
    pub min_level: u32,
    /// Minimum gear score.
    pub min_gear_score: u32,
    /// Trailing flags (Observed: `[0, 0x1c, 0]` in `Playthrough_Part_01`).
    pub tail_flags: [u8; 3],
    /// Lobby title bytes (1-byte length prefix on the wire).
    pub title: Vec<u8>,
    /// Creation timestamp (Unix nanoseconds).
    pub created_ns: u64,
}

impl GroupFinderGroupMetadata {
    /// Reads the metadata from `reader`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the input ends early.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut flags = [0u8; 3];
        flags.copy_from_slice(reader.bytes(3)?);
        let game_mode_crc = reader.u32()?;
        let mutator_crc = reader.u32()?;
        let min_level = reader.u32()?;
        let min_gear_score = reader.u32()?;
        let mut tail_flags = [0u8; 3];
        tail_flags.copy_from_slice(reader.bytes(3)?);
        let title_len = usize::from(reader.u8()?);
        let title = reader.bytes(title_len)?.to_vec();
        let created_ns = reader.u64()?;
        Ok(Self {
            flags,
            game_mode_crc,
            mutator_crc,
            min_level,
            min_gear_score,
            tail_flags,
            title,
            created_ns,
        })
    }

    /// Writes the metadata to `writer`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] when `title` exceeds 255 bytes.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let title_len =
            u8::try_from(self.title.len()).map_err(|_| MessageError::CountTooLarge {
                count: self.title.len() as u64,
                limit: u64::from(u8::MAX),
            })?;
        writer
            .bytes(&self.flags)
            .u32(self.game_mode_crc)
            .u32(self.mutator_crc)
            .u32(self.min_level)
            .u32(self.min_gear_score)
            .bytes(&self.tail_flags)
            .u8(title_len)
            .bytes(&self.title)
            .u64(self.created_ns);
        Ok(())
    }
}

/// One member or applicant card in a Group Finder listing (`GroupFinderMemberBlob`, verified
/// against class 1091 and `Playthrough_Part_01` 3032.841 s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFinderMemberBlob {
    /// Character id text (hyphenated UUID).
    pub character_id: String,
    /// Platform id kind (`NO_PLATFORM_ID_KIND` when absent).
    pub platform_id_kind: u32,
    /// Platform id text (empty when absent).
    pub platform_id: String,
    /// Picked role (`GroupFinderRole`, e.g. 2/3/4).
    pub role: u8,
    /// Character level.
    pub level: u32,
    /// Character gear score.
    pub gear_score: u32,
    /// `PlayerIconData` (37 bytes).
    pub icon: [u8; ICON_LEN],
}

impl GroupFinderMemberBlob {
    /// Reads one member blob from `reader`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the input ends early.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let character_id = read_text(reader)?;
        let platform_id_kind = reader.u32()?;
        let platform_id = read_text(reader)?;
        let role = reader.u8()?;
        let level = reader.u32()?;
        let gear_score = reader.u32()?;
        let mut icon = [0u8; ICON_LEN];
        icon.copy_from_slice(reader.bytes(ICON_LEN)?);
        Ok(Self {
            character_id,
            platform_id_kind,
            platform_id,
            role,
            level,
            gear_score,
            icon,
        })
    }

    /// Writes one member blob to `writer`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text is too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        write_text(writer, &self.character_id)?;
        writer.u32(self.platform_id_kind);
        write_text(writer, &self.platform_id)?;
        writer
            .u8(self.role)
            .u32(self.level)
            .u32(self.gear_score)
            .bytes(&self.icon);
        Ok(())
    }
}

/// `Javelin::GroupFinderGroupDataComponentReplicatedState` (class 1091).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupFinderGroupDataState {
    /// Slot 1 child 0 `groupFinderGroupId`.
    pub group_id: Option<[u8; RAW_ID_LEN]>,
    /// Slot 1 child 1 `groupMetadata`.
    pub metadata: Option<GroupFinderGroupMetadata>,
    /// Slot 1 child 2 `memberBlobs`.
    pub member_blobs: Option<FullList<GroupFinderMemberBlob>>,
    /// Slot 1 child 3 `isPendingConversion`.
    pub is_pending_conversion: Option<bool>,
    /// Slot 1 child 4 `pendingConversionConfirmations`.
    pub pending_conversion_confirmations: Option<u8>,
    /// Slot 2 child 0 `hasPendingApplications`.
    pub has_pending_applications: Option<bool>,
    /// Slot 3 child 0 `applicationIds`.
    pub application_ids: Option<FullList<String>>,
    /// Slot 3 child 1 `applicationMemberBlobs`.
    pub application_member_blobs: Option<FullList<GroupFinderMemberBlob>>,
    /// Slot 3 child 2 `applicationStates`.
    pub application_states: Option<FullList<u8>>,
}

impl GroupFinderGroupDataState {
    /// Reads the replicated state from `reader`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body is malformed.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        let slot_mask = read_slot_byte(reader, FIELD_STATE, SLOTS)?;
        if (slot_mask & SLOT_1_BIT) != 0 {
            read_children_64(reader, FIELD_STATE, SLOT_1_CHILDREN, |reader, child| {
                match child {
                    CHILD_GROUP_ID => state.group_id = Some(reader.raw_id()?),
                    CHILD_METADATA => {
                        state.metadata = Some(GroupFinderGroupMetadata::read(reader)?)
                    }
                    CHILD_MEMBER_BLOBS => {
                        state.member_blobs =
                            Some(FullList::read(reader, GroupFinderMemberBlob::read)?);
                    }
                    CHILD_PENDING_CONVERSION => {
                        state.is_pending_conversion = Some(reader.boolean()?);
                    }
                    CHILD_CONVERSION_CONFIRMATIONS => {
                        state.pending_conversion_confirmations = Some(reader.u8()?);
                    }
                    _ => {}
                }
                Ok(())
            })?;
        }
        if (slot_mask & SLOT_2_BIT) != 0 {
            read_children_64(reader, FIELD_STATE, SLOT_2_CHILDREN, |reader, child| {
                if child == CHILD_HAS_PENDING_APPLICATIONS {
                    state.has_pending_applications = Some(reader.boolean()?);
                }
                Ok(())
            })?;
        }
        if (slot_mask & SLOT_3_BIT) != 0 {
            read_children_64(reader, FIELD_STATE, SLOT_3_CHILDREN, |reader, child| {
                match child {
                    CHILD_APPLICATION_IDS => {
                        state.application_ids = Some(FullList::read(reader, read_text)?);
                    }
                    CHILD_APPLICATION_MEMBER_BLOBS => {
                        state.application_member_blobs =
                            Some(FullList::read(reader, GroupFinderMemberBlob::read)?);
                    }
                    CHILD_APPLICATION_STATES => {
                        state.application_states = Some(FullList::read(reader, |r| r.u8())?);
                    }
                    _ => {}
                }
                Ok(())
            })?;
        }
        Ok(state)
    }

    /// Writes the replicated state to `writer`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a field cannot encode.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let slot_1 = bitmask(&[
            self.group_id.is_some(),
            self.metadata.is_some(),
            self.member_blobs.is_some(),
            self.is_pending_conversion.is_some(),
            self.pending_conversion_confirmations.is_some(),
        ]);
        let slot_2 = bitmask(&[self.has_pending_applications.is_some()]);
        let slot_3 = bitmask(&[
            self.application_ids.is_some(),
            self.application_member_blobs.is_some(),
            self.application_states.is_some(),
        ]);
        let mut slot_mask = 0u8;
        if slot_1 != 0 {
            slot_mask |= SLOT_1_BIT;
        }
        if slot_2 != 0 {
            slot_mask |= SLOT_2_BIT;
        }
        if slot_3 != 0 {
            slot_mask |= SLOT_3_BIT;
        }
        writer.u8(slot_mask);
        if slot_1 != 0 {
            write_children_64(writer, slot_1, SLOT_1_CHILDREN, |writer, child| {
                match child {
                    CHILD_GROUP_ID => {
                        if let Some(id) = self.group_id {
                            writer.bytes(&id);
                        }
                    }
                    CHILD_METADATA => {
                        if let Some(metadata) = &self.metadata {
                            metadata.write(writer)?;
                        }
                    }
                    CHILD_MEMBER_BLOBS => {
                        if let Some(blobs) = &self.member_blobs {
                            blobs.write(writer, |w, blob| blob.write(w))?;
                        }
                    }
                    CHILD_PENDING_CONVERSION => {
                        if let Some(flag) = self.is_pending_conversion {
                            writer.boolean(flag);
                        }
                    }
                    CHILD_CONVERSION_CONFIRMATIONS => {
                        if let Some(count) = self.pending_conversion_confirmations {
                            writer.u8(count);
                        }
                    }
                    _ => {}
                }
                Ok::<(), MessageError>(())
            })?;
        }
        if slot_2 != 0 {
            write_children_64(writer, slot_2, SLOT_2_CHILDREN, |writer, child| {
                if child == CHILD_HAS_PENDING_APPLICATIONS
                    && let Some(flag) = self.has_pending_applications
                {
                    writer.boolean(flag);
                }
                Ok::<(), MessageError>(())
            })?;
        }
        if slot_3 != 0 {
            write_children_64(writer, slot_3, SLOT_3_CHILDREN, |writer, child| {
                match child {
                    CHILD_APPLICATION_IDS => {
                        if let Some(ids) = &self.application_ids {
                            ids.write(writer, |w, id| write_text(w, id))?;
                        }
                    }
                    CHILD_APPLICATION_MEMBER_BLOBS => {
                        if let Some(blobs) = &self.application_member_blobs {
                            blobs.write(writer, |w, blob| blob.write(w))?;
                        }
                    }
                    CHILD_APPLICATION_STATES => {
                        if let Some(states) = &self.application_states {
                            states.write(writer, |w, state| {
                                w.u8(*state);
                                Ok(())
                            })?;
                        }
                    }
                    _ => {}
                }
                Ok::<(), MessageError>(())
            })?;
        }
        Ok(())
    }
}

fn bitmask(bits: &[bool]) -> u64 {
    bits.iter().enumerate().fold(
        0u64,
        |acc, (i, on)| if *on { acc | (1u64 << i) } else { acc },
    )
}

/// One entry in `OnGroupFinderGetGroupListSuccess` (`3461`).
///
/// Inferred: no retail ledger in our captures holds a non-empty `3461`; the entry layout matches
/// `2010` (`group_id` + `metadata` + `member_count`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFinderGroupListEntry {
    /// Group Finder listing id.
    pub group_id: [u8; RAW_ID_LEN],
    /// Listing metadata.
    pub metadata: GroupFinderGroupMetadata,
    /// Current member count.
    pub member_count: u8,
}

impl GroupFinderGroupListEntry {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            group_id: reader.raw_id()?,
            metadata: GroupFinderGroupMetadata::read(reader)?,
            member_count: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.bytes(&self.group_id);
        self.metadata.write(writer)?;
        writer.u8(self.member_count);
        Ok(())
    }
}

/// One Group Finder call of the client (all 5 layouts observed in retail captures).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupFinderCall {
    /// 1960: ask for a page of Group Finder listings for `game_mode_crc`.
    GetGroupList {
        /// Filter flag (`1` in retail `07-Tempests_Heart` and `09-The_Ennead`).
        filter_flag: u8,
        /// Lowercase CRC-32 of the activity `GameModeId`.
        game_mode_crc: u32,
        /// Page index (`0` in retail).
        page: u16,
    },
    /// 1147: ask for full details of one Group Finder listing.
    GetGroup {
        /// Group Finder listing id.
        group_id: [u8; RAW_ID_LEN],
    },
    /// 3245: decline an accepted Group Finder application invite.
    DeclineApplicationInvite {
        /// Group Finder listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Application id text (UUID).
        application_id: String,
        /// Decline reason (`0` in retail `Playthrough_Part_01`).
        reason: u8,
    },
    /// 5355: subscribe or unsubscribe from Group Finder application updates.
    ModifyApplicationSubscription {
        /// Whether subscribed.
        subscribed: bool,
    },
    /// 4739: confirm converting the player's Group Finder lobby into a real group.
    ConfirmConvertToGroup,
}

impl GroupFinderCall {
    /// Reads a Group Finder call of `type_index`; `None` for another type.
    pub(crate) fn read(
        type_index: u64,
        reader: &mut Reader<'_>,
    ) -> Option<Result<Self, MessageError>> {
        Some(match type_index {
            GET_GROUP_LIST_TYPE_INDEX => (|| {
                Ok(Self::GetGroupList {
                    filter_flag: reader.u8()?,
                    game_mode_crc: reader.u32()?,
                    page: reader.u16()?,
                })
            })(),
            GET_GROUP_TYPE_INDEX => (|| {
                Ok(Self::GetGroup {
                    group_id: reader.raw_id()?,
                })
            })(),
            DECLINE_APPLICATION_INVITE_TYPE_INDEX => (|| {
                Ok(Self::DeclineApplicationInvite {
                    group_id: reader.raw_id()?,
                    application_id: read_text(reader)?,
                    reason: reader.u8()?,
                })
            })(),
            MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX => (|| {
                Ok(Self::ModifyApplicationSubscription {
                    subscribed: reader.boolean()?,
                })
            })(),
            CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX => Ok(Self::ConfirmConvertToGroup),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::GetGroupList { .. } => GET_GROUP_LIST_TYPE_INDEX,
            Self::GetGroup { .. } => GET_GROUP_TYPE_INDEX,
            Self::DeclineApplicationInvite { .. } => DECLINE_APPLICATION_INVITE_TYPE_INDEX,
            Self::ModifyApplicationSubscription { .. } => {
                MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX
            }
            Self::ConfirmConvertToGroup => CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX,
        }
    }

    /// Encodes the call body with `header`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text is too long.
    pub fn encode(&self, header: FacetHeader) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        header.write(&mut writer);
        match self {
            Self::GetGroupList {
                filter_flag,
                game_mode_crc,
                page,
            } => {
                writer.u8(*filter_flag).u32(*game_mode_crc).u16(*page);
            }
            Self::GetGroup { group_id } => {
                writer.bytes(group_id);
            }
            Self::DeclineApplicationInvite {
                group_id,
                application_id,
                reason,
            } => {
                writer.bytes(group_id);
                write_text(&mut writer, application_id)?;
                writer.u8(*reason);
            }
            Self::ModifyApplicationSubscription { subscribed } => {
                writer.boolean(*subscribed);
            }
            Self::ConfirmConvertToGroup => {}
        }
        Ok(writer.finish())
    }
}

/// One Group Finder push of the server (all 4 layouts observed in retail captures).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupFinderPush {
    /// 3461: a page of Group Finder listings.
    GetGroupListSuccess {
        /// Listings on this page.
        groups: Vec<GroupFinderGroupListEntry>,
        /// Page token byte (`0` in retail empty responses).
        page_token: u8,
        /// Whether this is the last page (`true` in retail empty responses `00 00 01`).
        is_last_page: bool,
    },
    /// 2010: full details of one Group Finder listing.
    GetGroupSuccess {
        /// Group Finder listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Listing metadata.
        metadata: GroupFinderGroupMetadata,
        /// Current members of the listing.
        members: Vec<GroupFinderMemberBlob>,
    },
    /// 3485: the player's application to a Group Finder listing was accepted.
    ApplicationAccepted {
        /// Group Finder listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Application id text (UUID).
        application_id: String,
    },
    /// 3475: the player's decline of an application invite succeeded.
    DeclineApplicationInviteSuccess,
}

impl GroupFinderPush {
    /// The type index of the push.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::GetGroupListSuccess { .. } => GET_GROUP_LIST_SUCCESS_TYPE_INDEX,
            Self::GetGroupSuccess { .. } => GET_GROUP_SUCCESS_TYPE_INDEX,
            Self::ApplicationAccepted { .. } => APPLICATION_ACCEPTED_TYPE_INDEX,
            Self::DeclineApplicationInviteSuccess => DECLINE_APPLICATION_INVITE_SUCCESS_TYPE_INDEX,
        }
    }

    pub(crate) fn read(
        type_index: u64,
        reader: &mut Reader<'_>,
    ) -> Option<Result<Self, MessageError>> {
        Some(match type_index {
            GET_GROUP_LIST_SUCCESS_TYPE_INDEX => (|| {
                let count = usize::from(reader.u8()?);
                let mut groups = Vec::with_capacity(count);
                for _ in 0..count {
                    groups.push(GroupFinderGroupListEntry::read(reader)?);
                }
                let page_token = reader.u8()?;
                let is_last_page = reader.boolean()?;
                Ok(Self::GetGroupListSuccess {
                    groups,
                    page_token,
                    is_last_page,
                })
            })(),
            GET_GROUP_SUCCESS_TYPE_INDEX => (|| {
                let group_id = reader.raw_id()?;
                let metadata = GroupFinderGroupMetadata::read(reader)?;
                let count = usize::from(reader.u8()?);
                let mut members = Vec::with_capacity(count);
                for _ in 0..count {
                    members.push(GroupFinderMemberBlob::read(reader)?);
                }
                Ok(Self::GetGroupSuccess {
                    group_id,
                    metadata,
                    members,
                })
            })(),
            APPLICATION_ACCEPTED_TYPE_INDEX => (|| {
                Ok(Self::ApplicationAccepted {
                    group_id: reader.raw_id()?,
                    application_id: read_text(reader)?,
                })
            })(),
            DECLINE_APPLICATION_INVITE_SUCCESS_TYPE_INDEX => {
                Ok(Self::DeclineApplicationInviteSuccess)
            }
            _ => return None,
        })
    }

    pub(crate) fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        match self {
            Self::GetGroupListSuccess {
                groups,
                page_token,
                is_last_page,
            } => {
                let count =
                    u8::try_from(groups.len()).map_err(|_| MessageError::CountTooLarge {
                        count: groups.len() as u64,
                        limit: u64::from(u8::MAX),
                    })?;
                writer.u8(count);
                for group in groups {
                    group.write(writer)?;
                }
                writer.u8(*page_token).boolean(*is_last_page);
            }
            Self::GetGroupSuccess {
                group_id,
                metadata,
                members,
            } => {
                let count =
                    u8::try_from(members.len()).map_err(|_| MessageError::CountTooLarge {
                        count: members.len() as u64,
                        limit: u64::from(u8::MAX),
                    })?;
                writer.bytes(group_id);
                metadata.write(writer)?;
                writer.u8(count);
                for member in members {
                    member.write(writer)?;
                }
            }
            Self::ApplicationAccepted {
                group_id,
                application_id,
            } => {
                writer.bytes(group_id);
                write_text(writer, application_id)?;
            }
            Self::DeclineApplicationInviteSuccess => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social::{SocialCall, SocialPush, assert_calls_round_trip};

    const HEADER: FacetHeader = FacetHeader {
        target: 0x0102_0304_0506_0708,
        entity: 0x1112_1314_1516_1718,
    };

    #[test]
    fn observed_group_finder_calls_and_pushes_round_trip() {
        let group_id = [0x42; RAW_ID_LEN];
        let app_id = "00000000-0000-0000-0000-000000000001".to_owned();
        let calls = [
            SocialCall::GroupFinder(GroupFinderCall::GetGroupList {
                filter_flag: 1,
                game_mode_crc: 0x7f1c_8a63,
                page: 0,
            }),
            SocialCall::GroupFinder(GroupFinderCall::GetGroup { group_id }),
            SocialCall::GroupFinder(GroupFinderCall::DeclineApplicationInvite {
                group_id,
                application_id: app_id.clone(),
                reason: 0,
            }),
            SocialCall::GroupFinder(GroupFinderCall::ModifyApplicationSubscription {
                subscribed: true,
            }),
            SocialCall::GroupFinder(GroupFinderCall::ConfirmConvertToGroup),
        ];
        assert_calls_round_trip(HEADER, &calls);

        // Retail empty 3461 (`07-Tempests_Heart` 4.383 s): `00 00 01`.
        let empty_list = SocialPush::GroupFinder(GroupFinderPush::GetGroupListSuccess {
            groups: Vec::new(),
            page_token: 0,
            is_last_page: true,
        });
        let args = empty_list.encode_args().expect("encode");
        assert_eq!(args, [0x00, 0x00, 0x01]);
        assert_eq!(
            SocialPush::decode_args(GET_GROUP_LIST_SUCCESS_TYPE_INDEX, &args),
            Some(Ok(empty_list))
        );

        let metadata = GroupFinderGroupMetadata {
            flags: [1, 0, 0],
            game_mode_crc: 0x6a64_c798,
            mutator_crc: 0,
            min_level: 25,
            min_gear_score: 100,
            tail_flags: [0, 0x1c, 0],
            title: b"Test Lobby".to_vec(),
            created_ns: 0x18df_abe1_a9be_c4e8,
        };
        let member = GroupFinderMemberBlob {
            character_id: "00000000-0000-0000-0000-000000000002".to_owned(),
            platform_id_kind: NO_PLATFORM_ID_KIND,
            platform_id: String::new(),
            role: 3,
            level: 65,
            gear_score: 700,
            icon: [0x11; ICON_LEN],
        };
        let pushes = [
            SocialPush::GroupFinder(GroupFinderPush::GetGroupSuccess {
                group_id,
                metadata,
                members: vec![member],
            }),
            SocialPush::GroupFinder(GroupFinderPush::ApplicationAccepted {
                group_id,
                application_id: app_id,
            }),
            SocialPush::GroupFinder(GroupFinderPush::DeclineApplicationInviteSuccess),
        ];
        for push in pushes {
            let args = push.encode_args().expect("encode");
            assert_eq!(
                SocialPush::decode_args(push.type_index(), &args),
                Some(Ok(push))
            );
        }
    }

    #[test]
    fn group_finder_replicated_state_1091_round_trips() {
        let state = GroupFinderGroupDataState {
            group_id: Some([0x55; RAW_ID_LEN]),
            metadata: Some(GroupFinderGroupMetadata {
                flags: [1, 0, 0],
                game_mode_crc: 0x6a64_c798,
                mutator_crc: 0,
                min_level: 25,
                min_gear_score: 100,
                tail_flags: [0, 0x1c, 0],
                title: b"Amrine Run".to_vec(),
                created_ns: 1_700_000_000_000_000_000,
            }),
            member_blobs: Some(FullList::first(vec![GroupFinderMemberBlob {
                character_id: "00000000-0000-0000-0000-000000000003".to_owned(),
                platform_id_kind: NO_PLATFORM_ID_KIND,
                platform_id: String::new(),
                role: 4,
                level: 65,
                gear_score: 725,
                icon: [0x22; ICON_LEN],
            }])),
            is_pending_conversion: Some(false),
            pending_conversion_confirmations: Some(0),
            has_pending_applications: Some(true),
            application_ids: Some(FullList::first(vec![
                "00000000-0000-0000-0000-000000000004".to_owned(),
            ])),
            application_member_blobs: Some(FullList::first(Vec::new())),
            application_states: Some(FullList::first(vec![1])),
        };
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        let decoded = GroupFinderGroupDataState::read(&mut reader).expect("read");
        reader.finish().expect("end");
        assert_eq!(decoded, state);
    }
}
