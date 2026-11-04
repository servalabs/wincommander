// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
fn context() -> CandidateContext {
    CandidateContext {
        trust: FleetTrust {
            org_id: "org".into(),
            device_id: "Device-1".into(),
            enrollment_generation: "gen".into(),
            signer_key: "key".into(),
        },
        origins: vec!["https://fleet.test".into()],
        profile: TransportProfile {
            spki_pins: String::new(),
            environment_fingerprint: "profile".into(),
        },
        revision: 1,
        connection_revision: 0,
        origin: "https://fleet.test".into(),
    }
}
#[test]
fn queue_is_bounded_and_latest_offer_fences_older_results() {
    let mut queue = CandidateQueue::default();
    assert!(queue
        .push(context(), Zeroizing::new(vec![1; 32]), [1; 32])
        .unwrap());
    let first = queue.take().unwrap();
    assert!(!queue
        .push(context(), Zeroizing::new(vec![1; 32]), [1; 32])
        .unwrap());
    assert!(queue
        .push(context(), Zeroizing::new(vec![2; 32]), [2; 32])
        .unwrap());
    // A new A after B must supersede B, not be mistaken for the older A in flight.
    assert!(queue
        .push(context(), Zeroizing::new(vec![1; 32]), [1; 32])
        .unwrap());
    assert_ne!(queue.sequence, first.sequence);
    queue.finish(first.sequence);
    let latest = queue.take().unwrap();
    assert_eq!(queue.sequence, latest.sequence);
    assert_eq!(latest.secret.as_slice(), &[1; 32]);
    assert!(queue.pending.is_none());
}

#[test]
fn failed_or_superseded_candidate_never_replaces_protected_credential() {
    use crate::fleet_machine_policy::credential_integration_tests::{setup_store, TestProtector};
    let store = setup_store();
    let (updates, _) = tokio::sync::watch::channel(());
    let (trust, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let profile = profile.unwrap();
    let prepare = |secret: &[u8]| {
        store
            .prepare_candidate(
                (&trust, &store.connection_token().unwrap()),
                &origins,
                &profile,
                &origins[0],
                secret,
                &TestProtector,
            )
            .unwrap()
            .unwrap()
    };
    let original = [1u8; 32];
    store
        .commit_candidate(&prepare(&original), &original, &TestProtector)
        .unwrap();
    let mut queue = CandidateQueue::default();
    let failed = [2u8; 32];
    queue
        .push(prepare(&failed), Zeroizing::new(failed.to_vec()), [2; 32])
        .unwrap();
    let failed_candidate = queue.take().unwrap();
    finish_candidate(
        &mut queue,
        &failed_candidate,
        false,
        &store,
        &TestProtector,
        &updates,
    );
    assert_eq!(
        store
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &original
    );

    let stale = [3u8; 32];
    let newer = [4u8; 32];
    queue
        .push(prepare(&stale), Zeroizing::new(stale.to_vec()), [3; 32])
        .unwrap();
    let stale_candidate = queue.take().unwrap();
    queue
        .push(prepare(&newer), Zeroizing::new(newer.to_vec()), [4; 32])
        .unwrap();
    finish_candidate(
        &mut queue,
        &stale_candidate,
        true,
        &store,
        &TestProtector,
        &updates,
    );
    assert_eq!(
        store
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &original
    );
    let latest = queue.take().unwrap();
    finish_candidate(&mut queue, &latest, true, &store, &TestProtector, &updates);
    assert_eq!(
        store
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &newer
    );
}

#[test]
fn disconnect_fences_a_queued_or_inflight_first_candidate() {
    use crate::fleet_machine_policy::credential_integration_tests::{setup_store, TestProtector};
    let store = setup_store();
    let (updates, _) = tokio::sync::watch::channel(());
    let (trust, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let profile = profile.unwrap();
    let secret = [7u8; 32];
    let prepare = || {
        store
            .prepare_candidate(
                (&trust, &store.connection_token().unwrap()),
                &origins,
                &profile,
                &origins[0],
                &secret,
                &TestProtector,
            )
            .unwrap()
            .unwrap()
    };
    let mut queue = CandidateQueue::default();
    queue
        .push(prepare(), Zeroizing::new(secret.to_vec()), [7; 32])
        .unwrap();
    let inflight = queue.take().unwrap();
    queue
        .push(prepare(), Zeroizing::new(secret.to_vec()), [8; 32])
        .unwrap();
    let token = store.credential_clear_token().unwrap().unwrap();
    assert!(store
        .clear_credential(&token, &store.connection_token().unwrap())
        .unwrap());
    finish_candidate(
        &mut queue,
        &inflight,
        true,
        &store,
        &TestProtector,
        &updates,
    );
    let queued = queue.take().unwrap();
    finish_candidate(&mut queue, &queued, true, &store, &TestProtector, &updates);
    assert!(store.credential_snapshot(&TestProtector).unwrap().is_none());
    assert!(matches!(
        store.prepare_candidate(
            (&trust, &store.connection_token().unwrap()),
            &origins,
            &profile,
            &origins[0],
            &secret,
            &TestProtector
        ),
        Err("service_disabled")
    ));
}

#[tokio::test(start_paused = true)]
async fn verified_credential_wakes_the_worker_without_retry_storms() {
    use crate::fleet_machine_policy::credential_integration_tests::{setup_store, TestProtector};
    let store = setup_store();
    let (trust, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let profile = profile.unwrap();
    let prepare = |secret: &[u8]| {
        store
            .prepare_candidate(
                (&trust, &store.connection_token().unwrap()),
                &origins,
                &profile,
                &origins[0],
                secret,
                &TestProtector,
            )
            .unwrap()
            .unwrap()
    };
    let (updates, mut receiver) = tokio::sync::watch::channel(());
    let mut queue = CandidateQueue::default();
    let failed = [1u8; 32];
    queue
        .push(prepare(&failed), Zeroizing::new(failed.to_vec()), [1; 32])
        .unwrap();
    let candidate = queue.take().unwrap();
    finish_candidate(
        &mut queue,
        &candidate,
        false,
        &store,
        &TestProtector,
        &updates,
    );
    assert!(!receiver.has_changed().unwrap());
    let older = [2u8; 32];
    let latest = [3u8; 32];
    queue
        .push(prepare(&older), Zeroizing::new(older.to_vec()), [2; 32])
        .unwrap();
    let stale = queue.take().unwrap();
    queue
        .push(prepare(&latest), Zeroizing::new(latest.to_vec()), [3; 32])
        .unwrap();
    finish_candidate(&mut queue, &stale, true, &store, &TestProtector, &updates);
    assert!(!receiver.has_changed().unwrap());

    // This is the same wait used by the running worker after a failed request.
    let waiter = tokio::spawn(async move {
        wait_for_retry(&mut receiver, Duration::from_secs(300)).await;
        receiver
    });
    tokio::task::yield_now().await;
    assert!(!waiter.is_finished());
    let accepted = queue.take().unwrap();
    finish_candidate(
        &mut queue,
        &accepted,
        true,
        &store,
        &TestProtector,
        &updates,
    );
    tokio::task::yield_now().await;
    assert!(waiter.is_finished());
    let mut receiver = waiter.await.unwrap();
    assert_eq!(
        store
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &latest
    );

    // No duplicate wake remains after the accepted update is handled.
    let waiter = tokio::spawn(async move {
        wait_for_retry(&mut receiver, Duration::from_secs(300)).await;
        receiver
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(!waiter.is_finished());
    tokio::time::advance(Duration::from_secs(299)).await;
    let mut receiver = waiter.await.unwrap();

    // A rotation that lands after the store read but before waiting must not
    // be lost. Several such commits coalesce into one immediate retry.
    receiver.borrow_and_update();
    let snapshot = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    for (number, secret) in [(4u8, [4u8; 32]), (5u8, [5u8; 32])] {
        queue
            .push(
                prepare(&secret),
                Zeroizing::new(secret.to_vec()),
                [number; 32],
            )
            .unwrap();
        let candidate = queue.take().unwrap();
        finish_candidate(
            &mut queue,
            &candidate,
            true,
            &store,
            &TestProtector,
            &updates,
        );
    }
    assert!(!store.credential_is_current(&snapshot));
    assert!(receiver.has_changed().unwrap());
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_retry(&mut receiver, Duration::from_secs(300)),
    )
    .await
    .unwrap();
    assert!(!receiver.has_changed().unwrap());
}

#[test]
fn new_connection_fences_both_active_and_queued_candidates_before_install() {
    use crate::fleet_machine_policy::credential_integration_tests::{setup_store, TestProtector};
    let store = setup_store();
    let (trust, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let profile = profile.unwrap();
    let old_connection = store.connection_token().unwrap();
    let prepare = |secret: &[u8]| {
        store
            .prepare_candidate(
                (&trust, &old_connection),
                &origins,
                &profile,
                &origins[0],
                secret,
                &TestProtector,
            )
            .unwrap()
            .unwrap()
    };
    let mut queue = CandidateQueue::default();
    queue
        .push(prepare(&[1; 32]), Zeroizing::new(vec![1; 32]), [1; 32])
        .unwrap();
    let active = queue.take().unwrap();
    queue
        .push(prepare(&[2; 32]), Zeroizing::new(vec![2; 32]), [2; 32])
        .unwrap();
    let clear = store.credential_clear_token().unwrap().unwrap();
    store
        .start_connection(&trust, &clear, &old_connection)
        .unwrap();
    let (updates, receiver) = tokio::sync::watch::channel(());
    finish_candidate(&mut queue, &active, true, &store, &TestProtector, &updates);
    let pending = queue.take().unwrap();
    finish_candidate(&mut queue, &pending, true, &store, &TestProtector, &updates);
    assert!(store.credential_snapshot(&TestProtector).unwrap().is_none());
    assert!(!receiver.has_changed().unwrap());
    assert!(matches!(
        store.prepare_candidate(
            (&trust, &old_connection),
            &origins,
            &profile,
            &origins[0],
            &[3; 32],
            &TestProtector
        ),
        Err("binding_changed")
    ));
}
