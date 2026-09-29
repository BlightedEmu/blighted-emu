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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use nw_ai::data::FragmentLength;
use nw_content::Tables;
use nw_content::creature_animations::{CreatureRow, tag_row};
use nw_content::mob_prefabs::{ASSET_ID_LEN, MobPrefabs};
use nw_formats::asset_catalog::{ASSET_CATALOG_ENTRY, SLICE_SUB_ID, paths_of};
use nw_formats::mannequin::{
    ActionList, AnimDatabase, CageGrid, ControllerDef, GridList, TagDefinition,
};
use nw_formats::objectstream::{Element, lowercase_crc32, parse_object_stream};

use crate::ai_data::{AdbEntry, ClipLengths, adb_entries, fragment_length};
use crate::collision::types::QUERY_SHAPE_CAPSULE;
use crate::error::ToolError;
use crate::source::GameSource;

/// The Mannequin component's fields (Observed names: their CRCs are those of these names).
const DEFAULT_TAGS_FIELD: &str = "m_defaulttags";
const CONTROLLER_FIELD: &str = "m_controllerdefinition";
const ANIMATION_DATABASE_FIELD: &str = "m_animationdatabase";
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
/// A creature's body collider component's fields: the filter and the radius by name (their CRCs
/// are those of these names), the shape list by CRC (name Unknown, CRC Observed in every such
/// component).
const FILTER_FIELD: &str = "m_strFilter";
const SHAPES_FIELD_CRC: u32 = 0x91c7_40bc;
const RADIUS_FIELD: &str = "m_radius";
/// The prefix of the body collider filters (`AICharacterCollider_Lost`, ...).
const CHARACTER_COLLIDER_FILTER: &str = "AICharacterCollider";
/// Millimetres per metre.
const MM_PER_M: f32 = 1_000.0;
/// The entity name field, and the name of the entity that holds a skinnable corpse's gatherable
/// states (Observed: in the boar, wolf, turkey, alligator and rat slices, not in the sailor and
/// risen ones).
const ENTITY_NAME_FIELD: &str = "Name";
const GATHERABLE_STATES_ENTITY: &str = "Gatherable_States";
/// The type of the component that makes the remains gatherable (type id
/// `{B52D0D52-99CD-41BE-B6F7-D2519C15BF14}`, a client type, `FUN_142d1b4b0`; Observed: missing
/// from the `disabledgatherablevariants` rat, present on every other creature slice read).
const GATHERABLE_REMAINS_TYPE: [u8; 16] = [
    0xb5, 0x2d, 0x0d, 0x52, 0x99, 0xcd, 0x41, 0xbe, 0xb6, 0xf7, 0xd2, 0x51, 0x9c, 0x15, 0xbf, 0x14,
];
/// Scope time ticks per second (`DAT_14807f948` = 1/30 s).
const SCOPE_TICKS_PER_SECOND: f64 = 30.0;
/// The slack of a length counted in ticks: a length is a sum of f32 frame times, so a length
/// of exactly 56 ticks can come out a millionth of a tick short.
const TICK_SLACK: f64 = 1e-3;

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
    /// The animation database (lowercase pak path), when the slice names one.
    pub animation_database: Option<String>,
    /// Its default tags.
    pub default_tags: Vec<String>,
    /// The item in its main hand, when it has one.
    pub main_hand: Option<String>,
    /// The radius of its body's capsule, mm ([`collider_radius_mm`]).
    pub collider_radius_mm: Option<u32>,
    /// Its corpse can be skinned ([`skinnable`]).
    pub skinnable: bool,
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
    let controller = asset_path_below(field(controller_crc)?)?;
    let database_crc = lowercase_crc32(ANIMATION_DATABASE_FIELD);
    let animation_database = all
        .iter()
        .find(|element| element.name_crc == Some(database_crc))
        .and_then(|element| asset_path_below(element));
    let grid =
        asset_hint(&field(GRID_FIELD_CRC)?.value).filter(|grid| grid.ends_with(GRID_SUFFIX))?;
    let default_tags = field(lowercase_crc32(DEFAULT_TAGS_FIELD))
        .map(|tags| tags.children.iter().filter_map(string_of).collect())
        .unwrap_or_default();
    Some(SliceMannequin {
        controller,
        grid,
        animation_database,
        default_tags,
        main_hand: main_hand_in(&all),
        collider_radius_mm: collider_radius_mm(&all),
        skinnable: skinnable(&all),
    })
}

/// Whether the corpse of the creature among the slice elements `all` can be skinned: the slice has
/// the gatherable states entity and the gatherable remains component (Observed over the 155
/// creature slices of 150 retail ledgers whose dead stayed at least 5 times: the 18 of them with
/// both stayed until skinned or 1.7 to 96 s, every other but one a fixed time per creature).
fn skinnable(all: &[&Element]) -> bool {
    let name_crc = lowercase_crc32(ENTITY_NAME_FIELD);
    let states = all.iter().any(|element| {
        element.name_crc == Some(name_crc)
            && element
                .as_string()
                .is_ok_and(|name| name == GATHERABLE_STATES_ENTITY)
    });
    states
        && all
            .iter()
            .any(|element| element.type_id == GATHERABLE_REMAINS_TYPE)
}

/// The radius of the body's capsule among the slice elements `all`, mm: the capsule query shape
/// of the component whose filter is an `AICharacterCollider` one (Observed in the build's
/// creature slices: one such component each, a capsule of 0.5 m for the Lost, 0.7 m for an
/// Ancient Guardian, 1.5 m for a large Angry Earth; the filters block the player's,
/// `default.collisionfilters`).
fn collider_radius_mm(all: &[&Element]) -> Option<u32> {
    let filter_crc = lowercase_crc32(FILTER_FIELD);
    let radius_crc = lowercase_crc32(RADIUS_FIELD);
    let component = all.iter().find(|element| {
        element.children.iter().any(|child| {
            child.name_crc == Some(filter_crc)
                && child
                    .as_string()
                    .is_ok_and(|filter| filter.starts_with(CHARACTER_COLLIDER_FILTER))
        })
    })?;
    let radius = component
        .children
        .iter()
        .filter(|child| child.name_crc == Some(SHAPES_FIELD_CRC))
        .flat_map(|shape| &shape.children)
        .filter(|shape| shape.type_id == QUERY_SHAPE_CAPSULE)
        .flat_map(|shape| &shape.children)
        .find(|field| field.name_crc == Some(radius_crc))
        .and_then(|field| field.as_f32().ok())?;
    // A capsule radius is a few metres.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mm = (radius * MM_PER_M).round().max(0.0) as u32;
    Some(mm)
}

/// The `AssetPath` (lowercase) below the asset field `field`.
fn asset_path_below(field: &Element) -> Option<String> {
    let mut below = Vec::new();
    descendants(field, &mut below);
    let asset_path_crc = lowercase_crc32(ASSET_PATH_FIELD);
    below
        .iter()
        .find(|element| element.name_crc == Some(asset_path_crc))
        .and_then(|element| string_of(element))
        .filter(|path| !path.is_empty())
        .map(|path| path.to_lowercase())
}

/// A length in whole scope ticks (the last tick the fragment plays); `None` for a loop, an
/// Unknown length or one beyond `u32`.
fn whole_ticks(length: FragmentLength) -> Option<u32> {
    let FragmentLength::Seconds(seconds) = length else {
        return None;
    };
    let ticks = (f64::from(seconds) * SCOPE_TICKS_PER_SECOND + TICK_SLACK).floor();
    if !(0.0..=f64::from(u32::MAX)).contains(&ticks) {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked to be within u32 above"
    )]
    let whole = ticks as u32;
    Some(whole)
}

/// An animation database: its entries' base layers and its fragments, in the same order.
struct ParsedDatabase {
    /// The base animation layer of each entry.
    entries: Vec<AdbEntry>,
    /// The entries.
    database: AnimDatabase,
}

impl ParsedDatabase {
    /// The database `text`; `None` when it does not parse or its two readings do not line up.
    fn parse(text: &str) -> Option<Self> {
        let entries = adb_entries(text).ok()?;
        let database = AnimDatabase::parse(text).ok()?;
        (entries.len() == database.fragments.len()).then_some(Self { entries, database })
    }

    /// The whole scope ticks of each option of each fragment of `fragments` for `tags`
    /// (Mannequin's options, [`AnimDatabase::options`]), by `lengths`; a fragment without an
    /// option of known length is left out.
    fn fragment_ticks(
        &self,
        fragments: &[String],
        tags: &[String],
        lengths: &ClipLengths,
    ) -> BTreeMap<String, Vec<Option<u32>>> {
        let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
        fragments
            .iter()
            .filter_map(|name| {
                let ticks: Vec<Option<u32>> = self
                    .database
                    .options(name, &tags)
                    .into_iter()
                    .map(|option| {
                        let index = self
                            .database
                            .fragments
                            .iter()
                            .position(|entry| std::ptr::eq(entry, option))?;
                        whole_ticks(fragment_length(self.entries.get(index)?, lengths).ok()?)
                    })
                    .collect();
                ticks
                    .iter()
                    .any(Option::is_some)
                    .then(|| (name.clone(), ticks))
            })
            .collect()
    }
}

/// The item of the main hand of the creature slice `root`'s paperdoll.
pub fn main_hand_item(root: &Element) -> Option<String> {
    let mut all = Vec::new();
    descendants(root, &mut all);
    main_hand_in(&all)
}

/// The main-hand item among the elements `all` of a slice.
fn main_hand_in(all: &[&Element]) -> Option<String> {
    let item_crc = lowercase_crc32(ITEM_NAME_FIELD);
    all.iter().find_map(|element| {
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
pub(crate) fn read_entries(
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

pub(crate) fn text_of(bytes: Option<&Vec<u8>>) -> Option<&str> {
    std::str::from_utf8(bytes?).ok()
}

/// A creature asset id.
pub type AssetId = [u8; ASSET_ID_LEN];

/// The slice of each creature asset, and the Mannequin component of each slice.
pub type SliceMannequins = (HashMap<AssetId, String>, HashMap<AssetId, SliceMannequin>);

/// The slice (by the asset catalog) of each of the creature `assets` that has one, and the
/// Mannequin component of each slice that has one.
///
/// # Errors
///
/// Returns [`ToolError`] when the paks or the asset catalog cannot be read.
pub fn slice_mannequins(
    source: &GameSource,
    assets: &HashSet<AssetId>,
) -> Result<SliceMannequins, ToolError> {
    let wanted = assets.iter().map(|asset| (*asset, SLICE_SUB_ID)).collect();
    let mut catalog = None;
    source.for_each_entry(
        |path| path == ASSET_CATALOG_ENTRY,
        |_, bytes| {
            catalog = Some(paths_of(&bytes, &wanted).map_err(CreatureAnimationsError::from)?);
            Ok::<(), ToolError>(())
        },
    )?;
    let slices: HashMap<AssetId, String> = catalog
        .ok_or(CreatureAnimationsError::NoCatalog)?
        .into_iter()
        .map(|((asset, _), slice)| (asset, slice))
        .collect();
    let slice_bytes = read_entries(source, &slices.values().cloned().collect())?;
    let mannequins = slices
        .iter()
        .filter_map(|(asset, slice)| Some((*asset, slice_mannequin(slice_bytes.get(slice)?)?)))
        .collect();
    Ok((slices, mannequins))
}

/// The action lists that the grids `grids` (pak path, text) name, read from `source` and
/// parsed, by pak path; a list that is missing or unreadable is left out.
///
/// # Errors
///
/// Returns [`ToolError`] when the paks cannot be read.
pub fn grid_action_lists<'a>(
    source: &GameSource,
    grids: impl IntoIterator<Item = &'a [GridList]>,
) -> Result<HashMap<String, ActionList>, ToolError> {
    let paths = grids
        .into_iter()
        .flatten()
        .map(GridList::pak_path)
        .collect();
    Ok(read_entries(source, &paths)?
        .into_iter()
        .filter_map(|(path, bytes)| {
            let list = ActionList::parse(std::str::from_utf8(&bytes).ok()?).ok()?;
            Some((path, list))
        })
        .collect())
}

/// Builds the rows of the creatures of `prefabs` from `source`, with the weapon tags of
/// `tables`.
///
/// # Errors
///
/// Returns [`ToolError`] when the paks or the asset catalog cannot be read.
pub fn build_rows(
    source: &GameSource,
    game_dir: &Path,
    prefabs: &MobPrefabs,
    tables: &Tables,
) -> Result<(Vec<CreatureRow>, CreatureAnimationsReport), ToolError> {
    let assets: HashSet<AssetId> = prefabs.iter().map(|((_, asset), _)| asset).collect();
    let mut report = CreatureAnimationsReport {
        assets: assets.len(),
        ..CreatureAnimationsReport::default()
    };
    let (slices, mannequins) = slice_mannequins(source, &assets)?;
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
    let grids: HashMap<&str, Vec<GridList>> = mannequins
        .values()
        .filter_map(|mannequin| {
            let lists = CageGrid::lists(text_of(file_bytes.get(&mannequin.grid))?).ok()?;
            Some((mannequin.grid.as_str(), lists))
        })
        .collect();
    let action_lists = grid_action_lists(source, grids.values().map(Vec::as_slice))?;
    let tag_files: HashSet<String> = controllers
        .values()
        .map(|def| def.tags_file.to_lowercase())
        .collect();
    let tag_bytes = read_entries(source, &tag_files)?;
    let database_paths: HashSet<String> = mannequins
        .values()
        .filter_map(|mannequin| mannequin.animation_database.clone())
        .collect();
    let databases: HashMap<String, ParsedDatabase> = read_entries(source, &database_paths)?
        .into_iter()
        .filter_map(|(path, bytes)| {
            let parsed = ParsedDatabase::parse(std::str::from_utf8(&bytes).ok()?)?;
            Some((path, parsed))
        })
        .collect();
    let clip_names: BTreeSet<String> = databases
        .values()
        .flat_map(|parsed| parsed.entries.iter().flatten())
        .map(|clip| clip.animation.clone())
        .filter(|name| !name.is_empty())
        .collect();
    let lengths = ClipLengths::read(source, game_dir, &clip_names)?;
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
        let Some(lists) = grids.get(mannequin.grid.as_str()) else {
            skip("grid missing or unreadable");
            continue;
        };
        let grid = match CageGrid::number(lists, |path| action_lists.get(path)) {
            Ok(grid) => grid,
            Err(error) => {
                skip(&error.to_string());
                continue;
            }
        };
        let mut default_tags = mannequin.default_tags.clone();
        if let Some(weapon) = mannequin
            .main_hand
            .as_ref()
            .and_then(|item| weapons.get(&item.to_lowercase()))
        {
            default_tags.extend(weapon.iter().cloned());
        }
        let fragment_ticks = mannequin
            .animation_database
            .as_ref()
            .and_then(|path| databases.get(path))
            .map(|parsed| parsed.fragment_ticks(&controller.fragments, &default_tags, &lengths))
            .unwrap_or_default();
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
            fragment_ticks,
            collider_radius_mm: mannequin.collider_radius_mm,
            skinnable: mannequin.skinnable,
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

    /// A collider component of filter `filter` whose shape is a capsule of `radius` m.
    fn collider(filter: &str, radius: f32) -> Element {
        let capsule = Element {
            type_id: QUERY_SHAPE_CAPSULE,
            ..named(
                "element",
                vec![Element::with_value(
                    lowercase_crc32(RADIUS_FIELD),
                    radius.to_be_bytes().to_vec(),
                )],
            )
        };
        Element::with_children(
            0,
            vec![
                text(FILTER_FIELD, filter),
                Element::with_children(SHAPES_FIELD_CRC, vec![capsule]),
            ],
        )
    }

    /// A slice with the boar's Mannequin component, a paperdoll of a main-hand item, a hit
    /// volume and its body collider.
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
        let database = named(
            ANIMATION_DATABASE_FIELD,
            vec![text(
                ASSET_PATH_FIELD,
                "Animations/Mannequin/ADB/NPC_Boar_Anims.adb",
            )],
        );
        let item = Element::with_children(
            0,
            vec![
                Element::with_value(ITEM_SLOT_FIELD_CRC, MAIN_HAND_SLOT.to_be_bytes().to_vec()),
                text(ITEM_NAME_FIELD, "Wolf_Teeth"),
            ],
        );
        write_object_stream(&Element::with_children(
            0,
            vec![
                component,
                database,
                item,
                collider("HitVolume", 0.1),
                collider("AICharacterCollider_Predator", 0.6),
                text(ENTITY_NAME_FIELD, GATHERABLE_STATES_ENTITY),
                gatherable_remains(),
            ],
        ))
        .expect("stream")
    }

    /// The component that makes a creature's remains gatherable.
    fn gatherable_remains() -> Element {
        Element {
            type_id: GATHERABLE_REMAINS_TYPE,
            ..named("element", Vec::new())
        }
    }

    #[test]
    fn a_corpse_is_skinnable_with_its_gatherable_states_and_remains() {
        let skinnable = |elements: Vec<Element>| {
            let root = Element::with_children(0, elements);
            let mut all = Vec::new();
            descendants(&root, &mut all);
            skinnable(&all)
        };
        let states = || text(ENTITY_NAME_FIELD, GATHERABLE_STATES_ENTITY);
        assert!(skinnable(vec![states(), gatherable_remains()]), "a boar");
        assert!(!skinnable(vec![gatherable_remains()]), "a sailor");
        assert!(!skinnable(vec![states()]), "the rat without gatherable");
        assert!(slice_mannequin(&boar_slice()).expect("mannequin").skinnable);
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
        assert_eq!(
            mannequin.collider_radius_mm,
            Some(600),
            "the body, not the hit volume"
        );
        assert_eq!(
            mannequin.animation_database.as_deref(),
            Some("animations/mannequin/adb/npc_boar_anims.adb")
        );
    }

    /// A cut of `npc_boar_anims.adb` (build 25810808): its death's armadillo and plain entries
    /// and a looping idle.
    const BOAR_DATABASE: &str = r#"<AnimDB><FragmentList>
 <r_Death>
  <Fragment Tags="Armadillo+CurledBall"><AnimLayer><Blend ExitTime="0" StartTime="0"/><Animation name="armadillo_death"/></AnimLayer></Fragment>
  <Fragment Tags=""><AnimLayer><Blend ExitTime="0" StartTime="0"/><Animation name="boar_death"/></AnimLayer></Fragment>
 </r_Death>
 <Idle><Fragment Tags=""><AnimLayer><Blend ExitTime="0" StartTime="0"/><Animation name="boar_idle" flags="Loop"/></AnimLayer></Fragment></Idle>
</FragmentList></AnimDB>"#;

    #[test]
    fn a_fragment_lasts_the_whole_ticks_of_the_entry_the_creature_plays() {
        let parsed = ParsedDatabase::parse(BOAR_DATABASE).expect("database");
        // The boar's death clip: 2.963 s (89 frames at 1/30 s, 88.89 ticks); retail's dead boar
        // states ended at tick 0x58 (glide note).
        let lengths = ClipLengths::of(&[
            ("armadillo_death", Ok(3.363)),
            ("boar_death", Ok(2.963)),
            ("boar_idle", Ok(1.0)),
        ]);
        let fragments = ["r_Death".to_owned(), "Idle".to_owned(), "Spawn".to_owned()];
        let ticks = parsed.fragment_ticks(&fragments, &["Boar".to_owned()], &lengths);
        assert_eq!(
            ticks,
            BTreeMap::from([("r_Death".to_owned(), vec![Some(0x58)])]),
            "the plain entry; a loop and a fragment with no entry have no length"
        );
    }

    #[test]
    fn each_option_of_a_fragment_has_its_ticks_in_file_order() {
        // `npc_risen_anims.adb`: two plain `r_Death` entries of 56 and 73 frames; retail's dead
        // risen ended at tick 0x49 (option 1, its tail 0x8f).
        let parsed = ParsedDatabase::parse(
            r#"<AnimDB><FragmentList><r_Death>
 <Fragment Tags="BlightFiend"><AnimLayer><Blend ExitTime="0"/><Animation name="fiend_death"/></AnimLayer></Fragment>
 <Fragment Tags=""><AnimLayer><Blend ExitTime="0"/><Animation name="risen_death_a"/></AnimLayer></Fragment>
 <Fragment Tags=""><AnimLayer><Blend ExitTime="0"/><Animation name="risen_death_b"/></AnimLayer></Fragment>
</r_Death></FragmentList></AnimDB>"#,
        )
        .expect("database");
        let lengths = ClipLengths::of(&[
            ("fiend_death", Ok(4.6)),
            ("risen_death_a", Ok(1.866_666_7)),
            ("risen_death_b", Ok(2.433_333_4)),
        ]);
        let ticks = parsed.fragment_ticks(
            &["r_Death".to_owned()],
            &["Base".to_owned(), "DT5".to_owned()],
            &lengths,
        );
        assert_eq!(ticks["r_Death"], [Some(56), Some(0x49)]);
    }

    #[test]
    fn whole_ticks_count_the_last_tick_the_fragment_plays() {
        // The sailor's `r_Death_F`: 56 frames at 1/30 s, a little short in f32 (retail 0x38).
        assert_eq!(
            whole_ticks(FragmentLength::Seconds(1.866_666_6)),
            Some(0x38)
        );
        assert_eq!(whole_ticks(FragmentLength::Seconds(2.963)), Some(88));
        assert_eq!(whole_ticks(FragmentLength::Loops), None);
        assert_eq!(whole_ticks(FragmentLength::Unknown), None);
    }
}

/// The capture check of the action numbers: in the retail capture, every full-body scope entry
/// of every creature names an action that its grid lets play the entry's fragment.
#[cfg(all(test, feature = "local-data"))]
mod capture_check {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use nw_content::client_build::load_client_build;
    use nw_content::creature_animations::creature_animations_path;
    use nw_formats::local_data::game_dir;
    use nw_formats::mannequin::{Node, pak_path, parse_tree};

    use super::*;
    use crate::ai_trace::{read_creatures, trace_ledger};
    use crate::ftue_entities::read_ledger;

    const CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
    const CONTENT_DIR: &str = "local-data/content";
    /// The retail capture of a new character's tutorial and beach (2026-09-18).
    const LEDGER: &str = "local-data/captures/own/newchar-retail-2026-09-18/ledger.bin";
    /// The creature slices with full-body scope entries in it (Observed: boar, aggressive
    /// boar, club, sword, axe and miner sailors, FTUE club sailor, legionnaire, signifier,
    /// swarmer, risen, Thorpe, beach first mate).
    const CAPTURE_CREATURES: usize = 13;
    /// Action list XML (Observed in `baseai_idle.actionlist`, `baseai_poi.actionlist`,
    /// `baseai_death.actionlist` and the reaction tables they name).
    const ACTION_ELEMENT: &str = "Action";
    const NAME_ATTRIBUTE: &str = "Name";
    const FRAGMENT_NAME_ATTRIBUTE: &str = "FragmentName";
    /// An activity that swaps the action's fragment for one of its `Default` / `Conditional`
    /// children (`Idle_No_Task` plays `Combat_Idle` when alerted).
    const CHANGE_FRAGMENT_ELEMENT: &str = "ChangeActionFragment";
    const CHANGE_FRAGMENT_ATTRIBUTES: [&str; 2] = ["FragmentName", "Fragment"];
    /// A `ChangeActionFragment` child that takes the fragment from the blackboard (the POI
    /// actions: the waypoint's task names it, so any fragment can play).
    const BLACKBOARD_FRAGMENT_ATTRIBUTE: &str = "FragmentBlackboardKeyName";
    /// An activity that plays a fragment of a reaction table (reactions and deaths).
    const SET_REACTION_ELEMENT: &str = "SetReactionFragment";
    const REACTION_TABLE_ATTRIBUTE: &str = "ReactionTablePath";
    const REACTION_ROW_ELEMENT: &str = "ReactionTableData";
    const REACTION_ATTRIBUTE: &str = "Reaction";

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn below<'a>(node: &'a Node, name: &'a str) -> Vec<&'a Node> {
        let mut found = Vec::new();
        node.descendants(name, &mut found);
        found
    }

    /// The fragments that `action` can play; `None` when any can (blackboard fragment).
    fn fragments_of(action: &Node, tables: &HashMap<String, Node>) -> Option<HashSet<String>> {
        let mut fragments: HashSet<String> = action
            .attribute(FRAGMENT_NAME_ATTRIBUTE)
            .map(str::to_owned)
            .into_iter()
            .collect();
        for change in below(action, CHANGE_FRAGMENT_ELEMENT) {
            for option in &change.children {
                if option
                    .attribute(BLACKBOARD_FRAGMENT_ATTRIBUTE)
                    .is_some_and(|key| !key.is_empty())
                {
                    return None;
                }
                fragments.extend(
                    CHANGE_FRAGMENT_ATTRIBUTES
                        .iter()
                        .filter_map(|attribute| option.attribute(attribute))
                        .map(str::to_owned),
                );
            }
        }
        for table in reaction_tables(action) {
            let rows = tables
                .get(&table)
                .map(|table| below(table, REACTION_ROW_ELEMENT));
            fragments.extend(
                rows.into_iter()
                    .flatten()
                    .filter_map(|row| row.attribute(REACTION_ATTRIBUTE))
                    .map(str::to_owned),
            );
        }
        Some(fragments)
    }

    /// The reaction tables (pak paths) that `action` plays from.
    fn reaction_tables(action: &Node) -> Vec<String> {
        below(action, SET_REACTION_ELEMENT)
            .into_iter()
            .flat_map(|set| &set.children)
            .filter_map(|option| option.attribute(REACTION_TABLE_ATTRIBUTE))
            .map(pak_path)
            .collect()
    }

    fn parsed(bytes: HashMap<String, Vec<u8>>) -> HashMap<String, Node> {
        bytes
            .into_iter()
            .filter_map(|(path, bytes)| {
                Some((path, parse_tree(std::str::from_utf8(&bytes).ok()?).ok()?))
            })
            .collect()
    }

    #[test]
    fn every_retail_entry_names_an_action_that_plays_its_fragment() {
        let build = load_client_build(&repo().join(CLIENT_BUILD_CONFIG)).expect("client build");
        let creatures =
            read_creatures(&creature_animations_path(&repo().join(CONTENT_DIR), &build))
                .expect("run `content-tool creature-animations` first");
        let trace = trace_ledger(
            &read_ledger(&repo().join(LEDGER)).expect("ledger"),
            &creatures,
        );
        let entries: BTreeSet<(&str, &str, u16, String, String)> = trace
            .mobs
            .iter()
            .flat_map(|mob| {
                mob.samples.iter().filter_map(move |sample| {
                    let entry = sample.full_body.as_ref()?;
                    Some((
                        mob.asset.as_str(),
                        mob.slice.as_deref()?,
                        entry.start_id,
                        entry.fragment_name.clone()?,
                        entry.action_name.clone()?,
                    ))
                })
            })
            .collect();
        let assets: HashSet<AssetId> = entries
            .iter()
            .map(|(asset, ..)| {
                hex::decode(asset)
                    .expect("hex")
                    .try_into()
                    .expect("asset id")
            })
            .collect();
        let source = GameSource::open(&game_dir()).expect("game install");
        let (_, mannequins) = slice_mannequins(&source, &assets).expect("slices");
        let grid_paths = mannequins.values().map(|m| m.grid.clone()).collect();
        let grids: HashMap<String, Vec<GridList>> = read_entries(&source, &grid_paths)
            .expect("grids")
            .into_iter()
            .map(|(path, bytes)| {
                let text = std::str::from_utf8(&bytes).expect("grid text");
                (path, CageGrid::lists(text).expect("grid"))
            })
            .collect();
        let list_paths = grids.values().flatten().map(GridList::pak_path).collect();
        let lists = parsed(read_entries(&source, &list_paths).expect("action lists"));
        let table_paths = lists
            .values()
            .flat_map(|list| below(list, ACTION_ELEMENT))
            .flat_map(reaction_tables)
            .collect();
        let tables = parsed(read_entries(&source, &table_paths).expect("reaction tables"));
        let mut disagreements: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
        for (asset, slice, _, fragment, action) in &entries {
            let id: AssetId = hex::decode(asset).expect("hex").try_into().expect("id");
            let grid = &grids[&mannequins[&id].grid];
            let plays = grid
                .iter()
                .filter(|list| {
                    list.aliased
                        .iter()
                        .any(|named| named.name.eq_ignore_ascii_case(action))
                })
                .filter_map(|list| lists.get(&list.pak_path()))
                .flat_map(|list| below(list, ACTION_ELEMENT))
                .filter(|node| {
                    node.attribute(NAME_ATTRIBUTE)
                        .is_some_and(|name| name.eq_ignore_ascii_case(action))
                })
                .any(|node| {
                    fragments_of(node, &tables).is_none_or(|known| known.contains(fragment))
                });
            let found = disagreements.entry(slice).or_default();
            if !plays {
                found.push((action.clone(), fragment.clone()));
            }
        }
        assert_eq!(disagreements.len(), CAPTURE_CREATURES, "{disagreements:#?}");
        assert!(
            disagreements.values().all(Vec::is_empty),
            "{disagreements:#?}"
        );
    }
}
