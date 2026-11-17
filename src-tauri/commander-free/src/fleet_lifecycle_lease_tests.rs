// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct TestLease {
    held: Arc<AtomicBool>,
    released_on_owner: Arc<AtomicBool>,
    owner: std::thread::ThreadId,
}
impl Drop for TestLease {
    fn drop(&mut self) {
        self.released_on_owner
            .store(self.owner == std::thread::current().id(), Ordering::SeqCst);
        self.held.store(false, Ordering::SeqCst);
    }
}

async fn wait_until_released(held: &AtomicBool) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while held.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled lifecycle lease remained held");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_releases_the_lease_on_its_owner_thread() {
    let held = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let (started, received) = tokio::sync::oneshot::channel();
    let lease_held = held.clone();
    let lease_released = released.clone();
    let operation = tokio::spawn(run_with(
        move || {
            lease_held.store(true, Ordering::SeqCst);
            let guard = TestLease {
                held: lease_held,
                released_on_owner: lease_released,
                owner: std::thread::current().id(),
            };
            let _ = started.send(());
            Ok(guard)
        },
        std::future::pending::<Result<(), String>>(),
    ));
    received.await.unwrap();
    operation.abort();
    let _ = operation.await;
    wait_until_released(&held).await;
    assert!(released.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_while_waiting_never_starts_the_operation() {
    let held = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let ran = Arc::new(AtomicBool::new(false));
    let (started, received) = tokio::sync::oneshot::channel();
    let (allow, blocked) = std::sync::mpsc::channel();
    let lease_held = held.clone();
    let lease_released = released.clone();
    let operation_ran = ran.clone();
    let operation = tokio::spawn(run_with(
        move || {
            let _ = started.send(());
            blocked.recv().unwrap();
            lease_held.store(true, Ordering::SeqCst);
            Ok(TestLease {
                held: lease_held,
                released_on_owner: lease_released,
                owner: std::thread::current().id(),
            })
        },
        async move {
            operation_ran.store(true, Ordering::SeqCst);
            Ok(())
        },
    ));
    received.await.unwrap();
    operation.abort();
    let _ = operation.await;
    allow.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !released.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled waiter did not release the lease");
    assert!(!held.load(Ordering::SeqCst));
    assert!(!ran.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_launch_inside_the_lifecycle_lease_is_reentrant() {
    let held = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let acquire_held = held.clone();
    let operation_held = held.clone();
    run_with(
        move || {
            acquire_held.store(true, Ordering::SeqCst);
            Ok(TestLease {
                held: acquire_held,
                released_on_owner: released,
                owner: std::thread::current().id(),
            })
        },
        async move {
            launch(async move {
                assert!(operation_held.load(Ordering::SeqCst));
                assert!(HELD.try_with(|_| ()).is_ok());
                Ok(())
            })
            .await
        },
    )
    .await
    .unwrap();
    assert!(!held.load(Ordering::SeqCst));
}

#[tokio::test]
async fn inaccessible_machine_mutex_never_runs_the_operation() {
    let ran = Arc::new(AtomicBool::new(false));
    let operation_ran = ran.clone();
    let result = run_with(
        || Err::<(), _>("Fleet connection coordination is unavailable or unsafe".into()),
        async move {
            operation_ran.store(true, Ordering::SeqCst);
            Ok(())
        },
    )
    .await;
    assert!(result.is_err());
    assert!(!ran.load(Ordering::SeqCst));
}

/// Regression: Fleet connect's future is far larger than the 2 MiB blocking-
/// pool stack the lease polls it on. Moving it there by value overflowed the
/// stack (STATUS_STACK_OVERFLOW on "Connect to Fleet"); the lease must hold
/// any operation on the heap. Without the boxing this test aborts the process.
#[test]
fn an_operation_larger_than_the_blocking_stack_runs_without_overflow() {
    async fn oversized(ran: Arc<AtomicBool>, pad: [u8; 3 * 1024 * 1024]) -> Result<(), String> {
        tokio::task::yield_now().await;
        ran.store(pad[pad.len() - 1] == 0, Ordering::SeqCst);
        Ok(())
    }
    let ran = Arc::new(AtomicBool::new(false));
    let thread_ran = ran.clone();
    // Build the oversized future on a roomy thread so only the lease's own
    // handling of it is under test.
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime
                .block_on(run_with(
                    || Ok(()),
                    oversized(thread_ran, [0u8; 3 * 1024 * 1024]),
                ))
                .unwrap();
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(ran.load(Ordering::SeqCst));
}
