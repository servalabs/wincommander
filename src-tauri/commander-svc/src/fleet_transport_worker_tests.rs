// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::fleet_machine_policy::credential_integration_tests::{setup_store_for, TestProtector};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

async fn wait_for_request(rx: &mut tokio::sync::mpsc::UnboundedReceiver<()>) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if rx.try_recv().is_ok() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("worker did not contact the local endpoint");
}

async fn wait_for_rejection() {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if status_slot().lock().unwrap().state == "request_rejected" {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("worker did not process the local rejection");
}

#[tokio::test(start_paused = true)]
async fn committed_rotation_interrupts_the_real_worker_backoff() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (seen, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            let _ = seen.send(());
        }
    });
    let store = Arc::new(setup_store_for(&origin));
    let (trust, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let profile = profile.unwrap();
    let first = [1u8; 32];
    let initial = store
        .prepare_candidate(&trust, &origins, &profile, &origin, &first, &TestProtector)
        .unwrap()
        .unwrap();
    store
        .commit_candidate(&initial, &first, &TestProtector)
        .unwrap();
    let (updates, mut receiver) = tokio::sync::watch::channel(());
    let worker_store = store.clone();
    let fingerprint = profile.environment_fingerprint.clone();
    let worker = tokio::spawn(async move {
        run_with(
            || Ok(worker_store.as_ref()),
            &TestProtector,
            || Some(fingerprint.clone()),
            |_| {
                reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .map_err(|e| e.to_string())
            },
            &mut receiver,
        )
        .await;
    });
    for attempt in 0..6 {
        if attempt != 0 {
            tokio::time::advance(Duration::from_secs(400)).await;
        }
        wait_for_request(&mut requests).await;
        wait_for_rejection().await;
    }
    let next = [2u8; 32];
    let context = store
        .prepare_candidate(&trust, &origins, &profile, &origin, &next, &TestProtector)
        .unwrap()
        .unwrap();
    let mut queue = CandidateQueue::default();
    queue
        .push(context, Zeroizing::new(next.to_vec()), [2; 32])
        .unwrap();
    let candidate = queue.take().unwrap();
    let start = tokio::time::Instant::now();
    finish_candidate(
        &mut queue,
        &candidate,
        true,
        &store,
        &TestProtector,
        &updates,
    );
    wait_for_request(&mut requests).await;
    assert!(tokio::time::Instant::now().duration_since(start) < Duration::from_secs(1));
    wait_for_rejection().await;
    assert!(
        requests.try_recv().is_err(),
        "one commit must not cause a request storm"
    );
    // The new credential gets the initial backoff after its first failure,
    // rather than inheriting the previous credential's five-minute delay.
    tokio::time::advance(Duration::from_secs(9)).await;
    tokio::task::yield_now().await;
    assert!(requests.try_recv().is_err());
    tokio::time::advance(Duration::from_secs(3)).await;
    wait_for_request(&mut requests).await;
    worker.abort();
    server.abort();
}
