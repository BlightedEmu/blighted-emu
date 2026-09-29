// SPDX-License-Identifier: AGPL-3.0-or-later
//! `MB::ALCReplicatedState` (type 11) on the bundle path (movement message note, "Type 11 pass";
//! instance virtual `+0x90`, `FUN_142a436b0`).
//!
//! The body starts with one byte whose bit 0 and bit 1 select two slots; the client tests no
//! other bit. A selected slot has a wide prefix integer whose low bits are child presence bits,
//! one per child in constructor order; the client tests only the low 54 bits, and only bits of
//! children that exist matter. A set bit reads that child; a clear bit reads nothing.
//!
//! Only `pos10` (slot 0, child 27) is kept as a value. The rest is skipped, and the body keeps
//! its exact bytes, so a read and write gives the same bytes.

use std::ops::Range;

use crate::client_input::{InputPosition, SlayerInput};
use crate::wire::{MessageError, Reader, Writer};

/// Type index of the class.
pub const ALC_TYPE_INDEX: u64 = 11;
/// Slot 0 child that holds the position (`pos10`), counted from 0.
pub const POS10_CHILD: usize = 27;
/// Slot 0 children of a state's number and clock (sailor-movement note, section 1): `idRel` and
/// `timeOffsetRel` (the low bytes), `idAbs` and `timeOffsetAbs` (the high bytes).
const ID_REL_CHILD: usize = 0;
const TIME_REL_CHILD: usize = 1;
const ID_ABS_CHILD: usize = 25;
const TIME_ABS_CHILD: usize = 26;
/// Slot 0 child of the facing (`rotation`, a quat).
const ROTATION_CHILD: usize = 11;
/// One quat byte: `byte * QUAT_STEP - QUAT_OFFSET` (`FUN_14087a8d0`: sqrt(2) / 255 and
/// 1 / sqrt(2); rotation note, section 1).
const QUAT_STEP: f32 = 0.005_545_935_6;
const QUAT_OFFSET: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// Quat tags of a turn about the up axis (x and y zero): the w part derived from the z byte,
/// the z part derived from the w byte, and yaw 0 (z zero too, no byte).
const QUAT_TAG_W_DERIVED: u8 = 0x66;
const QUAT_TAG_Z_DERIVED: u8 = 0x46;
const QUAT_TAG_NORTH: u8 = 0x6e;
/// Set in a quat tag when the derived part is negative.
const QUAT_TAG_NEGATIVE: u8 = 1;
/// The parts of a quat (x, y, z, w).
const QUAT_PARTS: usize = 4;
/// The bit of a quat tag where the index of the derived part starts.
const QUAT_DERIVED_SHIFT: u8 = 5;
/// Slot 0 child of the offset from `pos10` (`worldPosRel`).
pub const POS_REL_CHILD: usize = 10;
/// Slot 0 child of where a character looks (`lookDir`, a quat).
const LOOK_DIR_CHILD: usize = 12;
/// Slot 0 children of a character's first slayer state slot: `slayerStateId` (prefix),
/// `slayerStateIdStarted` (`u16`: the ALC state number in which it started; retail remote duel
/// `14=28ad` with `idRel` `ad`) and `slayerSequenceId` (prefix) (replication note).
const SLAYER_STATE_CHILD: usize = 13;
const SLAYER_STARTED_CHILD: usize = 14;
const SLAYER_SEQUENCE_CHILD: usize = 15;
/// Slot 0 children of the second slayer slot: its state, the state number in which it started and
/// its sequence (slayer note: the client's row 1 second state and sequence).
const SECOND_SLAYER_STATE_CHILD: usize = 16;
const SECOND_SLAYER_STARTED_CHILD: usize = 17;
const SECOND_SLAYER_SEQUENCE_CHILD: usize = 18;
/// The second slot's state of a slot whose action stops (child 16 = 0).
const SECOND_SLOT_STOPPED: u64 = 0;
/// Slot 0 children of the time into the first slayer slot's sequence: `slayerSeqTimeRel`, one
/// byte of [`SEQUENCE_TIME_STEP_S`] steps from [`SEQUENCE_TIME_REL_ZERO`], and
/// `slayerSeqTimeAbs`, a `half` in seconds that the steps count from ([`SequenceClock`]).
const SEQUENCE_TIME_REL_CHILD: usize = 2;
const SEQUENCE_TIME_ABS_CHILD: usize = 3;
/// One `slayerSeqTimeRel` step: 4/255 s (Observed: in the retail newchar capture, the own
/// sequence time `abs + (rel - 0x7f) × step` is the sum of the input records' frame times since
/// the record that started the slayer state, that record's own included, with this step: exact
/// in 10,653 and one step off in 1,309 of the 11,962 states whose start the scan placed; the
/// tutorial's 20 Hz records step it by 3 or 4, the 30 Hz ones by 2 or 3).
pub const SEQUENCE_TIME_STEP_S: f32 = 4.0 / 255.0;
/// The `slayerSeqTimeRel` byte of no step: every state that sends a new `slayerSeqTimeAbs`
/// carries it (Observed: 228 of 228 new absolute parts of the newchar capture).
pub const SEQUENCE_TIME_REL_ZERO: u8 = 0x7f;
/// The most steps the byte counts from its absolute part: one more and the state sends a new
/// absolute part (Observed: each new absolute part of a running sequence came when the steps
/// would have been 128 to 133, every 2.0 to 2.1 s).
const SEQUENCE_TIME_MAX_STEPS: f32 = 127.0;
/// Child 16 carries the client's second state plus one, so "none" (`0xffff`) is 0 (Observed:
/// row 1 `0x0024` came back as `25`, in all 7 retail rows with a second sequence).
const SECOND_STATE_WIRE_OFFSET: u16 = 1;
/// Slot 0 children of the FullBody scope time in 1/30 s ticks, low byte first
/// (`scopeTimeBlob0Data0` to `Data3`; mob-animation note, section 3.3).
const SCOPE_TIME_CHILDREN: [usize; 4] = [28, 38, 49, 50];
/// Slot 0 child of the times of the scopes above FullBody (`scopeTimeBlobEx`).
const SCOPE_TIME_EX_CHILD: usize = 29;
/// Slot 0 child of the mask of the active scopes (`scopeData`).
const SCOPE_DATA_CHILD: usize = 32;
/// Slot 0 child of the scope entries (`scopeInfoBlob`).
const SCOPE_INFO_CHILD: usize = 33;
/// Slot 0 child of the Mannequin tag state (`globalFragTags`).
const FRAG_TAGS_CHILD: usize = 34;
/// Slot 0 child of the angle from a mob's facing to the facing it wants
/// (`aiAngleToDesiredFacing`, radians; creature-animation note, section 7).
const FACING_GAP_CHILD: usize = 35;
/// Slot 0 child of the teleport number (`teleportAndMigrationId`). The client puts its own
/// player at a server position when it changes: the first own state sends 1, and retail moves the
/// player into the water with a state that sends 2 (live capture, 118580 ms).
const TELEPORT_CHILD: usize = 31;
/// Slot 0 child of the time anchor (`timeAnchor`, µs since 2000-01-01; mob-animation note,
/// section 1): a state's time is the anchor plus its offset in children 1 and 26.
const TIME_ANCHOR_CHILD: usize = 40;
/// Slot 0 child of the movement flags (`dataBits`). Bit 5 asks the client to throttle its input
/// to 20 Hz (clear: 30 Hz); the client sends 998 `SetInputsThrottled` when the bit changes
/// (network note: all 27 retail toggles followed the server's value).
const DATA_BITS_CHILD: usize = 41;
/// Slot 1 children of the height of the feet over the floor (`distGround`, a byte over 0 to
/// 10 m) and of the depth of the water at them (`waterDepth`) (ALC constructor `FUN_142a36d50`:
/// the first two names of the slot-1 list; run-15 movement study).
const DIST_GROUND_CHILD: usize = 0;
const WATER_DEPTH_CHILD: usize = 1;
/// `dataBits` of a retail own state without the throttle bit (40583 ms, then 998 `00`).
pub const DATA_BITS_ACTIVE: u8 = 211;
/// A `worldPosRel` that puts the entity at its `pos10`.
const AT_ANCHOR: [u8; POS3_LEN] = [0xff; POS3_LEN];
/// The `worldPosRel` byte of no step on an axis.
const POS_REL_ZERO: u8 = 0x7f;
/// The most `worldPosRel` steps from `pos10` on one axis, either way.
const POS_REL_MAX_STEPS: f32 = 127.0;
/// The span of the `worldPosRel` steps of one axis, in metres: 255 steps over 4 m.
const POS_REL_SPAN_M: f32 = 4.0;
/// The `worldPosRel` steps of [`POS_REL_SPAN_M`].
const POS_REL_SPAN_STEPS: f32 = 255.0;
/// One `worldPosRel` step, in metres: 4/255 m (Observed fit, movement study 2026-09-20: at each
/// new `pos10` after a steady walk, the old `pos10` plus the last offset plus one state's step
/// lands on the new `pos10` with a step of median 0.015695 m over 889 anchor changes of the
/// newchar capture and 0.01566 to 0.01572 m in each of the ten clean run and walk captures; with
/// the 2.1/127 m read before, every offset overshot and each new `pos10` pulled the own player
/// back by 0.08 to 0.14 m).
pub const POS_REL_STEP_M: f32 = POS_REL_SPAN_M / POS_REL_SPAN_STEPS;
/// `scopeData` with only the FullBody scope (bit 0).
const FULL_BODY_SCOPE: u64 = 1;
/// The playback speed of a scope entry that plays at its own speed.
const DEFAULT_SPEED: u16 = 0;
/// Bytes of the Mannequin tag state (child 34).
pub const FRAG_TAGS_LEN: usize = FRAG_LEN;

/// One state of a server move of the client's own player. Retail sent three states in a row
/// (118546, 118580, 118614 ms): the new place with the anchor offset and the facing, the place
/// again at zero offset with the new teleport number, then zero offset with the number alone. A
/// single state with the number and the anchor offset left the live client moving on with its
/// old speed (2026-09-14: thrown 18 m east and 3 m up after the water move).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeleportStep {
    /// The new place: `worldPosRel` at the anchor, the facing and `pos10`.
    Place,
    /// The place again at zero offset, the facing and the teleport number.
    Confirm,
    /// Zero offset and the teleport number.
    Hold,
}

impl TeleportStep {
    /// The step after this one; `None` after the last.
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Place => Some(Self::Confirm),
            Self::Confirm => Some(Self::Hold),
            Self::Hold => None,
        }
    }
}

/// The number and time of one state of an entity: the client places the states of an entity on
/// its timeline by them, at `anchor_us + offset_ms × 1000`, and blends between the two states
/// around its render time (mob-animation note, section 1; retail never sends a state without
/// `idRel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlcStamp {
    /// The state number; +1 per new state.
    pub id: u16,
    /// The time anchor: server microseconds since 2000-01-01.
    pub anchor_us: u64,
    /// The state's time after the anchor, in milliseconds.
    pub offset_ms: u16,
}

impl AlcStamp {
    fn low(value: u16) -> u8 {
        value.to_be_bytes()[1]
    }

    fn high(value: u16) -> u8 {
        value.to_be_bytes()[0]
    }
}

/// One FullBody scope entry: a Mannequin fragment and a CAGE action that started at state
/// `start_id` (mob-animation note, section 3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeEntry {
    /// The number of the state in which the entry started.
    pub start_id: u16,
    /// The fragment's index + 1 in the creature's Mannequin controller definitions.
    pub fragment: u16,
    /// The action's index + 1 in the creature's CAGE grid.
    pub action: u16,
    /// The last byte of a retail entry of this fragment (**Unknown** meaning).
    pub tail: u8,
}

impl ScopeEntry {
    /// Writes the entry: the start, the fragment, the action, the speed and the tail.
    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.u16(self.start_id);
        writer.prefixed(u64::from(self.fragment))?;
        writer.prefixed(u64::from(self.action))?;
        writer.u16(DEFAULT_SPEED).u8(self.tail);
        Ok(())
    }
}

/// The `scopeInfoBlob` of `entries`, in scope order: their byte count, then the entries.
fn info_blob(entries: &[ScopeEntry]) -> Result<Vec<u8>, MessageError> {
    let mut body = Writer::new();
    for entry in entries {
        entry.write(&mut body)?;
    }
    let body = body.finish();
    let mut blob = Writer::new();
    blob.prefixed(body.len() as u64)?;
    blob.bytes(&body);
    Ok(blob.finish())
}

/// The `scopeTimeBlobEx` of the scopes above FullBody: their byte count, then each scope's time
/// in seconds as a big-endian `half` (retail look pose: `02 3556`).
fn times_ex_blob(seconds: Option<f32>) -> Result<Vec<u8>, MessageError> {
    let Some(seconds) = seconds else {
        return Ok(EMPTY_BLOB.to_vec());
    };
    let mut blob = Writer::new();
    blob.prefixed(HALF_LEN)?;
    blob.u16(half_bits(seconds));
    Ok(blob.finish())
}

/// Bytes of a `half`.
const HALF_LEN: u64 = 2;
/// `half` layout: sign, 5 exponent bits with bias 15, 10 mantissa bits.
const HALF_SIGN: u32 = 0x8000;
const HALF_INFINITY: u32 = 0x7c00;
const HALF_EXPONENT_BIAS: i32 = 15;
const HALF_MAX_EXPONENT: i32 = 0x1f;
const HALF_MANTISSA_BITS: u32 = 10;
/// `f32` layout: 8 exponent bits with bias 127, 23 mantissa bits.
const F32_EXPONENT_BIAS: i32 = 127;
const F32_EXPONENT_MASK: u32 = 0xff;
const F32_MANTISSA_BITS: u32 = 23;
const F32_MANTISSA_MASK: u32 = (1 << F32_MANTISSA_BITS) - 1;
/// The mantissa bits that `half` drops from an `f32`.
const DROPPED_BITS: u32 = F32_MANTISSA_BITS - HALF_MANTISSA_BITS;

/// The `half` bits of `value`, rounded to the nearest (ties to even); a value too large for a
/// `half` becomes infinity, and a value not a number becomes infinity too (never sent).
pub(crate) fn half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = (bits >> u16::BITS) & HALF_SIGN;
    // The exponent field has 8 bits.
    let exponent = ((bits >> F32_MANTISSA_BITS) & F32_EXPONENT_MASK) as i32 - F32_EXPONENT_BIAS
        + HALF_EXPONENT_BIAS;
    let mantissa = bits & F32_MANTISSA_MASK;
    let round = |value: u32, dropped: u32| {
        let kept = value >> dropped;
        let rest = value & ((1 << dropped) - 1);
        let half_way = 1 << (dropped - 1);
        kept + u32::from(rest > half_way || (rest == half_way && kept & 1 == 1))
    };
    let magnitude = if exponent >= HALF_MAX_EXPONENT {
        HALF_INFINITY
    } else if exponent > 0 {
        // A carry out of the mantissa steps the exponent, as it should.
        // `exponent` is below 31 here.
        round(
            (exponent as u32) << F32_MANTISSA_BITS | mantissa,
            DROPPED_BITS,
        )
        .min(HALF_INFINITY)
    } else if exponent > -(HALF_MANTISSA_BITS as i32) {
        // A subnormal `half`: the hidden bit joins the mantissa. `exponent` is -9 to 0.
        round(
            mantissa | 1 << F32_MANTISSA_BITS,
            DROPPED_BITS + (1 - exponent) as u32,
        )
    } else {
        0
    };
    // Sign and magnitude fit 16 bits.
    (sign | magnitude) as u16
}

/// The value of the `half` bits `bits`.
pub(crate) fn half_value(bits: u16) -> f32 {
    let bits = u32::from(bits);
    let sign = if bits & HALF_SIGN == 0 { 1.0 } else { -1.0 };
    let exponent = (bits & HALF_INFINITY) >> HALF_MANTISSA_BITS;
    let mantissa = bits & ((1 << HALF_MANTISSA_BITS) - 1);
    // A half's exponent field and mantissa fit an f32 exactly.
    #[allow(clippy::cast_precision_loss, reason = "10 mantissa bits")]
    let fraction = mantissa as f32 / (1 << HALF_MANTISSA_BITS) as f32;
    // The exponent field has 5 bits.
    let magnitude = match exponent {
        0 => fraction * 2.0_f32.powi(1 - HALF_EXPONENT_BIAS),
        exponent if exponent == HALF_MAX_EXPONENT as u32 => f32::INFINITY,
        exponent => (1.0 + fraction) * 2.0_f32.powi(exponent as i32 - HALF_EXPONENT_BIAS),
    };
    sign * magnitude
}

/// A scope above FullBody that plays with it (the look pose): its scope bit, its entry and its
/// time in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerEntry {
    /// Its bit in `scopeData`.
    pub scope_bit: u8,
    /// Its entry.
    pub entry: ScopeEntry,
    /// Its time, in seconds.
    pub seconds: f32,
}

/// What a mob shows: one FullBody scope entry, the time since it started, its tags, a look
/// pose above it, and the angle it still has to turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobAnimation {
    /// The entry.
    pub entry: ScopeEntry,
    /// The time in the entry, in 1/30 s ticks.
    pub ticks: u32,
    /// The Mannequin tag state.
    pub tags: [u8; FRAG_TAGS_LEN],
    /// The look pose that plays with the entry, when one does.
    pub look_pose: Option<LayerEntry>,
    /// The angle from its facing to the facing it wants (radians), when it goes with the state.
    pub facing_gap: Option<f32>,
}

impl MobAnimation {
    /// The scope children of this animation: times, the times of the look pose, the mask of the
    /// scopes, the entries, the tags and the angle to turn.
    fn children(&self) -> Result<Vec<(usize, Vec<u8>)>, MessageError> {
        let mut children = scope_time_children(self.ticks, true);
        let mut entries = vec![self.entry];
        let mut scopes = FULL_BODY_SCOPE;
        if let Some(layer) = self.look_pose {
            entries.push(layer.entry);
            scopes |= 1 << layer.scope_bit;
        }
        children.extend([
            (
                SCOPE_TIME_EX_CHILD,
                times_ex_blob(self.look_pose.map(|layer| layer.seconds))?,
            ),
            (SCOPE_DATA_CHILD, prefixed_bytes(scopes)?),
            (SCOPE_INFO_CHILD, info_blob(&entries)?),
            (FRAG_TAGS_CHILD, self.tags.to_vec()),
        ]);
        if let Some(gap) = self.facing_gap {
            children.push((FACING_GAP_CHILD, gap.to_be_bytes().to_vec()));
        }
        Ok(children)
    }
}

/// The scope part of a later mob state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepScope {
    /// The animation goes on.
    Ticks {
        /// Its time in 1/30 s ticks.
        ticks: u32,
        /// The bytes above the low byte changed since the last state, and go with it.
        high_changed: bool,
        /// The time of its look pose in seconds, when one plays.
        look_pose_seconds: Option<f32>,
    },
    /// A new animation starts.
    Changed(MobAnimation),
}

/// Where a state puts an entity that moved: the client keeps the last `pos10` it got for the
/// entity and puts it at that place plus the state's `worldPosRel` offset ("Absolute portion in
/// delta compression"; own-player-position note). Retail moves a walking mob by offsets and sends
/// a new `pos10` only when the offset no longer fits: 14 of the first legionnaire's 959 groups
/// carried a new `pos10` in the newchar capture. Ours sent a new `pos10` with almost every move
/// (3,516 of 4,804 groups), and the client dropped its lock on the moving mob within 2 s
/// (lock-on note, run 9; Inferred cause).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepPlace {
    /// A new `pos10` at this place, at zero offset.
    Anchor(InputPosition),
    /// An offset from the `pos10` the client has.
    Offset([u8; POS3_LEN]),
    /// The `pos10` the state before sent, again, with an offset from it (retail own states:
    /// the state after each new own `pos10` carries it again, movement study 2026-09-20: states
    /// 38, 62, 228 and 247 of the clean walking capture, each after a new `pos10` at zero offset).
    AnchorAgain {
        /// The `pos10`.
        anchor: InputPosition,
        /// The offset from it.
        offset: [u8; POS3_LEN],
    },
}

impl StepPlace {
    /// The place `at` for a client whose `pos10` for the entity is `anchor`: an offset when it is
    /// within reach of the anchor on every axis, else a new anchor.
    pub fn of(anchor: Option<InputPosition>, at: InputPosition) -> Self {
        anchor
            .and_then(|anchor| offset_from(anchor, at))
            .map_or(Self::Anchor(at), Self::Offset)
    }

    /// Its children: `worldPosRel` and, with an anchor, `pos10`.
    fn children(self) -> Vec<(usize, Vec<u8>)> {
        match self {
            Self::Anchor(at) => position_children(at).into(),
            Self::Offset(offset) => vec![(POS_REL_CHILD, offset.to_vec())],
            Self::AnchorAgain { anchor, offset } => offset_children(anchor, offset).into(),
        }
    }

    /// The `pos10` it sends, when it sends one.
    const fn pos10(self) -> Option<InputPosition> {
        match self {
            Self::Anchor(at) | Self::AnchorAgain { anchor: at, .. } => Some(at),
            Self::Offset(_) => None,
        }
    }
}

/// Where a first state (a card) puts an entity at `at`: at the `pos10` `anchor` the entity's
/// later states are offsets from, plus the offset to `at`, so that a client that sees it from now
/// on keeps the same anchor as the others; a new `pos10` at `at` when it is out of reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardPlace {
    /// The `pos10` of the entity's later states, when it has one.
    pub anchor: Option<InputPosition>,
    /// Where it is.
    pub at: InputPosition,
}

impl CardPlace {
    /// Its `pos10` and its offset from it: the anchor and the offset when `at` is within reach,
    /// else `at` with no offset (the entity stands at its `pos10`).
    fn resolved(self) -> (InputPosition, Option<[u8; POS3_LEN]>) {
        self.anchor
            .and_then(|anchor| Some((anchor, Some(offset_from(anchor, self.at)?))))
            .unwrap_or((self.at, None))
    }

    /// Its `worldPosRel` and `pos10` children, and the `pos10`.
    fn children(self) -> (Vec<(usize, Vec<u8>)>, InputPosition) {
        match self.resolved() {
            (anchor, Some(offset)) => (offset_children(anchor, offset).into(), anchor),
            (at, None) => (position_children(at).into(), at),
        }
    }
}

/// The metres of one `worldPosRel` byte from the `pos10`: 0 for the byte that puts the axis at
/// the `pos10`.
pub fn offset_metres(byte: u8) -> f32 {
    if byte == AT_ANCHOR[0] {
        0.0
    } else {
        (f32::from(byte) - f32::from(POS_REL_ZERO)) * POS_REL_STEP_M
    }
}

/// The `worldPosRel` that puts an entity at `at` from the `pos10` `anchor`; `None` when an axis
/// is more than [`POS_REL_MAX_STEPS`] steps away.
pub fn offset_from(anchor: InputPosition, at: InputPosition) -> Option<[u8; POS3_LEN]> {
    let step = |from: f32, to: f32| {
        let steps = ((to - from) / POS_REL_STEP_M).round();
        // Within ±127 steps, so the byte is 0 to 254 and never 255 (the anchor marker).
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked to ±127 steps"
        )]
        (steps.abs() <= POS_REL_MAX_STEPS).then(|| (f32::from(POS_REL_ZERO) + steps) as u8)
    };
    Some([
        step(anchor.x, at.x)?,
        step(anchor.y, at.y)?,
        step(anchor.z(), at.z())?,
    ])
}

/// One later state of a mob (sailor-movement note, section 5; mob-animation note, section 5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobStep {
    /// Its number and time.
    pub stamp: AlcStamp,
    /// The anchor of `stamp` is new and goes with the state.
    pub anchored: bool,
    /// Where the mob is now, when it moved.
    pub position: Option<StepPlace>,
    /// Its new facing (yaw, radians), when it turned.
    pub facing: Option<f32>,
    /// Its animation, for a mob whose animations are known.
    pub scope: Option<StepScope>,
}

/// The slayer state of a character (children 13 to 18): the slayer script state it is in, the
/// ALC state number in which that state started and the sequence of the state, and its second
/// slot (slayer note).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlayerState {
    /// `slayerStateId`; `None` when it goes on unchanged (retail leaves child 13 out then).
    pub state: Option<u64>,
    /// `slayerStateIdStarted`; `None` when only the sequence changes in a state that goes on
    /// (retail leaves child 14 out then: the swim sequences of the newchar capture 2026-09-18,
    /// `15=a52e` alone at 1792695840527).
    pub started: Option<u16>,
    /// `slayerSequenceId`.
    pub sequence: u64,
    /// The second slot, when it goes with the state.
    pub second: Option<SecondSlayerState>,
}

/// The second slayer slot (children 16 to 18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecondSlayerState {
    /// The ALC state number in which it started (child 17; retail sends it with every action the
    /// client starts).
    pub started: u16,
    /// Its state (child 16, on the wire) and sequence (child 18), when it plays one.
    pub playing: Option<(u64, u64)>,
    /// With no `playing`: the slot's action stops, child 16 = 0 without a sequence (retail
    /// newchar 2026-09-12, Thorpe's grab pull: `13=04 14=3163 15=8a37 16=00 17=3163`).
    pub stopped: bool,
}

impl SlayerState {
    /// The slayer state of the action `input` the client started, in ALC state `started`. The
    /// state (child 13) goes only with an action of the second slot: in the retail newchar capture
    /// all 7 actions with a second sequence carried 13, 16 and 18, and none of the 78 without one
    /// carried 13, even when the client's state changed (206408 ms: state 10 after 27). An echoed
    /// 13 = 10 left the client in its attack state, and it then refused to start the Thorpe
    /// cutscene (live 2026-09-18, Inferred).
    pub fn of_input(input: SlayerInput, started: u16) -> Self {
        let second_plays = input.second_sequence != 0;
        Self {
            state: second_plays.then_some(u64::from(input.state)),
            started: Some(started),
            sequence: input.sequence,
            second: Some(SecondSlayerState {
                started,
                playing: second_plays.then(|| {
                    (
                        u64::from(input.second_state.wrapping_add(SECOND_STATE_WIRE_OFFSET)),
                        input.second_sequence,
                    )
                }),
                stopped: false,
            }),
        }
    }

    fn children(self) -> Result<Vec<(usize, Vec<u8>)>, MessageError> {
        let mut children = vec![(SLAYER_SEQUENCE_CHILD, prefixed_bytes(self.sequence)?)];
        if let Some(started) = self.started {
            children.push((SLAYER_STARTED_CHILD, started.to_be_bytes().to_vec()));
        }
        if let Some(state) = self.state {
            children.push((SLAYER_STATE_CHILD, prefixed_bytes(state)?));
        }
        if let Some(second) = self.second {
            children.push((
                SECOND_SLAYER_STARTED_CHILD,
                second.started.to_be_bytes().to_vec(),
            ));
            if let Some((state, sequence)) = second.playing {
                children.push((SECOND_SLAYER_STATE_CHILD, prefixed_bytes(state)?));
                children.push((SECOND_SLAYER_SEQUENCE_CHILD, prefixed_bytes(sequence)?));
            } else if second.stopped {
                children.push((
                    SECOND_SLAYER_STATE_CHILD,
                    prefixed_bytes(SECOND_SLOT_STOPPED)?,
                ));
            }
        }
        Ok(children)
    }
}

/// Slot 0 children of the third and fourth slayer slots (state, the state number in which it
/// started, sequence) and of the slayer marker (run-15 aim study, retail newchar 2026-09-18: the
/// blunderbuss aim `19=2e 20=54ad 21=9706 48=28` in the state after the right-click record
/// `54ad`, and `19=00 20=54b0 21=00 48=00` after its release; the block's fourth slot
/// `22=20 24=b533`).
const THIRD_SLAYER_STATE_CHILD: usize = 19;
const THIRD_SLAYER_STARTED_CHILD: usize = 20;
const THIRD_SLAYER_SEQUENCE_CHILD: usize = 21;
const FOURTH_SLAYER_STATE_CHILD: usize = 22;
const FOURTH_SLAYER_STARTED_CHILD: usize = 23;
const FOURTH_SLAYER_SEQUENCE_CHILD: usize = 24;
const SLAYER_MARKER_CHILD: usize = 48;

/// A change of one slayer slot past the first: any of its state, start and sequence children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotChange {
    /// Its state (the second slot's as on the wire).
    pub state: Option<u64>,
    /// The ALC state number in which it starts.
    pub started: Option<u16>,
    /// Its sequence.
    pub sequence: Option<u64>,
}

impl SlotChange {
    fn children(
        self,
        [state_child, started_child, sequence_child]: [usize; 3],
    ) -> Result<Vec<(usize, Vec<u8>)>, MessageError> {
        let mut children = Vec::new();
        if let Some(state) = self.state {
            children.push((state_child, prefixed_bytes(state)?));
        }
        if let Some(started) = self.started {
            children.push((started_child, started.to_be_bytes().to_vec()));
        }
        if let Some(sequence) = self.sequence {
            children.push((sequence_child, prefixed_bytes(sequence)?));
        }
        Ok(children)
    }
}

/// The slayer slots past the first that one own state changes, and the slayer marker (child 48):
/// the server's block, aim and draw (run-15 aim study).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UpperSlots {
    /// The second slot (children 16 to 18).
    pub second: SlotChange,
    /// The third slot (children 19 to 21).
    pub third: SlotChange,
    /// The fourth slot (children 22 to 24).
    pub fourth: SlotChange,
    /// The slayer marker.
    pub marker: Option<u64>,
}

impl UpperSlots {
    fn children(self) -> Result<Vec<(usize, Vec<u8>)>, MessageError> {
        let mut children = self.second.children([
            SECOND_SLAYER_STATE_CHILD,
            SECOND_SLAYER_STARTED_CHILD,
            SECOND_SLAYER_SEQUENCE_CHILD,
        ])?;
        children.extend(self.third.children([
            THIRD_SLAYER_STATE_CHILD,
            THIRD_SLAYER_STARTED_CHILD,
            THIRD_SLAYER_SEQUENCE_CHILD,
        ])?);
        children.extend(self.fourth.children([
            FOURTH_SLAYER_STATE_CHILD,
            FOURTH_SLAYER_STARTED_CHILD,
            FOURTH_SLAYER_SEQUENCE_CHILD,
        ])?);
        if let Some(marker) = self.marker {
            children.push((SLAYER_MARKER_CHILD, prefixed_bytes(marker)?));
        }
        Ok(children)
    }
}

/// The time into a character's first slayer slot sequence as one state sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceTime {
    /// `slayerSeqTimeRel`.
    pub rel: u8,
    /// `slayerSeqTimeAbs` (`half` bits), when it goes with the state.
    pub abs: Option<u16>,
}

impl SequenceTime {
    fn children(self) -> Vec<(usize, Vec<u8>)> {
        let mut children = vec![(SEQUENCE_TIME_REL_CHILD, vec![self.rel])];
        if let Some(abs) = self.abs {
            children.push((SEQUENCE_TIME_ABS_CHILD, abs.to_be_bytes().to_vec()));
        }
        children
    }
}

/// The sequence time of one character as its client has it: the absolute part it took last.
/// Retail sends a new absolute part, with the relative byte at zero, when a slayer state starts
/// and when the steps from the old part would pass [`SEQUENCE_TIME_MAX_STEPS`]; the states of the
/// next [`SEQUENCE_ABS_REPEAT_MS`] carry it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SequenceClock {
    /// The absolute part the client has (`half` bits).
    abs: Option<u16>,
    /// The state time (ms) of the state that sent it first.
    abs_sent_ms: Option<u64>,
}

/// The states this soon after a new absolute part carry it again, ms (Observed in the newchar
/// capture: the next state carried it in 217 of 249 cases when it came under 35 ms later and in
/// 31 of 247 when it came 40 ms or more later).
pub const SEQUENCE_ABS_REPEAT_MS: u64 = 40;

/// The `slayerSeqTimeRel` and `slayerSeqTimeAbs` of the first own state (the most common retail
/// first own ALC): 2 steps from 1/30 s, two input records into the slot's state.
const SPAWN_SEQUENCE_REL: u8 = 0x81;
const SPAWN_SEQUENCE_ABS: u16 = 0x2844;

impl SequenceClock {
    /// The clock of a client that took the first own state ([`AlcState::spawn`]): it has that
    /// state's absolute part.
    pub const fn after_spawn() -> Self {
        Self {
            abs: Some(SPAWN_SEQUENCE_ABS),
            abs_sent_ms: None,
        }
    }

    /// The sequence time, s, that the first own state sends.
    pub fn spawn_seconds() -> f32 {
        half_value(SPAWN_SEQUENCE_ABS)
            + f32::from(SPAWN_SEQUENCE_REL - SEQUENCE_TIME_REL_ZERO) * SEQUENCE_TIME_STEP_S
    }

    /// The wire form of `seconds` into the sequence for the next state, whose time is `state_ms`.
    /// `started` is the time at the record that started the sequence when it starts in this
    /// state: the absolute part is that time, the steps count the records since (retail
    /// sequence at 0x2aa: started by a 0.05 s record, first sent two records later as
    /// `85 2a66`). Steps round half away from zero; retail rounded some exact half steps down
    /// late in a sequence (110.5 and 127.5 at 0x6bb3), so such a state is one step above
    /// retail's, and the 127.5 one sends a new absolute part one state early.
    pub fn time(&mut self, seconds: f32, started: Option<f32>, state_ms: u64) -> SequenceTime {
        if let Some(start) = started {
            self.abs = Some(half_bits(start));
            self.abs_sent_ms = Some(state_ms);
        }
        let steps = self
            .abs
            .map(|abs| ((seconds - half_value(abs)) / SEQUENCE_TIME_STEP_S).round())
            .filter(|steps| (0.0..=SEQUENCE_TIME_MAX_STEPS).contains(steps));
        let Some(steps) = steps else {
            let abs = half_bits(seconds);
            self.abs = Some(abs);
            self.abs_sent_ms = Some(state_ms);
            return SequenceTime {
                rel: SEQUENCE_TIME_REL_ZERO,
                abs: Some(abs),
            };
        };
        let recent = self
            .abs_sent_ms
            .is_some_and(|sent| state_ms.saturating_sub(sent) < SEQUENCE_ABS_REPEAT_MS);
        // 0 to 127 steps above the zero byte: at most 254.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked to 0..=127 steps"
        )]
        SequenceTime {
            rel: SEQUENCE_TIME_REL_ZERO + steps as u8,
            abs: self.abs.filter(|_| recent),
        }
    }
}

/// One state of another player, as the clients that see it get it (replication note: retail
/// remote states carry their number and time, the place, the facing, where the player looks, and
/// its slayer state).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteStep {
    /// Its number and time; its anchor goes with every state (our choice: a client that comes
    /// into view of the player in the middle of an anchor period knows no other anchor).
    pub stamp: AlcStamp,
    /// Where the player is, when it goes with the state: an offset from the anchor every viewer
    /// shares, or a new anchor ([`StepPlace`]).
    pub position: Option<StepPlace>,
    /// Its facing (yaw, radians).
    pub facing: Option<f32>,
    /// Where it looks.
    pub look: Option<LookDir>,
    /// Its slayer state, when it goes with the state.
    pub slayer: Option<SlayerState>,
    /// The time into its slayer state's sequence, when it goes with the state.
    pub sequence_time: Option<SequenceTime>,
    /// Its slayer slots past the first and its slayer marker, when they go with the state.
    pub upper: Option<RemoteUpper>,
}

/// The slayer slots past the first and the slayer marker of another player as one of its states
/// sends them (children 16 to 24 and 48; Observed: the retail duel capture's remote players
/// carried them in the form of the own states, e.g. `19=2e 21=9706 48=28` at an aim, `48=01` at
/// its end).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteUpper {
    /// The slots and the marker.
    pub slots: UpperSlots,
    /// They changed after the slayer state started: each of their children takes the place of
    /// the same child of the slayer state; else the slayer state's children stay.
    pub after_state: bool,
}

impl RemoteUpper {
    /// Puts the children of the slots into `children` ([`RemoteUpper::after_state`]).
    fn put(self, children: &mut Vec<(usize, Vec<u8>)>) -> Result<(), MessageError> {
        for (child, value) in self.slots.children()? {
            match children.iter_mut().find(|(known, _)| *known == child) {
                Some(known) if self.after_state => known.1 = value,
                Some(_) => {}
                None => children.push((child, value)),
            }
        }
        Ok(())
    }
}

/// Where a character looks (the client input's view angles, record bit 6: yaw then pitch, in
/// radians).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookDir {
    /// The yaw: 0 looks north (+y), a positive yaw turns to the west.
    pub yaw: f32,
    /// The pitch: positive looks down (Observed: the own retail `lookDir` z is `-sin(pitch)`).
    pub pitch: f32,
}

impl LookDir {
    /// The `lookDir` bytes: the unit direction `(-sin yaw cos pitch, cos yaw cos pitch,
    /// -sin pitch, 0)` in the client's smallest-three form (Observed: every own retail `lookDir`
    /// of the duel capture against the input's view angles, and the remote ones have the same
    /// tags: w is always 0 and flagged so).
    pub fn bytes(self) -> Vec<u8> {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        smallest_three([-sin_yaw * cos_pitch, cos_yaw * cos_pitch, -sin_pitch, 0.0])
    }
}

/// The smallest-three form of the unit quat or direction `parts` (x, y, z, w;
/// `FUN_14087a8d0`): a tag with the index of the largest part in bits 5 and 6 (derived from
/// the others), its sign in bit 0 and a flag per part that is 0 in bits 1 to 4, then one byte
/// per other part, in part order.
fn smallest_three(parts: [f32; QUAT_PARTS]) -> Vec<u8> {
    let largest = (0..QUAT_PARTS)
        .max_by(|a, b| parts[*a].abs().total_cmp(&parts[*b].abs()))
        .unwrap_or(QUAT_PARTS - 1);
    let byte = |part: f32| ((part + QUAT_OFFSET) / QUAT_STEP).round().clamp(0.0, 255.0) as u8;
    // The largest part's index is below 4.
    let mut tag = (largest as u8) << QUAT_DERIVED_SHIFT;
    if parts[largest] < 0.0 {
        tag |= QUAT_TAG_NEGATIVE;
    }
    let mut bytes = Vec::with_capacity(QUAT_PARTS);
    for (index, part) in parts
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != largest)
    {
        if *part == 0.0 {
            tag |= QUAT_BIT_1 << index;
        } else {
            bytes.push(byte(*part));
        }
    }
    bytes.insert(0, tag);
    bytes
}

/// The `scopeTimeBlob0Data*` children of `ticks`: the low byte, and the higher ones when
/// `with_high`.
fn scope_time_children(ticks: u32, with_high: bool) -> Vec<(usize, Vec<u8>)> {
    let bytes = ticks.to_le_bytes();
    SCOPE_TIME_CHILDREN
        .iter()
        .zip(bytes)
        .take(if with_high {
            SCOPE_TIME_CHILDREN.len()
        } else {
            1
        })
        .map(|(child, byte)| (*child, vec![byte]))
        .collect()
}

/// The bytes of the prefix-coded `value`.
fn prefixed_bytes(value: u64) -> Result<Vec<u8>, MessageError> {
    Ok(Writer::new().prefixed(value)?.finish())
}

/// The `rotation` bytes of a facing `yaw` (radians; 0 faces north, +y, and a positive yaw turns
/// to the west): `(0, 0, sin(yaw/2), cos(yaw/2))` in the client's smallest-three form, with the
/// larger of z and w derived. Re-encodes all 3,648 retail values (rotation note, section 2).
pub fn rotation_bytes(yaw: f32) -> Vec<u8> {
    let half = yaw / 2.0;
    // q and -q are the same turn: keep w at 0 or above.
    let sign = if half.cos() < 0.0 { -1.0 } else { 1.0 };
    let (z, w) = (half.sin() * sign, half.cos() * sign);
    let byte = |part: f32| ((part + QUAT_OFFSET) / QUAT_STEP).round().clamp(0.0, 255.0) as u8;
    if z == 0.0 {
        vec![QUAT_TAG_NORTH]
    } else if w >= z.abs() {
        vec![QUAT_TAG_W_DERIVED, byte(z)]
    } else {
        let tag = QUAT_TAG_Z_DERIVED | if z < 0.0 { QUAT_TAG_NEGATIVE } else { 0 };
        vec![tag, byte(w)]
    }
}

/// The number and clock children of `stamp`.
fn stamp_children(stamp: AlcStamp) -> [(usize, Vec<u8>); 4] {
    [
        (ID_REL_CHILD, vec![AlcStamp::low(stamp.id)]),
        (TIME_REL_CHILD, vec![AlcStamp::low(stamp.offset_ms)]),
        (ID_ABS_CHILD, vec![AlcStamp::high(stamp.id)]),
        (TIME_ABS_CHILD, vec![AlcStamp::high(stamp.offset_ms)]),
    ]
}

/// The position children of an offset `offset` from the `pos10` `anchor`, which goes with it.
fn offset_children(anchor: InputPosition, offset: [u8; POS3_LEN]) -> [(usize, Vec<u8>); 2] {
    let mut position = Writer::new();
    anchor.write(&mut position);
    [
        (POS_REL_CHILD, offset.to_vec()),
        (POS10_CHILD, position.finish()),
    ]
}

/// The position children of `pos`: no offset from the new `pos10`.
fn position_children(pos: InputPosition) -> [(usize, Vec<u8>); 2] {
    let mut position = Writer::new();
    pos.write(&mut position);
    [
        (POS_REL_CHILD, AT_ANCHOR.to_vec()),
        (POS10_CHILD, position.finish()),
    ]
}
/// A `blob33` count is below this.
const BLOB33_LIMIT: u8 = 33;
/// A `blob177` count is below this.
const BLOB177_LIMIT: u8 = 177;
/// Bytes of a `frag` child.
const FRAG_LEN: usize = 12;
/// Bytes after a set `bounds` flag, before its `u8` and the next flag.
const BOUNDS_LEN: usize = 16;
/// Bytes of a `pos3` child.
pub const POS3_LEN: usize = 3;
/// The child presence bits the client tests.
const PRESENCE_BITS: u64 = (1 << 54) - 1;
/// Slot bits of the first byte.
const SLOT_BITS: [u8; 2] = [0b01, 0b10];
/// `quat` tag bits that shorten the child (movement message note, `FUN_14087a8d0`).
const QUAT_BIT_1: u8 = 0b10;
const QUAT_BIT_2: u8 = 0b100;
const QUAT_BIT_3: u8 = 0b1000;
const QUAT_BIT_4: u8 = 0b1_0000;
/// Extra `quat` bytes when no tag bit shortens it.
const QUAT_MAX_EXTRA: usize = 3;

/// The kind of one child, by its size on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlcChild {
    /// 1 byte.
    U8,
    /// 2 bytes.
    U16,
    /// 4 bytes.
    F32,
    /// 8 bytes.
    U64,
    /// 3 bytes.
    Pos3,
    /// `f32`, `f32`, `u16`, big-endian.
    Pos10,
    /// 2 bytes.
    Half,
    /// A short prefix integer.
    Prefix,
    /// A short prefix count below 33, then that many bytes.
    Blob33,
    /// A short prefix count below 177, then that many bytes.
    Blob177,
    /// 12 bytes.
    Frag,
    /// A flag byte; 1 adds 16 bytes, a `u8` and the next flag byte.
    Bounds,
    /// A tag byte, then 0 to 3 bytes.
    Quat,
}

use AlcChild::{Blob33, Blob177, Bounds, F32, Frag, Half, Pos3, Pos10, Prefix, Quat, U8, U16, U64};

/// The 54 children of slot 0, in constructor order.
pub const SLOT0_CHILDREN: [AlcChild; 54] = [
    U8, U8, U8, Half, U8, Half, U8, Half, U8, Half, Pos3, Quat, Quat, Prefix, U16, Prefix, Prefix,
    U16, Prefix, Prefix, U16, Prefix, Prefix, U16, Prefix, U8, U8, Pos10, U8, Blob33, U8, U8,
    Prefix, Blob177, Frag, F32, Half, Half, U8, U8, U64, U8, U8, U8, U8, U8, Prefix, U8, Prefix,
    U8, U8, U8, U8, F32,
];

/// The 9 children of slot 1, in constructor order.
pub const SLOT1_CHILDREN: [AlcChild; 9] = [U8, U8, Blob177, Blob33, U8, U8, U8, Bounds, U8];

/// The extra bytes after a `quat` tag.
const fn quat_extra(tag: u8) -> usize {
    let mut count = if tag & QUAT_BIT_1 == 0 {
        QUAT_MAX_EXTRA
    } else {
        QUAT_MAX_EXTRA - 1
    };
    if tag & QUAT_BIT_2 != 0 {
        count -= 1;
    }
    if tag & QUAT_BIT_3 != 0 {
        count -= 1;
    }
    if tag & QUAT_BIT_4 != 0 && count != 0 {
        count -= 1;
    }
    count
}

/// One type 11 member body: its bytes and its position.
#[derive(Debug, Clone, PartialEq)]
pub struct AlcState {
    /// The body as read.
    pub bytes: Vec<u8>,
    /// `pos10` when slot 0 sends it.
    pub pos10: Option<InputPosition>,
}

impl AlcState {
    /// Reads one body from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a blob count is over its limit, or a
    /// `bounds` flag is not 0 or 1.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let start = reader.rest();
        let walked = walk(reader)?;
        Ok(Self {
            bytes: start[..walked.used].to_vec(),
            pos10: walked.pos10,
        })
    }

    /// The slot-0 `worldPosRel` offset of the body, when it sends one.
    pub fn offset(&self) -> Option<[u8; POS3_LEN]> {
        let walked = walk(&mut Reader::new(&self.bytes)).ok()?;
        let (_, range) = walked
            .slot0_ranges
            .into_iter()
            .find(|(child, _)| *child == POS_REL_CHILD)?;
        self.bytes.get(range)?.try_into().ok()
    }

    /// Where the body puts its entity for a client whose `pos10` of the entity is `anchor`, as
    /// the client does: the body's own `pos10`, else `anchor`, plus the body's offset. `None` when
    /// the body sends no place, or only an offset and the client has no `pos10`.
    pub fn place(&self, anchor: Option<InputPosition>) -> Option<InputPosition> {
        let offset = self.offset();
        let base = match (self.pos10, offset) {
            (Some(pos10), _) => pos10,
            (None, Some(_)) => anchor?,
            (None, None) => return None,
        };
        let Some([dx, dy, dz]) = offset else {
            return Some(base);
        };
        InputPosition::from_metres(
            base.x + offset_metres(dx),
            base.y + offset_metres(dy),
            base.z() + offset_metres(dz),
        )
    }

    /// The same body as the first state of a mob at `pos`, with the number and clock of `stamp`
    /// and, when known, its facing and animation: a retail first state of a mob, moved to another spawn
    /// point and to this server's clock. Its offset from `pos10` becomes none. A child that the
    /// body does not send stays unsent. `None` when the body sends no `pos10`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a scope child cannot be encoded.
    pub fn mob_first(
        &self,
        stamp: AlcStamp,
        place: CardPlace,
        facing: Option<f32>,
        animation: Option<&MobAnimation>,
    ) -> Result<Option<Self>, MessageError> {
        let Ok(walked) = walk(&mut Reader::new(&self.bytes)) else {
            return Ok(None);
        };
        if walked.pos10.is_none() {
            return Ok(None);
        }
        let mut values: Vec<(usize, Vec<u8>)> = stamp_children(stamp).into();
        let (place_children, pos10) = place.children();
        values.extend(place_children);
        values.push((TIME_ANCHOR_CHILD, stamp.anchor_us.to_be_bytes().to_vec()));
        if let Some(yaw) = facing {
            values.push((ROTATION_CHILD, rotation_bytes(yaw)));
        }
        if let Some(animation) = animation {
            values.extend(animation.children()?);
        }
        Ok(Some(Self {
            bytes: self.with_children(walked, &values),
            pos10: Some(pos10),
        }))
    }

    /// The same body as the spawn card of a mob that has just spawned, at `at`: no state number,
    /// no clock, no time anchor and no scope, at its `pos10` (retail: 119 of 119 cards of fresh
    /// mob spawns in newchar-retail-2026-09-18; the mob's first state, about 100 ms later,
    /// starts its spawn fragment). A child that the body does not send stays unsent. `None`
    /// when the body sends no `pos10`.
    pub fn mob_spawn_card(&self, at: InputPosition) -> Option<Self> {
        let walked = walk(&mut Reader::new(&self.bytes)).ok()?;
        walked.pos10?;
        let mut values: Vec<(usize, Vec<u8>)> = position_children(at).into();
        values.extend(
            FRESH_CARD_VALUES
                .iter()
                .map(|(child, bytes)| (*child, bytes.to_vec())),
        );
        Some(Self {
            bytes: self.with_children(walked, &values),
            pos10: Some(at),
        })
    }

    /// Whether the body is the card of a fresh spawn: state number 0 and a time anchor of 0, as
    /// [`Self::mob_spawn_card`] writes (a card of a mob already alive carries its newest state).
    pub fn is_fresh_card(&self) -> bool {
        let Ok(children) = self.slot0_children() else {
            return false;
        };
        let zero = |index: usize| {
            children
                .iter()
                .find(|(child, _)| *child == index)
                .is_none_or(|(_, bytes)| bytes.iter().all(|byte| *byte == 0))
        };
        zero(ID_REL_CHILD) && zero(ID_ABS_CHILD) && zero(TIME_ANCHOR_CHILD)
    }

    /// The body with the slot-0 children of `values` written over the ones `walked` found.
    fn with_children(&self, walked: Walked, values: &[(usize, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.bytes.len());
        let mut copied = 0;
        for (child, range) in walked.slot0_ranges {
            if let Some((_, value)) = values.iter().find(|(index, _)| *index == child) {
                bytes.extend_from_slice(&self.bytes[copied..range.start]);
                bytes.extend_from_slice(value);
                copied = range.end;
            }
        }
        bytes.extend_from_slice(&self.bytes[copied..]);
        bytes
    }

    /// A later state of a mob: its number and time, the time anchor when it is new, the retail
    /// re-anchor form of the position when it moved, and its animation children (sailor-movement
    /// note, section 5; mob-animation note, section 5).
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a scope child cannot be encoded.
    pub fn mob_step(step: &MobStep) -> Result<Self, MessageError> {
        let mut children: Vec<(usize, Vec<u8>)> = stamp_children(step.stamp).into();
        if let Some(place) = step.position {
            children.extend(place.children());
        }
        if let Some(yaw) = step.facing {
            children.push((ROTATION_CHILD, rotation_bytes(yaw)));
        }
        match step.scope {
            Some(StepScope::Ticks {
                ticks,
                high_changed,
                look_pose_seconds,
            }) => {
                children.extend(scope_time_children(ticks, high_changed));
                if look_pose_seconds.is_some() {
                    children.push((SCOPE_TIME_EX_CHILD, times_ex_blob(look_pose_seconds)?));
                }
            }
            Some(StepScope::Changed(animation)) => children.extend(animation.children()?),
            None => {}
        }
        if step.anchored {
            children.push((
                TIME_ANCHOR_CHILD,
                step.stamp.anchor_us.to_be_bytes().to_vec(),
            ));
        }
        Ok(Self::of_slot0(
            children,
            step.position.and_then(StepPlace::pos10),
        ))
    }

    /// A state of another player ([`RemoteStep`]).
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a slayer value does not fit the prefix code.
    pub fn remote_step(step: &RemoteStep) -> Result<Self, MessageError> {
        let mut children: Vec<(usize, Vec<u8>)> = stamp_children(step.stamp).into();
        children.push((
            TIME_ANCHOR_CHILD,
            step.stamp.anchor_us.to_be_bytes().to_vec(),
        ));
        if let Some(place) = step.position {
            children.extend(place.children());
        }
        if let Some(yaw) = step.facing {
            children.push((ROTATION_CHILD, rotation_bytes(yaw)));
        }
        if let Some(look) = step.look {
            children.push((LOOK_DIR_CHILD, look.bytes()));
        }
        if let Some(slayer) = step.slayer {
            children.extend(slayer.children()?);
        }
        if let Some(upper) = step.upper {
            upper.put(&mut children)?;
        }
        if let Some(time) = step.sequence_time {
            children.extend(time.children());
        }
        Ok(Self::of_slot0(
            children,
            step.position.and_then(StepPlace::pos10),
        ))
    }

    /// A state of the client's own player on its clock `stamp`: the client takes an own state
    /// only with its number and time, and shows the network icon and clears its input history
    /// when none came for 2 s (network note: `FUN_14485b680`, `alc-server-timeout-for-input`).
    /// With `place`, it also sends the place: an offset from the own `pos10` the client has, or a
    /// new one when out of reach (lock-on note, run 10: retail sent the own `worldPosRel` as an
    /// offset in 264 of 438 own states during a 7.5 s lock and a new `pos10` in 23; ours sent a
    /// new `pos10` with every place, every ~200 ms, and the lock broke within 2.15 s; Inferred
    /// cause, as for the moving mob). With `data_bits`, the movement flags; when
    /// `anchored`, the clock's anchor (the client's 'now' for its cooldowns is the anchor plus
    /// the offset; clock note). With `slayer`, the slayer state the client started or the
    /// server's follow-up (slayer note). With `view`, the facing and the look direction of the
    /// client's newest input (lock-on note: retail sent the own `rotation` in 938 and the own
    /// `lookDir` in 408 of 1,193 own states around the first lock; ours sent neither, and the
    /// client's locks broke within 0.35 to 1.1 s). With `sequence_time`, the time into the
    /// slayer state's sequence (retail sends it in 86,132 of 86,677 own states).
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a slayer value does not fit the prefix code.
    pub fn own_step(
        stamp: AlcStamp,
        place: Option<StepPlace>,
        data_bits: Option<u8>,
        anchored: bool,
        slayer: Option<SlayerState>,
        view: Option<LookDir>,
        sequence_time: Option<SequenceTime>,
    ) -> Result<Self, MessageError> {
        let mut children: Vec<(usize, Vec<u8>)> = stamp_children(stamp).into();
        if let Some(time) = sequence_time {
            children.extend(time.children());
        }
        if let Some(view) = view {
            children.push((ROTATION_CHILD, rotation_bytes(view.yaw)));
            children.push((LOOK_DIR_CHILD, view.bytes()));
        }
        if let Some(slayer) = slayer {
            children.extend(slayer.children()?);
        }
        if anchored {
            children.push((TIME_ANCHOR_CHILD, stamp.anchor_us.to_be_bytes().to_vec()));
        }
        if let Some(place) = place {
            children.extend(place.children());
        }
        if let Some(bits) = data_bits {
            children.push((DATA_BITS_CHILD, vec![bits]));
        }
        Ok(Self::of_slot0(children, place.and_then(StepPlace::pos10)))
    }

    /// The same own state with the ground and water under the player in slot 1: `distGround`
    /// (child 0) when `ground_distance` is given and `waterDepth` (child 1) when `water_depth`
    /// is (run-15 movement study: retail sends each in an own state when it changes; the client
    /// swims, wades, jumps and dodges by them). A body that sends slot 1 already, or no change,
    /// stays as it is.
    #[must_use]
    pub fn with_footing(mut self, ground_distance: Option<u8>, water_depth: Option<u8>) -> Self {
        let has_slot1 = self
            .bytes
            .first()
            .is_none_or(|mask| mask & SLOT_BITS[1] != 0);
        if has_slot1 || (ground_distance.is_none() && water_depth.is_none()) {
            return self;
        }
        let children = [
            (DIST_GROUND_CHILD, ground_distance),
            (WATER_DEPTH_CHILD, water_depth),
        ];
        let present = children
            .iter()
            .filter(|(_, value)| value.is_some())
            .fold(0_u64, |mask, (child, _)| mask | 1 << child);
        let mut writer = Writer::new();
        writer.wide_prefixed(present);
        for value in children.iter().filter_map(|(_, value)| *value) {
            writer.u8(value);
        }
        self.bytes[0] |= SLOT_BITS[1];
        self.bytes.extend_from_slice(&writer.finish());
        self
    }

    /// The same own state of slot 0 alone with the slayer slots `slots` changed: each of their
    /// children takes the place of the same child of the state. Call it before
    /// [`AlcState::with_footing`]: a body that sends slot 1 stays as it is.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not walk or a value does not fit the prefix
    /// code.
    pub fn with_upper_slots(self, slots: UpperSlots) -> Result<Self, MessageError> {
        let has_slot1 = self
            .bytes
            .first()
            .is_none_or(|mask| mask & SLOT_BITS[1] != 0);
        if has_slot1 || slots == UpperSlots::default() {
            return Ok(self);
        }
        let added = slots.children()?;
        let mut children: Vec<(usize, Vec<u8>)> = self
            .slot0_children()?
            .into_iter()
            .filter(|(child, _)| added.iter().all(|(new, _)| new != child))
            .collect();
        children.extend(added);
        Ok(Self::of_slot0(children, self.pos10))
    }

    /// One of the three states that move the client's own player to `pos`, facing `facing`
    /// (yaw, radians) when it is known, with teleport number `teleport` (retail moved the player
    /// into the water so, 118546 to 118614 ms): see [`TeleportStep`]. Each carries the number and
    /// time of `stamp`, and the sequence time `sequence_time` when given (retail: `2=ce` with
    /// the move into the water).
    pub fn teleport(
        stamp: AlcStamp,
        pos: InputPosition,
        facing: Option<f32>,
        teleport: u8,
        step: TeleportStep,
        sequence_time: Option<SequenceTime>,
    ) -> Self {
        let mut children: Vec<(usize, Vec<u8>)> = stamp_children(stamp).into();
        if let Some(time) = sequence_time {
            children.extend(time.children());
        }
        let [at_anchor, pos10] = position_children(pos);
        let rotation = facing.map(|yaw| (ROTATION_CHILD, rotation_bytes(yaw)));
        let zero_offset = (POS_REL_CHILD, ZERO_POS3.to_vec());
        let number = (TELEPORT_CHILD, vec![teleport]);
        match step {
            TeleportStep::Place => children.extend([at_anchor, pos10].into_iter().chain(rotation)),
            TeleportStep::Confirm => {
                children.extend([zero_offset, pos10, number].into_iter().chain(rotation));
            }
            TeleportStep::Hold => children.extend([zero_offset, number]),
        }
        Self::of_slot0(children, (step != TeleportStep::Hold).then_some(pos))
    }

    /// A body of slot 0 alone with `children` (child index and bytes, in any order).
    fn of_slot0(mut children: Vec<(usize, Vec<u8>)>, pos10: Option<InputPosition>) -> Self {
        children.sort_unstable_by_key(|(child, _)| *child);
        let present = children
            .iter()
            .fold(0_u64, |mask, (child, _)| mask | 1 << child);
        let mut writer = Writer::new();
        writer.u8(SLOT_BITS[0]).wide_prefixed(present);
        for (_, value) in &children {
            writer.bytes(value);
        }
        Self {
            bytes: writer.finish(),
            pos10,
        }
    }

    /// Writes the body as read.
    pub fn write(&self, writer: &mut Writer) {
        writer.bytes(&self.bytes);
    }

    /// The slot-0 children this body sends, in child order, each with its bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not walk.
    pub fn slot0_children(&self) -> Result<Vec<(usize, Vec<u8>)>, MessageError> {
        let walked = walk(&mut Reader::new(&self.bytes))?;
        Ok(walked
            .slot0_ranges
            .into_iter()
            .map(|(child, range)| (child, self.bytes[range].to_vec()))
            .collect())
    }

    /// The first state of a player at `place`: both slots with the children and values of the
    /// most common retail first own ALC (own-player-position note: 315 logins). The client takes
    /// its start position only from such a full state (with the absolute `idAbs`,
    /// `timeOffsetAbs` and `worldPosAbs` parts); a lone `pos10` leaves it at the grid origin. Its
    /// `pos10` is the place's anchor and its `worldPosRel` the offset from it, so that a viewer
    /// that sees another player from now on keeps the anchor the others have ([`CardPlace`]).
    /// It carries the number, time and anchor of `stamp` (retail: number 2 and 9 ms in the most
    /// common first own state, the number of the client's input record then, as every own
    /// state's), and its slayer slots started one state before it, as retail's started at 1.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a prefix value cannot be encoded.
    pub fn spawn(place: CardPlace, stamp: AlcStamp) -> Result<Self, MessageError> {
        let (pos10, offset) = place.resolved();
        let at = SpawnPlace {
            pos10,
            offset: offset.unwrap_or(ZERO_POS3),
        };
        let mut writer = Writer::new();
        writer.u8(SLOT_BITS[0] | SLOT_BITS[1]);
        write_children(&mut writer, &SPAWN_SLOT0, at, stamp)?;
        write_children(&mut writer, &SPAWN_SLOT1, at, stamp)?;
        Ok(Self {
            bytes: writer.finish(),
            pos10: Some(pos10),
        })
    }

    /// A minimal body that replicates only `pos` through the slot-0 `pos10` child (child 27):
    /// the slot-0 mask bit, a presence integer with only child 27 set, then the 10-byte position.
    /// This is how entities carry their position in the retail bundles (the dominant member is
    /// type 11); [`read`](Self::read) of the result returns the same bytes and `pos10`.
    pub fn at_position(pos: InputPosition) -> Self {
        let mut writer = Writer::new();
        writer.u8(SLOT_BITS[0]);
        writer.wide_prefixed(1 << POS10_CHILD);
        writer.f32(pos.x).f32(pos.y).u16(pos.z_raw);
        Self {
            bytes: writer.finish(),
            pos10: Some(pos),
        }
    }
}

/// What a walk of one body found.
struct Walked {
    /// Bytes of the body.
    used: usize,
    /// Its `pos10`, when it sends one.
    pos10: Option<InputPosition>,
    /// The body bytes of each slot-0 child it sends.
    slot0_ranges: Vec<(usize, Range<usize>)>,
}

/// Walks one body.
fn walk(reader: &mut Reader<'_>) -> Result<Walked, MessageError> {
    let length = reader.remaining();
    let mask = reader.u8()?;
    let mut pos10 = None;
    let mut slot0_ranges = Vec::new();
    for (slot, (slot_bit, children)) in SLOT_BITS
        .iter()
        .zip([&SLOT0_CHILDREN[..], &SLOT1_CHILDREN])
        .enumerate()
    {
        if mask & slot_bit == 0 {
            continue;
        }
        let present = reader.wide_prefixed()? & PRESENCE_BITS;
        for (index, kind) in children.iter().enumerate() {
            if present >> index & 1 == 0 {
                continue;
            }
            let start = length - reader.remaining();
            if *kind == Pos10 {
                pos10 = Some(InputPosition::read(reader)?);
            } else {
                skip_child(*kind, reader)?;
            }
            if slot == 0 {
                slot0_ranges.push((index, start..length - reader.remaining()));
            }
        }
    }
    Ok(Walked {
        used: length - reader.remaining(),
        pos10,
        slot0_ranges,
    })
}

/// One child value of a written state.
#[derive(Debug, Clone, Copy)]
enum SpawnValue {
    /// A `u8`.
    Byte(u8),
    /// A `u16` or `half`.
    Word(u16),
    /// A short prefix integer.
    Prefixed(u64),
    /// An `f32`.
    Float(f32),
    /// Fixed bytes: a `pos3`, a `quat` (tag and extra bytes), a `frag`, or a blob (count and
    /// bytes).
    Raw(&'static [u8]),
    /// The `pos10` position.
    Position,
    /// The `worldPosRel` offset from `pos10`.
    PositionOffset,
    /// The clock anchor (µs since 2000-01-01).
    Anchor,
    /// The low or the high byte of the state number.
    IdLow,
    IdHigh,
    /// The low or the high byte of the state time.
    TimeLow,
    TimeHigh,
    /// The number of the state before: where the slayer slots started.
    StartedBefore,
}

use SpawnValue::{
    Anchor, Byte, Float, IdHigh, IdLow, Position as SpawnPosition, PositionOffset, Prefixed, Raw,
    StartedBefore, TimeHigh, TimeLow, Word,
};

/// Where a first state puts the player: its `pos10` and its `worldPosRel` offset from it.
#[derive(Debug, Clone, Copy)]
struct SpawnPlace {
    pos10: InputPosition,
    offset: [u8; POS3_LEN],
}

/// `worldPosRel` 0 on each axis (the relative part of the position; `0x7f` is the zero step).
const ZERO_POS3: [u8; POS3_LEN] = [0x7f; POS3_LEN];
/// An empty blob: count 0.
const EMPTY_BLOB: [u8; 1] = [0];
/// The two `half` children after the facing angle (meaning Unknown): 0 on every fresh mob card.
const FRESH_CARD_HALVES: [usize; 2] = [36, 37];
/// The slot-0 children of a fresh mob card that are the same on all 119 retail fresh spawn
/// cards of newchar-retail-2026-09-18 (only the facing, child 44 and the position differ): the
/// number and time parts 0, the scope times, mask, entries and tags empty, the facing angle,
/// the two halves and the time anchor 0, the look `3a`, the teleport number 1, the data bits
/// `10 10 00 _ 00`, the segmented stamina, child 47 and child 53 (2.0).
const FRESH_CARD_VALUES: [(usize, &[u8]); 21] = [
    (ID_REL_CHILD, &[0]),
    (TIME_REL_CHILD, &[0]),
    (LOOK_DIR_CHILD, &[0x3a]),
    (ID_ABS_CHILD, &[0]),
    (TIME_ABS_CHILD, &[0]),
    (SCOPE_TIME_CHILDREN[0], &[0]),
    (SCOPE_TIME_EX_CHILD, &EMPTY_BLOB),
    (TELEPORT_CHILD, &[1]),
    (SCOPE_DATA_CHILD, &[0]),
    (SCOPE_INFO_CHILD, &EMPTY_BLOB),
    (FRAG_TAGS_CHILD, &[0; FRAG_TAGS_LEN]),
    (FACING_GAP_CHILD, &[0; 4]),
    (FRESH_CARD_HALVES[0], &[0; 2]),
    (FRESH_CARD_HALVES[1], &[0; 2]),
    (TIME_ANCHOR_CHILD, &[0; 8]),
    (DATA_BITS_CHILD, &[0x10]),
    (42, &[0x10]),
    (43, &[0]),
    (45, &[0]),
    (46, &[0xf3, 0xdf, 0xc6, 0xf6, 0x1d]),
    (47, &[0]),
];
/// A `frag` of zeros.
const ZERO_FRAG: [u8; FRAG_LEN] = [0; FRAG_LEN];
/// The retail `segmentedStamina` (child 46) of every first own ALC (315 of 315 logins).
const FULL_SEGMENTED_STAMINA: u64 = 4_021_696_251;

/// The number of the first own state (the most common retail first own ALC).
pub const SPAWN_STATE_ID: u16 = 2;
/// How many states before the first state its slayer slots started (retail: 1 before 2).
const SPAWN_STARTED_BEFORE: u16 = 1;
/// The time of the first own state after its anchor of 0, in ms.
pub const SPAWN_STATE_MS: u16 = 9;
/// The teleport number of the first own state.
pub const SPAWN_TELEPORT: u8 = 1;

/// Slot-0 children of the first own state (child index and value), from the most common retail
/// first own ALC: id and time parts, the four slayer slots, zero relative position, a facing,
/// the absolute parts 0 with `worldPosAbs`, `teleportAndMigrationId` 1, and the fixed scope,
/// water, anchor and stamina children. Child 40 (`timeAnchor`) is 0, as in 16 logins.
const SPAWN_SLOT0: [(usize, SpawnValue); 46] = [
    (ID_REL_CHILD, IdLow),
    (TIME_REL_CHILD, TimeLow),
    (SEQUENCE_TIME_REL_CHILD, Byte(SPAWN_SEQUENCE_REL)),
    (SEQUENCE_TIME_ABS_CHILD, Word(SPAWN_SEQUENCE_ABS)),
    (4, Byte(127)),
    (5, Word(0)),
    (6, Byte(127)),
    (7, Word(0)),
    (8, Byte(127)),
    (9, Word(0)),
    (POS_REL_CHILD, PositionOffset),
    (11, Raw(&[0x66, 0xfe])),
    (12, Raw(&[0x19, 0x7f])),
    (13, Prefixed(31)),
    (14, StartedBefore),
    (15, Prefixed(1081)),
    (16, Prefixed(45)),
    (17, StartedBefore),
    (18, Prefixed(0)),
    (19, Prefixed(0)),
    (20, Word(0)),
    (21, Prefixed(0)),
    (22, Prefixed(0)),
    (23, Word(0)),
    (24, Prefixed(0)),
    (ID_ABS_CHILD, IdHigh),
    (TIME_ABS_CHILD, TimeHigh),
    (POS10_CHILD, SpawnPosition),
    (29, Raw(&EMPTY_BLOB)),
    (TELEPORT_CHILD, Byte(SPAWN_TELEPORT)),
    (32, Prefixed(0)),
    (33, Raw(&EMPTY_BLOB)),
    (34, Raw(&ZERO_FRAG)),
    (35, Float(0.0)),
    (36, Word(0x4a20)),
    (37, Word(0)),
    (TIME_ANCHOR_CHILD, Anchor),
    (DATA_BITS_CHILD, Byte(243)),
    (42, Byte(83)),
    (43, Byte(0)),
    (44, Byte(20)),
    (45, Byte(0)),
    (46, Prefixed(FULL_SEGMENTED_STAMINA)),
    (47, Byte(4)),
    (48, Prefixed(0)),
    (53, Float(2.0)),
];

/// Slot-1 children of the first own state: all 0 or empty.
const SPAWN_SLOT1: [(usize, SpawnValue); 7] = [
    (0, Byte(0)),
    (1, Byte(0)),
    (2, Raw(&EMPTY_BLOB)),
    (3, Raw(&EMPTY_BLOB)),
    (4, Byte(0)),
    (5, Byte(0)),
    (6, Byte(0)),
];

/// Writes one slot: the presence integer of `children`, then each value in order.
fn write_children(
    writer: &mut Writer,
    children: &[(usize, SpawnValue)],
    place: SpawnPlace,
    stamp: AlcStamp,
) -> Result<(), MessageError> {
    let present = children
        .iter()
        .fold(0_u64, |mask, (index, _)| mask | 1 << index);
    writer.wide_prefixed(present);
    for (_, value) in children {
        match *value {
            Byte(value) => {
                writer.u8(value);
            }
            Word(value) => {
                writer.u16(value);
            }
            Prefixed(value) => {
                writer.prefixed(value)?;
            }
            Float(value) => {
                writer.f32(value);
            }
            Raw(bytes) => {
                writer.bytes(bytes);
            }
            SpawnPosition => place.pos10.write(writer),
            PositionOffset => {
                writer.bytes(&place.offset);
            }
            Anchor => {
                writer.u64(stamp.anchor_us);
            }
            IdLow => {
                writer.u8(AlcStamp::low(stamp.id));
            }
            IdHigh => {
                writer.u8(AlcStamp::high(stamp.id));
            }
            TimeLow => {
                writer.u8(AlcStamp::low(stamp.offset_ms));
            }
            TimeHigh => {
                writer.u8(AlcStamp::high(stamp.offset_ms));
            }
            StartedBefore => {
                writer.u16(stamp.id.wrapping_sub(SPAWN_STARTED_BEFORE));
            }
        }
    }
    Ok(())
}

fn skip_child(kind: AlcChild, reader: &mut Reader<'_>) -> Result<(), MessageError> {
    match kind {
        U8 => {
            reader.u8()?;
        }
        U16 | Half => {
            reader.u16()?;
        }
        F32 => {
            reader.f32()?;
        }
        U64 => {
            reader.u64()?;
        }
        Pos3 => {
            reader.bytes(POS3_LEN)?;
        }
        Pos10 => {
            InputPosition::read(reader)?;
        }
        Prefix => {
            reader.prefixed()?;
        }
        Blob33 => skip_blob(reader, BLOB33_LIMIT)?,
        Blob177 => skip_blob(reader, BLOB177_LIMIT)?,
        Frag => {
            reader.bytes(FRAG_LEN)?;
        }
        Bounds => {
            if reader.boolean()? {
                reader.bytes(BOUNDS_LEN)?;
                reader.u8()?;
                reader.boolean()?;
            }
        }
        Quat => {
            let tag = reader.u8()?;
            reader.bytes(quat_extra(tag))?;
        }
    }
    Ok(())
}

fn skip_blob(reader: &mut Reader<'_>, limit: u8) -> Result<(), MessageError> {
    let count = reader.prefixed()?;
    if count >= u64::from(limit) {
        return Err(MessageError::CountTooLarge {
            count,
            limit: u64::from(limit - 1),
        });
    }
    // The count is below 177.
    reader.bytes(count as usize)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::payload::{BundlePayload, MemberState};
    use crate::position::{
        POSITION_IN_THE_WORLD_TYPE_INDEX, PositionInTheWorldReplicatedState, WorldPosition,
    };
    use crate::wire::Writer;

    const POS10_X: f32 = 100.5;
    const POS10_Y: f32 = 200.25;
    const POS10_Z_RAW: u16 = 1000;
    const SLOT_0: u8 = 0b01;
    const SLOT_1: u8 = 0b10;
    const ENTITY: u64 = 9;
    const GROUP: u64 = 1;
    /// A presence bit above the 54 children of slot 0.
    const STRAY_BIT: u64 = 1 << 60;

    /// Valid bytes of one child of `kind`, from `seed`.
    fn child_bytes(kind: AlcChild, seed: u8) -> Vec<u8> {
        let mut writer = Writer::new();
        match kind {
            AlcChild::U8 => {
                writer.u8(seed);
            }
            AlcChild::U16 | AlcChild::Half => {
                writer.u16(u16::from(seed));
            }
            AlcChild::F32 => {
                writer.f32(f32::from(seed));
            }
            AlcChild::U64 => {
                writer.u64(u64::from(seed));
            }
            AlcChild::Pos3 => {
                writer.bytes(&[seed; 3]);
            }
            AlcChild::Pos10 => {
                writer.f32(POS10_X).f32(POS10_Y).u16(POS10_Z_RAW);
            }
            AlcChild::Prefix => {
                writer.prefixed(u64::from(seed)).expect("prefix");
            }
            AlcChild::Blob33 => {
                let count = seed % BLOB33_LIMIT;
                writer
                    .prefixed(u64::from(count))
                    .expect("count")
                    .bytes(&vec![seed; usize::from(count)]);
            }
            AlcChild::Blob177 => {
                let count = seed % BLOB177_LIMIT;
                writer
                    .prefixed(u64::from(count))
                    .expect("count")
                    .bytes(&vec![seed; usize::from(count)]);
            }
            AlcChild::Frag => {
                writer.bytes(&[seed; FRAG_LEN]);
            }
            AlcChild::Bounds => {
                if seed.is_multiple_of(2) {
                    writer.u8(0);
                } else {
                    writer.u8(1).bytes(&[seed; BOUNDS_LEN]).u8(seed).u8(0);
                }
            }
            AlcChild::Quat => {
                writer.u8(seed).bytes(&vec![seed; quat_extra(seed)]);
            }
        }
        writer.finish()
    }

    /// A body with the given children of each slot set, from `seed`.
    fn body(slot0: &[usize], slot1: &[usize], seed: u8) -> Vec<u8> {
        let mut writer = Writer::new();
        let mut mask = 0;
        if !slot0.is_empty() {
            mask |= SLOT_0;
        }
        if !slot1.is_empty() {
            mask |= SLOT_1;
        }
        writer.u8(mask);
        for (children, table) in [(slot0, &SLOT0_CHILDREN[..]), (slot1, &SLOT1_CHILDREN[..])] {
            if children.is_empty() {
                continue;
            }
            let bits = children.iter().fold(0_u64, |bits, child| bits | 1 << child);
            writer.wide_prefixed(bits);
            for (index, kind) in table.iter().enumerate() {
                if children.contains(&index) {
                    let value = seed.wrapping_add(u8::try_from(index).expect("small"));
                    writer.bytes(&child_bytes(*kind, value));
                }
            }
        }
        writer.finish()
    }

    fn read(bytes: &[u8]) -> Result<(AlcState, usize), MessageError> {
        let mut reader = Reader::new(bytes);
        let state = AlcState::read(&mut reader)?;
        Ok((state, reader.remaining()))
    }

    #[test]
    fn the_tables_have_the_note_widths() {
        assert_eq!(SLOT0_CHILDREN.len(), 54);
        assert_eq!(SLOT1_CHILDREN.len(), 9);
        assert_eq!(SLOT0_CHILDREN[POS10_CHILD], AlcChild::Pos10);
    }

    #[test]
    fn an_own_step_carries_the_footing_in_slot_1_as_retail() {
        // Retail newchar 2026-09-18, the own state as the player sank into the tutorial sea:
        // `03 87 30 0d f1 cc 7c 83 4c 66 f5 03 c1 02`, slot 1 `03 c1 02` (`distGround` 193,
        // `waterDepth` 2) after slot 0.
        const RETAIL_SLOT1: [u8; 3] = [0x03, 0xc1, 0x02];
        const SWIM_DISTANCE: u8 = 0xc1;
        const SHALLOW: u8 = 2;
        const VERY_SHALLOW: u8 = 1;
        /// Slot 1 presence of `waterDepth` alone.
        const WATER_ONLY: u8 = 0b10;
        let stamp = AlcStamp {
            id: 0x0102,
            anchor_us: 0,
            offset_ms: 0x0304,
        };
        let plain =
            || AlcState::own_step(stamp, None, None, false, None, None, None).expect("step");
        let both = plain().with_footing(Some(SWIM_DISTANCE), Some(SHALLOW));
        assert_eq!(both.bytes[0], SLOT_BITS[0] | SLOT_BITS[1]);
        assert!(both.bytes.ends_with(&RETAIL_SLOT1), "{:02x?}", both.bytes);
        let read = AlcState::read(&mut Reader::new(&both.bytes)).expect("walks");
        assert_eq!(read.bytes, both.bytes, "the walk takes slot 1 to the end");
        assert_eq!(read.slot0_children(), plain().slot0_children());
        let water = plain().with_footing(None, Some(VERY_SHALLOW));
        assert!(water.bytes.ends_with(&[WATER_ONLY, VERY_SHALLOW]));
        assert_eq!(plain().with_footing(None, None), plain(), "no change");
        let spawn = both.clone().with_footing(Some(0), Some(0));
        assert_eq!(spawn, both, "a body with slot 1 keeps it");
    }

    #[test]
    fn an_own_step_carries_the_aim_slots_as_retail() {
        // Retail newchar 2026-09-18, the blunderbuss aim after right-click record 0x54ad:
        // `19=2e 20=54ad 21=9706 48=28`.
        const AIM_RECORD: u16 = 0x54ad;
        const AIM_STATE: u64 = 0x2e;
        const AIM_SEQUENCE: u64 = 407;
        const AIM_MARKER: u64 = 0x28;
        let stamp = AlcStamp {
            id: 0x0102,
            anchor_us: 0,
            offset_ms: 0x0304,
        };
        let plain =
            || AlcState::own_step(stamp, None, None, false, None, None, None).expect("step");
        let aim = UpperSlots {
            third: SlotChange {
                state: Some(AIM_STATE),
                started: Some(AIM_RECORD),
                sequence: Some(AIM_SEQUENCE),
            },
            marker: Some(AIM_MARKER),
            ..UpperSlots::default()
        };
        let state = plain().with_upper_slots(aim).expect("slots");
        let children = state.slot0_children().expect("walks");
        let child = |index: usize| {
            children
                .iter()
                .find(|(child, _)| *child == index)
                .map(|(_, bytes)| bytes.clone())
        };
        assert_eq!(child(THIRD_SLAYER_STATE_CHILD), Some(vec![0x2e]));
        assert_eq!(child(THIRD_SLAYER_STARTED_CHILD), Some(vec![0x54, 0xad]));
        assert_eq!(child(THIRD_SLAYER_SEQUENCE_CHILD), Some(vec![0x97, 0x06]));
        assert_eq!(child(SLAYER_MARKER_CHILD), Some(vec![0x28]));
        assert_eq!(
            plain()
                .with_upper_slots(UpperSlots::default())
                .expect("none"),
            plain(),
            "no change"
        );
        let again = state
            .clone()
            .with_upper_slots(UpperSlots {
                marker: Some(0),
                ..UpperSlots::default()
            })
            .expect("again");
        assert_eq!(
            again
                .slot0_children()
                .expect("walks")
                .iter()
                .filter(|(child, _)| *child == SLAYER_MARKER_CHILD)
                .map(|(_, bytes)| bytes.clone())
                .collect::<Vec<_>>(),
            vec![vec![0]],
            "a child takes the place of the same one"
        );
    }

    #[test]
    fn a_remote_step_carries_the_upper_slots_by_their_order_to_the_state() {
        // Retail arena capture 2026-09-13, a remote aim: `19=2e 20=0dea 21=9606 48=28`; here
        // with an attack whose second slot started later and a draw before it.
        const AIM_STARTED: u16 = 0x0dea;
        const ATTACK_STARTED: u16 = 0x0e2a;
        const DRAW_STARTED: u16 = 0x0d00;
        let stamp = AlcStamp {
            id: 0x14e5,
            anchor_us: 0,
            offset_ms: 0x0010,
        };
        let attack = SlayerState {
            state: None,
            started: Some(ATTACK_STARTED),
            sequence: 2205,
            second: Some(SecondSlayerState {
                started: ATTACK_STARTED,
                playing: None,
                stopped: false,
            }),
        };
        let slots = UpperSlots {
            second: SlotChange {
                state: Some(0x2c),
                started: Some(DRAW_STARTED),
                sequence: Some(3099),
            },
            third: SlotChange {
                state: Some(0x2e),
                started: Some(AIM_STARTED),
                sequence: Some(406),
            },
            marker: Some(0x28),
            ..UpperSlots::default()
        };
        let step = |after_state| RemoteStep {
            stamp,
            position: None,
            facing: None,
            look: None,
            slayer: Some(attack),
            sequence_time: None,
            upper: Some(RemoteUpper { slots, after_state }),
        };
        let child_of = |state: &AlcState, index: usize| {
            state
                .slot0_children()
                .expect("walks")
                .into_iter()
                .find(|(child, _)| *child == index)
                .map(|(_, bytes)| bytes)
        };
        let older = AlcState::remote_step(&step(false)).expect("step");
        assert_eq!(
            child_of(&older, SECOND_SLAYER_STARTED_CHILD),
            Some(ATTACK_STARTED.to_be_bytes().to_vec()),
            "the newer attack keeps its second slot"
        );
        assert_eq!(
            child_of(&older, THIRD_SLAYER_SEQUENCE_CHILD),
            Some(vec![0x96, 0x06])
        );
        assert_eq!(child_of(&older, SLAYER_MARKER_CHILD), Some(vec![0x28]));
        assert_eq!(
            child_of(&older, SECOND_SLAYER_STATE_CHILD),
            Some(vec![0x2c]),
            "a child the state does not send goes"
        );
        let newer = AlcState::remote_step(&step(true)).expect("step");
        assert_eq!(
            child_of(&newer, SECOND_SLAYER_STARTED_CHILD),
            Some(DRAW_STARTED.to_be_bytes().to_vec()),
            "the newer slots take the second slot"
        );
    }

    #[test]
    fn an_own_step_carries_its_clock_and_only_the_asked_parts() {
        let stamp = AlcStamp {
            id: 0x0102,
            anchor_us: 0,
            offset_ms: 0x0304,
        };
        let children_of = |state: &AlcState| -> Vec<usize> {
            walk(&mut Reader::new(&state.bytes))
                .expect("walk")
                .slot0_ranges
                .into_iter()
                .map(|(child, _)| child)
                .collect()
        };
        let clock_only =
            AlcState::own_step(stamp, None, None, false, None, None, None).expect("step");
        assert_eq!(
            children_of(&clock_only),
            [ID_REL_CHILD, TIME_REL_CHILD, ID_ABS_CHILD, TIME_ABS_CHILD]
        );
        assert_eq!(clock_only.pos10, None);
        let pos = InputPosition::from_metres(1.0, 2.0, 3.0).expect("finite");
        let full = AlcState::own_step(
            stamp,
            Some(StepPlace::of(None, pos)),
            Some(DATA_BITS_ACTIVE),
            false,
            None,
            None,
            None,
        )
        .expect("step");
        assert_eq!(
            children_of(&full),
            [
                ID_REL_CHILD,
                TIME_REL_CHILD,
                POS_REL_CHILD,
                ID_ABS_CHILD,
                TIME_ABS_CHILD,
                POS10_CHILD,
                DATA_BITS_CHILD
            ]
        );
        assert_eq!(full.pos10, Some(pos));
        assert_eq!(
            full.bytes.last(),
            Some(&DATA_BITS_ACTIVE),
            "throttle bit 5 clear"
        );
        assert_eq!(DATA_BITS_ACTIVE & 0x20, 0);
        // Retail re-anchor (clock note): `00 03 01 2f ef 71 ce 55` after the time children.
        const RETAIL_ANCHOR_US: u64 = 0x0003_012f_ef71_ce55;
        let anchored = AlcState::own_step(
            AlcStamp {
                anchor_us: RETAIL_ANCHOR_US,
                ..stamp
            },
            None,
            None,
            true,
            None,
            None,
            None,
        )
        .expect("step");
        assert_eq!(
            children_of(&anchored),
            [
                ID_REL_CHILD,
                TIME_REL_CHILD,
                ID_ABS_CHILD,
                TIME_ABS_CHILD,
                TIME_ANCHOR_CHILD
            ]
        );
        assert_eq!(
            anchored.bytes[anchored.bytes.len() - 8..],
            RETAIL_ANCHOR_US.to_be_bytes()
        );
    }

    #[test]
    fn an_own_step_near_its_anchor_sends_the_offset_alone() {
        // Retail own state during the first lock (197015 ms): `10=60 8e 82`, no `pos10`.
        const RETAIL_OFFSET: [u8; POS3_LEN] = [0x60, 0x8e, 0x82];
        let stamp = AlcStamp {
            id: 0x0605,
            anchor_us: 0,
            offset_ms: 0x0304,
        };
        let anchor = InputPosition::from_metres(480.0, 210.0, 22.0).expect("finite");
        let steps = |byte: u8| (f32::from(byte) - f32::from(POS_REL_ZERO)) * POS_REL_STEP_M;
        let at = InputPosition::from_metres(
            anchor.x + steps(RETAIL_OFFSET[0]),
            anchor.y + steps(RETAIL_OFFSET[1]),
            anchor.z() + steps(RETAIL_OFFSET[2]),
        )
        .expect("finite");
        let place = StepPlace::of(Some(anchor), at);
        assert_eq!(place, StepPlace::Offset(RETAIL_OFFSET));
        let state =
            AlcState::own_step(stamp, Some(place), None, false, None, None, None).expect("step");
        assert_eq!(state.pos10, None, "the client keeps its pos10");
        let children = state.slot0_children().expect("walk");
        assert_eq!(
            children.iter().map(|(child, _)| *child).collect::<Vec<_>>(),
            [
                ID_REL_CHILD,
                TIME_REL_CHILD,
                POS_REL_CHILD,
                ID_ABS_CHILD,
                TIME_ABS_CHILD
            ]
        );
        assert!(children.contains(&(POS_REL_CHILD, RETAIL_OFFSET.to_vec())));
        let far = InputPosition::from_metres(anchor.x + 3.0, anchor.y, anchor.z()).expect("finite");
        let moved = AlcState::own_step(
            stamp,
            Some(StepPlace::of(Some(anchor), far)),
            None,
            false,
            None,
            None,
            None,
        )
        .expect("step");
        assert_eq!(moved.pos10, Some(far), "out of reach: a new pos10");
    }

    #[test]
    fn an_alc_body_with_slot_0_reads_pos10() {
        let bytes = body(&[POS10_CHILD], &[], 0);
        let (state, left) = read(&bytes).expect("read");
        assert_eq!(left, 0);
        let position = state.pos10.expect("pos10");
        assert_eq!(
            (position.x, position.y, position.z_raw),
            (POS10_X, POS10_Y, POS10_Z_RAW)
        );
        assert_eq!(state.bytes, bytes);
    }

    #[test]
    fn an_alc_body_with_both_slots_reads_to_its_end() {
        let mut bytes = body(&[0, 10, 11, POS10_CHILD, 53], &[2, 3, 7], 3);
        let end = bytes.len();
        bytes.push(0xAB);
        let (state, left) = read(&bytes).expect("read");
        assert_eq!(left, 1, "the next byte is not read");
        assert_eq!(state.bytes, bytes[..end]);
    }

    #[test]
    fn a_body_without_slots_is_one_byte() {
        let (state, left) = read(&[0, 0xAB]).expect("read");
        assert_eq!((state.bytes, state.pos10, left), (vec![0], None, 1));
    }

    #[test]
    fn alc_presence_bits_above_the_children_are_ignored() {
        let mut writer = Writer::new();
        writer.u8(SLOT_0).wide_prefixed(STRAY_BIT | 1).u8(7);
        let bytes = writer.finish();
        let (state, left) = read(&bytes).expect("read");
        assert_eq!((state.bytes.len(), left), (bytes.len(), 0));
    }

    #[test]
    fn an_alc_blob_over_its_limit_is_an_error() {
        const BLOB33_CHILD: usize = 29;
        assert_eq!(SLOT0_CHILDREN[BLOB33_CHILD], AlcChild::Blob33);
        let mut writer = Writer::new();
        writer
            .u8(SLOT_0)
            .wide_prefixed(1 << BLOB33_CHILD)
            .prefixed(u64::from(BLOB33_LIMIT))
            .expect("count")
            .bytes(&[0; BLOB33_LIMIT as usize]);
        assert!(matches!(
            read(&writer.finish()),
            Err(MessageError::CountTooLarge { .. })
        ));
    }

    #[test]
    fn a_short_alc_body_is_an_error() {
        let bytes = body(&[POS10_CHILD], &[], 0);
        assert!(matches!(
            read(&bytes[..bytes.len() - 1]),
            Err(MessageError::Truncated { .. })
        ));
    }

    #[test]
    fn each_quat_tag_gives_its_byte_count() {
        for tag in 0..=u8::MAX {
            let mut count: usize = if tag & 0b10 == 0 { 3 } else { 2 };
            if tag & 0b100 != 0 {
                count -= 1;
            }
            if tag & 0b1000 != 0 {
                count -= 1;
            }
            if tag & 0b1_0000 != 0 && count != 0 {
                count -= 1;
            }
            assert_eq!(quat_extra(tag), count, "tag {tag:#04x}");
        }
    }

    #[test]
    fn a_bounds_child_reads_its_flag_chain() {
        const BOUNDS_CHILD: usize = 7;
        assert_eq!(SLOT1_CHILDREN[BOUNDS_CHILD], AlcChild::Bounds);
        for seed in [0, 1] {
            let bytes = body(&[], &[BOUNDS_CHILD], seed);
            let (state, left) = read(&bytes).expect("read");
            assert_eq!((state.bytes, left), (bytes, 0));
        }
        let mut writer = Writer::new();
        writer.u8(SLOT_1).wide_prefixed(1 << BOUNDS_CHILD).u8(2);
        assert!(matches!(
            read(&writer.finish()),
            Err(MessageError::NotBoolean { value: 2 })
        ));
    }

    proptest! {
        #[test]
        fn an_alc_body_reads_and_writes_the_same_bytes(
            slot0 in any::<u64>(),
            slot1 in any::<u16>(),
            seed in any::<u8>(),
        ) {
            let slot0: Vec<usize> = (0..SLOT0_CHILDREN.len()).filter(|bit| slot0 >> bit & 1 == 1).collect();
            let slot1: Vec<usize> = (0..SLOT1_CHILDREN.len()).filter(|bit| slot1 >> bit & 1 == 1).collect();
            let bytes = body(&slot0, &slot1, seed);
            let (state, left) = read(&bytes).expect("read");
            prop_assert_eq!(left, 0);
            let mut writer = Writer::new();
            state.write(&mut writer);
            prop_assert_eq!(writer.finish(), bytes);
        }
    }

    #[test]
    fn decode_known_goes_past_alc() {
        let mut writer = Writer::new();
        writer.prefixed(GROUP).expect("group").u8(2);
        writer
            .prefixed(ENTITY)
            .expect("entity")
            .bytes(
                &nw_reflect::encode_type_id(&nw_reflect::TypeId::FileSlot(ALC_TYPE_INDEX))
                    .expect("class"),
            )
            .bytes(&body(&[POS10_CHILD], &[], 0));
        let position = crate::payload::BundleMember {
            component: ENTITY,
            state: MemberState::Position(PositionInTheWorldReplicatedState {
                position: WorldPosition::from_metres(1.0, 2.0, 3.0),
                ..PositionInTheWorldReplicatedState::default()
            }),
        };
        writer.bytes(&position.encode().expect("member"));
        let payload = writer.finish();
        let known = BundlePayload::decode_known(&payload).expect("decode");
        assert_eq!(known.stopped_at, None);
        let classes: Vec<u64> = known
            .members
            .iter()
            .map(|(member, _)| member.state.type_index())
            .collect();
        assert_eq!(
            classes,
            vec![ALC_TYPE_INDEX, POSITION_IN_THE_WORLD_TYPE_INDEX]
        );
        let whole = BundlePayload::decode(&payload).expect("whole");
        assert_eq!(whole.encode().expect("encode"), payload);
    }

    /// The child sets of the most common retail first own ALC (own-player-position note).
    const RETAIL_FIRST_SLOT0: u64 = 0x0021_ff3f_afff_ffff;
    const RETAIL_FIRST_SLOT1: u64 = 0x7f;

    #[test]
    fn the_spawn_state_has_the_retail_shape_and_our_position() {
        let pos = InputPosition {
            x: POS10_X,
            y: POS10_Y,
            z_raw: POS10_Z_RAW,
        };
        // Retail first own ALC anchor (clock note): 845730262399198 µs since 2000-01-01.
        const RETAIL_ANCHOR_US: u64 = 0x0003_012f_ebdd_b8de;
        let state = AlcState::spawn(
            CardPlace {
                anchor: None,
                at: pos,
            },
            AlcStamp {
                id: SPAWN_STATE_ID,
                anchor_us: RETAIL_ANCHOR_US,
                offset_ms: SPAWN_STATE_MS,
            },
        )
        .expect("spawn");
        // The retail first own state's number, time, slayer starts and sequence time.
        let children = state.slot0_children().expect("walk");
        for (child, retail) in [
            (ID_REL_CHILD, &[0x02][..]),
            (TIME_REL_CHILD, &[0x09]),
            (SEQUENCE_TIME_REL_CHILD, &[0x81]),
            (SEQUENCE_TIME_ABS_CHILD, &[0x28, 0x44]),
            (SLAYER_STARTED_CHILD, &[0x00, 0x01]),
            (SECOND_SLAYER_STARTED_CHILD, &[0x00, 0x01]),
            (ID_ABS_CHILD, &[0x00]),
            (TIME_ABS_CHILD, &[0x00]),
        ] {
            assert!(
                children.contains(&(child, retail.to_vec())),
                "child {child}"
            );
        }
        let at = state
            .bytes
            .windows(8)
            .position(|window| window == RETAIL_ANCHOR_US.to_be_bytes());
        assert!(at.is_some(), "the spawn state carries the anchor");
        let mut reader = Reader::new(&state.bytes);
        assert_eq!(reader.u8().expect("mask"), SLOT_BITS[0] | SLOT_BITS[1]);
        assert_eq!(reader.wide_prefixed().expect("slot 0"), RETAIL_FIRST_SLOT0);
        let mut whole = Reader::new(&state.bytes);
        let read = AlcState::read(&mut whole).expect("read");
        whole.finish().expect("the whole body is one state");
        assert_eq!(read, state);
        assert_eq!(read.pos10, Some(pos));
        let slot1_at = state.bytes.len() - SLOT1_BODY_LEN;
        let mut slot1 = Reader::new(&state.bytes[slot1_at..]);
        assert_eq!(slot1.wide_prefixed().expect("slot 1"), RETAIL_FIRST_SLOT1);
    }

    /// Slot 1 of the spawn state: a 1-byte presence integer and seven 1-byte children.
    const SLOT1_BODY_LEN: usize = 8;

    /// Sailor 1's first ALC (mob-cards note, section 2.1): `pos10` (467.617, 846.008, raw 11329).
    const RETAIL_SAILOR: &str = concat!(
        "01fe031c00be7fff2601007f7f7f6600187f000043e9cefc445380802c4100000101080001b902240000",
        "52040328088b800800000000000000000000000000000002ff753e0c5a7dc313000b00f77b26fc0200",
        "000040000000"
    );
    const SAILOR_RAW_Z: u16 = 11329;
    #[test]
    fn a_move_within_reach_is_an_offset_from_the_anchor() {
        let anchor = InputPosition::from_metres(477.096_92, 847.669_86, 89.417_86).expect("anchor");
        let at = |dx: f32, dy: f32| {
            InputPosition::from_metres(anchor.x + dx, anchor.y + dy, anchor.z()).expect("at")
        };
        assert_eq!(
            offset_from(anchor, anchor),
            Some([POS_REL_ZERO; POS3_LEN]),
            "no step"
        );
        // The reach is 127 steps of 4/255 m: 1.992 m either way.
        assert_eq!(offset_from(anchor, at(-1.99, 0.0)).map(|o| o[0]), Some(0));
        assert_eq!(offset_from(anchor, at(1.99, 0.0)).map(|o| o[0]), Some(0xfe));
        assert_eq!(offset_from(anchor, at(2.01, 0.0)), None, "out of reach");
        assert_eq!(
            StepPlace::of(Some(anchor), at(3.0, 0.0)),
            StepPlace::Anchor(at(3.0, 0.0)),
            "a far move is a new anchor"
        );
        let near = StepPlace::of(Some(anchor), at(0.5, -0.5));
        assert!(matches!(near, StepPlace::Offset(_)));
        assert_eq!(
            near.children()
                .iter()
                .map(|(child, _)| *child)
                .collect::<Vec<_>>(),
            [POS_REL_CHILD],
            "an offset sends no pos10"
        );
        // A card puts a late viewer on the same anchor, with the offset to where the mob is.
        let (children, pos10) = CardPlace {
            anchor: Some(anchor),
            at: at(0.5, -0.5),
        }
        .children();
        assert_eq!(pos10, anchor);
        assert_eq!(children[0], near.children()[0]);
        assert_eq!(children[1].0, POS10_CHILD);
    }

    #[test]
    fn an_offset_byte_is_its_steps_of_four_255ths_of_a_metre() {
        assert_eq!(offset_metres(AT_ANCHOR[0]), 0.0, "at the pos10");
        assert_eq!(offset_metres(POS_REL_ZERO), 0.0, "no step");
        assert!((offset_metres(0) + 127.0 * 4.0 / 255.0).abs() < 1e-6);
        assert!((offset_metres(0xfe) - 127.0 * 4.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn a_retail_offset_plus_one_step_lands_on_the_next_anchor() {
        // Retail walk (clean walking capture, own states 37 to 61): `pos10` of state 37, the
        // offsets of states 59 and 60, then state 61 with a new `pos10`.
        const FIRST_ANCHOR: [f32; 3] = [4232.022, 9452.522, 342.635];
        const STATE_59: [u8; POS3_LEN] = [0xdb, 0xf8, 0x81];
        const STATE_60: [u8; POS3_LEN] = [0xdf, 0xfd, 0x81];
        const NEXT_ANCHOR: [f32; 3] = [4233.588, 9454.587, 342.686];
        /// The new anchor is within this of the last offset plus one state's step.
        const WITHIN_M: f32 = 0.025;
        let steps = |byte: u8| f32::from(byte) - f32::from(POS_REL_ZERO);
        for axis in 0..POS3_LEN {
            let last = steps(STATE_60[axis]);
            let step = last - steps(STATE_59[axis]);
            let predicted = FIRST_ANCHOR[axis] + (last + step) * POS_REL_STEP_M;
            assert!(
                (predicted - NEXT_ANCHOR[axis]).abs() < WITHIN_M,
                "axis {axis}: {predicted} vs {}",
                NEXT_ANCHOR[axis]
            );
        }
    }

    #[test]
    fn the_anchor_again_sends_the_pos10_with_its_offset_as_retail_did() {
        // Retail clean walking capture, own state 62 after the new pos10 of state 61: children
        // 0, 1, 2, 10 and 27, offset `83 84 80` from the same pos10 (child 2, the slayer
        // sequence time, is not part of the place).
        const RETAIL_OFFSET: [u8; POS3_LEN] = [0x83, 0x84, 0x80];
        let anchor = InputPosition::from_metres(4233.588, 9454.587, 342.686).expect("finite");
        let state = AlcState::own_step(
            STAMP,
            Some(StepPlace::AnchorAgain {
                anchor,
                offset: RETAIL_OFFSET,
            }),
            None,
            false,
            None,
            None,
            None,
        )
        .expect("step");
        assert_eq!(state.pos10, Some(anchor), "the client keeps the same pos10");
        let children = state.slot0_children().expect("walk");
        let numbers: Vec<usize> = children.iter().map(|(child, _)| *child).collect();
        assert_eq!(
            numbers,
            [
                ID_REL_CHILD,
                TIME_REL_CHILD,
                POS_REL_CHILD,
                ID_ABS_CHILD,
                TIME_ABS_CHILD,
                POS10_CHILD
            ]
        );
        let offset = children
            .iter()
            .find(|(child, _)| *child == POS_REL_CHILD)
            .map(|(_, bytes)| bytes.clone());
        assert_eq!(offset, Some(RETAIL_OFFSET.to_vec()));
    }

    const STAMP: AlcStamp = AlcStamp {
        id: 0x0102,
        anchor_us: 845_730_278_284_365,
        offset_ms: 0x0304,
    };
    /// A retail `Move_Combat` entry of the sailor (mob-animation note, section 4).
    const MOVE_ENTRY: ScopeEntry = ScopeEntry {
        start_id: 0x0393,
        fragment: 182,
        action: 26,
        tail: 0xeb,
    };
    const MOVE_ENTRY_BLOB: [u8; 9] = [8, 0x03, 0x93, 0xb6, 0x02, 0x1a, 0x00, 0x00, 0xeb];
    /// The retail tags of an alert sailor.
    const ALERT_TAGS: [u8; FRAG_TAGS_LEN] = [
        0x04, 0x03, 0x28, 0x08, 0x8b, 0x82, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    const TICKS: u32 = 0x0102_0304;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    /// The bytes of each slot-0 child of `state`.
    fn children_of(state: &AlcState) -> Vec<(usize, Vec<u8>)> {
        state.slot0_children().expect("walk")
    }

    fn child(state: &AlcState, index: usize) -> Option<Vec<u8>> {
        children_of(state)
            .into_iter()
            .find(|(child, _)| *child == index)
            .map(|(_, bytes)| bytes)
    }

    #[test]
    fn a_facing_has_the_retail_bytes() {
        // Retail samples (rotation note, sections 2 and 3), by heading (degrees from east,
        // counter-clockwise); the yaw is the heading minus 90 degrees.
        let yaw_of_heading = |heading: f32| (heading - 90.0).to_radians();
        assert_eq!(rotation_bytes(yaw_of_heading(0.0)), [0x66, 0x00]);
        assert_eq!(rotation_bytes(yaw_of_heading(18.4)), [0x66, 0x16]);
        assert_eq!(rotation_bytes(yaw_of_heading(90.0)), [QUAT_TAG_NORTH]);
        assert_eq!(rotation_bytes(yaw_of_heading(114.7)), [0x66, 0xa6]);
        assert_eq!(rotation_bytes(yaw_of_heading(-105.6)), [0x46, 0x98]);
    }

    #[test]
    fn a_scope_entry_has_the_retail_bytes() {
        assert_eq!(info_blob(&[MOVE_ENTRY]).expect("blob"), MOVE_ENTRY_BLOB);
    }

    #[test]
    fn a_retail_mob_state_moves_to_another_spawn_point() {
        let bytes = hex(RETAIL_SAILOR);
        let retail = AlcState::read(&mut Reader::new(&bytes)).expect("read");
        assert_eq!(retail.bytes, bytes);
        assert_eq!(retail.pos10.map(|pos| pos.z_raw), Some(SAILOR_RAW_Z));
        let to = InputPosition::from_metres(468.734, 847.221, 89.99).expect("position");
        let moved = retail
            .mob_first(
                STAMP,
                CardPlace {
                    anchor: None,
                    at: to,
                },
                None,
                None,
            )
            .expect("encode")
            .expect("moved");
        assert_eq!(moved.bytes.len(), bytes.len());
        let again = AlcState::read(&mut Reader::new(&moved.bytes)).expect("read");
        assert_eq!(again, moved);
        assert_eq!(again.pos10, Some(to));
        // Children 0, 1, 25 and 26 hold our number and clock; child 10 puts it at its pos10.
        let ids: Vec<Option<Vec<u8>>> =
            [ID_REL_CHILD, TIME_REL_CHILD, ID_ABS_CHILD, TIME_ABS_CHILD]
                .into_iter()
                .map(|index| child(&moved, index))
                .collect();
        assert_eq!(
            ids,
            [vec![0x02], vec![0x04], vec![0x01], vec![0x03]].map(Some)
        );
        assert_eq!(child(&moved, POS_REL_CHILD), Some(AT_ANCHOR.to_vec()));
        assert_eq!(
            child(&moved, TIME_ANCHOR_CHILD),
            Some(STAMP.anchor_us.to_be_bytes().to_vec())
        );
        // Without the recorded scope children, they keep the recorded bytes.
        assert_eq!(
            child(&moved, SCOPE_INFO_CHILD),
            child(&retail, SCOPE_INFO_CHILD)
        );
        assert_eq!(
            AlcState::at_position(to)
                .mob_first(
                    STAMP,
                    CardPlace {
                        anchor: None,
                        at: to
                    },
                    None,
                    None
                )
                .expect("encode")
                .map(|alc| alc.pos10),
            Some(Some(to))
        );
    }

    /// The card of retail sailor 1 at its spawn (newchar-retail-2026-09-18, 16319 ms).
    const RETAIL_FRESH_SAILOR_CARD: &str = concat!(
        "01fe031c00ae3fff200000ffffff66003a000043e9cefc445380802c410001000000000000000000000000",
        "0000000000000000000000000000000000001010000b00f3dfc6f61d0040000000"
    );

    #[test]
    fn a_fresh_spawn_card_has_no_number_clock_or_scope() {
        let template = AlcState::read(&mut Reader::new(&hex(RETAIL_SAILOR))).expect("read");
        let fresh = AlcState::read(&mut Reader::new(&hex(RETAIL_FRESH_SAILOR_CARD))).expect("read");
        let at = InputPosition::from_metres(467.617, 846.008, 89.42).expect("position");
        let card = template.mob_spawn_card(at).expect("a card");
        assert_eq!(
            AlcState::read(&mut Reader::new(&card.bytes)).expect("read"),
            card
        );
        assert_eq!(card.pos10, Some(at));
        for index in [
            ID_REL_CHILD,
            TIME_REL_CHILD,
            POS_REL_CHILD,
            ID_ABS_CHILD,
            TIME_ABS_CHILD,
            SCOPE_TIME_EX_CHILD,
            SCOPE_DATA_CHILD,
            SCOPE_INFO_CHILD,
            FRAG_TAGS_CHILD,
            FACING_GAP_CHILD,
            FRESH_CARD_HALVES[0],
            FRESH_CARD_HALVES[1],
            TIME_ANCHOR_CHILD,
            LOOK_DIR_CHILD,
            TELEPORT_CHILD,
            DATA_BITS_CHILD,
            42,
            43,
            45,
            46,
            47,
            53,
        ] {
            assert_eq!(child(&card, index), child(&fresh, index), "child {index}");
        }
        // The prefab's own facing stays.
        assert_eq!(child(&card, 11), child(&template, 11));
        assert!(card.is_fresh_card() && fresh.is_fresh_card());
        assert!(!template.is_fresh_card(), "a state of a mob already alive");
        assert_eq!(
            AlcState::at_position(at)
                .mob_spawn_card(at)
                .map(|card| card.pos10),
            Some(Some(at))
        );
    }

    #[test]
    fn a_first_mob_state_shows_our_animation() {
        let retail = AlcState::read(&mut Reader::new(&hex(RETAIL_SAILOR))).expect("read");
        let to = InputPosition::from_metres(468.734, 847.221, 89.99).expect("position");
        let animation = MobAnimation {
            entry: MOVE_ENTRY,
            ticks: TICKS,
            tags: ALERT_TAGS,
            look_pose: None,
            facing_gap: None,
        };
        let first = retail
            .mob_first(
                STAMP,
                CardPlace {
                    anchor: None,
                    at: to,
                },
                None,
                Some(&animation),
            )
            .expect("encode")
            .expect("first");
        assert_eq!(
            AlcState::read(&mut Reader::new(&first.bytes)).expect("read"),
            first
        );
        assert_eq!(
            child(&first, SCOPE_INFO_CHILD),
            Some(MOVE_ENTRY_BLOB.to_vec())
        );
        assert_eq!(child(&first, FRAG_TAGS_CHILD), Some(ALERT_TAGS.to_vec()));
        assert_eq!(child(&first, SCOPE_DATA_CHILD), Some(vec![1]));
        assert_eq!(child(&first, SCOPE_TIME_CHILDREN[0]), Some(vec![0x04]));
        // The other children keep the recorded bytes.
        assert_eq!(child(&first, 46), child(&retail, 46));
    }

    #[test]
    fn an_alert_with_its_look_pose_has_the_retail_children() {
        // Retail legionnaire alert (204711 of the tutorial capture; creature-animation note,
        // section 1): 29 = `02 3556`, 32 = `11`, 33 = alert and look pose, 35 = 1.774 rad.
        const RETAIL_GAP_BITS: u32 = 0x3fe3_0d9d;
        const START: u16 = 0x56;
        let alert = MobAnimation {
            entry: ScopeEntry {
                start_id: START,
                fragment: 7,
                action: 12,
                tail: 0x53,
            },
            ticks: 0,
            tags: ALERT_TAGS,
            look_pose: Some(LayerEntry {
                scope_bit: 4,
                entry: ScopeEntry {
                    start_id: START,
                    fragment: 45,
                    action: 25,
                    tail: 0xe9,
                },
                seconds: 0.3335,
            }),
            facing_gap: Some(f32::from_bits(RETAIL_GAP_BITS)),
        };
        let state = AlcState::mob_step(&MobStep {
            stamp: STAMP,
            anchored: false,
            position: None,
            facing: None,
            scope: Some(StepScope::Changed(alert)),
        })
        .expect("step");
        assert_eq!(child(&state, SCOPE_TIME_EX_CHILD), Some(hex("023556")));
        assert_eq!(child(&state, SCOPE_DATA_CHILD), Some(vec![0x11]));
        assert_eq!(
            child(&state, SCOPE_INFO_CHILD),
            Some(hex("0e0056070c00005300562d190000e9"))
        );
        assert_eq!(child(&state, FACING_GAP_CHILD), Some(hex("3fe30d9d")));
        let later = AlcState::mob_step(&MobStep {
            stamp: STAMP,
            anchored: false,
            position: None,
            facing: None,
            scope: Some(StepScope::Ticks {
                ticks: 1,
                high_changed: false,
                look_pose_seconds: Some(0.367),
            }),
        })
        .expect("step");
        assert_eq!(child(&later, SCOPE_TIME_EX_CHILD), Some(hex("0235df")));
    }

    #[test]
    fn halves_round_to_the_nearest() {
        assert_eq!(half_bits(0.0), 0);
        assert_eq!(half_bits(1.0), 0x3c00);
        assert_eq!(half_bits(1.0 / 3.0), 0x3555);
        assert_eq!(half_bits(-2.0), 0xc000);
        assert_eq!(half_bits(65_504.0), 0x7bff, "the largest half");
        assert_eq!(half_bits(1.0e6), 0x7c00, "too large: infinity");
        assert_eq!(half_bits(2.0_f32.powi(-24)), 1, "the smallest subnormal");
        assert_eq!(half_bits(2.0_f32.powi(-26)), 0, "below it: zero");
        // 1 + 2^-11 is half-way between two halves: ties go to the even one.
        assert_eq!(half_bits(1.0 + 2.0_f32.powi(-11)), 0x3c00);
        assert_eq!(half_bits(1.0 + 3.0 * 2.0_f32.powi(-11)), 0x3c02);
    }

    #[test]
    fn a_stopped_second_slot_writes_retails_pull_children() {
        // Retail newchar 2026-09-12, Thorpe's grab pull: `13=04 14=3163 15=8a37 16=00 17=3163`.
        const STARTED: u16 = 0x3163;
        let pulled = SlayerState {
            state: Some(4),
            started: Some(STARTED),
            sequence: 3530,
            second: Some(SecondSlayerState {
                started: STARTED,
                playing: None,
                stopped: true,
            }),
        };
        let mut children = pulled.children().expect("children");
        children.sort();
        assert_eq!(
            children,
            [
                (SLAYER_STATE_CHILD, vec![0x04]),
                (SLAYER_STARTED_CHILD, vec![0x31, 0x63]),
                (SLAYER_SEQUENCE_CHILD, vec![0x8a, 0x37]),
                (SECOND_SLAYER_STATE_CHILD, vec![0x00]),
                (SECOND_SLAYER_STARTED_CHILD, vec![0x31, 0x63]),
            ]
        );
    }

    #[test]
    fn a_teleport_sends_the_three_retail_states() {
        // Retail water move (118546, 118580, 118614 ms): pos10 `444675eb43a8b6081d5c`, facing
        // `66f6`, then zero offset with teleport number 2.
        const RETAIL_POS10: &str = "444675eb43a8b6081d5c";
        const RETAIL_FACING: [u8; 2] = [0x66, 0xf6];
        const WATER_YAW: f32 = 1.434;
        let pos = InputPosition::read(&mut Reader::new(&hex(RETAIL_POS10))).expect("pos10");
        let state = |step| {
            let state =
                AlcState::teleport(STAMP, pos, Some(WATER_YAW), SPAWN_TELEPORT + 1, step, None);
            assert_eq!(
                AlcState::read(&mut Reader::new(&state.bytes)).expect("read"),
                state
            );
            state
        };
        let present = |state: &AlcState| -> Vec<usize> {
            children_of(state).into_iter().map(|(c, _)| c).collect()
        };
        let place = state(TeleportStep::Place);
        assert_eq!(present(&place), [0, 1, 10, 11, 25, 26, 27]);
        assert_eq!(child(&place, POS_REL_CHILD), Some(AT_ANCHOR.to_vec()));
        assert_eq!(child(&place, ROTATION_CHILD), Some(RETAIL_FACING.to_vec()));
        assert_eq!(child(&place, POS10_CHILD), Some(hex(RETAIL_POS10)));
        let confirm = state(TeleportStep::Confirm);
        assert_eq!(
            present(&confirm),
            [0, 1, 10, 11, 25, 26, 27, TELEPORT_CHILD]
        );
        assert_eq!(child(&confirm, POS_REL_CHILD), Some(ZERO_POS3.to_vec()));
        assert_eq!(child(&confirm, TELEPORT_CHILD), Some(vec![2]));
        let hold = state(TeleportStep::Hold);
        assert_eq!(present(&hold), [0, 1, 10, 25, 26, TELEPORT_CHILD]);
        assert_eq!(child(&hold, POS_REL_CHILD), Some(ZERO_POS3.to_vec()));
        assert_eq!(TeleportStep::Place.next(), Some(TeleportStep::Confirm));
        assert_eq!(TeleportStep::Hold.next(), None);
    }

    #[test]
    fn a_mob_step_sends_only_its_parts() {
        let to = InputPosition::from_metres(470.0, 848.0, 90.0).expect("position");
        let walking = AlcState::mob_step(&MobStep {
            stamp: STAMP,
            anchored: false,
            position: Some(StepPlace::Anchor(to)),
            facing: None,
            scope: Some(StepScope::Ticks {
                ticks: TICKS,
                high_changed: false,
                look_pose_seconds: None,
            }),
        })
        .expect("step");
        assert_eq!(
            AlcState::read(&mut Reader::new(&walking.bytes)).expect("read"),
            walking
        );
        let present: Vec<usize> = children_of(&walking).into_iter().map(|(c, _)| c).collect();
        assert_eq!(present, [0, 1, 10, 25, 26, 27, 28]);
        let changed = AlcState::mob_step(&MobStep {
            stamp: STAMP,
            anchored: true,
            position: None,
            facing: None,
            scope: Some(StepScope::Changed(MobAnimation {
                entry: MOVE_ENTRY,
                ticks: 0,
                tags: ALERT_TAGS,
                look_pose: None,
                facing_gap: None,
            })),
        })
        .expect("step");
        let present: Vec<usize> = children_of(&changed).into_iter().map(|(c, _)| c).collect();
        assert_eq!(present, [0, 1, 25, 26, 28, 29, 32, 33, 34, 38, 40, 49, 50]);
        assert_eq!(changed.pos10, None);
        let idle = AlcState::mob_step(&MobStep {
            stamp: STAMP,
            anchored: false,
            position: None,
            facing: None,
            scope: None,
        })
        .expect("step");
        let present: Vec<usize> = children_of(&idle).into_iter().map(|(c, _)| c).collect();
        assert_eq!(present, [0, 1, 25, 26]);
    }

    #[test]
    fn an_own_state_carries_back_the_view_of_the_input() {
        // Retail own state of the duel capture with its input's view (yaw, pitch); the own
        // states around the first lock of the newchar capture sent `rotation` and `lookDir`.
        let view = LookDir {
            yaw: -1.549_167_8,
            pitch: 0.269_884_56,
        };
        let stamp = AlcStamp {
            id: 0x0102,
            offset_ms: 0x0304,
            anchor_us: 0,
        };
        let with_view =
            AlcState::own_step(stamp, None, None, false, None, Some(view), None).expect("step");
        let children = with_view.slot0_children().expect("walk");
        let child = |index: usize| {
            children
                .iter()
                .find(|(child, _)| *child == index)
                .map(|(_, bytes)| bytes.clone())
        };
        assert_eq!(child(ROTATION_CHILD), Some(rotation_bytes(view.yaw)));
        assert_eq!(child(LOOK_DIR_CHILD), Some(view.bytes()));
        let without = AlcState::own_step(stamp, None, None, false, None, None, None).expect("step");
        assert!(
            without
                .slot0_children()
                .expect("walk")
                .iter()
                .all(|(child, _)| *child != ROTATION_CHILD && *child != LOOK_DIR_CHILD),
            "no view, no facing or look"
        );
    }

    #[test]
    fn the_look_direction_is_the_retail_direction_vector() {
        // Retail own states of the duel capture with the input's view angles (yaw, pitch):
        // `lookDir` 0x10 = x derived, y and z bytes (w flagged 0); 0x18 = x derived, z also 0.
        let cases = [
            (
                (-1.549_167_8, 0.269_884_56),
                [0.962_950_7, 0.019_410_789, -0.268_977_85],
            ),
            (
                (-1.169_047_6, 0.079_825_92),
                [0.918_719, 0.385_442_55, -0.085_962],
            ),
        ];
        for ((yaw, pitch), retail) in cases {
            let bytes = LookDir { yaw, pitch }.bytes();
            assert_eq!(bytes.len(), 3, "a tag and two parts");
            assert_eq!(bytes[0], 0x10, "x derived, w zero");
            // The state and the input were not sampled at one instant: two steps of slack.
            let part = |byte: u8| f32::from(byte) * QUAT_STEP - QUAT_OFFSET;
            assert!(
                (part(bytes[1]) - retail[1]).abs() <= 2.0 * QUAT_STEP,
                "y {yaw}"
            );
            assert!(
                (part(bytes[2]) - retail[2]).abs() <= 2.0 * QUAT_STEP,
                "z {yaw}"
            );
        }
        // Level, looking east-north-east: x derived, y kept, z and w flagged 0 (retail `181b`).
        let level = LookDir {
            yaw: -2.191,
            pitch: 0.0,
        };
        assert_eq!(level.bytes()[0], 0x18);
        assert_eq!(level.bytes().len(), 2);
        // Looking south and down: y derived and negative.
        let south = LookDir {
            yaw: std::f32::consts::PI,
            pitch: 0.2,
        };
        assert_eq!(south.bytes()[0] & !QUAT_TAG_NEGATIVE, 0x30);
        assert_eq!(south.bytes()[0] & QUAT_TAG_NEGATIVE, QUAT_TAG_NEGATIVE);
    }

    #[test]
    fn the_own_state_echoes_the_action_the_client_started() {
        // Retail newchar capture, 93314 ms: input record 2286 (`0x08ee`) row 1
        // `[1038, 0x0024001b, 5008]`; the next own state (93348 ms) carried `13=1b 14=08ee
        // 15=8e10 16=25 17=08ee 18=904e`.
        const STARTED: u16 = 0x08ee;
        let input = SlayerInput {
            sequence: 1038,
            state: 0x1b,
            second_state: 0x24,
            second_sequence: 5008,
        };
        let stamp = AlcStamp {
            id: STARTED,
            anchor_us: 0,
            offset_ms: 0,
        };
        let slayer = SlayerState::of_input(input, STARTED);
        let state =
            AlcState::own_step(stamp, None, None, false, Some(slayer), None, None).expect("step");
        let slayer_children: Vec<(usize, Vec<u8>)> = state
            .slot0_children()
            .expect("walk")
            .into_iter()
            .filter(|(child, _)| {
                (SLAYER_STATE_CHILD..=SECOND_SLAYER_SEQUENCE_CHILD).contains(child)
            })
            .collect();
        assert_eq!(
            slayer_children,
            [
                (SLAYER_STATE_CHILD, vec![0x1b]),
                (SLAYER_STARTED_CHILD, vec![0x08, 0xee]),
                (SLAYER_SEQUENCE_CHILD, vec![0x8e, 0x10]),
                (SECOND_SLAYER_STATE_CHILD, vec![0x25]),
                (SECOND_SLAYER_STARTED_CHILD, vec![0x08, 0xee]),
                (SECOND_SLAYER_SEQUENCE_CHILD, vec![0x90, 0x4e]),
            ]
        );
        // Retail 78664 ms: row 1 `[1221, 0xffff000a, 0]`: `14=0737 15=8513 17=0737` alone. And
        // 206408 ms: row 1 `[5055, 0x002c000a, 0]`, state 10 after 27: `14=15c0 15=bf4e
        // 17=15c0` alone, no 13.
        let plain_actions = [
            (
                SlayerInput {
                    sequence: 1221,
                    state: 0x0a,
                    second_state: 0xffff,
                    second_sequence: 0,
                },
                0x0737,
            ),
            (
                SlayerInput {
                    sequence: 5055,
                    state: 0x0a,
                    second_state: 0x2c,
                    second_sequence: 0,
                },
                0x15c0,
            ),
        ];
        for (plain, started) in plain_actions {
            let state = AlcState::own_step(
                stamp,
                None,
                None,
                false,
                Some(SlayerState::of_input(plain, started)),
                None,
                None,
            )
            .expect("step");
            let children: Vec<usize> = state
                .slot0_children()
                .expect("walk")
                .into_iter()
                .map(|(child, _)| child)
                .filter(|child| (SLAYER_STATE_CHILD..=SECOND_SLAYER_SEQUENCE_CHILD).contains(child))
                .collect();
            assert_eq!(
                children,
                [
                    SLAYER_STARTED_CHILD,
                    SLAYER_SEQUENCE_CHILD,
                    SECOND_SLAYER_STARTED_CHILD
                ],
                "{plain:?}"
            );
        }
    }

    #[test]
    fn a_remote_state_carries_the_retail_slayer_and_place_children() {
        // Retail remote player in the duel capture (51302 ms): `idRel` c5, `timeOffsetRel` fd,
        // `worldPosRel` at the anchor, `rotation`, slayer state 1a started in state 28c5 with
        // sequence ab18, and `worldPosAbs`.
        let retail = AlcState::read(&mut Reader::new(&hex(
            "01e7c00e80c5fd7fffffff47441a28c5ab1846121ebc452b6f312ba9",
        )))
        .expect("read");
        let retail_child = |index| child(&retail, index).expect("retail child");
        let prefix_of = |bytes: Vec<u8>| Reader::new(&bytes).prefixed().expect("prefix");
        let slayer = SlayerState {
            state: Some(prefix_of(retail_child(SLAYER_STATE_CHILD))),
            started: Some(u16::from_be_bytes([0x28, 0xc5])),
            sequence: prefix_of(retail_child(SLAYER_SEQUENCE_CHILD)),
            second: None,
        };
        let step = RemoteStep {
            stamp: AlcStamp {
                id: 0x28c5,
                anchor_us: STAMP.anchor_us,
                offset_ms: 0x00fd,
            },
            position: retail.pos10.map(StepPlace::Anchor),
            facing: Some(0.5),
            look: Some(LookDir {
                yaw: 0.25,
                pitch: 0.0,
            }),
            slayer: Some(slayer),
            sequence_time: Some(SequenceTime {
                rel: retail_child(SEQUENCE_TIME_REL_CHILD)[0],
                abs: None,
            }),
            upper: None,
        };
        let ours = AlcState::remote_step(&step).expect("step");
        for index in [
            ID_REL_CHILD,
            TIME_REL_CHILD,
            SEQUENCE_TIME_REL_CHILD,
            POS_REL_CHILD,
            SLAYER_STATE_CHILD,
            SLAYER_STARTED_CHILD,
            SLAYER_SEQUENCE_CHILD,
            POS10_CHILD,
        ] {
            assert_eq!(
                child(&ours, index),
                Some(retail_child(index)),
                "child {index}"
            );
        }
        assert_eq!(
            child(&ours, TIME_ANCHOR_CHILD),
            Some(STAMP.anchor_us.to_be_bytes().to_vec())
        );
        assert_eq!(child(&ours, ROTATION_CHILD), Some(rotation_bytes(0.5)));
        assert_eq!(
            child(&ours, LOOK_DIR_CHILD),
            Some(
                LookDir {
                    yaw: 0.25,
                    pitch: 0.0
                }
                .bytes()
            )
        );
        assert_eq!(ours.pos10, retail.pos10);
        let bare = AlcState::remote_step(&RemoteStep {
            position: None,
            facing: None,
            look: None,
            slayer: None,
            sequence_time: None,
            ..step
        })
        .expect("step");
        let present: Vec<usize> = children_of(&bare).into_iter().map(|(c, _)| c).collect();
        assert_eq!(
            present,
            [0, 1, 25, 26, 40],
            "a state is never without its time"
        );
    }

    /// The retail newchar sequence started at record 0x2aa (20 Hz records of 0.05 s): the
    /// `slayerSeqTimeRel` of its states at records 0x2ac to 0x2d1, and the absolute part the
    /// first two carried.
    const RETAIL_RELS_FROM_0X2AC: [u8; 38] = [
        0x85, 0x89, 0x8c, 0x8f, 0x92, 0x95, 0x99, 0x9c, 0x9f, 0xa2, 0xa5, 0xa8, 0xac, 0xaf, 0xb2,
        0xb5, 0xb8, 0xbc, 0xbf, 0xc2, 0xc5, 0xc8, 0xcc, 0xcf, 0xd2, 0xd5, 0xd8, 0xdb, 0xdf, 0xe2,
        0xe5, 0xe8, 0xeb, 0xef, 0xf2, 0xf5, 0xf8, 0xfb,
    ];
    const RETAIL_ABS_0X2AA: u16 = 0x2a66;
    const TUTORIAL_FRAME_S: f32 = 0.05;
    /// Records from the start to the first state that carried it.
    const FIRST_SENT_AFTER: usize = 2;
    /// Our own states' spacing, ms (one per 30 Hz tick).
    const TICK_MS: u64 = 33;

    #[test]
    fn the_sequence_time_counts_the_records_since_the_start_as_retail() {
        let mut clock = SequenceClock::default();
        let mut seconds = TUTORIAL_FRAME_S;
        for _ in 0..FIRST_SENT_AFTER {
            seconds += TUTORIAL_FRAME_S;
        }
        let mut state_ms = 0;
        let mut sent = Vec::new();
        for (index, _) in RETAIL_RELS_FROM_0X2AC.iter().enumerate() {
            let started = (index == 0).then_some(TUTORIAL_FRAME_S);
            sent.push(clock.time(seconds, started, state_ms));
            seconds += TUTORIAL_FRAME_S;
            state_ms += TICK_MS;
        }
        let rels: Vec<u8> = sent.iter().map(|time| time.rel).collect();
        assert_eq!(rels, RETAIL_RELS_FROM_0X2AC);
        let abs: Vec<Option<u16>> = sent.iter().take(3).map(|time| time.abs).collect();
        assert_eq!(
            abs,
            [Some(RETAIL_ABS_0X2AA), Some(RETAIL_ABS_0X2AA), None],
            "the start's absolute part goes again in the next state"
        );
    }

    #[test]
    fn a_long_sequence_takes_a_new_absolute_part_every_two_seconds() {
        // Retail sequence at 0x6bb3 (30 Hz): `7f 4022` at 2.0667 s.
        const RETAIL_REBASE_ABS: u16 = 0x4022;
        const FRAME_S: f32 = 1.0 / 30.0;
        const REBASE_RECORD: usize = 61;
        let mut clock = SequenceClock::default();
        let mut seconds = 0.0;
        let mut first_new = None;
        for record in 0..=REBASE_RECORD {
            seconds += FRAME_S;
            let started = (record == 0).then_some(FRAME_S);
            let time = clock.time(seconds, started, record as u64 * TICK_MS);
            if record > 0 && time.rel == SEQUENCE_TIME_REL_ZERO && first_new.is_none() {
                first_new = Some((record, time.abs));
            }
        }
        let (record, abs) = first_new.expect("a new absolute part");
        // One state before retail's: the exact half step (see `SequenceClock::time`).
        assert_eq!(record, REBASE_RECORD - 1);
        assert_eq!(
            abs.map(half_value).map(|value| (value * 1000.0).round()),
            Some(2033.0)
        );
        assert!((half_value(RETAIL_REBASE_ABS) - 2.066).abs() < 0.001);
        let next = clock.time(
            seconds + FRAME_S,
            None,
            (REBASE_RECORD as u64 + 1) * TICK_MS,
        );
        assert_eq!(
            next.abs, None,
            "two ticks on, the part is too old to go again"
        );
    }

    #[test]
    fn the_first_own_state_starts_the_clock() {
        let mut clock = SequenceClock::after_spawn();
        let seconds = SequenceClock::spawn_seconds();
        assert!((seconds - 0.0647).abs() < 0.0005, "{seconds}");
        let time = clock.time(seconds + 1.0 / 30.0, None, 0);
        assert_eq!(
            time,
            SequenceTime {
                rel: 0x83,
                abs: None
            }
        );
    }

    #[test]
    fn own_and_remote_steps_carry_the_sequence_time() {
        let time = SequenceTime {
            rel: 0x85,
            abs: Some(RETAIL_ABS_0X2AA),
        };
        let own =
            AlcState::own_step(STAMP, None, None, false, None, None, Some(time)).expect("step");
        assert_eq!(child(&own, SEQUENCE_TIME_REL_CHILD), Some(vec![0x85]));
        assert_eq!(child(&own, SEQUENCE_TIME_ABS_CHILD), Some(vec![0x2a, 0x66]));
        let without_abs = AlcState::own_step(
            STAMP,
            None,
            None,
            false,
            None,
            None,
            Some(SequenceTime { abs: None, ..time }),
        )
        .expect("step");
        assert_eq!(child(&without_abs, SEQUENCE_TIME_ABS_CHILD), None);
        let teleport = AlcState::teleport(
            STAMP,
            InputPosition::from_metres(1.0, 2.0, 3.0).expect("finite"),
            None,
            SPAWN_TELEPORT,
            TeleportStep::Hold,
            Some(time),
        );
        assert_eq!(child(&teleport, SEQUENCE_TIME_REL_CHILD), Some(vec![0x85]));
    }
}
