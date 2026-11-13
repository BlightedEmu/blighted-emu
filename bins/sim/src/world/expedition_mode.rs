// SPDX-License-Identifier: AGPL-3.0-or-later
//! The game mode entity of an expedition (`InstanceKind::Expedition`): its state, its objective,
//! the run's score and times, and the mutation curse meter on it (findings
//! `2026-11-11-dungeon-gates-and-game-mode-state.md` section 5, `2026-11-13-catacomb-erosion.md`
//! sections C11 to C15). The shared game mode entity code is [`super::game_mode_entity`]; the
//! script values of each expedition are [`super::expedition_scripts`]; the meter runs on the
//! shared affliction runtime ([`super::afflictions::AfflictionMeters`]).
//!
//! **Observed** (44 retail expedition ledgers of 12 expeditions, scratch
//! `local-data/scratch-work/c14-expedition/runs/` and `c15-expedition/runs/`; seconds from each
//! ledger's first record):
//!
//! - The instance has one game mode entity: a card with a `GdeRef` and no asset, the game mode
//!   state (2343) on component 2 and the objectives (3857) on component 3 (Starstone 7.282 s).
//! - Its first state: script state 0, an empty spawner list, the mode's CRC as mode and map,
//!   the participant lists, empty raid ids, 19 values ([`ENTRY_VALUES`]: `timeToBeat` and its
//!   twin `d7e74d93` hold the expedition's time to beat), the timers `GameModeTimeout` (the
//!   opening + 3 h) and `TimeLimit` (the opening + 600 s, every dungeon's
//!   `TimerThresholdDefault`), the empty global affliction data and linked mode 0. Its
//!   objectives: tracked empty, the active record of the expedition's objective (id and
//!   instance the same in every run of one expedition, accept time the opening, POI) and no
//!   task; all three lists at the version of the state's lists (one counter for both members).
//! - A participant's status is a set of bits (the client's
//!   `eGameModeParticipantStatus_ActiveFlag`, `_DeadFlag`, `_DeathsDoorFlag`): 2 while its
//!   client loads (Starstone 7.282 s: `3, 2, 3`), 3 once in the game (11.852 s), 7 dead
//!   (178.243 s), 11 at death's door (Barnacles 285.941 s, 10 to 30 s; then 3, revived, 148
//!   times, or 7, 61 times), and with the bit 16 while in combat in a Fiendish run only (757
//!   changes in `20261002-184508-050`, none in the 43 other ledgers; the curse script's rise
//!   tests it). Inferred: bit 1 is the client in the game ([`Vitals::spawned`]).
//! - The bit 2 is the player online (Observed, scratch `local-data/scratch-work/c16-expedition/`
//!   `activetie.py`: 173 of 183 changes of it in 11 ledgers come within 0.21 s before to 0.26 s
//!   after the social service's `OnPlayerOnlineStateChange` (4128) about that participant's
//!   character, state 0 with the bit off and 1 with it on; the other 10 are dead or death's door
//!   changes of an offline player, 1 to 5 or 9). A player whose connection ends stays in the
//!   lists without it and gets it back when its character comes again
//!   ([`SimWorld::expedition_went_offline`]); the 20 to 36 s drops every 2 to 3 minutes of two players are
//!   their clients' own social connection drops. Inferred: an offline participant is never down
//!   for a team wipe, and stays until the instance closes.
//! - Its participant state names the entity with the flags `11 00 07` and the mutation context:
//!   empty in a normal run, the run's mutation in a mutated one.
//! - The script state is the count of the objective's done tasks, which end in the script's
//!   order (Starstone 360.171 s: 6 to 12, its ampitheater exit and the five mutation tasks of a
//!   normal run at once; the Depths, which has no task 1: state 1 with task 2 done). A task of
//!   [`ExpeditionScript::area_tasks`] ends when an online participant stands in its trigger
//!   area. **Inferred**: retail ends each task in turn (every step is +1 but the mutation one);
//!   the tasks before an area task whose own end the sim does not see (encounter ends,
//!   interactions) end with it, and standing in the area counts, not only the entry. The task
//!   list holds the done tasks and the root (`ConsecutiveTask`, progress the done count, state
//!   active), in the order they first came: the first task, the root, then the others.
//! - `startTimeSeconds` (`9e97fe15`): the Unix second of its setting, when the first player
//!   enters the level script's `AreaMutatorStart` trigger area
//!   ([`START_AREA_ENTITY`] of [`nw_world::dungeon_scripts::LevelScript::trigger_areas`];
//!   Observed, scratch
//!   `c16-expedition/startcheck.py`: in 26 of 26 runs whose capture shows a player cross it,
//!   the value is set between the first player place reported inside the area grown by the place
//!   resolution of 2 m and the first one inside it, -0.75 to +0.37 s, e.g. Starstone 20.818 s;
//!   the line `MutatorStartLine` is its visible edge). A level without one starts no clock.
//! - The run's end (24 runs: the script state of the last boss task, then one more task after
//!   the expedition's delay) sets `endTimeSeconds` (`e2100965`), the score `0b688bb2` to
//!   score + 5 x (time to beat - run seconds) (`pointsPerSecondUnder` 5; Starstone 7230, Depths 8745,
//!   Dynasty 9345 exact), `de23d4d2` to that times the multiplier `bc29d952`, and `MutatorRank`
//!   (`9a03db12`) to the rank of that product (`rankSilverPts` 25,000, `rankGoldPts` 50,000:
//!   0, 1 or 2 in all 26 ends). The score is `0b688bb2` with the sign flag `376e740b`.
//! - A mutated run scores its kills (Observed: runs 16, 20 and 34; no normal run changes
//!   `f963dc0c` or `4f6a27d3`): a normal kill the script's points
//!   ([`ExpeditionScript::kill_points`], Empyrean 24, Tempest 20) up to its kills for bonus (85 and 100; later kills
//!   count `f963dc0c` with 0), the last one sets `6974d92c` and the multiplier +1; a named kill
//!   500 up to 12 (`namedEnemiesKilled` `4f6a27d3`), the 12th sets `d15dc70a` and the multiplier
//!   +1 (Empyrean 151.752 s: the named fire knight's death, 4f6a27d3 1, +500). A death -500
//!   (`pointsLostForIndividualDeath`) and counts `1fa74921`; past 3 deaths
//!   (`individualDeathsForBonus`) `2f999238` goes 1 to 0 (runs 30, 33, 40, 41) and the
//!   multiplier stays; a team wipe -500 (`pointsLostForTeamWipe`), counts `95b5c8e9`, and the
//!   first one sets `7690bdb7` 1 to 0 and the multiplier 3 to 2 (`maxTeamWipesForBonus` 0; runs
//!   15, 37, 40, 41); the run past its time to beat sets `5f770ef6` 1 to 0 and the multiplier -1
//!   (run 40, 2400.009 s after the start). **Inferred**: a named kill is a kill of a mob of the
//!   creature type `DungeonBoss` or `DungeonMiniBoss` (the client's score data names them; no
//!   capture holds the named list's ids); a team wipe is every participant dead or at death's
//!   door at once.
//! - `gatherablesCollected` (`8f3d2f0e`, its lower-case CRC) rises by the expedition's points in
//!   normal and mutated runs (Depths, Lazarus, Ennead, Genesis, Glacial, Empyrean 400; Dynasty,
//!   Barnacles, Savage 350; Tempest 225; no boss death meets a rise, the Depths' priest boss died
//!   at 575.6 s and the rises are at 90.635 and 513.635 s): what a gatherable is, the ledgers do
//!   not show (no own interaction meets every rise), so the sim keeps it at 0.
//! - The `TimeLimit` timer goes at its end: the timers list again with `GameModeTimeout` alone
//!   (Starstone 606.379 s, Depths 604.364 s, Dynasty 607.784 s).
//! - Retail sends the changed values and tasks alone; the sim sends each list whole at its new
//!   version (Inferred: a full list replaces the client's).
//!
//! **The curse meter** (retail `20261002-184508-050`, a Fiendish run; the client's copy of the
//! server's curse script, [`nw_content::mutations::CurseMeter`]): child 22 holds the run's one
//! meter of the curse's `GlobalAffliction` ([`nw_messages::GlobalAffliction`]). Each second the
//! rise effect's gain comes once, or tick / 10 + 1 times for Fiendish, when a run player is
//! alive, in the game and (Fiendish) in combat. An ability a run player performs fires the minor
//! curse's `OnPerform` ability ([`SimWorld::ability_performed`]). At the top it is full: it falls
//! over the row's `AfflictedDrainTime` and takes no gain (`IgnoreWhenAfflicted`), and the curse's
//! fill acts ([`nw_content::mutations::CurseFill`]): the Fiendish Weary zone is cast on each
//! living run player (103 of 103 fills: `Mut_Curse2_Minor_SlowRend` on 3 to 5 players within
//! 0.7 s); Desiccated marks the living run player with the lowest health share, which explodes
//! when the fall ends: a chain from it to the nearest living target within the spell's radius,
//! a few times (Inferred: the chain's target order and that it hits mobs too, `TargetTypes`
//! `All`; no capture holds a Desiccated run).
//!
//! - The curse script's tick timers (`72959eca` Fiendish, `b9c94d6f` Desiccated: the client's
//!   copy adds them with 1.0 s and the flag 0, `FUN_1461f1790`, and removes them, `FUN_1461f2730`,
//!   `FUN_1461f1ab0`) are server timers the client never gets: the 254 changes of `72959eca` in
//!   the only Fiendish ledger are all removals of a key the list never held (none adds it), so the
//!   client's timers list is the same without them and the sim sends none.
//!
//! **Not built**: the spawner list, the level script's timer (`66281118`), the gatherables, and
//! the tasks between the start and the end that wait for no trigger area: they follow server
//! script events the client does not get; the ledgers tie only the last boss task to an
//! encounter's end (Starstone 874.535 s, its boss encounter succeeded, 874.617 s the task), so
//! the run's end does the rest at once.

use bevy_ecs::entity::Entity;
use nw_content::afflictions::AfflictionGain;
use nw_content::instances::InstanceKind;
use nw_content::mutations::{AppliedEffect, CurseExplosion, CurseFill, CurseMeter};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::game_mode_participant::{MutationContextWire, PARTICIPANT_FLAGS_LEN};
use nw_messages::game_mode_state::{GameModeState, Keyed};
use nw_messages::list_delta::{ListChange, ReplicatedList};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleMember, MemberState, ObjectiveRecord, ObjectivesState, TaskEntry, TaskWireState,
};
use nw_sim_core::Position;
use nw_world::dungeon_scripts::{Detector, START_AREA_ENTITY};

use super::afflictions::{AfflictionMeters, Change};
use super::expedition_scripts::{AreaTask, ExpeditionScript, expedition_script};
use super::game_mode_entity::{
    FIRST_LIST_VERSION, GAME_MODE_COMPONENT, MatchEntry, MatchParticipant, full, left_state,
    match_joined_state, participant_lists,
};
use super::spell_areas::SpellArea;
use super::{Body, Player, SimWorld, Vitals, unix_ns};

/// The namespace of the `GdeRef` of an expedition's game mode entity ("expemode").
const NAMESPACE: u64 = 0x6578_7065_6d6f_6465;
/// The component of the objectives on the game mode entity (Observed: 3 in every run).
const OBJECTIVES_COMPONENT: u64 = 3;
/// The script state of the first state (Observed: 0 in every expedition entry).
const FIRST_SCRIPT_STATE: u8 = 0;
/// The linked mode of an expedition (Observed: 0).
const LINKED_MODE: u8 = 0;
/// The participant flags of an expedition's player (Observed: `11 00 07`, Starstone 7.141 s).
const JOINED_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x11, 0x00, 0x07, 0x00, 0x00, 0x00];
/// The status bit of a participant whose client is in the game (module text).
const LOADED_BIT: u8 = 0x01;
/// `eGameModeParticipantStatus_ActiveFlag`.
const ACTIVE_BIT: u8 = 0x02;
/// `eGameModeParticipantStatus_DeadFlag`.
const DEAD_BIT: u8 = 0x04;
/// `eGameModeParticipantStatus_DeathsDoorFlag`.
const DEATHS_DOOR_BIT: u8 = 0x08;
/// The status bit of a participant in combat (module text).
const IN_COMBAT_BIT: u8 = 0x10;
/// The team of every expedition participant (Observed: 0).
const TEAM: u8 = 0;
/// `GameModeTimeout`: the instance's end, 3 h after its opening (Observed in 30 entries:
/// 10,799 s after the first state, which came about 1 s after the opening).
const GAME_MODE_TIMEOUT: u32 = 0xd815_d620;
/// The instance's time when its game mode names no `OverrideDungeonTimeout`, ms.
const DUNGEON_TIMEOUT_MS: u64 = 10_800_000;
/// `TimeLimit`: the end of every dungeon's `TimerThresholdDefault` (600 s) from the opening
/// (Observed: 599 s after the first state, gone at its end).
const TIME_LIMIT: u32 = 0x8f0e_9156;
/// `TimerThresholdDefault` of every dungeon of build 25810808, ms.
const TIME_LIMIT_MS: u64 = 600_000;
/// `timeToBeat` (the client's `mprogress.luac` names it).
const TIME_TO_BEAT: u32 = 0x01e4_da88;
/// The value that holds the time to beat with `timeToBeat` at the entry (name Unknown).
const TIME_TO_BEAT_TWIN: u32 = 0xd7e7_4d93;
/// `namedEnemiesKilled` (`mprogress.luac`): the named kills of a mutated run.
const NAMED_ENEMIES_KILLED: u32 = 0x4f6a_27d3;
/// 1 once a mutated run's named kills reach [`NAMED_FOR_BONUS`] (name Unknown).
const NAMED_BONUS: u32 = 0xd15d_c70a;
/// The normal kills of a mutated run (name Unknown).
const NORMAL_KILLS: u32 = 0xf963_dc0c;
/// 1 once a mutated run's normal kills reach its script's kills for bonus (name Unknown).
const KILL_BONUS: u32 = 0x6974_d92c;
/// `gatherablesCollected` (its lower-case CRC; module text).
const GATHERABLES: u32 = 0x8f3d_2f0e;
/// `endTimeSeconds` (`mprogress.luac`).
const END_TIME: u32 = 0xe210_0965;
/// 1 while the score is below 0 (name Unknown).
const SCORE_NEGATIVE: u32 = 0x376e_740b;
/// 1 until the first team wipe (name Unknown).
const WIPE_BONUS: u32 = 0x7690_bdb7;
/// The score's size (name Unknown).
const SCORE: u32 = 0x0b68_8bb2;
/// The end score times the multiplier (name Unknown).
const FINAL_SCORE: u32 = 0xde23_d4d2;
/// 1 until the run passes its time to beat (name Unknown).
const TIME_BONUS: u32 = 0x5f77_0ef6;
/// `startTimeSeconds` (`mprogress.luac`).
const START_TIME: u32 = 0x9e97_fe15;
/// `MutatorRank`.
const MUTATOR_RANK: u32 = 0x9a03_db12;
/// The multiplier: the bonuses still met (name Unknown).
const MULTIPLIER: u32 = 0xbc29_d952;
/// 1 until the deaths pass `individualDeathsForBonus` (name Unknown).
const DEATH_BONUS: u32 = 0x2f99_9238;
/// `TeamWipe`: the team wipes.
const TEAM_WIPES: u32 = 0x95b5_c8e9;
/// The deaths (name Unknown).
const DEATHS: u32 = 0x1fa7_4921;
/// The multiplier of the first state (Observed: 3).
const FIRST_MULTIPLIER: u64 = 3;
/// A bonus flag that holds (Observed: 1).
const BONUS_MET: u64 = 1;
/// A bonus flag that is lost (Observed: 0).
const BONUS_LOST: u64 = 0;
/// The values of the first state in retail order, the time to beat left 0 ([`TIME_TO_BEAT`],
/// [`TIME_TO_BEAT_TWIN`]).
const ENTRY_VALUES: [Keyed; 19] = [
    (TIME_TO_BEAT_TWIN, 0),
    (NAMED_ENEMIES_KILLED, 0),
    (END_TIME, 0),
    (SCORE_NEGATIVE, 0),
    (NAMED_BONUS, 0),
    (WIPE_BONUS, BONUS_MET),
    (SCORE, 0),
    (FINAL_SCORE, 0),
    (TIME_BONUS, BONUS_MET),
    (GATHERABLES, 0),
    (START_TIME, 0),
    (TIME_TO_BEAT, 0),
    (MUTATOR_RANK, 3),
    (NORMAL_KILLS, 0),
    (MULTIPLIER, FIRST_MULTIPLIER),
    (DEATH_BONUS, BONUS_MET),
    (TEAM_WIPES, 0),
    (KILL_BONUS, 0),
    (DEATHS, 0),
];
/// The time to beat of a game mode the script table lacks (Inferred: the most common observed
/// value, 5 of the 12 expeditions).
const DEFAULT_TIME_TO_BEAT_S: u64 = 2_400;
/// `pointsLostForIndividualDeath` (Observed: -500 a death in every run).
const DEATH_POINTS: i64 = -500;
/// `pointsLostForTeamWipe` (Observed: -500 a wipe, runs 15, 37, 40).
const TEAM_WIPE_POINTS: i64 = -500;
/// `pointsPerSecondUnder` and, below 0, `pointsPerSecondOver` (Observed).
const POINTS_PER_SECOND_UNDER: i64 = 5;
/// `individualDeathsForBonus` (Observed: the bonus goes at the 4th death).
const DEATHS_FOR_BONUS: u64 = 3;
/// `rankSilverPts` and `rankGoldPts` (client `DAT_149f38eec`, `DAT_149f38ef0`): the final score
/// of rank 1 and of rank 2.
const RANK_POINTS: [i64; 2] = [25_000, 50_000];
/// The creature types of a named kill (module text).
const NAMED_CREATURE_TYPES: [&str; 2] = ["DungeonBoss", "DungeonMiniBoss"];
/// The points of a mutated run's named kill (Observed: +500 at each `4f6a27d3` step of runs 16,
/// 20 and 34, Empyrean and Tempest).
const NAMED_KILL_POINTS: u64 = 500;
/// The named kills that score; the last sets [`NAMED_BONUS`] and the multiplier +1 (Observed:
/// the 12th in runs 16, 20 and 34; the 13th and 14th scored 0).
const NAMED_FOR_BONUS: u64 = 12;
/// The id of the objective's root task, `ConsecutiveTask` (Observed: `91d8155f` in every run).
const ROOT_TASK: u32 = 0x91d8_155f;
/// The progress of a done task (Observed: 1).
const DONE_PROGRESS: u32 = 1;
/// The mark of a task key of a mutated run only (Observed: Starstone's
/// `@dungeon_everfall_task_mut_*`, tasks 8 to 12).
const MUTATION_TASK_MARK: &str = "_task_mut_";
/// How often the curse meter rises, ms (Observed: the script's 1.0 s timer, `DAT_147eff940`).
const RISE_PERIOD_MS: u64 = 1_000;
/// The rise ticks per extra Fiendish gain (Observed: `FUN_1461f2730`, tick / 10).
const TICKS_PER_ESCALATION: u32 = 10;
/// The potency of an effect the curse puts on a player.
const CURSE_POTENCY: f64 = 1.0;
/// Milliseconds in a second.
const MS_PER_S: u64 = 1_000;

/// The chain of a marked player's explosion that runs ([`CurseExplosion`]).
#[derive(Debug, Clone, PartialEq)]
struct CurseChain {
    explosion: CurseExplosion,
    /// The entity numbers it hit, the marked player's first (the spell ignores its caster).
    hit: Vec<u64>,
    /// Where it jumps from.
    from: Position,
    /// The jumps left.
    jumps_left: u32,
    /// When it jumps next (server ms).
    next_ms: u64,
}

/// The curse meter of a mutated expedition.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RunCurse {
    /// The curse's meter rules.
    rule: CurseMeter,
    /// CRC-32 of the lowercase `GlobalAffliction`.
    kind: u32,
    /// CRC-32 of the lowercase minor curse: the holder of the run players' `OnPerform` ability.
    minor: u32,
    /// The meter.
    meters: AfflictionMeters,
    /// When the meter rises next (server ms); `None` while the Fiendish timer stops (from the
    /// fill to the fall's end).
    next_rise_ms: Option<u64>,
    /// The rise ticks since the start or the last fill.
    rise_ticks: u32,
    /// The marked player of a Desiccated fill, until its explosion.
    marked: Option<Entity>,
    /// The explosion's chain while it runs.
    chain: Option<CurseChain>,
}

/// The run's score and times (module text).
#[derive(Debug, Clone, PartialEq, Eq)]
struct RunScore {
    /// The points, below 0 after a death before any gain.
    points: i64,
    /// The start, Unix seconds.
    start_s: Option<u64>,
    /// The run ended.
    ended: bool,
    deaths: u64,
    team_wipes: u64,
    /// The named kills of a mutated run.
    named: u64,
    /// The normal kills of a mutated run.
    kills: u64,
    /// The bonuses still met.
    multiplier: u64,
    /// Every participant is down now (a team wipe counted).
    wiped: bool,
}

impl Default for RunScore {
    fn default() -> Self {
        Self {
            points: 0,
            start_s: None,
            ended: false,
            deaths: 0,
            team_wipes: 0,
            named: 0,
            kills: 0,
            multiplier: FIRST_MULTIPLIER,
            wiped: false,
        }
    }
}

/// The game mode entity of an expedition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExpeditionMode {
    /// Its slot key and raw id.
    entity: (u64, [u8; RAW_ID_LEN]),
    /// The CRC of the game mode (and of its map).
    mode_crc: u32,
    /// The expedition's script values; `None` for a game mode the table lacks.
    script: Option<&'static ExpeditionScript>,
    /// Its players, in arrival order.
    participants: Vec<MatchParticipant>,
    /// The characters of the participants that are offline (module text).
    offline: Vec<[u8; RAW_ID_LEN]>,
    /// The level script's start area ([`START_AREA_ENTITY`]).
    start_area: Option<Detector>,
    /// The tasks that end in a trigger area, with the area's volume.
    task_areas: Vec<(AreaTask, Detector)>,
    /// The version of its lists, its objectives' too.
    version: u64,
    /// The timers of its first state.
    timers: Vec<Keyed>,
    /// When the `TimeLimit` timer ends (server ms), while it runs.
    time_limit_ends_ms: Option<u64>,
    /// Its values as they stand.
    values: Vec<Keyed>,
    /// The run's score and times.
    score: RunScore,
    /// The objective's done tasks: the script state.
    done: u8,
    /// The objective's task entries in their first order.
    tasks: Vec<TaskEntry>,
    /// When the task after the run's end ends (server ms), while it waits.
    victory_due_ms: Option<u64>,
    /// The curse meter of a mutated run.
    curse: Option<RunCurse>,
}

impl ExpeditionMode {
    /// Its slot key.
    pub(super) const fn entity(&self) -> u64 {
        self.entity.0
    }

    /// The next version of its lists.
    fn next_version(&mut self) -> u64 {
        self.version += 1;
        self.version
    }

    /// The participant lists at a new version.
    fn lists(&mut self) -> GameModeState {
        let version = self.next_version();
        participant_lists(&self.participants, version, true)
    }

    /// Sets the values `changes`, each one in place.
    fn set_values(&mut self, changes: &[Keyed]) {
        for &(key, value) in changes {
            if let Some(held) = self.values.iter_mut().find(|(held, _)| *held == key) {
                held.1 = value;
            }
        }
    }

    /// The value of `key`.
    fn value(&self, key: u32) -> u64 {
        self.values
            .iter()
            .find(|(held, _)| *held == key)
            .map_or(0, |(_, value)| *value)
    }

    /// The score's values: its size and the sign flag.
    fn score_values(&self) -> [Keyed; 2] {
        let points = self.score.points;
        [
            (SCORE, points.unsigned_abs()),
            (SCORE_NEGATIVE, u64::from(points < 0)),
        ]
    }

    /// `done`, and past the mutation tasks that follow it: a normal run ends them with the
    /// task before them (Observed: Starstone runs 3 and 30, 360.171 s, the state 6 to 12, its
    /// `ampitheaterexit` and the five `_task_mut_` tasks).
    fn past_mutation_tasks(&self, done: u8) -> u8 {
        let Some(script) = self.script else {
            return done;
        };
        let skipped = script
            .tasks
            .iter()
            .skip(usize::from(done))
            .take_while(|(_, key)| key.contains(MUTATION_TASK_MARK))
            .count();
        done.saturating_add(u8::try_from(skipped).unwrap_or(u8::MAX))
    }

    /// The objective's entries with the first `done` tasks done (module text).
    fn tasks_done(&mut self, done: u8) {
        let Some(script) = self.script else {
            return;
        };
        let instance = script.instance;
        let entry = |index, task, progress| TaskEntry {
            instance,
            index,
            task,
            progress,
            state: TaskWireState::Complete,
        };
        let new = script
            .tasks
            .iter()
            .take(usize::from(done))
            .skip(usize::from(self.done));
        for &(index, key) in new {
            self.tasks
                .push(entry(index, lowercase_crc32(key), DONE_PROGRESS));
        }
        let root = TaskEntry {
            state: TaskWireState::Active,
            ..entry(script.root_index, ROOT_TASK, u32::from(done))
        };
        match self
            .tasks
            .iter_mut()
            .find(|task| task.index == script.root_index)
        {
            Some(held) => *held = root,
            // The root comes after the first task (Observed: `[1, 27, 2, ...]`).
            None => self.tasks.insert(self.tasks.len().min(1), root),
        }
        self.done = done;
    }
}

/// The time to beat of an expedition with the script values `script`, seconds.
fn time_to_beat_s(script: Option<&ExpeditionScript>) -> u64 {
    script.map_or(DEFAULT_TIME_TO_BEAT_S, |script| script.time_to_beat_s)
}

/// The values of the first state of an expedition with the script values `script` (module
/// text).
fn entry_values(script: Option<&ExpeditionScript>) -> Vec<Keyed> {
    let time_to_beat = time_to_beat_s(script);
    ENTRY_VALUES
        .iter()
        .map(|&(key, value)| match key {
            TIME_TO_BEAT | TIME_TO_BEAT_TWIN => (key, time_to_beat),
            _ => (key, value),
        })
        .collect()
}

/// The first state of an expedition of mode `mode_crc` with the values `values` and the timers
/// `timers`.
fn first_state(mode_crc: u32, values: &[Keyed], timers: &[Keyed]) -> GameModeState {
    GameModeState {
        script_state: Some(FIRST_SCRIPT_STATE),
        spawned: Some(full(FIRST_LIST_VERSION, Vec::new())),
        mode: Some(mode_crc),
        map: Some(mode_crc),
        raids: Some(ReplicatedList::Full(full(FIRST_LIST_VERSION, Vec::new()))),
        values: Some(full(FIRST_LIST_VERSION, values.to_vec())),
        timers: Some(full(FIRST_LIST_VERSION, timers.to_vec())),
        no_affliction: true,
        linked_mode: Some(LINKED_MODE),
        ..participant_lists(&[], FIRST_LIST_VERSION, true)
    }
}

/// The first objectives of the expedition `script` opened at server ms `opened_ms`: no tracked
/// objective, the active record, no task (module text).
fn first_objectives(script: &ExpeditionScript, opened_ms: u64) -> ObjectivesState {
    let mut record = ObjectiveRecord::available(script.objective, script.instance);
    record.accept_time = unix_ns(opened_ms);
    record.poi = true;
    ObjectivesState {
        tracked: Some(full(FIRST_LIST_VERSION, Vec::new())),
        active: Some(full(FIRST_LIST_VERSION, vec![record])),
        tasks: Some(full(FIRST_LIST_VERSION, Vec::new())),
        ..ObjectivesState::default()
    }
}

/// A kill of creature type `kind` is a named kill ([`NAMED_CREATURE_TYPES`]).
fn named_creature(kind: &str) -> bool {
    NAMED_CREATURE_TYPES
        .iter()
        .any(|named| named.eq_ignore_ascii_case(kind))
}

/// The straight distance from `from` to `to`, m.
fn reach_m(from: Position, to: Position) -> f32 {
    let (east, north, up) = (to.x - from.x, to.y - from.y, to.z - from.z);
    (east * east + north * north + up * up).sqrt()
}

/// The rank of the final score `score` ([`RANK_POINTS`]).
fn rank_of(score: i64) -> u64 {
    RANK_POINTS
        .iter()
        .filter(|&&points| score >= points)
        .count() as u64
}

impl SimWorld {
    /// The game mode entity of this sim's expedition.
    fn expedition(&self) -> Option<&ExpeditionMode> {
        self.instance_run.as_ref()?.expedition()
    }

    /// The game mode entity of this sim's expedition, to change.
    fn expedition_mut(&mut self) -> Option<&mut ExpeditionMode> {
        self.instance_run
            .as_mut()
            .and_then(super::instances::InstanceRun::expedition_mut)
    }

    /// Places the game mode entity of the expedition this sim opened (module text), at the
    /// map's first teleport; nothing for another kind of instance.
    pub(super) fn place_expedition_mode(&mut self) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(mode) = self
            .instance_run
            .as_ref()
            .and_then(|run| content.instances.by_id(run.game_mode()))
            .filter(|mode| mode.kind == InstanceKind::Expedition)
        else {
            return;
        };
        let opened_ms = self.server_ms;
        let timeout_ms = mode.timeout_seconds.map_or(DUNGEON_TIMEOUT_MS, |seconds| {
            seconds.saturating_mul(MS_PER_S)
        });
        let time_limit_ends_ms = opened_ms.saturating_add(TIME_LIMIT_MS);
        let timers = vec![
            (GAME_MODE_TIMEOUT, opened_ms.saturating_add(timeout_ms)),
            (TIME_LIMIT, time_limit_ends_ms),
        ];
        let [x, y, z] = mode
            .map
            .teleports
            .first()
            .map(|teleport| teleport.position)
            .unwrap_or_default();
        let script = expedition_script(&mode.id);
        let level = content.dungeon_scripts.game_mode(&mode.id);
        let start_area = level
            .and_then(|level| level.trigger_area(START_AREA_ENTITY))
            .cloned();
        if start_area.is_none() && script.is_some_and(|script| script.scored) {
            tracing::warn!(game_mode = %mode.id, "a scored expedition without a start area");
        }
        let task_areas: Vec<(AreaTask, Detector)> = script
            .map_or(&[][..], |script| script.area_tasks)
            .iter()
            .filter_map(|task| {
                let volume = level.and_then(|level| level.trigger_area(task.area)).cloned();
                if volume.is_none() {
                    tracing::warn!(game_mode = %mode.id, area = task.area, "a task area is missing");
                }
                Some((*task, volume?))
            })
            .collect();
        let values = entry_values(script);
        let state = first_state(mode.crc, &values, &timers);
        let objectives = script
            .map(|script| BundleMember {
                component: OBJECTIVES_COMPONENT,
                state: MemberState::Objectives(first_objectives(script, opened_ms)),
            })
            .into_iter()
            .collect();
        let entity = self.place_game_mode_entity_with(
            GAME_MODE_COMPONENT,
            NAMESPACE,
            None,
            Position { x, y, z },
            state,
            objectives,
        );
        tracing::info!(game_mode = %mode.id, "expedition game mode entity placed");
        if let Some(run) = self.instance_run.as_mut() {
            run.set_expedition(ExpeditionMode {
                entity,
                mode_crc: mode.crc,
                script,
                participants: Vec::new(),
                offline: Vec::new(),
                start_area,
                task_areas,
                version: FIRST_LIST_VERSION,
                timers,
                time_limit_ends_ms: Some(time_limit_ends_ms),
                values,
                score: RunScore::default(),
                done: 0,
                tasks: Vec::new(),
                victory_due_ms: None,
                curse: None,
            });
        }
    }

    /// The mutation context of this sim's run for a participant state: the empty one in a
    /// normal run (module text).
    fn run_mutation_context(&self) -> MutationContextWire {
        self.instance_mutation()
            .map_or(MutationContextWire::NONE, |context| MutationContextWire {
                elemental: lowercase_crc32(&context.set.elemental),
                promotion: lowercase_crc32(&context.set.promotion),
                curse: lowercase_crc32(&context.set.curse),
                difficulty: context.difficulty,
            })
    }

    /// The status of participant `entity` (module text): `combat` adds the in-combat bit.
    fn participant_status(&self, entity: Entity, combat: bool) -> u8 {
        let mut status = ACTIVE_BIT;
        if let Some(vitals) = self.world.get::<Vitals>(entity) {
            if vitals.spawned {
                status |= LOADED_BIT;
            }
            if vitals.downed() {
                status |= DEATHS_DOOR_BIT;
            } else if vitals.health.is_dead() {
                status |= DEAD_BIT;
            }
        }
        let fighting = self
            .world
            .get::<Player>(entity)
            .is_some_and(|player| player.combat.in_combat());
        if combat && fighting {
            status |= IN_COMBAT_BIT;
        }
        status
    }

    /// The run's curse needs the in-combat bit (its rise does).
    fn curse_counts_combat(&self) -> bool {
        self.expedition()
            .and_then(|expedition| expedition.curse.as_ref())
            .is_some_and(|curse| curse.rule.rise_needs_combat)
    }

    /// Player `entity` arrived in this sim's expedition: it joins the lists with its status, and
    /// its participant state names the entity with the joined flags and the run's mutation
    /// (module text).
    pub(super) fn join_expedition_mode(&mut self, entity: Entity) {
        let Some((number, character, eligible)) = self.world.get::<Player>(entity).map(|player| {
            (
                player.number,
                *player.character.as_bytes(),
                player.arena.queue_eligible().to_vec(),
            )
        }) else {
            return;
        };
        let phase = self.instance_run.as_ref().map_or(0, |run| run.phase());
        let server_ms = self.server_ms;
        let status = self.participant_status(entity, self.curse_counts_combat());
        let mutation_context = self.run_mutation_context();
        let Some(expedition) = self.expedition_mut() else {
            return;
        };
        if let Some(index) = expedition
            .offline
            .iter()
            .position(|held| *held == character)
        {
            // Back online: its place in the lists, the bit 2 set again (module text).
            expedition.offline.remove(index);
            if let Some(held) = expedition
                .participants
                .iter_mut()
                .find(|participant| participant.character == character)
            {
                held.number = number;
                held.entity = entity;
                held.status = status;
            }
        } else if expedition
            .participants
            .iter()
            .any(|participant| participant.number == number)
        {
            return;
        } else {
            expedition.participants.push(MatchParticipant {
                number,
                entity,
                character,
                team: TEAM,
                place: 0,
                status,
            });
        }
        let lists = expedition.lists();
        let (slot, gde_ref) = expedition.entity;
        let entry = MatchEntry {
            gde_ref,
            mode: expedition.mode_crc,
            map: expedition.mode_crc,
            phase,
            flags: JOINED_FLAGS,
            team: TEAM,
            list_version: expedition.version,
            eligible,
        };
        self.change_game_mode_state(slot, lists);
        let mut joined = match_joined_state(entry, server_ms);
        joined.mutation_context = Some(mutation_context);
        self.put_participant_state(entity, joined);
    }

    /// Player `entity` leaves this sim's expedition (its exit): it goes from the lists and its
    /// participant state names no mode (Observed: Starstone 913.618 s).
    pub(super) fn leave_expedition_mode(&mut self, entity: Entity) {
        if let Some(version) = self.drop_expedition_participant(entity) {
            self.put_participant_state(entity, left_state(version));
        }
    }

    /// A participant of this sim's expedition leaves the sim to another place (a handoff): it
    /// goes from the lists. An offline participant stays.
    pub(super) fn expedition_left_sim(&mut self, entity: Entity) {
        self.drop_expedition_participant(entity);
    }

    /// The connection of participant `entity` ended (a logout or a lost connection): it goes
    /// offline, its place kept with the bit 2 off, until its character comes again (module
    /// text).
    pub(super) fn expedition_went_offline(&mut self, entity: Entity) {
        let Some(expedition) = self.expedition_mut() else {
            return;
        };
        let Some((index, character, status)) = expedition
            .participants
            .iter()
            .enumerate()
            .find(|(_, participant)| participant.entity == entity)
            .map(|(index, participant)| (index, participant.character, participant.status))
        else {
            return;
        };
        if expedition.offline.contains(&character) {
            return;
        }
        expedition.offline.push(character);
        let offline = status & !ACTIVE_BIT;
        expedition.participants[index].status = offline;
        let version = expedition.next_version();
        let slot = expedition.entity.0;
        self.change_game_mode_state(
            slot,
            GameModeState {
                statuses: Some(ReplicatedList::Changes(vec![ListChange {
                    // A run holds a handful of players.
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                    version,
                    value: offline,
                }])),
                ..GameModeState::default()
            },
        );
    }

    /// Takes player `entity` out of the expedition's lists, unless it is offline; the new list
    /// version.
    fn drop_expedition_participant(&mut self, entity: Entity) -> Option<u64> {
        let expedition = self.expedition_mut()?;
        let index = expedition.participants.iter().position(|participant| {
            participant.entity == entity && !expedition.offline.contains(&participant.character)
        })?;
        expedition.participants.remove(index);
        let lists = expedition.lists();
        let (version, slot) = (expedition.version, expedition.entity.0);
        self.change_game_mode_state(slot, lists);
        Some(version)
    }

    /// One tick of this sim's expedition: the participants' statuses and what they count, the
    /// run's clock, the `TimeLimit` timer, the task after the end, and the curse meter (module
    /// text).
    pub(super) fn tick_expedition_mode(&mut self) {
        if self.expedition().is_none() {
            return;
        }
        let died = self.sync_expedition_statuses();
        for _ in 0..died {
            self.score_run(DEATH_POINTS, |score| score.deaths += 1);
        }
        self.count_team_wipe();
        self.run_clock();
        self.end_area_tasks();
        self.end_time_limit();
        self.victory_step();
        self.tick_run_curse();
        self.tick_curse_chain();
    }

    /// Sets each participant's status ([`Self::participant_status`]); the count of participants
    /// that died now.
    fn sync_expedition_statuses(&mut self) -> usize {
        let Some(participants) = self
            .expedition()
            .map(|expedition| expedition.participants.clone())
        else {
            return 0;
        };
        let combat = self.curse_counts_combat();
        let offline = self
            .expedition()
            .map(|expedition| expedition.offline.clone())
            .unwrap_or_default();
        let now: Vec<u8> = participants
            .iter()
            .map(|participant| {
                if offline.contains(&participant.character) {
                    participant.status
                } else {
                    self.participant_status(participant.entity, combat)
                }
            })
            .collect();
        let Some(expedition) = self.expedition_mut() else {
            return 0;
        };
        let changed: Vec<(usize, u8)> = expedition
            .participants
            .iter()
            .zip(&now)
            .enumerate()
            .filter(|(_, (participant, status))| participant.status != **status)
            .map(|(index, (_, status))| (index, *status))
            .collect();
        if changed.is_empty() {
            return 0;
        }
        let died = changed
            .iter()
            .filter(|(index, status)| {
                status & DEAD_BIT != 0 && expedition.participants[*index].status & DEAD_BIT == 0
            })
            .count();
        let version = expedition.next_version();
        let mut changes = Vec::new();
        for (order, (index, status)) in changed.into_iter().enumerate() {
            expedition.participants[index].status = status;
            changes.push(ListChange {
                // A run holds a handful of players.
                index: u32::try_from(index).unwrap_or(u32::MAX),
                // Retail: the first change carries the version, the others 0.
                version: if order == 0 { version } else { 0 },
                value: status,
            });
        }
        let slot = expedition.entity.0;
        self.change_game_mode_state(
            slot,
            GameModeState {
                statuses: Some(ReplicatedList::Changes(changes)),
                ..GameModeState::default()
            },
        );
        died
    }

    /// The scored run's points change by `points` and `count` counts it (module text); the
    /// changed values go out. Nothing in a run whose script keeps no score or that ended.
    fn score_run(&mut self, points: i64, count: impl FnOnce(&mut RunScore)) {
        let Some(expedition) = self
            .expedition_mut()
            .filter(|expedition| expedition.script.is_some_and(|script| script.scored))
            .filter(|expedition| !expedition.score.ended)
        else {
            return;
        };
        expedition.score.points = expedition.score.points.saturating_add(points);
        count(&mut expedition.score);
        let kills_for_bonus = expedition
            .script
            .map_or(u64::MAX, |script| script.kills_for_bonus);
        let score = &expedition.score;
        let mut changes = expedition.score_values().to_vec();
        changes.extend([
            (DEATHS, score.deaths),
            (TEAM_WIPES, score.team_wipes),
            (NAMED_ENEMIES_KILLED, score.named),
            (NORMAL_KILLS, score.kills),
            (MULTIPLIER, score.multiplier),
        ]);
        if score.deaths > DEATHS_FOR_BONUS {
            changes.push((DEATH_BONUS, BONUS_LOST));
        }
        if score.team_wipes > 0 {
            changes.push((WIPE_BONUS, BONUS_LOST));
        }
        if score.named >= NAMED_FOR_BONUS {
            changes.push((NAMED_BONUS, BONUS_MET));
        }
        if score.kills >= kills_for_bonus {
            changes.push((KILL_BONUS, BONUS_MET));
        }
        expedition.set_values(&changes);
        self.send_expedition_values();
    }

    /// Sends the expedition's values at a new version.
    fn send_expedition_values(&mut self) {
        let Some(expedition) = self.expedition_mut() else {
            return;
        };
        let version = expedition.next_version();
        let change = GameModeState {
            values: Some(full(version, expedition.values.clone())),
            ..GameModeState::default()
        };
        let slot = expedition.entity.0;
        self.change_game_mode_state(slot, change);
    }

    /// A team wipe counts when every participant is dead or at death's door, once until one of
    /// them stands again; the first one costs the multiplier its wipe bonus (module text).
    fn count_team_wipe(&mut self) {
        let Some(expedition) = self.expedition_mut() else {
            return;
        };
        let down = !expedition.participants.is_empty()
            && expedition
                .participants
                .iter()
                .all(|participant| participant.status & (DEAD_BIT | DEATHS_DOOR_BIT) != 0);
        if !down {
            expedition.score.wiped = false;
            return;
        }
        if expedition.score.wiped {
            return;
        }
        expedition.score.wiped = true;
        self.score_run(TEAM_WIPE_POINTS, |score| {
            if score.team_wipes == 0 {
                score.multiplier = score.multiplier.saturating_sub(1);
            }
            score.team_wipes += 1;
        });
    }

    /// Whether an online participant of this sim's expedition stands in `area`.
    fn participant_in(&self, area: &Detector) -> bool {
        let Some(expedition) = self.expedition() else {
            return false;
        };
        expedition
            .participants
            .iter()
            .filter(|participant| !expedition.offline.contains(&participant.character))
            .filter_map(|participant| self.world.get::<Body>(participant.entity))
            .map(|body| body.motion.position())
            .any(|at| area.contains([at.x, at.y, at.z]))
    }

    /// Whether a participant stands in the level's start area.
    fn participant_in_start_area(&self) -> bool {
        self.expedition()
            .and_then(|expedition| expedition.start_area.as_ref())
            .is_some_and(|area| self.participant_in(area))
    }

    /// A task of a trigger area that is not done ends, with the tasks before it, when a
    /// participant stands in its area (module text).
    fn end_area_tasks(&mut self) {
        let Some(expedition) = self
            .expedition()
            .filter(|expedition| !expedition.score.ended)
        else {
            return;
        };
        let done = expedition.done;
        let reached = expedition
            .task_areas
            .iter()
            .filter(|(task, area)| task.done >= done && self.participant_in(area))
            .map(|(task, _)| task.done)
            .max();
        if let Some(waiting) = reached {
            self.advance_expedition_tasks(waiting.saturating_add(1));
        }
    }

    /// The run's clock: it starts when the first participant enters the level's start area
    /// (module text), and the run past its time to beat loses the time bonus of the multiplier.
    fn run_clock(&mut self) {
        let unix_s = self.unix_s();
        let entered = self.participant_in_start_area();
        let Some(expedition) = self
            .expedition_mut()
            .filter(|expedition| expedition.script.is_some_and(|script| script.scored))
            .filter(|expedition| !expedition.score.ended)
        else {
            return;
        };
        let Some(start_s) = expedition.score.start_s else {
            if entered {
                expedition.score.start_s = Some(unix_s);
                expedition.set_values(&[(START_TIME, unix_s)]);
                self.send_expedition_values();
            }
            return;
        };
        let late = unix_s.saturating_sub(start_s) > time_to_beat_s(expedition.script);
        if late && expedition.value(TIME_BONUS) == BONUS_MET {
            expedition.score.multiplier = expedition.score.multiplier.saturating_sub(1);
            let multiplier = expedition.score.multiplier;
            expedition.set_values(&[(TIME_BONUS, BONUS_LOST), (MULTIPLIER, multiplier)]);
            self.send_expedition_values();
        }
    }

    /// Mob `mob` died by a player's hit: a mutated run scores it ([`Self::score_expedition_kill`]).
    pub(super) fn expedition_mob_killed(&mut self, mob: u64) {
        let named = self
            .mobs
            .vitals_of(mob)
            .and_then(|vitals| self.content.kill_rewards.creature_type(vitals))
            .is_some_and(named_creature);
        self.score_expedition_kill(named);
    }

    /// A mutated run's kill, `named` or normal, counts; it adds its points while the count is
    /// at most its kills for bonus, and the kill that reaches that count sets its bonus and the
    /// multiplier +1 (module text). Nothing in a normal run.
    fn score_expedition_kill(&mut self, named: bool) {
        if self.instance_mutation().is_none() {
            return;
        }
        let Some((counted, points, for_bonus)) = self.expedition().and_then(|expedition| {
            let script = expedition.script?;
            Some(if named {
                (expedition.score.named, NAMED_KILL_POINTS, NAMED_FOR_BONUS)
            } else {
                (expedition.score.kills, script.kill_points, script.kills_for_bonus)
            })
        }) else {
            return;
        };
        let count = counted.saturating_add(1);
        let scored = if count <= for_bonus { points } else { 0 };
        self.score_run(i64::try_from(scored).unwrap_or(i64::MAX), |score| {
            if named {
                score.named = count;
            } else {
                score.kills = count;
            }
            if count == for_bonus {
                score.multiplier = score.multiplier.saturating_add(1);
            }
        });
    }

    /// The expedition's tasks up to `done` end: the script state and the objective's task list
    /// change (module text).
    fn advance_expedition_tasks(&mut self, done: u8) {
        let normal = self.instance_mutation().is_none();
        let Some(expedition) = self
            .expedition_mut()
            .filter(|expedition| expedition.script.is_some())
        else {
            return;
        };
        let done = if normal {
            expedition.past_mutation_tasks(done)
        } else {
            done
        };
        if done <= expedition.done {
            return;
        }
        expedition.tasks_done(done);
        let version = expedition.next_version();
        let tasks = expedition.tasks.clone();
        let slot = expedition.entity.0;
        self.change_game_mode_state(
            slot,
            GameModeState {
                script_state: Some(done),
                ..GameModeState::default()
            },
        );
        self.change_game_mode_objectives(
            slot,
            ObjectivesState {
                tasks: Some(full(version, tasks)),
                ..ObjectivesState::default()
            },
        );
    }

    /// Every encounter of the run ended (the dungeon script's completion): the tasks up to the
    /// last boss end, the end time and the final score go out, and the task after it waits for
    /// the expedition's delay (module text).
    pub(super) fn complete_expedition_run(&mut self, now_ms: u64) {
        let unix_s = self.unix_s();
        let Some(script) = self
            .expedition()
            .filter(|expedition| !expedition.score.ended)
            .and_then(|expedition| expedition.script)
        else {
            return;
        };
        self.advance_expedition_tasks(script.end_done);
        let Some(expedition) = self.expedition_mut() else {
            return;
        };
        expedition.victory_due_ms = script
            .victory_delay_ms
            .map(|delay_ms| now_ms.saturating_add(delay_ms));
        if !script.scored {
            expedition.score.ended = true;
            return;
        }
        let start_s = *expedition.score.start_s.get_or_insert(unix_s);
        let run_s = i64::try_from(unix_s.saturating_sub(start_s)).unwrap_or(i64::MAX);
        let to_beat = i64::try_from(script.time_to_beat_s).unwrap_or(i64::MAX);
        let score = &mut expedition.score;
        score.points = score.points.saturating_add(
            POINTS_PER_SECOND_UNDER.saturating_mul(to_beat.saturating_sub(run_s)),
        );
        score.ended = true;
        let multiplier = i64::try_from(score.multiplier).unwrap_or(i64::MAX);
        let final_score = score.points.saturating_mul(multiplier);
        let mut changes = vec![
            (START_TIME, start_s),
            (END_TIME, unix_s),
            (FINAL_SCORE, final_score.unsigned_abs()),
            (MUTATOR_RANK, rank_of(final_score)),
        ];
        changes.extend(expedition.score_values());
        expedition.set_values(&changes);
        self.send_expedition_values();
    }

    /// The task after the run's end ends at its delay.
    fn victory_step(&mut self) {
        let now_ms = self.server_ms;
        let Some(expedition) = self.expedition_mut() else {
            return;
        };
        if expedition.victory_due_ms.is_none_or(|due_ms| now_ms < due_ms) {
            return;
        }
        expedition.victory_due_ms = None;
        let done = expedition.done.saturating_add(1);
        self.advance_expedition_tasks(done);
    }

    /// At the end of the `TimeLimit` timer the timers go again without it (module text).
    fn end_time_limit(&mut self) {
        let server_ms = self.server_ms;
        let Some(expedition) = self.expedition_mut() else {
            return;
        };
        if expedition
            .time_limit_ends_ms
            .is_none_or(|ends_ms| server_ms < ends_ms)
        {
            return;
        }
        expedition.time_limit_ends_ms = None;
        expedition.timers.retain(|(key, _)| *key != TIME_LIMIT);
        let version = expedition.next_version();
        let change = GameModeState {
            timers: Some(full(version, expedition.timers.clone())),
            ..GameModeState::default()
        };
        let slot = expedition.entity.0;
        self.change_game_mode_state(slot, change);
    }

    /// The run's curse meter starts with the rules `rule` (the first player's mutation,
    /// [`SimWorld::adopt_mutation`]); a curse whose kind the affliction rules lack has none.
    pub(super) fn start_run_curse(&mut self, rule: CurseMeter) {
        let kind = lowercase_crc32(&rule.kind);
        if self.content.afflictions.global_kind(kind).is_none() {
            tracing::warn!(kind = %rule.kind, "the curse's global affliction has no row");
            return;
        }
        let next_rise_ms = self.server_ms.saturating_add(RISE_PERIOD_MS);
        let Some(expedition) = self.expedition_mut() else {
            return;
        };
        if expedition.curse.is_some() {
            return;
        }
        expedition.curse = Some(RunCurse {
            minor: lowercase_crc32(&rule.minor),
            rule,
            kind,
            meters: AfflictionMeters::default(),
            next_rise_ms: Some(next_rise_ms),
            rise_ticks: 0,
            marked: None,
            chain: None,
        });
    }

    /// The minor curse of the run when player `entity` is one of its players: the holder of
    /// the `OnPerform` ability the run equips on them (module text).
    pub(super) fn run_minor_curse(&self, entity: Entity) -> Option<u32> {
        let expedition = self.expedition()?;
        let curse = expedition.curse.as_ref()?;
        expedition
            .participants
            .iter()
            .any(|participant| participant.entity == entity)
            .then_some(curse.minor)
    }

    /// A participant may take the rise: alive, not at death's door, in the game and, when the
    /// rise needs it, in combat (Observed: the curse script's tests of the status bits).
    fn rise_holder(&self, needs_combat: bool) -> bool {
        let wanted = LOADED_BIT | ACTIVE_BIT | if needs_combat { IN_COMBAT_BIT } else { 0 };
        self.expedition().is_some_and(|expedition| {
            expedition.participants.iter().any(|participant| {
                participant.status & wanted == wanted
                    && participant.status & (DEAD_BIT | DEATHS_DOOR_BIT) == 0
            })
        })
    }

    /// The curse meter's rise each second (module text), then its move on: a fall that ended
    /// acts.
    fn tick_run_curse(&mut self) {
        let content = std::sync::Arc::clone(&self.content);
        let rules = &content.afflictions;
        let now_ms = self.server_ms;
        let (needs_combat, escalates) = match self
            .expedition()
            .and_then(|expedition| expedition.curse.as_ref())
        {
            Some(curse) => (curse.rule.rise_needs_combat, curse.rule.rise_escalates),
            None => return,
        };
        let holder = self.rise_holder(needs_combat);
        let Some(curse) = self
            .expedition_mut()
            .and_then(|expedition| expedition.curse.as_mut())
        else {
            return;
        };
        let Some(kind) = rules.global_kind(curse.kind) else {
            return;
        };
        let mut rise = None;
        if curse.next_rise_ms.is_some_and(|due_ms| now_ms >= due_ms) {
            curse.next_rise_ms = Some(now_ms.saturating_add(RISE_PERIOD_MS));
            let times = if escalates {
                curse.rise_ticks / TICKS_PER_ESCALATION + 1
            } else {
                1
            };
            curse.rise_ticks = curse.rise_ticks.saturating_add(1);
            let per_time: f32 = curse
                .rule
                .rise
                .as_deref()
                .filter(|_| kind.timer)
                .and_then(|effect| rules.tick(lowercase_crc32(effect)))
                .map_or(0.0, |tick| {
                    tick.gains
                        .iter()
                        .filter(|gain| gain.kind == kind.crc)
                        .map(|gain| gain.amount)
                        .sum()
                });
            // A few gains a second.
            let amount = per_time * times as f32;
            if holder && amount != 0.0 {
                rise = Some(AfflictionGain {
                    kind: kind.crc,
                    amount,
                });
            }
        }
        let ended = curse.meters.advance(rules, now_ms);
        if let Some(gain) = rise {
            self.run_curse_gain(gain, now_ms);
        }
        if !ended.is_empty() {
            self.run_curse_fall_ended(now_ms);
        }
        self.send_run_curse();
    }

    /// The run's curse meter changes by `gain` at `at_ms` (a player's effect tick or the rise):
    /// a full meter's curse acts (module text).
    pub(super) fn run_curse_gain(&mut self, gain: AfflictionGain, at_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(kind) = content.afflictions.global_kind(gain.kind) else {
            return;
        };
        let Some(curse) = self
            .expedition_mut()
            .and_then(|expedition| expedition.curse.as_mut())
            .filter(|curse| curse.kind == kind.crc)
        else {
            return;
        };
        let change = curse.meters.change_global(kind, gain.amount, at_ms);
        let fill = curse.rule.fill.clone();
        if change == Change::Filled {
            curse.rise_ticks = 0;
            if curse.rule.rise_escalates {
                // The Fiendish timer stops until the fall ends (`FUN_1461f2370`).
                curse.next_rise_ms = None;
            }
            if let Some(fill) = fill {
                self.fill_run_curse(&fill);
            }
        }
        self.send_run_curse();
    }

    /// The full meter's fall ended (module text): the Fiendish timer starts again, the marked
    /// Desiccated player explodes and loses its mark.
    fn run_curse_fall_ended(&mut self, now_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(curse) = self
            .expedition_mut()
            .and_then(|expedition| expedition.curse.as_mut())
        else {
            return;
        };
        if curse.next_rise_ms.is_none() {
            curse.next_rise_ms = Some(now_ms.saturating_add(RISE_PERIOD_MS));
        }
        let Some(CurseFill::MarkLowestHealth {
            mark,
            explosion: Some(explosion),
            ..
        }) = curse.rule.fill.clone()
        else {
            return;
        };
        let Some(marked) = curse.marked.take() else {
            return;
        };
        let Some(from) = self
            .world
            .get::<Body>(marked)
            .map(|body| body.motion.position())
        else {
            return;
        };
        let server_ms = self.server_ms;
        let Some(number) = self.world.get_mut::<Player>(marked).map(|mut player| {
            let number = player.number;
            player.status_effects.remove(lowercase_crc32(&mark));
            player.status_effects.add_by_id(
                &content.status_effects,
                &explosion.cast,
                CURSE_POTENCY,
                number,
                server_ms,
            );
            number
        }) else {
            return;
        };
        if let Some(curse) = self
            .expedition_mut()
            .and_then(|expedition| expedition.curse.as_mut())
        {
            curse.chain = Some(CurseChain {
                jumps_left: explosion.jumps,
                explosion,
                hit: vec![number],
                from,
                next_ms: now_ms,
            });
        }
    }

    /// The explosion's chain jumps when due: to the nearest living player or mob within the
    /// spell's radius that it did not hit yet, which gets the spell's effects (module text).
    fn tick_curse_chain(&mut self) {
        let now_ms = self.server_ms;
        let Some(chain) = self
            .expedition()
            .and_then(|expedition| expedition.curse.as_ref())
            .and_then(|curse| curse.chain.clone())
            .filter(|chain| now_ms >= chain.next_ms)
        else {
            return;
        };
        let radius_m = chain.explosion.radius_m;
        let mut targets: Vec<(u64, Position)> = Vec::new();
        {
            let mut query = self.world.query::<(&Player, &Body, &Vitals)>();
            for (player, body, vitals) in query.iter(&self.world) {
                if !vitals.fallen() {
                    targets.push((player.number, body.motion.position()));
                }
            }
        }
        targets.extend(self.mobs.live_entities().into_iter().filter_map(|entity| {
            let mob = self.mobs.get(entity)?;
            (!mob.health.is_dead()).then_some((entity, mob.position))
        }));
        let next = targets
            .into_iter()
            .filter(|(number, _)| !chain.hit.contains(number))
            .map(|(number, at)| (number, at, reach_m(at, chain.from)))
            .filter(|(_, _, distance)| *distance <= radius_m)
            .min_by(|one, other| one.2.total_cmp(&other.2));
        let Some(curse) = self
            .expedition_mut()
            .and_then(|expedition| expedition.curse.as_mut())
        else {
            return;
        };
        let Some((number, at, _)) = next.filter(|_| chain.jumps_left > 0) else {
            curse.chain = None;
            return;
        };
        let mut chain = chain;
        chain.hit.push(number);
        chain.from = at;
        chain.jumps_left -= 1;
        chain.next_ms = now_ms.saturating_add(chain.explosion.delay_ms);
        let effects: Vec<(u64, AppliedEffect)> = chain
            .explosion
            .effects
            .iter()
            .map(|status_id| {
                (
                    number,
                    AppliedEffect {
                        status_id: status_id.clone(),
                        potency: CURSE_POTENCY,
                    },
                )
            })
            .collect();
        curse.chain = (chain.jumps_left > 0).then_some(chain);
        self.queue_mutation_effects(effects);
    }

    /// Sends the curse meter's data when it changed since the last send.
    fn send_run_curse(&mut self) {
        let content = std::sync::Arc::clone(&self.content);
        let now_ms = self.server_ms;
        let Some(expedition) = self.expedition_mut() else {
            return;
        };
        let slot = expedition.entity.0;
        let Some(curse) = expedition.curse.as_mut() else {
            return;
        };
        let Some(kind) = content.afflictions.global_kind(curse.kind) else {
            return;
        };
        if !curse.meters.take_changed() {
            return;
        }
        let data = curse.meters.global_data(kind, now_ms);
        self.change_game_mode_state(
            slot,
            GameModeState {
                affliction: Some(data),
                ..GameModeState::default()
            },
        );
    }

    /// The full curse meter acts: `fill` on the living run players (module text).
    fn fill_run_curse(&mut self, fill: &CurseFill) {
        let living: Vec<(Entity, u64, Position, f64)> = self
            .expedition()
            .map(|expedition| expedition.participants.clone())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|participant| {
                let vitals = self.world.get::<Vitals>(participant.entity)?;
                if vitals.fallen() {
                    return None;
                }
                let share =
                    f64::from(vitals.health.current()) / f64::from(vitals.health.max().max(1));
                let at = self
                    .world
                    .get::<Body>(participant.entity)?
                    .motion
                    .position();
                Some((participant.entity, participant.number, at, share))
            })
            .collect();
        match fill {
            CurseFill::CastOnEach { spell, lasting_ms } => {
                let Some(row) = self
                    .combat
                    .data
                    .mob_spell(spell)
                    .map(|spell| spell.row.clone())
                else {
                    tracing::warn!(spell = %spell, "the curse's spell has no row");
                    return;
                };
                let now_ms = self.last_now_ms;
                for (_, number, at, _) in living {
                    self.open_spell_area(SpellArea::of_game_mode(
                        number,
                        &row,
                        at,
                        now_ms,
                        *lasting_ms,
                    ));
                }
            }
            CurseFill::MarkLowestHealth { mark, weaken, .. } => {
                let content = std::sync::Arc::clone(&self.content);
                let server_ms = self.server_ms;
                let marked = living
                    .iter()
                    .min_by(|one, other| one.3.total_cmp(&other.3))
                    .map(|(entity, number, _, _)| (*entity, *number));
                let Some((entity, number)) = marked else {
                    return;
                };
                if let Some(mut player) = self.world.get_mut::<Player>(entity) {
                    player.status_effects.add_by_id(
                        &content.status_effects,
                        mark,
                        CURSE_POTENCY,
                        number,
                        server_ms,
                    );
                }
                if let Some(weaken) = weaken {
                    for (entity, number, _, _) in &living {
                        if let Some(mut player) = self.world.get_mut::<Player>(*entity) {
                            player.status_effects.add_by_id(
                                &content.status_effects,
                                weaken,
                                CURSE_POTENCY,
                                *number,
                                server_ms,
                            );
                        }
                    }
                }
                if let Some(curse) = self
                    .expedition_mut()
                    .and_then(|expedition| expedition.curse.as_mut())
                {
                    curse.marked = Some(entity);
                }
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use nw_bus::{InstanceOpened, InstanceStay, StoredPoint};
    use nw_content::afflictions::AfflictionRules;
    use nw_content::instances::{InstanceCatalog, InstanceMode};
    use nw_content::status_effects::StatusEffectRules;
    use nw_messages::GlobalAffliction;
    use nw_messages::game_mode_participant::GameModeParticipantState;
    use nw_messages::{LevelSpec, MemberState};
    use uuid::Uuid;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::game_mode_entity::PLAYING_STATUS;
    use crate::world::afflictions::tests::effect_row;
    use crate::world::catacombs::tests::held_state;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, settings, slot};

    const PLAYER: u64 = 61;
    const OTHER: u64 = 62;
    const INSTANCE: u128 = 0x14;
    const PHASE: u64 = 0x5e;
    const STARSTONE: &str = "DungeonShatteredObelisk";
    const CURSE2: &str = "AfflictionCurse2";
    const RISE: &str = "Mut_Curse2_Affliction";
    const TICK: &str = "Mut_Curse2_Affliction_Tick";
    const MINOR: &str = "Mut_Curse2_Minor_Buff";
    const WEARY_SPELL: &str = "Mut_Curse2_Minor";
    const WEARY: &str = "Mut_Curse2_Minor_SlowRend";
    const BLOOD_BUFF: &str = "Mut_Curse2_Fir_Major_Buff";
    const BLOOD_OFFERING: &str = "Mut_Curse2_Fir_Major_Damage";
    const MORTAR_FIRED: &str = "Blunderbuss_MortarShotFired";
    const EXHAUST_BUFF: &str = "Mut_Curse4_Major_Buff";
    const EXHAUST: &str = "Mut_Curse4_Major_Exhaust";
    /// `AfflictionCurse2`: `IntID` 11, `Max` 45, `AfflictedDrainTime` 8 s, `DrainDelay` 9,999 s.
    const CURSE_ID: u8 = 11;
    /// The status of a dead participant (Observed: Starstone 178.243 s, Ennead 179.702 s).
    const DEAD_STATUS: u8 = LOADED_BIT | ACTIVE_BIT | DEAD_BIT;
    /// The `Max` cell of `AfflictionCurse2`, a whole number in the data.
    const CURSE_MAX_CELL: i64 = 45;
    const CURSE_MAX: f32 = CURSE_MAX_CELL as f32;
    const HOLD_MS: u64 = 9_999_000;
    const NS_PER_MS: u64 = 1_000_000;

    /// Starstone with its CRC and one teleport.
    fn starstone() -> InstanceMode {
        let mut mode = crate::world::instances::tests::mode(
            STARSTONE,
            "NW_Dungeon_Everfall_00",
            InstanceKind::Expedition,
        );
        mode.crc = lowercase_crc32(STARSTONE);
        mode
    }

    /// The rows of build 25810808 the curse reads, cut to their cells: `AfflictionCurse2`, the
    /// Fiendish effects and abilities, the majors of Blood Offering and Exhaust.
    fn content() -> PlayerContent {
        let affliction: nw_content::generated::AfflictionData =
            serde_json::from_value(serde_json::json!({
                "affliction_id": CURSE2, "int_id": CURSE_ID, "max": CURSE_MAX_CELL,
                "afflicted_drain_time": 8.0, "drain_delay": 9999.0, "drain_rate": 9999.0,
                "ignore_when_afflicted": true, "is_global_affliction": true,
                "is_timer_affliction": true, "afflicted_tooltip_text": "",
                "affliction_tooltip_text": "", "color_hex": "", "filling_color_hex": "",
                "icon": "", "localized_text": "", "localized_text_afflicted": ""
            }))
            .expect("affliction row");
        let equipping = |id: &str, ability: &str| {
            let mut row = effect_row(id, -1.0);
            row.equip_ability = Some(ability.to_owned());
            row
        };
        let mut rise = effect_row(RISE, 0.0);
        rise.afflictions = Some(format!("{CURSE2}=1"));
        let mut tick = effect_row(TICK, 0.9);
        tick.afflictions = Some(format!("{CURSE2}=-1"));
        tick.tick_rate = Some(1.0);
        let effects = vec![
            rise,
            tick,
            equipping(MINOR, "Mut_Curse2_Minor"),
            equipping(BLOOD_BUFF, "Mut_Curse2_Fir_Major"),
            equipping(EXHAUST_BUFF, "Mut_Curse4_Major"),
            effect_row(BLOOD_OFFERING, 3.0),
            effect_row(EXHAUST, 3.0),
            effect_row(MORTAR_FIRED, 5.0),
            effect_row(WEARY, 8.0),
        ];
        let ability = |value: serde_json::Value| -> nw_content::generated::AbilityData {
            serde_json::from_value(value).expect("ability row")
        };
        let abilities = vec![
            ability(serde_json::json!({
                "ability_id": "Mut_Curse2_Minor", "ability_trigger": "OnPerform",
                "self_apply_status_effect": TICK
            })),
            ability(serde_json::json!({
                "ability_id": "Mut_Curse2_Fir_Major", "ability_trigger": "OnPerform",
                "dont_have_status_effect": MORTAR_FIRED,
                "self_apply_status_effect": BLOOD_OFFERING
            })),
            ability(serde_json::json!({
                "ability_id": "Mut_Curse4_Major", "ability_trigger": "OnPerform",
                "self_apply_status_effect": EXHAUST
            })),
        ];
        let curse = |id: &str, minor: &str, major: &str| {
            serde_json::from_value::<nw_content::generated::CurseMutationStaticData>(
                serde_json::json!({
                    "curse_major": major, "curse_minor": minor, "curse_mutation_id": id,
                    "description": "", "global_affliction": null, "icon_path": "", "name": ""
                }),
            )
            .expect("curse row")
        };
        let tables = nw_content::Tables {
            affliction_data: table(vec![affliction]),
            status_effect_data: table(effects),
            ability_data: table(abilities),
            curse_mutation_static_data: table(vec![
                curse("Fiendish", MINOR, BLOOD_BUFF),
                curse("Censored", MINOR, EXHAUST_BUFF),
            ]),
            ..nw_content::Tables::default()
        };
        PlayerContent {
            instances: InstanceCatalog::build([starstone()]),
            afflictions: AfflictionRules::of_tables(&tables),
            status_effects: StatusEffectRules::from_tables(&tables),
            ..PlayerContent::default()
        }
    }

    fn table<T>(rows: Vec<T>) -> Vec<nw_content::table::Table<T>> {
        vec![nw_content::table::Table {
            source_path: String::new(),
            unique_id: String::new(),
            rows,
        }]
    }

    /// The Fiendish meter's rules (Observed: the Weary zone lasts the cast effect's 10 s).
    fn fiendish() -> CurseMeter {
        CurseMeter {
            kind: CURSE2.to_owned(),
            minor: MINOR.to_owned(),
            rise: Some(RISE.to_owned()),
            rise_needs_combat: true,
            rise_escalates: true,
            fill: Some(CurseFill::CastOnEach {
                spell: WEARY_SPELL.to_owned(),
                lasting_ms: 10_000,
            }),
        }
    }

    /// An instance sim that opened Starstone, with the Weary spell row.
    fn opened() -> SimWorld {
        opened_with(starstone(), content())
    }

    /// A mutable expedition of [`nw_content::mutations::test_catalog`], Fiendish in its last
    /// observed week (Barnacles and Black Powder).
    const CUTLASS_KEYS: &str = "DungeonCutlassKeys00";

    /// The one mutation difficulty of [`nw_content::mutations::test_catalog`]: an arrival at a
    /// difficulty the catalog lacks brings no mutation.
    const CATALOG_DIFFICULTY: u8 = 3;

    /// An instance sim that opened Barnacles and Black Powder with the retail mutation catalog cut
    /// for the tests ([`nw_content::mutations::test_catalog::catalog`]), for a mutated run.
    pub(in crate::world) fn opened_mutated() -> SimWorld {
        let mut mode = crate::world::instances::tests::mode(
            CUTLASS_KEYS,
            "NW_Dungeon_CutlassKeys_00",
            InstanceKind::Expedition,
        );
        mode.crc = lowercase_crc32(CUTLASS_KEYS);
        mode.mutable = true;
        let content = PlayerContent {
            instances: InstanceCatalog::build([mode.clone()]),
            mutations: nw_content::mutations::test_catalog::catalog(),
            ..content()
        };
        opened_with(mode, content)
    }

    /// Player `number` arrives in the run of [`opened_mutated`] at mutation difficulty
    /// `difficulty`, activated inside the catalog's last observed week.
    pub(in crate::world) fn arrive_mutated(
        world: &mut SimWorld,
        number: u64,
        difficulty: u8,
    ) -> Entity {
        const NS_PER_SECOND: u64 = 1_000_000_000;
        arrive_in(
            world,
            number,
            InstanceStay {
                game_mode: CUTLASS_KEYS.to_owned(),
                activated_ns: nw_content::mutations::test_catalog::LAST_OBSERVED_DAY
                    * NS_PER_SECOND,
                mutation_difficulty: Some(difficulty),
                ..stay()
            },
        )
    }

    /// An instance sim of the instance role on `mode`'s level that opened `mode` with `content`
    /// and the Weary spell row.
    fn opened_with(mode: InstanceMode, content: PlayerContent) -> SimWorld {
        let mut settings = settings(4, 4);
        settings.instance_role = true;
        settings.level = LevelSpec::instance(
            &mode.map.folder,
            &mode.map.map_name,
            &mode.map.coatlicue_path,
            mode.map.frame,
        );
        let mut source = crate::combat::test_support::test_source();
        source.mob_spells.push(nw_content::combat::MobSpell {
            row: nw_content::player_abilities::SpellRow {
                id: WEARY_SPELL.to_owned(),
                types: None,
                shape: Some("Sphere".to_owned()),
                radius_m: Some(4.0),
                cast_distance_m: None,
                duration_ms: Some(-1),
                damage_row: None,
                status_effects: vec![WEARY.to_owned()],
                target_types: Some("All".to_owned()),
                mana_cost: None,
                projectile: None,
                clear_on_exit: true,
                clear_on_end: true,
                secondary: None,
            },
            flies: false,
        });
        let mut combat = crate::combat::tests::test_combat();
        combat.data = std::sync::Arc::new(
            nw_content::combat::CombatData::build(
                source,
                &nw_content::combat::CombatRows::default(),
            )
            .expect("data")
            .with_player_attacks(crate::combat::test_support::test_player_attacks()),
        );
        let mut world = SimWorld::new(
            settings,
            crate::login::WorldIds::random(0).expect("ids"),
            std::sync::Arc::new(nw_sim_core::test_support::NoGround),
            slot("inst-1"),
            combat,
            std::sync::Arc::new(content),
        );
        assert_eq!(
            world.open_instance(
                Uuid::from_u128(INSTANCE),
                &mode.id,
                PHASE,
                Vec::new(),
                None,
                NOW
            ),
            InstanceOpened::Opened
        );
        world
    }

    /// The stay of a normal Starstone run.
    fn stay() -> InstanceStay {
        InstanceStay {
            instance: Uuid::from_u128(INSTANCE),
            game_mode: STARSTONE.to_owned(),
            phase: PHASE,
            return_level: "newworld_vitaeeterna".to_owned(),
            return_position: StoredPoint::default(),
            activated_ns: 1,
            mutation_difficulty: None,
            seat: None,
        }
    }

    /// Player `number` arrives in the expedition of [`opened`].
    fn arrive(world: &mut SimWorld, number: u64) -> Entity {
        arrive_in(world, number, stay())
    }

    /// Player `number` arrives in the opened instance with `stay`.
    fn arrive_in(world: &mut SimWorld, number: u64, stay: InstanceStay) -> Entity {
        let mut admit = admit_at(number, 1.0, false);
        admit.progression.arena.stay = Some(stay);
        world.add_instance_member(Uuid::from_u128(INSTANCE), admit.character);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        world.player_by_number(number).expect("player")
    }

    fn mode_entity(world: &SimWorld) -> u64 {
        world.expedition().expect("expedition").entity()
    }

    fn participant_states(world: &mut SimWorld, entity: Entity) -> Vec<GameModeParticipantState> {
        let Some(mut player) = world.world.get_mut::<Player>(entity) else {
            return Vec::new();
        };
        std::mem::take(&mut player.extra_members)
            .into_iter()
            .filter_map(|member| match member.state {
                MemberState::GameModeParticipant(state) => Some(state),
                _ => None,
            })
            .collect()
    }

    fn statuses(world: &SimWorld) -> Vec<u8> {
        held_state(world, mode_entity(world))
            .statuses
            .as_ref()
            .and_then(ReplicatedList::full)
            .map(|list| list.elements.clone())
            .unwrap_or_default()
    }

    fn curse_data(world: &SimWorld) -> Option<GlobalAffliction> {
        held_state(world, mode_entity(world)).affliction
    }

    /// The held objectives of the game mode entity.
    fn objectives_of(world: &SimWorld) -> ObjectivesState {
        world
            .stations
            .members(mode_entity(world))
            .expect("entity")
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Objectives(state) if member.component == OBJECTIVES_COMPONENT => {
                    Some(state.clone())
                }
                _ => None,
            })
            .expect("the objectives")
    }

    /// The held value `key` of the game mode state.
    fn value_of(world: &SimWorld, key: u32) -> u64 {
        held_state(world, mode_entity(world))
            .values
            .expect("values")
            .elements
            .iter()
            .find(|(held, _)| *held == key)
            .map_or(0, |(_, value)| *value)
    }

    /// The held script state and task entries (index, task, progress, state).
    fn tasks_of(world: &SimWorld) -> (u8, Vec<(u8, u32, u32, TaskWireState)>) {
        let state = held_state(world, mode_entity(world)).script_state.expect("state");
        let tasks = objectives_of(world)
            .tasks
            .expect("tasks")
            .elements
            .iter()
            .map(|task| (task.index, task.task, task.progress, task.state))
            .collect();
        (state, tasks)
    }

    /// Kills (`health` 0) or raises player `entity`.
    fn set_dead(world: &mut SimWorld, entity: Entity, dead: bool) {
        let mut vitals = world.world.get_mut::<Vitals>(entity).expect("vitals");
        if dead {
            let max = vitals.health.max();
            vitals.health = nw_sim_core::Health::at(0, max);
        } else {
            vitals.health.refill();
        }
    }

    fn in_combat(world: &mut SimWorld, entity: Entity) {
        let now = world.last_now_ms;
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .combat
            .hit(now, false);
    }

    fn holds(world: &SimWorld, entity: Entity, id: &str) -> bool {
        world
            .world
            .get::<Player>(entity)
            .is_some_and(|player| player.status_effects.holds(lowercase_crc32(id)))
    }

    fn hold(world: &mut SimWorld, entity: Entity, id: &str) {
        let content = std::sync::Arc::clone(&world.content);
        let now = world.server_ms;
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .status_effects
            .add_by_id(&content.status_effects, id, CURSE_POTENCY, PLAYER, now);
    }

    /// Retail Starstone 7.282 s: the opening places the card with no asset and the first state
    /// (state 0, the mode's CRC twice, empty lists, the 19 values with the time to beat 2400,
    /// the timeout 3 h and the time limit 600 s after the opening, no affliction, linked 0).
    #[test]
    fn an_opened_expedition_places_its_game_mode_entity_with_the_retail_first_state() {
        let world = opened();
        let entity = mode_entity(&world);
        let members = world.stations.members(entity).expect("entity");
        assert!(matches!(
            &members[0].state,
            MemberState::GdeMetadata(card) if card.asset.is_none() && card.gde_ref.is_some()
        ));
        let state = held_state(&world, entity);
        let crc = lowercase_crc32(STARSTONE);
        assert_eq!(state.script_state, Some(FIRST_SCRIPT_STATE));
        assert_eq!((state.mode, state.map), (Some(crc), Some(crc)));
        assert_eq!(state.linked_mode, Some(LINKED_MODE));
        assert!(state.no_affliction && state.affliction.is_none());
        let values = state.values.expect("values").elements;
        assert_eq!(values.len(), ENTRY_VALUES.len());
        assert_eq!(values[0], (TIME_TO_BEAT_TWIN, 2_400));
        assert_eq!(values[11], (TIME_TO_BEAT, 2_400));
        assert_eq!(values[12], (0x9a03_db12, 3), "MutatorRank");
        let opened_ms = world.server_ms;
        assert_eq!(
            state.timers.expect("timers").elements,
            [
                (GAME_MODE_TIMEOUT, opened_ms + DUNGEON_TIMEOUT_MS),
                (TIME_LIMIT, opened_ms + TIME_LIMIT_MS),
            ]
        );
        assert_eq!(
            time_to_beat_s(expedition_script("DungeonShatterMtn00")),
            3_600
        );
        assert_eq!(
            time_to_beat_s(expedition_script("DungeonDunwood02")),
            DEFAULT_TIME_TO_BEAT_S,
            "no capture"
        );
        let objectives = objectives_of(&world);
        let record = &objectives.active.expect("active").elements[0];
        assert_eq!((record.objective, record.instance), (650_374_807, 3_812_856_275_804_359_371));
        assert_eq!(record.accept_time, unix_ns(opened_ms));
        assert!(record.poi && record.available);
        assert!(objectives.tracked.expect("tracked").elements.is_empty());
        assert_eq!(
            objectives.tasks.expect("tasks"),
            full(FIRST_LIST_VERSION, Vec::new())
        );
    }

    /// Retail Starstone: an arriving player joins the lists with status 3 and its participant
    /// state names the entity with `11 00 07`; a death sets 7, the respawn 3; the exit takes it
    /// out with the participant state of no mode.
    #[test]
    fn a_player_joins_dies_respawns_and_leaves_the_lists() {
        let mut world = opened();
        let entity = arrive(&mut world, PLAYER);
        assert_eq!(statuses(&world), [PLAYING_STATUS]);
        let joined = participant_states(&mut world, entity)
            .into_iter()
            .find(|state| state.active.is_some())
            .expect("the joined state");
        assert_eq!(joined.flags, Some(JOINED_FLAGS));
        let (_, gde_ref) = world.expedition().expect("expedition").entity;
        let active = joined.active.expect("active").elements;
        assert_eq!(active[0].raw_id, gde_ref);
        assert_eq!(active[0].phase, PHASE);
        let max = world
            .world
            .get::<Vitals>(entity)
            .expect("vitals")
            .health
            .max();
        world
            .world
            .get_mut::<Vitals>(entity)
            .expect("vitals")
            .health = nw_sim_core::Health::at(0, max);
        world.tick_expedition_mode();
        assert_eq!(statuses(&world), [DEAD_STATUS]);
        world
            .world
            .get_mut::<Vitals>(entity)
            .expect("vitals")
            .health
            .refill();
        world.tick_expedition_mode();
        assert_eq!(statuses(&world), [PLAYING_STATUS]);
        world.leave_expedition_mode(entity);
        assert!(statuses(&world).is_empty());
        let left = participant_states(&mut world, entity);
        assert!(
            left.iter()
                .any(|state| state.mutation_context == Some(MutationContextWire::NONE))
        );
    }

    /// Retail Starstone 606.379 s: at the end of the time limit the timers go again with the
    /// timeout alone.
    #[test]
    fn the_time_limit_goes_at_its_end() {
        let mut world = opened();
        let opened_ms = world.server_ms;
        world.server_ms = opened_ms + TIME_LIMIT_MS;
        world.tick_expedition_mode();
        let timers = held_state(&world, mode_entity(&world))
            .timers
            .expect("timers")
            .elements;
        assert_eq!(
            timers,
            [(GAME_MODE_TIMEOUT, opened_ms + DUNGEON_TIMEOUT_MS)]
        );
    }

    /// Client `FUN_1461f2730` (retail `20261002-184508-050`: +1 to +5 a second): each second
    /// the Fiendish meter gains its rise once while a run player is in combat (two in combat
    /// still once), tick / 10 + 1 times from the 11th tick, nothing with no player in combat;
    /// it holds its value for 9,999 s; an ability takes 1 off (`Mut_Curse2_Minor`'s
    /// `Mut_Curse2_Affliction_Tick`).
    #[test]
    fn the_curse_meter_rises_once_a_second_with_a_player_in_combat_and_escalates() {
        let mut world = opened();
        let first = arrive(&mut world, PLAYER);
        let second = arrive(&mut world, OTHER);
        world.start_run_curse(fiendish());
        let rise = |world: &mut SimWorld| {
            world.server_ms += RISE_PERIOD_MS;
            world.tick_expedition_mode();
        };
        rise(&mut world);
        assert_eq!(curse_data(&world), None, "no player in combat");
        in_combat(&mut world, first);
        in_combat(&mut world, second);
        rise(&mut world);
        assert_eq!(
            statuses(&world),
            [PLAYING_STATUS | IN_COMBAT_BIT; 2],
            "a Fiendish run shows the combat bit"
        );
        let line = |value: f32, at_ms: u64| {
            GlobalAffliction::of(
                nw_messages::MeterLine {
                    last: value,
                    last_ns: at_ms * NS_PER_MS,
                    target: value,
                    target_ns: (at_ms + HOLD_MS) * NS_PER_MS,
                },
                CURSE_MAX,
                CURSE_ID,
                false,
            )
        };
        assert_eq!(
            curse_data(&world),
            Some(line(1.0, world.server_ms)),
            "once for two players in combat"
        );
        for _ in 0..8 {
            in_combat(&mut world, first);
            rise(&mut world);
        }
        assert_eq!(
            curse_data(&world),
            Some(line(9.0, world.server_ms)),
            "ticks 2 to 9"
        );
        in_combat(&mut world, first);
        rise(&mut world);
        assert_eq!(
            curse_data(&world),
            Some(line(11.0, world.server_ms)),
            "the 11th tick gains twice"
        );
        world.ability_performed(first);
        assert!(holds(&world, first, TICK));
        assert_eq!(curse_data(&world), Some(line(10.0, world.server_ms)));
        assert!(
            world.run_minor_curse(first).is_some(),
            "a run player performs through the minor curse"
        );
    }

    /// The meter fills at its top: it falls over 8 s with the flag, and the Weary zone opens on
    /// each living run player for 10 s (Observed: 103 of 103 fills).
    #[test]
    fn a_full_curse_meter_casts_the_weary_zone_on_each_living_player() {
        let mut world = opened();
        let first = arrive(&mut world, PLAYER);
        arrive(&mut world, OTHER);
        world.start_run_curse(fiendish());
        world.run_curse_gain(
            AfflictionGain {
                kind: lowercase_crc32(CURSE2),
                amount: CURSE_MAX,
            },
            world.server_ms,
        );
        let data = curse_data(&world).expect("data");
        assert!(data.afflicted);
        assert_eq!(data.last_bits, CURSE_MAX.to_bits());
        assert_eq!(data.target_bits, 0.0_f32.to_bits());
        assert_eq!(data.target_ns - data.last_ns, 8_000 * NS_PER_MS);
        assert_eq!(world.spell_areas.len(), 2, "one zone on each player");
        let now = world.last_now_ms;
        world.tick_spell_areas(now);
        assert!(holds(&world, first, WEARY));
        world.ability_performed(first);
        assert_eq!(
            curse_data(&world).map(|data| data.afflicted),
            Some(true),
            "a full meter takes the -1 as a cut of its fall"
        );
    }

    /// A curse that marks: the living run player with the lowest health share gets the mark
    /// (Desiccated's Combustible, Inferred from its tooltip).
    #[test]
    fn a_marking_curse_marks_the_player_with_the_lowest_health_share() {
        let mut world = opened();
        let first = arrive(&mut world, PLAYER);
        let second = arrive(&mut world, OTHER);
        world.start_run_curse(CurseMeter {
            fill: Some(CurseFill::MarkLowestHealth {
                mark: WEARY.to_owned(),
                weaken: None,
                explosion: None,
            }),
            ..fiendish()
        });
        let max = world
            .world
            .get::<Vitals>(second)
            .expect("vitals")
            .health
            .max();
        world
            .world
            .get_mut::<Vitals>(second)
            .expect("vitals")
            .health = nw_sim_core::Health::at(max / 2, max);
        world.run_curse_gain(
            AfflictionGain {
                kind: lowercase_crc32(CURSE2),
                amount: CURSE_MAX,
            },
            world.server_ms,
        );
        assert!(holds(&world, second, WEARY));
        assert!(!holds(&world, first, WEARY));
    }

    /// Item 3: a held major curse's `OnPerform` ability fires on each ability: Blood Offering
    /// unless the player holds `Blunderbuss_MortarShotFired`, Exhaust always; a player outside a
    /// run with no curse effect gets nothing.
    #[test]
    fn the_major_curses_fire_their_effects_when_a_player_performs_an_ability() {
        let mut world = opened();
        let entity = arrive(&mut world, PLAYER);
        world.ability_performed(entity);
        assert!(!holds(&world, entity, BLOOD_OFFERING), "no curse held");
        hold(&mut world, entity, BLOOD_BUFF);
        hold(&mut world, entity, MORTAR_FIRED);
        world.ability_performed(entity);
        assert!(
            !holds(&world, entity, BLOOD_OFFERING),
            "barred by the mortar"
        );
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .status_effects
            .remove(lowercase_crc32(MORTAR_FIRED));
        world.ability_performed(entity);
        assert!(holds(&world, entity, BLOOD_OFFERING));
        hold(&mut world, entity, EXHAUST_BUFF);
        world.ability_performed(entity);
        assert!(holds(&world, entity, EXHAUST));
    }

    /// Player `number` arrives in the expedition of [`opened`] with its client still loading
    /// (a fresh login).
    fn arrive_loading(world: &mut SimWorld, number: u64) -> Entity {
        let mut admit = admit_at(number, 1.0, true);
        admit.progression.arena.stay = Some(stay());
        world.add_instance_member(Uuid::from_u128(INSTANCE), admit.character);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        world.player_by_number(number).expect("player")
    }

    /// Retail Starstone 7.282 s and 11.852 s: a participant whose client loads is 2, then 3;
    /// Barnacles 285.941 s: at death's door 11.
    #[test]
    fn a_loading_player_is_2_then_3_and_at_deaths_door_11() {
        let mut world = opened();
        let entity = arrive_loading(&mut world, PLAYER);
        assert_eq!(statuses(&world), [ACTIVE_BIT]);
        world
            .world
            .get_mut::<Vitals>(entity)
            .expect("vitals")
            .spawned = true;
        world.tick_expedition_mode();
        assert_eq!(statuses(&world), [PLAYING_STATUS]);
        let now = world.server_ms;
        world
            .world
            .get_mut::<Vitals>(entity)
            .expect("vitals")
            .deaths_door_since_ms = Some(now);
        world.tick_expedition_mode();
        assert_eq!(statuses(&world), [PLAYING_STATUS | DEATHS_DOOR_BIT]);
        assert_eq!(statuses(&world), [11]);
    }

    /// Retail `M2-TEMPEST-HEART` 443.560 s: a mutated run's join carries the run's mutation
    /// (the lowercase ids' CRCs and the difficulty); Starstone 7.141 s: a normal run's the
    /// empty context.
    #[test]
    fn a_join_carries_the_runs_mutation_context() {
        let mut world = opened_mutated();
        let entity = arrive_mutated(&mut world, PLAYER, CATALOG_DIFFICULTY);
        let context = world.instance_mutation().cloned().expect("mutated");
        let joined = participant_states(&mut world, entity)
            .into_iter()
            .find(|state| state.active.is_some())
            .expect("the joined state");
        assert_eq!(
            joined.mutation_context,
            Some(MutationContextWire {
                elemental: lowercase_crc32(&context.set.elemental),
                promotion: lowercase_crc32(&context.set.promotion),
                curse: lowercase_crc32(&context.set.curse),
                difficulty: 3,
            })
        );
        let mut normal = opened();
        let entity = arrive(&mut normal, PLAYER);
        let joined = participant_states(&mut normal, entity)
            .into_iter()
            .find(|state| state.active.is_some())
            .expect("the joined state");
        assert_eq!(joined.mutation_context, Some(MutationContextWire::NONE));
    }

    /// Retail Starstone (runs 3 and 30): the run's end leaves the script state 24 with the
    /// tasks 1 to 24 done, listed as the first task, the root (`ConsecutiveTask`, progress 24,
    /// active), then 2 to 24; 1.02 s later task 25 ends (state 25).
    #[test]
    fn the_run_end_does_the_tasks_up_to_the_last_boss_then_the_victory_task() {
        let mut world = opened();
        arrive(&mut world, PLAYER);
        let script = expedition_script(STARSTONE).expect("Starstone");
        let id = |index: u8| {
            script
                .tasks
                .iter()
                .find(|(task, _)| *task == index)
                .map(|(_, key)| lowercase_crc32(key))
                .expect("task")
        };
        let now = world.server_ms;
        world.complete_expedition_run(now);
        let (state, tasks) = tasks_of(&world);
        assert_eq!(state, 24);
        assert_eq!(tasks.len(), 25);
        assert_eq!(tasks[0], (1, id(1), DONE_PROGRESS, TaskWireState::Complete));
        assert_eq!(tasks[1], (27, ROOT_TASK, 24, TaskWireState::Active));
        assert_eq!(tasks[24], (24, id(24), DONE_PROGRESS, TaskWireState::Complete));
        world.server_ms += 1_019;
        world.tick_expedition_mode();
        assert_eq!(tasks_of(&world).0, 24, "not yet");
        world.server_ms += 1;
        world.tick_expedition_mode();
        let (state, tasks) = tasks_of(&world);
        assert_eq!(state, 25);
        assert_eq!(tasks[1], (27, ROOT_TASK, 25, TaskWireState::Active));
        assert_eq!(tasks[25], (25, id(25), DONE_PROGRESS, TaskWireState::Complete));
        world.complete_expedition_run(now);
        assert_eq!(tasks_of(&world).0, 25, "one end");
    }

    /// The Starstone start area: `AreaMutatorStart`'s 3 x 20 x 20 box at (942, 577, 317).
    const START_AT: [f32; 3] = [942.0, 577.0, 317.0];
    /// The Starstone arrival place (retail run 3, 7.141 s).
    const SPAWN_AT: [f32; 3] = [966.2, 577.26, 317.0];

    /// A box of half size `half_extents` at `at`.
    fn box_at(at: [f32; 3], half_extents: [f32; 3]) -> Detector {
        let mut place = nw_world::collision::IDENTITY_AFFINE;
        for (row, value) in place.iter_mut().zip(at) {
            row[3] = value;
        }
        Detector {
            shape: nw_world::collision::Geometry::Box { half_extents },
            place,
        }
    }

    /// Gives the run of `world` the Starstone start area.
    fn with_start_area(world: &mut SimWorld) {
        world.expedition_mut().expect("expedition").start_area =
            Some(box_at(START_AT, [1.5, 10.0, 10.0]));
    }

    /// Retail Starstone runs 3, 30 and 43: a player in `AreaSealedChamber` ends task 1
    /// (82.914 s, the area 83.030 s); in `AreaObelisk` the tasks up to the ampitheater exit
    /// (Inferred: with the ones before it the sim does not see end) and, in a normal run, the
    /// five mutation tasks after it (state 12, 360.171 s); an area task that is done waits no
    /// more.
    #[test]
    fn a_player_in_a_task_area_ends_the_task_and_the_ones_before_it() {
        const CHAMBER_AT: [f32; 3] = [900.0, 600.0, 317.0];
        const OBELISK_AT: [f32; 3] = [800.0, 500.0, 330.0];
        const HALF: [f32; 3] = [4.0, 4.0, 4.0];
        let mut world = opened();
        let first = arrive(&mut world, PLAYER);
        world.expedition_mut().expect("expedition").task_areas = vec![
            (
                AreaTask {
                    done: 0,
                    area: "AreaSealedChamber",
                },
                box_at(CHAMBER_AT, HALF),
            ),
            (
                AreaTask {
                    done: 6,
                    area: "AreaObelisk",
                },
                box_at(OBELISK_AT, HALF),
            ),
        ];
        stand_at(&mut world, first, SPAWN_AT);
        world.tick_expedition_mode();
        assert_eq!(tasks_of(&world).0, 0);
        stand_at(&mut world, first, CHAMBER_AT);
        world.tick_expedition_mode();
        assert_eq!(tasks_of(&world).0, 1);
        stand_at(&mut world, first, OBELISK_AT);
        world.tick_expedition_mode();
        let (state, tasks) = tasks_of(&world);
        assert_eq!(state, 12, "task 7 and the five mutation tasks");
        assert_eq!(tasks[1], (27, ROOT_TASK, 12, TaskWireState::Active));
        stand_at(&mut world, first, CHAMBER_AT);
        world.tick_expedition_mode();
        assert_eq!(tasks_of(&world).0, 12, "a done area task waits no more");
    }

    /// Puts player `entity` at `at`.
    fn stand_at(world: &mut SimWorld, entity: Entity, [x, y, z]: [f32; 3]) {
        let limits = world.settings.limits;
        world.world.get_mut::<Body>(entity).expect("body").motion =
            super::super::Motion::new(Position { x, y, z }, NOW, &limits);
    }

    /// Starts the clock of `world`: its start area, `entity` in it, a tick.
    fn start_clock(world: &mut SimWorld, entity: Entity) {
        with_start_area(world);
        stand_at(world, entity, START_AT);
        world.tick_expedition_mode();
    }

    /// Retail Starstone run 3: the clock starts when the first player enters
    /// `AreaMutatorStart` (20.818 s), not at the arrivals (7.141 and 11.852 s); a run without a
    /// start area starts none.
    #[test]
    fn the_clock_starts_when_a_player_enters_the_start_area() {
        let mut world = opened();
        let first = arrive(&mut world, PLAYER);
        world.tick_expedition_mode();
        assert_eq!(value_of(&world, START_TIME), 0, "no start area");
        with_start_area(&mut world);
        stand_at(&mut world, first, SPAWN_AT);
        world.tick_expedition_mode();
        assert_eq!(value_of(&world, START_TIME), 0, "at the spawn");
        stand_at(&mut world, first, START_AT);
        world.tick_expedition_mode();
        assert_eq!(value_of(&world, START_TIME), world.unix_s());
    }

    /// Retail Starstone (run 3): the clock starts at the start area; a death costs 500 and
    /// shows the sign; the end 854 s after the start scores -500 + 5 x (2400 - 854) = 7230,
    /// times the multiplier 3 = 21690, rank 0.
    #[test]
    fn the_clock_starts_and_the_end_scores_the_time_under_the_time_to_beat() {
        let mut world = opened();
        let first = arrive(&mut world, PLAYER);
        arrive(&mut world, OTHER);
        start_clock(&mut world, first);
        let start_s = world.unix_s();
        assert_eq!(value_of(&world, START_TIME), start_s);
        set_dead(&mut world, first, true);
        world.tick_expedition_mode();
        assert_eq!(statuses(&world), [DEAD_STATUS, PLAYING_STATUS]);
        assert_eq!(value_of(&world, SCORE), 500);
        assert_eq!(value_of(&world, SCORE_NEGATIVE), 1);
        assert_eq!(value_of(&world, DEATHS), 1);
        assert_eq!(value_of(&world, TEAM_WIPES), 0, "one player stands");
        set_dead(&mut world, first, false);
        world.server_ms += 854 * MS_PER_S;
        let now = world.server_ms;
        world.complete_expedition_run(now);
        assert_eq!(value_of(&world, END_TIME), start_s + 854);
        assert_eq!(value_of(&world, SCORE), 7_230);
        assert_eq!(value_of(&world, SCORE_NEGATIVE), 0);
        assert_eq!(value_of(&world, FINAL_SCORE), 21_690);
        assert_eq!(value_of(&world, MUTATOR_RANK), 0);
        assert_eq!((rank_of(26_235), rank_of(81_520)), (1, 2), "Depths, Fiendish run");
    }

    /// Retail runs 15, 30, 37 and 40: a team wipe costs 500, counts, and the first one sets the
    /// wipe bonus and the multiplier (3 to 2) once; past 3 deaths the death bonus goes and the
    /// multiplier stays; past the time to beat the time bonus goes and the multiplier -1.
    #[test]
    fn wipes_deaths_and_the_time_to_beat_cost_their_bonuses() {
        let mut world = opened();
        let first = arrive(&mut world, PLAYER);
        let second = arrive(&mut world, OTHER);
        start_clock(&mut world, first);
        let wipe = |world: &mut SimWorld| {
            set_dead(world, first, true);
            set_dead(world, second, true);
            world.tick_expedition_mode();
            set_dead(world, first, false);
            set_dead(world, second, false);
            world.tick_expedition_mode();
        };
        wipe(&mut world);
        assert_eq!(value_of(&world, TEAM_WIPES), 1);
        assert_eq!(value_of(&world, WIPE_BONUS), BONUS_LOST);
        assert_eq!(value_of(&world, MULTIPLIER), 2);
        assert_eq!(value_of(&world, DEATHS), 2);
        assert_eq!(value_of(&world, SCORE), 1_500, "two deaths and a wipe");
        assert_eq!(value_of(&world, DEATH_BONUS), BONUS_MET);
        wipe(&mut world);
        assert_eq!(value_of(&world, TEAM_WIPES), 2);
        assert_eq!(value_of(&world, MULTIPLIER), 2, "the bonus goes once");
        assert_eq!(value_of(&world, DEATH_BONUS), BONUS_LOST, "4 deaths");
        world.server_ms += 2_401 * MS_PER_S;
        world.tick_expedition_mode();
        assert_eq!(value_of(&world, TIME_BONUS), BONUS_LOST);
        assert_eq!(value_of(&world, MULTIPLIER), 1);
    }

    /// Retail runs 16, 20 and 34: a mutated run's normal kill adds the script's points and
    /// counts `f963dc0c` up to its kills for bonus, the last sets `6974d92c` and the multiplier
    /// +1, later kills count with 0; a named kill adds 500 and counts `4f6a27d3` up to 12, the
    /// 12th sets `d15dc70a` and the multiplier +1; a normal run scores no kill (runs 3 to 15);
    /// no kill moves `gatherablesCollected`.
    #[test]
    fn a_mutated_run_scores_its_kills_up_to_their_bonus() {
        assert!(named_creature("DungeonBoss") && named_creature("dungeonminiboss"));
        assert!(!named_creature("Dungeon+") && !named_creature("Named_Solo+"));
        let mut normal = opened();
        arrive(&mut normal, PLAYER);
        normal.score_expedition_kill(false);
        normal.score_expedition_kill(true);
        assert_eq!(
            [NORMAL_KILLS, NAMED_ENEMIES_KILLED, SCORE].map(|key| value_of(&normal, key)),
            [0, 0, 0],
            "a normal run"
        );
        let mut world = opened_mutated();
        arrive_mutated(&mut world, PLAYER, CATALOG_DIFFICULTY);
        let script = expedition_script(CUTLASS_KEYS).expect("Barnacles");
        for _ in 1..script.kills_for_bonus {
            world.score_expedition_kill(false);
        }
        assert_eq!(value_of(&world, KILL_BONUS), 0);
        world.score_expedition_kill(false);
        assert_eq!(value_of(&world, NORMAL_KILLS), script.kills_for_bonus);
        assert_eq!(value_of(&world, KILL_BONUS), BONUS_MET);
        assert_eq!(value_of(&world, MULTIPLIER), FIRST_MULTIPLIER + 1);
        let kills = script.kill_points * script.kills_for_bonus;
        assert_eq!(value_of(&world, SCORE), kills);
        world.score_expedition_kill(false);
        assert_eq!(value_of(&world, NORMAL_KILLS), script.kills_for_bonus + 1);
        assert_eq!(value_of(&world, SCORE), kills, "past the bonus a kill scores 0");
        for _ in 0..NAMED_FOR_BONUS {
            world.score_expedition_kill(true);
        }
        assert_eq!(value_of(&world, NAMED_ENEMIES_KILLED), NAMED_FOR_BONUS);
        assert_eq!(value_of(&world, NAMED_BONUS), BONUS_MET);
        assert_eq!(value_of(&world, MULTIPLIER), FIRST_MULTIPLIER + 2);
        let named = NAMED_KILL_POINTS * NAMED_FOR_BONUS;
        world.score_expedition_kill(true);
        assert_eq!(value_of(&world, SCORE), kills + named, "the 13th scores 0");
        assert_eq!(value_of(&world, GATHERABLES), 0);
    }

    /// Retail run 45: the social service's offline (88.659 s) and the participant's status 1
    /// (88.685 s), its online (116.328 s) and 3 again (116.150 s): a player whose connection
    /// ends keeps its place without the bit 2, and has it again when its character comes back;
    /// an exit still takes a player out of the lists.
    #[test]
    fn an_offline_player_keeps_its_place_without_the_active_bit() {
        let mut world = opened();
        let mut admit = admit_at(PLAYER, 1.0, false);
        admit.progression.arena.stay = Some(stay());
        world.add_instance_member(Uuid::from_u128(INSTANCE), admit.character);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        arrive(&mut world, OTHER);
        world.tick_expedition_mode();
        assert_eq!(statuses(&world), [PLAYING_STATUS, PLAYING_STATUS]);
        world.leave(&admit.connection).expect("left");
        world.tick_expedition_mode();
        assert_eq!(statuses(&world), [LOADED_BIT, PLAYING_STATUS]);
        world.add_instance_member(Uuid::from_u128(INSTANCE), admit.character);
        world.admit(&admit, NOW, SERVER_TIME).expect("back");
        world.tick_expedition_mode();
        assert_eq!(statuses(&world), [PLAYING_STATUS, PLAYING_STATUS]);
        let other = world.player_by_number(OTHER).expect("other");
        world.leave_expedition_mode(other);
        assert_eq!(statuses(&world), [PLAYING_STATUS], "an exit still leaves the lists");
    }

    /// Retail Amrine (4 runs): its script keeps no score, so no value changes; its end leaves
    /// the script state 13 with no task after it.
    #[test]
    fn amrine_keeps_no_score() {
        const AMRINE: &str = "DungeonAmrine";
        let mut mode = crate::world::instances::tests::mode(
            AMRINE,
            "NW_Dungeon_Windsward_00",
            InstanceKind::Expedition,
        );
        mode.crc = lowercase_crc32(AMRINE);
        let content = PlayerContent {
            instances: InstanceCatalog::build([mode.clone()]),
            ..content()
        };
        let mut world = opened_with(mode, content);
        arrive_in(
            &mut world,
            PLAYER,
            InstanceStay {
                game_mode: AMRINE.to_owned(),
                ..stay()
            },
        );
        world.tick_expedition_mode();
        assert_eq!(value_of(&world, START_TIME), 0);
        let now = world.server_ms;
        world.complete_expedition_run(now);
        assert_eq!(value_of(&world, END_TIME), 0);
        assert_eq!(tasks_of(&world).0, 13);
        world.server_ms += 60 * MS_PER_S;
        world.tick_expedition_mode();
        assert_eq!(tasks_of(&world).0, 13, "no task after the end");
    }

    /// Client `FUN_1461f2370` and `FUN_1461f2170`: a full Desiccated meter marks the living
    /// player with the lowest health share and weakens every player (the major curse); when the
    /// fall ends the marked player gets the cast effect, loses its mark, and the chain hits the
    /// nearest other target within its radius.
    #[test]
    fn a_marked_player_explodes_when_the_fall_ends() {
        let mut world = opened_mutated();
        let first = arrive_mutated(&mut world, PLAYER, CATALOG_DIFFICULTY);
        arrive_mutated(&mut world, OTHER, CATALOG_DIFFICULTY);
        if let Some(expedition) = world.expedition_mut() {
            expedition.curse = None;
        }
        world.start_run_curse(CurseMeter {
            fill: Some(CurseFill::MarkLowestHealth {
                mark: WEARY.to_owned(),
                weaken: Some(EXHAUST.to_owned()),
                explosion: Some(CurseExplosion {
                    cast: BLOOD_OFFERING.to_owned(),
                    effects: vec![MORTAR_FIRED.to_owned()],
                    radius_m: 4.0,
                    jumps: 4,
                    delay_ms: 250,
                }),
            }),
            rise_needs_combat: false,
            rise_escalates: false,
            ..fiendish()
        });
        let max = world.world.get::<Vitals>(first).expect("vitals").health.max();
        world.world.get_mut::<Vitals>(first).expect("vitals").health =
            nw_sim_core::Health::at(max / 2, max);
        world.run_curse_gain(
            AfflictionGain {
                kind: lowercase_crc32(CURSE2),
                amount: CURSE_MAX,
            },
            world.server_ms,
        );
        assert!(holds(&world, first, WEARY), "the lowest health share");
        let second = world.player_by_number(OTHER).expect("player");
        assert!(holds(&world, first, EXHAUST) && holds(&world, second, EXHAUST));
        world.take_mutation_effects();
        world.server_ms += 8_000;
        world.tick_expedition_mode();
        assert!(!holds(&world, first, WEARY), "the mark goes");
        assert!(holds(&world, first, BLOOD_OFFERING), "the cast effect");
        let hits: Vec<(u64, String)> = world
            .take_mutation_effects()
            .into_iter()
            .map(|(number, effect)| (number, effect.status_id))
            .collect();
        assert_eq!(hits, [(OTHER, MORTAR_FIRED.to_owned())]);
        world.server_ms += 250;
        world.tick_expedition_mode();
        assert!(world.take_mutation_effects().is_empty(), "no target left");
    }
}
