// SPDX-License-Identifier: AGPL-3.0-or-later
//! The affliction lists of the vitals block, slot 1 (vitals note, Round 4):
//! `replicatedAfflictionsHotData` (`FUN_14694edf0`) and `replicatedAfflictionsColdData`
//! (`FUN_14694eb20`).
//!
//! A list starts with a prefix count. A count above 0 is followed by the items in groups of at
//! most 8: a group byte whose bit `i` selects the long form of item `i`, then the items. Each
//! item is a byte and an id (a byte: 0 is no id, 1 adds a wide prefix integer), then its long
//! form when selected. A count of 0 is followed by an id, a second prefix count, and that many
//! items of a byte and the long form (`FUN_1469518e0`, `FUN_1469517e0`). The hot long form is
//! two big-endian binary16 values and two big-endian `u64` (20 bytes); the cold long form is a
//! binary16, a byte and a flag (4 bytes).

use crate::slots::children_mask;
use crate::wire::{MessageError, Reader, Writer};

const FIELD: &str = "affliction";
/// Items in one group, under one group byte.
const GROUP_ITEMS: usize = 8;
/// Most items of the full form (`FUN_1469518e0`).
const MAX_FULL_ITEMS: u64 = 0x200_0000;
/// Id kind byte: no id.
const ID_NONE: u8 = 0;
/// Id kind byte: a wide prefix integer follows.
const ID_WIDE: u8 = 1;

/// The id of an affliction item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfflictionId {
    /// Kind byte 0.
    None,
    /// Kind byte 1 and a wide prefix integer.
    Wide(u64),
}

impl AfflictionId {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        match reader.u8()? {
            ID_NONE => Ok(Self::None),
            ID_WIDE => reader.wide_prefixed().map(Self::Wide),
            kind => Err(MessageError::UnknownBits {
                field: FIELD,
                bits: kind,
            }),
        }
    }

    fn write(self, writer: &mut Writer) {
        match self {
            Self::None => {
                writer.u8(ID_NONE);
            }
            Self::Wide(id) => {
                writer.u8(ID_WIDE).wide_prefixed(id);
            }
        }
    }
}

/// The long form of the items of one list.
pub trait LongForm: Sized {
    /// Reads one long form.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the bytes end early or a field is out of its range.
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError>;

    /// Writes one long form.
    fn write(&self, writer: &mut Writer);
}

/// The hot long form (`FUN_145ce61b0`): two big-endian binary16 values (kept as their bits) and
/// two big-endian `u64`. The meaning is Unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotLong {
    /// The first binary16.
    pub a: u16,
    /// The second binary16.
    pub b: u16,
    /// The first `u64`.
    pub c: u64,
    /// The second `u64`.
    pub d: u64,
}

impl LongForm for HotLong {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            a: reader.u16()?,
            b: reader.u16()?,
            c: reader.u64()?,
            d: reader.u64()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.u16(self.a).u16(self.b).u64(self.c).u64(self.d);
    }
}

/// The cold long form (`FUN_145ce6130`): a big-endian binary16 (kept as its bits), a byte and a
/// flag (`FUN_1407d7c20`). The meaning is Unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColdLong {
    /// The binary16.
    pub value: u16,
    /// The byte.
    pub byte: u8,
    /// The flag.
    pub flag: bool,
}

impl LongForm for ColdLong {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            value: reader.u16()?,
            byte: reader.u8()?,
            flag: reader.boolean()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.u16(self.value).u8(self.byte).boolean(self.flag);
    }
}

/// One item of the counted form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Affliction<L> {
    /// The byte before the id. The meaning is Unknown.
    pub head: u8,
    /// The id.
    pub id: AfflictionId,
    /// The long form, when its group bit is set.
    pub long: Option<L>,
}

/// One affliction list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfflictionList<L> {
    /// A count above 0, then the items in groups. An empty list is written as the full form
    /// with no id and no item, which is what a count of 0 means.
    Items(Vec<Affliction<L>>),
    /// A count of 0: an id, then items of a byte and the long form.
    Full {
        /// The list id.
        id: AfflictionId,
        /// The items: the byte and the long form.
        items: Vec<(u8, L)>,
    },
}

impl<L: LongForm> AfflictionList<L> {
    /// Reads one list.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the bytes end early, a count is above the bytes left (or
    /// the full form's limit), an id kind is above 1, or a group byte selects an item past the
    /// group.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let count = checked_count(reader, u64::MAX)?;
        if count == 0 {
            let id = AfflictionId::read(reader)?;
            let items = (0..checked_count(reader, MAX_FULL_ITEMS)?)
                .map(|_| Ok((reader.u8()?, L::read(reader)?)))
                .collect::<Result<_, MessageError>>()?;
            return Ok(Self::Full { id, items });
        }
        let mut items = Vec::with_capacity(count);
        while items.len() < count {
            let in_group = (count - items.len()).min(GROUP_ITEMS);
            let group = reader.u8()?;
            if u32::from(group) >> in_group != 0 {
                return Err(MessageError::UnknownBits {
                    field: FIELD,
                    bits: group,
                });
            }
            for item in 0..in_group {
                items.push(Affliction {
                    head: reader.u8()?,
                    id: AfflictionId::read(reader)?,
                    long: (group >> item & 1 != 0)
                        .then(|| L::read(reader))
                        .transpose()?,
                });
            }
        }
        Ok(Self::Items(items))
    }

    /// Writes one list.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Prefix`] when a count does not fit the prefix code.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        match self {
            Self::Items(items) if !items.is_empty() => {
                writer.prefixed(items.len() as u64)?;
                for group in items.chunks(GROUP_ITEMS) {
                    let selected: Vec<bool> =
                        group.iter().map(|item| item.long.is_some()).collect();
                    // A group has at most 8 items, so its mask fits a byte.
                    writer.u8(children_mask(&selected) as u8);
                    for item in group {
                        writer.u8(item.head);
                        item.id.write(writer);
                        if let Some(long) = &item.long {
                            long.write(writer);
                        }
                    }
                }
            }
            Self::Items(_) => write_full::<L>(writer, AfflictionId::None, &[])?,
            Self::Full { id, items } => write_full(writer, *id, items)?,
        }
        Ok(())
    }
}

fn write_full<L: LongForm>(
    writer: &mut Writer,
    id: AfflictionId,
    items: &[(u8, L)],
) -> Result<(), MessageError> {
    writer.prefixed(0)?;
    id.write(writer);
    writer.prefixed(items.len() as u64)?;
    for (head, long) in items {
        writer.u8(*head);
        long.write(writer);
    }
    Ok(())
}

/// A prefix count, refused above `limit` or above the bytes left (every item takes a byte).
fn checked_count(reader: &mut Reader<'_>, limit: u64) -> Result<usize, MessageError> {
    let count = reader.prefixed()?;
    let left = reader.remaining() as u64;
    if count > limit || count > left {
        return Err(MessageError::CountTooLarge {
            count,
            limit: limit.min(left),
        });
    }
    // `count` is at most the bytes left, so it fits `usize`.
    Ok(count as usize)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const HEAD: u8 = 5;
    const WIDE_ID: u64 = 300;
    const HALF: u16 = 0x3c00;
    const TIME: u64 = 0x0102_0304_0506_0708;

    fn hot() -> HotLong {
        HotLong {
            a: HALF,
            b: HALF + 1,
            c: TIME,
            d: TIME + 1,
        }
    }

    fn read_all<L: LongForm>(bytes: &[u8]) -> AfflictionList<L> {
        let mut reader = Reader::new(bytes);
        let list = AfflictionList::read(&mut reader).expect("read");
        reader.finish().expect("to the end");
        list
    }

    #[test]
    fn a_counted_list_has_short_and_long_items() {
        let mut writer = Writer::new();
        writer.prefixed(2).expect("count").u8(0b10);
        writer.u8(HEAD).u8(0);
        writer.u8(HEAD + 1).u8(1).wide_prefixed(WIDE_ID);
        hot().write(&mut writer);
        let list: AfflictionList<HotLong> = read_all(&writer.finish());
        assert_eq!(
            list,
            AfflictionList::Items(vec![
                Affliction {
                    head: HEAD,
                    id: AfflictionId::None,
                    long: None,
                },
                Affliction {
                    head: HEAD + 1,
                    id: AfflictionId::Wide(WIDE_ID),
                    long: Some(hot()),
                },
            ])
        );
    }

    #[test]
    fn a_zero_count_is_the_full_form() {
        let mut writer = Writer::new();
        writer
            .prefixed(0)
            .expect("count")
            .u8(1)
            .wide_prefixed(WIDE_ID);
        writer.prefixed(1).expect("items").u8(HEAD);
        writer.u16(HALF).u8(HEAD).boolean(true);
        let list: AfflictionList<ColdLong> = read_all(&writer.finish());
        assert_eq!(
            list,
            AfflictionList::Full {
                id: AfflictionId::Wide(WIDE_ID),
                items: vec![(
                    HEAD,
                    ColdLong {
                        value: HALF,
                        byte: HEAD,
                        flag: true,
                    }
                )],
            }
        );
    }

    #[test]
    fn nine_items_take_two_group_bytes() {
        let items: Vec<Affliction<ColdLong>> = (0..9)
            .map(|head| Affliction {
                head,
                id: AfflictionId::None,
                long: None,
            })
            .collect();
        let mut writer = Writer::new();
        AfflictionList::Items(items.clone())
            .write(&mut writer)
            .expect("write");
        let bytes = writer.finish();
        // Count, group byte, 8 items of 2 bytes, group byte, 1 item of 2 bytes.
        assert_eq!(bytes.len(), 1 + 1 + 16 + 1 + 2);
        assert_eq!(read_all::<ColdLong>(&bytes), AfflictionList::Items(items));
    }

    #[test]
    fn an_id_kind_above_1_and_a_cold_flag_above_1_are_refused() {
        let mut bad_id = Writer::new();
        bad_id.prefixed(1).expect("count").u8(0).u8(HEAD).u8(2);
        let bad_id = bad_id.finish();
        let mut reader_id = Reader::new(&bad_id);
        assert!(AfflictionList::<HotLong>::read(&mut reader_id).is_err());
        let mut bad_flag = Writer::new();
        bad_flag.prefixed(1).expect("count").u8(1).u8(HEAD).u8(0);
        bad_flag.u16(HALF).u8(HEAD).u8(2);
        let bad_flag = bad_flag.finish();
        let mut reader_flag = Reader::new(&bad_flag);
        assert!(AfflictionList::<ColdLong>::read(&mut reader_flag).is_err());
    }

    #[test]
    fn a_count_above_the_bytes_left_is_refused() {
        let mut writer = Writer::new();
        writer.prefixed(1000).expect("count").u8(0);
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        assert!(matches!(
            AfflictionList::<HotLong>::read(&mut reader),
            Err(MessageError::CountTooLarge { .. })
        ));
    }

    fn id() -> impl Strategy<Value = AfflictionId> {
        prop_oneof![
            Just(AfflictionId::None),
            any::<u64>().prop_map(AfflictionId::Wide)
        ]
    }

    fn hot_long() -> impl Strategy<Value = HotLong> {
        (any::<u16>(), any::<u16>(), any::<u64>(), any::<u64>()).prop_map(|(a, b, c, d)| HotLong {
            a,
            b,
            c,
            d,
        })
    }

    fn hot_list() -> impl Strategy<Value = AfflictionList<HotLong>> {
        let item = (any::<u8>(), id(), proptest::option::of(hot_long()))
            .prop_map(|(head, id, long)| Affliction { head, id, long });
        prop_oneof![
            proptest::collection::vec(item, 1..20).prop_map(AfflictionList::Items),
            (
                id(),
                proptest::collection::vec((any::<u8>(), hot_long()), 0..4)
            )
                .prop_map(|(id, items)| AfflictionList::Full { id, items }),
        ]
    }

    proptest! {
        #[test]
        fn hot_lists_round_trip(list in hot_list()) {
            let mut writer = Writer::new();
            list.write(&mut writer).expect("write");
            prop_assert_eq!(read_all::<HotLong>(&writer.finish()), list);
        }
    }
}
