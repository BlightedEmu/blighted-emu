// SPDX-License-Identifier: AGPL-3.0-or-later
//! The reach of swings.
//!
//! A player's melee attack (combat audit, 2026-09-18): retail runs the attack's `CAGE-Damage`
//! shape cast on the server's own copy of the character's animation and checks the targets up to
//! 0.2 s back (`javelin.server-melee-hit-confirmation-max-lookback-time-sec`). The client names no
//! target of a swing (Observed: around the 84 melee hits of the 2026-09-18 retail capture the
//! client sent only its inputs and time syncs; the input's row 3 names the camera-lock target, on
//! 49 of them). Each tick the server casts the shape from its pose of the tick before to its pose
//! of this tick (`FUN_144837220` builds both transforms; `FUN_144860cd0` casts): here the convex
//! hull of the shape at the two poses ([`SweptShape`]; the server's sweep between two turned
//! poses is Unknown, the hull holds both and the straight path between them), against every
//! target's body, so one swing hits every target it passes (retail: 4 of the 84 hits were second
//! targets of one swing). The `Homing` clip turns the attacker to the best target in its reach
//! ([`homing_score`]); the client moves the body.
//!
//! A mob's swing is a wedge in front of its body ([`Wedge`]): mob-side values that its behavior
//! tree will own.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use nw_content::player_attacks::{Homing, ShapeKind};
use nw_formats::rigid::{
    Rigid, Vec3, ZERO, add, cross, dot, quat_about_z, quat_conjugate, rotate, scale, sub,
};
use nw_sim_core::Position;

use super::mob_timeline::yaw_toward;
use crate::mobs::Capsule;

/// The player's body (collision note: the player slice's capsule).
pub(super) const PLAYER_CAPSULE: Capsule = Capsule {
    radius_m: 0.4,
    height_m: 2.0,
};

/// How far apart in height two bodies may stand and still touch in a mob's swing (Inferred): the
/// server's height of a body on a ledge, a pool rim or a slope the height map does not hold can
/// be off by a metre.
const STEP_SLACK_M: f32 = 1.0;

/// Half the arc of a mob swing in front of its body. Best guess (run-15 combat study): the hit
/// test of a creature is the collider of its weapon in its attack clip, which no data the
/// emulator reads holds (the creature animation table has names only; Server.pak's trees test
/// only the angle to start an attack); retail mob hits found the player within 18 degrees of the
/// mob's facing on 16 of 17 hits and further off on one, so the arc keeps a margin over both.
pub(super) const MOB_SWING_HALF_ARC_RAD: f32 = FRAC_PI_4;

/// Degrees in half a turn.
const HALF_TURN_DEG: f32 = 180.0;

/// The turn from yaw `facing` to yaw `toward`, -π to π.
pub(super) fn turn(facing: f32, toward: f32) -> f32 {
    (toward - facing + PI).rem_euclid(TAU) - PI
}

/// The score of a target of body radius `radius_m` at `at` for the homing `homing` of an attacker
/// at `from`, lower is better: `RadiusWeight` x its share of `Radius` plus `AngleWeight` x its
/// share of `MaxAngle`. `None` when it is out of the homing's reach: farther than `Radius` (to its
/// body's edge for an AI target, `UseRadiusforAITarget`), further off `facing` than `MaxAngle`, or
/// more than `Height` above or below. `facing` `None`: a locked target, which the lock turns the
/// attacker to (`ApplyRotationDuringCameraLock`), so its angle counts nothing.
pub(super) fn homing_score(
    homing: &Homing,
    from: Position,
    facing: Option<f32>,
    at: Position,
    radius_m: f32,
) -> Option<f32> {
    let edge_m = if homing.use_radius_for_ai_target {
        radius_m
    } else {
        0.0
    };
    let distance = (at.horizontal_distance(from) - edge_m).max(0.0);
    let max_angle = homing.max_angle_deg * PI / HALF_TURN_DEG;
    let off = facing.map_or(0.0, |facing| turn(facing, yaw_toward(from, at)).abs());
    let in_reach =
        distance <= homing.radius_m && off <= max_angle && (at.z - from.z).abs() <= homing.height_m;
    in_reach.then(|| {
        homing.radius_weight * distance / homing.radius_m + homing.angle_weight * off / max_angle
    })
}

/// The world pose of a body at `at` facing yaw `facing` (0 north, positive to the west): the
/// frame of a pose in the body's root space.
pub(super) fn body_frame(at: Position, facing: f32) -> Rigid {
    Rigid {
        rotation: quat_about_z(facing),
        translation: [at.x, at.y, at.z],
    }
}

/// A melee shape at two poses (world space) and the space between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SweptShape {
    pub(super) shape: ShapeKind,
    pub(super) from: Rigid,
    pub(super) to: Rigid,
}

/// The farthest point along `direction` of the core of `shape` (the shape without its round
/// radius) at `pose`.
fn core_support(shape: ShapeKind, pose: &Rigid, direction: Vec3) -> Vec3 {
    let local = rotate(quat_conjugate(pose.rotation), direction);
    let signed = |half: f32, along: f32| if along < 0.0 { -half } else { half };
    let point = match shape {
        ShapeKind::Sphere { .. } => ZERO,
        ShapeKind::Capsule { half_height_m, .. } => [0.0, 0.0, signed(half_height_m, local[2])],
        ShapeKind::Box { half_extents_m } => {
            std::array::from_fn(|axis| signed(half_extents_m[axis], local[axis]))
        }
        ShapeKind::Cylinder {
            radius_m,
            half_height_m,
        } => {
            let across = (local[0] * local[0] + local[1] * local[1]).sqrt();
            let (x, y) = if across > 0.0 {
                (local[0] / across * radius_m, local[1] / across * radius_m)
            } else {
                (0.0, 0.0)
            };
            [x, y, signed(half_height_m, local[2])]
        }
    };
    pose.apply(point)
}

/// The round radius of `shape` around its core.
const fn round_radius(shape: ShapeKind) -> f32 {
    match shape {
        ShapeKind::Sphere { radius_m } | ShapeKind::Capsule { radius_m, .. } => radius_m,
        ShapeKind::Box { .. } | ShapeKind::Cylinder { .. } => 0.0,
    }
}

impl SweptShape {
    fn support(&self, direction: Vec3) -> Vec3 {
        let from = core_support(self.shape, &self.from, direction);
        let to = core_support(self.shape, &self.to, direction);
        if dot(from, direction) >= dot(to, direction) {
            from
        } else {
            to
        }
    }

    /// Whether a body of capsule `body` standing at `at` touches the shape anywhere between its
    /// two poses.
    pub(super) fn touches(&self, at: Position, body: Capsule) -> bool {
        let low = [at.x, at.y, at.z + body.radius_m];
        let high = [
            at.x,
            at.y,
            at.z + (body.height_m - body.radius_m).max(body.radius_m),
        ];
        let segment = |direction: Vec3| {
            if dot(sub(high, low), direction) > 0.0 {
                high
            } else {
                low
            }
        };
        let distance =
            gjk_distance(|direction| sub(self.support(direction), segment(scale(direction, -1.0))));
        distance <= round_radius(self.shape) + body.radius_m
    }
}

/// Iterations of the distance search: far more than the few a hull of a few points needs.
const GJK_ITERATIONS: usize = 64;
/// The relative progress below which the distance search stops (f32 sums of points metres from
/// the origin round at about 1e-7 of their size).
const GJK_TOLERANCE: f32 = 1e-5;
/// Six times the volume below which a tetrahedron of the search is flat, m³.
const GJK_FLAT_VOLUME: f32 = 1e-9;
/// A support point this near a point of the simplex brings no progress, m.
const GJK_SAME_POINT_M: f32 = 1e-6;

/// The point of segment `a`-`b` nearest the origin, and the part of the segment it lies on.
fn nearest_on_segment(a: Vec3, b: Vec3) -> (Vec3, Vec<Vec3>) {
    let ab = sub(b, a);
    let length = dot(ab, ab);
    if length <= 0.0 {
        return (a, vec![a]);
    }
    let share = -dot(a, ab) / length;
    if share <= 0.0 {
        (a, vec![a])
    } else if share >= 1.0 {
        (b, vec![b])
    } else {
        (add(a, scale(ab, share)), vec![a, b])
    }
}

/// The point of triangle `a`, `b`, `c` nearest the origin and the corners of the part it lies
/// on (Ericson, Real-Time Collision Detection, 5.1.5).
fn nearest_on_triangle(a: Vec3, b: Vec3, c: Vec3) -> (Vec3, Vec<Vec3>) {
    let p = ZERO;
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, vec![a]);
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, vec![b]);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let share = d1 / (d1 - d3);
        return (add(a, scale(ab, share)), vec![a, b]);
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, vec![c]);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let share = d2 / (d2 - d6);
        return (add(a, scale(ac, share)), vec![a, c]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let share = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (add(b, scale(sub(c, b), share)), vec![b, c]);
    }
    let sum = va + vb + vc;
    let (v, w) = (vb / sum, vc / sum);
    (add(a, add(scale(ab, v), scale(ac, w))), vec![a, b, c])
}

/// Whether the origin lies on the far side of face `a`, `b`, `c` from `other`.
fn outside_face(a: Vec3, b: Vec3, c: Vec3, other: Vec3) -> bool {
    let normal = cross(sub(b, a), sub(c, a));
    let origin_side = dot(normal, scale(a, -1.0));
    let other_side = dot(normal, sub(other, a));
    origin_side * other_side < 0.0
}

/// The point of the simplex `points` (1 to 4 points) nearest the origin and the smallest part of
/// it that holds that point; `None` when the origin lies inside a tetrahedron.
fn nearest_on_simplex(points: &[Vec3]) -> Option<(Vec3, Vec<Vec3>)> {
    match *points {
        [a] => Some((a, vec![a])),
        [a, b] => Some(nearest_on_segment(a, b)),
        [a, b, c] => Some(nearest_on_triangle(a, b, c)),
        [a, b, c, d] => {
            let faces = [(a, b, c, d), (a, c, d, b), (a, d, b, c), (b, d, c, a)];
            // A flat tetrahedron holds nothing inside: every face counts.
            let flat = dot(cross(sub(b, a), sub(c, a)), sub(d, a)).abs() <= GJK_FLAT_VOLUME;
            let mut best: Option<(Vec3, Vec<Vec3>)> = None;
            for (p, q, r, other) in faces {
                if flat || outside_face(p, q, r, other) {
                    let found = nearest_on_triangle(p, q, r);
                    if best
                        .as_ref()
                        .is_none_or(|(point, _)| dot(found.0, found.0) < dot(*point, *point))
                    {
                        best = Some(found);
                    }
                }
            }
            best
        }
        _ => None,
    }
}

/// The distance from the origin to the convex set of support function `support` (Gilbert,
/// Johnson and Keerthi): 0 when the origin is inside.
fn gjk_distance(support: impl Fn(Vec3) -> Vec3) -> f32 {
    let mut nearest = support([1.0, 0.0, 0.0]);
    let mut simplex = vec![nearest];
    for _ in 0..GJK_ITERATIONS {
        let length = dot(nearest, nearest);
        if length <= 0.0 {
            return 0.0;
        }
        let far = support(scale(nearest, -1.0));
        let repeated = simplex.iter().any(|point| {
            dot(sub(*point, far), sub(*point, far)) <= GJK_SAME_POINT_M * GJK_SAME_POINT_M
        });
        if repeated || length - dot(nearest, far) <= GJK_TOLERANCE * length {
            break;
        }
        simplex.push(far);
        let Some((point, part)) = nearest_on_simplex(&simplex) else {
            return 0.0;
        };
        nearest = point;
        simplex = part;
    }
    dot(nearest, nearest).sqrt()
}

/// A wedge in front of a mob's body at `from` (of capsule `own`), facing yaw `facing`, `reach_m`
/// deep in front of the body and `half_arc_rad` to each side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Wedge {
    pub(super) from: Position,
    pub(super) own: Capsule,
    pub(super) facing: f32,
    pub(super) reach_m: f32,
    pub(super) half_arc_rad: f32,
}

impl Wedge {
    /// Whether a body of capsule `body` at `at` reaches into it: the body's edge is within the
    /// reach, the arc widens by the angle the body's radius takes at its distance, and the two
    /// capsules overlap in height.
    pub(super) fn touches(&self, at: Position, body: Capsule) -> bool {
        let distance = at.horizontal_distance(self.from);
        if distance > self.reach_m + body.radius_m {
            return false;
        }
        let overlaps = at.z < self.from.z + self.own.height_m + STEP_SLACK_M
            && self.from.z < at.z + body.height_m + STEP_SLACK_M;
        if !overlaps {
            return false;
        }
        // Bodies that overlap touch whatever the facing: so near, the yaw toward the other body
        // says nothing.
        if distance <= self.own.radius_m + body.radius_m {
            return true;
        }
        let widen = if distance > 0.0 {
            (body.radius_m / distance).min(1.0).asin()
        } else {
            FRAC_PI_2
        };
        turn(self.facing, yaw_toward(self.from, at)).abs() <= self.half_arc_rad + widen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: Position = Position {
        x: 100.0,
        y: 100.0,
        z: 20.0,
    };
    const BODY: Capsule = Capsule {
        radius_m: 0.5,
        height_m: 1.8,
    };
    /// Yaw 0 faces north (+y).
    const NORTH: f32 = 0.0;
    /// The greatsword light attack's homing (`player_greatsword.adb`, build 25810808).
    const GREATSWORD: Homing = Homing {
        radius_m: 4.5,
        max_angle_deg: 60.0,
        height_m: 5.0,
        radius_weight: 1.0,
        angle_weight: 1.0,
        use_radius_for_ai_target: true,
    };

    fn at(dx: f32, dy: f32) -> Position {
        Position {
            x: ORIGIN.x + dx,
            y: ORIGIN.y + dy,
            ..ORIGIN
        }
    }

    fn score(facing: Option<f32>, target: Position) -> Option<f32> {
        homing_score(&GREATSWORD, ORIGIN, facing, target, BODY.radius_m)
    }

    #[test]
    fn a_target_in_the_homing_radius_angle_and_height_is_in_reach() {
        assert_eq!(score(Some(NORTH), at(0.0, 0.0)), Some(0.0));
        // 4.9 m to the middle, 4.4 m to the edge of a 0.5 m body.
        assert!(score(Some(NORTH), at(0.0, 4.9)).is_some());
        assert_eq!(score(Some(NORTH), at(0.0, 5.1)), None, "beyond the radius");
        let without_edge = Homing {
            use_radius_for_ai_target: false,
            ..GREATSWORD
        };
        assert_eq!(
            homing_score(&without_edge, ORIGIN, Some(NORTH), at(0.0, 4.9), 0.5),
            None,
            "the middle counts without UseRadiusforAITarget"
        );
        let high = Position {
            z: ORIGIN.z + GREATSWORD.height_m + 0.1,
            ..at(0.0, 2.0)
        };
        assert_eq!(score(Some(NORTH), high), None, "above the height");
    }

    #[test]
    fn the_angle_counts_unless_the_target_is_locked() {
        // 61 degrees off the facing at 2 m.
        let off = 61.0_f32.to_radians();
        let aside = at(-2.0 * off.sin(), 2.0 * off.cos());
        assert_eq!(score(Some(NORTH), aside), None);
        assert!(score(None, aside).is_some(), "the lock turns the attacker");
        let behind = at(0.0, -2.0);
        assert_eq!(score(Some(NORTH), behind), None);
    }

    #[test]
    fn the_weights_rank_nearer_and_straighter_targets_first() {
        let ahead_far = score(Some(NORTH), at(0.0, 4.0)).expect("in reach");
        let ahead_near = score(Some(NORTH), at(0.0, 1.5)).expect("in reach");
        assert!(ahead_near < ahead_far);
        let near_aside = score(Some(NORTH), at(1.0, 1.0)).expect("in reach");
        // 1 m to the edge and 45 of 60 degrees against 3.5 m and 0 degrees.
        assert!((near_aside - ((2.0_f32.sqrt() - 0.5) / 4.5 + 0.75)).abs() < 1e-4);
        assert!((ahead_far - 3.5 / 4.5).abs() < 1e-4);
    }

    /// The greatsword's light attack box (`playermaleanims_2h_meleeattach.adb`): 1.2 x 0.5 x 2 m.
    const BLADE: ShapeKind = ShapeKind::Box {
        half_extents_m: [0.6, 0.25, 1.0],
    };

    fn still(shape: ShapeKind, pose: Rigid) -> SweptShape {
        SweptShape {
            shape,
            from: pose,
            to: pose,
        }
    }

    #[test]
    fn the_distance_search_finds_points_segments_and_overlaps() {
        // A unit cube around (3, 0, 0): 2.5 m from the origin.
        let cube = |direction: Vec3| {
            let corner = |axis: usize| if direction[axis] < 0.0 { -0.5 } else { 0.5 };
            [3.0 + corner(0), corner(1), corner(2)]
        };
        assert!((gjk_distance(cube) - 2.5).abs() < 1e-4);
        let around = |direction: Vec3| {
            let corner = |axis: usize| if direction[axis] < 0.0 { -1.0 } else { 1.0 };
            [corner(0), corner(1), corner(2)]
        };
        assert_eq!(gjk_distance(around), 0.0, "the origin inside");
        // A segment from (-1, 2, 0) to (1, 2, 0): 2 m away at its middle.
        let segment = |direction: Vec3| {
            if direction[0] < 0.0 {
                [-1.0, 2.0, 0.0]
            } else {
                [1.0, 2.0, 0.0]
            }
        };
        assert!((gjk_distance(segment) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn a_flat_simplex_away_from_the_origin_has_a_nearest_point() {
        let square = [
            [1.0, 1.0, 2.0],
            [-1.0, 1.0, 2.0],
            [-1.0, -1.0, 2.0],
            [1.0, -1.0, 2.0],
        ];
        let (point, _) = nearest_on_simplex(&square).expect("flat: a face is nearest");
        assert!((point[2] - 2.0).abs() < 1e-6 && point[0].abs() < 1e-6 && point[1].abs() < 1e-6);
        let around = [
            [1.0, 0.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, -1.0],
            [0.0, 0.0, 1.0],
        ];
        assert_eq!(nearest_on_simplex(&around), None, "the origin inside");
    }

    #[test]
    fn a_shape_touches_a_body_it_overlaps_and_not_one_beyond() {
        let blade = Rigid::translation([ORIGIN.x, ORIGIN.y + 1.5, ORIGIN.z + 1.0]);
        let shape = still(BLADE, blade);
        // The box reaches 0.25 m past y + 1.5; a 0.5 m body touches from 2.25 m.
        assert!(shape.touches(at(0.0, 2.2), BODY));
        assert!(!shape.touches(at(0.0, 2.3), BODY));
        assert!(shape.touches(at(1.05, 1.5), BODY), "its side: 0.6 + 0.5");
        assert!(!shape.touches(at(1.15, 1.5), BODY));
        let sphere = still(ShapeKind::Sphere { radius_m: 0.5 }, blade);
        assert!(sphere.touches(at(0.0, 2.45), BODY));
        assert!(!sphere.touches(at(0.0, 2.55), BODY));
        let high = Position {
            z: ORIGIN.z + 2.6,
            ..at(0.0, 1.5)
        };
        assert!(
            !shape.touches(high, BODY),
            "above the box: its top is 2 m up"
        );
    }

    #[test]
    fn a_swept_shape_hits_what_it_passes_between_two_poses() {
        // A capsule along z swinging from the east side to the west side in one tick.
        let capsule = ShapeKind::Capsule {
            radius_m: 0.3,
            half_height_m: 0.6,
        };
        let pose = |dx: f32| Rigid::translation([ORIGIN.x + dx, ORIGIN.y + 2.0, ORIGIN.z + 1.0]);
        let swing = SweptShape {
            shape: capsule,
            from: pose(2.0),
            to: pose(-2.0),
        };
        assert!(
            swing.touches(at(0.0, 2.0), BODY),
            "passed through in the tick"
        );
        assert!(!still(capsule, pose(2.0)).touches(at(0.0, 2.0), BODY));
        assert!(!still(capsule, pose(-2.0)).touches(at(0.0, 2.0), BODY));
        assert!(!swing.touches(at(0.0, 3.0), BODY), "0.2 m beyond its path");
        // A cylinder ends flat: its rim is the nearest point of a body beside and above it.
        let cylinder = still(
            ShapeKind::Cylinder {
                radius_m: 0.5,
                half_height_m: 0.2,
            },
            Rigid::translation([ORIGIN.x, ORIGIN.y, ORIGIN.z + 0.2]),
        );
        assert!(cylinder.touches(at(0.9, 0.0), BODY));
        assert!(!cylinder.touches(at(1.1, 0.0), BODY));
    }

    #[test]
    fn a_body_frame_turns_the_root_space_to_the_facing() {
        // Facing west (a quarter turn): the root's forward (+y) points to -x.
        let frame = body_frame(ORIGIN, std::f32::consts::FRAC_PI_2);
        let ahead = frame.apply([0.0, 1.0, 0.0]);
        assert!((ahead[0] - (ORIGIN.x - 1.0)).abs() < 1e-5 && (ahead[1] - ORIGIN.y).abs() < 1e-5);
    }

    #[test]
    fn a_mob_wedge_reaches_a_body_in_front_and_not_behind() {
        let wedge = Wedge {
            from: ORIGIN,
            own: BODY,
            facing: NORTH,
            reach_m: 1.6,
            half_arc_rad: MOB_SWING_HALF_ARC_RAD,
        };
        assert!(wedge.touches(at(0.0, 2.1), BODY));
        assert!(!wedge.touches(at(0.0, 2.2), BODY), "edge past the reach");
        assert!(!wedge.touches(at(0.0, -1.5), BODY), "behind");
        assert!(wedge.touches(at(0.0, -0.5), BODY), "overlapping bodies");
        let above = Position {
            z: ORIGIN.z + BODY.height_m + STEP_SLACK_M + 0.1,
            ..at(0.0, 1.0)
        };
        assert!(!wedge.touches(above, BODY));
    }
}
