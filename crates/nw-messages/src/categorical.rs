// SPDX-License-Identifier: AGPL-3.0-or-later
//! `CategoricalProgressionReplicatedState` (type 911, player component 65): the ranks of the
//! weapon masteries, the trade skills and the other categorical progressions (progression note,
//! sections 1 and 2).
//!
//! Slot 0 holds three parallel lists: child 0 the progression ids (`u32`, see
//! [`progression_id`]), child 1 the ranks (`u16`, zero-based: rank `r` shows as `r + 1`) and
//! child 2 the points (`u64`, the experience inside the current rank). The login state sends all
//! three lists whole ([`CategoricalProgressionState::full`]). A later change names entries by
//! list position ([`CategoricalProgressionState::changes`]): retail sends only the children that
//! changed (child byte `04` for points, `06` for rank and points, `07` for a new entry), and a new
//! entry goes at index = the old length on all three children.

use nw_formats::objectstream::lowercase_crc32;

use crate::list_delta::{ReplicatedList, child_changes, full_child, write_child};
use crate::slots::{SLOT_0, children_mask, read_slot_0, write_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `CategoricalProgressionReplicatedState`.
pub const CATEGORICAL_PROGRESSION_TYPE_INDEX: u64 = 911;
/// The player component of the categorical progression.
pub const CATEGORICAL_PROGRESSION_COMPONENT: u64 = 65;

/// Children of slot 0; child 2 is the points.
const CHILDREN: u8 = 3;
const IDS_CHILD: u8 = 0;
const RANKS_CHILD: u8 = 1;
const FIELD: &str = "categorical progression";

/// The wire id of the categorical progression `name` (a `CategoricalProgressionId` of
/// `javelindata_categoricalprogression`): the CRC-32 (IEEE) of the lowercase name.
pub fn progression_id(name: &str) -> u32 {
    lowercase_crc32(name)
}

/// One entry of the three lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasteryEntry {
    /// The progression id ([`progression_id`]).
    pub progression_id: u32,
    /// The rank, zero-based.
    pub rank: u16,
    /// The experience inside the rank.
    pub points: u64,
}

/// The lists that a change of one entry sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasteryFields {
    /// Child 0: the progression id.
    pub progression_id: bool,
    /// Child 1: the rank.
    pub rank: bool,
    /// Child 2: the points.
    pub points: bool,
}

impl MasteryFields {
    /// Experience inside the rank only (retail child byte `04`).
    pub const POINTS: Self = Self {
        progression_id: false,
        rank: false,
        points: true,
    };
    /// A rank up with the experience left over (retail child byte `06`).
    pub const RANK_AND_POINTS: Self = Self {
        progression_id: false,
        rank: true,
        points: true,
    };
    /// A new entry (retail child byte `07`).
    pub const ALL: Self = Self {
        progression_id: true,
        rank: true,
        points: true,
    };
}

/// One changed entry: its list index, its new values and the lists that send them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasteryChange {
    /// The list index; the old length appends the entry.
    pub index: u32,
    /// The new values.
    pub entry: MasteryEntry,
    /// The lists that send it.
    pub fields: MasteryFields,
}

/// The categorical progression state; a child that is `None` is not sent, and a state without
/// children sends no slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CategoricalProgressionState {
    /// Child 0: the progression ids.
    pub progression_ids: Option<ReplicatedList<u32>>,
    /// Child 1: the ranks.
    pub ranks: Option<ReplicatedList<u16>>,
    /// Child 2: the points.
    pub points: Option<ReplicatedList<u64>>,
}

impl CategoricalProgressionState {
    /// The whole state with `version` (retail login: [`crate::items::FIRST_VERSION`]).
    pub fn full(version: u64, entries: &[MasteryEntry]) -> Self {
        Self {
            progression_ids: full_child(version, entries, |entry| entry.progression_id),
            ranks: full_child(version, entries, |entry| entry.rank),
            points: full_child(version, entries, |entry| entry.points),
        }
    }

    /// The change of `changes` with the shared `version`: each list sends the changes whose
    /// fields name it, in the order given.
    pub fn changes(version: u64, changes: &[MasteryChange]) -> Self {
        let child = |sent: fn(&MasteryFields) -> bool| {
            changes
                .iter()
                .filter(move |change| sent(&change.fields))
                .map(|change| (change.index, change.entry))
        };
        Self {
            progression_ids: child_changes(
                version,
                child(|fields| fields.progression_id)
                    .map(|(index, entry)| (index, entry.progression_id)),
            ),
            ranks: child_changes(
                version,
                child(|fields| fields.rank).map(|(index, entry)| (index, entry.rank)),
            ),
            points: child_changes(
                version,
                child(|fields| fields.points).map(|(index, entry)| (index, entry.points)),
            ),
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another slot or child, or a delta
    /// entry removes an element.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CHILDREN, |reader, child| {
            match child {
                IDS_CHILD => {
                    state.progression_ids =
                        Some(ReplicatedList::read(reader, FIELD, |reader| reader.u32())?);
                }
                RANKS_CHILD => {
                    state.ranks = Some(ReplicatedList::read(reader, FIELD, |reader| reader.u16())?);
                }
                _ => {
                    state.points = Some(ReplicatedList::read(reader, FIELD, |reader| reader.u64())?)
                }
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] from the list writer.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let children = children_mask(&[
            self.progression_ids.is_some(),
            self.ranks.is_some(),
            self.points.is_some(),
        ]);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_children(writer, children, CHILDREN, |writer, child| match child {
            IDS_CHILD => write_child(writer, self.progression_ids.as_ref(), |writer, id| {
                writer.u32(*id);
            }),
            RANKS_CHILD => write_child(writer, self.ranks.as_ref(), |writer, rank| {
                writer.u16(*rank);
            }),
            _ => write_child(writer, self.points.as_ref(), |writer, points| {
                writer.u64(*points);
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::FIRST_VERSION;

    /// Retail login (new character, 2026-09-18, t307): Cooking, Leatherworking, Fishing,
    /// SeasonPass and Camping, version 3.
    const RETAIL_LOGIN: [u8; 84] = [
        0x01, 0x07, 0x00, 0x01, 0x03, 0x05, 0x46, 0x7b, 0xe6, 0x6a, 0x73, 0x04, 0xcb, 0xe0, 0x75,
        0xbf, 0xff, 0xbd, 0x19, 0x55, 0x87, 0x0e, 0x81, 0xa9, 0x04, 0xe4, 0x00, 0x01, 0x03, 0x05,
        0x00, 0x32, 0x00, 0x32, 0x00, 0x32, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x05, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    /// Retail first kill (t204396): greatsword rank 1 with 10 points at index 5 and blunderbuss
    /// rank 0 with 4 points at index 6, version 5380.
    const RETAIL_FIRST_KILL: [u8; 57] = [
        0x01, 0x07, 0x02, 0x03, 0x05, 0x01, 0x84, 0x54, 0x18, 0x0b, 0x9a, 0x9f, 0x06, 0x01, 0x00,
        0x60, 0x20, 0xb3, 0x3f, 0x02, 0x03, 0x05, 0x01, 0x84, 0x54, 0x00, 0x01, 0x06, 0x01, 0x00,
        0x00, 0x00, 0x02, 0x03, 0x05, 0x01, 0x84, 0x54, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x0a, 0x06, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
    ];
    const RETAIL_FIRST_KILL_VERSION: u64 = 5380;
    /// Retail rank up of the blunderbuss (t237696): its rank at index 6, the points of both
    /// weapons, version 6248.
    const RETAIL_RANK_UP: [u8; 35] = [
        0x01, 0x06, 0x01, 0x01, 0x06, 0x01, 0xa8, 0x61, 0x00, 0x01, 0x02, 0x03, 0x05, 0x01, 0xa8,
        0x61, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0e, 0x06, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x0e,
    ];
    const RETAIL_RANK_UP_VERSION: u64 = 6248;
    /// Retail experience only (t281628): both weapons' points, version 7373.
    const RETAIL_POINTS: [u8; 27] = [
        0x01, 0x04, 0x02, 0x03, 0x05, 0x01, 0x8d, 0x73, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x22, 0x06, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12,
    ];
    const RETAIL_POINTS_VERSION: u64 = 7373;
    const GREATSWORD_INDEX: u32 = 5;
    const BLUNDERBUSS_INDEX: u32 = 6;

    fn entry(name: &str, rank: u16, points: u64) -> MasteryEntry {
        MasteryEntry {
            progression_id: progression_id(name),
            rank,
            points,
        }
    }

    fn change(index: u32, entry: MasteryEntry, fields: MasteryFields) -> MasteryChange {
        MasteryChange {
            index,
            entry,
            fields,
        }
    }

    fn encode(state: &CategoricalProgressionState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    fn decode(bytes: &[u8]) -> CategoricalProgressionState {
        let mut reader = Reader::new(bytes);
        let state = CategoricalProgressionState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        state
    }

    #[test]
    fn the_progression_id_is_the_retail_crc_of_the_lowercase_name() {
        assert_eq!(progression_id("GreatSwordAbilityTable"), 0x180b_9a9f);
        assert_eq!(progression_id("BlunderbussAbilityTable"), 0x6020_b33f);
    }

    #[test]
    fn the_login_state_is_the_retail_full_form() {
        let state = CategoricalProgressionState::full(
            FIRST_VERSION,
            &[
                entry("Cooking", 50, 0),
                entry("Leatherworking", 50, 0),
                entry("Fishing", 50, 0),
                entry("SeasonPass", 0, 25),
                entry("Camping", 1, 0),
            ],
        );
        assert_eq!(encode(&state), RETAIL_LOGIN);
        assert_eq!(decode(&RETAIL_LOGIN), state);
    }

    #[test]
    fn the_first_kill_appends_both_weapons_as_retail() {
        let state = CategoricalProgressionState::changes(
            RETAIL_FIRST_KILL_VERSION,
            &[
                change(
                    GREATSWORD_INDEX,
                    entry("GreatSwordAbilityTable", 1, 10),
                    MasteryFields::ALL,
                ),
                change(
                    BLUNDERBUSS_INDEX,
                    entry("BlunderbussAbilityTable", 0, 4),
                    MasteryFields::ALL,
                ),
            ],
        );
        assert_eq!(encode(&state), RETAIL_FIRST_KILL);
        assert_eq!(decode(&RETAIL_FIRST_KILL), state);
    }

    #[test]
    fn a_rank_up_and_an_experience_change_are_the_retail_deltas() {
        let greatsword = |points| entry("GreatSwordAbilityTable", 1, points);
        let blunderbuss = |rank, points| entry("BlunderbussAbilityTable", rank, points);
        let rank_up = CategoricalProgressionState::changes(
            RETAIL_RANK_UP_VERSION,
            &[
                change(GREATSWORD_INDEX, greatsword(14), MasteryFields::POINTS),
                change(
                    BLUNDERBUSS_INDEX,
                    blunderbuss(1, 14),
                    MasteryFields::RANK_AND_POINTS,
                ),
            ],
        );
        assert_eq!(encode(&rank_up), RETAIL_RANK_UP);
        assert_eq!(decode(&RETAIL_RANK_UP), rank_up);
        let points = CategoricalProgressionState::changes(
            RETAIL_POINTS_VERSION,
            &[
                change(GREATSWORD_INDEX, greatsword(34), MasteryFields::POINTS),
                change(BLUNDERBUSS_INDEX, blunderbuss(1, 18), MasteryFields::POINTS),
            ],
        );
        assert_eq!(encode(&points), RETAIL_POINTS);
        assert_eq!(decode(&RETAIL_POINTS), points);
    }

    #[test]
    fn no_change_is_no_slot_and_another_slot_is_refused() {
        let empty = CategoricalProgressionState::changes(1, &[]);
        assert_eq!(empty, CategoricalProgressionState::default());
        assert_eq!(encode(&empty), [0]);
        assert_eq!(decode(&[0]), empty);
        assert!(CategoricalProgressionState::read(&mut Reader::new(&[0x02])).is_err());
        assert!(CategoricalProgressionState::read(&mut Reader::new(&[0x01, 0x08])).is_err());
    }
}
