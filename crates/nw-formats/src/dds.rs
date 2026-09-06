// SPDX-License-Identifier: MIT OR Apache-2.0
//! DDS textures: the header (with the DX10 extension) and the top mip level decoded to RGBA8.
//! Block formats are decoded by `bcdec_rs`.

/// First four bytes of a DDS file.
pub const DDS_MAGIC: [u8; 4] = *b"DDS ";
/// The pixel format starts at this 32-bit word of the header (after the magic).
const PIXEL_FORMAT_WORD: usize = 18;
/// Data starts here without the DX10 header.
pub const DATA_OFFSET: usize = 128;
/// Data starts here with the DX10 header.
pub const DX10_DATA_OFFSET: usize = 148;
const HEIGHT_AT: usize = 12;
const WIDTH_AT: usize = 16;
const FOURCC_AT: usize = 4 + (PIXEL_FORMAT_WORD + 2) * 4;
const DXGI_AT: usize = DATA_OFFSET;
const BLOCK_SIDE: usize = 4;
const RGBA: usize = 4;
const SMALL_BLOCK: usize = 8;
const LARGE_BLOCK: usize = 16;
const DXGI_RGBA8_UNORM: u32 = 28;
const DXGI_RGBA8_SRGB: u32 = 29;
const DXGI_BC1_UNORM: u32 = 71;
const DXGI_BC1_SRGB: u32 = 72;
const DXGI_BC3_UNORM: u32 = 77;
const DXGI_BC3_SRGB: u32 = 78;
const DXGI_BC7_UNORM: u32 = 98;
const DXGI_BC7_SRGB: u32 = 99;

/// Decodes one 4x4 block into RGBA8 rows of the given pitch.
type BlockDecoder = fn(&[u8], &mut [u8], usize);

/// Pixel formats this reader decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DdsFormat {
    /// BC1 (DXT1).
    Bc1,
    /// BC3 (DXT5).
    Bc3,
    /// BC7.
    Bc7,
    /// 8-bit RGBA.
    Rgba8,
}

/// The size and format of a DDS image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsImage {
    /// Pixels.
    pub width: u32,
    /// Pixels.
    pub height: u32,
    /// Pixel format.
    pub format: DdsFormat,
}

/// A DDS file this reader cannot use.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DdsError {
    /// The file ends before its header or its data.
    #[error("DDS file is cut short")]
    Short,
    /// The file does not start with `DDS `.
    #[error("not a DDS file")]
    Magic,
    /// The pixel format is not decoded here.
    #[error("DDS format {0} is not supported")]
    Unsupported(String),
}

/// The image of a DDS header and the offset of its data.
///
/// # Errors
///
/// Returns [`DdsError`] for a short file, a wrong magic, or a format not decoded here.
pub fn dds_header(bytes: &[u8]) -> Result<(DdsImage, usize), DdsError> {
    let header = bytes.get(..DATA_OFFSET).ok_or(DdsError::Short)?;
    if header[..DDS_MAGIC.len()] != DDS_MAGIC {
        return Err(DdsError::Magic);
    }
    let word =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let fourcc: [u8; 4] = header[FOURCC_AT..FOURCC_AT + 4]
        .try_into()
        .map_err(|_| DdsError::Short)?;
    let (format, offset) = match &fourcc {
        b"DXT1" => (DdsFormat::Bc1, DATA_OFFSET),
        b"DXT5" => (DdsFormat::Bc3, DATA_OFFSET),
        b"DX10" => {
            if bytes.len() < DX10_DATA_OFFSET {
                return Err(DdsError::Short);
            }
            let format = match word(DXGI_AT) {
                DXGI_BC1_UNORM | DXGI_BC1_SRGB => DdsFormat::Bc1,
                DXGI_BC3_UNORM | DXGI_BC3_SRGB => DdsFormat::Bc3,
                DXGI_BC7_UNORM | DXGI_BC7_SRGB => DdsFormat::Bc7,
                DXGI_RGBA8_UNORM | DXGI_RGBA8_SRGB => DdsFormat::Rgba8,
                other => return Err(DdsError::Unsupported(format!("DXGI {other}"))),
            };
            (format, DX10_DATA_OFFSET)
        }
        other => {
            return Err(DdsError::Unsupported(
                String::from_utf8_lossy(other).into_owned(),
            ));
        }
    };
    let image = DdsImage {
        width: word(WIDTH_AT),
        height: word(HEIGHT_AT),
        format,
    };
    Ok((image, offset))
}

/// The top mip level of a DDS file as RGBA8 rows, top row first.
///
/// # Errors
///
/// Returns [`DdsError`] as [`dds_header`], and [`DdsError::Short`] when the data is cut short.
pub fn decode_dds(bytes: &[u8]) -> Result<(DdsImage, Vec<u8>), DdsError> {
    let (image, offset) = dds_header(bytes)?;
    let width = usize::try_from(image.width).map_err(|_| DdsError::Short)?;
    let height = usize::try_from(image.height).map_err(|_| DdsError::Short)?;
    let data = &bytes[offset..];
    if image.format == DdsFormat::Rgba8 {
        let len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(RGBA))
            .ok_or(DdsError::Short)?;
        let pixels = data.get(..len).ok_or(DdsError::Short)?;
        return Ok((image, pixels.to_vec()));
    }
    let (block_len, decode): (usize, BlockDecoder) = match image.format {
        DdsFormat::Bc1 => (SMALL_BLOCK, bcdec_rs::bc1),
        DdsFormat::Bc3 => (LARGE_BLOCK, bcdec_rs::bc3),
        DdsFormat::Bc7 => (LARGE_BLOCK, bcdec_rs::bc7),
        DdsFormat::Rgba8 => unreachable!("handled above"),
    };
    let columns = width.div_ceil(BLOCK_SIDE);
    let rows = height.div_ceil(BLOCK_SIDE);
    let needed = columns
        .checked_mul(rows)
        .and_then(|blocks| blocks.checked_mul(block_len))
        .ok_or(DdsError::Short)?;
    let blocks = data.get(..needed).ok_or(DdsError::Short)?;
    let padded_width = columns * BLOCK_SIDE;
    let pitch = padded_width * RGBA;
    let mut padded = vec![0_u8; pitch * rows * BLOCK_SIDE];
    for (index, block) in blocks.chunks_exact(block_len).enumerate() {
        let (row, column) = (index / columns, index % columns);
        let start = row * BLOCK_SIDE * pitch + column * BLOCK_SIDE * RGBA;
        decode(block, &mut padded[start..], pitch);
    }
    let mut rgba = Vec::with_capacity(width * height * RGBA);
    for row in padded.chunks_exact(pitch).take(height) {
        rgba.extend_from_slice(&row[..width * RGBA]);
    }
    Ok((image, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = 8;
    /// Value of the header's size field.
    const HEADER_SIZE: u32 = 124;
    /// Value of the pixel format's size field.
    const PIXEL_FORMAT_SIZE: u32 = 32;
    /// Pixel format flag: the four-character code is set.
    const FOURCC_FLAG: u32 = 0x4;
    const DXGI_UNKNOWN: u32 = 2;
    /// BC1 end colours in RGB565: pure red and pure blue.
    const RED_565: u16 = 0xF800;
    const BLUE_565: u16 = 0x001F;

    fn header(width: u32, height: u32, fourcc: &[u8; 4], dxgi: Option<u32>) -> Vec<u8> {
        let mut bytes = DDS_MAGIC.to_vec();
        let mut words = [0_u32; 31];
        words[0] = HEADER_SIZE;
        words[2] = height;
        words[3] = width;
        words[PIXEL_FORMAT_WORD] = PIXEL_FORMAT_SIZE;
        words[PIXEL_FORMAT_WORD + 1] = FOURCC_FLAG;
        words[PIXEL_FORMAT_WORD + 2] = u32::from_le_bytes(*fourcc);
        for word in words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        if let Some(format) = dxgi {
            let mut dx10 = [0_u32; 5];
            dx10[0] = format;
            for word in dx10 {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn a_dx10_header_gives_format_and_offset() {
        let bytes = header(SIZE, SIZE, b"DX10", Some(DXGI_BC7_UNORM));
        let (image, offset) = dds_header(&bytes).expect("header");
        assert_eq!(
            image,
            DdsImage {
                width: SIZE,
                height: SIZE,
                format: DdsFormat::Bc7
            }
        );
        assert_eq!(offset, DX10_DATA_OFFSET);
    }

    #[test]
    fn a_dxt1_header_is_bc1() {
        let bytes = header(SIZE, SIZE, b"DXT1", None);
        let (image, offset) = dds_header(&bytes).expect("header");
        assert_eq!((image.format, offset), (DdsFormat::Bc1, DATA_OFFSET));
    }

    #[test]
    fn an_unknown_format_is_named_in_the_error() {
        let bytes = header(SIZE, SIZE, b"DX10", Some(DXGI_UNKNOWN));
        assert_eq!(
            dds_header(&bytes),
            Err(DdsError::Unsupported("DXGI 2".to_owned()))
        );
        let bytes = header(SIZE, SIZE, b"ATI2", None);
        assert_eq!(
            dds_header(&bytes),
            Err(DdsError::Unsupported("ATI2".to_owned()))
        );
    }

    #[test]
    fn a_short_file_is_an_error() {
        let bytes = header(SIZE, SIZE, b"DXT1", None);
        assert_eq!(dds_header(&bytes[..10]), Err(DdsError::Short));
        // The header is whole, but the 8x8 BC1 data (4 blocks of 8 bytes) is missing.
        assert_eq!(
            decode_dds(&bytes).map(|(image, _)| image),
            Err(DdsError::Short)
        );
        let mut bad = bytes;
        bad[0] = b'X';
        assert_eq!(dds_header(&bad), Err(DdsError::Magic));
    }

    #[test]
    fn a_bc1_block_decodes_to_its_colours() {
        const BLOCK_PIXELS: usize = 4;
        let mut bytes = header(4, 4, b"DXT1", None);
        bytes.extend_from_slice(&RED_565.to_le_bytes());
        bytes.extend_from_slice(&BLUE_565.to_le_bytes());
        // Row 0 all index 0 (red), row 1 all index 1 (blue), rows 2 and 3 index 0.
        bytes.extend_from_slice(&[0b0000_0000, 0b0101_0101, 0, 0]);
        let (image, rgba) = decode_dds(&bytes).expect("decode");
        assert_eq!((image.width, image.height), (4, 4));
        assert_eq!(rgba.len(), BLOCK_PIXELS * BLOCK_PIXELS * 4);
        assert_eq!(rgba[..4], [255, 0, 0, 255]);
        let row_1 = BLOCK_PIXELS * 4;
        assert_eq!(rgba[row_1..row_1 + 4], [0, 0, 255, 255]);
    }

    #[test]
    fn an_image_that_is_not_whole_blocks_is_cropped() {
        let mut bytes = header(6, 2, b"DXT1", None);
        // Two blocks wide, one block high.
        bytes.extend_from_slice(&[0; 16]);
        let (image, rgba) = decode_dds(&bytes).expect("decode");
        assert_eq!((image.width, image.height, rgba.len()), (6, 2, 6 * 2 * 4));
    }
}
