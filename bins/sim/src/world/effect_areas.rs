// SPDX-License-Identifier: AGPL-3.0-or-later
//! The status effect areas of the level and of the open spell areas' slices
//! ([`nw_world::effect_areas`]; findings `2026-11-13-catacomb-erosion.md` section C10): the
//! `ApplyBlight`, `ApplyCurse`, `Env_SulfurPool_DoT` and `Fatigue` volumes of the level's chunks
//! and the Fulgoris gas and spew pools. Each gives its effects to the living players in its
//! volume; the affliction runtime ([`super::afflictions`]) then ticks their meters.
//!
//! **Observed** (`CATACOMBS6`, the gas of `AI_Fulgoris_Blight_Self_Destruct_Catacombs`, initial
//! and reapplication delays 0.5 s, `m_removeOnExit` off): the player's `AI_Fulgoris_Gas` came
//! again about every 0.5 s while it stood in the gas (applications 1 to 9 from 1310.975 s, each
//! one back to a 19.8 s rest), its blight ticks kept their 1 s beat (+5.0 a second from
//! 1310.842 s), and it held the effect after it left until its 20 s ran out (a new application 1
//! at 1336.306 s).
//!
//! **Observed** (client `FUN_143fe6ca0`, the server facet's entry handler, and `FUN_143ff3af0`,
//! its `RemoveStatusEffects`): an entry waits the area's initial delay, then the area removes its
//! `m_removeStatusEffects` from the character once and gives its effects; only the effects come
//! again. **Observed** (`99a_msq_05_unblockfountain`, the Everfall curse patches): a player in
//! several overlapping areas holds one `ApplyCurseL1` whose wire count is the number of
//! applications not taken back, and leaving one area lowers it without ending the effect
//! ([`super::status_effects::StatusEffects::withdraw`]).
//!
//! The areas an encounter stage spawns ([`nw_world::effect_areas::StageAreas`]) stand while
//! the part of the server that runs the stage keeps them open
//! ([`SimWorld::open_stage_areas`], [`SimWorld::close_stage_areas`]: the invasion's defense
//! stage, [`super::invasions`]); they apply as no one, as the level areas do, and their closing
//! takes back what they gave, as a spell area's end does.
//!
//! **Inferred**: an area without a reapplication delay gives its effects once per entry; a
//! player that leaves an area with `m_removeOnExit` (every level area found), dies in it, or
//! stays in it when its spell ends, loses the applications that area gave; the level areas
//! apply as no one (source 0); a spell's
//! slice areas are not turned with the spell (the ones found are spheres, which a turn does not
//! change).

use std::sync::Arc;

use bevy_ecs::entity::Entity;
use nw_formats::objectstream::lowercase_crc32;
use nw_sim_core::Position;
use nw_world::effect_areas::{AreaRules, EffectArea};

/// The world origin: the place of the level and stage areas, whose volumes are in the world.
const ORIGIN: Position = Position {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

use super::{Body, Player, SimWorld, Vitals};

/// The potency of an area's effects.
const AREA_POTENCY: f64 = 1.0;
/// The source of a level area's effects: no character.
const LEVEL_AREA_SOURCE: u64 = 0;
/// The next application of a character in an area that gives its effects once per entry.
const NEVER_MS: u64 = u64::MAX;

/// A character in an area, when the area next gives it its effects (server ms), and how many
/// applications it gave since the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Inside<T> {
    who: T,
    next_ms: u64,
    given: u8,
}

/// What one step of an area does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AreaStep<T> {
    /// The characters whose first application since their entry comes now: the area removes
    /// its `removes` from them, then gives them its effects.
    entered: Vec<T>,
    /// The characters it gives its effects again.
    again: Vec<T>,
    /// The characters that lose its effects (they left an area with `m_removeOnExit`), each
    /// with the applications the area gave it.
    lost: Vec<(T, u8)>,
}

/// An open spell area whose slice has effect areas.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpellKey {
    source: u64,
    spell: String,
    starts_ms: u64,
}

/// The open areas of an encounter stage, in the world, and the players in each.
#[derive(Debug, Clone)]
struct OpenStage {
    /// The stage's name.
    stage: String,
    areas: Vec<EffectArea>,
    runs: Vec<Vec<Inside<Entity>>>,
}

/// The players in each effect area.
#[derive(Debug, Clone, Default)]
pub(super) struct EffectAreaRuns {
    /// By level area, in the file's order.
    level: Vec<Vec<Inside<Entity>>>,
    /// By open spell area, then by area of its slice.
    spells: Vec<(SpellKey, Vec<Vec<Inside<Entity>>>)>,
    /// The open stage areas, in the order they opened.
    stages: Vec<OpenStage>,
}

/// One step of an area with `rules` at `now_ms`, whose volume holds `present` now (module
/// text).
fn step<T: Copy + PartialEq>(
    inside: &mut Vec<Inside<T>>,
    present: &[T],
    rules: AreaRules,
    now_ms: u64,
) -> AreaStep<T> {
    let left: Vec<(T, u8)> = inside
        .iter()
        .filter(|held| !present.contains(&held.who))
        .map(|held| (held.who, held.given))
        .collect();
    inside.retain(|held| present.contains(&held.who));
    for who in present {
        if !inside.iter().any(|held| held.who == *who) {
            inside.push(Inside {
                who: *who,
                next_ms: now_ms.saturating_add(rules.initial_delay_ms),
                given: 0,
            });
        }
    }
    let mut entered = Vec::new();
    let mut again = Vec::new();
    for held in inside.iter_mut().filter(|held| now_ms >= held.next_ms) {
        if held.given == 0 {
            entered.push(held.who);
        } else {
            again.push(held.who);
        }
        held.given = held.given.saturating_add(1);
        held.next_ms = if rules.reapply_ms == 0 {
            NEVER_MS
        } else {
            now_ms.saturating_add(rules.reapply_ms)
        };
    }
    AreaStep {
        entered,
        again,
        lost: if rules.remove_on_exit { left } else { Vec::new() },
    }
}

impl SimWorld {
    /// One step of every effect area at the server time: the level's areas, and the areas of
    /// the slices of the open spell areas (module text).
    pub(super) fn tick_effect_areas(&mut self) {
        let content = Arc::clone(&self.content);
        let areas = &content.effect_areas;
        if areas.is_empty() {
            return;
        }
        let now_ms = self.server_ms;
        let players: Vec<(Entity, Position)> = self
            .world
            .query::<(Entity, &Player, &Body, &Vitals)>()
            .iter(&self.world)
            .filter(|(_, _, _, vitals)| !vitals.health.is_dead())
            .map(|(entity, _, body, _)| (entity, body.motion.position()))
            .collect();
        let within = |area: &EffectArea, centre: Position| -> Vec<Entity> {
            players
                .iter()
                .filter(|(_, at)| area.contains([at.x - centre.x, at.y - centre.y, at.z - centre.z]))
                .map(|(entity, _)| *entity)
                .collect()
        };
        let level: Vec<&EffectArea> = areas.levels.values().flatten().collect();
        self.effect_area_runs.level.resize(level.len(), Vec::new());
        for (index, area) in level.into_iter().enumerate() {
            let present = within(area, ORIGIN);
            let done = step(&mut self.effect_area_runs.level[index], &present, area.rules, now_ms);
            self.run_effect_area(area, LEVEL_AREA_SOURCE, &done, now_ms);
        }
        let mut stages = std::mem::take(&mut self.effect_area_runs.stages);
        for open in &mut stages {
            for (area, run) in open.areas.iter().zip(&mut open.runs) {
                let present = within(area, ORIGIN);
                let done = step(run, &present, area.rules, now_ms);
                self.run_effect_area(area, LEVEL_AREA_SOURCE, &done, now_ms);
            }
        }
        self.effect_area_runs.stages = stages;
        let open: Vec<(SpellKey, Position)> = self
            .spell_areas
            .iter()
            .filter_map(|spell_area| spell_area.open_slice(now_ms))
            .filter(|(_, spell, _, _)| !areas.of_spell(spell).is_empty())
            .map(|(source, spell, starts_ms, centre)| {
                (
                    SpellKey {
                        source,
                        spell: spell.to_owned(),
                        starts_ms,
                    },
                    centre,
                )
            })
            .collect();
        let (kept, ended): (Vec<_>, Vec<_>) = std::mem::take(&mut self.effect_area_runs.spells)
            .into_iter()
            .partition(|(key, _)| open.iter().any(|(open, _)| open == key));
        self.effect_area_runs.spells = kept;
        for (key, runs) in ended {
            for (area, run) in areas.of_spell(&key.spell).iter().zip(runs) {
                self.close_effect_area(area, key.source, &run, now_ms);
            }
        }
        for (key, centre) in open {
            let slice_areas = areas.of_spell(&key.spell);
            let index = match self
                .effect_area_runs
                .spells
                .iter()
                .position(|(known, _)| *known == key)
            {
                Some(index) => index,
                None => {
                    self.effect_area_runs
                        .spells
                        .push((key.clone(), vec![Vec::new(); slice_areas.len()]));
                    self.effect_area_runs.spells.len() - 1
                }
            };
            for (slot, area) in slice_areas.iter().enumerate() {
                let present = within(area, centre);
                let done = step(
                    &mut self.effect_area_runs.spells[index].1[slot],
                    &present,
                    area.rules,
                    now_ms,
                );
                self.run_effect_area(area, key.source, &done, now_ms);
            }
        }
    }

    /// Opens the areas that the stages named `stage` spawn and that stand where `within` holds
    /// (module text); they give their effects from the next step on. A stage that is open
    /// already stays as it is.
    pub(super) fn open_stage_areas(&mut self, stage: &str, within: impl Fn([f32; 3]) -> bool) {
        if self
            .effect_area_runs
            .stages
            .iter()
            .any(|open| open.stage.eq_ignore_ascii_case(stage))
        {
            return;
        }
        let areas: Vec<EffectArea> = self
            .content
            .effect_areas
            .of_stage(stage)
            .filter(|area| within(area.centre()))
            .cloned()
            .collect();
        if areas.is_empty() {
            return;
        }
        tracing::info!(stage, areas = areas.len(), "stage areas open");
        self.effect_area_runs.stages.push(OpenStage {
            stage: stage.to_owned(),
            runs: vec![Vec::new(); areas.len()],
            areas,
        });
    }

    /// Closes the open areas of the stages named `stage`: the players in one with
    /// `m_removeOnExit` give back the applications it gave them.
    pub(super) fn close_stage_areas(&mut self, stage: &str) {
        let (closed, open): (Vec<_>, Vec<_>) = std::mem::take(&mut self.effect_area_runs.stages)
            .into_iter()
            .partition(|open| open.stage.eq_ignore_ascii_case(stage));
        self.effect_area_runs.stages = open;
        let now_ms = self.server_ms;
        for closed in closed {
            for (area, run) in closed.areas.iter().zip(&closed.runs) {
                self.close_effect_area(area, LEVEL_AREA_SOURCE, run, now_ms);
            }
        }
    }

    /// `area`, whose effects come from `source`, closes at `now_ms` with the players `run` in
    /// it: with `m_removeOnExit` they give back the applications it gave them.
    fn close_effect_area(
        &mut self,
        area: &EffectArea,
        source: u64,
        run: &[Inside<Entity>],
        now_ms: u64,
    ) {
        if !area.rules.remove_on_exit {
            return;
        }
        let done = AreaStep {
            entered: Vec::new(),
            again: Vec::new(),
            lost: run.iter().map(|held| (held.who, held.given)).collect(),
        };
        self.run_effect_area(area, source, &done, now_ms);
    }

    /// Runs step `done` of `area`, whose effects come from `source`, at server ms `now_ms`:
    /// the entered players lose its `removes` and get its effects, the others of `again` get
    /// its effects, and the lost players give back the applications it gave them.
    fn run_effect_area(
        &mut self,
        area: &EffectArea,
        source: u64,
        done: &AreaStep<Entity>,
        now_ms: u64,
    ) {
        let content = Arc::clone(&self.content);
        for entity in &done.entered {
            if let Some(mut player) = self.world.get_mut::<Player>(*entity) {
                for effect in &area.removes {
                    player.status_effects.remove(lowercase_crc32(effect));
                }
            }
        }
        for entity in done.entered.iter().chain(&done.again) {
            if let Some(mut player) = self.world.get_mut::<Player>(*entity) {
                for effect in &area.effects {
                    player.status_effects.add_by_id(
                        &content.status_effects,
                        effect,
                        AREA_POTENCY,
                        source,
                        now_ms,
                    );
                }
            }
        }
        for (entity, given) in done.lost.iter().filter(|(_, given)| *given > 0) {
            if let Some(mut player) = self.world.get_mut::<Player>(*entity) {
                for effect in &area.effects {
                    player.status_effects.withdraw(lowercase_crc32(effect), *given);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Fulgoris gas's rules: 0.5 s before the first application and between two.
    const GAS: AreaRules = AreaRules {
        remove_on_exit: false,
        initial_delay_ms: 500,
        reapply_ms: 500,
    };
    /// A level area's rules (`14ac14_props` `CorruptionComponent`).
    const CORRUPTION: AreaRules = AreaRules {
        remove_on_exit: true,
        initial_delay_ms: 0,
        reapply_ms: 0,
    };
    const PLAYER: u64 = 7;

    /// A step that gives the entered `entered`, again `again` and takes from `lost`.
    fn done(entered: &[u64], again: &[u64], lost: &[(u64, u8)]) -> AreaStep<u64> {
        AreaStep {
            entered: entered.to_vec(),
            again: again.to_vec(),
            lost: lost.to_vec(),
        }
    }

    #[test]
    fn a_gas_gives_its_effect_after_its_delay_and_again_every_half_second_while_held() {
        let mut inside = Vec::new();
        assert_eq!(step(&mut inside, &[PLAYER], GAS, 0), done(&[], &[], &[]));
        assert_eq!(step(&mut inside, &[PLAYER], GAS, 400), done(&[], &[], &[]));
        assert_eq!(
            step(&mut inside, &[PLAYER], GAS, 500),
            done(&[PLAYER], &[], &[]),
            "the first application removes the area's removes"
        );
        assert_eq!(step(&mut inside, &[PLAYER], GAS, 900), done(&[], &[], &[]));
        assert_eq!(
            step(&mut inside, &[PLAYER], GAS, 1_000),
            done(&[], &[PLAYER], &[]),
            "a reapplication only gives the effects"
        );
        assert_eq!(
            step(&mut inside, &[], GAS, 1_100),
            done(&[], &[], &[]),
            "the gas's effect stays after the player leaves"
        );
        assert!(inside.is_empty());
    }

    #[test]
    fn a_level_area_gives_its_effects_once_per_entry_and_takes_them_back_at_the_exit() {
        let mut inside = Vec::new();
        assert_eq!(step(&mut inside, &[PLAYER], CORRUPTION, 0), done(&[PLAYER], &[], &[]));
        assert_eq!(step(&mut inside, &[PLAYER], CORRUPTION, 5_000), done(&[], &[], &[]));
        assert_eq!(
            step(&mut inside, &[], CORRUPTION, 6_000),
            done(&[], &[], &[(PLAYER, 1)]),
            "the exit takes back the one application"
        );
        assert_eq!(
            step(&mut inside, &[PLAYER], CORRUPTION, 7_000),
            done(&[PLAYER], &[], &[]),
            "a new entry gives them again"
        );
    }

    /// A world with one player and the status effect `ApplyCurseL4`; the file holds one
    /// `Setup` stage area around the whole test world (Observed: the corruption area's rules,
    /// remove on exit, delays 0).
    fn invasion_world() -> (SimWorld, Entity) {
        use nw_world::collision::{Geometry, IDENTITY_AFFINE};
        use nw_world::dungeon_scripts::Detector;
        use nw_world::effect_areas::StageAreas;

        use crate::world::afflictions::tests::effect_row;
        use crate::world::tests::{NOW, SERVER_TIME, admit_at, world_with_spawns};
        /// A sphere around the whole test world, metres.
        const WIDE_M: f32 = 100_000.0;
        let mut world = world_with_spawns(Vec::new());
        let content = Arc::make_mut(&mut world.content);
        content.status_effects =
            nw_content::status_effects::StatusEffectRules::from_tables(&nw_content::Tables {
                status_effect_data: vec![nw_content::table::Table {
                    source_path: String::new(),
                    unique_id: String::new(),
                    rows: vec![effect_row(CORRUPTION_EFFECT, -1.0)],
                }],
                ..nw_content::Tables::default()
            });
        content.effect_areas.stages.insert(
            String::new(),
            vec![StageAreas {
                stage: SETUP.to_owned(),
                areas: vec![EffectArea {
                    effects: vec![CORRUPTION_EFFECT.to_owned()],
                    removes: Vec::new(),
                    volume: Detector {
                        shape: Geometry::Sphere { radius: WIDE_M },
                        place: IDENTITY_AFFINE,
                    },
                    rules: CORRUPTION,
                }],
            }],
        );
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        let entity = world.player_by_number(PLAYER).expect("player");
        (world, entity)
    }

    const SETUP: &str = "Setup";
    const CORRUPTION_EFFECT: &str = "ApplyCurseL4";

    fn applications(world: &SimWorld, entity: Entity) -> Option<u8> {
        world
            .world
            .get::<Player>(entity)
            .and_then(|player| player.status_effects.applications(lowercase_crc32(CORRUPTION_EFFECT)))
    }

    #[test]
    fn a_stage_area_gives_its_effect_while_open_and_takes_it_back_when_it_closes() {
        let (mut world, entity) = invasion_world();
        world.tick_effect_areas();
        assert_eq!(applications(&world, entity), None, "a stage area waits for its stage");
        world.open_stage_areas(SETUP, |_| true);
        world.tick_effect_areas();
        assert_eq!(applications(&world, entity), Some(1));
        world.open_stage_areas(SETUP, |_| true);
        world.tick_effect_areas();
        assert_eq!(applications(&world, entity), Some(1), "an open stage opens once");
        world.close_stage_areas("setup");
        assert_eq!(applications(&world, entity), None, "the closing takes it back");
        world.tick_effect_areas();
        assert_eq!(applications(&world, entity), None);
    }

    #[test]
    fn a_stage_area_outside_the_fort_does_not_open() {
        let (mut world, entity) = invasion_world();
        world.open_stage_areas(SETUP, |_| false);
        world.tick_effect_areas();
        assert_eq!(applications(&world, entity), None);
        world.open_stage_areas("Stage_1", |_| true);
        world.tick_effect_areas();
        assert_eq!(applications(&world, entity), None, "only the named stage opens");
    }

    #[test]
    fn an_exit_before_the_first_application_takes_back_nothing() {
        let delayed = AreaRules {
            initial_delay_ms: 500,
            ..CORRUPTION
        };
        let mut inside = Vec::new();
        assert_eq!(step(&mut inside, &[PLAYER], delayed, 0), done(&[], &[], &[]));
        assert_eq!(step(&mut inside, &[], delayed, 100), done(&[], &[], &[(PLAYER, 0)]));
    }
}
