// SPDX-License-Identifier: AGPL-3.0-or-later
//! The catacombs tiles (`catacomb-tiles-<build>.json`, written by `content-tool
//! catacomb-tiles`, read by the instance sim of the catacombs): for each tile variation of the
//! `DungeonTileStaticData` rows (a `VariationAssetPaths` entry, lower case), the gameplay tile
//! slice and the art tile slice that the gameplay slice's `PrefabSpawnerComponent` spawns, each
//! with its asset id (the asset catalog's), the replica index of its position component and its
//! place in the tile.
//!
//! Observed (retail catacomb ledgers, `CATACOMBS2` 17.292 s and on): each tile of a run comes as
//! two cards at one place, the gameplay slice (`slices/catacombs/gameplay/tiles/tile4_a_gameplay`,
//! asset `7372480c…`) and the art slice (`slices/catacombs/arttiles/tile4_a`, asset `acb8a866…`),
//! each with its position state at member 1 (`1:13`); the art slice is the one the gameplay
//! slice's prefab spawner names (`68091000` field, hint path
//! `slices/catacombs/arttiles/tile4_a.dynamicslice`).
//!
//! A gameplay tile also holds the slots of its room: one entity per kind of room gameplay
//! (`EncounterKey`, `EncounterBoss`, `EncounterNamedEnemy`, `EncounterHazard`, `EncounterGate`,
//! `Hallway`, `GateNorth`, `CheckpointTile`, `PowerUps`, ...), each a prefab spawner of a slice
//! or an alias asset (Observed: 52 gameplay tile slices, `tile1_a_gameplay` holds 52 entities).
//! The file keeps each slot that spawns a plain slice as a [`TileProp`] and each slot that
//! spawns an encounter slice (one with an `EncounterManagerComponent`, each entry of an alias
//! slot) as an [`Encounter`] in the tile's frame; the run picks the slots of its room's kind.
//! Observed (`CATACOMBS2` 17.292 s): the start room's checkpoint card stands 12 m west of its
//! unturned tile's centre, as `CheckpointTile` (-12, 0, -2) in `tile3_a_gameplay`; a named enemy
//! room's `dg_enc_catacombs_namedenemy_werebeast` stands at the tile's centre, 2 m down, as
//! `EncounterNamedEnemy` (0, 0, -2) in `tile4_a_gameplay`.
//!
//! Each tile also keeps its map picture id ([`tile_pictures`]): the id the run map's
//! `tileUiFilenameIdAndRotation` sends above the tile's quarter turns.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nw_content::dungeon_gates::GateTimeline;
use nw_content::instances::game_mode_indexes;
use nw_formats::objectstream::{Element, lowercase_crc32};
use serde::{Deserialize, Serialize};

use crate::collision::{Affine, Body, LevelCollision};
use crate::collision_index::CollisionIndex;
use crate::dungeon_scripts::Encounter;
use crate::level_script::{
    LevelScriptError, ReplicatedKind, SliceScript, Transform, pak_path, slice_script,
};
use crate::slice_data::GUID_LEN;

/// The file name prefix; the client build and [`CATACOMB_TILES_EXTENSION`] follow.
pub const CATACOMB_TILES_PREFIX: &str = "catacomb-tiles-";
/// The file extension.
pub const CATACOMB_TILES_EXTENSION: &str = ".json";
/// The pak folder of the art tile slices.
pub const ART_TILE_FOLDER: &str = "slices/catacombs/arttiles/";
/// The pak folder of the power-up slices (Observed: the 11 `catacombs_powerup_*` slices of
/// `CATACOMBS2` stand in it).
pub const POWER_UP_FOLDER: &str = "slices/catacombs/gameplay/powerups/";
/// The pak folder of the spawner slices whose entities are spawn points
/// (`tile_powerup_spawner_master`, `tile4_c_powerup_spawner`).
pub const SPAWNER_FOLDER: &str = "slices/catacombs/gameplay/spawners/";
/// The picture id of no picture: the client's file name table starts with an empty name at
/// index 0 (`FUN_14334d0e0`), and a map byte of 0 is a cell with no tile.
pub const NO_PICTURE: u8 = 0;
/// The bits of a map byte below the picture id: the tile's quarter turns
/// (`tileUiFilenameIdAndRotation`, Observed: `tile4_a` turned twice is 0x86 in `CATACOMBS2`
/// 17.409 s).
const MAP_BYTE_TURN_BITS: u32 = 2;
/// The largest picture id a map byte holds above its turn bits.
pub const MAX_PICTURE_ID: u8 = u8::MAX >> MAP_BYTE_TURN_BITS;
/// The client's first file id: the one after [`NO_PICTURE`].
const FIRST_PICTURE_ID: usize = NO_PICTURE as usize + 1;
/// Separates the paths of a `VariationAssetPaths` cell (`FUN_14334bcb0` splits it at `,` with
/// `FUN_14178ae30`, a stream `getline`: no trimming, an empty entry between two separators kept,
/// none after the last).
const VARIATION_SEPARATOR: char = ',';
/// The key of a row with an empty id: the client refuses it as an invalid entry
/// (`FUN_1447415e0`; the CRC of no text is 0).
const INVALID_TILE_KEY: u32 = 0;
/// Most rows whose map order [`game_mode_indexes`] tells apart: each row's place in one byte.
const MAX_ORDERED_ROWS: usize = u8::MAX as usize + 1;

/// One entity of a tile: its slice, asset id, position component and place in the tile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TilePiece {
    /// The pak path of its slice.
    pub slice: String,
    /// The slice's asset id.
    pub asset: [u8; GUID_LEN],
    /// The replica index of its position component (its member number on the wire).
    pub position_component: u32,
    /// Its place from the tile's centre before the tile's turn: east, north, up (metres).
    pub offset: [f32; 3],
    /// Its yaw from the tile's yaw, degrees counter-clockwise.
    pub yaw_deg: f32,
}

/// A place in a tile: its offset from the tile's centre before the tile's turn and its yaw.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TilePlace {
    /// East, north, up (metres).
    pub offset: [f32; 3],
    /// Degrees counter-clockwise from the tile's yaw.
    pub yaw_deg: f32,
}

/// A slot of a gameplay tile that spawns a plain slice (a gate, a checkpoint, a lever, a chest,
/// a power-up spawner).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TileProp {
    /// The slot entity's name (`GateNorth`, `CheckpointTile`, `PowerUps`).
    pub slot: String,
    /// The slice it spawns, at the slot's place.
    pub piece: TilePiece,
    /// The spawn points of the spawned slice, in the tile's frame: its entities with no spawner
    /// of their own (Observed: `tile_powerup_spawner_master` holds `PowerUp01` to `PowerUp06`, and
    /// `CATACOMBS2` 17.307 to 17.341 s shows six power-up cards at them).
    #[serde(default)]
    pub points: Vec<TilePlace>,
    /// The gatherable and the timelines of the spawned slice, when it has a gatherable (a
    /// door that opens, a lever that is pulled: `catacombs_gate_hidden`,
    /// `catacombs_lever_wall_torch`); `None` for another slice.
    #[serde(default)]
    pub gate: Option<PropGate>,
}

/// The replicated parts of a prop that opens or is pulled ([`TileProp::gate`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropGate {
    /// The component of its `GatherableControllerReplicatedState`.
    pub gatherable_component: u64,
    /// Its timelines, with their lengths.
    pub timelines: Vec<GateTimeline>,
    /// The component of its `InteractReplicatedState`, when it has one (the wall torch lever's
    /// 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interact_component: Option<u64>,
}

/// One encounter a slot may spawn, with its weight: how many entries of the slot's alias name
/// its slice (Observed: `catacombs_gameplay_tile_boss.aliasasset` names its 25 encounter slices
/// 144 times in all; one entry for a slot of one slice).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TileEncounter {
    /// The entries naming it.
    pub weight: u32,
    /// The encounter, in the tile's frame.
    pub encounter: Encounter,
}

/// One tile variation: its two entities, its plain slots and its encounters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatacombTile {
    /// The gameplay tile slice (the variation itself).
    pub gameplay: TilePiece,
    /// The art tile slice its prefab spawner spawns.
    pub art: TilePiece,
    /// Its slots that spawn plain slices.
    #[serde(default)]
    pub props: Vec<TileProp>,
    /// The encounters of its slots in the tile's frame, once per slot and slice; each is named
    /// after its slot, and the entries of one alias slot share a `spawn_group`.
    #[serde(default)]
    pub encounters: Vec<TileEncounter>,
    /// Its map picture id ([`tile_pictures`]); [`NO_PICTURE`] in a file from before this field.
    #[serde(default)]
    pub picture: u8,
}

/// The tiles, by lowercase variation path, and the power-ups a power-up spawner picks from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CatacombTiles {
    /// The tiles by lowercase variation path (`catacombs/gameplay/tiles/tile4_a_gameplay`).
    pub by_variation: BTreeMap<String, CatacombTile>,
    /// The power-up slices ([`POWER_UP_FOLDER`]), each at no offset.
    #[serde(default)]
    pub power_ups: Vec<TilePiece>,
}

/// Why a tile cannot be read, or the file cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum CatacombTilesError {
    /// A slice does not read as a script slice.
    #[error("{slice}: {source}")]
    Slice {
        /// The slice.
        slice: String,
        /// Why.
        source: LevelScriptError,
    },
    /// A slice has no root entity with a position component.
    #[error("{slice}: no root entity with a position component")]
    NoPosition {
        /// The slice.
        slice: String,
    },
    /// A gameplay slice spawns no art tile.
    #[error("{slice}: no prefab spawner of an art tile")]
    NoArtTile {
        /// The slice.
        slice: String,
    },
    /// The game mode has no map of its own: no level to write the tiles' collision for.
    #[error("game mode {game_mode}: no map")]
    NoLevel {
        /// The game mode.
        game_mode: String,
    },
    /// The catalog holds no asset id for a slice.
    #[error("{slice}: not in the asset catalog")]
    NoAsset {
        /// The slice.
        slice: String,
    },
    /// The tile rows are more than [`MAX_ORDERED_ROWS`]: their map order is not told apart.
    #[error("{rows} tile rows: at most {MAX_ORDERED_ROWS} have a known picture order")]
    TooManyTileRows {
        /// The rows.
        rows: usize,
    },
    /// A variation's picture id does not fit a map byte ([`MAX_PICTURE_ID`]).
    #[error("{variation}: picture id {id} is above {MAX_PICTURE_ID}")]
    PictureIdTooLarge {
        /// The variation (lower case).
        variation: String,
        /// Its id.
        id: usize,
    },
    /// A tile variation of the rules has no picture: the rows do not name it.
    #[error("{variation}: no tile row names this variation")]
    NoPicture {
        /// The variation (lower case).
        variation: String,
    },
    /// The file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not catacombs tiles.
    #[error("{path}: {source}")]
    Json {
        /// The file.
        path: PathBuf,
        /// Why.
        source: serde_json::Error,
    },
}

/// The catacombs tile file of `client_build` in `dir`.
pub fn catacomb_tiles_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!(
        "{CATACOMB_TILES_PREFIX}{client_build}{CATACOMB_TILES_EXTENSION}"
    ))
}

/// The pak path of the slice of variation `variation` (`Catacombs/Gameplay/Tiles/tile4_a_gameplay`
/// is `slices/catacombs/gameplay/tiles/tile4_a_gameplay.dynamicslice`, as the client builds it).
pub fn variation_slice(variation: &str) -> String {
    pak_path(&format!("slices/{variation}.dynamicslice"))
}

/// The paths of a `VariationAssetPaths` cell, split as the client splits it
/// ([`VARIATION_SEPARATOR`]).
fn variation_paths(cell: &str) -> Vec<&str> {
    let mut paths: Vec<&str> = cell.split(VARIATION_SEPARATOR).collect();
    if paths.last().is_some_and(|last| last.is_empty()) {
        paths.pop();
    }
    paths
}

/// The map picture id of every tile variation (lower case) of the `DungeonTileStaticData`
/// `rows`, each its `DungeonTileId` and its `VariationAssetPaths` cell, in datasheet order and
/// of every feature (the client's table also holds the `UnitTesting_Catacombs` rows).
///
/// Observed (client build 25810808, `local-data/findings/2026-11-13-catacomb-tile-pictures.md`):
/// `FUN_1447415e0` puts the rows into a hash map under the lower-case CRC of their id, skipping
/// an empty id and a repeated one (insert `FUN_1446a3aa0`, rehash `FUN_1446bd430`, one bucket at
/// the start, maximum load 1, a power of two buckets); `FUN_144777b70` copies the map, which
/// lists its buckets from the last to the first, each bucket's rows in datasheet order (the
/// order of [`game_mode_indexes`]). `FUN_14334d0e0` walks that copy and gives every path of a
/// row the next file id from [`FIRST_PICTURE_ID`]; a path that two rows name keeps the later
/// id (`FUN_143349fd0` keeps the first entry of an asset id, and the walk then sets its file
/// id). With the retail rows this gives all 25 picture ids of the ten retail catacomb ledgers
/// (`tile4_d` 38 and `tile5_d` 10, which no retail run showed).
///
/// # Errors
///
/// Returns [`CatacombTilesError`] when the rows are more than [`MAX_ORDERED_ROWS`] or a
/// picture id does not fit a map byte.
pub fn tile_pictures<'a>(
    rows: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<BTreeMap<String, u8>, CatacombTilesError> {
    let mut keys = HashSet::new();
    let mut rows: Vec<(&str, &str)> = rows
        .into_iter()
        .filter(|(id, _)| {
            let key = lowercase_crc32(id);
            key != INVALID_TILE_KEY && keys.insert(key)
        })
        .collect();
    if rows.len() > MAX_ORDERED_ROWS {
        return Err(CatacombTilesError::TooManyTileRows { rows: rows.len() });
    }
    let places = game_mode_indexes(rows.iter().map(|(id, _)| *id));
    rows.sort_by_key(|(id, _)| places.get(id).copied());
    let mut ids = BTreeMap::new();
    let paths = rows.iter().flat_map(|(_, cell)| variation_paths(cell));
    for (id, path) in (FIRST_PICTURE_ID..).zip(paths) {
        ids.insert(path.to_lowercase(), id);
    }
    ids.into_iter()
        .map(|(variation, id)| match u8::try_from(id) {
            Ok(picture) if picture <= MAX_PICTURE_ID => Ok((variation, picture)),
            _ => Err(CatacombTilesError::PictureIdTooLarge { variation, id }),
        })
        .collect()
}

/// The replica index of the position component of the root entity of `script`.
pub fn root_position(script: &SliceScript) -> Option<u32> {
    script
        .entities
        .iter()
        .filter(|entity| entity.parent.is_none())
        .flat_map(|entity| &entity.replicated)
        .find(|component| component.kind == ReplicatedKind::Position)
        .map(|component| component.index)
}

/// A piece at `place` in its tile.
fn piece(
    slice: &str,
    asset: [u8; GUID_LEN],
    position_component: u32,
    place: &Transform,
) -> TilePiece {
    // A tile is 64 m across: its places fit an f32.
    let [x, y, z] = place.translation.map(|value| value as f32);
    TilePiece {
        slice: slice.to_owned(),
        asset,
        position_component,
        offset: [x, y, z],
        yaw_deg: place.yaw_degrees() as f32,
    }
}

impl CatacombTile {
    /// The tile of the gameplay slice `slice` (pak path) with ObjectStream root `root`;
    /// `root_of` gives the root of a slice the tile spawns by its pak path, `asset_of` a slice's
    /// asset id, `length_of` a timeline asset's length in seconds by its pak path. Its
    /// encounters are left empty and its picture is [`NO_PICTURE`]: the content builder sets
    /// them.
    ///
    /// # Errors
    ///
    /// Returns [`CatacombTilesError`] when a slice does not read, has no positioned root, the
    /// gameplay slice spawns no art tile, or an asset id is missing.
    pub fn of(
        slice: &str,
        root: &Element,
        root_of: impl Fn(&str) -> Option<Element>,
        asset_of: impl Fn(&str) -> Option<[u8; GUID_LEN]>,
        length_of: impl Fn(&str) -> f32,
    ) -> Result<Self, CatacombTilesError> {
        let script = |slice: &str, root: &Element| {
            slice_script(root).map_err(|source| CatacombTilesError::Slice {
                slice: slice.to_owned(),
                source,
            })
        };
        let position = |slice: &str, script: &SliceScript| {
            root_position(script).ok_or_else(|| CatacombTilesError::NoPosition {
                slice: slice.to_owned(),
            })
        };
        let asset = |slice: &str| {
            asset_of(slice).ok_or_else(|| CatacombTilesError::NoAsset {
                slice: slice.to_owned(),
            })
        };
        let gameplay = script(slice, root)?;
        let (spawner, art_slice) = gameplay
            .entities
            .iter()
            .find_map(|entity| {
                let spawned = entity.spawner.as_ref()?.slice.as_ref()?;
                spawned
                    .path
                    .starts_with(ART_TILE_FOLDER)
                    .then(|| (entity.id, spawned.path.clone()))
            })
            .ok_or_else(|| CatacombTilesError::NoArtTile {
                slice: slice.to_owned(),
            })?;
        let art_root = root_of(&art_slice).ok_or_else(|| CatacombTilesError::NoArtTile {
            slice: slice.to_owned(),
        })?;
        let art_script = script(&art_slice, &art_root)?;
        let mut props = Vec::new();
        for entity in &gameplay.entities {
            let Some(spawned) = entity
                .spawner
                .as_ref()
                .and_then(|spawner| spawner.slice.as_ref())
            else {
                continue;
            };
            if spawned.path.starts_with(ART_TILE_FOLDER) {
                continue;
            }
            let Some(spawned_root) = root_of(&spawned.path) else {
                continue;
            };
            let spawned_script = script(&spawned.path, &spawned_root)?;
            if spawned_script.manager().is_some() {
                continue;
            }
            let place = gameplay.placement_of(entity.id);
            props.push(TileProp {
                slot: entity.name.clone(),
                piece: piece(
                    &spawned.path,
                    asset(&spawned.path)?,
                    position(&spawned.path, &spawned_script)?,
                    &place,
                ),
                points: if spawned.path.starts_with(SPAWNER_FOLDER) {
                    spawn_points(&spawned_script)
                        .map(|point| tile_place(&place.then(&spawned_script.placement_of(point))))
                        .collect()
                } else {
                    Vec::new()
                },
                gate: spawned_script
                    .gatherable_component()
                    .map(|gatherable_component| PropGate {
                        gatherable_component,
                        timelines: spawned_script.timelines(&length_of),
                        interact_component: spawned_script.interact_component(),
                    }),
            });
        }
        Ok(Self {
            gameplay: piece(
                slice,
                asset(slice)?,
                position(slice, &gameplay)?,
                &Transform::default(),
            ),
            art: piece(
                &art_slice,
                asset(&art_slice)?,
                position(&art_slice, &art_script)?,
                &gameplay.placement_of(spawner),
            ),
            props,
            encounters: Vec::new(),
            picture: NO_PICTURE,
        })
    }
}

/// The spawn points of a spawner slice ([`SPAWNER_FOLDER`]): its leaf entities below the root
/// with no spawner, encounter or stage of their own (module text).
fn spawn_points(script: &SliceScript) -> impl Iterator<Item = u64> + '_ {
    script
        .entities
        .iter()
        .filter(|entity| {
            entity.parent.is_some()
                && entity.spawner.is_none()
                && entity.manager.is_none()
                && entity.stage.is_none()
                && !script
                    .entities
                    .iter()
                    .any(|child| child.parent == Some(entity.id))
        })
        .map(|entity| entity.id)
}

/// `place` as a place in the tile.
fn tile_place(place: &Transform) -> TilePlace {
    // A tile is 64 m across: its places fit an f32.
    let [x, y, z] = place.translation.map(|value| value as f32);
    TilePlace {
        offset: [x, y, z],
        yaw_deg: place.yaw_degrees() as f32,
    }
}

/// The world place of `offset` (tile frame) in a tile whose centre stands at `centre`, turned
/// by `yaw_deg` counter-clockwise.
pub fn turned_point(offset: [f32; 3], centre: [f32; 3], yaw_deg: f32) -> [f32; 3] {
    let (sin, cos) = yaw_deg.to_radians().sin_cos();
    let [x, y, z] = offset;
    [
        centre[0] + x * cos - y * sin,
        centre[1] + x * sin + y * cos,
        centre[2] + z,
    ]
}

/// `place` (a tile-frame collision place) in a tile at `centre` turned by `yaw_deg`.
fn turned_affine(place: &Affine, centre: [f32; 3], yaw_deg: f32) -> Affine {
    let (sin, cos) = yaw_deg.to_radians().sin_cos();
    let turn = [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]];
    let mut out = [[0.0; 4]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (column, value) in out_row.iter_mut().enumerate() {
            *value = (0..3).map(|k| turn[row][k] * place[k][column]).sum();
        }
        out_row[3] += centre[row];
    }
    out
}

/// `encounter` (in a tile's frame) in the tile at `centre` turned by `yaw_deg`: its place, its
/// detector, its spawn locations and its targets' patrol paths turn with the tile.
pub fn placed_encounter(encounter: &Encounter, centre: [f32; 3], yaw_deg: f32) -> Encounter {
    let point = |offset: [f32; 3]| turned_point(offset, centre, yaw_deg);
    let mut placed = encounter.clone();
    placed.position = point(encounter.position);
    placed.yaw_deg = encounter.yaw_deg + yaw_deg;
    if let Some(detector) = placed.detector.as_mut() {
        detector.place = turned_affine(&detector.place, centre, yaw_deg);
    }
    for spawn in placed
        .stages
        .iter_mut()
        .flat_map(|stage| stage.spawns.iter_mut())
    {
        for location in &mut spawn.locations {
            *location = point(*location);
        }
    }
    for path in placed
        .targets
        .iter_mut()
        .flat_map(|target| target.slices.iter_mut())
        .filter_map(|slice| slice.waypoints.as_mut())
    {
        for waypoint in &mut path.points {
            *waypoint = point(*waypoint);
        }
    }
    placed
}

/// The solids of the art tiles (the floors and walls a player stands on and runs into), from the
/// collision file of the catacombs level (`content-tool catacomb-tiles` writes it: each art tile
/// slice a deferred slice at the origin of its tile's frame, [`crate::collision::DeferredSlice`]),
/// with the shapes of that file's index.
#[derive(Debug, Clone, Default)]
pub struct CatacombSolids {
    /// The level collision's index, whose shapes the bodies use.
    shapes: Option<Arc<CollisionIndex>>,
    /// The bodies of each art tile slice in its tile's frame, by asset id.
    by_art: BTreeMap<[u8; GUID_LEN], Vec<Body>>,
}

/// Two are equal when their solids are: the level index only lends them its shapes.
impl PartialEq for CatacombSolids {
    fn eq(&self, other: &Self) -> bool {
        self.by_art == other.by_art
    }
}

/// The solids of one run's tiles, placed in the world, and their index.
#[derive(Debug, Clone)]
pub struct RunSolids {
    /// The placed bodies.
    bodies: Vec<Body>,
    /// Their index over the level collision's shapes.
    index: Arc<CollisionIndex>,
}

/// Two are equal when their bodies are.
impl PartialEq for RunSolids {
    fn eq(&self, other: &Self) -> bool {
        self.bodies == other.bodies
    }
}

impl RunSolids {
    /// The index of the placed solids.
    pub const fn index(&self) -> &Arc<CollisionIndex> {
        &self.index
    }

    /// The placed bodies.
    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }
}

impl CatacombSolids {
    /// The solids of the art tiles of `tiles` in `collision`, whose index is `shapes`: the
    /// deferred slice of each art tile's asset id at the origin.
    pub fn of(
        collision: &LevelCollision,
        shapes: &Arc<CollisionIndex>,
        tiles: &CatacombTiles,
    ) -> Self {
        let by_art = tiles
            .by_variation
            .values()
            .filter_map(|tile| {
                let art = tile.art.asset;
                collision
                    .deferred
                    .iter()
                    .find(|deferred| deferred.slice == art && deferred.origin == [0.0; 3])
                    .map(|deferred| (art, deferred.bodies.clone()))
            })
            .collect();
        Self {
            shapes: Some(Arc::clone(shapes)),
            by_art,
        }
    }

    /// The art tiles that have solids.
    pub fn art_tiles(&self) -> usize {
        self.by_art.len()
    }

    /// The solids of the tiles `placed` (each an art tile's asset id, its centre and its yaw in
    /// degrees counter-clockwise), turned and moved into the world; `None` when no tile has
    /// solids or no collision file was read.
    pub fn placed(
        &self,
        placed: impl IntoIterator<Item = ([u8; GUID_LEN], [f32; 3], f32)>,
    ) -> Option<RunSolids> {
        let shapes = self.shapes.as_ref()?;
        let bodies: Vec<Body> = placed
            .into_iter()
            .filter_map(|(art, centre, yaw_deg)| {
                self.by_art.get(&art).map(|bodies| {
                    bodies
                        .iter()
                        .map(move |body| Body {
                            geometry: body.geometry,
                            place: turned_affine(&body.place, centre, yaw_deg),
                        })
                        .collect::<Vec<_>>()
                })
            })
            .flatten()
            .collect();
        if bodies.is_empty() {
            return None;
        }
        let index = Arc::new(shapes.with_bodies(bodies.clone()));
        Some(RunSolids { bodies, index })
    }
}

impl CatacombTiles {
    /// Reads `path`; a missing file has no tiles (content from before this file).
    ///
    /// # Errors
    ///
    /// Returns [`CatacombTilesError`] when the file cannot be read or is not catacombs tiles.
    pub fn load(path: &Path) -> Result<Self, CatacombTilesError> {
        match std::fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|source| CatacombTilesError::Json {
                    path: path.to_path_buf(),
                    source,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(CatacombTilesError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Writes the file to `path`.
    ///
    /// # Errors
    ///
    /// Returns [`CatacombTilesError`] when the file cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), CatacombTilesError> {
        let text =
            serde_json::to_string_pretty(self).map_err(|source| CatacombTilesError::Json {
                path: path.to_path_buf(),
                source,
            })?;
        std::fs::write(path, text).map_err(|source| CatacombTilesError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// The tile of variation `variation` (any case).
    pub fn get(&self, variation: &str) -> Option<&CatacombTile> {
        self.by_variation.get(&variation.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::{DeferredSlice, Geometry, IDENTITY_AFFINE};

    /// An art tile's floor slab (a box 1 m thick whose top is 0.5 m above its place, 10 m east of
    /// the tile's centre) stands in the run where its tile stands, turned with it; a tile of
    /// no solids adds none.
    #[test]
    fn a_run_s_solids_are_its_art_tiles_solids_turned_and_moved_into_the_world() {
        const CENTRE: [f32; 3] = [192.0, 768.0, 120.0];
        let mut slab = IDENTITY_AFFINE;
        slab[0][3] = 10.0;
        let collision = LevelCollision {
            level: "nw_catacomb_00".to_owned(),
            geometries: vec![Geometry::Box {
                half_extents: [2.0, 2.0, 0.5],
            }],
            deferred: vec![DeferredSlice {
                slice: [7; GUID_LEN],
                sub_id: 2,
                origin: [0.0; 3],
                bodies: vec![Body {
                    geometry: 0,
                    place: slab,
                }],
            }],
            ..LevelCollision::default()
        };
        let shapes = Arc::new(CollisionIndex::new(LevelCollision {
            deferred: Vec::new(),
            ..collision.clone()
        }));
        let tile = CatacombTile {
            gameplay: piece_at("slices/catacombs/gameplay/tiles/tile1_a_gameplay"),
            art: piece_at("slices/catacombs/arttiles/tile1_a"),
            props: Vec::new(),
            encounters: Vec::new(),
            picture: NO_PICTURE,
        };
        let tiles = CatacombTiles {
            by_variation: BTreeMap::from([("tile1_a_gameplay".to_owned(), tile)]),
            power_ups: Vec::new(),
        };
        let solids = CatacombSolids::of(&collision, &shapes, &tiles);
        assert_eq!(solids.art_tiles(), 1);
        let run = solids
            .placed([([7; GUID_LEN], CENTRE, 90.0), ([9; GUID_LEN], CENTRE, 0.0)])
            .expect("solids");
        assert_eq!(run.bodies().len(), 1, "the tile of no solids adds none");
        let place = run.bodies()[0].place;
        assert!(
            (place[0][3] - CENTRE[0]).abs() < 1e-3,
            "east turns to north"
        );
        assert!((place[1][3] - (CENTRE[1] + 10.0)).abs() < 1e-3);
        let floor = run
            .index()
            .floor_at(CENTRE[0], CENTRE[1] + 10.0, CENTRE[2] + 1.0)
            .expect("the slab is a floor");
        assert!((floor - (CENTRE[2] + 0.5)).abs() < 1e-3, "floor {floor}");
        assert!(solids.placed([([9; GUID_LEN], CENTRE, 0.0)]).is_none());
    }

    fn piece_at(slice: &str) -> TilePiece {
        TilePiece {
            slice: slice.to_owned(),
            asset: [7; GUID_LEN],
            position_component: 1,
            offset: [0.0; 3],
            yaw_deg: 0.0,
        }
    }

    /// The tile kinds of the retail `javelindata_dungeontiles` rows, by tile number.
    const TILE_KINDS: [&str; 5] = ["Straight", "Corner", "ThreeWay", "DeadEnd", "FourWay"];
    /// The variations of a retail hallway row, in `VariationAssetPaths` order.
    const HALLWAY_VARIANTS: [&str; 4] = ["b", "c", "ba", "ca"];
    /// The id prefix of the retail `UnitTesting_Catacombs` rows.
    const TEST_ROW_PREFIX: &str = "UT";

    /// The retail `DungeonTileStaticData` rows in datasheet order (client build 25810808): the
    /// five rooms, the five hallways and the five parkour hallways of `Catacombs`, then the same
    /// fifteen rows with the `UT` prefix (`UnitTesting_Catacombs`, the same paths).
    fn retail_rows() -> Vec<(String, String)> {
        let path = |number: usize, variant: &str| {
            format!("Catacombs/Gameplay/Tiles/tile{number}_{variant}_gameplay")
        };
        let mut feature = Vec::new();
        for (index, kind) in TILE_KINDS.iter().enumerate() {
            feature.push((format!("{kind}Room"), path(index + 1, "a")));
        }
        for (index, kind) in TILE_KINDS.iter().enumerate() {
            let cell: Vec<String> = HALLWAY_VARIANTS
                .iter()
                .map(|variant| path(index + 1, variant))
                .collect();
            feature.push((format!("{kind}Hallway"), cell.join(",")));
        }
        for (index, kind) in TILE_KINDS.iter().enumerate() {
            feature.push((format!("{kind}Hallway_Parkour"), path(index + 1, "d")));
        }
        let tests: Vec<(String, String)> = feature
            .iter()
            .map(|(id, cell)| (format!("{TEST_ROW_PREFIX}{id}"), cell.clone()))
            .collect();
        feature.into_iter().chain(tests).collect()
    }

    /// The client's id table of the retail rows gives every picture id seen in the ten retail
    /// catacomb ledgers (25 variations; `tile4_b`, `tile4_c` and `tile4_ba` were never seen
    /// and fit the hallway rule) and the two tiles no retail run showed: `tile4_d` 38 and
    /// `tile5_d` 10.
    #[test]
    fn the_retail_rows_give_the_retail_picture_ids() {
        const RETAIL_IDS: [(&str, u8); 30] = [
            ("tile1_a", 47),
            ("tile1_b", 56),
            ("tile1_c", 57),
            ("tile1_ba", 58),
            ("tile1_ca", 59),
            ("tile1_d", 31),
            ("tile2_a", 13),
            ("tile2_b", 43),
            ("tile2_c", 44),
            ("tile2_ba", 45),
            ("tile2_ca", 46),
            ("tile2_d", 32),
            ("tile3_a", 14),
            ("tile3_b", 52),
            ("tile3_c", 53),
            ("tile3_ba", 54),
            ("tile3_ca", 55),
            ("tile3_d", 60),
            ("tile4_a", 33),
            ("tile4_b", 34),
            ("tile4_c", 35),
            ("tile4_ba", 36),
            ("tile4_ca", 37),
            ("tile4_d", 38),
            ("tile5_a", 29),
            ("tile5_b", 24),
            ("tile5_c", 25),
            ("tile5_ba", 26),
            ("tile5_ca", 27),
            ("tile5_d", 10),
        ];
        let rows = retail_rows();
        let pictures =
            tile_pictures(rows.iter().map(|(id, cell)| (id.as_str(), cell.as_str())))
                .expect("pictures");
        assert_eq!(pictures.len(), RETAIL_IDS.len());
        for (tile, id) in RETAIL_IDS {
            let variation = format!("catacombs/gameplay/tiles/{tile}_gameplay");
            assert_eq!(pictures.get(&variation), Some(&id), "{tile}");
        }
    }

    /// A row with an empty id or an id already held (any case) adds no file; a path two rows
    /// name keeps the later id; an empty entry between two separators takes an id, a separator
    /// at the end does not.
    #[test]
    fn refused_rows_add_no_file_and_a_shared_path_keeps_its_later_id() {
        let one_row = |cell: &'static str| {
            tile_pictures([("Room", cell)]).expect("pictures")
        };
        assert_eq!(
            one_row("A,B,"),
            BTreeMap::from([("a".to_owned(), 1), ("b".to_owned(), 2)])
        );
        assert_eq!(
            one_row("A,,B"),
            BTreeMap::from([
                ("a".to_owned(), 1),
                (String::new(), 2),
                ("b".to_owned(), 3)
            ])
        );
        let with_refused = tile_pictures([("Room", "A"), ("", "B"), ("ROOM", "C")])
            .expect("pictures");
        assert_eq!(with_refused, BTreeMap::from([("a".to_owned(), 1)]));
        // `room` and `hall` share bucket 0 of 2: the walk keeps their datasheet order.
        let shared = tile_pictures([("Room", "A,B"), ("Hall", "a")]).expect("pictures");
        assert_eq!(
            shared,
            BTreeMap::from([("a".to_owned(), 3), ("b".to_owned(), 2)])
        );
    }

    /// The walk lists the map's buckets from the last to the first: `second` (bucket 1 of 2)
    /// comes before `first` (bucket 0), against the datasheet order.
    #[test]
    fn the_walk_lists_the_last_bucket_first() {
        let pictures = tile_pictures([("First", "X"), ("Second", "Y")]).expect("pictures");
        assert_eq!(
            pictures,
            BTreeMap::from([("x".to_owned(), 2), ("y".to_owned(), 1)])
        );
    }

    /// An id above [`MAX_PICTURE_ID`] does not fit a map byte: the table refuses it.
    #[test]
    fn a_picture_id_past_the_map_byte_is_refused() {
        let paths: Vec<String> = (0..=MAX_PICTURE_ID).map(|path| path.to_string()).collect();
        let cell = paths.join(",");
        assert!(matches!(
            tile_pictures([("Room", cell.as_str())]),
            Err(CatacombTilesError::PictureIdTooLarge { id, .. })
                if id == usize::from(MAX_PICTURE_ID) + 1
        ));
        let fits = paths[..usize::from(MAX_PICTURE_ID)].join(",");
        assert_eq!(
            tile_pictures([("Room", fits.as_str())])
                .expect("pictures")
                .values()
                .max(),
            Some(&MAX_PICTURE_ID)
        );
    }

    /// A tile of a file from before the picture field reads with no picture.
    #[test]
    fn a_tile_without_a_picture_field_has_no_picture() {
        let tile = CatacombTile {
            gameplay: piece_at("slices/catacombs/gameplay/tiles/tile4_d_gameplay"),
            art: piece_at("slices/catacombs/arttiles/tile4_d"),
            props: Vec::new(),
            encounters: Vec::new(),
            picture: 38,
        };
        let mut json = serde_json::to_value(&tile).expect("json");
        json.as_object_mut().expect("object").remove("picture");
        let read: CatacombTile = serde_json::from_value(json).expect("old tile");
        assert_eq!(read.picture, NO_PICTURE);
        assert_eq!(read.gameplay, tile.gameplay);
    }

    #[test]
    fn a_variation_names_the_slice_the_client_loads() {
        assert_eq!(
            variation_slice("Catacombs/Gameplay/Tiles/tile4_a_gameplay"),
            "slices/catacombs/gameplay/tiles/tile4_a_gameplay.dynamicslice"
        );
    }

    #[test]
    fn the_table_finds_a_tile_in_any_case_and_round_trips() {
        let dir = tempfile::tempdir().expect("dir");
        let path = catacomb_tiles_path(dir.path(), "25810808");
        assert!(path.ends_with("catacomb-tiles-25810808.json"));
        assert_eq!(
            CatacombTiles::load(&path).expect("missing file"),
            CatacombTiles::default()
        );
        let tiles = CatacombTiles {
            by_variation: [(
                "catacombs/gameplay/tiles/tile4_a_gameplay".to_owned(),
                CatacombTile {
                    gameplay: piece_at("slices/catacombs/gameplay/tiles/tile4_a_gameplay.dynamicslice"),
                    art: piece_at("slices/catacombs/arttiles/tile4_a.dynamicslice"),
                    props: vec![TileProp {
                        slot: "PowerUps".to_owned(),
                        piece: piece_at(
                            "slices/catacombs/gameplay/spawners/tile_powerup_spawner_master.dynamicslice",
                        ),
                        points: vec![TilePlace {
                            offset: [-6.0, 0.0, 0.0],
                            yaw_deg: 0.0,
                        }],
                        gate: None,
                    }],
                    encounters: vec![TileEncounter {
                        weight: 1,
                        encounter: Encounter {
                            name: "EncounterKey".to_owned(),
                            ..Encounter::default()
                        },
                    }],
                    picture: 33,
                },
            )]
            .into_iter()
            .collect(),
            power_ups: vec![piece_at(
                "slices/catacombs/gameplay/powerups/catacombs_powerup_healbomb.dynamicslice",
            )],
        };
        tiles.write(&path).expect("write");
        let read = CatacombTiles::load(&path).expect("read");
        assert_eq!(read, tiles);
        assert!(
            read.get("Catacombs/Gameplay/Tiles/tile4_a_gameplay")
                .is_some()
        );
        assert!(
            read.get("Catacombs/Gameplay/Tiles/tile5_a_gameplay")
                .is_none()
        );
    }

    #[test]
    fn a_point_and_an_encounter_turn_with_their_tile() {
        let centre = [128.0, 768.0, 120.0];
        let turned = turned_point([-12.0, 0.0, -2.0], centre, 180.0);
        assert!((turned[0] - 140.0).abs() < 1e-3, "{turned:?}");
        assert!((turned[1] - 768.0).abs() < 1e-3);
        assert!((turned[2] - 118.0).abs() < 1e-3);
        let encounter = Encounter {
            name: "EncounterKey".to_owned(),
            position: [2.0, 0.0, -2.0],
            stages: vec![crate::dungeon_scripts::Stage {
                spawns: vec![crate::dungeon_scripts::Spawn {
                    locations: vec![[0.0, 3.0, -2.0]],
                    ..crate::dungeon_scripts::Spawn::default()
                }],
                ..crate::dungeon_scripts::Stage::default()
            }],
            detector: Some(crate::dungeon_scripts::Detector {
                shape: crate::collision::Geometry::Sphere { radius: 5.0 },
                place: crate::collision::IDENTITY_AFFINE,
            }),
            ..Encounter::default()
        };
        let placed = placed_encounter(&encounter, centre, 90.0);
        assert!(
            (placed.position[0] - 128.0).abs() < 1e-3,
            "2 m east turns to north"
        );
        assert!((placed.position[1] - 770.0).abs() < 1e-3);
        let location = placed.stages[0].spawns[0].locations[0];
        assert!(
            (location[0] - 125.0).abs() < 1e-3,
            "3 m north turns to west: {location:?}"
        );
        let detector = placed.detector.expect("detector");
        assert!(detector.contains([128.0, 768.0, 120.0]));
        assert!(!detector.contains([0.0, 0.0, 120.0]));
        assert_eq!(placed.yaw_deg, 90.0);
    }
}
