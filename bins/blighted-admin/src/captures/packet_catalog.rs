// SPDX-License-Identifier: AGPL-3.0-or-later
//! Message type labels: our class names first (client type table joined with the
//! Aeternum-World names), then the packet catalog archive (`types.json`, saved by
//! `local-data/packet-catalog/archive.py` from the public API; idx = the wire type index).

use std::collections::HashMap;
use std::path::Path;

use nw_capture::TypeNames;
use serde::{Deserialize, Serialize};

/// The type list of the packet catalog archive.
pub const CATALOG_TYPES_FILE: &str = "types.json";

/// What the packet catalog site says about one type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogType {
    /// Class name.
    pub name: String,
    /// `solved`, `captured` or `wanted`.
    pub status: String,
    /// Their one-line description.
    pub what: String,
}

#[derive(Deserialize)]
struct CatalogFile {
    types: Vec<CatalogRow>,
}

#[derive(Deserialize)]
struct CatalogRow {
    idx: u64,
    name: String,
    st: String,
    #[serde(default)]
    what: String,
}

/// The packet catalog site types by idx.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogTypes(HashMap<u64, CatalogType>);

impl CatalogTypes {
    /// Reads `types.json` of `dir`; empty (with a warning) when it cannot be read.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(CATALOG_TYPES_FILE);
        let parsed = std::fs::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<CatalogFile>(&bytes).map_err(|error| error.to_string())
            });
        match parsed {
            Ok(file) => Self(
                file.types
                    .into_iter()
                    .map(|row| {
                        let label = CatalogType {
                            name: row.name,
                            status: row.st,
                            what: row.what,
                        };
                        (row.idx, label)
                    })
                    .collect(),
            ),
            Err(error) => {
                tracing::warn!(file = %path.display(), %error, "packet catalog types skipped");
                Self::default()
            }
        }
    }

    /// The type of `idx`.
    pub fn get(&self, idx: u64) -> Option<&CatalogType> {
        self.0.get(&idx)
    }
}

/// The label of one type index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TypeLabel {
    /// Class name: ours, else the packet catalog's.
    pub name: Option<String>,
    /// What the packet catalog site says.
    pub catalog: Option<CatalogType>,
}

/// Our names and the packet catalog site types.
#[derive(Debug, Default)]
pub struct TypeLabels {
    ours: Option<TypeNames>,
    catalog: CatalogTypes,
}

impl TypeLabels {
    /// Labels from our names (when the install's type table was found) and the packet catalog's.
    pub fn new(ours: Option<TypeNames>, catalog: CatalogTypes) -> Self {
        Self { ours, catalog }
    }

    /// The label of `idx`.
    pub fn label(&self, idx: u64) -> TypeLabel {
        let catalog = self.catalog.get(idx).cloned();
        let ours = self
            .ours
            .as_ref()
            .and_then(|names| names.name(idx))
            .map(str::to_owned);
        TypeLabel {
            name: ours.or_else(|| catalog.as_ref().map(|found| found.name.clone())),
            catalog,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG_TYPES: &str = r#"{"counts":{},"types":[
        {"idx":8,"name":"ReplicatedStateBundle","st":"solved","spec":true,"cat":"msg","dir":"s2c","act":"Movement","what":"Entity states."},
        {"idx":349,"name":"REPClient::PingMsg","st":"captured","spec":false,"cat":"msg","dir":"both","act":"Other","what":"A ping."}
    ]}"#;

    fn catalog_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(dir.path().join(CATALOG_TYPES_FILE), CATALOG_TYPES).expect("types");
        dir
    }

    #[test]
    fn catalog_types_load_by_idx() {
        let dir = catalog_dir();
        let types = CatalogTypes::load(dir.path());
        let ping = types.get(349).expect("ping");
        assert_eq!(
            (ping.name.as_str(), ping.status.as_str(), ping.what.as_str()),
            ("REPClient::PingMsg", "captured", "A ping.")
        );
        assert!(types.get(1).is_none());
    }

    #[test]
    fn a_missing_catalog_folder_gives_no_labels() {
        let dir = tempfile::tempdir().expect("dir");
        assert!(
            CatalogTypes::load(&dir.path().join("missing"))
                .get(8)
                .is_none()
        );
    }

    #[test]
    fn our_type_name_comes_before_the_catalog() {
        let dir = catalog_dir();
        let labels = TypeLabels::new(None, CatalogTypes::load(dir.path()));
        let label = labels.label(8);
        assert_eq!(label.name.as_deref(), Some("ReplicatedStateBundle"));
        assert_eq!(
            label.catalog.map(|catalog| catalog.status),
            Some("solved".to_owned())
        );
        assert_eq!(labels.label(1), TypeLabel::default());
    }
}
