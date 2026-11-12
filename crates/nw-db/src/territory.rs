// SPDX-License-Identifier: AGPL-3.0-or-later
//! The company owners of the territory claims (world migration 0038) and their upkeep (world
//! migration 0039). Every sim reads them for the land claim manager state and the governor's
//! desk its clients get; a game master's command sets and clears owners; the social service pays
//! the upkeep from the owner's treasury ([`crate::change_treasury`] with
//! [`crate::TreasuryReason::Upkeep`]).

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};
use crate::social_graph::{CREST_LEN, fixed, narrow};

const TERRITORY_FIELD: &str = "territory id";
const FACTION_FIELD: &str = "faction";
const CREST_FIELD: &str = "crest";
const SINCE_FIELD: &str = "territory owner since";
const UPKEEP_DUE_FIELD: &str = "territory upkeep due";
const CLAIMS_FIELD: &str = "territory claim count";
const NOW_FIELD: &str = "upkeep payment time";

/// One upkeep payment pays this much time (Observed: the retail payment moved the due time of
/// the desk's governance data by exactly 604,800 s, `pay-territory-tax` 1783335287246).
pub const UPKEEP_INTERVAL_MS: u64 = 604_800_000;
/// How long before the due time a payment is taken (Inferred: the governance data's second
/// time is 5 days before the due time in every retail body, and the one retail payment came
/// after it).
pub const UPKEEP_PAYABLE_BEFORE_MS: u64 = 432_000_000;

/// The company that owns a territory claim, with what the claim shows of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerritoryOwner {
    /// The territory id of the claim.
    pub territory: u16,
    /// The company id.
    pub company: Uuid,
    /// The company name.
    pub name: String,
    /// The company's faction.
    pub faction: u8,
    /// The company's crest bytes.
    pub crest: [u8; CREST_LEN],
    /// Since when it owns the claim, Unix milliseconds.
    pub since_ms: u64,
    /// When its upkeep is due, Unix milliseconds.
    pub upkeep_due_ms: u64,
}

impl TerritoryOwner {
    /// From when the next upkeep payment is taken, Unix milliseconds.
    pub const fn upkeep_payable_ms(&self) -> u64 {
        self.upkeep_due_ms.saturating_sub(UPKEEP_PAYABLE_BEFORE_MS)
    }
}

type OwnerRow = (i16, Uuid, String, i16, Vec<u8>, i64, i64);

fn owner_of(row: OwnerRow) -> Result<TerritoryOwner, DbError> {
    let (territory, company, name, faction, crest, since_ms, upkeep_due_ms) = row;
    Ok(TerritoryOwner {
        territory: narrow(i64::from(territory), TERRITORY_FIELD)?,
        company,
        name,
        faction: narrow(i64::from(faction), FACTION_FIELD)?,
        crest: fixed(crest, CREST_FIELD)?,
        since_ms: from_column(since_ms, SINCE_FIELD)?,
        upkeep_due_ms: from_column(upkeep_due_ms, UPKEEP_DUE_FIELD)?,
    })
}

/// Every owned claim of the world, by territory id.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails and [`DbError::OutOfRange`] for a stored
/// value the program's type cannot hold.
pub async fn territory_owners(world: &PgPool) -> Result<Vec<TerritoryOwner>, DbError> {
    let rows: Vec<OwnerRow> = sqlx::query_as(
        "SELECT o.territory_id, o.company_id, c.name, c.faction, c.crest, \
         (extract(epoch FROM o.since) * 1000)::bigint, o.upkeep_due_ms \
         FROM territory_owner o JOIN company c ON c.company_id = o.company_id \
         ORDER BY o.territory_id",
    )
    .fetch_all(world)
    .await?;
    rows.into_iter().map(owner_of).collect()
}

/// Gives the claim of `territory` to the company named `company` (any case) from now on, in place
/// of its owner; false when no company has that name. A claim the company already owns keeps its
/// time and its upkeep; a new owner's first upkeep is due [`UPKEEP_INTERVAL_MS`] from now
/// (Inferred).
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails.
pub async fn set_territory_owner(
    world: &PgPool,
    territory: u16,
    company: &str,
) -> Result<bool, DbError> {
    let set = sqlx::query(
        "INSERT INTO territory_owner (territory_id, company_id, upkeep_due_ms) \
         SELECT $1, company_id, (extract(epoch FROM now()) * 1000)::bigint + $3 \
         FROM company WHERE lower(name) = lower($2) \
         ON CONFLICT (territory_id) DO UPDATE SET company_id = excluded.company_id, \
         since = CASE WHEN territory_owner.company_id = excluded.company_id \
         THEN territory_owner.since ELSE now() END, \
         upkeep_due_ms = CASE WHEN territory_owner.company_id = excluded.company_id \
         THEN territory_owner.upkeep_due_ms ELSE excluded.upkeep_due_ms END",
    )
    .bind(narrow::<i16>(i64::from(territory), TERRITORY_FIELD)?)
    .bind(company)
    .bind(to_column(UPKEEP_INTERVAL_MS, UPKEEP_DUE_FIELD)?)
    .execute(world)
    .await?;
    Ok(set.rows_affected() > 0)
}

/// Takes the claim of `territory` from its owner; false when it had none.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails.
pub async fn clear_territory_owner(world: &PgPool, territory: u16) -> Result<bool, DbError> {
    let cleared = sqlx::query("DELETE FROM territory_owner WHERE territory_id = $1")
        .bind(narrow::<i16>(i64::from(territory), TERRITORY_FIELD)?)
        .execute(world)
        .await?;
    Ok(cleared.rows_affected() > 0)
}

/// How many claims each company owns, for the company list (`m_numClaims`).
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails and [`DbError::OutOfRange`] for a count
/// the program's type cannot hold.
pub async fn territory_claim_counts(world: &PgPool) -> Result<Vec<(Uuid, u32)>, DbError> {
    let rows: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT company_id, count(*) FROM territory_owner GROUP BY company_id ORDER BY company_id",
    )
    .fetch_all(world)
    .await?;
    rows.into_iter()
        .map(|(company, claims)| Ok((company, narrow(claims, CLAIMS_FIELD)?)))
        .collect()
}

/// Moves the upkeep of the claim of `territory` that `company` owns by [`UPKEEP_INTERVAL_MS`]
/// in `transaction`, when a payment is taken at Unix ms `now_ms` (from
/// [`TerritoryOwner::upkeep_payable_ms`]); the new due time, or `None` when the company does not
/// own the claim or its upkeep is not payable yet.
pub(crate) async fn move_upkeep_due(
    transaction: &mut Transaction<'_, Postgres>,
    company: Uuid,
    territory: u16,
    now_ms: u64,
) -> Result<Option<u64>, DbError> {
    let due: Option<i64> = sqlx::query_scalar(
        "UPDATE territory_owner SET upkeep_due_ms = upkeep_due_ms + $3 \
         WHERE territory_id = $1 AND company_id = $2 AND upkeep_due_ms - $4 <= $5 \
         RETURNING upkeep_due_ms",
    )
    .bind(narrow::<i16>(i64::from(territory), TERRITORY_FIELD)?)
    .bind(company)
    .bind(to_column(UPKEEP_INTERVAL_MS, UPKEEP_DUE_FIELD)?)
    .bind(to_column(UPKEEP_PAYABLE_BEFORE_MS, UPKEEP_DUE_FIELD)?)
    .bind(to_column(now_ms, NOW_FIELD)?)
    .fetch_optional(&mut **transaction)
    .await?;
    due.map(|due| from_column(due, UPKEEP_DUE_FIELD))
        .transpose()
}

#[cfg(all(test, feature = "infra"))]
pub(crate) mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::social_graph::{
        Company, TreasuryReason, change_treasury, create_company, delete_company,
    };
    use crate::test_db::with_test_schema;
    use crate::{NewCharacter, insert_character};

    /// Made-up territory ids and companies.
    const TERRITORY: u16 = 6;
    const OTHER_TERRITORY: u16 = 12;
    const FIRST: &str = "Made Up Company";
    const SECOND: &str = "Another Made Up Company";
    const CREST: [u8; CREST_LEN] = [7; CREST_LEN];

    pub(crate) async fn company(pool: &PgPool, name: &str, faction: u8) -> Uuid {
        let founder = Uuid::new_v4();
        insert_character(
            pool,
            &NewCharacter {
                id: founder,
                account_id: Uuid::new_v4(),
                name: format!("{name} founder"),
                appearance: serde_json::json!({}),
            },
        )
        .await
        .expect("insert character");
        let company = Company {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            faction,
            crest: CREST,
            motd: String::new(),
            motd_author: None,
            motd_at_ms: None,
            siege_hour: 18.0,
            treasury: 0,
            withdrawal_limit: None,
            renames: 0,
            crest_changes: 0,
        };
        assert!(
            create_company(pool, &company, founder)
                .await
                .expect("create")
        );
        company.id
    }

    #[tokio::test]
    async fn an_owner_is_set_by_company_name_moved_and_cleared() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            assert!(territory_owners(pool).await.expect("owners").is_empty());
            let first = company(pool, FIRST, 2).await;
            let second = company(pool, SECOND, 3).await;
            assert!(
                !set_territory_owner(pool, TERRITORY, "No Such Company")
                    .await
                    .expect("set")
            );
            assert!(
                set_territory_owner(pool, TERRITORY, &FIRST.to_uppercase())
                    .await
                    .expect("set")
            );
            let owners = territory_owners(pool).await.expect("owners");
            let [owner] = owners.as_slice() else {
                panic!("one owner: {owners:?}");
            };
            assert_eq!(
                (
                    owner.territory,
                    owner.company,
                    owner.name.as_str(),
                    owner.faction,
                    owner.crest
                ),
                (TERRITORY, first, FIRST, 2, CREST)
            );
            assert!(owner.since_ms > 0);
            assert!(
                set_territory_owner(pool, TERRITORY, FIRST)
                    .await
                    .expect("again")
            );
            assert_eq!(
                territory_owners(pool).await.expect("owners")[0].since_ms,
                owner.since_ms,
                "the same owner keeps its time"
            );
            assert!(
                set_territory_owner(pool, TERRITORY, SECOND)
                    .await
                    .expect("move")
            );
            assert!(
                set_territory_owner(pool, OTHER_TERRITORY, SECOND)
                    .await
                    .expect("second")
            );
            let owners = territory_owners(pool).await.expect("owners");
            assert_eq!(
                owners
                    .iter()
                    .map(|owner| (owner.territory, owner.company))
                    .collect::<Vec<_>>(),
                [(TERRITORY, second), (OTHER_TERRITORY, second)]
            );
            assert!(clear_territory_owner(pool, TERRITORY).await.expect("clear"));
            assert!(
                !clear_territory_owner(pool, TERRITORY)
                    .await
                    .expect("clear again")
            );
            delete_company(pool, second).await.expect("delete");
            assert!(
                territory_owners(pool).await.expect("owners").is_empty(),
                "a deleted company loses its claims"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn an_upkeep_payment_takes_the_treasury_and_moves_the_due_time_once() {
        /// A made-up treasury and upkeep cost, hundredths of a coin.
        const TREASURY: i64 = 250_000;
        const COST: i64 = 100_000;
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let first = company(pool, FIRST, 2).await;
            let second = company(pool, SECOND, 3).await;
            let payer = Uuid::new_v4();
            for owner in [first, second] {
                change_treasury(pool, owner, payer, TREASURY, TreasuryReason::Deposit)
                    .await
                    .expect("deposit");
            }
            set_territory_owner(pool, TERRITORY, FIRST)
                .await
                .expect("set");
            let owner = territory_owners(pool).await.expect("owners")[0].clone();
            assert!(owner.upkeep_due_ms >= owner.since_ms + UPKEEP_INTERVAL_MS);
            let pay = |company, now_ms| {
                change_treasury(
                    pool,
                    company,
                    payer,
                    -COST,
                    TreasuryReason::Upkeep {
                        territory: TERRITORY,
                        now_ms,
                    },
                )
            };
            assert_eq!(
                pay(first, owner.upkeep_payable_ms() - 1)
                    .await
                    .expect("pay"),
                None,
                "not payable yet"
            );
            assert_eq!(
                pay(second, owner.upkeep_payable_ms()).await.expect("pay"),
                None,
                "not the owner"
            );
            let balance = pay(first, owner.upkeep_payable_ms())
                .await
                .expect("pay")
                .expect("paid");
            assert_eq!(i64::try_from(balance).expect("balance"), TREASURY - COST);
            let paid = territory_owners(pool).await.expect("owners")[0].clone();
            assert_eq!(paid.upkeep_due_ms, owner.upkeep_due_ms + UPKEEP_INTERVAL_MS);
            assert_eq!(
                pay(first, owner.upkeep_payable_ms()).await.expect("pay"),
                None,
                "the next week is not payable yet"
            );
            set_territory_owner(pool, TERRITORY, FIRST)
                .await
                .expect("again");
            assert_eq!(
                territory_owners(pool).await.expect("owners")[0].upkeep_due_ms,
                paid.upkeep_due_ms,
                "the same owner keeps its upkeep"
            );
            set_territory_owner(pool, OTHER_TERRITORY, FIRST)
                .await
                .expect("second claim");
            assert_eq!(
                territory_claim_counts(pool).await.expect("counts"),
                [(first, 2)]
            );
        })
        .await;
    }
}
