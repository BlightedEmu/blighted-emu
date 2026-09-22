// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed New World world data (M1b spec): levels, ground height, spawn and point lists,
//! capitals, the world file, and the level collision file with its search index.

pub mod capitals;
pub mod collision;
pub mod collision_index;
pub mod height;
pub mod map_tiles;
pub mod metadata;
pub mod records;
pub mod roads;
pub mod water;
pub mod script;
pub mod slice_data;
pub mod stations;
pub mod world_file;

pub use records::WorldFile;
