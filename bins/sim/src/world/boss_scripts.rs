// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bosses of three story trials whose behaviour trees our Server.pak does not hold
//! (`grunt/abominable_liang_base`, `isabella/isabella_solo_msq2.3`, the Adiana archer's): the
//! sim plays what the retail captures show of them (research 2026-11-23 R1, `FINDINGS.md`), and
//! sends the encounter events their stages wait for. Where no capture shows the rule, the rule
//! is ours and marked Inferred.
//!
//! - Prince Liang (`QuestLiang`, `Abominable_Liang_MSQ`): player hits do almost nothing. A player
//!   takes a talisman pile by walking onto it and holds `AI_AbominableLiang_ProtectiveTalisman`;
//!   the next hit on Liang with it adds a `AI_AbominableLiang_TalismanBarrier` stack. At 4, 6 and
//!   8 stacks Liang reacts, loses 34 % of his max health (the last time: he dies) and the stacks
//!   clear.
//! - Isabella (`QuestIsabella`, `Isabella_MSQ2`): her health stops at 80, 65, 55, 40 and 25 %
//!   (the `AI_Isabella_MinHealth_*` effects); at each floor she turns invulnerable and plays its
//!   turn: she consumes her cultists, transforms, or calls the water.
//! - Adiana (`QuestAdiana`, `FL_AI_Adiana_Bow`): shadow archers on a timer, a second phase below
//!   half health.

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_bus::Point;
use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::Position;

use super::{Body, Player, SimWorld, Vitals};
use crate::mobs::{Invulnerable, MoveFragment};

/// The owner name of the health floors these fights set ([`super::health_floors`]).
const FLOOR_OWNER: &str = "boss_scripts";
/// How the bosses show their invulnerable turns: an intro and Isabella's transformation show
/// "Immune" (Observed: `99g_msq_sm_destroyenemy` 119.447 s and 276.807 s), her other turns and
/// Adiana's intro do not (155.045 s; `09a_m15_fightartemis` 264.530 s).
const IMMUNE: Invulnerable = Invulnerable {
    display_immune: true,
};
const UNTOUCHABLE: Invulnerable = Invulnerable {
    display_immune: false,
};
/// One mob of a spawn.
const ONE: u32 = 1;
/// The potency of the effects the bosses give.
const EFFECT_POTENCY: f64 = 1.0;

/// Liang's vitals id.
const LIANG_VITALS: &str = "abominable_liang_msq";
/// Isabella's vitals id.
const ISABELLA_VITALS: &str = "isabella_msq2";
/// Adiana's vitals id.
const ADIANA_VITALS: &str = "fl_ai_adiana_bow";

/// The talisman pile, a pickup a player takes by walking onto it: the plan of a kill step whose
/// encounter spawns it names its places ([`touch_pickup`]).
pub(crate) const TALISMAN_PILE_SLICE: &str =
    "slices/gatherables/msq/msq_abominableliang_talismanpile.dynamicslice";
/// The pickups taken by touch, by slice.
const TOUCH_PICKUPS: [&str; 1] = [TALISMAN_PILE_SLICE];
/// A player this near a talisman pile takes it, m. Inferred: the retail player stood 0.5-0.7 m
/// from each pile it took (`99f_msq_er_fightliang` 163.478, 166.579, 170.530 s); no client call
/// takes it.
const PILE_REACH_M: f32 = 1.0;
/// A taken talisman pile stands again after this time. Observed: i9 163.212 -> 180.879 s, i12
/// 166.345 -> 183.979 s, i11 170.143 -> 186.047 s (16-18 s).
const PILE_BACK_MS: u64 = 17_000;
/// The effect a taken talisman pile gives.
const TALISMAN_BUFF: &str = "AI_AbominableLiang_ProtectiveTalisman";
/// The stack a talisman hit adds to Liang.
const TALISMAN_BARRIER: &str = "AI_AbominableLiang_TalismanBarrier";
/// The effect whose `HealthModifierPercent` (-0.34) Liang loses at each stack target.
const LIANG_DROP: &str = "AI_AbominableLiang_33percentDamage";
/// The share of a player hit that Liang takes. Observed: each retail hit did about 6 points of
/// 9,888,912 with the DamageDealt scale 0.0015925765 (`99f_msq_er_fightliang` 160.335-300.710 s).
const LIANG_HIT_SCALE: f64 = 0.001_6;
/// The stacks at which Liang reacts, one per cycle. Observed: 4 (184.351 s), 6 (233.277 s), 8
/// (299.939 s); the game text "Apply 6 more talismans", "Apply 8 more talismans".
const LIANG_STACK_TARGETS: [u32; 3] = [4, 6, 8];
/// The event each cycle's end sends: the first drop sends `Phase2_started` too (Observed:
/// `Playthrough_Part_07` 998.995 s), the second `Phase3_started` (235.473 s), the death
/// `Phase4_started` (302.205 s).
const LIANG_CYCLE_EVENTS: [&str; 3] = ["Phase2_started", "Phase3_started", "Phase4_started"];
/// Liang's first event after his spawn. Observed: spawn + 3.7 s (132.697 -> 136.382 s,
/// `MSQ19-lv65` 1574.132 -> 1577.849 s).
const LIANG_PHASE1: &str = "Phase1_started";
const LIANG_PHASE1_AFTER_MS: u64 = 3_700;
/// The event a player's entry into the boss area sends (Observed: the player crossed the
/// `AreaBoss` box at 149.878 s and 1581.414 s).
const LIANG_PHASE2: &str = "Phase2_started";
const LIANG_AREA: &str = "AreaBoss";
/// From the reaction to the health drop. Observed: `ReactionTalisman` 184.351 -> 186.979 s,
/// stagger 232.843 -> 235.473 s.
const LIANG_REACTION_MS: u64 = 2_600;
/// Liang's reaction to a stack target (`r_R3_F`, action `ReactionTalisman`, 184.351 s).
const LIANG_REACTION_FRAGMENT: &str = "r_R3_F";
const LIANG_REACTION_ACTION: &str = "ReactionTalisman";

/// Isabella's intro: `Intro_Complete` this long after the stage that waits for it starts.
/// Observed: `Intro` 119.447 -> 134.214 s, `Soul-Trial---Tempest-Mind` 22.104 -> 36.919 s.
const ISABELLA_INTRO_STAGE: &str = "Phase 1b - Intro Complete";
const ISABELLA_INTRO_EVENT: &str = "Intro_Complete";
const ISABELLA_INTRO_MS: u64 = 14_800;
/// Her transformation, after the 40 % floor. Observed: 276.807 -> 281.306 s, 174.380 ->
/// 178.882 s.
const TRANSFORM_EVENT: &str = "Transform_Winged";
const TRANSFORM_AFTER_MS: u64 = 4_500;
/// The players she roots and consumes during it. Observed: `AI_Isabella_Solo_Root` 283.409 s,
/// `AI_Isabella_Solo_Player_Consume` 285.640 s (Transform_Winged + 2.1 s and + 4.3 s).
const TRANSFORM_PLAYER_EFFECTS: [&str; 2] =
    ["AI_Isabella_Solo_Root", "AI_Isabella_Solo_Player_Consume"];
const TRANSFORM_PLAYER_AFTER_MS: u64 = 2_100;
/// The radius around her in which the transformation takes the players, m. Inferred: the
/// arena (the `AreaBoss` box is 95 by 120 m).
const ARENA_RADIUS_M: f32 = 60.0;
/// `Winged_Ready` this long after the transformation stage starts. Observed: 281.306 ->
/// 338.622 s and 178.882 -> 236.111 s (57.3 s).
const WINGED_STAGE: &str = "Wave 2 - Transformation";
const WINGED_EVENT: &str = "Winged_Ready";
const WINGED_AFTER_MS: u64 = 57_300;
/// `Water_1` this long after its checkpoint stage starts. Observed: 338.622 -> 340.687 s,
/// 236.111 -> 238.179 s.
const WATER_1_STAGE: &str = "First_Checkpoint";
const WATER_1_EVENT: &str = "Water_1";
const WATER_1_AFTER_MS: u64 = 2_070;
/// She can be hurt again this long after `Water_1`. Observed: 340.687 -> 344.656 s.
const WINGED_VULNERABLE_AFTER_MS: u64 = 4_000;
/// `Water_2` this long after the 25 % floor. Observed: 269.893 -> 272.462 s (Tempest Mind);
/// Inferred: the shorter of the two runs (the quest run waited 7.3 s).
const WATER_2_EVENT: &str = "Water_2";
const WATER_2_AFTER_MS: u64 = 2_600;
/// Her water summon, invulnerable: from `Water_2` + 3.1 s for 4.5 s. Observed: 385.920 ->
/// 389.018 -> 393.518 s, 272.462 -> 275.561 -> 280.059 s.
const SUMMON_AFTER_MS: u64 = 3_100;
const SUMMON_MS: u64 = 4_500;
/// The event her death sends.
const DEFEAT_EVENT: &str = "Defeat";
/// The buff she gains for each cultist she consumes (Observed 173.878 s).
const CONSUME_BUFF: &str = "AI_Isabella_Solo_Consume_Buff";
/// Her cultists: vitals id and place. Observed: six `damned_cultist_isabella_msq2`, level 60,
/// at these places at the encounter's start (`99g_msq_sm_destroyenemy` 101.6-101.7 s); their
/// point spawners are not decoded, so the fight spawns them (Inferred).
const CULTISTS: [(&str, [f32; 3]); 6] = [
    (
        "Damned_Cultist_Isabella_MSQ2_Named_00",
        [1182.5, 1073.6, 126.5],
    ),
    (
        "Damned_Cultist_Isabella_MSQ2_Named_01",
        [1183.4, 1090.6, 126.5],
    ),
    (
        "Damned_Cultist_Isabella_MSQ2_Named_02",
        [1199.0, 1072.9, 126.2],
    ),
    (
        "Damned_Cultist_Isabella_MSQ2_Named_03",
        [1166.5, 1074.5, 126.4],
    ),
    (
        "Damned_Cultist_Isabella_MSQ2_Named_04",
        [1167.4, 1091.4, 127.0],
    ),
    (
        "Damned_Cultist_Isabella_MSQ2_Named_05",
        [1200.1, 1089.7, 126.6],
    ),
];
const CULTIST_LEVEL: u32 = 60;

/// Adiana's intro: `Intro_Complete` this long after its stage starts. Observed: 273.190 ->
/// 278.875 s, 18.270 -> 23.950 s.
const ADIANA_INTRO_STAGE: &str = "01 - Intro";
const ADIANA_INTRO_EVENT: &str = "Intro_Complete";
const ADIANA_INTRO_MS: u64 = 5_650;
/// Her shadow archers: two per call, at her sides.
const SHADOW_VITALS: &str = "FL_AI_Adiana_Shadow";
const SHADOW_LEVEL: u32 = 65;
const SHADOWS_PER_CALL: usize = 2;
/// How far to each side of her they stand, m. Observed: 725.6 810.0 and 725.8 820.8 with her at
/// 725.7 815.4 (`09a_m15_fightartemis` 292.974 s).
const SHADOW_SIDE_M: f32 = 5.4;
/// Her first call after her intro and the calls after it. Observed: 23.950 -> 36.785 s, then
/// 55.447 and 75.382 s (`Soul-Trial---The-End-of-new-Eden`); Inferred: the mean gap.
const SHADOW_FIRST_MS: u64 = 13_100;
const SHADOW_EVERY_MS: u64 = 19_300;
/// Her second phase starts below this share of her max health (Observed: `Intro_Phase2` at
/// 48.7 % and after 50.5 %; Inferred: half).
const ADIANA_PHASE2_SHARE: f64 = 0.5;
const ADIANA_PHASE2_FRAGMENT: &str = "Intro_Phase2";
/// Its first call and the calls after it. Observed: 84.813 -> 87.481 -> 120.729 s, 307.075 ->
/// 309.707 s.
const PHASE2_SHADOW_FIRST_MS: u64 = 2_650;
const PHASE2_SHADOW_EVERY_MS: u64 = 33_200;
/// The length of a reaction or transition the bosses play (Inferred, the Liang reaction).
const BOSS_MOVE_MS: u64 = LIANG_REACTION_MS;

/// The places of the pickups taken by touch that `targets` spawn (an encounter's targets and
/// their spawns' places): what a bot walks onto.
pub(crate) fn touch_pickup(slice: &str) -> bool {
    TOUCH_PICKUPS.contains(&slice)
}

/// What a boss does in a tick or on a hit.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    /// Send this encounter event.
    Event(&'static str),
    /// Its talisman barrier holds this many stacks (0: it clears).
    Stacks(u32),
    /// It plays this fragment and action.
    Play(&'static str),
    /// It loses this share of its max health, by the hit of player `attacker`; `kill`: all of it.
    Drop { attacker: u64, kill: bool },
    /// Its health floor effect changes: the old one ends, the new one starts.
    Floor {
        old: Option<&'static str>,
        new: Option<&'static str>,
    },
    /// It consumes cultist `index` of [`CULTISTS`] (gone, a buff more).
    Consume(usize),
    /// It roots and consumes the players near it.
    TakePlayers,
    /// It calls its shadow archers.
    Shadows,
}

/// Prince Liang's fight.
#[derive(Debug, Clone, PartialEq)]
struct Liang {
    seen_ms: u64,
    phase1: bool,
    phase2: bool,
    stacks: u32,
    /// The cycle: an index into [`LIANG_STACK_TARGETS`].
    cycle: usize,
    /// The drop due after a reaction: when, and the player whose hit reached the target.
    drop: Option<(u64, u64)>,
}

impl Liang {
    const fn new(now_ms: u64) -> Self {
        Self {
            seen_ms: now_ms,
            phase1: false,
            phase2: false,
            stacks: 0,
            cycle: 0,
            drop: None,
        }
    }

    /// A tick at `now_ms`; `in_area`: a living player is in the boss area.
    fn tick(&mut self, now_ms: u64, in_area: bool) -> Vec<Step> {
        let mut steps = Vec::new();
        if !self.phase1 && now_ms >= self.seen_ms.saturating_add(LIANG_PHASE1_AFTER_MS) {
            self.phase1 = true;
            steps.push(Step::Event(LIANG_PHASE1));
        }
        if self.phase1 && !self.phase2 && in_area {
            self.phase2 = true;
            steps.push(Step::Event(LIANG_PHASE2));
        }
        if let Some((due_ms, attacker)) = self.drop
            && now_ms >= due_ms
        {
            self.drop = None;
            let last = self.cycle + 1 == LIANG_STACK_TARGETS.len();
            steps.push(Step::Drop {
                attacker,
                kill: last,
            });
            steps.push(Step::Event(LIANG_CYCLE_EVENTS[self.cycle]));
            self.phase2 = true;
            self.stacks = 0;
            steps.push(Step::Stacks(0));
            self.cycle = (self.cycle + 1).min(LIANG_STACK_TARGETS.len() - 1);
        }
        steps
    }

    /// Player `attacker` hit him with a talisman at `now_ms`. Inferred: a hit counts whatever
    /// its kind (the game text asks for skills or heavy attacks; the captures do not split the
    /// hits), and a hit during a reaction counts none.
    fn talisman_hit(&mut self, attacker: u64, now_ms: u64) -> Vec<Step> {
        if self.drop.is_some() {
            return Vec::new();
        }
        self.stacks += 1;
        let mut steps = vec![Step::Stacks(self.stacks)];
        if self.stacks >= LIANG_STACK_TARGETS[self.cycle] {
            self.drop = Some((now_ms.saturating_add(LIANG_REACTION_MS), attacker));
            steps.push(Step::Play(LIANG_REACTION_ACTION));
        }
        steps
    }
}

/// One turn of Isabella at a health floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    /// She consumes one cultist after `after_ms`, during `window_ms`.
    Single { after_ms: u64, window_ms: u64 },
    /// She consumes every living cultist after `window_ms`.
    Multiple { window_ms: u64 },
    /// She transforms.
    Transform,
    /// She calls the water.
    Water,
}

/// Her floors, in order: the `AI_Isabella_MinHealth_*` effect (its `HealthMinPercent` is the
/// floor) and her turn there. Observed: 80 % 155.045 s (consume 161.710 s, the cultist gone
/// 175.2 s), 65 % 205.743 s (212.042 s), 55 % 230.441 s (`Consume_Minions_Multiple`, the last
/// cultists gone 249.96 s), 40 % 276.807 s, 25 % 378.638 s. Inferred: the 65 % window as the
/// 80 % one.
const FLOORS: [(&str, Turn); 5] = [
    (
        "AI_Isabella_MinHealth_80",
        Turn::Single {
            after_ms: 6_700,
            window_ms: 13_500,
        },
    ),
    (
        "AI_Isabella_MinHealth_65",
        Turn::Single {
            after_ms: 6_300,
            window_ms: 13_500,
        },
    ),
    (
        "AI_Isabella_MinHealth_55",
        Turn::Multiple { window_ms: 19_500 },
    ),
    ("AI_Isabella_MinHealth_40", Turn::Transform),
    ("AI_Isabella_MinHealth_25", Turn::Water),
];

/// What Isabella's tick sees.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct IsabellaView {
    /// A hit took her down to her floor.
    reached: bool,
    /// When her intro stage, the transformation stage and the first checkpoint stage started.
    intro_ms: Option<u64>,
    winged_ms: Option<u64>,
    water_1_ms: Option<u64>,
    /// The cultists that live, by index into [`CULTISTS`].
    living: [bool; CULTISTS.len()],
}

/// Isabella's fight.
#[derive(Debug, Clone, PartialEq)]
struct Isabella {
    intro_done: bool,
    /// The next floor: an index into [`FLOORS`].
    floor: usize,
    /// The turn running: when it started, and for a single consume the cultist it takes.
    turn: Option<(u64, Option<usize>)>,
    /// The events of the transformation and the water sent so far.
    sent: Vec<&'static str>,
    /// The players were taken in the transformation.
    took_players: bool,
    /// She can be hurt again from this time.
    vulnerable_ms: Option<u64>,
}

impl Isabella {
    const fn new() -> Self {
        Self {
            intro_done: false,
            floor: 0,
            turn: None,
            sent: Vec::new(),
            took_players: false,
            vulnerable_ms: None,
        }
    }

    /// How she shows that she takes no hits at `now_ms`, in her intro and in her turns; `None`
    /// while she takes them.
    fn invulnerable(&self, now_ms: u64) -> Option<Invulnerable> {
        let transforming =
            self.turn.is_some() && matches!(FLOORS.get(self.floor), Some((_, Turn::Transform)));
        if !self.intro_done || transforming {
            Some(IMMUNE)
        } else if self.turn.is_some() || self.vulnerable_ms.is_some_and(|from| now_ms < from) {
            Some(UNTOUCHABLE)
        } else {
            None
        }
    }

    fn send(&mut self, event: &'static str, steps: &mut Vec<Step>) {
        if !self.sent.contains(&event) {
            self.sent.push(event);
            steps.push(Step::Event(event));
        }
    }

    /// A tick at `now_ms`.
    fn tick(&mut self, now_ms: u64, view: &IsabellaView) -> Vec<Step> {
        let mut steps = Vec::new();
        if !self.intro_done {
            if view
                .intro_ms
                .is_some_and(|start| now_ms >= start.saturating_add(ISABELLA_INTRO_MS))
            {
                self.intro_done = true;
                steps.push(Step::Event(ISABELLA_INTRO_EVENT));
            }
            return steps;
        }
        let Some((_, turn)) = FLOORS.get(self.floor).copied() else {
            return steps;
        };
        let Some((started_ms, target)) = self.turn else {
            if view.reached {
                let target = matches!(turn, Turn::Single { .. })
                    .then(|| view.living.iter().position(|living| *living))
                    .flatten();
                self.turn = Some((now_ms, target));
            }
            return steps;
        };
        let since = now_ms.saturating_sub(started_ms);
        let done = match turn {
            Turn::Single {
                after_ms,
                window_ms,
            } => match target {
                Some(cultist) if !view.living[cultist] => since >= after_ms,
                Some(cultist) if since >= after_ms.saturating_add(window_ms) => {
                    steps.push(Step::Consume(cultist));
                    true
                }
                Some(_) => false,
                None => since >= after_ms,
            },
            Turn::Multiple { window_ms } => {
                if since >= window_ms {
                    for (cultist, living) in view.living.iter().enumerate() {
                        if *living {
                            steps.push(Step::Consume(cultist));
                        }
                    }
                    true
                } else {
                    false
                }
            }
            Turn::Transform => {
                if since >= TRANSFORM_AFTER_MS {
                    self.send(TRANSFORM_EVENT, &mut steps);
                }
                if !self.took_players
                    && since >= TRANSFORM_AFTER_MS.saturating_add(TRANSFORM_PLAYER_AFTER_MS)
                {
                    self.took_players = true;
                    steps.push(Step::TakePlayers);
                }
                if view
                    .winged_ms
                    .is_some_and(|start| now_ms >= start.saturating_add(WINGED_AFTER_MS))
                {
                    self.send(WINGED_EVENT, &mut steps);
                }
                let water = view
                    .water_1_ms
                    .map(|start| start.saturating_add(WATER_1_AFTER_MS))
                    .filter(|at| now_ms >= *at);
                if let Some(at) = water {
                    self.send(WATER_1_EVENT, &mut steps);
                    self.vulnerable_ms = Some(at.saturating_add(WINGED_VULNERABLE_AFTER_MS));
                }
                water.is_some()
            }
            Turn::Water => {
                if since >= WATER_2_AFTER_MS {
                    self.send(WATER_2_EVENT, &mut steps);
                    let summon = started_ms
                        .saturating_add(WATER_2_AFTER_MS)
                        .saturating_add(SUMMON_AFTER_MS);
                    self.vulnerable_ms = Some(summon.saturating_add(SUMMON_MS));
                    true
                } else {
                    false
                }
            }
        };
        if done {
            self.turn = None;
            self.floor += 1;
            steps.push(Step::Floor {
                old: FLOORS.get(self.floor - 1).map(|(effect, _)| *effect),
                new: FLOORS.get(self.floor).map(|(effect, _)| *effect),
            });
        }
        steps
    }
}

/// Adiana's fight.
#[derive(Debug, Clone, PartialEq)]
struct Adiana {
    intro_done: bool,
    /// When the fight after her intro started.
    fight_ms: Option<u64>,
    /// Her second phase started at this time.
    phase2_ms: Option<u64>,
    /// The shadow calls made so far in the phase.
    calls: u64,
}

impl Adiana {
    const fn new() -> Self {
        Self {
            intro_done: false,
            fight_ms: None,
            phase2_ms: None,
            calls: 0,
        }
    }

    /// How she shows that she takes no hits: in her intro.
    const fn invulnerable(&self) -> Option<Invulnerable> {
        if self.intro_done {
            None
        } else {
            Some(UNTOUCHABLE)
        }
    }

    /// A tick at `now_ms`, with her intro stage's start and her health's share of her max.
    fn tick(&mut self, now_ms: u64, intro_ms: Option<u64>, share: f64) -> Vec<Step> {
        let mut steps = Vec::new();
        if !self.intro_done {
            if let Some(start) = intro_ms
                && now_ms >= start.saturating_add(ADIANA_INTRO_MS)
            {
                self.intro_done = true;
                self.fight_ms = Some(now_ms);
                steps.push(Step::Event(ADIANA_INTRO_EVENT));
            }
            return steps;
        }
        if self.phase2_ms.is_none() && share < ADIANA_PHASE2_SHARE {
            self.phase2_ms = Some(now_ms);
            self.calls = 0;
            steps.push(Step::Play(ADIANA_PHASE2_FRAGMENT));
        }
        let (start, first, every) = match (self.phase2_ms, self.fight_ms) {
            (Some(start), _) => (start, PHASE2_SHADOW_FIRST_MS, PHASE2_SHADOW_EVERY_MS),
            (None, Some(start)) => (start, SHADOW_FIRST_MS, SHADOW_EVERY_MS),
            (None, None) => return steps,
        };
        let due = start
            .saturating_add(first)
            .saturating_add(every.saturating_mul(self.calls));
        if now_ms >= due {
            self.calls += 1;
            steps.push(Step::Shadows);
        }
        steps
    }
}

/// The fight of one boss.
#[derive(Debug, Clone, PartialEq)]
enum Fight {
    Liang(Liang),
    Isabella(Isabella),
    Adiana(Adiana),
}

/// The scripted bosses of the sim and their talisman piles.
#[derive(Debug, Default)]
pub(super) struct BossFights {
    /// The fights, by boss entity.
    fights: HashMap<u64, Fight>,
    /// The mobs each fight spawned (cultists, shadow archers), by boss: each by its index into
    /// [`CULTISTS`] for a cultist.
    adds: HashMap<u64, Vec<(usize, u64)>>,
    /// Taken talisman piles and when each stands again.
    taken: Vec<(u64, u64)>,
    /// The encounter events the bosses sent since the last take: (boss, event).
    events: Vec<(u64, String)>,
    /// A scripted health drop is being applied: its hit is not scaled.
    scripted: bool,
}

impl Fight {
    /// How the boss shows that it takes no hits at `now_ms`.
    fn invulnerable(&self, now_ms: u64) -> Option<Invulnerable> {
        match self {
            Self::Liang(_) => None,
            Self::Isabella(isabella) => isabella.invulnerable(now_ms),
            Self::Adiana(adiana) => adiana.invulnerable(),
        }
    }
}

impl BossFights {
    /// The encounter events the bosses sent since the last call: (boss, event id).
    pub(super) fn take_events(&mut self) -> Vec<(u64, String)> {
        std::mem::take(&mut self.events)
    }
}

/// The fight a mob of vitals id `vitals` (CRC-32 of the lowercase id) starts at `now_ms`.
fn fight_of(vitals: u32, now_ms: u64) -> Option<Fight> {
    if vitals == lowercase_crc32(LIANG_VITALS) {
        Some(Fight::Liang(Liang::new(now_ms)))
    } else if vitals == lowercase_crc32(ISABELLA_VITALS) {
        Some(Fight::Isabella(Isabella::new()))
    } else if vitals == lowercase_crc32(ADIANA_VITALS) {
        Some(Fight::Adiana(Adiana::new()))
    } else {
        None
    }
}

impl SimWorld {
    /// The scripted bosses at `now_ms`: each boss of the instance's script starts its fight, the
    /// fights run, and the talisman piles are taken and stand again.
    pub(super) fn tick_boss_fights(&mut self, now_ms: u64) {
        let bosses: Vec<u64> = self
            .dungeon_run
            .as_ref()
            .map(|run| run.mobs().collect())
            .unwrap_or_default();
        let living: Vec<u64> = bosses
            .into_iter()
            .filter(|boss| self.mobs.health(*boss).is_some_and(|health| health > 0))
            .collect();
        self.end_gone_fights(&living);
        for boss in living {
            if !self.boss_fights.fights.contains_key(&boss) {
                let Some(fight) = self
                    .mobs
                    .vitals_of(boss)
                    .and_then(|vitals| fight_of(vitals, now_ms))
                else {
                    continue;
                };
                tracing::info!(boss, ?fight, "boss fight starts");
                if matches!(fight, Fight::Isabella(_)) {
                    self.start_isabella(boss, now_ms);
                }
                self.boss_fights.fights.insert(boss, fight);
            }
            self.boss_tick(boss, now_ms);
        }
        self.tick_talisman_piles(now_ms);
    }

    /// The fights whose boss is not among `living` end; a dead boss sends its last event, and
    /// the mobs its fight spawned go.
    fn end_gone_fights(&mut self, living: &[u64]) {
        let gone: Vec<u64> = self
            .boss_fights
            .fights
            .keys()
            .filter(|boss| !living.contains(boss))
            .copied()
            .collect();
        for boss in gone {
            let fight = self.boss_fights.fights.remove(&boss);
            self.health_floors.clear(boss);
            self.mobs.set_invulnerable(boss, None);
            if matches!(fight, Some(Fight::Isabella(_))) && self.mobs.health(boss) == Some(0) {
                self.boss_fights
                    .events
                    .push((boss, DEFEAT_EVENT.to_owned()));
            }
            let adds: Vec<u64> = self
                .boss_fights
                .adds
                .remove(&boss)
                .unwrap_or_default()
                .into_iter()
                .map(|(_, mob)| mob)
                .collect();
            self.remove_gm(&adds);
            tracing::info!(boss, adds = adds.len(), "boss fight ends");
        }
    }

    /// Isabella's fight starts: her first health floor, and her cultists.
    fn start_isabella(&mut self, boss: u64, now_ms: u64) {
        self.set_floor(boss, Some(FLOORS[0].0));
        for (index, (vitals, at)) in CULTISTS.iter().enumerate() {
            let [x, y, z] = *at;
            match self.spawn_gm_of_category(
                vitals,
                None,
                CULTIST_LEVEL,
                Point { x, y, z },
                ONE,
                None,
            ) {
                Ok(spawned) => {
                    for mob in spawned {
                        self.boss_fights
                            .adds
                            .entry(boss)
                            .or_default()
                            .push((index, mob));
                    }
                }
                Err(reason) => tracing::warn!(vitals, %reason, "cultist spawn"),
            }
        }
        tracing::debug!(boss, now_ms, "isabella cultists");
    }

    /// One tick of boss `boss`'s fight at `now_ms`.
    fn boss_tick(&mut self, boss: u64, now_ms: u64) {
        let share = self.health_share(boss);
        let started = |world: &Self, stage: &str| {
            world
                .dungeon_run
                .as_ref()
                .and_then(|run| run.stage_started_ms(boss, stage))
        };
        let steps = match self.boss_fights.fights.get(&boss).cloned() {
            Some(Fight::Liang(mut liang)) => {
                let in_area = self.player_in_area(LIANG_AREA);
                let steps = liang.tick(now_ms, in_area);
                self.boss_fights.fights.insert(boss, Fight::Liang(liang));
                steps
            }
            Some(Fight::Isabella(mut isabella)) => {
                let reached = self
                    .health_floors
                    .take_reached(FLOOR_OWNER)
                    .iter()
                    .any(|reached| reached.mob == boss);
                let view = IsabellaView {
                    reached,
                    intro_ms: started(self, ISABELLA_INTRO_STAGE),
                    winged_ms: started(self, WINGED_STAGE),
                    water_1_ms: started(self, WATER_1_STAGE),
                    living: self.living_cultists(boss),
                };
                let steps = isabella.tick(now_ms, &view);
                self.boss_fights
                    .fights
                    .insert(boss, Fight::Isabella(isabella));
                steps
            }
            Some(Fight::Adiana(mut adiana)) => {
                let intro = started(self, ADIANA_INTRO_STAGE);
                let steps = adiana.tick(now_ms, intro, share);
                self.boss_fights.fights.insert(boss, Fight::Adiana(adiana));
                steps
            }
            None => Vec::new(),
        };
        self.boss_steps(boss, steps, now_ms);
        let invulnerable = self
            .boss_fights
            .fights
            .get(&boss)
            .and_then(|fight| fight.invulnerable(now_ms));
        if self.mobs.invulnerable(boss) != invulnerable {
            self.mobs.set_invulnerable(boss, invulnerable);
        }
    }

    /// Boss `boss`'s health floor becomes that of effect `effect` (its `HealthMinPercent`), shown
    /// as that effect; `None`: no floor.
    fn set_floor(&mut self, boss: u64, effect: Option<&str>) {
        let share = effect.and_then(|effect| {
            self.content
                .status_effects
                .by_id(effect)
                .and_then(|rule| rule.health_min_share)
        });
        match (effect, share) {
            (Some(effect), Some(share)) => {
                self.health_floors.set(boss, share, FLOOR_OWNER);
                self.add_boss_effect(boss, effect);
            }
            (Some(effect), None) => {
                tracing::warn!(boss, effect, "health floor effect without a floor");
                self.health_floors.clear(boss);
            }
            (None, _) => self.health_floors.clear(boss),
        }
    }

    /// Boss `boss` does `steps` at `now_ms`.
    fn boss_steps(&mut self, boss: u64, steps: Vec<Step>, now_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let rules = &content.status_effects;
        let server_ms = self.server_ms;
        for step in steps {
            tracing::info!(boss, ?step, "boss step");
            match step {
                Step::Event(event) => self.boss_fights.events.push((boss, event.to_owned())),
                Step::Stacks(stacks) => {
                    let held = self.mob_effects.entry(boss).or_default();
                    let barrier = lowercase_crc32(TALISMAN_BARRIER);
                    if stacks == 0 {
                        held.remove(barrier);
                    } else {
                        held.add_by_id(rules, TALISMAN_BARRIER, EFFECT_POTENCY, boss, server_ms);
                    }
                }
                Step::Play(action) => self.play_boss_move(boss, action),
                Step::Drop { attacker, kill } => {
                    let Some(max) = self.mobs.get(boss).map(|mob| mob.health.max()) else {
                        continue;
                    };
                    let share = rules
                        .by_id(LIANG_DROP)
                        .map_or(0.0, |rule| -rule.health_share);
                    let amount = if kill {
                        u32::MAX
                    } else {
                        // A share of a health of millions.
                        #[allow(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "a share of the max health"
                        )]
                        let amount = (f64::from(max) * share).round() as u32;
                        amount
                    };
                    self.boss_fights.scripted = true;
                    self.apply_hit(boss, attacker, amount, 0, now_ms);
                    self.boss_fights.scripted = false;
                }
                Step::Floor { old, new } => {
                    if let Some(old) = old {
                        self.mob_effects
                            .entry(boss)
                            .or_default()
                            .remove(lowercase_crc32(old));
                    }
                    self.set_floor(boss, new);
                }
                Step::Consume(cultist) => {
                    let taken: Vec<u64> = self
                        .boss_fights
                        .adds
                        .get(&boss)
                        .into_iter()
                        .flatten()
                        .filter(|(index, _)| *index == cultist)
                        .map(|(_, mob)| *mob)
                        .collect();
                    self.remove_gm(&taken);
                    if let Some(adds) = self.boss_fights.adds.get_mut(&boss) {
                        adds.retain(|(index, _)| *index != cultist);
                    }
                    self.add_boss_effect(boss, CONSUME_BUFF);
                }
                Step::TakePlayers => {
                    let Some(at) = self.mobs.get(boss).map(|mob| mob.position) else {
                        continue;
                    };
                    for player in self.players_within(at, ARENA_RADIUS_M) {
                        let Some(mut held) = self.world.get_mut::<Player>(player) else {
                            continue;
                        };
                        for effect in TRANSFORM_PLAYER_EFFECTS {
                            held.status_effects.add_by_id(
                                rules,
                                effect,
                                EFFECT_POTENCY,
                                boss,
                                server_ms,
                            );
                        }
                    }
                }
                Step::Shadows => self.call_shadows(boss, now_ms),
            }
        }
    }

    /// Boss `boss` holds effect `id`.
    fn add_boss_effect(&mut self, boss: u64, id: &str) {
        let content = std::sync::Arc::clone(&self.content);
        let server_ms = self.server_ms;
        let added = self.mob_effects.entry(boss).or_default().add_by_id(
            &content.status_effects,
            id,
            EFFECT_POTENCY,
            boss,
            server_ms,
        );
        if !added {
            tracing::warn!(boss, effect = id, "boss effect not added");
        }
    }

    /// Adiana calls her shadow archers at her sides; each takes the nearest player.
    fn call_shadows(&mut self, boss: u64, now_ms: u64) {
        let Some(at) = self.mobs.get(boss).map(|mob| mob.position) else {
            return;
        };
        let target = self.nearest_living_player(at);
        for side in 0..SHADOWS_PER_CALL {
            let offset = if side == 0 {
                -SHADOW_SIDE_M
            } else {
                SHADOW_SIDE_M
            };
            let place = Point {
                x: at.x,
                y: at.y + offset,
                z: at.z,
            };
            match self.spawn_gm_of_category(SHADOW_VITALS, None, SHADOW_LEVEL, place, ONE, None) {
                Ok(spawned) => {
                    for mob in spawned {
                        if let Some(player) = target {
                            self.mobs.provoke(mob, player, now_ms);
                        }
                        self.boss_fights
                            .adds
                            .entry(boss)
                            .or_default()
                            .push((side, mob));
                    }
                }
                Err(reason) => tracing::warn!(%reason, "shadow archer spawn"),
            }
        }
    }

    /// The number of the living player nearest to `at`.
    fn nearest_living_player(&mut self, at: Position) -> Option<u64> {
        let mut query = self.world.query::<(&Player, &Body, &Vitals)>();
        query
            .iter(&self.world)
            .filter(|(_, _, vitals)| !vitals.health.is_dead())
            .map(|(player, body, _)| (player.number, body.motion.position().distance(at)))
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(number, _)| number)
    }

    /// Boss `boss` plays its move `action` (a fragment and action of that name, or Liang's
    /// talisman reaction), when its creature has it.
    fn play_boss_move(&mut self, boss: u64, action: &str) {
        let content = std::sync::Arc::clone(&self.content);
        let fragment_name = if action == LIANG_REACTION_ACTION {
            LIANG_REACTION_FRAGMENT
        } else {
            action
        };
        let creature = self
            .mobs
            .vitals_of(boss)
            .and_then(|vitals| content.mob_looks.look(vitals, self.mobs.asset(boss)))
            .and_then(|look| content.creature_animations.get(&look.asset.id));
        let Some((fragment, action)) = creature.and_then(|creature| {
            Some((creature.fragment(fragment_name)?, creature.action(action)?))
        }) else {
            tracing::debug!(boss, action, "boss move without animation");
            return;
        };
        self.mob_timelines.play(
            boss,
            MoveFragment {
                fragment,
                action,
                tail: 0,
            },
            BOSS_MOVE_MS,
        );
    }

    /// The health of mob `mob` as a share of its max (1 for a mob not here).
    fn health_share(&self, mob: u64) -> f64 {
        self.mobs
            .get(mob)
            .filter(|mob| mob.health.max() > 0)
            .map_or(1.0, |mob| {
                f64::from(mob.health.current()) / f64::from(mob.health.max())
            })
    }

    /// Which of Isabella `boss`'s cultists live.
    fn living_cultists(&self, boss: u64) -> [bool; CULTISTS.len()] {
        let mut living = [false; CULTISTS.len()];
        for (index, mob) in self.boss_fights.adds.get(&boss).into_iter().flatten() {
            if self.mobs.health(*mob).is_some_and(|health| health > 0)
                && let Some(slot) = living.get_mut(*index)
            {
                *slot = true;
            }
        }
        living
    }

    /// Whether a living player stands in trigger area `name` of the instance's script.
    fn player_in_area(&mut self, name: &str) -> bool {
        let Some(area) = self
            .dungeon_run
            .as_ref()
            .and_then(|run| run.script().trigger_area(name))
            .cloned()
        else {
            return false;
        };
        let mut query = self.world.query::<(&Player, &Body, &Vitals)>();
        query.iter(&self.world).any(|(_, body, vitals)| {
            let at = body.motion.position();
            !vitals.health.is_dead() && area.contains([at.x, at.y, at.z])
        })
    }

    /// The talisman piles at `now_ms`: a living player that walks onto a standing one takes it
    /// and holds the talisman; a taken one stands again [`PILE_BACK_MS`] later.
    fn tick_talisman_piles(&mut self, now_ms: u64) {
        let (back, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.boss_fights.taken)
            .into_iter()
            .partition(|(_, back_ms)| now_ms >= *back_ms);
        self.boss_fights.taken = waiting;
        for (pile, _) in back {
            self.encounter_objects.replenish(pile);
        }
        let players: Vec<(Entity, Position)> = {
            let mut query = self.world.query::<(Entity, &Player, &Body, &Vitals)>();
            query
                .iter(&self.world)
                .filter(|(_, _, _, vitals)| !vitals.health.is_dead())
                .map(|(entity, _, body, _)| (entity, body.motion.position()))
                .collect()
        };
        let content = std::sync::Arc::clone(&self.content);
        let server_ms = self.server_ms;
        for (player, at) in players {
            let piles: Vec<u64> = TOUCH_PICKUPS
                .iter()
                .flat_map(|slice| self.encounter_objects.fresh_near(slice, at, PILE_REACH_M))
                .collect();
            let Some(pile) = piles.first().copied() else {
                continue;
            };
            if !self.encounter_objects.deplete(pile) {
                continue;
            }
            self.boss_fights
                .taken
                .push((pile, now_ms.saturating_add(PILE_BACK_MS)));
            if let Some(mut held) = self.world.get_mut::<Player>(player) {
                held.status_effects.add_by_id(
                    &content.status_effects,
                    TALISMAN_BUFF,
                    EFFECT_POTENCY,
                    pile,
                    server_ms,
                );
                tracing::info!(player = held.number, pile, "talisman taken");
            }
        }
    }

    /// The share of a hit of `amount` by `attacker` that mob `target` takes at `now_ms` by a
    /// scripted boss's rules: Liang takes a sliver of it, and a talisman on the attacker adds a
    /// stack. A scripted drop is taken whole.
    pub(super) fn boss_hit_amount(
        &mut self,
        target: u64,
        attacker: u64,
        amount: u32,
        now_ms: u64,
    ) -> u32 {
        if self.boss_fights.scripted {
            return amount;
        }
        self.scripted_boss_hit(target, attacker, amount, now_ms)
    }

    /// A hit of `amount` by `attacker` on a scripted boss `target` at `now_ms`.
    fn scripted_boss_hit(&mut self, target: u64, attacker: u64, amount: u32, now_ms: u64) -> u32 {
        match self.boss_fights.fights.get(&target).cloned() {
            Some(Fight::Liang(mut liang)) => {
                let content = std::sync::Arc::clone(&self.content);
                let buff = lowercase_crc32(TALISMAN_BUFF);
                let server_ms = self.server_ms;
                let buffed = self.player_by_number(attacker).is_some_and(|entity| {
                    self.world
                        .get_mut::<Player>(entity)
                        .is_some_and(|mut player| {
                            let holds = player.status_effects.holds(buff);
                            if holds {
                                player.status_effects.end(
                                    &content.status_effects,
                                    buff,
                                    target,
                                    server_ms,
                                );
                                player.status_effects.remove(buff);
                            }
                            holds
                        })
                });
                if buffed {
                    let steps = liang.talisman_hit(attacker, now_ms);
                    self.boss_fights.fights.insert(target, Fight::Liang(liang));
                    self.boss_steps(target, steps, now_ms);
                }
                // A hit of a few hundred points.
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a scaled hit"
                )]
                let scaled = (f64::from(amount) * LIANG_HIT_SCALE).round().max(1.0) as u32;
                scaled
            }
            _ => amount,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_000_000;
    const PLAYER: u64 = 7;

    fn events(steps: &[Step]) -> Vec<&'static str> {
        steps
            .iter()
            .filter_map(|step| match step {
                Step::Event(event) => Some(*event),
                _ => None,
            })
            .collect()
    }

    /// Retail `99f_msq_er_fightliang`: Phase1 3.7 s after the spawn, Phase2 at the boss area,
    /// a drop at 4, 6 and 8 talisman stacks, the last one his death with Phase4.
    #[test]
    fn liang_sends_his_phases_and_drops_at_four_six_and_eight_talismans() {
        let mut liang = Liang::new(T0);
        assert!(
            liang.tick(T0 + 1_000, true).is_empty(),
            "no Phase2 before Phase1"
        );
        assert_eq!(
            events(&liang.tick(T0 + LIANG_PHASE1_AFTER_MS, false)),
            [LIANG_PHASE1]
        );
        assert_eq!(events(&liang.tick(T0 + 5_000, true)), [LIANG_PHASE2]);
        assert!(liang.tick(T0 + 6_000, true).is_empty(), "once");
        let mut now = T0 + 10_000;
        for (cycle, target) in LIANG_STACK_TARGETS.into_iter().enumerate() {
            for stack in 1..target {
                let steps = liang.talisman_hit(PLAYER, now);
                assert_eq!(steps, [Step::Stacks(stack)]);
                now += 3_000;
            }
            let steps = liang.talisman_hit(PLAYER, now);
            assert_eq!(
                steps,
                [Step::Stacks(target), Step::Play(LIANG_REACTION_ACTION)]
            );
            assert!(
                liang.talisman_hit(PLAYER, now + 100).is_empty(),
                "none in the reaction"
            );
            assert!(liang.tick(now + LIANG_REACTION_MS - 1, true).is_empty());
            let steps = liang.tick(now + LIANG_REACTION_MS, true);
            let last = cycle + 1 == LIANG_STACK_TARGETS.len();
            assert_eq!(
                steps,
                [
                    Step::Drop {
                        attacker: PLAYER,
                        kill: last
                    },
                    Step::Event(LIANG_CYCLE_EVENTS[cycle]),
                    Step::Stacks(0),
                ]
            );
            now += LIANG_REACTION_MS + 1_000;
        }
    }

    /// `Playthrough_Part_07`: a player who never crossed into the boss area still moves the
    /// stages on: the first drop sends Phase2.
    #[test]
    fn liang_s_first_drop_sends_phase2_when_no_player_came_into_the_area() {
        let mut liang = Liang::new(T0);
        liang.tick(T0 + LIANG_PHASE1_AFTER_MS, false);
        let mut now = T0 + 5_000;
        for _ in 0..LIANG_STACK_TARGETS[0] {
            liang.talisman_hit(PLAYER, now);
            now += 1_000;
        }
        let steps = liang.tick(now + LIANG_REACTION_MS, false);
        assert_eq!(events(&steps), [LIANG_PHASE2]);
        assert!(events(&liang.tick(now + LIANG_REACTION_MS + 10, true)).is_empty());
    }

    fn view(reached: bool) -> IsabellaView {
        IsabellaView {
            reached,
            intro_ms: Some(T0),
            living: [true; CULTISTS.len()],
            ..IsabellaView::default()
        }
    }

    /// Retail `99g_msq_sm_destroyenemy`: invulnerable in her intro; at 80 % she consumes a
    /// cultist that lives at the window's end and the next floor starts; a cultist killed in
    /// the window ends the turn without a consume.
    #[test]
    fn isabella_consumes_a_living_cultist_at_her_floor_and_not_a_dead_one() {
        let mut isabella = Isabella::new();
        assert!(isabella.invulnerable(T0).is_some());
        assert!(isabella.tick(T0 + 1_000, &view(false)).is_empty());
        let intro = T0 + ISABELLA_INTRO_MS;
        assert_eq!(
            events(&isabella.tick(intro, &view(false))),
            [ISABELLA_INTRO_EVENT]
        );
        assert!(isabella.invulnerable(intro).is_none());
        let floor = intro + 10_000;
        assert!(isabella.tick(floor, &view(true)).is_empty());
        assert!(isabella.invulnerable(floor).is_some());
        let Turn::Single {
            after_ms,
            window_ms,
        } = FLOORS[0].1
        else {
            panic!("a single consume");
        };
        assert!(
            isabella
                .tick(floor + after_ms + window_ms - 1, &view(true))
                .is_empty()
        );
        let steps = isabella.tick(floor + after_ms + window_ms, &view(true));
        assert_eq!(
            steps,
            [
                Step::Consume(0),
                Step::Floor {
                    old: Some(FLOORS[0].0),
                    new: Some(FLOORS[1].0)
                }
            ]
        );
        assert!(
            isabella
                .invulnerable(floor + after_ms + window_ms)
                .is_none()
        );
        let second = floor + 60_000;
        isabella.tick(second, &view(true));
        let mut killed = view(true);
        killed.living[0] = false;
        killed.living[1] = false;
        let Turn::Single { after_ms, .. } = FLOORS[1].1 else {
            panic!("a single consume");
        };
        let steps = isabella.tick(second + after_ms, &killed);
        assert!(!steps.iter().any(|step| matches!(step, Step::Consume(_))));
        assert!(steps.iter().any(|step| matches!(step, Step::Floor { .. })));
    }

    /// Retail: at 40 % Transform_Winged 4.5 s later, the players taken 2.1 s after it,
    /// Winged_Ready 57.3 s after its stage, Water_1 2.07 s after its stage; at 25 % Water_2
    /// 2.6 s later and the summon; then no floor is left.
    #[test]
    fn isabella_transforms_at_forty_and_calls_the_water_at_twenty_five() {
        let mut isabella = Isabella {
            intro_done: true,
            floor: 3,
            ..Isabella::new()
        };
        let lock = T0;
        isabella.tick(lock, &view(true));
        assert_eq!(
            events(&isabella.tick(lock + TRANSFORM_AFTER_MS, &view(true))),
            [TRANSFORM_EVENT]
        );
        let steps = isabella.tick(
            lock + TRANSFORM_AFTER_MS + TRANSFORM_PLAYER_AFTER_MS,
            &view(true),
        );
        assert_eq!(steps, [Step::TakePlayers]);
        let stage = lock + TRANSFORM_AFTER_MS;
        let mut winged = view(true);
        winged.winged_ms = Some(stage);
        assert_eq!(
            events(&isabella.tick(stage + WINGED_AFTER_MS, &winged)),
            [WINGED_EVENT]
        );
        winged.water_1_ms = Some(stage + WINGED_AFTER_MS);
        let water_1 = stage + WINGED_AFTER_MS + WATER_1_AFTER_MS;
        let steps = isabella.tick(water_1, &winged);
        assert_eq!(events(&steps), [WATER_1_EVENT]);
        assert!(isabella.invulnerable(water_1).is_some());
        assert!(
            isabella
                .invulnerable(water_1 + WINGED_VULNERABLE_AFTER_MS)
                .is_none()
        );
        let low = water_1 + 30_000;
        isabella.tick(low, &view(true));
        let steps = isabella.tick(low + WATER_2_AFTER_MS, &view(true));
        assert_eq!(events(&steps), [WATER_2_EVENT]);
        assert!(steps.contains(&Step::Floor {
            old: Some(FLOORS[4].0),
            new: None
        }));
        let summon = low + WATER_2_AFTER_MS + SUMMON_AFTER_MS;
        assert!(isabella.invulnerable(summon).is_some());
        assert!(isabella.invulnerable(summon + SUMMON_MS).is_none());
        assert!(
            isabella
                .tick(summon + SUMMON_MS + 60_000, &view(true))
                .is_empty()
        );
    }

    /// Retail `Soul-Trial---The-End-of-new-Eden`: the intro 5.65 s after its stage, shadow
    /// archers 13.1 s into the fight and every 19.3 s, the second phase below half health with
    /// its own calls.
    #[test]
    fn adiana_calls_shadows_on_a_timer_and_starts_her_second_phase_below_half() {
        let mut adiana = Adiana::new();
        assert!(adiana.invulnerable().is_some());
        assert!(adiana.tick(T0, Some(T0), 1.0).is_empty());
        let fight = T0 + ADIANA_INTRO_MS;
        assert_eq!(
            events(&adiana.tick(fight, Some(T0), 1.0)),
            [ADIANA_INTRO_EVENT]
        );
        assert!(adiana.invulnerable().is_none());
        assert!(
            adiana
                .tick(fight + SHADOW_FIRST_MS - 1, Some(T0), 1.0)
                .is_empty()
        );
        assert_eq!(
            adiana.tick(fight + SHADOW_FIRST_MS, Some(T0), 0.9),
            [Step::Shadows]
        );
        assert!(
            adiana
                .tick(fight + SHADOW_FIRST_MS + 1, Some(T0), 0.9)
                .is_empty()
        );
        let second = fight + SHADOW_FIRST_MS + SHADOW_EVERY_MS;
        assert_eq!(adiana.tick(second, Some(T0), 0.6), [Step::Shadows]);
        let phase2 = second + 1_000;
        assert_eq!(
            adiana.tick(phase2, Some(T0), 0.49),
            [Step::Play(ADIANA_PHASE2_FRAGMENT)]
        );
        assert_eq!(
            adiana.tick(phase2 + PHASE2_SHADOW_FIRST_MS, Some(T0), 0.4),
            [Step::Shadows]
        );
        assert!(
            adiana
                .tick(
                    phase2 + PHASE2_SHADOW_FIRST_MS + SHADOW_EVERY_MS,
                    Some(T0),
                    0.3
                )
                .is_empty()
        );
        assert_eq!(
            adiana.tick(
                phase2 + PHASE2_SHADOW_FIRST_MS + PHASE2_SHADOW_EVERY_MS,
                Some(T0),
                0.2
            ),
            [Step::Shadows]
        );
    }

    #[test]
    fn the_fights_start_for_the_three_bosses_only() {
        assert!(matches!(
            fight_of(lowercase_crc32(LIANG_VITALS), T0),
            Some(Fight::Liang(_))
        ));
        assert!(matches!(
            fight_of(lowercase_crc32(ISABELLA_VITALS), T0),
            Some(Fight::Isabella(_))
        ));
        assert!(matches!(
            fight_of(lowercase_crc32(ADIANA_VITALS), T0),
            Some(Fight::Adiana(_))
        ));
        assert_eq!(
            fight_of(lowercase_crc32("abominable_liang_msq_idle"), T0),
            None
        );
        assert!(touch_pickup(TALISMAN_PILE_SLICE));
        assert!(!touch_pickup("slices/other.dynamicslice"));
    }
}
