// SPDX-License-Identifier: AGPL-3.0-or-later
//! The server's own movement of a player's character, from the keys and the camera yaw of each
//! client input record (movement study 2026-09-20). The client never sends where its character
//! stands; retail's server moves it itself: the own place of state `n` is the place of state
//! `n - 1` moved by input record `n` (in the newchar capture the motion of a new key showed in
//! the place of the record that held it, 30 of 41 key starts).
//!
//! The direction follows the keys and the camera yaw at once (a first diagonal record already
//! moved at 45° at full speed); the speed ramps at the action lists' rates. Up and down follow the
//! floor under the body (the terrain or a floor the caller knows), and the speed is along it
//! (retail ran 5.50 m/s along slopes, median of 33 steady windows, and less over the ground).
//! There is no level collision here: walls and structures are the camera check's
//! ([`crate::camera_check`]).

use crate::movement::{Ground, Position};

/// The forward speed by default, m/s (`playeractions_sprint.actionlist`
/// `ServerValidatedCanSprint` `Speed` 5.5; retail newchar forward 5.498 m/s, median of 360 steady
/// windows, and the same with a key to the side).
pub const SPRINT_SPEED_MPS: f32 = 5.5;
/// The speed in every direction after the walk toggle, m/s (`playeractions_nav.actionlist`
/// `ServerValidatedCanRun` `Speed` 3.25; retail clean walking captures 3.235 m/s forward and back).
pub const RUN_SPEED_MPS: f32 = 3.25;
/// The speed with the back key without the walk toggle, m/s (`playeractions_nav.actionlist`
/// `Speed` 0, 4.00, 0; retail clean back captures 3.98 to 4.00 m/s).
pub const BACK_SPEED_MPS: f32 = 4.0;
/// How fast the speed rises and falls, m/s² (`playeractions_sprint.actionlist`
/// `ServerValidatedCanSprint` `Accel` and `Decel` 20; retail positions one third of a second after
/// a key change fit 20 as well as 15 to 30).
pub const ACCELERATION_MPS2: f32 = 20.0;
/// How fast the speed falls when the keys let go, m/s².
pub const DECELERATION_MPS2: f32 = 20.0;
/// The most the floor rises under a walking body in one input record, m (Observed: the largest
/// rise of the retail own place in one record while walking, 28,600 records of the newchar open
/// world). A higher floor is a structure the server does not know: the body keeps its height.
pub const STEP_UP_M: f32 = 0.376;
/// The most the floor falls under a walking body in one input record before it falls, m
/// (Observed: the 99.9th percentile of the retail drops while walking, same records).
pub const STEP_DOWN_M: f32 = 0.282;
/// The upward speed of a jump, m/s, and the pull down while in the air, m/s² (Observed: a least
/// squares fit of a retail standing jump on flat ground, newchar record 24187, 22 states, root
/// mean square 0.03 m; two more jumps gave 5.26 and 5.49 m/s, 14.8 and 15.5 m/s²).
pub const JUMP_SPEED_MPS: f32 = 5.48;
/// See [`JUMP_SPEED_MPS`].
pub const GRAVITY_MPS2: f32 = 15.24;
/// Input records between a standing jump's key and the lift (Observed: 3 in 7 of the newchar
/// standing jumps; a running jump lifts at once).
pub const STANDING_JUMP_WIND_UP: u8 = 3;
/// How far below the sea a swimming body's feet are, m (Observed: the retail own place at 18.59 m
/// in the tutorial sea of level 20, every record of the swim to shore).
pub const SWIM_DEPTH_M: f32 = 1.41;
/// The speed while the block key is held and a key goes forward, m/s
/// (`playeractions_nav.actionlist` `Block_Nav_Speed` `Speed` 2.0; retail newchar: 5 of the 11
/// steady forward block windows at 1.98 m/s, the others still ramping, and 3 of 3 forward and
/// right at 1.96 m/s).
pub const BLOCK_SPEED_MPS: f32 = 2.0;
/// The speed while the block key is held and no key goes forward, m/s (`playeractions_nav.
/// actionlist` `Nav` `Speed` 1.65 in every direction; retail newchar: back 1.647 m/s, median of 87
/// steady block windows with quartiles 1.647 and 1.674, back and right 1.58 of 19).
pub const BLOCK_BACK_SPEED_MPS: f32 = 1.65;
/// How fast the speed rises and falls while blocking, m/s² (`Block_Nav_Speed` `Accel` and `Decel`
/// 25).
pub const BLOCK_ACCELERATION_MPS2: f32 = 25.0;
/// The speed of a swimming body, m/s (`playeractions_swim.actionlist` `Swim_Nav` `Speed` 3.0;
/// retail newchar tutorial swim: 3.0 m/s after the ramp).
pub const SWIM_SPEED_MPS: f32 = 3.0;
/// How fast a swimming body speeds up, m/s² (`Swim_Nav` `Accel` 3.0: retail swim windows of 10
/// records from 1.25 to 3.0 m/s as it ramps), and slows down (`Decel` 130: retail 6.1 to 3.3 m/s
/// in one record after a dash).
pub const SWIM_ACCELERATION_MPS2: f32 = 3.0;
/// See [`SWIM_ACCELERATION_MPS2`].
pub const SWIM_DECELERATION_MPS2: f32 = 130.0;
/// The speed of a swimming body's dash, a press of the dodge key in the water, m/s
/// (`playeractions_dodge.actionlist` `Dodge` `InheritVelocityMaxSpeed` 6; retail newchar tutorial:
/// 23 of the 25 steady dash windows at 6.0 m/s, from the first record of the press).
pub const SWIM_DASH_SPEED_MPS: f32 = 6.0;
/// How long a dash lasts, s (Observed: 37, 37 and 38 records of 1/30 s in the three tutorial dashes
/// that held the key to their end; the speed went back to the swim speed with the key still
/// held).
pub const SWIM_DASH_S: f32 = 37.0 / 30.0;

/// What the keys of one input record ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Intent {
    /// Forward (1), back (-1) or neither (0).
    pub forward: i8,
    /// Right (1), left (-1) or neither (0).
    pub right: i8,
    /// The jump key is held.
    pub jump: bool,
    /// The walk toggle is pressed in this record.
    pub walk_toggle: bool,
    /// The block key is held.
    pub block: bool,
    /// The dodge key is held.
    pub dodge: bool,
}

impl Intent {
    /// The intent of the held direction keys, without a jump or a walk toggle.
    pub fn of_keys(forward: bool, back: bool, left: bool, right: bool) -> Self {
        Self {
            forward: i8::from(forward) - i8::from(back),
            right: i8::from(right) - i8::from(left),
            ..Self::default()
        }
    }
}

/// One input record for the character.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterInput {
    /// The keys.
    pub intent: Intent,
    /// The camera yaw (radians; 0 faces north, positive turns west).
    pub yaw: f32,
    /// The record's frame time, s.
    pub dt_s: f32,
}

/// Where the character is between the floors.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Vertical {
    /// On a floor.
    Grounded,
    /// A standing jump before the lift: records left.
    WindUp(u8),
    /// In a jump, rising at this speed (falling below 0).
    Jumping(f32),
    /// Falling off a floor, rising at this speed (falling below 0).
    Falling(f32),
}

/// A player's character as the server moves it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Character {
    position: Position,
    speed_mps: f32,
    /// The direction of its motion (a unit vector in the ground plane).
    heading: [f32; 2],
    walking: bool,
    vertical: Vertical,
    jump_held: bool,
    /// It swims: it ended the record before at the swim depth.
    swimming: bool,
    dodge_held: bool,
    /// Seconds left of a swim dash.
    dash_s: f32,
}

/// The unit vectors in the ground plane of camera yaw `yaw`: forward and right.
pub fn yaw_axes(yaw: f32) -> ([f32; 2], [f32; 2]) {
    let (sin, cos) = yaw.sin_cos();
    ([-sin, cos], [cos, sin])
}

impl Character {
    /// A character standing at `position`.
    pub const fn new(position: Position) -> Self {
        Self {
            position,
            speed_mps: 0.0,
            heading: [0.0, 1.0],
            walking: false,
            vertical: Vertical::Grounded,
            jump_held: false,
            swimming: false,
            dodge_held: false,
            dash_s: 0.0,
        }
    }

    /// Where it is.
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Its velocity in the ground plane, m/s.
    pub fn velocity(&self) -> [f32; 2] {
        [
            self.heading[0] * self.speed_mps,
            self.heading[1] * self.speed_mps,
        ]
    }

    /// Whether it stands still: no speed on the ground plane and not in a jump (a fall moves it
    /// only up and down).
    pub fn is_resting(&self) -> bool {
        self.speed_mps == 0.0 && !self.is_jumping()
    }

    /// The direction of its motion (a unit vector in the ground plane).
    pub const fn heading(&self) -> [f32; 2] {
        self.heading
    }

    /// Whether it is in a jump (the client's camera does not follow a jump up and down).
    pub const fn is_jumping(&self) -> bool {
        matches!(self.vertical, Vertical::WindUp(_) | Vertical::Jumping(_))
    }

    /// Puts it at `at`, keeping its motion (the camera check found it elsewhere in a jump).
    pub const fn place(&mut self, at: Position) {
        self.position = at;
    }

    /// Puts it on a floor at `at`, keeping its speed (the camera check found its floor).
    pub const fn land_at(&mut self, at: Position) {
        self.position = at;
        self.vertical = Vertical::Grounded;
    }

    /// Lowers its speed to at most `speed_mps` (the client's body went slower than the keys).
    pub const fn slow_to(&mut self, speed_mps: f32) {
        if speed_mps < self.speed_mps {
            self.speed_mps = speed_mps;
        }
    }

    /// Puts it at `at` standing on its floor (a server move: a spawn, a teleport, a push).
    pub const fn stand_at(&mut self, at: Position) {
        self.position = at;
        self.speed_mps = 0.0;
        self.vertical = Vertical::Grounded;
    }

    /// Moves it by one input record over `ground` with the sea at `sea_level_m`; gives its
    /// motion in the ground plane.
    pub fn step(
        &mut self,
        input: CharacterInput,
        ground: &(impl Ground + ?Sized),
        sea_level_m: f32,
    ) -> [f32; 2] {
        let CharacterInput { intent, yaw, dt_s } = input;
        if intent.walk_toggle {
            self.walking = !self.walking;
        }
        let dodge_pressed = intent.dodge && !self.dodge_held;
        self.dodge_held = intent.dodge;
        if self.swimming && dodge_pressed {
            self.dash_s = SWIM_DASH_S;
        }
        // A record is in the dash when its middle is.
        let dashing = self.swimming && self.dash_s > dt_s / 2.0;
        self.dash_s = (self.dash_s - dt_s).max(0.0);
        let (target, heading) = self.target(intent, yaw, dashing);
        self.heading = heading.unwrap_or(self.heading);
        let (acceleration, deceleration) = self.ramp(intent);
        self.speed_mps = if dashing && heading.is_some() {
            // The dash takes its speed at once (the dodge inherits it).
            target
        } else if target >= self.speed_mps {
            (self.speed_mps + acceleration * dt_s).min(target)
        } else {
            (self.speed_mps - deceleration * dt_s).max(target)
        };
        self.start_jump(intent.jump);
        let distance = self.speed_mps * dt_s;
        let start = self.position;
        let mut next = Position {
            x: start.x + self.heading[0] * distance,
            y: start.y + self.heading[1] * distance,
            z: start.z,
        };
        match self.vertical {
            Vertical::Grounded => self.walk(&mut next, distance, ground),
            Vertical::WindUp(left) => {
                next = start;
                self.vertical = match left.saturating_sub(1) {
                    0 => Vertical::Jumping(JUMP_SPEED_MPS),
                    left => Vertical::WindUp(left),
                };
            }
            Vertical::Jumping(rise) => {
                self.vertical = self
                    .fly(&mut next, rise, dt_s, ground)
                    .map_or(Vertical::Grounded, Vertical::Jumping);
            }
            Vertical::Falling(rise) => {
                self.vertical = self
                    .fly(&mut next, rise, dt_s, ground)
                    .map_or(Vertical::Grounded, Vertical::Falling);
            }
        }
        let swim_floor = sea_level_m - SWIM_DEPTH_M;
        if next.z < swim_floor {
            next.z = swim_floor;
            self.vertical = Vertical::Grounded;
        }
        self.swimming = next.z <= swim_floor;
        if !self.swimming {
            self.dash_s = 0.0;
        }
        self.position = next;
        [next.x - start.x, next.y - start.y]
    }

    /// The speed its keys ask for and the direction, when they ask for one; `dashing` in a swim
    /// dash.
    fn target(&self, intent: Intent, yaw: f32, dashing: bool) -> (f32, Option<[f32; 2]>) {
        if intent.forward == 0 && intent.right == 0 {
            return (0.0, None);
        }
        let (forward, right) = yaw_axes(yaw);
        let (along, side) = (f32::from(intent.forward), f32::from(intent.right));
        let length = along.hypot(side);
        let heading = [
            (along * forward[0] + side * right[0]) / length,
            (along * forward[1] + side * right[1]) / length,
        ];
        let speed = if dashing {
            SWIM_DASH_SPEED_MPS
        } else if self.swimming {
            SWIM_SPEED_MPS
        } else if intent.block && intent.forward > 0 {
            BLOCK_SPEED_MPS
        } else if intent.block {
            BLOCK_BACK_SPEED_MPS
        } else if self.walking {
            RUN_SPEED_MPS
        } else if intent.forward < 0 {
            BACK_SPEED_MPS
        } else {
            SPRINT_SPEED_MPS
        };
        (speed, Some(heading))
    }

    /// How fast its speed rises and falls in its movement now, m/s².
    const fn ramp(&self, intent: Intent) -> (f32, f32) {
        if self.swimming {
            (SWIM_ACCELERATION_MPS2, SWIM_DECELERATION_MPS2)
        } else if intent.block {
            (BLOCK_ACCELERATION_MPS2, BLOCK_ACCELERATION_MPS2)
        } else {
            (ACCELERATION_MPS2, DECELERATION_MPS2)
        }
    }

    /// Starts a jump on a new press of the jump key while it stands on a floor.
    fn start_jump(&mut self, held: bool) {
        let pressed = held && !self.jump_held;
        self.jump_held = held;
        if pressed && self.vertical == Vertical::Grounded {
            self.vertical = if self.speed_mps == 0.0 {
                Vertical::WindUp(STANDING_JUMP_WIND_UP)
            } else {
                Vertical::Jumping(JUMP_SPEED_MPS)
            };
        }
    }

    /// A walk to `next` on the floor: along a floor within a step, the distance is along the
    /// slope; a floor further down starts a fall; a floor further up, or none, is not known
    /// here and the body keeps its height.
    fn walk(&mut self, next: &mut Position, distance: f32, ground: &(impl Ground + ?Sized)) {
        let Some(floor) = ground.floor_at(next.x, next.y, next.z) else {
            return;
        };
        let rise = floor - next.z;
        if rise > STEP_UP_M {
            return;
        }
        if -rise > STEP_DOWN_M {
            self.vertical = Vertical::Falling(0.0);
            return;
        }
        if distance > 0.0 {
            let along = distance / distance.hypot(rise);
            let start = self.position;
            next.x = start.x + (next.x - start.x) * along;
            next.y = start.y + (next.y - start.y) * along;
        }
        next.z = ground.floor_at(next.x, next.y, next.z).unwrap_or(floor);
    }

    /// One record in the air: up by the rise, pulled down, and onto the floor when it reaches it.
    /// Gives the rise after the record, or `None` when it landed.
    fn fly(
        &self,
        next: &mut Position,
        rise: f32,
        dt_s: f32,
        ground: &(impl Ground + ?Sized),
    ) -> Option<f32> {
        next.z += rise * dt_s - GRAVITY_MPS2 * dt_s * dt_s / 2.0;
        match ground.floor_at(next.x, next.y, self.position.z) {
            Some(floor) if next.z <= floor => {
                next.z = floor;
                None
            }
            _ => Some(rise - GRAVITY_MPS2 * dt_s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One input record at 30 Hz.
    const DT: f32 = 1.0 / 30.0;
    const SEA: f32 = -100.0;
    const CLOSE: f32 = 1e-4;

    /// Flat ground at height 0.
    struct Flat;

    impl Ground for Flat {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
    }

    /// Ground that rises 1 m per metre east.
    struct Ramp;

    impl Ground for Ramp {
        fn height_at(&self, x: f32, _: f32) -> Option<f32> {
            Some(x)
        }
    }

    fn origin() -> Position {
        Position {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    fn input(forward: i8, right: i8) -> CharacterInput {
        CharacterInput {
            intent: Intent {
                forward,
                right,
                ..Intent::default()
            },
            yaw: 0.0,
            dt_s: DT,
        }
    }

    fn run(character: &mut Character, records: usize, forward: i8, right: i8) {
        for _ in 0..records {
            character.step(input(forward, right), &Flat, SEA);
        }
    }

    #[test]
    fn the_keys_move_it_along_the_camera_yaw_at_the_action_list_speeds() {
        // A second of each key: the speed reaches its top within 0.28 s.
        let cases = [
            (1, 0, [0.0, 1.0], SPRINT_SPEED_MPS),
            (-1, 0, [0.0, -1.0], BACK_SPEED_MPS),
            (0, 1, [1.0, 0.0], SPRINT_SPEED_MPS),
            (0, -1, [-1.0, 0.0], SPRINT_SPEED_MPS),
        ];
        for (forward, right, direction, speed) in cases {
            let mut character = Character::new(origin());
            run(&mut character, 30, forward, right);
            let velocity = character.velocity();
            assert!(
                (velocity[0] - direction[0] * speed).abs() < CLOSE,
                "{velocity:?}"
            );
            assert!(
                (velocity[1] - direction[1] * speed).abs() < CLOSE,
                "{velocity:?}"
            );
        }
        // West of north at yaw 90°: forward is -x.
        let mut west = Character::new(origin());
        for _ in 0..30 {
            west.step(
                CharacterInput {
                    yaw: std::f32::consts::FRAC_PI_2,
                    ..input(1, 0)
                },
                &Flat,
                SEA,
            );
        }
        assert!(west.position().x < -4.0 && west.position().y.abs() < CLOSE);
    }

    #[test]
    fn a_diagonal_turns_at_once_and_keeps_the_speed() {
        let mut character = Character::new(origin());
        run(&mut character, 30, 1, 0);
        character.step(input(1, 1), &Flat, SEA);
        let [x, y] = character.velocity();
        assert!((x - y).abs() < CLOSE, "45°: {x} {y}");
        assert!((x.hypot(y) - SPRINT_SPEED_MPS).abs() < CLOSE);
    }

    #[test]
    fn the_speed_ramps_up_and_down_at_twenty() {
        let mut character = Character::new(origin());
        character.step(input(1, 0), &Flat, SEA);
        assert!((character.velocity()[1] - ACCELERATION_MPS2 * DT).abs() < CLOSE);
        run(&mut character, 30, 1, 0);
        character.step(input(0, 0), &Flat, SEA);
        assert!(
            (character.velocity()[1] - (SPRINT_SPEED_MPS - DECELERATION_MPS2 * DT)).abs() < CLOSE
        );
        run(&mut character, 30, 0, 0);
        assert!(character.is_resting());
    }

    #[test]
    fn the_walk_toggle_switches_to_the_run_speed_in_every_direction() {
        let mut character = Character::new(origin());
        character.step(
            CharacterInput {
                intent: Intent {
                    forward: 1,
                    walk_toggle: true,
                    ..Intent::default()
                },
                ..input(1, 0)
            },
            &Flat,
            SEA,
        );
        run(&mut character, 30, 1, 0);
        assert!((character.velocity()[1] - RUN_SPEED_MPS).abs() < CLOSE);
        run(&mut character, 30, -1, 0);
        assert!((character.velocity()[1] + RUN_SPEED_MPS).abs() < CLOSE);
    }

    #[test]
    fn on_a_slope_the_speed_is_along_the_ground() {
        let mut character = Character::new(origin());
        // East up a 45° ramp.
        for _ in 0..60 {
            character.step(input(0, 1), &Ramp, SEA);
        }
        let before = character.position();
        character.step(input(0, 1), &Ramp, SEA);
        let after = character.position();
        let along = (after.x - before.x).hypot(after.z - before.z);
        assert!((along - SPRINT_SPEED_MPS * DT).abs() < 1e-3, "{along}");
        assert!((after.z - after.x).abs() < CLOSE, "on the ramp");
    }

    #[test]
    fn a_standing_jump_waits_three_records_then_rises_and_lands() {
        let mut character = Character::new(origin());
        let jump = CharacterInput {
            intent: Intent {
                jump: true,
                ..Intent::default()
            },
            ..input(0, 0)
        };
        for _ in 0..STANDING_JUMP_WIND_UP {
            character.step(jump, &Flat, SEA);
            assert!(character.position().z.abs() < CLOSE, "wind-up");
        }
        let mut peak: f32 = 0.0;
        for _ in 0..40 {
            character.step(jump, &Flat, SEA);
            peak = peak.max(character.position().z);
        }
        // The retail standing jump peaked at 1.01 m.
        assert!((peak - 1.0).abs() < 0.05, "{peak}");
        assert!(
            character.is_resting(),
            "landed, and holding the key does not jump again"
        );
    }

    #[test]
    fn a_floor_far_below_is_a_fall_and_one_far_above_is_not_climbed() {
        struct Cliff;
        impl Ground for Cliff {
            fn height_at(&self, x: f32, _: f32) -> Option<f32> {
                Some(if x < 0.1 { 0.0 } else { -10.0 })
            }
        }
        let mut character = Character::new(origin());
        run(&mut character, 1, 0, 1);
        for _ in 0..30 {
            character.step(input(0, 1), &Cliff, SEA);
        }
        assert!(
            character.position().z < 0.0 && character.position().z > -10.0,
            "falling"
        );
        for _ in 0..60 {
            character.step(input(0, 0), &Cliff, SEA);
        }
        assert!((character.position().z + 10.0).abs() < CLOSE, "landed");
        struct Wall;
        impl Ground for Wall {
            fn height_at(&self, x: f32, _: f32) -> Option<f32> {
                Some(if x < 0.1 { 0.0 } else { 3.0 })
            }
        }
        let mut walker = Character::new(origin());
        run(&mut walker, 30, 0, 1);
        for _ in 0..30 {
            walker.step(input(0, 1), &Wall, SEA);
        }
        assert!(walker.position().z.abs() < CLOSE, "keeps its height");
    }

    #[test]
    fn in_deep_water_it_swims_below_the_sea() {
        const TUTORIAL_SEA: f32 = 20.0;
        struct SeaBed;
        impl Ground for SeaBed {
            fn height_at(&self, _: f32, _: f32) -> Option<f32> {
                Some(5.0)
            }
        }
        let mut character = Character::new(Position {
            x: 0.0,
            y: 0.0,
            z: TUTORIAL_SEA - SWIM_DEPTH_M,
        });
        run(&mut character, 1, 0, 0);
        for _ in 0..30 {
            character.step(input(1, 0), &SeaBed, TUTORIAL_SEA);
        }
        assert!((character.position().z - (TUTORIAL_SEA - SWIM_DEPTH_M)).abs() < CLOSE);
    }

    fn moving(forward: i8, right: i8, block: bool, dodge: bool) -> CharacterInput {
        CharacterInput {
            intent: Intent {
                block,
                dodge,
                ..input(forward, right).intent
            },
            ..input(forward, right)
        }
    }

    #[test]
    fn blocking_moves_at_the_block_speeds() {
        let mut character = Character::new(origin());
        character.step(moving(1, 0, true, false), &Flat, SEA);
        assert!(
            (character.velocity()[1] - BLOCK_ACCELERATION_MPS2 * DT).abs() < CLOSE,
            "the block ramp"
        );
        for _ in 0..30 {
            character.step(moving(1, 0, true, false), &Flat, SEA);
        }
        assert!((character.velocity()[1] - BLOCK_SPEED_MPS).abs() < CLOSE);
        for _ in 0..30 {
            character.step(moving(-1, 0, true, false), &Flat, SEA);
        }
        assert!((character.velocity()[1] + BLOCK_BACK_SPEED_MPS).abs() < CLOSE);
        for _ in 0..30 {
            character.step(moving(0, 1, true, false), &Flat, SEA);
        }
        assert!((character.velocity()[0] - BLOCK_BACK_SPEED_MPS).abs() < CLOSE);
    }

    /// Water 5 m deep everywhere, the sea at the tutorial's 20 m.
    const TUTORIAL_SEA: f32 = 20.0;
    struct SeaBed;
    impl Ground for SeaBed {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(5.0)
        }
    }

    fn swimmer() -> Character {
        let mut character = Character::new(Position {
            x: 0.0,
            y: 0.0,
            z: TUTORIAL_SEA - SWIM_DEPTH_M,
        });
        character.step(input(0, 0), &SeaBed, TUTORIAL_SEA);
        character
    }

    #[test]
    fn a_swimmer_ramps_to_the_swim_speed_and_dashes_on_a_dodge_press() {
        let mut character = swimmer();
        character.step(input(1, 0), &SeaBed, TUTORIAL_SEA);
        assert!((character.velocity()[1] - SWIM_ACCELERATION_MPS2 * DT).abs() < CLOSE);
        for _ in 0..60 {
            character.step(input(1, 0), &SeaBed, TUTORIAL_SEA);
        }
        assert!((character.velocity()[1] - SWIM_SPEED_MPS).abs() < CLOSE);
        character.step(moving(1, 0, false, true), &SeaBed, TUTORIAL_SEA);
        assert!(
            (character.velocity()[1] - SWIM_DASH_SPEED_MPS).abs() < CLOSE,
            "the dash at once"
        );
        let mut records = 1;
        loop {
            character.step(moving(1, 0, false, true), &SeaBed, TUTORIAL_SEA);
            if (character.velocity()[1] - SWIM_DASH_SPEED_MPS).abs() >= CLOSE {
                break;
            }
            records += 1;
        }
        // 37 records of 1/30 s, held to the end.
        assert_eq!(records, 37);
        assert!(
            (character.velocity()[1] - SWIM_SPEED_MPS).abs() < CLOSE,
            "back to the swim speed with the key held"
        );
    }

    #[test]
    fn a_dodge_on_land_is_no_dash() {
        let mut character = Character::new(origin());
        run(&mut character, 30, 1, 0);
        character.step(moving(1, 0, false, true), &Flat, SEA);
        assert!((character.velocity()[1] - SPRINT_SPEED_MPS).abs() < CLOSE);
    }
}
