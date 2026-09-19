// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads every design file of a design folder.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{DesignError, Problem};
use crate::file::{Action, DesignFile, DesignSet, Kind};

/// The design file schema this program reads (`schema = 1`).
pub const DESIGN_SCHEMA: i64 = 1;
/// The to-do list at the root of the design folder; it is not a design file.
pub const TODO_FILE: &str = "todo.toml";
/// Extension of design files.
pub(crate) const DESIGN_EXTENSION: &str = "toml";
/// Byte order mark that some editors put at the start of a file.
const BOM: char = '\u{feff}';
const SCHEMA_KEY: &str = "schema";
const KIND_KEY: &str = "kind";
const ACTION_KEY: &str = "action";
const ID_KEY: &str = "id";
const BASE_KEY: &str = "base";
const MAP_KEY: &str = "map";
/// Bytes of the text hash that make a file version.
const VERSION_BYTES: usize = 8;

/// The version of a design file's text: the start of its SHA-256, as hex.
pub fn file_version(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .take(VERSION_BYTES)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Reads every `*.toml` file under `dir` (except [`TODO_FILE`] at the root), sorted by kind,
/// id and path. A file that cannot be read gives an error and the others still load. A missing
/// folder is an empty set.
pub fn load_dir(dir: &Path) -> (DesignSet, Vec<DesignError>) {
    let mut relative_paths = Vec::new();
    let mut errors = Vec::new();
    collect_paths(dir, Path::new(""), &mut relative_paths, &mut errors);
    relative_paths.retain(|path| path != Path::new(TODO_FILE));
    relative_paths.sort();
    let mut files = Vec::new();
    for relative in relative_paths {
        match read_file(&dir.join(&relative), &relative) {
            Ok(file) => files.push(file),
            Err(error) => errors.push(error),
        }
    }
    sort_files(&mut files);
    (DesignSet { files }, errors)
}

/// Sorts design files by kind, id and path, the order every check and merge reads them in.
pub(crate) fn sort_files(files: &mut [DesignFile]) {
    files.sort_by(|left, right| {
        (left.kind, &left.id, &left.path).cmp(&(right.kind, &right.id, &right.path))
    });
}

/// Adds the `.toml` files under `dir/relative` to `out`, as `/`-separated relative paths.
fn collect_paths(
    dir: &Path,
    relative: &Path,
    out: &mut Vec<PathBuf>,
    errors: &mut Vec<DesignError>,
) {
    let Ok(entries) = std::fs::read_dir(dir.join(relative)) else {
        return;
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(DesignError::file(
                    relative,
                    Problem::Unreadable(error.to_string()),
                ));
                continue;
            }
        };
        let path = relative.join(entry.file_name());
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect_paths(dir, &path, out, errors);
        } else if path
            .extension()
            .is_some_and(|extension| extension == DESIGN_EXTENSION)
        {
            out.push(path);
        }
    }
}

fn read_file(full: &Path, relative: &Path) -> Result<DesignFile, DesignError> {
    let file_error = |problem| DesignError::file(relative, problem);
    let bytes =
        std::fs::read(full).map_err(|error| file_error(Problem::Unreadable(error.to_string())))?;
    let text = String::from_utf8(bytes)
        .map_err(|error| file_error(Problem::Unreadable(error.to_string())))?;
    parse_file(&text, relative)
}

/// The design file of `text`, found at `relative` in the design folder.
pub fn parse_file(text: &str, relative: &Path) -> Result<DesignFile, DesignError> {
    let file_error = |problem| DesignError::file(relative, problem);
    let mut table: toml::Table =
        text.trim_start_matches(BOM)
            .parse()
            .map_err(|error: toml::de::Error| {
                file_error(Problem::BadToml(error.message().to_owned()))
            })?;
    let schema = table
        .remove(SCHEMA_KEY)
        .and_then(|value| value.as_integer())
        .ok_or_else(|| file_error(Problem::MissingHeader(SCHEMA_KEY.to_owned())))?;
    if schema != DESIGN_SCHEMA {
        return Err(file_error(Problem::UnknownSchema(schema)));
    }
    let kind_name = take_string(&mut table, KIND_KEY)
        .ok_or_else(|| file_error(Problem::MissingHeader(KIND_KEY.to_owned())))?;
    let kind =
        Kind::from_name(&kind_name).ok_or_else(|| file_error(Problem::UnknownKind(kind_name)))?;
    let action_name = take_string(&mut table, ACTION_KEY)
        .ok_or_else(|| file_error(Problem::MissingHeader(ACTION_KEY.to_owned())))?;
    let action = Action::from_name(&action_name)
        .ok_or_else(|| file_error(Problem::UnknownAction(action_name)))?;
    let id = take_string(&mut table, ID_KEY)
        .ok_or_else(|| file_error(Problem::MissingHeader(ID_KEY.to_owned())))?;
    let base = take_string(&mut table, BASE_KEY);
    let map = take_string(&mut table, MAP_KEY);
    Ok(DesignFile {
        path: relative.to_path_buf(),
        kind,
        action,
        id,
        base,
        map,
        fields: table,
        version: file_version(text),
    })
}

fn take_string(table: &mut toml::Table, key: &str) -> Option<String> {
    match table.remove(key)? {
        toml::Value::String(text) => Some(text),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::error::Problem;
    use crate::file::{Action, Kind};

    pub(crate) fn write(dir: &Path, relative: &str, text: &str) {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    pub(crate) const LOOT_OVERRIDE: &str =
        "schema = 1\nkind = \"loot_table\"\naction = \"override\"\nid = \"LT_A\"\nmax_roll = 50\n";

    #[test]
    fn a_valid_override_loads() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "loot/tables/LT_A.toml", LOOT_OVERRIDE);
        let (set, errors) = load_dir(dir.path());
        assert_eq!(errors, []);
        assert_eq!(set.files.len(), 1);
        let file = &set.files[0];
        assert_eq!(file.path, Path::new("loot/tables/LT_A.toml"));
        assert_eq!(
            (file.kind, file.action),
            (Kind::LootTable, Action::Override)
        );
        assert_eq!(file.id, "LT_A");
        assert_eq!(file.fields["max_roll"].as_integer(), Some(50));
        assert!(!file.fields.contains_key("kind"));
    }

    #[test]
    fn crlf_and_bom_files_load() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!("\u{feff}{}", LOOT_OVERRIDE.replace('\n', "\r\n"));
        write(dir.path(), "loot/tables/LT_A.toml", &text);
        let (set, errors) = load_dir(dir.path());
        assert_eq!(errors, []);
        assert_eq!(set.files.len(), 1);
    }

    #[test]
    fn todo_and_readme_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), TODO_FILE, "[[task]]\n");
        write(dir.path(), "README.md", "# notes\n");
        let (set, errors) = load_dir(dir.path());
        assert_eq!((set.files.len(), errors.len()), (0, 0));
    }

    #[test]
    fn a_bad_toml_file_is_reported_and_others_still_load() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "loot/tables/LT_A.toml", LOOT_OVERRIDE);
        write(dir.path(), "loot/tables/LT_B.toml", "schema = = 1\n");
        let (set, errors) = load_dir(dir.path());
        assert_eq!(set.files.len(), 1);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, Path::new("loot/tables/LT_B.toml"));
        assert!(matches!(errors[0].problem, Problem::BadToml(_)));
    }

    #[test]
    fn header_problems_are_named() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "a.toml",
            "kind = \"loot_table\"\naction = \"override\"\nid = \"a\"\n",
        );
        write(
            dir.path(),
            "b.toml",
            "schema = 2\nkind = \"loot_table\"\naction = \"override\"\nid = \"b\"\n",
        );
        write(
            dir.path(),
            "c.toml",
            "schema = 1\nkind = \"sword\"\naction = \"override\"\nid = \"c\"\n",
        );
        write(
            dir.path(),
            "d.toml",
            "schema = 1\nkind = \"loot_table\"\naction = \"copy\"\nid = \"d\"\n",
        );
        let (set, errors) = load_dir(dir.path());
        assert!(set.files.is_empty());
        let problems: Vec<_> = errors.into_iter().map(|error| error.problem).collect();
        assert_eq!(
            problems,
            [
                Problem::MissingHeader("schema".to_owned()),
                Problem::UnknownSchema(2),
                Problem::UnknownKind("sword".to_owned()),
                Problem::UnknownAction("copy".to_owned()),
            ]
        );
    }

    #[test]
    fn files_are_sorted_by_kind_then_id() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "loot/tables/LT_B.toml",
            &LOOT_OVERRIDE.replace("LT_A", "LT_B"),
        );
        write(dir.path(), "loot/tables/LT_A.toml", LOOT_OVERRIDE);
        write(
            dir.path(),
            "items/I.toml",
            "schema = 1\nkind = \"item\"\naction = \"disable\"\nid = \"I\"\n",
        );
        let (set, _) = load_dir(dir.path());
        let ids: Vec<_> = set.files.iter().map(|file| file.id.as_str()).collect();
        assert_eq!(ids, ["I", "LT_A", "LT_B"]);
    }
}
