// SPDX-License-Identifier: AGPL-3.0-or-later
//! The season pass of the live season (`SeasonsRewards*` tables, `SeasonPassRankData`): its
//! reward track, journey, activity card and tracked stats (seasons finding 2026-11-12).
//!
//! **Observed** (retail newchar 2026-09-18 and every retail season rewards state of the capture
//! index): `SeasonIds` starts with `Season10`, the last row of `SeasonsRewardsSeasonData` in
//! client build 25810808, and its card tasks and tracked stats are the content's season 10 rows.
//! So the live season is [`LIVE_SEASON`].
//!
//! - Reward track: `SeasonPassRankData` level `n` names the free (`FreeRewardId`) and the premium
//!   (`PremiumRewardId`) reward of the level; a reward's `RewardIndex` is its bit in the redeem
//!   bit words. Each level takes `MaximumInfluence` season experience (2,750 in every season).
//! - Journey: `SeasonsRewardsJourneyData` rows are the tasks of each chapter
//!   (`SeasonsRewardsChapterData`); a task's `Task` names a `SeasonsRewardsTasks` row, whose
//!   `SeasonsTrackedStatId` stat must reach `TaskMaxValue`.
//! - Activity card: 21 tasks (`TasksPerList`) of `SeasonsRewardsActivitiesTasksData`, at least 3
//!   of each `Type` (`RequiredFromEachType`), on a 4 x 4 card of `SeasonsRewardsCardTemplates`
//!   (`Card<NNN>Row<R>`, columns 1 to 4: the square's task type and experience).
//! - Tracked stats: every `SeasonsRewardsStats` row, sorted by the CRC-32 of the lowercase
//!   `TrackedStatId`; a stat's index on the wire is its place in that order
//!   (`nw_messages::seasons::SeasonsTrackedStatsState`).

use std::collections::BTreeSet;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;
use crate::generated::SeasonsRewardsStats;

/// The live season of client build 25810808 (module doc).
pub const LIVE_SEASON: &str = "Season10";
/// The tracked stat group of the activity card's tasks (Observed: group `8d0024e9` in every
/// retail tracked stats state; its name is the lowercase string whose CRC-32 that is).
pub const CARD_TASKS_GROUP: &str = "ActivitiesCurrentTasks";
/// Squares per row and rows per card.
pub const CARD_SIDE: usize = 4;
/// `SeasonsRewardsActivitiesConfig` ids.
const TASKS_PER_LIST: &str = "TasksPerList";
const REQUIRED_FROM_EACH_TYPE: &str = "RequiredFromEachType";
/// The card id prefix and the row infix of a `SeasonsRewardsCardTemplates` row id.
const CARD_PREFIX: &str = "Card";
const ROW_INFIX: &str = "Row";
/// `SeasonsRewardData` `RewardType` values.
const FREE_REWARD: &str = "BattlePass_Free";
const PREMIUM_REWARD: &str = "BattlePass_Premium";
const JOURNEY_REWARD: &str = "Journey";
const CHAPTER_REWARD: &str = "Chapter";
/// `SeasonsRewardsChapterData` `ChapterType` of the challenge chapter.
const CHALLENGE_CHAPTER: &str = "Challenge";
/// `TrackingType` of a categorical progression stat that counts ranks.
const RANKED_TRACKING: &str = "Ranked";
/// The one `LootTags` value a kill stat reads: `Named` is also a vitals category of the named
/// mobs (Observed: 699 `VitalsBaseData` rows); no vitals category is `Elite`, so an `Elite` stat
/// is not read.
const NAMED_LOOT_TAG: &str = "Named";
/// The separator of a list cell.
const LIST_SEPARATOR: char = ',';

/// What kind of reward a season reward is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewardKind {
    /// A free reward of the reward track.
    Free,
    /// A premium reward of the reward track: only for an account with the season's
    /// `PremiumEntitlementId`.
    Premium,
    /// The reward of a journey task.
    Journey,
    /// The reward of a whole chapter.
    Chapter,
}

/// One `SeasonsRewardData` row of the live season.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonReward {
    /// `RewardId`.
    pub id: String,
    /// Its CRC-32 (lowercase): the low half of a 5611 call's reward id.
    pub crc: u32,
    /// `RewardIndex`: its bit in the redeem words.
    pub index: u16,
    /// `RewardType`.
    pub kind: RewardKind,
    /// The reward track level that gives it; `None` for a journey or chapter reward.
    pub level: Option<u16>,
    /// `ItemId`: the item it gives.
    pub item: Option<String>,
    /// `SeasonsXp`: the season experience it gives.
    pub experience: u32,
    /// `EntitlementIds`: the entitlements it gives (`id:count` list).
    pub entitlements: Option<String>,
}

/// What a tracked stat counts (`StatType`), with the filters this server reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatKind {
    /// `Kill`: a mob kill of a mob of one of `categories` (`VitalsCategories`, or the
    /// `LootTags` `Named`, a vitals category of the named mobs too),
    /// of one of `targets` (`TargetId`, the vitals id), at least `level`, with a weapon of one
    /// of `weapons` (`WeaponMasteryIds`). An empty list takes any.
    Kill {
        /// Vitals categories (any one).
        categories: Vec<String>,
        /// Vitals ids (any one).
        targets: Vec<String>,
        /// The lowest mob level.
        level: u32,
        /// Weapon masteries of the weapon in the hand (any one).
        weapons: Vec<String>,
    },
    /// `Gather`: a gather of one of `gatherables` (`GatherableIds`) with one of `tradeskills`;
    /// each gather counts one when `per_use`, else the items it gave.
    Gather {
        /// Gatherable ids (any one).
        gatherables: Vec<String>,
        /// Tradeskills (any one).
        tradeskills: Vec<String>,
        /// `PerUse`.
        per_use: bool,
    },
    /// `Craft`: a craft of one of `items` (`ResultItemId`), of one of `classes` (`ItemClass`,
    /// `+` separated: any one); each made item counts.
    Craft {
        /// Result item ids (any one).
        items: Vec<String>,
        /// Item classes (any one).
        classes: Vec<String>,
    },
    /// `CompleteQuest`: a finished quest; a mission when `missions_only` (`QuestType`
    /// `Mission`).
    CompleteQuest {
        /// Only town board missions count.
        missions_only: bool,
    },
    /// `CategoricalProgression`: a gain of one of `progressions`: its points, or its ranks when
    /// `ranks` (`TrackingType` `Ranked`).
    CategoricalProgression {
        /// Categorical progression ids (any one).
        progressions: Vec<String>,
        /// Counts ranks, not points.
        ranks: bool,
    },
    /// `ObtainLevel`: 1 once the character level is at least `level`.
    ObtainLevel {
        /// The level.
        level: u32,
    },
    /// `CompleteActivityCard`: a finished activity card (a new card after it).
    CompleteActivityCard,
    /// `CompleteJourneyTask`: a redeemed journey task of a chapter of kind `challenge`
    /// (`ChapterType` `Challenge`) or not.
    CompleteJourneyTask {
        /// The task is of the challenge chapter.
        challenge: bool,
    },
    /// A stat of a system this server does not run, or with a filter it does not read
    /// ([`unread_filters`]): it never counts.
    Unread,
}

/// One tracked stat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedStat {
    /// `TrackedStatId`.
    pub id: String,
    /// Its CRC-32 (lowercase).
    pub crc: u32,
    /// What it counts.
    pub kind: StatKind,
}

/// A journey chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    /// `ChapterId`.
    pub id: String,
    /// `ChapterType` is `Challenge`.
    pub challenge: bool,
    /// `ChapterTaskCount`: the redeemed tasks its reward needs (Inferred from the name).
    pub tasks_needed: u32,
    /// `ChapterRewardId`'s index in [`SeasonCatalog::rewards`].
    pub reward: Option<usize>,
}

/// A journey task or an activity card task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonTask {
    /// `JourneyTaskId` or `ActivitiesTaskID`.
    pub id: String,
    /// Its CRC-32 (lowercase).
    pub crc: u32,
    /// The index in [`SeasonCatalog::stats`] of its `SeasonsTrackedStatId`.
    pub stat: u16,
    /// `TaskMaxValue`: the stat count that completes it.
    pub goal: u32,
    /// A journey task: its chapter's index in [`SeasonCatalog::chapters`]; a card task: `None`.
    pub chapter: Option<usize>,
    /// A journey task: its `RewardId`'s index in [`SeasonCatalog::rewards`].
    pub reward: Option<usize>,
    /// A card task: its `Type` (the squares it may stamp).
    pub kind: String,
}

/// One square of an activity card: its task type and the experience a stamp of it gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardSquare {
    /// `Column<N>Type`.
    pub kind: String,
    /// `Column<N>Xp`.
    pub experience: u32,
}

/// The card experience of `SeasonsRewardsCardData` (`Default`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CardBonus {
    /// `CardBonusXp`: a finished card's experience.
    pub card: u32,
    /// `LineBonusXp`: each full row or column's experience.
    pub line: u32,
    /// `StampsToComplete`: the stamps that finish a card.
    pub stamps_to_complete: u32,
}

/// The live season's pass ([`LIVE_SEASON`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeasonCatalog {
    /// `SeasonId`; empty when the content has no such season.
    pub season: String,
    /// Its CRC-32 (lowercase): its `SeasonIds` entry and its stat group.
    pub season_crc: u32,
    /// `PremiumEntitlementId`.
    pub premium_entitlement: String,
    /// The rewards, in table order.
    pub rewards: Vec<SeasonReward>,
    /// Every tracked stat, in wire index order.
    pub stats: Vec<TrackedStat>,
    /// The journey chapters, in `ChapterIndex` order.
    pub chapters: Vec<Chapter>,
    /// The journey tasks, in table order.
    pub journey: Vec<SeasonTask>,
    /// The activity card tasks that are not `Disabled`, in table order.
    pub card_tasks: Vec<SeasonTask>,
    /// The cards: each a row-major 4 x 4 of squares, by card number (template 0 is `Card001`).
    pub cards: Vec<Vec<CardSquare>>,
    /// The card experience.
    pub card_bonus: CardBonus,
    /// `TasksPerList`.
    pub tasks_per_card: usize,
    /// `RequiredFromEachType`.
    pub required_per_type: usize,
}

/// The items of a list cell, trimmed; none for an empty cell.
fn list_of(cell: Option<&str>, separator: char) -> Vec<String> {
    cell.map(|cell| {
        cell.split(separator)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

fn set(flag: Option<bool>) -> bool {
    flag == Some(true)
}

/// Whether `row` sets a filter that its stat kind's reader does not read: such a stat never
/// counts. Read are the filters [`StatKind`] names, `IsPermanent` (one season runs here) and
/// `PerUse`.
fn unread_filters(row: &SeasonsRewardsStats) -> bool {
    let kill = row.stat_type == "Kill";
    let gather = row.stat_type == "Gather";
    let craft = row.stat_type == "Craft";
    let level_read = kill || row.stat_type == "ObtainLevel";
    let loot_tags_read = kill
        && row.vitals_categories.is_none()
        && list_of(row.loot_tags.as_deref(), LIST_SEPARATOR)
            .iter()
            .all(|tag| tag == NAMED_LOOT_TAG);
    let quest_type_read = row
        .quest_type
        .as_deref()
        .is_none_or(|kind| kind == "Mission");
    let tracking_read = row
        .tracking_type
        .as_deref()
        .is_none_or(|kind| kind == RANKED_TRACKING || kind == "Points");
    [
        row.categories.is_some(),
        row.challenge_task_id.is_some(),
        row.chapter_index.is_some(),
        row.crafting_categories.is_some(),
        row.crafting_groups.is_some(),
        row.crafting_type.is_some(),
        row.creature_types.is_some(),
        row.exclude_item_class.is_some(),
        row.game_event_i_ds.is_some(),
        row.game_event_types.is_some(),
        row.game_mode.is_some(),
        row.game_mode_result.is_some(),
        row.gatherable_ids.is_some() && !gather,
        row.ingredient_id.is_some(),
        row.ingredient_item_id.is_some(),
        set(row.is_damage),
        set(row.is_debit),
        set(row.is_flawless),
        set(row.is_healing),
        set(row.is_invasion),
        set(row.is_killing_blow),
        row.is_mutated_expedition.is_some(),
        set(row.is_outpost_rush),
        set(row.is_pvp),
        row.is_war.is_some(),
        row.item_class.is_some() && !craft,
        row.item_class_equipped_min_count.is_some(),
        row.item_gs.is_some(),
        row.item_rarity.is_some(),
        row.item_tier.is_some(),
        row.item_type.is_some(),
        row.level.is_some() && !level_read,
        row.loot_tags.is_some() && !loot_tags_read,
        row.max_participants_on_dungeon_clear.is_some(),
        row.min_weight.is_some(),
        set(row.must_win),
        row.mutation_level.is_some(),
        set(row.open_world_only),
        row.poi_tags.is_some(),
        row.precision.is_some(),
        !quest_type_read,
        row.reasons.is_some(),
        set(row.require_group),
        row.required_world_tag.is_some(),
        set(row.requires_pristine_group),
        row.requires_status_effect.is_some(),
        row.result_item_id.is_some() && !craft,
        row.score.is_some(),
        row.song_difficulty.is_some(),
        row.song_rank.is_some(),
        row.target_id.is_some() && !kill,
        !tracking_read,
        row.tradeskills.is_some() && !gather,
        row.trading_categories.is_some(),
        row.trading_families.is_some(),
        row.trading_groups.is_some(),
        row.vitals_categories.is_some() && !kill,
        row.weapon_mastery_ids.is_some() && !kill,
    ]
    .contains(&true)
}

/// The kind of the stat of `row`.
fn stat_kind(row: &SeasonsRewardsStats) -> StatKind {
    if unread_filters(row) {
        return StatKind::Unread;
    }
    let list = |cell: &Option<String>| list_of(cell.as_deref(), LIST_SEPARATOR);
    let level = row
        .level
        .and_then(|level| u32::try_from(level).ok())
        .unwrap_or_default();
    match row.stat_type.as_str() {
        "Kill" => StatKind::Kill {
            categories: list(&row.vitals_categories)
                .into_iter()
                .chain(list(&row.loot_tags))
                .collect(),
            targets: list(&row.target_id),
            level,
            weapons: list(&row.weapon_mastery_ids),
        },
        "Gather" => StatKind::Gather {
            gatherables: list(&row.gatherable_ids),
            tradeskills: list(&row.tradeskills),
            per_use: set(row.per_use),
        },
        "Craft" => StatKind::Craft {
            items: list(&row.result_item_id),
            classes: list_of(row.item_class.as_deref(), '+'),
        },
        "CompleteQuest" => StatKind::CompleteQuest {
            missions_only: row.quest_type.is_some(),
        },
        "CategoricalProgression" => StatKind::CategoricalProgression {
            progressions: list(&row.categorical_progression_ids),
            ranks: row.tracking_type.as_deref() == Some(RANKED_TRACKING),
        },
        "ObtainLevel" => StatKind::ObtainLevel { level },
        "CompleteActivityCard" => StatKind::CompleteActivityCard,
        "CompleteJourneyTask" => StatKind::CompleteJourneyTask {
            challenge: row.chapter_type.as_deref() == Some(CHALLENGE_CHAPTER),
        },
        _ => StatKind::Unread,
    }
}

/// The template number (1-based) and the row (1-based) of a card row id `Card<NNN>Row<R>`.
fn card_row(id: &str) -> Option<(usize, usize)> {
    let (card, row) = id.strip_prefix(CARD_PREFIX)?.split_once(ROW_INFIX)?;
    Some((card.parse().ok()?, row.parse().ok()?))
}

/// Whether `source_path` is a datasheet of the season of folder `folder` (`season10`).
fn of_season(source_path: &str, folder: &str) -> bool {
    source_path
        .to_ascii_lowercase()
        .split('/')
        .any(|part| part == folder)
}

impl SeasonCatalog {
    /// The live season of `tables`; the default (no season) when the tables have none.
    pub fn from_tables(tables: &Tables) -> Self {
        let Some(season_row) = tables
            .seasons_rewards_season_data
            .iter()
            .flat_map(|table| &table.rows)
            .find(|row| row.season_id.eq_ignore_ascii_case(LIVE_SEASON))
        else {
            return Self::default();
        };
        let folder = season_row.season_id.to_ascii_lowercase();
        let live = |path: &str| of_season(path, &folder);
        let mut stat_rows: Vec<&SeasonsRewardsStats> = tables
            .seasons_rewards_stats
            .iter()
            .flat_map(|table| &table.rows)
            .collect();
        stat_rows.sort_by_key(|row| lowercase_crc32(&row.tracked_stat_id));
        let stats: Vec<TrackedStat> = stat_rows
            .iter()
            .map(|row| TrackedStat {
                id: row.tracked_stat_id.clone(),
                crc: lowercase_crc32(&row.tracked_stat_id),
                kind: stat_kind(row),
            })
            .collect();
        let levels: Vec<(u16, &str, Option<&str>)> = tables
            .season_pass_rank_data
            .iter()
            .filter(|table| live(&table.source_path))
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                Some((
                    u16::try_from(row.level).ok()?,
                    row.free_reward_id.as_str(),
                    row.premium_reward_id.as_deref(),
                ))
            })
            .collect();
        let rewards: Vec<SeasonReward> = tables
            .seasons_reward_data
            .iter()
            .filter(|table| live(&table.source_path))
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let kind = match row.reward_type.as_str() {
                    FREE_REWARD => RewardKind::Free,
                    PREMIUM_REWARD => RewardKind::Premium,
                    JOURNEY_REWARD => RewardKind::Journey,
                    CHAPTER_REWARD => RewardKind::Chapter,
                    _ => return None,
                };
                let level = levels
                    .iter()
                    .find(|(_, free, premium)| {
                        free.eq_ignore_ascii_case(&row.reward_id)
                            || premium
                                .is_some_and(|premium| premium.eq_ignore_ascii_case(&row.reward_id))
                    })
                    .map(|(level, _, _)| *level);
                Some(SeasonReward {
                    id: row.reward_id.clone(),
                    crc: lowercase_crc32(&row.reward_id),
                    index: u16::try_from(row.reward_index).ok()?,
                    kind,
                    level,
                    item: row.item_id.clone().filter(|item| !item.is_empty()),
                    experience: row
                        .seasons_xp
                        .and_then(|xp| u32::try_from(xp).ok())
                        .unwrap_or_default(),
                    entitlements: row.entitlement_ids.clone().filter(|ids| !ids.is_empty()),
                })
            })
            .collect();
        let reward_of = |id: &str| {
            rewards
                .iter()
                .position(|reward| reward.id.eq_ignore_ascii_case(id))
        };
        let mut chapter_rows: Vec<_> = tables
            .seasons_rewards_chapter_data
            .iter()
            .filter(|table| live(&table.source_path))
            .flat_map(|table| &table.rows)
            .collect();
        chapter_rows.sort_by_key(|row| row.chapter_index);
        let chapters: Vec<Chapter> = chapter_rows
            .iter()
            .map(|row| Chapter {
                id: row.chapter_id.clone(),
                challenge: row.chapter_type.eq_ignore_ascii_case(CHALLENGE_CHAPTER),
                tasks_needed: u32::try_from(row.chapter_task_count).unwrap_or_default(),
                reward: row.chapter_reward_id.as_deref().and_then(reward_of),
            })
            .collect();
        let task_rows: Vec<_> = tables
            .seasons_rewards_tasks
            .iter()
            .flat_map(|table| &table.rows)
            .collect();
        let task = |id: &str| {
            let row = task_rows
                .iter()
                .find(|row| row.seasons_task_id.eq_ignore_ascii_case(id))?;
            let stat = stats
                .iter()
                .position(|stat| stat.id.eq_ignore_ascii_case(&row.seasons_tracked_stat_id))?;
            Some((
                u16::try_from(stat).ok()?,
                u32::try_from(row.task_max_value).ok()?,
            ))
        };
        let journey = tables
            .seasons_rewards_journey_data
            .iter()
            .filter(|table| live(&table.source_path))
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let (stat, goal) = task(&row.task)?;
                Some(SeasonTask {
                    id: row.journey_task_id.clone(),
                    crc: lowercase_crc32(&row.journey_task_id),
                    stat,
                    goal,
                    chapter: chapters
                        .iter()
                        .position(|chapter| chapter.id.eq_ignore_ascii_case(&row.chapter)),
                    reward: reward_of(&row.reward_id),
                    kind: String::new(),
                })
            })
            .collect();
        let card_tasks = tables
            .seasons_rewards_activities_tasks_data
            .iter()
            .filter(|table| live(&table.source_path))
            .flat_map(|table| &table.rows)
            .filter(|row| row.disabled.as_deref().is_none_or(str::is_empty))
            .filter_map(|row| {
                let (stat, goal) = task(&row.task)?;
                Some(SeasonTask {
                    id: row.activities_task_id.clone(),
                    crc: lowercase_crc32(&row.activities_task_id),
                    stat,
                    goal,
                    chapter: None,
                    reward: None,
                    kind: row.r#type.clone(),
                })
            })
            .collect();
        let mut card_rows: Vec<(usize, usize, [CardSquare; CARD_SIDE])> = tables
            .seasons_rewards_card_templates
            .iter()
            .filter(|table| live(&table.source_path))
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let (card, line) = card_row(&row.card_and_row_id)?;
                let square = |kind: &str, xp: i64| CardSquare {
                    kind: kind.to_owned(),
                    experience: u32::try_from(xp).unwrap_or_default(),
                };
                Some((
                    card,
                    line,
                    [
                        square(&row.column1_type, row.column1_xp),
                        square(&row.column2_type, row.column2_xp),
                        square(&row.column3_type, row.column3_xp),
                        square(&row.column4_type, row.column4_xp),
                    ],
                ))
            })
            .collect();
        card_rows.sort_by_key(|(card, line, _)| (*card, *line));
        let numbers: BTreeSet<usize> = card_rows.iter().map(|(card, _, _)| *card).collect();
        let cards = numbers
            .into_iter()
            .map(|number| {
                card_rows
                    .iter()
                    .filter(|(card, _, _)| *card == number)
                    .flat_map(|(_, _, squares)| squares.iter().cloned())
                    .collect::<Vec<_>>()
            })
            .filter(|squares| squares.len() == CARD_SIDE * CARD_SIDE)
            .collect();
        let card_bonus = tables
            .seasons_rewards_card_data
            .iter()
            .filter(|table| live(&table.source_path))
            .flat_map(|table| &table.rows)
            .next()
            .map(|row| CardBonus {
                card: u32::try_from(row.card_bonus_xp).unwrap_or_default(),
                line: u32::try_from(row.line_bonus_xp).unwrap_or_default(),
                stamps_to_complete: u32::try_from(row.stamps_to_complete).unwrap_or_default(),
            })
            .unwrap_or_default();
        let config = |id: &str| {
            tables
                .seasons_rewards_activities_config
                .iter()
                .filter(|table| live(&table.source_path))
                .flat_map(|table| &table.rows)
                .find(|row| row.config_id.eq_ignore_ascii_case(id))
                .and_then(|row| usize::try_from(row.config_value).ok())
                .unwrap_or_default()
        };
        Self {
            season: season_row.season_id.clone(),
            season_crc: lowercase_crc32(&season_row.season_id),
            premium_entitlement: season_row.premium_entitlement_id.clone(),
            rewards,
            stats,
            chapters,
            journey,
            card_tasks,
            cards,
            card_bonus,
            tasks_per_card: config(TASKS_PER_LIST),
            required_per_type: config(REQUIRED_FROM_EACH_TYPE),
        }
    }

    /// The reward whose CRC-32 is `crc`.
    pub fn reward(&self, crc: u32) -> Option<&SeasonReward> {
        self.rewards.iter().find(|reward| reward.crc == crc)
    }

    /// The card task whose CRC-32 is `crc`.
    pub fn card_task(&self, crc: u32) -> Option<&SeasonTask> {
        self.card_tasks.iter().find(|task| task.crc == crc)
    }

    /// The distinct stat indexes of the journey tasks, ascending: the season group's stats.
    pub fn journey_stats(&self) -> Vec<u16> {
        let stats: BTreeSet<u16> = self.journey.iter().map(|task| task.stat).collect();
        stats.into_iter().collect()
    }

    /// The card task types, in first-seen order.
    pub fn card_task_kinds(&self) -> Vec<&str> {
        let mut kinds: Vec<&str> = Vec::new();
        for task in &self.card_tasks {
            if !kinds.contains(&task.kind.as_str()) {
                kinds.push(&task.kind);
            }
        }
        kinds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_row_id_names_its_card_and_row() {
        assert_eq!(card_row("Card001Row1"), Some((1, 1)));
        assert_eq!(card_row("Card010Row4"), Some((10, 4)));
        assert_eq!(card_row("Default"), None);
    }

    #[test]
    fn a_season_table_is_found_by_its_folder() {
        let path =
            "sharedassets/springboardentitites/datatables/seasonsrewards/season10/x.datasheet";
        assert!(of_season(path, "season10"));
        assert!(!of_season(path, "season1"));
    }

    fn stat_row(stat_type: &str) -> SeasonsRewardsStats {
        serde_json::from_value(serde_json::json!({
            "stat_type": stat_type,
            "tracked_stat_id": "Test",
        }))
        .expect("row")
    }

    #[test]
    fn a_stat_reads_its_filters_or_never_counts() {
        // Content bundle 25810808: `KillNamed60` (loot tag Named, level 60), `KillLost`.
        let mut named = stat_row("Kill");
        named.loot_tags = Some("Named".to_owned());
        named.level = Some(60);
        assert_eq!(
            stat_kind(&named),
            StatKind::Kill {
                categories: vec!["Named".to_owned()],
                targets: Vec::new(),
                level: 60,
                weapons: Vec::new(),
            }
        );
        // `KillLost65Nighthaven` asks for a place: not read here.
        let mut placed = stat_row("Kill");
        placed.poi_tags = Some("dunwood1".to_owned());
        assert_eq!(stat_kind(&placed), StatKind::Unread);
        // `RefineT5Resource`: the item tier is not read.
        let mut refine = stat_row("Craft");
        refine.item_tier = Some(5);
        assert_eq!(stat_kind(&refine), StatKind::Unread);
        let mut town = stat_row("CompleteQuest");
        town.quest_type = Some("CommunityGoal".to_owned());
        assert_eq!(stat_kind(&town), StatKind::Unread);
        let mut mission = stat_row("CompleteQuest");
        mission.quest_type = Some("Mission".to_owned());
        assert_eq!(
            stat_kind(&mission),
            StatKind::CompleteQuest {
                missions_only: true
            }
        );
        let mut level = stat_row("ObtainLevel");
        level.level = Some(70);
        level.is_permanent = Some(true);
        assert_eq!(stat_kind(&level), StatKind::ObtainLevel { level: 70 });
        assert_eq!(stat_kind(&stat_row("CompleteSong")), StatKind::Unread);
    }
}
