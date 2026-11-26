// SPDX-License-Identifier: AGPL-3.0-or-later
//! Group Finder calls, pushes and replicated state (`GroupsComponent` and
//! `Javelin::GroupFinderGroupDataComponentReplicatedState`, type 1091).
//!
//! Wire layouts verified from retail captures (`Playthrough_Part_01` 3032.773 s to 3071.193 s,
//! `07-Tempests_Heart` 1.906 s to 9.866 s, `09-The_Ennead`, `08-Barnacles_and_Black_Powder`),
//! the client's replicated-state table ([`crate::member_layouts::table`] class 1091), and the
//! client binary's `GroupsComponent` facet serializers (`FUN_143421300`..`FUN_143426fd0`,
//! `FUN_1434084c0`, `FUN_14340c460`, `FUN_14340e780`, `FUN_143420630`).

use crate::facet::FacetHeader;
use crate::gde::{AssetId, GDE_ID_LEN, GdeMetadataReplicatedState};
use crate::items::FullList;
use crate::lookups::{LOBBY_COUNT_SLOTS, LOBBY_COUNTS_TYPE_INDEX, REQUEST_LOBBY_COUNTS_TYPE_INDEX};
use crate::payload::{BundleMember, MemberState};
use crate::slots::{read_children_64, read_slot_byte, write_children_64};
use crate::social::players::ICON_LEN;
use crate::social::text::{CharacterRef, read_text, write_text};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `Javelin::GroupFinderGroupDataComponentReplicatedState`.
pub const GROUP_FINDER_DATA_TYPE_INDEX: u64 = 1091;
/// Component of the Group Finder entity's spawn card.
pub const GROUP_FINDER_CARD_COMPONENT: u64 = 0;
/// Component of `GroupFinderGroupDataComponentReplicatedState` on the Group Finder entity
/// (Observed: `c1 group=[1091]` in `07-Tempests_Heart` and `08-Barnacles_and_Black_Powder`).
pub const GROUP_FINDER_DATA_COMPONENT: u64 = 1;
/// Asset of `slices/groups/groupfindergroupgde.dynamicslice` from `Engine.pak`
/// `assetcatalog.catalog` (`sub_id = 2`).
pub const GROUP_FINDER_ENTITY_ASSET: AssetId = AssetId {
    id: [
        0xdc, 0x8e, 0xc3, 0x98, 0x03, 0x2d, 0x5e, 0x0a, 0x94, 0x7f, 0x7a, 0x20, 0xc9, 0x70, 0x7a,
        0xe6,
    ],
    sub_id: 2,
};

/// `GroupsComponentServerFacet_RequestCreateGroupFinderGroup`.
pub const CREATE_GROUP_TYPE_INDEX: u64 = 3509;
/// `GroupsComponentClientFacet_OnGroupFinderCreateGroupSuccess`.
pub const CREATE_GROUP_SUCCESS_TYPE_INDEX: u64 = 3374;
/// `GroupsComponentServerFacet_RequestUpdateGroupFinderGroup`.
pub const UPDATE_GROUP_TYPE_INDEX: u64 = 2872;
/// `GroupsComponentClientFacet_OnGroupFinderUpdateGroupSuccess`.
pub const UPDATE_GROUP_SUCCESS_TYPE_INDEX: u64 = 3219;
/// `GroupsComponentServerFacet_RequestRemoveGroupFinderGroup`.
pub const REMOVE_GROUP_TYPE_INDEX: u64 = 1683;
/// `GroupsComponentClientFacet_OnGroupFinderRemoveGroupSuccess`.
pub const REMOVE_GROUP_SUCCESS_TYPE_INDEX: u64 = 3459;
/// `GroupsComponentServerFacet_RequestGetGroupFinderGroupList`.
pub const GET_GROUP_LIST_TYPE_INDEX: u64 = 1960;
/// `GroupsComponentClientFacet_OnGroupFinderGetGroupListSuccess`.
pub const GET_GROUP_LIST_SUCCESS_TYPE_INDEX: u64 = 3461;
/// `GroupsComponentServerFacet_RequestGetGroupFinderGroup`.
pub const GET_GROUP_TYPE_INDEX: u64 = 1147;
/// `GroupsComponentClientFacet_OnGroupFinderGetGroupSuccess`.
pub const GET_GROUP_SUCCESS_TYPE_INDEX: u64 = 2010;
/// `GroupsComponentServerFacet_RequestSubmitGroupFinderApplication`.
pub const SUBMIT_APPLICATION_TYPE_INDEX: u64 = 3510;
/// `GroupsComponentClientFacet_OnGroupFinderSubmitApplicationSuccess`.
pub const SUBMIT_APPLICATION_SUCCESS_TYPE_INDEX: u64 = 861;
/// `GroupsComponentServerFacet_RequestAcceptGroupFinderApplication`.
pub const ACCEPT_APPLICATION_TYPE_INDEX: u64 = 3515;
/// `GroupsComponentClientFacet_OnGroupFinderAcceptApplicationSuccess`.
pub const ACCEPT_APPLICATION_SUCCESS_TYPE_INDEX: u64 = 3470;
/// `GroupsComponentServerFacet_RequestRejectGroupFinderApplication`.
pub const REJECT_APPLICATION_TYPE_INDEX: u64 = 1383;
/// `GroupsComponentClientFacet_OnGroupFinderRejectApplicationSuccess`.
pub const REJECT_APPLICATION_SUCCESS_TYPE_INDEX: u64 = 3473;
/// `GroupsComponentClientFacet_OnGroupFinderApplicationRejected`.
pub const APPLICATION_REJECTED_TYPE_INDEX: u64 = 2985;
/// `GroupsComponentServerFacet_RequestRemoveGroupFinderApplication`.
pub const REMOVE_APPLICATION_TYPE_INDEX: u64 = 3513;
/// `GroupsComponentClientFacet_OnGroupFinderRemoveApplicationSuccess`.
pub const REMOVE_APPLICATION_SUCCESS_TYPE_INDEX: u64 = 3465;
/// `GroupsComponentServerFacet_RequestRemoveAllGroupFinderApplications`.
pub const REMOVE_ALL_APPLICATIONS_TYPE_INDEX: u64 = 1553;
/// `GroupsComponentClientFacet_OnGroupFinderRemoveAllApplicationsSuccess`.
pub const REMOVE_ALL_APPLICATIONS_SUCCESS_TYPE_INDEX: u64 = 3468;
/// `GroupsComponentClientFacet_OnGroupFinderApplicationAccepted`.
pub const APPLICATION_ACCEPTED_TYPE_INDEX: u64 = 3485;
/// `GroupsComponentServerFacet_RequestConfirmGroupFinderApplicationInvite`.
pub const CONFIRM_APPLICATION_INVITE_TYPE_INDEX: u64 = 105;
/// `GroupsComponentClientFacet_OnGroupFinderConfirmApplicationInviteSuccess`.
pub const CONFIRM_APPLICATION_INVITE_SUCCESS_TYPE_INDEX: u64 = 1708;
/// `GroupsComponentServerFacet_RequestDeclineGroupFinderApplicationInvite`.
pub const DECLINE_APPLICATION_INVITE_TYPE_INDEX: u64 = 3245;
/// `GroupsComponentClientFacet_OnGroupFinderDeclineApplicationInviteSuccess`.
pub const DECLINE_APPLICATION_INVITE_SUCCESS_TYPE_INDEX: u64 = 3475;
/// `GroupsComponentServerFacet_RequestUpdateMemberInGroupFinderGroup`.
pub const UPDATE_MEMBER_TYPE_INDEX: u64 = 3518;
/// `GroupsComponentClientFacet_OnGroupFinderUpdateMemberSuccess`.
pub const UPDATE_MEMBER_SUCCESS_TYPE_INDEX: u64 = 3480;
/// `GroupsComponentServerFacet_RequestRemoveMemberFromGroupFinderGroup`.
pub const REMOVE_MEMBER_TYPE_INDEX: u64 = 3519;
/// `GroupsComponentClientFacet_OnGroupFinderRemoveMemberSuccess`.
pub const REMOVE_MEMBER_SUCCESS_TYPE_INDEX: u64 = 3482;
/// `GroupsComponentClientFacet_OnGroupFinderKickedFromGroup`.
pub const KICKED_FROM_GROUP_TYPE_INDEX: u64 = 5239;
/// `GroupsComponentServerFacet_RequestModifyGroupFinderApplicationSubscription`.
pub const MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX: u64 = 5355;
/// `GroupsComponentServerFacet_RequestConvertGroupFinderGroupToGroup`.
pub const CONVERT_TO_GROUP_TYPE_INDEX: u64 = 4723;
/// `GroupsComponentServerFacet_RequestConfirmConvertGroupFinderGroupToGroup`.
pub const CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX: u64 = 4739;
/// `GroupsComponentServerFacet_RequestDeclineConvertGroupFinderGroupToGroup`.
pub const DECLINE_CONVERT_TO_GROUP_TYPE_INDEX: u64 = 4744;

/// The `createSource` byte of `GroupsState` (`1994`) when a group was formed from Group Finder
/// (Observed: `07-Tempests_Heart`, `09-The_Ennead`).
pub const GROUP_FINDER_CREATE_SOURCE: u8 = 1;
/// `platform_id_kind` when a member has no platform id (Observed: `0xffff_ffff` in
/// `Playthrough_Part_01` 3032.841 s).
pub const NO_PLATFORM_ID_KIND: u32 = u32::MAX;

/// Maximum role sets in `3509` (`FUN_1434084c0`: `uVar3 < 6`).
const MAX_ROLE_SETS: usize = 5;
/// Maximum bytes per role set in `3509` (`FUN_1434084c0`: `< 0x29`).
const MAX_ROLE_SET_LEN: usize = 40;

/// All Group Finder client calls routed through `SocialCall::GroupFinder`.
pub(crate) const CALL_TYPES: [u64; 19] = [
    CREATE_GROUP_TYPE_INDEX,
    UPDATE_GROUP_TYPE_INDEX,
    REMOVE_GROUP_TYPE_INDEX,
    GET_GROUP_LIST_TYPE_INDEX,
    GET_GROUP_TYPE_INDEX,
    SUBMIT_APPLICATION_TYPE_INDEX,
    ACCEPT_APPLICATION_TYPE_INDEX,
    REJECT_APPLICATION_TYPE_INDEX,
    REMOVE_APPLICATION_TYPE_INDEX,
    REMOVE_ALL_APPLICATIONS_TYPE_INDEX,
    CONFIRM_APPLICATION_INVITE_TYPE_INDEX,
    DECLINE_APPLICATION_INVITE_TYPE_INDEX,
    UPDATE_MEMBER_TYPE_INDEX,
    REMOVE_MEMBER_TYPE_INDEX,
    MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX,
    CONVERT_TO_GROUP_TYPE_INDEX,
    CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX,
    DECLINE_CONVERT_TO_GROUP_TYPE_INDEX,
    REQUEST_LOBBY_COUNTS_TYPE_INDEX,
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

/// Metadata of a Group Finder listing (`GroupFinderGroupMetadata`, verified against class 1091,
/// `FUN_14344ac60`, and the 282-byte `2010` sample of `Playthrough_Part_01` 3032.841 s).
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
/// against class 1091, `FUN_14344ad00`, and `Playthrough_Part_01` 3032.841 s).
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

/// Bundle members for a Group Finder entity (`group_id`, `state`, and spawn card when `with_card`).
pub fn group_finder_entity_members(
    group_id: [u8; GDE_ID_LEN],
    state: GroupFinderGroupDataState,
    with_card: bool,
) -> Vec<BundleMember> {
    let mut members = Vec::with_capacity(if with_card { 2 } else { 1 });
    if with_card {
        members.push(BundleMember {
            component: GROUP_FINDER_CARD_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(GROUP_FINDER_ENTITY_ASSET),
                gde_ref: Some(group_id),
            }),
        });
    }
    members.push(BundleMember {
        component: GROUP_FINDER_DATA_COMPONENT,
        state: MemberState::GroupFinderData(Box::new(state)),
    });
    members
}

fn bitmask(bits: &[bool]) -> u64 {
    bits.iter().enumerate().fold(
        0u64,
        |acc, (i, on)| if *on { acc | (1u64 << i) } else { acc },
    )
}

/// 100-byte tail block of `GroupFinderGroupListEntry` (`FUN_143420630`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GroupFinderListEntryTail {
    /// Leading `u32` (`FUN_140876e70`).
    pub flags: u32,
    /// First 16-byte id.
    pub id_1: [u8; RAW_ID_LEN],
    /// Second 16-byte id.
    pub id_2: [u8; RAW_ID_LEN],
    /// Third 16-byte id.
    pub id_3: [u8; RAW_ID_LEN],
    /// First `u64` pair.
    pub value_1: u64,
    /// Second `u64`.
    pub value_2: u64,
    /// Fourth 16-byte id.
    pub id_4: [u8; RAW_ID_LEN],
    /// Third `u64`.
    pub value_3: u64,
    /// Fourth `u64`.
    pub value_4: u64,
}

impl GroupFinderListEntryTail {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            flags: reader.u32()?,
            id_1: reader.raw_id()?,
            id_2: reader.raw_id()?,
            id_3: reader.raw_id()?,
            value_1: reader.u64()?,
            value_2: reader.u64()?,
            id_4: reader.raw_id()?,
            value_3: reader.u64()?,
            value_4: reader.u64()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .u32(self.flags)
            .bytes(&self.id_1)
            .bytes(&self.id_2)
            .bytes(&self.id_3)
            .u64(self.value_1)
            .u64(self.value_2)
            .bytes(&self.id_4)
            .u64(self.value_3)
            .u64(self.value_4);
    }
}

/// One entry in `OnGroupFinderGetGroupListSuccess` (`3461`), verified from client serializer
/// `FUN_14340e780` (`group_id`, `metadata`, `members`, `world_name`, `leader_id`, `tail`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFinderGroupListEntry {
    /// Group Finder listing id.
    pub group_id: [u8; RAW_ID_LEN],
    /// Listing metadata.
    pub metadata: GroupFinderGroupMetadata,
    /// Current member blobs (`FUN_143420590`).
    pub members: Vec<GroupFinderMemberBlob>,
    /// Leader world/realm text (`FUN_14344a920`).
    pub world_name: String,
    /// Leader character reference (`FUN_140877140`).
    pub leader_id: CharacterRef,
    /// 100-byte tail block (`FUN_143420630`).
    pub tail: GroupFinderListEntryTail,
}

impl GroupFinderGroupListEntry {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let group_id = reader.raw_id()?;
        let metadata = GroupFinderGroupMetadata::read(reader)?;
        let count = usize::from(reader.u8()?);
        let mut members = Vec::with_capacity(count);
        for _ in 0..count {
            members.push(GroupFinderMemberBlob::read(reader)?);
        }
        let world_name = read_text(reader)?;
        let leader_id = CharacterRef::read(reader)?;
        let tail = GroupFinderListEntryTail::read(reader)?;
        Ok(Self {
            group_id,
            metadata,
            members,
            world_name,
            leader_id,
            tail,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let count = u8::try_from(self.members.len()).map_err(|_| MessageError::CountTooLarge {
            count: self.members.len() as u64,
            limit: u64::from(u8::MAX),
        })?;
        writer.bytes(&self.group_id);
        self.metadata.write(writer)?;
        writer.u8(count);
        for member in &self.members {
            member.write(writer)?;
        }
        write_text(writer, &self.world_name)?;
        self.leader_id.write(writer);
        self.tail.write(writer);
        Ok(())
    }
}

/// One Group Finder call of the client (verified from retail captures and client serializers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupFinderCall {
    /// 3509: create a Group Finder listing (`FUN_1434084c0`).
    CreateGroup {
        /// Listing metadata.
        metadata: GroupFinderGroupMetadata,
        /// Creator's chosen role.
        role: u8,
        /// Up to 5 role-set byte arrays (each up to 40 bytes).
        role_sets: Vec<Vec<u8>>,
    },
    /// 2872: update the player's Group Finder listing metadata (`FUN_14340c460`).
    UpdateGroup {
        /// Updated metadata.
        metadata: GroupFinderGroupMetadata,
    },
    /// 1683: remove/disband the player's Group Finder listing (`FUN_14259f170`).
    RemoveGroup,
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
    /// 3510: apply to join a Group Finder listing (`FUN_143407220`).
    SubmitApplication {
        /// Target listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Chosen role.
        role: u8,
    },
    /// 3515: leader accepts a pending application (`FUN_14340c060`).
    AcceptApplication {
        /// Listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Application id text (UUID).
        application_id: String,
    },
    /// 1383: leader rejects a pending application (`FUN_14340c060`).
    RejectApplication {
        /// Listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Application id text (UUID).
        application_id: String,
    },
    /// 3513: applicant withdraws one pending application (`FUN_14340c060`).
    RemoveApplication {
        /// Listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Application id text (UUID).
        application_id: String,
    },
    /// 1553: applicant withdraws all pending applications (`FUN_14259f170`).
    RemoveAllApplications,
    /// 105: applicant confirms an accepted invite (`FUN_1433fb0c0`).
    ConfirmApplicationInvite {
        /// Listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Application id text (UUID).
        application_id: String,
        /// Role / flags byte.
        role: u8,
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
    /// 3518: update the player's role inside its Group Finder listing (`FUN_142746100`).
    UpdateMember {
        /// New role.
        role: u8,
    },
    /// 3519: remove/kick a member from the player's Group Finder listing (`FUN_1433fc730`).
    RemoveMember {
        /// Target character id text (UUID).
        character_id: String,
    },
    /// 5355: subscribe or unsubscribe from Group Finder application updates.
    ModifyApplicationSubscription {
        /// Whether subscribed.
        subscribed: bool,
    },
    /// 4723: leader initiates converting the Group Finder lobby into a real group (`FUN_14259f170`).
    ConvertToGroup,
    /// 4739: confirm converting the player's Group Finder lobby into a real group.
    ConfirmConvertToGroup,
    /// 4744: decline converting the player's Group Finder lobby into a real group (`FUN_14259f170`).
    DeclineConvertToGroup,
    /// 5463: ask for joinable Group Finder lobby counts for `activity`.
    LobbyCounts {
        /// The activity index (`u8`, matching `lookups::LookupRequest::LobbyCounts`).
        activity: u8,
    },
}

impl GroupFinderCall {
    /// Reads a Group Finder call of `type_index`; `None` for another type.
    pub(crate) fn read(
        type_index: u64,
        reader: &mut Reader<'_>,
    ) -> Option<Result<Self, MessageError>> {
        Some(match type_index {
            CREATE_GROUP_TYPE_INDEX => (|| {
                let metadata = GroupFinderGroupMetadata::read(reader)?;
                let role = reader.u8()?;
                let set_count = usize::from(reader.u8()?);
                if set_count > MAX_ROLE_SETS {
                    return Err(MessageError::CountTooLarge {
                        count: set_count as u64,
                        limit: MAX_ROLE_SETS as u64,
                    });
                }
                let mut role_sets = Vec::with_capacity(set_count);
                for _ in 0..set_count {
                    let len = usize::from(reader.u8()?);
                    if len > MAX_ROLE_SET_LEN {
                        return Err(MessageError::CountTooLarge {
                            count: len as u64,
                            limit: MAX_ROLE_SET_LEN as u64,
                        });
                    }
                    role_sets.push(reader.bytes(len)?.to_vec());
                }
                Ok(Self::CreateGroup {
                    metadata,
                    role,
                    role_sets,
                })
            })(),
            UPDATE_GROUP_TYPE_INDEX => (|| {
                Ok(Self::UpdateGroup {
                    metadata: GroupFinderGroupMetadata::read(reader)?,
                })
            })(),
            REMOVE_GROUP_TYPE_INDEX => Ok(Self::RemoveGroup),
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
            SUBMIT_APPLICATION_TYPE_INDEX => (|| {
                Ok(Self::SubmitApplication {
                    group_id: reader.raw_id()?,
                    role: reader.u8()?,
                })
            })(),
            ACCEPT_APPLICATION_TYPE_INDEX => (|| {
                Ok(Self::AcceptApplication {
                    group_id: reader.raw_id()?,
                    application_id: read_text(reader)?,
                })
            })(),
            REJECT_APPLICATION_TYPE_INDEX => (|| {
                Ok(Self::RejectApplication {
                    group_id: reader.raw_id()?,
                    application_id: read_text(reader)?,
                })
            })(),
            REMOVE_APPLICATION_TYPE_INDEX => (|| {
                Ok(Self::RemoveApplication {
                    group_id: reader.raw_id()?,
                    application_id: read_text(reader)?,
                })
            })(),
            REMOVE_ALL_APPLICATIONS_TYPE_INDEX => Ok(Self::RemoveAllApplications),
            CONFIRM_APPLICATION_INVITE_TYPE_INDEX => (|| {
                Ok(Self::ConfirmApplicationInvite {
                    group_id: reader.raw_id()?,
                    application_id: read_text(reader)?,
                    role: reader.u8()?,
                })
            })(),
            DECLINE_APPLICATION_INVITE_TYPE_INDEX => (|| {
                Ok(Self::DeclineApplicationInvite {
                    group_id: reader.raw_id()?,
                    application_id: read_text(reader)?,
                    reason: reader.u8()?,
                })
            })(),
            UPDATE_MEMBER_TYPE_INDEX => (|| Ok(Self::UpdateMember { role: reader.u8()? }))(),
            REMOVE_MEMBER_TYPE_INDEX => (|| {
                Ok(Self::RemoveMember {
                    character_id: read_text(reader)?,
                })
            })(),
            MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX => (|| {
                Ok(Self::ModifyApplicationSubscription {
                    subscribed: reader.boolean()?,
                })
            })(),
            CONVERT_TO_GROUP_TYPE_INDEX => Ok(Self::ConvertToGroup),
            CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX => Ok(Self::ConfirmConvertToGroup),
            DECLINE_CONVERT_TO_GROUP_TYPE_INDEX => Ok(Self::DeclineConvertToGroup),
            REQUEST_LOBBY_COUNTS_TYPE_INDEX => (|| {
                Ok(Self::LobbyCounts {
                    activity: reader.u8()?,
                })
            })(),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::CreateGroup { .. } => CREATE_GROUP_TYPE_INDEX,
            Self::UpdateGroup { .. } => UPDATE_GROUP_TYPE_INDEX,
            Self::RemoveGroup => REMOVE_GROUP_TYPE_INDEX,
            Self::GetGroupList { .. } => GET_GROUP_LIST_TYPE_INDEX,
            Self::GetGroup { .. } => GET_GROUP_TYPE_INDEX,
            Self::SubmitApplication { .. } => SUBMIT_APPLICATION_TYPE_INDEX,
            Self::AcceptApplication { .. } => ACCEPT_APPLICATION_TYPE_INDEX,
            Self::RejectApplication { .. } => REJECT_APPLICATION_TYPE_INDEX,
            Self::RemoveApplication { .. } => REMOVE_APPLICATION_TYPE_INDEX,
            Self::RemoveAllApplications => REMOVE_ALL_APPLICATIONS_TYPE_INDEX,
            Self::ConfirmApplicationInvite { .. } => CONFIRM_APPLICATION_INVITE_TYPE_INDEX,
            Self::DeclineApplicationInvite { .. } => DECLINE_APPLICATION_INVITE_TYPE_INDEX,
            Self::UpdateMember { .. } => UPDATE_MEMBER_TYPE_INDEX,
            Self::RemoveMember { .. } => REMOVE_MEMBER_TYPE_INDEX,
            Self::ModifyApplicationSubscription { .. } => {
                MODIFY_APPLICATION_SUBSCRIPTION_TYPE_INDEX
            }
            Self::ConvertToGroup => CONVERT_TO_GROUP_TYPE_INDEX,
            Self::ConfirmConvertToGroup => CONFIRM_CONVERT_TO_GROUP_TYPE_INDEX,
            Self::DeclineConvertToGroup => DECLINE_CONVERT_TO_GROUP_TYPE_INDEX,
            Self::LobbyCounts { .. } => REQUEST_LOBBY_COUNTS_TYPE_INDEX,
        }
    }

    /// Encodes the call body with `header`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a field does not fit.
    pub fn encode(&self, header: FacetHeader) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        header.write(&mut writer);
        match self {
            Self::CreateGroup {
                metadata,
                role,
                role_sets,
            } => {
                if role_sets.len() > MAX_ROLE_SETS {
                    return Err(MessageError::CountTooLarge {
                        count: role_sets.len() as u64,
                        limit: MAX_ROLE_SETS as u64,
                    });
                }
                metadata.write(&mut writer)?;
                writer.u8(*role);
                writer.u8(u8::try_from(role_sets.len()).unwrap_or(0));
                for set in role_sets {
                    if set.len() > MAX_ROLE_SET_LEN {
                        return Err(MessageError::CountTooLarge {
                            count: set.len() as u64,
                            limit: MAX_ROLE_SET_LEN as u64,
                        });
                    }
                    writer.u8(u8::try_from(set.len()).unwrap_or(0)).bytes(set);
                }
            }
            Self::UpdateGroup { metadata } => metadata.write(&mut writer)?,
            Self::RemoveGroup
            | Self::RemoveAllApplications
            | Self::ConvertToGroup
            | Self::ConfirmConvertToGroup
            | Self::DeclineConvertToGroup => {}
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
            Self::SubmitApplication { group_id, role } => {
                writer.bytes(group_id).u8(*role);
            }
            Self::AcceptApplication {
                group_id,
                application_id,
            }
            | Self::RejectApplication {
                group_id,
                application_id,
            }
            | Self::RemoveApplication {
                group_id,
                application_id,
            } => {
                writer.bytes(group_id);
                write_text(&mut writer, application_id)?;
            }
            Self::ConfirmApplicationInvite {
                group_id,
                application_id,
                role,
            } => {
                writer.bytes(group_id);
                write_text(&mut writer, application_id)?;
                writer.u8(*role);
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
            Self::UpdateMember { role } => {
                writer.u8(*role);
            }
            Self::RemoveMember { character_id } => {
                write_text(&mut writer, character_id)?;
            }
            Self::ModifyApplicationSubscription { subscribed } => {
                writer.boolean(*subscribed);
            }
            Self::LobbyCounts { activity } => {
                writer.u8(*activity);
            }
        }
        Ok(writer.finish())
    }
}

/// One Group Finder push of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupFinderPush {
    /// 3374: listing created.
    CreateGroupSuccess,
    /// 3219: listing updated.
    UpdateGroupSuccess,
    /// 3459: listing removed.
    RemoveGroupSuccess,
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
    /// 861: application submitted.
    SubmitApplicationSuccess,
    /// 3470: application accepted (sent to leader).
    AcceptApplicationSuccess,
    /// 3473: application rejected (sent to leader).
    RejectApplicationSuccess,
    /// 2985: application rejected (sent to applicant).
    ApplicationRejected {
        /// Listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Application id text (UUID).
        application_id: String,
    },
    /// 3465: application removed.
    RemoveApplicationSuccess,
    /// 3468: all applications removed.
    RemoveAllApplicationsSuccess,
    /// 3485: the player's application to a Group Finder listing was accepted.
    ApplicationAccepted {
        /// Group Finder listing id.
        group_id: [u8; RAW_ID_LEN],
        /// Application id text (UUID).
        application_id: String,
    },
    /// 1708: application invite confirmed.
    ConfirmApplicationInviteSuccess,
    /// 3475: the player's decline of an application invite succeeded.
    DeclineApplicationInviteSuccess,
    /// 3480: member role updated.
    UpdateMemberSuccess,
    /// 3482: member removed.
    RemoveMemberSuccess,
    /// 5239: member kicked from Group Finder listing.
    KickedFromGroup,
    /// 5458: joinable Group Finder lobby counts for `activity` (`LOBBY_COUNT_SLOTS` slots).
    LobbyCounts {
        /// The activity index (`u8`, matching `lookups::LobbyCountsMsg`).
        activity: u8,
        /// Open lobby count per slot (`LOBBY_COUNT_SLOTS` = 132).
        counts: Vec<u32>,
    },
}

impl GroupFinderPush {
    /// The type index of the push.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::CreateGroupSuccess => CREATE_GROUP_SUCCESS_TYPE_INDEX,
            Self::UpdateGroupSuccess => UPDATE_GROUP_SUCCESS_TYPE_INDEX,
            Self::RemoveGroupSuccess => REMOVE_GROUP_SUCCESS_TYPE_INDEX,
            Self::GetGroupListSuccess { .. } => GET_GROUP_LIST_SUCCESS_TYPE_INDEX,
            Self::GetGroupSuccess { .. } => GET_GROUP_SUCCESS_TYPE_INDEX,
            Self::SubmitApplicationSuccess => SUBMIT_APPLICATION_SUCCESS_TYPE_INDEX,
            Self::AcceptApplicationSuccess => ACCEPT_APPLICATION_SUCCESS_TYPE_INDEX,
            Self::RejectApplicationSuccess => REJECT_APPLICATION_SUCCESS_TYPE_INDEX,
            Self::ApplicationRejected { .. } => APPLICATION_REJECTED_TYPE_INDEX,
            Self::RemoveApplicationSuccess => REMOVE_APPLICATION_SUCCESS_TYPE_INDEX,
            Self::RemoveAllApplicationsSuccess => REMOVE_ALL_APPLICATIONS_SUCCESS_TYPE_INDEX,
            Self::ApplicationAccepted { .. } => APPLICATION_ACCEPTED_TYPE_INDEX,
            Self::ConfirmApplicationInviteSuccess => CONFIRM_APPLICATION_INVITE_SUCCESS_TYPE_INDEX,
            Self::DeclineApplicationInviteSuccess => DECLINE_APPLICATION_INVITE_SUCCESS_TYPE_INDEX,
            Self::UpdateMemberSuccess => UPDATE_MEMBER_SUCCESS_TYPE_INDEX,
            Self::RemoveMemberSuccess => REMOVE_MEMBER_SUCCESS_TYPE_INDEX,
            Self::KickedFromGroup => KICKED_FROM_GROUP_TYPE_INDEX,
            Self::LobbyCounts { .. } => LOBBY_COUNTS_TYPE_INDEX,
        }
    }

    pub(crate) fn read(
        type_index: u64,
        reader: &mut Reader<'_>,
    ) -> Option<Result<Self, MessageError>> {
        Some(match type_index {
            CREATE_GROUP_SUCCESS_TYPE_INDEX => Ok(Self::CreateGroupSuccess),
            UPDATE_GROUP_SUCCESS_TYPE_INDEX => Ok(Self::UpdateGroupSuccess),
            REMOVE_GROUP_SUCCESS_TYPE_INDEX => Ok(Self::RemoveGroupSuccess),
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
            SUBMIT_APPLICATION_SUCCESS_TYPE_INDEX => Ok(Self::SubmitApplicationSuccess),
            ACCEPT_APPLICATION_SUCCESS_TYPE_INDEX => Ok(Self::AcceptApplicationSuccess),
            REJECT_APPLICATION_SUCCESS_TYPE_INDEX => Ok(Self::RejectApplicationSuccess),
            APPLICATION_REJECTED_TYPE_INDEX => (|| {
                Ok(Self::ApplicationRejected {
                    group_id: reader.raw_id()?,
                    application_id: read_text(reader)?,
                })
            })(),
            REMOVE_APPLICATION_SUCCESS_TYPE_INDEX => Ok(Self::RemoveApplicationSuccess),
            REMOVE_ALL_APPLICATIONS_SUCCESS_TYPE_INDEX => Ok(Self::RemoveAllApplicationsSuccess),
            APPLICATION_ACCEPTED_TYPE_INDEX => (|| {
                Ok(Self::ApplicationAccepted {
                    group_id: reader.raw_id()?,
                    application_id: read_text(reader)?,
                })
            })(),
            CONFIRM_APPLICATION_INVITE_SUCCESS_TYPE_INDEX => {
                Ok(Self::ConfirmApplicationInviteSuccess)
            }
            DECLINE_APPLICATION_INVITE_SUCCESS_TYPE_INDEX => {
                Ok(Self::DeclineApplicationInviteSuccess)
            }
            UPDATE_MEMBER_SUCCESS_TYPE_INDEX => Ok(Self::UpdateMemberSuccess),
            REMOVE_MEMBER_SUCCESS_TYPE_INDEX => Ok(Self::RemoveMemberSuccess),
            KICKED_FROM_GROUP_TYPE_INDEX => Ok(Self::KickedFromGroup),
            LOBBY_COUNTS_TYPE_INDEX => (|| {
                let activity = reader.u8()?;
                let count = reader.prefixed()?;
                let limit =
                    u64::try_from(reader.remaining() / size_of::<u32>()).unwrap_or(u64::MAX);
                if count > limit || count > LOBBY_COUNT_SLOTS as u64 {
                    return Err(MessageError::CountTooLarge {
                        count,
                        limit: limit.min(LOBBY_COUNT_SLOTS as u64),
                    });
                }
                let mut counts = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
                for _ in 0..count {
                    counts.push(reader.u32()?);
                }
                Ok(Self::LobbyCounts { activity, counts })
            })(),
            _ => return None,
        })
    }

    pub(crate) fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        match self {
            Self::CreateGroupSuccess
            | Self::UpdateGroupSuccess
            | Self::RemoveGroupSuccess
            | Self::SubmitApplicationSuccess
            | Self::AcceptApplicationSuccess
            | Self::RejectApplicationSuccess
            | Self::RemoveApplicationSuccess
            | Self::RemoveAllApplicationsSuccess
            | Self::ConfirmApplicationInviteSuccess
            | Self::DeclineApplicationInviteSuccess
            | Self::UpdateMemberSuccess
            | Self::RemoveMemberSuccess
            | Self::KickedFromGroup => {}
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
            }
            | Self::ApplicationRejected {
                group_id,
                application_id,
            } => {
                writer.bytes(group_id);
                write_text(writer, application_id)?;
            }
            Self::LobbyCounts { activity, counts } => {
                writer.u8(*activity);
                writer.prefixed(u64::try_from(counts.len()).unwrap_or(u64::MAX))?;
                for slot in counts {
                    writer.u32(*slot);
                }
            }
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

    fn sample_metadata() -> GroupFinderGroupMetadata {
        GroupFinderGroupMetadata {
            flags: [1, 0, 0],
            game_mode_crc: 0x6a64_c798,
            mutator_crc: 0,
            min_level: 25,
            min_gear_score: 100,
            tail_flags: [0, 0x1c, 0],
            title: b"Test Lobby".to_vec(),
            created_ns: 0x18df_abe1_a9be_c4e8,
        }
    }

    fn sample_member() -> GroupFinderMemberBlob {
        GroupFinderMemberBlob {
            character_id: "00000000-0000-0000-0000-000000000002".to_owned(),
            platform_id_kind: NO_PLATFORM_ID_KIND,
            platform_id: String::new(),
            role: 3,
            level: 65,
            gear_score: 700,
            icon: [0x11; ICON_LEN],
        }
    }

    #[test]
    fn all_group_finder_calls_and_pushes_round_trip() {
        let group_id = [0x42; RAW_ID_LEN];
        let app_id = "00000000-0000-0000-0000-000000000001".to_owned();
        let calls = [
            SocialCall::GroupFinder(GroupFinderCall::CreateGroup {
                metadata: sample_metadata(),
                role: 4,
                role_sets: vec![vec![2, 3, 4]],
            }),
            SocialCall::GroupFinder(GroupFinderCall::UpdateGroup {
                metadata: sample_metadata(),
            }),
            SocialCall::GroupFinder(GroupFinderCall::RemoveGroup),
            SocialCall::GroupFinder(GroupFinderCall::GetGroupList {
                filter_flag: 1,
                game_mode_crc: 0x7f1c_8a63,
                page: 0,
            }),
            SocialCall::GroupFinder(GroupFinderCall::GetGroup { group_id }),
            SocialCall::GroupFinder(GroupFinderCall::SubmitApplication { group_id, role: 3 }),
            SocialCall::GroupFinder(GroupFinderCall::AcceptApplication {
                group_id,
                application_id: app_id.clone(),
            }),
            SocialCall::GroupFinder(GroupFinderCall::RejectApplication {
                group_id,
                application_id: app_id.clone(),
            }),
            SocialCall::GroupFinder(GroupFinderCall::RemoveApplication {
                group_id,
                application_id: app_id.clone(),
            }),
            SocialCall::GroupFinder(GroupFinderCall::RemoveAllApplications),
            SocialCall::GroupFinder(GroupFinderCall::ConfirmApplicationInvite {
                group_id,
                application_id: app_id.clone(),
                role: 3,
            }),
            SocialCall::GroupFinder(GroupFinderCall::DeclineApplicationInvite {
                group_id,
                application_id: app_id.clone(),
                reason: 0,
            }),
            SocialCall::GroupFinder(GroupFinderCall::UpdateMember { role: 2 }),
            SocialCall::GroupFinder(GroupFinderCall::RemoveMember {
                character_id: "00000000-0000-0000-0000-000000000002".to_owned(),
            }),
            SocialCall::GroupFinder(GroupFinderCall::ModifyApplicationSubscription {
                subscribed: true,
            }),
            SocialCall::GroupFinder(GroupFinderCall::ConvertToGroup),
            SocialCall::GroupFinder(GroupFinderCall::ConfirmConvertToGroup),
            SocialCall::GroupFinder(GroupFinderCall::DeclineConvertToGroup),
            SocialCall::GroupFinder(GroupFinderCall::LobbyCounts { activity: 1 }),
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

        let non_empty_list = SocialPush::GroupFinder(GroupFinderPush::GetGroupListSuccess {
            groups: vec![GroupFinderGroupListEntry {
                group_id,
                metadata: sample_metadata(),
                members: vec![sample_member()],
                world_name: "TestWorld".to_owned(),
                leader_id: CharacterRef(Some([0x77; RAW_ID_LEN])),
                tail: GroupFinderListEntryTail::default(),
            }],
            page_token: 0,
            is_last_page: true,
        });
        let pushes = [
            SocialPush::GroupFinder(GroupFinderPush::CreateGroupSuccess),
            SocialPush::GroupFinder(GroupFinderPush::UpdateGroupSuccess),
            SocialPush::GroupFinder(GroupFinderPush::RemoveGroupSuccess),
            non_empty_list,
            SocialPush::GroupFinder(GroupFinderPush::GetGroupSuccess {
                group_id,
                metadata: sample_metadata(),
                members: vec![sample_member()],
            }),
            SocialPush::GroupFinder(GroupFinderPush::SubmitApplicationSuccess),
            SocialPush::GroupFinder(GroupFinderPush::AcceptApplicationSuccess),
            SocialPush::GroupFinder(GroupFinderPush::RejectApplicationSuccess),
            SocialPush::GroupFinder(GroupFinderPush::ApplicationRejected {
                group_id,
                application_id: app_id.clone(),
            }),
            SocialPush::GroupFinder(GroupFinderPush::RemoveApplicationSuccess),
            SocialPush::GroupFinder(GroupFinderPush::RemoveAllApplicationsSuccess),
            SocialPush::GroupFinder(GroupFinderPush::ApplicationAccepted {
                group_id,
                application_id: app_id,
            }),
            SocialPush::GroupFinder(GroupFinderPush::ConfirmApplicationInviteSuccess),
            SocialPush::GroupFinder(GroupFinderPush::DeclineApplicationInviteSuccess),
            SocialPush::GroupFinder(GroupFinderPush::UpdateMemberSuccess),
            SocialPush::GroupFinder(GroupFinderPush::RemoveMemberSuccess),
            SocialPush::GroupFinder(GroupFinderPush::KickedFromGroup),
            SocialPush::GroupFinder(GroupFinderPush::LobbyCounts {
                activity: 1,
                counts: vec![2; LOBBY_COUNT_SLOTS],
            }),
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
    fn group_finder_replicated_state_1091_and_entity_members_round_trip() {
        let state = GroupFinderGroupDataState {
            group_id: Some([0x55; RAW_ID_LEN]),
            metadata: Some(sample_metadata()),
            member_blobs: Some(FullList::first(vec![sample_member()])),
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

        let members = group_finder_entity_members([0x55; GDE_ID_LEN], state, true);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].component, GROUP_FINDER_CARD_COMPONENT);
        assert_eq!(members[1].component, GROUP_FINDER_DATA_COMPONENT);
    }
}
