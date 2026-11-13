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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nw_formats::objectstream::Element;
use serde::{Deserialize, Serialize};

use crate::collision::Affine;
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
    /// The catalog holds no asset id for a slice.
    #[error("{slice}: not in the asset catalog")]
    NoAsset {
        /// The slice.
        slice: String,
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
    /// asset id. Its encounters are left empty: the content builder fills them.
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

    fn piece_at(slice: &str) -> TilePiece {
        TilePiece {
            slice: slice.to_owned(),
            asset: [7; GUID_LEN],
            position_component: 1,
            offset: [0.0; 3],
            yaw_deg: 0.0,
        }
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
                    }],
                    encounters: vec![TileEncounter {
                        weight: 1,
                        encounter: Encounter {
                            name: "EncounterKey".to_owned(),
                            ..Encounter::default()
                        },
                    }],
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
