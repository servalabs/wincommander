// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn enrollment_proof(store: &FleetMachinePolicy, binding: &FleetTrust) -> String {
    let (_, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let challenge = store
        .begin_enrollment(
            binding.device_id.clone(),
            binding.signer_key.clone(),
            origins,
            profile,
        )
        .unwrap();
    let bytes = serde_json::to_vec(&[
        "wincommander:shield-progress:enrollment:v1",
        &binding.org_id,
        &binding.device_id,
        &binding.enrollment_generation,
        &challenge,
    ])
    .unwrap();
    STANDARD.encode(key().sign(&bytes).to_bytes())
}
fn tokens(store: &FleetMachinePolicy) -> (String, String) {
    (
        store.credential_clear_token().unwrap().unwrap(),
        store.connection_token().unwrap(),
    )
}
fn clear(store: &FleetMachinePolicy, pair: &(String, String)) -> bool {
    store.clear_credential(&pair.0, &pair.1).unwrap()
}
fn start(store: &FleetMachinePolicy, pair: &(String, String)) -> (String, String) {
    let (connection, clear) = store.start_connection(&trust(), &pair.0, &pair.1).unwrap();
    (clear, connection)
}
fn prepare(
    store: &FleetMachinePolicy,
    connection: &str,
    secret: &[u8],
) -> Result<Option<CandidateContext>, &'static str> {
    let (binding, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    store.prepare_candidate(
        (&binding, connection),
        &origins,
        &profile.unwrap(),
        &origins[0],
        secret,
        &TestProtector,
    )
}
fn reopen(fs: &MemoryFs) -> FleetMachinePolicy {
    FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("credential-test")).unwrap()
}

#[test]
fn lost_clear_reply_is_reconciled_after_restart_without_another_write() {
    let (store, fs) = setup();
    submit(&store, "https://fleet.test", &[1u8; 32]).unwrap();
    let pair = tokens(&store);
    assert!(clear(&store, &pair));
    let committed = fs.bytes.lock().unwrap().clone();
    let reopened = reopen(&fs);
    fs.fail.store(true, Ordering::SeqCst);
    assert!(clear(&reopened, &pair));
    assert!(!reopened.clear_credential("unrelated", &pair.1).unwrap());
    assert!(!reopened.clear_credential(&pair.0, "unrelated").unwrap());
    assert!(reopened
        .credential_snapshot(&TestProtector)
        .unwrap()
        .is_none());
    assert_eq!(*fs.bytes.lock().unwrap(), committed);
}

#[test]
fn replay_preserves_pending_rebind_but_cannot_clear_reenabled_or_new_credential() {
    let (store, fs) = setup();
    submit(&store, "https://fleet.test", &[1u8; 32]).unwrap();
    let old = tokens(&store);
    assert!(clear(&store, &old));
    let signature = enrollment_proof(&store, &trust());
    assert!(clear(&store, &old));
    store.bind_origins(trust(), &signature, "S-1-5-18").unwrap();
    assert!(!clear(&store, &old));
    submit(&store, "https://fleet.test", &[2u8; 32]).unwrap();
    let newer = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    assert!(!clear(&store, &old));
    assert!(store.credential_is_current(&newer));
    let current = tokens(&store);
    assert!(clear(&store, &current));
    let reopened = reopen(&fs);
    assert!(!clear(&reopened, &old));
    assert!(clear(&reopened, &current));
}

#[test]
fn replaced_trust_does_not_inherit_old_clear_acknowledgement() {
    let (store, _) = setup();
    let old = tokens(&store);
    assert!(clear(&store, &old));
    let mut replacement = trust();
    replacement.enrollment_generation = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
    let signature = enrollment_proof(&store, &replacement);
    store
        .replace_trust(trust(), replacement, &signature, "S-1-5-18")
        .unwrap();
    assert!(!clear(&store, &old));
    let new = tokens(&store);
    assert!(clear(&store, &new));
    assert!(!clear(&store, &old));
    assert!(clear(&store, &new));
}

#[test]
fn get_returns_tokens_and_nonsecret_trust_from_the_same_record() {
    let (store, _) = setup();
    submit(&store, "https://fleet.test", &[1u8; 32]).unwrap();
    let binding = trust();
    let get = crate::fleet_contact_transfer::handle(&store, serde_json::json!({"operation":"get"}))
        .unwrap();
    assert_eq!(
        get["serviceTrust"],
        serde_json::json!({
            "orgId":binding.org_id, "deviceId":binding.device_id,
            "enrollmentGeneration":binding.enrollment_generation, "signerKey":binding.signer_key,
        })
    );
    assert_eq!(get["deviceId"], get["serviceTrust"]["deviceId"]);
    assert_eq!(get["credential_clear_token"], tokens(&store).0);
    assert_eq!(get["connection_token"], tokens(&store).1);
    assert_eq!(get["serviceTrust"].as_object().unwrap().len(), 4);
    assert!(get.get("credential").is_none());
}

#[test]
fn simultaneous_duplicate_clears_commit_only_one_revision() {
    let (store, _) = setup();
    let pair = tokens(&store);
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            clear(&store, &pair)
        });
        barrier.wait();
        assert!(clear(&store, &pair));
        assert!(first.join().unwrap());
    });
    assert_eq!(
        store
            .state
            .lock()
            .unwrap()
            .record
            .as_ref()
            .unwrap()
            .credential_revision,
        1
    );
    assert!(clear(&store, &pair));
}

#[test]
fn concurrent_old_clear_and_rebind_leave_the_new_binding_enabled() {
    let (store, _) = setup();
    let pair = tokens(&store);
    assert!(clear(&store, &pair));
    let signature = enrollment_proof(&store, &trust());
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let old_clear = scope.spawn(|| {
            barrier.wait();
            clear(&store, &pair)
        });
        barrier.wait();
        store.bind_origins(trust(), &signature, "S-1-5-18").unwrap();
        old_clear.join().unwrap();
    });
    assert!(store.trust_with_origins().unwrap().unwrap().4);
    submit(&store, "https://fleet.test", &[3u8; 32]).unwrap();
    let current = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    assert!(!clear(&store, &pair));
    assert!(store.credential_is_current(&current));
}

#[test]
fn disconnect_without_existing_credential_fences_inflight_and_delayed_offers() {
    let (store, fs) = setup();
    let binding = trust();
    let (_, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let profile = profile.unwrap();
    let secret = [9u8; 32];
    let before = store
        .prepare_candidate(
            (&binding, &store.connection_token().unwrap()),
            &origins,
            &profile,
            &origins[0],
            &secret,
            &TestProtector,
        )
        .unwrap()
        .unwrap();
    let token = store.credential_clear_token().unwrap().unwrap();
    let connection = store.connection_token().unwrap();
    assert!(store.clear_credential(&token, &connection).unwrap());
    assert_eq!(
        store.commit_candidate(&before, &secret, &TestProtector),
        Err("binding_changed")
    );
    assert!(matches!(
        store.prepare_candidate(
            (&binding, &store.connection_token().unwrap()),
            &origins,
            &profile,
            &origins[0],
            &secret,
            &TestProtector
        ),
        Err("service_disabled")
    ));
    let reopened =
        FleetMachinePolicy::open(Box::new(fs), PathBuf::from("credential-test")).unwrap();
    assert!(matches!(
        reopened.prepare_candidate(
            (&binding, &reopened.connection_token().unwrap()),
            &origins,
            &profile,
            &origins[0],
            &secret,
            &TestProtector
        ),
        Err("service_disabled")
    ));

    let challenge = reopened
        .begin_enrollment(
            binding.device_id.clone(),
            binding.signer_key.clone(),
            origins.clone(),
            Some(profile.clone()),
        )
        .unwrap();
    let bytes = serde_json::to_vec(&[
        "wincommander:shield-progress:enrollment:v1",
        &binding.org_id,
        &binding.device_id,
        &binding.enrollment_generation,
        &challenge,
    ])
    .unwrap();
    let signature = STANDARD.encode(key().sign(&bytes).to_bytes());
    reopened
        .bind_origins(binding.clone(), &signature, "S-1-5-18")
        .unwrap();
    let current = reopened
        .prepare_candidate(
            (&binding, &reopened.connection_token().unwrap()),
            &origins,
            &profile,
            &origins[0],
            &secret,
            &TestProtector,
        )
        .unwrap()
        .unwrap();
    reopened
        .commit_candidate(&current, &secret, &TestProtector)
        .unwrap();
    assert!(
        !reopened.clear_credential(&token, &connection).unwrap(),
        "old disconnect cannot clear a new enrollment"
    );
    assert_eq!(
        reopened
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &secret
    );
}

#[path = "fleet_credential_connection_tests.rs"]
mod connection_tests;
