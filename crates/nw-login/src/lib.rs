// SPDX-License-Identifier: AGPL-3.0-or-later
//! Login server logic (M2 spec): configuration, secrets, keys, entry ticket signatures, name
//! rules, password hashing, and the stores behind them. `edge-http` runs it.

pub mod config;
pub mod keys;
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
