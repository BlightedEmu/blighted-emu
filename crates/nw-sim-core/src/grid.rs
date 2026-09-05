// SPDX-License-Identifier: AGPL-3.0-or-later
//! Who is near a point, fast (M5 load spec section 5): entities in square buckets of the interest
//! radius. Everything within the radius of a point lies in the 3 x 3 buckets around it.

use std::collections::HashMap;

use crate::movement::Position;

/// Bucket offsets around a point: the 3 x 3 buckets.
const NEIGHBOUR_OFFSETS: [i32; 3] = [-1, 0, 1];

/// Entities in square buckets.
#[derive(Debug, Clone)]
pub struct SpatialGrid<Id> {
    bucket_m: f32,
    buckets: HashMap<(i32, i32), Vec<(Id, Position)>>,
}

impl<Id: Copy> SpatialGrid<Id> {
    /// An empty grid of buckets `bucket_m` wide; `None` unless it is a finite number above 0.
    pub fn new(bucket_m: f32) -> Option<Self> {
        (bucket_m.is_finite() && bucket_m > 0.0).then(|| Self {
            bucket_m,
            buckets: HashMap::new(),
        })
    }

    /// Adds `id` at `position`; a position that is not finite is left out.
    pub fn insert(&mut self, id: Id, position: Position) {
        if position.x.is_finite() && position.y.is_finite() {
            self.buckets
                .entry(self.bucket_of(position))
                .or_default()
                .push((id, position));
        }
    }

    /// Every entity in the 3 x 3 buckets around `position`: all those within the bucket width,
    /// and some farther ones.
    pub fn near(&self, position: Position) -> impl Iterator<Item = (Id, Position)> + '_ {
        let (column, row) = self.bucket_of(position);
        NEIGHBOUR_OFFSETS
            .iter()
            .flat_map(move |dx| {
                NEIGHBOUR_OFFSETS
                    .iter()
                    .map(move |dy| (column + dx, row + dy))
            })
            .filter_map(|bucket| self.buckets.get(&bucket))
            .flatten()
            .copied()
    }

    fn bucket_of(&self, position: Position) -> (i32, i32) {
        // The cast saturates for far values; such a point only meets its own far bucket.
        let line = |value: f32| (value / self.bucket_m).floor() as i32;
        (line(position.x), line(position.y))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;

    const RADIUS: f32 = 100.0;
    const AREA: f32 = 1000.0;

    fn at(x: f32, y: f32) -> Position {
        Position { x, y, z: 0.0 }
    }

    fn within(grid: &SpatialGrid<u32>, point: Position) -> BTreeSet<u32> {
        grid.near(point)
            .filter(|(_, position)| point.horizontal_distance(*position) <= RADIUS)
            .map(|(id, _)| id)
            .collect()
    }

    proptest! {
        #[test]
        fn the_grid_finds_what_a_full_search_finds(
            points in prop::collection::vec((0.0..AREA, 0.0..AREA), 1..200),
            query in (0.0..AREA, 0.0..AREA),
        ) {
            let mut grid = SpatialGrid::new(RADIUS).expect("grid");
            for (id, (x, y)) in points.iter().enumerate() {
                grid.insert(id as u32, at(*x, *y));
            }
            let point = at(query.0, query.1);
            let full: BTreeSet<u32> = points
                .iter()
                .enumerate()
                .filter(|(_, (x, y))| point.horizontal_distance(at(*x, *y)) <= RADIUS)
                .map(|(id, _)| id as u32)
                .collect();
            prop_assert_eq!(within(&grid, point), full);
        }
    }

    #[test]
    fn bucket_edges_are_found() {
        let mut grid = SpatialGrid::new(RADIUS).expect("grid");
        grid.insert(1, at(RADIUS, 0.0));
        grid.insert(2, at(RADIUS * 2.0 - 0.001, 0.0));
        grid.insert(3, at(-RADIUS, 0.0));
        grid.insert(4, at(f32::NAN, 0.0));
        assert_eq!(within(&grid, at(RADIUS * 1.5, 0.0)), BTreeSet::from([1, 2]));
        assert_eq!(within(&grid, at(0.0, 0.0)), BTreeSet::from([1, 3]));
        assert!(SpatialGrid::<u32>::new(0.0).is_none());
        assert!(SpatialGrid::<u32>::new(f32::NAN).is_none());
    }
}
