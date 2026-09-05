// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/sim.toml` with `NW_SIM_` overrides (M4 spec sections 6.3 and 7, M5 spec section 7.1).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use nw_bus::{BusId, MAX_BUS_MESSAGE_BYTES, MAX_GHOST_BYTES};
use nw_common::config::{ConfigError, SECRET_ENV_PREFIX, load_config};
use nw_db::DatabaseConfig;
use nw_sim_core::{CellBounds, MoveLimits, MoveLimitsError};
use serde::Deserialize;
use uuid::Uuid;

use crate::world::WorldSettings;

/// Environment prefix of `sim` overrides.
pub const SIM_ENV_PREFIX: &str = "NW_SIM_";
/// Room in one ghost message for everything but the ghosts (the sender id and the list length).
pub const GHOSTS_HEADER_BYTES: usize = 128;

/// The whole `sim` configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimConfig {
    /// This sim's id in bus subjects; unique in the world.
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
    /// The open map level; the same as the director's.
    pub level: String,
    /// Most players in this sim.
    pub max_players: u32,
    /// Players see each other inside this radius, in metres.
    pub interest_radius_m: f32,
    /// Most other players and ghosts in a player's bundles: the nearest.
    pub max_visible_players: usize,
    /// How far a position may be below the ground, in metres.
    pub ground_tolerance_m: f32,
    /// How far a position may be above the ground, in metres.
    pub max_height_above_ground_m: f32,
    /// The run speed of the datasheets (`ServerValidatedCanRun`), in metres per second.
    pub run_speed_mps: f32,
    /// How much faster than `run_speed_mps` a move may be, as a share (1.2 is 120 % more).
    pub speed_margin: f32,
    /// Longest burst of movement that the distance budget holds, in seconds.
    pub burst_seconds: f32,
    /// How far into another sim's cells a player goes before its handoff starts, in metres.
    pub handoff_margin_m: f32,
    /// Longest wait for the answer to a handoff.
    pub handoff_timeout_ms: u64,
    /// Wait after a refused or late handoff, and after a take, before the next handoff.
    pub handoff_retry_ms: u64,
    /// How long inputs of a handed-off connection go on to the new sim.
    pub forward_ttl_ms: u64,
    /// Ghosts of another sim not renewed for this long are dropped.
    pub ghost_ttl_ms: u64,
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
        let ghosts_fit = (self.max_players as usize)
            .checked_mul(MAX_GHOST_BYTES)
            .is_some_and(|bytes| bytes + GHOSTS_HEADER_BYTES <= MAX_BUS_MESSAGE_BYTES);
        if self.max_players == 0 || !ghosts_fit {
            return invalid(
                "max_players",
                "must be above 0, and its ghosts must fit one bus message",
            );
        }
        if !(self.handoff_margin_m.is_finite() && self.handoff_margin_m >= 0.0) {
            return invalid("handoff_margin_m", "must be a finite number of at least 0");
        }
        let times = [
            self.handoff_timeout_ms,
            self.handoff_retry_ms,
            self.forward_ttl_ms,
            self.ghost_ttl_ms,
        ];
        if times.contains(&0) {
            return invalid("handoff", "times must be above 0");
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
        self.move_limits()
            .map_err(|error| SimConfigError::Invalid {
                key: error.name,
                reason: error.rule,
            })?;
        Ok(())
    }

    /// The settings of the world state on a level with the edges `bounds`.
    ///
    /// # Errors
    ///
    /// Returns [`SimConfigError::Invalid`] for a movement limit that is not usable.
    pub fn world_settings(&self, bounds: CellBounds) -> Result<WorldSettings, SimConfigError> {
        let limits = self
            .move_limits()
            .map_err(|error| SimConfigError::Invalid {
                key: error.name,
                reason: error.rule,
            })?;
        Ok(WorldSettings {
            interest_radius_m: self.interest_radius_m,
            max_visible_players: self.max_visible_players,
            max_players: self.max_players,
            limits,
            bounds,
            handoff_margin_m: self.handoff_margin_m,
            handoff_retry_ms: self.handoff_retry_ms,
            ghost_ttl_ms: self.ghost_ttl_ms,
        })
    }

    /// The movement limits (M4 spec section 7.2 with sections 15 and 16).
    ///
    /// # Errors
    ///
    /// Returns [`MoveLimitsError`] for a value that is not usable.
    pub fn move_limits(&self) -> Result<MoveLimits, MoveLimitsError> {
        MoveLimits::new(
            self.run_speed_mps,
            self.speed_margin,
            self.burst_seconds,
            self.ground_tolerance_m,
            self.max_height_above_ground_m,
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const COMMITTED_CONFIG: &str = "../../config/sim.toml";
    /// Sprint (5.5 m/s) with the 15 % road boost (movement numbers note, Check section).
    const BOOSTED_SPRINT_MPS: f32 = 6.325;
    /// The longest gap between two captured movement messages (movement numbers note, section 3).
    const LONGEST_HONEST_GAP_S: f32 = 2.5;
    /// The largest captured fall (movement numbers note, Check section).
    const LARGEST_CAPTURED_FALL_M: f32 = 128.6;
    /// A bundle holds the player and 47 others (M5 spec section 7.3).
    const ONE_BUNDLE_OF_OTHERS: usize = 47;

    /// The committed example configuration.
    pub(crate) fn committed() -> SimConfig {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED_CONFIG);
        load_sim_config(&path).expect("committed config")
    }

    #[test]
    fn the_committed_config_is_valid() {
        let config = committed();
        assert_eq!(config.level, "newworld_vitaeeterna");
        assert_eq!(config.max_visible_players, ONE_BUNDLE_OF_OTHERS);
        assert!(config.max_height_above_ground_m > LARGEST_CAPTURED_FALL_M);
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
        config.heartbeat_misses = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.speed_margin = -1.0;
        assert!(matches!(
            config.validate(),
            Err(SimConfigError::Invalid {
                key: "speed_margin",
                ..
            })
        ));
        let mut config = committed();
        config.max_players = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.max_players = u32::MAX;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.handoff_margin_m = -1.0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.handoff_timeout_ms = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn the_speed_limit_is_above_boosted_sprint() {
        let limits = committed().move_limits().expect("limits");
        assert!(limits.max_speed_mps > BOOSTED_SPRINT_MPS);
        assert!(committed().burst_seconds > LONGEST_HONEST_GAP_S);
    }
}
