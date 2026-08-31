// SPDX-License-Identifier: GPL-3.0-or-later
//! Oodle Kraken decoding for pak entries, with the `oozextract` decoder.
//!
//! `oozextract` declares the MIT licence but is a port of the GPL-3.0 `ooz` decoder, so this
//! crate is GPL-3.0-or-later (M1 spec section 3.2).

use nw_formats::pak::{DecompressError, Decompressor};

/// Decodes Oodle Kraken data (the only Oodle codec in the pinned client).
#[derive(Debug, Default, Clone, Copy)]
pub struct KrakenDecompressor;

impl Decompressor for KrakenDecompressor {
    fn decompress(
        &self,
        input: &[u8],
        uncompressed_size: usize,
    ) -> Result<Vec<u8>, DecompressError> {
        let mut output = vec![0u8; uncompressed_size];
        let written = oozextract::Extractor::new()
            .read_from_slice(input, &mut output)
            .map_err(|error| DecompressError(error.to_string()))?;
        if written != uncompressed_size {
            return Err(DecompressError(format!(
                "decoded {written} bytes, expected {uncompressed_size}"
            )));
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT_SIZE: usize = 64;
    /// Starts like a Kraken block header (low nibble 0xC, decoder type 6) but is not valid data.
    const FAKE_KRAKEN_HEADER: [u8; 2] = [0xCC, 0x06];
    const PATTERN_LEN: usize = 256;

    #[test]
    fn empty_input_is_an_error() {
        assert!(KrakenDecompressor.decompress(&[], OUTPUT_SIZE).is_err());
    }

    #[test]
    fn bare_header_is_an_error() {
        assert!(
            KrakenDecompressor
                .decompress(&FAKE_KRAKEN_HEADER, OUTPUT_SIZE)
                .is_err()
        );
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let garbage: Vec<u8> = (0..PATTERN_LEN)
            .map(|index| (index * 7 % PATTERN_LEN) as u8)
            .collect();
        assert!(
            KrakenDecompressor
                .decompress(&garbage, OUTPUT_SIZE)
                .is_err()
        );
    }
}

#[cfg(all(test, feature = "local-data"))]
mod local_data_tests {
    use super::*;
    use nw_formats::datasheet::DATASHEET_EXTENSION;
    use nw_formats::local_data::*;
    use nw_formats::pak::{EntryMethod, PakArchive, find_paks};

    #[test]
    fn every_oodle_datasheet_of_the_pinned_build_decodes_with_matching_crc() {
        let mut decoded = 0;
        for path in find_paks(&game_dir()).unwrap() {
            let mut pak = PakArchive::open(&path).unwrap();
            for entry in pak.entries().unwrap() {
                if entry.method == EntryMethod::Oodle && entry.path.ends_with(DATASHEET_EXTENSION) {
                    // PakArchive::read checks the size and the ZIP CRC32.
                    pak.read(&entry, &KrakenDecompressor)
                        .unwrap_or_else(|error| panic!("{error}"));
                    decoded += 1;
                }
            }
        }
        assert_eq!(decoded, PINNED_OODLE_DATASHEETS);
    }
}
