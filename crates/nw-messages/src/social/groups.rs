// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GroupsComponent` calls and pushes of a plain group: invite, answer, leave, leader, kick vote.
//! Layouts from the retail sessions `Hive 3v3 group`, `3v3_arena_group`, `dungeon_dynasty_tank_`,
//! `invasion` and `msq14` (6 invites, 27 answers, 15 leaves, 47 removed invites, 297 removed
//! members), with the argument kinds of the Azoth catalog for 3502 and 3498.
//!
//! Retail flow (Observed): an invite (3492) names the invitee; when the invite ends, both sides
//! get a 1677 with the invite id: the inviter with the invitee and `outbound` 1, the invitee with
//! the inviter and `outbound` 0. A member who leaves (2252) gets a 5283 for each other member,
//! reason [`REMOVED_LEFT`]. An invite of an offline player gets a 3453 with
//! [`ERROR_INVITEE_OFFLINE`] and the invitee.

use crate::facet::FacetHeader;
use crate::social::text::{CharacterRef, read_text, write_text};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// `GroupsComponentServerFacet_RequestGroupInvite`.
pub const GROUP_INVITE_TYPE_INDEX: u64 = 3492;
/// `GroupsComponentServerFacet_RequestReplyToGroupInvite`.
pub const REPLY_TO_INVITE_TYPE_INDEX: u64 = 3495;
/// `GroupsComponentServerFacet_RequestLeaveGroup`.
pub const LEAVE_GROUP_TYPE_INDEX: u64 = 2252;
/// `GroupsComponentServerFacet_RequestSetGroupLeader`.
pub const SET_GROUP_LEADER_TYPE_INDEX: u64 = 3502;
/// `GroupsComponentServerFacet_RequestInitiateKickVotePlayer`.
pub const KICK_VOTE_TYPE_INDEX: u64 = 3498;
/// `GroupsComponentClientFacet_OnReceivedGroupMemberRemoved`.
pub const MEMBER_REMOVED_TYPE_INDEX: u64 = 5283;
/// `GroupsComponentClientFacet_OnReceivedGroupInviteRemoved`.
pub const INVITE_REMOVED_TYPE_INDEX: u64 = 1677;
/// `GroupsComponentClientFacet_OnReceivedErrorMessage`.
pub const GROUP_ERROR_TYPE_INDEX: u64 = 3453;
/// `GroupsComponentServerFacet_RequestPing`.
pub const GROUP_PING_TYPE_INDEX: u64 = 2407;
/// `GroupsComponentClientFacet_OnReceivedPreRaidStatus`.
pub const PRE_RAID_STATUS_TYPE_INDEX: u64 = 715;
/// `GroupsComponentClientFacet_OnReceivedSiegeWarfareStarted`.
pub const SIEGE_WARFARE_STARTED_TYPE_INDEX: u64 = 3454;
/// `GroupsComponentClientFacet_OnReceivedSiegeWarfareEnded`.
pub const SIEGE_WARFARE_ENDED_TYPE_INDEX: u64 = 245;
/// `GroupsComponentClientFacet_OnSiegeWarfareCompleted`.
pub const SIEGE_WARFARE_COMPLETED_TYPE_INDEX: u64 = 3455;
/// `GroupsComponentServerFacet_RequestMoveRaidMember`.
pub const MOVE_RAID_MEMBER_TYPE_INDEX: u64 = 3501;
/// `GroupsComponentServerFacet_RequestSwapRaidMembers`.
pub const SWAP_RAID_MEMBERS_TYPE_INDEX: u64 = 541;
/// `GroupsComponentServerFacet_RequestChooseRole`.
pub const CHOOSE_ROLE_TYPE_INDEX: u64 = 5390;
/// `GroupsComponentServerFacet_RequestSetRaidMemberPermission`.
pub const SET_RAID_PERMISSION_TYPE_INDEX: u64 = 2422;
/// `GroupsComponentServerFacet_RequestInitiateConvertToRaid`.
pub const INITIATE_CONVERT_TO_RAID_TYPE_INDEX: u64 = 5317;
/// `GroupsComponentServerFacet_RequestConvertToRaidVote`.
pub const CONVERT_TO_RAID_VOTE_TYPE_INDEX: u64 = 5318;
/// `GroupsComponentClientFacet_OnInitiateConvertToRaidFailed`.
pub const INITIATE_CONVERT_TO_RAID_FAILED_TYPE_INDEX: u64 = 5308;
/// `GroupsComponentClientFacet_OnConvertToRaidVoteEnded`.
pub const CONVERT_TO_RAID_VOTE_ENDED_TYPE_INDEX: u64 = 5309;
/// `GroupsComponentClientFacet_OnSubmitConvertToRaidVoteFailed`.
pub const SUBMIT_CONVERT_TO_RAID_VOTE_FAILED_TYPE_INDEX: u64 = 5310;
/// `GroupsComponentClientFacet_OnConvertToRaidVoteInitiated`.
pub const CONVERT_TO_RAID_VOTE_INITIATED_TYPE_INDEX: u64 = 5311;

/// The client's `eConvertToRaidError` values: the byte of 5308, 5309 and 5310. Observed: the
/// client registers the six names in this order in `FUN_144f418b0` (one value helper per number,
/// shared by every enum there; the same order gives `eRaidType` 1 to 4 and `ePreRaidStatus_WarEnded`
/// 4, both seen in the ledgers). Lua `socialscreen.luac` shows `VoteInProgress` and `TooFrequent`
/// for a failed start (5308), `AlreadyVoted` for a failed vote (5310) and `NotConverted` for an
/// ended vote (5309).
pub const CONVERT_TO_RAID_CONVERTED: u8 = 0;
/// See [`CONVERT_TO_RAID_CONVERTED`]: the group was not converted.
pub const CONVERT_TO_RAID_NOT_CONVERTED: u8 = 1;
/// See [`CONVERT_TO_RAID_CONVERTED`]: a vote of the group is open.
pub const CONVERT_TO_RAID_VOTE_IN_PROGRESS: u8 = 2;
/// See [`CONVERT_TO_RAID_CONVERTED`]: the group has no open vote.
pub const CONVERT_TO_RAID_VOTE_NOT_IN_PROGRESS: u8 = 3;
/// See [`CONVERT_TO_RAID_CONVERTED`]: the last start of the group was too recent.
pub const CONVERT_TO_RAID_TOO_FREQUENT: u8 = 4;
/// See [`CONVERT_TO_RAID_CONVERTED`]: the player voted already.
pub const CONVERT_TO_RAID_ALREADY_VOTED: u8 = 5;

/// The client's `eRaidPermission` values (Observed: names in this order in `FUN_144f418b0`;
/// `playerflyouthandler.luac` compares them as numbers, `Normal` < `Assistant` < `Leader`, and a
/// promote adds 1). The byte of 2422: the retail call (`Hive gorgon raid third boss` 230.534 s)
/// sent [`RAID_PERMISSION_LEADER`].
pub const RAID_PERMISSION_NORMAL: u8 = 0;
/// See [`RAID_PERMISSION_NORMAL`].
pub const RAID_PERMISSION_ASSISTANT: u8 = 1;
/// See [`RAID_PERMISSION_NORMAL`].
pub const RAID_PERMISSION_LEADER: u8 = 2;

/// The client's `ePreRaidStatus` values: the status of 715 (Observed: names in this order in
/// `FUN_144f418b0`; `army-management` 24.033 s to 68.215 s sent 0, 2, 3, 1, 0, 1 as the war
/// roster changed, every war and invasion ledger 4 at the war's end, 7 ms before the 245).
/// `notifications.luac` shows the war selected text for 0, unselected for 1 and the permission
/// change for 2 and 3.
pub const PRE_RAID_SELECTED: u32 = 0;
/// See [`PRE_RAID_SELECTED`]: the player left the war roster.
pub const PRE_RAID_UNSELECTED: u32 = 1;
/// See [`PRE_RAID_SELECTED`]: the player's roster permission rose.
pub const PRE_RAID_PERMISSION_PROMOTED: u32 = 2;
/// See [`PRE_RAID_SELECTED`]: the player's roster permission fell.
pub const PRE_RAID_PERMISSION_DEMOTED: u32 = 3;
/// See [`PRE_RAID_SELECTED`]: the war ended.
pub const PRE_RAID_WAR_ENDED: u32 = 4;

/// Bytes after the header of every retail 2407 (all zero in the war, raid and healer
/// ledgers; meaning Unknown).
pub const PING_DATA_LEN: usize = 14;

/// The group calls of the client.
pub(crate) const CALL_TYPES: [u64; 12] = [
    GROUP_INVITE_TYPE_INDEX,
    REPLY_TO_INVITE_TYPE_INDEX,
    LEAVE_GROUP_TYPE_INDEX,
    SET_GROUP_LEADER_TYPE_INDEX,
    KICK_VOTE_TYPE_INDEX,
    GROUP_PING_TYPE_INDEX,
    MOVE_RAID_MEMBER_TYPE_INDEX,
    SWAP_RAID_MEMBERS_TYPE_INDEX,
    CHOOSE_ROLE_TYPE_INDEX,
    SET_RAID_PERMISSION_TYPE_INDEX,
    INITIATE_CONVERT_TO_RAID_TYPE_INDEX,
    CONVERT_TO_RAID_VOTE_TYPE_INDEX,
];

/// The answer of an accepted invite (27 of 27 retail answers; the client sends no other in the
/// captures).
pub const REPLY_ACCEPT: u32 = 0;
/// The 2252 value of every retail leave (15 of 15).
pub const LEAVE_VALUE: u32 = 0;
/// The 5283 reason of a member who left (every retail 5283 after a 2252).
pub const REMOVED_LEFT: u32 = 0;
/// The 1677 `kind` of an invite that ended by an answer (46 of 47 retail).
pub const INVITE_ENDED_ANSWERED: u32 = 0;
/// The 3453 error of an invite of an offline player (`3v3_arena_group`: the invitee's online
/// notice came 18 s after the error, and the next invite went through).
pub const ERROR_INVITEE_OFFLINE: u32 = 6;
/// The bytes after the invitee of every retail 3492 (6 of 6): 17 zero bytes.
const INVITE_EXTRA_LEN: usize = RAW_ID_LEN;

/// A group call of the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupCall {
    /// 3492: invite a player.
    Invite {
        /// The invitee's character id as UUID text.
        character: String,
        /// The raid group the invitee fills first: zero in every retail invite (all plain
        /// group invites). Inferred: a raid member's invite carries the group id here, as the
        /// client's `RequestRaidGroupInvite(player, raidInviteGroupId)` of `socialmenu.luac`
        /// passes it and no other call names a raid group ("Invited Players will fill Group
        /// {groupIndex} first").
        extra: [u8; INVITE_EXTRA_LEN],
        /// The last byte (0 in every retail invite; meaning Unknown).
        flag: u8,
    },
    /// 3495: answer an invite.
    Reply {
        /// The invite id.
        invite: [u8; RAW_ID_LEN],
        /// [`REPLY_ACCEPT`], or another value to decline.
        answer: u32,
    },
    /// 2252: leave the group.
    Leave {
        /// [`LEAVE_VALUE`].
        value: u32,
    },
    /// 3502: make a member the leader.
    SetLeader {
        /// The member's character id as UUID text.
        character: String,
    },
    /// 3498: start a vote to remove a member.
    KickVote {
        /// The member's character id as UUID text.
        character: String,
    },
    /// 2407: place a group ping. The bytes after the header are kept whole (all zero in
    /// retail; meaning Unknown).
    Ping {
        /// The ping bytes.
        data: [u8; PING_DATA_LEN],
    },
    /// 3501: move a raid member to another group of the raid (retail
    /// `raid_group_influence_race`: the member as UUID text, then the group index).
    MoveRaidMember {
        /// The member's character id as UUID text.
        character: String,
        /// The index of the group the member goes to in the raid's `groupIds`, from 0
        /// (Observed: 0 to 3 in `raid_group_influence_race` 43.292 to 65.476 s, each move
        /// followed by the member's groups state naming that group).
        group: u8,
    },
    /// 541: swap two raid members (two character ids as UUID text).
    SwapRaidMembers {
        /// The first member's character id as UUID text.
        first: String,
        /// The second member's character id as UUID text.
        second: String,
    },
    /// 5390: choose the player's role in its group (Observed: one byte after the header in all
    /// 7 retail calls, values 2, 3 and 4; `raid_group_influence_race` 71.624 s and 75.675 s: the
    /// group entity's role child of the player's member block took the value 48 to 68 ms
    /// later).
    ChooseRole {
        /// The role (the client's `GroupFinderRole`).
        role: u8,
    },
    /// 2422: set a raid member's permission (Observed: `Hive gorgon raid third boss` 230.534 s,
    /// the member as UUID text, then the permission byte).
    SetRaidPermission {
        /// The member's character id as UUID text.
        character: String,
        /// The permission ([`RAID_PERMISSION_NORMAL`] to [`RAID_PERMISSION_LEADER`]).
        permission: u8,
    },
    /// 5317: start the vote to turn the player's group into a raid (Observed: the client's
    /// `RequestInitiateConvertToRaid` `FUN_1434239e0` takes no argument and its reader
    /// `FUN_14259f180` reads the header alone).
    InitiateConvertToRaid,
    /// 5318: the player's vote (Observed: the reader `FUN_1425a30b0` reads one byte as a
    /// boolean; `socialscreen.luac` sends the result of the vote notification, whose choices are
    /// accept and deny).
    ConvertToRaidVote {
        /// Inferred: `true` accepts.
        accept: bool,
    },
}

impl GroupCall {
    pub(crate) fn read(
        type_index: u64,
        reader: &mut Reader<'_>,
    ) -> Option<Result<Self, MessageError>> {
        let call = match type_index {
            GROUP_INVITE_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::Invite {
                    character: read_text(reader)?,
                    extra: reader.raw_id()?,
                    flag: reader.u8()?,
                })
            })(),
            REPLY_TO_INVITE_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::Reply {
                    invite: reader.raw_id()?,
                    answer: reader.u32()?,
                })
            })(),
            LEAVE_GROUP_TYPE_INDEX => reader.u32().map(|value| Self::Leave { value }),
            SET_GROUP_LEADER_TYPE_INDEX => {
                read_text(reader).map(|character| Self::SetLeader { character })
            }
            KICK_VOTE_TYPE_INDEX => read_text(reader).map(|character| Self::KickVote { character }),
            GROUP_PING_TYPE_INDEX => reader.array().map(|data| Self::Ping { data }),
            MOVE_RAID_MEMBER_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::MoveRaidMember {
                    character: read_text(reader)?,
                    group: reader.u8()?,
                })
            })(),
            SWAP_RAID_MEMBERS_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::SwapRaidMembers {
                    first: read_text(reader)?,
                    second: read_text(reader)?,
                })
            })(),
            CHOOSE_ROLE_TYPE_INDEX => reader.u8().map(|role| Self::ChooseRole { role }),
            SET_RAID_PERMISSION_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::SetRaidPermission {
                    character: read_text(reader)?,
                    permission: reader.u8()?,
                })
            })(),
            INITIATE_CONVERT_TO_RAID_TYPE_INDEX => Ok(Self::InitiateConvertToRaid),
            CONVERT_TO_RAID_VOTE_TYPE_INDEX => reader
                .boolean()
                .map(|accept| Self::ConvertToRaidVote { accept }),
            _ => return None,
        };
        Some(call)
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::Invite { .. } => GROUP_INVITE_TYPE_INDEX,
            Self::Reply { .. } => REPLY_TO_INVITE_TYPE_INDEX,
            Self::Leave { .. } => LEAVE_GROUP_TYPE_INDEX,
            Self::SetLeader { .. } => SET_GROUP_LEADER_TYPE_INDEX,
            Self::KickVote { .. } => KICK_VOTE_TYPE_INDEX,
            Self::Ping { .. } => GROUP_PING_TYPE_INDEX,
            Self::MoveRaidMember { .. } => MOVE_RAID_MEMBER_TYPE_INDEX,
            Self::SwapRaidMembers { .. } => SWAP_RAID_MEMBERS_TYPE_INDEX,
            Self::ChooseRole { .. } => CHOOSE_ROLE_TYPE_INDEX,
            Self::SetRaidPermission { .. } => SET_RAID_PERMISSION_TYPE_INDEX,
            Self::InitiateConvertToRaid => INITIATE_CONVERT_TO_RAID_TYPE_INDEX,
            Self::ConvertToRaidVote { .. } => CONVERT_TO_RAID_VOTE_TYPE_INDEX,
        }
    }

    /// The body of the call to the groups facet `header`, as [`crate::social::SocialCall::decode`]
    /// reads it (a test client's call).
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text does not fit its prefix.
    pub fn encode(&self, header: FacetHeader) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        header.write(&mut writer);
        match self {
            Self::Invite {
                character,
                extra,
                flag,
            } => {
                write_text(&mut writer, character)?;
                writer.bytes(extra).u8(*flag);
            }
            Self::Reply { invite, answer } => {
                writer.bytes(invite).u32(*answer);
            }
            Self::Leave { value } => {
                writer.u32(*value);
            }
            Self::SetLeader { character } | Self::KickVote { character } => {
                write_text(&mut writer, character)?;
            }
            Self::Ping { data } => {
                writer.bytes(data);
            }
            Self::MoveRaidMember { character, group } => {
                write_text(&mut writer, character)?;
                writer.u8(*group);
            }
            Self::SwapRaidMembers { first, second } => {
                write_text(&mut writer, first)?;
                write_text(&mut writer, second)?;
            }
            Self::ChooseRole { role } => {
                writer.u8(*role);
            }
            Self::SetRaidPermission {
                character,
                permission,
            } => {
                write_text(&mut writer, character)?;
                writer.u8(*permission);
            }
            Self::InitiateConvertToRaid => {}
            Self::ConvertToRaidVote { accept } => {
                writer.boolean(*accept);
            }
        }
        Ok(writer.finish())
    }
}

/// A group push of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupPush {
    /// 5283: a member left the player's group (or the player left: one per other member).
    MemberRemoved {
        /// The member.
        character: CharacterRef,
        /// [`REMOVED_LEFT`].
        reason: u32,
    },
    /// 1677: an invite of the player ended.
    InviteRemoved {
        /// The invite id.
        invite: [u8; RAW_ID_LEN],
        /// The other side: the invitee for the inviter, the inviter for the invitee (none for a
        /// system invite).
        other: CharacterRef,
        /// How it ended ([`INVITE_ENDED_ANSWERED`]; 16 in the one other retail body).
        kind: u32,
        /// A second character (the invitee in the one retail body of kind 16; else none).
        second: CharacterRef,
        /// Whether the player sent the invite.
        outbound: bool,
        /// The last byte (0 in every retail body).
        flag: u8,
    },
    /// 3453: a group request failed.
    Error {
        /// The error ([`ERROR_INVITEE_OFFLINE`]).
        error: u32,
        /// The player it is about.
        character: CharacterRef,
    },
    /// 2407: a group ping of another member, sent on as it came.
    Ping {
        /// The ping bytes.
        data: [u8; PING_DATA_LEN],
    },
    /// 715: the pre-raid status (Observed in 13 ledgers: a `u32` status, then the war id, the
    /// same 16 bytes as the 3454 of the war and the activity of its raid invite; reader
    /// `FUN_14340fa40`).
    PreRaidStatus {
        /// The status ([`PRE_RAID_SELECTED`] to [`PRE_RAID_WAR_ENDED`]).
        status: u32,
        /// The war id.
        raid: [u8; RAW_ID_LEN],
    },
    /// 3454: the raid's war started.
    SiegeWarfareStarted {
        /// The raid id.
        raid: [u8; RAW_ID_LEN],
    },
    /// 245: the raid's war ended (a byte of 1 in retail, then a big-endian `u64`; meanings
    /// Unknown).
    SiegeWarfareEnded {
        /// The first byte.
        flag: u8,
        /// The value after it.
        value: u64,
    },
    /// 3455: the player's game mode ended for it (Observed: a big-endian `u32` of 0 as the
    /// player leaves an arena match, `3v3` 647.789 s, `20261010-125035-658` 516.049 s,
    /// `arena-4165c8761c29c499` 171.998 s, 3 of 3; meaning of the value Unknown).
    SiegeWarfareCompleted {
        /// The value.
        value: u32,
    },
    /// 5308: the start of a convert-to-raid vote failed (Observed: reader `FUN_143098a90`, one
    /// byte after the header).
    InitiateConvertToRaidFailed {
        /// [`CONVERT_TO_RAID_VOTE_IN_PROGRESS`] or [`CONVERT_TO_RAID_TOO_FREQUENT`].
        error: u8,
    },
    /// 5309: the convert-to-raid vote ended (Observed: one byte after the header, as 5308).
    ConvertToRaidVoteEnded {
        /// [`CONVERT_TO_RAID_CONVERTED`] or [`CONVERT_TO_RAID_NOT_CONVERTED`].
        result: u8,
    },
    /// 5310: the player's vote failed (Observed: one byte after the header, as 5308).
    SubmitConvertToRaidVoteFailed {
        /// [`CONVERT_TO_RAID_VOTE_NOT_IN_PROGRESS`] or [`CONVERT_TO_RAID_ALREADY_VOTED`].
        error: u8,
    },
    /// 5311: a convert-to-raid vote started (Observed: reader `FUN_143403650`: a character
    /// reference (`FUN_14087ac50`), then a big-endian `u64` (`FUN_14087af30`);
    /// `socialscreen.luac` compares the reference with the player's own character id and shows
    /// the vote until the time).
    ConvertToRaidVoteInitiated {
        /// The player that started it.
        initiator: CharacterRef,
        /// When the vote ends. Inferred: Unix nanoseconds, the unit of the group invite times.
        ends_ns: u64,
    },
}

impl GroupPush {
    /// The type index.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::MemberRemoved { .. } => MEMBER_REMOVED_TYPE_INDEX,
            Self::InviteRemoved { .. } => INVITE_REMOVED_TYPE_INDEX,
            Self::Error { .. } => GROUP_ERROR_TYPE_INDEX,
            Self::Ping { .. } => GROUP_PING_TYPE_INDEX,
            Self::PreRaidStatus { .. } => PRE_RAID_STATUS_TYPE_INDEX,
            Self::SiegeWarfareStarted { .. } => SIEGE_WARFARE_STARTED_TYPE_INDEX,
            Self::SiegeWarfareEnded { .. } => SIEGE_WARFARE_ENDED_TYPE_INDEX,
            Self::SiegeWarfareCompleted { .. } => SIEGE_WARFARE_COMPLETED_TYPE_INDEX,
            Self::InitiateConvertToRaidFailed { .. } => INITIATE_CONVERT_TO_RAID_FAILED_TYPE_INDEX,
            Self::ConvertToRaidVoteEnded { .. } => CONVERT_TO_RAID_VOTE_ENDED_TYPE_INDEX,
            Self::SubmitConvertToRaidVoteFailed { .. } => {
                SUBMIT_CONVERT_TO_RAID_VOTE_FAILED_TYPE_INDEX
            }
            Self::ConvertToRaidVoteInitiated { .. } => CONVERT_TO_RAID_VOTE_INITIATED_TYPE_INDEX,
        }
    }

    pub(crate) fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        match self {
            Self::MemberRemoved { character, reason } => {
                character.write(writer);
                writer.u32(*reason);
            }
            Self::InviteRemoved {
                invite,
                other,
                kind,
                second,
                outbound,
                flag,
            } => {
                writer.bytes(invite);
                other.write(writer);
                writer.u32(*kind);
                second.write(writer);
                writer.boolean(*outbound).u8(*flag);
            }
            Self::Error { error, character } => {
                writer.u32(*error);
                character.write(writer);
            }
            Self::Ping { data } => {
                writer.bytes(data);
            }
            Self::PreRaidStatus { status, raid } => {
                writer.u32(*status).bytes(raid);
            }
            Self::SiegeWarfareStarted { raid } => {
                writer.bytes(raid);
            }
            Self::SiegeWarfareEnded { flag, value } => {
                writer.u8(*flag).u64(*value);
            }
            Self::SiegeWarfareCompleted { value } => {
                writer.u32(*value);
            }
            Self::InitiateConvertToRaidFailed { error: code }
            | Self::ConvertToRaidVoteEnded { result: code }
            | Self::SubmitConvertToRaidVoteFailed { error: code } => {
                writer.u8(*code);
            }
            Self::ConvertToRaidVoteInitiated { initiator, ends_ns } => {
                initiator.write(writer);
                writer.u64(*ends_ns);
            }
        }
        Ok(())
    }

    pub(crate) fn read(
        type_index: u64,
        reader: &mut Reader<'_>,
    ) -> Option<Result<Self, MessageError>> {
        let push = match type_index {
            MEMBER_REMOVED_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::MemberRemoved {
                    character: CharacterRef::read(reader)?,
                    reason: reader.u32()?,
                })
            })(),
            INVITE_REMOVED_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::InviteRemoved {
                    invite: reader.raw_id()?,
                    other: CharacterRef::read(reader)?,
                    kind: reader.u32()?,
                    second: CharacterRef::read(reader)?,
                    outbound: reader.boolean()?,
                    flag: reader.u8()?,
                })
            })(),
            GROUP_ERROR_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::Error {
                    error: reader.u32()?,
                    character: CharacterRef::read(reader)?,
                })
            })(),
            GROUP_PING_TYPE_INDEX => reader.array().map(|data| Self::Ping { data }),
            PRE_RAID_STATUS_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::PreRaidStatus {
                    status: reader.u32()?,
                    raid: reader.raw_id()?,
                })
            })(),
            SIEGE_WARFARE_STARTED_TYPE_INDEX => reader
                .raw_id()
                .map(|raid| Self::SiegeWarfareStarted { raid }),
            SIEGE_WARFARE_ENDED_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::SiegeWarfareEnded {
                    flag: reader.u8()?,
                    value: reader.u64()?,
                })
            })(),
            SIEGE_WARFARE_COMPLETED_TYPE_INDEX => reader
                .u32()
                .map(|value| Self::SiegeWarfareCompleted { value }),
            INITIATE_CONVERT_TO_RAID_FAILED_TYPE_INDEX => reader
                .u8()
                .map(|error| Self::InitiateConvertToRaidFailed { error }),
            CONVERT_TO_RAID_VOTE_ENDED_TYPE_INDEX => reader
                .u8()
                .map(|result| Self::ConvertToRaidVoteEnded { result }),
            SUBMIT_CONVERT_TO_RAID_VOTE_FAILED_TYPE_INDEX => reader
                .u8()
                .map(|error| Self::SubmitConvertToRaidVoteFailed { error }),
            CONVERT_TO_RAID_VOTE_INITIATED_TYPE_INDEX => (|| -> Result<Self, MessageError> {
                Ok(Self::ConvertToRaidVoteInitiated {
                    initiator: CharacterRef::read(reader)?,
                    ends_ns: reader.u64()?,
                })
            })(),
            _ => return None,
        };
        Some(push)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social::{SocialCall, SocialPush};

    fn text_bytes(text: &str) -> Vec<u8> {
        let mut writer = Writer::new();
        write_text(&mut writer, text).expect("short text");
        writer.finish()
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    const HEADER: &str = "11c62f11c397b2f4e34e14f344a974cc";
    const CHARACTER: &str = "00000000-0000-4000-8000-000000000003";
    const INVITE: &str = "63f5b7c17a794a4fab110a31c021e598";

    /// The call of the retail body of `type_index` with arguments `args`; its encode gives the
    /// same body back.
    fn call(type_index: u64, args: &[u8]) -> GroupCall {
        let body = [hex(HEADER), args.to_vec()].concat();
        match SocialCall::decode(type_index, &body).expect("a social call") {
            Ok((header, SocialCall::Group(call))) => {
                assert_eq!(call.type_index(), type_index);
                assert_eq!(call.encode(header).expect("encode"), body);
                call
            }
            other => panic!("a group call: {other:?}"),
        }
    }

    /// The retail forms (dungeon_dynasty_tank_, Hive 3v3 group), the id replaced.
    #[test]
    fn the_retail_calls_read() {
        let invite = [text_bytes(CHARACTER), vec![0; INVITE_EXTRA_LEN + 1]].concat();
        assert_eq!(invite.len() + 16, 70, "the retail body size");
        assert_eq!(
            call(GROUP_INVITE_TYPE_INDEX, &invite),
            GroupCall::Invite {
                character: CHARACTER.to_owned(),
                extra: [0; INVITE_EXTRA_LEN],
                flag: 0,
            }
        );
        assert_eq!(
            call(
                REPLY_TO_INVITE_TYPE_INDEX,
                &hex(&format!("{INVITE}00000000"))
            ),
            GroupCall::Reply {
                invite: hex(INVITE).try_into().expect("16 bytes"),
                answer: REPLY_ACCEPT,
            }
        );
        assert_eq!(
            call(LEAVE_GROUP_TYPE_INDEX, &hex("00000000")),
            GroupCall::Leave { value: LEAVE_VALUE }
        );
        assert_eq!(
            call(SET_GROUP_LEADER_TYPE_INDEX, &text_bytes(CHARACTER)),
            GroupCall::SetLeader {
                character: CHARACTER.to_owned()
            }
        );
    }

    /// Retail `raid_group_influence_race` and the war ledger: raid moves, a swap, a ping and
    /// the siege pushes (the ids replaced where a test needs a fixed one).
    #[test]
    fn the_retail_raid_calls_and_pushes_read() {
        let member = text_bytes("561138d6-1b4f-4d0a-a0f0-eb0d09e02782");
        assert_eq!(
            call(
                MOVE_RAID_MEMBER_TYPE_INDEX,
                &[member.clone(), vec![1]].concat()
            ),
            GroupCall::MoveRaidMember {
                character: "561138d6-1b4f-4d0a-a0f0-eb0d09e02782".to_owned(),
                group: 1,
            }
        );
        let swap = [
            text_bytes("68ae0505-1380-45c8-8f94-60ed13aa849c"),
            text_bytes("e584623a-9fb9-40ab-b3ae-7dae4e83430f"),
        ]
        .concat();
        assert!(matches!(
            call(SWAP_RAID_MEMBERS_TYPE_INDEX, &swap),
            GroupCall::SwapRaidMembers { .. }
        ));
        assert_eq!(
            call(MOVE_RAID_MEMBER_TYPE_INDEX, &[member, vec![0]].concat()),
            GroupCall::MoveRaidMember {
                character: "561138d6-1b4f-4d0a-a0f0-eb0d09e02782".to_owned(),
                group: 0,
            },
            "the first group is 0"
        );
        // Retail `raid_group_influence_race` 71.624 s: role 3, one byte after the header.
        assert_eq!(
            call(CHOOSE_ROLE_TYPE_INDEX, &[3]),
            GroupCall::ChooseRole { role: 3 }
        );
        assert_eq!(
            call(GROUP_PING_TYPE_INDEX, &[0; PING_DATA_LEN]),
            GroupCall::Ping {
                data: [0; PING_DATA_LEN]
            }
        );
        let raid = [9; RAW_ID_LEN];
        for (push, expected) in [
            (
                GroupPush::PreRaidStatus { status: 4, raid },
                format!("00000004{}", "09".repeat(RAW_ID_LEN)),
            ),
            (
                GroupPush::SiegeWarfareStarted { raid },
                "09".repeat(RAW_ID_LEN),
            ),
            (
                GroupPush::SiegeWarfareEnded {
                    flag: 1,
                    value: 0x18c0_66e7_6a1b_6afc,
                },
                "0118c066e76a1b6afc".to_owned(),
            ),
            (
                GroupPush::SiegeWarfareCompleted { value: 0 },
                "00000000".to_owned(),
            ),
        ] {
            let push = SocialPush::Group(push);
            let args = push.encode_args().expect("encode");
            assert_eq!(args, hex(&expected), "{push:?}");
            assert_eq!(
                SocialPush::decode_args(push.type_index(), &args),
                Some(Ok(push))
            );
        }
    }

    /// The raid permission call of `Hive gorgon raid third boss` 230.534 s (the id replaced) and
    /// the convert-to-raid forms of the client's readers (`FUN_14259f180`, `FUN_1425a30b0`,
    /// `FUN_143098a90`, `FUN_143403650`).
    #[test]
    fn the_raid_permission_and_convert_to_raid_forms_read() {
        let member = text_bytes(CHARACTER);
        assert_eq!(
            call(
                SET_RAID_PERMISSION_TYPE_INDEX,
                &[member, vec![RAID_PERMISSION_LEADER]].concat()
            ),
            GroupCall::SetRaidPermission {
                character: CHARACTER.to_owned(),
                permission: RAID_PERMISSION_LEADER,
            }
        );
        assert_eq!(
            call(INITIATE_CONVERT_TO_RAID_TYPE_INDEX, &[]),
            GroupCall::InitiateConvertToRaid,
            "the header alone"
        );
        assert_eq!(
            call(CONVERT_TO_RAID_VOTE_TYPE_INDEX, &[1]),
            GroupCall::ConvertToRaidVote { accept: true }
        );
        let initiator = [0x33; RAW_ID_LEN];
        let ends_ns: u64 = 0x18c0_66e7_6a1b_6afc;
        for (push, expected) in [
            (
                GroupPush::InitiateConvertToRaidFailed {
                    error: CONVERT_TO_RAID_TOO_FREQUENT,
                },
                "04".to_owned(),
            ),
            (
                GroupPush::ConvertToRaidVoteEnded {
                    result: CONVERT_TO_RAID_NOT_CONVERTED,
                },
                "01".to_owned(),
            ),
            (
                GroupPush::SubmitConvertToRaidVoteFailed {
                    error: CONVERT_TO_RAID_ALREADY_VOTED,
                },
                "05".to_owned(),
            ),
            (
                GroupPush::ConvertToRaidVoteInitiated {
                    initiator: CharacterRef(Some(initiator)),
                    ends_ns,
                },
                format!("05{}18c066e76a1b6afc", "33".repeat(RAW_ID_LEN)),
            ),
            (
                GroupPush::PreRaidStatus {
                    status: PRE_RAID_PERMISSION_DEMOTED,
                    raid: initiator,
                },
                format!("00000003{}", "33".repeat(RAW_ID_LEN)),
            ),
        ] {
            let push = SocialPush::Group(push);
            let args = push.encode_args().expect("encode");
            assert_eq!(args, hex(&expected), "{push:?}");
            assert_eq!(
                SocialPush::decode_args(push.type_index(), &args),
                Some(Ok(push))
            );
        }
    }

    /// The retail bodies (Hive 3v3 group, 3v3_arena_group), the ids replaced.
    #[test]
    fn the_retail_pushes_have_the_retail_bytes() {
        let id = [0x22; RAW_ID_LEN];
        let member = format!("05{}", "22".repeat(RAW_ID_LEN));
        let cases = [
            (
                GroupPush::MemberRemoved {
                    character: CharacterRef(Some(id)),
                    reason: REMOVED_LEFT,
                },
                format!("{member}00000000"),
            ),
            (
                GroupPush::InviteRemoved {
                    invite: hex(INVITE).try_into().expect("16 bytes"),
                    other: CharacterRef(Some(id)),
                    kind: INVITE_ENDED_ANSWERED,
                    second: CharacterRef(None),
                    outbound: true,
                    flag: 0,
                },
                format!("{INVITE}{member}0000000000000100"),
            ),
            (
                GroupPush::InviteRemoved {
                    invite: hex(INVITE).try_into().expect("16 bytes"),
                    other: CharacterRef(None),
                    kind: INVITE_ENDED_ANSWERED,
                    second: CharacterRef(None),
                    outbound: false,
                    flag: 0,
                },
                format!("{INVITE}00000000000000000000"),
            ),
            (
                GroupPush::Error {
                    error: ERROR_INVITEE_OFFLINE,
                    character: CharacterRef(Some(id)),
                },
                format!("00000006{member}"),
            ),
        ];
        for (push, expected) in cases {
            let push = SocialPush::Group(push);
            let args = push.encode_args().expect("encode");
            assert_eq!(args, hex(&expected));
            assert_eq!(
                SocialPush::decode_args(push.type_index(), &args),
                Some(Ok(push))
            );
        }
    }

    /// A header of synthetic ids.
    const ROUND_TRIP_HEADER: FacetHeader = FacetHeader {
        target: 0x0102_0304_0506_0708,
        entity: 0x1112_1314_1516_1718,
    };

    #[test]
    fn every_call_encodes_and_reads_back() {
        let calls = [
            GroupCall::Invite {
                character: CHARACTER.to_owned(),
                extra: [7; INVITE_EXTRA_LEN],
                flag: 1,
            },
            GroupCall::Reply {
                invite: [8; RAW_ID_LEN],
                answer: 1,
            },
            GroupCall::Leave { value: 2 },
            GroupCall::SetLeader {
                character: CHARACTER.to_owned(),
            },
            GroupCall::KickVote {
                character: CHARACTER.to_owned(),
            },
            GroupCall::Ping {
                data: [5; PING_DATA_LEN],
            },
            GroupCall::MoveRaidMember {
                character: CHARACTER.to_owned(),
                group: 2,
            },
            GroupCall::SwapRaidMembers {
                first: CHARACTER.to_owned(),
                second: "00000000-0000-4000-8000-000000000009".to_owned(),
            },
            GroupCall::ChooseRole { role: 4 },
            GroupCall::SetRaidPermission {
                character: CHARACTER.to_owned(),
                permission: RAID_PERMISSION_ASSISTANT,
            },
            GroupCall::InitiateConvertToRaid,
            GroupCall::ConvertToRaidVote { accept: false },
        ]
        .map(SocialCall::Group);
        crate::social::assert_calls_round_trip(ROUND_TRIP_HEADER, &calls);
    }
}
