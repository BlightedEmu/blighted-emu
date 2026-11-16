// SPDX-License-Identifier: AGPL-3.0-or-later
//! The design step (spec section 5.2): reads the retail files, merges the design files over
//! them, and writes the merged files the servers read. A merged file that equals its retail file
//! (a level no design file changes, the bundle when no design file changes a table) is a hard
//! link to the retail file, so it takes no room twice.

use std::collections::BTreeSet;
use std::path::PathBuf;

use nw_content::layout::{
    BUNDLE, ContentLayout, DESIGN, FileRef, LayoutError, PendingFile, RETAIL_BUNDLE, RETAIL_WORLD,
    WORLD,
};
use nw_content::manifest::record_made_from;
use nw_content::{ContainerError, ContentBundle};
use nw_world::WorldFile;
use nw_world::script::DesignDataError;
use serde::Serialize;

use crate::error::{DesignError, Problem};
use crate::file::{DesignSet, Kind};
use crate::load::{design_files, load_dir};
use crate::points::apply_world;
use crate::rows::apply_tables;
use crate::schema::check_schema;
use crate::scripts::design_data;

/// The folders of the design step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignPaths {
    /// The design files (`content/`).
    pub design_dir: PathBuf,
    /// The content folder: the retail files are read from it and the merged files go into it.
    pub layout: ContentLayout,
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
    /// A folder cannot be listed or a file linked or copied.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The design data cannot be written.
    #[error(transparent)]
    DesignData(#[from] DesignDataError),
    /// The levels or the manifest of the content folder cannot be read or written.
    #[error(transparent)]
    Layout(#[from] LayoutError),
}

/// The retail bundle with the table-kind design files merged in memory.
#[derive(Debug)]
pub struct MergedBundle {
    /// The merged bundle.
    pub bundle: ContentBundle,
    /// The design files without a schema problem.
    pub set: DesignSet,
    /// Every design file read.
    pub files: usize,
    /// The problems so far.
    pub errors: Vec<DesignError>,
}

/// Reads the design files of `paths` and merges the table kinds into the retail bundle, in
/// memory. Files with a schema problem are left out, so each problem is reported once.
pub fn merged_bundle(paths: &DesignPaths) -> Result<MergedBundle, DesignRunError> {
    let (loaded, mut errors) = load_dir(&paths.design_dir);
    let schema_errors = check_schema(&loaded);
    let files = loaded.files.len();
    let set = DesignSet {
        files: loaded
            .files
            .into_iter()
            .filter(|file| !schema_errors.iter().any(|error| error.path == file.path))
            .collect(),
    };
    errors.extend(schema_errors);
    let mut bundle = ContentBundle::load_retail(&paths.layout)?;
    errors.extend(apply_tables(&set, &mut bundle.tables));
    Ok(MergedBundle {
        bundle,
        set,
        files,
        errors,
    })
}

/// Runs the design step on `paths`; written files are recorded in the manifest as made by
/// `made_by`.
pub fn run_design(
    paths: &DesignPaths,
    mode: DesignMode,
    made_by: &str,
) -> Result<DesignReport, DesignRunError> {
    let MergedBundle {
        bundle,
        set,
        files,
        mut errors,
    } = merged_bundle(paths)?;
    let maps: BTreeSet<String> = paths
        .layout
        .levels_with(RETAIL_WORLD)?
        .into_iter()
        .collect();
    let mut changed_maps = BTreeSet::new();
    // An AI profile goes into every level's world file.
    if set.files.iter().any(|file| file.kind == Kind::AiProfile) {
        changed_maps.extend(maps.iter().map(String::as_str));
    }
    for file in set.files.iter().filter(|file| file.kind.needs_map()) {
        match file.map.as_deref() {
            Some(map) if maps.contains(map) => {
                changed_maps.insert(map);
            }
            Some(_) => errors.push(DesignError::file(&file.path, Problem::UnknownMap)),
            None => {}
        }
    }
    for map in &changed_maps {
        let mut world = WorldFile::load_retail(&paths.layout, map)?;
        errors.extend(apply_world(&set, map, &mut world));
    }
    let mut report = DesignReport {
        files,
        levels: maps.len(),
        written: false,
        errors,
    };
    if mode == DesignMode::Write && report.errors.is_empty() {
        let written = Merged {
            layout: &paths.layout,
            set: &set,
            bundle: &bundle,
            maps: &maps,
            changed_maps: &changed_maps,
        }
        .write()?;
        record_made_from(
            &paths.layout,
            &written,
            made_by,
            &design_files(&paths.design_dir),
        )?;
        report.written = true;
    }
    Ok(report)
}

/// The merged files of one design run.
struct Merged<'a> {
    layout: &'a ContentLayout,
    set: &'a DesignSet,
    bundle: &'a ContentBundle,
    maps: &'a BTreeSet<String>,
    changed_maps: &'a BTreeSet<&'a str>,
}

impl Merged<'_> {
    /// Writes the merged bundle, the design data and a merged world file per level: the levels
    /// of `changed_maps` merged, the others hard links to their retail files. One world file is
    /// in memory at a time. Every file is first written under a new temporary name
    /// ([`PendingFile`]); only when all are written are they renamed, so a failed write leaves
    /// the files from before (and no temporary file). A failed rename can leave some new files
    /// beside old ones; nothing is recorded then, so the next build makes them all again.
    /// Returns the files written.
    fn write(&self) -> Result<Vec<FileRef>, DesignRunError> {
        let layout = self.layout;
        let client_build = layout.client_build();
        let mut files = vec![FileRef::build(BUNDLE), FileRef::build(DESIGN)];
        let mut pending = Vec::new();
        let bundle_bytes = self.bundle.to_bytes()?;
        let retail_bundle = layout.file(RETAIL_BUNDLE);
        let bundle_out = layout.file(BUNDLE);
        pending.push(if std::fs::read(&retail_bundle)? == bundle_bytes {
            PendingFile::link(&retail_bundle, &bundle_out)?
        } else {
            PendingFile::write(&bundle_out, &bundle_bytes)?
        });
        let data_out = layout.file(DESIGN);
        let data = design_data(self.set).to_json(&data_out)?;
        pending.push(PendingFile::write(&data_out, data.as_bytes())?);
        for map in self.maps {
            let out = layout.level_file(WORLD, map);
            pending.push(if self.changed_maps.contains(map.as_str()) {
                let mut world = WorldFile::load_retail(layout, map)?;
                apply_world(self.set, map, &mut world);
                PendingFile::write(&out, &world.to_bytes(client_build)?)?
            } else {
                PendingFile::link(&layout.level_file(RETAIL_WORLD, map), &out)?
            });
            files.push(FileRef::level(WORLD, map));
        }
        for file in pending {
            file.commit()?;
        }
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    use nw_content::ContentBundle;
    use nw_content::layout::is_temp_file;
    use nw_content::manifest::MADE_BY_UNKNOWN;
    use nw_world::WorldFile;

    use super::*;
    use crate::error::Problem;
    use crate::load::tests::write;
    use crate::points::tests::{MAP, world};

    const BUILD: &str = "22469132";
    /// A second level, which no design file changes.
    const OTHER_MAP: &str = "other";

    struct Folders {
        _root: tempfile::TempDir,
        paths: DesignPaths,
    }

    fn folders() -> Folders {
        let root = tempfile::tempdir().unwrap();
        let paths = DesignPaths {
            design_dir: root.path().join("content"),
            layout: ContentLayout::new(root.path().join("local-data"), BUILD),
        };
        let bundle = ContentBundle {
            client_build: BUILD.to_owned(),
            ..ContentBundle::default()
        };
        bundle.save(&paths.layout.file(RETAIL_BUNDLE)).unwrap();
        for map in [MAP, OTHER_MAP] {
            world()
                .save(&paths.layout.level_file(RETAIL_WORLD, map), BUILD)
                .unwrap();
        }
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

    /// The merged files of `paths`: the bundle, the design data and the world files.
    fn merged_files(paths: &DesignPaths) -> Vec<PathBuf> {
        let layout = &paths.layout;
        [layout.file(BUNDLE), layout.file(DESIGN)]
            .into_iter()
            .chain(
                [MAP, OTHER_MAP]
                    .into_iter()
                    .map(|map| layout.level_file(WORLD, map)),
            )
            .filter(|path| path.exists())
            .collect()
    }

    /// True when `path` and `other` are the same file (one is a hard link to the other).
    fn same_file(path: &Path, other: &Path) -> bool {
        let (left, right) = (
            std::fs::metadata(path).unwrap(),
            std::fs::metadata(other).unwrap(),
        );
        left.ino() == right.ino() && left.dev() == right.dev()
    }

    fn run(paths: &DesignPaths, mode: DesignMode) -> DesignReport {
        run_design(paths, mode, MADE_BY_UNKNOWN).unwrap()
    }

    #[test]
    fn write_mode_writes_the_merged_files() {
        let folders = folders();
        let paths = &folders.paths;
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 30");
        let report = run(paths, DesignMode::Write);
        assert_eq!(report.errors, []);
        assert!(report.written);
        assert_eq!((report.files, report.levels), (1, 2));
        let merged = WorldFile::load(&paths.layout, MAP).unwrap();
        assert_eq!(merged.vitals[0].level, 30);
        assert!(paths.layout.file(BUNDLE).exists());
        let layout = &paths.layout;
        assert!(!same_file(
            &layout.level_file(WORLD, MAP),
            &layout.level_file(RETAIL_WORLD, MAP)
        ));
        assert!(same_file(
            &layout.level_file(WORLD, OTHER_MAP),
            &layout.level_file(RETAIL_WORLD, OTHER_MAP)
        ));
        let manifest = layout.read_manifest().unwrap().expect("manifest");
        for key in ["bundle", "design", "world/lv", "world/other"] {
            assert!(manifest.files.contains_key(key), "{key}");
        }
    }

    #[test]
    fn no_design_files_link_the_retail_files() {
        let folders = folders();
        let paths = &folders.paths;
        let layout = &paths.layout;
        let report = run(paths, DesignMode::Write);
        assert!(report.written);
        assert!(same_file(&layout.file(BUNDLE), &layout.file(RETAIL_BUNDLE)));
        for map in [MAP, OTHER_MAP] {
            assert!(same_file(
                &layout.level_file(WORLD, map),
                &layout.level_file(RETAIL_WORLD, map)
            ));
        }
    }

    #[test]
    fn check_mode_writes_nothing() {
        let folders = folders();
        let paths = &folders.paths;
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 30");
        let report = run(paths, DesignMode::Check);
        assert_eq!(report.errors, []);
        assert!(!report.written);
        assert_eq!(merged_files(paths), Vec::<PathBuf>::new());
    }

    #[test]
    fn errors_write_no_file() {
        let folders = folders();
        let paths = &folders.paths;
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 30");
        spawn_file(&paths.design_dir, MAP, "1_0.0_0.0_0.0", "level = 30");
        let report = run(paths, DesignMode::Write);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].problem, Problem::UnknownId);
        assert!(!report.written);
        assert_eq!(merged_files(paths), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_point_on_a_missing_map_is_an_error() {
        let folders = folders();
        let paths = &folders.paths;
        spawn_file(&paths.design_dir, "nowhere", "1_0.0_0.0_0.0", "level = 30");
        let report = run(paths, DesignMode::Check);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].problem, Problem::UnknownMap);
    }

    #[test]
    fn a_schema_error_is_reported_once() {
        let folders = folders();
        let paths = &folders.paths;
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 66");
        let report = run(paths, DesignMode::Check);
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    }

    /// The temporary files in the folder of `path`.
    fn temp_files_beside(path: &Path) -> usize {
        std::fs::read_dir(path.parent().expect("folder")).map_or(0, |entries| {
            entries
                .filter(|entry| is_temp_file(&entry.as_ref().expect("entry").path()))
                .count()
        })
    }

    #[test]
    fn a_failed_write_leaves_no_new_file() {
        let folders = folders();
        let paths = &folders.paths;
        let world_out = paths.layout.level_file(WORLD, OTHER_MAP);
        // A file where the folder of the last world file goes makes that write fail after the
        // bundle, the design data and the first world file.
        let level_folder = world_out.parent().expect("level folder");
        std::fs::create_dir_all(level_folder.parent().expect("levels")).unwrap();
        std::fs::write(level_folder, b"not a folder").unwrap();
        assert!(run_design(paths, DesignMode::Write, MADE_BY_UNKNOWN).is_err());
        assert_eq!(merged_files(paths), Vec::<PathBuf>::new());
        let temp_files: usize = [
            paths.layout.file(BUNDLE),
            paths.layout.level_file(WORLD, MAP),
        ]
        .iter()
        .map(|path| temp_files_beside(path))
        .sum();
        assert_eq!(temp_files, 0);
        assert!(paths.layout.read_manifest().unwrap().is_none());
    }

    #[test]
    fn a_second_merge_leaves_no_temporary_file_keeps_the_links_and_the_times() {
        let folders = folders();
        let paths = &folders.paths;
        let layout = &paths.layout;
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 30");
        run(paths, DesignMode::Write);
        let first = layout.read_manifest().unwrap().expect("manifest");
        run(paths, DesignMode::Write);
        for path in merged_files(paths) {
            assert_eq!(temp_files_beside(&path), 0, "{}", path.display());
        }
        assert!(same_file(&layout.file(BUNDLE), &layout.file(RETAIL_BUNDLE)));
        assert!(same_file(
            &layout.level_file(WORLD, OTHER_MAP),
            &layout.level_file(RETAIL_WORLD, OTHER_MAP)
        ));
        let again = layout.read_manifest().unwrap().expect("manifest");
        for key in ["bundle", "design", "world/lv", "world/other"] {
            assert_eq!(again.files[key].made_at, first.files[key].made_at, "{key}");
        }
        let made_from = again.files["world/lv"]
            .made_from
            .as_ref()
            .expect("design files");
        assert_eq!(made_from.len(), 1);
        assert!(made_from[0].path.ends_with(".toml"));
    }

    #[test]
    fn a_left_over_temporary_link_to_a_retail_file_is_never_written_through() {
        let folders = folders();
        let paths = &folders.paths;
        let layout = &paths.layout;
        let retail = layout.level_file(RETAIL_WORLD, MAP);
        let before = std::fs::read(&retail).unwrap();
        let world_out = layout.level_file(WORLD, MAP);
        std::fs::create_dir_all(world_out.parent().expect("folder")).unwrap();
        // A crash between the link and the rename of an older merge left this.
        let left_over = world_out.with_extension("bin.tmp");
        std::fs::hard_link(&retail, &left_over).unwrap();
        let key = world().vitals_keys[0].clone();
        spawn_file(&paths.design_dir, MAP, &key, "level = 30");
        assert!(run(paths, DesignMode::Write).written);
        assert_eq!(std::fs::read(&retail).unwrap(), before);
        assert_eq!(std::fs::read(&left_over).unwrap(), before);
        assert_eq!(WorldFile::load(layout, MAP).unwrap().vitals[0].level, 30);
    }

    #[test]
    fn a_profile_alone_reaches_every_world_file() {
        let folders = folders();
        let paths = &folders.paths;
        write(
            &paths.design_dir,
            "ai/guards.toml",
            "schema = 1\nkind = \"ai_profile\"\naction = \"add\"\nid = \"guards\"\nbehaviour = \"guard\"\n",
        );
        let report = run(paths, DesignMode::Write);
        assert_eq!(report.errors, []);
        for map in [MAP, OTHER_MAP] {
            let merged = WorldFile::load(&paths.layout, map).unwrap();
            assert!(merged.ai_profiles.contains_key("guards"));
        }
    }

    #[test]
    fn scripts_and_events_reach_the_design_data() {
        use nw_world::script::DesignData;

        use crate::scripts::tests::{EVENT, SCRIPT};
        let folders = folders();
        let paths = &folders.paths;
        write(
            &paths.design_dir,
            &format!("scripts/{MAP}/greeting.toml"),
            SCRIPT,
        );
        write(
            &paths.design_dir,
            &format!("events/{MAP}/invasion.toml"),
            EVENT,
        );
        let report = run(paths, DesignMode::Write);
        assert_eq!(report.errors, []);
        let data = DesignData::load(&paths.layout).unwrap();
        assert!(data.levels[MAP].scripts.contains_key("greeting"));
        assert!(data.levels[MAP].events.contains_key("invasion"));
    }

    #[test]
    fn a_missing_retail_bundle_is_a_run_error() {
        let folders = folders();
        std::fs::remove_file(folders.paths.layout.file(RETAIL_BUNDLE)).unwrap();
        assert!(run_design(&folders.paths, DesignMode::Check, MADE_BY_UNKNOWN).is_err());
    }
}
