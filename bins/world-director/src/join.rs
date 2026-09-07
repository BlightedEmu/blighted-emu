// SPDX-License-Identifier: AGPL-3.0-or-later
//! One join (M5 spec section 3.1): check the account, load the position, take an entity number,
//! and ask the owner of the cell to take the player.

use std::time::Duration;

use nw_bus::{Admit, AdmitReply, Bus, Join, MOB_ENTITY_BASE, Point, SimSlot, subject};
use nw_db::{PgPool, character_by_id, character_position, next_entity_number};
use nw_sim_core::Position;
use uuid::Uuid;

use crate::sims::Assignment;

/// The handoff count and the bundle sequence of a new player.
const NEW_PLAYER_COUNT: u64 = 0;

/// What every join needs.
#[derive(Debug, Clone)]
pub struct JoinContext {
    /// The world.
    pub world_id: Uuid,
    /// The open map level.
    pub level: String,
    /// Where characters that were never in the world appear, in turn.
    pub starts: Vec<Position>,
    /// Longest wait for the sim.
    pub admit_timeout: Duration,
    /// The world database.
    pub db: PgPool,
    /// NATS.
    pub bus: Bus,
}

/// Why a join was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JoinRefusal {
    /// The world has no such character.
    #[error("no such character")]
    NoSuchCharacter,
    /// The character belongs to another account.
    #[error("the character belongs to another account")]
    OtherAccount,
    /// The database failed.
    #[error("database: {0}")]
    Database(String),
    /// No live sim owns the player's cell.
    #[error("no sim owns the cell")]
    NoOwner,
    /// The director has no start position.
    #[error("no start position")]
    NoStart,
    /// The sim refused the player.
    #[error("the sim refused: {0}")]
    SimRefused(String),
    /// The sim did not answer in time.
    #[error("the sim did not answer: {0}")]
    SimSilent(String),
    /// The entity number is in the mob range.
    #[error("entity number {0} is in the mob range")]
    EntityRange(u64),
}

impl JoinRefusal {
    /// Metric label.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::NoSuchCharacter => "no_such_character",
            Self::OtherAccount => "other_account",
            Self::Database(_) => "database",
            Self::NoOwner => "no_owner",
            Self::NoStart => "no_start",
            Self::SimRefused(_) => "sim_refused",
            Self::SimSilent(_) => "sim_silent",
            Self::EntityRange(_) => "entity_range",
        }
    }
}

/// `entity` as a player's entity number: refused in the mob range (M6-A spec section 6.1).
pub fn player_entity(entity: u64) -> Result<u64, JoinRefusal> {
    if entity >= MOB_ENTITY_BASE {
        return Err(JoinRefusal::EntityRange(entity));
    }
    Ok(entity)
}

/// The start position of a new character with entity number `entity`: the start positions in
/// turn (M5 load spec section 6).
pub fn start_for(starts: &[Position], entity: u64) -> Option<Position> {
    let count = u64::try_from(starts.len())
        .ok()
        .filter(|count| *count > 0)?;
    // The remainder is below the length, so it fits usize.
    starts.get((entity % count) as usize).copied()
}

/// Routes `join` to the owner of the player's cell in `assignment`. Returns the sim and the
/// entity number: a new one, or the resumed one.
///
/// # Errors
///
/// Returns the [`JoinRefusal`] of the first step that fails.
pub async fn route_join(
    context: &JoinContext,
    assignment: &Assignment,
    join: &Join,
) -> Result<(SimSlot, u64), JoinRefusal> {
    let database = |error: nw_db::DbError| JoinRefusal::Database(error.to_string());
    let character = character_by_id(&context.db, join.character)
        .await
        .map_err(database)?
        .ok_or(JoinRefusal::NoSuchCharacter)?;
    if character.account_id != join.account {
        return Err(JoinRefusal::OtherAccount);
    }
    let saved = character_position(&context.db, join.character)
        .await
        .map_err(database)?;
    let (entity, handoffs, bundles_sent) = match join.resume {
        Some(resume) => (
            player_entity(resume.entity)?,
            resume.handoffs,
            resume.bundles_sent,
        ),
        None => (
            player_entity(next_entity_number(&context.db).await.map_err(database)?)?,
            NEW_PLAYER_COUNT,
            NEW_PLAYER_COUNT,
        ),
    };
    let position = match saved {
        Some(saved) if saved.level == context.level => {
            let [x, y, z] = saved.metres();
            Position { x, y, z }
        }
        _ => start_for(&context.starts, entity).ok_or(JoinRefusal::NoStart)?,
    };
    let sim = assignment
        .owner_at(position)
        .ok_or(JoinRefusal::NoOwner)?
        .clone();
    let admit = Admit {
        connection: join.connection.clone(),
        account: join.account,
        character: join.character,
        name: character.name,
        entity,
        position: Point::from(position),
        login: join.resume.is_none(),
        handoffs,
        bundles_sent,
    };
    let reply = context
        .bus
        .request::<Admit, AdmitReply>(
            subject::sim_admit(context.world_id, &sim.id),
            &admit,
            context.admit_timeout,
        )
        .await
        .map_err(|error| JoinRefusal::SimSilent(error.to_string()))?;
    match reply {
        AdmitReply::Admitted => Ok((sim, entity)),
        AdmitReply::Refused { reason } => Err(JoinRefusal::SimRefused(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_in_the_mob_range_is_refused() {
        assert_eq!(player_entity(MOB_ENTITY_BASE - 1), Ok(MOB_ENTITY_BASE - 1));
        assert_eq!(
            player_entity(MOB_ENTITY_BASE),
            Err(JoinRefusal::EntityRange(MOB_ENTITY_BASE))
        );
    }

    const STARTS: [Position; 3] = [
        Position {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        Position {
            x: 2.0,
            y: 0.0,
            z: 0.0,
        },
        Position {
            x: 3.0,
            y: 0.0,
            z: 0.0,
        },
    ];

    #[test]
    fn new_characters_take_the_start_points_in_turn() {
        let xs: Vec<f32> = (1..=6)
            .map(|entity| start_for(&STARTS, entity).map(|start| start.x))
            .collect::<Option<_>>()
            .expect("starts");
        assert_eq!(xs, vec![2.0, 3.0, 1.0, 2.0, 3.0, 1.0]);
        assert_eq!(start_for(&[], 1), None);
    }
}
