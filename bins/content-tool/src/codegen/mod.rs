// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generates typed Rust structs for every datasheet table type (M1 spec section 4).

pub mod naming;
pub mod render;
pub mod schema;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use render::{GENERATED_MARKER, GeneratedFile, RUST_EXTENSION, render};
use schema::SchemaBuilder;

use crate::error::ToolError;
use crate::source::GameSource;

/// The marker is on line 2 of every generated file.
const MARKER_HEADER_LINES: usize = 2;

/// Error from code generation.
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    /// No datasheet was read.
    #[error("no datasheets were found")]
    NoTables,
    /// A table type name (or its module name) is not a Rust identifier.
    #[error("table type `{0}` is not a valid Rust type or module name")]
    InvalidTypeName(String),
    /// A table type has no columns in any file.
    #[error("table type `{0}` has no columns")]
    NoColumns(String),
    /// Two table types give the same module name.
    #[error("table types `{first}` and `{second}` both map to module `{module}`")]
    ModuleCollision {
        /// First type (sorted order).
        first: String,
        /// Second type.
        second: String,
        /// The shared module name.
        module: String,
    },
    /// Two table types give the same struct name.
    #[error("table types `{first}` and `{second}` both map to struct `{struct_name}`")]
    StructCollision {
        /// First type (sorted order).
        first: String,
        /// Second type.
        second: String,
        /// The shared struct name.
        struct_name: String,
    },
    /// Generated tokens do not form a valid Rust file.
    #[error("generated code for `{name}` does not parse: {message}")]
    Render {
        /// File name.
        name: String,
        /// Parser message.
        message: String,
    },
    /// A file in the output folder cannot be read, written or removed.
    #[error("cannot access {}: {source}", .path.display())]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// Result of [`write_generated`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteSummary {
    /// Files written.
    pub written: usize,
    /// Stale generated files removed.
    pub removed: usize,
}

/// Writes `files` into `out_dir` and removes generated `.rs` files there that are no longer
/// produced. Files without [`GENERATED_MARKER`] are never removed.
pub fn write_generated(
    out_dir: &Path,
    files: &[GeneratedFile],
) -> Result<WriteSummary, CodegenError> {
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| CodegenError::Io { path, source }
    };
    std::fs::create_dir_all(out_dir).map_err(io_error(out_dir))?;
    let keep: BTreeSet<&str> = files.iter().map(|file| file.name.as_str()).collect();
    let mut removed = 0;
    for item in std::fs::read_dir(out_dir).map_err(io_error(out_dir))? {
        let path = item.map_err(io_error(out_dir))?.path();
        let is_rust = path
            .extension()
            .is_some_and(|extension| extension == RUST_EXTENSION);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !is_rust || keep.contains(name) {
            continue;
        }
        let contents = std::fs::read_to_string(&path).map_err(io_error(&path))?;
        if contents
            .lines()
            .take(MARKER_HEADER_LINES)
            .any(|line| line.contains(GENERATED_MARKER))
        {
            std::fs::remove_file(&path).map_err(io_error(&path))?;
            removed += 1;
        }
    }
    for file in files {
        let path = out_dir.join(&file.name);
        std::fs::write(&path, &file.contents).map_err(io_error(&path))?;
    }
    Ok(WriteSummary {
        written: files.len(),
        removed,
    })
}

/// Result of [`run_codegen`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodegenSummary {
    /// Datasheets read.
    pub datasheet_files: usize,
    /// Table types (structs) generated.
    pub table_types: usize,
    /// Files written.
    pub files_written: usize,
    /// Stale generated files removed.
    pub stale_files_removed: usize,
}

/// Reads every datasheet, infers the schemas, and writes the generated code into `out_dir`.
pub fn run_codegen(
    source: &GameSource,
    client_build: &str,
    out_dir: &Path,
) -> Result<CodegenSummary, ToolError> {
    let mut builder = SchemaBuilder::default();
    let datasheet_files = source.for_each_datasheet(|_, sheet| {
        builder.add(&sheet);
        Ok::<(), ToolError>(())
    })?;
    let schemas = builder.finish()?;
    let files = render(&schemas, client_build)?;
    let written = write_generated(out_dir, &files)?;
    Ok(CodegenSummary {
        datasheet_files,
        table_types: schemas.len(),
        files_written: written.written,
        stale_files_removed: written.removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_generated_removes_only_stale_generated_files() {
        let dir = tempfile::tempdir().unwrap();
        let generated_header =
            format!("// SPDX-License-Identifier: AGPL-3.0-or-later\n// {GENERATED_MARKER} x\n");
        std::fs::write(dir.path().join("old_table.rs"), &generated_header).unwrap();
        std::fs::write(dir.path().join("notes.rs"), "// hand written\n").unwrap();
        std::fs::write(dir.path().join("keep.txt"), &generated_header).unwrap();
        let files = vec![GeneratedFile {
            name: "mod.rs".into(),
            contents: "// new\n".into(),
        }];

        let summary = write_generated(dir.path(), &files).unwrap();

        assert_eq!(
            summary,
            WriteSummary {
                written: 1,
                removed: 1
            }
        );
        assert!(!dir.path().join("old_table.rs").exists());
        assert!(dir.path().join("notes.rs").exists());
        assert!(dir.path().join("keep.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("mod.rs")).unwrap(),
            "// new\n"
        );
    }

    #[test]
    fn write_generated_creates_the_output_folder() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("a/b");
        let files = vec![GeneratedFile {
            name: "mod.rs".into(),
            contents: String::new(),
        }];
        assert_eq!(
            write_generated(&out, &files).unwrap(),
            WriteSummary {
                written: 1,
                removed: 0
            }
        );
        assert!(out.join("mod.rs").exists());
    }
}
