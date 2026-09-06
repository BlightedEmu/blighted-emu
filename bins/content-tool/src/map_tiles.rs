// SPDX-License-Identifier: AGPL-3.0-or-later
//! `map-tiles`: decodes the world map tiles of one level from the install
//! (`lyshineui/worldtiles/<level>/map_l{L}_y{YYY}_x{XXX}.dds`) and writes them as PNG files with
//! a `tiles.json` index (admin tools spec section 3.1). The file name indexes count level 1
//! tiles at every level; the written files use the grid index of their own level.
//!
//! Map tiles are game data: they stay in the git-ignored `local-data/`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::sync_channel;

use nw_formats::dds::{DdsError, DdsFormat, dds_header, decode_dds};
use nw_world::map_tiles::{LEVEL_1_TILE_M, TILES_FILE, TilesIndex};

use crate::error::ToolError;
use crate::source::GameSource;

/// Default output folder; each level gets a folder in it.
pub const DEFAULT_MAP_DIR: &str = "local-data/content/map";
/// Folder of the world map tiles in the paks.
pub const WORLDTILES_PREFIX: &str = "lyshineui/worldtiles/";
const NAME_START: &str = "map_l";
const Y_MARK: &str = "_y";
const X_MARK: &str = "_x";
const DDS_EXTENSION: &str = ".dds";
/// Tiles waiting for a worker.
const QUEUE_PER_WORKER: usize = 4;

/// Which edge tile row 0 is (admin tools spec section 3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum YIndex {
    /// Row 0 is the north edge.
    NorthUp,
    /// Row 0 is the south edge.
    SouthUp,
}

/// One tile file name: its level and its index in level 1 tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileName {
    /// Level, from 1.
    pub level: u8,
    /// Row, counted in level 1 tiles.
    pub y: u16,
    /// Column, counted in level 1 tiles.
    pub x: u16,
}

impl TileName {
    /// Row and column in the grid of its own level.
    pub fn grid_index(&self) -> (u16, u16) {
        let shift = u32::from(self.level.saturating_sub(1));
        (
            self.y.checked_shr(shift).unwrap_or(0),
            self.x.checked_shr(shift).unwrap_or(0),
        )
    }
}

/// The tile of `path` when it is a world map tile of `level_name` (case-insensitive).
pub fn parse_tile_name(path: &str, level_name: &str) -> Option<TileName> {
    let path = path.to_ascii_lowercase();
    let folder = format!("{WORLDTILES_PREFIX}{}/", level_name.to_ascii_lowercase());
    let name = path.strip_prefix(&folder)?;
    let rest = name.strip_prefix(NAME_START)?.strip_suffix(DDS_EXTENSION)?;
    let (level, rest) = rest.split_once(Y_MARK)?;
    let (y, x) = rest.split_once(X_MARK)?;
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if !(digits(level) && digits(y) && digits(x)) {
        return None;
    }
    Some(TileName {
        level: level.parse().ok()?,
        y: y.parse().ok()?,
        x: x.parse().ok()?,
    })
}

/// A tile that cannot be written.
#[derive(Debug, thiserror::Error)]
pub enum MapTilesError {
    /// The tile does not decode.
    #[error("{path}: {source}")]
    Dds {
        /// Tile.
        path: String,
        /// Cause.
        source: DdsError,
    },
    /// The PNG cannot be written.
    #[error("{path}: {source}")]
    Png {
        /// Output file.
        path: PathBuf,
        /// Cause.
        source: png::EncodingError,
    },
    /// A file or folder cannot be written.
    #[error("{path}: {source}")]
    Io {
        /// File or folder.
        path: PathBuf,
        /// Cause.
        source: std::io::Error,
    },
    /// The install has no tile of the level.
    #[error("no map tiles for level {0}")]
    NoTiles(String),
}

/// Decodes one tile and writes it to `<level_dir>/<level>/<row>_<column>.png`; returns its format.
///
/// # Errors
///
/// Returns [`MapTilesError`] when the tile does not decode or cannot be written.
pub fn write_tile(
    level_dir: &Path,
    name: &TileName,
    bytes: &[u8],
) -> Result<DdsFormat, MapTilesError> {
    let (y, x) = name.grid_index();
    let label = format!("level {} tile {y}_{x}", name.level);
    let (image, rgba) = decode_dds(bytes).map_err(|source| MapTilesError::Dds {
        path: label,
        source,
    })?;
    let folder = level_dir.join(name.level.to_string());
    std::fs::create_dir_all(&folder).map_err(|source| MapTilesError::Io {
        path: folder.clone(),
        source,
    })?;
    let path = folder.join(format!("{y}_{x}.png"));
    let file = File::create(&path).map_err(|source| MapTilesError::Io {
        path: path.clone(),
        source,
    })?;
    let png_error = |source| MapTilesError::Png {
        path: path.clone(),
        source,
    };
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(png_error)?;
    writer.write_image_data(&rgba).map_err(png_error)?;
    writer.finish().map_err(png_error)?;
    Ok(image.format)
}

/// What `map-tiles` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MapTilesReport {
    /// Tiles per level.
    pub per_level: BTreeMap<u8, u64>,
    /// Tiles per pixel format (or per unsupported format name).
    pub formats: BTreeMap<String, u64>,
    /// Tiles whose pak entry cannot be read (path and reason); they are left out.
    pub unreadable: Vec<(String, String)>,
}

/// Counts the formats of every tile of `level` (`formats_only`), or writes every tile and
/// `tiles.json` into `<out_dir>/<level>/`. A tile whose pak entry cannot be read is left out and
/// listed in [`MapTilesReport::unreadable`] (the Oodle decoder fails on 2 of the 3,031 tiles of
/// the pinned client); the map shows the coarser tile there.
///
/// # Errors
///
/// Returns [`ToolError`] when the install cannot be read, a tile cannot be written, or the level
/// has no tile.
pub fn run_map_tiles(
    source: &GameSource,
    level: &str,
    out_dir: &Path,
    y_index: YIndex,
    formats_only: bool,
) -> Result<MapTilesReport, ToolError> {
    let report = Mutex::new(MapTilesReport::default());
    let level_dir = out_dir.join(level);
    let first_error: Mutex<Option<MapTilesError>> = Mutex::new(None);
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    let (sender, receiver) = sync_channel::<(TileName, Vec<u8>)>(workers * QUEUE_PER_WORKER);
    let receiver = Mutex::new(receiver);
    let mut max_index: u16 = 0;
    std::thread::scope(|scope| -> Result<(), ToolError> {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let next = match receiver.lock() {
                        Ok(queue) => queue.recv(),
                        Err(_) => return,
                    };
                    let Ok((name, bytes)) = next else {
                        return;
                    };
                    let format = if formats_only {
                        Ok(dds_header(&bytes).map_or_else(
                            |error| error.to_string(),
                            |(image, _)| format!("{:?}", image.format),
                        ))
                    } else {
                        write_tile(&level_dir, &name, &bytes).map(|format| format!("{format:?}"))
                    };
                    match format {
                        Ok(format) => {
                            if let Ok(mut report) = report.lock() {
                                *report.per_level.entry(name.level).or_default() += 1;
                                *report.formats.entry(format).or_default() += 1;
                            }
                        }
                        Err(error) => {
                            if let Ok(mut first) = first_error.lock() {
                                first.get_or_insert(error);
                            }
                        }
                    }
                }
            });
        }
        let sent = source.for_each_entry_read(
            |path| parse_tile_name(path, level).is_some(),
            |path, bytes| {
                let bytes = match bytes {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        if let Ok(mut report) = report.lock() {
                            report.unreadable.push((path.to_owned(), error.to_string()));
                        }
                        return Ok(());
                    }
                };
                if let Some(name) = parse_tile_name(path, level) {
                    if name.level == 1 {
                        max_index = max_index.max(name.x).max(name.y);
                    }
                    // A closed queue means every worker stopped; the count below shows it.
                    let _ = sender.send((name, bytes));
                }
                Ok::<(), ToolError>(())
            },
        );
        drop(sender);
        sent.map(|_| ())
    })?;
    if let Some(error) = first_error.into_inner().unwrap_or(None) {
        return Err(error.into());
    }
    let report = report.into_inner().unwrap_or_default();
    let Some(levels) = report.per_level.keys().next_back().copied() else {
        return Err(MapTilesError::NoTiles(level.to_owned()).into());
    };
    if !formats_only {
        let world_m = (u32::from(max_index) + 1) * LEVEL_1_TILE_M;
        let index = TilesIndex::new(level, world_m, levels, y_index == YIndex::NorthUp);
        let path = level_dir.join(TILES_FILE);
        let json = serde_json::to_vec_pretty(&index).map_err(|source| MapTilesError::Io {
            path: path.clone(),
            source: source.into(),
        })?;
        std::fs::write(&path, json).map_err(|source| MapTilesError::Io { path, source })?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVEL: &str = "newworld_vitaeeterna";
    const DX10_HEADER_LEN: usize = 148;
    const DXGI_RGBA8_UNORM: u32 = 28;
    const TILE_PX: u32 = 4;

    /// An uncompressed RGBA8 DDS of `side` x `side` pixels, all one colour.
    fn rgba_dds(side: u32, colour: [u8; 4]) -> Vec<u8> {
        let mut bytes = vec![0_u8; DX10_HEADER_LEN];
        bytes[..4].copy_from_slice(b"DDS ");
        bytes[4..8].copy_from_slice(&124_u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&side.to_le_bytes());
        bytes[16..20].copy_from_slice(&side.to_le_bytes());
        bytes[84..88].copy_from_slice(b"DX10");
        bytes[128..132].copy_from_slice(&DXGI_RGBA8_UNORM.to_le_bytes());
        for _ in 0..side * side {
            bytes.extend_from_slice(&colour);
        }
        bytes
    }

    #[test]
    fn tile_names_parse_and_others_do_not() {
        assert_eq!(
            parse_tile_name(
                "lyshineui/worldtiles/newworld_vitaeeterna/map_l2_y012_x034.dds",
                LEVEL
            ),
            Some(TileName {
                level: 2,
                y: 12,
                x: 34
            })
        );
        assert_eq!(
            parse_tile_name(
                "LyShineUI/WorldTiles/NewWorld_VitaeEterna/MAP_L1_Y000_X055.DDS",
                LEVEL
            ),
            Some(TileName {
                level: 1,
                y: 0,
                x: 55
            })
        );
        for other in [
            "lyshineui/worldtiles/nw_dungeon_x/map_l1_y000_x000.dds",
            "lyshineui/worldtiles/newworld_vitaeeterna/map_l1_y000.dds",
            "lyshineui/worldtiles/newworld_vitaeeterna/map_lx_y000_x000.dds",
            "lyshineui/worldtiles/newworld_vitaeeterna/sub/map_l1_y000_x000.dds",
            "lyshineui/images/map/map_l1_y000_x000.dds",
        ] {
            assert_eq!(parse_tile_name(other, LEVEL), None, "{other}");
        }
    }

    #[test]
    fn a_level_index_counts_its_own_tiles() {
        let name = TileName {
            level: 3,
            y: 8,
            x: 52,
        };
        assert_eq!(name.grid_index(), (2, 13));
    }

    #[test]
    fn a_tile_is_written_as_png_at_its_level_index() {
        let out = tempfile::tempdir().expect("dir");
        let name = TileName {
            level: 2,
            y: 4,
            x: 6,
        };
        let format =
            write_tile(out.path(), &name, &rgba_dds(TILE_PX, [10, 20, 30, 255])).expect("write");
        assert_eq!(format, DdsFormat::Rgba8);
        let path = out.path().join("2").join("2_3.png");
        let decoder = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(&path).expect("tile file"),
        ));
        let mut reader = decoder.read_info().expect("png");
        let mut pixels = vec![0; reader.output_buffer_size().expect("size")];
        reader.next_frame(&mut pixels).expect("frame");
        assert_eq!(pixels[..4], [10, 20, 30, 255]);
    }

    #[test]
    fn a_tile_that_does_not_decode_is_an_error() {
        let out = tempfile::tempdir().expect("dir");
        let name = TileName {
            level: 1,
            y: 0,
            x: 0,
        };
        assert!(matches!(
            write_tile(out.path(), &name, b"not a texture"),
            Err(MapTilesError::Dds { .. })
        ));
    }
}
