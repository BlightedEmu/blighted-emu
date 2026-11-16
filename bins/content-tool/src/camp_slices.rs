// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool camp-slices`: the camp slice file ([`nw_world::camps`]) from the game paks:
//! every `.dynamicslice` under [`CAMP_SLICE_FOLDER`] that is a camp, with its asset id from the
//! asset catalog.

use std::path::PathBuf;

use nw_content::layout::{CAMP_SLICES, ContentLayout};
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, slice_assets_in};
use nw_world::camps::{CAMP_SLICE_FOLDER, CampSlice, CampSlices};

use crate::error::ToolError;
use crate::source::GameSource;
use crate::world::{WorldBuildError, stream_of};

/// What `camp-slices` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampSlicesReport {
    /// The file.
    pub path: PathBuf,
    /// The dynamic slices of the camp folder in the catalog.
    pub slices: usize,
    /// The camps among them.
    pub camps: usize,
    /// The PvP camps among them (with a vitals component).
    pub pvp_camps: usize,
}

/// Writes the camp slice file of `layout` from `source`.
///
/// # Errors
///
/// Returns [`ToolError`] when the paks, the catalog or a slice cannot be read, or the file
/// cannot be written.
pub fn build_camp_slices(
    source: &GameSource,
    layout: &ContentLayout,
) -> Result<CampSlicesReport, ToolError> {
    let mut catalog = Vec::new();
    source.for_each_entry(
        |path| path == ASSET_CATALOG_ENTRY,
        |_, bytes| {
            catalog = bytes;
            Ok::<(), ToolError>(())
        },
    )?;
    let assets = slice_assets_in(&catalog, CAMP_SLICE_FOLDER).map_err(WorldBuildError::from)?;
    let mut camps = Vec::new();
    source.for_each_entry(
        |path| assets.contains_key(path),
        |path, bytes| {
            let root = stream_of(path, &bytes)?;
            let (asset, sub_id) = assets[path];
            camps.extend(CampSlice::of(path, &root, asset, sub_id));
            Ok::<(), ToolError>(())
        },
    )?;
    let pvp_camps = camps.iter().filter(|camp| camp.vitals.is_some()).count();
    let table = CampSlices::of(camps);
    let path = layout.file(CAMP_SLICES);
    table.write(&path)?;
    Ok(CampSlicesReport {
        path,
        slices: assets.len(),
        camps: table.len(),
        pvp_camps,
    })
}
