// SPDX-License-Identifier: AGPL-3.0-or-later
//! Converts datasheet cell text into typed values. These functions are the single definition
//! of the cell rules; `content-tool codegen` uses the same functions to choose field types.

use std::collections::{BTreeMap, HashMap};

use nw_formats::datasheet::Datasheet;

/// Cell texts read as `true` (compared without case).
pub const TRUE_TEXTS: [&str; 2] = ["1", "true"];
/// Cell texts read as `false` (compared without case).
pub const FALSE_TEXTS: [&str; 2] = ["0", "false"];

const EXPECTED_BOOL: &str = "boolean";
const EXPECTED_I64: &str = "whole number";
const EXPECTED_F64: &str = "number";
const EXPECTED_TEXT: &str = "text";

/// Reads `1`/`0`/`TRUE`/`FALSE` (any case).
pub fn parse_bool(text: &str) -> Option<bool> {
    if TRUE_TEXTS
        .iter()
        .any(|candidate| text.eq_ignore_ascii_case(candidate))
    {
        Some(true)
    } else if FALSE_TEXTS
        .iter()
        .any(|candidate| text.eq_ignore_ascii_case(candidate))
    {
        Some(false)
    } else {
        None
    }
}

/// Reads a whole number.
pub fn parse_i64(text: &str) -> Option<i64> {
    text.parse().ok()
}

/// Reads a whole or decimal number.
pub fn parse_f64(text: &str) -> Option<f64> {
    text.parse().ok()
}

/// Reads a text cell as it is.
fn parse_text(text: &str) -> Option<String> {
    Some(text.to_owned())
}

/// Splits a numbered column name such as `Item12` into its base (`Item`) and number (`12`).
/// `None` when the name has no trailing digits, no base, or digits too large for `u32`.
pub fn split_numbered(column: &str) -> Option<(&str, u32)> {
    let base = column.trim_end_matches(|ch: char| ch.is_ascii_digit());
    let number = column[base.len()..].parse().ok()?;
    (!base.is_empty()).then_some((base, number))
}

/// Error from filling a typed row.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FillError {
    /// A required column is not in the file.
    #[error("{source_path}: column `{column}` is missing")]
    MissingColumn {
        /// Datasheet path.
        source_path: String,
        /// Column name.
        column: String,
    },
    /// A required cell is empty.
    #[error("{source_path}: row {row}, column `{column}` is empty")]
    EmptyValue {
        /// Datasheet path.
        source_path: String,
        /// Row index.
        row: usize,
        /// Column name.
        column: String,
    },
    /// A cell does not fit the field type.
    #[error("{source_path}: row {row}, column `{column}`: `{value}` is not a valid {expected}")]
    BadValue {
        /// Datasheet path.
        source_path: String,
        /// Row index.
        row: usize,
        /// Column name.
        column: String,
        /// The cell text.
        value: String,
        /// The expected kind of value.
        expected: &'static str,
    },
    /// The datasheet type has no generated struct.
    #[error("{source_path}: unknown table type `{type_name}`")]
    UnknownTableType {
        /// Datasheet path.
        source_path: String,
        /// The table type name.
        type_name: String,
    },
}

/// One row of a datasheet, read by column name.
pub struct RowView<'a> {
    source_path: &'a str,
    sheet: &'a Datasheet,
    columns: &'a HashMap<&'a str, usize>,
    row: usize,
}

impl<'a> RowView<'a> {
    pub(crate) fn new(
        source_path: &'a str,
        sheet: &'a Datasheet,
        columns: &'a HashMap<&'a str, usize>,
        row: usize,
    ) -> Self {
        Self {
            source_path,
            sheet,
            columns,
            row,
        }
    }

    /// Text of a required, non-empty cell.
    pub fn required_string(&self, column: &str) -> Result<String, FillError> {
        self.required_text(column).map(str::to_owned)
    }

    /// Text of a cell; `None` when the column is missing or the cell is empty.
    pub fn optional_string(&self, column: &str) -> Result<Option<String>, FillError> {
        Ok(self.optional_text(column).map(str::to_owned))
    }

    /// Whole number of a required cell.
    pub fn required_i64(&self, column: &str) -> Result<i64, FillError> {
        self.required(column, EXPECTED_I64, parse_i64)
    }

    /// Whole number of a cell; `None` when the column is missing or the cell is empty.
    pub fn optional_i64(&self, column: &str) -> Result<Option<i64>, FillError> {
        self.optional(column, EXPECTED_I64, parse_i64)
    }

    /// Number of a required cell.
    pub fn required_f64(&self, column: &str) -> Result<f64, FillError> {
        self.required(column, EXPECTED_F64, parse_f64)
    }

    /// Number of a cell; `None` when the column is missing or the cell is empty.
    pub fn optional_f64(&self, column: &str) -> Result<Option<f64>, FillError> {
        self.optional(column, EXPECTED_F64, parse_f64)
    }

    /// Boolean of a required cell.
    pub fn required_bool(&self, column: &str) -> Result<bool, FillError> {
        self.required(column, EXPECTED_BOOL, parse_bool)
    }

    /// Boolean of a cell; `None` when the column is missing or the cell is empty.
    pub fn optional_bool(&self, column: &str) -> Result<Option<bool>, FillError> {
        self.optional(column, EXPECTED_BOOL, parse_bool)
    }

    /// Texts of the non-empty cells of the columns `<base>1`, `<base>2`, ..., by number.
    pub fn numbered_string(&self, base: &str) -> Result<BTreeMap<u32, String>, FillError> {
        self.numbered(base, EXPECTED_TEXT, parse_text)
    }

    /// Whole numbers of the non-empty cells of the columns `<base>1`, `<base>2`, ..., by number.
    pub fn numbered_i64(&self, base: &str) -> Result<BTreeMap<u32, i64>, FillError> {
        self.numbered(base, EXPECTED_I64, parse_i64)
    }

    /// Numbers of the non-empty cells of the columns `<base>1`, `<base>2`, ..., by number.
    pub fn numbered_f64(&self, base: &str) -> Result<BTreeMap<u32, f64>, FillError> {
        self.numbered(base, EXPECTED_F64, parse_f64)
    }

    /// Booleans of the non-empty cells of the columns `<base>1`, `<base>2`, ..., by number.
    pub fn numbered_bool(&self, base: &str) -> Result<BTreeMap<u32, bool>, FillError> {
        self.numbered(base, EXPECTED_BOOL, parse_bool)
    }

    fn cell(&self, column: &str) -> Option<&'a str> {
        let index = *self.columns.get(column)?;
        self.sheet
            .rows
            .get(self.row)?
            .get(index)
            .map(String::as_str)
    }

    fn required_text(&self, column: &str) -> Result<&'a str, FillError> {
        if !self.columns.contains_key(column) {
            return Err(FillError::MissingColumn {
                source_path: self.source_path.to_owned(),
                column: column.to_owned(),
            });
        }
        match self.cell(column) {
            Some(text) if !text.is_empty() => Ok(text),
            _ => Err(FillError::EmptyValue {
                source_path: self.source_path.to_owned(),
                row: self.row,
                column: column.to_owned(),
            }),
        }
    }

    fn optional_text(&self, column: &str) -> Option<&'a str> {
        self.cell(column).filter(|text| !text.is_empty())
    }

    fn convert<T>(
        &self,
        column: &str,
        text: &str,
        expected: &'static str,
        parse: fn(&str) -> Option<T>,
    ) -> Result<T, FillError> {
        parse(text).ok_or_else(|| FillError::BadValue {
            source_path: self.source_path.to_owned(),
            row: self.row,
            column: column.to_owned(),
            value: text.to_owned(),
            expected,
        })
    }

    fn required<T>(
        &self,
        column: &str,
        expected: &'static str,
        parse: fn(&str) -> Option<T>,
    ) -> Result<T, FillError> {
        let text = self.required_text(column)?;
        self.convert(column, text, expected, parse)
    }

    fn numbered<T>(
        &self,
        base: &str,
        expected: &'static str,
        parse: fn(&str) -> Option<T>,
    ) -> Result<BTreeMap<u32, T>, FillError> {
        let mut values = BTreeMap::new();
        for column in &self.sheet.columns {
            let Some((column_base, number)) = split_numbered(&column.name) else {
                continue;
            };
            if column_base != base {
                continue;
            }
            if let Some(text) = self.optional_text(&column.name) {
                values.insert(number, self.convert(&column.name, text, expected, parse)?);
            }
        }
        Ok(values)
    }

    fn optional<T>(
        &self,
        column: &str,
        expected: &'static str,
        parse: fn(&str) -> Option<T>,
    ) -> Result<Option<T>, FillError> {
        self.optional_text(column)
            .map(|text| self.convert(column, text, expected, parse))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use nw_formats::datasheet::{Column, ColumnKind};

    use super::*;

    const SOURCE: &str = "sheet.datasheet";

    fn sheet(columns: &[&str], row: &[&str]) -> Datasheet {
        Datasheet {
            type_name: "Test".to_owned(),
            unique_id: "test".to_owned(),
            columns: columns
                .iter()
                .map(|name| Column {
                    name: (*name).to_owned(),
                    kind: ColumnKind::String,
                })
                .collect(),
            rows: vec![row.iter().map(|value| (*value).to_owned()).collect()],
        }
    }

    fn with_row_view<T>(columns: &[&str], row: &[&str], read: impl FnOnce(&RowView<'_>) -> T) -> T {
        let sheet = sheet(columns, row);
        let index: HashMap<&str, usize> = columns
            .iter()
            .enumerate()
            .map(|(position, name)| (*name, position))
            .collect();
        read(&RowView::new(SOURCE, &sheet, &index, 0))
    }

    #[test]
    fn bool_accepts_digits_and_words_in_any_case() {
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("TRUE"), Some(true));
        assert_eq!(parse_bool("True"), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("2"), None);
        assert_eq!(parse_bool("yes"), None);
    }

    #[test]
    fn numbers_parse_whole_and_decimal() {
        assert_eq!(parse_i64("-42"), Some(-42));
        assert_eq!(parse_i64("0.5"), None);
        assert_eq!(parse_f64("0.12"), Some(0.12));
        assert_eq!(parse_f64("7"), Some(7.0));
        assert_eq!(parse_f64("abc"), None);
    }

    #[test]
    fn split_numbered_reads_trailing_digits() {
        assert_eq!(split_numbered("Item12"), Some(("Item", 12)));
        assert_eq!(split_numbered("Tags2D3"), Some(("Tags2D", 3)));
        assert_eq!(split_numbered("Item"), None);
        assert_eq!(split_numbered("123"), None);
        assert_eq!(split_numbered("Item1x"), None);
        assert_eq!(split_numbered("Item99999999999"), None);
    }

    #[test]
    fn numbered_values_skip_gaps_and_empty_cells() {
        let columns = ["Base1", "Base3", "Base4", "Base10", "Other2"];
        let row = ["5", "7", "", "-1", "9"];
        let values = with_row_view(&columns, &row, |view| view.numbered_i64("Base")).unwrap();
        assert_eq!(values, BTreeMap::from([(1, 5), (3, 7), (10, -1)]));
    }

    #[test]
    fn numbered_column_with_a_suffix_is_not_a_member() {
        let columns = ["Base1", "Base1x", "Base2y3"];
        let row = ["a", "b", "c"];
        let values = with_row_view(&columns, &row, |view| view.numbered_string("Base")).unwrap();
        assert_eq!(values, BTreeMap::from([(1, "a".to_owned())]));
    }

    #[test]
    fn numbered_readers_cover_every_type() {
        let columns = ["Value1", "Value2"];
        let row = ["1", "0"];
        with_row_view(&columns, &row, |view| {
            assert_eq!(
                view.numbered_bool("Value").unwrap(),
                BTreeMap::from([(1, true), (2, false)])
            );
            assert_eq!(
                view.numbered_f64("Value").unwrap(),
                BTreeMap::from([(1, 1.0), (2, 0.0)])
            );
        });
    }

    #[test]
    fn numbered_bad_value_names_the_full_column() {
        let columns = ["Base1", "Base2"];
        let row = ["4", "x"];
        let error = with_row_view(&columns, &row, |view| view.numbered_i64("Base")).unwrap_err();
        assert_eq!(
            error,
            FillError::BadValue {
                source_path: SOURCE.to_owned(),
                row: 0,
                column: "Base2".to_owned(),
                value: "x".to_owned(),
                expected: EXPECTED_I64,
            }
        );
    }
}
