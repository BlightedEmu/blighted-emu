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

use crate::client_input::InputPosition;
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
/// Slot 0 child of the offset from `pos10` (`worldPosRel`).
const POS_REL_CHILD: usize = 10;
/// Slot 0 child of where a character looks (`lookDir`, a quat).
const LOOK_DIR_CHILD: usize = 12;
/// Slot 0 children of a character's first slayer state slot: `slayerStateId` (prefix),
/// `slayerStateIdStarted` (`u16`: the ALC state number in which it started; retail remote duel
/// `14=28ad` with `idRel` `ad`) and `slayerSequenceId` (prefix) (replication note).
const SLAYER_STATE_CHILD: usize = 13;
const SLAYER_STARTED_CHILD: usize = 14;
const SLAYER_SEQUENCE_CHILD: usize = 15;
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
/// `dataBits` of a retail own state without the throttle bit (40583 ms, then 998 `00`).
pub const DATA_BITS_ACTIVE: u8 = 211;
/// A `worldPosRel` that puts the entity at its `pos10`.
const AT_ANCHOR: [u8; POS3_LEN] = [0xff; POS3_LEN];
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
fn half_bits(value: f32) -> u16 {
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

/// One later state of a mob (sailor-movement note, section 5; mob-animation note, section 5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobStep {
    /// Its number and time.
    pub stamp: AlcStamp,
    /// The anchor of `stamp` is new and goes with the state.
    pub anchored: bool,
    /// Where the mob is now, when it moved.
    pub position: Option<InputPosition>,
    /// Its new facing (yaw, radians), when it turned.
    pub facing: Option<f32>,
    /// Its animation, for a mob whose animations are known.
    pub scope: Option<StepScope>,
}

/// The first slayer state of a character (children 13 to 15): the slayer script state it is in,
/// the ALC state number in which that state started, and the sequence of the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlayerState {
    /// `slayerStateId`.
    pub state: u64,
    /// `slayerStateIdStarted`.
    pub started: u16,
    /// `slayerSequenceId`.
    pub sequence: u64,
}

impl SlayerState {
    fn children(self) -> Result<[(usize, Vec<u8>); 3], MessageError> {
        Ok([
            (SLAYER_STATE_CHILD, prefixed_bytes(self.state)?),
            (SLAYER_STARTED_CHILD, self.started.to_be_bytes().to_vec()),
            (SLAYER_SEQUENCE_CHILD, prefixed_bytes(self.sequence)?),
        ])
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
    /// Where the player is, when it goes with the state.
    pub position: Option<InputPosition>,
    /// Its facing (yaw, radians).
    pub facing: Option<f32>,
    /// Where it looks (yaw, radians).
    pub look: Option<f32>,
    /// Its slayer state, when it goes with the state.
    pub slayer: Option<SlayerState>,
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
const POS3_LEN: usize = 3;
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
        pos: InputPosition,
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
        values.extend(position_children(pos));
        values.push((TIME_ANCHOR_CHILD, stamp.anchor_us.to_be_bytes().to_vec()));
        if let Some(yaw) = facing {
            values.push((ROTATION_CHILD, rotation_bytes(yaw)));
        }
        if let Some(animation) = animation {
            values.extend(animation.children()?);
        }
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
        Ok(Some(Self {
            bytes,
            pos10: Some(pos),
        }))
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
        if let Some(pos) = step.position {
            children.extend(position_children(pos));
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
        Ok(Self::of_slot0(children, step.position))
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
        if let Some(pos) = step.position {
            children.extend(position_children(pos));
        }
        if let Some(yaw) = step.facing {
            children.push((ROTATION_CHILD, rotation_bytes(yaw)));
        }
        if let Some(yaw) = step.look {
            children.push((LOOK_DIR_CHILD, rotation_bytes(yaw)));
        }
        if let Some(slayer) = step.slayer {
            children.extend(slayer.children()?);
        }
        Ok(Self::of_slot0(children, step.position))
    }

    /// A state of the client's own player on its clock `stamp`: the client takes an own state
    /// only with its number and time, and shows the network icon and clears its input history
    /// when none came for 2 s (network note: `FUN_14485b680`, `alc-server-timeout-for-input`).
    /// With `pos`, it also sends the place; with `data_bits`, the movement flags; when
    /// `anchored`, the clock's anchor (the client's 'now' for its cooldowns is the anchor plus
    /// the offset; clock note).
    pub fn own_step(
        stamp: AlcStamp,
        pos: Option<InputPosition>,
        data_bits: Option<u8>,
        anchored: bool,
    ) -> Self {
        let mut children: Vec<(usize, Vec<u8>)> = stamp_children(stamp).into();
        if anchored {
            children.push((TIME_ANCHOR_CHILD, stamp.anchor_us.to_be_bytes().to_vec()));
        }
        if let Some(pos) = pos {
            children.extend(position_children(pos));
        }
        if let Some(bits) = data_bits {
            children.push((DATA_BITS_CHILD, vec![bits]));
        }
        Self::of_slot0(children, pos)
    }

    /// One of the three states that move the client's own player to `pos`, facing `facing`
    /// (yaw, radians) when it is known, with teleport number `teleport` (retail moved the player
    /// into the water so, 118546 to 118614 ms): see [`TeleportStep`]. Each carries the number and
    /// time of `stamp`.
    pub fn teleport(
        stamp: AlcStamp,
        pos: InputPosition,
        facing: Option<f32>,
        teleport: u8,
        step: TeleportStep,
    ) -> Self {
        let mut children: Vec<(usize, Vec<u8>)> = stamp_children(stamp).into();
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

    /// The first state of the client's own player, at `pos`: both slots with the children and
    /// values of the most common retail first own ALC (own-player-position note: 315 logins).
    /// The client takes its start position only from such a full state (with the absolute
    /// `idAbs`, `timeOffsetAbs` and `worldPosAbs` parts); a lone `pos10` leaves it at the grid
    /// origin.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when a prefix value cannot be encoded.
    pub fn spawn(pos: InputPosition, anchor_us: u64) -> Result<Self, MessageError> {
        let mut writer = Writer::new();
        writer.u8(SLOT_BITS[0] | SLOT_BITS[1]);
        write_children(&mut writer, &SPAWN_SLOT0, pos, anchor_us)?;
        write_children(&mut writer, &SPAWN_SLOT1, pos, anchor_us)?;
        Ok(Self {
            bytes: writer.finish(),
            pos10: Some(pos),
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
    /// The clock anchor (µs since 2000-01-01).
    Anchor,
}

use SpawnValue::{Anchor, Byte, Float, Position as SpawnPosition, Prefixed, Raw, Word};

/// `worldPosRel` 0 on each axis (the relative part of the position; `0x7f` is the zero step).
const ZERO_POS3: [u8; POS3_LEN] = [0x7f; POS3_LEN];
/// An empty blob: count 0.
const EMPTY_BLOB: [u8; 1] = [0];
/// A `frag` of zeros.
const ZERO_FRAG: [u8; FRAG_LEN] = [0; FRAG_LEN];
/// The retail `segmentedStamina` (child 46) of every first own ALC (315 of 315 logins).
const FULL_SEGMENTED_STAMINA: u64 = 4_021_696_251;

/// The number of the first own state (the most common retail first own ALC).
pub const SPAWN_STATE_ID: u16 = 2;
/// The time of the first own state after its anchor of 0, in ms.
pub const SPAWN_STATE_MS: u16 = 9;
/// The teleport number of the first own state.
pub const SPAWN_TELEPORT: u8 = 1;

/// Slot-0 children of the first own state (child index and value), from the most common retail
/// first own ALC: id and time parts, the four slayer slots, zero relative position, a facing,
/// the absolute parts 0 with `worldPosAbs`, `teleportAndMigrationId` 1, and the fixed scope,
/// water, anchor and stamina children. Child 40 (`timeAnchor`) is 0, as in 16 logins.
const SPAWN_SLOT0: [(usize, SpawnValue); 46] = [
    (ID_REL_CHILD, Byte(SPAWN_STATE_ID.to_be_bytes()[1])),
    (TIME_REL_CHILD, Byte(SPAWN_STATE_MS.to_be_bytes()[1])),
    (2, Byte(129)),
    (3, Word(0x2844)),
    (4, Byte(127)),
    (5, Word(0)),
    (6, Byte(127)),
    (7, Word(0)),
    (8, Byte(127)),
    (9, Word(0)),
    (10, Raw(&ZERO_POS3)),
    (11, Raw(&[0x66, 0xfe])),
    (12, Raw(&[0x19, 0x7f])),
    (13, Prefixed(31)),
    (14, Word(1)),
    (15, Prefixed(1081)),
    (16, Prefixed(45)),
    (17, Word(1)),
    (18, Prefixed(0)),
    (19, Prefixed(0)),
    (20, Word(0)),
    (21, Prefixed(0)),
    (22, Prefixed(0)),
    (23, Word(0)),
    (24, Prefixed(0)),
    (25, Byte(0)),
    (26, Byte(0)),
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
    pos: InputPosition,
    anchor_us: u64,
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
            SpawnPosition => pos.write(writer),
            Anchor => {
                writer.u64(anchor_us);
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
        let clock_only = AlcState::own_step(stamp, None, None, false);
        assert_eq!(
            children_of(&clock_only),
            [ID_REL_CHILD, TIME_REL_CHILD, ID_ABS_CHILD, TIME_ABS_CHILD]
        );
        assert_eq!(clock_only.pos10, None);
        let pos = InputPosition::from_metres(1.0, 2.0, 3.0).expect("finite");
        let full = AlcState::own_step(stamp, Some(pos), Some(DATA_BITS_ACTIVE), false);
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
        );
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
        let state = AlcState::spawn(pos, RETAIL_ANCHOR_US).expect("spawn");
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
            .mob_first(STAMP, to, None, None)
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
                .mob_first(STAMP, to, None, None)
                .expect("encode")
                .map(|alc| alc.pos10),
            Some(Some(to))
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
            .mob_first(STAMP, to, None, Some(&animation))
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
    fn a_teleport_sends_the_three_retail_states() {
        // Retail water move (118546, 118580, 118614 ms): pos10 `444675eb43a8b6081d5c`, facing
        // `66f6`, then zero offset with teleport number 2.
        const RETAIL_POS10: &str = "444675eb43a8b6081d5c";
        const RETAIL_FACING: [u8; 2] = [0x66, 0xf6];
        const WATER_YAW: f32 = 1.434;
        let pos = InputPosition::read(&mut Reader::new(&hex(RETAIL_POS10))).expect("pos10");
        let state = |step| {
            let state = AlcState::teleport(STAMP, pos, Some(WATER_YAW), SPAWN_TELEPORT + 1, step);
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
            position: Some(to),
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
            state: prefix_of(retail_child(SLAYER_STATE_CHILD)),
            started: u16::from_be_bytes([0x28, 0xc5]),
            sequence: prefix_of(retail_child(SLAYER_SEQUENCE_CHILD)),
        };
        let step = RemoteStep {
            stamp: AlcStamp {
                id: 0x28c5,
                anchor_us: STAMP.anchor_us,
                offset_ms: 0x00fd,
            },
            position: retail.pos10,
            facing: Some(0.5),
            look: Some(0.25),
            slayer: Some(slayer),
        };
        let ours = AlcState::remote_step(&step).expect("step");
        for index in [
            ID_REL_CHILD,
            TIME_REL_CHILD,
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
        assert_eq!(child(&ours, LOOK_DIR_CHILD), Some(rotation_bytes(0.25)));
        assert_eq!(ours.pos10, retail.pos10);
        let bare = AlcState::remote_step(&RemoteStep {
            position: None,
            facing: None,
            look: None,
            slayer: None,
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
}
