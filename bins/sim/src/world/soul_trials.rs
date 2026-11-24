// SPDX-License-Identifier: AGPL-3.0-or-later
//! Soul trials: a player who ended a story trial plays it again as its daily trial
//! ([`nw_content::soul_trials`]). Retail soul trial runs (`Soul-Trial---Tempest-Mind`,
//! `soultrialhealervg`, `Beyond-the-gates-SOUL-TRIAL`) are entered from afar (5039, 5037, 946)
//! and run the trial's daily encounter (`super::dungeon_script::trial_variant`); the trial door's
//! condition admits a player with the daily entry flag and without its done flag.
//!
//! **Inferred** (no capture shows how the server's scheduler grants the flags, nor a daily
//! reset): a player who holds a trial's story end achievement and asks to enter that trial gets
//! its daily entry flag, unless it holds the daily done flag; the end of a daily run sets the
//! done flag, takes the entry flag back and fires the reward game event
//! (`DailyTrialIsabellaReward`). **Not built**: the daily reset of the done flag (Unknown).

use bevy_ecs::entity::Entity;

use super::{Player, SimWorld};

impl SimWorld {
    /// Player `entity` asks to enter game mode `crc`: a soul trial it may play today opens for
    /// it (its daily entry flag; module text).
    pub(super) fn open_soul_trial(&mut self, entity: Entity, crc: u32) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(trial) = content
            .instances
            .by_crc(crc)
            .and_then(|mode| content.soul_trials.of(&mode.id))
        else {
            return;
        };
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        let rules = &self.achievement_rules;
        let opens = player.flow.has_achievement(rules, &trial.story_done)
            && !player.flow.has_achievement(rules, &trial.entry)
            && !player.flow.has_achievement(rules, &trial.done);
        if opens {
            let number = player.number;
            tracing::info!(player = number, trial = %trial.game_mode, "soul trial opened");
            let now_ms = self.server_ms;
            self.earn_achievement(number, &trial.entry, now_ms);
        }
    }

    /// The run of this instance sim ended: each player of a soul trial's daily run did it (its
    /// done flag, the entry flag back, the reward game event; module text).
    pub(super) fn soul_trial_ended(&mut self, now_ms: u64) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(trial) = self
            .instance_run
            .as_ref()
            .and_then(|run| content.soul_trials.of(run.game_mode()))
        else {
            return;
        };
        let rules = &self.achievement_rules;
        let daily: Vec<u64> = {
            let mut query = self.world.query::<&Player>();
            query
                .iter(&self.world)
                .filter(|player| {
                    player.flow.has_achievement(rules, &trial.entry)
                        && !player.flow.has_achievement(rules, &trial.done)
                })
                .map(|player| player.number)
                .collect()
        };
        for number in daily {
            tracing::info!(player = number, trial = %trial.game_mode, "soul trial done");
            self.earn_achievement(number, &trial.done, now_ms);
            if let Some(entity) = self.player_by_number(number)
                && let Some(mut player) = self.world.get_mut::<Player>(entity)
            {
                player
                    .flow
                    .clear_achievement(&self.achievement_rules, &trial.entry);
            }
            self.fire_game_event(number, &trial.reward_event, now_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nw_content::Tables;
    use nw_content::achievements::AchievementIndexes;
    use nw_content::generated::GameModeSchedulerStaticData;
    use nw_content::instances::{InstanceCatalog, InstanceKind};
    use nw_content::soul_trials::SoulTrials;

    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::level_flow::AchievementRules;
    use crate::world::tests::{join, world_with_spawns};

    const PLAYER: u64 = 51;
    const MODE: &str = "QuestIsabella";
    const STORY_DONE: &str = "Trial_Quest_Isabella_Complete";
    const ENTRY: &str = "DailyTrialIsabella";
    const DONE: &str = "DailyTrialIsabellaComplete";

    /// A world with the Isabella trial, its scheduler row and its three achievements, and a
    /// player in it.
    fn world() -> (SimWorld, Entity, u32) {
        let mode = crate::world::instances::tests::mode(
            MODE,
            "nw_ori_sm_questisabella",
            InstanceKind::SoloTrial,
        );
        let crc = mode.crc;
        let mut tables = Tables::default();
        tables
            .game_mode_scheduler_static_data
            .push(nw_content::table::Table {
                source_path: "javelindata_gamemodescheduler.datasheet".to_owned(),
                unique_id: "GameModeScheduler".to_owned(),
                rows: vec![GameModeSchedulerStaticData {
                    category: "Trial".to_owned(),
                    game_mode_id: MODE.to_owned(),
                    quest_active_achievement_id: "Quest_Isabella_Active".to_owned(),
                    quest_completion_achievement_id: STORY_DONE.to_owned(),
                    scheduled_gm_completion_achievement_id: DONE.to_owned(),
                    scheduled_gm_completion_game_event: "DailyTrialIsabellaReward".to_owned(),
                    scheduled_gm_entry_achievement_id: ENTRY.to_owned(),
                }],
            });
        let indexes = AchievementIndexes::build(
            [(STORY_DONE, 1), (ENTRY, 2), (DONE, 3)].map(|(id, bit)| (id.to_owned(), bit)),
        );
        let mut world = world_with_spawns(Vec::new());
        world.achievement_rules = AchievementRules::of(&indexes, &[]);
        world.content = Arc::new(PlayerContent {
            instances: InstanceCatalog::build([mode]),
            soul_trials: SoulTrials::from_tables(&tables),
            achievements: indexes,
            ..PlayerContent::default()
        });
        join(&mut world, PLAYER, 0.0);
        let entity = world.player_by_number(PLAYER).expect("player");
        (world, entity, crc)
    }

    fn has(world: &SimWorld, entity: Entity, id: &str) -> bool {
        world
            .world
            .get::<Player>(entity)
            .expect("player")
            .flow
            .has_achievement(&world.achievement_rules, id)
    }

    fn set(world: &mut SimWorld, entity: Entity, id: &str) {
        let rules = world.achievement_rules.clone();
        world
            .world
            .get_mut::<Player>(entity)
            .expect("player")
            .flow
            .set_achievement(&rules, id);
    }

    #[test]
    fn a_player_who_ended_the_story_trial_gets_its_daily_entry_when_it_asks() {
        let (mut world, entity, crc) = world();
        world.open_soul_trial(entity, crc);
        assert!(!has(&world, entity, ENTRY), "the story trial is not done");
        set(&mut world, entity, STORY_DONE);
        world.open_soul_trial(entity, crc);
        assert!(has(&world, entity, ENTRY), "the daily trial opens");
    }

    #[test]
    fn a_player_who_did_the_daily_trial_gets_no_entry() {
        let (mut world, entity, crc) = world();
        set(&mut world, entity, STORY_DONE);
        set(&mut world, entity, DONE);
        world.open_soul_trial(entity, crc);
        assert!(!has(&world, entity, ENTRY));
    }
}
