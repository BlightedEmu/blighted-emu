// SPDX-License-Identifier: AGPL-3.0-or-later
//! Design files of the team (admin studio spec section 5): TOML files in `content/` that change,
//! add or disable client records. This crate loads them, checks them, and merges them over the
//! retail content bundle and world files.

pub mod error;
pub mod file;
pub mod load;
pub mod schema;

pub use error::{DesignError, Problem};
pub use file::{Action, DesignFile, DesignSet, Kind, struct_fields};
pub use load::{DESIGN_SCHEMA, TODO_FILE, load_dir};
pub use schema::check_schema;
