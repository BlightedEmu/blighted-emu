// SPDX-License-Identifier: AGPL-3.0-or-later
//! Edits of one design file from the Studio (admin studio spec section 7): a safe path, the
//! problems an edit would add to the set, and the write.

use std::path::{Component, Path, PathBuf};

use crate::error::DesignError;
use crate::file::DesignSet;
use crate::load::{DESIGN_EXTENSION, TODO_FILE, file_version, load_dir, parse_file, sort_files};
use crate::run::temp_path;
use crate::schema::check_schema;

/// Why an edit through a symbolic link is refused.
pub const LINK_REFUSED: &str = "the path goes through a symbolic link";

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

/// What an edit expects to find at its path, so it replaces only the text its editor opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    /// Any file or none.
    Anything,
    /// No file: the edit makes a new one.
    Nothing,
    /// The file whose text has this [`file_version`].
    Version(String),
}

/// Whether the file at `relative` under `dir` is what `expected` names.
///
/// # Errors
///
/// Refuses a path through a symbolic link (see [`no_link`]); otherwise the read error, other
/// than a missing file.
pub fn is_expected(dir: &Path, relative: &Path, expected: &Expected) -> std::io::Result<bool> {
    let version = match expected {
        Expected::Anything => return no_link(dir, relative).map(|()| true),
        Expected::Nothing => None,
        Expected::Version(version) => Some(version),
    };
    match read_file(dir, relative) {
        Ok(text) => Ok(version.is_some_and(|version| *version == file_version(&text))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(version.is_none()),
        Err(error) => Err(error),
    }
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

/// The text of `relative` under `dir`.
///
/// # Errors
///
/// Refuses a path through a symbolic link (see [`no_link`]); otherwise the read error.
pub fn read_file(dir: &Path, relative: &Path) -> std::io::Result<String> {
    no_link(dir, relative)?;
    std::fs::read_to_string(dir.join(relative))
}

/// Writes `text` to `relative` under `dir`, through a temporary file renamed at the end, so a
/// reader sees the old file or the new one.
///
/// # Errors
///
/// Refuses a path through a symbolic link (see [`no_link`]); otherwise the write error.
pub fn write_file(dir: &Path, relative: &Path, text: &str) -> std::io::Result<()> {
    no_link(dir, relative)?;
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
///
/// # Errors
///
/// Refuses a path through a symbolic link (see [`no_link`]); otherwise the remove error.
pub fn delete_file(dir: &Path, relative: &Path) -> std::io::Result<()> {
    no_link(dir, relative)?;
    std::fs::remove_file(dir.join(relative))
}

/// Refuses `relative` when a folder or the file on its way under `dir` is a symbolic link, so
/// an edit stays inside the design folder. The parts that do not exist yet pass.
fn no_link(dir: &Path, relative: &Path) -> std::io::Result<()> {
    let mut path = dir.to_path_buf();
    for component in relative.components() {
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    LINK_REFUSED,
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
    }
    Ok(())
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

    #[cfg(unix)]
    #[test]
    fn an_edit_through_a_symbolic_link_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("x.toml"), PROFILE).expect("write");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("ai")).expect("folder link");
        std::os::unix::fs::symlink(
            outside.path().join("x.toml"),
            dir.path().join("linked.toml"),
        )
        .expect("file link");
        for raw in ["ai/x.toml", "linked.toml"] {
            let relative = path(raw);
            for result in [
                read_file(dir.path(), &relative).map(|_| ()),
                write_file(dir.path(), &relative, PROFILE),
                delete_file(dir.path(), &relative),
            ] {
                let error = result.expect_err(raw);
                assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied, "{raw}");
            }
        }
        assert!(outside.path().join("x.toml").exists());
        let relative = path(PROFILE_PATH.replace("ai/", "paths/").as_str());
        write_file(dir.path(), &relative, PROFILE).expect("a new folder is not a link");
        assert_eq!(read_file(dir.path(), &relative).expect("read"), PROFILE);
    }

    #[test]
    fn an_edit_expects_the_version_it_opened() {
        let dir = tempfile::tempdir().expect("dir");
        let relative = path(PROFILE_PATH);
        assert!(is_expected(dir.path(), &relative, &Expected::Nothing).expect("new"));
        write_file(dir.path(), &relative, PROFILE).expect("write");
        let opened = Expected::Version(file_version(PROFILE));
        assert!(is_expected(dir.path(), &relative, &opened).expect("same"));
        assert!(!is_expected(dir.path(), &relative, &Expected::Nothing).expect("exists"));
        write_file(dir.path(), &relative, &PROFILE.replace("guard", "patrol")).expect("write");
        assert!(!is_expected(dir.path(), &relative, &opened).expect("changed"));
        assert!(is_expected(dir.path(), &relative, &Expected::Anything).expect("any"));
        delete_file(dir.path(), &relative).expect("delete");
        assert!(!is_expected(dir.path(), &relative, &opened).expect("removed"));
        let (set, _) = load_dir(dir.path());
        assert!(set.files.is_empty());
        write_file(dir.path(), &relative, PROFILE).expect("write");
        assert_eq!(
            load_dir(dir.path()).0.files[0].version,
            file_version(PROFILE)
        );
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
