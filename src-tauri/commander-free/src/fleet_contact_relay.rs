// SPDX-License-Identifier: AGPL-3.0-or-later
//! Isolate best-effort contact relay work from the ordinary sidecar reader.
use std::{
    future::Future,
    sync::{Arc, OnceLock},
};
use tokio::sync::Semaphore;

pub(crate) fn try_spawn(work: impl Future<Output = ()> + Send + 'static) -> bool {
    static SLOT: OnceLock<Arc<Semaphore>> = OnceLock::new();
    spawn_in(&SLOT, work)
}

pub(crate) fn try_spawn_offer(work: impl Future<Output = ()> + Send + 'static) -> bool {
    static SLOT: OnceLock<Arc<Semaphore>> = OnceLock::new();
    spawn_in(&SLOT, work)
}

fn spawn_in(slot: &OnceLock<Arc<Semaphore>>, work: impl Future<Output = ()> + Send + 'static) -> bool {
    let slot = slot.get_or_init(|| Arc::new(Semaphore::new(1))).clone();
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
        let (offer_tx, offer_rx) = tokio::sync::oneshot::channel();
        assert!(try_spawn_offer(async move { let _ = offer_tx.send(()); }));
        offer_rx.await.unwrap();
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

    #[tokio::test]
    async fn pending_credential_offer_does_not_block_contact_observation() {
        let offer_slot = OnceLock::new();
        let contact_slot = OnceLock::new();
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let (finish, finish_rx) = tokio::sync::oneshot::channel();
        assert!(spawn_in(&offer_slot, async move {
            let _ = started.send(());
            let _ = finish_rx.await;
        }));
        started_rx.await.unwrap();
        assert!(!spawn_in(&offer_slot, async {}));
        let (contact, contact_rx) = tokio::sync::oneshot::channel();
        assert!(spawn_in(&contact_slot, async move { let _ = contact.send(()); }));
        contact_rx.await.unwrap();
        finish.send(()).unwrap();
    }
}
