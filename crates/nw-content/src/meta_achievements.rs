// SPDX-License-Identifier: AGPL-3.0-or-later
//! The meta achievements of the game data (`MetaAchievementData`, the achievements window) and
//! what counts for each (progression note, section 8).
//!
//! A row's `Type` names what it measures and `Total` the progress that completes it. The tiers
//! of one measure share a counter: the first tier names it in `ProgressId` with its filters
//! (`VitalsCategories`, `IsPvP`, `InGroup`, `FactionType`, `TradeskillType`), the later tiers
//! name it in `ProgressIdParent` and have no filters of their own (Observed, content bundle
//! 25810808: `PvE_KillsWhileInGroup1` to `5`). The client names a meta achievement by the CRC-32
//! of its lowercase id and a window tab by the CRC-32 of its lowercase `UIDisplayCategory`
//! (retail menus-linux 2026-09-12: 3794 `a5aa3c61` is `Places_Discovered`, and the answer listed
//! its 8 meta achievements).

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The `Total` of a row that names none: one event completes it (Observed: every
/// `BinaryAchievement` row says 1).
const ONE_EVENT: u32 = 1;
/// The `NotificationVerbosity` of a row whose player hears of it at 75 % as well as at 100 %.
const THREE_QUARTERS_VERBOSITY: &str = "SeventyFiveOneHundred";
/// Separates the ids of a list cell.
const LIST_SEPARATOR: char = ',';
/// The `IsPvP` / `InGroup` cells.
const YES: &str = "YES";
const ANY: &str = "ANY";
/// The `FactionType` cell of a row that takes every faction, and none.
const ANY_FACTION: &str = "Any";

/// The `Type` cells this server measures.
mod types {
    pub const CATEGORY_KILLS: &str = "CategoryKills";
    pub const TOTAL_KILLS: &str = "TotalKills";
    pub const CHARACTER_LEVEL: &str = "CharacterLevel";
    pub const CATEGORICAL_PROGRESSION: &str = "CategoricalProgression";
    pub const RANK_TABLE_ANY: &str = "RankTableIdProgressionAny";
    pub const META_ACHIEVEMENT_COUNT: &str = "MetaAchievementCount";
    pub const TITLE_COUNT: &str = "TitleCount";
    pub const BINARY_ACHIEVEMENT: &str = "BinaryAchievement";
    pub const ACHIEVEMENT_CATEGORY_COUNT: &str = "AchievementCategoryCount";
    pub const QUEST_TOTAL: &str = "QuestTotal";
    pub const QUEST_ARC: &str = "QuestArc";
    pub const GATHERING: &str = "Gathering";
    pub const PLAYER_DEATH: &str = "PlayerDeath";
}

/// Whether a kill's side must be so: `IsPvP` or `InGroup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// `YES`: it must be.
    Yes,
    /// `NO` or empty: it must not be.
    No,
    /// `ANY`.
    Any,
}

impl Side {
    fn of(cell: Option<&str>) -> Self {
        match cell.map(str::trim) {
            Some(cell) if cell.eq_ignore_ascii_case(YES) => Self::Yes,
            Some(cell) if cell.eq_ignore_ascii_case(ANY) => Self::Any,
            _ => Self::No,
        }
    }

    /// Whether a kill that is (`is`) or is not on this side counts.
    pub const fn allows(self, is: bool) -> bool {
        match self {
            Self::Yes => is,
            Self::No => !is,
            Self::Any => true,
        }
    }
}

/// What one meta achievement measures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaKind {
    /// `CategoryKills` and `TotalKills`: kills of mobs with every one of `categories` (none for
    /// any mob), on the `pvp` and `group` sides, for `faction` (`None` for any).
    Kills {
        /// `VitalsCategories`, lowercase.
        categories: Vec<String>,
        /// `IsPvP`.
        pvp: Side,
        /// `InGroup`.
        group: Side,
        /// `FactionType` unless `Any`.
        faction: Option<String>,
    },
    /// `CharacterLevel`: the zero-based level.
    CharacterLevel,
    /// `CategoricalProgression`: the rank of the categorical progression.
    Progression(String),
    /// `RankTableIdProgressionAny`: the highest rank of the progressions on the rank table.
    RankTableAny(String),
    /// `MetaAchievementCount`: the meta achievements completed.
    MetaAchievementCount,
    /// `TitleCount`: the titles earned.
    TitleCount,
    /// `BinaryAchievement`: 1 once every one of the achievements is set.
    Achievements(Vec<String>),
    /// `AchievementCategoryCount`: the set achievements of the `AchievementData` category.
    AchievementCategory(String),
    /// `QuestTotal`: the objectives done.
    QuestTotal,
    /// `QuestArc`: 1 once every objective whose `QuestArcTag` is the tag is done.
    QuestArc(String),
    /// `Gathering`: gathers of the tradeskill.
    Gathering(String),
    /// `PlayerDeath`: deaths.
    Deaths,
    /// A measure of a system this server does not run (the `Type`): never progresses.
    Untracked(String),
}

impl MetaKind {
    /// Whether its progress is a counter kept per player (kills, gathers, deaths); else it is
    /// measured from the player's state.
    pub const fn counted(&self) -> bool {
        matches!(self, Self::Kills { .. } | Self::Gathering(_) | Self::Deaths)
    }

    /// Whether a solo PvE kill of a mob with `categories` (any case) counts for it.
    pub fn counts_kill(&self, categories: &[String]) -> bool {
        let Self::Kills {
            categories: wanted,
            pvp,
            group,
            faction,
        } = self
        else {
            return false;
        };
        pvp.allows(false)
            && group.allows(false)
            && faction.is_none()
            && wanted.iter().all(|wanted| {
                categories
                    .iter()
                    .any(|category| category.eq_ignore_ascii_case(wanted))
            })
    }
}

/// One meta achievement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaRule {
    /// `MetaAchievementId`.
    pub id: String,
    /// The CRC-32 of the lowercase id: its id on the wire.
    pub crc: u32,
    /// The CRC-32 of the lowercase `UIDisplayCategory`: its window tab.
    pub category: u32,
    /// `Total`: the progress that completes it.
    pub total: u32,
    /// The lowercase key of its counter: its `ProgressId`, else its `ProgressIdParent`, else
    /// its id.
    pub counter: String,
    /// What it measures (a later tier measures what its first tier does).
    pub kind: MetaKind,
    /// Whether the player hears of it at 75 % too (`NotificationVerbosity`).
    pub notify_three_quarters: bool,
    /// `PredecessorMetaAchievementIds`: those to complete first.
    pub predecessors: Vec<String>,
    /// `HideFromUI`: the window does not show it.
    pub hidden: bool,
}

/// One `MetaAchievementData` row as [`MetaAchievementRules::build`] takes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetaRow<'a> {
    /// `MetaAchievementId`.
    pub id: &'a str,
    /// `Type`.
    pub kind: &'a str,
    /// `UIDisplayCategory`.
    pub category: &'a str,
    /// `Total`.
    pub total: Option<i64>,
    /// `ProgressId`.
    pub progress_id: Option<&'a str>,
    /// `ProgressIdParent`.
    pub progress_parent: Option<&'a str>,
    /// `NotificationVerbosity`.
    pub verbosity: &'a str,
    /// `PredecessorMetaAchievementIds`.
    pub predecessors: Option<&'a str>,
    /// `HideFromUI`.
    pub hidden: bool,
    /// `VitalsCategories`.
    pub vitals_categories: Option<&'a str>,
    /// `IsPvP`.
    pub is_pvp: Option<&'a str>,
    /// `InGroup`.
    pub in_group: Option<&'a str>,
    /// `FactionType`.
    pub faction: Option<&'a str>,
    /// `CategoricalProgression`.
    pub progression: Option<&'a str>,
    /// `RankTableId`.
    pub rank_table: Option<&'a str>,
    /// `AchievementsId`.
    pub achievements: Option<&'a str>,
    /// `AchievementCategory`.
    pub achievement_category: Option<&'a str>,
    /// `QuestGroupTag`.
    pub quest_group: Option<&'a str>,
    /// `TradeskillType`.
    pub tradeskill: Option<&'a str>,
}

/// Every meta achievement of the game data, with the objectives of each quest arc and the
/// achievements of each achievement category.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaAchievementRules {
    rules: Vec<MetaRule>,
    by_id: HashMap<String, usize>,
    by_crc: HashMap<u32, usize>,
    /// The objective ids of each lowercase `QuestArcTag`.
    arcs: HashMap<String, Vec<String>>,
    /// The achievement ids of each lowercase `AchievementData` category.
    achievement_categories: HashMap<String, Vec<String>>,
}

impl MetaAchievementRules {
    /// The meta achievements of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let rows = tables
            .meta_achievement_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| MetaRow {
                id: &row.meta_achievement_id,
                kind: &row.r#type,
                category: &row.ui_display_category,
                total: row.total,
                progress_id: row.progress_id.as_deref(),
                progress_parent: row.progress_id_parent.as_deref(),
                verbosity: &row.notification_verbosity,
                predecessors: row.predecessor_meta_achievement_ids.as_deref(),
                hidden: row.hide_from_ui.unwrap_or_default(),
                vitals_categories: row.vitals_categories.as_deref(),
                is_pvp: row.is_pvp.as_deref(),
                in_group: row.in_group.as_deref(),
                faction: row.faction_type.as_deref(),
                progression: row.categorical_progression.as_deref(),
                rank_table: row.rank_table_id.as_deref(),
                achievements: row.achievements_id.as_deref(),
                achievement_category: row.achievement_category.as_deref(),
                quest_group: row.quest_group_tag.as_deref(),
                tradeskill: row.tradeskill_type.as_deref(),
            });
        let arcs = tables
            .objectives
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| Some((row.quest_arc_tag.as_deref()?, row.objective_id.as_str())));
        let achievements = tables
            .achievement_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| Some((row.category.as_deref()?, row.achievement_id.as_str())));
        Self::build(rows, arcs, achievements)
    }

    /// The rules of `rows`, the quest `arcs` (`QuestArcTag`, objective id) and the
    /// `achievements` (category, achievement id). The first row of an id wins.
    pub fn build<'a>(
        rows: impl IntoIterator<Item = MetaRow<'a>>,
        arcs: impl IntoIterator<Item = (&'a str, &'a str)>,
        achievements: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let rows: Vec<MetaRow<'a>> = rows.into_iter().collect();
        // The first tier of each counter, whose filters the later tiers take.
        let heads: HashMap<String, &MetaRow<'a>> = rows
            .iter()
            .filter_map(|row| Some((present(row.progress_id)?.to_lowercase(), row)))
            .collect();
        let mut rules = Self::default();
        for row in &rows {
            let key = row.id.to_lowercase();
            if rules.by_id.contains_key(&key) {
                continue;
            }
            let counter = present(row.progress_id)
                .or_else(|| present(row.progress_parent))
                .map_or_else(|| key.clone(), str::to_lowercase);
            let head = heads.get(&counter).copied().unwrap_or(row);
            let index = rules.rules.len();
            rules.rules.push(MetaRule {
                id: row.id.to_owned(),
                crc: lowercase_crc32(row.id),
                category: lowercase_crc32(row.category),
                total: row
                    .total
                    .and_then(|total| u32::try_from(total).ok())
                    .unwrap_or(ONE_EVENT),
                counter,
                kind: kind_of(head),
                notify_three_quarters: row.verbosity == THREE_QUARTERS_VERBOSITY,
                predecessors: list(row.predecessors),
                hidden: row.hidden,
            });
            rules.by_id.insert(key, index);
            rules.by_crc.insert(lowercase_crc32(row.id), index);
        }
        for (tag, objective) in arcs {
            rules
                .arcs
                .entry(tag.to_lowercase())
                .or_default()
                .push(objective.to_owned());
        }
        for (category, achievement) in achievements {
            rules
                .achievement_categories
                .entry(category.to_lowercase())
                .or_default()
                .push(achievement.to_owned());
        }
        rules
    }

    /// Every meta achievement, in sheet order.
    pub fn rules(&self) -> &[MetaRule] {
        &self.rules
    }

    /// The meta achievement `id` (any case).
    pub fn rule(&self, id: &str) -> Option<&MetaRule> {
        self.by_id
            .get(&id.to_lowercase())
            .map(|index| &self.rules[*index])
    }

    /// The meta achievement whose wire id is `crc`.
    pub fn rule_by_crc(&self, crc: u32) -> Option<&MetaRule> {
        self.by_crc.get(&crc).map(|index| &self.rules[*index])
    }

    /// The meta achievements of the window tab `category` (CRC-32 of the lowercase
    /// `UIDisplayCategory`), by ascending wire id (retail order of a 3800 answer).
    pub fn in_category(&self, category: u32) -> Vec<&MetaRule> {
        let mut rules: Vec<&MetaRule> = self
            .rules
            .iter()
            .filter(|rule| rule.category == category)
            .collect();
        rules.sort_by_key(|rule| rule.crc);
        rules
    }

    /// The objective ids of the quest arc `tag` (any case).
    pub fn arc_objectives(&self, tag: &str) -> &[String] {
        self.arcs
            .get(&tag.to_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// The achievement ids of the `AchievementData` category `category` (any case).
    pub fn category_achievements(&self, category: &str) -> &[String] {
        self.achievement_categories
            .get(&category.to_lowercase())
            .map_or(&[], Vec::as_slice)
    }
}

/// What the first tier `row` of a counter measures.
fn kind_of(row: &MetaRow<'_>) -> MetaKind {
    let text = |cell: Option<&str>| present(cell).map(str::to_owned);
    match row.kind {
        types::CATEGORY_KILLS | types::TOTAL_KILLS => MetaKind::Kills {
            categories: list(row.vitals_categories)
                .into_iter()
                .map(|category| category.to_lowercase())
                .collect(),
            pvp: Side::of(row.is_pvp),
            group: Side::of(row.in_group),
            faction: present(row.faction)
                .filter(|faction| !faction.eq_ignore_ascii_case(ANY_FACTION))
                .map(str::to_owned),
        },
        types::CHARACTER_LEVEL => MetaKind::CharacterLevel,
        types::CATEGORICAL_PROGRESSION => {
            text(row.progression).map_or_else(|| untracked(row), MetaKind::Progression)
        }
        types::RANK_TABLE_ANY => {
            text(row.rank_table).map_or_else(|| untracked(row), MetaKind::RankTableAny)
        }
        types::META_ACHIEVEMENT_COUNT => MetaKind::MetaAchievementCount,
        types::TITLE_COUNT => MetaKind::TitleCount,
        types::BINARY_ACHIEVEMENT => MetaKind::Achievements(list(row.achievements)),
        types::ACHIEVEMENT_CATEGORY_COUNT => text(row.achievement_category)
            .map_or_else(|| untracked(row), MetaKind::AchievementCategory),
        types::QUEST_TOTAL => MetaKind::QuestTotal,
        types::QUEST_ARC => {
            text(row.quest_group).map_or_else(|| untracked(row), MetaKind::QuestArc)
        }
        types::GATHERING => {
            text(row.tradeskill).map_or_else(|| untracked(row), MetaKind::Gathering)
        }
        types::PLAYER_DEATH => MetaKind::Deaths,
        _ => untracked(row),
    }
}

fn untracked(row: &MetaRow<'_>) -> MetaKind {
    MetaKind::Untracked(row.kind.to_owned())
}

/// A cell with text, trimmed.
fn present(cell: Option<&str>) -> Option<&str> {
    cell.map(str::trim).filter(|cell| !cell.is_empty())
}

/// The ids of a comma-separated cell.
fn list(cell: Option<&str>) -> Vec<String> {
    present(cell)
        .map(|cell| {
            cell.split(LIST_SEPARATOR)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Places_Discovered`, the tab of retail's first 3794 (menus-linux 2026-09-12).
    const PLACES_TAB: u32 = 0xa5aa_3c61;
    /// `POIVisitedCount1`, one of its meta achievements.
    const POI_1: u32 = 0x39a2_2307;

    fn rows() -> Vec<MetaRow<'static>> {
        vec![
            MetaRow {
                id: "PvE_FamilyKills_Corrupted1",
                kind: "CategoryKills",
                category: "Enemies_By_Type",
                total: Some(250),
                progress_id: Some("PvE_FamilyKills_Corrupted"),
                verbosity: "OneHundred",
                vitals_categories: Some("Named,Corrupted"),
                is_pvp: Some("NO"),
                in_group: Some("ANY"),
                ..MetaRow::default()
            },
            MetaRow {
                id: "PvE_FamilyKills_Corrupted2",
                kind: "CategoryKills",
                category: "Enemies_By_Type",
                total: Some(1_000),
                progress_parent: Some("PvE_FamilyKills_Corrupted"),
                verbosity: "SeventyFiveOneHundred",
                ..MetaRow::default()
            },
            MetaRow {
                id: "PvE_KillsWhileInGroup1",
                kind: "TotalKills",
                category: "PvE_Kill_Counts",
                total: Some(100),
                progress_id: Some("PvE_KillsWhileInGroup"),
                verbosity: "OneHundred",
                is_pvp: Some("NO"),
                in_group: Some("YES"),
                faction: Some("Any"),
                ..MetaRow::default()
            },
            MetaRow {
                id: "POIVisitedCount1",
                kind: "PoiChartedCount",
                category: "Places_Discovered",
                total: Some(25),
                verbosity: "OneHundred",
                ..MetaRow::default()
            },
            MetaRow {
                id: "CharacterProgression2",
                kind: "CharacterLevel",
                category: "Overall",
                total: Some(19),
                verbosity: "OneHundred",
                predecessors: Some("CharacterProgression1"),
                ..MetaRow::default()
            },
            MetaRow {
                id: "QuestArc_FishTraining",
                kind: "QuestArc",
                category: "Territory_Stories",
                total: Some(1),
                verbosity: "OneHundred",
                quest_group: Some("FishingQuestsArc"),
                ..MetaRow::default()
            },
            MetaRow {
                id: "Lore_Brimstone_AncientGlyphs",
                kind: "BinaryAchievement",
                category: "Lore_Pages",
                verbosity: "OneHundred",
                achievements: Some("Lore_A, Lore_B"),
                ..MetaRow::default()
            },
            MetaRow {
                id: "POIVisitedCount1",
                kind: "Duplicate",
                category: "Other",
                verbosity: "OneHundred",
                ..MetaRow::default()
            },
        ]
    }

    fn rules() -> MetaAchievementRules {
        MetaAchievementRules::build(
            rows(),
            [
                ("FishingQuestsArc", "C91_FQ_01_UnpathedWaters"),
                ("FishingQuestsArc", "C91_FQ_08_FisherKing"),
            ],
            [("Recipe", "Recipe_A")],
        )
    }

    #[test]
    fn later_tiers_count_on_the_first_tiers_counter_and_filters() {
        let rules = rules();
        let second = rules.rule("pve_familykills_corrupted2").expect("tier 2");
        assert_eq!(second.counter, "pve_familykills_corrupted");
        assert_eq!(second.total, 1_000);
        assert!(second.notify_three_quarters);
        let named_corrupted = [
            "Hostile".to_owned(),
            "Named".to_owned(),
            "Corrupted".to_owned(),
        ];
        assert!(second.kind.counts_kill(&named_corrupted));
        assert!(
            second
                .kind
                .counts_kill(&["CORRUPTED".to_owned(), "named".to_owned()])
        );
        assert!(
            !second.kind.counts_kill(&["Corrupted".to_owned()]),
            "not named"
        );
        assert!(second.kind.counted());
        let grouped = rules.rule("PvE_KillsWhileInGroup1").expect("group");
        assert!(
            !grouped.kind.counts_kill(&named_corrupted),
            "a solo kill is no kill in a group"
        );
        assert_eq!(
            rules
                .rule("CharacterProgression2")
                .map(|rule| &rule.predecessors),
            Some(&vec!["CharacterProgression1".to_owned()])
        );
        assert_eq!(
            rules
                .rule("CharacterProgression2")
                .map(|rule| &rule.counter),
            Some(&"characterprogression2".to_owned())
        );
    }

    #[test]
    fn rows_name_their_kind_and_tab() {
        let rules = rules();
        assert_eq!(rules.rules().len(), 7, "the first row of an id wins");
        let poi = rules.rule_by_crc(POI_1).expect("poi");
        assert_eq!(poi.kind, MetaKind::Untracked("PoiChartedCount".to_owned()));
        assert_eq!(poi.category, PLACES_TAB);
        assert_eq!(rules.in_category(PLACES_TAB).len(), 1);
        let glyphs = rules.rule("Lore_Brimstone_AncientGlyphs").expect("binary");
        assert_eq!(glyphs.total, ONE_EVENT);
        assert_eq!(
            glyphs.kind,
            MetaKind::Achievements(vec!["Lore_A".to_owned(), "Lore_B".to_owned()])
        );
        assert_eq!(rules.arc_objectives("fishingquestsarc").len(), 2);
        assert_eq!(
            rules.category_achievements("RECIPE"),
            ["Recipe_A".to_owned()]
        );
        assert!(rules.category_achievements("none").is_empty());
        assert_eq!(
            rules.rule("QuestArc_FishTraining").map(|rule| &rule.kind),
            Some(&MetaKind::QuestArc("FishingQuestsArc".to_owned()))
        );
    }

    #[test]
    fn sides_follow_their_cells() {
        assert!(Side::of(Some("YES")).allows(true));
        assert!(!Side::of(Some("YES")).allows(false));
        assert!(Side::of(Some("ANY")).allows(false));
        assert!(Side::of(None).allows(false));
        assert!(!Side::of(Some("NO")).allows(true));
    }
}
