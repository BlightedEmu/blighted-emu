// SPDX-License-Identifier: AGPL-3.0-or-later
//! The waypoints of a spawner capital's slice (`PointSpawner_Boar_Graze_*`,
//! `PointSpawner_Boar_4wp_Patrol_*`): entities `wp1`, `wp2`, ... with an `AIWaypointComponent`
//! whose server facet names the task its mob plays there. The level's vitals spawn of the
//! spawner stands on the capital's position (Observed: 0.00 m on the beach graze and patrol
//! spawners of `newworld_vitaeeterna`), and retail boars played the tasks (fragment
//! `POI_Graze_LOOP` with action `POI_Waypoint_Loop`, newchar 2026-09-18).

use nw_formats::objectstream::{Element, lowercase_crc32};

use crate::records::{Capital, PathMode, PatrolPath, WaypointTask};
use crate::travel_slices::{AXES, SliceEntities, TRANSLATION_AT, capital_point, descendants};

/// The name of a waypoint entity: this prefix, then its number on the path.
const WAYPOINT_PREFIX: &str = "wp";
/// The duration of a waypoint task whose slice names none: the slices' own "none" (`-1`).
const NO_DURATION_S: f32 = -1.0;

/// One waypoint of a spawner slice, in the slice's frame.
#[derive(Debug, Clone, PartialEq)]
struct SliceWaypoint {
    number: u32,
    at: [f32; AXES],
    /// Its task; `None` for a waypoint with an empty task name (a point to walk through).
    task: Option<WaypointTask>,
}

/// The waypoints of a spawner's dynamic slice, in path order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpawnerSlice {
    waypoints: Vec<SliceWaypoint>,
}

impl SpawnerSlice {
    /// The waypoints of the dynamic slice whose root element is `root`; `None` for a slice
    /// without a waypoint entity.
    pub fn of(root: &Element) -> Option<Self> {
        let all = descendants(root);
        let entities = SliceEntities::of(&all);
        let name = lowercase_crc32("Name");
        let task_name = lowercase_crc32("m_taskName");
        let min = lowercase_crc32("m_minDuration");
        let max = lowercase_crc32("m_maxDuration");
        let align = lowercase_crc32("m_matchAlignment");
        let mut waypoints: Vec<SliceWaypoint> = entities
            .all()
            .filter_map(|(id, entity)| {
                let number = entity
                    .child(name)?
                    .as_string()
                    .ok()?
                    .strip_prefix(WAYPOINT_PREFIX)?
                    .parse()
                    .ok()?;
                let facet = descendants(entity)
                    .into_iter()
                    .find(|element| element.child(task_name).is_some())?;
                let float = |crc| facet.child(crc).and_then(|value| value.as_f32().ok());
                let placed = entities.placed(id);
                let task = facet.child(task_name)?.as_string().ok()?;
                Some(SliceWaypoint {
                    number,
                    at: std::array::from_fn(|axis| placed[TRANSLATION_AT + axis]),
                    task: (!task.is_empty()).then(|| WaypointTask {
                        task: task.to_owned(),
                        duration_s: [
                            float(min).unwrap_or(NO_DURATION_S),
                            float(max).unwrap_or(NO_DURATION_S),
                        ],
                        align: facet
                            .child(align)
                            .and_then(|value| value.as_bool().ok())
                            .unwrap_or(false),
                    }),
                })
            })
            .collect();
        waypoints.sort_by_key(|waypoint| waypoint.number);
        (!waypoints.is_empty()).then_some(Self { waypoints })
    }

    /// The path this slice's waypoints make at `capital`: a loop through them in number order
    /// (Inferred: the Patrol sub-tree's `SelectWaypointAction` goes on from the last to the
    /// first).
    pub fn at(&self, capital: &Capital) -> PatrolPath {
        PatrolPath {
            points: self
                .waypoints
                .iter()
                .map(|waypoint| capital_point(capital, waypoint.at))
                .collect(),
            waits_ms: Vec::new(),
            mode: PathMode::Loop,
            tasks: self
                .waypoints
                .iter()
                .map(|waypoint| waypoint.task.clone())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::travel_slices::{ENTITY, IDENTITY, Transform, UUID_LEN};

    fn named(name: &str, value: Vec<u8>) -> Element {
        Element::with_value(lowercase_crc32(name), value)
    }

    fn typed(type_id: [u8; UUID_LEN], children: Vec<Element>) -> Element {
        Element {
            type_id,
            ..Element::with_children(0, children)
        }
    }

    /// A waypoint entity `name` at `at` with task `task` of `min` to `max` seconds.
    fn waypoint(id: u64, name: &str, at: [f32; AXES], task: &str, min: f32, max: f32) -> Element {
        let mut local: Transform = IDENTITY;
        local[TRANSLATION_AT..].copy_from_slice(&at);
        let bytes = local.iter().flat_map(|value| value.to_be_bytes()).collect();
        typed(
            ENTITY,
            vec![
                Element::with_children(
                    lowercase_crc32("Id"),
                    vec![named("Id", id.to_be_bytes().to_vec())],
                ),
                named("Name", name.as_bytes().to_vec()),
                Element::with_children(0, vec![named("m_localTM", bytes)]),
                Element::with_children(
                    0,
                    vec![
                        named("m_taskName", task.as_bytes().to_vec()),
                        named("m_minDuration", min.to_be_bytes().to_vec()),
                        named("m_maxDuration", max.to_be_bytes().to_vec()),
                        named("m_matchAlignment", vec![0]),
                    ],
                ),
            ],
        )
    }

    #[test]
    fn a_spawners_waypoints_make_its_path_in_number_order() {
        // The retail beach patrol spawner's wp2 and wp1 (`PointSpawner_Boar_4wp_Patrol`).
        let root = Element::with_children(
            0,
            vec![
                waypoint(2, "wp2", [-3.6, -1.4, 1.3], "POI_Graze_LOOP", 15.0, 45.0),
                waypoint(1, "wp1", [-2.7, 4.5, -0.3], "POI_Wallow_LOOP", 3.0, 5.0),
            ],
        );
        let slice = SpawnerSlice::of(&root).expect("waypoints");
        let capital = Capital {
            position: [10.0, 20.0, 30.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            ..Capital::default()
        };
        let path = slice.at(&capital);
        assert_eq!(path.points, [[7.3, 24.5, 29.7], [6.4, 18.6, 31.3]]);
        let tasks: Vec<&str> = path
            .tasks
            .iter()
            .flatten()
            .map(|task| task.task.as_str())
            .collect();
        assert_eq!(tasks, ["POI_Wallow_LOOP", "POI_Graze_LOOP"]);
        assert_eq!(
            path.tasks[1].as_ref().map(|task| task.duration_s),
            Some([15.0, 45.0])
        );
        assert_eq!(
            SpawnerSlice::of(&Element::with_children(0, Vec::new())),
            None
        );
        let walk_through =
            Element::with_children(0, vec![waypoint(1, "wp1", [0.0; AXES], "", -1.0, -1.0)]);
        let path = SpawnerSlice::of(&walk_through)
            .expect("waypoint")
            .at(&capital);
        assert_eq!(path.tasks, [None], "an empty task name is no task");
    }
}
