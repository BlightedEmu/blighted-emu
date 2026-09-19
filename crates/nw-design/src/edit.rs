// SPDX-License-Identifier: AGPL-3.0-or-later
//! Edits of one design file from the Studio (admin studio spec section 7): a safe path, the
//! problems an edit would add to the set, and the write.

use std::path::{Component, Path, PathBuf};

use crate::error::DesignError;
use crate::file::DesignSet;
use crate::load::{DESIGN_EXTENSION, TODO_FILE, load_dir, parse_file, sort_files};
use crate::run::temp_path;
use crate::schema::check_schema;

/// The design file `raw` names, relative to the design folder: plain folder names only (no
/// `..`, no root), ending in `.toml`, and not the to-do list.
pub fn design_path(raw: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for component in Path::new(raw).components() {
        match component {
            Component::Normal(name) => relative.push(name),
            _ => return None,
        }
    }
    let is_design = relative
        .extension()
        .is_some_and(|extension| extension == DESIGN_EXTENSION);
    (is_design && relative != Path::new(TODO_FILE)).then_some(relative)
}

/// The problems that refuse writing `text` to `relative`: every problem of that file, and every
/// problem of another file that the write would add.
pub fn check_put(dir: &Path, relative: &Path, text: &str) -> Vec<DesignError> {
    blocking(dir, relative, Some(text))
}

/// The problems that refuse removing `relative`: the problems of other files that the removal
/// would add (a spawn group that names a removed path).
pub fn check_delete(dir: &Path, relative: &Path) -> Vec<DesignError> {
    blocking(dir, relative, None)
}

/// Writes `text` to `relative` under `dir`, through a temporary file renamed at the end, so a
/// reader sees the old file or the new one.
pub fn write_file(dir: &Path, relative: &Path, text: &str) -> std::io::Result<()> {
    let target = dir.join(relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = temp_path(&target);
    let written = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, &target));
    if written.is_err() {
        // A temporary file that was never made is not there.
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// Removes `relative` under `dir`.
pub fn delete_file(dir: &Path, relative: &Path) -> std::io::Result<()> {
    std::fs::remove_file(dir.join(relative))
}

/// The problems of the set with `relative` set to `text` (removed when `None`) that are about
/// `relative` or were not there before.
fn blocking(dir: &Path, relative: &Path, text: Option<&str>) -> Vec<DesignError> {
    let (set, load_errors) = load_dir(dir);
    let before = problems(&set, load_errors.clone());
    let mut files: Vec<_> = set
        .files
        .into_iter()
        .filter(|file| file.path != relative)
        .collect();
    let mut after_load: Vec<_> = load_errors
        .into_iter()
        .filter(|error| error.path != relative)
        .collect();
    if let Some(text) = text {
        match parse_file(text, relative) {
            Ok(file) => files.push(file),
            Err(error) => after_load.push(error),
        }
    }
    sort_files(&mut files);
    problems(&DesignSet { files }, after_load)
        .into_iter()
        .filter(|error| error.path == relative || !before.contains(error))
        .collect()
}

fn problems(set: &DesignSet, mut load_errors: Vec<DesignError>) -> Vec<DesignError> {
    load_errors.extend(check_schema(set));
    load_errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Problem;
    use crate::load::tests::write;

    const PROFILE_PATH: &str = "ai/gate_guard.toml";
    const PROFILE: &str = "schema = 1\nkind = \"ai_profile\"\naction = \"add\"\nid = \"gate_guard\"\nbehaviour = \"guard\"\n";
    const WALK_PATH: &str = "paths/lv/walk.toml";
    const WALK: &str = "schema = 1\nkind = \"path\"\naction = \"add\"\nid = \"walk\"\nmap = \"lv\"\npoints = [[1.0, 2.0, 3.0]]\n";
    const GROUP_PATH: &str = "spawns/lv/wolves.toml";
    const GROUP: &str = "schema = 1\nkind = \"spawn_group\"\naction = \"add\"\nid = \"wolves\"\nmap = \"lv\"\nvitals = \"Wolf\"\nlevel = 5\npoints = [[1.0, 2.0, 3.0]]\npath = \"walk\"\n";

    fn path(raw: &str) -> PathBuf {
        design_path(raw).expect("design path")
    }

    #[test]
    fn a_design_path_stays_inside_the_folder() {
        assert_eq!(design_path(PROFILE_PATH), Some(PathBuf::from(PROFILE_PATH)));
        for refused in [
            "../x.toml",
            "ai/../../x.toml",
            "/etc/x.toml",
            "./ai/x.toml",
            "ai/x.txt",
            "ai/",
            "",
            TODO_FILE,
        ] {
            assert_eq!(design_path(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_good_file_puts_and_writes() {
        let dir = tempfile::tempdir().expect("dir");
        let relative = path(PROFILE_PATH);
        assert_eq!(check_put(dir.path(), &relative, PROFILE), []);
        write_file(dir.path(), &relative, PROFILE).expect("write");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(PROFILE_PATH)).expect("read"),
            PROFILE
        );
        assert!(!temp_path(&dir.path().join(PROFILE_PATH)).exists());
        let (set, errors) = load_dir(dir.path());
        assert_eq!((set.files.len(), errors), (1, vec![]));
    }

    #[test]
    fn a_put_is_refused_with_the_problems_of_its_file() {
        let dir = tempfile::tempdir().expect("dir");
        let bad = PROFILE.replace("\"guard\"", "\"dance\"");
        let errors = check_put(dir.path(), &path(PROFILE_PATH), &bad);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, Path::new(PROFILE_PATH));
        assert_eq!(errors[0].field.as_deref(), Some("behaviour"));
        let broken = check_put(dir.path(), &path(PROFILE_PATH), "schema = ");
        assert!(matches!(broken[0].problem, Problem::BadToml(_)));
    }

    #[test]
    fn a_reference_must_name_a_file_of_the_set() {
        let dir = tempfile::tempdir().expect("dir");
        let errors = check_put(dir.path(), &path(GROUP_PATH), GROUP);
        assert_eq!(
            errors[0].problem,
            Problem::UnknownReference("path walk".to_owned())
        );
        write(dir.path(), WALK_PATH, WALK);
        assert_eq!(check_put(dir.path(), &path(GROUP_PATH), GROUP), []);
    }

    #[test]
    fn a_broken_other_file_does_not_refuse_a_put() {
        let dir = tempfile::tempdir().expect("dir");
        write(dir.path(), "ai/broken.toml", "schema = ");
        assert_eq!(check_put(dir.path(), &path(PROFILE_PATH), PROFILE), []);
    }

    #[test]
    fn a_put_that_repeats_an_id_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        write(dir.path(), PROFILE_PATH, PROFILE);
        let errors = check_put(
            dir.path(),
            &path("ai/Gate_Guard.toml"),
            &PROFILE.replace("gate_guard", "Gate_Guard"),
        );
        assert!(
            errors
                .iter()
                .any(|error| matches!(error.problem, Problem::DuplicateId { .. })),
            "{errors:?}"
        );
    }

    #[test]
    fn a_delete_that_breaks_a_reference_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        write(dir.path(), WALK_PATH, WALK);
        write(dir.path(), GROUP_PATH, GROUP);
        let errors = check_delete(dir.path(), &path(WALK_PATH));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, Path::new(GROUP_PATH));
        assert_eq!(check_delete(dir.path(), &path(GROUP_PATH)), []);
        delete_file(dir.path(), &path(GROUP_PATH)).expect("delete");
        assert!(!dir.path().join(GROUP_PATH).exists());
    }
}
