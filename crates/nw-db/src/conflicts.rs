// SPDX-License-Identifier: AGPL-3.0-or-later
//! The influence race of each territory, its conflict, and the war declarations of the
//! conflict's lottery (world migration 0042).
//!
//! **Influence** (Observed, `GameEventData`): the success event of a PvP faction mission gives
//! the player's faction `FactionInfluenceAmount` influence (50, 100 or 200, source `Missions`) in
//! the mission's territory. The client shows each faction's influence as a percent of the claim
//! (3086 `replicatedFaction1InfluencePercentages` to `3`). The points that make 100 % are the
//! server asset's `MaxInfluence` (`FactionInfluenceConfigData`, `FUN_145c9d110`), which is in no
//! client file: [`MAX_INFLUENCE`] is Inferred from the retail rises, 4,053 of 4,121 of them one
//! percent (every 3086 of the capture index), the step of one 100-point mission. The members of
//! a company also count the points for their company ([`MIN_COMPANY_CONTRIBUTION`]).
//!
//! **Decay** (Observed): outside a conflict each faction loses one percent about every 90 s (176
//! of 230 timed retail decreases came 70 to 120 s apart; 363 of 369 decreases were outside a
//! conflict). The points keep the time their decay was last counted, so a read at any time
//! counts the steps since then ([`decayed`]).
//!
//! **Conflict** (Observed, the ~140 retail conflict starts): a conflict starts at minute 45 of an
//! hour, 13 h 45 min plus whole hours after the race start; its faction is the faction with the
//! most influence. Inferred: the hour is the defender's siege window (the localization: a war is
//! declared during the defender's siege window and is fought in the siege window of the next
//! day), so a conflict starts [`CONFLICT_BEFORE_SIEGE_MS`] before the owner company's siege hour
//! on the first day at least [`RACE_MIN_MS`] after the race start ([`conflict_slot`]); the
//! owner's faction never has the conflict, and a race whose attacking factions have no influence
//! has none. No influence is gained or lost during a conflict (Observed: the retail values stay
//! the same, Inferred for the gains).
//!
//! **Lottery** (Observed): the first declaration sets the lottery end to its time +
//! [`LOTTERY_LENGTH_MS`] (9 of 9 retail lottery ends). The conflict ends at the lottery end or
//! [`CONFLICT_LENGTH_MS`] after its start; then every faction's influence and the conflict go
//! back to zero and a new race starts at the end time. Inferred: the lottery draws one
//! declaration with the same chance for each; the drawn company gets the war at the defender's
//! siege hour of the next day ([`war_start_ms`]), the others get their coin back.
//!
//! **Cost** (Observed, `TerritoryUpkeepDefinition`): `WardecCost` 1,000,000 hundredths at every
//! claim level and `UpkeepModifier` 0.1 on every claim: [`WAR_DECLARATION_COST`]. Inferred: each
//! war camp tier above the first adds that cost once more ([`war_cost`]); the client's own cost
//! function is in no client file of the sweep.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};
use crate::social_graph::{TreasuryReason, narrow};

const TERRITORY_FIELD: &str = "conflict territory id";
const FACTION_FIELD: &str = "conflict faction";
const TIME_FIELD: &str = "conflict time";
const POINTS_FIELD: &str = "influence points";
const COST_FIELD: &str = "war declaration cost";
const CAMP_FIELD: &str = "war camp tier";

/// The influence points that make 100 % (Inferred, module doc).
pub const MAX_INFLUENCE: u64 = 10_000;
/// The percent of [`MAX_INFLUENCE`].
pub const FULL_PERCENT: u64 = 100;
/// The points a faction loses at each decay step: one percent (Observed, module doc).
pub const DECAY_POINTS: u64 = MAX_INFLUENCE / FULL_PERCENT;
/// The time between two decay steps (Observed, module doc).
pub const DECAY_INTERVAL_MS: u64 = 90_000;
/// The shortest race (Observed: 49,500 s, the shortest of the hour-aligned retail races).
pub const RACE_MIN_MS: u64 = 49_500_000;
/// How long before the siege hour a conflict starts (Observed: at minute 45).
pub const CONFLICT_BEFORE_SIEGE_MS: u64 = 900_000;
/// The longest conflict (Observed: a conflict without a drawn lottery ended 3,600 s after its
/// start, `ef_tour_0-Hermit-s-Rally` 1790361914491).
pub const CONFLICT_LENGTH_MS: u64 = 3_600_000;
/// The lottery after the first declaration (Observed, module doc).
pub const LOTTERY_LENGTH_MS: u64 = 600_000;
/// One day.
pub const DAY_MS: u64 = 86_400_000;
/// One hour.
pub const HOUR_MS: u64 = 3_600_000;
/// The cost of a tier I war camp declaration, hundredths of a coin (Observed, module doc).
pub const WAR_DECLARATION_COST: u64 = 100_000;
/// The war camp tiers: 0 to 2 for tier I to III (Observed: the popup's three camps).
pub const WAR_CAMP_TIERS: u8 = 3;
/// The least influence a company's members earned in a territory since its last conflict for
/// the company to declare there (Inferred: one mission of the smallest grant, 50; the client
/// refuses with `InsufficientContribution`, its threshold is the server asset's
/// `MinimumInfluenceThresholdForWar`, in no client file).
pub const MIN_COMPANY_CONTRIBUTION: u64 = 50;
/// The faction byte of no faction.
pub const NO_FACTION: u8 = 0;
/// The factions, 1 to 3.
pub const FACTIONS: [u8; 3] = [1, 2, 3];

/// The points left of `points` whose decay was counted at Unix ms `decayed_ms`, at `now_ms`, and
/// the time the decay is counted to (a whole number of steps after `decayed_ms`).
pub const fn decayed(points: u64, decayed_ms: u64, now_ms: u64) -> (u64, u64) {
    let steps = now_ms.saturating_sub(decayed_ms) / DECAY_INTERVAL_MS;
    (
        points.saturating_sub(steps.saturating_mul(DECAY_POINTS)),
        decayed_ms.saturating_add(steps.saturating_mul(DECAY_INTERVAL_MS)),
    )
}

/// The percent the client shows for `points`, at most 100.
pub fn influence_percent(points: u64) -> u8 {
    // At most FULL_PERCENT, which fits a u8.
    u8::try_from((points.saturating_mul(FULL_PERCENT) / MAX_INFLUENCE).min(FULL_PERCENT))
        .unwrap_or(u8::MAX)
}

/// The time of day of siege hour `siege_hour` (hours from midnight, Unix time), ms.
fn siege_ms_of_day(siege_hour: f32) -> u64 {
    // A siege hour is 0 to 24; any other value is taken modulo one day.
    ((f64::from(siege_hour) * HOUR_MS as f64).round().max(0.0) as u64) % DAY_MS
}

/// The start of the conflict a race that started at Unix ms `race_start_ms` has at `now_ms` for
/// an owner of siege hour `siege_hour`: the latest daily slot ([`CONFLICT_BEFORE_SIEGE_MS`]
/// before the siege hour) not before `now_ms` and at least [`RACE_MIN_MS`] after the race start;
/// `None` before the first slot.
pub fn conflict_slot(race_start_ms: u64, siege_hour: f32, now_ms: u64) -> Option<u64> {
    let earliest = race_start_ms.saturating_add(RACE_MIN_MS);
    let slot_of_day = (siege_ms_of_day(siege_hour) + DAY_MS - CONFLICT_BEFORE_SIEGE_MS) % DAY_MS;
    let day_start = earliest - earliest % DAY_MS;
    let mut first = day_start + slot_of_day;
    if first < earliest {
        first += DAY_MS;
    }
    (now_ms >= first).then(|| first + (now_ms - first) / DAY_MS * DAY_MS)
}

/// When a conflict that started at `start_ms` ends: at its lottery end `lottery_end_ms` when a
/// declaration set one, else [`CONFLICT_LENGTH_MS`] after its start, whichever comes first.
pub fn conflict_end_ms(start_ms: u64, lottery_end_ms: u64) -> u64 {
    let longest = start_ms.saturating_add(CONFLICT_LENGTH_MS);
    if lottery_end_ms > start_ms {
        longest.min(lottery_end_ms)
    } else {
        longest
    }
}

/// When the war of a conflict that started at `start_ms` starts: the siege hour of the next day.
pub const fn war_start_ms(start_ms: u64) -> u64 {
    start_ms + CONFLICT_BEFORE_SIEGE_MS + DAY_MS
}

/// The cost of a declaration with war camp tier `camp`, hundredths of a coin; `None` for a tier
/// the popup does not have.
pub fn war_cost(camp: u8) -> Option<u64> {
    (camp < WAR_CAMP_TIERS).then(|| WAR_DECLARATION_COST * (u64::from(camp) + 1))
}

/// The attacking faction with the most of `points` (factions 1 to 3) when the owner has faction
/// `owner_faction`; the first of equals; `None` when no attacking faction has points.
pub fn conflict_leader(points: [u64; 3], owner_faction: u8) -> Option<u8> {
    FACTIONS
        .iter()
        .zip(points)
        .filter(|(faction, points)| **faction != owner_faction && *points > 0)
        .fold(None::<(u8, u64)>, |best, (faction, points)| match best {
            Some((_, most)) if most >= points => best,
            _ => Some((*faction, points)),
        })
        .map(|(faction, _)| faction)
}

/// A territory's race and conflict, as every client sees them in 3086.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerritoryConflict {
    /// The territory id.
    pub territory: u16,
    /// The influence percent of factions 1 to 3.
    pub influence: [u8; 3],
    /// When the race started, Unix ms.
    pub race_start_ms: u64,
    /// The conflict's faction, [`NO_FACTION`] for none.
    pub conflict_faction: u8,
    /// When the conflict started, Unix ms; 0 for none.
    pub conflict_start_ms: u64,
    /// When the last lottery ends or ended, Unix ms; 0 for none.
    pub lottery_end_ms: u64,
}

type ConflictRow = (i16, i64, i16, i64, i64);

/// Every territory with a race, with each faction's influence at Unix ms `now_ms`, by territory.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails and [`DbError::OutOfRange`] for a stored
/// value the program's type cannot hold.
pub async fn territory_conflicts(
    world: &PgPool,
    now_ms: u64,
) -> Result<Vec<TerritoryConflict>, DbError> {
    let rows: Vec<ConflictRow> = sqlx::query_as(
        "SELECT territory_id, race_start_ms, conflict_faction, conflict_start_ms, lottery_end_ms \
         FROM territory_conflict ORDER BY territory_id",
    )
    .fetch_all(world)
    .await?;
    let points: Vec<(i16, i16, i64, i64)> =
        sqlx::query_as("SELECT territory_id, faction, points, decayed_ms FROM territory_influence")
            .fetch_all(world)
            .await?;
    rows.into_iter()
        .map(|(territory, race, faction, start, lottery)| {
            let conflict_faction = narrow(i64::from(faction), FACTION_FIELD)?;
            let mut influence = [0; 3];
            for (entry, faction, points, decayed_ms) in &points {
                if entry != &territory {
                    continue;
                }
                let index = usize::from(narrow::<u8>(i64::from(*faction), FACTION_FIELD)?)
                    .saturating_sub(1)
                    .min(FACTIONS.len() - 1);
                let points = from_column(*points, POINTS_FIELD)?;
                let decayed_ms = from_column(*decayed_ms, TIME_FIELD)?;
                let left = if conflict_faction == NO_FACTION {
                    decayed(points, decayed_ms, now_ms).0
                } else {
                    points
                };
                influence[index] = influence_percent(left);
            }
            Ok(TerritoryConflict {
                territory: narrow(i64::from(territory), TERRITORY_FIELD)?,
                influence,
                race_start_ms: from_column(race, TIME_FIELD)?,
                conflict_faction,
                conflict_start_ms: from_column(start, TIME_FIELD)?,
                lottery_end_ms: from_column(lottery, TIME_FIELD)?,
            })
        })
        .collect()
}

/// Locks the conflict row of `territory`, making it with a race from `now_ms` when it has none;
/// its conflict faction and start and its lottery end.
async fn lock_conflict(
    transaction: &mut Transaction<'_, Postgres>,
    territory: i16,
    now_ms: i64,
) -> Result<(i16, i64, i64), DbError> {
    sqlx::query(
        "INSERT INTO territory_conflict (territory_id, race_start_ms) VALUES ($1, $2) \
         ON CONFLICT (territory_id) DO NOTHING",
    )
    .bind(territory)
    .bind(now_ms)
    .execute(&mut **transaction)
    .await?;
    Ok(sqlx::query_as(
        "SELECT conflict_faction, conflict_start_ms, lottery_end_ms FROM territory_conflict \
         WHERE territory_id = $1 FOR UPDATE",
    )
    .bind(territory)
    .fetch_one(&mut **transaction)
    .await?)
}

/// Gives the faction `faction` of the player `character` `points` influence in `territory` at
/// Unix ms `now_ms`, and the same to the player's company when it is in one; the points stop at
/// [`MAX_INFLUENCE`]. False when the territory is in a conflict (no gain then).
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails and [`DbError::OutOfRange`] for a value a
/// column cannot hold.
pub async fn add_influence(
    world: &PgPool,
    territory: u16,
    faction: u8,
    character: Uuid,
    points: u64,
    now_ms: u64,
) -> Result<bool, DbError> {
    let territory = narrow::<i16>(i64::from(territory), TERRITORY_FIELD)?;
    let now = to_column(now_ms, TIME_FIELD)?;
    let mut transaction = world.begin().await?;
    let (conflict, _, _) = lock_conflict(&mut transaction, territory, now).await?;
    if conflict != i16::from(NO_FACTION) {
        return Ok(false);
    }
    let stored: Option<(i64, i64)> = sqlx::query_as(
        "SELECT points, decayed_ms FROM territory_influence \
         WHERE territory_id = $1 AND faction = $2 FOR UPDATE",
    )
    .bind(territory)
    .bind(i16::from(faction))
    .fetch_optional(&mut *transaction)
    .await?;
    let (left, decayed_ms) = match stored {
        Some((stored, at)) => decayed(
            from_column(stored, POINTS_FIELD)?,
            from_column(at, TIME_FIELD)?,
            now_ms,
        ),
        None => (0, now_ms),
    };
    // Points that decayed to nothing start a new decay step with this gain.
    let decayed_ms = if left == 0 { now_ms } else { decayed_ms };
    let total = left.saturating_add(points).min(MAX_INFLUENCE);
    sqlx::query(
        "INSERT INTO territory_influence (territory_id, faction, points, decayed_ms) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (territory_id, faction) \
         DO UPDATE SET points = EXCLUDED.points, decayed_ms = EXCLUDED.decayed_ms",
    )
    .bind(territory)
    .bind(i16::from(faction))
    .bind(to_column(total, POINTS_FIELD)?)
    .bind(to_column(decayed_ms, TIME_FIELD)?)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO company_influence (territory_id, company_id, points) \
         SELECT $1, company_id, $3 FROM company_member WHERE character_id = $2 \
         ON CONFLICT (territory_id, company_id) \
         DO UPDATE SET points = company_influence.points + EXCLUDED.points",
    )
    .bind(territory)
    .bind(character)
    .bind(to_column(points, POINTS_FIELD)?)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(true)
}

/// A declaration a company asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarRequest {
    /// The territory id.
    pub territory: u16,
    /// The declaring company.
    pub company: Uuid,
    /// Its faction.
    pub faction: u8,
    /// The declaring player.
    pub character: Uuid,
    /// The company that owns the territory, as the player saw it.
    pub defender: Uuid,
    /// The war camp tier, 0 to 2.
    pub camp: u8,
    /// The cost, hundredths of a coin ([`war_cost`]).
    pub cost: u64,
    /// The company treasury pays (else the player's wallet paid already).
    pub from_treasury: bool,
}

/// Why the database refused a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarRefusal {
    /// The territory has no conflict now (`FailNoConflictState`).
    NoConflict,
    /// The conflict is of another faction.
    OtherFaction,
    /// The conflict's declarations closed (`FailPastSiegeWindow`).
    PastWindow,
    /// The defender does not own the territory (`FailDefenderNoTerritory`).
    NotTheOwner,
    /// The territory has a war without a result (`TerritoryAlreadyAtWar`).
    TerritoryAtWar,
    /// The company attacks in a war without a result (`FailAlreadyAtWar`).
    CompanyAtWar,
    /// The company declared in this conflict (`FailAlreadyInLottery`).
    AlreadyDeclared,
    /// The company earned too little influence there (`InsufficientContribution`).
    TooLittleInfluence,
    /// The treasury has less than the cost (`FailNoCurrency`).
    TooFewCoins,
}

/// What the database did with a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declaration {
    /// It is in the lottery that ends at Unix ms `lottery_end_ms`; the treasury's new balance
    /// when the treasury paid.
    InLottery {
        /// When the lottery ends.
        lottery_end_ms: u64,
        /// The treasury after the payment.
        treasury: Option<u64>,
    },
    /// It was refused; the treasury paid nothing.
    Refused(WarRefusal),
}

/// Takes the treasury change `change` of `company` for `character` with `reason` in
/// `transaction`, as `crate::change_treasury` does; the new balance, `None` when the treasury
/// has too little.
async fn treasury_change(
    transaction: &mut Transaction<'_, Postgres>,
    company: Uuid,
    character: Uuid,
    change: i64,
    reason: TreasuryReason,
) -> Result<Option<u64>, DbError> {
    let balance: Option<i64> = sqlx::query_scalar(
        "UPDATE company SET treasury = treasury + $2 \
         WHERE company_id = $1 AND treasury + $2 >= 0 RETURNING treasury",
    )
    .bind(company)
    .bind(change)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(balance) = balance else {
        return Ok(None);
    };
    sqlx::query(
        "INSERT INTO company_treasury_log (company_id, character_id, change, balance, reason) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(company)
    .bind(character)
    .bind(change)
    .bind(balance)
    .bind(reason.name())
    .execute(&mut **transaction)
    .await?;
    from_column(balance, COST_FIELD).map(Some)
}

/// Puts `request` into the lottery of its territory's conflict at Unix ms `now_ms`: the first
/// declaration of a conflict starts the lottery; the treasury pays in the same transaction when
/// it pays.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails and [`DbError::OutOfRange`] for a value a
/// column cannot hold.
pub async fn declare_in_lottery(
    world: &PgPool,
    request: &WarRequest,
    now_ms: u64,
) -> Result<Declaration, DbError> {
    let territory = narrow::<i16>(i64::from(request.territory), TERRITORY_FIELD)?;
    let now = to_column(now_ms, TIME_FIELD)?;
    let mut transaction = world.begin().await?;
    let (faction, start, lottery) = lock_conflict(&mut transaction, territory, now).await?;
    let start_ms: u64 = from_column(start, TIME_FIELD)?;
    let lottery_ms: u64 = from_column(lottery, TIME_FIELD)?;
    let refused = |refusal| Ok(Declaration::Refused(refusal));
    if faction == i16::from(NO_FACTION) {
        return refused(WarRefusal::NoConflict);
    }
    if faction != i16::from(request.faction) {
        return refused(WarRefusal::OtherFaction);
    }
    if now_ms >= conflict_end_ms(start_ms, lottery_ms) {
        return refused(WarRefusal::PastWindow);
    }
    let owner: Option<Uuid> =
        sqlx::query_scalar("SELECT company_id FROM territory_owner WHERE territory_id = $1")
            .bind(territory)
            .fetch_optional(&mut *transaction)
            .await?;
    if owner != Some(request.defender) {
        return refused(WarRefusal::NotTheOwner);
    }
    let (territory_war, company_war): (bool, bool) = sqlx::query_as(
        "SELECT \
         EXISTS (SELECT 1 FROM territory_war WHERE territory_id = $1 AND resolved_ms IS NULL), \
         EXISTS (SELECT 1 FROM territory_war WHERE attacker_company = $2 AND resolved_ms IS NULL)",
    )
    .bind(territory)
    .bind(request.company)
    .fetch_one(&mut *transaction)
    .await?;
    if territory_war {
        return refused(WarRefusal::TerritoryAtWar);
    }
    if company_war {
        return refused(WarRefusal::CompanyAtWar);
    }
    let contribution: Option<i64> = sqlx::query_scalar(
        "SELECT points FROM company_influence WHERE territory_id = $1 AND company_id = $2",
    )
    .bind(territory)
    .bind(request.company)
    .fetch_optional(&mut *transaction)
    .await?;
    if from_column::<u64>(contribution.unwrap_or_default(), POINTS_FIELD)?
        < MIN_COMPANY_CONTRIBUTION
    {
        return refused(WarRefusal::TooLittleInfluence);
    }
    let lottery_end_ms = if lottery_ms > start_ms {
        lottery_ms
    } else {
        now_ms.saturating_add(LOTTERY_LENGTH_MS)
    };
    let cost = to_column(request.cost, COST_FIELD)?;
    let inserted = sqlx::query(
        "INSERT INTO war_declaration (territory_id, conflict_start_ms, company_id, character_id, \
         defender_company, cost, war_camp, from_treasury, lottery_end_ms, war_starts_ms, \
         declared_ms) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         ON CONFLICT (territory_id, conflict_start_ms, company_id) DO NOTHING",
    )
    .bind(territory)
    .bind(start)
    .bind(request.company)
    .bind(request.character)
    .bind(request.defender)
    .bind(cost)
    .bind(narrow::<i16>(i64::from(request.camp), CAMP_FIELD)?)
    .bind(request.from_treasury)
    .bind(to_column(lottery_end_ms, TIME_FIELD)?)
    .bind(to_column(war_start_ms(start_ms), TIME_FIELD)?)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    if inserted.rows_affected() == 0 {
        return refused(WarRefusal::AlreadyDeclared);
    }
    let treasury = if request.from_treasury {
        let reason = TreasuryReason::WarDeclaration;
        let paid = treasury_change(
            &mut transaction,
            request.company,
            request.character,
            -cost,
            reason,
        )
        .await?;
        let Some(balance) = paid else {
            return refused(WarRefusal::TooFewCoins);
        };
        Some(balance)
    } else {
        None
    };
    sqlx::query("UPDATE territory_conflict SET lottery_end_ms = $2 WHERE territory_id = $1")
        .bind(territory)
        .bind(to_column(lottery_end_ms, TIME_FIELD)?)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(Declaration::InLottery {
        lottery_end_ms,
        treasury,
    })
}

/// A declaration of a drawn lottery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LotteryEntry {
    /// The declaring company.
    pub company: Uuid,
    /// The declaring player.
    pub character: Uuid,
    /// What it paid, hundredths of a coin.
    pub cost: u64,
    /// The treasury paid; its balance after the refund of a lost lottery.
    pub treasury: Option<u64>,
}

/// One drawn lottery: the war of the drawn declaration (`None` when the territory got a war
/// another way, and every declaration lost) and the declarations that get their coin back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotteryDraw {
    /// The territory id.
    pub territory: u16,
    /// The drawn declaration and its war.
    pub winner: Option<(LotteryEntry, Uuid)>,
    /// The others: a treasury that paid has its coin back already, a wallet does not.
    pub refunds: Vec<LotteryEntry>,
}

type EntryRow = (Uuid, Uuid, Uuid, i64, bool, i64);

/// Draws one lottery of `territory` and `conflict_start` in `transaction`: `pick(n)` is the drawn
/// index below `n`.
async fn draw_lottery(
    transaction: &mut Transaction<'_, Postgres>,
    territory: i16,
    conflict_start: i64,
    pick: &mut impl FnMut(usize) -> usize,
) -> Result<LotteryDraw, DbError> {
    let rows: Vec<EntryRow> = sqlx::query_as(
        "SELECT company_id, character_id, defender_company, cost, from_treasury, war_starts_ms \
         FROM war_declaration WHERE territory_id = $1 AND conflict_start_ms = $2 AND NOT drawn \
         ORDER BY declared_ms, company_id FOR UPDATE",
    )
    .bind(territory)
    .bind(conflict_start)
    .fetch_all(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE war_declaration SET drawn = true \
         WHERE territory_id = $1 AND conflict_start_ms = $2",
    )
    .bind(territory)
    .bind(conflict_start)
    .execute(&mut **transaction)
    .await?;
    let drawn = pick(rows.len()).min(rows.len().saturating_sub(1));
    let mut winner = None;
    let mut refunds = Vec::new();
    for (index, (company, character, defender, cost, from_treasury, starts)) in
        rows.into_iter().enumerate()
    {
        let mut entry = LotteryEntry {
            company,
            character,
            cost: from_column(cost, COST_FIELD)?,
            treasury: None,
        };
        if index == drawn {
            let war = Uuid::new_v4();
            let made = sqlx::query(
                "INSERT INTO territory_war (war_id, territory_id, attacker_company, \
                 defender_company, starts_ms) VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT (territory_id) WHERE resolved_ms IS NULL DO NOTHING",
            )
            .bind(war)
            .bind(territory)
            .bind(company)
            .bind(defender)
            .bind(starts)
            .execute(&mut **transaction)
            .await?;
            if made.rows_affected() > 0 {
                sqlx::query(
                    "UPDATE war_declaration SET war_id = $4 WHERE territory_id = $1 \
                     AND conflict_start_ms = $2 AND company_id = $3",
                )
                .bind(territory)
                .bind(conflict_start)
                .bind(company)
                .bind(war)
                .execute(&mut **transaction)
                .await?;
                winner = Some((entry, war));
                continue;
            }
        }
        if from_treasury {
            let reason = TreasuryReason::WarDeclarationFailed;
            entry.treasury = treasury_change(transaction, company, character, cost, reason).await?;
        }
        refunds.push(entry);
    }
    Ok(LotteryDraw {
        territory: narrow(i64::from(territory), TERRITORY_FIELD)?,
        winner,
        refunds,
    })
}

type RaceRow = (i16, i64, i16, i64, i64, Option<i16>, Option<f32>);

/// Moves every race, conflict and lottery to Unix ms `now_ms`: a race whose slot came starts its
/// conflict, a conflict whose end came resets its territory, a lottery whose end came is drawn
/// (`pick(n)` draws an index below `n`). The drawn lotteries.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails and [`DbError::OutOfRange`] for a stored
/// value the program's type cannot hold.
pub async fn advance_conflicts(
    world: &PgPool,
    now_ms: u64,
    mut pick: impl FnMut(usize) -> usize,
) -> Result<Vec<LotteryDraw>, DbError> {
    let now = to_column(now_ms, TIME_FIELD)?;
    let mut transaction = world.begin().await?;
    let races: Vec<RaceRow> = sqlx::query_as(
        "SELECT r.territory_id, r.race_start_ms, r.conflict_faction, r.conflict_start_ms, \
         r.lottery_end_ms, c.faction, c.siege_hour FROM territory_conflict r \
         LEFT JOIN territory_owner o ON o.territory_id = r.territory_id \
         LEFT JOIN company c ON c.company_id = o.company_id \
         ORDER BY r.territory_id FOR UPDATE OF r",
    )
    .fetch_all(&mut *transaction)
    .await?;
    for (territory, race, faction, start, lottery, owner_faction, siege_hour) in races {
        let start_ms: u64 = from_column(start, TIME_FIELD)?;
        if faction != i16::from(NO_FACTION) {
            let end = conflict_end_ms(start_ms, from_column(lottery, TIME_FIELD)?);
            if now_ms >= end {
                end_conflict(&mut transaction, territory, to_column(end, TIME_FIELD)?).await?;
            }
            continue;
        }
        let (Some(owner_faction), Some(siege_hour)) = (owner_faction, siege_hour) else {
            continue;
        };
        let Some(slot) = conflict_slot(from_column(race, TIME_FIELD)?, siege_hour, now_ms) else {
            continue;
        };
        if now_ms >= conflict_end_ms(slot, 0) {
            continue;
        }
        let points = frozen_points(&mut transaction, territory, now_ms).await?;
        let owner_faction = narrow::<u8>(i64::from(owner_faction), FACTION_FIELD)?;
        if let Some(leader) = conflict_leader(points, owner_faction) {
            sqlx::query(
                "UPDATE territory_conflict SET conflict_faction = $2, conflict_start_ms = $3 \
                 WHERE territory_id = $1",
            )
            .bind(territory)
            .bind(i16::from(leader))
            .bind(to_column(slot, TIME_FIELD)?)
            .execute(&mut *transaction)
            .await?;
        }
    }
    let due: Vec<(i16, i64)> = sqlx::query_as(
        "SELECT DISTINCT territory_id, conflict_start_ms FROM war_declaration \
         WHERE NOT drawn AND lottery_end_ms <= $1 ORDER BY territory_id, conflict_start_ms",
    )
    .bind(now)
    .fetch_all(&mut *transaction)
    .await?;
    let mut draws = Vec::with_capacity(due.len());
    for (territory, conflict_start) in due {
        draws.push(draw_lottery(&mut transaction, territory, conflict_start, &mut pick).await?);
    }
    transaction.commit().await?;
    Ok(draws)
}

/// Counts the decay of every faction's points in `territory` up to `now_ms` and stores it (a
/// conflict starts: the points stay as they are until it ends); the points of factions 1 to 3.
async fn frozen_points(
    transaction: &mut Transaction<'_, Postgres>,
    territory: i16,
    now_ms: u64,
) -> Result<[u64; 3], DbError> {
    let stored: Vec<(i16, i64, i64)> = sqlx::query_as(
        "SELECT faction, points, decayed_ms FROM territory_influence WHERE territory_id = $1 \
         FOR UPDATE",
    )
    .bind(territory)
    .fetch_all(&mut **transaction)
    .await?;
    let mut points = [0; 3];
    for (faction, stored, at) in stored {
        let (left, _) = decayed(
            from_column(stored, POINTS_FIELD)?,
            from_column(at, TIME_FIELD)?,
            now_ms,
        );
        let index = usize::from(narrow::<u8>(i64::from(faction), FACTION_FIELD)?)
            .saturating_sub(1)
            .min(FACTIONS.len() - 1);
        points[index] = left;
        sqlx::query(
            "UPDATE territory_influence SET points = $3, decayed_ms = $4 \
             WHERE territory_id = $1 AND faction = $2",
        )
        .bind(territory)
        .bind(faction)
        .bind(to_column(left, POINTS_FIELD)?)
        .bind(to_column(now_ms, TIME_FIELD)?)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(points)
}

/// Ends the conflict of `territory` at Unix ms `end`: no influence for any faction or company,
/// no conflict, a new race from `end`; the lottery end stays (Observed).
async fn end_conflict(
    transaction: &mut Transaction<'_, Postgres>,
    territory: i16,
    end: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE territory_influence SET points = 0, decayed_ms = $2 WHERE territory_id = $1",
    )
    .bind(territory)
    .bind(end)
    .execute(&mut **transaction)
    .await?;
    sqlx::query("DELETE FROM company_influence WHERE territory_id = $1")
        .bind(territory)
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "UPDATE territory_conflict SET conflict_faction = 0, conflict_start_ms = 0, \
         race_start_ms = $2 WHERE territory_id = $1",
    )
    .bind(territory)
    .bind(end)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up time: 2026-11-12 00:00:00 UTC.
    const MIDNIGHT_MS: u64 = 1_794_441_600_000;
    const SIEGE_HOUR: f32 = 18.0;

    #[test]
    fn the_points_lose_one_percent_each_interval_and_keep_the_rest_of_the_interval() {
        let half = DECAY_INTERVAL_MS / 2;
        assert_eq!(decayed(500, 0, half), (500, 0));
        assert_eq!(
            decayed(500, 0, 2 * DECAY_INTERVAL_MS + half),
            (500 - 2 * DECAY_POINTS, 2 * DECAY_INTERVAL_MS)
        );
        assert_eq!(
            decayed(50, 0, 10 * DECAY_INTERVAL_MS).0,
            0,
            "never below zero"
        );
    }

    #[test]
    fn the_percent_is_the_share_of_the_most_influence() {
        assert_eq!(influence_percent(0), 0);
        assert_eq!(influence_percent(DECAY_POINTS - 1), 0);
        assert_eq!(influence_percent(DECAY_POINTS), 1);
        assert_eq!(influence_percent(MAX_INFLUENCE * 2), 100);
    }

    #[test]
    fn a_conflict_starts_at_minute_45_before_the_siege_hour_after_the_shortest_race() {
        let siege = MIDNIGHT_MS + 18 * HOUR_MS;
        let slot_today = siege - CONFLICT_BEFORE_SIEGE_MS;
        // A race from midnight is 13.75 h old at 13:45 and 17.75 h old at 17:45.
        assert_eq!(conflict_slot(MIDNIGHT_MS, SIEGE_HOUR, slot_today - 1), None);
        assert_eq!(
            conflict_slot(MIDNIGHT_MS, SIEGE_HOUR, slot_today),
            Some(slot_today)
        );
        // A race from 06:00 is too young at 17:45: the slot is the next day's.
        let late_race = MIDNIGHT_MS + 6 * HOUR_MS;
        assert_eq!(
            conflict_slot(late_race, SIEGE_HOUR, slot_today + HOUR_MS),
            None
        );
        assert_eq!(
            conflict_slot(late_race, SIEGE_HOUR, slot_today + DAY_MS),
            Some(slot_today + DAY_MS)
        );
        // Two days later the slot is the latest one.
        assert_eq!(
            conflict_slot(MIDNIGHT_MS, SIEGE_HOUR, slot_today + 2 * DAY_MS + 5),
            Some(slot_today + 2 * DAY_MS)
        );
    }

    #[test]
    fn a_conflict_ends_at_its_lottery_or_after_one_hour() {
        let start = MIDNIGHT_MS;
        assert_eq!(conflict_end_ms(start, 0), start + CONFLICT_LENGTH_MS);
        let lottery = start + LOTTERY_LENGTH_MS;
        assert_eq!(conflict_end_ms(start, lottery), lottery);
        let late = start + CONFLICT_LENGTH_MS + 1;
        assert_eq!(conflict_end_ms(start, late), start + CONFLICT_LENGTH_MS);
        let old = start - 1;
        assert_eq!(
            conflict_end_ms(start, old),
            start + CONFLICT_LENGTH_MS,
            "an old lottery"
        );
    }

    #[test]
    fn the_war_is_at_the_siege_hour_of_the_next_day() {
        let slot = MIDNIGHT_MS + 18 * HOUR_MS - CONFLICT_BEFORE_SIEGE_MS;
        assert_eq!(war_start_ms(slot), MIDNIGHT_MS + DAY_MS + 18 * HOUR_MS);
        assert_eq!(war_start_ms(slot) % HOUR_MS, 0, "on the hour");
    }

    #[test]
    fn each_camp_tier_adds_the_base_cost_and_no_fourth_tier_exists() {
        assert_eq!(war_cost(0), Some(WAR_DECLARATION_COST));
        assert_eq!(war_cost(2), Some(3 * WAR_DECLARATION_COST));
        assert_eq!(war_cost(WAR_CAMP_TIERS), None);
    }

    #[test]
    fn the_leader_is_the_attacking_faction_with_the_most_points() {
        assert_eq!(conflict_leader([0, 5800, 2500], 1), Some(2));
        assert_eq!(
            conflict_leader([0, 5800, 2500], 2),
            Some(3),
            "never the owner"
        );
        assert_eq!(
            conflict_leader([0, 4000, 4000], 1),
            Some(2),
            "the first of equals"
        );
        assert_eq!(conflict_leader([0, 0, 0], 1), None);
        assert_eq!(conflict_leader([700, 0, 0], 1), None);
    }
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::set_territory_owner;
    use crate::territory::infra_tests::company;
    use crate::test_db::with_test_schema;

    const TERRITORY: u16 = 6;
    const OWNER: &str = "Made Up Owner";
    const ATTACKER: &str = "Made Up Attacker";
    const SECOND: &str = "Second Made Up Attacker";
    const OWNER_FACTION: u8 = 1;
    const ATTACKING_FACTION: u8 = 2;
    const MISSION_POINTS: u64 = 100;
    /// 2026-11-12 00:00 UTC; the test owner's siege hour is 18.
    const MIDNIGHT_MS: u64 = 1_794_441_600_000;
    const TREASURY: i64 = 1_000_000;

    async fn founder(pool: &PgPool, company: Uuid) -> Uuid {
        sqlx::query_scalar("SELECT character_id FROM company_member WHERE company_id = $1")
            .bind(company)
            .fetch_one(pool)
            .await
            .expect("founder")
    }

    async fn fund(pool: &PgPool, company: Uuid) {
        sqlx::query("UPDATE company SET treasury = $2 WHERE company_id = $1")
            .bind(company)
            .bind(TREASURY)
            .execute(pool)
            .await
            .expect("fund");
    }

    async fn treasury(pool: &PgPool, company: Uuid) -> i64 {
        sqlx::query_scalar("SELECT treasury FROM company WHERE company_id = $1")
            .bind(company)
            .fetch_one(pool)
            .await
            .expect("treasury")
    }

    fn request(company: Uuid, character: Uuid, defender: Uuid, from_treasury: bool) -> WarRequest {
        WarRequest {
            territory: TERRITORY,
            company,
            faction: ATTACKING_FACTION,
            character,
            defender,
            camp: 0,
            cost: WAR_DECLARATION_COST,
            from_treasury,
        }
    }

    #[tokio::test]
    async fn influence_makes_a_conflict_a_lottery_and_a_war_with_refunds() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let owner = company(pool, OWNER, OWNER_FACTION).await;
            let attacker = company(pool, ATTACKER, ATTACKING_FACTION).await;
            let second = company(pool, SECOND, ATTACKING_FACTION).await;
            assert!(
                set_territory_owner(pool, TERRITORY, OWNER)
                    .await
                    .expect("owner")
            );
            let (attacker_player, second_player) =
                (founder(pool, attacker).await, founder(pool, second).await);
            fund(pool, second).await;
            // The race starts with the first influence at midnight.
            for player in [attacker_player, second_player] {
                let gained = add_influence(
                    pool,
                    TERRITORY,
                    ATTACKING_FACTION,
                    player,
                    MISSION_POINTS,
                    MIDNIGHT_MS,
                )
                .await
                .expect("gain");
                assert!(gained);
            }
            let read = territory_conflicts(pool, MIDNIGHT_MS).await.expect("read");
            assert_eq!(read[0].influence, [0, 2, 0]);
            assert_eq!(read[0].race_start_ms, MIDNIGHT_MS);
            // No conflict and no declaration before the slot.
            let slot = MIDNIGHT_MS + 18 * HOUR_MS - CONFLICT_BEFORE_SIEGE_MS;
            assert!(
                advance_conflicts(pool, slot - 1, |_| 0)
                    .await
                    .expect("advance")
                    .is_empty()
            );
            let early = declare_in_lottery(
                pool,
                &request(attacker, attacker_player, owner, false),
                slot - 1,
            )
            .await
            .expect("declare");
            assert_eq!(early, Declaration::Refused(WarRefusal::NoConflict));
            // Decay takes the points away over the day: give one more mission just before.
            add_influence(
                pool,
                TERRITORY,
                ATTACKING_FACTION,
                attacker_player,
                MISSION_POINTS,
                slot - 1,
            )
            .await
            .expect("gain");
            advance_conflicts(pool, slot, |_| 0).await.expect("advance");
            let read = territory_conflicts(pool, slot).await.expect("read");
            assert_eq!(read[0].conflict_faction, ATTACKING_FACTION);
            assert_eq!(read[0].conflict_start_ms, slot);
            // No gain during a conflict.
            let gained =
                add_influence(pool, TERRITORY, ATTACKING_FACTION, attacker_player, 1, slot)
                    .await
                    .expect("gain");
            assert!(!gained);
            // The wrong defender, then two companies declare; the second pays from its treasury.
            let wrong = declare_in_lottery(
                pool,
                &request(attacker, attacker_player, attacker, false),
                slot,
            )
            .await
            .expect("declare");
            assert_eq!(wrong, Declaration::Refused(WarRefusal::NotTheOwner));
            let first = declare_in_lottery(
                pool,
                &request(attacker, attacker_player, owner, false),
                slot,
            )
            .await
            .expect("declare");
            let lottery_end_ms = slot + LOTTERY_LENGTH_MS;
            assert_eq!(
                first,
                Declaration::InLottery {
                    lottery_end_ms,
                    treasury: None
                }
            );
            let again = declare_in_lottery(
                pool,
                &request(attacker, attacker_player, owner, false),
                slot,
            )
            .await
            .expect("declare");
            assert_eq!(again, Declaration::Refused(WarRefusal::AlreadyDeclared));
            let paid =
                declare_in_lottery(pool, &request(second, second_player, owner, true), slot + 1)
                    .await
                    .expect("declare");
            let after = TREASURY as u64 - WAR_DECLARATION_COST;
            assert_eq!(
                paid,
                Declaration::InLottery {
                    lottery_end_ms,
                    treasury: Some(after)
                }
            );
            // The lottery draws the first entry: the treasury of the second gets its coin back.
            let draws = advance_conflicts(pool, lottery_end_ms, |entries| {
                assert_eq!(entries, 2);
                0
            })
            .await
            .expect("advance");
            let [draw] = draws.as_slice() else {
                panic!("one draw: {draws:?}");
            };
            let (winner, war) = draw.winner.expect("a war");
            assert_eq!(winner.company, attacker);
            assert_eq!(draw.refunds.len(), 1);
            assert_eq!(draw.refunds[0].treasury, Some(TREASURY as u64));
            assert_eq!(treasury(pool, second).await, TREASURY);
            let wars = crate::territory_wars(pool, lottery_end_ms)
                .await
                .expect("wars");
            assert_eq!(wars[0].war, war);
            assert_eq!(wars[0].starts_ms, war_start_ms(slot));
            // The conflict ended at the lottery end: influence and conflict are back at zero.
            let read = territory_conflicts(pool, lottery_end_ms)
                .await
                .expect("read");
            assert_eq!(read[0].influence, [0, 0, 0]);
            assert_eq!(read[0].conflict_faction, NO_FACTION);
            assert_eq!(read[0].race_start_ms, lottery_end_ms);
            assert_eq!(read[0].lottery_end_ms, lottery_end_ms);
            assert!(
                advance_conflicts(pool, lottery_end_ms + 1, |_| 0)
                    .await
                    .expect("again")
                    .is_empty()
            );
        })
        .await;
    }

    #[tokio::test]
    async fn a_company_without_influence_or_coin_is_refused() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let owner = company(pool, OWNER, OWNER_FACTION).await;
            let attacker = company(pool, ATTACKER, ATTACKING_FACTION).await;
            let second = company(pool, SECOND, ATTACKING_FACTION).await;
            assert!(
                set_territory_owner(pool, TERRITORY, OWNER)
                    .await
                    .expect("owner")
            );
            let attacker_player = founder(pool, attacker).await;
            let second_player = founder(pool, second).await;
            let slot = MIDNIGHT_MS + 18 * HOUR_MS - CONFLICT_BEFORE_SIEGE_MS;
            add_influence(
                pool,
                TERRITORY,
                ATTACKING_FACTION,
                attacker_player,
                MISSION_POINTS,
                MIDNIGHT_MS,
            )
            .await
            .expect("gain");
            add_influence(
                pool,
                TERRITORY,
                ATTACKING_FACTION,
                attacker_player,
                MISSION_POINTS,
                slot - 1,
            )
            .await
            .expect("gain");
            advance_conflicts(pool, slot, |_| 0).await.expect("advance");
            let none =
                declare_in_lottery(pool, &request(second, second_player, owner, false), slot)
                    .await
                    .expect("declare");
            assert_eq!(none, Declaration::Refused(WarRefusal::TooLittleInfluence));
            let poor =
                declare_in_lottery(pool, &request(attacker, attacker_player, owner, true), slot)
                    .await
                    .expect("declare");
            assert_eq!(poor, Declaration::Refused(WarRefusal::TooFewCoins));
            let past = declare_in_lottery(
                pool,
                &request(attacker, attacker_player, owner, false),
                slot + CONFLICT_LENGTH_MS,
            )
            .await
            .expect("declare");
            assert_eq!(past, Declaration::Refused(WarRefusal::PastWindow));
            // Without a declaration the conflict ends after one hour.
            assert!(
                advance_conflicts(pool, slot + CONFLICT_LENGTH_MS, |_| 0)
                    .await
                    .expect("advance")
                    .is_empty()
            );
            let read = territory_conflicts(pool, slot + CONFLICT_LENGTH_MS)
                .await
                .expect("read");
            assert_eq!(read[0].conflict_faction, NO_FACTION);
            assert_eq!(read[0].race_start_ms, slot + CONFLICT_LENGTH_MS);
        })
        .await;
    }
}
