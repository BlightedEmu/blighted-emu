// SPDX-License-Identifier: MIT OR Apache-2.0
//! The CryEngine chunk file (`CrCh`, version `0x746`) that holds the client's animations (`.caf`)
//! and skeletons (`.chr`) (layout Observed in build 25810808, 2026-09-18): the
//! magic, the version, the chunk count and the offset of the chunk table, whose 16-byte entries
//! hold each chunk's type, version, id, size and offset, little-endian.

/// The magic of a chunk file.
const MAGIC: &[u8; 4] = b"CrCh";
/// The version this reader knows.
const VERSION: u32 = 0x746;
const VERSION_AT: usize = 4;
const COUNT_AT: usize = 8;
const TABLE_AT: usize = 12;
const ENTRY_LEN: usize = 16;
const ENTRY_VERSION: usize = 2;
const ENTRY_ID: usize = 4;
const ENTRY_SIZE: usize = 8;
const ENTRY_OFFSET: usize = 12;
const U16_LEN: usize = 2;
const U32_LEN: usize = 4;

/// A chunk file that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChunkFileError {
    /// The file is not a version-`0x746` chunk file.
    #[error("not a CrCh chunk file of version 0x746")]
    NotAChunkFile,
    /// A read lies outside the file or its chunk.
    #[error("chunk file is truncated at byte {at}")]
    Truncated {
        /// The offset that was read.
        at: usize,
    },
}

/// One chunk: its type, version, id and bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk<'a> {
    /// The chunk type.
    pub kind: u16,
    /// The chunk version.
    pub version: u16,
    /// The chunk id.
    pub id: u32,
    /// Its bytes.
    pub data: &'a [u8],
}

/// The little-endian `u16` at `at` of `bytes`.
///
/// # Errors
///
/// Returns [`ChunkFileError::Truncated`] when it lies outside `bytes`.
pub fn u16_at(bytes: &[u8], at: usize) -> Result<u16, ChunkFileError> {
    bytes
        .get(at..at + U16_LEN)
        .and_then(|slice| <[u8; U16_LEN]>::try_from(slice).ok())
        .map(u16::from_le_bytes)
        .ok_or(ChunkFileError::Truncated { at })
}

/// The little-endian `u32` at `at` of `bytes`.
///
/// # Errors
///
/// Returns [`ChunkFileError::Truncated`] when it lies outside `bytes`.
pub fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ChunkFileError> {
    bytes
        .get(at..at + U32_LEN)
        .and_then(|slice| <[u8; U32_LEN]>::try_from(slice).ok())
        .map(u32::from_le_bytes)
        .ok_or(ChunkFileError::Truncated { at })
}

/// The little-endian `f32` at `at` of `bytes`.
///
/// # Errors
///
/// Returns [`ChunkFileError::Truncated`] when it lies outside `bytes`.
pub fn f32_at(bytes: &[u8], at: usize) -> Result<f32, ChunkFileError> {
    u32_at(bytes, at).map(f32::from_bits)
}

/// The offset `at` of `bytes` as a `usize`.
fn offset_at(bytes: &[u8], at: usize) -> Result<usize, ChunkFileError> {
    usize::try_from(u32_at(bytes, at)?).map_err(|_| ChunkFileError::Truncated { at })
}

/// Every chunk of the chunk file `bytes`, in table order.
///
/// # Errors
///
/// Returns [`ChunkFileError`] when the magic or version is wrong or a chunk lies outside the
/// file.
pub fn chunks(bytes: &[u8]) -> Result<Vec<Chunk<'_>>, ChunkFileError> {
    if bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) || u32_at(bytes, VERSION_AT)? != VERSION {
        return Err(ChunkFileError::NotAChunkFile);
    }
    let count = offset_at(bytes, COUNT_AT)?;
    let table = offset_at(bytes, TABLE_AT)?;
    (0..count)
        .map(|index| {
            let entry = table + index * ENTRY_LEN;
            let offset = offset_at(bytes, entry + ENTRY_OFFSET)?;
            let size = offset_at(bytes, entry + ENTRY_SIZE)?;
            Ok(Chunk {
                kind: u16_at(bytes, entry)?,
                version: u16_at(bytes, entry + ENTRY_VERSION)?,
                id: u32_at(bytes, entry + ENTRY_ID)?,
                data: bytes
                    .get(offset..offset + size)
                    .ok_or(ChunkFileError::Truncated { at: offset })?,
            })
        })
        .collect()
}

/// Test helpers: a chunk file of the given chunks.
#[cfg(test)]
pub(crate) fn chunk_file(parts: &[(u16, u16, &[u8])]) -> Vec<u8> {
    let header_len = TABLE_AT + U32_LEN;
    let table_len = parts.len() * ENTRY_LEN;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(parts.len()).expect("count").to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(header_len).expect("table").to_le_bytes());
    let mut offset = header_len + table_len;
    for (id, (kind, version, data)) in parts.iter().enumerate() {
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(id).expect("id").to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(data.len()).expect("size").to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(offset).expect("offset").to_le_bytes());
        offset += data.len();
    }
    for (_, _, data) in parts {
        bytes.extend_from_slice(data);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chunk_file_lists_its_chunks() {
        let bytes = chunk_file(&[(0x3002, 0x925, &[1, 2, 3]), (0x100d, 0x831, &[4])]);
        let found = chunks(&bytes).expect("chunks");
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].kind, found[0].version, found[0].data),
            (0x3002, 0x925, [1_u8, 2, 3].as_slice())
        );
        assert_eq!((found[1].id, found[1].data), (1, [4_u8].as_slice()));
    }

    #[test]
    fn other_files_and_cut_files_are_errors() {
        assert_eq!(chunks(b"RASC0000"), Err(ChunkFileError::NotAChunkFile));
        let mut bytes = chunk_file(&[(1, 1, &[9; 8])]);
        bytes.truncate(bytes.len() - 1);
        assert!(matches!(
            chunks(&bytes),
            Err(ChunkFileError::Truncated { .. })
        ));
    }
}
