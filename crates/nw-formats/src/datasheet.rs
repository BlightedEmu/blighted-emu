// SPDX-License-Identifier: MIT OR Apache-2.0
//! Datasheets (`.datasheet`): binary tables of game rules.
//!
//! Layout (little-endian), observed on all 2,250 files of the pinned client:
//! - header: 15 × `u32` (60 bytes); index 2 = unique id offset, 4 = type name offset,
//!   14 = body offset;
//! - meta: 32 bytes; byte 8 = column count (`u32`), byte 12 = row count (`u32`);
//! - column index: 12 bytes per column = name CRC32, name offset, kind code;
//! - cell index: 8 bytes per cell, row by row = text offset + 4 value bytes (not read);
//! - strings: NUL-terminated UTF-8. Every offset is relative to `HEADER_SIZE + body offset`.
//!
//! Many cells can point at the same string. Each string is decoded once, and the total text of
//! the parsed sheet is limited to [`MAX_DECODED_TEXT_BYTES`].

use std::collections::HashMap;
use std::collections::hash_map::Entry;

/// File name ending of datasheets.
pub const DATASHEET_EXTENSION: &str = ".datasheet";

const U32_SIZE: usize = 4;
const HEADER_SIZE: usize = 60;
const HEADER_UNIQUE_ID_OFFSET_INDEX: usize = 2;
const HEADER_TYPE_OFFSET_INDEX: usize = 4;
const HEADER_BODY_OFFSET_INDEX: usize = 14;
const META_SIZE: usize = 32;
const META_COLUMN_COUNT_OFFSET: usize = 8;
const META_ROW_COUNT_OFFSET: usize = 12;
const COLUMN_ENTRY_SIZE: usize = 12;
const COLUMN_NAME_OFFSET: usize = 4;
const COLUMN_KIND_OFFSET: usize = 8;
const CELL_ENTRY_SIZE: usize = 8;
const STRING_TERMINATOR: u8 = 0;
/// Smallest string: the terminator alone.
const MIN_STRING_SIZE: usize = 1;
/// Largest total of decoded text (all names and cells, each cell counted) in one datasheet.
/// The largest datasheet of the pinned client decodes to 5,158,304 bytes of text
/// (`javelindata_itemappearancedefinitions`); this limit is about 13 times that.
pub const MAX_DECODED_TEXT_BYTES: usize = 64 << 20;

/// Declared kind of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    /// Text.
    String,
    /// Number (whole or decimal).
    Number,
    /// Boolean (`1`, `0`, `TRUE`, `FALSE`).
    Boolean,
}

impl ColumnKind {
    /// Code of a text column.
    pub const STRING_CODE: u32 = 1;
    /// Code of a number column.
    pub const NUMBER_CODE: u32 = 2;
    /// Code of a boolean column.
    pub const BOOLEAN_CODE: u32 = 3;

    /// The kind for a column code, or `None` for an unknown code.
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            Self::STRING_CODE => Some(Self::String),
            Self::NUMBER_CODE => Some(Self::Number),
            Self::BOOLEAN_CODE => Some(Self::Boolean),
            _ => None,
        }
    }
}

/// One column of a datasheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// Column name as stored in the file.
    pub name: String,
    /// Declared kind.
    pub kind: ColumnKind,
}

/// A parsed datasheet. Cell values are kept as their stored text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datasheet {
    /// Table type name, for example `MasterItemDefinitions`.
    pub type_name: String,
    /// Unique id of this file.
    pub unique_id: String,
    /// Columns in file order.
    pub columns: Vec<Column>,
    /// Rows; each row has one text value per column. An empty cell is an empty string.
    pub rows: Vec<Vec<String>>,
}

/// Error from datasheet parsing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DatasheetError {
    /// A read goes past the end of the data.
    #[error("datasheet is truncated: {needed} bytes needed at offset {offset}, file has {len}")]
    Truncated {
        /// Start of the read.
        offset: usize,
        /// Bytes needed.
        needed: usize,
        /// File length.
        len: usize,
    },
    /// Row count × column count does not fit in memory sizes.
    #[error("datasheet has an impossible size: {rows} rows × {columns} columns")]
    TooLarge {
        /// Row count.
        rows: usize,
        /// Column count.
        columns: usize,
    },
    /// Rows are declared, but the datasheet has no columns.
    #[error("datasheet has {rows} rows but no columns")]
    RowsWithoutColumns {
        /// Row count.
        rows: usize,
    },
    /// A string has no NUL terminator.
    #[error("string at offset {offset} has no NUL terminator")]
    UnterminatedString {
        /// String start.
        offset: usize,
    },
    /// A string is not UTF-8.
    #[error("string at offset {offset} is not valid UTF-8")]
    InvalidUtf8 {
        /// String start.
        offset: usize,
    },
    /// A column kind code is not 1, 2 or 3.
    #[error("column `{column}` has unknown kind code {code}")]
    UnknownColumnKind {
        /// Column name.
        column: String,
        /// The code.
        code: u32,
    },
    /// The decoded text is larger than [`MAX_DECODED_TEXT_BYTES`].
    #[error("datasheet text is larger than the limit of {limit} bytes")]
    TextTooLarge {
        /// The limit.
        limit: usize,
    },
}

impl Datasheet {
    /// Parses a decoded `.datasheet` file.
    pub fn parse(bytes: &[u8]) -> Result<Self, DatasheetError> {
        let reader = ByteReader { bytes };
        let header = |index: usize| reader.u32_at(index * U32_SIZE);
        let mut strings = StringTable {
            reader: &reader,
            base: HEADER_SIZE + header(HEADER_BODY_OFFSET_INDEX)? as usize,
            decoded: HashMap::new(),
            total_bytes: 0,
        };
        let column_count = reader.u32_at(HEADER_SIZE + META_COLUMN_COUNT_OFFSET)? as usize;
        let row_count = reader.u32_at(HEADER_SIZE + META_ROW_COUNT_OFFSET)? as usize;
        // With no columns the cell index is empty, so the index check below cannot bound the rows.
        if column_count == 0 && row_count > 0 {
            return Err(DatasheetError::RowsWithoutColumns { rows: row_count });
        }
        let column_index = HEADER_SIZE + META_SIZE;
        let cell_index = column_index + column_count * COLUMN_ENTRY_SIZE;
        let too_large = DatasheetError::TooLarge {
            rows: row_count,
            columns: column_count,
        };
        let index_end = row_count
            .checked_mul(column_count)
            .and_then(|cells| cells.checked_mul(CELL_ENTRY_SIZE))
            .and_then(|size| size.checked_add(cell_index))
            .ok_or(too_large)?;
        // Check the whole index before allocating. With columns, every row takes at least one
        // cell entry, so the file length bounds the row and column allocations.
        if index_end > bytes.len() {
            return Err(DatasheetError::Truncated {
                offset: column_index,
                needed: index_end - column_index,
                len: bytes.len(),
            });
        }
        let type_name = strings.text(header(HEADER_TYPE_OFFSET_INDEX)?)?;
        let unique_id = strings.text(header(HEADER_UNIQUE_ID_OFFSET_INDEX)?)?;
        let columns = (0..column_count)
            .map(|column| {
                let entry = column_index + column * COLUMN_ENTRY_SIZE;
                let name = strings.text(reader.u32_at(entry + COLUMN_NAME_OFFSET)?)?;
                let code = reader.u32_at(entry + COLUMN_KIND_OFFSET)?;
                match ColumnKind::from_code(code) {
                    Some(kind) => Ok(Column { name, kind }),
                    None => Err(DatasheetError::UnknownColumnKind { column: name, code }),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rows = (0..row_count)
            .map(|row| {
                (0..column_count)
                    .map(|column| {
                        let entry = cell_index + (row * column_count + column) * CELL_ENTRY_SIZE;
                        strings.text(reader.u32_at(entry)?)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            type_name,
            unique_id,
            columns,
            rows,
        })
    }
}

/// Decodes each string offset once and counts the text handed out, against
/// [`MAX_DECODED_TEXT_BYTES`].
struct StringTable<'a> {
    reader: &'a ByteReader<'a>,
    /// Absolute position that string offsets are relative to.
    base: usize,
    /// Decoded strings by absolute position.
    decoded: HashMap<usize, String>,
    /// Bytes of text handed out so far.
    total_bytes: usize,
}

impl StringTable<'_> {
    fn text(&mut self, offset: u32) -> Result<String, DatasheetError> {
        let position = self.base + offset as usize;
        let text = match self.decoded.entry(position) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(self.reader.string_at(position)?),
        };
        self.total_bytes = self
            .total_bytes
            .checked_add(text.len())
            .filter(|&total| total <= MAX_DECODED_TEXT_BYTES)
            .ok_or(DatasheetError::TextTooLarge {
                limit: MAX_DECODED_TEXT_BYTES,
            })?;
        Ok(text.clone())
    }
}

/// Bounds-checked little-endian reads.
struct ByteReader<'a> {
    bytes: &'a [u8],
}

impl ByteReader<'_> {
    fn u32_at(&self, offset: usize) -> Result<u32, DatasheetError> {
        let truncated = DatasheetError::Truncated {
            offset,
            needed: U32_SIZE,
            len: self.bytes.len(),
        };
        let slice = offset
            .checked_add(U32_SIZE)
            .and_then(|end| self.bytes.get(offset..end))
            .ok_or(truncated)?;
        let mut word = [0u8; U32_SIZE];
        word.copy_from_slice(slice);
        Ok(u32::from_le_bytes(word))
    }

    fn string_at(&self, offset: usize) -> Result<String, DatasheetError> {
        let tail = self.bytes.get(offset..).ok_or(DatasheetError::Truncated {
            offset,
            needed: MIN_STRING_SIZE,
            len: self.bytes.len(),
        })?;
        let length = tail
            .iter()
            .position(|&byte| byte == STRING_TERMINATOR)
            .ok_or(DatasheetError::UnterminatedString { offset })?;
        std::str::from_utf8(&tail[..length])
            .map(str::to_owned)
            .map_err(|_| DatasheetError::InvalidUtf8 { offset })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGNATURE: u32 = 0x12;
    const HEADER_WORDS: usize = 15;
    const META_WORDS: usize = 8;
    const META_COLUMN_COUNT_WORD: usize = 2;
    const META_ROW_COUNT_WORD: usize = 3;
    const UNKNOWN_KIND: u32 = 7;
    const NOT_UTF8: u8 = 0xFF;
    const SHARED_ROWS: usize = 64;
    const SHARED_TEXT_LEN: usize = 1024;

    fn add_string(strings: &mut Vec<u8>, text: &str) -> u32 {
        let offset = u32::try_from(strings.len()).unwrap();
        strings.extend_from_slice(text.as_bytes());
        strings.push(STRING_TERMINATOR);
        offset
    }

    /// Writes a datasheet with the observed layout.
    fn write_sheet(
        type_name: &str,
        unique_id: &str,
        columns: &[(&str, u32)],
        rows: &[&[&str]],
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let type_offset = add_string(&mut strings, type_name);
        let unique_id_offset = add_string(&mut strings, unique_id);
        let mut column_entries = Vec::new();
        for (name, code) in columns {
            let name_offset = add_string(&mut strings, name);
            for word in [0, name_offset, *code] {
                column_entries.extend_from_slice(&word.to_le_bytes());
            }
        }
        let mut cells = Vec::new();
        for row in rows {
            for value in *row {
                let text_offset = add_string(&mut strings, value);
                for word in [text_offset, 0] {
                    cells.extend_from_slice(&word.to_le_bytes());
                }
            }
        }
        let body_offset = META_SIZE + column_entries.len() + cells.len();
        let mut header = [0u32; HEADER_WORDS];
        header[0] = SIGNATURE;
        header[HEADER_UNIQUE_ID_OFFSET_INDEX] = unique_id_offset;
        header[HEADER_TYPE_OFFSET_INDEX] = type_offset;
        header[HEADER_BODY_OFFSET_INDEX] = u32::try_from(body_offset).unwrap();
        let mut meta = [0u32; META_WORDS];
        meta[META_COLUMN_COUNT_WORD] = u32::try_from(columns.len()).unwrap();
        meta[META_ROW_COUNT_WORD] = u32::try_from(rows.len()).unwrap();
        let mut out = Vec::new();
        for word in header.iter().chain(meta.iter()) {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&column_entries);
        out.extend_from_slice(&cells);
        out.extend_from_slice(&strings);
        out
    }

    fn sample() -> Vec<u8> {
        write_sheet(
            "MasterItemDefinitions",
            "Items_1",
            &[
                ("ItemID", ColumnKind::STRING_CODE),
                ("Tier", ColumnKind::NUMBER_CODE),
                ("CanBeSold", ColumnKind::BOOLEAN_CODE),
            ],
            &[&["sword_t1", "1", "TRUE"], &["shield_t2", "", "0"]],
        )
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + U32_SIZE].copy_from_slice(&value.to_le_bytes());
    }

    /// A one-column sheet of `rows` cells that all point at the same text of `text_len` bytes.
    fn sheet_with_shared_text(rows: usize, text_len: usize) -> Vec<u8> {
        let shared = "x".repeat(text_len);
        let mut values = vec![""; rows];
        values[0] = &shared;
        let rows: Vec<&[&str]> = values.iter().map(std::slice::from_ref).collect();
        let mut bytes = write_sheet("T", "U", &[("A", ColumnKind::STRING_CODE)], &rows);
        let cell_index = HEADER_SIZE + META_SIZE + COLUMN_ENTRY_SIZE;
        let shared_offset =
            u32::from_le_bytes(bytes[cell_index..cell_index + U32_SIZE].try_into().unwrap());
        for row in 1..rows.len() {
            put_u32(
                &mut bytes,
                cell_index + row * CELL_ENTRY_SIZE,
                shared_offset,
            );
        }
        bytes
    }

    #[test]
    fn cells_sharing_one_text_parse() {
        let bytes = sheet_with_shared_text(SHARED_ROWS, SHARED_TEXT_LEN);
        let sheet = Datasheet::parse(&bytes).unwrap();
        assert_eq!(sheet.rows.len(), SHARED_ROWS);
        assert!(sheet.rows.iter().all(|row| row[0].len() == SHARED_TEXT_LEN));
    }

    #[test]
    fn shared_text_above_the_limit_is_rejected() {
        let bytes = sheet_with_shared_text(SHARED_ROWS, MAX_DECODED_TEXT_BYTES / SHARED_ROWS + 1);
        // `matches!` keeps the 64 MiB of parsed text out of the failure message.
        assert!(matches!(
            Datasheet::parse(&bytes),
            Err(DatasheetError::TextTooLarge {
                limit: MAX_DECODED_TEXT_BYTES
            })
        ));
    }

    #[test]
    fn parses_names_columns_and_rows() {
        let sheet = Datasheet::parse(&sample()).unwrap();
        assert_eq!(sheet.type_name, "MasterItemDefinitions");
        assert_eq!(sheet.unique_id, "Items_1");
        assert_eq!(
            sheet.columns,
            vec![
                Column {
                    name: "ItemID".to_owned(),
                    kind: ColumnKind::String
                },
                Column {
                    name: "Tier".to_owned(),
                    kind: ColumnKind::Number
                },
                Column {
                    name: "CanBeSold".to_owned(),
                    kind: ColumnKind::Boolean
                },
            ]
        );
        assert_eq!(
            sheet.rows,
            vec![vec!["sword_t1", "1", "TRUE"], vec!["shield_t2", "", "0"]]
        );
    }

    #[test]
    fn sheet_without_rows_parses() {
        let bytes = write_sheet("Empty", "E", &[("A", ColumnKind::STRING_CODE)], &[]);
        let sheet = Datasheet::parse(&bytes).unwrap();
        assert_eq!(sheet.columns.len(), 1);
        assert!(sheet.rows.is_empty());
    }

    #[test]
    fn every_truncation_is_an_error_not_a_panic() {
        let bytes = sample();
        for len in 0..bytes.len() {
            assert!(
                Datasheet::parse(&bytes[..len]).is_err(),
                "prefix of {len} bytes parsed"
            );
        }
    }

    #[test]
    fn unknown_column_kind_is_an_error() {
        let bytes = write_sheet("T", "U", &[("A", UNKNOWN_KIND)], &[]);
        assert_eq!(
            Datasheet::parse(&bytes),
            Err(DatasheetError::UnknownColumnKind {
                column: "A".to_owned(),
                code: UNKNOWN_KIND
            })
        );
    }

    #[test]
    fn huge_row_count_is_rejected() {
        let mut bytes = sample();
        put_u32(&mut bytes, HEADER_SIZE + META_ROW_COUNT_OFFSET, u32::MAX);
        assert!(matches!(
            Datasheet::parse(&bytes),
            Err(DatasheetError::Truncated { .. })
        ));
    }

    #[test]
    fn overflowing_cell_count_is_rejected() {
        let mut bytes = sample();
        put_u32(&mut bytes, HEADER_SIZE + META_COLUMN_COUNT_OFFSET, u32::MAX);
        put_u32(&mut bytes, HEADER_SIZE + META_ROW_COUNT_OFFSET, u32::MAX);
        assert!(matches!(
            Datasheet::parse(&bytes),
            Err(DatasheetError::TooLarge { .. })
        ));
    }

    #[test]
    fn rows_without_columns_are_rejected() {
        let mut bytes = write_sheet("T", "U", &[], &[]);
        put_u32(&mut bytes, HEADER_SIZE + META_ROW_COUNT_OFFSET, u32::MAX);
        assert!(matches!(
            Datasheet::parse(&bytes),
            Err(DatasheetError::RowsWithoutColumns { .. })
        ));
    }

    #[test]
    fn string_offset_outside_file_is_an_error() {
        let mut bytes = sample();
        put_u32(&mut bytes, HEADER_TYPE_OFFSET_INDEX * U32_SIZE, u32::MAX);
        assert!(matches!(
            Datasheet::parse(&bytes),
            Err(DatasheetError::Truncated { .. })
        ));
    }

    #[test]
    fn invalid_utf8_is_an_error() {
        let mut bytes = sample();
        let type_offset = HEADER_SIZE + META_SIZE + 3 * COLUMN_ENTRY_SIZE + 6 * CELL_ENTRY_SIZE;
        bytes[type_offset] = NOT_UTF8;
        assert!(matches!(
            Datasheet::parse(&bytes),
            Err(DatasheetError::InvalidUtf8 { .. })
        ));
    }
}

#[cfg(all(test, feature = "local-data"))]
mod local_data_tests {
    use super::*;
    use crate::local_data::*;
    use crate::pak::{DecompressError, Decompressor, EntryMethod, PakArchive, find_paks};

    /// Stored entries never call the decompressor.
    struct NoOodle;

    impl Decompressor for NoOodle {
        fn decompress(
            &self,
            _input: &[u8],
            _uncompressed_size: usize,
        ) -> Result<Vec<u8>, DecompressError> {
            Err(DecompressError(
                "nw-formats tests have no Oodle decoder".to_owned(),
            ))
        }
    }

    #[test]
    fn stored_datasheets_of_the_pinned_build_parse() {
        let mut parsed = 0;
        for path in find_paks(&game_dir()).unwrap() {
            let mut pak = PakArchive::open(&path).unwrap();
            for entry in pak.entries().unwrap() {
                if entry.method == EntryMethod::Store && entry.path.ends_with(DATASHEET_EXTENSION) {
                    let bytes = pak.read(&entry, &NoOodle).unwrap();
                    Datasheet::parse(&bytes)
                        .unwrap_or_else(|error| panic!("{}: {error}", entry.path));
                    parsed += 1;
                }
            }
        }
        assert_eq!(parsed, PINNED_STORED_DATASHEETS);
    }
}
