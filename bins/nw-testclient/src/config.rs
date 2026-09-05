// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/nw-testclient.toml` with `NW_TESTCLIENT_` overrides (M3 spec section 8, M4 spec
//! section 9, M5 spec section 11).

use std::path::{Path, PathBuf};
use std::time::Duration;

use nw_common::config::{ConfigError, load_config};
use serde::Deserialize;

use crate::route::{Route, RouteError};
use crate::swarm::{MAX_BOTS, SwarmSettings};

/// Environment prefix of overrides.
pub const TESTCLIENT_ENV_PREFIX: &str = "NW_TESTCLIENT_";
/// Environment variable with the account password.
pub const PASSWORD_ENV: &str = "NW_SECRET_TESTCLIENT_PASSWORD";
/// Environment variable with the password of the second account (`pair`).
pub const SECOND_PASSWORD_ENV: &str = "NW_SECRET_TESTCLIENT_PASSWORD_B";
/// Environment variable with the password of every bot account (`swarm`).
pub const SWARM_PASSWORD_ENV: &str = "NW_SECRET_TESTCLIENT_SWARM_PASSWORD";

/// The whole test client configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestClientConfig {
    /// `https://login.<domain>`.
    pub login_url: String,
    /// `https://game.<domain>`.
    pub game_url: String,
    /// Account name; registered when it is new.
    pub account: String,
    /// Character created when the account has none.
    pub character_name: String,
    /// The second account of `pair`.
    pub second_account: String,
    /// The character of the second account.
    pub second_character_name: String,
    /// The gateway's certificate; the client trusts only this file.
    pub certificate_file: PathBuf,
    /// DTLS MTU.
    pub mtu: u32,
    /// Seconds to stay connected after registration.
    pub hold_seconds: u64,
    /// Seconds to wait for the registration response.
    pub registration_timeout_seconds: u64,
    /// Largest distance between the saved position and the next login, in metres (`pair`).
    pub position_tolerance_m: f32,
    /// The walk of `pair`: points east and north of where the first client stands, in metres.
    pub route_m: Vec<[f32; 2]>,
    /// Walking speed of `pair`, in metres per second.
    pub route_speed_mps: f32,
    /// The far move of `pair` after the walk, east, in metres; the world must refuse it.
    pub jump_m: f32,
    /// Largest distance of a seen position from the route, in metres (`pair` and `swarm`).
    pub path_tolerance_m: f32,
    /// `[swarm]`: the bots of the M5 exit test.
    pub swarm: SwarmConfig,
}

/// `[swarm]` (M5 spec section 11).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmConfig {
    /// Bots to run.
    pub bots: usize,
    /// Account names are this and five digits.
    pub account_prefix: String,
    /// Character names are this and three letters; letters only.
    pub character_prefix: String,
    /// New bots per second.
    pub bots_per_second: u32,
    /// Lanes are spread this far north and south of the start, in metres.
    pub lane_spread_m: f32,
    /// Bots walk this far east and west of the start line, in metres.
    pub swing_m: f32,
    /// Walking speed, in metres per second.
    pub speed_mps: f32,
    /// Seconds in the world per bot.
    pub duration_seconds: u64,
    /// Fewest crossings of the start line per bot.
    pub min_crossings: u32,
    /// Longest gap between two own bundles, in milliseconds.
    pub max_bundle_gap_ms: u64,
    /// The metrics URL of every sim.
    pub metrics_urls: Vec<String>,
    /// The pinned client build file.
    pub client_build_config: PathBuf,
    /// Folder of the world files; the bots walk on the ground of the level.
    pub content_dir: PathBuf,
    /// The open map level.
    pub level: String,
}

/// A `[swarm]` value that is not allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("swarm config value `{key}` is not valid: {reason}")]
pub struct SwarmConfigError {
    /// Key.
    pub key: &'static str,
    /// Why.
    pub reason: &'static str,
}

impl TestClientConfig {
    /// The walk of `pair`.
    ///
    /// # Errors
    ///
    /// Returns [`RouteError`] when the points or the speed cannot be walked.
    pub fn route(&self) -> Result<Route, RouteError> {
        Route::new(self.route_m.clone(), self.route_speed_mps)
    }

    /// The swarm settings with the bot password `password`.
    ///
    /// # Errors
    ///
    /// Returns [`SwarmConfigError`] for a `[swarm]` value that is not allowed.
    pub fn swarm_settings(&self, password: String) -> Result<SwarmSettings, SwarmConfigError> {
        let swarm = &self.swarm;
        let invalid = |key, reason| Err(SwarmConfigError { key, reason });
        if !(1..=MAX_BOTS).contains(&swarm.bots) {
            return invalid("bots", "must be 1 to 17576");
        }
        if !swarm
            .character_prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic())
        {
            return invalid("character_prefix", "must be letters only");
        }
        let positive = |value: f32| value.is_finite() && value > 0.0;
        if !(positive(swarm.swing_m) && positive(swarm.speed_mps)) {
            return invalid("swing_m", "swing and speed must be finite numbers above 0");
        }
        if !(swarm.lane_spread_m.is_finite() && swarm.lane_spread_m >= 0.0) {
            return invalid("lane_spread_m", "must be a finite number of at least 0");
        }
        if swarm.bots_per_second == 0 || swarm.duration_seconds == 0 {
            return invalid(
                "bots_per_second",
                "the rate and the duration must be above 0",
            );
        }
        Ok(SwarmSettings {
            bots: swarm.bots,
            account_prefix: swarm.account_prefix.clone(),
            character_prefix: swarm.character_prefix.clone(),
            password,
            login_url: self.login_url.clone(),
            game_url: self.game_url.clone(),
            bots_per_second: swarm.bots_per_second,
            lane_spread_m: swarm.lane_spread_m,
            swing_m: swarm.swing_m,
            speed_mps: swarm.speed_mps,
            duration: Duration::from_secs(swarm.duration_seconds),
            registration_timeout: Duration::from_secs(self.registration_timeout_seconds),
            min_crossings: swarm.min_crossings,
            max_bundle_gap: Duration::from_millis(swarm.max_bundle_gap_ms),
            path_tolerance_m: self.path_tolerance_m,
            metrics_urls: swarm.metrics_urls.clone(),
        })
    }
}

/// Reads the configuration.
///
/// # Errors
///
/// Returns [`ConfigError`] when the file is missing or has a wrong key or type.
pub fn load_testclient_config(path: &Path) -> Result<TestClientConfig, ConfigError> {
    load_config(path, TESTCLIENT_ENV_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMITTED_CONFIG: &str = "../../config/nw-testclient.toml";
    const HOLD_SECONDS: u64 = 60;
    /// M4 spec section 1.2 criterion 3.5.
    const SPEC_TOLERANCE_M: f32 = 0.1;
    /// M4 spec section 1.2 criterion 3.3 (`path_tolerance_m`, default 1.0).
    const SPEC_PATH_TOLERANCE_M: f32 = 1.0;
    /// M5 spec section 1.2 criterion 2.2.
    const M5_BOTS: usize = 1000;
    const M5_BUNDLE_GAP_MS: u64 = 250;

    #[test]
    fn committed_example_config_loads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED_CONFIG);
        let config = load_testclient_config(&path).expect("config");
        assert_eq!(config.hold_seconds, HOLD_SECONDS);
        assert_ne!(config.second_account, config.account);
        assert!((config.position_tolerance_m - SPEC_TOLERANCE_M).abs() < f32::EPSILON);
        assert!((config.path_tolerance_m - SPEC_PATH_TOLERANCE_M).abs() < f32::EPSILON);
        let route = config.route().expect("route");
        assert!(route.length() > 0.0);
        assert!(config.jump_m > 0.0);
        let swarm = config.swarm_settings(String::new()).expect("swarm");
        assert_eq!(swarm.bots, M5_BOTS);
        assert_eq!(
            swarm.max_bundle_gap,
            Duration::from_millis(M5_BUNDLE_GAP_MS)
        );
    }

    #[test]
    fn bad_swarm_values_are_refused() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED_CONFIG);
        let config = load_testclient_config(&path).expect("config");
        let refused = |change: fn(&mut SwarmConfig)| {
            let mut bad = config.clone();
            change(&mut bad.swarm);
            bad.swarm_settings(String::new()).is_err()
        };
        assert!(refused(|swarm| swarm.bots = 0));
        assert!(refused(|swarm| swarm.bots = MAX_BOTS + 1));
        assert!(refused(|swarm| swarm.character_prefix = "Bot1".to_owned()));
        assert!(refused(|swarm| swarm.swing_m = 0.0));
        assert!(refused(|swarm| swarm.speed_mps = f32::NAN));
        assert!(refused(|swarm| swarm.lane_spread_m = -1.0));
        assert!(refused(|swarm| swarm.bots_per_second = 0));
    }
}
