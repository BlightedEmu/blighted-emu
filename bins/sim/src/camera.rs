// SPDX-License-Identifier: AGPL-3.0-or-later
//! The player's body from the client's camera (ship-mobs-lockon-ai session note, section 1).
//!
//! The position of a client input record (bit 7) is the third-person camera, not the body. With
//! the camera yaw `a` of the same record (bit 6, first value), the retail server's own position
//! of the player (own ALC) was, in every window of the tutorial capture, the camera moved forward
//! along `(-sin a, cos a)` by the boom length and 0.50 m to the left, 2.1 to 3.2 m lower. The boom
//! is 4.75 m while the player stands and shorter while it moves (Observed medians by speed). A
//! server that took the camera as the body made mobs chase a point behind the player and refused
//! most of its swings as out of reach (live run 2026-09-14: 141 of 218).

use nw_sim_core::Position;

/// The camera boom by the player's speed (m/s): (speed from, boom m), retail medians.
const BOOM_BY_SPEED: [(f32, f32); 4] = [(0.0, 4.75), (0.3, 4.44), (2.0, 3.23), (4.0, 2.88)];
/// How far the camera sits to the right of the body (retail: -0.42 to -0.54 m, median 0.50).
const SHOULDER_RIGHT_M: f32 = 0.50;
/// How far the camera sits above the feet (our choice: the middle of the retail 2.1 to 3.2 m; the
/// tutorial level has no ground heights to put the body on).
const CAMERA_HEIGHT_M: f32 = 2.65;

/// Below this speed (m/s) the player stands still and the body estimate is the retail stationary
/// fit; above it the boom shortens with the camera's lag and the estimate errs by about 1 m.
pub const STILL_SPEED_MPS: f32 = BOOM_BY_SPEED[1].0;

/// The boom length at `speed_mps`: the retail medians, joined by straight lines (Inferred: the
/// camera's lag grows with the speed; steps between the medians made the body jump 0.3 to 0.4 m
/// whenever the measured speed wobbled across one, live 2026-09-18).
fn boom_m(speed_mps: f32) -> f32 {
    let [(first_speed, first_boom), ..] = BOOM_BY_SPEED;
    if speed_mps <= first_speed {
        return first_boom;
    }
    BOOM_BY_SPEED
        .windows(2)
        .find(|pair| speed_mps < pair[1].0)
        .map_or(BOOM_BY_SPEED[BOOM_BY_SPEED.len() - 1].1, |pair| {
            let [(low_speed, low_boom), (high_speed, high_boom)] = [pair[0], pair[1]];
            let share = (speed_mps - low_speed) / (high_speed - low_speed);
            low_boom + (high_boom - low_boom) * share
        })
}

/// The body under the camera at `camera`, looking along yaw `yaw` (radians), moving at
/// `speed_mps`.
pub fn body_of(camera: Position, yaw: f32, speed_mps: f32) -> Position {
    let (sin, cos) = yaw.sin_cos();
    let boom = boom_m(speed_mps);
    Position {
        x: camera.x - boom * sin - SHOULDER_RIGHT_M * cos,
        y: camera.y + boom * cos - SHOULDER_RIGHT_M * sin,
        z: camera.z - CAMERA_HEIGHT_M,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOSE_M: f32 = 0.05;

    #[test]
    fn a_standing_player_is_where_retail_put_it() {
        // Retail 104513: input (478.10, 852.74), camera yaw 2.6197; own ALC (476.18, 848.37).
        let camera = Position {
            x: 478.10,
            y: 852.74,
            z: 92.67,
        };
        let body = body_of(camera, 2.6197, 0.0);
        assert!((body.x - 476.18).abs() < CLOSE_M, "{body:?}");
        assert!((body.y - 848.37).abs() < CLOSE_M, "{body:?}");
        assert!((body.z - (92.67 - CAMERA_HEIGHT_M)).abs() < f32::EPSILON);
    }

    #[test]
    fn the_boom_is_shorter_at_speed_and_has_no_steps() {
        for (speed, boom) in BOOM_BY_SPEED {
            assert!(
                (boom_m(speed) - boom).abs() < 1e-5,
                "the retail median at {speed}"
            );
        }
        assert!(
            (boom_m(6.0) - 2.88).abs() < f32::EPSILON,
            "the shortest above the last"
        );
        // Halfway between two medians, halfway between their booms.
        assert!((boom_m(3.0) - (3.23 + 2.88) / 2.0).abs() < 1e-5);
        // A small wobble of the speed moves the boom a little, not by a step.
        assert!((boom_m(4.01) - boom_m(3.99)).abs() < 0.01);
    }
}
