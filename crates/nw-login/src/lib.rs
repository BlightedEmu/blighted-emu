// SPDX-License-Identifier: AGPL-3.0-or-later
//! Login server logic (M2 spec): configuration, secrets, keys, entry ticket signatures, name
//! rules, password hashing, the stores behind them, and the launcher API. `edge-http` runs it.

pub mod config;
pub mod discord;
pub mod error;
pub mod keys;
pub mod launcher;
#[cfg(test)]
mod memory;
pub mod names;
pub mod password;
pub mod pg;
pub mod secrets;
pub mod signature;
pub mod store;
pub mod time;
pub mod valkey;

/// Installs the `ring` TLS crypto provider for this process. HTTPS serving and the Discord client
/// both use it. A second call has no effect.
pub fn install_crypto_provider() {
    // Err only means a provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
}
