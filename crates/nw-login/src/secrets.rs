// SPDX-License-Identifier: AGPL-3.0-or-later
//! Secrets from environment variables (M2 spec sections 8 and 9). They use the prefix
//! `NW_SECRET_`, because every `NW_EDGE_` variable is read as a config key.

use hmac::{Hmac, KeyInit};
use nw_protocol::launcher_api::AuthMode;
use sha2::Sha256;

use crate::config::EdgeConfig;

/// Entry ticket signing key, hex.
pub const SIGNING_KEY_ENV: &str = "NW_SECRET_SIGNING_KEY";
/// Password of the global database.
pub const GLOBAL_DATABASE_PASSWORD_ENV: &str = "NW_SECRET_GLOBAL_DATABASE_PASSWORD";
/// Discord client secret (Discord mode only).
pub const DISCORD_CLIENT_SECRET_ENV: &str = "NW_SECRET_DISCORD_CLIENT_SECRET";
/// Shortest signing key, in bytes after hex decoding (review finding I2).
pub const MIN_SIGNING_KEY_BYTES: usize = 32;

/// The entry ticket signing key, kept as a ready HMAC-SHA256 state.
#[derive(Clone)]
pub struct SigningKey {
    mac: Hmac<Sha256>,
    length: usize,
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SigningKey(..)")
    }
}

impl SigningKey {
    /// Parses a hex key. Refuses keys shorter than [`MIN_SIGNING_KEY_BYTES`] and keys whose
    /// bytes are all equal (a placeholder such as all zeros).
    pub fn from_hex(text: &str) -> Result<Self, SecretError> {
        let bytes = hex::decode(text.trim()).map_err(|_| SecretError::SigningKeyNotHex)?;
        if bytes.len() < MIN_SIGNING_KEY_BYTES {
            return Err(SecretError::SigningKeyTooShort {
                length: bytes.len(),
            });
        }
        if bytes.windows(2).all(|pair| pair[0] == pair[1]) {
            return Err(SecretError::SigningKeyPlaceholder);
        }
        // HMAC accepts keys of every length, so this error cannot happen with a 32-byte key.
        let mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&bytes).map_err(|_| {
            SecretError::SigningKeyTooShort {
                length: bytes.len(),
            }
        })?;
        Ok(Self {
            mac,
            length: bytes.len(),
        })
    }

    /// Key length in bytes.
    pub fn byte_len(&self) -> usize {
        self.length
    }

    /// A fresh HMAC state with this key.
    pub(crate) fn mac(&self) -> Hmac<Sha256> {
        self.mac.clone()
    }
}

/// All secrets of one `edge-http` process.
#[derive(Debug, Clone)]
pub struct Secrets {
    /// Entry ticket signing key.
    pub signing_key: SigningKey,
    /// Global database password.
    pub global_database_password: String,
    /// World database passwords, in the order of the `[[world]]` blocks.
    pub world_database_passwords: Vec<String>,
    /// Discord client secret, in Discord mode.
    pub discord_client_secret: Option<String>,
}

/// Error from [`load_secrets`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretError {
    /// A required variable is not set or empty.
    #[error("environment variable {0} is not set")]
    Missing(String),
    /// The signing key is not hex.
    #[error("{SIGNING_KEY_ENV} is not hex")]
    SigningKeyNotHex,
    /// The signing key is too short.
    #[error("{SIGNING_KEY_ENV} has {length} bytes, at least {MIN_SIGNING_KEY_BYTES} are needed")]
    SigningKeyTooShort {
        /// Decoded length.
        length: usize,
    },
    /// The signing key is a placeholder.
    #[error("{SIGNING_KEY_ENV} is a placeholder; use `openssl rand -hex 32`")]
    SigningKeyPlaceholder,
}

/// Reads every secret that `config` needs. `lookup` returns an environment variable;
/// production passes `|name| std::env::var(name).ok()`.
pub fn load_secrets(
    config: &EdgeConfig,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Secrets, SecretError> {
    let required = |name: &str| {
        lookup(name)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| SecretError::Missing(name.to_owned()))
    };
    let signing_key = SigningKey::from_hex(&required(SIGNING_KEY_ENV)?)?;
    let global_database_password = required(GLOBAL_DATABASE_PASSWORD_ENV)?;
    let world_database_passwords = config
        .worlds
        .iter()
        .map(|world| required(&world.database_password_env))
        .collect::<Result<Vec<_>, _>>()?;
    let discord_client_secret = match config.auth.mode {
        AuthMode::Discord => Some(required(DISCORD_CLIENT_SECRET_ENV)?),
        AuthMode::Password => None,
    };
    Ok(Secrets {
        signing_key,
        global_database_password,
        world_database_passwords,
        discord_client_secret,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::config::tests::valid;

    const KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const WORLD_PASSWORD_ENV: &str = "NW_SECRET_WORLD_1_DATABASE_PASSWORD";

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn full_env() -> HashMap<String, String> {
        env(&[
            (SIGNING_KEY_ENV, KEY_HEX),
            (GLOBAL_DATABASE_PASSWORD_ENV, "global-pw"),
            (WORLD_PASSWORD_ENV, "world-pw"),
        ])
    }

    #[test]
    fn password_mode_needs_key_and_database_passwords() {
        let vars = full_env();
        let secrets = load_secrets(&valid(), |name| vars.get(name).cloned()).expect("secrets");
        assert_eq!(secrets.signing_key.byte_len(), MIN_SIGNING_KEY_BYTES);
        assert_eq!(secrets.global_database_password, "global-pw");
        assert_eq!(secrets.world_database_passwords, ["world-pw"]);
        assert!(secrets.discord_client_secret.is_none());
        assert_eq!(format!("{:?}", secrets.signing_key), "SigningKey(..)");
    }

    #[test]
    fn missing_variable_is_named() {
        let mut vars = full_env();
        vars.remove(WORLD_PASSWORD_ENV);
        assert_eq!(
            load_secrets(&valid(), |name| vars.get(name).cloned()).unwrap_err(),
            SecretError::Missing(WORLD_PASSWORD_ENV.to_owned())
        );
    }

    #[test]
    fn discord_mode_needs_the_client_secret() {
        let mut config = valid();
        config.auth.mode = AuthMode::Discord;
        let vars = full_env();
        assert_eq!(
            load_secrets(&config, |name| vars.get(name).cloned()).unwrap_err(),
            SecretError::Missing(DISCORD_CLIENT_SECRET_ENV.to_owned())
        );
    }

    #[test]
    fn weak_signing_keys_are_refused() {
        assert!(matches!(
            SigningKey::from_hex("change-me-signature-key"),
            Err(SecretError::SigningKeyNotHex)
        ));
        assert!(matches!(
            SigningKey::from_hex("0011"),
            Err(SecretError::SigningKeyTooShort { length: 2 })
        ));
        assert!(matches!(
            SigningKey::from_hex(&"00".repeat(MIN_SIGNING_KEY_BYTES)),
            Err(SecretError::SigningKeyPlaceholder)
        ));
    }
}
