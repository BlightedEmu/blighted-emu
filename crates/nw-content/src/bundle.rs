// SPDX-License-Identifier: AGPL-3.0-or-later
//! The content bundle file (M1 spec section 5), format version 2:
//! magic `NWCB` (4 bytes), format version (`u16` LE), schema hash (`u32` LE, the generated
//! [`SCHEMA_HASH`]), client build id length (`u16` LE), client build id (UTF-8), body CRC32
//! (`u32` LE), postcard body (tables and text).
//!
//! Loading checks every header field before it decodes the body.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::generated::{SCHEMA_HASH, Tables};

/// First 4 bytes of every bundle.
pub const BUNDLE_MAGIC: [u8; 4] = *b"NWCB";
/// Current bundle format version.
pub const BUNDLE_FORMAT_VERSION: u16 = 2;
const MAGIC_SIZE: usize = BUNDLE_MAGIC.len();
const VERSION_SIZE: usize = size_of::<u16>();
const SCHEMA_HASH_SIZE: usize = size_of::<u32>();
const BUILD_ID_LENGTH_SIZE: usize = size_of::<u16>();
const CHECKSUM_SIZE: usize = size_of::<u32>();
/// Header size without the build id.
const FIXED_HEADER_SIZE: usize =
    MAGIC_SIZE + VERSION_SIZE + SCHEMA_HASH_SIZE + BUILD_ID_LENGTH_SIZE + CHECKSUM_SIZE;

/// All content that `sim` loads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContentBundle {
    /// Steam build id of the client the content came from. Stored in the header, not the body.
    #[serde(skip)]
    pub client_build: String,
    /// Every datasheet, typed.
    pub tables: Tables,
    /// English text by localization key.
    pub text: BTreeMap<String, String>,
}

/// Error from bundle reading or writing.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    /// The file cannot be read or written.
    #[error("cannot access bundle {}: {source}", .path.display())]
    Io {
        /// The bundle file.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The data ends inside the header.
    #[error("bundle is shorter than its header")]
    Truncated,
    /// The magic is not `NWCB`.
    #[error("not a content bundle (bad magic)")]
    BadMagic,
    /// The format version is not supported.
    #[error("bundle format version {found} is not supported")]
    UnsupportedVersion {
        /// Version in the file.
        found: u16,
    },
    /// The body CRC32 does not match the header.
    #[error("bundle checksum is {actual:08x}, expected {expected:08x}")]
    ChecksumMismatch {
        /// CRC32 in the header.
        expected: u32,
        /// CRC32 of the body.
        actual: u32,
    },
    /// The content cannot be encoded.
    #[error("cannot encode bundle: {0}")]
    Encode(postcard::Error),
    /// The body cannot be decoded.
    #[error("cannot decode bundle: {0}")]
    Decode(postcard::Error),
    /// The bundle was written with other generated table code.
    #[error("bundle schema hash is {found:08x}, expected {expected:08x}; rebuild the bundle")]
    SchemaMismatch {
        /// `SCHEMA_HASH` of this build of `nw-content`.
        expected: u32,
        /// Schema hash in the bundle header.
        found: u32,
    },
    /// The body has bytes after the encoded content.
    #[error("bundle body has {count} bytes after the content")]
    TrailingBytes {
        /// Number of extra bytes.
        count: usize,
    },
    /// The client build id does not fit the length field of the header.
    #[error("client build id of {length} bytes is too long for the bundle header")]
    BuildIdTooLong {
        /// Length of the build id in bytes.
        length: usize,
    },
    /// The bundle is from another client build.
    #[error("bundle is for client build {found}, expected {expected}")]
    BuildMismatch {
        /// Build the caller expects.
        expected: String,
        /// Build in the bundle.
        found: String,
    },
}

impl ContentBundle {
    /// Encodes the bundle with its header.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BundleError> {
        let build = self.client_build.as_bytes();
        let build_length = u16::try_from(build.len()).map_err(|_| BundleError::BuildIdTooLong {
            length: build.len(),
        })?;
        let body = postcard::to_allocvec(self).map_err(BundleError::Encode)?;
        let mut bytes = Vec::with_capacity(FIXED_HEADER_SIZE + build.len() + body.len());
        bytes.extend_from_slice(&BUNDLE_MAGIC);
        bytes.extend_from_slice(&BUNDLE_FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&SCHEMA_HASH.to_le_bytes());
        bytes.extend_from_slice(&build_length.to_le_bytes());
        bytes.extend_from_slice(build);
        bytes.extend_from_slice(&crc32fast::hash(&body).to_le_bytes());
        bytes.extend_from_slice(&body);
        Ok(bytes)
    }

    /// Checks the header, then decodes the bundle. Refuses another format version, other
    /// generated table code ([`SCHEMA_HASH`]) and another client build before decoding.
    pub fn from_bytes(bytes: &[u8], expected_build: &str) -> Result<Self, BundleError> {
        let (magic, rest) = take::<MAGIC_SIZE>(bytes)?;
        if *magic != BUNDLE_MAGIC {
            return Err(BundleError::BadMagic);
        }
        let (version, rest) = take::<VERSION_SIZE>(rest)?;
        let version = u16::from_le_bytes(*version);
        if version != BUNDLE_FORMAT_VERSION {
            return Err(BundleError::UnsupportedVersion { found: version });
        }
        let (schema_hash, rest) = take::<SCHEMA_HASH_SIZE>(rest)?;
        let schema_hash = u32::from_le_bytes(*schema_hash);
        if schema_hash != SCHEMA_HASH {
            return Err(BundleError::SchemaMismatch {
                expected: SCHEMA_HASH,
                found: schema_hash,
            });
        }
        let (build_length, rest) = take::<BUILD_ID_LENGTH_SIZE>(rest)?;
        let (build, rest) = rest
            .split_at_checked(usize::from(u16::from_le_bytes(*build_length)))
            .ok_or(BundleError::Truncated)?;
        if build != expected_build.as_bytes() {
            return Err(BundleError::BuildMismatch {
                expected: expected_build.to_owned(),
                found: String::from_utf8_lossy(build).into_owned(),
            });
        }
        let (checksum, body) = take::<CHECKSUM_SIZE>(rest)?;
        let expected = u32::from_le_bytes(*checksum);
        let actual = crc32fast::hash(body);
        if actual != expected {
            return Err(BundleError::ChecksumMismatch { expected, actual });
        }
        let (mut bundle, trailing): (Self, _) =
            postcard::take_from_bytes(body).map_err(BundleError::Decode)?;
        if !trailing.is_empty() {
            return Err(BundleError::TrailingBytes {
                count: trailing.len(),
            });
        }
        bundle.client_build = expected_build.to_owned();
        Ok(bundle)
    }

    /// Writes the bundle to `path`, creating parent folders.
    pub fn save(&self, path: &Path) -> Result<(), BundleError> {
        let io_error = |source| BundleError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        std::fs::write(path, self.to_bytes()?).map_err(io_error)
    }

    /// Reads and checks the bundle at `path`.
    pub fn load(path: &Path, expected_build: &str) -> Result<Self, BundleError> {
        let bytes = std::fs::read(path).map_err(|source| BundleError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_bytes(&bytes, expected_build)
    }
}

/// Splits the first `N` header bytes from `bytes`.
fn take<const N: usize>(bytes: &[u8]) -> Result<(&[u8; N], &[u8]), BundleError> {
    bytes.split_first_chunk::<N>().ok_or(BundleError::Truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: &str = "22469132";
    const OTHER_BUILD: &str = "1";
    const VERSION_OFFSET: usize = MAGIC_SIZE;
    const SCHEMA_HASH_OFFSET: usize = VERSION_OFFSET + VERSION_SIZE;
    const BUILD_ID_OFFSET: usize = SCHEMA_HASH_OFFSET + SCHEMA_HASH_SIZE + BUILD_ID_LENGTH_SIZE;
    const CHECKSUM_OFFSET: usize = BUILD_ID_OFFSET + BUILD.len();
    const BODY_OFFSET: usize = CHECKSUM_OFFSET + CHECKSUM_SIZE;
    const EXTRA_BYTE: u8 = 0;

    fn bundle() -> ContentBundle {
        let mut bundle = ContentBundle {
            client_build: BUILD.to_owned(),
            ..ContentBundle::default()
        };
        bundle
            .text
            .insert("@ui_hello".to_owned(), "Hello".to_owned());
        bundle
    }

    #[test]
    fn round_trip_keeps_everything() {
        let bytes = bundle().to_bytes().unwrap();
        assert_eq!(&bytes[..MAGIC_SIZE], &BUNDLE_MAGIC);
        assert_eq!(ContentBundle::from_bytes(&bytes, BUILD).unwrap(), bundle());
    }

    #[test]
    fn truncated_data_is_refused() {
        assert!(matches!(
            ContentBundle::from_bytes(b"NWC", BUILD),
            Err(BundleError::Truncated)
        ));
    }

    #[test]
    fn bad_magic_is_refused() {
        let mut bytes = bundle().to_bytes().unwrap();
        bytes[0] = b'X';
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, BUILD),
            Err(BundleError::BadMagic)
        ));
    }

    #[test]
    fn other_version_is_refused() {
        let mut bytes = bundle().to_bytes().unwrap();
        let other = BUNDLE_FORMAT_VERSION + 1;
        bytes[VERSION_OFFSET..VERSION_OFFSET + VERSION_SIZE].copy_from_slice(&other.to_le_bytes());
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, BUILD),
            Err(BundleError::UnsupportedVersion { found }) if found == other
        ));
    }

    #[test]
    fn checksum_mismatch_is_refused() {
        let mut bytes = bundle().to_bytes().unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, BUILD),
            Err(BundleError::ChecksumMismatch { .. })
        ));
    }

    /// Writes a new body after the header and fixes the body CRC32.
    fn replace_body(bytes: &mut Vec<u8>, body: &[u8]) {
        bytes.truncate(BODY_OFFSET);
        bytes.extend_from_slice(body);
        bytes[CHECKSUM_OFFSET..BODY_OFFSET].copy_from_slice(&crc32fast::hash(body).to_le_bytes());
    }

    #[test]
    fn header_holds_schema_hash_and_build() {
        let bytes = bundle().to_bytes().unwrap();
        let schema_hash = &bytes[SCHEMA_HASH_OFFSET..SCHEMA_HASH_OFFSET + SCHEMA_HASH_SIZE];
        assert_eq!(schema_hash, SCHEMA_HASH.to_le_bytes());
        assert_eq!(&bytes[BUILD_ID_OFFSET..CHECKSUM_OFFSET], BUILD.as_bytes());
    }

    #[test]
    fn every_header_prefix_is_truncated() {
        let bytes = bundle().to_bytes().unwrap();
        for len in 0..BODY_OFFSET {
            assert!(
                matches!(
                    ContentBundle::from_bytes(&bytes[..len], BUILD),
                    Err(BundleError::Truncated)
                ),
                "prefix of {len} bytes"
            );
        }
    }

    #[test]
    fn schema_mismatch_is_refused() {
        let mut bytes = bundle().to_bytes().unwrap();
        let other = SCHEMA_HASH ^ 1;
        bytes[SCHEMA_HASH_OFFSET..SCHEMA_HASH_OFFSET + SCHEMA_HASH_SIZE]
            .copy_from_slice(&other.to_le_bytes());
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, BUILD),
            Err(BundleError::SchemaMismatch { expected, found })
                if expected == SCHEMA_HASH && found == other
        ));
    }

    #[test]
    fn build_mismatch_is_refused_before_decoding() {
        let mut bytes = bundle().to_bytes().unwrap();
        replace_body(&mut bytes, &[]);
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, OTHER_BUILD),
            Err(BundleError::BuildMismatch { expected, found }) if expected == OTHER_BUILD && found == BUILD
        ));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut bytes = bundle().to_bytes().unwrap();
        let mut body = bytes[BODY_OFFSET..].to_vec();
        body.push(EXTRA_BYTE);
        replace_body(&mut bytes, &body);
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, BUILD),
            Err(BundleError::TrailingBytes { count: 1 })
        ));
    }

    #[test]
    fn too_long_build_id_is_refused() {
        let length = usize::from(u16::MAX) + 1;
        let bundle = ContentBundle {
            client_build: "1".repeat(length),
            ..ContentBundle::default()
        };
        assert!(matches!(
            bundle.to_bytes(),
            Err(BundleError::BuildIdTooLong { length: found }) if found == length
        ));
    }

    #[test]
    fn build_mismatch_is_refused() {
        let bytes = bundle().to_bytes().unwrap();
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, OTHER_BUILD),
            Err(BundleError::BuildMismatch { expected, found }) if expected == OTHER_BUILD && found == BUILD
        ));
    }

    #[test]
    fn save_creates_folders_and_load_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("content/bundle.bin");
        bundle().save(&path).unwrap();
        assert_eq!(ContentBundle::load(&path, BUILD).unwrap(), bundle());
    }
}
