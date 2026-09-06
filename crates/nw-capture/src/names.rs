// SPDX-License-Identifier: AGPL-3.0-or-later
//! Class names by type index.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::CaptureError;

/// Class names by type index, from the client type table and the Aeternum-World name map.
#[derive(Debug, Default)]
pub struct TypeNames {
    names: Vec<Option<String>>,
}

#[derive(Deserialize)]
struct TypeIndexFile {
    #[serde(rename = "typeIndex")]
    type_index: Vec<String>,
}

#[derive(Deserialize)]
struct NameMapFile {
    entries: HashMap<String, NameEntry>,
}

#[derive(Deserialize)]
struct NameEntry {
    name: Option<String>,
}

impl TypeNames {
    /// Joins `typeindex.json` (list of UUIDs) and `uuid_to_class.json` (UUID to name) by UUID.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] when a file cannot be read or parsed.
    pub fn load(type_index: &Path, name_map: &Path) -> Result<Self, CaptureError> {
        let type_index: TypeIndexFile = read_json(type_index)?;
        let name_map: NameMapFile = read_json(name_map)?;
        let by_uuid: HashMap<String, String> = name_map
            .entries
            .into_iter()
            .filter_map(|(uuid, entry)| entry.name.map(|name| (normalize_uuid(&uuid), name)))
            .collect();
        Ok(Self {
            names: type_index
                .type_index
                .iter()
                .map(|uuid| by_uuid.get(&normalize_uuid(uuid)).cloned())
                .collect(),
        })
    }

    /// The class name of a type index.
    pub fn name(&self, index: u64) -> Option<&str> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.names.get(index))
            .and_then(Option::as_deref)
    }
}

fn normalize_uuid(uuid: &str) -> String {
    uuid.chars()
        .filter(char::is_ascii_hexdigit)
        .map(|digit| digit.to_ascii_uppercase())
        .collect()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, CaptureError> {
    let bytes = std::fs::read(path).map_err(|source| CaptureError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| CaptureError::Json {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMED_SLOT: u64 = 2;
    const UNNAMED_SLOT: u64 = 1;

    #[test]
    fn joins_type_names_by_uuid() {
        let dir = tempfile::tempdir().expect("dir");
        let type_index = dir.path().join("typeindex.json");
        let names = dir.path().join("uuid_to_class.json");
        std::fs::write(
            &type_index,
            r#"{"typeIndex":["00000000000000000000000000000000","AAAAAAAA000000000000000000000001","6A379FB80BDD43A1AB3E9843D7BE8CD3"]}"#,
        )
        .expect("write");
        std::fs::write(
            &names,
            r#"{"entries":{"6a379fb8-0bdd-43a1-ab3e-9843d7be8cd3":{"name":"REPClient::PingMsg"}}}"#,
        )
        .expect("write");
        let names = TypeNames::load(&type_index, &names).expect("load");
        assert_eq!(names.name(NAMED_SLOT), Some("REPClient::PingMsg"));
        assert_eq!(names.name(UNNAMED_SLOT), None);
        assert_eq!(names.name(u64::MAX), None);
    }
}
