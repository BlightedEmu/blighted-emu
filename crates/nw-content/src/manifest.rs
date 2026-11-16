// SPDX-License-Identifier: AGPL-3.0-or-later
//! The manifest of a build folder (`<build>/manifest.json`): the build, and for each content
//! file where its data came from, what made it, when, from which files, and its size and file
//! time when it was recorded. Every content writer records its files with [`record`] (or
//! [`record_made_from`]), which also writes the folder's `README.md`. A reader takes fields it
//! does not know (a manifest of a newer `content-tool`), and an entry without the newer fields
//! (an older manifest) still loads.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::layout::{
    ContentLayout, FILES, FileRef, LEVEL_PLACEHOLDER, LayoutError, Source, Topic, write_atomic,
};

/// The manifest's file name in a build folder.
pub const MANIFEST_FILE: &str = "manifest.json";
/// The file name of the build folder's guide.
const README_FILE: &str = "README.md";
/// The lock file of a manifest write, beside the manifest. A copy of the build folder to another
/// machine leaves it out.
const LOCK_FILE: &str = "manifest.lock";
/// `made_by` when the writer does not know its commit.
pub const MADE_BY_UNKNOWN: &str = "unknown";

/// The manifest of a build folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The client build of the folder.
    pub build: String,
    /// One entry per content file, by its key (`<name>` or `<name>/<level>`).
    pub files: BTreeMap<String, ManifestEntry>,
}

/// What the manifest knows of one content file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Where its data came from.
    pub source: Source,
    /// The commit of the program that wrote it, `migrated`, or `unknown`.
    pub made_by: String,
    /// When its bytes were written (UTC, RFC 3339); the files made from it compare with this.
    pub made_at: String,
    /// When its step last made it from its inputs and wrote the same bytes again (UTC, RFC
    /// 3339): `made_at` keeps the time its bytes changed, and the file's own state compares its
    /// inputs with this time. `None` when the last record changed its bytes (then `made_at` is
    /// that time), and in an entry from before this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    /// The keys of the content files it was made from.
    pub inputs: Vec<String>,
    /// Its size and file time when it was recorded; `None` in an entry from before this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<FileStamp>,
    /// The other files it was made from (the design files, `Server.pak`) as they were when it
    /// was recorded; `None` for a file made from content files only, and in an entry from
    /// before this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_from: Option<Vec<MadeFrom>>,
}

/// The size and the file time (whole seconds, so a copy that keeps file times to the second,
/// `rsync -t`, keeps it) of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    /// Bytes.
    pub size: u64,
    /// The file time, seconds since the Unix epoch.
    pub modified_s: u64,
}

impl FileStamp {
    /// The stamp of the file at `path`; `None` when there is no file.
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path)
            .ok()
            .filter(std::fs::Metadata::is_file)?;
        let modified = metadata.modified().ok()?;
        Some(Self {
            size: metadata.len(),
            modified_s: modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .ok()?
                .as_secs(),
        })
    }
}

/// One file a content file was made from that is no content file, with its stamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MadeFrom {
    /// The path from the folder the writer ran in (`content/spawns/lv/x.toml`) with no `.` or
    /// `..` parts, or the full path of a file outside that folder; one file named in two ways
    /// (`content/x`, `./content/x`, the full path) has one path.
    pub path: String,
    /// Its size and file time.
    pub stamp: FileStamp,
}

impl MadeFrom {
    /// The files `paths` as they are now, sorted by path; a file that is gone is left out.
    pub fn list(paths: &[PathBuf]) -> Vec<Self> {
        let here = std::env::current_dir().ok();
        let mut list: Vec<Self> = paths
            .iter()
            .filter_map(|path| {
                Some(Self {
                    path: one_name(path, here.as_deref()),
                    stamp: FileStamp::of(path)?,
                })
            })
            .collect();
        list.sort_by(|left, right| left.path.cmp(&right.path));
        list
    }
}

/// The one name of `path` ([`MadeFrom::path`]) from the folder `here`: the full path without
/// `.` and `..` parts, then the part below `here` when it is below it. With no `here`, the
/// path without those parts.
fn one_name(path: &Path, here: Option<&Path>) -> String {
    let full = match here {
        Some(here) if path.is_relative() => here.join(path),
        _ => path.to_path_buf(),
    };
    let mut plain = PathBuf::new();
    for part in full.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if plain.file_name().is_some() {
                    plain.pop();
                } else if !plain.has_root() {
                    plain.push(std::path::Component::ParentDir);
                }
            }
            other => plain.push(other),
        }
    }
    let shown = here
        .and_then(|here| plain.strip_prefix(here).ok())
        .unwrap_or(&plain);
    shown.to_string_lossy().into_owned()
}

impl Manifest {
    /// An empty manifest of `build`.
    pub fn new(build: &str) -> Self {
        Self {
            build: build.to_owned(),
            files: BTreeMap::new(),
        }
    }

    /// The manifest at `path`, or `None` when there is none.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError`] when it cannot be read or is not a manifest.
    pub fn read(path: &Path) -> Result<Option<Self>, LayoutError> {
        match std::fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(|source| LayoutError::Json {
                        path: path.to_path_buf(),
                        source,
                    })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(LayoutError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Writes the manifest to `path` ([`write_atomic`]).
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::Io`] when it cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), LayoutError> {
        let mut text = serde_json::to_string_pretty(self).map_err(|source| LayoutError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        text.push('\n');
        write_atomic(path, text.as_bytes()).map_err(|source| LayoutError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Sets the entry of `file` (now at `path`): its registry source, `made_by`, the keys
    /// `inputs` and the other files `made_from` it was made from, and its stamp. Its time is
    /// `made_at`, unless the entry before has the file as it is now (same stamp; for an entry
    /// without a stamp, a file time not after its time): then the file did not change and keeps
    /// its time and maker, so the files made from it stay fresh, and `made_at` becomes its
    /// `checked_at`, so the file itself is not older than the inputs it was made from again.
    pub fn set(
        &mut self,
        file: &FileRef,
        path: &Path,
        made_by: &str,
        made_at: &str,
        inputs: Vec<String>,
        made_from: Option<Vec<MadeFrom>>,
    ) {
        let stamp = FileStamp::of(path);
        let key = file.key();
        let unchanged = self.files.get(&key).filter(|before| {
            stamp.is_some()
                && match before.stamp {
                    Some(before_stamp) => Some(before_stamp) == stamp,
                    None => before
                        .made_at_time()
                        .zip(stamp)
                        .is_some_and(|(time, stamp)| {
                            time.duration_since(SystemTime::UNIX_EPOCH)
                                .is_ok_and(|since| stamp.modified_s <= since.as_secs())
                        }),
                }
        });
        let (made_by, checked_at, made_at) = unchanged.map_or_else(
            || (made_by.to_owned(), None, made_at.to_owned()),
            |before| {
                (
                    before.made_by.clone(),
                    Some(made_at.to_owned()),
                    before.made_at.clone(),
                )
            },
        );
        self.files.insert(
            key,
            ManifestEntry {
                source: file.file.source,
                made_by,
                made_at,
                checked_at,
                inputs,
                stamp,
                made_from,
            },
        );
    }
}

impl ManifestEntry {
    /// When its bytes were written; `None` when `made_at` is no time.
    pub fn made_at_time(&self) -> Option<SystemTime> {
        time_of(&self.made_at)
    }

    /// When its step last made it: `checked_at`, or else `made_at`; `None` when that is no time.
    pub fn checked_at_time(&self) -> Option<SystemTime> {
        self.checked_at
            .as_deref()
            .map_or_else(|| self.made_at_time(), time_of)
    }
}

/// The time of the RFC 3339 text `text`; `None` when it is no time or before 1970.
fn time_of(text: &str) -> Option<SystemTime> {
    let nanoseconds = text.parse::<jiff::Timestamp>().ok()?.as_nanosecond();
    let nanoseconds = u64::try_from(nanoseconds).ok()?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_nanos(nanoseconds))
}

/// `time` as RFC 3339 text in UTC, to the nanosecond (the form of [`utc_now`]); `None` before
/// 1970 or past the range of a timestamp.
fn utc_text(time: SystemTime) -> Option<String> {
    let since = time.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let nanoseconds = i128::try_from(since.as_nanos()).ok()?;
    Some(
        jiff::Timestamp::from_nanosecond(nanoseconds)
            .ok()?
            .to_string(),
    )
}

/// The time now in UTC (RFC 3339, to the nanosecond). A file is recorded after it is written,
/// so its time is never before its file time.
fn utc_now() -> String {
    jiff::Timestamp::now().to_string()
}

/// Records `files` in the manifest of `layout` as made by `made_by`, now (one time for all of
/// them; a file that did not change keeps its entry's time, [`Manifest::set`]; read, change and
/// write under a lock, so two writers do not lose an entry), and writes the build folder's
/// `README.md`.
///
/// # Errors
///
/// Returns [`LayoutError`] when the manifest or the guide cannot be read or written.
pub fn record(layout: &ContentLayout, files: &[FileRef], made_by: &str) -> Result<(), LayoutError> {
    record_files(layout, files, made_by, None, RecordTime::Now)
}

/// [`record`] with each file's own file time as its time (now for a file without one): for files
/// that were made before they were recorded (moved from the flat folder), so a file older than
/// its inputs stays older than them.
///
/// # Errors
///
/// Returns [`LayoutError`] when the manifest or the guide cannot be read or written.
pub fn record_at_file_times(
    layout: &ContentLayout,
    files: &[FileRef],
    made_by: &str,
) -> Result<(), LayoutError> {
    record_files(layout, files, made_by, None, RecordTime::FileTime)
}

/// Which time a record gives its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordTime {
    /// One time, now, for all of them.
    Now,
    /// Each file's own file time.
    FileTime,
}

/// [`record`] for files made from the other files `made_from` too (the design files,
/// `Server.pak`), whose list and stamps go into each entry.
///
/// # Errors
///
/// Returns [`LayoutError`] when the manifest or the guide cannot be read or written.
pub fn record_made_from(
    layout: &ContentLayout,
    files: &[FileRef],
    made_by: &str,
    made_from: &[PathBuf],
) -> Result<(), LayoutError> {
    record_files(
        layout,
        files,
        made_by,
        Some(MadeFrom::list(made_from)),
        RecordTime::Now,
    )
}

fn record_files(
    layout: &ContentLayout,
    files: &[FileRef],
    made_by: &str,
    made_from: Option<Vec<MadeFrom>>,
    time: RecordTime,
) -> Result<(), LayoutError> {
    let build_dir = layout.build_dir();
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| LayoutError::Io { path, source }
    };
    std::fs::create_dir_all(build_dir).map_err(io(build_dir))?;
    let lock_path = build_dir.join(LOCK_FILE);
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(io(&lock_path))?;
    lock.lock().map_err(io(&lock_path))?;
    let path = layout.manifest_path();
    let mut manifest =
        Manifest::read(&path)?.unwrap_or_else(|| Manifest::new(layout.client_build()));
    let now = utc_now();
    for file in files {
        let path = layout.path(file);
        let made_at = match time {
            RecordTime::Now => None,
            RecordTime::FileTime => std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(utc_text),
        };
        manifest.set(
            file,
            &path,
            made_by,
            made_at.as_deref().unwrap_or(&now),
            file.input_keys(layout)?,
            made_from.clone(),
        );
    }
    manifest.write(&path)?;
    let readme = build_dir.join(README_FILE);
    write_atomic(&readme, build_readme(layout.client_build()).as_bytes()).map_err(io(&readme))
}

/// The guide of a build folder: what each folder holds and which files are capture-made.
fn build_readme(build: &str) -> String {
    let mut text = format!(
        "# Content of client build {build}\n\n\
         Written by `content-tool` from its file registry; do not edit. `content-tool content \
         status` shows the state of every file, `manifest.json` what made each one. `{LOCK_FILE}` \
         is the lock of the manifest: a copy of this folder to another machine leaves it out \
         (`rsync -a --exclude {LOCK_FILE}`).\n"
    );
    for topic in Topic::ALL {
        let _ = write!(
            text,
            "\n## {}/\n\n{}.\n\n",
            topic.folder(),
            capitalized(topic.summary())
        );
        for file in FILES.iter().filter(|file| file.topic == topic) {
            let capture_made = if file.source == Source::Pack {
                " (capture-made: the realm data pack)"
            } else {
                ""
            };
            let _ = writeln!(
                text,
                "- `{}`: {}{capture_made}",
                file.pattern(),
                file.source.label()
            );
        }
    }
    let _ = write!(
        text,
        "\n`{LEVEL_PLACEHOLDER}` is the level's folder name (`newworld_vitaeeterna`).\n"
    );
    text
}

/// `text` with its first letter in upper case.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{COLLISION, MOB_PREFABS, TUTORIAL_LEVEL};

    const BUILD: &str = "12345";
    const MADE_BY: &str = "0123abcd";

    #[test]
    fn record_adds_entries_and_keeps_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        record(&layout, &[FileRef::build(MOB_PREFABS)], MADE_BY).unwrap();
        record(
            &layout,
            &[FileRef::level(COLLISION, TUTORIAL_LEVEL)],
            MADE_BY,
        )
        .unwrap();
        let manifest = layout.read_manifest().unwrap().expect("manifest");
        assert_eq!(manifest.build, BUILD);
        assert_eq!(manifest.files["mob-prefabs"].source, Source::Pack);
        let collision = &manifest.files["collision/ftue_v2"];
        assert_eq!(collision.made_by, MADE_BY);
        assert_eq!(collision.inputs, ["world/ftue_v2", "water/ftue_v2"]);
        assert!(collision.made_at.ends_with('Z'), "{}", collision.made_at);
        assert!(collision.made_at_time().is_some());
        let readme = std::fs::read_to_string(layout.build_dir().join(README_FILE)).unwrap();
        assert!(readme.contains("`mobs/mob-prefabs.json`: pack (capture-made"));
    }

    #[test]
    fn a_manifest_of_a_newer_or_older_tool_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MANIFEST_FILE);
        std::fs::write(
            &path,
            br#"{"build":"1","extra":1,"files":{"mob-prefabs":{"source":"pack","made_by":"migrated","made_at":"2026-11-16T13:39:45Z","inputs":[],"later":true}}}"#,
        )
        .unwrap();
        let manifest = Manifest::read(&path).unwrap().expect("manifest");
        let entry = &manifest.files["mob-prefabs"];
        assert_eq!((entry.stamp, entry.made_from.as_ref()), (None, None));
        std::fs::write(&path, b"[]").unwrap();
        assert!(matches!(
            Manifest::read(&path),
            Err(LayoutError::Json { .. })
        ));
    }

    /// The manifest entry of the mob prefab table of `layout`.
    fn prefab_entry(layout: &ContentLayout) -> ManifestEntry {
        layout.read_manifest().unwrap().expect("manifest").files["mob-prefabs"].clone()
    }

    #[test]
    fn a_record_of_an_unchanged_file_keeps_its_time() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        let file = FileRef::build(MOB_PREFABS);
        crate::layout::write_atomic(&layout.path(&file), b"[]").unwrap();
        record(&layout, std::slice::from_ref(&file), MADE_BY).unwrap();
        let first = prefab_entry(&layout);
        assert!(first.stamp.is_some());
        record(&layout, std::slice::from_ref(&file), "later").unwrap();
        let again = prefab_entry(&layout);
        assert_eq!(
            (&again.made_by, &again.made_at, again.stamp),
            (&first.made_by, &first.made_at, first.stamp)
        );
        let checked = again.checked_at_time().expect("checked time");
        assert!(checked >= first.made_at_time().unwrap());
        crate::layout::write_atomic(&layout.path(&file), b"[1]").unwrap();
        record(&layout, std::slice::from_ref(&file), "later").unwrap();
        let changed = prefab_entry(&layout);
        assert_eq!(changed.made_by, "later");
        assert_ne!(changed.stamp, first.stamp);
        assert_eq!(changed.checked_at, None, "a new record of new bytes");
    }

    #[test]
    fn a_record_at_file_times_takes_each_file_time() {
        /// A file time well before now.
        const EARLIER: Duration = Duration::from_secs(3600);
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        let file = FileRef::build(MOB_PREFABS);
        let path = layout.path(&file);
        crate::layout::write_atomic(&path, b"[]").unwrap();
        let earlier = SystemTime::now() - EARLIER;
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(earlier)
            .unwrap();
        record_at_file_times(&layout, std::slice::from_ref(&file), MADE_BY).unwrap();
        assert_eq!(prefab_entry(&layout).made_at_time(), Some(earlier));
    }

    #[test]
    fn one_file_named_in_two_ways_has_one_name() {
        let here = Path::new("/repo");
        let name = |path: &str| one_name(Path::new(path), Some(here));
        assert_eq!(name("content/x.toml"), "content/x.toml");
        assert_eq!(name("./content/x.toml"), "content/x.toml");
        assert_eq!(name("/repo/content/x.toml"), "content/x.toml");
        assert_eq!(name("bins/../content/x.toml"), "content/x.toml");
        assert_eq!(name("/other/x.toml"), "/other/x.toml");
        assert_eq!(one_name(Path::new("../x.toml"), None), "../x.toml");
    }

    #[test]
    fn an_entry_without_a_stamp_keeps_its_time_while_the_file_is_not_newer() {
        /// A time after the file's, inside the nanosecond range of `made_at_time`.
        const FUTURE: &str = "2099-01-01T00:00:00Z";
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        let file = FileRef::build(MOB_PREFABS);
        crate::layout::write_atomic(&layout.path(&file), b"[]").unwrap();
        let mut manifest = Manifest::new(BUILD);
        manifest.files.insert(
            file.key(),
            ManifestEntry {
                source: Source::Pack,
                made_by: "migrated".to_owned(),
                made_at: FUTURE.to_owned(),
                checked_at: None,
                inputs: Vec::new(),
                stamp: None,
                made_from: None,
            },
        );
        manifest.write(&layout.manifest_path()).unwrap();
        record(&layout, std::slice::from_ref(&file), MADE_BY).unwrap();
        let entry = prefab_entry(&layout);
        assert_eq!(
            (entry.made_by.as_str(), entry.made_at.as_str()),
            ("migrated", FUTURE)
        );
        assert!(entry.stamp.is_some());
    }

    #[test]
    fn the_guide_names_every_topic_folder() {
        let readme = build_readme(BUILD);
        for topic in Topic::ALL {
            assert!(readme.contains(&format!("## {}/", topic.folder())));
        }
    }
}
