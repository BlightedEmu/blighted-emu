// SPDX-License-Identifier: AGPL-3.0-or-later
//! Account and character records for the preservation server.
//!
//! Both tables live in the one PostgreSQL database started by `deploy/`.
//! The architecture spec also describes one database per world.
//! This milestone keeps one database. `world_id` is a column on each character.

mod account;
mod character;
mod error;
mod limits;
mod migrate;
mod password;

#[cfg(any(test, feature = "test-util"))]
pub mod test_db;

pub use account::{account_by_id, authenticate, create_account};
pub use character::{character_by_id, characters_for_account, create_character};
pub use error::DbError;
pub use limits::{
    MAX_CHARACTER_NAME_LEN, MAX_PASSWORD_LEN, MAX_USERNAME_LEN, MAX_WORLD_ID_LEN, MIN_PASSWORD_LEN,
};
pub use migrate::migrate;

/// An account row. The password hash stays in the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Account id. This server generates it.
    pub id: String,
    /// Login name.
    pub username: String,
    /// Unix time when the row was inserted, in seconds.
    pub created_at_unix: i64,
}

/// A character row in one world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Character {
    /// Character id. This server generates it.
    pub id: String,
    /// Owning account id.
    pub account_id: String,
    /// World that holds the character.
    pub world_id: String,
    /// Display name. Unique inside one world.
    pub name: String,
    /// Unix time when the row was inserted, in seconds.
    pub created_at_unix: i64,
}
