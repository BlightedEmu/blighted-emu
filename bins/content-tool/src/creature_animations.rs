// SPDX-License-Identifier: AGPL-3.0-or-later
//! `creature-animations`: builds the creature animation table (`creature-animations-<build>.json`,
//! read by `nw_content::creature_animations`) from the game paks (creature-animation note,
//! 2026-09-18).
//!
//! The creatures are the slice assets of the mob prefab table: the ones a sim can show. The asset
//! catalog gives each asset's slice. The slice's Mannequin component names the controller
//! definitions, the CAGE grid and the default tags; its paperdoll names the main-hand weapon,
//! whose `MannequinTag` adds tags (retail: the sailor's `1H_Melee` and `Club` come from its
//! `1hClubT2FTUE`). The controller definitions name the tag definition.

use std::collections::{BTreeMap, HashMap, HashSet};

use nw_content::Tables;
use nw_content::creature_animations::{CreatureRow, tag_row};
use nw_content::mob_prefabs::{ASSET_ID_LEN, MobPrefabs};
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, SLICE_SUB_ID, paths_of};
use nw_formats::mannequin::{CageGrid, ControllerDef, TagDefinition};
use nw_formats::objectstream::{Element, lowercase_crc32, parse_object_stream};

use crate::error::ToolError;
use crate::source::GameSource;

/// The Mannequin component's fields (Observed names: their CRCs are those of these names).
const DEFAULT_TAGS_FIELD: &str = "m_defaulttags";
const CONTROLLER_FIELD: &str = "m_controllerdefinition";
const ASSET_PATH_FIELD: &str = "assetpath";
/// The CAGE grid field of the same component: its name is Unknown, its CRC Observed in every
/// creature slice read (a value holding an asset reference whose hint ends in `.grid`).
const GRID_FIELD_CRC: u32 = 0x0d58_32e9;
/// An asset reference value: GUID (16), sub id and padding (16), asset type (16), then the hint's
/// length (`u64` big-endian) and the hint (Observed).
const ASSET_HINT_LENGTH_AT: usize = 48;
const ASSET_HINT_AT: usize = ASSET_HINT_LENGTH_AT + size_of::<u64>();
const GRID_SUFFIX: &str = ".grid";
/// The paperdoll item fields: the item name, and the slot (name Unknown, CRC Observed).
const ITEM_NAME_FIELD: &str = "m_itemname";
const ITEM_SLOT_FIELD_CRC: u32 = 0x5cdb_aa46;
/// The main hand's paperdoll slot (`MAIN_HAND_OPTION_1`).
const MAIN_HAND_SLOT: u32 = 24;
/// The separator of a `MannequinTag` cell (retail `1H_Melee,Club`).
const MANNEQUIN_TAG_SEPARATOR: char = ',';

/// A failure of the creature animation build.
#[derive(Debug, thiserror::Error)]
pub enum CreatureAnimationsError {
    /// The asset catalog cannot be read.
    #[error("asset catalog: {0}")]
    Catalog(#[from] nw_formats::asset_catalog::AssetCatalogError),
    /// The game install has no asset catalog.
    #[error("the game install has no {ASSET_CATALOG_ENTRY}")]
    NoCatalog,
    /// The mob prefab table or the output cannot be read or written.
    #[error(transparent)]
    Table(#[from] nw_content::creature_animations::CreatureAnimationsError),
    /// The mob prefab table cannot be read.
    #[error(transparent)]
    Prefabs(#[from] nw_content::mob_prefabs::MobPrefabsError),
}

/// What the build did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreatureAnimationsReport {
    /// Creature assets in the mob prefab table.
    pub assets: usize,
    /// Creatures written.
    pub creatures: usize,
    /// Assets with a problem, and what it was.
    pub skipped: Vec<(String, String)>,
}

/// What a creature slice says of its animations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SliceMannequin {
    /// The controller definitions (lowercase pak path).
    pub controller: String,
    /// The CAGE grid (lowercase pak path).
    pub grid: String,
    /// Its default tags.
    pub default_tags: Vec<String>,
    /// The item in its main hand, when it has one.
    pub main_hand: Option<String>,
}

fn descendants<'a>(element: &'a Element, found: &mut Vec<&'a Element>) {
    for child in &element.children {
        found.push(child);
        descendants(child, found);
    }
}

fn string_of(element: &Element) -> Option<String> {
    element.as_string().ok().map(str::to_owned)
}

/// The hint path of an asset reference value.
fn asset_hint(value: &[u8]) -> Option<String> {
    let length = value.get(ASSET_HINT_LENGTH_AT..ASSET_HINT_AT)?;
    let length = usize::try_from(u64::from_be_bytes(length.try_into().ok()?)).ok()?;
    let hint = value.get(ASSET_HINT_AT..ASSET_HINT_AT.checked_add(length)?)?;
    std::str::from_utf8(hint).ok().map(str::to_lowercase)
}

/// What the creature slice `bytes` (a binary ObjectStream) says of its animations; `None` when
/// it has no Mannequin component with a controller and a grid.
pub fn slice_mannequin(bytes: &[u8]) -> Option<SliceMannequin> {
    let root = parse_object_stream(bytes).ok()?;
    let mut all = Vec::new();
    descendants(&root, &mut all);
    let controller_crc = lowercase_crc32(CONTROLLER_FIELD);
    let component = all.iter().find(|element| {
        element
            .children
            .iter()
            .any(|child| child.name_crc == Some(controller_crc))
    })?;
    let field = |crc: u32| {
        component
            .children
            .iter()
            .find(|child| child.name_crc == Some(crc))
    };
    let mut in_controller = Vec::new();
    descendants(field(controller_crc)?, &mut in_controller);
    let asset_path_crc = lowercase_crc32(ASSET_PATH_FIELD);
    let controller = in_controller
        .iter()
        .find(|element| element.name_crc == Some(asset_path_crc))
        .and_then(|element| string_of(element))?
        .to_lowercase();
    let grid =
        asset_hint(&field(GRID_FIELD_CRC)?.value).filter(|grid| grid.ends_with(GRID_SUFFIX))?;
    let default_tags = field(lowercase_crc32(DEFAULT_TAGS_FIELD))
        .map(|tags| tags.children.iter().filter_map(string_of).collect())
        .unwrap_or_default();
    let item_crc = lowercase_crc32(ITEM_NAME_FIELD);
    let main_hand = all.iter().find_map(|element| {
        let slot = element
            .children
            .iter()
            .find(|child| child.name_crc == Some(ITEM_SLOT_FIELD_CRC))?
            .as_u32()
            .ok()?;
        (slot == MAIN_HAND_SLOT)
            .then(|| {
                element
                    .children
                    .iter()
                    .find(|child| child.name_crc == Some(item_crc))
                    .and_then(string_of)
            })
            .flatten()
    });
    Some(SliceMannequin {
        controller,
        grid,
        default_tags,
        main_hand,
    })
}

/// The `MannequinTag` tags of each item's weapon row (by lowercase item id): the master item's
/// `ItemStatsRef` names its weapon row.
pub fn weapon_tags(tables: &Tables) -> HashMap<String, Vec<String>> {
    let weapon_tags: HashMap<String, &str> = tables
        .weapon_item_definitions
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| Some((row.weapon_id.to_lowercase(), row.mannequin_tag.as_deref()?)))
        .collect();
    tables
        .master_item_definitions
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let stats = row.item_stats_ref.as_deref()?.to_lowercase();
            let tags = weapon_tags.get(&stats)?;
            Some((
                row.item_id.to_lowercase(),
                tags.split(MANNEQUIN_TAG_SEPARATOR)
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ))
        })
        .collect()
}

/// Reads the pak entries `paths` (lowercase) of `source`, by path.
fn read_entries(
    source: &GameSource,
    paths: &HashSet<String>,
) -> Result<HashMap<String, Vec<u8>>, ToolError> {
    let mut read = HashMap::new();
    source.for_each_entry(
        |path| paths.contains(path),
        |path, bytes| {
            read.insert(path.to_owned(), bytes);
            Ok::<(), ToolError>(())
        },
    )?;
    Ok(read)
}

fn text_of(bytes: Option<&Vec<u8>>) -> Option<&str> {
    std::str::from_utf8(bytes?).ok()
}

/// Builds the rows of the creatures of `prefabs` from `source`, with the weapon tags of
/// `tables`.
///
/// # Errors
///
/// Returns [`ToolError`] when the paks or the asset catalog cannot be read.
pub fn build_rows(
    source: &GameSource,
    prefabs: &MobPrefabs,
    tables: &Tables,
) -> Result<(Vec<CreatureRow>, CreatureAnimationsReport), ToolError> {
    let assets: HashSet<[u8; ASSET_ID_LEN]> = prefabs.iter().map(|((_, asset), _)| asset).collect();
    let mut report = CreatureAnimationsReport {
        assets: assets.len(),
        ..CreatureAnimationsReport::default()
    };
    let wanted = assets.iter().map(|asset| (*asset, SLICE_SUB_ID)).collect();
    let mut catalog = None;
    source.for_each_entry(
        |path| path == ASSET_CATALOG_ENTRY,
        |_, bytes| {
            catalog = Some(paths_of(&bytes, &wanted).map_err(CreatureAnimationsError::from)?);
            Ok::<(), ToolError>(())
        },
    )?;
    let slices: HashMap<[u8; ASSET_ID_LEN], String> = catalog
        .ok_or(CreatureAnimationsError::NoCatalog)?
        .into_iter()
        .map(|((asset, _), slice)| (asset, slice))
        .collect();
    let slice_bytes = read_entries(source, &slices.values().cloned().collect())?;
    let mannequins: HashMap<[u8; ASSET_ID_LEN], SliceMannequin> = slices
        .iter()
        .filter_map(|(asset, slice)| Some((*asset, slice_mannequin(slice_bytes.get(slice)?)?)))
        .collect();
    let files: HashSet<String> = mannequins
        .values()
        .flat_map(|mannequin| [mannequin.controller.clone(), mannequin.grid.clone()])
        .collect();
    let file_bytes = read_entries(source, &files)?;
    let controllers: HashMap<&str, ControllerDef> = mannequins
        .values()
        .filter_map(|mannequin| {
            let def = ControllerDef::parse(text_of(file_bytes.get(&mannequin.controller))?).ok()?;
            Some((mannequin.controller.as_str(), def))
        })
        .collect();
    let tag_files: HashSet<String> = controllers
        .values()
        .map(|def| def.tags_file.to_lowercase())
        .collect();
    let tag_bytes = read_entries(source, &tag_files)?;
    let weapons = weapon_tags(tables);
    let mut rows = Vec::new();
    for asset in &assets {
        let name = hex::encode(asset);
        let mut skip = |why: &str| report.skipped.push((name.clone(), why.to_owned()));
        let Some(slice) = slices.get(asset) else {
            skip("no slice in the asset catalog");
            continue;
        };
        let Some(mannequin) = mannequins.get(asset) else {
            skip("no Mannequin component with a controller and a grid in its slice");
            continue;
        };
        let Some(controller) = controllers.get(mannequin.controller.as_str()) else {
            skip("controller definitions missing or unreadable");
            continue;
        };
        let Some(tags) = text_of(tag_bytes.get(&controller.tags_file.to_lowercase()))
            .and_then(|text| TagDefinition::parse(text).ok())
        else {
            skip("tag definition missing or unreadable");
            continue;
        };
        let Some(grid) =
            text_of(file_bytes.get(&mannequin.grid)).and_then(|text| CageGrid::parse(text).ok())
        else {
            skip("grid missing or unreadable");
            continue;
        };
        let mut default_tags = mannequin.default_tags.clone();
        if let Some(weapon) = mannequin
            .main_hand
            .as_ref()
            .and_then(|item| weapons.get(&item.to_lowercase()))
        {
            default_tags.extend(weapon.iter().cloned());
        }
        rows.push(CreatureRow {
            asset: name,
            slice: slice.clone(),
            fragments: controller.fragments.clone(),
            scopes: controller.scopes.clone(),
            actions: grid.actions,
            tags: tags
                .iter()
                .map(|(tag, bits)| (tag.clone(), tag_row(bits)))
                .collect::<BTreeMap<_, _>>(),
            default_tags,
        });
    }
    rows.sort_by(|left, right| left.asset.cmp(&right.asset));
    report.creatures = rows.len();
    report.skipped.sort();
    Ok((rows, report))
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::write_object_stream;

    use super::*;

    fn named(name: &str, children: Vec<Element>) -> Element {
        Element::with_children(lowercase_crc32(name), children)
    }

    fn text(name: &str, value: &str) -> Element {
        Element::with_value(lowercase_crc32(name), value.as_bytes().to_vec())
    }

    fn grid_reference(hint: &str) -> Element {
        let mut value = vec![0; ASSET_HINT_LENGTH_AT];
        value.extend_from_slice(&u64::try_from(hint.len()).expect("short").to_be_bytes());
        value.extend_from_slice(hint.as_bytes());
        Element::with_value(GRID_FIELD_CRC, value)
    }

    /// A slice with the boar's Mannequin component and a paperdoll of a main-hand item.
    fn boar_slice() -> Vec<u8> {
        let component = Element::with_children(
            0,
            vec![
                grid_reference("SharedAssets/Characters/Predator/CAGE/Boar/Boar.grid"),
                named(DEFAULT_TAGS_FIELD, vec![text("element", "Boar")]),
                named(
                    CONTROLLER_FIELD,
                    vec![text(ASSET_PATH_FIELD, "Animations/Boar_ControllerDefs.xml")],
                ),
            ],
        );
        let item = Element::with_children(
            0,
            vec![
                Element::with_value(ITEM_SLOT_FIELD_CRC, MAIN_HAND_SLOT.to_be_bytes().to_vec()),
                text(ITEM_NAME_FIELD, "Wolf_Teeth"),
            ],
        );
        write_object_stream(&Element::with_children(0, vec![component, item])).expect("stream")
    }

    #[test]
    fn a_slice_names_its_controller_grid_tags_and_main_hand() {
        let mannequin = slice_mannequin(&boar_slice()).expect("mannequin");
        assert_eq!(mannequin.controller, "animations/boar_controllerdefs.xml");
        assert_eq!(
            mannequin.grid,
            "sharedassets/characters/predator/cage/boar/boar.grid"
        );
        assert_eq!(mannequin.default_tags, ["Boar"]);
        assert_eq!(mannequin.main_hand.as_deref(), Some("Wolf_Teeth"));
    }
}
