// SPDX-License-Identifier: AGPL-3.0-or-later
//! Isolate best-effort contact relay work from the ordinary sidecar reader.
use std::{
    future::Future,
    sync::{Arc, OnceLock},
};
use tokio::sync::Semaphore;

pub(crate) fn try_spawn(work: impl Future<Output = ()> + Send + 'static) -> bool {
    static SLOT: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let slot = SLOT.get_or_init(|| Arc::new(Semaphore::new(1))).clone();
    let Ok(permit) = slot.try_acquire_owned() else {
        return false;
    };
    tokio::spawn(async move {
        let _permit = permit;
        work.await;
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_service_work_does_not_block_reader_or_accumulate() {
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let (finish, finish_rx) = tokio::sync::oneshot::channel();
        assert!(try_spawn(async move {
            let _ = started.send(());
            let _ = finish_rx.await;
        }));
        started_rx.await.unwrap();
        assert!(!try_spawn(async {}));
        // A separate ordinary reader can advance while the service work waits.
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = tx.send(7);
        });
        assert_eq!(rx.await.unwrap(), 7);
        finish.send(()).unwrap();
        tokio::task::yield_now().await;
        assert!(try_spawn(async {}));
    }
}
