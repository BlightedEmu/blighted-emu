// SPDX-License-Identifier: GPL-3.0-or-later
//! The player launcher (M2 spec section 6): logs in to a community server (password or Discord)
//! and starts New World directly under Proton with the server's launch arguments, in a private
//! network view that leads the server's Amazon names to the server ([`mirror`]).

pub mod api;
pub mod app;
/// The Windows loopback proxy (`docs/re/2026-09-20-windows-launcher-network.md`). Linux uses the
/// `bwrap` path of [`mirror`] instead, so this is built only on Windows and for tests.
#[cfg(any(windows, test))]
pub mod aws_proxy;
pub mod game;
pub mod mirror;
pub mod server;
pub mod settings;
pub mod theme;

/// Installs the `ring` TLS crypto provider for this process. A second call has no effect.
pub fn install_crypto_provider() {
    // Err only means a provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
}
