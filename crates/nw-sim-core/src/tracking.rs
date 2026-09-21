// SPDX-License-Identifier: AGPL-3.0-or-later
//! A player's body as the server keeps it: moved by its keys ([`crate::character`]) and held to
//! the client's own view ([`crate::camera_check`]) where the server cannot see the level.
//!
//! The rule, per input record: the character moves; the camera check solves where the client's
//! camera puts the body; when the two are more than [`RESYNC_M`] apart on the ground plane, the
//! body goes where the camera puts it (a wall stopped the client, a dodge or an attack moved it,
//! the server's speed was not the client's). When that pulls the body back against its motion,
//! the character's speed drops to the camera body's own speed along it since the record before (a
//! client held at a wall stays there instead of being pulled back record after record; replay of
//! the newchar capture: 1,433 pull-backs instead of 1,622 in the tutorial and 1,638 instead of
//! 2,022 in the open world, a median of 0.22 m instead of 0.29 m in the tutorial and 0.16 m
//! instead of 0.14 m in the open world). The height of a body on a floor is the floor under the
//! body nearest the camera's height when one is within [`FLOOR_PICK_M`] of it (the terrain, a
//! walked floor, or the swim floor of a sea deep enough), else the camera's height (a deck, stairs
//! or a roof the server does not know), and the body stands there; a body in a jump or a fall
//! keeps the character's height (retail's jump suppresses the camera's vertical motion,
//! `playeractions_traversal.actionlist` `SuppressVerticalCameraMovement`, and the camera trails a
//! fall), and so does a body under a camera that comes down from above it, a camera that still
//! trails the fall the body landed from (retail tutorial sea: the camera's body 1.1 m above the
//! landed place, then 0.75, 0.48, 0.36, 0.20 and 0.15 m in the next records). The client shows the
//! server's own place, so a height taken from a trailing camera would hold the body and its client
//! up there (run 13, 2026-09-20). A record whose camera does not follow the body changes nothing.

use crate::camera_check::{CameraCheck, CameraView};
use crate::character::{Character, CharacterInput, swim_floor};
use crate::movement::{Ground, Position};

/// How far the server's body may be from the camera's body before it goes there, m (Observed:
/// replayed with the retail motion itself, the camera body was within 0.164 m of the retail own
/// place in 75% of the moving records of the newchar open world, 0.267 m in the tutorial; a
/// replay of the retail keys erred by a median 0.15 m with 0.1 m, 0.16 m with 0.15 m and 0.16 m
/// with 0.2 m in the open world, 0.35, 0.22 and 0.55 m in the tutorial).
pub const RESYNC_M: f32 = 0.15;
/// A floor this near the camera's height is the floor the body stands on, m (the camera height
/// trails a moving body over slopes by up to 1 m: retail running camera rise 1.03 to 2.15 m).
pub const FLOOR_PICK_M: f32 = 1.0;
/// Records after the client starts an action (an attack, a dodge, an ability) while its moves may
/// carry the body without the movement keys, so the camera check does not hold it still
/// (Observed: after the 20 actions the newchar tutorial started without a movement key, the own
/// place last moved 25 to 60 records later, median 52, by up to 8.5 m; each new action starts
/// the count again).
pub const ACTION_RECORDS: u8 = 52;

/// One input record for a tracked body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackInput {
    /// The keys, the yaw and the frame time.
    pub character: CharacterInput,
    /// The record's camera, when the client's camera carries the body.
    pub view: Option<CameraView>,
    /// The record pressed a key that changes the camera's boom.
    pub boom_key: bool,
    /// The client starts an action in this record.
    pub action_started: bool,
}

/// What one record did to a tracked body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracked {
    /// Where the body is.
    pub position: Position,
    /// The camera put the body elsewhere than the keys.
    pub resynced: bool,
    /// The camera followed the body.
    pub consistent: bool,
    /// The body stands on a floor (it is not in a jump or a fall).
    pub on_floor: bool,
}

/// A player's body.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackedBody {
    character: Character,
    camera: CameraCheck,
    /// Where the camera put the body in the record before, when it did.
    last_seen: Option<Position>,
    /// Records left while an action the client started may move the body.
    acting_records: u8,
}

impl TrackedBody {
    /// A body standing at `at`.
    pub fn new(at: Position) -> Self {
        Self {
            character: Character::new(at),
            camera: CameraCheck::default(),
            last_seen: None,
            acting_records: 0,
        }
    }

    /// Where it is.
    pub const fn position(&self) -> Position {
        self.character.position()
    }

    /// Its velocity in the ground plane, m/s.
    pub fn velocity(&self) -> [f32; 2] {
        self.character.velocity()
    }

    /// Whether it stands still: no speed on the ground plane, not in a jump, and no action of
    /// the client moving it.
    pub fn is_resting(&self) -> bool {
        self.character.is_resting() && self.acting_records == 0
    }

    /// The camera boom it holds, once a camera fitted.
    pub const fn boom_m(&self) -> Option<f32> {
        self.camera.boom_m()
    }

    /// The server puts it at `at` (a spawn, a teleport, a push): it stands there, and the camera
    /// that follows it starts again.
    pub fn stand_at(&mut self, at: Position) {
        self.character.stand_at(at);
        self.camera.forget();
        self.last_seen = None;
    }

    /// The server puts it at `at` in the air (a teleport, a respawn): it falls from there to the
    /// floor it finds, and the camera that follows it starts again.
    pub fn drop_at(&mut self, at: Position) {
        self.character.drop_at(at);
        self.camera.forget();
        self.last_seen = None;
    }

    /// Moves it by one input record over `ground`, with the sea at `sea_level_m`.
    pub fn record(
        &mut self,
        input: TrackInput,
        ground: &(impl Ground + ?Sized),
        sea_level_m: f32,
    ) -> Tracked {
        if input.action_started {
            self.acting_records = ACTION_RECORDS;
        }
        let moved = self.character.step(input.character, ground, sea_level_m);
        let mut position = self.character.position();
        let resting = self.is_resting();
        self.acting_records = self.acting_records.saturating_sub(1);
        let seen = input.view.and_then(|view| {
            self.camera.observe(
                view,
                position,
                moved,
                self.character.velocity(),
                resting,
                input.boom_key,
            )
        });
        let last_seen = std::mem::replace(&mut self.last_seen, seen.map(|seen| seen.at));
        let Some(seen) = seen else {
            return Tracked {
                position,
                resynced: false,
                consistent: false,
                on_floor: self.character.is_grounded(),
            };
        };
        let resynced = position.horizontal_distance(seen.at) > RESYNC_M;
        if resynced {
            let heading = self.character.heading();
            let along = |from: Position, to: Position| {
                (to.x - from.x) * heading[0] + (to.y - from.y) * heading[1]
            };
            if along(position, seen.at) < 0.0
                && let Some(last_seen) = last_seen
            {
                self.character
                    .slow_to((along(last_seen, seen.at) / input.character.dt_s).max(0.0));
            }
            position.x = seen.at.x;
            position.y = seen.at.y;
        }
        let trailing = last_seen.is_some_and(|last| seen.at.z < last.z) && seen.at.z > position.z;
        if !self.character.is_grounded() || trailing {
            self.character.place(position);
        } else {
            position.z = floor_near(ground, position, seen.at.z, sea_level_m)
                .filter(|floor| (floor - seen.at.z).abs() <= FLOOR_PICK_M)
                .unwrap_or(seen.at.z);
            self.character.land_at(position);
        }
        Tracked {
            position,
            resynced,
            consistent: true,
            on_floor: self.character.is_grounded(),
        }
    }
}

/// The known floor at the ground-plane point of `at` nearest the height `z`: the floor of
/// `ground` there, or the swim floor of the sea at `sea_level_m` where the terrain is under it.
fn floor_near(
    ground: &(impl Ground + ?Sized),
    at: Position,
    z: f32,
    sea_level_m: f32,
) -> Option<f32> {
    let swim = swim_floor(sea_level_m);
    let sea = ground
        .height_at(at.x, at.y)
        .is_none_or(|terrain| terrain < swim)
        .then_some(swim);
    ground
        .floor_at(at.x, at.y, z)
        .into_iter()
        .chain(sea)
        .min_by(|a, b| (a - z).abs().total_cmp(&(b - z).abs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera_check::{CAMERA_LAG_S, CAMERA_RISE_M, SHOULDER_RIGHT_M};
    use crate::character::CharacterInput;
    use crate::character::{ACCELERATION_MPS2, Intent, SPRINT_SPEED_MPS, yaw_axes};

    const DT: f32 = 1.0 / 30.0;
    const SEA: f32 = -100.0;
    const BOOM: f32 = 4.0;
    const PITCH: f32 = 0.2;
    /// The height of the flat ground.
    const GROUND_Z: f32 = 10.0;

    struct Flat;

    impl Ground for Flat {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_Z)
        }
    }

    /// No terrain: a deck the server does not know.
    struct Nothing;

    impl Ground for Nothing {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            None
        }
    }

    /// The camera of a body at `body` looking north with boom `boom`, trailing `velocity`.
    fn view_of(body: Position, boom: f32, velocity: [f32; 2]) -> CameraView {
        let (forward, right) = yaw_axes(0.0);
        let level = boom * PITCH.cos();
        CameraView {
            camera: Position {
                x: body.x - level * forward[0] + SHOULDER_RIGHT_M * right[0]
                    - CAMERA_LAG_S * velocity[0],
                y: body.y - level * forward[1] + SHOULDER_RIGHT_M * right[1]
                    - CAMERA_LAG_S * velocity[1],
                z: body.z + CAMERA_RISE_M + boom * PITCH.sin(),
            },
            yaw: 0.0,
            pitch: PITCH,
        }
    }

    fn input(forward: i8, view: CameraView) -> TrackInput {
        TrackInput {
            character: CharacterInput {
                intent: Intent {
                    forward,
                    ..Intent::default()
                },
                yaw: 0.0,
                dt_s: DT,
            },
            view: Some(view),
            boom_key: false,
            action_started: false,
        }
    }

    fn start() -> Position {
        Position {
            x: 100.0,
            y: 100.0,
            z: GROUND_Z,
        }
    }

    #[test]
    fn a_client_running_free_keeps_the_body_of_the_keys() {
        // The client runs north as the server does: its camera follows the same body.
        let mut body = TrackedBody::new(start());
        let mut client = Character::new(start());
        for _ in 0..90 {
            client.step(
                CharacterInput {
                    intent: Intent {
                        forward: 1,
                        ..Intent::default()
                    },
                    yaw: 0.0,
                    dt_s: DT,
                },
                &Flat,
                SEA,
            );
            let tracked = body.record(
                input(1, view_of(client.position(), BOOM, client.velocity())),
                &Flat,
                SEA,
            );
            assert!(!tracked.resynced, "{tracked:?}");
            assert!(tracked.position.horizontal_distance(client.position()) < 0.05);
            assert!(
                (tracked.position.z - GROUND_Z).abs() < 1e-3,
                "on the terrain"
            );
        }
    }

    #[test]
    fn a_client_stopped_by_a_wall_holds_the_body_at_the_wall() {
        // The client's body stops at a wall 1 m north while the key stays down; the server's
        // runs on until the camera shows the body is not there.
        let mut body = TrackedBody::new(start());
        let wall = Position {
            y: start().y + 1.0,
            ..start()
        };
        let mut client = Character::new(start());
        const RECORDS: u32 = 120;
        let mut resyncs = 0;
        let mut last = start();
        for _ in 0..RECORDS {
            if client.position().y < wall.y {
                client.step(
                    CharacterInput {
                        intent: Intent {
                            forward: 1,
                            ..Intent::default()
                        },
                        yaw: 0.0,
                        dt_s: DT,
                    },
                    &Flat,
                    SEA,
                );
            }
            let stopped = client.position().y >= wall.y;
            let velocity = if stopped { [0.0; 2] } else { client.velocity() };
            let tracked = body.record(
                input(1, view_of(client.position(), BOOM, velocity)),
                &Flat,
                SEA,
            );
            resyncs += u32::from(tracked.resynced);
            last = tracked.position;
        }
        // The server's body stays within the threshold, the camera's trail of a running body and
        // one record of running of the wall: it never runs away into the wall. Each pull-back
        // stops the character, so it takes records to drift past the threshold again.
        let bound = RESYNC_M + (CAMERA_LAG_S + DT) * SPRINT_SPEED_MPS;
        assert!(last.horizontal_distance(wall) <= bound, "{last:?}");
        assert!(resyncs > 0 && resyncs * 2 <= RECORDS, "{resyncs}");
    }

    #[test]
    fn on_a_deck_the_server_does_not_know_the_body_stands_at_the_camera_height() {
        let deck = Position {
            z: 89.42,
            ..start()
        };
        let mut body = TrackedBody::new(deck);
        let tracked = body.record(input(0, view_of(deck, BOOM, [0.0; 2])), &Nothing, SEA);
        assert!(tracked.consistent);
        assert!((tracked.position.z - deck.z).abs() < 0.01, "{tracked:?}");
    }

    #[test]
    fn a_long_boom_still_puts_the_body_on_the_terrain() {
        // Run 12 (2026-09-18): the client's zoom held an 8.1 m boom and the old camera fit put
        // the body 0.7 to 1.6 m above the boar's terrain. The terrain is within reach of the
        // camera's height, so the body stands on it.
        const RUN_12_BOOM_M: f32 = 8.1;
        let mut body = TrackedBody::new(start());
        for _ in 0..10 {
            let tracked = body.record(
                input(0, view_of(start(), RUN_12_BOOM_M, [0.0; 2])),
                &Flat,
                SEA,
            );
            assert!((tracked.position.z - GROUND_Z).abs() < 1e-3, "{tracked:?}");
        }
        assert!((body.boom_m().expect("fitted") - RUN_12_BOOM_M).abs() < 0.01);
    }

    #[test]
    fn a_camera_that_does_not_follow_leaves_the_keys_alone() {
        let mut body = TrackedBody::new(start());
        body.record(input(0, view_of(start(), BOOM, [0.0; 2])), &Flat, SEA);
        let far = Position {
            x: start().x + 50.0,
            ..start()
        };
        let mut cutscene = view_of(far, BOOM, [0.0; 2]);
        cutscene.yaw = 2.0;
        let tracked = body.record(input(1, cutscene), &Flat, SEA);
        assert!(!tracked.consistent && !tracked.resynced);
        assert!((tracked.position.y - start().y - ACCELERATION_MPS2 * DT * DT).abs() < 1e-4);
    }

    #[test]
    fn a_server_move_starts_the_camera_again() {
        let mut body = TrackedBody::new(start());
        body.record(input(0, view_of(start(), BOOM, [0.0; 2])), &Flat, SEA);
        let moved = Position {
            x: 500.0,
            ..start()
        };
        body.stand_at(moved);
        assert_eq!(body.boom_m(), None);
        let tracked = body.record(input(0, view_of(moved, 6.0, [0.0; 2])), &Flat, SEA);
        assert!(tracked.consistent && !tracked.resynced);
        assert!(tracked.position.horizontal_distance(moved) < 0.01);
    }

    /// The tutorial sea (the `ftue_v2` ocean level) and the terrain under the move into it, m.
    const TUTORIAL_SEA_M: f32 = 20.0;
    const TUTORIAL_SEABED_M: f32 = 10.82;
    /// Records that the test client shows the server's place late: the client shows the
    /// server's own place, a few records after the server moved it.
    const CLIENT_LAG_RECORDS: usize = 3;
    /// Where the camera's body was above the landed retail place in the records after its drop
    /// into the tutorial sea (newchar records 3110 to 3114), m.
    const RETAIL_CAMERA_TRAIL_M: [f32; 5] = [0.75, 0.48, 0.36, 0.20, 0.15];

    struct TutorialSea;

    impl Ground for TutorialSea {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(TUTORIAL_SEABED_M)
        }
    }

    /// A client at rest that shows the server's places `CLIENT_LAG_RECORDS` late.
    struct LateClient {
        shown: std::collections::VecDeque<Position>,
    }

    impl LateClient {
        fn at(place: Position) -> Self {
            Self {
                shown: std::iter::repeat_n(place, CLIENT_LAG_RECORDS).collect(),
            }
        }

        /// The camera of the place it shows now; it gets `server` for later.
        fn camera(&mut self, server: Position) -> CameraView {
            self.shown.push_back(server);
            let shown = self.shown.pop_front().expect("lagged places");
            view_of(shown, BOOM, [0.0; 2])
        }
    }

    #[test]
    fn a_dropped_body_falls_while_its_client_s_camera_shows_it_late() {
        // Run 13: after the move into the water the server's body took the height of the camera
        // of a client that showed the server's place, and both hung 7.6 m above the sea.
        let placed = Position {
            z: 26.16,
            ..start()
        };
        let mut body = TrackedBody::new(start());
        body.drop_at(placed);
        let mut free = Character::new(placed);
        free.drop_at(placed);
        let mut client = LateClient::at(placed);
        let mut server = placed;
        for record in 0..60 {
            let tracked = body.record(
                input(0, client.camera(server)),
                &TutorialSea,
                TUTORIAL_SEA_M,
            );
            free.step(
                input(0, view_of(placed, BOOM, [0.0; 2])).character,
                &TutorialSea,
                TUTORIAL_SEA_M,
            );
            assert!(
                (tracked.position.z - free.position().z).abs() < 1e-4,
                "record {record}: the camera held the drop at {:?}",
                tracked.position
            );
            assert_eq!(tracked.on_floor, free.is_grounded(), "no floor in the air");
            server = tracked.position;
        }
        assert!(
            (server.z - swim_floor(TUTORIAL_SEA_M)).abs() < 1e-4,
            "{server:?}"
        );
    }

    #[test]
    fn a_camera_settling_over_the_sea_keeps_the_landed_body_on_the_swim_floor() {
        let swim = swim_floor(TUTORIAL_SEA_M);
        let placed = Position {
            z: swim + 3.0,
            ..start()
        };
        let mut body = TrackedBody::new(start());
        body.drop_at(placed);
        let mut client = LateClient::at(placed);
        let mut server = placed;
        while !body.character.is_grounded() {
            server = body
                .record(
                    input(0, client.camera(server)),
                    &TutorialSea,
                    TUTORIAL_SEA_M,
                )
                .position;
        }
        let trail = RETAIL_CAMERA_TRAIL_M.into_iter().chain([0.0]);
        for above in trail {
            let camera = view_of(
                Position {
                    z: swim + above,
                    ..server
                },
                BOOM,
                [0.0; 2],
            );
            let tracked = body.record(input(0, camera), &TutorialSea, TUTORIAL_SEA_M);
            assert!(
                (tracked.position.z - swim).abs() < 1e-4,
                "{above} m above: {tracked:?}"
            );
        }
    }

    /// The retail replay data: a 1 m terrain grid and input records with the retail places.
    struct Replay {
        x0: f32,
        y0: f32,
        columns: usize,
        heights: Vec<f32>,
        start: Position,
        records: Vec<(TrackInput, Option<Position>)>,
    }

    impl Ground for Replay {
        fn height_at(&self, x: f32, y: f32) -> Option<f32> {
            let (column, row) = (x - self.x0, y - self.y0);
            if column < 0.0 || row < 0.0 {
                return None;
            }
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a grid of a few dozen metres"
            )]
            let (i, j) = (column.floor() as usize, row.floor() as usize);
            let height = |i: usize, j: usize| self.heights.get(j * self.columns + i).copied();
            if i + 1 >= self.columns {
                return None;
            }
            let (u, w) = (column.fract(), row.fract());
            let south = height(i, j)? * (1.0 - u) + height(i + 1, j)? * u;
            let north = height(i, j + 1)? * (1.0 - u) + height(i + 1, j + 1)? * u;
            Some(south * (1.0 - w) + north * w)
        }
    }

    fn parse_replay(text: &str) -> Replay {
        let mut lines = text.lines().filter(|line| !line.starts_with('#'));
        let numbers = |line: &str| -> Vec<f32> {
            line.split_whitespace()
                .filter_map(|word| word.parse().ok())
                .collect()
        };
        let grid = numbers(lines.next().expect("grid"));
        let [x0, y0, columns, rows] = grid[..] else {
            panic!("grid line: {grid:?}");
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a grid of a few dozen metres"
        )]
        let (columns, rows) = (columns as usize, rows as usize);
        let heights: Vec<f32> = (0..rows)
            .flat_map(|_| numbers(lines.next().expect("heights")))
            .collect();
        let start = numbers(lines.next().expect("start"));
        let start = Position {
            x: start[0],
            y: start[1],
            z: start[2],
        };
        let flag = |value: f32| value > 0.5;
        let records = lines
            .map(|line| {
                let v = numbers(line);
                let (keys, place) = v[6..].split_at(8);
                let intent = Intent {
                    jump: flag(keys[4]),
                    walk_toggle: flag(keys[5]),
                    ..Intent::of_keys(flag(keys[0]), flag(keys[1]), flag(keys[2]), flag(keys[3]))
                };
                let input = TrackInput {
                    character: CharacterInput {
                        intent,
                        yaw: v[1],
                        dt_s: v[0],
                    },
                    view: Some(CameraView {
                        camera: Position {
                            x: v[3],
                            y: v[4],
                            z: v[5],
                        },
                        yaw: v[1],
                        pitch: v[2],
                    }),
                    boom_key: flag(keys[6]),
                    action_started: flag(keys[7]),
                };
                let place = (place.len() == 3).then(|| Position {
                    x: place[0],
                    y: place[1],
                    z: place[2],
                });
                (input, place)
            })
            .collect();
        Replay {
            x0,
            y0,
            columns,
            heights,
            start,
            records,
        }
    }

    #[test]
    fn a_retail_open_world_walk_replays_within_the_camera_models_accuracy() {
        // Retail newchar capture, open world, 1,417 records (47 s): running, turning, stops,
        // zooms and actions. The server's body from the retail keys and cameras against retail's
        // own places (movement study 2026-09-20: the whole open world part replayed with a median
        // of 0.16 m; the body of the camera fit erred by a median 0.22 m while moving).
        const OPEN_WORLD_SEA_M: f32 = 40.0;
        const MEDIAN_M: f32 = 0.2;
        const NINETIETH_M: f32 = 1.2;
        let replay = parse_replay(include_str!("testdata/retail-open-world-walk.txt"));
        let mut body = TrackedBody::new(replay.start);
        let mut truth = replay.start;
        let mut errors: Vec<f32> = replay
            .records
            .iter()
            .map(|(input, place)| {
                let tracked = body.record(*input, &replay, OPEN_WORLD_SEA_M);
                truth = place.unwrap_or(truth);
                tracked.position.horizontal_distance(truth)
            })
            .collect();
        errors.sort_by(f32::total_cmp);
        let median = errors[errors.len() / 2];
        let ninetieth = errors[errors.len() * 9 / 10];
        assert!(median < MEDIAN_M, "median {median} m");
        assert!(ninetieth < NINETIETH_M, "90th percentile {ninetieth} m");
    }
}
