// SPDX-License-Identifier: AGPL-3.0-or-later
//! Where logins, ranks and the audit log are kept: the global database, or memory in tests.

use nw_db::{AdminRank, AuditEntry, AuditRow, DbError, PgPool};
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

    /// The store, also after a panicked test thread.
    pub fn lock(memory: &Arc<Mutex<MemoryStore>>) -> MutexGuard<'_, MemoryStore> {
        memory.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
