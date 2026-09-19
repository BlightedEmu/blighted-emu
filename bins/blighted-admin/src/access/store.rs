// SPDX-License-Identifier: AGPL-3.0-or-later
//! Where logins, ranks and the audit log are kept: the global database, or memory in tests.

use nw_db::{AdminRank, AuditEntry, AuditRow, DbError, PgPool, RankRow};
use uuid::Uuid;

/// The login data of one username.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginRecord {
    /// The account.
    pub account: Uuid,
    /// True when the account is banned.
    pub banned: bool,
    /// The stored Argon2 hash.
    pub password_hash: String,
}

/// The store of an admin site.
#[derive(Debug, Clone)]
pub enum Store {
    /// The global database.
    Postgres(PgPool),
    /// Memory, for tests.
    #[cfg(test)]
    Memory(std::sync::Arc<std::sync::Mutex<memory::MemoryStore>>),
}

impl Store {
    /// The login data of a lower-case username.
    pub async fn login(&self, username: &str) -> Result<Option<LoginRecord>, DbError> {
        match self {
            Self::Postgres(pool) => {
                Ok(nw_db::password_login(pool, username)
                    .await?
                    .map(|(account, password_hash)| LoginRecord {
                        account: account.id,
                        banned: account.banned,
                        password_hash,
                    }))
            }
            #[cfg(test)]
            Self::Memory(memory) => Ok(memory::lock(memory).logins.get(username).cloned()),
        }
    }

    /// The rank of an account.
    pub async fn rank(&self, account: Uuid) -> Result<Option<AdminRank>, DbError> {
        match self {
            Self::Postgres(pool) => nw_db::admin_rank(pool, account).await,
            #[cfg(test)]
            Self::Memory(memory) => Ok(memory::lock(memory).ranks.get(&account).copied()),
        }
    }

    /// Every account with an admin rank.
    pub async fn ranks(&self) -> Result<Vec<RankRow>, DbError> {
        match self {
            Self::Postgres(pool) => nw_db::admin_ranks(pool).await,
            #[cfg(test)]
            Self::Memory(memory) => Ok(memory::lock(memory).rank_rows()),
        }
    }

    /// Gives the account of login `username` the rank `rank` (`None`: removes it); `false`
    /// when no account has that login.
    pub async fn set_rank(
        &self,
        username: &str,
        rank: Option<AdminRank>,
        given_by: Option<Uuid>,
    ) -> Result<bool, DbError> {
        let Some(record) = self.login(username).await? else {
            return Ok(false);
        };
        match self {
            Self::Postgres(pool) => match rank {
                Some(rank) => nw_db::set_admin_rank(pool, record.account, rank, given_by).await,
                None => nw_db::remove_admin_rank(pool, record.account)
                    .await
                    .map(drop),
            },
            #[cfg(test)]
            Self::Memory(memory) => {
                let mut memory = memory::lock(memory);
                match rank {
                    Some(rank) => memory.ranks.insert(record.account, rank),
                    None => memory.ranks.remove(&record.account),
                };
                Ok(())
            }
        }
        .map(|()| true)
    }

    /// Bans or unbans the account of login `username`; `false` when no account has it.
    pub async fn set_banned(&self, username: &str, banned: bool) -> Result<bool, DbError> {
        let Some(record) = self.login(username).await? else {
            return Ok(false);
        };
        match self {
            Self::Postgres(pool) => nw_db::set_banned(pool, record.account, banned).await,
            #[cfg(test)]
            Self::Memory(memory) => Ok(memory::lock(memory)
                .logins
                .get_mut(username)
                .map(|login| login.banned = banned)
                .is_some()),
        }
    }

    /// Adds an audit row.
    pub async fn audit(&self, entry: &AuditEntry) -> Result<(), DbError> {
        match self {
            Self::Postgres(pool) => nw_db::insert_audit(pool, entry).await.map(drop),
            #[cfg(test)]
            Self::Memory(memory) => {
                memory::lock(memory).audit.push(entry.clone());
                Ok(())
            }
        }
    }

    /// The newest `limit` audit rows; only those of `account` when it is set.
    pub async fn audit_rows(
        &self,
        account: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<AuditRow>, DbError> {
        match self {
            Self::Postgres(pool) => nw_db::audit_rows(pool, account, limit).await,
            #[cfg(test)]
            Self::Memory(memory) => Ok(memory::lock(memory).rows(account, limit)),
        }
    }
}

#[cfg(test)]
pub mod memory {
    //! The in-memory store of the tests.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

    use super::*;

    /// Logins, ranks and audit entries.
    #[derive(Debug, Default)]
    pub struct MemoryStore {
        /// By lower-case username.
        pub logins: HashMap<String, LoginRecord>,
        /// By account.
        pub ranks: HashMap<Uuid, AdminRank>,
        /// In order.
        pub audit: Vec<AuditEntry>,
    }

    impl MemoryStore {
        pub(super) fn rows(&self, account: Option<Uuid>, limit: i64) -> Vec<AuditRow> {
            let limit = usize::try_from(limit).unwrap_or(0);
            self.audit
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, entry)| account.is_none() || entry.account == account)
                .take(limit)
                .map(|(index, entry)| AuditRow {
                    id: i64::try_from(index).unwrap_or(i64::MAX),
                    at_unix_ms: 0,
                    entry: entry.clone(),
                })
                .collect()
        }
    }

    impl MemoryStore {
        pub(super) fn rank_rows(&self) -> Vec<RankRow> {
            let mut rows: Vec<RankRow> = self
                .ranks
                .iter()
                .map(|(account, rank)| RankRow {
                    account: *account,
                    username: self
                        .logins
                        .iter()
                        .find(|(_, login)| login.account == *account)
                        .map(|(name, _)| name.clone()),
                    rank: *rank,
                    given_by: None,
                    given_at_unix_ms: 0,
                })
                .collect();
            rows.sort_by(|left, right| {
                right
                    .rank
                    .cmp(&left.rank)
                    .then_with(|| left.username.cmp(&right.username))
            });
            rows
        }
    }

    /// The store, also after a panicked test thread.
    pub fn lock(memory: &Arc<Mutex<MemoryStore>>) -> MutexGuard<'_, MemoryStore> {
        memory.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
