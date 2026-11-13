// SPDX-License-Identifier: AGPL-3.0-or-later
//! The town projects of a territory (`javelindata_territoryprogression`,
//! `TerritoryProgressionData`) and the town board missions that add points to them
//! (`MissionData.TerritoryProjectId`, `CommunityGoalAmount`).
//!
//! Observed (client build 25810808, wars and sieges findings "Town projects"):
//!
//! - `ProgressionCategory` names a `SettlementProgressionCategory` (client enum `FUN_142f94920`):
//!   its value is the upgrade id of the claims' progression (Blacksmithing 0 to Smelting 9,
//!   FortGates 11 to ExplosiveUpgrade 17). A lifestyle category (`LumberJackSpirit`, ...) is not
//!   in the enum: such a project raises no upgrade.
//! - `ProjectType` names a `TerritoryProgressionType` (client enum `FUN_1444d8dd0`): Fortress 1,
//!   Settlement 2, Lifestyle 3, AlwaysAvailable 6, Event 7. A territory runs one project per type.
//! - A completed project sets its category's tier to its `Level` (the claims' tier rose to the
//!   completed project's level in every retail completion of the captures).
//! - A town board mission names the first project of its category (`CookingProjectId`); retail's
//!   client then sends the type's running project (`CookingProjectId3`) with the mission (3819).
//!
//! Inferred: `Cost` is in the wallet's unit (hundredths of a coin, as the upkeep cost of the same
//! governance data); `CompletionTimeMinutes` is how long a started project runs.

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// `TerritoryProgressionType` Fortress.
pub const FORTRESS: u8 = 1;
/// `TerritoryProgressionType` Settlement.
pub const SETTLEMENT: u8 = 2;
/// `TerritoryProgressionType` Lifestyle.
pub const LIFESTYLE: u8 = 3;
/// `TerritoryProgressionType` AlwaysAvailable.
pub const ALWAYS_AVAILABLE: u8 = 6;
/// `TerritoryProgressionType` Event.
pub const EVENT: u8 = 7;

/// `TerritoryProgressionType` by name (client enum `FUN_1444d8dd0`).
const PROGRESSION_TYPES: [(&str, u8); 7] = [
    ("Fortress", FORTRESS),
    ("Settlement", SETTLEMENT),
    ("Lifestyle", LIFESTYLE),
    ("WarPrep", 4),
    ("WeakenInvasion", 5),
    ("AlwaysAvailable", ALWAYS_AVAILABLE),
    ("Event", EVENT),
];

/// `SettlementProgressionCategory` by name (client enum `FUN_142f94920`).
const CATEGORIES: [(&str, u8); 22] = [
    ("Blacksmithing", 0),
    ("Engineering", 1),
    ("Outfitting", 2),
    ("Cooking", 3),
    ("Alchemy", 4),
    ("Carpentry", 5),
    ("Masonry", 6),
    ("Weaving", 7),
    ("Tanning", 8),
    ("Smelting", 9),
    ("Church", 10),
    ("FortGates", 11),
    ("FortHardPoints", 12),
    ("BallistaUpgrade", 13),
    ("FireBarrelUpgrade", 14),
    ("RepeaterUpgrade", 15),
    ("HornUpgrade", 16),
    ("ExplosiveUpgrade", 17),
    ("TradingPost", 18),
    ("AlwaysAvailable", 19),
    ("Event", 20),
    ("SummerEvent", 21),
];

/// Milliseconds per minute.
const MS_PER_MINUTE: u64 = 60_000;

/// One town project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerritoryProject {
    /// `ProjectId`.
    pub id: String,
    /// The CRC-32 of the lowercase `ProjectId` (the client's `Crc32` of the project).
    pub crc: u32,
    /// Its `TerritoryProgressionType`.
    pub kind: u8,
    /// The upgrade id it raises; `None` for a project that raises none (lifestyle, always
    /// available).
    pub upgrade: Option<u8>,
    /// `Level`: the tier its upgrade gets when it completes.
    pub level: u8,
    /// `Cost`, hundredths of a coin.
    pub cost: u64,
    /// `TerritoryProgressionNeeded`: the points it needs.
    pub needed: u32,
    /// `CompletionTimeMinutes` in ms: how long it runs.
    pub run_ms: u64,
}

/// What a town board mission adds to a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissionGoal {
    /// The `TerritoryProgressionType` of its `TerritoryProjectId`.
    pub kind: u8,
    /// The upgrade id of its `TerritoryProjectId`, when it raises one.
    pub upgrade: Option<u8>,
    /// `CommunityGoalAmount`: the points it adds.
    pub amount: u32,
}

/// Every town project and the missions that add points to them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerritoryProjects {
    by_crc: HashMap<u32, TerritoryProject>,
    by_upgrade_level: HashMap<(u8, u8), u32>,
    goals: HashMap<u32, MissionGoal>,
}

impl TerritoryProjects {
    /// The project rows and the town board missions of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let projects = tables
            .territory_progression_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                project_of(
                    &row.project_id,
                    &row.project_type,
                    &row.progression_category,
                    row.level,
                    row.cost,
                    row.territory_progression_needed,
                    row.completion_time_minutes,
                )
            });
        let missions = tables
            .mission_data
            .iter()
            .flat_map(|table| &table.rows)
            .filter(|row| row.enabled != Some(false))
            .filter_map(|row| {
                Some((
                    row.mission_id.as_str(),
                    row.territory_project_id.as_deref()?,
                    u32::try_from(row.community_goal_amount?).ok()?,
                ))
            });
        Self::build(projects, missions)
    }

    /// The table of `projects` and of `missions` (mission id, its `TerritoryProjectId`, its
    /// `CommunityGoalAmount`); the first row of an id wins, a mission of an unknown project or
    /// without points is left out.
    pub fn build<'a>(
        projects: impl IntoIterator<Item = TerritoryProject>,
        missions: impl IntoIterator<Item = (&'a str, &'a str, u32)>,
    ) -> Self {
        let mut table = Self::default();
        for project in projects {
            if let Some(upgrade) = project.upgrade {
                table
                    .by_upgrade_level
                    .entry((upgrade, project.level))
                    .or_insert(project.crc);
            }
            table.by_crc.entry(project.crc).or_insert(project);
        }
        for (mission, project, amount) in missions {
            let Some(project) = table.by_crc.get(&lowercase_crc32(project)) else {
                continue;
            };
            if amount == 0 {
                continue;
            }
            let goal = MissionGoal {
                kind: project.kind,
                upgrade: project.upgrade,
                amount,
            };
            table.goals.entry(lowercase_crc32(mission)).or_insert(goal);
        }
        table
    }

    /// The project of `crc`.
    pub fn project(&self, crc: u32) -> Option<&TerritoryProject> {
        self.by_crc.get(&crc)
    }

    /// The project that raises `upgrade` to `level`.
    pub fn of_level(&self, upgrade: u8, level: u8) -> Option<&TerritoryProject> {
        self.by_crc
            .get(self.by_upgrade_level.get(&(upgrade, level))?)
    }

    /// The projects that raise `upgrade` up to `tier`, by level: the completed ones of a
    /// territory with that tier.
    pub fn completed(&self, upgrade: u8, tier: u8) -> impl Iterator<Item = &TerritoryProject> {
        (1..=tier).filter_map(move |level| self.of_level(upgrade, level))
    }

    /// The goal of the mission whose lowercase id has the CRC-32 `mission`.
    pub fn goal(&self, mission: u32) -> Option<MissionGoal> {
        self.goals.get(&mission).copied()
    }
}

/// The project of one row, or `None` for a row whose numbers do not fit or whose type is not a
/// client `TerritoryProgressionType`.
fn project_of(
    id: &str,
    kind: &str,
    category: &str,
    level: i64,
    cost: i64,
    needed: i64,
    minutes: i64,
) -> Option<TerritoryProject> {
    let kind = named(&PROGRESSION_TYPES, kind)?;
    Some(TerritoryProject {
        id: id.to_owned(),
        crc: lowercase_crc32(id),
        kind,
        upgrade: named(&CATEGORIES, category),
        level: u8::try_from(level).ok()?,
        cost: u64::try_from(cost).ok()?,
        needed: u32::try_from(needed).ok()?,
        run_ms: u64::try_from(minutes).ok()?.checked_mul(MS_PER_MINUTE)?,
    })
}

/// The value of `name` in `names`.
fn named(names: &[(&str, u8)], name: &str) -> Option<u8> {
    names
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build 25810808 rows (`javelindata_territoryprogression`).
    const COOKING_1: &str = "CookingProjectId";
    const COOKING_3: &str = "CookingProjectId3";
    const DAY_MINUTES: i64 = 1440;
    const DAY_MS: u64 = 86_400_000;
    const COOKING_UPGRADE: u8 = 3;
    /// A town board mission of the cooking projects (`CommunityGoalAmount` 40).
    const MISSION: &str = "CookingProjectId_1_Faction_Armaments10_T3_1";
    const MISSION_POINTS: u32 = 40;

    fn table() -> TerritoryProjects {
        let rows = [
            (COOKING_1, "Settlement", "Cooking", 1, 10_000, 900),
            (
                "CookingProjectId2",
                "Settlement",
                "Cooking",
                2,
                500_000,
                1500,
            ),
            (COOKING_3, "Settlement", "Cooking", 3, 1_000_000, 3000),
            (
                "HornUpgradeProjectId",
                "Fortress",
                "HornUpgrade",
                1,
                50_000,
                900,
            ),
            (
                "MinersResolveProjectId",
                "Lifestyle",
                "MinersResolve",
                1,
                250_000,
                1000,
            ),
            ("BadType", "NoSuchType", "Cooking", 1, 1, 1),
        ];
        TerritoryProjects::build(
            rows.into_iter()
                .filter_map(|(id, kind, category, level, cost, needed)| {
                    project_of(id, kind, category, level, cost, needed, DAY_MINUTES)
                }),
            [
                (MISSION, COOKING_1, MISSION_POINTS),
                ("MinersResolveProjectId_1_x", "MinersResolveProjectId", 25),
                ("NoProject_1", "NoSuchProjectId", 10),
                ("NoPoints_1", COOKING_1, 0),
            ],
        )
    }

    #[test]
    fn a_project_names_its_type_upgrade_level_and_run_time() {
        let table = table();
        let cooking = table.project(lowercase_crc32(COOKING_3)).expect("row");
        assert_eq!(cooking.kind, SETTLEMENT);
        assert_eq!(cooking.upgrade, Some(COOKING_UPGRADE));
        assert_eq!(
            (cooking.level, cooking.cost, cooking.needed),
            (3, 1_000_000, 3000)
        );
        assert_eq!(cooking.run_ms, DAY_MS);
        let horn = table.of_level(16, 1).expect("horn");
        assert_eq!(horn.kind, FORTRESS);
        let lifestyle = table
            .project(lowercase_crc32("MinersResolveProjectId"))
            .expect("lifestyle");
        assert_eq!((lifestyle.kind, lifestyle.upgrade), (LIFESTYLE, None));
        assert!(table.project(lowercase_crc32("BadType")).is_none());
    }

    #[test]
    fn the_completed_projects_of_a_tier_are_its_levels() {
        let table = table();
        let ids: Vec<&str> = table
            .completed(COOKING_UPGRADE, 2)
            .map(|project| project.id.as_str())
            .collect();
        assert_eq!(ids, [COOKING_1, "CookingProjectId2"]);
        assert_eq!(table.completed(COOKING_UPGRADE, 0).count(), 0);
        assert_eq!(table.of_level(COOKING_UPGRADE, 4), None);
    }

    #[test]
    fn a_mission_adds_its_points_to_its_projects_type_and_category() {
        let table = table();
        assert_eq!(
            table.goal(lowercase_crc32(MISSION)),
            Some(MissionGoal {
                kind: SETTLEMENT,
                upgrade: Some(COOKING_UPGRADE),
                amount: MISSION_POINTS,
            })
        );
        assert_eq!(
            table.goal(lowercase_crc32("MinersResolveProjectId_1_x")),
            Some(MissionGoal {
                kind: LIFESTYLE,
                upgrade: None,
                amount: 25,
            })
        );
        assert_eq!(table.goal(lowercase_crc32("NoProject_1")), None);
        assert_eq!(table.goal(lowercase_crc32("NoPoints_1")), None);
    }
}
