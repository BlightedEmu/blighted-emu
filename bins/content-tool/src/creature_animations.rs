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
use nw_formats::mannequin::{ActionList, CageGrid, ControllerDef, GridList, TagDefinition};
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
    Some(SliceMannequin {
        controller,
        grid,
        default_tags,
        main_hand: main_hand_in(&all),
    })
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
