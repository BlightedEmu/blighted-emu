// SPDX-License-Identifier: AGPL-3.0-or-later
//! Scripts and events of the design files (admin studio spec sections 6.6, 7.5, 8.3): the block
//! registry that the checks, the sim and the editor share, the graph of one script, the waves
//! of one event, and the per-level file the design step writes beside the world files.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The output of a block that goes on.
pub const NEXT: &str = "next";
/// The output of a condition that holds.
pub const TRUE: &str = "true";
/// The output of a condition that does not hold.
pub const FALSE: &str = "false";
const ONE_OUTPUT: &[&str] = &[NEXT];
const CONDITION_OUTPUTS: &[&str] = &[TRUE, FALSE];
/// The file name prefix of the design data.
const DESIGN_DATA_PREFIX: &str = "design-";
const DESIGN_DATA_EXTENSION: &str = ".json";

/// What a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockCategory {
    /// Starts a run.
    Trigger,
    /// Picks `true` or `false`.
    Condition,
    /// Does something.
    Action,
    /// Waits.
    Flow,
}

/// The type of a block value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    /// A number above 0.
    Number,
    /// A whole number, 0 or more.
    Whole,
    /// A whole number 0 to 100.
    Percent,
    /// Text that is not empty.
    Text,
    /// `[x, y, z]` in metres.
    Position,
}

/// One value of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ValueSpec {
    /// Its name in files.
    pub name: &'static str,
    /// Its type.
    pub kind: ValueKind,
    /// Whether every node of the block needs it.
    pub required: bool,
}

/// One block kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BlockSpec {
    /// Its name in files.
    pub name: &'static str,
    /// What it is.
    pub category: BlockCategory,
    /// Its values.
    pub values: &'static [ValueSpec],
    /// Its outputs.
    pub outputs: &'static [&'static str],
    /// One line for the editor.
    pub help: &'static str,
}

const fn value(name: &'static str, kind: ValueKind, required: bool) -> ValueSpec {
    ValueSpec {
        name,
        kind,
        required,
    }
}

const AREA: &[ValueSpec] = &[
    value("center", ValueKind::Position, true),
    value("radius_m", ValueKind::Number, true),
];

/// Every block (admin studio spec section 7.5; message blocks are Unknown and left out).
pub const BLOCKS: &[BlockSpec] = &[
    BlockSpec {
        name: "player_enters_area",
        category: BlockCategory::Trigger,
        values: AREA,
        outputs: ONE_OUTPUT,
        help: "A player comes within radius_m of center.",
    },
    BlockSpec {
        name: "player_leaves_area",
        category: BlockCategory::Trigger,
        values: AREA,
        outputs: ONE_OUTPUT,
        help: "A player inside the area goes out of it.",
    },
    BlockSpec {
        name: "mob_dies",
        category: BlockCategory::Trigger,
        values: &[value("vitals", ValueKind::Text, false)],
        outputs: ONE_OUTPUT,
        help: "A player kills a mob (of the vitals id, when set); the killer is the run's player.",
    },
    BlockSpec {
        name: "timer",
        category: BlockCategory::Trigger,
        values: &[value("every_ms", ValueKind::Whole, true)],
        outputs: ONE_OUTPUT,
        help: "Every every_ms milliseconds, with no player.",
    },
    BlockSpec {
        name: "quest_done",
        category: BlockCategory::Trigger,
        values: &[value("objective", ValueKind::Text, true)],
        outputs: ONE_OUTPUT,
        help: "A player finishes the objective.",
    },
    BlockSpec {
        name: "event_starts",
        category: BlockCategory::Trigger,
        values: &[value("event", ValueKind::Text, true)],
        outputs: ONE_OUTPUT,
        help: "A GM starts the event, with no player.",
    },
    BlockSpec {
        name: "random_chance",
        category: BlockCategory::Condition,
        values: &[value("percent", ValueKind::Percent, true)],
        outputs: CONDITION_OUTPUTS,
        help: "True in percent of the runs.",
    },
    BlockSpec {
        name: "has_item",
        category: BlockCategory::Condition,
        values: &[
            value("item", ValueKind::Text, true),
            value("quantity", ValueKind::Whole, false),
        ],
        outputs: CONDITION_OUTPUTS,
        help: "True when the run's player carries quantity (1) of the item.",
    },
    BlockSpec {
        name: "quest_state",
        category: BlockCategory::Condition,
        values: &[
            value("objective", ValueKind::Text, true),
            value("state", ValueKind::Text, true),
        ],
        outputs: CONDITION_OUTPUTS,
        help: "True when the run's player has the objective in state none, active or done.",
    },
    BlockSpec {
        name: "flag_is",
        category: BlockCategory::Condition,
        values: &[
            value("flag", ValueKind::Text, true),
            value("value", ValueKind::Whole, true),
        ],
        outputs: CONDITION_OUTPUTS,
        help: "True when the script's flag has the value (a flag starts at 0).",
    },
    BlockSpec {
        name: "mobs_alive",
        category: BlockCategory::Condition,
        values: &[value("at_most", ValueKind::Whole, true)],
        outputs: CONDITION_OUTPUTS,
        help: "True when at most at_most of the mobs this script spawned live.",
    },
    BlockSpec {
        name: "spawn_mobs",
        category: BlockCategory::Action,
        values: &[
            value("vitals", ValueKind::Text, true),
            value("level", ValueKind::Whole, true),
            value("count", ValueKind::Whole, true),
            value("at", ValueKind::Position, true),
        ],
        outputs: ONE_OUTPUT,
        help: "Spawns count mobs of the vitals id around at; they end when the sim restarts.",
    },
    BlockSpec {
        name: "despawn_mobs",
        category: BlockCategory::Action,
        values: &[],
        outputs: ONE_OUTPUT,
        help: "Removes the mobs this script spawned.",
    },
    BlockSpec {
        name: "give_item",
        category: BlockCategory::Action,
        values: &[
            value("item", ValueKind::Text, true),
            value("quantity", ValueKind::Whole, false),
        ],
        outputs: ONE_OUTPUT,
        help: "Puts quantity (1) of the item into the run's player's bag.",
    },
    BlockSpec {
        name: "start_quest",
        category: BlockCategory::Action,
        values: &[value("objective", ValueKind::Text, true)],
        outputs: ONE_OUTPUT,
        help: "Starts the objective for the run's player.",
    },
    BlockSpec {
        name: "set_flag",
        category: BlockCategory::Action,
        values: &[
            value("flag", ValueKind::Text, true),
            value("value", ValueKind::Whole, true),
        ],
        outputs: ONE_OUTPUT,
        help: "Sets the script's flag.",
    },
    BlockSpec {
        name: "start_event",
        category: BlockCategory::Action,
        values: &[
            value("event", ValueKind::Text, true),
            value("at", ValueKind::Position, true),
        ],
        outputs: ONE_OUTPUT,
        help: "Starts the event at the point.",
    },
    BlockSpec {
        name: "wait",
        category: BlockCategory::Flow,
        values: &[value("ms", ValueKind::Whole, true)],
        outputs: ONE_OUTPUT,
        help: "Waits ms milliseconds.",
    },
];

/// The block called `name`.
pub fn block_spec(name: &str) -> Option<&'static BlockSpec> {
    BLOCKS.iter().find(|block| block.name == name)
}

/// A value of a script node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptValue {
    /// A number.
    Number(f64),
    /// Text.
    Text(String),
    /// A position.
    Position([f32; 3]),
}

impl ScriptValue {
    /// The number, when it is one.
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    /// The text, when it is text.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The position, when it is one.
    pub fn position(&self) -> Option<[f32; 3]> {
        match self {
            Self::Position(position) => Some(*position),
            _ => None,
        }
    }

    /// Whether this value fits `kind`.
    pub fn fits(&self, kind: ValueKind) -> bool {
        let whole = |number: f64| number.is_finite() && number >= 0.0 && number.fract() == 0.0;
        match (self, kind) {
            (Self::Number(number), ValueKind::Number) => number.is_finite() && *number > 0.0,
            (Self::Number(number), ValueKind::Whole) => whole(*number),
            (Self::Number(number), ValueKind::Percent) => whole(*number) && *number <= 100.0,
            (Self::Text(text), ValueKind::Text) => !text.trim().is_empty(),
            (Self::Position(position), ValueKind::Position) => {
                position.iter().all(|axis| axis.is_finite())
            }
            _ => false,
        }
    }
}

/// One node of a script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptNode {
    /// Its id in the script.
    pub id: String,
    /// Its block name.
    pub block: String,
    /// Its values by name.
    pub values: BTreeMap<String, ScriptValue>,
}

/// A link from an output of one node to another node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLink {
    /// The node it leaves.
    pub from: String,
    /// The output it leaves by.
    pub output: String,
    /// The node it enters.
    pub to: String,
}

/// One script.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScriptGraph {
    /// The nodes.
    pub nodes: Vec<ScriptNode>,
    /// The links.
    pub links: Vec<ScriptLink>,
}

/// What is wrong with a script graph.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphProblem {
    /// No node is a trigger.
    #[error("the script has no trigger")]
    NoTrigger,
    /// Two nodes share an id.
    #[error("node {0}: the id is used twice")]
    DuplicateNode(String),
    /// A node names no known block.
    #[error("node {0}: unknown block")]
    UnknownBlock(String),
    /// A node lacks a required value.
    #[error("node {node}: missing value {value}")]
    MissingValue {
        /// The node.
        node: String,
        /// The value.
        value: String,
    },
    /// A node has a value of the wrong type.
    #[error("node {node}: wrong value {value}")]
    WrongValue {
        /// The node.
        node: String,
        /// The value.
        value: String,
    },
    /// A node has a value its block does not have.
    #[error("node {node}: unknown value {value}")]
    UnknownValue {
        /// The node.
        node: String,
        /// The value.
        value: String,
    },
    /// A link names a node that does not exist.
    #[error("a link names unknown node {0}")]
    UnknownNode(String),
    /// A link leaves by an output its block does not have.
    #[error("node {node}: no output {output}")]
    BadOutput {
        /// The node.
        node: String,
        /// The output.
        output: String,
    },
    /// A link goes into a trigger.
    #[error("node {0}: a trigger has no input")]
    IntoTrigger(String),
    /// A node that is not a trigger has no link into it.
    #[error("node {0}: nothing links into it")]
    NotReached(String),
    /// A loop has no wait.
    #[error("node {0}: a loop through it has no wait")]
    LoopWithoutWait(String),
}

impl ScriptGraph {
    /// Every problem of the graph (admin studio spec section 7.5).
    pub fn problems(&self) -> Vec<GraphProblem> {
        let mut problems = Vec::new();
        let mut blocks: HashMap<&str, &BlockSpec> = HashMap::new();
        for node in &self.nodes {
            if blocks.contains_key(node.id.as_str()) {
                problems.push(GraphProblem::DuplicateNode(node.id.clone()));
                continue;
            }
            let Some(spec) = block_spec(&node.block) else {
                problems.push(GraphProblem::UnknownBlock(node.id.clone()));
                continue;
            };
            blocks.insert(&node.id, spec);
            problems.extend(value_problems(node, spec));
        }
        if !blocks
            .values()
            .any(|spec| spec.category == BlockCategory::Trigger)
        {
            problems.push(GraphProblem::NoTrigger);
        }
        let mut reached = HashSet::new();
        for link in &self.links {
            let (Some(from), Some(to)) =
                (blocks.get(link.from.as_str()), blocks.get(link.to.as_str()))
            else {
                for end in [&link.from, &link.to] {
                    if !blocks.contains_key(end.as_str()) {
                        problems.push(GraphProblem::UnknownNode(end.clone()));
                    }
                }
                continue;
            };
            if !from.outputs.contains(&link.output.as_str()) {
                problems.push(GraphProblem::BadOutput {
                    node: link.from.clone(),
                    output: link.output.clone(),
                });
            }
            if to.category == BlockCategory::Trigger {
                problems.push(GraphProblem::IntoTrigger(link.to.clone()));
            }
            reached.insert(link.to.as_str());
        }
        for node in &self.nodes {
            let unreached = blocks
                .get(node.id.as_str())
                .is_some_and(|spec| spec.category != BlockCategory::Trigger)
                && !reached.contains(node.id.as_str());
            if unreached {
                problems.push(GraphProblem::NotReached(node.id.clone()));
            }
        }
        if let Some(node) = self.loop_without_wait(&blocks) {
            problems.push(GraphProblem::LoopWithoutWait(node));
        }
        problems
    }

    /// A node on a cycle of links that passes no `wait`, when there is one.
    fn loop_without_wait(&self, blocks: &HashMap<&str, &BlockSpec>) -> Option<String> {
        let waits = |id: &str| {
            blocks
                .get(id)
                .is_some_and(|spec| spec.category == BlockCategory::Flow)
        };
        let mut next: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for link in &self.links {
            if !waits(&link.from) && !waits(&link.to) {
                next.entry(&link.from).or_default().insert(&link.to);
            }
        }
        // Depth first with three colours: on the path (grey) or finished (black).
        let mut grey = HashSet::new();
        let mut black = HashSet::new();
        fn visit<'a>(
            node: &'a str,
            next: &BTreeMap<&'a str, BTreeSet<&'a str>>,
            grey: &mut HashSet<&'a str>,
            black: &mut HashSet<&'a str>,
        ) -> Option<&'a str> {
            if black.contains(node) {
                return None;
            }
            if !grey.insert(node) {
                return Some(node);
            }
            for child in next.get(node).into_iter().flatten() {
                if let Some(found) = visit(child, next, grey, black) {
                    return Some(found);
                }
            }
            grey.remove(node);
            black.insert(node);
            None
        }
        next.keys()
            .find_map(|node| visit(node, &next, &mut grey, &mut black))
            .map(str::to_owned)
    }
}

/// The value problems of `node` of block `spec`.
fn value_problems(node: &ScriptNode, spec: &BlockSpec) -> Vec<GraphProblem> {
    let mut problems = Vec::new();
    for wanted in spec.values {
        match node.values.get(wanted.name) {
            Some(given) if !given.fits(wanted.kind) => problems.push(GraphProblem::WrongValue {
                node: node.id.clone(),
                value: wanted.name.to_owned(),
            }),
            None if wanted.required => problems.push(GraphProblem::MissingValue {
                node: node.id.clone(),
                value: wanted.name.to_owned(),
            }),
            _ => {}
        }
    }
    for name in node.values.keys() {
        if !spec.values.iter().any(|wanted| wanted.name == name) {
            problems.push(GraphProblem::UnknownValue {
                node: node.id.clone(),
                value: name.clone(),
            });
        }
    }
    problems
}

/// One wave of an event: GM mobs spawned after a delay, around the event's point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventWave {
    /// The vitals id.
    pub vitals: String,
    /// Their level.
    pub level: u32,
    /// How many.
    pub count: u32,
    /// After the start, in ms.
    pub delay_ms: u64,
    /// From the event's point, in metres.
    pub offset: [f32; 3],
}

/// One event (admin studio spec section 8.3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameEvent {
    /// The waves.
    pub waves: Vec<EventWave>,
    /// It ends this long after its start; `None`: no time limit.
    pub end_after_ms: Option<u64>,
    /// It ends when every wave has spawned and none of its mobs lives.
    pub end_when_cleared: bool,
}

/// The scripts and events of one level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LevelDesign {
    /// The scripts by id.
    pub scripts: BTreeMap<String, ScriptGraph>,
    /// The events by id.
    pub events: BTreeMap<String, GameEvent>,
}

/// The scripts and events of every level, as the design step writes them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DesignData {
    /// The levels by name.
    pub levels: BTreeMap<String, LevelDesign>,
}

/// Why the design data cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum DesignDataError {
    /// The file cannot be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not design data.
    #[error("{path}: {source}")]
    Json {
        /// The file.
        path: PathBuf,
        /// Why.
        source: serde_json::Error,
    },
}

/// The design data file of `client_build` in `dir`.
pub fn design_data_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!(
        "{DESIGN_DATA_PREFIX}{client_build}{DESIGN_DATA_EXTENSION}"
    ))
}

impl DesignData {
    /// Reads `path`; a missing file is empty design data (content from before scripts).
    ///
    /// # Errors
    ///
    /// Returns [`DesignDataError`] when the file cannot be read or is not design data.
    pub fn load(path: &Path) -> Result<Self, DesignDataError> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| DesignDataError::Json {
                path: path.to_path_buf(),
                source,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(DesignDataError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// The JSON text of the data.
    ///
    /// # Errors
    ///
    /// Returns [`DesignDataError::Json`] when a value cannot be written (a number that is not
    /// finite).
    pub fn to_json(&self, path: &Path) -> Result<String, DesignDataError> {
        serde_json::to_string_pretty(self).map_err(|source| DesignDataError::Json {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, block: &str, values: &[(&str, ScriptValue)]) -> ScriptNode {
        ScriptNode {
            id: id.to_owned(),
            block: block.to_owned(),
            values: values
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
        }
    }

    fn link(from: &str, output: &str, to: &str) -> ScriptLink {
        ScriptLink {
            from: from.to_owned(),
            output: output.to_owned(),
            to: to.to_owned(),
        }
    }

    fn area() -> Vec<(&'static str, ScriptValue)> {
        vec![
            ("center", ScriptValue::Position([1.0, 2.0, 3.0])),
            ("radius_m", ScriptValue::Number(10.0)),
        ]
    }

    fn greeting() -> ScriptGraph {
        ScriptGraph {
            nodes: vec![
                node("enter", "player_enters_area", &area()),
                node(
                    "roll",
                    "random_chance",
                    &[("percent", ScriptValue::Number(50.0))],
                ),
                node(
                    "give",
                    "give_item",
                    &[("item", ScriptValue::Text("FlintT1".to_owned()))],
                ),
            ],
            links: vec![link("enter", NEXT, "roll"), link("roll", TRUE, "give")],
        }
    }

    #[test]
    fn a_good_graph_has_no_problem() {
        assert_eq!(greeting().problems(), []);
    }

    #[test]
    fn values_and_links_are_checked() {
        let mut graph = greeting();
        graph.nodes[1]
            .values
            .insert("percent".to_owned(), ScriptValue::Number(150.0));
        graph.nodes[2].values.clear();
        graph.nodes[2]
            .values
            .insert("colour".to_owned(), ScriptValue::Text("red".to_owned()));
        graph.links.push(link("give", "maybe", "enter"));
        graph.links.push(link("give", NEXT, "nowhere"));
        let problems = graph.problems();
        for expected in [
            GraphProblem::WrongValue {
                node: "roll".to_owned(),
                value: "percent".to_owned(),
            },
            GraphProblem::MissingValue {
                node: "give".to_owned(),
                value: "item".to_owned(),
            },
            GraphProblem::UnknownValue {
                node: "give".to_owned(),
                value: "colour".to_owned(),
            },
            GraphProblem::BadOutput {
                node: "give".to_owned(),
                output: "maybe".to_owned(),
            },
            GraphProblem::IntoTrigger("enter".to_owned()),
            GraphProblem::UnknownNode("nowhere".to_owned()),
        ] {
            assert!(problems.contains(&expected), "{expected:?} in {problems:?}");
        }
    }

    #[test]
    fn a_graph_needs_a_trigger_and_reached_nodes() {
        let graph = ScriptGraph {
            nodes: vec![node("despawn", "despawn_mobs", &[])],
            links: Vec::new(),
        };
        assert_eq!(
            graph.problems(),
            [
                GraphProblem::NoTrigger,
                GraphProblem::NotReached("despawn".to_owned())
            ]
        );
    }

    #[test]
    fn a_loop_needs_a_wait() {
        let mut graph = ScriptGraph {
            nodes: vec![
                node(
                    "tick",
                    "timer",
                    &[("every_ms", ScriptValue::Number(1000.0))],
                ),
                node("a", "despawn_mobs", &[]),
                node("b", "despawn_mobs", &[]),
            ],
            links: vec![
                link("tick", NEXT, "a"),
                link("a", NEXT, "b"),
                link("b", NEXT, "a"),
            ],
        };
        assert!(matches!(
            graph.problems()[..],
            [GraphProblem::LoopWithoutWait(_)]
        ));
        graph
            .nodes
            .push(node("pause", "wait", &[("ms", ScriptValue::Number(500.0))]));
        graph.links.pop();
        graph.links.push(link("b", NEXT, "pause"));
        graph.links.push(link("pause", NEXT, "a"));
        assert_eq!(graph.problems(), []);
    }

    #[test]
    fn design_data_reads_back_and_a_missing_file_is_empty() {
        let dir = tempfile::tempdir().expect("dir");
        let path = design_data_path(dir.path(), "1");
        assert_eq!(
            DesignData::load(&path).expect("load"),
            DesignData::default()
        );
        let data = DesignData {
            levels: BTreeMap::from([(
                "lv".to_owned(),
                LevelDesign {
                    scripts: BTreeMap::from([("greeting".to_owned(), greeting())]),
                    events: BTreeMap::from([(
                        "invasion".to_owned(),
                        GameEvent {
                            waves: vec![EventWave {
                                vitals: "Wolf".to_owned(),
                                level: 5,
                                count: 3,
                                delay_ms: 0,
                                offset: [0.0, 0.0, 0.0],
                            }],
                            end_after_ms: Some(60_000),
                            end_when_cleared: true,
                        },
                    )]),
                },
            )]),
        };
        std::fs::write(&path, data.to_json(&path).expect("json")).expect("write");
        assert_eq!(DesignData::load(&path).expect("load"), data);
    }
}
