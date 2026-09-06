// SPDX-License-Identifier: MIT OR Apache-2.0
//! RGBA8 images as PNG bytes.

/// Encodes `rgba` (`width` x `height` pixels of 4 bytes, top row first) as PNG bytes, with fast
/// compression.
///
/// # Errors
///
/// Returns the encoder error, for example when `rgba` does not hold `width` x `height` pixels.
pub fn encode_rgba_png(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<Vec<u8>, png::EncodingError> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_rgba_image_reads_back() {
        let rgba: Vec<u8> = (0..2 * 3 * 4).map(|value| value as u8).collect();
        let bytes = encode_rgba_png(2, 3, &rgba).expect("encode");
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .expect("png");
        let mut pixels = vec![0; reader.output_buffer_size().expect("size")];
        let info = reader.next_frame(&mut pixels).expect("frame");
        assert_eq!((info.width, info.height), (2, 3));
        assert_eq!(pixels, rgba);
    }

    #[test]
    fn a_wrong_pixel_count_is_an_error() {
        assert!(encode_rgba_png(2, 2, &[0; 4]).is_err());
    }
}
