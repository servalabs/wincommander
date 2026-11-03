// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn connection_fence_invalidates_old_clear_and_offers_but_preserves_working_credential() {
    let (store, fs) = setup();
    submit(&store, "https://fleet.test", &[1u8; 32]).unwrap();
    let old = tokens(&store);
    let candidate = prepare(&store, &old.1, &[2u8; 32]).unwrap().unwrap();
    let new = start(&store, &old);
    assert!(!clear(&store, &old));
    assert!(!store.candidate_is_current(&candidate));
    assert_eq!(
        store.commit_candidate(&candidate, &[2u8; 32], &TestProtector),
        Err("binding_changed")
    );
    assert!(matches!(
        prepare(&store, &old.1, &[2u8; 32]),
        Err("binding_changed")
    ));
    let (binding, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    assert_eq!(
        store.observe_runtime_transport(&binding, &origins, None, &old.1),
        Err("binding_changed")
    );
    let current = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    assert_eq!(current.secret.as_slice(), &[1u8; 32]);
    assert_eq!(current.profile, profile.unwrap());
    let reopened = reopen(&fs);
    assert_eq!(tokens(&reopened), new);
    assert!(!clear(&reopened, &old));
    assert_eq!(
        reopened
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &[1u8; 32]
    );
}

#[test]
fn credential_rotation_keeps_connection_token_without_authorizing_stale_credential_clear() {
    let (store, _) = setup();
    let connection = start(&store, &tokens(&store));
    let first = prepare(&store, &connection.1, &[1u8; 32]).unwrap().unwrap();
    store
        .commit_candidate(&first, &[1u8; 32], &TestProtector)
        .unwrap();
    assert_eq!(tokens(&store).1, connection.1);
    assert!(!clear(&store, &connection));
    let second = prepare(&store, &connection.1, &[2u8; 32]).unwrap().unwrap();
    store
        .commit_candidate(&second, &[2u8; 32], &TestProtector)
        .unwrap();
    assert_eq!(tokens(&store).1, connection.1);
    assert!(clear(&store, &tokens(&store)));
}

#[test]
fn clear_then_new_connection_invalidates_replay_without_reenabling_or_accepting_old_offer() {
    let (store, fs) = setup();
    let old = tokens(&store);
    assert!(clear(&store, &old));
    assert_eq!(
        store.start_connection(&trust(), &old.0, &old.1),
        Err("binding_changed")
    );
    let new = start(&store, &tokens(&store));
    assert!(!clear(&store, &old));
    assert!(!store.trust_with_origins().unwrap().unwrap().4);
    assert!(matches!(
        prepare(&store, &new.1, &[1u8; 32]),
        Err("service_disabled")
    ));
    let reopened = reopen(&fs);
    assert_eq!(tokens(&reopened), new);
    assert!(!clear(&reopened, &old));
}

#[test]
fn delayed_start_wrong_trust_and_failed_persistence_have_no_effect() {
    let (store, fs) = setup();
    submit(&store, "https://fleet.test", &[1u8; 32]).unwrap();
    let old = tokens(&store);
    let mut wrong = trust();
    wrong.org_id = "other-org".into();
    assert_eq!(
        store.start_connection(&wrong, &old.0, &old.1),
        Err("binding_changed")
    );
    fs.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        store.start_connection(&trust(), &old.0, &old.1),
        Err("store_unavailable")
    );
    assert_eq!(tokens(&store), old);
    assert_eq!(tokens(&reopen(&fs)), old);
    fs.fail.store(false, Ordering::SeqCst);
    let new = start(&store, &old);
    let bytes = fs.bytes.lock().unwrap().clone();
    assert_eq!(
        store.start_connection(&trust(), &old.0, &old.1),
        Err("binding_changed")
    );
    assert_eq!(tokens(&store), new);
    assert_eq!(*fs.bytes.lock().unwrap(), bytes);
}

#[test]
fn clear_and_connection_start_race_have_one_winner_and_no_stale_mutation() {
    let (store, _) = setup();
    let old = tokens(&store);
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let stale_clear = scope.spawn(|| {
            barrier.wait();
            clear(&store, &old)
        });
        barrier.wait();
        let started = store.start_connection(&trust(), &old.0, &old.1);
        let cleared = stale_clear.join().unwrap();
        assert_ne!(started.is_ok(), cleared);
        if started.is_ok() {
            assert!(store.trust_with_origins().unwrap().unwrap().4);
            assert!(!clear(&store, &old));
        } else {
            assert_eq!(started, Err("binding_changed"));
            assert!(!store.trust_with_origins().unwrap().unwrap().4);
            assert!(clear(&store, &old));
        }
    });
}

#[test]
fn wire_requires_connection_token_and_returns_the_committed_pair() {
    use crate::fleet_contact_transfer::handle;
    let (store, _) = setup();
    let old = tokens(&store);
    assert_eq!(
        handle(
            &store,
            serde_json::json!({"operation":"clear","token":old.0})
        ),
        Err("invalid_contact_request")
    );
    let result = handle(&store, serde_json::json!({"operation":"connection_start", "trust":trust(), "token":old.0,"connection_token":old.1})).unwrap();
    assert_eq!(result["connection_token"], tokens(&store).1);
    assert_eq!(result["credential_clear_token"], tokens(&store).0);
    assert_eq!(
        handle(
            &store,
            serde_json::json!({"operation":"clear","token":old.0,"connection_token":old.1})
        )
        .unwrap()["cleared"],
        false
    );
}

#[test]
fn failed_clear_does_not_persist_an_acknowledgement_or_rotate_the_connection() {
    let (store, fs) = setup();
    submit(&store, "https://fleet.test", &[1u8; 32]).unwrap();
    let pair = tokens(&store);
    fs.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        store.clear_credential(&pair.0, &pair.1),
        Err("store_unavailable")
    );
    assert_eq!(tokens(&store), pair);
    let reopened = reopen(&fs);
    assert_eq!(tokens(&reopened), pair);
    assert!(reopened
        .credential_snapshot(&TestProtector)
        .unwrap()
        .is_some());
    assert_eq!(
        reopened.clear_credential(&pair.0, &pair.1),
        Err("store_unavailable")
    );
    fs.fail.store(false, Ordering::SeqCst);
    assert!(clear(&reopened, &pair));
}

#[test]
fn legacy_protected_record_loads_without_an_invented_clear_acknowledgement() {
    let (store, fs) = setup();
    submit(&store, "https://fleet.test", &[1u8; 32]).unwrap();
    let mut record: serde_json::Value =
        serde_json::from_slice(fs.bytes.lock().unwrap().as_ref().unwrap()).unwrap();
    record["record"]
        .as_object_mut()
        .unwrap()
        .remove("connection_revision");
    record["record"].as_object_mut().unwrap().remove("cleared");
    *fs.bytes.lock().unwrap() = Some(serde_json::to_vec(&record).unwrap());
    let reopened = reopen(&fs);
    assert_eq!(
        reopened
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &[1u8; 32]
    );
    assert_eq!(tokens(&reopened), tokens(&store));
    assert!(reopened
        .state
        .lock()
        .unwrap()
        .record
        .as_ref()
        .unwrap()
        .cleared
        .is_none());
}

#[test]
fn authorized_trust_aba_never_resurrects_old_tokens_or_candidate_after_restart() {
    let (store, fs) = setup();
    let first = trust();
    let old = tokens(&store);
    let candidate = prepare(&store, &old.1, &[7u8; 32]).unwrap().unwrap();
    let mut second = first.clone();
    second.enrollment_generation = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
    second.org_id = "replacement-org".into();
    let proof = enrollment_proof(&store, &second);
    store
        .replace_trust(first.clone(), second.clone(), &proof, "S-1-5-18")
        .unwrap();
    let proof = enrollment_proof(&store, &first);
    store
        .replace_trust(second, first.clone(), &proof, "S-1-5-18")
        .unwrap();
    let current = tokens(&store);
    assert_ne!(current.0, old.0);
    assert_ne!(current.1, old.1);
    for store in [&store, &reopen(&fs)] {
        assert_eq!(tokens(store), current);
        assert!(!clear(store, &old));
        assert_eq!(
            store.start_connection(&first, &old.0, &old.1),
            Err("binding_changed")
        );
        assert!(!store.candidate_is_current(&candidate));
        assert_eq!(
            store.commit_candidate(&candidate, &[7u8; 32], &TestProtector),
            Err("binding_changed")
        );
        assert!(matches!(
            prepare(store, &old.1, &[7u8; 32]),
            Err("binding_changed")
        ));
    }
    let fresh = prepare(&store, &current.1, &[8u8; 32]).unwrap().unwrap();
    store
        .commit_candidate(&fresh, &[8u8; 32], &TestProtector)
        .unwrap();
    assert_eq!(
        store
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &[8u8; 32]
    );
}

#[test]
fn exhausted_replacement_revision_leaves_trust_tokens_and_pending_proof_unchanged() {
    for field in ["credential_revision", "connection_revision"] {
        let (_, fs) = setup();
        let mut bytes = fs.bytes.lock().unwrap();
        let mut state: serde_json::Value = serde_json::from_slice(bytes.as_ref().unwrap()).unwrap();
        state["record"][field] = serde_json::json!(u64::MAX);
        *bytes = Some(serde_json::to_vec(&state).unwrap());
        drop(bytes);
        let store = reopen(&fs);
        let before = tokens(&store);
        let mut replacement = trust();
        replacement.enrollment_generation = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
        let proof = enrollment_proof(&store, &replacement);
        let persisted = fs.bytes.lock().unwrap().clone();
        let memory = serde_json::to_vec(&*store.state.lock().unwrap()).unwrap();
        assert_eq!(
            store.replace_trust(trust(), replacement, &proof, "S-1-5-18"),
            Err("revision_exhausted")
        );
        assert_eq!(tokens(&store), before);
        assert_eq!(
            serde_json::to_vec(&*store.state.lock().unwrap()).unwrap(),
            memory
        );
        assert_eq!(*fs.bytes.lock().unwrap(), persisted);
        assert_eq!(tokens(&reopen(&fs)), before);
    }
}
