// SPDX-License-Identifier: AGPL-3.0-or-later
//! The group entity (run 15 social2 study): `Javelin::GroupDataComponentReplicatedState` (type
//! 3451, component 1) and the group's spawn card (component 0, asset [`GROUP_ENTITY_ASSET`]).
//! Every member of a group has the group as one more entity of its own; the client's group
//! panel reads the members from it.
//!
//! Layout (constructor `FUN_1433d9c40`, child read functions at vtable `+0x30`; checked on
//! 9,033 to 20,537 retail bodies per session): slot 0 holds 49 children, slots 1 to 5 one member
//! each with 9 children.
//!
//! - Slot 0: the leader's two positions (0, 1); per member `k` (0 to 4) its role, matchmaking role,
//!   health and mana (one byte each, health and mana in percent) and flags (two bytes) as
//!   children `2 + 5k` to `6 + 5k`; the leader's ping (27); five raid positions (28 to 32, three
//!   half floats); five member cards (33 to 37: character reference, name, `PlayerIconData`);
//!   the group id (38, 16 bytes); the dungeon time (39, `u64`); eight one-byte indexes (40 to
//!   47: raid group, name, icon, colour, objective, dungeon state, game mode, difficulty) and the
//!   join-queue throttle (48, a boolean).
//! - Slots 1 to 5: the member's two positions (0, 1), game-mode index (2, optional byte),
//!   faction (3), level (4), waypoint (5), ping (6), ping target (7) and houses (8).
//!
//! The flag bits: `0x10` of the first byte is set on every connected member and cleared when it
//! disconnects; `0x08` while it is dead (set and cleared with its deaths in the raid sessions);
//! `0x01` on the member that leads (the inviter of `HiveDynastiHealer`, `OPR_TEST3` and
//! `opr_match_2`: Inferred). The other bits seen (`0x02`, `0x04`, `0x40` of the first byte,
//! `0x04` and `0x08` of the second) have no meaning here and are not sent.
//!
//! Positions, pings, waypoints and houses are not sent by this server and read as
//! [`MessageError::Unsized`].

use crate::appearance::PlayerIconData;
use crate::gde::{AssetId, GDE_ID_LEN, GdeMetadataReplicatedState};
use crate::payload::{BundleMember, MemberState};
use crate::slots::{read_children_64, read_slot_byte, write_children_64};
use crate::social::text::{CharacterRef, read_text, write_text};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `Javelin::GroupDataComponentReplicatedState`.
pub const GROUP_DATA_TYPE_INDEX: u64 = 3451;
/// The component of the group data on the group entity (every retail group entity).
pub const GROUP_DATA_COMPONENT: u64 = 1;
/// The component of the group entity's spawn card.
pub const GROUP_CARD_COMPONENT: u64 = 0;
/// The group entity's asset: the same in all 5,042 group cards of 19 retail sessions.
pub const GROUP_ENTITY_ASSET: AssetId = AssetId {
    id: [
        0xb4, 0x42, 0x44, 0x2b, 0x49, 0xee, 0x52, 0x4e, 0x8c, 0x5f, 0xcc, 0xe5, 0x55, 0x0e, 0xdd,
        0xad,
    ],
    sub_id: 2,
};
/// Members of a group: five member blocks in slot 0 and five member slots.
pub const GROUP_SIZE: usize = 5;
/// Flag bit (first byte): the member is connected.
pub const FLAG_CONNECTED: u8 = 0x10;
/// Flag bit (first byte): the member is dead.
pub const FLAG_DEAD: u8 = 0x08;
/// Flag bit (first byte): the member leads the group (Inferred, see the module text).
pub const FLAG_LEADER: u8 = 0x01;
/// The matchmaking role of every member block of a retail full card, the empty ones too.
pub const MATCHMAKING_ROLE_NONE: u8 = 1;
/// The role of a member that chose none (the inviter's block of the retail join card).
pub const ROLE_NONE: u8 = 0;
/// The icon and colour index of a group without one (retail join card: `ff` both).
pub const NO_GROUP_INDEX: u8 = u8::MAX;
/// Health or mana in full, in percent.
pub const FULL_PERCENT: u8 = 100;

/// Slots: slot 0 and one per member.
const SLOTS: u8 = 6;
/// Children of slot 0.
const SLOT_0_CHILDREN: u8 = 49;
/// Children of a member slot.
const MEMBER_SLOT_CHILDREN: u8 = 9;
/// The first member child of slot 0.
const FIRST_MEMBER_CHILD: u8 = 2;
/// Children per member block of slot 0.
const MEMBER_BLOCK: u8 = 5;
const ROLE: u8 = 0;
const MATCHMAKING_ROLE: u8 = 1;
const HEALTH: u8 = 2;
const MANA: u8 = 3;
const FLAGS: u8 = 4;
/// The first member card of slot 0.
const FIRST_MEMBER_CARD: u8 = 33;
const GROUP_ID: u8 = 38;
const DUNGEON_TIME: u8 = 39;
/// The first of the eight one-byte indexes.
const FIRST_INDEX: u8 = 40;
/// The one-byte indexes.
const INDEXES: usize = 8;
/// Index of the icon among the indexes.
const ICON_INDEX: usize = 2;
/// Index of the colour among the indexes.
const COLOR_INDEX: usize = 3;
const JOIN_QUEUE_THROTTLE: u8 = 48;
/// Member slot children.
const MEMBER_FACTION: u8 = 3;
const MEMBER_LEVEL: u8 = 4;
const FIELD: &str = "GroupDataComponentReplicatedState";

/// A member card (`groupMemberBaseData`, `FUN_145ce7c40`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupMemberCard {
    /// The member; none for an empty block.
    pub character: CharacterRef,
    /// Its name.
    pub name: String,
    /// Its icon.
    pub icon: PlayerIconData,
}

/// One member block of slot 0.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupMemberBlock {
    /// `groupMemberRole`.
    pub role: Option<u8>,
    /// `groupMemberMatchmakingRole`.
    pub matchmaking_role: Option<u8>,
    /// `groupMemberHealth`, percent.
    pub health: Option<u8>,
    /// `groupMemberMana`, percent.
    pub mana: Option<u8>,
    /// `groupMemberFlags`, as on the wire.
    pub flags: Option<[u8; 2]>,
    /// `groupMemberBaseData`.
    pub card: Option<GroupMemberCard>,
}

/// One member slot (slots 1 to 5): the children this server sends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GroupMemberPlace {
    /// `groupMemberFaction`.
    pub faction: Option<u8>,
    /// `groupMemberLevel`.
    pub level: Option<u8>,
}

/// `Javelin::GroupDataComponentReplicatedState`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupDataState {
    /// The member blocks of slot 0.
    pub members: [GroupMemberBlock; GROUP_SIZE],
    /// The member slots.
    pub places: [GroupMemberPlace; GROUP_SIZE],
    /// `groupId`.
    pub group_id: Option<[u8; RAW_ID_LEN]>,
    /// `groupDungeonTime`, Unix nanoseconds.
    pub dungeon_time_ns: Option<u64>,
    /// The eight one-byte indexes, children 40 to 47.
    pub indexes: [Option<u8>; INDEXES],
    /// `joinQueueThrottleActive`.
    pub join_queue_throttle: Option<bool>,
}

/// What the group entity shows of one member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMemberView {
    /// The character id.
    pub character: [u8; RAW_ID_LEN],
    /// Its name.
    pub name: String,
    /// Its icon.
    pub icon: PlayerIconData,
    /// Health in percent.
    pub health: u8,
    /// Mana in percent.
    pub mana: u8,
    /// Connected.
    pub connected: bool,
    /// Dead.
    pub dead: bool,
    /// Leads the group.
    pub leader: bool,
    /// The faction (0 none).
    pub faction: u8,
    /// The level as the client's player data shows it.
    pub level: u8,
}

impl GroupMemberView {
    fn flags(&self) -> [u8; 2] {
        let mut first = 0;
        for (set, bit) in [
            (self.connected, FLAG_CONNECTED),
            (self.dead, FLAG_DEAD),
            (self.leader, FLAG_LEADER),
        ] {
            if set {
                first |= bit;
            }
        }
        [first, 0]
    }
}

impl GroupDataState {
    /// The whole state of group `group` with `members` in join order (at most [`GROUP_SIZE`];
    /// more are left out): every block is set, an empty one with no member, no flags and the
    /// matchmaking role of retail's empty blocks, so a member who left is cleared.
    pub fn whole(group: [u8; RAW_ID_LEN], members: &[GroupMemberView]) -> Self {
        let mut state = Self {
            group_id: Some(group),
            ..Self::default()
        };
        state.indexes[ICON_INDEX] = Some(NO_GROUP_INDEX);
        state.indexes[COLOR_INDEX] = Some(NO_GROUP_INDEX);
        for (index, block) in state.members.iter_mut().enumerate() {
            *block = match members.get(index) {
                Some(member) => GroupMemberBlock {
                    role: Some(ROLE_NONE),
                    matchmaking_role: Some(MATCHMAKING_ROLE_NONE),
                    health: Some(member.health),
                    mana: Some(member.mana),
                    flags: Some(member.flags()),
                    card: Some(GroupMemberCard {
                        character: CharacterRef(Some(member.character)),
                        name: member.name.clone(),
                        icon: member.icon,
                    }),
                },
                None => GroupMemberBlock {
                    matchmaking_role: Some(MATCHMAKING_ROLE_NONE),
                    flags: Some([0; 2]),
                    card: Some(GroupMemberCard::default()),
                    ..GroupMemberBlock::default()
                },
            };
        }
        for (place, member) in state.places.iter_mut().zip(members) {
            *place = GroupMemberPlace {
                faction: Some(member.faction),
                level: Some(member.level),
            };
        }
        state
    }

    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sets a child not sized here.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let slots = read_slot_byte(reader, FIELD, SLOTS)?;
        let mut state = Self::default();
        if slots & 1 != 0 {
            read_children_64(reader, FIELD, SLOT_0_CHILDREN, |reader, child| {
                state.read_slot_0_child(reader, child)
            })?;
        }
        for (index, place) in state.places.iter_mut().enumerate() {
            if slots & 1 << (index + 1) == 0 {
                continue;
            }
            read_children_64(reader, FIELD, MEMBER_SLOT_CHILDREN, |reader, child| {
                match child {
                    MEMBER_FACTION => place.faction = Some(reader.u8()?),
                    MEMBER_LEVEL => place.level = Some(reader.u8()?),
                    _ => {
                        return Err(MessageError::Unsized {
                            field: FIELD,
                            byte: child,
                        });
                    }
                }
                Ok(())
            })?;
        }
        Ok(state)
    }

    fn read_slot_0_child(
        &mut self,
        reader: &mut Reader<'_>,
        child: u8,
    ) -> Result<(), MessageError> {
        let unsized_child = MessageError::Unsized {
            field: FIELD,
            byte: child,
        };
        let member_end = FIRST_MEMBER_CHILD + MEMBER_BLOCK * GROUP_SIZE as u8;
        if (FIRST_MEMBER_CHILD..member_end).contains(&child) {
            let offset = child - FIRST_MEMBER_CHILD;
            let block = &mut self.members[usize::from(offset / MEMBER_BLOCK)];
            match offset % MEMBER_BLOCK {
                ROLE => block.role = Some(reader.u8()?),
                MATCHMAKING_ROLE => block.matchmaking_role = Some(reader.u8()?),
                HEALTH => block.health = Some(reader.u8()?),
                MANA => block.mana = Some(reader.u8()?),
                FLAGS => block.flags = Some(reader.array()?),
                _ => return Err(unsized_child),
            }
            return Ok(());
        }
        let cards_end = FIRST_MEMBER_CARD + GROUP_SIZE as u8;
        if (FIRST_MEMBER_CARD..cards_end).contains(&child) {
            self.members[usize::from(child - FIRST_MEMBER_CARD)].card = Some(GroupMemberCard {
                character: CharacterRef::read(reader)?,
                name: read_text(reader)?,
                icon: PlayerIconData::read(reader)?,
            });
            return Ok(());
        }
        let indexes_end = FIRST_INDEX + INDEXES as u8;
        if (FIRST_INDEX..indexes_end).contains(&child) {
            self.indexes[usize::from(child - FIRST_INDEX)] = Some(reader.u8()?);
            return Ok(());
        }
        match child {
            GROUP_ID => self.group_id = Some(reader.raw_id()?),
            DUNGEON_TIME => self.dungeon_time_ns = Some(reader.u64()?),
            JOIN_QUEUE_THROTTLE => self.join_queue_throttle = Some(reader.boolean()?),
            _ => return Err(unsized_child),
        }
        Ok(())
    }

    fn slot_0_children(&self) -> u64 {
        let mut children = 0_u64;
        for (index, block) in self.members.iter().enumerate() {
            // Five blocks: the index fits a byte.
            let first = FIRST_MEMBER_CHILD + MEMBER_BLOCK * index as u8;
            for (offset, present) in [
                block.role.is_some(),
                block.matchmaking_role.is_some(),
                block.health.is_some(),
                block.mana.is_some(),
                block.flags.is_some(),
            ]
            .into_iter()
            .enumerate()
            {
                if present {
                    children |= 1 << (u64::from(first) + offset as u64);
                }
            }
            if block.card.is_some() {
                children |= 1 << (u64::from(FIRST_MEMBER_CARD) + index as u64);
            }
        }
        for (present, child) in [
            (self.group_id.is_some(), GROUP_ID),
            (self.dungeon_time_ns.is_some(), DUNGEON_TIME),
            (self.join_queue_throttle.is_some(), JOIN_QUEUE_THROTTLE),
        ] {
            if present {
                children |= 1 << child;
            }
        }
        for (index, value) in self.indexes.iter().enumerate() {
            if value.is_some() {
                children |= 1 << (u64::from(FIRST_INDEX) + index as u64);
            }
        }
        children
    }

    fn write_slot_0_child(&self, writer: &mut Writer, child: u8) -> Result<(), MessageError> {
        let member_end = FIRST_MEMBER_CHILD + MEMBER_BLOCK * GROUP_SIZE as u8;
        if (FIRST_MEMBER_CHILD..member_end).contains(&child) {
            let offset = child - FIRST_MEMBER_CHILD;
            let block = &self.members[usize::from(offset / MEMBER_BLOCK)];
            let byte = match offset % MEMBER_BLOCK {
                ROLE => block.role,
                MATCHMAKING_ROLE => block.matchmaking_role,
                HEALTH => block.health,
                MANA => block.mana,
                FLAGS => {
                    block.flags.map(|flags| writer.bytes(&flags));
                    return Ok(());
                }
                _ => {
                    return Err(MessageError::Unsized {
                        field: FIELD,
                        byte: child,
                    });
                }
            };
            byte.map(|byte| writer.u8(byte));
            return Ok(());
        }
        let cards_end = FIRST_MEMBER_CARD + GROUP_SIZE as u8;
        if (FIRST_MEMBER_CARD..cards_end).contains(&child) {
            if let Some(card) = &self.members[usize::from(child - FIRST_MEMBER_CARD)].card {
                card.character.write(writer);
                write_text(writer, &card.name)?;
                card.icon.write(writer);
            }
            return Ok(());
        }
        let indexes_end = FIRST_INDEX + INDEXES as u8;
        if (FIRST_INDEX..indexes_end).contains(&child) {
            self.indexes[usize::from(child - FIRST_INDEX)].map(|value| writer.u8(value));
            return Ok(());
        }
        match child {
            GROUP_ID => {
                self.group_id.map(|id| writer.bytes(&id));
            }
            DUNGEON_TIME => {
                self.dungeon_time_ns.map(|time| writer.u64(time));
            }
            _ => {
                self.join_queue_throttle.map(|value| writer.boolean(value));
            }
        }
        Ok(())
    }

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a name is too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let slot_0 = self.slot_0_children();
        let places: Vec<u64> = self
            .places
            .iter()
            .map(|place| {
                u64::from(place.faction.is_some()) << MEMBER_FACTION
                    | u64::from(place.level.is_some()) << MEMBER_LEVEL
            })
            .collect();
        let mut slots = u8::from(slot_0 != 0);
        for (index, children) in places.iter().enumerate() {
            if *children != 0 {
                slots |= 1 << (index + 1);
            }
        }
        writer.u8(slots);
        if slot_0 != 0 {
            write_children_64(writer, slot_0, SLOT_0_CHILDREN, |writer, child| {
                self.write_slot_0_child(writer, child)
            })?;
        }
        for (place, children) in self.places.iter().zip(places) {
            if children == 0 {
                continue;
            }
            write_children_64::<MessageError>(
                writer,
                children,
                MEMBER_SLOT_CHILDREN,
                |writer, child| {
                    let value = if child == MEMBER_FACTION {
                        place.faction
                    } else {
                        place.level
                    };
                    value.map(|value| writer.u8(value));
                    Ok(())
                },
            )?;
        }
        Ok(())
    }
}

/// The members of the group entity of group `group` with state `state`: the group data, then,
/// with `carded`, the spawn card whose entity reference is the group id (retail order: the data
/// first, the card after it).
pub fn group_entity_members(
    group: [u8; GDE_ID_LEN],
    state: GroupDataState,
    carded: bool,
) -> Vec<BundleMember> {
    let mut members = vec![BundleMember {
        component: GROUP_DATA_COMPONENT,
        state: MemberState::GroupData(Box::new(state)),
    }];
    if carded {
        members.push(BundleMember {
            component: GROUP_CARD_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(GROUP_ENTITY_ASSET),
                gde_ref: Some(group),
            }),
        });
    }
    members
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex;

    /// The retail card of a group of three after its third member joined (HiveDynastiHealer,
    /// slot 0 only; names and ids replaced, the third member's level and faction of its slot).
    const RETAIL_JOIN_CARD: &str = concat!(
        "01",
        "f8016464",
        "1104",
        "df02016464",
        "1000",
        "00",
        "97646410",
        "0001",
        "8401",
        "e0",
        "05c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1",
        "03416e6e",
        "011b0331000018",
        "0000",
        "05c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2",
        "03426f62",
        "001a0028180318",
        "0000",
        "89",
        "05c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3",
        "03436964",
        "00170306100110",
        "0000",
        "a1a2a3a4a5a6a7a8a9aaabacadaeafb0",
        "03",
        "ffff",
    );

    #[test]
    fn a_retail_group_card_reads_and_writes_back() {
        let bytes = hex(RETAIL_JOIN_CARD);
        let mut reader = Reader::new(&bytes);
        let state = GroupDataState::read(&mut reader).expect("reads");
        reader.finish().expect("whole body");
        assert_eq!(state.members[0].flags, Some([0x11, 0x04]));
        assert_eq!(state.members[1].role, Some(2));
        assert_eq!(state.members[2].health, Some(FULL_PERCENT));
        assert_eq!(
            state.members[2]
                .card
                .as_ref()
                .map(|card| card.name.as_str()),
            Some("Cid")
        );
        assert_eq!(state.indexes[ICON_INDEX], Some(NO_GROUP_INDEX));
        let mut writer = Writer::new();
        state.write(&mut writer).expect("writes");
        assert_eq!(writer.finish(), bytes);
    }

    #[test]
    fn retail_health_and_flag_changes_read_and_write_back() {
        // Raid session changes: member 2 health and mana, member 4 health and mana; member 3
        // health, member 3 flags (dead) and member 4 health.
        for body in ["01808083635218611c", "018080a0000918005d"] {
            let bytes = hex(body);
            let mut reader = Reader::new(&bytes);
            let state = GroupDataState::read(&mut reader).expect("reads");
            reader.finish().expect("whole body");
            let mut writer = Writer::new();
            state.write(&mut writer).expect("writes");
            assert_eq!(writer.finish(), bytes, "{body}");
        }
    }

    #[test]
    fn a_whole_state_sets_every_block_and_clears_the_empty_ones() {
        let member = |id: u8, leader: bool| GroupMemberView {
            character: [id; RAW_ID_LEN],
            name: format!("M{id}"),
            icon: PlayerIconData::default(),
            health: FULL_PERCENT,
            mana: FULL_PERCENT,
            connected: true,
            dead: false,
            leader,
            faction: 0,
            level: 5,
        };
        let group = [9; RAW_ID_LEN];
        let state = GroupDataState::whole(group, &[member(1, true), member(2, false)]);
        assert_eq!(
            state.members[0].flags,
            Some([FLAG_CONNECTED | FLAG_LEADER, 0])
        );
        assert_eq!(state.members[1].flags, Some([FLAG_CONNECTED, 0]));
        assert_eq!(state.members[2].flags, Some([0, 0]));
        assert_eq!(state.members[2].card, Some(GroupMemberCard::default()));
        assert_eq!(state.places[1].level, Some(5));
        assert_eq!(state.places[2], GroupMemberPlace::default());
        let mut writer = Writer::new();
        state.write(&mut writer).expect("writes");
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        assert_eq!(GroupDataState::read(&mut reader).expect("reads"), state);
        reader.finish().expect("whole body");
        let members = group_entity_members(group, state, true);
        assert_eq!(members.len(), 2);
        assert_eq!(members[1].component, GROUP_CARD_COMPONENT);
    }
}
