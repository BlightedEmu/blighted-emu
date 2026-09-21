// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/edge.toml` (M2 spec section 9). Secrets are not in this file; see [`crate::secrets`].

use std::collections::HashSet;
use std::net::{SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};

use nw_common::config::{ConfigError, load_config};
pub use nw_db::DatabaseConfig;
use nw_protocol::aws_mirror::{REMOTE_CONFIG_BUCKETS, is_aws_mirror_host, remote_config_host};
use nw_protocol::launcher_api::{AuthMode, SESSION_PLACEHOLDER, is_forbidden_host};
use serde::Deserialize;
use uuid::Uuid;

use crate::secrets::SECRET_ENV_PREFIX;

/// Environment variable prefix for overrides of `config/edge.toml`.
pub const EDGE_ENV_PREFIX: &str = "NW_EDGE_";

/// Host name label of the launcher API and the channel config: `login.<domain>`.
pub const LOGIN_HOST_LABEL: &str = "login";
/// Host name label of the game routes: `game.<domain>`.
pub const GAME_HOST_LABEL: &str = "game";

/// Window of `queue_requests_per_minute`, in seconds.
pub const QUEUE_LIMIT_WINDOW_SECONDS: u64 = 60;
/// Queue requests of one client besides its refreshes in one window: the join.
const QUEUE_JOIN_REQUESTS: u64 = 1;

/// Launch argument that sets the channel base URL (endpoint RE note).
pub const CHANNEL_SERVICE_ARGUMENT: &str = "-ChannelService";
/// Launch argument that sets the channel key (endpoint RE note).
pub const LOGIN_CHANNEL_OVERRIDE_ARGUMENT: &str = "-LoginChannelOverride";
/// Launch argument that selects the client's auth backend (unknowns note, fifth pass).
pub const AUTH_BACKEND_ARGUMENT: &str = "-AuthBackend";
/// Auth backend `nw` (index 0): the only one that sends the Steam ticket to our `/steam` route.
/// The client compares the name with exact case; its built-in default is `omni`, which skips
/// `/steam` (unknowns note, fifth pass).
pub const AUTH_BACKEND_NW: &str = "nw";
/// Launch argument that sets `client-connection.authorization.cognito-pool-id`
/// (`FUN_146467680`). The client names its two `CognitoId` remote-config layers after it and
/// skips them when it is empty (character-select note); it also sends it in `ListWorlds`.
pub const COGNITO_POOL_ARGUMENT: &str = "-CognitoPool";

/// The whole `edge-http` configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeConfig {
    /// Name shown in the launcher.
    pub server_name: String,
    /// The community's own domain; `login.<domain>` is the launcher API host and `game.<domain>`
    /// the game route host.
    pub domain: String,
    /// Public HTTPS listener.
    pub https_listen: SocketAddr,
    /// Internal Prometheus listener (review finding I1).
    pub metrics_listen: SocketAddr,
    /// `Token.RepAddress`: `IPv4:port` of a `rep-gateway`.
    pub rep_address: String,
    /// Time limit for one HTTP request.
    pub request_timeout_seconds: u64,
    /// Most requests in progress at one time (spec amendment 6). More requests wait for a free
    /// slot, within `request_timeout_seconds`.
    pub max_connections: usize,
    /// `tracing` filter, for example `info`.
    pub log_filter: String,
    /// Certificate files.
    pub tls: TlsConfig,
    /// Account and session settings.
    pub auth: AuthConfig,
    /// Discord app; used when `auth.mode = "discord"`.
    #[serde(default)]
    pub discord: DiscordConfig,
    /// Global database.
    pub global_database: DatabaseConfig,
    /// Valkey.
    pub valkey: ValkeyConfig,
    /// Our own remote config sets.
    pub remote_config: RemoteConfigConfig,
    /// The Amazon names this server answers for its game clients.
    pub aws_mirror: AwsMirrorConfig,
    /// Worlds, one `[[world]]` block each.
    #[serde(rename = "world")]
    pub worlds: Vec<WorldConfig>,
}

/// `[tls]`: PEM files from an ACME tool such as `certbot` or `lego`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    /// Certificate chain.
    pub certificate_pem_path: PathBuf,
    /// Private key.
    pub private_key_pem_path: PathBuf,
}

/// `[auth]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    /// Password or Discord mode.
    pub mode: AuthMode,
    /// Whether password sign-up is open.
    pub allow_registration: bool,
    /// Parallel Argon2id hashes; 0 = the number of CPUs.
    pub argon2_parallel: usize,
    /// Failed logins allowed per IP and per username in one window.
    pub login_fail_limit: u32,
    /// Length of the failed-login window.
    pub login_fail_window_seconds: u64,
    /// Lifetime of a game session after each use.
    pub game_session_ttl_seconds: u64,
}

/// `[discord]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordConfig {
    /// Discord application (client) id.
    pub client_id: String,
}

/// `[valkey]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValkeyConfig {
    /// For example `redis://127.0.0.1:6379`.
    pub url: String,
}

/// `[remote_config]`: our own remote config sets (study note
/// `docs/re/2026-09-18-remote-config-layers.md`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteConfigConfig {
    /// Folder of the config sets. The path of a file under it is its S3 key, for example
    /// `applications/public/configuration-sets/ProductId/STEAM_APP_ID.1063730/versionless`.
    pub directory: PathBuf,
    /// The bucket the client asks for; one of [`REMOTE_CONFIG_BUCKETS`].
    pub bucket: String,
    /// Our Cognito pool id: the client's [`COGNITO_POOL_ARGUMENT`] and the id of the
    /// `CognitoId` layers. It names no Amazon pool.
    pub cognito_pool_id: Uuid,
}

/// `[aws_mirror]`: the Amazon names our launcher maps to this server for the game process, and
/// the self-signed certificate this server shows for them (`edge-http aws-mirror-certificate`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwsMirrorConfig {
    /// Host names in lower case; each passes [`is_aws_mirror_host`], and the bucket host is one
    /// of them.
    pub hosts: Vec<String>,
    /// Certificate for exactly these names.
    pub certificate_pem_path: PathBuf,
    /// Its private key.
    pub private_key_pem_path: PathBuf,
}

/// One `[[world]]` block.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldConfig {
    /// World id.
    pub id: Uuid,
    /// Name shown in the world list.
    pub name: String,
    /// Region text shown in the world list.
    pub region: String,
    /// The world database.
    pub database: DatabaseConfig,
    /// Environment variable that holds the world database password.
    pub database_password_env: String,
    /// Live entry tickets allowed at one time; `world-director` also limits by free player slots.
    pub capacity: u32,
    /// Characters one account may have in this world.
    pub max_characters_per_account: u32,
    /// Seconds between queue refreshes of the client.
    pub refresh_interval_seconds: u64,
    /// Queue joins and refreshes allowed per account per minute.
    pub queue_requests_per_minute: u32,
}

/// Error from [`load_edge_config`].
#[derive(Debug, thiserror::Error)]
pub enum EdgeConfigError {
    /// The file cannot be read or has a wrong key or type.
    #[error(transparent)]
    Load(#[from] ConfigError),
    /// A value is not allowed.
    #[error("config value `{key}` is not valid: {reason}")]
    Invalid {
        /// Key path, for example `world.capacity`.
        key: &'static str,
        /// Why the value is refused.
        reason: String,
    },
}

/// Reads and checks `config/edge.toml` (with `NW_EDGE_` overrides).
pub fn load_edge_config(path: &Path) -> Result<EdgeConfig, EdgeConfigError> {
    let config: EdgeConfig = load_config(path, EDGE_ENV_PREFIX)?;
    config.validate()?;
    Ok(config)
}

impl EdgeConfig {
    /// `login.<domain>`.
    pub fn login_host(&self) -> String {
        format!("{LOGIN_HOST_LABEL}.{}", self.domain)
    }

    /// `game.<domain>`.
    pub fn game_host(&self) -> String {
        format!("{GAME_HOST_LABEL}.{}", self.domain)
    }

    /// The launch arguments the launcher passes to the game (M2 spec section 6).
    pub fn launch_arguments(&self) -> Vec<String> {
        vec![
            CHANNEL_SERVICE_ARGUMENT.to_owned(),
            format!("https://{}", self.login_host()),
            LOGIN_CHANNEL_OVERRIDE_ARGUMENT.to_owned(),
            SESSION_PLACEHOLDER.to_owned(),
            AUTH_BACKEND_ARGUMENT.to_owned(),
            AUTH_BACKEND_NW.to_owned(),
            COGNITO_POOL_ARGUMENT.to_owned(),
            self.remote_config.cognito_pool_id.to_string(),
        ]
    }

    /// `rep_address` as a socket address.
    pub fn rep_socket_address(&self) -> Result<SocketAddrV4, EdgeConfigError> {
        let address: SocketAddrV4 =
            self.rep_address
                .parse()
                .map_err(|_| EdgeConfigError::Invalid {
                    key: "rep_address",
                    reason: "must be IPv4:port".to_owned(),
                })?;
        if address.port() == 0 {
            return Err(EdgeConfigError::Invalid {
                key: "rep_address",
                reason: "port must not be 0".to_owned(),
            });
        }
        Ok(address)
    }

    /// The world with this id.
    pub fn world(&self, id: Uuid) -> Option<&WorldConfig> {
        self.worlds.iter().find(|world| world.id == id)
    }

    fn validate_aws_mirror(&self) -> Result<(), EdgeConfigError> {
        let invalid = |reason: &str| {
            Err(EdgeConfigError::Invalid {
                key: "aws_mirror.hosts",
                reason: reason.to_owned(),
            })
        };
        let mut hosts = HashSet::new();
        for host in &self.aws_mirror.hosts {
            if host.to_ascii_lowercase() != *host || !is_aws_mirror_host(host) {
                return invalid(&format!(
                    "{host} is not a lower-case remote-config, Kinesis or STS host"
                ));
            }
            if !hosts.insert(host.as_str()) {
                return invalid(&format!("{host} is listed twice"));
            }
        }
        let bucket_host = remote_config_host(&self.remote_config.bucket);
        if !hosts.contains(bucket_host.as_str()) {
            return invalid(&format!("must contain the bucket host {bucket_host}"));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), EdgeConfigError> {
        let invalid = |key: &'static str, reason: &str| {
            Err(EdgeConfigError::Invalid {
                key,
                reason: reason.to_owned(),
            })
        };
        if self.domain.is_empty() || self.domain.contains('/') || self.domain.contains(':') {
            return invalid("domain", "must be a plain domain name");
        }
        if is_forbidden_host(&self.domain) {
            return invalid("domain", "must not be an Amazon or CloudFront domain");
        }
        self.rep_socket_address()?;
        if self.request_timeout_seconds == 0 {
            return invalid("request_timeout_seconds", "must be above 0");
        }
        if self.max_connections == 0 {
            return invalid("max_connections", "must be above 0");
        }
        if self.auth.login_fail_limit == 0 || self.auth.login_fail_window_seconds == 0 {
            return invalid("auth.login_fail_limit", "limit and window must be above 0");
        }
        if self.auth.game_session_ttl_seconds == 0 {
            return invalid("auth.game_session_ttl_seconds", "must be above 0");
        }
        if self.auth.mode == AuthMode::Discord && self.discord.client_id.is_empty() {
            return invalid("discord.client_id", "is required in Discord mode");
        }
        if !REMOTE_CONFIG_BUCKETS.contains(&self.remote_config.bucket.as_str()) {
            return invalid(
                "remote_config.bucket",
                "must be a bucket the client asks for",
            );
        }
        self.validate_aws_mirror()?;
        if self.worlds.is_empty() {
            return invalid("world", "at least one [[world]] is required");
        }
        let mut ids = HashSet::new();
        for world in &self.worlds {
            if !ids.insert(world.id) {
                return invalid("world.id", "world ids must be unique");
            }
            if world.capacity == 0 || world.refresh_interval_seconds == 0 {
                return invalid(
                    "world.capacity",
                    "capacity and refresh interval must be above 0",
                );
            }
            if world.max_characters_per_account == 0 {
                return invalid("world.max_characters_per_account", "must be above 0");
            }
            // A normal client refreshes once per interval and joins once (review minor m4).
            let needed = QUEUE_LIMIT_WINDOW_SECONDS.div_ceil(world.refresh_interval_seconds)
                + QUEUE_JOIN_REQUESTS;
            if u64::from(world.queue_requests_per_minute) < needed {
                return invalid(
                    "world.queue_requests_per_minute",
                    &format!(
                        "must be at least {needed} for refresh_interval_seconds = {}",
                        world.refresh_interval_seconds
                    ),
                );
            }
            if !world.database_password_env.starts_with(SECRET_ENV_PREFIX) {
                return invalid(
                    "world.database_password_env",
                    &format!("must start with {SECRET_ENV_PREFIX}"),
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A valid configuration in TOML.
    pub(crate) const VALID: &str = r#"
server_name = "Test server"
domain = "example.org"
https_listen = "0.0.0.0:443"
metrics_listen = "127.0.0.1:9100"
rep_address = "127.0.0.1:33435"
request_timeout_seconds = 10
max_connections = 4096
log_filter = "info"

[tls]
certificate_pem_path = "local-data/edge-http/fullchain.pem"
private_key_pem_path = "local-data/edge-http/privkey.pem"

[auth]
mode = "password"
allow_registration = true
argon2_parallel = 0
login_fail_limit = 10
login_fail_window_seconds = 600
game_session_ttl_seconds = 7200

[global_database]
url = "postgres://nw@127.0.0.1:5432/nw_global"
max_connections = 5

[valkey]
url = "redis://127.0.0.1:6379"

[remote_config]
directory = "config/remote-config"
bucket = "ags-javelin-remote-config"
cognito_pool_id = "b04fbfcd-942c-472d-88c0-c762c4076b2f"

[aws_mirror]
hosts = [
    "ags-javelin-remote-config.s3.amazonaws.com",
    "kinesis.us-east-1.amazonaws.com",
    "sts.us-east-1.amazonaws.com",
]
certificate_pem_path = "local-data/edge-http/aws-mirror.pem"
private_key_pem_path = "local-data/edge-http/aws-mirror-key.pem"

[[world]]
id = "8d0f4c2e-0000-4000-8000-000000000001"
name = "Aeternum"
region = "us-east"
database_password_env = "NW_SECRET_WORLD_1_DATABASE_PASSWORD"
capacity = 100
max_characters_per_account = 2
refresh_interval_seconds = 5
queue_requests_per_minute = 30

[world.database]
url = "postgres://nw@127.0.0.1:5432/nw_world_1"
max_connections = 5
"#;

    fn load(text: &str) -> Result<EdgeConfig, EdgeConfigError> {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("edge.toml");
        std::fs::write(&path, text).expect("write");
        load_edge_config(&path)
    }

    /// The valid configuration as a value.
    pub(crate) fn valid() -> EdgeConfig {
        load(VALID).expect("valid config")
    }

    fn refused(text: &str, key: &str) {
        match load(text) {
            Err(EdgeConfigError::Invalid { key: found, .. }) => assert_eq!(found, key),
            other => panic!("expected {key} to be refused, got {other:?}"),
        }
    }

    #[test]
    fn valid_file_loads() {
        let config = valid();
        assert_eq!(config.login_host(), "login.example.org");
        assert_eq!(config.game_host(), "game.example.org");
        assert_eq!(config.worlds.len(), 1);
        assert_eq!(config.auth.mode, AuthMode::Password);
        assert_eq!(
            config.launch_arguments(),
            [
                "-ChannelService",
                "https://login.example.org",
                "-LoginChannelOverride",
                SESSION_PLACEHOLDER,
                "-AuthBackend",
                "nw",
                "-CognitoPool",
                "b04fbfcd-942c-472d-88c0-c762c4076b2f"
            ]
        );
    }

    #[test]
    fn bad_values_are_refused() {
        refused(
            &VALID.replace("domain = \"example.org\"", "domain = \"x.cloudfront.net\""),
            "domain",
        );
        refused(
            &VALID.replace("\"127.0.0.1:33435\"", "\"rep.example.org:33435\""),
            "rep_address",
        );
        refused(
            &VALID.replace("\"127.0.0.1:33435\"", "\"127.0.0.1:0\""),
            "rep_address",
        );
        refused(
            &VALID.replace("mode = \"password\"", "mode = \"discord\""),
            "discord.client_id",
        );
        refused(
            &VALID.replace("capacity = 100", "capacity = 0"),
            "world.capacity",
        );
        refused(
            &VALID.replace(
                "max_characters_per_account = 2",
                "max_characters_per_account = 0",
            ),
            "world.max_characters_per_account",
        );
    }

    #[test]
    fn remote_config_and_mirror_values_are_checked() {
        refused(
            &VALID.replace(
                "bucket = \"ags-javelin-remote-config\"",
                "bucket = \"my-bucket\"",
            ),
            "remote_config.bucket",
        );
        refused(
            &VALID.replace(
                "\"sts.us-east-1.amazonaws.com\",",
                "\"cognito-identity.us-east-1.amazonaws.com\",",
            ),
            "aws_mirror.hosts",
        );
        refused(
            &VALID.replace(
                "\"sts.us-east-1.amazonaws.com\",",
                "\"STS.us-east-1.amazonaws.com\",",
            ),
            "aws_mirror.hosts",
        );
        refused(
            &VALID.replace(
                "\"sts.us-east-1.amazonaws.com\",",
                "\"kinesis.us-east-1.amazonaws.com\",",
            ),
            "aws_mirror.hosts",
        );
        refused(
            &VALID.replace(
                "\"ags-javelin-remote-config.s3.amazonaws.com\",",
                "\"ags-javelin-dev-remote-config.s3.amazonaws.com\",",
            ),
            "aws_mirror.hosts",
        );
    }

    #[test]
    fn queue_limit_must_allow_normal_refreshes() {
        // A refresh every second needs 61 requests per minute (60 refreshes and the join).
        refused(
            &VALID.replace(
                "refresh_interval_seconds = 5",
                "refresh_interval_seconds = 1",
            ),
            "world.queue_requests_per_minute",
        );
        assert!(
            load(&VALID.replace(
                "queue_requests_per_minute = 30",
                "queue_requests_per_minute = 13"
            ))
            .is_ok()
        );
        refused(
            &VALID.replace(
                "queue_requests_per_minute = 30",
                "queue_requests_per_minute = 12",
            ),
            "world.queue_requests_per_minute",
        );
    }

    #[test]
    fn world_password_must_come_from_a_secret_variable() {
        refused(
            &VALID.replace("\"NW_SECRET_WORLD_1_DATABASE_PASSWORD\"", "\"HOME\""),
            "world.database_password_env",
        );
    }

    #[test]
    fn duplicate_world_ids_are_refused() {
        let start = VALID.find("[[world]]").expect("world block");
        let twice = format!("{VALID}{}", &VALID[start..]);
        refused(&twice, "world.id");
    }

    #[test]
    fn unknown_key_is_an_error() {
        assert!(matches!(
            load(&format!("{VALID}\n[extra]\nkey = 1\n")),
            Err(EdgeConfigError::Load(_))
        ));
    }
}
