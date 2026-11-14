// SPDX-License-Identifier: AGPL-3.0-or-later
//! The game's own strings (every string of the merged content bundle), for the privacy scan:
//! readable text in a pack table that is a game string is a game name, not a player's.
//!
//! The strings are taken by a `serde::Serializer` that keeps only the strings, so no copy of the
//! whole bundle is made.

use std::collections::BTreeSet;
use std::fmt;

use nw_content::ContentBundle;
use serde::ser::{self, Serialize};

/// The game's strings, sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameWords {
    strings: Vec<String>,
}

impl GameWords {
    /// The strings of `bundle`.
    pub fn from_bundle(bundle: &ContentBundle) -> Self {
        let mut collector = StringCollector::default();
        // The collector cannot fail: every method records or ignores.
        bundle
            .serialize(&mut collector)
            .expect("the string collector never fails");
        Self::from_strings(collector.strings)
    }

    /// The given strings.
    pub fn from_strings(strings: impl IntoIterator<Item = String>) -> Self {
        Self {
            strings: strings
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        }
    }

    /// Whether `word` is a game string.
    pub fn contains(&self, word: &str) -> bool {
        self.strings
            .binary_search_by(|string| string.as_str().cmp(word))
            .is_ok()
    }
}

/// The never-raised error of [`StringCollector`].
#[derive(Debug)]
pub struct CollectError(String);

impl fmt::Display for CollectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CollectError {}

impl ser::Error for CollectError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

/// A serializer that keeps every string and ignores every other value.
#[derive(Default)]
struct StringCollector {
    strings: BTreeSet<String>,
}

/// Ignored scalar methods.
macro_rules! ignore {
    ($($method:ident: $kind:ty),* $(,)?) => {
        $(fn $method(self, _: $kind) -> Result<(), CollectError> { Ok(()) })*
    };
}

impl ser::Serializer for &mut StringCollector {
    type Ok = ();
    type Error = CollectError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    ignore!(
        serialize_bool: bool,
        serialize_i8: i8,
        serialize_i16: i16,
        serialize_i32: i32,
        serialize_i64: i64,
        serialize_u8: u8,
        serialize_u16: u16,
        serialize_u32: u32,
        serialize_u64: u64,
        serialize_f32: f32,
        serialize_f64: f64,
        serialize_char: char,
        serialize_bytes: &[u8],
        serialize_unit_struct: &'static str,
    );

    fn serialize_str(self, value: &str) -> Result<(), CollectError> {
        if !self.strings.contains(value) {
            self.strings.insert(value.to_owned());
        }
        Ok(())
    }

    fn serialize_none(self) -> Result<(), CollectError> {
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), CollectError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), CollectError> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), CollectError> {
        Ok(())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), CollectError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), CollectError> {
        value.serialize(self)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self, CollectError> {
        Ok(self)
    }

    fn serialize_tuple(self, _: usize) -> Result<Self, CollectError> {
        Ok(self)
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, CollectError> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, CollectError> {
        Ok(self)
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self, CollectError> {
        Ok(self)
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, CollectError> {
        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, CollectError> {
        Ok(self)
    }
}

/// The compound methods of [`StringCollector`]: every element goes through the collector.
macro_rules! compound {
    ($($trait:ident :: $method:ident),* $(,)?) => {
        $(
            impl ser::$trait for &mut StringCollector {
                type Ok = ();
                type Error = CollectError;

                fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CollectError> {
                    value.serialize(&mut **self)
                }

                fn end(self) -> Result<(), CollectError> {
                    Ok(())
                }
            }
        )*
    };
}

compound!(
    SerializeSeq::serialize_element,
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
    SerializeTupleVariant::serialize_field,
);

impl ser::SerializeMap for &mut StringCollector {
    type Ok = ();
    type Error = CollectError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), CollectError> {
        key.serialize(&mut **self)
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CollectError> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), CollectError> {
        Ok(())
    }
}

impl ser::SerializeStruct for &mut StringCollector {
    type Ok = ();
    type Error = CollectError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _: &'static str,
        value: &T,
    ) -> Result<(), CollectError> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), CollectError> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut StringCollector {
    type Ok = ();
    type Error = CollectError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _: &'static str,
        value: &T,
    ) -> Result<(), CollectError> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), CollectError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[derive(serde::Serialize)]
    struct Row {
        id: String,
        count: u32,
        tags: Vec<String>,
        names: BTreeMap<String, Option<String>>,
    }

    #[test]
    fn collects_every_string_and_nothing_else() {
        let row = Row {
            id: "OutpostRushWood".to_owned(),
            count: 3,
            tags: vec!["Armory".to_owned()],
            names: BTreeMap::from([("key".to_owned(), Some("value".to_owned()))]),
        };
        let mut collector = StringCollector::default();
        row.serialize(&mut collector).unwrap();
        let strings: Vec<_> = collector.strings.into_iter().collect();
        assert_eq!(strings, ["Armory", "OutpostRushWood", "key", "value"]);
    }

    #[test]
    fn a_game_string_is_found_exactly() {
        let words =
            GameWords::from_strings(["OutpostRushWood".to_owned(), "SiegeSupplyWood".to_owned()]);
        assert!(words.contains("OutpostRushWood"));
        assert!(
            !words.contains("OutpostRush"),
            "a part of a game string is no game string"
        );
        assert!(!words.contains("Vszef"));
    }
}
