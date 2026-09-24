// SPDX-License-Identifier: AGPL-3.0-or-later
//! `world-director` (M5 spec section 6): one per world. It shares the cells of the open map over
//! the live `sim` processes, routes each join to the owner of the player's cell, takes the entity
//! numbers, and reports the free player slots to the login queue.

pub mod config;
pub mod instance_loop;
pub mod instances;
pub mod join;
pub mod run;
pub mod server;
pub mod sims;
pub mod trading_post;
