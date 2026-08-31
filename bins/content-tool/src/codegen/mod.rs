// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generates typed Rust structs for every datasheet table type (M1 spec section 4).

pub mod naming;
pub mod schema;

/// Error from code generation.
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    /// No datasheet was read.
    #[error("no datasheets were found")]
    NoTables,
    /// A table type name (or its module name) is not a Rust identifier.
    #[error("table type `{0}` is not a valid Rust type or module name")]
    InvalidTypeName(String),
    /// A table type has no columns in any file.
    #[error("table type `{0}` has no columns")]
    NoColumns(String),
    /// Two table types give the same module name.
    #[error("table types `{first}` and `{second}` both map to module `{module}`")]
    ModuleCollision {
        /// First type (sorted order).
        first: String,
        /// Second type.
        second: String,
        /// The shared module name.
        module: String,
    },
}
