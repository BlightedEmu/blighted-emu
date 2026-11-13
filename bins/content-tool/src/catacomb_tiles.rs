// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool catacomb-tiles`: the catacombs tile file ([`nw_world::catacomb_tiles`]) from
//! the game paks: for each tile variation of the catacombs generation rules, its gameplay slice,
//! the art slice that slice spawns, the plain slices its slots spawn and the encounters of its
//! encounter slots (each gameplay tile read as a level script in the tile's frame,
//! [`build_level`]), with their asset ids from the asset catalog; and the power-up slices.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use nw_content::Tables;
use nw_content::catacombs::CatacombRules;
use nw_content::instances::CATACOMBS_GAME_MODE;
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, slice_assets_in};
use nw_formats::objectstream::Element;
use nw_world::catacomb_tiles::{
    CatacombTile, CatacombTiles, CatacombTilesError, POWER_UP_FOLDER, TileEncounter, TilePiece,
    catacomb_tiles_path, root_position, variation_slice,
};
use nw_world::dungeon_scripts::Encounter;
use nw_world::level_script::{Transform, slice_script};

use crate::collision::PakIndex;
use crate::dungeon_scripts::{
    DungeonScriptsReport, MobSources, PakSlices, SliceSource, build_level, mob_tables,
};
use crate::error::ToolError;
use crate::world::WorldBuildError;

/// The pak folder of every slice.
const SLICES_FOLDER: &str = "slices/";

/// What `catacomb-tiles` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatacombTilesReport {
    /// The file.
    pub path: PathBuf,
    /// The tile variations of the rules.
    pub variations: usize,
    /// The art tile slices the tiles spawn.
    pub art_tiles: usize,
    /// The plain slots of all tiles.
    pub props: usize,
    /// The encounters of all tiles.
    pub encounters: usize,
    /// The power-up slices.
    pub power_ups: usize,
}

/// The root of the slice at `path` in `slices`.
fn root_of(slices: &mut PakSlices<'_>, path: &str) -> Result<Option<Element>, ToolError> {
    Ok(slices.root(path)?)
}

/// The slices the slot entities of the gameplay slice `root` spawn.
fn spawned_slices(slice: &str, root: &Element) -> Result<BTreeSet<String>, ToolError> {
    let script = slice_script(root).map_err(|source| CatacombTilesError::Slice {
        slice: slice.to_owned(),
        source,
    })?;
    Ok(script
        .entities
        .iter()
        .filter_map(|entity| entity.spawner.as_ref()?.slice.as_ref())
        .map(|spawned| spawned.path.clone())
        .collect())
}

/// `encounters` once per slot and slice, each weighed by how many times it came (an alias slot
/// names a slice once per entry).
fn weighed(encounters: Vec<Encounter>) -> Vec<TileEncounter> {
    let mut weighed: Vec<TileEncounter> = Vec::new();
    for encounter in encounters {
        match weighed.iter_mut().find(|known| {
            known.encounter.name == encounter.name && known.encounter.slice == encounter.slice
        }) {
            Some(known) => known.weight += 1,
            None => weighed.push(TileEncounter {
                weight: 1,
                encounter,
            }),
        }
    }
    weighed
}

/// Writes the catacombs tile file of `client_build` into `out_dir` from the paks of `index`,
/// for the tile variations of the catacombs rules of `tables`; the mobs of the encounters come
/// from the tables and the mob prefab file in `out_dir`.
///
/// # Errors
///
/// Returns [`ToolError`] when the rules, the paks, the catalog or a slice cannot be read, a
/// tile has no art slice, or the file cannot be written.
pub fn build_catacomb_tiles(
    index: &PakIndex,
    tables: &Tables,
    client_build: &str,
    out_dir: &Path,
) -> Result<CatacombTilesReport, ToolError> {
    let rules = CatacombRules::for_game_mode(tables, CATACOMBS_GAME_MODE)?;
    let variations: BTreeMap<String, String> = rules
        .tiles
        .iter()
        .flat_map(|tile| &tile.variations)
        .map(|variation| (variation.to_lowercase(), variation_slice(variation)))
        .collect();
    let catalog = index
        .read_each(&[ASSET_CATALOG_ENTRY.to_owned()], |_, bytes| bytes)
        .into_iter()
        .next()
        .map(|(_, bytes)| bytes)
        .transpose()
        .map_err(crate::dungeon_scripts::DungeonScriptsBuildError::from)?
        .unwrap_or_default();
    // The slots spawn slices of other folders too (`CheckpointTile`:
    // `slices/interactobjects/checkpoint/interactobject_checkpoint_catacombs`).
    let assets = slice_assets_in(&catalog, SLICES_FOLDER).map_err(WorldBuildError::from)?;
    let asset_of = |path: &str| assets.get(path).map(|(guid, _)| *guid);
    let (levels, prefabs) = mob_tables(tables, out_dir, client_build);
    let mobs = MobSources {
        levels: &levels,
        prefabs: Some(&prefabs),
    };
    let mut slices = PakSlices::new(index);
    let mut report = DungeonScriptsReport::default();
    let mut by_variation = BTreeMap::new();
    for (variation, slice) in &variations {
        let root = root_of(&mut slices, slice)?.ok_or_else(|| CatacombTilesError::NoAsset {
            slice: slice.clone(),
        })?;
        let mut spawned: HashMap<String, Element> = HashMap::new();
        for path in spawned_slices(slice, &root)? {
            if let Some(spawned_root) = root_of(&mut slices, &path)? {
                spawned.insert(path, spawned_root);
            }
        }
        let mut tile = CatacombTile::of(slice, &root, |path| spawned.get(path).cloned(), asset_of)?;
        if let Some(level) = build_level(
            &mut slices,
            slice,
            Transform::default(),
            mobs,
            &[],
            &mut report,
        )? {
            tile.encounters = weighed(level.encounters);
        }
        by_variation.insert(variation.clone(), tile);
    }
    let mut power_ups = Vec::new();
    for (path, (asset, _)) in assets
        .iter()
        .filter(|(path, _)| path.starts_with(POWER_UP_FOLDER))
    {
        let Some(root) = root_of(&mut slices, path)? else {
            continue;
        };
        let script = slice_script(&root).map_err(|source| CatacombTilesError::Slice {
            slice: path.clone(),
            source,
        })?;
        let position_component =
            root_position(&script).ok_or_else(|| CatacombTilesError::NoPosition {
                slice: path.clone(),
            })?;
        power_ups.push(TilePiece {
            slice: path.clone(),
            asset: *asset,
            position_component,
            offset: [0.0; 3],
            yaw_deg: 0.0,
        });
    }
    power_ups.sort_by(|a, b| a.slice.cmp(&b.slice));
    let art_tiles = by_variation
        .values()
        .map(|tile: &CatacombTile| tile.art.slice.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let props = by_variation.values().map(|tile| tile.props.len()).sum();
    let encounters = by_variation
        .values()
        .map(|tile| tile.encounters.len())
        .sum();
    let table = CatacombTiles {
        by_variation,
        power_ups,
    };
    let path = catacomb_tiles_path(out_dir, client_build);
    table.write(&path)?;
    Ok(CatacombTilesReport {
        path,
        variations: table.by_variation.len(),
        art_tiles,
        props,
        encounters,
        power_ups: table.power_ups.len(),
    })
}
