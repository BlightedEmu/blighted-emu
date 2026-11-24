// SPDX-License-Identifier: AGPL-3.0-or-later
//! The soul trials: the daily repeat of a story trial (`GameModeSchedulerStaticData`, build
//! 25810808: 16 rows of category `Trial`, `QuestLiang` and `QuestAdiana` among them).
//!
//! Each row names the trial's game mode, the achievement its story quest sets while the trial is
//! open (`QuestActiveAchievementId`, `Quest_Isabella_Active`), the one the story's end sets
//! (`QuestCompletionAchievementId`, `Trial_Quest_Isabella_Complete`), the daily entry flag
//! (`ScheduledGMEntryAchievementId`, `DailyTrialIsabella`), the daily done flag
//! (`ScheduledGMCompletionAchievementId`, `DailyTrialIsabellaComplete`) and the game event of
//! the daily run's reward (`ScheduledGMCompletionGameEvent`, `DailyTrialIsabellaReward`). The
//! trial doors' conditions read the flags (`99F_Sun_Greet_FightLiang_00 || (DailyTrialLiang &&
//! !DailyTrialLiangComplete)`).

use crate::Tables;

/// The category of the scheduler rows of the soul trials.
const TRIAL_CATEGORY: &str = "Trial";

/// One soul trial (a `GameModeSchedulerStaticData` row of category `Trial`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoulTrial {
    /// `GameModeId`.
    pub game_mode: String,
    /// `QuestCompletionAchievementId`: the story trial is done.
    pub story_done: String,
    /// `ScheduledGMEntryAchievementId`: the daily trial is open to the player.
    pub entry: String,
    /// `ScheduledGMCompletionAchievementId`: the player did the daily trial.
    pub done: String,
    /// `ScheduledGMCompletionGameEvent`: the daily run's reward.
    pub reward_event: String,
}

/// The soul trials of the content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoulTrials {
    trials: Vec<SoulTrial>,
}

impl SoulTrials {
    /// The soul trials of `tables`' scheduler rows of category `Trial`.
    pub fn from_tables(tables: &Tables) -> Self {
        let trials = tables
            .game_mode_scheduler_static_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter(|row| row.category.eq_ignore_ascii_case(TRIAL_CATEGORY))
            .map(|row| SoulTrial {
                game_mode: row.game_mode_id.clone(),
                story_done: row.quest_completion_achievement_id.clone(),
                entry: row.scheduled_gm_entry_achievement_id.clone(),
                done: row.scheduled_gm_completion_achievement_id.clone(),
                reward_event: row.scheduled_gm_completion_game_event.clone(),
            })
            .collect();
        Self { trials }
    }

    /// The soul trial of game mode `game_mode` (any case).
    pub fn of(&self, game_mode: &str) -> Option<&SoulTrial> {
        self.trials
            .iter()
            .find(|trial| trial.game_mode.eq_ignore_ascii_case(game_mode))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::GameModeSchedulerStaticData;

    fn row(category: &str, mode: &str) -> GameModeSchedulerStaticData {
        GameModeSchedulerStaticData {
            category: category.to_owned(),
            game_mode_id: mode.to_owned(),
            quest_active_achievement_id: "Quest_Isabella_Active".to_owned(),
            quest_completion_achievement_id: "Trial_Quest_Isabella_Complete".to_owned(),
            scheduled_gm_completion_achievement_id: "DailyTrialIsabellaComplete".to_owned(),
            scheduled_gm_completion_game_event: "DailyTrialIsabellaReward".to_owned(),
            scheduled_gm_entry_achievement_id: "DailyTrialIsabella".to_owned(),
        }
    }

    #[test]
    fn a_trial_row_of_the_scheduler_is_a_soul_trial() {
        let mut tables = Tables::default();
        tables
            .game_mode_scheduler_static_data
            .push(crate::table::Table {
                source_path: "javelindata_gamemodescheduler.datasheet".to_owned(),
                unique_id: "GameModeScheduler".to_owned(),
                rows: vec![row("Trial", "QuestIsabella"), row("Event", "QuestOther")],
            });
        let trials = SoulTrials::from_tables(&tables);
        let isabella = trials.of("questisabella").expect("the trial");
        assert_eq!(isabella.entry, "DailyTrialIsabella");
        assert_eq!(isabella.done, "DailyTrialIsabellaComplete");
        assert_eq!(isabella.reward_event, "DailyTrialIsabellaReward");
        assert_eq!(isabella.story_done, "Trial_Quest_Isabella_Complete");
        assert!(trials.of("QuestOther").is_none(), "another category");
    }
}
