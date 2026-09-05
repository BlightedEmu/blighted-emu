// SPDX-License-Identifier: AGPL-3.0-or-later
//! PostgreSQL migrations and records (M2 spec sections 5.1 and 7).
//!
//! The global database holds accounts, logins, launcher sessions and the global character name
//! table. Each world has its own database with its characters and their saved positions. Migrations run only through
//! `edge-http migrate`; the server checks the schema version at start.

mod account;
mod character;
mod error;
mod migrate;
mod position;
mod session;

#[cfg(any(test, feature = "test-util"))]
pub mod test_db;

pub use account::{
    account_by_id, create_password_account, discord_account, password_login, set_banned,
};
pub use character::{
    character_by_id, character_name_taken, characters_for_account, insert_character,
    release_character_name, reserve_character_name,
};
pub use error::DbError;
pub use migrate::{GLOBAL_MIGRATOR, WORLD_MIGRATOR, check_schema};
pub use position::{SavedPosition, character_position, save_character_position};
pub use session::{create_launcher_session, delete_launcher_session, launcher_session_account};
pub use sqlx::PgPool;
pub use uuid::Uuid;

/// An account row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Account id.
    pub id: Uuid,
    /// Name shown to other players and in logs.
    pub display_name: String,
    /// Unix time of creation, seconds.
    pub created_at_unix: i64,
    /// A banned account cannot log in or play.
    pub banned: bool,
}

/// A character row in one world database.
#[derive(Debug, Clone, PartialEq)]
pub struct Character {
    /// Character id.
    pub id: Uuid,
    /// Owning account id (global database).
    pub account_id: Uuid,
    /// Name as the player typed it.
    pub name: String,
    /// The client's create request body, stored as received.
    pub appearance: serde_json::Value,
    /// Unix time of creation, seconds.
    pub created_at_unix: i64,
}

/// A character to insert.
#[derive(Debug, Clone, PartialEq)]
pub struct NewCharacter {
    /// New character id.
    pub id: Uuid,
    /// Owning account id.
    pub account_id: Uuid,
    /// Name as the player typed it.
    pub name: String,
    /// The client's create request body.
    pub appearance: serde_json::Value,
}
