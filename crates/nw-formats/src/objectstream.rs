// SPDX-License-Identifier: MIT OR Apache-2.0
//! Binary ObjectStream version 3 (M1b spec section 3.1). A stream is a binary tag, a version,
//! and one root element. Each element has a flag byte, an optional name CRC, an optional class
//! version, a type UUID, an optional value, and a child list that ends with a `0x00` byte.
//! All numbers are big-endian.

/// The only stream version in the pinned client.
pub const OBJECT_STREAM_VERSION: u32 = 3;
/// Deepest element nesting this reader accepts (the pinned client nests at most 27 deep).
pub const MAX_OBJECT_STREAM_DEPTH: usize = 64;
/// Size of a type UUID.
pub const UUID_SIZE: usize = 16;

const BINARY_TAG: u8 = 0x00;
const END_MARKER: u8 = 0x00;
const FLAG_HAS_VERSION: u8 = 0x80;
const FLAG_HAS_NAME: u8 = 0x40;
const FLAG_SIZE_FIELD: u8 = 0x20;
const FLAG_HAS_VALUE: u8 = 0x10;
const FLAG_ELEMENT_HEADER: u8 = 0x08;
const SIZE_BITS: u8 = 0x07;
/// Largest value size that fits in the low flag bits.
const MAX_INLINE_SIZE: usize = SIZE_BITS as usize;
const SIZE_WIDTH_U8: u8 = 1;
const SIZE_WIDTH_U16: u8 = 2;
const SIZE_WIDTH_U32: u8 = 4;
const VEC3_SIZE: usize = 3 * size_of::<f32>();
/// Elements in the stream body: exactly one root.
const ROOT_COUNT: usize = 1;
/// Nesting depth of the root element.
const ROOT_DEPTH: usize = 1;

/// One element of an ObjectStream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Element {
    /// [`lowercase_crc32`] of the field name; `None` for the root and some containers.
    pub name_crc: Option<u32>,
    /// Class version, when the element has one.
    pub version: Option<u8>,
    /// Type UUID, raw bytes as stored.
    pub type_id: [u8; UUID_SIZE],
    /// Value bytes; empty when the element has no value.
    pub value: Vec<u8>,
    /// Child elements in stream order.
    pub children: Vec<Element>,
}

/// Error from ObjectStream reading, writing, or value access.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ObjectStreamError {
    /// The data ends inside an element.
    #[error("object stream is truncated at byte {offset}")]
    Truncated {
        /// Offset of the read that failed.
        offset: usize,
    },
    /// The first byte is not the binary tag.
    #[error("object stream tag {tag:#04x} is not the binary tag")]
    NotBinary {
        /// First byte.
        tag: u8,
    },
    /// The stream version is not 3.
    #[error("object stream version {version} is not supported")]
    UnsupportedVersion {
        /// Version in the stream.
        version: u32,
    },
    /// An element flag byte lacks the element header flag.
    #[error("element at byte {offset} has flags {flags:#04x} without the header flag")]
    MissingHeaderFlag {
        /// Offset of the flag byte.
        offset: usize,
        /// The flag byte.
        flags: u8,
    },
    /// A size field width other than 1, 2 or 4 bytes.
    #[error("element at byte {offset} has a size field of {width} bytes")]
    BadSizeWidth {
        /// Offset of the flag byte.
        offset: usize,
        /// Width from the flag byte.
        width: u8,
    },
    /// Elements nest deeper than [`MAX_OBJECT_STREAM_DEPTH`].
    #[error("object stream nests deeper than {MAX_OBJECT_STREAM_DEPTH}")]
    TooDeep,
    /// The stream body does not hold exactly one root element.
    #[error("object stream has {count} root elements, expected 1")]
    RootCount {
        /// Root elements found.
        count: usize,
    },
    /// Bytes follow the end of the stream.
    #[error("object stream has {count} bytes after its end")]
    TrailingBytes {
        /// Number of extra bytes.
        count: usize,
    },
    /// A value is too large for a 4-byte size field.
    #[error("element value of {size} bytes is too large to write")]
    ValueTooLarge {
        /// Value size.
        size: usize,
    },
    /// A value has another size than the type needs.
    #[error("value of field {name_crc:08x?} has {actual} bytes, expected {expected}")]
    ValueSize {
        /// Name CRC of the element.
        name_crc: Option<u32>,
        /// Size the type needs.
        expected: usize,
        /// Size of the value.
        actual: usize,
    },
    /// A string value is not UTF-8.
    #[error("value of field {name_crc:08x?} is not UTF-8")]
    NotUtf8 {
        /// Name CRC of the element.
        name_crc: Option<u32>,
    },
}

/// crc32 of the lower-case form of `text`. ObjectStream name CRCs and the spawn IDs in world
/// metadata both use it.
pub fn lowercase_crc32(text: &str) -> u32 {
    crc32fast::hash(text.to_lowercase().as_bytes())
}

impl Element {
    /// An element with a name and a value, and no version, type or children.
    pub fn with_value(name_crc: u32, value: Vec<u8>) -> Self {
        Self {
            name_crc: Some(name_crc),
            value,
            ..Self::default()
        }
    }

    /// An element with a name and children, and no version, type or value.
    pub fn with_children(name_crc: u32, children: Vec<Element>) -> Self {
        Self {
            name_crc: Some(name_crc),
            children,
            ..Self::default()
        }
    }

    /// The first child with this name CRC.
    pub fn child(&self, name_crc: u32) -> Option<&Element> {
        self.children
            .iter()
            .find(|child| child.name_crc == Some(name_crc))
    }

    /// Every child with this name CRC.
    pub fn children_named(&self, name_crc: u32) -> impl Iterator<Item = &Element> {
        self.children
            .iter()
            .filter(move |child| child.name_crc == Some(name_crc))
    }

    /// The value as a bool (1 byte, not zero = true).
    pub fn as_bool(&self) -> Result<bool, ObjectStreamError> {
        Ok(self.as_u8()? != 0)
    }

    /// The value as a `u8`.
    pub fn as_u8(&self) -> Result<u8, ObjectStreamError> {
        self.fixed().map(u8::from_be_bytes)
    }

    /// The value as a big-endian `u16`.
    pub fn as_u16(&self) -> Result<u16, ObjectStreamError> {
        self.fixed().map(u16::from_be_bytes)
    }

    /// The value as a big-endian `u32`.
    pub fn as_u32(&self) -> Result<u32, ObjectStreamError> {
        self.fixed().map(u32::from_be_bytes)
    }

    /// The value as a big-endian `u64`.
    pub fn as_u64(&self) -> Result<u64, ObjectStreamError> {
        self.fixed().map(u64::from_be_bytes)
    }

    /// The value as a big-endian IEEE-754 `f32`.
    pub fn as_f32(&self) -> Result<f32, ObjectStreamError> {
        self.fixed().map(f32::from_be_bytes)
    }

    /// The value as a `Vector3` (3 big-endian `f32`).
    pub fn as_vec3(&self) -> Result<[f32; 3], ObjectStreamError> {
        let bytes: [u8; VEC3_SIZE] = self.fixed()?;
        let component = |index: usize| {
            let start = index * size_of::<f32>();
            f32::from_be_bytes([
                bytes[start],
                bytes[start + 1],
                bytes[start + 2],
                bytes[start + 3],
            ])
        };
        Ok([component(0), component(1), component(2)])
    }

    /// The value as UTF-8 text.
    pub fn as_string(&self) -> Result<&str, ObjectStreamError> {
        std::str::from_utf8(&self.value).map_err(|_| ObjectStreamError::NotUtf8 {
            name_crc: self.name_crc,
        })
    }

    /// The value as a 16-byte UUID.
    pub fn as_uuid(&self) -> Result<[u8; UUID_SIZE], ObjectStreamError> {
        self.fixed()
    }

    fn fixed<const N: usize>(&self) -> Result<[u8; N], ObjectStreamError> {
        self.value
            .as_slice()
            .try_into()
            .map_err(|_| ObjectStreamError::ValueSize {
                name_crc: self.name_crc,
                expected: N,
                actual: self.value.len(),
            })
    }
}

/// Reads a binary ObjectStream and returns its root element.
pub fn parse_object_stream(bytes: &[u8]) -> Result<Element, ObjectStreamError> {
    let mut reader = Reader { bytes, offset: 0 };
    let tag = reader.u8()?;
    if tag != BINARY_TAG {
        return Err(ObjectStreamError::NotBinary { tag });
    }
    let version = u32::from_be_bytes(reader.array()?);
    if version != OBJECT_STREAM_VERSION {
        return Err(ObjectStreamError::UnsupportedVersion { version });
    }
    let mut roots = read_list(&mut reader, ROOT_DEPTH)?;
    let trailing = bytes.len() - reader.offset;
    if trailing != 0 {
        return Err(ObjectStreamError::TrailingBytes { count: trailing });
    }
    match roots.len() {
        ROOT_COUNT => Ok(roots.remove(0)),
        count => Err(ObjectStreamError::RootCount { count }),
    }
}

/// Writes `root` as a binary ObjectStream version 3. Values up to 7 bytes use the flag bits;
/// longer values use the smallest size field that fits.
pub fn write_object_stream(root: &Element) -> Result<Vec<u8>, ObjectStreamError> {
    let mut out = vec![BINARY_TAG];
    out.extend_from_slice(&OBJECT_STREAM_VERSION.to_be_bytes());
    write_element(root, &mut out)?;
    out.push(END_MARKER);
    Ok(out)
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ObjectStreamError> {
        let end = self
            .offset
            .checked_add(length)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(ObjectStreamError::Truncated {
                offset: self.offset,
            })?;
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ObjectStreamError> {
        let offset = self.offset;
        self.take(N)?
            .try_into()
            .map_err(|_| ObjectStreamError::Truncated { offset })
    }

    fn u8(&mut self) -> Result<u8, ObjectStreamError> {
        self.array().map(u8::from_be_bytes)
    }
}

/// Reads elements at `depth` until the end marker.
fn read_list(reader: &mut Reader<'_>, depth: usize) -> Result<Vec<Element>, ObjectStreamError> {
    let mut list = Vec::new();
    loop {
        let flags_offset = reader.offset;
        let flags = reader.u8()?;
        if flags == END_MARKER {
            return Ok(list);
        }
        if depth > MAX_OBJECT_STREAM_DEPTH {
            return Err(ObjectStreamError::TooDeep);
        }
        if flags & FLAG_ELEMENT_HEADER == 0 {
            return Err(ObjectStreamError::MissingHeaderFlag {
                offset: flags_offset,
                flags,
            });
        }
        let name_crc = if flags & FLAG_HAS_NAME != 0 {
            Some(u32::from_be_bytes(reader.array()?))
        } else {
            None
        };
        let version = if flags & FLAG_HAS_VERSION != 0 {
            Some(reader.u8()?)
        } else {
            None
        };
        let type_id = reader.array()?;
        let size = if flags & FLAG_HAS_VALUE == 0 {
            0
        } else if flags & FLAG_SIZE_FIELD == 0 {
            usize::from(flags & SIZE_BITS)
        } else {
            match flags & SIZE_BITS {
                SIZE_WIDTH_U8 => usize::from(reader.u8()?),
                SIZE_WIDTH_U16 => usize::from(u16::from_be_bytes(reader.array()?)),
                SIZE_WIDTH_U32 => u32::from_be_bytes(reader.array()?) as usize,
                width => {
                    return Err(ObjectStreamError::BadSizeWidth {
                        offset: flags_offset,
                        width,
                    });
                }
            }
        };
        let value = reader.take(size)?.to_vec();
        let children = read_list(reader, depth + 1)?;
        list.push(Element {
            name_crc,
            version,
            type_id,
            value,
            children,
        });
    }
}

fn write_element(element: &Element, out: &mut Vec<u8>) -> Result<(), ObjectStreamError> {
    let size = element.value.len();
    let mut flags = FLAG_ELEMENT_HEADER;
    if element.name_crc.is_some() {
        flags |= FLAG_HAS_NAME;
    }
    if element.version.is_some() {
        flags |= FLAG_HAS_VERSION;
    }
    let mut size_field = Vec::new();
    if size > 0 {
        flags |= FLAG_HAS_VALUE;
        if size <= MAX_INLINE_SIZE {
            // MAX_INLINE_SIZE fits in the 3 size bits.
            flags |= size as u8;
        } else if let Ok(small) = u8::try_from(size) {
            flags |= FLAG_SIZE_FIELD | SIZE_WIDTH_U8;
            size_field.push(small);
        } else if let Ok(medium) = u16::try_from(size) {
            flags |= FLAG_SIZE_FIELD | SIZE_WIDTH_U16;
            size_field.extend_from_slice(&medium.to_be_bytes());
        } else {
            let large =
                u32::try_from(size).map_err(|_| ObjectStreamError::ValueTooLarge { size })?;
            flags |= FLAG_SIZE_FIELD | SIZE_WIDTH_U32;
            size_field.extend_from_slice(&large.to_be_bytes());
        }
    }
    out.push(flags);
    if let Some(name_crc) = element.name_crc {
        out.extend_from_slice(&name_crc.to_be_bytes());
    }
    if let Some(version) = element.version {
        out.push(version);
    }
    out.extend_from_slice(&element.type_id);
    out.extend_from_slice(&size_field);
    out.extend_from_slice(&element.value);
    for child in &element.children {
        write_element(child, out)?;
    }
    out.push(END_MARKER);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME_ELEMENT: u32 = 0x4140_5e39;
    const NAME_CHUNKS: u32 = 0xc74d_c4b5;
    const FLOAT_8400: [u8; 4] = [0x46, 0x03, 0x40, 0x00];
    const TYPE_ID: [u8; UUID_SIZE] = [7; UUID_SIZE];
    const CLASS_VERSION: u8 = 2;
    /// Value sizes that need a 1-, 2- and 4-byte size field.
    const SIZE_U8_FIELD: usize = 200;
    const SIZE_U16_FIELD: usize = 300;
    const SIZE_U32_FIELD: usize = 70_000;
    const HEADER: [u8; 5] = [BINARY_TAG, 0, 0, 0, 3];
    /// Bytes of an element without name, version or value: flags, UUID, end marker.
    const EMPTY_ELEMENT_SIZE: usize = 1 + UUID_SIZE + 1;

    fn sample() -> Element {
        Element {
            version: Some(CLASS_VERSION),
            type_id: TYPE_ID,
            children: vec![
                Element::with_value(NAME_CHUNKS, FLOAT_8400.to_vec()),
                Element::with_children(
                    NAME_ELEMENT,
                    vec![
                        Element::with_value(NAME_ELEMENT, vec![1; SIZE_U8_FIELD]),
                        Element::with_value(NAME_ELEMENT, vec![2; SIZE_U16_FIELD]),
                        Element::with_value(NAME_ELEMENT, vec![3; SIZE_U32_FIELD]),
                    ],
                ),
            ],
            ..Element::default()
        }
    }

    /// A stream whose root holds `depth - 1` nested elements.
    fn nested(depth: usize) -> Vec<u8> {
        let mut out = HEADER.to_vec();
        for _ in 0..depth {
            out.push(FLAG_ELEMENT_HEADER);
            out.extend_from_slice(&TYPE_ID);
        }
        out.resize(out.len() + depth + 1, END_MARKER);
        out
    }

    #[test]
    fn name_crc_is_crc32_of_the_lower_case_name() {
        assert_eq!(lowercase_crc32("element"), NAME_ELEMENT);
        assert_eq!(lowercase_crc32("Chunks"), NAME_CHUNKS);
    }

    #[test]
    fn write_then_parse_keeps_the_tree() {
        let bytes = write_object_stream(&sample()).unwrap();
        assert_eq!(parse_object_stream(&bytes).unwrap(), sample());
    }

    #[test]
    fn reads_a_hand_written_stream() {
        let mut bytes = HEADER.to_vec();
        // Root: header + version, class version 1.
        bytes.extend_from_slice(&[FLAG_ELEMENT_HEADER | FLAG_HAS_VERSION, 1]);
        bytes.extend_from_slice(&TYPE_ID);
        // Child: header + name + value with a 1-byte size field holding 4.
        bytes.push(FLAG_ELEMENT_HEADER | FLAG_HAS_NAME | FLAG_HAS_VALUE | FLAG_SIZE_FIELD | 1);
        bytes.extend_from_slice(&NAME_CHUNKS.to_be_bytes());
        bytes.extend_from_slice(&TYPE_ID);
        bytes.push(4);
        bytes.extend_from_slice(&FLOAT_8400);
        bytes.extend_from_slice(&[END_MARKER, END_MARKER, END_MARKER]);
        let root = parse_object_stream(&bytes).unwrap();
        assert_eq!(root.version, Some(1));
        assert_eq!(root.child(NAME_CHUNKS).unwrap().as_f32().unwrap(), 8400.0);
    }

    #[test]
    fn every_strict_prefix_is_truncated() {
        let bytes = write_object_stream(&Element {
            children: vec![Element::with_value(NAME_CHUNKS, FLOAT_8400.to_vec())],
            ..Element::default()
        })
        .unwrap();
        for length in 0..bytes.len() {
            assert!(
                matches!(
                    parse_object_stream(&bytes[..length]),
                    Err(ObjectStreamError::Truncated { .. })
                ),
                "prefix of {length} bytes"
            );
        }
    }

    #[test]
    fn other_tag_and_version_are_refused() {
        let mut bytes = write_object_stream(&Element::default()).unwrap();
        bytes[4] = 2;
        assert_eq!(
            parse_object_stream(&bytes),
            Err(ObjectStreamError::UnsupportedVersion { version: 2 })
        );
        bytes[0] = 1;
        assert_eq!(
            parse_object_stream(&bytes),
            Err(ObjectStreamError::NotBinary { tag: 1 })
        );
    }

    #[test]
    fn element_without_header_flag_is_refused() {
        let mut bytes = write_object_stream(&Element::default()).unwrap();
        bytes[HEADER.len()] = FLAG_HAS_NAME;
        assert_eq!(
            parse_object_stream(&bytes),
            Err(ObjectStreamError::MissingHeaderFlag {
                offset: HEADER.len(),
                flags: FLAG_HAS_NAME
            })
        );
    }

    #[test]
    fn size_field_width_3_is_refused() {
        let mut bytes = write_object_stream(&Element::default()).unwrap();
        let flags = FLAG_ELEMENT_HEADER | FLAG_HAS_VALUE | FLAG_SIZE_FIELD | 3;
        bytes[HEADER.len()] = flags;
        assert_eq!(
            parse_object_stream(&bytes),
            Err(ObjectStreamError::BadSizeWidth {
                offset: HEADER.len(),
                width: 3
            })
        );
    }

    #[test]
    fn nesting_is_limited() {
        assert!(parse_object_stream(&nested(MAX_OBJECT_STREAM_DEPTH)).is_ok());
        assert_eq!(
            parse_object_stream(&nested(MAX_OBJECT_STREAM_DEPTH + 1)),
            Err(ObjectStreamError::TooDeep)
        );
    }

    #[test]
    fn stream_needs_exactly_one_root() {
        let mut empty = HEADER.to_vec();
        empty.push(END_MARKER);
        assert_eq!(
            parse_object_stream(&empty),
            Err(ObjectStreamError::RootCount { count: 0 })
        );
        let one = write_object_stream(&Element::default()).unwrap();
        let mut two = one[..one.len() - 1].to_vec();
        two.extend_from_slice(&one[HEADER.len()..]);
        assert_eq!(
            parse_object_stream(&two),
            Err(ObjectStreamError::RootCount { count: 2 })
        );
        assert_eq!(one.len(), HEADER.len() + EMPTY_ELEMENT_SIZE + 1);
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut bytes = write_object_stream(&Element::default()).unwrap();
        bytes.push(END_MARKER);
        assert_eq!(
            parse_object_stream(&bytes),
            Err(ObjectStreamError::TrailingBytes { count: 1 })
        );
    }

    #[test]
    fn value_helpers_read_big_endian() {
        let value = |bytes: &[u8]| Element::with_value(NAME_ELEMENT, bytes.to_vec());
        assert!(value(&[1]).as_bool().unwrap());
        assert!(!value(&[0]).as_bool().unwrap());
        assert_eq!(value(&[0xab]).as_u8().unwrap(), 0xab);
        assert_eq!(value(&[0x12, 0x34]).as_u16().unwrap(), 0x1234);
        assert_eq!(value(&[0, 0, 0x12, 0x34]).as_u32().unwrap(), 0x1234);
        assert_eq!(value(&[0, 0, 0, 0, 0, 0, 0, 0x20]).as_u64().unwrap(), 32);
        assert_eq!(value(&FLOAT_8400).as_f32().unwrap(), 8400.0);
        let vector = [FLOAT_8400, FLOAT_8400, [0; 4]].concat();
        assert_eq!(value(&vector).as_vec3().unwrap(), [8400.0, 8400.0, 0.0]);
        assert_eq!(value(b"chunks").as_string().unwrap(), "chunks");
        assert_eq!(value(&TYPE_ID).as_uuid().unwrap(), TYPE_ID);
    }

    #[test]
    fn value_helpers_check_the_size_and_text() {
        let short = Element::with_value(NAME_ELEMENT, vec![0; 3]);
        assert_eq!(
            short.as_u32(),
            Err(ObjectStreamError::ValueSize {
                name_crc: Some(NAME_ELEMENT),
                expected: 4,
                actual: 3
            })
        );
        assert!(short.as_vec3().is_err());
        assert!(short.as_bool().is_err());
        let not_text = Element::with_value(NAME_ELEMENT, vec![0xff]);
        assert_eq!(
            not_text.as_string(),
            Err(ObjectStreamError::NotUtf8 {
                name_crc: Some(NAME_ELEMENT)
            })
        );
    }

    #[test]
    fn child_lookup_uses_the_name_crc() {
        let root = sample();
        assert_eq!(root.child(NAME_CHUNKS).unwrap().value, FLOAT_8400);
        assert!(root.child(0).is_none());
        let element = root.child(NAME_ELEMENT).unwrap();
        assert_eq!(element.children_named(NAME_ELEMENT).count(), 3);
    }
}
