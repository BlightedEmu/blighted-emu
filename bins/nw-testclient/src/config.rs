// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/nw-testclient.toml` with `NW_TESTCLIENT_` overrides (M3 spec section 8, M4 spec
//! section 9).

use std::path::{Path, PathBuf};

use nw_common::config::{ConfigError, load_config};
use serde::Deserialize;

use crate::route::{Route, RouteError};

/// Environment prefix of overrides.
pub const TESTCLIENT_ENV_PREFIX: &str = "NW_TESTCLIENT_";
/// Environment variable with the account password.
pub const PASSWORD_ENV: &str = "NW_SECRET_TESTCLIENT_PASSWORD";
/// Environment variable with the password of the second account (`pair`).
pub const SECOND_PASSWORD_ENV: &str = "NW_SECRET_TESTCLIENT_PASSWORD_B";

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
    /// Largest distance of a seen position from the route, in metres (`pair`).
    pub path_tolerance_m: f32,
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
    }
}
