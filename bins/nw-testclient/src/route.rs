// SPDX-License-Identifier: AGPL-3.0-or-later
//! The walk of the movement check (M4 spec section 9, criterion 3.3): points relative to where
//! the client stands when it starts, walked at one speed, at the start height.

use std::time::Duration;

use thiserror::Error;

/// A route that cannot be walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RouteError {
    /// Fewer than two points.
    #[error("a route needs at least two points")]
    TooShort,
    /// A point or the speed is not a finite number, or the speed is not above 0.
    #[error("route points must be finite and the speed above 0")]
    BadNumber,
    /// The walk would take longer than [`MAX_ROUTE_DURATION`].
    #[error("the walk takes too long")]
    TooLong,
}

/// Longest walk.
pub const MAX_ROUTE_DURATION: Duration = Duration::from_secs(3600);

/// Points east and north of the start, in metres, walked in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    points: Vec<[f32; 2]>,
    speed_mps: f32,
    duration: Duration,
}

impl Route {
    /// A route through `points` at `speed_mps`.
    ///
    /// # Errors
    ///
    /// Returns [`RouteError`] for fewer than two points, a number that is not finite, a speed of
    /// 0 or less, or a walk longer than [`MAX_ROUTE_DURATION`].
    pub fn new(points: Vec<[f32; 2]>, speed_mps: f32) -> Result<Self, RouteError> {
        if points.len() < 2 {
            return Err(RouteError::TooShort);
        }
        let finite = points.iter().flatten().all(|value| value.is_finite());
        if !(finite && speed_mps.is_finite() && speed_mps > 0.0) {
            return Err(RouteError::BadNumber);
        }
        let length: f32 = points
            .windows(2)
            .map(|pair| distance(pair[0], pair[1]))
            .sum();
        let duration = Duration::try_from_secs_f32(length / speed_mps)
            .ok()
            .filter(|duration| *duration <= MAX_ROUTE_DURATION)
            .ok_or(RouteError::TooLong)?;
        Ok(Self {
            points,
            speed_mps,
            duration,
        })
    }

    fn segments(&self) -> impl Iterator<Item = ([f32; 2], [f32; 2])> + '_ {
        self.points.windows(2).map(|pair| (pair[0], pair[1]))
    }

    /// Length in metres.
    pub fn length(&self) -> f32 {
        self.segments().map(|(a, b)| distance(a, b)).sum()
    }

    /// Time to walk the route.
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    /// The offset reached after `elapsed`; the last point after [`Route::duration`].
    pub fn offset_at(&self, elapsed: Duration) -> [f32; 2] {
        let mut left = elapsed.as_secs_f32() * self.speed_mps;
        for (a, b) in self.segments() {
            let length = distance(a, b);
            if left <= length && length > 0.0 {
                let share = left / length;
                return [a[0] + (b[0] - a[0]) * share, a[1] + (b[1] - a[1]) * share];
            }
            left -= length;
        }
        self.end()
    }

    /// The last point.
    pub fn end(&self) -> [f32; 2] {
        // `new` keeps at least two points.
        self.points.last().copied().unwrap_or_default()
    }

    /// Distance from `offset` to the nearest point of the route, in metres.
    pub fn distance_to(&self, offset: [f32; 2]) -> f32 {
        self.segments()
            .map(|(a, b)| distance_to_segment(offset, a, b))
            .fold(f32::INFINITY, f32::min)
    }
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn distance_to_segment(point: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let along = [b[0] - a[0], b[1] - a[1]];
    let length_squared = along[0] * along[0] + along[1] * along[1];
    if length_squared == 0.0 {
        return distance(point, a);
    }
    let share = (((point[0] - a[0]) * along[0] + (point[1] - a[1]) * along[1]) / length_squared)
        .clamp(0.0, 1.0);
    distance(point, [a[0] + along[0] * share, a[1] + along[1] * share])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEED: f32 = 2.0;
    const SIDE: f32 = 4.0;
    const CLOSE: f32 = 1e-4;

    fn square() -> Route {
        Route::new(
            vec![[0.0, 0.0], [SIDE, 0.0], [SIDE, SIDE], [0.0, SIDE]],
            SPEED,
        )
        .expect("route")
    }

    #[test]
    fn the_walk_moves_along_the_points_at_the_speed() {
        let route = square();
        assert!((route.length() - 3.0 * SIDE).abs() < CLOSE);
        assert_eq!(route.duration(), Duration::from_secs(6));
        let [x, y] = route.offset_at(Duration::from_secs(1));
        assert!((x - SPEED).abs() < CLOSE && y.abs() < CLOSE);
        let [x, y] = route.offset_at(Duration::from_secs(3));
        assert!((x - SIDE).abs() < CLOSE && (y - SPEED).abs() < CLOSE);
        assert_eq!(route.offset_at(Duration::from_secs(60)), route.end());
    }

    #[test]
    fn the_distance_is_to_the_nearest_segment() {
        let route = square();
        assert!(route.distance_to([SIDE / 2.0, 0.0]) < CLOSE);
        assert!((route.distance_to([SIDE / 2.0, -1.0]) - 1.0).abs() < CLOSE);
        assert!((route.distance_to([SIDE + 3.0, SIDE + 4.0]) - 5.0).abs() < CLOSE);
    }

    #[test]
    fn a_route_needs_two_points_and_a_usable_speed() {
        assert_eq!(
            Route::new(vec![[0.0, 0.0]], SPEED),
            Err(RouteError::TooShort)
        );
        assert_eq!(
            Route::new(vec![[0.0, 0.0], [1.0, f32::NAN]], SPEED),
            Err(RouteError::BadNumber)
        );
        assert_eq!(
            Route::new(vec![[0.0, 0.0], [1.0, 0.0]], 0.0),
            Err(RouteError::BadNumber)
        );
        assert_eq!(
            Route::new(vec![[0.0, 0.0], [f32::MAX, 0.0]], f32::MIN_POSITIVE),
            Err(RouteError::TooLong)
        );
    }
}
