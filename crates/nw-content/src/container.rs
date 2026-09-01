// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared header of the content files (M1b spec section 3.2): the content bundle and the world
//! files.
//!
//! magic (4 bytes), format version (`u16` LE), schema hash (`u32` LE), client build id length
//! (`u16` LE), client build id (UTF-8), body CRC32 (`u32` LE), body.
//!
//! Decoding checks every header field before it returns the body. [`encode_value`] and
//! [`decode_value`] add the postcard body that both file types use.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

const MAGIC_SIZE: usize = 4;
const VERSION_SIZE: usize = size_of::<u16>();
const SCHEMA_HASH_SIZE: usize = size_of::<u32>();
const BUILD_ID_LENGTH_SIZE: usize = size_of::<u16>();
const CHECKSUM_SIZE: usize = size_of::<u32>();
/// Header size without the build id.
const FIXED_HEADER_SIZE: usize =
    MAGIC_SIZE + VERSION_SIZE + SCHEMA_HASH_SIZE + BUILD_ID_LENGTH_SIZE + CHECKSUM_SIZE;

/// What a file type writes into the header and expects back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerSpec {
    /// First 4 bytes of the file.
    pub magic: [u8; MAGIC_SIZE],
    /// Format version of the file type.
    pub format_version: u16,
    /// Hash or version of the body schema.
    pub schema_hash: u32,
}

/// Error from the shared header.
#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    /// The file cannot be read or written.
    #[error("cannot access {}: {source}", .path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The data ends inside the header.
    #[error("file is shorter than its header")]
    Truncated,
    /// The magic is not the expected one.
    #[error("bad magic {found:?}, expected {expected:?}")]
    BadMagic {
        /// Magic the caller expects.
        expected: [u8; MAGIC_SIZE],
        /// Magic in the file.
        found: [u8; MAGIC_SIZE],
    },
    /// The format version is not supported.
    #[error("format version {found} is not supported")]
    UnsupportedVersion {
        /// Version in the file.
        found: u16,
    },
    /// The body was written with another schema.
    #[error("schema hash is {found:08x}, expected {expected:08x}; rebuild the file")]
    SchemaMismatch {
        /// Schema hash the caller expects.
        expected: u32,
        /// Schema hash in the file.
        found: u32,
    },
    /// The client build id does not fit the length field of the header.
    #[error("client build id of {length} bytes is too long for the header")]
    BuildIdTooLong {
        /// Length of the build id in bytes.
        length: usize,
    },
    /// The file is from another client build.
    #[error("file is for client build {found}, expected {expected}")]
    BuildMismatch {
        /// Build the caller expects.
        expected: String,
        /// Build in the file.
        found: String,
    },
    /// The body CRC32 does not match the header.
    #[error("body checksum is {actual:08x}, expected {expected:08x}")]
    ChecksumMismatch {
        /// CRC32 in the header.
        expected: u32,
        /// CRC32 of the body.
        actual: u32,
    },
    /// The value cannot be encoded.
    #[error("cannot encode body: {0}")]
    Encode(postcard::Error),
    /// The body cannot be decoded.
    #[error("cannot decode body: {0}")]
    Decode(postcard::Error),
    /// The body has bytes after the encoded value.
    #[error("body has {count} bytes after the content")]
    TrailingBytes {
        /// Number of extra bytes.
        count: usize,
    },
}

/// Puts the header in front of `body`.
pub fn encode_container(
    spec: &ContainerSpec,
    client_build: &str,
    body: &[u8],
) -> Result<Vec<u8>, ContainerError> {
    let build = client_build.as_bytes();
    let build_length = u16::try_from(build.len()).map_err(|_| ContainerError::BuildIdTooLong {
        length: build.len(),
    })?;
    let mut bytes = Vec::with_capacity(FIXED_HEADER_SIZE + build.len() + body.len());
    bytes.extend_from_slice(&spec.magic);
    bytes.extend_from_slice(&spec.format_version.to_le_bytes());
    bytes.extend_from_slice(&spec.schema_hash.to_le_bytes());
    bytes.extend_from_slice(&build_length.to_le_bytes());
    bytes.extend_from_slice(build);
    bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
    bytes.extend_from_slice(body);
    Ok(bytes)
}

/// Checks magic, format version, schema hash, build id and body CRC32, in that order, and
/// returns the body.
pub fn decode_container<'a>(
    spec: &ContainerSpec,
    bytes: &'a [u8],
    expected_build: &str,
) -> Result<&'a [u8], ContainerError> {
    let (magic, rest) = take::<MAGIC_SIZE>(bytes)?;
    if *magic != spec.magic {
        return Err(ContainerError::BadMagic {
            expected: spec.magic,
            found: *magic,
        });
    }
    let (version, rest) = take::<VERSION_SIZE>(rest)?;
    let version = u16::from_le_bytes(*version);
    if version != spec.format_version {
        return Err(ContainerError::UnsupportedVersion { found: version });
    }
    let (schema_hash, rest) = take::<SCHEMA_HASH_SIZE>(rest)?;
    let schema_hash = u32::from_le_bytes(*schema_hash);
    if schema_hash != spec.schema_hash {
        return Err(ContainerError::SchemaMismatch {
            expected: spec.schema_hash,
            found: schema_hash,
        });
    }
    let (build_length, rest) = take::<BUILD_ID_LENGTH_SIZE>(rest)?;
    let (build, rest) = rest
        .split_at_checked(usize::from(u16::from_le_bytes(*build_length)))
        .ok_or(ContainerError::Truncated)?;
    if build != expected_build.as_bytes() {
        return Err(ContainerError::BuildMismatch {
            expected: expected_build.to_owned(),
            found: String::from_utf8_lossy(build).into_owned(),
        });
    }
    let (checksum, body) = take::<CHECKSUM_SIZE>(rest)?;
    let expected = u32::from_le_bytes(*checksum);
    let actual = crc32fast::hash(body);
    if actual != expected {
        return Err(ContainerError::ChecksumMismatch { expected, actual });
    }
    Ok(body)
}

/// Encodes `value` with postcard and puts the header in front of it.
pub fn encode_value<T: Serialize>(
    spec: &ContainerSpec,
    client_build: &str,
    value: &T,
) -> Result<Vec<u8>, ContainerError> {
    let body = postcard::to_allocvec(value).map_err(ContainerError::Encode)?;
    encode_container(spec, client_build, &body)
}

/// Checks the header with [`decode_container`], then decodes the whole body as one `T`.
pub fn decode_value<T: DeserializeOwned>(
    spec: &ContainerSpec,
    bytes: &[u8],
    expected_build: &str,
) -> Result<T, ContainerError> {
    let body = decode_container(spec, bytes, expected_build)?;
    let (value, trailing) = postcard::take_from_bytes(body).map_err(ContainerError::Decode)?;
    if !trailing.is_empty() {
        return Err(ContainerError::TrailingBytes {
            count: trailing.len(),
        });
    }
    Ok(value)
}

/// Writes `bytes` to `path`, creating parent folders.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), ContainerError> {
    let io_error = |source| ContainerError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    std::fs::write(path, bytes).map_err(io_error)
}

/// Reads the whole file at `path`.
pub fn read_file(path: &Path) -> Result<Vec<u8>, ContainerError> {
    std::fs::read(path).map_err(|source| ContainerError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Splits the first `N` header bytes from `bytes`.
fn take<const N: usize>(bytes: &[u8]) -> Result<(&[u8; N], &[u8]), ContainerError> {
    bytes
        .split_first_chunk::<N>()
        .ok_or(ContainerError::Truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: ContainerSpec = ContainerSpec {
        magic: *b"TEST",
        format_version: 3,
        schema_hash: 0x1234_5678,
    };
    const BUILD: &str = "22469132";
    const OTHER_BUILD: &str = "1";
    const BODY: &[u8] = b"body bytes";
    const VERSION_OFFSET: usize = MAGIC_SIZE;
    const SCHEMA_HASH_OFFSET: usize = VERSION_OFFSET + VERSION_SIZE;
    const BUILD_ID_OFFSET: usize = SCHEMA_HASH_OFFSET + SCHEMA_HASH_SIZE + BUILD_ID_LENGTH_SIZE;
    const CHECKSUM_OFFSET: usize = BUILD_ID_OFFSET + BUILD.len();
    const BODY_OFFSET: usize = CHECKSUM_OFFSET + CHECKSUM_SIZE;
    const EXTRA_BYTE: u8 = 0;
    const SAMPLE_COUNT: u32 = 7;

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Sample {
        name: String,
        count: u32,
    }

    fn sample() -> Sample {
        Sample {
            name: "lv".to_owned(),
            count: SAMPLE_COUNT,
        }
    }

    fn encoded() -> Vec<u8> {
        encode_container(&SPEC, BUILD, BODY).unwrap()
    }

    #[test]
    fn round_trip_returns_the_body() {
        let bytes = encoded();
        assert_eq!(&bytes[..MAGIC_SIZE], &SPEC.magic);
        assert_eq!(
            &bytes[SCHEMA_HASH_OFFSET..SCHEMA_HASH_OFFSET + SCHEMA_HASH_SIZE],
            SPEC.schema_hash.to_le_bytes()
        );
        assert_eq!(&bytes[BUILD_ID_OFFSET..CHECKSUM_OFFSET], BUILD.as_bytes());
        assert_eq!(decode_container(&SPEC, &bytes, BUILD).unwrap(), BODY);
    }

    #[test]
    fn every_header_prefix_is_truncated() {
        let bytes = encoded();
        for length in 0..BODY_OFFSET {
            assert!(
                matches!(
                    decode_container(&SPEC, &bytes[..length], BUILD),
                    Err(ContainerError::Truncated)
                ),
                "prefix of {length} bytes"
            );
        }
    }

    #[test]
    fn bad_magic_is_refused() {
        let mut bytes = encoded();
        bytes[0] = b'X';
        assert!(matches!(
            decode_container(&SPEC, &bytes, BUILD),
            Err(ContainerError::BadMagic { expected, found })
                if expected == SPEC.magic && found == *b"XEST"
        ));
    }

    #[test]
    fn other_version_is_refused() {
        let mut bytes = encoded();
        let other = SPEC.format_version + 1;
        bytes[VERSION_OFFSET..SCHEMA_HASH_OFFSET].copy_from_slice(&other.to_le_bytes());
        assert!(matches!(
            decode_container(&SPEC, &bytes, BUILD),
            Err(ContainerError::UnsupportedVersion { found }) if found == other
        ));
    }

    #[test]
    fn schema_mismatch_is_refused() {
        let mut bytes = encoded();
        let other = SPEC.schema_hash ^ 1;
        bytes[SCHEMA_HASH_OFFSET..SCHEMA_HASH_OFFSET + SCHEMA_HASH_SIZE]
            .copy_from_slice(&other.to_le_bytes());
        assert!(matches!(
            decode_container(&SPEC, &bytes, BUILD),
            Err(ContainerError::SchemaMismatch { expected, found })
                if expected == SPEC.schema_hash && found == other
        ));
    }

    #[test]
    fn build_mismatch_is_refused() {
        assert!(matches!(
            decode_container(&SPEC, &encoded(), OTHER_BUILD),
            Err(ContainerError::BuildMismatch { expected, found })
                if expected == OTHER_BUILD && found == BUILD
        ));
    }

    #[test]
    fn checksum_mismatch_is_refused() {
        let mut bytes = encoded();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(matches!(
            decode_container(&SPEC, &bytes, BUILD),
            Err(ContainerError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn too_long_build_id_is_refused() {
        let length = usize::from(u16::MAX) + 1;
        assert!(matches!(
            encode_container(&SPEC, &"1".repeat(length), BODY),
            Err(ContainerError::BuildIdTooLong { length: found }) if found == length
        ));
    }

    #[test]
    fn value_round_trip_keeps_the_value() {
        let bytes = encode_value(&SPEC, BUILD, &sample()).unwrap();
        assert_eq!(&bytes[..MAGIC_SIZE], &SPEC.magic);
        assert_eq!(
            decode_value::<Sample>(&SPEC, &bytes, BUILD).unwrap(),
            sample()
        );
    }

    #[test]
    fn value_with_trailing_bytes_is_refused() {
        let mut body = postcard::to_allocvec(&sample()).unwrap();
        body.push(EXTRA_BYTE);
        let bytes = encode_container(&SPEC, BUILD, &body).unwrap();
        assert!(matches!(
            decode_value::<Sample>(&SPEC, &bytes, BUILD),
            Err(ContainerError::TrailingBytes { count: 1 })
        ));
    }

    #[test]
    fn body_that_is_not_the_value_is_refused() {
        let bytes = encode_container(&SPEC, BUILD, &[]).unwrap();
        assert!(matches!(
            decode_value::<Sample>(&SPEC, &bytes, BUILD),
            Err(ContainerError::Decode(_))
        ));
    }

    #[test]
    fn write_creates_folders_and_read_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("content/file.bin");
        write_file(&path, BODY).unwrap();
        assert_eq!(read_file(&path).unwrap(), BODY);
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            read_file(&dir.path().join("missing.bin")),
            Err(ContainerError::Io { .. })
        ));
    }
}
