// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cross-session serialization for Fleet connection lifecycle changes.
use std::future::Future;

tokio::task_local! { static HELD: bool; }
static LOCAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The named Windows mutex is thread-owned. Hold it on the blocking thread
/// while driving the async operation there; dropping the caller cancels the
/// operation and lets that same thread release the mutex.
pub(super) async fn run<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
{
    let local = tokio::time::timeout(std::time::Duration::from_secs(5), LOCAL.lock())
        .await
        .map_err(|_| "A Fleet connection change is already in progress".to_string())?;
    run_with_scope(
        || {
            let machine =
                optional_machine_lock(crate::paths::acquire_machine_state_lock("fleet-lifecycle"))?;
            let shared = machine.is_some();
            Ok((machine, shared))
        },
        async move {
            let _local = local;
            operation.await
        },
    )
    .await
}

pub(super) fn optional_machine_lock<T>(result: Result<T, String>) -> Result<Option<T>, String> {
    match result {
        Ok(lock) => Ok(Some(lock)),
        Err(error) if error == "machine-state lock access denied" => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub(super) async fn run_with<T, F, L, A>(acquire: A, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
    L: 'static,
    A: FnOnce() -> Result<L, String> + Send + 'static,
{
    run_with_scope(|| acquire().map(|guard| (guard, true)), operation).await
}

pub(super) async fn run_with_scope<T, F, L, A>(acquire: A, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
    L: 'static,
    A: FnOnce() -> Result<(L, bool), String> + Send + 'static,
{
    let runtime = tokio::runtime::Handle::current();
    let (cancel, mut cancelled) = tokio::sync::oneshot::channel::<()>();
    let cancellation_guard = CancelOnDrop(Some(cancel));
    let result = tokio::task::spawn_blocking(move || {
        let (_lease, shared) = acquire()?;
        runtime.block_on(async move {
            tokio::select! {
                biased;
                _ = &mut cancelled => Err("Fleet lifecycle operation was cancelled".into()),
                result = HELD.scope(shared, operation) => result,
            }
        })
    })
    .await
    .map_err(|_| "Fleet lifecycle worker failed".to_string())?;
    drop(cancellation_guard);
    result
}

/// The access-denied fallback protects only this process, so shared state may
/// not be cleared or disabled while another user's connection can be starting.
pub(super) fn shared_lease_held() -> bool {
    HELD.try_with(|shared| *shared).unwrap_or(false)
}

/// An automatic Fleet-agent launch must join the same ordering as Connect and
/// removal. A launch already invoked by a lease-owning operation is reentrant.
pub(crate) async fn launch<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
{
    if HELD.try_with(|_| ()).is_ok() {
        operation.await
    } else {
        run(operation).await
    }
}

struct CancelOnDrop(Option<tokio::sync::oneshot::Sender<()>>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[cfg(test)]
#[path = "fleet_lifecycle_lease_tests.rs"]
mod tests;
