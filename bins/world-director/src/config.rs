// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/world-director.toml` with `NW_DIRECTOR_` overrides (M5 spec section 6).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use nw_common::config::{ConfigError, SECRET_ENV_PREFIX, load_config};
use nw_db::{DatabaseConfig, GlobalDatabase};
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
    /// The global database, for the admin ranks and the account ages: an account with the game
    /// master rank gets the client GM flag, and a young account without a rank is in probation.
    /// Without it no account gets GM rights and none is in probation.
    #[serde(default)]
    pub global_database: Option<GlobalDatabase>,
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
    /// The instances (expeditions, raids, trials); without this section the world opens none.
    #[serde(default)]
    pub instances: Option<InstancesConfig>,
}

/// The `[instances]` section: how the director runs the instance sims. These are operator
/// settings (resource use), not game values.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstancesConfig {
    /// An instance with no player for this long ends and its sim is free again.
    pub empty_timeout_ms: u64,
    /// Longest wait for an instance sim to open an instance.
    pub open_timeout_ms: u64,
    /// Longest wait for the social service's answer about a group.
    pub group_timeout_ms: u64,
    /// How the director starts instance sims itself; without it the operator starts them
    /// (each with `role = "instance"` and the level of its expeditions).
    #[serde(default)]
    pub launch: Option<LaunchConfig>,
    /// The matchmaking queue of the arena.
    #[serde(default = "MatchConfig::arena")]
    pub arena: MatchConfig,
    /// The matchmaking queue of Outpost Rush.
    #[serde(default = "MatchConfig::outpost_rush")]
    pub outpost_rush: MatchConfig,
    /// The matchmaking queue of capture the flag.
    #[serde(default = "MatchConfig::capture_the_flag")]
    pub capture_the_flag: MatchConfig,
    /// The matchmaking queue of the catacombs.
    #[serde(default)]
    pub catacombs: CatacombsQueueConfig,
}

/// How long the players have to accept a catacombs run by default (Inferred: the `Catacombs`
/// row's `RaidInviteDurationSec` 90, as the arena's window is its row's 45).
pub const DEFAULT_CATACOMBS_ACCEPT_TIMEOUT_MS: u64 = 90_000;
/// How long a catacombs join with the fill byte waits for other players by default (Inferred
/// from the retail pops of a short team 62.2, 63.3 and 65.6 s after the queue entry; no row
/// value holds it).
pub const DEFAULT_CATACOMBS_FILL_WAIT_MS: u64 = 60_000;

/// The `[instances.catacombs]` section: the accept window and the fill wait of a catacombs run
/// (one team of the row's `TeamCapacity` 3).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatacombsQueueConfig {
    /// How long the players have to accept a found run.
    pub accept_timeout_ms: u64,
    /// How long a join with the fill byte waits for other players before its team forms.
    pub fill_wait_ms: u64,
}

impl Default for CatacombsQueueConfig {
    fn default() -> Self {
        Self {
            accept_timeout_ms: DEFAULT_CATACOMBS_ACCEPT_TIMEOUT_MS,
            fill_wait_ms: DEFAULT_CATACOMBS_FILL_WAIT_MS,
        }
    }
}

/// The players of an arena match by default: a full 3v3.
pub const DEFAULT_MATCH_SIZE: u32 = nw_content::instances::ARENA_MATCH_PLAYERS;
/// How long the players have to accept a found match by default (Observed: the solo pop of
/// retail `3v3` ended 46.7 s after it, the arena row's `RaidInviteDurationSec` is 45).
pub const DEFAULT_ACCEPT_TIMEOUT_MS: u64 = 45_000;
/// The players of an Outpost Rush match by default: two teams of the row's `TeamCapacity` 20.
pub const DEFAULT_OUTPOST_RUSH_MATCH_SIZE: u32 = nw_content::instances::OUTPOST_RUSH_MATCH_PLAYERS;
/// How long the players have to accept an Outpost Rush match by default (Inferred: the
/// `OutpostRush` row's `RaidInviteDurationSec` 90, as the arena's window is its row's 45).
pub const DEFAULT_OUTPOST_RUSH_ACCEPT_TIMEOUT_MS: u64 = 90_000;
/// The players of a capture the flag match by default: two teams of the row's `TeamCapacity` 10.
pub const DEFAULT_CAPTURE_THE_FLAG_MATCH_SIZE: u32 =
    nw_content::instances::CAPTURE_THE_FLAG_MATCH_PLAYERS;
/// How long the players have to accept a capture the flag match by default (Inferred: the
/// `CaptureTheFlag` row's `RaidInviteDurationSec` 90, as the arena's window is its row's 45).
pub const DEFAULT_CAPTURE_THE_FLAG_ACCEPT_TIMEOUT_MS: u64 = 90_000;
/// The teams of a match: the match size is a multiple of it.
const MATCH_TEAMS: u32 = 2;

/// The `[instances.arena]`, `[instances.outpost_rush]` and `[instances.capture_the_flag]`
/// sections: the size of a match (6 for a 3v3 arena, 2 makes the 1v1 matches retail made when
/// few players queued; 40 for Outpost Rush and 20 for capture the flag, a small even size for
/// tests) and the accept window.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchConfig {
    /// Players of a match: two teams of half of it.
    pub match_size: u32,
    /// How long the players have to accept a found match.
    pub accept_timeout_ms: u64,
}

impl MatchConfig {
    /// The arena's retail values.
    pub const fn arena() -> Self {
        Self {
            match_size: DEFAULT_MATCH_SIZE,
            accept_timeout_ms: DEFAULT_ACCEPT_TIMEOUT_MS,
        }
    }

    /// Outpost Rush's retail values.
    pub const fn outpost_rush() -> Self {
        Self {
            match_size: DEFAULT_OUTPOST_RUSH_MATCH_SIZE,
            accept_timeout_ms: DEFAULT_OUTPOST_RUSH_ACCEPT_TIMEOUT_MS,
        }
    }

    /// Capture the flag's retail values.
    pub const fn capture_the_flag() -> Self {
        Self {
            match_size: DEFAULT_CAPTURE_THE_FLAG_MATCH_SIZE,
            accept_timeout_ms: DEFAULT_CAPTURE_THE_FLAG_ACCEPT_TIMEOUT_MS,
        }
    }

    /// Whether the match is two teams of the same size with an accept window.
    const fn is_valid(&self) -> bool {
        self.match_size != 0
            && self.match_size.is_multiple_of(MATCH_TEAMS)
            && self.accept_timeout_ms != 0
    }
}

/// The `[instances.launch]` section: the command that starts one instance sim. The director
/// adds the sim's id, level and role as `NW_SIM_SIM_ID`, `NW_SIM_LEVEL` and `NW_SIM_ROLE`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchConfig {
    /// The `sim` program.
    pub program: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Free instance sims each level keeps ready while players ask for it.
    pub spare_per_level: u32,
    /// Most instance sims the director runs at once.
    pub max_sims: u32,
    /// Prefix of the ids of the sims it starts.
    pub id_prefix: String,
    /// A level nobody asked for in this long keeps no spare sim.
    pub forget_after_ms: u64,
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
        if self
            .global_database
            .as_ref()
            .is_some_and(|global| !global.password_env.starts_with(SECRET_ENV_PREFIX))
        {
            return invalid("global_database.password_env", "must start with NW_SECRET_");
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
        if let Some(instances) = &self.instances {
            if instances.empty_timeout_ms == 0
                || instances.open_timeout_ms == 0
                || instances.group_timeout_ms == 0
            {
                return invalid("instances", "timeouts must be above 0");
            }
            if instances.open_timeout_ms >= self.admit_timeout_ms {
                return invalid(
                    "instances.open_timeout_ms",
                    "must be below admit_timeout_ms",
                );
            }
            for (section, queue) in [
                ("instances.arena", &instances.arena),
                ("instances.outpost_rush", &instances.outpost_rush),
                ("instances.capture_the_flag", &instances.capture_the_flag),
            ] {
                if !queue.is_valid() {
                    return invalid(
                        section,
                        "match_size must be an even number above 0 and accept_timeout_ms above 0",
                    );
                }
            }
            if instances.catacombs.accept_timeout_ms == 0 || instances.catacombs.fill_wait_ms == 0 {
                return invalid(
                    "instances.catacombs",
                    "accept_timeout_ms and fill_wait_ms must be above 0",
                );
            }
            if let Some(launch) = &instances.launch
                && (launch.program.is_empty()
                    || launch.id_prefix.is_empty()
                    || launch.max_sims == 0
                    || launch.forget_after_ms == 0)
            {
                return invalid(
                    "instances.launch",
                    "needs a program, an id prefix, max_sims and forget_after_ms above 0",
                );
            }
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
        let mut config = committed();
        let global = config.global_database.as_mut().expect("global database");
        global.password_env = "PASSWORD".to_owned();
        assert!(config.validate().is_err());
        let mut config = committed();
        config.global_database = None;
        assert!(config.validate().is_ok(), "the ranks are optional");
    }

    #[test]
    fn the_instance_section_is_checked() {
        let config = committed();
        let instances = config.instances.clone().expect("instances");
        assert!(instances.open_timeout_ms < config.admit_timeout_ms);
        let mut bad = config.clone();
        if let Some(instances) = bad.instances.as_mut() {
            instances.empty_timeout_ms = 0;
        }
        assert!(bad.validate().is_err());
        let mut bad = config.clone();
        if let Some(instances) = bad.instances.as_mut() {
            instances.open_timeout_ms = config.admit_timeout_ms;
        }
        assert!(bad.validate().is_err());
        let mut bad = config.clone();
        if let Some(instances) = bad.instances.as_mut() {
            instances.launch = Some(LaunchConfig {
                program: String::new(),
                args: Vec::new(),
                spare_per_level: 1,
                max_sims: 1,
                id_prefix: "inst".to_owned(),
                forget_after_ms: 1,
            });
        }
        assert!(bad.validate().is_err());
        let mut bad = config.clone();
        if let Some(instances) = bad.instances.as_mut() {
            instances.arena.match_size = 3;
        }
        assert!(bad.validate().is_err(), "two teams of the same size");
        let mut one_vs_one = config.clone();
        if let Some(instances) = one_vs_one.instances.as_mut() {
            instances.arena.match_size = 2;
        }
        assert!(one_vs_one.validate().is_ok(), "a 1v1 as retail made");
        assert_eq!(MatchConfig::arena().match_size, 6);
        assert_eq!(MatchConfig::outpost_rush().match_size, 40);
        assert_eq!(MatchConfig::capture_the_flag().match_size, 20);
        assert_eq!(
            CatacombsQueueConfig::default(),
            CatacombsQueueConfig {
                accept_timeout_ms: DEFAULT_CATACOMBS_ACCEPT_TIMEOUT_MS,
                fill_wait_ms: DEFAULT_CATACOMBS_FILL_WAIT_MS,
            }
        );
        let mut no_fill_wait = one_vs_one.clone();
        if let Some(instances) = no_fill_wait.instances.as_mut() {
            instances.catacombs.fill_wait_ms = 0;
        }
        assert!(no_fill_wait.validate().is_err(), "a catacombs fill wait");
        let mut odd_flag = one_vs_one.clone();
        if let Some(instances) = odd_flag.instances.as_mut() {
            instances.capture_the_flag.match_size = 7;
        }
        assert!(
            odd_flag.validate().is_err(),
            "two capture the flag teams of the same size"
        );
        let mut odd_rush = one_vs_one.clone();
        if let Some(instances) = odd_rush.instances.as_mut() {
            instances.outpost_rush.match_size = 5;
        }
        assert!(
            odd_rush.validate().is_err(),
            "two Outpost Rush teams of the same size"
        );
        let mut none = config;
        none.instances = None;
        assert!(none.validate().is_ok(), "a world without instances");
    }
}
