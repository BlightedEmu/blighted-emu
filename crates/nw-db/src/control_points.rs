// SPDX-License-Identifier: AGPL-3.0-or-later
//! The faction control points: the fort of each claim and the influence towers (world
//! migration 0047; findings `2026-11-12-wars-and-sieges.md`, "Control points and towers").
//!
//! **Points** (Observed, every retail 3086 of the capture index): the claims' faction control
//! point list has one four-bit entry per point; entries 0 to 12 are the forts of claims 0 to 12
//! ([`FORT_POINTS`]) and entries 18 to 56 the 39 influence towers, three per territory
//! ([`FIRST_TOWER_POINT`]). A row here holds a point's owner faction, the faction whose capture
//! runs, and a fort's lock.
//!
//! **The race window** (Observed, ~140 tower activations): a territory's towers turn active
//! [`RACE_WINDOW_MS`] before each daily conflict slot (`crate::conflicts`) and go back to
//! inactive with no owner at the slot; forts captured in the window do not lock, forts captured
//! outside it lock for [`FORT_LOCK_MS`] (Observed, `fort_capping_pvp_flagged`: the lock ended
//! 3,599 s after the capture; the map text "will lock for one hour").
//!
//! **Influence** (`crate::add_influence`): a faction's gain in a territory rises by
//! [`TOWER_BONUS_PERCENT`] for each tower it holds there in the window (Inferred: the client's
//! `-tower-influence-percent-bonus` setting is in no client file), by [`FORT_BONUS_PERCENT`]
//! when it holds the territory's fort (Observed, `javelindata_factioncontrol_buffs`
//! `InfluencePoints_Modifier` 0.2, a territory buff) and by [`UNDERDOG_BONUS_PERCENT`] when it
//! owns at least [`UNDERDOG_TERRITORY_DIFF`] territories less than the faction with the most
//! (Observed, localization `ui_territoryinfluencetooltip`: "Factions with 2 less territories
//! owned earn an added 10% Influence Point Bonus"; Inferred: less than the faction with the
//! most). Each tower held in the window gives its faction [`TOWER_INFLUENCE_POINTS`] every
//! [`TOWER_INFLUENCE_INTERVAL_MS`] (Observed text "+N Influence every 5 seconds"; the N is the
//! client's `-tower-influence-amount` setting, in no client file: Inferred).

use sqlx::{PgPool, Postgres, Transaction};

use crate::error::DbError;
use crate::progression::{from_column, to_column};
use crate::social_graph::narrow;

const INDEX_FIELD: &str = "control point index";
const TERRITORY_FIELD: &str = "control point territory id";
const FACTION_FIELD: &str = "control point faction";
const TIME_FIELD: &str = "control point time";

/// The forts: one per claim, entries 0 to 12 (Observed).
pub const FORT_POINTS: u8 = 13;
/// The first tower entry (Observed: 18 to 56 hold the 39 towers).
pub const FIRST_TOWER_POINT: u8 = 18;
/// The towers of each territory (Observed: three entries turn active together).
pub const TOWERS_PER_TERRITORY: usize = 3;
/// How long before a conflict slot the race window opens (Observed: 2,700 s).
pub const RACE_WINDOW_MS: u64 = 2_700_000;
/// How long a fort captured outside the window stays locked (Observed: one hour).
pub const FORT_LOCK_MS: u64 = 3_600_000;
/// The influence a held tower gives every interval (Inferred, module doc).
pub const TOWER_INFLUENCE_POINTS: u64 = 1;
/// The interval of a tower's influence (Observed text).
pub const TOWER_INFLUENCE_INTERVAL_MS: u64 = 5_000;
/// The gain bonus of each held tower, percent (Inferred, module doc).
pub const TOWER_BONUS_PERCENT: u64 = 10;
/// The gain bonus of the held fort, percent (Observed: `InfluencePoints_Modifier` 0.2).
pub const FORT_BONUS_PERCENT: u64 = 20;
/// The underdog gain bonus, percent (Observed, localization).
pub const UNDERDOG_BONUS_PERCENT: u64 = 10;
/// The character experience bonus of the faction that holds a territory's fort, percent
/// (Observed: `javelindata_factioncontrol_buffs` `ExperiencePoints_Modifier` 0.05, a territory
/// buff of every claimed territory's `FactionControlTerritoryBuffs`; localization
/// `@ui_experiencepoints_modifier_desc` "Experience Points increased by {value}" under the fort's
/// "Territory Bonuses"). Inferred: it counts for the holding faction's players while they are
/// in that territory, on top of the remote config multiplier, rounded down ([`with_bonus`]).
pub const FORT_EXPERIENCE_BONUS_PERCENT: u64 = 5;
/// How many territories less an underdog faction owns (Observed, localization).
pub const UNDERDOG_TERRITORY_DIFF: u64 = 2;
/// The whole of a gain, percent.
const WHOLE_PERCENT: u64 = 100;
/// The faction byte of no faction.
const NO_FACTION: u8 = 0;
/// The factions, 1 to 3.
const FACTIONS: usize = 3;

/// Whether control point `index` is an influence tower.
pub const fn is_tower(index: u8) -> bool {
    index >= FIRST_TOWER_POINT
}

/// Whether Unix ms `now_ms` lies in the race window before the conflict slot `next_slot_ms`
/// (0: no slot, the claim has no owner).
pub const fn in_race_window(next_slot_ms: u64, now_ms: u64) -> bool {
    next_slot_ms != 0 && now_ms < next_slot_ms && now_ms + RACE_WINDOW_MS >= next_slot_ms
}

/// The gain bonus, percent, of a faction that holds `towers` towers in the window, holds the
/// fort when `fort` and is an underdog when `underdog`.
pub const fn bonus_percent(towers: u64, fort: bool, underdog: bool) -> u64 {
    let fort = if fort { FORT_BONUS_PERCENT } else { 0 };
    let underdog = if underdog { UNDERDOG_BONUS_PERCENT } else { 0 };
    towers * TOWER_BONUS_PERCENT + fort + underdog
}

/// `points` with the bonus `percent` (rounded down).
pub const fn with_bonus(points: u64, percent: u64) -> u64 {
    points.saturating_mul(WHOLE_PERCENT + percent) / WHOLE_PERCENT
}

/// Whether faction `faction` (1 to 3) is an underdog when the factions own `owned` territories.
pub fn is_underdog(faction: u8, owned: [u64; FACTIONS]) -> bool {
    let Some(own) = usize::from(faction)
        .checked_sub(1)
        .and_then(|index| owned.get(index))
    else {
        return false;
    };
    owned
        .iter()
        .max()
        .is_some_and(|most| *most >= *own + UNDERDOG_TERRITORY_DIFF)
}

/// A control point's shared state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlPoint {
    /// The entry of the claims' list (0 to 12 forts, 18 to 56 towers).
    pub index: u8,
    /// The territory id.
    pub territory: u16,
    /// The owner faction, 0 for none.
    pub owner: u8,
    /// When the owner took it, Unix ms.
    pub captured_ms: u64,
    /// The faction whose capture runs, 0 for none.
    pub capturing: u8,
    /// When a fort's lock ends, Unix ms; 0 for none.
    pub locked_until_ms: u64,
}

type PointRow = (i16, i16, i16, i64, i16, i64);

/// Every stored control point, by index.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails and [`DbError::OutOfRange`] for a stored
/// value the program's type cannot hold.
pub async fn control_points(world: &PgPool) -> Result<Vec<ControlPoint>, DbError> {
    let rows: Vec<PointRow> = sqlx::query_as(
        "SELECT point_index, territory_id, owner_faction, captured_ms, capturing_faction, \
         locked_until_ms FROM faction_control_point ORDER BY point_index",
    )
    .fetch_all(world)
    .await?;
    rows.into_iter()
        .map(|(index, territory, owner, captured, capturing, locked)| {
            Ok(ControlPoint {
                index: narrow(i64::from(index), INDEX_FIELD)?,
                territory: narrow(i64::from(territory), TERRITORY_FIELD)?,
                owner: narrow(i64::from(owner), FACTION_FIELD)?,
                captured_ms: from_column(captured, TIME_FIELD)?,
                capturing: narrow(i64::from(capturing), FACTION_FIELD)?,
                locked_until_ms: from_column(locked, TIME_FIELD)?,
            })
        })
        .collect()
}

/// Stores `point`. A new owner or capture time starts its tower influence at the capture.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails and [`DbError::OutOfRange`] for a value a
/// column cannot hold.
pub async fn store_control_point(world: &PgPool, point: &ControlPoint) -> Result<(), DbError> {
    let captured = to_column(point.captured_ms, TIME_FIELD)?;
    sqlx::query(
        "INSERT INTO faction_control_point (point_index, territory_id, owner_faction, \
         captured_ms, capturing_faction, locked_until_ms, credited_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $4) ON CONFLICT (point_index) DO UPDATE SET \
         territory_id = EXCLUDED.territory_id, owner_faction = EXCLUDED.owner_faction, \
         captured_ms = EXCLUDED.captured_ms, capturing_faction = EXCLUDED.capturing_faction, \
         locked_until_ms = EXCLUDED.locked_until_ms, credited_ms = CASE \
         WHEN faction_control_point.owner_faction <> EXCLUDED.owner_faction \
         OR faction_control_point.captured_ms <> EXCLUDED.captured_ms \
         THEN EXCLUDED.credited_ms ELSE faction_control_point.credited_ms END",
    )
    .bind(i16::from(point.index))
    .bind(narrow::<i16>(i64::from(point.territory), TERRITORY_FIELD)?)
    .bind(i16::from(point.owner))
    .bind(captured)
    .bind(i16::from(point.capturing))
    .bind(to_column(point.locked_until_ms, TIME_FIELD)?)
    .execute(world)
    .await?;
    Ok(())
}

/// The gain bonus, percent, of faction `faction` in `territory` at Unix ms `now_ms`, when the
/// territory's race window is the one before `next_slot_ms`.
pub(crate) async fn faction_bonus(
    transaction: &mut Transaction<'_, Postgres>,
    territory: i16,
    faction: u8,
    next_slot_ms: u64,
    now_ms: u64,
) -> Result<u64, DbError> {
    let towers: i64 = if in_race_window(next_slot_ms, now_ms) {
        sqlx::query_scalar(
            "SELECT count(*) FROM faction_control_point WHERE territory_id = $1 \
             AND point_index >= $2 AND owner_faction = $3 AND captured_ms >= $4",
        )
        .bind(territory)
        .bind(i16::from(FIRST_TOWER_POINT))
        .bind(i16::from(faction))
        .bind(to_column(next_slot_ms - RACE_WINDOW_MS, TIME_FIELD)?)
        .fetch_one(&mut **transaction)
        .await?
    } else {
        0
    };
    let fort: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM faction_control_point WHERE territory_id = $1 \
         AND point_index < $2 AND owner_faction = $3)",
    )
    .bind(territory)
    .bind(i16::from(FORT_POINTS))
    .bind(i16::from(faction))
    .fetch_one(&mut **transaction)
    .await?;
    let counts: Vec<(i16, i64)> = sqlx::query_as(
        "SELECT c.faction, count(*) FROM territory_owner o \
         JOIN company c ON c.company_id = o.company_id GROUP BY c.faction",
    )
    .fetch_all(&mut **transaction)
    .await?;
    let mut owned = [0; FACTIONS];
    for (owner, count) in counts {
        let owner = narrow::<u8>(i64::from(owner), FACTION_FIELD)?;
        if let Some(slot) = usize::from(owner)
            .checked_sub(1)
            .and_then(|index| owned.get_mut(index))
        {
            *slot = from_column(count, INDEX_FIELD)?;
        }
    }
    Ok(bonus_percent(
        from_column(towers, INDEX_FIELD)?,
        fort,
        faction != NO_FACTION && is_underdog(faction, owned),
    ))
}

/// The tower influence due in `territory` up to Unix ms `now_ms` in the race window before
/// `next_slot_ms`: for each tower held in the window, its faction and the points of the
/// intervals since it was last credited; marks them credited.
pub(crate) async fn take_tower_influence(
    transaction: &mut Transaction<'_, Postgres>,
    territory: i16,
    next_slot_ms: u64,
    now_ms: u64,
) -> Result<Vec<(u8, u64)>, DbError> {
    if next_slot_ms == 0 || now_ms + RACE_WINDOW_MS < next_slot_ms {
        return Ok(Vec::new());
    }
    let start = next_slot_ms - RACE_WINDOW_MS;
    let until = now_ms.min(next_slot_ms);
    let rows: Vec<(i16, i16, i64)> = sqlx::query_as(
        "SELECT point_index, owner_faction, credited_ms FROM faction_control_point \
         WHERE territory_id = $1 AND point_index >= $2 AND owner_faction > 0 \
         AND captured_ms >= $3 FOR UPDATE",
    )
    .bind(territory)
    .bind(i16::from(FIRST_TOWER_POINT))
    .bind(to_column(start, TIME_FIELD)?)
    .fetch_all(&mut **transaction)
    .await?;
    let mut due = Vec::with_capacity(rows.len());
    for (index, owner, credited) in rows {
        let credited: u64 = from_column(credited, TIME_FIELD)?;
        let steps = until.saturating_sub(credited) / TOWER_INFLUENCE_INTERVAL_MS;
        if steps == 0 {
            continue;
        }
        sqlx::query("UPDATE faction_control_point SET credited_ms = $2 WHERE point_index = $1")
            .bind(index)
            .bind(to_column(
                credited + steps * TOWER_INFLUENCE_INTERVAL_MS,
                TIME_FIELD,
            )?)
            .execute(&mut **transaction)
            .await?;
        due.push((
            narrow(i64::from(owner), FACTION_FIELD)?,
            steps * TOWER_INFLUENCE_POINTS,
        ));
    }
    Ok(due)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLOT_MS: u64 = 1_794_506_400_000;

    #[test]
    fn the_window_is_the_45_minutes_before_the_slot() {
        assert!(in_race_window(SLOT_MS, SLOT_MS - RACE_WINDOW_MS));
        assert!(in_race_window(SLOT_MS, SLOT_MS - 1));
        assert!(!in_race_window(SLOT_MS, SLOT_MS));
        assert!(!in_race_window(SLOT_MS, SLOT_MS - RACE_WINDOW_MS - 1));
        assert!(!in_race_window(0, SLOT_MS));
    }

    #[test]
    fn towers_fort_and_underdog_add_their_percent() {
        assert_eq!(bonus_percent(0, false, false), 0);
        assert_eq!(bonus_percent(2, true, true), 50);
        assert_eq!(with_bonus(100, bonus_percent(1, true, false)), 130);
        assert_eq!(with_bonus(50, bonus_percent(0, false, true)), 55);
    }

    #[test]
    fn an_underdog_owns_two_territories_less_than_the_most() {
        assert!(is_underdog(3, [5, 2, 3]));
        assert!(!is_underdog(2, [5, 4, 3]));
        assert!(!is_underdog(1, [5, 2, 3]));
        assert!(!is_underdog(0, [5, 0, 0]));
    }

    #[test]
    fn the_towers_are_the_entries_from_18() {
        assert!(!is_tower(FORT_POINTS - 1));
        assert!(is_tower(FIRST_TOWER_POINT));
    }
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::conflicts::FULL_PERCENT;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::territory::infra_tests::company;
    use crate::test_db::with_test_schema;
    use uuid::Uuid;

    use crate::conflicts::DAY_MS;
    use crate::{add_influence, advance_conflicts, set_territory_owner, territory_conflicts};

    /// Territory 6: claim index 2 (its fort) and towers 23, 38 and 50 (findings).
    const TERRITORY: u16 = 6;
    const FORT: u8 = 2;
    const TOWER: u8 = 23;
    const OWNER: &str = "Made Up Owner";
    const ATTACKER: &str = "Made Up Attacker";
    const OWNER_FACTION: u8 = 1;
    const ATTACKING_FACTION: u8 = 2;
    const MISSION_POINTS: u64 = 100;
    /// 2026-11-12 00:00 UTC; the test owner's siege hour is 18, so the slot is at 17:45 and
    /// the window opens at 17:00.
    const MIDNIGHT_MS: u64 = 1_794_441_600_000;
    /// 17:45 from midnight.
    const SLOT_OF_DAY_MS: u64 = 63_900_000;
    const SLOT_MS: u64 = MIDNIGHT_MS + SLOT_OF_DAY_MS;
    /// The race started two days before: the slot of this day is the next one.
    const RACE_DAYS: u64 = 2;
    const RACE_START_MS: u64 = MIDNIGHT_MS - RACE_DAYS * DAY_MS;
    const IN_WINDOW_MS: u64 = SLOT_MS - RACE_WINDOW_MS + 600_000;
    const TEN_SECONDS_MS: u64 = 10_000;

    async fn attacker_points(pool: &PgPool) -> i64 {
        sqlx::query_scalar(
            "SELECT points FROM territory_influence WHERE territory_id = $1 AND faction = $2",
        )
        .bind(i16::try_from(TERRITORY).expect("territory"))
        .bind(i16::from(ATTACKING_FACTION))
        .fetch_one(pool)
        .await
        .expect("points")
    }

    #[tokio::test]
    async fn a_held_fort_and_tower_raise_the_gain_and_the_tower_gives_influence() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            company(pool, OWNER, OWNER_FACTION).await;
            let attacker = company(pool, ATTACKER, ATTACKING_FACTION).await;
            assert!(
                set_territory_owner(pool, TERRITORY, OWNER)
                    .await
                    .expect("owner")
            );
            sqlx::query(
                "INSERT INTO territory_conflict (territory_id, race_start_ms) VALUES ($1, $2)",
            )
            .bind(i16::try_from(TERRITORY).expect("territory"))
            .bind(i64::try_from(RACE_START_MS).expect("time"))
            .execute(pool)
            .await
            .expect("race");
            let read = territory_conflicts(pool, IN_WINDOW_MS).await.expect("read");
            assert_eq!(read[0].next_slot_ms, SLOT_MS);
            for index in [FORT, TOWER] {
                store_control_point(
                    pool,
                    &ControlPoint {
                        index,
                        territory: TERRITORY,
                        owner: ATTACKING_FACTION,
                        captured_ms: IN_WINDOW_MS,
                        capturing: NO_FACTION,
                        locked_until_ms: 0,
                    },
                )
                .await
                .expect("store");
            }
            let stored = control_points(pool).await.expect("points");
            assert_eq!(stored.len(), 2);
            assert_eq!(stored[1].index, TOWER);
            let member: Uuid =
                sqlx::query_scalar("SELECT character_id FROM company_member WHERE company_id = $1")
                    .bind(attacker)
                    .fetch_one(pool)
                    .await
                    .expect("member");
            add_influence(
                pool,
                TERRITORY,
                ATTACKING_FACTION,
                member,
                MISSION_POINTS,
                IN_WINDOW_MS,
            )
            .await
            .expect("gain");
            // One mission, +20 % for the fort and +10 % for the tower.
            let bonus = FORT_BONUS_PERCENT + TOWER_BONUS_PERCENT;
            let gain = MISSION_POINTS * (FULL_PERCENT + bonus) / FULL_PERCENT;
            assert_eq!(
                attacker_points(pool).await,
                i64::try_from(gain).expect("points")
            );
            // Ten seconds later: two tower intervals, with the same bonus.
            advance_conflicts(pool, IN_WINDOW_MS + TEN_SECONDS_MS, |_| 0)
                .await
                .expect("advance");
            let tower = with_bonus(2 * TOWER_INFLUENCE_POINTS, bonus);
            assert_eq!(
                attacker_points(pool).await,
                i64::try_from(gain + tower).expect("points")
            );
            // A tower taken in one window gives nothing in the next.
            let next_slot = SLOT_MS + DAY_MS;
            let mut transaction = pool.begin().await.expect("begin");
            let due = take_tower_influence(
                &mut transaction,
                i16::try_from(TERRITORY).expect("territory"),
                next_slot,
                next_slot - RACE_WINDOW_MS + TEN_SECONDS_MS,
            )
            .await
            .expect("due");
            assert!(due.is_empty());
        })
        .await;
    }
}
