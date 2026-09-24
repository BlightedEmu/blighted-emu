// SPDX-License-Identifier: AGPL-3.0-or-later
//! The replicated social states of the player (run 15 social2 study,
//! `local-data/scratch-work/run15/social2/REPORT.md`): `Javelin::GroupsComponentReplicatedState`
//! (type 1994, component 51), `MB::SocialReplicatedState` (type 4176, component 55),
//! `Javelin::GuildsComponentReplicatedState` (type 3147, component 56) and
//! `MB::ChatReplicatedState` (type 1566, component 57).
//!
//! The child order is the constructor's registration order (`FUN_143430280`, `FUN_143ed8270`,
//! `FUN_1434e0d70`, `FUN_143e72190`); the child types are the read functions at `+0x30` of each
//! child's vtable (`u8` `FUN_14279f1b0`, `u16` `FUN_142a43db0`, `u32` `FUN_1417b4590`, `u64`
//! `FUN_14279f420`, `f32` `FUN_142a43ed0`, `bool` `FUN_1417b4c20`, 16 bytes `FUN_1433f63b0`,
//! text `FUN_1417b46c0`, character reference `FUN_1434cb580`, list of texts `FUN_1417b4250`).
//! Every list goes out in its full form ([`FullList`]). Children whose element layout no capture
//! shows are not written; a body that sets one reads as [`MessageError::Unsized`], so a bundle
//! walk stops there.

use crate::buildables::GuildCrest;
use crate::items::FullList;
use crate::slots::{read_slot_byte, read_wide_children, write_known_children, write_wide_children};
use crate::social::text::{CharacterRef, read_text, write_text};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `Javelin::GroupsComponentReplicatedState`.
pub const GROUPS_STATE_TYPE_INDEX: u64 = 1994;
/// The player component of the groups state.
pub const GROUPS_STATE_COMPONENT: u64 = 51;
/// Type index of `Javelin::GuildsComponentReplicatedState`.
pub const GUILDS_STATE_TYPE_INDEX: u64 = 3147;
/// The player component of the company state.
pub const GUILDS_STATE_COMPONENT: u64 = 56;
/// Type index of `MB::ChatReplicatedState`.
pub const CHAT_STATE_TYPE_INDEX: u64 = 1566;
/// The player component of the chat state.
pub const CHAT_STATE_COMPONENT: u64 = 57;
/// `guildRank` of a player in no company (retail `leaving_a_company`: `ffff` with the zero id).
pub const NO_GUILD_RANK: u16 = u16::MAX;
/// The `GroupInviteData` byte after the raid id: 0 in every retail invite (48 of 48).
pub const GROUP_INVITE_KIND: u8 = 0;
/// The `GroupInviteData` byte after the 16-byte id near its end: 4 in every retail invite.
pub const GROUP_INVITE_SOURCE: u8 = 4;
/// The `GuildInviteData` `u32` after the inviter (1 in the retail invite of
/// `ranks-rights-compagny`; meaning Unknown).
pub const GUILD_INVITE_KIND: u32 = 1;

/// Slot-byte bit of slot 0.
const SLOT_0_BIT: u8 = 0b01;
/// Slot-byte bit of slot 1.
const SLOT_1_BIT: u8 = 0b10;
/// Slots of the four classes: slot 0 and one add-slot call (player-component note), the chat
/// state slot 0 only.
const TWO_SLOTS: u8 = 2;
/// Slots of the chat state.
const ONE_SLOT: u8 = 1;

/// Groups slot 0: `groupId`, `raidId`, `opposingRaidId`.
const GROUPS_SLOT_0_CHILDREN: u8 = 3;
const GROUP_ID: u8 = 0;
const RAID_ID: u8 = 1;
const OPPOSING_RAID_ID: u8 = 2;
/// Groups slot 1: `raidType` to `isGroupPristine`.
const GROUPS_SLOT_1_CHILDREN: u8 = 10;
const RAID_TYPE: u8 = 0;
const GROUP_FINDER_CREATOR: u8 = 2;
const CREATE_SOURCE: u8 = 3;
const INBOUND_INVITES: u8 = 5;
const OUTBOUND_INVITES: u8 = 6;
const NEXT_ABANDON_VOTE: u8 = 7;
const GAME_INVITE: u8 = 8;
const GROUP_PRISTINE: u8 = 9;

/// Social slot 0: `playerTitleId`, `pronounType`, `chattingStateMessageType`.
const SOCIAL_SLOT_0_CHILDREN: u8 = 3;
const TITLE: u8 = 0;
const PRONOUN: u8 = 1;
const CHATTING_STATE: u8 = 2;
/// Social slot 1: `warData` to `mostRecentJoinCharacterCall`.
const SOCIAL_SLOT_1_CHILDREN: u8 = 8;
const DAILY_WARS_AS_ATTACKER: u8 = 1;
const DAILY_WARS_AS_DEFENDER: u8 = 2;
const LAST_DAILY_RESET: u8 = 3;
const FRIENDS: u8 = 4;
const FRIEND_INVITES: u8 = 5;
const SOCIAL_BLOCKS: u8 = 6;
const LAST_JOIN_CALL: u8 = 7;

/// Company slot 0: `guildId`, `guildRank`.
const GUILDS_SLOT_0_CHILDREN: u8 = 2;
const GUILD_ID: u8 = 0;
const GUILD_RANK: u8 = 1;
/// Company slot 1: `usingGmCommands` to `numberOfOutstandingInvites`.
const GUILDS_SLOT_1_CHILDREN: u8 = 24;
const USING_GM_COMMANDS: u8 = 0;
const FORCED_RENAME: u8 = 1;
const MOTD: u8 = 2;
const MOTD_AUTHOR: u8 = 3;
const MOTD_TIME: u8 = 4;
const SIEGE_WINDOW: u8 = 5;
const LAST_SIEGE_WINDOW_SET: u8 = 6;
const TREASURY: u8 = 7;
const WITHDRAWAL_LIMIT: u8 = 8;
const LAST_GUILD_LEAVE: u8 = 9;
const WITHDRAWN_TODAY: u8 = 10;
const SEAT_OVERTHROWABLE: u8 = 11;
const SEAT_VACANT: u8 = 12;
const GOVERNOR_LEAVE: u8 = 13;
const MEMBER_IDS: u8 = 14;
const MEMBER_RANKS: u8 = 15;
const MEMBER_ONLINE: u8 = 16;
const MEMBER_LAST_ONLINE: u8 = 17;
const MEMBER_WAS_KICKED: u8 = 18;
const WAR_LOTTERY_DEADLINE: u8 = 21;
const GUILD_INVITES: u8 = 22;
const OUTSTANDING_INVITES: u8 = 23;

/// Chat slot 0: `chatMutes`.
const CHAT_SLOT_0_CHILDREN: u8 = 1;
const CHAT_MUTES: u8 = 0;

/// The optional-value flag of `GroupInviteData` (0 none, 1 a `u64` follows).
const OPTION_PRESENT: u8 = 1;

const GROUPS_FIELD: &str = "GroupsComponentReplicatedState";
const SOCIAL_FIELD: &str = "SocialReplicatedState";
const GUILDS_FIELD: &str = "GuildsComponentReplicatedState";
const CHAT_FIELD: &str = "ChatReplicatedState";

/// A child that no reader here sizes: the walk stops at it.
const fn unsized_child(field: &'static str, child: u8) -> MessageError {
    MessageError::Unsized { field, byte: child }
}

/// The bitmap of the present children; `present[n]` is child `n`.
fn mask(present: &[bool]) -> u32 {
    present
        .iter()
        .enumerate()
        .filter(|(_, present)| **present)
        .fold(0, |bits, (child, _)| bits | 1 << child)
}

/// Writes the slot byte of two slots and each slot whose bitmap is not 0.
fn write_two_slots(
    writer: &mut Writer,
    slot_0: (u32, u8),
    slot_1: (u32, u8),
    mut write_child: impl FnMut(&mut Writer, u8, u8) -> Result<(), MessageError>,
) -> Result<(), MessageError> {
    let slots =
        if slot_0.0 == 0 { 0 } else { SLOT_0_BIT } | if slot_1.0 == 0 { 0 } else { SLOT_1_BIT };
    writer.u8(slots);
    for (slot, (children, count)) in [slot_0, slot_1].into_iter().enumerate() {
        if children != 0 {
            // Two slots: the index fits a byte.
            let slot = slot as u8;
            write_wide_children(writer, children, count, |writer, child| {
                write_child(writer, slot, child)
            })?;
        }
    }
    Ok(())
}

/// Reads the slot byte of a two-slot class and each slot it sets.
fn read_two_slots(
    reader: &mut Reader<'_>,
    field: &'static str,
    counts: [u8; 2],
    mut read_child: impl FnMut(&mut Reader<'_>, u8, u8) -> Result<(), MessageError>,
) -> Result<(), MessageError> {
    let slots = read_slot_byte(reader, field, TWO_SLOTS)?;
    for (slot, count) in counts.into_iter().enumerate() {
        if slots & 1 << slot != 0 {
            // Two slots: the index fits a byte.
            let slot = slot as u8;
            read_wide_children(reader, field, count, |reader, child| {
                read_child(reader, slot, child)
            })?;
        }
    }
    Ok(())
}

fn read_texts_list(reader: &mut Reader<'_>) -> Result<FullList<String>, MessageError> {
    FullList::read(reader, read_text)
}

fn write_texts_list(writer: &mut Writer, list: &FullList<String>) -> Result<(), MessageError> {
    list.write(writer, |writer, text| write_text(writer, text))
}

/// `GroupInviteData` (`FUN_145ce7aa0`, `FUN_145b548b0`, `FUN_145b51bf0`): one entry of
/// `inboundGroupInvites` or `outboundGroupInvites`, after its 16-byte key. Retail (48 invites of
/// 9 sessions): the key equals the invite id, the inviter is the first reference, the invitee
/// the second, and the time is the invite's creation in Unix nanoseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupInviteData {
    /// The map key (the invite id on retail).
    pub key: [u8; RAW_ID_LEN],
    /// The invite id; the client answers it with 3495.
    pub invite: [u8; RAW_ID_LEN],
    /// The inviter's group (zero when it has none yet).
    pub group: [u8; RAW_ID_LEN],
    /// The raid id (0 outside raids).
    pub raid: u64,
    /// [`GROUP_INVITE_KIND`].
    pub kind: u8,
    /// The inviter.
    pub inviter: CharacterRef,
    /// The invitee.
    pub invitee: CharacterRef,
    /// Unix nanoseconds when the invite was made.
    pub created_ns: u64,
    /// The optional `u64` after it (absent in every retail invite; meaning Unknown).
    pub optional: Option<u64>,
    /// The 16 bytes near the end (zero in plain invites; meaning Unknown).
    pub activity: [u8; RAW_ID_LEN],
    /// [`GROUP_INVITE_SOURCE`].
    pub source: u8,
    /// The two booleans, the `u32` and the three booleans of the end (all 0 in plain invites but
    /// one boolean in one war invite; meaning Unknown).
    pub flags: [bool; 2],
    /// The `u32` of the end.
    pub value: u32,
    /// The last three booleans.
    pub tail: [bool; 3],
}

impl GroupInviteData {
    /// A plain invite of `inviter` to `invitee` into `group`, made at `created_ns`: the values
    /// of every retail plain invite.
    pub const fn plain(
        invite: [u8; RAW_ID_LEN],
        group: [u8; RAW_ID_LEN],
        inviter: CharacterRef,
        invitee: CharacterRef,
        created_ns: u64,
    ) -> Self {
        Self {
            key: invite,
            invite,
            group,
            raid: 0,
            kind: GROUP_INVITE_KIND,
            inviter,
            invitee,
            created_ns,
            optional: None,
            activity: [0; RAW_ID_LEN],
            source: GROUP_INVITE_SOURCE,
            flags: [false; 2],
            value: 0,
            tail: [false; 3],
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let key = reader.raw_id()?;
        let invite = reader.raw_id()?;
        let group = reader.raw_id()?;
        let raid = reader.u64()?;
        let kind = reader.u8()?;
        let inviter = CharacterRef::read(reader)?;
        let invitee = CharacterRef::read(reader)?;
        let created_ns = reader.u64()?;
        let optional = if reader.boolean()? {
            Some(reader.u64()?)
        } else {
            None
        };
        Ok(Self {
            key,
            invite,
            group,
            raid,
            kind,
            inviter,
            invitee,
            created_ns,
            optional,
            activity: reader.raw_id()?,
            source: reader.u8()?,
            flags: [reader.boolean()?, reader.boolean()?],
            value: reader.u32()?,
            tail: [reader.boolean()?, reader.boolean()?, reader.boolean()?],
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .bytes(&self.key)
            .bytes(&self.invite)
            .bytes(&self.group)
            .u64(self.raid)
            .u8(self.kind);
        self.inviter.write(writer);
        self.invitee.write(writer);
        writer.u64(self.created_ns);
        match self.optional {
            Some(value) => writer.u8(OPTION_PRESENT).u64(value),
            None => writer.boolean(false),
        };
        writer
            .bytes(&self.activity)
            .u8(self.source)
            .boolean(self.flags[0])
            .boolean(self.flags[1])
            .u32(self.value)
            .boolean(self.tail[0])
            .boolean(self.tail[1])
            .boolean(self.tail[2]);
    }
}

/// `gameInviteData` (vtable `0x1481dc1f8`): retail's login card sets it on every player (an id
/// new each session, the player itself, the session's start in Unix nanoseconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameInviteData {
    /// The id.
    pub id: [u8; RAW_ID_LEN],
    /// The player.
    pub character: CharacterRef,
    /// Unix nanoseconds.
    pub created_ns: u64,
}

/// `Javelin::GroupsComponentReplicatedState`. Children 1 (`groupFinderGroupId`) and 4
/// (`groupFinderApplications`) are not sized here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupsState {
    /// Slot 0 child 0 `groupId`: the group (all zero for none).
    pub group_id: Option<[u8; RAW_ID_LEN]>,
    /// Slot 0 child 1 `raidId`.
    pub raid_id: Option<u64>,
    /// Slot 0 child 2 `opposingRaidId`.
    pub opposing_raid_id: Option<u64>,
    /// Slot 1 child 0 `raidType`.
    pub raid_type: Option<u8>,
    /// Slot 1 child 2 `isGroupFinderGroupCreator`.
    pub group_finder_creator: Option<bool>,
    /// Slot 1 child 3 `createSource`.
    pub create_source: Option<u8>,
    /// Slot 1 child 5 `inboundGroupInvites`.
    pub inbound_invites: Option<FullList<GroupInviteData>>,
    /// Slot 1 child 6 `outboundGroupInvites`.
    pub outbound_invites: Option<FullList<GroupInviteData>>,
    /// Slot 1 child 7 `nextEligibleAbandonGameModeVoteTime`.
    pub next_abandon_vote_ns: Option<u64>,
    /// Slot 1 child 8 `gameInviteData`.
    pub game_invite: Option<GameInviteData>,
    /// Slot 1 child 9 `isGroupPristine`.
    pub group_pristine: Option<bool>,
}

impl GroupsState {
    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sets a child not sized here.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        let counts = [GROUPS_SLOT_0_CHILDREN, GROUPS_SLOT_1_CHILDREN];
        read_two_slots(reader, GROUPS_FIELD, counts, |reader, slot, child| {
            match (slot, child) {
                (0, GROUP_ID) => state.group_id = Some(reader.raw_id()?),
                (0, RAID_ID) => state.raid_id = Some(reader.u64()?),
                (0, OPPOSING_RAID_ID) => state.opposing_raid_id = Some(reader.u64()?),
                (_, RAID_TYPE) => state.raid_type = Some(reader.u8()?),
                (_, GROUP_FINDER_CREATOR) => state.group_finder_creator = Some(reader.boolean()?),
                (_, CREATE_SOURCE) => state.create_source = Some(reader.u8()?),
                (_, INBOUND_INVITES) => {
                    state.inbound_invites = Some(FullList::read(reader, GroupInviteData::read)?);
                }
                (_, OUTBOUND_INVITES) => {
                    state.outbound_invites = Some(FullList::read(reader, GroupInviteData::read)?);
                }
                (_, NEXT_ABANDON_VOTE) => state.next_abandon_vote_ns = Some(reader.u64()?),
                (_, GAME_INVITE) => {
                    state.game_invite = Some(GameInviteData {
                        id: reader.raw_id()?,
                        character: CharacterRef::read(reader)?,
                        created_ns: reader.u64()?,
                    });
                }
                (_, GROUP_PRISTINE) => state.group_pristine = Some(reader.boolean()?),
                (_, child) => return Err(unsized_child(GROUPS_FIELD, child)),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a list does not encode.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let slot_0 = mask(&[
            self.group_id.is_some(),
            self.raid_id.is_some(),
            self.opposing_raid_id.is_some(),
        ]);
        let slot_1 = mask(&[
            self.raid_type.is_some(),
            false,
            self.group_finder_creator.is_some(),
            self.create_source.is_some(),
            false,
            self.inbound_invites.is_some(),
            self.outbound_invites.is_some(),
            self.next_abandon_vote_ns.is_some(),
            self.game_invite.is_some(),
            self.group_pristine.is_some(),
        ]);
        let write_invites = |writer: &mut Writer, list: &Option<FullList<GroupInviteData>>| {
            list.as_ref().map_or(Ok(()), |list| {
                list.write(writer, |writer, invite| {
                    invite.write(writer);
                    Ok(())
                })
            })
        };
        write_two_slots(
            writer,
            (slot_0, GROUPS_SLOT_0_CHILDREN),
            (slot_1, GROUPS_SLOT_1_CHILDREN),
            |writer, slot, child| {
                match (slot, child) {
                    (0, GROUP_ID) => {
                        self.group_id.map(|id| writer.bytes(&id));
                    }
                    (0, RAID_ID) => {
                        self.raid_id.map(|id| writer.u64(id));
                    }
                    (0, _) => {
                        self.opposing_raid_id.map(|id| writer.u64(id));
                    }
                    (_, RAID_TYPE) => {
                        self.raid_type.map(|value| writer.u8(value));
                    }
                    (_, GROUP_FINDER_CREATOR) => {
                        self.group_finder_creator.map(|value| writer.boolean(value));
                    }
                    (_, CREATE_SOURCE) => {
                        self.create_source.map(|value| writer.u8(value));
                    }
                    (_, INBOUND_INVITES) => write_invites(writer, &self.inbound_invites)?,
                    (_, OUTBOUND_INVITES) => write_invites(writer, &self.outbound_invites)?,
                    (_, NEXT_ABANDON_VOTE) => {
                        self.next_abandon_vote_ns.map(|value| writer.u64(value));
                    }
                    (_, GAME_INVITE) => {
                        if let Some(invite) = self.game_invite {
                            writer.bytes(&invite.id);
                            invite.character.write(writer);
                            writer.u64(invite.created_ns);
                        }
                    }
                    (_, _) => {
                        self.group_pristine.map(|value| writer.boolean(value));
                    }
                }
                Ok(())
            },
        )
    }
}

/// `MB::SocialReplicatedState`. Slot 1 child 0 (`warData`) is not sized here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SocialState {
    /// Slot 0 child 0 `playerTitleId`: the shown title, the CRC-32 of its lowercase id; 0 none.
    pub title: Option<u32>,
    /// Slot 0 child 1 `pronounType`.
    pub pronoun: Option<u8>,
    /// Slot 0 child 2 `chattingStateMessageType`.
    pub chatting_state: Option<u32>,
    /// Slot 1 child 1 `dailyWarAsAttackerCount`.
    pub daily_wars_as_attacker: Option<u8>,
    /// Slot 1 child 2 `dailyWarAsDefenderCount`.
    pub daily_wars_as_defender: Option<u8>,
    /// Slot 1 child 3 `lastDailyResetTime`, Unix nanoseconds.
    pub last_daily_reset_ns: Option<u64>,
    /// Slot 1 child 4 `friends`: character ids as UUID text.
    pub friends: Option<FullList<String>>,
    /// Slot 1 child 5 `friendInvites`: character ids as UUID text.
    pub friend_invites: Option<FullList<String>>,
    /// Slot 1 child 6 `socialBlocks`: character ids as UUID text.
    pub social_blocks: Option<FullList<String>>,
    /// Slot 1 child 7 `mostRecentJoinCharacterCall`, Unix nanoseconds.
    pub last_join_call_ns: Option<u64>,
}

impl SocialState {
    /// The change that shows `title` (retail own change `01 01` and the title after a 2474).
    pub fn with_title(title: u32) -> Self {
        Self {
            title: Some(title),
            ..Self::default()
        }
    }

    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sets a child not sized here.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        let counts = [SOCIAL_SLOT_0_CHILDREN, SOCIAL_SLOT_1_CHILDREN];
        read_two_slots(reader, SOCIAL_FIELD, counts, |reader, slot, child| {
            match (slot, child) {
                (0, TITLE) => state.title = Some(reader.u32()?),
                (0, PRONOUN) => state.pronoun = Some(reader.u8()?),
                (0, CHATTING_STATE) => state.chatting_state = Some(reader.u32()?),
                (_, DAILY_WARS_AS_ATTACKER) => state.daily_wars_as_attacker = Some(reader.u8()?),
                (_, DAILY_WARS_AS_DEFENDER) => state.daily_wars_as_defender = Some(reader.u8()?),
                (_, LAST_DAILY_RESET) => state.last_daily_reset_ns = Some(reader.u64()?),
                (_, FRIENDS) => state.friends = Some(read_texts_list(reader)?),
                (_, FRIEND_INVITES) => state.friend_invites = Some(read_texts_list(reader)?),
                (_, SOCIAL_BLOCKS) => state.social_blocks = Some(read_texts_list(reader)?),
                (_, LAST_JOIN_CALL) => state.last_join_call_ns = Some(reader.u64()?),
                (_, child) => return Err(unsized_child(SOCIAL_FIELD, child)),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text is too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let slot_0 = mask(&[
            self.title.is_some(),
            self.pronoun.is_some(),
            self.chatting_state.is_some(),
        ]);
        let slot_1 = mask(&[
            false,
            self.daily_wars_as_attacker.is_some(),
            self.daily_wars_as_defender.is_some(),
            self.last_daily_reset_ns.is_some(),
            self.friends.is_some(),
            self.friend_invites.is_some(),
            self.social_blocks.is_some(),
            self.last_join_call_ns.is_some(),
        ]);
        let write_list = |writer: &mut Writer, list: &Option<FullList<String>>| {
            list.as_ref()
                .map_or(Ok(()), |list| write_texts_list(writer, list))
        };
        write_two_slots(
            writer,
            (slot_0, SOCIAL_SLOT_0_CHILDREN),
            (slot_1, SOCIAL_SLOT_1_CHILDREN),
            |writer, slot, child| {
                match (slot, child) {
                    (0, TITLE) => {
                        self.title.map(|title| writer.u32(title));
                    }
                    (0, PRONOUN) => {
                        self.pronoun.map(|pronoun| writer.u8(pronoun));
                    }
                    (0, CHATTING_STATE) => {
                        self.chatting_state.map(|state| writer.u32(state));
                    }
                    (_, DAILY_WARS_AS_ATTACKER) => {
                        self.daily_wars_as_attacker.map(|count| writer.u8(count));
                    }
                    (_, DAILY_WARS_AS_DEFENDER) => {
                        self.daily_wars_as_defender.map(|count| writer.u8(count));
                    }
                    (_, LAST_DAILY_RESET) => {
                        self.last_daily_reset_ns.map(|time| writer.u64(time));
                    }
                    (_, FRIENDS) => write_list(writer, &self.friends)?,
                    (_, FRIEND_INVITES) => write_list(writer, &self.friend_invites)?,
                    (_, SOCIAL_BLOCKS) => write_list(writer, &self.social_blocks)?,
                    (_, _) => {
                        self.last_join_call_ns.map(|time| writer.u64(time));
                    }
                }
                Ok(())
            },
        )
    }
}

/// `GuildInviteData`: one entry of `guildInvites`, a company invite the player got (retail
/// `ranks-rights-compagny`: the list came 3 s before the player's answer 118).
#[derive(Debug, Clone, PartialEq)]
pub struct GuildInviteData {
    /// The company.
    pub guild: [u8; RAW_ID_LEN],
    /// Its name.
    pub name: String,
    /// Its crest.
    pub crest: GuildCrest,
    /// The inviter's character id as UUID text.
    pub inviter: String,
    /// [`GUILD_INVITE_KIND`].
    pub kind: u32,
    /// The inviter's platform id text (retail: a 17-digit Steam id; empty on this server).
    pub inviter_platform: String,
    /// Unix nanoseconds when the invite was made.
    pub created_ns: u64,
}

impl GuildInviteData {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            guild: reader.raw_id()?,
            name: read_text(reader)?,
            crest: GuildCrest::read(reader)?,
            inviter: read_text(reader)?,
            kind: reader.u32()?,
            inviter_platform: read_text(reader)?,
            created_ns: reader.u64()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.bytes(&self.guild);
        write_text(writer, &self.name)?;
        self.crest.write(writer);
        write_text(writer, &self.inviter)?;
        writer.u32(self.kind);
        write_text(writer, &self.inviter_platform)?;
        writer.u64(self.created_ns);
        Ok(())
    }
}

/// `Javelin::GuildsComponentReplicatedState`: the own company. Slot 1 children 19
/// (`guildInfluenceData`) and 20 (`eligibleTerritoryWars`) are not sized here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuildsState {
    /// Slot 0 child 0 `guildId` (all zero for none).
    pub guild_id: Option<[u8; RAW_ID_LEN]>,
    /// Slot 0 child 1 `guildRank`: 0 governor to 3 settler, [`NO_GUILD_RANK`] for none.
    pub rank: Option<u16>,
    /// Slot 1 child 0 `usingGmCommands`.
    pub using_gm_commands: Option<bool>,
    /// Slot 1 child 1 `guildForcedRename`.
    pub forced_rename: Option<bool>,
    /// Slot 1 child 2 `guildMessageOfTheDay`.
    pub motd: Option<String>,
    /// Slot 1 child 3 `guildMessageOfTheDayAuthor`.
    pub motd_author: Option<CharacterRef>,
    /// Slot 1 child 4 `guildMessageOfTheDayTime`, Unix nanoseconds.
    pub motd_time_ns: Option<u64>,
    /// Slot 1 child 5 `guildSiegeWindow`: the hour the war window starts.
    pub siege_hour: Option<f32>,
    /// Slot 1 child 6 `guildLastSiegeWindowSet`, Unix nanoseconds.
    pub last_siege_window_set_ns: Option<u64>,
    /// Slot 1 child 7 `guildTreasuryCurrentFunds`, hundredths of a coin.
    pub treasury: Option<u64>,
    /// Slot 1 child 8 `guildTreasuryDailyWithdrawalLimit`, hundredths of a coin.
    pub withdrawal_limit: Option<u64>,
    /// Slot 1 child 9 `lastGuildLeaveTime`, Unix nanoseconds (0: never left).
    pub last_leave_ns: Option<u64>,
    /// Slot 1 child 10 `guildTreasuryWithdrawnToday`, hundredths of a coin.
    pub withdrawn_today: Option<u64>,
    /// Slot 1 child 11 `isGuildMasterSeatOverthrowable`.
    pub seat_overthrowable: Option<bool>,
    /// Slot 1 child 12 `isGuildMasterSeatVacant`.
    pub seat_vacant: Option<bool>,
    /// Slot 1 child 13 `governorLeaveGuildTime`, Unix nanoseconds.
    pub governor_leave_ns: Option<u64>,
    /// Slot 1 child 14 `guildMembersCharacterIdString`.
    pub member_ids: Option<FullList<String>>,
    /// Slot 1 child 15 `guildMembersRank`, in member order.
    pub member_ranks: Option<FullList<u16>>,
    /// Slot 1 child 16 `guildMembersOnlineStatus`, in member order (1 online).
    pub member_online: Option<FullList<u8>>,
    /// Slot 1 child 17 `guildMembersLastOnlineTime`, Unix nanoseconds, in member order (0 when
    /// online).
    pub member_last_online_ns: Option<FullList<u64>>,
    /// Slot 1 child 18 `guildMemberWasKicked`.
    pub was_kicked: Option<bool>,
    /// Slot 1 child 21 `guildWarLotteryDeadline`, Unix nanoseconds.
    pub war_lottery_deadline_ns: Option<u64>,
    /// Slot 1 child 22 `guildInvites`: the company invites the player got.
    pub invites: Option<FullList<GuildInviteData>>,
    /// Slot 1 child 23 `numberOfOutstandingInvites`: the invites its company has open.
    pub outstanding_invites: Option<u64>,
}

impl GuildsState {
    /// The first state of a character that never left a company: `lastGuildLeaveTime` 0
    /// (retail: both new characters, 90 of 482 first groups).
    pub fn new_character() -> Self {
        Self {
            last_leave_ns: Some(0),
            ..Self::default()
        }
    }

    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sets a child not sized here.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut s = Self::default();
        let counts = [GUILDS_SLOT_0_CHILDREN, GUILDS_SLOT_1_CHILDREN];
        read_two_slots(reader, GUILDS_FIELD, counts, |r, slot, child| {
            match (slot, child) {
                (0, GUILD_ID) => s.guild_id = Some(r.raw_id()?),
                (0, GUILD_RANK) => s.rank = Some(r.u16()?),
                (_, USING_GM_COMMANDS) => s.using_gm_commands = Some(r.boolean()?),
                (_, FORCED_RENAME) => s.forced_rename = Some(r.boolean()?),
                (_, MOTD) => s.motd = Some(read_text(r)?),
                (_, MOTD_AUTHOR) => s.motd_author = Some(CharacterRef::read(r)?),
                (_, MOTD_TIME) => s.motd_time_ns = Some(r.u64()?),
                (_, SIEGE_WINDOW) => s.siege_hour = Some(r.f32()?),
                (_, LAST_SIEGE_WINDOW_SET) => s.last_siege_window_set_ns = Some(r.u64()?),
                (_, TREASURY) => s.treasury = Some(r.u64()?),
                (_, WITHDRAWAL_LIMIT) => s.withdrawal_limit = Some(r.u64()?),
                (_, LAST_GUILD_LEAVE) => s.last_leave_ns = Some(r.u64()?),
                (_, WITHDRAWN_TODAY) => s.withdrawn_today = Some(r.u64()?),
                (_, SEAT_OVERTHROWABLE) => s.seat_overthrowable = Some(r.boolean()?),
                (_, SEAT_VACANT) => s.seat_vacant = Some(r.boolean()?),
                (_, GOVERNOR_LEAVE) => s.governor_leave_ns = Some(r.u64()?),
                (_, MEMBER_IDS) => s.member_ids = Some(read_texts_list(r)?),
                (_, MEMBER_RANKS) => {
                    s.member_ranks = Some(FullList::read(r, |reader| reader.u16())?);
                }
                (_, MEMBER_ONLINE) => {
                    s.member_online = Some(FullList::read(r, |reader| reader.u8())?);
                }
                (_, MEMBER_LAST_ONLINE) => {
                    s.member_last_online_ns = Some(FullList::read(r, |reader| reader.u64())?);
                }
                (_, MEMBER_WAS_KICKED) => s.was_kicked = Some(r.boolean()?),
                (_, WAR_LOTTERY_DEADLINE) => s.war_lottery_deadline_ns = Some(r.u64()?),
                (_, GUILD_INVITES) => s.invites = Some(FullList::read(r, GuildInviteData::read)?),
                (_, OUTSTANDING_INVITES) => s.outstanding_invites = Some(r.u64()?),
                (_, child) => return Err(unsized_child(GUILDS_FIELD, child)),
            }
            Ok(())
        })?;
        Ok(s)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text is too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let slot_0 = mask(&[self.guild_id.is_some(), self.rank.is_some()]);
        let slot_1 = mask(&[
            self.using_gm_commands.is_some(),
            self.forced_rename.is_some(),
            self.motd.is_some(),
            self.motd_author.is_some(),
            self.motd_time_ns.is_some(),
            self.siege_hour.is_some(),
            self.last_siege_window_set_ns.is_some(),
            self.treasury.is_some(),
            self.withdrawal_limit.is_some(),
            self.last_leave_ns.is_some(),
            self.withdrawn_today.is_some(),
            self.seat_overthrowable.is_some(),
            self.seat_vacant.is_some(),
            self.governor_leave_ns.is_some(),
            self.member_ids.is_some(),
            self.member_ranks.is_some(),
            self.member_online.is_some(),
            self.member_last_online_ns.is_some(),
            self.was_kicked.is_some(),
            false,
            false,
            self.war_lottery_deadline_ns.is_some(),
            self.invites.is_some(),
            self.outstanding_invites.is_some(),
        ]);
        let boolean = |w: &mut Writer, value: Option<bool>| {
            value.map(|value| w.boolean(value));
        };
        let time = |w: &mut Writer, value: Option<u64>| {
            value.map(|value| w.u64(value));
        };
        write_two_slots(
            writer,
            (slot_0, GUILDS_SLOT_0_CHILDREN),
            (slot_1, GUILDS_SLOT_1_CHILDREN),
            |w, slot, child| {
                match (slot, child) {
                    (0, GUILD_ID) => {
                        self.guild_id.map(|id| w.bytes(&id));
                    }
                    (0, GUILD_RANK) => {
                        self.rank.map(|rank| w.u16(rank));
                    }
                    (_, USING_GM_COMMANDS) => boolean(w, self.using_gm_commands),
                    (_, FORCED_RENAME) => boolean(w, self.forced_rename),
                    (_, MOTD) => {
                        if let Some(motd) = &self.motd {
                            write_text(w, motd)?;
                        }
                    }
                    (_, MOTD_AUTHOR) => {
                        if let Some(author) = &self.motd_author {
                            author.write(w);
                        }
                    }
                    (_, MOTD_TIME) => time(w, self.motd_time_ns),
                    (_, SIEGE_WINDOW) => {
                        self.siege_hour.map(|hour| w.f32(hour));
                    }
                    (_, LAST_SIEGE_WINDOW_SET) => time(w, self.last_siege_window_set_ns),
                    (_, TREASURY) => time(w, self.treasury),
                    (_, WITHDRAWAL_LIMIT) => time(w, self.withdrawal_limit),
                    (_, LAST_GUILD_LEAVE) => time(w, self.last_leave_ns),
                    (_, WITHDRAWN_TODAY) => time(w, self.withdrawn_today),
                    (_, SEAT_OVERTHROWABLE) => boolean(w, self.seat_overthrowable),
                    (_, SEAT_VACANT) => boolean(w, self.seat_vacant),
                    (_, GOVERNOR_LEAVE) => time(w, self.governor_leave_ns),
                    (_, MEMBER_IDS) => {
                        if let Some(list) = &self.member_ids {
                            write_texts_list(w, list)?;
                        }
                    }
                    (_, MEMBER_RANKS) => {
                        if let Some(list) = &self.member_ranks {
                            list.write(w, |w, rank| {
                                w.u16(*rank);
                                Ok(())
                            })?;
                        }
                    }
                    (_, MEMBER_ONLINE) => {
                        if let Some(list) = &self.member_online {
                            list.write(w, |w, online| {
                                w.u8(*online);
                                Ok(())
                            })?;
                        }
                    }
                    (_, MEMBER_LAST_ONLINE) => {
                        if let Some(list) = &self.member_last_online_ns {
                            list.write(w, |w, time| {
                                w.u64(*time);
                                Ok(())
                            })?;
                        }
                    }
                    (_, MEMBER_WAS_KICKED) => boolean(w, self.was_kicked),
                    (_, WAR_LOTTERY_DEADLINE) => time(w, self.war_lottery_deadline_ns),
                    (_, GUILD_INVITES) => {
                        if let Some(list) = &self.invites {
                            list.write(w, |w, invite| invite.write(w))?;
                        }
                    }
                    (_, _) => {
                        self.outstanding_invites.map(|count| w.u64(count));
                    }
                }
                Ok(())
            },
        )
    }
}

/// `MB::ChatReplicatedState`: the muted characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatState {
    /// Slot 0 child 0 `chatMutes`: character ids as UUID text.
    pub mutes: Option<FullList<String>>,
}

impl ChatState {
    /// The first state: an empty mute list of [`crate::items::FIRST_VERSION`] (retail: every
    /// new character).
    pub fn first() -> Self {
        Self {
            mutes: Some(FullList::first(Vec::new())),
        }
    }

    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or does not match.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        if read_slot_byte(reader, CHAT_FIELD, ONE_SLOT)? != 0 {
            read_wide_children(reader, CHAT_FIELD, CHAT_SLOT_0_CHILDREN, |reader, _| {
                state.mutes = Some(read_texts_list(reader)?);
                Ok(())
            })?;
        }
        Ok(state)
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a text is too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let Some(mutes) = &self.mutes else {
            writer.u8(0);
            return Ok(());
        };
        writer.u8(SLOT_0_BIT);
        let mut result = Ok(());
        write_known_children(
            writer,
            1 << CHAT_MUTES,
            CHAT_SLOT_0_CHILDREN,
            |writer, _| {
                result = write_texts_list(writer, mutes);
            },
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    fn groups_round(bytes: &[u8]) -> GroupsState {
        let mut reader = Reader::new(bytes);
        let state = GroupsState::read(&mut reader).expect("reads");
        reader.finish().expect("whole body");
        let mut writer = Writer::new();
        state.write(&mut writer).expect("writes");
        assert_eq!(writer.finish(), bytes, "writes back the same bytes");
        state
    }

    /// Retail own change after an accepted invite (HiveDynastiHealer): the group id, and the
    /// inbound invites emptied; ids replaced.
    const RETAIL_JOINED: &str = "0301a1a2a3a4a5a6a7a8a9aaabacadaeafb0200001830f00";
    /// Retail inbound invite (HiveDynastiHealer, 7 s before the 3495): ids replaced.
    const RETAIL_INBOUND: &str = concat!(
        "02200001830f01",
        "1111111111111111111111111111111111111111111111111111111111111111",
        "a1a2a3a4a5a6a7a8a9aaabacadaeafb0",
        "000000000000000000",
        "05c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1",
        "05d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1",
        "18b950df3943b904",
        "00",
        "00000000000000000000000000000000",
        "04000000000000000000",
    );
    /// Retail login card body of the groups state (newchar 2026-09-18): the game invite data;
    /// ids replaced.
    const RETAIL_LOGIN: &str = concat!(
        "028002",
        "e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1",
        "05d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1",
        "18e0ef3b3161024c",
    );

    #[test]
    fn retail_groups_bodies_read_and_write_back() {
        let joined = groups_round(&hex(RETAIL_JOINED));
        assert_eq!(joined.group_id.map(|id| id[0]), Some(0xa1));
        assert_eq!(
            joined.inbound_invites.map(|list| list.elements.len()),
            Some(0)
        );
        let inbound = groups_round(&hex(RETAIL_INBOUND));
        let invites = inbound.inbound_invites.expect("invites");
        let invite = &invites.elements[0];
        assert_eq!(invite.key, invite.invite);
        assert_eq!(invite.source, GROUP_INVITE_SOURCE);
        assert_eq!(
            *invite,
            GroupInviteData::plain(
                invite.invite,
                invite.group,
                invite.inviter,
                invite.invitee,
                invite.created_ns
            )
        );
        let login = groups_round(&hex(RETAIL_LOGIN));
        assert_eq!(
            login.game_invite.map(|invite| invite.character),
            Some(CharacterRef(Some([0xd1; RAW_ID_LEN])))
        );
    }

    #[test]
    fn a_war_invite_with_an_empty_inviter_and_an_activity_reads_back() {
        // RS_offensive_war: the inviter reference empty, an activity id; ids replaced.
        let body = concat!(
            "02200001a81301",
            "1212121212121212121212121212121212121212121212121212121212121212",
            "a1a2a3a4a5a6a7a8a9aaabacadaeafb0",
            "000000000000055a",
            "00",
            "0000",
            "05d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1",
            "18bb7ee9c5f32dc8",
            "00",
            "c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2",
            "04000000000000000000",
        );
        let state = groups_round(&hex(body));
        let invites = state.inbound_invites.expect("invites");
        let invite = &invites.elements[0];
        assert_eq!(invite.inviter, CharacterRef(None));
        assert_eq!(invite.raid, 0x055a);
    }

    #[test]
    fn a_group_finder_child_stops_the_walk() {
        assert!(matches!(
            GroupsState::read(&mut Reader::new(&[SLOT_1_BIT, 0b10])),
            Err(MessageError::Unsized { .. })
        ));
    }

    fn social_round(bytes: &[u8]) -> SocialState {
        let mut reader = Reader::new(bytes);
        let state = SocialState::read(&mut reader).expect("reads");
        reader.finish().expect("whole body");
        let mut writer = Writer::new();
        state.write(&mut writer).expect("writes");
        assert_eq!(writer.finish(), bytes, "writes back the same bytes");
        state
    }

    #[test]
    fn retail_social_bodies_read_and_write_back() {
        // A login card: no title, pronoun 1, the daily reset, one friend, no invites or blocks
        // (open-world session; the friend replaced).
        let card = concat!(
            "0303000000000178",
            "18d6999cf6ba70f4",
            "000181fd01",
            "2430303030303030302d303030302d343030302d383030302d303030303030303030303031",
            "000181fd00",
            "000181fd00",
        );
        let state = social_round(&hex(card));
        assert_eq!(state.title, Some(0));
        assert_eq!(state.pronoun, Some(1));
        assert_eq!(
            state.friends.expect("friends").elements,
            ["00000000-0000-4000-8000-000000000001"]
        );
        // The title change of title_choice (Hive_Social).
        let change = social_round(&hex("0101431df4ea"));
        assert_eq!(change, SocialState::with_title(0x431d_f4ea));
    }

    #[test]
    fn retail_company_states_read_and_write_back() {
        // A company created (creating_a_company): governor, siege hour 14, the founder alone;
        // ids replaced.
        let created = concat!(
            "0303",
            "a1a2a3a4a5a6a7a8a9aaabacadaeafb0",
            "0000",
            "ff",
            "0000",
            "00",
            "0000",
            "0000000000000000",
            "41600000",
            "18b790fe669320f4",
            "f3",
            "0000000000000000",
            "0000000000002710",
            "00",
            "00",
            "0000000000000000",
            "af",
            "00019913012430303030303030302d303030302d343030302d383030302d303030303030303030303031",
            "00019913010000",
            "000199130101",
            "00019913010000000000000000",
            "0001991300",
            "04",
            "0000000000000000",
        );
        let created_bytes = hex(created);
        let mut reader = Reader::new(&created_bytes);
        // The influence list (child 19) is empty: the rest reads up to it and stops.
        assert!(matches!(
            GuildsState::read(&mut reader),
            Err(MessageError::Unsized { .. })
        ));
        // A player who left (leaving_a_company): no company, rank none, the leave time.
        let left = concat!(
            "0303",
            "00000000000000000000000000000000",
            "ffff",
            "e0",
            "00000000",
            "18b9b713a15f3168",
            "8e",
            "0000000000000000",
            "18b9b713a15f2e48",
            "0000000000000000",
            "1f",
            "0001b81900",
            "0001b81900",
            "0001b81900",
            "0001b81900",
            "00",
        );
        let bytes = hex(left);
        let mut reader = Reader::new(&bytes);
        let state = GuildsState::read(&mut reader).expect("reads");
        reader.finish().expect("whole body");
        assert_eq!(state.rank, Some(NO_GUILD_RANK));
        assert_eq!(state.last_leave_ns, Some(0x18b9_b713_a15f_2e48));
        let mut writer = Writer::new();
        state.write(&mut writer).expect("writes");
        assert_eq!(writer.finish(), bytes);
    }

    #[test]
    fn a_retail_company_invite_reads_and_writes_back() {
        // ranks-rights-compagny: the invite list 3 s before the answer; ids, name, inviter and
        // platform id replaced, crest and kind retail.
        let body = concat!(
            "02808080020001ab2001",
            "a1a2a3a4a5a6a7a8a9aaabacadaeafb0",
            "07436f6d70616e79",
            "00f13edcdcde3e20a0a13f2dadae3f80000001043f6eeef03f6feff13f6dedef3f800000",
            "2430303030303030302d303030302d343030302d383030302d303030303030303030303032",
            "00000001",
            "00",
            "18c75c5c506d7890",
        );
        let bytes = hex(body);
        let mut reader = Reader::new(&bytes);
        let state = GuildsState::read(&mut reader).expect("reads");
        reader.finish().expect("whole body");
        let invite = &state.invites.as_ref().expect("invites").elements[0];
        assert_eq!(invite.name, "Company");
        assert_eq!(invite.kind, GUILD_INVITE_KIND);
        let mut writer = Writer::new();
        state.write(&mut writer).expect("writes");
        assert_eq!(writer.finish(), bytes);
    }

    #[test]
    fn retail_chat_mutes_read_and_write_back() {
        // Hive Chat mute and unmute: the mute list (one entry kept, replaced).
        let body = concat!(
            "01010001a91001",
            "2430303030303030302d303030302d343030302d383030302d303030303030303030303033",
        );
        let bytes = hex(body);
        let mut reader = Reader::new(&bytes);
        let state = ChatState::read(&mut reader).expect("reads");
        reader.finish().expect("whole body");
        assert_eq!(
            state.mutes.as_ref().map(|list| list.elements.len()),
            Some(1)
        );
        let mut writer = Writer::new();
        state.write(&mut writer).expect("writes");
        assert_eq!(writer.finish(), bytes);
        let mut writer = Writer::new();
        ChatState::default().write(&mut writer).expect("writes");
        assert_eq!(writer.finish(), [0]);
    }
}
