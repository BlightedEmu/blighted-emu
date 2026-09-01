// SPDX-License-Identifier: AGPL-3.0-or-later
//! Listen addresses, the advertised `RepAddress`, and the channel JSON fields the client reads.
//!
//! The game client keeps one embedded certificate for `gridmate-udp` and `gridmate-udp-bsd`.
//! This crate records that certificate's subject and the cipher name.
//! It does not contain the certificate or a private key.

pub mod channel;
pub mod server;
