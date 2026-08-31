// SPDX-License-Identifier: AGPL-3.0-or-later
//! The content bundle file (M1 spec section 5):
//! magic `NWCB` (4 bytes), format version (`u16` LE), body CRC32 (`u32` LE), postcard body.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::generated::Tables;

/// First 4 bytes of every bundle.
pub const BUNDLE_MAGIC: [u8; 4] = *b"NWCB";
/// Current bundle format version.
pub const BUNDLE_FORMAT_VERSION: u16 = 1;
const MAGIC_SIZE: usize = 4;
const VERSION_SIZE: usize = 2;
const CHECKSUM_SIZE: usize = 4;
const HEADER_SIZE: usize = MAGIC_SIZE + VERSION_SIZE + CHECKSUM_SIZE;

/// All content that `sim` loads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContentBundle {
    /// Steam build id of the client the content came from.
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
    /// The data is shorter than the header.
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
        let body = postcard::to_allocvec(self).map_err(BundleError::Encode)?;
        let mut bytes = Vec::with_capacity(HEADER_SIZE + body.len());
        bytes.extend_from_slice(&BUNDLE_MAGIC);
        bytes.extend_from_slice(&BUNDLE_FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&crc32fast::hash(&body).to_le_bytes());
        bytes.extend_from_slice(&body);
        Ok(bytes)
    }

    /// Checks the header and decodes the bundle; refuses another client build.
    pub fn from_bytes(bytes: &[u8], expected_build: &str) -> Result<Self, BundleError> {
        let (header, body) = bytes
            .split_at_checked(HEADER_SIZE)
            .ok_or(BundleError::Truncated)?;
        let (magic, rest) = header.split_at(MAGIC_SIZE);
        if magic != BUNDLE_MAGIC {
            return Err(BundleError::BadMagic);
        }
        let (version, checksum) = rest.split_at(VERSION_SIZE);
        let version = u16::from_le_bytes(version.try_into().map_err(|_| BundleError::Truncated)?);
        if version != BUNDLE_FORMAT_VERSION {
            return Err(BundleError::UnsupportedVersion { found: version });
        }
        let expected = u32::from_le_bytes(checksum.try_into().map_err(|_| BundleError::Truncated)?);
        let actual = crc32fast::hash(body);
        if actual != expected {
            return Err(BundleError::ChecksumMismatch { expected, actual });
        }
        let bundle: Self = postcard::from_bytes(body).map_err(BundleError::Decode)?;
        if bundle.client_build != expected_build {
            return Err(BundleError::BuildMismatch {
                expected: expected_build.to_owned(),
                found: bundle.client_build,
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: &str = "22469132";
    const VERSION_OFFSET: usize = MAGIC_SIZE;

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

    #[test]
    fn build_mismatch_is_refused() {
        let bytes = bundle().to_bytes().unwrap();
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, "1"),
            Err(BundleError::BuildMismatch { expected, found }) if expected == "1" && found == BUILD
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
