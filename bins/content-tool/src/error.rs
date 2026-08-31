// SPDX-License-Identifier: AGPL-3.0-or-later
//! Top-level error of `content-tool`.

use nw_common::config::ConfigError;
use nw_content::fill::FillError;

use crate::codegen::CodegenError;
use crate::source::SourceError;

/// Any error of a `content-tool` command.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Reading the install failed.
    #[error(transparent)]
    Source(#[from] SourceError),
    /// Code generation failed.
    #[error(transparent)]
    Codegen(#[from] CodegenError),
    /// A datasheet does not fit its generated struct.
    #[error(transparent)]
    Fill(#[from] FillError),
    /// The client build file is missing or invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
}
