// SPDX-License-Identifier: AGPL-3.0-or-later
//! The catacombs: a level made per run (`GameModeData` `LevelGeneratorType` `Cyclic`,
//! `LevelGeneratorFeatureId` `Catacombs`) from the procedural generation tables
//! (`javelindata_dungeonclusters`, `_dungeongrammars`, `_dungeonrooms`, `_dungeontiles`; run 15
//! dungeon study, `local-data/scratch-work/run15/dungeon2/REPORT-catacombs.md`).
//!
//! What the client shows (Observed, 10 retail catacomb ledgers): every tile of the run is an
//! ordinary replicated slice entity, a spawn card naming the tile's variation slice with a
//! position on a grid of `TileSize` metres (all at 120 m height) and a yaw of a whole number of
//! quarter turns. The client has no generator (the decompile holds only the table readers:
//! `FUN_143337a60` clusters, `FUN_14334bcb0` tiles, `FUN_14334d0e0` the map's tile pictures), so
//! the layout is the server's. The rule of retail's generator is not in the client; this module
//! builds a layout from the same tables:
//!
//! 1. A cluster's `SeedGraph` is the start graph ([`SeedGraph`] has the text form).
//! 2. Each node whose symbol is no room is replaced by the graph of a grammar row of that symbol
//!    (weighted draw, rows of the run's theme), at most `ReplacementCount` passes (the client
//!    caps it at [`MAX_REPLACEMENT_COUNT`]).
//! 3. The rooms go on the tile grid from the start room's cell (the map's team teleport spot),
//!    each next to the room it hangs from; a link between rooms that are not neighbours is a path
//!    of connector hallways (the grammar row `Connector`).
//! 4. Each cell takes a tile whose `SupportedRoomTypes` has the room's type and whose
//!    `Connections`, turned by a whole number of quarter turns up to `Rotations`, are the cell's
//!    open sides.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::Tables;
use crate::generated::{
    DungeonClusterStaticData, DungeonGrammarStaticData, DungeonRoomStaticData,
    DungeonTileStaticData,
};

/// Separates the nodes of one path of a seed graph (`StartRoom_1-CycleB_1`).
const NODE_SEPARATOR: char = '-';
/// Separates the items of one chain (`HallwayRoom_1-NamedRoom_1,LoopA_1`).
const ITEM_SEPARATOR: char = ',';
/// Separates the chains of a seed graph (`T-K-P-L;L-T`).
const CHAIN_SEPARATOR: char = ';';
/// Ends a path with a link back to a node (`...-HallwayRoom_1:ParkourRoom_1`).
const LOOP_SEPARATOR: char = ':';
/// Marks a locked room (`#BossRoom_1`).
const LOCK_MARK: char = '#';
/// Opens the room a node is tied to by its lock (`KeyRoom_1(GateRoom_1)`).
const LINK_OPEN: char = '(';
/// Closes it.
const LINK_CLOSE: char = ')';
/// Separates a symbol from its instance number (`CycleA_2`).
const INSTANCE_SEPARATOR: char = '_';
/// Separates the entries of a list column (`SupportedRoomTypes`, `ThemeTags`).
const LIST_SEPARATOR: char = ',';
/// The folder the client puts before a variation path (`FUN_14334bcb0`:
/// `slices/%s.dynamicslice`).
const SLICE_PREFIX: &str = "slices/";
/// The extension it puts after it.
const SLICE_SUFFIX: &str = ".dynamicslice";
/// The grammar symbol of the hallway that fills a path between rooms.
pub const CONNECTOR_SYMBOL: &str = "Connector";
/// The client caps a cluster's `ReplacementCount` at 5 (`FUN_143337a60`).
pub const MAX_REPLACEMENT_COUNT: u32 = 5;
/// The weight of a row without one (`FUN_143337a60` sets 1 before it reads `Weight`).
const DEFAULT_WEIGHT: u32 = 1;
/// The north bit of a tile's `Connections` (`FUN_14334bcb0`: N 1, E 2, S 4, W 8).
pub const NORTH: u8 = 1;
/// The east bit of a tile's `Connections`.
pub const EAST: u8 = 2;
/// The south bit of a tile's `Connections`.
pub const SOUTH: u8 = 4;
/// The west bit of a tile's `Connections`.
pub const WEST: u8 = 8;
/// Every side.
const ALL_SIDES: u8 = NORTH | EAST | SOUTH | WEST;
/// Bits a quarter turn moves the sides by.
const SIDE_BITS: u8 = 4;
/// The sides with the cell step to the neighbour on that side (north is +y).
const SIDES: [(u8, [i32; 2]); 4] = [
    (NORTH, [0, 1]),
    (EAST, [1, 0]),
    (SOUTH, [0, -1]),
    (WEST, [-1, 0]),
];
/// Degrees of one quarter turn.
pub const QUARTER_TURN_DEG: f32 = 90.0;
/// The height of every tile entity (Observed: all 124 tile cards of the 10 retail runs, and the
/// map's team teleport spot `492,761,120`).
pub const TILE_HEIGHT_M: f32 = 120.0;
/// The first grid cell a tile may take on either axis (best guess: the smallest grid that holds
/// every tile of the 10 retail runs, x 64 to 1216 m and y 64 to 1024 m in 64 m steps).
pub const FIRST_CELL: i32 = 1;
/// The last grid cell a tile may take on the x axis (best guess, as [`FIRST_CELL`]).
pub const LAST_CELL_X: i32 = 19;
/// The last grid cell a tile may take on the y axis (best guess, as [`FIRST_CELL`]).
pub const LAST_CELL_Y: i32 = 16;
/// Layouts tried before a run gives up (best guess: the rule of retail's generator is Unknown;
/// the most a run of seeds 0 to 999 needed was 67).
pub const MAX_ATTEMPTS: u32 = 256;
/// SplitMix64 constants (best guess: the generator of retail's server is Unknown): the step and
/// the two mixing multipliers and three shifts of its output.
const GOLDEN_GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;
const MIX_1: u64 = 0xbf58_476d_1ce4_e5b9;
const MIX_2: u64 = 0x94d0_49bb_1331_11eb;
const MIX_SHIFT_1: u32 = 30;
const MIX_SHIFT_2: u32 = 27;
const MIX_SHIFT_3: u32 = 31;

/// Why the catacomb rules cannot be read or a layout cannot be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatacombError {
    /// No cluster has the feature.
    #[error("no dungeon cluster of feature {feature}")]
    NoCluster {
        /// `FeatureId`.
        feature: String,
    },
    /// A seed graph does not read.
    #[error("seed graph {text:?} does not read")]
    BadGraph {
        /// The text.
        text: String,
    },
    /// A node is still no room after the last replacement pass.
    #[error("symbol {symbol} is no room and has no grammar")]
    UnknownSymbol {
        /// The symbol.
        symbol: String,
    },
    /// No tile fits a cell.
    #[error("no tile of room type {room_type} with open sides {sides:#x}")]
    NoTile {
        /// The room type.
        room_type: String,
        /// The open sides.
        sides: u8,
    },
    /// The tiles do not have one size.
    #[error("the tiles of feature {feature} differ in size")]
    MixedTileSizes {
        /// `FeatureId`.
        feature: String,
    },
    /// The feature has no tiles.
    #[error("no dungeon tile of feature {feature}")]
    NoTiles {
        /// `FeatureId`.
        feature: String,
    },
    /// No layout fit the grid.
    #[error("no layout fit the grid in {attempts} attempts")]
    NoLayout {
        /// Layouts tried.
        attempts: u32,
    },
    /// The game mode is no generated level.
    #[error("game mode {game_mode} has no level generator feature")]
    NotGenerated {
        /// `GameModeID`.
        game_mode: String,
    },
}

/// One node of a seed graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    /// Its label in the graph (`BossRoom_1`).
    pub label: String,
    /// The label without the instance number: a room id or a grammar symbol (`BossRoom`).
    pub symbol: String,
    /// `#`: the room is locked.
    pub locked: bool,
    /// The label in parentheses: the room its lock ties it to (a locked room opens when that one
    /// is done; a key room opens that one). Best guess from the rows `GateBoss` and `HazardBoss`.
    pub lock_link: Option<String>,
}

/// A seed graph (`SeedGraph` of a cluster or grammar row).
///
/// Text form: chains separated by `;`; each chain is items separated by `,`; each item is a
/// path of nodes separated by `-`, linked in order, and may end with `:` and a node to link back
/// to. A node is `#` (locked), a label, and `(label)` (its lock link). A label names the same
/// node everywhere in the graph. The first node of the first item is the entry, the last node of
/// that item's path is the exit. An item after the first whose first node is new hangs from the
/// entry (best guess: `HallwayRoom_1-NamedRoom_1,LoopA_1` puts the loop beside the hallway the
/// cycle starts with, `GateRoom_1-...,KeyRoom_1(GateRoom_1)` the key room beside its gate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedGraph {
    /// The nodes.
    pub nodes: Vec<GraphNode>,
    /// The links, from the earlier node of a path to the later one.
    pub edges: Vec<(usize, usize)>,
    /// Where links into the graph's place go.
    pub entry: usize,
    /// Where links out of the graph's place leave.
    pub exit: usize,
}

impl SeedGraph {
    /// Reads `text`.
    ///
    /// # Errors
    ///
    /// Returns [`CatacombError::BadGraph`] for an empty graph, an empty node or a bad lock link.
    pub fn parse(text: &str) -> Result<Self, CatacombError> {
        let bad = || CatacombError::BadGraph {
            text: text.to_owned(),
        };
        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut edges = Vec::new();
        let mut entry = None;
        let mut exit = None;
        for chain in text.split(CHAIN_SEPARATOR) {
            for item in chain.split(ITEM_SEPARATOR) {
                let (path, back_to) = match item.split_once(LOOP_SEPARATOR) {
                    Some((path, back_to)) => (path, Some(back_to)),
                    None => (item, None),
                };
                let first_item = entry.is_none();
                let mut previous: Option<usize> = None;
                let mut item_first: Option<usize> = None;
                for token in path.split(NODE_SEPARATOR) {
                    let (index, new) = node_of(&mut nodes, token).ok_or_else(bad)?;
                    match (previous, entry) {
                        (Some(previous), _) => edges.push((previous, index)),
                        (None, Some(entry)) if new => edges.push((entry, index)),
                        _ => {}
                    }
                    if first_item {
                        exit = Some(index);
                    }
                    item_first.get_or_insert(index);
                    previous = Some(index);
                }
                if first_item {
                    entry = item_first;
                }
                if let Some(back_to) = back_to {
                    let (target, _) = node_of(&mut nodes, back_to).ok_or_else(bad)?;
                    edges.push((previous.ok_or_else(bad)?, target));
                }
            }
        }
        let known = |link: &String| nodes.iter().any(|node| &node.label == link);
        if nodes
            .iter()
            .any(|node| node.lock_link.as_ref().is_some_and(|link| !known(link)))
        {
            return Err(bad());
        }
        Ok(Self {
            entry: entry.ok_or_else(bad)?,
            exit: exit.ok_or_else(bad)?,
            nodes,
            edges,
        })
    }

    /// The index of the node labelled `label`.
    pub fn index_of(&self, label: &str) -> Option<usize> {
        self.nodes.iter().position(|node| node.label == label)
    }

    /// Whether the graph is its own symbol only (`StartRoom` for `StartRoom`).
    fn is_identity(&self, symbol: &str) -> bool {
        self.nodes.len() == 1 && self.nodes[0].symbol == symbol
    }
}

/// The node of `token` in `nodes`, added when new; `None` for an empty label or link.
fn node_of(nodes: &mut Vec<GraphNode>, token: &str) -> Option<(usize, bool)> {
    let token = token.trim();
    let (locked, rest) = match token.strip_prefix(LOCK_MARK) {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let (label, lock_link) = match rest.split_once(LINK_OPEN) {
        Some((label, link)) => {
            let link = link.strip_suffix(LINK_CLOSE)?.trim();
            if link.is_empty() {
                return None;
            }
            (label.trim(), Some(link.to_owned()))
        }
        None => (rest, None),
    };
    if label.is_empty() {
        return None;
    }
    if let Some(index) = nodes.iter().position(|node| node.label == label) {
        let node = &mut nodes[index];
        node.locked |= locked;
        if node.lock_link.is_none() {
            node.lock_link = lock_link;
        }
        return Some((index, false));
    }
    nodes.push(GraphNode {
        label: label.to_owned(),
        symbol: symbol_of(label).to_owned(),
        locked,
        lock_link,
    });
    Some((nodes.len() - 1, true))
}

/// `label` without a trailing `_<number>`.
fn symbol_of(label: &str) -> &str {
    match label.rsplit_once(INSTANCE_SEPARATOR) {
        Some((symbol, number))
            if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) =>
        {
            symbol
        }
        _ => label,
    }
}

/// One cluster: a start graph.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterRule {
    /// `ClusterId`.
    pub id: String,
    /// `SeedGraph`.
    pub seed: SeedGraph,
    /// `ReplacementCount`, at most [`MAX_REPLACEMENT_COUNT`].
    pub replacement_count: u32,
    /// `Weight` (1 when absent).
    pub weight: u32,
}

/// One grammar row: a graph that replaces a symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct GrammarRule {
    /// `GrammaReplacementId`.
    pub id: String,
    /// `GrammarReplacements`: the symbol it replaces.
    pub replaces: String,
    /// `SeedGraph`.
    pub seed: SeedGraph,
    /// `ThemeTags` without the feature's own tag.
    pub themes: Vec<String>,
    /// `Weight` (1 when absent).
    pub weight: u32,
}

/// One room kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomRule {
    /// `RoomId`.
    pub id: String,
    /// `RoomType`: what tiles it may use.
    pub room_type: String,
    /// `StartingState`: the room script state it starts in.
    pub starting_state: String,
}

/// One tile kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileRule {
    /// `DungeonTileId`.
    pub id: String,
    /// `Connections` as side bits.
    pub sides: u8,
    /// `Rotations`: the most quarter turns it may take.
    pub rotations: u8,
    /// `SupportedRoomTypes`.
    pub room_types: Vec<String>,
    /// `VariationAssetPaths`.
    pub variations: Vec<String>,
    /// `Weight` (1 when absent).
    pub weight: u32,
}

/// The sides `sides` after `turns` quarter turns counter-clockwise seen from above (north goes
/// west). Inferred from the retail tiles: a corner `ES` at 0 degrees beside a straight tile turned
/// 90 degrees to its east, and three-way tiles turned 90 degrees whose open sides meet their
/// neighbours.
pub const fn turned(sides: u8, turns: u8) -> u8 {
    let mut sides = sides & ALL_SIDES;
    let mut left = turns % SIDE_BITS;
    while left > 0 {
        sides = ((sides >> 1) | (sides << (SIDE_BITS - 1))) & ALL_SIDES;
        left -= 1;
    }
    sides
}

/// The side of `from` that faces the neighbour `to`; `None` when they are no neighbours.
fn side_towards(from: [i32; 2], to: [i32; 2]) -> Option<u8> {
    let step = [to[0] - from[0], to[1] - from[1]];
    SIDES
        .iter()
        .find(|(_, side_step)| *side_step == step)
        .map(|(side, _)| *side)
}

/// The grid cell of a world position on a grid of `tile_size_m` (cells are centred on whole
/// multiples of the tile size, as the retail tile positions).
pub fn cell_of(position: [f32; 3], tile_size_m: f32) -> [i32; 2] {
    // A grid of a few dozen cells: the rounded value fits an i32.
    [
        (position[0] / tile_size_m).round() as i32,
        (position[1] / tile_size_m).round() as i32,
    ]
}

/// The slice the client loads for a variation path (`FUN_14334bcb0`), lower case as in the paks.
pub fn variation_slice(variation: &str) -> String {
    format!("{SLICE_PREFIX}{}{SLICE_SUFFIX}", variation.to_lowercase())
}

/// One tile of a layout.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedRoom {
    /// The room kind (`RoomId`).
    pub room_id: String,
    /// Its `RoomType`.
    pub room_type: String,
    /// Whether it fills a path between rooms.
    pub connector: bool,
    /// Whether it is locked.
    pub locked: bool,
    /// The room its lock ties it to, by index in [`CatacombLayout::rooms`].
    pub lock_link: Option<usize>,
    /// Its grid cell.
    pub cell: [i32; 2],
    /// Its open sides.
    pub sides: u8,
    /// The tile kind.
    pub tile_id: String,
    /// The variation path (`VariationAssetPaths` entry).
    pub variation: String,
    /// Quarter turns counter-clockwise.
    pub turns: u8,
    /// The tile entity's position: east, north, up (metres).
    pub position: [f32; 3],
    /// The tile entity's yaw, degrees.
    pub yaw_deg: f32,
}

/// The tiles of one run.
#[derive(Debug, Clone, PartialEq)]
pub struct CatacombLayout {
    /// The cluster used.
    pub cluster: String,
    /// The run's theme (`Lost`, `Corrupted`, ...), when the grammar has themes.
    pub theme: Option<String>,
    /// The tiles; the first is the start room.
    pub rooms: Vec<PlacedRoom>,
    /// `TileSize`, metres.
    pub tile_size_m: f32,
}

impl CatacombLayout {
    /// The phasing frame of the run's 1635 (x, y, width, height): from the origin to one tile
    /// past the largest cell on each axis. Observed: every retail catacomb 1635 (7 runs) has
    /// origin 0, 0 and a width and height of whole tiles that differ per run (640 x 960 to
    /// 1280 x 896); where the run's tiles reach the frame edge, the edge is the largest tile
    /// position plus one tile (1024 to 1088 and 1216 to 1280).
    pub fn frame(&self) -> [f32; 4] {
        let largest = |axis: usize| {
            self.rooms
                .iter()
                .map(|room| room.cell[axis])
                .max()
                .unwrap_or_default()
        };
        // A cell index is a few dozen.
        let edge = |axis: usize| (largest(axis) + 1) as f32 * self.tile_size_m;
        [0.0, 0.0, edge(0), edge(1)]
    }

    /// The room at `cell`.
    pub fn at(&self, cell: [i32; 2]) -> Option<&PlacedRoom> {
        self.rooms.iter().find(|room| room.cell == cell)
    }
}

/// A small deterministic draw (best guess: SplitMix64; retail's is Unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Draw {
    state: u64,
}

impl Draw {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GOLDEN_GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> MIX_SHIFT_1)).wrapping_mul(MIX_1);
        z = (z ^ (z >> MIX_SHIFT_2)).wrapping_mul(MIX_2);
        z ^ (z >> MIX_SHIFT_3)
    }

    /// A whole number below `count` (`count` above 0).
    fn below(&mut self, count: usize) -> usize {
        // `count` is a list length: it and the remainder fit a u64 and a usize.
        (self.next() % count as u64) as usize
    }

    /// One of `items` drawn by `weight`; `None` for no item or no weight.
    fn weighted<'a, T>(&mut self, items: &[&'a T], weight: impl Fn(&T) -> u32) -> Option<&'a T> {
        let total: u64 = items.iter().map(|item| u64::from(weight(*item))).sum();
        if total == 0 {
            return None;
        }
        let mut left = self.next() % total;
        for item in items {
            let item_weight = u64::from(weight(*item));
            if left < item_weight {
                return Some(*item);
            }
            left -= item_weight;
        }
        None
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for last in (1..items.len()).rev() {
            let other = self.below(last + 1);
            items.swap(last, other);
        }
    }
}

/// One node of the graph being expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkNode {
    symbol: String,
    locked: bool,
    lock_link: Option<usize>,
    alive: bool,
}

/// The graph of a run while its symbols are replaced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WorkGraph {
    nodes: Vec<WorkNode>,
    edges: Vec<(usize, usize)>,
    entry: usize,
}

impl WorkGraph {
    /// Adds the nodes and links of `seed`; its entry and exit.
    fn instantiate(&mut self, seed: &SeedGraph) -> (usize, usize) {
        let base = self.nodes.len();
        for node in &seed.nodes {
            self.nodes.push(WorkNode {
                symbol: node.symbol.clone(),
                locked: node.locked,
                lock_link: node
                    .lock_link
                    .as_deref()
                    .and_then(|label| seed.index_of(label))
                    .map(|index| base + index),
                alive: true,
            });
        }
        self.edges
            .extend(seed.edges.iter().map(|(from, to)| (base + from, base + to)));
        (base + seed.entry, base + seed.exit)
    }

    /// Puts `seed` in the place of `node`: links into it go to the seed's entry, links out of it
    /// leave from the seed's exit.
    fn replace(&mut self, node: usize, seed: &SeedGraph) {
        let (entry, exit) = self.instantiate(seed);
        for edge in &mut self.edges {
            if edge.1 == node {
                edge.1 = entry;
            }
            if edge.0 == node {
                edge.0 = exit;
            }
        }
        for other in &mut self.nodes {
            if other.lock_link == Some(node) {
                other.lock_link = Some(entry);
            }
        }
        if self.entry == node {
            self.entry = entry;
        }
        self.nodes[node].alive = false;
    }
}

/// The rooms of a run and their links, before they go on the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RoomGraph {
    rooms: Vec<WorkNode>,
    /// Neighbour lists, sorted.
    links: Vec<Vec<usize>>,
}

/// One cell of the grid being filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fill {
    Room(usize),
    Connector,
}

/// The procedural generation rules of one feature.
#[derive(Debug, Clone, PartialEq)]
pub struct CatacombRules {
    /// `FeatureId`.
    pub feature: String,
    /// The clusters.
    pub clusters: Vec<ClusterRule>,
    /// The grammar rows.
    pub grammars: Vec<GrammarRule>,
    /// The room kinds, by id.
    pub rooms: BTreeMap<String, RoomRule>,
    /// The tile kinds.
    pub tiles: Vec<TileRule>,
    /// `TileSize`, metres.
    pub tile_size_m: f32,
}

impl CatacombRules {
    /// The rules of the level generator feature of game mode `game_mode`.
    ///
    /// # Errors
    ///
    /// Returns [`CatacombError::NotGenerated`] when the mode has no `LevelGeneratorFeatureId`,
    /// else as [`CatacombRules::from_tables`].
    pub fn for_game_mode(tables: &Tables, game_mode: &str) -> Result<Self, CatacombError> {
        let feature = tables
            .game_mode_data
            .iter()
            .flat_map(|table| &table.rows)
            .find(|row| row.game_mode_id.eq_ignore_ascii_case(game_mode))
            .and_then(|row| row.level_generator_feature_id.clone())
            .ok_or_else(|| CatacombError::NotGenerated {
                game_mode: game_mode.to_owned(),
            })?;
        Self::from_tables(tables, &feature)
    }

    /// The rules of `feature` in `tables`.
    ///
    /// # Errors
    ///
    /// Returns [`CatacombError`] when a seed graph does not read, the feature has no cluster or
    /// no tile, or its tiles differ in size.
    pub fn from_tables(tables: &Tables, feature: &str) -> Result<Self, CatacombError> {
        let of_feature = |row_feature: &str| row_feature.eq_ignore_ascii_case(feature);
        let clusters = rows(&tables.dungeon_cluster_static_data)
            .filter(|row| of_feature(&row.feature_id))
            .map(cluster_rule)
            .collect::<Result<Vec<_>, _>>()?;
        if clusters.is_empty() {
            return Err(CatacombError::NoCluster {
                feature: feature.to_owned(),
            });
        }
        let grammars = rows(&tables.dungeon_grammar_static_data)
            .filter(|row| of_feature(&row.feature_id))
            .map(|row| grammar_rule(row, feature))
            .collect::<Result<Vec<_>, _>>()?;
        let rooms = rows(&tables.dungeon_room_static_data)
            .filter(|row| of_feature(&row.feature_id))
            .map(|row| (row.room_id.clone(), room_rule(row)))
            .collect();
        let tiles: Vec<TileRule> = rows(&tables.dungeon_tile_static_data)
            .filter(|row| of_feature(&row.feature_id))
            .map(tile_rule)
            .collect();
        let sizes: BTreeSet<i64> = rows(&tables.dungeon_tile_static_data)
            .filter(|row| of_feature(&row.feature_id))
            .map(|row| row.tile_size)
            .collect();
        let tile_size = match (sizes.len(), sizes.first()) {
            (0, _) | (_, None) => {
                return Err(CatacombError::NoTiles {
                    feature: feature.to_owned(),
                });
            }
            (1, Some(size)) => *size,
            _ => {
                return Err(CatacombError::MixedTileSizes {
                    feature: feature.to_owned(),
                });
            }
        };
        Ok(Self {
            feature: feature.to_owned(),
            clusters,
            grammars,
            rooms,
            tiles,
            // A tile size is a few dozen metres.
            tile_size_m: tile_size as f32,
        })
    }

    /// Every slice a tile variation names, once.
    pub fn variation_slices(&self) -> BTreeSet<String> {
        self.tiles
            .iter()
            .flat_map(|tile| &tile.variations)
            .map(|variation| variation_slice(variation))
            .collect()
    }

    /// The themes of the grammar rows, sorted.
    fn themes(&self) -> Vec<String> {
        self.grammars
            .iter()
            .flat_map(|rule| rule.themes.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The layout of run `seed` whose start room is at `start_cell`.
    ///
    /// # Errors
    ///
    /// Returns [`CatacombError`] when a symbol has no room and no grammar, a cell has no tile, or
    /// no layout fits the grid in [`MAX_ATTEMPTS`] tries.
    pub fn generate(&self, seed: u64, start_cell: [i32; 2]) -> Result<CatacombLayout, CatacombError> {
        let mut draw = Draw::new(seed);
        let clusters: Vec<&ClusterRule> = self.clusters.iter().collect();
        let cluster = draw
            .weighted(&clusters, |cluster| cluster.weight)
            .ok_or_else(|| CatacombError::NoCluster {
                feature: self.feature.clone(),
            })?;
        let themes = self.themes();
        let theme = (!themes.is_empty()).then(|| themes[draw.below(themes.len())].clone());
        let graph = self.expand(cluster, theme.as_deref(), &mut draw)?;
        let connector = self.connector_room()?;
        for _ in 0..MAX_ATTEMPTS {
            if let Some((cells, links)) = place(&graph, start_cell, &mut draw) {
                let rooms = self.tile_rooms(&graph, connector, &cells, &links, &mut draw)?;
                return Ok(CatacombLayout {
                    cluster: cluster.id.clone(),
                    theme,
                    rooms,
                    tile_size_m: self.tile_size_m,
                });
            }
        }
        Err(CatacombError::NoLayout {
            attempts: MAX_ATTEMPTS,
        })
    }

    /// The room a connector symbol becomes (the `Connector` grammar row's single room).
    fn connector_room(&self) -> Result<&RoomRule, CatacombError> {
        let unknown = || CatacombError::UnknownSymbol {
            symbol: CONNECTOR_SYMBOL.to_owned(),
        };
        let rule = self
            .grammars
            .iter()
            .find(|rule| rule.replaces == CONNECTOR_SYMBOL)
            .ok_or_else(unknown)?;
        let [node] = rule.seed.nodes.as_slice() else {
            return Err(unknown());
        };
        self.rooms.get(&node.symbol).ok_or_else(unknown)
    }

    /// The rooms and links of `cluster` after its replacement passes, in the run's `theme`.
    fn expand(
        &self,
        cluster: &ClusterRule,
        theme: Option<&str>,
        draw: &mut Draw,
    ) -> Result<RoomGraph, CatacombError> {
        let mut work = WorkGraph::default();
        let (entry, _) = work.instantiate(&cluster.seed);
        work.entry = entry;
        for _ in 0..cluster.replacement_count {
            let open: Vec<usize> = (0..work.nodes.len())
                .filter(|index| {
                    let node = &work.nodes[*index];
                    node.alive && !self.rooms.contains_key(&node.symbol)
                })
                .collect();
            if open.is_empty() {
                break;
            }
            for node in open {
                let symbol = work.nodes[node].symbol.clone();
                let Some(rule) = self.rule_for(&symbol, theme, draw) else {
                    continue;
                };
                if !rule.seed.is_identity(&symbol) {
                    work.replace(node, &rule.seed);
                }
            }
        }
        if let Some(node) = work
            .nodes
            .iter()
            .find(|node| node.alive && !self.rooms.contains_key(&node.symbol))
        {
            return Err(CatacombError::UnknownSymbol {
                symbol: node.symbol.clone(),
            });
        }
        Ok(room_graph(&work))
    }

    /// A grammar row for `symbol`: the rows of `theme` (and the rows without a theme), or every
    /// row of the symbol when none is of the theme; drawn by weight.
    fn rule_for(&self, symbol: &str, theme: Option<&str>, draw: &mut Draw) -> Option<&GrammarRule> {
        let of_symbol: Vec<&GrammarRule> = self
            .grammars
            .iter()
            .filter(|rule| rule.replaces == symbol)
            .collect();
        let of_theme: Vec<&GrammarRule> = of_symbol
            .iter()
            .copied()
            .filter(|rule| {
                rule.themes.is_empty()
                    || theme.is_some_and(|theme| rule.themes.iter().any(|own| own == theme))
            })
            .collect();
        let pool = if of_theme.is_empty() { of_symbol } else { of_theme };
        draw.weighted(&pool, |rule| rule.weight)
    }

    /// The tiles of the placed cells.
    fn tile_rooms(
        &self,
        graph: &RoomGraph,
        connector: &RoomRule,
        cells: &BTreeMap<[i32; 2], Fill>,
        links: &BTreeSet<([i32; 2], [i32; 2])>,
        draw: &mut Draw,
    ) -> Result<Vec<PlacedRoom>, CatacombError> {
        let mut sides: HashMap<[i32; 2], u8> = HashMap::new();
        for (from, to) in links {
            if let (Some(out), Some(back)) = (side_towards(*from, *to), side_towards(*to, *from)) {
                *sides.entry(*from).or_default() |= out;
                *sides.entry(*to).or_default() |= back;
            }
        }
        // Rooms first in graph order (the start room first), then the connectors by cell.
        let mut order: Vec<([i32; 2], Fill)> = cells
            .iter()
            .filter(|(_, fill)| matches!(fill, Fill::Room(_)))
            .map(|(cell, fill)| (*cell, *fill))
            .collect();
        order.sort_by_key(|(_, fill)| match fill {
            Fill::Room(index) => *index,
            Fill::Connector => usize::MAX,
        });
        order.extend(
            cells
                .iter()
                .filter(|(_, fill)| matches!(fill, Fill::Connector))
                .map(|(cell, fill)| (*cell, *fill)),
        );
        let position_of: HashMap<usize, usize> = order
            .iter()
            .enumerate()
            .filter_map(|(position, (_, fill))| match fill {
                Fill::Room(index) => Some((*index, position)),
                Fill::Connector => None,
            })
            .collect();
        let mut placed = Vec::with_capacity(order.len());
        for (cell, fill) in order {
            let open = sides.get(&cell).copied().unwrap_or_default();
            let (room, locked, lock_link) = match fill {
                Fill::Room(index) => {
                    let node = &graph.rooms[index];
                    let room = self.rooms.get(&node.symbol).ok_or_else(|| {
                        CatacombError::UnknownSymbol {
                            symbol: node.symbol.clone(),
                        }
                    })?;
                    let link = node.lock_link.and_then(|link| position_of.get(&link).copied());
                    (room, node.locked, link)
                }
                Fill::Connector => (connector, false, None),
            };
            let (tile, turns, variation) = self.tile_for(&room.room_type, open, draw)?;
            placed.push(PlacedRoom {
                room_id: room.id.clone(),
                room_type: room.room_type.clone(),
                connector: matches!(fill, Fill::Connector),
                locked,
                lock_link,
                cell,
                sides: open,
                tile_id: tile.id.clone(),
                variation,
                turns,
                // A cell index is a few dozen.
                position: [
                    cell[0] as f32 * self.tile_size_m,
                    cell[1] as f32 * self.tile_size_m,
                    TILE_HEIGHT_M,
                ],
                yaw_deg: f32::from(turns) * QUARTER_TURN_DEG,
            });
        }
        Ok(placed)
    }

    /// A tile of `room_type` with open sides `open`, its quarter turns and a variation.
    fn tile_for(
        &self,
        room_type: &str,
        open: u8,
        draw: &mut Draw,
    ) -> Result<(&TileRule, u8, String), CatacombError> {
        let fits: Vec<(&TileRule, u8)> = self
            .tiles
            .iter()
            .filter(|tile| tile.room_types.iter().any(|own| own == room_type))
            .flat_map(|tile| {
                (0..=tile.rotations)
                    .filter(move |turns| turned(tile.sides, *turns) == open)
                    .map(move |turns| (tile, turns))
            })
            .collect();
        let refs: Vec<&(&TileRule, u8)> = fits.iter().collect();
        let (tile, turns) = draw
            .weighted(&refs, |(tile, _)| tile.weight)
            .copied()
            .ok_or_else(|| CatacombError::NoTile {
                room_type: room_type.to_owned(),
                sides: open,
            })?;
        let variation = match tile.variations.len() {
            0 => {
                return Err(CatacombError::NoTile {
                    room_type: room_type.to_owned(),
                    sides: open,
                });
            }
            count => tile.variations[draw.below(count)].clone(),
        };
        Ok((tile, turns, variation))
    }
}

/// The rooms of `work` that are still alive, with their links both ways.
fn room_graph(work: &WorkGraph) -> RoomGraph {
    // The entry first, so it is room 0.
    let mut alive: Vec<usize> = vec![work.entry];
    alive.extend((0..work.nodes.len()).filter(|index| *index != work.entry && work.nodes[*index].alive));
    let new_index: HashMap<usize, usize> = alive
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect();
    let rooms: Vec<WorkNode> = alive
        .iter()
        .map(|old| {
            let mut node = work.nodes[*old].clone();
            node.lock_link = node.lock_link.and_then(|link| new_index.get(&link).copied());
            node
        })
        .collect();
    let mut links: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); rooms.len()];
    for (from, to) in &work.edges {
        if let (Some(from), Some(to)) = (new_index.get(from), new_index.get(to))
            && from != to
        {
            links[*from].insert(*to);
            links[*to].insert(*from);
        }
    }
    RoomGraph {
        rooms,
        links: links.into_iter().map(|set| set.into_iter().collect()).collect(),
    }
}

/// Whether `cell` is on the grid.
const fn on_grid(cell: [i32; 2]) -> bool {
    cell[0] >= FIRST_CELL && cell[0] <= LAST_CELL_X && cell[1] >= FIRST_CELL && cell[1] <= LAST_CELL_Y
}

fn neighbour(cell: [i32; 2], step: [i32; 2]) -> [i32; 2] {
    [cell[0] + step[0], cell[1] + step[1]]
}

fn link_key(a: [i32; 2], b: [i32; 2]) -> ([i32; 2], [i32; 2]) {
    if a <= b { (a, b) } else { (b, a) }
}

/// The rooms of `graph` on the grid from `start_cell`: each room beside the first placed room it
/// links to (the free neighbour with the most free neighbours, ties drawn), then every other link
/// a path of connectors through free cells. `None` when a room or a link finds no room.
fn place(
    graph: &RoomGraph,
    start_cell: [i32; 2],
    draw: &mut Draw,
) -> Option<(BTreeMap<[i32; 2], Fill>, BTreeSet<([i32; 2], [i32; 2])>)> {
    if graph.rooms.is_empty() || !on_grid(start_cell) {
        return None;
    }
    let mut cells: BTreeMap<[i32; 2], Fill> = BTreeMap::new();
    let mut at: Vec<Option<[i32; 2]>> = vec![None; graph.rooms.len()];
    let mut links: BTreeSet<([i32; 2], [i32; 2])> = BTreeSet::new();
    cells.insert(start_cell, Fill::Room(0));
    at[0] = Some(start_cell);
    let mut queue: VecDeque<usize> = VecDeque::from([0]);
    let mut queued = vec![false; graph.rooms.len()];
    queued[0] = true;
    while let Some(room) = queue.pop_front() {
        let from = at[room]?;
        let mut children: Vec<usize> = graph.links[room]
            .iter()
            .copied()
            .filter(|child| !queued[*child])
            .collect();
        draw.shuffle(&mut children);
        for child in children {
            let cell = free_neighbour(&cells, from, draw)?;
            cells.insert(cell, Fill::Room(child));
            at[child] = Some(cell);
            links.insert(link_key(from, cell));
            queued[child] = true;
            queue.push_back(child);
        }
    }
    for (room, neighbours) in graph.links.iter().enumerate() {
        for other in neighbours.iter().copied().filter(|other| *other > room) {
            let (a, b) = (at[room]?, at[other]?);
            if side_towards(a, b).is_some() {
                links.insert(link_key(a, b));
                continue;
            }
            let path = free_path(&cells, a, b)?;
            let mut previous = a;
            for cell in path {
                if cell != b {
                    cells.insert(cell, Fill::Connector);
                }
                links.insert(link_key(previous, cell));
                previous = cell;
            }
        }
    }
    Some((cells, links))
}

/// The free neighbour of `from` with the most free neighbours of its own (ties drawn).
fn free_neighbour(
    cells: &BTreeMap<[i32; 2], Fill>,
    from: [i32; 2],
    draw: &mut Draw,
) -> Option<[i32; 2]> {
    let free = |cell: [i32; 2]| on_grid(cell) && !cells.contains_key(&cell);
    let mut candidates: Vec<([i32; 2], usize)> = SIDES
        .iter()
        .map(|(_, step)| neighbour(from, *step))
        .filter(|cell| free(*cell))
        .map(|cell| {
            let room = SIDES
                .iter()
                .filter(|(_, step)| free(neighbour(cell, *step)))
                .count();
            (cell, room)
        })
        .collect();
    draw.shuffle(&mut candidates);
    let best = candidates.iter().map(|(_, room)| *room).max()?;
    candidates
        .into_iter()
        .find(|(_, room)| *room == best)
        .map(|(cell, _)| cell)
}

/// The shortest path of free cells from `from` (left out) to `to` (the last cell); `None` when
/// there is none.
fn free_path(
    cells: &BTreeMap<[i32; 2], Fill>,
    from: [i32; 2],
    to: [i32; 2],
) -> Option<Vec<[i32; 2]>> {
    let mut came_from: HashMap<[i32; 2], [i32; 2]> = HashMap::new();
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        for (_, step) in SIDES {
            let next = neighbour(cell, step);
            if next == from || came_from.contains_key(&next) {
                continue;
            }
            if next == to {
                came_from.insert(next, cell);
                let mut path = vec![to];
                let mut back = cell;
                while back != from {
                    path.push(back);
                    back = *came_from.get(&back)?;
                }
                path.reverse();
                return Some(path);
            }
            if on_grid(next) && !cells.contains_key(&next) {
                came_from.insert(next, cell);
                queue.push_back(next);
            }
        }
    }
    None
}

fn rows<T>(tables: &[crate::table::Table<T>]) -> impl Iterator<Item = &T> {
    tables.iter().flat_map(|table| &table.rows)
}

fn list(text: &str) -> Vec<String> {
    text.split(LIST_SEPARATOR)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

fn weight_of(text: Option<&str>) -> u32 {
    text.and_then(|text| text.trim().parse().ok())
        .unwrap_or(DEFAULT_WEIGHT)
}

fn cluster_rule(row: &DungeonClusterStaticData) -> Result<ClusterRule, CatacombError> {
    Ok(ClusterRule {
        id: row.cluster_id.clone(),
        seed: SeedGraph::parse(&row.seed_graph)?,
        replacement_count: u32::try_from(row.replacement_count)
            .unwrap_or_default()
            .min(MAX_REPLACEMENT_COUNT),
        weight: weight_of(row.weight.as_deref()),
    })
}

fn grammar_rule(row: &DungeonGrammarStaticData, feature: &str) -> Result<GrammarRule, CatacombError> {
    Ok(GrammarRule {
        id: row.gramma_replacement_id.clone(),
        replaces: row.grammar_replacements.clone(),
        seed: SeedGraph::parse(&row.seed_graph)?,
        themes: row
            .theme_tags
            .as_deref()
            .map(list)
            .unwrap_or_default()
            .into_iter()
            .filter(|tag| !tag.eq_ignore_ascii_case(feature))
            .collect(),
        weight: row
            .weight
            .and_then(|weight| u32::try_from(weight).ok())
            .unwrap_or(DEFAULT_WEIGHT),
    })
}

fn room_rule(row: &DungeonRoomStaticData) -> RoomRule {
    RoomRule {
        id: row.room_id.clone(),
        room_type: row.room_type.clone(),
        starting_state: row.starting_state.clone(),
    }
}

fn tile_rule(row: &DungeonTileStaticData) -> TileRule {
    // `FUN_14334bcb0`: each letter sets its side, any case; other characters are skipped.
    let sides = row
        .connections
        .chars()
        .fold(0, |sides, letter| match letter.to_ascii_uppercase() {
            'N' => sides | NORTH,
            'E' => sides | EAST,
            'S' => sides | SOUTH,
            'W' => sides | WEST,
            _ => sides,
        });
    TileRule {
        id: row.dungeon_tile_id.clone(),
        sides,
        rotations: u8::try_from(row.rotations).unwrap_or_default(),
        room_types: list(&row.supported_room_types),
        variations: list(&row.variation_asset_paths),
        weight: weight_of(row.weight.as_deref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;

    const FEATURE: &str = "Catacombs";
    /// The map's team teleport spot (`TeamTeleportData` `492,761,120,0`).
    const TELEPORT: [f32; 3] = [492.0, 761.0, 120.0];
    const TILE_SIZE: f32 = 64.0;

    fn table<T>(rows: Vec<T>) -> Vec<Table<T>> {
        vec![Table {
            source_path: "test.datasheet".to_owned(),
            unique_id: "Test".to_owned(),
            rows,
        }]
    }

    fn from_json<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).expect("row")
    }

    /// The retail Catacombs rows of build 25810808 (the four procedural generation datasheets).
    fn tables() -> Tables {
        let mut tables = Tables::default();
        tables.dungeon_cluster_static_data = table(vec![from_json(serde_json::json!({
            "cluster_id": "CatacombsBranch",
            "feature_id": FEATURE,
            "replacement_count": 5,
            "seed_graph": "StartRoom_1-CycleB_1-CycleA_1-CycleB_2-CycleA_2-CycleB_3-CycleA_3",
        }))]);
        let grammar = |id: &str, replaces: &str, seed: &str, themes: Option<&str>| {
            from_json::<DungeonGrammarStaticData>(serde_json::json!({
                "feature_id": FEATURE,
                "gramma_replacement_id": id,
                "grammar_replacements": replaces,
                "seed_graph": seed,
                "theme_tags": themes,
            }))
        };
        let mut grammars = vec![
            grammar("StartRoom", "StartRoom", "StartRoom", None),
            grammar("BossRoom", "BossRoom", "BossRoom", None),
            grammar("Connector", "Connector", "HallwayRoom", None),
            grammar(
                "LoopA",
                "LoopA",
                "ParkourRoom_1-NamedRoom_1-RewardRoom_1-HazardBoss_1-HallwayRoom_1:ParkourRoom_1",
                None,
            ),
            grammar(
                "LoopB",
                "LoopB",
                "ParkourRoom_1-NamedRoom_1-RewardRoom_1-GateBoss_1-HallwayRoom_1:ParkourRoom_1",
                None,
            ),
            grammar("HazardBoss", "HazardBoss", "HazardRoom_1-#BossRoom_1(HazardRoom_1)", None),
            grammar(
                "GateBoss",
                "GateBoss",
                "GateRoom_1-#BossRoom_1(GateRoom_1),KeyRoom_1(GateRoom_1)",
                None,
            ),
        ];
        for theme in ["Lost", "Corrupted", "Human", "Beast", "Ancient", "Angry", "Hercyne", "Upyr"] {
            let tags = format!("{theme},{FEATURE}");
            for (cycle, room) in [("CycleA", "LoopA_1"), ("CycleB", "LoopB_1")] {
                grammars.push(grammar(
                    format!("{cycle}_{theme}").as_str(),
                    cycle,
                    format!("HallwayRoom_1-NamedRoom_1,{room}").as_str(),
                    Some(tags.as_str()),
                ));
            }
        }
        tables.dungeon_grammar_static_data = table(grammars);
        let room = |id: &str, room_type: &str| {
            from_json::<DungeonRoomStaticData>(serde_json::json!({
                "alias_category1": room_type,
                "feature_id": FEATURE,
                "imgui_color": "1,1,1,1",
                "room_id": id,
                "room_type": room_type,
                "starting_state": format!("State{id}"),
            }))
        };
        tables.dungeon_room_static_data = table(vec![
            room("StartRoom", "EncounterStart"),
            room("BossRoom", "EncounterBoss"),
            room("NamedRoom", "EncounterNamedEnemy"),
            room("GateRoom", "EncounterGate"),
            room("KeyRoom", "EncounterKey"),
            room("HazardRoom", "EncounterHazard"),
            room("HallwayRoom", "Hallway"),
            room("ParkourRoom", "Parkour"),
            room("RewardRoom", "Reward"),
        ]);
        let rooms = "EncounterStart,EncounterBoss,EncounterNamedEnemy,EncounterGate,EncounterHazard,Reward";
        let tile = |id: &str, sides: &str, rotations: i64, types: &str, paths: &str| {
            from_json::<DungeonTileStaticData>(serde_json::json!({
                "connections": sides,
                "dungeon_tile_id": id,
                "feature_id": FEATURE,
                "rotations": rotations,
                "supported_room_types": types,
                "tile_size": 64,
                "variation_asset_paths": paths,
            }))
        };
        let mut tiles = Vec::new();
        for (kind, sides, rotations, number) in [
            ("Straight", "NS", 1, 1),
            ("Corner", "ES", 3, 2),
            ("ThreeWay", "NES", 3, 3),
            ("DeadEnd", "S", 3, 4),
            ("FourWay", "NESW", 0, 5),
        ] {
            tiles.push(tile(
                format!("{kind}Room").as_str(),
                sides,
                rotations,
                rooms,
                format!("Catacombs/Gameplay/Tiles/tile{number}_a_gameplay").as_str(),
            ));
            tiles.push(tile(
                format!("{kind}Hallway").as_str(),
                sides,
                rotations,
                "Hallway,EncounterKey",
                format!(
                    "Catacombs/Gameplay/Tiles/tile{number}_ba_gameplay,Catacombs/Gameplay/Tiles/tile{number}_ca_gameplay"
                )
                .as_str(),
            ));
            tiles.push(tile(
                format!("{kind}Hallway_Parkour").as_str(),
                sides,
                rotations,
                "Parkour",
                format!("Catacombs/Gameplay/Tiles/tile{number}_d_gameplay").as_str(),
            ));
        }
        tables.dungeon_tile_static_data = table(tiles);
        tables
    }

    fn rules() -> CatacombRules {
        CatacombRules::from_tables(&tables(), FEATURE).expect("rules")
    }

    #[test]
    fn a_gate_boss_graph_reads_its_lock_and_its_key_room() {
        let graph =
            SeedGraph::parse("GateRoom_1-#BossRoom_1(GateRoom_1),KeyRoom_1(GateRoom_1)").expect("graph");
        let labels: Vec<&str> = graph.nodes.iter().map(|node| node.label.as_str()).collect();
        assert_eq!(labels, ["GateRoom_1", "BossRoom_1", "KeyRoom_1"]);
        assert_eq!(graph.nodes[1].symbol, "BossRoom");
        assert!(graph.nodes[1].locked && !graph.nodes[2].locked);
        assert_eq!(graph.nodes[1].lock_link.as_deref(), Some("GateRoom_1"));
        assert_eq!(graph.nodes[2].lock_link.as_deref(), Some("GateRoom_1"));
        assert_eq!(graph.edges, [(0, 1), (0, 2)], "the key room hangs from the entry");
        assert_eq!((graph.entry, graph.exit), (0, 1));
    }

    #[test]
    fn a_loop_links_back_and_a_second_chain_reuses_its_nodes() {
        let graph = SeedGraph::parse(
            "ParkourRoom_1-NamedRoom_1-RewardRoom_1-HazardBoss_1-HallwayRoom_1:ParkourRoom_1",
        )
        .expect("loop");
        assert_eq!(graph.nodes.len(), 5);
        assert_eq!(graph.edges.last(), Some(&(4, 0)));
        assert_eq!((graph.entry, graph.exit), (0, 4));
        let puzzle = SeedGraph::parse("T-K-P-L;L-T").expect("puzzle");
        assert_eq!(puzzle.nodes.len(), 4);
        assert_eq!(puzzle.edges, [(0, 1), (1, 2), (2, 3), (3, 0)]);
        assert_eq!(puzzle.nodes[0].symbol, "T", "a label without a number is its symbol");
    }

    #[test]
    fn bad_graphs_do_not_read() {
        for text in ["", "A--B", "A-#(B)", "A(Missing)", "A(B"] {
            assert!(SeedGraph::parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn a_quarter_turn_moves_north_to_west() {
        assert_eq!(turned(NORTH, 1), WEST);
        assert_eq!(turned(EAST | SOUTH, 1), NORTH | EAST);
        assert_eq!(turned(NORTH | SOUTH, 1), EAST | WEST);
        assert_eq!(turned(NORTH | EAST | SOUTH, 2), SOUTH | WEST | NORTH);
        assert_eq!(turned(ALL_SIDES, 3), ALL_SIDES);
        assert_eq!(turned(EAST, 4), EAST, "four turns are a whole turn");
    }

    #[test]
    fn the_rules_read_the_retail_rows() {
        let rules = rules();
        assert_eq!(rules.clusters[0].replacement_count, 5);
        assert_eq!(rules.clusters[0].weight, DEFAULT_WEIGHT);
        assert_eq!(rules.tile_size_m, TILE_SIZE);
        let corner = rules.tiles.iter().find(|tile| tile.id == "CornerRoom").expect("corner");
        assert_eq!((corner.sides, corner.rotations), (EAST | SOUTH, 3));
        assert_eq!(rules.themes().len(), 8);
        let cycle = rules
            .grammars
            .iter()
            .find(|rule| rule.id == "CycleA_Lost")
            .expect("cycle");
        assert_eq!(cycle.themes, ["Lost"], "the feature's own tag is no theme");
        assert!(
            rules
                .variation_slices()
                .contains("slices/catacombs/gameplay/tiles/tile2_ba_gameplay.dynamicslice")
        );
    }

    #[test]
    fn a_run_has_the_rooms_of_its_six_cycles() {
        let rules = rules();
        let layout = rules
            .generate(7, cell_of(TELEPORT, TILE_SIZE))
            .expect("layout");
        let count = |id: &str| {
            layout
                .rooms
                .iter()
                .filter(|room| room.room_id == id && !room.connector)
                .count()
        };
        assert_eq!(count("StartRoom"), 1);
        assert_eq!(count("BossRoom"), 6, "one boss per loop");
        assert_eq!((count("GateRoom"), count("KeyRoom")), (3, 3), "the three B loops");
        assert_eq!(count("HazardRoom"), 3, "the three A loops");
        assert_eq!((count("NamedRoom"), count("RewardRoom"), count("ParkourRoom")), (12, 6, 6));
        assert_eq!(count("HallwayRoom"), 12);
        assert!(layout.theme.is_some());
        let bosses: Vec<&PlacedRoom> = layout
            .rooms
            .iter()
            .filter(|room| room.room_id == "BossRoom")
            .collect();
        for boss in bosses {
            assert!(boss.locked);
            let link = boss.lock_link.expect("its lock room");
            assert!(["GateRoom", "HazardRoom"].contains(&layout.rooms[link].room_id.as_str()));
        }
    }

    #[test]
    fn every_tile_fits_its_cell_and_the_run_is_one_piece() {
        let rules = rules();
        for seed in [1, 2, 3, 99] {
            let start = cell_of(TELEPORT, TILE_SIZE);
            let layout = rules.generate(seed, start).expect("layout");
            let start_room = &layout.rooms[0];
            assert_eq!((start_room.room_id.as_str(), start_room.cell), ("StartRoom", start));
            assert_eq!(start_room.position, [512.0, 768.0, TILE_HEIGHT_M]);
            let cells: BTreeSet<[i32; 2]> = layout.rooms.iter().map(|room| room.cell).collect();
            assert_eq!(cells.len(), layout.rooms.len(), "one tile per cell");
            for room in &layout.rooms {
                assert!(on_grid(room.cell));
                let tile = rules.tiles.iter().find(|tile| tile.id == room.tile_id).expect("tile");
                assert_eq!(turned(tile.sides, room.turns), room.sides);
                assert!(room.turns <= tile.rotations);
                assert!(tile.room_types.contains(&room.room_type));
                assert!(tile.variations.contains(&room.variation));
                assert_eq!(room.yaw_deg, f32::from(room.turns) * QUARTER_TURN_DEG);
                for (side, step) in SIDES {
                    if room.sides & side != 0 {
                        let other = layout
                            .at(neighbour(room.cell, step))
                            .expect("an open side faces a tile");
                        assert_ne!(other.sides & turned(side, 2), 0, "and that tile opens back");
                    }
                }
            }
            let mut seen = BTreeSet::from([start]);
            let mut queue = VecDeque::from([start]);
            while let Some(cell) = queue.pop_front() {
                let room = layout.at(cell).expect("room");
                for (side, step) in SIDES {
                    let next = neighbour(cell, step);
                    if room.sides & side != 0 && seen.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
            assert_eq!(seen.len(), layout.rooms.len(), "every tile is reached from the start");
        }
    }

    #[test]
    fn the_frame_ends_one_tile_past_the_largest_cell() {
        let layout = rules().generate(11, cell_of(TELEPORT, TILE_SIZE)).expect("layout");
        let [x, y, width, height] = layout.frame();
        assert_eq!((x, y), (0.0, 0.0));
        let largest_x = layout.rooms.iter().map(|room| room.position[0]).fold(0.0, f32::max);
        let largest_y = layout.rooms.iter().map(|room| room.position[1]).fold(0.0, f32::max);
        assert_eq!((width, height), (largest_x + TILE_SIZE, largest_y + TILE_SIZE));
        assert!(width <= (LAST_CELL_X + 1) as f32 * TILE_SIZE);
    }

    #[test]
    fn a_seed_gives_the_same_run_every_time() {
        let rules = rules();
        let start = cell_of(TELEPORT, TILE_SIZE);
        assert_eq!(rules.generate(42, start), rules.generate(42, start));
    }

    #[test]
    fn connectors_are_hallways_and_a_start_off_the_grid_fails() {
        let rules = rules();
        let layout = rules
            .generate(5, cell_of(TELEPORT, TILE_SIZE))
            .expect("layout");
        for room in layout.rooms.iter().filter(|room| room.connector) {
            assert_eq!((room.room_id.as_str(), room.room_type.as_str()), ("HallwayRoom", "Hallway"));
        }
        assert_eq!(
            rules.generate(5, [0, 0]),
            Err(CatacombError::NoLayout {
                attempts: MAX_ATTEMPTS
            })
        );
    }

    #[test]
    fn a_game_mode_without_a_generator_has_no_rules() {
        let mut tables = tables();
        tables.game_mode_data = table(vec![from_json(serde_json::json!({
            "allow_camping": false,
            "feature_flag": "",
            "game_mode_id": "DungeonAmrine",
            "raid_invite_delay_sec": 0,
            "raid_invite_duration_sec": 0,
            "script_name": "DungeonAmrine",
        }))]);
        assert_eq!(
            CatacombRules::for_game_mode(&tables, "DungeonAmrine"),
            Err(CatacombError::NotGenerated {
                game_mode: "DungeonAmrine".to_owned()
            })
        );
        let mut row = tables.game_mode_data[0].rows[0].clone();
        row.game_mode_id = "Catacombs".to_owned();
        row.level_generator_feature_id = Some(FEATURE.to_owned());
        tables.game_mode_data[0].rows.push(row);
        assert_eq!(
            CatacombRules::for_game_mode(&tables, "catacombs").map(|rules| rules.feature),
            Ok(FEATURE.to_owned())
        );
    }
}
