// SPDX-License-Identifier: AGPL-3.0-or-later
//! The plan of a catacombs run as a game master or a test bot plays it (`!questplan
//! run:Catacombs`, [`super::super::run_plan`]): the boss room's fight, then its extraction (the
//! core's activation, the survive and the exit), until the player is extracted or the run ends.
//!
//! Observed (module text of [`super::end`]): a cleared boss room opens its extraction; its top
//! stages run `Setup`, `Activate core` (the destroy of `catacombs_extraction_initiator_a`, a
//! gather of `DG_Catacombs_Extraction_Initiator`), `Survive` (60 s), `Cleanup` (3 s) and
//! `Exit` (60 s); a player in the open exit is extracted, and the run ends when every
//! participant is.
//!
//! Inferred: the plan goes to the first boss room and leaves by its extraction. The other rooms raise the run's difficulty
//! and fill its pool, but the boss does not wait for them: this sim spawns the side gates
//! (`Gate*`, `LockGate*`) as cards and runs no rule that holds them shut, so no key or clear
//! opens a way (the retail rule that opens a gate is Unknown).

use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::Position;

use super::super::SimWorld;
use super::super::dungeon_script::volume_radius_m;
use super::super::quest_walk::StepPlan;
use super::end::{EXIT_TOP, SURVIVE_TOP};

/// The room kind of a boss room, and the encounter slot of its fight.
const BOSS_ENCOUNTER: &str = "EncounterBoss";
/// The place in its top order of the extraction's `Activate core` stage, before `Survive`.
const CORE_TOP: usize = SURVIVE_TOP - 1;
/// The file stem of the extraction's core slice (its target of `Activate core`).
const CORE_SLICE_STEM: &str = "catacombs_extraction_initiator_a";
/// The gatherable of the core (`GatherableData`).
const CORE_GATHERABLE: &str = "DG_Catacombs_Extraction_Initiator";
/// The height over the exit the plan sends the player to, metres: on its floor.
const EXIT_STAND_M: f32 = 1.0;
/// One kill: the room's boss.
const ONE: u32 = 1;

fn position_of([x, y, z]: [f32; 3]) -> Position {
    Position { x, y, z }
}

/// Whether `slice` is the extraction's core.
fn is_core(slice: &str) -> bool {
    slice
        .rsplit('/')
        .next()
        .and_then(|file| file.split('.').next())
        .is_some_and(|stem| stem.eq_ignore_ascii_case(CORE_SLICE_STEM))
}

impl SimWorld {
    /// Whether player `number` left the catacombs run of this sim: extracted, or the run ended;
    /// `None` when this sim runs no catacombs.
    pub(in crate::world) fn catacomb_run_left(&self, number: u64) -> Option<bool> {
        self.catacomb_run().map(|run| run.end.left_by(number))
    }

    /// The plan of player `number` in the catacombs run of this sim (module text); `None` when
    /// this sim runs no catacombs.
    pub(in crate::world) fn catacomb_plan(&self, number: u64) -> Option<StepPlan> {
        let run = self.catacomb_run()?;
        let dungeon = self.dungeon_run.as_ref()?;
        if run.end.left_by(number) {
            return Some(StepPlan::Wait);
        }
        let encounters = &dungeon.script().encounters;
        // The extraction of a cleared boss room ends the run sooner than the next boss (retail
        // `CATACOMBS3` and `CATACOMBS6` extracted after their bosses); the start room's opens
        // with no boss fought and is not the plan's.
        let extraction = run.end.open_extractions().find(|(index, room, _)| {
            !dungeon.encounter_done(*index)
                && run
                    .layout
                    .rooms
                    .get(*room)
                    .is_some_and(|room| room.room_type == BOSS_ENCOUNTER)
        });
        let Some((extraction, _, exit)) = extraction else {
            let boss = encounters.iter().enumerate().find(|(index, encounter)| {
                encounter.name == BOSS_ENCOUNTER && !dungeon.encounter_done(*index)
            });
            let Some((index, encounter)) = boss else {
                return Some(StepPlan::Wait);
            };
            return Some(match self.boss_now(index, encounter) {
                Some(spawn) => StepPlan::Kill {
                    spawn,
                    count: ONE,
                    territory: None,
                    spawned: false,
                    encounter: Some(encounter.slice.clone()),
                },
                None => StepPlan::Go {
                    at: position_of(encounter.position),
                    territory: None,
                },
            });
        };
        let encounter = encounters.get(extraction)?;
        let place = position_of(encounter.position);
        Some(match dungeon.running_top(extraction) {
            Some(top) if top <= CORE_TOP => {
                match self.encounter_objects.standing_where(
                    place,
                    volume_radius_m(encounter),
                    is_core,
                ) {
                    Some((_, asset, at)) => StepPlan::Start {
                        gatherable: self
                            .content
                            .gatherables
                            .get(lowercase_crc32(CORE_GATHERABLE))
                            .map_or_else(|| CORE_GATHERABLE.to_owned(), |core| core.id.clone()),
                        asset,
                        at,
                        event: Box::new(StepPlan::Wait),
                    },
                    None => StepPlan::Go {
                        at: place,
                        territory: None,
                    },
                }
            }
            Some(EXIT_TOP) => StepPlan::Go {
                at: Position {
                    z: exit[2] + EXIT_STAND_M,
                    ..position_of(exit)
                },
                territory: None,
            },
            Some(_) => StepPlan::Wait,
            None => StepPlan::Go {
                at: place,
                territory: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use nw_content::instances::CATACOMBS_GAME_MODE;

    use super::super::super::gm_chat::QUEST_RUNNING;
    use super::super::tests::{PLAYER, arrived_run};
    use super::*;

    /// A player who arrived plans the boss room's fight first, and the run is not done for it.
    #[test]
    fn a_new_run_plans_the_boss_room_first() {
        let (mut world, _) = arrived_run();
        let now_ms = world.server_ms;
        world.tick_dungeon_script(now_ms);
        let dungeon = world.dungeon_run.as_ref().expect("script");
        let boss = dungeon
            .script()
            .encounters
            .iter()
            .find(|encounter| encounter.name == BOSS_ENCOUNTER)
            .expect("a boss encounter");
        let boss_at = position_of(boss.position);
        let plan = world.catacomb_plan(PLAYER).expect("a catacombs plan");
        let at = match &plan {
            StepPlan::Kill { spawn, .. } => position_of(spawn.position),
            StepPlan::Go { at, .. } => *at,
            other => panic!("not the boss room: {other:?}"),
        };
        assert!(
            at.horizontal_distance(boss_at) <= volume_radius_m(boss).max(1.0),
            "{at:?} vs {boss_at:?}"
        );
        assert_eq!(world.catacomb_run_left(PLAYER), Some(false));
        assert_eq!(
            world.run_state_lines(PLAYER, "run:Catacombs", CATACOMBS_GAME_MODE),
            [format!("{QUEST_RUNNING} run:Catacombs")]
        );
    }

    #[test]
    fn the_core_is_the_extraction_initiator_slice() {
        assert!(is_core(
            "slices/catacombs/gameplay/extractiongameplay/catacombs_extraction_initiator_a.dynamicslice"
        ));
        assert!(!is_core(
            "slices/catacombs/gameplay/extractiongameplay/catacombs_extraction_exit.dynamicslice"
        ));
        assert_eq!(CORE_TOP, 1, "Setup, then Activate core");
    }
}
