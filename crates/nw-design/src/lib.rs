// SPDX-License-Identifier: AGPL-3.0-or-later
//! Design files of the team (admin studio spec section 5): TOML files in `content/` that change,
//! add or disable client records. This crate loads them, checks them, and merges them over the
//! retail content bundle and world files.

pub mod edit;
pub mod error;
pub mod file;
pub mod load;
pub mod points;
pub mod records;
pub mod rows;
pub mod run;
pub mod schema;

pub use edit::{check_delete, check_put, delete_file, design_path, write_file};
pub use error::{DesignError, Problem};
pub use file::{Action, DesignFile, DesignSet, Kind, struct_fields};
pub use load::{DESIGN_SCHEMA, TODO_FILE, load_dir, parse_file};
pub use points::apply_world;
pub use records::{ClientRecord, client_records, record_field_names};
pub use rows::{DESIGN_SOURCE_PREFIX, apply_tables};
pub use run::{
    DesignMode, DesignPaths, DesignReport, DesignRunError, MergedBundle, merged_bundle, run_design,
};
pub use schema::check_schema;
