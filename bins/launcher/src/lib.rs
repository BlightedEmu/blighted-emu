// SPDX-License-Identifier: GPL-3.0-or-later
//! The player launcher (M2 spec section 6): logs in to a community server (password or Discord)
//! and starts New World directly under Proton with the server's launch arguments, in a private
//! network view that leads the server's Amazon names to the server ([`mirror`]).

pub mod api;
pub mod app;
pub mod game;
pub mod mirror;
pub mod server;
pub mod settings;

/// Installs the `ring` TLS crypto provider for this process. A second call has no effect.
pub fn install_crypto_provider() {
    // Err only means a provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
}
