// SPDX-License-Identifier: AGPL-3.0-or-later
//! Removes an account for good: its characters with all they own in each world database, then
//! the account with its logins, ranks, store items and names in the global database
//! (`edge-http delete-account`).
//!
//! The world part runs first, one transaction per world ([`delete_account_from_world`]); the
//! global part runs last ([`delete_account_from_global`]). A removal that stops between two
//! parts leaves the account in the global database, so the same command finishes it.
//!
//! In a world database every table of a character's own rows names the character with a link
//! that deletes the row with the character (`ON DELETE CASCADE`). The rows without such a link
//! are removed here: the rows keyed by the account ([`WORLD_ACCOUNT_TABLES`]), the groups the
//! characters led, the company invites they sent and the reports about them. A company that
//! loses its last member is removed with its territories, wars and treasury (they link to the
//! company); a company that loses its governor gets its highest and oldest member as governor.
//! The audit rows (`economy_audit`, `company_treasury_log`, the global `admin_audit`) stay: they
//! are a record of what happened and name ids only. A company's town project keeps the id of
//! the member who started it.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;

/// The world tables whose rows are keyed by the account, not by a character.
pub const WORLD_ACCOUNT_TABLES: [&str; 2] = ["account_title", "account_skin"];
/// The company rank of a governor (`company_member.rank`).
const GOVERNOR_RANK: i16 = 0;
/// The `stat_value.scope` of a character's stats; their `entity_id` is the character id as text.
const CHARACTER_SCOPE: &str = "Character";

/// What a removal took out of one world database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemovedFromWorld {
    /// The characters the account had in the world.
    pub characters: Vec<Uuid>,
    /// Companies removed because their last member was one of those characters.
    pub companies: u64,
}

/// Removes every character of `account` from a world database, with all rows that belong to
/// the characters or to the account (module text), in one transaction.
///
/// # Errors
///
/// Returns [`DbError`] when a query fails; the world database is then unchanged.
pub async fn delete_account_from_world(
    world: &PgPool,
    account: Uuid,
) -> Result<RemovedFromWorld, DbError> {
    let mut transaction = world.begin().await?;
    let characters: Vec<Uuid> = sqlx::query_scalar(
        "SELECT character_id FROM player_character WHERE account_id = $1 \
         ORDER BY created_at, character_id",
    )
    .bind(account)
    .fetch_all(&mut *transaction)
    .await?;
    let companies: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT company_id FROM company_member WHERE character_id = ANY($1)",
    )
    .bind(&characters)
    .fetch_all(&mut *transaction)
    .await?;
    for statement in [
        "DELETE FROM social_group WHERE leader_id = ANY($1)",
        "DELETE FROM company_invite WHERE inviter_id = ANY($1)",
        "DELETE FROM player_report WHERE reported_id = ANY($1)",
        "UPDATE company SET motd_author = NULL WHERE motd_author = ANY($1)",
    ] {
        sqlx::query(statement)
            .bind(&characters)
            .execute(&mut *transaction)
            .await?;
    }
    sqlx::query("DELETE FROM player_character WHERE account_id = $1")
        .bind(account)
        .execute(&mut *transaction)
        .await?;
    let removed_companies = sqlx::query(
        "DELETE FROM company WHERE company_id = ANY($1) AND NOT EXISTS \
         (SELECT 1 FROM company_member WHERE company_member.company_id = company.company_id)",
    )
    .bind(&companies)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    sqlx::query(
        "UPDATE company_member SET rank = $2 WHERE character_id IN ( \
             SELECT DISTINCT ON (company_id) character_id FROM company_member \
             WHERE company_id = ANY($1) AND company_id NOT IN \
                 (SELECT company_id FROM company_member WHERE rank = $2) \
             ORDER BY company_id, rank, joined_at, character_id)",
    )
    .bind(&companies)
    .bind(GOVERNOR_RANK)
    .execute(&mut *transaction)
    .await?;
    for table in WORLD_ACCOUNT_TABLES {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE account_id = $1"
        )))
        .bind(account)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(RemovedFromWorld {
        characters,
        companies: removed_companies,
    })
}

/// Removes `account` from the global database in one transaction: the names and the stats of
/// `characters` (its characters of every world), then the account, and with it every row that
/// links to it (its logins, launcher sessions, admin rank, privacy settings, store items,
/// wallet, consumables and season). Returns whether the account was there.
///
/// # Errors
///
/// Returns [`DbError`] when a query fails; the global database is then unchanged.
pub async fn delete_account_from_global(
    global: &PgPool,
    account: Uuid,
    characters: &[Uuid],
) -> Result<bool, DbError> {
    let mut transaction = global.begin().await?;
    sqlx::query("DELETE FROM character_name WHERE character_id = ANY($1)")
        .bind(characters)
        .execute(&mut *transaction)
        .await?;
    let stat_entities: Vec<String> = characters.iter().map(Uuid::to_string).collect();
    sqlx::query("DELETE FROM stat_value WHERE scope = $1 AND entity_id = ANY($2)")
        .bind(CHARACTER_SCOPE)
        .bind(&stat_entities)
        .execute(&mut *transaction)
        .await?;
    let removed = sqlx::query("DELETE FROM account WHERE account_id = $1")
        .bind(account)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
    transaction.commit().await?;
    Ok(removed > 0)
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::account::{account_by_id, create_password_account};
    use crate::character::{characters_for_account, reserve_character_name};
    use crate::entitlements::give_marks;
    use crate::migrate::{GLOBAL_MIGRATOR, WORLD_MIGRATOR};
    use crate::test_db::with_test_schema;

    const WORLD_ID: Uuid = Uuid::from_u128(1);
    /// The link action of a row that goes with its character (`pg_constraint.confdeltype`).
    const CASCADE: &str = "c";
    /// World tables that name a character and keep the row after the character is gone: the
    /// audit rows, and a company's town project (it names the member who started it).
    const KEPT_CHARACTER_TABLES: [&str; 3] =
        ["economy_audit", "company_treasury_log", "territory_project"];
    const CREST: [u8; 36] = [0; 36];

    async fn character(world: &PgPool, account: Uuid, id: u128, name: &str) -> Uuid {
        let character = Uuid::from_u128(id);
        sqlx::query(
            "INSERT INTO player_character (character_id, account_id, name, appearance) \
             VALUES ($1, $2, $3, '{}'::jsonb)",
        )
        .bind(character)
        .bind(account)
        .bind(name)
        .execute(world)
        .await
        .expect("character");
        character
    }

    async fn company(world: &PgPool, id: u128, name: &str, members: &[(Uuid, i16)]) -> Uuid {
        let company = Uuid::from_u128(id);
        sqlx::query(
            "INSERT INTO company (company_id, name, faction, crest, siege_hour) \
             VALUES ($1, $2, 1, $3, 20.0)",
        )
        .bind(company)
        .bind(name)
        .bind(&CREST[..])
        .execute(world)
        .await
        .expect("company");
        for (member, rank) in members {
            sqlx::query(
                "INSERT INTO company_member (character_id, company_id, rank) VALUES ($1, $2, $3)",
            )
            .bind(member)
            .bind(company)
            .bind(rank)
            .execute(world)
            .await
            .expect("member");
        }
        company
    }

    async fn count(pool: &PgPool, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
            .fetch_one(pool)
            .await
            .expect("count")
    }

    #[tokio::test]
    async fn a_removed_account_leaves_nothing_of_its_own_in_a_world() {
        with_test_schema(async |world| {
            WORLD_MIGRATOR.run(world).await.expect("migrate");
            let (gone, stays) = (Uuid::from_u128(10), Uuid::from_u128(11));
            let first = character(world, gone, 100, "Firstgone").await;
            let second = character(world, gone, 101, "Secondgone").await;
            let other = character(world, stays, 102, "Stays").await;
            let officer = character(world, stays, 103, "Officer").await;
            // One company of the account alone, one it governs with two other members.
            let alone = company(world, 200, "Alone", &[(first, 0)]).await;
            let shared = company(world, 201, "Shared", &[(second, 0), (other, 3), (officer, 2)])
                .await;
            for (account, appearance) in [(gone, 1_i64), (stays, 2)] {
                sqlx::query("INSERT INTO account_skin (account_id, appearance) VALUES ($1, $2)")
                    .bind(account)
                    .bind(appearance)
                    .execute(world)
                    .await
                    .expect("skin");
                sqlx::query("INSERT INTO account_title (account_id, title_id) VALUES ($1, $2)")
                    .bind(account)
                    .bind(appearance)
                    .execute(world)
                    .await
                    .expect("title");
            }
            sqlx::query(
                "INSERT INTO social_group (group_id, leader_id, voice_channel) VALUES ($1, $2, 'v')",
            )
            .bind(Uuid::from_u128(300))
            .bind(first)
            .execute(world)
            .await
            .expect("group");
            sqlx::query("UPDATE company SET motd_author = $1 WHERE company_id = $2")
                .bind(second)
                .bind(shared)
                .execute(world)
                .await
                .expect("motd");

            let removed = delete_account_from_world(world, gone)
                .await
                .expect("delete");
            assert_eq!(removed.characters, [first, second]);
            assert_eq!(removed.companies, 1, "the company of the account alone");
            assert!(
                characters_for_account(world, gone)
                    .await
                    .expect("characters")
                    .is_empty()
            );
            assert_eq!(count(world, "player_character").await, 2, "the other account");
            assert_eq!(count(world, "account_skin").await, 1);
            assert_eq!(count(world, "account_title").await, 1);
            assert_eq!(count(world, "social_group").await, 0);
            let companies: Vec<Uuid> = sqlx::query_scalar("SELECT company_id FROM company")
                .fetch_all(world)
                .await
                .expect("companies");
            assert_eq!(companies, [shared], "{alone} is gone");
            let ranks: Vec<(Uuid, i16)> = sqlx::query_as(
                "SELECT character_id, rank FROM company_member ORDER BY character_id",
            )
            .fetch_all(world)
            .await
            .expect("ranks");
            assert_eq!(
                ranks,
                [(other, 3), (officer, 0)],
                "the highest member left governs"
            );
            let author: Option<Uuid> =
                sqlx::query_scalar("SELECT motd_author FROM company WHERE company_id = $1")
                    .bind(shared)
                    .fetch_one(world)
                    .await
                    .expect("author");
            assert_eq!(author, None);
            // A second run finds nothing and changes nothing.
            let again = delete_account_from_world(world, gone)
                .await
                .expect("again");
            assert_eq!(again, RemovedFromWorld::default());
            assert_eq!(count(world, "player_character").await, 2);
        })
        .await;
    }

    #[tokio::test]
    async fn a_removed_account_leaves_the_global_database_with_its_names_and_store() {
        with_test_schema(async |global| {
            GLOBAL_MIGRATOR.run(global).await.expect("migrate");
            let gone = create_password_account(global, "leaver", "$argon2id$x")
                .await
                .expect("account");
            let stays = create_password_account(global, "stayer", "$argon2id$x")
                .await
                .expect("account");
            let (mine, theirs) = (Uuid::from_u128(100), Uuid::from_u128(102));
            for (name, character) in [("Leavername", mine), ("Stayername", theirs)] {
                reserve_character_name(global, name, WORLD_ID, character)
                    .await
                    .expect("name");
                sqlx::query(
                    "INSERT INTO stat_value (namespace, stat_id, scope, entity_id, value) \
                     VALUES ('pc', 'kills', $1, $2, 3)",
                )
                .bind(CHARACTER_SCOPE)
                .bind(character.to_string())
                .execute(global)
                .await
                .expect("stat");
            }
            for account in [gone.id, stays.id] {
                give_marks(global, account, 5).await.expect("marks");
            }
            assert!(
                delete_account_from_global(global, gone.id, &[mine])
                    .await
                    .expect("delete")
            );
            assert_eq!(account_by_id(global, gone.id).await.expect("read"), None);
            assert!(
                account_by_id(global, stays.id)
                    .await
                    .expect("read")
                    .is_some()
            );
            for table in [
                "account",
                "password_login",
                "account_wallet",
                "character_name",
                "stat_value",
            ] {
                assert_eq!(
                    count(global, table).await,
                    1,
                    "{table}: the other account's"
                );
            }
            assert!(
                !delete_account_from_global(global, gone.id, &[mine])
                    .await
                    .expect("again"),
                "no account the second time"
            );
        })
        .await;
    }

    /// A new world table must go with its character or its account: a table that names a
    /// character links to it with a cascade (or is a kept audit table), and a table that names
    /// an account is in [`WORLD_ACCOUNT_TABLES`].
    #[tokio::test]
    async fn every_world_table_of_a_character_or_an_account_is_removed_with_it() {
        with_test_schema(async |world| {
            WORLD_MIGRATOR.run(world).await.expect("migrate");
            let named = |column: &'static str| async move {
                let tables: Vec<String> = sqlx::query_scalar(
                    "SELECT table_name::text FROM information_schema.columns \
                     WHERE table_schema = current_schema() AND column_name = $1 \
                     ORDER BY table_name",
                )
                .bind(column)
                .fetch_all(world)
                .await
                .expect("columns");
                tables
            };
            // The tables that go with a character: linked to it with a cascade, or to a table
            // that is (a home's items go with the home).
            let cascading: Vec<String> = sqlx::query_scalar(
                "WITH RECURSIVE linked(relation) AS ( \
                     SELECT 'player_character'::regclass::oid \
                     UNION \
                     SELECT conrelid FROM pg_constraint \
                     JOIN linked ON confrelid = linked.relation \
                     WHERE confdeltype::text = $1) \
                 SELECT relation::regclass::text FROM linked",
            )
            .bind(CASCADE)
            .fetch_all(world)
            .await
            .expect("links");
            let unlinked: Vec<String> = named("character_id")
                .await
                .into_iter()
                .filter(|table| {
                    table != "player_character"
                        && !KEPT_CHARACTER_TABLES.contains(&table.as_str())
                        && !cascading
                            .iter()
                            .any(|linked| linked.rsplit('.').next() == Some(table.as_str()))
                })
                .collect();
            assert!(
                unlinked.is_empty(),
                "tables that keep a removed character's rows: {unlinked:?}"
            );
            let by_account: Vec<String> = named("account_id")
                .await
                .into_iter()
                .filter(|table| table != "player_character")
                .collect();
            let mut removed: Vec<&str> = WORLD_ACCOUNT_TABLES.to_vec();
            removed.sort_unstable();
            assert_eq!(by_account, removed);
        })
        .await;
    }
}
