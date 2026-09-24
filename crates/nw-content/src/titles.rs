// SPDX-License-Identifier: AGPL-3.0-or-later
//! The player titles of the game data (`javelindata_playertitles`, `PlayerTitleData`) and what
//! earns each one. The client names a title by the CRC-32 of its lowercase `TitleID` (all 36
//! ids of the retail newchar 4147 are in the sheet so). A row needs at most one of:
//!
//! - `RequiredPlayerLevel`: the zero-based player level (`PlayerTitle_PlayerProgression5` at 49
//!   is the level-50 title);
//! - `CategoricalProgressionId` with `RequiredCategoricalProgressionLevel`: a rank of that
//!   progression (the trade skills at 140 and 200);
//! - `AchievementId`: one achievement;
//! - `MetaAchievementId`: one meta achievement of a comma-separated list;
//!
//! and a row with none of them is granted (pre-order, seasons, events). A granted title is
//! earned only where a grant is stored. A rank is the zero-based rank of the type-911 list
//! (Observed: the retail newchar's Fishing rank 50 completed `SkillLevel_Fishing2`, `Total` 40,
//! with a 331 notice of progress 50; `WeaponMastery_Sword3` asks 19, the top weapon rank).

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// `TitleType` of a title every character of the account may use.
const ACCOUNT_TYPE: &str = "Account";

/// Whose title it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleScope {
    /// Every character of the account may use it once one earned it (482 of 497 rows).
    Account,
    /// Only the character that earned it (15 rows: territory owners and cult quests).
    Character,
}

/// What earns a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleRequirement {
    /// A zero-based player level of at least this.
    PlayerLevel(u32),
    /// A rank of at least `rank` in the categorical progression `progression`.
    Rank {
        /// `CategoricalProgressionId`.
        progression: String,
        /// `RequiredCategoricalProgressionLevel`.
        rank: u32,
    },
    /// The achievement `AchievementId`.
    Achievement(String),
    /// Any of these meta achievements (`MetaAchievementId`).
    MetaAchievement(Vec<String>),
    /// No condition: a grant.
    Granted,
}

/// One title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleRule {
    /// The CRC-32 of the lowercase `TitleID`: its id on the wire.
    pub id: u32,
    /// Whose title it is.
    pub scope: TitleScope,
    /// What earns it.
    pub requirement: TitleRequirement,
}

/// What a character has done, as the titles test it.
pub struct TitleProgress<'a> {
    /// Its zero-based level.
    pub level: u32,
    /// Its rank in a categorical progression, by `CategoricalProgressionId` (any case).
    pub rank: &'a dyn Fn(&str) -> u32,
    /// Whether it has an achievement, by `AchievementId` (any case).
    pub achievement: &'a dyn Fn(&str) -> bool,
    /// Whether it completed a meta achievement, by `MetaAchievementId` (any case).
    pub meta_achievement: &'a dyn Fn(&str) -> bool,
}

/// Every title of the game data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleRules {
    titles: Vec<TitleRule>,
}

impl TitleRules {
    /// The titles of `tables`; a row with a level or rank outside `u32` is left out.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .player_title_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let requirement = requirement_of(
                        row.required_player_level,
                        row.categorical_progression_id.as_deref(),
                        row.required_categorical_progression_level,
                        row.achievement_id.as_deref(),
                        row.meta_achievement_id.as_deref(),
                    )?;
                    let scope = if row.title_type == ACCOUNT_TYPE {
                        TitleScope::Account
                    } else {
                        TitleScope::Character
                    };
                    Some((row.title_id.as_str(), scope, requirement))
                }),
        )
    }

    /// The titles of `rows`: (`TitleID`, scope, requirement).
    pub fn build<'a>(
        rows: impl IntoIterator<Item = (&'a str, TitleScope, TitleRequirement)>,
    ) -> Self {
        Self {
            titles: rows
                .into_iter()
                .map(|(id, scope, requirement)| TitleRule {
                    id: lowercase_crc32(id),
                    scope,
                    requirement,
                })
                .collect(),
        }
    }

    /// The titles that `progress` earns now, in sheet order. A granted title is never among
    /// them: this server stores no grant (pre-order, season, event).
    pub fn earned<'a>(
        &'a self,
        progress: &'a TitleProgress<'_>,
    ) -> impl Iterator<Item = &'a TitleRule> + 'a {
        self.titles
            .iter()
            .filter(move |title| match &title.requirement {
                TitleRequirement::PlayerLevel(level) => progress.level >= *level,
                TitleRequirement::Rank { progression, rank } => {
                    (progress.rank)(progression.as_str()) >= *rank
                }
                TitleRequirement::Achievement(id) => (progress.achievement)(id.as_str()),
                TitleRequirement::MetaAchievement(ids) => {
                    ids.iter().any(|id| (progress.meta_achievement)(id.as_str()))
                }
                TitleRequirement::Granted => false,
            })
    }

    /// Whether completing meta achievement `id` (any case) earns a title.
    pub fn meta_achievement_earns(&self, id: &str) -> bool {
        self.titles.iter().any(|title| {
            matches!(&title.requirement, TitleRequirement::MetaAchievement(ids)
                if ids.iter().any(|meta| meta.eq_ignore_ascii_case(id)))
        })
    }
}

/// The requirement of a row's columns; `None` for a level or rank outside `u32`.
fn requirement_of(
    player_level: Option<i64>,
    progression: Option<&str>,
    rank: Option<i64>,
    achievement: Option<&str>,
    meta_achievements: Option<&str>,
) -> Option<TitleRequirement> {
    fn present(text: Option<&str>) -> Option<&str> {
        let trimmed = text?.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }
    Some(if let Some(level) = player_level {
        TitleRequirement::PlayerLevel(u32::try_from(level).ok()?)
    } else if let Some(progression) = present(progression) {
        TitleRequirement::Rank {
            progression: progression.to_owned(),
            rank: u32::try_from(rank.unwrap_or_default()).ok()?,
        }
    } else if let Some(achievement) = present(achievement) {
        TitleRequirement::Achievement(achievement.to_owned())
    } else if let Some(meta) = present(meta_achievements) {
        TitleRequirement::MetaAchievement(
            meta.split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    } else {
        TitleRequirement::Granted
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CRC-32 of `playertitle_stoic` (the first title of the retail newchar 4147).
    const STOIC: u32 = 0x431d_f4ea;
    /// `PlayerTitle_PlayerProgression5`: zero-based level 49.
    const LEVEL_50: u32 = 49;
    /// `PlayerTitle_Skill_Mining4`: rank 140 of `Mining`.
    const MINING_RANK: u32 = 140;

    fn rules() -> TitleRules {
        TitleRules::build([
            (
                "PlayerTitle_Stoic",
                TitleScope::Account,
                TitleRequirement::Granted,
            ),
            (
                "PlayerTitle_PlayerProgression5",
                TitleScope::Account,
                TitleRequirement::PlayerLevel(LEVEL_50),
            ),
            (
                "PlayerTitle_Skill_Mining4",
                TitleScope::Account,
                TitleRequirement::Rank {
                    progression: "Mining".to_owned(),
                    rank: MINING_RANK,
                },
            ),
            (
                "PlayerTitle_CultInitiate",
                TitleScope::Character,
                TitleRequirement::Achievement("14_Quest_20".to_owned()),
            ),
            (
                "PlayerTitle_WeaponMaster_Sword2",
                TitleScope::Account,
                TitleRequirement::MetaAchievement(vec!["WeaponMastery_Sword2".to_owned()]),
            ),
        ])
    }

    fn earned(rules: &TitleRules, level: u32, mining: u32, quest: bool) -> Vec<u32> {
        earned_with_meta(rules, level, mining, quest, &|_: &str| false)
    }

    fn earned_with_meta(
        rules: &TitleRules,
        level: u32,
        mining: u32,
        quest: bool,
        meta_achievement: &dyn Fn(&str) -> bool,
    ) -> Vec<u32> {
        let rank = move |id: &str| {
            if id.eq_ignore_ascii_case("mining") {
                mining
            } else {
                0
            }
        };
        let achievement = move |id: &str| quest && id.eq_ignore_ascii_case("14_quest_20");
        let progress = TitleProgress {
            level,
            rank: &rank,
            achievement: &achievement,
            meta_achievement,
        };
        rules.earned(&progress).map(|title| title.id).collect()
    }

    #[test]
    fn ids_are_the_crc_of_the_lowercase_title_id() {
        assert_eq!(lowercase_crc32("PlayerTitle_Stoic"), STOIC);
        let rules = rules();
        assert_eq!(rules.titles[0].id, STOIC);
        assert_eq!(rules.titles[0].requirement, TitleRequirement::Granted);
    }

    #[test]
    fn a_title_is_earned_by_its_one_requirement() {
        let rules = rules();
        assert!(earned(&rules, LEVEL_50 - 1, MINING_RANK - 1, false).is_empty());
        let all = earned(&rules, LEVEL_50, MINING_RANK, true);
        assert_eq!(
            all,
            [
                lowercase_crc32("PlayerTitle_PlayerProgression5"),
                lowercase_crc32("PlayerTitle_Skill_Mining4"),
                lowercase_crc32("PlayerTitle_CultInitiate"),
            ],
            "a granted title needs a stored grant, a meta one its meta achievement"
        );
    }

    #[test]
    fn the_columns_give_one_requirement() {
        assert_eq!(
            requirement_of(None, None, None, None, Some("A1, B1,")),
            Some(TitleRequirement::MetaAchievement(vec![
                "A1".to_owned(),
                "B1".to_owned()
            ]))
        );
        assert_eq!(
            requirement_of(None, Some(" "), None, Some(""), None),
            Some(TitleRequirement::Granted)
        );
        assert_eq!(requirement_of(Some(-1), None, None, None, None), None);
    }

    #[test]
    fn a_completed_meta_achievement_earns_its_title() {
        let rules = rules();
        let completed = |id: &str| id.eq_ignore_ascii_case("weaponmastery_sword2");
        assert_eq!(
            earned_with_meta(&rules, 0, 0, false, &completed),
            [lowercase_crc32("PlayerTitle_WeaponMaster_Sword2")]
        );
        assert!(rules.meta_achievement_earns("WEAPONMASTERY_SWORD2"));
        assert!(!rules.meta_achievement_earns("WeaponMastery_Sword1"));
    }
}
