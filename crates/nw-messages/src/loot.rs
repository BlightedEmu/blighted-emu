// SPDX-License-Identifier: AGPL-3.0-or-later
//! The player states of a mob's loot bag (loot note 2026-09-18, retail kill at ledger
//! `newchar-retail-2026-09-18` 1792696951908): `MB::LootTrackerComponentReplicatedState` (type
//! 982, player component 33) lists the bags whose loot is the player's, and
//! `MB::GlobalStorageComponentReplicatedState` (type 2938, component 77) the items of each
//! storage the client asked for (`GlobalStorageComponentServerFacet_SetReplicateStorage`, 387).
//!
//! Both have one slot. Its child 0 is a map in the replicated list form ([`FullList`]): the
//! loot tracker's `m_lootDataMap` (reader `FUN_143728b00`, element `FUN_143728d60`), the global
//! storage's `m_globalItemMap` (`FUN_142d1c0c0`). The loot tracker's child 5 is its
//! `m_lootLimitDataMap` in the same form (reader `FUN_14377a810`: the list's version and count,
//! then per entry a u32 key and the element `FUN_145ce8230`: u64, u64, u16, u8). The other
//! children (`m_lootCollectibles`, ...; `m_overflowItemCount`, ...) are not encoded here.

use crate::items::{FullList, ItemRecord};
use crate::slots::{SLOT_0, children_mask, read_slot_0, write_children};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `MB::LootTrackerComponentReplicatedState`.
pub const LOOT_TRACKER_TYPE_INDEX: u64 = 982;
/// Type index of `MB::GlobalStorageComponentReplicatedState`.
pub const GLOBAL_STORAGE_TYPE_INDEX: u64 = 2938;
/// The player component of the loot tracker.
pub const LOOT_TRACKER_COMPONENT: u64 = 33;
/// The player component of the global storage.
pub const GLOBAL_STORAGE_COMPONENT: u64 = 77;

/// Children of the loot tracker's slot 0 (`m_lootDataMap`, `m_lootCollectibles`,
/// `m_failedRollBonusPercent`, `m_slayerScriptDataMap`, `m_lootDivertMap`,
/// `m_lootLimitDataMap`).
const LOOT_TRACKER_CHILDREN: u8 = 6;
/// Children of the global storage's slot 0 (`m_globalItemMap`, `m_overflowItemCount`,
/// `m_weightMap`, `m_slotCountMap`).
const GLOBAL_STORAGE_CHILDREN: u8 = 4;
/// The only child of the global storage encoded here: child 0.
const MAP_CHILD: u16 = 1;
/// The loot tracker's `m_lootDataMap`.
const LOOT_DATA_CHILD: u8 = 0;
/// The loot tracker's `m_lootLimitDataMap`.
const LOOT_LIMIT_CHILD: u8 = 5;
const LOOT_TRACKER_FIELD: &str = "loot tracker";
const GLOBAL_STORAGE_FIELD: &str = "global storage";

/// `m_lootState` of a bag whose loot was rolled and is not taken (retail 1 at the kill).
pub const LOOT_ROLLED: u32 = 1;
/// `m_lootState` of a bag the player looted (retail 2 at the take).
pub const LOOT_TAKEN: u32 = 2;
/// `m_lootType` of a point of interest's loot: a chest the player opened (craft2 report: retail
/// 0 at each chest's open; the client's enum names it `POILoot`).
pub const LOOT_POI: u32 = 0;
/// `m_lootType` of a creature's loot (retail 1 for every mob bag).
pub const LOOT_CREATURE: u32 = 1;
/// `m_lootType` of a bag of items its owner dropped (retail 5 at each drop, open-world
/// `item_actions`).
pub const LOOT_DROPPED: u32 = 5;

/// One bag of the loot tracker: `InstancedLootData` (`FUN_14376b520`) keyed by the bag id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackedLoot {
    /// The bag's entity id (its `GdeRef`).
    pub bag: [u8; RAW_ID_LEN],
    /// `m_lootState`: [`LOOT_ROLLED`], [`LOOT_TAKEN`].
    pub state: u32,
    /// `m_lootType`: [`LOOT_CREATURE`].
    pub kind: u32,
    /// `m_resetTime`, unix ns.
    pub reset_ns: u64,
    /// `m_lootedTime`, unix ns.
    pub looted_ns: u64,
    /// `m_requiresPrivilegedLooters`.
    pub privileged: bool,
    /// `m_position`.
    pub position: [f32; 3],
    /// `m_interactTag`.
    pub interact_tag: u32,
    /// The byte after `m_interactTag` (its name is not reflected; retail 0 to 4, the best rarity
    /// of the bag's items for gear: Observed, loot note).
    pub rarity: u8,
}

impl TrackedLoot {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            bag: reader.raw_id()?,
            state: reader.u32()?,
            kind: reader.u32()?,
            reset_ns: reader.u64()?,
            looted_ns: reader.u64()?,
            privileged: reader.boolean()?,
            position: [reader.f32()?, reader.f32()?, reader.f32()?],
            interact_tag: reader.u32()?,
            rarity: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .bytes(&self.bag)
            .u32(self.state)
            .u32(self.kind)
            .u64(self.reset_ns)
            .u64(self.looted_ns)
            .boolean(self.privileged);
        for axis in self.position {
            writer.f32(axis);
        }
        writer.u32(self.interact_tag).u8(self.rarity);
    }
}

/// One count of the player's loot limits: an entry of `m_lootLimitDataMap`
/// (`nw_content::loot_limits` has what the values mean).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LootLimitEntry {
    /// The key: the CRC-32 of the lowercase `LootLimitID`.
    pub key: u32,
    /// The first time, unix ns: the last count, or the end of a count at its limit.
    pub first_ns: u64,
    /// The end of the count, unix ns.
    pub ends_ns: u64,
    /// The count.
    pub count: u16,
    /// The last byte (its name is not reflected; retail 0 in all 1,406 entries).
    pub last: u8,
}

impl LootLimitEntry {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            key: reader.u32()?,
            first_ns: reader.u64()?,
            ends_ns: reader.u64()?,
            count: reader.u16()?,
            last: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer
            .u32(self.key)
            .u64(self.first_ns)
            .u64(self.ends_ns)
            .u16(self.count)
            .u8(self.last);
    }
}

/// The loot tracker: `m_lootDataMap` and `m_lootLimitDataMap`, each when sent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootTrackerState {
    /// The player's bags.
    pub loot: Option<FullList<TrackedLoot>>,
    /// The counts of the player's loot limits.
    pub limits: Option<FullList<LootLimitEntry>>,
}

impl LootTrackerState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets a child other than 0 and 5, or
    /// a map is a delta.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            LOOT_TRACKER_FIELD,
            LOOT_TRACKER_CHILDREN,
            |reader, child| {
                match child {
                    LOOT_DATA_CHILD => {
                        state.loot = Some(FullList::read(reader, TrackedLoot::read)?);
                    }
                    LOOT_LIMIT_CHILD => {
                        state.limits = Some(FullList::read(reader, LootLimitEntry::read)?);
                    }
                    _ => {
                        return Err(MessageError::Unsized {
                            field: LOOT_TRACKER_FIELD,
                            byte: child,
                        });
                    }
                }
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path; without a map the slot is not sent.
    ///
    /// # Errors
    ///
    /// Never fails today; the list writer's form keeps the signature of the other states.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let mut present = [false; LOOT_TRACKER_CHILDREN as usize];
        present[usize::from(LOOT_DATA_CHILD)] = self.loot.is_some();
        present[usize::from(LOOT_LIMIT_CHILD)] = self.limits.is_some();
        let children = children_mask(&present);
        if children == 0 {
            writer.u8(0);
            return Ok(());
        }
        writer.u8(SLOT_0);
        write_children(
            writer,
            children,
            LOOT_TRACKER_CHILDREN,
            |writer, child| match (child, &self.loot, &self.limits) {
                (LOOT_DATA_CHILD, Some(loot), _) => loot.write(writer, |writer, loot| {
                    loot.write(writer);
                    Ok(())
                }),
                (LOOT_LIMIT_CHILD, _, Some(limits)) => limits.write(writer, |writer, limit| {
                    limit.write(writer);
                    Ok(())
                }),
                _ => Ok(()),
            },
        )
    }
}

/// The items of one storage of the global storage map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredItems {
    /// The storage id (a loot bag's entity id).
    pub storage: [u8; RAW_ID_LEN],
    /// Its items.
    pub items: Vec<ItemRecord>,
}

impl StoredItems {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let storage = reader.raw_id()?;
        let count = reader.wide_prefixed()?;
        Ok(Self {
            storage,
            items: (0..count)
                .map(|_| ItemRecord::read(reader))
                .collect::<Result<_, _>>()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        // A `usize` count fits 64 bits.
        writer
            .bytes(&self.storage)
            .wide_prefixed(self.items.len() as u64);
        self.items.iter().try_for_each(|item| item.write(writer))
    }
}

/// The global storage: `m_globalItemMap`, when sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalStorageState {
    /// The storages the client asked for.
    pub storages: Option<FullList<StoredItems>>,
}

impl GlobalStorageState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, sets another child, or the map is a
    /// delta.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_map(
            reader,
            GLOBAL_STORAGE_FIELD,
            GLOBAL_STORAGE_CHILDREN,
            |reader| {
                state.storages = Some(FullList::read(reader, StoredItems::read)?);
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when an item does not fit its packed form.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        write_map(
            writer,
            self.storages.as_ref(),
            GLOBAL_STORAGE_CHILDREN,
            |writer, list| list.write(writer, |writer, stored| stored.write(writer)),
        )
    }
}

/// Reads a one-slot state whose only known child is the map of child 0, which `read_list`
/// reads.
fn read_map(
    reader: &mut Reader<'_>,
    field: &'static str,
    children: u8,
    mut read_list: impl FnMut(&mut Reader<'_>) -> Result<(), MessageError>,
) -> Result<(), MessageError> {
    read_slot_0(reader, field, children, |reader, child| {
        if child != 0 {
            return Err(MessageError::Unsized { field, byte: child });
        }
        read_list(reader)
    })?;
    Ok(())
}

/// Writes a one-slot state with only the map of child 0, which `write_list` writes; without a
/// map the slot is not sent.
fn write_map<T>(
    writer: &mut Writer,
    list: Option<&T>,
    children: u8,
    mut write_list: impl FnMut(&mut Writer, &T) -> Result<(), MessageError>,
) -> Result<(), MessageError> {
    let Some(list) = list else {
        writer.u8(0);
        return Ok(());
    };
    writer.u8(SLOT_0);
    write_children(writer, MAP_CHILD, children, |writer, _| {
        write_list(writer, list)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::tests::hex as bytes;
    use crate::items::{ITEM_BOUND, SYNC_OWNED};

    /// The retail bodies of the kill (loot note): the bag at the kill (982 at +57 ms, without
    /// its `m_lootLimitDataMap`), the bag looted (+4323 ms), the storage of the bag (2938 at
    /// +1322 ms) and the storage emptied (+4323 ms).
    const BAG: &str = "b88319b3219d456295f52fb304838f5c";
    const TRACKED: &str = "01010001d32e0401b88319b3219d456295f52fb304838f5c0000000100000001\
        18e0f0a7090e9e1818e0f0612fa9e55001461a0925450e80f8422bb0870000000001";
    const LOOTED: &str = "01010001d3320401b88319b3219d456295f52fb304838f5c0000000200000001\
        18e0f062e1797b6418e0f0612fa9e55001461a0925450e80f8422bb0870000000001";
    const STORED: &str = "01010001d92f0401b88319b3219d456295f52fb304838f5c0100013a1693c3d974\
        b82e006eb312b51600000000000000f4fe8f529320415e936ee41c7483ea90010000000000000000";
    const EMPTIED: &str = "01010001d3320400";

    fn bag() -> [u8; RAW_ID_LEN] {
        bytes(BAG).try_into().expect("bag id")
    }

    fn tracked(state: u32, version: u64, reset_ns: u64) -> LootTrackerState {
        LootTrackerState {
            loot: Some(FullList {
                version: Some(version),
                elements: vec![TrackedLoot {
                    bag: bag(),
                    state,
                    kind: LOOT_CREATURE,
                    reset_ns,
                    looted_ns: 0x18e0_f061_2fa9_e550,
                    privileged: true,
                    position: [9858.286, 2280.0605, 42.92239],
                    interact_tag: 0,
                    rarity: 1,
                }],
            }),
            limits: None,
        }
    }

    fn written(state: &LootTrackerState) -> Vec<u8> {
        let mut writer = Writer::new();
        state.write(&mut writer).expect("write");
        writer.finish()
    }

    #[test]
    fn the_loot_tracker_writes_the_retail_bodies() {
        const KILL_VERSION: u64 = 34_259;
        const TAKE_VERSION: u64 = 34_387;
        let at_kill = tracked(LOOT_ROLLED, KILL_VERSION, 0x18e0_f0a7_090e_9e18);
        assert_eq!(written(&at_kill), bytes(TRACKED));
        let looted = tracked(LOOT_TAKEN, TAKE_VERSION, 0x18e0_f062_e179_7b64);
        assert_eq!(written(&looted), bytes(LOOTED));
        let body = bytes(LOOTED);
        let mut reader = Reader::new(&body);
        assert_eq!(LootTrackerState::read(&mut reader), Ok(looted));
        assert!(reader.is_empty());
        assert_eq!(written(&LootTrackerState::default()), [0]);
    }

    #[test]
    fn the_global_storage_writes_the_retail_bodies() {
        const STORE_VERSION: u64 = 34_297;
        const EMPTY_VERSION: u64 = 34_387;
        let item = ItemRecord {
            quantity: 1,
            sync_reason: SYNC_OWNED,
            container_slot: None,
            item_index: 23_228,
            durability: 3_000,
            max_durability: 3_000,
            counter: 0,
            gear_score: 110,
            perks: [1_203, 1_461, 0, 0, 0],
            instance: bytes("f4fe8f529320415e936ee41c7483ea90")
                .try_into()
                .expect("id"),
            flags: ITEM_BOUND,
            ..ItemRecord::EMPTY_SLOT
        };
        let stored = GlobalStorageState {
            storages: Some(FullList {
                version: Some(STORE_VERSION),
                elements: vec![StoredItems {
                    storage: bag(),
                    items: vec![item],
                }],
            }),
        };
        let write = |state: &GlobalStorageState| {
            let mut writer = Writer::new();
            state.write(&mut writer).expect("write");
            writer.finish()
        };
        assert_eq!(write(&stored), bytes(STORED));
        let emptied = GlobalStorageState {
            storages: Some(FullList {
                version: Some(EMPTY_VERSION),
                elements: Vec::new(),
            }),
        };
        assert_eq!(write(&emptied), bytes(EMPTIED));
        let body = bytes(STORED);
        let mut reader = Reader::new(&body);
        assert_eq!(GlobalStorageState::read(&mut reader), Ok(stored));
        assert!(reader.is_empty());
    }

    #[test]
    fn another_child_is_unsized() {
        // Child 1 (`m_lootCollectibles`) after the map.
        let mut body = bytes(TRACKED);
        body[1] = 0x03;
        assert!(matches!(
            LootTrackerState::read(&mut Reader::new(&body)),
            Err(MessageError::Unsized { .. })
        ));
    }

    /// The retail state at the kill (ledger `newchar-retail-2026-09-18` 1792696951965): the bag
    /// (child 0) and the count of `2hWarhammer_FTUE_CorruptedSailer_V2` (child 5), both maps at
    /// the same version.
    const TRACKED_WITH_LIMIT: &str = "01210001d32e0401b88319b3219d456295f52fb304838f5c00000001\
        0000000118e0f0a7090e9e1818e0f0612fa9e55001461a0925450e80f8422bb08700000000010001d32e04\
        0132c77cf418e2574f910f754818e2574f910f7548000100";
    /// The retail `m_lootLimitDataMap` of the login state of ledger `nightaven`
    /// (1789758345255): six counts, ended ones too.
    const LOGIN_LIMITS: &str = "0001d9e90506863500b017a7a4fc23bd6fb817a7a616cbca0944000100\
        f5c2c85717a7a48847eb484817a7a616b8cc8d08000300cd60623017a753820698349817a7578235737f80\
        000300e64428531791f0ce2514b04417922630d687db340001001c894dc218d4fd58fbc49d1818d55f4a8c\
        a93ad4033900ddbd0ff6186d01f99d8eb5c0186d01f99d8eb5c0000300";
    /// A slot byte and a child byte that sets child 5 only.
    const LIMITS_ONLY: &str = "0120";

    #[test]
    fn the_loot_limit_map_writes_the_retail_bodies() {
        const KILL_VERSION: u64 = 34_259;
        const LOGIN_VERSION: u64 = 48_441;
        // Tuesday 2026-10-27 09:00 UTC with the kill's sub-second part.
        const WEEKLY_END_NS: u64 = 1_793_091_600_894_621_000;
        let mut at_kill = tracked(LOOT_ROLLED, KILL_VERSION, 0x18e0_f0a7_090e_9e18);
        at_kill.limits = Some(FullList {
            version: Some(KILL_VERSION),
            elements: vec![LootLimitEntry {
                key: 0x32c7_7cf4,
                first_ns: WEEKLY_END_NS,
                ends_ns: WEEKLY_END_NS,
                count: 1,
                last: 0,
            }],
        });
        assert_eq!(written(&at_kill), bytes(TRACKED_WITH_LIMIT));
        let body = bytes(TRACKED_WITH_LIMIT);
        let mut reader = Reader::new(&body);
        assert_eq!(LootTrackerState::read(&mut reader), Ok(at_kill));
        assert!(reader.is_empty());

        let body = bytes(&format!("{LIMITS_ONLY}{LOGIN_LIMITS}"));
        let mut reader = Reader::new(&body);
        let login = LootTrackerState::read(&mut reader).expect("read");
        assert!(reader.is_empty());
        assert_eq!(login.loot, None);
        let limits = login.limits.as_ref().expect("limits");
        assert_eq!(limits.version, Some(LOGIN_VERSION));
        assert_eq!(limits.elements.len(), 6);
        assert_eq!(
            limits.elements[4],
            LootLimitEntry {
                key: 0x1c89_4dc2,
                first_ns: 1_789_333_510_595_845_400,
                ends_ns: 1_789_441_200_741_759_700,
                count: 825,
                last: 0,
            },
            "the `UmbralCurrency` count"
        );
        assert_eq!(written(&login), body);
    }
}
