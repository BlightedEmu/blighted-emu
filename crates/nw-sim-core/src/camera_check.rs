// SPDX-License-Identifier: AGPL-3.0-or-later
//! Where the client's camera says the body is (movement study 2026-09-20). The server has no
//! level collision, so the character it moves from the keys ([`crate::character`]) walks through
//! walls the client stops at; the camera of each input record is the client's own view and shows
//! where the client's body really is.
//!
//! Retail's third-person camera sits on a boom behind the body: the body is the camera moved
//! along the view by the boom (horizontally `boom × cos(pitch)`), 0.50 m to the left of the
//! camera, and `1.685 m + boom × sin(pitch)` below it, and while the body moves the camera trails
//! it by 0.06 s of its motion. The boom is the client's zoom (3 to 7 m at rest, 2.75 m for the
//! short boom, shorter where the camera meets a wall) and is never sent, so each camera gives the
//! body only up to the boom. The check keeps the boom:
//!
//! - While the body rests, the camera moves around a still body (a zoom, a turn of the view): the
//!   boom is the distance of the body along the view. When the body comes to rest, a boom within
//!   half a zoom step of a zoom length is that length, and the body is where it puts it (retail's
//!   resting booms are the zoom lengths to 0.01 m unless a wall shortens them).
//! - While the body moves, the boom keeps its length unless a boom key was pressed: the boom is
//!   solved by weighted least squares over the last records' cameras and the server's own motion
//!   between them, with a prior on the boom and a weak one on the server's body (a turning view
//!   shows the boom; a straight view leaves it as it was). The body is where the newest camera
//!   puts it with that boom.
//!
//! A camera that does not fit the solve or puts the boom out of its range (a cutscene camera, a
//! camera the client moved away, a server move not yet followed) gives nothing.

use std::collections::VecDeque;

use crate::character::yaw_axes;
use crate::movement::Position;

/// How far the camera sits to the right of the body, m (Observed: the side offset of the camera
/// from the retail own place, median 0.50 m; 8,029 of 14,584 newchar samples within 0.01 m).
pub const SHOULDER_RIGHT_M: f32 = 0.50;
/// How far the camera sits above the feet besides the boom's rise, m (Observed: retail resting
/// camera height above the own place less `boom × sin(pitch)`, median 1.679 m over 2,492 tutorial
/// and 1.686 m over 5,071 open world samples, quartiles 1.67 to 1.69 m).
pub const CAMERA_RISE_M: f32 = 1.685;
/// How far behind its body's motion the camera trails, s (Observed: a fit of the retail moving
/// places against the camera with the resting boom: 0.060 s in the open world, 0.064 s in the
/// tutorial; the camera body then erred by a median 0.10 m instead of 0.22 m).
pub const CAMERA_LAG_S: f32 = 0.06;
/// The zoom lengths of the boom at rest, m (Observed: the retail resting booms of the newchar
/// capture gather at 3.00, 4.00, 5.00, 6.00 and 7.00 m, the zoom keys step between them, and
/// the short-boom key sets 2.75 m).
pub const ZOOM_BOOMS_M: [f32; 6] = [2.75, 3.0, 4.0, 5.0, 6.0, 7.0];
/// A resting boom this near a zoom length is that length, m: half a zoom step.
const ZOOM_SNAP_M: f32 = 0.5;
/// Input records in the moving solve (replay of the newchar capture with the other values here,
/// open world and tutorial: 5 records erred by a median 0.24 and 0.31 m, 8 by 0.16 and 0.22 m, 12
/// by 0.17 and 0.79 m).
pub const WINDOW_RECORDS: usize = 8;
/// The spread of one camera sample about the boom model, m (Observed: the side offset of the
/// retail camera is within 0.02 m of the model in most samples and trails by up to 0.25 m while
/// the view turns).
const CAMERA_SIGMA_M: f32 = 0.05;
/// The spread of the boom between records while it keeps its length, m: the trail of a turning
/// camera.
const BOOM_HELD_SIGMA_M: f32 = 0.05;
/// The spread of the boom after a boom key, m: a zoom step.
const BOOM_FREE_SIGMA_M: f32 = 1.0;
/// The spread of the server's moving body, m: as large as a wall stop between records.
const BODY_MOVING_SIGMA_M: f32 = 1.0;
/// Records after a boom key while the boom may change (Observed: a retail zoom glided over 17
/// records, 0.57 s, from 4.00 to 4.90 m and settled by 20).
const BOOM_FREE_RECORDS: u8 = 20;
/// The newest camera is within this of the solve, m, or the camera does not follow the body.
const CAMERA_FIT_M: f32 = 1.0;
/// The boom of a camera that follows the body, m (Observed: retail resting booms from 0.1 m,
/// against a wall, to 7.0 m, the longest zoom; moving booms up to 8.4 m with the trail).
const BOOM_RANGE_M: std::ops::Range<f32> = 0.0..9.0;

/// The client's camera of one input record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraView {
    /// The camera position.
    pub camera: Position,
    /// The camera yaw (radians; 0 faces north, positive turns west).
    pub yaw: f32,
    /// The camera pitch (radians; positive looks down).
    pub pitch: f32,
}

impl CameraView {
    /// The ground-plane step of one metre of boom along the view.
    fn ray(&self) -> [f32; 2] {
        let (forward, _) = yaw_axes(self.yaw);
        let level = self.pitch.cos();
        [forward[0] * level, forward[1] * level]
    }

    /// The ground-plane point the boom starts from: the camera, the shoulder and the trail of a
    /// body moving at `velocity`.
    fn base(&self, velocity: [f32; 2]) -> [f32; 2] {
        let (_, right) = yaw_axes(self.yaw);
        [
            self.camera.x - SHOULDER_RIGHT_M * right[0] + CAMERA_LAG_S * velocity[0],
            self.camera.y - SHOULDER_RIGHT_M * right[1] + CAMERA_LAG_S * velocity[1],
        ]
    }

    /// The boom that puts a body moving at `velocity` at `body` (its distance along the view).
    fn boom_to(&self, body: Position, velocity: [f32; 2]) -> f32 {
        let ray = self.ray();
        let base = self.base(velocity);
        let along = [body.x - base[0], body.y - base[1]];
        (along[0] * ray[0] + along[1] * ray[1]) / (ray[0] * ray[0] + ray[1] * ray[1])
    }

    /// The body under this camera with boom `boom`, moving at `velocity`.
    fn body(&self, boom: f32, velocity: [f32; 2]) -> Position {
        let ray = self.ray();
        let base = self.base(velocity);
        Position {
            x: base[0] + boom * ray[0],
            y: base[1] + boom * ray[1],
            z: self.camera.z - CAMERA_RISE_M - boom * self.pitch.sin(),
        }
    }
}

/// One camera of the window: the body was at `base + boom × ray`, and moved since by `moved`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Seen {
    base: [f32; 2],
    ray: [f32; 2],
    /// The server's motion of the body since this record.
    moved: [f32; 2],
}

/// The body and the boom the camera gives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraBody {
    /// The body; its height is the feet under the camera.
    pub at: Position,
    /// The boom, m.
    pub boom_m: f32,
}

/// The camera check of one player.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraCheck {
    seen: VecDeque<Seen>,
    boom_m: Option<f32>,
    free_records: u8,
    /// The body rested in the record before; a body the server just placed rests where it put it.
    was_resting: bool,
}

impl Default for CameraCheck {
    fn default() -> Self {
        Self {
            seen: VecDeque::with_capacity(WINDOW_RECORDS),
            boom_m: None,
            free_records: 0,
            was_resting: true,
        }
    }
}

/// Weighted normal equations of the solve: `matrix × [x, y, boom] = rhs`.
#[derive(Debug, Clone, Copy, Default)]
struct Normal {
    matrix: [[f32; 3]; 3],
    rhs: [f32; 3],
}

impl Normal {
    /// Adds the row `coefficients · unknowns = value` with weight `weight`.
    fn add(&mut self, coefficients: [f32; 3], value: f32, weight: f32) {
        for (row, &left) in coefficients.iter().enumerate() {
            for (column, &right) in coefficients.iter().enumerate() {
                self.matrix[row][column] += weight * left * right;
            }
            self.rhs[row] += weight * left * value;
        }
    }

    /// The unknowns, by Cramer's rule; `None` for a singular system.
    fn solve(&self) -> Option<[f32; 3]> {
        let determinant = |m: &[[f32; 3]; 3]| {
            m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
                - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
        };
        let whole = determinant(&self.matrix);
        if whole.abs() <= f32::EPSILON {
            return None;
        }
        let mut solution = [0.0; 3];
        for (column, value) in solution.iter_mut().enumerate() {
            let mut replaced = self.matrix;
            for (row, line) in replaced.iter_mut().enumerate() {
                line[column] = self.rhs[row];
            }
            *value = determinant(&replaced) / whole;
        }
        Some(solution)
    }
}

/// The weight of a spread.
fn weight(sigma: f32) -> f32 {
    1.0 / (sigma * sigma)
}

/// The zoom length nearest `boom`, when it is within [`ZOOM_SNAP_M`].
fn zoom_length(boom: f32) -> Option<f32> {
    ZOOM_BOOMS_M
        .into_iter()
        .min_by(|a, b| (a - boom).abs().total_cmp(&(b - boom).abs()))
        .filter(|zoom| (zoom - boom).abs() <= ZOOM_SNAP_M)
}

impl CameraCheck {
    /// Forgets the cameras: after a server move the camera jumps with the body.
    pub fn forget(&mut self) {
        *self = Self::default();
    }

    /// The boom the check holds, once a camera fitted.
    pub const fn boom_m(&self) -> Option<f32> {
        self.boom_m
    }

    /// Takes the camera `view` of a record after which the server's body is at `body`, moved by
    /// `moved` in the record and moving at `velocity`; `resting` when it stands still on a
    /// floor, `boom_key` when the record pressed a key that changes the boom. Gives where the
    /// camera puts the body, or `None` when the camera does not follow it.
    pub fn observe(
        &mut self,
        view: CameraView,
        body: Position,
        moved: [f32; 2],
        velocity: [f32; 2],
        resting: bool,
        boom_key: bool,
    ) -> Option<CameraBody> {
        if boom_key {
            self.free_records = BOOM_FREE_RECORDS;
        }
        for seen in &mut self.seen {
            seen.moved = [seen.moved[0] + moved[0], seen.moved[1] + moved[1]];
        }
        if self.seen.len() == WINDOW_RECORDS {
            self.seen.pop_front();
        }
        self.seen.push_back(Seen {
            base: view.base(velocity),
            ray: view.ray(),
            moved: [0.0; 2],
        });
        let came_to_rest = resting && !self.was_resting;
        self.was_resting = resting;
        let found = if resting {
            Self::resting(view, body, came_to_rest)
        } else {
            self.moving(view, body, velocity)
        };
        self.free_records = self.free_records.saturating_sub(1);
        let found = found.filter(|found| BOOM_RANGE_M.contains(&found.boom_m))?;
        self.boom_m = Some(found.boom_m);
        Some(found)
    }

    /// The body and boom of a resting body: the body stays, the boom is its distance along the
    /// view, or the zoom length near it as the body comes to rest.
    fn resting(view: CameraView, body: Position, came_to_rest: bool) -> Option<CameraBody> {
        let boom = view.boom_to(body, [0.0; 2]);
        let zoom = zoom_length(boom).filter(|_| came_to_rest);
        Some(match zoom {
            Some(zoom) => CameraBody {
                at: view.body(zoom, [0.0; 2]),
                boom_m: zoom,
            },
            None => CameraBody {
                at: Position {
                    z: view.body(boom, [0.0; 2]).z,
                    ..body
                },
                boom_m: boom,
            },
        })
    }

    /// The body and boom of a moving body: the boom of the window's solve, the body on the
    /// newest camera.
    fn moving(&self, view: CameraView, body: Position, velocity: [f32; 2]) -> Option<CameraBody> {
        let boom_prior = self.boom_m.unwrap_or_else(|| view.boom_to(body, velocity));
        let mut normal = Normal::default();
        let camera = weight(CAMERA_SIGMA_M);
        for seen in &self.seen {
            let x = seen.base[0] + seen.moved[0];
            let y = seen.base[1] + seen.moved[1];
            normal.add([1.0, 0.0, -seen.ray[0]], x, camera);
            normal.add([0.0, 1.0, -seen.ray[1]], y, camera);
        }
        let boom_sigma = if self.free_records > 0 {
            BOOM_FREE_SIGMA_M
        } else {
            BOOM_HELD_SIGMA_M
        };
        normal.add([0.0, 0.0, 1.0], boom_prior, weight(boom_sigma));
        let body_weight = weight(BODY_MOVING_SIGMA_M);
        normal.add([1.0, 0.0, 0.0], body.x, body_weight);
        normal.add([0.0, 1.0, 0.0], body.y, body_weight);
        let [x, y, boom] = normal.solve()?;
        let at = view.body(boom, velocity);
        ((x - at.x).hypot(y - at.y) < CAMERA_FIT_M).then_some(CameraBody { at, boom_m: boom })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOSE_M: f32 = 0.01;
    const BOOM: f32 = 4.0;
    const PITCH: f32 = 0.2;

    /// The camera of a body at `body` with boom `boom`, yaw `yaw`, trailing `lag`.
    fn camera_of(body: Position, yaw: f32, boom: f32, lag: [f32; 2]) -> CameraView {
        let mut view = CameraView {
            camera: body,
            yaw,
            pitch: PITCH,
        };
        let ray = view.ray();
        let (_, right) = yaw_axes(yaw);
        view.camera = Position {
            x: body.x - boom * ray[0] + SHOULDER_RIGHT_M * right[0] - lag[0],
            y: body.y - boom * ray[1] + SHOULDER_RIGHT_M * right[1] - lag[1],
            z: body.z + CAMERA_RISE_M + boom * PITCH.sin(),
        };
        view
    }

    fn at(x: f32, y: f32) -> Position {
        Position { x, y, z: 10.0 }
    }

    #[test]
    fn a_retail_camera_turning_around_a_resting_body_gives_its_place_and_boom() {
        // Retail newchar open world, records 22897 to 22899: the camera (x, y, z, yaw, pitch)
        // circles the resting own place (9864.567, 2289.073, 48.224) on the 4 m zoom.
        const CAMERAS: [[f32; 5]; 3] = [
            [9866.42, 2286.74, 52.625, 0.506_11, 0.744_27],
            [9866.167, 2286.506, 52.575, 0.394_98, 0.727_43],
            [9865.425, 2286.079, 52.474, 0.121_67, 0.693_77],
        ];
        const RETAIL: Position = Position {
            x: 9864.567,
            y: 2289.073,
            z: 48.224,
        };
        const RETAIL_BOOM_M: f32 = 4.0;
        const WITHIN_M: f32 = 0.02;
        let mut check = CameraCheck::default();
        for [x, y, z, yaw, pitch] in CAMERAS {
            let view = CameraView {
                camera: Position { x, y, z },
                yaw,
                pitch,
            };
            let seen = check
                .observe(view, RETAIL, [0.0; 2], [0.0; 2], true, false)
                .expect("fits");
            assert!(seen.at.horizontal_distance(RETAIL) < WITHIN_M, "{seen:?}");
            assert!((seen.boom_m - RETAIL_BOOM_M).abs() < WITHIN_M, "{seen:?}");
            assert!((seen.at.z - RETAIL.z).abs() < WITHIN_M, "{seen:?}");
        }
    }

    #[test]
    fn a_wall_stop_shows_as_a_camera_that_stays() {
        // The server's body runs on 2 m into a wall while the client's body and camera stop.
        let mut check = CameraCheck::default();
        let wall = at(0.0, 0.0);
        let view = camera_of(wall, 0.0, BOOM, [0.0; 2]);
        check.observe(view, wall, [0.0; 2], [0.0; 2], true, false);
        let step = 5.5 / 30.0;
        let mut server = wall;
        let mut last = None;
        for _ in 0..11 {
            server.y += step;
            last = check.observe(view, server, [0.0, step], [0.0, 5.5], false, false);
        }
        let seen = last.expect("fits");
        assert!(
            seen.at.horizontal_distance(wall) < 0.4,
            "{seen:?} vs 2 m ahead"
        );
    }

    #[test]
    fn a_zoom_at_rest_moves_the_boom_not_the_body() {
        let mut check = CameraCheck::default();
        let body = at(5.0, 5.0);
        check.observe(
            camera_of(body, 1.0, BOOM, [0.0; 2]),
            body,
            [0.0; 2],
            [0.0; 2],
            true,
            false,
        );
        let mut last = None;
        for record in 1..=20_u8 {
            let boom = BOOM + f32::from(record) / 20.0;
            let view = camera_of(body, 1.0, boom, [0.0; 2]);
            last = check.observe(view, body, [0.0; 2], [0.0; 2], true, record == 1);
        }
        let seen = last.expect("fits");
        assert!(seen.at.horizontal_distance(body) < CLOSE_M, "{seen:?}");
        assert!((seen.boom_m - (BOOM + 1.0)).abs() < CLOSE_M);
    }

    #[test]
    fn a_turning_camera_mends_a_wrong_boom_while_the_body_moves() {
        // The server's body stands 1 m behind the client's, so the boom it gives is 1 m short
        // along the ground; the camera then circles the body as it runs north.
        let mut check = CameraCheck::default();
        let mut body = at(0.0, 0.0);
        let first = check
            .observe(
                camera_of(body, 0.0, BOOM, [0.0; 2]),
                at(0.0, -1.0),
                [0.0; 2],
                [0.0; 2],
                true,
                false,
            )
            .expect("fits");
        assert!((first.boom_m - (BOOM - 1.0 / PITCH.cos())).abs() < CLOSE_M);
        let mut last = None;
        let velocity = [0.0, 5.5];
        let lag = [CAMERA_LAG_S * velocity[0], CAMERA_LAG_S * velocity[1]];
        for record in 1..=30_u8 {
            body.y += 5.5 / 30.0;
            let yaw = f32::from(record) * 0.05;
            last = check.observe(
                camera_of(body, yaw, BOOM, lag),
                at(body.x, body.y - 1.0),
                [0.0, 5.5 / 30.0],
                velocity,
                false,
                false,
            );
        }
        let seen = last.expect("fits");
        assert!(
            seen.at.horizontal_distance(body) < 0.15,
            "{seen:?} vs {body:?}"
        );
        assert!((seen.boom_m - BOOM).abs() < 0.15, "{seen:?}");
    }

    #[test]
    fn a_camera_far_from_the_body_does_not_fit() {
        let mut check = CameraCheck::default();
        let body = at(0.0, 0.0);
        check.observe(
            camera_of(body, 0.0, BOOM, [0.0; 2]),
            body,
            [0.0; 2],
            [0.0; 2],
            true,
            false,
        );
        // A cutscene camera 40 m away, looking elsewhere.
        let cutscene = camera_of(at(30.0, 30.0), 2.0, BOOM, [0.0; 2]);
        assert_eq!(
            check.observe(cutscene, body, [0.0; 2], [0.0; 2], true, false),
            None
        );
    }

    #[test]
    fn the_feet_are_below_the_camera_by_the_rise_and_the_boom() {
        let body = Position {
            x: 1.0,
            y: 2.0,
            z: 22.5,
        };
        let view = camera_of(body, 0.3, BOOM, [0.0; 2]);
        assert!((view.body(BOOM, [0.0; 2]).z - body.z).abs() < CLOSE_M);
    }
}
