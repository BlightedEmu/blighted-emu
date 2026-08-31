// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads the pinned client build from `config/client-build.toml` (master spec D6).

use std::path::Path;

use nw_common::config::{ConfigError, load_config};
use serde::Deserialize;

/// Environment variable prefix for overrides of the client build file.
pub const CLIENT_BUILD_ENV_PREFIX: &str = "NW_CLIENT_";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientBuildConfig {
    steam_build_id: String,
}

/// Reads the pinned Steam build id.
pub fn load_client_build(path: &Path) -> Result<String, ConfigError> {
    load_config::<ClientBuildConfig>(path, CLIENT_BUILD_ENV_PREFIX)
        .map(|config| config.steam_build_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_steam_build_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client-build.toml");
        std::fs::write(&path, "steam_build_id = \"12345\"\n").unwrap();
        assert_eq!(load_client_build(&path).unwrap(), "12345");
    }

    #[test]
    fn unknown_key_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client-build.toml");
        std::fs::write(&path, "steam_build_id = \"1\"\nbuild = \"2\"\n").unwrap();
        assert!(load_client_build(&path).is_err());
    }
}
