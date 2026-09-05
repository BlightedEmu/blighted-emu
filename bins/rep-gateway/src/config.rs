// SPDX-License-Identifier: AGPL-3.0-or-later
//! `config/rep-gateway.toml` with `NW_GATEWAY_` overrides (M3 spec section 6, M4 spec section
//! 6.3, M5 spec section 8).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nw_bus::BusId;
use nw_common::config::{ConfigError, load_config};
use serde::Deserialize;
use uuid::Uuid;

use crate::peer::PeerSettings;
use crate::workers::share;
use crate::world::WorldLinkSettings;

/// Environment prefix of `rep-gateway` overrides.
pub const GATEWAY_ENV_PREFIX: &str = "NW_GATEWAY_";
/// Smallest DTLS MTU OpenSSL accepts for DTLS 1.2 with this cipher.
pub const MIN_MTU: u32 = 576;
/// Largest DTLS MTU on Ethernet.
pub const MAX_MTU: u32 = 1500;
/// Most workers: the worker number is a `u16`.
pub const MAX_WORKERS: usize = u16::MAX as usize;

/// The whole `rep-gateway` configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
    /// Public UDP listener; `RepAddress` in `config/edge.toml` points here.
    pub udp_listen: SocketAddr,
    /// World this gateway serves; a ticket for another world is refused.
    pub world_id: Uuid,
    /// Server version text in the registration response.
    pub server_version: String,
    /// The gateway's own certificate (PEM); test clients trust exactly this file.
    pub certificate_file: PathBuf,
    /// Its private key (PEM).
    pub private_key_file: PathBuf,
    /// Valkey with the entry records of `edge-http`.
    pub valkey_url: String,
    /// Internal Prometheus listener.
    pub metrics_listen: SocketAddr,
    /// DTLS MTU.
    pub mtu: u32,
    /// `tracing` filter, for example `info`.
    pub log_filter: String,
    /// This gateway's id in bus subjects.
    pub gateway_id: BusId,
    /// NATS, internal only.
    pub nats_url: String,
    /// Longest wait for the director to place a character; below
    /// `limits.registration_timeout_ms`.
    pub join_timeout_ms: u64,
    /// Time between heartbeats.
    pub heartbeat_interval_ms: u64,
    /// Workers on `udp_listen`, one per core given to the gateway.
    pub workers: usize,
    /// Receive and send buffer of each worker socket, in bytes.
    pub udp_buffer_bytes: usize,
    /// Most resumes that wait for the director at one time.
    pub max_resumes_in_flight: usize,
    /// Connection limits and timers; the counts are shared out over the workers.
    pub limits: GatewayLimits,
}

/// `[limits]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayLimits {
    /// Connections per gateway.
    pub max_connections: usize,
    /// DTLS handshakes in progress per gateway.
    pub max_handshakes: usize,
    /// New connections per IP address (IPv6: per /64) per second.
    pub handshakes_per_ip_per_second: u32,
    /// Time from the first datagram to the registration response.
    pub registration_timeout_ms: u64,
    /// Time without a datagram before a connection is dropped.
    pub silence_timeout_ms: u64,
    /// Datagrams one connection may send per second.
    pub datagrams_per_connection_per_second: u32,
    /// Time between server pings.
    pub ping_interval_ms: u64,
}

/// A value of the configuration that is not allowed.
#[derive(Debug, thiserror::Error)]
pub enum GatewayConfigError {
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
/// Returns [`GatewayConfigError`] when the file is missing, has a wrong key, or a value is not
/// allowed.
pub fn load_gateway_config(path: &Path) -> Result<GatewayConfig, GatewayConfigError> {
    let config: GatewayConfig = load_config(path, GATEWAY_ENV_PREFIX)?;
    config.validate()?;
    Ok(config)
}

impl GatewayConfig {
    fn validate(&self) -> Result<(), GatewayConfigError> {
        let invalid = |key, reason| Err(GatewayConfigError::Invalid { key, reason });
        if self.server_version.is_empty() {
            return invalid("server_version", "must not be empty");
        }
        if !(MIN_MTU..=MAX_MTU).contains(&self.mtu) {
            return invalid("mtu", "must be 576 to 1500");
        }
        let limits = &self.limits;
        let counts = [
            limits.max_connections,
            limits.max_handshakes,
            limits.handshakes_per_ip_per_second as usize,
            limits.datagrams_per_connection_per_second as usize,
        ];
        if counts.contains(&0) {
            return invalid("limits", "counts must be above 0");
        }
        let times = [
            limits.registration_timeout_ms,
            limits.silence_timeout_ms,
            limits.ping_interval_ms,
        ];
        if times.contains(&0) {
            return invalid("limits", "times must be above 0");
        }
        if limits.ping_interval_ms >= limits.silence_timeout_ms {
            return invalid(
                "limits.ping_interval_ms",
                "must be below silence_timeout_ms",
            );
        }
        if self.join_timeout_ms == 0 || self.join_timeout_ms >= limits.registration_timeout_ms {
            return invalid(
                "join_timeout_ms",
                "must be above 0 and below limits.registration_timeout_ms",
            );
        }
        if self.heartbeat_interval_ms == 0 {
            return invalid("heartbeat_interval_ms", "must be above 0");
        }
        if !(1..=MAX_WORKERS).contains(&self.workers) || (self.workers > 1 && !cfg!(unix)) {
            return invalid(
                "workers",
                "must be 1 to 65535; more than 1 needs SO_REUSEPORT (Unix)",
            );
        }
        if self.udp_buffer_bytes == 0 {
            return invalid("udp_buffer_bytes", "must be above 0");
        }
        if self.max_resumes_in_flight == 0 {
            return invalid("max_resumes_in_flight", "must be above 0");
        }
        Ok(())
    }

    /// The settings of the link to the world.
    pub fn world_link(&self) -> WorldLinkSettings {
        WorldLinkSettings {
            world_id: self.world_id,
            gateway_id: self.gateway_id.clone(),
            join_timeout: Duration::from_millis(self.join_timeout_ms),
            heartbeat_interval: Duration::from_millis(self.heartbeat_interval_ms),
            max_resumes_in_flight: self.max_resumes_in_flight,
        }
    }

    /// The limits of one worker: the handshakes in progress shared out. The connection limit is
    /// one count for every worker ([`ConnectionCount`](crate::workers::ConnectionCount)).
    pub const fn worker_limits(&self) -> GatewayLimits {
        GatewayLimits {
            max_handshakes: share(self.limits.max_handshakes, self.workers),
            ..self.limits
        }
    }

    /// The settings of one connection.
    pub const fn peer_settings(&self) -> PeerSettings {
        PeerSettings {
            mtu: self.mtu,
            registration_timeout_ms: self.limits.registration_timeout_ms,
            silence_timeout_ms: self.limits.silence_timeout_ms,
            ping_interval_ms: self.limits.ping_interval_ms,
            datagrams_per_second: self.limits.datagrams_per_connection_per_second,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const COMMITTED_CONFIG: &str = "../../config/rep-gateway.toml";
    const BAD_MTU: u32 = 100;

    /// The committed example configuration.
    pub(crate) fn committed() -> GatewayConfig {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED_CONFIG);
        load_gateway_config(&path).expect("committed config")
    }

    #[test]
    fn committed_example_config_is_valid() {
        let config = committed();
        assert_eq!(config.limits.ping_interval_ms, 1000);
        assert_eq!(config.peer_settings().mtu, config.mtu);
        let link = config.world_link();
        assert_eq!(link.world_id, config.world_id);
        assert_eq!(
            link.join_timeout,
            Duration::from_millis(config.join_timeout_ms)
        );
        assert_eq!(
            link.heartbeat_interval,
            Duration::from_millis(config.heartbeat_interval_ms)
        );
        let mut many = config.clone();
        many.workers = 4;
        assert_eq!(
            many.worker_limits().max_handshakes,
            config.limits.max_handshakes.div_ceil(4)
        );
        assert_eq!(
            many.worker_limits().max_connections,
            config.limits.max_connections
        );
    }

    #[test]
    fn bad_values_are_refused() {
        let mut config = committed();
        config.mtu = BAD_MTU;
        assert!(matches!(
            config.validate(),
            Err(GatewayConfigError::Invalid { key: "mtu", .. })
        ));
        let mut config = committed();
        config.limits.max_handshakes = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.limits.ping_interval_ms = config.limits.silence_timeout_ms;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.server_version.clear();
        assert!(config.validate().is_err());
        let mut config = committed();
        config.join_timeout_ms = config.limits.registration_timeout_ms;
        assert!(matches!(
            config.validate(),
            Err(GatewayConfigError::Invalid {
                key: "join_timeout_ms",
                ..
            })
        ));
        let mut config = committed();
        config.heartbeat_interval_ms = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.workers = 0;
        assert!(config.validate().is_err());
        let mut config = committed();
        config.udp_buffer_bytes = 0;
        assert!(config.validate().is_err());
    }
}
