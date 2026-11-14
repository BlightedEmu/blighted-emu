// SPDX-License-Identifier: AGPL-3.0-or-later
//! The territory wars, their sign-ups and their rosters (world migration 0040). Every sim reads
//! them for the war data of the world entity and its players' war boards; a game master
//! declares a war and gives its result; the players' sign-up and roster calls change them.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};
use crate::social_graph::narrow;

const TERRITORY_FIELD: &str = "war territory id";
const NUMBER_FIELD: &str = "war number";
const TIME_FIELD: &str = "war time";
const SIDE_FIELD: &str = "war side";
const GEAR_FIELD: &str = "war gear score";
const PLACE_FIELD: &str = "war roster place";
const PERMISSION_FIELD: &str = "war permission";
const FACTION_FIELD: &str = "war faction";
const RANK_FIELD: &str = "war company rank";

/// How long a war with a result stays in the list (Observed: the retail war data removes a war
/// 180 s after its result, `Playthrough_Part_21` 1793819069152).
pub const WAR_RESULT_SHOWN_MS: u64 = 180_000;

/// How long a war runs (Observed: the running war's time is its start + 1,800 s in every war of
/// the index).
pub const WAR_LENGTH_MS: u64 = 1_800_000;

/// How long an invasion runs (Observed: the running invasion's time is its start + 1,500 s in
/// the war data of `invasion`, `Reekwater_Invasion` and `Playthrough_Part_21`, and the won
/// invasion of `invasion` ended at that time, 1590.110 s).
pub const INVASION_LENGTH_MS: u64 = 1_500_000;

/// The attacker of an invasion: no company, faction 0 (Observed: every retail invasion entry of
/// the war data carries a zero attacker company and faction).
pub const NO_ATTACKER: (Uuid, u8) = (Uuid::nil(), 0);

/// A war of a territory, as the war data shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerritoryWar {
    /// The war id.
    pub war: Uuid,
    /// The territory id.
    pub territory: u16,
    /// The attacking company and its faction; [`NO_ATTACKER`] for an invasion.
    pub attacker: (Uuid, u8),
    /// The defending company and its faction.
    pub defender: (Uuid, u8),
    /// The war's number.
    pub number: u64,
    /// When it starts, Unix ms.
    pub starts_ms: u64,
    /// Its result: the company that won and when, Unix ms; a lost invasion's winner is the nil
    /// id (Observed: zero in the retail war data).
    pub result: Option<(Uuid, u64)>,
    /// Its sign-ups, kicked players too.
    pub signups: Vec<WarSignup>,
}

impl TerritoryWar {
    /// Whether it is an invasion: a war of no attacking company.
    #[must_use]
    pub const fn is_invasion(&self) -> bool {
        self.attacker.0.is_nil()
    }

    /// When its time ends, Unix ms: [`WAR_LENGTH_MS`] or [`INVASION_LENGTH_MS`] after its start.
    #[must_use]
    pub const fn ends_ms(&self) -> u64 {
        let length = if self.is_invasion() {
            INVASION_LENGTH_MS
        } else {
            WAR_LENGTH_MS
        };
        self.starts_ms.saturating_add(length)
    }

    /// The players on its roster with their sides: the war's fighters (Observed: the roster
    /// players got the war's move, `arvaysa_ES_attacker_war_45vs45` 1783536301572).
    pub fn roster(&self) -> impl Iterator<Item = (Uuid, u8)> + '_ {
        self.signups
            .iter()
            .filter(|signup| !signup.kicked && signup.roster.place.is_some())
            .map(|signup| (signup.character, signup.side))
    }
}

/// A player's sign-up for a war.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarSignup {
    /// The character.
    pub character: Uuid,
    /// Its name.
    pub name: String,
    /// Its company and its rank there (0 governor to 3 settler), when it is in one.
    pub company: Option<(Uuid, u8)>,
    /// The side (1 attacker, 2 defender).
    pub side: u8,
    /// Its gear score at the sign-up.
    pub gear_score: u16,
    /// Its roster place and permission.
    pub roster: RosterSeat,
    /// It was kicked out of the war.
    pub kicked: bool,
}

/// A sign-up's place on the roster and its permission.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RosterSeat {
    /// The group and slot, when the player is on the roster.
    pub place: Option<(u8, u8)>,
    /// 0 member, 1 officer, 2 war leader.
    pub permission: u8,
}

/// What a declaration did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declared {
    /// The war is declared.
    Declared,
    /// No company has the attacker's name.
    NoCompany,
    /// No company owns the territory.
    NoOwner,
    /// The attacker owns the territory.
    OwnCompany,
    /// The territory already has a war without a result.
    AtWar,
}

type WarRow = (
    Uuid,
    i16,
    Option<Uuid>,
    Option<i16>,
    Uuid,
    i16,
    i64,
    i64,
    Option<Uuid>,
    Option<i64>,
);
type SignupRow = (
    Uuid,
    Uuid,
    String,
    Option<Uuid>,
    Option<i16>,
    i16,
    i16,
    Option<i16>,
    Option<i16>,
    i16,
    bool,
);

fn war_of(row: WarRow) -> Result<TerritoryWar, DbError> {
    let (
        war,
        territory,
        attacker,
        attacker_faction,
        defender,
        defender_faction,
        number,
        starts,
        winner,
        resolved,
    ) = row;
    let result = match (winner, resolved) {
        (Some(winner), Some(at)) => Some((winner, from_column(at, TIME_FIELD)?)),
        (None, Some(at)) => Some((Uuid::nil(), from_column(at, TIME_FIELD)?)),
        _ => None,
    };
    Ok(TerritoryWar {
        war,
        territory: narrow(i64::from(territory), TERRITORY_FIELD)?,
        attacker: match (attacker, attacker_faction) {
            (Some(attacker), Some(faction)) => {
                (attacker, narrow(i64::from(faction), FACTION_FIELD)?)
            }
            _ => NO_ATTACKER,
        },
        defender: (
            defender,
            narrow(i64::from(defender_faction), FACTION_FIELD)?,
        ),
        number: from_column(number, NUMBER_FIELD)?,
        starts_ms: from_column(starts, TIME_FIELD)?,
        result,
        signups: Vec::new(),
    })
}

fn signup_of(row: SignupRow) -> Result<(Uuid, WarSignup), DbError> {
    let (war, character, name, company, rank, side, gear, group, slot, permission, kicked) = row;
    let company = match (company, rank) {
        (Some(company), Some(rank)) => Some((company, narrow(i64::from(rank), RANK_FIELD)?)),
        _ => None,
    };
    let place = match (group, slot) {
        (Some(group), Some(slot)) => Some((
            narrow(i64::from(group), PLACE_FIELD)?,
            narrow(i64::from(slot), PLACE_FIELD)?,
        )),
        _ => None,
    };
    Ok((
        war,
        WarSignup {
            character,
            name,
            company,
            side: narrow(i64::from(side), SIDE_FIELD)?,
            gear_score: narrow(i64::from(gear), GEAR_FIELD)?,
            roster: RosterSeat {
                place,
                permission: narrow(i64::from(permission), PERMISSION_FIELD)?,
            },
            kicked,
        },
    ))
}

/// Every war without a result, and every war whose result came less than
/// [`WAR_RESULT_SHOWN_MS`] before Unix ms `now_ms`, with its sign-ups, by territory.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails and [`DbError::OutOfRange`] for a stored
/// value the program's type cannot hold.
pub async fn territory_wars(world: &PgPool, now_ms: u64) -> Result<Vec<TerritoryWar>, DbError> {
    let shown_from = to_column(now_ms.saturating_sub(WAR_RESULT_SHOWN_MS), TIME_FIELD)?;
    let rows: Vec<WarRow> = sqlx::query_as(
        "SELECT w.war_id, w.territory_id, w.attacker_company, a.faction, w.defender_company, \
         d.faction, w.number, w.starts_ms, w.winner_company, w.resolved_ms \
         FROM territory_war w \
         LEFT JOIN company a ON a.company_id = w.attacker_company \
         JOIN company d ON d.company_id = w.defender_company \
         WHERE w.resolved_ms IS NULL OR w.resolved_ms >= $1 ORDER BY w.territory_id, w.number",
    )
    .bind(shown_from)
    .fetch_all(world)
    .await?;
    let mut wars = rows
        .into_iter()
        .map(war_of)
        .collect::<Result<Vec<_>, _>>()?;
    let ids: Vec<Uuid> = wars.iter().map(|war| war.war).collect();
    let signups: Vec<SignupRow> = sqlx::query_as(
        "SELECT s.war_id, s.character_id, c.name, m.company_id, m.rank, s.side, s.gear_score, \
         s.roster_group, s.roster_slot, s.permission, s.kicked \
         FROM war_signup s JOIN player_character c ON c.character_id = s.character_id \
         LEFT JOIN company_member m ON m.character_id = s.character_id \
         WHERE s.war_id = ANY($1) ORDER BY s.signed, s.character_id",
    )
    .bind(&ids)
    .fetch_all(world)
    .await?;
    for row in signups {
        let (war, signup) = signup_of(row)?;
        if let Some(war) = wars.iter_mut().find(|entry| entry.war == war) {
            war.signups.push(signup);
        }
    }
    Ok(wars)
}

/// Declares a war of the company named `attacker` (any case) against the owner of the claim of
/// `territory`, starting at Unix ms `starts_ms`.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails.
pub async fn declare_war(
    world: &PgPool,
    territory: u16,
    attacker: &str,
    starts_ms: u64,
) -> Result<Declared, DbError> {
    let territory = narrow::<i16>(i64::from(territory), TERRITORY_FIELD)?;
    let mut transaction = world.begin().await?;
    let attacker: Option<Uuid> =
        sqlx::query_scalar("SELECT company_id FROM company WHERE lower(name) = lower($1)")
            .bind(attacker)
            .fetch_optional(&mut *transaction)
            .await?;
    let Some(attacker) = attacker else {
        return Ok(Declared::NoCompany);
    };
    let owner: Option<Uuid> =
        sqlx::query_scalar("SELECT company_id FROM territory_owner WHERE territory_id = $1")
            .bind(territory)
            .fetch_optional(&mut *transaction)
            .await?;
    let Some(owner) = owner else {
        return Ok(Declared::NoOwner);
    };
    if owner == attacker {
        return Ok(Declared::OwnCompany);
    }
    let declared = sqlx::query(
        "INSERT INTO territory_war (war_id, territory_id, attacker_company, defender_company, \
         starts_ms) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (territory_id) WHERE resolved_ms IS NULL DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(territory)
    .bind(attacker)
    .bind(owner)
    .bind(to_column(starts_ms, TIME_FIELD)?)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(if declared.rows_affected() > 0 {
        Declared::Declared
    } else {
        Declared::AtWar
    })
}

/// Gives the war `war` its result: `winner` won at Unix ms `at_ms`. When the attacker won, it
/// takes the territory's claim from now on, with its first upkeep due one interval later (as
/// [`crate::set_territory_owner`] gives a new owner). For an invasion the nil id as `winner` is
/// its loss (no winner is stored) and the defender its win; the claim stays with its owner.
/// False when the war had a result already.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails.
pub async fn resolve_war(
    world: &PgPool,
    war: Uuid,
    winner: Uuid,
    at_ms: u64,
) -> Result<bool, DbError> {
    let mut transaction = world.begin().await?;
    let resolved: Option<(i16, Option<Uuid>)> = sqlx::query_as(
        "UPDATE territory_war SET winner_company = NULLIF($2, $4), resolved_ms = $3 \
         WHERE war_id = $1 AND resolved_ms IS NULL \
         AND ($2 IN (attacker_company, defender_company) \
              OR ($2 = $4 AND attacker_company IS NULL)) \
         RETURNING territory_id, attacker_company",
    )
    .bind(war)
    .bind(winner)
    .bind(to_column(at_ms, TIME_FIELD)?)
    .bind(Uuid::nil())
    .fetch_optional(&mut *transaction)
    .await?;
    let Some((territory, attacker)) = resolved else {
        return Ok(false);
    };
    if attacker == Some(winner) {
        sqlx::query(
            "UPDATE territory_owner SET company_id = $2, since = now(), \
             upkeep_due_ms = (extract(epoch FROM now()) * 1000)::bigint + $3 \
             WHERE territory_id = $1",
        )
        .bind(territory)
        .bind(winner)
        .bind(to_column(crate::UPKEEP_INTERVAL_MS, TIME_FIELD)?)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(true)
}

/// Takes back the war of `territory` that has no result; false when it had none.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails.
pub async fn cancel_war(world: &PgPool, territory: u16) -> Result<bool, DbError> {
    let cancelled =
        sqlx::query("DELETE FROM territory_war WHERE territory_id = $1 AND resolved_ms IS NULL")
            .bind(narrow::<i16>(i64::from(territory), TERRITORY_FIELD)?)
            .execute(world)
            .await?;
    Ok(cancelled.rows_affected() > 0)
}

/// Signs `character` up for `side` of `war` with its gear score; a sign-up it had moves to the
/// side, off the roster. False when the player was kicked out of the war.
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails.
pub async fn sign_up_for_war(
    world: &PgPool,
    war: Uuid,
    character: Uuid,
    side: u8,
    gear_score: u16,
) -> Result<bool, DbError> {
    let signed = sqlx::query(
        "INSERT INTO war_signup (war_id, character_id, side, gear_score) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (war_id, character_id) DO UPDATE SET side = excluded.side, \
         gear_score = excluded.gear_score, roster_group = NULL, roster_slot = NULL, \
         permission = 0 WHERE NOT war_signup.kicked",
    )
    .bind(war)
    .bind(character)
    .bind(i16::from(side))
    .bind(narrow::<i16>(i64::from(gear_score), GEAR_FIELD)?)
    .execute(world)
    .await?;
    Ok(signed.rows_affected() > 0)
}

/// Takes `character` off the sign-up of `war` (and off its roster); false when it had none, or
/// was kicked (its sign-up stays, as the ban).
///
/// # Errors
///
/// Returns [`DbError::Database`] when the query fails.
pub async fn leave_war(world: &PgPool, war: Uuid, character: Uuid) -> Result<bool, DbError> {
    let left = sqlx::query(
        "DELETE FROM war_signup WHERE war_id = $1 AND character_id = $2 AND NOT kicked",
    )
    .bind(war)
    .bind(character)
    .execute(world)
    .await?;
    Ok(left.rows_affected() > 0)
}

/// The new roster seat of a sign-up of a war, or its kick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatChange {
    /// The character.
    pub character: Uuid,
    /// Its new seat.
    pub seat: RosterSeat,
    /// It is kicked out of the war (its seat is then none).
    pub kicked: bool,
}

/// Writes the roster `changes` of `war` in one transaction: the seats are first cleared, then
/// set, so a swap passes the place index. False when a sign-up is missing or a place is taken.
///
/// # Errors
///
/// Returns [`DbError::Database`] when a query fails for another reason than a taken place.
pub async fn change_roster(
    world: &PgPool,
    war: Uuid,
    changes: &[SeatChange],
) -> Result<bool, DbError> {
    let mut transaction = world.begin().await?;
    for change in changes {
        let cleared = sqlx::query(
            "UPDATE war_signup SET roster_group = NULL, roster_slot = NULL \
             WHERE war_id = $1 AND character_id = $2 AND NOT kicked",
        )
        .bind(war)
        .bind(change.character)
        .execute(&mut *transaction)
        .await?;
        if cleared.rows_affected() == 0 {
            return Ok(false);
        }
    }
    for change in changes {
        let (group, slot) = change.seat.place.map_or((None, None), |(group, slot)| {
            (Some(i16::from(group)), Some(i16::from(slot)))
        });
        let set = sqlx::query(
            "UPDATE war_signup SET roster_group = $3, roster_slot = $4, permission = $5, \
             kicked = $6 WHERE war_id = $1 AND character_id = $2",
        )
        .bind(war)
        .bind(change.character)
        .bind(group)
        .bind(slot)
        .bind(i16::from(change.seat.permission))
        .bind(change.kicked)
        .execute(&mut *transaction)
        .await;
        match set {
            Ok(_) => {}
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => return Ok(false),
            Err(error) => return Err(error.into()),
        }
    }
    transaction.commit().await?;
    Ok(true)
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::territory::infra_tests::company;
    use crate::test_db::with_test_schema;
    use crate::{NewCharacter, insert_character, set_territory_owner, territory_owners};

    /// Made-up companies, a territory, players and a start time.
    const TERRITORY: u16 = 8;
    const OWNER: &str = "Made Up Owner";
    const ATTACKER: &str = "Made Up Attacker";
    const STARTS_MS: u64 = 1_900_000_000_000;
    const GEAR_SCORE: u16 = 700;
    const SIDE_ATTACKER: u8 = 1;

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

    /// A second territory of the same owner.
    const OTHER_TERRITORY: u16 = 9;
    /// The numbers a war takes: its attacker side and its defender side.
    const WAR_SIDES: u64 = 2;

    #[tokio::test]
    async fn the_next_war_takes_its_number_after_the_last_wars_defender() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            company(pool, OWNER, 1).await;
            company(pool, ATTACKER, 2).await;
            for territory in [TERRITORY, OTHER_TERRITORY] {
                set_territory_owner(pool, territory, OWNER)
                    .await
                    .expect("owner");
                assert_eq!(
                    declare_war(pool, territory, ATTACKER, STARTS_MS)
                        .await
                        .expect("declare"),
                    Declared::Declared
                );
            }
            let wars = territory_wars(pool, STARTS_MS).await.expect("wars");
            let [first, second] = wars.as_slice() else {
                panic!("two wars: {wars:?}");
            };
            assert_eq!(second.number, first.number + WAR_SIDES);
        })
        .await;
    }

    #[tokio::test]
    async fn a_war_is_declared_signed_up_for_rostered_and_won() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let owner = company(pool, OWNER, 1).await;
            let attacker = company(pool, ATTACKER, 2).await;
            let declare = |name| declare_war(pool, TERRITORY, name, STARTS_MS);
            assert_eq!(declare(ATTACKER).await.expect("declare"), Declared::NoOwner);
            set_territory_owner(pool, TERRITORY, OWNER)
                .await
                .expect("owner");
            assert_eq!(
                declare("No Such Company").await.expect("declare"),
                Declared::NoCompany
            );
            assert_eq!(declare(OWNER).await.expect("declare"), Declared::OwnCompany);
            assert_eq!(
                declare(ATTACKER).await.expect("declare"),
                Declared::Declared
            );
            assert_eq!(declare(ATTACKER).await.expect("declare"), Declared::AtWar);
            let wars = territory_wars(pool, STARTS_MS).await.expect("wars");
            let [war] = wars.as_slice() else {
                panic!("one war: {wars:?}");
            };
            assert_eq!((war.attacker, war.defender), ((attacker, 2), (owner, 1)));
            let (first, second) = (player(pool, "Testa").await, player(pool, "Testb").await);
            for character in [first, second] {
                assert!(
                    sign_up_for_war(pool, war.war, character, SIDE_ATTACKER, GEAR_SCORE)
                        .await
                        .expect("sign up")
                );
            }
            let seat = |character, place| SeatChange {
                character,
                seat: RosterSeat {
                    place,
                    permission: 0,
                },
                kicked: false,
            };
            assert!(
                change_roster(
                    pool,
                    war.war,
                    &[seat(first, Some((0, 0))), seat(second, Some((0, 1)))]
                )
                .await
                .expect("seat")
            );
            assert!(
                !change_roster(pool, war.war, &[seat(second, Some((0, 0)))])
                    .await
                    .expect("taken"),
                "a taken place is refused"
            );
            assert!(
                change_roster(
                    pool,
                    war.war,
                    &[seat(first, Some((0, 1))), seat(second, Some((0, 0)))]
                )
                .await
                .expect("swap")
            );
            let kick = SeatChange {
                kicked: true,
                ..seat(second, None)
            };
            assert!(change_roster(pool, war.war, &[kick]).await.expect("kick"));
            assert!(
                !sign_up_for_war(pool, war.war, second, SIDE_ATTACKER, GEAR_SCORE)
                    .await
                    .expect("banned"),
                "a kicked player may not sign up again"
            );
            assert!(
                !leave_war(pool, war.war, second)
                    .await
                    .expect("kicked stays")
            );
            let signups = &territory_wars(pool, STARTS_MS).await.expect("wars")[0].signups;
            let seated = signups
                .iter()
                .find(|signup| signup.character == first)
                .expect("first");
            assert_eq!(seated.roster.place, Some((0, 1)));
            assert_eq!(seated.gear_score, GEAR_SCORE);
            assert!(leave_war(pool, war.war, first).await.expect("leave"));
            assert!(
                resolve_war(pool, war.war, attacker, STARTS_MS)
                    .await
                    .expect("win")
            );
            assert!(
                !resolve_war(pool, war.war, owner, STARTS_MS)
                    .await
                    .expect("again"),
                "one result"
            );
            assert_eq!(
                territory_owners(pool).await.expect("owners")[0].company,
                attacker
            );
            assert_eq!(
                territory_wars(pool, STARTS_MS + WAR_RESULT_SHOWN_MS)
                    .await
                    .expect("wars")
                    .len(),
                1,
                "shown until the result's end"
            );
            assert!(
                territory_wars(pool, STARTS_MS + WAR_RESULT_SHOWN_MS + 1)
                    .await
                    .expect("wars")
                    .is_empty()
            );
            assert!(
                !cancel_war(pool, TERRITORY).await.expect("cancel"),
                "no open war"
            );
        })
        .await;
    }
}
