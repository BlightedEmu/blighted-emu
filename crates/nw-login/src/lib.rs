// SPDX-License-Identifier: AGPL-3.0-or-later
//! Login server logic (M2 spec): configuration, secrets, keys, entry ticket signatures, name
//! rules, password hashing, the stores behind them, the launcher API and the game routes.
//! `edge-http` runs it.

pub mod config;
pub mod discord;
pub mod error;
pub mod game;
pub mod host;
pub mod keys;
pub mod launcher;
#[cfg(test)]
mod memory;
pub mod names;
pub mod password;
pub mod pg;
pub mod queue;
#[cfg(all(test, feature = "infra"))]
mod scripted_client;
pub mod secrets;
pub mod signature;
pub mod store;
#[cfg(test)]
mod test_http;
#[cfg(any(test, feature = "test-util"))]
pub mod test_support;
pub mod time;
pub mod valkey;

/// Installs the `ring` TLS crypto provider for this process. HTTPS serving and the Discord client
/// both use it. A second call has no effect.
pub fn install_crypto_provider() {
    // Err only means a provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
}
