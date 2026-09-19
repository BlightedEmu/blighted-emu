// SPDX-License-Identifier: AGPL-3.0-or-later
//! The design file model: kinds, actions, one file, and the set of all files.

use std::fmt;
use std::path::PathBuf;

use nw_content::Tables;
use nw_content::generated::{
    ArmorItemDefinitions, ConsumableItemDefinitions, LootTablesData, MasterItemDefinitions,
    NPCData, ObjectiveTasks, Objectives, PerkData, VitalsData, WeaponItemDefinitions,
};
use nw_content::table::{FromDatasheet, Table};
use serde::de::{self, DeserializeOwned, Visitor};
use serde::{Deserialize, Serialize};

/// What a design file changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A `MasterItemDefinitions` row.
    Item,
    /// A `WeaponItemDefinitions` row.
    Weapon,
    /// An `ArmorItemDefinitions` row.
    Armor,
    /// A `ConsumableItemDefinitions` row.
    Consumable,
    /// A `PerkData` row.
    Perk,
    /// A `LootTablesData` row.
    LootTable,
    /// A `VitalsData` row (a mob type, with its loot table).
    Mob,
    /// An `NPCData` row.
    Npc,
    /// An `Objectives` row.
    Objective,
    /// An `ObjectiveTasks` row.
    ObjectiveTask,
    /// A retail creature spawn point of a world file.
    SpawnPoint,
    /// A retail NPC point of a world file.
    NpcPoint,
    /// An AI profile: a behaviour and brain numbers for spawns (spec section 6.5).
    AiProfile,
    /// A patrol path of one level (spec section 6.4).
    Path,
    /// New mob spawns of one level, with their profile and path (spec section 7.1).
    SpawnGroup,
}

/// Every kind with its name in files and its folder under the design folder. Point kinds add
/// `/<map>` to the folder.
const KINDS: [(Kind, &str, &str); 15] = [
    (Kind::Item, "item", "items"),
    (Kind::Weapon, "weapon", "items/weapons"),
    (Kind::Armor, "armor", "items/armor"),
    (Kind::Consumable, "consumable", "items/consumables"),
    (Kind::Perk, "perk", "perks"),
    (Kind::LootTable, "loot_table", "loot/tables"),
    (Kind::Mob, "mob", "mobs"),
    (Kind::Npc, "npc", "npcs/types"),
    (Kind::Objective, "objective", "quests"),
    (Kind::ObjectiveTask, "objective_task", "quests/tasks"),
    (Kind::SpawnPoint, "spawn_point", "spawns"),
    (Kind::NpcPoint, "npc_point", "npcs"),
    (Kind::AiProfile, "ai_profile", "ai"),
    (Kind::Path, "path", "paths"),
    (Kind::SpawnGroup, "spawn_group", "spawns"),
];

impl Kind {
    /// The kind of a file's `kind` value.
    pub fn from_name(name: &str) -> Option<Self> {
        KINDS
            .iter()
            .find(|(_, known, _)| *known == name)
            .map(|(kind, _, _)| *kind)
    }

    /// The name used in files.
    pub fn name(self) -> &'static str {
        Self::entry(self).1
    }

    /// The folder of this kind under the design folder; point kinds add `/<map>`.
    pub fn folder(self) -> &'static str {
        Self::entry(self).2
    }

    /// True for the retail world file point kinds.
    pub fn is_point(self) -> bool {
        matches!(self, Self::SpawnPoint | Self::NpcPoint)
    }

    /// True for the kinds that the client data does not have: only design files make them.
    pub fn is_own(self) -> bool {
        matches!(self, Self::AiProfile | Self::Path | Self::SpawnGroup)
    }

    /// True for the kinds of one level: their files name it with `map` and lie in
    /// `<folder>/<map>/`.
    pub fn needs_map(self) -> bool {
        self.is_point() || matches!(self, Self::Path | Self::SpawnGroup)
    }

    /// True when files of this kind may use `action`. Retail points can only be changed or
    /// disabled (new spawns come with spawn groups); own kinds can only be added.
    pub fn allows(self, action: Action) -> bool {
        if self.is_own() {
            action == Action::Add
        } else {
            !self.is_point() || action != Action::Add
        }
    }

    fn entry(self) -> &'static (Kind, &'static str, &'static str) {
        KINDS
            .iter()
            .find(|(kind, _, _)| *kind == self)
            .expect("KINDS lists every kind")
    }

    /// Calls `visitor` with the row type of a table kind; `None` for point kinds.
    pub(crate) fn visit_row<V: RowVisitor>(self, visitor: V) -> Option<V::Output> {
        Some(match self {
            Self::Item => visitor.visit::<MasterItemDefinitions>(),
            Self::Weapon => visitor.visit::<WeaponItemDefinitions>(),
            Self::Armor => visitor.visit::<ArmorItemDefinitions>(),
            Self::Consumable => visitor.visit::<ConsumableItemDefinitions>(),
            Self::Perk => visitor.visit::<PerkData>(),
            Self::LootTable => visitor.visit::<LootTablesData>(),
            Self::Mob => visitor.visit::<VitalsData>(),
            Self::Npc => visitor.visit::<NPCData>(),
            Self::Objective => visitor.visit::<Objectives>(),
            Self::ObjectiveTask => visitor.visit::<ObjectiveTasks>(),
            Self::SpawnPoint | Self::NpcPoint | Self::AiProfile | Self::Path | Self::SpawnGroup => {
                return None;
            }
        })
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// What a design file does to its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Replaces the named fields of the client record.
    Override,
    /// Makes a new record.
    Add,
    /// Removes the client record.
    Disable,
}

impl Action {
    /// The action of a file's `action` value.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "override" => Some(Self::Override),
            "add" => Some(Self::Add),
            "disable" => Some(Self::Disable),
            _ => None,
        }
    }
}

/// One loaded design file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesignFile {
    /// The file, relative to the design folder, with `/` separators.
    pub path: PathBuf,
    /// What it changes.
    pub kind: Kind,
    /// What it does.
    pub action: Action,
    /// The record id (the point key for point kinds).
    pub id: String,
    /// The client record an `add` starts from.
    pub base: Option<String>,
    /// The level of a point kind.
    pub map: Option<String>,
    /// Every key except the header keys.
    pub fields: toml::Table,
}

/// Every design file, sorted by kind, then id, then path.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DesignSet {
    /// The files.
    pub files: Vec<DesignFile>,
}

/// A client row type that design files can change.
pub(crate) trait Row: FromDatasheet + Serialize + DeserializeOwned + Clone {
    /// The serde name of the id field.
    const KEY_FIELD: &'static str;
    /// The id of the row.
    fn key(&self) -> &str;
    /// Sets the id of the row.
    fn set_key(&mut self, key: String);
    /// The table list of this type in `tables`.
    fn tables_mut(tables: &mut Tables) -> &mut Vec<Table<Self>>;
    /// The table list of this type in `tables`, to read.
    fn tables(tables: &Tables) -> &Vec<Table<Self>>;
}

macro_rules! row {
    ($ty:ty, $key:ident, $list:ident) => {
        impl Row for $ty {
            const KEY_FIELD: &'static str = stringify!($key);
            fn key(&self) -> &str {
                &self.$key
            }
            fn set_key(&mut self, key: String) {
                self.$key = key;
            }
            fn tables_mut(tables: &mut Tables) -> &mut Vec<Table<Self>> {
                &mut tables.$list
            }
            fn tables(tables: &Tables) -> &Vec<Table<Self>> {
                &tables.$list
            }
        }
    };
}

row!(MasterItemDefinitions, item_id, master_item_definitions);
row!(WeaponItemDefinitions, weapon_id, weapon_item_definitions);
row!(ArmorItemDefinitions, weapon_id, armor_item_definitions);
row!(
    ConsumableItemDefinitions,
    consumable_id,
    consumable_item_definitions
);
row!(PerkData, perk_id, perk_data);
row!(LootTablesData, loot_table_id, loot_tables_data);
row!(VitalsData, vitals_id, vitals_data);
row!(NPCData, npc_id, npc_data);
row!(Objectives, objective_id, objectives);
row!(ObjectiveTasks, task_id, objective_tasks);

/// Work done with the row type of a kind ([`Kind::visit_row`]).
pub(crate) trait RowVisitor {
    /// The result.
    type Output;
    /// Runs with row type `T`.
    fn visit<T: Row>(self) -> Self::Output;
}

/// The serde field names of struct `T`, in struct order; empty when `T` is not a struct.
pub fn struct_fields<T: DeserializeOwned>() -> &'static [&'static str] {
    match T::deserialize(FieldGrab) {
        Err(FieldGrabError::Fields(fields)) => fields,
        _ => &[],
    }
}

/// A deserializer that only asks a struct for its field list.
struct FieldGrab;

#[derive(Debug)]
enum FieldGrabError {
    Fields(&'static [&'static str]),
    NotAStruct,
}

impl fmt::Display for FieldGrabError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("field list read")
    }
}

impl std::error::Error for FieldGrabError {}

impl de::Error for FieldGrabError {
    fn custom<T: fmt::Display>(_: T) -> Self {
        Self::NotAStruct
    }
}

impl<'de> de::Deserializer<'de> for FieldGrab {
    type Error = FieldGrabError;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
        Err(FieldGrabError::NotAStruct)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Self::Error> {
        Err(FieldGrabError::Fields(fields))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier
        ignored_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_fields_lists_the_serde_names_in_order() {
        let fields = struct_fields::<LootTablesData>();
        assert_eq!(fields.first(), Some(&"and_or"));
        assert!(fields.contains(&"loot_table_id"));
        assert!(struct_fields::<ObjectiveTasks>().contains(&"type"));
    }

    #[test]
    fn a_non_struct_has_no_fields() {
        assert!(struct_fields::<u32>().is_empty());
    }

    #[test]
    fn every_kind_has_a_name_that_reads_back() {
        for (kind, name, _) in KINDS {
            assert_eq!(Kind::from_name(name), Some(kind));
            assert_eq!(kind.name(), name);
        }
    }

    #[test]
    fn points_cannot_be_added() {
        assert!(!Kind::SpawnPoint.allows(Action::Add));
        assert!(Kind::SpawnPoint.allows(Action::Disable));
        assert!(Kind::LootTable.allows(Action::Add));
    }
}
