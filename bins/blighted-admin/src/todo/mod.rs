// SPDX-License-Identifier: AGPL-3.0-or-later
//! The team's to-do list (admin studio spec section 10): `content/todo.toml`, one `[[task]]`
//! table per task, so two people who add tasks at once get small merge conflicts.

pub mod api;
pub mod github;

use std::path::{Path, PathBuf};

use nw_design::{Kind, TODO_FILE, write_file};
use serde::{Deserialize, Serialize};

/// The id prefix of a task: `t1`, `t2`, ...
const TASK_ID_PREFIX: char = 't';
/// The first task number.
const FIRST_TASK: u32 = 1;

/// What a task is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// A quest.
    Quest,
    /// Loot.
    Loot,
    /// A spawn.
    Spawn,
    /// A patrol path.
    Path,
    /// An AI profile.
    Ai,
    /// A script.
    Script,
    /// An item.
    Item,
    /// An event.
    Event,
    /// Anything else.
    Other,
}

/// Where a task stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Nobody works on it.
    Open,
    /// Someone works on it.
    Taken,
    /// Finished.
    Done,
}

/// One task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// The id, `t<number>`.
    pub id: String,
    /// One line.
    pub title: String,
    /// More words.
    #[serde(default)]
    pub text: String,
    /// What it is about.
    pub kind: TaskKind,
    /// Where it stands.
    pub state: TaskState,
    /// The name of whoever took it (free text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taken_by: Option<String>,
    /// The GitHub issue it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    /// The design files it touches, relative to the design folder.
    #[serde(default)]
    pub records: Vec<String>,
}

/// The whole list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoList {
    /// The tasks, in the order they were added.
    #[serde(default)]
    pub task: Vec<Task>,
}

impl TodoList {
    /// The id the next task gets: one above the highest number in use.
    pub fn next_id(&self) -> String {
        let highest = self
            .task
            .iter()
            .filter_map(|task| task.id.strip_prefix(TASK_ID_PREFIX)?.parse::<u32>().ok())
            .max();
        let next = highest.map_or(FIRST_TASK, |number| number.saturating_add(1));
        format!("{TASK_ID_PREFIX}{next}")
    }

    /// The task `id`.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.task.iter_mut().find(|task| task.id == id)
    }
}

/// The to-do file of the design folder `design_dir`.
pub fn todo_path(design_dir: &Path) -> PathBuf {
    design_dir.join(TODO_FILE)
}

/// Reads the list; a missing file is an empty list.
///
/// # Errors
///
/// Returns the reason when the file cannot be read or is not a to-do list.
pub fn load(design_dir: &Path) -> Result<TodoList, String> {
    let path = todo_path(design_dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(TodoList::default()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Writes the list through a temporary file, so a reader sees the old list or the new one.
///
/// # Errors
///
/// Returns the reason when the file cannot be written.
pub fn save(design_dir: &Path, list: &TodoList) -> Result<(), String> {
    let text = toml::to_string(list).map_err(|error| error.to_string())?;
    write_file(design_dir, Path::new(TODO_FILE), &text).map_err(|error| error.to_string())
}

/// The editor page of the design file `record`: the records page for a client table kind, the
/// studio for the others.
pub fn editor_url(record: &str) -> String {
    let path = Path::new(record);
    match Kind::of_path(path) {
        Some(kind) if !kind.is_point() && !kind.is_own() => {
            let id = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            format!("/records?kind={}&id={id}", kind.name())
        }
        _ => "/studio".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> Task {
        Task {
            id: id.to_owned(),
            title: "Wolves drop nothing".to_owned(),
            text: String::new(),
            kind: TaskKind::Loot,
            state: TaskState::Open,
            taken_by: None,
            issue: None,
            records: vec!["mobs/Wolf.toml".to_owned()],
        }
    }

    #[test]
    fn ids_count_up_from_the_highest() {
        assert_eq!(TodoList::default().next_id(), "t1");
        let list = TodoList {
            task: vec![task("t4"), task("t2"), task("custom")],
        };
        assert_eq!(list.next_id(), "t5");
    }

    #[test]
    fn a_list_round_trips_through_the_file() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(load(dir.path()), Ok(TodoList::default()));
        let list = TodoList {
            task: vec![task("t1")],
        };
        save(dir.path(), &list).expect("save");
        assert_eq!(load(dir.path()), Ok(list));
        let text = std::fs::read_to_string(todo_path(dir.path())).expect("read");
        assert!(text.contains("[[task]]"), "{text}");
    }

    #[test]
    fn records_open_their_editor() {
        assert_eq!(
            editor_url("items/1hSwordT2.toml"),
            "/records?kind=item&id=1hSwordT2"
        );
        assert_eq!(
            editor_url("loot/tables/LT_A.toml"),
            "/records?kind=loot_table&id=LT_A"
        );
        assert_eq!(editor_url("paths/lv/walk.toml"), "/studio");
        assert_eq!(editor_url("notes.txt"), "/studio");
    }
}
