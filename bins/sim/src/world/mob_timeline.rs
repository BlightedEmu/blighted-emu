// SPDX-License-Identifier: AGPL-3.0-or-later
//! The state numbers, clock and animation of each mob (mob-animation note, sections 1 to 5;
//! sailor-movement note, appendix A).
//!
//! The client places the states of a remote entity on a timeline by their number and time, and
//! shows the entity at its render clock between two states. Retail gives each mob its own
//! timeline: the first state has number 2 and a time anchor (µs since 2000-01-01, the clock of
//! `TimeSynchMsg`); every later state has the next number and its time after the anchor in ms.
//! An idle mob sends one state per 333 ms, a moving mob one per tick, a dead mob one per 67 ms;
//! the anchor moves on after about 60 s, so the 16-bit offset never wraps. For a creature in the
//! creature animation table ([`animations_of`]), the states also name the fragment it plays and
//! the time in it.
//!
//! A mob that runs its behavior tree shows the CAGE action its tree plays (the grid action's
//! fragment and action numbers in its creature's files) and its tree's Mannequin tags; its hit
//! reactions and its death come from the timeline as for every creature, and a reaction's start
//! goes back to its tree ([`MobFrame::reaction_ms`]).

use std::collections::{HashMap, HashSet};

use nw_content::creature_animations::{CreatureAnimation, CreatureAnimations};
use nw_content::mob_prefabs::ASSET_ID_LEN;
use nw_messages::{
    AlcStamp, FRAG_TAGS_LEN, InputPosition, LayerEntry, MobAnimation, MobStep, ScopeEntry,
    StepPlace, StepScope,
};
pub(super) use nw_sim_core::yaw_toward;
use nw_sim_core::{Position, yaw_gap};

use crate::mobs::MoveFragment;

/// The number a fresh mob's spawn card carries: none (retail: all 119 fresh spawn cards of
/// newchar-retail-2026-09-18 carry number 0, no clock and no scope).
const CARD_STATE_ID: u16 = 0;
/// The number of a mob's first state, which starts its spawn fragment (retail sailor 1: state 1,
/// with scope entry `0001 b902 24 0000 77`, starting at state 1).
const FIRST_STATE_ID: u16 = 1;
/// How long after its card a fresh mob sends its first state, unless it moves, turns or acts
/// before (retail sailor 1: card at 16319 ms, state 1 at 16421 ms).
const FIRST_STATE_DELAY_MS: u64 = 100;
/// How many of a fresh mob's first states carry its draw (retail sailor 1: its paperdoll
/// change in the three groups after its card, 16421 to 16455 ms).
const DRAW_STATES: u32 = 3;
/// How far apart a fresh mob's draw states go (retail sailor 1: 16421, 16437 and 16455 ms); a
/// tick is longer, so they go in the next ticks.
const DRAW_STATE_MS: u64 = 16;
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
const CREATURE_REACTION_MS: (u64, u64) = (775, 835);
/// Thorpe's reactions: `r_R2_F` 850-903 ms, `r_R3_F` 1296-1303 ms (boss note).
const THORPE_REACTION_MS: (u64, u64) = (870, 1_300);
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
    /// It walks without a target.
    Move,
    /// It runs at its target.
    Chase,
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
    /// It plays a move of its pace (an attack of a boss).
    Play(MoveFragment),
    /// It plays the CAGE action of its tree.
    Tree(TreeEntry),
}

/// The CAGE action a mob's tree plays: its fragment and action numbers, and the count of its
/// tree's starts (a new start of the same action is a new entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TreeEntry {
    /// The fragment number.
    pub(super) fragment: u16,
    /// The action number.
    pub(super) action: u16,
    /// The start's count.
    pub(super) serial: u32,
}

/// What a mob that runs its tree shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TreePose {
    /// The action it plays, when its creature's files name it.
    pub(super) entry: Option<TreeEntry>,
    /// Its tag state: its creature's tags with its tree's.
    pub(super) tags: [u8; FRAG_TAGS_LEN],
    /// Where it faces, once it turned or moved.
    pub(super) facing: Option<f32>,
}

/// One fragment of a creature: its fragment and action numbers and the last byte of its entry.
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
    /// It walks without a target.
    walk: FragmentEntry,
    /// It runs at its target.
    chase: FragmentEntry,
    /// The swing of a paced mob: its retail attack, else its combat idle (a creature without a
    /// tree and without a retail attack never swings: it stands, `Mind::Still`).
    attack: FragmentEntry,
    death: FragmentEntry,
    /// The full-body reactions to a first and a second hit; `None` when it has none.
    reactions: Option<(FragmentEntry, FragmentEntry)>,
    /// The alert when it first sees its target; `None` for a creature that has none.
    alert: Option<FragmentEntry>,
    /// The look pose that plays while it fights (its scope bit and entry); `None` for a
    /// creature that has none. Its bit is the `LookPose` scope's place in the creature's scope
    /// definitions (retail masks 17, 9 and 5: sailor and legionnaire 4, swarmer 3, risen 2;
    /// creature-animation note, section 0).
    look_pose: Option<(u8, FragmentEntry)>,
    /// The tags of a mob without a target.
    calm_tags: [u8; FRAG_TAGS_LEN],
    /// The tags of a fighting mob.
    alert_tags: [u8; FRAG_TAGS_LEN],
    /// It shows its fighting tags only from its first reaction on (retail Thorpe: `08 01 0b`
    /// until his first reaction at 461718, through his first force grab and attacks).
    alert_after_hurt: bool,
    /// Its fighting tags in a boss's second phase (retail Thorpe from 503682: `08 81 13`).
    phase_two_tags: Option<[u8; FRAG_TAGS_LEN]>,
    /// How long its light and heavy reactions play.
    reaction_ms: (u64, u64),
    /// The last scope tick of its death fragment: the length of the option its entry's tail
    /// picks, in whole ticks (`CreatureAnimation::fragment_ticks`). Its dead states carry the
    /// ticks until then; `None` when the length is Unknown (the table has none): they carry
    /// them on.
    death_ticks: Option<u32>,
}

impl Animations {
    fn entry(&self, fragment: Fragment) -> Option<FragmentEntry> {
        Some(match fragment {
            Fragment::Spawn => self.spawn,
            Fragment::Idle => self.idle,
            Fragment::CombatIdle => self.combat_idle,
            Fragment::Move => self.walk,
            Fragment::Chase => self.chase,
            Fragment::Alert => self.alert?,
            Fragment::Attack => self.attack,
            Fragment::Death => self.death,
            Fragment::ReactLight => self.reactions?.0,
            Fragment::ReactHeavy => self.reactions?.1,
            Fragment::Play(played) => entry(played.fragment, played.action, played.tail),
            Fragment::Tree(played) => entry(
                played.fragment,
                played.action,
                self.tail_of(played.fragment),
            ),
        })
    }

    /// The last byte of a retail entry of fragment `fragment`, when retail showed one; else
    /// [`DEFAULT_ENTRY_TAIL`].
    fn tail_of(&self, fragment: u16) -> u8 {
        [
            Some(self.spawn),
            Some(self.idle),
            Some(self.combat_idle),
            Some(self.walk),
            Some(self.chase),
            Some(self.attack),
            Some(self.death),
            self.alert,
        ]
        .into_iter()
        .flatten()
        .find(|known| known.fragment == fragment)
        .map_or(DEFAULT_ENTRY_TAIL, |known| known.tail)
    }

    /// The animations of `creature` (its row of the creature animation table), with the retail
    /// choices of [`RETAIL_CREATURES`] for its slice `asset`; `None` without a spawn and an idle.
    pub(super) fn of(asset: &[u8; ASSET_ID_LEN], creature: &CreatureAnimation) -> Option<Self> {
        let retail = RETAIL_CREATURES
            .iter()
            .find(|retail| retail.asset == *asset);
        let pick = |role: Role| -> Option<FragmentEntry> {
            let chosen = retail
                .and_then(|retail| retail.choices.iter().find(|(chosen, _, _)| *chosen == role));
            let (fragment, action) = match chosen {
                Some((_, fragment, action)) => {
                    (creature.fragment(fragment)?, creature.action(action)?)
                }
                None => role.by_rule(creature)?,
            };
            let tail = retail
                .and_then(|retail| retail.tails.iter().find(|(tailed, _)| *tailed == role))
                .map_or(DEFAULT_ENTRY_TAIL, |(_, tail)| *tail);
            Some(entry(fragment, action, tail))
        };
        let spawn = pick(Role::Spawn)?;
        let idle = pick(Role::Idle)?;
        let combat_idle = pick(Role::CombatIdle).unwrap_or(idle);
        let walk = pick(Role::Walk);
        let chase = pick(Role::Chase);
        let light = pick(Role::ReactLight);
        let reactions = light.map(|light| (light, pick(Role::ReactHeavy).unwrap_or(light)));
        let death = pick(Role::Death).unwrap_or(idle);
        let fights_quietly = retail.is_some_and(|retail| retail.fights_quietly);
        let look_pose = pick(Role::LookPose)
            .zip(creature.scope_bit(LOOK_POSE))
            .map(|(layer, bit)| (bit, layer))
            .filter(|_| !fights_quietly);
        Some(Self {
            spawn,
            idle,
            combat_idle,
            walk: walk.or(chase).unwrap_or(idle),
            chase: chase.or(walk).unwrap_or(combat_idle),
            attack: pick(Role::Attack).unwrap_or(combat_idle),
            death,
            reactions,
            alert: pick(Role::Alert).filter(|_| !fights_quietly),
            look_pose,
            calm_tags: creature.tag_state(&[], &[]),
            alert_tags: creature.tag_state(&[], &[ALERTED_TAG]),
            alert_after_hurt: retail.is_some_and(|retail| retail.alert_after_hurt),
            phase_two_tags: creature
                .has_tag(PHASE_TWO_TAG)
                .then(|| creature.tag_state(&[PHASE_ONE_TAG], &[PHASE_TWO_TAG, ALERTED_TAG])),
            reaction_ms: retail.map_or(CREATURE_REACTION_MS, |retail| retail.reaction_ms),
            death_ticks: usize::from(death.fragment)
                .checked_sub(1)
                .and_then(|index| creature.fragments().get(index))
                .and_then(|name| creature.fragment_ticks(name, death.tail)),
        })
    }
}

/// The tag every fighting creature shows (creature-animation note: retail sets it in every
/// creature's fight; the boar's `22`, the risen's `0b b1`).
const ALERTED_TAG: &str = "Alerted";
/// A boss's phase tags: retail Thorpe shows `Phase_2` in place of `Phase_1` in his second phase
/// (`08 81 0b` to `08 81 13`).
const PHASE_ONE_TAG: &str = "Phase_1";
const PHASE_TWO_TAG: &str = "Phase_2";
/// The look pose's fragment, action and scope (one name for all three in every creature file).
const LOOK_POSE: &str = "LookPose";
/// The last byte of an entry whose retail value is Unknown (Inferred: the client takes any; retail
/// sent several for one fragment of one creature, the boar's `Move` 0x10, 0x14, 0x1d, 0x27,
/// 0x29).
const DEFAULT_ENTRY_TAIL: u8 = 0;

/// What a mob shows, by the names of its creature's fragments and actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Spawn,
    Idle,
    CombatIdle,
    Walk,
    Chase,
    Attack,
    Death,
    ReactLight,
    ReactHeavy,
    Alert,
    LookPose,
}

impl Role {
    /// The fragment and action names of the role, in order of preference (creature-animation
    /// note: the names retail played on the tutorial creatures and the boar).
    const fn names(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Self::Spawn => (&["Spawn"], &["Spawn"]),
            Self::Idle => (&["Idle"], &["Idle_No_Task"]),
            Self::CombatIdle => (&["Combat_Idle"], &["Combat_Idle"]),
            Self::Walk => (&["Move"], &["Move"]),
            Self::Chase => (&["Move_Combat"], &["Move_Combat"]),
            // A death plays the hit's direction in retail (Inferred); the plain one is ours.
            Self::Death => (
                &["r_Death", "r_Death_F", "r_Death_R4_F"],
                &["Death", "Death_FB"],
            ),
            Self::ReactLight => (&["r_R2_F"], &["Reaction"]),
            Self::ReactHeavy => (&["r_R3_F"], &["Reaction"]),
            Self::Alert => (&["Alert_Turn", "Alert"], &["Alert_Turn", "Alert"]),
            Self::LookPose => (&[LOOK_POSE], &[LOOK_POSE]),
            // A mob with a tree plays the CAGE action its tree picks ([`Fragment::Tree`]); the
            // swing of a paced mob is the attack retail played for its creature
            // ([`RetailCreature::choices`]). No name rule picks one.
            Self::Attack => (&[], &[]),
        }
    }

    /// The fragment and action numbers the role's names find in `creature`.
    fn by_rule(self, creature: &CreatureAnimation) -> Option<(u16, u16)> {
        let (fragments, actions) = self.names();
        let fragment = fragments.iter().find_map(|name| creature.fragment(name))?;
        let action = actions.iter().find_map(|name| creature.action(name))?;
        Some((fragment, action))
    }
}

/// What retail showed of one creature that the rules of [`Role`] do not give (creature-animation
/// note, sections 1 to 5; mob-animation note, sections 3.4 and 4).
struct RetailCreature {
    /// Its slice asset.
    asset: [u8; ASSET_ID_LEN],
    /// The fragment and action names retail played where the rules pick others.
    choices: &'static [(Role, &'static str, &'static str)],
    /// The last byte of its retail entries.
    tails: &'static [(Role, u8)],
    /// How long its light and heavy reactions play.
    reaction_ms: (u64, u64),
    /// See [`Animations::alert_after_hurt`].
    alert_after_hurt: bool,
    /// Retail sent no alert and no look pose for it.
    fights_quietly: bool,
}

/// The tutorial's corrupted sailor (`Corrupted_Sailor_1H_Club_FTUE`): reactions `r_R2_F` then
/// `r_R1_F` (creature-animation note, section 5.4).
const FTUE_SAILOR: RetailCreature = RetailCreature {
    asset: [
        0xb7, 0xe3, 0x3b, 0x29, 0x4b, 0xcf, 0x57, 0xb8, 0xa4, 0x76, 0xab, 0xdc, 0x3a, 0x31, 0xfa,
        0xe7,
    ],
    choices: &[
        // Retail entry fragment 109, action 71 (mob-animation note, section 3.4).
        (
            Role::Attack,
            "Attack_Close_1H_Sword_01",
            "Attack_Close_1H_Sword_01",
        ),
        (Role::Death, "r_Death_F", "Death"),
        (Role::ReactHeavy, "r_R1_F", "Reaction"),
        (Role::Alert, "Alert", "Alert"),
    ],
    tails: &[
        (Role::Spawn, 0xc7),
        (Role::Idle, 0xac),
        (Role::CombatIdle, 0x85),
        (Role::Chase, 0xeb),
        (Role::Attack, 0x85),
        (Role::Death, 0xa5),
        (Role::ReactLight, 0x6a),
        (Role::ReactHeavy, 0xab),
        (Role::Alert, 0x85),
        (Role::LookPose, 0x7b),
    ],
    reaction_ms: CREATURE_REACTION_MS,
    alert_after_hurt: false,
    fights_quietly: false,
};
/// The legionnaire and the signifier (`Legion_Legionnaire_FTUE`; creature-animation note,
/// section 1).
const LEGION_CHOICES: &[(Role, &str, &str)] = &[
    (
        Role::Attack,
        "Legionnaire_LightStab",
        "Legionnaire_LightStab",
    ),
    (Role::Death, "r_Death_R4_F", "Death_FB"),
];
const LEGION_TAILS: &[(Role, u8)] = &[
    (Role::Spawn, 0x7d),
    (Role::Idle, 0x33),
    (Role::CombatIdle, 0x05),
    (Role::Chase, 0x27),
    (Role::Attack, 0x3e),
    (Role::Death, 0x40),
    (Role::ReactLight, 0x96),
    (Role::ReactHeavy, 0x0d),
    (Role::Alert, 0x53),
    (Role::LookPose, 0xe9),
];
const FTUE_LEGIONNAIRE: RetailCreature = RetailCreature {
    asset: [
        0xc0, 0x5f, 0xc9, 0x57, 0x84, 0x87, 0x55, 0x59, 0xaf, 0xe8, 0x74, 0xa7, 0x8f, 0x95, 0xfb,
        0x64,
    ],
    choices: LEGION_CHOICES,
    tails: LEGION_TAILS,
    reaction_ms: CREATURE_REACTION_MS,
    alert_after_hurt: false,
    fights_quietly: false,
};
const FTUE_SIGNIFIER: RetailCreature = RetailCreature {
    asset: [
        0x87, 0xa3, 0x35, 0xe7, 0x6b, 0x86, 0x50, 0xd5, 0xa4, 0x65, 0xfe, 0x08, 0x10, 0xbd, 0xa5,
        0xdf,
    ],
    ..FTUE_LEGIONNAIRE
};
/// The corruption swarmer (`Corruption_Entity_Crawler_FTUE`; creature-animation note,
/// section 2). Its combat idle and reactions were never seen (each died from one hit): its own
/// files give them.
const FTUE_SWARMER: RetailCreature = RetailCreature {
    asset: [
        0xe0, 0x6d, 0x1b, 0xb7, 0x31, 0x1f, 0x5a, 0xcb, 0x98, 0x57, 0xcc, 0x20, 0x46, 0x4c, 0xe4,
        0xdf,
    ],
    choices: &[
        (
            Role::Attack,
            "Attack_Crawler_Slash_R",
            "Attack_Crawler_Slash_R",
        ),
        (Role::Death, "r_Death_R4_F", "Death"),
        (Role::Alert, "Alert", "Alert"),
    ],
    tails: &[
        (Role::Spawn, 0xa5),
        (Role::Idle, 0x33),
        (Role::Chase, 0x41),
        (Role::Attack, 0xe5),
        (Role::Death, 0x72),
        (Role::Alert, 0x0f),
        (Role::LookPose, 0xda),
    ],
    reaction_ms: CREATURE_REACTION_MS,
    alert_after_hurt: false,
    fights_quietly: false,
};
/// The risen corruption entity (`Corruption_Entity`; creature-animation note, section 3). Retail
/// never sent a plain idle: it rummages.
const FTUE_RISEN: RetailCreature = RetailCreature {
    asset: [
        0xa8, 0xb6, 0x0b, 0xc7, 0xfe, 0xaf, 0x57, 0x4c, 0xbe, 0x7a, 0xe3, 0xc2, 0xd8, 0xa4, 0x8d,
        0xec,
    ],
    choices: &[
        (Role::Idle, "POI_Rummage_LOOP", "POI_Waypoint"),
        (
            Role::Attack,
            "Attack_Stretch_Swipe_R",
            "Attack_Stretch_Swipe_R",
        ),
    ],
    tails: &[
        (Role::Spawn, 0x08),
        (Role::Idle, 0x43),
        (Role::CombatIdle, 0xec),
        (Role::Chase, 0xd4),
        (Role::Attack, 0x78),
        (Role::Death, 0x8f),
        (Role::ReactLight, 0xc9),
        (Role::ReactHeavy, 0x5b),
        (Role::Alert, 0xa4),
        (Role::LookPose, 0xde),
    ],
    reaction_ms: CREATURE_REACTION_MS,
    alert_after_hurt: false,
    fights_quietly: false,
};
/// Thorpe (`Damned_Commander_FTUE`; creature-animation note, section 4). He never stands idle:
/// he strafes between attacks.
const FTUE_THORPE: RetailCreature = RetailCreature {
    asset: [
        0x92, 0x98, 0x31, 0xa9, 0x8d, 0xd2, 0x5e, 0x3e, 0x8b, 0xd8, 0xd4, 0x5d, 0x4e, 0x08, 0x68,
        0xac,
    ],
    choices: &[
        (Role::Idle, "Strafe", "Strafe"),
        (Role::CombatIdle, "Strafe", "Strafe"),
        (Role::Attack, "Attack_Stab_01", "Attack_Stab_01"),
    ],
    tails: &[
        (Role::Spawn, 0xb0),
        (Role::Idle, 0x2b),
        (Role::CombatIdle, 0x2b),
        (Role::Chase, 0xa1),
        (Role::Attack, 0xa5),
        (Role::Death, 0x14),
        (Role::ReactLight, 0x47),
        (Role::ReactHeavy, 0x43),
    ],
    reaction_ms: THORPE_REACTION_MS,
    alert_after_hurt: true,
    fights_quietly: true,
};
/// The start beach's boar (`Boar`; glide note: q3 984 spawn `07 0001 22 01 0000 1c`, q3 3215
/// idle tail 0x0a, q3 77665 walk tail 0x29, q1 716003 death tail 0xe9 and dead ticks 0x58).
const BOAR: RetailCreature = RetailCreature {
    asset: [
        0xdb, 0x96, 0xd7, 0x65, 0xb5, 0x04, 0x59, 0xd9, 0x8c, 0x9e, 0x04, 0xad, 0xaa, 0x9c, 0x94,
        0x85,
    ],
    choices: &[],
    tails: &[
        (Role::Spawn, 0x1c),
        (Role::Idle, 0x0a),
        (Role::Walk, 0x29),
        (Role::Death, 0xe9),
    ],
    reaction_ms: CREATURE_REACTION_MS,
    alert_after_hurt: false,
    fights_quietly: false,
};

/// The creatures retail showed, with what it showed of them.
const RETAIL_CREATURES: [RetailCreature; 7] = [
    FTUE_SAILOR,
    FTUE_LEGIONNAIRE,
    FTUE_SIGNIFIER,
    FTUE_SWARMER,
    FTUE_RISEN,
    FTUE_THORPE,
    BOAR,
];

/// The animations of every creature of `table` that has a spawn and an idle, by slice asset.
pub(super) fn animations_of(table: &CreatureAnimations) -> HashMap<[u8; ASSET_ID_LEN], Animations> {
    table
        .iter()
        .filter_map(|(asset, creature)| Some((*asset, Animations::of(asset, creature)?)))
        .collect()
}

/// A mob as the bundle step sees it this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MobPose {
    /// Where it is.
    pub(super) position: Position,
    /// It fights a target (`false` for a mob of another sim).
    pub(super) fighting: bool,
    /// It has no health left.
    pub(super) dead: bool,
    /// Where its target is, while it fights.
    pub(super) target: Option<Position>,
    /// It fights in a boss's second phase.
    pub(super) phase_two: bool,
    /// What it shows, when it runs its tree.
    pub(super) tree: Option<TreePose>,
}

impl MobPose {
    fn fighting(&self) -> bool {
        self.fighting
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
    /// The `pos10` its states' offsets start from (a card carries it with the offset).
    pub(super) anchor: Option<InputPosition>,
    /// The new state of this tick.
    pub(super) step: Option<MobStep>,
    /// Its alert started this tick: it stands for [`ALERT_MS`].
    pub(super) alert_started: bool,
    /// It has sent no state yet: a card shows it as retail shows a fresh spawn
    /// ([`nw_messages::alc::AlcState::mob_spawn_card`]).
    pub(super) fresh: bool,
    /// The new state of this tick is one of its first [`DRAW_STATES`]: its draw goes with it.
    pub(super) drawing: bool,
}

/// The timeline of one mob.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MobTimeline {
    anchor_ms: u64,
    id: u16,
    last_state_ms: u64,
    /// The position of its newest state.
    position: Position,
    /// The `pos10` its states' offsets start from ([`StepPlace`]).
    anchor: Option<InputPosition>,
    last_moved_ms: Option<u64>,
    born_ms: u64,
    fragment: Fragment,
    entry_start_id: u16,
    entry_start_ms: u64,
    /// The scope time of its newest state.
    ticks: u32,
    attack_until_ms: u64,
    /// The move it plays, and until when.
    play: Option<(MoveFragment, u64)>,
    /// A hit made it react once: a creature with [`Animations::alert_after_hurt`] shows its
    /// fighting tags from then on.
    reacted: bool,
    /// It fights in a boss's second phase.
    phase_two: bool,
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
    /// The tag state of its tree, when it runs one.
    tree_tags: Option<[u8; FRAG_TAGS_LEN]>,
    /// It sent its first state: before it, a card shows it fresh, with no number and no scope.
    started: bool,
    /// States sent since its spawn, up to [`DRAW_STATES`] + 1.
    states: u32,
}

impl MobTimeline {
    /// A mob that spawns at `now_ms`: its card carries no state; its first state starts the
    /// spawn fragment ([`FIRST_STATE_DELAY_MS`]).
    fn new(now_ms: u64, position: Position) -> Self {
        Self {
            anchor_ms: now_ms,
            id: CARD_STATE_ID,
            last_state_ms: now_ms,
            position,
            anchor: InputPosition::from_metres(position.x, position.y, position.z),
            last_moved_ms: None,
            born_ms: now_ms,
            fragment: Fragment::Spawn,
            entry_start_id: FIRST_STATE_ID,
            entry_start_ms: now_ms,
            ticks: 0,
            attack_until_ms: now_ms,
            play: None,
            reacted: false,
            phase_two: false,
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
            tree_tags: None,
            started: false,
            states: 0,
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
            tags: match (self.tree_tags, self.alert, self.phase_two) {
                (Some(tags), _, _) => tags,
                (None, false, _) => animations.calm_tags,
                (None, true, true) => animations.phase_two_tags.unwrap_or(animations.alert_tags),
                (None, true, false) => animations.alert_tags,
            },
        }
    }

    /// The fragment of a mob in `pose` at `now_ms`.
    fn fragment_at(&self, pose: &MobPose, now_ms: u64) -> Fragment {
        if let Some(tree) = pose.tree {
            return if pose.dead {
                Fragment::Death
            } else if let Some((reaction, _)) = self.reaction.filter(|(_, until)| now_ms < *until) {
                reaction
            } else if now_ms.saturating_sub(self.born_ms) < SPAWN_MS {
                Fragment::Spawn
            } else {
                tree.entry.map_or(Fragment::Idle, Fragment::Tree)
            };
        }
        if pose.dead {
            Fragment::Death
        } else if let Some((played, _)) = self.play.filter(|(_, until)| now_ms < *until) {
            Fragment::Play(played)
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
            if pose.fighting() {
                Fragment::Chase
            } else {
                Fragment::Move
            }
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
    /// makes it longer (retail plays an additive reaction then; not sent here). How long the
    /// reaction plays, when one plays.
    fn hurt(&mut self, now_ms: u64, (light_ms, heavy_ms): (u64, u64)) -> Option<u64> {
        if now_ms.saturating_sub(self.last_hurt_ms) >= REACTION_LIMIT_MS {
            self.reactions_in_chain = 0;
        }
        self.last_hurt_ms = now_ms;
        if now_ms < self.reaction_limit_until_ms {
            self.reaction_limit_until_ms = now_ms + REACTION_LIMIT_MS;
            return None;
        }
        self.reactions_in_chain += 1;
        self.reacted = true;
        let (fragment, length_ms) = if self.reactions_in_chain == 1 {
            (Fragment::ReactLight, light_ms)
        } else {
            self.reactions_in_chain = 0;
            self.reaction_limit_until_ms = now_ms + REACTION_LIMIT_MS;
            (Fragment::ReactHeavy, heavy_ms)
        };
        self.reaction = Some((fragment, now_ms + length_ms));
        Some(length_ms)
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
        self.phase_two = pose.phase_two;
        self.tree_tags = pose.tree.map(|tree| tree.tags);
        // The first time it fights, it turns to its target in its alert (a tree plays its own).
        let alert_started = pose.tree.is_none()
            && pose.fighting()
            && !pose.dead
            && !std::mem::replace(&mut self.alerted, true)
            && animations.is_some_and(|animations| animations.alert.is_some());
        if alert_started {
            self.alert_until_ms = now_ms + ALERT_MS;
        }
        let turn = self.turn(pose, moved);
        let fragment = self.fragment_at(pose, now_ms);
        let changed = fragment != self.fragment;
        let period_ms = if !self.started {
            FIRST_STATE_DELAY_MS
        } else if self.states < DRAW_STATES {
            DRAW_STATE_MS
        } else if pose.dead {
            DEAD_STATE_MS
        } else {
            IDLE_STATE_MS
        };
        let due = moved
            || turn.is_some()
            || changed
            || now_ms.saturating_sub(self.last_state_ms) >= period_ms;
        let step = due.then(|| self.next_state(now_ms, pose, fragment, moved, turn, animations));
        let drawing = step.is_some() && self.states <= DRAW_STATES;
        MobFrame {
            stamp: self.stamp(now_ms),
            animation: animations
                .filter(|_| self.started)
                .map(|animations| self.animation(animations, now_ms, None)),
            facing: self.yaw,
            anchor: self.anchor,
            step,
            alert_started,
            fresh: !self.started,
            drawing,
        }
    }

    /// The facing of the next state when the mob turns: toward where it walks, else toward its
    /// target while it fights, at most [`TURN_PER_STATE_RAD`] per state (retail; rotation note,
    /// section 3). `None` when it does not turn.
    fn turn(&self, pose: &MobPose, moved: bool) -> Option<f32> {
        if pose.dead {
            return None;
        }
        if let Some(tree) = pose.tree {
            // A tree turns its mob at its own rates: the state carries its facing.
            let facing = tree.facing?;
            return self
                .yaw
                .is_none_or(|now| yaw_gap(now, facing).abs() > TURN_EPSILON_RAD)
                .then_some(facing);
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
        let gap = yaw_gap(now, wanted);
        (gap.abs() > TURN_EPSILON_RAD)
            .then(|| now + gap.clamp(-TURN_PER_STATE_RAD, TURN_PER_STATE_RAD))
    }

    /// The angle from its facing to its target (radians, -π to π), when both are known.
    fn facing_gap_to(&self, pose: &MobPose) -> Option<f32> {
        let now = self.yaw?;
        let wanted = yaw_toward(pose.position, pose.target?);
        Some(yaw_gap(now, wanted))
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
        // The first state brings the clock and the first scope entry: the card had neither.
        let first = !std::mem::replace(&mut self.started, true);
        self.states = self.states.saturating_add(1).min(DRAW_STATES + 1);
        let anchored = first || now_ms.saturating_sub(self.anchor_ms) >= ANCHOR_PERIOD_MS;
        if anchored {
            self.anchor_ms = now_ms;
        }
        let changed = first || fragment != self.fragment;
        let mut facing_gap = None;
        if changed {
            self.fragment = fragment;
            self.entry_start_id = self.id;
            self.entry_start_ms = now_ms;
            self.alert = if animations.is_some_and(|animations| animations.alert_after_hurt) {
                self.reacted
            } else {
                pose.fighting() || matches!(fragment, Fragment::Attack | Fragment::Play(_))
            };
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
        // A dead mob's states carry the ticks until its death fragment ends, then only the number
        // and time (retail sailor to tick 0x38, legionnaire 0x32; without them the client played
        // the death again at each state, live 2026-09-18).
        let scope = animations.and_then(|animations| {
            if changed {
                Some(StepScope::Changed(
                    self.animation(animations, now_ms, facing_gap),
                ))
            } else {
                let playing = !pose.dead || animations.death_ticks.is_none_or(|end| ticks <= end);
                playing.then_some(StepScope::Ticks {
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
        let position = moved
            .then(|| InputPosition::from_metres(pose.position.x, pose.position.y, pose.position.z))
            .flatten()
            .map(|at| {
                let place = StepPlace::of(self.anchor, at);
                if let StepPlace::Anchor(anchor) = place {
                    self.anchor = Some(anchor);
                }
                place
            });
        MobStep {
            stamp: self.stamp(now_ms),
            anchored,
            position,
            facing: turn,
            scope,
            awake: first,
        }
    }
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
    /// Mobs that started a move since the last frame: its fragment and length.
    played: HashMap<u64, (MoveFragment, u64)>,
    /// When each mob last started a reaction (server ms).
    reeled_ms: HashMap<u64, u64>,
    /// The reactions that started in the last frames and their lengths, until taken.
    reactions: HashMap<u64, u64>,
}

impl MobTimelines {
    /// Mob `mob` swings at its target.
    pub(super) fn attacked(&mut self, mob: u64) {
        self.attacked.insert(mob);
    }

    /// Mob `mob` plays the move fragment `fragment` for `length_ms`.
    pub(super) fn play(&mut self, mob: u64, fragment: MoveFragment, length_ms: u64) {
        self.played.insert(mob, (fragment, length_ms));
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
        if let Some((fragment, length_ms)) = self.played.remove(&mob) {
            timeline.play = Some((fragment, now_ms + length_ms));
        }
        let reaction_ms = animations
            .filter(|animations| animations.reactions.is_some())
            .map(|animations| animations.reaction_ms);
        if self.hurt.remove(&mob)
            && let Some(reaction_ms) = reaction_ms
            && !pose.dead
            && let Some(length_ms) = timeline.hurt(now_ms, reaction_ms)
        {
            self.reeled_ms.insert(mob, now_ms);
            self.reactions.insert(mob, length_ms);
        }
        timeline.advance(now_ms, pose, animations)
    }

    /// The length of the hit reaction mob `mob` started in its last frame, when one started (a
    /// tree mob's tree learns of it).
    pub(super) fn take_reaction(&mut self, mob: u64) -> Option<u64> {
        self.reactions.remove(&mob)
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
        self.played.retain(|mob, _| shown.contains(mob));
        self.reeled_ms.retain(|mob, _| shown.contains(mob));
        self.reactions.retain(|mob, _| shown.contains(mob));
    }

    /// Forgets `mob`: its next state starts a new timeline, as a new spawn's does.
    pub(super) fn forget(&mut self, mob: u64) {
        self.by_mob.remove(&mob);
        self.attacked.remove(&mob);
        self.hurt.remove(&mob);
        self.played.remove(&mob);
        self.reeled_ms.remove(&mob);
        self.reactions.remove(&mob);
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
    /// The last scope tick of each creature's death fragment in its retail dead states (dead-mob
    /// note: sailor 0x38, legionnaire 0x32, risen 0x49, Thorpe 0x64; glide note: boar 0x58).
    const SAILOR_DEATH_TICKS: u32 = 0x38;
    const LEGION_DEATH_TICKS: u32 = 0x32;
    const RISEN_DEATH_TICKS: u32 = 0x49;
    const THORPE_DEATH_TICKS: u32 = 0x64;
    const BOAR_DEATH_TICKS: u32 = 0x58;

    fn at(x: f32) -> Position {
        Position {
            x,
            y: 846.0,
            z: 90.0,
        }
    }

    fn pose(x: f32, fighting: bool) -> MobPose {
        MobPose {
            position: at(x),
            fighting,
            dead: false,
            target: None,
            phase_two: false,
            tree: None,
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

    /// The creature animation rows of the tutorial's creatures and the beach boar (made by
    /// `content-tool creature-animations` from build 25810808).
    fn table() -> CreatureAnimations {
        CreatureAnimations::parse(include_str!("testdata/creature-animations-tutorial.json"))
            .expect("fixture")
    }

    fn creature(retail: &RetailCreature) -> Animations {
        let table = table();
        let row = table.get(&retail.asset).expect("a row");
        Animations::of(&retail.asset, row).expect("animations")
    }

    fn sailor() -> Animations {
        creature(&FTUE_SAILOR)
    }

    fn legionnaire() -> Animations {
        creature(&FTUE_LEGIONNAIRE)
    }

    fn thorpe() -> Animations {
        creature(&FTUE_THORPE)
    }

    /// A creature whose files have no reactions.
    fn without_reactions() -> Animations {
        const ASSET: [u8; ASSET_ID_LEN] = [7; ASSET_ID_LEN];
        let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect();
        let row = nw_content::creature_animations::CreatureRow {
            asset: "07".repeat(ASSET_ID_LEN),
            slice: "slices/characters/test.dynamicslice".to_owned(),
            fragments: names(&[
                "Spawn",
                "Idle",
                "Combat_Idle",
                "Move",
                "Attack_Bite",
                "r_Death",
            ]),
            scopes: names(&["FullBody"]),
            actions: names(&[
                "Spawn",
                "Idle_No_Task",
                "Combat_Idle",
                "Move",
                "Attack_Bite",
                "Death",
            ]),
            tags: std::collections::BTreeMap::new(),
            default_tags: Vec::new(),
            fragment_ticks: std::collections::BTreeMap::new(),
            collider_radius_mm: None,
            skinnable: false,
        };
        let table = CreatureAnimations::of_rows(vec![row]).expect("table");
        Animations::of(&ASSET, table.get(&ASSET).expect("row")).expect("animations")
    }

    /// The tag state that starts with `bytes`.
    fn tags(bytes: &[u8]) -> [u8; FRAG_TAGS_LEN] {
        let mut tags = [0; FRAG_TAGS_LEN];
        tags[..bytes.len()].copy_from_slice(bytes);
        tags
    }

    #[test]
    fn the_files_give_the_retail_tutorial_entries_and_tags() {
        // The retail values (mob-animation note, sections 3.4 and 4; creature-animation note,
        // sections 1 to 5).
        let sailor = sailor();
        assert_eq!(sailor.spawn, entry(185, 36, 0xc7));
        assert_eq!(sailor.idle, entry(1, 10, 0xac));
        assert_eq!(sailor.combat_idle, entry(30, 12, 0x85));
        assert_eq!(sailor.chase, entry(182, 26, 0xeb));
        assert_eq!(sailor.attack, entry(109, 71, 0x85));
        assert_eq!(sailor.death, entry(99, 46, 0xa5));
        assert_eq!(
            sailor.reactions,
            Some((entry(16, 39, 0x6a), entry(12, 39, 0xab)))
        );
        assert_eq!(sailor.alert, Some(entry(2, 2, 0x85)));
        assert_eq!(sailor.look_pose, Some((4, entry(37, 4, 0x7b))));
        assert_eq!(
            sailor.calm_tags,
            tags(&[0x04, 0x03, 0x28, 0x08, 0x8b, 0x80, 0x08])
        );
        assert_eq!(
            sailor.alert_tags,
            tags(&[0x04, 0x03, 0x28, 0x08, 0x8b, 0x82, 0x08])
        );
        for legion in [legionnaire(), creature(&FTUE_SIGNIFIER)] {
            assert_eq!(legion.spawn, entry(143, 51, 0x7d));
            assert_eq!(legion.idle, entry(33, 41, 0x33));
            assert_eq!(legion.combat_idle, entry(12, 43, 0x05));
            assert_eq!(legion.chase, entry(47, 13, 0x27));
            assert_eq!(legion.attack, entry(39, 2, 0x3e));
            assert_eq!(legion.death, entry(151, 50, 0x40));
            assert_eq!(
                legion.reactions,
                Some((entry(110, 32, 0x96), entry(114, 32, 0x0d)))
            );
            assert_eq!(legion.alert, Some(entry(7, 12, 0x53)));
            assert_eq!(legion.look_pose, Some((4, entry(45, 25, 0xe9))));
            assert_eq!(legion.calm_tags, tags(&[0x05, 0x02, 0x00, 0x00, 0x02]));
            assert_eq!(legion.alert_tags, tags(&[0x85, 0x02, 0x00, 0x00, 0x02]));
        }
        let swarmer = creature(&FTUE_SWARMER);
        assert_eq!(swarmer.spawn, entry(86, 74, 0xa5));
        assert_eq!(swarmer.idle, entry(1, 66, 0x33));
        assert_eq!(swarmer.chase, entry(79, 72, 0x41));
        assert_eq!(swarmer.attack, entry(44, 39, 0xe5));
        assert_eq!(swarmer.death, entry(171, 56, 0x72));
        assert_eq!(swarmer.alert, Some(entry(21, 1, 0x0f)));
        assert_eq!(swarmer.look_pose, Some((3, entry(65, 51, 0xda))));
        assert_eq!(swarmer.calm_tags, tags(&[0x10, 0x40, 0x40, 0x01]));
        assert_eq!(swarmer.alert_tags, tags(&[0x50, 0x40, 0x40, 0x01]));
        let risen = creature(&FTUE_RISEN);
        assert_eq!(risen.spawn, entry(57, 17, 0x08));
        assert_eq!(risen.idle, entry(25, 27, 0x43));
        assert_eq!(risen.combat_idle, entry(18, 32, 0xec));
        assert_eq!(risen.chase, entry(46, 35, 0xd4));
        assert_eq!(risen.attack, entry(67, 11, 0x78));
        assert_eq!(risen.death, entry(4, 22, 0x8f));
        assert_eq!(
            risen.reactions,
            Some((entry(13, 20, 0xc9), entry(42, 20, 0x5b)))
        );
        assert_eq!(risen.alert, Some(entry(37, 2, 0xa4)));
        assert_eq!(risen.look_pose, Some((2, entry(8, 16, 0xde))));
        assert_eq!(risen.calm_tags, tags(&[0x0b, 0x31]));
        assert_eq!(risen.alert_tags, tags(&[0x0b, 0xb1]));
        let thorpe = thorpe();
        assert_eq!(thorpe.spawn, entry(23, 68, 0xb0));
        assert_eq!(thorpe.idle, entry(65, 62, 0x2b));
        assert_eq!(thorpe.combat_idle, entry(65, 62, 0x2b));
        assert_eq!(thorpe.chase, entry(19, 61, 0xa1));
        assert_eq!(thorpe.attack, entry(58, 28, 0xa5));
        assert_eq!(thorpe.death, entry(5, 63, 0x14));
        assert_eq!(
            thorpe.reactions,
            Some((entry(27, 45, 0x47), entry(31, 45, 0x43)))
        );
        assert_eq!((thorpe.alert, thorpe.look_pose), (None, None));
        assert_eq!(thorpe.calm_tags, tags(&[0x08, 0x01, 0x0b]));
        assert_eq!(thorpe.alert_tags, tags(&[0x08, 0x81, 0x0b]));
        assert_eq!(thorpe.phase_two_tags, Some(tags(&[0x08, 0x81, 0x13])));
        assert!(thorpe.alert_after_hurt);
        assert_eq!(thorpe.reaction_ms, THORPE_REACTION_MS);
        assert_eq!(sailor.phase_two_tags, None, "no phase tags");
    }

    #[test]
    fn each_death_lasts_the_option_its_tail_picks_as_retail_showed() {
        // The last tick of the retail dead states: each creature's death fragment option
        // (tail modulo the options) in whole ticks.
        assert_eq!(
            sailor().death_ticks,
            Some(SAILOR_DEATH_TICKS),
            "0xa5 % 5 options"
        );
        assert_eq!(legionnaire().death_ticks, Some(LEGION_DEATH_TICKS));
        assert_eq!(
            creature(&FTUE_RISEN).death_ticks,
            Some(RISEN_DEATH_TICKS),
            "0x8f % 2 options: the second"
        );
        assert_eq!(thorpe().death_ticks, Some(THORPE_DEATH_TICKS));
        // Not seen in the captures: its files' `r_Death_R4_F` for `Crawler`, 2.667 s.
        assert_eq!(creature(&FTUE_SWARMER).death_ticks, Some(80));
    }

    #[test]
    fn a_dead_mob_of_unknown_death_length_keeps_its_ticks() {
        let plain = without_reactions();
        assert_eq!(plain.death_ticks, None, "the row has no lengths");
        let mut timelines = MobTimelines::default();
        let dead = MobPose {
            dead: true,
            ..pose(1.0, true)
        };
        timelines.advance(MOB, START, &pose(1.0, true), Some(&plain));
        timelines.advance(MOB, START + SPAWN_MS, &dead, Some(&plain));
        let long_after = START + SPAWN_MS + 60 * DEAD_STATE_MS;
        let step = timelines
            .advance(MOB, long_after, &dead, Some(&plain))
            .step
            .expect("due");
        assert!(matches!(step.scope, Some(StepScope::Ticks { .. })));
    }

    #[test]
    fn the_boar_walks_with_its_retail_entries_and_tags() {
        let boar = creature(&BOAR);
        // Retail card `07 0001 22 01 0000 1c`, idle `0c 01 17 0000 0a`, walk `0b 1b 1f 0000 29`,
        // death `a8 04 11 0000 e9`; tags calm `02 00..`, alerted `22 00..` (glide note).
        assert_eq!(boar.spawn, entry(34, 1, 0x1c));
        assert_eq!(boar.idle, entry(1, 23, 0x0a));
        assert_eq!(boar.walk, entry(27, 31, 0x29));
        assert_eq!(boar.death, entry(4, 17, 0xe9));
        assert_eq!(boar.death_ticks, Some(BOAR_DEATH_TICKS));
        assert_eq!(boar.calm_tags, tags(&[0x02]));
        assert_eq!(boar.alert_tags, tags(&[0x22]));
        // Not seen in the captures: from its files (Inferred).
        assert_eq!(boar.chase, entry(26, 32, DEFAULT_ENTRY_TAIL), "Move_Combat");
        assert_eq!(
            boar.attack, boar.combat_idle,
            "its tree picks its attacks; no name rule does"
        );
        let mut timelines = MobTimelines::default();
        let card = timelines.advance(MOB, START, &pose(1.0, false), Some(&boar));
        assert!(
            card.fresh && card.animation.is_none(),
            "a fresh card shows no scope"
        );
        let first = timelines
            .advance(
                MOB,
                START + FIRST_STATE_DELAY_MS,
                &pose(1.0, false),
                Some(&boar),
            )
            .step
            .expect("the first state");
        let spawn = animation_of(&first).expect("the spawn starts");
        assert_eq!((spawn.entry.fragment, spawn.entry.action), (34, 1));
        assert_eq!(spawn.tags, boar.calm_tags);
        let wanders = timelines
            .advance(MOB, START + SPAWN_MS, &pose(1.2, false), Some(&boar))
            .step
            .expect("moved");
        assert_eq!(
            scope_entry(&wanders).map(|walk| (walk.fragment, walk.action, walk.tail)),
            Some((27, 31, 0x29)),
            "a moving boar plays Move, not a glide"
        );
    }

    #[test]
    fn every_fixture_creature_has_animations() {
        let table = table();
        let animations = animations_of(&table);
        assert_eq!(animations.len(), table.len());
        for retail in &RETAIL_CREATURES {
            assert!(animations.contains_key(&retail.asset));
        }
    }

    #[test]
    fn thorpe_plays_his_moves_and_shows_his_fight_tags_after_a_reaction() {
        let thorpe = thorpe();
        const STAB: MoveFragment = MoveFragment {
            fragment: 58,
            action: 28,
            tail: 0xa5,
        };
        const STAB_MS: u64 = 1_434;
        let mut timelines = MobTimelines::default();
        let fighting = pose(1.0, true);
        timelines.advance(MOB, START, &fighting, Some(&thorpe));
        let changed = |timelines: &mut MobTimelines, now, pose: &MobPose| {
            let frame = timelines.advance(MOB, now, pose, Some(&thorpe));
            frame
                .step
                .as_ref()
                .and_then(animation_of)
                .expect("a new fragment")
        };
        timelines.play(MOB, STAB, STAB_MS);
        let mut now = START + SPAWN_MS;
        let stab = changed(&mut timelines, now, &fighting);
        assert_eq!(stab.entry.fragment, STAB.fragment);
        assert_eq!(stab.tags, thorpe.calm_tags, "no reaction yet");
        now += STAB_MS;
        timelines.hurt(MOB);
        let reaction = changed(&mut timelines, now, &fighting);
        assert_eq!(reaction.tags, thorpe.alert_tags);
        now += THORPE_REACTION_MS.0;
        let phase_two = MobPose {
            phase_two: true,
            ..fighting
        };
        let after = changed(&mut timelines, now, &phase_two);
        assert_eq!(Some(after.tags), thorpe.phase_two_tags);
    }

    #[test]
    fn a_mob_alerts_once_then_fights_with_its_look_pose() {
        let legion = legionnaire();
        let mut timelines = MobTimelines::default();
        // Idle first, facing north after a step north.
        timelines.advance(MOB, START, &pose(1.0, false), Some(&legion));
        let north = MobPose {
            position: Position {
                y: 847.0,
                ..at(1.0)
            },
            ..pose(1.0, false)
        };
        timelines.advance(MOB, START + SPAWN_MS, &north, Some(&legion));
        let seen = START + SPAWN_MS + MOVE_HOLD_MS;
        let fighting = MobPose {
            fighting: true,
            target: Some(Position {
                x: -10.0,
                ..north.position
            }),
            ..north
        };
        let alert = timelines.advance(MOB, seen, &fighting, Some(&legion));
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
        let later = timelines.advance(MOB, seen + TICK_MS, &fighting, Some(&legion));
        assert!(!later.alert_started, "once only");
        let after = timelines
            .advance(MOB, seen + ALERT_MS, &fighting, Some(&legion))
            .step
            .expect("the combat idle");
        let animation = animation_of(&after).expect("changed");
        assert_eq!(animation.entry.fragment, legion.combat_idle.fragment);
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
                Some(&legion),
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
        let thorpe = thorpe();
        let mut timelines = MobTimelines::default();
        let frame = timelines.advance(MOB, START, &pose(1.0, true), Some(&thorpe));
        assert!(!frame.alert_started);
        assert_eq!(
            frame.animation.and_then(|animation| animation.look_pose),
            None
        );
    }

    #[test]
    fn the_card_is_fresh_and_the_first_state_starts_the_spawn_at_its_own_anchor() {
        let sailor = sailor();
        let mut timelines = MobTimelines::default();
        let card = timelines.advance(MOB, START, &pose(1.0, false), Some(&sailor));
        assert!(
            card.fresh,
            "retail: the card of a fresh spawn carries no state"
        );
        assert_eq!(card.stamp.id, CARD_STATE_ID);
        assert_eq!((card.step, card.animation), (None, None));
        let early = timelines.advance(MOB, START + TICK_MS, &pose(1.0, false), Some(&sailor));
        assert_eq!(
            early.step, None,
            "standing still, it waits for its first state"
        );
        let at = START + FIRST_STATE_DELAY_MS;
        let frame = timelines.advance(MOB, at, &pose(1.0, false), Some(&sailor));
        assert!(!frame.fresh && frame.drawing);
        let step = frame.step.expect("the first state");
        assert_eq!(
            step.stamp,
            AlcStamp {
                id: FIRST_STATE_ID,
                anchor_us: at * US_PER_MS,
                offset_ms: 0
            }
        );
        assert!(step.anchored, "the card had no clock");
        let animation = animation_of(&step).expect("the spawn starts");
        assert_eq!(animation.entry.fragment, sailor.spawn.fragment);
        assert_eq!(animation.entry.start_id, FIRST_STATE_ID);
        assert_eq!(animation.tags, sailor.calm_tags);
        // The draw goes with three states, one per tick as retail's 16 ms apart, then the idle
        // period holds.
        let frames: Vec<(bool, bool)> = (1..=DRAW_STATES + 1)
            .map(|tick| {
                let now = at + u64::from(tick) * TICK_MS;
                let frame = timelines.advance(MOB, now, &pose(1.0, false), Some(&sailor));
                (frame.step.is_some(), frame.drawing)
            })
            .collect();
        assert_eq!(
            frames,
            [(true, true), (true, true), (false, false), (false, false)]
        );
        let later = at + IDLE_STATE_MS + 3 * TICK_MS;
        let idle = timelines.advance(MOB, later, &pose(1.0, false), Some(&sailor));
        assert!(
            idle.step.is_some() && !idle.drawing,
            "then the idle period, no draw"
        );
    }

    #[test]
    fn an_idle_mob_sends_a_state_per_period_and_then_its_idle_fragment() {
        let sailor = sailor();
        let mut timelines = MobTimelines::default();
        let idle = pose(1.0, false);
        timelines.advance(MOB, START, &idle, Some(&sailor));
        let mut steps = Vec::new();
        for tick in 1..=60 {
            let now = START + tick * TICK_MS;
            if let Some(step) = timelines.advance(MOB, now, &idle, Some(&sailor)).step {
                steps.push((now, step));
            }
        }
        let ids: Vec<u16> = steps.iter().map(|(_, step)| step.stamp.id).collect();
        assert_eq!(
            ids,
            (FIRST_STATE_ID..FIRST_STATE_ID + ids.len() as u16).collect::<Vec<_>>(),
            "+1 per state"
        );
        assert!(
            steps
                .windows(2)
                .all(|pair| pair[1].0 - pair[0].0 <= IDLE_STATE_MS + TICK_MS)
        );
        let (anchor, first) = &steps[0];
        assert!(first.anchored, "the first state brings the clock");
        for (now, step) in &steps[1..] {
            assert_eq!(u64::from(step.stamp.offset_ms), now - anchor);
            assert!(!step.anchored);
            assert_eq!(step.position, None);
        }
        assert_eq!(
            scope_entry(first).map(|entry| entry.fragment),
            Some(sailor.spawn.fragment),
            "the first state starts the spawn"
        );
        let idle_at = steps[1..]
            .iter()
            .find(|(_, step)| scope_entry(step).is_some())
            .expect("the idle fragment starts");
        assert!(idle_at.0 - START >= SPAWN_MS);
        let entry = scope_entry(&idle_at.1).expect("entry");
        assert_eq!(entry.fragment, sailor.idle.fragment);
        assert_eq!(entry.start_id, idle_at.1.stamp.id);
    }

    #[test]
    fn a_moving_mob_sends_every_tick_with_its_position_and_walk() {
        let sailor = sailor();
        let mut timelines = MobTimelines::default();
        // Its alert plays first, while it stands.
        timelines.advance(MOB, START, &pose(1.0, true), Some(&sailor));
        let start = START + ALERT_MS;
        timelines.advance(MOB, start, &pose(1.0, true), Some(&sailor));
        let first = timelines
            .advance(MOB, start + TICK_MS, &pose(1.2, true), Some(&sailor))
            .step
            .expect("moved");
        assert_eq!(
            scope_entry(&first).map(|entry| entry.fragment),
            Some(sailor.chase.fragment)
        );
        let second = timelines
            .advance(MOB, start + 2 * TICK_MS, &pose(1.4, true), Some(&sailor))
            .step
            .expect("moved");
        assert_eq!(second.stamp.id, first.stamp.id + 1);
        assert!(matches!(
            second.scope,
            Some(StepScope::Ticks { ticks: 0 | 1, .. })
        ));
        assert!(
            matches!(second.position, Some(StepPlace::Offset(_))),
            "a short walk is an offset from the spawn's pos10: {:?}",
            second.position
        );
    }

    #[test]
    fn a_walking_mob_sends_offsets_and_a_new_anchor_only_out_of_reach() {
        /// Half a metre per state: retail's 2.1 m reach holds four of them.
        const STEP_M: f32 = 0.5;
        const STATES: u64 = 10;
        let mut timelines = MobTimelines::default();
        timelines.advance(MOB, START, &pose(0.0, true), None);
        let places: Vec<StepPlace> = (1..=STATES)
            .filter_map(|n| {
                #[allow(clippy::cast_precision_loss, reason = "a few states")]
                let x = n as f32 * STEP_M;
                timelines
                    .advance(MOB, START + n * TICK_MS, &pose(x, true), None)
                    .step?
                    .position
            })
            .collect();
        let anchors = places
            .iter()
            .filter(|place| matches!(place, StepPlace::Anchor(_)))
            .count();
        assert_eq!(places.len(), 10);
        assert_eq!(
            anchors, 2,
            "a new pos10 each time the mob is 2.1 m past the last: {places:?}"
        );
    }

    #[test]
    fn a_mob_faces_where_it_walks_then_turns_to_its_target_step_by_step() {
        let sailor = sailor();
        let mut timelines = MobTimelines::default();
        timelines.advance(MOB, START, &pose(1.0, true), Some(&sailor));
        let east = timelines
            .advance(MOB, START + TICK_MS, &pose(1.2, true), Some(&sailor))
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
                ..pose(1.2, true)
            };
            let frame = timelines.advance(MOB, now, &standing, Some(&sailor));
            if let Some(next) = frame.step.and_then(|step| step.facing) {
                assert!((next - facing).abs() <= TURN_PER_STATE_RAD + f32::EPSILON);
                facing = next;
            }
        }
        assert!(facing.abs() < TURN_EPSILON_RAD, "it faces north: {facing}");
        let frame = timelines.advance(MOB, now + TICK_MS, &pose(1.2, true), None);
        assert_eq!(frame.facing, Some(facing), "the card faces the same way");
    }

    #[test]
    fn a_reaction_is_kept_for_the_swings_that_started_before_it() {
        let sailor = sailor();
        let mut timelines = MobTimelines::default();
        let fighting = pose(1.0, true);
        timelines.advance(MOB, START, &fighting, Some(&sailor));
        let swing_started = START + SPAWN_MS;
        assert!(!timelines.reeled_since(MOB, swing_started));
        timelines.hurt(MOB);
        let reeled = swing_started + TICK_MS;
        timelines.advance(MOB, reeled, &fighting, Some(&sailor));
        assert!(timelines.reeled_since(MOB, swing_started));
        assert!(
            !timelines.reeled_since(MOB, reeled + 1),
            "a later swing is not dropped"
        );
    }

    #[test]
    fn hits_play_a_light_then_a_heavy_reaction_then_the_limit_holds() {
        let sailor = sailor();
        let mut timelines = MobTimelines::default();
        let fighting = pose(1.0, true);
        timelines.advance(MOB, START, &fighting, Some(&sailor));
        let fragment_of = |step: Option<MobStep>| match step.and_then(|step| step.scope) {
            Some(StepScope::Changed(animation)) => Some(animation.entry.fragment),
            _ => None,
        };
        let mut now = START + SPAWN_MS;
        timelines.hurt(MOB);
        let light = timelines.advance(MOB, now, &fighting, Some(&sailor)).step;
        assert_eq!(
            fragment_of(light),
            Some(sailor.reactions.expect("known").0.fragment)
        );
        now += TICK_MS;
        timelines.hurt(MOB);
        let heavy = timelines.advance(MOB, now, &fighting, Some(&sailor)).step;
        assert_eq!(
            fragment_of(heavy),
            Some(sailor.reactions.expect("known").1.fragment)
        );
        // While the limit runs, a hit does not start another reaction.
        now += CREATURE_REACTION_MS.1;
        timelines.advance(MOB, now, &fighting, Some(&sailor));
        timelines.hurt(MOB);
        now += TICK_MS;
        let limited = timelines.advance(MOB, now, &fighting, Some(&sailor)).step;
        assert_ne!(
            fragment_of(limited),
            Some(sailor.reactions.expect("known").0.fragment)
        );
        // A creature without reactions does not reel.
        let plain = without_reactions();
        let mut plains = MobTimelines::default();
        plains.advance(MOB, START, &fighting, Some(&plain));
        plains.hurt(MOB);
        let step = plains
            .advance(MOB, START + SPAWN_MS, &fighting, Some(&plain))
            .step;
        assert_eq!(fragment_of(step), Some(plain.combat_idle.fragment));
    }

    #[test]
    fn a_swing_plays_the_attack_and_a_creature_without_fragments_sends_no_scope() {
        let sailor = sailor();
        let mut timelines = MobTimelines::default();
        let fighting = pose(1.0, true);
        timelines.advance(MOB, START, &fighting, None);
        timelines.attacked(MOB);
        let step = timelines
            .advance(MOB, START + TICK_MS, &fighting, None)
            .step
            .expect("the attack starts");
        assert_eq!(step.scope, None);
        let mut sailors = MobTimelines::default();
        sailors.advance(MOB, START, &fighting, Some(&sailor));
        sailors.attacked(MOB);
        let step = sailors
            .advance(MOB, START + TICK_MS, &fighting, Some(&sailor))
            .step
            .expect("the attack starts");
        let Some(StepScope::Changed(animation)) = step.scope else {
            panic!("a new fragment");
        };
        assert_eq!(animation.entry.fragment, sailor.attack.fragment);
        assert_eq!(animation.tags, sailor.alert_tags);
    }

    #[test]
    fn the_anchor_moves_on_after_a_minute_and_a_dead_mob_plays_out_its_death() {
        let sailor = sailor();
        let mut timelines = MobTimelines::default();
        let idle = pose(1.0, false);
        timelines.advance(MOB, START, &idle, Some(&sailor));
        let later = START + ANCHOR_PERIOD_MS;
        let step = timelines
            .advance(MOB, later, &idle, Some(&sailor))
            .step
            .expect("due");
        assert!(step.anchored);
        assert_eq!(step.stamp.anchor_us, later * US_PER_MS);
        assert_eq!(step.stamp.offset_ms, 0);
        let dead = MobPose { dead: true, ..idle };
        let death = timelines
            .advance(MOB, later + TICK_MS, &dead, Some(&sailor))
            .step
            .expect("death");
        assert_eq!(
            scope_entry(&death).map(|entry| entry.fragment),
            Some(sailor.death.fragment)
        );
        let dying = timelines
            .advance(MOB, later + TICK_MS + DEAD_STATE_MS, &dead, Some(&sailor))
            .step
            .expect("due");
        assert!(
            matches!(dying.scope, Some(StepScope::Ticks { .. })),
            "the death fragment's ticks go on"
        );
        let death_ms = u64::from(SAILOR_DEATH_TICKS) * MS_PER_SECOND / TICKS_PER_SECOND;
        let after = timelines
            .advance(
                MOB,
                later + TICK_MS + death_ms + DEAD_STATE_MS,
                &dead,
                Some(&sailor),
            )
            .step
            .expect("due");
        assert_eq!(after.scope, None, "then only the clock");
        timelines.keep_only(&HashSet::new());
        let again = timelines.advance(MOB, later, &idle, None);
        assert_eq!(
            (again.stamp.id, again.fresh),
            (CARD_STATE_ID, true),
            "a forgotten mob starts over"
        );
    }

    /// A boar that runs its tree, playing `entry` with the tags `tags`.
    fn tree_pose(
        entry: Option<TreeEntry>,
        tags: [u8; FRAG_TAGS_LEN],
        facing: Option<f32>,
    ) -> MobPose {
        MobPose {
            tree: Some(TreePose {
                entry,
                tags,
                facing,
            }),
            ..pose(1.0, true)
        }
    }

    #[test]
    fn a_tree_mob_shows_its_trees_action_tags_and_facing() {
        // The boar's `Attack_Gore` (fragment 23, action 9) and its alerted tags `22`.
        const GORE: TreeEntry = TreeEntry {
            fragment: 23,
            action: 9,
            serial: 1,
        };
        const FACING: f32 = 1.0;
        let boar = creature(&BOAR);
        let alerted = tags(&[0x22]);
        let mut timelines = MobTimelines::default();
        let card = timelines.advance(
            MOB,
            START,
            &tree_pose(Some(GORE), alerted, None),
            Some(&boar),
        );
        assert!(card.fresh);
        let first = timelines
            .advance(
                MOB,
                START + FIRST_STATE_DELAY_MS,
                &tree_pose(Some(GORE), alerted, None),
                Some(&boar),
            )
            .step
            .expect("the first state");
        assert_eq!(
            scope_entry(&first).map(|entry| (entry.fragment, entry.action)),
            Some((34, 1)),
            "it spawns first"
        );
        let after_spawn = START + SPAWN_MS;
        let frame = timelines.advance(
            MOB,
            after_spawn,
            &tree_pose(Some(GORE), alerted, Some(FACING)),
            Some(&boar),
        );
        let step = frame.step.expect("a new state");
        let entry = scope_entry(&step).expect("the gore starts");
        assert_eq!((entry.fragment, entry.action), (23, 9));
        assert_eq!(
            entry.tail, DEFAULT_ENTRY_TAIL,
            "retail sent no gore of the boar"
        );
        assert_eq!(
            animation_of(&step).map(|animation| animation.tags),
            Some(alerted)
        );
        assert_eq!(step.facing, Some(FACING), "the tree's facing");
        assert!(!frame.alert_started, "a tree plays its own alert");
        let again = timelines.advance(
            MOB,
            after_spawn + TICK_MS,
            &tree_pose(Some(TreeEntry { serial: 2, ..GORE }), alerted, Some(FACING)),
            Some(&boar),
        );
        let entry = again
            .step
            .as_ref()
            .and_then(scope_entry)
            .expect("a new start is a new entry");
        assert_eq!(entry.start_id, again.stamp.id);
        let idle = timelines.advance(
            MOB,
            after_spawn + 2 * TICK_MS,
            &tree_pose(None, alerted, Some(FACING)),
            Some(&boar),
        );
        let entry = idle.step.as_ref().and_then(scope_entry).expect("idle");
        assert_eq!(
            (entry.fragment, entry.tail),
            (boar.idle.fragment, boar.idle.tail)
        );
    }

    #[test]
    fn a_tree_mobs_reaction_and_death_come_from_the_timeline() {
        let risen = creature(&FTUE_RISEN);
        let calm = risen.calm_tags;
        let mut timelines = MobTimelines::default();
        timelines.advance(MOB, START, &tree_pose(None, calm, None), Some(&risen));
        let now = START + SPAWN_MS;
        timelines.hurt(MOB);
        let frame = timelines.advance(MOB, now, &tree_pose(None, calm, None), Some(&risen));
        let entry = frame
            .step
            .as_ref()
            .and_then(scope_entry)
            .expect("a reaction");
        assert_eq!(
            entry.fragment,
            risen.reactions.expect("reactions").0.fragment
        );
        assert_eq!(
            timelines.take_reaction(MOB),
            Some(CREATURE_REACTION_MS.0),
            "its tree learns of the light reaction"
        );
        assert_eq!(timelines.take_reaction(MOB), None, "once");
        let dead = MobPose {
            dead: true,
            ..tree_pose(None, calm, None)
        };
        let frame = timelines.advance(MOB, now + TICK_MS, &dead, Some(&risen));
        let entry = frame
            .step
            .as_ref()
            .and_then(scope_entry)
            .expect("its death");
        assert_eq!(entry.fragment, risen.death.fragment);
    }
}
