// SPDX-License-Identifier: AGPL-3.0-or-later
//! Chooses one Rust field type per column over all datasheets of a table type (M1 spec 4.1).

use std::collections::BTreeMap;

use nw_content::fill::{parse_bool, parse_f64, parse_i64, split_numbered};
use nw_formats::datasheet::{ColumnKind, Datasheet};

use super::CodegenError;
use super::naming::{FieldName, field_names, is_valid_type_name, snake_case, struct_name};

/// A base with at least this many numbered columns in one table type becomes one map field.
pub const FAMILY_MIN_MEMBERS: usize = 16;

/// Rust type of a generated field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// `bool`.
    Bool,
    /// `i64`.
    I64,
    /// `f64`.
    F64,
    /// `String`.
    String,
}

impl FieldType {
    /// Suffix of the matching `RowView` method, for example `i64` in `required_i64`.
    pub fn method_suffix(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::I64 => "i64",
            Self::F64 => "f64",
            Self::String => "string",
        }
    }
}

/// One generated field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSchema {
    /// Original column name; the fill code matches columns by this exact name. For a numbered
    /// field it is the base name shared by the numbered columns.
    pub column: String,
    /// Generated field name.
    pub name: FieldName,
    /// Field type.
    pub ty: FieldType,
    /// True when the field is `Option<ty>`. Never true for a numbered field.
    pub optional: bool,
    /// True when the field is a `BTreeMap<u32, ty>` over the columns `<column>1`, `<column>2`, ...
    pub numbered: bool,
}

/// One generated struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    /// Datasheet type name.
    pub type_name: String,
    /// Struct name: the upper camel case form of `type_name`.
    pub struct_name: String,
    /// Module (file) name and `Tables` field name.
    pub module: String,
    /// Fields, sorted by original column name.
    pub fields: Vec<FieldSchema>,
}

/// Facts about one column over all files of its table type. The `any_non_*` flags start false
/// and become true at the first value that does not parse as that type.
#[derive(Debug, Default)]
struct ColumnStats {
    files: usize,
    declared_boolean: bool,
    any_empty: bool,
    any_value: bool,
    any_non_bool: bool,
    any_non_i64: bool,
    any_non_f64: bool,
}

impl ColumnStats {
    fn observe(&mut self, value: &str) {
        if value.is_empty() {
            self.any_empty = true;
            return;
        }
        self.any_value = true;
        // Skip a parse once its flag is set; the flag never goes back to false.
        if !self.any_non_bool {
            self.any_non_bool = parse_bool(value).is_none();
        }
        if !self.any_non_i64 {
            self.any_non_i64 = parse_i64(value).is_none();
        }
        if !self.any_non_f64 {
            self.any_non_f64 = parse_f64(value).is_none();
        }
    }

    /// Adds the facts of `other`, as if its values were observed in this column.
    fn merge(&mut self, other: &Self) {
        self.declared_boolean |= other.declared_boolean;
        self.any_empty |= other.any_empty;
        self.any_value |= other.any_value;
        self.any_non_bool |= other.any_non_bool;
        self.any_non_i64 |= other.any_non_i64;
        self.any_non_f64 |= other.any_non_f64;
    }

    fn field_type(&self) -> FieldType {
        if !self.any_value {
            FieldType::String
        } else if !self.any_non_bool && self.declared_boolean {
            FieldType::Bool
        } else if !self.any_non_i64 {
            FieldType::I64
        } else if !self.any_non_f64 {
            FieldType::F64
        } else {
            FieldType::String
        }
    }
}

#[derive(Debug, Default)]
struct TableStats {
    files: usize,
    columns: BTreeMap<String, ColumnStats>,
}

/// A field before naming: one column, or the merged numbered columns of one base.
struct PlannedField {
    column: String,
    numbered: bool,
    ty: FieldType,
    optional: bool,
}

impl TableStats {
    /// One planned field per plain column and per numbered family, sorted by column or base
    /// name (a plain column before a family with the same name).
    fn plan_fields(&self) -> Vec<PlannedField> {
        let mut member_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for column in self.columns.keys() {
            if let Some((base, _)) = split_numbered(column) {
                *member_counts.entry(base).or_default() += 1;
            }
        }
        let mut families: BTreeMap<&str, ColumnStats> = BTreeMap::new();
        let mut planned = Vec::new();
        for (column, stats) in &self.columns {
            let family = split_numbered(column)
                .map(|(base, _)| base)
                .filter(|base| member_counts[base] >= FAMILY_MIN_MEMBERS);
            match family {
                Some(base) => families.entry(base).or_default().merge(stats),
                None => planned.push(PlannedField {
                    column: column.clone(),
                    numbered: false,
                    ty: stats.field_type(),
                    optional: stats.files < self.files || stats.any_empty || !stats.any_value,
                }),
            }
        }
        planned.extend(families.into_iter().map(|(base, stats)| PlannedField {
            column: base.to_owned(),
            numbered: true,
            ty: stats.field_type(),
            optional: false,
        }));
        planned.sort_by(|left, right| {
            (&left.column, left.numbered).cmp(&(&right.column, right.numbered))
        });
        planned
    }
}

/// Collects column facts over datasheets, then builds the table schemas.
#[derive(Debug, Default)]
pub struct SchemaBuilder {
    tables: BTreeMap<String, TableStats>,
}

impl SchemaBuilder {
    /// Adds one datasheet.
    pub fn add(&mut self, sheet: &Datasheet) {
        let table = self.tables.entry(sheet.type_name.clone()).or_default();
        table.files += 1;
        for (index, column) in sheet.columns.iter().enumerate() {
            let stats = table.columns.entry(column.name.clone()).or_default();
            stats.files += 1;
            stats.declared_boolean |= column.kind == ColumnKind::Boolean;
            for row in &sheet.rows {
                if let Some(value) = row.get(index) {
                    stats.observe(value);
                }
            }
        }
    }

    /// Builds one schema per table type, sorted by type name.
    pub fn finish(self) -> Result<Vec<TableSchema>, CodegenError> {
        if self.tables.is_empty() {
            return Err(CodegenError::NoTables);
        }
        let mut modules: BTreeMap<String, String> = BTreeMap::new();
        let mut structs: BTreeMap<String, String> = BTreeMap::new();
        let mut schemas = Vec::with_capacity(self.tables.len());
        for (type_name, table) in self.tables {
            let module = snake_case(&type_name);
            let struct_name = struct_name(&type_name);
            if !is_valid_type_name(&struct_name) || !is_valid_type_name(&module) {
                return Err(CodegenError::InvalidTypeName(type_name));
            }
            if table.columns.is_empty() {
                return Err(CodegenError::NoColumns(type_name));
            }
            if let Some(first) = modules.insert(module.clone(), type_name.clone()) {
                return Err(CodegenError::ModuleCollision {
                    first,
                    second: type_name,
                    module,
                });
            }
            if let Some(first) = structs.insert(struct_name.clone(), type_name.clone()) {
                return Err(CodegenError::StructCollision {
                    first,
                    second: type_name,
                    struct_name,
                });
            }
            let planned = table.plan_fields();
            let names = field_names(planned.iter().map(|field| field.column.as_str()));
            let fields = planned
                .into_iter()
                .zip(names)
                .map(|(field, name)| FieldSchema {
                    column: field.column,
                    name,
                    ty: field.ty,
                    optional: field.optional,
                    numbered: field.numbered,
                })
                .collect();
            schemas.push(TableSchema {
                type_name,
                struct_name,
                module,
                fields,
            });
        }
        Ok(schemas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_formats::datasheet::Column;

    const TYPE: &str = "MasterItemDefinitions";

    fn sheet(type_name: &str, columns: &[(&str, ColumnKind)], rows: &[&[&str]]) -> Datasheet {
        Datasheet {
            type_name: type_name.to_owned(),
            unique_id: format!("{type_name}_id"),
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

    fn build(sheets: &[Datasheet]) -> Result<Vec<TableSchema>, CodegenError> {
        let mut builder = SchemaBuilder::default();
        for sheet in sheets {
            builder.add(sheet);
        }
        builder.finish()
    }

    fn field<'a>(schema: &'a TableSchema, column: &str) -> &'a FieldSchema {
        schema
            .fields
            .iter()
            .find(|field| field.column == column)
            .unwrap()
    }

    #[test]
    fn values_choose_the_field_type() {
        let schemas = build(&[sheet(
            TYPE,
            &[
                ("Flag", ColumnKind::Boolean),
                ("Count", ColumnKind::Number),
                ("Ratio", ColumnKind::Number),
                ("Name", ColumnKind::String),
                ("Mixed", ColumnKind::Number),
            ],
            &[
                &["1", "3", "0.5", "a", "1"],
                &["FALSE", "-4", "2", "b", "x"],
            ],
        )])
        .unwrap();
        let schema = &schemas[0];
        assert_eq!(field(schema, "Flag").ty, FieldType::Bool);
        assert_eq!(field(schema, "Count").ty, FieldType::I64);
        assert_eq!(field(schema, "Ratio").ty, FieldType::F64);
        assert_eq!(field(schema, "Name").ty, FieldType::String);
        assert_eq!(field(schema, "Mixed").ty, FieldType::String);
        assert!(schema.fields.iter().all(|field| !field.optional));
    }

    #[test]
    fn bool_needs_a_declared_boolean() {
        let schemas = build(&[sheet(
            TYPE,
            &[("Bit", ColumnKind::Number)],
            &[&["0"], &["1"]],
        )])
        .unwrap();
        assert_eq!(field(&schemas[0], "Bit").ty, FieldType::I64);
    }

    #[test]
    fn kind_drift_across_files_keeps_one_type() {
        let schemas = build(&[
            sheet(
                TYPE,
                &[
                    ("Repeatable", ColumnKind::Boolean),
                    ("Level", ColumnKind::Boolean),
                ],
                &[&["1", "1"]],
            ),
            sheet(
                TYPE,
                &[
                    ("Repeatable", ColumnKind::Number),
                    ("Level", ColumnKind::Number),
                ],
                &[&["0", "2"]],
            ),
        ])
        .unwrap();
        assert_eq!(field(&schemas[0], "Repeatable").ty, FieldType::Bool);
        assert_eq!(field(&schemas[0], "Level").ty, FieldType::I64);
    }

    #[test]
    fn missing_column_or_empty_cell_makes_a_field_optional() {
        let schemas = build(&[
            sheet(
                TYPE,
                &[("A", ColumnKind::String), ("B", ColumnKind::String)],
                &[&["x", "y"]],
            ),
            sheet(
                TYPE,
                &[("A", ColumnKind::String), ("C", ColumnKind::String)],
                &[&["", "z"]],
            ),
        ])
        .unwrap();
        let schema = &schemas[0];
        assert!(field(schema, "A").optional, "empty cell");
        assert!(field(schema, "B").optional, "missing in file 2");
        assert!(field(schema, "C").optional, "missing in file 1");
    }

    #[test]
    fn column_without_values_is_optional_string() {
        let schemas = build(&[sheet(
            TYPE,
            &[("Unused", ColumnKind::Number)],
            &[&[""], &[""]],
        )])
        .unwrap();
        let unused = field(&schemas[0], "Unused");
        assert_eq!(unused.ty, FieldType::String);
        assert!(unused.optional);
    }

    #[test]
    fn fields_are_sorted_and_named() {
        let schemas = build(&[sheet(
            TYPE,
            &[
                ("Type", ColumnKind::String),
                ("ItemID", ColumnKind::String),
                ("GatherableEntryId", ColumnKind::String),
                ("GatherableEntryID", ColumnKind::String),
            ],
            &[&["a", "b", "c", "d"]],
        )])
        .unwrap();
        let schema = &schemas[0];
        assert_eq!(schema.module, "master_item_definitions");
        let names: Vec<(&str, &str, bool)> = schema
            .fields
            .iter()
            .map(|field| {
                (
                    field.column.as_str(),
                    field.name.text.as_str(),
                    field.name.raw,
                )
            })
            .collect();
        assert_eq!(
            names,
            vec![
                ("GatherableEntryID", "gatherable_entry_id", false),
                ("GatherableEntryId", "gatherable_entry_id_2", false),
                ("ItemID", "item_id", false),
                ("Type", "type", true),
            ]
        );
    }

    #[test]
    fn tables_are_sorted_by_type_name() {
        let schemas = build(&[
            sheet("ZoneData", &[("A", ColumnKind::String)], &[]),
            sheet("AffixData", &[("A", ColumnKind::String)], &[]),
        ])
        .unwrap();
        let types: Vec<&str> = schemas
            .iter()
            .map(|schema| schema.type_name.as_str())
            .collect();
        assert_eq!(types, vec!["AffixData", "ZoneData"]);
    }

    /// A sheet with the columns `<base>1` to `<base>count`, then the `extra` columns, and one
    /// row where numbered column `n` holds `values(n)`.
    fn numbered_schema(
        count: usize,
        values: impl Fn(usize) -> String,
        extra: &[(&str, ColumnKind)],
    ) -> TableSchema {
        let names: Vec<String> = (1..=count).map(|number| format!("Item{number}")).collect();
        let mut columns: Vec<(&str, ColumnKind)> = names
            .iter()
            .map(|name| (name.as_str(), ColumnKind::String))
            .collect();
        columns.extend_from_slice(extra);
        let mut row: Vec<String> = (1..=count).map(values).collect();
        row.extend(extra.iter().map(|_| "7".to_owned()));
        let row: Vec<&str> = row.iter().map(String::as_str).collect();
        build(&[sheet(TYPE, &columns, &[&row])]).unwrap().remove(0)
    }

    #[test]
    fn sixteen_numbered_columns_become_one_field() {
        let schema = numbered_schema(
            FAMILY_MIN_MEMBERS,
            |number| number.to_string(),
            &[("Name", ColumnKind::String)],
        );
        let columns: Vec<(&str, bool)> = schema
            .fields
            .iter()
            .map(|field| (field.column.as_str(), field.numbered))
            .collect();
        assert_eq!(columns, vec![("Item", true), ("Name", false)]);
        let family = field(&schema, "Item");
        assert_eq!(family.name.text, "item");
        assert_eq!(family.ty, FieldType::I64);
        assert!(!family.optional);
    }

    #[test]
    fn fewer_numbered_columns_stay_separate_fields() {
        let schema = numbered_schema(FAMILY_MIN_MEMBERS - 1, |number| number.to_string(), &[]);
        assert_eq!(schema.fields.len(), FAMILY_MIN_MEMBERS - 1);
        assert!(schema.fields.iter().all(|field| !field.numbered));
    }

    #[test]
    fn family_type_merges_all_members() {
        let schema = numbered_schema(
            FAMILY_MIN_MEMBERS,
            |number| {
                if number == 3 {
                    "0.5".to_owned()
                } else {
                    number.to_string()
                }
            },
            &[],
        );
        assert_eq!(field(&schema, "Item").ty, FieldType::F64);
    }

    #[test]
    fn family_without_values_is_a_string_map() {
        let schema = numbered_schema(FAMILY_MIN_MEMBERS, |_| String::new(), &[]);
        let family = field(&schema, "Item");
        assert_eq!(family.ty, FieldType::String);
        assert!(!family.optional);
    }

    #[test]
    fn gaps_in_the_numbers_still_make_a_family() {
        let names: Vec<String> = (0..FAMILY_MIN_MEMBERS)
            .map(|index| format!("Slot{}", index * 3 + 1))
            .collect();
        let columns: Vec<(&str, ColumnKind)> = names
            .iter()
            .map(|name| (name.as_str(), ColumnKind::Number))
            .collect();
        let schemas = build(&[sheet(TYPE, &columns, &[])]).unwrap();
        assert_eq!(schemas[0].fields.len(), 1);
        assert!(schemas[0].fields[0].numbered);
        assert_eq!(schemas[0].fields[0].column, "Slot");
    }

    #[test]
    fn family_name_avoids_a_plain_column_of_the_same_name() {
        let schema = numbered_schema(
            FAMILY_MIN_MEMBERS,
            |number| number.to_string(),
            &[("Item", ColumnKind::Number)],
        );
        let names: Vec<(&str, bool)> = schema
            .fields
            .iter()
            .map(|field| (field.name.text.as_str(), field.numbered))
            .collect();
        assert_eq!(names, vec![("item", false), ("item_2", true)]);
    }

    #[test]
    fn schema_errors_are_reported() {
        assert!(matches!(build(&[]), Err(CodegenError::NoTables)));
        assert!(matches!(
            build(&[sheet("Bad-Name", &[("A", ColumnKind::String)], &[])]),
            Err(CodegenError::InvalidTypeName(name)) if name == "Bad-Name"
        ));
        assert!(
            matches!(build(&[sheet(TYPE, &[], &[])]), Err(CodegenError::NoColumns(name)) if name == TYPE)
        );
        assert!(matches!(
            build(&[
                sheet("ABData", &[("A", ColumnKind::String)], &[]),
                sheet("AbData", &[("A", ColumnKind::String)], &[]),
            ]),
            Err(CodegenError::ModuleCollision { first, second, module })
                if first == "ABData" && second == "AbData" && module == "ab_data"
        ));
        assert!(matches!(
            build(&[
                sheet("AB_c", &[("A", ColumnKind::String)], &[]),
                sheet("ABC", &[("A", ColumnKind::String)], &[]),
            ]),
            Err(CodegenError::StructCollision { first, second, struct_name })
                if first == "ABC" && second == "AB_c" && struct_name == "ABC"
        ));
    }
}
