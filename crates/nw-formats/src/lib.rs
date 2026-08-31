// SPDX-License-Identifier: MIT OR Apache-2.0
//! Readers for New World client file formats: pak archives, datasheets and localization files.

pub mod datasheet;
#[cfg(feature = "local-data")]
pub mod local_data;
pub mod pak;
