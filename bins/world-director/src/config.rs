// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/world-director.toml` with `NW_DIRECTOR_` overrides (M5 spec section 6).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use nw_common::config::{ConfigError, SECRET_ENV_PREFIX, load_config};
use nw_db::DatabaseConfig;
use nw_sim_core::{Ground, Position};
use serde::Deserialize;
use uuid::Uuid;

/// Environment prefix of `world-director` overrides.
pub const DIRECTOR_ENV_PREFIX: &str = "NW_DIRECTOR_";

/// The whole `world-director` configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectorConfig {
    /// The world, the same `[[world]] id` as in `config/edge.toml`.
    pub world_id: Uuid,
    /// NATS, internal only.
    pub nats_url: String,
    /// Valkey of `edge-http`, for the free player slots.
    pub valkey_url: String,
    /// The world database.
    pub world_database: DatabaseConfig,
    /// The environment variable with the world database password; it starts with `NW_SECRET_`.
    pub world_database_password_env: String,
    /// The pinned client build file.
    pub client_build_config: PathBuf,
    /// Folder of the world files (`world-<build>-<level>.bin`).
    pub content_dir: PathBuf,
    /// The open map level.
    pub level: String,
    /// Where characters that were never in the world appear, in turn.
    pub start_positions: Vec<StartPosition>,
    /// Edge length of a cell, in metres.
    pub cell_size_m: f32,
    /// Longest wait for a sim to take a player.
    pub admit_timeout_ms: u64,
    /// Time between table publications and free-slot reports; sims beat at the same interval.
    pub heartbeat_interval_ms: u64,
    /// Missed sim heartbeats before its cells go to the other sims.
    pub heartbeat_misses: u32,
    /// Internal Prometheus listener.
    pub metrics_listen: SocketAddr,
    /// `tracing` filter, for example `info`.
    pub log_filter: String,
}

/// A start position, in metres. The height comes from the ground of the level.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartPosition {
    /// East.
    pub x: f32,
    /// North.
    pub y: f32,
}

/// A value of the configuration that is not allowed.
#[derive(Debug, thiserror::Error)]
pub enum DirectorConfigError {
    /// The file cannot be read or has a wrong key or type.
    #[error(transparent)]
    Load(#[from] ConfigError),
    /// A value is not allowed.
    #[error("config value `{key}` is not valid: {reason}")]
    Invalid {
        /// Key path.
        key: &'static str,
        /// Why the value is refused.
        reason: &'static str,
    },
}

/// Reads and checks the configuration.
///
/// # Errors
///
/// Returns [`DirectorConfigError`] when the file is missing, has a wrong key, or a value is not
/// allowed.
pub fn load_director_config(path: &Path) -> Result<DirectorConfig, DirectorConfigError> {
    let config: DirectorConfig = load_config(path, DIRECTOR_ENV_PREFIX)?;
    config.validate()?;
    Ok(config)
}

impl DirectorConfig {
    fn validate(&self) -> Result<(), DirectorConfigError> {
        let invalid = |key, reason| Err(DirectorConfigError::Invalid { key, reason });
        if !self
            .world_database_password_env
            .starts_with(SECRET_ENV_PREFIX)
        {
            return invalid("world_database_password_env", "must start with NW_SECRET_");
        }
        if self.level.is_empty() {
            return invalid("level", "must not be empty");
        }
        let finite = |start: &StartPosition| start.x.is_finite() && start.y.is_finite();
        if self.start_positions.is_empty() || !self.start_positions.iter().all(finite) {
            return invalid("start_positions", "must hold at least one finite position");
        }
        if !(self.cell_size_m.is_finite() && self.cell_size_m > 0.0) {
            return invalid("cell_size_m", "must be a finite number above 0");
        }
        if self.admit_timeout_ms == 0 || self.heartbeat_interval_ms == 0 {
            return invalid("timeouts", "must be above 0");
        }
        if self.heartbeat_misses == 0 {
            return invalid("heartbeat_misses", "must be above 0");
        }
        Ok(())
    }

    /// The start positions on the ground of `ground`.
    ///
    /// # Errors
    ///
    /// Returns [`DirectorConfigError::Invalid`] when the level has no height at one of them.
    pub fn starts(&self, ground: &impl Ground) -> Result<Vec<Position>, DirectorConfigError> {
        self.start_positions
            .iter()
            .map(|&StartPosition { x, y }| {
                let z = ground.height_at(x, y).ok_or(DirectorConfigError::Invalid {
                    key: "start_positions",
                    reason: "the level has no ground height at one of them",
                })?;
                Ok(Position { x, y, z })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nw_sim_core::Ground;

    use super::*;

    const COMMITTED_CONFIG: &str = "../../config/world-director.toml";
    const GROUND_HEIGHT: f32 = 168.0;

    struct Flat;

    impl Ground for Flat {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_HEIGHT)
        }
    }

    struct Nothing;

    impl Ground for Nothing {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            None
        }
    }

    fn committed() -> DirectorConfig {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED_CONFIG);
        load_director_config(&path).expect("committed config")
    }

    #[test]
    fn the_committed_config_is_valid_and_starts_on_the_ground() {
        let config = committed();
        assert_eq!(config.level, "newworld_vitaeeterna");
        let starts = config.starts(&Flat).expect("starts");
        assert_eq!(starts[0].z, GROUND_HEIGHT);
        assert_eq!(starts[0].x, config.start_positions[0].x);
        assert!(config.starts(&Nothing).is_err());
        assert!(config.admit_timeout_ms > 0);
    }

    #[test]
    fn bad_values_are_refused() {
        let mut config = committed();
        config.world_database_password_env = "PASSWORD".to_owned();
        assert!(config.validate().is_err());
        let mut config = committed();
        config.start_positions[0].y = f32::NAN;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.start_positions.clear();
        assert!(config.validate().is_err());
        let mut config = committed();
        config.cell_size_m = 0.0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.heartbeat_misses = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.admit_timeout_ms = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.level = String::new();
        assert!(config.validate().is_err());
    }
}
