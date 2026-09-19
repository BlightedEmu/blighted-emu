// SPDX-License-Identifier: AGPL-3.0-or-later
//! Top-level error of `content-tool`.

use nw_common::config::ConfigError;
use nw_content::ContainerError;
use nw_content::combat::CombatDataError;
use nw_content::fill::FillError;
use nw_design::DesignRunError;

use crate::codegen::CodegenError;
use crate::combat::NwdbError;
use crate::creature_animations::CreatureAnimationsError;
use crate::ftue_entities::FtueEntitiesError;
use crate::map_tiles::MapTilesError;
use crate::mob_prefabs::MobPrefabsError;
use crate::source::SourceError;
use crate::world::WorldBuildError;
use nw_capture::CaptureError;

/// Any error of a `content-tool` command.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// A map tile cannot be decoded or written.
    #[error(transparent)]
    MapTiles(#[from] MapTilesError),
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
    /// The mob prefab scan failed.
    #[error(transparent)]
    MobPrefabs(#[from] MobPrefabsError),
    /// The creature animation build failed.
    #[error(transparent)]
    CreatureAnimations(#[from] CreatureAnimationsError),
    /// The tutorial static entity scan failed.
    #[error(transparent)]
    FtueEntities(#[from] FtueEntitiesError),
    /// The design step cannot read or write a retail or merged file.
    #[error("design: {0}")]
    Design(#[from] DesignRunError),
    /// The design step found problems in design files.
    #[error("design check found {count} problems")]
    DesignErrors {
        /// Problems found.
        count: usize,
    },
    /// The command reads the game install, and none was given.
    #[error("this command reads the game install: set NW_GAME_DIR or pass --game-dir")]
    MissingGameDir,
    /// The capture check found decode failures.
    #[error("capture check found {count} decode failures")]
    CaptureFailures {
        /// Failures found.
        count: usize,
    },
}
