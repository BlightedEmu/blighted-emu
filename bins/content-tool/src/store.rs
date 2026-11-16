// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool store`: the store table ([`nw_protocol::store_api`]) from the content bundle,
//! for `edge-http`'s store catalog. It needs no game install.

use std::path::PathBuf;

use nw_content::ContentBundle;
use nw_content::layout::{ContentLayout, STORE, write_atomic};
use nw_content::store::store_table;
use nw_protocol::store_api::StoreTableError;

use crate::error::ToolError;

/// What `store` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreReport {
    /// The file.
    pub path: PathBuf,
    /// The mounts the store sells.
    pub mounts: usize,
    /// The token packs the store sells.
    pub tokens: usize,
    /// The entitlements every account holds.
    pub held_by_all: usize,
}

/// Writes the store table of `layout`, from its merged bundle.
///
/// # Errors
///
/// Returns [`ToolError`] when the bundle cannot be read or the table cannot be written.
pub fn write_store_table(layout: &ContentLayout) -> Result<StoreReport, ToolError> {
    let bundle = ContentBundle::load(layout)?;
    let table = store_table(&bundle.tables, &bundle.text);
    let path = layout.file(STORE);
    let bytes = table.to_json().map_err(|source| StoreTableError::Json {
        path: path.clone(),
        source,
    })?;
    write_atomic(&path, &bytes).map_err(|source| StoreTableError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(StoreReport {
        path,
        mounts: table.mounts.len(),
        tokens: table.tokens.len(),
        held_by_all: table.held_by_all.len(),
    })
}
