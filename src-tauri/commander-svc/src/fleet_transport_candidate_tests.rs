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
    let (trust, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let profile = profile.unwrap();
    let prepare = |secret: &[u8]| {
        store
            .prepare_candidate(
                &trust,
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
    finish_candidate(&mut queue, &failed_candidate, false, &store, &TestProtector);
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
    finish_candidate(&mut queue, &stale_candidate, true, &store, &TestProtector);
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
    finish_candidate(&mut queue, &latest, true, &store, &TestProtector);
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
    let (trust, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let profile = profile.unwrap();
    let secret = [7u8; 32];
    let prepare = || {
        store
            .prepare_candidate(
                &trust,
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
    assert!(store.clear_credential(&token).unwrap());
    finish_candidate(&mut queue, &inflight, true, &store, &TestProtector);
    let queued = queue.take().unwrap();
    finish_candidate(&mut queue, &queued, true, &store, &TestProtector);
    assert!(store.credential_snapshot(&TestProtector).unwrap().is_none());
    assert!(matches!(
        store.prepare_candidate(
            &trust,
            &origins,
            &profile,
            &origins[0],
            &secret,
            &TestProtector
        ),
        Err("service_disabled")
    ));
}
