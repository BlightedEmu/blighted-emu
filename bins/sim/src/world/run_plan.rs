// SPDX-License-Identifier: AGPL-3.0-or-later
//! The run of an expedition as a game master or a test bot plays it: `!queststate run:<mode>`
//! and `!questplan run:<mode>` (the objective id [`RUN_OBJECTIVE_PREFIX`] and a game mode id)
//! answer in the words of a quest's lines ([`super::gm_chat`], [`super::quest_walk`]), so a
//! bot plays the run as it plays a quest: it enters the expedition, fights its last boss, and
//! opens the end chest; the run is done for a player once the run ended
//! ([`SimWorld::expedition_reached_end`]) and that player opened the chest.
//!
//! **Observed** (retail expedition ledgers, scratch
//! `local-data/scratch-work/research-2026-11-23/r2/led/*.inter`): after the last boss a player
//! holds a 5 s gather (kind 14) on a large dungeon loot container, `LootLargeT3` in
//! Barnacles (`ck08` 1218.0 to 1223.1 s); the level holds that chest 8.4 m from the siren's
//! encounter, with a `LootLargeT2` and two medium chests on the same spot (tier variants).
//! A large container stands near every boss of a run (5 to 9 such gathers in a run).
//!
//! **Inferred**: the end chest is the large loot container node nearest the last boss's
//! encounter within [`END_CHEST_REACH_M`] (the nearest of the tier variants; which tier retail
//! shows for which expedition level is Unknown); the last boss is the mob that a top stage of
//! the encounter of the script's last boss task tracks ([`ExpeditionScript::event_tasks`]).
//!
//! A trial (a game mode with no expedition script: the story and soul trials) ends when every
//! encounter of its script is done; its plan is the boss of its first encounter not done yet
//! (the daily or the story twin the run keeps), and it has no end chest.

use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::Position;
use nw_world::dungeon_scripts::{Encounter, ObjectiveSpec, TargetSlice};
use nw_world::records::VitalsSpawn;

use super::SimWorld;
use super::expedition_mode::slice_stem;
use super::expedition_scripts::{ExpeditionScript, TaskEvent};
use super::gm_chat::{QUEST_DONE, QUEST_RUNNING};
use super::quest_walk::{Outcome, StepPlan};

/// The start of the objective id of an expedition's run: `run:DungeonAmrine`.
pub(super) const RUN_OBJECTIVE_PREFIX: &str = "run:";
/// The task word of a run's plan lines.
const RUN_TASK: &str = "run";
/// The start of the gatherable id of a large loot container (`LootLargeT3`).
const END_CHEST_PREFIX: &str = "lootlarge";
/// How far from the last boss's encounter the end chest stands at most, m (Inferred: 8.4 m
/// in Barnacles, 16.0 m in Savage Divide, 30.0 m in Tempest's Heart).
const END_CHEST_REACH_M: f32 = 40.0;
/// The source words of the end chest's plan line.
const END_CHEST_SOURCE: &str = "the run's end chest";
/// One kill: the last boss.
const ONE: u32 = 1;

/// The game mode of run objective `objective` (`run:<mode>`); `None` for a quest's id.
pub(super) fn run_mode(objective: &str) -> Option<&str> {
    objective
        .get(..RUN_OBJECTIVE_PREFIX.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(RUN_OBJECTIVE_PREFIX))
        .and_then(|_| objective.get(RUN_OBJECTIVE_PREFIX.len()..))
        .filter(|mode| !mode.is_empty())
}

/// The slice stem of the encounter whose success ends `script`'s last boss task: the event task
/// with the highest done count among the encounter successes.
fn last_boss_stem(script: &ExpeditionScript) -> Option<&'static str> {
    script
        .event_tasks
        .iter()
        .filter_map(|task| match task.event {
            TaskEvent::EncounterSucceeded(slice) => Some((task.done, slice)),
            _ => None,
        })
        .max_by_key(|(done, _)| *done)
        .map(|(_, slice)| slice)
}

/// The targets of `encounter`'s bosses: each target that a top stage's `SliceDestroyed`
/// objective tracks, in stage order (the Ennead Goliaths are two). An encounter whose top
/// stages wait for encounter events alone (`Enc_Quest_Isabella_Daily`, Liang's, Mordred's: the
/// boss script sends them) has as bosses its targets that spawn a mob (**Inferred**: each such
/// encounter has one, its boss; live 2026-11-24 08:10:55Z the Isabella soul trial plan said "no
/// boss encounter left").
fn boss_targets(encounter: &Encounter) -> Vec<usize> {
    let mut targets: Vec<usize> = Vec::new();
    for objective in encounter
        .top_stages
        .iter()
        .filter_map(|top| encounter.stages.get(*top))
        .flat_map(|stage| &stage.objectives)
    {
        if let ObjectiveSpec::SliceDestroyed {
            target: Some(target),
            ..
        } = objective.kind
            && !targets.contains(&target)
        {
            targets.push(target);
        }
    }
    if targets.is_empty() {
        targets = (0..encounter.targets.len())
            .filter(|target| {
                encounter.targets[*target]
                    .slices
                    .iter()
                    .any(|slice| slice.mob.is_some())
            })
            .collect();
    }
    targets
}

/// The spawn of the boss of `encounter`: its first boss target ([`boss_targets`]) that is a mob
/// (`QuestDracula`'s first one is the setup's area trigger).
fn boss_spawn(encounter: &Encounter) -> Option<VitalsSpawn> {
    boss_targets(encounter)
        .into_iter()
        .find_map(|target| boss_spawn_of(encounter, target))
}

/// The spawn of boss target `target` of `encounter`: its first mob, where a stage spawns it
/// (else at the encounter).
fn boss_spawn_of(encounter: &Encounter, target: usize) -> Option<VitalsSpawn> {
    let mob = encounter
        .targets
        .get(target)?
        .slices
        .iter()
        .flat_map(TargetSlice::mobs)
        .next()?;
    let at = encounter
        .stages
        .iter()
        .flat_map(|stage| &stage.spawns)
        .filter(|spawn| spawn.target == target)
        .find_map(|spawn| spawn.locations.first().copied())
        .unwrap_or(encounter.position);
    Some(VitalsSpawn {
        vitals_id: lowercase_crc32(&mob.vitals_id),
        category_id: lowercase_crc32(&mob.category_id),
        level: mob.level,
        position: at,
        is_encounter: true,
        territory_ids: Vec::new(),
    })
}

fn position([x, y, z]: [f32; 3]) -> Position {
    Position { x, y, z }
}

impl SimWorld {
    /// Whether this sim runs an instance of game mode `mode`.
    fn runs_mode(&self, mode: &str) -> bool {
        self.instance_run
            .as_ref()
            .is_some_and(|run| run.game_mode().eq_ignore_ascii_case(mode))
    }

    /// The encounter of the running expedition's last boss task.
    fn last_boss_encounter(&self) -> Option<&Encounter> {
        let (script, _) = self.expedition_run()?;
        let stem = last_boss_stem(script)?;
        self.dungeon_run
            .as_ref()?
            .script()
            .encounters
            .iter()
            .find(|encounter| slice_stem(&encounter.slice).eq_ignore_ascii_case(stem))
    }

    /// The encounter whose boss the run of this sim fights next: the expedition's last boss
    /// encounter, else (a trial) the first encounter not done whose top stage tracks a boss.
    fn next_boss_encounter(&self) -> Option<(usize, &Encounter)> {
        let run = self.dungeon_run.as_ref()?;
        let mut encounters = run.script().encounters.iter().enumerate();
        if self.expedition_run().is_some() {
            let last = self.last_boss_encounter()?;
            return encounters.find(|(_, encounter)| std::ptr::eq(*encounter, last));
        }
        encounters.find(|(index, encounter)| {
            !run.encounter_done(*index) && boss_spawn(encounter).is_some()
        })
    }

    /// The boss of `encounter` (encounter `index` of the run) to fight now: of its boss targets
    /// with a living mob, the one whose mob takes hits and has the most health, at that mob's
    /// place; else its first boss target where it spawns. Linked bosses take turns (the Ennead
    /// Goliaths: Horus takes no hits while the Brute stands, and both must fall at once; live
    /// 2026-11-24 05:02:48Z a group fought only the Brute in the `Both` phase).
    pub(super) fn boss_now(&self, index: usize, encounter: &Encounter) -> Option<VitalsSpawn> {
        let targets = boss_targets(encounter);
        let living = self
            .dungeon_run
            .as_ref()
            .map(|run| run.living_mobs_of(index))
            .unwrap_or_default();
        let standing = living
            .iter()
            .filter(|(entity, target)| {
                targets.contains(target) && self.mobs.invulnerable(*entity).is_none()
            })
            .max_by_key(|(entity, _)| self.mobs.health(*entity).unwrap_or_default());
        match standing {
            Some((entity, target)) => {
                let spawn = boss_spawn_of(encounter, *target)?;
                let position = self.mobs.get(*entity).map_or(spawn.position, |mob| {
                    [mob.position.x, mob.position.y, mob.position.z]
                });
                Some(VitalsSpawn { position, ..spawn })
            }
            None => boss_spawn(encounter),
        }
    }

    /// Whether the run of this sim ended: the expedition's (its last boss task), else the
    /// script's (every encounter done).
    fn run_ended(&self) -> bool {
        match self.expedition_run() {
            Some((_, ended)) => ended,
            None => self
                .dungeon_run
                .as_ref()
                .is_some_and(super::dungeon_script::DungeonRun::completed),
        }
    }

    /// The end chest of the running expedition: its gatherable id and node place (module
    /// text).
    fn end_chest(&self) -> Option<(String, Position)> {
        self.expedition_run()?;
        let at = position(self.last_boss_encounter()?.position);
        let content = &self.content;
        content
            .gatherables
            .iter()
            .filter(|gatherable| {
                gatherable.loot_container
                    && gatherable.id.to_lowercase().starts_with(END_CHEST_PREFIX)
            })
            .filter_map(|gatherable| {
                let (_, node) = content
                    .gather_points
                    .within(lowercase_crc32(&gatherable.id), at, END_CHEST_REACH_M)
                    .into_iter()
                    .next()?;
                Some((gatherable.id.clone(), node))
            })
            .min_by(|(_, a), (_, b)| {
                a.horizontal_distance(at)
                    .total_cmp(&b.horizontal_distance(at))
            })
    }

    /// Player `number` gathered the loot container node at `node` of gatherable `gatherable`:
    /// when it is a node of the running expedition's end chest's gatherable beside the last
    /// boss (within [`END_CHEST_REACH_M`] of its encounter: Barnacles holds two `LootLargeT3`
    /// there, live 2026-11-24 04:55:18Z a bot opened the farther one), the player opened it.
    pub(super) fn end_chest_gathered(&mut self, number: u64, node: Position, gatherable: &str) {
        let Some((chest, _)) = self.end_chest() else {
            return;
        };
        let Some(boss_at) = self
            .last_boss_encounter()
            .map(|encounter| position(encounter.position))
        else {
            return;
        };
        if chest.eq_ignore_ascii_case(gatherable)
            && node.horizontal_distance(boss_at) <= END_CHEST_REACH_M
        {
            tracing::info!(player = number, chest, "end chest opened");
            self.note_end_chest_opened(number);
        }
    }

    /// Whether the run of game mode `mode` is done for player `number`: the run in this sim
    /// ended, and the player opened its end chest (a run with none needs no chest).
    /// A catacombs run is done for a player once it was extracted or the run ended
    /// ([`Self::catacomb_run_left`]).
    fn run_done(&self, number: u64, mode: &str) -> bool {
        if let Some(left) = self.catacomb_run_left(number) {
            return self.runs_mode(mode) && left;
        }
        self.runs_mode(mode)
            && self.run_ended()
            && (self.end_chest().is_none() || self.opened_end_chest(number))
    }

    /// The `!queststate` lines of run objective `objective` of game mode `mode` for player
    /// `number`.
    pub(super) fn run_state_lines(&self, number: u64, objective: &str, mode: &str) -> Vec<String> {
        let word = if self.run_done(number, mode) {
            QUEST_DONE
        } else {
            QUEST_RUNNING
        };
        vec![format!("{word} {objective}")]
    }

    /// The `!questplan` lines of the run of game mode `mode` for player `number`: its entry
    /// outside the instance, the last boss while the run goes on, then the end chest; a
    /// catacombs run's own plan ([`Self::catacomb_plan`]).
    pub(crate) fn run_plan_lines(&self, number: u64, mode: &str) -> Vec<String> {
        let plan = if !self.runs_mode(mode) {
            self.trial_into(
                mode,
                StepPlan::Stop(Outcome::Broken, format!("no entry of game mode {mode}")),
            )
        } else if let Some(plan) = self.catacomb_plan(number) {
            plan
        } else {
            match (self.dungeon_run.is_some(), self.run_ended()) {
                (false, _) => StepPlan::Stop(
                    Outcome::Broken,
                    format!("game mode {mode} runs no level script"),
                ),
                (true, false) => match self.next_boss_encounter() {
                    Some((index, encounter)) => match self.boss_now(index, encounter) {
                        Some(spawn) => StepPlan::Kill {
                            spawn,
                            count: ONE,
                            territory: None,
                            spawned: false,
                            encounter: Some(encounter.slice.clone()),
                        },
                        None => StepPlan::Stop(
                            Outcome::Broken,
                            format!("the boss encounter {} tracks no mob", encounter.name),
                        ),
                    },
                    None => StepPlan::Stop(
                        Outcome::Broken,
                        "the level script has no boss encounter left".to_owned(),
                    ),
                },
                (true, true) => match self.end_chest() {
                    Some((chest, _)) if !self.opened_end_chest(number) => StepPlan::Item {
                        item: chest.clone(),
                        count: ONE,
                        gathered: true,
                        source: END_CHEST_SOURCE.to_owned(),
                        node: Some(chest),
                    },
                    _ => StepPlan::Wait,
                },
            }
        };
        vec![self.plan_line(RUN_TASK, &plan)]
    }
}

#[cfg(test)]
mod tests {
    use nw_world::dungeon_scripts::{Spawn, Stage, StageObjective, Target, TargetMob};

    use super::super::expedition_scripts::expedition_script;
    use super::*;

    #[test]
    fn a_run_objective_names_its_game_mode() {
        assert_eq!(run_mode("run:DungeonAmrine"), Some("DungeonAmrine"));
        assert_eq!(run_mode("RUN:DungeonAmrine"), Some("DungeonAmrine"));
        assert_eq!(run_mode("run:"), None);
        assert_eq!(run_mode("04A_Side02_06_Amrine"), None);
    }

    /// Amrine's last boss task is the boss encounter's success (task 12), not its stage 3.
    #[test]
    fn the_last_boss_task_names_the_last_boss_encounter() {
        let amrine = expedition_script("DungeonAmrine").expect("Amrine");
        assert_eq!(last_boss_stem(amrine), Some("dg_enc_windsward_00_boss"));
        let dynasty = expedition_script("DungeonEbonscale00").expect("Dynasty");
        assert_eq!(last_boss_stem(dynasty), Some("dg_enc_ebonscale_00_empress"));
    }

    /// A trial's next boss is its first encounter with a tracked boss that is not done.
    #[test]
    fn a_trial_fights_the_boss_of_its_first_boss_encounter() {
        let boss = boss_encounter();
        let lead_up = Encounter {
            name: "EncounterLeadup".to_owned(),
            ..Encounter::default()
        };
        let mut world = crate::world::tests::world_with_spawns(Vec::new());
        world.dungeon_run = Some(super::super::dungeon_script::DungeonRun::new(
            uuid::Uuid::nil(),
            nw_world::dungeon_scripts::LevelScript {
                encounters: vec![lead_up, boss.clone()],
                ..nw_world::dungeon_scripts::LevelScript::default()
            },
        ));
        assert_eq!(world.next_boss_encounter(), Some((1, &boss)));
        assert_eq!(
            world.boss_now(1, &boss).map(|spawn| spawn.position),
            Some(BOSS_AT),
            "no boss mob lives yet: where it spawns"
        );
        assert!(!world.run_ended());
    }

    /// The boss is the mob a top stage tracks, where a child stage spawns it; the adds of the
    /// first target are no boss.
    /// Where the test boss spawns.
    const BOSS_AT: [f32; 3] = [886.0, 906.0, 141.0];

    /// An encounter that waits for events alone (`Enc_Quest_Isabella_Daily`) fights the mob of
    /// its one mob target.
    #[test]
    fn an_encounter_of_events_alone_fights_its_mob_target() {
        let mut encounter = boss_encounter();
        for stage in &mut encounter.stages {
            stage.objectives.clear();
        }
        encounter.targets.remove(0);
        for stage in &mut encounter.stages {
            stage.spawns.retain(|spawn| spawn.target == 1);
            for spawn in &mut stage.spawns {
                spawn.target = 0;
            }
        }
        assert_eq!(boss_targets(&encounter), [0]);
        let spawn = boss_spawn(&encounter).expect("boss");
        assert_eq!(spawn.vitals_id, lowercase_crc32("Withered_Brute_Named_08"));
        assert_eq!(spawn.position, BOSS_AT);
    }

    /// `QuestDracula`: the first stage tracks the setup's area trigger (no mob); the boss is the
    /// next tracked target that is a mob, not the first target's mob (Cecilia, an ally).
    #[test]
    fn a_tracked_target_without_a_mob_is_no_boss() {
        let mut encounter = boss_encounter();
        encounter.targets.push(Target::default());
        let trigger = encounter.targets.len() - 1;
        let mut setup = encounter.stages[0].clone();
        setup.children.clear();
        setup.objectives[0].kind = ObjectiveSpec::SliceDestroyed {
            target: Some(trigger),
            required: 1,
        };
        encounter.stages.push(setup);
        encounter.top_stages.insert(0, encounter.stages.len() - 1);
        assert_eq!(boss_targets(&encounter), [trigger, 1]);
        let spawn = boss_spawn(&encounter).expect("boss");
        assert_eq!(spawn.vitals_id, lowercase_crc32("Withered_Brute_Named_08"));
    }

    #[test]
    fn the_boss_is_the_tracked_mob_where_its_stage_spawns_it() {
        let spawn = boss_spawn(&boss_encounter()).expect("boss");
        assert_eq!(spawn.vitals_id, lowercase_crc32("Withered_Brute_Named_08"));
        assert_eq!(spawn.position, BOSS_AT);
    }

    /// An encounter whose top stage tracks its second target, the boss, which a child stage
    /// spawns at [`BOSS_AT`] beside an add of the first target.
    fn boss_encounter() -> Encounter {
        let mob = |vitals: &str| TargetSlice {
            mob: Some(TargetMob {
                vitals_id: vitals.to_owned(),
                ..TargetMob::default()
            }),
            ..TargetSlice::default()
        };
        Encounter {
            name: "EncounterBoss".to_owned(),
            top_stages: vec![0],
            stages: vec![
                Stage {
                    children: vec![1],
                    objectives: vec![StageObjective {
                        time_limit_s: 0,
                        succeed_on_timeout: false,
                        failure: false,
                        kind: ObjectiveSpec::SliceDestroyed {
                            target: Some(1),
                            required: 1,
                        },
                    }],
                    ..Stage::default()
                },
                Stage {
                    spawns: vec![
                        Spawn {
                            target: 0,
                            locations: vec![[0.0; 3]],
                            ..Spawn::default()
                        },
                        Spawn {
                            target: 1,
                            locations: vec![BOSS_AT],
                            ..Spawn::default()
                        },
                    ],
                    ..Stage::default()
                },
            ],
            targets: vec![
                Target {
                    slices: vec![mob("Add")],
                    ..Target::default()
                },
                Target {
                    slices: vec![mob("Withered_Brute_Named_08")],
                    ..Target::default()
                },
            ],
            ..Encounter::default()
        }
    }
}
