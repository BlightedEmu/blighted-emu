// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client's camera and keys of each input record, for the server's own movement of the
//! player ([`nw_sim_core::TrackedBody`], movement study 2026-09-20).
//!
//! The client sends its newest record first and writes each record against the record before it
//! in the message, the newer one (`FUN_142a28f30` passes the previous record to the record writer
//! `FUN_142a26320`, which sets a mask bit only for a field that differs): a record without its
//! frame time (bit 5), view angles (bit 6) or camera point (bit 7) has the newer record's. The
//! newest record of every retail input after the spawn carries all three (41,669 records of the
//! newchar capture; the 828 without were sent before the spawn).

use nw_messages::{InputRecord, LookDir};
use nw_sim_core::{CameraView, CharacterInput, Intent, Position, TrackInput};

/// The frame time of a record that never said one, s: the client's 30 Hz input (Observed: 0.0333
/// s in every unthrottled retail record, 0.05 s while the server throttles the input to 20 Hz).
const DEFAULT_FRAME_S: f32 = 1.0 / 30.0;

/// The camera, view and frame time of one record, after the fields it left out.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RecordView {
    /// The frame time, s.
    pub frame_s: Option<f32>,
    /// The camera yaw and pitch.
    pub view: Option<LookDir>,
    /// The camera point.
    pub camera: Option<Position>,
}

/// The view of each record of `records` (newest first, as the client sends them), each record
/// taking the fields it leaves out from the newer one.
pub fn record_views(records: &[InputRecord]) -> Vec<RecordView> {
    let mut newer = RecordView::default();
    records
        .iter()
        .map(|record| {
            newer = RecordView {
                frame_s: record.bit_5.or(newer.frame_s),
                view: record
                    .bit_6
                    .map(|[yaw, pitch]| LookDir { yaw, pitch })
                    .or(newer.view),
                camera: record
                    .bit_7
                    .map(|camera| Position {
                        x: camera.x,
                        y: camera.y,
                        z: camera.z(),
                    })
                    .or(newer.camera),
            };
            newer
        })
        .collect()
}

/// The movement input of `record` with its view `seen`; `None` without view angles (a client
/// that sends only a point: the test client, the load-test bots).
pub fn track_input(record: &InputRecord, seen: RecordView) -> Option<TrackInput> {
    let look = seen.view?;
    let keys = record.move_keys();
    let intent = Intent {
        jump: keys.jump,
        walk_toggle: record.walk_toggle_pressed(),
        block: record.blocking(),
        dodge: record.dodging(),
        crouch_toggle: record.crouch_toggle_pressed(),
        prone_toggle: record.prone_toggle_pressed(),
        walk: record.walk_held(),
        ..Intent::of_keys(keys.forward, keys.back, keys.left, keys.right)
    }
    .with_stick(record.left_stick());
    let (intent, yaw) = intent.toward_stick(look.yaw);
    Some(TrackInput {
        character: CharacterInput {
            intent,
            yaw,
            dt_s: seen.frame_s.unwrap_or(DEFAULT_FRAME_S),
        },
        view: seen
            .camera
            .map(|camera| CameraView::new(camera, look.yaw, look.pitch)),
        boom_key: record.boom_key_pressed(),
        // A press or hold of the attack key acts too: the server starts the press start and the
        // charge from it, and the camera rises with the pose (live 2026-11-18: a held heavy
        // charge lifted the camera's body 1.8 m and the body stood up there for the others).
        // So does a held ability key: its ability's camera leaves the boom (live 2026-11-19,
        // log live-2026-11-19e, a spear ability held from counter 8469: the camera came in from
        // a 7.0 m boom to 1.13 m at the same height, its body stood 2.6 m above the floor, and
        // from the first movement key at 8472 the body took that height for minutes, as a floor
        // the level lacks; the slayer input came only at 8479).
        action_started: record.slayer_input().is_some() || record.ability_key().is_some() || {
            let attack = record.attack_key();
            attack.pressed || attack.held
        },
    })
}

#[cfg(test)]
mod tests {
    use nw_messages::{InputPosition, InputSlots};

    use super::*;

    const CLOSE: f32 = 1e-3;

    fn camera(x: f32) -> InputPosition {
        InputPosition::from_metres(x, 850.0, 92.0).expect("finite")
    }

    #[test]
    fn an_older_record_takes_what_it_leaves_out_from_the_newer_one() {
        // Newest first: the newest record carries everything, the older one only a new camera.
        let newest = InputRecord {
            u16_after_mask: 11,
            bit_5: Some(0.05),
            bit_6: Some([2.6, 0.2]),
            bit_7: Some(camera(478.0)),
            ..InputRecord::default()
        };
        let older = InputRecord {
            u16_after_mask: 10,
            bit_7: Some(camera(477.5)),
            ..InputRecord::default()
        };
        let views = record_views(&[newest, older]);
        assert_eq!(views.len(), 2);
        assert_eq!(views[1].frame_s, Some(0.05));
        assert_eq!(
            views[1].view,
            Some(LookDir {
                yaw: 2.6,
                pitch: 0.2
            })
        );
        assert!((views[1].camera.expect("camera").x - 477.5).abs() < CLOSE);
        assert!((views[0].camera.expect("camera").x - 478.0).abs() < CLOSE);
    }

    #[test]
    fn a_record_gives_the_keys_the_view_and_the_camera() {
        // Retail running forward (clean running capture): value 6 bit 32 and value 0 bits 14
        // and 43, with the standing values 7 and 8.
        let record = InputRecord {
            slots: InputSlots {
                count: 19,
                values: vec![
                    0x0800_0000_4000,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0x1_0000_0000,
                    0x0402_0000_0800_0000,
                    0x0200_0000_4000_0000,
                    0,
                ],
                bytes: [128; 4],
            },
            ..InputRecord::default()
        };
        let seen = RecordView {
            frame_s: None,
            view: Some(LookDir {
                yaw: 2.5,
                pitch: 0.1,
            }),
            camera: Some(Position {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }),
        };
        let input = track_input(&record, seen).expect("a view");
        assert_eq!(
            input.character.intent,
            Intent::of_keys(true, false, false, false)
        );
        assert!((input.character.dt_s - DEFAULT_FRAME_S).abs() < f32::EPSILON);
        assert!((input.character.yaw - 2.5).abs() < f32::EPSILON);
        assert!(input.view.is_some());
        assert!(!input.boom_key && !input.action_started);
        let no_camera = RecordView {
            camera: None,
            ..seen
        };
        assert_eq!(track_input(&record, no_camera).expect("a view").view, None);
        let point_only = RecordView { view: None, ..seen };
        assert_eq!(track_input(&record, point_only), None);
    }

    #[test]
    fn the_block_and_dodge_keys_go_to_the_character() {
        // Slot value 1: bit 62 while block is held, bit 3 while the dodge key is (input note).
        const BLOCK_AND_DODGE: u64 = 1 << 62 | 1 << 3;
        let record = InputRecord {
            slots: InputSlots {
                count: 19,
                values: vec![0, BLOCK_AND_DODGE],
                bytes: [128; 4],
            },
            ..InputRecord::default()
        };
        let seen = RecordView {
            frame_s: None,
            view: Some(LookDir {
                yaw: 0.0,
                pitch: 0.0,
            }),
            camera: None,
        };
        let intent = track_input(&record, seen).expect("a view").character.intent;
        assert!(intent.block && intent.dodge, "{intent:?}");
    }

    #[test]
    fn a_held_attack_key_acts_for_the_camera() {
        // Live 2026-11-18: a held heavy charge lifted the camera's body 1.8 m, and with no action
        // the body took that height. Slot value 2: bit 63 the attack key held, bit 40 its press.
        const HELD: u64 = 1 << 63;
        const PRESS: u64 = 1 << 40;
        let seen = RecordView {
            frame_s: None,
            view: Some(LookDir {
                yaw: 0.0,
                pitch: 0.0,
            }),
            camera: None,
        };
        let acts = |attack: u64| {
            let record = InputRecord {
                slots: InputSlots {
                    count: 19,
                    values: vec![0, 0, attack],
                    bytes: [128; 4],
                },
                ..InputRecord::default()
            };
            track_input(&record, seen).expect("a view").action_started
        };
        assert!(acts(HELD), "a held key");
        assert!(acts(PRESS), "a press");
        assert!(!acts(0), "no key");
    }

    #[test]
    fn a_held_ability_key_acts_for_the_camera() {
        // Live 2026-11-19 (live-2026-11-19e, counters 8469 to 8479): a held ability key brought
        // the camera in to 1.13 m with no action, and the body took the camera's height.
        let seen = RecordView {
            frame_s: None,
            view: Some(LookDir {
                yaw: 0.0,
                pitch: 0.0,
            }),
            camera: None,
        };
        // The slot values that hold every ability key (the highest, input 402, is in value 6).
        const SLOT_VALUES: usize = 7;
        for number in nw_messages::ABILITY_KEYS {
            let mut values = vec![0; SLOT_VALUES];
            values[(number / u64::BITS) as usize] = 1 << (number % u64::BITS);
            let record = InputRecord {
                slots: InputSlots {
                    count: 19,
                    values,
                    bytes: [128; 4],
                },
                ..InputRecord::default()
            };
            assert!(record.ability_key().is_some(), "key {number}");
            assert!(
                track_input(&record, seen).expect("a view").action_started,
                "key {number}"
            );
        }
    }

    #[test]
    fn the_posture_toggles_and_the_walk_key_go_to_the_character() {
        // Slot value 4 bit 57 the walk key; value 8 bit 31 a crouch press and value 3 bit 59 a
        // prone press of any device (`crouch_toggle_virtual`, `prone_toggle_virtual`).
        const WALK: u64 = 1 << 57;
        const CROUCH: u64 = 1 << 31;
        const PRONE: u64 = 1 << 59;
        let record = InputRecord {
            slots: InputSlots {
                count: 19,
                values: vec![0, 0, 0, PRONE, WALK, 0, 0, 0, CROUCH],
                bytes: [128; 4],
            },
            ..InputRecord::default()
        };
        let seen = RecordView {
            frame_s: None,
            view: Some(LookDir {
                yaw: 0.0,
                pitch: 0.0,
            }),
            camera: None,
        };
        let intent = track_input(&record, seen).expect("a view").character.intent;
        assert!(
            intent.crouch_toggle && intent.prone_toggle && intent.walk,
            "{intent:?}"
        );
    }

    #[test]
    fn the_left_stick_moves_the_character_without_a_key() {
        // A game pad sends no direction key bit: scaled byte 1 is the left stick forward.
        const STICK_FORWARD: [u8; 4] = [128, 255, 128, 128];
        let record = InputRecord {
            slots: InputSlots {
                count: 19,
                values: vec![0],
                bytes: STICK_FORWARD,
            },
            ..InputRecord::default()
        };
        let seen = RecordView {
            frame_s: None,
            view: Some(LookDir {
                yaw: 0.0,
                pitch: 0.0,
            }),
            camera: None,
        };
        let intent = track_input(&record, seen).expect("a view").character.intent;
        let [right, forward] = intent.stick.expect("a tilted stick");
        assert!(right.abs() < CLOSE && forward > 0.99, "{intent:?}");
        assert_eq!((intent.forward, intent.right), (1, 0));
    }

    #[test]
    fn a_stick_pulled_back_runs_forward_facing_the_camera_s_back() {
        // Live 2026-11-17: a controller player turned round on its own screen; the others saw
        // it walk backwards.
        const STICK_BACK: [u8; 4] = [128, 0, 128, 128];
        let record = InputRecord {
            slots: InputSlots {
                count: 19,
                values: vec![0],
                bytes: STICK_BACK,
            },
            ..InputRecord::default()
        };
        let seen = RecordView {
            frame_s: None,
            view: Some(LookDir {
                yaw: 0.0,
                pitch: 0.0,
            }),
            camera: None,
        };
        let character = track_input(&record, seen).expect("a view").character;
        assert_eq!((character.intent.forward, character.intent.right), (1, 0));
        assert!(
            (character.yaw.abs() - std::f32::consts::PI).abs() < CLOSE,
            "faces the camera's back: {}",
            character.yaw
        );
    }
}
