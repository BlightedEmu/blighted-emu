// SPDX-License-Identifier: AGPL-3.0-or-later
//! The AI profiles and patrol paths of the design files (admin studio spec sections 6.4, 6.5),
//! resolved once at sim start into each vitals spawn's behaviour, brain numbers and route.

use std::sync::Arc;

use nw_sim_core::{Behaviour, BrainSettings, Gait, Position, Route, RouteMode};
use nw_world::WorldFile;
use nw_world::records::{AiBehaviour, AiProfile, PathMode, PatrolPath};

/// The AI of one vitals spawn that a design file changed.
#[derive(Debug, Clone, PartialEq)]
pub struct MobAi {
    /// What it does without a target.
    pub behaviour: Behaviour,
    /// Its brain numbers: the sim's, with the profile's on top.
    pub settings: BrainSettings,
    /// Its sight, when the profile sets one.
    pub sight_m: Option<f32>,
    /// The route it patrols.
    pub route: Option<Route>,
}

/// The AI of each vitals spawn of `world`, in spawn order; `None` keeps the sim's `base` brain.
/// A spawn with a path and no other behaviour patrols it.
pub fn resolve_ai(world: &WorldFile, base: &BrainSettings) -> Arc<[Option<MobAi>]> {
    world
        .vitals_ai
        .iter()
        .map(|ai| {
            if ai.profile.is_none() && ai.path.is_none() {
                return None;
            }
            let profile = ai
                .profile
                .as_ref()
                .and_then(|id| world.ai_profiles.get(id))
                .cloned()
                .unwrap_or_default();
            let route = ai
                .path
                .as_ref()
                .and_then(|id| world.paths.get(id))
                .map(route_of);
            let behaviour = match profile.behaviour {
                AiBehaviour::Wander if route.is_some() => Behaviour::Patrol,
                AiBehaviour::Wander => Behaviour::Wander,
                AiBehaviour::Guard => Behaviour::Guard,
                AiBehaviour::Patrol => Behaviour::Patrol,
                AiBehaviour::Idle => Behaviour::Idle,
            };
            Some(MobAi {
                behaviour,
                settings: settings_of(&profile, base),
                sight_m: profile.sight_m,
                route,
            })
        })
        .collect()
}

/// `base` with the numbers of `profile`; a new reach or speed makes a new uniform gait, as the
/// sim's `[combat]` does.
fn settings_of(profile: &AiProfile, base: &BrainSettings) -> BrainSettings {
    let attack_reach_m = profile.attack_reach_m.unwrap_or(base.attack_reach_m);
    let mob_speed_mps = profile.speed_mps.unwrap_or(base.mob_speed_mps);
    let gait = if profile.attack_reach_m.is_some() || profile.speed_mps.is_some() {
        Gait::uniform(attack_reach_m, mob_speed_mps)
    } else {
        base.gait
    };
    BrainSettings {
        wake_radius_m: profile.wake_radius_m.unwrap_or(base.wake_radius_m),
        wander_radius_m: profile.wander_radius_m.unwrap_or(base.wander_radius_m),
        wander_pause_ms: profile.wander_pause_ms.unwrap_or(base.wander_pause_ms),
        leash_radius_m: profile.leash_radius_m.unwrap_or(base.leash_radius_m),
        attack_reach_m,
        mob_attack_interval_ms: profile
            .attack_interval_ms
            .unwrap_or(base.mob_attack_interval_ms),
        mob_speed_mps,
        gait,
        mob_respawn_ms: profile.respawn_ms.unwrap_or(base.mob_respawn_ms),
        ..*base
    }
}

fn route_of(path: &PatrolPath) -> Route {
    Route {
        points: path
            .points
            .iter()
            .map(|&[x, y, z]| Position { x, y, z })
            .collect(),
        waits_ms: path.waits_ms.clone(),
        mode: match path.mode {
            PathMode::Loop => RouteMode::Loop,
            PathMode::BackAndForth => RouteMode::BackAndForth,
            PathMode::Once => RouteMode::Once,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nw_world::records::SpawnAi;

    use super::*;

    const BASE_REACH_M: f32 = 3.0;
    const BASE_SPEED_MPS: f32 = 3.25;

    fn base() -> BrainSettings {
        BrainSettings {
            wake_radius_m: 100.0,
            sleep_after_ms: 30_000,
            base_sight_m: 20.0,
            wander_radius_m: 10.0,
            wander_pause_ms: 5_000,
            leash_radius_m: 50.0,
            attack_reach_m: BASE_REACH_M,
            mob_attack_interval_ms: 2_000,
            mob_speed_mps: BASE_SPEED_MPS,
            gait: Gait::uniform(BASE_REACH_M, BASE_SPEED_MPS),
            mob_respawn_ms: 360_000,
        }
    }

    fn world(vitals_ai: Vec<SpawnAi>) -> WorldFile {
        WorldFile {
            ai_profiles: BTreeMap::from([
                (
                    "fast_guard".to_owned(),
                    AiProfile {
                        behaviour: AiBehaviour::Guard,
                        speed_mps: Some(5.0),
                        respawn_ms: Some(1_000),
                        sight_m: Some(30.0),
                        ..AiProfile::default()
                    },
                ),
                ("empty".to_owned(), AiProfile::default()),
            ]),
            paths: BTreeMap::from([(
                "walk".to_owned(),
                PatrolPath {
                    points: vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
                    waits_ms: vec![100],
                    mode: PathMode::BackAndForth,
                },
            )]),
            vitals_ai,
            ..WorldFile::default()
        }
    }

    fn spawn_ai(profile: Option<&str>, path: Option<&str>) -> SpawnAi {
        SpawnAi {
            profile: profile.map(str::to_owned),
            path: path.map(str::to_owned),
        }
    }

    #[test]
    fn a_profile_sets_behaviour_and_numbers_over_the_base() {
        let ai = resolve_ai(&world(vec![spawn_ai(Some("fast_guard"), None)]), &base());
        let mob = ai[0].as_ref().expect("ai");
        assert_eq!(mob.behaviour, Behaviour::Guard);
        assert_eq!(mob.settings.mob_speed_mps, 5.0);
        assert_eq!(mob.settings.mob_respawn_ms, 1_000);
        assert_eq!(mob.settings.gait, Gait::uniform(BASE_REACH_M, 5.0));
        assert_eq!(mob.settings.leash_radius_m, base().leash_radius_m);
        assert_eq!(mob.sight_m, Some(30.0));
        assert_eq!(mob.route, None);
    }

    #[test]
    fn an_empty_profile_keeps_every_base_number() {
        let ai = resolve_ai(&world(vec![spawn_ai(Some("empty"), None)]), &base());
        assert_eq!(ai[0].as_ref().expect("ai").settings, base());
    }

    #[test]
    fn a_path_without_a_behaviour_patrols_it() {
        let ai = resolve_ai(
            &world(vec![spawn_ai(None, None), spawn_ai(None, Some("walk"))]),
            &base(),
        );
        assert_eq!(ai[0], None);
        let mob = ai[1].as_ref().expect("ai");
        assert_eq!(mob.behaviour, Behaviour::Patrol);
        let route = mob.route.as_ref().expect("route");
        assert_eq!(route.points.len(), 2);
        assert_eq!(route.mode, RouteMode::BackAndForth);
        assert_eq!(route.waits_ms, [100]);
    }
}
