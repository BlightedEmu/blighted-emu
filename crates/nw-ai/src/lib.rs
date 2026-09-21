// SPDX-License-Identifier: AGPL-3.0-or-later
//! Mob AI from the game's own data: the Server.pak behavior trees, decoded into one shared node
//! arena ([`decode`], [`data`]), and the per-mob runtime that plans and executes them
//! ([`brain`]). Study: `local-data/session-notes/btree-STUDY.md` (behavior-tree study,
//! 2026-09-18). Marks in the docs: **Observed** (read in the data or the decompile),
//! **Inferred** (follows from the data, not seen run), **Unknown** (not found).

pub mod brain;
#[cfg(any(test, feature = "test-util"))]
pub mod build;
pub mod classes;
pub mod data;
pub mod decode;
pub mod keys;
pub mod node;
pub mod rng;
pub mod value;

pub use brain::{Brain, Command, Runtime, World};
pub use data::AiData;
