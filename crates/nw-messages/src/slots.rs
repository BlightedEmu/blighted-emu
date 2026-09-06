// SPDX-License-Identifier: AGPL-3.0-or-later
//! The slot and child bytes of a bundle member body (mob-spawn note section 1): the reader
//! `FUN_1417b4110` reads one byte per group of up to 8 slots, and `FUN_1417b43c0` one byte per
//! group of up to 7 children of a slot, where bit 7 means that another child byte follows.
//!
//! These members use slot 0 only. A form that `write` would not produce (a slot other than 0, a
//! slot without children, a last child byte that sets no child) is refused, so every body that
//! reads encodes back to the same bytes. A child byte before the last may set none: that is how
//! only a later child is sent.

use crate::wire::{MessageError, Reader, Writer};

/// Slot byte bit: slot 0 follows.
pub(crate) const SLOT_0: u8 = 0x01;
/// Child byte bit: another child byte follows.
pub(crate) const CONTINUE: u8 = 0x80;
/// Children in one child byte.
const CHILDREN_PER_BYTE: u8 = 7;
/// The child bits of one child byte.
const CHILD_BITS: u8 = 0x7f;

/// Reads the slot byte and, when slot 0 is set, the child bitmap of `count` children (bit `n`
/// is child `n`). `None` when slot 0 is not set.
pub(crate) fn read_slot_0(
    reader: &mut Reader<'_>,
    field: &'static str,
    count: u8,
) -> Result<Option<u16>, MessageError> {
    let slot = reader.u8()?;
    if slot & !SLOT_0 != 0 {
        return Err(MessageError::OtherSlot { field, slots: slot });
    }
    if slot == 0 {
        return Ok(None);
    }
    let mut children: u16 = 0;
    let mut first: u8 = 0;
    loop {
        let byte = reader.u8()?;
        let bits = byte & CHILD_BITS;
        let last = byte & CONTINUE == 0;
        let in_group = count.saturating_sub(first).min(CHILDREN_PER_BYTE);
        let known: u8 = if in_group >= CHILDREN_PER_BYTE {
            CHILD_BITS
        } else {
            (1_u8 << in_group) - 1
        };
        let more_allowed = count > first + CHILDREN_PER_BYTE;
        if bits & !known != 0 || (bits == 0 && last) || (!last && !more_allowed) {
            return Err(MessageError::UnknownBits { field, bits: byte });
        }
        children |= u16::from(bits) << first;
        if last {
            return Ok(Some(children));
        }
        first += CHILDREN_PER_BYTE;
    }
}

/// Writes the slot byte and, for `Some`, the child bitmap of `count` children.
pub(crate) fn write_slot_0(writer: &mut Writer, children: Option<u16>, count: u8) {
    let Some(children) = children.filter(|children| *children != 0) else {
        writer.u8(0);
        return;
    };
    writer.u8(SLOT_0);
    let mut first: u8 = 0;
    while first < count {
        // A child byte holds 7 children: the shift leaves at most 7 bits.
        let bits = (children >> first) as u8 & CHILD_BITS;
        let rest = children >> (first + CHILDREN_PER_BYTE);
        writer.u8(if rest == 0 { bits } else { bits | CONTINUE });
        if rest == 0 {
            return;
        }
        first += CHILDREN_PER_BYTE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(present: bool, children: u16, count: u8) -> Vec<u8> {
        let mut writer = Writer::new();
        write_slot_0(&mut writer, present.then_some(children), count);
        writer.finish()
    }

    #[test]
    fn no_slot_is_one_zero_byte() {
        assert_eq!(round(false, 0, 2), vec![0]);
        let mut reader = Reader::new(&[0]);
        assert_eq!(read_slot_0(&mut reader, "test", 2).expect("read"), None);
    }

    #[test]
    fn children_up_to_seven_take_one_byte() {
        let bytes = round(true, 0b11, 2);
        assert_eq!(bytes, vec![SLOT_0, 0b11]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            read_slot_0(&mut reader, "test", 2).expect("read"),
            Some(0b11)
        );
    }

    #[test]
    fn the_eighth_child_takes_a_second_byte() {
        let bytes = round(true, 1 << 7 | 1, 8);
        assert_eq!(bytes, vec![SLOT_0, 1 | CONTINUE, 1]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            read_slot_0(&mut reader, "test", 8).expect("read"),
            Some(1 << 7 | 1)
        );
    }

    #[test]
    fn only_the_eighth_child_is_an_empty_first_byte_then_one() {
        let bytes = round(true, 1 << 7, 8);
        assert_eq!(bytes, vec![SLOT_0, CONTINUE, 1]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            read_slot_0(&mut reader, "test", 8).expect("read"),
            Some(1 << 7)
        );
    }

    #[test]
    fn unknown_slots_children_and_empty_forms_are_refused() {
        let cases: [(&[u8], u8); 6] = [
            (&[0b10], 2),
            (&[SLOT_0, 0], 2),
            (&[SLOT_0, 0b100], 2),
            (&[SLOT_0, 1 | CONTINUE, 1], 2),
            (&[SLOT_0, CONTINUE, 0], 8),
            (&[SLOT_0, CONTINUE, 0b10], 8),
        ];
        for (bytes, count) in cases {
            let mut reader = Reader::new(bytes);
            assert!(
                read_slot_0(&mut reader, "test", count).is_err(),
                "{bytes:?} with {count}"
            );
        }
    }
}
