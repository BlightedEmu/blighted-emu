// SPDX-License-Identifier: AGPL-3.0-or-later
//! What feeds a catacombs run's pool of cursed currencies: its chests, its named rooms and its
//! bosses (client `FUN_1464857d0`; the pool is paid out at the end, [`super::end`]).
//!
//! Observed (the ten retail catacomb ledgers; the own player's `CatacombsCurrency_CursedSilvers`
//! and `_CursedCrowns` points):
//!
//! - A gathered cursed silver chest (`GatheredChestSilverSmall`, `GatheredChestSilverLarge`)
//!   and a cleared named room (`DefeatedEncounterNamedEnemy`) add
//!   `floor(base x factor x dynamicdifficulty + base)` silvers (the client's float sum): the
//!   small chest 10 (12 at difficulty 1, `CATACOMBS5` 1116.124 s), the large 50 (60 at 1,
//!   983.910 s; 70 at 2, `catacombshigh` 394.130 s), the named room 25 (37 at 1, five times:
//!   `CATACOMBS5` 1054.964 s, 1220.605 s, 1424.395 s, `CATACOMBS` 636.061 s, 700.827 s). So the
//!   chests' factor is 0.2 and the named room's lies from 0.48 to 0.52 (0.5 here). A named
//!   enemy killed outside a named room adds nothing (`CATACOMBS5` 518.615 s).
//! - A killed boss (`DefeatedEncounterBoss`) adds the silvers and crowns of the difficulty
//!   before its room's clear raises it: 100/0, 200/1, 400/3, 800/9, 1200/18, 1600/27 for 0 to
//!   5 (`CATACOMBS5` 929.066 s, 1575.505 s; `CATACOMBS6` 948.339 s to 1367.890 s;
//!   `catacombshigh` 310.887 s to 1001.808 s).
//! - Every participant gets the same amount (the client adds it for each, `FUN_146484b80`).
//! - A gathered chest stays, emptied (`CATACOMBS5` 157.079 s), and counts on the war board
//!   ([`super::board`]).
//!
//! Inferred: a difficulty above 5 pays as 5 (the client's tables have eight entries; no run
//! reached 6).

use super::super::SimWorld;
use super::board::{
    BOSSES_CROWNS, BOSSES_KILLED, BOSSES_SILVERS, CHESTS_OPENED, CHESTS_OPENED_CROWNS,
    CHESTS_OPENED_SILVERS, NAMED_ENEMIES_CROWNS, NAMED_ENEMIES_KILLED, NAMED_ENEMIES_SILVERS,
};
use super::end::CURRENCIES;
use super::{CatacombRun, DYNAMIC_DIFFICULTY_VALUE};
use crate::world::Player;

/// The cursed silver chests: slice and silvers at difficulty 0 (module text).
pub(super) const CHESTS: [(&str, u32); 2] = [
    (
        "slices/catacombs/gameplay/gatherables/catacombs_gatherable_chest_cursedsilver_s.dynamicslice",
        10,
    ),
    (
        "slices/catacombs/gameplay/gatherables/catacombs_gatherable_chest_cursedsilver_l.dynamicslice",
        50,
    ),
];
/// The share of a chest's silvers each difficulty step adds.
const CHEST_FACTOR: f32 = 0.2;
/// The room kind of a named enemy room, its silvers at difficulty 0 and the share each
/// difficulty step adds.
const NAMED_ROOM: &str = "EncounterNamedEnemy";
const NAMED_ROOM_SILVERS: u32 = 25;
const NAMED_FACTOR: f32 = 0.5;
/// The room kind of a boss room and a boss's silvers and crowns by difficulty.
const BOSS_ROOM: &str = "EncounterBoss";
const BOSS_SILVERS: [u32; 6] = [100, 200, 400, 800, 1200, 1600];
const BOSS_CROWNS: [u32; 6] = [0, 1, 3, 9, 18, 27];

/// `base` raised by `factor` per difficulty step (the client's float sum, rounded down).
fn scaled(base: u32, factor: f32, difficulty: u64) -> u32 {
    let steps = u32::try_from(difficulty).unwrap_or(u32::MAX);
    // Pools stay far below the float's whole-number range.
    (base.saturating_mul(steps) as f32 * factor + base as f32).floor() as u32
}

/// A boss's silvers and crowns at `difficulty` (module text).
fn boss_reward(difficulty: u64) -> (u32, u32) {
    let last = BOSS_SILVERS.len() - 1;
    let index = usize::try_from(difficulty).map_or(last, |index| index.min(last));
    (BOSS_SILVERS[index], BOSS_CROWNS[index])
}

/// What the pool of a run took in so far, for the war board's end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RunPool {
    /// The run's chests: start object number and silvers at difficulty 0.
    chests: Vec<(u64, u32)>,
    chest_silvers: u32,
    boss_silvers: u32,
    boss_crowns: u32,
    named_silvers: u32,
}

impl RunPool {
    /// The silvers at difficulty 0 of the chest of slice `slice`, when it is one.
    pub(super) fn chest_base(slice: &str) -> Option<u32> {
        CHESTS
            .iter()
            .find(|(chest, _)| *chest == slice)
            .map(|(_, base)| *base)
    }

    /// Chest `object` (a start object) of `base` silvers stands in the run.
    pub(super) fn add_chest(&mut self, object: u64, base: u32) {
        self.chests.push((object, base));
    }

    /// The run's chests, to take away at its end.
    pub(super) fn take_chests(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.chests)
            .into_iter()
            .map(|(object, _)| object)
            .collect()
    }

    /// The board's end slots of what the pool took in: chests, bosses and named rooms (the
    /// chests and named rooms give no crowns).
    pub(super) fn board_totals(&self) -> [(u8, u32); 6] {
        [
            (CHESTS_OPENED_SILVERS, self.chest_silvers),
            (CHESTS_OPENED_CROWNS, 0),
            (BOSSES_SILVERS, self.boss_silvers),
            (BOSSES_CROWNS, self.boss_crowns),
            (NAMED_ENEMIES_SILVERS, self.named_silvers),
            (NAMED_ENEMIES_CROWNS, 0),
        ]
    }
}

impl CatacombRun {
    /// The run's difficulty now.
    fn difficulty(&self) -> u64 {
        self.end.value(DYNAMIC_DIFFICULTY_VALUE)
    }

    /// Room `room`'s clear, before it raises the difficulty: the silvers and crowns a named or
    /// boss room adds, counted on the board at `server_ms`.
    pub(super) fn room_feed(&mut self, room: usize, server_ms: u64) -> Option<(u32, u32)> {
        let difficulty = self.difficulty();
        match self.layout.rooms.get(room)?.room_type.as_str() {
            NAMED_ROOM => {
                let silvers = scaled(NAMED_ROOM_SILVERS, NAMED_FACTOR, difficulty);
                self.pool.named_silvers = self.pool.named_silvers.saturating_add(silvers);
                self.board.count(NAMED_ENEMIES_KILLED, server_ms);
                Some((silvers, 0))
            }
            BOSS_ROOM => {
                let (silvers, crowns) = boss_reward(difficulty);
                self.pool.boss_silvers = self.pool.boss_silvers.saturating_add(silvers);
                self.pool.boss_crowns = self.pool.boss_crowns.saturating_add(crowns);
                self.board.count(BOSSES_KILLED, server_ms);
                Some((silvers, crowns))
            }
            _ => None,
        }
    }

    /// Chest `object` of the run was gathered at `server_ms`: the silvers it adds, counted on
    /// the board; `None` for an object that is no chest of the run.
    fn chest_feed(&mut self, object: u64, server_ms: u64) -> Option<u32> {
        let base = self
            .pool
            .chests
            .iter()
            .find(|(chest, _)| *chest == object)
            .map(|(_, base)| *base)?;
        let silvers = scaled(base, CHEST_FACTOR, self.difficulty());
        self.pool.chest_silvers = self.pool.chest_silvers.saturating_add(silvers);
        self.board.count(CHESTS_OPENED, server_ms);
        Some(silvers)
    }
}

impl SimWorld {
    /// Start object `object` was gathered: when it is a chest of this sim's catacombs run it
    /// stays, emptied, and feeds the pool (module text). Whether it was such a chest.
    pub(in crate::world) fn catacomb_chest_opened(&mut self, object: u64) -> bool {
        let server_ms = self.server_ms;
        let Some(silvers) = self
            .catacomb_run_mut()
            .and_then(|run| run.chest_feed(object, server_ms))
        else {
            return false;
        };
        self.encounter_objects.deplete(object);
        self.feed_catacomb_pool(silvers, 0);
        true
    }

    /// Every participant's cursed silvers and crowns rise by `silvers` and `crowns`.
    pub(super) fn feed_catacomb_pool(&mut self, silvers: u32, crowns: u32) {
        let content = std::sync::Arc::clone(&self.content);
        let players: Vec<_> = self
            .catacomb_run()
            .map(|run| {
                run.progress
                    .participants
                    .iter()
                    .map(|participant| participant.entity)
                    .collect()
            })
            .unwrap_or_default();
        let gains = [silvers, crowns];
        for entity in players {
            let Some(mut player) = self.world.get_mut::<Player>(entity) else {
                continue;
            };
            for ((cursed, _, _), gain) in CURRENCIES.iter().zip(gains) {
                if gain == 0 {
                    continue;
                }
                let Some(mastery) = content.progression.categorical(cursed) else {
                    tracing::warn!(currency = cursed, "catacombs currency not in the content");
                    continue;
                };
                let held = player.masteries.experience(mastery);
                player.masteries.set(mastery, held.saturating_add(gain));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::arrived_run;
    use super::*;

    /// The retail amounts (module text).
    #[test]
    fn the_feed_scales_with_the_difficulty_as_the_retail_runs() {
        assert_eq!(scaled(10, CHEST_FACTOR, 0), 10);
        assert_eq!(scaled(10, CHEST_FACTOR, 1), 12);
        assert_eq!(scaled(50, CHEST_FACTOR, 1), 60);
        assert_eq!(scaled(50, CHEST_FACTOR, 2), 70);
        assert_eq!(scaled(NAMED_ROOM_SILVERS, NAMED_FACTOR, 0), 25);
        assert_eq!(scaled(NAMED_ROOM_SILVERS, NAMED_FACTOR, 1), 37);
        assert_eq!(boss_reward(0), (100, 0));
        assert_eq!(boss_reward(1), (200, 1));
        assert_eq!(boss_reward(2), (400, 3));
        assert_eq!(boss_reward(5), (1600, 27));
        assert_eq!(boss_reward(7), (1600, 27), "Inferred: as 5");
        assert_eq!(RunPool::chest_base(CHESTS[0].0), Some(10));
        assert_eq!(RunPool::chest_base("slices/other.dynamicslice"), None);
    }

    /// The cursed silvers and crowns of the run's first participant.
    fn pool_of(world: &SimWorld) -> [u32; 2] {
        let entity = world.catacomb_run().expect("run").progress.participants[0].entity;
        let player = world.world.get::<Player>(entity).expect("player");
        CURRENCIES.map(|(cursed, _, _)| {
            let mastery = world.content.progression.categorical(cursed).expect(cursed);
            player.masteries.experience(mastery)
        })
    }

    /// A run whose tiles hold one small chest (the test content's start objects table holds
    /// the retail chest), and the chest's start object.
    fn chest_world() -> (SimWorld, u64) {
        let (mut world, _) = arrived_run();
        let run = world.catacomb_run_mut().expect("run");
        let mut chest = run.pieces[0].clone();
        chest.slice = CHESTS[0].0.to_owned();
        run.pieces = vec![chest];
        run.gameplay.clear();
        world.place_catacomb_tiles();
        let run = world.catacomb_run().expect("run");
        assert_eq!(run.pool.chests.len(), 1, "the chest is a start object");
        assert_eq!(run.placed.len(), 1, "only the run's own entity is a station");
        let object = run.pool.chests[0].0;
        assert!(world.encounter_objects.stands(object));
        (world, object)
    }

    /// Retail `CATACOMBS5` 157.079 s to 157.113 s and 1116.124 s: a gathered chest stays emptied
    /// and adds 10 silvers (12 at difficulty 1); an object that is no chest of the run does not
    /// count.
    #[test]
    fn a_gathered_chest_stays_emptied_and_feeds_the_pool_by_the_difficulty() {
        let (mut world, object) = chest_world();
        assert!(!world.catacomb_chest_opened(object + 1), "no chest of the run");
        assert!(world.catacomb_chest_opened(object));
        assert!(world.encounter_objects.stands(object), "it stays");
        assert_eq!(pool_of(&world), [10, 0]);
        let second = world
            .encounter_objects
            .spawn(CHESTS[0].0, [500.0, 761.0, 120.0], 0.0)
            .expect("chest");
        let run = world.catacomb_run_mut().expect("run");
        run.pool.add_chest(second, CHESTS[0].1);
        let boss = run
            .layout
            .rooms
            .iter()
            .position(|room| room.room_type == BOSS_ROOM)
            .expect("boss room");
        run.room_cleared(boss);
        assert_eq!(run.difficulty(), 1);
        assert!(world.catacomb_chest_opened(second));
        assert_eq!(pool_of(&world), [22, 0]);
        let run = world.catacomb_run().expect("run");
        assert_eq!(run.pool.board_totals()[0], (CHESTS_OPENED_SILVERS, 22));
    }

    /// Retail `CATACOMBS5` 929.066 s and 929.415 s: the boss pays at the difficulty before its
    /// room's clear raises it; a named room pays 25 at 0 and 37 at 1 (1054.964 s).
    #[test]
    fn a_cleared_boss_or_named_room_feeds_the_pool_at_the_difficulty_before_the_clear() {
        let (mut world, _) = arrived_run();
        let boss = {
            let run = world.catacomb_run().expect("run");
            run.layout
                .rooms
                .iter()
                .position(|room| room.room_type == BOSS_ROOM)
                .expect("boss room")
        };
        let encounters = world.catacomb_run().expect("run").rooms[boss]
            .open_encounters
            .clone();
        for encounter in encounters {
            world.catacomb_encounter_done(encounter);
        }
        assert_eq!(pool_of(&world), [100, 0]);
        let run = world.catacomb_run_mut().expect("run");
        assert_eq!(run.difficulty(), 1, "raised after the pay");
        assert_eq!(run.room_feed(boss, 0), Some((200, 1)));
        run.layout.rooms[boss].room_type = NAMED_ROOM.to_owned();
        assert_eq!(run.room_feed(boss, 0), Some((37, 0)));
        assert_eq!(
            run.pool.board_totals(),
            [
                (CHESTS_OPENED_SILVERS, 0),
                (CHESTS_OPENED_CROWNS, 0),
                (BOSSES_SILVERS, 300),
                (BOSSES_CROWNS, 1),
                (NAMED_ENEMIES_SILVERS, 37),
                (NAMED_ENEMIES_CROWNS, 0),
            ]
        );
        assert_eq!(run.room_feed(0, 0), None, "the start room pays nothing");
    }
}
