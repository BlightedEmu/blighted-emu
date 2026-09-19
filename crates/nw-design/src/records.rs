// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client records of the table kinds, as the field maps a design file names (admin studio
//! spec section 7): the editors show them next to the design values.

use nw_content::Tables;
use serde_json::{Map, Value};

use crate::file::{Kind, Row, RowVisitor, struct_fields};

/// One client record: its id and every field by its design file name.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientRecord {
    /// The record id.
    pub id: String,
    /// Every field except the id.
    pub fields: Map<String, Value>,
}

/// The records of `kind` in `tables`, in table order; `None` for a kind with no client table.
pub fn client_records(tables: &Tables, kind: Kind) -> Option<Vec<ClientRecord>> {
    kind.visit_row(ReadRecords { tables })
}

/// The field names of `kind`'s records, in row order without the id; empty for a kind with no
/// client table.
pub fn record_field_names(kind: Kind) -> Vec<&'static str> {
    kind.visit_row(FieldNames).unwrap_or_default()
}

struct ReadRecords<'a> {
    tables: &'a Tables,
}

impl RowVisitor for ReadRecords<'_> {
    type Output = Vec<ClientRecord>;

    fn visit<T: Row>(self) -> Vec<ClientRecord> {
        T::tables(self.tables)
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let Ok(Value::Object(mut fields)) = serde_json::to_value(row) else {
                    return None;
                };
                fields.remove(T::KEY_FIELD);
                Some(ClientRecord {
                    id: row.key().to_owned(),
                    fields,
                })
            })
            .collect()
    }
}

struct FieldNames;

impl RowVisitor for FieldNames {
    type Output = Vec<&'static str>;

    fn visit<T: Row>(self) -> Vec<&'static str> {
        struct_fields::<T>()
            .iter()
            .copied()
            .filter(|name| *name != T::KEY_FIELD)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nw_content::generated::{LootTablesData, MasterItemDefinitions};
    use nw_content::table::Table;

    use super::*;

    const ITEM: &str = "1hSwordT2";
    const TIER: i64 = 2;

    fn tables() -> Tables {
        let mut tables = Tables::default();
        let item: MasterItemDefinitions = serde_json::from_value(serde_json::json!({
            "item_id": ITEM,
            "tier": TIER,
        }))
        .expect("row");
        tables.master_item_definitions.push(Table {
            source_path: "items.datasheet".to_owned(),
            unique_id: "items".to_owned(),
            rows: vec![item],
        });
        tables
    }

    #[test]
    fn records_carry_their_fields_without_the_id() {
        let records = client_records(&tables(), Kind::Item).expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, ITEM);
        assert_eq!(records[0].fields["tier"], TIER);
        assert!(!records[0].fields.contains_key("item_id"));
        assert_eq!(client_records(&tables(), Kind::Path), None);
    }

    #[test]
    fn field_names_follow_the_row_type() {
        let names = record_field_names(Kind::Item);
        assert!(names.contains(&"tier") && names.contains(&"gear_score_override"));
        assert!(!names.contains(&"item_id"));
        assert!(
            record_field_names(Kind::LootTable).len()
                >= struct_fields::<LootTablesData>().len() - 1
        );
        assert!(record_field_names(Kind::SpawnPoint).is_empty());
    }
}
