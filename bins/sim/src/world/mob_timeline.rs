// SPDX-License-Identifier: AGPL-3.0-or-later
//! The state numbers, clock and animation of each mob (mob-animation note, sections 1 to 5;
//! sailor-movement note, appendix A).
//!
//! The client places the states of a remote entity on a timeline by their number and time, and
//! shows the entity at its render clock between two states. Retail gives each mob its own
//! timeline: the first state has number 2 and a time anchor (µs since 2000-01-01, the clock of
//! `TimeSynchMsg`); every later state has the next number and its time after the anchor in ms.
//! An idle mob sends one state per 333 ms, a moving mob one per tick, a dead mob one per 67 ms;
//! the anchor moves on after about 60 s, so the 16-bit offset never wraps. For a creature whose
//! Mannequin fragments are known ([`animations_of`]), the states also name the fragment it plays
//! and the time in it.

use std::collections::{HashMap, HashSet};
use std::f32::consts::{PI, TAU};

use nw_messages::{
    AlcStamp, AssetId, FRAG_TAGS_LEN, InputPosition, LayerEntry, MobAnimation, MobStep, ScopeEntry,
    StepScope,
};
use nw_sim_core::{MobState, Position};

/// The number of the first state of a mob (retail sailors 1 to 3).
const FIRST_STATE_ID: u16 = 2;
/// An idle mob sends a state once per this many ms (retail sailor 1 while held: 16584 to 29048).
const IDLE_STATE_MS: u64 = 333;
/// A dead mob sends a state once per this many ms (retail sailor 1 after its death).
const DEAD_STATE_MS: u64 = 67;
/// The anchor moves on when a state is this many ms after it (retail: 60021 ms).
const ANCHOR_PERIOD_MS: u64 = 60_000;
/// Microseconds per millisecond.
const US_PER_MS: u64 = 1000;
/// Scope time ticks per second (`DAT_14807f948` = 1/30 s).
const TICKS_PER_SECOND: u64 = 30;
/// Milliseconds per second.
const MS_PER_SECOND: u64 = 1000;
/// How long the spawn fragment plays (retail sailor 1: first state at 16198 ms, idle at 17548).
const SPAWN_MS: u64 = 1350;
/// How long an attack fragment plays (retail: the next full-body fragment 1.5 s after
/// `Attack_Close_1H_Sword_01` starts).
const ATTACK_MS: u64 = 1500;
/// How long a reaction plays: `r_R2_F` 769-784 ms, `r_R3_F` 831-836 ms (legion and risen;
/// creature-animation note, section 5.2).
const REACT_LIGHT_MS: u64 = 775;
const REACT_HEAVY_MS: u64 = 835;
/// After the second reaction of a chain, hits do not make a mob reel until this long after the
/// last hit (retail `ReactionLimiter_Active`, off 2.0-2.08 s after the last hit; section 0.4).
const REACTION_LIMIT_MS: u64 = 2_000;
/// A mob that moved within this many ms is moving: the brain does not move a mob every tick.
const MOVE_HOLD_MS: u64 = IDLE_STATE_MS;
/// A smaller move is no move.
const MOVE_EPSILON_M: f32 = 0.01;
/// The most a mob turns per state (retail: 1.5 to 9 degrees per 33 ms state; rotation note,
/// section 3).
const TURN_PER_STATE_RAD: f32 = 9.0 * std::f32::consts::PI / 180.0;
/// A smaller turn is no turn.
const TURN_EPSILON_RAD: f32 = 0.5 * std::f32::consts::PI / 180.0;
/// How long the alert plays when a mob first sees its target (retail `Alert_Turn` / `Alert`:
/// 883 to 1066 ms on every creature; creature-animation note, section 5.1). The mob stands
/// while it plays: retail runs only after it.
pub(super) const ALERT_MS: u64 = 1_000;
/// The time of a look pose in the state it starts (retail `half` `3556` on the sailor and the
/// legionnaire).
const LOOK_POSE_START_S: f32 = 0.3335;
/// Milliseconds per second, as a float.
const MS_PER_SECOND_F32: f32 = 1_000.0;

/// What a mob does, as one full-body fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fragment {
    /// It comes into the world.
    Spawn,
    /// It stands and has no target.
    Idle,
    /// It stands and fights.
    CombatIdle,
    /// It walks.
    Move,
    /// It turns to its target the first time it sees it.
    Alert,
    /// It swings.
    Attack,
    /// It reels from a first hit.
    ReactLight,
    /// It reels from a second hit.
    ReactHeavy,
    /// It dies.
    Death,
}

/// One fragment of a creature: its fragment and action numbers and the last byte of a retail
/// entry of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FragmentEntry {
    fragment: u16,
    action: u16,
    tail: u8,
}

const fn entry(fragment: u16, action: u16, tail: u8) -> FragmentEntry {
    FragmentEntry {
        fragment,
        action,
        tail,
    }
}

/// The fragments and tags of one creature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Animations {
    spawn: FragmentEntry,
    idle: FragmentEntry,
    combat_idle: FragmentEntry,
    walk: FragmentEntry,
    attack: FragmentEntry,
    death: FragmentEntry,
    /// The full-body reactions to a first and a second hit; `None` when Unknown.
    reactions: Option<(FragmentEntry, FragmentEntry)>,
    /// The alert when it first sees its target; `None` for a creature that has none.
    alert: Option<FragmentEntry>,
    /// The look pose that plays while it fights (its scope bit and entry); `None` for a
    /// creature that has none.
    look_pose: Option<(u8, FragmentEntry)>,
    /// The tags of a mob without a target.
    calm_tags: [u8; FRAG_TAGS_LEN],
    /// The tags of a fighting mob.
    alert_tags: [u8; FRAG_TAGS_LEN],
}

impl Animations {
    const fn entry(&self, fragment: Fragment) -> Option<FragmentEntry> {
        Some(match fragment {
            Fragment::Spawn => self.spawn,
            Fragment::Idle => self.idle,
            Fragment::CombatIdle => self.combat_idle,
            Fragment::Move => self.walk,
            Fragment::Alert => match self.alert {
                Some(alert) => alert,
                None => return None,
            },
            Fragment::Attack => self.attack,
            Fragment::Death => self.death,
            Fragment::ReactLight => match self.reactions {
                Some((light, _)) => light,
                None => return None,
            },
            Fragment::ReactHeavy => match self.reactions {
                Some((_, heavy)) => heavy,
                None => return None,
            },
        })
    }
}

/// The `scopeData` bit of the look pose: sailor and legionnaire 4, swarmer 3, risen 2 (retail
/// masks 17, 9 and 5; creature-animation note, section 0). Thorpe sends none.
const LOOK_POSE_SCOPE_HUMANOID: u8 = 4;
const LOOK_POSE_SCOPE_SWARMER: u8 = 3;
const LOOK_POSE_SCOPE_RISEN: u8 = 2;

/// The tutorial's corrupted sailor (`Corrupted_Sailor_1H_Club_FTUE`): fragments of
/// `damned_controllerdefs.xml` and actions of `drownedsailor.grid`, with the tags and entry bytes
/// of the retail sailors (mob-animation note, sections 3.4 and 4; reactions `r_R2_F` then
/// `r_R1_F`, creature-animation note, section 5.4).
const FTUE_SAILOR: Animations = Animations {
    spawn: entry(185, 36, 0xc7),
    idle: entry(1, 10, 0xac),
    combat_idle: entry(30, 12, 0x85),
    walk: entry(182, 26, 0xeb),
    attack: entry(109, 71, 0x85),
    death: entry(99, 46, 0xa5),
    reactions: Some((entry(16, 39, 0x6a), entry(12, 39, 0xab))),
    alert: Some(entry(2, 2, 0x85)),
    look_pose: Some((LOOK_POSE_SCOPE_HUMANOID, entry(37, 4, 0x7b))),
    calm_tags: [
        0x04, 0x03, 0x28, 0x08, 0x8b, 0x80, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
    alert_tags: [
        0x04, 0x03, 0x28, 0x08, 0x8b, 0x82, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
};
/// The legionnaire and the signifier (`Legion_Legionnaire_FTUE`; creature-animation note,
/// section 1). Retail's chase uses action 13 with the `Move_Combat` fragment.
const FTUE_LEGIONNAIRE: Animations = Animations {
    spawn: entry(143, 51, 0x7d),
    idle: entry(33, 41, 0x33),
    combat_idle: entry(12, 43, 0x05),
    walk: entry(47, 13, 0x27),
    attack: entry(39, 2, 0x3e),
    death: entry(151, 50, 0x40),
    reactions: Some((entry(110, 32, 0x96), entry(114, 32, 0x0d))),
    alert: Some(entry(7, 12, 0x53)),
    look_pose: Some((LOOK_POSE_SCOPE_HUMANOID, entry(45, 25, 0xe9))),
    calm_tags: [
        0x05, 0x02, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
    alert_tags: [
        0x85, 0x02, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
};
/// The corruption swarmer (`Corruption_Entity_Crawler_FTUE`; creature-animation note, section 2).
/// Its combat idle and reactions were never seen (each died from one hit): it stands in its idle
/// (our choice) and does not reel.
const FTUE_SWARMER: Animations = Animations {
    spawn: entry(86, 74, 0xa5),
    idle: entry(1, 66, 0x33),
    combat_idle: entry(1, 66, 0x33),
    walk: entry(79, 72, 0x41),
    attack: entry(44, 39, 0xe5),
    death: entry(171, 56, 0x72),
    reactions: None,
    alert: Some(entry(21, 1, 0x0f)),
    look_pose: Some((LOOK_POSE_SCOPE_SWARMER, entry(65, 51, 0xda))),
    calm_tags: [
        0x10, 0x40, 0x40, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
    alert_tags: [
        0x50, 0x40, 0x40, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
};
/// The risen corruption entity (`Corruption_Entity`; creature-animation note, section 3). Retail
/// never sent a plain idle: it rummages.
const FTUE_RISEN: Animations = Animations {
    spawn: entry(57, 17, 0x08),
    idle: entry(25, 27, 0x43),
    combat_idle: entry(18, 32, 0xec),
    walk: entry(46, 35, 0xd4),
    attack: entry(67, 11, 0x78),
    death: entry(4, 22, 0x8f),
    reactions: Some((entry(13, 20, 0xc9), entry(42, 20, 0x5b))),
    alert: Some(entry(37, 2, 0xa4)),
    look_pose: Some((LOOK_POSE_SCOPE_RISEN, entry(8, 16, 0xde))),
    calm_tags: [
        0x0b, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
    alert_tags: [
        0x0b, 0xb1, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
};
/// Thorpe (`Damned_Commander_FTUE`; creature-animation note, section 4). He never stands idle:
/// he strafes between attacks.
const FTUE_THORPE: Animations = Animations {
    spawn: entry(23, 68, 0xb0),
    idle: entry(65, 62, 0x2b),
    combat_idle: entry(65, 62, 0x2b),
    walk: entry(19, 61, 0xa1),
    attack: entry(58, 28, 0xa5),
    death: entry(5, 63, 0x14),
    reactions: Some((entry(27, 45, 0x47), entry(31, 45, 0x43))),
    alert: None,
    look_pose: None,
    calm_tags: [
        0x08, 0x01, 0x0b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
    alert_tags: [
        0x08, 0x81, 0x0b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
};
/// The creature slices of the tutorial's mobs, with their fragments.
const CREATURES: [([u8; 16], &Animations); 6] = [
    (
        // b7e33b29: the corrupted sailor.
        [
            0xb7, 0xe3, 0x3b, 0x29, 0x4b, 0xcf, 0x57, 0xb8, 0xa4, 0x76, 0xab, 0xdc, 0x3a, 0x31,
            0xfa, 0xe7,
        ],
        &FTUE_SAILOR,
    ),
    (
        // c05fc957: the legionnaire.
        [
            0xc0, 0x5f, 0xc9, 0x57, 0x84, 0x87, 0x55, 0x59, 0xaf, 0xe8, 0x74, 0xa7, 0x8f, 0x95,
            0xfb, 0x64,
        ],
        &FTUE_LEGIONNAIRE,
    ),
    (
        // 87a335e7: the signifier.
        [
            0x87, 0xa3, 0x35, 0xe7, 0x6b, 0x86, 0x50, 0xd5, 0xa4, 0x65, 0xfe, 0x08, 0x10, 0xbd,
            0xa5, 0xdf,
        ],
        &FTUE_LEGIONNAIRE,
    ),
    (
        // e06d1bb7: the corruption swarmer.
        [
            0xe0, 0x6d, 0x1b, 0xb7, 0x31, 0x1f, 0x5a, 0xcb, 0x98, 0x57, 0xcc, 0x20, 0x46, 0x4c,
            0xe4, 0xdf,
        ],
        &FTUE_SWARMER,
    ),
    (
        // a8b60bc7: the risen corruption entity.
        [
            0xa8, 0xb6, 0x0b, 0xc7, 0xfe, 0xaf, 0x57, 0x4c, 0xbe, 0x7a, 0xe3, 0xc2, 0xd8, 0xa4,
            0x8d, 0xec,
        ],
        &FTUE_RISEN,
    ),
    (
        // 929831a9: Thorpe.
        [
            0x92, 0x98, 0x31, 0xa9, 0x8d, 0xd2, 0x5e, 0x3e, 0x8b, 0xd8, 0xd4, 0x5d, 0x4e, 0x08,
            0x68, 0xac,
        ],
        &FTUE_THORPE,
    ),
];

/// The fragments of the creature of slice `asset`, when known.
pub(super) fn animations_of(asset: &AssetId) -> Option<&'static Animations> {
    CREATURES
        .iter()
        .find(|(id, _)| *id == asset.id)
        .map(|(_, animations)| *animations)
}

/// A mob as the bundle step sees it this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MobPose {
    /// Where it is.
    pub(super) position: Position,
    /// Its brain state; `None` for a mob of another sim.
    pub(super) brain: Option<MobState>,
    /// It has no health left.
    pub(super) dead: bool,
    /// Where its target is, while it fights.
    pub(super) target: Option<Position>,
}

impl MobPose {
    fn fighting(&self) -> bool {
        matches!(self.brain, Some(MobState::Chase | MobState::Attack))
    }
}

/// The states of one mob this tick: the state a new viewer gets with the card, and the later
/// state, when one is due.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MobFrame {
    /// The number and clock of the mob's newest state.
    pub(super) stamp: AlcStamp,
    /// What it shows now, for a creature whose fragments are known.
    pub(super) animation: Option<MobAnimation>,
    /// Where it faces (yaw, radians), once it turned.
    pub(super) facing: Option<f32>,
    /// The new state of this tick.
    pub(super) step: Option<MobStep>,
    /// Its alert started this tick: it stands for [`ALERT_MS`].
    pub(super) alert_started: bool,
}

/// The timeline of one mob.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MobTimeline {
    anchor_ms: u64,
    id: u16,
    last_state_ms: u64,
    /// The position of its newest state.
    position: Position,
    last_moved_ms: Option<u64>,
    born_ms: u64,
    fragment: Fragment,
    entry_start_id: u16,
    entry_start_ms: u64,
    /// The scope time of its newest state.
    ticks: u32,
    attack_until_ms: u64,
    /// The reaction it plays, and until when.
    reaction: Option<(Fragment, u64)>,
    /// Full reactions since the chain started.
    reactions_in_chain: u8,
    /// When the last hit came.
    last_hurt_ms: u64,
    /// Until when hits make it reel no more.
    reaction_limit_until_ms: u64,
    alert: bool,
    /// Its facing in its newest state; `None` until it first turns (its first state keeps the
    /// prefab's).
    yaw: Option<f32>,
    /// It saw a target once: the alert played.
    alerted: bool,
    /// Until when its alert plays.
    alert_until_ms: u64,
    /// The state number and time at which its look pose started.
    look_pose_start: Option<(u16, u64)>,
    /// Its last fragment change sent an angle to turn: the next one sends 0 (retail).
    facing_gap_sent: bool,
}

impl MobTimeline {
    /// A mob whose first state is at `now_ms`: retail starts the spawn fragment one state before
    /// it.
    const fn new(now_ms: u64, position: Position) -> Self {
        Self {
            anchor_ms: now_ms,
            id: FIRST_STATE_ID,
            last_state_ms: now_ms,
            position,
            last_moved_ms: None,
            born_ms: now_ms,
            fragment: Fragment::Spawn,
            entry_start_id: FIRST_STATE_ID - 1,
            entry_start_ms: now_ms,
            ticks: 0,
            attack_until_ms: now_ms,
            reaction: None,
            reactions_in_chain: 0,
            last_hurt_ms: now_ms,
            reaction_limit_until_ms: now_ms,
            alert: false,
            yaw: None,
            alerted: false,
            alert_until_ms: now_ms,
            look_pose_start: None,
            facing_gap_sent: false,
        }
    }

    fn stamp(&self, now_ms: u64) -> AlcStamp {
        AlcStamp {
            id: self.id,
            anchor_us: self.anchor_ms * US_PER_MS,
            // A state is due before the offset passes `ANCHOR_PERIOD_MS` + `IDLE_STATE_MS`.
            offset_ms: u16::try_from(now_ms.saturating_sub(self.anchor_ms)).unwrap_or(u16::MAX),
        }
    }

    fn ticks_at(&self, now_ms: u64) -> u32 {
        let ticks = now_ms.saturating_sub(self.entry_start_ms) * TICKS_PER_SECOND / MS_PER_SECOND;
        u32::try_from(ticks).unwrap_or(u32::MAX)
    }

    /// The time of its look pose at `now_ms`, while one plays.
    fn look_pose_seconds(&self, now_ms: u64) -> Option<f32> {
        self.look_pose_start.map(|(_, start_ms)| {
            #[allow(clippy::cast_precision_loss, reason = "a fight lasts minutes")]
            let elapsed = now_ms.saturating_sub(start_ms) as f32 / MS_PER_SECOND_F32;
            LOOK_POSE_START_S + elapsed
        })
    }

    fn animation(
        &self,
        animations: &Animations,
        now_ms: u64,
        facing_gap: Option<f32>,
    ) -> MobAnimation {
        // The timeline only takes fragments the creature has; the idle is the fallback.
        let entry = animations.entry(self.fragment).unwrap_or(animations.idle);
        let look_pose = match (animations.look_pose, self.look_pose_start) {
            (Some((scope_bit, layer)), Some((start_id, _))) => Some(LayerEntry {
                scope_bit,
                entry: ScopeEntry {
                    start_id,
                    fragment: layer.fragment,
                    action: layer.action,
                    tail: layer.tail,
                },
                seconds: self.look_pose_seconds(now_ms).unwrap_or(LOOK_POSE_START_S),
            }),
            _ => None,
        };
        MobAnimation {
            look_pose,
            facing_gap,
            entry: ScopeEntry {
                start_id: self.entry_start_id,
                fragment: entry.fragment,
                action: entry.action,
                tail: entry.tail,
            },
            ticks: self.ticks_at(now_ms),
            tags: if self.alert {
                animations.alert_tags
            } else {
                animations.calm_tags
            },
        }
    }

    /// The fragment of a mob in `pose` at `now_ms`.
    fn fragment_at(&self, pose: &MobPose, now_ms: u64) -> Fragment {
        if pose.dead {
            Fragment::Death
        } else if now_ms < self.attack_until_ms {
            Fragment::Attack
        } else if let Some((reaction, _)) = self.reaction.filter(|(_, until)| now_ms < *until) {
            reaction
        } else if now_ms < self.alert_until_ms {
            Fragment::Alert
        } else if self
            .last_moved_ms
            .is_some_and(|moved| now_ms.saturating_sub(moved) < MOVE_HOLD_MS)
        {
            Fragment::Move
        } else if now_ms.saturating_sub(self.born_ms) < SPAWN_MS {
            Fragment::Spawn
        } else if pose.fighting() {
            Fragment::CombatIdle
        } else {
            Fragment::Idle
        }
    }

    /// A hit at `now_ms` on a creature that can reel: the first hit of a chain plays the light
    /// reaction, the second the heavy one and starts the limit; while the limit runs, a hit only
    /// makes it longer (retail plays an additive reaction then; not sent here). Whether a
    /// reaction plays.
    fn hurt(&mut self, now_ms: u64) -> bool {
        if now_ms.saturating_sub(self.last_hurt_ms) >= REACTION_LIMIT_MS {
            self.reactions_in_chain = 0;
        }
        self.last_hurt_ms = now_ms;
        if now_ms < self.reaction_limit_until_ms {
            self.reaction_limit_until_ms = now_ms + REACTION_LIMIT_MS;
            return false;
        }
        self.reactions_in_chain += 1;
        self.reaction = Some(if self.reactions_in_chain == 1 {
            (Fragment::ReactLight, now_ms + REACT_LIGHT_MS)
        } else {
            self.reactions_in_chain = 0;
            self.reaction_limit_until_ms = now_ms + REACTION_LIMIT_MS;
            (Fragment::ReactHeavy, now_ms + REACT_HEAVY_MS)
        });
        true
    }

    /// The frame of `now_ms` (a server clock that may step back a little: differences saturate
    /// at 0): a new state when the mob moved, changed its fragment, or its
    /// period passed.
    fn advance(
        &mut self,
        now_ms: u64,
        pose: &MobPose,
        animations: Option<&Animations>,
    ) -> MobFrame {
        let moved = distance(self.position, pose.position) > MOVE_EPSILON_M;
        if moved {
            self.last_moved_ms = Some(now_ms);
        }
        // The first time it fights, it turns to its target in its alert.
        let alert_started = pose.fighting()
            && !pose.dead
            && !std::mem::replace(&mut self.alerted, true)
            && animations.is_some_and(|animations| animations.alert.is_some());
        if alert_started {
            self.alert_until_ms = now_ms + ALERT_MS;
        }
        let turn = self.turn(pose, moved);
        let fragment = self.fragment_at(pose, now_ms);
        let changed = fragment != self.fragment;
        let period_ms = if pose.dead {
            DEAD_STATE_MS
        } else {
            IDLE_STATE_MS
        };
        let due = moved
            || turn.is_some()
            || changed
            || now_ms.saturating_sub(self.last_state_ms) >= period_ms;
        let step = due.then(|| self.next_state(now_ms, pose, fragment, moved, turn, animations));
        MobFrame {
            stamp: self.stamp(now_ms),
            animation: animations.map(|animations| self.animation(animations, now_ms, None)),
            facing: self.yaw,
            step,
            alert_started,
        }
    }

    /// The facing of the next state when the mob turns: toward where it walks, else toward its
    /// target while it fights, at most [`TURN_PER_STATE_RAD`] per state (retail; rotation note,
    /// section 3). `None` when it does not turn.
    fn turn(&self, pose: &MobPose, moved: bool) -> Option<f32> {
        if pose.dead {
            return None;
        }
        let toward = |to: Position| yaw_toward(pose.position, to);
        let wanted = if moved {
            Some(yaw_toward(self.position, pose.position))
        } else {
            pose.target.filter(|_| pose.fighting()).map(toward)
        }?;
        let Some(now) = self.yaw else {
            return Some(wanted);
        };
        let gap = (wanted - now + PI).rem_euclid(TAU) - PI;
        (gap.abs() > TURN_EPSILON_RAD)
            .then(|| now + gap.clamp(-TURN_PER_STATE_RAD, TURN_PER_STATE_RAD))
    }

    /// The angle from its facing to its target (radians, -π to π), when both are known.
    fn facing_gap_to(&self, pose: &MobPose) -> Option<f32> {
        let now = self.yaw?;
        let wanted = yaw_toward(pose.position, pose.target?);
        Some((wanted - now + PI).rem_euclid(TAU) - PI)
    }

    fn next_state(
        &mut self,
        now_ms: u64,
        pose: &MobPose,
        fragment: Fragment,
        moved: bool,
        turn: Option<f32>,
        animations: Option<&Animations>,
    ) -> MobStep {
        self.id = self.id.wrapping_add(1);
        let anchored = now_ms.saturating_sub(self.anchor_ms) >= ANCHOR_PERIOD_MS;
        if anchored {
            self.anchor_ms = now_ms;
        }
        let changed = fragment != self.fragment;
        let mut facing_gap = None;
        if changed {
            self.fragment = fragment;
            self.entry_start_id = self.id;
            self.entry_start_ms = now_ms;
            self.alert = pose.fighting() || fragment == Fragment::Attack;
            // The look pose starts with the alert and plays on while the mob lives.
            if self.alert && self.look_pose_start.is_none() {
                self.look_pose_start = Some((self.id, now_ms));
            }
            // The alert says how far it still has to turn; the next fragment says 0.
            facing_gap = if fragment == Fragment::Alert {
                self.facing_gap_to(pose)
            } else {
                std::mem::take(&mut self.facing_gap_sent).then_some(0.0)
            };
            self.facing_gap_sent = facing_gap.is_some_and(|gap| gap != 0.0);
        }
        let ticks = self.ticks_at(now_ms);
        let high_changed = ticks >> u8::BITS != self.ticks >> u8::BITS;
        // Retail sends only the number and time after the death fragment.
        let scope = animations.and_then(|animations| {
            if changed {
                Some(StepScope::Changed(
                    self.animation(animations, now_ms, facing_gap),
                ))
            } else {
                (!pose.dead).then_some(StepScope::Ticks {
                    ticks,
                    high_changed,
                    look_pose_seconds: animations
                        .look_pose
                        .and_then(|_| self.look_pose_seconds(now_ms)),
                })
            }
        });
        self.ticks = ticks;
        self.last_state_ms = now_ms;
        self.position = pose.position;
        if turn.is_some() {
            self.yaw = turn;
        }
        MobStep {
            stamp: self.stamp(now_ms),
            anchored,
            position: moved
                .then(|| {
                    InputPosition::from_metres(pose.position.x, pose.position.y, pose.position.z)
                })
                .flatten(),
            facing: turn,
            scope,
        }
    }
}

/// The yaw that faces from `from` to `to` (0 faces north, +y; a positive yaw turns west).
pub(super) fn yaw_toward(from: Position, to: Position) -> f32 {
    (-(to.x - from.x)).atan2(to.y - from.y)
}

fn distance(a: Position, b: Position) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// The timelines of the mobs a sim shows.
#[derive(Debug, Clone, Default)]
pub(super) struct MobTimelines {
    by_mob: HashMap<u64, MobTimeline>,
    /// Mobs that swung since the last frame.
    attacked: HashSet<u64>,
    /// Mobs that the player hit since the last frame.
    hurt: HashSet<u64>,
    /// When each mob last started a reaction (server ms).
    reeled_ms: HashMap<u64, u64>,
}

impl MobTimelines {
    /// Mob `mob` swings at its target.
    pub(super) fn attacked(&mut self, mob: u64) {
        self.attacked.insert(mob);
    }

    /// A player hit mob `mob`.
    pub(super) fn hurt(&mut self, mob: u64) {
        self.hurt.insert(mob);
    }

    /// The frame of mob `mob` in `pose` at server time `now_ms` (ms since 2000-01-01); its first
    /// frame starts its timeline.
    pub(super) fn advance(
        &mut self,
        mob: u64,
        now_ms: u64,
        pose: &MobPose,
        animations: Option<&Animations>,
    ) -> MobFrame {
        let timeline = self
            .by_mob
            .entry(mob)
            .or_insert_with(|| MobTimeline::new(now_ms, pose.position));
        if self.attacked.remove(&mob) {
            timeline.attack_until_ms = now_ms + ATTACK_MS;
        }
        let can_react = animations.is_some_and(|animations| animations.reactions.is_some());
        if self.hurt.remove(&mob) && can_react && !pose.dead && timeline.hurt(now_ms) {
            self.reeled_ms.insert(mob, now_ms);
        }
        timeline.advance(now_ms, pose, animations)
    }

    /// Whether mob `mob` started a reaction at or after `since_ms` (server ms): retail dropped a
    /// creature's swing when a player's hit made it reel before the swing landed (swing note,
    /// 205594, 207659, 296654, 305722).
    pub(super) fn reeled_since(&self, mob: u64, since_ms: u64) -> bool {
        self.reeled_ms
            .get(&mob)
            .is_some_and(|&reeled| reeled >= since_ms)
    }

    /// Forgets the mobs not in `shown`.
    pub(super) fn keep_only(&mut self, shown: &HashSet<u64>) {
        self.by_mob.retain(|mob, _| shown.contains(mob));
        self.attacked.retain(|mob| shown.contains(mob));
        self.hurt.retain(|mob| shown.contains(mob));
        self.reeled_ms.retain(|mob, _| shown.contains(mob));
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    const MOB: u64 = 1 << 34;
    /// A server time (ms since 2000-01-01).
    const START: u64 = 845_730_278_284;
    const TICK_MS: u64 = 33;

    fn at(x: f32) -> Position {
        Position {
            x,
            y: 846.0,
            z: 90.0,
        }
    }

    fn pose(x: f32, brain: MobState) -> MobPose {
        MobPose {
            position: at(x),
            brain: Some(brain),
            dead: false,
            target: None,
        }
    }

    fn scope_entry(step: &MobStep) -> Option<ScopeEntry> {
        match step.scope {
            Some(StepScope::Changed(animation)) => Some(animation.entry),
            _ => None,
        }
    }

    fn animation_of(step: &MobStep) -> Option<MobAnimation> {
        match step.scope {
            Some(StepScope::Changed(animation)) => Some(animation),
            _ => None,
        }
    }

    #[test]
    fn a_mob_alerts_once_then_fights_with_its_look_pose() {
        let mut timelines = MobTimelines::default();
        // Idle first, facing north after a step north.
        timelines.advance(
            MOB,
            START,
            &pose(1.0, MobState::Idle),
            Some(&FTUE_LEGIONNAIRE),
        );
        let north = MobPose {
            position: Position {
                y: 847.0,
                ..at(1.0)
            },
            ..pose(1.0, MobState::Idle)
        };
        timelines.advance(MOB, START + SPAWN_MS, &north, Some(&FTUE_LEGIONNAIRE));
        let seen = START + SPAWN_MS + MOVE_HOLD_MS;
        let fighting = MobPose {
            brain: Some(MobState::Chase),
            target: Some(Position {
                x: -10.0,
                ..north.position
            }),
            ..north
        };
        let alert = timelines.advance(MOB, seen, &fighting, Some(&FTUE_LEGIONNAIRE));
        assert!(alert.alert_started, "the sim holds it for the alert");
        let step = alert.step.expect("a new fragment");
        let animation = animation_of(&step).expect("changed");
        assert_eq!(animation.entry.fragment, 7, "Alert_Turn");
        let layer = animation.look_pose.expect("the look pose starts with it");
        assert_eq!(layer.entry.start_id, step.stamp.id);
        assert_eq!((layer.scope_bit, layer.entry.fragment), (4, 45));
        assert!(
            animation.facing_gap.is_some_and(|gap| gap > 0.0),
            "it turns west"
        );
        let later = timelines.advance(MOB, seen + TICK_MS, &fighting, Some(&FTUE_LEGIONNAIRE));
        assert!(!later.alert_started, "once only");
        let after = timelines
            .advance(MOB, seen + ALERT_MS, &fighting, Some(&FTUE_LEGIONNAIRE))
            .step
            .expect("the combat idle");
        let animation = animation_of(&after).expect("changed");
        assert_eq!(
            animation.entry.fragment,
            FTUE_LEGIONNAIRE.combat_idle.fragment
        );
        assert_eq!(
            animation.look_pose.map(|layer| layer.entry.start_id),
            Some(step.stamp.id),
            "the look pose goes on"
        );
        assert_eq!(
            animation.facing_gap,
            Some(0.0),
            "retail: 0 at the next fragment"
        );
        let ticking = timelines
            .advance(
                MOB,
                seen + ALERT_MS + IDLE_STATE_MS,
                &fighting,
                Some(&FTUE_LEGIONNAIRE),
            )
            .step
            .expect("a period");
        let Some(StepScope::Ticks {
            look_pose_seconds: Some(seconds),
            ..
        }) = ticking.scope
        else {
            panic!("the look pose time goes with each state: {ticking:?}");
        };
        assert!(seconds > LOOK_POSE_START_S + 1.0);
    }

    #[test]
    fn thorpe_has_no_alert_and_no_look_pose() {
        let mut timelines = MobTimelines::default();
        let frame = timelines.advance(MOB, START, &pose(1.0, MobState::Chase), Some(&FTUE_THORPE));
        assert!(!frame.alert_started);
        assert_eq!(
            frame.animation.and_then(|animation| animation.look_pose),
            None
        );
    }

    #[test]
    fn every_tutorial_creature_slice_has_its_table() {
        // The slices of the tutorial's mob cards (ftue-entities table).
        let slices = [
            ("b7e33b294bcf57b8a476abdc3a31fae7", &FTUE_SAILOR),
            ("c05fc95784875559afe874a78f95fb64", &FTUE_LEGIONNAIRE),
            ("87a335e76b8650d5a465fe0810bda5df", &FTUE_LEGIONNAIRE),
            ("e06d1bb7311f5acb9857cc20464ce4df", &FTUE_SWARMER),
            ("a8b60bc7feaf574cbe7ae3c2d8a48dec", &FTUE_RISEN),
            ("929831a98dd25e3e8bd8d45d4e0868ac", &FTUE_THORPE),
        ];
        for (hex, table) in slices {
            let id: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
                .collect();
            let asset = AssetId {
                id: id.try_into().expect("16 bytes"),
                sub_id: 2,
            };
            assert_eq!(animations_of(&asset), Some(table), "{hex}");
        }
    }

    #[test]
    fn the_first_frame_is_state_2_at_its_own_anchor() {
        let mut timelines = MobTimelines::default();
        let frame = timelines.advance(MOB, START, &pose(1.0, MobState::Idle), Some(&FTUE_SAILOR));
        assert_eq!(
            frame.stamp,
            AlcStamp {
                id: FIRST_STATE_ID,
                anchor_us: START * US_PER_MS,
                offset_ms: 0
            }
        );
        assert_eq!(frame.step, None, "the card carries the first state");
        let animation = frame.animation.expect("a known creature");
        assert_eq!(animation.entry.fragment, FTUE_SAILOR.spawn.fragment);
        assert_eq!(animation.entry.start_id, FIRST_STATE_ID - 1);
        assert_eq!(animation.tags, FTUE_SAILOR.calm_tags);
    }

    #[test]
    fn an_idle_mob_sends_a_state_per_period_and_then_its_idle_fragment() {
        let mut timelines = MobTimelines::default();
        let idle = pose(1.0, MobState::Idle);
        timelines.advance(MOB, START, &idle, Some(&FTUE_SAILOR));
        let mut steps = Vec::new();
        for tick in 1..=60 {
            let now = START + tick * TICK_MS;
            if let Some(step) = timelines.advance(MOB, now, &idle, Some(&FTUE_SAILOR)).step {
                steps.push((now, step));
            }
        }
        let ids: Vec<u16> = steps.iter().map(|(_, step)| step.stamp.id).collect();
        assert_eq!(
            ids,
            (3..3 + ids.len() as u16).collect::<Vec<_>>(),
            "+1 per state"
        );
        assert!(
            steps
                .windows(2)
                .all(|pair| pair[1].0 - pair[0].0 <= IDLE_STATE_MS + TICK_MS)
        );
        for (now, step) in &steps {
            assert_eq!(u64::from(step.stamp.offset_ms), now - START);
            assert!(!step.anchored);
            assert_eq!(step.position, None);
        }
        let idle_at = steps
            .iter()
            .find(|(_, step)| scope_entry(step).is_some())
            .expect("the idle fragment starts");
        assert!(idle_at.0 - START >= SPAWN_MS);
        let entry = scope_entry(&idle_at.1).expect("entry");
        assert_eq!(entry.fragment, FTUE_SAILOR.idle.fragment);
        assert_eq!(entry.start_id, idle_at.1.stamp.id);
    }

    #[test]
    fn a_moving_mob_sends_every_tick_with_its_position_and_walk() {
        let mut timelines = MobTimelines::default();
        // Its alert plays first, while it stands.
        timelines.advance(MOB, START, &pose(1.0, MobState::Chase), Some(&FTUE_SAILOR));
        let start = START + ALERT_MS;
        timelines.advance(MOB, start, &pose(1.0, MobState::Chase), Some(&FTUE_SAILOR));
        let first = timelines
            .advance(
                MOB,
                start + TICK_MS,
                &pose(1.2, MobState::Chase),
                Some(&FTUE_SAILOR),
            )
            .step
            .expect("moved");
        assert_eq!(
            scope_entry(&first).map(|entry| entry.fragment),
            Some(FTUE_SAILOR.walk.fragment)
        );
        let second = timelines
            .advance(
                MOB,
                start + 2 * TICK_MS,
                &pose(1.4, MobState::Chase),
                Some(&FTUE_SAILOR),
            )
            .step
            .expect("moved");
        assert_eq!(second.stamp.id, first.stamp.id + 1);
        assert!(matches!(
            second.scope,
            Some(StepScope::Ticks { ticks: 0 | 1, .. })
        ));
        assert_eq!(second.position.map(|pos| pos.x), Some(1.4));
    }

    #[test]
    fn a_mob_faces_where_it_walks_then_turns_to_its_target_step_by_step() {
        let mut timelines = MobTimelines::default();
        timelines.advance(MOB, START, &pose(1.0, MobState::Chase), Some(&FTUE_SAILOR));
        let east = timelines
            .advance(
                MOB,
                START + TICK_MS,
                &pose(1.2, MobState::Chase),
                Some(&FTUE_SAILOR),
            )
            .step
            .expect("moved");
        assert_eq!(east.facing, Some(-FRAC_PI_2), "walking east");
        let north = Position {
            x: 1.2,
            y: 900.0,
            z: 90.0,
        };
        let mut facing = -FRAC_PI_2;
        let mut now = START + TICK_MS;
        for _ in 0..20 {
            now += TICK_MS;
            let standing = MobPose {
                target: Some(north),
                ..pose(1.2, MobState::Attack)
            };
            let frame = timelines.advance(MOB, now, &standing, Some(&FTUE_SAILOR));
            if let Some(next) = frame.step.and_then(|step| step.facing) {
                assert!((next - facing).abs() <= TURN_PER_STATE_RAD + f32::EPSILON);
                facing = next;
            }
        }
        assert!(facing.abs() < TURN_EPSILON_RAD, "it faces north: {facing}");
        let frame = timelines.advance(MOB, now + TICK_MS, &pose(1.2, MobState::Attack), None);
        assert_eq!(frame.facing, Some(facing), "the card faces the same way");
    }

    #[test]
    fn a_reaction_is_kept_for_the_swings_that_started_before_it() {
        let mut timelines = MobTimelines::default();
        let fighting = pose(1.0, MobState::Attack);
        timelines.advance(MOB, START, &fighting, Some(&FTUE_SAILOR));
        let swing_started = START + SPAWN_MS;
        assert!(!timelines.reeled_since(MOB, swing_started));
        timelines.hurt(MOB);
        let reeled = swing_started + TICK_MS;
        timelines.advance(MOB, reeled, &fighting, Some(&FTUE_SAILOR));
        assert!(timelines.reeled_since(MOB, swing_started));
        assert!(
            !timelines.reeled_since(MOB, reeled + 1),
            "a later swing is not dropped"
        );
    }

    #[test]
    fn hits_play_a_light_then_a_heavy_reaction_then_the_limit_holds() {
        let mut timelines = MobTimelines::default();
        let fighting = pose(1.0, MobState::Attack);
        timelines.advance(MOB, START, &fighting, Some(&FTUE_SAILOR));
        let fragment_of = |step: Option<MobStep>| match step.and_then(|step| step.scope) {
            Some(StepScope::Changed(animation)) => Some(animation.entry.fragment),
            _ => None,
        };
        let mut now = START + SPAWN_MS;
        timelines.hurt(MOB);
        let light = timelines
            .advance(MOB, now, &fighting, Some(&FTUE_SAILOR))
            .step;
        assert_eq!(
            fragment_of(light),
            Some(FTUE_SAILOR.reactions.expect("known").0.fragment)
        );
        now += TICK_MS;
        timelines.hurt(MOB);
        let heavy = timelines
            .advance(MOB, now, &fighting, Some(&FTUE_SAILOR))
            .step;
        assert_eq!(
            fragment_of(heavy),
            Some(FTUE_SAILOR.reactions.expect("known").1.fragment)
        );
        // While the limit runs, a hit does not start another reaction.
        now += REACT_HEAVY_MS;
        timelines.advance(MOB, now, &fighting, Some(&FTUE_SAILOR));
        timelines.hurt(MOB);
        now += TICK_MS;
        let limited = timelines
            .advance(MOB, now, &fighting, Some(&FTUE_SAILOR))
            .step;
        assert_ne!(
            fragment_of(limited),
            Some(FTUE_SAILOR.reactions.expect("known").0.fragment)
        );
        // A creature without known reactions does not reel.
        let mut swarmers = MobTimelines::default();
        swarmers.advance(MOB, START, &fighting, Some(&FTUE_SWARMER));
        swarmers.hurt(MOB);
        let step = swarmers
            .advance(MOB, START + SPAWN_MS, &fighting, Some(&FTUE_SWARMER))
            .step;
        assert_eq!(fragment_of(step), Some(FTUE_SWARMER.combat_idle.fragment));
    }

    #[test]
    fn a_swing_plays_the_attack_and_a_creature_without_fragments_sends_no_scope() {
        let mut timelines = MobTimelines::default();
        let fighting = pose(1.0, MobState::Attack);
        timelines.advance(MOB, START, &fighting, None);
        timelines.attacked(MOB);
        let step = timelines
            .advance(MOB, START + TICK_MS, &fighting, None)
            .step
            .expect("the attack starts");
        assert_eq!(step.scope, None);
        let mut sailors = MobTimelines::default();
        sailors.advance(MOB, START, &fighting, Some(&FTUE_SAILOR));
        sailors.attacked(MOB);
        let step = sailors
            .advance(MOB, START + TICK_MS, &fighting, Some(&FTUE_SAILOR))
            .step
            .expect("the attack starts");
        let Some(StepScope::Changed(animation)) = step.scope else {
            panic!("a new fragment");
        };
        assert_eq!(animation.entry.fragment, FTUE_SAILOR.attack.fragment);
        assert_eq!(animation.tags, FTUE_SAILOR.alert_tags);
    }

    #[test]
    fn the_anchor_moves_on_after_a_minute_and_a_dead_mob_sends_only_its_clock() {
        let mut timelines = MobTimelines::default();
        let idle = pose(1.0, MobState::Idle);
        timelines.advance(MOB, START, &idle, Some(&FTUE_SAILOR));
        let later = START + ANCHOR_PERIOD_MS;
        let step = timelines
            .advance(MOB, later, &idle, Some(&FTUE_SAILOR))
            .step
            .expect("due");
        assert!(step.anchored);
        assert_eq!(step.stamp.anchor_us, later * US_PER_MS);
        assert_eq!(step.stamp.offset_ms, 0);
        let dead = MobPose { dead: true, ..idle };
        let death = timelines
            .advance(MOB, later + TICK_MS, &dead, Some(&FTUE_SAILOR))
            .step
            .expect("death");
        assert_eq!(
            scope_entry(&death).map(|entry| entry.fragment),
            Some(FTUE_SAILOR.death.fragment)
        );
        let after = timelines
            .advance(
                MOB,
                later + TICK_MS + DEAD_STATE_MS,
                &dead,
                Some(&FTUE_SAILOR),
            )
            .step
            .expect("due");
        assert_eq!(after.scope, None);
        timelines.keep_only(&HashSet::new());
        let again = timelines.advance(MOB, later, &idle, None);
        assert_eq!(
            again.stamp.id, FIRST_STATE_ID,
            "a forgotten mob starts over"
        );
    }
}
