// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads datasheets from a New World install, in a fixed order.

use std::path::{Path, PathBuf};

use nw_formats::datasheet::{DATASHEET_EXTENSION, Datasheet, DatasheetError};
use nw_formats::loc::{LOC_EXTENSION, LocError, LocFile, parse_loc_xml};
use nw_formats::pak::{PakArchive, PakError, find_paks};
use nw_oodle::KrakenDecompressor;

/// Path prefix of the English localization files inside the paks.
pub const EN_US_TEXT_PREFIX: &str = "localization/en-us/";

/// Error from reading the game install.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// A pak cannot be read.
    #[error(transparent)]
    Pak(#[from] PakError),
    /// A datasheet cannot be parsed.
    #[error("{path}: {source}")]
    Datasheet {
        /// Entry path.
        path: String,
        /// Parser error.
        #[source]
        source: DatasheetError,
    },
    /// A localization file cannot be parsed.
    #[error("{path}: {source}")]
    Loc {
        /// Entry path.
        path: String,
        /// Parser error.
        #[source]
        source: LocError,
    },
}

/// The paks of one game install.
#[derive(Debug, Clone)]
pub struct GameSource {
    paks: Vec<PathBuf>,
}

impl GameSource {
    /// Finds every pak under `<game_dir>/assets`.
    pub fn open(game_dir: &Path) -> Result<Self, SourceError> {
        Ok(Self {
            paks: find_paks(game_dir)?,
        })
    }

    /// Calls `visit` for every datasheet, in sorted pak order and sorted entry path order.
    /// Returns the number of datasheets.
    pub fn for_each_datasheet<E: From<SourceError>>(
        &self,
        mut visit: impl FnMut(&str, Datasheet) -> Result<(), E>,
    ) -> Result<usize, E> {
        self.for_each_entry(
            |path| path.ends_with(DATASHEET_EXTENSION),
            |path, bytes| {
                let sheet = Datasheet::parse(&bytes).map_err(|source| SourceError::Datasheet {
                    path: path.to_owned(),
                    source,
                })?;
                visit(path, sheet)
            },
        )
    }

    /// Calls `visit` for every `localization/en-us/*.loc.xml` file, in the same order rules as
    /// [`GameSource::for_each_datasheet`]. Returns the number of files.
    pub fn for_each_en_us_text<E: From<SourceError>>(
        &self,
        mut visit: impl FnMut(&str, LocFile) -> Result<(), E>,
    ) -> Result<usize, E> {
        self.for_each_entry(
            |path| path.starts_with(EN_US_TEXT_PREFIX) && path.ends_with(LOC_EXTENSION),
            |path, bytes| {
                let file = parse_loc_xml(&bytes).map_err(|source| SourceError::Loc {
                    path: path.to_owned(),
                    source,
                })?;
                visit(path, file)
            },
        )
    }

    /// Calls `visit` for every entry whose path passes `select`, in sorted pak order and sorted
    /// entry path order. Returns the number of entries.
    pub fn for_each_entry<E: From<SourceError>>(
        &self,
        select: impl Fn(&str) -> bool,
        mut visit: impl FnMut(&str, Vec<u8>) -> Result<(), E>,
    ) -> Result<usize, E> {
        let mut count = 0;
        for pak_path in &self.paks {
            let mut pak = PakArchive::open(pak_path).map_err(SourceError::from)?;
            let mut entries = pak.entries().map_err(SourceError::from)?;
            entries.retain(|entry| select(&entry.path));
            entries.sort_by(|left, right| left.path.cmp(&right.path));
            for entry in &entries {
                let bytes = pak
                    .read(entry, &KrakenDecompressor)
                    .map_err(SourceError::from)?;
                visit(&entry.path, bytes)?;
                count += 1;
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_without_assets_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            GameSource::open(dir.path()),
            Err(SourceError::Pak(PakError::Io { .. }))
        ));
    }

    #[test]
    fn empty_install_has_no_datasheets() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(nw_formats::pak::ASSETS_DIR)).unwrap();
        let source = GameSource::open(dir.path()).unwrap();
        let count = source
            .for_each_datasheet(|_, _| Ok::<(), SourceError>(()))
            .unwrap();
        assert_eq!(count, 0);
    }
}
