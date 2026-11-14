// SPDX-License-Identifier: AGPL-3.0-or-later
//! The realm data pack: the content tables that only retail captures give, scrubbed of player
//! data, zipped with a manifest (`content-tool realm-pack make`), and installed into a content
//! folder (`content-tool realm-pack install`). The pack holds no capture, no capture name and no
//! `Server.pak` data; every other table is built from the person's own install.

pub mod install;
pub mod make;
pub mod manifest;
pub mod owner;
pub mod scan;
pub mod scrub;
pub mod strings;

use std::path::PathBuf;

use scan::ScanHit;

/// Why a pack cannot be made or installed.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// The pack is for another client build.
    #[error("the pack is for client build {pack}, this server is build {server}")]
    WrongBuild {
        /// The pack's build.
        pack: String,
        /// The server's build.
        server: String,
    },
    /// The pack has another format than this code reads.
    #[error(
        "the pack has format {pack}, this server reads format {code}: get the pack that matches this server"
    )]
    WrongFormat {
        /// The pack's format.
        pack: u32,
        /// The format this code reads.
        code: u32,
    },
    /// A file's bytes do not match the manifest.
    #[error("{file}: the checksum or size does not match the manifest")]
    Checksum {
        /// The file.
        file: String,
    },
    /// The zip holds a file the manifest does not name, or a name with a folder in it.
    #[error("{file}: not a file of this pack")]
    UnknownFile {
        /// The entry name.
        file: String,
    },
    /// A file the pack needs is missing.
    #[error("{file}: missing")]
    MissingFile {
        /// The file.
        file: String,
    },
    /// A table does not load with its loader.
    #[error("{table}: does not load: {reason}")]
    TableLoad {
        /// The table.
        table: String,
        /// Why.
        reason: String,
    },
    /// A replicated state in a table does not decode.
    #[error("{table} {path}: a replicated state does not decode: {reason}")]
    MemberDecode {
        /// The table.
        table: String,
        /// The JSON path of the state.
        path: String,
        /// Why.
        reason: String,
    },
    /// The privacy scan found data that must not leave this machine.
    #[error("the privacy scan found {} hit(s), first: {}", .0.len(), .0.first().map(ToString::to_string).unwrap_or_default())]
    PrivacyHits(Vec<ScanHit>),
    /// A table of the pack does not fit this server's code.
    #[error(
        "pack version {pack_version} (made from server commit {source_commit}) does not fit this server: {reason}"
    )]
    PackDoesNotFit {
        /// The pack version.
        pack_version: u32,
        /// The server commit the pack was made from.
        source_commit: String,
        /// Why.
        reason: Box<PackError>,
    },
    /// A file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// A table or the manifest is not valid JSON.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// The zip cannot be read or written.
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// The content bundle cannot be read.
    #[error("content bundle: {0}")]
    Bundle(#[from] nw_content::ContainerError),
}

/// A [`PackError::Io`] for `path`.
pub(crate) fn io_error(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> PackError {
    let path = path.into();
    move |source| PackError::Io { path, source }
}
