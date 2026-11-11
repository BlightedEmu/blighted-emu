// SPDX-License-Identifier: AGPL-3.0-or-later
//! A region's `region.distribution` file: the trees, stones and plants the client places in the
//! region, each a slice with a variant, a position on the region's square, a turn and a scale.
//!
//! Layout (little-endian; the build's empty dungeon files read to their end with it, and the
//! collision build checks that each open-world file ends where its tables end):
//!
//! - an optional mark `0xB9D6` and a version (i16);
//! - the slice count (u16), then that many slice names and as many variant names (each a u8
//!   size and its bytes);
//! - the row count (u32), then per row the slice index (u16), then per row the packed position
//!   (u32), then the packed turn (u32), then the scale (u8, hundredths), then with version 1 or
//!   more the height source (u8);
//! - two tails, each a count (u32) and five bytes per entry, which hold no placement.
//!
//! A packed position is x in its high 16 bits and y in its low 16 bits, each a share of the
//! region's 2048 m. A packed turn is a quaternion: x in bits 21 to 30 and y in bits 11 to 20
//! (`/ 255 - 1`), z in bits 0 to 10 (`/ 511 - 1`), w from the rest, negative when bit 31 is set.
//! The height source picks the row's height: 0 the terrain, 2 the water surface, any other the
//! higher of the two.

/// The optional mark at the start.
const MARK: u16 = 0xB9D6;
/// The first version with a height source per row.
const HEIGHT_SOURCE_VERSION: i16 = 1;
/// The bytes of one tail entry.
const TAIL_ENTRY_LEN: usize = 5;
/// The tails after the rows.
const TAILS: usize = 2;
/// The side of a region, m.
pub const REGION_SIDE_M: f32 = 2048.0;
/// The largest 16-bit share.
const SHARE_MAX: f32 = 65_535.0;
/// The packed turn's fields.
const QUAT_X_SHIFT: u32 = 21;
const QUAT_Y_SHIFT: u32 = 11;
const QUAT_XY_MASK: u32 = 0x3FF;
const QUAT_Z_MASK: u32 = 0x7FF;
const QUAT_XY_SCALE: f32 = 255.0;
const QUAT_Z_SCALE: f32 = 511.0;
const QUAT_W_SIGN_BIT: u32 = 31;
/// Hundredths of the scale byte.
const SCALE_UNIT: f32 = 0.01;
/// The 16-bit halves of a packed position.
const HALF_BITS: u32 = 16;
const LOW_HALF: u32 = 0xFFFF;

/// Which height a row stands at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightSource {
    /// The terrain's.
    Terrain,
    /// The water surface's (the terrain's where there is no water).
    Water,
    /// The higher of the two.
    Higher,
}

impl HeightSource {
    const TERRAIN: u8 = 0;
    const WATER: u8 = 2;

    const fn of(value: u8) -> Self {
        match value {
            Self::TERRAIN => Self::Terrain,
            Self::WATER => Self::Water,
            _ => Self::Higher,
        }
    }

    /// The height of a row over `terrain` with the water surface `water` there.
    pub fn height(self, terrain: f32, water: Option<f32>) -> f32 {
        match (self, water) {
            (Self::Terrain, _) | (_, None) => terrain,
            (Self::Water, Some(water)) => water,
            (Self::Higher, Some(water)) => terrain.max(water),
        }
    }
}

/// One placement of a distribution file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// The slice (index into [`Distribution::slices`] and [`Distribution::variants`]).
    pub slice: u16,
    /// Its place on the region's square, m from the region's corner.
    pub at: [f32; 2],
    /// Its turn: a unit quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Its scale.
    pub scale: f32,
    /// Its height source.
    pub height: HeightSource,
}

/// A distribution file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Distribution {
    /// The slice names.
    pub slices: Vec<String>,
    /// The variant name of each slice (empty: none).
    pub variants: Vec<String>,
    /// The placements.
    pub rows: Vec<Row>,
}

/// A distribution file that does not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DistributionError {
    /// The file ends inside a table.
    #[error("distribution ends at byte {at}, inside a table")]
    Truncated {
        /// Where the read stopped.
        at: usize,
    },
    /// Bytes follow the tables.
    #[error("distribution has {len} bytes, its tables end at {end}")]
    TrailingBytes {
        /// The file size.
        len: usize,
        /// Where the tables end.
        end: usize,
    },
    /// A row names a slice past the slice table.
    #[error("distribution row {row} names slice {slice} of {count}")]
    BadSlice {
        /// The row.
        row: usize,
        /// Its slice index.
        slice: u16,
        /// The slice count.
        count: usize,
    },
}

/// A reader over the bytes of a file.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], DistributionError> {
        let end = self
            .at
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(DistributionError::Truncated { at: self.at })?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DistributionError> {
        let at = self.at;
        self.take(N)?
            .try_into()
            .map_err(|_| DistributionError::Truncated { at })
    }

    fn u16(&mut self) -> Result<u16, DistributionError> {
        self.array().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Result<u32, DistributionError> {
        self.array().map(u32::from_le_bytes)
    }

    fn name(&mut self) -> Result<String, DistributionError> {
        let [len] = self.array()?;
        Ok(String::from_utf8_lossy(self.take(usize::from(len))?).into_owned())
    }

    fn len_of(&mut self, count: usize, width: usize) -> Result<&'a [u8], DistributionError> {
        let at = self.at;
        self.take(
            count
                .checked_mul(width)
                .ok_or(DistributionError::Truncated { at })?,
        )
    }
}

/// The quaternion of a packed turn.
fn unpack_rotation(packed: u32) -> [f32; 4] {
    // Fields of 10 and 11 bits.
    #[allow(clippy::cast_precision_loss, reason = "fields of 10 and 11 bits")]
    let field = |shift: u32, mask: u32, scale: f32| ((packed >> shift) & mask) as f32 / scale - 1.0;
    let x = field(QUAT_X_SHIFT, QUAT_XY_MASK, QUAT_XY_SCALE);
    let y = field(QUAT_Y_SHIFT, QUAT_XY_MASK, QUAT_XY_SCALE);
    let z = field(0, QUAT_Z_MASK, QUAT_Z_SCALE);
    let mut w = (1.0 - (x * x + y * y + z * z).min(1.0)).sqrt();
    if packed >> QUAT_W_SIGN_BIT != 0 {
        w = -w;
    }
    let length = (x * x + y * y + z * z + w * w).sqrt();
    if length > 0.0 {
        [x / length, y / length, z / length, w / length]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// The place of a packed position on the region's square.
fn unpack_position(packed: u32) -> [f32; 2] {
    // Shares of 16 bits.
    #[allow(clippy::cast_precision_loss, reason = "shares of 16 bits")]
    let share = |value: u32| value as f32 * REGION_SIDE_M / SHARE_MAX;
    [share(packed >> HALF_BITS), share(packed & LOW_HALF)]
}

/// Reads a distribution file.
///
/// # Errors
///
/// Returns [`DistributionError`] when a table is cut short, bytes follow the tables, or a row
/// names a slice the file does not have.
pub fn parse_distribution(bytes: &[u8]) -> Result<Distribution, DistributionError> {
    let mut reader = Reader { bytes, at: 0 };
    let mut version = None;
    if bytes.len() >= size_of::<u16>() && u16::from_le_bytes([bytes[0], bytes[1]]) == MARK {
        reader.u16()?;
        version = Some(i16::from_le_bytes(reader.array()?));
    }
    let count = usize::from(reader.u16()?);
    let slices = (0..count)
        .map(|_| reader.name())
        .collect::<Result<Vec<_>, _>>()?;
    let variants = (0..count)
        .map(|_| reader.name())
        .collect::<Result<Vec<_>, _>>()?;
    let rows = reader.u32()? as usize;
    let indexes = reader.len_of(rows, size_of::<u16>())?;
    let positions = reader.len_of(rows, size_of::<u32>())?;
    let rotations = reader.len_of(rows, size_of::<u32>())?;
    let scales = reader.len_of(rows, size_of::<u8>())?;
    let sources = if version.is_some_and(|version| version >= HEIGHT_SOURCE_VERSION) {
        Some(reader.len_of(rows, size_of::<u8>())?)
    } else {
        None
    };
    for _ in 0..TAILS {
        let entries = reader.u32()? as usize;
        reader.len_of(entries, TAIL_ENTRY_LEN)?;
    }
    if reader.at != bytes.len() {
        return Err(DistributionError::TrailingBytes {
            len: bytes.len(),
            end: reader.at,
        });
    }
    let word = |table: &[u8], row: usize| {
        u32::from_le_bytes([
            table[row * 4],
            table[row * 4 + 1],
            table[row * 4 + 2],
            table[row * 4 + 3],
        ])
    };
    let rows = (0..rows)
        .map(|row| {
            let slice = u16::from_le_bytes([indexes[row * 2], indexes[row * 2 + 1]]);
            if usize::from(slice) >= count {
                return Err(DistributionError::BadSlice { row, slice, count });
            }
            Ok(Row {
                slice,
                at: unpack_position(word(positions, row)),
                rotation: unpack_rotation(word(rotations, row)),
                scale: f32::from(scales[row]) * SCALE_UNIT,
                height: HeightSource::of(sources.map_or(HeightSource::TERRAIN, |table| table[row])),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Distribution {
        slices,
        variants,
        rows,
    })
}

/// The corner of region folder `name` (`r_+XX_+YY`: x = XX * 2048, y = YY * 2048); `None` for
/// another name.
pub fn region_origin(name: &str) -> Option<[f32; 2]> {
    let (x, y) = name.strip_prefix("r_")?.split_once('_')?;
    let cell = |text: &str| {
        text.parse::<i16>()
            .ok()
            .map(|cell| f32::from(cell) * REGION_SIDE_M)
    };
    Some([cell(x)?, cell(y)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of version 1 with one slice and two rows.
    fn sample() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(MARK.to_le_bytes());
        bytes.extend(1i16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        for name in ["slices/tree.dynamicslice", "Oak_01"] {
            bytes.push(u8::try_from(name.len()).expect("short"));
            bytes.extend(name.as_bytes());
        }
        bytes.extend(2u32.to_le_bytes());
        bytes.extend([0u16, 0].iter().flat_map(|index| index.to_le_bytes()));
        // (1024 m, 0) and (2048 m, 2048 m).
        bytes.extend(
            [0x7FFF_8000u32 & 0xFFFF_0000, 0xFFFF_FFFF]
                .iter()
                .flat_map(|p| p.to_le_bytes()),
        );
        // No turn: x, y and z at their middle (255, 255, 511), w positive.
        let none = (255u32 << QUAT_X_SHIFT) | (255 << QUAT_Y_SHIFT) | 511;
        bytes.extend(
            [none, none | (1 << QUAT_W_SIGN_BIT)]
                .iter()
                .flat_map(|r| r.to_le_bytes()),
        );
        bytes.extend([100u8, 150]);
        bytes.extend([0u8, 2]);
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([9u8; TAIL_ENTRY_LEN]);
        bytes.extend(0u32.to_le_bytes());
        bytes
    }

    #[test]
    fn a_file_reads_its_names_and_rows_to_its_end() {
        let file = parse_distribution(&sample()).expect("reads");
        assert_eq!(file.slices, ["slices/tree.dynamicslice"]);
        assert_eq!(file.variants, ["Oak_01"]);
        assert_eq!(file.rows.len(), 2);
        let first = file.rows[0];
        assert!((first.at[0] - 1024.0).abs() < 0.05 && first.at[1].abs() < 1e-6);
        assert!((first.rotation[3] - 1.0).abs() < 1e-6, "no turn");
        assert!((first.scale - 1.0).abs() < 1e-6);
        assert_eq!(first.height, HeightSource::Terrain);
        let second = file.rows[1];
        assert!((second.at[0] - REGION_SIDE_M).abs() < 1e-3);
        assert!((second.rotation[3] + 1.0).abs() < 1e-6, "the sign bit");
        assert!((second.scale - 1.5).abs() < 1e-6);
        assert_eq!(second.height, HeightSource::Water);
    }

    #[test]
    fn a_cut_file_or_extra_bytes_or_a_bad_slice_are_refused() {
        let bytes = sample();
        assert!(matches!(
            parse_distribution(&bytes[..bytes.len() - 1]),
            Err(DistributionError::Truncated { .. })
        ));
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(matches!(
            parse_distribution(&longer),
            Err(DistributionError::TrailingBytes { .. })
        ));
        let mut bad = bytes;
        let rows_at = 4 + 2 + 1 + "slices/tree.dynamicslice".len() + 1 + "Oak_01".len() + 4;
        bad[rows_at] = 7;
        assert!(matches!(
            parse_distribution(&bad),
            Err(DistributionError::BadSlice { slice: 7, .. })
        ));
    }

    #[test]
    fn an_old_file_without_a_mark_stands_on_the_terrain() {
        // The build's empty dungeon files: one empty slice name and variant, no rows.
        let mut bytes = 1u16.to_le_bytes().to_vec();
        bytes.extend([0u8, 0]);
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        let file = parse_distribution(&bytes).expect("reads");
        assert!(file.rows.is_empty());
        assert!((HeightSource::Higher.height(10.0, Some(12.0)) - 12.0).abs() < 1e-6);
        assert!((HeightSource::Water.height(10.0, None) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn a_region_folder_names_its_corner() {
        assert_eq!(region_origin("r_+04_+02"), Some([8192.0, 4096.0]));
        assert_eq!(region_origin("capitals"), None);
    }
}
