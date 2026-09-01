// SPDX-License-Identifier: MIT OR Apache-2.0
//! Facts about the pinned client build (Steam build 22469132), for tests that read a real
//! install. Observed on 2026-08-29 (M1 spec section 2) and 2026-08-31 (M1b spec section 2).

use std::path::PathBuf;

/// Environment variable that names the New World install folder.
pub const GAME_DIR_ENV: &str = "NW_GAME_DIR";
/// Steam build id of the pinned client (also in `config/client-build.toml`).
pub const PINNED_STEAM_BUILD: &str = "22469132";
/// Number of `.pak` files under `assets/`.
pub const PINNED_PAK_COUNT: usize = 130;
/// Number of entries in all paks.
pub const PINNED_ENTRY_COUNT: usize = 1_446_650;
/// Entries that use Oodle (method 15).
pub const PINNED_OODLE_ENTRIES: usize = 1_340_017;
/// Entries that are stored (method 0).
pub const PINNED_STORED_ENTRIES: usize = 106_030;
/// Entries that use deflate (method 8).
pub const PINNED_DEFLATE_ENTRIES: usize = 603;
/// Datasheet files in all paks.
pub const PINNED_DATASHEET_FILES: usize = 2_250;
/// Datasheet files that use Oodle.
pub const PINNED_OODLE_DATASHEETS: usize = 2_052;
/// Datasheet files that are stored.
pub const PINNED_STORED_DATASHEETS: usize = 198;
/// Rows in all datasheet files.
pub const PINNED_DATASHEET_ROWS: usize = 285_031;
/// Table types over all datasheet files.
pub const PINNED_TABLE_TYPES: usize = 220;
/// `localization/en-us/*.loc.xml` files.
pub const PINNED_EN_US_TEXT_FILES: usize = 184;
/// `<string>` elements without a `key` attribute in the en-us files.
pub const PINNED_EN_US_KEYLESS: usize = 4;

/// `.timeline` files (all AZCS around an ObjectStream).
pub const PINNED_TIMELINE_FILES: usize = 80_367;
/// `region.metadata` files (open world only).
pub const PINNED_REGION_METADATA_FILES: usize = 49;
/// Capital `.metadata` files in all levels.
pub const PINNED_CAPITAL_METADATA_FILES: usize = 878;
/// `region.heightmap` files of 2048 x 2048 samples.
pub const PINNED_FULL_HEIGHTMAPS: usize = 98;
/// `region.heightmap` files of 1024 x 1024 samples (the two FTUE levels).
pub const PINNED_SMALL_HEIGHTMAPS: usize = 2;
/// Edge length of the small heightmaps.
pub const PINNED_SMALL_HEIGHTMAP_SIZE: u32 = 1024;
/// Level folders with a `tracts.json`.
pub const PINNED_LEVELS: usize = 41;
/// Folder name of the open world level.
pub const PINNED_OPEN_WORLD_LEVEL: &str = "newworld_vitaeeterna";
/// Open world regions (7 x 7).
pub const PINNED_OPEN_WORLD_REGIONS: usize = 49;
/// Open world NPC points.
pub const PINNED_OPEN_WORLD_NPCS: usize = 1_626;
/// Open world creature spawn points.
pub const PINNED_OPEN_WORLD_VITALS: usize = 115_937;
/// Open world gatherable points.
pub const PINNED_OPEN_WORLD_GATHERABLES: usize = 148_069;
/// Open world lore points.
pub const PINNED_OPEN_WORLD_LORE: usize = 1_235;
/// Open world landmarks.
pub const PINNED_OPEN_WORLD_LANDMARKS: usize = 2_020;
/// Open world encounter points.
pub const PINNED_OPEN_WORLD_ENCOUNTERS: usize = 920;
/// Open world capitals.
pub const PINNED_OPEN_WORLD_CAPITALS: usize = 267_847;

/// The install folder from [`GAME_DIR_ENV`]. Panics with a clear message when it is not set,
/// because the `local-data` tests cannot run without it.
pub fn game_dir() -> PathBuf {
    std::env::var_os(GAME_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {GAME_DIR_ENV} to the New World install folder"))
}
