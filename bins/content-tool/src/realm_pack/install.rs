// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool realm-pack install`: checks a pack zip in full, then writes its tables into a
//! content folder. Nothing is written when a check fails, and a table of the folder that differs
//! from the pack's is kept unless the caller asks to replace it.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use super::make::parse_pack_table;
use super::manifest::{
    MANIFEST_FILE, Manifest, PACK_TABLES, check_manifest, sha256_hex, table_file_name,
};
use super::owner::collect_identities;
use super::{PackError, io_error};

/// The temporary name of a table while it is written: `.<name>.part`.
const PART_PREFIX: &str = ".";
const PART_SUFFIX: &str = ".part";

/// One checked table of a pack.
struct PackTable {
    /// The table.
    table: &'static str,
    /// Its file name.
    name: String,
    /// Its bytes, checked against the manifest.
    bytes: Vec<u8>,
}

/// What the install did with one table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableOutcome {
    /// The folder had no such table; the pack's was written.
    Written,
    /// The folder's table is the pack's; nothing was written.
    Same,
    /// The folder's table differed and was replaced (`--replace`).
    Replaced,
    /// The folder's table differs and was kept (no `--replace`).
    Kept,
}

/// What an install did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    /// Each table and what was done with it.
    pub tables: Vec<(String, TableOutcome)>,
    /// Replicated members of a class this code has no reader for (not checked).
    pub unknown_class_members: usize,
}

/// Installs the pack `zip` into `content_dir`.
///
/// # Errors
///
/// Returns [`PackError`] when the pack is for another build or format, a file is missing, extra
/// or changed, or a table does not load or a member does not decode
/// ([`PackError::PackDoesNotFit`]). Nothing is written then.
pub fn install_pack(
    zip: &Path,
    content_dir: &Path,
    client_build: &str,
    replace: bool,
) -> Result<InstallReport, PackError> {
    let mut entries = read_entries(zip)?;
    let manifest_bytes = entries
        .remove(MANIFEST_FILE)
        .ok_or_else(|| PackError::MissingFile {
            file: MANIFEST_FILE.to_owned(),
        })?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    check_manifest(&manifest, client_build)?;
    let tables = checked_tables(&manifest, &mut entries, client_build)?;
    if let Some(extra) = entries.into_keys().next() {
        return Err(PackError::UnknownFile { file: extra });
    }
    let mut unknown_class_members = 0;
    for PackTable { table, bytes, .. } in &tables {
        unknown_class_members +=
            check_table(table, bytes).map_err(|reason| PackError::PackDoesNotFit {
                pack_version: manifest.pack_version,
                source_commit: manifest.source_commit.clone(),
                reason: Box::new(reason),
            })?;
    }
    let outcomes = plan_writes(content_dir, &tables, replace)?;
    std::fs::create_dir_all(content_dir).map_err(io_error(content_dir))?;
    for (PackTable { name, bytes, .. }, (_, outcome)) in tables.iter().zip(&outcomes) {
        if matches!(outcome, TableOutcome::Same | TableOutcome::Kept) {
            continue;
        }
        let part = content_dir.join(format!("{PART_PREFIX}{name}{PART_SUFFIX}"));
        std::fs::write(&part, bytes).map_err(io_error(&part))?;
        let target = content_dir.join(name);
        std::fs::rename(&part, &target).map_err(io_error(&target))?;
    }
    Ok(InstallReport {
        tables: outcomes,
        unknown_class_members,
    })
}

/// Checks that table `table` loads and that its replicated states decode; the members of a class
/// with no reader.
fn check_table(table: &str, bytes: &[u8]) -> Result<usize, PackError> {
    let text = std::str::from_utf8(bytes).map_err(|error| PackError::TableLoad {
        table: table.to_owned(),
        reason: error.to_string(),
    })?;
    parse_pack_table(table, text)?;
    let (visit, _) = collect_identities(table, &serde_json::from_str(text)?)?;
    Ok(visit.unknown_class_members)
}

/// Every entry of the zip by name. A folder or a name with a folder in it is refused.
fn read_entries(zip: &Path) -> Result<BTreeMap<String, Vec<u8>>, PackError> {
    let file = std::fs::File::open(zip).map_err(io_error(zip))?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name()?.into_owned();
        if entry.is_dir() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(PackError::UnknownFile { file: name });
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(io_error(zip))?;
        entries.insert(name, bytes);
    }
    Ok(entries)
}

/// The pack tables with their file names and checked bytes, taken out of `entries`.
fn checked_tables(
    manifest: &Manifest,
    entries: &mut BTreeMap<String, Vec<u8>>,
    client_build: &str,
) -> Result<Vec<PackTable>, PackError> {
    let listed: BTreeMap<&str, _> = manifest
        .files
        .iter()
        .map(|file| (file.name.as_str(), file))
        .collect();
    let expected: BTreeSet<String> = PACK_TABLES
        .iter()
        .map(|table| table_file_name(table, client_build))
        .collect();
    if let Some(name) = listed.keys().find(|name| !expected.contains(**name)) {
        return Err(PackError::UnknownFile {
            file: (*name).to_owned(),
        });
    }
    PACK_TABLES
        .iter()
        .map(|&table| {
            let name = table_file_name(table, client_build);
            let file = listed
                .get(name.as_str())
                .ok_or_else(|| PackError::MissingFile { file: name.clone() })?;
            let bytes = entries
                .remove(&name)
                .ok_or_else(|| PackError::MissingFile { file: name.clone() })?;
            if bytes.len() as u64 != file.bytes || sha256_hex(&bytes) != file.sha256 {
                return Err(PackError::Checksum { file: name });
            }
            Ok(PackTable { table, name, bytes })
        })
        .collect()
}

/// What will be done with each table: a table that differs is kept unless `replace`.
fn plan_writes(
    content_dir: &Path,
    tables: &[PackTable],
    replace: bool,
) -> Result<Vec<(String, TableOutcome)>, PackError> {
    let mut outcomes = Vec::new();
    for PackTable { table, name, bytes } in tables {
        let path = content_dir.join(name);
        let outcome = match std::fs::read(&path) {
            Ok(existing) if existing == *bytes => TableOutcome::Same,
            Ok(_) if replace => TableOutcome::Replaced,
            Ok(_) => TableOutcome::Kept,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => TableOutcome::Written,
            Err(source) => return Err(PackError::Io { path, source }),
        };
        outcomes.push(((*table).to_owned(), outcome));
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::super::make::tests::{BUILD, make_fixture_pack};
    use super::super::manifest::pack_zip_name;
    use super::*;

    struct Fixture {
        _content: tempfile::TempDir,
        out: tempfile::TempDir,
        target: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let content = tempfile::tempdir().unwrap();
            let out = tempfile::tempdir().unwrap();
            make_fixture_pack(content.path(), out.path());
            Self {
                _content: content,
                out,
                target: tempfile::tempdir().unwrap(),
            }
        }

        fn zip(&self) -> std::path::PathBuf {
            self.out.path().join(pack_zip_name(BUILD))
        }

        fn install(&self, replace: bool) -> Result<InstallReport, PackError> {
            install_pack(&self.zip(), self.target.path(), BUILD, replace)
        }

        /// Rewrites the pack zip with `change` applied to its entries.
        fn rewrite(&self, change: impl FnOnce(&mut BTreeMap<String, Vec<u8>>)) {
            let mut entries = read_entries(&self.zip()).unwrap();
            change(&mut entries);
            let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
            for (name, bytes) in entries {
                zip.start_file(name, SimpleFileOptions::default()).unwrap();
                zip.write_all(&bytes).unwrap();
            }
            std::fs::write(self.zip(), zip.finish().unwrap().into_inner()).unwrap();
        }

        fn written(&self) -> usize {
            std::fs::read_dir(self.target.path()).map_or(0, Iterator::count)
        }
    }

    fn all(report: &InstallReport, outcome: TableOutcome) -> bool {
        report.tables.len() == PACK_TABLES.len()
            && report.tables.iter().all(|(_, done)| *done == outcome)
    }

    #[test]
    fn installs_into_an_empty_folder() {
        let fixture = Fixture::new();
        assert!(all(&fixture.install(false).unwrap(), TableOutcome::Written));
        assert_eq!(fixture.written(), PACK_TABLES.len(), "no part file is left");
    }

    #[test]
    fn makes_a_missing_content_folder() {
        let fixture = Fixture::new();
        let folder = fixture.target.path().join("local-data").join("content");
        install_pack(&fixture.zip(), &folder, BUILD, false).unwrap();
        assert_eq!(
            std::fs::read_dir(folder).unwrap().count(),
            PACK_TABLES.len()
        );
    }

    #[test]
    fn second_install_reports_same() {
        let fixture = Fixture::new();
        fixture.install(false).unwrap();
        assert!(all(&fixture.install(false).unwrap(), TableOutcome::Same));
    }

    #[test]
    fn keeps_different_tables_without_replace() {
        let fixture = Fixture::new();
        let mine = fixture
            .target
            .path()
            .join(table_file_name("war-forts", BUILD));
        std::fs::write(&mine, b"{\"mine\": true}").unwrap();
        let report = fixture.install(false).unwrap();
        assert!(
            report
                .tables
                .contains(&("war-forts".to_owned(), TableOutcome::Kept))
        );
        assert_eq!(std::fs::read(&mine).unwrap(), b"{\"mine\": true}");
        assert_eq!(
            fixture.written(),
            PACK_TABLES.len(),
            "the other tables are written"
        );
    }

    #[test]
    fn a_table_that_does_not_load_names_the_pack() {
        let fixture = Fixture::new();
        fixture.rewrite(|entries| {
            let name = table_file_name("spell-slices", BUILD);
            let bytes = b"{\"spells\": 1}".to_vec();
            let mut manifest: Manifest = serde_json::from_slice(&entries[MANIFEST_FILE]).unwrap();
            let file = manifest
                .files
                .iter_mut()
                .find(|file| file.name == name)
                .unwrap();
            file.bytes = bytes.len() as u64;
            file.sha256 = sha256_hex(&bytes);
            entries.insert(name, bytes);
            entries.insert(
                MANIFEST_FILE.to_owned(),
                serde_json::to_vec(&manifest).unwrap(),
            );
        });
        match fixture.install(false) {
            Err(PackError::PackDoesNotFit {
                pack_version,
                source_commit,
                ..
            }) => {
                assert_eq!((pack_version, source_commit.as_str()), (1, "0123abcd"));
            }
            other => panic!("expected a pack that does not fit, got {other:?}"),
        }
        assert_eq!(fixture.written(), 0);
    }

    #[test]
    fn replaces_with_replace() {
        let fixture = Fixture::new();
        fixture.install(false).unwrap();
        let mine = fixture
            .target
            .path()
            .join(table_file_name("war-forts", BUILD));
        std::fs::write(&mine, b"{}").unwrap();
        let report = fixture.install(true).unwrap();
        assert!(
            report
                .tables
                .contains(&("war-forts".to_owned(), TableOutcome::Replaced))
        );
        assert_ne!(std::fs::read(&mine).unwrap(), b"{}");
    }

    #[test]
    fn refuses_an_extra_zip_entry() {
        let fixture = Fixture::new();
        fixture.rewrite(|entries| {
            entries.insert("extra.json".to_owned(), b"{}".to_vec());
        });
        assert!(matches!(
            fixture.install(false),
            Err(PackError::UnknownFile { .. })
        ));
        assert_eq!(fixture.written(), 0);
    }

    #[test]
    fn refuses_a_path_entry() {
        let fixture = Fixture::new();
        fixture.rewrite(|entries| {
            entries.insert("../x.json".to_owned(), b"{}".to_vec());
        });
        assert!(matches!(
            fixture.install(false),
            Err(PackError::UnknownFile { .. })
        ));
        assert_eq!(fixture.written(), 0);
    }

    #[test]
    fn refuses_a_bad_checksum() {
        let fixture = Fixture::new();
        fixture.rewrite(|entries| {
            let name = table_file_name("spell-slices", BUILD);
            entries.insert(name, b"{\"spells\": [ ]}".to_vec());
        });
        assert!(matches!(
            fixture.install(false),
            Err(PackError::Checksum { .. })
        ));
        assert_eq!(fixture.written(), 0);
    }

    #[test]
    fn refuses_a_wrong_build() {
        let fixture = Fixture::new();
        assert!(matches!(
            install_pack(&fixture.zip(), fixture.target.path(), "54321", false),
            Err(PackError::WrongBuild { .. })
        ));
        assert_eq!(fixture.written(), 0);
    }
}
