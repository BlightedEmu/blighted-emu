// SPDX-License-Identifier: AGPL-3.0-or-later
//! The quest tree of the studio (admin studio spec section 7.4): an objective's task tree, from
//! the client task records, for the records page.

use std::collections::{BTreeMap, HashSet};

use axum::Router;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use nw_design::Kind;
use serde::Serialize;
use serde_json::Value;

use crate::server::AppState;
use crate::studio::catalog::Catalog;

/// `GET`: the task tree of an objective.
pub const QUEST_TREE_ROUTE: &str = "/api/studio/quest-tree/{objective}";
/// The objective field that names the root task.
const ROOT_FIELD: &str = "task";
/// The task fields: its type and its sub-tasks.
const TYPE_FIELD: &str = "type";
const SUB_TASK_FIELD: &str = "sub_task";
/// How deep a tree may go (retail trees are a few levels deep).
const MAX_DEPTH: usize = 32;

/// The quest tree route.
pub fn quest_routes() -> Router<AppState> {
    Router::new().route(QUEST_TREE_ROUTE, get(quest_tree))
}

/// One task of a tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskNode {
    /// The task id as the parent names it.
    pub id: String,
    /// `Type`; `None` when there is no such task record.
    pub kind: Option<String>,
    /// True when the task is already in the tree above: its sub-tasks are shown there, so a
    /// loop or a task shared by several parents is walked once.
    pub repeat: bool,
    /// The sub-tasks in order.
    pub children: Vec<TaskNode>,
}

/// The tree under task `id` of `catalog`, `depth` levels down; `shown` holds the tasks already
/// in the tree.
fn node(catalog: &Catalog, id: &str, depth: usize, shown: &mut HashSet<String>) -> TaskNode {
    let record = catalog.record(Kind::ObjectiveTask, id);
    let fields = record.map(super::catalog::CatalogEntry::fields);
    let kind = fields
        .as_ref()
        .and_then(|fields| fields.get(TYPE_FIELD))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let sub_tasks: BTreeMap<u32, String> = fields
        .as_ref()
        .and_then(|fields| fields.get(SUB_TASK_FIELD))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    if !shown.insert(id.to_lowercase()) {
        return TaskNode {
            id: id.to_owned(),
            kind,
            repeat: true,
            children: Vec::new(),
        };
    }
    let children = if depth < MAX_DEPTH {
        sub_tasks
            .values()
            .map(|child| node(catalog, child, depth + 1, shown))
            .collect()
    } else {
        Vec::new()
    };
    TaskNode {
        id: id.to_owned(),
        kind,
        repeat: false,
        children,
    }
}

/// The task tree of objective `objective`; `None` when there is no such objective.
pub fn quest_tree_of(catalog: &Catalog, objective: &str) -> Option<TaskNode> {
    let root = catalog
        .record(Kind::Objective, objective)?
        .fields()
        .get(ROOT_FIELD)
        .and_then(Value::as_str)
        .map(str::to_owned)?;
    Some(node(catalog, &root, 0, &mut HashSet::new()))
}

async fn quest_tree(
    State(state): State<AppState>,
    UrlPath(objective): UrlPath<String>,
) -> Response {
    let Some(studio) = state.studio.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match studio.catalog().await {
        Ok(catalog) => match quest_tree_of(&catalog, &objective) {
            Some(tree) => Json(tree).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nw_content::generated::{ObjectiveTasks, Objectives};
    use nw_content::table::Table;
    use nw_content::{ContentBundle, Tables};

    use super::*;

    fn table<T>(rows: Vec<T>) -> Table<T> {
        Table {
            source_path: "test.datasheet".to_owned(),
            unique_id: "test".to_owned(),
            rows,
        }
    }

    fn task(id: &str, kind: &str, children: &[&str]) -> ObjectiveTasks {
        let sub_task: BTreeMap<String, &str> = children
            .iter()
            .enumerate()
            .map(|(index, child)| ((index + 1).to_string(), *child))
            .collect();
        serde_json::from_value(serde_json::json!({
            "task_id": id,
            "type": kind,
            "sub_task": sub_task,
        }))
        .expect("task")
    }

    #[test]
    fn a_tree_follows_sub_tasks_and_stops_at_a_loop() {
        let mut tables = Tables::default();
        let objective: Objectives = serde_json::from_value(serde_json::json!({
            "objective_id": "Q1",
            "task": "root",
            "type": "Quest",
        }))
        .expect("objective");
        tables.objectives.push(table(vec![objective]));
        tables.objective_tasks.push(table(vec![
            task("root", "ConsecutiveTaskContainer", &["a", "missing", "b"]),
            task("a", "TaskTimer", &["root", "shared"]),
            task("b", "TaskTimer", &["shared"]),
            task("shared", "TaskKill", &["deep"]),
        ]));
        let catalog = Catalog::of_bundle(&ContentBundle {
            tables,
            ..ContentBundle::default()
        });
        let tree = quest_tree_of(&catalog, "q1").expect("tree");
        assert_eq!(tree.kind.as_deref(), Some("ConsecutiveTaskContainer"));
        assert_eq!(tree.children.len(), 3);
        let a = &tree.children[0];
        assert_eq!(a.kind.as_deref(), Some("TaskTimer"));
        assert!(
            a.children[0].repeat && a.children[0].children.is_empty(),
            "the loop back to root stops"
        );
        assert_eq!(a.children[1].children.len(), 1, "shared is walked once");
        assert_eq!(tree.children[1].kind, None, "a missing task has no record");
        let again = &tree.children[2].children[0];
        assert_eq!((again.id.as_str(), again.repeat), ("shared", true));
        assert!(again.children.is_empty());
        assert_eq!(quest_tree_of(&catalog, "Q2"), None);
    }
}
