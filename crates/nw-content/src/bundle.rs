// SPDX-License-Identifier: AGPL-3.0-or-later
//! The content bundle file (M1 spec section 5), format version 2: the shared header of
//! [`crate::container`] with magic `NWCB` and the generated [`SCHEMA_HASH`], then a postcard
//! body (tables and text).
//!
//! Loading checks every header field before it decodes the body.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::container::{
    ContainerError, ContainerSpec, decode_container, encode_container, read_file, write_file,
};
use crate::generated::{SCHEMA_HASH, Tables};

/// First 4 bytes of every bundle.
pub const BUNDLE_MAGIC: [u8; 4] = *b"NWCB";
/// Current bundle format version.
pub const BUNDLE_FORMAT_VERSION: u16 = 2;
/// Header fields of the bundle.
pub const BUNDLE_CONTAINER: ContainerSpec = ContainerSpec {
    magic: BUNDLE_MAGIC,
    format_version: BUNDLE_FORMAT_VERSION,
    schema_hash: SCHEMA_HASH,
};

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
    /// The header is not valid or the file cannot be accessed.
    #[error("content bundle: {0}")]
    Container(#[from] ContainerError),
    /// The content cannot be encoded.
    #[error("cannot encode bundle: {0}")]
    Encode(postcard::Error),
    /// The body cannot be decoded.
    #[error("cannot decode bundle: {0}")]
    Decode(postcard::Error),
    /// The body has bytes after the encoded content.
    #[error("bundle body has {count} bytes after the content")]
    TrailingBytes {
        /// Number of extra bytes.
        count: usize,
    },
}

impl ContentBundle {
    /// Encodes the bundle with its header.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BundleError> {
        let body = postcard::to_allocvec(self).map_err(BundleError::Encode)?;
        Ok(encode_container(
            &BUNDLE_CONTAINER,
            &self.client_build,
            &body,
        )?)
    }

    /// Checks the header, then decodes the bundle. Refuses another format version, other
    /// generated table code ([`SCHEMA_HASH`]) and another client build before decoding.
    pub fn from_bytes(bytes: &[u8], expected_build: &str) -> Result<Self, BundleError> {
        let body = decode_container(&BUNDLE_CONTAINER, bytes, expected_build)?;
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
        Ok(write_file(path, &self.to_bytes()?)?)
    }

    /// Reads and checks the bundle at `path`.
    pub fn load(path: &Path, expected_build: &str) -> Result<Self, BundleError> {
        Self::from_bytes(&read_file(path)?, expected_build)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: &str = "22469132";
    const OTHER_BUILD: &str = "1";
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
        assert_eq!(&bytes[..BUNDLE_MAGIC.len()], &BUNDLE_MAGIC);
        assert_eq!(ContentBundle::from_bytes(&bytes, BUILD).unwrap(), bundle());
    }

    #[test]
    fn header_errors_come_from_the_container() {
        let bytes = bundle().to_bytes().unwrap();
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, OTHER_BUILD),
            Err(BundleError::Container(ContainerError::BuildMismatch { .. }))
        ));
    }

    #[test]
    fn other_generated_code_is_refused() {
        let body = postcard::to_allocvec(&bundle()).unwrap();
        let spec = ContainerSpec {
            schema_hash: SCHEMA_HASH ^ 1,
            ..BUNDLE_CONTAINER
        };
        let bytes = encode_container(&spec, BUILD, &body).unwrap();
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, BUILD),
            Err(BundleError::Container(
                ContainerError::SchemaMismatch { .. }
            ))
        ));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut body = postcard::to_allocvec(&bundle()).unwrap();
        body.push(EXTRA_BYTE);
        let bytes = encode_container(&BUNDLE_CONTAINER, BUILD, &body).unwrap();
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, BUILD),
            Err(BundleError::TrailingBytes { count: 1 })
        ));
    }

    #[test]
    fn body_that_is_not_a_bundle_is_refused() {
        let bytes = encode_container(&BUNDLE_CONTAINER, BUILD, &[]).unwrap();
        assert!(matches!(
            ContentBundle::from_bytes(&bytes, BUILD),
            Err(BundleError::Decode(_))
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
