// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ProgressionPointReplicatedState` (type 3681, player component 67): the unspent points of
//! the weapon and territory pools and the points spent in them (progression note, sections 1
//! and 4).
//!
//! Slot 0 holds four parallel lists: child 0 the pool ids (`u32`; a weapon pool has the type 911
//! id of its mastery), child 1 the unspent points of each pool (`u16`), child 2 the pool
//! categories (`u8`), child 3 the spent points (10 bytes each: pool, point, level). Retail sends
//! the four lists empty
//! at login of a new character, the three pool lists whole when the first pool comes, a new pool
//! as an appended entry of the three pool lists, and a count change of one pool after each
//! weapon rank up (+1) and each spent point.

use crate::list_delta::{ReplicatedList, child_changes, full_child, write_child};
use crate::slots::{SLOT_0, children_mask, read_slot_0, write_children};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of `ProgressionPointReplicatedState`.
pub const PROGRESSION_POINT_TYPE_INDEX: u64 = 3681;
/// The player component of the progression points.
pub const PROGRESSION_POINT_COMPONENT: u64 = 67;

/// Children of slot 0; child 3 is the spent points.
const CHILDREN: u8 = 4;
const POOL_IDS_CHILD: u8 = 0;
const POOL_COUNTS_CHILD: u8 = 1;
const POOL_CATEGORIES_CHILD: u8 = 2;
const FIELD: &str = "progression points";

/// The category of a pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PoolCategory {
    /// A weapon mastery pool.
    Weapon = 1,
    /// A territory standing pool.
    Territory = 2,
}

impl PoolCategory {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        match reader.u8()? {
            tag if tag == Self::Weapon as u8 => Ok(Self::Weapon),
            tag if tag == Self::Territory as u8 => Ok(Self::Territory),
            tag => Err(MessageError::BadTag { field: FIELD, tag }),
        }
    }
}

/// One pool: an entry of the three pool lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointPool {
    /// The pool id (CRC-32 of the lowercase pool name).
    pub pool: u32,
    /// The unspent points.
    pub count: u16,
    /// The category.
    pub category: PoolCategory,
}

/// One spent point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointEntry {
    /// The pool id.
    pub pool: u32,
    /// The `ProgressionPointId` CRC.
    pub point: u32,
    /// The points in it: retail's rose by the points of each 1112 (newchar 2026-09-18: 1 each
    /// time); territory standing entries hold up to 14 and refunded weapon entries 0
    /// (NH-MSQ-2.1).
    pub level: u16,
}

impl PointEntry {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            pool: reader.u32()?,
            point: reader.u32()?,
            level: reader.u16()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.u32(self.pool).u32(self.point).u16(self.level);
    }
}

/// The progression point state; a child that is `None` is not sent, and a state without
/// children sends no slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgressionPointState {
    /// Child 0: the pool ids.
    pub pool_ids: Option<ReplicatedList<u32>>,
    /// Child 1: the unspent points of each pool.
    pub pool_counts: Option<ReplicatedList<u16>>,
    /// Child 2: the pool categories.
    pub pool_categories: Option<ReplicatedList<PoolCategory>>,
    /// Child 3: the spent points.
    pub point_entries: Option<ReplicatedList<PointEntry>>,
}

impl ProgressionPointState {
    /// The three pool lists whole with `version` and no spent points list (retail: the first
    /// pool of a new character).
    pub fn pools(version: u64, pools: &[PointPool]) -> Self {
        Self {
            pool_ids: full_child(version, pools, |pool| pool.pool),
            pool_counts: full_child(version, pools, |pool| pool.count),
            pool_categories: full_child(version, pools, |pool| pool.category),
            point_entries: None,
        }
    }

    /// The whole state with `version` (retail login: [`crate::items::FIRST_VERSION`]).
    pub fn full(version: u64, pools: &[PointPool], points: &[PointEntry]) -> Self {
        Self {
            point_entries: full_child(version, points, |point| *point),
            ..Self::pools(version, pools)
        }
    }

    /// A new pool at list index `index` (the old length) with `version`.
    #[cfg(test)]
    pub(crate) fn added_pool(version: u64, index: u32, pool: PointPool) -> Self {
        Self {
            pool_ids: child_changes(version, [(index, pool.pool)]),
            pool_counts: child_changes(version, [(index, pool.count)]),
            pool_categories: child_changes(version, [(index, pool.category)]),
            point_entries: None,
        }
    }

    /// This state with the whole spent point list `points` at `version` (retail's answer to a
    /// 1112: the pool's new count and the whole point list, newchar 2026-09-18).
    pub fn with_points(self, version: u64, points: &[PointEntry]) -> Self {
        Self {
            point_entries: full_child(version, points, |point| *point),
            ..self
        }
    }

    /// New unspent counts, as (pool list index, count) in send order, with the shared `version`.
    pub fn pool_counts(version: u64, counts: &[(u32, u16)]) -> Self {
        Self {
            pool_counts: child_changes(version, counts.iter().copied()),
            ..Self::default()
        }
    }

    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another slot or child, sends an
    /// unknown pool category, or a delta entry removes an element.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, FIELD, CHILDREN, |reader, child| {
            match child {
                POOL_IDS_CHILD => {
                    state.pool_ids =
                        Some(ReplicatedList::read(reader, FIELD, |reader| reader.u32())?);
                }
                POOL_COUNTS_CHILD => {
                    state.pool_counts =
                        Some(ReplicatedList::read(reader, FIELD, |reader| reader.u16())?);
                }
                POOL_CATEGORIES_CHILD => {
                    state.pool_categories =
                        Some(ReplicatedList::read(reader, FIELD, PoolCategory::read)?);
                }
                _ => {
                    state.point_entries =
                        Some(ReplicatedList::read(reader, FIELD, PointEntry::read)?);
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
            self.pool_ids.is_some(),
            self.pool_counts.is_some(),
            self.pool_categories.is_some(),
            self.point_entries.is_some(),
        ]);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_children(writer, children, CHILDREN, |writer, child| match child {
            POOL_IDS_CHILD => write_child(writer, self.pool_ids.as_ref(), |writer, pool| {
                writer.u32(*pool);
            }),
            POOL_COUNTS_CHILD => write_child(writer, self.pool_counts.as_ref(), |writer, count| {
                writer.u16(*count);
            }),
            POOL_CATEGORIES_CHILD => {
                write_child(writer, self.pool_categories.as_ref(), |writer, category| {
                    writer.u8(*category as u8);
                })
            }
            _ => write_child(writer, self.point_entries.as_ref(), |writer, point| {
                point.write(writer);
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::FIRST_VERSION;
    use nw_formats::objectstream::lowercase_crc32;

    /// Retail login of a new character (2026-09-18, t307): four empty lists, version 3.
    const RETAIL_LOGIN: [u8; 18] = [
        0x01, 0x0f, 0x00, 0x01, 0x03, 0x00, 0x00, 0x01, 0x03, 0x00, 0x00, 0x01, 0x03, 0x00, 0x00,
        0x01, 0x03, 0x00,
    ];
    /// Retail first pool (t204396): the greatsword pool with 1 point, version 5380.
    const RETAIL_FIRST_POOL: [u8; 24] = [
        0x01, 0x07, 0x00, 0x01, 0x84, 0x54, 0x01, 0x18, 0x0b, 0x9a, 0x9f, 0x00, 0x01, 0x84, 0x54,
        0x01, 0x00, 0x01, 0x00, 0x01, 0x84, 0x54, 0x01, 0x01,
    ];
    const RETAIL_FIRST_POOL_VERSION: u64 = 5380;
    /// Retail second pool (t237696): the blunderbuss pool with 1 point at index 1, version 6248.
    const RETAIL_ADDED_POOL: [u8; 27] = [
        0x01, 0x07, 0x01, 0x01, 0x01, 0x01, 0xa8, 0x61, 0x60, 0x20, 0xb3, 0x3f, 0x01, 0x01, 0x01,
        0x01, 0xa8, 0x61, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0xa8, 0x61, 0x01,
    ];
    const RETAIL_ADDED_POOL_VERSION: u64 = 6248;
    /// Retail rank up (t282445): the greatsword pool count to 1, version 7396.
    const RETAIL_COUNT: [u8; 10] = [0x01, 0x02, 0x01, 0x01, 0x00, 0x01, 0xa4, 0x73, 0x00, 0x01];
    const RETAIL_COUNT_VERSION: u64 = 7396;
    /// Retail spent point (t597078): the blunderbuss pool count to 0 and two point entries at
    /// index 2 (first all zero, then the point), version 15704.
    const RETAIL_SPENT: [u8; 39] = [
        0x01, 0x0a, 0x01, 0x01, 0x01, 0x01, 0x98, 0xf5, 0x00, 0x00, 0x02, 0x03, 0x02, 0x01, 0x98,
        0xf5, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x01, 0x00, 0x60,
        0x20, 0xb3, 0x3f, 0x47, 0x3a, 0xc2, 0xed, 0x00, 0x01,
    ];

    /// Retail answer to the first 1112 (newchar 2026-09-18): the greatsword pool count to 0 and
    /// the whole point list with `Ability_Greatsword_Combo` at level 1, version 6787.
    const RETAIL_ASSIGNED: [u8; 25] = [
        0x01, 0x0a, 0x01, 0x01, 0x00, 0x01, 0x83, 0x6a, 0x00, 0x00, 0x00, 0x01, 0x83, 0x6a, 0x01,
        0x18, 0x0b, 0x9a, 0x9f, 0x2e, 0xdf, 0x28, 0xbe, 0x00, 0x01,
    ];
    const RETAIL_ASSIGNED_VERSION: u64 = 6787;

    fn weapon_pool(name: &str) -> PointPool {
        PointPool {
            pool: lowercase_crc32(name),
            count: 1,
            category: PoolCategory::Weapon,
        }
    }

    fn encode(state: &ProgressionPointState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    fn decode(bytes: &[u8]) -> ProgressionPointState {
        let mut reader = Reader::new(bytes);
        let state = ProgressionPointState::read(&mut reader).expect("read");
        reader.finish().expect("whole");
        state
    }

    #[test]
    fn the_empty_login_is_the_retail_body() {
        let state = ProgressionPointState::full(FIRST_VERSION, &[], &[]);
        assert_eq!(encode(&state), RETAIL_LOGIN);
        assert_eq!(decode(&RETAIL_LOGIN), state);
    }

    #[test]
    fn the_first_and_the_second_pool_are_the_retail_bodies() {
        let first = ProgressionPointState::pools(
            RETAIL_FIRST_POOL_VERSION,
            &[weapon_pool("GreatSwordAbilityTable")],
        );
        assert_eq!(encode(&first), RETAIL_FIRST_POOL);
        assert_eq!(decode(&RETAIL_FIRST_POOL), first);
        let second = ProgressionPointState::added_pool(
            RETAIL_ADDED_POOL_VERSION,
            1,
            weapon_pool("BlunderbussAbilityTable"),
        );
        assert_eq!(encode(&second), RETAIL_ADDED_POOL);
        assert_eq!(decode(&RETAIL_ADDED_POOL), second);
    }

    #[test]
    fn a_count_change_is_the_retail_delta() {
        let state = ProgressionPointState::pool_counts(RETAIL_COUNT_VERSION, &[(0, 1)]);
        assert_eq!(encode(&state), RETAIL_COUNT);
        assert_eq!(decode(&RETAIL_COUNT), state);
    }

    #[test]
    fn a_spent_point_reads_and_writes_the_same_bytes() {
        let state = decode(&RETAIL_SPENT);
        assert_eq!(encode(&state), RETAIL_SPENT);
        let Some(ReplicatedList::Changes(points)) = &state.point_entries else {
            panic!("point changes: {state:?}");
        };
        assert_eq!(
            points[1].value.pool,
            lowercase_crc32("BlunderbussAbilityTable")
        );
        assert_eq!(points[1].value.level, 1);
        assert_eq!(state.pool_ids, None);
    }

    #[test]
    fn an_assigned_point_is_the_retail_answer() {
        let point = PointEntry {
            pool: lowercase_crc32("GreatSwordAbilityTable"),
            point: lowercase_crc32("Ability_Greatsword_Combo"),
            level: 1,
        };
        let state = ProgressionPointState::pool_counts(RETAIL_ASSIGNED_VERSION, &[(0, 0)])
            .with_points(RETAIL_ASSIGNED_VERSION, &[point]);
        assert_eq!(encode(&state), RETAIL_ASSIGNED);
        assert_eq!(decode(&RETAIL_ASSIGNED), state);
    }

    #[test]
    fn an_unknown_category_and_no_slot() {
        // One pool category list with the value 3.
        let unknown = [0x01, 0x04, 0x00, 0x01, 0x03, 0x01, 0x03];
        assert!(matches!(
            ProgressionPointState::read(&mut Reader::new(&unknown)),
            Err(MessageError::BadTag { .. })
        ));
        assert_eq!(encode(&ProgressionPointState::default()), [0]);
        assert_eq!(decode(&[0]), ProgressionPointState::default());
    }
}
