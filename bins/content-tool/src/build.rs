// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool build`: writes the content bundle, then reloads it and checks the counts.

use std::collections::btree_map::Entry;
use std::path::{Path, PathBuf};

use nw_content::ContentBundle;

use crate::error::ToolError;
use crate::source::GameSource;

/// Folder for bundles and world files. It is inside `local-data/`, which git ignores.
pub const DEFAULT_CONTENT_DIR: &str = "local-data/content";

/// Result of [`run_build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSummary {
    /// The bundle file.
    pub path: PathBuf,
    /// Datasheets read (one table each).
    pub datasheet_files: usize,
    /// Rows over all datasheets.
    pub rows: usize,
    /// en-us text files read.
    pub text_files: usize,
    /// Text entries in the bundle.
    pub text_entries: usize,
    /// Later values of a key that was already present (skipped; the first value is kept).
    pub duplicate_text_keys: usize,
    /// `<string>` elements without a key (skipped).
    pub keyless_text_entries: usize,
}

/// The default bundle path for a client build.
pub fn default_bundle_path(client_build: &str) -> PathBuf {
    Path::new(DEFAULT_CONTENT_DIR).join(format!("bundle-{client_build}.bin"))
}

/// Builds the bundle from `source`, writes it to `out`, reloads it and checks the counts.
pub fn run_build(
    source: &GameSource,
    client_build: &str,
    out: &Path,
) -> Result<BuildSummary, ToolError> {
    let mut bundle = ContentBundle {
        client_build: client_build.to_owned(),
        ..ContentBundle::default()
    };
    let mut rows = 0;
    let datasheet_files = source.for_each_datasheet(|path, sheet| {
        rows += sheet.rows.len();
        bundle.tables.insert_datasheet(path, &sheet)?;
        Ok::<(), ToolError>(())
    })?;
    let mut duplicate_text_keys = 0;
    let mut keyless_text_entries = 0;
    let text_files = source.for_each_en_us_text(|_, file| {
        keyless_text_entries += file.keyless;
        for (key, text) in file.entries {
            match bundle.text.entry(key) {
                Entry::Vacant(slot) => {
                    slot.insert(text);
                }
                Entry::Occupied(_) => duplicate_text_keys += 1,
            }
        }
        Ok::<(), ToolError>(())
    })?;
    bundle.save(out)?;
    let reloaded = ContentBundle::load(out, client_build)?;
    let counts = reloaded.tables.counts();
    check_count("tables", datasheet_files, counts.tables)?;
    check_count("rows", rows, counts.rows)?;
    check_count("text entries", bundle.text.len(), reloaded.text.len())?;
    Ok(BuildSummary {
        path: out.to_path_buf(),
        datasheet_files,
        rows,
        text_files,
        text_entries: reloaded.text.len(),
        duplicate_text_keys,
        keyless_text_entries,
    })
}

fn check_count(what: &'static str, expected: usize, found: usize) -> Result<(), ToolError> {
    if expected == found {
        Ok(())
    } else {
        Err(ToolError::CountMismatch {
            what,
            expected,
            found,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bundle_path_is_in_local_data() {
        assert_eq!(
            default_bundle_path("22469132"),
            PathBuf::from("local-data/content/bundle-22469132.bin")
        );
    }

    #[test]
    fn count_mismatch_is_an_error() {
        assert!(check_count("rows", 2, 2).is_ok());
        assert!(matches!(
            check_count("rows", 2, 1),
            Err(ToolError::CountMismatch {
                what: "rows",
                expected: 2,
                found: 1
            })
        ));
    }
}
