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

#[test]
fn reachability_is_never_paired_with_a_different_contact_snapshot() {
    let old = FleetTrust {
        org_id: "old-org".into(),
        device_id: "device".into(),
        enrollment_generation: "old-generation".into(),
        signer_key: "old-key".into(),
    };
    let mut new = old.clone();
    new.org_id = "new-org".into();
    new.enrollment_generation = "new-generation".into();
    let observed = Status {
        state: "observed",
        observed_unix_ms: Some(123),
        fence: Some((old.clone(), 7)),
    };
    assert_eq!(
        render_status(&observed, Some((&old, 7)), |_, _| true)["state"],
        "observed"
    );
    // A rebind between contact and worker-status reads must never associate an
    // old observation with the new trust, even if both revisions are equal.
    assert_eq!(
        render_status(&observed, Some((&new, 7)), |_, _| true)["state"],
        "unconfirmed"
    );
    assert_eq!(
        render_status(&observed, Some((&old, 8)), |_, _| true)["state"],
        "unconfirmed"
    );
    assert_eq!(
        render_status(&observed, Some((&old, 7)), |_, _| false)["state"],
        "unconfirmed"
    );
    assert_eq!(
        render_status(&observed, None, |_, _| true)["state"],
        "unconfirmed"
    );
}

#[test]
fn credential_rotation_between_snapshot_and_status_is_unconfirmed() {
    use crate::fleet_machine_policy::credential_integration_tests::{
        setup_store, submit, TestProtector,
    };
    let store = setup_store();
    submit(&store, "https://fleet.test", &[1; 32]).unwrap();
    let credential = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    let captured = store.contact_state().unwrap().unwrap();
    let observed = Status {
        state: "observed",
        observed_unix_ms: Some(123),
        fence: Some((credential.trust.clone(), credential.revision)),
    };
    let render = |snapshot: &(_, _, _, _, _)| {
        render_status(
            &observed,
            Some((&snapshot.0, snapshot.4)),
            |trust, revision| store.credential_revision_is_current(trust, revision),
        )
    };
    assert_eq!(render(&captured)["state"], "observed");
    submit(&store, "https://fleet.test", &[2; 32]).unwrap();
    let current = store.contact_state().unwrap().unwrap();
    assert_eq!(render(&captured)["state"], "unconfirmed");
    assert_eq!(render(&current)["state"], "unconfirmed");
}
