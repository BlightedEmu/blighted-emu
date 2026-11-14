// SPDX-License-Identifier: AGPL-3.0-or-later
//! The slayer state of a player (slayer note, 2026-09-18; movement study 2026-09-20; run-15 lock
//! and aim study 2026-09-22): the action it plays, which the client and the other players see in
//! the ALC children 13 to 24 and 48, and the time into that action's sequence (children 2 and 3).
//!
//! The client starts some actions itself and names each one in row 1 of its input
//! ([`SlayerInput`]); the server echoes it in the next own state, started in the state numbered
//! with that record's counter, then adds its follow-up for some of them. Every other action is
//! the server's (Observed, retail newchar capture 2026-09-18; the client waits for them): the
//! light attack at the release of a tap of the attack key, the charge after a hold, the dodge,
//! the jump, the block and the aim of the right-click, the weapon draw and the movement states.
//! Runs 13 and 15 sent none of the attack starts: their clients started no attack at all (37 and
//! 103 taps, every one shorter than the hold; retail's server started the first attack of each
//! chain at the release, 463 at record 0x6bd after the press at 0x6ba). The sequences come from
//! the player sequence table ([`PlayerSequences`]): by the weapon's stance and, for an attack,
//! by the attack that runs ([`SlayerTrack::attack_start`]).
//!
//! The sequence time is the sum of the input records' frame times since the record that started
//! the state, that record's own included ([`SequenceClock`] has the wire form).

use nw_content::player_sequences::{
    AbilityPlay, AttackPress, AttackSlot, Direction, FirstPlay, PlayerSequences, RunningAttack,
    SlotsChange, StanceSequences, StatePlay,
};
use nw_messages::{
    RemoteUpper, SecondSlayerState, SequenceClock, SequenceTime, SlayerInput, SlayerState,
    SlotChange, UpperSlots,
};
use nw_sim_core::{Intent, Posture};

use super::attack::counter_is_newer;

/// The value of a slot's state, sequence or marker that clears it (retail `19=00 21=00 48=00`).
const CLEARED_SLOT: u64 = 0;

/// How many own states carry one slayer change (Observed: retail sent a follow-up in the state
/// that started it and the next one, 15c1 and 15c2 at 206528 to 206577 ms).
const ECHO_STATES: u8 = 2;
/// Input records an attack runs after its start: a press of the attack key in that time
/// starts no new chain (Observed, attack sequence study 2026-11-11: with 34 records, 18,098 of
/// 18,589 retail presses of the first-slot weapons got a press start or none as this rule
/// says; Inferred for the second-slot weapons).
const ATTACK_RUN_RECORDS: u16 = 34;
/// Input records from the last start of a first-slot chain to its end (Observed: retail's
/// `48=0` came 932 ms, 28 records, after the last attack start at the median).
const CHAIN_END_RECORDS: u16 = 28;
/// The second slot's state at the end of a cast (Observed: `16=45 18=0 48=0`) and of a hit
/// reaction ([`reaction`]).
const CAST_END_STATE: u64 = 45;

/// The slayer states the server starts for the player's own movement with the weapon sheathed,
/// with the sequence each transition took most often in the retail newchar capture (the server's
/// slayer changes against the input keys of the records before and at them). The sequence of one
/// transition varies with what the capture does not show (the foot in front, the turn of the
/// camera); Unknown: the client's slayer script that picks it. A drawn weapon and a held
/// right-click take the stance's own ([`MoveSet::of`]).
mod movement {
    /// `slayerStateId` of a move that starts from standing still (`13=1a`).
    pub(super) const START_STATE: u64 = 0x1a;
    /// `slayerStateId` of the moves that follow and of a stop (`13=1b`).
    pub(super) const MOVE_STATE: u64 = 0x1b;
    /// `slayerStateId` of standing still (`13=1f`, the first own state's).
    pub(super) const IDLE_STATE: u64 = 0x1f;
    /// Standing still, a key forward or to the side: `aa2b`, 29 of the 57 forward starts and 11
    /// of the 21 side starts.
    pub(super) const START_SEQUENCE: u64 = 2794;
    /// Standing still, the back key: state `1b` with `9814`, 6 of the 16 back starts.
    pub(super) const BACK_START_SEQUENCE: u64 = 1304;
    /// Moving, the keys let go: `bd15`, 29 of the 96 stops of a forward move.
    pub(super) const STOP_SEQUENCE: u64 = 1405;
    /// Stopping, a key again: `b625`, 21 of the 51 forward presses during a stop.
    pub(super) const RESTART_SEQUENCE: u64 = 2422;
    /// Stopped: `b910`, 34 of the 106 idle states after a stop.
    pub(super) const IDLE_SEQUENCE: u64 = 1081;
    /// Input records from the stop to the idle state: the length of the stop sequence (9 of the
    /// 14 `bd15` stops that ended in an idle state did after 39 records).
    pub(super) const STOP_RECORDS: u16 = 39;
}

/// The slayer states of a crouching or prone player (retail, the four `walk_running_crouch_prone`
/// and `equip_and_prone` captures, weapon sheathed, 15 presses of the crouch and prone toggles;
/// Inferred for a drawn weapon, which no capture has in these postures).
///
/// A change of posture is a new sequence of the state that goes on (child 15 alone), 33 to 54 ms
/// after the toggle. The sequence of the new posture's stand or move follows it, alone too, unless
/// a move started or stopped in between. Other players got no state for these postures before:
/// they saw a crouching or prone player stand (live 2026-11-09).
mod posture {
    use super::{MoveSet, Posture, movement};

    /// Crouching: a start `13=1a 15=b61c` (forward 2 of 2, back 1 of 1), a stop `13=1b 15=ac30`
    /// (3 of 3), a key during the stop `15=8511` (1 of 1), still `13=1f 15=9e3e` 10 records
    /// after the stop (2 of 2).
    const CROUCH: MoveSet = MoveSet {
        start: (movement::START_STATE, 1846),
        back_start: (movement::START_STATE, 1846),
        stop: (movement::MOVE_STATE, 3116),
        restart: (movement::MOVE_STATE, 1093),
        idle: (movement::IDLE_STATE, 3998),
        moving: 1846,
        stop_records: STOP_RECORDS,
    };
    /// Prone: a start `13=1a 15=971c` (5 of 5, every direction; after a stop too, 1 of 1), a stop
    /// `13=1b 15=8c15` (6 of 6), still `13=1f 15=bc3b` 10 records after the stop (4 of 5).
    const PRONE: MoveSet = MoveSet {
        start: (movement::START_STATE, 1815),
        back_start: (movement::START_STATE, 1815),
        stop: (movement::MOVE_STATE, 1356),
        restart: (movement::START_STATE, 1815),
        idle: (movement::IDLE_STATE, 3836),
        moving: 1815,
        stop_records: STOP_RECORDS,
    };
    /// Input records from a crouching or prone stop to its idle state.
    const STOP_RECORDS: u16 = 10;
    /// Standing still to a crouch: `8913` (2 of 2).
    const STAND_TO_CROUCH: u64 = 1225;
    /// Moving to a crouch: `a51b` (1 of 1).
    const MOVING_TO_CROUCH: u64 = 1765;
    /// A crouch to standing: `a13c` (2 of 2).
    const CROUCH_TO_STAND: u64 = 3873;
    /// Standing still to prone: `bf36` (1 of 1).
    const STAND_TO_PRONE: u64 = 3519;
    /// Moving to prone: `931c` (1 of 1).
    const MOVING_TO_PRONE: u64 = 1811;
    /// A crouch to prone: `912f` (2 of 2).
    const CROUCH_TO_PRONE: u64 = 3025;
    /// Prone to standing: `a03c` (3 of 3).
    const PRONE_TO_STAND: u64 = 3872;
    /// Prone to a crouch: `b93c` (1 of 1).
    const PRONE_TO_CROUCH: u64 = 3897;
    /// Input records from most toggles to the new posture's stand or move sequence (517 to
    /// 554 ms in 8 of the changes).
    const SETTLE_RECORDS: u16 = 16;
    /// The same from a toggle to prone while moving (769 ms, 1 of 1).
    const MOVING_TO_PRONE_SETTLE_RECORDS: u16 = 23;
    /// The same from prone to a crouch (867 ms, 1 of 1).
    const PRONE_TO_CROUCH_SETTLE_RECORDS: u16 = 26;
    /// Input records from a toggle to prone while a start waits: a movement key in that time
    /// starts the prone move at their end (2 of 2: `13=1a 15=971c` 20 records after the toggle,
    /// with the key down from the first or the seventh record after it). A key during a change
    /// to a crouch starts its move at once (1 of 1, 6 records after the toggle).
    pub(super) const PRONE_HOLD_RECORDS: u16 = 20;
    /// Input records from a toggle to the record whose slayer state knows the new posture: the
    /// body takes the toggle after the slayer state read the record.
    pub(super) const SEEN_AFTER_RECORDS: u16 = 1;

    /// The movement of a crouching or prone player; `None` for a standing one.
    pub(super) const fn moves(posture: Posture) -> Option<MoveSet> {
        match posture {
            Posture::Stand => None,
            Posture::Crouch => Some(CROUCH),
            Posture::Prone => Some(PRONE),
        }
    }

    /// The sequence that starts at a change from posture `from` to `to` of a player that is
    /// `moving` or not, and the input records from the toggle to the sequence of the new
    /// posture's stand or move. No sequence: retail sent none when a moving player stood up
    /// from a crouch (1 of 1), and the others must still see it stand, so the stand's move
    /// goes at once (Inferred). Without a sample of its own, a moving change takes the still
    /// one's sequence (Inferred), and a crouch to prone settles when its hold ends (Inferred).
    pub(super) const fn change(from: Posture, to: Posture, moving: bool) -> (Option<u64>, u16) {
        match (from, to, moving) {
            (Posture::Stand, Posture::Crouch, false) => (Some(STAND_TO_CROUCH), SETTLE_RECORDS),
            (Posture::Stand, Posture::Crouch, true) => (Some(MOVING_TO_CROUCH), SETTLE_RECORDS),
            (Posture::Crouch, Posture::Stand, false) => (Some(CROUCH_TO_STAND), SETTLE_RECORDS),
            (Posture::Stand, Posture::Prone, false) => (Some(STAND_TO_PRONE), SETTLE_RECORDS),
            (Posture::Stand, Posture::Prone, true) => {
                (Some(MOVING_TO_PRONE), MOVING_TO_PRONE_SETTLE_RECORDS)
            }
            (Posture::Crouch, Posture::Prone, _) => (Some(CROUCH_TO_PRONE), PRONE_HOLD_RECORDS),
            (Posture::Prone, Posture::Stand, _) => (Some(PRONE_TO_STAND), SETTLE_RECORDS),
            (Posture::Prone, Posture::Crouch, _) => {
                (Some(PRONE_TO_CROUCH), PRONE_TO_CROUCH_SETTLE_RECORDS)
            }
            (Posture::Crouch, Posture::Stand, true)
            | (Posture::Stand, Posture::Stand, _)
            | (Posture::Crouch, Posture::Crouch, _)
            | (Posture::Prone, Posture::Prone, _) => (None, 0),
        }
    }
}

/// The second slot of a weapon put away (retail, the first own slayer change after the sheathe
/// key in 60 open world ledgers and the own captures: `16=2c 18=9b2c` in all 8 changes that
/// carried a second slot, with the sheathed stand or move in the first slot).
mod sheathe {
    /// `slayerStateId` of the second slot (`16=2c`), the draw's too.
    pub(super) const STATE: u64 = 0x2c;
    /// Its sequence: `9b2c`.
    pub(super) const SEQUENCE: u64 = 2843;
}

/// The slayer state of a player that a mob pulls (retail newchar 2026-09-12, Thorpe's first
/// grab pull at 456719 ms: `13=04 14=3163 15=8a37 16=00 17=3163` in the state numbered `3163`,
/// the pull's places in the next states, then `13=1f 15=b910`, standing still, at 457852 ms).
/// Without it the client kept moving by its own keys and dropped the pull (live 2026-09-26: the
/// body stood 7 m from its client after the first pull).
mod pull {
    /// `slayerStateId` of being pulled (`13=04`).
    pub(super) const STATE: u64 = 0x04;
    /// Its sequence: `8a37`.
    pub(super) const SEQUENCE: u64 = 3530;
    /// From the pull's state to the state that stands the player still again.
    pub(super) const LENGTH_MS: u64 = 1_133;
}

/// The hit reaction of a player (Observed, all 3,237 own reaction starts of 310 capture ledgers,
/// `local-data/scratch-work/hitreact/mined.tsv`; newchar 2026-09-18, the great sword's `2960`
/// from 1792695974821): the server starts the reaction state in the own state after the hit,
/// `13=04 14=<n> 15=<sequence> 16=00 17=<n>`, with `18=00` when the second slot played, `19=00
/// 20=<n> 21=00` when the third did, `22=00 23=<n> 24=00` when the fourth did (the block) and
/// `48=00` when the slayer marker was set (1,543 of the starts carried only 48 more, 848 none).
/// The client names the state in row 1 of its next record (`[2960, 0xffff0004, 0]`) and sends
/// no row 1 while it reacts: the server does not echo it. Its keys start nothing until the
/// reaction lets them (`hit_reaction::reaction_times`); the end
/// is the second slot's end state alone, `16=2d 17=<n>` (newchar: 17 records after the start,
/// `17=1e13`), and the state after it stands the player still or moves it again (1,689 of the
/// starts after an end came 31 to 36 ms after it). The grab pull ([`pull`]) is one of these
/// reactions.
mod reaction {
    /// Records from the end that stand the player still unless a key moves it: one.
    pub(super) const STOP_RECORDS: u16 = 1;
}

/// The upper-body hit reaction (`UpperBodyReaction`, an additive shake that holds nothing) plays
/// in the fourth slot. Observed (newchar 2026-09-18, the own state after each 2218 that started
/// no full-body reaction, probe `blighted-emu-tools/scratch-work/combat/hitreact` `flinch`): the
/// start `22=20 23=<n> 24=<sequence>` (`b533`, 3317, after the level 0 hits on the sword at
/// 94.211, 97.912, 202.556, 203.489 and 235.961 s; `aa1d`, 1898, after the blocked great sword
/// hits at 414.090 and 421.256 s), a hit while it plays starts it again (`23=<n>` alone, the
/// other children unchanged, 414.622 s), and the end `22=00 23=<n> 24=00` a record before its
/// fragment is over (`unarmed_r0_f`, 567 ms: 16 records after the start, 94.235 to 94.803 and
/// 97.952 to 98.484 s).
mod upper_reaction {
    /// The fourth slot's state while it plays (`22=20`).
    pub(super) const STATE: u64 = 0x20;
}

/// The kneel of a fast travel's channel and of an accepted unstuck's wait: the unstuck's state and
/// sequence (`13=19 15=b247`) for both.
///
/// - Unstuck (2026-11-08, the first slayer change after each of the 56 retail 3980 of `captures/`
///   (`own/` and `ow captures extracted/`): `13=19 15=b247`, with the interactor off, in 38 (the others:
///   the state that went on, a fall or a swim). At the move, standing still `13=1f 15=b910` with
///   the interactor on, in 26 of the 56 (most others: a fall `13=0f` or a swim). Without the
///   stand the client kept the kneel's pose after the move (live 2026-11-08: arms out, stiff
///   legs, the weapon pose until a jump). It plays live.
/// - Fast travel: retail sends sequence `05` (`14=3fcc 15=05`, the sequence alone in 466 of 487
///   starts of a 3713, with `13=1f` in 21; probe `ftafter`). That kneel did not play live in
///   either form (2026-11-07, and 2026-11-08 three times, from standing still, with the channel
///   flag and the interactor before it in the same bundle as retail). The user's call
///   (2026-11-08): the fast travel kneel is the same animation as the unstuck's, so the sim
///   sends the unstuck's for both.
mod kneel {
    /// `slayerStateId` of the kneel (`13=19`).
    pub(super) const STATE: u64 = 0x19;
    /// Its sequence: `b247`.
    pub(super) const SEQUENCE: u64 = 4594;
}

/// The slayer states the server sends when the player falls or swims (retail newchar 2026-09-18,
/// capture check 2026-09-24): `13=0d 15=9d3c` at the fall, `13=06 15=a021` once at the water
/// entry, then only the sequence (child 15, no 13 and no 14) while the player swims: `b621`
/// 1568 ms after the entry while it floats, `a52e` 31 to 43 ms after each forward key press,
/// `972e` and `a52e` in turn while the key stays down, and `b621` 34 ms after the key goes up.
/// None of the client's input records in the water named an action: the server sends them.
mod swim {
    /// `slayerStateId` of swimming (`13=06`).
    pub(super) const STATE: u64 = 0x06;
    /// `slayerStateId` of falling (`13=0d`).
    pub(super) const FALL_STATE: u64 = 0x0d;
    /// The water entry: `a021`.
    pub(super) const IDLE_SEQUENCE: u64 = 2144;
    /// Floating with no key: `b621`.
    pub(super) const FLOAT_SEQUENCE: u64 = 2166;
    /// The stroke at a key press, and every second one while the key stays down: `a52e`.
    pub(super) const STROKE_SEQUENCE: u64 = 2981;
    /// The stroke between two `a52e` while the key stays down: `972e`.
    pub(super) const NEXT_STROKE_SEQUENCE: u64 = 2967;
    /// Seconds from the entry to the float when no key is down (one entry: 815911 to 817479 ms).
    pub(super) const ENTRY_S: f32 = 1.568;
    /// Seconds of `a52e` before `972e` while the key stays down (median of 7, 0.669 to 1.502 s;
    /// Unknown: what makes it vary).
    pub(super) const STROKE_S: f32 = 1.098;
    /// Seconds of `972e` before `a52e` (median of 6, 1.230 to 1.283 s).
    pub(super) const NEXT_STROKE_S: f32 = 1.249;
    /// Falling: `9d3c` (best guess: retail started five different fall sequences and the rule
    /// that picks one is Unknown; this is one of them).
    pub(super) const FALL_SEQUENCE: u64 = 3869;
}

/// The second slayer slot while the inventory is open (retail newchar 2026-09-18, capture check
/// 2026-09-24, 5 of 5 opens): `16=23 18=962b` 30 to 40 ms after the client's open, `18=890a`
/// 1536 to 1568 ms later, `18=8b2e` 32 to 43 ms after its close, and `16=2d 18=00` 35 to 40 ms
/// after the second close the client sends 500 to 750 ms later. A miss of this slot rolls the
/// client's prediction back and closes the inventory (`FUN_1421ae2c0` / `FUN_14486f2a0`; live
/// 2026-09-24 without it: every open closed again 20 to 60 ms later). In 2 of the 5 opens the
/// first sequence was `a709` and `962b` followed 330 to 370 ms later with the idle state; Unknown:
/// the rule, so the open always plays `962b`.
mod loadout {
    /// `16=23`.
    pub(super) const OPEN_STATE: u64 = 0x23;
    /// `16=2d`.
    pub(super) const CLOSED_STATE: u64 = 0x2d;
    /// `962b`.
    pub(super) const OPEN_SEQUENCE: u64 = 2774;
    /// `890a`.
    pub(super) const LOOP_SEQUENCE: u64 = 649;
    /// `8b2e`.
    pub(super) const CLOSE_SEQUENCE: u64 = 2955;
    /// Time from the open to the loop sequence, ms (median of 5).
    pub(super) const LOOP_MS: u64 = 1_552;
}

/// The movement states and sequences of one moment: the sheathed ones, the stance's drawn ones,
/// its block and aim walk while the right-click is held, or the posture's while the player
/// crouches or lies prone ([`posture`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MoveSet {
    start: (u64, u64),
    back_start: (u64, u64),
    stop: (u64, u64),
    restart: (u64, u64),
    idle: (u64, u64),
    /// The sequence of a move that goes on.
    moving: u64,
    /// Input records from a stop to the idle state.
    stop_records: u16,
}

impl MoveSet {
    /// The sheathed movement of the newchar capture.
    const SHEATHED: Self = Self {
        start: (movement::START_STATE, movement::START_SEQUENCE),
        back_start: (movement::MOVE_STATE, movement::BACK_START_SEQUENCE),
        stop: (movement::MOVE_STATE, movement::STOP_SEQUENCE),
        restart: (movement::MOVE_STATE, movement::RESTART_SEQUENCE),
        idle: (movement::IDLE_STATE, movement::IDLE_SEQUENCE),
        moving: movement::RESTART_SEQUENCE,
        stop_records: movement::STOP_RECORDS,
    };

    /// The movement of a player in `posture`: the posture's when it crouches or lies prone,
    /// else that of `stance` (the weapon `drawn` or not) with the right-click held or not.
    /// A value the table does not have keeps the sheathed one (best guess: the table's drawn
    /// moves are the most common non-sheathed sequence of each transition per stance).
    fn of(stance: Option<&StanceSequences>, drawn: bool, alt: bool, posture: Posture) -> Self {
        if let Some(set) = posture::moves(posture) {
            return set;
        }
        let mut set = Self::SHEATHED;
        let Some(stance) = stance.filter(|_| drawn) else {
            return set;
        };
        let pair = |play: StatePlay| (play.state, play.sequence);
        if let Some(moves) = stance.drawn_moves {
            set.start = moves.start.map_or(set.start, pair);
            set.back_start = moves.back_start.map_or(set.back_start, pair);
            set.stop.1 = moves.stop.unwrap_or(set.stop.1);
            set.restart.1 = moves.restart.unwrap_or(set.restart.1);
            set.idle.1 = moves.idle.unwrap_or(set.idle.1);
        }
        if alt && let Some(walk) = stance.alt_moves {
            if let Some(moving) = walk.moving.map(pair) {
                set.start = moving;
                set.back_start = moving;
                set.stop = moving;
                set.restart = moving;
            }
            set.idle = walk.idle.map_or(set.idle, pair);
        }
        set.moving = set.restart.1;
        set
    }
}

/// Where the player's own movement is, for its movement slayer states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Locomotion {
    /// Standing still (the first own state's idle state).
    #[default]
    Idle,
    /// Moving by its keys.
    Moving,
    /// Its keys let go: records left until it stands still.
    Stopping(u16),
}

impl Locomotion {
    /// The next movement after a record with `intent`, and the slayer state and sequence the
    /// server starts in that record, when it starts one.
    fn step(self, intent: Intent, set: MoveSet) -> (Self, Option<(u64, u64)>) {
        let moving = intent.forward != 0 || intent.right != 0;
        match (self, moving) {
            (Self::Idle, true) => {
                let start = if intent.forward < 0 {
                    set.back_start
                } else {
                    set.start
                };
                (Self::Moving, Some(start))
            }
            (Self::Moving, false) => (Self::Stopping(set.stop_records), Some(set.stop)),
            (Self::Stopping(_), true) => (Self::Moving, Some(set.restart)),
            (Self::Stopping(left), false) => match left.saturating_sub(1) {
                0 => (Self::Idle, Some(set.idle)),
                left => (Self::Stopping(left), None),
            },
            (Self::Idle, false) | (Self::Moving, true) => (self, None),
        }
    }

    /// The state the movement plays now in `set`: sent again when the set changes.
    const fn current(self, set: MoveSet) -> (u64, u64) {
        match self {
            Self::Idle => set.idle,
            Self::Moving => (set.restart.0, set.moving),
            Self::Stopping(_) => set.stop,
        }
    }
}

/// Where the player's swim is, for its swim sequences ([`swim`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Swim {
    /// Out of the water.
    #[default]
    Out,
    /// Came into the water: seconds since, while no key is down.
    Entered(f32),
    /// Floating with no key down.
    Floating,
    /// Swimming by its keys: the stroke sequence it plays, and seconds into it.
    Stroking(u64, f32),
}

impl Swim {
    /// The next swim after a record in the water with `moving` keys and frame time `frame_s`,
    /// and the sequence the server sends in that record, when it sends one.
    fn step(self, moving: bool, frame_s: f32) -> (Self, Option<u64>) {
        match (self, moving) {
            (Self::Out | Self::Entered(_) | Self::Floating, true) => (
                Self::Stroking(swim::STROKE_SEQUENCE, 0.0),
                Some(swim::STROKE_SEQUENCE),
            ),
            (Self::Stroking(..), false) => (Self::Floating, Some(swim::FLOAT_SEQUENCE)),
            (Self::Entered(since_s), false) => {
                let since_s = since_s + frame_s;
                if since_s >= swim::ENTRY_S {
                    (Self::Floating, Some(swim::FLOAT_SEQUENCE))
                } else {
                    (Self::Entered(since_s), None)
                }
            }
            (Self::Stroking(sequence, into_s), true) => {
                let into_s = into_s + frame_s;
                let (length_s, next) = if sequence == swim::STROKE_SEQUENCE {
                    (swim::STROKE_S, swim::NEXT_STROKE_SEQUENCE)
                } else {
                    (swim::NEXT_STROKE_S, swim::STROKE_SEQUENCE)
                };
                if into_s >= length_s {
                    (Self::Stroking(next, 0.0), Some(next))
                } else {
                    (Self::Stroking(sequence, into_s), None)
                }
            }
            (Self::Out | Self::Floating, false) => (self, None),
        }
    }
}

/// The inventory's second slayer slot ([`loadout`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LoadoutPlay {
    /// No inventory slot.
    #[default]
    Closed,
    /// The open sequence, until [`loadout::LOOP_MS`].
    Opening {
        /// The ALC state that started it.
        started: u16,
        /// When the client opened it.
        opened_ms: u64,
        /// Own states left that carry it.
        left: u8,
    },
    /// The loop sequence while it stays open.
    Looping {
        /// The ALC state that started the open.
        started: u16,
        /// Own states left that carry it.
        left: u8,
    },
    /// The close sequence: own states left that carry it; then nothing until the client's
    /// second close.
    Closing {
        /// The ALC state that started the close.
        started: u16,
        /// Own states left that carry the close sequence.
        left: u8,
    },
    /// The clear to `16=2d` after the second close: own states left that carry it.
    Cleared {
        /// The ALC state that started the clear.
        started: u16,
        /// Own states left that carry the clear.
        left: u8,
    },
}

/// A slayer state that started in an input record and that no own state carried yet.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Pending {
    /// A new sequence of the state that goes on (the swim strokes: child 15 alone).
    Sequence(u64),
    /// An action the client started.
    Client(SlayerInput),
    /// An attack the server started (the retail form of its starts and follow-ups: children 14,
    /// 15 and 17, no state).
    Attack(u64),
    /// A state the server started: its state and sequence (movement, dodge, jump).
    State(u64, u64),
    /// The first-slot chain the server opened at a press: its state and sequence, with the
    /// second slot started with it (Observed: all 64 own and 157 remote state-10 starts of
    /// `11-Savage-Divide-ab1760f2e3180cab/EXP-Savage-Divide` carried child 17, for example the
    /// remote flail press start `13=10 14=711 15=1319 17=711 48=1` at 1793443086841).
    Press(u64, u64),
    /// A full-body weapon ability the server started ([`SlayerTrack::ability_started`]).
    Ability(FirstPlay),
}

/// Which upper slots play ([`SlayerTrack::slots_on`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct SlotsOn {
    second: bool,
    third: bool,
    fourth: bool,
}

impl SlotsOn {
    /// The slots after `change` went out: a slot whose change sets its sequence plays when the
    /// sequence is not 0, one whose change sets only its state when the state is not 0.
    fn after(self, change: UpperSlots) -> Self {
        let on = |was: bool, slot: SlotChange| {
            slot.sequence
                .or(slot.state)
                .map_or(was, |value| value != CLEARED_SLOT)
        };
        Self {
            second: on(self.second, change.second),
            third: on(self.third, change.third),
            fourth: on(self.fourth, change.fourth),
        }
    }
}

/// The weapon ability whose animation the slayer state plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AbilityOn {
    /// CRC-32 of its lowercase id.
    ability: u32,
    /// It plays in the first slot (a full-body ability).
    first: bool,
    /// It plays a third slot state (an aimed ability).
    third: bool,
    /// Its cast went out.
    cast: bool,
}

/// The weapon in the player's hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HeldWeapon {
    /// Its stance key ([`nw_content::player_attacks::stance_key`]).
    pub(super) stance: u32,
    /// Whether it is drawn.
    pub(super) drawn: bool,
    /// It has shots and none is loaded ([`super::gear::Gear::held_empty`]).
    pub(super) empty: bool,
}

/// What an attack key press changed in the upper slots ([`SlayerTrack::shot_slots`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shot {
    /// The reload: the press fires nothing.
    Reload(UpperSlots),
    /// The aimed shot.
    Aimed(UpperSlots),
}

/// The attack key in one input record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct AttackKey {
    /// It went down in this record.
    pub(super) pressed: bool,
    /// It came up in this record.
    pub(super) released: bool,
}

/// One input record for the slayer state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SlayerRecord {
    /// The record's counter.
    pub(super) counter: u16,
    /// Its frame time, s.
    pub(super) frame_s: f32,
    /// The action its client starts in it.
    pub(super) action: Option<SlayerInput>,
    /// Its movement keys, when the server moves the body by them; with them the dodge, jump and
    /// right-click keys.
    pub(super) intent: Option<Intent>,
    /// Its attack key.
    pub(super) attack: AttackKey,
    /// The weapon in the hand.
    pub(super) weapon: Option<HeldWeapon>,
    /// The body swims ([`nw_sim_core::TrackedBody::is_swimming`]).
    pub(super) swimming: bool,
    /// The body falls and is not in a jump ([`nw_sim_core::TrackedBody::is_falling`]).
    pub(super) falling: bool,
    /// How the body holds itself ([`nw_sim_core::TrackedBody::posture`]).
    pub(super) posture: Posture,
}

/// What one own state carries of the slayer state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct SlayerDue {
    /// The slayer state, when it changed or echoes.
    pub(super) state: Option<SlayerState>,
    /// The time into its sequence.
    pub(super) time: Option<SequenceTime>,
    /// The slots past the first and the marker, when they changed or echo.
    pub(super) slots: Option<UpperSlots>,
}

/// One player's slayer state.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SlayerTrack {
    /// A state that started in a record (its counter, and that record's frame time) and that no
    /// own state carried yet.
    pending: Option<(Pending, u16, f32)>,
    /// The follow-up that starts in the next own state.
    follow_up: Option<u64>,
    /// The follow-up that starts at the attack key's next release.
    release_follow_up: Option<u64>,
    /// The record of the attack key's press, while it is down and no action answered it.
    press: Option<u16>,
    /// The attack key is down.
    attack_down: bool,
    /// The attack that plays (the server's last start or the client's last action), and the
    /// record that started it.
    running: Option<(RunningAttack, u16)>,
    /// The server opened the chain that runs with its marker (child 48) set; the chain's end
    /// clears it.
    chain_marked: bool,
    /// The upper slots that changed in records no own state carried yet.
    upper: Option<UpperSlots>,
    /// The newest upper slot change, and how many more own states carry it.
    upper_current: Option<UpperSlots>,
    upper_repeats: u8,
    /// The newest upper slot change came after the newest slayer state started.
    upper_after_state: bool,
    /// The upper slots of the newest tick's own state, for the players who see this one; kept
    /// after the own state took them, until the next tick.
    remote: Option<RemoteUpper>,
    /// The previous record's weapon had shots and none loaded.
    was_empty: bool,
    /// The weapon was loaded again after it emptied and no reload played since.
    reload_due: bool,
    /// The previous record's keys: right-click, dodge, jump; and whether the weapon was drawn.
    alt_held: bool,
    dodge_held: bool,
    jump_held: bool,
    drawn: bool,
    /// The stance of the weapon the previous record had drawn.
    drawn_stance: Option<u32>,
    /// The movement states of the newest record ([`MoveSet`]).
    set: Option<MoveSet>,
    /// The player's own movement.
    locomotion: Locomotion,
    /// The posture of the newest record.
    posture: Posture,
    /// Input records left while a change to prone holds a start
    /// ([`posture::PRONE_HOLD_RECORDS`]).
    posture_hold: u16,
    /// Input records left until the new posture's stand or move sequence goes out
    /// ([`posture::change`]).
    posture_settle: Option<u16>,
    /// The last record was a fall the server already started.
    in_air: bool,
    /// The player's swim.
    swim: Swim,
    /// The inventory's second slayer slot.
    loadout: LoadoutPlay,
    /// A mob pulls the player: the pull state starts in the next own state.
    pull_due: bool,
    /// When the pull ends and the player stands still again, while it is pulled.
    pull_ends_ms: Option<u64>,
    /// A hit reaction's sequence: the reaction state starts in the next own state ([`reaction`]).
    reaction_due: Option<u64>,
    /// While a hit reaction holds the player: when its keys may start states again.
    reaction_free_ms: Option<u64>,
    /// While a hit reaction plays: when it ends (the second slot's end state).
    reaction_ends_ms: Option<u64>,
    /// An upper-body hit reaction's sequence: the fourth slot starts it in the next own state
    /// ([`upper_reaction`]).
    upper_reaction_due: Option<u64>,
    /// While an upper-body hit reaction plays: when its fourth slot ends.
    upper_reaction_ends_ms: Option<u64>,
    /// The upper slots that play: each one's last change set a state or sequence other than 0
    /// (the hit reaction clears them).
    slots_on: SlotsOn,
    /// A fast travel's channel or an unstuck's wait started: the kneel starts in the next own
    /// state ([`kneel`]).
    kneel_due: bool,
    /// A fast travel's channel ended: the next own state stands the player still, unless a
    /// state of its own records starts there ([`kneel`]).
    stand_due: bool,
    /// The newest slayer state, and how many more own states carry it.
    current: Option<SlayerState>,
    repeats: u8,
    /// The slayer state the attack that runs plays in: the state of the press start the server
    /// opened its chain with, or of the action the client named (row 1). Its release, hold and
    /// follow-up starts go on in it.
    chain_state: Option<u64>,
    /// The slayer state the newest start plays in, as the players who see this one have it.
    played_state: Option<u64>,
    /// The newest slayer state as the players who see this one get it
    /// ([`SlayerTrack::remote_action`]).
    remote_action: Option<SlayerState>,
    /// The weapon ability the slayer state plays, from its start to its end.
    ability: Option<AbilityOn>,
    /// The counter of the newest record.
    newest_counter: u16,
    /// Seconds into the newest state's sequence.
    sequence_s: f32,
    /// The frame time of the newest record.
    newest_frame_s: f32,
    /// The sequence time the client has; `None` before its first own state.
    clock: Option<SequenceClock>,
    /// What this tick's own state carries, until that state takes it.
    due: SlayerDue,
}

impl SlayerTrack {
    /// The player's client took its first own state ([`nw_messages::AlcState::spawn`]): its
    /// slots and sequence time start there, standing still.
    pub(super) fn spawned(&mut self) {
        *self = Self {
            sequence_s: SequenceClock::spawn_seconds(),
            clock: Some(SequenceClock::after_spawn()),
            played_state: Some(movement::IDLE_STATE),
            ..Self::default()
        };
    }

    /// Reads one input record not read before (oldest first), with the sequences of `table`:
    /// its frame time goes on the sequence; an action its client starts goes out with the next
    /// own state, else a state the server starts for its keys (dodge, jump, attack, movement);
    /// the right-click and the draw change the upper slots.
    pub(super) fn record(&mut self, record: SlayerRecord, table: &PlayerSequences) {
        self.newest_frame_s = record.frame_s;
        self.newest_counter = record.counter;
        self.sequence_s += record.frame_s;
        if self.reaction_due.is_some() || self.reaction_free_ms.is_some() {
            // A hit reaction holds the player: its records start nothing, and the client's row
            // 1 names the reaction itself, which is not echoed ([`reaction`]).
            self.press = None;
            self.attack_down = false;
            return;
        }
        // A row 1 that names the reaction state is the client's copy of the server's own start.
        let record = SlayerRecord {
            action: record
                .action
                .filter(|input| u64::from(input.state) != pull::STATE),
            ..record
        };
        let stance = record.weapon.and_then(|weapon| table.stance(weapon.stance));
        let drawn = record.weapon.is_some_and(|weapon| weapon.drawn);
        let upper = self.upper_slots(&record, stance, drawn);
        let alt = drawn && record.intent.is_some_and(|intent| intent.block);
        let posture_change =
            (record.posture != self.posture).then_some((self.posture, record.posture));
        self.posture = record.posture;
        if posture_change.is_some_and(|(_, to)| to == Posture::Prone) {
            self.posture_hold = posture::PRONE_HOLD_RECORDS - posture::SEEN_AFTER_RECORDS;
        }
        let held_start = self.posture_hold > 0 && self.locomotion == Locomotion::Idle;
        self.posture_hold = self.posture_hold.saturating_sub(1);
        let set = MoveSet::of(stance, drawn, alt, record.posture);
        self.set = Some(set);
        let entered_air = record.falling && !record.swimming && !self.in_air;
        let entered_water = record.swimming && self.swim == Swim::Out;
        self.in_air = record.falling && !record.swimming;
        let mut stroke = None;
        if entered_water {
            self.locomotion = Locomotion::Idle;
            self.swim = Swim::Entered(0.0);
        } else if !record.swimming {
            self.swim = Swim::Out;
        } else if let Some(intent) = record.intent {
            let moving = intent.forward != 0 || intent.right != 0;
            let (next, sequence) = self.swim.step(moving, record.frame_s);
            self.swim = next;
            stroke = sequence;
        }
        // A full-body ability holds the first slot until its own end, which starts the movement
        // the player is in then ([`SlayerTrack::ability_ended`]). Retail `general-other-01`: the
        // flail's Burst at 171.506, 498.311 and 717.672 s, with the move keys held, kept its
        // state until its end 617 to 634 ms later, as the two starts with no move key did. Ours
        // sent the stop of the move 235 ms into a Trip; the client then played none of that
        // Trip, Slam and Blast, and played all three a minute later with no cut (live
        // 2026-11-14).
        let ability_holds = self.ability.is_some_and(|on| on.first);
        let movement = if entered_air {
            Some((swim::FALL_STATE, swim::FALL_SEQUENCE))
        } else if entered_water {
            Some((swim::STATE, swim::IDLE_SEQUENCE))
        } else if record.swimming {
            None
        } else {
            record.intent.filter(|_| !held_start).and_then(|intent| {
                let (next, started) = self.locomotion.step(intent, set);
                self.locomotion = next;
                started
            })
        };
        let mut movement = movement.filter(|_| !ability_holds);
        let stroke = stroke.filter(|_| !ability_holds);
        if upper.is_some() && movement.is_none() && !self.keeps_attack() && !ability_holds {
            // The walk changes with the block or aim: retail sent the new walk sequence in the
            // state of the right-click (`15=af10` with the aim). A draw that arrives after a
            // tap has already started the light must not replace that attack with a walk.
            movement = Some(self.locomotion.current(set));
        }
        if let Some(upper) = upper {
            self.upper = Some(merge(self.upper.unwrap_or_default(), upper));
        }
        let shot = self.shot_slots(&record, stance, drawn);
        if let Some(Shot::Reload(slots) | Shot::Aimed(slots)) = shot {
            self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
        }
        let server = self.server_start(&record, stance);
        if matches!(shot, Some(Shot::Reload(_))) {
            // The reload's press is no tap: its release starts no attack.
            self.press = None;
        }
        let started = record
            .action
            .map(Pending::Client)
            .or(server)
            .or_else(|| movement.map(|(state, sequence)| Pending::State(state, sequence)))
            .or_else(|| stroke.map(Pending::Sequence));
        let posture_sequence =
            self.posture_sequence(posture_change, set, movement.is_some(), started.is_some());
        let started = started.or_else(|| {
            posture_sequence
                .filter(|_| !ability_holds)
                .map(Pending::Sequence)
        });
        if let Some(input) = record.action {
            // The attack the client named runs now: a release or a hold follows it.
            self.running = Some((first_slot(input.sequence), record.counter));
            self.chain_state = Some(u64::from(input.state));
            match table.follow_up(input.sequence) {
                Some(follow_up) if follow_up.at_release && !record.attack.released => {
                    self.release_follow_up = Some(follow_up.server);
                }
                Some(follow_up) => self.queue_follow_up(follow_up.server),
                None => self.release_follow_up = None,
            }
        }
        if let Some(started) = started {
            self.pending = Some((started, record.counter, record.frame_s));
            if !matches!(started, Pending::Sequence(_)) {
                // A new sequence of a state that goes on keeps that state's time.
                self.sequence_s = record.frame_s;
            }
            if !matches!(started, Pending::Client(_)) {
                self.follow_up = None;
            }
        }
    }

    /// The sequence the posture starts in this record: the change's own at a change of posture
    /// `change` ([`posture::change`]), then the sequence of the stand or the move in `set` when
    /// the change settles. A movement state that starts (`moved`) shows the posture by itself
    /// and ends the wait; while another state starts (`started`) the sequence waits a record.
    fn posture_sequence(
        &mut self,
        change: Option<(Posture, Posture)>,
        set: MoveSet,
        moved: bool,
        started: bool,
    ) -> Option<u64> {
        if moved {
            self.posture_settle = None;
            return None;
        }
        let settled = self.locomotion.current(set).1;
        if let Some((from, to)) = change {
            let moving = self.locomotion == Locomotion::Moving;
            let (sequence, records) = posture::change(from, to, moving);
            let wait = records.saturating_sub(posture::SEEN_AFTER_RECORDS).max(1);
            self.posture_settle = (sequence.is_some() || started).then_some(wait);
            return sequence.or(Some(settled)).filter(|_| !started);
        }
        let left = self.posture_settle?.saturating_sub(1);
        let due = left == 0 && !started;
        self.posture_settle = (!due).then_some(left.max(1));
        due.then_some(settled)
    }

    /// Queues `sequence` to start one own state after the client's action (the retail
    /// follow-ups that came without a key, 0 to 132 ms after it).
    fn queue_follow_up(&mut self, sequence: u64) {
        self.follow_up = Some(sequence);
        self.release_follow_up = None;
    }

    /// The state the server starts for the keys of `record` with the sequences of `stance`:
    /// a dodge or a jump at its key's press (state 07 and 0e, by direction), else an attack
    /// ([`SlayerTrack::attack_start`]).
    fn server_start(
        &mut self,
        record: &SlayerRecord,
        stance: Option<&StanceSequences>,
    ) -> Option<Pending> {
        let intent = record.intent?;
        let dodge = intent.dodge && !self.dodge_held;
        let jump = intent.jump && !self.jump_held;
        self.dodge_held = intent.dodge;
        self.jump_held = intent.jump;
        let direction = Direction::of_keys(
            intent.forward > 0,
            intent.forward < 0,
            intent.right < 0,
            intent.right > 0,
        );
        let state = |play: StatePlay| Pending::State(play.state, play.sequence);
        let keyed = if dodge {
            stance
                .and_then(|stance| stance.dodge?.of(direction))
                .map(state)
        } else if jump {
            stance
                .and_then(|stance| stance.jump?.of(direction))
                .map(state)
        } else {
            None
        };
        let attack = self.attack_start(record, stance, intent.block);
        keyed.or(attack)
    }

    /// The attack the server starts for the attack key of `record` (attack sequence study
    /// 2026-11-11): at a press while no attack or ability runs, the stance's press start
    /// ([`AttackPress`]); at a release, the follow-up of the client's last action, else what
    /// follows the attack that runs ([`StanceSequences::release_after`]); while the key stays
    /// down, what follows the attack that runs after its hold records
    /// ([`StanceSequences::hold_after`]). An attack runs for [`ATTACK_RUN_RECORDS`] records
    /// after its start. A first-slot start is the returned state; a second-slot start goes
    /// into the upper slots. No attack starts while the right-click is held (`aiming`;
    /// Observed: the aimed shots of the retail newchar capture were the client's; the server
    /// sent no start at their releases).
    fn attack_start(
        &mut self,
        record: &SlayerRecord,
        stance: Option<&StanceSequences>,
        aiming: bool,
    ) -> Option<Pending> {
        let counter = record.counter;
        if record.attack.pressed {
            self.press = Some(counter);
            self.attack_down = true;
        }
        self.end_attack(counter, stance);
        let running = self
            .running
            .filter(|(_, started)| counter.wrapping_sub(*started) < ATTACK_RUN_RECORDS);
        if record.attack.released {
            self.attack_down = false;
            let tapped = self.press.take().filter(|_| !aiming);
            if let Some(sequence) = self.release_follow_up.take() {
                return self.start_attack(first_slot(sequence), counter, None);
            }
            tapped?;
            let next = stance?.release_after(running?.0)?;
            return self.start_attack(next, counter, None);
        }
        if aiming {
            return None;
        }
        // The client opens some chains itself (input row 1); the press start is the server's
        // only when the press record names none.
        if record.attack.pressed
            && record.action.is_none()
            && running.is_none()
            && self.ability.is_none()
        {
            let press = stance?.press?;
            return self.start_attack(press.start, counter, Some(press));
        }
        // A hold follows the attack that started last, however long it runs: retail's great
        // sword started its second charged attack 60 records into the hold.
        if self.attack_down
            && let Some((attack, started)) = self.running
            && let Some((next, records)) = stance.and_then(|stance| stance.hold_after(attack))
            && counter.wrapping_sub(started) == records
        {
            self.press = None;
            return self.start_attack(next, counter, None);
        }
        None
    }

    /// Ends the attack that runs, in record `counter`, when no start followed it. A first-slot
    /// chain the server opened ends [`CHAIN_END_RECORDS`] after its last start with the marker
    /// back at 0 (`48=0` alone, still in state 10; Observed 1,104 times, 932 ms median after the
    /// last start). A second-slot cast ends the stance's
    /// [`StanceSequences::cast_end_records`] after its last start with `16=45 18=0 48=0`
    /// (Observed: no end went out when a new press came first).
    fn end_attack(&mut self, counter: u16, stance: Option<&StanceSequences>) {
        let Some((attack, started)) = self.running else {
            return;
        };
        let age = counter.wrapping_sub(started);
        let change = match attack.slot {
            AttackSlot::First if age == CHAIN_END_RECORDS && self.chain_marked => {
                self.chain_marked = false;
                SlotsChange {
                    marker: Some(CLEARED_SLOT),
                    ..SlotsChange::default()
                }
            }
            AttackSlot::Second
                if stance.and_then(|stance| stance.cast_end_records) == Some(age) =>
            {
                self.running = None;
                SlotsChange {
                    second_state: Some(CAST_END_STATE),
                    second_sequence: Some(CLEARED_SLOT),
                    marker: Some(CLEARED_SLOT),
                    ..SlotsChange::default()
                }
            }
            _ => return,
        };
        if let Some(slots) = slots_of(change, counter) {
            self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
        }
    }

    /// Starts the attack `attack` in record `counter`: it runs from there. In the first slot
    /// it is the returned state: the press start with its state (`13=10 14 15`, and the marker
    /// in child 48), any other start in retail's attack form (`14 15 17`). In the second slot
    /// it goes into the upper slots: the press start with its state and marker (`16=33 17 18
    /// 48=3`), any other start as the sequence alone (child 18).
    fn start_attack(
        &mut self,
        attack: RunningAttack,
        counter: u16,
        press: Option<AttackPress>,
    ) -> Option<Pending> {
        self.running = Some((attack, counter));
        let change = match (attack.slot, press) {
            (AttackSlot::First, Some(press)) => {
                self.chain_marked = true;
                SlotsChange {
                    marker: Some(press.marker),
                    ..SlotsChange::default()
                }
            }
            (AttackSlot::First, None) => SlotsChange::default(),
            (AttackSlot::Second, Some(press)) => SlotsChange {
                second_state: Some(press.state),
                second_sequence: Some(attack.sequence),
                marker: Some(press.marker),
                ..SlotsChange::default()
            },
            (AttackSlot::Second, None) => SlotsChange {
                second_sequence: Some(attack.sequence),
                ..SlotsChange::default()
            },
        };
        if let Some(slots) = slots_of(change, counter) {
            self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
        }
        match (attack.slot, press) {
            (AttackSlot::First, Some(press)) => {
                self.chain_state = Some(press.state);
                Some(Pending::Press(press.state, attack.sequence))
            }
            (AttackSlot::First, None) => Some(Pending::Attack(attack.sequence)),
            (AttackSlot::Second, _) => None,
        }
    }

    /// The upper slot change of `record`: the block or aim at the right-click's press and its
    /// end at the release, with the weapon drawn; the draw's second slot when the weapon came
    /// to the hand. A slot whose state or sequence goes out starts in the record (retail: the
    /// aim's `20` is the right-click record), except the second slot without a state (retail's
    /// block sends child 18 alone).
    fn upper_slots(
        &mut self,
        record: &SlayerRecord,
        stance: Option<&StanceSequences>,
        drawn: bool,
    ) -> Option<UpperSlots> {
        let alt = drawn && record.intent.is_some_and(|intent| intent.block);
        let was_alt = self.alt_held;
        let was_drawn = self.drawn;
        let was_stance = self.drawn_stance;
        if record.intent.is_some() {
            self.alt_held = alt;
        }
        self.drawn = drawn;
        self.drawn_stance = record
            .weapon
            .filter(|weapon| weapon.drawn)
            .map(|weapon| weapon.stance);
        let second = |state: u64, sequence: u64| SlotsChange {
            second_state: Some(state),
            second_sequence: Some(sequence),
            ..Default::default()
        };
        if was_drawn && !drawn {
            return slots_of(second(sheathe::STATE, sheathe::SEQUENCE), record.counter);
        }
        let stance = stance?;
        let change = (if alt && !was_alt {
            stance.alt_start
        } else if !alt && was_alt && record.intent.is_some() {
            stance.alt_end
        } else if drawn && (!was_drawn || was_stance != self.drawn_stance) {
            // A draw, and a swap to the other weapon while one is out: retail starts the new
            // weapon's draw at every weapon key (351 presses: `16=2c 18=<draw>` with the new
            // stance's sequence in the first slot). Ours started it from no weapon only, and
            // the other players kept the old weapon's pose (live 2026-11-09).
            stance.draw.map(|draw| second(draw.state, draw.sequence))
        } else {
            None
        })?;
        slots_of(change, record.counter)
    }

    /// The upper slot change of an attack key press with the weapon drawn ([`Shot`]): the
    /// stance's reload when the weapon was loaded again after it emptied and no reload played
    /// since, else, with the right-click held and a shot loaded, its aimed shot. Retail
    /// blunderbuss (newchar 2026-09-18): the aimed shot `18=5e 48=29` 27 ms after the press; the
    /// presses while empty changed nothing; the first press 2.1 s after the shot sent the reload
    /// `16=24 17=557a 18=8142`, and the next press shot again.
    fn shot_slots(
        &mut self,
        record: &SlayerRecord,
        stance: Option<&StanceSequences>,
        drawn: bool,
    ) -> Option<Shot> {
        let empty = record.weapon.is_some_and(|weapon| weapon.empty);
        if self.was_empty && !empty {
            self.reload_due = true;
        }
        self.was_empty = empty;
        if !drawn || !record.attack.pressed {
            return None;
        }
        let stance = stance?;
        if std::mem::take(&mut self.reload_due) {
            return slots_of(stance.reload?, record.counter).map(Shot::Reload);
        }
        let aiming = record.intent.is_some_and(|intent| intent.block);
        if !aiming || empty {
            return None;
        }
        slots_of(stance.alt_fire?, record.counter).map(Shot::Aimed)
    }

    /// The server started weapon ability `ability` (CRC-32 of its lowercase id) for the key
    /// press in record `started`: the next own state plays its animation `play`, started there.
    /// A full-body ability takes the first slot (its state and sequence, the second slot started
    /// with it as retail's `13=08 14=<press> 15=<sequence> 17=<press>`); every ability sets the
    /// slots past it and the marker it names.
    pub(super) fn ability_started(&mut self, ability: u32, play: &AbilityPlay, started: u16) {
        let started = play
            .first
            .and_then(|first| self.client_named_since(first.sequence, started))
            .unwrap_or(started);
        if let Some(first) = play.first {
            self.pending = Some((Pending::Ability(first), started, self.newest_frame_s));
            self.sequence_s = self.newest_frame_s;
            self.follow_up = None;
        }
        let second_started = UpperSlots {
            second: SlotChange {
                started: Some(started),
                ..SlotChange::default()
            },
            ..UpperSlots::default()
        };
        let slots = slots_of(play.slots, started).unwrap_or_default();
        self.upper = Some(merge(
            self.upper.unwrap_or_default(),
            merge(second_started, slots),
        ));
        self.ability = Some(AbilityOn {
            ability,
            first: play.first.is_some(),
            third: play
                .slots
                .third_state
                .is_some_and(|state| state != CLEARED_SLOT),
            cast: false,
        });
    }

    /// The record from press record `pressed` on in which the client named first-slot sequence
    /// `sequence` itself (row 1), when it did: a full-body ability starts there. Retail
    /// `general-other-01` 171.471 s: the client named the flail's Burst in frame 0x0e49 and the
    /// server's start was `13=08 14=0e49 15=2c 17=0e49`. Ours started a Blast that the client
    /// named at a later record (pressed in the Slam, it ran when the Slam let it) from the press
    /// record, 16 records before the client's own start: two starts, the second older, and no
    /// Blast played (live 2026-11-14).
    fn client_named_since(&self, sequence: u64, pressed: u16) -> Option<u16> {
        self.running
            .filter(|(attack, counter)| {
                *attack == first_slot(sequence)
                    && (*counter == pressed || counter_is_newer(*counter, pressed))
            })
            .map(|(_, counter)| counter)
    }

    /// Weapon ability `ability` cast its first spell: an aimed one plays its cast `play.cast`,
    /// started in the newest record (retail Sacred Ground: `16=33 18=993 19=00 21=00 48=03` at
    /// the click; Inferred: the server sends it at the cast, about 300 ms after the click).
    pub(super) fn ability_cast(&mut self, ability: u32, play: &AbilityPlay) {
        let Some(on) = self
            .ability
            .as_mut()
            .filter(|on| on.ability == ability && !on.cast)
        else {
            return;
        };
        on.cast = true;
        if let Some(cast) = play
            .cast
            .and_then(|cast| slots_of(cast, self.newest_counter))
        {
            self.upper = Some(merge(self.upper.unwrap_or_default(), cast));
        }
    }

    /// Weapon ability `ability` ended: a full-body one goes back to the movement it stands or
    /// moves in, an upper-body one clears the second slot (and the third, when it aimed), and
    /// the marker goes back to 0 (retail: `13=1f 14=<record> 15=<idle> 48=00` after the flail's
    /// Trip, `16=2d 17=<record> 18=00 48=00` after the life staff's Orb of Protection). Another
    /// ability's end, after a new one started, changes nothing.
    pub(super) fn ability_ended(&mut self, ability: u32) {
        let Some(on) = self.ability.filter(|on| on.ability == ability) else {
            return;
        };
        self.ability = None;
        let counter = self.newest_counter;
        let mut end = UpperSlots {
            marker: Some(CLEARED_SLOT),
            ..UpperSlots::default()
        };
        if on.first {
            let (state, sequence) = self.card_movement();
            self.pending = Some((
                Pending::State(state, sequence),
                counter,
                self.newest_frame_s,
            ));
            self.sequence_s = self.newest_frame_s;
        } else {
            end.second = SlotChange {
                state: Some(loadout::CLOSED_STATE),
                started: Some(counter),
                sequence: Some(CLEARED_SLOT),
            };
        }
        if on.third {
            end.third = SlotChange {
                state: Some(CLEARED_SLOT),
                started: Some(counter),
                sequence: Some(CLEARED_SLOT),
            };
        }
        self.upper = Some(merge(self.upper.unwrap_or_default(), end));
    }

    /// A mob pulls the player at `now_ms`: the next own state starts the pull state, and the
    /// player stands still again [`pull::LENGTH_MS`] later.
    pub(super) fn pulled(&mut self, now_ms: u64) {
        self.pull_due = true;
        self.pull_ends_ms = Some(now_ms.saturating_add(pull::LENGTH_MS));
    }

    /// A hit reaction of sequence `sequence` started at `now_ms` ([`reaction`]): the next own
    /// state starts the reaction state; the player's records start nothing for `free_after_ms`
    /// (then a key moves it again, else it stands still), and the reaction ends `length_ms`
    /// after its start.
    pub(super) fn reacted(
        &mut self,
        sequence: u64,
        now_ms: u64,
        free_after_ms: u64,
        length_ms: u64,
    ) {
        self.reaction_due = Some(sequence);
        self.reaction_free_ms = Some(now_ms.saturating_add(free_after_ms));
        self.reaction_ends_ms = Some(now_ms.saturating_add(length_ms));
        // The full-body reaction's start clears the fourth slot when it plays.
        self.upper_reaction_due = None;
        self.upper_reaction_ends_ms = None;
    }

    /// A dodge broke the hit reaction out (`Dodge_Stunlock`): the player's records start states
    /// again at once, and the dodge's own state takes the reaction's place, so no reaction end
    /// goes out (Inferred: no retail breakout was mined).
    pub(super) fn break_reaction(&mut self) {
        self.reaction_due = None;
        self.reaction_free_ms = None;
        self.reaction_ends_ms = None;
    }

    /// An upper-body hit reaction of sequence `sequence` started at `now_ms`
    /// ([`upper_reaction`]): the next own state starts the fourth slot, which ends `end_after_ms`
    /// later; it holds no key.
    pub(super) fn shook(&mut self, sequence: u64, now_ms: u64, end_after_ms: u64) {
        self.upper_reaction_due = Some(sequence);
        self.upper_reaction_ends_ms = Some(now_ms.saturating_add(end_after_ms));
    }

    /// The upper slots that a hit reaction's start clears in the own state numbered `number`:
    /// each slot that plays, and the slayer marker when the server's chain set it ([`reaction`]).
    fn reaction_clears(&self, number: u16) -> UpperSlots {
        let cleared = |on: bool| {
            if on {
                SlotChange {
                    state: Some(CLEARED_SLOT),
                    started: Some(number),
                    sequence: Some(CLEARED_SLOT),
                }
            } else {
                SlotChange::default()
            }
        };
        UpperSlots {
            // The second slot's state and start go with the reaction state (`16=00 17=<n>`).
            second: SlotChange {
                sequence: self.slots_on.second.then_some(CLEARED_SLOT),
                ..SlotChange::default()
            },
            third: cleared(self.slots_on.third),
            fourth: cleared(self.slots_on.fourth),
            marker: self.chain_marked.then_some(CLEARED_SLOT),
        }
    }

    /// A fast travel's channel or an unstuck's wait started: the next own state starts the
    /// kneel, and the player stands still under it ([`kneel`]).
    pub(super) fn kneel(&mut self) {
        self.kneel_due = true;
        self.locomotion = Locomotion::Idle;
    }

    /// A fast travel's channel or an unstuck's wait ended (arrived, cancelled or failed): the
    /// next own state stands the player still with the second slot cleared, unless a state of
    /// its records starts there ([`kneel`]).
    pub(super) fn stand(&mut self) {
        self.kneel_due = false;
        self.stand_due = true;
        self.locomotion = Locomotion::Idle;
    }

    /// The client opened its inventory in record `started` at `now_ms`: the second slot plays the
    /// open sequence ([`loadout`]).
    pub(super) fn open_loadout(&mut self, started: u16, now_ms: u64) {
        self.loadout = LoadoutPlay::Opening {
            started,
            opened_ms: now_ms,
            left: ECHO_STATES,
        };
    }

    /// The client closed its inventory in record `started`: the first close plays the close
    /// sequence, the second clears the slot ([`loadout`]); a close with no open does nothing.
    pub(super) fn close_loadout(&mut self, started: u16) {
        self.loadout = match self.loadout {
            LoadoutPlay::Opening { .. } | LoadoutPlay::Looping { .. } => LoadoutPlay::Closing {
                started,
                left: ECHO_STATES,
            },
            LoadoutPlay::Closing { .. } => LoadoutPlay::Cleared {
                started,
                left: ECHO_STATES,
            },
            play @ (LoadoutPlay::Closed | LoadoutPlay::Cleared { .. }) => play,
        };
    }

    /// Moves the inventory slot on at `state_ms`: the loop after [`loadout::LOOP_MS`], and the
    /// end of the clear.
    fn step_loadout(&mut self, state_ms: u64) {
        match self.loadout {
            LoadoutPlay::Opening {
                started, opened_ms, ..
            } if state_ms.saturating_sub(opened_ms) >= loadout::LOOP_MS => {
                self.loadout = LoadoutPlay::Looping {
                    started,
                    left: ECHO_STATES,
                };
            }
            LoadoutPlay::Cleared { left: 0, .. } => self.loadout = LoadoutPlay::Closed,
            _ => {}
        }
    }

    /// The inventory's second slot for this own state, while a change of it echoes.
    fn loadout_slots(&self) -> Option<UpperSlots> {
        let (state, started, sequence) = match self.loadout {
            LoadoutPlay::Closed
            | LoadoutPlay::Opening { left: 0, .. }
            | LoadoutPlay::Looping { left: 0, .. }
            | LoadoutPlay::Closing { left: 0, .. }
            | LoadoutPlay::Cleared { left: 0, .. } => return None,
            LoadoutPlay::Opening { started, .. } => {
                (Some(loadout::OPEN_STATE), started, loadout::OPEN_SEQUENCE)
            }
            LoadoutPlay::Looping { started, .. } => {
                (Some(loadout::OPEN_STATE), started, loadout::LOOP_SEQUENCE)
            }
            LoadoutPlay::Closing { started, .. } => (None, started, loadout::CLOSE_SEQUENCE),
            LoadoutPlay::Cleared { started, .. } => (Some(loadout::CLOSED_STATE), started, 0),
        };
        Some(UpperSlots {
            second: SlotChange {
                state,
                started: Some(started),
                sequence: Some(sequence),
            },
            ..UpperSlots::default()
        })
    }

    /// One own state carried the inventory slot.
    fn consume_loadout_echo(&mut self) {
        if let LoadoutPlay::Opening { left, .. }
        | LoadoutPlay::Looping { left, .. }
        | LoadoutPlay::Closing { left, .. }
        | LoadoutPlay::Cleared { left, .. } = &mut self.loadout
        {
            *left = left.saturating_sub(1);
        }
    }

    /// Moves on to the tick whose own state is numbered `number` at state time `state_ms`: its
    /// slayer state is a newly started one, else the server's follow-up, else the last change once
    /// more ([`ECHO_STATES`]); its sequence time goes with every own state. The players who see
    /// this one get the newest state at once ([`SlayerTrack::current`]).
    pub(super) fn advance(&mut self, number: u16, state_ms: u64) {
        self.step_loadout(state_ms);
        if self
            .reaction_free_ms
            .is_some_and(|free_ms| state_ms >= free_ms)
        {
            // From here a key moves the player again, else it stands still.
            self.reaction_free_ms = None;
            self.locomotion = Locomotion::Stopping(reaction::STOP_RECORDS);
        }
        if self
            .reaction_ends_ms
            .is_some_and(|ends_ms| state_ms >= ends_ms)
        {
            self.reaction_ends_ms = None;
            let end = UpperSlots {
                second: SlotChange {
                    state: Some(CAST_END_STATE),
                    started: Some(number),
                    sequence: None,
                },
                ..UpperSlots::default()
            };
            self.upper = Some(merge(self.upper.unwrap_or_default(), end));
        }
        if self
            .upper_reaction_ends_ms
            .is_some_and(|ends_ms| state_ms >= ends_ms)
            && self.upper_reaction_due.is_none()
        {
            self.upper_reaction_ends_ms = None;
            let end = UpperSlots {
                fourth: SlotChange {
                    state: Some(CLEARED_SLOT),
                    started: Some(number),
                    sequence: Some(CLEARED_SLOT),
                },
                ..UpperSlots::default()
            };
            self.upper = Some(merge(self.upper.unwrap_or_default(), end));
        }
        if let Some(sequence) = self.upper_reaction_due.take() {
            let start = UpperSlots {
                fourth: SlotChange {
                    state: Some(upper_reaction::STATE),
                    started: Some(number),
                    sequence: Some(sequence),
                },
                ..UpperSlots::default()
            };
            self.upper = Some(merge(self.upper.unwrap_or_default(), start));
        }
        if self.pull_ends_ms.is_some_and(|ends_ms| state_ms >= ends_ms) {
            self.pull_ends_ms = None;
            self.locomotion = Locomotion::Idle;
            self.pending = Some((
                Pending::State(movement::IDLE_STATE, movement::IDLE_SEQUENCE),
                number,
                self.newest_frame_s,
            ));
        }
        let (state, started) = self.next(number);
        let sequence_s = self.sequence_s;
        let time = self
            .clock
            .as_mut()
            .map(|clock| clock.time(sequence_s, started, state_ms));
        let slots = match (self.next_upper(), self.loadout_slots()) {
            (Some(upper), Some(loadout)) => Some(merge(upper, loadout)),
            (Some(slots), None) | (None, Some(slots)) => Some(slots),
            (None, None) => None,
        };
        self.consume_loadout_echo();
        if let Some(slots) = slots {
            self.slots_on = self.slots_on.after(slots);
        }
        self.remote = slots.map(|slots| RemoteUpper {
            slots,
            after_state: self.upper_after_state,
        });
        self.due = SlayerDue { state, time, slots };
    }

    /// What this tick's own state carries, once.
    pub(super) fn take_due(&mut self) -> SlayerDue {
        std::mem::take(&mut self.due)
    }

    /// An attack the server started is still this state's action: a pending start, or the echo
    /// of one (`state` empty and the second slot started with it, retail's attack form).
    fn keeps_attack(&self) -> bool {
        matches!(self.pending, Some((Pending::Attack(_), _, _)))
            || (self.repeats > 0
                && self
                    .current
                    .is_some_and(|state| state.state.is_none() && state.second.is_some()))
    }

    /// The upper slot change of the next own state: a new one, else the last once more.
    fn next_upper(&mut self) -> Option<UpperSlots> {
        if let Some(upper) = self.upper.take() {
            self.upper_current = Some(upper);
            self.upper_repeats = ECHO_STATES - 1;
            self.upper_after_state = true;
            return Some(upper);
        }
        if self.upper_repeats == 0 {
            return None;
        }
        self.upper_repeats -= 1;
        self.upper_current
    }

    /// The slayer state of the own state numbered `number`, and the frame time of the record that
    /// started it when it starts there.
    fn next(&mut self, number: u16) -> (Option<SlayerState>, Option<f32>) {
        let stand = std::mem::take(&mut self.stand_due);
        if let Some(sequence) = self.reaction_due.take() {
            // The reaction drops what the records started and the attack that ran.
            self.pending = None;
            self.follow_up = None;
            self.release_follow_up = None;
            self.press = None;
            self.running = None;
            self.chain_state = None;
            let clears = self.reaction_clears(number);
            self.chain_marked = false;
            self.upper = Some(merge(self.upper.unwrap_or_default(), clears));
            self.sequence_s = self.newest_frame_s;
            return (
                self.start(pulled_state(number, sequence), None),
                Some(self.newest_frame_s),
            );
        }
        if std::mem::take(&mut self.pull_due) {
            self.sequence_s = self.newest_frame_s;
            return (
                self.start(pulled_state(number, pull::SEQUENCE), None),
                Some(self.newest_frame_s),
            );
        }
        if std::mem::take(&mut self.kneel_due) {
            self.sequence_s = self.newest_frame_s;
            return (
                self.start(kneel_state(number), None),
                Some(self.newest_frame_s),
            );
        }
        if let Some((pending, counter, frame_s)) = self.pending.take() {
            let mut plays = None;
            let state = match pending {
                Pending::Client(input) => {
                    plays = Some(u64::from(input.state));
                    SlayerState::of_input(input, counter)
                }
                Pending::Attack(sequence) => {
                    plays = self.chain_state;
                    attack_start(counter, sequence)
                }
                Pending::Press(state, sequence) => SlayerState {
                    state: Some(state),
                    ..attack_start(counter, sequence)
                },
                Pending::State(state, sequence) => SlayerState {
                    state: Some(state),
                    started: Some(counter),
                    sequence,
                    second: None,
                },
                Pending::Ability(first) => SlayerState {
                    state: Some(first.state),
                    started: Some(counter),
                    sequence: first.sequence,
                    second: Some(SecondSlayerState {
                        started: counter,
                        playing: None,
                        stopped: false,
                    }),
                },
                Pending::Sequence(sequence) => {
                    let state = SlayerState {
                        state: None,
                        started: None,
                        sequence,
                        second: None,
                    };
                    return (self.start(state, None), None);
                }
            };
            return (self.start(state, plays), Some(frame_s));
        }
        if stand {
            self.sequence_s = self.newest_frame_s;
            return (
                self.start(stand_state(number), None),
                Some(self.newest_frame_s),
            );
        }
        if let Some(sequence) = self.follow_up.take() {
            self.sequence_s = self.newest_frame_s;
            let plays = self.chain_state;
            return (
                self.start(attack_start(number, sequence), plays),
                Some(self.newest_frame_s),
            );
        }
        if self.repeats == 0 {
            return (None, None);
        }
        self.repeats -= 1;
        (self.current, None)
    }

    /// The newest slayer state as the players who see this one get it: the own form, with the
    /// state (child 13) added when the start plays in another state than the start before it.
    /// Observed (`11-Savage-Divide-ab1760f2e3180cab/EXP-Savage-Divide`, the starts of the other
    /// players with child 14): 3,042 of the 3,921 starts named a new state in child 13 and 233
    /// the same one again; of the 646 without it, 529 went on in state 10 and 94 in state 8.
    /// `02-The_Starstone_Barrows-c29df4e9ee294c08/EXP-Starstone-Barrows`: 1,183 new and 97
    /// same of 1,430; of the 150 without it, 54 went on in state 10 and 76 in state 8. The client names its attacks in
    /// row 1 and its own state needs no 13 ([`SlayerState::of_input`]), but a viewer's copy of
    /// the player is still in its movement state: without 13 it never played the light and
    /// heavy attacks (live 2026-11-12: others saw the abilities, whose starts carry 13, and no
    /// attack). Example: the remote flail light `13=10 14=959 15=461 17=959` at 1793443095058
    /// after an ability in state 8.
    pub(super) const fn remote_action(&self) -> Option<SlayerState> {
        self.remote_action
    }

    /// The state and sequence of its movement now, for the first card of a player who comes
    /// into view of it: its stand, move or stop in the stance of the weapon in its hand.
    pub(super) fn card_movement(&self) -> (u64, u64) {
        self.locomotion
            .current(self.set.unwrap_or(MoveSet::SHEATHED))
    }

    /// The upper slots of this tick's own state, for the players who see this one: retail's
    /// remote states echoed them as the own ones (arena capture 2026-09-13, a remote aim
    /// `16=24 18=9837 19=2e 21=9606 48=28` in 3 to 4 states, then without them).
    pub(super) const fn remote_upper(&self) -> Option<RemoteUpper> {
        self.remote
    }

    /// Seconds into the newest slayer state's sequence, for the players who see this one;
    /// `None` before the first own state. Retail sends other players' `slayerSeqTimeRel` in
    /// 1,750 of 1,751 of their states (general-other-01 and newchar-retail, the states after each
    /// first card).
    pub(super) fn remote_sequence(&self) -> Option<f32> {
        self.clock.map(|_| self.sequence_s)
    }

    /// Starts `state`, which plays in slayer state `plays` when its own form leaves child 13
    /// out ([`SlayerTrack::remote_action`]).
    fn start(&mut self, state: SlayerState, plays: Option<u64>) -> Option<SlayerState> {
        let shown = state.state.or(plays);
        let changed = shown.filter(|shown| Some(*shown) != self.played_state);
        self.remote_action = Some(SlayerState {
            state: state.state.or(changed),
            ..state
        });
        self.played_state = shown.or(self.played_state);
        self.upper_after_state = false;
        self.current = Some(state);
        self.repeats = ECHO_STATES - 1;
        self.current
    }
}

/// The first-slot attack of sequence `sequence`.
const fn first_slot(sequence: u64) -> RunningAttack {
    RunningAttack {
        slot: AttackSlot::First,
        sequence,
    }
}

/// An attack the server starts in the state numbered `started`: retail's form of its starts and
/// follow-ups (`14=06bd 15=8f07 17=06bd`: no state, the second slot started with it).
const fn attack_start(started: u16, sequence: u64) -> SlayerState {
    SlayerState {
        state: None,
        started: Some(started),
        sequence,
        second: Some(SecondSlayerState {
            started,
            playing: None,
            stopped: false,
        }),
    }
}

/// The reaction state of sequence `sequence` (a pull's, [`pull`], or a hit's, [`reaction`]) in
/// the own state numbered `started`: the second slot's action stops with it.
const fn pulled_state(started: u16, sequence: u64) -> SlayerState {
    SlayerState {
        state: Some(pull::STATE),
        started: Some(started),
        sequence,
        second: Some(SecondSlayerState {
            started,
            playing: None,
            stopped: true,
        }),
    }
}

/// The kneel in the own state numbered `started` ([`kneel`]).
const fn kneel_state(started: u16) -> SlayerState {
    SlayerState {
        state: Some(kneel::STATE),
        started: Some(started),
        sequence: kneel::SEQUENCE,
        second: None,
    }
}

/// Standing still with the second slot cleared (`16=2d 18=00`), in the own state numbered
/// `started`: the state after a fast travel's channel. The first slayer change after 65 of the
/// retail 3696 (fast travel completed) of the kneel's captures was `13=1f 15=b910 16=2d 18=00`.
/// Without it the client kept the channel's pose after the loading screen (live 2026-11-07: the
/// sword and shield showed in the hands while the server held them sheathed).
const fn stand_state(started: u16) -> SlayerState {
    SlayerState {
        state: Some(movement::IDLE_STATE),
        started: Some(started),
        sequence: movement::IDLE_SEQUENCE,
        second: Some(SecondSlayerState {
            started,
            playing: Some((loadout::CLOSED_STATE, 0)),
            stopped: false,
        }),
    }
}

/// The upper slots that `change` sends, started in record `counter`: a slot whose state or
/// sequence goes out starts there, except the second slot without a state (retail's block sends
/// child 18 alone). `None` when it sends nothing.
fn slots_of(change: SlotsChange, counter: u16) -> Option<UpperSlots> {
    let slot = |state: Option<u64>, sequence: Option<u64>, starts: bool| SlotChange {
        state,
        started: (starts && (state.is_some() || sequence.is_some())).then_some(counter),
        sequence,
    };
    let slots = UpperSlots {
        second: slot(
            change.second_state,
            change.second_sequence,
            change.second_state.is_some(),
        ),
        third: slot(change.third_state, change.third_sequence, true),
        fourth: slot(change.fourth_state, change.fourth_sequence, true),
        marker: change.marker,
    };
    (slots != UpperSlots::default()).then_some(slots)
}

/// The upper slots of `older` with the children `newer` sends in their place.
fn merge(older: UpperSlots, newer: UpperSlots) -> UpperSlots {
    let slot = |older: SlotChange, newer: SlotChange| SlotChange {
        state: newer.state.or(older.state),
        started: newer.started.or(older.started),
        sequence: newer.sequence.or(older.sequence),
    };
    UpperSlots {
        second: slot(older.second, newer.second),
        third: slot(older.third, newer.third),
        fourth: slot(older.fourth, newer.fourth),
        marker: newer.marker.or(older.marker),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use nw_content::player_attacks::stance_key;
    use nw_content::player_sequences::{
        AltMoves, AttackHold, AttackStep, Directional, DrawnMoves, FollowUp, SlotsChange,
    };

    use super::*;

    /// The 30 Hz frame time.
    const FRAME_S: f32 = 1.0 / 30.0;
    const TICK_MS: u64 = 33;
    /// The greatsword's tags and the mined table's values for it.
    const GREATSWORD: &str = "2H_Melee,GreatSword";
    const PRESS_START: u64 = 4120;
    const PRESS_STATE: u64 = 10;
    const PRESS_MARKER: u64 = 1;
    const LIGHT: u64 = 4510;
    const CHARGE: u64 = 57;
    const HOLD_RECORDS: u16 = 19;
    /// The life staff's tags and the mined table's values for it.
    const LIFE_STAFF: &str = "MagicStaff,Heal";
    const CAST_START: u64 = 934;
    const CAST_STATE: u64 = 33;
    const CAST_MARKER: u64 = 3;
    const CAST_LIGHT: u64 = 140;
    const CAST_END_RECORDS: u16 = 35;

    const fn slot(slot: AttackSlot, sequence: u64) -> RunningAttack {
        RunningAttack { slot, sequence }
    }

    /// The attack starts of a stance whose press start is `press` (slot, sequence, state,
    /// marker), with the light `light` after it and the charged attack `charge` after
    /// [`HOLD_RECORDS`].
    pub(crate) fn attacks(
        tags: &str,
        (at, press, state, marker): (AttackSlot, u64, u64, u64),
        light: u64,
        charge: Option<u64>,
    ) -> StanceSequences {
        StanceSequences {
            tags: tags.to_owned(),
            press: Some(AttackPress {
                start: slot(at, press),
                state,
                marker,
            }),
            releases: vec![AttackStep {
                after: slot(at, press),
                start: slot(at, light),
            }],
            holds: charge
                .map(|charge| AttackHold {
                    after: slot(at, press),
                    start: slot(at, charge),
                    records: HOLD_RECORDS,
                })
                .into_iter()
                .collect(),
            ..StanceSequences::default()
        }
    }

    /// The greatsword's attack starts.
    fn greatsword_attacks() -> StanceSequences {
        attacks(
            GREATSWORD,
            (AttackSlot::First, PRESS_START, PRESS_STATE, PRESS_MARKER),
            LIGHT,
            Some(CHARGE),
        )
    }
    const BLOCK_SEQUENCE: u64 = 2351;
    const BLOCK_WALK: StatePlay = StatePlay {
        state: 27,
        sequence: 1251,
    };
    const DRAWN_START: StatePlay = StatePlay {
        state: 26,
        sequence: 2205,
    };
    const DODGE_BACK: StatePlay = StatePlay {
        state: 7,
        sequence: 2100,
    };
    /// The blunderbuss's tags and its aim (retail newchar `19=2e 21=9706 48=28`).
    const BLUNDERBUSS: &str = "2H_Ranged,Blunderbuss";
    const AIM_STATE: u64 = 0x2e;
    const AIM_SEQUENCE: u64 = 407;
    const AIM_MARKER: u64 = 0x28;
    /// The blunderbuss's aimed shot (`18=5e 48=29`) and reload (`16=24 18=8142`) of the table.
    const SHOT_SEQUENCE: u64 = 94;
    const SHOT_MARKER: u64 = 41;
    const RELOAD_STATE: u64 = 36;
    const RELOAD_SEQUENCE: u64 = 4225;

    /// The retail follow-up: record 5568 started sequence 5055; the server followed with 4603.
    fn table() -> PlayerSequences {
        PlayerSequences::of_parts(
            vec![
                StanceSequences {
                    alt_start: Some(SlotsChange {
                        second_sequence: Some(BLOCK_SEQUENCE),
                        ..SlotsChange::default()
                    }),
                    alt_moves: Some(AltMoves {
                        moving: Some(BLOCK_WALK),
                        idle: None,
                    }),
                    drawn_moves: Some(DrawnMoves {
                        start: Some(DRAWN_START),
                        ..DrawnMoves::default()
                    }),
                    dodge: Some(Directional {
                        back: Some(DODGE_BACK),
                        ..Directional::default()
                    }),
                    ..greatsword_attacks()
                },
                StanceSequences {
                    cast_end_records: Some(CAST_END_RECORDS),
                    ..attacks(
                        LIFE_STAFF,
                        (AttackSlot::Second, CAST_START, CAST_STATE, CAST_MARKER),
                        CAST_LIGHT,
                        None,
                    )
                },
                StanceSequences {
                    tags: BLUNDERBUSS.to_owned(),
                    alt_start: Some(SlotsChange {
                        third_state: Some(AIM_STATE),
                        third_sequence: Some(AIM_SEQUENCE),
                        marker: Some(AIM_MARKER),
                        ..SlotsChange::default()
                    }),
                    alt_end: Some(SlotsChange {
                        third_state: Some(0),
                        third_sequence: Some(0),
                        marker: Some(0),
                        ..SlotsChange::default()
                    }),
                    alt_fire: Some(SlotsChange {
                        second_sequence: Some(SHOT_SEQUENCE),
                        marker: Some(SHOT_MARKER),
                        ..SlotsChange::default()
                    }),
                    reload: Some(SlotsChange {
                        second_state: Some(RELOAD_STATE),
                        second_sequence: Some(RELOAD_SEQUENCE),
                        ..SlotsChange::default()
                    }),
                    ..StanceSequences::default()
                },
            ],
            vec![FollowUp {
                client: 5055,
                server: 4603,
                at_release: false,
                seen: 13,
            }],
        )
    }

    fn weapon(tags: &str, drawn: bool) -> Option<HeldWeapon> {
        Some(HeldWeapon {
            stance: stance_key(tags),
            drawn,
            empty: false,
        })
    }

    fn attack_record(counter: u16) -> SlayerRecord {
        SlayerRecord {
            action: Some(SlayerInput {
                sequence: 5055,
                state: 0x000a,
                second_state: 0x002c,
                second_sequence: 0,
            }),
            intent: None,
            ..quiet_record(counter, Intent::default())
        }
    }

    fn quiet_record(counter: u16, intent: Intent) -> SlayerRecord {
        SlayerRecord {
            counter,
            frame_s: FRAME_S,
            action: None,
            intent: Some(intent),
            attack: AttackKey::default(),
            weapon: None,
            swimming: false,
            falling: false,
            posture: Posture::Stand,
        }
    }

    fn keyed(counter: u16, intent: Intent, attack: AttackKey, tags: &str) -> SlayerRecord {
        SlayerRecord {
            attack,
            weapon: weapon(tags, true),
            ..quiet_record(counter, intent)
        }
    }

    /// The own state of the tick numbered `number`.
    fn own_state(track: &mut SlayerTrack, number: u16) -> SlayerDue {
        track.advance(number, u64::from(number) * TICK_MS);
        track.take_due()
    }

    fn spawned() -> SlayerTrack {
        let mut track = SlayerTrack::default();
        track.spawned();
        track
    }

    const PRESS: AttackKey = AttackKey {
        pressed: true,
        released: false,
    };
    const RELEASE: AttackKey = AttackKey {
        pressed: false,
        released: true,
    };
    const HELD: AttackKey = AttackKey {
        pressed: false,
        released: false,
    };

    #[test]
    fn an_action_goes_out_started_in_its_record_then_its_follow_up() {
        const ACTION_RECORD: u16 = 0x15c0;
        const NEWEST: u16 = ACTION_RECORD + 1;
        let table = table();
        let mut track = spawned();
        track.record(attack_record(ACTION_RECORD), &table);
        track.record(quiet_record(NEWEST, Intent::default()), &table);
        let started = own_state(&mut track, NEWEST).state.expect("the action");
        assert_eq!(
            (started.state, started.started, started.sequence),
            (None, Some(ACTION_RECORD), 5055),
            "started in the record that named it; retail echoes no state without a second-slot \
             action"
        );
        let follow_up = own_state(&mut track, NEWEST).state.expect("the follow-up");
        assert_eq!(
            (follow_up.state, follow_up.started, follow_up.sequence),
            (None, Some(NEWEST), 4603)
        );
        assert_eq!(
            own_state(&mut track, NEWEST).state,
            Some(follow_up),
            "once more"
        );
        assert_eq!(own_state(&mut track, NEWEST).state, None);
        assert_eq!(
            track.remote_action(),
            Some(follow_up),
            "the others still see it: it goes on in the action's state"
        );
    }

    #[test]
    fn a_press_starts_the_press_start_and_its_release_the_light_attack() {
        // Retail great sword: `13=10 14=<press> 15=4120 48=1` in the press record, then the
        // light in retail's attack form at the release.
        let table = table();
        let mut track = spawned();
        let still = Intent::default();
        track.record(keyed(0x6ba, still, PRESS, GREATSWORD), &table);
        let due = own_state(&mut track, 0x6ba);
        let press = due.state.expect("the press start");
        assert_eq!(
            (press.state, press.started, press.sequence),
            (Some(PRESS_STATE), Some(0x6ba), PRESS_START)
        );
        assert_eq!(
            press.second.map(|second| (second.started, second.playing)),
            Some((0x6ba, None)),
            "the second slot starts with it (retail `17=<press>`)"
        );
        assert_eq!(
            due.slots.and_then(|slots| slots.marker),
            Some(PRESS_MARKER),
            "the server opens the chain with its marker"
        );
        track.record(keyed(0x6bb, still, HELD, GREATSWORD), &table);
        track.record(keyed(0x6bd, still, RELEASE, GREATSWORD), &table);
        let light = own_state(&mut track, 0x6bd)
            .state
            .expect("the light attack");
        assert_eq!(
            (light.state, light.started, light.sequence),
            (None, Some(0x6bd), LIGHT)
        );
        assert_eq!(
            light.second.map(|second| (second.started, second.playing)),
            Some((0x6bd, None))
        );
    }

    #[test]
    fn the_others_get_the_state_of_an_attack_that_starts_in_a_new_state() {
        // Retail remote flail (Savage Divide 1793443095058): `13=10 14=959 15=461 17=959` after
        // the player's ability in state 8; the own client named the start in row 1 and its own
        // state left 13 out.
        let table = table();
        let mut track = spawned();
        let walk = Intent {
            forward: 1,
            ..Intent::default()
        };
        track.record(quiet_record(200, walk), &table);
        let moving = own_state(&mut track, 200).state.expect("the move");
        assert_eq!(
            track.remote_action(),
            Some(moving),
            "the move has its state"
        );
        track.record(
            SlayerRecord {
                intent: Some(walk),
                ..attack_record(201)
            },
            &table,
        );
        let own = own_state(&mut track, 201)
            .state
            .expect("the client's attack");
        assert_eq!(own.state, None, "the own form leaves the state out");
        let seen = track.remote_action().expect("the others' form");
        assert_eq!(
            (seen.state, seen.started, seen.sequence),
            (
                Some(u64::from(attack_record(201).action.expect("row 1").state)),
                Some(201),
                5055
            ),
            "the others get the state the attack plays in"
        );
        let follow_up = own_state(&mut track, 201).state.expect("the follow-up");
        assert_eq!(
            track.remote_action(),
            Some(follow_up),
            "the follow-up goes on in that state: no state again"
        );
    }

    #[test]
    fn a_press_while_an_attack_runs_starts_no_new_chain() {
        let table = table();
        let mut track = spawned();
        let still = Intent::default();
        track.record(keyed(100, still, PRESS, GREATSWORD), &table);
        track.record(keyed(102, still, RELEASE, GREATSWORD), &table);
        let light = own_state(&mut track, 102).state.expect("the light");
        assert_eq!(light.sequence, LIGHT);
        own_state(&mut track, 102);
        let during = 102 + ATTACK_RUN_RECORDS - 1;
        track.record(keyed(during, still, PRESS, GREATSWORD), &table);
        assert_eq!(
            own_state(&mut track, during).state,
            None,
            "the light attack still runs"
        );
        track.record(keyed(during + 1, still, RELEASE, GREATSWORD), &table);
        let after = during + 1 + ATTACK_RUN_RECORDS;
        track.record(keyed(after, still, PRESS, GREATSWORD), &table);
        let press = own_state(&mut track, after).state.expect("a new chain");
        assert_eq!(press.sequence, PRESS_START);
    }

    #[test]
    fn a_chain_the_server_opened_ends_with_its_marker_cleared() {
        let table = table();
        let mut track = spawned();
        let still = Intent::default();
        track.record(keyed(100, still, PRESS, GREATSWORD), &table);
        track.record(keyed(102, still, RELEASE, GREATSWORD), &table);
        while own_state(&mut track, 102).slots.is_some() {}
        for counter in 103..102 + CHAIN_END_RECORDS {
            track.record(keyed(counter, still, HELD, GREATSWORD), &table);
        }
        assert_eq!(
            own_state(&mut track, 102 + CHAIN_END_RECORDS - 1).slots,
            None
        );
        track.record(
            keyed(102 + CHAIN_END_RECORDS, still, HELD, GREATSWORD),
            &table,
        );
        let end = own_state(&mut track, 102 + CHAIN_END_RECORDS)
            .slots
            .expect("the chain's end");
        assert_eq!(end.marker, Some(CLEARED_SLOT));
        assert_eq!(end.second, SlotChange::default(), "the marker alone");
    }

    #[test]
    fn a_staff_casts_in_the_second_slot_and_the_cast_ends() {
        // Retail life staff: `16=33 17=<press> 18=934 48=3` in the press record, child 18 alone
        // (140) at the release, `16=45 18=0 48=0` when no press followed.
        let table = table();
        let mut track = spawned();
        let still = Intent::default();
        track.record(keyed(200, still, PRESS, LIFE_STAFF), &table);
        let due = own_state(&mut track, 200);
        assert_eq!(due.state, None, "nothing in the first slot");
        let cast = due.slots.expect("the cast");
        assert_eq!(
            cast.second,
            SlotChange {
                state: Some(CAST_STATE),
                started: Some(200),
                sequence: Some(CAST_START),
            }
        );
        assert_eq!(cast.marker, Some(CAST_MARKER));
        while own_state(&mut track, 200).slots.is_some() {}
        track.record(keyed(203, still, RELEASE, LIFE_STAFF), &table);
        let light = own_state(&mut track, 203).slots.expect("the light");
        assert_eq!(
            light.second,
            SlotChange {
                state: None,
                started: None,
                sequence: Some(CAST_LIGHT),
            },
            "child 18 alone"
        );
        while own_state(&mut track, 203).slots.is_some() {}
        let end_record = 203 + CAST_END_RECORDS;
        for counter in 204..=end_record {
            track.record(keyed(counter, still, HELD, LIFE_STAFF), &table);
        }
        let end = own_state(&mut track, end_record).slots.expect("the end");
        assert_eq!(
            (end.second.state, end.second.sequence, end.marker),
            (Some(CAST_END_STATE), Some(CLEARED_SLOT), Some(CLEARED_SLOT))
        );
    }

    #[test]
    fn a_late_draw_does_not_replace_the_light_attack() {
        /// The draw the live table sends when the weapon reaches the hand.
        const DRAW_STATE: u64 = 44;
        const DRAW_SEQUENCE: u64 = 2310;
        let table = PlayerSequences::of_parts(
            vec![StanceSequences {
                draw: Some(StatePlay {
                    state: DRAW_STATE,
                    sequence: DRAW_SEQUENCE,
                }),
                ..greatsword_attacks()
            }],
            vec![],
        );
        let mut track = spawned();
        let still = Intent::default();
        let sheathed = |counter, attack| SlayerRecord {
            weapon: weapon(GREATSWORD, false),
            ..keyed(counter, still, attack, GREATSWORD)
        };
        track.record(sheathed(0x6ba, PRESS), &table);
        track.record(sheathed(0x6bd, RELEASE), &table);
        let light = own_state(&mut track, 0x6bd)
            .state
            .expect("the light attack");
        assert_eq!(light.sequence, LIGHT);
        track.record(
            SlayerRecord {
                weapon: weapon(GREATSWORD, true),
                ..quiet_record(0x6be, still)
            },
            &table,
        );
        let after = own_state(&mut track, 0x6be)
            .state
            .expect("the attack stays");
        assert_eq!(after.sequence, LIGHT, "a draw after the tap is not a walk");
    }

    #[test]
    fn a_hold_starts_the_charge_and_its_release_no_light() {
        let table = table();
        let mut track = spawned();
        let still = Intent::default();
        track.record(keyed(100, still, PRESS, GREATSWORD), &table);
        let press = own_state(&mut track, 100).state.expect("the press start");
        assert_eq!(press.sequence, PRESS_START);
        while own_state(&mut track, 100).state.is_some() {}
        for counter in 101..100 + HOLD_RECORDS {
            track.record(keyed(counter, still, HELD, GREATSWORD), &table);
        }
        assert_eq!(own_state(&mut track, 100 + HOLD_RECORDS - 1).state, None);
        track.record(keyed(100 + HOLD_RECORDS, still, HELD, GREATSWORD), &table);
        let charge = own_state(&mut track, 100 + HOLD_RECORDS)
            .state
            .expect("the charge");
        assert_eq!(
            (charge.started, charge.sequence),
            (Some(100 + HOLD_RECORDS), CHARGE)
        );
        own_state(&mut track, 100 + HOLD_RECORDS);
        track.record(
            keyed(100 + HOLD_RECORDS + 5, still, RELEASE, GREATSWORD),
            &table,
        );
        assert_eq!(
            own_state(&mut track, 100 + HOLD_RECORDS + 5).state,
            None,
            "the heavy attack is the client's"
        );
    }

    #[test]
    fn a_tap_the_client_answered_gets_no_light_attack() {
        let table = table();
        let mut track = spawned();
        let still = Intent::default();
        track.record(keyed(10, still, PRESS, GREATSWORD), &table);
        track.record(
            SlayerRecord {
                weapon: weapon(GREATSWORD, true),
                ..attack_record(11)
            },
            &table,
        );
        track.record(keyed(12, still, RELEASE, GREATSWORD), &table);
        let due = own_state(&mut track, 12).state.expect("the client's");
        assert_eq!(due.sequence, 5055);
    }

    #[test]
    fn the_right_click_blocks_and_aims_and_ends_at_the_release() {
        let table = table();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let mut track = spawned();
        track.record(keyed(20, block, HELD, GREATSWORD), &table);
        let due = own_state(&mut track, 20);
        let slots = due.slots.expect("the block");
        assert_eq!(slots.second.sequence, Some(BLOCK_SEQUENCE));
        assert_eq!(slots.second.started, None, "retail sends child 18 alone");
        assert_eq!(own_state(&mut track, 20).slots, Some(slots), "once more");
        assert_eq!(own_state(&mut track, 20).slots, None);
        let mut track = spawned();
        track.record(keyed(0x54ad, block, HELD, BLUNDERBUSS), &table);
        let aim = own_state(&mut track, 0x54ad).slots.expect("the aim");
        assert_eq!(
            aim.third,
            SlotChange {
                state: Some(AIM_STATE),
                started: Some(0x54ad),
                sequence: Some(AIM_SEQUENCE),
            }
        );
        assert_eq!(aim.marker, Some(AIM_MARKER));
        track.record(keyed(0x54b0, Intent::default(), HELD, BLUNDERBUSS), &table);
        let end = own_state(&mut track, 0x54b0).slots.expect("the end");
        assert_eq!(
            (
                end.third.state,
                end.third.started,
                end.third.sequence,
                end.marker
            ),
            (Some(0), Some(0x54b0), Some(0), Some(0))
        );
        let mut sheathed = spawned();
        sheathed.record(
            SlayerRecord {
                weapon: weapon(BLUNDERBUSS, false),
                ..quiet_record(5, block)
            },
            &table,
        );
        assert_eq!(
            own_state(&mut sheathed, 5).slots,
            None,
            "no weapon in the hand"
        );
    }

    #[test]
    fn the_players_who_see_one_get_its_upper_slots_while_its_state_echoes_them() {
        let table = table();
        let aim = Intent {
            block: true,
            ..Intent::default()
        };
        let mut track = spawned();
        track.record(keyed(0x54ad, aim, HELD, BLUNDERBUSS), &table);
        let own = own_state(&mut track, 0x54ad).slots.expect("the aim");
        let remote = track
            .remote_upper()
            .expect("kept after the own state took it");
        assert_eq!(remote.slots, own);
        assert!(remote.after_state, "the aim came after the newest state");
        own_state(&mut track, 0x54ad);
        assert_eq!(
            track.remote_upper().map(|upper| upper.slots),
            Some(own),
            "the echo"
        );
        own_state(&mut track, 0x54ad);
        assert_eq!(track.remote_upper(), None, "the echo is over");
        track.record(attack_record(0x54b0), &table);
        own_state(&mut track, 0x54b0);
        track.record(keyed(0x54b1, Intent::default(), HELD, BLUNDERBUSS), &table);
        own_state(&mut track, 0x54b1);
        assert!(
            track.remote_upper().is_some_and(|upper| upper.after_state),
            "the aim's end came after the attack"
        );
    }

    #[test]
    fn an_aimed_press_shoots_and_the_first_press_after_the_reload_time_reloads() {
        let table = table();
        let aim = Intent {
            block: true,
            ..Intent::default()
        };
        let gun = |counter: u16, attack: AttackKey, empty: bool| SlayerRecord {
            weapon: Some(HeldWeapon {
                empty,
                ..weapon(BLUNDERBUSS, true).expect("a gun")
            }),
            ..keyed(counter, aim, attack, BLUNDERBUSS)
        };
        let mut track = spawned();
        track.record(gun(0x5550, HELD, false), &table);
        own_state(&mut track, 0x5550);
        own_state(&mut track, 0x5550);
        track.record(gun(0x5551, PRESS, false), &table);
        let shot = own_state(&mut track, 0x5551).slots.expect("the aimed shot");
        assert_eq!(
            (
                shot.second.state,
                shot.second.started,
                shot.second.sequence,
                shot.marker
            ),
            (None, None, Some(SHOT_SEQUENCE), Some(SHOT_MARKER)),
            "retail `18=5e 48=29`: child 18 alone in the second slot"
        );
        own_state(&mut track, 0x5551);
        track.record(gun(0x5552, RELEASE, true), &table);
        track.record(gun(0x5553, PRESS, true), &table);
        assert_eq!(own_state(&mut track, 0x5553).slots, None, "empty: nothing");
        track.record(gun(0x5554, RELEASE, false), &table);
        assert_eq!(
            own_state(&mut track, 0x5554).slots,
            None,
            "a load is no reload"
        );
        track.record(gun(0x557a, PRESS, false), &table);
        let reload = own_state(&mut track, 0x557a).slots.expect("the reload");
        assert_eq!(
            (
                reload.second.state,
                reload.second.started,
                reload.second.sequence
            ),
            (Some(RELOAD_STATE), Some(0x557a), Some(RELOAD_SEQUENCE))
        );
        own_state(&mut track, 0x557a);
        track.record(gun(0x557b, RELEASE, false), &table);
        assert_eq!(
            own_state(&mut track, 0x557b)
                .state
                .map(|state| state.started),
            None,
            "the reload's release starts no attack"
        );
        track.record(gun(0x5584, PRESS, false), &table);
        let again = own_state(&mut track, 0x5584).slots.expect("shoots again");
        assert_eq!(again.second.sequence, Some(SHOT_SEQUENCE));
        let mut hip = spawned();
        hip.record(keyed(0x5600, Intent::default(), PRESS, BLUNDERBUSS), &table);
        assert_eq!(
            own_state(&mut hip, 0x5600).slots,
            None,
            "no aimed shot from the hip"
        );
    }

    #[test]
    fn no_attack_starts_while_aiming() {
        let table = table();
        let aim = Intent {
            block: true,
            ..Intent::default()
        };
        let mut track = spawned();
        track.record(keyed(30, aim, PRESS, GREATSWORD), &table);
        track.record(keyed(31, aim, RELEASE, GREATSWORD), &table);
        let due = own_state(&mut track, 31);
        assert_ne!(due.state.map(|state| state.sequence), Some(LIGHT));
    }

    #[test]
    fn a_swap_draws_the_new_weapon_and_a_sheathe_puts_it_away() {
        const SWORD_DRAW: StatePlay = StatePlay {
            state: 44,
            sequence: 2310,
        };
        const GUN_DRAW: StatePlay = StatePlay {
            state: 44,
            sequence: 3250,
        };
        let draws = |tags: &str, draw| StanceSequences {
            tags: tags.to_owned(),
            draw: Some(draw),
            ..StanceSequences::default()
        };
        let table = PlayerSequences::of_parts(
            vec![draws(GREATSWORD, SWORD_DRAW), draws(BLUNDERBUSS, GUN_DRAW)],
            Vec::new(),
        );
        let still = Intent::default();
        let second = |track: &mut SlayerTrack, number: u16| {
            own_state(track, number).slots.map(|slots| slots.second)
        };
        let drawn = |sequence: u64, started: u16| {
            Some(SlotChange {
                state: Some(SWORD_DRAW.state),
                started: Some(started),
                sequence: Some(sequence),
            })
        };
        let mut track = spawned();
        track.record(keyed(10, still, HELD, GREATSWORD), &table);
        assert_eq!(
            second(&mut track, 10),
            drawn(SWORD_DRAW.sequence, 10),
            "the draw"
        );
        for number in 11..20 {
            track.record(keyed(number, still, HELD, GREATSWORD), &table);
            own_state(&mut track, number);
        }
        track.record(keyed(20, still, HELD, BLUNDERBUSS), &table);
        assert_eq!(
            second(&mut track, 20),
            drawn(GUN_DRAW.sequence, 20),
            "the other weapon's draw at a swap (retail: every weapon key)"
        );
        for number in 21..30 {
            track.record(keyed(number, still, HELD, BLUNDERBUSS), &table);
            own_state(&mut track, number);
        }
        let away = SlayerRecord {
            weapon: weapon(BLUNDERBUSS, false),
            ..quiet_record(30, still)
        };
        track.record(away, &table);
        assert_eq!(
            second(&mut track, 30),
            Some(SlotChange {
                state: Some(sheathe::STATE),
                started: Some(30),
                sequence: Some(sheathe::SEQUENCE),
            }),
            "the sheathe (retail `16=2c 18=9b2c`)"
        );
    }

    #[test]
    fn the_drawn_weapon_walks_and_dodges_with_its_stance() {
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        track.record(keyed(40, forward, HELD, GREATSWORD), &table);
        let start = own_state(&mut track, 40).state.expect("a start");
        assert_eq!(
            (start.state, start.sequence),
            (Some(DRAWN_START.state), DRAWN_START.sequence)
        );
        let dodge = Intent {
            dodge: true,
            ..Intent::of_keys(false, true, false, false)
        };
        let mut track = spawned();
        track.record(keyed(50, dodge, HELD, GREATSWORD), &table);
        let roll = own_state(&mut track, 50).state.expect("the dodge");
        assert_eq!(
            (roll.state, roll.started, roll.sequence),
            (Some(DODGE_BACK.state), Some(50), DODGE_BACK.sequence)
        );
        track.record(keyed(51, dodge, HELD, GREATSWORD), &table);
        own_state(&mut track, 51);
        assert_ne!(
            own_state(&mut track, 51)
                .state
                .and_then(|state| state.started),
            Some(51),
            "a held key dodges once"
        );
        let block_walk = Intent {
            block: true,
            ..forward
        };
        let mut track = spawned();
        track.record(keyed(60, block_walk, HELD, GREATSWORD), &table);
        let walk = own_state(&mut track, 60).state.expect("the block walk");
        assert_eq!(
            (walk.state, walk.sequence),
            (Some(BLOCK_WALK.state), BLOCK_WALK.sequence)
        );
    }

    #[test]
    fn the_sequence_time_counts_the_records_since_the_action() {
        // Two records after the action's record: 3 frames in, from an absolute part of one.
        let table = table();
        let mut track = spawned();
        track.record(attack_record(10), &table);
        track.record(quiet_record(11, Intent::default()), &table);
        track.record(quiet_record(12, Intent::default()), &table);
        let time = own_state(&mut track, 12).time.expect("time");
        let mut clock = SequenceClock::default();
        assert_eq!(
            time,
            clock.time(3.0 * FRAME_S, Some(FRAME_S), 12 * TICK_MS),
            "{time:?}"
        );
        assert!(time.abs.is_some(), "a start sends its absolute part");
    }

    #[test]
    fn no_time_goes_before_the_first_own_state() {
        let table = table();
        let mut track = SlayerTrack::default();
        track.record(quiet_record(1, Intent::default()), &table);
        assert_eq!(own_state(&mut track, 1), SlayerDue::default());
        let mut track = spawned();
        track.record(quiet_record(3, Intent::default()), &table);
        assert_eq!(
            own_state(&mut track, 3).time.map(|time| time.rel),
            Some(0x83),
            "the first state's two steps and one more record"
        );
    }

    #[test]
    fn moving_starts_stops_and_comes_to_rest_with_the_retail_sequences() {
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut counter = 100;
        let mut step = |track: &mut SlayerTrack, intent: Intent| {
            counter += 1;
            track.record(quiet_record(counter, intent), &table);
            own_state(track, counter).state
        };
        let start = step(&mut track, forward).expect("a start");
        assert_eq!(
            (start.state, start.sequence),
            (Some(movement::START_STATE), movement::START_SEQUENCE)
        );
        // The echo, then nothing while running.
        assert_eq!(step(&mut track, forward), Some(start));
        assert_eq!(step(&mut track, forward), None);
        let stop = step(&mut track, Intent::default()).expect("a stop");
        assert_eq!(
            (stop.state, stop.sequence),
            (Some(movement::MOVE_STATE), movement::STOP_SEQUENCE)
        );
        let idle = (0..movement::STOP_RECORDS)
            .find_map(|_| {
                step(&mut track, Intent::default())
                    .filter(|state| state.state == Some(movement::IDLE_STATE))
            })
            .expect("standing still");
        assert_eq!(idle.sequence, movement::IDLE_SEQUENCE);
        assert_eq!(
            idle.started
                .zip(stop.started)
                .map(|(idle, stop)| idle.wrapping_sub(stop)),
            Some(movement::STOP_RECORDS)
        );
        let back = step(&mut track, Intent::of_keys(false, true, false, false)).expect("back");
        assert_eq!(
            (back.state, back.sequence),
            (Some(movement::MOVE_STATE), movement::BACK_START_SEQUENCE)
        );
        step(&mut track, Intent::default());
        let again = step(&mut track, forward).expect("moving again");
        assert_eq!(again.sequence, movement::RESTART_SEQUENCE);
    }

    #[test]
    fn the_movement_sequences_are_the_retail_bytes() {
        for (sequence, retail) in [
            (movement::START_SEQUENCE, [0xaa, 0x2b]),
            (movement::BACK_START_SEQUENCE, [0x98, 0x14]),
            (movement::STOP_SEQUENCE, [0xbd, 0x15]),
            (movement::RESTART_SEQUENCE, [0xb6, 0x25]),
            (movement::IDLE_SEQUENCE, [0xb9, 0x10]),
            (swim::IDLE_SEQUENCE, [0xa0, 0x21]),
            (swim::FLOAT_SEQUENCE, [0xb6, 0x21]),
            (swim::STROKE_SEQUENCE, [0xa5, 0x2e]),
            (swim::NEXT_STROKE_SEQUENCE, [0x97, 0x2e]),
            (swim::FALL_SEQUENCE, [0x9d, 0x3c]),
            (loadout::OPEN_SEQUENCE, [0x96, 0x2b]),
            (loadout::LOOP_SEQUENCE, [0x89, 0x0a]),
            (loadout::CLOSE_SEQUENCE, [0x8b, 0x2e]),
        ] {
            assert_eq!(sequence_bytes(sequence), retail, "{sequence}");
        }
    }

    /// A sequence as the wire carries it (child 15).
    fn sequence_bytes(sequence: u64) -> Vec<u8> {
        nw_messages::wire::Writer::new()
            .prefixed(sequence)
            .expect("prefix")
            .finish()
    }

    /// Reads the next record of a player in `posture` with keys `intent`: the record's counter
    /// and the slayer state of its own state.
    fn posture_step(
        track: &mut SlayerTrack,
        counter: &mut u16,
        intent: Intent,
        posture: Posture,
    ) -> (u16, Option<SlayerState>) {
        *counter += 1;
        track.record(
            SlayerRecord {
                posture,
                ..quiet_record(*counter, intent)
            },
            &table(),
        );
        (*counter, own_state(track, *counter).state)
    }

    /// The first state within `records` records of `intent` in `posture` that `wanted` takes,
    /// with its record's counter.
    fn posture_state(
        track: &mut SlayerTrack,
        counter: &mut u16,
        (intent, posture): (Intent, Posture),
        records: u16,
        wanted: impl Fn(&SlayerState) -> bool,
    ) -> Option<(u16, SlayerState)> {
        (0..records).find_map(|_| {
            let (at, state) = posture_step(track, counter, intent, posture);
            state.filter(&wanted).map(|state| (at, state))
        })
    }

    #[test]
    fn a_crouch_and_its_moves_play_the_retail_sequences() {
        // Retail `arvaysa_walk_running_crouch_prone_2`: the toggle at 72411 ms, `15=8913` 34 ms
        // later, `15=9e3e` 548 ms after that; the moves from 37448 ms.
        const TOGGLE: u16 = 0x7ac;
        let still = Intent::default();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut counter = TOGGLE;
        // The record after the toggle knows the crouch.
        let (_, change) = posture_step(&mut track, &mut counter, still, Posture::Crouch);
        let change = change.expect("the change to a crouch");
        assert_eq!(
            (change.state, change.started),
            (None, None),
            "child 15 alone"
        );
        assert_eq!(sequence_bytes(change.sequence), [0x89, 0x13]);
        assert_eq!(track.remote_action(), Some(change), "the others see it");
        let (at, settled) = posture_state(
            &mut track,
            &mut counter,
            (still, Posture::Crouch),
            20,
            |state| state.sequence != change.sequence,
        )
        .expect("the crouch's stand");
        assert_eq!((settled.state, settled.started), (None, None));
        assert_eq!(sequence_bytes(settled.sequence), [0x9e, 0x3e]);
        assert_eq!(at - TOGGLE, 16, "records from the toggle");
        let (_, start) = posture_step(&mut track, &mut counter, forward, Posture::Crouch);
        let start = start.expect("a crouching start");
        assert_eq!(start.state, Some(movement::START_STATE));
        assert_eq!(sequence_bytes(start.sequence), [0xb6, 0x1c]);
        posture_step(&mut track, &mut counter, forward, Posture::Crouch);
        let (_, stop) = posture_step(&mut track, &mut counter, still, Posture::Crouch);
        let stop = stop.expect("a crouching stop");
        assert_eq!(stop.state, Some(movement::MOVE_STATE));
        assert_eq!(sequence_bytes(stop.sequence), [0xac, 0x30]);
        let (_, idle) = posture_state(
            &mut track,
            &mut counter,
            (still, Posture::Crouch),
            12,
            |state| state.state == Some(movement::IDLE_STATE),
        )
        .expect("crouching still");
        assert_eq!(sequence_bytes(idle.sequence), [0x9e, 0x3e]);
        assert_eq!(
            idle.started
                .zip(stop.started)
                .map(|(idle, stop)| idle.wrapping_sub(stop)),
            Some(10),
            "retail: the stop at 04ac, still at 04b6"
        );
        // Standing up: `15=a13c`, then the stand's `15=b910`.
        let (_, up) = posture_step(&mut track, &mut counter, still, Posture::Stand);
        assert_eq!(
            up.map(|state| sequence_bytes(state.sequence)),
            Some(vec![0xa1, 0x3c])
        );
        let (_, stand) = posture_state(
            &mut track,
            &mut counter,
            (still, Posture::Stand),
            20,
            |state| state.sequence == movement::IDLE_SEQUENCE,
        )
        .expect("standing still");
        assert_eq!(stand.state, None);
    }

    #[test]
    fn a_change_to_prone_holds_the_start_of_a_move() {
        // Retail `arvaysa_walk_running_crouch_prone_2`: the toggle at 0530, the forward key from
        // 0531, `15=912f`, then `13=1a 14=0544 15=971c`.
        const TOGGLE: u16 = 0x530;
        let still = Intent::default();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut counter = TOGGLE - 40;
        for _ in 0..40 {
            posture_step(&mut track, &mut counter, still, Posture::Crouch);
        }
        assert_eq!(counter, TOGGLE);
        let (_, change) = posture_step(&mut track, &mut counter, forward, Posture::Prone);
        assert_eq!(
            change.map(|state| (state.state, sequence_bytes(state.sequence))),
            Some((None, vec![0x91, 0x2f]))
        );
        let (_, start) = posture_state(
            &mut track,
            &mut counter,
            (forward, Posture::Prone),
            25,
            |state| state.state == Some(movement::START_STATE),
        )
        .expect("the prone start");
        assert_eq!(sequence_bytes(start.sequence), [0x97, 0x1c]);
        assert_eq!(start.started, Some(TOGGLE + 20));
        // The start shows the posture: no sequence of its own follows.
        posture_step(&mut track, &mut counter, forward, Posture::Prone);
        for _ in 0..30 {
            let (at, state) = posture_step(&mut track, &mut counter, forward, Posture::Prone);
            assert_eq!(state, None, "record {at:#x}");
        }
        // Prone to standing while it moves: `15=a03c`, then the stand's move.
        let (_, up) = posture_step(&mut track, &mut counter, forward, Posture::Stand);
        assert_eq!(
            up.map(|state| sequence_bytes(state.sequence)),
            Some(vec![0xa0, 0x3c])
        );
    }

    #[test]
    fn a_moving_player_crouches_and_stands_up_without_a_stop() {
        // Retail `arvaysa_walk_running_crouch_prone`: the forward key held, the toggle at
        // 38240 ms, `15=a51b` 52 ms later, `15=b61c` 535 ms after that.
        const TOGGLE: u16 = 0x39f;
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut counter = TOGGLE - 1;
        let (_, start) = posture_step(&mut track, &mut counter, forward, Posture::Stand);
        assert_eq!(
            start.map(|state| state.sequence),
            Some(movement::START_SEQUENCE)
        );
        let (_, change) = posture_step(&mut track, &mut counter, forward, Posture::Crouch);
        let change = change.expect("the change to a crouch");
        assert_eq!(
            (change.state, sequence_bytes(change.sequence)),
            (None, vec![0xa5, 0x1b])
        );
        let (at, settled) = posture_state(
            &mut track,
            &mut counter,
            (forward, Posture::Crouch),
            20,
            |state| state.sequence != change.sequence,
        )
        .expect("the crouching move");
        assert_eq!(
            (settled.state, sequence_bytes(settled.sequence)),
            (None, vec![0xb6, 0x1c])
        );
        assert_eq!(at - TOGGLE, 16, "records from the toggle");
        // Standing up while moving: the stand's move at once.
        let (_, up) = posture_step(&mut track, &mut counter, forward, Posture::Stand);
        assert_eq!(
            up.map(|state| (state.state, state.sequence)),
            Some((None, movement::RESTART_SEQUENCE))
        );
        for _ in 0..30 {
            let (_, state) = posture_step(&mut track, &mut counter, forward, Posture::Stand);
            assert!(state.is_none_or(|state| state.sequence == movement::RESTART_SEQUENCE));
        }
    }

    #[test]
    fn the_server_sends_the_fall_the_entry_and_the_swim_strokes_like_retail() {
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut counter = 200;
        let mut step = |track: &mut SlayerTrack, intent: Intent, swimming, falling| {
            counter += 1;
            track.record(
                SlayerRecord {
                    swimming,
                    falling,
                    ..quiet_record(counter, intent)
                },
                &table,
            );
            own_state(track, counter).state
        };
        let fall = step(&mut track, Intent::default(), false, true).expect("a fall");
        assert_eq!(
            (fall.state, fall.sequence),
            (Some(swim::FALL_STATE), swim::FALL_SEQUENCE)
        );
        assert_eq!(
            step(&mut track, Intent::default(), false, true),
            Some(fall),
            "the fall echoes, it does not start again"
        );
        let entry = step(&mut track, Intent::default(), true, false).expect("the entry");
        assert_eq!(
            (entry.state, entry.sequence),
            (Some(swim::STATE), swim::IDLE_SEQUENCE)
        );
        // Each change after the entry is the sequence alone: no state, no start (child 15 only).
        let mut next_change = |track: &mut SlayerTrack, intent: Intent, last: u64| {
            for record in 1..1_000_u32 {
                let state = step(track, intent, true, false);
                if let Some(state) = state.filter(|state| state.sequence != last) {
                    assert_eq!((state.state, state.started), (None, None), "{state:?}");
                    #[allow(clippy::cast_precision_loss, reason = "a few hundred records")]
                    return (state.sequence, record as f32 * FRAME_S);
                }
            }
            panic!("no change after {last}");
        };
        let (float, after_s) = next_change(&mut track, Intent::default(), swim::IDLE_SEQUENCE);
        assert_eq!(float, swim::FLOAT_SEQUENCE);
        assert!((after_s - swim::ENTRY_S).abs() <= FRAME_S, "{after_s}");
        let (stroke, at_s) = next_change(&mut track, forward, float);
        assert_eq!(
            (stroke, at_s),
            (swim::STROKE_SEQUENCE, FRAME_S),
            "at the press"
        );
        let (next, after_s) = next_change(&mut track, forward, stroke);
        assert_eq!(next, swim::NEXT_STROKE_SEQUENCE);
        assert!((after_s - swim::STROKE_S).abs() <= FRAME_S, "{after_s}");
        let (again, after_s) = next_change(&mut track, forward, next);
        assert_eq!(again, swim::STROKE_SEQUENCE);
        assert!(
            (after_s - swim::NEXT_STROKE_S).abs() <= FRAME_S,
            "{after_s}"
        );
        let (stop, at_s) = next_change(&mut track, Intent::default(), again);
        assert_eq!(
            (stop, at_s),
            (swim::FLOAT_SEQUENCE, FRAME_S),
            "at the release"
        );
        let out = step(&mut track, forward, false, false).expect("a walk out of the water");
        assert!(
            out.state.is_some(),
            "on land the movement states come back: {out:?}"
        );
    }

    #[test]
    fn a_hit_reaction_starts_clears_the_slots_holds_the_keys_and_ends_like_retail() {
        // Retail newchar 2026-09-18, the great sword's `2960`: `13=04 14=1e02 15=902e 16=00
        // 17=1e02 48=00`, 17 records, then `16=2d 17=1e13`; the hit came 20 ms before the start.
        const STARTED: u16 = 0x1e02;
        const SEQUENCE: u64 = 2960;
        const LENGTH_RECORDS: u16 = 17;
        const HIT_BEFORE_MS: u64 = 20;
        let mut track = spawned();
        track.chain_marked = true;
        track.slots_on.second = true;
        let length_ms = u64::from(LENGTH_RECORDS) * TICK_MS;
        let hit_ms = u64::from(STARTED) * TICK_MS - HIT_BEFORE_MS;
        track.reacted(SEQUENCE, hit_ms, length_ms, length_ms);
        let start = own_state(&mut track, STARTED);
        let reaction = pulled_state(STARTED, SEQUENCE);
        assert_eq!(start.state, Some(reaction));
        let clears = start.slots.expect("the slots it clears");
        assert_eq!(clears.marker, Some(CLEARED_SLOT), "48=00");
        assert_eq!(clears.second.sequence, Some(CLEARED_SLOT), "18=00");
        assert_eq!(
            clears.third,
            SlotChange::default(),
            "the third slot did not play"
        );
        // The client's row 1 of the reaction, a held move key and attack presses start nothing.
        let walk = Intent {
            forward: 1,
            ..Intent::default()
        };
        let row = SlayerInput {
            sequence: SEQUENCE,
            state: u16::try_from(pull::STATE).expect("state"),
            second_state: u16::MAX,
            second_sequence: 0,
        };
        for counter in STARTED..STARTED + LENGTH_RECORDS {
            track.record(
                SlayerRecord {
                    action: (counter == STARTED).then_some(row),
                    attack: PRESS,
                    ..quiet_record(counter, walk)
                },
                &table(),
            );
            let due = own_state(&mut track, counter + 1);
            if counter + 1 < STARTED + LENGTH_RECORDS {
                assert!(
                    due.state.is_none_or(|state| state == reaction),
                    "record {counter}: {due:?}"
                );
            } else {
                let end = due.slots.expect("the end");
                assert_eq!(
                    end.second,
                    SlotChange {
                        state: Some(CAST_END_STATE),
                        started: Some(STARTED + LENGTH_RECORDS),
                        sequence: None,
                    },
                    "16=2d 17=1e13"
                );
            }
        }
        // Without keys the player stands still in the state after the end.
        track.record(
            quiet_record(STARTED + LENGTH_RECORDS, Intent::default()),
            &table(),
        );
        let after = own_state(&mut track, STARTED + LENGTH_RECORDS + 1).state;
        assert_eq!(
            after.map(|state| (state.state, state.sequence)),
            Some((Some(movement::IDLE_STATE), movement::IDLE_SEQUENCE))
        );
    }

    #[test]
    fn a_hit_reaction_lets_a_held_key_move_the_player_again_at_its_free_point() {
        const STARTED: u16 = 100;
        const FREE_RECORDS: u16 = 10;
        let mut track = spawned();
        let walk = Intent {
            forward: 1,
            ..Intent::default()
        };
        track.record(quiet_record(STARTED - 2, walk), &table());
        own_state(&mut track, STARTED - 1);
        let free_ms = u64::from(FREE_RECORDS) * TICK_MS;
        track.reacted(
            pull::SEQUENCE,
            u64::from(STARTED) * TICK_MS - 1,
            free_ms,
            2 * free_ms,
        );
        own_state(&mut track, STARTED);
        let mut moved = None;
        for counter in STARTED..STARTED + 2 * FREE_RECORDS {
            track.record(quiet_record(counter, walk), &table());
            let due = own_state(&mut track, counter + 1);
            if moved.is_none()
                && let Some(state) = due.state.filter(|state| state.state != Some(pull::STATE))
            {
                moved = Some((counter, state.state));
            }
        }
        assert_eq!(
            moved,
            Some((STARTED + FREE_RECORDS, Some(movement::MOVE_STATE))),
            "the held key moves it again from the free point, in the move state (13=1b)"
        );
    }

    #[test]
    fn a_pull_plays_the_pulled_state_then_stands_the_player_still() {
        /// The retail pull's own state number (`14=3163`), as this test's tick number.
        const PULLED_IN: u16 = 0x3163;
        let mut track = spawned();
        let pulled_at_ms = u64::from(PULLED_IN - 1) * TICK_MS;
        track.pulled(pulled_at_ms);
        let state = own_state(&mut track, PULLED_IN).state;
        assert_eq!(state, Some(pulled_state(PULLED_IN, pull::SEQUENCE)));
        assert_eq!(
            own_state(&mut track, PULLED_IN + 1).state,
            state,
            "it echoes once"
        );
        let ends_at = PULLED_IN - 1 + u16::try_from(pull::LENGTH_MS / TICK_MS).expect("ticks") + 1;
        let mut idle = None;
        for number in PULLED_IN + 2..=ends_at {
            idle = idle.or(own_state(&mut track, number).state);
        }
        assert_eq!(
            idle.map(|idle| (idle.state, idle.sequence)),
            Some((Some(movement::IDLE_STATE), movement::IDLE_SEQUENCE)),
            "standing still once the pull is over"
        );
    }

    #[test]
    fn a_fast_travel_kneels_as_an_unstuck_and_a_key_moves_again() {
        /// The retail kneel's own state number (`14=3fcc`), as this test's tick number.
        const KNEELS_IN: u16 = 0x3fcc;
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        // A stop in progress must not stand the player over the kneel.
        track.record(quiet_record(KNEELS_IN - 3, forward), &table);
        own_state(&mut track, KNEELS_IN - 3);
        track.record(quiet_record(KNEELS_IN - 2, Intent::default()), &table);
        own_state(&mut track, KNEELS_IN - 2);
        own_state(&mut track, KNEELS_IN - 1);
        track.kneel();
        let kneel = own_state(&mut track, KNEELS_IN).state;
        assert_eq!(
            kneel,
            Some(SlayerState {
                state: Some(kneel::STATE),
                started: Some(KNEELS_IN),
                sequence: kneel::SEQUENCE,
                second: None,
            })
        );
        assert_eq!(
            own_state(&mut track, KNEELS_IN + 1).state,
            kneel,
            "it echoes once"
        );
        for number in KNEELS_IN + 2..KNEELS_IN + 2 + movement::STOP_RECORDS {
            track.record(quiet_record(number, Intent::default()), &table);
            assert_eq!(
                own_state(&mut track, number).state,
                None,
                "kneeling at {number}"
            );
        }
        let moved_in = KNEELS_IN + 2 + movement::STOP_RECORDS;
        track.record(quiet_record(moved_in, forward), &table);
        let moved = own_state(&mut track, moved_in).state.expect("a start");
        assert_eq!(
            (moved.state, moved.sequence),
            (Some(movement::START_STATE), movement::START_SEQUENCE)
        );
    }

    #[test]
    fn the_end_of_a_channel_stands_the_player_still_unless_it_moves() {
        /// The retail arrival's state (`13=1f 14=409b 15=b910 16=2d 17=409b 18=00`).
        const STANDS_IN: u16 = 0x409b;
        let table = table();
        let mut track = spawned();
        track.kneel();
        own_state(&mut track, STANDS_IN - 2);
        track.stand();
        assert_eq!(
            own_state(&mut track, STANDS_IN).state,
            Some(SlayerState {
                state: Some(movement::IDLE_STATE),
                started: Some(STANDS_IN),
                sequence: movement::IDLE_SEQUENCE,
                second: Some(SecondSlayerState {
                    started: STANDS_IN,
                    playing: Some((loadout::CLOSED_STATE, 0)),
                    stopped: false,
                }),
            })
        );
        track.stand();
        let moved_in = STANDS_IN + 5;
        track.record(
            quiet_record(moved_in, Intent::of_keys(true, false, false, false)),
            &table,
        );
        let moved = own_state(&mut track, moved_in).state.expect("a start");
        assert_eq!(
            moved.state,
            Some(movement::START_STATE),
            "a cancel by a key moves"
        );
    }

    #[test]
    fn the_inventory_slot_opens_loops_closes_and_clears_like_retail() {
        const OPENED_IN: u16 = 0x11a9;
        const CLOSED_IN: u16 = 0x1264;
        const CLEARED_IN: u16 = 0x1290;
        let mut track = spawned();
        let mut number = 1;
        let mut tick = |track: &mut SlayerTrack| {
            number += 1;
            own_state(track, number).slots.map(|slots| {
                (
                    slots.second.state,
                    slots.second.started,
                    slots.second.sequence,
                )
            })
        };
        let open = Some((
            Some(loadout::OPEN_STATE),
            Some(OPENED_IN),
            Some(loadout::OPEN_SEQUENCE),
        ));
        track.open_loadout(OPENED_IN, 2 * TICK_MS);
        assert_eq!([tick(&mut track), tick(&mut track)], [open, open]);
        assert_eq!(tick(&mut track), None, "the open echoes, then stops");
        let mut looping = None;
        for _ in 0..loadout::LOOP_MS / TICK_MS + 1 {
            looping = looping.or(tick(&mut track));
        }
        assert_eq!(
            looping,
            Some((
                Some(loadout::OPEN_STATE),
                Some(OPENED_IN),
                Some(loadout::LOOP_SEQUENCE)
            ))
        );
        track.close_loadout(CLOSED_IN);
        let close = Some((None, Some(CLOSED_IN), Some(loadout::CLOSE_SEQUENCE)));
        assert_eq!([tick(&mut track), tick(&mut track)], [close, close]);
        assert_eq!(tick(&mut track), None, "no clear before the second close");
        track.close_loadout(CLEARED_IN);
        let clear = Some((Some(loadout::CLOSED_STATE), Some(CLEARED_IN), Some(0)));
        assert_eq!([tick(&mut track), tick(&mut track)], [clear, clear]);
        assert_eq!(tick(&mut track), None, "the slot ends");
        track.close_loadout(CLEARED_IN + 1);
        assert_eq!(tick(&mut track), None, "a close with no open sends nothing");
    }

    /// The flail's Trip (retail `general-other-01`: the press at 0x0fe4, the server's
    /// `13=08 14=0fe4 15=2344 17=0fe4 48=01` 34 ms later).
    fn trip() -> AbilityPlay {
        AbilityPlay {
            ability: "Ability_Flail_Trip".to_owned(),
            first: Some(FirstPlay {
                state: 8,
                sequence: 2344,
            }),
            slots: SlotsChange {
                marker: Some(1),
                ..SlotsChange::default()
            },
            cast: None,
            seen: 15,
        }
    }

    /// The life staff's Sacred Ground: its aim, then its cast.
    fn sacred_ground() -> AbilityPlay {
        AbilityPlay {
            ability: "Ability_Lifestaff_SacredGround".to_owned(),
            first: None,
            slots: SlotsChange {
                second_state: Some(36),
                second_sequence: Some(142),
                third_state: Some(46),
                third_sequence: Some(449),
                marker: Some(33),
                ..SlotsChange::default()
            },
            cast: Some(SlotsChange {
                second_state: Some(33),
                second_sequence: Some(993),
                third_state: Some(0),
                third_sequence: Some(0),
                marker: Some(3),
                ..SlotsChange::default()
            }),
            seen: 417,
        }
    }

    const PRESS_RECORD: u16 = 0x0fe4;
    const TRIP: u32 = 1;
    const GROUND: u32 = 2;

    #[test]
    fn a_full_body_ability_plays_in_the_first_slot_from_its_press_then_stands() {
        let table = table();
        let mut track = spawned();
        track.record(quiet_record(PRESS_RECORD, Intent::default()), &table);
        track.ability_started(TRIP, &trip(), PRESS_RECORD);
        let due = own_state(&mut track, PRESS_RECORD + 1);
        let state = due.state.expect("the ability");
        assert_eq!(
            (state.state, state.started, state.sequence),
            (Some(8), Some(PRESS_RECORD), 2344)
        );
        assert_eq!(
            state.second.map(|second| second.started),
            Some(PRESS_RECORD)
        );
        let slots = due.slots.expect("the marker");
        assert_eq!(slots.marker, Some(1));
        assert_eq!(slots.second.started, Some(PRESS_RECORD));
        let end_record = PRESS_RECORD + 40;
        track.record(quiet_record(end_record, Intent::default()), &table);
        track.ability_ended(GROUND);
        track.ability_ended(TRIP);
        track.ability_ended(TRIP);
        let end = own_state(&mut track, end_record + 1);
        let stand = end.state.expect("the stand");
        assert_eq!(
            (stand.state, stand.started),
            (Some(movement::IDLE_STATE), Some(end_record)),
            "back to standing still, once"
        );
        assert_eq!(end.slots.expect("the marker").marker, Some(0));
    }

    #[test]
    fn a_full_body_ability_holds_its_state_while_the_player_starts_and_stops_moving() {
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        track.record(quiet_record(PRESS_RECORD - 1, forward), &table);
        own_state(&mut track, PRESS_RECORD - 1);
        track.record(quiet_record(PRESS_RECORD, forward), &table);
        track.ability_started(TRIP, &trip(), PRESS_RECORD);
        let start = own_state(&mut track, PRESS_RECORD + 1)
            .state
            .expect("the ability");
        assert_eq!((start.state, start.sequence), (Some(8), 2344));
        let mut counter = PRESS_RECORD;
        for intent in [Intent::default(), Intent::default(), forward, Intent::default()] {
            counter += 1;
            track.record(quiet_record(counter, intent), &table);
            let state = own_state(&mut track, counter + 1).state;
            assert!(
                state.is_none_or(|state| state == start),
                "no move replaces the ability: {state:?}"
            );
        }
        counter += 1;
        track.record(quiet_record(counter, Intent::default()), &table);
        track.ability_ended(TRIP);
        let end = own_state(&mut track, counter + 1)
            .state
            .expect("the movement at the end");
        assert_eq!(end.started, Some(counter));
        assert_ne!(end.state, Some(8), "the ability's end leaves its state");
    }

    /// A record in which the client names the Trip's sequence in row 1.
    fn trip_named(counter: u16) -> SlayerRecord {
        SlayerRecord {
            action: Some(SlayerInput {
                sequence: 2344,
                state: 8,
                second_state: 0,
                second_sequence: 0,
            }),
            ..quiet_record(counter, Intent::default())
        }
    }

    #[test]
    fn a_full_body_ability_the_client_names_starts_at_the_clients_record() {
        let table = table();
        let mut track = spawned();
        track.record(quiet_record(PRESS_RECORD, Intent::default()), &table);
        own_state(&mut track, PRESS_RECORD);
        let named = PRESS_RECORD + 16;
        track.record(trip_named(named), &table);
        track.ability_started(TRIP, &trip(), PRESS_RECORD);
        let state = own_state(&mut track, named + 1)
            .state
            .expect("the ability");
        assert_eq!(
            (state.state, state.started, state.sequence),
            (Some(8), Some(named), 2344),
            "one start, at the client's record"
        );
        assert_eq!(state.second.map(|second| second.started), Some(named));

        let mut earlier = spawned();
        earlier.record(trip_named(PRESS_RECORD - 5), &table);
        own_state(&mut earlier, PRESS_RECORD - 5);
        earlier.record(quiet_record(PRESS_RECORD, Intent::default()), &table);
        earlier.ability_started(TRIP, &trip(), PRESS_RECORD);
        let state = own_state(&mut earlier, PRESS_RECORD + 1)
            .state
            .expect("the ability");
        assert_eq!(
            state.started,
            Some(PRESS_RECORD),
            "a start the client named before the press is not this one"
        );
    }

    #[test]
    fn an_aimed_ability_plays_its_aim_then_its_cast_then_clears_its_slots() {
        let table = table();
        let mut track = spawned();
        track.record(quiet_record(PRESS_RECORD, Intent::default()), &table);
        track.ability_started(GROUND, &sacred_ground(), PRESS_RECORD);
        let aim = own_state(&mut track, PRESS_RECORD + 1);
        assert_eq!(aim.state, None, "the first slot keeps the walk");
        let slots = aim.slots.expect("the aim");
        assert_eq!(
            (
                slots.second.state,
                slots.second.started,
                slots.second.sequence
            ),
            (Some(36), Some(PRESS_RECORD), Some(142))
        );
        assert_eq!(
            (slots.third.state, slots.third.started, slots.third.sequence),
            (Some(46), Some(PRESS_RECORD), Some(449))
        );
        assert_eq!(slots.marker, Some(33));
        let click = PRESS_RECORD + 18;
        track.record(quiet_record(click, Intent::default()), &table);
        track.ability_cast(GROUND, &sacred_ground());
        track.ability_cast(GROUND, &sacred_ground());
        let cast = own_state(&mut track, click + 1).slots.expect("the cast");
        assert_eq!(
            (cast.second.state, cast.second.started, cast.second.sequence),
            (Some(33), Some(click), Some(993))
        );
        assert_eq!((cast.third.state, cast.marker), (Some(0), Some(3)));
        let end_record = click + 30;
        track.record(quiet_record(end_record, Intent::default()), &table);
        track.ability_ended(GROUND);
        let end = own_state(&mut track, end_record + 1);
        assert_eq!(end.state, None);
        let cleared = end.slots.expect("the end");
        assert_eq!(
            (
                cleared.second.state,
                cleared.second.started,
                cleared.second.sequence
            ),
            (Some(loadout::CLOSED_STATE), Some(end_record), Some(0))
        );
        assert_eq!(
            (cleared.third.state, cleared.third.sequence, cleared.marker),
            (Some(0), Some(0), Some(0))
        );
    }
}
