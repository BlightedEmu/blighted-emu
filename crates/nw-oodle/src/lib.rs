// SPDX-License-Identifier: GPL-3.0-or-later
//! Oodle Kraken decoding for pak entries, with the `oozextract` decoder.
//!
//! `oozextract` declares the MIT licence but is a port of the GPL-3.0 `ooz` decoder, so this
//! crate is GPL-3.0-or-later (M1 spec section 3.2).

use nw_formats::pak::{DecompressError, Decompressor};

/// Error message when `oozextract` panics.
const DECODER_PANIC_MESSAGE: &str = "the Oodle decoder panicked on invalid input";

/// Decodes Oodle Kraken data (the only Oodle codec in the pinned client).
///
/// `oozextract` can panic on invalid input. The panic is caught and returned as a
/// [`DecompressError`], so hostile pak entries cannot stop the caller. The default panic hook
/// still prints the panic message to stderr.
#[derive(Debug, Default, Clone, Copy)]
pub struct KrakenDecompressor;

impl Decompressor for KrakenDecompressor {
    fn decompress(
        &self,
        input: &[u8],
        uncompressed_size: usize,
    ) -> Result<Vec<u8>, DecompressError> {
        let mut output = vec![0u8; uncompressed_size];
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            oozextract::Extractor::new().read_from_slice(input, &mut output)
        }))
        .map_err(|_| DecompressError(DECODER_PANIC_MESSAGE.to_owned()))?;
        let written = decoded.map_err(|error| DecompressError(error.to_string()))?;
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
    /// A BitKnit block that makes `oozextract` 0.5.5 panic (found by fuzzing): a subtraction
    /// overflow with overflow checks, an out-of-range slice index without them.
    const DECODER_PANIC_INPUT: [u8; 20] = [
        0x8C, 0x0B, 0x00, 0x0F, 0xAE, 0x8F, 0x00, 0x5C, 0x00, 0x00, 0xB3, 0x66, 0x00, 0x00, 0x00,
        0x98, 0x00, 0x45, 0x00, 0x00,
    ];

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
    fn decoder_panic_is_an_error() {
        let error = KrakenDecompressor
            .decompress(&DECODER_PANIC_INPUT, OUTPUT_SIZE)
            .unwrap_err();
        assert_eq!(error.0, DECODER_PANIC_MESSAGE);
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
    use nw_formats::rnr::{RNR_EXTENSION, parse_rnr};

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

    #[test]
    fn every_rnr_physics_asset_of_the_pinned_build_reads_to_its_end() {
        let mut read = 0;
        for path in find_paks(&game_dir()).unwrap() {
            let mut pak = PakArchive::open(&path).unwrap();
            for entry in pak.entries().unwrap() {
                if entry.path.ends_with(RNR_EXTENSION) {
                    let bytes = pak
                        .read(&entry, &KrakenDecompressor)
                        .unwrap_or_else(|error| panic!("{error}"));
                    parse_rnr(&bytes).unwrap_or_else(|error| panic!("{}: {error}", entry.path));
                    read += 1;
                }
            }
        }
        assert_eq!(read, PINNED_RNR_FILES);
    }
}
