// SPDX-License-Identifier: AGPL-3.0-or-later
//! Argon2id password hashes. The hash string is stored. The password is not.

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::error::DbError;
use crate::limits::{ARGON2_ITERATIONS, ARGON2_MEMORY_KIB, ARGON2_PARALLELISM, PASSWORD_SALT_LEN};

pub(crate) fn hasher() -> Result<Argon2<'static>, DbError> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        None,
    )
    .map_err(|_| DbError::PasswordHash)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

pub(crate) fn hash_password(password: &str) -> Result<String, DbError> {
    let mut salt = [0u8; PASSWORD_SALT_LEN];
    getrandom::fill(&mut salt).map_err(|_| DbError::PasswordHash)?;
    let hash = hasher()?
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|_| DbError::PasswordHash)?;
    Ok(hash.to_string())
}

pub(crate) fn password_matches(password: &str, stored: &str) -> Result<bool, DbError> {
    match hasher()?.verify_password(password.as_bytes(), stored) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
        Err(_) => Err(DbError::PasswordHash),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::limits::MIN_PASSWORD_LEN;

    const PASSWORD: &str = "password-ok";
    const OTHER_PASSWORD: &str = "password-other";

    #[test]
    fn hash_matches_the_same_password_only() {
        let stored = hash_password(PASSWORD).expect("hash");
        assert!(password_matches(PASSWORD, &stored).expect("verify"));
        assert!(!password_matches(OTHER_PASSWORD, &stored).expect("verify other"));
        assert_ne!(stored, PASSWORD);
        let again = hash_password(PASSWORD).expect("second hash");
        assert_ne!(stored, again);
    }

    #[test]
    fn argon2id_uses_the_named_costs() {
        let params = Params::new(
            ARGON2_MEMORY_KIB,
            ARGON2_ITERATIONS,
            ARGON2_PARALLELISM,
            None,
        )
        .expect("costs");
        assert_eq!(params.m_cost(), ARGON2_MEMORY_KIB);
        assert_eq!(params.t_cost(), ARGON2_ITERATIONS);
        assert_eq!(params.p_cost(), ARGON2_PARALLELISM);
        assert!(PASSWORD.len() >= MIN_PASSWORD_LEN);
    }

    #[test]
    fn malformed_hash_is_an_error() {
        let error = password_matches(PASSWORD, "not-a-hash").expect_err("malformed");
        assert!(matches!(error, DbError::PasswordHash));
    }
}
