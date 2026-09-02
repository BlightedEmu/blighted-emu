// SPDX-License-Identifier: AGPL-3.0-or-later
//! Storage interfaces of the login logic. PostgreSQL ([`crate::pg`]) and Valkey
//! ([`crate::valkey`]) implement them for `edge-http`; the tests use in-memory versions.

use std::future::Future;

use nw_db::Account;
use nw_protocol::launcher_api::LauncherSession;
use uuid::Uuid;

/// A store could not answer.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// That username is already registered.
    #[error("username is already registered")]
    DuplicateUsername,
    /// PostgreSQL or Valkey failed.
    #[error("store is unavailable")]
    Unavailable(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl StoreError {
    /// Wraps a backend error.
    pub fn unavailable(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Unavailable(Box::new(error))
    }
}

/// Accounts and launcher sessions (global database).
pub trait AccountStore: Clone + Send + Sync + 'static {
    /// Creates an account with a password login; `username` is already lower case.
    fn create_password_account(
        &self,
        username: &str,
        password_hash: &str,
    ) -> impl Future<Output = Result<Account, StoreError>> + Send;

    /// The account and stored hash of a lower-case username.
    fn password_login(
        &self,
        username: &str,
    ) -> impl Future<Output = Result<Option<(Account, String)>, StoreError>> + Send;

    /// Finds or creates the account of a Discord user.
    fn discord_account(
        &self,
        discord_user_id: &str,
        display_name: &str,
    ) -> impl Future<Output = Result<Account, StoreError>> + Send;

    /// Stores a launcher session by token hash.
    fn create_launcher_session(
        &self,
        token_hash: [u8; 32],
        account_id: Uuid,
        expires_at: i64,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The account of a live launcher session.
    fn launcher_session_account(
        &self,
        token_hash: [u8; 32],
    ) -> impl Future<Output = Result<Option<Account>, StoreError>> + Send;

    /// Ends a launcher session.
    fn delete_launcher_session(
        &self,
        token_hash: [u8; 32],
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// Game session keys (Valkey).
pub trait GameSessionStore: Clone + Send + Sync + 'static {
    /// Stores a new key for `account_id`; it must be used within `claim_seconds`.
    fn create(
        &self,
        key: &str,
        account_id: Uuid,
        claim_seconds: u64,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The account of a live key; extends its life to `ttl_seconds`.
    fn resolve(
        &self,
        key: &str,
        ttl_seconds: u64,
    ) -> impl Future<Output = Result<Option<Uuid>, StoreError>> + Send;
}

/// Counters for rate limits (Valkey).
pub trait RateLimiter: Clone + Send + Sync + 'static {
    /// The current count of `key` (0 when absent or expired).
    fn count(&self, key: &str) -> impl Future<Output = Result<u64, StoreError>> + Send;

    /// Adds one to `key`; a new counter lives for `window_seconds`. Returns the new count.
    fn increment(
        &self,
        key: &str,
        window_seconds: u64,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;
}

/// State of one Discord login attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscordAttempt {
    /// No such attempt, or it expired.
    Unknown,
    /// Started; the player has not finished in the browser.
    Pending,
    /// Finished. Returned once, then removed.
    Ready(LauncherSession),
}

/// Discord login attempts (Valkey).
pub trait DiscordStateStore: Clone + Send + Sync + 'static {
    /// Starts an attempt with its PKCE verifier.
    fn begin(
        &self,
        state: &str,
        verifier: &str,
        ttl_seconds: u64,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The PKCE verifier of a started attempt.
    fn verifier(
        &self,
        state: &str,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;

    /// Stores the launcher session of a finished attempt.
    fn complete(
        &self,
        state: &str,
        session: &LauncherSession,
        ttl_seconds: u64,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The attempt state; `Ready` is removed when it is returned.
    fn poll(&self, state: &str) -> impl Future<Output = Result<DiscordAttempt, StoreError>> + Send;
}
