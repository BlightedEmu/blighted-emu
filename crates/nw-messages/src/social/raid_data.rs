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
//! Unknown: what `lifetime` means beyond the two values.

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

impl RaidDataState {
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
