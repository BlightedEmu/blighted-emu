// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed New World game rule tables and the content bundle file.

pub mod fill;
/// Typed row structs and `Tables`, written by `content-tool codegen`. Do not edit by hand.
// The generator writes prettyplease output, which rustfmt would format differently.
#[rustfmt::skip]
pub mod generated;
pub mod table;

pub use generated::Tables;
