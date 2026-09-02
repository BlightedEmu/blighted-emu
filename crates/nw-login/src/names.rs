// SPDX-License-Identifier: AGPL-3.0-or-later
//! Username, password and character name rules (M2 spec sections 5.1, 5.2 and 3.3 of the game
//! routes). The character name rule is the spec default until the RE step finds the client's
//! own rule.

/// Shortest username.
pub const USERNAME_MIN_LEN: usize = 3;
/// Longest username.
pub const USERNAME_MAX_LEN: usize = 32;
/// Shortest password, in bytes.
pub const PASSWORD_MIN_BYTES: usize = 8;
/// Longest password, in bytes.
pub const PASSWORD_MAX_BYTES: usize = 128;
/// Shortest character name.
pub const CHARACTER_NAME_MIN_LEN: usize = 3;
/// Longest character name.
pub const CHARACTER_NAME_MAX_LEN: usize = 16;

/// A name or password that breaks a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    /// Username length or characters.
    #[error(
        "username must be {USERNAME_MIN_LEN} to {USERNAME_MAX_LEN} characters of a-z, 0-9 and _"
    )]
    Username,
    /// Password length.
    #[error("password must be {PASSWORD_MIN_BYTES} to {PASSWORD_MAX_BYTES} bytes")]
    Password,
    /// Character name length or characters.
    #[error("character name must be {CHARACTER_NAME_MIN_LEN} to {CHARACTER_NAME_MAX_LEN} letters")]
    CharacterName,
}

/// The stored form of a username: lower case, checked.
pub fn normalize_username(input: &str) -> Result<String, NameError> {
    let username = input.to_ascii_lowercase();
    let valid_length = (USERNAME_MIN_LEN..=USERNAME_MAX_LEN).contains(&username.len());
    let valid_characters = username
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if valid_length && valid_characters {
        Ok(username)
    } else {
        Err(NameError::Username)
    }
}

/// Checks the password length.
pub fn check_password(password: &str) -> Result<(), NameError> {
    if (PASSWORD_MIN_BYTES..=PASSWORD_MAX_BYTES).contains(&password.len()) {
        Ok(())
    } else {
        Err(NameError::Password)
    }
}

/// Checks a character name: ASCII letters only.
pub fn check_character_name(name: &str) -> Result<(), NameError> {
    let valid_length = (CHARACTER_NAME_MIN_LEN..=CHARACTER_NAME_MAX_LEN).contains(&name.len());
    if valid_length && name.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        Ok(())
    } else {
        Err(NameError::CharacterName)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_lower_cased_and_checked() {
        assert_eq!(normalize_username("Ada_99").as_deref(), Ok("ada_99"));
        assert_eq!(normalize_username("ab"), Err(NameError::Username));
        assert_eq!(normalize_username("a b c"), Err(NameError::Username));
        assert_eq!(normalize_username("ädä"), Err(NameError::Username));
        assert_eq!(
            normalize_username(&"a".repeat(USERNAME_MAX_LEN + 1)),
            Err(NameError::Username)
        );
    }

    #[test]
    fn password_length_is_checked() {
        assert!(check_password("12345678").is_ok());
        assert_eq!(check_password("1234567"), Err(NameError::Password));
        assert_eq!(
            check_password(&"p".repeat(PASSWORD_MAX_BYTES + 1)),
            Err(NameError::Password)
        );
    }

    #[test]
    fn character_names_are_letters() {
        assert!(check_character_name("Ada").is_ok());
        assert_eq!(check_character_name("Ad"), Err(NameError::CharacterName));
        assert_eq!(check_character_name("Ada1"), Err(NameError::CharacterName));
        assert_eq!(
            check_character_name(&"a".repeat(CHARACTER_NAME_MAX_LEN + 1)),
            Err(NameError::CharacterName)
        );
    }
}
