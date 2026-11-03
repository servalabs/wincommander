// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn disconnect_without_existing_credential_fences_inflight_and_delayed_offers() {
    let (store, fs) = setup();
    let binding = trust();
    let (_, _, origins, profile, _) = store.trust_with_origins().unwrap().unwrap();
    let profile = profile.unwrap();
    let secret = [9u8; 32];
    let before = store
        .prepare_candidate(
            &binding,
            &origins,
            &profile,
            &origins[0],
            &secret,
            &TestProtector,
        )
        .unwrap()
        .unwrap();
    let token = store.credential_clear_token().unwrap().unwrap();
    assert!(store.clear_credential(&token).unwrap());
    assert_eq!(
        store.commit_candidate(&before, &secret, &TestProtector),
        Err("binding_changed")
    );
    assert!(matches!(
        store.prepare_candidate(
            &binding,
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
            &binding,
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
            &binding,
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
        !reopened.clear_credential(&token).unwrap(),
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
