// SPDX-License-Identifier: AGPL-3.0-or-later
//! The field codecs the social calls share: text, character references, lists and request ids.
//!
//! Text is a prefix-coded length and that many bytes (`FUN_14087a3d0`, a length below
//! `0x30000`). Counts are prefix-coded as well.

use crate::combat::{read_string, write_string};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// The tag of a character reference that holds a character id as 16 bytes (every retail 5283,
/// 6453 and 6675 entry, and the set references of 1677 and 3604).
const CHARACTER_TAG_ID: u8 = 5;
/// The tag of an empty character reference: an empty text follows (the unset references of the
/// retail 1677 bodies).
const CHARACTER_TAG_NONE: u8 = 0;
/// The most entries of a list in one social body: a body is at most `0x30000` bytes and an
/// entry at least one byte.
const MAX_LIST: u64 = 0x2_FFFF;

/// Reads a text. Bytes that are not UTF-8 become U+FFFD: a name or chat line with them still
/// reads.
pub(crate) fn read_text(reader: &mut Reader<'_>) -> Result<String, MessageError> {
    Ok(String::from_utf8_lossy(&read_string(reader)?).into_owned())
}

/// Writes a text.
pub(crate) fn write_text(writer: &mut Writer, text: &str) -> Result<(), MessageError> {
    write_string(writer, text.as_bytes())
}

/// Reads a list count, at most [`MAX_LIST`].
pub(crate) fn read_count(reader: &mut Reader<'_>) -> Result<usize, MessageError> {
    let count = reader.prefixed()?;
    if count > MAX_LIST || count > reader.remaining() as u64 {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_LIST.min(reader.remaining() as u64),
        });
    }
    // `count` is at most the bytes left.
    Ok(count as usize)
}

/// Writes a list count.
pub(crate) fn write_count(writer: &mut Writer, count: usize) -> Result<(), MessageError> {
    writer.prefixed(count as u64)?;
    Ok(())
}

/// A character reference: the character id as 16 bytes (the UUID in text order), or none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CharacterRef(pub Option<[u8; RAW_ID_LEN]>);

impl CharacterRef {
    /// Reads a reference: tag 5 and 16 bytes, or tag 0 and an empty text.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::BadTag`] for another tag, and for tag 0 with a text that is not
    /// empty.
    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        match reader.u8()? {
            CHARACTER_TAG_ID => Ok(Self(Some(reader.raw_id()?))),
            CHARACTER_TAG_NONE if read_string(reader)?.is_empty() => Ok(Self(None)),
            tag => Err(MessageError::BadTag {
                field: "character reference",
                tag,
            }),
        }
    }

    /// Writes the reference.
    pub(crate) fn write(&self, writer: &mut Writer) {
        match self.0 {
            Some(id) => {
                writer.u8(CHARACTER_TAG_ID).bytes(&id);
            }
            None => {
                writer.u8(CHARACTER_TAG_NONE).u8(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: [u8; RAW_ID_LEN] = [7; RAW_ID_LEN];

    #[test]
    fn a_set_reference_is_tag_5_and_16_bytes() {
        let mut writer = Writer::new();
        CharacterRef(Some(ID)).write(&mut writer);
        let bytes = writer.finish();
        assert_eq!(bytes.len(), 1 + RAW_ID_LEN);
        assert_eq!(bytes[0], CHARACTER_TAG_ID);
        assert_eq!(
            CharacterRef::read(&mut Reader::new(&bytes)),
            Ok(CharacterRef(Some(ID)))
        );
    }

    #[test]
    fn an_empty_reference_is_two_zero_bytes() {
        let mut writer = Writer::new();
        CharacterRef(None).write(&mut writer);
        assert_eq!(writer.finish(), [0, 0]);
        assert_eq!(
            CharacterRef::read(&mut Reader::new(&[0, 0])),
            Ok(CharacterRef(None))
        );
        assert!(CharacterRef::read(&mut Reader::new(&[0, 1, b'x'])).is_err());
        assert!(CharacterRef::read(&mut Reader::new(&[3])).is_err());
    }

    #[test]
    fn text_that_is_not_utf8_still_reads() {
        let bytes = [2, 0x66, 0xff];
        assert_eq!(
            read_text(&mut Reader::new(&bytes)),
            Ok("f\u{fffd}".to_owned())
        );
    }

    #[test]
    fn a_count_above_the_bytes_left_is_refused() {
        assert!(read_count(&mut Reader::new(&[5, 1])).is_err());
    }
}
