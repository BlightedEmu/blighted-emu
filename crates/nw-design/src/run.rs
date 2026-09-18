// SPDX-License-Identifier: AGPL-3.0-or-later
//! The design step (spec section 5.2): reads the retail files, merges the design files over
//! them, and writes the merged files the servers read.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use nw_content::bundle::bundle_path;
use nw_content::{ContainerError, ContentBundle};
use nw_world::WorldFile;
use nw_world::world_file::world_file_path;
use serde::Serialize;

use crate::error::{DesignError, Problem};
use crate::file::DesignSet;
use crate::load::load_dir;
use crate::points::apply_world;
use crate::rows::apply_tables;
use crate::schema::check_schema;

/// Extension of bundle and world files.
const BIN_EXTENSION: &str = ".bin";

/// The folders of the design step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignPaths {
    /// The design files (`content/`).
    pub design_dir: PathBuf,
    /// The retail bundle and world files (`local-data/content/retail/`).
    pub retail_dir: PathBuf,
    /// Where the merged files go (`local-data/content/`).
    pub out_dir: PathBuf,
}

/// Whether the design step writes the merged files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignMode {
    /// Runs every check and writes nothing.
    Check,
    /// Writes the merged files when there is no error.
    Write,
}

/// What the design step did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DesignReport {
    /// Design files read.
    pub files: usize,
    /// Retail world files found.
    pub levels: usize,
    /// True when the merged files were written.
    pub written: bool,
    /// Every problem found; nothing is written when there is one.
    pub errors: Vec<DesignError>,
}

/// A retail or merged file cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum DesignRunError {
    /// A bundle or world file cannot be read or written.
    #[error(transparent)]
    Container(#[from] ContainerError),
    /// A folder cannot be listed or a file copied.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Runs the design step on `paths` for `client_build`.
pub fn run_design(
    paths: &DesignPaths,
    client_build: &str,
    mode: DesignMode,
) -> Result<DesignReport, DesignRunError> {
    let (loaded, mut errors) = load_dir(&paths.design_dir);
    let schema_errors = check_schema(&loaded);
    // Files with a schema error are not merged, so each problem is reported once.
    let set = DesignSet {
        files: loaded
            .files
            .iter()
            .filter(|file| !schema_errors.iter().any(|error| error.path == file.path))
            .cloned()
            .collect(),
    };
    errors.extend(schema_errors);
    let mut bundle =
        ContentBundle::load(&bundle_path(&paths.retail_dir, client_build), client_build)?;
    errors.extend(apply_tables(&set, &mut bundle.tables));
    let maps = retail_maps(&paths.retail_dir, client_build)?;
    let mut changed_maps = BTreeSet::new();
    for file in set.files.iter().filter(|file| file.kind.is_point()) {
        match file.map.as_deref() {
            Some(map) if maps.contains(map) => {
                changed_maps.insert(map);
            }
            Some(_) => errors.push(DesignError::file(&file.path, Problem::UnknownMap)),
            None => {}
        }
    }
    for map in &changed_maps {
        let mut world = load_world(&paths.retail_dir, client_build, map)?;
        errors.extend(apply_world(&set, map, &mut world));
    }
    let mut report = DesignReport {
        files: loaded.files.len(),
        levels: maps.len(),
        written: false,
        errors,
    };
    if mode == DesignMode::Write && report.errors.is_empty() {
        write_merged(paths, client_build, &set, &bundle, &maps, &changed_maps)?;
        report.written = true;
    }
    Ok(report)
}

/// Writes the merged bundle, the merged world files of `changed_maps`, and copies of the other
/// retail world files. One world file is in memory at a time.
fn write_merged(
    paths: &DesignPaths,
    client_build: &str,
    set: &DesignSet,
    bundle: &ContentBundle,
    maps: &BTreeSet<String>,
    changed_maps: &BTreeSet<&str>,
) -> Result<(), DesignRunError> {
    std::fs::create_dir_all(&paths.out_dir)?;
    bundle.save(&bundle_path(&paths.out_dir, client_build))?;
    for map in maps {
        let out = world_file_path(&paths.out_dir, client_build, map);
        if changed_maps.contains(map.as_str()) {
            let mut world = load_world(&paths.retail_dir, client_build, map)?;
            apply_world(set, map, &mut world);
            world.save(&out, client_build)?;
        } else {
            std::fs::copy(world_file_path(&paths.retail_dir, client_build, map), out)?;
        }
    }
    Ok(())
}

fn load_world(dir: &Path, client_build: &str, map: &str) -> Result<WorldFile, DesignRunError> {
    Ok(WorldFile::load(
        &world_file_path(dir, client_build, map),
        client_build,
    )?)
}

/// The levels that have a retail world file of `client_build` in `dir`.
fn retail_maps(dir: &Path, client_build: &str) -> Result<BTreeSet<String>, DesignRunError> {
    let prefix = format!("world-{client_build}-");
    let mut maps = BTreeSet::new();
    for entry in std::fs::read_dir(dir)? {
        let name = entry?.file_name();
        if let Some(map) = name
            .to_str()
            .and_then(|name| name.strip_prefix(&prefix))
            .and_then(|rest| rest.strip_suffix(BIN_EXTENSION))
        {
            maps.insert(map.to_owned());
        }
    }
    Ok(maps)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use nw_content::ContentBundle;
    use nw_content::bundle::bundle_path;
    use nw_world::WorldFile;
    use nw_world::world_file::world_file_path;

    use super::*;
    use crate::error::Problem;
    use crate::load::tests::write;
    use crate::points::tests::{MAP, world};

    const BUILD: &str = "22469132";

    struct Folders {
        _root: tempfile::TempDir,
        paths: DesignPaths,
    }

    fn folders() -> Folders {
        let root = tempfile::tempdir().unwrap();
        let paths = DesignPaths {
            design_dir: root.path().join("content"),
            retail_dir: root.path().join("retail"),
            out_dir: root.path().join("out"),
        };
        let bundle = ContentBundle {
            client_build: BUILD.to_owned(),
            ..ContentBundle::default()
        };
        bundle.save(&bundle_path(&paths.retail_dir, BUILD)).unwrap();
        world()
            .save(&world_file_path(&paths.retail_dir, BUILD, MAP), BUILD)
            .unwrap();
        Folders { _root: root, paths }
    }

    fn spawn_file(dir: &Path, map: &str, key: &str, fields: &str) {
        write(
            dir,
            &format!("spawns/{map}/{key}.toml"),
            &format!(
                "schema = 1\nkind = \"spawn_point\"\naction = \"override\"\nid = \"{key}\"\nmap = \"{map}\"\n{fields}\n"
            ),
        );
    }

    fn out_files(paths: &DesignPaths) -> usize {
        std::fs::read_dir(&paths.out_dir).map_or(0, Iterator::count)
    }

    #[test]
    fn write_mode_writes_the_merged_files() {
        let folders = folders();
        let paths = &folders.paths;
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 30");
        let report = run_design(paths, BUILD, DesignMode::Write).unwrap();
        assert_eq!(report.errors, []);
        assert!(report.written);
        assert_eq!((report.files, report.levels), (1, 1));
        let merged = WorldFile::load(&world_file_path(&paths.out_dir, BUILD, MAP), BUILD).unwrap();
        assert_eq!(merged.vitals[0].level, 30);
        assert!(bundle_path(&paths.out_dir, BUILD).exists());
    }

    #[test]
    fn no_design_files_write_copies_equal_to_the_retail_files() {
        let folders = folders();
        let paths = &folders.paths;
        let report = run_design(paths, BUILD, DesignMode::Write).unwrap();
        assert!(report.written);
        for name in [
            format!("bundle-{BUILD}.bin"),
            format!("world-{BUILD}-{MAP}.bin"),
        ] {
            assert_eq!(
                std::fs::read(paths.retail_dir.join(&name)).unwrap(),
                std::fs::read(paths.out_dir.join(&name)).unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn check_mode_writes_nothing() {
        let folders = folders();
        let paths = &folders.paths;
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 30");
        let report = run_design(paths, BUILD, DesignMode::Check).unwrap();
        assert_eq!(report.errors, []);
        assert!(!report.written);
        assert_eq!(out_files(paths), 0);
    }

    #[test]
    fn errors_write_no_file() {
        let folders = folders();
        let paths = &folders.paths;
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 30");
        spawn_file(&paths.design_dir, MAP, "1_0.0_0.0_0.0", "level = 30");
        let report = run_design(paths, BUILD, DesignMode::Write).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].problem, Problem::UnknownId);
        assert!(!report.written);
        assert_eq!(out_files(paths), 0);
    }

    #[test]
    fn a_point_on_a_missing_map_is_an_error() {
        let folders = folders();
        let paths = &folders.paths;
        spawn_file(&paths.design_dir, "nowhere", "1_0.0_0.0_0.0", "level = 30");
        let report = run_design(paths, BUILD, DesignMode::Check).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].problem, Problem::UnknownMap);
    }

    #[test]
    fn a_schema_error_is_reported_once() {
        let folders = folders();
        let paths = &folders.paths;
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 66");
        let report = run_design(paths, BUILD, DesignMode::Check).unwrap();
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    }

    #[test]
    fn a_missing_retail_bundle_is_a_run_error() {
        let folders = folders();
        std::fs::remove_file(bundle_path(&folders.paths.retail_dir, BUILD)).unwrap();
        assert!(run_design(&folders.paths, BUILD, DesignMode::Check).is_err());
    }
}
