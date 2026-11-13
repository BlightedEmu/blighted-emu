// SPDX-License-Identifier: AGPL-3.0-or-later
//! The raid entity's state: `Javelin::RaidDataComponentReplicatedState` (type 28, component 1 of
//! the raid entity; its card on component 0 names the slice `slices/groups/raidgde`).
//!
//! Layout (client constructor table, `member_layouts`): slot 0 holds five children, `raidId`
//! (`u64`), `raidType` (`u8`), `numMembers` (`u8`), `lifetime` (`u32`) and `groupIds` (a
//! prefix-coded count, then that many 16-byte group ids); slot 1 has no child.
//!
//! Observed (group finder and raids study 2026-11-11, every class-28 body of the 68 retail
//! ledgers that hold one: 139 whole bodies, 484 changes):
//!
//! - A whole body sets all five children; every other body sets `numMembers` only (`01 04 nn`).
//! - The group ids never change after the whole body; a raid member's own groups state names the
//!   raid id and one of these group ids, and a raid move (3501) names a group by its index in
//!   this list, from 0 (`raid_group_influence_race` 43.292 to 52.418 s: indexes 1, 2, 3 and 0).
//! - The raid types by ledger kind: [`RAID_TYPE_WAR`] (9 war ledgers), [`RAID_TYPE_INVASION`]
//!   (3 invasion ledgers), [`RAID_TYPE_GAME_MODE`] (27 ledgers: expeditions, 3v3 arenas, soul
//!   trials, capture the flag, Outpost Rush), [`RAID_TYPE_PLAYER`] (15 ledgers: the Gorgon raid,
//!   open-world raids, elite areas). The names are Inferred from the ledger kinds.
//! - Types 1 to 3 have [`EVENT_RAID_LIFETIME`] and [`EVENT_RAID_GROUPS`] groups (10, the client's
//!   `Hud.LocalPlayer.Raid.MaxGroups` of `FUN_14344f6c0`); type 4 has [`PLAYER_RAID_LIFETIME`]
//!   and [`PLAYER_RAID_GROUPS`] groups, with at most 20 members (`WURM`, `Playthrough_Part_18`).
//!
//! - The raid entity's card (component [`RAID_CARD_COMPONENT`]) has asset [`RAID_ENTITY_ASSET`]
//!   and the raid entity's id as its `gde_ref`; a whole body comes first, the card after it, in
//!   one group (`arvaysa_ES_attacker_war_45vs45` 27.039 s: raid 9428, entity `243c1c92...`). A
//!   war's data names the same entity id in its side's `raid` and the id's facet entity in the
//!   side's `key` (`arvaysa_ES_attacker_war_45vs45` 1.410 s: key `f665a76e921c3c24`).
//!
//! Unknown: what `lifetime` means beyond the two values.

use crate::gde::{AssetId, GdeMetadataReplicatedState};
use crate::payload::{BundleMember, MemberState};
use crate::slots::{children_mask, read_slot_0, write_known};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `Javelin::RaidDataComponentReplicatedState`.
pub const RAID_DATA_TYPE_INDEX: u64 = 28;
/// The component of the raid data on the raid entity (every retail raid entity).
pub const RAID_DATA_COMPONENT: u64 = 1;
/// `raidType` of a war raid.
pub const RAID_TYPE_WAR: u8 = 1;
/// `raidType` of an invasion raid.
pub const RAID_TYPE_INVASION: u8 = 2;
/// `raidType` of a game mode team (expedition, arena, soul trial, Outpost Rush).
pub const RAID_TYPE_GAME_MODE: u8 = 3;
/// `raidType` of a raid that players formed in the open world.
pub const RAID_TYPE_PLAYER: u8 = 4;
/// The groups of a war, invasion or game mode raid.
pub const EVENT_RAID_GROUPS: usize = 10;
/// The groups of a players' raid.
pub const PLAYER_RAID_GROUPS: usize = 4;
/// `lifetime` of a war, invasion or game mode raid.
pub const EVENT_RAID_LIFETIME: u32 = 1;
/// `lifetime` of a players' raid.
pub const PLAYER_RAID_LIFETIME: u32 = 0;

/// The component of the raid entity's spawn card.
pub const RAID_CARD_COMPONENT: u64 = 0;
/// The raid entity's asset (`slices/groups/raidgde`): the same in the raid cards of a raid
/// trial, an arena and a war (`arvaysa_gorgon_raid` 0.657 s, `20261010-125035-658` 300.038 s,
/// `everfall_war` 51.027 s).
pub const RAID_ENTITY_ASSET: AssetId = AssetId {
    id: [
        0x51, 0x78, 0xda, 0x08, 0xd2, 0xb6, 0x59, 0xee, 0xb2, 0x24, 0x27, 0x95, 0xe3, 0x97, 0x17,
        0x00,
    ],
    sub_id: 2,
};

/// Children of slot 0.
const SLOT_0_CHILDREN: u8 = 5;
const RAID_ID: u8 = 0;
const RAID_TYPE: u8 = 1;
const MEMBERS: u8 = 2;
const LIFETIME: u8 = 3;
const GROUP_IDS: u8 = 4;
const FIELD: &str = "RaidDataComponentReplicatedState";

/// `Javelin::RaidDataComponentReplicatedState`: each child is set when the body sends it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RaidDataState {
    /// `raidId`.
    pub raid_id: Option<u64>,
    /// `raidType` ([`RAID_TYPE_WAR`] to [`RAID_TYPE_PLAYER`]).
    pub raid_type: Option<u8>,
    /// `numMembers`: the players in the raid.
    pub members: Option<u8>,
    /// `lifetime` ([`EVENT_RAID_LIFETIME`] or [`PLAYER_RAID_LIFETIME`]).
    pub lifetime: Option<u32>,
    /// `groupIds`: the raid's groups in index order.
    pub group_ids: Option<Vec<[u8; RAW_ID_LEN]>>,
}

/// The `lifetime` of a raid of type `kind` (module doc).
pub const fn lifetime_of(kind: u8) -> u32 {
    if kind == RAID_TYPE_PLAYER {
        PLAYER_RAID_LIFETIME
    } else {
        EVENT_RAID_LIFETIME
    }
}

/// The group count of a raid of type `kind` (module doc).
pub const fn groups_of(kind: u8) -> usize {
    if kind == RAID_TYPE_PLAYER {
        PLAYER_RAID_GROUPS
    } else {
        EVENT_RAID_GROUPS
    }
}

impl RaidDataState {
    /// The whole state of raid `raid_id` of type `kind` with `members` players and the groups
    /// `group_ids` in index order: every child set, as the first body of a raid entity.
    pub fn whole(raid_id: u64, kind: u8, members: u8, group_ids: Vec<[u8; RAW_ID_LEN]>) -> Self {
        Self {
            raid_id: Some(raid_id),
            raid_type: Some(kind),
            members: Some(members),
            lifetime: Some(lifetime_of(kind)),
            group_ids: Some(group_ids),
        }
    }

    /// The change from `sent` (what the client has) to `self`: the member count alone when only
    /// it changed (Observed: every body but the whole one is `01 04 nn`), else the whole state;
    /// `None` when nothing changed.
    pub fn change_from(&self, sent: &Self) -> Option<Self> {
        if self == sent {
            return None;
        }
        let same_but_members = Self {
            members: sent.members,
            ..self.clone()
        } == *sent;
        Some(if same_but_members {
            Self {
                members: self.members,
                ..Self::default()
            }
        } else {
            self.clone()
        })
    }

    /// Reads the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets a slot other than 0, or holds
    /// more group ids than bytes.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, SLOT_0_CHILDREN, |reader, child| {
            match child {
                RAID_ID => state.raid_id = Some(reader.u64()?),
                RAID_TYPE => state.raid_type = Some(reader.u8()?),
                MEMBERS => state.members = Some(reader.u8()?),
                LIFETIME => state.lifetime = Some(reader.u32()?),
                GROUP_IDS => state.group_ids = Some(read_group_ids(reader)?),
                _ => {
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

    /// Writes the state.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] when the group count does not fit its prefix.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        // The group ids first, so the child writer below cannot fail.
        let group_ids = self
            .group_ids
            .as_ref()
            .map(|ids| write_group_ids(ids))
            .transpose()?;
        let children = children_mask(&[
            self.raid_id.is_some(),
            self.raid_type.is_some(),
            self.members.is_some(),
            self.lifetime.is_some(),
            self.group_ids.is_some(),
        ]);
        write_known(
            writer,
            children,
            SLOT_0_CHILDREN,
            |writer, child| match child {
                RAID_ID => {
                    self.raid_id.map(|id| writer.u64(id));
                }
                RAID_TYPE => {
                    self.raid_type.map(|kind| writer.u8(kind));
                }
                MEMBERS => {
                    self.members.map(|members| writer.u8(members));
                }
                LIFETIME => {
                    self.lifetime.map(|lifetime| writer.u32(lifetime));
                }
                GROUP_IDS => {
                    group_ids.as_ref().map(|bytes| writer.bytes(bytes));
                }
                // Slot 0 has no other child.
                _ => {}
            },
        );
        Ok(())
    }
}

/// The members of raid entity `entity` with state `state`: the raid data, then, with `carded`,
/// the spawn card whose entity reference is `entity` (retail order, module doc).
pub fn raid_entity_members(
    entity: [u8; RAW_ID_LEN],
    state: RaidDataState,
    carded: bool,
) -> Vec<BundleMember> {
    let mut members = vec![BundleMember {
        component: RAID_DATA_COMPONENT,
        state: MemberState::RaidData(state),
    }];
    if carded {
        members.push(BundleMember {
            component: RAID_CARD_COMPONENT,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(RAID_ENTITY_ASSET),
                gde_ref: Some(entity),
            }),
        });
    }
    members
}

/// The bytes of `ids` as [`read_group_ids`] reads them.
fn write_group_ids(ids: &[[u8; RAW_ID_LEN]]) -> Result<Vec<u8>, MessageError> {
    let mut writer = Writer::new();
    writer.prefixed(u64::try_from(ids.len()).unwrap_or(u64::MAX))?;
    for id in ids {
        writer.bytes(id);
    }
    Ok(writer.finish())
}

/// A prefix-coded count, then that many 16-byte group ids.
fn read_group_ids(reader: &mut Reader<'_>) -> Result<Vec<[u8; RAW_ID_LEN]>, MessageError> {
    let count = reader.prefixed()?;
    let limit = u64::try_from(reader.remaining() / RAW_ID_LEN).unwrap_or(u64::MAX);
    if count > limit {
        return Err(MessageError::CountTooLarge { count, limit });
    }
    (0..count).map(|_| reader.raw_id()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::MemberState;
    use crate::test_support::hex_bytes;

    fn group(byte: u8) -> String {
        format!("{byte:02x}").repeat(RAW_ID_LEN)
    }

    /// Reads `body` whole, checks it writes back to the same bytes, and that the member decoder
    /// gives the same state.
    fn round_trip(body: &str) -> RaidDataState {
        let bytes = hex_bytes(body);
        let mut reader = Reader::new(&bytes);
        let state = RaidDataState::read(&mut reader).expect("reads");
        reader.finish().expect("whole body");
        let mut writer = Writer::new();
        state.write(&mut writer).expect("writes");
        assert_eq!(writer.finish(), bytes, "{body}");
        assert_eq!(
            MemberState::decode(RAID_DATA_TYPE_INDEX, &bytes).expect("decodes"),
            MemberState::RaidData(state.clone())
        );
        state
    }

    /// Retail `arvaysa_gorgon_raid` 0.657 s: a players' raid of nine with its four groups (the
    /// group ids made up).
    #[test]
    fn a_retail_players_raid_reads_and_writes_back() {
        let body = format!(
            "011f00000000000016e504090000000004{}{}{}{}",
            group(1),
            group(2),
            group(3),
            group(4)
        );
        let state = round_trip(&body);
        assert_eq!(state.raid_id, Some(0x16e5));
        assert_eq!(state.raid_type, Some(RAID_TYPE_PLAYER));
        assert_eq!(state.members, Some(9));
        assert_eq!(state.lifetime, Some(PLAYER_RAID_LIFETIME));
        assert_eq!(
            state.group_ids.as_ref().map(Vec::len),
            Some(PLAYER_RAID_GROUPS)
        );
    }

    /// Retail `20261010-125035-658` 300.038 s: a 3v3 arena team of one with its ten groups (the
    /// group ids made up).
    #[test]
    fn a_retail_game_mode_raid_reads_and_writes_back() {
        let groups: String = (1..=10).map(group).collect();
        let state = round_trip(&format!("011f00000000000004330301000000010a{groups}"));
        assert_eq!(state.raid_type, Some(RAID_TYPE_GAME_MODE));
        assert_eq!(state.lifetime, Some(EVENT_RAID_LIFETIME));
        assert_eq!(state.members, Some(1));
        assert_eq!(
            state.group_ids.as_ref().map(Vec::len),
            Some(EVENT_RAID_GROUPS)
        );
    }

    /// Retail `arvaysa_gorgon_raid` 85.176 s and `3v3` 647.784 s: the member count alone.
    #[test]
    fn a_retail_member_count_change_reads_and_writes_back() {
        for (body, members) in [("01040a", 10), ("010400", 0)] {
            let state = round_trip(body);
            assert_eq!(
                state,
                RaidDataState {
                    members: Some(members),
                    ..RaidDataState::default()
                }
            );
        }
    }

    /// Retail `arvaysa_ES_attacker_war_45vs45` 27.039 s: the war raid's whole body (group ids
    /// made up) is what [`RaidDataState::whole`] writes for a war raid of two.
    #[test]
    fn a_whole_war_raid_writes_the_retail_form() {
        let ids: Vec<[u8; RAW_ID_LEN]> = (1..=10).map(|byte| [byte; RAW_ID_LEN]).collect();
        let groups: String = (1..=10).map(group).collect();
        let mut writer = Writer::new();
        RaidDataState::whole(0x24d4, RAID_TYPE_WAR, 2, ids)
            .write(&mut writer)
            .expect("writes");
        assert_eq!(
            hex::encode(writer.finish()),
            format!("011f00000000000024d40102000000010a{groups}")
        );
        assert_eq!(lifetime_of(RAID_TYPE_PLAYER), PLAYER_RAID_LIFETIME);
        assert_eq!(groups_of(RAID_TYPE_PLAYER), PLAYER_RAID_GROUPS);
        assert_eq!(groups_of(RAID_TYPE_GAME_MODE), EVENT_RAID_GROUPS);
    }

    #[test]
    fn a_member_count_change_goes_alone_and_any_other_change_whole() {
        let sent = RaidDataState::whole(7, RAID_TYPE_GAME_MODE, 1, vec![[1; RAW_ID_LEN]]);
        assert_eq!(sent.change_from(&sent), None);
        let more = RaidDataState {
            members: Some(2),
            ..sent.clone()
        };
        assert_eq!(
            more.change_from(&sent),
            Some(RaidDataState {
                members: Some(2),
                ..RaidDataState::default()
            })
        );
        let other = RaidDataState::whole(8, RAID_TYPE_GAME_MODE, 2, vec![[1; RAW_ID_LEN]]);
        assert_eq!(other.change_from(&sent), Some(other.clone()));
    }

    #[test]
    fn the_raid_entity_is_the_data_then_the_card() {
        let state = RaidDataState::whole(7, RAID_TYPE_WAR, 1, Vec::new());
        let members = raid_entity_members([9; RAW_ID_LEN], state.clone(), true);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].component, RAID_DATA_COMPONENT);
        assert_eq!(members[0].state, MemberState::RaidData(state.clone()));
        assert_eq!(
            members[1].state,
            MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(RAID_ENTITY_ASSET),
                gde_ref: Some([9; RAW_ID_LEN]),
            })
        );
        assert_eq!(raid_entity_members([9; RAW_ID_LEN], state, false).len(), 1);
    }

    #[test]
    fn a_group_count_beyond_the_body_and_slot_1_are_refused() {
        let mut reader = Reader::new(&[0x01, 0x10, 0x05, 0x00]);
        assert!(matches!(
            RaidDataState::read(&mut reader),
            Err(MessageError::CountTooLarge { .. })
        ));
        let mut reader = Reader::new(&[0x02]);
        assert!(RaidDataState::read(&mut reader).is_err());
    }
}
