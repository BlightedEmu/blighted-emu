// SPDX-License-Identifier: AGPL-3.0-or-later
//! A bot that walks as the game client walks: one input record per frame with a rising counter,
//! the frame time, the view, the camera and the keys or the pad's stick. The server moves a
//! client's body by each record's keys and holds it to the record's camera (the sim's
//! `move_from_input`); a bot that sends only points takes the server's test path instead, which
//! numbers no records (bot soak 2026-11-21: the state number fix of the remote players was not
//! exercised). The bot moves its own body by the same movement rules over the level's ground, as
//! the client moves its own, and puts its camera on a boom behind that body.

use clap::ValueEnum;
use nw_messages::{ClientInputMsg, FrameMove, InputPosition, InputRecord, MoveKeys};
use nw_sim_core::camera_check::ZOOM_BOOMS_M;
use nw_sim_core::{
    CameraView, CharacterInput, Ground, Intent, Position, TrackInput, TrackedBody, yaw_toward,
};

/// The device whose input a walking bot sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InputDevice {
    /// The keyboard: the forward key.
    Keys,
    /// A game pad: the left stick pushed forward.
    Pad,
}

/// The boom of the bot's camera, m: the 4 m zoom length, one of the client's resting booms
/// ([`ZOOM_BOOMS_M`]).
pub const CAMERA_BOOM_M: f32 = ZOOM_BOOMS_M[2];
/// The pitch of the bot's view, radians: a little down, toward the ground ahead (our choice; the
/// camera check holds any pitch).
pub const VIEW_PITCH: f32 = 0.3;
/// The bot is at its goal within this, m, and holds no key.
pub const ARRIVED_M: f32 = 0.3;
/// The server's place of the bot is this far from the bot's own body, m: the server moved it (a
/// teleport, a refused move), and the bot's body goes there, as the client takes the server's
/// correction. Above the distance of a few states of latency at the sprint speed.
pub const SERVER_MOVED_M: f32 = 3.0;
/// The counter of the bot's first record.
const FIRST_COUNTER: u16 = 1;
/// The left stick pushed all the way forward.
const STICK_FORWARD: [f32; 2] = [0.0, 1.0];

/// A bot's own body, walked by its records.
#[derive(Debug, Clone)]
pub struct RecordWalker {
    device: InputDevice,
    body: TrackedBody,
    /// The counter of the next record.
    counter: u16,
    /// The view's yaw of the last record.
    yaw: f32,
    /// The last record: the client sends it again after the new one.
    last: Option<InputRecord>,
    /// The body after each record, in order.
    track: Vec<Position>,
}

impl RecordWalker {
    /// A bot standing at `at` that walks with `device`.
    pub fn new(at: Position, device: InputDevice) -> Self {
        Self {
            device,
            body: TrackedBody::new(at),
            counter: FIRST_COUNTER,
            yaw: 0.0,
            last: None,
            track: vec![at],
        }
    }

    /// Where its body is.
    pub const fn position(&self) -> Position {
        self.body.position()
    }

    /// Its body at the start and after each record, in order.
    pub fn track(&self) -> &[Position] {
        &self.track
    }

    /// Takes the server's place of the bot, `server`: a place farther than [`SERVER_MOVED_M`]
    /// from its body is a move of the server, and the body stands there.
    pub fn follow_server(&mut self, server: Position) {
        let here = self.body.position();
        if (server.x - here.x).hypot(server.y - here.y) > SERVER_MOVED_M {
            self.body.stand_at(server);
            self.track.push(server);
        }
    }

    /// One frame of `frame_s` toward `goal` (east and north, m) over `ground`, with the sea at
    /// `sea_level_m`: the body moves by the frame's keys or stick, and the input holds the new
    /// record and the last one, newest first. `None` when the camera falls outside what a
    /// record holds.
    pub fn frame(
        &mut self,
        goal: [f32; 2],
        frame_s: f32,
        ground: &(impl Ground + ?Sized),
        sea_level_m: f32,
    ) -> Option<ClientInputMsg> {
        let here = self.body.position();
        let moving = (goal[0] - here.x).hypot(goal[1] - here.y) > ARRIVED_M;
        if moving {
            let to = Position {
                x: goal[0],
                y: goal[1],
                z: here.z,
            };
            self.yaw = yaw_toward(here, to);
        }
        let moves = match (moving, self.device) {
            (false, _) => FrameMove::Keys(MoveKeys::default()),
            (true, InputDevice::Keys) => FrameMove::Keys(MoveKeys {
                forward: true,
                ..MoveKeys::default()
            }),
            (true, InputDevice::Pad) => FrameMove::Stick(STICK_FORWARD),
        };
        let view = [self.yaw, VIEW_PITCH];
        let mut record = InputRecord::frame(self.counter, frame_s, view, camera_at(here)?, moves);
        // The keys as the server reads them back from the record (the stick in its bytes).
        let keys = record.move_keys();
        let (intent, yaw) = Intent::of_keys(keys.forward, keys.back, keys.left, keys.right)
            .with_stick(record.left_stick())
            .toward_stick(self.yaw);
        self.body.record(
            TrackInput {
                character: CharacterInput {
                    intent,
                    yaw,
                    dt_s: frame_s,
                },
                view: None,
                boom_key: false,
                action_started: false,
            },
            ground,
            sea_level_m,
        );
        let body = self.body.position();
        self.track.push(body);
        let camera = CameraView::of_body(
            body,
            self.yaw,
            VIEW_PITCH,
            CAMERA_BOOM_M,
            self.body.velocity(),
        )
        .camera;
        record.bit_7 = Some(camera_at(camera)?);
        self.counter = self.counter.wrapping_add(1);
        let records = std::iter::once(record.clone())
            .chain(self.last.replace(record))
            .collect();
        Some(ClientInputMsg {
            records,
            ..ClientInputMsg::default()
        })
    }
}

/// `at` as a record's point.
fn camera_at(at: Position) -> Option<InputPosition> {
    InputPosition::from_metres(at.x, at.y, at.z)
}

#[cfg(test)]
mod tests {
    use nw_sim_core::test_support::FlatGround;

    use super::*;

    const GROUND_M: f32 = 50.0;
    const SEA_M: f32 = 0.0;
    const FRAME_S: f32 = 0.033;
    /// Frames of a walk: about 3 s.
    const FRAMES: usize = 90;
    const GOAL: [f32; 2] = [100.0, 40.0];
    /// The body under a record's camera is within this of the bot's body, m (one record point's
    /// height step and the float error).
    const CAMERA_CLOSE_M: f32 = 0.05;

    fn start() -> Position {
        Position {
            x: 100.0,
            y: 10.0,
            z: GROUND_M,
        }
    }

    fn walk(device: InputDevice) -> (RecordWalker, Vec<ClientInputMsg>) {
        let mut walker = RecordWalker::new(start(), device);
        let inputs = (0..FRAMES)
            .map(|_| {
                walker
                    .frame(GOAL, FRAME_S, &FlatGround(GROUND_M), SEA_M)
                    .expect("input")
            })
            .collect();
        (walker, inputs)
    }

    #[test]
    fn a_walking_bot_numbers_its_records_and_moves_toward_its_goal_with_keys_and_with_a_pad() {
        for device in [InputDevice::Keys, InputDevice::Pad] {
            let (walker, inputs) = walk(device);
            let moved = walker.position().y - start().y;
            assert!(moved > 1.0, "{device:?}: moved {moved}");
            assert!((walker.position().x - start().x).abs() < CAMERA_CLOSE_M);
            assert_eq!(walker.track().len(), FRAMES + 1);
            for (frame, input) in inputs.iter().enumerate() {
                let counter = FIRST_COUNTER + u16::try_from(frame).expect("frames");
                let newest = &input.records[0];
                assert_eq!(newest.u16_after_mask, counter, "{device:?}");
                assert_eq!(newest.bit_5, Some(FRAME_S));
                if let Some(older) = input.records.get(1) {
                    assert_eq!(older.u16_after_mask, counter - 1, "the last record again");
                }
                let moves = match device {
                    InputDevice::Keys => newest.move_keys().forward,
                    InputDevice::Pad => newest.left_stick().is_some(),
                };
                assert!(moves, "{device:?} frame {frame}");
            }
        }
    }

    #[test]
    fn each_records_camera_puts_the_bots_own_body_back_on_its_boom() {
        let mut walker = RecordWalker::new(start(), InputDevice::Keys);
        for _ in 0..FRAMES {
            let input = walker
                .frame(GOAL, FRAME_S, &FlatGround(GROUND_M), SEA_M)
                .expect("input");
            let record = &input.records[0];
            let camera = record.bit_7.expect("camera");
            let [yaw, pitch] = record.bit_6.expect("view");
            let seen = CameraView::new(
                Position {
                    x: camera.x,
                    y: camera.y,
                    z: camera.z(),
                },
                yaw,
                pitch,
            )
            .body(CAMERA_BOOM_M, walker.body.velocity());
            let body = walker.position();
            let off = (seen.x - body.x).hypot(seen.y - body.y);
            assert!(off < CAMERA_CLOSE_M, "camera body {seen:?}, own {body:?}");
            assert!(
                (seen.z - body.z).abs() < CAMERA_CLOSE_M,
                "{seen:?} {body:?}"
            );
        }
    }

    #[test]
    fn a_bot_at_its_goal_holds_no_key() {
        for device in [InputDevice::Keys, InputDevice::Pad] {
            let mut walker = RecordWalker::new(start(), device);
            let goal = [start().x, start().y];
            let input = walker
                .frame(goal, FRAME_S, &FlatGround(GROUND_M), SEA_M)
                .expect("input");
            let record = &input.records[0];
            assert_eq!(record.move_keys(), MoveKeys::default(), "{device:?}");
            assert_eq!(record.left_stick(), None, "{device:?}");
        }
    }

    #[test]
    fn a_far_server_place_moves_the_body_and_a_near_one_does_not() {
        let mut walker = RecordWalker::new(start(), InputDevice::Keys);
        let near = Position {
            y: start().y + SERVER_MOVED_M / 2.0,
            ..start()
        };
        walker.follow_server(near);
        assert_eq!(walker.position(), start());
        let far = Position {
            y: start().y + SERVER_MOVED_M * 2.0,
            ..start()
        };
        walker.follow_server(far);
        assert_eq!(walker.position(), far);
        assert_eq!(walker.track().last(), Some(&far));
    }
}
