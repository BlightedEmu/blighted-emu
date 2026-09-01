// SPDX-License-Identifier: MIT OR Apache-2.0
//! AZCS compressed wrapper (M1b spec section 3.1): a big-endian header (magic, compressor id,
//! decoded size, seek point count), one zlib stream, then 16 bytes per seek point.

use std::io::Read;

use flate2::read::ZlibDecoder;

use crate::pak::MAX_ENTRY_SIZE;

/// First 4 bytes of every AZCS file.
pub const AZCS_MAGIC: [u8; 4] = *b"AZCS";
/// Compressor id of zlib (crc32 of "zlib"), the only compressor in the pinned client.
pub const AZCS_ZLIB: u32 = 0x7388_7d3a;
/// Largest decoded size this reader accepts.
pub const MAX_AZCS_DECODED_SIZE: u64 = MAX_ENTRY_SIZE;
/// Size of one seek point at the end of the file.
const SEEK_POINT_SIZE: usize = 16;
const MAGIC_SIZE: usize = AZCS_MAGIC.len();
const U32_SIZE: usize = size_of::<u32>();
const U64_SIZE: usize = size_of::<u64>();

/// Error from AZCS decoding.
#[derive(Debug, thiserror::Error)]
pub enum AzcsError {
    /// The data ends inside the header.
    #[error("AZCS data is shorter than its header")]
    Truncated,
    /// The magic is not `AZCS`.
    #[error("not an AZCS file (bad magic)")]
    BadMagic,
    /// The compressor is not zlib.
    #[error("AZCS compressor {id:08x} is not supported")]
    UnsupportedCompressor {
        /// Compressor id in the header.
        id: u32,
    },
    /// The declared decoded size is above [`MAX_AZCS_DECODED_SIZE`].
    #[error("AZCS decoded size {size} is above the limit {limit}")]
    TooLarge {
        /// Declared size.
        size: u64,
        /// Largest allowed size.
        limit: u64,
    },
    /// The seek table does not fit in the data.
    #[error("AZCS seek table of {count} points does not fit in the data")]
    BadSeekTable {
        /// Seek point count in the header.
        count: u32,
    },
    /// The zlib stream is not valid.
    #[error("AZCS zlib stream is not valid: {0}")]
    Inflate(#[source] std::io::Error),
    /// The zlib stream decodes to another size than the header declares.
    #[error("AZCS data decodes to {actual} bytes, header declares {expected}")]
    SizeMismatch {
        /// Declared size.
        expected: u64,
        /// Decoded size.
        actual: u64,
    },
}

/// True when `bytes` starts with the AZCS magic.
pub fn is_azcs(bytes: &[u8]) -> bool {
    bytes.starts_with(&AZCS_MAGIC)
}

/// Decodes an AZCS file. Checks every header field before it inflates.
pub fn decode_azcs(bytes: &[u8]) -> Result<Vec<u8>, AzcsError> {
    let (magic, rest) = take::<MAGIC_SIZE>(bytes)?;
    if magic != AZCS_MAGIC {
        return Err(AzcsError::BadMagic);
    }
    let (compressor, rest) = take::<U32_SIZE>(rest)?;
    let (size, rest) = take::<U64_SIZE>(rest)?;
    let (count, rest) = take::<U32_SIZE>(rest)?;
    let compressor = u32::from_be_bytes(compressor);
    if compressor != AZCS_ZLIB {
        return Err(AzcsError::UnsupportedCompressor { id: compressor });
    }
    let size = u64::from_be_bytes(size);
    if size > MAX_AZCS_DECODED_SIZE {
        return Err(AzcsError::TooLarge {
            size,
            limit: MAX_AZCS_DECODED_SIZE,
        });
    }
    let count = u32::from_be_bytes(count);
    let stream_length = (count as usize)
        .checked_mul(SEEK_POINT_SIZE)
        .and_then(|table| rest.len().checked_sub(table))
        .ok_or(AzcsError::BadSeekTable { count })?;
    let mut decoded = Vec::new();
    // One byte past the declared size is enough to detect a size mismatch.
    ZlibDecoder::new(&rest[..stream_length])
        .take(size + 1)
        .read_to_end(&mut decoded)
        .map_err(AzcsError::Inflate)?;
    let actual = decoded.len() as u64;
    if actual != size {
        return Err(AzcsError::SizeMismatch {
            expected: size,
            actual,
        });
    }
    Ok(decoded)
}

/// Splits the first `N` bytes from `bytes`.
fn take<const N: usize>(bytes: &[u8]) -> Result<([u8; N], &[u8]), AzcsError> {
    bytes
        .split_first_chunk::<N>()
        .map(|(head, rest)| (*head, rest))
        .ok_or(AzcsError::Truncated)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::ZlibEncoder;

    use super::*;

    const DATA: &[u8] = b"\0\0\0\0\x03object stream bytes";
    /// Compressor id of zstd (crc32 of "zstd"); it does not occur in the pinned client.
    const AZCS_ZSTD: u32 = 0x72fd_505e;
    const SEEK_COUNT_OFFSET: usize = MAGIC_SIZE + U32_SIZE + U64_SIZE;
    const HEADER_SIZE: usize = SEEK_COUNT_OFFSET + U32_SIZE;
    const ONE_SEEK_POINT: u32 = 1;

    /// Builds an AZCS file around the zlib form of `data`.
    fn wrap(data: &[u8], compressor: u32, declared_size: u64, seek_points: u32) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        let stream = encoder.finish().unwrap();
        let mut bytes = AZCS_MAGIC.to_vec();
        bytes.extend_from_slice(&compressor.to_be_bytes());
        bytes.extend_from_slice(&declared_size.to_be_bytes());
        bytes.extend_from_slice(&seek_points.to_be_bytes());
        bytes.extend_from_slice(&stream);
        bytes.resize(bytes.len() + seek_points as usize * SEEK_POINT_SIZE, 0);
        bytes
    }

    fn valid() -> Vec<u8> {
        wrap(DATA, AZCS_ZLIB, DATA.len() as u64, ONE_SEEK_POINT)
    }

    #[test]
    fn decodes_the_zlib_stream() {
        assert_eq!(decode_azcs(&valid()).unwrap(), DATA);
    }

    #[test]
    fn is_azcs_checks_the_magic() {
        assert!(is_azcs(&valid()));
        assert!(!is_azcs(DATA));
    }

    #[test]
    fn every_header_prefix_is_truncated() {
        let bytes = valid();
        for length in 0..HEADER_SIZE {
            assert!(
                matches!(decode_azcs(&bytes[..length]), Err(AzcsError::Truncated)),
                "prefix of {length} bytes"
            );
        }
    }

    #[test]
    fn bad_magic_is_refused() {
        let mut bytes = valid();
        bytes[0] = b'X';
        assert!(matches!(decode_azcs(&bytes), Err(AzcsError::BadMagic)));
    }

    #[test]
    fn other_compressor_is_refused() {
        let bytes = wrap(DATA, AZCS_ZSTD, DATA.len() as u64, ONE_SEEK_POINT);
        assert!(matches!(
            decode_azcs(&bytes),
            Err(AzcsError::UnsupportedCompressor { id: AZCS_ZSTD })
        ));
    }

    #[test]
    fn size_above_limit_is_refused_before_inflating() {
        let bytes = wrap(DATA, AZCS_ZLIB, MAX_AZCS_DECODED_SIZE + 1, ONE_SEEK_POINT);
        assert!(matches!(
            decode_azcs(&bytes),
            Err(AzcsError::TooLarge { size, .. }) if size == MAX_AZCS_DECODED_SIZE + 1
        ));
    }

    #[test]
    fn seek_table_larger_than_the_data_is_refused() {
        let mut bytes = valid();
        let count = u32::MAX;
        bytes[SEEK_COUNT_OFFSET..HEADER_SIZE].copy_from_slice(&count.to_be_bytes());
        assert!(matches!(
            decode_azcs(&bytes),
            Err(AzcsError::BadSeekTable { count: u32::MAX })
        ));
    }

    #[test]
    fn declared_size_must_match_the_stream() {
        for declared in [DATA.len() as u64 - 1, DATA.len() as u64 + 1] {
            let bytes = wrap(DATA, AZCS_ZLIB, declared, ONE_SEEK_POINT);
            assert!(matches!(
                decode_azcs(&bytes),
                Err(AzcsError::SizeMismatch { expected, .. }) if expected == declared
            ));
        }
    }

    #[test]
    fn broken_zlib_stream_is_refused() {
        let mut bytes = valid();
        bytes[HEADER_SIZE] ^= 0xff;
        assert!(matches!(decode_azcs(&bytes), Err(AzcsError::Inflate(_))));
    }
}
