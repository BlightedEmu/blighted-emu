// SPDX-License-Identifier: AGPL-3.0-or-later
//! The motion of a mob that a behavior tree moves (`NavigationAction`, `StrafeAction`;
//! behavior-tree study, section 3.2): the numbers come from the slice's navigation config or the
//! node (speed, minimum speed, acceleration, deceleration, turn rate, arrival threshold).
//!
//! The server has no navigation mesh (the level `.bai` files are not read: their format is
//! Unknown), so a mob moves on the straight line to its goal (Inferred: the trees' moves are
//! short, a few to tens of metres) and the caller checks the ground on that line. Facing: a
//! moving mob turns toward where it moves at the turn rate (`FacingMode` 0, `Movement`); a
//! strafing mob faces its target.

use std::f32::consts::{PI, TAU};

use crate::movement::Position;

/// A smaller distance is no move (our engineering limit).
const MOVE_EPSILON_M: f32 = 1e-4;
/// Twice: `v^2 = 2 a d`.
const TWO: f32 = 2.0;

/// The yaw that faces from `from` to `to` (0 faces north, +y; a positive yaw turns west).
pub fn yaw_toward(from: Position, to: Position) -> f32 {
    (-(to.x - from.x)).atan2(to.y - from.y)
}

/// The signed angle from yaw `from` to yaw `to`, in (-π, π].
pub fn yaw_gap(from: f32, to: f32) -> f32 {
    let gap = (to - from + PI).rem_euclid(TAU) - PI;
    if gap <= -PI { gap + TAU } else { gap }
}

/// Yaw `from` turned toward `to` by at most `max_rad`.
pub fn turn_toward(from: f32, to: f32, max_rad: f32) -> f32 {
    from + yaw_gap(from, to).clamp(-max_rad, max_rad)
}

/// How a mob moves (a navigation config, or a node's own fields).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Steering {
    /// Its top speed.
    pub max_speed_mps: f32,
    /// Its least speed while it moves.
    pub min_speed_mps: f32,
    /// How fast it speeds up.
    pub acceleration_mps2: f32,
    /// How fast it slows down.
    pub deceleration_mps2: f32,
    /// How fast it turns, degrees per second.
    pub turn_rate_deg_s: f32,
    /// It has arrived this near its goal.
    pub arrival_m: f32,
}

/// Where a moving mob is, where it faces, and its speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mover {
    /// Where it is.
    pub position: Position,
    /// Where it faces (radians).
    pub yaw: f32,
    /// Its speed.
    pub speed_mps: f32,
}

impl Mover {
    /// The speed it can have `left_m` before a stop with `steering`: it slows so that it stops
    /// at its goal, and speeds up or slows down at the config's rates.
    fn next_speed(&self, left_m: f32, steering: &Steering, dt_s: f32) -> f32 {
        let stop_speed = (TWO * steering.deceleration_mps2 * left_m).sqrt();
        let wanted = steering.max_speed_mps.min(stop_speed);
        let speed = if wanted > self.speed_mps {
            (self.speed_mps + steering.acceleration_mps2 * dt_s).min(wanted)
        } else {
            (self.speed_mps - steering.deceleration_mps2 * dt_s).max(wanted)
        };
        speed.max(steering.min_speed_mps)
    }

    /// Moves `dt_s` toward `goal` with `steering`; whether it arrived (it then stops).
    pub fn steer_to(&mut self, goal: Position, steering: &Steering, dt_s: f32) -> bool {
        let left_m = self.position.horizontal_distance(goal);
        if left_m <= steering.arrival_m {
            self.speed_mps = 0.0;
            return true;
        }
        self.speed_mps = self.next_speed(left_m, steering, dt_s);
        let step_m = (self.speed_mps * dt_s).min(left_m);
        if step_m > MOVE_EPSILON_M {
            let share = step_m / left_m;
            let heading = yaw_toward(self.position, goal);
            self.position = Position {
                x: self.position.x + (goal.x - self.position.x) * share,
                y: self.position.y + (goal.y - self.position.y) * share,
                z: self.position.z,
            };
            self.yaw = turn_toward(
                self.yaw,
                heading,
                steering.turn_rate_deg_s.to_radians() * dt_s,
            );
        }
        let arrived = self.position.horizontal_distance(goal) <= steering.arrival_m;
        if arrived {
            self.speed_mps = 0.0;
        }
        arrived
    }

    /// Strafes `dt_s` around `target` with `steering`: sideways (`left` on its left, else on its
    /// right), and toward or away from it while it is outside the band `ideal_m` (min, max);
    /// it faces the target.
    pub fn strafe_around(
        &mut self,
        target: Position,
        ideal_m: (f32, f32),
        left: bool,
        steering: &Steering,
        dt_s: f32,
    ) {
        let distance = self.position.horizontal_distance(target);
        self.speed_mps = self.next_speed(f32::INFINITY, steering, dt_s);
        let step_m = self.speed_mps * dt_s;
        let to_target = yaw_toward(self.position, target);
        let side = if left { PI / TWO } else { -PI / TWO };
        // Toward the target is the -sin, +cos of the yaw (yaw 0 faces +y, a positive yaw west).
        let radial = if distance > ideal_m.1 {
            1.0
        } else if distance < ideal_m.0 {
            -1.0
        } else {
            0.0
        };
        let (lateral_x, lateral_y) = (-(to_target + side).sin(), (to_target + side).cos());
        let (radial_x, radial_y) = (-to_target.sin() * radial, to_target.cos() * radial);
        let (dx, dy) = (lateral_x + radial_x, lateral_y + radial_y);
        let length = dx.hypot(dy);
        if length > MOVE_EPSILON_M && step_m > MOVE_EPSILON_M {
            self.position.x += dx / length * step_m;
            self.position.y += dy / length * step_m;
        }
        let facing = yaw_toward(self.position, target);
        self.yaw = turn_toward(
            self.yaw,
            facing,
            steering.turn_rate_deg_s.to_radians() * dt_s,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: Position = Position {
        x: 100.0,
        y: 100.0,
        z: 5.0,
    };
    const DT_S: f32 = 1.0 / 30.0;
    const CLOSE: f32 = 1e-3;
    /// The beach boar's `RunToTarget` config (slice `boar`: 2-7 m/s, 30 m/s², 120 °/s, arrival
    /// 0.5 m).
    const RUN: Steering = Steering {
        max_speed_mps: 7.0,
        min_speed_mps: 2.0,
        acceleration_mps2: 30.0,
        deceleration_mps2: 30.0,
        turn_rate_deg_s: 120.0,
        arrival_m: 0.5,
    };

    fn north(metres: f32) -> Position {
        Position {
            y: START.y + metres,
            ..START
        }
    }

    #[test]
    fn yaws_face_north_at_zero_and_turn_west_positive() {
        assert!(yaw_toward(START, north(1.0)).abs() < CLOSE);
        let west = Position {
            x: START.x - 1.0,
            ..START
        };
        assert!((yaw_toward(START, west) - PI / 2.0).abs() < CLOSE);
        assert!(
            (yaw_gap(3.0, -3.0) - (TAU - 6.0)).abs() < CLOSE,
            "the short way round"
        );
        assert!((turn_toward(0.0, 1.0, 0.25) - 0.25).abs() < CLOSE);
    }

    #[test]
    fn a_run_speeds_up_at_its_rate_tops_out_and_stops_at_the_goal() {
        let mut mover = Mover {
            position: START,
            yaw: 0.0,
            speed_mps: 0.0,
        };
        assert!(!mover.steer_to(north(20.0), &RUN, DT_S));
        assert!(
            (mover.speed_mps - RUN.min_speed_mps).abs() < CLOSE,
            "it moves at its least speed at once"
        );
        mover.steer_to(north(20.0), &RUN, DT_S);
        assert!((mover.speed_mps - RUN.min_speed_mps - RUN.acceleration_mps2 * DT_S).abs() < CLOSE);
        let mut top: f32 = 0.0;
        let mut ticks = 1;
        while !mover.steer_to(north(20.0), &RUN, DT_S) {
            top = top.max(mover.speed_mps);
            ticks += 1;
            assert!(ticks < 1_000, "it arrives");
        }
        assert!((top - RUN.max_speed_mps).abs() < CLOSE, "{top}");
        assert!(mover.position.horizontal_distance(north(20.0)) <= RUN.arrival_m);
        assert_eq!(mover.speed_mps, 0.0);
        assert_eq!(
            mover.position.z, START.z,
            "the caller puts it on the ground"
        );
    }

    #[test]
    fn a_mover_turns_at_its_turn_rate() {
        let mut mover = Mover {
            position: START,
            yaw: PI / 2.0,
            speed_mps: RUN.max_speed_mps,
        };
        mover.steer_to(north(20.0), &RUN, DT_S);
        let turned = PI / 2.0 - mover.yaw;
        assert!(
            (turned - RUN.turn_rate_deg_s.to_radians() * DT_S).abs() < CLOSE,
            "{turned}"
        );
    }

    #[test]
    fn a_strafe_circles_the_target_inside_its_band_and_faces_it() {
        const SIDE_TICKS: usize = 10;
        const AROUND_TICKS: usize = 60;
        let target = north(4.0);
        let mut mover = Mover {
            position: START,
            yaw: 0.0,
            speed_mps: 0.0,
        };
        for _ in 0..SIDE_TICKS {
            mover.strafe_around(target, (3.0, 5.0), true, &RUN, DT_S);
        }
        assert!(mover.position.x < START.x, "on its left: west of the start");
        for _ in SIDE_TICKS..AROUND_TICKS {
            mover.strafe_around(target, (3.0, 5.0), true, &RUN, DT_S);
        }
        let distance = mover.position.horizontal_distance(target);
        assert!(
            (3.0..=5.0 + RUN.max_speed_mps * DT_S).contains(&distance),
            "{distance}"
        );
        let facing = yaw_toward(mover.position, target);
        assert!(yaw_gap(mover.yaw, facing).abs() < 0.3, "faces the target");
    }
}
