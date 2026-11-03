// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{contact_test_support::*, *};
use crate::fleet_credential::Protector;
use ed25519_dalek::Signer;
use std::{path::PathBuf, sync::atomic::Ordering};
use zeroize::Zeroizing;

pub(crate) struct TestProtector;
impl Protector for TestProtector {
    fn protect(&self, plaintext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, &'static str> {
        Ok(plaintext.iter().zip(entropy).map(|(a, b)| a ^ b).collect())
    }
    fn unprotect(
        &self,
        ciphertext: &[u8],
        entropy: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        Ok(Zeroizing::new(
            ciphertext.iter().zip(entropy).map(|(a, b)| a ^ b).collect(),
        ))
    }
}

fn setup() -> (FleetMachinePolicy, MemoryFs) {
    let fs = MemoryFs::default();
    let store =
        FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("credential-test")).unwrap();
    let binding = trust();
    let profile = TransportProfile {
        spki_pins: String::new(),
        environment_fingerprint: STANDARD.encode([9u8; 32]),
    };
    let challenge = store
        .begin_enrollment(
            binding.device_id.clone(),
            binding.signer_key.clone(),
            vec!["https://fleet.test".into()],
            Some(profile),
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
    store.provision(binding, &signature, "S-1-5-18").unwrap();
    (store, fs)
}

pub(crate) fn setup_store() -> FleetMachinePolicy {
    setup().0
}

fn submit(store: &FleetMachinePolicy, origin: &str, secret: &[u8]) -> Result<u64, &'static str> {
    let binding = trust();
    let profile = TransportProfile {
        spki_pins: String::new(),
        environment_fingerprint: STANDARD.encode([9u8; 32]),
    };
    let origins = vec!["https://fleet.test".to_string()];
    let context = store
        .prepare_candidate(&binding, &origins, &profile, origin, secret, &TestProtector)?
        .ok_or("already_current")?;
    store.commit_candidate(&context, secret, &TestProtector)
}

#[test]
fn verified_credential_survives_restart_and_rotation_fences_stale_work() {
    let (store, fs) = setup();
    let one = [1u8; 32];
    let two = [2u8; 32];
    assert_eq!(
        submit(&store, "https://wrong.test", &one),
        Err("transport_not_authorized")
    );
    assert!(store.credential_snapshot(&TestProtector).unwrap().is_none());
    submit(&store, "https://fleet.test", &one).unwrap();
    let first = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    let first_token = store.credential_clear_token().unwrap().unwrap();
    assert_eq!(first.secret.as_slice(), &one);
    let reopened =
        FleetMachinePolicy::open(Box::new(fs.clone()), PathBuf::from("credential-test")).unwrap();
    assert_eq!(
        reopened
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &one
    );
    assert_eq!(
        submit(&store, "https://fleet.test", &one),
        Err("already_current")
    );
    assert!(
        store.credential_is_current(&first),
        "same secret must not invalidate in-flight work"
    );
    submit(&store, "https://fleet.test", &two).unwrap();
    assert!(!store.credential_is_current(&first));
    assert!(
        !store.clear_credential(&first_token).unwrap(),
        "old clear request must not erase new credential"
    );
    let current = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    let token = store.credential_clear_token().unwrap().unwrap();
    fs.fail.store(true, Ordering::SeqCst);
    assert!(store.clear_credential(&token).is_err());
    assert!(store.credential_is_current(&current));
    fs.fail.store(false, Ordering::SeqCst);
    assert!(store.clear_credential(&token).unwrap());
    assert!(store.credential_snapshot(&TestProtector).unwrap().is_none());
}

#[test]
fn changed_runtime_origin_or_proxy_profile_can_only_suspend_service_transport() {
    let (store, _) = setup();
    let key = [4u8; 32];
    submit(&store, "https://fleet.test", &key).unwrap();
    let first = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    let mut changed_profile = first.profile.clone();
    changed_profile.environment_fingerprint = STANDARD.encode([8u8; 32]);
    assert!(!store
        .observe_runtime_transport(&first.trust, &first.origins, Some(&changed_profile))
        .unwrap());
    assert!(!store.credential_is_current(&first));
    assert!(matches!(
        store.credential_snapshot(&TestProtector),
        Err("network_configuration_differs")
    ));
    assert_eq!(
        store.credential_missing_reason(),
        "network_configuration_differs"
    );
    assert!(store
        .observe_runtime_transport(&first.trust, &first.origins, Some(&first.profile))
        .unwrap());
    submit(&store, "https://fleet.test", &key).unwrap();
    let second = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    assert!(!store
        .observe_runtime_transport(
            &second.trust,
            &["https://other.test".into()],
            Some(&second.profile)
        )
        .unwrap());
    assert!(!store.credential_is_current(&second));
    assert_eq!(
        store.authorized_origins().unwrap().unwrap(),
        vec!["https://fleet.test"]
    );
}

#[test]
fn credential_write_failure_retains_the_previous_revision() {
    let (store, fs) = setup();
    let first = [1u8; 32];
    let second = [2u8; 32];
    submit(&store, "https://fleet.test", &first).unwrap();
    let before = store.credential_snapshot(&TestProtector).unwrap().unwrap();
    fs.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        submit(&store, "https://fleet.test", &second),
        Err("store_unavailable")
    );
    assert!(store.credential_is_current(&before));
    fs.fail.store(false, Ordering::SeqCst);
    let reopened =
        FleetMachinePolicy::open(Box::new(fs), PathBuf::from("credential-test")).unwrap();
    assert_eq!(
        reopened
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &first
    );
}

#[test]
fn late_verified_candidate_cannot_replace_a_newer_credential() {
    let (store, _) = setup();
    let binding = trust();
    let profile = TransportProfile {
        spki_pins: String::new(),
        environment_fingerprint: STANDARD.encode([9u8; 32]),
    };
    let origins = vec!["https://fleet.test".to_string()];
    let first = store
        .prepare_candidate(
            &binding,
            &origins,
            &profile,
            &origins[0],
            &[1u8; 32],
            &TestProtector,
        )
        .unwrap()
        .unwrap();
    let second = store
        .prepare_candidate(
            &binding,
            &origins,
            &profile,
            &origins[0],
            &[2u8; 32],
            &TestProtector,
        )
        .unwrap()
        .unwrap();
    store
        .commit_candidate(&second, &[2u8; 32], &TestProtector)
        .unwrap();
    assert_eq!(
        store.commit_candidate(&first, &[1u8; 32], &TestProtector),
        Err("binding_changed")
    );
    assert_eq!(
        store
            .credential_snapshot(&TestProtector)
            .unwrap()
            .unwrap()
            .secret
            .as_slice(),
        &[2u8; 32]
    );
}

#[path = "fleet_credential_disconnect_tests.rs"]
mod disconnect_tests;
