// SPDX-License-Identifier: AGPL-3.0-or-later
//! The places a game master can send a player to (`GET /api/world/places`): the settlements,
//! outposts, forts and spirit shrines of the level's landmarks, and its expedition, raid and
//! trial entrances, each with its English name and its region.
//!
//! - A landmark's kind is its `eTerritoryLandmarkType` (the sim's travel rules: 1 settlement,
//!   22 outpost, 27 shrine; 2 the fort of a territory, Observed: one per territory, at its war
//!   fort). Its name is the territory definition of its point that is a point of interest, else
//!   an area, else its region (a shrine landmark lies in its shrine's point of interest,
//!   `Church of the Courts`).
//! - An entrance is a level capital whose slice is an instance entrance; its game mode comes
//!   from [`entrance_mode`] (as the sim opens it), its name from the mode's `DisplayName`, else
//!   from the territory definition of that `GameMode` (some trials have no display name). One
//!   place stands for the entrances of one mode within [`SAME_ENTRANCE_M`] (a locked and an open
//!   form of one door).
//! - The region is the territory (`IsTerritory`) of the point; an entrance has no territory list
//!   and takes the region of the nearest landmark that has one.

use std::collections::{BTreeMap, HashMap};

use nw_content::bundle::english;
use nw_content::dungeon_entrances::{entrance_mode, entrance_rest};
use nw_content::instances::{InstanceCatalog, InstanceKind};
use nw_content::{ContentBundle, Tables};
use nw_world::WorldFile;
use nw_world::records::Landmark;
use serde::Serialize;

/// The route of the places.
pub const PLACES_ROUTE: &str = "/api/world/places";
/// Landmark types (`eTerritoryLandmarkType`).
const SETTLEMENT_LANDMARK: u8 = 1;
const FORT_LANDMARK: u8 = 2;
const OUTPOST_LANDMARK: u8 = 22;
const SHRINE_LANDMARK: u8 = 27;
/// Entrances of one game mode closer than this are one place, in metres.
pub const SAME_ENTRANCE_M: f32 = 50.0;
/// The region of a point that no territory holds.
const NO_REGION: &str = "Elsewhere";
/// A fort is named after its region: `<region> fort`.
const FORT_SUFFIX: &str = " fort";

/// What a place is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    /// A settlement (town).
    Settlement,
    /// An outpost.
    Outpost,
    /// A territory's fort.
    Fort,
    /// A spirit shrine.
    Shrine,
    /// An expedition entrance.
    Expedition,
    /// A raid or group trial entrance.
    Raid,
    /// A solo trial entrance.
    Trial,
}

/// A place to send a player to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Place {
    /// What it is.
    pub kind: PlaceKind,
    /// Its English name.
    pub name: String,
    /// The region (territory) it lies in.
    pub region: String,
    /// East, metres.
    pub x: f32,
    /// North, metres.
    pub y: f32,
    /// Up, metres.
    pub z: f32,
}

/// The names and kinds of the level's territory definitions, by territory id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Territories(HashMap<u16, Territory>);

/// One territory definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Territory {
    /// Its English name.
    pub name: String,
    /// A region (`IsTerritory`).
    pub region: bool,
    /// A point of interest (`IsPOI`).
    pub poi: bool,
    /// An area (`IsArea`).
    pub area: bool,
}

impl Territories {
    /// The territories of `tables`, named by `text`; a territory without an English name is left
    /// out.
    pub fn of_tables(tables: &Tables, text: &BTreeMap<String, String>) -> Self {
        Self(
            tables
                .territory_definition
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let id = u16::try_from(row.territory_id).ok()?;
                    let name = english(text, row.name_localization_key.as_deref()?)?;
                    Some((
                        id,
                        Territory {
                            name: name.to_owned(),
                            region: row.is_territory.unwrap_or(false),
                            poi: row.is_poi.unwrap_or(false),
                            area: row.is_area.unwrap_or(false),
                        },
                    ))
                })
                .collect(),
        )
    }

    /// A list of `(id, territory)` pairs.
    pub fn from_list(list: impl IntoIterator<Item = (u16, Territory)>) -> Self {
        Self(list.into_iter().collect())
    }

    fn of_point<'a, 'b>(
        &'a self,
        ids: &'b [u16],
    ) -> impl Iterator<Item = &'a Territory> + use<'a, 'b> {
        ids.iter().filter_map(|id| self.0.get(id))
    }

    fn region(&self, ids: &[u16]) -> Option<&str> {
        self.of_point(ids)
            .find(|territory| territory.region)
            .map(|territory| territory.name.as_str())
    }

    /// The name of a point: its point of interest that is no area, else one that is, else its
    /// area, else its region (an outpost lies in a point of interest of its own inside one that
    /// is also an area: `Grand Carnival` in `The Manorlands`).
    fn name(&self, ids: &[u16]) -> Option<&str> {
        let pick =
            |wanted: fn(&Territory) -> bool| self.of_point(ids).find(|territory| wanted(territory));
        pick(|territory| territory.poi && !territory.area && !territory.region)
            .or_else(|| pick(|territory| territory.poi && !territory.region))
            .or_else(|| pick(|territory| territory.area))
            .or_else(|| pick(|territory| territory.region))
            .map(|territory| territory.name.as_str())
    }
}

/// The places of the level of `bundle`.
pub fn places_of_bundle(world: &WorldFile, bundle: &ContentBundle) -> Vec<Place> {
    let territories = Territories::of_tables(&bundle.tables, &bundle.text);
    let catalog = InstanceCatalog::from_tables(&bundle.tables);
    let display_names: HashMap<&str, &str> = bundle
        .tables
        .game_mode_data
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let name = english(&bundle.text, row.display_name.as_deref()?)?;
            Some((row.game_mode_id.as_str(), name))
        })
        .collect();
    let territory_names: HashMap<&str, &str> = bundle
        .tables
        .territory_definition
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            let name = english(&bundle.text, row.name_localization_key.as_deref()?)?;
            Some((row.game_mode.as_deref()?, name))
        })
        .collect();
    places_of(world, &territories, |slice| {
        let mode = entrance_mode(catalog.modes(), slice)?;
        let kind = match mode.kind {
            InstanceKind::Expedition | InstanceKind::Catacombs => PlaceKind::Expedition,
            InstanceKind::RaidTrial => PlaceKind::Raid,
            InstanceKind::SoloTrial => PlaceKind::Trial,
            InstanceKind::Arena | InstanceKind::OutpostRush | InstanceKind::CaptureTheFlag => {
                return None;
            }
        };
        let name = display_names
            .get(mode.id.as_str())
            .or_else(|| territory_names.get(mode.id.as_str()))
            .map_or_else(|| mode.id.clone(), |name| (*name).to_owned());
        Some(Entrance {
            mode: mode.id.clone(),
            name,
            kind,
        })
    })
}

/// The game mode an entrance slice opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entrance {
    /// The game mode id.
    pub mode: String,
    /// The mode's English name.
    pub name: String,
    /// What it is.
    pub kind: PlaceKind,
}

/// The places of `world`: its landmark places named by `territories`, and its entrances that
/// `entrance_of` gives a game mode (by the capital's slice name), sorted by region, kind and
/// name.
pub fn places_of(
    world: &WorldFile,
    territories: &Territories,
    entrance_of: impl Fn(&str) -> Option<Entrance>,
) -> Vec<Place> {
    let mut places: Vec<Place> = world
        .landmarks
        .iter()
        .filter_map(|landmark| landmark_place(landmark, territories))
        .collect();
    let mut entrances: Vec<(String, Place)> = Vec::new();
    for capital in &world.capitals {
        let file = capital
            .slice_name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default();
        if entrance_rest(&file.to_lowercase()).is_none() {
            continue;
        }
        let Some(entrance) = entrance_of(&capital.slice_name) else {
            continue;
        };
        let [x, y, z] = capital.position;
        let near = |(mode, place): &(String, Place)| {
            *mode == entrance.mode && (place.x - x).hypot(place.y - y) < SAME_ENTRANCE_M
        };
        if entrances.iter().any(near) {
            continue;
        }
        let region = nearest_region(world, territories, x, y).unwrap_or(NO_REGION);
        entrances.push((
            entrance.mode,
            Place {
                kind: entrance.kind,
                name: entrance.name,
                region: region.to_owned(),
                x,
                y,
                z,
            },
        ));
    }
    places.extend(entrances.into_iter().map(|(_, place)| place));
    places.sort_by(|a, b| (&a.region, a.kind, &a.name).cmp(&(&b.region, b.kind, &b.name)));
    places
}

fn landmark_place(landmark: &Landmark, territories: &Territories) -> Option<Place> {
    let kind = match landmark.landmark_type {
        SETTLEMENT_LANDMARK => PlaceKind::Settlement,
        OUTPOST_LANDMARK => PlaceKind::Outpost,
        FORT_LANDMARK => PlaceKind::Fort,
        SHRINE_LANDMARK => PlaceKind::Shrine,
        _ => return None,
    };
    let ids = &landmark.territory_ids;
    let region = territories.region(ids).unwrap_or(NO_REGION);
    let name = if kind == PlaceKind::Fort {
        format!("{region}{FORT_SUFFIX}")
    } else {
        territories.name(ids)?.to_owned()
    };
    let [x, y, z] = landmark.position;
    Some(Place {
        kind,
        name,
        region: region.to_owned(),
        x,
        y,
        z,
    })
}

/// The region of the landmark nearest to (`x`, `y`) that lies in one.
fn nearest_region<'a>(
    world: &WorldFile,
    territories: &'a Territories,
    x: f32,
    y: f32,
) -> Option<&'a str> {
    world
        .landmarks
        .iter()
        .filter_map(|landmark| {
            let region = territories.region(&landmark.territory_ids)?;
            let [lx, ly, _] = landmark.position;
            Some(((lx - x).hypot(ly - y), region))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, region)| region)
}

#[cfg(test)]
mod tests {
    use nw_world::records::Capital;

    use super::*;

    const REGION: u16 = 12;
    const OTHER_REGION: u16 = 4;
    const AREA: u16 = 9010;
    const TOWN: u16 = 30155;
    const SHRINE_POI: u16 = 10228;
    const OUTPOST_AREA: u16 = 9210;
    const OUTPOST_POI: u16 = 10330;

    fn territory(name: &str, region: bool, poi: bool, area: bool) -> Territory {
        Territory {
            name: name.to_owned(),
            region,
            poi,
            area,
        }
    }

    fn territories() -> Territories {
        Territories::from_list([
            (REGION, territory("Monarch's Bluffs", true, false, false)),
            (OTHER_REGION, territory("Everfall", true, false, false)),
            (AREA, territory("Devil's Quarry", false, false, true)),
            (TOWN, territory("Prydwen", false, true, false)),
            (
                SHRINE_POI,
                territory("Church of the Courts", false, true, false),
            ),
            (OUTPOST_AREA, territory("The Manorlands", false, true, true)),
            (OUTPOST_POI, territory("Grand Carnival", false, true, false)),
        ])
    }

    fn landmark(landmark_type: u8, position: [f32; 3], ids: &[u16]) -> Landmark {
        Landmark {
            landmark_type,
            position,
            territory_ids: ids.to_vec(),
            ..Landmark::default()
        }
    }

    fn capital(slice: &str, position: [f32; 3]) -> Capital {
        Capital {
            slice_name: slice.to_owned(),
            position,
            ..Capital::default()
        }
    }

    fn amrine(slice: &str) -> Option<Entrance> {
        slice.to_lowercase().contains("amrine").then(|| Entrance {
            mode: "DungeonAmrine".to_owned(),
            name: "The Amrine Excavation".to_owned(),
            kind: PlaceKind::Expedition,
        })
    }

    #[test]
    fn landmarks_give_named_places_in_their_region() {
        let world = WorldFile {
            landmarks: vec![
                landmark(
                    SETTLEMENT_LANDMARK,
                    [7348.0, 3740.0, 104.0],
                    &[REGION, TOWN, AREA],
                ),
                landmark(
                    SHRINE_LANDMARK,
                    [3991.0, 5604.0, 119.0],
                    &[REGION, AREA, SHRINE_POI],
                ),
                landmark(FORT_LANDMARK, [6304.0, 3350.0, 60.0], &[REGION, AREA]),
                landmark(OUTPOST_LANDMARK, [1.0, 2.0, 3.0], &[]),
                landmark(
                    OUTPOST_LANDMARK,
                    [3845.0, 6931.0, 159.0],
                    &[REGION, OUTPOST_AREA, OUTPOST_POI],
                ),
                landmark(SHRINE_LANDMARK, [5.0, 5.0, 5.0], &[AREA]),
                landmark(20, [0.0, 0.0, 0.0], &[REGION, TOWN]),
            ],
            ..WorldFile::default()
        };
        let places = places_of(&world, &territories(), |_| None);
        let names: Vec<(&str, PlaceKind, &str)> = places
            .iter()
            .map(|place| (place.region.as_str(), place.kind, place.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("Elsewhere", PlaceKind::Shrine, "Devil's Quarry"),
                ("Monarch's Bluffs", PlaceKind::Settlement, "Prydwen"),
                ("Monarch's Bluffs", PlaceKind::Outpost, "Grand Carnival"),
                ("Monarch's Bluffs", PlaceKind::Fort, "Monarch's Bluffs fort"),
                (
                    "Monarch's Bluffs",
                    PlaceKind::Shrine,
                    "Church of the Courts"
                ),
            ],
            "the outpost of no territory has no name and the inn is no place"
        );
        assert_eq!(
            [places[1].x, places[1].y, places[1].z],
            [7348.0, 3740.0, 104.0]
        );
    }

    #[test]
    fn an_entrance_is_one_place_per_door_in_the_nearest_landmarks_region() {
        let world = WorldFile {
            landmarks: vec![
                landmark(
                    SHRINE_LANDMARK,
                    [9300.0, 3300.0, 90.0],
                    &[OTHER_REGION, SHRINE_POI],
                ),
                landmark(SHRINE_LANDMARK, [100.0, 100.0, 90.0], &[REGION, SHRINE_POI]),
            ],
            capitals: vec![
                capital(
                    "dungeon/entrance/dungeonentrance_amrine",
                    [9306.0, 3214.0, 90.0],
                ),
                capital(
                    "Dungeon/Entrance/DungeonEntrance_Amrine_Locked",
                    [9310.0, 3220.0, 90.0],
                ),
                capital(
                    "Dungeon/Entrance/DungeonEntrance_Amrine",
                    [100.0, 300.0, 50.0],
                ),
                capital(
                    "Dungeon/Entrance/DungeonEntrance_FirstLight02",
                    [5.0, 5.0, 5.0],
                ),
                capital("POIs/Settlement/Townboard_Amrine", [6.0, 6.0, 6.0]),
            ],
            ..WorldFile::default()
        };
        let places = places_of(&world, &territories(), amrine);
        let entrances: Vec<(&str, f32)> = places
            .iter()
            .filter(|place| place.kind == PlaceKind::Expedition)
            .map(|place| (place.region.as_str(), place.x))
            .collect();
        assert_eq!(
            entrances,
            [("Everfall", 9306.0), ("Monarch's Bluffs", 100.0)],
            "the locked form of the first door is the same place; a far door is its own; a \
             slice that is no entrance and one with no mode are no place"
        );
    }
}
