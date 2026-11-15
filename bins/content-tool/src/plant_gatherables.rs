// SPDX-License-Identifier: AGPL-3.0-or-later
//! The gatherables among the plants a level's regions place (`region.distribution`,
//! [`crate::distribution`]): the trees, bushes and stones a player chops, picks and mines.
//!
//! Observed (live 2026-11-15): every bush near a player in Monarch's Bluffs was refused as out of
//! reach, the region metadata holds no point there, and the region's distribution places 37
//! bushes, 2 boulders and about 20 trees within 25 m of the player; the open world's
//! distributions place 2,479,886 rows of slices under `gatherables/` (build 25810808).
//!
//! A row's gatherable is its variant's: the slice's `VariationDataComponent` names its variation
//! table (`Gatherable_Bushes`), whose row of the variant holds the `GatherableEntryID`
//! (`Bush_SupPawPaw_b` gives `BushMedium`, the kind the client named in its gather). A row with
//! no variant, or whose variant has no row there, has the slice's own `m_gatherableEntryId`
//! (`gatherables/master_bush`: `BushLarge`). A slice with neither places no gatherable. The point
//! stands on the terrain (Inferred for the rows on water: the gather reach counts across the
//! ground).

use std::collections::{BTreeSet, HashMap};

use nw_content::Tables;
use nw_formats::objectstream::{Element, lowercase_crc32};
use nw_world::WorldFile;
use nw_world::level_script::GATHERABLE_CONTROLLER_TYPE;
use nw_world::records::GatherablePoint;

use crate::collision::{VARIATION_TABLE_FIELD, types::VARIATION_DATA};
use crate::distribution::{Distribution, region_origin};
use crate::slice_meta::slice_key;

/// `m_gatherableEntryId`: the field of a `GatherableControllerComponent` that names its
/// gatherable.
const GATHERABLE_ENTRY_FIELD: u32 = 0x2d69_6003;

/// What a plant slice gives: its own gatherable and its variation table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlantSlice {
    /// `m_gatherableEntryId` of its gatherable controller.
    pub gatherable: Option<String>,
    /// The variation table its `VariationDataComponent` names.
    pub table: Option<String>,
}

impl PlantSlice {
    /// The gatherable controller and variation table of the slice whose ObjectStream is `root`.
    pub fn of(root: &Element) -> Self {
        let mut slice = Self::default();
        let mut pending = vec![root];
        while let Some(element) = pending.pop() {
            let text = |field: u32| {
                element
                    .child(field)
                    .and_then(|value| value.as_string().ok())
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            };
            if element.type_id == GATHERABLE_CONTROLLER_TYPE && slice.gatherable.is_none() {
                slice.gatherable = text(GATHERABLE_ENTRY_FIELD);
            }
            if element.type_id == VARIATION_DATA && slice.table.is_none() {
                slice.table = text(VARIATION_TABLE_FIELD);
            }
            pending.extend(element.children.iter().rev());
        }
        slice
    }
}

/// The gatherables of the plant slices and of their variants.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlantGatherables {
    /// The plant slices, by slice key ([`slice_key`]).
    slices: HashMap<String, PlantSlice>,
    /// The `GatherableEntryID` of each variation row, by lowercase table id and variant id.
    variants: HashMap<(String, String), String>,
}

/// The variation rows of `tables` that name a gatherable: table id, variant id and
/// `GatherableEntryID`.
pub fn variation_gatherables(tables: &Tables) -> impl Iterator<Item = (&str, &str, &str)> {
    tables.variation_data.iter().flat_map(|table| {
        table.rows.iter().filter_map(|row| {
            let entry = row
                .gatherable_entry_id
                .as_deref()
                .or(row.gatherable_entry_id_2.as_deref())
                .filter(|entry| !entry.is_empty())?;
            Some((table.unique_id.as_str(), row.variant_id.as_deref()?, entry))
        })
    })
}

impl PlantGatherables {
    /// The plant `slices` (by slice key) with the variation `rows` (table id, variant id,
    /// gatherable; [`variation_gatherables`]).
    pub fn build<'a>(
        slices: HashMap<String, PlantSlice>,
        rows: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
    ) -> Self {
        let variants = rows
            .into_iter()
            .map(|(table, variant, entry)| {
                ((table.to_lowercase(), variant.to_lowercase()), entry.to_owned())
            })
            .collect();
        Self { slices, variants }
    }

    /// The gatherable of a plant of slice `slice` and variant `variant` (empty: none).
    pub fn gatherable(&self, slice: &str, variant: &str) -> Option<&str> {
        let plant = self.slices.get(&slice_key(slice))?;
        plant
            .table
            .as_ref()
            .filter(|_| !variant.is_empty())
            .and_then(|table| {
                self.variants
                    .get(&(table.to_lowercase(), variant.to_lowercase()))
            })
            .or(plant.gatherable.as_ref())
            .map(String::as_str)
    }
}

/// The slice keys of the plants of `distributions`.
pub fn plant_slice_keys<'a>(distributions: impl IntoIterator<Item = &'a Distribution>) -> BTreeSet<String> {
    distributions
        .into_iter()
        .flat_map(|distribution| distribution.slices.iter().map(|slice| slice_key(slice)))
        .collect()
}

/// The gatherable points of the plants that `distribution` (of region folder `region`) places
/// on the terrain of `world`; a row off the terrain is left out.
pub fn plant_points(
    region: &str,
    distribution: &Distribution,
    world: &WorldFile,
    plants: &PlantGatherables,
) -> Vec<GatherablePoint> {
    let Some(origin) = region_origin(region) else {
        return Vec::new();
    };
    let kinds: Vec<Option<u32>> = distribution
        .slices
        .iter()
        .zip(&distribution.variants)
        .map(|(slice, variant)| plants.gatherable(slice, variant).map(lowercase_crc32))
        .collect();
    distribution
        .rows
        .iter()
        .filter_map(|row| {
            let gatherable_id = kinds.get(usize::from(row.slice)).copied().flatten()?;
            let [x, y] = [origin[0] + row.at[0], origin[1] + row.at[1]];
            Some(GatherablePoint {
                gatherable_id,
                position: [x, y, world.height_at(x, y)?],
                ..GatherablePoint::default()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use nw_world::records::{HeightGrid, LevelInfo, Region};

    use super::*;
    use crate::distribution::{HeightSource, Row};

    const BUSH: &str = "gatherables/master_bush";
    const BEECH: &str = "gatherables/beechtree_b_all";
    const ROCK: &str = "props/rock_a";
    const BUSH_TABLE: &str = "Gatherable_Bushes";
    /// `Gatherable_Bushes` `Bush_SupPawPaw_b` (content bundle 25810808).
    const PAWPAW: (&str, &str, &str) = (BUSH_TABLE, "Bush_SupPawPaw_b", "BushMedium");
    const REGION_SIDE: u32 = 2048;
    /// A flat height grid's raw sample.
    const FLAT_SAMPLE: u16 = 4_000;

    fn plants() -> PlantGatherables {
        let slices = HashMap::from([
            (
                slice_key(BUSH),
                PlantSlice {
                    gatherable: Some("BushLarge".to_owned()),
                    table: Some(BUSH_TABLE.to_owned()),
                },
            ),
            (
                slice_key(BEECH),
                PlantSlice {
                    gatherable: Some("TreeHardwoodMedium".to_owned()),
                    table: None,
                },
            ),
        ]);
        PlantGatherables::build(slices, [PAWPAW])
    }

    #[test]
    fn a_plant_s_gatherable_is_its_variant_s_or_else_its_slice_s() {
        let plants = plants();
        assert_eq!(plants.gatherable(BUSH, "bush_suppawpaw_b"), Some("BushMedium"));
        assert_eq!(
            plants.gatherable("Slices\\Gatherables\\Master_Bush.dynamicslice", PAWPAW.1),
            Some("BushMedium"),
            "the slice name in any form"
        );
        assert_eq!(
            plants.gatherable(BUSH, "Bush_WaxMyrtle_a"),
            Some("BushLarge"),
            "a variant of no row: the slice's own"
        );
        assert_eq!(plants.gatherable(BEECH, ""), Some("TreeHardwoodMedium"));
        assert_eq!(plants.gatherable(ROCK, ""), None, "no gatherable slice");
    }

    #[test]
    fn a_slice_names_its_gatherable_and_variation_table_in_its_components() {
        let text = |name: u32, text: &str| Element {
            name_crc: Some(name),
            value: text.as_bytes().to_vec(),
            ..Element::default()
        };
        let component = |type_id: [u8; 16], field: Element| Element {
            type_id,
            children: vec![field],
            ..Element::default()
        };
        let root = Element {
            children: vec![Element {
                children: vec![
                    component(GATHERABLE_CONTROLLER_TYPE, text(GATHERABLE_ENTRY_FIELD, "BushLarge")),
                    component(VARIATION_DATA, text(VARIATION_TABLE_FIELD, BUSH_TABLE)),
                ],
                ..Element::default()
            }],
            ..Element::default()
        };
        assert_eq!(
            PlantSlice::of(&root),
            PlantSlice {
                gatherable: Some("BushLarge".to_owned()),
                table: Some(BUSH_TABLE.to_owned()),
            }
        );
        assert_eq!(PlantSlice::of(&Element::default()), PlantSlice::default());
    }

    #[test]
    fn the_gatherable_plants_of_a_distribution_stand_on_the_terrain() {
        let world = WorldFile {
            level: LevelInfo {
                region_size: REGION_SIDE,
                has_terrain: true,
                ..LevelInfo::default()
            },
            regions: vec![Region {
                x: 1,
                y: 0,
                heights: Some(HeightGrid {
                    samples: vec![FLAT_SAMPLE; 4],
                }),
            }],
            ..WorldFile::default()
        };
        let row = |slice: u16, at: [f32; 2]| Row {
            slice,
            at,
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: 1.0,
            height: HeightSource::Terrain,
        };
        let distribution = Distribution {
            slices: vec![BUSH.to_owned(), ROCK.to_owned(), BEECH.to_owned()],
            variants: vec![PAWPAW.1.to_owned(), String::new(), String::new()],
            rows: vec![row(0, [10.0, 20.0]), row(1, [11.0, 21.0]), row(2, [100.0, 200.0])],
        };
        let points = plant_points("r_+01_+00", &distribution, &world, &plants());
        let height = world.height_at(2058.0, 20.0).expect("terrain");
        assert_eq!(
            points
                .iter()
                .map(|point| (point.gatherable_id, point.position))
                .collect::<Vec<_>>(),
            [
                (lowercase_crc32("BushMedium"), [2058.0, 20.0, height]),
                (lowercase_crc32("TreeHardwoodMedium"), [2148.0, 200.0, height]),
            ],
            "the rock is no gatherable"
        );
        assert!(
            plant_points("r_+05_+05", &distribution, &world, &plants()).is_empty(),
            "no terrain there"
        );
    }
}
