// SPDX-License-Identifier: AGPL-3.0-or-later
//! Objective task trees: the server script of a quest (live-tutorial note, section 0 and
//! appendix A).
//!
//! An objective's root task is a tree of `ObjectiveTasks` rows. A container task runs its
//! children: one after the other (`ConsecutiveTaskContainer`), all at once until one is done
//! (`PartialTaskContainer`), or all at once until all are done (`SimpleTaskContainer`). A leaf
//! task is done at once (an achievement unlock, an item removal), after a time (a timer), or on a
//! game event (a video, a cutscene, a kill, an area, an item, an equip, a UI screen). The client
//! plays the scenes on its own when the server sets the achievements of the tree.

use std::collections::HashMap;
use std::sync::Arc;

use nw_bus::{QuestRun, QuestTask};
use nw_content::Tables;
use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::Position;

/// How near a player must come to a trigger area's `OverrideLocation` when its volume has no
/// size of its own in [`TRIGGER_RADIUS_BY_TAG`] (our choice: the open-world start trigger fired
/// with the player 17.2 m from it at its spawn; the real trigger volumes are client level data
/// the server does not read).
const TRIGGER_RADIUS_M: f32 = 18.0;
/// The size of a trigger volume by its `TP_TargetTag`, just above the distance at which retail
/// fired it (tutorial capture, ruins-to-exit and beach-ruins-chest notes). One 18 m circle fired
/// the ruins entry 15.6 m away through the closed door in a live run (2026-09-14) and fired too
/// late where retail fires from farther.
const TRIGGER_RADIUS_BY_TAG: [(&str, f32); 6] = [
    // Fired at 6.9 m.
    ("FTUE_EnterRuins_00_Trigger", 7.0),
    // Fired at 4.9 m.
    ("FTUE_First_Fight_Trigger", 5.5),
    // Not fired with the player 10 to 11 m away for 5 s; the failsafe shares it.
    ("FTUE_First_Fight_Trigger_Failsafe", 9.5),
    // Fired at 4.2 m.
    ("FTUE_Chest_Discovery_Trigger", 4.5),
    // ForClimb fired at 24.8 m.
    ("FTUE_Second_Fight_Trigger_Failsafe", 25.0),
    // Fired at 20.1 m.
    ("FTUE_TF_Trigger", 20.5),
];

/// The size of the trigger volume of tag `tag`.
fn trigger_radius_m(tag: Option<&str>) -> f32 {
    tag.and_then(|tag| {
        TRIGGER_RADIUS_BY_TAG
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(tag))
    })
    .map_or(TRIGGER_RADIUS_M, |(_, radius)| *radius)
}
/// Separates the numbers of an `OverrideLocation` cell.
const LOCATION_SEPARATOR: char = ' ';
/// Milliseconds per second of a `TaskTimer` duration.
const MS_PER_SECOND: f64 = 1000.0;
/// Separates the categories of a `VitalsCategories` cell.
const CATEGORY_SEPARATOR: char = ',';
/// Joins a kill type to the rest of a longer vitals id (`Legion_Legionnaire` names
/// `Legion_Legionnaire_FTUE`).
const ID_PART_SEPARATOR: char = '_';
/// The `UIScreenTag` of a `TaskGameEvent` that designers made to never end (beach-quests note:
/// the slice-swap timer branch of `12_skinningknife` waits on it, so the knife ends its partial
/// container).
const NEVER_TRUE_TAG: &str = "neverbetrue";
/// The chance of a quest drop when its task names none, in percent.
const ALWAYS_PERCENT: u32 = 100;

/// What a task waits for.
#[derive(Debug, Clone, PartialEq)]
enum TaskKind {
    /// Runs its children one after the other.
    Consecutive,
    /// Runs its children at once; done when one is done.
    Partial,
    /// Runs its children at once; done when all are done.
    Simple,
    /// Sets an achievement and is done.
    UnlockAchievement(String),
    /// Clears an achievement and is done.
    LockAchievement(String),
    /// Done when the player has the achievement.
    HasAchievement(String),
    /// Removes an item from the player and is done.
    RemoveItem(String),
    /// Done after a time.
    Timer { duration_ms: u64 },
    /// Done when the cinematic video of this id starts.
    CinematicStart(u32),
    /// Done when the cinematic video of this id ends.
    CinematicComplete(u32),
    /// Done when the in-game cutscene of this id ends.
    CutsceneComplete(u32),
    /// Done when the player comes within `radius_m` of the location; without one, never.
    TriggerArea {
        /// The location.
        at: Option<Position>,
        /// The size of its volume.
        radius_m: f32,
    },
    /// Done after `count` kills of mobs of the type.
    KillContribution { enemy: String, count: u32 },
    /// Done when the player gets the item; without one, when it loots a container.
    HaveItem(Option<String>),
    /// Done when the player holds `count` of the item, in the bag or worn (`TaskHaveItems`).
    HaveItems { item: String, count: u32 },
    /// Done after the player gathered `count` of the item; a corpse of `drop`'s enemy that the
    /// player skins while it waits drops it (`TaskGatherItems`).
    GatherItems {
        item: String,
        count: u32,
        drop: Option<QuestDrop>,
    },
    /// Done when the player crafts the recipe of this id (`TaskCraftRecipe`).
    CraftRecipe(String),
    /// Never done: a `TaskGameEvent` on [`NEVER_TRUE_TAG`].
    Never,
    /// Done when the player equips the item.
    EquipItem(String),
    /// Done when the client opens the UI screen of this id.
    OpenUiScreen(u32),
    /// A task type this server does not run: done at once, so the tree goes on.
    Unsupported,
}

/// The item a gather task adds when the player skins a corpse of its enemy.
#[derive(Debug, Clone, PartialEq, Eq)]
struct QuestDrop {
    /// `ItemDropVC`: the enemy type, as a kill task names one.
    enemy: String,
    /// `ItemDropProbability`, in percent.
    percent: u32,
}

/// One task of a tree.
#[derive(Debug, Clone, PartialEq)]
struct Task {
    id: String,
    kind: TaskKind,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// The task tree of one objective, built from the content.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectiveTree {
    /// Depth first; the root is task 0.
    tasks: Vec<Task>,
    /// Each task's number on the wire: its place when the tree is walked children first
    /// (objectives-state note, section 4: the root is last).
    wire_index: Vec<u8>,
}

/// The `ObjectiveTasks` fields one task needs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskRow {
    /// `TaskID`.
    pub task_id: String,
    /// `Type`.
    pub kind: String,
    /// `ItemName`.
    pub item_name: Option<String>,
    /// `TargetQty`.
    pub target_qty: Option<i64>,
    /// `Duration` in seconds.
    pub duration: Option<f64>,
    /// `OverrideLocation`: "x y z".
    pub override_location: Option<String>,
    /// `KillEnemyType`.
    pub kill_enemy_type: Option<String>,
    /// `UIScreenTag`.
    pub ui_screen_tag: Option<String>,
    /// `SubTask` ids in order.
    pub sub_tasks: Vec<String>,
    /// `TP_TargetTag`: the trigger volume of an area task.
    pub target_tag: Option<String>,
    /// `CraftingRecipeID`.
    pub crafting_recipe_id: Option<String>,
    /// `ItemDropVC`.
    pub item_drop_vc: Option<String>,
    /// `ItemDropProbability`.
    pub item_drop_probability: Option<i64>,
}

/// Every task row with an index by lowercase task id, made once for the trees of many
/// objectives.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskBook {
    rows: Vec<TaskRow>,
    by_id: HashMap<String, usize>,
}

impl TaskBook {
    /// The book of `rows`.
    pub fn new(rows: Vec<TaskRow>) -> Self {
        let by_id = rows
            .iter()
            .enumerate()
            .map(|(index, row)| (row.task_id.to_lowercase(), index))
            .collect();
        Self { rows, by_id }
    }

    /// The tree under task `root`, as [`ObjectiveTree::build`].
    pub fn tree(&self, root: &str) -> Option<ObjectiveTree> {
        ObjectiveTree::build_with(
            &|id| {
                self.by_id
                    .get(&id.to_lowercase())
                    .and_then(|index| self.rows.get(*index))
            },
            root,
        )
    }
}

/// The task rows of `tables`.
pub fn task_rows(tables: &Tables) -> Vec<TaskRow> {
    tables
        .objective_tasks
        .iter()
        .flat_map(|table| &table.rows)
        .map(|row| TaskRow {
            task_id: row.task_id.clone(),
            kind: row.r#type.clone(),
            item_name: row.item_name.clone(),
            target_qty: row.target_qty,
            duration: row.duration,
            override_location: row.override_location.clone(),
            kill_enemy_type: row.kill_enemy_type.clone(),
            ui_screen_tag: row.ui_screen_tag.clone(),
            sub_tasks: row.sub_task.values().cloned().collect(),
            target_tag: row.tp_target_tag.clone(),
            crafting_recipe_id: row.crafting_recipe_id.clone(),
            item_drop_vc: row.item_drop_vc.clone(),
            item_drop_probability: row.item_drop_probability,
        })
        .collect()
}

/// A location cell "x y z"; `None` when it is not three numbers.
fn location_of(cell: &str) -> Option<Position> {
    let mut numbers = cell
        .split(LOCATION_SEPARATOR)
        .filter(|part| !part.is_empty())
        .map(str::parse::<f32>);
    let position = Position {
        x: numbers.next()?.ok()?,
        y: numbers.next()?.ok()?,
        z: numbers.next()?.ok()?,
    };
    numbers.next().is_none().then_some(position)
}

/// The kind of `row`. The type cell's case varies (Observed: `TASKUNLOCKACHIEVEMENT` and
/// `TasklockAchievement` in the objective task sheets), so it is matched without case.
fn kind_of(row: &TaskRow) -> TaskKind {
    let item = || row.item_name.clone().unwrap_or_default();
    let crc = || lowercase_crc32(&item());
    let count = row
        .target_qty
        .and_then(|qty| u32::try_from(qty).ok())
        .unwrap_or(1)
        .max(1);
    match row.kind.to_ascii_lowercase().as_str() {
        "consecutivetaskcontainer" => TaskKind::Consecutive,
        "partialtaskcontainer" => TaskKind::Partial,
        "simpletaskcontainer" => TaskKind::Simple,
        "taskunlockachievement" => TaskKind::UnlockAchievement(item()),
        "tasklockachievement" => TaskKind::LockAchievement(item()),
        "taskachievement" => TaskKind::HasAchievement(item()),
        "taskremoveitem" => TaskKind::RemoveItem(item()),
        "tasktimer" => TaskKind::Timer {
            // Durations are a few seconds: the cast cannot overflow.
            duration_ms: (row.duration.unwrap_or_default().max(0.0) * MS_PER_SECOND) as u64,
        },
        "taskcinematicstart" => TaskKind::CinematicStart(crc()),
        "taskcinematiccomplete" => TaskKind::CinematicComplete(crc()),
        "taskcutscenecomplete" => TaskKind::CutsceneComplete(crc()),
        "tasktriggerarea" => TaskKind::TriggerArea {
            at: row.override_location.as_deref().and_then(location_of),
            radius_m: trigger_radius_m(row.target_tag.as_deref()),
        },
        "taskkillcontribution" => TaskKind::KillContribution {
            enemy: row.kill_enemy_type.clone().unwrap_or_default(),
            count,
        },
        "taskhaveandreturnitems" => TaskKind::HaveItem(row.item_name.clone()),
        "taskhaveitems" => TaskKind::HaveItems {
            item: item(),
            count,
        },
        "taskgatheritems" => TaskKind::GatherItems {
            item: item(),
            count,
            drop: row
                .item_drop_vc
                .clone()
                .filter(|enemy| !enemy.is_empty())
                .map(|enemy| QuestDrop {
                    enemy,
                    percent: row
                        .item_drop_probability
                        .and_then(|percent| u32::try_from(percent).ok())
                        .unwrap_or(ALWAYS_PERCENT),
                }),
        },
        "taskcraftrecipe" => {
            TaskKind::CraftRecipe(row.crafting_recipe_id.clone().unwrap_or_default())
        }
        "taskgameevent"
            if row
                .ui_screen_tag
                .as_deref()
                .is_some_and(|tag| tag.eq_ignore_ascii_case(NEVER_TRUE_TAG)) =>
        {
            TaskKind::Never
        }
        "taskequipitem" => TaskKind::EquipItem(item()),
        "taskopenuiscreen" => {
            TaskKind::OpenUiScreen(lowercase_crc32(row.ui_screen_tag.as_deref().unwrap_or("")))
        }
        _ => TaskKind::Unsupported,
    }
}

impl ObjectiveTree {
    /// The tree under task `root` of `rows`; `None` when `root` is not a row. A sub-task that is
    /// not a row, or that is already one of its own parents, is left out.
    pub fn build(rows: &[TaskRow], root: &str) -> Option<Self> {
        let by_id: HashMap<String, &TaskRow> = rows
            .iter()
            .map(|row| (row.task_id.to_lowercase(), row))
            .collect();
        Self::build_with(&|id| by_id.get(&id.to_lowercase()).copied(), root)
    }

    /// The tree under task `root`, with `find` giving the row of a task id.
    fn build_with<'r>(find: &dyn Fn(&str) -> Option<&'r TaskRow>, root: &str) -> Option<Self> {
        let mut tree = Self {
            tasks: Vec::new(),
            wire_index: Vec::new(),
        };
        let mut path = Vec::new();
        tree.add(find, root, None, &mut path)?;
        tree.wire_index = vec![0; tree.tasks.len()];
        let mut next = 0;
        tree.number_children_first(0, &mut next);
        Some(tree)
    }

    /// Numbers task `index` after its children, from `next` on (the wire number is a `u8`; a
    /// tree of more than 256 tasks wraps).
    fn number_children_first(&mut self, index: usize, next: &mut usize) {
        for child in self.tasks[index].children.clone() {
            self.number_children_first(child, next);
        }
        self.wire_index[index] = *next as u8;
        *next += 1;
    }

    fn add<'r>(
        &mut self,
        find: &dyn Fn(&str) -> Option<&'r TaskRow>,
        id: &str,
        parent: Option<usize>,
        path: &mut Vec<String>,
    ) -> Option<usize> {
        let key = id.to_lowercase();
        if path.contains(&key) {
            return None;
        }
        let row = find(&key)?;
        let index = self.tasks.len();
        self.tasks.push(Task {
            id: row.task_id.clone(),
            kind: kind_of(row),
            parent,
            children: Vec::new(),
        });
        path.push(key);
        let children: Vec<usize> = row
            .sub_tasks
            .iter()
            .filter_map(|child| self.add(find, child, Some(index), path))
            .collect();
        path.pop();
        self.tasks[index].children = children;
        Some(index)
    }

    /// Tasks in the tree.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Whether the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

/// A game event that can complete tasks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event<'a> {
    /// A cinematic video started (CRC-32 of its lowercase id).
    CinematicStarted(u32),
    /// A cinematic video ended.
    CinematicEnded(u32),
    /// An in-game cutscene ended (CRC-32 of its lowercase name).
    CutsceneEnded(u32),
    /// The player killed a mob with these vitals id and categories.
    Killed {
        /// Its vitals id.
        vitals_id: &'a str,
        /// Its `VitalsCategories`.
        categories: &'a [String],
    },
    /// The player is here.
    Moved(Position),
    /// The player got this many of this item.
    ItemGained {
        /// The item id.
        item: &'a str,
        /// How many.
        quantity: u32,
    },
    /// The player took all items of a container.
    ContainerLooted,
    /// The player equipped this item.
    Equipped(&'a str),
    /// The client opened this UI screen (CRC-32 of its lowercase name).
    UiScreenOpened(u32),
    /// The player got this achievement.
    AchievementSet(&'a str),
    /// The player crafted the recipe of this id.
    Crafted(&'a str),
}

/// Where a task the quest tracker shows stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// It runs.
    Running,
    /// It is done.
    Done,
    /// It stopped undone: a branch of a partial container that another branch finished.
    Stopped,
}

/// A task the quest tracker shows: a task that is done, or that runs and has progress, or that
/// stopped after it had some (objectives-state note, section 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskProgress {
    /// Its number on the wire (children first).
    pub index: u8,
    /// CRC-32 of the lowercase id.
    pub task: u32,
    /// Children done for a container; kills or items so far for a counting task; 1 for another
    /// task that is done.
    pub progress: u32,
    /// Where it stands.
    pub status: TaskStatus,
}

/// What the tree asks of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Set this achievement.
    Achievement(String),
    /// Clear this achievement.
    LockAchievement(String),
    /// Remove this item from the player.
    RemoveItem(String),
    /// This task is done.
    Done(String),
}

/// The state of one task in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskState {
    /// Not started, or stopped because a partial container ended.
    Idle,
    /// Waiting: kills or items so far, and a timer's end.
    Active { progress: u32, due_ms: Option<u64> },
    /// Done.
    Done,
}

/// One player's run of an objective tree.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectiveRun {
    tree: Arc<ObjectiveTree>,
    states: Vec<TaskState>,
    /// The tasks with progress, in the order they first had it (retail keeps an entry's place
    /// and adds new ones at the end).
    listed: Vec<usize>,
}

/// Whether `enemy` names a mob of `vitals_id` and `categories`: one of its categories or its
/// vitals id, or the start of its vitals id (live-tutorial note: `Legion_Legionnaire` counts
/// `Legion_Legionnaire_FTUE`). Case does not matter.
fn kills_count(enemy: &str, vitals_id: &str, categories: &[String]) -> bool {
    let names = |name: &str| {
        name.eq_ignore_ascii_case(enemy)
            || name.len() > enemy.len()
                && name.is_char_boundary(enemy.len())
                && name[..enemy.len()].eq_ignore_ascii_case(enemy)
                && name[enemy.len()..].starts_with(ID_PART_SEPARATOR)
    };
    names(vitals_id) || categories.iter().any(|category| names(category))
}

/// The categories of a `VitalsCategories` cell.
pub fn categories_of(cell: &str) -> Vec<String> {
    cell.split(CATEGORY_SEPARATOR)
        .map(str::trim)
        .filter(|category| !category.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The vitals id and categories of every mob type, by the CRC-32 of its lowercase vitals id: what
/// a kill task matches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KillTypes {
    by_vitals: HashMap<u32, (String, Vec<String>)>,
}

impl KillTypes {
    /// The kill types of `VitalsBaseData` in `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .vitals_base_data
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| {
                    let categories = row.vitals_categories.clone().unwrap_or_default();
                    (row.vitals_id.clone(), categories)
                }),
        )
    }

    /// The kill types of `rows`: (vitals id, `VitalsCategories` cell).
    pub fn build(rows: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            by_vitals: rows
                .into_iter()
                .map(|(vitals_id, cell)| {
                    let categories = categories_of(&cell);
                    (lowercase_crc32(&vitals_id), (vitals_id, categories))
                })
                .collect(),
        }
    }

    /// The vitals id and categories of vitals id `vitals_id`.
    pub fn of(&self, vitals_id: u32) -> Option<(&str, &[String])> {
        self.by_vitals
            .get(&vitals_id)
            .map(|(id, categories)| (id.as_str(), categories.as_slice()))
    }
}

impl ObjectiveRun {
    /// A run of `tree`, started at `now_ms`; the effects of its first tasks.
    pub fn start(tree: Arc<ObjectiveTree>, now_ms: u64) -> (Self, Vec<Effect>) {
        let states = vec![TaskState::Idle; tree.tasks.len()];
        let mut run = Self {
            tree,
            states,
            listed: Vec::new(),
        };
        let mut effects = Vec::new();
        if !run.states.is_empty() {
            run.activate(0, now_ms, &mut effects);
        }
        (run, effects)
    }

    /// The run as a handoff carries it at `now_ms`, with the client's quest tracker at
    /// `tracker_version`.
    pub fn export(&self, now_ms: u64, tracker_version: u64) -> QuestRun {
        QuestRun {
            tasks: self
                .states
                .iter()
                .map(|state| match *state {
                    TaskState::Idle => QuestTask::Idle,
                    TaskState::Active { progress, due_ms } => QuestTask::Active {
                        progress,
                        due_in_ms: due_ms.map(|due| due.saturating_sub(now_ms)),
                    },
                    TaskState::Done => QuestTask::Done,
                })
                .collect(),
            listed: self
                .listed
                .iter()
                .filter_map(|index| u32::try_from(*index).ok())
                .collect(),
            tracker_version,
        }
    }

    /// The run of `tree` that `run` carried, going on at `now_ms`; `None` when `run` does not
    /// fit `tree` (another tree or a task out of it).
    pub fn resume(tree: Arc<ObjectiveTree>, run: &QuestRun, now_ms: u64) -> Option<Self> {
        if run.tasks.len() != tree.tasks.len() {
            return None;
        }
        let listed = run
            .listed
            .iter()
            .map(|index| {
                usize::try_from(*index)
                    .ok()
                    .filter(|index| *index < tree.tasks.len())
            })
            .collect::<Option<Vec<usize>>>()?;
        let states = run
            .tasks
            .iter()
            .map(|task| match *task {
                QuestTask::Idle => TaskState::Idle,
                QuestTask::Active {
                    progress,
                    due_in_ms,
                } => TaskState::Active {
                    progress,
                    due_ms: due_in_ms.map(|left| now_ms.saturating_add(left)),
                },
                QuestTask::Done => TaskState::Done,
            })
            .collect();
        Some(Self {
            tree,
            states,
            listed,
        })
    }

    /// Whether the whole tree is done.
    pub fn finished(&self) -> bool {
        self.states.first() == Some(&TaskState::Done)
    }

    /// Whether task `id` is done.
    pub fn is_done(&self, id: &str) -> bool {
        self.tree
            .tasks
            .iter()
            .zip(&self.states)
            .any(|(task, state)| *state == TaskState::Done && task.id.eq_ignore_ascii_case(id))
    }

    /// Whether task `id` waits.
    pub fn is_active(&self, id: &str) -> bool {
        self.tree
            .tasks
            .iter()
            .zip(&self.states)
            .any(|(task, state)| {
                matches!(state, TaskState::Active { .. }) && task.id.eq_ignore_ascii_case(id)
            })
    }

    /// The tasks the quest tracker shows, in the order they first had progress.
    pub fn progress(&self) -> Vec<TaskProgress> {
        self.listed
            .iter()
            .map(|index| {
                let task = &self.tree.tasks[*index];
                // A listed task that is idle had progress, then its partial container stopped it.
                let status = match self.states[*index] {
                    TaskState::Done => TaskStatus::Done,
                    TaskState::Active { .. } => TaskStatus::Running,
                    TaskState::Idle => TaskStatus::Stopped,
                };
                let progress = match (&task.kind, self.states[*index]) {
                    (TaskKind::Consecutive | TaskKind::Partial | TaskKind::Simple, _) => {
                        task.children
                            .iter()
                            .filter(|child| self.states[**child] == TaskState::Done)
                            .count() as u32
                    }
                    (
                        TaskKind::KillContribution { count, .. }
                        | TaskKind::HaveItems { count, .. }
                        | TaskKind::GatherItems { count, .. },
                        TaskState::Done,
                    ) => *count,
                    (_, TaskState::Active { progress, .. }) => progress,
                    _ => 1,
                };
                TaskProgress {
                    index: self.tree.wire_index[*index],
                    task: lowercase_crc32(&task.id),
                    progress,
                    status,
                }
            })
            .collect()
    }

    /// Completes the waiting item tasks that the player's items `held` (item id, quantity,
    /// worn) already meet: a have-item task by any stack of its item, a have-items task by its
    /// count of the item, an equip task by its item worn; a have-items task short of its count
    /// shows how many the player holds. Item events count only for tasks that wait when they
    /// come; an item the player took before its task started would else never count
    /// (beach-ruins-chest note: the chest's potions taken before `ChestDiscovery`).
    pub fn sync_held(&mut self, held: &[(&str, u32, bool)], now_ms: u64) -> Vec<Effect> {
        let mut effects = Vec::new();
        for index in 0..self.states.len() {
            let TaskState::Active { progress, due_ms } = self.states[index] else {
                continue;
            };
            let holds = |item: &str| -> u32 {
                held.iter()
                    .filter(|(id, _, _)| id.eq_ignore_ascii_case(item))
                    .map(|(_, quantity, _)| *quantity)
                    .sum()
            };
            let tree = Arc::clone(&self.tree);
            let met = match &tree.tasks[index].kind {
                TaskKind::HaveItem(Some(item)) => holds(item) > 0,
                TaskKind::HaveItems { item, count } => {
                    let holding = holds(item);
                    if holding < *count && holding != progress {
                        self.states[index] = TaskState::Active {
                            progress: holding,
                            due_ms,
                        };
                        self.list(index);
                    }
                    holding >= *count
                }
                TaskKind::EquipItem(item) => held
                    .iter()
                    .any(|(id, _, worn)| *worn && id.eq_ignore_ascii_case(item)),
                _ => false,
            };
            if met {
                self.complete(index, now_ms, &mut effects);
            }
        }
        effects
    }

    /// The items the waiting gather tasks drop for a corpse of `vitals_id` and `categories`
    /// that the player skins: each item and its chance in percent.
    pub fn quest_drops(&self, vitals_id: &str, categories: &[String]) -> Vec<(String, u32)> {
        self.tree
            .tasks
            .iter()
            .zip(&self.states)
            .filter(|(_, state)| matches!(state, TaskState::Active { .. }))
            .filter_map(|(task, _)| match &task.kind {
                TaskKind::GatherItems {
                    item,
                    drop: Some(drop),
                    ..
                } if kills_count(&drop.enemy, vitals_id, categories) => {
                    Some((item.clone(), drop.percent))
                }
                _ => None,
            })
            .collect()
    }

    /// Lists task `index` in the tracker once it has progress.
    fn list(&mut self, index: usize) {
        if !self.listed.contains(&index) {
            self.listed.push(index);
        }
    }

    /// The effects of `event` at `now_ms`.
    pub fn event(&mut self, event: Event<'_>, now_ms: u64) -> Vec<Effect> {
        let mut effects = Vec::new();
        // Only tasks waiting before the event take it: a task it starts does not.
        let waiting: Vec<usize> = (0..self.states.len())
            .filter(|index| matches!(self.states[*index], TaskState::Active { .. }))
            .collect();
        for index in waiting {
            // A task may have been closed by an earlier one in this loop.
            let TaskState::Active { progress, due_ms } = self.states[index] else {
                continue;
            };
            // Whether the event counts, how many it counts and how many the task needs.
            let (matches, step, count) = match (&self.tree.tasks[index].kind, event) {
                (TaskKind::CinematicStart(id), Event::CinematicStarted(seen))
                | (TaskKind::CinematicComplete(id), Event::CinematicEnded(seen))
                | (TaskKind::CutsceneComplete(id), Event::CutsceneEnded(seen))
                | (TaskKind::OpenUiScreen(id), Event::UiScreenOpened(seen)) => (*id == seen, 1, 1),
                (
                    TaskKind::KillContribution { enemy, count },
                    Event::Killed {
                        vitals_id,
                        categories,
                    },
                ) => (kills_count(enemy, vitals_id, categories), 1, *count),
                (
                    TaskKind::TriggerArea {
                        at: Some(at),
                        radius_m,
                    },
                    Event::Moved(position),
                ) => (at.horizontal_distance(position) <= *radius_m, 1, 1),
                (TaskKind::HaveItem(Some(wanted)), Event::ItemGained { item, .. })
                | (TaskKind::EquipItem(wanted), Event::Equipped(item))
                | (TaskKind::CraftRecipe(wanted), Event::Crafted(item)) => {
                    (wanted.eq_ignore_ascii_case(item), 1, 1)
                }
                (
                    TaskKind::GatherItems {
                        item: wanted,
                        count,
                        ..
                    },
                    Event::ItemGained { item, quantity },
                ) => (wanted.eq_ignore_ascii_case(item), quantity, *count),
                (TaskKind::HaveItem(None), Event::ContainerLooted) => (true, 1, 1),
                (TaskKind::HasAchievement(wanted), Event::AchievementSet(set)) => {
                    (wanted.eq_ignore_ascii_case(set), 1, 1)
                }
                _ => (false, 1, 1),
            };
            if !matches {
                continue;
            }
            let progress = progress.saturating_add(step);
            if progress >= count {
                self.complete(index, now_ms, &mut effects);
            } else {
                self.states[index] = TaskState::Active { progress, due_ms };
                self.list(index);
            }
        }
        effects
    }

    /// The effects of the timers due at `now_ms`.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Effect> {
        let mut effects = Vec::new();
        for index in 0..self.states.len() {
            if let TaskState::Active {
                due_ms: Some(due), ..
            } = self.states[index]
                && now_ms >= due
            {
                self.complete(index, now_ms, &mut effects);
            }
        }
        effects
    }

    fn activate(&mut self, index: usize, now_ms: u64, effects: &mut Vec<Effect>) {
        let task = &self.tree.tasks[index];
        let due_ms = match task.kind {
            TaskKind::Timer { duration_ms } => Some(now_ms.saturating_add(duration_ms)),
            _ => None,
        };
        self.states[index] = TaskState::Active {
            progress: 0,
            due_ms,
        };
        let tree = Arc::clone(&self.tree);
        let task = &tree.tasks[index];
        match &task.kind {
            TaskKind::Consecutive => match task.children.first() {
                Some(first) => self.activate(*first, now_ms, effects),
                None => self.complete(index, now_ms, effects),
            },
            TaskKind::Partial | TaskKind::Simple => {
                if task.children.is_empty() {
                    self.complete(index, now_ms, effects);
                }
                for child in &task.children {
                    // A partial container may be done by an earlier child already.
                    if matches!(self.states[index], TaskState::Active { .. }) {
                        self.activate(*child, now_ms, effects);
                    }
                }
            }
            TaskKind::UnlockAchievement(achievement) => {
                effects.push(Effect::Achievement(achievement.clone()));
                self.complete(index, now_ms, effects);
            }
            TaskKind::RemoveItem(item) => {
                effects.push(Effect::RemoveItem(item.clone()));
                self.complete(index, now_ms, effects);
            }
            TaskKind::LockAchievement(achievement) => {
                effects.push(Effect::LockAchievement(achievement.clone()));
                self.complete(index, now_ms, effects);
            }
            TaskKind::Unsupported => self.complete(index, now_ms, effects),
            _ => {}
        }
    }

    fn complete(&mut self, index: usize, now_ms: u64, effects: &mut Vec<Effect>) {
        self.states[index] = TaskState::Done;
        self.list(index);
        let tree = Arc::clone(&self.tree);
        let task = &tree.tasks[index];
        effects.push(Effect::Done(task.id.clone()));
        let Some(parent) = task.parent else {
            return;
        };
        self.list(parent);
        if !matches!(self.states[parent], TaskState::Active { .. }) {
            return;
        }
        let siblings = &tree.tasks[parent].children;
        match tree.tasks[parent].kind {
            TaskKind::Consecutive => {
                let next = siblings
                    .iter()
                    .skip_while(|sibling| **sibling != index)
                    .nth(1);
                match next {
                    Some(next) => self.activate(*next, now_ms, effects),
                    None => self.complete(parent, now_ms, effects),
                }
            }
            TaskKind::Partial => {
                for sibling in siblings {
                    self.stop(*sibling);
                }
                self.complete(parent, now_ms, effects);
            }
            TaskKind::Simple
                if siblings
                    .iter()
                    .all(|sibling| self.states[*sibling] == TaskState::Done) =>
            {
                self.complete(parent, now_ms, effects);
            }
            _ => {}
        }
    }

    /// Stops task `index` and its children unless done.
    fn stop(&mut self, index: usize) {
        if self.states[index] == TaskState::Done {
            return;
        }
        self.states[index] = TaskState::Idle;
        let tree = Arc::clone(&self.tree);
        for child in &tree.tasks[index].children {
            self.stop(*child);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const NOW: u64 = 1000;
    const TIMER_S: f64 = 2.0;
    const TIMER_MS: u64 = 2000;
    const AREA: Position = Position {
        x: 755.87,
        y: 342.49,
        z: 20.9,
    };

    fn row(task_id: &str, kind: &str) -> TaskRow {
        TaskRow {
            task_id: task_id.to_owned(),
            kind: kind.to_owned(),
            ..TaskRow::default()
        }
    }

    fn with_item(task_id: &str, kind: &str, item: &str) -> TaskRow {
        TaskRow {
            item_name: Some(item.to_owned()),
            ..row(task_id, kind)
        }
    }

    fn container(task_id: &str, kind: &str, children: &[&str]) -> TaskRow {
        TaskRow {
            sub_tasks: children.iter().map(|child| (*child).to_owned()).collect(),
            ..row(task_id, kind)
        }
    }

    /// A small tree in the shape of the tutorial's start: an unlock, a video start, a timer then
    /// an unlock, a video end, two kills of a category, a weapon pick in a partial container of
    /// simple containers, an area, and a cycle that is left out.
    fn rows() -> Vec<TaskRow> {
        vec![
            container(
                "Root",
                "ConsecutiveTaskContainer",
                &[
                    "Unlock", "Start", "Wait", "End", "Kills", "Pick", "Area", "Root",
                ],
            ),
            with_item("Unlock", "TaskUnlockAchievement", "Intro_Unlock"),
            with_item("Start", "TaskCinematicStart", "FTUE_Intro2"),
            container("Wait", "ConsecutiveTaskContainer", &["Timer", "Unlock2"]),
            TaskRow {
                duration: Some(TIMER_S),
                ..row("Timer", "TaskTimer")
            },
            with_item("Unlock2", "TaskUnlockAchievement", "Crew_Unlock"),
            with_item("End", "TaskCinematicComplete", "FTUE_Intro2"),
            TaskRow {
                kill_enemy_type: Some("Legion_Legionnaire".to_owned()),
                target_qty: Some(2),
                ..row("Kills", "TaskKillContribution")
            },
            container("Pick", "PartialTaskContainer", &["Soldier", "Destroyer"]),
            container("Soldier", "SimpleTaskContainer", &["Sword", "Hatchet"]),
            with_item("Sword", "TaskEquipItem", "SoldierArchetype_1hLongsword"),
            with_item("Hatchet", "TaskEquipItem", "SoldierArchetype_1hHatchet"),
            container("Destroyer", "SimpleTaskContainer", &["Axe", "Hammer"]),
            with_item("Axe", "TaskEquipItem", "DestroyerArchetype_2hGreataxe"),
            with_item("Hammer", "TaskEquipItem", "DestroyerArchetype_2hWarhammer"),
            TaskRow {
                override_location: Some("755.87 342.49 20.90".to_owned()),
                ..row("Area", "TaskTriggerArea")
            },
        ]
    }

    fn achievements(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Achievement(name) => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_cycle_is_left_out_of_the_tree() {
        let tree = ObjectiveTree::build(&rows(), "root").expect("tree");
        assert_eq!(tree.len(), rows().len());
        assert!(ObjectiveTree::build(&rows(), "Nothing").is_none());
    }

    #[test]
    fn the_tracker_lists_tasks_with_progress_numbered_children_first() {
        let tree = Arc::new(ObjectiveTree::build(&rows(), "Root").expect("tree"));
        // Children first: Unlock 0, Start 1, Timer 2, Unlock2 3, Wait 4, End 5, Kills 6,
        // Sword 7, Hatchet 8, Soldier 9, Axe 10, Hammer 11, Destroyer 12, Pick 13, Area 14,
        // Root 15.
        let (mut run, _) = ObjectiveRun::start(tree, NOW);
        let entry = |index, task: &str, progress, done: bool| TaskProgress {
            index,
            task: lowercase_crc32(task),
            progress,
            status: if done {
                TaskStatus::Done
            } else {
                TaskStatus::Running
            },
        };
        assert_eq!(
            run.progress(),
            [entry(0, "Unlock", 1, true), entry(15, "Root", 1, false)],
            "the done unlock and its container, as the retail first card"
        );
        let categories = categories_of("Legion_Legionnaire");
        run.event(Event::CinematicStarted(lowercase_crc32("FTUE_Intro2")), NOW);
        run.tick(NOW + TIMER_MS);
        run.event(Event::CinematicEnded(lowercase_crc32("FTUE_Intro2")), NOW);
        run.event(
            Event::Killed {
                vitals_id: "Legion_Legionnaire_FTUE",
                categories: &categories,
            },
            NOW,
        );
        let progress = run.progress();
        assert!(
            progress.contains(&entry(6, "Kills", 1, false)),
            "{progress:?}"
        );
        assert!(progress.contains(&entry(4, "Wait", 2, true)));
        assert!(progress.contains(&entry(15, "Root", 4, false)));
    }

    #[test]
    fn a_task_type_is_read_in_any_case() {
        // Retail sheets: `TASKUNLOCKACHIEVEMENT` and `TasklockAchievement`.
        assert_eq!(
            kind_of(&with_item("Up", "TASKUNLOCKACHIEVEMENT", "Pushed")),
            TaskKind::UnlockAchievement("Pushed".to_owned())
        );
        assert_eq!(
            kind_of(&with_item("Down", "TasklockAchievement", "Pushed")),
            TaskKind::LockAchievement("Pushed".to_owned())
        );
    }

    #[test]
    fn a_lock_clears_an_achievement_and_a_wait_ends_when_one_is_set() {
        let rows = vec![
            container("Root", "ConsecutiveTaskContainer", &["Lock", "Wait"]),
            with_item(
                "Lock",
                "TaskLockAchievement",
                "SwapActive_01_CaveCorruption",
            ),
            with_item("Wait", "TaskAchievement", "Watched_IGC_Aelstrom_MB"),
        ];
        let tree = Arc::new(ObjectiveTree::build(&rows, "Root").expect("tree"));
        let (mut run, effects) = ObjectiveRun::start(tree, NOW);
        assert!(effects.contains(&Effect::LockAchievement(
            "SwapActive_01_CaveCorruption".to_owned()
        )));
        assert!(
            run.event(Event::AchievementSet("Other"), NOW).is_empty(),
            "another achievement"
        );
        run.event(Event::AchievementSet("watched_igc_aelstrom_mb"), NOW);
        assert!(run.finished());
    }

    #[test]
    fn the_tree_runs_on_events_in_order() {
        let tree = Arc::new(ObjectiveTree::build(&rows(), "Root").expect("tree"));
        let (mut run, effects) = ObjectiveRun::start(tree, NOW);
        assert_eq!(achievements(&effects), ["Intro_Unlock"]);
        // The video end waits for its start and the timer.
        let intro = lowercase_crc32("FTUE_Intro2");
        assert!(run.event(Event::CinematicEnded(intro), NOW).is_empty());
        run.event(Event::CinematicStarted(intro), NOW);
        assert!(achievements(&run.tick(NOW + TIMER_MS - 1)).is_empty());
        assert_eq!(achievements(&run.tick(NOW + TIMER_MS)), ["Crew_Unlock"]);
        run.event(Event::CinematicEnded(intro), NOW);
        assert!(run.is_done("end"));
        let categories = categories_of("Legion_Legionnaire_FTUE,Hostile,Corrupted");
        let kill = Event::Killed {
            vitals_id: "Legion_Legionnaire_FTUE",
            categories: &categories,
        };
        run.event(kill, NOW);
        assert!(!run.is_done("Kills"));
        run.event(kill, NOW);
        assert!(run.is_done("Kills"));
        // One weapon of one archetype does not finish the pick; both do.
        run.event(Event::Equipped("DestroyerArchetype_2hGreataxe"), NOW);
        assert!(!run.is_done("Pick"));
        let effects = run.event(Event::Equipped("DestroyerArchetype_2hWarhammer"), NOW);
        assert!(effects.contains(&Effect::Done("Pick".to_owned())));
        // The other archetype's weapons no longer count.
        assert!(
            run.event(Event::Equipped("SoldierArchetype_1hHatchet"), NOW)
                .is_empty()
        );
        let far = Position {
            x: AREA.x + TRIGGER_RADIUS_M + 1.0,
            ..AREA
        };
        assert!(run.event(Event::Moved(far), NOW).is_empty());
        run.event(Event::Moved(AREA), NOW);
        assert!(run.finished());
    }

    #[test]
    fn a_run_goes_on_after_a_handoff_with_its_timer_and_kills() {
        const LATER: u64 = NOW + 50_000;
        const TRACKER_VERSION: u64 = 9;
        let tree = Arc::new(ObjectiveTree::build(&rows(), "Root").expect("tree"));
        let (mut run, _) = ObjectiveRun::start(Arc::clone(&tree), NOW);
        let intro = lowercase_crc32("FTUE_Intro2");
        run.event(Event::CinematicStarted(intro), NOW);
        let carried = run.export(NOW + 1, TRACKER_VERSION);
        assert_eq!(carried.tracker_version, TRACKER_VERSION);
        let mut resumed = ObjectiveRun::resume(Arc::clone(&tree), &carried, LATER).expect("fits");
        assert_eq!(resumed.progress(), run.progress());
        // The timer keeps what was left of it on the new clock.
        let left = TIMER_MS - 1;
        assert!(achievements(&resumed.tick(LATER + left - 1)).is_empty());
        assert_eq!(achievements(&resumed.tick(LATER + left)), ["Crew_Unlock"]);
        let other = Arc::new(ObjectiveTree::build(&rows(), "Pick").expect("tree"));
        assert!(
            ObjectiveRun::resume(other, &carried, LATER).is_none(),
            "another tree"
        );
        let mut broken = carried;
        broken.listed.push(u32::MAX);
        assert!(
            ObjectiveRun::resume(tree, &broken, LATER).is_none(),
            "a task out of the tree"
        );
    }

    #[test]
    fn items_held_before_their_task_starts_count() {
        let rows = vec![
            container(
                "Root",
                "ConsecutiveTaskContainer",
                &["Wait", "Potions", "Equip"],
            ),
            TaskRow {
                duration: Some(TIMER_S),
                ..row("Wait", "TaskTimer")
            },
            with_item("Potions", "TaskHaveAndReturnItems", "PotionHealthT1"),
            with_item("Equip", "TaskEquipItem", "PotionHealthT1"),
        ];
        let tree = Arc::new(ObjectiveTree::build(&rows, "Root").expect("tree"));
        let (mut run, _) = ObjectiveRun::start(tree, NOW);
        // The potions came while the timer ran: the event found no waiting task.
        run.event(
            Event::ItemGained {
                item: "PotionHealthT1",
                quantity: 1,
            },
            NOW,
        );
        run.tick(NOW + TIMER_MS);
        assert!(run.is_active("Potions"));
        run.sync_held(&[("PotionHealthT1", 1, false)], NOW + TIMER_MS);
        assert!(run.is_done("Potions"), "held in the bag");
        assert!(run.is_active("Equip"), "not worn yet");
        run.sync_held(&[("PotionHealthT1", 1, false)], NOW + TIMER_MS);
        assert!(run.is_active("Equip"));
        run.sync_held(&[("PotionHealthT1", 1, true)], NOW + TIMER_MS);
        assert!(run.is_done("Equip"), "worn in its quick slot");
    }

    #[test]
    fn a_tagged_area_fires_at_its_own_size() {
        let at = Position {
            x: 717.6479,
            y: 371.0387,
            z: 23.0589,
        };
        let rows = vec![
            container("Root", "ConsecutiveTaskContainer", &["Ruins"]),
            TaskRow {
                override_location: Some("717.6479 371.0387 23.0589".to_owned()),
                target_tag: Some("FTUE_EnterRuins_00_Trigger".to_owned()),
                ..row("Ruins", "TaskTriggerArea")
            },
        ];
        let tree = Arc::new(ObjectiveTree::build(&rows, "Root").expect("tree"));
        let (mut run, _) = ObjectiveRun::start(tree, NOW);
        let outside_door = Position {
            x: at.x + 15.6,
            ..at
        };
        run.event(Event::Moved(outside_door), NOW);
        assert!(!run.is_done("Ruins"), "15.6 m out, behind the door");
        let inside = Position {
            x: at.x + 6.9,
            ..at
        };
        run.event(Event::Moved(inside), NOW);
        assert!(run.is_done("Ruins"), "retail fired at 6.9 m");
        assert!((trigger_radius_m(None) - TRIGGER_RADIUS_M).abs() < f32::EPSILON);
    }

    #[test]
    fn kill_types_name_categories_and_id_starts() {
        let categories = categories_of("Corrupted_Sailor_1H_Club_FTUE, Hostile,Corrupted");
        assert!(kills_count(
            "corrupted",
            "Corrupted_Sailor_1H_Club_FTUE",
            &categories
        ));
        assert!(kills_count(
            "Legion_Legionnaire",
            "Legion_Legionnaire_FTUE",
            &[]
        ));
        assert!(!kills_count(
            "Legion_Legion",
            "Legion_Legionnaire_FTUE",
            &[]
        ));
        assert!(!kills_count(
            "Damned_Commander_FTUE",
            "Corruption_Entity",
            &categories
        ));
    }

    #[test]
    fn kill_types_are_found_by_vitals_crc() {
        let types = KillTypes::build([(
            "Corruption_Entity".to_owned(),
            "Corruption_Entity,Hostile,Corrupted".to_owned(),
        )]);
        let (id, categories) = types
            .of(lowercase_crc32("corruption_entity"))
            .expect("type");
        assert_eq!(id, "Corruption_Entity");
        assert_eq!(categories, ["Corruption_Entity", "Hostile", "Corrupted"]);
        assert_eq!(types.of(0), None);
    }

    /// The retail events of the live tutorial run (live-tutorial note, step table), in order.
    fn retail_tutorial_events() -> Vec<RetailStep> {
        use RetailStep::{Cinematic, Cutscene, Equip, Gain, Kill, Loot, Move, Start, Wait};
        let at = |x, y, z| Move(Position { x, y, z });
        vec![
            Start("ftue_intro2"),
            Wait(2_000),
            Cinematic("ftue_intro2"),
            Cutscene("FTUE_01_SoleSurvivor_ConfrontingCrewmember_IGC"),
            Kill("Corrupted_Sailor_1H_Club_FTUE"),
            Kill("Corrupted_Sailor_1H_Club_FTUE"),
            Kill("Corrupted_Sailor_1H_Club_FTUE"),
            Wait(2_000),
            Cinematic("ftue_intro3"),
            Cutscene("FTUE_01_SoleSurvivor_WaterIntro_IGC"),
            at(755.87, 342.49, 20.9),
            at(725.16, 360.02, 20.0),
            Loot,
            Equip("DestroyerArchetype_2hWarhammer"),
            Equip("DestroyerArchetype_2hGreataxe"),
            at(717.65, 371.04, 23.06),
            at(692.46, 381.35, 26.47),
            Kill("Legion_Legionnaire_FTUE"),
            at(661.06, 394.73, 26.07),
            at(663.24, 407.64, 26.47),
            Gain("PotionHealthT1"),
            Equip("PotionHealthT1"),
            Kill("Legion_Legionnaire_FTUE"),
            at(591.2, 392.56, 22.36),
            at(591.2, 392.56, 22.36),
            Kill("Corruption_Entity_Crawler_FTUE"),
            Kill("Corruption_Entity_Crawler_FTUE"),
            Kill("Corruption_Entity_Crawler_FTUE"),
            Kill("Corruption_Entity"),
            at(566.43, 371.83, 26.08),
            at(516.0, 371.0, 43.0),
            Wait(15_000),
            at(487.82, 350.29, 36.58),
            at(479.59, 294.95, 36.52),
            at(465.93, 269.1, 36.56),
            at(477.72, 232.44, 22.36),
            Cutscene("FTUE_01_SoleSurvivor2_TheFountain_IGC"),
            Cutscene("FTUE_01_SoleSurvivor2_OpeningTheBox_IGC"),
            at(489.23, 220.87, 22.0),
            Cinematic("ftue_rez"),
        ]
    }

    /// One retail event of [`retail_tutorial_events`].
    enum RetailStep {
        Start(&'static str),
        Cinematic(&'static str),
        Cutscene(&'static str),
        Kill(&'static str),
        Move(Position),
        Loot,
        Gain(&'static str),
        Equip(&'static str),
        Wait(u64),
    }

    #[test]
    #[ignore = "needs the build 25810808 content bundle in NW_CONTENT_DIR"]
    fn the_real_open_world_quest_starts_as_retail() {
        const BUILD: &str = "25810808";
        /// The open-world start: the client spawns inside the start trigger.
        const SPAWN: Position = Position {
            x: 7280.49,
            y: 3141.49,
            z: 40.12,
        };
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let path = nw_content::bundle::bundle_path(std::path::Path::new(&dir), BUILD);
        let bundle = nw_content::ContentBundle::load(&path, BUILD).expect("bundle");
        let tree = Arc::new(
            ObjectiveTree::build(
                &task_rows(&bundle.tables),
                crate::player_content::OPEN_WORLD_QUEST_ROOT,
            )
            .expect("open-world quest"),
        );
        let (mut run, mut effects) = ObjectiveRun::start(tree, NOW);
        effects.extend(run.event(Event::Moved(SPAWN), NOW));
        // Retail's first open-world state (xp-potion-exit note, section 3.3): the slice swaps and
        // the grace unlocks done, the start trigger and its unlock done.
        for wanted in [
            Effect::Achievement("SwapActive_0425_HerbalistPath_mb".to_owned()),
            Effect::LockAchievement("SwapActive_01_CaveCorruption".to_owned()),
            Effect::Achievement("SwapActive_01_UrdaMB_01".to_owned()),
            Effect::Achievement("SwapActive_01_GraceCombatLoop_mb".to_owned()),
            Effect::Achievement("SwapActive_01_GraceCombatLoop_VO_MB".to_owned()),
            Effect::Achievement("Start_IGC_Aelstrom_MB".to_owned()),
        ] {
            assert!(effects.contains(&wanted), "{wanted:?} in {effects:?}");
        }
        assert!(!run.finished());
    }

    #[test]
    #[ignore = "needs the build 25810808 content bundle in NW_CONTENT_DIR"]
    fn the_real_tutorial_quest_runs_to_its_end_on_the_retail_events() {
        const BUILD: &str = "25810808";
        /// The achievement deltas of the live run, in order (live-tutorial note, section 1).
        const RETAIL_ACHIEVEMENTS: [&str; 27] = [
            "FTUE_Intro2_Cinematic_Unlock",
            "FTUE_PCCM_Cinematic_Unlock",
            "FTUE_Intro2_Cinematic_Watched",
            "FTUE_PCCM_Cinematic_Watched",
            "FTUE_Intro3_Cinematic_Unlock",
            "FTUE_S1_Cinematic_Unlock",
            "FTUE_Intro3_Cinematic_Watched",
            "FTUE_S1_Cinematic_Watched",
            "MSQ_Tutorial_Swimming",
            "MSQ_Tutorial_SwimDash",
            "MSQ_Tutorial_Loot",
            "MSQ_Tutorial_Inventory",
            "FTUE_AncientRuins_Door",
            "Tutorial_TargetLock1",
            "Tutorial_TargetLock2",
            "MSQ_Tutorial_Climb",
            "Tutorial_TargetLockSwitch1",
            "MSQ_Tutorial_WeaponMastery",
            "FTUE_Vista_Cinematic_Unlock",
            "FTUE_Vista_Cinematic_Watched",
            "Tutorial_Zoom",
            "FTUE_TF_Cinematic_Unlock",
            "FTUE_TF_Cinematic_Watched",
            "FTUE_OTB_Cinematic_Watched",
            "FTUE_Rez_Cinematic_Unlock",
            "FTUE_Rez_Cinematic_Watched",
            "SoleSurvivor02_Check",
        ];
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let path = nw_content::bundle::bundle_path(std::path::Path::new(&dir), BUILD);
        let bundle = nw_content::ContentBundle::load(&path, BUILD).expect("bundle");
        let tree = Arc::new(
            ObjectiveTree::build(
                &task_rows(&bundle.tables),
                crate::player_content::TUTORIAL_QUEST_ROOT,
            )
            .expect("tutorial quest"),
        );
        let kill_types = KillTypes::from_tables(&bundle.tables);
        let (mut run, mut effects) = ObjectiveRun::start(tree, NOW);
        let mut now = NOW;
        for step in retail_tutorial_events() {
            let more = match step {
                RetailStep::Start(video) => {
                    run.event(Event::CinematicStarted(lowercase_crc32(video)), now)
                }
                RetailStep::Cinematic(video) => {
                    run.event(Event::CinematicEnded(lowercase_crc32(video)), now)
                }
                RetailStep::Cutscene(cutscene) => {
                    run.event(Event::CutsceneEnded(lowercase_crc32(cutscene)), now)
                }
                RetailStep::Kill(vitals) => {
                    let (vitals_id, categories) =
                        kill_types.of(lowercase_crc32(vitals)).expect("kill type");
                    run.event(
                        Event::Killed {
                            vitals_id,
                            categories,
                        },
                        now,
                    )
                }
                RetailStep::Move(position) => run.event(Event::Moved(position), now),
                RetailStep::Loot => run.event(Event::ContainerLooted, now),
                RetailStep::Gain(item) => run.event(Event::ItemGained { item, quantity: 1 }, now),
                RetailStep::Equip(item) => run.event(Event::Equipped(item), now),
                RetailStep::Wait(ms) => {
                    now += ms;
                    run.tick(now)
                }
            };
            effects.extend(more);
        }
        assert!(run.finished(), "the quest ends after the last video");
        // The wire numbers of the retail tracker (objectives-state note, section 4).
        let progress = run.progress();
        let index_of = |task: &str| {
            progress
                .iter()
                .find(|entry| entry.task == lowercase_crc32(task))
                .map(|entry| entry.index)
        };
        assert_eq!(index_of("01_SoleSurvivor2_FTUE_Intro2_Unlock"), Some(0));
        assert_eq!(
            index_of("01_SoleSurvivor2_Container_BK2_FTUE_Intro2"),
            Some(7)
        );
        assert_eq!(index_of("01_SoleSurvivor2_Container_Boat"), Some(24));
        assert_eq!(index_of("01_SoleSurvivor2_Container"), Some(133));
        let mut set = achievements(&effects);
        // The tree sets `FTUE_OTB_Cinematic_Watched` twice; the second changes nothing.
        set.dedup();
        assert_eq!(set, RETAIL_ACHIEVEMENTS);
        assert!(effects.contains(&Effect::RemoveItem("1hSwordT2_FTUE".to_owned())));
    }

    /// The retail task rows of `12_skinningknife` and `12_boarfeast` (build 25810808), as their
    /// trees use them.
    pub(crate) fn beach_rows() -> Vec<TaskRow> {
        let unlock =
            |id: &str, achievement: &str| with_item(id, "TaskUnlockAchievement", achievement);
        let have = |id: &str, item: &str| TaskRow {
            target_qty: Some(1),
            ..with_item(id, "TaskHaveItems", item)
        };
        vec![
            container(
                "12_skinningknife_container",
                "ConsecutiveTaskContainer",
                &[
                    "12_skinningknife_01_Walkaway_01_Aelstrom_p01_Off",
                    "12_skinningknife_02_TutorialUnlock",
                    "12_skinningknife_03_timerAndGather_container_partial",
                ],
            ),
            unlock(
                "12_skinningknife_01_Walkaway_01_Aelstrom_p01_Off",
                "SwapActive_01_AelstromMB_01",
            ),
            unlock(
                "12_skinningknife_02_TutorialUnlock",
                "tutorial_gathering_bushflint",
            ),
            container(
                "12_skinningknife_03_timerAndGather_container_partial",
                "PartialTaskContainer",
                &[
                    "12_skinningknife_03_HaveKnife_container",
                    "12_skinningknife_03_gatherThenCraft_container",
                    "12_skinningknife_03_SwapTimerNeverComplete_container",
                ],
            ),
            container(
                "12_skinningknife_03_HaveKnife_container",
                "ConsecutiveTaskContainer",
                &[
                    "12_skinningknife_03_HaveKnife",
                    "12_skinningknife_03_SliceSwap_01_Aelstrom_p03_On",
                ],
            ),
            have("12_skinningknife_03_HaveKnife", "1hSkinningKnifeT1"),
            unlock(
                "12_skinningknife_03_SliceSwap_01_Aelstrom_p03_On",
                "SwapActive_01_AelstromMB_03",
            ),
            container(
                "12_skinningknife_03_gatherThenCraft_container",
                "ConsecutiveTaskContainer",
                &[
                    "12_skinningknife_03_gather_container",
                    "12_skinningknife_03_TaskHaveItems_1_1hSkinningKnifeT1",
                ],
            ),
            container(
                "12_skinningknife_03_gather_container",
                "SimpleTaskContainer",
                &[
                    "12_skinningknife_03_TaskHaveItems_1_FlintT1",
                    "12_skinningknife_03_TaskHaveItems_1_WoodT1",
                ],
            ),
            have("12_skinningknife_03_TaskHaveItems_1_FlintT1", "FlintT1"),
            have("12_skinningknife_03_TaskHaveItems_1_WoodT1", "WoodT1"),
            have(
                "12_skinningknife_03_TaskHaveItems_1_1hSkinningKnifeT1",
                "1hSkinningKnifeT1",
            ),
            container(
                "12_skinningknife_03_SwapTimerNeverComplete_container",
                "ConsecutiveTaskContainer",
                &[
                    "12_skinningknife_03_swapDelay",
                    "12_skinningknife_03_NeverTrue_task",
                ],
            ),
            TaskRow {
                duration: Some(TIMER_S),
                ..row("12_skinningknife_03_swapDelay", "TaskTimer")
            },
            TaskRow {
                ui_screen_tag: Some("neverbetrue".to_owned()),
                ..row("12_skinningknife_03_NeverTrue_task", "TaskGameEvent")
            },
            container(
                "12_boarfeast_container",
                "ConsecutiveTaskContainer",
                &[
                    "12_boarfeast_01_kill_1_Boar",
                    "12_boarfeast_02_equip_skinning",
                    "12_boarfeast_03_TaskGatherItems_1_RedMeatT1",
                    "12_boarfeast_05_01_TaskGameEvent_1_Craft_CookingT1",
                    "12_boarfeast_05_02_TaskHaveItems_1_LightRation",
                ],
            ),
            TaskRow {
                kill_enemy_type: Some("Boar".to_owned()),
                target_qty: Some(1),
                ..row("12_boarfeast_01_kill_1_Boar", "TaskKillContribution")
            },
            with_item(
                "12_boarfeast_02_equip_skinning",
                "TaskEquipItem",
                "1hSkinningKnifeT1",
            ),
            TaskRow {
                target_qty: Some(1),
                item_drop_vc: Some("Boar".to_owned()),
                item_drop_probability: Some(100),
                ..with_item(
                    "12_boarfeast_03_TaskGatherItems_1_RedMeatT1",
                    "TaskGatherItems",
                    "PorkT1",
                )
            },
            TaskRow {
                crafting_recipe_id: Some("Procedural_HealthRecoveryT1".to_owned()),
                ..with_item(
                    "12_boarfeast_05_01_TaskGameEvent_1_Craft_CookingT1",
                    "TaskCraftRecipe",
                    "FoodHealthRecoveryT1",
                )
            },
            have(
                "12_boarfeast_05_02_TaskHaveItems_1_LightRation",
                "FoodHealthRecoveryT1",
            ),
        ]
    }

    #[test]
    fn the_knife_ends_the_skinning_knife_quest_and_the_timer_branch_stops() {
        let tree = Arc::new(
            ObjectiveTree::build(&beach_rows(), "12_skinningknife_container").expect("tree"),
        );
        let (mut run, effects) = ObjectiveRun::start(tree, NOW);
        assert_eq!(
            achievements(&effects),
            [
                "SwapActive_01_AelstromMB_01",
                "tutorial_gathering_bushflint"
            ]
        );
        // The slice-swap timer runs out; its never-true task keeps the branch open.
        run.tick(NOW + TIMER_MS);
        assert!(run.is_done("12_skinningknife_03_swapDelay"));
        assert!(!run.finished());
        run.sync_held(&[("FlintT1", 1, false)], NOW + TIMER_MS);
        assert!(run.is_done("12_skinningknife_03_TaskHaveItems_1_FlintT1"));
        assert!(!run.is_done("12_skinningknife_03_gather_container"));
        run.sync_held(
            &[("FlintT1", 1, false), ("WoodT1", 1, false)],
            NOW + TIMER_MS,
        );
        assert!(run.is_done("12_skinningknife_03_gather_container"));
        let effects = run.sync_held(&[("1hSkinningKnifeT1", 1, false)], NOW + TIMER_MS);
        assert!(run.finished(), "the knife ends it");
        assert!(effects.contains(&Effect::Achievement(
            "SwapActive_01_AelstromMB_03".to_owned()
        )));
        let stopped = run
            .progress()
            .into_iter()
            .find(|entry| {
                entry.task
                    == lowercase_crc32("12_skinningknife_03_SwapTimerNeverComplete_container")
            })
            .expect("listed");
        assert_eq!(stopped.status, TaskStatus::Stopped, "retail state 4");
        assert_eq!(stopped.progress, 1, "its timer done");
    }

    #[test]
    fn the_boar_feast_runs_on_a_kill_an_equip_a_skinning_and_a_craft() {
        let tree =
            Arc::new(ObjectiveTree::build(&beach_rows(), "12_boarfeast_container").expect("tree"));
        let (mut run, _) = ObjectiveRun::start(tree, NOW);
        let boar = categories_of("Boar,Hostile");
        assert!(
            run.quest_drops("Boar", &boar).is_empty(),
            "no drop before its task"
        );
        run.event(
            Event::Killed {
                vitals_id: "Boar",
                categories: &boar,
            },
            NOW,
        );
        run.event(Event::Equipped("1hSkinningKnifeT1"), NOW);
        assert_eq!(run.quest_drops("Boar", &boar), [("PorkT1".to_owned(), 100)]);
        assert!(run.quest_drops("Wolf", &[]).is_empty(), "another enemy");
        run.event(
            Event::ItemGained {
                item: "PorkT1",
                quantity: 2,
            },
            NOW,
        );
        assert!(run.is_done("12_boarfeast_03_TaskGatherItems_1_RedMeatT1"));
        run.event(Event::Crafted("procedural_healthrecoveryt1"), NOW);
        assert!(!run.finished(), "the ration must be in the bag");
        run.sync_held(&[("FoodHealthRecoveryT1", 1, false)], NOW);
        assert!(run.finished());
    }

    #[test]
    fn a_have_items_task_shows_how_many_the_player_holds() {
        let rows = vec![
            container("Root", "ConsecutiveTaskContainer", &["Stones"]),
            TaskRow {
                target_qty: Some(3),
                ..with_item("Stones", "TaskHaveItems", "FlintT1")
            },
        ];
        let tree = Arc::new(ObjectiveTree::build(&rows, "Root").expect("tree"));
        let (mut run, _) = ObjectiveRun::start(tree, NOW);
        run.sync_held(&[("FlintT1", 1, false), ("FlintT1", 1, true)], NOW);
        let stones = |run: &ObjectiveRun| {
            run.progress()
                .into_iter()
                .find(|entry| entry.task == lowercase_crc32("Stones"))
                .map(|entry| (entry.progress, entry.status))
        };
        assert_eq!(stones(&run), Some((2, TaskStatus::Running)));
        run.sync_held(&[("FlintT1", 3, false)], NOW);
        assert_eq!(stones(&run), Some((3, TaskStatus::Done)));
    }

    #[test]
    fn a_location_is_three_numbers() {
        assert_eq!(location_of("755.87 342.49 20.90"), Some(AREA));
        assert_eq!(location_of("1 2"), None);
        assert_eq!(location_of("1 2 3 4"), None);
    }
}
