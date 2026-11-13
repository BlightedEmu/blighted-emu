// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Baroness of the Outpost Rush maps (Outpost Rush study 2026-11-12, part 3,
//! `local-data/findings/2026-11-12-outpost-rush.md`): her spawn on the match's timer, her
//! barriers on the island, her death, the freeze of the other team's score and her return.
//!
//! Observed (retail island matches; seconds from each ledger's first record; the key names are
//! the CRC-32 of the lowercase words):
//!
//! - **Who she is**: the mob `OR_Boss_TBD` (vitals `f68a05b6`, `slices/characters/outpostrush/
//!   or_ghost_boss.dynamicslice`, AI tree `outpost_rush/boss/or_boss_base`), 330,960 health at
//!   her spawn (`OPR---ISLAND-2` 662.656 s), the max health of level 68 and of no other level
//!   for her vitals row (`health_mod` 0.5, the `Boss` creature type's 40).
//! - **Where**: the `BOSS` entity of the island's layout slice (178.725, 61.094, -6.138 in the
//!   slice, plus the slice's capital 1272.7565, 11016.635, 68.71661): retail first places
//!   1451, 11078, 62 to 63 (`OPR---ISLAND-2` 662.7 s, `OPR_TEST3` 809.7 s) and her loot bag at
//!   1451.4812, 11077.729, 62.57 (`OPR---ISLAND-2` 695.540 s).
//! - **Trench and Vertical**: the same mob (vitals `f68a05b6`, `or_ghost_boss`) on the same
//!   timers: 330,960 health at her spawn on Trench (`OPR_BAGGUNS` 644.6 s; 327,997 on Vertical
//!   0.1 s after her card, `opr_que_and_lost` 1265.5 s), `bossspawned` 600 s after the start
//!   (`opr_que_and_lost` start 665.446 s, event 1265.479 s), `bosskilled` (`79`: 60 s) at
//!   1315.753 s and `bossspawned` again 600 s later (1915.757 s); her first places, to the
//!   metre: Trench 1040, 1000, 41 (`OPR_BAGGUNS` 644.7, 667.6, 727.8 s; `OPR-Coral-Divide-2`
//!   664.1 s), Vertical 992, 1052, 137 (`opr_que_and_lost` 1265.5 and 1915.3 s, `opr_match_4`
//!   634.8 s, `OPR---Tower` 221.9 s); x and y are the `BOSS` entities of the slices
//!   `outpostrush_layout_004_trench` (15.91, -23.79 from the map's middle 1024, 1024) and
//!   `gamemode_outpostrush_005_vertical` (-32.20, 28.13). No barrier stands around her there
//!   (no barrier asset in either capture).
//! - **The spawn**: no summon; the timer `bossspawntimer` (`e7c0ae3d`) runs from the start of
//!   the match (state 0) for 600 s (`OPR_TEST3`: start 187.097 s, timer end 787.097 s; the
//!   timers of `OPR---ISLAND` and `OPR---ISLAND-2` end 2,100 s before `TimeLimit`, 600 s after
//!   the start). At its end the timer leaves the list, the event `bossspawned` (`3c2675ad`)
//!   follows and her card comes 16 ms later (`OPR---ISLAND-2` 662.607, 662.640, 662.656 s;
//!   `OPR_TEST3` 787.106, 787.206 s; `OPR_Original_map` 394.060 s).
//! - **The death**: the event `bosskilled` (`de640472`), the value `frozenstatus` (`d6d5afdb`)
//!   set to a team, `bossfreezetimer` (`cb39d465`) on, and `bossspawntimer` on again exactly
//!   600 s after the death (3 of 3: `OPR---ISLAND-2` 695.540 s, `OPR_TEST3` 836.139 s and
//!   1463.566 s). The frozen team's score does not rise while the timer runs (`OPR_TEST3` team 0
//!   at 325 from 836.139 s to 897.504 s while team 1 rose from 323 to 365); at its end the
//!   timer leaves and `frozenstatus` is 255 (`OPR---ISLAND-2` 755.537 s, `OPR_TEST3` 897.504 s,
//!   1585.648 s).
//! - **The freeze length**: 60 s and 0.45 s for each point the frozen team led by at the death
//!   (3 of 3 within 1 ms: 60.000 s with the frozen team 156 points behind,
//!   `OPR---ISLAND-2`; 61.349 s with a lead of 3, `OPR_TEST3` 836.139 s; 122.099 s with a lead
//!   of 138, `OPR_TEST3` 1463.566 s); the number of the `bosskilled` event packs the freeze's
//!   seconds and the killing team ([`super::outpost_rush_events`]).
//! - **The barriers**: [`super::outpost_rush_doors`] (`Barrier_Baroness_01` and `_02`).
//! - **The reward**: her loot bag (her loot table `OutpostRush_Large_Creatures`, a privileged
//!   entry of the killer's loot tracker, `OPR---ISLAND-2` 695.540 s), dropped by the shared mob
//!   loot ([`super::loot_bag`]); no status effect reached a player at her death (the own
//!   player's 4236 from 690 to 760 s, `OPR---ISLAND-2`): the freeze of the other team is the
//!   killing team's gain.
//!
//! Inferred: the frozen team is the other team of the player whose hit killed her (the frozen
//! team of each retail death was the team that gained no points; who hit last is not in the
//! captures); a frozen team also gains no kill points; the lead is counted from the scores at
//! the death; a death by no player of the match freezes no team and sends no `bosskilled` (no
//! retail death had no killer); her level on Vertical is Trench's.

use super::SimWorld;
use super::outpost_rush::{TRENCH_FOLDER, VERTICAL_FOLDER};
use super::outpost_rush_doors::ISLAND_FOLDER;
use super::outpost_rush_events::{BOSS_KILLED, BOSS_SPAWNED, NO_NUMBER, boss_killed_number};
use nw_bus::Point;
use nw_messages::game_mode_state::Keyed;

/// `bossspawntimer`.
pub(super) const BOSS_SPAWN_TIMER: u32 = 0xe7c0_ae3d;
/// `bossfreezetimer`.
pub(super) const BOSS_FREEZE_TIMER: u32 = 0xcb39_d465;
/// `frozenstatus`.
pub(super) const FROZEN_STATUS: u32 = 0xd6d5_afdb;
/// The `frozenstatus` of no frozen team (Observed: 255 after each freeze).
const NO_FROZEN_TEAM: u8 = 0xff;
/// From the start of the match, and from each death, to her spawn (Observed).
const SPAWN_DELAY_MS: u64 = 600_000;
/// The freeze with no lead, and its length for each point of lead (Observed).
const FREEZE_BASE_MS: u64 = 60_000;
const FREEZE_MS_PER_POINT: u64 = 450;
/// Her vitals id and level (Observed).
const BARONESS_VITALS: &str = "OR_Boss_TBD";
const BARONESS_LEVEL: u32 = 68;
/// One mob.
const ONE: u32 = 1;
/// Her spawn place on each map (Observed): on the island the layout's `BOSS` entity plus the
/// slice's capital; on Trench and Vertical her first retail places.
const BARONESS_SPOTS: [(&str, Point); 3] = [
    (
        ISLAND_FOLDER,
        Point {
            x: 1451.4815,
            y: 11077.729,
            z: 62.578_61,
        },
    ),
    (
        TRENCH_FOLDER,
        Point {
            x: 1040.0,
            y: 1000.0,
            z: 41.0,
        },
    ),
    (
        VERTICAL_FOLDER,
        Point {
            x: 992.0,
            y: 1052.0,
            z: 137.0,
        },
    ),
];

/// The Baroness of one match.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct BaronessRun {
    /// Her vitals id, her level and her spawn place on the match's map.
    vitals: &'static str,
    level: u32,
    spot: Point,
    /// Server ms of her next spawn (`bossspawntimer`); `None` before the start and while she
    /// lives.
    next_spawn_ms: Option<u64>,
    /// Her mob entity while she lives.
    boss: Option<u64>,
    /// The frozen team and the server ms its freeze ends.
    freeze: Option<(u8, u64)>,
    /// `frozenstatus` once her first death set it.
    frozen_status: Option<u8>,
    /// The slot keys of her barriers.
    pub(super) barriers: Vec<u64>,
}

/// The freeze of a team that led the killing team by `lead` points (negative: it was behind).
pub(super) fn freeze_ms(lead: i64) -> u64 {
    FREEZE_BASE_MS + u64::try_from(lead).unwrap_or_default() * FREEZE_MS_PER_POINT
}

impl BaronessRun {
    /// The Baroness of a match on the map in `folder` with the barriers `barriers`; `None` on
    /// a map with no place for her.
    pub(super) fn of_map(folder: &str, barriers: Vec<u64>) -> Option<Self> {
        let (_, spot) = BARONESS_SPOTS.iter().find(|(map, _)| *map == folder)?;
        Some(Self {
            vitals: BARONESS_VITALS,
            level: BARONESS_LEVEL,
            spot: *spot,
            next_spawn_ms: None,
            boss: None,
            freeze: None,
            frozen_status: None,
            barriers,
        })
    }

    /// The match started at server ms `now_ms`: her timer runs.
    pub(super) fn start(&mut self, now_ms: u64) {
        self.next_spawn_ms = Some(now_ms.saturating_add(SPAWN_DELAY_MS));
    }

    /// The team whose score is frozen.
    pub(super) fn frozen_team(&self) -> Option<u8> {
        self.freeze.map(|(team, _)| team)
    }

    /// Her values: `frozenstatus` once set.
    pub(super) fn values(&self) -> Vec<Keyed> {
        self.frozen_status
            .map(|team| (FROZEN_STATUS, u64::from(team)))
            .into_iter()
            .collect()
    }

    /// Her timers: the spawn timer while it runs and the freeze timer while a team is frozen.
    pub(super) fn timers(&self) -> Vec<Keyed> {
        let spawn = self.next_spawn_ms.map(|at| (BOSS_SPAWN_TIMER, at));
        let freeze = self.freeze.map(|(_, until)| (BOSS_FREEZE_TIMER, until));
        spawn.into_iter().chain(freeze).collect()
    }

    /// Whether her spawn is due at `now_ms`.
    fn spawn_due(&self, now_ms: u64) -> bool {
        self.boss.is_none() && self.next_spawn_ms.is_some_and(|at| now_ms >= at)
    }

    /// Whether the freeze ended at `now_ms`; it ends then.
    fn end_freeze(&mut self, now_ms: u64) -> bool {
        if self.freeze.is_some_and(|(_, until)| now_ms >= until) {
            self.freeze = None;
            self.frozen_status = Some(NO_FROZEN_TEAM);
            return true;
        }
        false
    }

    /// She died at `now_ms` with `frozen` the team to freeze (`None`: no player of the match
    /// killed her) and `lead` its lead over the killing team: the freeze and her next spawn.
    pub(super) fn died(&mut self, now_ms: u64, frozen: Option<u8>, lead: i64) {
        self.boss = None;
        self.next_spawn_ms = Some(now_ms.saturating_add(SPAWN_DELAY_MS));
        if let Some(team) = frozen {
            self.freeze = Some((team, now_ms.saturating_add(freeze_ms(lead))));
            self.frozen_status = Some(team);
        }
    }
}

impl SimWorld {
    /// The island match's Baroness at server ms `now_ms`: she spawns when her timer ends, and
    /// the freeze ends at its time. Whether a value or a timer changed.
    pub(super) fn tick_baroness(&mut self, now_ms: u64) -> bool {
        let Some(run) = self
            .arena_matches
            .outpost_rush
            .as_mut()
            .and_then(|running| running.baroness.as_mut())
        else {
            return false;
        };
        let ended = run.end_freeze(now_ms);
        if !run.spawn_due(now_ms) {
            return ended;
        }
        let (vitals, level, spot) = (run.vitals, run.level, run.spot);
        match self.spawn_gm(vitals, level, spot, ONE, None) {
            Ok(spawned) => {
                if let Some(running) = self.arena_matches.outpost_rush.as_mut()
                    && let Some(run) = running.baroness.as_mut()
                {
                    run.boss = spawned.first().copied();
                    run.next_spawn_ms = None;
                    running.feed.note(BOSS_SPAWNED, now_ms, NO_NUMBER);
                }
                tracing::info!("the Baroness spawned");
                true
            }
            Err(reason) => {
                tracing::warn!(%reason, "the Baroness did not spawn");
                ended
            }
        }
    }

    /// Mob `mob` died by a hit of player `killer`: when it is the Baroness, the killer's foe
    /// team is frozen for its lead and she comes back later. Whether it was she.
    pub(super) fn baroness_killed(&mut self, mob: u64, killer: u64) -> bool {
        let now_ms = self.server_ms;
        let Some(running) = self.arena_matches.outpost_rush.as_mut() else {
            return false;
        };
        if running.baroness.as_ref().and_then(|run| run.boss) != Some(mob) {
            return false;
        }
        let killing_team = running.team_of(killer);
        let frozen = killing_team.map(|team| 1 - team.min(1));
        let lead = frozen.zip(killing_team).map_or(0, |(frozen, killing)| {
            i64::from(running.score_of(frozen)) - i64::from(running.score_of(killing))
        });
        if let Some(run) = running.baroness.as_mut() {
            run.died(now_ms, frozen, lead);
        }
        if let Some(team) = killing_team {
            let number = boss_killed_number(freeze_ms(lead), team);
            running.feed.note(BOSS_KILLED, now_ms, number);
        }
        tracing::info!(?frozen, lead, "the Baroness died");
        true
    }

    /// The match ends its cleanup: her barriers go, and she goes if she lives.
    pub(super) fn remove_baroness(&mut self, run: &BaronessRun) {
        for barrier in &run.barriers {
            self.stations.remove_camp(*barrier);
        }
        if let Some(boss) = run.boss {
            self.remove_gm(&[boss]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::test_support::{TEST_LEVEL, TEST_MOB};
    use crate::world::arena_match::PREGAME_MS;
    use crate::world::outpost_rush::tests::{
        FIRST, SECOND, START_MS, held_state, island_rush, outpost_rush, rush_sim_on,
    };
    use crate::world::tests::NOW;
    use nw_messages::game_mode_state::GameModeEvent;

    /// The Baroness of the test match.
    fn baroness(world: &SimWorld) -> &BaronessRun {
        world
            .arena_matches
            .outpost_rush
            .as_ref()
            .and_then(|running| running.baroness.as_ref())
            .expect("the island's Baroness")
    }

    /// The score of team `team` in the test match.
    fn score(world: &SimWorld, team: u8) -> u32 {
        world
            .arena_matches
            .outpost_rush
            .as_ref()
            .expect("the match")
            .score_of(team)
    }

    #[test]
    fn the_island_match_places_her_barriers_and_starts_her_timer() {
        let mut world = rush_sim_on(island_rush());
        let barriers = baroness(&world).barriers.clone();
        assert_eq!(barriers.len(), 2, "Barrier_Baroness_01 and _02");
        assert!(
            barriers
                .iter()
                .all(|barrier| world.stations.members(*barrier).is_some())
        );
        assert!(
            baroness(&world).timers().is_empty(),
            "no timer in the pregame"
        );
        let start = START_MS + PREGAME_MS;
        world.tick(NOW, start).expect("tick");
        assert_eq!(baroness(&world).next_spawn_ms, Some(start + SPAWN_DELAY_MS));
        let trench = rush_sim_on(outpost_rush());
        assert!(
            baroness(&trench).barriers.is_empty(),
            "she has no barriers on Trench"
        );
    }

    /// Makes the test match's Baroness a test mob that the test combat data can spawn.
    fn test_baroness(world: &mut SimWorld) {
        if let Some(run) = world
            .arena_matches
            .outpost_rush
            .as_mut()
            .and_then(|running| running.baroness.as_mut())
        {
            run.vitals = TEST_MOB;
            run.level = TEST_LEVEL;
        }
    }

    #[test]
    fn she_spawns_at_her_timer_on_trench_with_the_event_bossspawned() {
        let mut world = rush_sim_on(outpost_rush());
        test_baroness(&mut world);
        let start = START_MS + PREGAME_MS;
        world.tick(NOW, start).expect("tick");
        world.tick(NOW, start + SPAWN_DELAY_MS - 1).expect("tick");
        assert!(baroness(&world).boss.is_none(), "not before her timer");
        world.tick(NOW, start + SPAWN_DELAY_MS).expect("tick");
        assert!(baroness(&world).boss.is_some());
        assert!(
            baroness(&world).timers().is_empty(),
            "her timer left the list"
        );
        let spawned = held_state(&world)
            .events
            .into_iter()
            .find(|(_, event)| event.name == BOSS_SPAWNED)
            .expect("the event bossspawned");
        assert_eq!(
            spawned.1,
            GameModeEvent {
                name: BOSS_SPAWNED,
                at_ms: start + SPAWN_DELAY_MS,
                number: NO_NUMBER
            }
        );
    }

    #[test]
    fn her_death_freezes_the_killers_foes_for_their_lead_then_lets_them_score_again() {
        let mut world = rush_sim_on(island_rush());
        let start = START_MS + PREGAME_MS;
        world.tick(NOW, start).expect("tick");
        // Team 1 leads by two kills.
        world.outpost_rush_death(FIRST, SECOND);
        world.outpost_rush_death(FIRST, SECOND);
        assert_eq!((score(&world, 0), score(&world, 1)), (0, 2));
        let at = Point {
            x: 1451.0,
            y: 11078.0,
            z: 62.0,
        };
        let mob = world
            .spawn_gm(TEST_MOB, TEST_LEVEL, at, ONE, None)
            .expect("a mob")[0];
        if let Some(run) = world
            .arena_matches
            .outpost_rush
            .as_mut()
            .and_then(|running| running.baroness.as_mut())
        {
            run.boss = Some(mob);
            run.next_spawn_ms = None;
        }
        // The player of team 0 kills her.
        world.reward_kill(mob, FIRST);
        let freeze_end = start + freeze_ms(2);
        let killed = held_state(&world)
            .events
            .into_iter()
            .find(|(_, event)| event.name == BOSS_KILLED)
            .expect("the event bosskilled");
        assert_eq!(killed.1.number, boss_killed_number(freeze_ms(2), 0));
        assert_eq!(killed.1.number, 60 << 1, "60.9 s, killed by team 0");
        assert_eq!(baroness(&world).values(), [(FROZEN_STATUS, 1)]);
        assert_eq!(
            baroness(&world).timers(),
            [
                (BOSS_SPAWN_TIMER, start + SPAWN_DELAY_MS),
                (BOSS_FREEZE_TIMER, freeze_end)
            ]
        );
        world.outpost_rush_death(FIRST, SECOND);
        assert_eq!(score(&world, 1), 2, "a frozen team's kill scores nothing");
        world.outpost_rush_death(SECOND, FIRST);
        assert_eq!(score(&world, 0), 1, "the killing team still scores");
        world.tick(NOW, freeze_end).expect("tick");
        assert_eq!(baroness(&world).frozen_team(), None);
        assert_eq!(
            baroness(&world).values(),
            [(FROZEN_STATUS, u64::from(NO_FROZEN_TEAM))]
        );
        world.outpost_rush_death(FIRST, SECOND);
        assert_eq!(score(&world, 1), 3, "the freeze is over");
    }

    #[test]
    fn a_mob_that_is_not_she_changes_nothing() {
        let mut world = rush_sim_on(island_rush());
        world.tick(NOW, START_MS + PREGAME_MS).expect("tick");
        let at = Point {
            x: 1300.0,
            y: 11000.0,
            z: 60.0,
        };
        let mob = world
            .spawn_gm(TEST_MOB, TEST_LEVEL, at, ONE, None)
            .expect("a mob")[0];
        assert!(!world.baroness_killed(mob, FIRST));
        assert_eq!(baroness(&world).frozen_team(), None);
    }

    /// Her level is the one whose max health is her retail 330,960 (module text).
    #[test]
    #[ignore = "needs the build 25810808 content bundle in NW_CONTENT_DIR"]
    fn her_level_gives_her_retail_health() {
        const BUILD: &str = "25810808";
        const RETAIL_HEALTH: u32 = 330_960;
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let path = nw_content::bundle::bundle_path(std::path::Path::new(&dir), BUILD);
        let bundle = nw_content::ContentBundle::load(&path, BUILD).expect("bundle");
        let data = nw_content::combat::CombatData::from_tables(
            &bundle.tables,
            &nw_content::combat::CombatRows::default(),
        )
        .expect("combat data");
        let kind = data
            .mob_type(nw_formats::objectstream::lowercase_crc32(BARONESS_VITALS))
            .expect("her vitals row");
        let level = data.level(BARONESS_LEVEL).expect("her level row");
        assert_eq!(
            nw_sim_core::combat::mob_max_health(
                level.base_max_health,
                kind.health_mod,
                kind.category_health_mod
            ),
            RETAIL_HEALTH
        );
    }

    #[test]
    fn the_freeze_is_sixty_seconds_and_045_seconds_a_point_of_lead() {
        // The three retail deaths (module text): 60.000, 61.349 and 122.099 s (the timers are
        // whole ms of the server's clock).
        assert_eq!(freeze_ms(-156), 60_000);
        assert_eq!(freeze_ms(0), 60_000);
        assert_eq!(freeze_ms(3), 61_350);
        assert_eq!(freeze_ms(138), 122_100);
    }

    #[test]
    fn each_outpost_rush_map_has_her_place() {
        for (folder, x) in [
            (ISLAND_FOLDER, 1451.4815),
            (TRENCH_FOLDER, 1040.0),
            (VERTICAL_FOLDER, 992.0),
        ] {
            let run = BaronessRun::of_map(folder, Vec::new()).expect("her map");
            assert!((run.spot.x - x).abs() < f32::EPSILON, "{folder}");
            assert_eq!(run.vitals, BARONESS_VITALS);
        }
        assert!(BaronessRun::of_map("nw_ctf_002_wide", Vec::new()).is_none());
    }

    #[test]
    fn her_timer_runs_from_the_start_and_again_from_her_death() {
        let mut run = BaronessRun::of_map(ISLAND_FOLDER, Vec::new()).expect("the island");
        assert!(
            run.timers().is_empty() && run.values().is_empty(),
            "the pregame"
        );
        run.start(START_MS);
        assert_eq!(
            run.timers(),
            [(BOSS_SPAWN_TIMER, START_MS + SPAWN_DELAY_MS)]
        );
        assert!(!run.spawn_due(START_MS + SPAWN_DELAY_MS - 1));
        assert!(run.spawn_due(START_MS + SPAWN_DELAY_MS));
        run.boss = Some(7);
        run.next_spawn_ms = None;
        let death = START_MS + SPAWN_DELAY_MS + 30_000;
        run.died(death, Some(0), 3);
        assert_eq!(
            run.timers(),
            [
                (BOSS_SPAWN_TIMER, death + SPAWN_DELAY_MS),
                (BOSS_FREEZE_TIMER, death + 61_350)
            ]
        );
        assert_eq!(run.values(), [(FROZEN_STATUS, 0)]);
        assert_eq!(run.frozen_team(), Some(0));
        assert!(!run.end_freeze(death + 61_349));
        assert!(run.end_freeze(death + 61_350));
        assert_eq!(run.frozen_team(), None);
        assert_eq!(run.values(), [(FROZEN_STATUS, u64::from(NO_FROZEN_TEAM))]);
        assert_eq!(run.timers(), [(BOSS_SPAWN_TIMER, death + SPAWN_DELAY_MS)]);
    }

    #[test]
    fn a_death_by_no_player_of_the_match_freezes_no_team() {
        let mut run = BaronessRun::of_map(ISLAND_FOLDER, Vec::new()).expect("the island");
        run.died(START_MS, None, 0);
        assert_eq!(run.frozen_team(), None);
        assert!(run.values().is_empty());
        assert_eq!(
            run.timers(),
            [(BOSS_SPAWN_TIMER, START_MS + SPAWN_DELAY_MS)]
        );
    }
}
