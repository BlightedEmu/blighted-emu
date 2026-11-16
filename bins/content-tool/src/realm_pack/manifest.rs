// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pack's manifest (`manifest.json`), its entry names and the checksums. The tables the pack
//! holds are the capture-made files of the content registry
//! ([`nw_content::layout::pack_files`]); no game install can make them.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::PackError;

/// The pack format this code makes and reads. Any change to how a pack table or its replicated
/// states are read bumps it in the same change, and a new pack is published.
pub const PACK_FORMAT: u32 = 1;

/// The manifest's name in the zip.
pub const MANIFEST_FILE: &str = "manifest.json";
/// The checksum file next to the zip.
pub const SUMS_FILE: &str = "SHA256SUMS";
/// The review report next to the zip.
pub const REPORT_FILE: &str = "scan-report.txt";
/// The zip name prefix and extension.
const ZIP_PREFIX: &str = "realm-data-";
const ZIP_EXTENSION: &str = ".zip";
/// The table file extension.
const TABLE_EXTENSION: &str = ".json";

/// The manifest of a pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The pack format ([`PACK_FORMAT`] of the code that made it).
    pub pack_format: u32,
    /// The pack version (one up per published pack).
    pub pack_version: u32,
    /// The client build of the tables.
    pub client_build: String,
    /// The commit of the server code whose loaders read these tables.
    pub source_commit: String,
    /// One entry per table file.
    pub files: Vec<PackFile>,
}

/// One table file of a pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackFile {
    /// The entry name (`<table>-<build>.json`, the table's registry name).
    pub name: String,
    /// The file size.
    pub bytes: u64,
    /// The SHA-256 of the file, lower-case hex.
    pub sha256: String,
}

/// The zip entry name of `table` for `client_build`: `<table>-<build>.json` (the name of the flat
/// content folder the pack format was made with).
pub fn table_file_name(table: &str, client_build: &str) -> String {
    format!("{table}-{client_build}{TABLE_EXTENSION}")
}

/// The zip name for `client_build`: `realm-data-<build>.zip`.
pub fn pack_zip_name(client_build: &str) -> String {
    format!("{ZIP_PREFIX}{client_build}{ZIP_EXTENSION}")
}

/// The SHA-256 of `bytes`, lower-case hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Checks that a pack fits this server: same client build, same pack format.
///
/// # Errors
///
/// Returns [`PackError::WrongBuild`] or [`PackError::WrongFormat`].
pub fn check_manifest(manifest: &Manifest, client_build: &str) -> Result<(), PackError> {
    if manifest.client_build != client_build {
        return Err(PackError::WrongBuild {
            pack: manifest.client_build.clone(),
            server: client_build.to_owned(),
        });
    }
    if manifest.pack_format != PACK_FORMAT {
        return Err(PackError::WrongFormat {
            pack: manifest.pack_format,
            code: PACK_FORMAT,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const BUILD: &str = "12345";

    fn manifest() -> Manifest {
        Manifest {
            pack_format: PACK_FORMAT,
            pack_version: 3,
            client_build: BUILD.to_owned(),
            source_commit: "0123abcd".to_owned(),
            files: vec![PackFile {
                name: table_file_name("mob-prefabs", BUILD),
                bytes: 10,
                sha256: sha256_hex(b"x"),
            }],
        }
    }

    #[test]
    fn manifest_round_trips() {
        let text = serde_json::to_string(&manifest()).unwrap();
        assert_eq!(serde_json::from_str::<Manifest>(&text).unwrap(), manifest());
    }

    #[test]
    fn wrong_build_is_refused() {
        assert!(matches!(
            check_manifest(&manifest(), "54321"),
            Err(PackError::WrongBuild { .. })
        ));
        assert!(check_manifest(&manifest(), BUILD).is_ok());
    }

    #[test]
    fn other_format_is_refused() {
        let mut other = manifest();
        other.pack_format = PACK_FORMAT + 1;
        assert!(matches!(
            check_manifest(&other, BUILD),
            Err(PackError::WrongFormat { .. })
        ));
    }

    #[test]
    fn unknown_manifest_key_is_refused() {
        let mut value = serde_json::to_value(manifest()).unwrap();
        value["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }

    #[test]
    fn sha256_of_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn entry_names_are_unique_and_name_the_build() {
        let unique: BTreeSet<_> = nw_content::layout::pack_files()
            .map(|file| table_file_name(file.name, BUILD))
            .collect();
        assert_eq!(unique.len(), nw_content::layout::pack_files().count());
        assert_eq!(table_file_name("war-forts", BUILD), "war-forts-12345.json");
        assert_eq!(pack_zip_name(BUILD), "realm-data-12345.zip");
    }
}
