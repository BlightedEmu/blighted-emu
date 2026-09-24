// SPDX-License-Identifier: AGPL-3.0-or-later
//! `catacomb-check`: makes the layouts of many runs of a generated game mode
//! (`nw_content::catacombs`) and checks them against the game install: every tile variation must
//! name a slice the paks hold (the client logs `DungeonTileStaticData invalid` for a row without
//! one, `FUN_14334bcb0`), and every run must lay out from the map's team teleport spot.

use std::collections::BTreeSet;

use nw_content::Tables;
use nw_content::catacombs::{CatacombError, CatacombLayout, CatacombRules, cell_of};
use nw_content::instances::InstanceCatalog;

/// The game mode checked when none is named.
pub const DEFAULT_GAME_MODE: &str = "Catacombs";
/// Runs made when no count is given.
pub const DEFAULT_RUNS: u64 = 100;
/// The map character of a cell without a tile.
const EMPTY_CELL: char = '.';
/// The map character of a connector hallway.
const CONNECTOR_CELL: char = '+';
/// The map character of the start room.
const START_CELL: char = 'S';
/// The map character of a boss room.
const BOSS_CELL: char = 'B';
/// The room ids drawn with their own character.
const START_ROOM: &str = "StartRoom";
const BOSS_ROOM: &str = "BossRoom";

/// Why the check cannot run.
#[derive(Debug, thiserror::Error)]
pub enum CatacombCheckError {
    /// The rules do not read or a run has no layout.
    #[error("catacombs: {0}")]
    Rules(#[from] CatacombError),
    /// The game mode has no map with a team teleport spot.
    #[error("game mode {game_mode} has no instance map")]
    NoMap {
        /// `GameModeID`.
        game_mode: String,
    },
}

/// What the check found.
#[derive(Debug, Clone, PartialEq)]
pub struct CatacombCheck {
    /// The game mode.
    pub game_mode: String,
    /// The start room's cell.
    pub start_cell: [i32; 2],
    /// Runs made.
    pub runs: u64,
    /// Fewest and most tiles of a run.
    pub tiles: (usize, usize),
    /// Fewest and most connector hallways of a run.
    pub connectors: (usize, usize),
    /// Variation slices the paks do not hold.
    pub missing_slices: Vec<String>,
    /// The layout of the first run (seed 0).
    pub first: CatacombLayout,
}

/// Makes runs `0..runs` of `game_mode` from `tables` and looks up each variation slice with
/// `in_paks`.
///
/// # Errors
///
/// Returns [`CatacombCheckError`] when the rules do not read, the mode has no map, or a run has
/// no layout.
pub fn check(
    tables: &Tables,
    game_mode: &str,
    runs: u64,
    in_paks: impl Fn(&BTreeSet<String>) -> BTreeSet<String>,
) -> Result<CatacombCheck, CatacombCheckError> {
    let rules = CatacombRules::for_game_mode(tables, game_mode)?;
    let catalog = InstanceCatalog::from_tables(tables);
    let teleport = catalog
        .by_id(game_mode)
        .and_then(|mode| mode.map.teleports.first())
        .ok_or_else(|| CatacombCheckError::NoMap {
            game_mode: game_mode.to_owned(),
        })?;
    let start_cell = cell_of(teleport.position, rules.tile_size_m);
    let first = rules.generate(0, start_cell)?;
    let mut tiles = (usize::MAX, 0);
    let mut connectors = (usize::MAX, 0);
    for seed in 0..runs.max(1) {
        let layout = rules.generate(seed, start_cell)?;
        let count = layout.rooms.len();
        let connector_count = layout.rooms.iter().filter(|room| room.connector).count();
        tiles = (tiles.0.min(count), tiles.1.max(count));
        connectors = (
            connectors.0.min(connector_count),
            connectors.1.max(connector_count),
        );
    }
    let wanted = rules.variation_slices();
    let present = in_paks(&wanted);
    Ok(CatacombCheck {
        game_mode: game_mode.to_owned(),
        start_cell,
        runs: runs.max(1),
        tiles,
        connectors,
        missing_slices: wanted.difference(&present).cloned().collect(),
        first,
    })
}

/// The map of `layout`, north row first: `S` the start, `B` a boss, `+` a connector, `.` no
/// tile, else the first letter of the room id in lower case.
pub fn map_rows(layout: &CatacombLayout) -> Vec<String> {
    let cells = || layout.rooms.iter().map(|room| room.cell);
    let (Some(min_x), Some(max_x), Some(min_y), Some(max_y)) = (
        cells().map(|cell| cell[0]).min(),
        cells().map(|cell| cell[0]).max(),
        cells().map(|cell| cell[1]).min(),
        cells().map(|cell| cell[1]).max(),
    ) else {
        return Vec::new();
    };
    (min_y..=max_y)
        .rev()
        .map(|y| {
            (min_x..=max_x)
                .map(|x| match layout.at([x, y]) {
                    None => EMPTY_CELL,
                    Some(room) if room.connector => CONNECTOR_CELL,
                    Some(room) if room.room_id == START_ROOM => START_CELL,
                    Some(room) if room.room_id == BOSS_ROOM => BOSS_CELL,
                    Some(room) => room
                        .room_id
                        .chars()
                        .next()
                        .map_or(EMPTY_CELL, |letter| letter.to_ascii_lowercase()),
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_content::catacombs::PlacedRoom;

    fn room(room_id: &str, cell: [i32; 2], connector: bool) -> PlacedRoom {
        PlacedRoom {
            room_id: room_id.to_owned(),
            room_type: String::new(),
            connector,
            locked: false,
            lock_link: None,
            cell,
            sides: 0,
            tile_id: String::new(),
            variation: String::new(),
            turns: 0,
            position: [0.0; 3],
            yaw_deg: 0.0,
        }
    }

    #[test]
    fn the_map_draws_rooms_north_row_first() {
        let layout = CatacombLayout {
            cluster: "CatacombsBranch".to_owned(),
            theme: None,
            rooms: vec![
                room(START_ROOM, [8, 12], false),
                room("HallwayRoom", [8, 13], true),
                room(BOSS_ROOM, [9, 13], false),
                room("NamedRoom", [9, 12], false),
            ],
            tile_size_m: 64.0,
        };
        assert_eq!(map_rows(&layout), ["+B", "Sn"]);
        let empty = CatacombLayout {
            rooms: Vec::new(),
            ..layout
        };
        assert!(map_rows(&empty).is_empty());
    }

    #[test]
    fn a_mode_without_rules_is_refused() {
        let error = check(&Tables::default(), DEFAULT_GAME_MODE, 1, |_| BTreeSet::new())
            .expect_err("no rows");
        assert!(matches!(
            error,
            CatacombCheckError::Rules(CatacombError::NotGenerated { .. })
        ));
    }
}
