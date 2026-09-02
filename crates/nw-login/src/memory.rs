// SPDX-License-Identifier: AGPL-3.0-or-later
//! In-memory stores for tests. They follow the same contract as the PostgreSQL and Valkey
//! stores; [`contract`] checks both.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use nw_db::Account;
use nw_protocol::launcher_api::LauncherSession;
use uuid::Uuid;

use crate::queue::{
    ENTRY_TICKET_SECONDS, EntryRecord, QueueAction, QueueSettings, QueueStatus, QueueStore,
};
use crate::store::{
    AccountStore, DiscordAttempt, DiscordStateStore, GameSessionStore, RateLimiter, StoreError,
};
use crate::time::{MILLIS_PER_SECOND, unix_now};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default)]
struct Accounts {
    accounts: HashMap<Uuid, Account>,
    passwords: HashMap<String, (Uuid, String)>,
    discord: HashMap<String, Uuid>,
    sessions: HashMap<[u8; 32], (Uuid, i64)>,
}

/// Accounts in memory.
#[derive(Clone, Default)]
pub(crate) struct MemoryAccounts(Arc<Mutex<Accounts>>);

impl MemoryAccounts {
    /// Sets the ban flag of an account.
    pub(crate) fn set_banned(&self, account_id: Uuid, banned: bool) {
        if let Some(account) = lock(&self.0).accounts.get_mut(&account_id) {
            account.banned = banned;
        }
    }
}

fn new_account(accounts: &mut Accounts, display_name: &str) -> Account {
    let account = Account {
        id: Uuid::new_v4(),
        display_name: display_name.to_owned(),
        created_at_unix: unix_now(),
        banned: false,
    };
    accounts.accounts.insert(account.id, account.clone());
    account
}

impl AccountStore for MemoryAccounts {
    async fn create_password_account(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<Account, StoreError> {
        let mut accounts = lock(&self.0);
        if accounts.passwords.contains_key(username) {
            return Err(StoreError::DuplicateUsername);
        }
        let account = new_account(&mut accounts, username);
        accounts
            .passwords
            .insert(username.to_owned(), (account.id, password_hash.to_owned()));
        Ok(account)
    }

    async fn password_login(
        &self,
        username: &str,
    ) -> Result<Option<(Account, String)>, StoreError> {
        let accounts = lock(&self.0);
        Ok(accounts.passwords.get(username).and_then(|(id, hash)| {
            accounts
                .accounts
                .get(id)
                .map(|account| (account.clone(), hash.clone()))
        }))
    }

    async fn discord_account(
        &self,
        discord_user_id: &str,
        display_name: &str,
    ) -> Result<Account, StoreError> {
        let mut accounts = lock(&self.0);
        if let Some(account) = accounts
            .discord
            .get(discord_user_id)
            .and_then(|id| accounts.accounts.get(id))
        {
            return Ok(account.clone());
        }
        let account = new_account(&mut accounts, display_name);
        accounts
            .discord
            .insert(discord_user_id.to_owned(), account.id);
        Ok(account)
    }

    async fn create_launcher_session(
        &self,
        token_hash: [u8; 32],
        account_id: Uuid,
        expires_at: i64,
    ) -> Result<(), StoreError> {
        lock(&self.0)
            .sessions
            .insert(token_hash, (account_id, expires_at));
        Ok(())
    }

    async fn launcher_session_account(
        &self,
        token_hash: [u8; 32],
    ) -> Result<Option<Account>, StoreError> {
        let accounts = lock(&self.0);
        Ok(accounts
            .sessions
            .get(&token_hash)
            .filter(|(_, expires_at)| *expires_at > unix_now())
            .and_then(|(id, _)| accounts.accounts.get(id).cloned()))
    }

    async fn delete_launcher_session(&self, token_hash: [u8; 32]) -> Result<(), StoreError> {
        lock(&self.0).sessions.remove(&token_hash);
        Ok(())
    }
}

/// A value that expires.
struct Expiring<T> {
    value: T,
    until: Instant,
}

impl<T> Expiring<T> {
    fn new(value: T, seconds: u64) -> Self {
        Self {
            value,
            until: Instant::now() + Duration::from_secs(seconds),
        }
    }

    fn live(&self) -> bool {
        Instant::now() < self.until
    }
}

/// Game session keys in memory.
#[derive(Clone, Default)]
pub(crate) struct MemoryGameSessions(Arc<Mutex<HashMap<String, Expiring<Uuid>>>>);

impl GameSessionStore for MemoryGameSessions {
    async fn create(
        &self,
        key: &str,
        account_id: Uuid,
        claim_seconds: u64,
    ) -> Result<(), StoreError> {
        lock(&self.0).insert(key.to_owned(), Expiring::new(account_id, claim_seconds));
        Ok(())
    }

    async fn resolve(&self, key: &str, ttl_seconds: u64) -> Result<Option<Uuid>, StoreError> {
        let mut sessions = lock(&self.0);
        let Some(entry) = sessions.get(key).filter(|entry| entry.live()) else {
            sessions.remove(key);
            return Ok(None);
        };
        let account_id = entry.value;
        sessions.insert(key.to_owned(), Expiring::new(account_id, ttl_seconds));
        Ok(Some(account_id))
    }
}

/// Rate-limit counters in memory.
#[derive(Clone, Default)]
pub(crate) struct MemoryRateLimiter(Arc<Mutex<HashMap<String, Expiring<u64>>>>);

impl RateLimiter for MemoryRateLimiter {
    async fn increment(&self, key: &str, window_seconds: u64) -> Result<u64, StoreError> {
        let mut counters = lock(&self.0);
        match counters.get_mut(key).filter(|entry| entry.live()) {
            Some(entry) => {
                entry.value += 1;
                Ok(entry.value)
            }
            None => {
                counters.insert(key.to_owned(), Expiring::new(1, window_seconds));
                Ok(1)
            }
        }
    }

    async fn release(&self, key: &str) -> Result<(), StoreError> {
        if let Some(entry) = lock(&self.0).get_mut(key).filter(|entry| entry.live()) {
            entry.value = entry.value.saturating_sub(1);
        }
        Ok(())
    }
}

#[derive(Default)]
struct DiscordStates {
    verifiers: HashMap<String, Expiring<String>>,
    results: HashMap<String, Expiring<LauncherSession>>,
}

/// Discord login attempts in memory.
#[derive(Clone, Default)]
pub(crate) struct MemoryDiscordStates(Arc<Mutex<DiscordStates>>);

impl DiscordStateStore for MemoryDiscordStates {
    async fn begin(&self, state: &str, verifier: &str, ttl_seconds: u64) -> Result<(), StoreError> {
        lock(&self.0).verifiers.insert(
            state.to_owned(),
            Expiring::new(verifier.to_owned(), ttl_seconds),
        );
        Ok(())
    }

    async fn verifier(&self, state: &str) -> Result<Option<String>, StoreError> {
        Ok(lock(&self.0)
            .verifiers
            .get(state)
            .filter(|entry| entry.live())
            .map(|entry| entry.value.clone()))
    }

    async fn complete(
        &self,
        state: &str,
        session: &LauncherSession,
        ttl_seconds: u64,
    ) -> Result<(), StoreError> {
        lock(&self.0).results.insert(
            state.to_owned(),
            Expiring::new(session.clone(), ttl_seconds),
        );
        Ok(())
    }

    async fn poll(&self, state: &str) -> Result<DiscordAttempt, StoreError> {
        let mut states = lock(&self.0);
        if let Some(result) = states.results.remove(state).filter(|entry| entry.live()) {
            states.verifiers.remove(state);
            return Ok(DiscordAttempt::Ready(result.value));
        }
        if states
            .verifiers
            .get(state)
            .is_some_and(|entry| entry.live())
        {
            Ok(DiscordAttempt::Pending)
        } else {
            Ok(DiscordAttempt::Unknown)
        }
    }
}

#[derive(Debug, Clone)]
struct MemoryEntry {
    account_id: Uuid,
    character_id: Uuid,
    joined_ms: u64,
    refreshed_ms: u64,
}

#[derive(Default)]
struct WorldQueue {
    waiting: HashMap<String, MemoryEntry>,
    admitted: HashMap<String, (EntryRecord, u64)>,
    accounts: HashMap<Uuid, String>,
}

/// Login queues in memory, with the rules of the Valkey script.
#[derive(Clone, Default)]
pub(crate) struct MemoryQueue(Arc<Mutex<HashMap<Uuid, WorldQueue>>>);

impl WorldQueue {
    fn forget(&mut self, ticket: &str) {
        self.waiting.remove(ticket);
        self.admitted.remove(ticket);
        self.accounts.retain(|_, owned| owned != ticket);
    }

    fn order(&self) -> Vec<String> {
        let mut tickets: Vec<(&String, u64)> = self
            .waiting
            .iter()
            .map(|(ticket, entry)| (ticket, entry.joined_ms))
            .collect();
        tickets.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(right.0)));
        tickets
            .into_iter()
            .map(|(ticket, _)| ticket.clone())
            .collect()
    }
}

impl QueueStore for MemoryQueue {
    async fn update(
        &self,
        settings: &QueueSettings,
        account_id: Uuid,
        action: QueueAction,
        now_ms: u64,
    ) -> Result<Option<QueueStatus>, StoreError> {
        let mut worlds = lock(&self.0);
        let queue = worlds.entry(settings.world_id).or_default();
        let stale_before = now_ms.saturating_sub(settings.stale_after_ms);
        let stale: Vec<String> = queue
            .waiting
            .iter()
            .filter(|(_, entry)| entry.refreshed_ms < stale_before)
            .map(|(ticket, _)| ticket.clone())
            .collect();
        for ticket in stale {
            queue.forget(&ticket);
        }
        queue
            .admitted
            .retain(|_, (_, expires_ms)| *expires_ms > now_ms);
        let ticket = match action {
            QueueAction::Join {
                character_id,
                new_ticket_id,
            } => {
                let existing = queue.accounts.get(&account_id).cloned();
                let reuse = existing.as_ref().filter(|ticket| {
                    queue
                        .waiting
                        .get(*ticket)
                        .map(|entry| entry.character_id)
                        .or_else(|| {
                            queue
                                .admitted
                                .get(*ticket)
                                .map(|(record, _)| record.character_id)
                        })
                        == Some(character_id)
                });
                match reuse {
                    Some(ticket) => ticket.clone(),
                    None => {
                        if let Some(old) = existing {
                            queue.forget(&old);
                        }
                        queue.waiting.insert(
                            new_ticket_id.clone(),
                            MemoryEntry {
                                account_id,
                                character_id,
                                joined_ms: now_ms,
                                refreshed_ms: now_ms,
                            },
                        );
                        queue.accounts.insert(account_id, new_ticket_id.clone());
                        new_ticket_id
                    }
                }
            }
            QueueAction::Refresh { ticket_id } => {
                let owner = queue
                    .waiting
                    .get(&ticket_id)
                    .map(|entry| entry.account_id)
                    .or_else(|| {
                        queue
                            .admitted
                            .get(&ticket_id)
                            .map(|(record, _)| record.account_id)
                    });
                if owner != Some(account_id) {
                    return Ok(None);
                }
                ticket_id
            }
        };
        if let Some(entry) = queue.waiting.get_mut(&ticket) {
            entry.refreshed_ms = now_ms;
        }
        for head in queue.order() {
            if queue.admitted.len() >= settings.capacity as usize {
                break;
            }
            if let Some(entry) = queue.waiting.remove(&head) {
                let expires_ms = now_ms + ENTRY_TICKET_SECONDS * MILLIS_PER_SECOND;
                let record = EntryRecord {
                    ticket_id: head.clone(),
                    account_id: entry.account_id,
                    world_id: settings.world_id,
                    character_id: entry.character_id,
                    expires_at: i64::try_from(expires_ms / MILLIS_PER_SECOND).unwrap_or(i64::MAX),
                };
                queue.admitted.insert(head, (record, expires_ms));
            }
        }
        if let Some((record, _)) = queue.admitted.get(&ticket) {
            return Ok(Some(QueueStatus::Admitted(record.clone())));
        }
        let position = queue.order().iter().position(|waiting| *waiting == ticket);
        Ok(position.map(|index| QueueStatus::Waiting {
            ticket_id: ticket,
            position: index as u64 + 1,
        }))
    }
}

/// Contract checks shared by the in-memory and the real stores.
pub(crate) mod contract {
    use super::*;

    const HASH: &str = "$argon2id$stored";
    const FAR_FUTURE: i64 = 4_102_444_800;
    const PAST: i64 = 1_000_000_000;
    const SHORT_SECONDS: u64 = 60;
    const LONG_SECONDS: u64 = 600;
    const LIMIT_WINDOW: u64 = 60;
    const SESSION_EXPIRES_AT: i64 = 1_760_400_000;

    /// A name no other test run uses.
    pub(crate) fn unique(prefix: &str) -> String {
        format!("{prefix}{}", Uuid::new_v4().simple())
    }

    /// Checks an [`AccountStore`].
    pub(crate) async fn accounts(store: impl AccountStore) {
        let username = unique("u");
        let account = store
            .create_password_account(&username, HASH)
            .await
            .expect("create");
        assert!(matches!(
            store.create_password_account(&username, HASH).await,
            Err(StoreError::DuplicateUsername)
        ));
        assert_eq!(
            store.password_login(&username).await.expect("login"),
            Some((account.clone(), HASH.to_owned()))
        );
        assert_eq!(
            store.password_login(&unique("u")).await.expect("none"),
            None
        );

        let discord_id = unique("d");
        let first = store
            .discord_account(&discord_id, "Nelly")
            .await
            .expect("discord");
        let again = store
            .discord_account(&discord_id, "Renamed")
            .await
            .expect("again");
        assert_eq!(first, again);

        let live = crate::keys::token_hash(&unique("t"));
        let old = crate::keys::token_hash(&unique("t"));
        store
            .create_launcher_session(live, account.id, FAR_FUTURE)
            .await
            .expect("live");
        store
            .create_launcher_session(old, account.id, PAST)
            .await
            .expect("old");
        assert_eq!(
            store.launcher_session_account(live).await.expect("live"),
            Some(account)
        );
        assert_eq!(
            store.launcher_session_account(old).await.expect("old"),
            None
        );
        store.delete_launcher_session(live).await.expect("delete");
        assert_eq!(
            store.launcher_session_account(live).await.expect("gone"),
            None
        );
    }

    /// Checks a [`GameSessionStore`].
    pub(crate) async fn game_sessions(store: impl GameSessionStore) {
        let key = unique("k");
        let account = Uuid::new_v4();
        assert_eq!(store.resolve(&key, LONG_SECONDS).await.expect("none"), None);
        store
            .create(&key, account, SHORT_SECONDS)
            .await
            .expect("create");
        assert_eq!(
            store.resolve(&key, LONG_SECONDS).await.expect("resolve"),
            Some(account)
        );
        assert_eq!(
            store.resolve(&key, LONG_SECONDS).await.expect("again"),
            Some(account)
        );
    }

    /// Checks a [`RateLimiter`].
    pub(crate) async fn rate_limiter(store: impl RateLimiter) {
        let key = unique("r");
        assert_eq!(store.increment(&key, LIMIT_WINDOW).await.expect("one"), 1);
        assert_eq!(store.increment(&key, LIMIT_WINDOW).await.expect("two"), 2);
        store.release(&key).await.expect("release");
        assert_eq!(store.increment(&key, LIMIT_WINDOW).await.expect("again"), 2);
        let absent = unique("r");
        store.release(&absent).await.expect("release absent");
        assert_eq!(
            store.increment(&absent, LIMIT_WINDOW).await.expect("first"),
            1
        );
    }

    /// Start time of the queue checks, ms.
    pub(crate) const START_MS: u64 = 1_760_400_000_000;
    /// Stale limit of the queue checks.
    pub(crate) const STALE_AFTER_MS: u64 = 40_000;
    const SECOND_MS: u64 = 1_000;
    const ONE_SLOT: u32 = 1;

    fn join(character_id: Uuid) -> QueueAction {
        QueueAction::Join {
            character_id,
            new_ticket_id: unique("t"),
        }
    }

    fn refresh(ticket_id: &str) -> QueueAction {
        QueueAction::Refresh {
            ticket_id: ticket_id.to_owned(),
        }
    }

    fn waiting(status: Option<QueueStatus>) -> (String, u64) {
        match status {
            Some(QueueStatus::Waiting {
                ticket_id,
                position,
            }) => (ticket_id, position),
            other => panic!("expected waiting, got {other:?}"),
        }
    }

    fn admitted(status: Option<QueueStatus>) -> EntryRecord {
        match status {
            Some(QueueStatus::Admitted(record)) => record,
            other => panic!("expected admitted, got {other:?}"),
        }
    }

    /// Checks a [`QueueStore`] with one slot of capacity. Times are seconds after [`START_MS`].
    pub(crate) async fn queue(store: impl QueueStore) {
        let settings = QueueSettings {
            world_id: Uuid::new_v4(),
            capacity: ONE_SLOT,
            stale_after_ms: STALE_AFTER_MS,
        };
        let at = |seconds: u64| START_MS + seconds * SECOND_MS;
        let (first, second, third) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let first_character = Uuid::new_v4();

        // 0 s: the first account takes the only slot.
        let record = admitted(
            store
                .update(&settings, first, join(first_character), at(0))
                .await
                .expect("first"),
        );
        assert_eq!(
            (record.account_id, record.character_id),
            (first, first_character)
        );
        assert_eq!(record.world_id, settings.world_id);

        // 1 s: the second account waits; joining again with the same character keeps the entry.
        let second_character = Uuid::new_v4();
        let (second_ticket, position) = waiting(
            store
                .update(&settings, second, join(second_character), at(1))
                .await
                .expect("second"),
        );
        assert_eq!(position, 1);
        let (again, _) = waiting(
            store
                .update(&settings, second, join(second_character), at(1))
                .await
                .expect("again"),
        );
        assert_eq!(again, second_ticket);

        // 2 s: the third account waits behind it; it cannot refresh the second ticket.
        let (third_ticket, position) = waiting(
            store
                .update(&settings, third, join(Uuid::new_v4()), at(2))
                .await
                .expect("third"),
        );
        assert_eq!(position, 2);
        assert_eq!(
            store
                .update(&settings, third, refresh(&second_ticket), at(2))
                .await
                .expect("stolen"),
            None
        );

        // 3 s: a new character replaces the second entry and goes to the back.
        let (replaced, position) = waiting(
            store
                .update(&settings, second, join(Uuid::new_v4()), at(3))
                .await
                .expect("replace"),
        );
        assert_ne!(replaced, second_ticket);
        assert_eq!(position, 2);
        assert_eq!(
            store
                .update(&settings, second, refresh(&second_ticket), at(3))
                .await
                .expect("old"),
            None
        );

        // 30 s: the third account refreshes and is first in line.
        let (_, position) = waiting(
            store
                .update(&settings, third, refresh(&third_ticket), at(30))
                .await
                .expect("refresh"),
        );
        assert_eq!(position, 1);

        // 61 s: the first record has expired, so the third account is admitted. The second
        // account stopped refreshing at 3 s and is removed as stale.
        let record = admitted(
            store
                .update(&settings, third, refresh(&third_ticket), at(61))
                .await
                .expect("admitted"),
        );
        assert_eq!(record.ticket_id, third_ticket);
        assert_eq!(
            store
                .update(&settings, second, refresh(&replaced), at(61))
                .await
                .expect("stale"),
            None
        );
    }

    /// Checks a [`DiscordStateStore`].
    pub(crate) async fn discord_states(store: impl DiscordStateStore) {
        let state = unique("s");
        assert_eq!(
            store.poll(&state).await.expect("unknown"),
            DiscordAttempt::Unknown
        );
        store
            .begin(&state, "verifier", LONG_SECONDS)
            .await
            .expect("begin");
        assert_eq!(
            store.verifier(&state).await.expect("verifier").as_deref(),
            Some("verifier")
        );
        assert_eq!(
            store.poll(&state).await.expect("pending"),
            DiscordAttempt::Pending
        );
        let session = LauncherSession {
            launcher_token: "token".to_owned(),
            expires_at: SESSION_EXPIRES_AT,
        };
        store
            .complete(&state, &session, LONG_SECONDS)
            .await
            .expect("complete");
        assert_eq!(
            store.poll(&state).await.expect("ready"),
            DiscordAttempt::Ready(session)
        );
        assert_eq!(
            store.poll(&state).await.expect("taken"),
            DiscordAttempt::Unknown
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_accounts_follow_the_contract() {
        contract::accounts(MemoryAccounts::default()).await;
    }

    #[tokio::test]
    async fn memory_game_sessions_follow_the_contract() {
        contract::game_sessions(MemoryGameSessions::default()).await;
    }

    #[tokio::test]
    async fn memory_rate_limiter_follows_the_contract() {
        contract::rate_limiter(MemoryRateLimiter::default()).await;
    }

    #[tokio::test]
    async fn memory_discord_states_follow_the_contract() {
        contract::discord_states(MemoryDiscordStates::default()).await;
    }

    #[tokio::test]
    async fn memory_queue_follows_the_contract() {
        contract::queue(MemoryQueue::default()).await;
    }

    const ALREADY_EXPIRED: u64 = 0;
    const TTL_SECONDS: u64 = 60;

    #[tokio::test]
    async fn expired_game_session_is_gone() {
        let store = MemoryGameSessions::default();
        store
            .create("k", Uuid::new_v4(), ALREADY_EXPIRED)
            .await
            .expect("create");
        assert_eq!(
            store.resolve("k", TTL_SECONDS).await.expect("resolve"),
            None
        );
    }
}
