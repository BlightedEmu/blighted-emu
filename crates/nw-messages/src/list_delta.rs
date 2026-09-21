// SPDX-License-Identifier: AGPL-3.0-or-later
//! The delta form of the replicated list that the achievements (type 3210), the categorical
//! progression (type 911) and the progression points (type 3681) share (tutorial-intro note,
//! section 2; progression note, section 1).
//!
//! A delta is a count above 0, then groups of up to 8 entries. Each group starts with a mask
//! byte: bit `i` set means that entry `i` of the group carries a value. Each entry is the list
//! index, a version flag byte, a version and the value. The entries of one child share one
//! version: retail sends it on the first entry and version 0 on the others. An entry without its
//! mask bit removes the element; no retail body of these classes has one, so it is
//! [`MessageError::Unsized`].

use crate::items::{FULL_FORM, FullList, VERSION_PRESENT, read_number};
use crate::wire::{MessageError, Reader, Writer};

/// Entries per delta group.
const DELTA_GROUP: usize = 8;

/// One changed or appended element of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListChange<T> {
    /// The list index; an index equal to the old length appends the element.
    pub index: u32,
    /// The version; retail sends it on a child's first entry and 0 on the others.
    pub version: u64,
    /// The new element.
    pub value: T,
}

/// A list child in its full form or as changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplicatedList<T> {
    /// The whole list.
    Full(FullList<T>),
    /// Changed or appended elements.
    Changes(Vec<ListChange<T>>),
}

impl<T> ReplicatedList<T> {
    /// Reads a list in either form; `read_element` reads one element.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a delta entry removes an element, or
    /// an element does not read.
    pub fn read(
        reader: &mut Reader<'_>,
        field: &'static str,
        read_element: impl FnMut(&mut Reader<'_>) -> Result<T, MessageError>,
    ) -> Result<Self, MessageError> {
        if Reader::new(reader.rest()).wide_prefixed()? == FULL_FORM {
            FullList::read(reader, read_element).map(Self::Full)
        } else {
            read_changes(reader, field, read_element).map(Self::Changes)
        }
    }

    /// Writes the list; `write_element` writes one element.
    ///
    /// # Errors
    ///
    /// Returns the error of an element.
    pub fn write(
        &self,
        writer: &mut Writer,
        write_element: impl FnMut(&mut Writer, &T) -> Result<(), MessageError>,
    ) -> Result<(), MessageError> {
        match self {
            Self::Full(list) => list.write(writer, write_element),
            Self::Changes(changes) => write_changes(writer, changes, write_element),
        }
    }
}

/// The changes of one child from `(index, value)` pairs in send order: the first carries
/// `version`, the others version 0 (retail). `None` when there are no changes.
pub(crate) fn child_changes<T>(
    version: u64,
    changes: impl IntoIterator<Item = (u32, T)>,
) -> Option<ReplicatedList<T>> {
    let changes: Vec<ListChange<T>> = changes
        .into_iter()
        .enumerate()
        .map(|(position, (index, value))| ListChange {
            index,
            version: if position == 0 { version } else { 0 },
            value,
        })
        .collect();
    (!changes.is_empty()).then_some(ReplicatedList::Changes(changes))
}

/// One child list in its full form with `version`: `value` of each of `items`.
pub(crate) fn full_child<S, T>(
    version: u64,
    items: &[S],
    value: impl FnMut(&S) -> T,
) -> Option<ReplicatedList<T>> {
    Some(ReplicatedList::Full(FullList {
        version: Some(version),
        elements: items.iter().map(value).collect(),
    }))
}

/// Writes a child list when it is present; `write_element` writes one element.
pub(crate) fn write_child<T>(
    writer: &mut Writer,
    list: Option<&ReplicatedList<T>>,
    mut write_element: impl FnMut(&mut Writer, &T),
) -> Result<(), MessageError> {
    list.map_or(Ok(()), |list| {
        list.write(writer, |writer, element| {
            write_element(writer, element);
            Ok(())
        })
    })
}

/// Reads a delta form.
fn read_changes<T>(
    reader: &mut Reader<'_>,
    field: &'static str,
    mut read_element: impl FnMut(&mut Reader<'_>) -> Result<T, MessageError>,
) -> Result<Vec<ListChange<T>>, MessageError> {
    let count = reader.wide_prefixed()?;
    // Each entry takes at least three bytes.
    if count > reader.remaining() as u64 {
        return Err(MessageError::LengthMismatch { field });
    }
    // At most the bytes left.
    let mut left = count as usize;
    let mut changes = Vec::with_capacity(left);
    while left > 0 {
        let mask = reader.u8()?;
        let group = left.min(DELTA_GROUP);
        for entry in 0..group {
            let index = read_number(reader)?;
            if reader.u8()? != VERSION_PRESENT || mask & 1 << entry == 0 {
                return Err(MessageError::Unsized { field, byte: mask });
            }
            let version = reader.wide_prefixed()?;
            changes.push(ListChange {
                index,
                version,
                value: read_element(reader)?,
            });
        }
        left -= group;
    }
    Ok(changes)
}

/// Writes a delta form.
pub(crate) fn write_changes<T>(
    writer: &mut Writer,
    changes: &[ListChange<T>],
    mut write_element: impl FnMut(&mut Writer, &T) -> Result<(), MessageError>,
) -> Result<(), MessageError> {
    // A `usize` count fits 64 bits.
    writer.wide_prefixed(changes.len() as u64);
    for group in changes.chunks(DELTA_GROUP) {
        // Every entry of a group carries a value; a group has at most 8 entries.
        writer.u8(u8::MAX >> (DELTA_GROUP - group.len()));
        for change in group {
            writer
                .wide_prefixed(u64::from(change.index))
                .u8(VERSION_PRESENT)
                .wide_prefixed(change.version);
            write_element(writer, &change.value)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(list: &ReplicatedList<u16>) -> Vec<u8> {
        let mut writer = Writer::new();
        list.write(&mut writer, |writer, value| {
            writer.u16(*value);
            Ok(())
        })
        .expect("write");
        writer.finish()
    }

    fn decode(bytes: &[u8]) -> Result<ReplicatedList<u16>, MessageError> {
        let mut reader = Reader::new(bytes);
        let list = ReplicatedList::read(&mut reader, "test", |reader| reader.u16())?;
        reader.finish()?;
        Ok(list)
    }

    #[test]
    fn both_forms_round_trip() {
        let full = ReplicatedList::Full(FullList::first(vec![50_u16, 1]));
        assert_eq!(encode(&full)[..4], [0x00, 0x01, 0x03, 0x02]);
        assert_eq!(decode(&encode(&full)), Ok(full));
        let changes = child_changes(146, [(5_u32, 1_u16), (6, 0)]).expect("changes");
        let bytes = encode(&changes);
        assert_eq!(
            bytes,
            [
                0x02, 0x03, 0x05, 0x01, 0x92, 0x02, 0x00, 0x01, 0x06, 0x01, 0x00, 0x00, 0x00
            ]
        );
        assert_eq!(decode(&bytes), Ok(changes));
        assert_eq!(child_changes::<u16>(1, []), None);
    }

    #[test]
    fn nine_changes_take_two_groups_and_a_removal_is_unsized() {
        let changes = child_changes(1, (0..9).map(|index| (index, 7_u16))).expect("changes");
        let bytes = encode(&changes);
        assert_eq!(bytes[..2], [9, 0xff]);
        assert_eq!(decode(&bytes), Ok(changes));
        // One entry, mask bit 0 clear: a removal.
        assert!(matches!(
            decode(&[0x01, 0x00, 0x00, 0x01, 0x01]),
            Err(MessageError::Unsized { .. })
        ));
    }
}
