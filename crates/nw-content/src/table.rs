// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generic table type and the trait that generated row structs implement.

use std::collections::HashMap;

use nw_formats::datasheet::Datasheet;
use serde::{Deserialize, Serialize};

use crate::fill::{FillError, RowView};

/// A row struct generated for one datasheet type.
pub trait FromDatasheet: Sized {
    /// The datasheet type name this struct reads.
    const TYPE_NAME: &'static str;
    /// Reads one row.
    fn from_row(row: &RowView<'_>) -> Result<Self, FillError>;
}

/// All rows of one datasheet file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table<T> {
    /// Entry path of the datasheet inside its pak.
    pub source_path: String,
    /// Unique id stored in the datasheet.
    pub unique_id: String,
    /// The rows.
    pub rows: Vec<T>,
}

impl<T: FromDatasheet> Table<T> {
    /// Reads every row of `sheet` into `T`.
    pub fn fill(source_path: &str, sheet: &Datasheet) -> Result<Self, FillError> {
        let columns: HashMap<&str, usize> = sheet
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| (column.name.as_str(), index))
            .collect();
        let rows = (0..sheet.rows.len())
            .map(|row| T::from_row(&RowView::new(source_path, sheet, &columns, row)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            source_path: source_path.to_owned(),
            unique_id: sheet.unique_id.clone(),
            rows,
        })
    }
}

/// Number of tables and rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TableCounts {
    /// Number of tables (datasheet files).
    pub tables: usize,
    /// Number of rows over all tables.
    pub rows: usize,
}

impl TableCounts {
    /// Adds the tables and rows of one table list.
    pub fn add<T>(&mut self, tables: &[Table<T>]) {
        self.tables += tables.len();
        self.rows += tables.iter().map(|table| table.rows.len()).sum::<usize>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_formats::datasheet::{Column, ColumnKind};

    const SOURCE: &str = "datatables/sample.datasheet";

    #[derive(Debug, Clone, PartialEq)]
    struct Sample {
        name: String,
        level: Option<i64>,
        scale: f64,
        enabled: Option<bool>,
        note: Option<String>,
    }

    impl FromDatasheet for Sample {
        const TYPE_NAME: &'static str = "Sample";
        fn from_row(row: &RowView<'_>) -> Result<Self, FillError> {
            Ok(Self {
                name: row.required_string("Name")?,
                level: row.optional_i64("Level")?,
                scale: row.required_f64("Scale")?,
                enabled: row.optional_bool("Enabled")?,
                note: row.optional_string("Note")?,
            })
        }
    }

    fn sheet(columns: &[(&str, ColumnKind)], rows: &[&[&str]]) -> Datasheet {
        Datasheet {
            type_name: Sample::TYPE_NAME.to_owned(),
            unique_id: "Sample_1".to_owned(),
            columns: columns
                .iter()
                .map(|(name, kind)| Column {
                    name: (*name).to_owned(),
                    kind: *kind,
                })
                .collect(),
            rows: rows
                .iter()
                .map(|row| row.iter().map(|value| (*value).to_owned()).collect())
                .collect(),
        }
    }

    fn full_columns() -> Vec<(&'static str, ColumnKind)> {
        vec![
            ("Name", ColumnKind::String),
            ("Level", ColumnKind::Number),
            ("Scale", ColumnKind::Number),
            ("Enabled", ColumnKind::Boolean),
            ("Note", ColumnKind::String),
        ]
    }

    #[test]
    fn fills_typed_rows() {
        let sheet = sheet(
            &full_columns(),
            &[
                &["sword", "5", "0.12", "TRUE", ""],
                &["shield", "", "1", "0", "x"],
            ],
        );
        let table = Table::<Sample>::fill(SOURCE, &sheet).unwrap();
        assert_eq!(table.source_path, SOURCE);
        assert_eq!(table.unique_id, "Sample_1");
        assert_eq!(
            table.rows,
            vec![
                Sample {
                    name: "sword".into(),
                    level: Some(5),
                    scale: 0.12,
                    enabled: Some(true),
                    note: None
                },
                Sample {
                    name: "shield".into(),
                    level: None,
                    scale: 1.0,
                    enabled: Some(false),
                    note: Some("x".into())
                },
            ]
        );
    }

    #[test]
    fn missing_optional_column_is_none() {
        let sheet = sheet(
            &[("Name", ColumnKind::String), ("Scale", ColumnKind::Number)],
            &[&["sword", "1"]],
        );
        let table = Table::<Sample>::fill(SOURCE, &sheet).unwrap();
        assert_eq!(table.rows[0].level, None);
        assert_eq!(table.rows[0].enabled, None);
    }

    #[test]
    fn missing_required_column_is_an_error() {
        let sheet = sheet(&[("Scale", ColumnKind::Number)], &[&["1"]]);
        assert_eq!(
            Table::<Sample>::fill(SOURCE, &sheet),
            Err(FillError::MissingColumn {
                source_path: SOURCE.into(),
                column: "Name".into()
            })
        );
    }

    #[test]
    fn empty_required_cell_is_an_error() {
        let sheet = sheet(&full_columns(), &[&["", "1", "1", "1", ""]]);
        assert_eq!(
            Table::<Sample>::fill(SOURCE, &sheet),
            Err(FillError::EmptyValue {
                source_path: SOURCE.into(),
                row: 0,
                column: "Name".into()
            })
        );
    }

    #[test]
    fn bad_value_names_row_column_and_text() {
        let sheet = sheet(
            &full_columns(),
            &[&["a", "1", "1", "1", ""], &["b", "1.5", "1", "1", ""]],
        );
        assert_eq!(
            Table::<Sample>::fill(SOURCE, &sheet),
            Err(FillError::BadValue {
                source_path: SOURCE.into(),
                row: 1,
                column: "Level".into(),
                value: "1.5".into(),
                expected: "whole number",
            })
        );
    }

    #[test]
    fn counts_add_tables_and_rows() {
        let sheet = sheet(
            &full_columns(),
            &[&["a", "", "1", "", ""], &["b", "", "2", "", ""]],
        );
        let table = Table::<Sample>::fill(SOURCE, &sheet).unwrap();
        let mut counts = TableCounts::default();
        counts.add(&[table.clone(), table]);
        assert_eq!(counts, TableCounts { tables: 2, rows: 4 });
    }
}
