// SPDX-License-Identifier: AGPL-3.0-or-later
//! `sim` (M4 spec section 7): one cell of one level at 30 Hz. Players join through the bus, get
//! the login messages, see the players near them in replication bundles, and leave.

pub mod combat;
pub mod config;
pub mod forward;
pub mod gateways;
pub mod login;
pub mod mobs;
pub mod run;
pub mod saves;
pub mod server;
pub mod world;
