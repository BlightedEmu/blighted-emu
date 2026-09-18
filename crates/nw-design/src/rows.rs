// SPDX-License-Identifier: AGPL-3.0-or-later
//! Merges design files of table kinds into the client tables (spec section 5.1).

use nw_content::Tables;
use nw_content::table::Table;

use crate::error::{DesignError, Problem};
use crate::file::{Action, DesignFile, DesignSet, Row, RowVisitor};
use crate::schema::row_from_toml;

/// `source_path` prefix of the tables that hold added rows: `design/<kind folder>`.
pub const DESIGN_SOURCE_PREFIX: &str = "design/";
/// `unique_id` of the tables that hold added rows.
const DESIGN_UNIQUE_ID: &str = "design";

/// Applies every table-kind file of `set` to `tables`, in set order. A file with an error
/// changes nothing; the other files still apply.
pub fn apply_tables(set: &DesignSet, tables: &mut Tables) -> Vec<DesignError> {
    let mut errors = Vec::new();
    for file in &set.files {
        if let Some(Err(error)) = file.kind.visit_row(ApplyRow {
            file,
            tables: &mut *tables,
        }) {
            errors.push(error);
        }
    }
    errors
}

struct ApplyRow<'a> {
    file: &'a DesignFile,
    tables: &'a mut Tables,
}

impl RowVisitor for ApplyRow<'_> {
    type Output = Result<(), DesignError>;

    fn visit<T: Row>(self) -> Result<(), DesignError> {
        let file = self.file;
        let list = T::tables_mut(self.tables);
        let file_error = |problem| DesignError::file(&file.path, problem);
        let mut matching = list
            .iter_mut()
            .flat_map(|table| table.rows.iter_mut())
            .filter(|row| row.key() == file.id)
            .peekable();
        match file.action {
            Action::Override => {
                if matching.peek().is_none() {
                    return Err(file_error(Problem::UnknownId));
                }
                let changed = matching
                    .map(|row| merged(row, file).map(|new| (row, new)))
                    .collect::<Result<Vec<_>, _>>()?;
                for (row, new) in changed {
                    *row = new;
                }
                Ok(())
            }
            Action::Disable => {
                if matching.peek().is_none() {
                    return Err(file_error(Problem::UnknownId));
                }
                for table in list.iter_mut() {
                    table.rows.retain(|row| row.key() != file.id);
                }
                Ok(())
            }
            Action::Add => {
                if matching.peek().is_some() {
                    return Err(file_error(Problem::IdExists));
                }
                let mut row = match &file.base {
                    Some(base) => {
                        let base_row = list
                            .iter()
                            .flat_map(|table| &table.rows)
                            .find(|row| row.key() == base)
                            .ok_or_else(|| file_error(Problem::UnknownBase))?;
                        merged(base_row, file)?
                    }
                    None => {
                        let mut full = file.fields.clone();
                        full.insert(
                            T::KEY_FIELD.to_owned(),
                            toml::Value::String(file.id.clone()),
                        );
                        row_from_toml::<T>(full)
                            .map_err(|error| file_error(Problem::IncompleteRow(error)))?
                    }
                };
                row.set_key(file.id.clone());
                design_table(list, file).rows.push(row);
                Ok(())
            }
        }
    }
}

/// `row` with the fields of `file` set. A field whose value does not fit gives `WrongType`.
fn merged<T: Row>(row: &T, file: &DesignFile) -> Result<T, DesignError> {
    let mut json = serde_json::to_value(row)
        .map_err(|error| DesignError::file(&file.path, Problem::WrongType(error.to_string())))?;
    for (name, value) in &file.fields {
        let wrong_type = |error: serde_json::Error| {
            DesignError::field(&file.path, name, Problem::WrongType(error.to_string()))
        };
        let value = serde_json::to_value(value).map_err(wrong_type)?;
        if let Some(object) = json.as_object_mut() {
            object.insert(name.clone(), value);
        }
        serde_json::from_value::<T>(json.clone()).map_err(wrong_type)?;
    }
    serde_json::from_value(json)
        .map_err(|error| DesignError::file(&file.path, Problem::WrongType(error.to_string())))
}

/// The table of `list` that holds the rows added by files of `file`'s kind; made when missing.
fn design_table<'a, T>(list: &'a mut Vec<Table<T>>, file: &DesignFile) -> &'a mut Table<T> {
    let source_path = format!("{DESIGN_SOURCE_PREFIX}{}", file.kind.folder());
    let index = match list
        .iter()
        .position(|table| table.source_path == source_path)
    {
        Some(index) => index,
        None => {
            list.push(Table {
                source_path,
                unique_id: DESIGN_UNIQUE_ID.to_owned(),
                rows: Vec::new(),
            });
            list.len() - 1
        }
    };
    &mut list[index]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use nw_content::generated::LootTablesData;
    use nw_content::table::Table;

    use super::*;
    use crate::error::Problem;
    use crate::file::{Action, DesignFile, Kind};

    fn loot_row(id: &str, max_roll: Option<i64>) -> LootTablesData {
        LootTablesData {
            and_or: Some("OR".to_owned()),
            chance_to_exceed_index: None,
            condition_overrides_roll: None,
            conditions: None,
            gs_bonus: None,
            gear_score_range: BTreeMap::new(),
            hwm_mult: None,
            ignore_hwmgs: None,
            item: BTreeMap::from([(1, "Item_A".to_owned()), (2, "Item_B".to_owned())]),
            loot_biasing_chance_override: None,
            loot_biasing_enabled: None,
            loot_table_id: id.to_owned(),
            max_roll,
            perk_bucket_overrides1: None,
            perk_bucket_overrides2: None,
            perk_overrides: BTreeMap::new(),
            roll_bonus_setting: None,
            trigger_limit_on_visit: None,
            use_level_gs: None,
        }
    }

    fn tables() -> Tables {
        let mut tables = Tables::default();
        tables.loot_tables_data.push(Table {
            source_path: "datatables/loot.datasheet".to_owned(),
            unique_id: "Loot".to_owned(),
            rows: vec![loot_row("A", Some(10)), loot_row("B", None)],
        });
        tables
    }

    fn file(action: Action, id: &str, base: Option<&str>, fields: &str) -> DesignSet {
        DesignSet {
            files: vec![DesignFile {
                path: PathBuf::from(format!("loot/tables/{id}.toml")),
                kind: Kind::LootTable,
                action,
                id: id.to_owned(),
                base: base.map(str::to_owned),
                map: None,
                fields: fields.parse().unwrap(),
            }],
        }
    }

    fn rows(tables: &Tables) -> Vec<&LootTablesData> {
        tables
            .loot_tables_data
            .iter()
            .flat_map(|table| &table.rows)
            .collect()
    }

    #[test]
    fn override_changes_only_named_fields() {
        let mut tables = tables();
        let errors = apply_tables(
            &file(Action::Override, "A", None, "max_roll = 50"),
            &mut tables,
        );
        assert_eq!(errors, []);
        let rows = rows(&tables);
        assert_eq!(rows[0].max_roll, Some(50));
        assert_eq!(rows[0].and_or.as_deref(), Some("OR"));
        assert_eq!(rows[1], &loot_row("B", None));
    }

    #[test]
    fn override_of_a_missing_id_is_an_error() {
        let mut tables = tables();
        let errors = apply_tables(
            &file(Action::Override, "Z", None, "max_roll = 5"),
            &mut tables,
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].problem, Problem::UnknownId);
    }

    #[test]
    fn disable_removes_the_row() {
        let mut tables = tables();
        assert_eq!(
            apply_tables(&file(Action::Disable, "A", None, ""), &mut tables),
            []
        );
        let ids: Vec<_> = rows(&tables)
            .iter()
            .map(|row| row.loot_table_id.as_str())
            .collect();
        assert_eq!(ids, ["B"]);
    }

    #[test]
    fn disable_of_a_missing_id_is_an_error() {
        let mut tables = tables();
        let errors = apply_tables(&file(Action::Disable, "Z", None, ""), &mut tables);
        assert_eq!(errors[0].problem, Problem::UnknownId);
    }

    #[test]
    fn add_with_base_copies_then_changes() {
        let mut tables = tables();
        assert_eq!(
            apply_tables(
                &file(Action::Add, "C", Some("A"), "max_roll = 7"),
                &mut tables
            ),
            []
        );
        let design = tables
            .loot_tables_data
            .iter()
            .find(|table| table.source_path == "design/loot/tables")
            .expect("a design table");
        assert_eq!(design.rows.len(), 1);
        let mut expected = loot_row("C", Some(7));
        expected.and_or = Some("OR".to_owned());
        assert_eq!(design.rows[0], expected);
    }

    #[test]
    fn add_without_base_reads_the_full_row() {
        let mut tables = tables();
        let full_row = "max_roll = 3\ngear_score_range = {}\nitem = {}\nperk_overrides = {}";
        let errors = apply_tables(&file(Action::Add, "D", None, full_row), &mut tables);
        assert_eq!(errors, []);
        assert!(
            rows(&tables)
                .iter()
                .any(|row| row.loot_table_id == "D" && row.max_roll == Some(3))
        );
    }

    #[test]
    fn add_of_an_existing_id_is_an_error() {
        let mut tables = tables();
        let errors = apply_tables(&file(Action::Add, "A", Some("B"), ""), &mut tables);
        assert_eq!(errors[0].problem, Problem::IdExists);
    }

    #[test]
    fn add_with_a_missing_base_is_an_error() {
        let mut tables = tables();
        let errors = apply_tables(&file(Action::Add, "C", Some("Z"), ""), &mut tables);
        assert_eq!(errors[0].problem, Problem::UnknownBase);
    }

    #[test]
    fn a_wrong_type_is_named() {
        let mut tables = tables();
        let errors = apply_tables(
            &file(
                Action::Override,
                "A",
                None,
                "and_or = \"AND\"\nmax_roll = \"many\"",
            ),
            &mut tables,
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field.as_deref(), Some("max_roll"));
        assert!(matches!(errors[0].problem, Problem::WrongType(_)));
        assert_eq!(rows(&tables)[0], &loot_row("A", Some(10)));
    }

    #[test]
    fn numbered_columns_are_replaced_whole() {
        let mut tables = tables();
        let errors = apply_tables(
            &file(Action::Override, "A", None, "item = { 1 = \"X\" }"),
            &mut tables,
        );
        assert_eq!(errors, []);
        assert_eq!(rows(&tables)[0].item, BTreeMap::from([(1, "X".to_owned())]));
    }
}
