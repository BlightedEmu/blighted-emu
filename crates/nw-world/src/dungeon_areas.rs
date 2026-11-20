// SPDX-License-Identifier: AGPL-3.0-or-later
//! The dungeon areas of a level: the territory definitions with a `GameMode` (the area of an
//! expedition, raid or trial entrance, such as `The Ennead` of `DungeonBrimstoneSands00`) and
//! the level's points that lie in them.
//!
//! A level capital has no territory list. A door stands in the area of the nearest point within
//! [`AREA_REACH_M`] that lies in a dungeon area ([`nw_content::dungeon_entrances::entrance_mode`]
//! opens that area's mode when the door's name names none).

use std::collections::HashMap;

use nw_content::Tables;

use crate::WorldFile;
use crate::records::Position;

/// A door this close to a point of a dungeon area stands in it, in metres (Inferred from the
/// open world: the two entrances that no name names stand 9 and 10 m from a point of their
/// area; the named `DungeonEntrance_CutlassKeys` is 117 m from the nearest point of another
/// mode's area).
pub const AREA_REACH_M: f32 = 25.0;

/// The points of a level that lie in a dungeon area, each with the area's game mode id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DungeonAreas {
    points: Vec<([f32; 2], String)>,
}

impl DungeonAreas {
    /// The dungeon areas of the territory definitions of `tables` at the points of `world`.
    pub fn of_level(world: &WorldFile, tables: &Tables) -> Self {
        let area_modes: HashMap<u16, &str> = tables
            .territory_definition
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    u16::try_from(row.territory_id).ok()?,
                    row.game_mode.as_deref()?,
                ))
            })
            .collect();
        Self::new(&area_modes, territory_points(world))
    }

    /// The points among `points` (position, territory ids) that lie in an area of `area_modes`
    /// (territory id to game mode id).
    pub fn new<'a>(
        area_modes: &HashMap<u16, &str>,
        points: impl IntoIterator<Item = (&'a Position, &'a [u16])>,
    ) -> Self {
        let points = points
            .into_iter()
            .filter_map(|([x, y, _], ids)| {
                let mode = ids.iter().find_map(|id| area_modes.get(id))?;
                Some(([*x, *y], (*mode).to_owned()))
            })
            .collect();
        Self { points }
    }

    /// The game mode id of the dungeon area that holds `at`: the area of the nearest of its
    /// points within [`AREA_REACH_M`] on the ground plane.
    pub fn mode_at(&self, at: Position) -> Option<&str> {
        let [x, y, _] = at;
        self.points
            .iter()
            .map(|([px, py], mode)| ((px - x).hypot(py - y), mode))
            .filter(|(distance, _)| *distance <= AREA_REACH_M)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, mode)| mode.as_str())
    }
}

/// Every point of `world` that carries the territory ids that contain it.
fn territory_points(world: &WorldFile) -> impl Iterator<Item = (&Position, &[u16])> {
    let npcs = world
        .npcs
        .iter()
        .map(|point| (&point.position, point.territory_ids.as_slice()));
    let vitals = world
        .vitals
        .iter()
        .map(|point| (&point.position, point.territory_ids.as_slice()));
    let gatherables = world
        .gatherables
        .iter()
        .map(|point| (&point.position, point.territory_ids.as_slice()));
    let lore = world
        .lore
        .iter()
        .map(|point| (&point.position, point.territory_ids.as_slice()));
    let landmarks = world
        .landmarks
        .iter()
        .map(|point| (&point.position, point.territory_ids.as_slice()));
    let encounters = world
        .encounters
        .iter()
        .map(|point| (&point.position, point.territory_ids.as_slice()));
    npcs.chain(vitals)
        .chain(gatherables)
        .chain(lore)
        .chain(landmarks)
        .chain(encounters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{LorePoint, VitalsSpawn};

    /// The territory id of The Ennead's area (`TerritoryDefinition` 20413).
    const ENNEAD_AREA: u16 = 20413;
    /// The territory id of the Hive of Gorgons' area (30072).
    const HIVE_AREA: u16 = 30072;
    /// A territory with no game mode.
    const REGION: u16 = 2;

    fn world() -> WorldFile {
        WorldFile {
            vitals: vec![VitalsSpawn {
                position: [5913.0, 9038.0, 180.0],
                territory_ids: vec![REGION, ENNEAD_AREA],
                ..VitalsSpawn::default()
            }],
            lore: vec![
                LorePoint {
                    position: [8240.0, 410.0, 45.0],
                    territory_ids: vec![HIVE_AREA],
                    ..LorePoint::default()
                },
                LorePoint {
                    position: [8233.0, 300.0, 45.0],
                    territory_ids: vec![REGION],
                    ..LorePoint::default()
                },
            ],
            ..WorldFile::default()
        }
    }

    fn areas() -> DungeonAreas {
        let modes = HashMap::from([
            (ENNEAD_AREA, "DungeonBrimstoneSands00"),
            (HIVE_AREA, "RaidCutlassKeys00"),
        ]);
        DungeonAreas::new(&modes, territory_points(&world()))
    }

    #[test]
    fn a_door_stands_in_the_area_of_the_nearest_area_point() {
        let areas = areas();
        assert_eq!(
            areas.mode_at([5913.4, 9028.8, 179.9]),
            Some("DungeonBrimstoneSands00")
        );
        assert_eq!(
            areas.mode_at([8232.9, 409.4, 45.0]),
            Some("RaidCutlassKeys00")
        );
    }

    #[test]
    fn a_door_far_from_every_area_point_stands_in_none() {
        let areas = areas();
        assert_eq!(
            areas.mode_at([8185.0, 530.4, 40.3]),
            None,
            "117 m from the area"
        );
        assert_eq!(
            areas.mode_at([8233.0, 300.0, 45.0]),
            None,
            "a point of no dungeon area"
        );
    }
}
