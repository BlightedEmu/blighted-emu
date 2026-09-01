// SPDX-License-Identifier: AGPL-3.0-or-later
//! Login file: database, Valkey, TLS paths, and queue limits.
//!
//! The HTTPS listen address and `RepAddress` stay in the edge file loaded by
//! `nw-edge-config`. This file uses a different environment prefix so those
//! keys do not collide.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nw_common::config::{ConfigError, load_config};
use serde::Deserialize;

use crate::queue::QueueSettings;

/// Environment prefix for this file.
/// Nested keys use `__`, for example `NW_LOGIN_DATABASE__URL`.
pub(crate) const LOGIN_ENV_PREFIX: &str = "NW_LOGIN_";

/// Shortest HMAC key, in bytes.
const MIN_SIGNATURE_KEY_LEN: usize = 16;

/// Longest world id and queue name, in bytes.
const MAX_NAME_LEN: usize = 128;

/// Longest `RepAddress`, in bytes.
const MAX_REP_ADDRESS_LEN: usize = 256;

/// Login settings that are not in the edge file.
#[derive(Debug, Clone)]
pub(crate) struct LoginConfig {
    /// Tracing filter.
    pub log_filter: String,
    /// PEM certificate file for the HTTPS listener.
    pub certificate_pem_path: PathBuf,
    /// PEM private key file for the HTTPS listener.
    pub private_key_pem_path: PathBuf,
    /// PostgreSQL URL for the global database.
    pub database_url: String,
    /// Pool size.
    pub max_connections: u32,
    /// Valkey URL.
    pub valkey_url: String,
    /// Queue limits. `RepAddress` is filled from the edge file.
    pub queue: QueueParts,
}

/// Queue values from the login file.
#[derive(Debug, Clone)]
pub(crate) struct QueueParts {
    /// World id.
    pub world_id: String,
    /// Queue name.
    pub queue_name: String,
    /// Live entry-ticket limit.
    pub capacity: u32,
    /// Refresh interval in seconds.
    pub refresh_interval_seconds: i64,
    /// Seconds added to the estimate for each queue place.
    pub estimated_seconds_per_position: i64,
    /// HMAC key.
    pub signature_key: Vec<u8>,
}

/// The login file could not be loaded or a value is not usable.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LoginConfigError {
    /// The file is missing or a key has the wrong type.
    #[error(transparent)]
    Load(#[from] ConfigError),
    /// A required string is empty or too long.
    #[error("login config field {field} is empty or too long")]
    InvalidField {
        /// Config key.
        field: &'static str,
    },
    /// A number is zero or does not fit the queue reply.
    #[error("login config field {field} is not in range")]
    InvalidNumber {
        /// Config key.
        field: &'static str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginFile {
    log_filter: String,
    tls: TlsFile,
    database: DatabaseFile,
    valkey: ValkeyFile,
    login_queue: QueueFile,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TlsFile {
    certificate_pem_path: PathBuf,
    private_key_pem_path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DatabaseFile {
    url: String,
    max_connections: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValkeyFile {
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueFile {
    world_id: String,
    queue_name: String,
    capacity: u32,
    refresh_interval_seconds: u64,
    estimated_seconds_per_position: u64,
    signature_key: String,
}

/// Loads and checks the login file.
pub(crate) fn load_login_config(path: &Path) -> Result<LoginConfig, LoginConfigError> {
    let file: LoginFile = load_config(path, LOGIN_ENV_PREFIX)?;
    let queue = queue_parts(&file.login_queue)?;
    require_text("log_filter", &file.log_filter, MAX_NAME_LEN)?;
    require_text("database.url", &file.database.url, MAX_REP_ADDRESS_LEN * 4)?;
    require_text("valkey.url", &file.valkey.url, MAX_REP_ADDRESS_LEN)?;
    if file.tls.certificate_pem_path.as_os_str().is_empty()
        || file.tls.private_key_pem_path.as_os_str().is_empty()
    {
        return Err(LoginConfigError::InvalidField { field: "tls" });
    }
    if file.database.max_connections == 0 {
        return Err(LoginConfigError::InvalidNumber {
            field: "database.max_connections",
        });
    }
    Ok(LoginConfig {
        log_filter: file.log_filter,
        certificate_pem_path: file.tls.certificate_pem_path,
        private_key_pem_path: file.tls.private_key_pem_path,
        database_url: file.database.url,
        max_connections: file.database.max_connections,
        valkey_url: file.valkey.url,
        queue,
    })
}

impl LoginConfig {
    /// Queue settings with `RepAddress` taken from the edge file.
    pub(crate) fn queue_settings(
        &self,
        rep_address: String,
    ) -> Result<Arc<QueueSettings>, LoginConfigError> {
        require_text("rep_address", &rep_address, MAX_REP_ADDRESS_LEN)?;
        Ok(Arc::new(QueueSettings {
            world_id: self.queue.world_id.clone(),
            queue_name: self.queue.queue_name.clone(),
            capacity: self.queue.capacity,
            refresh_interval_seconds: self.queue.refresh_interval_seconds,
            estimated_seconds_per_position: self.queue.estimated_seconds_per_position,
            rep_address,
            signature_key: self.queue.signature_key.clone(),
        }))
    }
}

fn queue_parts(file: &QueueFile) -> Result<QueueParts, LoginConfigError> {
    require_text("login_queue.world_id", &file.world_id, MAX_NAME_LEN)?;
    require_text("login_queue.queue_name", &file.queue_name, MAX_NAME_LEN)?;
    if file.signature_key.len() < MIN_SIGNATURE_KEY_LEN {
        return Err(LoginConfigError::InvalidField {
            field: "login_queue.signature_key",
        });
    }
    let refresh_interval_seconds = i64::try_from(file.refresh_interval_seconds).map_err(|_| {
        LoginConfigError::InvalidNumber {
            field: "login_queue.refresh_interval_seconds",
        }
    })?;
    if refresh_interval_seconds <= 0 {
        return Err(LoginConfigError::InvalidNumber {
            field: "login_queue.refresh_interval_seconds",
        });
    }
    let estimated_seconds_per_position = i64::try_from(file.estimated_seconds_per_position)
        .map_err(|_| LoginConfigError::InvalidNumber {
            field: "login_queue.estimated_seconds_per_position",
        })?;
    Ok(QueueParts {
        world_id: file.world_id.clone(),
        queue_name: file.queue_name.clone(),
        capacity: file.capacity,
        refresh_interval_seconds,
        estimated_seconds_per_position,
        signature_key: file.signature_key.as_bytes().to_vec(),
    })
}

fn require_text(field: &'static str, value: &str, max_len: usize) -> Result<(), LoginConfigError> {
    if value.is_empty() || value.len() > max_len {
        Err(LoginConfigError::InvalidField { field })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_file_loads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("login.example.toml");
        let config = load_login_config(&path).expect("example");
        let settings = config
            .queue_settings("127.0.0.1:33435".to_owned())
            .expect("settings");
        assert_eq!(settings.world_id, "world-1");
        assert_eq!(settings.rep_address, "127.0.0.1:33435");
        assert!(settings.capacity > 0);
    }

    #[test]
    fn short_signature_key_is_rejected() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("login.toml");
        let text = include_str!("../login.example.toml").replace(
            "signature_key = \"change-me-signature-key\"",
            "signature_key = \"short\"",
        );
        std::fs::write(&path, text).expect("write");
        let error = load_login_config(&path).expect_err("short key");
        assert!(matches!(
            error,
            LoginConfigError::InvalidField {
                field: "login_queue.signature_key"
            }
        ));
    }
}
