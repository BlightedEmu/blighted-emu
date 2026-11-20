// SPDX-License-Identifier: AGPL-3.0-or-later
//! The player slayer sequence table (`combat/player-sequences.json`; run-15 lock and aim study,
//! 2026-09-22): which slayer states and sequences the server starts for a player, by weapon
//! stance, the way the retail server did in the capture ledgers of the pinned client build.
//!
//! The client numbers its slayer sequences at run time (no pak file holds the table, and neither
//! the fragment order of the animation databases nor the order of the player's animations gives
//! the numbers), so the table holds the numbers the retail server sent: mined from the own ALC
//! states (children 13 to 24 and 48) against the own inputs and the own weapon of 740 ledgers
//! (`local-data/scratch-work/run15/lockaim2/seqmine.py` and `gentable.py`; the aimed shot and the
//! reload of the gun and bow stances: `lockaim3/aimtable.py`). Each value is the most common one
//! of its case. Match rates on the October ledgers (the attack numbers moved between
//! builds; the right-click, dodge, jump, draw and movement numbers held): light starts 62.6 %
//! (228 of 364), charge starts 97.8 % (572 of 585), follow-ups 82.2 % (2,353 of 2,861).
//!
//! What the client leaves to the server (Observed, attack sequence study 2026-11-11 on the 103
//! ledgers whose item indexes resolve on the pinned build; `blighted-emu-tools`
//! `scratch-work/capdecode/src/bin/attackseq.rs`): a press of the attack key from no attack
//! starts the stance's press start in the press record ([`AttackPress`]: a melee weapon's charge
//! start in the first slot, a staff's or gauntlet's cast in the second); a release starts what
//! follows the attack that runs then ([`AttackStep`]); a hold starts what follows it after its
//! records ([`AttackHold`]). A sequence names an attack, not a stance: one sequence per stance
//! did not fit retail. A dodge, a jump, a weapon draw, the block and the aim (right-click) and
//! every movement state are the server's too. The client starts the later attacks of a combo
//! itself (input row 1), and the server follows some of them ([`FollowUp`]). The bow, the musket
//! and the blunderbuss send no row 1 for their shots either: the server sends the aimed shot
//! ([`StanceSequences::alt_fire`], the bow's at the release), the hip shot ([`HipShot`]) and
//! what follows them ([`TimedSlots`]; ranged shot study 2026-11-18,
//! `local-data/scratch-work/ranged-shots-2026-11-18/findings.md`; the re-fires, reloads, aim legs
//! and shot gap of 2026-11-19: the miners in `local-data/scratch-work/ranged-shots-2026-11-19/`;
//! the bow's full draw hold, [`DrawHold`]).
//! Unknown (no retail data on the pinned build): the dagger.
//!
//! The client sends no slayer state for a weapon ability either (input row 1 is empty at every
//! ability key press): the server starts the ability's animation in the own state after the
//! press, 31 to 42 ms later ([`AbilityPlay`]; ability animation study 2026-11-11, 17,488 answered
//! presses of 95 abilities in 1,195 ledgers: `blighted-emu-tools`
//! `scratch-work/capdecode/src/bin/abilityslayer.rs` and `abilityplays.py`). Without it the
//! client shows the ability's effects but no animation (live 2026-11-11: Flail Trip, the life
//! staff's abilities).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use nw_formats::objectstream::lowercase_crc32;

use crate::layout::{ContentLayout, PLAYER_SEQUENCES};
use crate::player_attacks::stance_key;

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum PlayerSequencesError {
    /// The file cannot be read.
    #[error("player sequence table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table's JSON.
    #[error("player sequence table: {0}")]
    Json(#[from] serde_json::Error),
}

/// A slayer state and the sequence it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatePlay {
    /// `slayerStateId`.
    pub state: u64,
    /// `slayerSequenceId`.
    pub sequence: u64,
}

/// The slayer children past the first slot that one server reaction sends: the second slot
/// (children 16 and 18; the state as on the wire, one more than the client's), the third (19 and
/// 21), the fourth (22 and 24) and the slayer marker (child 48). `None`: not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SlotsChange {
    /// Child 16.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_state: Option<u64>,
    /// Child 18.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_sequence: Option<u64>,
    /// Child 19.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third_state: Option<u64>,
    /// Child 21.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third_sequence: Option<u64>,
    /// Child 22.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fourth_state: Option<u64>,
    /// Child 24.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fourth_sequence: Option<u64>,
    /// Child 48.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<u64>,
}

impl SlotsChange {
    /// Whether it sends no child.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The upper-body sequences of the right-click's block or aim on one kind of legs: the
/// second-slot sequence it starts with, and the one it holds after its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AltLegs {
    /// The second-slot sequence of a block or aim that starts on these legs (`16=37` or `16=36`
    /// with `18=<start>`).
    pub start: u64,
    /// The second-slot sequence it holds after its start, and that a change to these legs
    /// sends while it is held (`18=<hold>` alone).
    pub hold: u64,
    /// The third-slot sequence (child 21) of an aim on these legs: sent with its start, in the
    /// state of a change between walking and standing, and in place of the other legs' one in
    /// a shot's step. Observed (ranged shot study 2026-11-19, 1,195 retail ledgers, other
    /// players' streams): the bow's 422, the musket's 406 and the blunderbuss's 408 went with
    /// the aim's stand (first slot `13=31`) in 15,032 of 15,050, 2,574 of 2,578 and 1,533 of
    /// 1,534 states, their 421, 405 and 407 with the walk and its start; a change of the first
    /// slot between them sent the other one in its own state (bow 10,645 of 10,791 and 9,945 of
    /// 10,061, musket 2,095 of 2,096 and 1,782 of 1,788, blunderbuss 1,219 of 1,224 and 1,372 of
    /// 1,375); the bow's step after a drawn shot sent 422 standing (1,292) and 421 walking
    /// (2,544). `None`: the third slot does not change with the legs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third: Option<u64>,
}

/// The upper body of the right-click's block or aim by legs: retail plays a start then a hold,
/// and for most blocks one pair while it walks and another while it stands (Observed in 1,195
/// retail ledgers, block sequence study 2026-11-18: the spear blocks walking with 3044 then
/// 4380 and standing with 4998 then 5016, a leg change sending the new legs' hold; the rifle
/// aims with 3544 then 4249 on both legs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AltHold {
    /// While it walks (the walk of [`AltMoves::moving`]).
    pub walk: AltLegs,
    /// While it stands (the stand of [`AltMoves::idle`]).
    pub stand: AltLegs,
    /// Milliseconds from its start to its hold (the median of the retail starts that kept
    /// their legs).
    pub hold_after_ms: u64,
}

impl AltHold {
    /// The upper body on walking (`walking`) or standing legs.
    pub const fn legs(&self, walking: bool) -> AltLegs {
        if walking { self.walk } else { self.stand }
    }
}

/// The first-slot movement states while the right-click is held (block or aim walk).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AltMoves {
    /// While a movement key is held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving: Option<StatePlay>,
    /// Standing still.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle: Option<StatePlay>,
}

/// The first-slot movement sequences of the weapon drawn (the sheathed ones are the sim's own,
/// from the newchar capture). A missing one keeps the sheathed sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DrawnMoves {
    /// A move from standing still, forward or to the side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<StatePlay>,
    /// A move from standing still, back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back_start: Option<StatePlay>,
    /// The run a start goes on in once its start sequence has played: the sequence alone, in
    /// the start's state (Observed, 2026-11-19 study of the 391 ledgers with verified weapons:
    /// the own `13=1a` starts of each stance went on in one run sequence, for example the
    /// rifle's 1855 in 1584 in 96 of 98, 1,366 ms after the start at the median).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<u64>,
    /// The keys let go.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<u64>,
    /// The keys let go soon after a start (Observed, same study: a start let go within 270 ms
    /// stopped in its own sequence, the rifle's in 3543 in 55 of 103).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_stop: Option<u64>,
    /// A key again while stopping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<u64>,
    /// Stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle: Option<u64>,
}

/// Where the movement keys point when a dodge or a jump starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Forward (with or without a side key).
    Forward,
    /// Back.
    Back,
    /// Left alone.
    Left,
    /// Right alone.
    Right,
    /// No movement key.
    None,
}

impl Direction {
    /// The direction of the held keys.
    pub const fn of_keys(forward: bool, back: bool, left: bool, right: bool) -> Self {
        if back {
            Self::Back
        } else if forward {
            Self::Forward
        } else if left {
            Self::Left
        } else if right {
            Self::Right
        } else {
            Self::None
        }
    }
}

/// A first-slot state the server starts for a key, by direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Directional {
    /// Forward.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward: Option<StatePlay>,
    /// Back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<StatePlay>,
    /// Left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<StatePlay>,
    /// Right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<StatePlay>,
    /// No key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub none: Option<StatePlay>,
}

impl Directional {
    /// The state of `direction`, else of no key, else of forward (best guess: a direction the
    /// ledgers did not show plays the stance's default one).
    pub fn of(&self, direction: Direction) -> Option<StatePlay> {
        self.exact(direction).or(self.none).or(self.forward)
    }

    /// The state of `direction` alone.
    pub const fn exact(&self, direction: Direction) -> Option<StatePlay> {
        match direction {
            Direction::Forward => self.forward,
            Direction::Back => self.back,
            Direction::Left => self.left,
            Direction::Right => self.right,
            Direction::None => self.none,
        }
    }
}

/// How heavy a player's equipped gear is, for its dodge: the light roll, the medium hop or the
/// heavy step (the client's `EquipLoad` categories Fast, Normal and Slow).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DodgeLoad {
    /// Light gear: the roll of [`StanceSequences::dodge`].
    #[default]
    Fast,
    /// Medium gear.
    Normal,
    /// Heavy gear.
    Slow,
}

/// The dodges of the medium and heavy loads (Observed, 2026-11-17 study: retail sends the own
/// player's load as its `EquipLoad_*_<Light|Medium|Heavy>` effects, and the dodge sequence of each
/// load differs; [`StanceSequences::dodge`] is the light one, the most common in the ledgers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DodgeLoads {
    /// Medium gear.
    #[serde(rename = "Normal", default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<Directional>,
    /// Heavy gear.
    #[serde(rename = "Slow", default, skip_serializing_if = "Option::is_none")]
    pub slow: Option<Directional>,
}

impl DodgeLoads {
    /// The dodges of `load`; none for the light load (that is the stance's own dodge).
    pub const fn of(&self, load: DodgeLoad) -> Option<Directional> {
        match load {
            DodgeLoad::Fast => None,
            DodgeLoad::Normal => self.normal,
            DodgeLoad::Slow => self.slow,
        }
    }
}

/// The landing sequence (slayer state 12) of one way into the air, by whether a move key is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Landing {
    /// No move key held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub still: Option<u64>,
    /// A move key held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving: Option<u64>,
}

impl Landing {
    /// The landing of a body that holds a move key (`moving`) or not, else the other one (best
    /// guess: a case the ledgers did not show lands as the other).
    pub fn of(&self, moving: bool) -> Option<u64> {
        if moving {
            self.moving.or(self.still)
        } else {
            self.still.or(self.moving)
        }
    }
}

/// A stance's fall (slayer state 13) and landings (Observed, 2026-11-17 study of 391 retail
/// ledgers: the land followed 91 % of falls and 79 % of jumps, and its sequence depends more on
/// a jump against a fall than on a move key; the jump's direction showed no difference).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AirSequences {
    /// The fall's sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fall: Option<u64>,
    /// The landing after a jump.
    #[serde(default)]
    pub land_after_jump: Landing,
    /// The landing after a fall.
    #[serde(default)]
    pub land_after_fall: Landing,
}

/// The server's sequences of one weapon stance.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StanceSequences {
    /// The weapon's `MannequinTag` cell (empty: no weapon).
    pub tags: String,
    /// What an attack key press starts when no attack or ability runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press: Option<AttackPress>,
    /// What a release of the attack key starts, by the attack that runs then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub releases: Vec<AttackStep>,
    /// What a hold of the attack key starts, by the attack that runs then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holds: Vec<AttackHold>,
    /// Input records from the last start of a second-slot cast to its end (`16=45 18=0 48=0`):
    /// the median of the retail casts that no new press followed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast_end_records: Option<u16>,
    /// What a right-click press starts (block for a melee weapon, aim for a ranged one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_start: Option<SlotsChange>,
    /// What its release sends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_end: Option<SlotsChange>,
    /// The movement states while it is held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_moves: Option<AltMoves>,
    /// The upper body of the right-click's block or aim by legs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_hold: Option<AltHold>,
    /// The movement sequences of the weapon drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawn_moves: Option<DrawnMoves>,
    /// The dodge (of the light load).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dodge: Option<Directional>,
    /// The dodges of the medium and heavy loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dodge_loads: Option<DodgeLoads>,
    /// The jump.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<Directional>,
    /// The draw: the second slot's state (wire value) and sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw: Option<StatePlay>,
    /// Milliseconds from the draw's start to the end of its second slot (`16=45 18=0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_end_ms: Option<u64>,
    /// The sheathe's second-slot sequence, in the draw's state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheathe: Option<u64>,
    /// Milliseconds from the sheathe's start to the end of its second slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheathe_end_ms: Option<u64>,
    /// The fall and the landings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air: Option<AirSequences>,
    /// What an attack key press with the right-click held sends while a shot is loaded: the
    /// aimed shot (retail blunderbuss `18=5e 48=29`, 30 to 80 ms after the press).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_fire: Option<SlotsChange>,
    /// What the first attack key press sends after the weapon emptied and was loaded again: the
    /// reload (retail blunderbuss `16=24 17=<record> 18=8142`, 1.5 to 1.9 s after the last shot;
    /// that press fires nothing). The pinned build's blunderbuss has none: it reloads by the
    /// steps of the shot that empties it ([`StanceSequences::alt_fire_empty_after`],
    /// [`HipShot::empty_after`]), and its first press after them shoots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reload: Option<SlotsChange>,
    /// The aimed shot goes out at the attack key's release, not at its press: the press draws
    /// (Observed, ranged shot study 2026-11-18 on the own bow of 1,195 retail ledgers: 401 of 401
    /// plain and drawn shots of 718 aimed press and release pairs came after the release, a
    /// median 51 ms after it, none at the press; the musket fired 400 of 433 and the blunderbuss
    /// 241 of 249 shots before the release).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alt_fire_at_release: bool,
    /// What follows the aimed shot ([`StanceSequences::alt_fire`]), by milliseconds from the own
    /// state that sent it: the draw of the next shot and the aim's hold again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt_fire_after: Vec<TimedSlots>,
    /// What follows the aimed shot that empties the weapon, in place of
    /// [`StanceSequences::alt_fire_after`]: its reload, then the aim again. Observed, blunderbuss
    /// (ranged shot study 2026-11-19, aimed shots that only their hold step `18=4248 48=40`
    /// parted from the reload): `16=39 18=96 48=40` 766 ms after the shot (137 seen by other
    /// players, p10 749, p90 784; own 159, median 767), then the aim again `16=36 18=4225` a
    /// median 532 ms after the reload (84 of 137; own 94 of 159, 117 of them with no press
    /// between). Runs of plain shots between two reloads held 2 shots (326 of 443 seen by
    /// other players).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt_fire_empty_after: Vec<TimedSlots>,
    /// The shortest time from an aimed shot's own state to the next aimed shot's: a release
    /// before it holds the next shot until then. Observed, bow (ranged shot study 2026-11-19):
    /// 159 own releases with one press and one release under 1,000 ms after the shot before
    /// sent their plain shot 1,066 ms after it (p10 1,048, p90 1,083), a median 267 ms after the
    /// release; 345 later releases shot a median 36 ms after the release; of 11,807 plain shots
    /// seen by other players, 122 came 100 to 1,000 ms after the shot before (median 1,068 ms).
    /// Observed, musket (rifle study 2026-11-20, 1,195 retail ledgers): 86 aimed shots that a
    /// press before them waited for came a median 1,734 ms after the shot before (p10 1,712),
    /// 267 ms after the aim's return `16=36 18=3544` (p10 258, p90 283).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_fire_gap_ms: Option<u64>,
    /// How long after an aimed shot's own state an aimed shot asked for waits for
    /// [`StanceSequences::alt_fire_gap_ms`]: one asked for before this fires nothing. `None`:
    /// every one waits. Observed, musket (rifle study 2026-11-20): of the presses inside a
    /// shot's reload with none after the aim's return, 16 of 17 whose newest press came before
    /// the reload's `18=1716` step (867 ms after the shot) fired nothing, and 4 of 5 after it
    /// fired 267 ms after the aim's return; a press 131 ms after the shot (2026-09-27 musket
    /// capture) fired nothing, and the next shot came at the next press 2.5 s after the shot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_fire_wait_from_ms: Option<u64>,
    /// An aimed shot asked for less than this after the aim's start (the right-click's
    /// [`StanceSequences::alt_start`]) waits until then. Observed (rifle study 2026-11-20, own
    /// aim starts with no shot in the 2.5 s before and an aimed press before 150 ms after the
    /// start): the musket shot 267 ms after the start (p10 252, 71 aims; none before 247), the
    /// blunderbuss 133 ms after it at the earliest (147 aims, p10 134). The bow shoots at its
    /// release after its draw, so none for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_fire_ready_ms: Option<u64>,
    /// The aimed shot of a key held to its full draw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_charge: Option<AltCharge>,
    /// The shot of an attack key press without the right-click (from the hip).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hip: Option<HipShot>,
}

/// An upper slot change that follows a shot, `after_ms` after the own state that sent the shot
/// (or the start before it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TimedSlots {
    /// Milliseconds from the shot's own state.
    pub after_ms: u64,
    /// The children it sends.
    #[serde(flatten)]
    pub slots: SlotsChange,
}

/// The full draw of an aimed shot that goes out at the release
/// ([`StanceSequences::alt_fire_at_release`]): after the key was held `after_ms` from its press,
/// `drawn` goes out, and the release then sends `fire` instead of the plain shot (Observed, bow:
/// 137 of 157 drawn shots `18=38 48=25` came after `48=24`, and 0 of 244 plain shots `18=35
/// 48=41`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AltCharge {
    /// Milliseconds of input records from the press to the full draw.
    pub after_ms: u64,
    /// What the full draw sends while the key is held.
    pub drawn: SlotsChange,
    /// What the release sends after the full draw.
    pub fire: SlotsChange,
    /// What follows `fire`, by milliseconds from its own state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fire_after: Vec<TimedSlots>,
    /// The full draw's hold while the key stays held. `None`: the full draw holds nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<DrawHold>,
}

/// The hold of a full draw ([`AltCharge`]) while the attack key stays held: `after_ms` after the
/// full draw's own state, the second-slot `sequence` (child 18 alone: the aim's second-slot
/// state stays) with the third-slot sequence of the legs (child 21 alone, no start), which
/// takes the place of the aim's by legs ([`AltLegs::third`]): a change between walking and
/// standing sends the other one in its own state, and the shot of the full draw sends that of
/// its legs when the third slot does not play it already, up to the shot's step that sends the
/// aim's again. A stance whose aim has no third slot by legs sends `sequence` alone.
///
/// Observed, bow (bow full draw study 2026-11-19, 1,195 retail ledgers, the miners in
/// `local-data/scratch-work/bow-draw-2026-11-19/`; weapon checked per segment as the ranged shot
/// study): `18=928` a median 133 ms after the `48=24` state (own 991, p10 120, p90 149; other
/// players 1,675, median 134, p10 116, p90 151); the aim's second-slot state 36 before it in
/// all; with `21=4275` on walking legs (first slot `13=27`) and `21=3077` standing (`13=31`):
/// own 804 and 139, other players 1,009 and 569, the rest already playing it; no child 17 or 20
/// in 1,020 own and 1,962 of 1,964 others' states. While it played, 320 of 322 own and 510 of
/// 512 others' changes between 27 and 31 sent the other one in the same state. A release before
/// it sent the drawn shot from the aim's hold `18=29` with the legs' one (own 462 of 464,
/// others 934 of 946); after it, the shot sent none in 807 of 930 own; the shot's step `48=41`
/// then sent the aim's of the legs (`4275` to `421` in 1,077 own, `3077` to `422` in 237).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DrawHold {
    /// Milliseconds from the full draw's own state.
    pub after_ms: u64,
    /// Its second-slot sequence.
    pub sequence: u64,
    /// Its third-slot sequence on walking legs.
    pub walk_third: u64,
    /// Its third-slot sequence on standing legs.
    pub stand_third: u64,
    /// What follows it while the key stays held, by milliseconds from its own state. Observed,
    /// bow: `18=32` alone 3,033 ms after `18=928` (own 47, 2,997 to 3,049 ms; other players 89,
    /// median 3,033); 47 of 49 own holds of 2,950 ms or more went to it, the other 2 shot first;
    /// its shot is still the drawn one `18=38 48=25` (own 38, others 46).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<TimedSlots>,
}

/// The shot of an attack key press from the hip: the server's, the client names none in row 1
/// (Observed, ranged shot study 2026-11-18: 4 of 362 own bow hip presses had a row 1).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HipShot {
    /// What the press sends, started in its record.
    pub start: SlotsChange,
    /// What follows `start`, by milliseconds from its own state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<TimedSlots>,
    /// The shot's end, by milliseconds from the start's own state.
    pub end: TimedSlots,
    /// A shot again from a press while the shot plays: the end's own state sends this in place
    /// of the end, and `after` and `end` count again from there (the bow); with
    /// [`HipShot::refire_window`], the press's own state sends it. `None`: such a press starts
    /// nothing. Observed (ranged shot study 2026-11-19): the musket's 19 own hip shots with a
    /// press while they played (94 presses) sent nothing of their own, and none of its 28 hip
    /// shots seen by other players carried a second fire marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refire: Option<SlotsChange>,
    /// When a press re-fires, by milliseconds from the shot's own state (or its re-fire's):
    /// only a press then sends [`HipShot::refire`], at once. Observed, blunderbuss (ranged shot
    /// study 2026-11-19, own hip shots with a shot still loaded): a first press from 20 ms
    /// before the `48=66` step to 100 ms after it re-fired `48=67` (20 of 20), 37 ms after the
    /// press (p10 29, p90 50); 100 to 120 ms after it, 1 re-fire and 1 end; a press only later,
    /// or none, ended the shot (34 of 34; 7 re-fires in stretches with no input records left
    /// out). Other players saw 82 re-fires, 77 ends and 83 reloads after 251 such steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refire_window: Option<PressWindow>,
    /// What follows the start of the shot that empties the weapon, in place of `after` and
    /// `end` (its last step is its end): the reload. Observed, blunderbuss (ranged shot study
    /// 2026-11-19): `16=39 18=96 19=0 21=0 48=0` 766 ms after the hip shot or its re-fire (148
    /// seen by other players, p10 745, p90 784; own 72, median 767), 33 ms after its `48=66`
    /// step, then `16=45 18=0` a median 566 ms after the reload (98 of 148).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub empty_after: Vec<TimedSlots>,
    /// With the right-click held, the aim starts this long after the shot's own state (or its
    /// re-fire's), not at once; `None`: at the shot's end ([`HipShot::aim_at_ms`]). Observed
    /// (rifle study 2026-11-20, own hip shots with the block held from before the end): the
    /// musket's aim `16=36 18=3544` came 2,215 to 2,269 ms after its hip shot (16 cases,
    /// median 2,249), before its end at 2,566 ms and whenever the block went down; the bow's
    /// 919 to 1,021 ms (79 cases, end 1,000 ms) and the blunderbuss's 850 to 903 ms (12 of 16,
    /// end 894 ms) came at their ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aim_after_ms: Option<u64>,
}

impl HipShot {
    /// Milliseconds from the shot's own state to the aim's start with the right-click held
    /// ([`HipShot::aim_after_ms`], else the end's).
    pub fn aim_at_ms(&self) -> u64 {
        self.aim_after_ms.unwrap_or(self.end.after_ms)
    }
}

/// A span of milliseconds from a shot's own state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PressWindow {
    /// Its first millisecond.
    pub from_ms: u64,
    /// Its last millisecond.
    pub until_ms: u64,
}

impl PressWindow {
    /// Whether `elapsed_ms` falls in it.
    pub const fn holds(&self, elapsed_ms: u64) -> bool {
        elapsed_ms >= self.from_ms && elapsed_ms <= self.until_ms
    }
}

impl StanceSequences {
    /// The stance shoots: its shots are an aimed shot ([`StanceSequences::alt_fire`]) or a hip
    /// shot ([`StanceSequences::hip`]).
    pub fn shoots(&self) -> bool {
        self.alt_fire.is_some() || self.hip.is_some()
    }

    /// What a release of the attack key starts while the attack `running` plays.
    pub fn release_after(&self, running: RunningAttack) -> Option<RunningAttack> {
        self.releases
            .iter()
            .find(|step| step.after == running)
            .map(|step| step.start)
    }

    /// What a hold of the attack key starts while the attack `running` plays, and after how
    /// many input records of the hold.
    pub fn hold_after(&self, running: RunningAttack) -> Option<(RunningAttack, u16)> {
        self.holds
            .iter()
            .find(|hold| hold.after == running)
            .map(|hold| (hold.start, hold.records))
    }
}

/// The slayer slot an attack plays in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttackSlot {
    /// The first slot (children 13 to 15): the melee weapons.
    First,
    /// The second slot (children 16 to 18): the staffs, the gauntlets, the bow and the guns.
    Second,
}

/// An attack that plays: its slot and its sequence. The sequence names the attack: the same
/// attack has the same number for every player (Observed, attack sequence study 2026-11-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningAttack {
    /// Its slot.
    pub slot: AttackSlot,
    /// Its sequence (child 15 or 18).
    pub sequence: u64,
}

/// The start the retail server sent in the record of an attack key press from no attack: a
/// melee weapon's charge start in the first slot (the flail's `13=10 14=<record> 15=1319 48=1`),
/// a staff's or gauntlet's cast in the second (the life staff's `16=33 17=<record> 18=934
/// 48=3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackPress {
    /// The attack it starts.
    #[serde(flatten)]
    pub start: RunningAttack,
    /// The slot's state (child 13 or 16, wire value).
    pub state: u64,
    /// Child 48.
    pub marker: u64,
}

/// What the retail server started at a release of the attack key while `after` played (the
/// flail's light `461` after its press start `1319`; a combo step the client named: `481` after
/// `5046`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackStep {
    /// The attack that plays at the release.
    pub after: RunningAttack,
    /// The attack the release starts.
    pub start: RunningAttack,
}

/// What the retail server started when the attack key stayed down while `after` played (the
/// flail's charged attack `583`, 14 records after the press; most stances 19 records).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackHold {
    /// The attack that plays while the key is held.
    pub after: RunningAttack,
    /// The attack the hold starts.
    pub start: RunningAttack,
    /// Input records from the start of `after` to the start (the median of the retail holds).
    pub records: u16,
}

/// A server follow-up of an action the client started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowUp {
    /// The client's sequence (input row 1).
    pub client: u64,
    /// The sequence the server starts after it.
    pub server: u64,
    /// The server starts it at the attack key's next release (else one own state after).
    pub at_release: bool,
    /// How many times the ledgers showed the pair.
    pub seen: u32,
}

/// The first-slot state of an ability's animation (children 13 and 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstPlay {
    /// `slayerStateId` (child 13): 8 for the full-body abilities.
    pub state: u64,
    /// `slayerSequenceId` (child 15).
    pub sequence: u64,
}

/// The animation the server starts for one weapon ability, in the own state after its key's
/// press (each slot it sets starts in the press's state): a full-body ability plays in the
/// first slot (the flail's Trip: `13=08 15=2344 17=<press> 48=01`), an upper-body one in the
/// slots past it (the life staff's Orb of Protection: `16=33 17=<press> 18=3039 48=03`; an aimed
/// one also in the third: Sacred Ground's `16=36 18=142 19=46 21=449 48=33`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityPlay {
    /// The ability id (its `CooldownData` and `AbilityData` id, as retail's cooldown names it).
    pub ability: String,
    /// The first slot; `None` for an upper-body ability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<FirstPlay>,
    /// The slots past the first and the marker.
    #[serde(default)]
    pub slots: SlotsChange,
    /// What an aimed ability's cast sends (Sacred Ground's `16=33 18=993 19=00 21=00 48=03`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast: Option<SlotsChange>,
    /// How many presses showed the start.
    pub seen: u32,
}

/// The file form of the table.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct TableFile {
    stances: Vec<StanceSequences>,
    follow_ups: Vec<FollowUp>,
    #[serde(default)]
    abilities: Vec<AbilityPlay>,
    #[serde(default)]
    reactions: Vec<ReactionPlay>,
    #[serde(default)]
    riders: Vec<RiderSequences>,
    #[serde(default)]
    interactions: Vec<InteractionPlay>,
    #[serde(default)]
    stations: Vec<StationPlay>,
    #[serde(default)]
    gathers: Vec<GatherPlay>,
}

/// A pair of sequences of a rider's change, by the legs it starts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LegPair {
    /// Moving by the movement keys.
    pub moving: u64,
    /// Standing still.
    pub still: u64,
}

/// The first-slot sequences of a rider of one mount type (2026-11-19 study, `local-data/
/// scratch-work/remote-all-2026-11-19` `rider3.py`: every player's states of 1,196 ledgers
/// against the mount of its mount member, slot 2). The server sends them all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RiderPlays {
    /// The mount-up (`13=20`).
    pub mount_up: LegPair,
    /// The ride (`13=21`) a moving mount-up goes on in.
    pub after_mount_up: u64,
    /// The ride (`13=21`) moving.
    pub ride: u64,
    /// The ride dashing.
    pub fast: u64,
    /// The ride starting or stopping: the walk the stand comes and goes through.
    pub slow: u64,
    /// Standing still (`13=23`).
    pub idle: u64,
    /// The jump (`13=18`).
    pub jump: u64,
    /// The jump's landing (`13=17`).
    pub land: u64,
    /// A fall without a jump (`13=19`).
    pub fall: u64,
    /// The fall's landing (`13=17`).
    pub fall_land: u64,
    /// The dismount (`13=15`).
    pub dismount: LegPair,
}

/// The rider sequences of one mount type (`MountTypeData` `MountType`), with whether a capture
/// holds them or they are another type's (**Inferred**).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RiderSequences {
    /// The mount type.
    pub mount_type: String,
    /// Its sequences.
    #[serde(flatten)]
    pub plays: RiderPlays,
    /// No capture holds this type: its sequences are another type's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inferred: bool,
}

/// The first-slot state and sequence the server starts when it accepts an interaction of one
/// kind (2026-11-19 study, `interact.py`: the own state 10 ms after each accepted 3743 of the
/// kind, 1,196 ledgers). A kind without a row starts none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionPlay {
    /// The interaction kind.
    pub kind: u8,
    /// Its state.
    pub state: u64,
    /// Its sequence.
    pub sequence: u64,
}

/// The sequence of the use of one kind of crafting station (state `13=24`), by the stem of the
/// station's display name (`@SmeltingT2_MasterName`: `Smelting`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StationPlay {
    /// The stem of the display name.
    pub station: String,
    /// Its sequence.
    pub sequence: u64,
}

/// The sequence of a gather (state `13=09`), by the gatherable's `GatheringFragment`, else its
/// `GatheringAction`, and whether the gatherer crouches in it (ALC child 43).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatherPlay {
    /// The fragment or action.
    pub animation: String,
    /// Its sequence.
    pub sequence: u64,
    /// The gatherer crouches.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub crouch: bool,
}

/// The sequence of one hit reaction entry: the server starts it in the reaction state
/// (`13=04`) of the own state after the hit. The client numbers the sequence by the entry it
/// plays, not by the stance (Observed: `2960` in the great sword, the spear and no weapon), so
/// the table keys it by the entry's animations
/// ([`crate::player_attacks::ReactionEntry::animations`]). Mined from the own reaction starts
/// of the capture ledgers against the side of the hit and the power level of the creature's
/// attack row (`blighted-emu-tools` `scratch-work/combat/hitreact`, `reactlabel` and
/// `reacttable.py`, 2026-11-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReactionPlay {
    /// The entry's animations, joined by `+`.
    pub animations: String,
    /// `slayerSequenceId` (child 15).
    pub sequence: u64,
}

/// The table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerSequences {
    by_key: HashMap<u32, StanceSequences>,
    follow_ups: HashMap<u64, FollowUp>,
    /// The ability animations, by CRC-32 of the lowercase ability id.
    abilities: HashMap<u32, AbilityPlay>,
    /// The reaction sequences, by the entry's animations ([`ReactionPlay`]).
    reactions: HashMap<String, u64>,
    /// The rider sequences, by lowercase mount type.
    riders: HashMap<String, RiderSequences>,
    /// The interactions' states, by kind.
    interactions: HashMap<u8, InteractionPlay>,
    /// The crafting stations' sequences, by lowercase stem.
    stations: HashMap<String, u64>,
    /// The gathers' sequences, by lowercase fragment or action.
    gathers: HashMap<String, GatherPlay>,
}

/// The mount type whose rider sequences a type without a row plays (**Inferred**: the horse is
/// the type with the most rides in the captures after the wolf, and the one of every mount
/// quest).
const FALLBACK_MOUNT_TYPE: &str = "horse";
/// The station sequence of a station kind without a row (**Inferred**: `2513` is the sequence
/// of the most station kinds, alchemy, engineering, the kiln and the shops).
const FALLBACK_STATION_SEQUENCE: u64 = 2513;
/// The gather animation of a gatherable that names none (its `GatheringAction` is `None`).
const NO_GATHER_ANIMATION: &str = "none";
/// The station name's stem ends at its tier (`T` and a digit).
const STATION_TIER: char = 'T';

impl PlayerSequences {
    /// The table of `layout`.
    ///
    /// # Errors
    ///
    /// Returns [`PlayerSequencesError`] when the file cannot be read or is not the table.
    pub fn load(layout: &ContentLayout) -> Result<Self, PlayerSequencesError> {
        let path = layout.file(PLAYER_SEQUENCES);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| PlayerSequencesError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`PlayerSequencesError::Json`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, PlayerSequencesError> {
        let file: TableFile = serde_json::from_str(text)?;
        let mut table = Self::of_parts(file.stances, file.follow_ups)
            .with_abilities(file.abilities)
            .with_reactions(file.reactions);
        table.riders = file
            .riders
            .into_iter()
            .map(|rider| (rider.mount_type.to_lowercase(), rider))
            .collect();
        table.interactions = file
            .interactions
            .into_iter()
            .map(|play| (play.kind, play))
            .collect();
        table.stations = file
            .stations
            .into_iter()
            .map(|play| (play.station.to_lowercase(), play.sequence))
            .collect();
        table.gathers = file
            .gathers
            .into_iter()
            .map(|play| (play.animation.to_lowercase(), play))
            .collect();
        Ok(table)
    }

    /// The rider sequences of mount type `mount_type`, else the fallback type's
    /// ([`FALLBACK_MOUNT_TYPE`]).
    pub fn rider(&self, mount_type: &str) -> Option<RiderPlays> {
        self.riders
            .get(&mount_type.to_lowercase())
            .or_else(|| self.riders.get(FALLBACK_MOUNT_TYPE))
            .map(|rider| rider.plays)
    }

    /// The state and sequence an accepted interaction of `kind` starts, when it starts one.
    pub fn interaction(&self, kind: u8) -> Option<InteractionPlay> {
        self.interactions.get(&kind).copied()
    }

    /// The sequence of the use of the crafting station whose display name is `name`
    /// (`@SmeltingT2_MasterName`), else the fallback ([`FALLBACK_STATION_SEQUENCE`]).
    pub fn station(&self, name: &str) -> u64 {
        self.stations
            .get(&station_stem(name).to_lowercase())
            .copied()
            .unwrap_or(FALLBACK_STATION_SEQUENCE)
    }

    /// The gather of a gatherable whose gatherer plays `animation` (its `GatheringFragment`,
    /// else its `GatheringAction`): that animation's row, else that of no animation.
    pub fn gather(&self, animation: Option<&str>) -> Option<&GatherPlay> {
        let row = |name: &str| self.gathers.get(&name.to_lowercase());
        animation.and_then(row).or_else(|| row(NO_GATHER_ANIMATION))
    }

    /// The same table with the reaction sequences `reactions`.
    #[must_use]
    pub fn with_reactions(mut self, reactions: Vec<ReactionPlay>) -> Self {
        self.reactions = reactions
            .into_iter()
            .map(|play| (play.animations, play.sequence))
            .collect();
        self
    }

    /// The sequence of the reaction entry that plays `animations`.
    pub fn reaction(&self, animations: &str) -> Option<u64> {
        self.reactions.get(animations).copied()
    }

    /// The same table with the ability animations `abilities`.
    #[must_use]
    pub fn with_abilities(mut self, abilities: Vec<AbilityPlay>) -> Self {
        self.abilities = abilities
            .into_iter()
            .map(|play| (lowercase_crc32(&play.ability), play))
            .collect();
        self
    }

    /// The animation of the ability whose lowercase id has CRC-32 `ability`.
    pub fn ability(&self, ability: u32) -> Option<&AbilityPlay> {
        self.abilities.get(&ability)
    }

    /// The table of `stances` and `follow_ups`.
    pub fn of_parts(stances: Vec<StanceSequences>, follow_ups: Vec<FollowUp>) -> Self {
        Self {
            by_key: stances
                .into_iter()
                .map(|stance| (stance_key(&stance.tags), stance))
                .collect(),
            follow_ups: follow_ups
                .into_iter()
                .map(|follow_up| (follow_up.client, follow_up))
                .collect(),
            abilities: HashMap::new(),
            reactions: HashMap::new(),
            riders: HashMap::new(),
            interactions: HashMap::new(),
            stations: HashMap::new(),
            gathers: HashMap::new(),
        }
    }

    /// The sequences of the stance of key `key` ([`stance_key`]).
    pub fn stance(&self, key: u32) -> Option<&StanceSequences> {
        self.by_key.get(&key)
    }

    /// The sequences of no weapon in the hand: the stance of the empty `MannequinTag` cell, which
    /// a sheathed weapon plays too (Observed, 2026-11-17 study: with the weapon sheathed, retail
    /// sent the sheathed dodge 4845 and jump 1208 whatever the weapon).
    pub fn sheathed(&self) -> Option<&StanceSequences> {
        self.stance(stance_key(""))
    }

    /// The dodge of a player whose moves play `stance` (the drawn weapon's, else no weapon's)
    /// with gear `load`, toward `direction`: the load's own of the stance in that direction, else
    /// of no weapon in that direction, else the stance's or no weapon's load dodge of another
    /// direction, else the stance's light dodge. The ledgers hold few medium and heavy dodges per
    /// weapon (2026-11-17 study), so a missing one plays the load's dodge of no weapon (Inferred:
    /// the load sets the kind of dodge, the roll, hop or step).
    pub fn dodge(
        &self,
        stance: Option<&StanceSequences>,
        load: DodgeLoad,
        direction: Direction,
    ) -> Option<StatePlay> {
        let of_load = |stance: Option<&StanceSequences>| {
            stance.and_then(|stance| stance.dodge_loads?.of(load))
        };
        let own = of_load(stance);
        let bare = of_load(self.sheathed());
        own.and_then(|dodge| dodge.exact(direction))
            .or_else(|| bare.and_then(|dodge| dodge.exact(direction)))
            .or_else(|| own.and_then(|dodge| dodge.of(direction)))
            .or_else(|| bare.and_then(|dodge| dodge.of(direction)))
            .or_else(|| stance?.dodge?.of(direction))
    }

    /// The server's follow-up of the client's sequence `client`.
    pub fn follow_up(&self, client: u64) -> Option<FollowUp> {
        self.follow_ups.get(&client).copied()
    }

    /// Stances in the table.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// Whether it has no stance.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Abilities in the table ([`AbilityPlay`]).
    pub fn abilities_len(&self) -> usize {
        self.abilities.len()
    }
}

/// The stem of a station's display name: without the leading `@` and up to its tier (`T` and
/// a digit), else the whole name (`@SmeltingT2_MasterName`: `Smelting`;
/// `@ui_watcherscampfire`: `ui_watcherscampfire`).
fn station_stem(name: &str) -> &str {
    let name = name.trim().trim_start_matches('@');
    let bytes = name.as_bytes();
    let end = (1..bytes.len())
        .find(|&at| {
            char::from(bytes[at]) == STATION_TIER
                && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)
        })
        .unwrap_or(bytes.len());
    &name[..end]
}

#[cfg(test)]
mod tests {
    /// The stems of the retail station names (2026-11-19 study, `interact.py`).
    #[test]
    fn a_station_is_named_by_the_stem_of_its_display_name() {
        assert_eq!(super::station_stem("@SmeltingT2_MasterName"), "Smelting");
        assert_eq!(super::station_stem("@CookingKitT1_Name"), "CookingKit");
        assert_eq!(
            super::station_stem("@ui_watcherscampfire"),
            "ui_watcherscampfire"
        );
        assert_eq!(super::station_stem(""), "");
    }

    use super::*;

    /// Two stances and a follow-up as the table file holds them (values of the mined table).
    const TABLE: &str = r#"{
        "stances": [
            {"tags": "2H_Melee,GreatSword",
             "press": {"slot": "first", "sequence": 4120, "state": 10, "marker": 1},
             "releases": [{"after": {"slot": "first", "sequence": 4120},
                           "start": {"slot": "first", "sequence": 4510}}],
             "holds": [{"after": {"slot": "first", "sequence": 4120},
                        "start": {"slot": "first", "sequence": 57}, "records": 19}],
             "alt_start": {"second_sequence": 2351},
             "alt_moves": {"moving": {"state": 27, "sequence": 1251},
                           "idle": {"state": 31, "sequence": 1673}},
             "dodge": {"forward": {"state": 7, "sequence": 4007},
                       "back": {"state": 7, "sequence": 2100}}},
            {"tags": "2H_Ranged,Blunderbuss",
             "alt_start": {"second_sequence": 4248, "third_state": 46, "third_sequence": 407,
                           "marker": 40},
             "alt_end": {"third_state": 0, "third_sequence": 0, "marker": 0},
             "alt_fire": {"second_sequence": 94, "marker": 41},
             "reload": {"second_state": 36, "second_sequence": 4225}}
        ],
        "follow_ups": [{"client": 5051, "server": 484, "at_release": true, "seen": 9}],
        "abilities": [
            {"ability": "Ability_Flail_Trip", "first": {"state": 8, "sequence": 2344},
             "slots": {"marker": 1}, "seen": 15},
            {"ability": "Ability_Lifestaff_SacredGround",
             "slots": {"second_state": 36, "second_sequence": 142, "third_state": 46,
                       "third_sequence": 449, "marker": 33},
             "cast": {"second_state": 33, "second_sequence": 993, "third_state": 0,
                      "third_sequence": 0, "marker": 3}, "seen": 417}
        ]
    }"#;

    #[test]
    fn an_ability_animation_is_found_by_the_crc_of_its_lowercase_id() {
        let table = PlayerSequences::parse(TABLE).expect("table");
        let trip = table
            .ability(lowercase_crc32("ability_flail_trip"))
            .expect("trip");
        assert_eq!(
            trip.first,
            Some(FirstPlay {
                state: 8,
                sequence: 2344
            })
        );
        assert_eq!(trip.slots.marker, Some(1));
        let ground = table
            .ability(lowercase_crc32("Ability_Lifestaff_SacredGround"))
            .expect("sacred ground");
        assert_eq!(ground.first, None, "an upper-body ability");
        assert_eq!(
            ground.cast.map(|cast| cast.second_sequence),
            Some(Some(993))
        );
        assert_eq!(table.ability(lowercase_crc32("Ability_Unknown")), None);
        assert_eq!(table.abilities_len(), 2);
        let old = PlayerSequences::parse(r#"{"stances": [], "follow_ups": []}"#)
            .expect("a table without abilities");
        assert_eq!(old.ability(lowercase_crc32("Ability_Flail_Trip")), None);
        assert_eq!(old.abilities_len(), 0);
    }

    #[test]
    fn a_stance_reads_its_air_and_second_slot_ends_and_no_weapon_has_its_own() {
        let table = PlayerSequences::parse(
            r#"{"stances": [
                {"tags": "",
                 "air": {"fall": 1675,
                         "land_after_jump": {"still": 2376, "moving": 789},
                         "land_after_fall": {"still": 1355}}},
                {"tags": "2H_Melee,GreatSword",
                 "draw": {"state": 44, "sequence": 2467}, "draw_end_ms": 1096,
                 "sheathe": 2011, "sheathe_end_ms": 799}
            ], "follow_ups": []}"#,
        )
        .expect("table");
        let air = table.sheathed().and_then(|stance| stance.air).expect("air");
        assert_eq!(air.fall, Some(1675));
        assert_eq!(air.land_after_jump.of(true), Some(789));
        assert_eq!(air.land_after_jump.of(false), Some(2376));
        assert_eq!(
            air.land_after_fall.of(true),
            Some(1355),
            "the other landing"
        );
        let sword = table
            .stance(stance_key("2H_Melee,GreatSword"))
            .expect("greatsword");
        assert_eq!(
            (sword.draw_end_ms, sword.sheathe, sword.sheathe_end_ms),
            (Some(1096), Some(2011), Some(799))
        );
        assert_eq!(sword.air, None);
    }

    /// Live 2026-11-17: a medium-gear player's hop showed as the light roll to the others.
    #[test]
    fn a_dodge_plays_the_gear_load_s_own_then_no_weapon_s_of_that_load() {
        let table = PlayerSequences::parse(
            r#"{"stances": [
                {"tags": "",
                 "dodge": {"back": {"state": 7, "sequence": 4822}},
                 "dodge_loads": {"Normal": {"forward": {"state": 7, "sequence": 4841},
                                            "back": {"state": 7, "sequence": 4877}}}},
                {"tags": "2H_Melee,Spear,Throwable",
                 "dodge": {"forward": {"state": 7, "sequence": 4834},
                           "back": {"state": 7, "sequence": 4833}},
                 "dodge_loads": {"Normal": {"forward": {"state": 7, "sequence": 3355}}}}
            ], "follow_ups": []}"#,
        )
        .expect("table");
        let spear = table.stance(stance_key("2H_Melee,Spear,Throwable"));
        let sequence = |load, direction| {
            table
                .dodge(spear, load, direction)
                .map(|play| play.sequence)
        };
        assert_eq!(
            sequence(DodgeLoad::Fast, Direction::Back),
            Some(4833),
            "light"
        );
        assert_eq!(
            sequence(DodgeLoad::Normal, Direction::Forward),
            Some(3355),
            "the spear's medium hop"
        );
        assert_eq!(
            sequence(DodgeLoad::Normal, Direction::Back),
            Some(4877),
            "no spear medium back: no weapon's medium back"
        );
        assert_eq!(
            sequence(DodgeLoad::Slow, Direction::Back),
            Some(4833),
            "no heavy dodge at all: the light one"
        );
    }

    #[test]
    fn the_table_reads_by_stance_key() {
        let table = PlayerSequences::parse(TABLE).expect("table");
        assert_eq!(table.len(), 2);
        let sword = table
            .stance(stance_key("2H_Melee,GreatSword"))
            .expect("greatsword");
        let first = |sequence| RunningAttack {
            slot: AttackSlot::First,
            sequence,
        };
        let press = sword.press.expect("the press start");
        assert_eq!(
            (press.start, press.state, press.marker),
            (first(4120), 10, 1)
        );
        assert_eq!(sword.release_after(press.start), Some(first(4510)));
        assert_eq!(sword.hold_after(press.start), Some((first(57), 19)));
        assert_eq!(
            sword.release_after(first(4510)),
            None,
            "no step after the light"
        );
        assert_eq!(
            sword.alt_start.map(|change| change.second_sequence),
            Some(Some(2351))
        );
        let gun = table
            .stance(stance_key("2H_Ranged,Blunderbuss"))
            .expect("blunderbuss");
        assert_eq!(gun.press, None, "no mined press start");
        let aim = gun.alt_start.expect("aim");
        assert_eq!((aim.third_state, aim.third_sequence), (Some(46), Some(407)));
        assert!(!aim.is_empty());
        let shot = gun.alt_fire.expect("the aimed shot");
        assert_eq!((shot.second_sequence, shot.marker), (Some(94), Some(41)));
        let reload = gun.reload.expect("the reload");
        assert_eq!(
            (reload.second_state, reload.second_sequence),
            (Some(36), Some(4225))
        );
        assert_eq!(sword.alt_fire, None, "a melee weapon blocks");
        assert_eq!(
            table.follow_up(5051),
            Some(FollowUp {
                client: 5051,
                server: 484,
                at_release: true,
                seen: 9
            })
        );
        assert_eq!(table.follow_up(1), None);
    }

    #[test]
    fn a_stance_reads_its_release_shot_full_draw_and_hip_shot() {
        let table = PlayerSequences::parse(
            r#"{"stances": [
                {"tags": "Bow,LongBow",
                 "alt_fire": {"second_sequence": 35, "marker": 41},
                 "alt_fire_at_release": true,
                 "alt_fire_gap_ms": 1066,
                 "alt_fire_after": [{"after_ms": 433, "second_state": 39,
                                     "second_sequence": 2206, "marker": 40}],
                 "alt_charge": {"after_ms": 587, "drawn": {"marker": 24},
                                "fire": {"second_sequence": 38, "marker": 25},
                                "hold": {"after_ms": 133, "sequence": 928,
                                         "walk_third": 4275, "stand_third": 3077,
                                         "after": [{"after_ms": 3033,
                                                    "second_sequence": 32}]}},
                 "hip": {"start": {"second_state": 33, "second_sequence": 4720, "marker": 3},
                         "after": [{"after_ms": 33, "third_state": 46,
                                    "third_sequence": 4170, "marker": 67}],
                         "end": {"after_ms": 1000, "second_state": 45, "second_sequence": 0},
                         "refire": {"third_state": 0, "third_sequence": 0, "marker": 3}}},
                {"tags": "2H_Ranged,Blunderbuss",
                 "alt_hold": {"walk": {"start": 4225, "hold": 4248, "third": 407},
                              "stand": {"start": 4225, "hold": 4248, "third": 408},
                              "hold_after_ms": 83},
                 "alt_fire": {"second_sequence": 94, "marker": 41},
                 "alt_fire_empty_after": [{"after_ms": 766, "second_state": 39,
                                           "second_sequence": 96}],
                 "hip": {"start": {"second_state": 33, "second_sequence": 97, "marker": 67},
                         "end": {"after_ms": 898, "second_state": 45},
                         "refire": {"marker": 67},
                         "refire_window": {"from_ms": 733, "until_ms": 833},
                         "empty_after": [{"after_ms": 1332, "second_state": 45}]}}
            ], "follow_ups": []}"#,
        )
        .expect("table");
        let bow = table.stance(stance_key("Bow,LongBow")).expect("bow");
        assert!(bow.alt_fire_at_release);
        let nock = bow.alt_fire_after.first().expect("the next draw");
        assert_eq!(
            (nock.after_ms, nock.slots.second_state, nock.slots.marker),
            (433, Some(39), Some(40))
        );
        let charge = bow.alt_charge.as_ref().expect("the full draw");
        assert_eq!(
            (charge.after_ms, charge.drawn.marker, charge.fire.marker),
            (587, Some(24), Some(25))
        );
        assert!(charge.fire_after.is_empty());
        let hold = charge.hold.as_ref().expect("the full draw's hold");
        assert_eq!(
            (
                hold.after_ms,
                hold.sequence,
                hold.walk_third,
                hold.stand_third
            ),
            (133, 928, 4275, 3077)
        );
        assert_eq!(
            hold.after
                .first()
                .map(|step| (step.after_ms, step.slots.second_sequence)),
            Some((3033, Some(32)))
        );
        let hip = bow.hip.as_ref().expect("the hip shot");
        assert_eq!(hip.start.second_sequence, Some(4720));
        assert_eq!(hip.after.first().map(|step| step.after_ms), Some(33));
        assert_eq!(
            (hip.end.after_ms, hip.end.slots.second_state),
            (1000, Some(45))
        );
        assert_eq!(hip.refire.and_then(|refire| refire.marker), Some(3));
        let gun = table
            .stance(stance_key("2H_Ranged,Blunderbuss"))
            .expect("blunderbuss");
        assert!(!gun.alt_fire_at_release, "it shoots at the press");
        assert_eq!(gun.alt_charge, None);
        let legs = gun.alt_hold.expect("the aim by legs");
        assert_eq!((legs.walk.third, legs.stand.third), (Some(407), Some(408)));
        assert_eq!(
            gun.alt_fire_empty_after.first().map(|step| step.after_ms),
            Some(766)
        );
        assert_eq!(
            (bow.alt_fire_gap_ms, gun.alt_fire_gap_ms),
            (Some(1066), None)
        );
        let hip = gun.hip.as_ref().expect("the hip shot");
        let window = hip.refire_window.expect("the re-fire window");
        assert!(window.holds(733) && window.holds(833) && !window.holds(834));
        assert!(!window.holds(732));
        assert_eq!(hip.empty_after.last().map(|step| step.after_ms), Some(1332));
        assert!(
            bow.hip
                .as_ref()
                .is_some_and(|hip| hip.refire_window.is_none())
        );
    }

    #[test]
    fn a_direction_the_ledgers_did_not_show_plays_the_default() {
        let table = PlayerSequences::parse(TABLE).expect("table");
        let dodge = table
            .stance(stance_key("2H_Melee,GreatSword"))
            .and_then(|stance| stance.dodge)
            .expect("dodge");
        assert_eq!(
            dodge.of(Direction::Back).map(|play| play.sequence),
            Some(2100)
        );
        assert_eq!(
            dodge.of(Direction::Left).map(|play| play.sequence),
            Some(4007)
        );
        assert_eq!(
            Direction::of_keys(true, false, true, false),
            Direction::Forward
        );
        assert_eq!(
            Direction::of_keys(false, false, false, true),
            Direction::Right
        );
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let missing = PlayerSequences::load(&ContentLayout::new("/nonexistent", "0"));

        assert!(matches!(missing, Err(PlayerSequencesError::Io { .. })));
    }

    #[test]
    fn a_reaction_entry_has_its_retail_sequence() {
        // The great sword's front stagger: `2960`.
        let text = r#"{"stances": [], "follow_ups": [], "reactions": [
            {"animations": "unarmed_r2_f+grtswrd_r2_f", "sequence": 2960}]}"#;
        let table = PlayerSequences::parse(text).expect("table");
        assert_eq!(table.reaction("unarmed_r2_f+grtswrd_r2_f"), Some(2960));
        assert_eq!(table.reaction("unarmed_r2_f"), None);
    }
}
