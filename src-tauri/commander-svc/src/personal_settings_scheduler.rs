// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded account transactions; blocking storage never occupies pipe workers.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_ACCOUNTS: usize = 64;
const MAX_REQUESTS: usize = 32;
const MAX_ACCOUNT_REQUESTS: usize = 4;
const QUEUE_TIMEOUT: Duration = Duration::from_secs(2);
const BUSY: &str = "personal_settings_busy";

struct Account {
    admission: Arc<Semaphore>,
    transaction: Arc<Semaphore>,
}

pub(super) struct Scheduler {
    accounts: Mutex<HashMap<String, Weak<Account>>>,
    admission: Arc<Semaphore>,
}

struct Transaction {
    // Retain the lane while blocking work runs, even if its caller is cancelled.
    _account: Arc<Account>,
    _account_admission: OwnedSemaphorePermit,
    _admission: OwnedSemaphorePermit,
    _transaction: OwnedSemaphorePermit,
}

impl Scheduler {
    pub(super) fn new() -> Self {
        Self {
            accounts: Mutex::new(HashMap::new()),
            admission: Arc::new(Semaphore::new(MAX_REQUESTS)),
        }
    }

    fn account(&self, sid: &str) -> Result<Arc<Account>, &'static str> {
        let mut accounts = self.accounts.lock().map_err(|_| super::UNAVAILABLE)?;
        accounts.retain(|_, account| account.strong_count() > 0);
        if let Some(account) = accounts.get(sid).and_then(Weak::upgrade) {
            return Ok(account);
        }
        if accounts.len() >= MAX_ACCOUNTS {
            return Err(BUSY);
        }
        let account = Arc::new(Account {
            admission: Arc::new(Semaphore::new(MAX_ACCOUNT_REQUESTS)),
            transaction: Arc::new(Semaphore::new(1)),
        });
        accounts.insert(sid.to_owned(), Arc::downgrade(&account));
        Ok(account)
    }

    async fn acquire(&self, sid: &str) -> Result<Transaction, &'static str> {
        let account = self.account(sid)?;
        let account_admission = account
            .admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| BUSY)?;
        let admission = self
            .admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| BUSY)?;
        let transaction =
            tokio::time::timeout(QUEUE_TIMEOUT, account.transaction.clone().acquire_owned())
                .await
                .map_err(|_| BUSY)?
                .map_err(|_| super::UNAVAILABLE)?;
        Ok(Transaction {
            _account: account,
            _account_admission: account_admission,
            _admission: admission,
            _transaction: transaction,
        })
    }

    pub(super) async fn run<T: Send + 'static>(
        &self,
        sid: &str,
        work: impl FnOnce() -> super::StoreResult<T> + Send + 'static,
    ) -> super::StoreResult<T> {
        let transaction = self.acquire(sid).await?;
        tokio::task::spawn_blocking(move || {
            let _transaction = transaction;
            work()
        })
        .await
        .map_err(|_| super::UNAVAILABLE)?
    }
}

#[cfg(test)]
#[path = "personal_settings_scheduler_tests.rs"]
mod tests;
