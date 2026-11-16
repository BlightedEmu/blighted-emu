// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool content`: builds every content file in registry order (`build`), shows the
//! state of every file (`status`), and moves an old flat content folder into the layout
//! (`migrate`). The files, their steps and their inputs come from the registry
//! ([`nw_content::layout::FILES`] and [`STEPS`]).

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use nw_content::layout::{
    BUNDLE, ContentFile, ContentLayout, FILES, FileRef, LEVEL_PLACEHOLDER, LayoutError, MAP_FOLDER,
    RETAIL_BUNDLE, RETAIL_WORLD, STEPS, Scope, Source, Step, StepFile, StepSpec, Topic, WORLD,
    link_atomic, pack_files,
};
use nw_content::manifest::{FileStamp, MadeFrom, Manifest, ManifestEntry, record};

use crate::error::ToolError;

/// `made_by` of the content files this program writes: its commit (set by the build script).
pub const MADE_BY: &str = env!("CONTENT_TOOL_COMMIT");
/// `made_by` of the files `content migrate` moves.
const MADE_BY_MIGRATED: &str = "migrated";
/// The folder of the retail files in the old flat content folder.
const LEGACY_RETAIL_FOLDER: &str = "retail";
/// The start of the registry name of a retail file; the old flat name has the rest only.
const RETAIL_NAME_PREFIX: &str = "retail-";
/// Bytes compared at a time by [`same_bytes`].
const COMPARE_CHUNK: usize = 1 << 20;

/// What `content build` builds and reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOptions {
    /// Builds every step again, even one whose files are fresh.
    pub rebuild: bool,
    /// Builds only the steps that make derived files ([`StepSpec::is_derived`]).
    pub derived_only: bool,
    /// True when the install is known (`NW_GAME_DIR`).
    pub has_game_dir: bool,
    /// The design files: inputs of the design step.
    pub design_dir: PathBuf,
    /// The `Server.pak` file: the input of the AI data.
    pub server_pak: PathBuf,
}

/// What `content build` did with a step, or what it found of a pack file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutcome {
    /// The step ran.
    Built,
    /// Every file of the step exists, is in the manifest and is newer than its inputs.
    Fresh,
    /// The step reads the install, none is set, and its files exist: they are kept.
    NoGameDir,
    /// The step reads `Server.pak`, which is not there.
    NoServerPak,
    /// The step reads these capture-made files, which are missing.
    NoPack(Vec<&'static str>),
    /// This capture-made file is missing.
    PackMissing,
}

/// One line of `content build`: a step's command or a pack file's name, and what was done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLine {
    /// The step's command or the pack file's registry name.
    pub what: &'static str,
    /// What was done.
    pub outcome: BuildOutcome,
}

/// Builds the content of `layout`: every step of [`STEPS`] in order (only the derived ones with
/// [`BuildOptions::derived_only`]), skipping a step whose files are fresh unless
/// [`BuildOptions::rebuild`]. `run` runs a step (and records its files); `note` gets a line per
/// missing pack file and per step.
///
/// # Errors
///
/// Returns [`ToolError::StepNeedsGameDir`] when a step that reads the install has missing files
/// and no install is set, or the error of `run`.
pub fn build_content(
    layout: &ContentLayout,
    options: &BuildOptions,
    mut run: impl FnMut(&StepSpec) -> Result<(), ToolError>,
    mut note: impl FnMut(&BuildLine),
) -> Result<(), ToolError> {
    for file in pack_files() {
        if !layout.path(&whole(file)).exists() {
            note(&BuildLine {
                what: file.name,
                outcome: BuildOutcome::PackMissing,
            });
        }
    }
    for spec in STEPS
        .iter()
        .filter(|spec| !options.derived_only || spec.is_derived())
    {
        let outcome = step_outcome(layout, spec, options)?;
        if outcome == BuildOutcome::Built {
            run(spec)?;
        }
        note(&BuildLine {
            what: spec.command,
            outcome,
        });
    }
    Ok(())
}

/// What `content build` does with `spec`.
fn step_outcome(
    layout: &ContentLayout,
    spec: &StepSpec,
    options: &BuildOptions,
) -> Result<BuildOutcome, ToolError> {
    let missing_pack: Vec<&'static str> = spec
        .pack_inputs()
        .filter(|file| !layout.path(&whole(file)).exists())
        .map(|file| file.name)
        .collect();
    if !missing_pack.is_empty() {
        return Ok(BuildOutcome::NoPack(missing_pack));
    }
    if spec.needs_server_pak && !options.server_pak.is_file() {
        return Ok(BuildOutcome::NoServerPak);
    }
    let manifest = layout.read_manifest()?;
    let outputs = step_outputs(layout, spec)?;
    let extra = made_from(spec, &options.design_dir, &options.server_pak);
    let mut fresh = !outputs.is_empty();
    for output in &outputs {
        let inputs = output.inputs(layout)?;
        if file_state(layout, manifest.as_ref(), output, &inputs, &extra) != FileState::Ok {
            fresh = false;
        }
    }
    if fresh && !options.rebuild {
        return Ok(BuildOutcome::Fresh);
    }
    if spec.needs_game_dir && !options.has_game_dir {
        let all_there =
            !outputs.is_empty() && outputs.iter().all(|output| layout.path(output).exists());
        return if all_there {
            Ok(BuildOutcome::NoGameDir)
        } else {
            Err(ToolError::StepNeedsGameDir { step: spec.command })
        };
    }
    Ok(BuildOutcome::Built)
}

/// A file per build of the registry (a pack file, or a file of a fixed level's folder).
fn whole(file: &'static ContentFile) -> FileRef {
    FileRef { file, level: None }
}

/// The files `spec` writes.
fn step_outputs(layout: &ContentLayout, spec: &StepSpec) -> Result<Vec<FileRef>, LayoutError> {
    let levels = spec.level_names(layout)?;
    Ok(spec
        .outputs
        .iter()
        .flat_map(|output| match *output {
            StepFile::Build(file) => vec![FileRef::build(file)],
            StepFile::Level(file) => levels
                .iter()
                .map(|level| FileRef::level(file, level))
                .collect(),
        })
        .collect())
}

/// The inputs of `spec` that are no content files: the design files of `design_dir` that the
/// design step reads ([`nw_design::design_files`]), the `Server.pak` file `server_pak`.
pub fn made_from(spec: &StepSpec, design_dir: &Path, server_pak: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if spec.step == Step::Design {
        paths.extend(nw_design::design_files(design_dir));
    }
    if spec.needs_server_pak {
        paths.push(server_pak.to_path_buf());
    }
    paths
}

/// The state of a content file.
///
/// A file's time is its manifest time (`made_at`), not its file time: a hard link keeps the
/// time of the file it links to. An input's time is its manifest time while it is as it was
/// recorded (same size and file time, [`FileStamp`]); an input changed since (a new edit, a copy
/// back from a backup with an older time) counts as changed now. An entry with no stamp (from
/// before stamps) keeps the rule from before: the later of its manifest and file time. The other
/// files a file was made from (the design files, `Server.pak`) must be the list its entry has
/// (`made_from`, the same paths and stamps); an entry without that list takes them as inputs
/// with their file times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    /// It exists as it was recorded, and no input is newer or changed.
    Ok,
    /// It exists, but it or an input changed, or the manifest does not have it.
    Old,
    /// It does not exist.
    Missing,
}

impl FileState {
    /// The word the status table shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Old => "old",
            Self::Missing => "missing",
        }
    }
}

/// The state of `file`, made from the content files `inputs` and the other files `made_from`.
fn file_state(
    layout: &ContentLayout,
    manifest: Option<&Manifest>,
    file: &FileRef,
    inputs: &[FileRef],
    made_from: &[PathBuf],
) -> FileState {
    let path = layout.path(file);
    let Some(stamp) = FileStamp::of(&path) else {
        return FileState::Missing;
    };
    let entry_of = |file: &FileRef| manifest.and_then(|manifest| manifest.files.get(&file.key()));
    let Some(entry) = entry_of(file) else {
        return FileState::Old;
    };
    let Some(made) = entry.made_at_time() else {
        return FileState::Old;
    };
    if entry.stamp.is_some_and(|recorded| recorded != stamp) {
        return FileState::Old;
    }
    let newer_input = inputs.iter().any(|input| {
        let input_path = layout.path(input);
        input_time(entry_of(input), &input_path).is_some_and(|time| time > made)
    });
    let other_changed = match &entry.made_from {
        Some(recorded) => *recorded != MadeFrom::list(made_from),
        None => made_from
            .iter()
            .filter_map(|path| modified(path))
            .any(|time| time > made),
    };
    if newer_input || other_changed {
        FileState::Old
    } else {
        FileState::Ok
    }
}

/// The time of the input at `path` with the manifest entry `entry` (module text of
/// [`FileState`]); `None` when it does not exist.
fn input_time(entry: Option<&ManifestEntry>, path: &Path) -> Option<SystemTime> {
    let modified = modified(path)?;
    let Some(entry) = entry else {
        return Some(modified);
    };
    match entry.stamp {
        Some(recorded) if Some(recorded) == FileStamp::of(path) => entry.made_at_time(),
        Some(_) => Some(SystemTime::now()),
        None => entry.made_at_time().max(Some(modified)),
    }
}

/// When `path` was last written; `None` when it does not exist.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// One row of `content status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    /// The manifest key (`<name>` or `<name>/<level>`).
    pub key: String,
    /// The topic folder.
    pub topic: &'static str,
    /// Where its data comes from.
    pub source: Source,
    /// Its state.
    pub state: FileState,
    /// When it was written, from the manifest.
    pub made_at: Option<String>,
}

/// The state of every content file of `layout`: a row per file, and per level for a level file
/// (the levels its steps make and the levels that have it).
///
/// # Errors
///
/// Returns [`ToolError`] when the manifest or a folder cannot be read.
pub fn content_status(
    layout: &ContentLayout,
    options: &BuildOptions,
) -> Result<Vec<StatusRow>, ToolError> {
    let manifest = layout.read_manifest()?;
    let mut rows = Vec::new();
    for &file in FILES {
        for level in file_levels(layout, file)? {
            let file_ref = FileRef {
                file,
                level: level.clone(),
            };
            let step = STEPS
                .iter()
                .find(|step| step.makes_at(file, level.as_deref()));
            let state = match step {
                Some(step) => file_state(
                    layout,
                    manifest.as_ref(),
                    &file_ref,
                    &file_ref.inputs(layout)?,
                    &made_from(step, &options.design_dir, &options.server_pak),
                ),
                None => file_state(layout, manifest.as_ref(), &file_ref, &[], &[]),
            };
            let key = match (&level, file.scope) {
                (None, Scope::Level) => format!("{}/{LEVEL_PLACEHOLDER}", file.name),
                _ => file_ref.key(),
            };
            rows.push(StatusRow {
                made_at: manifest
                    .as_ref()
                    .and_then(|manifest| manifest.files.get(&key))
                    .map(|entry| entry.made_at.clone()),
                key,
                topic: file.topic.folder(),
                source: file.source,
                state,
            });
        }
    }
    Ok(rows)
}

/// The levels of the status rows of `file`: `[None]` for a file per build; for a level file the
/// levels its steps make and the levels that have it, or `[None]` (a missing row) when none.
fn file_levels(
    layout: &ContentLayout,
    file: &'static ContentFile,
) -> Result<Vec<Option<String>>, LayoutError> {
    if file.scope != Scope::Level {
        return Ok(vec![None]);
    }
    let handle = nw_content::layout::LevelFile(file);
    let mut levels: BTreeSet<String> = layout.levels_with(handle)?.into_iter().collect();
    for step in STEPS.iter().filter(|step| step.makes(file)) {
        levels.extend(step.level_names(layout)?);
    }
    if levels.is_empty() {
        return Ok(vec![None]);
    }
    Ok(levels.into_iter().map(Some).collect())
}

/// How `content migrate` moves one old file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveKind {
    /// Renamed to its layout path.
    Rename,
    /// Byte-equal to its retail file: its layout path becomes a hard link to the retail file
    /// at its layout path, and the old file is removed.
    LinkToRetail(PathBuf),
    /// Its layout path holds the same bytes already (a migration that stopped half-way): the
    /// old file is removed.
    RemoveDuplicate,
}

/// One file `content migrate` moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    /// The registry file.
    pub file: FileRef,
    /// The old flat path.
    pub from: PathBuf,
    /// The layout path.
    pub to: PathBuf,
    /// How it moves.
    pub kind: MoveKind,
}

/// What `content migrate` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratePlan {
    /// The moves, the retail files first.
    pub moves: Vec<Move>,
    /// The entries of the old folder that are no content file of the registry: left in place.
    pub left: Vec<PathBuf>,
    /// The files at their layout paths that the manifest does not have (a migration that
    /// stopped half-way): recorded as `migrated`.
    pub unrecorded: Vec<FileRef>,
}

/// The moves that put the old flat content folder of `layout` (`<table>-<build>.<ext>`,
/// `<kind>-<build>-<level>.<ext>`, `retail/bundle-<build>.bin`, `retail/world-<build>-<level>.bin`)
/// into the layout.
///
/// A run can follow one that stopped half-way: an old file whose layout path holds the same bytes
/// is only removed, a merged file equal to a retail file already at its layout path still
/// becomes a hard link to it, and every file at its layout path that the manifest does not have
/// is recorded.
///
/// # Errors
///
/// Returns [`ToolError::MigrateTargetExists`] when a layout path holds other bytes, or
/// [`ToolError::ContentIo`] when a folder or a file cannot be read.
pub fn plan_migration(layout: &ContentLayout) -> Result<MigratePlan, ToolError> {
    let content_dir = layout.content_dir();
    let retail_dir = content_dir.join(LEGACY_RETAIL_FOLDER);
    let build_folder = layout.build_dir().file_name().map(ToOwned::to_owned);
    let mut moves = Vec::new();
    let mut left = Vec::new();
    for (dir, retail) in [(&retail_dir, true), (&content_dir.to_path_buf(), false)] {
        for path in entries_of(dir)? {
            let name = path.file_name().map(ToOwned::to_owned).unwrap_or_default();
            if !retail
                && (name == MAP_FOLDER
                    || name == LEGACY_RETAIL_FOLDER
                    || Some(&name) == build_folder.as_ref())
            {
                continue;
            }
            let found = name
                .to_str()
                .and_then(|name| legacy_file(name, layout.client_build(), retail));
            match found {
                Some(file) if path.is_file() => moves.push(Move {
                    to: layout.path(&file),
                    file,
                    from: path,
                    kind: MoveKind::Rename,
                }),
                _ => left.push(path),
            }
        }
    }
    for index in 0..moves.len() {
        let done = &moves[index];
        let kind = if done.to.exists() {
            if !same_bytes(&done.from, &done.to)? {
                return Err(ToolError::MigrateTargetExists {
                    path: done.to.clone(),
                });
            }
            MoveKind::RemoveDuplicate
        } else {
            match retail_source(layout, &moves, &done.file) {
                Some((retail_bytes, retail_to)) if same_bytes(&done.from, &retail_bytes)? => {
                    MoveKind::LinkToRetail(retail_to)
                }
                _ => MoveKind::Rename,
            }
        };
        moves[index].kind = kind;
    }
    moves.sort_by_key(|done| (done.file.file.topic != Topic::Retail, done.to.clone()));
    left.sort();
    Ok(MigratePlan {
        moves,
        left,
        unrecorded: unrecorded_files(layout)?,
    })
}

/// The retail file of the merged file `file` (when it has one): where its bytes are now (its
/// old path when it moves in this run, else its layout path) and its layout path.
fn retail_source(
    layout: &ContentLayout,
    moves: &[Move],
    file: &FileRef,
) -> Option<(PathBuf, PathBuf)> {
    let retail = retail_of(file)?;
    match moves.iter().find(|other| other.file == retail) {
        Some(other) => Some((other.from.clone(), other.to.clone())),
        None => {
            let path = layout.path(&retail);
            path.is_file().then(|| (path.clone(), path))
        }
    }
}

/// The registry files at their layout paths that the manifest of `layout` does not have.
fn unrecorded_files(layout: &ContentLayout) -> Result<Vec<FileRef>, ToolError> {
    let recorded = layout
        .read_manifest()?
        .map(|manifest| manifest.files)
        .unwrap_or_default();
    let mut found = Vec::new();
    for &file in FILES {
        let files = match file.scope {
            Scope::Level => layout
                .levels_with(nw_content::layout::LevelFile(file))?
                .into_iter()
                .map(|level| FileRef {
                    file,
                    level: Some(level),
                })
                .collect(),
            Scope::Build | Scope::FixedLevel(_) => vec![whole(file)],
        };
        found.extend(
            files
                .into_iter()
                .filter(|file| layout.path(file).is_file() && !recorded.contains_key(&file.key())),
        );
    }
    Ok(found)
}

/// Carries out `plan` in `layout`: renames every file (a byte-equal merged file becomes a hard
/// link to its retail file, an old file whose layout path holds its bytes is removed), records
/// the moved and the unrecorded files in the manifest as `migrated`, and removes the old
/// `retail/` folder when it is empty. Returns true when it was removed.
///
/// # Errors
///
/// Returns [`ToolError`] when a file cannot be moved or linked, or the manifest written.
pub fn migrate(layout: &ContentLayout, plan: &MigratePlan) -> Result<bool, ToolError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ToolError::ContentIo { path, source }
    };
    for done in &plan.moves {
        if let Some(folder) = done.to.parent() {
            std::fs::create_dir_all(folder).map_err(io(folder))?;
        }
        match &done.kind {
            MoveKind::Rename => std::fs::rename(&done.from, &done.to).map_err(io(&done.to))?,
            MoveKind::LinkToRetail(retail) => {
                link_atomic(retail, &done.to).map_err(io(&done.to))?;
                std::fs::remove_file(&done.from).map_err(io(&done.from))?;
            }
            MoveKind::RemoveDuplicate => {
                std::fs::remove_file(&done.from).map_err(io(&done.from))?;
            }
        }
    }
    let mut files: Vec<FileRef> = plan
        .moves
        .iter()
        .filter(|done| done.kind != MoveKind::RemoveDuplicate)
        .map(|done| done.file.clone())
        .collect();
    let unrecorded: Vec<FileRef> = plan
        .unrecorded
        .iter()
        .filter(|file| !files.contains(file))
        .cloned()
        .collect();
    files.extend(unrecorded);
    record(layout, &files, MADE_BY_MIGRATED)?;
    let retail_dir = layout.content_dir().join(LEGACY_RETAIL_FOLDER);
    match std::fs::remove_dir(&retail_dir) {
        Ok(()) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(false)
        }
        Err(source) => Err(ToolError::ContentIo {
            path: retail_dir,
            source,
        }),
    }
}

/// The entries of `dir`, none when it does not exist.
fn entries_of(dir: &Path) -> Result<Vec<PathBuf>, ToolError> {
    let io = |source| ToolError::ContentIo {
        path: dir.to_path_buf(),
        source,
    };
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|entry| entry.map(|entry| entry.path()).map_err(io))
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(io(source)),
    }
}

/// The registry file of the old flat name `name` of `client_build` (`retail`: a name of the old
/// `retail/` folder).
fn legacy_file(name: &str, client_build: &str, retail: bool) -> Option<FileRef> {
    FILES.iter().copied().find_map(|file| {
        if (file.topic == Topic::Retail) != retail {
            return None;
        }
        let stem = file
            .name
            .strip_prefix(RETAIL_NAME_PREFIX)
            .unwrap_or(file.name);
        let rest = name
            .strip_prefix(stem)?
            .strip_prefix('-')?
            .strip_prefix(client_build)?
            .strip_suffix(file.extension())?;
        match (file.scope, rest.strip_prefix('-')) {
            (Scope::Level, Some(level)) if !level.is_empty() => Some(FileRef {
                file,
                level: Some(level.to_owned()),
            }),
            (Scope::Build | Scope::FixedLevel(_), None) if rest.is_empty() => Some(whole(file)),
            _ => None,
        }
    })
}

/// The retail file a merged file is made from: the retail bundle of the bundle, the retail world
/// file of a level's world file.
fn retail_of(file: &FileRef) -> Option<FileRef> {
    if file.file == BUNDLE.0 {
        Some(FileRef::build(RETAIL_BUNDLE))
    } else if file.file == WORLD.0 {
        file.level
            .as_deref()
            .map(|level| FileRef::level(RETAIL_WORLD, level))
    } else {
        None
    }
}

/// True when the files `left` and `right` hold the same bytes.
///
/// # Errors
///
/// Returns [`ToolError::ContentIo`] when one cannot be read.
fn same_bytes(left: &Path, right: &Path) -> Result<bool, ToolError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ToolError::ContentIo { path, source }
    };
    let length = |path: &Path| std::fs::metadata(path).map(|metadata| metadata.len());
    if length(left).map_err(io(left))? != length(right).map_err(io(right))? {
        return Ok(false);
    }
    let mut left_file = std::fs::File::open(left).map_err(io(left))?;
    let mut right_file = std::fs::File::open(right).map_err(io(right))?;
    let mut left_chunk = vec![0; COMPARE_CHUNK];
    let mut right_chunk = vec![0; COMPARE_CHUNK];
    loop {
        let read = read_full(&mut left_file, &mut left_chunk).map_err(io(left))?;
        let other = read_full(&mut right_file, &mut right_chunk).map_err(io(right))?;
        if read != other || left_chunk[..read] != right_chunk[..other] {
            return Ok(false);
        }
        if read == 0 {
            return Ok(true);
        }
    }
}

/// Reads into `buffer` until it is full or the file ends; the bytes read.
fn read_full(file: &mut std::fs::File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use nw_content::layout::{
        AI_DATA, COLLISION, CREATURE_ANIMATIONS, FTUE_ENTITIES, MOB_PREFABS, OPEN_WORLD_LEVEL,
        TUTORIAL_LEVEL, write_atomic,
    };

    use super::*;

    const BUILD: &str = "12345";
    /// A level of the test folders.
    const LEVEL: &str = "lv";

    /// A content folder with a retail world file of [`LEVEL`], so the world steps have a level.
    fn layout_with_a_level(dir: &Path) -> ContentLayout {
        let layout = ContentLayout::new(dir, BUILD);
        write_atomic(&layout.level_file(RETAIL_WORLD, LEVEL), b"world").unwrap();
        layout
    }

    fn options(dir: &Path) -> BuildOptions {
        BuildOptions {
            rebuild: false,
            derived_only: false,
            has_game_dir: true,
            design_dir: dir.join("content"),
            server_pak: dir.join("Server.pak"),
        }
    }

    /// The steps `build_content` runs and its lines, with a runner that writes and records every
    /// output of a step.
    fn build(
        layout: &ContentLayout,
        options: &BuildOptions,
    ) -> Result<(Vec<&'static str>, Vec<BuildLine>), ToolError> {
        let mut ran = Vec::new();
        let mut lines = Vec::new();
        build_content(
            layout,
            options,
            |spec| {
                let outputs = step_outputs(layout, spec)?;
                for output in &outputs {
                    write_atomic(&layout.path(output), spec.command.as_bytes()).unwrap();
                }
                record(layout, &outputs, MADE_BY)?;
                ran.push(spec.command);
                Ok(())
            },
            |line| lines.push(line.clone()),
        )?;
        Ok((ran, lines))
    }

    fn outcome_of<'a>(lines: &'a [BuildLine], what: &str) -> &'a BuildOutcome {
        &lines
            .iter()
            .find(|line| line.what == what)
            .unwrap_or_else(|| panic!("no line for {what}"))
            .outcome
    }

    #[test]
    fn the_first_build_runs_every_step_and_names_the_missing_pack_files() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_with_a_level(dir.path());
        let (ran, lines) = build(&layout, &options(dir.path())).unwrap();
        assert_eq!(ran[0], "build");
        assert!(ran.contains(&"design") && ran.contains(&"store"));
        assert_eq!(
            outcome_of(&lines, "mob-prefabs"),
            &BuildOutcome::PackMissing
        );
        assert_eq!(
            outcome_of(&lines, "creature-animations"),
            &BuildOutcome::NoPack(vec!["mob-prefabs"])
        );
        assert_eq!(outcome_of(&lines, "ai-data"), &BuildOutcome::NoServerPak);
    }

    #[test]
    fn a_second_build_keeps_fresh_files_and_rebuild_runs_them_again() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_with_a_level(dir.path());
        let options = options(dir.path());
        build(&layout, &options).unwrap();
        let (ran, lines) = build(&layout, &options).unwrap();
        assert!(ran.is_empty(), "{ran:?}");
        assert_eq!(outcome_of(&lines, "store"), &BuildOutcome::Fresh);
        let rebuild = BuildOptions {
            rebuild: true,
            ..options
        };
        let (ran, _) = build(&layout, &rebuild).unwrap();
        assert!(ran.contains(&"store"));
    }

    #[test]
    fn a_newer_input_makes_its_step_old() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_with_a_level(dir.path());
        let options = options(dir.path());
        build(&layout, &options).unwrap();
        let bundle = std::fs::File::options()
            .write(true)
            .open(layout.file(BUNDLE))
            .unwrap();
        bundle
            .set_modified(SystemTime::now() + std::time::Duration::from_secs(60))
            .unwrap();
        let (ran, _) = build(&layout, &options).unwrap();
        assert!(ran.contains(&"store") && ran.contains(&"leaderboards"));
        assert!(!ran.contains(&"water"), "{ran:?}");
    }

    #[test]
    fn derived_builds_only_the_derived_steps() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        write_atomic(&layout.file(MOB_PREFABS), b"[]").unwrap();
        std::fs::write(dir.path().join("Server.pak"), b"pak").unwrap();
        let derived = BuildOptions {
            derived_only: true,
            ..options(dir.path())
        };
        let (ran, _) = build(&layout, &derived).unwrap();
        assert!(ran.contains(&"creature-animations") && ran.contains(&"ai-data"));
        assert!(!ran.contains(&"build") && !ran.contains(&"water"));
        assert!(layout.file(AI_DATA).exists());
    }

    #[test]
    fn no_game_dir_keeps_existing_files_and_refuses_missing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        let no_game = BuildOptions {
            has_game_dir: false,
            ..options(dir.path())
        };
        assert!(matches!(
            build(&layout, &no_game),
            Err(ToolError::StepNeedsGameDir { step: "build" })
        ));
        write_atomic(&layout.file(RETAIL_BUNDLE), b"old").unwrap();
        assert!(matches!(
            build(&layout, &no_game),
            Err(ToolError::StepNeedsGameDir { step: "world" })
        ));
    }

    #[test]
    fn status_names_every_file_with_its_state() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        write_atomic(&layout.file(CREATURE_ANIMATIONS), b"[]").unwrap();
        record(&layout, &[FileRef::build(MOB_PREFABS)], MADE_BY).unwrap();
        write_atomic(&layout.file(MOB_PREFABS), b"[]").unwrap();
        let rows = content_status(&layout, &options(dir.path())).unwrap();
        let state = |key: &str| {
            rows.iter()
                .find(|row| row.key == key)
                .unwrap_or_else(|| panic!("no row {key}"))
                .state
        };
        assert_eq!(state("mob-prefabs"), FileState::Ok);
        assert_eq!(state("creature-animations"), FileState::Old);
        assert_eq!(state("ftue-entities"), FileState::Missing);
        assert_eq!(state("collision/ftue_v2"), FileState::Missing);
        assert_eq!(state("collision/nw_catacomb_00"), FileState::Missing);
        assert_eq!(state("world/<level>"), FileState::Missing);
        assert!(
            rows.iter()
                .any(|row| row.key == "mob-prefabs" && row.made_at.is_some())
        );
    }

    /// An old flat folder: the retail bundle and world, a merged world equal to retail, a merged
    /// world that differs, a table, a level file, the tutorial table, an unknown file, and map
    /// tiles.
    fn flat_folder(dir: &Path) {
        let retail = dir.join(LEGACY_RETAIL_FOLDER);
        std::fs::create_dir_all(&retail).unwrap();
        std::fs::create_dir_all(dir.join(MAP_FOLDER).join(LEVEL)).unwrap();
        for (path, bytes) in [
            (retail.join(format!("bundle-{BUILD}.bin")), &b"bundle"[..]),
            (retail.join(format!("world-{BUILD}-{LEVEL}.bin")), b"world"),
            (
                retail.join(format!("world-{BUILD}-{OPEN_WORLD_LEVEL}.bin")),
                b"open world",
            ),
            (dir.join(format!("bundle-{BUILD}.bin")), b"bundle"),
            (dir.join(format!("world-{BUILD}-{LEVEL}.bin")), b"world"),
            (
                dir.join(format!("world-{BUILD}-{OPEN_WORLD_LEVEL}.bin")),
                b"merged open world",
            ),
            (dir.join(format!("mob-prefabs-{BUILD}.json")), b"[]"),
            (
                dir.join(format!("collision-{BUILD}-{TUTORIAL_LEVEL}.bin")),
                b"solids",
            ),
            (dir.join(format!("ftue-entities-{BUILD}.json")), b"{}"),
            (dir.join("notes.txt"), b"mine"),
        ] {
            std::fs::write(path, bytes).unwrap();
        }
    }

    #[test]
    fn migrate_moves_the_flat_folder_and_links_equal_merged_files() {
        let dir = tempfile::tempdir().unwrap();
        flat_folder(dir.path());
        let layout = ContentLayout::new(dir.path(), BUILD);
        let plan = plan_migration(&layout).unwrap();
        assert_eq!(plan.moves.len(), 9);
        assert_eq!(plan.left, [dir.path().join("notes.txt")]);
        assert_eq!(plan.moves[0].file.file.topic, Topic::Retail);
        assert!(
            migrate(&layout, &plan).unwrap(),
            "the empty retail folder is removed"
        );
        let same = |left: &Path, right: &Path| {
            let (left, right) = (
                std::fs::metadata(left).unwrap(),
                std::fs::metadata(right).unwrap(),
            );
            left.ino() == right.ino()
        };
        assert!(same(&layout.file(BUNDLE), &layout.file(RETAIL_BUNDLE)));
        assert!(same(
            &layout.level_file(WORLD, LEVEL),
            &layout.level_file(RETAIL_WORLD, LEVEL)
        ));
        assert!(!same(
            &layout.level_file(WORLD, OPEN_WORLD_LEVEL),
            &layout.level_file(RETAIL_WORLD, OPEN_WORLD_LEVEL)
        ));
        assert_eq!(
            std::fs::read(layout.level_file(WORLD, OPEN_WORLD_LEVEL)).unwrap(),
            b"merged open world"
        );
        assert!(layout.level_file(COLLISION, TUTORIAL_LEVEL).exists());
        assert!(layout.file(FTUE_ENTITIES).exists());
        assert!(dir.path().join("notes.txt").exists());
        assert!(dir.path().join(MAP_FOLDER).join(LEVEL).exists());
        assert!(!dir.path().join(format!("bundle-{BUILD}.bin")).exists());
        let manifest = layout.read_manifest().unwrap().expect("manifest");
        assert_eq!(manifest.files.len(), 9);
        assert_eq!(manifest.files["mob-prefabs"].made_by, MADE_BY_MIGRATED);
        assert_eq!(manifest.files["retail-world/lv"].source, Source::Retail);
        // A hard link keeps the time of its retail file; the manifest time keeps it fresh.
        let rows = content_status(&layout, &options(dir.path())).unwrap();
        for done in &plan.moves {
            let key = done.file.key();
            let row = rows
                .iter()
                .find(|row| row.key == key)
                .unwrap_or_else(|| panic!("no row {key}"));
            assert_eq!(row.state, FileState::Ok, "{key}");
        }
    }

    #[test]
    fn migrate_refuses_a_taken_target() {
        let dir = tempfile::tempdir().unwrap();
        flat_folder(dir.path());
        let layout = ContentLayout::new(dir.path(), BUILD);
        write_atomic(&layout.file(MOB_PREFABS), b"[1]").unwrap();
        assert!(matches!(
            plan_migration(&layout),
            Err(ToolError::MigrateTargetExists { .. })
        ));
    }

    #[test]
    fn a_rerun_removes_an_old_file_whose_layout_copy_is_equal() {
        let dir = tempfile::tempdir().unwrap();
        flat_folder(dir.path());
        let layout = ContentLayout::new(dir.path(), BUILD);
        // The run before linked the merged bundle and stopped before it removed the old file.
        let old_bundle = dir.path().join(format!("bundle-{BUILD}.bin"));
        write_atomic(&layout.file(BUNDLE), b"bundle").unwrap();
        let plan = plan_migration(&layout).unwrap();
        let bundle = plan
            .moves
            .iter()
            .find(|done| done.file == FileRef::build(BUNDLE))
            .expect("the bundle");
        assert_eq!(bundle.kind, MoveKind::RemoveDuplicate);
        migrate(&layout, &plan).unwrap();
        assert!(!old_bundle.exists());
        assert_eq!(std::fs::read(layout.file(BUNDLE)).unwrap(), b"bundle");
    }

    #[test]
    fn a_rerun_records_the_files_already_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        // The run before moved these and stopped before it wrote the manifest.
        write_atomic(&layout.file(MOB_PREFABS), b"[]").unwrap();
        write_atomic(&layout.level_file(COLLISION, TUTORIAL_LEVEL), b"solids").unwrap();
        let plan = plan_migration(&layout).unwrap();
        assert!(plan.moves.is_empty());
        assert_eq!(plan.unrecorded.len(), 2);
        migrate(&layout, &plan).unwrap();
        let manifest = layout.read_manifest().unwrap().expect("manifest");
        assert_eq!(manifest.files["mob-prefabs"].made_by, MADE_BY_MIGRATED);
        assert_eq!(
            manifest.files["collision/ftue_v2"].made_by,
            MADE_BY_MIGRATED
        );
        assert!(plan_migration(&layout).unwrap().unrecorded.is_empty());
    }

    #[test]
    fn a_rerun_links_a_merged_file_to_a_retail_file_already_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ContentLayout::new(dir.path(), BUILD);
        // The run before moved the retail world and stopped before the merged one.
        write_atomic(&layout.level_file(RETAIL_WORLD, LEVEL), b"world").unwrap();
        std::fs::write(
            dir.path().join(format!("world-{BUILD}-{LEVEL}.bin")),
            b"world",
        )
        .unwrap();
        let plan = plan_migration(&layout).unwrap();
        assert_eq!(
            plan.moves[0].kind,
            MoveKind::LinkToRetail(layout.level_file(RETAIL_WORLD, LEVEL))
        );
        migrate(&layout, &plan).unwrap();
        let inode = |path: PathBuf| std::fs::metadata(path).unwrap().ino();
        assert_eq!(
            inode(layout.level_file(WORLD, LEVEL)),
            inode(layout.level_file(RETAIL_WORLD, LEVEL))
        );
    }

    #[test]
    fn legacy_names_map_to_their_registry_files() {
        assert_eq!(
            legacy_file("dungeon-gate-rules-1.json", "1", false),
            Some(FileRef::build(nw_content::layout::DUNGEON_GATE_RULES))
        );
        assert_eq!(
            legacy_file("level-doors-1-newworld_vitaeeterna.json", "1", false),
            Some(FileRef::level(
                nw_content::layout::LEVEL_DOORS,
                OPEN_WORLD_LEVEL
            ))
        );
        assert_eq!(
            legacy_file("world-1-lv.bin", "1", true).map(|file| file.key()),
            Some("retail-world/lv".to_owned())
        );
        assert_eq!(legacy_file("mob-prefabs-2.json", "1", false), None);
        assert_eq!(legacy_file("world-1.bin", "1", false), None);
    }

    #[test]
    fn same_bytes_compares_every_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            dir.path().join("a"),
            dir.path().join("b"),
            dir.path().join("c"),
        );
        let mut long = vec![7; COMPARE_CHUNK + 1];
        std::fs::write(&a, &long).unwrap();
        std::fs::write(&b, &long).unwrap();
        long[COMPARE_CHUNK] = 8;
        std::fs::write(&c, &long).unwrap();
        assert!(same_bytes(&a, &b).unwrap());
        assert!(!same_bytes(&a, &c).unwrap());
    }
}
