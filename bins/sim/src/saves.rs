// SPDX-License-Identifier: AGPL-3.0-or-later
//! Saves in order (M4 spec section 7.5), with no I/O: positions, items (admin studio S5), quests
//! (S7) and progression (world migration 0006). At most one save of a character runs at a time,
//! so an older value can never land after a newer one. While it runs, only the newest wanted
//! value waits. A failed save waits for the next retry round, unless a newer value already
//! waits.

use std::collections::HashMap;

use nw_bus::{CarriedItem, PlayerInventory, PlayerProgression, PlayerQuests};
use uuid::Uuid;

/// The item list of a character to save, with the Unix time it was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSave {
    /// Every item stack of the character.
    pub items: Vec<CarriedItem>,
    /// Its storage chests and loadouts, saved with the items.
    pub inventory: PlayerInventory,
    /// Unix milliseconds when the list was taken: a save of an older list never overwrites a
    /// newer one.
    pub taken_at_ms: u64,
}

/// The quests of a character to save, with the Unix time they were taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestSave {
    /// The active run and the finished objectives.
    pub quests: PlayerQuests,
    /// Unix milliseconds when the state was taken: a save of an older state never overwrites
    /// a newer one.
    pub taken_at_ms: u64,
}

/// The progression of a character to save, with the Unix time it was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressionSave {
    /// Its experience, coins, achievements and loot limit counts.
    pub progression: PlayerProgression,
    /// Unix milliseconds when the state was taken: a save of an older state never overwrites
    /// a newer one.
    pub taken_at_ms: u64,
}

/// The saves of one kind of every character.
#[derive(Debug)]
pub struct SaveQueue<T> {
    running: HashMap<Uuid, T>,
    waiting: HashMap<Uuid, T>,
}

impl<T> Default for SaveQueue<T> {
    fn default() -> Self {
        Self {
            running: HashMap::new(),
            waiting: HashMap::new(),
        }
    }
}

impl<T: Clone> SaveQueue<T> {
    /// `character` should be saved with `value`. Returns the value when the caller must start
    /// the save now; with a save of that character running, it waits.
    pub fn want(&mut self, character: Uuid, value: T) -> Option<T> {
        if self.running.contains_key(&character) {
            self.waiting.insert(character, value);
            return None;
        }
        self.waiting.remove(&character);
        self.running.insert(character, value.clone());
        Some(value)
    }

    /// The running save of `character` ended. Returns the next value to start now, if any.
    pub fn finished(&mut self, character: Uuid, ok: bool) -> Option<T> {
        let done = self.running.remove(&character)?;
        if let Some(next) = self.waiting.remove(&character) {
            self.running.insert(character, next.clone());
            return Some(next);
        }
        if !ok {
            self.waiting.insert(character, done);
        }
        None
    }

    /// Starts every waiting save whose character has none running (the retry round).
    pub fn retry(&mut self) -> Vec<(Uuid, T)> {
        let ready: Vec<Uuid> = self
            .waiting
            .keys()
            .filter(|character| !self.running.contains_key(*character))
            .copied()
            .collect();
        ready
            .into_iter()
            .filter_map(|character| {
                let value = self.waiting.remove(&character)?;
                self.running.insert(character, value.clone());
                Some((character, value))
            })
            .collect()
    }

    /// The newest value of `character` that may not be in the database yet.
    pub fn latest(&self, character: Uuid) -> Option<&T> {
        self.waiting
            .get(&character)
            .or_else(|| self.running.get(&character))
    }

    /// Whether no save runs.
    pub fn is_idle(&self) -> bool {
        self.running.is_empty()
    }

    /// Values that wait for a retry.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

#[cfg(test)]
mod tests {
    use nw_db::SavedPosition;

    use super::*;

    const LEVEL: &str = "newworld_vitaeeterna";

    fn at(x: f64) -> SavedPosition {
        SavedPosition {
            level: LEVEL.to_owned(),
            x,
            y: x,
            z: x,
            yaw: 0.0,
            saved_at_ms: 0,
        }
    }

    #[test]
    fn one_save_per_character_runs_and_the_newest_position_waits() {
        let mut saves = SaveQueue::default();
        let one = Uuid::new_v4();
        let other = Uuid::new_v4();
        assert_eq!(saves.want(one, at(1.0)), Some(at(1.0)));
        assert_eq!(saves.want(other, at(9.0)), Some(at(9.0)));
        assert_eq!(saves.want(one, at(2.0)), None);
        assert_eq!(saves.want(one, at(3.0)), None);
        assert_eq!(saves.latest(one), Some(&at(3.0)));
        assert_eq!(saves.finished(one, true), Some(at(3.0)));
        assert_eq!(saves.finished(one, true), None);
        assert_eq!(saves.latest(one), None);
        assert!(!saves.is_idle());
        assert_eq!(saves.finished(other, true), None);
        assert!(saves.is_idle());
    }

    #[test]
    fn a_failed_save_waits_for_the_retry_unless_a_newer_one_waits() {
        let mut saves = SaveQueue::default();
        let one = Uuid::new_v4();
        assert_eq!(saves.want(one, at(1.0)), Some(at(1.0)));
        assert_eq!(saves.finished(one, false), None);
        assert_eq!(saves.latest(one), Some(&at(1.0)));
        assert_eq!(saves.retry(), vec![(one, at(1.0))]);
        assert!(saves.retry().is_empty());
        assert_eq!(saves.finished(one, true), None);
        assert_eq!(saves.want(one, at(2.0)), Some(at(2.0)));
        assert_eq!(saves.want(one, at(3.0)), None);
        assert_eq!(saves.finished(one, false), Some(at(3.0)));
    }
}
