// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn paths() -> (String, String) {
    let id = uuid::Uuid::new_v4();
    (format!(r"SOFTWARE\WinCommanderActivationIntentTest-{id}"),
     format!(r"SOFTWARE\WinCommanderActivationGateTest-{id}"))
}

fn example_pending() -> PendingActivation {
    let credential_id = [4; 32];
    let credential_secret = [5; 32];
    PendingActivation { operation_id: "b".repeat(64), licence_key: "secret".into(),
        device_hash: "a".repeat(64), portable: false, credential_id, credential_secret,
        credential_commitment: credential_commitment(&credential_id, &credential_secret), reactivation: None }
}

#[test]
fn commitment_matches_wire_vector_without_disclosing_secret() {
    let pending = example_pending();
    assert_eq!(pending.credential_commitment, "fdef1206908c31b0f3b9ecce79bfb437e32deabe702d7f6fbf529b14f44a136f");
    let body = pending.request_body().unwrap();
    assert!(!String::from_utf8_lossy(&body).contains(&hex_bytes(&pending.credential_secret)));
    assert_ne!(pending.credential_commitment, credential_commitment(&pending.credential_id, &[6; 32]));
}

#[test]
fn reactivation_witness_matches_cross_runtime_vector_and_binds_operation() {
    let mut pending = example_pending();
    let previous = PreviousCredential { id: &pending.credential_id, secret: &pending.credential_secret,
        licence_id: "licence", generation: 7 };
    let witness = reactivation_witness(&pending.operation_id, &pending.device_hash, &previous).unwrap();
    assert_eq!(witness.proof, "kao9Ske4UPTEMa76Bh79GhyzOwAkPg7Rwv1QmIG9EDE");
    pending.reactivation = Some(witness);
    let request = pending.request_body().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(json["reactivationProof"], "kao9Ske4UPTEMa76Bh79GhyzOwAkPg7Rwv1QmIG9EDE");
    assert_eq!(serde_json::from_str::<serde_json::Value>(json["reactivationPayload"].as_str().unwrap()).unwrap()["generation"], 7);
    pending.operation_id = "c".repeat(64);
    assert!(pending.request_body().is_err());
}

#[test]
fn reactivation_witness_survives_restart_with_same_protected_credential() {
    let (path, gate_path) = paths();
    let store = Anchor::test_user(path.clone());
    let gate = Anchor::test_user(gate_path);
    let id = [4; 32]; let secret = [5; 32];
    let old = PreviousCredential { id: &id, secret: &secret, licence_id: "licence", generation: 7 };
    let first = { let _lock = crate::licence_security_lock().unwrap();
        begin_locked(&store, &gate, &MachineDpapi, "secret", &"a".repeat(64), false, Some(old)).unwrap() };
    assert_eq!(first.credential_id, id);
    assert_eq!(first.credential_secret, secret);
    let original = first.request_body().unwrap();
    let reopened = load_at(&Anchor::test_user(path.clone()), &MachineDpapi).unwrap().unwrap();
    assert_eq!(reopened.request_body().unwrap().as_slice(), original.as_slice());
    let mut forged = reopened;
    forged.reactivation.as_mut().unwrap().proof = "A".repeat(43);
    assert!(forged.request_body().is_err());
    super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
}

#[test]
fn protected_pending_intent_reopens_exactly_and_refuses_replacement() {
    let (path, gate_path) = paths();
    let store = Anchor::test_user(path.clone());
    let gate = Anchor::test_user(gate_path.clone());
    let device = "a".repeat(64);
    let first = begin_at(&store, &gate, &MachineDpapi, "secret-licence-key", &device, false).unwrap();
    assert!(valid_hex(first.operation_id()));
    assert_eq!(first.licence_key(), "secret-licence-key");
    let raw = store.read().unwrap().unwrap();
    assert!(!raw.windows("secret-licence-key".len()).any(|part| part == b"secret-licence-key"));
    assert!(ensure_idle_at(&store).is_err());
    let reopened = Anchor::test_user(path.clone());
    let next = begin_at(&reopened, &gate, &MachineDpapi, "secret-licence-key", &device, false).unwrap();
    assert_eq!(first.operation_id(), next.operation_id());
    let body = first.request_body().unwrap();
    assert_eq!(body.as_slice(), next.request_body().unwrap().as_slice());
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["operationId"], first.operation_id());
    assert_eq!(json["licenseKey"], "secret-licence-key");
    assert_eq!(json["deviceHash"], device);
    assert_eq!(json["appId"], "com.servalabs.wincommander");
    assert_eq!(json["isPortable"], false);
    assert_eq!(json["activationCredentialId"], hex_bytes(first.credential_id()));
    assert_eq!(json["activationCredentialCommitment"], first.credential_commitment);
    assert_eq!(json.as_object().unwrap().len(), 7);
    assert!(begin_at(&reopened, &gate, &MachineDpapi, "another-key", &device, false).is_err());
    assert!(begin_at(&reopened, &gate, &MachineDpapi, "secret-licence-key", &"b".repeat(64), false).is_err());
    assert!(begin_at(&reopened, &gate, &MachineDpapi, "secret-licence-key", &device, true).is_err());
    reopened.write(b"corrupt").unwrap();
    assert!(begin_at(&reopened, &gate, &MachineDpapi, "secret-licence-key", &device, false).is_err());
    assert!(ensure_idle_at(&reopened).is_err());
    super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
}

#[test]
fn release_gate_blocks_intent_before_any_request_is_created() {
    let (path, gate_path) = paths();
    let store = Anchor::test_user(path.clone());
    let gate = Anchor::test_user(gate_path.clone());
    release_gate::begin_at(&gate, &[3; 32]).unwrap();
    assert!(begin_at(&store, &gate, &MachineDpapi, "secret", &"a".repeat(64), false).is_err());
    assert!(!store.exists().unwrap());
    super::super::license_cache::licence_device_anchor::remove_test_key(&gate_path).unwrap();
}

#[test]
fn completed_intent_never_reissues_same_operation_after_lost_reply() {
    let (path, gate_path) = paths();
    let store = Anchor::test_user(path.clone());
    let gate = Anchor::test_user(gate_path);
    let device = "a".repeat(64);
    let pending = begin_at(&store, &gate, &MachineDpapi, "unique-retry-key-123", &device, false).unwrap();
    {
        let _lock = crate::licence_security_lock().unwrap();
        complete_at(&store, &MachineDpapi, &pending, b"signed payload", "signed receipt").unwrap();
    }
    assert!(ensure_idle_at(&store).is_ok());
    let bytes = store.read().unwrap().unwrap();
    assert!(!bytes.windows(20).any(|part| part == b"unique-retry-key-123"));
    let reopened = Anchor::test_user(path.clone());
    let retry = begin_at(&reopened, &gate, &MachineDpapi, "unique-retry-key-123", &device, false);
    assert!(matches!(retry, Err("licence activation already completed; check status")));
    assert!(begin_at(&reopened, &gate, &MachineDpapi, "different", &device, false).is_err());
    super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
}

#[test]
fn signed_response_must_bind_exact_pending_operation_and_machine() {
    let mut pending = example_pending();
    let claims = serde_json::json!({"license_id":"licence", "device_hash":pending.device_hash,
        "activation_operation_id":pending.operation_id, "activation_generation":1,
        "activation_credential_id":hex_bytes(&pending.credential_id), "activation_credential_commitment":pending.credential_commitment,
        "is_portable":false, "plan":"pro", "iss":"wincommander-license-worker",
        "sub":"com.servalabs.wincommander", "iat":100, "exp":200});
    let body = |payload: serde_json::Value| serde_json::to_vec(&serde_json::json!({"ok":true,
        "payload":payload.to_string(), "signature":"signed", "hybrid":"hybrid", "seatsUsed":1, "seatLimit":3})).unwrap();
    let verify = |_: &str, signature: &str, hybrid: &str| {
        if signature == "signed" && hybrid == "hybrid" { Ok(()) } else { Err(INVALID) }
    };
    let accepted = verify_response_with(&pending, &body(claims.clone()), 150, verify).unwrap();
    assert_eq!(accepted.seats_used, Some(1));
    for (field, value) in [
        ("activation_operation_id", serde_json::json!("c".repeat(64))),
        ("device_hash", serde_json::json!("d".repeat(64))),
        ("activation_generation", serde_json::json!(0)),
        ("activation_credential_id", serde_json::json!("f".repeat(64))),
        ("activation_credential_commitment", serde_json::json!("f".repeat(64))),
        ("is_portable", serde_json::json!(true)),
        ("plan", serde_json::json!("trial")),
        ("iss", serde_json::json!("other")),
        ("sub", serde_json::json!("other")),
        ("exp", serde_json::json!(149)),
    ] {
        let mut changed = claims.clone(); changed[field] = value;
        assert!(verify_response_with(&pending, &body(changed), 150, verify).is_err(), "{field}");
    }
    let mut missing = claims.clone(); missing.as_object_mut().unwrap().remove("activation_operation_id");
    assert!(verify_response_with(&pending, &body(missing), 150, verify).is_err());
    assert!(verify_response_with(&pending, &body(claims.clone()), 150, |_, _, _| Err(INVALID)).is_err());
    let previous = PreviousCredential { id: &pending.credential_id, secret: &pending.credential_secret,
        licence_id: "licence", generation: 7 };
    pending.reactivation = Some(reactivation_witness(&pending.operation_id, &pending.device_hash, &previous).unwrap());
    let mut same_generation = claims.clone(); same_generation["activation_generation"] = serde_json::json!(7);
    assert!(verify_response_with(&pending, &body(same_generation), 150, verify).is_err());
    let mut new_generation = claims; new_generation["activation_generation"] = serde_json::json!(8);
    assert!(verify_response_with(&pending, &body(new_generation.clone()), 150, verify).is_ok());
    new_generation["license_id"] = serde_json::json!("other-licence");
    assert!(verify_response_with(&pending, &body(new_generation), 150, verify).is_err());
}

#[test]
fn real_classical_and_hybrid_signatures_bind_activation_operation() {
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};
    use wincmd_pq_experiment::{jws, Purpose, VerifyError};
    let pending = example_pending();
    let classical = SigningKey::from_bytes(&[7; 32]);
    let ed = SigningKey::from_bytes(&[8; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let ed_public = ed.verifying_key().to_bytes();
    let pq_public = pq.public_key().as_ref().try_into().unwrap();
    let trust = jws::Trust { purpose: Purpose::Licence, minimum_epoch: 1, ed_key_id: "ed",
        ed25519_public: &ed_public, pq_key_id: "pq", ml_dsa_65_public: pq_public };
    let payload = serde_json::json!({"license_id":"licence", "device_hash":pending.device_hash,
        "activation_operation_id":pending.operation_id, "activation_generation":1,
        "activation_credential_id":hex_bytes(&pending.credential_id), "activation_credential_commitment":pending.credential_commitment,
        "is_portable":false, "plan":"pro", "iss":"wincommander-license-worker",
        "sub":"com.servalabs.wincommander", "iat":100, "exp":200}).to_string();
    let signature = URL_SAFE_NO_PAD.encode(classical.sign(payload.as_bytes()).to_bytes());
    let hybrid = String::from_utf8(jws::sign::assemble(payload.as_bytes(), 1, &trust,
        |message| Ok(ed.sign(message).to_bytes().to_vec()),
        |message| { let mut value = vec![0; 3309]; pq.sign(message, &mut value)
            .map_err(|_| VerifyError::InvalidSignature)?; Ok(value) }).unwrap()).unwrap();
    let response = |content: &str, signature: &str, hybrid: &str| serde_json::to_vec(&serde_json::json!({
        "ok":true, "payload":content, "signature":signature, "hybrid":hybrid})).unwrap();
    let public = URL_SAFE_NO_PAD.encode(classical.verifying_key().to_bytes());
    let verify = |p: &str, s: &str, h: &str| crate::license_cache::verify_activation_envelope_with(p, s, h, &public, &trust);
    assert!(verify_response_with(&pending, &response(&payload, &signature, &hybrid), 150, verify).is_ok());
    let altered = payload.replace(&"b".repeat(64), &"c".repeat(64));
    assert!(verify_response_with(&pending, &response(&altered, &signature, &hybrid), 150, verify).is_err());
    assert!(verify_response_with(&pending, &response(&payload, &URL_SAFE_NO_PAD.encode([0u8; 64]), &hybrid), 150, verify).is_err());
}

#[test]
fn concurrent_begins_share_one_operation_id() {
    let (path, gate_path) = paths();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2).map(|_| {
        let path = path.clone(); let gate_path = gate_path.clone(); let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            begin_at(&Anchor::test_user(path), &Anchor::test_user(gate_path), &MachineDpapi,
                "same-key", &"a".repeat(64), false).unwrap().operation_id().to_owned()
        })
    }).collect();
    let ids: Vec<_> = handles.into_iter().map(|handle| handle.join().unwrap()).collect();
    assert_eq!(ids[0], ids[1]);
    super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
}

#[test]
fn activation_and_release_cannot_both_start() {
    let (path, gate_path) = paths();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let activation = {
        let path = path.clone(); let gate_path = gate_path.clone(); let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            begin_at(&Anchor::test_user(path), &Anchor::test_user(gate_path), &MachineDpapi,
                "race-key", &"a".repeat(64), false).is_ok()
        })
    };
    let release = {
        let path = path.clone(); let gate_path = gate_path.clone(); let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let _lock = crate::licence_security_lock().unwrap();
            ensure_idle_at(&Anchor::test_user(path))
                .and_then(|()| release_gate::begin_at(&Anchor::test_user(gate_path), &[5; 32])).is_ok()
        })
    };
    assert_ne!(activation.join().unwrap(), release.join().unwrap());
    if Anchor::test_user(path.clone()).exists().unwrap() {
        super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
    }
    if Anchor::test_user(gate_path.clone()).exists().unwrap() {
        super::super::license_cache::licence_device_anchor::remove_test_key(&gate_path).unwrap();
    }
}

#[test]
fn competing_settlements_do_not_overlap_and_failure_releases_lock() {
    use std::sync::{Arc, Barrier, atomic::{AtomicUsize, Ordering}};
    let active = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2).map(|_| {
        let active = Arc::clone(&active);
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            with_settlement_lock(|| {
                assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                std::thread::sleep(std::time::Duration::from_millis(50));
                assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                Err::<(), _>(INVALID)
            }).unwrap_err();
        })
    }).collect();
    for thread in threads { thread.join().unwrap(); }
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert!(with_settlement_lock(|| Ok(())).is_ok());
}

#[test]
fn settlement_lock_is_shared_across_processes() {
    const READY: &str = "WINCMD_TEST_SETTLEMENT_READY";
    if let Ok(path) = std::env::var(READY) {
        std::fs::write(path, b"ready").unwrap();
        with_settlement_lock(|| Ok(())).unwrap();
        return;
    }
    let path = std::env::temp_dir().join(format!("wincommander-settlement-{}", uuid::Uuid::new_v4()));
    let mut child = with_settlement_lock(|| {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "licence_activation_intent::tests::settlement_lock_is_shared_across_processes"])
            .env(READY, &path).spawn().unwrap();
        let start = std::time::Instant::now();
        while !path.exists() && start.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(path.exists(), "child did not start");
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(child.try_wait().unwrap().is_none(), "child bypassed the settlement lock");
        Ok(child)
    }).unwrap();
    assert!(child.wait().unwrap().success());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn pending_intent_reopens_in_another_process() {
    const STORE: &str = "WINCMD_TEST_ACTIVATION_INTENT_PATH";
    const GATE: &str = "WINCMD_TEST_ACTIVATION_GATE_PATH";
    const EXPECTED: &str = "WINCMD_TEST_ACTIVATION_OPERATION";
    if let (Ok(path), Ok(gate_path), Ok(expected)) = (std::env::var(STORE), std::env::var(GATE), std::env::var(EXPECTED)) {
        let pending = begin_at(&Anchor::test_user(path), &Anchor::test_user(gate_path), &MachineDpapi,
            "restart-key", &"a".repeat(64), false).unwrap();
        assert_eq!(pending.operation_id(), expected);
        return;
    }
    let (path, gate_path) = paths();
    let first = begin_at(&Anchor::test_user(path.clone()), &Anchor::test_user(gate_path.clone()), &MachineDpapi,
        "restart-key", &"a".repeat(64), false).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "licence_activation_intent::tests::pending_intent_reopens_in_another_process"])
        .env(STORE, &path).env(GATE, &gate_path).env(EXPECTED, first.operation_id())
        .output().unwrap();
    super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
    assert!(child.status.success());
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}
