// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient` (M3 spec section 8): the client side of the REP connection, used by the test
//! client program and by the `rep-gateway` tests.

pub mod config;
pub mod pair;
pub mod rep;
pub mod route;
pub mod udp;
pub mod view;
pub mod walk;
