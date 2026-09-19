// SPDX-License-Identifier: AGPL-3.0-or-later
//! Events (admin studio spec section 8.3): a GM starts an event of the design data at a point;
//! its waves of GM mobs spawn after their delays, and it ends after its time, when it is cleared,
//! or when a GM stops it. Its mobs end with it.

use std::collections::BTreeMap;

use nw_bus::{Point, RunningEvent};
use nw_common::time::unix_ms;
use nw_world::script::{EventWave, GameEvent};

use super::SimWorld;
use super::scripts::ScriptEvent;

/// One running event.
#[derive(Debug, Clone, PartialEq)]
struct Running {
    id: String,
    at: Point,
    started_unix_ms: u64,
    /// The waves still to spawn, with the sim time each is due.
    waves: Vec<(u64, EventWave)>,
    mobs: Vec<u64>,
    end_at_ms: Option<u64>,
    end_when_cleared: bool,
}

/// The events of the level and the running ones.
#[derive(Debug, Clone, Default)]
pub(crate) struct Events {
    defs: BTreeMap<String, GameEvent>,
    running: Vec<Running>,
}

impl Events {
    /// The events of `defs`, none running.
    pub(crate) fn new(defs: BTreeMap<String, GameEvent>) -> Self {
        Self {
            defs,
            running: Vec::new(),
        }
    }

    fn def(&self, id: &str) -> Option<(&String, &GameEvent)> {
        self.defs
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(id))
    }
}

impl SimWorld {
    /// Starts event `id` at `at`; its `event_starts` scripts run.
    ///
    /// # Errors
    ///
    /// Returns why when the level has no such event or it already runs here.
    pub(super) fn start_event(&mut self, id: &str, at: Point, now_ms: u64) -> Result<(), String> {
        let (name, def) = self
            .events
            .def(id)
            .map(|(name, def)| (name.clone(), def.clone()))
            .ok_or_else(|| format!("no event {id} on this level"))?;
        if self.events.running.iter().any(|event| event.id == name) {
            return Err(format!("event {name} already runs"));
        }
        self.events.running.push(Running {
            id: name.clone(),
            at,
            started_unix_ms: unix_ms(),
            waves: def
                .waves
                .iter()
                .map(|wave| (now_ms.saturating_add(wave.delay_ms), wave.clone()))
                .collect(),
            mobs: Vec::new(),
            end_at_ms: def.end_after_ms.map(|after| now_ms.saturating_add(after)),
            end_when_cleared: def.end_when_cleared,
        });
        tracing::info!(event = %name, "event started");
        self.scripts.push(ScriptEvent::EventStarted { event: name });
        self.tick_events(now_ms);
        Ok(())
    }

    /// Stops event `id` and removes its mobs; whether it ran here.
    pub(super) fn stop_event(&mut self, id: &str) -> bool {
        let Some(index) = self
            .events
            .running
            .iter()
            .position(|event| event.id.eq_ignore_ascii_case(id))
        else {
            return false;
        };
        let event = self.events.running.remove(index);
        self.remove_gm(&event.mobs);
        tracing::info!(event = %event.id, "event stopped");
        true
    }

    /// The events running here.
    pub(super) fn running_events(&self) -> Vec<RunningEvent> {
        self.events
            .running
            .iter()
            .map(|event| RunningEvent {
                event: event.id.clone(),
                started_ms: event.started_unix_ms,
                mobs_alive: u32::try_from(self.alive(&event.mobs)).unwrap_or(u32::MAX),
                waves_left: u32::try_from(event.waves.len()).unwrap_or(u32::MAX),
            })
            .collect()
    }

    /// How many of the mobs `entities` live here.
    pub(super) fn alive(&self, entities: &[u64]) -> usize {
        entities
            .iter()
            .filter(|entity| self.mobs.health(**entity).is_some_and(|health| health > 0))
            .count()
    }

    /// Spawns the waves that are due and ends the events whose time is up or that are cleared.
    pub(super) fn tick_events(&mut self, now_ms: u64) {
        let mut running = std::mem::take(&mut self.events.running);
        running.retain_mut(|event| {
            let (due, later): (Vec<_>, Vec<_>) = event
                .waves
                .drain(..)
                .partition(|(at_ms, _)| *at_ms <= now_ms);
            event.waves = later;
            for (_, wave) in due {
                let at = Point {
                    x: event.at.x + wave.offset[0],
                    y: event.at.y + wave.offset[1],
                    z: event.at.z + wave.offset[2],
                };
                match self.spawn_gm(&wave.vitals, wave.level, at, wave.count) {
                    Ok(entities) => event.mobs.extend(entities),
                    Err(reason) => tracing::warn!(event = %event.id, %reason, "event wave"),
                }
            }
            let timed_out = event.end_at_ms.is_some_and(|end| now_ms >= end);
            let cleared =
                event.end_when_cleared && event.waves.is_empty() && self.alive(&event.mobs) == 0;
            if timed_out || cleared {
                self.remove_gm(&event.mobs);
                tracing::info!(event = %event.id, timed_out, cleared, "event ended");
                return false;
            }
            true
        });
        // An event started by a script during this tick is already in `self.events.running`.
        running.append(&mut self.events.running);
        self.events.running = running;
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{NOW, world_with_spawns};
    use super::*;
    use crate::world::scripts::Scripts;
    use nw_world::script::LevelDesign;

    const EVENT: &str = "invasion";
    const SECOND_WAVE_MS: u64 = 5_000;
    const END_MS: u64 = 60_000;
    const AT: Point = Point {
        x: 100.0,
        y: 100.0,
        z: 10.0,
    };

    fn wave(count: u32, delay_ms: u64) -> EventWave {
        EventWave {
            vitals: crate::combat::test_support::TEST_MOB.to_owned(),
            level: crate::combat::test_support::TEST_LEVEL,
            count,
            delay_ms,
            offset: [0.0, 0.0, 0.0],
        }
    }

    fn event_world(end_when_cleared: bool) -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        let design = LevelDesign {
            events: BTreeMap::from([(
                EVENT.to_owned(),
                GameEvent {
                    waves: vec![wave(2, 0), wave(1, SECOND_WAVE_MS)],
                    end_after_ms: Some(END_MS),
                    end_when_cleared,
                },
            )]),
            ..LevelDesign::default()
        };
        world.events = Events::new(design.events.clone());
        world.scripts = Scripts::new(&design);
        world
    }

    #[test]
    fn waves_spawn_on_time_and_the_event_ends_after_its_time() {
        let mut world = event_world(false);
        assert!(world.start_event("nope", AT, NOW).is_err());
        world.start_event("INVASION", AT, NOW).expect("start");
        assert!(
            world.start_event(EVENT, AT, NOW).is_err(),
            "it already runs"
        );
        let first = &world.running_events()[0];
        assert_eq!((first.mobs_alive, first.waves_left), (2, 1));
        world.tick_events(NOW + SECOND_WAVE_MS);
        assert_eq!(world.running_events()[0].mobs_alive, 3);
        world.tick_events(NOW + END_MS);
        assert!(world.running_events().is_empty());
        assert!(world.mobs.gm_mobs().is_empty(), "its mobs end with it");
    }

    #[test]
    fn a_stopped_event_removes_its_mobs() {
        let mut world = event_world(false);
        world.start_event(EVENT, AT, NOW).expect("start");
        assert!(world.stop_event(EVENT));
        assert!(!world.stop_event(EVENT));
        assert!(world.mobs.gm_mobs().is_empty());
    }

    #[test]
    fn a_cleared_event_ends_when_its_last_mob_dies() {
        let mut world = event_world(true);
        world.start_event(EVENT, AT, NOW).expect("start");
        world.tick_events(NOW + SECOND_WAVE_MS);
        for entity in world.mobs.gm_mobs() {
            world.remove_gm(&[entity]);
        }
        world.tick_events(NOW + SECOND_WAVE_MS + 1);
        assert!(world.running_events().is_empty());
    }
}
