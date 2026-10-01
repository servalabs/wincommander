// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::sync::mpsc;

#[tokio::test]
async fn slow_account_does_not_block_other_accounts_or_async_timers() {
    let scheduler = Arc::new(Scheduler::new());
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = mpsc::channel();
    let slow = {
        let scheduler = scheduler.clone();
        tokio::spawn(async move {
            scheduler
                .run("account-a", move || {
                    let _ = started.send(());
                    blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(1)
                })
                .await
        })
    };
    ready.await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        scheduler.run("account-b", || Ok(2)).await
    })
    .await;
    release.send(()).unwrap();
    assert_eq!(slow.await.unwrap(), Ok(1));
    assert_eq!(result.unwrap(), Ok(2));
}

#[tokio::test]
async fn cancelled_caller_does_not_release_running_account_transaction() {
    let scheduler = Arc::new(Scheduler::new());
    let (started, ready) = tokio::sync::oneshot::channel();
    let (finished, completed) = tokio::sync::oneshot::channel();
    let (release, blocked) = mpsc::channel();
    let caller = {
        let scheduler = scheduler.clone();
        tokio::spawn(async move {
            scheduler
                .run("account-a", move || {
                    let _ = started.send(());
                    blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                    let _ = finished.send(());
                    Ok(())
                })
                .await
        })
    };
    ready.await.unwrap();
    caller.abort();
    let _ = caller.await;
    let lane = scheduler.account("account-a").unwrap();
    assert_eq!(lane.transaction.available_permits(), 0);
    assert_eq!(scheduler.run("account-b", || Ok(())).await, Ok(()));
    release.send(()).unwrap();
    completed.await.unwrap();
    assert_eq!(scheduler.run("account-a", || Ok(())).await, Ok(()));
}

#[tokio::test(start_paused = true)]
async fn queue_timeout_does_not_execute_or_leak_admission() {
    let scheduler = Scheduler::new();
    let transaction = scheduler.acquire("account-a").await.unwrap();
    assert_eq!(
        scheduler
            .run("account-a", || panic!("expired work ran"))
            .await,
        Err::<(), _>(BUSY)
    );
    drop(transaction);
    assert_eq!(scheduler.run("account-a", || Ok(())).await, Ok(()));
}

#[tokio::test]
async fn per_account_and_global_admission_are_bounded_and_recover() {
    let scheduler = Scheduler::new();
    let lane = scheduler.account("account-a").unwrap();
    let held = lane
        .admission
        .clone()
        .try_acquire_many_owned(MAX_ACCOUNT_REQUESTS as u32)
        .unwrap();
    assert_eq!(
        scheduler
            .run("account-a", || panic!("excess work ran"))
            .await,
        Err::<(), _>(BUSY)
    );
    assert_eq!(scheduler.run("account-b", || Ok(())).await, Ok(()));
    drop(held);
    let held = scheduler
        .admission
        .clone()
        .try_acquire_many_owned(MAX_REQUESTS as u32)
        .unwrap();
    assert_eq!(
        scheduler
            .run("account-a", || panic!("excess work ran"))
            .await,
        Err::<(), _>(BUSY)
    );
    drop(held);
    assert_eq!(scheduler.run("account-a", || Ok(())).await, Ok(()));
}

#[test]
fn account_registry_is_bounded_and_expired_lanes_are_reclaimed() {
    let scheduler = Scheduler::new();
    let mut lanes: Vec<_> = (0..MAX_ACCOUNTS)
        .map(|i| scheduler.account(&i.to_string()).unwrap())
        .collect();
    assert!(matches!(scheduler.account("excess"), Err(BUSY)));
    lanes.pop();
    assert!(scheduler.account("replacement").is_ok());
    assert!(scheduler.accounts.lock().unwrap().len() <= MAX_ACCOUNTS);
}
