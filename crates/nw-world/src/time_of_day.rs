// SPDX-License-Identifier: AGPL-3.0-or-later
//! The time of day of the slices (time-of-day spawns study 2026-11-12): an entity with a
//! `Javelin::Spawner::TimeOfDayConstraintComponent` exists only in the day phases that its
//! `m_night`, `m_dawn`, `m_day` and `m_dusk` name, and goes when its time ends when its
//! `m_despawnOnTimeEnd` is set (the lower-case CRC-32 of those client strings).
//!
//! Observed (build 25810808): 606 such components in 555 slices, each with
//! `m_despawnOnTimeEnd` set. The client registers the component with a server facet and a client
//! facet (`TimeOfDayConstraintComponentServerFacet`, `TimeOfDayConstraintComponentClientFacet`,
//! `FUN_143c605e0` and `FUN_143c60380`) as a `SpawnerConstraintService` (`FUN_143c6aff0`): it
//! holds back the spawner on its entity. In `newworld_vitaeeterna` the holders stand:
//! - on 474 level vitals spawns (the point spawners' entity on the capital's place, 0.00 m): the
//!   mobs of those spawns;
//! - on 114 level gatherable points (the `Gatherable` entity of the 54
//!   `17dw_gatherables_32x32_static_chunk_*` slices the region chunk lists place, 0.00 m; all
//!   `DyePlantBlackWhite`, night only);
//! - in encounter slices that an open world spawn group spawns (`enc_worldevent_*_walshamhaunt`,
//!   `_perivillehaunt`, the khepri elite hunt, the tarot tower);
//! - on props and spawners the client loads with the level and the server does not run (pirate
//!   flags by day or night, glyph chest force fields, the lucky rabbit spawner of the fast travel
//!   shrines, spawn group masters of no open world encounter).

use nw_formats::objectstream::{Element, lowercase_crc32};
use serde::{Deserialize, Serialize};

use crate::records::{Capital, Position};
use crate::travel_slices::{
    AXES, SliceEntities, TRANSLATION_AT, UUID_LEN, capital_point, descendants, uuid,
};

/// `Javelin::Spawner::TimeOfDayConstraintComponent` (`{E949F549-71A8-4914-861C-20A2CA6B75C4}`).
pub const TIME_OF_DAY_CONSTRAINT: [u8; UUID_LEN] = uuid(0xe949_f549_71a8_4914_861c_20a2_ca6b_75c4);
/// The largest difference on each axis between a holder entity and the level point it stands
/// on, metres (Observed: 0.00 m for every matched spawn and gatherable; a float's rounding).
pub const ON_POINT_M: f32 = 0.01;

/// The day phases in which an entity exists, and whether it goes when they end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeOfDay {
    /// `m_night`.
    pub night: bool,
    /// `m_dawn`.
    pub dawn: bool,
    /// `m_day`.
    pub day: bool,
    /// `m_dusk`.
    pub dusk: bool,
    /// `m_despawnOnTimeEnd`.
    pub despawn_on_end: bool,
}

/// The names read, as their name CRCs.
struct Names {
    night: u32,
    dawn: u32,
    day: u32,
    dusk: u32,
    despawn: u32,
}

impl Names {
    fn new() -> Self {
        Self {
            night: lowercase_crc32("m_night"),
            dawn: lowercase_crc32("m_dawn"),
            day: lowercase_crc32("m_day"),
            dusk: lowercase_crc32("m_dusk"),
            despawn: lowercase_crc32("m_despawnOnTimeEnd"),
        }
    }
}

impl TimeOfDay {
    /// The flags of a `TimeOfDayConstraintComponent` element; a missing flag is off.
    fn of(component: &Element) -> Self {
        let names = Names::new();
        let all = descendants(component);
        let flag = |name: u32| {
            all.iter()
                .find(|element| element.name_crc == Some(name))
                .is_some_and(|element| element.as_bool().unwrap_or(false))
        };
        Self {
            night: flag(names.night),
            dawn: flag(names.dawn),
            day: flag(names.day),
            dusk: flag(names.dusk),
            despawn_on_end: flag(names.despawn),
        }
    }
}

/// One entity of a slice that holds a time of day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimedHolder {
    /// Where it stands in the slice's frame: its `m_localTM` up its `m_parentId` chain.
    pub at: [f32; AXES],
    /// Its time of day.
    pub window: TimeOfDay,
}

/// The entities of a dynamic slice that hold a time of day.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimedSlice {
    holders: Vec<TimedHolder>,
}

impl TimedSlice {
    /// Whether the decoded ObjectStream `stream` names the component at all: a cheap test before
    /// a slice is parsed.
    pub fn may_hold(stream: &[u8]) -> bool {
        stream
            .windows(UUID_LEN)
            .any(|bytes| bytes == TIME_OF_DAY_CONSTRAINT)
    }

    /// The holders of the dynamic slice whose root element is `root`; `None` for a slice without
    /// the component.
    pub fn of(root: &Element) -> Option<Self> {
        let all = descendants(root);
        let entities = SliceEntities::of(&all);
        let holders: Vec<TimedHolder> = entities
            .all()
            .filter_map(|(id, entity)| {
                let component = entity
                    .children
                    .iter()
                    .flat_map(|child| std::iter::once(child).chain(&child.children))
                    .find(|element| element.type_id == TIME_OF_DAY_CONSTRAINT)?;
                let placed = entities.placed(id);
                Some(TimedHolder {
                    at: std::array::from_fn(|axis| placed[TRANSLATION_AT + axis]),
                    window: TimeOfDay::of(component),
                })
            })
            .collect();
        (!holders.is_empty()).then_some(Self { holders })
    }

    /// The time of day of the slice: its first holder's (an encounter slice holds one).
    pub fn window(&self) -> Option<TimeOfDay> {
        self.holders.first().map(|holder| holder.window)
    }

    /// The holders of this slice placed by `capital`, in world space.
    pub fn at_capital(&self, capital: &Capital) -> Vec<TimedHolder> {
        self.holders
            .iter()
            .map(|holder| TimedHolder {
                at: capital_point(capital, holder.at),
                window: holder.window,
            })
            .collect()
    }

    /// The holders of this slice placed by a chunk list at `position` (a chunk list places its
    /// slices without a rotation, `content-tool collision`).
    pub fn at_chunk(&self, position: Position) -> Vec<TimedHolder> {
        self.holders
            .iter()
            .map(|holder| TimedHolder {
                at: std::array::from_fn(|axis| position[axis] + holder.at[axis]),
                window: holder.window,
            })
            .collect()
    }
}

/// A level point (a vitals spawn, a gatherable point) with a time of day, by its index in the
/// world file's list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedPoint {
    /// Its index in the list.
    pub index: u32,
    /// Its time of day.
    pub window: TimeOfDay,
}

/// A slice with a time of day, by its pak path: an encounter slice that a spawn group spawns
/// has no place in the level files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedSlicePath {
    /// The pak path of its `.dynamicslice`, lower case.
    pub slice: String,
    /// Its time of day.
    pub window: TimeOfDay,
}

/// The time of day of a level's points and of the slices with one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeOfDayPoints {
    /// The vitals spawns with a time of day, by index in `WorldFile::vitals`.
    pub vitals: Vec<TimedPoint>,
    /// The gatherable points with a time of day, by index in `WorldFile::gatherables`.
    pub gatherables: Vec<TimedPoint>,
    /// Every slice with a time of day, by pak path.
    pub slices: Vec<TimedSlicePath>,
}

/// The points of `points` (in list order) on which a holder of `holders` stands, within
/// [`ON_POINT_M`] on each axis; a point under two holders takes the first one's time of day.
pub fn points_under(
    holders: &[TimedHolder],
    points: impl IntoIterator<Item = Position>,
) -> Vec<TimedPoint> {
    (0_u32..)
        .zip(points)
        .filter_map(|(index, point)| {
            let holder = holders.iter().find(|holder| {
                holder
                    .at
                    .iter()
                    .zip(point)
                    .all(|(a, b)| (a - b).abs() <= ON_POINT_M)
            })?;
            Some(TimedPoint {
                index,
                window: holder.window,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::travel_slices::{ENTITY, IDENTITY, Transform};

    /// The flags of the retail Dunwood dye plant chunks: night only, gone when the night ends.
    const NIGHT_ONLY: TimeOfDay = TimeOfDay {
        night: true,
        dawn: false,
        day: false,
        dusk: false,
        despawn_on_end: true,
    };
    const HOLDER_ID: u64 = 7;
    const PARENT_ID: u64 = 3;
    /// The retail chunk `17dw_gatherables_32x32_static_chunk_11_21_3`'s `Gatherable` entity, in
    /// its slice's frame.
    const HOLDER_AT: [f32; AXES] = [-13.88, 1.47, 0.0];

    fn named(name: &str, value: Vec<u8>) -> Element {
        Element::with_value(lowercase_crc32(name), value)
    }

    fn typed(type_id: [u8; UUID_LEN], children: Vec<Element>) -> Element {
        Element {
            type_id,
            ..Element::with_children(0, children)
        }
    }

    fn entity(id: u64, parent: Option<u64>, at: [f32; AXES], components: Vec<Element>) -> Element {
        let mut local: Transform = IDENTITY;
        local[TRANSLATION_AT..].copy_from_slice(&at);
        let bytes = local.iter().flat_map(|value| value.to_be_bytes()).collect();
        let mut transform = vec![named("m_localTM", bytes)];
        if let Some(parent) = parent {
            transform.push(Element::with_children(
                lowercase_crc32("m_parentId"),
                vec![named("Id", parent.to_be_bytes().to_vec())],
            ));
        }
        let mut children = vec![
            Element::with_children(
                lowercase_crc32("Id"),
                vec![named("Id", id.to_be_bytes().to_vec())],
            ),
            Element::with_children(0, transform),
        ];
        children.extend(components);
        typed(ENTITY, children)
    }

    /// A component as the retail slices hold it: the flags under a base class element.
    fn constraint(window: TimeOfDay) -> Element {
        let flag = |name: &str, on: bool| named(name, vec![u8::from(on)]);
        typed(
            TIME_OF_DAY_CONSTRAINT,
            vec![Element::with_children(
                0,
                vec![
                    flag("m_night", window.night),
                    flag("m_dawn", window.dawn),
                    flag("m_day", window.day),
                    flag("m_dusk", window.dusk),
                    flag("m_despawnOnTimeEnd", window.despawn_on_end),
                ],
            )],
        )
    }

    fn chunk() -> Element {
        Element::with_children(
            0,
            vec![
                entity(PARENT_ID, None, [1.0, 2.0, 0.0], Vec::new()),
                entity(
                    HOLDER_ID,
                    Some(PARENT_ID),
                    HOLDER_AT,
                    vec![Element::with_children(0, vec![constraint(NIGHT_ONLY)])],
                ),
            ],
        )
    }

    #[test]
    fn a_holder_gives_its_flags_and_its_place_up_its_parents() {
        let slice = TimedSlice::of(&chunk()).expect("a holder");
        assert_eq!(slice.window(), Some(NIGHT_ONLY));
        let [holder] = slice.at_chunk([100.0, 200.0, 50.0])[..] else {
            panic!("one holder");
        };
        let expected = [100.0 + 1.0 - 13.88, 200.0 + 2.0 + 1.47, 50.0];
        assert!(
            holder
                .at
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-3),
            "{holder:?}"
        );
        assert_eq!(TimedSlice::of(&Element::with_children(0, Vec::new())), None);
    }

    #[test]
    fn the_cheap_test_finds_the_component_bytes() {
        let mut stream = vec![0; 4];
        stream.extend(TIME_OF_DAY_CONSTRAINT);
        assert!(TimedSlice::may_hold(&stream));
        assert!(!TimedSlice::may_hold(&[0; UUID_LEN]));
    }

    #[test]
    fn a_capital_places_its_holders_by_its_rotation_and_scale() {
        const HALF_TURN: [f32; 4] = [0.0, 0.0, 1.0, 0.0];
        let slice = TimedSlice::of(&Element::with_children(
            0,
            vec![entity(
                HOLDER_ID,
                None,
                [2.0, 0.0, 0.0],
                vec![constraint(NIGHT_ONLY)],
            )],
        ))
        .expect("a holder");
        let capital = Capital {
            position: [10.0, 10.0, 0.0],
            rotation: HALF_TURN,
            scale: Some(2.0),
            ..Capital::default()
        };
        let [holder] = slice.at_capital(&capital)[..] else {
            panic!("one holder");
        };
        assert!((holder.at[0] - 6.0).abs() < 1e-4, "{holder:?}");
        assert!((holder.at[1] - 10.0).abs() < 1e-4, "{holder:?}");
    }

    #[test]
    fn only_points_under_a_holder_take_its_time() {
        let holders = [TimedHolder {
            at: [5.0, 5.0, 5.0],
            window: NIGHT_ONLY,
        }];
        let points = [[0.0, 0.0, 0.0], [5.0, 5.005, 5.0], [5.0, 5.5, 5.0]];
        assert_eq!(
            points_under(&holders, points),
            [TimedPoint {
                index: 1,
                window: NIGHT_ONLY
            }]
        );
    }
}
