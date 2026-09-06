// SPDX-License-Identifier: AGPL-3.0-or-later
//! Message type labels: our class names first (client type table joined with the
//! Aeternum-World names), then the Azoth catalog archive (`types.json`, saved by
//! `local-data/azoth-catalog/archive.py` from the public API; idx = the wire type index).

use std::collections::HashMap;
use std::path::Path;

use nw_capture::TypeNames;
use serde::{Deserialize, Serialize};

/// The type list of the Azoth catalog archive.
pub const AZOTH_TYPES_FILE: &str = "types.json";

/// What the Azoth catalog site says about one type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AzothType {
    /// Class name.
    pub name: String,
    /// `solved`, `captured` or `wanted`.
    pub status: String,
    /// Their one-line description.
    pub what: String,
}

#[derive(Deserialize)]
struct AzothFile {
    types: Vec<AzothRow>,
}

#[derive(Deserialize)]
struct AzothRow {
    idx: u64,
    name: String,
    st: String,
    #[serde(default)]
    what: String,
}

/// The Azoth catalog site types by idx.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AzothTypes(HashMap<u64, AzothType>);

impl AzothTypes {
    /// Reads `types.json` of `dir`; empty (with a warning) when it cannot be read.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(AZOTH_TYPES_FILE);
        let parsed = std::fs::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<AzothFile>(&bytes).map_err(|error| error.to_string())
            });
        match parsed {
            Ok(file) => Self(
                file.types
                    .into_iter()
                    .map(|row| {
                        let label = AzothType {
                            name: row.name,
                            status: row.st,
                            what: row.what,
                        };
                        (row.idx, label)
                    })
                    .collect(),
            ),
            Err(error) => {
                tracing::warn!(file = %path.display(), %error, "Azoth catalog types skipped");
                Self::default()
            }
        }
    }

    /// The type of `idx`.
    pub fn get(&self, idx: u64) -> Option<&AzothType> {
        self.0.get(&idx)
    }
}

/// The label of one type index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TypeLabel {
    /// Class name: ours, else the Azoth catalog's.
    pub name: Option<String>,
    /// What the Azoth catalog site says.
    pub azoth: Option<AzothType>,
}

/// Our names and the Azoth catalog site types.
#[derive(Debug, Default)]
pub struct TypeLabels {
    ours: Option<TypeNames>,
    azoth: AzothTypes,
}

impl TypeLabels {
    /// Labels from our names (when the install's type table was found) and the Azoth catalog's.
    pub fn new(ours: Option<TypeNames>, azoth: AzothTypes) -> Self {
        Self { ours, azoth }
    }

    /// The label of `idx`.
    pub fn label(&self, idx: u64) -> TypeLabel {
        let azoth = self.azoth.get(idx).cloned();
        let ours = self
            .ours
            .as_ref()
            .and_then(|names| names.name(idx))
            .map(str::to_owned);
        TypeLabel {
            name: ours.or_else(|| azoth.as_ref().map(|found| found.name.clone())),
            azoth,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AZOTH_TYPES: &str = r#"{"counts":{},"types":[
        {"idx":8,"name":"ReplicatedStateBundle","st":"solved","spec":true,"cat":"msg","dir":"s2c","act":"Movement","what":"Entity states."},
        {"idx":349,"name":"REPClient::PingMsg","st":"captured","spec":false,"cat":"msg","dir":"both","act":"Other","what":"A ping."}
    ]}"#;

    fn azoth_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(dir.path().join(AZOTH_TYPES_FILE), AZOTH_TYPES).expect("types");
        dir
    }

    #[test]
    fn azoth_types_load_by_idx() {
        let dir = azoth_dir();
        let types = AzothTypes::load(dir.path());
        let ping = types.get(349).expect("ping");
        assert_eq!(
            (ping.name.as_str(), ping.status.as_str(), ping.what.as_str()),
            ("REPClient::PingMsg", "captured", "A ping.")
        );
        assert!(types.get(1).is_none());
    }

    #[test]
    fn a_missing_azoth_folder_gives_no_labels() {
        let dir = tempfile::tempdir().expect("dir");
        assert!(
            AzothTypes::load(&dir.path().join("missing"))
                .get(8)
                .is_none()
        );
    }

    #[test]
    fn our_type_name_comes_before_azoth() {
        let dir = azoth_dir();
        let labels = TypeLabels::new(None, AzothTypes::load(dir.path()));
        let label = labels.label(8);
        assert_eq!(label.name.as_deref(), Some("ReplicatedStateBundle"));
        assert_eq!(
            label.azoth.map(|azoth| azoth.status),
            Some("solved".to_owned())
        );
        assert_eq!(labels.label(1), TypeLabel::default());
    }
}
