// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a character had at its last save besides its progression (world migration 0034): its
//! vitals, running ability cooldowns, PvP flag wish, puzzle-stone turns and fishing bait. It is
//! part of the progression ([`crate::SavedProgression`]): it loads and saves with it, in the
//! same transaction.

use std::collections::BTreeMap;

use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::DbError;
use crate::progression::{from_column, to_column};

/// A character's health, mana and stamina; mana and stamina in thousandths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SavedVitals {
    /// The health; 0 for a character that was dead.
    pub health: u32,
    /// The mana, in thousandths.
    pub mana_milli: u32,
    /// The stamina, in thousandths.
    pub stamina_milli: u32,
}

/// One running ability cooldown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedAbilityCooldown {
    /// The CRC-32 of the lowercase ability id.
    pub ability: u32,
    /// The key (0 to 2) of the press that started it.
    pub key: u8,
    /// When it started, microseconds since 2000-01-01.
    pub start_us: u64,
    /// When it ends, microseconds since 2000-01-01.
    pub end_us: u64,
}

/// The state of a character at its last save besides its progression.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedLogOutState {
    /// Its vitals; `None` for a character saved before migration 0034 (it comes back full).
    pub vitals: Option<SavedVitals>,
    /// Its running ability cooldowns, by ability.
    pub ability_cooldowns: Vec<SavedAbilityCooldown>,
    /// It wants its PvP flag on: the flag was on or on its way on.
    pub pvp_wanted: bool,
    /// Its puzzle-stone turns, by lowercase stone id.
    pub cyclic_turns: BTreeMap<String, u32>,
    /// The bait on its fishing pole: its item id.
    pub fishing_bait: Option<String>,
}

type StateRow = (
    Option<i64>,
    Option<i64>,
    Option<i64>,
    bool,
    Json<BTreeMap<String, u32>>,
    Option<String>,
);

/// The log out state of a character that exists.
pub(crate) async fn character_log_out_state(
    world: &PgPool,
    character_id: Uuid,
) -> Result<SavedLogOutState, DbError> {
    let row: Option<StateRow> = sqlx::query_as(
        "SELECT health, mana_milli, stamina_milli, pvp_wanted, cyclic_turns, fishing_bait \
         FROM player_character WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_optional(world)
    .await?;
    let (health, mana, stamina, pvp_wanted, Json(cyclic_turns), fishing_bait) =
        row.ok_or(DbError::NoSuchCharacter)?;
    let vitals = match (health, mana, stamina) {
        (Some(health), Some(mana), Some(stamina)) => Some(SavedVitals {
            health: from_column(health, "health")?,
            mana_milli: from_column(mana, "mana_milli")?,
            stamina_milli: from_column(stamina, "stamina_milli")?,
        }),
        _ => None,
    };
    let cooldowns: Vec<(i64, i16, i64, i64)> = sqlx::query_as(
        "SELECT ability, press_key, start_us, end_us FROM character_ability_cooldown \
         WHERE character_id = $1 ORDER BY ability",
    )
    .bind(character_id)
    .fetch_all(world)
    .await?;
    Ok(SavedLogOutState {
        vitals,
        ability_cooldowns: cooldowns
            .into_iter()
            .map(|(ability, key, start_us, end_us)| {
                Ok(SavedAbilityCooldown {
                    ability: from_column(ability, "ability cooldown ability")?,
                    key: from_column(i64::from(key), "ability cooldown key")?,
                    start_us: from_column(start_us, "ability cooldown start")?,
                    end_us: from_column(end_us, "ability cooldown end")?,
                })
            })
            .collect::<Result<_, DbError>>()?,
        pvp_wanted,
        cyclic_turns,
        fishing_bait,
    })
}

/// Saves `state` as the log out state of a character whose progression row the transaction
/// already updated: the stored cooldown rows become exactly these.
pub(crate) async fn save_character_log_out_state(
    transaction: &mut Transaction<'_, Postgres>,
    character_id: Uuid,
    state: &SavedLogOutState,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE player_character SET health = $2, mana_milli = $3, stamina_milli = $4, \
         pvp_wanted = $5, cyclic_turns = $6, fishing_bait = $7 WHERE character_id = $1",
    )
    .bind(character_id)
    .bind(state.vitals.map(|vitals| i64::from(vitals.health)))
    .bind(state.vitals.map(|vitals| i64::from(vitals.mana_milli)))
    .bind(state.vitals.map(|vitals| i64::from(vitals.stamina_milli)))
    .bind(state.pvp_wanted)
    .bind(Json(&state.cyclic_turns))
    .bind(state.fishing_bait.as_deref())
    .execute(&mut **transaction)
    .await?;
    sqlx::query("DELETE FROM character_ability_cooldown WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut **transaction)
        .await?;
    for cooldown in &state.ability_cooldowns {
        sqlx::query(
            "INSERT INTO character_ability_cooldown \
             (character_id, ability, press_key, start_us, end_us) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(character_id)
        .bind(i64::from(cooldown.ability))
        .bind(i16::from(cooldown.key))
        .bind(to_column(cooldown.start_us, "ability cooldown start")?)
        .bind(to_column(cooldown.end_us, "ability cooldown end")?)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}
