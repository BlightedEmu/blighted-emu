// SPDX-License-Identifier: AGPL-3.0-or-later
//! Town projects (wars and sieges findings "Town projects", 2026-11-13): the projects that raise
//! a territory's upgrades, their points from the town board missions, and what the claims and
//! the territory planning board show of them.
//!
//! **Observed**
//!
//! - The planning board (interaction kind [`PLANNING_BOARD_KIND`],
//!   `@ui_territory_board_territory_planning`) gets a 2722 and the board's 1746 with the
//!   progression data and the governance data (`territory-status` 1782906884737).
//! - The progression data lists one running project per `TerritoryProgressionType` in the order
//!   [`SLOT_ORDER`] with its end and points, the completed upgrade projects of the territory
//!   (time 0) and the running lifestyle buffs (`Hivetownboardmissions` 1781378033421).
//! - The claims (3086) carry one `(project, type, state)` entry per type; an empty slot is
//!   `(0, type, Expired)`, the always available project is `CommunityProjectId` Completed in every
//!   retail claim; a completed project is Blocking while its category's tier shows its level.
//! - A start (453) names the project and the wallet; the client's own handler checks the company
//!   privilege 27 and pays the project's `Cost`. A cancel (2739) names the project.
//! - A turned-in town board mission adds its `CommunityGoalAmount` to the running project of its
//!   type (the points rose by 10 and 40 at the turn-ins of `Hivetownboardmissions`).
//!
//! **Inferred**
//!
//! - A start comes from the planning board the player has open, for a project of the Fortress or
//!   Settlement type whose category's tier is one below its level, in a free slot; it runs
//!   `CompletionTimeMinutes`. A cancel needs the board open and names the running project.
//! - A mission adds points only to a running project of its own category (one runs per type).
//! - A project that ends Active, a Blocking one that ends and a cancelled one change the claims
//!   at the next read of the territories ([`super::land_claims::OWNER_READ_MS`]).
//! - The 1746's single entry stays zero (as in every retail body); the always available slot of
//!   the board has no end time (retail's was in the past; its meaning is Unknown).
//!
//! **Not run here**: lifestyle projects (they raise no upgrade; their buffs are status effects of
//! the territory's players), so the lifestyle slot is always empty and a lifestyle start is
//! refused.

use std::collections::BTreeMap;

use nw_bus::{ConnectionRef, SocialInput, TownProjectAnswer, TownProjectCancel, TownProjectStart};
use nw_content::territory_projects::{
    ALWAYS_AVAILABLE, EVENT, FORTRESS, LIFESTYLE, SETTLEMENT, TerritoryProjects,
};
use nw_db::{
    DbError, PROJECT_ACTIVE, PROJECT_BLOCKING, PROJECT_COMPLETED, PROJECT_EXPIRED, PgPool,
    TownProject, advance_town_projects, town_projects,
};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::{
    ProgressionEntry, ProjectEntry, ProjectSlot, ProjectTime, TerritoryProgressionData,
    TownProjectCallMsg,
};

use nw_messages::wire::RAW_ID_LEN;
use uuid::Uuid;

use super::invasions::UPGRADE_IDS;
use super::{Player, SimWorld};

/// The interaction kind of a territory planning board (Observed: 3743 kind 55
/// `@ui_territory_board_territory_planning`, `territory-status` 1782906884737).
pub(super) const PLANNING_BOARD_KIND: u8 = 55;

/// The progression types of the planning board's slots, in retail order (Observed in every
/// 1746 body).
const SLOT_ORDER: [u8; 5] = [LIFESTYLE, EVENT, ALWAYS_AVAILABLE, SETTLEMENT, FORTRESS];

/// The progression types of the claims' entries, in one retail order (Observed: the claims list
/// the same five types in several orders; this is one of the most seen).
const ENTRY_ORDER: [u8; 5] = [ALWAYS_AVAILABLE, SETTLEMENT, FORTRESS, LIFESTYLE, EVENT];

/// The project of the always available slot (Observed in every retail claim).
const ALWAYS_AVAILABLE_PROJECT: &str = "CommunityProjectId";

/// Nanoseconds per millisecond.
const NS_PER_MS: u64 = 1_000_000;

/// Metric: town project calls this sim refused, by `reason`.
const METRIC_REFUSED: &str = "sim_town_project_calls_refused_total";
const REASON_LABEL: &str = "reason";

/// Why this sim refused a start or cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The player has no planning board open.
    NoBoard,
    /// No project has that id.
    UnknownProject,
    /// The project raises no upgrade (lifestyle projects are not run here).
    NoUpgrade,
    /// No company owns the territory, or not the player's.
    NotOwner,
    /// The project's slot runs a project.
    SlotBusy,
    /// The upgrade's tier is not one below the project's level.
    WrongTier,
    /// The slot does not run that project.
    NotRunning,
}

impl Refusal {
    const fn label(self) -> &'static str {
        match self {
            Self::NoBoard => "no_board",
            Self::UnknownProject => "unknown_project",
            Self::NoUpgrade => "no_upgrade",
            Self::NotOwner => "not_owner",
            Self::SlotBusy => "slot_busy",
            Self::WrongTier => "wrong_tier",
            Self::NotRunning => "not_running",
        }
    }
}

/// Points of a turned-in mission for a territory's running project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectPoints {
    /// The territory id.
    pub(crate) territory: u16,
    /// The project's progression type.
    pub(crate) kind: u8,
    /// The upgrade id of the mission's category.
    pub(crate) upgrade: u8,
    /// The points.
    pub(crate) points: u32,
    /// When, Unix ms.
    pub(crate) at_ms: u64,
}

/// The town projects of every territory, as last read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProjectSlots {
    slots: BTreeMap<(u16, u8), TownProject>,
}

impl ProjectSlots {
    /// Ends the projects due at Unix ms `now_ms`, then reads every project.
    pub(crate) async fn read(world: &PgPool, now_ms: u64) -> Result<Self, DbError> {
        advance_town_projects(world, now_ms).await?;
        Ok(Self::of(town_projects(world).await?))
    }

    pub(crate) fn of(projects: Vec<TownProject>) -> Self {
        Self {
            slots: projects
                .into_iter()
                .map(|project| ((project.territory, project.kind), project))
                .collect(),
        }
    }

    /// The project of `territory`'s slot of type `kind`.
    fn slot(&self, territory: u16, kind: u8) -> Option<&TownProject> {
        self.slots.get(&(territory, kind))
    }

    /// Whether `territory`'s slot of type `kind` runs a project (Active or Blocking) at Unix ms
    /// `now_ms`.
    fn busy(&self, territory: u16, kind: u8, now_ms: u64) -> bool {
        self.slot(territory, kind).is_some_and(|project| {
            matches!(project.state, PROJECT_ACTIVE | PROJECT_BLOCKING) && project.ends_ms > now_ms
        })
    }

    /// The claims' entries of `territory` (module doc).
    pub(crate) fn entries(&self, territory: u16) -> Vec<ProgressionEntry> {
        ENTRY_ORDER
            .iter()
            .map(|kind| match (*kind, self.slot(territory, *kind)) {
                (ALWAYS_AVAILABLE, _) => ProgressionEntry {
                    project: lowercase_crc32(ALWAYS_AVAILABLE_PROJECT),
                    kind: ALWAYS_AVAILABLE,
                    state: u32::from(PROJECT_COMPLETED),
                },
                (kind, Some(project)) => ProgressionEntry {
                    project: project.project,
                    kind,
                    state: u32::from(project.state),
                },
                (kind, None) => ProgressionEntry {
                    project: 0,
                    kind,
                    state: u32::from(PROJECT_EXPIRED),
                },
            })
            .collect()
    }

    /// The planning board's progression data of `territory`, whose upgrades have the tiers
    /// `tier` gives, with the projects `projects` (module doc).
    pub(crate) fn progression_data(
        &self,
        territory: u16,
        projects: &TerritoryProjects,
        tier: impl Fn(u8) -> u8,
    ) -> TerritoryProgressionData {
        let slots = SLOT_ORDER
            .iter()
            .map(|kind| {
                let entry = match (*kind, self.slot(territory, *kind)) {
                    (ALWAYS_AVAILABLE, _) => ProjectEntry {
                        project: lowercase_crc32(ALWAYS_AVAILABLE_PROJECT),
                        ..ProjectEntry::default()
                    },
                    (_, Some(project)) if project.state == PROJECT_ACTIVE => ProjectEntry {
                        project: project.project,
                        time_ns: project.ends_ms.saturating_mul(NS_PER_MS),
                        value: project.points,
                    },
                    _ => ProjectEntry::default(),
                };
                ProjectSlot { slot: *kind, entry }
            })
            .collect();
        let completed = UPGRADE_IDS
            .iter()
            .flat_map(|upgrade| projects.completed(*upgrade, tier(*upgrade)))
            .map(|project| ProjectTime {
                project: project.crc,
                time_ns: 0,
            })
            .collect();
        TerritoryProgressionData {
            slots,
            projects: completed,
            entry: ProjectEntry::default(),
            times: Vec::new(),
        }
    }
}

/// A start as the player asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartAsk {
    /// The player.
    character: Uuid,
    /// Its company.
    company: Option<[u8; RAW_ID_LEN]>,
    /// The territory of its open planning board.
    board: Option<u16>,
    /// The CRC-32 of the project's lowercase `ProjectId`.
    project: u32,
    /// Pay from the own wallet.
    own_wallet: bool,
}

/// The start `ask` under the projects `projects`, the slots `slots`, the territory's owner
/// company `owner` and the upgrade tiers `tier`, at Unix ms `now_ms`: the service's input, or why
/// it is refused.
fn checked_start(
    ask: StartAsk,
    owner: Option<[u8; RAW_ID_LEN]>,
    projects: &TerritoryProjects,
    slots: &ProjectSlots,
    tier: impl Fn(u8) -> u8,
    now_ms: u64,
) -> Result<TownProjectStart, Refusal> {
    let StartAsk {
        character,
        company,
        board,
        project,
        own_wallet,
    } = ask;
    let territory = board.ok_or(Refusal::NoBoard)?;
    let project = projects.project(project).ok_or(Refusal::UnknownProject)?;
    let upgrade = project
        .upgrade
        .filter(|_| matches!(project.kind, FORTRESS | SETTLEMENT))
        .ok_or(Refusal::NoUpgrade)?;
    if owner.is_none() || owner != company {
        return Err(Refusal::NotOwner);
    }
    if slots.busy(territory, project.kind, now_ms) {
        return Err(Refusal::SlotBusy);
    }
    if u16::from(tier(upgrade)) + 1 != u16::from(project.level) {
        return Err(Refusal::WrongTier);
    }
    Ok(TownProjectStart {
        character,
        territory,
        kind: project.kind,
        project: project.crc,
        upgrade,
        level: project.level,
        needed: project.needed,
        cost: project.cost,
        from_treasury: !own_wallet,
        at_ms: now_ms,
        ends_ms: now_ms.saturating_add(project.run_ms),
    })
}

/// Whether the cancel of `project` is taken for a player with the planning board of `board`
/// open, under `projects` and `slots` at Unix ms `now_ms`.
fn checked_cancel(
    project: u32,
    board: Option<u16>,
    projects: &TerritoryProjects,
    slots: &ProjectSlots,
    now_ms: u64,
) -> Result<u16, Refusal> {
    let territory = board.ok_or(Refusal::NoBoard)?;
    let kind = projects
        .project(project)
        .ok_or(Refusal::UnknownProject)?
        .kind;
    slots
        .slot(territory, kind)
        .filter(|running| {
            running.project == project
                && running.state == PROJECT_ACTIVE
                && running.ends_ms > now_ms
        })
        .map(|_| territory)
        .ok_or(Refusal::NotRunning)
}

impl SimWorld {
    /// The town project call `call` of the player of `connection` at Unix ms `now_ms`: the social
    /// service's input that does it, or `None` when this sim refuses it: the player's board then
    /// shows its data again (module doc).
    pub fn town_project_call(
        &mut self,
        connection: &ConnectionRef,
        call: &TownProjectCallMsg,
        now_ms: u64,
    ) -> Option<SocialInput> {
        let entity = *self.by_connection.get(connection)?;
        let player = self.world.get::<Player>(entity)?;
        let board = player.desk.planning_territory();
        let slots = self.territories.dominion().projects();
        let dominion = self.territories.dominion();
        let projects = &self.content.territory_projects;
        let checked = match *call {
            TownProjectCallMsg::Start {
                project,
                own_wallet,
                ..
            } => {
                let owner = board
                    .and_then(|territory| self.territories.owner(territory))
                    .map(|owner| owner.guild_id);
                let ask = StartAsk {
                    character: player.character,
                    company: player.social.company(),
                    board,
                    project,
                    own_wallet,
                };
                checked_start(
                    ask,
                    owner,
                    projects,
                    slots,
                    |upgrade| board.map_or(0, |territory| dominion.tier(territory, upgrade)),
                    now_ms,
                )
                .map(SocialInput::StartTownProject)
            }
            TownProjectCallMsg::Cancel { project, .. } => {
                checked_cancel(project, board, projects, slots, now_ms).map(|territory| {
                    SocialInput::CancelTownProject(TownProjectCancel {
                        character: player.character,
                        territory,
                        project,
                        at_ms: now_ms,
                    })
                })
            }
        };
        match checked {
            Ok(input) => Some(input),
            Err(refusal) => {
                metrics::counter!(METRIC_REFUSED, REASON_LABEL => refusal.label()).increment(1);
                tracing::info!(
                    player = player.number,
                    ?board,
                    ?call,
                    ?refusal,
                    "town project call refused"
                );
                self.world.get_mut::<Player>(entity)?.desk.show_again();
                None
            }
        }
    }

    /// The social service answered the town project calls `answers`: after a done one the
    /// territories are read at once (the boards and claims then show it); a refused one shows
    /// the player's board, when the player is here, its data again.
    pub(super) fn project_answered(&mut self, answers: &[TownProjectAnswer]) {
        for answer in answers {
            tracing::info!(character = %answer.character, territory = answer.territory,
                done = answer.done, "town project call answered");
            if answer.done {
                self.territories.read_soon();
                continue;
            }
            if let Some(entity) = self.player_of_character(answer.character)
                && let Some(mut player) = self.world.get_mut::<Player>(entity)
            {
                player.desk.show_again();
            }
        }
    }

    /// Player `number` finished the mission `mission`: a town board mission's points go to the
    /// running project of its type and category in the mission's territory (module doc).
    pub(super) fn gain_project_points(&mut self, number: u64, mission: &str) {
        let Some(goal) = self
            .content
            .territory_projects
            .goal(lowercase_crc32(mission))
        else {
            return;
        };
        let Some(upgrade) = goal.upgrade else {
            tracing::debug!(player = number, mission, "points of a project not run here");
            return;
        };
        let Some(territory) = self
            .content
            .quests
            .mission(mission)
            .map(|row| row.territory)
            .filter(|territory| *territory != 0)
        else {
            tracing::debug!(player = number, mission, "town mission without a territory");
            return;
        };
        let at_ms = self.unix_ms();
        self.territories.add_points(ProjectPoints {
            territory,
            kind: goal.kind,
            upgrade,
            points: goal.amount,
            at_ms,
        });
    }
}

#[cfg(test)]
mod tests {
    use nw_content::territory_projects::TerritoryProject;
    use nw_db::PROJECT_CANCELLED;

    use super::*;

    const TERRITORY: u16 = 6;
    const COMPANY: [u8; RAW_ID_LEN] = [7; RAW_ID_LEN];
    const COOKING: u8 = 3;
    const NOW_MS: u64 = 1_800_000_000_000;
    const DAY_MS: u64 = 86_400_000;
    const NEEDED: u32 = 1500;

    fn project(id: &str, kind: u8, upgrade: Option<u8>, level: u8) -> TerritoryProject {
        TerritoryProject {
            id: id.to_owned(),
            crc: lowercase_crc32(id),
            kind,
            upgrade,
            level,
            cost: 500_000,
            needed: NEEDED,
            run_ms: DAY_MS,
        }
    }

    fn projects() -> TerritoryProjects {
        TerritoryProjects::build(
            [
                project("CookingProjectId", SETTLEMENT, Some(COOKING), 1),
                project("CookingProjectId2", SETTLEMENT, Some(COOKING), 2),
                project("HornUpgradeProjectId", FORTRESS, Some(16), 1),
                project("MinersResolveProjectId", LIFESTYLE, None, 1),
            ],
            [],
        )
    }

    fn running(id: &str, kind: u8, state: u8, ends_ms: u64) -> TownProject {
        TownProject {
            territory: TERRITORY,
            kind,
            project: lowercase_crc32(id),
            upgrade: COOKING,
            level: 2,
            state,
            points: 40,
            needed: NEEDED,
            company: Uuid::nil(),
            started_ms: ends_ms - DAY_MS,
            ends_ms,
        }
    }

    fn start(
        id: &str,
        board: Option<u16>,
        slots: &ProjectSlots,
        cooking_tier: u8,
    ) -> Result<TownProjectStart, Refusal> {
        let ask = StartAsk {
            character: Uuid::from_u128(1),
            company: Some(COMPANY),
            board,
            project: lowercase_crc32(id),
            own_wallet: true,
        };
        checked_start(
            ask,
            Some(COMPANY),
            &projects(),
            slots,
            |upgrade| if upgrade == COOKING { cooking_tier } else { 0 },
            NOW_MS,
        )
    }

    #[test]
    fn a_start_needs_the_board_an_upgrade_project_the_owner_a_free_slot_and_the_tier() {
        let empty = ProjectSlots::default();
        let started = start("CookingProjectId2", Some(TERRITORY), &empty, 1).expect("start");
        assert_eq!(
            (
                started.kind,
                started.upgrade,
                started.level,
                started.from_treasury
            ),
            (SETTLEMENT, COOKING, 2, false),
            "the flag is the own wallet"
        );
        assert_eq!(started.ends_ms, NOW_MS + DAY_MS);
        assert_eq!(
            start("CookingProjectId2", None, &empty, 1),
            Err(Refusal::NoBoard)
        );
        assert_eq!(
            start("NoSuchProjectId", Some(TERRITORY), &empty, 1),
            Err(Refusal::UnknownProject)
        );
        assert_eq!(
            start("MinersResolveProjectId", Some(TERRITORY), &empty, 1),
            Err(Refusal::NoUpgrade)
        );
        assert_eq!(
            start("CookingProjectId2", Some(TERRITORY), &empty, 0),
            Err(Refusal::WrongTier)
        );
        let busy = ProjectSlots::of(vec![running(
            "CookingProjectId",
            SETTLEMENT,
            PROJECT_BLOCKING,
            NOW_MS + 1,
        )]);
        assert_eq!(
            start("CookingProjectId2", Some(TERRITORY), &busy, 1),
            Err(Refusal::SlotBusy)
        );
        let ended = ProjectSlots::of(vec![running(
            "CookingProjectId",
            SETTLEMENT,
            PROJECT_BLOCKING,
            NOW_MS,
        )]);
        assert!(
            start("CookingProjectId2", Some(TERRITORY), &ended, 1).is_ok(),
            "its end freed it"
        );
        assert!(
            start("HornUpgradeProjectId", Some(TERRITORY), &busy, 1).is_ok(),
            "another type"
        );
        let other = StartAsk {
            character: Uuid::from_u128(2),
            company: Some([8; RAW_ID_LEN]),
            board: Some(TERRITORY),
            project: lowercase_crc32("CookingProjectId2"),
            own_wallet: false,
        };
        assert_eq!(
            checked_start(other, Some(COMPANY), &projects(), &empty, |_| 1, NOW_MS),
            Err(Refusal::NotOwner)
        );
        assert_eq!(
            checked_start(other, None, &projects(), &empty, |_| 1, NOW_MS),
            Err(Refusal::NotOwner),
            "no owner"
        );
    }

    #[test]
    fn a_cancel_names_the_running_project_of_the_open_board() {
        let id = "CookingProjectId2";
        let slots = ProjectSlots::of(vec![running(id, SETTLEMENT, PROJECT_ACTIVE, NOW_MS + 1)]);
        let crc = lowercase_crc32(id);
        assert_eq!(
            checked_cancel(crc, Some(TERRITORY), &projects(), &slots, NOW_MS),
            Ok(TERRITORY)
        );
        assert_eq!(
            checked_cancel(crc, None, &projects(), &slots, NOW_MS),
            Err(Refusal::NoBoard)
        );
        assert_eq!(
            checked_cancel(
                lowercase_crc32("CookingProjectId"),
                Some(TERRITORY),
                &projects(),
                &slots,
                NOW_MS
            ),
            Err(Refusal::NotRunning)
        );
        let cancelled =
            ProjectSlots::of(vec![running(id, SETTLEMENT, PROJECT_CANCELLED, NOW_MS + 1)]);
        assert_eq!(
            checked_cancel(crc, Some(TERRITORY), &projects(), &cancelled, NOW_MS),
            Err(Refusal::NotRunning)
        );
    }

    #[test]
    fn the_claims_and_the_board_show_the_running_project_and_the_completed_ones() {
        let id = "CookingProjectId2";
        let slots = ProjectSlots::of(vec![running(id, SETTLEMENT, PROJECT_ACTIVE, NOW_MS)]);
        let entries = slots.entries(TERRITORY);
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.project, entry.kind, entry.state))
                .collect::<Vec<_>>(),
            [
                (
                    lowercase_crc32(ALWAYS_AVAILABLE_PROJECT),
                    ALWAYS_AVAILABLE,
                    1
                ),
                (lowercase_crc32(id), SETTLEMENT, 0),
                (0, FORTRESS, 2),
                (0, LIFESTYLE, 2),
                (0, EVENT, 2),
            ]
        );
        let data = slots.progression_data(TERRITORY, &projects(), |upgrade| {
            if upgrade == COOKING { 1 } else { 0 }
        });
        assert_eq!(
            data.slots.iter().map(|slot| slot.slot).collect::<Vec<_>>(),
            SLOT_ORDER
        );
        let settlement = data.slots[3].entry;
        assert_eq!(
            (settlement.project, settlement.time_ns, settlement.value),
            (lowercase_crc32(id), NOW_MS * NS_PER_MS, 40)
        );
        assert_eq!(
            data.slots[2].entry.project,
            lowercase_crc32(ALWAYS_AVAILABLE_PROJECT)
        );
        assert_eq!(
            data.slots[4].entry,
            ProjectEntry::default(),
            "no fortress project"
        );
        assert_eq!(
            data.projects,
            [ProjectTime {
                project: lowercase_crc32("CookingProjectId"),
                time_ns: 0
            }]
        );
        assert!(data.times.is_empty(), "no lifestyle buff runs");
    }
}
