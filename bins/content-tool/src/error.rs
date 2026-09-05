// SPDX-License-Identifier: AGPL-3.0-or-later
//! Top-level error of `content-tool`.

use nw_common::config::ConfigError;
use nw_content::ContainerError;
use nw_content::combat::CombatDataError;
use nw_content::fill::FillError;

use crate::capture::CaptureError;
use crate::codegen::CodegenError;
use crate::combat::NwdbError;
use crate::source::SourceError;
use crate::world::WorldBuildError;

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
    /// The bundle cannot be written or read back.
    #[error("content bundle: {0}")]
    Bundle(#[from] ContainerError),
    /// A world file cannot be built, written or read back.
    #[error(transparent)]
    World(#[from] WorldBuildError),
    /// The reloaded bundle does not hold what was written.
    #[error("bundle check failed: {what} is {found} after reload, expected {expected}")]
    CountMismatch {
        /// What was counted.
        what: &'static str,
        /// Count before writing.
        expected: usize,
        /// Count after reload.
        found: usize,
    },
    /// A capture ledger or a type name file cannot be read.
    #[error(transparent)]
    Capture(#[from] CaptureError),
    /// A config row of the combat check is not in the bundle.
    #[error(transparent)]
    Combat(#[from] CombatDataError),
    /// The nwdb.info records cannot be read.
    #[error(transparent)]
    Nwdb(#[from] NwdbError),
    /// The capture check found decode failures.
    #[error("capture check found {count} decode failures")]
    CaptureFailures {
        /// Failures found.
        count: usize,
    },
}
