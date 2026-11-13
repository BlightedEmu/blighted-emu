// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool catacomb-tiles`: the catacombs tile file ([`nw_world::catacomb_tiles`]) from
//! the game paks: for each tile variation of the catacombs generation rules, its gameplay slice,
//! the art slice that slice spawns, the plain slices its slots spawn and the encounters of its
//! encounter slots (each gameplay tile read as a level script in the tile's frame,
//! [`build_level`]), with their asset ids from the asset catalog, and its map picture id
//! ([`tile_pictures`] of every `DungeonTileStaticData` row); and the power-up slices. It
//! also writes the collision file of the catacombs level: the solids of each art tile slice in
//! its tile's frame ([`build_slices_collision`]), which the sim places with the run's tiles.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use nw_content::Tables;
use nw_content::catacombs::CatacombRules;
use nw_content::instances::{CATACOMBS_GAME_MODE, InstanceCatalog};
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, slice_assets_in};
use nw_formats::objectstream::Element;
use nw_world::catacomb_tiles::{
    CatacombTile, CatacombTiles, CatacombTilesError, POWER_UP_FOLDER, TileEncounter, TilePiece,
    catacomb_tiles_path, root_position, tile_pictures, variation_slice,
};
use nw_world::dungeon_scripts::Encounter;
use nw_world::level_script::{Transform, slice_script};

use crate::collision::{AssetKey, PakIndex, build_slices_collision};
use crate::dungeon_gates::latest_clip_end;
use crate::dungeon_scripts::{
    DungeonScriptsReport, PakSlices, SliceSource, build_level, mob_tables,
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
    /// The collision file of the catacombs level.
    pub collision_path: PathBuf,
    /// The art tiles with solids in it.
    pub solid_art_tiles: usize,
    /// Their placed shapes.
    pub solid_bodies: usize,
}

/// The root of the slice at `path` in `slices`.
fn root_of(slices: &mut PakSlices<'_>, path: &str) -> Result<Option<Element>, ToolError> {
    Ok(slices.root(path)?)
}

/// Adds to `lengths` the length of each timeline asset of the slice `path` with root `root`
/// that it lacks (seconds, the latest clip end; 0 for an asset not in the paks).
fn timeline_lengths(
    slices: &mut PakSlices<'_>,
    path: &str,
    root: &Element,
    lengths: &mut HashMap<String, f32>,
) -> Result<(), ToolError> {
    let script = slice_script(root).map_err(|source| CatacombTilesError::Slice {
        slice: path.to_owned(),
        source,
    })?;
    for asset in script.timeline_assets() {
        if lengths.contains_key(&asset.path) {
            continue;
        }
        let length = root_of(slices, &asset.path)?
            .and_then(|timeline| latest_clip_end(&timeline))
            .unwrap_or_default();
        lengths.insert(asset.path.clone(), length);
    }
    Ok(())
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

/// The map picture id of every tile variation (lower case) of `tables`: [`tile_pictures`] of
/// all `DungeonTileStaticData` rows of every sheet and feature, in datasheet order, as the
/// client builds its table from every row it loads.
fn tile_pictures_of(tables: &Tables) -> Result<BTreeMap<String, u8>, CatacombTilesError> {
    tile_pictures(
        tables
            .dungeon_tile_static_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                (
                    row.dungeon_tile_id.as_str(),
                    row.variation_asset_paths.as_str(),
                )
            }),
    )
}

/// Writes the catacombs tile file of `client_build` into `out_dir` from the paks of `index`,
/// for the tile variations of the catacombs rules of `tables`; the mobs of the encounters come
/// from the tables and the mob prefab file in `out_dir`.
///
/// # Errors
///
/// Returns [`ToolError`] when the rules, the paks, the catalog or a slice cannot be read, a
/// tile has no art slice or no picture id, or the file cannot be written.
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
    let pictures = tile_pictures_of(tables)?;
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
    let tables_of_mobs = mob_tables(tables, out_dir, client_build);
    let mobs = tables_of_mobs.sources();
    let mut slices = PakSlices::new(index);
    let mut report = DungeonScriptsReport::default();
    let mut by_variation = BTreeMap::new();
    let mut lengths: HashMap<String, f32> = HashMap::new();
    for (variation, slice) in &variations {
        let root = root_of(&mut slices, slice)?.ok_or_else(|| CatacombTilesError::NoAsset {
            slice: slice.clone(),
        })?;
        let mut spawned: HashMap<String, Element> = HashMap::new();
        for path in spawned_slices(slice, &root)? {
            if let Some(spawned_root) = root_of(&mut slices, &path)? {
                timeline_lengths(&mut slices, &path, &spawned_root, &mut lengths)?;
                spawned.insert(path, spawned_root);
            }
        }
        let mut tile = CatacombTile::of(
            slice,
            &root,
            |path| spawned.get(path).cloned(),
            asset_of,
            |path| lengths.get(path).copied().unwrap_or_default(),
        )?;
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
        tile.picture =
            *pictures
                .get(variation)
                .ok_or_else(|| CatacombTilesError::NoPicture {
                    variation: variation.clone(),
                })?;
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
    let level = InstanceCatalog::from_tables(tables)
        .by_id(CATACOMBS_GAME_MODE)
        .map(|mode| mode.map.folder.clone())
        .ok_or_else(|| CatacombTilesError::NoLevel {
            game_mode: CATACOMBS_GAME_MODE.to_owned(),
        })?;
    let art_keys: Vec<AssetKey> = table
        .by_variation
        .values()
        .filter_map(|tile| assets.get(&tile.art.slice).copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let collision = build_slices_collision(index, &level, &art_keys, client_build, out_dir)?;
    Ok(CatacombTilesReport {
        path,
        variations: table.by_variation.len(),
        art_tiles,
        props,
        encounters,
        power_ups: table.power_ups.len(),
        collision_path: collision.path,
        solid_art_tiles: collision.deferred_slices,
        solid_bodies: collision.deferred_bodies,
    })
}

#[cfg(test)]
mod tests {
    use nw_content::generated::DungeonTileStaticData;
    use nw_content::table::Table;

    use super::*;

    /// A tile row of `feature` with id `id` and the paths `paths`.
    fn tile_row(id: &str, feature: &str, paths: &str) -> DungeonTileStaticData {
        DungeonTileStaticData {
            connections: "NS".to_owned(),
            dungeon_tile_id: id.to_owned(),
            feature_id: feature.to_owned(),
            rotations: 1,
            supported_room_types: "Hallway".to_owned(),
            tile_size: 64,
            variation_asset_paths: paths.to_owned(),
            weight: None,
        }
    }

    /// The pictures come from the rows of every sheet and feature in one table: `Second` (a
    /// test feature row of the second sheet, bucket 1 of 2) takes the first id before `First`
    /// (bucket 0), as the client lists its map's last bucket first.
    #[test]
    fn the_pictures_count_every_row_of_every_sheet() {
        let mut tables = Tables::default();
        for (sheet, row) in [
            ("first.datasheet", tile_row("First", "Catacombs", "Tiles/X")),
            (
                "second.datasheet",
                tile_row("Second", "UnitTesting_Catacombs", "Tiles/Y"),
            ),
        ] {
            tables.dungeon_tile_static_data.push(Table {
                source_path: sheet.to_owned(),
                unique_id: String::new(),
                rows: vec![row],
            });
        }
        assert_eq!(
            tile_pictures_of(&tables).expect("pictures"),
            BTreeMap::from([("tiles/x".to_owned(), 2), ("tiles/y".to_owned(), 1)])
        );
        assert!(
            tile_pictures_of(&Tables::default())
                .expect("no rows")
                .is_empty()
        );
    }
}
