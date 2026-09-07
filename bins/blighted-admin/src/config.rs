// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/blighted-admin.toml` with `BLIGHTED_ADMIN_` overrides (admin tools spec section 6).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use nw_common::config::{ConfigError, load_config};
use serde::Deserialize;
use uuid::Uuid;

/// Prefix of the environment overrides.
pub const ADMIN_ENV_PREFIX: &str = "BLIGHTED_ADMIN_";
/// Shortest scrape and state interval.
pub const MIN_LIVE_INTERVAL_MS: u64 = 100;
const INTERVAL_TOO_SHORT: &str = "is shorter than MIN_LIVE_INTERVAL_MS";
const NOT_LETTERS: &str = "must be one or more letters a to z or A to Z";

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
    /// The live panel; without it the live routes are 404.
    pub live: Option<LiveConfig>,
}

/// The live panel part of the configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveConfig {
    /// The bus.
    pub nats_url: String,
    /// The world, as in the world-director configuration.
    pub world_id: Uuid,
    /// The metrics pages of the servers.
    pub metrics_urls: Vec<String>,
    /// Time between scrapes (also the CPU sample time).
    pub scrape_interval_ms: u64,
    /// Time between two states sent to the page.
    pub state_interval_ms: u64,
    /// Program names shown in the CPU tile.
    pub processes: Vec<String>,
    /// The `nw-testclient` program of the bot batches.
    pub testclient_bin: PathBuf,
    /// Its configuration.
    pub testclient_config: PathBuf,
    /// `KEY=VALUE` file; only its `NW_SECRET_*` lines go to the batches.
    pub testclient_env_file: PathBuf,
    /// Account prefix of the batches; the batch letters follow it.
    pub bot_account_prefix: String,
    /// Character prefix of the batches (letters only); the batch letters follow it.
    pub bot_character_prefix: String,
    /// Most bots of all batches together.
    pub max_bots: u32,
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

/// Reads and checks `path` with `BLIGHTED_ADMIN_` overrides.
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
    if let Some(live) = &config.live {
        for (key, interval) in [
            ("live.scrape_interval_ms", live.scrape_interval_ms),
            ("live.state_interval_ms", live.state_interval_ms),
        ] {
            if interval < MIN_LIVE_INTERVAL_MS {
                return Err(AdminConfigError::Invalid {
                    key,
                    reason: INTERVAL_TOO_SHORT,
                });
            }
        }
        let prefix = &live.bot_character_prefix;
        if prefix.is_empty() || !prefix.chars().all(|letter| letter.is_ascii_alphabetic()) {
            return Err(AdminConfigError::Invalid {
                key: "live.bot_character_prefix",
                reason: NOT_LETTERS,
            });
        }
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
        let config =
            load_admin_config(&repo_root().join("config/blighted-admin.toml")).expect("load");
        assert!(config.listen.ip().is_loopback());
        assert!(config.max_spawns_per_request > 0);
    }

    fn load_changed(from: &str, to: &str) -> Result<AdminConfig, AdminConfigError> {
        let committed =
            std::fs::read_to_string(repo_root().join("config/blighted-admin.toml")).expect("read");
        let changed = committed.replace(from, to);
        assert_ne!(changed, committed);
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("blighted-admin.toml");
        std::fs::write(&path, changed).expect("write");
        load_admin_config(&path)
    }

    #[test]
    fn the_committed_config_has_a_live_section() {
        let config =
            load_admin_config(&repo_root().join("config/blighted-admin.toml")).expect("load");
        let live = config.live.expect("live section");
        assert!(!live.metrics_urls.is_empty());
        assert!(live.scrape_interval_ms >= MIN_LIVE_INTERVAL_MS);
        assert!(live.state_interval_ms >= MIN_LIVE_INTERVAL_MS);
        assert!(live.max_bots > 0);
    }

    #[test]
    fn a_live_interval_below_the_minimum_is_refused() {
        for (from, to, key) in [
            (
                "scrape_interval_ms = 1000",
                "scrape_interval_ms = 10",
                "live.scrape_interval_ms",
            ),
            (
                "state_interval_ms = 250",
                "state_interval_ms = 0",
                "live.state_interval_ms",
            ),
        ] {
            assert!(
                matches!(load_changed(from, to), Err(AdminConfigError::Invalid { key: found, .. }) if found == key),
                "{key}"
            );
        }
    }

    #[test]
    fn a_character_prefix_with_digits_is_refused() {
        assert!(matches!(
            load_changed(
                "bot_character_prefix = \"Adminbot\"",
                "bot_character_prefix = \"Admin1\""
            ),
            Err(AdminConfigError::Invalid {
                key: "live.bot_character_prefix",
                ..
            })
        ));
    }

    #[test]
    fn a_listen_address_off_loopback_is_refused() {
        assert!(matches!(
            load_changed("listen = \"127.0.0.1:", "listen = \"0.0.0.0:"),
            Err(AdminConfigError::NotLoopback(_))
        ));
    }
}
