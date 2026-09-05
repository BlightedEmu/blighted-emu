// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed New World game rule tables and the content bundle file.

pub mod bundle;
pub mod client_build;
pub mod combat;
pub mod container;
pub mod fill;
/// Typed row structs and `Tables`, written by `content-tool codegen`. Do not edit by hand.
// The generator writes prettyplease output, which rustfmt would format differently.
#[rustfmt::skip]
pub mod generated;
pub mod table;

pub use bundle::ContentBundle;
pub use container::ContainerError;
pub use generated::Tables;
