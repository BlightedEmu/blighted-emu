// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool ability-instances`: the ability instance file
//! ([`nw_content::ability_instances`]) from the game paks: the `AbilityInstanceType` of every
//! dynamic slice with an `AbilityInstanceComponent`, by its asset id from the asset catalog, and
//! the instance limits of the player slice.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use nw_content::ability_instances::{
    AbilityInstanceTable, INSTANCE_COMPONENT, InstanceSlice, PLAYER_SLICE, TRACKING_COMPONENT,
    ability_instances_path, instance_type_of, limits_of,
};
use nw_content::town_props::ASSET_ID_LEN;
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, slice_assets_in};
use nw_formats::azcs::{decode_azcs, is_azcs};

use crate::error::ToolError;
use crate::source::GameSource;
use crate::world::{WorldBuildError, stream_of};

/// The folder of every dynamic slice in the catalog.
const SLICES_FOLDER: &str = "slices/";

/// What `ability-instances` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityInstancesReport {
    /// The file.
    pub path: PathBuf,
    /// The dynamic slices in the catalog.
    pub slices: usize,
    /// The slices with an instance type.
    pub typed: usize,
    /// The types with a limit in the player slice.
    pub limits: usize,
}

/// Whether the (AZCS decoded) bytes of a slice hold the type id `type_id`; only those slices are
/// parsed.
fn holds(bytes: &[u8], type_id: &[u8; ASSET_ID_LEN]) -> bool {
    let decoded = if is_azcs(bytes) {
        decode_azcs(bytes).map_or(Cow::Borrowed(bytes), Cow::Owned)
    } else {
        Cow::Borrowed(bytes)
    };
    decoded
        .windows(type_id.len())
        .any(|window| window == type_id)
}

/// Writes the ability instance file of `client_build` into `out_dir` from `source`.
///
/// # Errors
///
/// Returns [`ToolError`] when the paks, the catalog or a slice cannot be read, or the file
/// cannot be written.
pub fn build_ability_instances(
    source: &GameSource,
    client_build: &str,
    out_dir: &Path,
) -> Result<AbilityInstancesReport, ToolError> {
    let mut catalog = Vec::new();
    source.for_each_entry(
        |path| path == ASSET_CATALOG_ENTRY,
        |_, bytes| {
            catalog = bytes;
            Ok::<(), ToolError>(())
        },
    )?;
    let assets = slice_assets_in(&catalog, SLICES_FOLDER).map_err(WorldBuildError::from)?;
    let mut table = AbilityInstanceTable::default();
    source.for_each_entry(
        |path| assets.contains_key(path),
        |path, bytes| {
            if path == PLAYER_SLICE && holds(&bytes, &TRACKING_COMPONENT) {
                table.limits = limits_of(&stream_of(path, &bytes)?);
            }
            if !holds(&bytes, &INSTANCE_COMPONENT) {
                return Ok::<(), ToolError>(());
            }
            if let Some(instance_type) = instance_type_of(&stream_of(path, &bytes)?) {
                let (asset, _) = assets[path];
                table.slices.insert(
                    hex::encode(asset),
                    InstanceSlice {
                        path: path.to_owned(),
                        instance_type,
                    },
                );
            }
            Ok(())
        },
    )?;
    let path = ability_instances_path(out_dir, client_build);
    table.write(&path)?;
    Ok(AbilityInstancesReport {
        path,
        slices: assets.len(),
        typed: table.len(),
        limits: table.limits.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_slices_with_the_type_id_are_parsed() {
        let mut bytes = vec![0; 8];
        bytes.extend(INSTANCE_COMPONENT);
        bytes.push(0);
        assert!(holds(&bytes, &INSTANCE_COMPONENT));
        assert!(!holds(&bytes, &TRACKING_COMPONENT));
    }
}
