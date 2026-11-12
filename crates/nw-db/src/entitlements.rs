// SPDX-License-Identifier: AGPL-3.0-or-later
//! The store of each account (global migration 0005): the store entitlements it holds and its
//! wallet of Marks of Fortune; and its consumable entitlements with their counts (global migration
//! 0009: the barbershop token). A store purchase pays from the wallet (nothing in a free store) and
//! adds the entitlement, or the offer's amount of a consumable; a use takes one consumable. The
//! store service lists them all, and the sims let a player use what its account holds.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;

/// One entitlement an account holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountEntitlement {
    /// The game data's `UniqueTagID`.
    pub entitlement: String,
    /// The id the store service gives the client for it.
    pub instance: Uuid,
    /// When the account got it, Unix milliseconds.
    pub granted_at_ms: i64,
}

type EntitlementRow = (String, Uuid, i64);

/// The columns of one entitlement, its time in Unix milliseconds.
const COLUMNS: &str =
    "entitlement, instance_id, CAST(EXTRACT(EPOCH FROM granted_at) * 1000 AS BIGINT)";

fn of_row((entitlement, instance, granted_at_ms): EntitlementRow) -> AccountEntitlement {
    AccountEntitlement {
        entitlement,
        instance,
        granted_at_ms,
    }
}

/// The entitlements `account` holds, oldest first.
///
/// # Errors
///
/// Returns [`DbError`] when the query fails.
pub async fn account_entitlements(
    global: &PgPool,
    account: Uuid,
) -> Result<Vec<AccountEntitlement>, DbError> {
    let rows: Vec<EntitlementRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM account_entitlement WHERE account_id = $1 \
         ORDER BY granted_at, entitlement"
    )))
    .bind(account)
    .fetch_all(global)
    .await?;
    Ok(rows.into_iter().map(of_row).collect())
}

/// An account's Marks of Fortune, the store currency.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Wallet {
    /// The balance.
    pub marks: i64,
    /// When it last changed, Unix milliseconds; 0 for an account that never had any.
    pub updated_at_ms: i64,
}

type WalletRow = (i64, i64);

/// The columns of a wallet, its time in Unix milliseconds.
const WALLET_COLUMNS: &str =
    "marks_of_fortune, CAST(EXTRACT(EPOCH FROM updated_at) * 1000 AS BIGINT)";

fn wallet_of((marks, updated_at_ms): WalletRow) -> Wallet {
    Wallet {
        marks,
        updated_at_ms,
    }
}

/// The wallet of `account`; an empty one when it never had any.
///
/// # Errors
///
/// Returns [`DbError`] when the query fails.
pub async fn wallet(global: &PgPool, account: Uuid) -> Result<Wallet, DbError> {
    let row: Option<WalletRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {WALLET_COLUMNS} FROM account_wallet WHERE account_id = $1"
    )))
    .bind(account)
    .fetch_optional(global)
    .await?;
    Ok(row.map(wallet_of).unwrap_or_default())
}

/// Adds `amount` Marks of Fortune to the wallet of `account` (a negative amount takes them) and
/// returns the wallet.
///
/// # Errors
///
/// Returns [`DbError`] when the account does not exist, the balance would go below 0, or the
/// query fails.
pub async fn give_marks(global: &PgPool, account: Uuid, amount: i64) -> Result<Wallet, DbError> {
    open_wallet(global, account).await?;
    let row: WalletRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE account_wallet SET marks_of_fortune = marks_of_fortune + $2, updated_at = now() \
         WHERE account_id = $1 RETURNING {WALLET_COLUMNS}"
    )))
    .bind(account)
    .bind(amount)
    .fetch_one(global)
    .await?;
    Ok(wallet_of(row))
}

/// Gives `account` an empty wallet when it has none.
async fn open_wallet<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    account: Uuid,
) -> Result<(), DbError> {
    sqlx::query("INSERT INTO account_wallet (account_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(account)
        .execute(executor)
        .await?;
    Ok(())
}

/// A consumable entitlement an account holds, and how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountConsumable {
    /// The game data's `UniqueTagID`.
    pub entitlement: String,
    /// How many the account holds.
    pub amount: i64,
    /// How many store purchases it made of it.
    pub purchases: i64,
    /// When the amount last changed, Unix milliseconds.
    pub updated_at_ms: i64,
}

type ConsumableRow = (String, i64, i64, i64);

/// The columns of one consumable, its time in Unix milliseconds.
const CONSUMABLE_COLUMNS: &str =
    "entitlement, amount, purchases, CAST(EXTRACT(EPOCH FROM updated_at) * 1000 AS BIGINT)";

fn consumable_of(
    (entitlement, amount, purchases, updated_at_ms): ConsumableRow,
) -> AccountConsumable {
    AccountConsumable {
        entitlement,
        amount,
        purchases,
        updated_at_ms,
    }
}

/// The consumable entitlements `account` holds, by entitlement; one it used up has amount 0.
///
/// # Errors
///
/// Returns [`DbError`] when the query fails.
pub async fn account_consumables(
    global: &PgPool,
    account: Uuid,
) -> Result<Vec<AccountConsumable>, DbError> {
    let rows: Vec<ConsumableRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CONSUMABLE_COLUMNS} FROM account_consumable WHERE account_id = $1 \
         ORDER BY entitlement"
    )))
    .bind(account)
    .fetch_all(global)
    .await?;
    Ok(rows.into_iter().map(consumable_of).collect())
}

/// `account` uses `amount` of the consumable `entitlement`: the count it holds now, or `None`
/// when it holds fewer (nothing changed).
///
/// # Errors
///
/// Returns [`DbError`] when the query fails.
pub async fn use_consumable(
    global: &PgPool,
    account: Uuid,
    entitlement: &str,
    amount: i64,
) -> Result<Option<AccountConsumable>, DbError> {
    let row: Option<ConsumableRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE account_consumable SET amount = amount - $3, updated_at = now() \
         WHERE account_id = $1 AND entitlement = $2 AND amount >= $3 \
         RETURNING {CONSUMABLE_COLUMNS}"
    )))
    .bind(account)
    .bind(entitlement)
    .bind(amount)
    .fetch_optional(global)
    .await?;
    Ok(row.map(consumable_of))
}

/// What a store purchase did; `H` is what the account holds of the offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purchase<H = AccountEntitlement> {
    /// The account paid the price and now holds the entitlement.
    Bought {
        /// The entitlement.
        held: H,
        /// The wallet after the payment.
        wallet: Wallet,
    },
    /// The account already held the entitlement; nothing was paid (never for a consumable).
    AlreadyHeld {
        /// The entitlement.
        held: H,
        /// The wallet.
        wallet: Wallet,
    },
    /// The wallet holds less than the price; nothing changed.
    TooPoor {
        /// The wallet.
        wallet: Wallet,
    },
}

/// `account` buys `entitlement` for `price` Marks of Fortune (0 for a free store), in one
/// transaction: an entitlement it holds is not bought again, and a wallet below the price buys
/// nothing.
///
/// # Errors
///
/// Returns [`DbError`] when the account does not exist or a query fails.
pub async fn buy_entitlement(
    global: &PgPool,
    account: Uuid,
    entitlement: &str,
    price: i64,
) -> Result<Purchase, DbError> {
    let mut transaction = global.begin().await?;
    let before = locked_wallet(&mut transaction, account).await?;
    let held: Option<EntitlementRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM account_entitlement WHERE account_id = $1 AND entitlement = $2"
    )))
    .bind(account)
    .bind(entitlement)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some(held) = held {
        return Ok(Purchase::AlreadyHeld {
            held: of_row(held),
            wallet: before,
        });
    }
    if before.marks < price {
        return Ok(Purchase::TooPoor { wallet: before });
    }
    let after = pay(&mut transaction, account, price).await?;
    let held: EntitlementRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO account_entitlement (account_id, entitlement, instance_id) \
         VALUES ($1, $2, $3) RETURNING {COLUMNS}"
    )))
    .bind(account)
    .bind(entitlement)
    .bind(Uuid::new_v4())
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Purchase::Bought {
        held: of_row(held),
        wallet: after,
    })
}

/// `account` buys `amount` of the consumable `entitlement` for `price` Marks of Fortune (0 for a
/// free store), in one transaction: a wallet below the price buys nothing.
///
/// # Errors
///
/// Returns [`DbError`] when the account does not exist or a query fails.
pub async fn buy_consumable(
    global: &PgPool,
    account: Uuid,
    entitlement: &str,
    amount: i64,
    price: i64,
) -> Result<Purchase<AccountConsumable>, DbError> {
    let mut transaction = global.begin().await?;
    let before = locked_wallet(&mut transaction, account).await?;
    if before.marks < price {
        return Ok(Purchase::TooPoor { wallet: before });
    }
    let after = pay(&mut transaction, account, price).await?;
    let held: ConsumableRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO account_consumable (account_id, entitlement, amount, purchases) \
         VALUES ($1, $2, $3, 1) ON CONFLICT (account_id, entitlement) \
         DO UPDATE SET amount = account_consumable.amount + $3, \
         purchases = account_consumable.purchases + 1, updated_at = now() \
         RETURNING {CONSUMABLE_COLUMNS}"
    )))
    .bind(account)
    .bind(entitlement)
    .bind(amount)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Purchase::Bought {
        held: consumable_of(held),
        wallet: after,
    })
}

/// The wallet of `account` in `transaction`, locked until it ends; an empty one is opened first.
async fn locked_wallet(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account: Uuid,
) -> Result<Wallet, DbError> {
    open_wallet(&mut **transaction, account).await?;
    let row: WalletRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {WALLET_COLUMNS} FROM account_wallet WHERE account_id = $1 FOR UPDATE"
    )))
    .bind(account)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(wallet_of(row))
}

/// Takes `price` Marks of Fortune from the wallet of `account` in `transaction`; the wallet after.
async fn pay(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account: Uuid,
    price: i64,
) -> Result<Wallet, DbError> {
    let row: WalletRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE account_wallet SET marks_of_fortune = marks_of_fortune - $2, updated_at = now() \
         WHERE account_id = $1 RETURNING {WALLET_COLUMNS}"
    )))
    .bind(account)
    .bind(price)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(wallet_of(row))
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::account::create_password_account;
    use crate::migrate::GLOBAL_MIGRATOR;
    use crate::test_db::with_test_schema;

    const BEAR: &str = "Ent_Mount_MTX_7_bear_Ronin";
    const WOLF: &str = "Ent_Mount_MTX_1_wolf";
    /// A retail mount price, and a wallet that buys one mount and not two.
    const PRICE: i64 = 20_000;
    const GIVEN: i64 = 30_000;

    #[tokio::test]
    async fn a_purchase_pays_once_and_a_short_wallet_buys_nothing() {
        let (empty, given, bought, again, poor, free, listed, taken, below) =
            with_test_schema(async |pool| {
                GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
                let account = create_password_account(pool, "rider", "$argon2id$x")
                    .await
                    .expect("account");
                let empty = wallet(pool, account.id).await.expect("empty");
                let given = give_marks(pool, account.id, GIVEN).await.expect("give");
                let bought = buy_entitlement(pool, account.id, BEAR, PRICE)
                    .await
                    .expect("buy");
                let again = buy_entitlement(pool, account.id, BEAR, PRICE)
                    .await
                    .expect("again");
                let poor = buy_entitlement(pool, account.id, WOLF, PRICE)
                    .await
                    .expect("poor");
                let free = buy_entitlement(pool, account.id, WOLF, 0)
                    .await
                    .expect("free");
                let listed = account_entitlements(pool, account.id).await.expect("list");
                let taken = give_marks(pool, account.id, -1).await.expect("take");
                let below = give_marks(pool, account.id, -GIVEN).await.is_err();
                (
                    empty, given, bought, again, poor, free, listed, taken, below,
                )
            })
            .await;
        assert_eq!(empty, Wallet::default());
        assert_eq!(given.marks, GIVEN);
        let Purchase::Bought { held, wallet } = bought else {
            panic!("bought: {bought:?}");
        };
        assert_eq!(
            (held.entitlement.as_str(), wallet.marks),
            (BEAR, GIVEN - PRICE)
        );
        assert!(matches!(
            again,
            Purchase::AlreadyHeld { held: ref again, wallet } if *again == held && wallet.marks == GIVEN - PRICE
        ));
        assert!(matches!(poor, Purchase::TooPoor { wallet } if wallet.marks == GIVEN - PRICE));
        assert!(matches!(free, Purchase::Bought { wallet, .. } if wallet.marks == GIVEN - PRICE));
        let names: Vec<&str> = listed.iter().map(|e| e.entitlement.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&BEAR) && names.contains(&WOLF));
        assert_eq!(taken.marks, GIVEN - PRICE - 1);
        assert!(below, "a wallet never goes below 0");
    }

    #[tokio::test]
    async fn a_consumable_adds_up_and_a_use_takes_one_until_none_is_left() {
        const TOKEN: &str = "Ent_Barbershop_Token";
        const TOKEN_PRICE: i64 = 10_000;
        let (none, bought, again, poor, used, refused, listed) = with_test_schema(async |pool| {
            GLOBAL_MIGRATOR.run(pool).await.expect("migrate");
            let account = create_password_account(pool, "barber", "$argon2id$x")
                .await
                .expect("account");
            let none = use_consumable(pool, account.id, TOKEN, 1)
                .await
                .expect("none held");
            let bought = buy_consumable(pool, account.id, TOKEN, 1, 0)
                .await
                .expect("buy");
            let again = buy_consumable(pool, account.id, TOKEN, 1, 0)
                .await
                .expect("again");
            let poor = buy_consumable(pool, account.id, TOKEN, 1, TOKEN_PRICE)
                .await
                .expect("poor");
            let used = use_consumable(pool, account.id, TOKEN, 2)
                .await
                .expect("use");
            let refused = use_consumable(pool, account.id, TOKEN, 1)
                .await
                .expect("used up");
            let listed = account_consumables(pool, account.id).await.expect("list");
            (none, bought, again, poor, used, refused, listed)
        })
        .await;
        assert_eq!(none, None, "an account with no row holds none");
        assert!(matches!(bought, Purchase::Bought { ref held, .. } if held.amount == 1));
        assert!(matches!(
            again,
            Purchase::Bought { ref held, .. } if (held.amount, held.purchases) == (2, 2)
        ));
        assert!(matches!(poor, Purchase::TooPoor { wallet } if wallet.marks == 0));
        assert_eq!(used.map(|held| held.amount), Some(0));
        assert_eq!(refused, None, "no use below 0");
        assert_eq!(
            listed
                .iter()
                .map(|held| (held.entitlement.as_str(), held.amount))
                .collect::<Vec<_>>(),
            [(TOKEN, 0)]
        );
    }
}
