// SPDX-License-Identifier: AGPL-3.0-or-later
//! The wire layout of every replicated state class the captures hold, so a bundle walk passes a
//! member of a class without a reader here ([`MemberState::Recorded`]) instead of stopping.
//!
//! **Observed** (client build 25810808, decoder coverage work 2026-09-26): each state's
//! constructor registers its fields with `FUN_141776b80(state, name, field, slot)`, the slot
//! being the handle `FUN_1416789e0` returned less 1 (a literal 0 is slot 0); each field object's
//! virtual `+0x30` reads its wire form, `+0x78` the full form of a list. A class's constructor is
//! found from its type id function (the class UUID of `typeindex.json`), whose factory's second
//! virtual allocates the state and constructs it. The constructors were emulated in Ghidra to
//! list the fields in registration order, and each reader was classed into the forms of
//! [`Wire`]; [`table`] is generated from that. Over the 728 capture ledgers (25.3 M bundles) the
//! table reads 99.99 % of the bundles to their end.
//!
//! A body is the slot byte, then per slot that is set its child bytes ([`crate::slots`]) and the
//! children they set. A list or map child is a header count: 0 for the full form (a version, a
//! count and the entries), else that many delta entries in groups of up to 8, each group a mask
//! byte, then per entry its key, its version and, when its mask bit is set, its value.
//!
//! **Inferred**: the reader classing reads every call of a reader in order and does not follow
//! its branches. A field read only under a flag of the same body (the paperdoll's item visuals)
//! comes out too long, so a class's own reader here wins wherever it reads the body.
//!
//! [`MemberState::Recorded`]: crate::payload::MemberState::Recorded

mod table;

use crate::slots::read_children_64;
use crate::wire::{MessageError, Reader};

/// The field of the errors of this walk.
const FIELD: &str = "member layout";
/// The most entries a list count may give (a count beyond it is a misread body).
const MAX_ENTRIES: u64 = 100_000;
/// Entries per delta group: one mask byte covers them.
const DELTA_GROUP: u64 = 8;
/// A list version's flag (`FUN_1461adf60`): 0 for none, 1 for a wide integer that follows.
const VERSION_ABSENT: u8 = 0;
const VERSION_PRESENT: u8 = 1;
/// A flag id's bit 0: clear for a text id, set for 16 raw bytes.
const FLAG_ID_RAW: u8 = 1;
const FLAG_ID_RAW_LEN: usize = 16;
/// A packed quaternion's flag (`FUN_14087b3f0`): per part, its zero bit and its one bit; a part
/// with neither is 2 bytes.
const QUAT_PARTS: [(u8, u8); 3] = [(0x01, 0x08), (0x02, 0x10), (0x04, 0x20)];
const QUAT_PART_LEN: usize = 2;
/// A packed turn's flag (`FUN_14087aa70`): 3 bytes, 2 with bit 1; one fewer for each of bits 2
/// and 3, and for bit 4 while any are left.
const TURN_FULL_LEN: usize = 3;
const TURN_SHORT_BIT: u8 = 0x02;
const TURN_DROP_BITS: [u8; 2] = [0x04, 0x08];
const TURN_LAST_DROP_BIT: u8 = 0x10;
/// A scale's flag (`FUN_14087b250`): 0 the unit scale, 1 one `f16` for all axes, else three.
const SCALE_UNIT: u8 = 0;
const SCALE_UNIFORM: u8 = 1;
const HALF_LEN: usize = 2;
const AXES: usize = 3;

/// The wire form of a child, or of a part of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wire {
    /// That many bytes.
    Fixed(usize),
    /// A prefix-coded integer (`FUN_14087b760`).
    Prefixed,
    /// A wide prefix-coded integer (`FUN_14087b910`).
    Wide,
    /// A prefix-coded length and that many bytes (`FUN_14087a570`, `FUN_1407fc810`).
    Text,
    /// A flag byte, then a text id or 16 raw bytes (`FUN_14087ac50`).
    FlagId,
    /// A packed quaternion (`FUN_14087b3f0`).
    Quat,
    /// A packed turn (`FUN_14087aa70`).
    PackedTurn,
    /// A packed scale (`FUN_14087b250`).
    Scale,
    /// These forms in order.
    Seq(&'static [Wire]),
    /// A prefix-coded count and that many of a form.
    Array(&'static Wire),
    /// A replicated list or map: its delta value and key, and its full form's entry.
    List {
        /// The value of a delta entry.
        value: &'static Wire,
        /// The key of a delta entry.
        key: &'static Wire,
        /// One entry of the full form.
        full: &'static Wire,
    },
}

/// One child of a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Child {
    /// The field's registered name.
    pub(crate) name: &'static str,
    /// Its wire form.
    pub(crate) wire: Wire,
}

/// The layout of one class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassLayout {
    /// The class type index.
    pub(crate) class: u64,
    /// Its reflection name.
    pub(crate) name: &'static str,
    /// Its slots, each with its children in registration order.
    pub(crate) slots: &'static [&'static [Child]],
}

/// The layout of class `class`, when the table has it.
pub(crate) fn layout(class: u64) -> Option<&'static ClassLayout> {
    let at = table::LAYOUTS
        .binary_search_by_key(&class, |layout| layout.class)
        .ok()?;
    table::LAYOUTS.get(at)
}

/// Reads past one body of `layout`.
///
/// # Errors
///
/// Returns [`MessageError::Unsized`] for a slot the class does not have or a slot without
/// children, and another [`MessageError`] when the bytes do not fit the layout.
pub(crate) fn skip_body(layout: &ClassLayout, reader: &mut Reader<'_>) -> Result<(), MessageError> {
    let slots = u8::try_from(layout.slots.len()).map_err(|_| unsized_by(u8::MAX))?;
    let byte = crate::slots::read_slot_byte(reader, FIELD, slots)?;
    for (slot, children) in layout.slots.iter().enumerate() {
        if byte & (1 << slot) == 0 {
            continue;
        }
        let count = u8::try_from(children.len())
            .ok()
            .filter(|count| *count > 0)
            .ok_or_else(|| unsized_by(byte))?;
        read_children_64(reader, FIELD, count, |reader, child| {
            let child = children
                .get(usize::from(child))
                .ok_or_else(|| unsized_by(child))?;
            skip(child.wire, reader)
        })?;
    }
    Ok(())
}

const fn unsized_by(byte: u8) -> MessageError {
    MessageError::Unsized { field: FIELD, byte }
}

/// A count read before entries; a count beyond [`MAX_ENTRIES`] is a misread body.
fn count(reader: &mut Reader<'_>) -> Result<u64, MessageError> {
    let count = reader.prefixed()?;
    if count > MAX_ENTRIES {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_ENTRIES,
        });
    }
    Ok(count)
}

fn skip_version(reader: &mut Reader<'_>) -> Result<(), MessageError> {
    match reader.u8()? {
        VERSION_ABSENT => Ok(()),
        VERSION_PRESENT => reader.wide_prefixed().map(drop),
        value => Err(MessageError::NotBoolean { value }),
    }
}

fn skip_len(reader: &mut Reader<'_>, len: usize) -> Result<(), MessageError> {
    reader.bytes(len).map(drop)
}

/// Reads past one value of form `wire`.
fn skip(wire: Wire, reader: &mut Reader<'_>) -> Result<(), MessageError> {
    match wire {
        Wire::Fixed(len) => skip_len(reader, len),
        Wire::Prefixed => reader.prefixed().map(drop),
        Wire::Wide => reader.wide_prefixed().map(drop),
        Wire::Text => {
            let len = reader.prefixed()?;
            skip_len(reader, usize::try_from(len).unwrap_or(usize::MAX))
        }
        Wire::FlagId => {
            if reader.u8()? & FLAG_ID_RAW == 0 {
                skip(Wire::Text, reader)
            } else {
                skip_len(reader, FLAG_ID_RAW_LEN)
            }
        }
        Wire::Quat => {
            let flag = reader.u8()?;
            let parts = QUAT_PARTS
                .iter()
                .filter(|(zero, one)| flag & (zero | one) == 0)
                .count();
            skip_len(reader, parts * QUAT_PART_LEN)
        }
        Wire::PackedTurn => {
            let flag = reader.u8()?;
            let mut len = if flag & TURN_SHORT_BIT == 0 {
                TURN_FULL_LEN
            } else {
                TURN_FULL_LEN - 1
            };
            len -= TURN_DROP_BITS
                .iter()
                .filter(|bit| flag & **bit != 0)
                .count();
            if flag & TURN_LAST_DROP_BIT != 0 {
                len = len.saturating_sub(1);
            }
            skip_len(reader, len)
        }
        Wire::Scale => match reader.u8()? {
            SCALE_UNIT => Ok(()),
            SCALE_UNIFORM => skip_len(reader, HALF_LEN),
            _ => skip_len(reader, AXES * HALF_LEN),
        },
        Wire::Seq(parts) => parts.iter().try_for_each(|part| skip(*part, reader)),
        Wire::Array(element) => {
            for _ in 0..count(reader)? {
                skip(*element, reader)?;
            }
            Ok(())
        }
        Wire::List { value, key, full } => {
            let entries = count(reader)?;
            if entries == 0 {
                skip_version(reader)?;
                for _ in 0..count(reader)? {
                    skip(*full, reader)?;
                }
                return Ok(());
            }
            let mut left = entries;
            while left > 0 {
                let group = left.min(DELTA_GROUP);
                let mask = reader.u8()?;
                for entry in 0..group {
                    skip(*key, reader)?;
                    skip_version(reader)?;
                    if mask & (1 << entry) != 0 {
                        skip(*value, reader)?;
                    }
                }
                left -= group;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Writer;

    /// `MB::TransformLinkComponentReplicatedState` (5980): one child, `m_childGDEVector`.
    const TRANSFORM_LINK: u64 = 5980;

    #[test]
    fn the_table_is_sorted_by_class_and_every_slot_fits_a_slot_byte() {
        assert!(
            table::LAYOUTS
                .windows(2)
                .all(|pair| pair[0].class < pair[1].class)
        );
        assert!(table::LAYOUTS.iter().all(|layout| layout.slots.len() <= 8));
        assert!(
            table::LAYOUTS
                .iter()
                .flat_map(|layout| layout.slots.iter())
                .all(|slot| slot.len() <= 64)
        );
    }

    #[test]
    fn a_body_of_a_class_without_a_reader_is_passed_to_its_end() {
        let layout = layout(TRANSFORM_LINK).expect("layout");
        assert_eq!(layout.name, "MB::TransformLinkComponentReplicatedState");
        let [children, _] = layout.slots else {
            panic!("two slots");
        };
        assert_eq!(children[0].name, "m_childGDEVector");
        let mut body = Writer::new();
        // Slot 0, child 0: the full form of an empty list (version 0, no entries), then a byte
        // of the next member.
        body.u8(1)
            .u8(1)
            .prefixed(0)
            .expect("count")
            .u8(0)
            .prefixed(0)
            .expect("count");
        body.u8(0xaa);
        let bytes = body.finish();
        let mut reader = Reader::new(&bytes);
        skip_body(layout, &mut reader).expect("skip");
        assert_eq!(reader.rest(), [0xaa]);
    }

    #[test]
    fn a_slot_the_class_does_not_have_is_unsized() {
        let layout = layout(TRANSFORM_LINK).expect("layout");
        let mut reader = Reader::new(&[0x04]);
        assert!(matches!(
            skip_body(layout, &mut reader),
            Err(MessageError::Unsized { .. })
        ));
    }

    #[test]
    fn the_packed_forms_read_the_bytes_their_flags_give() {
        let cases: [(Wire, &[u8], usize); 8] = [
            // A quaternion with its first part zero: two parts of 2 bytes.
            (Wire::Quat, &[0x01, 1, 2, 3, 4, 9], 5),
            (Wire::PackedTurn, &[0x00, 1, 2, 3, 9], 4),
            // Bit 1 and bit 2: one byte.
            (Wire::PackedTurn, &[0x06, 1, 9], 2),
            (Wire::Scale, &[0, 9], 1),
            (Wire::Scale, &[1, 1, 2, 9], 3),
            (Wire::Scale, &[2, 1, 2, 3, 4, 5, 6, 9], 7),
            (
                Wire::FlagId,
                &[1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 9],
                17,
            ),
            // A text id: length 2, then two bytes.
            (Wire::FlagId, &[0, 2, 7, 7, 9], 4),
        ];
        for (wire, bytes, len) in cases {
            let mut reader = Reader::new(bytes);
            skip(wire, &mut reader).expect("skip");
            assert_eq!(bytes.len() - reader.remaining(), len, "{wire:?}");
        }
    }

    #[test]
    fn a_delta_list_reads_the_value_of_each_entry_its_mask_sets() {
        const VALUE: Wire = Wire::Fixed(4);
        let list = Wire::List {
            value: &VALUE,
            key: &Wire::Wide,
            full: &VALUE,
        };
        let mut body = Writer::new();
        // Two entries: the first with its value, the second without.
        body.prefixed(2).expect("count").u8(0b01);
        body.wide_prefixed(3).u8(0).u32(7);
        body.wide_prefixed(4).u8(1).wide_prefixed(9);
        body.u8(0xaa);
        let bytes = body.finish();
        let mut reader = Reader::new(&bytes);
        skip(list, &mut reader).expect("skip");
        assert_eq!(reader.rest(), [0xaa]);
    }
}
