// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `Homing` procedural clips of a mob's attack: the server turns the mob toward its target and
//! moves it in to the clip's distance while the clip plays.
//!
//! The client plays a `Homing` clip only on the player's own character (Observed: retail mobs in
//! the 2026-11-10 live checks struck from where the server put them, and our mobs that stood
//! still swung from 3 to 4.5 m and missed). So the sim moves and turns a mob by the clips of the
//! fragment entry it plays.
//!
//! The parameters are the clip's `<ProceduralParams>` (Observed: all 240 `Homing` clips of the
//! player animation databases of the pinned build carry the same names; the creature databases
//! use the same clip type, `HomingContext`). The rules below (the wedge at the clip start, the
//! exponential turn per 1/30 s, the smoothstep move in `MoveToDuration`, `MaxMoveDistance` 0 is no
//! limit) are Inferred from the parameter names and their values in the data.

use nw_sim_core::{Position, yaw_gap, yaw_toward};

/// The clip type of a homing clip.
pub const HOMING_CLIP: &str = "Homing";
/// The parameters of a homing clip.
pub const RADIUS_PARAM: &str = "Radius";
/// Its wedge's half angle (degrees).
pub const MAX_ANGLE_PARAM: &str = "MaxAngle";
/// Its wedge's height (metres; 0 is no limit).
pub const HEIGHT_PARAM: &str = "Height";
/// Its turn rate: a share of the angle per frame with `UseExponential`, else degrees per second.
pub const TURN_RATE_PARAM: &str = "TurnRate";
/// The turn is a share of the angle that is left.
pub const EXPONENTIAL_PARAM: &str = "UseExponential";
/// It keeps the target's place of the clip start.
pub const LOCK_TARGET_PARAM: &str = "LockInitialTargetPos";
/// It keeps turning after it faces the target.
pub const TURN_AFTER_PARAM: &str = "AllowRotationAfterReachingTargetAngle";
/// It moves toward the target.
pub const MOVE_TO_TARGET_PARAM: &str = "MoveToTarget";
/// It stops this far from the target.
pub const MOVE_TO_DISTANCE_PARAM: &str = "MoveToDistance";
/// The time of the move (seconds).
pub const MOVE_TO_DURATION_PARAM: &str = "MoveToDuration";
/// The speed of the move (metres per second; 0: the move follows `MoveToDuration`).
pub const MOVE_TO_VELOCITY_PARAM: &str = "MoveToVelocity";
/// The longest move from the clip start (metres; 0 is no limit).
pub const MAX_MOVE_PARAM: &str = "MaxMoveDistance";
/// The value of a true clip parameter.
pub const PARAM_TRUE: &str = "true";

/// Frames per second of an exponential turn rate (Inferred: the client's animation frame).
const TURN_FRAMES_PER_S: f32 = 30.0;
/// A turn this near the target's direction is done (our engineering limit).
const TURN_DONE_RAD: f32 = 0.01;
/// A target this near is in every direction (our engineering limit).
const SAME_PLACE_M: f32 = 0.01;
/// A smaller move is no move (our engineering limit).
const MOVE_EPSILON_M: f32 = 1e-4;
/// Milliseconds per second.
const MS_PER_S: f32 = 1_000.0;
/// The smoothstep `u^2 (3 - 2u)`.
const SMOOTHSTEP_A: f32 = 3.0;
const SMOOTHSTEP_B: f32 = 2.0;

/// One homing clip of a fragment entry.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HomingClip {
    /// Its start after the attack start (seconds).
    pub start_s: f32,
    /// Its end after the attack start; `None`: to the end of the attack.
    pub end_s: Option<f32>,
    /// The wedge the target must stand in.
    pub radius_m: f32,
    /// Half the wedge's angle (radians).
    pub max_angle_rad: f32,
    /// The wedge's height (0: no limit).
    pub height_m: f32,
    /// The turn rate (see [`TURN_RATE_PARAM`]).
    pub turn_rate: f32,
    /// The turn is exponential.
    pub exponential: bool,
    /// It aims at the target's place of the clip start.
    pub lock_target: bool,
    /// It keeps turning after it faces the target.
    pub turn_after_target: bool,
    /// It moves toward the target.
    pub move_to_target: bool,
    /// It stops this far from the target.
    pub move_to_distance_m: f32,
    /// The time of the move.
    pub move_to_duration_s: f32,
    /// The speed of the move (0: by the time).
    pub move_to_velocity_mps: f32,
    /// The longest move from the clip start (0: no limit).
    pub max_move_m: f32,
}

impl HomingClip {
    /// The clip from `start_s` to `end_s` with the parameters `params` (name, value).
    pub fn from_params<'a>(
        start_s: f32,
        end_s: Option<f32>,
        params: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let mut clip = Self {
            start_s,
            end_s,
            ..Self::default()
        };
        for (name, value) in params {
            let value = value.trim();
            let number = value.parse::<f32>().unwrap_or(0.0);
            let flag = value == PARAM_TRUE;
            match name {
                RADIUS_PARAM => clip.radius_m = number,
                MAX_ANGLE_PARAM => clip.max_angle_rad = number.to_radians(),
                HEIGHT_PARAM => clip.height_m = number,
                TURN_RATE_PARAM => clip.turn_rate = number,
                EXPONENTIAL_PARAM => clip.exponential = flag,
                LOCK_TARGET_PARAM => clip.lock_target = flag,
                TURN_AFTER_PARAM => clip.turn_after_target = flag,
                MOVE_TO_TARGET_PARAM => clip.move_to_target = flag,
                MOVE_TO_DISTANCE_PARAM => clip.move_to_distance_m = number,
                MOVE_TO_DURATION_PARAM => clip.move_to_duration_s = number,
                MOVE_TO_VELOCITY_PARAM => clip.move_to_velocity_mps = number,
                MAX_MOVE_PARAM => clip.max_move_m = number,
                _ => {}
            }
        }
        clip
    }

    /// Whether `target` stands in the clip's wedge from `from` facing `yaw`.
    fn holds(&self, from: Position, yaw: f32, target: Position) -> bool {
        let distance = from.horizontal_distance(target);
        if distance >= self.radius_m {
            return false;
        }
        if self.height_m > 0.0 && (target.z - from.z).abs() >= self.height_m {
            return false;
        }
        distance < SAME_PLACE_M
            || yaw_gap(yaw, yaw_toward(from, target)).abs() <= self.max_angle_rad
    }

    /// The turn from `yaw` at `at` toward `aim` over `dt_s`.
    fn turn(&self, yaw: f32, at: Position, aim: Position, dt_s: f32) -> f32 {
        let left = yaw_gap(yaw, yaw_toward(at, aim));
        let turn = if self.exponential {
            let kept = 1.0 - self.turn_rate.clamp(0.0, 1.0);
            left * (1.0 - kept.powf(TURN_FRAMES_PER_S * dt_s))
        } else {
            left.signum() * left.abs().min(self.turn_rate.to_radians() * dt_s)
        };
        if turn.abs() > left.abs() { left } else { turn }
    }
}

/// A body a homing clip moves: where it stands and where it faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    /// Where it stands.
    pub position: Position,
    /// Where it faces (radians).
    pub yaw: f32,
}

/// A clip that started: where the body stood, where it aims, and the place of its move curve.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Started {
    origin: Position,
    scan_yaw: f32,
    aim: Position,
    curve: Position,
    last_ms: u64,
    turn_done: bool,
    move_done: bool,
}

/// A homing clip of the attack a mob plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HomingRun {
    clip: HomingClip,
    starts_ms: u64,
    ends_ms: Option<u64>,
    started: Option<Started>,
}

impl HomingRun {
    /// The run of `clip` of an attack that starts at `attack_ms` and ends at `attack_end_ms`
    /// (`None`: its end is Unknown, the clip ends when its turn and its move are done).
    pub fn new(clip: HomingClip, attack_ms: u64, attack_end_ms: Option<u64>) -> Self {
        let at = |seconds: f32| {
            // Seconds of a fragment.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "seconds of a fragment"
            )]
            let ms = (seconds.max(0.0) * MS_PER_S).round() as u64;
            attack_ms + ms
        };
        Self {
            clip,
            starts_ms: at(clip.start_s),
            ends_ms: clip.end_s.map(at).or(attack_end_ms),
            started: None,
        }
    }

    /// Whether the run is over at `now_ms`.
    fn over(&self, now_ms: u64) -> bool {
        self.ends_ms.is_some_and(|end| now_ms >= end)
            || self
                .started
                .is_some_and(|started| started.turn_done && started.move_done)
    }
}

/// One step at `now_ms` of the homing runs of a mob's attack on `body`, whose target stands at
/// `target`: each clip that starts takes the target when it stands in its wedge (else the clip
/// does nothing); each running clip turns the body and moves it in. A `rooted` body only turns
/// (its move curve goes on without travel). `slide(from, to)` is where the move from `from` to
/// `to` ends on the floors, `None` when a wall or an edge stops it: that ends the clip's move.
/// The runs that are over leave `runs`.
pub fn step(
    runs: &mut Vec<HomingRun>,
    now_ms: u64,
    body: &mut Body,
    target: Option<Position>,
    rooted: bool,
    mut slide: impl FnMut(Position, Position) -> Option<Position>,
) {
    for run in runs.iter_mut() {
        if now_ms < run.starts_ms {
            continue;
        }
        let clip = run.clip;
        if run.started.is_none() {
            let aim = target.filter(|target| clip.holds(body.position, body.yaw, *target));
            run.started = Some(Started {
                origin: body.position,
                scan_yaw: body.yaw,
                aim: aim.unwrap_or(body.position),
                curve: body.position,
                last_ms: run.starts_ms,
                turn_done: aim.is_none(),
                move_done: aim.is_none() || !clip.move_to_target,
            });
        }
        let Some(started) = run.started.as_mut() else {
            continue;
        };
        let until_ms = run.ends_ms.map_or(now_ms, |end| now_ms.min(end));
        // A step and a clip time below a few seconds.
        #[allow(clippy::cast_precision_loss, reason = "a step of a few seconds")]
        let dt_s = until_ms.saturating_sub(started.last_ms) as f32 / MS_PER_S;
        #[allow(clippy::cast_precision_loss, reason = "a clip of a few seconds")]
        let t_s = until_ms.saturating_sub(run.starts_ms) as f32 / MS_PER_S;
        started.last_ms = until_ms;
        if dt_s <= 0.0 || (started.turn_done && started.move_done) {
            continue;
        }
        if !clip.lock_target
            && let Some(target) =
                target.filter(|target| clip.holds(started.origin, started.scan_yaw, *target))
        {
            started.aim = target;
        }
        if !started.turn_done && body.position.horizontal_distance(started.aim) > SAME_PLACE_M {
            let turn = clip.turn(body.yaw, body.position, started.aim, dt_s);
            body.yaw += turn;
            let left = yaw_gap(body.yaw, yaw_toward(body.position, started.aim));
            if left.abs() < TURN_DONE_RAD && !clip.turn_after_target {
                started.turn_done = true;
            }
        }
        if started.move_done {
            continue;
        }
        let Some(step) = move_step(&clip, started, body.position, dt_s, t_s) else {
            continue;
        };
        if rooted {
            continue;
        }
        let goal = Position {
            x: body.position.x + step[0],
            y: body.position.y + step[1],
            z: body.position.z,
        };
        match slide(body.position, goal) {
            Some(end) => body.position = end,
            None => started.move_done = true,
        }
    }
    runs.retain(|run| !run.over(now_ms));
}

/// The ground step of a clip's move over `dt_s` at clip time `t_s` for a body at `at`; `None`
/// when the move is done (the body is within `MoveToDistance` of its aim).
fn move_step(
    clip: &HomingClip,
    started: &mut Started,
    at: Position,
    dt_s: f32,
    t_s: f32,
) -> Option<[f32; 2]> {
    if at.horizontal_distance(started.aim) <= clip.move_to_distance_m {
        started.move_done = true;
        return None;
    }
    let by_speed = clip.move_to_velocity_mps > 0.0;
    let from = if by_speed { at } else { started.curve };
    let line = from.horizontal_distance(started.aim).max(MOVE_EPSILON_M);
    let unit = [
        (started.aim.x - from.x) / line,
        (started.aim.y - from.y) / line,
    ];
    let mut dest = [
        started.aim.x - unit[0] * clip.move_to_distance_m,
        started.aim.y - unit[1] * clip.move_to_distance_m,
    ];
    let reach = (dest[0] - started.origin.x).hypot(dest[1] - started.origin.y);
    if clip.max_move_m > 0.0 && reach > clip.max_move_m {
        let scale = clip.max_move_m / reach;
        dest = [
            started.origin.x + (dest[0] - started.origin.x) * scale,
            started.origin.y + (dest[1] - started.origin.y) * scale,
        ];
    }
    if by_speed {
        let gap = [dest[0] - at.x, dest[1] - at.y];
        let left = gap[0].hypot(gap[1]);
        if left <= MOVE_EPSILON_M {
            started.move_done = true;
            return None;
        }
        let way = left.min(clip.move_to_velocity_mps * dt_s);
        return Some([gap[0] / left * way, gap[1] / left * way]);
    }
    let u = if clip.move_to_duration_s > 0.0 {
        (t_s / clip.move_to_duration_s).min(1.0)
    } else {
        1.0
    };
    let w = u * u * (SMOOTHSTEP_A - SMOOTHSTEP_B * u);
    let curve = [
        started.origin.x + w * (dest[0] - started.origin.x),
        started.origin.y + w * (dest[1] - started.origin.y),
    ];
    let mut step = [curve[0] - started.curve.x, curve[1] - started.curve.y];
    let length = step[0].hypot(step[1]);
    if clip.max_move_m > 0.0 && clip.move_to_duration_s > 0.0 {
        let limit = clip.max_move_m / clip.move_to_duration_s * dt_s;
        if length > limit && length > 0.0 {
            step = [step[0] * limit / length, step[1] * limit / length];
        }
    }
    started.curve.x += step[0];
    started.curve.y += step[1];
    if u >= 1.0 {
        started.move_done = true;
    }
    Some(step)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spear clip of the pinned build's player database (`player_spear.adb`): radius 4.5,
    /// angle 45, height 5, exponential turn 0.4, move to 2 m in 0.3 s.
    fn spear_clip() -> HomingClip {
        HomingClip::from_params(
            0.0,
            Some(0.5),
            [
                (RADIUS_PARAM, "4.5"),
                (MAX_ANGLE_PARAM, "45"),
                (HEIGHT_PARAM, "5"),
                (TURN_RATE_PARAM, "0.40000001"),
                (EXPONENTIAL_PARAM, "true"),
                (MOVE_TO_TARGET_PARAM, "true"),
                (MOVE_TO_DISTANCE_PARAM, "2"),
                (MOVE_TO_DURATION_PARAM, "0.30000001"),
                (MAX_MOVE_PARAM, "0"),
                (LOCK_TARGET_PARAM, "false"),
                (TURN_AFTER_PARAM, "false"),
            ],
        )
    }

    fn at(x: f32, y: f32) -> Position {
        Position { x, y, z: 0.0 }
    }

    /// Facing north (+y) at the origin.
    fn north_body() -> Body {
        Body {
            position: at(0.0, 0.0),
            yaw: 0.0,
        }
    }

    fn free(_: Position, to: Position) -> Option<Position> {
        Some(to)
    }

    fn run_to(runs: &mut Vec<HomingRun>, body: &mut Body, target: Position, end_ms: u64) {
        const FRAME_MS: u64 = 33;
        let mut now = 0;
        while now <= end_ms {
            step(runs, now, body, Some(target), false, free);
            now += FRAME_MS;
        }
    }

    #[test]
    fn the_params_of_a_clip_are_read_by_name() {
        let clip = spear_clip();
        assert!((clip.radius_m - 4.5).abs() < 1e-6);
        assert!((clip.max_angle_rad - 45f32.to_radians()).abs() < 1e-6);
        assert!(clip.exponential && clip.move_to_target);
        assert!((clip.move_to_distance_m - 2.0).abs() < 1e-6);
        assert!(clip.max_move_m.abs() < f32::EPSILON);
    }

    #[test]
    fn a_target_in_the_wedge_draws_the_body_in_to_the_clip_distance() {
        let mut runs = vec![HomingRun::new(spear_clip(), 0, None)];
        let mut body = north_body();
        let target = at(-1.0, 4.0);
        // Past the clip's 0.5 s end.
        run_to(&mut runs, &mut body, target, 700);
        let left = body.position.horizontal_distance(target);
        assert!((left - 2.0).abs() < 0.05, "stops 2 m away, {left}");
        let gap = yaw_gap(body.yaw, yaw_toward(body.position, target));
        assert!(gap.abs() < 0.05, "faces the target, {gap}");
        assert!(runs.is_empty(), "the clip ended");
    }

    #[test]
    fn a_target_out_of_the_wedge_at_the_clip_start_is_not_followed() {
        let mut body = north_body();
        for target in [
            at(0.0, -4.0),
            at(0.0, 6.0),
            Position {
                z: 6.0,
                ..at(0.0, 3.0)
            },
        ] {
            let mut runs = vec![HomingRun::new(spear_clip(), 0, None)];
            run_to(&mut runs, &mut body, target, 500);
            assert_eq!(
                body,
                north_body(),
                "behind, too far or too high: {target:?}"
            );
        }
    }

    #[test]
    fn the_longest_move_holds_the_body_near_its_start() {
        let clip = HomingClip {
            max_move_m: 1.0,
            ..spear_clip()
        };
        let mut runs = vec![HomingRun::new(clip, 0, None)];
        let mut body = north_body();
        run_to(&mut runs, &mut body, at(0.0, 4.4), 500);
        let moved = body.position.horizontal_distance(at(0.0, 0.0));
        assert!((moved - 1.0).abs() < 0.05, "1 m at most, {moved}");
    }

    #[test]
    fn a_wall_ends_the_move_and_a_root_holds_the_body_while_it_turns() {
        let mut runs = vec![HomingRun::new(spear_clip(), 0, None)];
        let mut body = north_body();
        step(
            &mut runs,
            0,
            &mut body,
            Some(at(-2.0, 4.0)),
            false,
            |_, _| None,
        );
        step(
            &mut runs,
            100,
            &mut body,
            Some(at(-2.0, 4.0)),
            false,
            |_, _| None,
        );
        assert_eq!(body.position, at(0.0, 0.0), "the wall stops it");
        assert!(body.yaw > 0.0, "it still turns west");
        let mut rooted = vec![HomingRun::new(spear_clip(), 0, None)];
        let mut held = north_body();
        for now in [0, 100, 200, 300] {
            step(&mut rooted, now, &mut held, Some(at(-2.0, 4.0)), true, free);
        }
        assert_eq!(held.position, at(0.0, 0.0), "the root holds it");
        assert!(held.yaw > 0.0, "it turns");
    }

    #[test]
    fn a_clip_waits_for_its_start_and_a_speed_clip_moves_at_its_speed() {
        let clip = HomingClip {
            start_s: 0.2,
            end_s: None,
            move_to_velocity_mps: 2.0,
            ..spear_clip()
        };
        let mut runs = vec![HomingRun::new(clip, 1_000, Some(2_000))];
        let mut body = north_body();
        step(&mut runs, 1_100, &mut body, Some(at(0.0, 4.0)), false, free);
        assert_eq!(body, north_body(), "not started");
        step(&mut runs, 1_200, &mut body, Some(at(0.0, 4.0)), false, free);
        step(&mut runs, 1_700, &mut body, Some(at(0.0, 4.0)), false, free);
        assert!((body.position.y - 1.0).abs() < 1e-3, "2 m/s for 0.5 s");
    }
}
