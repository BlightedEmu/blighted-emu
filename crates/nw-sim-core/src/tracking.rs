// SPDX-License-Identifier: AGPL-3.0-or-later
//! A player's body as the server keeps it: moved by its keys ([`crate::character`]) over the
//! level's floors, and held to the client's own view ([`crate::camera_check`]) on the ground
//! plane.
//!
//! The rule, per input record: the character moves; the camera check solves where the client's
//! camera puts the body; on the ground plane the body goes toward where the camera puts it, at
//! most [`FOLLOW_STEP_M`] in one record (a wall stopped the client, a dodge or an attack moved
//! it, the server's speed was not the client's). The client moves its own player, and every
//! difference the server keeps pulls the client back. The replay of the retail tutorial captures
//! against retail's own places (`content-tool` test `own_replay`, 2026-09-24) measured it: with a
//! pull past 0.15 m by half the distance and at most 0.15 m a record, the client would have been
//! corrected (past 0.5 m) in 40% of the open-world states, with this follow in 36%, and the open
//! world's errors were 0.31 and 2.51 m (median, 90th percentile) against 0.25 and 1.71 m. A full
//! snap in one record stepped the sent place by 0.5 to 0.8 m (live 2026-09-22, the tutorial
//! ship), which the client showed as judder. When that pulls the body back against its motion,
//! the character's speed drops to the camera body's own speed along it since the record before (a
//! client held at a wall stays there instead of being pulled back record after record; replay of
//! the newchar capture, 2026-09-20: 1,366 pull-backs instead of 1,486 in the tutorial and 1,610
//! instead of 1,945 in the open world, a median of 0.34 m instead of 0.29 m in the tutorial and
//! 0.16 m instead of 0.14 m in the open world).
//!
//! The height is the level's floor, not the camera's. The ground knows the level's floors: the
//! terrain and the structures of the level collision file (`crate::LevelGround`; the tutorial deck
//! at the retail own place, 89.38 m at the start). Out of a jump, the camera's height only picks
//! which floor under the body the client stands on: the highest one at most a step-up detection
//! height above the higher of the camera's body and the body
//! ([`nw_world::collision_index::FLOOR_SEARCH_ABOVE_M`]; in an action the camera's body went
//! 1.5 m under the tutorial deck, run 19, and a search from it missed the deck); the
//! deck and not the sea under it, a building's upper floor and not its cellar. A camera's height
//! errs by up to half a metre, the floors of one place lie metres apart. A body that falls lands
//! on that floor once it is down at it. Where the level has neither height data nor a structure
//! the body takes the camera's height. So does a body whose camera stands more than
//! [`CAMERA_FLOOR_SPREAD_M`] above the highest floor the level knows there while it sinks no
//! faster than a walk down the steepest walkable slope: it stands on a floor the level lacks (the
//! steps of the tutorial cave, run 17: the level collision has no surface between 23.13 m and
//! 33 m there, and the body stayed at the foot while the client climbed); in an action, a dodge
//! or a jump it keeps its own height there and finds no such floor, as the camera's height is
//! off in them ([`CAMERA_SETTLE_RECORDS`]). A resting body's still camera that stands it more than
//! [`BOOM_SHORT_GAP_M`] above the floor under it, where a longer boom stands it on a floor the
//! level knows, held a boom shorter than the client's: the body goes onto that floor with that
//! boom and does not take a floor the level lacks. Nor does a body whose camera holds a boom
//! shorter than the client's zoom (`BOOM_BELOW_ZOOM_M`): that camera met the level and its
//! boom glides back out. A body the server dropped
//! (a move into the air) falls by the physics until it lands.
//!
//! The camera does not carry the body through a wall ([`wall_between`]) from close by: the
//! client's capsule does not pass one, so a camera body just beyond it is the solve's error, a
//! boom the level shortened (run 15: by the beach stairs the camera's body stood 1.2 m from the
//! camera and resyncs of 0.7 to 1.3 m carried the body into the solid block under the stair
//! landing). The wall is the one at the higher body's feet, so a client that climbed onto a ledge
//! takes the body along. From farther than [`WALL_HOLD_M`], a camera's body that stands in the
//! open on the far side (the way back to the body meets the wall too) shows a client that went
//! around or over the wall where the level's collision differs from the client's, and the camera
//! carries the body again: in the tutorial replay a wall held the body behind the beach block for
//! the rest of the run, 127 m from retail's place (median); with this limit 0.40 m (1.5 m and 3 m
//! gave the same within 0.03 m). A camera's body inside a solid (the way back enters no wall)
//! never carries it, from any distance (run 15's stayed in the block for a second).
//!
//! A camera does not give the body's height itself: the boom is never sent, the camera check
//! measures it from the server's own motion of the body while the view turns, and an action
//! moves the body by its animation, which the solve reads as a change of the boom. Run 14
//! (2026-09-20), the first sailor fight on the tutorial deck: in a fast turn of the view the
//! solve's boom grew from 4.4 to 5.25 m without a boom key, the camera's body went 0.55 m under
//! the deck, and a body that took the camera's height went down through the deck, 89.39 to 82.1 m
//! in two seconds. The client's camera also shortens its boom where it meets the level
//! (`libs/camera/gamecamera.xml`: `CameraCollides`); on the retail tutorial deck the camera's body
//! stood 0.48 m below to 0.73 m above the retail own place while moving (5% to 95%). Replay of
//! the retail newchar tutorial with the level collision (level-collision note 2026-09-20): the
//! body's height erred by a median 0.022 m and 0.117 m at the 90th percentile, against 0.124 m and
//! 0.654 m with the camera's height. In a jump the body keeps its own height and the camera moves
//! it only on the ground plane (a jump that dropped the camera's pull-back went on 0.58 m beside
//! retail's place over the Fury Bay ledge gap, open-world replay records 25,304 to 25,321). A record whose camera does not follow the body changes
//! nothing.
//!
//! An attack whose action's animation drives the motion (`AnimationDrivenMotion`,
//! [`AttackMotion`]) moves the body by the animation's root, not by the keys: toward the keys of
//! the record it starts in, else the camera's forward (Observed, retail newchar tutorial: the
//! first attacks after a pause without a movement key went 1 to 24 degrees from the camera's
//! forward; with a key held the place kept 0.84 to 0.97 m/s from 0.5 to 1 s after the start,
//! against 3.5 to 5 m/s free), with the velocity it keeps, until a movement key ends it from its
//! `CanTransition` marker on, a dodge or a block ends it, or its motion is over. The sim learns
//! of an attack after the records of its message moved the body: those records are taken again
//! with the attack's motion ([`TrackedBody::start_attack`]). An ability's fragment with an
//! animation-driven motion moves the body so too, from the record after the message that
//! pressed its key ([`TrackedBody::start_motion`]).

use std::collections::VecDeque;
use std::sync::Arc;

use nw_content::player_attacks::AttackMotion;

use crate::camera_check::{CameraBody, CameraCheck, CameraView, MAX_BOOM_M, MOUNTED_CAMERA_LAG_S};
use crate::character::{
    Character, CharacterInput, MountPaces, Posture, SPRINT_SPEED_MPS, key_direction, swim_floor_at,
    wall_between, yaw_axes,
};
use crate::equip_load::EquipLoad;
use crate::movement::{Ground, Position};
use nw_world::collision_index::MAX_WALKABLE_SLOPE_DEG;

/// How far the server's body may be from the camera's body and stay, m: the own place's step on
/// the wire (4/255 m), below which the client cannot see the difference.
pub const FOLLOW_FROM_M: f32 = 4.0 / 255.0;
/// The longest step toward the camera's body in one record, m (Observed: retail's own places step
/// at most 0.36 m from one record to the next in the open-world walk replay, motion included).
pub const FOLLOW_STEP_M: f32 = 0.36;
/// How far the camera's body may be from the server's body and a wall between them still hold the
/// body, m (the patched client's full-correction distance; see the module notes).
pub const WALL_HOLD_M: f32 = 2.0;
/// Records after the client starts an action (an attack, a dodge, an ability) while its moves may
/// carry the body without the movement keys, so the camera check does not hold it still
/// (Observed: after the 20 actions the newchar tutorial started without a movement key, the own
/// place last moved 25 to 60 records later, median 52, by up to 8.5 m; each new action starts
/// the count again).
pub const ACTION_RECORDS: u8 = 52;
/// Records in a row whose game camera does not look at the body after which the body goes to
/// where the camera puts it at its last boom (our choice: about one second). The camera check
/// takes no camera that passes more than 1 m (`CAMERA_FIT_M`) beside the body,
/// so a body that got that far from its client (a server move the client did not take, a client
/// move the server did not see) was never found again: live 2026-09-26, Thorpe's first grab pull
/// left the body 7 m from the client and it drifted to 21 m for the rest of the fight, where
/// Thorpe fought it. Cutscene and video cameras never come here (the sim drops their views).
///
/// Only a record in which the client can move its body counts: one with a movement key, a jump
/// or a dodge, or one in the time of an action it started ([`ACTION_RECORDS`]). A camera that
/// leaves a body at rest shows something else, an NPC's talk or a town board, and the body stays
/// (Observed, retail: the talk camera of `06a_msq_06_rescuefarmers` from 101.4 s, no key held,
/// took the body 1.75 m away onto a crate, 1.18 m above retail's place, for 17 s; the board
/// camera of `Hivetownboardmissions` from 457.6 s took it 4.8 m away, 1.69 m into the air).
///
/// In an action with no movement key or stick, the first camera that does not look at the body
/// already shows where the client is: the action's own motion (an attack's lunge, a push) carried
/// the client, the window's older cameras moved by the keys alone no longer fit the newest, and
/// the body stood where the lunge began until the window emptied (Observed, retail Monarch's
/// Bluffs ledgers with no key or stick held: `12-4_crewmates` from 84.58 s, 2.7 m in 0.33 s, the
/// body up to 1.71 m from retail's place; `12-9_demonstorm2` from 82.33 s, 3.3 m in 0.2 s, up to
/// 3.04 m; with the first camera 0.45 m and 0.73 m. Replay of 11 open-world ledgers: swings
/// standing 0.18/0.76 m off, median/90th percentile, to 0.18/0.64 m; tutorial 0.43/1.60 to
/// 0.42/1.54 m; the other classes as before).
pub const RECOVER_AFTER_RECORDS: u16 = 30;
/// How far above the highest known floor the camera's body may stand and still stand on it, m
/// (Observed: on the retail tutorial deck the camera's body stood 0.48 m below to 0.73 m above the
/// retail own place while moving, 5% to 95%; live 2026-11-17, on flat open ground with no
/// structure, the camera's bodies of two players at rest stood 0.9 m and 1.07 m above it for
/// seconds, and with 1.0 m the body of the second one stood on a floor 1 m up in the air).
pub const CAMERA_FLOOR_SPREAD_M: f32 = 1.5;
/// How far above the floor under it the camera's body of a resting body under a still camera
/// ([`STILL_CAMERA_M`]) may stand before a longer boom that stands it on a floor the level knows
/// is the client's, m.
///
/// The boom is never sent, and a camera that met the level (`CameraCollides`) shortens it and
/// glides back out with no boom key, while the check holds the short one: the camera's body then
/// stands too near the camera and too high by the missing boom × sin(pitch). Observed, retail
/// towns session 2026-11-19 with the client's own trace: on stairs at 21:08:32 the client's boom
/// went from 2.4 to 7.5 m in 0.4 s, the check held 2.17 m, and the body stood on its camera's
/// height 1.3 to 1.5 m above the client (the floor the level lacks, [`CAMERA_FLOOR_SPREAD_M`]);
/// over the session 75% of the records off by more than 0.3 m in height held a boom more than
/// 1 m off the client's, mostly too short. The first longer boom whose body meets a known floor
/// came within 0.49 m of the client's boom (median), against 4.39 m for the held one. A moving
/// camera's height lags the body on slopes and stairs (the client's camera stood 0.9 to 2.0 m
/// above its body, not 1.685 m), and the search then lengthened right booms in the old
/// open-world ledgers, so only a resting body's camera counts. Our choice from the replays
/// against retail's own places (records more than 0.3 m off in height, towns / 11 open-world
/// ledgers / tutorial): before 7.06 % / 2.69 % / 15.17 %; at rest with no pitch limits and a
/// moving camera, 1.5 m gave 7.03 % / 2.68 % / 15.17 %, 1.0 m 6.96 % / 2.74 % / 15.17 % and
/// 0.5 m 4.57 % / 2.57 % / 15.11 %; 0.5 m with the pitch limits and a still camera 5.18 % /
/// 2.49 % / 15.16 %, and the place error over 0.5 m 30.5 / 30.7 / 28.0 % before, 26.5 / 30.1 /
/// 26.9 % after.
pub const BOOM_SHORT_GAP_M: f32 = 0.5;
/// How much shorter than the client's zoom ([`CameraCheck::zoom_m`]) the boom may be and its
/// camera's height still stand the body on a floor the level lacks, m. A camera that met the
/// level shortened its boom and glides back out to the zoom with no boom key, and the check
/// holds the short boom: its camera's body stands too near the camera and too high, over higher
/// steps behind the client (Observed, retail towns session 2026-11-19 against the client's own
/// trace: on stairs at 21:08:51.7 the client's boom fell to 0.68 m and grew back to 7.4 m in
/// 2 s while it walked down, the check held 0.04 to 0.6 m, and the body stood on its camera's
/// height up to 4.4 m above the client; of the session's 189 records that stood on a floor the
/// level lacks, the 58 with a boom under 2.2 m all stood more than 0.3 m off). Our choice from
/// the replays (records more than 0.3 m off in height, towns against the client's trace /
/// towns / 11 open-world ledgers against retail's places): 0.5 m raised the tutorial's records
/// more than 1 m off from 2.01 % to 2.04 %; 1.0 m raised the open world's median place error
/// from 0.261 to 0.262 m (both with 9.0 m among the zoom lengths); 0.75 m gave
/// 4.74 / 4.88 / 2.27 % against 5.06 / 5.18 / 2.49 % before and kept or improved every other
/// summary value.
const BOOM_BELOW_ZOOM_M: f32 = 0.75;
/// The flattest look down whose camera's height may show a short boom, radians: below it a
/// metre of boom moves the camera's body less than 0.2 m in height, and the line along the view
/// meets the rising floor ahead, not the floor under the client (Observed, retail newchar
/// tutorial at 232.7 s, pitch 0.047 rad: a boom 1.7 m longer than retail's took the body 1.2 m
/// off; with 0.2 rad the tutorial replay lost none of its improved records and kept no worse
/// one).
const FLOOR_BOOM_MIN_PITCH_RAD: f32 = 0.2;
/// How far the camera may move in one record and still be still, m, on each axis (our choice:
/// the camera's own spread about the boom model is 0.05 m). A resting body's camera that moves
/// (a zoom, a camera that met the level, its rise after a landing) does not show its boom by its
/// height (Observed, retail `13a_sq_18_killboar` at 93.6 s: at rest the camera rose 0.6 m in
/// 0.1 s and the check's boom went 4.5, 3.2, 3.1 m against retail's 3.75 m, and the longer boom
/// the floor met was 6.1 m; 0.01 m left 41 records of that ledger worse by more than 0.5 m
/// against 280 with no limit, and 0.05 m 40).
const STILL_CAMERA_M: f32 = 0.01;
/// The steepest look down whose camera's height may show a short boom, radians. Observed, retail
/// `12-9_demonstorm2` from 174.7 s (pitch 1.12 rad, at rest): the client stood on a floor the
/// level lacks 2.1 to 2.6 m above the known one, and a boom 2 m longer met that floor 1 m ahead;
/// the camera's body at steep pitches stands over the floor below a ledge for a small boom
/// error. Our choice: 0.8 rad kept that ledger as before; 0.6 rad dropped 6 % of the towns
/// session's improved records.
const FLOOR_BOOM_MAX_PITCH_RAD: f32 = 0.8;
/// The boom step of the search for that boom, m (our choice: 0.07 m of height at a pitch of
/// 0.35 rad, and the meeting point is interpolated between the steps).
const FLOOR_BOOM_STEP_M: f32 = 0.2;
/// Records after a dodge or a jump in which the camera's height still shows that move, not a
/// floor (live 2026-11-17: in a controller player's dodge the camera's body rose
/// 1.9 m over 8 records on flat ground; after a standing jump it came down to its resting height
/// 6 records after the landing; our choice: 10 records, a third of a second).
pub const CAMERA_SETTLE_RECORDS: u8 = 10;
/// The share of the way to the camera's body a game pad's move takes in one record, past its
/// stick's own step (our choice: a quarter). A pad's camera body is noisy while it moves (live
/// 2026-11-17: a controller player's body went back and forth in 7.3 % of its moving records
/// against 1.4 % with keys, and the camera put it 3.9 m above flat ground); the share smooths
/// that noise, and a steady speed error of 1 m/s still leaves the body only 0.13 m behind.
/// The share starts from where the stick took the body, not from where the record began: from
/// there the body lagged its client by three records of its own speed, and the camera check,
/// which weighs the server's body, shortened its held boom toward it (replay of the retail
/// tutorial with the moves as a pad's stick: the boom fell from 4.0 to 2.75 m in 10 s of moves,
/// and a still block stood 0.98 m behind retail's place, median, against 0.01 m with keys; from
/// the stick's step, 0.01 m, and the pad's walk erred by 0.27 m instead of 0.40 m in the tutorial
/// and 0.26 m instead of 0.52 m in 11 open-world ledgers, as with keys).
pub const PAD_FOLLOW_SHARE: f32 = 0.25;

/// The records a body remembers, to take them again with the motion of an attack that started
/// in them (our choice: an input message holds three records, a resent one up to six).
pub const ATTACK_REWIND_RECORDS: usize = 8;
/// Milliseconds per second.
const MS_PER_SECOND: f32 = 1_000.0;

/// The fastest a walking body comes down, m/s: a sprint down the steepest walkable slope. A
/// camera that sinks faster shows a fall.
fn walking_descent_mps() -> f32 {
    SPRINT_SPEED_MPS * MAX_WALKABLE_SLOPE_DEG.to_radians().sin()
}

/// One input record for a tracked body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackInput {
    /// The keys, the yaw and the frame time.
    pub character: CharacterInput,
    /// The record's camera, when the client's camera carries the body.
    pub view: Option<CameraView>,
    /// The record pressed a key that changes the camera's boom.
    pub boom_key: bool,
    /// The client starts an action in this record, or presses or holds the attack key (an
    /// attack, a charge or an ability: combat, which ends the road boost).
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
    /// The body stands on a floor: not in a jump or a fall.
    pub on_floor: bool,
    /// Where the camera check put the body in this record, when the camera looked at it.
    pub camera_body: Option<Position>,
}

/// An attack whose animation moves the body.
#[derive(Debug, Clone, PartialEq)]
struct AttackPlay {
    motion: Arc<AttackMotion>,
    /// Its forward on the ground plane, once it started (a unit vector).
    forward: Option<[f32; 2]>,
    /// Ms since it started (below 0: it starts that much later).
    at_ms: f32,
    /// From this long after its start a movement key ends it, ms.
    keys_from_ms: f32,
    /// The velocity it keeps, along its right and its forward, m/s.
    kept_mps: [f32; 2],
    /// A hit reaction: it moves the body along its facing, and neither a key nor a dodge or a
    /// block ends it before `keys_from_ms`.
    held: bool,
}

impl AttackPlay {
    /// An action of motion `motion` that starts `at_ms` from now (below 0: later), whose
    /// movement keys may end it from `keys_from_ms` after its start; `held` for a hit reaction.
    const fn new(motion: Arc<AttackMotion>, keys_from_ms: f32, at_ms: f32, held: bool) -> Self {
        Self {
            motion,
            forward: None,
            at_ms,
            keys_from_ms,
            kept_mps: [0.0; 2],
            held,
        }
    }
}

/// One record the body took: where it stood before it, the record's input and its velocity then.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TakenRecord {
    before: Position,
    input: CharacterInput,
    velocity: [f32; 2],
}

/// A player's body.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackedBody {
    character: Character,
    camera: CameraCheck,
    /// The attack whose animation moves it.
    attack: Option<AttackPlay>,
    /// Its last records, oldest first ([`ATTACK_REWIND_RECORDS`]).
    taken: VecDeque<TakenRecord>,
    /// Where the camera put the body in the record before, when it did.
    last_seen: Option<Position>,
    /// Records left while an action the client started may move the body.
    acting_records: u8,
    /// Records left while the camera still shows a dodge or a jump ([`CAMERA_SETTLE_RECORDS`]).
    camera_settle_records: u8,
    /// The server dropped it and it has not landed yet.
    dropping: bool,
    /// The last record stood it on a floor the level lacks, at its camera's height: the physics
    /// finds no floor under it, but it does not fall.
    on_lacking_floor: bool,
    /// Records in a row whose camera did not look at it.
    unfit_records: u16,
    /// The camera of the record before, when it had one.
    last_camera: Option<Position>,
}

impl TrackedBody {
    /// A body standing at `at`.
    pub fn new(at: Position) -> Self {
        Self {
            character: Character::new(at),
            camera: CameraCheck::default(),
            attack: None,
            taken: VecDeque::with_capacity(ATTACK_REWIND_RECORDS),
            last_seen: None,
            acting_records: 0,
            camera_settle_records: 0,
            dropping: false,
            on_lacking_floor: false,
            unfit_records: 0,
            last_camera: None,
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

    /// The boom the camera check holds, once a camera fitted ([`CameraCheck::boom_m`]).
    pub const fn camera_boom_m(&self) -> Option<f32> {
        self.camera.boom_m()
    }

    /// Whether it stands still: no speed on the ground plane, not in a jump, and no action of
    /// the client moving it.
    pub fn is_resting(&self) -> bool {
        self.character.is_resting() && self.acting_records == 0 && self.attack.is_none()
    }

    /// The server started an attack of motion `motion` `since_ms` ago (below 0: it starts that
    /// much later), whose movement keys may end it from `keys_from_ms` after its start: its
    /// animation moves the body from then on. The records the body took since its start are
    /// taken again with its motion, over `ground` with the sea at `sea_level_m`.
    pub fn start_attack(
        &mut self,
        motion: Arc<AttackMotion>,
        keys_from_ms: f32,
        since_ms: f32,
        ground: &(impl Ground + ?Sized),
        sea_level_m: f32,
    ) {
        let mut rewind = 0;
        let mut rewound_ms = 0.0;
        for record in self.taken.iter().rev() {
            if rewound_ms >= since_ms {
                break;
            }
            rewound_ms += record.input.dt_s * MS_PER_SECOND;
            rewind += 1;
        }
        let again: Vec<TakenRecord> = self.taken.drain(self.taken.len() - rewind..).collect();
        self.attack = Some(AttackPlay::new(
            motion,
            keys_from_ms,
            since_ms - rewound_ms,
            false,
        ));
        let Some(first) = again.first() else {
            return;
        };
        self.character.land_at(first.before);
        for record in again {
            let driven = self.attack_motion(record.input, record.velocity);
            self.remember(record.input);
            match driven {
                Some(driven) => {
                    self.character
                        .step_driven(record.input, driven, ground, sea_level_m);
                }
                None => {
                    self.character.step(record.input, ground, sea_level_m);
                }
            }
        }
    }

    /// A hit reaction of motion `motion` starts now (`playeractions_reaction.actionlist`: the
    /// `Reaction` actions' `CharacterRelativeMovement` speed 0 and their `ReactionMotion`): its
    /// animation moves the body along its facing, and its keys, its dodge and its block move it
    /// no more until `keys_from_ms` after the start, when a movement key ends it.
    pub fn start_reaction(&mut self, motion: Arc<AttackMotion>, keys_from_ms: f32) {
        self.attack = Some(AttackPlay::new(motion, keys_from_ms, 0.0, true));
    }

    /// An ability's fragment of motion `motion` starts with the next record (the sim's ability
    /// run starts it in the tick after the input message that pressed its key, whose records
    /// moved the body already; Inferred: the client starts it with the press, one message
    /// earlier): its animation moves the body as an attack's does, from the keys of that record
    /// or else the camera's forward, until a movement key ends it from `keys_from_ms` after its
    /// start on, a dodge or a block ends it, or its motion is over. Observed, retail
    /// `12a_msq_15_defeatnavigator` from 182.13 s (`Ability_Greatsword_DashAttack`, ability key
    /// 1, no movement key): the own place went 6.2 m along the camera's forward (3 to 8 degrees
    /// off) within 1.0 s of the press and stopped, where the fragment's root motion goes 6.1 m
    /// in 1.1 s; the keys alone left the body standing.
    pub fn start_motion(&mut self, motion: Arc<AttackMotion>, keys_from_ms: f32) {
        self.attack = Some(AttackPlay::new(motion, keys_from_ms, 0.0, false));
    }

    /// A dodge breaks the hit reaction out (`playeractions_dodge.actionlist`,
    /// `Dodge_Stunlock`): the reaction's motion stops at once and the keys and the dodge move the
    /// body again. An attack's motion goes on.
    pub fn break_reaction(&mut self) {
        if self.attack.as_ref().is_some_and(|play| play.held) {
            self.attack = None;
        }
    }

    /// Keeps `input` among the records it took, with where it stands and its velocity now.
    fn remember(&mut self, input: CharacterInput) {
        if self.taken.len() == ATTACK_REWIND_RECORDS {
            self.taken.pop_front();
        }
        self.taken.push_back(TakenRecord {
            before: self.character.position(),
            input,
            velocity: self.character.velocity(),
        });
    }

    /// The motion of the attack under way in the record of `input` (m on the ground plane),
    /// when its animation moves the body; a body of velocity `velocity` that starts it keeps
    /// what its motion keeps of that. `None` when no attack moves it (a dodge, a block, a
    /// movement key past its marker or the motion's end ends it).
    fn attack_motion(&mut self, input: CharacterInput, velocity: [f32; 2]) -> Option<[f32; 2]> {
        let play = self.attack.as_mut()?;
        let CharacterInput { intent, yaw, dt_s } = input;
        let dt_ms = dt_s * MS_PER_SECOND;
        let free = play.at_ms >= play.keys_from_ms;
        let over = play.at_ms >= play.motion.length_ms() || (free && intent.moves());
        if ((intent.dodge || intent.block) && (free || !play.held)) || over {
            self.attack = None;
            return None;
        }
        let to_ms = play.at_ms + dt_ms;
        if to_ms <= 0.0 {
            play.at_ms = to_ms;
            return None;
        }
        let held = play.held;
        let forward = *play.forward.get_or_insert_with(|| {
            let facing = yaw_axes(yaw).0;
            let forward = if held {
                facing
            } else {
                key_direction(intent, yaw).unwrap_or(facing)
            };
            let right = [forward[1], -forward[0]];
            let along = |axis: [f32; 2]| velocity[0] * axis[0] + velocity[1] * axis[1];
            let kept = [along(right), along(forward)];
            let speed = kept[0].hypot(kept[1]);
            let limit = play.motion.inherit_max_mps.unwrap_or(f32::INFINITY);
            let share = if play.motion.inherit_velocity && speed > f32::EPSILON {
                (limit / speed).min(1.0)
            } else {
                0.0
            };
            play.kept_mps = [kept[0] * share, kept[1] * share];
            forward
        });
        let right = [forward[1], -forward[0]];
        let from_ms = play.at_ms.max(0.0);
        let (from, to) = (play.motion.offset_at(from_ms), play.motion.offset_at(to_ms));
        let run_s = (to_ms - from_ms) / MS_PER_SECOND;
        let mut local = [0.0; 2];
        for axis in 0..2 {
            local[axis] = to[axis] - from[axis] + play.kept_mps[axis] * run_s;
            let fall = play.motion.decel_mps2[axis] * run_s;
            play.kept_mps[axis] = if play.kept_mps[axis] > 0.0 {
                (play.kept_mps[axis] - fall).max(0.0)
            } else {
                (play.kept_mps[axis] + fall).min(0.0)
            };
        }
        play.at_ms = to_ms;
        Some([
            right[0] * local[0] + forward[0] * local[1],
            right[1] * local[0] + forward[1] * local[1],
        ])
    }

    /// Its equip load category changes to `load` ([`Character::set_equip_load`]).
    pub const fn set_equip_load(&mut self, load: EquipLoad) {
        self.character.set_equip_load(load);
    }

    /// Its equip load category.
    pub const fn equip_load(&self) -> EquipLoad {
        self.character.equip_load()
    }

    /// A held `NoSprint` effect keeps it from sprinting ([`Character::set_no_sprint`]).
    pub const fn set_no_sprint(&mut self, no_sprint: bool) {
        self.character.set_no_sprint(no_sprint);
    }

    /// Whether the road boost speeds up its sprint.
    pub fn has_road_boost(&self) -> bool {
        self.character.has_road_boost()
    }

    /// A hit on it: combat, which ends the road boost while its action is on
    /// ([`Character::enter_combat`]).
    pub const fn enter_combat(&mut self) {
        self.character.enter_combat();
    }

    /// It gets on a mount with `paces` ([`Character::mount`]).
    pub const fn mount(&mut self, paces: MountPaces) {
        self.character.mount(paces);
    }

    /// It gets off its mount ([`Character::dismount`]).
    pub const fn dismount(&mut self) {
        self.character.dismount();
    }

    /// Whether it rides a mount.
    pub const fn is_mounted(&self) -> bool {
        self.character.is_mounted()
    }

    /// Its mount dashes, or stops dashing ([`Character::set_mount_dash`]).
    pub const fn set_mount_dash(&mut self, dashing: bool) {
        self.character.set_mount_dash(dashing);
    }

    /// The walk key gives its mount's slow walk, or its walk.
    pub const fn set_mount_slow_walk(&mut self, slow_walk: bool) {
        self.character.set_mount_slow_walk(slow_walk);
    }

    /// Whether it stands on a floor: not in a jump or a fall (the landing of a fall, for its
    /// damage).
    pub const fn is_grounded(&self) -> bool {
        self.character.is_grounded()
    }

    /// Whether it is off its floor: lifted in a jump or falling ([`Character::is_lifted`]).
    pub const fn is_lifted(&self) -> bool {
        self.character.is_lifted()
    }

    /// Whether it swims.
    pub const fn is_swimming(&self) -> bool {
        self.character.is_swimming()
    }

    /// Whether it is in a land dodge ([`Character::is_dodging`]): a dodge press in it starts
    /// none.
    pub const fn is_dodging(&self) -> bool {
        self.character.is_dodging()
    }

    /// How it holds itself: standing, crouching or prone.
    pub const fn posture(&self) -> Posture {
        self.character.posture()
    }

    /// Whether it falls and is not in a jump or a swim.
    pub const fn is_falling(&self) -> bool {
        !self.character.is_grounded()
            && !self.character.is_jumping()
            && !self.character.is_swimming()
    }

    /// The camera boom it holds, once a camera fitted.
    pub const fn boom_m(&self) -> Option<f32> {
        self.camera.boom_m()
    }

    /// The server puts it at `at` (a spawn, a teleport, a push): it stands there, and the camera
    /// that follows it starts again.
    pub fn stand_at(&mut self, at: Position) {
        self.character.stand_at(at);
        self.attack = None;
        self.taken.clear();
        self.camera.forget();
        self.last_seen = None;
        self.dropping = false;
        self.on_lacking_floor = false;
        self.last_camera = None;
    }

    /// The server puts it at `at` in the air (a teleport, a respawn): it falls from there to the
    /// floor it finds, and the camera that follows it starts again.
    pub fn drop_at(&mut self, at: Position) {
        self.character.drop_at(at);
        self.attack = None;
        self.taken.clear();
        self.camera.forget();
        self.last_seen = None;
        self.dropping = true;
        self.on_lacking_floor = false;
        self.last_camera = None;
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
            self.character.enter_combat();
        }
        let before = self.character.position();
        let driven = self.attack_motion(input.character, self.character.velocity());
        self.remember(input.character);
        let moved = match driven {
            Some(driven) => {
                self.character
                    .step_driven(input.character, driven, ground, sea_level_m)
            }
            None => self.character.step(input.character, ground, sea_level_m),
        };
        let mut position = self.character.position();
        let resting = self.is_resting();
        let acting = self.acting_records > 0;
        self.acting_records = self.acting_records.saturating_sub(1);
        // The camera's height shows a dodge or a jump and its landing: in them it picks no floor
        // the level lacks, as in an action.
        let dodging_or_jumping = self.character.is_dodging() || self.character.is_jumping();
        if dodging_or_jumping {
            self.camera_settle_records = CAMERA_SETTLE_RECORDS;
        }
        let camera_moved = acting || self.camera_settle_records > 0;
        if !dodging_or_jumping {
            self.camera_settle_records = self.camera_settle_records.saturating_sub(1);
        }
        // A rider's camera trails its mount farther than a walker's.
        let view = input.view.map(|view| {
            if self.character.is_mounted() {
                view.trailing(MOUNTED_CAMERA_LAG_S)
            } else {
                view
            }
        });
        let last_camera = std::mem::replace(&mut self.last_camera, view.map(|view| view.camera));
        let camera_still = view.zip(last_camera).is_some_and(|(view, last)| {
            let moved = [
                view.camera.x - last.x,
                view.camera.y - last.y,
                view.camera.z - last.z,
            ];
            moved.iter().all(|axis| axis.abs() <= STILL_CAMERA_M)
        });
        // A camera on the floor shows no body along its view ([`camera_met_floor`]): it gives
        // the body nothing, as a record with no camera.
        let met_floor = view.is_some_and(|view| camera_met_floor(view, ground));
        let view = view.filter(|_| !met_floor);
        let seen = view.and_then(|view| {
            self.camera.observe(
                view,
                position,
                moved,
                self.character.velocity(),
                resting,
                input.boom_key,
            )
        });
        // A camera that has not looked at the body for [`RECOVER_AFTER_RECORDS`] shows where
        // the client is: the body goes there. In an action with no movement key or stick, the
        // first one does (see [`RECOVER_AFTER_RECORDS`]).
        let intent = input.character.intent;
        let moves_itself = acting || intent.moves() || intent.jump || intent.dodge;
        let carried_by_action = acting && !intent.moves();
        let input_view = input.view.filter(|_| !met_floor);
        let recovering = seen.is_none() && input_view.is_some() && moves_itself && {
            self.unfit_records = self.unfit_records.saturating_add(1);
            self.unfit_records >= RECOVER_AFTER_RECORDS || carried_by_action
        };
        let seen = if recovering {
            input_view.and_then(|view| self.camera.body_at_last_boom(view))
        } else {
            seen
        };
        if seen.is_some() {
            self.unfit_records = 0;
        }
        let last_seen = std::mem::replace(&mut self.last_seen, seen.map(|seen| seen.at));
        let Some(seen) = seen else {
            return Tracked {
                position,
                resynced: false,
                consistent: false,
                on_floor: self.character.is_grounded(),
                camera_body: None,
            };
        };
        // A resting body's camera body high above the floor under it shows a boom shorter than
        // the client's when a longer one puts it on a floor the level knows
        // ([`BOOM_SHORT_GAP_M`]): the body goes there, and the check holds that boom.
        let floor_boom = view
            .filter(|_| {
                resting
                    && camera_still
                    && !self.dropping
                    && !self.is_swimming()
                    && !self.character.is_mounted()
                    && !self.character.is_jumping()
                    && !self.is_falling()
                    && !camera_moved
            })
            .and_then(|view| {
                boom_to_known_floor(view, seen, self.character.velocity(), ground, sea_level_m)
            });
        if let Some(found) = floor_boom {
            self.camera.hold_boom(found.boom_m);
            self.last_seen = Some(found.at);
        }
        let seen = floor_boom.unwrap_or(seen);
        // The wall is the one at the higher of the two bodies' feet: a client that climbed onto
        // a ledge (a mantle, a vault) shows its body on top, and the line passes over the ledge.
        let over = Position {
            z: position.z.max(seen.at.z),
            ..position
        };
        let distance = position.horizontal_distance(seen.at);
        // A fall keeps the keys' place: the server's physics brings it down. A swim and a jump
        // follow the camera on the ground plane (the tutorial replay: a swim that kept the keys
        // erred by 3.21 m at the 90th percentile in the sea, one that followed by 2.27 m).
        let falling = self.is_falling();
        // A body on a floor the level lacks has no floor under it in the physics, but its
        // client walks: the camera still carries it.
        let walled = wall_between(ground, over, seen.at).is_some() && {
            // From far, a camera's body on the open far side of the wall carries the body; one
            // inside a solid (the way back to the body enters no wall) does not.
            let back = Position {
                z: over.z,
                ..seen.at
            };
            distance <= WALL_HOLD_M || wall_between(ground, back, position).is_none()
        };
        let resynced = recovering
            || ((!falling || self.on_lacking_floor) && distance > FOLLOW_FROM_M && !walled);
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
            // Toward the camera's body, the record's whole step at most a retail step from where
            // the body was: a camera that jumps (a new boom, the stop of its trail) does not throw
            // the body the others see, and the step stays inside the sim's speed limit (10.8 m/s
            // against 13 m/s).
            let (dx, dy) = (seen.at.x - before.x, seen.at.y - before.y);
            // A rider's step is its mount's: the walking step held a horse at 0.36 m a record,
            // 7.2 m/s at 20 records a second, below its 9.25 m/s road run (replay of retail
            // Hermit's Rally: the ridden body fell 3 m behind retail's place, median).
            let step_m = self
                .character
                .mount_top_speed_mps()
                .map_or(FOLLOW_STEP_M, |top_mps| {
                    FOLLOW_STEP_M.max(top_mps * input.character.dt_s)
                });
            // A recovery goes the whole way at once, and so does a resting body to the floor its
            // camera's longer boom meets: in steps, the resting check would take the boom from
            // the body on the way and lose it short of the client. A pad's move keeps its stick's
            // step and goes a share of the rest of the way, then the record's whole step is held
            // to the limit.
            let whole_way = recovering || floor_boom.is_some();
            let toward = if whole_way || intent.stick.is_none() {
                [dx, dy]
            } else {
                [
                    position.x - before.x + (seen.at.x - position.x) * PAD_FOLLOW_SHARE,
                    position.y - before.y + (seen.at.y - position.y) * PAD_FOLLOW_SHARE,
                ]
            };
            let share = if whole_way {
                1.0
            } else {
                (step_m / toward[0].hypot(toward[1])).min(1.0)
            };
            position.x = before.x + toward[0] * share;
            position.y = before.y + toward[1] * share;
        }
        let seen_at = Position {
            z: seen.at.z,
            ..position
        };
        let walking_height = last_seen.is_none_or(|last| {
            (last.z - seen.at.z) / input.character.dt_s <= walking_descent_mps()
        });
        if self.character.is_jumping() {
            if self.on_lacking_floor
                && walking_height
                && self.character.is_past_jump_top()
                && position.z <= seen.at.z
            {
                // A jump from a floor the level lacks comes down on that floor: the physics
                // knows no floor there and would bring the body down to the level's floor under
                // it, a fall the client never made.
                self.character.land_at(seen_at);
            } else {
                // A jump keeps its own height; the camera only moves it on the ground plane.
                self.character.place(position);
            }
        } else {
            // An action moves the camera's body off the body's height (run 14: 0.55 m under
            // the deck; run 19: 1.5 m, past the step-up search, and the body went down through
            // the deck to the sea). The floor is searched from the higher of the two bodies'
            // feet, and a body on a floor the level lacks keeps its own height in an action.
            let was_on_lacking_floor = std::mem::take(&mut self.on_lacking_floor);
            let held = was_on_lacking_floor && camera_moved;
            let height = if held { position.z } else { seen.at.z };
            // A rider's camera sits on the saddle, about 2 m above a walker's: its height picks
            // no floor and gives the body none (replay of retail Hermit's Rally: with it the
            // ridden body stood 2.0 m above retail's place, median).
            let mounted = self.character.is_mounted();
            let search_at = Position {
                z: if mounted {
                    position.z
                } else {
                    position.z.max(seen.at.z)
                },
                ..position
            };
            // A camera whose boom is shorter than the client's zoom met the level: its height
            // shows no floor ([`BOOM_BELOW_ZOOM_M`]).
            let boom_of_zoom = self
                .camera
                .zoom_m()
                .is_none_or(|zoom| seen.boom_m >= zoom - BOOM_BELOW_ZOOM_M);
            match floor_under(ground, search_at, sea_level_m) {
                // A floor the level lacks. An action does not find one: its camera's body rises
                // with the animation (live 2026-11-17: a held heavy attack put the camera's body
                // 1.0 and 1.3 m above the flat floor, the body went up there and hung in the air
                // for the other players for up to 7 s).
                // A pad's move does not find one either: its camera's height is off while it moves
                // ([`PAD_FOLLOW_SHARE`]).
                // A body that is not on one yet climbs onto one only while its keys move it (the
                // cave steps were a walk); a still camera's height says nothing (live 2026-11-18:
                // a controller player stopped with its camera 2.1 m and 2.7 m above flat ground,
                // and the body went up there).
                Some(floor)
                    if !self.dropping
                        && floor_boom.is_none()
                        && !self.is_swimming()
                        && !mounted
                        && intent.stick.is_none()
                        && (was_on_lacking_floor || intent.forward != 0 || intent.right != 0)
                        && (held || (walking_height && !camera_moved))
                        && height - floor > CAMERA_FLOOR_SPREAD_M
                        && boom_of_zoom =>
                {
                    self.character.land_at(Position {
                        z: height,
                        ..position
                    });
                    self.on_lacking_floor = true;
                }
                // Its client left a floor the level lacks and comes down faster than a walk:
                // the body falls from there by the physics (its landing is the fall damage's
                // drop), and does not go to the level's floor in one record.
                Some(floor)
                    if was_on_lacking_floor
                        && !falling
                        && position.z - floor > CAMERA_FLOOR_SPREAD_M =>
                {
                    self.character.fall_from(position);
                }
                Some(floor) if !falling || position.z <= floor => {
                    position.z = floor;
                    self.character.land_at(position);
                }
                Some(_) => self.character.place(position),
                // No height data and no structure here: only the camera gives a height.
                None => self.character.land_at(seen_at),
            }
        }
        self.dropping &= !self.character.is_grounded();
        Tracked {
            position: self.character.position(),
            resynced,
            consistent: true,
            on_floor: self.character.is_grounded(),
            camera_body: Some(seen.at),
        }
    }
}

/// The camera's body `seen` of `view` again, at the shortest longer boom that stands it on a
/// floor of `ground` (the sea at `sea_level_m`), for a body moving at `velocity`: `None` when
/// `seen` stands at most [`BOOM_SHORT_GAP_M`] above the floor under it, or no boom up to
/// [`MAX_BOOM_M`] meets a floor.
fn boom_to_known_floor(
    view: CameraView,
    seen: CameraBody,
    velocity: [f32; 2],
    ground: &(impl Ground + ?Sized),
    sea_level_m: f32,
) -> Option<CameraBody> {
    let gap_m = |at: Position| floor_under(ground, at, sea_level_m).map(|floor| at.z - floor);
    if !(FLOOR_BOOM_MIN_PITCH_RAD..=FLOOR_BOOM_MAX_PITCH_RAD).contains(&view.pitch)
        || gap_m(seen.at).is_none_or(|gap| gap <= BOOM_SHORT_GAP_M)
    {
        return None;
    }
    let mut last: Option<(f32, f32)> = None;
    let mut boom_m = seen.boom_m;
    while boom_m < MAX_BOOM_M {
        boom_m = (boom_m + FLOOR_BOOM_STEP_M).min(MAX_BOOM_M);
        let Some(gap) = gap_m(view.body(boom_m, velocity)) else {
            last = None;
            continue;
        };
        if gap <= 0.0 {
            // Between the last boom above the floor and this one, where the line meets it.
            let met_m = last.map_or(boom_m, |(above_m, above_gap)| {
                above_m + (boom_m - above_m) * above_gap / (above_gap - gap)
            });
            let at = view.body(met_m, velocity);
            let floor = floor_under(ground, at, sea_level_m)?;
            return Some(CameraBody {
                at: Position { z: floor, ..at },
                boom_m: met_m,
            });
        }
        last = Some((boom_m, gap));
    }
    None
}

/// The radius of the sphere the client's camera casts against the level, m
/// (`libs/camera/gamecamera.xml`: `collisionSphereCastRadius`).
const CAMERA_SPHERE_RADIUS_M: f32 = 0.084;
/// The client camera's near plane, m (`libs/camera/gamecamera.xml`: `NearPlane`).
const CAMERA_NEAR_PLANE_M: f32 = 0.1;
/// How far above its floor a camera that met it may stand past the sphere and the near plane, m
/// (our choice: the level's floor under the camera may differ a little from the client's).
const CAMERA_FLOOR_SLACK_M: f32 = 0.1;
/// The highest a camera may stand above the floor under it and have met that floor, m.
const CAMERA_FLOOR_MET_M: f32 = CAMERA_SPHERE_RADIUS_M + CAMERA_NEAR_PLANE_M + CAMERA_FLOOR_SLACK_M;

/// Whether the camera of `view` met the floor of `ground` under it: it looks up (a negative
/// pitch puts the camera below the body's eyes) and stands at most [`CAMERA_FLOOR_MET_M`] above
/// that floor. Such a camera's boom is shorter than the check's by what the floor took off it
/// (`CameraCollides`), so it gives no body along the view. Live 2026-11-22 01:46:40.485 UTC, a
/// controller player at rest after a musket aim (records 5385 to 5398): the pitch went from
/// -0.19 to -0.61 rad, the camera stayed at 45.106 m, 0.18 m above the flat floor, and came
/// from 7.54 to 1.88 m of the still body; the check put the body where its 7.67 m boom met that
/// camera and moved it 4.4 m for the other player. 2,220 of that player's 9,553 records looked
/// up so (pitch below -0.2 rad, camera within 0.4 m of the feet), 16 of the keyboard player's
/// 20,147.
fn camera_met_floor(view: CameraView, ground: &(impl Ground + ?Sized)) -> bool {
    let camera = view.camera;
    view.pitch < 0.0
        && ground
            .floor_at(camera.x, camera.y, camera.z)
            .is_some_and(|floor| camera.z - floor <= CAMERA_FLOOR_MET_M)
}

/// The floor under the feet of a body at `at`: the floor of `ground` there, or the swim floor of
/// the water there (the sea at `sea_level_m` where `ground` knows no water) when that is higher.
fn floor_under(ground: &(impl Ground + ?Sized), at: Position, sea_level_m: f32) -> Option<f32> {
    let swim = swim_floor_at(ground, at.x, at.y, sea_level_m);
    match ground.floor_at(at.x, at.y, at.z) {
        Some(floor) => Some(swim.map_or(floor, |swim| floor.max(swim))),
        None => ground.height_at(at.x, at.y).and(swim),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera_check::{CAMERA_LAG_S, CAMERA_RISE_M, SHOULDER_RIGHT_M};
    use crate::character::{ACCELERATION_MPS2, Intent, yaw_axes};
    use crate::character::{CharacterInput, swim_floor};

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

    /// The camera of a body at `body` looking north with boom `boom`, trailing `velocity`.
    fn view_of(body: Position, boom: f32, velocity: [f32; 2]) -> CameraView {
        let (forward, right) = yaw_axes(0.0);
        let level = boom * PITCH.cos();
        CameraView::new(
            Position {
                x: body.x - level * forward[0] + SHOULDER_RIGHT_M * right[0]
                    - CAMERA_LAG_S * velocity[0],
                y: body.y - level * forward[1] + SHOULDER_RIGHT_M * right[1]
                    - CAMERA_LAG_S * velocity[1],
                z: body.z + CAMERA_RISE_M + boom * PITCH.sin(),
            },
            0.0,
            PITCH,
        )
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

    /// A body at [`start`] whose camera learnt its boom there, and the view of a client that
    /// stands `gap_m` to its side.
    fn lost_by(gap_m: f32) -> (TrackedBody, Position, CameraView) {
        let client = Position {
            x: start().x + gap_m,
            ..start()
        };
        let mut body = TrackedBody::new(start());
        for _ in 0..10 {
            body.record(input(0, view_of(start(), BOOM, [0.0; 2])), &Flat, SEA);
        }
        body.stand_at(start());
        (body, client, view_of(client, BOOM, [0.0; 2]))
    }

    #[test]
    fn a_body_its_camera_lost_goes_back_to_the_camera_when_its_client_acts() {
        // The client's body: 7 m to the side of the server's (live 2026-09-26 after Thorpe's
        // first grab pull). It attacks on the spot: its action can move it.
        const GAP_M: f32 = 7.0;
        let (mut body, client, away) = lost_by(GAP_M);
        let attack = TrackInput {
            action_started: true,
            ..input(0, away)
        };
        let mut tracked = body.record(attack, &Flat, SEA);
        for _ in 0..RECOVER_AFTER_RECORDS {
            tracked = body.record(input(0, away), &Flat, SEA);
        }
        assert!(
            tracked.position.horizontal_distance(client) < 0.1,
            "{:?}: the body goes to its client",
            tracked.position
        );
    }

    #[test]
    fn a_camera_that_leaves_a_body_at_rest_does_not_move_it() {
        // An NPC's talk or a town board: the camera shows something 4.8 m away, no key is held
        // (retail `Hivetownboardmissions` from 457.6 s).
        const GAP_M: f32 = 4.8;
        let (mut body, _, away) = lost_by(GAP_M);
        for record in 0..RECOVER_AFTER_RECORDS * 4 {
            let tracked = body.record(input(0, away), &Flat, SEA);
            assert!(!tracked.resynced, "record {record}");
            assert_eq!(tracked.position, start(), "record {record}");
        }
    }

    /// An attack whose animation moves its player 1 m forward over 300 ms.
    fn lunge() -> Arc<AttackMotion> {
        Arc::new(AttackMotion {
            step_ms: 100.0,
            offsets: vec![[0.0, 0.0], [0.0, 0.2], [0.0, 0.6], [0.0, 1.0]],
            inherit_velocity: false,
            inherit_max_mps: None,
            decel_mps2: [0.0; 2],
        })
    }

    /// A record of keys alone (no camera) with camera yaw `yaw`.
    fn keys(forward: i8, yaw: f32) -> TrackInput {
        TrackInput {
            character: CharacterInput {
                intent: Intent {
                    forward,
                    ..Intent::default()
                },
                yaw,
                dt_s: DT,
            },
            view: None,
            boom_key: false,
            action_started: false,
        }
    }

    /// How close two places count as the same, m.
    const CLOSE: f32 = 1e-3;
    /// Records of 1/30 s in the lunge's 300 ms.
    const LUNGE_RECORDS: usize = 9;
    /// Records of 1/30 s in its first 100 ms.
    const FIRST_STEP_RECORDS: usize = 3;
    /// How long an attack may run before a movement key ends it, ms: longer than the lunge.
    const NO_KEYS_MS: f32 = 1_000.0;

    #[test]
    fn an_attack_moves_the_body_by_its_animation_toward_the_camera_not_by_the_keys() {
        // A quarter turn: the camera looks along -x.
        let yaw = std::f32::consts::FRAC_PI_2;
        let mut body = TrackedBody::new(start());
        body.start_attack(lunge(), NO_KEYS_MS, 0.0, &Flat, SEA);
        for _ in 0..LUNGE_RECORDS {
            body.record(keys(0, yaw), &Flat, SEA);
        }
        let at = body.position();
        assert!(
            (at.x - (start().x - 1.0)).abs() < CLOSE && (at.y - start().y).abs() < CLOSE,
            "{at:?}"
        );
        body.stand_at(start());
        body.record(keys(0, yaw), &Flat, SEA);
        assert_eq!(body.position(), start(), "a server move ends the attack");
    }

    /// An ability's fragment starts with the record after the message that pressed its key
    /// ([`TrackedBody::start_motion`]): the records taken before stay as they were, and its
    /// motion carries the body on from there along the camera's forward, with no movement key.
    #[test]
    fn an_ability_s_motion_starts_with_the_next_record_along_the_camera() {
        // A quarter turn: the camera looks along -x.
        let yaw = std::f32::consts::FRAC_PI_2;
        let mut body = TrackedBody::new(start());
        body.record(keys(1, yaw), &Flat, SEA);
        let pressed_at = body.position();
        body.start_motion(lunge(), NO_KEYS_MS);
        assert_eq!(
            body.position(),
            pressed_at,
            "the press's record is not taken again"
        );
        for _ in 0..LUNGE_RECORDS {
            body.record(keys(0, yaw), &Flat, SEA);
        }
        let at = body.position();
        assert!(
            (at.x - (pressed_at.x - 1.0)).abs() < CLOSE && (at.y - pressed_at.y).abs() < CLOSE,
            "{at:?}"
        );
    }

    #[test]
    fn a_hit_reaction_holds_the_keys_and_the_dodge_until_its_free_point() {
        // A stagger that pushes the body 1 m along its facing (+y) over 300 ms, free from 200 ms.
        let mut body = TrackedBody::new(start());
        body.start_reaction(lunge(), 200.0);
        let dodge = TrackInput {
            character: CharacterInput {
                intent: Intent {
                    forward: -1,
                    dodge: true,
                    ..Intent::default()
                },
                yaw: 0.0,
                dt_s: DT,
            },
            ..keys(0, 0.0)
        };
        for _ in 0..FIRST_STEP_RECORDS {
            body.record(dodge, &Flat, SEA);
        }
        assert!(
            (body.position().y - (start().y + 0.2)).abs() < CLOSE,
            "the reaction's motion, not the back key or the dodge: {:?}",
            body.position()
        );
        for _ in FIRST_STEP_RECORDS..LUNGE_RECORDS {
            body.record(keys(1, 0.0), &Flat, SEA);
        }
        assert!(
            body.position().y > start().y + 0.6 + CLOSE,
            "a key from the free point moves the body by itself: {:?}",
            body.position()
        );
    }

    #[test]
    fn a_breakout_ends_the_reaction_s_motion_and_not_an_attack_s() {
        let mut reacting = TrackedBody::new(start());
        reacting.start_reaction(lunge(), 200.0);
        reacting.break_reaction();
        reacting.record(keys(0, 0.0), &Flat, SEA);
        assert_eq!(
            reacting.position(),
            start(),
            "no reaction motion after the breakout"
        );
        let mut attacking = TrackedBody::new(start());
        attacking.start_attack(lunge(), NO_KEYS_MS, 0.0, &Flat, SEA);
        attacking.break_reaction();
        attacking.record(keys(0, 0.0), &Flat, SEA);
        assert_ne!(attacking.position(), start(), "the attack's motion goes on");
    }

    #[test]
    fn a_movement_key_takes_the_body_back_from_the_attack_s_marker_on() {
        let mut keyed = TrackedBody::new(start());
        let mut still = TrackedBody::new(start());
        keyed.start_attack(lunge(), 100.0, 0.0, &Flat, SEA);
        still.start_attack(lunge(), 100.0, 0.0, &Flat, SEA);
        for _ in 0..FIRST_STEP_RECORDS {
            keyed.record(keys(1, 0.0), &Flat, SEA);
            still.record(keys(0, 0.0), &Flat, SEA);
        }
        assert!(
            (keyed.position().y - (start().y + 0.2)).abs() < CLOSE,
            "the animation, not the keys, before the marker: {:?}",
            keyed.position()
        );
        for _ in FIRST_STEP_RECORDS..LUNGE_RECORDS {
            keyed.record(keys(1, 0.0), &Flat, SEA);
            still.record(keys(0, 0.0), &Flat, SEA);
        }
        assert!((still.position().y - (start().y + 1.0)).abs() < CLOSE);
        assert!(
            (keyed.position().y - still.position().y).abs() > CLOSE,
            "the keys moved the body from the marker on"
        );
    }

    #[test]
    fn an_attack_halts_at_the_capsule_of_the_character_it_meets() {
        // The camera looks along +y; a character's 0.4 m capsule stands 1.2 m ahead.
        let characters = [crate::CharacterBody {
            x: start().x,
            y: start().y + 1.2,
            radius_m: 0.4,
        }];
        let ground = crate::AmongCharacters {
            ground: &Flat,
            characters: &characters,
        };
        let mut body = TrackedBody::new(start());
        body.start_attack(lunge(), NO_KEYS_MS, 0.0, &ground, SEA);
        for _ in 0..LUNGE_RECORDS {
            body.record(keys(0, 0.0), &ground, SEA);
        }
        let gone = body.position().y - start().y;
        assert!(
            gone < 1.2 - 0.4 - crate::character::PLAYER_RADIUS_M + CLOSE && gone > 0.0,
            "the capsules touch, the body goes no further: {gone}"
        );
    }

    #[test]
    fn an_attack_older_than_the_body_s_records_goes_on_from_where_it_is() {
        // Started 200 ms ago, before any record the body took: 0.6 m of its 1 m are behind it.
        let mut body = TrackedBody::new(start());
        body.start_attack(lunge(), NO_KEYS_MS, 200.0, &Flat, SEA);
        for _ in 0..FIRST_STEP_RECORDS {
            body.record(keys(0, 0.0), &Flat, SEA);
        }
        assert!(
            (body.position().y - (start().y + 0.4)).abs() < CLOSE,
            "{:?}",
            body.position()
        );
    }

    #[test]
    fn an_attack_that_starts_later_waits_for_its_start() {
        let mut body = TrackedBody::new(start());
        body.start_attack(lunge(), NO_KEYS_MS, -100.0, &Flat, SEA);
        for _ in 0..FIRST_STEP_RECORDS {
            body.record(keys(0, 0.0), &Flat, SEA);
        }
        assert_eq!(body.position(), start(), "not yet started");
        for _ in 0..FIRST_STEP_RECORDS {
            body.record(keys(0, 0.0), &Flat, SEA);
        }
        assert!(
            (body.position().y - (start().y + 0.2)).abs() < CLOSE,
            "{:?}",
            body.position()
        );
    }

    #[test]
    fn an_attack_the_server_learns_late_takes_its_records_again() {
        let mut late = TrackedBody::new(start());
        for _ in 0..FIRST_STEP_RECORDS {
            late.record(keys(1, 0.0), &Flat, SEA);
        }
        late.start_attack(lunge(), NO_KEYS_MS, 100.0, &Flat, SEA);
        let mut early = TrackedBody::new(start());
        early.start_attack(lunge(), NO_KEYS_MS, 0.0, &Flat, SEA);
        for _ in 0..FIRST_STEP_RECORDS {
            early.record(keys(1, 0.0), &Flat, SEA);
        }
        assert!(
            late.position().horizontal_distance(early.position()) < CLOSE,
            "{:?} against {:?}",
            late.position(),
            early.position()
        );
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
    fn a_rider_keeps_its_mount_s_speed_and_the_floor_under_a_saddle_camera() {
        use crate::camera_check::MOUNTED_CAMERA_LAG_S;
        use crate::character::{MountPace, MountPaces};
        /// Retail Moonshadow's road run, and retail's 20 input records a second.
        const RUN_MPS: f32 = 9.25;
        const RECORD_S: f32 = 0.05;
        /// A rider's camera above a walker's (retail Hermit's Rally: 3.91 m against 1.685 m).
        const SADDLE_M: f32 = 2.2;
        const RECORDS: u32 = 60;
        let pace = MountPace {
            run_mps: RUN_MPS,
            walk_mps: 5.0,
            slow_walk_mps: 2.0,
            acceleration_mps2: 1000.0,
            deceleration_mps2: 9.0,
            walk_acceleration_mps2: 15.0,
            walk_deceleration_mps2: 12.0,
        };
        let mut body = TrackedBody::new(start());
        body.mount(MountPaces {
            on_road: pace,
            off_road: pace,
            very_shallow_water: pace,
            shallow_water: pace,
            deep_water: pace,
            dash_speed_scale: 1.0,
            dash_acceleration_scale: 1.0,
        });
        let mut client = start();
        // `view_of` trails by the walker's lag: scale the motion to the rider's.
        let trail = [0.0, RUN_MPS * MOUNTED_CAMERA_LAG_S / CAMERA_LAG_S];
        for _ in 0..RECORDS {
            client.y += RUN_MPS * RECORD_S;
            let camera = Position {
                z: client.z + SADDLE_M,
                ..client
            };
            let mut record = input(1, view_of(camera, BOOM, trail));
            record.character.dt_s = RECORD_S;
            body.record(record, &Flat, SEA);
        }
        let at = body.position();
        assert!(
            (at.z - GROUND_Z).abs() < 1e-3,
            "on the floor, not the saddle: {at:?}"
        );
        assert!(
            at.horizontal_distance(client) < 0.5,
            "at the mount's speed, past the walking step of 0.36 m a record: {at:?} {client:?}"
        );
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
        let mut max_step = 0.0_f32;
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
            max_step = max_step.max(tracked.position.horizontal_distance(last));
            last = tracked.position;
        }
        // The server's body stays within the threshold, the camera's trail of a running body and
        // one record of running of the wall: it never runs away into the wall. Each pull-back
        // stops the character, so it takes records to drift past the threshold again; while it
        // is held there the corrections run on, each capped, so no step of the body is larger
        // than one record of running plus one correction.
        let bound = FOLLOW_FROM_M + (CAMERA_LAG_S + DT) * SPRINT_SPEED_MPS;
        assert!(last.horizontal_distance(wall) <= bound, "{last:?}");
        assert!(resyncs > 0, "{resyncs}");
        assert!(
            max_step <= SPRINT_SPEED_MPS * DT + FOLLOW_STEP_M + 0.01,
            "{max_step}"
        );
    }

    #[test]
    fn a_camera_that_holds_a_jumping_body_moves_it_on_the_ground_plane_only() {
        // Both run north and jump; the client's body stops at a wall 1 m ahead in the air, the
        // server's flies on until the camera puts it back.
        let keys = |jump: bool| CharacterInput {
            intent: Intent {
                forward: 1,
                jump,
                ..Intent::default()
            },
            yaw: 0.0,
            dt_s: DT,
        };
        let mut body = TrackedBody::new(start());
        let mut client = Character::new(start());
        for _ in 0..30 {
            client.step(keys(false), &Flat, SEA);
            body.record(
                input(1, view_of(client.position(), BOOM, client.velocity())),
                &Flat,
                SEA,
            );
        }
        let wall_y = client.position().y + 1.0;
        let bound = FOLLOW_FROM_M + (CAMERA_LAG_S + DT) * SPRINT_SPEED_MPS;
        let mut highest: f32 = GROUND_Z;
        for record in 0..15 {
            if client.position().y < wall_y {
                client.step(keys(record == 0), &Flat, SEA);
            }
            let stopped = client.position().y >= wall_y;
            let velocity = if stopped { [0.0; 2] } else { client.velocity() };
            let tracked = body.record(
                TrackInput {
                    character: keys(record == 0),
                    ..input(1, view_of(client.position(), BOOM, velocity))
                },
                &Flat,
                SEA,
            );
            assert!(
                tracked.position.horizontal_distance(client.position()) <= bound,
                "record {record}: {tracked:?}"
            );
            highest = highest.max(tracked.position.z);
        }
        assert!(highest > GROUND_Z + 0.5, "the jump keeps its own height");
    }

    #[test]
    fn a_client_on_steps_the_level_lacks_takes_the_camera_height() {
        // Run 17, the tutorial cave: the client climbs steps that the level collision does not
        // have; the server's body climbs with its camera instead of staying at the foot.
        const RISE_PER_RECORD_M: f32 = 0.1;
        const RECORDS: u32 = 40;
        let mut body = TrackedBody::new(start());
        let mut client = start();
        for _ in 0..RECORDS {
            client.y += SPRINT_SPEED_MPS * DT;
            client.z += RISE_PER_RECORD_M;
            let velocity = [0.0, SPRINT_SPEED_MPS];
            body.record(input(1, view_of(client, BOOM, velocity)), &Flat, SEA);
        }
        let at = body.position();
        assert!(
            (at.z - client.z).abs() < CAMERA_FLOOR_SPREAD_M,
            "the body climbed with the client: {at:?} {client:?}"
        );
    }

    #[test]
    fn an_action_s_camera_rise_does_not_lift_the_body_onto_a_floor_the_level_lacks() {
        // Live 2026-11-17: a held heavy attack raised the camera's body 1.2 m over flat ground.
        const ACTION_RISE_M: f32 = 1.2;
        let mut body = TrackedBody::new(start());
        for _ in 0..10 {
            body.record(input(0, view_of(start(), BOOM, [0.0; 2])), &Flat, SEA);
        }
        let raised = Position {
            z: GROUND_Z + ACTION_RISE_M,
            ..start()
        };
        let attack = TrackInput {
            action_started: true,
            ..input(0, view_of(raised, BOOM, [0.0; 2]))
        };
        body.record(attack, &Flat, SEA);
        for _ in 0..20 {
            body.record(input(0, view_of(raised, BOOM, [0.0; 2])), &Flat, SEA);
        }
        assert!(
            (body.position().z - GROUND_Z).abs() < 1e-3,
            "the body stays on the floor: {:?}",
            body.position()
        );
    }

    /// The view of a client whose camera's body stands `rise_m` above the tracked body.
    fn raised_view(body: &TrackedBody, rise_m: f32) -> CameraView {
        let at = body.position();
        view_of(
            Position {
                z: at.z + rise_m,
                ..at
            },
            BOOM,
            body.velocity(),
        )
    }

    /// A body that stood on flat ground with its camera at rest.
    fn settled() -> TrackedBody {
        let mut body = TrackedBody::new(start());
        for _ in 0..10 {
            body.record(input(0, view_of(start(), BOOM, [0.0; 2])), &Flat, SEA);
        }
        body
    }

    fn assert_on_the_ground(body: &TrackedBody, what: &str) {
        assert!(
            (body.position().z - GROUND_Z).abs() < 1e-3,
            "{what}: {:?}",
            body.position()
        );
    }

    #[test]
    fn a_resting_camera_1_07_m_up_keeps_the_body_on_open_ground() {
        // Live 2026-11-17: a controller player's camera's body stood 1.07 m above flat ground
        // with no structure; with a 1.0 m spread her body stood 1 m up in the air.
        const REST_RISE_M: f32 = 1.07;
        const REST_RECORDS: usize = 60;
        let mut body = settled();
        for _ in 0..REST_RECORDS {
            let view = raised_view(&body, REST_RISE_M);
            body.record(input(0, view), &Flat, SEA);
        }
        assert_on_the_ground(&body, "the body stays on the ground");
    }

    #[test]
    fn a_still_body_climbs_onto_no_floor_the_level_lacks() {
        // Live 2026-11-18: a controller player stopped with its camera's body 2.1 m and 2.7 m
        // above flat ground, and the body went up there.
        const RISE_M: f32 = 2.7;
        const RECORDS: usize = 60;
        let mut body = settled();
        for _ in 0..RECORDS {
            let view = raised_view(&body, RISE_M);
            body.record(input(0, view), &Flat, SEA);
            assert_on_the_ground(&body, "standing");
        }
    }

    #[test]
    fn a_dodge_s_camera_rise_does_not_lift_the_body() {
        // Live 2026-11-17: in a controller player's dodge the camera's body rose 1.9 m over 8
        // records on flat ground; the body went up with it and the others saw a fall, no roll.
        const DODGE_RECORDS: u8 = 8;
        const RISE_PER_RECORD_M: f32 = 0.24;
        const HIGH_RECORDS: usize = 3;
        const AFTER_RECORDS: usize = 20;
        let mut body = settled();
        let dodge = |view| TrackInput {
            character: CharacterInput {
                intent: Intent {
                    forward: 1,
                    dodge: true,
                    ..Intent::default()
                },
                yaw: 0.0,
                dt_s: DT,
            },
            ..input(0, view)
        };
        for record in 1..=DODGE_RECORDS {
            let view = raised_view(&body, RISE_PER_RECORD_M * f32::from(record));
            body.record(dodge(view), &Flat, SEA);
            assert_on_the_ground(&body, "in the dodge");
        }
        let top_m = RISE_PER_RECORD_M * f32::from(DODGE_RECORDS);
        for _ in 0..HIGH_RECORDS {
            let view = raised_view(&body, top_m);
            body.record(input(0, view), &Flat, SEA);
            assert_on_the_ground(&body, "after the dodge");
        }
        for _ in 0..AFTER_RECORDS {
            let view = raised_view(&body, 0.0);
            body.record(input(0, view), &Flat, SEA);
        }
        assert_on_the_ground(&body, "at rest again");
    }

    #[test]
    fn a_jump_s_landing_with_its_camera_still_up_stays_on_the_ground() {
        // Live 2026-11-17: at a standing jump's landing the camera's body still stood 1.4 m up
        // and came down over 6 records; the body went up onto it, fell again and landed twice.
        const LANDING_RISE_M: f32 = 1.8;
        const SETTLE_RECORDS: usize = 6;
        const AFTER_RECORDS: usize = 20;
        const AIR_RECORDS_MAX: usize = 120;
        let mut body = settled();
        let jump = TrackInput {
            character: CharacterInput {
                intent: Intent {
                    jump: true,
                    ..Intent::default()
                },
                yaw: 0.0,
                dt_s: DT,
            },
            ..input(0, raised_view(&body, 0.0))
        };
        body.record(jump, &Flat, SEA);
        let mut records = 0;
        while !body.is_grounded() {
            records += 1;
            assert!(records < AIR_RECORDS_MAX, "the jump lands");
            let view = raised_view(&body, 0.0);
            body.record(input(0, view), &Flat, SEA);
        }
        for record in 0..SETTLE_RECORDS {
            let share = 1.0 - record as f32 / SETTLE_RECORDS as f32;
            let view = raised_view(&body, LANDING_RISE_M * share);
            body.record(input(0, view), &Flat, SEA);
            assert_on_the_ground(&body, "after the landing");
            assert!(body.is_grounded(), "no second fall");
        }
        for _ in 0..AFTER_RECORDS {
            let view = raised_view(&body, 0.0);
            body.record(input(0, view), &Flat, SEA);
        }
        assert_on_the_ground(&body, "at rest again");
    }

    /// The input of a game pad tilted fully forward, with camera `view`.
    fn pad(view: CameraView) -> TrackInput {
        TrackInput {
            character: CharacterInput {
                intent: Intent {
                    forward: 1,
                    stick: Some([0.0, 1.0]),
                    ..Intent::default()
                },
                yaw: 0.0,
                dt_s: DT,
            },
            ..input(0, view)
        }
    }

    #[test]
    fn a_pad_run_under_a_camera_too_high_stays_on_the_ground() {
        // Live 2026-11-17: a controller player's camera put its running body 3.9 m above flat
        // ground, and the others saw it float.
        const RISE_M: f32 = 3.9;
        const RECORDS: usize = 40;
        let mut body = settled();
        for _ in 0..RECORDS {
            let view = raised_view(&body, RISE_M);
            body.record(pad(view), &Flat, SEA);
            assert_on_the_ground(&body, "in the run");
        }
    }

    #[test]
    fn a_pad_run_smooths_a_noisy_camera() {
        // Live 2026-11-17: the body followed the pad's noisy camera and went back and forth.
        const NOISE_M: f32 = 0.3;
        const RECORDS: usize = 60;
        const MOST_REVERSES: usize = 3;
        let mut body = settled();
        let mut client = start();
        let mut last = body.position();
        let mut last_step: Option<f32> = None;
        let mut reverses = 0;
        for record in 0..RECORDS {
            client.y += SPRINT_SPEED_MPS * DT;
            let noise = if record % 2 == 0 { NOISE_M } else { -NOISE_M };
            let seen = Position {
                y: client.y + noise,
                ..client
            };
            body.record(
                pad(view_of(seen, BOOM, [0.0, SPRINT_SPEED_MPS])),
                &Flat,
                SEA,
            );
            let step = body.position().y - last.y;
            if last_step.is_some_and(|last| last * step < 0.0) {
                reverses += 1;
            }
            last_step = Some(step);
            last = body.position();
        }
        assert!(reverses <= MOST_REVERSES, "{reverses} reverses");
    }

    #[test]
    fn a_pad_run_then_a_still_block_keeps_the_boom_and_stands_with_its_client() {
        // Replay of the retail tutorial with the moves as a pad's stick: the body lagged its
        // client, the held boom fell from 4.0 to 2.75 m, and a still block stood 0.98 m behind.
        // A 20 s run: the old rule lost 0.04 m of boom a second, past half a zoom step.
        const RUN_RECORDS: usize = 600;
        const STILL_RECORDS: usize = 30;
        /// The most the boom may leave its zoom length, m (our choice: a tenth of a zoom step).
        const BOOM_KEPT_M: f32 = 0.1;
        /// The most the still body may stand from its client, m (our choice).
        const STILL_GAP_M: f32 = 0.1;
        let mut body = settled();
        let mut client = start();
        for _ in 0..RUN_RECORDS {
            client.y += SPRINT_SPEED_MPS * DT;
            body.record(
                pad(view_of(client, BOOM, [0.0, SPRINT_SPEED_MPS])),
                &Flat,
                SEA,
            );
        }
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        for _ in 0..STILL_RECORDS {
            body.record(
                TrackInput {
                    character: CharacterInput {
                        intent: block,
                        yaw: 0.0,
                        dt_s: DT,
                    },
                    ..input(0, view_of(client, BOOM, [0.0; 2]))
                },
                &Flat,
                SEA,
            );
        }
        let boom = body.boom_m().expect("a held boom");
        assert!((boom - BOOM).abs() < BOOM_KEPT_M, "boom {boom} m");
        let gap = body.position().horizontal_distance(client);
        assert!(gap < STILL_GAP_M, "still block {gap} m from its client");
    }

    /// A ledge: a floor [`LEDGE_M`] above the flat ground south of [`start`]'s y plus 1 m.
    struct Ledge;

    /// The height of [`Ledge`] above the ground, m.
    const LEDGE_M: f32 = 15.0;

    impl Ground for Ledge {
        fn height_at(&self, _: f32, y: f32) -> Option<f32> {
            Some(if y < start().y + 1.0 {
                GROUND_Z + LEDGE_M
            } else {
                GROUND_Z
            })
        }
    }

    #[test]
    fn a_client_that_runs_off_a_ledge_falls_by_the_physics() {
        // A real fall must stay a fall (its landing is the fall damage's drop), not a climb down
        // the camera.
        let top = Position {
            z: GROUND_Z + LEDGE_M,
            ..start()
        };
        let mut body = TrackedBody::new(top);
        let mut client = Character::new(top);
        let mut airborne = false;
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
                &Ledge,
                SEA,
            );
            let tracked = body.record(
                input(1, view_of(client.position(), BOOM, client.velocity())),
                &Ledge,
                SEA,
            );
            airborne |= !tracked.on_floor;
        }
        assert!(airborne, "the body fell");
        assert!(
            (body.position().z - GROUND_Z).abs() < 1e-3,
            "it landed: {:?}",
            body.position()
        );
    }

    #[test]
    fn a_client_that_runs_off_a_floor_the_level_lacks_falls_by_the_physics() {
        // The client stands on a wall that the server's level does not have (a town building
        // spawned by a card) and runs off it. The body went to the ground in one record, on a
        // floor the whole time, so its landing had no drop and no fall damage (live 2026-11-10).
        /// The least of the wall's height that the body's fall covers: its first part follows
        /// the camera down until the camera sinks faster than a walk.
        const LEAST_FALL_M: f32 = LEDGE_M - 3.0;
        let top = Position {
            z: GROUND_Z + LEDGE_M,
            ..start()
        };
        let mut body = TrackedBody::new(top);
        let mut client = Character::new(top);
        let mut fall_top: Option<f32> = None;
        for _ in 0..120 {
            client.step(
                CharacterInput {
                    intent: Intent {
                        forward: 1,
                        ..Intent::default()
                    },
                    yaw: 0.0,
                    dt_s: DT,
                },
                &Ledge,
                SEA,
            );
            let tracked = body.record(
                input(1, view_of(client.position(), BOOM, client.velocity())),
                &Flat,
                SEA,
            );
            if !tracked.on_floor {
                fall_top =
                    Some(fall_top.map_or(tracked.position.z, |top| top.max(tracked.position.z)));
            }
        }
        let landed = body.position();
        assert!((landed.z - GROUND_Z).abs() < 1e-3, "it landed: {landed:?}");
        let fall_top = fall_top.expect("the body fell");
        assert!(
            fall_top - landed.z > LEAST_FALL_M,
            "the fall has its height: from {fall_top} to {}",
            landed.z
        );
    }

    #[test]
    fn a_jump_on_a_floor_the_level_lacks_comes_down_on_that_floor() {
        let top = Position {
            z: GROUND_Z + LEDGE_M,
            ..start()
        };
        /// The client walks onto the floor first: a still body climbs onto none.
        const WALK_RECORDS: usize = 5;
        let keys = |record: usize, jump: bool| CharacterInput {
            intent: Intent {
                forward: i8::from(record < WALK_RECORDS),
                jump,
                ..Intent::default()
            },
            yaw: 0.0,
            dt_s: DT,
        };
        /// A ground at the wall's height everywhere: the client's own floor.
        struct Wall;
        impl Ground for Wall {
            fn height_at(&self, _: f32, _: f32) -> Option<f32> {
                Some(GROUND_Z + LEDGE_M)
            }
        }
        let mut body = TrackedBody::new(top);
        let mut client = Character::new(top);
        let mut lowest = top.z;
        for record in 0..80 {
            let jump = record == 10;
            client.step(keys(record, jump), &Wall, SEA);
            let tracked = body.record(
                TrackInput {
                    character: keys(record, jump),
                    ..input(0, view_of(client.position(), BOOM, client.velocity()))
                },
                &Flat,
                SEA,
            );
            lowest = lowest.min(tracked.position.z);
        }
        assert!(
            top.z - lowest < CAMERA_FLOOR_SPREAD_M,
            "the body stayed on the wall: lowest {lowest}"
        );
        assert!(body.is_grounded(), "it landed on the wall");
    }

    /// Stairs: a floor that rises 0.25 m per 0.5 m north of [`start`], over the flat ground.
    struct Stairs;

    impl Ground for Stairs {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_Z)
        }

        fn floor_at(&self, _: f32, y: f32, z: f32) -> Option<f32> {
            const RISER_M: f32 = 0.25;
            const TREAD_M: f32 = 0.5;
            let steps = ((y - start().y) / TREAD_M).floor().max(0.0);
            let step = GROUND_Z + steps * RISER_M;
            Some(if step <= z + 1.3 { step } else { GROUND_Z })
        }
    }

    #[test]
    fn the_body_climbs_known_stairs_whatever_height_the_camera_gives() {
        // The camera's body 0.5 m under the stairs: the floor is the ground's, not the camera's.
        const CAMERA_ERROR_M: f32 = -0.5;
        let mut body = TrackedBody::new(start());
        let mut runner = Character::new(start());
        for _ in 0..40 {
            let keys = input(1, view_of(start(), BOOM, [0.0; 2]));
            runner.step(keys.character, &Stairs, SEA);
            let client = runner.position();
            let shown = Position {
                z: client.z + CAMERA_ERROR_M,
                ..client
            };
            let tracked = body.record(
                input(1, view_of(shown, BOOM, runner.velocity())),
                &Stairs,
                SEA,
            );
            let floor = Stairs
                .floor_at(tracked.position.x, tracked.position.y, tracked.position.z)
                .expect("floor");
            assert!((tracked.position.z - floor).abs() < 1e-3, "{tracked:?}");
        }
        assert!(body.position().z > GROUND_Z + 1.0, "{:?}", body.position());
    }

    /// The tutorial galleon's upper deck ([`deck_triangles`]) over the `ftue_v2` terrain.
    fn galleon() -> crate::LevelGround<crate::test_support::FlatGround> {
        use nw_world::collision::{Body, Geometry, IDENTITY_AFFINE, LevelCollision};
        use nw_world::collision_index::CollisionIndex;
        /// The `ftue_v2` terrain under the ship, m.
        const SHIP_TERRAIN_M: f32 = 50.0;
        let triangles = deck_triangles();
        let points: Vec<[f32; 3]> = triangles.iter().flatten().copied().collect();
        let corners = (0..triangles.len())
            .map(|index| {
                let first = u16::try_from(index * 3).expect("a few triangles");
                [first, first + 1, first + 2]
            })
            .collect();
        let collision = CollisionIndex::new(LevelCollision {
            level: "ftue_v2".to_owned(),
            geometries: vec![Geometry::Mesh {
                points,
                triangles: corners,
            }],
            bodies: vec![Body {
                geometry: 0,
                place: IDENTITY_AFFINE,
            }],
            ..LevelCollision::default()
        });
        crate::LevelGround::new(
            crate::test_support::FlatGround(SHIP_TERRAIN_M),
            std::sync::Arc::new(collision),
        )
    }

    /// The triangles of `testdata/ftue-galleon-deck.txt`.
    fn deck_triangles() -> Vec<[[f32; 3]; 3]> {
        include_str!("testdata/ftue-galleon-deck.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(|line| {
                let v: Vec<f32> = line
                    .split_whitespace()
                    .map(|value| value.parse().expect("number"))
                    .collect();
                [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]]
            })
            .collect()
    }

    /// The place of the block under the tutorial beach's stair landing: body 6 of the `ftue_v2`
    /// level collision, a box of half size 1 (4 m high from 19.39 m; retail walks onto its top
    /// at 23.39 m from the stairs).
    const BEACH_BLOCK: [[f32; 4]; 3] = [
        [-8.812_316, 1.654_394, 0.0, 719.742_25],
        [-4.726_837, -3.084_312, 0.0, 368.216_64],
        [0.0, 0.0, 2.0, 21.390_278],
    ];
    /// The beach sand under run 15's own places by the block, m.
    const BEACH_SAND_M: f32 = 19.945;

    /// The block over the sand.
    fn beach_block() -> crate::LevelGround<crate::test_support::FlatGround> {
        use nw_world::collision::{Body, Geometry, LevelCollision};
        use nw_world::collision_index::CollisionIndex;
        let collision = CollisionIndex::new(LevelCollision {
            level: "ftue_v2".to_owned(),
            geometries: vec![Geometry::Box {
                half_extents: [1.0; 3],
            }],
            bodies: vec![Body {
                geometry: 0,
                place: BEACH_BLOCK,
            }],
            ..LevelCollision::default()
        });
        crate::LevelGround::new(
            crate::test_support::FlatGround(BEACH_SAND_M),
            std::sync::Arc::new(collision),
        )
    }

    /// Whether `at` stands in the block's ground-plane outline.
    fn in_beach_block(at: Position) -> bool {
        let [x_row, y_row, _] = BEACH_BLOCK;
        let offset = [at.x - x_row[3], at.y - y_row[3]];
        [0, 1].into_iter().all(|axis| {
            let column = [x_row[axis], y_row[axis]];
            let share = (offset[0] * column[0] + offset[1] * column[1])
                / (column[0] * column[0] + column[1] * column[1]);
            share.abs() <= 1.0
        })
    }

    #[test]
    fn the_run_15_camera_does_not_carry_the_body_into_the_beach_block() {
        // Run 15 (2026-09-22, 03:49:22Z): by the beach stairs the camera's body ran north into
        // the block, 0.7 to 1.3 m ahead of the server's, and the resyncs put the body inside it;
        // its client was pushed into the solid and stuck. These are the server's own places of
        // those states, from 715.55, 359.17 on; the client's capsule cannot be inside the block.
        const RUN_15_PLACES: [[f32; 2]; 9] = [
            [715.008, 359.864],
            [715.729, 360.947],
            [716.043, 361.653],
            [715.887, 362.202],
            [715.746, 362.924],
            [715.966, 363.065],
            [715.887, 364.053],
            [716.039, 365.316],
            [715.673, 366.556],
        ];
        /// Records each place shows for (the states came 33 to 65 ms apart).
        const RECORDS_EACH: usize = 2;
        /// Records the last place holds after the run.
        const HOLD_RECORDS: usize = 30;
        let start = Position {
            x: 715.551,
            y: 359.173,
            z: BEACH_SAND_M,
        };
        let ground = beach_block();
        assert!(in_beach_block(Position {
            x: RUN_15_PLACES[8][0],
            y: RUN_15_PLACES[8][1],
            ..start
        }));
        assert!(!in_beach_block(start));
        let mut body = TrackedBody::new(start);
        let shown = RUN_15_PLACES
            .iter()
            .flat_map(|place| std::iter::repeat_n(*place, RECORDS_EACH))
            .chain(std::iter::repeat_n(RUN_15_PLACES[8], HOLD_RECORDS));
        let mut last = start;
        for (record, [x, y]) in shown.enumerate() {
            let client = Position { x, y, ..start };
            let tracked = body.record(input(1, view_of(client, BOOM, [0.0; 2])), &ground, SEA);
            assert!(
                !in_beach_block(tracked.position),
                "record {record}: {tracked:?}"
            );
            last = tracked.position;
        }
        assert!(last.y > start.y + 2.0, "it went up to the block: {last:?}");
    }

    #[test]
    fn a_client_that_climbed_onto_the_block_takes_the_body_onto_its_top() {
        // A mantle or a vault: the client shows its body on the block's top, past its side.
        /// The block's top, m.
        const TOP_M: f32 = 23.39;
        /// Records at rest, then records on the top.
        const REST_RECORDS: usize = 10;
        const TOP_RECORDS: usize = 20;
        let below = Position {
            x: 716.043,
            y: 361.653,
            z: BEACH_SAND_M,
        };
        let on_top = Position {
            x: 715.746,
            y: 362.924,
            z: TOP_M,
        };
        assert!(!in_beach_block(below) && in_beach_block(on_top));
        let ground = beach_block();
        let mut body = TrackedBody::new(below);
        for _ in 0..REST_RECORDS {
            body.record(input(0, view_of(below, BOOM, [0.0; 2])), &ground, SEA);
        }
        let mut last = below;
        for _ in 0..TOP_RECORDS {
            last = body
                .record(input(1, view_of(on_top, BOOM, [0.0; 2])), &ground, SEA)
                .position;
        }
        assert!(in_beach_block(last), "{last:?}");
        assert!((last.z - TOP_M).abs() < 0.01, "on the top: {last:?}");
    }

    #[test]
    fn where_the_level_has_no_data_the_body_takes_the_camera_height() {
        let place = Position { z: 40.0, ..start() };
        let mut body = TrackedBody::new(start());
        let tracked = body.record(
            input(0, view_of(place, BOOM, [0.0; 2])),
            &crate::test_support::NoGround,
            SEA,
        );
        assert!(tracked.consistent);
        assert!((tracked.position.z - place.z).abs() < 0.01, "{tracked:?}");
    }

    /// The tutorial start: retail's own place before any key.
    const RETAIL_START: Position = Position {
        x: 478.5,
        y: 849.7,
        z: 89.382,
    };

    #[test]
    fn the_retail_start_stands_on_the_galleon_deck() {
        /// The own place's step: 4/255 m.
        const PLACE_STEP_M: f32 = 4.0 / 255.0;
        let deck = galleon()
            .floor_at(RETAIL_START.x, RETAIL_START.y, RETAIL_START.z)
            .expect("deck");
        assert!((deck - RETAIL_START.z).abs() < PLACE_STEP_M, "{deck}");
    }

    #[test]
    fn the_run_14_ship_fight_keeps_the_body_on_the_deck() {
        // Run 14 (2026-09-20): in the first sailor fight the solve's boom drifted 1.1 m long in
        // a fast turn of the view, the camera's body went down to 0.55 m under the deck, and a
        // body that took the camera's height went through the deck (89.38 to 88.84 m in these
        // records, then 82.1 m). The deck of the level's collision is the floor.
        const TUTORIAL_SEA_M: f32 = 20.0;
        /// The deck's lowest point in the fight's area, m.
        const DECK_LOW_M: f32 = 89.27;
        let replay = parse_replay(include_str!("testdata/run14-ship-fight.txt"));
        let ground = galleon();
        let mut body = TrackedBody::new(replay.start);
        for (record, (input, _)) in replay.records.iter().enumerate() {
            let tracked = body.record(*input, &ground, TUTORIAL_SEA_M);
            let deck = ground
                .floor_at(tracked.position.x, tracked.position.y, tracked.position.z)
                .expect("deck");
            assert!(
                (tracked.position.z - deck).abs() < 1e-3 && deck >= DECK_LOW_M,
                "record {record}: {tracked:?}"
            );
        }
    }

    #[test]
    fn the_run_19_attack_camera_under_the_deck_keeps_the_body_on_the_deck() {
        // Run 19 (2026-09-24), the second sailor fight: while the player attacked, the camera's
        // body went 1.5 m under the deck, past the step-up search from it; the body took the
        // floor under the deck and went down to the sea (89.46 to 50 m in 14 s).
        const TUTORIAL_SEA_M: f32 = 20.0;
        const CAMERA_UNDER_DECK_M: f32 = 1.5;
        const WALK_RECORDS: u32 = 15;
        const ATTACK_RECORDS: u32 = 30;
        /// The back key: south from the start, as run 19 went, where the deck reaches 13 m.
        const BACK: i8 = -1;
        let ground = galleon();
        let start = Position {
            z: ground
                .floor_at(RETAIL_START.x, RETAIL_START.y, RETAIL_START.z)
                .expect("deck"),
            ..RETAIL_START
        };
        let mut body = TrackedBody::new(start);
        let mut client = Character::new(start);
        for record in 0..WALK_RECORDS + ATTACK_RECORDS {
            let attacking = record >= WALK_RECORDS;
            client.step(
                CharacterInput {
                    intent: Intent {
                        forward: BACK,
                        ..Intent::default()
                    },
                    yaw: 0.0,
                    dt_s: DT,
                },
                &ground,
                TUTORIAL_SEA_M,
            );
            let on_deck = client.position();
            assert_eq!(
                ground.floor_at(on_deck.x, on_deck.y, on_deck.z),
                Some(on_deck.z),
                "record {record}: the client walks on the deck"
            );
            let mut seen = on_deck;
            if attacking {
                seen.z -= CAMERA_UNDER_DECK_M;
            }
            let tracked = body.record(
                TrackInput {
                    action_started: record == WALK_RECORDS,
                    ..input(BACK, view_of(seen, BOOM, client.velocity()))
                },
                &ground,
                TUTORIAL_SEA_M,
            );
            assert!(
                (tracked.position.z - client.position().z).abs() < 1e-3,
                "record {record}: {tracked:?} client {:?}",
                client.position()
            );
        }
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

    #[test]
    fn a_swimming_body_does_not_snap_back_to_a_trailing_camera() {
        // Live 2026-09-24: after the sink the player swam forward and the camera
        // trail pulled the body backward each record.
        let swim = swim_floor(TUTORIAL_SEA_M);
        let start = Position {
            x: 793.84,
            y: 337.42,
            z: swim,
        };
        let mut body = TrackedBody::new(start);
        body.drop_at(start);
        let mut at = start;
        for _ in 0..8 {
            at = body
                .record(
                    input(0, view_of(at, BOOM, [0.0; 2])),
                    &TutorialSea,
                    TUTORIAL_SEA_M,
                )
                .position;
        }
        assert!(body.is_swimming() || (at.z - swim).abs() < 1e-3, "{at:?}");
        let behind = Position {
            y: at.y - 4.0,
            ..at
        };
        let tracked = body.record(
            input(1, view_of(behind, BOOM, [0.0; 2])),
            &TutorialSea,
            TUTORIAL_SEA_M,
        );
        assert!(!tracked.resynced, "{tracked:?}");
        assert!(
            tracked.position.y > at.y,
            "the camera pulled it back: {at:?} -> {:?}",
            tracked.position
        );
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
                    view: Some(CameraView::new(
                        Position {
                            x: v[3],
                            y: v[4],
                            z: v[5],
                        },
                        v[1],
                        v[2],
                    )),
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

    #[test]
    fn the_sent_places_of_a_retail_walk_step_like_retails_own() {
        // The place the server sends is the body: it must step like retail's own places
        // step (at most 0.36 m in this replay), never in one snap. With the whole-way
        // snap this replay sent steps of 0.45 m and the live tutorial ship (threshold
        // 0.57 m) sent 0.8 m (live 2026-09-22).
        const OPEN_WORLD_SEA_M: f32 = 40.0;
        /// The largest step of a retail own place in this replay, m, with headroom.
        const RETAIL_STEP_M: f32 = 0.40;
        const MEDIAN_M: f32 = 0.2;
        let replay = parse_replay(include_str!("testdata/retail-open-world-walk.txt"));
        let mut body = TrackedBody::new(replay.start);
        let mut truth = replay.start;
        let mut before_body = replay.start;
        let mut body_steps: Vec<f32> = Vec::new();
        let mut errors: Vec<f32> = Vec::new();
        for (input, place) in &replay.records {
            let tracked = body.record(*input, &replay, OPEN_WORLD_SEA_M);
            truth = place.unwrap_or(truth);
            body_steps.push(tracked.position.horizontal_distance(before_body));
            before_body = tracked.position;
            errors.push(tracked.position.horizontal_distance(truth));
        }
        let largest = body_steps.iter().copied().fold(0.0_f32, f32::max);
        assert!(largest < RETAIL_STEP_M, "largest step {largest} m");
        errors.sort_by(f32::total_cmp);
        let median = errors[errors.len() / 2];
        assert!(median < MEDIAN_M, "median {median} m");
    }

    /// A dodge far from the level's origin with a free boom (live 2026-11-18, a controller
    /// player at (7305, 3083) on the 7 m zoom, its camera's height unchanged: its block presses
    /// freed the boom, the check's boom fell to 0.0, in the roll the body went back and forth and
    /// snapped 1.1 m back after it). The client rolls as the server's own dodge does (`intent` of
    /// each record by whether the dodge key is held); its camera follows it on the boom. Gives
    /// the largest gap between the body and the client's body over the records, and the gap at
    /// rest after the roll.
    fn free_boom_dodge_far_away(intent: impl Fn(bool) -> Intent) -> (f32, f32) {
        const FAR: Position = Position {
            x: 7305.69,
            y: 3083.31,
            z: GROUND_Z,
        };
        const LIVE_BOOM_M: f32 = 7.0;
        const REST_RECORDS: usize = 10;
        const BLOCK_RECORDS: usize = 3;
        const DODGE_KEY_RECORDS: usize = 4;
        const RUN_RECORDS: usize = 25;
        const STOP_RECORDS: usize = 30;
        let mut body = TrackedBody::new(FAR);
        body.set_equip_load(EquipLoad::Fast);
        let mut client = Character::new(FAR);
        client.set_equip_load(EquipLoad::Fast);
        let mut worst_m: f32 = 0.0;
        let records = (0..REST_RECORDS)
            .map(|_| None)
            .chain((0..BLOCK_RECORDS).map(|_| Some(false)))
            .chain((0..DODGE_KEY_RECORDS).map(|_| Some(true)))
            .chain((0..RUN_RECORDS).map(|_| Some(false)))
            .chain((0..STOP_RECORDS).map(|_| None));
        let mut blocking = false;
        for dodge in records {
            let character = CharacterInput {
                intent: dodge.map_or_else(Intent::default, &intent),
                yaw: 0.0,
                dt_s: DT,
            };
            client.step(character, &Flat, SEA);
            let tracked = body.record(
                TrackInput {
                    character,
                    view: Some(view_of(client.position(), LIVE_BOOM_M, client.velocity())),
                    // The block press frees the boom, as the controller's did.
                    boom_key: dodge.is_some() && !blocking,
                    action_started: false,
                },
                &Flat,
                SEA,
            );
            blocking = dodge.is_some();
            worst_m = worst_m.max(tracked.position.horizontal_distance(client.position()));
        }
        (
            worst_m,
            body.position().horizontal_distance(client.position()),
        )
    }

    /// The most a free-boom dodge's body may stand from its client's, m (our choice: the error of
    /// a retail walk, 90th percentile ~1 m, at most half of it).
    const FREE_BOOM_DODGE_GAP_M: f32 = 0.5;
    /// The most it may stand from its client's at rest after the roll, m (our choice).
    const FREE_BOOM_REST_GAP_M: f32 = 0.1;

    #[test]
    fn a_keyboard_dodge_far_from_the_origin_with_a_free_boom_stays_with_its_client() {
        let (worst_m, rest_m) = free_boom_dodge_far_away(|dodge| Intent {
            forward: 1,
            block: true,
            dodge,
            ..Intent::default()
        });
        assert!(worst_m < FREE_BOOM_DODGE_GAP_M, "in the roll {worst_m} m");
        assert!(rest_m < FREE_BOOM_REST_GAP_M, "at rest {rest_m} m");
    }

    #[test]
    fn a_controller_dodge_far_from_the_origin_with_a_free_boom_stays_with_its_client() {
        // The live stick: right -0.156, forward 0.984, with the block held.
        const LIVE_STICK: [f32; 2] = [-0.156, 0.984];
        let (worst_m, rest_m) = free_boom_dodge_far_away(|dodge| {
            Intent {
                block: true,
                dodge,
                ..Intent::default()
            }
            .with_stick(Some(LIVE_STICK))
        });
        assert!(worst_m < FREE_BOOM_DODGE_GAP_M, "in the roll {worst_m} m");
        assert!(rest_m < FREE_BOOM_REST_GAP_M, "at rest {rest_m} m");
    }

    /// One input record of the live pad roll: the camera (x, y, z), the stick, the dodge key and
    /// the block key.
    type LiveRecord = (f32, f32, f32, Option<[f32; 2]>, bool, bool);

    /// A controller player's roll with the block held, far from the level origin on an 8 m
    /// boom (live 2026-11-19, 11:38:01.234 to 02.054, records 2466 to 2490; the view held yaw
    /// 0.6155 and pitch 0.3903; the camera went 4.25 m along the view and its height changed
    /// by 0.07 m, so the boom stayed). The sim took the pad's dodge press as a boom key: the
    /// check's boom fell from 7.93 to 4.85 m, the body went 2.3 m instead of 4.3 m and snapped
    /// 1.1 m back after the roll.
    #[rustfmt::skip]
    const LIVE_PAD_ROLL: [LiveRecord; 25] = [
        (7306.9546, 3083.848, 49.704727, Some([0.0, 0.0625]), false, true),
        (7306.9546, 3083.848, 49.704727, Some([0.046875, 0.8046875]), false, true),
        (7306.9463, 3083.861, 49.704727, Some([0.046875, 0.9921875]), false, true),
        (7306.9204, 3083.9023, 49.704727, Some([-0.0078125, 0.9921875]), true, true),
        (7306.82, 3084.0435, 49.704727, Some([-0.0546875, 0.9921875]), true, true),
        (7306.633, 3084.3018, 49.704727, Some([-0.1328125, 0.984375]), true, true),
        (7306.4175, 3084.6013, 49.704727, Some([-0.1484375, 0.984375]), true, true),
        (7306.317, 3084.7407, 49.704727, Some([-0.1484375, 0.984375]), true, true),
        (7306.097, 3085.0461, 49.704727, Some([-0.15625, 0.984375]), false, true),
        (7305.8804, 3085.3408, 49.704727, Some([-0.1640625, 0.984375]), false, true),
        (7305.7603, 3085.4988, 49.704727, Some([-0.1640625, 0.984375]), false, true),
        (7305.533, 3085.7905, 49.704727, Some([-0.171875, 0.984375]), false, true),
        (7305.4224, 3085.9287, 49.704727, Some([-0.171875, 0.984375]), false, true),
        (7305.189, 3086.2148, 49.687943, Some([-0.1953125, 0.9765625]), false, true),
        (7304.949, 3086.5007, 49.687943, Some([-0.0625, 0.5859375]), false, true),
        (7304.821, 3086.6516, 49.687943, Some([0.0, 0.2578125]), false, true),
        (7304.5796, 3086.9392, 49.687943, None, false, true),
        (7304.4683, 3087.0747, 49.671158, None, false, true),
        (7304.355, 3087.213, 49.671158, None, false, false),
        (7304.355, 3087.213, 49.671158, None, false, false),
        (7304.355, 3087.213, 49.671158, None, false, false),
        (7304.355, 3087.213, 49.654373, None, false, true),
        (7304.355, 3087.213, 49.654373, None, false, true),
        (7304.355, 3087.213, 49.654373, None, false, true),
        (7304.355, 3087.213, 49.63759, None, false, true),
    ];

    /// The live pad roll of [`LIVE_PAD_ROLL`], after `REST_RECORDS` records at rest under its
    /// first camera, with the stick (`pad`) or with the forward key where the stick was tilted.
    /// Gives the largest gap between the body and where the cameras put the client on the held
    /// boom, and the gap at the end.
    fn live_pad_roll(pad: bool) -> (f32, f32) {
        const START: Position = Position {
            x: 7302.314,
            y: 3089.5442,
            z: LIVE_FLOOR_M,
        };
        const LIVE_FLOOR_M: f32 = 44.95;
        const YAW: f32 = 0.615_543_1;
        const PITCH_LIVE: f32 = 0.390_333_6;
        /// The resting boom the check held before the roll (live record 2465).
        const HELD_BOOM_M: f32 = 7.926_7;
        const REST_RECORDS: usize = 10;
        struct LiveFloor;
        impl Ground for LiveFloor {
            fn height_at(&self, _: f32, _: f32) -> Option<f32> {
                Some(LIVE_FLOOR_M)
            }
        }
        let (forward, right) = yaw_axes(YAW);
        let client_of = |camera: Position| Position {
            x: camera.x - SHOULDER_RIGHT_M * right[0] + HELD_BOOM_M * PITCH_LIVE.cos() * forward[0],
            y: camera.y - SHOULDER_RIGHT_M * right[1] + HELD_BOOM_M * PITCH_LIVE.cos() * forward[1],
            z: LIVE_FLOOR_M,
        };
        let mut body = TrackedBody::new(START);
        body.set_equip_load(EquipLoad::Fast);
        let (first_x, first_y, first_z, ..) = LIVE_PAD_ROLL[0];
        let rest: LiveRecord = (first_x, first_y, first_z, None, false, true);
        let mut worst_m: f32 = 0.0;
        let mut gap_m = 0.0;
        for (index, (x, y, z, stick, dodge, block)) in std::iter::repeat_n(rest, REST_RECORDS)
            .chain(LIVE_PAD_ROLL)
            .enumerate()
        {
            let camera = Position { x, y, z };
            let keys = Intent {
                forward: i8::from(!pad && stick.is_some()),
                dodge,
                block,
                ..Intent::default()
            };
            let intent = if pad { keys.with_stick(stick) } else { keys };
            let (intent, yaw) = intent.toward_stick(YAW);
            let tracked = body.record(
                TrackInput {
                    character: CharacterInput {
                        intent,
                        yaw,
                        dt_s: DT,
                    },
                    view: Some(CameraView::new(camera, YAW, PITCH_LIVE)),
                    boom_key: false,
                    action_started: false,
                },
                &LiveFloor,
                SEA,
            );
            let client = client_of(camera);
            gap_m = tracked.position.horizontal_distance(client);
            if index >= REST_RECORDS {
                worst_m = worst_m.max(gap_m);
            }
        }
        (worst_m, gap_m)
    }

    /// The most the live pad roll's body may stand from its client's, m (our choice: the
    /// camera's trail of the roll, `CAMERA_LAG_S` × 8.5 m/s = 0.5 m, with a quarter step of
    /// [`PAD_FOLLOW_SHARE`] behind it, rounded up).
    const LIVE_ROLL_GAP_M: f32 = 1.0;

    // Both bodies step back by one follow step, 0.36 m, when the roll stops: the camera's body
    // stood ahead by the trail of the server's roll speed, which went on for two records after
    // the client's camera stopped (not the snap-back of a shrunk boom). This is still open: a
    // trail that fades over the camera's lag, or one from the camera's own motion, cut the back
    // steps but made the error of other moves larger in the retail replays. So did (2026-11-19)
    // a trail of the body's motion over the last 0.06 s (the retail tutorial's median place
    // error 0.258 to 0.368 m), the mean of its velocities over that time (0.306 m), the same
    // mean in a dodge only (the open world's records more than 0.5 m off 30.03 to 30.04 %, and
    // still a step back of 0.24 m), and a fast roll cut at its 0.5 s marker (a step back of
    // 0.36 m against 0.26 m).

    #[test]
    fn a_controller_s_live_roll_on_a_long_boom_stays_with_its_client() {
        let (worst_m, end_m) = live_pad_roll(true);
        assert!(worst_m < LIVE_ROLL_GAP_M, "in the roll {worst_m} m");
        assert!(end_m < FREE_BOOM_REST_GAP_M, "at the end {end_m} m");
    }

    #[test]
    fn a_keyboard_roll_under_the_live_pad_cameras_stays_with_its_client() {
        let (worst_m, end_m) = live_pad_roll(false);
        assert!(worst_m < LIVE_ROLL_GAP_M, "in the roll {worst_m} m");
        assert!(end_m < FREE_BOOM_REST_GAP_M, "at the end {end_m} m");
    }

    /// The cameras (x, y, z, yaw, pitch) of a controller player at rest after a musket aim,
    /// live 2026-11-22 01:46:39.995 to 01:46:41.065 UTC (records 5370 to 5402): the view tips up,
    /// the camera comes down to the floor at 45.106 m and slides in along the view from 7.54 to
    /// 1.88 m of the still body. The server moved the body 4.4 m.
    const LIVE_CAMERA_TO_FLOOR: [(f32, f32, f32, f32, f32); 33] = [
        (7305.898, 3087.708, 49.72151, 1.1493, 0.3983),
        (7305.9517, 3087.6758, 49.57045, 1.1483, 0.3779),
        (7306.1, 3087.585, 49.10048, 1.1455, 0.3147),
        (7306.1465, 3087.564, 48.932632, 1.1455, 0.2935),
        (7306.1973, 3087.539, 48.747986, 1.1452, 0.2683),
        (7306.2686, 3087.4985, 48.412292, 1.1443, 0.2268),
        (7306.3013, 3087.443, 48.177307, 1.1396, 0.1965),
        (7306.3447, 3087.33, 47.690536, 1.1289, 0.1347),
        (7306.354, 3087.2795, 47.472336, 1.1236, 0.1063),
        (7306.352, 3087.1765, 46.968796, 1.1119, 0.0437),
        (7306.3413, 3087.135, 46.73381, 1.1067, 0.0146),
        (7306.2954, 3087.062, 46.14633, 1.0960, -0.0597),
        (7306.2603, 3087.0466, 45.81064, 1.0923, -0.1008),
        (7306.214, 3087.0479, 45.458145, 1.0896, -0.1447),
        (7306.159, 3087.0566, 45.139236, 1.0874, -0.1855),
        (7304.063, 3088.152, 45.105667, 1.0866, -0.2693),
        (7303.031, 3088.695, 45.105667, 1.0866, -0.3345),
        (7302.6553, 3088.8923, 45.105667, 1.0866, -0.3678),
        (7302.344, 3089.0562, 45.105667, 1.0866, -0.4005),
        (7302.2446, 3089.1084, 45.105667, 1.0866, -0.4123),
        (7302.1475, 3089.1594, 45.12245, 1.0866, -0.4240),
        (7302.0137, 3089.2297, 45.12245, 1.0866, -0.4413),
        (7301.867, 3089.307, 45.12245, 1.0866, -0.4616),
        (7301.551, 3089.4734, 45.12245, 1.0866, -0.5151),
        (7301.2646, 3089.6228, 45.105667, 1.0863, -0.5741),
        (7301.2, 3089.6567, 45.105667, 1.0863, -0.5889),
        (7301.1006, 3089.7092, 45.105667, 1.0863, -0.6135),
        (7301.101, 3089.709, 45.105667, 1.0863, -0.6135),
        (7301.1016, 3089.7087, 45.105667, 1.0863, -0.6135),
        (7301.1016, 3089.7085, 45.105667, 1.0863, -0.6135),
        (7301.102, 3089.7085, 45.105667, 1.0863, -0.6135),
        (7301.102, 3089.7085, 45.105667, 1.0863, -0.6135),
        (7301.102, 3089.7083, 45.105667, 1.0863, -0.6135),
    ];

    /// The flat open ground under the live cameras of [`LIVE_CAMERA_TO_FLOOR`], m.
    const LIVE_CAMERA_FLOOR_M: f32 = 44.929;

    struct LiveCameraFloor;
    impl Ground for LiveCameraFloor {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(LIVE_CAMERA_FLOOR_M)
        }
    }

    /// The most a still body may move under a camera that came down to the floor, m (our
    /// choice: one step on the wire, [`FOLLOW_FROM_M`], with some margin).
    const STILL_UNDER_FLOOR_CAMERA_M: f32 = 0.05;

    #[test]
    fn a_camera_that_comes_down_to_the_floor_leaves_a_still_body_where_it_stands() {
        // The live body; its client held no key and no stick (at rest a controller sends no
        // stick, as the keys none). The aim's end is an action of the client.
        const STILL: Position = Position {
            x: 7299.263,
            y: 3_090.134_5,
            z: LIVE_CAMERA_FLOOR_M,
        };
        const REST_RECORDS: usize = 30;
        let view_of_live = |(x, y, z, yaw, pitch): (f32, f32, f32, f32, f32)| {
            CameraView::new(Position { x, y, z }, yaw, pitch)
        };
        let mut body = TrackedBody::new(STILL);
        let first = view_of_live(LIVE_CAMERA_TO_FLOOR[0]);
        for _ in 0..REST_RECORDS {
            body.record(input(0, first), &LiveCameraFloor, SEA);
        }
        let mut worst_m: f32 = 0.0;
        for (index, camera) in LIVE_CAMERA_TO_FLOOR.into_iter().enumerate() {
            let tracked = body.record(
                TrackInput {
                    action_started: index == 0,
                    ..input(0, view_of_live(camera))
                },
                &LiveCameraFloor,
                SEA,
            );
            worst_m = worst_m.max(tracked.position.horizontal_distance(STILL));
        }
        assert!(
            worst_m < STILL_UNDER_FLOOR_CAMERA_M,
            "the body moved {worst_m} m"
        );
    }

    #[test]
    fn a_camera_on_the_floor_gives_a_walk_nothing_with_keyboard_and_controller() {
        // The view tipped up so that the camera lies on the floor 2 m behind the body: its
        // boom is short, and the body moves by its keys or stick alone, as with no camera.
        const LOOK_UP_PITCH: f32 = -0.5;
        const FLOOR_BOOM_M: f32 = 2.0;
        const WALK_RECORDS: usize = 60;
        const CAMERA_ON_FLOOR_M: f32 = CAMERA_SPHERE_RADIUS_M + CAMERA_NEAR_PLANE_M;
        /// The left stick pushed forward: right at rest, forward 1.
        const STICK_FORWARD: [f32; 2] = [0.0, 1.0];
        let keys = Intent {
            forward: 1,
            ..Intent::default()
        };
        let pad = Intent::default().with_stick(Some(STICK_FORWARD));
        for (device, intent) in [("keyboard", keys), ("controller", pad)] {
            let (intent, yaw) = intent.toward_stick(0.0);
            let character = CharacterInput {
                intent,
                yaw,
                dt_s: DT,
            };
            let mut body = TrackedBody::new(start());
            let mut keys_alone = TrackedBody::new(start());
            for record in 0..WALK_RECORDS {
                let at = body.position();
                let (forward, _) = yaw_axes(0.0);
                let camera = Position {
                    x: at.x - FLOOR_BOOM_M * forward[0],
                    y: at.y - FLOOR_BOOM_M * forward[1],
                    z: GROUND_Z + CAMERA_ON_FLOOR_M,
                };
                let tracked = body.record(
                    TrackInput {
                        character,
                        view: Some(CameraView::new(camera, 0.0, LOOK_UP_PITCH)),
                        boom_key: false,
                        action_started: false,
                    },
                    &Flat,
                    SEA,
                );
                let alone = keys_alone.record(
                    TrackInput {
                        character,
                        view: None,
                        boom_key: false,
                        action_started: false,
                    },
                    &Flat,
                    SEA,
                );
                assert!(!tracked.resynced, "{device} record {record}");
                assert!(
                    tracked.position.horizontal_distance(alone.position) < CLOSE,
                    "{device} record {record}: {:?} against {:?}",
                    tracked.position,
                    alone.position
                );
            }
        }
    }

    /// A player on the Monarch's Bluffs beach, far from the level origin, walks up to a target
    /// with `approach`, lets go of its keys or stick and attacks; the attack's lunge carries the
    /// client 2.5 m along the view in 0.2 s with no key or stick held (retail `12-4_crewmates`
    /// from 84.71 s; the replay's body there stood up to 1.71 m from retail's place). Its camera
    /// follows it on a 3 m boom. Gives the largest gap between the body and the client's body in
    /// the lunge and after it.
    fn keyless_lunge_far_away(approach: Intent) -> f32 {
        const BEACH: Position = Position {
            x: 7196.37,
            y: 3134.77,
            z: GROUND_Z,
        };
        const RETAIL_BOOM_M: f32 = 3.0;
        const WALK_RECORDS: usize = 30;
        const STOP_RECORDS: usize = 15;
        const LUNGE_M: f32 = 2.5;
        const LUNGE_RECORDS: u8 = 6;
        const AFTER_RECORDS: usize = 30;
        let mut body = TrackedBody::new(BEACH);
        let mut client = Character::new(BEACH);
        let still = CharacterInput {
            intent: Intent::default(),
            yaw: 0.0,
            dt_s: DT,
        };
        let walk = CharacterInput {
            intent: approach,
            ..still
        };
        let record_of = |character, view, action_started| TrackInput {
            character,
            view: Some(view),
            boom_key: false,
            action_started,
        };
        for character in (0..WALK_RECORDS)
            .map(|_| walk)
            .chain((0..STOP_RECORDS).map(|_| still))
        {
            client.step(character, &Flat, SEA);
            let view = view_of(client.position(), RETAIL_BOOM_M, client.velocity());
            body.record(record_of(character, view, false), &Flat, SEA);
        }
        let lunge_step_m = LUNGE_M / f32::from(LUNGE_RECORDS);
        let lunge_mps = [0.0, lunge_step_m / DT];
        let mut at = client.position();
        let mut worst_m: f32 = 0.0;
        for record in 0..LUNGE_RECORDS {
            at.y += lunge_step_m;
            let view = view_of(at, RETAIL_BOOM_M, lunge_mps);
            let tracked = body.record(record_of(still, view, record == 0), &Flat, SEA);
            worst_m = worst_m.max(tracked.position.horizontal_distance(at));
        }
        for _ in 0..AFTER_RECORDS {
            let view = view_of(at, RETAIL_BOOM_M, [0.0; 2]);
            let tracked = body.record(record_of(still, view, false), &Flat, SEA);
            worst_m = worst_m.max(tracked.position.horizontal_distance(at));
        }
        worst_m
    }

    /// The most a key-less lunge's body may stand from its client's, m (our choice): the
    /// camera's trail of the lunge, which the server's still body does not give
    /// (`CAMERA_LAG_S` × 12.5 m/s = 0.75 m), and one [`FOLLOW_STEP_M`], rounded up. Without the
    /// first-camera recovery the body stood 1.75 m from its client.
    const KEYLESS_LUNGE_GAP_M: f32 = 1.2;

    #[test]
    fn a_keyboard_player_s_keyless_lunge_far_from_the_origin_keeps_the_body_with_its_client() {
        let worst_m = keyless_lunge_far_away(Intent {
            forward: 1,
            ..Intent::default()
        });
        assert!(worst_m < KEYLESS_LUNGE_GAP_M, "{worst_m} m");
    }

    #[test]
    fn a_controller_player_s_keyless_lunge_far_from_the_origin_keeps_the_body_with_its_client() {
        // A stick tilted forward and a little left, as the live controller player's.
        const STICK: [f32; 2] = [-0.156, 0.984];
        let worst_m = keyless_lunge_far_away(Intent::default().with_stick(Some(STICK)));
        assert!(worst_m < KEYLESS_LUNGE_GAP_M, "{worst_m} m");
    }

    /// The live float of 2026-11-19 (live-2026-11-19e, counters 8469 to 8472): the client stood
    /// at rest with its 7.0 m zoom and a pitch of 0.445 rad, the check held a boom of 1.13 m, and
    /// the camera put the body 2.6 m above the floor. Gives the client's body, the body the check
    /// placed (1.13 m along the view from the camera's base) and the client's camera.
    fn short_boom_at_rest() -> (Position, Position, CameraView) {
        const PITCH: f32 = 0.445;
        const CLIENT_BOOM_M: f32 = 7.0;
        const HELD_BOOM_M: f32 = 1.13;
        let client = start();
        let (forward, right) = yaw_axes(0.0);
        let level = |boom: f32| boom * PITCH.cos();
        let base = [
            client.x - level(CLIENT_BOOM_M) * forward[0],
            client.y - level(CLIENT_BOOM_M) * forward[1],
        ];
        let camera = Position {
            x: base[0] + SHOULDER_RIGHT_M * right[0],
            y: base[1] + SHOULDER_RIGHT_M * right[1],
            z: client.z + CAMERA_RISE_M + CLIENT_BOOM_M * PITCH.sin(),
        };
        let held = Position {
            x: base[0] + level(HELD_BOOM_M) * forward[0],
            y: base[1] + level(HELD_BOOM_M) * forward[1],
            z: GROUND_Z,
        };
        (client, held, CameraView::new(camera, 0.0, PITCH))
    }

    #[test]
    fn a_resting_camera_too_high_over_the_floor_lengthens_the_boom_to_it() {
        /// Records at rest.
        const RECORDS: usize = 3;
        const WITHIN_M: f32 = 0.05;
        let (client, held, view) = short_boom_at_rest();
        let mut body = TrackedBody::new(held);
        for _ in 0..RECORDS {
            body.record(input(0, view), &Flat, SEA);
        }
        let at = body.position();
        assert!(at.horizontal_distance(client) < WITHIN_M, "{at:?}");
        assert!((at.z - GROUND_Z).abs() < WITHIN_M, "{at:?}");
        let boom = body.boom_m().expect("a boom");
        assert!((boom - 7.0).abs() < WITHIN_M, "{boom}");
    }

    #[test]
    fn a_short_boom_from_rest_does_not_lift_the_walking_body() {
        // Live: from the first movement key the body took its camera's height, 2.6 m up, as a
        // floor the level lacks, and stood there for minutes.
        const RESTING_RECORDS: usize = 3;
        const WALKING_RECORDS: usize = 30;
        const FLOOR_WITHIN_M: f32 = 0.1;
        let (_, held, view) = short_boom_at_rest();
        let mut body = TrackedBody::new(held);
        for _ in 0..RESTING_RECORDS {
            body.record(input(0, view), &Flat, SEA);
        }
        let mut walk = input(0, view);
        walk.character.intent.right = 1;
        for _ in 0..WALKING_RECORDS {
            let tracked = body.record(walk, &Flat, SEA);
            assert!(
                (tracked.position.z - GROUND_Z).abs() < FLOOR_WITHIN_M,
                "{:?}",
                tracked.position
            );
        }
    }
    /// A client on its zoom whose camera meets the level at rest and then walks off while the
    /// boom glides back out with no boom key (retail towns session 2026-11-19, 21:08:46 to
    /// 21:08:54 against the client's own trace: the boom 7.2 m at rest, 0.68 m against the
    /// level, back to 7.4 m in 2 s, at a pitch of 0.62 rad). Gives the highest the body stood
    /// above the flat ground in the walk.
    fn walk_while_the_boom_glides_out() -> f32 {
        const PITCH_STAIRS: f32 = 0.62;
        const ZOOM_M: f32 = 7.2;
        const MET_M: f32 = 0.68;
        const GLIDE_M_PER_RECORD: f32 = (7.4 - MET_M) / 60.0;
        const REST_RECORDS: usize = 30;
        const WALK_RECORDS: usize = 90;
        let camera_of = |body: Position, boom: f32, velocity: [f32; 2]| {
            let (forward, right) = yaw_axes(0.0);
            let level = boom * PITCH_STAIRS.cos();
            CameraView::new(
                Position {
                    x: body.x - level * forward[0] + SHOULDER_RIGHT_M * right[0]
                        - CAMERA_LAG_S * velocity[0],
                    y: body.y - level * forward[1] + SHOULDER_RIGHT_M * right[1]
                        - CAMERA_LAG_S * velocity[1],
                    z: body.z + CAMERA_RISE_M + boom * PITCH_STAIRS.sin(),
                },
                0.0,
                PITCH_STAIRS,
            )
        };
        let mut body = TrackedBody::new(start());
        let mut client = Character::new(start());
        for boom in std::iter::repeat_n(ZOOM_M, REST_RECORDS)
            .chain(std::iter::repeat_n(MET_M, REST_RECORDS))
        {
            body.record(input(0, camera_of(start(), boom, [0.0; 2])), &Flat, SEA);
        }
        let mut highest_m: f32 = 0.0;
        let mut boom = MET_M;
        for _ in 0..WALK_RECORDS {
            let walk = input(1, camera_of(start(), boom, [0.0; 2]));
            client.step(walk.character, &Flat, SEA);
            boom = (boom + GLIDE_M_PER_RECORD).min(ZOOM_M);
            let view = camera_of(client.position(), boom, client.velocity());
            let tracked = body.record(
                TrackInput {
                    view: Some(view),
                    ..walk
                },
                &Flat,
                SEA,
            );
            highest_m = highest_m.max(tracked.position.z - GROUND_Z);
        }
        highest_m
    }

    #[test]
    fn a_walk_under_a_boom_shorter_than_the_zoom_stands_on_no_floor_the_level_lacks() {
        // Before, the body stood on its camera's height as a floor the level lacks, up to 4.4 m
        // above the client in the retail session.
        const FLOOR_WITHIN_M: f32 = 0.1;
        let highest_m = walk_while_the_boom_glides_out();
        assert!(highest_m < FLOOR_WITHIN_M, "{highest_m} m above the floor");
    }
}
