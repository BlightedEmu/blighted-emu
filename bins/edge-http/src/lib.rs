// SPDX-License-Identifier: AGPL-3.0-or-later
//! HTTPS login queue for the preservation server.
//!
//! The architecture spec names `RepAddress` and `Signature` on the queue reply.
//! The client reads both strings from `Token`, so this server puts them there.

mod characters;
mod config;
mod http;
#[cfg(test)]
mod memory;
mod queue;
mod request;
mod run;
mod signature;
mod valkey;

pub use run::run;
