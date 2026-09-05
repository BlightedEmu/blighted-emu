// SPDX-License-Identifier: AGPL-3.0-or-later
//! Position saves in order (M4 spec section 7.5), with no I/O. At most one save of a character
//! runs at a time, so an older position can never land after a newer one. While it runs, only the
//! newest wanted position waits. A failed save waits for the next retry round, unless a newer
//! position already waits.

use std::collections::HashMap;

use nw_db::SavedPosition;
use uuid::Uuid;

/// The saves of every character.
#[derive(Debug, Default)]
pub struct SaveQueue {
    running: HashMap<Uuid, SavedPosition>,
    waiting: HashMap<Uuid, SavedPosition>,
}

impl SaveQueue {
    /// `character` should be saved at `position`. Returns the position when the caller must start
    /// the save now; with a save of that character running, it waits.
    pub fn want(&mut self, character: Uuid, position: SavedPosition) -> Option<SavedPosition> {
        if self.running.contains_key(&character) {
            self.waiting.insert(character, position);
            return None;
        }
        self.waiting.remove(&character);
        self.running.insert(character, position.clone());
        Some(position)
    }

    /// The running save of `character` ended. Returns the next position to start now, if any.
    pub fn finished(&mut self, character: Uuid, ok: bool) -> Option<SavedPosition> {
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
    pub fn retry(&mut self) -> Vec<(Uuid, SavedPosition)> {
        let ready: Vec<Uuid> = self
            .waiting
            .keys()
            .filter(|character| !self.running.contains_key(*character))
            .copied()
            .collect();
        ready
            .into_iter()
            .filter_map(|character| {
                let position = self.waiting.remove(&character)?;
                self.running.insert(character, position.clone());
                Some((character, position))
            })
            .collect()
    }

    /// The newest position of `character` that may not be in the database yet.
    pub fn latest(&self, character: Uuid) -> Option<&SavedPosition> {
        self.waiting
            .get(&character)
            .or_else(|| self.running.get(&character))
    }

    /// Whether no save runs.
    pub fn is_idle(&self) -> bool {
        self.running.is_empty()
    }

    /// Positions that wait for a retry.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

#[cfg(test)]
mod tests {
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
