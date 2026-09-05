// SPDX-License-Identifier: AGPL-3.0-or-later
//! `rep-gateway` (M3 spec section 3): the UDP entry point of a world. It terminates DTLS, runs the
//! GridMate carrier, checks the login ticket in the V3 registration, and keeps registered
//! connections alive with pings.

pub mod config;
#[cfg(all(test, feature = "infra"))]
mod end_to_end;
pub mod peer;
pub mod registration;
pub mod routes;
pub mod run;
pub mod server;
pub mod workers;
pub mod world;
