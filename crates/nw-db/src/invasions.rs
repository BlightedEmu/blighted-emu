// SPDX-License-Identifier: AGPL-3.0-or-later
//! The invasions of the territories (world migration 0043): the darkness of each territory, the
//! invasions it brings, the roster fill of an invasion and the territory upgrades a lost
//! invasion lowers.
//!
//! An invasion is a [`crate::TerritoryWar`] row without an attacking company (Observed: every
//! retail invasion of the war data has a zero attacker company and faction 0, `invasion`,
//! `Reekwater_Invasion`, `Playthrough_Part_21`). Its sign-ups and roster are the war's
//! ([`crate::sign_up_for_war`], [`crate::change_roster`]); a won invasion is resolved for its
//! defender with [`crate::resolve_war`], a lost one with [`lose_invasion`].
//!
//! **Upgrades** (Observed, the claims 3086 of the same ledgers, read with
//! `LandClaimManagerState::decode`): each claim carries the tiers of 19 upgrades (ids 0 to 9,
//! 11 to 17, 20 and 21; tiers 0 to 3) and a level that is the sum of the tiers. At the loss of
//! `Playthrough_Part_21` (territory 2, 3322.4 s) six upgrades lost one tier (level 33 to 27), at
//! the loss of `Reekwater_Invasion` (territory 5, 956.5 s) four (level 27 to 23); no tier rose.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};
use crate::social_graph::narrow;

const TERRITORY_FIELD: &str = "invasion territory id";
const LEVEL_FIELD: &str = "darkness level";
const TIME_FIELD: &str = "darkness time";
const UPGRADE_FIELD: &str = "territory upgrade";
const PLACE_FIELD: &str = "invasion roster place";

/// The highest darkness level (Observed: the claims' darkness thresholds are 0 to 3, and every
/// invaded territory showed 3).
pub const DARKNESS_INVASION_LEVEL: u8 = 3;

/// The highest tier of a territory upgrade (Observed: the claims' upgrade tiers are 0 to 3).
pub const UPGRADE_TIER_MAX: u8 = 3;

/// The defender side of a sign-up (the 2762 side; Observed: every invasion sign-up is side 2).
const DEFENDER_SIDE: i16 = 2;

/// The groups of a roster and the places of a group (the `war_signup` place checks of world
/// migration 0040: groups 0 to 9, slots 0 to 4).
const ROSTER_GROUP_COUNT: u8 = 10;
const ROSTER_GROUP_SIZE: u8 = 5;

/// The darkness of a territory, as the claims show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerritoryDarkness {
    /// The territory id.
    pub territory: u16,
    /// Its level, 0 to [`DARKNESS_INVASION_LEVEL`].
    pub level: u8,
    /// When the level changes next, Unix ms.
    pub cycle_end_ms: u64,
}

/// An owned territory with its owner's siege hour and its darkness (none before its first
/// cycle).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DarknessRow {
    /// The territory id.
    pub territory: u16,
    /// The owner company's siege hour (UTC, `company.siege_hour`).
    pub siege_hour: f32,
    /// Its darkness.
    pub darkness: Option<TerritoryDarkness>,
}

/// One change of a territory's darkness, with the invasion it brings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DarknessStep {
    /// The new darkness.
    pub darkness: TerritoryDarkness,
    /// The start of the invasion it schedules, Unix ms.
    pub invasion_starts_ms: Option<u64>,
}

/// What a scheduling did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvasionScheduled {
    /// The invasion is scheduled.
    Scheduled,
    /// No company owns the territory.
    NoOwner,
    /// The territory already has a war or an invasion without a result.
    AtWar,
}

/// The tier of one upgrade of a territory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerritoryUpgrade {
    /// The territory id.
    pub territory: u16,
    /// The upgrade id (the claims' progression tier id).
    pub upgrade: u8,
    /// Its tier, 0 to [`UPGRADE_TIER_MAX`].
    pub tier: u8,
}

type DarknessRowColumns = (i16, f32, Option<i16>, Option<i64>);

fn darkness_of(territory: i16, level: i16, cycle_end: i64) -> Result<TerritoryDarkness, DbError> {
    Ok(TerritoryDarkness {
        territory: narrow(i64::from(territory), TERRITORY_FIELD)?,
        level: narrow(i64::from(level), LEVEL_FIELD)?,
        cycle_end_ms: from_column(cycle_end, TIME_FIELD)?,
    })
}

/// Every owned territory with its owner's siege hour and its darkness, by territory id.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails and [`DbError::OutOfRange`] for a stored
/// value the program's type cannot hold.
pub async fn darkness_rows(world: &PgPool) -> Result<Vec<DarknessRow>, DbError> {
    let rows: Vec<DarknessRowColumns> = sqlx::query_as(
        "SELECT o.territory_id, c.siege_hour, d.level, d.cycle_end_ms \
         FROM territory_owner o JOIN company c ON c.company_id = o.company_id \
         LEFT JOIN territory_darkness d ON d.territory_id = o.territory_id \
         ORDER BY o.territory_id",
    )
    .fetch_all(world)
    .await?;
    rows.into_iter()
        .map(|(territory, siege_hour, level, cycle_end)| {
            let darkness = match (level, cycle_end) {
                (Some(level), Some(cycle_end)) => Some(darkness_of(territory, level, cycle_end)?),
                _ => None,
            };
            Ok(DarknessRow {
                territory: narrow(i64::from(territory), TERRITORY_FIELD)?,
                siege_hour,
                darkness,
            })
        })
        .collect()
}

/// The darkness of every territory that has one, by territory id.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails and [`DbError::OutOfRange`] for a stored
/// value the program's type cannot hold.
pub async fn territory_darkness(world: &PgPool) -> Result<Vec<TerritoryDarkness>, DbError> {
    let rows: Vec<(i16, i16, i64)> = sqlx::query_as(
        "SELECT territory_id, level, cycle_end_ms FROM territory_darkness ORDER BY territory_id",
    )
    .fetch_all(world)
    .await?;
    rows.into_iter()
        .map(|(territory, level, cycle_end)| darkness_of(territory, level, cycle_end))
        .collect()
}

async fn write_darkness(
    connection: &mut sqlx::PgConnection,
    darkness: &TerritoryDarkness,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO territory_darkness (territory_id, level, cycle_end_ms) VALUES ($1, $2, $3) \
         ON CONFLICT (territory_id) DO UPDATE SET level = excluded.level, \
         cycle_end_ms = excluded.cycle_end_ms",
    )
    .bind(narrow::<i16>(
        i64::from(darkness.territory),
        TERRITORY_FIELD,
    )?)
    .bind(i16::from(darkness.level.min(DARKNESS_INVASION_LEVEL)))
    .bind(to_column(darkness.cycle_end_ms, TIME_FIELD)?)
    .execute(connection)
    .await?;
    Ok(())
}

/// Inserts an invasion of `territory` against its owner, starting at Unix ms `starts_ms`.
async fn insert_invasion(
    connection: &mut sqlx::PgConnection,
    territory: u16,
    starts_ms: u64,
) -> Result<InvasionScheduled, DbError> {
    let territory = narrow::<i16>(i64::from(territory), TERRITORY_FIELD)?;
    let owner: Option<Uuid> =
        sqlx::query_scalar("SELECT company_id FROM territory_owner WHERE territory_id = $1")
            .bind(territory)
            .fetch_optional(&mut *connection)
            .await?;
    let Some(owner) = owner else {
        return Ok(InvasionScheduled::NoOwner);
    };
    let inserted = sqlx::query(
        "INSERT INTO territory_war (war_id, territory_id, attacker_company, defender_company, \
         starts_ms) VALUES ($1, $2, NULL, $3, $4) \
         ON CONFLICT (territory_id) WHERE resolved_ms IS NULL DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(territory)
    .bind(owner)
    .bind(to_column(starts_ms, TIME_FIELD)?)
    .execute(&mut *connection)
    .await?;
    Ok(if inserted.rows_affected() > 0 {
        InvasionScheduled::Scheduled
    } else {
        InvasionScheduled::AtWar
    })
}

/// Writes the darkness `steps` in one transaction, each with the invasion it schedules (an
/// invasion of a territory that has a war or lost its owner is not scheduled; its darkness is
/// written all the same).
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails.
pub async fn apply_darkness(world: &PgPool, steps: &[DarknessStep]) -> Result<(), DbError> {
    let mut transaction = world.begin().await?;
    for step in steps {
        write_darkness(&mut transaction, &step.darkness).await?;
        if let Some(starts_ms) = step.invasion_starts_ms {
            insert_invasion(&mut transaction, step.darkness.territory, starts_ms).await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

/// Sets the darkness of a territory (a game master's change).
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails.
pub async fn set_darkness(world: &PgPool, darkness: &TerritoryDarkness) -> Result<(), DbError> {
    let mut connection = world.acquire().await?;
    write_darkness(&mut connection, darkness).await
}

/// Schedules an invasion of `territory` against its owner at Unix ms `starts_ms`.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails.
pub async fn schedule_invasion(
    world: &PgPool,
    territory: u16,
    starts_ms: u64,
) -> Result<InvasionScheduled, DbError> {
    let mut transaction = world.begin().await?;
    let scheduled = insert_invasion(&mut transaction, territory, starts_ms).await?;
    transaction.commit().await?;
    Ok(scheduled)
}

/// The free roster places in fill order: group by group, slot by slot.
fn free_places(taken: &[(u8, u8)]) -> impl Iterator<Item = (u8, u8)> + '_ {
    (0..ROSTER_GROUP_COUNT)
        .flat_map(|group| (0..ROSTER_GROUP_SIZE).map(move |slot| (group, slot)))
        .filter(|place| !taken.contains(place))
}

/// Puts the defender sign-ups of invasion `war` that have no roster place on its free places,
/// in sign-up order; the number placed.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails.
pub async fn fill_invasion_roster(world: &PgPool, war: Uuid) -> Result<u64, DbError> {
    let mut transaction = world.begin().await?;
    let taken: Vec<(i16, i16)> = sqlx::query_as(
        "SELECT roster_group, roster_slot FROM war_signup \
         WHERE war_id = $1 AND side = $2 AND roster_group IS NOT NULL FOR UPDATE",
    )
    .bind(war)
    .bind(DEFENDER_SIDE)
    .fetch_all(&mut *transaction)
    .await?;
    let taken = taken
        .into_iter()
        .map(|(group, slot)| {
            Ok((
                narrow(i64::from(group), PLACE_FIELD)?,
                narrow(i64::from(slot), PLACE_FIELD)?,
            ))
        })
        .collect::<Result<Vec<(u8, u8)>, DbError>>()?;
    let waiting: Vec<Uuid> = sqlx::query_scalar(
        "SELECT s.character_id FROM war_signup s JOIN territory_war w ON w.war_id = s.war_id \
         WHERE s.war_id = $1 AND s.side = $2 AND s.roster_group IS NULL AND NOT s.kicked \
         AND w.attacker_company IS NULL ORDER BY s.signed, s.character_id FOR UPDATE",
    )
    .bind(war)
    .bind(DEFENDER_SIDE)
    .fetch_all(&mut *transaction)
    .await?;
    let mut placed = 0_u64;
    for (character, (group, slot)) in waiting.into_iter().zip(free_places(&taken)) {
        sqlx::query(
            "UPDATE war_signup SET roster_group = $3, roster_slot = $4 \
             WHERE war_id = $1 AND character_id = $2",
        )
        .bind(war)
        .bind(character)
        .bind(i16::from(group))
        .bind(i16::from(slot))
        .execute(&mut *transaction)
        .await?;
        placed = placed.saturating_add(1);
    }
    transaction.commit().await?;
    Ok(placed)
}

/// Every stored upgrade tier, by territory and upgrade id.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails and [`DbError::OutOfRange`] for a stored
/// value the program's type cannot hold.
pub async fn territory_upgrades(world: &PgPool) -> Result<Vec<TerritoryUpgrade>, DbError> {
    let rows: Vec<(i16, i16, i16)> = sqlx::query_as(
        "SELECT territory_id, upgrade_id, tier FROM territory_upgrade \
         ORDER BY territory_id, upgrade_id",
    )
    .fetch_all(world)
    .await?;
    rows.into_iter()
        .map(|(territory, upgrade, tier)| {
            Ok(TerritoryUpgrade {
                territory: narrow(i64::from(territory), TERRITORY_FIELD)?,
                upgrade: narrow(i64::from(upgrade), UPGRADE_FIELD)?,
                tier: narrow(i64::from(tier), UPGRADE_FIELD)?,
            })
        })
        .collect()
}

/// Sets the tier of one upgrade of a territory (a game master's change; the town projects that
/// raise tiers in retail are in no capture).
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails.
pub async fn set_territory_upgrade(
    world: &PgPool,
    upgrade: &TerritoryUpgrade,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO territory_upgrade (territory_id, upgrade_id, tier) VALUES ($1, $2, $3) \
         ON CONFLICT (territory_id, upgrade_id) DO UPDATE SET tier = excluded.tier",
    )
    .bind(narrow::<i16>(
        i64::from(upgrade.territory),
        TERRITORY_FIELD,
    )?)
    .bind(i16::from(upgrade.upgrade))
    .bind(i16::from(upgrade.tier.min(UPGRADE_TIER_MAX)))
    .execute(world)
    .await?;
    Ok(())
}

/// Gives the invasion `war` its loss at Unix ms `at_ms` (resolved without a winner) and lowers
/// each upgrade of `lowered` of its territory by one tier, in one transaction. False when the
/// war is not an invasion or had a result already.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails.
pub async fn lose_invasion(
    world: &PgPool,
    war: Uuid,
    at_ms: u64,
    lowered: &[u8],
) -> Result<bool, DbError> {
    let mut transaction = world.begin().await?;
    let territory: Option<i16> = sqlx::query_scalar(
        "UPDATE territory_war SET resolved_ms = $2 \
         WHERE war_id = $1 AND resolved_ms IS NULL AND attacker_company IS NULL \
         RETURNING territory_id",
    )
    .bind(war)
    .bind(to_column(at_ms, TIME_FIELD)?)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(territory) = territory else {
        return Ok(false);
    };
    let lowered: Vec<i16> = lowered.iter().copied().map(i16::from).collect();
    sqlx::query(
        "UPDATE territory_upgrade SET tier = tier - 1 \
         WHERE territory_id = $1 AND upgrade_id = ANY($2) AND tier > 0",
    )
    .bind(territory)
    .bind(&lowered)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roster_fill_takes_the_free_places_group_by_group() {
        let taken = [(0, 0), (0, 2)];
        let places: Vec<(u8, u8)> = free_places(&taken).take(4).collect();
        assert_eq!(places, vec![(0, 1), (0, 3), (0, 4), (1, 0)]);
        assert_eq!(
            free_places(&[]).count(),
            usize::from(ROSTER_GROUP_COUNT) * usize::from(ROSTER_GROUP_SIZE)
        );
    }
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::territory::infra_tests::company;
    use crate::test_db::with_test_schema;
    use crate::{
        NewCharacter, insert_character, resolve_war, set_territory_owner, sign_up_for_war,
        territory_wars,
    };

    /// A made-up territory, company, start time and gear score.
    const TERRITORY: u16 = 16;
    const OWNER: &str = "Made Up Defender";
    const STARTS_MS: u64 = 1_900_000_000_000;
    const CYCLE_END_MS: u64 = 1_900_014_450_000;
    const GEAR_SCORE: u16 = 700;
    const SIDE_DEFENDER: u8 = 2;
    const UPGRADE: u8 = 12;
    const OTHER_UPGRADE: u8 = 8;

    async fn player(pool: &PgPool, name: &str) -> Uuid {
        let id = Uuid::new_v4();
        insert_character(
            pool,
            &NewCharacter {
                id,
                account_id: Uuid::new_v4(),
                name: name.to_owned(),
                appearance: serde_json::json!({}),
            },
        )
        .await
        .expect("insert character");
        id
    }

    #[tokio::test]
    async fn an_invasion_is_scheduled_filled_and_lost_with_its_upgrades_lowered() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            assert_eq!(
                schedule_invasion(pool, TERRITORY, STARTS_MS)
                    .await
                    .expect("schedule"),
                InvasionScheduled::NoOwner
            );
            let owner = company(pool, OWNER, 1).await;
            set_territory_owner(pool, TERRITORY, OWNER)
                .await
                .expect("owner");
            let step = DarknessStep {
                darkness: TerritoryDarkness {
                    territory: TERRITORY,
                    level: DARKNESS_INVASION_LEVEL,
                    cycle_end_ms: CYCLE_END_MS,
                },
                invasion_starts_ms: Some(STARTS_MS),
            };
            apply_darkness(pool, &[step]).await.expect("darkness");
            assert_eq!(
                territory_darkness(pool).await.expect("darkness"),
                vec![step.darkness]
            );
            assert_eq!(
                schedule_invasion(pool, TERRITORY, STARTS_MS)
                    .await
                    .expect("schedule"),
                InvasionScheduled::AtWar
            );
            let wars = territory_wars(pool, STARTS_MS).await.expect("wars");
            let [war] = wars.as_slice() else {
                panic!("one invasion: {wars:?}");
            };
            assert!(war.is_invasion());
            assert_eq!(war.defender.0, owner);
            let (first, second) = (player(pool, "Testc").await, player(pool, "Testd").await);
            for character in [first, second] {
                assert!(
                    sign_up_for_war(pool, war.war, character, SIDE_DEFENDER, GEAR_SCORE)
                        .await
                        .expect("sign up")
                );
            }
            assert_eq!(fill_invasion_roster(pool, war.war).await.expect("fill"), 2);
            assert_eq!(fill_invasion_roster(pool, war.war).await.expect("fill"), 0);
            for (upgrade, tier) in [(UPGRADE, 2), (OTHER_UPGRADE, 0)] {
                let upgrade = TerritoryUpgrade {
                    territory: TERRITORY,
                    upgrade,
                    tier,
                };
                set_territory_upgrade(pool, &upgrade)
                    .await
                    .expect("upgrade");
            }
            assert!(
                lose_invasion(pool, war.war, STARTS_MS, &[UPGRADE, OTHER_UPGRADE])
                    .await
                    .expect("lose")
            );
            assert!(
                !resolve_war(pool, war.war, owner, STARTS_MS)
                    .await
                    .expect("resolve")
            );
            let tiers = territory_upgrades(pool).await.expect("upgrades");
            assert_eq!(
                tiers
                    .iter()
                    .map(|u| (u.upgrade, u.tier))
                    .collect::<Vec<_>>(),
                vec![(OTHER_UPGRADE, 0), (UPGRADE, 1)]
            );
            let wars = territory_wars(pool, STARTS_MS).await.expect("wars");
            assert_eq!(wars[0].result, Some((Uuid::nil(), STARTS_MS)));
            assert_eq!(wars[0].roster().count(), 2);
        })
        .await;
    }
}
