// SPDX-License-Identifier: AGPL-3.0-or-later
//! The objectives state of one player's quest (objectives-state note, section 8): the client's
//! quest tracker and journal show it.
//!
//! The first own card carries the tracked objective, the active objective and its task list.
//! Every later change of the task list goes whole again, at a higher version, in the bundle of the
//! same tick. When the quest ends, retail adds it to the completed list as one change, in
//! [`END_REPEATS`] bundles, then sends the tracked, active and task lists empty, in
//! [`EMPTY_REPEATS`] bundles at the next version (newchar 2026-09-18, 492.95 to 493.05 s).

use std::collections::VecDeque;

use nw_formats::objectstream::lowercase_crc32;
use nw_messages::items::FullList;
use nw_messages::{
    ListChange, ObjectiveRecord, ObjectivesState, ReplicatedList, TaskEntry, TaskWireState,
};
use uuid::Uuid;

use crate::objectives::{TaskProgress, TaskStatus};

/// The version of the first lists (retail first own card).
const FIRST_VERSION: u64 = 3;
/// How many bundles in a row carry the state of an accepted quest (retail: 4, all at one
/// version, quest-journal note).
const ACCEPT_REPEATS: usize = 4;
/// How many bundles in a row carry the completed change of an ended quest (retail: 4).
const END_REPEATS: usize = 4;
/// How many bundles in a row carry the empty lists after it (retail: 3).
const EMPTY_REPEATS: usize = 3;
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
    /// A new accept pins this quest (2709). Off when every client flag is 0.
    auto_pin: bool,
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
            auto_pin: true,
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
                self.list(self.completed.clone()).map(ReplicatedList::Full)
            },
            active: self.list(vec![ObjectiveRecord::available(
                self.objective,
                self.instance,
            )]),
            tasks: self.list(self.entries()),
        }
    }

    /// The state of the first own card, which makes every queued state moot: its lists are the
    /// newest, and the client drops a list version lower than one it has (live run 12; live
    /// 2026-09-26: a beach card at version 11 followed by the accept's four states at 10). A
    /// quest that ended sends its last state instead.
    pub(super) fn take_first_state(&mut self) -> ObjectivesState {
        let newest = self.pending.drain(..).next_back();
        match newest {
            Some(last) if self.ended => last,
            _ => self.first_state(),
        }
    }

    /// The client is in game: the whole state goes once more, at the newest lists. Its quest
    /// tracker pins only a tracked objective it gets in game (retail newchar 2026-09-18: the
    /// first card's state came every 0.34 s from 0.31 s on; the client's in-game burst, its
    /// auto-pin flags (2709) among it, came at 9.12 s, the next copy at 9.32 s and the pin (1458)
    /// at 9.37 s. Live 2026-09-26: our two copies came before the client was in game, and it
    /// pinned nothing for 171 s). A quest that ended sends nothing more.
    pub(super) fn in_game(&mut self) {
        if !self.ended {
            self.pending.push_back(self.first_state());
        }
    }

    /// Keeps the client's auto-pin choice (2709) for the next accept.
    pub(super) fn set_auto_pin(&mut self, on: bool) {
        self.auto_pin = on;
    }

    /// The same tracker with auto-pin `on`.
    pub(super) fn with_auto_pin(mut self, on: bool) -> Self {
        self.auto_pin = on;
        self
    }

    /// The player accepted the quest, whose task list is `tasks`: the client learns it is tracked
    /// and active, with its tasks, in [`ACCEPT_REPEATS`] bundles at one new version (retail reply
    /// to a 3772; without the active record the journal shows nothing, live 2026-09-18).
    /// A client that turned every auto-pin flag off gets the quest active and not tracked.
    pub(super) fn accept(&mut self, tasks: Vec<TaskProgress>) {
        self.tasks = tasks;
        self.version += 1;
        let mut state = ObjectivesState {
            completed: None,
            ..self.first_state()
        };
        if !self.auto_pin {
            state.tracked = self.list(Vec::new());
        }
        self.pending
            .extend(std::iter::repeat_n(state, ACCEPT_REPEATS));
    }

    /// Takes the quest's task list `tasks` and whether it `finished` (a quest with a turn-in NPC
    /// finishes at its turn-in, after its last task); queues the states that change the client's
    /// tracker.
    pub(super) fn update(&mut self, tasks: Vec<TaskProgress>, finished: bool) {
        if self.ended || (tasks == self.tasks && !finished) {
            return;
        }
        if tasks != self.tasks {
            self.tasks = tasks;
            self.version += 1;
            self.pending.push_back(ObjectivesState {
                tasks: self.list(self.entries()),
                ..ObjectivesState::default()
            });
        }
        if !finished {
            return;
        }
        self.ended = true;
        self.version += 1;
        let added = ListChange {
            index: u32::try_from(self.completed.len()).unwrap_or(u32::MAX),
            version: self.version,
            value: self.instance,
        };
        self.completed.push(self.instance);
        let completed = ObjectivesState {
            completed: Some(ReplicatedList::Changes(vec![added])),
            ..ObjectivesState::default()
        };
        self.pending
            .extend(std::iter::repeat_n(completed, END_REPEATS));
        self.version += 1;
        let empty = ObjectivesState {
            tracked: self.list(Vec::new()),
            active: self.list(Vec::new()),
            tasks: self.list(Vec::new()),
            completed: None,
        };
        self.pending
            .extend(std::iter::repeat_n(empty, EMPTY_REPEATS));
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
    fn an_accept_sends_the_quest_tracked_and_active_above_the_old_lists() {
        const OLD_VERSION: u64 = 40;
        let mut tracker = QuestTracker::new("12_skinningknife", PLAYER)
            .after(&[TUTORIAL], PLAYER)
            .above(OLD_VERSION);
        tracker.accept(vec![task(0, 0, false)]);
        let states: Vec<_> = std::iter::from_fn(|| tracker.take_change()).collect();
        assert_eq!(states.len(), ACCEPT_REPEATS);
        let state = &states[0];
        assert!(
            states.iter().all(|repeat| repeat == state),
            "one state, repeated"
        );
        assert_eq!(state.completed, None, "retail sends no completed list");
        let version = |list: Option<u64>| list.expect("list");
        assert!(version(state.tracked.as_ref().and_then(|list| list.version)) > OLD_VERSION);
        assert_eq!(
            state.tracked.as_ref().map(|list| list.elements.clone()),
            Some(vec![tracker.instance])
        );
        assert_eq!(
            state.active.as_ref().map(|list| list.elements.len()),
            Some(1)
        );
        assert_eq!(
            state.tasks.as_ref().map(|list| list.elements.len()),
            Some(1)
        );
        tracker.update(vec![task(0, 0, false)], false);
        assert_eq!(tracker.take_change(), None, "no change, nothing more");
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
    fn the_first_card_drops_the_older_queued_states() {
        let mut tracker = QuestTracker::new(TUTORIAL, PLAYER);
        tracker.accept(vec![task(0, 0, false)]);
        tracker.update(vec![task(0, 1, true), task(1, 0, false)], false);
        let card = tracker.take_first_state();
        assert_eq!(
            card.tasks.as_ref().and_then(|list| list.version),
            Some(tracker.version())
        );
        assert_eq!(
            card.tasks.map(|list| list.elements.len()),
            Some(2),
            "the newest list"
        );
        assert!(card.tracked.is_some() && card.active.is_some());
        assert_eq!(
            tracker.take_change(),
            None,
            "nothing older follows the card"
        );
        tracker.update(vec![task(0, 1, true), task(1, 1, true)], true);
        let ended = tracker.take_first_state();
        assert_eq!(
            ended.active.map(|list| list.elements.len()),
            Some(0),
            "an ended quest's card is its last state"
        );
    }

    #[test]
    fn a_client_in_game_gets_the_whole_state_once_more() {
        let mut tracker = QuestTracker::new(TUTORIAL, PLAYER);
        tracker.update(vec![task(0, 0, false)], false);
        let card = tracker.take_first_state();
        tracker.update(vec![task(0, 1, true), task(1, 0, false)], false);
        let change = tracker.take_change().expect("the task change");
        tracker.in_game();
        let again = tracker.take_change().expect("the state in game");
        assert_eq!(again, tracker.first_state());
        assert_eq!(
            again.tracked.map(|list| list.elements),
            card.tracked.map(|list| list.elements)
        );
        assert_eq!(
            again.tasks.and_then(|list| list.version),
            change.tasks.and_then(|list| list.version),
            "at the newest lists"
        );
        tracker.update(vec![task(0, 1, true), task(1, 1, true)], true);
        while tracker.take_change().is_some() {}
        tracker.in_game();
        assert_eq!(
            tracker.take_change(),
            None,
            "an ended quest sends nothing more"
        );
    }

    /// The instances a completed change of `state` adds.
    fn added(state: &ObjectivesState) -> Vec<(u32, u64)> {
        match &state.completed {
            Some(ReplicatedList::Changes(changes)) => changes
                .iter()
                .map(|change| (change.index, change.value))
                .collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn the_end_sends_the_retail_completed_change_then_the_empty_lists() {
        let done = vec![task(133, 5, true)];
        let mut tracker = QuestTracker::new(TUTORIAL, PLAYER).after(&["Earlier"], PLAYER);
        tracker.update(done.clone(), false);
        assert!(tracker.take_change().is_some(), "the last task list");
        tracker.update(done.clone(), true);
        for _ in 0..END_REPEATS {
            let change = tracker.take_change().expect("completed");
            assert_eq!(
                added(&change),
                [(1, tracker.instance)],
                "appended after the earlier"
            );
            assert_eq!((&change.tracked, &change.tasks), (&None, &None));
        }
        for _ in 0..EMPTY_REPEATS {
            let empty = tracker.take_change().expect("empty");
            assert_eq!(empty.tracked.map(|list| list.elements), Some(Vec::new()));
            assert!(empty.active.is_some_and(|list| list.elements.is_empty()));
            assert_eq!(empty.tasks.map(|list| list.elements), Some(Vec::new()));
            assert_eq!(empty.completed, None);
        }
        tracker.update(done, true);
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
        assert_eq!(added(&completed), [(0, tracker.instance)]);
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

    #[test]
    fn an_accept_with_auto_pin_off_is_not_tracked() {
        let mut tracker = QuestTracker::new(TUTORIAL, PLAYER);
        tracker.set_auto_pin(false);
        tracker.accept(vec![task(1, 0, false)]);
        let accepted = tracker.take_change().expect("accepted");
        assert_eq!(
            accepted.tracked.map(|list| list.elements),
            Some(Vec::new()),
            "every auto-pin flag off leaves the quest untracked"
        );
    }
}
