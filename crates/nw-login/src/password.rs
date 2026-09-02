// SPDX-License-Identifier: AGPL-3.0-or-later
//! Argon2id password hashes (M2 spec section 5.2). Hashing runs on Tokio's blocking pool behind
//! a semaphore, so it never blocks the async workers (review finding I9).

use std::sync::Arc;

use argon2::password_hash::{PasswordHasher as _, PasswordVerifier as _};
use argon2::{Algorithm, Argon2, Params, Version};
use tokio::sync::Semaphore;

/// Argon2id memory cost in KiB (the Argon2 recommended default).
pub const ARGON2_MEMORY_KIB: u32 = 19 * 1024;
/// Argon2id passes.
pub const ARGON2_ITERATIONS: u32 = 2;
/// Argon2id lanes.
pub const ARGON2_LANES: u32 = 1;
/// Salt length in bytes.
pub const PASSWORD_SALT_LEN: usize = 16;
/// Text of the hash that unknown users are checked against.
const DUMMY_PASSWORD: &str = "dummy password for unknown users";

/// Error from hashing.
#[derive(Debug, thiserror::Error)]
pub enum PasswordError {
    /// The salt could not be made, or Argon2 refused the input.
    #[error("password hash failed")]
    Hash,
    /// A stored hash is not a valid Argon2 hash string.
    #[error("stored password hash is not valid")]
    StoredHash,
    /// The blocking worker stopped.
    #[error("password worker stopped")]
    Worker,
}

/// Runs Argon2id with a limit on parallel hashes.
#[derive(Debug, Clone)]
pub struct PasswordHasher {
    permits: Arc<Semaphore>,
    /// Hash of a fixed text. An unknown user is checked against it, so that an unknown user and
    /// a wrong password take the same time.
    dummy_hash: Arc<str>,
}

fn argon2() -> Result<Argon2<'static>, PasswordError> {
    let params = Params::new(ARGON2_MEMORY_KIB, ARGON2_ITERATIONS, ARGON2_LANES, None)
        .map_err(|_| PasswordError::Hash)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

fn hash_now(password: &str) -> Result<String, PasswordError> {
    let mut salt = [0_u8; PASSWORD_SALT_LEN];
    getrandom::fill(&mut salt).map_err(|_| PasswordError::Hash)?;
    let hash = argon2()?
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|_| PasswordError::Hash)?;
    Ok(hash.to_string())
}

fn verify_now(password: &str, stored: &str) -> Result<bool, PasswordError> {
    match argon2()?.verify_password(password.as_bytes(), stored) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
        Err(_) => Err(PasswordError::StoredHash),
    }
}

impl PasswordHasher {
    /// A hasher that runs at most `parallel` hashes at once (0 = the number of CPUs).
    pub fn new(parallel: usize) -> Result<Self, PasswordError> {
        let parallel = if parallel == 0 {
            std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
        } else {
            parallel
        };
        Ok(Self {
            permits: Arc::new(Semaphore::new(parallel)),
            dummy_hash: hash_now(DUMMY_PASSWORD)?.into(),
        })
    }

    /// Hashes a new password.
    pub async fn hash(&self, password: String) -> Result<String, PasswordError> {
        self.run(move || hash_now(&password)).await
    }

    /// Checks `password` against `stored`. With `None` (unknown user) it does the same work
    /// against a dummy hash and returns false.
    pub async fn verify(
        &self,
        password: String,
        stored: Option<String>,
    ) -> Result<bool, PasswordError> {
        let known = stored.is_some();
        let stored = stored.unwrap_or_else(|| self.dummy_hash.to_string());
        let matches = self.run(move || verify_now(&password, &stored)).await?;
        Ok(known && matches)
    }

    async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, PasswordError> + Send + 'static,
    ) -> Result<T, PasswordError> {
        // The permit moves into the blocking task: a dropped request does not free it while
        // Argon2 still runs (review finding 3).
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| PasswordError::Worker)?;
        tokio::task::spawn_blocking(move || {
            let result = work();
            drop(permit);
            result
        })
        .await
        .map_err(|_| PasswordError::Worker)?
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const PASSWORD: &str = "password-ok";
    const OTHER: &str = "password-other";
    const ONE_AT_A_TIME: usize = 1;
    /// Time after which the waiting call is dropped.
    const CANCEL_AFTER: Duration = Duration::from_millis(50);
    /// Longest wait for the permit to come back after the work ends.
    const RELEASE_LIMIT: Duration = Duration::from_secs(5);
    const POLL_INTERVAL: Duration = Duration::from_millis(5);

    #[tokio::test]
    async fn hash_matches_only_its_password() {
        let hasher = PasswordHasher::new(ONE_AT_A_TIME).expect("hasher");
        let stored = hasher.hash(PASSWORD.to_owned()).await.expect("hash");
        assert!(stored.starts_with("$argon2id$"));
        assert!(
            hasher
                .verify(PASSWORD.to_owned(), Some(stored.clone()))
                .await
                .expect("verify")
        );
        assert!(
            !hasher
                .verify(OTHER.to_owned(), Some(stored))
                .await
                .expect("verify other")
        );
    }

    #[tokio::test]
    async fn unknown_user_never_matches() {
        let hasher = PasswordHasher::new(ONE_AT_A_TIME).expect("hasher");
        assert!(
            !hasher
                .verify(DUMMY_PASSWORD.to_owned(), None)
                .await
                .expect("verify")
        );
    }

    #[tokio::test]
    async fn broken_stored_hash_is_an_error() {
        let hasher = PasswordHasher::new(ONE_AT_A_TIME).expect("hasher");
        assert!(matches!(
            hasher
                .verify(PASSWORD.to_owned(), Some("not-a-hash".to_owned()))
                .await,
            Err(PasswordError::StoredHash)
        ));
    }

    #[tokio::test]
    async fn a_dropped_call_keeps_its_permit_until_the_work_ends() {
        let hasher = PasswordHasher::new(ONE_AT_A_TIME).expect("hasher");
        let (finish, finished) = std::sync::mpsc::channel::<()>();
        let call = hasher.run(move || finished.recv().map_err(|_| PasswordError::Worker));
        assert!(tokio::time::timeout(CANCEL_AFTER, call).await.is_err());
        assert_eq!(hasher.permits.available_permits(), 0);
        finish.send(()).expect("finish");
        tokio::time::timeout(RELEASE_LIMIT, async {
            while hasher.permits.available_permits() == 0 {
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        })
        .await
        .expect("permit returned");
    }
}
