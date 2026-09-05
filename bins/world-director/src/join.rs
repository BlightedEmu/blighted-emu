// SPDX-License-Identifier: AGPL-3.0-or-later
//! One join (M5 spec section 3.1): check the account, load the position, take an entity number,
//! and ask the owner of the cell to take the player.

use std::time::Duration;

use nw_bus::{Admit, AdmitReply, Bus, Join, Point, SimSlot, subject};
use nw_db::{PgPool, character_by_id, character_position, next_entity_number};
use nw_sim_core::Position;
use uuid::Uuid;

use crate::sims::Assignment;

/// The handoff count of a new player.
const NEW_PLAYER_HANDOFFS: u64 = 0;

/// What every join needs.
#[derive(Debug, Clone)]
pub struct JoinContext {
    /// The world.
    pub world_id: Uuid,
    /// The open map level.
    pub level: String,
    /// Where a character that was never in the world appears.
    pub start: Position,
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
    /// The sim refused the player.
    #[error("the sim refused: {0}")]
    SimRefused(String),
    /// The sim did not answer in time.
    #[error("the sim did not answer: {0}")]
    SimSilent(String),
}

impl JoinRefusal {
    /// Metric label.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::NoSuchCharacter => "no_such_character",
            Self::OtherAccount => "other_account",
            Self::Database(_) => "database",
            Self::NoOwner => "no_owner",
            Self::SimRefused(_) => "sim_refused",
            Self::SimSilent(_) => "sim_silent",
        }
    }
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
    let position = match saved {
        // f64 to f32: world positions are below 2^24 metres.
        Some(saved) if saved.level == context.level => Position {
            x: saved.x as f32,
            y: saved.y as f32,
            z: saved.z as f32,
        },
        _ => context.start,
    };
    let sim = assignment
        .owner_at(position)
        .ok_or(JoinRefusal::NoOwner)?
        .clone();
    let (entity, handoffs) = match join.resume {
        Some(resume) => (resume.entity, resume.handoffs),
        None => (
            next_entity_number(&context.db).await.map_err(database)?,
            NEW_PLAYER_HANDOFFS,
        ),
    };
    let admit = Admit {
        connection: join.connection.clone(),
        account: join.account,
        character: join.character,
        entity,
        position: Point {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        login: join.resume.is_none(),
        handoffs,
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
