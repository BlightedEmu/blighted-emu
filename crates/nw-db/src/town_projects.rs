// SPDX-License-Identifier: AGPL-3.0-or-later
//! The town projects of the territories (world migration 0045): one project per territory and
//! progression type, the points the town board missions add, and the upgrade tier a completed
//! project gives.
//!
//! Observed (retail claims 3086, wars and sieges findings "Town projects"): a completed project
//! is Blocking ([`PROJECT_BLOCKING`]) in the same claims state that shows its category's tier at
//! the project's level; later its slot is empty. Inferred: a project runs from its start to
//! its end time; at its end an Active project without its points is Expired ([`PROJECT_EXPIRED`])
//! and a Blocking one frees the slot ([`advance_town_projects`]); an Expired or Cancelled project
//! keeps its slot until the next start replaces it.

use sqlx::PgPool;
use uuid::Uuid;

use crate::conflicts::treasury_change;
use crate::error::DbError;
use crate::progression::{from_column, to_column};
use crate::social_graph::{TreasuryReason, narrow};

const TERRITORY_FIELD: &str = "town project territory id";
const KIND_FIELD: &str = "town project progression type";
const PROJECT_FIELD: &str = "town project id";
const NUMBER_FIELD: &str = "town project number";
const TIME_FIELD: &str = "town project time";

/// `SettlementProgressionState` Active: the missions add points (client enum `FUN_142f94920`).
pub const PROJECT_ACTIVE: u8 = 0;
/// `SettlementProgressionState` Completed (the always available project's state in every
/// retail claim).
pub const PROJECT_COMPLETED: u8 = 1;
/// `SettlementProgressionState` Expired: the project ended without its points, or the slot is
/// empty (Observed: `(0, type, 2)` for an empty slot).
pub const PROJECT_EXPIRED: u8 = 2;
/// `SettlementProgressionState` Cancelled.
pub const PROJECT_CANCELLED: u8 = 3;
/// `SettlementProgressionState` Blocking: the project completed and holds its slot to its end.
pub const PROJECT_BLOCKING: u8 = 4;

/// A town project as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TownProject {
    /// The territory id.
    pub territory: u16,
    /// Its `TerritoryProgressionType`.
    pub kind: u8,
    /// The CRC-32 of its lowercase `ProjectId`.
    pub project: u32,
    /// The upgrade id it raises and the tier it gives.
    pub upgrade: u8,
    /// The tier it gives.
    pub level: u8,
    /// Its `SettlementProgressionState`.
    pub state: u8,
    /// The points the missions added.
    pub points: u32,
    /// The points it needs.
    pub needed: u32,
    /// The company that started it.
    pub company: Uuid,
    /// When it started and when it ends, Unix ms.
    pub started_ms: u64,
    /// When it ends, Unix ms.
    pub ends_ms: u64,
}

/// A start a company's member asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectStart {
    /// The id of this start: the same start tried again (its answer was lost) finds its own
    /// project and is not refused, so a wallet payment is never given back for a running project.
    pub id: Uuid,
    /// The territory id.
    pub territory: u16,
    /// The project's `TerritoryProgressionType`.
    pub kind: u8,
    /// The CRC-32 of its lowercase `ProjectId`.
    pub project: u32,
    /// The upgrade id it raises.
    pub upgrade: u8,
    /// The tier it gives (the upgrade's tier must be one below).
    pub level: u8,
    /// The points it needs.
    pub needed: u32,
    /// Its cost, hundredths of a coin.
    pub cost: u64,
    /// The company that must own the territory.
    pub company: Uuid,
    /// The member that starts it.
    pub character: Uuid,
    /// The company treasury pays (else the member's wallet paid already).
    pub from_treasury: bool,
    /// When it starts and when it ends, Unix ms.
    pub at_ms: u64,
    /// When it ends, Unix ms.
    pub ends_ms: u64,
}

/// Why the database refused a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRefusal {
    /// The company does not own the territory.
    NotOwner,
    /// The type's slot runs an Active or Blocking project.
    SlotBusy,
    /// The upgrade's tier is not one below the project's level.
    WrongTier,
    /// The treasury has less than the cost.
    TooFewCoins,
}

/// What the database did with a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStarted {
    /// The project runs; the treasury's new balance when the treasury paid.
    Started {
        /// The treasury after the payment.
        treasury: Option<u64>,
    },
    /// It was refused; the treasury paid nothing.
    Refused(ProjectRefusal),
}

/// What a mission's points did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectProgress {
    /// No Active project of that type and upgrade runs in the territory.
    NoProject,
    /// The points were added.
    Added,
    /// The points completed the project: its upgrade got its level.
    Completed,
}

type ProjectColumns = (i16, i16, i64, i16, i16, i16, i32, i32, Uuid, i64, i64);

fn project_of(columns: ProjectColumns) -> Result<TownProject, DbError> {
    let (territory, kind, project, upgrade, level, state, points, needed, company, started, ends) =
        columns;
    Ok(TownProject {
        territory: narrow(i64::from(territory), TERRITORY_FIELD)?,
        kind: narrow(i64::from(kind), KIND_FIELD)?,
        project: narrow(project, PROJECT_FIELD)?,
        upgrade: narrow(i64::from(upgrade), NUMBER_FIELD)?,
        level: narrow(i64::from(level), NUMBER_FIELD)?,
        state: narrow(i64::from(state), NUMBER_FIELD)?,
        points: narrow(i64::from(points), NUMBER_FIELD)?,
        needed: narrow(i64::from(needed), NUMBER_FIELD)?,
        company,
        started_ms: from_column(started, TIME_FIELD)?,
        ends_ms: from_column(ends, TIME_FIELD)?,
    })
}

/// Every town project, by territory and type.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails and [`DbError::OutOfRange`] for a stored
/// value the program's type cannot hold.
pub async fn town_projects(world: &PgPool) -> Result<Vec<TownProject>, DbError> {
    let rows: Vec<ProjectColumns> = sqlx::query_as(
        "SELECT territory_id, progression_type, project_crc, upgrade_id, level, state, points, \
         needed, company_id, started_ms, ends_ms FROM territory_project \
         ORDER BY territory_id, progression_type",
    )
    .fetch_all(world)
    .await?;
    rows.into_iter().map(project_of).collect()
}

/// Starts `start` in one transaction: the company owns the territory, the type's slot is free
/// (no Active or Blocking project before its end), the upgrade's tier is one below the level,
/// and the treasury pays when it pays (reason `territory progression`). The new project replaces
/// an Expired or Cancelled one of its slot. A start whose id the slot holds already is the same
/// start tried again: it is started, and nothing is paid again.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails and [`DbError::OutOfRange`] for a value the
/// columns cannot hold.
pub async fn start_town_project(
    world: &PgPool,
    start: &ProjectStart,
) -> Result<ProjectStarted, DbError> {
    let territory: i16 = narrow(i64::from(start.territory), TERRITORY_FIELD)?;
    let at_ms = to_column(start.at_ms, TIME_FIELD)?;
    let mut transaction = world.begin().await?;
    let owner: Option<Uuid> = sqlx::query_scalar(
        "SELECT company_id FROM territory_owner WHERE territory_id = $1 FOR UPDATE",
    )
    .bind(territory)
    .fetch_optional(&mut *transaction)
    .await?;
    if owner != Some(start.company) {
        return Ok(ProjectStarted::Refused(ProjectRefusal::NotOwner));
    }
    let slot: Option<(Uuid, bool)> = sqlx::query_as(
        "SELECT start_id, state IN ($3, $4) AND ends_ms > $5 FROM territory_project \
         WHERE territory_id = $1 AND progression_type = $2 FOR UPDATE",
    )
    .bind(territory)
    .bind(i16::from(start.kind))
    .bind(i16::from(PROJECT_ACTIVE))
    .bind(i16::from(PROJECT_BLOCKING))
    .bind(at_ms)
    .fetch_optional(&mut *transaction)
    .await?;
    match slot {
        Some((id, _)) if id == start.id => {
            let treasury = if start.from_treasury {
                let balance: i64 =
                    sqlx::query_scalar("SELECT treasury FROM company WHERE company_id = $1")
                        .bind(start.company)
                        .fetch_one(&mut *transaction)
                        .await?;
                Some(from_column(balance, NUMBER_FIELD)?)
            } else {
                None
            };
            return Ok(ProjectStarted::Started { treasury });
        }
        Some((_, true)) => return Ok(ProjectStarted::Refused(ProjectRefusal::SlotBusy)),
        _ => {}
    }
    let tier: Option<i16> = sqlx::query_scalar(
        "SELECT tier FROM territory_upgrade WHERE territory_id = $1 AND upgrade_id = $2",
    )
    .bind(territory)
    .bind(i16::from(start.upgrade))
    .fetch_optional(&mut *transaction)
    .await?;
    if i64::from(tier.unwrap_or(0)) + 1 != i64::from(start.level) {
        return Ok(ProjectStarted::Refused(ProjectRefusal::WrongTier));
    }
    let treasury = if start.from_treasury {
        let change = -to_column(start.cost, NUMBER_FIELD)?;
        let paid = treasury_change(
            &mut transaction,
            start.company,
            start.character,
            change,
            TreasuryReason::TerritoryProgression,
        )
        .await?;
        let Some(balance) = paid else {
            return Ok(ProjectStarted::Refused(ProjectRefusal::TooFewCoins));
        };
        Some(balance)
    } else {
        None
    };
    sqlx::query(
        "INSERT INTO territory_project (territory_id, progression_type, project_crc, upgrade_id, \
         level, state, points, needed, company_id, character_id, started_ms, ends_ms, start_id) \
         VALUES ($1, $2, $3, $4, $5, $6, 0, $7, $8, $9, $10, $11, $12) \
         ON CONFLICT (territory_id, progression_type) DO UPDATE SET \
         project_crc = excluded.project_crc, upgrade_id = excluded.upgrade_id, \
         level = excluded.level, state = excluded.state, points = 0, needed = excluded.needed, \
         company_id = excluded.company_id, character_id = excluded.character_id, \
         started_ms = excluded.started_ms, ends_ms = excluded.ends_ms, \
         start_id = excluded.start_id",
    )
    .bind(territory)
    .bind(i16::from(start.kind))
    .bind(i64::from(start.project))
    .bind(i16::from(start.upgrade))
    .bind(i16::from(start.level))
    .bind(i16::from(PROJECT_ACTIVE))
    .bind(narrow::<i32>(i64::from(start.needed), NUMBER_FIELD)?)
    .bind(start.company)
    .bind(start.character)
    .bind(at_ms)
    .bind(to_column(start.ends_ms, TIME_FIELD)?)
    .bind(start.id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(ProjectStarted::Started { treasury })
}

/// Cancels the Active project `project` of `territory` for `company`, the territory's owner, at
/// Unix ms `at_ms`; false when no such project runs or the company does not own the territory.
/// Inferred: a cancelled project gives no coin back (no client text names a refund).
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails.
pub async fn cancel_town_project(
    world: &PgPool,
    territory: u16,
    project: u32,
    company: Uuid,
    at_ms: u64,
) -> Result<bool, DbError> {
    let cancelled = sqlx::query(
        "UPDATE territory_project p SET state = $4 \
         FROM territory_owner o \
         WHERE p.territory_id = $1 AND p.project_crc = $2 AND p.state = $5 AND p.ends_ms > $6 \
         AND o.territory_id = p.territory_id AND o.company_id = $3",
    )
    .bind(narrow::<i16>(i64::from(territory), TERRITORY_FIELD)?)
    .bind(i64::from(project))
    .bind(company)
    .bind(i16::from(PROJECT_CANCELLED))
    .bind(i16::from(PROJECT_ACTIVE))
    .bind(to_column(at_ms, TIME_FIELD)?)
    .execute(world)
    .await?;
    Ok(cancelled.rows_affected() > 0)
}

/// Adds a mission's `points` at Unix ms `at_ms` to the Active project of type `kind` that raises
/// `upgrade` in `territory`; at its needed points it is Blocking and its upgrade gets its level,
/// in one transaction.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails.
pub async fn add_project_points(
    world: &PgPool,
    territory: u16,
    kind: u8,
    upgrade: u8,
    points: u32,
    at_ms: u64,
) -> Result<ProjectProgress, DbError> {
    let territory: i16 = narrow(i64::from(territory), TERRITORY_FIELD)?;
    let mut transaction = world.begin().await?;
    let added: Option<(i32, i32, i16)> = sqlx::query_as(
        "UPDATE territory_project SET points = points + $4 \
         WHERE territory_id = $1 AND progression_type = $2 AND upgrade_id = $3 \
         AND state = $5 AND ends_ms > $6 RETURNING points, needed, level",
    )
    .bind(territory)
    .bind(i16::from(kind))
    .bind(i16::from(upgrade))
    .bind(narrow::<i32>(i64::from(points), NUMBER_FIELD)?)
    .bind(i16::from(PROJECT_ACTIVE))
    .bind(to_column(at_ms, TIME_FIELD)?)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some((total, needed, level)) = added else {
        return Ok(ProjectProgress::NoProject);
    };
    if total < needed {
        transaction.commit().await?;
        return Ok(ProjectProgress::Added);
    }
    sqlx::query(
        "UPDATE territory_project SET state = $3 \
         WHERE territory_id = $1 AND progression_type = $2",
    )
    .bind(territory)
    .bind(i16::from(kind))
    .bind(i16::from(PROJECT_BLOCKING))
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO territory_upgrade (territory_id, upgrade_id, tier) VALUES ($1, $2, $3) \
         ON CONFLICT (territory_id, upgrade_id) DO UPDATE \
         SET tier = GREATEST(territory_upgrade.tier, excluded.tier)",
    )
    .bind(territory)
    .bind(i16::from(upgrade))
    .bind(level)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(ProjectProgress::Completed)
}

/// Ends the projects whose end is at or before Unix ms `now_ms`: an Active one is Expired, a
/// Blocking one frees its slot. The count of projects changed.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails.
pub async fn advance_town_projects(world: &PgPool, now_ms: u64) -> Result<u64, DbError> {
    let now = to_column(now_ms, TIME_FIELD)?;
    let mut transaction = world.begin().await?;
    let expired =
        sqlx::query("UPDATE territory_project SET state = $1 WHERE state = $2 AND ends_ms <= $3")
            .bind(i16::from(PROJECT_EXPIRED))
            .bind(i16::from(PROJECT_ACTIVE))
            .bind(now)
            .execute(&mut *transaction)
            .await?;
    let freed = sqlx::query("DELETE FROM territory_project WHERE state = $1 AND ends_ms <= $2")
        .bind(i16::from(PROJECT_BLOCKING))
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(expired.rows_affected() + freed.rows_affected())
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::territory::infra_tests::company;
    use crate::test_db::with_test_schema;
    use crate::{change_treasury, set_territory_owner, territory_upgrades};

    /// Made-up names and numbers; `CookingProjectId2`'s level, points and cost (build 25810808).
    const TERRITORY: u16 = 6;
    const OWNER: &str = "Made Up Governors";
    const OTHER: &str = "Made Up Rivals";
    const COOKING: u8 = 3;
    const PROJECT: u32 = 0x00c0_ffee;
    const LEVEL: u8 = 2;
    const NEEDED: u32 = 1500;
    const COST: u64 = 500_000;
    const TREASURY: i64 = 600_000;
    const START_MS: u64 = 1_900_000_000_000;
    const DAY_MS: u64 = 86_400_000;
    const SETTLEMENT: u8 = 2;

    fn start(company: Uuid, character: Uuid, from_treasury: bool) -> ProjectStart {
        ProjectStart {
            id: Uuid::new_v4(),
            territory: TERRITORY,
            kind: SETTLEMENT,
            project: PROJECT,
            upgrade: COOKING,
            level: LEVEL,
            needed: NEEDED,
            cost: COST,
            company,
            character,
            from_treasury,
            at_ms: START_MS,
            ends_ms: START_MS + DAY_MS,
        }
    }

    async fn cooking_tier(pool: &PgPool) -> u8 {
        territory_upgrades(pool)
            .await
            .expect("upgrades")
            .iter()
            .find(|row| row.territory == TERRITORY && row.upgrade == COOKING)
            .map_or(0, |row| row.tier)
    }

    #[tokio::test]
    async fn a_project_starts_once_takes_points_and_raises_its_upgrade() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let owner = company(pool, OWNER, 1).await;
            let other = company(pool, OTHER, 2).await;
            let member = Uuid::new_v4();
            change_treasury(pool, owner, member, TREASURY, TreasuryReason::Deposit)
                .await
                .expect("deposit");
            set_territory_owner(pool, TERRITORY, OWNER)
                .await
                .expect("owner");
            let refused = ProjectStarted::Refused;
            assert_eq!(
                start_town_project(pool, &start(other, member, false))
                    .await
                    .expect("start"),
                refused(ProjectRefusal::NotOwner)
            );
            assert_eq!(
                start_town_project(pool, &start(owner, member, true))
                    .await
                    .expect("start"),
                refused(ProjectRefusal::WrongTier),
                "level 2 needs tier 1"
            );
            crate::set_territory_upgrade(
                pool,
                &crate::TerritoryUpgrade {
                    territory: TERRITORY,
                    upgrade: COOKING,
                    tier: LEVEL - 1,
                },
            )
            .await
            .expect("tier");
            let balance = u64::try_from(TREASURY).expect("treasury") - COST;
            let paid = start(owner, member, true);
            for _ in 0..2 {
                assert_eq!(
                    start_town_project(pool, &paid).await.expect("start"),
                    ProjectStarted::Started {
                        treasury: Some(balance)
                    },
                    "a retried start is started once and paid once"
                );
            }
            assert_eq!(
                start_town_project(pool, &start(owner, member, false))
                    .await
                    .expect("start"),
                refused(ProjectRefusal::SlotBusy)
            );
            let at = START_MS + 1;
            let add = async |points| {
                add_project_points(pool, TERRITORY, SETTLEMENT, COOKING, points, at)
                    .await
                    .expect("points")
            };
            assert_eq!(
                add_project_points(pool, TERRITORY, SETTLEMENT, COOKING + 1, 10, at)
                    .await
                    .expect("points"),
                ProjectProgress::NoProject,
                "another category"
            );
            assert_eq!(add(NEEDED - 1).await, ProjectProgress::Added);
            assert_eq!(cooking_tier(pool).await, LEVEL - 1);
            assert_eq!(add(1).await, ProjectProgress::Completed);
            assert_eq!(cooking_tier(pool).await, LEVEL);
            assert_eq!(add(1).await, ProjectProgress::NoProject, "Blocking");
            let stored = town_projects(pool).await.expect("projects");
            assert_eq!(
                stored
                    .iter()
                    .map(|p| (p.state, p.points))
                    .collect::<Vec<_>>(),
                [(PROJECT_BLOCKING, NEEDED)]
            );
            assert_eq!(
                advance_town_projects(pool, START_MS + DAY_MS)
                    .await
                    .expect("advance"),
                1
            );
            assert!(
                town_projects(pool).await.expect("projects").is_empty(),
                "slot free"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn a_project_expires_at_its_end_or_is_cancelled_by_the_owner() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let owner = company(pool, OWNER, 1).await;
            let other = company(pool, OTHER, 2).await;
            let member = Uuid::new_v4();
            set_territory_owner(pool, TERRITORY, OWNER)
                .await
                .expect("owner");
            crate::set_territory_upgrade(
                pool,
                &crate::TerritoryUpgrade {
                    territory: TERRITORY,
                    upgrade: COOKING,
                    tier: LEVEL - 1,
                },
            )
            .await
            .expect("tier");
            assert_eq!(
                start_town_project(pool, &start(owner, member, true))
                    .await
                    .expect("start"),
                ProjectStarted::Refused(ProjectRefusal::TooFewCoins)
            );
            assert!(town_projects(pool).await.expect("projects").is_empty());
            let wallet = start(owner, member, false);
            assert_eq!(
                start_town_project(pool, &wallet).await.expect("start"),
                ProjectStarted::Started { treasury: None }
            );
            assert!(
                !cancel_town_project(pool, TERRITORY, PROJECT, other, START_MS)
                    .await
                    .expect("cancel"),
                "not the owner"
            );
            assert!(
                cancel_town_project(pool, TERRITORY, PROJECT, owner, START_MS)
                    .await
                    .expect("cancel")
            );
            assert_eq!(
                town_projects(pool).await.expect("projects")[0].state,
                PROJECT_CANCELLED
            );
            assert_eq!(
                start_town_project(pool, &start(owner, member, false))
                    .await
                    .expect("start"),
                ProjectStarted::Started { treasury: None },
                "a cancelled project frees its slot for a new start"
            );
            assert_eq!(
                advance_town_projects(pool, START_MS + DAY_MS)
                    .await
                    .expect("advance"),
                1
            );
            assert_eq!(
                town_projects(pool).await.expect("projects")[0].state,
                PROJECT_EXPIRED
            );
            assert_eq!(
                cooking_tier(pool).await,
                LEVEL - 1,
                "no tier for an expired one"
            );
        })
        .await;
    }
}
