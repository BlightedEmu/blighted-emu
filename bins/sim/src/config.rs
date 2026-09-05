// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/sim.toml` with `NW_SIM_` overrides (M4 spec sections 6.3 and 7).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use nw_bus::BusId;
use nw_common::config::{ConfigError, SECRET_ENV_PREFIX, load_config};
use nw_db::DatabaseConfig;
use nw_sim_core::{Ground, Position};
use serde::Deserialize;
use uuid::Uuid;

/// Environment prefix of `sim` overrides.
pub const SIM_ENV_PREFIX: &str = "NW_SIM_";

/// The whole `sim` configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimConfig {
    /// This sim's id in bus subjects; `rep-gateway` names the same id.
    pub sim_id: BusId,
    /// The world, the same `[[world]] id` as in `config/edge.toml`.
    pub world_id: Uuid,
    /// NATS, internal only.
    pub nats_url: String,
    /// The world database.
    pub world_database: DatabaseConfig,
    /// The environment variable with the world database password; it starts with `NW_SECRET_`.
    pub world_database_password_env: String,
    /// The pinned client build file.
    pub client_build_config: PathBuf,
    /// Folder of the world files (`world-<build>-<level>.bin`).
    pub content_dir: PathBuf,
    /// The level of this cell.
    pub level: String,
    /// Where a character that was never in the world appears.
    pub start_position: StartPosition,
    /// Players see each other inside this radius, in metres.
    pub interest_radius_m: f32,
    /// How far a position may be below the ground, in metres.
    pub ground_tolerance_m: f32,
    /// How far a position may be above the ground, in metres.
    pub max_height_above_ground_m: f32,
    /// Time between position saves.
    pub save_interval_ms: u64,
    /// Time between heartbeats.
    pub heartbeat_interval_ms: u64,
    /// Missed gateway heartbeats before its players leave.
    pub heartbeat_misses: u32,
    /// Internal Prometheus listener.
    pub metrics_listen: SocketAddr,
    /// `tracing` filter, for example `info`.
    pub log_filter: String,
}

/// `[start_position]`, in metres.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartPosition {
    /// East.
    pub x: f32,
    /// North.
    pub y: f32,
    /// Up.
    pub z: f32,
}

impl StartPosition {
    /// As a simulation position.
    pub const fn position(&self) -> Position {
        Position {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

/// A value of the configuration that is not allowed.
#[derive(Debug, thiserror::Error)]
pub enum SimConfigError {
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
/// Returns [`SimConfigError`] when the file is missing, has a wrong key, or a value is not
/// allowed.
pub fn load_sim_config(path: &Path) -> Result<SimConfig, SimConfigError> {
    let config: SimConfig = load_config(path, SIM_ENV_PREFIX)?;
    config.validate()?;
    Ok(config)
}

impl SimConfig {
    fn validate(&self) -> Result<(), SimConfigError> {
        let invalid = |key, reason| Err(SimConfigError::Invalid { key, reason });
        if !self
            .world_database_password_env
            .starts_with(SECRET_ENV_PREFIX)
        {
            return invalid("world_database_password_env", "must start with NW_SECRET_");
        }
        if self.level.is_empty() {
            return invalid("level", "must not be empty");
        }
        let start = self.start_position;
        if !(start.x.is_finite() && start.y.is_finite() && start.z.is_finite()) {
            return invalid("start_position", "must be finite");
        }
        if !(self.interest_radius_m.is_finite() && self.interest_radius_m > 0.0) {
            return invalid("interest_radius_m", "must be a finite number above 0");
        }
        let band = [self.ground_tolerance_m, self.max_height_above_ground_m];
        if band
            .iter()
            .any(|value| !(value.is_finite() && *value >= 0.0))
        {
            return invalid("ground", "tolerances must be finite numbers of at least 0");
        }
        if self.save_interval_ms == 0 || self.heartbeat_interval_ms == 0 {
            return invalid("intervals", "must be above 0");
        }
        if self.heartbeat_misses == 0 {
            return invalid("heartbeat_misses", "must be above 0");
        }
        Ok(())
    }

    /// Checks that the start position is on the ground of `ground` (M4 spec section 7.4).
    ///
    /// # Errors
    ///
    /// Returns [`SimConfigError::Invalid`] when the level has no height there, or the start is
    /// under the ground or too far above it.
    pub fn check_start(&self, ground: &impl Ground) -> Result<(), SimConfigError> {
        let start = self.start_position;
        let Some(height) = ground.height_at(start.x, start.y) else {
            return Err(SimConfigError::Invalid {
                key: "start_position",
                reason: "the level has no ground height there",
            });
        };
        let low = height - self.ground_tolerance_m;
        let high = height + self.max_height_above_ground_m;
        if !(low..=high).contains(&start.z) {
            return Err(SimConfigError::Invalid {
                key: "start_position",
                reason: "z is under the ground or too far above it",
            });
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const COMMITTED_CONFIG: &str = "../../config/sim.toml";
    const GROUND_HEIGHT: f32 = 168.0;

    struct Flat;

    impl Ground for Flat {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_HEIGHT)
        }
    }

    struct NoGround;

    impl Ground for NoGround {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            None
        }
    }

    /// The committed example configuration.
    pub(crate) fn committed() -> SimConfig {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED_CONFIG);
        load_sim_config(&path).expect("committed config")
    }

    #[test]
    fn the_committed_config_is_valid_and_starts_on_the_ground() {
        let config = committed();
        assert_eq!(config.level, "newworld_vitaeeterna");
        config.check_start(&Flat).expect("start on flat ground");
        assert!(config.check_start(&NoGround).is_err());
    }

    #[test]
    fn bad_values_are_refused() {
        let mut config = committed();
        config.world_database_password_env = "WORLD_PASSWORD".to_owned();
        assert!(config.validate().is_err());
        let mut config = committed();
        config.interest_radius_m = f32::NAN;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.start_position.z = f32::INFINITY;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.heartbeat_misses = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.start_position.z = GROUND_HEIGHT - config.ground_tolerance_m - 1.0;
        assert!(config.check_start(&Flat).is_err());
    }
}
