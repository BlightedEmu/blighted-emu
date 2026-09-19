// SPDX-License-Identifier: AGPL-3.0-or-later
//! The objectives state of one player's quest (objectives-state note, section 8): the client's
//! quest tracker and journal show it.
//!
//! The first own card carries the tracked objective, the active objective and its task list.
//! Every later change of the task list goes whole again, at a higher version, in the bundle of the
//! same tick. When the quest ends, retail sends three states: the objective as completed with
//! the last task list, then no tracked objective, then all lists empty.

use std::collections::VecDeque;

use nw_formats::objectstream::lowercase_crc32;
use nw_messages::items::FullList;
use nw_messages::{ObjectiveRecord, ObjectivesState, TaskEntry, TaskWireState};
use uuid::Uuid;

use crate::objectives::{TaskProgress, TaskStatus};

/// The version of the first lists (retail first own card).
const FIRST_VERSION: u64 = 3;
/// Names the objective instance ids this server makes.
const INSTANCE_NAMESPACE: u64 = 0x6f62_6a65_6374_6976;

/// The instance id of objective `objective` (its id) for player `player`: the same on every call
/// and every sim, never 0.
fn instance_of(objective: &str, player: u64) -> u64 {
    let namespace = Uuid::from_u64_pair(INSTANCE_NAMESPACE, player);
    let (high, _) = Uuid::new_v5(&namespace, objective.as_bytes()).as_u64_pair();
    high.max(1)
}

/// The tracker state of one player's quest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct QuestTracker {
    /// CRC-32 of the lowercase objective id.
    objective: u32,
    /// The player's instance of the objective: the same on every call for a player, never 0.
    instance: u64,
    /// The version of the newest lists sent.
    version: u64,
    /// The task list of the newest state.
    tasks: Vec<TaskProgress>,
    /// The instances of the objectives the player finished before this one.
    completed: Vec<u64>,
    /// The quest ended and its end states are queued.
    ended: bool,
    /// States to send, one per bundle.
    pending: VecDeque<ObjectivesState>,
}

impl QuestTracker {
    /// The tracker of objective `objective` (its id) for player `player`.
    pub(super) fn new(objective: &str, player: u64) -> Self {
        Self {
            objective: lowercase_crc32(objective),
            instance: instance_of(objective, player),
            completed: Vec::new(),
            version: FIRST_VERSION,
            tasks: Vec::new(),
            ended: false,
            pending: VecDeque::new(),
        }
    }

    /// The version of the newest lists it made.
    pub(super) const fn version(&self) -> u64 {
        self.version
    }

    /// The tracker of a client that has lists up to `version` from another sim: its lists go
    /// on above it.
    pub(super) const fn above(mut self, version: u64) -> Self {
        self.version = version + 1;
        self
    }

    /// The tracker of a player who finished the objectives `finished` before this one (retail:
    /// the first open-world state lists the tutorial as completed).
    pub(super) fn after(mut self, finished: &[&str], player: u64) -> Self {
        self.completed = finished
            .iter()
            .map(|objective| instance_of(objective, player))
            .collect();
        self
    }

    fn list<T>(&self, elements: Vec<T>) -> Option<FullList<T>> {
        Some(FullList {
            version: Some(self.version),
            elements,
        })
    }

    fn entries(&self) -> Vec<TaskEntry> {
        self.tasks
            .iter()
            .map(|task| TaskEntry {
                instance: self.instance,
                index: task.index,
                task: task.task,
                progress: task.progress,
                state: match task.status {
                    TaskStatus::Running => TaskWireState::Active,
                    TaskStatus::Done => TaskWireState::Complete,
                    TaskStatus::Stopped => TaskWireState::Inactive,
                },
            })
            .collect()
    }

    /// The state of the first own card: the quest tracked and active, with its task list.
    pub(super) fn first_state(&self) -> ObjectivesState {
        ObjectivesState {
            tracked: self.list(vec![self.instance]),
            completed: if self.completed.is_empty() {
                None
            } else {
                self.list(self.completed.clone())
            },
            active: self.list(vec![ObjectiveRecord::available(
                self.objective,
                self.instance,
            )]),
            tasks: self.list(self.entries()),
        }
    }

    /// Takes the quest's task list `tasks` and whether it `finished` (a quest with a turn-in NPC
    /// finishes at its turn-in, after its last task); queues the states that change the client's
    /// tracker.
    pub(super) fn update(&mut self, tasks: Vec<TaskProgress>, finished: bool) {
        if self.ended || (tasks == self.tasks && !finished) {
            return;
        }
        self.tasks = tasks;
        self.version += 1;
        if !finished {
            self.pending.push_back(ObjectivesState {
                tasks: self.list(self.entries()),
                ..ObjectivesState::default()
            });
            return;
        }
        self.ended = true;
        self.pending.push_back(ObjectivesState {
            completed: self.list(vec![self.instance]),
            tasks: self.list(self.entries()),
            ..ObjectivesState::default()
        });
        self.version += 1;
        self.pending.push_back(ObjectivesState {
            tracked: self.list(Vec::new()),
            ..ObjectivesState::default()
        });
        self.version += 1;
        self.pending.push_back(ObjectivesState {
            tracked: self.list(Vec::new()),
            active: self.list(Vec::new()),
            tasks: self.list(Vec::new()),
            ..ObjectivesState::default()
        });
    }

    /// The next state to send, one per bundle.
    pub(super) fn take_change(&mut self) -> Option<ObjectivesState> {
        self.pending.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: u64 = 7;
    const TUTORIAL: &str = "01_SoleSurvivor2";

    fn task(index: u8, progress: u32, done: bool) -> TaskProgress {
        TaskProgress {
            index,
            task: u32::from(index),
            progress,
            status: if done {
                TaskStatus::Done
            } else {
                TaskStatus::Running
            },
        }
    }

    #[test]
    fn the_first_card_tracks_the_quest_and_changes_send_the_whole_task_list() {
        let mut tracker = QuestTracker::new(TUTORIAL, PLAYER);
        assert_ne!(tracker.instance, 0);
        assert_eq!(
            tracker.instance,
            QuestTracker::new(TUTORIAL, PLAYER).instance
        );
        assert_ne!(
            tracker.instance,
            QuestTracker::new(TUTORIAL, PLAYER + 1).instance
        );
        tracker.update(vec![task(0, 1, true), task(7, 1, false)], false);
        assert!(tracker.take_change().is_some());
        let first = tracker.first_state();
        assert_eq!(
            first.active.map(|list| list.elements),
            Some(vec![ObjectiveRecord::available(
                lowercase_crc32(TUTORIAL),
                tracker.instance
            )])
        );
        assert_eq!(first.tasks.map(|list| list.elements.len()), Some(2));
        tracker.update(vec![task(0, 1, true), task(7, 1, false)], false);
        assert_eq!(tracker.take_change(), None, "no change, nothing to send");
        tracker.update(vec![task(0, 1, true), task(7, 2, false)], false);
        let change = tracker.take_change().expect("change");
        assert_eq!(change.tracked, None);
        let tasks = change.tasks.expect("tasks");
        assert_eq!(tasks.version, Some(FIRST_VERSION + 2));
        assert_eq!(tasks.elements[1].progress, 2);
    }

    #[test]
    fn the_end_sends_the_three_retail_states() {
        let mut tracker = QuestTracker::new(TUTORIAL, PLAYER);
        tracker.update(vec![task(133, 5, true)], true);
        let completed = tracker.take_change().expect("completed");
        assert_eq!(
            completed.completed.map(|list| list.elements),
            Some(vec![tracker.instance])
        );
        let untracked = tracker.take_change().expect("untracked");
        assert_eq!(
            untracked.tracked.map(|list| list.elements),
            Some(Vec::new())
        );
        let empty = tracker.take_change().expect("empty");
        assert_eq!(empty.tasks.map(|list| list.elements), Some(Vec::new()));
        tracker.update(vec![task(133, 5, true)], true);
        assert_eq!(tracker.take_change(), None, "once only");
    }

    #[test]
    fn a_turn_in_ends_a_quest_whose_tasks_are_all_done_already() {
        const QUEST: &str = "12_skinningknife";
        let mut tracker = QuestTracker::new(QUEST, PLAYER);
        let done = vec![task(19, 4, true)];
        tracker.update(done.clone(), false);
        assert!(tracker.take_change().is_some(), "the last task list");
        tracker.update(done.clone(), false);
        assert_eq!(tracker.take_change(), None, "it waits for the turn-in");
        tracker.update(done, true);
        let completed = tracker.take_change().expect("completed");
        assert_eq!(
            completed.completed.map(|list| list.elements),
            Some(vec![tracker.instance])
        );
    }

    #[test]
    fn a_stopped_branch_goes_out_inactive() {
        let mut tracker = QuestTracker::new(TUTORIAL, PLAYER);
        tracker.update(
            vec![TaskProgress {
                index: 17,
                task: 17,
                progress: 2,
                status: TaskStatus::Stopped,
            }],
            false,
        );
        let change = tracker.take_change().expect("change");
        let entries = change.tasks.expect("tasks").elements;
        assert_eq!(entries[0].state, TaskWireState::Inactive);
    }
}
