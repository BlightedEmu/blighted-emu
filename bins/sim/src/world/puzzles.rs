//! The puzzles of server slayer scripts (`TaskSlayerScript`, the script in its `ItemName`): what
//! a player does to solve one, and the sim's [`Event::PuzzleSolved`] for the player's quests
//! when the player did it.
//!
//! No file of the client or of `Server.pak` holds the scripts `PuzzleMSQGong` and
//! `PuzzleMSQAttackMentally`; their rules here come from the retail captures:
//!
//! - `PuzzleMSQGong` (`99F_MSQ_ER_MeetWiseman_Puzzle`, capture `99f_msq_er_meetwiseman`): the
//!   player struck the three gongs (the quest gatherables `99F_MSQ_02_Puzzle1` to `3`) with
//!   weapon hits; each hit came back as `DamageDealt` (2071) and `GatheringInterrupted` (2724)
//!   on the gong, at 340.729 s (gong 2), 347.060 s (gong 1) and 353.460 s (gong 3), the player
//!   0.9 to 1.8 m from the gong. The task ended at 353.484 s, with the third hit (Observed).
//!   Any order is Inferred: the capture holds one run, in the order 2, 1, 3.
//! - `PuzzleMSQAttackMentally` (`99G_MSQ_SM_AttackMentally_CompletePuzzle`, capture
//!   `99g_msq_sm_attackmentally`): the player gathered three altars (2 s gathers); each gather's
//!   end gave the achievement of the next `PuzzleAchievement` task within 40 ms
//!   (`99G_MSQ_SM_AttackMentally_Altar_A` at 176.082 s, `_B` at 204.180 s, `_C` at
//!   236.446 s), and `CompletePuzzle` ended 15.07 s after the third (251.512 s). Inferred: the
//!   altar of each achievement (Brooch A, Log B, Cup C: the task `UsePuzzle1` of the brooch
//!   altar and the `Puzzle1` container share the place 9396.79 8990.15, the log altar's
//!   `UsePuzzle2` and the `Puzzle2` timer 9401.54 8985.03); a wrong order is not refused, as
//!   no capture shows one.
//!
//! The gongs are spawned slices of `master_quest_damageable_instanced_scripted` (variants
//! `99f_gong_a`, `99f_gong_bf`, `99f_gong_c`; the world file's spawned ids are the facet
//! entities of the retail hits, `14d1789a25a8180f` and so on), so a strike goes through the
//! quest object hits, which send the same 2071 and 2724 as retail.

use nw_formats::objectstream::lowercase_crc32;

use super::SimWorld;
use crate::objectives::Event;

/// How a puzzle's objects are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PuzzleUse {
    /// A weapon hit of the player on the object (a quest object hit, the swing reach of
    /// `super::quest_objects`).
    Strike,
    /// The end of the player's gather of the object.
    Gather,
}

/// One object of a puzzle: its gatherable, and the achievement its use gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PuzzleObject {
    /// The `GatherableID` of the object's gatherable points.
    pub gatherable: &'static str,
    /// The achievement the object's use gives, when it gives one.
    pub achievement: Option<&'static str>,
}

/// The puzzle of one slayer script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Puzzle {
    /// The script (`TaskSlayerScript` `ItemName`).
    pub script: &'static str,
    /// How its objects are used.
    pub used: PuzzleUse,
    /// Its objects: the puzzle is solved when each was used once, in any order.
    pub objects: &'static [PuzzleObject],
    /// How long after the last object's use the puzzle is solved, ms.
    pub solved_after_ms: u64,
}

/// The 15.07 s from the third altar's achievement to `CompletePuzzle` (Observed).
const ALTARS_TO_SOLVED_MS: u64 = 15_066;

/// The puzzles the sim runs (module text).
pub(crate) const PUZZLES: &[Puzzle] = &[
    Puzzle {
        script: "PuzzleMSQGong",
        used: PuzzleUse::Strike,
        objects: &[
            PuzzleObject {
                gatherable: "99F_MSQ_02_Puzzle1",
                achievement: None,
            },
            PuzzleObject {
                gatherable: "99F_MSQ_02_Puzzle2",
                achievement: None,
            },
            PuzzleObject {
                gatherable: "99F_MSQ_02_Puzzle3",
                achievement: None,
            },
        ],
        solved_after_ms: 0,
    },
    Puzzle {
        script: "PuzzleMSQAttackMentally",
        used: PuzzleUse::Gather,
        objects: &[
            PuzzleObject {
                gatherable: "99G_MSQ_SM_AttackMentally_BroochAltar",
                achievement: Some("99G_MSQ_SM_AttackMentally_Altar_A"),
            },
            PuzzleObject {
                gatherable: "99G_MSQ_SM_AttackMentally_LogAltar",
                achievement: Some("99G_MSQ_SM_AttackMentally_Altar_B"),
            },
            PuzzleObject {
                gatherable: "99G_MSQ_SM_AttackMentally_CupAltar",
                achievement: Some("99G_MSQ_SM_AttackMentally_Altar_C"),
            },
        ],
        solved_after_ms: ALTARS_TO_SOLVED_MS,
    },
];

/// The puzzle of slayer script `script` (any case).
pub(crate) fn puzzle_of(script: &str) -> Option<&'static Puzzle> {
    PUZZLES
        .iter()
        .find(|puzzle| puzzle.script.eq_ignore_ascii_case(script))
}

/// The puzzle object whose use gives achievement `achievement` (any case).
pub(crate) fn object_giving(achievement: &str) -> Option<&'static PuzzleObject> {
    PUZZLES
        .iter()
        .flat_map(|puzzle| puzzle.objects)
        .find(|object| {
            object
                .achievement
                .is_some_and(|own| own.eq_ignore_ascii_case(achievement))
        })
}

/// One player's progress in one puzzle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PuzzleRun {
    /// The objects used, by index in [`Puzzle::objects`].
    used: Vec<usize>,
    /// When the puzzle is solved, once every object was used.
    solved_at_ms: Option<u64>,
}

impl SimWorld {
    /// The puzzles that player `number`'s quests wait on now.
    fn running_puzzles(&self, number: u64) -> Vec<&'static Puzzle> {
        let Some(quests) = self.quests.get(&number) else {
            return Vec::new();
        };
        PUZZLES
            .iter()
            .filter(|puzzle| {
                quests
                    .iter()
                    .any(|quest| quest.run.runs_puzzle(puzzle.script))
            })
            .collect()
    }

    /// Whether gatherable `crc` (CRC-32 of its lowercase id) is an object of a gather puzzle
    /// that player `number`'s quests wait on: its gather is open to the player, though no
    /// active task names its interact tag (the altars' `UsePuzzle` tasks are in no container).
    pub(super) fn puzzle_opens(&self, number: u64, crc: u32) -> bool {
        self.running_puzzles(number).iter().any(|puzzle| {
            puzzle.used == PuzzleUse::Gather
                && puzzle
                    .objects
                    .iter()
                    .any(|object| lowercase_crc32(object.gatherable) == crc)
        })
    }

    /// Player `number`'s gather of gatherable `gatherable` ended at `now_ms`.
    pub(super) fn puzzle_gathered(&mut self, number: u64, gatherable: &str, now_ms: u64) {
        for puzzle in self.running_puzzles(number) {
            if puzzle.used != PuzzleUse::Gather {
                continue;
            }
            if let Some(index) = puzzle
                .objects
                .iter()
                .position(|object| object.gatherable.eq_ignore_ascii_case(gatherable))
            {
                self.use_puzzle_object(number, puzzle, index, now_ms);
            }
        }
    }

    /// The objects of the strike puzzles that player `number`'s quests wait on and that the
    /// player has not struck yet: the quest objects its weapon hits strike
    /// ([`SimWorld::strike_quest_objects`]).
    pub(super) fn open_strike_objects(&self, number: u64) -> Vec<&'static str> {
        self.running_puzzles(number)
            .into_iter()
            .filter(|puzzle| puzzle.used == PuzzleUse::Strike)
            .flat_map(|puzzle| {
                let used = self
                    .puzzle_runs
                    .get(&(number, puzzle.script))
                    .map(|run| run.used.clone())
                    .unwrap_or_default();
                puzzle
                    .objects
                    .iter()
                    .enumerate()
                    .filter(move |(index, _)| !used.contains(index))
                    .map(|(_, object)| object.gatherable)
            })
            .collect()
    }

    /// Player `number`'s weapon hit struck the object of gatherable `gatherable` at `now_ms`.
    pub(super) fn puzzle_struck(&mut self, number: u64, gatherable: &str, now_ms: u64) {
        for puzzle in self.running_puzzles(number) {
            if puzzle.used != PuzzleUse::Strike {
                continue;
            }
            if let Some(index) = puzzle
                .objects
                .iter()
                .position(|object| object.gatherable.eq_ignore_ascii_case(gatherable))
            {
                self.use_puzzle_object(number, puzzle, index, now_ms);
            }
        }
    }

    /// Player `number` used object `index` of `puzzle` at `now_ms`: its achievement, once; the
    /// puzzle's solve time once every object was used.
    fn use_puzzle_object(&mut self, number: u64, puzzle: &Puzzle, index: usize, now_ms: u64) {
        let run = self.puzzle_runs.entry((number, puzzle.script)).or_default();
        if run.used.contains(&index) {
            return;
        }
        run.used.push(index);
        let used = run.used.len();
        if used == puzzle.objects.len() {
            run.solved_at_ms = Some(now_ms.saturating_add(puzzle.solved_after_ms));
        }
        tracing::debug!(
            player = number,
            script = puzzle.script,
            object = puzzle.objects[index].gatherable,
            used,
            "puzzle object used"
        );
        if let Some(achievement) = puzzle.objects[index].achievement {
            self.earn_achievement(number, achievement, now_ms);
        }
        self.solve_puzzles_due(now_ms);
    }

    /// The puzzles whose solve time came by `now_ms` are solved for their players; a run of a
    /// player no longer on it goes.
    pub(super) fn solve_puzzles_due(&mut self, now_ms: u64) {
        let due: Vec<(u64, &'static str)> = self
            .puzzle_runs
            .iter()
            .filter(|(_, run)| run.solved_at_ms.is_some_and(|at| at <= now_ms))
            .map(|(key, _)| *key)
            .collect();
        for (number, script) in due {
            self.puzzle_runs.remove(&(number, script));
            tracing::info!(player = number, script, "puzzle solved");
            self.quest_event(number, Event::PuzzleSolved(script), now_ms);
        }
        let running: Vec<(u64, &'static str)> = self.puzzle_runs.keys().copied().collect();
        for (number, script) in running {
            let waits = self
                .running_puzzles(number)
                .iter()
                .any(|puzzle| puzzle.script == script);
            if !waits {
                self.puzzle_runs.remove(&(number, script));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_puzzle_is_found_by_its_script_and_each_altar_by_its_achievement() {
        assert_eq!(
            puzzle_of("puzzlemsqgong").map(|puzzle| puzzle.used),
            Some(PuzzleUse::Strike)
        );
        assert_eq!(
            puzzle_of("PuzzleMSQAttackMentally").map(|puzzle| puzzle.solved_after_ms),
            Some(ALTARS_TO_SOLVED_MS)
        );
        assert!(puzzle_of("PuzzleUnknown").is_none());
        assert_eq!(
            object_giving("99G_MSQ_SM_AttackMentally_Altar_B").map(|object| object.gatherable),
            Some("99G_MSQ_SM_AttackMentally_LogAltar")
        );
        assert!(object_giving("Altar_D").is_none());
    }
}
