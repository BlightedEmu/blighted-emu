// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed New World world data (M1b spec): levels, ground height, spawn and point lists,
//! capitals, and the world file.

pub mod capitals;
pub mod height;
pub mod map_tiles;
pub mod metadata;
pub mod records;
pub mod world_file;

pub use records::WorldFile;
