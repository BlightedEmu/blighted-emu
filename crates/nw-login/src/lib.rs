// SPDX-License-Identifier: AGPL-3.0-or-later
//! Login server logic (M2 spec): configuration, secrets, keys, entry ticket signatures, name
//! rules, and password hashing. `edge-http` runs it.

pub mod config;
pub mod keys;
pub mod names;
pub mod password;
pub mod secrets;
pub mod signature;
