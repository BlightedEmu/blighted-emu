// SPDX-License-Identifier: AGPL-3.0-or-later
//! Errors from account and character records.

use crate::limits::{
    MAX_CHARACTER_NAME_LEN, MAX_PASSWORD_LEN, MAX_USERNAME_LEN, MAX_WORLD_ID_LEN, MIN_PASSWORD_LEN,
};

/// Failure while reading or writing an account or a character.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// The username is empty or only whitespace.
    #[error("username is empty")]
    EmptyUsername,
    /// The username is longer than [`MAX_USERNAME_LEN`] bytes.
    #[error("username is longer than {MAX_USERNAME_LEN} bytes")]
    UsernameTooLong,
    /// The username contains a space or a control character.
    #[error("username has a space or a control character")]
    UsernameInvalid,
    /// The password is shorter than [`MIN_PASSWORD_LEN`] bytes.
    #[error("password is shorter than {MIN_PASSWORD_LEN} bytes")]
    PasswordTooShort,
    /// The password is longer than [`MAX_PASSWORD_LEN`] bytes.
    #[error("password is longer than {MAX_PASSWORD_LEN} bytes")]
    PasswordTooLong,
    /// That username is already stored.
    #[error("username is already registered")]
    DuplicateUsername,
    /// The username is unknown, or the password does not match.
    #[error("credentials are not valid")]
    InvalidCredentials,
    /// The character name is empty after trimming.
    #[error("character name is empty")]
    EmptyCharacterName,
    /// The character name is longer than [`MAX_CHARACTER_NAME_LEN`] bytes.
    #[error("character name is longer than {MAX_CHARACTER_NAME_LEN} bytes")]
    CharacterNameTooLong,
    /// The character name contains a control character.
    #[error("character name has a control character")]
    CharacterNameInvalid,
    /// The world id is empty.
    #[error("world id is empty")]
    EmptyWorldId,
    /// The world id is longer than [`MAX_WORLD_ID_LEN`] bytes.
    #[error("world id is longer than {MAX_WORLD_ID_LEN} bytes")]
    WorldIdTooLong,
    /// The world id contains a space or a control character.
    #[error("world id has a space or a control character")]
    WorldIdInvalid,
    /// That name is already used in this world.
    #[error("character name is already used in this world")]
    DuplicateCharacterName,
    /// The account id is not in the database.
    #[error("account does not exist")]
    UnknownAccount,
    /// The password hasher could not make a salt or a hash.
    #[error("password hash failed")]
    PasswordHash,
    /// PostgreSQL returned an error.
    #[error("database error")]
    Database(#[source] Box<sqlx::Error>),
}

pub(crate) fn database(error: sqlx::Error) -> DbError {
    DbError::Database(Box::new(error))
}
