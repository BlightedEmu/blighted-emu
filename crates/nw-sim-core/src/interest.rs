// SPDX-License-Identifier: AGPL-3.0-or-later
//! Who sees whom (M4 spec section 7.3, M5 spec section 7.3). An observer sees at most
//! `max_visible` entities within a radius in the ground plane: the pinned ones first (a fight
//! with the observer: it must never lose sight of a foe in a crowd), then the nearest; at equal
//! distance the lower id wins. Each update says which entities entered, left, or stayed, so the
//! caller sends a full state, a removal, or changes only.

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
    max_visible: usize,
    seen: BTreeSet<Id>,
}

impl<Id: Copy + Ord> Interest<Id> {
    /// An observer that sees nothing yet, and later at most `max_visible` entities.
    pub const fn new(radius_m: f32, max_visible: usize) -> Self {
        Self {
            radius_m,
            max_visible,
            seen: BTreeSet::new(),
        }
    }

    /// Updates the seen set from the observer's position and the other entities, each with
    /// whether it is pinned. The caller leaves the observer out of `others`.
    pub fn update(
        &mut self,
        observer: Position,
        others: impl IntoIterator<Item = (Id, Position, bool)>,
    ) -> InterestChanges<Id> {
        let mut near: Vec<(bool, f32, Id)> = others
            .into_iter()
            .map(|(id, position, pinned)| (!pinned, observer.horizontal_distance(position), id))
            .filter(|(_, distance, _)| *distance <= self.radius_m)
            .collect();
        if near.len() > self.max_visible {
            near.sort_unstable_by(|a, b| {
                a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2))
            });
            near.truncate(self.max_visible);
        }
        let now: BTreeSet<Id> = near.into_iter().map(|(.., id)| id).collect();
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
    const MANY: usize = 1000;
    const TWO: usize = 2;

    fn at(x: f32) -> Position {
        Position { x, y: 0.0, z: 0.0 }
    }

    #[test]
    fn entities_enter_stay_and_leave() {
        let mut interest = Interest::new(RADIUS, MANY);
        let first = interest.update(at(0.0), [(1, at(50.0), false), (2, at(150.0), false)]);
        assert_eq!(first.entered, vec![1]);
        assert!(first.left.is_empty() && first.stayed.is_empty());
        let second = interest.update(at(60.0), [(1, at(50.0), false), (2, at(150.0), false)]);
        assert_eq!(second.entered, vec![2]);
        assert_eq!(second.stayed, vec![1]);
        let third = interest.update(at(300.0), [(2, at(250.0), false)]);
        assert_eq!(third.left, vec![1]);
        assert_eq!(third.stayed, vec![2]);
    }

    #[test]
    fn only_the_nearest_are_seen() {
        let mut interest = Interest::new(RADIUS, TWO);
        let first = interest.update(
            at(0.0),
            [
                (1, at(90.0), false),
                (2, at(10.0), false),
                (3, at(20.0), false),
            ],
        );
        assert_eq!(first.entered, vec![2, 3]);
        let second = interest.update(
            at(0.0),
            [
                (1, at(5.0), false),
                (2, at(10.0), false),
                (3, at(20.0), false),
            ],
        );
        assert_eq!(second.entered, vec![1]);
        assert_eq!(second.left, vec![3]);
        assert_eq!(second.stayed, vec![2]);
    }

    #[test]
    fn a_pinned_entity_stays_in_sight_in_a_crowd() {
        let mut interest = Interest::new(RADIUS, TWO);
        let seen = interest.update(
            at(0.0),
            [
                (1, at(5.0), false),
                (2, at(10.0), false),
                (3, at(90.0), true),
            ],
        );
        assert_eq!(
            seen.entered,
            vec![1, 3],
            "the foe far away beats the nearer stranger"
        );
        let out = interest.update(at(0.0), [(3, at(RADIUS + 1.0), true)]);
        assert_eq!(out.left, vec![1, 3], "a pin does not reach past the radius");
    }

    #[test]
    fn ties_go_to_the_lower_id() {
        let mut interest = Interest::new(RADIUS, TWO);
        let seen = interest.update(
            at(0.0),
            [
                (9, at(10.0), false),
                (4, at(10.0), false),
                (7, at(10.0), false),
            ],
        );
        assert_eq!(seen.entered, vec![4, 7]);
    }

    #[test]
    fn the_radius_is_inclusive_and_ignores_height() {
        let mut interest = Interest::new(RADIUS, MANY);
        let high = Position {
            x: RADIUS,
            y: 0.0,
            z: 1000.0,
        };
        assert_eq!(
            interest.update(at(0.0), [(7, high, false)]).entered,
            vec![7]
        );
    }
}
