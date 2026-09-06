// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/nw-admin.toml` with `NW_ADMIN_` overrides (admin tools spec section 6).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use nw_common::config::{ConfigError, load_config};
use serde::Deserialize;

/// Prefix of the environment overrides.
pub const ADMIN_ENV_PREFIX: &str = "NW_ADMIN_";

/// The admin tools configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConfig {
    /// HTTP address; loopback only.
    pub listen: SocketAddr,
    /// `tracing` filter.
    pub log_filter: String,
    /// File with the pinned client build.
    pub client_build_config: PathBuf,
    /// Folder of the world files, the content bundle and `map/`.
    pub content_dir: PathBuf,
    /// Level of the map.
    pub level: String,
    /// Most spawn points one request returns.
    pub max_spawns_per_request: usize,
    /// The capture viewer.
    pub captures: CapturesConfig,
}

/// The capture viewer part of the configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturesConfig {
    /// Folder searched for `ledger.bin` files.
    pub captures_dir: PathBuf,
    /// Saved Discord threads of the open-world sessions.
    pub discord_dir: PathBuf,
    /// The local Azoth catalog archive.
    pub azoth_dir: PathBuf,
    /// The client type table, relative to the install folder.
    pub type_index: PathBuf,
    /// Aeternum-World class names.
    pub type_names: PathBuf,
}

/// A configuration that cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum AdminConfigError {
    /// The file or an override cannot be read.
    #[error(transparent)]
    Load(#[from] ConfigError),
    /// `listen` is not a loopback address: the tools have no login.
    #[error("listen address {0} is not a loopback address")]
    NotLoopback(SocketAddr),
    /// A value is out of range.
    #[error("config value `{key}` is not valid: {reason}")]
    Invalid {
        /// Key.
        key: &'static str,
        /// Why.
        reason: &'static str,
    },
}

/// Reads and checks `path` with `NW_ADMIN_` overrides.
///
/// # Errors
///
/// Returns [`AdminConfigError`] when the file cannot be read or a value is not allowed.
pub fn load_admin_config(path: &Path) -> Result<AdminConfig, AdminConfigError> {
    let config: AdminConfig = load_config(path, ADMIN_ENV_PREFIX)?;
    if !config.listen.ip().is_loopback() {
        return Err(AdminConfigError::NotLoopback(config.listen));
    }
    if config.max_spawns_per_request == 0 {
        return Err(AdminConfigError::Invalid {
            key: "max_spawns_per_request",
            reason: "must be at least 1",
        });
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn the_committed_config_loads() {
        let config = load_admin_config(&repo_root().join("config/nw-admin.toml")).expect("load");
        assert!(config.listen.ip().is_loopback());
        assert!(config.max_spawns_per_request > 0);
    }

    #[test]
    fn a_listen_address_off_loopback_is_refused() {
        let committed =
            std::fs::read_to_string(repo_root().join("config/nw-admin.toml")).expect("read");
        let changed = committed.replace("listen = \"127.0.0.1:", "listen = \"0.0.0.0:");
        assert_ne!(changed, committed);
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("nw-admin.toml");
        std::fs::write(&path, changed).expect("write");
        assert!(matches!(
            load_admin_config(&path),
            Err(AdminConfigError::NotLoopback(_))
        ));
    }
}
