// SPDX-License-Identifier: MIT OR Apache-2.0
//! Localization files (`.loc.xml`): `<resources><string key="...">text</string></resources>`.

use serde::Deserialize;

/// File name ending of localization files.
pub const LOC_EXTENSION: &str = ".loc.xml";
const UTF8_BOM: &str = "\u{feff}";

#[derive(Deserialize)]
struct Resources {
    #[serde(rename = "string", default)]
    strings: Vec<LocString>,
}

#[derive(Deserialize)]
struct LocString {
    #[serde(rename = "@key", default)]
    key: Option<String>,
    #[serde(rename = "$text", default)]
    text: String,
}

/// Parsed content of one localization file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocFile {
    /// `(key, text)` pairs in file order.
    pub entries: Vec<(String, String)>,
    /// `<string>` elements without a `key` attribute (skipped).
    pub keyless: usize,
}

/// Error from localization parsing.
#[derive(Debug, thiserror::Error)]
pub enum LocError {
    /// The file is not UTF-8.
    #[error("localization file is not valid UTF-8: {0}")]
    InvalidUtf8(#[source] std::str::Utf8Error),
    /// The XML does not have the expected shape.
    #[error("invalid localization XML: {0}")]
    Xml(#[source] quick_xml::DeError),
}

/// Parses one `.loc.xml` file.
pub fn parse_loc_xml(bytes: &[u8]) -> Result<LocFile, LocError> {
    let text = std::str::from_utf8(bytes).map_err(LocError::InvalidUtf8)?;
    let text = text.strip_prefix(UTF8_BOM).unwrap_or(text);
    let resources: Resources = quick_xml::de::from_str(text).map_err(LocError::Xml)?;
    let mut file = LocFile::default();
    for string in resources.strings {
        match string.key {
            Some(key) => file.entries.push((key, string.text)),
            None => file.keyless += 1,
        }
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_keys_and_text_in_order() {
        let xml = br#"<resources><string key="a_key">First</string><string key="b_key">Second</string></resources>"#;
        let file = parse_loc_xml(xml).unwrap();
        assert_eq!(
            file.entries,
            vec![
                ("a_key".to_owned(), "First".to_owned()),
                ("b_key".to_owned(), "Second".to_owned())
            ]
        );
        assert_eq!(file.keyless, 0);
    }

    #[test]
    fn unescapes_markup_in_text() {
        let xml =
            br#"<resources><string key="k">but&lt;g id="x" /&gt; &amp; more</string></resources>"#;
        let file = parse_loc_xml(xml).unwrap();
        assert_eq!(file.entries[0].1, r#"but<g id="x" /> & more"#);
    }

    #[test]
    fn counts_strings_without_key() {
        let xml = br#"<resources><string></string><string key="k">v</string></resources>"#;
        let file = parse_loc_xml(xml).unwrap();
        assert_eq!(file.entries.len(), 1);
        assert_eq!(file.keyless, 1);
    }

    #[test]
    fn empty_resources_and_bom_are_accepted() {
        let file = parse_loc_xml("\u{feff}<resources/>".as_bytes()).unwrap();
        assert_eq!(file, LocFile::default());
    }

    #[test]
    fn invalid_utf8_is_an_error() {
        assert!(matches!(
            parse_loc_xml(&[0xFF, 0xFE]),
            Err(LocError::InvalidUtf8(_))
        ));
    }

    #[test]
    fn broken_xml_is_an_error() {
        assert!(matches!(
            parse_loc_xml(br#"<resources><string key="#),
            Err(LocError::Xml(_))
        ));
    }
}
