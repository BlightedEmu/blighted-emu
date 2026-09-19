// SPDX-License-Identifier: AGPL-3.0-or-later
//! The script and event design files (admin studio spec sections 7.5, 8.3): their TOML form,
//! their checks, and the design data the merge writes for the sim.
//!
//! A script file holds `[[nodes]]` (an `id`, a `block`, the canvas place `x` and `y`, and the
//! block's values) and `[[links]]` (`from`, `output`, `to`). An event file holds `[[waves]]`
//! (`vitals`, `level`, `count`, `delay_ms`, `offset`), `end_after_ms` and `end_when_cleared`.

use nw_world::script::{
    DesignData, EventWave, GameEvent, LevelDesign, ScriptGraph, ScriptLink, ScriptNode, ScriptValue,
};

use crate::error::{DesignError, Problem};
use crate::file::{DesignFile, DesignSet, Kind};
use crate::schema::{MAX_LEVEL, MIN_LEVEL, position_of};

/// The node keys that are not block values.
const NODE_ID: &str = "id";
const NODE_BLOCK: &str = "block";
const NODE_X: &str = "x";
const NODE_Y: &str = "y";
/// The link keys.
const LINK_FROM: &str = "from";
const LINK_OUTPUT: &str = "output";
const LINK_TO: &str = "to";
/// The fields of the two kinds.
pub(crate) const NODES: &str = "nodes";
pub(crate) const LINKS: &str = "links";
pub(crate) const WAVES: &str = "waves";
const END_AFTER: &str = "end_after_ms";
const END_WHEN_CLEARED: &str = "end_when_cleared";
/// The wave keys.
const WAVE_VITALS: &str = "vitals";
const WAVE_LEVEL: &str = "level";
const WAVE_COUNT: &str = "count";
const WAVE_DELAY: &str = "delay_ms";
const WAVE_OFFSET: &str = "offset";
/// Most mobs of one wave (the sim's GM spawn limit).
const MAX_WAVE_COUNT: i64 = 50;
/// The blocks whose `event` value names an event of the same level.
const EVENT_BLOCKS: [&str; 2] = ["event_starts", "start_event"];
const EVENT_VALUE: &str = "event";

fn wrong(what: &str) -> Problem {
    Problem::WrongType(what.to_owned())
}

fn tables<'a>(
    file: &'a DesignFile,
    field: &str,
) -> Result<impl Iterator<Item = &'a toml::Table>, Problem> {
    let list = match file.fields.get(field) {
        Some(value) => value
            .as_array()
            .ok_or_else(|| wrong("a list of tables"))?
            .as_slice(),
        None => &[],
    };
    if list.iter().all(toml::Value::is_table) {
        Ok(list.iter().filter_map(toml::Value::as_table))
    } else {
        Err(wrong("a list of tables"))
    }
}

fn text(table: &toml::Table, key: &str) -> Result<String, Problem> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| wrong(&format!("{key} as text")))
}

/// The value of a node key; `None` for a type a block value cannot have.
fn script_value(value: &toml::Value) -> Option<ScriptValue> {
    match value {
        toml::Value::Integer(number) => Some(ScriptValue::Number(*number as f64)),
        toml::Value::Float(number) => Some(ScriptValue::Number(*number)),
        toml::Value::String(text) => Some(ScriptValue::Text(text.clone())),
        toml::Value::Array(_) => position_of(value).map(ScriptValue::Position),
        _ => None,
    }
}

/// The graph of a script file.
pub(crate) fn script_of(file: &DesignFile) -> Result<ScriptGraph, Problem> {
    let mut nodes = Vec::new();
    for table in tables(file, NODES)? {
        let mut values = std::collections::BTreeMap::new();
        for (key, value) in table {
            if [NODE_ID, NODE_BLOCK, NODE_X, NODE_Y].contains(&key.as_str()) {
                continue;
            }
            let value = script_value(value)
                .ok_or_else(|| wrong(&format!("{key}: a number, text or [x, y, z]")))?;
            values.insert(key.clone(), value);
        }
        nodes.push(ScriptNode {
            id: text(table, NODE_ID)?,
            block: text(table, NODE_BLOCK)?,
            values,
        });
    }
    let links = tables(file, LINKS)?
        .map(|table| {
            Ok(ScriptLink {
                from: text(table, LINK_FROM)?,
                output: text(table, LINK_OUTPUT)?,
                to: text(table, LINK_TO)?,
            })
        })
        .collect::<Result<_, Problem>>()?;
    Ok(ScriptGraph { nodes, links })
}

fn whole(
    table: &toml::Table,
    key: &str,
    min: i64,
    max: i64,
    default: Option<i64>,
) -> Result<i64, Problem> {
    let number = match table.get(key) {
        Some(value) => value
            .as_integer()
            .ok_or_else(|| wrong(&format!("{key}: a whole number")))?,
        None => default.ok_or_else(|| wrong(&format!("{key}: a whole number")))?,
    };
    if (min..=max).contains(&number) {
        Ok(number)
    } else {
        Err(Problem::OutOfRange(format!("{key}: {min} to {max}")))
    }
}

/// The event of an event file.
pub(crate) fn event_of(file: &DesignFile) -> Result<GameEvent, Problem> {
    let waves = tables(file, WAVES)?
        .map(|table| {
            let offset = match table.get(WAVE_OFFSET) {
                Some(value) => {
                    position_of(value).ok_or_else(|| wrong("offset: [x, y, z] in metres"))?
                }
                None => [0.0; 3],
            };
            Ok(EventWave {
                vitals: text(table, WAVE_VITALS)?,
                level: u32::try_from(whole(table, WAVE_LEVEL, MIN_LEVEL, MAX_LEVEL, None)?)
                    .unwrap_or_default(),
                count: u32::try_from(whole(table, WAVE_COUNT, 1, MAX_WAVE_COUNT, None)?)
                    .unwrap_or_default(),
                delay_ms: u64::try_from(whole(table, WAVE_DELAY, 0, i64::MAX, Some(0))?)
                    .unwrap_or_default(),
                offset,
            })
        })
        .collect::<Result<Vec<_>, Problem>>()?;
    if waves.is_empty() {
        return Err(Problem::MissingField);
    }
    let end_after_ms = match file.fields.get(END_AFTER) {
        Some(value) => Some(
            value
                .as_integer()
                .and_then(|ms| u64::try_from(ms).ok())
                .ok_or_else(|| wrong("end_after_ms: whole milliseconds"))?,
        ),
        None => None,
    };
    let end_when_cleared = match file.fields.get(END_WHEN_CLEARED) {
        Some(value) => value
            .as_bool()
            .ok_or_else(|| wrong("end_when_cleared: true or false"))?,
        None => false,
    };
    Ok(GameEvent {
        waves,
        end_after_ms,
        end_when_cleared,
    })
}

/// The problems of a script or event file beyond its field names.
pub(crate) fn check_script_or_event(file: &DesignFile, errors: &mut Vec<DesignError>) {
    match file.kind {
        Kind::Script => match script_of(file) {
            Ok(graph) => errors.extend(graph.problems().into_iter().map(|problem| {
                DesignError::field(&file.path, NODES, Problem::Script(problem.to_string()))
            })),
            Err(problem) => errors.push(DesignError::field(&file.path, NODES, problem)),
        },
        Kind::Event => {
            if let Err(problem) = event_of(file) {
                errors.push(DesignError::field(&file.path, WAVES, problem));
            }
        }
        _ => {}
    }
}

/// Every `event` value of a script names an event file of the same level.
pub(crate) fn check_event_references(set: &DesignSet, errors: &mut Vec<DesignError>) {
    for file in set.files.iter().filter(|file| file.kind == Kind::Script) {
        let Ok(graph) = script_of(file) else {
            continue;
        };
        for node in graph
            .nodes
            .iter()
            .filter(|node| EVENT_BLOCKS.contains(&node.block.as_str()))
        {
            let Some(event) = node.values.get(EVENT_VALUE).and_then(ScriptValue::text) else {
                continue;
            };
            let made = set.files.iter().any(|other| {
                other.kind == Kind::Event
                    && other.map == file.map
                    && other.id.eq_ignore_ascii_case(event)
            });
            if !made {
                errors.push(DesignError::field(
                    &file.path,
                    NODES,
                    Problem::UnknownReference(format!("event {event}")),
                ));
            }
        }
    }
}

/// The scripts and events of `set` by level; files that do not read are left out (the check
/// reports them).
pub fn design_data(set: &DesignSet) -> DesignData {
    let mut data = DesignData::default();
    for file in &set.files {
        let Some(map) = &file.map else {
            continue;
        };
        let level = || LevelDesign::default();
        match file.kind {
            Kind::Script => {
                if let Ok(graph) = script_of(file) {
                    data.levels
                        .entry(map.clone())
                        .or_insert_with(level)
                        .scripts
                        .insert(file.id.clone(), graph);
                }
            }
            Kind::Event => {
                if let Ok(event) = event_of(file) {
                    data.levels
                        .entry(map.clone())
                        .or_insert_with(level)
                        .events
                        .insert(file.id.clone(), event);
                }
            }
            _ => {}
        }
    }
    data
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use super::*;
    use crate::load::parse_file;
    use crate::schema::check_schema;

    pub(crate) const SCRIPT: &str = r#"schema = 1
kind = "script"
action = "add"
id = "greeting"
map = "lv"

[[nodes]]
id = "enter"
block = "player_enters_area"
x = 40
y = 60
center = [1.0, 2.0, 3.0]
radius_m = 10

[[nodes]]
id = "start"
block = "start_event"
x = 200
y = 60
event = "invasion"
at = [1, 2, 3]

[[links]]
from = "enter"
output = "next"
to = "start"
"#;

    pub(crate) const EVENT: &str = r#"schema = 1
kind = "event"
action = "add"
id = "invasion"
map = "lv"
end_after_ms = 600000
end_when_cleared = true

[[waves]]
vitals = "Wolf_Alpha"
level = 20
count = 5

[[waves]]
vitals = "Wolf_Alpha"
level = 22
count = 3
delay_ms = 30000
offset = [10.0, 0.0, 0.0]
"#;

    fn file(text: &str, path: &str) -> DesignFile {
        parse_file(text, Path::new(path)).expect("file")
    }

    #[test]
    fn a_script_reads_its_nodes_values_and_links() {
        let graph = script_of(&file(SCRIPT, "scripts/lv/greeting.toml")).expect("graph");
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.nodes[0].values["radius_m"], ScriptValue::Number(10.0));
        assert_eq!(
            graph.nodes[1].values["at"],
            ScriptValue::Position([1.0, 2.0, 3.0])
        );
        assert!(
            !graph.nodes[0].values.contains_key("x"),
            "x and y are the canvas"
        );
        assert_eq!(graph.links[0].to, "start");
        assert_eq!(graph.problems(), []);
    }

    #[test]
    fn an_event_reads_its_waves_and_ends() {
        let event = event_of(&file(EVENT, "events/lv/invasion.toml")).expect("event");
        assert_eq!(event.waves.len(), 2);
        assert_eq!(
            (event.waves[0].delay_ms, event.waves[0].offset),
            (0, [0.0; 3])
        );
        assert_eq!(event.waves[1].delay_ms, 30_000);
        assert_eq!(event.end_after_ms, Some(600_000));
        assert!(event.end_when_cleared);
    }

    #[test]
    fn the_schema_check_reports_graph_problems_and_unknown_events() {
        let broken = SCRIPT.replace("radius_m = 10\n", "");
        let set = DesignSet {
            files: vec![file(&broken, "scripts/lv/greeting.toml")],
        };
        let errors = check_schema(&set);
        assert!(
            errors.iter().any(
                |error| matches!(&error.problem, Problem::Script(text) if text.contains("radius_m"))
            ),
            "{errors:?}"
        );
        assert!(
            errors.iter().any(
                |error| error.problem == Problem::UnknownReference("event invasion".to_owned())
            ),
            "{errors:?}"
        );
        let good = DesignSet {
            files: vec![
                file(EVENT, "events/lv/invasion.toml"),
                file(SCRIPT, "scripts/lv/greeting.toml"),
            ],
        };
        assert_eq!(check_schema(&good), []);
        let data = design_data(&good);
        assert_eq!(data.levels["lv"].scripts.len(), 1);
        assert_eq!(data.levels["lv"].events["invasion"].waves.len(), 2);
    }
}
