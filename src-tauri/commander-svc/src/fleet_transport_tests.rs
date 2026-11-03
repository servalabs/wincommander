// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[test]
fn new_credential_resets_only_its_own_retry_backoff() {
    let trust = FleetTrust {
        org_id: "org".into(),
        device_id: "device".into(),
        enrollment_generation: "generation".into(),
        signer_key: "key".into(),
    };
    let mut previous = None;
    let mut failures = 8;
    let first = (trust.clone(), 1);
    reset_failures_on_new_credential(&mut previous, &first, &mut failures);
    assert_eq!(failures, 0);
    failures = 4;
    reset_failures_on_new_credential(&mut previous, &first, &mut failures);
    assert_eq!(failures, 4);
    reset_failures_on_new_credential(&mut previous, &(trust, 2), &mut failures);
    assert_eq!(failures, 0);
}

#[tokio::test]
async fn disconnect_during_first_request_prevents_old_failover_request() {
    let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origins = vec![
        format!("http://{}", first.local_addr().unwrap()),
        format!("http://{}", second.local_addr().unwrap()),
    ];
    let current = Arc::new(AtomicBool::new(true));
    let state = current.clone();
    let trust = FleetTrust {
        org_id: "org".into(),
        device_id: "device".into(),
        enrollment_generation: "gen".into(),
        signer_key: "key".into(),
    };
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let (accepted, accepted_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let first_server = tokio::spawn(async move {
        let (connection, _) = first.accept().await.unwrap();
        let _ = accepted.send(());
        let _ = release_rx.await;
        drop(connection);
    });
    let task = tokio::spawn(async move {
        attempt(&client, &trust, &origins, &[1; 32], || {
            state.load(Ordering::SeqCst)
        })
        .await
    });
    accepted_rx.await.unwrap();
    current.store(false, Ordering::SeqCst);
    release.send(()).unwrap();
    assert_eq!(task.await.unwrap(), Outcome::Superseded);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), second.accept())
            .await
            .is_err()
    );
    first_server.await.unwrap();
}
