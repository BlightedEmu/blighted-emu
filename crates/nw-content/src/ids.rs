// SPDX-License-Identifier: AGPL-3.0-or-later
//! The datasheet ids that world file records name, by record kind (M1b spec section 8). A world
//! file stores the crc32 of the lower-case id.

use crate::Tables;

/// The ids of `VitalsData`, `VitalsBaseData` and `VitalsLevelVariantData`.
pub fn vitals_ids(tables: &Tables) -> impl Iterator<Item = &str> {
    let data = tables.vitals_data.iter().flat_map(|table| &table.rows);
    let base = tables.vitals_base_data.iter().flat_map(|table| &table.rows);
    let variant = tables
        .vitals_level_variant_data
        .iter()
        .flat_map(|table| &table.rows);
    data.map(|row| row.vitals_id.as_str())
        .chain(base.map(|row| row.vitals_id.as_str()))
        .chain(variant.map(|row| row.vitals_id.as_str()))
}

/// The ids of `GatherableData`.
pub fn gatherable_ids(tables: &Tables) -> impl Iterator<Item = &str> {
    tables
        .gatherable_data
        .iter()
        .flat_map(|table| &table.rows)
        .map(|row| row.gatherable_id.as_str())
}

/// The ids of `NPCData`.
pub fn npc_ids(tables: &Tables) -> impl Iterator<Item = &str> {
    tables
        .npc_data
        .iter()
        .flat_map(|table| &table.rows)
        .map(|row| row.npc_id.as_str())
}

/// The ids of `LoreData`.
pub fn lore_ids(tables: &Tables) -> impl Iterator<Item = &str> {
    tables
        .lore_data
        .iter()
        .flat_map(|table| &table.rows)
        .map(|row| row.lore_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tables_have_no_ids() {
        let tables = Tables::default();
        assert_eq!(vitals_ids(&tables).count(), 0);
        assert_eq!(gatherable_ids(&tables).count(), 0);
        assert_eq!(npc_ids(&tables).count(), 0);
        assert_eq!(lore_ids(&tables).count(), 0);
    }
}
