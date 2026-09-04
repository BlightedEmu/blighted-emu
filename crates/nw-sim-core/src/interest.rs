// SPDX-License-Identifier: AGPL-3.0-or-later
//! Who sees whom (M4 spec section 7.3). An observer sees the entities within a radius in the
//! ground plane. Each update says which entities entered, left, or stayed, so the caller sends a
//! full state, a removal, or changes only.

use std::collections::BTreeSet;

use crate::movement::Position;

/// The result of one interest update. Each list is sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterestChanges<Id> {
    /// Entities that came inside the radius.
    pub entered: Vec<Id>,
    /// Entities that went outside the radius or are gone.
    pub left: Vec<Id>,
    /// Entities that were and still are inside the radius.
    pub stayed: Vec<Id>,
}

/// The entities one observer sees.
#[derive(Debug, Clone)]
pub struct Interest<Id> {
    radius_m: f32,
    seen: BTreeSet<Id>,
}

impl<Id: Copy + Ord> Interest<Id> {
    /// An observer that sees nothing yet.
    pub const fn new(radius_m: f32) -> Self {
        Self {
            radius_m,
            seen: BTreeSet::new(),
        }
    }

    /// Updates the seen set from the observer's position and the other entities. The caller
    /// leaves the observer out of `others`.
    pub fn update(
        &mut self,
        observer: Position,
        others: impl IntoIterator<Item = (Id, Position)>,
    ) -> InterestChanges<Id> {
        let now: BTreeSet<Id> = others
            .into_iter()
            .filter(|(_, position)| observer.horizontal_distance(*position) <= self.radius_m)
            .map(|(id, _)| id)
            .collect();
        let changes = InterestChanges {
            entered: now.difference(&self.seen).copied().collect(),
            left: self.seen.difference(&now).copied().collect(),
            stayed: now.intersection(&self.seen).copied().collect(),
        };
        self.seen = now;
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RADIUS: f32 = 100.0;

    fn at(x: f32) -> Position {
        Position { x, y: 0.0, z: 0.0 }
    }

    #[test]
    fn entities_enter_stay_and_leave() {
        let mut interest = Interest::new(RADIUS);
        let first = interest.update(at(0.0), [(1, at(50.0)), (2, at(150.0))]);
        assert_eq!(first.entered, vec![1]);
        assert!(first.left.is_empty() && first.stayed.is_empty());
        let second = interest.update(at(60.0), [(1, at(50.0)), (2, at(150.0))]);
        assert_eq!(second.entered, vec![2]);
        assert_eq!(second.stayed, vec![1]);
        let third = interest.update(at(300.0), [(2, at(250.0))]);
        assert_eq!(third.left, vec![1]);
        assert_eq!(third.stayed, vec![2]);
    }

    #[test]
    fn the_radius_is_inclusive_and_ignores_height() {
        let mut interest = Interest::new(RADIUS);
        let high = Position {
            x: RADIUS,
            y: 0.0,
            z: 1000.0,
        };
        assert_eq!(interest.update(at(0.0), [(7, high)]).entered, vec![7]);
    }
}
