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

use nw_content::player_attacks::AttackKind;
use nw_content::player_sequences::{
    AbilityCast, AbilityPlay, AirSequences, AltHold, AttackPress, AttackSlot, Direction, DodgeLoad,
    DrawHold, FirstPlay, HipShot, PlayerSequences, PressWindow, RiderPlays, RunningAttack,
    SlotsChange, StanceSequences, StatePlay, TimedSlots,
};
use nw_messages::{
    AlcPosture, AttackKeyBits, RemoteUpper, SecondSlayerState, SequenceClock, SequenceTime,
    SlayerInput, SlayerState, SlotChange, UPPER_SLOTS, UpperSlots,
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
/// The landing's slayer state (`13=0c`; Observed, 2026-11-17 study: the land followed 91 % of
/// 13,882 retail falls and 79 % of 23,473 jumps).
const LAND_STATE: u64 = 0x0c;
/// Input records from a landing to the move or stand it lands into (Observed: retail sent a move
/// or a stand a median 132 ms after the land, four records at 30 Hz, in 30,655 of 31,668 lands).
const LAND_MOVE_RECORDS: u16 = 4;
/// Input records from a jump's press within which a body that leaves the floor jumped, else it
/// fell (Inferred: the body leaves the floor in the record after the press, and the slayer
/// reads the body one record late).
const JUMP_LIFT_RECORDS: u16 = 3;
/// Seconds from a jump's start through which the players who see the player still get its floor
/// bit (child 41 bit `0x10`) set (Observed, `retail-2026-11-19-towns-duels`: the other players'
/// floor bit cleared 103/166/235 ms after the jump's start, p10/50/90 of 170 jumps; 164 to 167 ms
/// at the median for each of the five most common jump sequences, moving or standing, with the
/// body 0.34 m up then at the median). Ours cleared it at the body's lift, 0/52/66 ms (live
/// 2026-11-19c and 19e).
const JUMP_FLOOR_S: f32 = 0.165;
/// Milliseconds from a draw or sheathe in the second slot to that slot's end when the stance
/// names none (Observed: retail ended a `16=44` start with `16=45 18=0` a median 534 ms later,
/// 29,150 ends in 1,196 ledgers).
const SECOND_END_MS: u64 = 534;
/// The second slot's state of a draw or sheathe (`16=2c`).
const DRAW_STATE: u64 = sheathe::STATE;

/// The second slot's state at the end of a cast (Observed: `16=45 18=0 48=0`), of a hit
/// reaction ([`reaction`]) and of the right-click's block or aim ([`alt_end`]).
const CAST_END_STATE: u64 = 45;
/// The first-slot states of a dodge, a jump and an attack's press start (the table's `dodge`,
/// `jump` and `press` plays).
const DODGE_STATE: u64 = 7;
const JUMP_STATE: u64 = 14;
const ATTACK_PRESS_STATE: u64 = 10;
/// The first-slot state of the attacks that follow a press start (`13=08`; Unknown: the slayer
/// script's name of it).
const CHAIN_ATTACK_STATE: u64 = 8;
/// Bit 0 of the slayer marker (child 48), set while an attack chain plays.
const CHAIN_MARKER: u64 = 1;
/// Milliseconds per second.
const MS_PER_S: f32 = 1_000.0;
/// The weapon accuracy of a player's movement, by the movement state it plays
/// ([`nw_messages::RemoteStep::movement_accuracy`], Observed): the move start and run, a
/// restart or back move, the other moves (stops, block and aim walks), standing still or in
/// a jump. Another state keeps the last value (Unknown: the dodge's, the fall's and the
/// attacks' own; retail mostly kept it).
const RUN_ACCURACY: f32 = -5.0;
const BACK_ACCURACY: f32 = -7.0;
const WALK_ACCURACY: f32 = -2.0;
const STILL_ACCURACY: f32 = 0.0;
/// Seconds from a dodge's start to the move or stand it goes on in (Observed, 1,206 ledgers:
/// every player's dodge went on in its next state 481/500/521 ms after its start, p10/50/90 of
/// the 92,308 that went on in a run). Ours sent nothing at the dodge's end: the others kept the
/// roll's last pose until a key changed.
const DODGE_END_S: f32 = 0.5;
/// Seconds from a dodge's start through which the players who see the player get its dodge bit
/// (child 41 bit `0x02`) cleared (Observed, `retail-2026-11-19-towns-duels`: the other players'
/// bit set again 384/400/415 ms after the dodge's start, p10/50/90 of 251 dodges;
/// [`nw_messages::RemoteMoveFlags::dodging`]).
const DODGE_FLAG_S: f32 = 0.4;

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
    /// The run a start goes on in once its start sequence has played (Observed, 2026-11-19 study
    /// of the own states of 391 ledgers: 3,562 of the sheathed `13=1a 15=2794` starts went on in
    /// `15=3050` alone; every player of 1,206 ledgers, 12,776 times, 1,330/1,363/1,382 ms after
    /// the start, p10/50/90). Without it the others played the start's clip once and slid on
    /// still legs (live 2026-11-19f: a rifle start held 7.4 s).
    pub(super) const RUN_SEQUENCE: u64 = 3050;
    /// Seconds from a start to its run (the median above).
    pub(super) const RUN_AFTER_S: f32 = 1.366;
    /// A start let go within [`SHORT_STOP_S`] stops in `1370` (Observed, same study: 1,798 of
    /// 2,108 own short stops; every player, 7,362 of 8,888), and stands still
    /// [`SHORT_STOP_RECORDS`] later.
    pub(super) const SHORT_STOP_SEQUENCE: u64 = 1370;
    /// Seconds after a start within which a stop is a short stop (Observed, 1,206 ledgers: the
    /// `1370` stops came 64/151/266 ms after their start, p10/50/90; the `1405` ones 286/550/
    /// 1,120 ms).
    pub(super) const SHORT_STOP_S: f32 = 0.27;
    /// Input records from a short stop to the idle state (Observed: 517/535/557 ms, p10/50/90 of
    /// 4,441 sheathed short stops; 533 ms at the median for most stances).
    pub(super) const SHORT_STOP_RECORDS: u16 = 16;
}

/// The slayer states of a crouching or prone player (retail, the four `walk_running_crouch_prone`
/// and `equip_and_prone` captures, weapon sheathed, 15 presses of the crouch and prone toggles;
/// Inferred for a drawn weapon, which no capture has in these postures).
///
/// A change of posture is a new sequence of the state that goes on (child 15 alone), 33 to 54 ms
/// after the toggle, with the new posture in child 43 ([`AlcPosture`]). The sequence of the new
/// posture's stand or move follows it, alone too, unless a move started or stopped in between.
/// Other players got no state for these postures before: they saw a crouching or prone player
/// stand (live 2026-11-09). With the sequences alone and no child 43 they saw the crouch about
/// a second late (live 2026-11-18).
mod posture {
    use super::{AlcPosture, MoveSet, Posture, movement};

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
        run: None,
        stop_records: STOP_RECORDS,
        short_stop: (movement::MOVE_STATE, 3116),
        short_stop_records: STOP_RECORDS,
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
        run: None,
        stop_records: STOP_RECORDS,
        short_stop: (movement::MOVE_STATE, 1356),
        short_stop_records: STOP_RECORDS,
    };
    /// Input records from a crouching or prone stop to its idle state. A crouching or prone start
    /// goes on in its own sequence: it has no run, and its stops are one (Observed, 1,206
    /// ledgers: of the 199 crouching starts `13=1a 15=1846` none changed to a run sequence
    /// alone, 108 stopped in `3116`; of the 424 prone ones `1815`, 204 stopped in `1356`).
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

    /// How the clients show `posture` (child 43).
    pub(super) const fn shown(posture: Posture) -> AlcPosture {
        match posture {
            Posture::Stand => AlcPosture::Stand,
            Posture::Crouch => AlcPosture::Crouch,
            Posture::Prone => AlcPosture::Prone,
        }
    }

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
    /// posture's stand or move. No sequence for a moving player that stands up from a crouch:
    /// the stand's move goes at once (our choice: retail's own stand-up with the back key held
    /// sent `aa2f`, 1 of 1; of the other players' states in 1,195 ledgers whose child 43 went
    /// from crouch to stand, 165 carried the run's `b625` and 97 `aa2f`, by a rule that is
    /// Unknown). Without a sample of its own, a moving change takes the still one's sequence
    /// (Inferred), and a crouch to prone settles when its hold ends (Inferred).
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

/// The first-slot states of a rider (2026-11-19 study, `local-data/scratch-work/
/// remote-all-2026-11-19` `rider3.py`: every player's states of 1,196 ledgers against the
/// mount of its mount member, slot 2; the sequences are each mount type's
/// ([`RiderPlays`]); the times are the medians of every type). The server sends them all: none
/// of the 7,964 own rider starts was one the client named. Ours sent none: the others saw a
/// rider stand or run on foot over the mount (live 2026-11-19 has no rider; Inferred from the
/// code, no slayer state for a ride).
mod rider {
    /// The mount-up (`13=20`).
    pub(super) const MOUNT_UP_STATE: u64 = 20;
    /// The ride (`13=21`).
    pub(super) const RIDE_STATE: u64 = 21;
    /// Standing still on the mount (`13=23`).
    pub(super) const IDLE_STATE: u64 = 23;
    /// The jump (`13=18`), a fall (`13=19`) and their landing (`13=17`).
    pub(super) const JUMP_STATE: u64 = 18;
    pub(super) const FALL_STATE: u64 = 19;
    pub(super) const LAND_STATE: u64 = 17;
    /// The dismount (`13=15`).
    pub(super) const DISMOUNT_STATE: u64 = 15;
    /// Seconds from a moving mount-up to its ride: 516 to 541 ms at the median of each type
    /// (2,938 starts).
    pub(super) const MOUNT_UP_MOVING_S: f32 = 0.534;
    /// Seconds from a standing mount-up to the stand: 833 to 850 ms (98 starts).
    pub(super) const MOUNT_UP_STILL_S: f32 = 0.836;
    /// Seconds of the walk a ride starts from the stand with before its ride: 280 to 303 ms
    /// (the walk's sequence alone to the ride's, 1,016 changes).
    pub(super) const START_S: f32 = 0.3;
    /// Seconds of the walk a ride stops in before the stand: 351 to 398 ms (1,049 stops).
    pub(super) const STOP_S: f32 = 0.384;
    /// Seconds from a jump's landing to the ride: 233 to 234 ms (1,601 landings).
    pub(super) const LAND_S: f32 = 0.234;
    /// Seconds from a fall's landing to the ride: 167 to 169 ms (725 landings).
    pub(super) const FALL_LAND_S: f32 = 0.168;
    /// Seconds from a moving dismount to the run on foot: 533 to 534 ms (2,565 dismounts).
    pub(super) const DISMOUNT_MOVING_S: f32 = 0.534;
    /// Seconds from a standing dismount to the stand on foot: 967 to 1,016 ms (304
    /// dismounts).
    pub(super) const DISMOUNT_STILL_S: f32 = 0.984;
}

/// The first-slot state of an interaction's user interface (`13=24`; 2026-11-19 study,
/// `interact.py`): a talk, a trading post, a lore note, a town board, a crafting station. The
/// server starts it in the own state 10 ms after it accepts the interaction, with the second
/// slot stopped (`16=00`), and ends it with the stand that clears that slot (`13=1f 15=1081
/// 16=2d 18=00`). A gather plays state `13=09` and ends in the stance's stand.
pub(super) const INTERACTION_STATE: u64 = 24;
/// The first-slot state of a gather (`13=09`), whose sequence is its gatherable's animation's
/// ([`PlayerSequences::gather`]).
pub(super) const GATHER_STATE: u64 = 9;

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
///
/// On walking legs the slot plays the walk's own sequences (Observed, 2026-11-19 study of every
/// player of 1,206 ledgers, the `16=23` sequences): an open while moving `1152` (3,800 of its
/// 3,829 opens came in the move states `13=1a` and `13=1b`), its loop `615` a median 380 ms later
/// (2,294 times), and its close `3699` (515 after `615`, 1,250 after `1152`). A move that starts
/// while it is open goes to `615` (602 from `962b`, 461 from `890a`), and a stop goes back to the
/// open's `962b` (3,225 from `615`), whose loop follows as after an open. Ours played the
/// standing ones on walking legs.
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
    /// The open on walking legs.
    pub(super) const WALK_OPEN_SEQUENCE: u64 = 1152;
    /// The loop on walking legs.
    pub(super) const WALK_LOOP_SEQUENCE: u64 = 615;
    /// The close on walking legs.
    pub(super) const WALK_CLOSE_SEQUENCE: u64 = 3699;
    /// Time from the open on walking legs to its loop, ms (the median above).
    pub(super) const WALK_LOOP_MS: u64 = 380;

    /// The open, loop and close sequences and the time to the loop on walking legs
    /// (`walking`) or standing ones.
    pub(super) const fn on(walking: bool) -> (u64, u64, u64, u64) {
        if walking {
            (
                WALK_OPEN_SEQUENCE,
                WALK_LOOP_SEQUENCE,
                WALK_CLOSE_SEQUENCE,
                WALK_LOOP_MS,
            )
        } else {
            (OPEN_SEQUENCE, LOOP_SEQUENCE, CLOSE_SEQUENCE, LOOP_MS)
        }
    }
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
    /// The run a start goes on in once its start sequence has played
    /// ([`movement::RUN_SEQUENCE`]); none for a set whose start goes on as it is.
    run: Option<u64>,
    /// Input records from a stop to the idle state.
    stop_records: u16,
    /// The stop of a start let go within [`movement::SHORT_STOP_S`], and its records to the
    /// idle state.
    short_stop: (u64, u64),
    short_stop_records: u16,
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
        run: Some(movement::RUN_SEQUENCE),
        stop_records: movement::STOP_RECORDS,
        short_stop: (movement::MOVE_STATE, movement::SHORT_STOP_SEQUENCE),
        short_stop_records: movement::SHORT_STOP_RECORDS,
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
            set.run = moves.run.or(set.run);
            set.stop.1 = moves.stop.unwrap_or(set.stop.1);
            set.short_stop.1 = moves.short_stop.unwrap_or(set.short_stop.1);
            set.restart.1 = moves.restart.unwrap_or(set.restart.1);
            set.idle.1 = moves.idle.unwrap_or(set.idle.1);
        }
        if alt && let Some(walk) = stance.alt_moves {
            if let Some(moving) = walk.moving.map(pair) {
                set.start = moving;
                set.back_start = moving;
                set.restart = moving;
            }
            set.idle = walk.idle.map_or(set.idle, pair);
            // The block or aim walk stops into its stand ([`Locomotion::step`]) and goes on in
            // its walk.
            set.stop = set.idle;
            set.short_stop = set.idle;
            set.run = None;
        }
        set.moving = set.restart.1;
        set
    }
}

/// A block or aim that plays its upper body by legs
/// ([`AltHold`](nw_content::player_sequences::AltHold)).
#[derive(Debug, Clone, Copy, PartialEq)]
struct AltPlay {
    /// It plays on walking legs.
    walking: bool,
    /// Seconds of input records since its start or its last start of new legs.
    since_s: f32,
    /// It reached its hold.
    holding: bool,
    /// The second-slot sequence it plays.
    sequence: u64,
    /// The aim's third slot by legs, when it has one.
    third: Option<AimThird>,
}

/// Third-slot sequences on walking and standing legs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LegThirds {
    walk: u64,
    stand: u64,
}

impl LegThirds {
    /// The aim's of `hold` ([`AltLegs::third`](nw_content::player_sequences::AltLegs::third)).
    fn of_aim(hold: AltHold) -> Option<Self> {
        Some(Self {
            walk: hold.walk.third?,
            stand: hold.stand.third?,
        })
    }

    /// The full draw hold's ([`DrawHold`]).
    const fn of_draw(hold: &DrawHold) -> Self {
        Self {
            walk: hold.walk_third,
            stand: hold.stand_third,
        }
    }

    /// The sequence on walking (`walking`) or standing legs.
    const fn on(self, walking: bool) -> u64 {
        if walking { self.walk } else { self.stand }
    }

    /// Whether `sequence` is one of them.
    const fn has(self, sequence: u64) -> bool {
        sequence == self.walk || sequence == self.stand
    }
}

/// The third slot of an aim by legs: the aim's sequences, the ones it plays now (the aim's, or
/// a full draw hold's from that hold to the shot step that sends the aim's again,
/// [`DrawHold`]), and the one it sent last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AimThird {
    aim: LegThirds,
    playing: LegThirds,
    sent: u64,
}

impl AimThird {
    /// The third slot of `hold` on walking (`walking`) or standing legs, sent now.
    fn of(hold: AltHold, walking: bool) -> Option<Self> {
        let aim = LegThirds::of_aim(hold)?;
        Some(Self {
            aim,
            playing: aim,
            sent: aim.on(walking),
        })
    }

    /// The third-slot sequence it plays on walking (`walking`) or standing legs.
    const fn on(&self, walking: bool) -> u64 {
        self.playing.on(walking)
    }

    /// `sequence` on walking (`walking`) or standing legs: either legs' sequence of the aim
    /// plays the aim's again and becomes that of these legs, and is the one sent last; another
    /// sequence stays.
    fn sending(&mut self, sequence: u64, walking: bool) -> u64 {
        if !self.aim.has(sequence) {
            return sequence;
        }
        self.playing = self.aim;
        self.sent = self.aim.on(walking);
        self.sent
    }

    /// Plays `thirds` in place of the aim's: their sequence on walking (`walking`) or standing
    /// legs, when it is not the one sent last.
    fn drawing(&mut self, thirds: LegThirds, walking: bool) -> Option<u64> {
        self.playing = thirds;
        let wanted = thirds.on(walking);
        (wanted != self.sent).then(|| {
            self.sent = wanted;
            wanted
        })
    }
}

/// The upper slots that send third-slot `sequence` alone: no third state (child 19) and no
/// start (child 20), as retail's aim and full draw hold by legs.
fn third_alone(sequence: u64) -> UpperSlots {
    UpperSlots {
        third: SlotChange {
            sequence: Some(sequence),
            ..SlotChange::default()
        },
        ..UpperSlots::default()
    }
}

/// Whether a time of `elapsed_s` reached `mark_s` in a record of `frame_s`: the record nearest
/// the mark reaches it.
fn reached(elapsed_s: f32, frame_s: f32, mark_s: f32) -> bool {
    elapsed_s + frame_s / 2.0 >= mark_s
}

/// What the movement starts in one input record ([`Locomotion::step`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveStart {
    /// A slayer state and its sequence.
    State(u64, u64),
    /// A new sequence (`.1`) of the state that goes on (`.0`): child 15 alone.
    Sequence(u64, u64),
    /// The run (`.1`) of the start in state `.0`: child 15 alone, its time from the start's
    /// again (Observed, `retail-2026-11-19-towns-duels`: 102 of the 103 other players' runs
    /// after a start came with `slayerSeqTimeRel` at its zero and no new absolute part).
    Run(u64, u64),
}

/// Where the player's own movement is, for its movement slayer states.
///
/// Retail never follows a move state by itself (Observed, other players' starts in
/// `retail-2026-11-19-towns-duels`: 0 of 802 moves `13=1b` after a `13=1b`). The sheathed
/// moves of every player in 1,206 ledgers (2026-11-19 study, `local-data/scratch-work/
/// remote-all-2026-11-19`): a start `13=1a 15=2794` from standing still (21,776 of the 26,884
/// starts out of `13=1f`), its run `15=3050` alone 1,363 ms later ([`movement::RUN_SEQUENCE`]),
/// a stop `13=1b 15=1405` (or the short stop `1370` within 270 ms of the start) and the stand
/// 1,308 ms later. A key during a stop sends the restart `15=2422` alone (10,210 of the 19,535
/// `1405` stops), and the next record the start (18,556 of the 25,694 restarts went on in
/// `13=1a 15=2794` 36 ms later at the median); the restart let go stops short (`1370` alone,
/// 931 of 1,489). A dodge and a landing go on in the run itself (14,398 of the 18,229
/// forward dodges `7/4845` in `13=1a 15=3050` 500 ms after their start; 4,404 of the 4,636
/// moving landings `12/789` 134 ms after it). Ours held the start: its clip played once and
/// the others saw the legs stand still while the body slid (live 2026-11-19f, a rifle start
/// `13=1a 15=1855` held 7.4 s), and a stop and a restart both went out in `1b`: 159 of 227
/// moves after a move (live 2026-11-19c and 19e).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Locomotion {
    /// Standing still (the first own state's idle state).
    #[default]
    Idle,
    /// Moving by its keys, back or not, in the slayer state its last start went out in; while it
    /// plays its set's start, the seconds of input records since that start (`None` once it
    /// runs, and for any other move).
    Moving {
        back: bool,
        state: u64,
        start_s: Option<f32>,
    },
    /// A key again during a stop: its restart went out, and the next record starts the move.
    Restarting,
    /// Its keys let go: records left until it stands still.
    Stopping(u16),
}

impl Locomotion {
    /// Whether it moves by its keys (a restart too: its key is down).
    const fn moving(self) -> bool {
        matches!(self, Self::Moving { .. } | Self::Restarting)
    }

    /// The move that plays `play` of `set`, back or not: the set's start counts its seconds.
    fn playing(back: bool, play: (u64, u64), set: MoveSet) -> Self {
        Self::Moving {
            back,
            state: play.0,
            start_s: (play == set.start).then_some(0.0),
        }
    }

    /// The next movement after a record of `frame_s` with `intent`, and what the server starts
    /// in that record, when it starts something: a move from standing still, a new direction
    /// (back or not) that starts in another state, the run of a start, a stop (short after a
    /// fresh start), a stand, or a restart and then a move again after a stop ([`Locomotion`]).
    fn step(self, intent: Intent, set: MoveSet, frame_s: f32) -> (Self, Option<MoveStart>) {
        let moving = intent.forward != 0 || intent.right != 0;
        let back = intent.forward < 0;
        let start = if back { set.back_start } else { set.start };
        let state = |play: (u64, u64)| Some(MoveStart::State(play.0, play.1));
        let moves = |play: (u64, u64)| (Self::playing(back, play, set), state(play));
        match (self, moving) {
            (Self::Idle | Self::Restarting, true) => moves(start),
            // A set whose stop is its stand (the block or aim walk) stands at once (Observed: in
            // 398 retail ledgers 2,127 of 2,759 stops of a block walk started the block's stand
            // within 100 ms, 1,693 within 50 ms; ours held the walk 39 records, live 2026-11-17).
            // A move in the stop's own state stands at once too ([`Locomotion`]).
            (Self::Moving { state: now, .. }, false)
                if set.stop == set.idle || now == set.stop.0 =>
            {
                (Self::Idle, state(set.idle))
            }
            (
                Self::Moving {
                    start_s: Some(since_s),
                    ..
                },
                false,
            ) if since_s + frame_s < movement::SHORT_STOP_S => (
                Self::Stopping(set.short_stop_records),
                state(set.short_stop),
            ),
            (Self::Moving { .. }, false) => (Self::Stopping(set.stop_records), state(set.stop)),
            (
                Self::Moving {
                    back: was,
                    state: now,
                    ..
                },
                true,
            ) if was != back && start.0 != now => moves(start),
            (
                Self::Moving {
                    state: now,
                    start_s: Some(since_s),
                    ..
                },
                true,
            ) => {
                let since_s = since_s + frame_s;
                let run = set.run.filter(|_| {
                    !back && now == set.start.0 && reached(since_s, frame_s, movement::RUN_AFTER_S)
                });
                match run {
                    Some(run) => (
                        Self::Moving {
                            back,
                            state: now,
                            start_s: None,
                        },
                        Some(MoveStart::Run(now, run)),
                    ),
                    None => (
                        Self::Moving {
                            back,
                            state: now,
                            start_s: Some(since_s),
                        },
                        None,
                    ),
                }
            }
            (Self::Moving { state: now, .. }, true) => (
                Self::Moving {
                    back,
                    state: now,
                    start_s: None,
                },
                None,
            ),
            (Self::Stopping(_), true)
                if !back && set.restart.0 == set.stop.0 && set.start.0 != set.stop.0 =>
            {
                (
                    Self::Restarting,
                    Some(MoveStart::Sequence(set.stop.0, set.restart.1)),
                )
            }
            // A back key during a stop goes on in the stop's state with the back move's sequence
            // alone (Observed, `retail-2026-11-19-towns-duels`: a back key during the stop
            // `27/1386` sent `15=2747` 134 ms later and held it 1,066 ms while the body moved
            // back; of 179 runs and 892 starts while other players moved back, 3 and 25). Ours
            // sent the forward start `13=26` and 1.37 s later its run: the legs ran forward
            // while the body backed up (live 2026-11-20, 148 of 322 back-key records).
            (Self::Stopping(_), true) if back && start.0 == set.stop.0 => (
                Self::Moving {
                    back,
                    state: start.0,
                    start_s: None,
                },
                Some(MoveStart::Sequence(start.0, start.1)),
            ),
            (Self::Stopping(_), true) if start.0 == set.stop.0 => {
                (Self::playing(back, set.start, set), state(set.start))
            }
            (Self::Stopping(_), true) => moves(start),
            (Self::Restarting, false) => (
                Self::Stopping(set.short_stop_records),
                Some(MoveStart::Sequence(set.stop.0, set.short_stop.1)),
            ),
            (Self::Stopping(left), false) => match left.saturating_sub(1) {
                0 => (Self::Idle, state(set.idle)),
                left => (Self::Stopping(left), None),
            },
            (Self::Idle, false) => (self, None),
        }
    }

    /// The state the movement plays now in `set`: a move back in the set's back move; a move
    /// goes on in the set's move state, or starts again in the set's start when that is the
    /// state it is in, or runs on in the start's state once it runs ([`Locomotion`]).
    fn current(self, set: MoveSet) -> (u64, u64) {
        match self {
            Self::Idle => set.idle,
            // A move back goes on in its back move, never the forward start.
            Self::Moving { back: true, .. } => set.back_start,
            Self::Moving { state, .. } if state == set.restart.0 => set.start,
            Self::Moving {
                state,
                start_s: None,
                ..
            } if state == set.start.0 && set.run.is_some() => {
                set.run.map_or(set.start, |run| (state, run))
            }
            Self::Moving { .. } => (set.restart.0, set.moving),
            Self::Restarting => set.restart,
            Self::Stopping(_) => set.stop,
        }
    }

    /// [`Locomotion::current`] sent again: a move is in its state from here, and a start that
    /// goes out counts its seconds again.
    fn resend(&mut self, set: MoveSet) -> (u64, u64) {
        let play = self.current(set);
        if let Self::Moving { back, .. } = *self {
            *self = Self::playing(back, play, set);
        }
        play
    }

    /// The move or stand that goes out, in `set`, when an attack lets the first slot go after
    /// it held the movement's starts and stops back ([`SlayerTrack::keeps_attack`]), `back`
    /// with the back key: a body that stopped stands (no stop; Observed,
    /// `retail-2026-11-19-towns-duels`: no stop came after 536 other players' attack starts,
    /// and their next movement was a stand a median 866 ms after it, a start 1,119 ms, a block
    /// walk or a back move); a restart starts its move; a move goes on ([`Locomotion::resend`]).
    fn after_attack(&mut self, back: bool, set: MoveSet) -> (u64, u64) {
        match *self {
            Self::Idle | Self::Stopping(_) => {
                *self = Self::Idle;
                set.idle
            }
            Self::Restarting => {
                let start = if back { set.back_start } else { set.start };
                *self = Self::playing(back, start, set);
                start
            }
            Self::Moving { .. } => self.resend(set),
        }
    }

    /// The move or stand a landing or a dodge goes on in, in `set`: a move forward runs at once
    /// in the start's state, a move back starts its back move ([`Locomotion`]; Observed,
    /// `retail-2026-11-19-towns-duels`: 104 of the 178 other players' landings went on in
    /// `13=1a`, 62 in `13=1b`).
    fn resumed(&mut self, set: MoveSet) -> (u64, u64) {
        let Self::Moving { back, .. } = *self else {
            return self.current(set);
        };
        let play = match set.run {
            _ if back => set.back_start,
            Some(run) => (set.start.0, run),
            None => set.start,
        };
        *self = Self::playing(back, play, set);
        play
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
    /// The open sequence, until its loop ([`loadout::on`]).
    Opening {
        /// The ALC state that started it.
        started: u16,
        /// When the client opened it, or when the player stood still while it was open.
        opened_ms: u64,
        /// Own states left that carry it.
        left: u8,
        /// It plays on walking legs.
        walking: bool,
    },
    /// The loop sequence while it stays open.
    Looping {
        /// The ALC state that started the open.
        started: u16,
        /// Own states left that carry it.
        left: u8,
        /// It plays on walking legs.
        walking: bool,
    },
    /// The close sequence: own states left that carry it; then nothing until the client's
    /// second close.
    Closing {
        /// The ALC state that started the close.
        started: u16,
        /// Own states left that carry the close sequence.
        left: u8,
        /// It plays on walking legs.
        walking: bool,
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
    /// The run of a start: a new sequence of the state that goes on, its time from the start's
    /// again ([`MoveStart::Run`]).
    Run(u64),
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
    /// The slayer marker (child 48) is set.
    marker: bool,
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
            marker: change
                .marker
                .map_or(self.marker, |marker| marker != CLEARED_SLOT),
        }
    }
}

/// Seconds into the sequence of each upper slot that plays, for the players who see this one
/// ([`nw_messages::RemoteUpperClocks`]): a slot's time starts at 0 in the own state that first
/// sends a new state or sequence of it, goes on with the records' frame times, and goes on
/// through the states that send its end. Observed (`retail-2026-11-19-towns-duels`, other
/// players): a new second-slot sequence without a new start (child 18 alone) took the byte back
/// in 510 of 626 cases; the end state (`16=0` or `16=45`) carried child 4 in 2,281 of 2,803.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct UpperSeconds {
    /// Seconds into each slot's sequence while it plays, second slot first.
    seconds: [Option<f32>; UPPER_SLOTS],
    /// The slot's end went out: its time stops once no state sends that end any more.
    ending: [bool; UPPER_SLOTS],
}

impl UpperSeconds {
    /// One input record of `frame_s` seconds went by.
    fn advance(&mut self, frame_s: f32) {
        for seconds in self.seconds.iter_mut().flatten() {
            *seconds += frame_s;
        }
    }

    /// An own state went out with the slot changes `changes` (second slot first, `None` for a
    /// slot it leaves alone), which it sends for the first time when `first`.
    fn after(&mut self, changes: [Option<SlotChange>; UPPER_SLOTS], first: bool) {
        for (slot, change) in changes.into_iter().enumerate() {
            let change =
                change.filter(|change| change.state.is_some() || change.sequence.is_some());
            match change {
                Some(change) if first => {
                    let ends = change.state == Some(CLEARED_SLOT)
                        || change.sequence == Some(CLEARED_SLOT)
                        || (slot == SECOND_SLOT && change.state == Some(CAST_END_STATE));
                    self.ending[slot] = ends;
                    if !ends {
                        self.seconds[slot] = Some(0.0);
                    }
                }
                Some(_) => {}
                None if self.ending[slot] => {
                    self.seconds[slot] = None;
                    self.ending[slot] = false;
                }
                None => {}
            }
        }
    }
}

/// The index of the second slot in [`UpperSeconds`].
const SECOND_SLOT: usize = 0;

/// The second slot's change that slayer state `state` sends ([`SecondSlayerState`]): a stop is
/// its state 0.
fn second_change(state: &SlayerState) -> Option<SlotChange> {
    let second = state.second?;
    let (state, sequence) = match second.playing {
        Some((state, sequence)) => (Some(state), Some(sequence)),
        None => (second.stopped.then_some(CLEARED_SLOT), None),
    };
    Some(SlotChange {
        state,
        started: Some(second.started),
        sequence,
    })
}

/// The weapon ability whose animation the slayer state plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AbilityOn {
    /// CRC-32 of its lowercase id.
    ability: u32,
    /// It plays in the first slot (a full-body ability).
    first: bool,
    /// It plays a third slot (an aimed ability, or a shot's third-slot sequence).
    third: bool,
    /// Its cast went out.
    cast: bool,
}

/// The timed changes of the weapon ability that plays ([`AbilityPlay::after`],
/// [`AbilityCast::after`]) while they go out, and its end when retail ends it before its action
/// stops ([`AbilityCast::ends_after_ms`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct AbilitySteps {
    /// CRC-32 of the ability's lowercase id.
    ability: u32,
    /// The changes not sent yet, by milliseconds from `from_ms`, in their order.
    steps: Vec<TimedSlots>,
    /// Milliseconds from `from_ms` to the ability's end.
    ends_after_ms: Option<u64>,
    /// The time of the own state that sent the start or the cast; `None` until it goes out.
    from_ms: Option<u64>,
}

impl AbilitySteps {
    fn new(ability: u32, steps: &[TimedSlots], ends_after_ms: Option<u64>) -> Self {
        Self {
            ability,
            steps: steps.to_vec(),
            ends_after_ms,
            from_ms: None,
        }
    }

    /// The changes due by own state time `state_ms` (the first call is the time of the state
    /// that sent the start or the cast), in their order, and whether the ability ends now.
    fn due(&mut self, state_ms: u64) -> (Vec<SlotsChange>, bool) {
        let since_ms = state_ms.saturating_sub(*self.from_ms.get_or_insert(state_ms));
        let sent = self
            .steps
            .iter()
            .take_while(|step| step.after_ms <= since_ms)
            .count();
        let due = self.steps.drain(..sent).map(|step| step.slots).collect();
        let ends = self
            .ends_after_ms
            .is_some_and(|ends_ms| since_ms >= ends_ms);
        (due, ends)
    }
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

/// What the attack key changed in the upper slots of a weapon that shoots
/// ([`SlayerTrack::shot_slots`]).
#[derive(Debug, Clone, PartialEq)]
enum Shot {
    /// The reload: the press fires nothing.
    Reload(UpperSlots),
    /// The full draw of an aimed shot that goes out at the release
    /// ([`StanceSequences::alt_charge`]).
    Drawn(UpperSlots),
    /// A shot, aimed or from the hip, and what follows it.
    Fire(UpperSlots, Box<ShotSteps>),
}

/// A shot the other players see go out ([`SlayerTrack::take_fired`]): the server fires the same
/// shot, so its projectile and its damage come with the shot they see. Live 2026-11-20: the
/// server read a bow's and a gun's attack key as a melee weapon's; it fired a gun's aimed shot
/// every 530 ms while the client and the others played one shot and its 1.7 s reload, and it
/// fired no arrow for a bow's hip shots (a hold of 167 ms swung `Bow_Gunbutt` instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FiredShot {
    /// The own state number of the input record it started in ([`super::members::own_state_counter`]);
    /// `None` for a shot that went out with an own state after its record (a held aimed shot, a
    /// hip re-fire).
    pub(super) record: Option<u16>,
    /// The hip shot and the aimed shot are light attacks; the aimed shot of a full draw is the
    /// charged one ([`nw_content::player_attacks::Stance::aimed_attack`]).
    pub(super) kind: AttackKind,
    /// The right-click's aimed shot, not the hip shot.
    pub(super) aimed: bool,
}

/// An aimed press of a weapon that shoots at the release
/// ([`StanceSequences::alt_fire_at_release`]), while the key is down.
#[derive(Debug, Clone, PartialEq)]
struct AimedDraw {
    /// Seconds of input records since the press.
    since_s: f32,
    /// The full draw went out ([`StanceSequences::alt_charge`]).
    full: bool,
    /// The full draw's hold, from the full draw on.
    hold: Option<DrawPlay>,
}

/// The hold of a full draw ([`DrawHold`]) while it goes out.
#[derive(Debug, Clone, PartialEq)]
struct DrawPlay {
    hold: DrawHold,
    /// When the own state of the full draw went out.
    from_ms: Option<u64>,
    /// When the hold's own state went out.
    held_ms: Option<u64>,
    /// The next of the hold's steps to go out.
    next: usize,
}

/// The upper slot changes that follow a shot ([`TimedSlots`]), while they go out.
#[derive(Debug, Clone, PartialEq)]
struct ShotSteps {
    steps: Vec<TimedSlots>,
    /// The next step to go out.
    next: usize,
    /// When the own state of the shot (or of its re-fire) went out.
    from_ms: Option<u64>,
    /// The newest own state time the steps were stepped to.
    at_ms: Option<u64>,
    /// The steps that follow the shot when it emptied the weapon, counted from the same own
    /// state: they take the place of the steps not sent yet once the weapon is empty
    /// ([`StanceSequences::alt_fire_empty_after`], [`HipShot::empty_after`]).
    empty_steps: Option<Vec<TimedSlots>>,
    /// The second-slot sequence the last step sent.
    last_second: Option<u64>,
    kind: ShotKind,
    /// The attack of the shot ([`FiredShot::kind`]).
    attack: AttackKind,
}

/// An aimed shot sent before the stance's shortest time from the shot before
/// ([`StanceSequences::alt_fire_gap_ms`]), until it goes out.
#[derive(Debug, Clone, PartialEq)]
struct HeldShot {
    change: SlotsChange,
    steps: ShotSteps,
    /// The own state time it goes out at.
    at_ms: u64,
    /// The stance's ready time after the aim's start ([`StanceSequences::alt_fire_ready_ms`]):
    /// it goes out only once the aim was up that long.
    aim_ready_ms: Option<u64>,
}

/// Which shot [`ShotSteps`] follow.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ShotKind {
    /// An aimed shot: the aim's hold waits for its steps ([`SlayerTrack::alt_hold_step`]).
    Aimed,
    /// A hip shot, whose last step is its end ([`HipShot`]).
    Hip {
        refire: Option<SlotsChange>,
        /// A press came while it played.
        refire_due: bool,
        /// When a press re-fires at once ([`HipShot::refire_window`]).
        window: Option<PressWindow>,
    },
}

impl ShotSteps {
    /// The steps after an aimed shot, and `empty_steps` in their place when it emptied the
    /// weapon (none when empty).
    fn aimed(steps: &[TimedSlots], empty_steps: &[TimedSlots]) -> Self {
        Self {
            steps: steps.to_vec(),
            next: 0,
            from_ms: None,
            at_ms: None,
            empty_steps: (!empty_steps.is_empty()).then(|| empty_steps.to_vec()),
            last_second: None,
            kind: ShotKind::Aimed,
            attack: AttackKind::Light,
        }
    }

    /// The shot these steps follow, started in own state number `record` (`None`: the newest).
    fn fired(&self, record: Option<u16>) -> FiredShot {
        FiredShot {
            record,
            kind: self.attack,
            aimed: self.kind == ShotKind::Aimed,
        }
    }

    /// The steps after the start of `hip`, its end last.
    fn hip(hip: &HipShot) -> Self {
        let mut steps = hip.after.clone();
        steps.push(hip.end);
        Self {
            kind: ShotKind::Hip {
                refire: hip.refire,
                refire_due: false,
                window: hip.refire_window,
            },
            ..Self::aimed(&steps, &hip.empty_after)
        }
    }

    /// Milliseconds from the shot's own state to the newest own state the steps went to.
    fn elapsed_ms(&self) -> Option<u64> {
        Some(self.at_ms?.saturating_sub(self.from_ms?))
    }

    /// Puts the steps of an emptied weapon in the place of the steps not sent yet: those due
    /// after the last step sent.
    fn empty(&mut self) {
        let Some(empty_steps) = self.empty_steps.take() else {
            return;
        };
        let sent_ms = self
            .next
            .checked_sub(1)
            .and_then(|last| self.steps.get(last))
            .map(|step| step.after_ms);
        self.steps = empty_steps
            .into_iter()
            .filter(|step| sent_ms.is_none_or(|sent_ms| step.after_ms > sent_ms))
            .collect();
        self.next = 0;
    }

    /// The changes due by own state time `state_ms` (the first call is the shot's own state),
    /// merged in their order; with the weapon `empty`, the steps of an emptied weapon go on
    /// ([`ShotSteps::empty`]). A hip shot's end goes out as its re-fire when a press asked for
    /// one; its steps then count again from there. Gives the changes, and whether the re-fire
    /// went out (a shot of its own: [`FiredShot`]).
    fn due(&mut self, state_ms: u64, empty: bool) -> (Vec<SlotsChange>, bool) {
        let from_ms = *self.from_ms.get_or_insert(state_ms);
        self.at_ms = Some(state_ms);
        if empty {
            self.empty();
        }
        let mut due = Vec::new();
        let mut refired = false;
        while let Some(step) = self.steps.get(self.next) {
            if state_ms.saturating_sub(from_ms) < step.after_ms {
                break;
            }
            let ends = self.next + 1 == self.steps.len();
            if ends
                && let ShotKind::Hip {
                    refire: Some(refire),
                    refire_due,
                    ..
                } = &mut self.kind
                && *refire_due
            {
                *refire_due = false;
                due.push(*refire);
                refired = true;
                self.next = 0;
                self.from_ms = Some(state_ms);
                break;
            }
            due.push(step.slots);
            self.next += 1;
        }
        if let Some(sequence) = due.iter().rev().find_map(|change| change.second_sequence) {
            self.last_second = Some(sequence);
        }
        (due, refired)
    }

    /// Every step went out.
    fn done(&self) -> bool {
        self.next == self.steps.len()
    }
}

/// How a body went into the air: its landing differs ([`AirSequences`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AirWay {
    Jumped,
    Fell,
}

/// A draw or sheathe in the second slot and the end that follows it ([`SECOND_END_MS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SecondEnd {
    /// Its sequence.
    sequence: u64,
    /// Milliseconds from its first state to the end.
    after_ms: u64,
    /// When the end is due, once its first state went out.
    at_ms: Option<u64>,
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
    pub(super) attack: AttackKeyBits,
    /// The weapon in the hand.
    pub(super) weapon: Option<HeldWeapon>,
    /// The body swims ([`nw_sim_core::TrackedBody::is_swimming`]).
    pub(super) swimming: bool,
    /// The body falls and is not in a jump ([`nw_sim_core::TrackedBody::is_falling`]).
    pub(super) falling: bool,
    /// The body is off the floor, in a jump or a fall, and does not swim
    /// ([`nw_sim_core::TrackedBody::is_grounded`]).
    pub(super) airborne: bool,
    /// How the body holds itself ([`nw_sim_core::TrackedBody::posture`]).
    pub(super) posture: Posture,
    /// The body is in a land dodge of an earlier record ([`nw_sim_core::TrackedBody::is_dodging`]):
    /// the body starts no dodge at a press in it, so the others get none.
    pub(super) dodging: bool,
    /// How heavy its gear is: the kind of its dodge ([`PlayerSequences::dodge`]).
    pub(super) load: DodgeLoad,
    /// The mount the player rides, when it rides.
    pub(super) ride: Option<RideKeys>,
}

/// The mount a player rides in one record: its type's sequences, and whether it dashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RideKeys {
    pub(super) plays: RiderPlays,
    pub(super) dashing: bool,
}

/// The first-slot state and sequence of an interaction the server accepted, and whether the
/// user crouches in it ([`INTERACTION_STATE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InteractPlay {
    pub(super) state: u64,
    pub(super) sequence: u64,
    pub(super) crouch: bool,
}

/// Where a player's ride is ([`rider`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Ride {
    /// On foot.
    #[default]
    Foot,
    /// Mounting up, moving or not: seconds since.
    MountingUp {
        plays: RiderPlays,
        moving: bool,
        since_s: f32,
    },
    /// Riding.
    Riding {
        plays: RiderPlays,
        legs: RideLegs,
        air: RideAir,
    },
    /// Getting off, moving or not: seconds since.
    Dismounting { moving: bool, since_s: f32 },
}

/// The mount's legs on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RideLegs {
    /// Standing still.
    Idle,
    /// Walking off from the stand: seconds since.
    Starting(f32),
    /// Riding.
    Ride,
    /// Riding with the dash.
    Fast,
    /// Walking to a stop: seconds since.
    Stopping(f32),
}

/// The mount in the air.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RideAir {
    /// On the ground.
    Ground,
    /// In a jump.
    Jumped,
    /// In a fall.
    Fell,
    /// Landed: seconds since, and the seconds to the ride.
    Landed { since_s: f32, after_s: f32 },
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
    /// The last record held block.
    block_down: bool,
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
    /// The last upper slot change sent, untimed, and the own state that sent it first
    /// ([`SlayerTrack::timed`]).
    upper_first: Option<(UpperSlots, u16)>,
    /// The previous record's weapon had shots and none loaded.
    was_empty: bool,
    /// The weapon was loaded again after it emptied and no reload played since.
    reload_due: bool,
    /// The aimed press of a weapon that shoots at the release, while its key is down.
    aimed_draw: Option<AimedDraw>,
    /// The steps that follow the last shot, while they go out.
    shot_steps: Option<ShotSteps>,
    /// An aimed shot that waits for the stance's shortest time from the shot before.
    held_shot: Option<HeldShot>,
    /// The own state time of the last aimed shot's own state.
    aimed_from_ms: Option<u64>,
    /// The right-click's aim started in a record no own state carried yet.
    aim_start_due: bool,
    /// The own state time of the newest aim start's own state.
    aim_from_ms: Option<u64>,
    /// The newest own state time ([`SlayerTrack::advance`]).
    newest_state_ms: Option<u64>,
    /// The shots that went out since the server last took them ([`SlayerTrack::take_fired`]).
    fired: Vec<FiredShot>,
    /// The second-slot sequence an aimed shot's steps left the aim on, for its hold.
    alt_resume: Option<u64>,
    /// The previous record's keys: right-click, dodge, jump; and whether the weapon was drawn.
    alt_held: bool,
    /// A dodge, a jump or an attack ended the block while the right-click stayed held:
    /// it starts again with the next move ([`SlayerTrack::break_alt`]).
    alt_broken: bool,
    /// The block or aim that plays its upper body by legs, when one does.
    alt_play: Option<AltPlay>,
    dodge_held: bool,
    jump_held: bool,
    drawn: bool,
    /// The stance of the weapon the previous record had drawn.
    drawn_stance: Option<u32>,
    /// The movement states of the newest record ([`MoveSet`]).
    set: Option<MoveSet>,
    /// The player's own movement.
    locomotion: Locomotion,
    /// An attack held a movement start or stop back ([`Locomotion::after_attack`]).
    move_held: bool,
    /// Seconds of input records since a dodge started, while its state plays
    /// ([`DODGE_END_S`]).
    roll_s: Option<f32>,
    /// The player's ride.
    ride: Ride,
    /// An interaction's state that starts in the next own state.
    interaction_due: Option<InteractPlay>,
    /// The interaction that plays, from its start to its end.
    interacting: Option<InteractPlay>,
    /// A gather ended: the next own state stands the player still in its stance.
    rest_due: bool,
    /// The sequence time at the newest state's start: a run's time starts there again
    /// ([`MoveStart::Run`]).
    state_from_s: f32,
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
    /// How the body went into the air, while it is off the floor.
    air: Option<AirWay>,
    /// The record of the last jump press.
    jumped_at: Option<u16>,
    /// Input records left until the landing's move or stand goes out ([`LAND_MOVE_RECORDS`]).
    land_settle: Option<u16>,
    /// The draw or sheathe in the second slot whose end is due.
    second_end: Option<SecondEnd>,
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
    /// The time into each upper slot's sequence, for the players who see this one.
    upper_seconds: UpperSeconds,
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
    /// The timed changes of that ability ([`AbilitySteps`]).
    ability_steps: Option<AbilitySteps>,
    /// The weapon accuracy of the movement the newest state plays ([`RUN_ACCURACY`]).
    accuracy: f32,
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
        self.upper_seconds.advance(record.frame_s);
        // The key goes down at its press or at a controller's first held record, as the
        // attacks read it ([`AttackKeyBits::goes_down`]; a block start presses none,
        // [`AttackKeyBits::at_block_start`]); it stays down through a hit reaction.
        let blocking = record.intent.is_some_and(|intent| intent.block);
        let attack = record.attack.at_block_start(blocking && !self.block_down);
        self.block_down = blocking;
        let record = SlayerRecord {
            attack: AttackKeyBits {
                pressed: attack.goes_down(self.attack_down),
                ..attack
            },
            ..record
        };
        self.attack_down = record.attack.down_after(self.attack_down);
        if self.reaction_due.is_some() || self.reaction_free_ms.is_some() {
            // A hit reaction holds the player: its records start nothing, and the client's row
            // 1 names the reaction itself, which is not echoed ([`reaction`]).
            self.press = None;
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
        // The moves, dodges, jumps, falls and landings play the weapon's stance while it is
        // drawn, else the stance of no weapon ([`PlayerSequences::sheathed`]).
        let moves_stance = if drawn { stance } else { table.sheathed() };
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
        if record.ride.is_some() || self.ride != Ride::Foot {
            // A rider plays the ride's states only ([`rider`]).
            let started = self.ride_step(&record, set);
            if let Some(started) = started {
                self.begin(started, &record);
            }
            if self.ride != Ride::Foot || started.is_some() {
                return;
            }
        }
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
        let air = moves_stance.and_then(|stance| stance.air);
        // A body that was in the air before this record starts no jump and no dodge (live
        // 2026-11-17: a controller player's jump presses in the air each started a new jump,
        // five in 0.8 s, and the others saw the jump twitch).
        let in_air = self.air.is_some();
        let landing = self.air_step(&record, air);
        let mut move_sequence = None;
        let movement = if entered_air {
            let fall = air.and_then(|air| air.fall).unwrap_or(swim::FALL_SEQUENCE);
            Some((swim::FALL_STATE, fall))
        } else if entered_water {
            Some((swim::STATE, swim::IDLE_SEQUENCE))
        } else if record.swimming {
            None
        } else {
            // The end of a stop is no key: it does not replace the attack that runs (live
            // 2026-11-17: a spear player stopped and held the heavy attack; the stand started
            // two records after the charge and the others saw no heavy attack).
            // A movement start or stop does not replace the attack that runs either: the
            // movement steps, and the move or stand it is in goes out once the attack lets the
            // first slot go ([`Locomotion::after_attack`]). Live 2026-11-20: a stop 64 ms after
            // a press start replaced it, and the others saw the body slide 1.2 m with no legs
            // (6 of 28 stops came within 1.1 s of an attack start; retail 2 of 191, none within
            // 0.5 s).
            let keeps_attack = self.keeps_attack();
            let stepped = record.intent.filter(|_| !held_start).and_then(|intent| {
                let (next, started) = self.locomotion.step(intent, set, record.frame_s);
                self.locomotion = next;
                if keeps_attack && started.is_some() {
                    self.move_held = true;
                    return None;
                }
                started
            });
            let stepped = stepped.or_else(|| {
                let back = record.intent?.forward < 0;
                if keeps_attack || !std::mem::take(&mut self.move_held) {
                    return None;
                }
                let (state, sequence) = self.locomotion.after_attack(back, set);
                Some(MoveStart::State(state, sequence))
            });
            // The air plays the jump or the fall until the landing: the keys start no move or
            // stand there (Observed, `retail-2026-11-19-towns-duels`: 148 of the 170 other
            // players' jumps went on to the landing, 6 to a stand; ours sent the stand of a
            // stop that ended 34 to 117 ms into the jump, 3 of 12 jumps, live 2026-11-19c and
            // 19e). The movement still steps, and the landing goes on in it.
            // A dodge plays to its end too: the movement goes on from there
            // ([`DODGE_END_S`]; Observed, 1,206 ledgers: of the 154,956 states that followed a
            // dodge, 5,143 stood still, at 251/499/533 ms; ours stood a dodge with the keys let
            // go in its second record).
            let rolling = self.roll_s.is_some();
            let stepped = stepped.filter(|_| self.air.is_none() && !rolling);
            let (stepped, sequence) = match stepped {
                Some(MoveStart::State(state, sequence)) => (Some((state, sequence)), None),
                Some(MoveStart::Sequence(state, sequence)) => {
                    (None, Some((state, Pending::Sequence(sequence))))
                }
                Some(MoveStart::Run(state, sequence)) => {
                    (None, Some((state, Pending::Run(sequence))))
                }
                None => (None, None),
            };
            // A new sequence of the movement's state goes out only while that state plays; a
            // restart of a stop that does not play (after a hit reaction) starts the move at
            // once.
            let shown = sequence
                .filter(|(state, _)| self.played_state == Some(*state) && self.pending.is_none());
            let stepped = match (shown, self.locomotion, record.intent) {
                (None, Locomotion::Restarting, Some(intent)) if sequence.is_some() => {
                    let (next, started) = self.locomotion.step(intent, set, record.frame_s);
                    self.locomotion = next;
                    match started {
                        Some(MoveStart::State(state, sequence)) => Some((state, sequence)),
                        _ => None,
                    }
                }
                // A back move's sequence whose state does not play (after a hit reaction, whose
                // stop does not play) starts in its state, as a back move after a dodge does
                // (Observed: retail went from a dodge `7/4487` into the back move `27/1246`).
                (None, Locomotion::Moving { back: true, .. }, _) => match sequence {
                    Some((state, Pending::Sequence(sequence))) => Some((state, sequence)),
                    _ => stepped,
                },
                _ => stepped,
            };
            move_sequence = shown.map(|(_, sequence)| sequence);
            let rolled = self.roll_end(record.frame_s) && self.air.is_none();
            // A landing goes first; the move or stand it lands into follows
            // ([`LAND_MOVE_RECORDS`]).
            landing
                .or_else(|| self.settled_landing(stepped.is_some(), set))
                .or(stepped)
                .or_else(|| rolled.then(|| self.locomotion.resumed(set)))
        };
        let mut movement = movement.filter(|_| !ability_holds);
        let stroke = stroke.filter(|_| !ability_holds);
        let move_sequence = move_sequence.filter(|_| !ability_holds && !self.keeps_attack());
        if upper.is_some()
            && movement.is_none()
            && !self.keeps_attack()
            && !ability_holds
            && self.air.is_none()
        {
            // The walk changes with the block or aim: retail sent the new walk sequence in the
            // state of the right-click (`15=af10` with the aim). A draw that arrives after a
            // tap has already started the light must not replace that attack with a walk.
            movement = Some(self.locomotion.resend(set));
        }
        if movement.is_none()
            && record.action.is_none()
            && self.pending.is_none()
            && self.locomotion.moving()
            && self.played_state == Some(set.idle.0)
            && !self.keeps_attack()
            && !ability_holds
            && self.air.is_none()
            && self.roll_s.is_none()
            && !record.swimming
        {
            // A body that moves goes on in its move when a stand plays (a stand the client
            // named in the move's first record took its place). Observed: 3 of 4 retail block
            // stands that started while moving went on in the block walk 15 to 234 ms later.
            // Live 2026-11-20: a controller player's stand `31/1249` played for 848 ms while the
            // body moved, and the others saw it glide with no legs.
            movement = Some(self.locomotion.resend(set));
        }
        if let Some(upper) = upper {
            self.upper = Some(merge(self.upper.unwrap_or_default(), upper));
        }
        let shot = self.shot_slots(&record, stance, drawn);
        let reloads = matches!(shot, Some(Shot::Reload(_)));
        match shot {
            Some(Shot::Reload(slots) | Shot::Drawn(slots)) => {
                self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
            }
            Some(Shot::Fire(slots, steps)) => {
                self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
                self.fired.push(steps.fired(Some(record.counter)));
                self.shot_steps = Some(*steps);
                self.alt_resume = None;
            }
            None => {}
        }
        let server = self.server_start(&record, table, stance, moves_stance, in_air);
        if reloads {
            // The reload's press is no tap: its release starts no attack.
            self.press = None;
        }
        let started = record
            .action
            .map(Pending::Client)
            .or(server)
            .or_else(|| movement.map(|(state, sequence)| Pending::State(state, sequence)))
            .or(move_sequence)
            .or_else(|| stroke.map(Pending::Sequence));
        let posture_sequence =
            self.posture_sequence(posture_change, set, movement.is_some(), started.is_some());
        let started = started.or_else(|| {
            posture_sequence
                .filter(|_| !ability_holds)
                .map(Pending::Sequence)
        });
        self.break_alt(started, stance, alt, record.counter);
        if let Some(Pending::Client(input)) = started {
            self.mark_named_attack(u64::from(input.state), record.counter);
        }
        if started.is_some_and(Self::rolls) && self.marker_set() {
            // A dodge clears the slayer marker that is set (Observed,
            // `retail-2026-11-19-towns-duels`: 165 of the 166 other players' dodges that started
            // with the marker set carried `48=00`, after a block's or an aim's marker too; 276
            // of the 284 with it clear carried no child 48). Ours sent none.
            let clear = UpperSlots {
                marker: Some(CLEARED_SLOT),
                ..UpperSlots::default()
            };
            self.upper = Some(merge(self.upper.unwrap_or_default(), clear));
        }
        self.alt_hold_step(stance, record.frame_s, record.counter);
        if let Some(input) = record.action
            && u64::from(input.state) != set.idle.0
        {
            // The attack the client named runs now: a release or a hold follows it. A stand the
            // client names is none: the move goes on after it (live 2026-11-20: a controller
            // player's named stand held the move back as an attack for 1.1 s).
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
            self.begin(started, &record);
        }
    }

    /// `started` starts in `record`: it goes out with the next own state.
    fn begin(&mut self, started: Pending, record: &SlayerRecord) {
        self.pending = Some((started, record.counter, record.frame_s));
        self.roll_s = Self::rolls(started).then_some(0.0);
        match started {
            // A new sequence of a state that goes on keeps that state's time.
            Pending::Sequence(_) => {}
            Pending::Run(_) => self.sequence_s = self.state_from_s,
            _ => {
                self.sequence_s = record.frame_s;
                self.state_from_s = record.frame_s;
            }
        }
        if !matches!(started, Pending::Client(_)) {
            self.follow_up = None;
        }
    }

    /// The ride's start of `record` ([`rider`]): the mount-up when the record rides first, its
    /// ride or stand once it played, the ride's walk, dash, stops and stands by the keys, its
    /// jump, fall and landing, the dismount when the record rides no more, and then the run or
    /// the stand on foot of `set`.
    fn ride_step(&mut self, record: &SlayerRecord, set: MoveSet) -> Option<Pending> {
        let intent = record.intent.unwrap_or_default();
        let moving = intent.moves();
        let pressed_jump = intent.jump && !self.jump_held;
        self.jump_held = intent.jump;
        self.dodge_held = intent.dodge;
        let frame_s = record.frame_s;
        let state = |state: u64, sequence: u64| Some(Pending::State(state, sequence));
        match (self.ride, record.ride) {
            (Ride::Foot, None) => None,
            (Ride::Foot | Ride::Dismounting { .. }, Some(keys)) => {
                self.locomotion = Locomotion::Idle;
                self.ride = Ride::MountingUp {
                    plays: keys.plays,
                    moving,
                    since_s: 0.0,
                };
                let up = keys.plays.mount_up;
                state(
                    rider::MOUNT_UP_STATE,
                    if moving { up.moving } else { up.still },
                )
            }
            (
                Ride::MountingUp {
                    plays,
                    moving: up_moving,
                    since_s,
                },
                Some(_),
            ) => {
                let since_s = since_s + frame_s;
                let (mark_s, play, legs) = if up_moving {
                    (
                        rider::MOUNT_UP_MOVING_S,
                        (rider::RIDE_STATE, plays.after_mount_up),
                        RideLegs::Ride,
                    )
                } else {
                    (
                        rider::MOUNT_UP_STILL_S,
                        (rider::IDLE_STATE, plays.idle),
                        RideLegs::Idle,
                    )
                };
                if reached(since_s, frame_s, mark_s) {
                    self.ride = Ride::Riding {
                        plays,
                        legs,
                        air: RideAir::Ground,
                    };
                    return state(play.0, play.1);
                }
                self.ride = Ride::MountingUp {
                    plays,
                    moving: up_moving,
                    since_s,
                };
                None
            }
            (Ride::Riding { plays, legs, air }, Some(keys)) => {
                let (next, started) =
                    self.riding_step((plays, legs, air), keys, record, (moving, pressed_jump));
                self.ride = next;
                started
            }
            (Ride::MountingUp { plays, .. } | Ride::Riding { plays, .. }, None) => {
                self.ride = Ride::Dismounting {
                    moving,
                    since_s: 0.0,
                };
                let off = plays.dismount;
                state(
                    rider::DISMOUNT_STATE,
                    if moving { off.moving } else { off.still },
                )
            }
            (
                Ride::Dismounting {
                    moving: off_moving,
                    since_s,
                },
                None,
            ) => {
                let since_s = since_s + frame_s;
                let mark_s = if off_moving {
                    rider::DISMOUNT_MOVING_S
                } else {
                    rider::DISMOUNT_STILL_S
                };
                if !reached(since_s, frame_s, mark_s) {
                    self.ride = Ride::Dismounting {
                        moving: off_moving,
                        since_s,
                    };
                    return None;
                }
                self.ride = Ride::Foot;
                if !moving {
                    self.locomotion = Locomotion::Idle;
                    return state(set.idle.0, set.idle.1);
                }
                // The dismount's run (Observed: 2,565 of the moving dismounts went on in a run
                // `13=1a`, the sheathed 3050 in 2,438).
                self.locomotion = Locomotion::Moving {
                    back: intent.forward < 0,
                    state: set.start.0,
                    start_s: None,
                };
                let play = self.locomotion.resumed(set);
                state(play.0, play.1)
            }
        }
    }

    /// One record of a ride on the ground or in the air ([`rider`]): the ride's next place and
    /// its start, when it starts one. A new sequence of the ride goes out only while the ride
    /// state plays.
    fn riding_step(
        &self,
        (plays, legs, air): (RiderPlays, RideLegs, RideAir),
        keys: RideKeys,
        record: &SlayerRecord,
        (moving, pressed_jump): (bool, bool),
    ) -> (Ride, Option<Pending>) {
        let frame_s = record.frame_s;
        let (target, flowing) = if keys.dashing {
            (plays.fast, RideLegs::Fast)
        } else {
            (plays.ride, RideLegs::Ride)
        };
        let riding = |legs, air| Ride::Riding { plays, legs, air };
        let state = |state: u64, sequence: u64| Some(Pending::State(state, sequence));
        match air {
            RideAir::Ground if pressed_jump => {
                return (
                    riding(legs, RideAir::Jumped),
                    state(rider::JUMP_STATE, plays.jump),
                );
            }
            RideAir::Ground if record.airborne => {
                return (
                    riding(legs, RideAir::Fell),
                    state(rider::FALL_STATE, plays.fall),
                );
            }
            RideAir::Jumped | RideAir::Fell if !record.airborne => {
                let (sequence, after_s) = if air == RideAir::Jumped {
                    (plays.land, rider::LAND_S)
                } else {
                    (plays.fall_land, rider::FALL_LAND_S)
                };
                return (
                    riding(
                        legs,
                        RideAir::Landed {
                            since_s: 0.0,
                            after_s,
                        },
                    ),
                    state(rider::LAND_STATE, sequence),
                );
            }
            RideAir::Landed { since_s, after_s } => {
                let since_s = since_s + frame_s;
                if !reached(since_s, frame_s, after_s) {
                    return (riding(legs, RideAir::Landed { since_s, after_s }), None);
                }
                // The landing goes on in the ride (Observed: 2,326 landings, every one in
                // `13=21`), or in its walk to a stop.
                return if moving {
                    (
                        riding(flowing, RideAir::Ground),
                        state(rider::RIDE_STATE, target),
                    )
                } else {
                    (
                        riding(RideLegs::Stopping(0.0), RideAir::Ground),
                        state(rider::RIDE_STATE, plays.slow),
                    )
                };
            }
            RideAir::Jumped | RideAir::Fell => return (riding(legs, air), None),
            RideAir::Ground => {}
        }
        let sequence = |sequence: u64| Some(Pending::Sequence(sequence));
        let (legs, started) = match legs {
            RideLegs::Idle if moving => (
                RideLegs::Starting(0.0),
                state(rider::RIDE_STATE, plays.slow),
            ),
            RideLegs::Idle => (legs, None),
            RideLegs::Starting(since_s) if moving => {
                let since_s = since_s + frame_s;
                if reached(since_s, frame_s, rider::START_S) {
                    (flowing, sequence(target))
                } else {
                    (RideLegs::Starting(since_s), None)
                }
            }
            RideLegs::Starting(_) => (RideLegs::Stopping(0.0), None),
            RideLegs::Ride | RideLegs::Fast if moving && legs == flowing => (legs, None),
            RideLegs::Ride | RideLegs::Fast if moving => (flowing, sequence(target)),
            RideLegs::Ride | RideLegs::Fast => (RideLegs::Stopping(0.0), sequence(plays.slow)),
            RideLegs::Stopping(_) if moving => (flowing, sequence(target)),
            RideLegs::Stopping(since_s) => {
                let since_s = since_s + frame_s;
                if reached(since_s, frame_s, rider::STOP_S) {
                    (RideLegs::Idle, state(rider::IDLE_STATE, plays.idle))
                } else {
                    (RideLegs::Stopping(since_s), None)
                }
            }
        };
        let shown = self.played_state == Some(rider::RIDE_STATE) && self.pending.is_none();
        let started = started.filter(|started| !matches!(started, Pending::Sequence(_)) || shown);
        (riding(legs, RideAir::Ground), started)
    }

    /// The right-click's start of `stance`: for a block or aim with its upper body by legs
    /// ([`AltHold`]), the start of the legs it plays on now, whose hold follows
    /// ([`SlayerTrack::alt_hold_step`]).
    ///
    /// [`AltHold`]: nw_content::player_sequences::AltHold
    fn start_alt(&mut self, stance: &StanceSequences) -> Option<SlotsChange> {
        let start = stance.alt_start?;
        let Some(hold) = stance.alt_hold else {
            return Some(start);
        };
        let walking = self.locomotion.moving();
        let sequence = hold.legs(walking).start;
        let third = AimThird::of(hold, walking);
        self.alt_play = Some(AltPlay {
            walking,
            since_s: 0.0,
            holding: false,
            sequence,
            third,
        });
        Some(SlotsChange {
            second_sequence: Some(sequence),
            third_sequence: third.map(|third| third.sent).or(start.third_sequence),
            ..start
        })
    }

    /// The upper body of the block or aim that plays, after record `counter` of `frame_s`: its
    /// hold [`AltHold::hold_after_ms`] after its start, and at a change between walking and
    /// standing the new legs' hold (their start while the first has not reached its hold:
    /// retail went to the new legs' start in 78 and 110 of such early changes of the spear's
    /// block, to their hold in 63 and 79). Each sends child 18 alone, and only a sequence the
    /// second slot does not play already (the rifle's aim is the same on both legs). While an
    /// aimed shot's steps go out it waits; then it goes on from the sequence they left: a hold
    /// (the bow's `18=29`), or the start again (the musket's `18=3544`), whose hold follows.
    ///
    /// [`AltHold::hold_after_ms`]: nw_content::player_sequences::AltHold::hold_after_ms
    fn alt_hold_step(&mut self, stance: Option<&StanceSequences>, frame_s: f32, counter: u16) {
        let Some(hold) = stance.and_then(|stance| stance.alt_hold) else {
            return;
        };
        let walking = self.locomotion.moving();
        self.aim_third_step(walking);
        if self
            .shot_steps
            .as_ref()
            .is_some_and(|steps| steps.kind == ShotKind::Aimed)
        {
            return;
        }
        let Some(play) = self.alt_play.as_mut() else {
            return;
        };
        if let Some(sequence) = self.alt_resume.take() {
            play.sequence = sequence;
            play.holding = sequence != hold.legs(play.walking).start;
            if !play.holding {
                play.since_s = 0.0;
            }
        }
        let legs = hold.legs(walking);
        let wanted = if walking == play.walking {
            play.since_s += frame_s;
            #[allow(clippy::cast_precision_loss, reason = "a hold time of milliseconds")]
            let hold_after_s = hold.hold_after_ms as f32 / MS_PER_S;
            if play.since_s >= hold_after_s {
                play.holding = true;
            }
            if play.holding { legs.hold } else { legs.start }
        } else {
            play.walking = walking;
            if play.holding {
                legs.hold
            } else {
                play.since_s = 0.0;
                legs.start
            }
        };
        if wanted == play.sequence {
            return;
        }
        play.sequence = wanted;
        let change = SlotsChange {
            second_sequence: Some(wanted),
            ..SlotsChange::default()
        };
        if let Some(slots) = slots_of(change, counter) {
            self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
        }
    }

    /// The aim's third slot on walking (`walking`) or standing legs: at a change of legs, the
    /// new legs' sequence in the state of the first slot's change, while a shot's steps go out
    /// too ([`AltLegs::third`]). The sequence alone: retail sent no third state (child 19) and
    /// no new start (child 20) with 44,688 of 46,603 such changes of all players.
    ///
    /// [`AltLegs::third`]: nw_content::player_sequences::AltLegs::third
    fn aim_third_step(&mut self, walking: bool) {
        let Some(third) = self.alt_play.as_mut().and_then(|play| play.third.as_mut()) else {
            return;
        };
        let wanted = third.on(walking);
        if wanted == third.sent {
            return;
        }
        third.sent = wanted;
        self.upper = Some(merge(self.upper.unwrap_or_default(), third_alone(wanted)));
    }

    /// Plays the third slot `thirds` of a full draw hold in place of the aim's
    /// ([`AimThird::drawing`]): its sequence of the legs the player is on goes out alone when the
    /// third slot does not play it already.
    fn draw_third(&mut self, thirds: LegThirds) {
        let walking = self.locomotion.moving();
        let Some(third) = self.alt_play.as_mut().and_then(|play| play.third.as_mut()) else {
            return;
        };
        if let Some(sequence) = third.drawing(thirds, walking) {
            self.upper = Some(merge(self.upper.unwrap_or_default(), third_alone(sequence)));
        }
    }

    /// Sends the full draw's hold due by own state time `state_ms`, while the key stays held
    /// ([`DrawHold`]): its clock starts at the own state that sends the full draw; at its time
    /// its second-slot sequence alone with its third slot of the legs
    /// ([`SlayerTrack::draw_third`]), then its steps.
    fn step_draw(&mut self, state_ms: u64) {
        let Some(play) = self.aimed_draw.as_mut().and_then(|draw| draw.hold.as_mut()) else {
            return;
        };
        let from_ms = *play.from_ms.get_or_insert(state_ms);
        let mut due = Vec::new();
        let starts = play.held_ms.is_none();
        if starts {
            if state_ms.saturating_sub(from_ms) < play.hold.after_ms {
                return;
            }
            play.held_ms = Some(state_ms);
            due.push(SlotsChange {
                second_sequence: Some(play.hold.sequence),
                ..SlotsChange::default()
            });
        }
        let held_ms = play.held_ms.unwrap_or(state_ms);
        while let Some(step) = play.hold.after.get(play.next) {
            if state_ms.saturating_sub(held_ms) < step.after_ms {
                break;
            }
            due.push(step.slots);
            play.next += 1;
        }
        let thirds = starts.then(|| LegThirds::of_draw(&play.hold));
        let counter = self.newest_counter;
        for change in due {
            if let Some(slots) = slots_of(change, counter) {
                self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
            }
        }
        if let Some(thirds) = thirds {
            self.draw_third(thirds);
        }
    }

    /// Ends the block that plays when `started` (in record `counter`) is a dodge, a jump or an
    /// attack, in the same state; with the right-click still held (`alt`), starts it again
    /// with the next move or stand. Observed in 400 retail ledgers: 986 of 986 dodges, 7 of 7
    /// jumps and 119 of 123 attacks that started during a held block carried the block's end
    /// (`16=45 18=00`); with the block still held it started again with the move after the
    /// dodge (12 cases, 0.45 to 0.63 s after the dodge's start). Ours kept the block, and the
    /// other players saw no roll under its pose (live 2026-11-18).
    fn break_alt(
        &mut self,
        started: Option<Pending>,
        stance: Option<&StanceSequences>,
        alt: bool,
        counter: u16,
    ) {
        let breaks = |started: Pending| match started {
            Pending::State(state, _) => state == DODGE_STATE || state == JUMP_STATE,
            Pending::Client(input) => {
                [DODGE_STATE, JUMP_STATE, ATTACK_PRESS_STATE].contains(&u64::from(input.state))
            }
            Pending::Attack(_) | Pending::Press(..) => true,
            Pending::Sequence(_) | Pending::Run(_) | Pending::Ability(_) => false,
        };
        let moves = |started: Pending| {
            matches!(started, Pending::State(state, _) if [
                movement::START_STATE,
                movement::MOVE_STATE,
                movement::IDLE_STATE,
            ]
            .contains(&state))
        };
        // Only a block: the retail counts were of block starts (`16=37`); an aim's shots and
        // dodges were not counted, and an aimed shot plays inside its aim.
        let blocks = stance.is_some_and(|stance| stance.alt_fire.is_none());
        let change = match started {
            Some(started) if blocks && self.alt_held && breaks(started) => {
                self.alt_held = false;
                self.alt_broken = true;
                self.alt_play = None;
                stance.and_then(alt_end)
            }
            Some(started) if self.alt_broken && moves(started) => {
                self.alt_broken = false;
                self.alt_held = alt;
                stance
                    .filter(|_| alt)
                    .and_then(|stance| self.start_alt(stance))
            }
            _ => None,
        };
        if let Some(slots) = change.and_then(|change| slots_of(change, counter)) {
            self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
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
            let moving = self.locomotion.moving();
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

    /// Notes how the body of `record` is in the air, and gives the landing it starts when it is
    /// back on a floor: state [`LAND_STATE`] with the stance's landing (`air`) after a jump or a
    /// fall, by the move key (Observed, 2026-11-17 study: after a jump the sheathed landing was
    /// 789 with a move key, 2376 without; after a fall 1355). No landing without the stance's
    /// sequence; its move or stand still follows ([`SlayerTrack::settled_landing`]).
    fn air_step(&mut self, record: &SlayerRecord, air: Option<AirSequences>) -> Option<(u64, u64)> {
        if record.swimming {
            self.air = None;
            return None;
        }
        if record.airborne {
            if self.air.is_none() {
                let jumped = self
                    .jumped_at
                    .is_some_and(|at| record.counter.wrapping_sub(at) <= JUMP_LIFT_RECORDS);
                self.air = Some(if jumped { AirWay::Jumped } else { AirWay::Fell });
            }
            return None;
        }
        let way = self.air.take()?;
        self.land_settle = Some(LAND_MOVE_RECORDS);
        let moving = record.intent.is_some_and(Intent::moves);
        let landing = match way {
            AirWay::Jumped => air?.land_after_jump,
            AirWay::Fell => air?.land_after_fall,
        };
        landing.of(moving).map(|sequence| (LAND_STATE, sequence))
    }

    /// Whether the slayer marker is set: by the last change of it that went out, or by one that
    /// waits to go out.
    fn marker_set(&self) -> bool {
        self.upper
            .and_then(|upper| upper.marker)
            .map_or(self.slots_on.marker, |marker| marker != CLEARED_SLOT)
    }

    /// The slayer marker of a press start (`13=10`) that opens its chain with `opens`: cleared
    /// when it follows an attack of a chain that holds the marker, else `opens`. Observed (1,206
    /// ledgers, the starts of `13=10`): after a `13=08` with the marker set, 16,445 of the 19,904
    /// other players' and 2,945 own carried `48=0` (219 own carried none); the others carried the
    /// marker set (`48=1`), 13,588 after a dodge, 13,356 after a move start, 7,446 after another
    /// press start. Ours sent none with the presses the client named: 7 of 14 (live 2026-11-19c
    /// and 19e).
    fn press_marker(&mut self, opens: u64) -> u64 {
        let ends = self.marker_set() && self.played_state == Some(CHAIN_ATTACK_STATE);
        self.chain_marked = !ends;
        if ends { CLEARED_SLOT } else { opens }
    }

    /// The slayer marker of an attack the client names in first-slot state `state` in record
    /// `counter`: a press start's ([`SlayerTrack::press_marker`]); an attack of the chain
    /// (`13=08`) sets it when it is clear (Observed, 1,206 ledgers: 19,173 of the other players'
    /// `13=08` starts after a move start carried `48=1`, 20,002 after a press start with the
    /// marker set carried no child 48).
    fn mark_named_attack(&mut self, state: u64, counter: u16) {
        let marker = match state {
            ATTACK_PRESS_STATE => self.press_marker(CHAIN_MARKER),
            CHAIN_ATTACK_STATE if !self.marker_set() => {
                self.chain_marked = true;
                CHAIN_MARKER
            }
            _ => return,
        };
        let change = SlotsChange {
            marker: Some(marker),
            ..SlotsChange::default()
        };
        if let Some(slots) = slots_of(change, counter) {
            self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
        }
    }

    /// Whether `started` starts a dodge: the server's or one the client names.
    fn rolls(started: Pending) -> bool {
        match started {
            Pending::State(state, _) => state == DODGE_STATE,
            Pending::Client(input) => u64::from(input.state) == DODGE_STATE,
            _ => false,
        }
    }

    /// Counts a record of `frame_s` into the dodge that plays; whether the dodge ends in it, so
    /// that its movement goes on ([`DODGE_END_S`]).
    fn roll_end(&mut self, frame_s: f32) -> bool {
        let Some(since_s) = self.roll_s.map(|since_s| since_s + frame_s) else {
            return false;
        };
        let ends = reached(since_s, frame_s, DODGE_END_S);
        self.roll_s = (!ends).then_some(since_s);
        ends
    }

    /// The move or stand a landing lands into, once its records passed ([`LAND_MOVE_RECORDS`]);
    /// none when a move `started` in this record already, which ends the wait.
    fn settled_landing(&mut self, started: bool, set: MoveSet) -> Option<(u64, u64)> {
        let left = self.land_settle?;
        if started {
            self.land_settle = None;
            return None;
        }
        if left > 1 {
            self.land_settle = Some(left - 1);
            return None;
        }
        self.land_settle = None;
        Some(self.locomotion.resumed(set))
    }

    /// The state the server starts for the keys of `record` with the sequences of `stance`:
    /// a dodge or a jump at its key's press (state 07 and 0e, by direction), else an attack
    /// ([`SlayerTrack::attack_start`]). A dodge starts only when the body starts one, never in
    /// the body's dodge ([`SlayerRecord::dodging`]; live 2026-11-20: 5 of a pad player's 25
    /// dodges started again with a new stamp 164 to 200 ms after their start, at a second press,
    /// while the body went on in the first; of 510 retail dodges seen by other players
    /// (`retail-2026-11-19-towns-duels`) 13 started again with a new stamp, 12 of them 400 to
    /// 486 ms after the first's start).
    fn server_start(
        &mut self,
        record: &SlayerRecord,
        table: &PlayerSequences,
        stance: Option<&StanceSequences>,
        moves_stance: Option<&StanceSequences>,
        in_air: bool,
    ) -> Option<Pending> {
        let intent = record.intent?;
        let dodge = intent.dodge && !self.dodge_held && !in_air && !record.dodging;
        let jump = intent.jump && !self.jump_held && !in_air;
        self.dodge_held = intent.dodge;
        self.jump_held = intent.jump;
        if jump {
            self.jumped_at = Some(record.counter);
        }
        let direction = Direction::of_keys(
            intent.forward > 0,
            intent.forward < 0,
            intent.right < 0,
            intent.right > 0,
        );
        let state = |play: StatePlay| Pending::State(play.state, play.sequence);
        let keyed = if dodge {
            table.dodge(moves_stance, record.load, direction).map(state)
        } else if jump {
            moves_stance
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
        }
        self.end_attack(counter, stance);
        let running = self
            .running
            .filter(|(_, started)| counter.wrapping_sub(*started) < ATTACK_RUN_RECORDS);
        if record.attack.released {
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
            (AttackSlot::First, Some(press)) => SlotsChange {
                marker: Some(self.press_marker(press.marker)),
                ..SlotsChange::default()
            },
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
        let held = drawn && record.intent.is_some_and(|intent| intent.block);
        if !held {
            // Let go during the action that ended it: nothing to start again.
            self.alt_broken = false;
        }
        // A hip shot that plays holds a new aim back until its aim time
        // ([`HipShot::aim_at_ms`]); the right-click held starts it there. Ours started the aim
        // at once and dropped the hip shot's reload.
        let hip_holds =
            !self.alt_held && stance.is_some_and(|stance| self.hip_aim_left_ms(stance).is_some());
        let alt = held && !self.alt_broken && !hip_holds;
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
            // The sheathe of the weapon put away, ended after its time ([`SecondEnd`]).
            let sequence = stance
                .and_then(|stance| stance.sheathe)
                .unwrap_or(sheathe::SEQUENCE);
            let after_ms = stance.and_then(|stance| stance.sheathe_end_ms);
            self.end_second_later(sequence, after_ms);
            self.drop_shot();
            return slots_of(second(DRAW_STATE, sequence), record.counter);
        }
        let stance = stance?;
        if !alt && record.intent.is_some() {
            self.alt_play = None;
        }
        let alt_starts = alt && !was_alt;
        let alt_ends = !alt && was_alt && record.intent.is_some();
        let draws = drawn && (!was_drawn || was_stance != self.drawn_stance);
        if alt_starts {
            // An aimed shot asked for while the aim was held back waits for the aim
            // ([`SlayerTrack::aim_ready_in_ms`]).
            self.drop_shot_steps();
        } else if alt_ends || draws {
            self.drop_shot();
        }
        let change = (if alt_starts {
            self.aim_start_due = true;
            self.start_alt(stance)
        } else if alt_ends {
            alt_end(stance)
        } else if draws {
            // A draw, and a swap to the other weapon while one is out: retail starts the new
            // weapon's draw at every weapon key (351 presses: `16=2c 18=<draw>` with the new
            // stance's sequence in the first slot). Ours started it from no weapon only, and
            // the other players kept the old weapon's pose (live 2026-11-09).
            stance.draw.map(|draw| {
                self.end_second_later(draw.sequence, stance.draw_end_ms);
                second(draw.state, draw.sequence)
            })
        } else {
            None
        })?;
        slots_of(change, record.counter)
    }

    /// The shots that went out since the last call, oldest first ([`FiredShot`]).
    pub(super) fn take_fired(&mut self) -> Vec<FiredShot> {
        std::mem::take(&mut self.fired)
    }

    /// Drops the shot that plays: the aim's start or end, a draw, a sheathe, a hit reaction or
    /// an ability takes the upper slots, and the shot's later steps would put the shot's pose
    /// back over it (Inferred); the aim's third slot by legs plays the aim's again, not a full
    /// draw hold's (Inferred).
    fn drop_shot(&mut self) {
        self.drop_shot_steps();
        self.aimed_draw = None;
        self.held_shot = None;
    }

    /// Drops the steps of the shot that plays ([`SlayerTrack::drop_shot`]), not an aimed draw or
    /// an aimed shot that waits.
    fn drop_shot_steps(&mut self) {
        if let Some(third) = self.alt_play.as_mut().and_then(|play| play.third.as_mut()) {
            third.playing = third.aim;
        }
        self.shot_steps = None;
        self.alt_resume = None;
    }

    /// Ends the second slot's draw or sheathe `sequence` `after_ms` after its first state (else
    /// [`SECOND_END_MS`]), unless another second-slot change comes first. Live 2026-11-17: the
    /// server never ended a draw, and its pose hid the upper body's attacks from the other
    /// players.
    fn end_second_later(&mut self, sequence: u64, after_ms: Option<u64>) {
        self.second_end = Some(SecondEnd {
            sequence,
            after_ms: after_ms.unwrap_or(SECOND_END_MS),
            at_ms: None,
        });
    }

    /// The upper slot change of the attack key of `record` with the weapon drawn ([`Shot`]): at
    /// a press, the stance's reload when the weapon was loaded again after it emptied and no
    /// reload played since; else, with the right-click held, the aimed shot
    /// ([`SlayerTrack::aimed_shot`]); else, with a shot loaded and no ability playing, the hip
    /// shot ([`SlayerTrack::hip_shot`]). Retail blunderbuss (newchar 2026-09-18): the aimed shot
    /// `18=5e 48=29` 27 ms after the press; the presses while empty changed nothing; the first
    /// press 2.1 s after the shot sent the reload `16=24 17=557a 18=8142`, and the next press shot
    /// again. A stance without a reload shoots at that press (the musket and the pinned build's
    /// blunderbuss: their reload is a step of their shot, [`StanceSequences::alt_fire_after`] and
    /// [`StanceSequences::alt_fire_empty_after`]).
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
        let aiming = drawn && record.intent.is_some_and(|intent| intent.block);
        if !aiming {
            // Let go before the aim was up (a hip shot held it back): no aimed shot.
            self.aimed_draw = None;
            self.held_shot = None;
        }
        if !drawn {
            return None;
        }
        let stance = stance?;
        if record.attack.pressed
            && std::mem::take(&mut self.reload_due)
            && let Some(reload) = stance.reload
        {
            return slots_of(reload, record.counter).map(Shot::Reload);
        }
        if aiming {
            return self.aimed_shot(record, stance, empty);
        }
        if !record.attack.pressed || empty || self.ability.is_some() {
            return None;
        }
        self.hip_shot(stance, record.counter)
    }

    /// The aimed shot of `stance` for the attack key of `record`, aiming, `empty` or with a shot
    /// loaded. A weapon that shoots at the press (the musket, the blunderbuss) sends
    /// [`StanceSequences::alt_fire`] there. One that shoots at the release (the bow,
    /// [`StanceSequences::alt_fire_at_release`]) draws from the press: the full draw goes out
    /// once the key was held the stance's full draw time ([`StanceSequences::alt_charge`]), and
    /// the release sends the full draw's shot after it, else the plain shot; the full draw's
    /// hold goes on while the key stays held ([`SlayerTrack::step_draw`]), and its shot sends
    /// the hold's third slot of the legs when the third slot does not play it already
    /// ([`DrawHold`]: retail sent it when the release came before the hold; a shot held for the
    /// shot gap sends it at the release, Inferred). Live 2026-11-18:
    /// ours sent the bow's shot at the press and nothing at the release, and the other players
    /// saw no arrow leave.
    fn aimed_shot(
        &mut self,
        record: &SlayerRecord,
        stance: &StanceSequences,
        empty: bool,
    ) -> Option<Shot> {
        let counter = record.counter;
        if !stance.alt_fire_at_release {
            if !record.attack.pressed || empty {
                return None;
            }
            let steps = ShotSteps::aimed(&stance.alt_fire_after, &stance.alt_fire_empty_after);
            return self.aimed_fire(stance, stance.alt_fire?, steps, counter);
        }
        if record.attack.pressed && !empty {
            self.aimed_draw = Some(AimedDraw {
                since_s: 0.0,
                full: false,
                hold: None,
            });
        } else if !record.attack.released
            && let Some(draw) = self.aimed_draw.as_mut()
        {
            draw.since_s += record.frame_s;
            let charge = stance.alt_charge.as_ref()?;
            #[allow(clippy::cast_precision_loss, reason = "a draw time of milliseconds")]
            let full_s = charge.after_ms as f32 / MS_PER_S;
            if draw.full || draw.since_s < full_s {
                return None;
            }
            draw.full = true;
            draw.hold = charge.hold.clone().map(|hold| DrawPlay {
                hold,
                from_ms: None,
                held_ms: None,
                next: 0,
            });
            return slots_of(charge.drawn, counter).map(Shot::Drawn);
        }
        if !record.attack.released {
            return None;
        }
        let draw = self.aimed_draw.take()?;
        let (change, steps) = match stance.alt_charge.as_ref().filter(|_| draw.full) {
            Some(charge) => {
                if let Some(hold) = charge.hold.as_ref() {
                    self.draw_third(LegThirds::of_draw(hold));
                }
                let steps = ShotSteps {
                    attack: AttackKind::Charged,
                    ..ShotSteps::aimed(&charge.fire_after, &[])
                };
                (charge.fire, steps)
            }
            None => (
                stance.alt_fire?,
                ShotSteps::aimed(&stance.alt_fire_after, &stance.alt_fire_empty_after),
            ),
        };
        self.aimed_fire(stance, change, steps, counter)
    }

    /// The aimed shot `change` of `stance` started in record `counter`, with its `steps`; before
    /// the stance's shortest time from the aimed shot before
    /// ([`StanceSequences::alt_fire_gap_ms`]) it waits for that time, then goes out with
    /// [`SlayerTrack::step_shot`]; asked for before the stance's wait starts
    /// ([`StanceSequences::alt_fire_wait_from_ms`], the musket's reload), it fires nothing. A
    /// release or a press while a shot waits shoots no other. Live 2026-11-20: a musket's presses
    /// during its reload each started a new shot for the other players (one every 200 ms while
    /// the player pressed fast), as its steps ended before the gap.
    fn aimed_fire(
        &mut self,
        stance: &StanceSequences,
        change: SlotsChange,
        steps: ShotSteps,
        counter: u16,
    ) -> Option<Shot> {
        if self.held_shot.is_some() {
            return None;
        }
        let early_ms = stance.alt_fire_gap_ms.and_then(|gap_ms| {
            let since_ms = self.since_aimed_shot_ms()?;
            (since_ms < gap_ms).then_some((since_ms, gap_ms))
        });
        if let Some((since_ms, _)) = early_ms
            && stance
                .alt_fire_wait_from_ms
                .is_some_and(|wait_from_ms| since_ms < wait_from_ms)
        {
            return None;
        }
        let wait_ms = early_ms
            .map_or(0, |(since_ms, gap_ms)| gap_ms - since_ms)
            .max(self.aim_ready_in_ms(stance));
        if wait_ms > 0 {
            self.held_shot = Some(HeldShot {
                change,
                steps,
                at_ms: self
                    .newest_state_ms
                    .unwrap_or_default()
                    .saturating_add(wait_ms),
                aim_ready_ms: stance.alt_fire_ready_ms,
            });
            return None;
        }
        slots_of(change, counter).map(|slots| Shot::Fire(slots, Box::new(steps)))
    }

    /// Milliseconds from the newest own state until the aim lets an aimed shot of `stance` go
    /// out ([`StanceSequences::alt_fire_ready_ms`] after the aim's start): an aim start no own
    /// state carried yet starts now, and an aim a hip shot holds back starts at its aim time
    /// ([`SlayerTrack::hip_aim_left_ms`]). 0 for a stance with no ready time.
    fn aim_ready_in_ms(&self, stance: &StanceSequences) -> u64 {
        let Some(ready_ms) = stance.alt_fire_ready_ms else {
            return 0;
        };
        if let Some(left_ms) = self.hip_aim_left_ms(stance) {
            return left_ms.saturating_add(ready_ms);
        }
        let since_start_ms = if self.aim_start_due {
            0
        } else {
            match self.newest_state_ms.zip(self.aim_from_ms) {
                Some((at_ms, from_ms)) => at_ms.saturating_sub(from_ms),
                None => return 0,
            }
        };
        ready_ms.saturating_sub(since_start_ms)
    }

    /// Milliseconds left until the hip shot of `stance` that plays lets the aim start
    /// ([`HipShot::aim_at_ms`] from its own state, or its re-fire's; all of it while its own
    /// state did not go out yet); `None` when no hip shot holds it.
    fn hip_aim_left_ms(&self, stance: &StanceSequences) -> Option<u64> {
        let aim_at_ms = stance.hip.as_ref()?.aim_at_ms();
        let steps = self
            .shot_steps
            .as_ref()
            .filter(|steps| matches!(steps.kind, ShotKind::Hip { .. }))?;
        let elapsed_ms = steps.elapsed_ms().unwrap_or(0);
        (elapsed_ms < aim_at_ms).then(|| aim_at_ms - elapsed_ms)
    }

    /// Milliseconds from the last aimed shot's own state to the newest own state: 0 while an
    /// aimed shot started and no own state carried it yet; `None` with no aimed shot before.
    fn since_aimed_shot_ms(&self) -> Option<u64> {
        if self
            .shot_steps
            .as_ref()
            .is_some_and(|steps| steps.kind == ShotKind::Aimed && steps.from_ms.is_none())
        {
            return Some(0);
        }
        Some(self.newest_state_ms?.saturating_sub(self.aimed_from_ms?))
    }

    /// The hip shot of `stance` for an attack key press in record `counter`: its start, and its
    /// steps and end after it ([`HipShot`]). A press while the hip shot plays re-fires, when the
    /// stance has a re-fire: at once in the press's own state inside its window
    /// ([`HipShot::refire_window`], the blunderbuss; the re-fire's steps count from there, and a
    /// press outside it starts nothing), else where the end was due (the bow). Live
    /// 2026-11-18: the server sent nothing for 57 of 71 bow hip presses, and the other players
    /// saw no shot.
    fn hip_shot(&mut self, stance: &StanceSequences, counter: u16) -> Option<Shot> {
        let hip = stance.hip.as_ref()?;
        if let Some(steps) = self.shot_steps.as_mut() {
            let elapsed_ms = steps.elapsed_ms();
            if let ShotKind::Hip {
                refire,
                refire_due,
                window,
            } = &mut steps.kind
            {
                let Some(window) = window else {
                    *refire_due = refire.is_some();
                    return None;
                };
                let refire = refire.filter(|_| elapsed_ms.is_some_and(|ms| window.holds(ms)))?;
                return slots_of(refire, counter)
                    .map(|slots| Shot::Fire(slots, Box::new(ShotSteps::hip(hip))));
            }
        }
        slots_of(hip.start, counter).map(|slots| Shot::Fire(slots, Box::new(ShotSteps::hip(hip))))
    }

    /// Sends the steps of the shot that plays due by own state time `state_ms`, started in the
    /// newest record: first a held aimed shot whose time came ([`SlayerTrack::aimed_fire`]),
    /// whose steps take the place of the shot before's. An aimed shot's last steps leave the
    /// aim's second-slot sequence for its hold ([`SlayerTrack::alt_hold_step`]); in the aim,
    /// a step's third-slot sequence of the aim's legs goes out as that of the legs it is on
    /// ([`AimThird`]). The weapon emptied by the shot turns its steps to those of an emptied
    /// weapon ([`ShotSteps::due`]). A held shot and a hip re-fire that go out are shots of their
    /// own ([`SlayerTrack::take_fired`]).
    fn step_shot(&mut self, state_ms: u64) {
        let counter = self.newest_counter;
        let aim_up_ms = self
            .aim_from_ms
            .filter(|_| self.alt_held && !self.aim_start_due)
            .map(|from_ms| state_ms.saturating_sub(from_ms));
        if let Some(held) = self.held_shot.take_if(|held| {
            state_ms >= held.at_ms
                && held
                    .aim_ready_ms
                    .is_none_or(|ready_ms| aim_up_ms.is_some_and(|up_ms| up_ms >= ready_ms))
        }) {
            if let Some(slots) = slots_of(held.change, counter) {
                self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
                self.fired.push(held.steps.fired(None));
            }
            self.shot_steps = Some(held.steps);
            self.alt_resume = None;
        }
        let Some(steps) = self.shot_steps.as_mut() else {
            return;
        };
        let (due, refired) = steps.due(state_ms, self.was_empty);
        if refired {
            self.fired.push(steps.fired(None));
        }
        if steps.kind == ShotKind::Aimed {
            self.aimed_from_ms = steps.from_ms;
        }
        if steps.done() {
            if steps.kind == ShotKind::Aimed {
                self.alt_resume = steps.last_second;
            }
            self.shot_steps = None;
        }
        let walking = self.locomotion.moving();
        let mut third = self.alt_play.as_mut().and_then(|play| play.third.as_mut());
        for change in due {
            let change = SlotsChange {
                third_sequence: change.third_sequence.map(|sequence| {
                    third
                        .as_mut()
                        .map_or(sequence, |third| third.sending(sequence, walking))
                }),
                ..change
            };
            if let Some(slots) = slots_of(change, counter) {
                self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
            }
        }
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
        self.drop_shot();
        if let Some(first) = play.first {
            self.play_ability_first(first, started);
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
            third: plays_third(&play.slots),
            cast: false,
        });
        self.ability_steps = Some(AbilitySteps::new(ability, &play.after, None));
    }

    /// The first slot plays `first`, the full-body state of a weapon ability, from record
    /// `started` ([`Pending::Ability`]).
    fn play_ability_first(&mut self, first: FirstPlay, started: u16) {
        self.pending = Some((Pending::Ability(first), started, self.newest_frame_s));
        self.sequence_s = self.newest_frame_s;
        self.follow_up = None;
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

    /// Weapon ability `ability` fired (its first spell or its shot): an aimed one plays its cast
    /// `play.cast`, started in the newest record (retail Sacred Ground: `16=33 18=993 19=00 21=00
    /// 48=03` at the click; Inferred: the server sends it at the cast, about 300 ms after the
    /// click). A full-body cast takes the first slot (Rain of Arrows' shot), and the ability then
    /// ends as a full-body one does; the cast's own steps take the place of the start's, and the
    /// ability ends at the cast's retail end when it has one ([`AbilityCast`]). Live 2026-11-21:
    /// Rain of Arrows' shot sent Splinter Shot's `16=33 18=571 48=43` and no first slot, and the
    /// own client stayed in the aim.
    pub(super) fn ability_cast(&mut self, ability: u32, play: &AbilityPlay) {
        let Some(on) = self
            .ability
            .as_mut()
            .filter(|on| on.ability == ability && !on.cast)
        else {
            return;
        };
        on.cast = true;
        let Some(cast) = play.cast.as_ref() else {
            return;
        };
        let AbilityCast {
            first,
            slots,
            after,
            ends_after_ms,
        } = cast;
        on.first |= first.is_some();
        on.third |= plays_third(slots);
        let counter = self.newest_counter;
        if let Some(first) = *first {
            self.play_ability_first(first, counter);
        }
        if let Some(slots) = slots_of(*slots, counter) {
            self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
        }
        self.ability_steps = Some(AbilitySteps::new(ability, after, *ends_after_ms));
    }

    /// Sends the timed changes of the weapon ability that plays that are due by own state time
    /// `state_ms`, and ends it at its retail end ([`AbilitySteps`]).
    fn step_ability(&mut self, state_ms: u64) {
        let Some(steps) = self.ability_steps.as_mut() else {
            return;
        };
        let ability = steps.ability;
        let (due, ends) = steps.due(state_ms);
        let counter = self.newest_counter;
        for change in due {
            if let Some(slots) = slots_of(change, counter) {
                self.upper = Some(merge(self.upper.unwrap_or_default(), slots));
            }
        }
        if ends {
            self.ability_ended(ability);
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
        self.ability_steps = None;
        let counter = self.newest_counter;
        let mut end = UpperSlots {
            marker: Some(CLEARED_SLOT),
            ..UpperSlots::default()
        };
        if on.first {
            let (state, sequence) = self
                .locomotion
                .resend(self.set.unwrap_or(MoveSet::SHEATHED));
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
            time: None,
        }
    }

    /// The server accepted an interaction that plays `play` ([`INTERACTION_STATE`]): it starts
    /// in the next own state, and the player stands still under it.
    pub(super) fn interacted(&mut self, play: InteractPlay) {
        self.interaction_due = Some(play);
        self.interacting = Some(play);
        self.locomotion = Locomotion::Idle;
    }

    /// The player's interaction is over: a user interface's state ends in the stand that clears
    /// the second slot, a gather's in the stance's stand (Observed, 2026-11-19 study: the own
    /// state after 1,750 interface interactions was `13=1f 15=1081 16=2d 18=00`; after the
    /// gathers, the stance's stand, the great sword's `1136` in 142 of 290 `3810` gathers with it
    /// drawn). An interaction a move or another action already left ends with that.
    pub(super) fn interaction_over(&mut self) {
        self.interaction_due = None;
        let Some(play) = self.interacting.take() else {
            return;
        };
        if self.played_state != Some(play.state) {
            return;
        }
        if play.state == INTERACTION_STATE {
            self.stand_due = true;
        } else {
            self.rest_due = true;
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
            walking: self.locomotion.moving(),
        };
    }

    /// The client closed its inventory in record `started`: the first close plays the close
    /// sequence, the second clears the slot ([`loadout`]); a close with no open does nothing.
    pub(super) fn close_loadout(&mut self, started: u16) {
        self.loadout = match self.loadout {
            LoadoutPlay::Opening { .. } | LoadoutPlay::Looping { .. } => LoadoutPlay::Closing {
                started,
                left: ECHO_STATES,
                walking: self.locomotion.moving(),
            },
            LoadoutPlay::Closing { .. } => LoadoutPlay::Cleared {
                started,
                left: ECHO_STATES,
            },
            play @ (LoadoutPlay::Closed | LoadoutPlay::Cleared { .. }) => play,
        };
    }

    /// Moves the inventory slot on at `state_ms`: the loop once the open played
    /// ([`loadout::on`]), the walk's loop at a move and the open again at a stop, and the end of
    /// the clear.
    fn step_loadout(&mut self, state_ms: u64) {
        let walking = self.locomotion.moving();
        let looping = |started, walking| LoadoutPlay::Looping {
            started,
            left: ECHO_STATES,
            walking,
        };
        self.loadout = match self.loadout {
            LoadoutPlay::Opening {
                started,
                walking: false,
                ..
            }
            | LoadoutPlay::Looping {
                started,
                walking: false,
                ..
            } if walking => looping(started, true),
            LoadoutPlay::Opening {
                started,
                walking: true,
                ..
            }
            | LoadoutPlay::Looping {
                started,
                walking: true,
                ..
            } if !walking => LoadoutPlay::Opening {
                started,
                opened_ms: state_ms,
                left: ECHO_STATES,
                walking: false,
            },
            LoadoutPlay::Opening {
                started,
                opened_ms,
                walking,
                ..
            } if state_ms.saturating_sub(opened_ms) >= loadout::on(walking).3 => {
                looping(started, walking)
            }
            LoadoutPlay::Cleared { left: 0, .. } => LoadoutPlay::Closed,
            play => play,
        };
    }

    /// The inventory's second slot for this own state, while a change of it echoes.
    fn loadout_slots(&self) -> Option<UpperSlots> {
        let (state, started, sequence) = match self.loadout {
            LoadoutPlay::Closed
            | LoadoutPlay::Opening { left: 0, .. }
            | LoadoutPlay::Looping { left: 0, .. }
            | LoadoutPlay::Closing { left: 0, .. }
            | LoadoutPlay::Cleared { left: 0, .. } => return None,
            LoadoutPlay::Opening {
                started, walking, ..
            } => (Some(loadout::OPEN_STATE), started, loadout::on(walking).0),
            LoadoutPlay::Looping {
                started, walking, ..
            } => (Some(loadout::OPEN_STATE), started, loadout::on(walking).1),
            LoadoutPlay::Closing {
                started, walking, ..
            } => (None, started, loadout::on(walking).2),
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
        self.newest_state_ms = Some(state_ms);
        if std::mem::take(&mut self.aim_start_due) {
            self.aim_from_ms = Some(state_ms);
        }
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
        if self
            .second_end
            .and_then(|end| end.at_ms)
            .is_some_and(|at_ms| state_ms >= at_ms)
        {
            self.second_end = None;
            let end = UpperSlots {
                second: SlotChange {
                    state: Some(CAST_END_STATE),
                    started: Some(number),
                    sequence: Some(CLEARED_SLOT),
                },
                ..UpperSlots::default()
            };
            self.upper = Some(merge(self.upper.unwrap_or_default(), end));
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
        self.step_ability(state_ms);
        let (state, started) = self.next(number);
        self.step_draw(state_ms);
        self.step_shot(state_ms);
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
        let slots = slots.map(|slots| self.timed(slots, number));
        if let Some(second) = state
            .filter(|_| started.is_some())
            .and_then(|state| second_change(&state))
        {
            self.upper_seconds.after([Some(second), None, None], true);
        }
        let first = self.upper_first.is_some_and(|(_, first)| first == number);
        self.upper_seconds.after(
            slots.map_or([None; UPPER_SLOTS], |slots| {
                [Some(slots.second), Some(slots.third), Some(slots.fourth)]
            }),
            first,
        );
        if let Some(slots) = slots {
            self.slots_on = self.slots_on.after(slots);
            self.track_second_end(slots.second, state_ms);
        }
        self.remote = slots.map(|slots| RemoteUpper {
            slots,
            after_state: self.upper_after_state,
        });
        self.due = SlayerDue { state, time, slots };
    }

    /// Starts the clock of the due second-slot end when its draw or sheathe goes out in a state
    /// at `state_ms`; another change of the second slot drops it.
    fn track_second_end(&mut self, second: SlotChange, state_ms: u64) {
        let Some(end) = self.second_end else {
            return;
        };
        if second == SlotChange::default() {
            return;
        }
        let ours = second.state == Some(DRAW_STATE) && second.sequence == Some(end.sequence);
        self.second_end = ours.then(|| SecondEnd {
            at_ms: end.at_ms.or(Some(state_ms.saturating_add(end.after_ms))),
            ..end
        });
    }

    /// `slots` as own state `number` sends them, with the time since the state that sent the
    /// same change first: the echoes of a change count on from its start (retail: `7f`, `81`,
    /// `83` over a start and its echoes; [`UpperSlots::time`]).
    fn timed(&mut self, slots: UpperSlots, number: u16) -> UpperSlots {
        let first = match self.upper_first {
            Some((sent, first)) if sent == slots => first,
            _ => {
                self.upper_first = Some((slots, number));
                number
            }
        };
        slots.at_seconds(f32::from(number.wrapping_sub(first)) * self.newest_frame_s)
    }

    /// What this tick's own state carries, once.
    pub(super) fn take_due(&mut self) -> SlayerDue {
        std::mem::take(&mut self.due)
    }

    /// An attack is still this state's action: a pending start, a first-slot attack that runs
    /// ([`ATTACK_RUN_RECORDS`]), or the echo of one (`state` empty and the second slot started
    /// with it, retail's attack form). Live 2026-11-17: the draw of a sheathed spear came one
    /// record after its press start, and the stand that replaced the start hid the attack from
    /// the other players.
    fn keeps_attack(&self) -> bool {
        matches!(self.pending, Some((Pending::Attack(_), _, _)))
            || self.running.is_some_and(|(attack, started)| {
                attack.slot == AttackSlot::First
                    && self.newest_counter.wrapping_sub(started) < ATTACK_RUN_RECORDS
            })
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
            self.drop_shot();
            self.roll_s = None;
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
            self.roll_s = None;
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
        if let Some(play) = self.interaction_due.take() {
            self.sequence_s = self.newest_frame_s;
            let state = SlayerState {
                state: Some(play.state),
                started: Some(number),
                sequence: play.sequence,
                second: (play.state == INTERACTION_STATE).then_some(SecondSlayerState {
                    started: number,
                    playing: None,
                    stopped: true,
                }),
            };
            return (self.start(state, None), Some(self.newest_frame_s));
        }
        if std::mem::take(&mut self.rest_due) {
            self.sequence_s = self.newest_frame_s;
            let idle = self.set.unwrap_or(MoveSet::SHEATHED).idle;
            let state = SlayerState {
                state: Some(idle.0),
                started: Some(number),
                sequence: idle.1,
                second: None,
            };
            return (self.start(state, None), Some(self.newest_frame_s));
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
                Pending::Sequence(sequence) | Pending::Run(sequence) => {
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
        match self.ride {
            Ride::Riding {
                plays,
                legs: RideLegs::Idle,
                ..
            } => (rider::IDLE_STATE, plays.idle),
            Ride::Riding {
                plays,
                legs: RideLegs::Fast,
                ..
            } => (rider::RIDE_STATE, plays.fast),
            Ride::Riding { plays, .. } | Ride::MountingUp { plays, .. } => {
                (rider::RIDE_STATE, plays.ride)
            }
            Ride::Foot | Ride::Dismounting { .. } => self
                .locomotion
                .current(self.set.unwrap_or(MoveSet::SHEATHED)),
        }
    }

    /// Whether the players who see this one still get its floor bit set for its jump: the
    /// newest start is a jump, under [`JUMP_FLOOR_S`] from its start state (the sequence time
    /// counts the start record's frame too).
    pub(super) fn jump_holds_floor(&self) -> bool {
        self.played_state == Some(JUMP_STATE)
            && self.sequence_s - self.newest_frame_s < JUMP_FLOOR_S
    }

    /// Whether the players who see this one get its dodge bit (child 41 bit `0x02`) cleared: the
    /// newest start is a dodge, under [`DODGE_FLAG_S`] from its start state (the sequence time
    /// counts the start record's frame too).
    pub(super) fn dodge_clears_flag(&self) -> bool {
        self.played_state == Some(DODGE_STATE)
            && self.sequence_s - self.newest_frame_s < DODGE_FLAG_S
    }

    /// How its body holds itself now, for the players who see this one: on a mount from the
    /// mount-up to the dismount ([`rider`]), crouching in a gather that crouches (Observed,
    /// 2026-11-19 study: child 43's crouch bit `40` set 32 ms after the start of 2,664 of the
    /// 3,095 gathers by hand `13=09 15=3832`, 872 of 967 `3812` and 516 of 573 `3811`, none of
    /// the others; cleared in the stand after them), else the newest record's posture, which
    /// the change's sequence goes out with ([`posture`]).
    pub(super) fn remote_posture(&self) -> AlcPosture {
        if matches!(self.ride, Ride::MountingUp { .. } | Ride::Riding { .. }) {
            return AlcPosture::Riding;
        }
        let crouches = self
            .interacting
            .is_some_and(|play| play.crouch && self.played_state == Some(play.state));
        if crouches {
            return AlcPosture::Crouch;
        }
        posture::shown(self.posture)
    }

    /// The upper slots of this tick's own state, for the players who see this one: retail's
    /// remote states echoed them as the own ones (arena capture 2026-09-13, a remote aim
    /// `16=24 18=9837 19=2e 21=9606 48=28` in 3 to 4 states, then without them).
    pub(super) const fn remote_upper(&self) -> Option<RemoteUpper> {
        self.remote
    }

    /// Seconds into the sequence of each upper slot that plays, second slot first, for the
    /// players who see this one ([`UpperSeconds`]).
    pub(super) const fn remote_upper_seconds(&self) -> [Option<f32>; UPPER_SLOTS] {
        self.upper_seconds.seconds
    }

    /// The weapon accuracy of its movement, for the players who see this one ([`RUN_ACCURACY`]).
    pub(super) const fn remote_movement_accuracy(&self) -> f32 {
        self.accuracy
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
        let set = self.set.unwrap_or(MoveSet::SHEATHED);
        let accuracy = match shown.or(self.played_state) {
            Some(movement::START_STATE) => Some(RUN_ACCURACY),
            Some(movement::MOVE_STATE)
                if state.sequence == set.restart.1 || state.sequence == set.back_start.1 =>
            {
                Some(BACK_ACCURACY)
            }
            Some(movement::MOVE_STATE) => Some(WALK_ACCURACY),
            Some(movement::IDLE_STATE | JUMP_STATE) => Some(STILL_ACCURACY),
            _ => None,
        };
        self.accuracy = accuracy.unwrap_or(self.accuracy);
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

/// What the right-click's release sends for `stance`: its end (`alt_end`) for the slots past
/// the second, and, when its press started the second slot, that slot's end: state
/// [`CAST_END_STATE`], sequence cleared.
///
/// Observed (attack sequence study of 1,196 retail ledgers, 2026-11-17): the second slot ended
/// in state 45 in about nine of ten ends of every stance (own and other players' states), with
/// sequence 0 in four of five of those; an own end came a median 34 ms after the right-click's
/// release (193 ends). The table's second-slot ends were mined from the own states only, where
/// the client ends its own block itself: some name no second slot, some the press's sequence
/// again, and the others (the spear's 2958, the blunderbuss's 4241) are in no retail end.
/// Live 2026-11-17: the other players saw a rapier's block pose (`18=2563`) for the rest of
/// the session, over its attacks and dodges.
fn alt_end(stance: &StanceSequences) -> Option<SlotsChange> {
    if stance
        .alt_start
        .and_then(|start| start.second_sequence)
        .is_none()
    {
        return stance.alt_end;
    }
    Some(SlotsChange {
        second_state: Some(CAST_END_STATE),
        second_sequence: Some(CLEARED_SLOT),
        ..stance.alt_end.unwrap_or_default()
    })
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
        time: None,
    };
    (slots != UpperSlots::default()).then_some(slots)
}

/// Whether `slots` play a third slot: a state or a sequence in it that is not the cleared one
/// (Sacred Ground's aim `19=46 21=449`; Penetrating Shot's `21=2343` and Rain of Arrows' shot
/// `21=387`, whose retail end clears it: `19=0 21=0`).
fn plays_third(slots: &SlotsChange) -> bool {
    [slots.third_state, slots.third_sequence]
        .into_iter()
        .flatten()
        .any(|value| value != CLEARED_SLOT)
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
        time: None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use nw_content::player_attacks::stance_key;
    use nw_content::player_sequences::{
        AltHold, AltLegs, AltMoves, AttackHold, AttackStep, Directional, DrawnMoves, FollowUp,
        Landing, SlotsChange,
    };

    use super::*;

    /// The 30 Hz frame time.
    const FRAME_S: f32 = 1.0 / 30.0;
    /// Records at [`FRAME_S`] from a start to its run: 1,366 ms.
    const RUN_RECORDS: u16 = 41;
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
            attack: AttackKeyBits::default(),
            weapon: None,
            swimming: false,
            falling: false,
            airborne: false,
            posture: Posture::Stand,
            dodging: false,
            load: DodgeLoad::Fast,
            ride: None,
        }
    }

    fn keyed(counter: u16, intent: Intent, attack: AttackKeyBits, tags: &str) -> SlayerRecord {
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

    const PRESS: AttackKeyBits = AttackKeyBits {
        pressed: true,
        held: false,
        released: false,
    };
    const RELEASE: AttackKeyBits = AttackKeyBits {
        pressed: false,
        held: false,
        released: true,
    };
    /// A record with no attack key bit: the key stays as it was.
    const HELD: AttackKeyBits = AttackKeyBits {
        pressed: false,
        held: false,
        released: false,
    };
    /// A controller's right trigger held: the held bit and no press.
    const PAD_HOLD: AttackKeyBits = AttackKeyBits {
        pressed: false,
        held: true,
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
    fn a_draw_right_after_the_press_start_does_not_replace_it() {
        // Live 2026-11-17: a sheathed spear's press start went out, and the stand sent with the
        // draw one record later hid it from the other players.
        const DRAW_STATE: u64 = 44;
        const DRAW_SEQUENCE: u64 = 2011;
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
        track.record(
            SlayerRecord {
                weapon: weapon(GREATSWORD, false),
                ..keyed(0x6ba, still, PRESS, GREATSWORD)
            },
            &table,
        );
        let press = own_state(&mut track, 0x6ba).state.expect("the press start");
        assert_eq!(press.sequence, PRESS_START);
        track.record(
            SlayerRecord {
                weapon: weapon(GREATSWORD, true),
                ..keyed(0x6bb, still, HELD, GREATSWORD)
            },
            &table,
        );
        let after = [own_state(&mut track, 0x6bb), own_state(&mut track, 0x6bc)];
        assert!(
            after.iter().any(
                |due| due.slots.map(|slots| slots.second.sequence) == Some(Some(DRAW_SEQUENCE))
            ),
            "the draw goes out"
        );
        for due in after {
            if let Some(state) = due.state {
                assert_eq!(
                    state.sequence, PRESS_START,
                    "no stand or walk replaces the press start: {state:?}"
                );
            }
        }
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

    /// Live 2026-11-17: other players saw none of a controller player's server attacks; the
    /// trigger sends the held bit and no press.
    #[test]
    fn a_controller_trigger_held_without_a_press_starts_the_press_start_and_the_charge() {
        let table = table();
        let mut track = spawned();
        let still = Intent::default();
        track.record(keyed(100, still, PAD_HOLD, GREATSWORD), &table);
        let press = own_state(&mut track, 100).state.expect("the press start");
        assert_eq!(press.sequence, PRESS_START);
        while own_state(&mut track, 100).state.is_some() {}
        for counter in 101..=100 + HOLD_RECORDS {
            track.record(keyed(counter, still, PAD_HOLD, GREATSWORD), &table);
        }
        let charge = own_state(&mut track, 100 + HOLD_RECORDS)
            .state
            .expect("the charge");
        assert_eq!(charge.sequence, CHARGE);
    }

    /// Live 2026-11-17: a controller player's block button set the attack press bit in its block
    /// start; read as a press, it held the attack key down, and the trigger's next hold was no
    /// new press.
    #[test]
    fn a_press_bit_in_the_block_start_leaves_the_attack_key_up() {
        let table = table();
        let mut track = spawned();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let still = Intent::default();
        let press_starts = |track: &mut SlayerTrack, counter| {
            std::iter::from_fn(|| own_state(track, counter).state)
                .take(usize::from(ECHO_STATES) + 2)
                .any(|due| due.sequence == PRESS_START)
        };
        track.record(keyed(100, block, PRESS, GREATSWORD), &table);
        assert!(!press_starts(&mut track, 100), "the block button's own bit");
        track.record(keyed(101, still, HELD, GREATSWORD), &table);
        while own_state(&mut track, 101).state.is_some() {}
        track.record(keyed(102, still, PAD_HOLD, GREATSWORD), &table);
        assert!(
            press_starts(&mut track, 102),
            "the trigger's next hold attacks"
        );
    }

    /// Live 2026-11-17: a spear player stopped walking and held the heavy attack; the stop's
    /// end started the stand two records after the charge, and the others saw no heavy attack.
    #[test]
    fn the_end_of_a_stop_does_not_replace_the_attack_that_runs() {
        const AFTER_RECORDS: u16 = 5;
        let table = table();
        let mut track = spawned();
        let forward = Intent::of_keys(true, false, false, false);
        let still = Intent::default();
        let mut counter = 100;
        let mut states = Vec::new();
        let mut step = |track: &mut SlayerTrack, intent, attack| {
            counter += 1;
            track.record(keyed(counter, intent, attack, GREATSWORD), &table);
            states.extend(std::iter::from_fn(|| own_state(track, counter).state));
        };
        step(&mut track, forward, HELD);
        step(&mut track, still, HELD);
        step(&mut track, still, PRESS);
        for _ in 0..movement::STOP_RECORDS + HOLD_RECORDS + AFTER_RECORDS {
            step(&mut track, still, HELD);
        }
        let charged_at = states
            .iter()
            .find(|state| state.sequence == CHARGE)
            .and_then(|state| state.started)
            .expect("the charge");
        let stood_at = states
            .iter()
            .find(|state| state.state == Some(movement::IDLE_STATE))
            .and_then(|state| state.started);
        assert!(
            stood_at.is_none_or(|at| at.wrapping_sub(charged_at) >= ATTACK_RUN_RECORDS),
            "no stand in the attack: {states:?}"
        );
    }

    /// Records of `intent` from counter `from` on, fed one by one with own states: the slayer
    /// states that went out (echoes left out).
    fn moved(
        track: &mut SlayerTrack,
        table: &PlayerSequences,
        from: u16,
        intents: &[Intent],
    ) -> Vec<SlayerState> {
        let mut states: Vec<SlayerState> = Vec::new();
        for (counter, intent) in (from..).zip(intents) {
            track.record(quiet_record(counter, *intent), table);
            if let Some(state) = own_state(track, counter).state
                && states.last() != Some(&state)
            {
                states.push(state);
            }
        }
        states
    }

    /// Retail (`retail-2026-11-19-towns-duels`): a back key during a stop sent the back move's
    /// sequence alone in the stop's state (`15=2747` 134 ms after the stop `27/1386`) and held
    /// it while the body backed up; of 179 runs and 892 starts while other players moved back,
    /// 3 and 25. Live 2026-11-20 (keys): ours sent the forward start `13=26` and 1.37 s later
    /// its run, and the legs ran forward while the body backed up. A controller's stick pulled
    /// back runs toward the stick instead (no back move), and its block walk never stops.
    #[test]
    fn a_back_key_during_a_stop_backs_up_in_the_stop_s_state_and_never_runs() {
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let back = Intent::of_keys(false, true, false, false);
        let mut track = spawned();
        let mut intents = vec![forward; 5];
        intents.push(Intent::default());
        let before = moved(&mut track, &table, 100, &intents);
        let stop = before.last().expect("the stop");
        assert_eq!(
            (stop.state, stop.sequence),
            (Some(movement::MOVE_STATE), movement::SHORT_STOP_SEQUENCE)
        );
        let backing = moved(
            &mut track,
            &table,
            106,
            &vec![back; usize::from(RUN_RECORDS) * 2],
        );
        let first = backing.first().expect("the back move");
        assert_eq!(
            (first.state, first.started, first.sequence),
            (None, None, movement::BACK_START_SEQUENCE),
            "the back move's sequence alone in the stop's state"
        );
        assert!(
            backing
                .iter()
                .all(|state| state.state != Some(movement::START_STATE)
                    && state.sequence != movement::RUN_SEQUENCE),
            "no forward start and no run: {backing:?}"
        );
    }

    /// A back key once a hit reaction lets the player go (its stop does not play) starts the
    /// back move in its state (Observed: retail went from a dodge `7/4487` into the back move
    /// `27/1246`).
    #[test]
    fn a_back_key_after_a_hit_reaction_starts_the_back_move() {
        const STARTED: u16 = 100;
        const FREE_RECORDS: u16 = 10;
        let back = Intent::of_keys(false, true, false, false);
        let mut track = spawned();
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
            track.record(quiet_record(counter, back), &table());
            let due = own_state(&mut track, counter + 1);
            if moved.is_none()
                && let Some(state) = due.state.filter(|state| state.state != Some(pull::STATE))
            {
                moved = Some((state.state, state.sequence));
            }
        }
        assert_eq!(
            moved,
            Some((Some(movement::MOVE_STATE), movement::BACK_START_SEQUENCE))
        );
    }

    /// Retail (`retail-2026-11-19-towns-duels`): no stop came after 536 other players' attack
    /// starts; their next movement was a stand a median 866 ms after it. Live 2026-11-20: a stop
    /// 64 ms after a press start replaced it, and the others saw the body slide 1.2 m with no
    /// legs.
    #[test]
    fn keys_let_go_in_an_attack_send_no_stop_and_the_stand_follows_the_attack() {
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let still = Intent::default();
        let mut track = spawned();
        let mut states = Vec::new();
        let mut feed = |track: &mut SlayerTrack, counter: u16, intent, attack| {
            track.record(keyed(counter, intent, attack, GREATSWORD), &table);
            if let Some(state) = own_state(track, counter).state {
                states.push(state);
            }
        };
        for counter in 100..110 {
            feed(&mut track, counter, forward, HELD);
        }
        feed(&mut track, 110, forward, PRESS);
        feed(&mut track, 111, still, RELEASE);
        let last = 111 + 2 * ATTACK_RUN_RECORDS;
        for counter in 112..last {
            feed(&mut track, counter, still, HELD);
        }
        let light_at = states
            .iter()
            .find(|state| state.sequence == LIGHT)
            .and_then(|state| state.started)
            .expect("the light attack");
        assert!(
            !states.iter().any(|state| {
                state.sequence == movement::STOP_SEQUENCE
                    || state.sequence == movement::SHORT_STOP_SEQUENCE
            }),
            "no stop: {states:?}"
        );
        let stood = states
            .iter()
            .find(|state| state.state == Some(movement::IDLE_STATE))
            .and_then(|state| state.started)
            .expect("the stand after the attack");
        assert_eq!(stood.wrapping_sub(light_at), ATTACK_RUN_RECORDS);
    }

    /// Live 2026-11-20: a controller player's client named the stand `31/1249` in the first
    /// record of a move; the stand played 848 ms while the body moved, and the others saw it
    /// glide with no legs. Retail: 3 of 4 block stands that started while moving went on in the
    /// block walk 15 to 234 ms later. Keys and a stick move by the same intent.
    #[test]
    fn a_stand_the_client_names_while_moving_goes_on_in_the_move() {
        let table = table();
        let stick = Intent {
            stick: Some([0.0, 1.0]),
            ..Intent::of_keys(true, false, false, false)
        };
        for (device, moving) in [
            ("keyboard", Intent::of_keys(true, false, false, false)),
            ("controller", stick),
        ] {
            let mut track = spawned();
            let named = SlayerRecord {
                action: Some(SlayerInput {
                    sequence: movement::IDLE_SEQUENCE,
                    state: u16::try_from(movement::IDLE_STATE).expect("a state"),
                    second_state: u16::MAX,
                    second_sequence: 0,
                }),
                ..quiet_record(100, moving)
            };
            track.record(named, &table);
            let stand = own_state(&mut track, 100)
                .state
                .expect("the client's stand");
            assert_eq!(stand.sequence, movement::IDLE_SEQUENCE, "{device}");
            let states = moved(&mut track, &table, 101, &[moving; 3]);
            let next = states
                .iter()
                .find(|state| state.state.is_some() && state != &&stand)
                .expect("a move");
            assert!(
                matches!(
                    next.state,
                    Some(movement::START_STATE | movement::MOVE_STATE)
                ),
                "{device}: {next:?}"
            );
            // After the stand's echo: 66 ms, inside retail's 15 to 234 ms.
            assert_eq!(next.started, Some(102), "{device}: after the stand's echo");
        }
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

    /// Retail sequences of the 2026-11-17 study: no weapon (dodge 4845, jump 1208, fall 1675,
    /// landings 789 / 2376 / 1355) and the great sword drawn (dodge 4834, jump 1561, landing 2488,
    /// draw 2467 ended after 1,096 ms).
    mod air {
        pub(super) const SHEATHED_DODGE: u64 = 4845;
        pub(super) const SHEATHED_JUMP: u64 = 1208;
        pub(super) const SHEATHED_FALL: u64 = 1675;
        pub(super) const LAND_MOVING_AFTER_JUMP: u64 = 789;
        pub(super) const LAND_STILL_AFTER_JUMP: u64 = 2376;
        pub(super) const LAND_AFTER_FALL: u64 = 1355;
        pub(super) const DRAWN_DODGE: u64 = 4834;
        pub(super) const DRAWN_JUMP: u64 = 1561;
        pub(super) const DRAWN_LAND: u64 = 2488;
        pub(super) const DRAW: u64 = 2467;
        pub(super) const DRAW_END_MS: u64 = 1096;
    }

    fn air_table() -> PlayerSequences {
        let play = |state: u64, sequence: u64| StatePlay { state, sequence };
        let all = |play: StatePlay| Directional {
            forward: Some(play),
            none: Some(play),
            ..Directional::default()
        };
        PlayerSequences::of_parts(
            vec![
                StanceSequences {
                    tags: String::new(),
                    dodge: Some(all(play(7, air::SHEATHED_DODGE))),
                    jump: Some(all(play(14, air::SHEATHED_JUMP))),
                    air: Some(AirSequences {
                        fall: Some(air::SHEATHED_FALL),
                        land_after_jump: Landing {
                            still: Some(air::LAND_STILL_AFTER_JUMP),
                            moving: Some(air::LAND_MOVING_AFTER_JUMP),
                        },
                        land_after_fall: Landing {
                            still: Some(air::LAND_AFTER_FALL),
                            moving: None,
                        },
                    }),
                    ..StanceSequences::default()
                },
                StanceSequences {
                    dodge: Some(all(play(7, air::DRAWN_DODGE))),
                    jump: Some(all(play(14, air::DRAWN_JUMP))),
                    air: Some(AirSequences {
                        land_after_jump: Landing {
                            still: Some(air::DRAWN_LAND),
                            moving: None,
                        },
                        ..AirSequences::default()
                    }),
                    draw: Some(play(DRAW_STATE, air::DRAW)),
                    draw_end_ms: Some(air::DRAW_END_MS),
                    ..greatsword_attacks()
                },
            ],
            vec![],
        )
    }

    /// The first slot's state and sequence of the own states `from..=to`, of those that start one.
    fn starts(track: &mut SlayerTrack, from: u16, to: u16) -> Vec<(Option<u64>, u64)> {
        (from..=to)
            .filter_map(|number| own_state(track, number).state)
            .map(|state| (state.state, state.sequence))
            .collect()
    }

    #[test]
    fn a_jump_lands_with_the_stance_s_landing_then_its_move() {
        // Retail: `13=0e` at the jump, `13=0c` at the landing, a move or stand 132 ms later.
        let table = air_table();
        let still = Intent::default();
        let jump = Intent {
            jump: true,
            ..still
        };
        let sheathed = |counter, intent, airborne| SlayerRecord {
            weapon: weapon(GREATSWORD, false),
            airborne,
            ..quiet_record(counter, intent)
        };
        let mut track = spawned();
        track.record(sheathed(10, jump, false), &table);
        let jumped = own_state(&mut track, 10).state.expect("the jump");
        assert_eq!(
            (jumped.state, jumped.sequence),
            (Some(14), air::SHEATHED_JUMP),
            "a sheathed weapon jumps as no weapon"
        );
        for counter in 11..20 {
            track.record(sheathed(counter, still, true), &table);
            own_state(&mut track, counter);
        }
        track.record(sheathed(20, still, false), &table);
        let landed = own_state(&mut track, 20).state.expect("the landing");
        assert_eq!(
            (landed.state, landed.sequence),
            (Some(LAND_STATE), air::LAND_STILL_AFTER_JUMP)
        );
        let mut after = Vec::new();
        for counter in 21..=20 + LAND_MOVE_RECORDS {
            track.record(sheathed(counter, still, false), &table);
            after.extend(starts(&mut track, counter, counter));
        }
        assert_eq!(
            after.last(),
            Some(&(Some(MoveSet::SHEATHED.idle.0), MoveSet::SHEATHED.idle.1)),
            "the stand follows the landing {LAND_MOVE_RECORDS} records later: {after:?}"
        );
    }

    #[test]
    fn a_jump_keeps_its_floor_bit_and_its_air_until_the_landing() {
        // Retail `retail-2026-11-19-towns-duels`: the floor bit cleared 166 ms (median) after
        // the jump's start, and 148 of 170 jumps went on to the landing. Live 2026-11-19c: a
        // stop that ended 52 ms into a jump sent the stand.
        const FLOOR_STATES: u16 = 4;
        let table = air_table();
        let forward = Intent::of_keys(true, false, false, false);
        let still = Intent::default();
        let sheathed = |counter, intent, airborne| SlayerRecord {
            weapon: weapon(GREATSWORD, false),
            airborne,
            ..quiet_record(counter, intent)
        };
        let mut track = spawned();
        let mut counter = 10;
        track.record(sheathed(counter, forward, false), &table);
        own_state(&mut track, counter);
        counter += 1;
        track.record(sheathed(counter, still, false), &table);
        let stop = own_state(&mut track, counter).state.expect("the stop");
        assert_eq!(stop.state, Some(movement::MOVE_STATE));
        counter += 1;
        let jump = Intent {
            jump: true,
            ..still
        };
        track.record(sheathed(counter, jump, false), &table);
        let jumped = own_state(&mut track, counter).state.expect("the jump");
        assert_eq!(jumped.state, Some(JUMP_STATE));
        assert!(track.jump_holds_floor(), "the start");
        for held in 1..=movement::STOP_RECORDS {
            counter += 1;
            track.record(sheathed(counter, still, true), &table);
            let state = own_state(&mut track, counter).state;
            assert!(
                state.is_none_or(|state| state.state == Some(JUMP_STATE)),
                "record {held} in the air: {state:?}"
            );
            assert_eq!(
                track.jump_holds_floor(),
                held <= FLOOR_STATES,
                "the floor bit at {held} records"
            );
        }
        counter += 1;
        track.record(sheathed(counter, still, false), &table);
        let landed = own_state(&mut track, counter).state.expect("the landing");
        assert_eq!(landed.state, Some(LAND_STATE));
        assert!(!track.jump_holds_floor());
    }

    /// Live 2026-11-22 01:47:53 to 01:48:22 UTC: a controller player's jumps with a drawn musket
    /// sent no state 14 (the table had no musket jump), only the landing, and the other player
    /// saw the body rise with straight legs; its rapier jumps showed. Retail (re-mine
    /// 2026-11-17, other players): the musket jumps with 14/2022 (213 of 247 jumps).
    #[test]
    fn a_drawn_musket_jumps_with_its_retail_jump_with_keyboard_and_controller() {
        const MUSKET: &str = "2H_Ranged,Rifle";
        const MUSKET_JUMP: u64 = 2022;
        const JUMP_RECORD: u16 = 300;
        /// The left stick pushed forward: right at rest, forward 1.
        const STICK_FORWARD: [f32; 2] = [0.0, 1.0];
        let table = crate::combat::test_support::test_player_sequences();
        let keys = Intent::of_keys(true, false, false, false);
        let pad = Intent::default().with_stick(Some(STICK_FORWARD));
        for (device, moving) in [
            ("keyboard still", Intent::default()),
            ("keyboard", keys),
            ("controller", pad),
        ] {
            let jump = Intent {
                jump: true,
                ..moving
            };
            let mut track = spawned();
            track.record(
                keyed(JUMP_RECORD - 1, moving, AttackKeyBits::default(), MUSKET),
                &table,
            );
            own_state(&mut track, JUMP_RECORD - 1);
            track.record(
                keyed(JUMP_RECORD, jump, AttackKeyBits::default(), MUSKET),
                &table,
            );
            let jumped = own_state(&mut track, JUMP_RECORD).state.expect("the jump");
            assert_eq!(
                (jumped.state, jumped.sequence),
                (Some(JUMP_STATE), MUSKET_JUMP),
                "{device}"
            );
        }
    }

    /// Live 2026-11-18: a rapier's block stood on the ice gauntlet's stand (31/3987, never sent
    /// with a rapier in 1,195 retail ledgers; others stood on 31/1249 in 1,185 cases) and a
    /// spear's standing block played the walking block's start.
    #[test]
    fn the_mined_table_blocks_the_rapier_and_the_spear_as_retail() {
        const RAPIER: &str = "1H_Melee,Rapier";
        const SPEAR: &str = "2H_Melee,Spear,Throwable";
        const RAPIER_STAND: StatePlay = StatePlay {
            state: 31,
            sequence: 1249,
        };
        let table = crate::combat::test_support::test_player_sequences();
        let stance = |tags: &str| table.stance(stance_key(tags)).expect("stance");
        let rapier = stance(RAPIER);
        assert_eq!(
            rapier.alt_moves.and_then(|moves| moves.idle),
            Some(RAPIER_STAND)
        );
        let rapier_hold = rapier.alt_hold.expect("the rapier's block hold");
        assert_eq!(
            (rapier_hold.stand.start, rapier_hold.stand.hold),
            (4099, 1049)
        );
        let spear_hold = stance(SPEAR).alt_hold.expect("the spear's block hold");
        assert_eq!(spear_hold.walk, WALK_BLOCK);
        assert_eq!(spear_hold.stand, STAND_BLOCK);
    }

    /// Retail (1,195 ledgers, 2026-11-20): a rapier's heavy attack 584 started 19 records after
    /// its press start 5057 in 6 of 6 own holds and 90 of 107 holds of other players (the other
    /// 17 at 13 records), and 19 records after its second step 766 in 4 of 4. The client's
    /// rapier charge is at 667 ms, one record more. The table said 27 and 22, so other players
    /// saw the heavy attack late.
    #[test]
    fn a_rapier_hold_starts_its_heavy_19_records_in_with_keyboard_and_controller() {
        const RAPIER: &str = "1H_Melee,Rapier";
        const RAPIER_PRESS: u64 = 5057;
        const RAPIER_SECOND_STEP: u64 = 766;
        const RAPIER_HEAVY: u64 = 584;
        const RAPIER_HOLD_RECORDS: u16 = 19;
        const PRESS_RECORD: u16 = 100;
        let table = crate::combat::test_support::test_player_sequences();
        let holds = &table.stance(stance_key(RAPIER)).expect("stance").holds;
        for after in [RAPIER_PRESS, RAPIER_SECOND_STEP] {
            let hold = holds
                .iter()
                .find(|hold| hold.after.sequence == after)
                .expect("the hold");
            assert_eq!(
                (hold.start.sequence, hold.records),
                (RAPIER_HEAVY, RAPIER_HOLD_RECORDS),
                "after {after}"
            );
        }
        let still = Intent::default();
        for (device, first, held) in [("keys", PRESS, HELD), ("pad", PAD_HOLD, PAD_HOLD)] {
            let mut track = spawned();
            track.record(keyed(PRESS_RECORD, still, first, RAPIER), &table);
            let press = own_state(&mut track, PRESS_RECORD)
                .state
                .expect("the press start");
            assert_eq!(press.sequence, RAPIER_PRESS, "{device}");
            while own_state(&mut track, PRESS_RECORD).state.is_some() {}
            let heavy_record = PRESS_RECORD + RAPIER_HOLD_RECORDS;
            for counter in PRESS_RECORD + 1..heavy_record {
                track.record(keyed(counter, still, held, RAPIER), &table);
                assert_eq!(
                    own_state(&mut track, counter).state,
                    None,
                    "{device} at {counter}"
                );
            }
            track.record(keyed(heavy_record, still, held, RAPIER), &table);
            let heavy = own_state(&mut track, heavy_record)
                .state
                .expect("the heavy attack");
            assert_eq!(
                (heavy.started, heavy.sequence),
                (Some(heavy_record), RAPIER_HEAVY),
                "{device}"
            );
        }
    }

    /// Live 2026-11-17: a medium-gear spear player's hop showed as the light roll to the others.
    #[test]
    fn a_dodge_plays_the_player_s_gear_load() {
        const SPEAR: &str = "2H_Melee,Spear,Throwable";
        /// The mined table's spear dodges forward: the light roll and the medium hop.
        const LIGHT_FORWARD: u64 = 4834;
        const MEDIUM_FORWARD: u64 = 3355;
        let table = crate::combat::test_support::test_player_sequences();
        let dodge = Intent {
            forward: 1,
            dodge: true,
            ..Intent::default()
        };
        let sequence = |load| {
            let mut track = spawned();
            track.record(
                SlayerRecord {
                    load,
                    ..keyed(100, dodge, HELD, SPEAR)
                },
                &table,
            );
            own_state(&mut track, 100).state.map(|state| state.sequence)
        };
        assert_eq!(sequence(DodgeLoad::Fast), Some(LIGHT_FORWARD));
        assert_eq!(sequence(DodgeLoad::Normal), Some(MEDIUM_FORWARD));
    }

    /// Live 2026-11-17: a controller player's jump presses in the air each started a new jump.
    #[test]
    fn a_jump_press_in_the_air_starts_no_jump() {
        const AIR_RECORDS: u16 = 12;
        const PRESS_EVERY: u16 = 3;
        let table = air_table();
        let still = Intent::default();
        let jump = Intent {
            jump: true,
            ..still
        };
        let sheathed = |counter, intent, airborne| SlayerRecord {
            weapon: weapon(GREATSWORD, false),
            airborne,
            ..quiet_record(counter, intent)
        };
        let mut track = spawned();
        let mut jump_starts = std::collections::BTreeSet::new();
        let mut step = |track: &mut SlayerTrack, record: SlayerRecord| {
            let counter = record.counter;
            track.record(record, &table);
            while let Some(state) = own_state(track, counter).state {
                if state.state == Some(14) {
                    jump_starts.insert(state.started);
                }
            }
        };
        step(&mut track, sheathed(10, jump, false));
        for counter in 11..11 + AIR_RECORDS {
            let intent = if counter % PRESS_EVERY == 0 {
                jump
            } else {
                still
            };
            step(&mut track, sheathed(counter, intent, true));
        }
        assert_eq!(jump_starts.len(), 1, "one jump: {jump_starts:?}");
    }

    /// Retail: a block walk stops into the block's stand at once (2,127 of 2,759 stops within
    /// 100 ms); ours held the walk 39 records, live 2026-11-17.
    #[test]
    fn a_block_walk_stops_into_the_block_stand_at_once() {
        let table = table();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let walking = Intent {
            forward: 1,
            ..block
        };
        let stance = table.stance(stance_key(GREATSWORD));
        let stand = MoveSet::of(stance, true, true, Posture::Stand).idle;
        let mut track = spawned();
        track.record(keyed(100, walking, HELD, GREATSWORD), &table);
        while own_state(&mut track, 100).state.is_some() {}
        track.record(keyed(101, walking, HELD, GREATSWORD), &table);
        while own_state(&mut track, 101).state.is_some() {}
        track.record(keyed(102, block, HELD, GREATSWORD), &table);
        let stopped = own_state(&mut track, 102).state.expect("the stop");
        assert_eq!((stopped.state, stopped.sequence), (Some(stand.0), stand.1));
    }

    #[test]
    fn a_fall_plays_the_stance_s_fall_and_its_own_landing() {
        let table = air_table();
        let still = Intent::default();
        let sheathed = |counter, airborne, falling| SlayerRecord {
            weapon: weapon(GREATSWORD, false),
            airborne,
            falling,
            ..quiet_record(counter, still)
        };
        let mut track = spawned();
        track.record(sheathed(10, true, true), &table);
        let fall = own_state(&mut track, 10).state.expect("the fall");
        assert_eq!(fall.sequence, air::SHEATHED_FALL);
        track.record(sheathed(11, false, false), &table);
        let landed = own_state(&mut track, 11).state.expect("the landing");
        assert_eq!(
            (landed.state, landed.sequence),
            (Some(LAND_STATE), air::LAND_AFTER_FALL)
        );
    }

    #[test]
    fn the_dodge_and_jump_play_the_drawn_weapon_s_and_else_no_weapon_s() {
        let table = air_table();
        let dodge = Intent {
            dodge: true,
            ..Intent::default()
        };
        let mut drawn = spawned();
        drawn.record(keyed(10, dodge, HELD, GREATSWORD), &table);
        assert_eq!(
            starts(&mut drawn, 10, 11).last().map(|start| start.1),
            Some(air::DRAWN_DODGE)
        );
        let mut sheathed = spawned();
        sheathed.record(
            SlayerRecord {
                weapon: weapon(GREATSWORD, false),
                ..quiet_record(10, dodge)
            },
            &table,
        );
        assert_eq!(
            starts(&mut sheathed, 10, 11).last().map(|start| start.1),
            Some(air::SHEATHED_DODGE)
        );
    }

    #[test]
    fn a_draw_s_second_slot_ends_after_the_stance_s_time() {
        // Retail: `16=2c 18=<draw>`, then `16=2d 18=0` the stance's time later.
        let table = air_table();
        let still = Intent::default();
        let mut track = spawned();
        track.record(
            SlayerRecord {
                weapon: weapon(GREATSWORD, false),
                ..quiet_record(10, still)
            },
            &table,
        );
        own_state(&mut track, 10);
        track.record(keyed(11, still, HELD, GREATSWORD), &table);
        let draw = own_state(&mut track, 11).slots.expect("the draw");
        assert_eq!(draw.second.sequence, Some(air::DRAW));
        let end_number = 11 + u16::try_from(air::DRAW_END_MS.div_ceil(TICK_MS)).expect("states");
        let mut ends = Vec::new();
        for number in 12..=end_number + 1 {
            track.record(keyed(number, still, HELD, GREATSWORD), &table);
            if let Some(slots) = own_state(&mut track, number).slots
                && slots.second.state == Some(CAST_END_STATE)
            {
                ends.push((number, slots.second.sequence));
            }
        }
        assert_eq!(
            ends.first(),
            Some(&(end_number, Some(CLEARED_SLOT))),
            "the end after {} ms",
            air::DRAW_END_MS
        );
    }

    #[test]
    fn an_upper_slot_change_and_its_echo_carry_the_time_since_its_start() {
        // Retail: `4=7f`, then `4=81` in the echo one state later.
        const STEPS_A_STATE: u8 = 2;
        let table = table();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let mut track = spawned();
        track.record(keyed(20, block, HELD, GREATSWORD), &table);
        let start = own_state(&mut track, 20).slots.expect("the block");
        let echo = own_state(&mut track, 21).slots.expect("its echo");
        assert_eq!(start.time, Some(nw_messages::SEQUENCE_TIME_REL_ZERO));
        assert_eq!(
            echo.time,
            Some(nw_messages::SEQUENCE_TIME_REL_ZERO + STEPS_A_STATE)
        );
        assert_eq!(
            track.remote_upper().map(|upper| upper.slots.time),
            Some(echo.time),
            "the other players get the same time"
        );
    }

    #[test]
    fn the_block_s_release_ends_its_second_slot_for_the_other_players() {
        // Live 2026-11-17: the other players saw the block pose for the rest of the session.
        let table = table();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let mut track = spawned();
        track.record(keyed(20, block, HELD, GREATSWORD), &table);
        while own_state(&mut track, 20).slots.is_some() {}
        track.record(keyed(30, Intent::default(), HELD, GREATSWORD), &table);
        let end = own_state(&mut track, 30).slots.expect("the block's end");
        assert_eq!(
            end.second,
            SlotChange {
                state: Some(CAST_END_STATE),
                started: Some(30),
                sequence: Some(CLEARED_SLOT),
            }
        );
    }

    #[test]
    fn a_block_end_clears_the_second_slot_and_keeps_the_table_s_other_slots() {
        const PRESSED: u64 = 2351;
        const TABLE_END: u64 = 2958;
        let stance = |end: Option<u64>| StanceSequences {
            alt_start: Some(SlotsChange {
                second_sequence: Some(PRESSED),
                ..SlotsChange::default()
            }),
            alt_end: Some(SlotsChange {
                second_sequence: end,
                fourth_state: Some(CLEARED_SLOT),
                ..SlotsChange::default()
            }),
            ..StanceSequences::default()
        };
        let ends = |end: Option<u64>| {
            alt_end(&stance(end)).map(|change| {
                (
                    change.second_state,
                    change.second_sequence,
                    change.fourth_state,
                )
            })
        };
        let cleared = Some((Some(CAST_END_STATE), Some(CLEARED_SLOT), Some(CLEARED_SLOT)));
        assert_eq!(ends(Some(PRESSED)), cleared);
        assert_eq!(ends(None), cleared);
        assert_eq!(ends(Some(TABLE_END)), cleared);
        let aim = StanceSequences {
            alt_end: Some(SlotsChange {
                third_state: Some(CLEARED_SLOT),
                ..SlotsChange::default()
            }),
            ..StanceSequences::default()
        };
        assert_eq!(alt_end(&aim), aim.alt_end, "no second slot to end");
    }

    /// The spear's block upper body by legs (block sequence study 2026-11-18).
    const WALK_BLOCK: AltLegs = AltLegs {
        start: 3044,
        hold: 4380,
        third: None,
    };
    const STAND_BLOCK: AltLegs = AltLegs {
        start: 4998,
        hold: 5016,
        third: None,
    };
    const BLOCK_HOLD_AFTER_MS: u64 = 83;
    /// The block's second-slot state (`16=37`).
    const BLOCK_STATE: u64 = 37;
    const BLOCK_STAND: StatePlay = StatePlay {
        state: 31,
        sequence: 636,
    };

    /// The great sword stance of [`table`] with the spear's block upper body by legs.
    fn alt_hold_table() -> PlayerSequences {
        PlayerSequences::of_parts(
            vec![StanceSequences {
                alt_start: Some(SlotsChange {
                    second_state: Some(BLOCK_STATE),
                    second_sequence: Some(BLOCK_SEQUENCE),
                    ..SlotsChange::default()
                }),
                alt_moves: Some(AltMoves {
                    moving: Some(BLOCK_WALK),
                    idle: Some(BLOCK_STAND),
                }),
                alt_hold: Some(AltHold {
                    walk: WALK_BLOCK,
                    stand: STAND_BLOCK,
                    hold_after_ms: BLOCK_HOLD_AFTER_MS,
                }),
                ..greatsword_attacks()
            }],
            Vec::new(),
        )
    }

    /// The second-slot sequences the own states carry while `track` reads `records` (counter and
    /// keys, the weapon of `tags` drawn), one own state after each record, without repeats.
    fn alt_sequences(
        track: &mut SlayerTrack,
        table: &PlayerSequences,
        tags: &str,
        records: &[(u16, Intent)],
    ) -> Vec<u64> {
        let mut seen: Vec<u64> = Vec::new();
        for (counter, intent) in records {
            track.record(keyed(*counter, *intent, HELD, tags), table);
            let sequence = own_state(track, *counter)
                .slots
                .and_then(|slots| slots.second.sequence);
            if let Some(sequence) = sequence.filter(|sequence| seen.last() != Some(sequence)) {
                seen.push(sequence);
            }
        }
        seen
    }

    /// Retail (1,195 ledgers): the spear's block standing starts with 4998 and holds 5016 from
    /// about 83 ms on; walking it starts with 3044 and holds 4380. Live 2026-11-18: ours sent
    /// the walk's start for every block, and the other player saw a standing block shake. A
    /// controller's stick reaches the track as forward keys ([`Intent::toward_stick`]).
    #[test]
    fn a_block_plays_its_start_then_its_hold_for_the_legs_it_stands_or_walks_on() {
        let table = alt_hold_table();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let mut standing = spawned();
        let records: Vec<(u16, Intent)> = (20..=24).map(|counter| (counter, block)).collect();
        assert_eq!(
            alt_sequences(&mut standing, &table, GREATSWORD, &records),
            [STAND_BLOCK.start, STAND_BLOCK.hold],
            "standing"
        );
        let walk = Intent {
            forward: 1,
            ..Intent::default()
        };
        let mut walking = spawned();
        let walk_block = Intent {
            block: true,
            ..walk
        };
        let records: Vec<(u16, Intent)> = std::iter::once((10, walk))
            .chain((11..=15).map(|counter| (counter, walk_block)))
            .collect();
        assert_eq!(
            alt_sequences(&mut walking, &table, GREATSWORD, &records),
            [WALK_BLOCK.start, WALK_BLOCK.hold],
            "walking"
        );
    }

    /// Retail: a leg change in a held block sends the new legs' hold (spear: 4380 to 5016 at
    /// a stop, 877 cases; 5016 to 4380 at a move, 1,076).
    #[test]
    fn a_held_block_takes_the_new_legs_hold_when_it_stops_or_walks() {
        let table = alt_hold_table();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let walk_block = Intent {
            forward: 1,
            ..block
        };
        let mut track = spawned();
        let records: Vec<(u16, Intent)> = (20..=30)
            .map(|counter| {
                let stopped = (25..=27).contains(&counter);
                (counter, if stopped { block } else { walk_block })
            })
            .collect();
        let seen = alt_sequences(&mut track, &table, GREATSWORD, &records);
        assert_eq!(
            seen.iter().rev().take(2).copied().collect::<Vec<_>>(),
            [WALK_BLOCK.hold, STAND_BLOCK.hold],
            "stand hold, then walk hold: {seen:?}"
        );
    }

    /// Retail (1,195 ledgers): the rifle's aim raises with 3544 and holds 4249 from about 215 ms
    /// on, on walking and standing legs alike. Live 2026-11-18: ours sent the raise alone, and
    /// the other player saw no aim. A stop in the aim sends nothing new.
    #[test]
    fn a_rifle_aim_raises_then_holds_and_a_stop_restarts_nothing() {
        const RIFLE: &str = "2H_Ranged,Rifle";
        const RAISE: u64 = 3544;
        const AIM_HOLD: u64 = 4249;
        let table = crate::combat::test_support::test_player_sequences();
        let aim_walk = Intent {
            block: true,
            forward: 1,
            ..Intent::default()
        };
        let aim_still = Intent {
            forward: 0,
            ..aim_walk
        };
        let mut track = spawned();
        let records: Vec<(u16, Intent)> = (40..=60)
            .map(|counter| (counter, if counter < 52 { aim_walk } else { aim_still }))
            .collect();
        assert_eq!(
            alt_sequences(&mut track, &table, RIFLE, &records),
            [RAISE, AIM_HOLD]
        );
    }

    /// Retail: a dodge during a held block carries the block's end (`16=45 18=00`) in its own
    /// state (986 of 986), and the block starts again with the move after it while the
    /// right-click stays held. Live 2026-11-18: ours kept the block over a back roll, and the
    /// other player saw no roll.
    #[test]
    fn a_dodge_ends_the_block_and_the_held_block_starts_again_with_the_next_move() {
        let table = table();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let dodge_back = Intent {
            dodge: true,
            forward: -1,
            ..block
        };
        let mut track = spawned();
        track.record(keyed(20, block, HELD, GREATSWORD), &table);
        let start = own_state(&mut track, 20).slots.expect("the block");
        assert_eq!(start.second.sequence, Some(BLOCK_SEQUENCE));
        own_state(&mut track, 20);
        track.record(keyed(21, dodge_back, HELD, GREATSWORD), &table);
        let due = own_state(&mut track, 21);
        assert_eq!(
            due.state.map(|state| (state.state, state.sequence)),
            Some((Some(DODGE_BACK.state), DODGE_BACK.sequence)),
            "the roll"
        );
        let end = due.slots.expect("the block's end with the roll");
        assert_eq!(end.second.state, Some(CAST_END_STATE));
        assert_eq!(end.second.sequence, Some(CLEARED_SLOT));
        own_state(&mut track, 21);
        let held_still = Intent {
            forward: -1,
            ..block
        };
        track.record(keyed(22, held_still, HELD, GREATSWORD), &table);
        assert_eq!(
            own_state(&mut track, 22).slots,
            None,
            "no block over the roll"
        );
        own_state(&mut track, 22);
        // The block starts again with the stand at the roll's end, 500 ms after its start
        // (retail: 0.45 to 0.63 s).
        let (at, again) = (23..60)
            .find_map(|counter| {
                track.record(keyed(counter, block, HELD, GREATSWORD), &table);
                own_state(&mut track, counter)
                    .slots
                    .filter(|slots| slots.second.sequence.is_some())
                    .map(|slots| (counter, slots))
            })
            .expect("the block again");
        assert_eq!(again.second.sequence, Some(BLOCK_SEQUENCE));
        assert_eq!(at - 21, 15, "at the roll's end");
    }

    #[test]
    fn a_block_let_go_during_its_dodge_does_not_start_again() {
        let table = table();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let mut track = spawned();
        track.record(keyed(20, block, HELD, GREATSWORD), &table);
        own_state(&mut track, 20);
        own_state(&mut track, 20);
        let dodge_back = Intent {
            dodge: true,
            forward: -1,
            ..block
        };
        track.record(keyed(21, dodge_back, HELD, GREATSWORD), &table);
        own_state(&mut track, 21);
        own_state(&mut track, 21);
        let back = Intent {
            forward: -1,
            ..Intent::default()
        };
        track.record(keyed(22, back, HELD, GREATSWORD), &table);
        own_state(&mut track, 22);
        own_state(&mut track, 22);
        track.record(keyed(23, Intent::default(), HELD, GREATSWORD), &table);
        let after = own_state(&mut track, 23).slots;
        assert!(
            after.is_none_or(|slots| slots.second.sequence != Some(BLOCK_SEQUENCE)),
            "no block: {after:?}"
        );
    }

    /// Live 2026-11-18: a block press with the weapon sheathed showed the other players nothing.
    /// The press draws the weapon (`SimWorld::attack_input`); the next record holds it drawn and
    /// starts the block.
    #[test]
    fn a_block_held_from_a_sheath_blocks_once_the_press_drew_the_weapon() {
        let table = table();
        let block = Intent {
            block: true,
            ..Intent::default()
        };
        let mut track = spawned();
        let sheathed = SlayerRecord {
            weapon: weapon(GREATSWORD, false),
            ..keyed(20, block, HELD, GREATSWORD)
        };
        track.record(sheathed, &table);
        assert_eq!(
            own_state(&mut track, 20).slots,
            None,
            "no block while sheathed"
        );
        track.record(keyed(21, block, HELD, GREATSWORD), &table);
        let slots = own_state(&mut track, 21).slots.expect("the block");
        assert_eq!(slots.second.sequence, Some(BLOCK_SEQUENCE));
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
    fn the_players_who_see_one_get_the_time_of_each_upper_slot_in_every_state() {
        // Retail `retail-2026-11-19-towns-duels`: child 4 in 151,132 of 151,536 remote states
        // while the second slot played, rising each state; ours sent it in the echoes alone.
        const HELD_STATES: u16 = 6;
        const THIRD: usize = 1;
        let table = table();
        let aim = Intent {
            block: true,
            ..Intent::default()
        };
        let mut track = spawned();
        assert_eq!(
            track.remote_upper_seconds(),
            [None; UPPER_SLOTS],
            "no slot plays"
        );
        let start: u16 = 0x54ad;
        track.record(keyed(start, aim, HELD, BLUNDERBUSS), &table);
        own_state(&mut track, start);
        assert_eq!(
            track.remote_upper_seconds()[THIRD],
            Some(0.0),
            "the aim starts"
        );
        for held in 1..=HELD_STATES {
            track.record(keyed(start + held, aim, HELD, BLUNDERBUSS), &table);
            own_state(&mut track, start + held);
            let seconds = track.remote_upper_seconds()[THIRD].expect("the aim plays");
            assert!(
                (seconds - f32::from(held) * FRAME_S).abs() < f32::EPSILON,
                "state {held}: {seconds} s, after the echo too"
            );
        }
        assert_eq!(track.remote_upper(), None, "the echo of the start is over");
        let end = start + HELD_STATES + 1;
        track.record(keyed(end, Intent::default(), HELD, BLUNDERBUSS), &table);
        own_state(&mut track, end);
        assert!(
            track.remote_upper_seconds()[THIRD].is_some(),
            "the end carries the time (retail 2,281 of 2,803 ends)"
        );
        own_state(&mut track, end);
        assert!(
            track.remote_upper_seconds()[THIRD].is_some(),
            "and its echo"
        );
        own_state(&mut track, end);
        assert_eq!(track.remote_upper_seconds()[THIRD], None, "then none");
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
        let gun = |counter: u16, attack: AttackKeyBits, empty: bool| SlayerRecord {
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
        // The echo, then nothing until the run, then nothing while running.
        assert_eq!(step(&mut track, forward), Some(start));
        assert_eq!(step(&mut track, forward), None);
        let run = (0..RUN_RECORDS)
            .find_map(|_| step(&mut track, forward))
            .expect("the run");
        assert_eq!((run.state, run.sequence), (None, movement::RUN_SEQUENCE));
        let stop = (0..ECHO_STATES)
            .find_map(|_| step(&mut track, Intent::default()).filter(|state| state != &run))
            .expect("a stop");
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
        // Retail, other players: a move in `13=1b` let go stands at once (`1b` 2422 to `1f`
        // 1081), and a key after a stop starts in `13=1a` (`1b` 1405 to `1a` 2794); never a
        // `13=1b` after a `13=1b` (0 of 802).
        let stood = step(&mut track, Intent::default()).expect("standing");
        assert_eq!(
            (stood.state, stood.sequence),
            (Some(movement::IDLE_STATE), movement::IDLE_SEQUENCE)
        );
        step(&mut track, forward).expect("a start");
        let stop = step(&mut track, Intent::default()).expect("a stop");
        assert_eq!(
            (stop.state, stop.sequence),
            (Some(movement::MOVE_STATE), movement::SHORT_STOP_SEQUENCE),
            "a start let go at once stops short"
        );
        let restart = step(&mut track, forward).expect("the restart");
        assert_eq!(
            (restart.state, restart.started, restart.sequence),
            (None, None, movement::RESTART_SEQUENCE),
            "a key during the stop restarts it alone"
        );
        let again = step(&mut track, forward).expect("moving again");
        assert_eq!(
            (again.state, again.sequence),
            (Some(movement::START_STATE), movement::START_SEQUENCE),
            "the record after the restart starts the move"
        );
        let turned = step(&mut track, Intent::of_keys(false, true, false, false));
        assert_eq!(
            turned.map(|state| (state.state, state.sequence)),
            Some((Some(movement::MOVE_STATE), movement::BACK_START_SEQUENCE)),
            "back while moving forward starts in the back move's state"
        );
        let ahead = step(&mut track, forward).expect("forward again");
        assert_eq!(
            ahead.state,
            Some(movement::START_STATE),
            "not 13=1b after 13=1b"
        );
    }

    /// Live 2026-11-19f: a rifle player's start `13=1a 15=1855` held 7.4 s while she ran, and
    /// the others saw its clip once, then still legs. Retail (1,206 ledgers): the rifle's start
    /// went on in `15=1584` alone 1,366 ms later at the median (498 times), its time at the
    /// start's again (`2=7f`, no new absolute part; `retail-2026-11-19-towns-duels`, a player at
    /// 1795121835309).
    #[test]
    fn a_rifle_start_runs_in_the_rifle_s_run_with_the_start_s_time() {
        const RIFLE: &str = "2H_Ranged,Rifle";
        const RIFLE_START: u64 = 1855;
        const RIFLE_RUN: u64 = 1584;
        let table = crate::combat::test_support::test_player_sequences();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut sent = Vec::new();
        for counter in 200..200 + RUN_RECORDS + 2 {
            track.record(keyed(counter, forward, HELD, RIFLE), &table);
            let due = own_state(&mut track, counter);
            if let Some(state) = due.state {
                sent.push((counter, state, due.time));
            }
        }
        let (at, start, start_time) = sent[0];
        assert_eq!((start.state, start.sequence), (Some(26), RIFLE_START));
        let (run_at, run, run_time) = sent
            .iter()
            .copied()
            .find(|(_, state, _)| state.sequence == RIFLE_RUN)
            .expect("the rifle's run");
        assert_eq!(run_at - at, RUN_RECORDS, "1,366 ms after the start");
        assert_eq!((run.state, run.started), (None, None), "the sequence alone");
        let zero = start_time.map(|time| time.rel);
        assert_eq!(
            run_time.map(|time| time.rel),
            zero,
            "the run's time starts again"
        );
        assert_eq!(
            run_time.and_then(|time| time.abs),
            None,
            "no new absolute part"
        );
    }

    /// Retail, 1,206 ledgers: a sheathed start let go within 270 ms stopped in `1370` and stood
    /// 535 ms later (4,441 times); a run let go stopped in `1405` and stood 1,315 ms later.
    #[test]
    fn a_fresh_start_stops_short_and_stands_sooner() {
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut counter = 300;
        let mut step = |track: &mut SlayerTrack, intent: Intent| {
            counter += 1;
            track.record(quiet_record(counter, intent), &table);
            own_state(track, counter).state
        };
        step(&mut track, forward).expect("a start");
        for _ in 0..6 {
            step(&mut track, forward);
        }
        let stop = step(&mut track, Intent::default()).expect("the short stop");
        assert_eq!(
            (stop.state, stop.sequence),
            (Some(movement::MOVE_STATE), movement::SHORT_STOP_SEQUENCE)
        );
        let idle = (0..movement::SHORT_STOP_RECORDS)
            .find_map(|_| {
                step(&mut track, Intent::default())
                    .filter(|state| state.state == Some(movement::IDLE_STATE))
            })
            .expect("standing still");
        assert_eq!(
            idle.started
                .zip(stop.started)
                .map(|(idle, stop)| idle.wrapping_sub(stop)),
            Some(movement::SHORT_STOP_RECORDS)
        );
        step(&mut track, forward).expect("a start");
        for _ in 0..10 {
            step(&mut track, forward);
        }
        let stop = step(&mut track, Intent::default()).expect("the stop");
        assert_eq!(
            stop.sequence,
            movement::STOP_SEQUENCE,
            "300 ms after the start"
        );
    }

    /// Retail, 1,206 ledgers: 14,398 of the 18,229 sheathed forward dodges `7/4845` went on in
    /// the run `13=1a 15=3050` 500 ms after their start; ours sent nothing at the dodge's end.
    #[test]
    fn a_dodge_with_the_key_held_goes_on_in_the_run_at_its_end() {
        const DODGE_RECORDS: u16 = 15;
        let table = crate::combat::test_support::test_player_sequences();
        let forward = Intent::of_keys(true, false, false, false);
        let dodge = Intent {
            dodge: true,
            ..forward
        };
        let mut track = spawned();
        track.record(quiet_record(400, forward), &table);
        own_state(&mut track, 400);
        track.record(quiet_record(401, dodge), &table);
        let roll = own_state(&mut track, 401).state.expect("the dodge");
        assert_eq!(roll.state, Some(DODGE_STATE));
        let next = (402..402 + DODGE_RECORDS)
            .find_map(|counter| {
                track.record(quiet_record(counter, forward), &table);
                own_state(&mut track, counter)
                    .state
                    .filter(|state| state.started != roll.started)
                    .map(|state| (counter, state))
            })
            .expect("the move after the dodge");
        assert_eq!(
            next.0 - 401,
            DODGE_RECORDS,
            "500 ms after the dodge's start"
        );
        assert_eq!(
            (next.1.state, next.1.sequence),
            (Some(movement::START_STATE), movement::RUN_SEQUENCE)
        );
    }

    /// Live 2026-11-20: a second dodge press in the body's dodge started the dodge again for
    /// the others (5 of 25, 164 to 200 ms after the start) while the body went on in the first.
    /// Only a press after the body's dodge starts one.
    #[test]
    fn a_dodge_press_in_the_body_s_dodge_starts_none_with_keyboard_and_controller() {
        const SECOND_PRESS: u16 = 406;
        const AFTER_THE_DODGE: u16 = 418;
        /// The left stick pushed forward: right at rest, forward 1.
        const STICK_FORWARD: [f32; 2] = [0.0, 1.0];
        let table = crate::combat::test_support::test_player_sequences();
        let keys = Intent::of_keys(true, false, false, false);
        let pad = Intent::default().with_stick(Some(STICK_FORWARD));
        for (device, forward) in [("keyboard", keys), ("controller", pad)] {
            let dodge = Intent {
                dodge: true,
                ..forward
            };
            let mut track = spawned();
            track.record(quiet_record(400, forward), &table);
            own_state(&mut track, 400);
            track.record(quiet_record(401, dodge), &table);
            let roll = own_state(&mut track, 401).state.expect("the dodge");
            assert_eq!(roll.state, Some(DODGE_STATE), "{device}");
            let mut starts = Vec::new();
            for counter in 402..=AFTER_THE_DODGE {
                let pressed = counter == SECOND_PRESS || counter == AFTER_THE_DODGE;
                let record = SlayerRecord {
                    dodging: counter < AFTER_THE_DODGE,
                    ..quiet_record(counter, if pressed { dodge } else { forward })
                };
                track.record(record, &table);
                if let Some(state) = own_state(&mut track, counter).state
                    && state.state == Some(DODGE_STATE)
                    && state.started != roll.started
                {
                    starts.push(counter);
                }
            }
            assert_eq!(
                starts,
                vec![AFTER_THE_DODGE],
                "{device}: no dodge at the press in the body's dodge, one after it"
            );
        }
    }

    /// Retail `retail-2026-11-19-towns-duels`: a dodge with the marker set carried `48=00`
    /// (165 of 166) and cleared child 41's bit `0x02` for 400 ms (251 dodges); ours sent
    /// neither.
    #[test]
    fn a_dodge_clears_the_marker_and_its_flag_for_its_first_400_ms() {
        const FLAG_RECORDS: u16 = 12;
        let table = table();
        let still = Intent::default();
        let dodge = Intent {
            dodge: true,
            ..Intent::of_keys(false, true, false, false)
        };
        let mut track = spawned();
        track.record(keyed(500, still, PRESS, GREATSWORD), &table);
        let press = own_state(&mut track, 500);
        assert_eq!(
            press.slots.and_then(|slots| slots.marker),
            Some(PRESS_MARKER)
        );
        assert!(!track.dodge_clears_flag());
        track.record(keyed(501, dodge, HELD, GREATSWORD), &table);
        let roll = own_state(&mut track, 501);
        assert_eq!(roll.state.and_then(|state| state.state), Some(DODGE_STATE));
        assert_eq!(
            roll.slots.and_then(|slots| slots.marker),
            Some(CLEARED_SLOT),
            "the dodge clears the marker"
        );
        assert!(track.dodge_clears_flag(), "the dodge's start");
        for counter in 502..501 + FLAG_RECORDS {
            track.record(keyed(counter, still, HELD, GREATSWORD), &table);
            own_state(&mut track, counter);
            assert!(track.dodge_clears_flag(), "record {counter}");
        }
        track.record(keyed(501 + FLAG_RECORDS, still, HELD, GREATSWORD), &table);
        own_state(&mut track, 501 + FLAG_RECORDS);
        assert!(!track.dodge_clears_flag(), "400 ms after the start");
        let mut track = spawned();
        track.record(keyed(600, dodge, HELD, GREATSWORD), &table);
        assert_eq!(
            own_state(&mut track, 600)
                .slots
                .and_then(|slots| slots.marker),
            None,
            "no marker to clear"
        );
    }

    /// Retail, 1,206 ledgers: every press start `13=10` carried the chain marker `48=1`, except
    /// after a chain attack `13=08` with the marker set, which carried `48=0` (16,445 other and
    /// 2,945 own); an `13=08` start set a clear marker. Ours sent none with the presses the
    /// client named (7 of 14, live 2026-11-19c and 19e).
    #[test]
    fn a_named_press_start_marks_its_chain_and_one_after_a_chain_attack_clears_it() {
        let named = |counter: u16, state: u16, sequence: u64| SlayerRecord {
            action: Some(SlayerInput {
                sequence,
                state,
                second_state: 0x002c,
                second_sequence: 0,
            }),
            ..quiet_record(counter, Intent::default())
        };
        let marker = |due: SlayerDue| due.slots.and_then(|slots| slots.marker);
        let table = table();
        let mut track = spawned();
        // Each start, then the states that echo it.
        let start = |track: &mut SlayerTrack, counter: u16, state: u16, sequence: u64| {
            track.record(named(counter, state, sequence), &table);
            let due = own_state(track, counter);
            for echo in 1..=u16::from(ECHO_STATES) {
                own_state(track, counter + echo);
            }
            due
        };
        assert_eq!(
            marker(start(&mut track, 700, 0x000a, 855)),
            Some(CHAIN_MARKER)
        );
        let attack = start(&mut track, 710, 0x0008, 4776);
        assert_eq!(attack.state.and_then(|state| state.started), Some(710));
        assert_eq!(marker(attack), None, "the chain holds its marker");
        assert_eq!(
            marker(start(&mut track, 720, 0x000a, 3931)),
            Some(CLEARED_SLOT),
            "a press start after a chain attack"
        );
        assert_eq!(
            marker(start(&mut track, 730, 0x0008, 4776)),
            Some(CHAIN_MARKER),
            "a chain attack sets a clear marker"
        );
    }

    /// Retail, 1,196 ledgers (`rider3.py`): a horse rider's moving mount-up `13=20 15=926` went
    /// on in `13=21 15=946` 534 ms later (511 of 517), a stop in the walk `957` alone and the
    /// stand `13=23 15=953` 369 ms later, a jump `13=18 15=884` landed in `13=17 15=893` and rode
    /// on 234 ms later, a moving dismount `13=15 15=881` ran on foot `13=1a 15=3050` 533 ms
    /// later; the rider's child 43 was `20` from the mount-up to the dismount. Ours sent none of
    /// these.
    #[test]
    fn a_rider_mounts_rides_stops_jumps_and_dismounts_with_its_mount_s_sequences() {
        let table = crate::combat::test_support::test_player_sequences();
        let horse = table.rider("Horse").expect("the horse");
        let forward = Intent::of_keys(true, false, false, false);
        let jump = Intent {
            jump: true,
            ..Intent::default()
        };
        let mut track = spawned();
        let mut counter = 800;
        let mut step = |track: &mut SlayerTrack, intent: Intent, ride: bool, airborne: bool| {
            counter += 1;
            let record = SlayerRecord {
                ride: ride.then_some(RideKeys {
                    plays: horse,
                    dashing: false,
                }),
                airborne,
                ..quiet_record(counter, intent)
            };
            track.record(record, &table);
            let state = own_state(track, counter).state;
            (counter, state.map(|state| (state.state, state.sequence)))
        };
        // Starts, and the first state after each: (records from the start before, state).
        let mut starts = |track: &mut SlayerTrack, records, intent, ride, airborne| {
            (0..records)
                .filter_map(|_| {
                    let (at, state) = step(track, intent, ride, airborne);
                    state.map(|state| (at, state))
                })
                .collect::<Vec<_>>()
        };
        let up = starts(&mut track, 20, forward, true, false);
        assert_eq!(
            up[0].1,
            (Some(rider::MOUNT_UP_STATE), horse.mount_up.moving)
        );
        let ride = up
            .iter()
            .find(|(_, state)| state.0 == Some(rider::RIDE_STATE));
        assert_eq!(
            ride.map(|(at, state)| (at - up[0].0, state.1)),
            Some((16, horse.after_mount_up)),
            "534 ms after the mount-up"
        );
        assert_eq!(track.remote_posture(), AlcPosture::Riding);
        let stop = starts(&mut track, 15, Intent::default(), true, false);
        assert_eq!(stop[0].1, (None, horse.slow), "the walk alone");
        assert_eq!(
            stop.iter()
                .find(|(_, state)| state.0 == Some(rider::IDLE_STATE))
                .map(|(at, state)| (at - stop[0].0, state.1)),
            Some((12, horse.idle)),
            "384 ms later, the stand"
        );
        let jumped = starts(&mut track, 1, jump, true, false);
        assert_eq!(jumped[0].1, (Some(rider::JUMP_STATE), horse.jump));
        starts(&mut track, 10, Intent::default(), true, true);
        let landed = starts(&mut track, 10, forward, true, false);
        assert_eq!(landed[0].1, (Some(rider::LAND_STATE), horse.land));
        assert_eq!(
            landed
                .iter()
                .find(|(_, state)| state.0 == Some(rider::RIDE_STATE))
                .map(|(at, state)| (at - landed[0].0, state.1)),
            Some((7, horse.ride)),
            "234 ms after the landing"
        );
        let off = starts(&mut track, 20, forward, false, false);
        assert_eq!(
            off[0].1,
            (Some(rider::DISMOUNT_STATE), horse.dismount.moving)
        );
        assert_eq!(track.remote_posture(), AlcPosture::Stand);
        assert_eq!(
            off.iter()
                .find(|(_, state)| state.0 == Some(movement::START_STATE))
                .map(|(at, state)| (at - off[0].0, state.1)),
            Some((16, movement::RUN_SEQUENCE)),
            "533 ms after the dismount, the run"
        );
    }

    /// The mined table's interactions (2026-11-19 study): the smelter `24/3490` (37 of 37), a
    /// mine `09/2161` (496 of 503), a gather by hand `09/3832` crouching, a talk `24/1081`;
    /// a mount type without a row rides as the horse (Inferred).
    #[test]
    fn the_mined_table_names_the_retail_interaction_and_rider_sequences() {
        const SMELTER_SEQUENCE: u64 = 3490;
        const MINING_SEQUENCE: u64 = 2161;
        const BY_HAND_SEQUENCE: u64 = 3832;
        const TALK_KIND: u8 = 63;
        let table = crate::combat::test_support::test_player_sequences();
        assert_eq!(table.station("@SmeltingT2_MasterName"), SMELTER_SEQUENCE);
        assert_eq!(
            table
                .gather(Some("Mining"))
                .map(|play| (play.sequence, play.crouch)),
            Some((MINING_SEQUENCE, false))
        );
        assert_eq!(
            table.gather(None).map(|play| (play.sequence, play.crouch)),
            Some((BY_HAND_SEQUENCE, true))
        );
        assert_eq!(
            table
                .interaction(TALK_KIND)
                .map(|play| (play.state, play.sequence)),
            Some((INTERACTION_STATE, movement::IDLE_SEQUENCE))
        );
        assert_eq!(table.rider("NoSuchMount"), table.rider("Horse"));
    }

    /// Retail `retail-2026-11-19-towns-duels`, other players' child 37: -5 in the start and run
    /// `13=1a`, -2 in a stop `13=1b`, -7 in a restart `13=1b`, 0 standing still.
    #[test]
    fn the_movement_accuracy_follows_the_movement_state() {
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut counter = 1000;
        let mut step = |track: &mut SlayerTrack, intent: Intent| {
            counter += 1;
            track.record(quiet_record(counter, intent), &table);
            own_state(track, counter);
            track.remote_movement_accuracy()
        };
        assert_eq!(step(&mut track, Intent::default()), STILL_ACCURACY);
        assert_eq!(step(&mut track, forward), RUN_ACCURACY, "the start");
        assert_eq!(
            step(&mut track, Intent::default()),
            WALK_ACCURACY,
            "the stop"
        );
        assert_eq!(step(&mut track, forward), BACK_ACCURACY, "the restart");
        assert_eq!(step(&mut track, forward), RUN_ACCURACY, "the start again");
        for _ in 0..movement::STOP_RECORDS + 2 {
            step(&mut track, Intent::default());
        }
        assert_eq!(track.remote_movement_accuracy(), STILL_ACCURACY, "standing");
    }

    /// Retail, 1,196 ledgers (`interact.py`, `gather.py`): the own state 10 ms after an accepted
    /// trading post was `13=24 15=3839 16=00` (89 of 89) and the end `13=1f 15=1081 16=2d
    /// 18=00`; a gather by hand `13=09 15=3832` crouched (child 43 `40`) and ended in the
    /// stance's stand. Ours sent none of them.
    #[test]
    fn an_interaction_plays_its_state_and_ends_in_its_stand() {
        let table = table();
        let mut track = spawned();
        track.record(quiet_record(900, Intent::default()), &table);
        own_state(&mut track, 900);
        track.interacted(InteractPlay {
            state: INTERACTION_STATE,
            sequence: 3839,
            crouch: false,
        });
        let started = own_state(&mut track, 901).state.expect("the interaction");
        assert_eq!(
            (started.state, started.sequence, started.second),
            (
                Some(INTERACTION_STATE),
                3839,
                Some(SecondSlayerState {
                    started: 901,
                    playing: None,
                    stopped: true,
                })
            )
        );
        assert_eq!(track.remote_posture(), AlcPosture::Stand);
        track.interaction_over();
        let end = own_state(&mut track, 902).state.expect("the stand");
        assert_eq!(end, stand_state(902));
        track.interacted(InteractPlay {
            state: GATHER_STATE,
            sequence: 3832,
            crouch: true,
        });
        let gather = own_state(&mut track, 903).state.expect("the gather");
        assert_eq!(
            (gather.state, gather.sequence, gather.second),
            (Some(9), 3832, None)
        );
        assert_eq!(
            track.remote_posture(),
            AlcPosture::Crouch,
            "a gather by hand crouches"
        );
        track.interaction_over();
        let rest = own_state(&mut track, 904).state.expect("the stand");
        assert_eq!(
            (rest.state, rest.sequence, rest.second),
            (Some(movement::IDLE_STATE), movement::IDLE_SEQUENCE, None)
        );
        assert_eq!(track.remote_posture(), AlcPosture::Stand);
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

    /// Retail, the four posture captures: each of the 16 toggles changed child 43 in the own
    /// state of the change's sequence, and other players' states carried it with theirs. The
    /// keys and a pad's stick crouch alike: the posture comes with the change's sequence.
    #[test]
    fn the_others_get_the_posture_with_its_change() {
        const TOGGLE: u16 = 0x386;
        /// Records past each change: longer than its longest settle (prone to a crouch, 26).
        const HELD_RECORDS: u16 = 30;
        const STICK_FORWARD: [f32; 2] = [0.0, 1.0];
        let still = Intent::default();
        let keys = Intent::of_keys(true, false, false, false);
        let pad = Intent::default().with_stick(Some(STICK_FORWARD));
        for (device, intent) in [("still", still), ("keys", keys), ("pad", pad)] {
            let mut track = spawned();
            let mut counter = TOGGLE - 1;
            posture_step(&mut track, &mut counter, intent, Posture::Stand);
            assert_eq!(track.remote_posture(), AlcPosture::Stand, "{device}");
            for (to, shown) in [
                (Posture::Crouch, AlcPosture::Crouch),
                (Posture::Stand, AlcPosture::Stand),
                (Posture::Prone, AlcPosture::Prone),
                (Posture::Stand, AlcPosture::Stand),
            ] {
                let (_, change) = posture_step(&mut track, &mut counter, intent, to);
                assert!(
                    change.is_some(),
                    "{device}: a state with the change to {to:?}"
                );
                assert_eq!(
                    (track.remote_posture(), track.remote_action()),
                    (shown, change),
                    "{device}: the others get the posture with it"
                );
                for _ in 0..HELD_RECORDS {
                    posture_step(&mut track, &mut counter, intent, to);
                    assert_eq!(track.remote_posture(), shown, "{device}: it holds");
                }
            }
        }
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
        // No stop, and no start: the move goes on, into the stand's run once its start's time
        // is over (retail sent the run `aa2f` at 97 of these stand-ups).
        for _ in 0..30 {
            let (_, state) = posture_step(&mut track, &mut counter, forward, Posture::Stand);
            assert!(state.is_none_or(|state| state.state.is_none()
                && [movement::RESTART_SEQUENCE, movement::RUN_SEQUENCE].contains(&state.sequence)));
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
            Some((STARTED + FREE_RECORDS, Some(movement::START_STATE))),
            "the held key moves it again from the free point, in the move start (13=1a; retail \
             `retail-2026-11-19-towns-duels`: 5 and 5 of the moves after a hit in 13=1a and 13=1b)"
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

    /// Retail, 1,206 ledgers: an inventory open while moving played `1152` and its loop `615`
    /// 380 ms later, a stop the open `962b` again, and a close while moving `3699`; ours played
    /// the standing sequences on walking legs.
    #[test]
    fn the_inventory_slot_plays_the_walk_s_sequences_on_walking_legs() {
        const OPENED_IN: u16 = 0x2000;
        let table = table();
        let forward = Intent::of_keys(true, false, false, false);
        let mut track = spawned();
        let mut counter = 0x1ff0;
        let mut sequences = Vec::new();
        let step = |track: &mut SlayerTrack,
                    counter: &mut u16,
                    intent: Intent,
                    sequences: &mut Vec<u64>| {
            *counter += 1;
            track.record(quiet_record(*counter, intent), &table);
            if let Some(sequence) = own_state(track, *counter)
                .slots
                .and_then(|slots| slots.second.sequence)
                .filter(|sequence| sequences.last() != Some(sequence))
            {
                sequences.push(sequence);
            }
        };
        step(&mut track, &mut counter, forward, &mut sequences);
        track.open_loadout(OPENED_IN, u64::from(counter) * TICK_MS);
        let loop_ticks = loadout::WALK_LOOP_MS / TICK_MS + 2;
        for _ in 0..loop_ticks {
            step(&mut track, &mut counter, forward, &mut sequences);
        }
        assert_eq!(
            sequences,
            [loadout::WALK_OPEN_SEQUENCE, loadout::WALK_LOOP_SEQUENCE]
        );
        step(&mut track, &mut counter, Intent::default(), &mut sequences);
        assert_eq!(sequences.last(), Some(&loadout::OPEN_SEQUENCE), "a stop");
        step(&mut track, &mut counter, forward, &mut sequences);
        assert_eq!(
            sequences.last(),
            Some(&loadout::WALK_LOOP_SEQUENCE),
            "a move"
        );
        track.close_loadout(OPENED_IN + 0x40);
        step(&mut track, &mut counter, forward, &mut sequences);
        assert_eq!(sequences.last(), Some(&loadout::WALK_CLOSE_SEQUENCE));
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
            after: Vec::new(),
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
            after: Vec::new(),
            cast: Some(AbilityCast {
                slots: SlotsChange {
                    second_state: Some(33),
                    second_sequence: Some(993),
                    third_state: Some(0),
                    third_sequence: Some(0),
                    marker: Some(3),
                    ..SlotsChange::default()
                },
                ..AbilityCast::default()
            }),
            seen: 417,
        }
    }

    const PRESS_RECORD: u16 = 0x0fe4;
    const TRIP: u32 = 1;
    const GROUND: u32 = 2;

    /// Records and own states (one each, standing still) from own state `number` on until the
    /// own state for which `wanted` holds: gives its number and what it sent. Panics after
    /// `MAX_STATES`.
    fn state_until(
        track: &mut SlayerTrack,
        table: &PlayerSequences,
        number: u16,
        wanted: impl Fn(&SlayerDue) -> bool,
    ) -> (u16, SlayerDue) {
        const MAX_STATES: u16 = 60;
        for next in number..number + MAX_STATES {
            track.record(quiet_record(next, Intent::default()), table);
            let due = own_state(track, next);
            if wanted(&due) {
                return (next, due);
            }
        }
        panic!("no such own state in {MAX_STATES} states from {number}");
    }

    /// Milliseconds of own states from `from` to `to`.
    fn states_ms(from: u16, to: u16) -> u64 {
        u64::from(to - from) * TICK_MS
    }

    /// Rain of Arrows plays as retail's own states (1,195 ledgers, 2026-11-21: the miners in
    /// `local-data/scratch-work/bow-live-2026-11-21/`), from the shared table: its aim's start,
    /// `18=34 48=128` 352 ms later (16 of 16); at its fire the full-body shot `13=08 15=5037` with
    /// `16=45 18=0 21=387 48=129` (15 of 16); 535 ms after the shot its end `19=0 21=0 48=0` (19
    /// of 25 shots), the first slot back to the movement; the end of its action after that
    /// changes nothing. The play needs no input: it is the same for a keyboard and a pad. Before,
    /// live 2026-11-21: the shot sent Splinter Shot's `16=33 18=571 48=43` and no first slot, and
    /// the own client stayed in the aim.
    #[test]
    fn rain_of_arrows_aims_then_shoots_on_the_full_body_and_ends_535_ms_after_the_shot() {
        const RAIN: &str = "Ability_Bow_RainofArrows";
        const AIM_SEQUENCE: u64 = 1453;
        const AIM_LOOP_SEQUENCE: u64 = 34;
        const AIM_LOOP_MS: u64 = 352;
        const SHOT_STATE: u64 = 8;
        const SHOT_SEQUENCE: u64 = 5037;
        const SHOT_THIRD_SEQUENCE: u64 = 387;
        const SHOT_MARKER: u64 = 129;
        const SHOT_END_MS: u64 = 535;
        let table = crate::combat::test_support::test_player_sequences();
        let rain = nw_formats::objectstream::lowercase_crc32(RAIN);
        let play = table.ability(rain).expect("the rain's play");
        let mut track = spawned();
        track.record(quiet_record(PRESS_RECORD, Intent::default()), &table);
        track.ability_started(rain, play, PRESS_RECORD);
        let start_state = PRESS_RECORD + 1;
        let start = own_state(&mut track, start_state).slots.expect("the aim");
        assert_eq!(start.second.sequence, Some(AIM_SEQUENCE));
        let (looped, _) = state_until(&mut track, &table, start_state + 1, |due| {
            due.slots
                .is_some_and(|slots| slots.second.sequence == Some(AIM_LOOP_SEQUENCE))
        });
        let loop_ms = states_ms(start_state, looped);
        assert!(
            (AIM_LOOP_MS..AIM_LOOP_MS + TICK_MS).contains(&loop_ms),
            "the aim's loop at {loop_ms} ms"
        );
        let shot_state = looped + 1;
        track.record(quiet_record(shot_state, Intent::default()), &table);
        track.ability_cast(rain, play);
        let shot = own_state(&mut track, shot_state);
        let first = shot.state.expect("the full-body shot");
        assert_eq!(
            (first.state, first.sequence, first.started),
            (Some(SHOT_STATE), SHOT_SEQUENCE, Some(shot_state))
        );
        let slots = shot.slots.expect("the shot's slots");
        assert_eq!(
            (
                slots.second.state,
                slots.second.sequence,
                slots.third.sequence,
                slots.marker
            ),
            (
                Some(CAST_END_STATE),
                Some(CLEARED_SLOT),
                Some(SHOT_THIRD_SEQUENCE),
                Some(SHOT_MARKER)
            )
        );
        let (ended, end) = state_until(&mut track, &table, shot_state + 1, |due| {
            due.slots
                .is_some_and(|slots| slots.marker == Some(CLEARED_SLOT))
        });
        let end_ms = states_ms(shot_state, ended);
        assert!(
            (SHOT_END_MS..SHOT_END_MS + TICK_MS).contains(&end_ms),
            "the end at {end_ms} ms"
        );
        let slots = end.slots.expect("the end");
        assert_eq!(
            (slots.third.state, slots.third.sequence),
            (Some(CLEARED_SLOT), Some(CLEARED_SLOT))
        );
        let stand = end.state.expect("back to the movement");
        assert_ne!(stand.state, Some(SHOT_STATE));
        assert!(track.ability.is_none(), "the play ended");
        track.ability_ended(rain);
        assert!(track.pending.is_none(), "the action's end changes nothing");
    }

    /// Penetrating Shot plays as retail's own states (733 own shots, 2026-11-21): at its fire
    /// `18=36`, then `48=520` 667 ms after the shot, `18=4981 48=8` at 766 ms, `48=40` at 800 ms
    /// and its end at 834 ms, which closes the second slot and clears the third (`21=2343` of
    /// its aim) and the marker. The play needs no input: it is the same for a keyboard and a pad.
    #[test]
    fn penetrating_shot_shoots_then_lowers_and_ends_834_ms_after_the_shot() {
        const PENETRATING: &str = "Ability_Bow_PenetratingShot";
        const SHOT_SEQUENCE: u64 = 36;
        const LOWER_SEQUENCE: u64 = 4981;
        /// The changes after the shot: ms, and the marker each sends.
        const STEPS: [(u64, u64); 3] = [(667, 520), (766, 8), (800, 40)];
        const SHOT_END_MS: u64 = 834;
        let table = crate::combat::test_support::test_player_sequences();
        let penetrating = nw_formats::objectstream::lowercase_crc32(PENETRATING);
        let play = table.ability(penetrating).expect("the shot's play");
        let mut track = spawned();
        track.record(quiet_record(PRESS_RECORD, Intent::default()), &table);
        track.ability_started(penetrating, play, PRESS_RECORD);
        own_state(&mut track, PRESS_RECORD + 1);
        let shot_state = PRESS_RECORD + 2;
        track.record(quiet_record(shot_state, Intent::default()), &table);
        track.ability_cast(penetrating, play);
        let shot = own_state(&mut track, shot_state).slots.expect("the shot");
        assert_eq!(shot.second.sequence, Some(SHOT_SEQUENCE));
        let mut number = shot_state + 1;
        for (after_ms, marker) in STEPS {
            let (at, due) = state_until(&mut track, &table, number, |due| {
                due.slots.is_some_and(|slots| slots.marker == Some(marker))
            });
            let at_ms = states_ms(shot_state, at);
            assert!(
                (after_ms..after_ms + TICK_MS).contains(&at_ms),
                "48={marker} at {at_ms} ms"
            );
            if marker != STEPS[0].1 {
                assert_eq!(
                    due.slots.map(|slots| slots.second.sequence),
                    Some(Some(LOWER_SEQUENCE))
                );
            }
            number = at + 1;
        }
        let (ended, end) = state_until(&mut track, &table, number, |due| {
            due.slots
                .is_some_and(|slots| slots.marker == Some(CLEARED_SLOT))
        });
        let end_ms = states_ms(shot_state, ended);
        assert!(
            (SHOT_END_MS..SHOT_END_MS + TICK_MS).contains(&end_ms),
            "the end at {end_ms} ms"
        );
        let slots = end.slots.expect("the end");
        assert_eq!(
            (
                slots.second.state,
                slots.second.sequence,
                slots.third.sequence
            ),
            (
                Some(loadout::CLOSED_STATE),
                Some(CLEARED_SLOT),
                Some(CLEARED_SLOT)
            )
        );
    }

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
        for intent in [
            Intent::default(),
            Intent::default(),
            forward,
            Intent::default(),
        ] {
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
        let state = own_state(&mut track, named + 1).state.expect("the ability");
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

    /// The stances of the ranged shot study (2026-11-18) in the mined table.
    const BOW: &str = "Bow,LongBow";
    const MUSKET: &str = "2H_Ranged,Rifle";
    /// Own states to settle a draw and an aim before a shot.
    const SETTLE_STATES: u16 = 40;

    fn aim() -> Intent {
        Intent {
            block: true,
            ..Intent::default()
        }
    }

    /// Whether `slots` send every child `change` names.
    fn sends(slots: &UpperSlots, change: SlotsChange) -> bool {
        let same = |want: Option<u64>, got: Option<u64>| want.is_none() || want == got;
        same(change.second_state, slots.second.state)
            && same(change.second_sequence, slots.second.sequence)
            && same(change.third_state, slots.third.state)
            && same(change.third_sequence, slots.third.sequence)
            && same(change.marker, slots.marker)
    }

    /// Feeds one record of `tags` drawn with `intent` and the key bits `bits` of each own state
    /// numbered in `numbers`, and gives the new upper slot changes of those states (echoes left
    /// out) with their number.
    fn shots(
        track: &mut SlayerTrack,
        table: &PlayerSequences,
        tags: &str,
        intent: Intent,
        numbers: std::ops::Range<u16>,
        bits: impl Fn(u16) -> AttackKeyBits,
    ) -> Vec<(u16, UpperSlots)> {
        loaded_shots(track, table, tags, intent, numbers, bits, |_| false)
    }

    /// [`shots`] of a weapon that is `empty` in the records it names.
    fn loaded_shots(
        track: &mut SlayerTrack,
        table: &PlayerSequences,
        tags: &str,
        intent: Intent,
        numbers: std::ops::Range<u16>,
        bits: impl Fn(u16) -> AttackKeyBits,
        empty: impl Fn(u16) -> bool,
    ) -> Vec<(u16, UpperSlots)> {
        let mut seen: Vec<(u16, UpperSlots)> = Vec::new();
        for number in numbers {
            let record = SlayerRecord {
                weapon: Some(HeldWeapon {
                    empty: empty(number),
                    ..weapon(tags, true).expect("a weapon")
                }),
                ..keyed(number, intent, bits(number), tags)
            };
            track.record(record, table);
            let Some(slots) = own_state(track, number).slots else {
                continue;
            };
            let untimed = UpperSlots {
                time: None,
                ..slots
            };
            if seen.last().is_none_or(|(_, last)| *last != untimed) {
                seen.push((number, untimed));
            }
        }
        seen
    }

    /// A track of `tags` drawn and settled, aiming (`aiming`) or not, up to own state `until`.
    fn settled(table: &PlayerSequences, tags: &str, aiming: bool, until: u16) -> SlayerTrack {
        let mut track = spawned();
        let still = Intent::default();
        shots(&mut track, table, tags, still, 1..SETTLE_STATES, |_| HELD);
        if aiming {
            shots(&mut track, table, tags, aim(), SETTLE_STATES..until, |_| {
                HELD
            });
        }
        track
    }

    /// The number of the first state in `seen` that sends `change`.
    fn first_sending(seen: &[(u16, UpperSlots)], change: SlotsChange) -> Option<u16> {
        seen.iter()
            .find(|(_, slots)| sends(slots, change))
            .map(|(number, _)| *number)
    }

    /// Own states from `after_ms` of a shot in state `shot` to the state that carries it.
    fn state_after(shot: u16, after_ms: u64) -> u16 {
        shot + u16::try_from(after_ms.div_ceil(TICK_MS)).expect("a short time")
    }

    /// Retail bow (ranged shot study 2026-11-18): an aimed press draws and sends no shot; the
    /// release sends `18=35 48=41`, then the next arrow's draw `16=39 18=2206 48=40` and the aim's
    /// hold `18=29`. Live 2026-11-18: ours shot at the press and the others saw no arrow leave.
    #[test]
    fn the_bow_shoots_its_aimed_shot_at_the_release_with_keyboard_and_controller() {
        const PRESS_AT: u16 = 80;
        const RELEASE_AT: u16 = 84;
        const END: u16 = 140;
        let table = crate::combat::test_support::test_player_sequences();
        let bow = table.stance(stance_key(BOW)).expect("bow");
        let shot = bow.alt_fire.expect("the aimed shot");
        for (device, down) in [("keyboard", PRESS), ("controller", PAD_HOLD)] {
            let mut track = settled(&table, BOW, true, PRESS_AT);
            let key = |number: u16| match number {
                PRESS_AT => down,
                RELEASE_AT => RELEASE,
                number if number < RELEASE_AT => {
                    if down == PAD_HOLD {
                        PAD_HOLD
                    } else {
                        HELD
                    }
                }
                _ => HELD,
            };
            let seen = shots(&mut track, &table, BOW, aim(), PRESS_AT..END, key);
            let fired = first_sending(&seen, shot);
            assert_eq!(
                fired,
                Some(RELEASE_AT),
                "{device}: at the release, not the press"
            );
            let remote = track.remote_upper();
            assert!(remote.is_none(), "{device}: the steps are over");
            for step in &bow.alt_fire_after {
                let at = first_sending(&seen, step.slots).expect("the step");
                assert_eq!(
                    at,
                    state_after(RELEASE_AT, step.after_ms),
                    "{device}: {step:?}"
                );
            }
            let hold = bow.alt_hold.expect("the aim's hold").stand.hold;
            let hold_again_ms = bow.alt_fire_after.last().expect("steps").after_ms;
            let (last_number, last) = seen.last().expect("the last change");
            assert_eq!(
                (last.second.sequence, *last_number),
                (Some(hold), state_after(RELEASE_AT, hold_again_ms)),
                "{device}: the steps end on the aim's hold, and the hold sends no other"
            );
        }
    }

    /// Retail bow: a key held to the full draw sends `48=24` (137 of 157 drawn shots), and the
    /// release then sends the drawn shot `18=38 48=25`, not the plain one.
    #[test]
    fn a_bow_held_to_its_full_draw_marks_it_and_its_release_shoots_the_drawn_shot() {
        const PRESS_AT: u16 = 80;
        const RELEASE_AT: u16 = 110;
        const END: u16 = 120;
        let table = crate::combat::test_support::test_player_sequences();
        let bow = table.stance(stance_key(BOW)).expect("bow");
        let charge = bow.alt_charge.as_ref().expect("the full draw");
        for (device, down, hold) in [
            ("keyboard", PRESS, HELD),
            ("controller", PAD_HOLD, PAD_HOLD),
        ] {
            let mut track = settled(&table, BOW, true, PRESS_AT);
            let key = |number: u16| match number {
                PRESS_AT => down,
                RELEASE_AT => RELEASE,
                number if number < RELEASE_AT => hold,
                _ => HELD,
            };
            let seen = shots(&mut track, &table, BOW, aim(), PRESS_AT..END, key);
            let drawn = first_sending(&seen, charge.drawn).expect("the full draw");
            let held_s = f32::from(drawn - PRESS_AT) * FRAME_S;
            #[allow(clippy::cast_precision_loss, reason = "a draw time of milliseconds")]
            let full_s = charge.after_ms as f32 / MS_PER_S;
            assert!(
                held_s >= full_s && held_s < full_s + FRAME_S,
                "{device}: drawn after {held_s} s"
            );
            assert_eq!(
                first_sending(&seen, charge.fire),
                Some(RELEASE_AT),
                "{device}"
            );
            assert_eq!(
                first_sending(&seen, bow.alt_fire.expect("the plain shot")),
                None,
                "{device}: no plain shot"
            );
        }
    }

    #[test]
    fn a_bow_aim_let_go_before_the_release_shoots_nothing() {
        const PRESS_AT: u16 = 80;
        const LET_GO_AT: u16 = 83;
        const RELEASE_AT: u16 = 85;
        let table = crate::combat::test_support::test_player_sequences();
        let bow = table.stance(stance_key(BOW)).expect("bow");
        let mut track = settled(&table, BOW, true, PRESS_AT);
        let key = |number: u16| match number {
            PRESS_AT => PRESS,
            RELEASE_AT => RELEASE,
            _ => HELD,
        };
        let mut seen = shots(&mut track, &table, BOW, aim(), PRESS_AT..LET_GO_AT, key);
        let still = Intent::default();
        seen.extend(shots(
            &mut track,
            &table,
            BOW,
            still,
            LET_GO_AT..RELEASE_AT + 2,
            key,
        ));
        assert_eq!(first_sending(&seen, bow.alt_fire.expect("shot")), None);
    }

    /// Retail hip shots (ranged shot study 2026-11-18): the bow's `16=33 18=4720 48=3` then
    /// `19=46 21=4170 48=67` (99 % of 468 seen by other players), the musket's `16=33 18=74
    /// 48=3`, the blunderbuss's `16=33 18=97 19=46 21=416 48=67`, each with its steps and end.
    /// Live 2026-11-18: the server sent no state for 57 of 71 bow hip presses.
    #[test]
    fn a_hip_press_shoots_the_bow_the_musket_and_the_blunderbuss_with_keyboard_and_controller() {
        const PRESS_AT: u16 = 50;
        const RELEASE_AT: u16 = 53;
        let table = crate::combat::test_support::test_player_sequences();
        for tags in [BOW, MUSKET, BLUNDERBUSS] {
            let hip = table
                .stance(stance_key(tags))
                .and_then(|stance| stance.hip.clone())
                .expect("the hip shot");
            let end = state_after(PRESS_AT, hip.end.after_ms);
            for (device, down) in [("keyboard", PRESS), ("controller", PAD_HOLD)] {
                let mut track = settled(&table, tags, false, PRESS_AT);
                let key = |number: u16| match number {
                    PRESS_AT => down,
                    RELEASE_AT => RELEASE,
                    number if number < RELEASE_AT && down == PAD_HOLD => PAD_HOLD,
                    _ => HELD,
                };
                let still = Intent::default();
                let seen = shots(&mut track, &table, tags, still, PRESS_AT..end + 4, key);
                let (first, start) = seen.first().expect("the start");
                assert_eq!(
                    *first, PRESS_AT,
                    "{tags} {device}: in the press's own state"
                );
                assert!(sends(start, hip.start), "{tags} {device}: {start:?}");
                assert_eq!(start.second.started, Some(PRESS_AT), "{tags} {device}");
                for step in &hip.after {
                    assert_eq!(
                        first_sending(&seen, step.slots),
                        Some(state_after(PRESS_AT, step.after_ms)),
                        "{tags} {device}: {step:?}"
                    );
                }
                let (last_number, last) = seen.last().expect("the end");
                assert!(sends(last, hip.end.slots), "{tags} {device}: {last:?}");
                assert_eq!(*last_number, end, "{tags} {device}");
            }
        }
    }

    /// Retail bow: a hip press while the hip shot plays fires again where its end was due (86
    /// re-fires, 936 ms after the shot before): `19=0 21=0 48=3`, then `19=46 21=4170 48=67`.
    #[test]
    fn a_bow_hip_press_while_its_shot_plays_fires_again_at_its_end() {
        const PRESS_AT: u16 = 50;
        const AGAIN_AT: u16 = 62;
        let table = crate::combat::test_support::test_player_sequences();
        let hip = table
            .stance(stance_key(BOW))
            .and_then(|stance| stance.hip.clone())
            .expect("the hip shot");
        let refire = hip.refire.expect("the re-fire");
        let fire = *hip.after.first().expect("the fire");
        let mut track = settled(&table, BOW, false, PRESS_AT);
        let key = |number: u16| match number {
            PRESS_AT | AGAIN_AT => PRESS,
            _ if number == PRESS_AT + 1 || number == AGAIN_AT + 1 => RELEASE,
            _ => HELD,
        };
        let due_end = state_after(PRESS_AT, hip.end.after_ms);
        let later_end = state_after(due_end, hip.end.after_ms);
        let still = Intent::default();
        let seen = shots(&mut track, &table, BOW, still, PRESS_AT..later_end + 4, key);
        let at = |change: SlotsChange, from: u16| {
            seen.iter()
                .find(|(number, slots)| *number >= from && sends(slots, change))
                .map(|(number, _)| *number)
        };
        assert_eq!(
            at(refire, PRESS_AT),
            Some(due_end),
            "the end's state re-fires"
        );
        assert_eq!(
            at(hip.end.slots, PRESS_AT),
            Some(later_end),
            "the end counts again from the re-fire"
        );
        assert_eq!(
            at(fire.slots, due_end),
            Some(state_after(due_end, fire.after_ms))
        );
    }

    /// Retail musket: the aimed shot `18=1535 48=41` at the press, then `48=45`, the reload
    /// `16=39 18=888 48=44`, `18=1716` and the aim again `16=36 18=3544 48=40`, whose hold
    /// `18=4249` follows (2,637 shots seen by other players). It has no reload press: the first
    /// press after it was loaded again shoots.
    #[test]
    fn the_musket_shoots_at_the_press_reloads_by_its_steps_and_aims_again() {
        const PRESS_AT: u16 = 80;
        const LOADED_AT: u16 = 140;
        const AGAIN_AT: u16 = 150;
        let table = crate::combat::test_support::test_player_sequences();
        let musket = table.stance(stance_key(MUSKET)).expect("musket");
        let hold = musket.alt_hold.expect("the aim's hold");
        let mut track = settled(&table, MUSKET, true, PRESS_AT);
        let key = |number: u16| match number {
            PRESS_AT | AGAIN_AT => PRESS,
            _ if number == PRESS_AT + 1 || number == AGAIN_AT + 1 => RELEASE,
            _ => HELD,
        };
        let seen = loaded_shots(
            &mut track,
            &table,
            MUSKET,
            aim(),
            PRESS_AT..AGAIN_AT + 2,
            key,
            |number| number > PRESS_AT && number < LOADED_AT,
        );
        let shot = musket.alt_fire.expect("the aimed shot");
        assert_eq!(first_sending(&seen, shot), Some(PRESS_AT));
        for step in &musket.alt_fire_after {
            assert_eq!(
                first_sending(&seen, step.slots),
                Some(state_after(PRESS_AT, step.after_ms)),
                "{step:?}"
            );
        }
        let aim_again = musket
            .alt_fire_after
            .iter()
            .find(|step| step.slots.second_sequence == Some(hold.stand.start))
            .expect("the aim again");
        let aimed_again = state_after(PRESS_AT, aim_again.after_ms);
        let held = seen
            .iter()
            .find(|(number, slots)| {
                *number > aimed_again && slots.second.sequence == Some(hold.stand.hold)
            })
            .map(|(number, _)| *number)
            .expect("the aim's hold again");
        let hold_states = u16::try_from(hold.hold_after_ms.div_ceil(TICK_MS)).expect("short");
        assert!(held <= aimed_again + hold_states + 1, "held at {held}");
        assert!(
            seen.iter()
                .any(|(number, slots)| *number == AGAIN_AT && sends(slots, shot)),
            "the first press after the load shoots"
        );
    }

    /// Retail musket (rifle study 2026-11-20): presses inside the shot's reload before its
    /// `18=1716` step fire nothing; a press after it waits, and the shot goes out 1,734 ms after
    /// the shot before. Live 2026-11-20: a player pressing every 200 ms through the reload
    /// showed the others a new shot each time.
    #[test]
    fn a_musket_press_in_its_reload_fires_nothing_and_a_later_one_waits_for_its_gap() {
        const PRESS_AT: u16 = 80;
        /// Presses 165 to 660 ms after the shot, before the wait starts at 867 ms.
        const EARLY: [u16; 3] = [5, 10, 20];
        /// A press 990 ms after the shot.
        const WAITING: u16 = 30;
        let table = crate::combat::test_support::test_player_sequences();
        let musket = table.stance(stance_key(MUSKET)).expect("musket");
        let shot = musket.alt_fire.expect("the aimed shot");
        let gap_ms = musket.alt_fire_gap_ms.expect("the gap");
        let wait_from_ms = musket.alt_fire_wait_from_ms.expect("the wait start");
        assert!(u64::from(EARLY[2]) * TICK_MS < wait_from_ms);
        assert!(u64::from(WAITING) * TICK_MS > wait_from_ms);
        let end = state_after(PRESS_AT, gap_ms) + 4;
        let fired = FiredShot {
            record: Some(PRESS_AT),
            kind: AttackKind::Light,
            aimed: true,
        };
        for (device, down) in DEVICES {
            let mut track = settled(&table, MUSKET, true, PRESS_AT);
            let early = EARLY.map(|after| PRESS_AT + after);
            let presses = taps(down, std::iter::once(PRESS_AT).chain(early).collect());
            let seen = shots(&mut track, &table, MUSKET, aim(), PRESS_AT..end, presses);
            let fires: Vec<u16> = seen
                .iter()
                .filter(|(_, slots)| sends(slots, shot))
                .map(|(number, _)| *number)
                .collect();
            assert_eq!(
                fires,
                vec![PRESS_AT],
                "{device}: one shot, none in the reload"
            );
            assert_eq!(
                track.take_fired(),
                vec![fired],
                "{device}: the server's one shot"
            );
            let mut track = settled(&table, MUSKET, true, PRESS_AT);
            let presses = taps(down, vec![PRESS_AT, PRESS_AT + WAITING]);
            let seen = shots(&mut track, &table, MUSKET, aim(), PRESS_AT..end, presses);
            assert_eq!(
                sending_from(&seen, shot, PRESS_AT + 1),
                Some(state_after(PRESS_AT, gap_ms)),
                "{device}: the waiting press shoots at the gap"
            );
            assert_eq!(
                track.take_fired(),
                vec![
                    fired,
                    FiredShot {
                        record: None,
                        ..fired
                    }
                ],
                "{device}: the waiting shot goes out with its own state"
            );
        }
    }

    /// Retail musket (rifle study 2026-11-20): with the right-click held during a hip shot, the
    /// aim starts 2,249 ms after the shot (16 cases, 2,215 to 2,269), and an aimed press in that
    /// time shoots 267 ms after the aim's start; a fresh aim's press shoots 267 ms after the
    /// aim's start (71 aims). Ours raised the aim at once, dropped the hip shot's reload and shot
    /// at the press.
    #[test]
    fn a_musket_aim_waits_for_its_hip_shot_and_shoots_once_it_is_up_with_keyboard_and_controller() {
        const PRESS_AT: u16 = 50;
        /// The right-click goes down 330 ms after the hip shot, the aimed press 1,320 ms after.
        const AIM_AFTER: u16 = 10;
        const AIMED_PRESS_AFTER: u16 = 40;
        /// A fresh aim's start and its press in the next record.
        const FRESH_AIM_AT: u16 = 40;
        let table = crate::combat::test_support::test_player_sequences();
        let musket = table.stance(stance_key(MUSKET)).expect("musket");
        let hip = musket.hip.clone().expect("the hip shot");
        let start = musket.alt_start.expect("the aim's start");
        let aim_start = SlotsChange {
            second_state: start.second_state,
            second_sequence: start.second_sequence,
            ..SlotsChange::default()
        };
        let shot = musket.alt_fire.expect("the aimed shot");
        let ready_ms = musket.alt_fire_ready_ms.expect("the aim's ready time");
        let aim_due = state_after(PRESS_AT, hip.aim_at_ms());
        let end = aim_due + 40;
        let still = Intent::default();
        for (device, down) in DEVICES {
            let mut track = settled(&table, MUSKET, false, PRESS_AT);
            let presses = taps(down, vec![PRESS_AT, PRESS_AT + AIMED_PRESS_AFTER]);
            let mut seen = shots(
                &mut track,
                &table,
                MUSKET,
                still,
                PRESS_AT..PRESS_AT + AIM_AFTER,
                &presses,
            );
            seen.extend(shots(
                &mut track,
                &table,
                MUSKET,
                aim(),
                PRESS_AT + AIM_AFTER..end,
                &presses,
            ));
            let aimed = sending_from(&seen, aim_start, PRESS_AT + 1).expect("the aim");
            assert!(
                (aim_due..=aim_due + 1).contains(&aimed),
                "{device}: the aim at {aimed}, due at {aim_due}"
            );
            let reload = hip
                .after
                .iter()
                .find(|step| step.slots.second_sequence.is_some())
                .expect("the hip shot's reload");
            assert_eq!(
                sending_from(&seen, reload.slots, PRESS_AT),
                Some(state_after(PRESS_AT, reload.after_ms)),
                "{device}: the hip shot's reload plays"
            );
            assert_eq!(
                sending_from(&seen, shot, PRESS_AT),
                Some(state_after(aimed, ready_ms)),
                "{device}: the aimed shot once the aim is up"
            );
            let mut track = settled(&table, MUSKET, false, FRESH_AIM_AT);
            let presses = taps(down, vec![FRESH_AIM_AT + 1]);
            let seen = shots(
                &mut track,
                &table,
                MUSKET,
                aim(),
                FRESH_AIM_AT..FRESH_AIM_AT + 20,
                presses,
            );
            assert_eq!(
                sending_from(&seen, aim_start, FRESH_AIM_AT),
                Some(FRESH_AIM_AT),
                "{device}: a fresh aim starts at once"
            );
            assert_eq!(
                sending_from(&seen, shot, FRESH_AIM_AT),
                Some(state_after(FRESH_AIM_AT, ready_ms)),
                "{device}: its press shoots once the aim is up"
            );
        }
    }

    /// The shots the others see are the shots the server fires ([`SlayerTrack::take_fired`]):
    /// the bow's hip shot at its press and its re-fire at the end's time, and its full draw's
    /// shot as the charged aimed attack.
    #[test]
    fn the_server_takes_each_shot_the_others_see_with_keyboard_and_controller() {
        const PRESS_AT: u16 = 50;
        const AGAIN_AT: u16 = 62;
        const DRAW_PRESS_AT: u16 = 80;
        const DRAW_RELEASE_AT: u16 = 110;
        let table = crate::combat::test_support::test_player_sequences();
        let hip = table
            .stance(stance_key(BOW))
            .and_then(|stance| stance.hip.clone())
            .expect("the hip shot");
        let due_end = state_after(PRESS_AT, hip.end.after_ms);
        let still = Intent::default();
        let hip_shot = FiredShot {
            record: Some(PRESS_AT),
            kind: AttackKind::Light,
            aimed: false,
        };
        for (device, down) in DEVICES {
            let mut track = settled(&table, BOW, false, PRESS_AT);
            let presses = taps(down, vec![PRESS_AT, AGAIN_AT]);
            shots(
                &mut track,
                &table,
                BOW,
                still,
                PRESS_AT..due_end + 2,
                presses,
            );
            assert_eq!(
                track.take_fired(),
                vec![
                    hip_shot,
                    FiredShot {
                        record: None,
                        ..hip_shot
                    }
                ],
                "{device}: the hip shot and its re-fire"
            );
            let mut track = settled(&table, BOW, true, DRAW_PRESS_AT);
            let hold = if down == PAD_HOLD { PAD_HOLD } else { HELD };
            let key = |number: u16| match number {
                DRAW_PRESS_AT => down,
                DRAW_RELEASE_AT => RELEASE,
                number if number < DRAW_RELEASE_AT => hold,
                _ => HELD,
            };
            shots(
                &mut track,
                &table,
                BOW,
                aim(),
                DRAW_PRESS_AT..DRAW_RELEASE_AT + 2,
                key,
            );
            assert_eq!(
                track.take_fired(),
                vec![FiredShot {
                    record: Some(DRAW_RELEASE_AT),
                    kind: AttackKind::Charged,
                    aimed: true,
                }],
                "{device}: the full draw's shot at the release"
            );
        }
    }

    /// The key pressed and the controller's trigger held, for the ranged shot tests.
    const DEVICES: [(&str, AttackKeyBits); 2] = [("keyboard", PRESS), ("controller", PAD_HOLD)];
    /// Records from a tap's press to its release.
    const TAP_RECORDS: u16 = 2;

    /// The key bits of taps pressed with `down` (a key's press or a controller's first held
    /// record) at each of `presses`, each let go [`TAP_RECORDS`] later.
    fn taps(down: AttackKeyBits, presses: Vec<u16>) -> impl Fn(u16) -> AttackKeyBits {
        move |number| {
            let tap = presses
                .iter()
                .find(|press| (**press..=**press + TAP_RECORDS).contains(&number));
            match tap {
                Some(press) if number == *press => down,
                Some(press) if number == *press + TAP_RECORDS => RELEASE,
                Some(_) if down == PAD_HOLD => PAD_HOLD,
                _ => HELD,
            }
        }
    }

    /// The first state in `seen` from `from` on that sends `change`.
    fn sending_from(seen: &[(u16, UpperSlots)], change: SlotsChange, from: u16) -> Option<u16> {
        seen.iter()
            .find(|(number, slots)| *number >= from && sends(slots, change))
            .map(|(number, _)| *number)
    }

    /// Retail blunderbuss (ranged shot study 2026-11-19): a hip press from the `48=66` step to
    /// 100 ms after it re-fires `48=67` in its own state (20 of 20), whose steps count again
    /// from there; a press before or after that window starts nothing, and the shot ends.
    #[test]
    fn a_blunderbuss_hip_press_in_its_window_fires_again_and_one_outside_it_starts_nothing() {
        const PRESS_AT: u16 = 50;
        let table = crate::combat::test_support::test_player_sequences();
        let hip = table
            .stance(stance_key(BLUNDERBUSS))
            .and_then(|stance| stance.hip.clone())
            .expect("the hip shot");
        let refire = hip.refire.expect("the re-fire");
        let window = hip.refire_window.expect("its window");
        let step = *hip.after.first().expect("the 48=66 step");
        // The first press record read after the own state the window opens in, and one press
        // record before and after the window.
        let inside = state_after(PRESS_AT, window.from_ms) + 1;
        let early = state_after(PRESS_AT, window.from_ms) - 1;
        let late = state_after(PRESS_AT, window.until_ms) + 1;
        let still = Intent::default();
        for (device, down) in DEVICES {
            let mut track = settled(&table, BLUNDERBUSS, false, PRESS_AT);
            let refired_end = state_after(inside, hip.end.after_ms);
            let presses = taps(down, vec![PRESS_AT, inside]);
            let seen = shots(
                &mut track,
                &table,
                BLUNDERBUSS,
                still,
                PRESS_AT..refired_end + 4,
                presses,
            );
            assert_eq!(
                sending_from(&seen, refire, PRESS_AT + 1),
                Some(inside),
                "{device}: the press's own state re-fires"
            );
            assert_eq!(
                sending_from(&seen, step.slots, inside),
                Some(state_after(inside, step.after_ms)),
                "{device}: the steps count again from the re-fire"
            );
            assert_eq!(
                sending_from(&seen, hip.end.slots, PRESS_AT),
                Some(refired_end),
                "{device}: the end counts again from the re-fire"
            );
            for outside in [early, late] {
                let mut track = settled(&table, BLUNDERBUSS, false, PRESS_AT);
                let end = state_after(PRESS_AT, hip.end.after_ms);
                let presses = taps(down, vec![PRESS_AT, outside]);
                let seen = shots(
                    &mut track,
                    &table,
                    BLUNDERBUSS,
                    still,
                    PRESS_AT..end + 4,
                    presses,
                );
                assert_eq!(
                    sending_from(&seen, refire, PRESS_AT + 1),
                    None,
                    "{device}: a press in record {outside} re-fires nothing"
                );
                assert_eq!(
                    sending_from(&seen, hip.end.slots, PRESS_AT),
                    Some(end),
                    "{device}: the shot ends at its time"
                );
            }
        }
    }

    /// Retail musket (ranged shot study 2026-11-19): 94 presses in 19 own hip shots sent nothing
    /// of their own; the shot's steps and end went on.
    #[test]
    fn a_musket_hip_press_while_its_shot_plays_starts_nothing() {
        const PRESS_AT: u16 = 50;
        let table = crate::combat::test_support::test_player_sequences();
        let hip = table
            .stance(stance_key(MUSKET))
            .and_then(|stance| stance.hip.clone())
            .expect("the hip shot");
        assert_eq!(hip.refire, None);
        let end = state_after(PRESS_AT, hip.end.after_ms);
        let expected: Vec<u16> = std::iter::once(PRESS_AT)
            .chain(
                hip.after
                    .iter()
                    .map(|step| state_after(PRESS_AT, step.after_ms)),
            )
            .chain(std::iter::once(end))
            .collect();
        let again = [5, 15, 25, 40].map(|after| PRESS_AT + after);
        for (device, down) in DEVICES {
            let mut track = settled(&table, MUSKET, false, PRESS_AT);
            let presses = taps(down, std::iter::once(PRESS_AT).chain(again).collect());
            let still = Intent::default();
            let seen = shots(
                &mut track,
                &table,
                MUSKET,
                still,
                PRESS_AT..end + 4,
                presses,
            );
            let numbers: Vec<u16> = seen.iter().map(|(number, _)| *number).collect();
            assert_eq!(numbers, expected, "{device}: the shot's own changes only");
        }
    }

    /// Retail blunderbuss (ranged shot study 2026-11-19): the shot that empties the weapon
    /// reloads `16=39 18=96` 766 ms after it, from the hip then ends `16=45 18=0`, aimed then
    /// aims again `16=36 18=4225`, whose hold follows; the first press once it is loaded
    /// again shoots (no reload press).
    #[test]
    fn the_blunderbuss_reloads_after_the_shot_that_empties_it() {
        const PRESS_AT: u16 = 80;
        let table = crate::combat::test_support::test_player_sequences();
        let gun = table.stance(stance_key(BLUNDERBUSS)).expect("blunderbuss");
        let hip = gun.hip.as_ref().expect("the hip shot");
        let hold = gun.alt_hold.expect("the aim's hold");
        let shot = gun.alt_fire.expect("the aimed shot");
        let emptied = |number: u16| number > PRESS_AT;
        let hip_end = hip.empty_after.last().expect("its end").after_ms;
        let aim_again = gun.alt_fire_empty_after.last().expect("the aim again");
        let aimed_again = state_after(PRESS_AT, aim_again.after_ms);
        let loaded_at = aimed_again + 10;
        let again_at = loaded_at + 2;
        for (device, down) in DEVICES {
            let mut track = settled(&table, BLUNDERBUSS, false, PRESS_AT);
            let seen = loaded_shots(
                &mut track,
                &table,
                BLUNDERBUSS,
                Intent::default(),
                PRESS_AT..state_after(PRESS_AT, hip_end) + 4,
                taps(down, vec![PRESS_AT]),
                emptied,
            );
            for step in &hip.empty_after {
                assert_eq!(
                    sending_from(&seen, step.slots, PRESS_AT + 1),
                    Some(state_after(PRESS_AT, step.after_ms)),
                    "{device} hip: {step:?}"
                );
            }
            assert_eq!(
                first_sending(&seen, hip.end.slots),
                None,
                "{device} hip: no end of a loaded shot"
            );
            let mut track = settled(&table, BLUNDERBUSS, true, PRESS_AT);
            let seen = loaded_shots(
                &mut track,
                &table,
                BLUNDERBUSS,
                aim(),
                PRESS_AT..again_at + 4,
                taps(down, vec![PRESS_AT, again_at]),
                |number| emptied(number) && number < loaded_at,
            );
            assert_eq!(first_sending(&seen, shot), Some(PRESS_AT), "{device}");
            for step in &gun.alt_fire_empty_after {
                assert_eq!(
                    sending_from(&seen, step.slots, PRESS_AT + 1),
                    Some(state_after(PRESS_AT, step.after_ms)),
                    "{device} aimed: {step:?}"
                );
            }
            let hold_again = seen
                .iter()
                .find(|(number, slots)| {
                    *number > aimed_again && slots.second.sequence == Some(hold.stand.hold)
                })
                .map(|(number, _)| *number);
            assert!(
                hold_again.is_some_and(|number| number < loaded_at),
                "{device} aimed: the aim's hold again at {hold_again:?}"
            );
            assert_eq!(
                sending_from(&seen, shot, PRESS_AT + 1),
                Some(again_at),
                "{device} aimed: the first press after the load shoots"
            );
        }
    }

    /// The first-slot state and the third slot of the own states numbered in `numbers`, with
    /// `intent` and no key, of `tags` drawn.
    fn legs_and_third(
        track: &mut SlayerTrack,
        table: &PlayerSequences,
        tags: &str,
        intent: Intent,
        numbers: std::ops::Range<u16>,
    ) -> Vec<(u16, Option<u64>, SlotChange)> {
        numbers
            .map(|number| {
                track.record(keyed(number, intent, HELD, tags), table);
                let due = own_state(track, number);
                let first = due.state.and_then(|state| state.state);
                let third = due.slots.map(|slots| slots.third).unwrap_or_default();
                (number, first, third)
            })
            .collect()
    }

    /// Retail (ranged shot study 2026-11-19, 1,195 ledgers): the aim's third slot plays the
    /// stand's sequence with the aim's stand (bow 422, musket 406, blunderbuss 408) and the
    /// walk's with its walk (421, 405, 407); a change of legs sends the other one alone in the
    /// first slot's own state.
    #[test]
    fn an_aim_plays_its_third_slot_by_legs_and_a_leg_change_sends_the_other() {
        let table = crate::combat::test_support::test_player_sequences();
        let walk = Intent {
            forward: 1,
            ..aim()
        };
        for tags in [BOW, MUSKET, BLUNDERBUSS] {
            let stance = table.stance(stance_key(tags)).expect("the stance");
            let hold = stance.alt_hold.expect("the aim's hold");
            let (walks, stands) = (
                hold.walk.third.expect("walk"),
                hold.stand.third.expect("stand"),
            );
            let moves = stance.alt_moves.expect("the aim's moves");
            let walk_state = moves.moving.expect("walk").state;
            let stand_state = moves.idle.expect("stand").state;
            let mut track = settled(&table, tags, false, SETTLE_STATES);
            let start = legs_and_third(&mut track, &table, tags, aim(), 40..50);
            let mut thirds: Vec<u64> = start
                .iter()
                .filter_map(|(_, _, third)| third.sequence)
                .collect();
            thirds.dedup();
            assert_eq!(thirds, [stands], "{tags}: a standing aim");
            for (intent, state, third, legs) in [
                (walk, walk_state, walks, "walk"),
                (aim(), stand_state, stands, "stand"),
            ] {
                let seen = legs_and_third(&mut track, &table, tags, intent, 50..60);
                let changed = seen
                    .iter()
                    .find(|(_, _, change)| change.sequence.is_some())
                    .expect("a third-slot change");
                assert_eq!(changed.1, Some(state), "{tags} {legs}: with the first slot");
                assert_eq!(
                    changed.2,
                    SlotChange {
                        sequence: Some(third),
                        ..SlotChange::default()
                    },
                    "{tags} {legs}: the sequence alone"
                );
            }
            let mut walking = settled(&table, tags, false, SETTLE_STATES);
            let start = legs_and_third(&mut walking, &table, tags, walk, 40..50);
            let mut thirds: Vec<u64> = start
                .iter()
                .filter_map(|(_, _, third)| third.sequence)
                .collect();
            thirds.dedup();
            assert_eq!(thirds, [walks], "{tags}: a walking aim");
        }
    }

    /// Retail bow: the step after a drawn shot sends the aim's third slot of the legs it is
    /// on, 422 standing (1,292 shots) and 421 walking (2,544).
    #[test]
    fn a_drawn_bow_shot_step_sends_the_third_slot_of_its_legs() {
        const PRESS_AT: u16 = 80;
        const RELEASE_AT: u16 = 110;
        let table = crate::combat::test_support::test_player_sequences();
        let bow = table.stance(stance_key(BOW)).expect("bow");
        let hold = bow.alt_hold.expect("the aim's hold");
        let charge = bow.alt_charge.as_ref().expect("the full draw");
        let step = charge
            .fire_after
            .iter()
            .find(|step| step.slots.third_sequence.is_some())
            .expect("the third-slot step");
        let at = state_after(RELEASE_AT, step.after_ms);
        for (device, down, held) in [
            ("keyboard", PRESS, HELD),
            ("controller", PAD_HOLD, PAD_HOLD),
        ] {
            let mut track = settled(&table, BOW, true, PRESS_AT);
            let key = |number: u16| match number {
                PRESS_AT => down,
                RELEASE_AT => RELEASE,
                number if number < RELEASE_AT => held,
                _ => HELD,
            };
            let seen = shots(&mut track, &table, BOW, aim(), PRESS_AT..at + 2, key);
            let third = seen
                .iter()
                .find(|(number, _)| *number == at)
                .and_then(|(_, slots)| slots.third.sequence);
            assert_eq!(third, hold.stand.third, "{device}: standing");
        }
    }

    /// Retail bow (ranged shot study 2026-11-19): a release under 1,000 ms after the shot
    /// before sent its shot 1,066 ms after that shot (159 own cases), and the shot's steps
    /// counted from there.
    #[test]
    fn a_bow_release_before_its_shot_gap_shoots_when_the_gap_has_passed() {
        const PRESS_AT: u16 = 80;
        const AGAIN_AT: u16 = PRESS_AT + 10;
        let table = crate::combat::test_support::test_player_sequences();
        let bow = table.stance(stance_key(BOW)).expect("bow");
        let gap_ms = bow.alt_fire_gap_ms.expect("the gap");
        let shot = bow.alt_fire.expect("the plain shot");
        let next_draw = *bow.alt_fire_after.get(1).expect("the next draw");
        let first = PRESS_AT + TAP_RECORDS;
        let held_until = state_after(first, gap_ms);
        for (device, down) in DEVICES {
            let mut track = settled(&table, BOW, true, PRESS_AT);
            let presses = taps(down, vec![PRESS_AT, AGAIN_AT]);
            let end = state_after(held_until, next_draw.after_ms) + 2;
            let seen = shots(&mut track, &table, BOW, aim(), PRESS_AT..end, presses);
            let fired: Vec<u16> = seen
                .iter()
                .filter(|(_, slots)| sends(slots, shot))
                .map(|(number, _)| *number)
                .collect();
            assert_eq!(fired, [first, held_until], "{device}");
            assert_eq!(
                sending_from(&seen, next_draw.slots, held_until),
                Some(state_after(held_until, next_draw.after_ms)),
                "{device}: the held shot's steps count from it"
            );
        }
    }

    /// The bow key of `down` (a keyboard's press or a controller's held trigger) pressed in
    /// record `press_at` and held to its release in record `release_at`.
    fn held_until(
        down: AttackKeyBits,
        press_at: u16,
        release_at: u16,
    ) -> impl Fn(u16) -> AttackKeyBits {
        let hold = if down == PAD_HOLD { PAD_HOLD } else { HELD };
        move |number| match number {
            number if number == press_at => down,
            number if number == release_at => RELEASE,
            number if number > press_at && number < release_at => hold,
            _ => HELD,
        }
    }

    /// The own state that sends the bow's full draw `48=24` for a key of `down` pressed in record
    /// `press_at` of an aim standing still.
    fn full_draw_at(table: &PlayerSequences, down: AttackKeyBits, press_at: u16) -> u16 {
        let charge = table
            .stance(stance_key(BOW))
            .and_then(|bow| bow.alt_charge.as_ref())
            .expect("the full draw");
        let mut track = settled(table, BOW, true, press_at);
        let key = held_until(down, press_at, u16::MAX);
        let seen = shots(&mut track, table, BOW, aim(), press_at..press_at + 40, key);
        first_sending(&seen, charge.drawn).expect("the full draw")
    }

    /// The upper slots own state `number` of `seen` sends, when it sends any.
    fn slots_at(seen: &[(u16, UpperSlots)], number: u16) -> Option<UpperSlots> {
        seen.iter()
            .find(|(at, _)| *at == number)
            .map(|(_, slots)| *slots)
    }

    /// Retail bow (bow full draw study 2026-11-19): `18=928` alone 133 ms after the full draw
    /// `48=24` (own 991, others 1,675), with `21=3077` standing; a change to walking legs sent
    /// `21=4275` in its own state (320 of 322 own); the drawn shot after it sent no third slot
    /// (807 of 930 own); its step `48=41` sent the aim's of the legs (`4275` to `421`, 1,077).
    #[test]
    fn a_bow_full_draw_holds_with_the_third_slot_of_its_legs_with_keyboard_and_controller() {
        const PRESS_AT: u16 = 80;
        let table = crate::combat::test_support::test_player_sequences();
        let bow = table.stance(stance_key(BOW)).expect("bow");
        let charge = bow.alt_charge.as_ref().expect("the full draw");
        let hold = charge.hold.as_ref().expect("the full draw's hold");
        let aim_walk = bow
            .alt_hold
            .and_then(|legs| legs.walk.third)
            .expect("the aim's walk");
        let step = charge
            .fire_after
            .iter()
            .find(|step| step.slots.third_sequence.is_some())
            .expect("the third-slot step");
        let walk = Intent {
            forward: 1,
            ..aim()
        };
        let third_alone = |sequence| SlotChange {
            sequence: Some(sequence),
            ..SlotChange::default()
        };
        for (device, down) in DEVICES {
            let drawn = full_draw_at(&table, down, PRESS_AT);
            let held_at = state_after(drawn, hold.after_ms);
            let walk_at = held_at + 5;
            let release_at = walk_at + 5;
            let step_at = state_after(release_at, step.after_ms);
            let key = held_until(down, PRESS_AT, release_at);
            let mut track = settled(&table, BOW, true, PRESS_AT);
            let mut seen = shots(&mut track, &table, BOW, aim(), PRESS_AT..walk_at, &key);
            seen.extend(shots(
                &mut track,
                &table,
                BOW,
                walk,
                walk_at..step_at + 2,
                &key,
            ));
            let held = slots_at(&seen, held_at).expect("the hold");
            assert_eq!(
                (held.second, held.third),
                (
                    SlotChange {
                        sequence: Some(hold.sequence),
                        ..SlotChange::default()
                    },
                    third_alone(hold.stand_third)
                ),
                "{device}: the hold alone with the stand's third slot"
            );
            assert_eq!(
                first_sending(
                    &seen,
                    SlotsChange {
                        second_sequence: Some(hold.sequence),
                        ..SlotsChange::default()
                    }
                ),
                Some(held_at),
                "{device}: {} ms after the full draw",
                hold.after_ms
            );
            let walked = slots_at(&seen, walk_at).expect("the leg change");
            assert_eq!(
                walked.third,
                third_alone(hold.walk_third),
                "{device}: walking"
            );
            let shot = slots_at(&seen, release_at).expect("the drawn shot");
            assert!(sends(&shot, charge.fire), "{device}: the drawn shot");
            assert_eq!(
                shot.third,
                SlotChange::default(),
                "{device}: no third slot again"
            );
            let stepped = slots_at(&seen, step_at).expect("the shot's step");
            assert_eq!(
                stepped.third.sequence,
                Some(aim_walk),
                "{device}: the aim's walk"
            );
        }
    }

    /// Retail bow: a release after the full draw `48=24` and before `18=928` sent the drawn shot
    /// with the third slot of the legs (own 462 of 464, others 934 of 946), and no hold.
    #[test]
    fn a_bow_released_before_its_full_draw_hold_shoots_with_the_hold_s_third_slot() {
        const PRESS_AT: u16 = 80;
        let table = crate::combat::test_support::test_player_sequences();
        let bow = table.stance(stance_key(BOW)).expect("bow");
        let charge = bow.alt_charge.as_ref().expect("the full draw");
        let hold = charge.hold.as_ref().expect("the full draw's hold");
        let aim_stand = bow
            .alt_hold
            .and_then(|legs| legs.stand.third)
            .expect("the aim's stand");
        for (device, down) in DEVICES {
            let drawn = full_draw_at(&table, down, PRESS_AT);
            let release_at = drawn + 2;
            let key = held_until(down, PRESS_AT, release_at);
            let mut track = settled(&table, BOW, true, PRESS_AT);
            let seen = shots(
                &mut track,
                &table,
                BOW,
                aim(),
                PRESS_AT..release_at + 40,
                key,
            );
            let shot = slots_at(&seen, release_at).expect("the drawn shot");
            assert!(sends(&shot, charge.fire), "{device}: the drawn shot");
            assert_eq!(
                shot.third,
                SlotChange {
                    sequence: Some(hold.stand_third),
                    ..SlotChange::default()
                },
                "{device}: the stand's third slot alone"
            );
            assert!(
                seen.iter()
                    .all(|(_, slots)| slots.second.sequence != Some(hold.sequence)),
                "{device}: no hold after the release"
            );
            assert!(
                seen.iter()
                    .any(|(_, slots)| slots.third.sequence == Some(aim_stand)),
                "{device}: the shot's step plays the aim's third slot again"
            );
        }
    }

    /// Retail bow: a full draw held 3,033 ms past its hold sent `18=32` alone (own 47 of 49
    /// such holds), and the release after it still sent the drawn shot (own 38).
    #[test]
    fn a_bow_full_draw_held_long_goes_on_to_its_next_hold_and_still_shoots_drawn() {
        const PRESS_AT: u16 = 80;
        let table = crate::combat::test_support::test_player_sequences();
        let bow = table.stance(stance_key(BOW)).expect("bow");
        let charge = bow.alt_charge.as_ref().expect("the full draw");
        let hold = charge.hold.as_ref().expect("the full draw's hold");
        let next = hold.after.first().expect("the next hold");
        for (device, down) in DEVICES {
            let drawn = full_draw_at(&table, down, PRESS_AT);
            let next_at = state_after(state_after(drawn, hold.after_ms), next.after_ms);
            let release_at = next_at + 3;
            let key = held_until(down, PRESS_AT, release_at);
            let mut track = settled(&table, BOW, true, PRESS_AT);
            let seen = shots(
                &mut track,
                &table,
                BOW,
                aim(),
                PRESS_AT..release_at + 2,
                key,
            );
            assert_eq!(
                slots_at(&seen, next_at).map(|slots| slots.second),
                Some(SlotChange {
                    sequence: next.slots.second_sequence,
                    ..SlotChange::default()
                }),
                "{device}: the next hold alone"
            );
            assert!(
                slots_at(&seen, release_at).is_some_and(|slots| sends(&slots, charge.fire)),
                "{device}: the drawn shot"
            );
        }
    }
}
