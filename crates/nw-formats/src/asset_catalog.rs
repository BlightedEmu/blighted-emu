// SPDX-License-Identifier: MIT OR Apache-2.0
//! The asset catalog (`assetcatalog.catalog` in `Engine.pak`): the path of each asset id
//! (creature-animation note, 2026-09-18; layout Observed in build 25810808).
//!
//! The file is little-endian: the magic `RASC`, version 1, then the offsets of its sections. A
//! table of 40-byte records follows the header: each holds an asset id (an index into a table of
//! 16-byte GUIDs, and a sub id) and, at its end, its path (a directory and a file: offsets into
//! two tables of NUL-ended strings). A creature's spawn card names its slice's asset id, whose
//! sub id is [`SLICE_SUB_ID`] (Observed: all 747 creature assets of the mob prefab table).

use std::collections::{HashMap, HashSet};

/// The catalog's entry in `Engine.pak`.
pub const ASSET_CATALOG_ENTRY: &str = "assetcatalog.catalog";
/// The sub id of a `.dynamicslice` asset id (Observed: `slices/characters/boar.dynamicslice` is
/// `db96d765…` sub 2, the id a boar's spawn card names).
pub const SLICE_SUB_ID: u32 = 2;

const MAGIC: &[u8; 4] = b"RASC";
const VERSION: u32 = 1;
const VERSION_AT: usize = 4;
const GUID_TABLE_AT: usize = 0x10;
const DIRECTORY_NAMES_AT: usize = 0x18;
const FILE_NAMES_AT: usize = 0x1c;
const RECORD_COUNT_AT: usize = 0x24;
/// Where the records start (Observed).
const RECORDS_AT: usize = 0x78;
const RECORD_LEN: usize = 40;
const RECORD_GUID_INDEX: usize = 0;
const RECORD_SUB_ID: usize = 4;
const RECORD_DIRECTORY: usize = 32;
const RECORD_FILE: usize = 36;
/// Bytes of a GUID.
pub const GUID_LEN: usize = 16;
const U32_LEN: usize = 4;

/// An asset catalog that cannot be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AssetCatalogError {
    /// The file is not a version-1 `RASC` catalog.
    #[error("not an asset catalog (magic or version)")]
    NotACatalog,
    /// A section, record or string lies outside the file.
    #[error("asset catalog is truncated at byte {at}")]
    Truncated {
        /// The offset that was read.
        at: usize,
    },
    /// A path is not UTF-8.
    #[error("asset catalog path at byte {at} is not UTF-8")]
    NotUtf8 {
        /// The string's offset.
        at: usize,
    },
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, AssetCatalogError> {
    bytes
        .get(at..at + U32_LEN)
        .and_then(|slice| <[u8; U32_LEN]>::try_from(slice).ok())
        .map(u32::from_le_bytes)
        .ok_or(AssetCatalogError::Truncated { at })
}

fn offset_at(bytes: &[u8], at: usize) -> Result<usize, AssetCatalogError> {
    usize::try_from(u32_at(bytes, at)?).map_err(|_| AssetCatalogError::Truncated { at })
}

fn string_at(bytes: &[u8], at: usize) -> Result<&str, AssetCatalogError> {
    let tail = bytes.get(at..).ok_or(AssetCatalogError::Truncated { at })?;
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(AssetCatalogError::Truncated { at })?;
    std::str::from_utf8(&tail[..end]).map_err(|_| AssetCatalogError::NotUtf8 { at })
}

/// The paths of the asset ids `wanted` (GUID and sub id) in the catalog `bytes`: directory and
/// file joined. An asset id the catalog does not have is left out.
///
/// # Errors
///
/// Returns [`AssetCatalogError`] when the bytes are not a catalog or are cut short.
pub fn paths_of(
    bytes: &[u8],
    wanted: &HashSet<([u8; GUID_LEN], u32)>,
) -> Result<HashMap<([u8; GUID_LEN], u32), String>, AssetCatalogError> {
    if bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) || u32_at(bytes, VERSION_AT)? != VERSION {
        return Err(AssetCatalogError::NotACatalog);
    }
    let guids_at = offset_at(bytes, GUID_TABLE_AT)?;
    let directories_at = offset_at(bytes, DIRECTORY_NAMES_AT)?;
    let files_at = offset_at(bytes, FILE_NAMES_AT)?;
    let count = offset_at(bytes, RECORD_COUNT_AT)?;
    let guid = |index: usize| -> Result<[u8; GUID_LEN], AssetCatalogError> {
        let at = guids_at + index * GUID_LEN;
        bytes
            .get(at..at + GUID_LEN)
            .and_then(|slice| <[u8; GUID_LEN]>::try_from(slice).ok())
            .ok_or(AssetCatalogError::Truncated { at })
    };
    let mut paths = HashMap::new();
    for record in 0..count {
        let at = RECORDS_AT + record * RECORD_LEN;
        let sub_id = u32_at(bytes, at + RECORD_SUB_ID)?;
        let key = (guid(offset_at(bytes, at + RECORD_GUID_INDEX)?)?, sub_id);
        if !wanted.contains(&key) {
            continue;
        }
        let directory = string_at(
            bytes,
            directories_at + offset_at(bytes, at + RECORD_DIRECTORY)?,
        )?;
        let file = string_at(bytes, files_at + offset_at(bytes, at + RECORD_FILE)?)?;
        paths.insert(key, format!("{directory}{file}"));
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOAR: [u8; GUID_LEN] = [
        0xdb, 0x96, 0xd7, 0x65, 0xb5, 0x04, 0x59, 0xd9, 0x8c, 0x9e, 0x04, 0xad, 0xaa, 0x9c, 0x94,
        0x85,
    ];

    /// A catalog of two records of one GUID: the dynamic slice (sub 2) and another product.
    fn catalog() -> Vec<u8> {
        let directories = b"\0slices/characters/\0";
        let files = b"boar.dynamicslice\0boar.aislicemetadata\0";
        let records_end = RECORDS_AT + 2 * RECORD_LEN;
        let guids_at = records_end;
        let directories_at = guids_at + GUID_LEN;
        let files_at = directories_at + directories.len();
        let mut bytes = vec![0; directories_at];
        bytes[..4].copy_from_slice(MAGIC);
        let mut put = |at: usize, value: usize| {
            let value = u32::try_from(value).expect("small");
            bytes[at..at + U32_LEN].copy_from_slice(&value.to_le_bytes());
        };
        put(VERSION_AT, 1);
        put(GUID_TABLE_AT, guids_at);
        put(DIRECTORY_NAMES_AT, directories_at);
        put(FILE_NAMES_AT, files_at);
        put(RECORD_COUNT_AT, 2);
        for (record, (file, sub_id)) in [(0, 2), (18, 6)].into_iter().enumerate() {
            let at = RECORDS_AT + record * RECORD_LEN;
            put(at + RECORD_DIRECTORY, 1);
            put(at + RECORD_FILE, file);
            put(at + RECORD_GUID_INDEX, 0);
            put(at + RECORD_SUB_ID, sub_id);
        }
        bytes[guids_at..guids_at + GUID_LEN].copy_from_slice(&BOAR);
        bytes.extend_from_slice(directories);
        bytes.extend_from_slice(files);
        bytes
    }

    #[test]
    fn a_slice_id_gives_its_dynamic_slice() {
        let wanted = HashSet::from([(BOAR, SLICE_SUB_ID), ([0; GUID_LEN], SLICE_SUB_ID)]);
        let paths = paths_of(&catalog(), &wanted).expect("catalog");
        assert_eq!(paths.len(), 1, "an unknown id is left out");
        assert_eq!(
            paths.get(&(BOAR, SLICE_SUB_ID)).map(String::as_str),
            Some("slices/characters/boar.dynamicslice")
        );
    }

    #[test]
    fn another_file_is_not_a_catalog() {
        assert_eq!(
            paths_of(b"PK\x03\x04", &HashSet::new()),
            Err(AssetCatalogError::NotACatalog)
        );
        let mut cut = catalog();
        cut.truncate(RECORDS_AT + RECORD_LEN / 2);
        assert!(matches!(
            paths_of(&cut, &HashSet::new()),
            Err(AssetCatalogError::Truncated { .. })
        ));
    }
}
