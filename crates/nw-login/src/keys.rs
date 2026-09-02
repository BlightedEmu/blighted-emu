// SPDX-License-Identifier: AGPL-3.0-or-later
//! Random tokens and keys (M2 spec sections 3.3 and 5).
//!
//! - Launcher token and Discord state: 32 random bytes, base64url without padding.
//! - Game session key: 32 random bytes, lower-case base32 without padding (52 characters), so
//!   the same text works in a URL path and as a DNS label.

use data_encoding::{BASE32_NOPAD, BASE64URL_NOPAD};
use sha2::{Digest, Sha256};

/// Random bytes in every token and key.
pub const TOKEN_BYTES: usize = 32;
/// Length of a game session key.
pub const GAME_SESSION_KEY_LEN: usize = 52;
/// Length of a launcher token or Discord state.
pub const LAUNCHER_TOKEN_LEN: usize = 43;
/// Lower-case form of the RFC 4648 base32 alphabet.
const BASE32_LOWER_SYMBOLS: &str = "abcdefghijklmnopqrstuvwxyz234567";

/// The random source failed.
#[derive(Debug, thiserror::Error)]
#[error("random source failed")]
pub struct RandomError;

fn random_bytes() -> Result<[u8; TOKEN_BYTES], RandomError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| RandomError)?;
    Ok(bytes)
}

/// 32 random bytes as base64url text: launcher tokens, Discord login states, PKCE verifiers and the
/// random part of queue ticket ids.
pub fn new_random_token() -> Result<String, RandomError> {
    Ok(BASE64URL_NOPAD.encode(&random_bytes()?))
}

/// A new game session key.
pub fn new_game_session_key() -> Result<String, RandomError> {
    Ok(BASE32_NOPAD.encode(&random_bytes()?).to_ascii_lowercase())
}

/// True when `text` has the form of a game session key.
pub fn is_game_session_key(text: &str) -> bool {
    text.len() == GAME_SESSION_KEY_LEN
        && text
            .bytes()
            .all(|byte| BASE32_LOWER_SYMBOLS.as_bytes().contains(&byte))
}

/// True when `text` has the form of a launcher token or Discord state.
pub fn is_launcher_token(text: &str) -> bool {
    text.len() == LAUNCHER_TOKEN_LEN && BASE64URL_NOPAD.decode(text.as_bytes()).is_ok()
}

/// SHA-256 of a token, as stored in the database.
pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_tokens_are_random_base64url() {
        let first = new_random_token().expect("token");
        let second = new_random_token().expect("token");
        assert_eq!(first.len(), LAUNCHER_TOKEN_LEN);
        assert!(is_launcher_token(&first));
        assert_ne!(first, second);
        assert!(!is_launcher_token("short"));
    }

    #[test]
    fn game_session_keys_are_lower_case_base32() {
        let key = new_game_session_key().expect("key");
        assert_eq!(key.len(), GAME_SESSION_KEY_LEN);
        assert!(is_game_session_key(&key));
        assert!(!is_game_session_key(&key.to_uppercase()));
        assert!(!is_game_session_key(&key[1..]));
    }

    #[test]
    fn token_hash_is_sha256() {
        assert_eq!(
            hex::encode(token_hash("abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
