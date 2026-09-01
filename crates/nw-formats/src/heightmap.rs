// SPDX-License-Identifier: MIT OR Apache-2.0
//! Region heightmaps (`region.heightmap`, M1b spec section 3.1): little-endian TIFF, one image
//! of 2048 x 2048 16-bit samples, LZW, predictor 1 or 2, strips. The Photometric tag is ignored,
//! because 59 files of the pinned client set it to RGB with one sample, which generic TIFF
//! readers refuse.

use weezl::decode::Configuration;
use weezl::{BitOrder, LzwStatus};

/// Rows and columns of a heightmap.
pub const HEIGHTMAP_SIZE: usize = 2048;
/// First 4 bytes of a little-endian TIFF.
pub const TIFF_LITTLE_ENDIAN_MAGIC: [u8; 4] = *b"II*\0";

const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_PLANAR_CONFIGURATION: u16 = 284;
const TAG_PREDICTOR: u16 = 317;
const TAG_TILE_WIDTH: u16 = 322;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const COMPRESSION_LZW: u32 = 5;
const PREDICTOR_NONE: u32 = 1;
const PREDICTOR_HORIZONTAL: u32 = 2;
const BITS_PER_SAMPLE: u32 = 16;
const SAMPLES_PER_PIXEL: u32 = 1;
const PLANAR_CHUNKY: u32 = 1;
const PHOTOMETRIC_BLACK_IS_ZERO: u32 = 1;
/// Rows per strip that [`encode_heightmap`] writes (as in 67 of the 100 pinned files).
const ENCODED_ROWS_PER_STRIP: usize = 4;
const IFD_NEXT_OFFSET_SIZE: usize = size_of::<u32>();
const TIFF_WORD_SIZE: usize = size_of::<u16>();
/// TIFF LZW codes start at 8 bits.
const LZW_MIN_CODE_SIZE: u8 = 8;
const IFD_ENTRY_SIZE: usize = 12;
const INLINE_VALUE_SIZE: usize = 4;
const SAMPLE_SIZE: usize = size_of::<u16>();
const ROW_BYTES: usize = HEIGHTMAP_SIZE * SAMPLE_SIZE;
const SAMPLE_COUNT: usize = HEIGHTMAP_SIZE * HEIGHTMAP_SIZE;

/// Decoded heightmap samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heightmap {
    /// `HEIGHTMAP_SIZE` rows of `HEIGHTMAP_SIZE` raw samples, row 0 first (as in the file).
    pub samples: Vec<u16>,
}

/// Error from heightmap reading.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HeightmapError {
    /// The data ends before a structure it points to.
    #[error("heightmap is truncated")]
    Truncated,
    /// The file is not a little-endian TIFF.
    #[error("not a little-endian TIFF")]
    BadMagic,
    /// A required tag is missing.
    #[error("TIFF tag {tag} is missing")]
    MissingTag {
        /// Tag number.
        tag: u16,
    },
    /// A tag has a type other than SHORT or LONG.
    #[error("TIFF tag {tag} has unsupported type {field_type}")]
    UnsupportedTagType {
        /// Tag number.
        tag: u16,
        /// TIFF field type.
        field_type: u16,
    },
    /// The image is not 2048 x 2048.
    #[error("heightmap is {width} x {height}, expected {HEIGHTMAP_SIZE} x {HEIGHTMAP_SIZE}")]
    UnsupportedSize {
        /// Image width.
        width: u32,
        /// Image height.
        height: u32,
    },
    /// A tag value other than the one supported profile.
    #[error("TIFF tag {tag} = {value} is not supported")]
    UnsupportedValue {
        /// Tag number.
        tag: u16,
        /// Value in the file.
        value: u32,
    },
    /// The image uses tiles, not strips.
    #[error("tiled TIFF is not supported")]
    Tiled,
    /// Strip offsets and byte counts do not fit the image.
    #[error("strip table has {offsets} offsets and {counts} byte counts, expected {expected}")]
    StripCount {
        /// Number of strip offsets.
        offsets: usize,
        /// Number of strip byte counts.
        counts: usize,
        /// Strips the image needs.
        expected: usize,
    },
    /// The samples to encode are not 2048 x 2048.
    #[error("heightmap has {actual} samples, expected {SAMPLE_COUNT}")]
    SampleCount {
        /// Number of samples.
        actual: usize,
    },
    /// A tag holds more values than the image can use.
    #[error("TIFF tag {tag} has {count} values, at most {limit} allowed")]
    TooManyValues {
        /// Tag number.
        tag: u16,
        /// Values in the file.
        count: usize,
        /// Largest allowed count.
        limit: usize,
    },
    /// LZW data is not valid.
    #[error("strip {strip}: LZW data is not valid")]
    Lzw {
        /// Strip index.
        strip: usize,
    },
    /// A strip decodes to fewer bytes than its rows need.
    #[error("strip {strip} decodes to {actual} bytes, expected {expected}")]
    ShortStrip {
        /// Strip index.
        strip: usize,
        /// Bytes the rows need.
        expected: usize,
        /// Bytes decoded.
        actual: usize,
    },
}

/// One IFD entry.
#[derive(Debug, Clone, Copy)]
struct IfdEntry {
    tag: u16,
    field_type: u16,
    count: u32,
    /// The 4 value bytes: the value itself, or an offset to it.
    value: [u8; INLINE_VALUE_SIZE],
}

/// Strip layout of a written heightmap.
struct TiffLayout {
    /// `None` writes one strip and no RowsPerStrip tag.
    rows_per_strip: Option<usize>,
    predictor: u32,
}

const HEIGHTMAP_LAYOUT: TiffLayout = TiffLayout {
    rows_per_strip: Some(ENCODED_ROWS_PER_STRIP),
    predictor: PREDICTOR_HORIZONTAL,
};

/// Reads a `region.heightmap` file.
pub fn parse_heightmap(bytes: &[u8]) -> Result<Heightmap, HeightmapError> {
    if bytes.get(..TIFF_LITTLE_ENDIAN_MAGIC.len()) != Some(&TIFF_LITTLE_ENDIAN_MAGIC[..]) {
        return Err(HeightmapError::BadMagic);
    }
    let ifd_offset = read_u32(bytes, TIFF_LITTLE_ENDIAN_MAGIC.len())? as usize;
    let entries = read_ifd(bytes, ifd_offset)?;
    let find = |tag| entries.iter().find(|entry| entry.tag == tag);
    if find(TAG_TILE_WIDTH).is_some() {
        return Err(HeightmapError::Tiled);
    }
    let scalar = |tag| -> Result<Option<u32>, HeightmapError> {
        find(tag)
            .map(|entry| scalar_value(bytes, entry))
            .transpose()
    };
    let required = |tag| scalar(tag)?.ok_or(HeightmapError::MissingTag { tag });
    let width = required(TAG_IMAGE_WIDTH)?;
    let height = required(TAG_IMAGE_LENGTH)?;
    if width as usize != HEIGHTMAP_SIZE || height as usize != HEIGHTMAP_SIZE {
        return Err(HeightmapError::UnsupportedSize { width, height });
    }
    expect_value(
        TAG_BITS_PER_SAMPLE,
        required(TAG_BITS_PER_SAMPLE)?,
        &[BITS_PER_SAMPLE],
    )?;
    expect_value(
        TAG_COMPRESSION,
        required(TAG_COMPRESSION)?,
        &[COMPRESSION_LZW],
    )?;
    let samples_per_pixel = scalar(TAG_SAMPLES_PER_PIXEL)?.unwrap_or(SAMPLES_PER_PIXEL);
    expect_value(
        TAG_SAMPLES_PER_PIXEL,
        samples_per_pixel,
        &[SAMPLES_PER_PIXEL],
    )?;
    let planar = scalar(TAG_PLANAR_CONFIGURATION)?.unwrap_or(PLANAR_CHUNKY);
    expect_value(TAG_PLANAR_CONFIGURATION, planar, &[PLANAR_CHUNKY])?;
    let predictor = scalar(TAG_PREDICTOR)?.unwrap_or(PREDICTOR_NONE);
    expect_value(
        TAG_PREDICTOR,
        predictor,
        &[PREDICTOR_NONE, PREDICTOR_HORIZONTAL],
    )?;
    let rows_per_strip = scalar(TAG_ROWS_PER_STRIP)?.map_or(HEIGHTMAP_SIZE, |rows| {
        (rows as usize).clamp(1, HEIGHTMAP_SIZE)
    });
    let strips = HEIGHTMAP_SIZE.div_ceil(rows_per_strip);
    let offsets = array_values(bytes, find(TAG_STRIP_OFFSETS), TAG_STRIP_OFFSETS, strips)?;
    let counts = array_values(
        bytes,
        find(TAG_STRIP_BYTE_COUNTS),
        TAG_STRIP_BYTE_COUNTS,
        strips,
    )?;
    if offsets.len() != strips || counts.len() != strips {
        return Err(HeightmapError::StripCount {
            offsets: offsets.len(),
            counts: counts.len(),
            expected: strips,
        });
    }
    let mut raw = vec![0_u8; SAMPLE_COUNT * SAMPLE_SIZE];
    for (strip, (&offset, &count)) in offsets.iter().zip(&counts).enumerate() {
        let first_row = strip * rows_per_strip;
        let rows = rows_per_strip.min(HEIGHTMAP_SIZE - first_row);
        let output = &mut raw[first_row * ROW_BYTES..(first_row + rows) * ROW_BYTES];
        let input = slice(bytes, offset as usize, count as usize)?;
        decode_strip(strip, input, output)?;
    }
    let mut samples: Vec<u16> = raw
        .chunks_exact(SAMPLE_SIZE)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if predictor == PREDICTOR_HORIZONTAL {
        for row in samples.chunks_exact_mut(HEIGHTMAP_SIZE) {
            for column in 1..HEIGHTMAP_SIZE {
                row[column] = row[column].wrapping_add(row[column - 1]);
            }
        }
    }
    Ok(Heightmap { samples })
}

/// Writes `map` as a heightmap file in the profile [`parse_heightmap`] reads: 4 rows per strip,
/// LZW, horizontal predictor.
pub fn encode_heightmap(map: &Heightmap) -> Result<Vec<u8>, HeightmapError> {
    write_tiff(&map.samples, &HEIGHTMAP_LAYOUT)
}

fn write_tiff(samples: &[u16], layout: &TiffLayout) -> Result<Vec<u8>, HeightmapError> {
    if samples.len() != SAMPLE_COUNT {
        return Err(HeightmapError::SampleCount {
            actual: samples.len(),
        });
    }
    let rows_per_strip = layout.rows_per_strip.unwrap_or(HEIGHTMAP_SIZE);
    let mut out = TIFF_LITTLE_ENDIAN_MAGIC.to_vec();
    out.extend_from_slice(&[0; size_of::<u32>()]);
    let mut offsets = Vec::new();
    let mut counts = Vec::new();
    for (strip, rows) in samples.chunks(rows_per_strip * HEIGHTMAP_SIZE).enumerate() {
        let mut raw = Vec::with_capacity(rows.len() * SAMPLE_SIZE);
        for row in rows.chunks_exact(HEIGHTMAP_SIZE) {
            for (column, &sample) in row.iter().enumerate() {
                let stored = if layout.predictor == PREDICTOR_HORIZONTAL && column > 0 {
                    sample.wrapping_sub(row[column - 1])
                } else {
                    sample
                };
                raw.extend_from_slice(&stored.to_le_bytes());
            }
        }
        let packed =
            weezl::encode::Encoder::with_tiff_size_switch(BitOrder::Msb, LZW_MIN_CODE_SIZE)
                .encode(&raw)
                .map_err(|_| HeightmapError::Lzw { strip })?;
        offsets.push(file_offset(&out));
        counts.push(file_offset(&packed));
        out.extend_from_slice(&packed);
    }
    let mut entries = vec![
        short_entry(TAG_IMAGE_WIDTH, HEIGHTMAP_SIZE as u32),
        short_entry(TAG_IMAGE_LENGTH, HEIGHTMAP_SIZE as u32),
        short_entry(TAG_BITS_PER_SAMPLE, BITS_PER_SAMPLE),
        short_entry(TAG_COMPRESSION, COMPRESSION_LZW),
        short_entry(TAG_PHOTOMETRIC, PHOTOMETRIC_BLACK_IS_ZERO),
        long_entry(TAG_STRIP_OFFSETS, &offsets, &mut out),
        short_entry(TAG_SAMPLES_PER_PIXEL, SAMPLES_PER_PIXEL),
    ];
    if let Some(rows) = layout.rows_per_strip {
        entries.push(short_entry(TAG_ROWS_PER_STRIP, rows as u32));
    }
    entries.push(long_entry(TAG_STRIP_BYTE_COUNTS, &counts, &mut out));
    entries.push(short_entry(TAG_PREDICTOR, layout.predictor));
    align_to_word(&mut out);
    let ifd_offset = file_offset(&out);
    out[TIFF_LITTLE_ENDIAN_MAGIC.len()..TIFF_LITTLE_ENDIAN_MAGIC.len() + size_of::<u32>()]
        .copy_from_slice(&ifd_offset.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for entry in &entries {
        out.extend_from_slice(&entry.tag.to_le_bytes());
        out.extend_from_slice(&entry.field_type.to_le_bytes());
        out.extend_from_slice(&entry.count.to_le_bytes());
        out.extend_from_slice(&entry.value);
    }
    out.extend_from_slice(&[0; IFD_NEXT_OFFSET_SIZE]);
    Ok(out)
}

/// A heightmap file is far below 4 GiB, so every offset fits in a TIFF LONG.
fn file_offset(bytes: &[u8]) -> u32 {
    bytes.len() as u32
}

fn short_entry(tag: u16, value: u32) -> IfdEntry {
    let mut inline = [0; INLINE_VALUE_SIZE];
    inline[..size_of::<u16>()].copy_from_slice(&(value as u16).to_le_bytes());
    IfdEntry {
        tag,
        field_type: TYPE_SHORT,
        count: 1,
        value: inline,
    }
}

/// A LONG array entry. One value is stored inline; more values are appended to `out`.
fn long_entry(tag: u16, values: &[u32], out: &mut Vec<u8>) -> IfdEntry {
    let value = match values {
        [single] => single.to_le_bytes(),
        _ => {
            align_to_word(out);
            let offset = file_offset(out);
            for value in values {
                out.extend_from_slice(&value.to_le_bytes());
            }
            offset.to_le_bytes()
        }
    };
    IfdEntry {
        tag,
        field_type: TYPE_LONG,
        count: values.len() as u32,
        value,
    }
}

/// TIFF offsets must be even.
fn align_to_word(out: &mut Vec<u8>) {
    if !out.len().is_multiple_of(TIFF_WORD_SIZE) {
        out.push(0);
    }
}

/// Decodes one LZW strip into exactly `output.len()` bytes.
fn decode_strip(strip: usize, input: &[u8], output: &mut [u8]) -> Result<(), HeightmapError> {
    let mut decoder = Configuration::with_tiff_size_switch(BitOrder::Msb, LZW_MIN_CODE_SIZE)
        .with_yield_on_full_buffer(true)
        .build();
    let mut written = 0;
    let mut read = 0;
    while written < output.len() {
        let result = decoder.decode_bytes(&input[read..], &mut output[written..]);
        read += result.consumed_in;
        written += result.consumed_out;
        match result.status {
            Ok(LzwStatus::Ok) if result.consumed_in + result.consumed_out > 0 => {}
            Ok(_) => break,
            Err(_) => return Err(HeightmapError::Lzw { strip }),
        }
    }
    if written == output.len() {
        Ok(())
    } else {
        Err(HeightmapError::ShortStrip {
            strip,
            expected: output.len(),
            actual: written,
        })
    }
}

fn read_ifd(bytes: &[u8], offset: usize) -> Result<Vec<IfdEntry>, HeightmapError> {
    let count = usize::from(read_u16(bytes, offset)?);
    let table = slice(bytes, offset + size_of::<u16>(), count * IFD_ENTRY_SIZE)?;
    Ok(table
        .chunks_exact(IFD_ENTRY_SIZE)
        .map(|entry| IfdEntry {
            tag: u16::from_le_bytes([entry[0], entry[1]]),
            field_type: u16::from_le_bytes([entry[2], entry[3]]),
            count: u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]),
            value: [entry[8], entry[9], entry[10], entry[11]],
        })
        .collect())
}

fn value_size(entry: &IfdEntry) -> Result<usize, HeightmapError> {
    match entry.field_type {
        TYPE_SHORT => Ok(size_of::<u16>()),
        TYPE_LONG => Ok(size_of::<u32>()),
        field_type => Err(HeightmapError::UnsupportedTagType {
            tag: entry.tag,
            field_type,
        }),
    }
}

/// The first value of a SHORT or LONG tag.
fn scalar_value(bytes: &[u8], entry: &IfdEntry) -> Result<u32, HeightmapError> {
    let values = array_values(bytes, Some(entry), entry.tag, 1)?;
    values
        .first()
        .copied()
        .ok_or(HeightmapError::MissingTag { tag: entry.tag })
}

/// All values of a SHORT or LONG tag. Refuses more than `limit` values before it reads them.
fn array_values(
    bytes: &[u8],
    entry: Option<&IfdEntry>,
    tag: u16,
    limit: usize,
) -> Result<Vec<u32>, HeightmapError> {
    let entry = entry.ok_or(HeightmapError::MissingTag { tag })?;
    let size = value_size(entry)?;
    let count = entry.count as usize;
    if count > limit {
        return Err(HeightmapError::TooManyValues { tag, count, limit });
    }
    let data = if count * size <= INLINE_VALUE_SIZE {
        &entry.value[..count * size]
    } else {
        slice(
            bytes,
            u32::from_le_bytes(entry.value) as usize,
            count * size,
        )?
    };
    Ok(data
        .chunks_exact(size)
        .map(|value| match value {
            [low, high] => u32::from(u16::from_le_bytes([*low, *high])),
            _ => u32::from_le_bytes([value[0], value[1], value[2], value[3]]),
        })
        .collect())
}

fn expect_value(tag: u16, value: u32, allowed: &[u32]) -> Result<(), HeightmapError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(HeightmapError::UnsupportedValue { tag, value })
    }
}

fn slice(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8], HeightmapError> {
    offset
        .checked_add(length)
        .and_then(|end| bytes.get(offset..end))
        .ok_or(HeightmapError::Truncated)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, HeightmapError> {
    let data = slice(bytes, offset, size_of::<u16>())?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, HeightmapError> {
    let data = slice(bytes, offset, size_of::<u32>())?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;

    /// Offset of the 4 value bytes inside an IFD entry.
    const ENTRY_VALUE_OFFSET: usize = 8;
    const SMALL_SIZE: u32 = 1024;
    const UNKNOWN_TAG: u16 = 65_000;
    const PHOTOMETRIC_RGB: u32 = 2;
    const BYTE_SAMPLES: u32 = 8;
    const NO_COMPRESSION: u32 = 1;

    fn samples() -> Vec<u16> {
        (0..SAMPLE_COUNT)
            .map(|index| {
                let (row, column) = (index / HEIGHTMAP_SIZE, index % HEIGHTMAP_SIZE);
                (row * 3 + column * 5) as u16
            })
            .collect()
    }

    /// The default encoding, built once for all tests.
    fn encoded() -> &'static [u8] {
        static ENCODED: OnceLock<Vec<u8>> = OnceLock::new();
        ENCODED.get_or_init(|| encode_heightmap(&Heightmap { samples: samples() }).unwrap())
    }

    fn entry_offset(bytes: &[u8], tag: u16) -> usize {
        let ifd = read_u32(bytes, TIFF_LITTLE_ENDIAN_MAGIC.len()).unwrap() as usize;
        let count = usize::from(read_u16(bytes, ifd).unwrap());
        (0..count)
            .map(|index| ifd + size_of::<u16>() + index * IFD_ENTRY_SIZE)
            .find(|&offset| read_u16(bytes, offset).unwrap() == tag)
            .unwrap_or_else(|| panic!("tag {tag} is not in the test file"))
    }

    /// Replaces the inline value of a SHORT or one-value LONG tag.
    fn with_tag(bytes: &[u8], tag: u16, value: u32) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        let offset = entry_offset(&bytes, tag) + ENTRY_VALUE_OFFSET;
        bytes[offset..offset + INLINE_VALUE_SIZE].copy_from_slice(&value.to_le_bytes());
        bytes
    }

    fn with_tag_renamed(bytes: &[u8], from: u16, to: u16) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        let offset = entry_offset(&bytes, from);
        bytes[offset..offset + size_of::<u16>()].copy_from_slice(&to.to_le_bytes());
        bytes
    }

    #[test]
    fn round_trip_keeps_every_sample() {
        assert_eq!(parse_heightmap(encoded()).unwrap().samples, samples());
    }

    #[test]
    fn reads_one_strip_without_rows_per_strip_or_predictor() {
        let layout = TiffLayout {
            rows_per_strip: None,
            predictor: PREDICTOR_NONE,
        };
        let bytes = write_tiff(&samples(), &layout).unwrap();
        // Without these tags the reader must use the TIFF defaults (1 sample, predictor 1).
        let bytes = with_tag_renamed(&bytes, TAG_SAMPLES_PER_PIXEL, UNKNOWN_TAG);
        let bytes = with_tag_renamed(&bytes, TAG_PREDICTOR, UNKNOWN_TAG);
        assert_eq!(parse_heightmap(&bytes).unwrap().samples, samples());
    }

    #[test]
    fn photometric_rgb_is_ignored() {
        let bytes = with_tag(encoded(), TAG_PHOTOMETRIC, PHOTOMETRIC_RGB);
        assert_eq!(parse_heightmap(&bytes).unwrap().samples, samples());
    }

    #[test]
    fn predictor_1_keeps_the_stored_differences() {
        let bytes = with_tag(encoded(), TAG_PREDICTOR, PREDICTOR_NONE);
        let mut expected = samples();
        for row in expected.chunks_exact_mut(HEIGHTMAP_SIZE) {
            for column in (1..HEIGHTMAP_SIZE).rev() {
                row[column] = row[column].wrapping_sub(row[column - 1]);
            }
        }
        assert_eq!(parse_heightmap(&bytes).unwrap().samples, expected);
    }

    #[test]
    fn other_size_is_refused() {
        let bytes = with_tag(encoded(), TAG_IMAGE_WIDTH, SMALL_SIZE);
        assert_eq!(
            parse_heightmap(&bytes),
            Err(HeightmapError::UnsupportedSize {
                width: SMALL_SIZE,
                height: HEIGHTMAP_SIZE as u32
            })
        );
    }

    #[test]
    fn other_bit_depth_and_compression_are_refused() {
        let bytes = with_tag(encoded(), TAG_BITS_PER_SAMPLE, BYTE_SAMPLES);
        assert_eq!(
            parse_heightmap(&bytes),
            Err(HeightmapError::UnsupportedValue {
                tag: TAG_BITS_PER_SAMPLE,
                value: BYTE_SAMPLES
            })
        );
        let bytes = with_tag(encoded(), TAG_COMPRESSION, NO_COMPRESSION);
        assert_eq!(
            parse_heightmap(&bytes),
            Err(HeightmapError::UnsupportedValue {
                tag: TAG_COMPRESSION,
                value: NO_COMPRESSION
            })
        );
    }

    #[test]
    fn tiles_are_refused() {
        let bytes = with_tag_renamed(encoded(), TAG_PREDICTOR, TAG_TILE_WIDTH);
        assert_eq!(parse_heightmap(&bytes), Err(HeightmapError::Tiled));
    }

    #[test]
    fn missing_width_is_refused() {
        let bytes = with_tag_renamed(encoded(), TAG_IMAGE_WIDTH, UNKNOWN_TAG);
        assert_eq!(
            parse_heightmap(&bytes),
            Err(HeightmapError::MissingTag {
                tag: TAG_IMAGE_WIDTH
            })
        );
    }

    #[test]
    fn other_byte_order_is_refused() {
        let mut bytes = encoded().to_vec();
        bytes[..TIFF_LITTLE_ENDIAN_MAGIC.len()].copy_from_slice(b"MM\0*");
        assert_eq!(parse_heightmap(&bytes), Err(HeightmapError::BadMagic));
    }

    #[test]
    fn truncated_files_are_refused() {
        let bytes = encoded();
        let without_last_entry = bytes.len() - IFD_NEXT_OFFSET_SIZE - 1;
        assert_eq!(
            parse_heightmap(&bytes[..without_last_entry]),
            Err(HeightmapError::Truncated)
        );
        let inside_header = TIFF_LITTLE_ENDIAN_MAGIC.len() + 1;
        assert_eq!(
            parse_heightmap(&bytes[..inside_header]),
            Err(HeightmapError::Truncated)
        );
    }

    #[test]
    fn empty_strip_is_short() {
        let layout = TiffLayout {
            rows_per_strip: None,
            predictor: PREDICTOR_HORIZONTAL,
        };
        let bytes = write_tiff(&samples(), &layout).unwrap();
        let bytes = with_tag(&bytes, TAG_STRIP_BYTE_COUNTS, 0);
        assert_eq!(
            parse_heightmap(&bytes),
            Err(HeightmapError::ShortStrip {
                strip: 0,
                expected: SAMPLE_COUNT * SAMPLE_SIZE,
                actual: 0
            })
        );
    }

    #[test]
    fn strip_table_must_fit_the_rows_per_strip() {
        let bytes = with_tag(encoded(), TAG_ROWS_PER_STRIP, 1);
        assert_eq!(
            parse_heightmap(&bytes),
            Err(HeightmapError::StripCount {
                offsets: HEIGHTMAP_SIZE / ENCODED_ROWS_PER_STRIP,
                counts: HEIGHTMAP_SIZE / ENCODED_ROWS_PER_STRIP,
                expected: HEIGHTMAP_SIZE
            })
        );
        let rows = 2 * ENCODED_ROWS_PER_STRIP;
        let bytes = with_tag(encoded(), TAG_ROWS_PER_STRIP, rows as u32);
        assert_eq!(
            parse_heightmap(&bytes),
            Err(HeightmapError::TooManyValues {
                tag: TAG_STRIP_OFFSETS,
                count: HEIGHTMAP_SIZE / ENCODED_ROWS_PER_STRIP,
                limit: HEIGHTMAP_SIZE / rows
            })
        );
    }

    #[test]
    fn encoder_needs_a_full_grid() {
        assert_eq!(
            encode_heightmap(&Heightmap {
                samples: vec![0; 3]
            }),
            Err(HeightmapError::SampleCount { actual: 3 })
        );
    }
}
