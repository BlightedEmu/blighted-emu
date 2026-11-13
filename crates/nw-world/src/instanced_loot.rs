// SPDX-License-Identifier: AGPL-3.0-or-later
//! Whether the gathers of a level object are instanced, their loot type and the range of their
//! tracker entries: its slice holds a
//! gatherable controller whose instanced loot entity (`m_instancedLootEntityRef`, offset 0x200 of
//! the client's `GatherableControllerComponent` reflection `FUN_143379560`) names an entity, and
//! that entity's instanced loot component holds the type (`m_instancedLootType`, reflection
//! `FUN_1436fdd10`; the client enum `InstancedLootType`, `FUN_1436fe850`: `POILoot` 0, `AILoot`
//! 1, `Gathering` 2, `Collectible` 3, `Quest` 4, `Discarded` 5, `GameMode` 6).
//!
//! Observed (finding 2026-11-13 "gatherer counts and group gathers", 3,379 retail gathers of
//! open-world capitals and spawned slices whose request came at most 0.5 s before the gather's
//! start): each of the 2,584 gathers of an object whose slice names an instanced loot entity
//! counted 0 gatherers in the gatherer's own gather state (`m_numGathering`), 2,443 of them with
//! an entry for the object in the gatherer's own loot tracker; each of the 795 gathers of an object
//! whose slice names none counted 1, none with a tracker entry. `master_lootcontainer` and
//! `master_quest_instanced` name one; `master_lootcontainer_pvp`, `master_quest` and the Brimstone
//! glyph lock `16bs_io_ancientglyphlock_sutekh` name none.
//!
//! Observed (finding D8 "per-player gather lock", 2,122 tracker entries of retail gathers): the
//! entry's `m_lootType` is the slice's type: 0 in every entry of a `master_lootcontainer` chest
//! (842), 4 in every entry of a `master_quest_instanced` object (1,248, loot containers among
//! them), 2 for the Edengrove Azoth sapling.
//!
//! Observed (finding D9 "tracker entries out of range"): the entry leaves the looter's list beyond
//! the slice's `m_aoiRadius` (`Javelin::AoiComponent`, reflection `FUN_14175f1a0`): 80 m in
//! `master_lootcontainer`, 100 m in `master_quest_instanced`. Each of the 126 gathered slices
//! dumped (`local-data/scratch-work/d7-gather/trees/gathered`) holds one `m_aoiRadius`; in these
//! two slices `m_sliceSpawnRadius` is the same, and no retail removal of a slice where the two
//! differ (the vistas: 200 m and 1,100 m) was captured, so which of the two the server measures
//! is Inferred from the field name.

use nw_content::gathering::InstancedObject;
use nw_formats::objectstream::{Element, lowercase_crc32};
use serde::{Deserialize, Serialize};

use crate::interact_tags::TagOwner;
use crate::level_script::NO_ENTITY;

/// The gatherable controller's reference to its instanced loot entity.
const INSTANCED_LOOT_FIELD: &str = "m_instancedLootEntityRef";
/// The entity id holder of an entity reference.
const ENTITY_ID_FIELD: &str = "EntityId";
/// The id of an entity id holder.
const ID_FIELD: &str = "Id";
/// The instanced loot component's loot type.
const LOOT_TYPE_FIELD: &str = "m_instancedLootType";
/// The area of interest component's radius, m.
const AOI_RADIUS_FIELD: &str = "m_aoiRadius";
/// `InstancedLootType::POILoot`, the type of a slice that names an instanced loot entity but
/// holds no type field. Inferred: the enum's first value; every instanced slice dump read
/// (`master_lootcontainer`, `master_quest_instanced`) holds the field.
pub const POI_LOOT: u32 = 0;

/// A level object whose gathers are instanced, with the values of its slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstancedGather {
    /// The object.
    pub owner: TagOwner,
    /// Its slice's loot type and area of interest radius.
    pub object: InstancedObject,
}

/// The values of the slice `root` when it holds a gatherable controller whose instanced loot
/// entity names an entity (not [`NO_ENTITY`]): its first `m_instancedLootType`, else
/// [`POI_LOOT`], and its first `m_aoiRadius`; `None` for a slice whose gathers are shared.
pub fn instanced_object(root: &Element) -> Option<InstancedObject> {
    let names = (
        lowercase_crc32(INSTANCED_LOOT_FIELD),
        lowercase_crc32(ENTITY_ID_FIELD),
        lowercase_crc32(ID_FIELD),
    );
    holds(root, names).then(|| InstancedObject {
        loot_type: first(root, lowercase_crc32(LOOT_TYPE_FIELD))
            .and_then(|field| field.as_u32().ok())
            .unwrap_or(POI_LOOT),
        aoi_radius_m: first(root, lowercase_crc32(AOI_RADIUS_FIELD))
            .and_then(|field| field.as_f32().ok()),
    })
}

fn holds(element: &Element, names: (u32, u32, u32)) -> bool {
    let (field, entity_id, id) = names;
    let names_entity = element.name_crc == Some(field)
        && element
            .child(entity_id)
            .and_then(|holder| holder.child(id))
            .and_then(|id| id.as_u64().ok())
            .is_some_and(|id| id != NO_ENTITY);
    names_entity
        || element
            .children
            .iter()
            .any(|child| holds(child, names))
}

/// The first element named `field` of `element` and its children, in stream order.
fn first(element: &Element, field: u32) -> Option<&Element> {
    if element.name_crc == Some(field) {
        return Some(element);
    }
    element
        .children
        .iter()
        .find_map(|child| first(child, field))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entity of build 25810808's `master_lootcontainer` (its loot entity).
    const LOOT_ENTITY: u64 = 0x0001_34d9_0963_dc55;
    /// `InstancedLootType::Quest`, the type of `master_quest_instanced`.
    const QUEST_LOOT: u32 = 4;
    /// The `m_aoiRadius` of `master_quest_instanced`, m.
    const QUEST_AOI_RADIUS_M: f32 = 100.0;

    fn controller(loot_entity: u64) -> Element {
        let id = Element::with_value(lowercase_crc32(ID_FIELD), loot_entity.to_be_bytes().to_vec());
        let holder = Element::with_children(lowercase_crc32(ENTITY_ID_FIELD), vec![id]);
        let reference = Element::with_children(lowercase_crc32(INSTANCED_LOOT_FIELD), vec![holder]);
        Element::with_children(lowercase_crc32("m_serverFacetPtr"), vec![reference])
    }

    fn loot_component(loot_type: u32) -> Element {
        let field = Element::with_value(
            lowercase_crc32(LOOT_TYPE_FIELD),
            loot_type.to_be_bytes().to_vec(),
        );
        Element::with_children(lowercase_crc32("m_serverFacetPtr"), vec![field])
    }

    fn aoi_component(radius_m: f32) -> Element {
        let field = Element::with_value(
            lowercase_crc32(AOI_RADIUS_FIELD),
            radius_m.to_be_bytes().to_vec(),
        );
        Element::with_children(lowercase_crc32("m_serverFacetPtr"), vec![field])
    }

    fn slice(components: Vec<Element>) -> Element {
        Element::with_children(lowercase_crc32("Components"), components)
    }

    #[test]
    fn a_slice_is_instanced_when_a_controller_names_its_loot_entity() {
        let quest = slice(vec![
            controller(LOOT_ENTITY),
            loot_component(QUEST_LOOT),
            aoi_component(QUEST_AOI_RADIUS_M),
        ]);
        assert_eq!(
            instanced_object(&quest),
            Some(InstancedObject {
                loot_type: QUEST_LOOT,
                aoi_radius_m: Some(QUEST_AOI_RADIUS_M),
            }),
            "its components' type and radius"
        );
        assert_eq!(
            instanced_object(&slice(vec![
                controller(NO_ENTITY),
                controller(LOOT_ENTITY),
                loot_component(POI_LOOT),
            ]))
            .map(|object| object.loot_type),
            Some(POI_LOOT),
            "any controller of the slice"
        );
        assert_eq!(
            instanced_object(&slice(vec![controller(LOOT_ENTITY)])),
            Some(InstancedObject {
                loot_type: POI_LOOT,
                aoi_radius_m: None,
            }),
            "no type or radius field"
        );
    }

    #[test]
    fn a_slice_whose_controllers_name_no_loot_entity_is_shared() {
        assert_eq!(
            instanced_object(&slice(vec![
                controller(NO_ENTITY),
                loot_component(QUEST_LOOT),
                aoi_component(QUEST_AOI_RADIUS_M),
            ])),
            None
        );
        assert_eq!(instanced_object(&slice(Vec::new())), None, "no controller");
    }
}
