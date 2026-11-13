// SPDX-License-Identifier: AGPL-3.0-or-later
//! A replicated map, or a list keyed by its index, whose delta form may remove entries: the
//! status effect maps (type 4236), the war data (1739), the siege warfare data (4103) and the
//! territory interactor's sign-ups (1880), and the resources committed to a buildable (1594,
//! keyed by the item id text: retail `12a_msq_12_camptutorial` 290.886 s added `FlintT1` with
//! version 14 in this delta form).
//!
//! The full form is the version and every (key, value) entry ([`FullList`]). The delta form is a
//! count above 0, then groups of up to 8 entries. Each group starts with a mask byte: bit `i` set
//! means that entry `i` of the group carries a value. Each entry is its key, a version flag byte
//! (with the version when the flag is 1), and the value when its mask bit is set; an entry
//! without its mask bit removes the key (Observed: 8 removals in the 5,917 retail war data
//! bodies of the capture index).

use crate::items::{FULL_FORM, FullList, VERSION_PRESENT};
use crate::list_delta::DELTA_GROUP;
use crate::wire::{MessageError, Reader, Writer};

/// The most delta entries of one map child.
const MAX_CHANGES: u64 = 16_383;
/// The version flag byte of an entry without a version.
const VERSION_ABSENT: u8 = 0;

/// The wire form of a map key.
pub trait MapKey: Clone {
    /// Reads a key.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early.
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError>;
    /// Writes the key.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a text key too long for its prefix.
    fn write(&self, writer: &mut Writer) -> Result<(), MessageError>;
}

impl MapKey for u16 {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        reader.u16()
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.u16(*self);
        Ok(())
    }
}

impl MapKey for u32 {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        reader.u32()
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.u32(*self);
        Ok(())
    }
}

impl MapKey for u64 {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        reader.u64()
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.u64(*self);
        Ok(())
    }
}

/// The key of a list in the keyed delta form: the element's index as a wide prefix-coded
/// integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ListIndex(pub u64);

impl MapKey for ListIndex {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        reader.wide_prefixed().map(Self)
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.wide_prefixed(self.0);
        Ok(())
    }
}

/// One delta entry of a map: the key, its version (`None`: flag 0), and its new value
/// (`None`: removed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapChange<K, V> {
    /// The key.
    pub key: K,
    /// The version; retail sends it on the first entry and 0 on the others.
    pub version: Option<u64>,
    /// The new value; `None` removes the entry.
    pub value: Option<V>,
}

/// A map child in its full form or as changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyedList<K, V> {
    /// The whole map: (key, value) entries.
    Full(FullList<(K, V)>),
    /// Changed, added and removed entries.
    Changes(Vec<MapChange<K, V>>),
}

impl<K: MapKey, V> KeyedList<K, V> {
    /// The whole map of `entries` at `version`.
    pub const fn full(version: u64, entries: Vec<(K, V)>) -> Self {
        Self::Full(FullList {
            version: Some(version),
            elements: entries,
        })
    }

    /// Reads a map in either form; `read_value` reads one value.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, holds too many changes, or a value
    /// does not read.
    pub fn read(
        reader: &mut Reader<'_>,
        mut read_value: impl FnMut(&mut Reader<'_>) -> Result<V, MessageError>,
    ) -> Result<Self, MessageError> {
        if Reader::new(reader.rest()).wide_prefixed()? == FULL_FORM {
            return FullList::read(reader, |reader| Ok((K::read(reader)?, read_value(reader)?)))
                .map(Self::Full);
        }
        read_changes(reader, read_value).map(Self::Changes)
    }

    /// Writes the map; `write_value` writes one value.
    ///
    /// # Errors
    ///
    /// Returns the error of a value.
    pub fn write(
        &self,
        writer: &mut Writer,
        mut write_value: impl FnMut(&mut Writer, &V) -> Result<(), MessageError>,
    ) -> Result<(), MessageError> {
        match self {
            Self::Full(list) => list.write(writer, |writer, (key, value)| {
                key.write(writer)?;
                write_value(writer, value)
            }),
            Self::Changes(changes) => write_changes(writer, changes, write_value),
        }
    }
}

/// A list child whose full form holds the elements alone and whose delta form is keyed by the
/// element's index ([`ListIndex`]), with removals (the war data's wars, 1739 child 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexedList<V> {
    /// The whole list.
    Full(FullList<V>),
    /// Changed, added and removed elements.
    Changes(Vec<MapChange<ListIndex, V>>),
}

impl<V> IndexedList<V> {
    /// Reads a list in either form; `read_value` reads one element.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, holds too many changes, or an element
    /// does not read.
    pub fn read(
        reader: &mut Reader<'_>,
        read_value: impl FnMut(&mut Reader<'_>) -> Result<V, MessageError>,
    ) -> Result<Self, MessageError> {
        if Reader::new(reader.rest()).wide_prefixed()? == FULL_FORM {
            return FullList::read(reader, read_value).map(Self::Full);
        }
        read_changes(reader, read_value).map(Self::Changes)
    }

    /// Writes the list; `write_value` writes one element.
    ///
    /// # Errors
    ///
    /// Returns the error of an element.
    pub fn write(
        &self,
        writer: &mut Writer,
        write_value: impl FnMut(&mut Writer, &V) -> Result<(), MessageError>,
    ) -> Result<(), MessageError> {
        match self {
            Self::Full(list) => list.write(writer, write_value),
            Self::Changes(changes) => write_changes(writer, changes, write_value),
        }
    }
}

/// Reads the delta form (module doc).
fn read_changes<K: MapKey, V>(
    reader: &mut Reader<'_>,
    mut read_value: impl FnMut(&mut Reader<'_>) -> Result<V, MessageError>,
) -> Result<Vec<MapChange<K, V>>, MessageError> {
    let count = reader.wide_prefixed()?;
    if count > MAX_CHANGES {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_CHANGES,
        });
    }
    // At most `MAX_CHANGES` entries.
    let mut left = count as usize;
    let mut changes = Vec::with_capacity(left);
    while left > 0 {
        let mask = reader.u8()?;
        let in_group = left.min(DELTA_GROUP);
        for entry in 0..in_group {
            let key = K::read(reader)?;
            let version = read_version(reader)?;
            let value = if mask & (1 << entry) != 0 {
                Some(read_value(reader)?)
            } else {
                None
            };
            changes.push(MapChange {
                key,
                version,
                value,
            });
        }
        left -= in_group;
    }
    Ok(changes)
}

/// Writes the delta form (module doc).
fn write_changes<K: MapKey, V>(
    writer: &mut Writer,
    changes: &[MapChange<K, V>],
    mut write_value: impl FnMut(&mut Writer, &V) -> Result<(), MessageError>,
) -> Result<(), MessageError> {
    writer.wide_prefixed(changes.len() as u64);
    for group in changes.chunks(DELTA_GROUP) {
        let mask = group
            .iter()
            .enumerate()
            .filter(|(_, change)| change.value.is_some())
            .fold(0_u8, |mask, (entry, _)| mask | (1 << entry));
        writer.u8(mask);
        for change in group {
            change.key.write(writer)?;
            match change.version {
                Some(version) => writer.u8(VERSION_PRESENT).wide_prefixed(version),
                None => writer.u8(VERSION_ABSENT),
            };
            if let Some(value) = &change.value {
                write_value(writer, value)?;
            }
        }
    }
    Ok(())
}

/// Reads a version: the flag byte, then the version when the flag is 1.
fn read_version(reader: &mut Reader<'_>) -> Result<Option<u64>, MessageError> {
    match reader.u8()? {
        VERSION_ABSENT => Ok(None),
        VERSION_PRESENT => Ok(Some(reader.wide_prefixed()?)),
        value => Err(MessageError::NotBoolean { value }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(map: &KeyedList<u16, u8>) -> Vec<u8> {
        let mut writer = Writer::new();
        map.write(&mut writer, |writer, value| {
            writer.u8(*value);
            Ok(())
        })
        .expect("write");
        writer.finish()
    }

    fn decode(bytes: &[u8]) -> KeyedList<u16, u8> {
        let mut reader = Reader::new(bytes);
        let map = KeyedList::read(&mut reader, |reader| reader.u8()).expect("read");
        reader.finish().expect("used up");
        map
    }

    #[test]
    fn a_full_map_and_a_delta_with_a_removal_write_back() {
        let full = KeyedList::full(7, vec![(10_u16, 1_u8), (11, 2)]);
        assert_eq!(decode(&encode(&full)), full);
        let changes = KeyedList::Changes(vec![
            MapChange {
                key: 10,
                version: Some(8),
                value: Some(3),
            },
            MapChange {
                key: 11,
                version: None,
                value: None,
            },
        ]);
        let bytes = encode(&changes);
        // Two entries, the mask of the first alone, key 10, version 8, value 3, key 11, no
        // version, no value.
        assert_eq!(bytes, [2, 0b01, 0, 10, 1, 8, 3, 0, 11, 0]);
        assert_eq!(decode(&bytes), changes);
    }
}
