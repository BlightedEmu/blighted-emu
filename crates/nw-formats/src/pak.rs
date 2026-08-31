// SPDX-License-Identifier: MIT OR Apache-2.0
//! Pak archives: standard ZIP files whose entries use store (0), deflate (8) or Oodle (15).

use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use flate2::read::DeflateDecoder;
use zip::{CompressionMethod, ZipArchive};

/// ZIP method number that New World uses for Oodle entries.
const METHOD_OODLE: u16 = 15;
/// `zip` has no named constant for method 15; its `Unsupported` variant is the only way to name it.
#[allow(deprecated)]
const ZIP_METHOD_OODLE: CompressionMethod = CompressionMethod::Unsupported(METHOD_OODLE);
/// Largest entry this reader decodes. The largest entry in the pinned client is 498 MiB.
pub const MAX_ENTRY_SIZE: u64 = 1 << 30;
/// Folder inside the game install that holds the paks.
pub const ASSETS_DIR: &str = "assets";
/// File extension of pak archives.
pub const PAK_EXTENSION: &str = "pak";

/// Decodes Oodle entry data. Implemented outside this crate (see `nw-oodle`).
pub trait Decompressor {
    /// Decodes `input` into exactly `uncompressed_size` bytes.
    fn decompress(
        &self,
        input: &[u8],
        uncompressed_size: usize,
    ) -> Result<Vec<u8>, DecompressError>;
}

/// Error from a [`Decompressor`].
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DecompressError(pub String);

/// How a pak entry is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryMethod {
    /// ZIP method 0: raw bytes.
    Store,
    /// ZIP method 8: deflate.
    Deflate,
    /// ZIP method 15: Oodle.
    Oodle,
    /// Any other method, as reported by the `zip` crate.
    Other(String),
}

/// One entry of a pak archive, from the ZIP central directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakEntry {
    /// Position in the central directory.
    pub index: usize,
    /// Path inside the pak, for example `sharedassets/.../x.datasheet`.
    pub path: String,
    /// How the entry is stored.
    pub method: EntryMethod,
    /// Size of the stored bytes.
    pub compressed_size: u64,
    /// Size after decoding.
    pub uncompressed_size: u64,
    /// ZIP CRC32 of the decoded bytes.
    pub crc32: u32,
}

/// Error from pak reading.
#[derive(Debug, thiserror::Error)]
pub enum PakError {
    /// A file or folder cannot be read.
    #[error("cannot read {}: {source}", .path.display())]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The ZIP structure is invalid.
    #[error("invalid pak {}: {source}", .path.display())]
    Zip {
        /// The pak.
        path: PathBuf,
        /// The ZIP error.
        #[source]
        source: zip::result::ZipError,
    },
    /// The entry uses a method this reader does not decode.
    #[error("{}: entry `{entry}` uses unsupported compression method {method}", .pak.display())]
    UnsupportedMethod {
        /// The pak.
        pak: PathBuf,
        /// The entry path.
        entry: String,
        /// The method as reported by the `zip` crate.
        method: String,
    },
    /// The entry, stored or decoded, is larger than [`MAX_ENTRY_SIZE`].
    #[error("{}: entry `{entry}` is {size} bytes, above the limit of {limit} bytes", .pak.display())]
    EntryTooLarge {
        /// The pak.
        pak: PathBuf,
        /// The entry path.
        entry: String,
        /// Declared compressed or uncompressed size, whichever is above the limit.
        size: u64,
        /// The limit.
        limit: u64,
    },
    /// Deflate decoding failed.
    #[error("{}: entry `{entry}` cannot be inflated: {source}", .pak.display())]
    Inflate {
        /// The pak.
        pak: PathBuf,
        /// The entry path.
        entry: String,
        /// The decoder error.
        #[source]
        source: std::io::Error,
    },
    /// Oodle decoding failed.
    #[error("{}: entry `{entry}` cannot be decompressed: {source}", .pak.display())]
    Decompress {
        /// The pak.
        pak: PathBuf,
        /// The entry path.
        entry: String,
        /// The decompressor error.
        #[source]
        source: DecompressError,
    },
    /// The decoded size differs from the central directory.
    #[error("{}: entry `{entry}` has {actual} bytes, expected {expected}", .pak.display())]
    SizeMismatch {
        /// The pak.
        pak: PathBuf,
        /// The entry path.
        entry: String,
        /// Size from the central directory.
        expected: u64,
        /// Decoded size.
        actual: u64,
    },
    /// The decoded CRC32 differs from the central directory.
    #[error("{}: entry `{entry}` has CRC32 {actual:08x}, expected {expected:08x}", .pak.display())]
    CrcMismatch {
        /// The pak.
        pak: PathBuf,
        /// The entry path.
        entry: String,
        /// CRC32 from the central directory.
        expected: u32,
        /// CRC32 of the decoded bytes.
        actual: u32,
    },
}

/// An open pak archive.
pub struct PakArchive<R> {
    label: PathBuf,
    zip: ZipArchive<R>,
}

impl PakArchive<File> {
    /// Opens the pak at `path`.
    pub fn open(path: &Path) -> Result<Self, PakError> {
        let file = File::open(path).map_err(|source| PakError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_reader(file, path)
    }
}

impl<R: Read + Seek> PakArchive<R> {
    /// Reads the ZIP central directory from `reader`. `label` names the pak in errors.
    pub fn from_reader(reader: R, label: &Path) -> Result<Self, PakError> {
        let zip = ZipArchive::new(reader).map_err(|source| PakError::Zip {
            path: label.to_path_buf(),
            source,
        })?;
        Ok(Self {
            label: label.to_path_buf(),
            zip,
        })
    }

    /// Lists all entries in central directory order.
    pub fn entries(&mut self) -> Result<Vec<PakEntry>, PakError> {
        let label = &self.label;
        let zip_error = |source| PakError::Zip {
            path: label.clone(),
            source,
        };
        let count = self.zip.len();
        (0..count)
            .map(|index| {
                let file = self.zip.by_index_raw(index).map_err(zip_error)?;
                let path = file.name().map_err(zip_error)?.into_owned();
                Ok(PakEntry {
                    index,
                    path,
                    method: entry_method(file.compression()),
                    compressed_size: file.compressed_size(),
                    uncompressed_size: file.size(),
                    crc32: file.crc32(),
                })
            })
            .collect()
    }

    /// Reads and decodes one entry, then checks its size and ZIP CRC32.
    pub fn read(
        &mut self,
        entry: &PakEntry,
        decompressor: &dyn Decompressor,
    ) -> Result<Vec<u8>, PakError> {
        let pak = self.label.clone();
        let too_large = |size| PakError::EntryTooLarge {
            pak: pak.clone(),
            entry: entry.path.clone(),
            size,
            limit: MAX_ENTRY_SIZE,
        };
        let expected = entry.uncompressed_size;
        if expected > MAX_ENTRY_SIZE {
            return Err(too_large(expected));
        }
        // MAX_ENTRY_SIZE fits in usize on every supported (64-bit) target.
        let expected_len = expected as usize;
        let mut file = self
            .zip
            .by_index_raw(entry.index)
            .map_err(|source| PakError::Zip {
                path: pak.clone(),
                source,
            })?;
        // The raw reader reads as many bytes as the central directory declares.
        let stored_size = file.compressed_size();
        if stored_size > MAX_ENTRY_SIZE {
            return Err(too_large(stored_size));
        }
        let mut raw = Vec::new();
        file.read_to_end(&mut raw).map_err(|source| PakError::Io {
            path: pak.clone(),
            source,
        })?;
        let data = match &entry.method {
            EntryMethod::Store => raw,
            EntryMethod::Deflate => {
                let mut out = Vec::with_capacity(expected_len);
                // One byte past the expected size is enough to detect a size mismatch.
                DeflateDecoder::new(raw.as_slice())
                    .take(expected + 1)
                    .read_to_end(&mut out)
                    .map_err(|source| PakError::Inflate {
                        pak: pak.clone(),
                        entry: entry.path.clone(),
                        source,
                    })?;
                out
            }
            EntryMethod::Oodle => {
                decompressor
                    .decompress(&raw, expected_len)
                    .map_err(|source| PakError::Decompress {
                        pak: pak.clone(),
                        entry: entry.path.clone(),
                        source,
                    })?
            }
            EntryMethod::Other(method) => {
                return Err(PakError::UnsupportedMethod {
                    pak,
                    entry: entry.path.clone(),
                    method: method.clone(),
                });
            }
        };
        let actual = data.len() as u64;
        if actual != expected {
            return Err(PakError::SizeMismatch {
                pak,
                entry: entry.path.clone(),
                expected,
                actual,
            });
        }
        let crc32 = crc32fast::hash(&data);
        if crc32 != entry.crc32 {
            return Err(PakError::CrcMismatch {
                pak,
                entry: entry.path.clone(),
                expected: entry.crc32,
                actual: crc32,
            });
        }
        Ok(data)
    }
}

/// Lists every `.pak` file under `<game_dir>/assets`, sorted by path.
pub fn find_paks(game_dir: &Path) -> Result<Vec<PathBuf>, PakError> {
    let mut paks = Vec::new();
    collect_paks(&game_dir.join(ASSETS_DIR), &mut paks)?;
    paks.sort();
    Ok(paks)
}

fn collect_paks(dir: &Path, paks: &mut Vec<PathBuf>) -> Result<(), PakError> {
    let io_error = |source| PakError::Io {
        path: dir.to_path_buf(),
        source,
    };
    for item in std::fs::read_dir(dir).map_err(io_error)? {
        let path = item.map_err(io_error)?.path();
        if path.is_dir() {
            collect_paks(&path, paks)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == PAK_EXTENSION)
        {
            paks.push(path);
        }
    }
    Ok(())
}

fn entry_method(method: CompressionMethod) -> EntryMethod {
    if method == CompressionMethod::STORE {
        EntryMethod::Store
    } else if method == CompressionMethod::DEFLATE {
        EntryMethod::Deflate
    } else if method == ZIP_METHOD_OODLE {
        EntryMethod::Oodle
    } else {
        EntryMethod::Other(format!("{method:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    const ZIP_STORE: u16 = 0;
    const ZIP_DEFLATE: u16 = 8;
    const ZIP_BZIP2: u16 = 12;
    const LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4b50;
    const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4b50;
    const END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x0605_4b50;
    const ZIP_VERSION: u16 = 20;
    /// DOS date 1980-01-01, the first valid date.
    const DOS_DATE: u16 = 0x0021;
    const PACKED: &[u8] = b"PACKED";
    const UNPACKED: &[u8] = b"unpacked data";

    struct TestEntry<'a> {
        path: &'a str,
        method: u16,
        stored: &'a [u8],
        /// Size written to the headers; equal to `stored.len()` unless a test forges it.
        compressed_size: u32,
        uncompressed_size: u32,
        crc32: u32,
    }

    fn u16le(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn u32le(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    /// Builds a ZIP file byte by byte, so tests can use any method, size and CRC32.
    fn build_zip(entries: &[TestEntry<'_>]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for entry in entries {
            let offset = u32::try_from(out.len()).unwrap();
            let name_len = u16::try_from(entry.path.len()).unwrap();
            u32le(&mut out, LOCAL_HEADER_SIGNATURE);
            u16le(&mut out, ZIP_VERSION);
            u16le(&mut out, 0);
            u16le(&mut out, entry.method);
            u16le(&mut out, 0);
            u16le(&mut out, DOS_DATE);
            u32le(&mut out, entry.crc32);
            u32le(&mut out, entry.compressed_size);
            u32le(&mut out, entry.uncompressed_size);
            u16le(&mut out, name_len);
            u16le(&mut out, 0);
            out.extend_from_slice(entry.path.as_bytes());
            out.extend_from_slice(entry.stored);

            u32le(&mut central, CENTRAL_HEADER_SIGNATURE);
            u16le(&mut central, ZIP_VERSION);
            u16le(&mut central, ZIP_VERSION);
            u16le(&mut central, 0);
            u16le(&mut central, entry.method);
            u16le(&mut central, 0);
            u16le(&mut central, DOS_DATE);
            u32le(&mut central, entry.crc32);
            u32le(&mut central, entry.compressed_size);
            u32le(&mut central, entry.uncompressed_size);
            u16le(&mut central, name_len);
            u16le(&mut central, 0);
            u16le(&mut central, 0);
            u16le(&mut central, 0);
            u16le(&mut central, 0);
            u32le(&mut central, 0);
            u32le(&mut central, offset);
            central.extend_from_slice(entry.path.as_bytes());
        }
        let central_offset = u32::try_from(out.len()).unwrap();
        let central_size = u32::try_from(central.len()).unwrap();
        let count = u16::try_from(entries.len()).unwrap();
        out.extend_from_slice(&central);
        u32le(&mut out, END_OF_CENTRAL_DIRECTORY_SIGNATURE);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, count);
        u16le(&mut out, count);
        u32le(&mut out, central_size);
        u32le(&mut out, central_offset);
        u16le(&mut out, 0);
        out
    }

    fn stored<'a>(path: &'a str, data: &'static [u8]) -> TestEntry<'a> {
        TestEntry {
            path,
            method: ZIP_STORE,
            stored: data,
            compressed_size: u32::try_from(data.len()).unwrap(),
            uncompressed_size: u32::try_from(data.len()).unwrap(),
            crc32: crc32fast::hash(data),
        }
    }

    fn open(entries: &[TestEntry<'_>]) -> PakArchive<Cursor<Vec<u8>>> {
        PakArchive::from_reader(Cursor::new(build_zip(entries)), Path::new("test.pak")).unwrap()
    }

    /// Maps `PACKED` to `UNPACKED`; anything else is an error.
    struct TestDecompressor;

    impl Decompressor for TestDecompressor {
        fn decompress(
            &self,
            input: &[u8],
            _uncompressed_size: usize,
        ) -> Result<Vec<u8>, DecompressError> {
            if input == PACKED {
                Ok(UNPACKED.to_vec())
            } else {
                Err(DecompressError("unknown test input".to_owned()))
            }
        }
    }

    fn oodle_entry(stored: &'static [u8]) -> TestEntry<'static> {
        TestEntry {
            path: "a.datasheet",
            method: METHOD_OODLE,
            stored,
            compressed_size: u32::try_from(stored.len()).unwrap(),
            uncompressed_size: u32::try_from(UNPACKED.len()).unwrap(),
            crc32: crc32fast::hash(UNPACKED),
        }
    }

    fn read_first(entries: &[TestEntry<'_>]) -> Result<Vec<u8>, PakError> {
        let mut pak = open(entries);
        let entry = pak.entries().unwrap().remove(0);
        pak.read(&entry, &TestDecompressor)
    }

    #[test]
    fn lists_entries_with_method_sizes_and_crc() {
        let mut pak = open(&[stored("a.txt", b"hello"), oodle_entry(PACKED)]);
        let entries = pak.entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "a.txt");
        assert_eq!(entries[0].method, EntryMethod::Store);
        assert_eq!(entries[0].uncompressed_size, 5);
        assert_eq!(entries[0].crc32, crc32fast::hash(b"hello"));
        assert_eq!(entries[1].method, EntryMethod::Oodle);
        assert_eq!(
            entries[1].compressed_size,
            u64::try_from(PACKED.len()).unwrap()
        );
    }

    #[test]
    fn reads_stored_entry() {
        assert_eq!(read_first(&[stored("a.txt", b"hello")]).unwrap(), b"hello");
    }

    #[test]
    fn reads_deflate_entry() {
        let data: &[u8] = b"deflate me deflate me deflate me";
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        let packed: &'static [u8] = Box::leak(encoder.finish().unwrap().into_boxed_slice());
        let entry = TestEntry {
            path: "a.bin",
            method: ZIP_DEFLATE,
            stored: packed,
            compressed_size: u32::try_from(packed.len()).unwrap(),
            uncompressed_size: u32::try_from(data.len()).unwrap(),
            crc32: crc32fast::hash(data),
        };
        assert_eq!(read_first(&[entry]).unwrap(), data);
    }

    #[test]
    fn oodle_entry_uses_the_decompressor() {
        assert_eq!(read_first(&[oodle_entry(PACKED)]).unwrap(), UNPACKED);
    }

    #[test]
    fn decompressor_failure_is_reported() {
        let error = read_first(&[oodle_entry(b"GARBAGE")]).unwrap_err();
        assert!(
            matches!(error, PakError::Decompress { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn unsupported_method_is_rejected() {
        let mut entry = stored("a.bz2", b"x");
        entry.method = ZIP_BZIP2;
        let error = read_first(&[entry]).unwrap_err();
        assert!(
            matches!(error, PakError::UnsupportedMethod { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn crc_mismatch_is_rejected() {
        let mut entry = stored("a.txt", b"hello");
        entry.crc32 ^= 1;
        let error = read_first(&[entry]).unwrap_err();
        assert!(
            matches!(error, PakError::CrcMismatch { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn size_mismatch_is_rejected() {
        let mut entry = oodle_entry(PACKED);
        entry.uncompressed_size += 1;
        let error = read_first(&[entry]).unwrap_err();
        assert!(
            matches!(error, PakError::SizeMismatch { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn entry_above_size_limit_is_rejected() {
        let mut entry = oodle_entry(PACKED);
        entry.uncompressed_size = u32::try_from(MAX_ENTRY_SIZE + 1).unwrap();
        let error = read_first(&[entry]).unwrap_err();
        assert!(
            matches!(error, PakError::EntryTooLarge { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn compressed_size_above_limit_is_rejected() {
        let mut entry = oodle_entry(PACKED);
        entry.compressed_size = u32::try_from(MAX_ENTRY_SIZE + 1).unwrap();
        let error = read_first(&[entry]).unwrap_err();
        assert!(
            matches!(error, PakError::EntryTooLarge { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn not_a_zip_is_an_error() {
        let result =
            PakArchive::from_reader(Cursor::new(b"not a zip".to_vec()), Path::new("bad.pak"));
        assert!(matches!(result, Err(PakError::Zip { .. })));
    }

    #[test]
    fn find_paks_lists_pak_files_recursively_sorted() {
        let root = tempfile::tempdir().unwrap();
        let assets = root.path().join(ASSETS_DIR);
        std::fs::create_dir_all(assets.join("levels/x")).unwrap();
        std::fs::write(assets.join("b.pak"), b"").unwrap();
        std::fs::write(assets.join("levels/x/a.pak"), b"").unwrap();
        std::fs::write(assets.join("notes.txt"), b"").unwrap();
        let paks = find_paks(root.path()).unwrap();
        assert_eq!(
            paks,
            vec![assets.join("b.pak"), assets.join("levels/x/a.pak")]
        );
    }

    #[test]
    fn find_paks_without_assets_folder_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(find_paks(root.path()), Err(PakError::Io { .. })));
    }
}

#[cfg(all(test, feature = "local-data"))]
mod local_data_tests {
    use super::*;
    use crate::local_data::*;

    #[test]
    fn pinned_paks_open_and_have_the_known_entry_methods() {
        let paks = find_paks(&game_dir()).unwrap();
        assert_eq!(paks.len(), PINNED_PAK_COUNT);
        let (mut store, mut deflate, mut oodle, mut total) = (0, 0, 0, 0);
        for path in &paks {
            for entry in PakArchive::open(path).unwrap().entries().unwrap() {
                total += 1;
                match entry.method {
                    EntryMethod::Store => store += 1,
                    EntryMethod::Deflate => deflate += 1,
                    EntryMethod::Oodle => oodle += 1,
                    EntryMethod::Other(method) => {
                        panic!("{}: unexpected method {method}", entry.path)
                    }
                }
            }
        }
        assert_eq!(total, PINNED_ENTRY_COUNT);
        assert_eq!(store, PINNED_STORED_ENTRIES);
        assert_eq!(deflate, PINNED_DEFLATE_ENTRIES);
        assert_eq!(oodle, PINNED_OODLE_ENTRIES);
    }
}
