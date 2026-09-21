// SPDX-License-Identifier: AGPL-3.0-or-later
//! Crafting recipes (beach-quests note, section 4): a recipe makes `OutputQty` of its item from up
//! to seven ingredients at one of its stations. An ingredient of type `Category_Only` is any item
//! whose `IngredientCategories` hold the category (the Light Ration takes one
//! `FoodIngredientT1`: the captured craft used `PorkT1`); every other ingredient names an item.

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The ingredient type of a category.
const CATEGORY_ONLY: &str = "Category_Only";
/// Separates the categories of an `IngredientCategories` cell.
const CATEGORY_SEPARATOR: char = ',';

/// What one ingredient of a recipe takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngredientKind {
    /// This item.
    Item(String),
    /// Any item of this category.
    Category(String),
}

/// One ingredient of a recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ingredient {
    /// What it takes.
    pub kind: IngredientKind,
    /// How many per craft.
    pub quantity: u32,
}

/// One recipe of `CraftingRecipeData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    /// `RecipeID`.
    pub id: String,
    /// `ItemID`: what it makes.
    pub item: String,
    /// `OutputQty`.
    pub quantity: u32,
    /// The ingredients, in column order.
    pub ingredients: Vec<Ingredient>,
    /// `StationType1` to `StationType4`.
    pub stations: Vec<String>,
}

impl Recipe {
    /// Whether station type `station` (any case) makes it.
    pub fn made_at(&self, station: &str) -> bool {
        self.stations
            .iter()
            .any(|known| known.eq_ignore_ascii_case(station))
    }
}

/// Every recipe by the CRC-32 of its lowercase id, and the ingredient categories of every item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recipes {
    by_crc: HashMap<u32, Recipe>,
    /// The categories of each item, by lowercase item id.
    categories: HashMap<String, Vec<String>>,
}

/// A quantity cell as a count of at least 1.
fn count_of(cell: Option<i64>) -> u32 {
    cell.and_then(|quantity| u32::try_from(quantity).ok())
        .unwrap_or(1)
        .max(1)
}

impl Recipes {
    /// The recipes and item categories of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let recipes = tables
            .crafting_recipe_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let cells = [
                    (&row.ingredient1, row.qty1, &row.type1),
                    (&row.ingredient2, row.qty2, &row.type2),
                    (&row.ingredient3, row.qty3, &row.type3),
                    (&row.ingredient4, row.qty4, &row.type4),
                    (&row.ingredient5, row.qty5, &row.type5),
                    (&row.ingredient6, row.qty6, &row.type6),
                    (&row.ingredient7, row.qty7, &row.type7),
                ];
                let ingredients = cells
                    .into_iter()
                    .filter_map(|(name, quantity, kind)| {
                        let name = name.clone().filter(|name| !name.is_empty())?;
                        let kind = if kind.as_deref() == Some(CATEGORY_ONLY) {
                            IngredientKind::Category(name)
                        } else {
                            IngredientKind::Item(name)
                        };
                        Some(Ingredient {
                            kind,
                            quantity: count_of(quantity),
                        })
                    })
                    .collect();
                Some(Recipe {
                    id: row.recipe_id.clone(),
                    item: row.item_id.clone().filter(|item| !item.is_empty())?,
                    quantity: count_of(row.output_qty),
                    ingredients,
                    stations: [
                        &row.station_type1,
                        &row.station_type2,
                        &row.station_type3,
                        &row.station_type4,
                    ]
                    .into_iter()
                    .filter_map(Clone::clone)
                    .filter(|station| !station.is_empty())
                    .collect(),
                })
            });
        let categories = tables
            .master_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let cell = row.ingredient_categories.as_deref()?;
                Some((row.item_id.clone(), cell.to_owned()))
            });
        Self::build(recipes, categories)
    }

    /// The book of `recipes`, with each item's `IngredientCategories` cell in `categories` (item
    /// id, cell).
    pub fn build(
        recipes: impl IntoIterator<Item = Recipe>,
        categories: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        Self {
            by_crc: recipes
                .into_iter()
                .map(|recipe| (lowercase_crc32(&recipe.id), recipe))
                .collect(),
            categories: categories
                .into_iter()
                .map(|(item, cell)| {
                    let list = cell
                        .split(CATEGORY_SEPARATOR)
                        .map(str::trim)
                        .filter(|category| !category.is_empty())
                        .map(str::to_owned)
                        .collect();
                    (item.to_lowercase(), list)
                })
                .collect(),
        }
    }

    /// The recipe whose lowercase id has the CRC-32 `crc`.
    pub fn get(&self, crc: u32) -> Option<&Recipe> {
        self.by_crc.get(&crc)
    }

    /// Whether item `item` (any case) is what `ingredient` takes.
    pub fn fits(&self, ingredient: &Ingredient, item: &str) -> bool {
        match &ingredient.kind {
            IngredientKind::Item(wanted) => wanted.eq_ignore_ascii_case(item),
            IngredientKind::Category(category) => self
                .categories
                .get(&item.to_lowercase())
                .is_some_and(|list| {
                    list.iter()
                        .any(|known| known.eq_ignore_ascii_case(category))
                }),
        }
    }
}

/// The resources a structure's blueprint costs (`BlueprintItemDefinitions`
/// `StructureCost_Ingredient1..4` and `StructureCost_Qty1..4`; Observed: `CampT1` costs 5
/// `WoodT1` and 1 `FlintT1`, the resources the retail campfire group lists as committed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StructureCosts {
    by_blueprint: HashMap<String, Vec<(String, u32)>>,
}

impl StructureCosts {
    /// The costs of every blueprint of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .blueprint_item_definitions
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    let cells = [
                        (&row.structure_cost_ingredient1, row.structure_cost_qty1),
                        (&row.structure_cost_ingredient2, row.structure_cost_qty2),
                        (&row.structure_cost_ingredient3, row.structure_cost_qty3),
                        // The table types `StructureCost_Qty4` as text.
                        (
                            &row.structure_cost_ingredient4,
                            row.structure_cost_qty4
                                .as_deref()
                                .and_then(|quantity| quantity.parse().ok()),
                        ),
                    ];
                    let costs = cells
                        .into_iter()
                        .filter_map(|(item, quantity)| {
                            let item = item.clone().filter(|item| !item.is_empty())?;
                            Some((item, count_of(quantity)))
                        })
                        .collect();
                    (row.blueprint_id.clone(), costs)
                }),
        )
    }

    /// The costs of `blueprints`: (blueprint id, (item id, quantity) in column order).
    pub fn build(blueprints: impl IntoIterator<Item = (String, Vec<(String, u32)>)>) -> Self {
        Self {
            by_blueprint: blueprints
                .into_iter()
                .map(|(id, costs)| (id.to_lowercase(), costs))
                .collect(),
        }
    }

    /// The costs of blueprint `id` (any case); none for an unknown one.
    pub fn of(&self, id: &str) -> &[(String, u32)] {
        self.by_blueprint
            .get(&id.to_lowercase())
            .map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blueprints_costs_are_found_in_any_case() {
        let costs = StructureCosts::build([(
            "CampT1".to_owned(),
            vec![("WoodT1".to_owned(), 5), ("FlintT1".to_owned(), 1)],
        )]);
        assert_eq!(
            costs.of("campt1"),
            [("WoodT1".to_owned(), 5), ("FlintT1".to_owned(), 1)]
        );
        assert!(costs.of("").is_empty());
    }

    /// The retail rows of the skinning knife and the Light Ration (build 25810808), with the
    /// categories of pork and wood.
    fn recipes() -> Recipes {
        let item = |name: &str| Ingredient {
            kind: IngredientKind::Item(name.to_owned()),
            quantity: 1,
        };
        Recipes::build(
            [
                Recipe {
                    id: "1hSkinningKnifeT1".to_owned(),
                    item: "1hSkinningKnifeT1".to_owned(),
                    quantity: 1,
                    ingredients: vec![item("FlintT1"), item("WoodT1")],
                    stations: vec!["engineering2".to_owned(), "camp1".to_owned()],
                },
                Recipe {
                    id: "Procedural_HealthRecoveryT1".to_owned(),
                    item: "FoodHealthRecoveryT1".to_owned(),
                    quantity: 1,
                    ingredients: vec![Ingredient {
                        kind: IngredientKind::Category("FoodIngredientT1".to_owned()),
                        quantity: 1,
                    }],
                    stations: vec!["cooking2".to_owned(), "camp1".to_owned()],
                },
            ],
            [
                (
                    "PorkT1".to_owned(),
                    "MeatT1,Meats,FoodIngredient,FoodIngredientT1".to_owned(),
                ),
                ("WoodT1".to_owned(), "Wood".to_owned()),
            ],
        )
    }

    #[test]
    fn a_recipe_is_found_by_the_crc_of_its_id_and_takes_its_ingredients() {
        // Retail 2997 recipe CRCs (beach-quests note).
        const KNIFE: u32 = 0xa546_f7c5;
        const RATION: u32 = 0x968f_ae8b;
        let recipes = recipes();
        let knife = recipes.get(KNIFE).expect("knife");
        assert_eq!(knife.item, "1hSkinningKnifeT1");
        assert!(knife.made_at("Camp1"));
        assert!(!knife.made_at("cooking2"));
        assert!(recipes.fits(&knife.ingredients[0], "flintt1"));
        assert!(!recipes.fits(&knife.ingredients[0], "WoodT1"));
        let ration = recipes.get(RATION).expect("ration");
        assert_eq!(ration.item, "FoodHealthRecoveryT1");
        assert!(recipes.fits(&ration.ingredients[0], "PorkT1"), "a category");
        assert!(!recipes.fits(&ration.ingredients[0], "WoodT1"));
        assert_eq!(count_of(None), 1);
        assert_eq!(count_of(Some(0)), 1);
        assert_eq!(count_of(Some(3)), 3);
    }
}
