// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn paths() -> (String, String) {
    let id = uuid::Uuid::new_v4();
    (format!(r"SOFTWARE\WinCommanderTrialIntentTest-{id}"), format!(r"SOFTWARE\WinCommanderTrialGateTest-{id}"))
}

#[test]
fn trial_credential_commitment_matches_activation_vector() {
    let pending = PendingTrial { operation_id: "b".repeat(64), device_hash: "a".repeat(64),
        credential_id: [4; 32], credential_secret: [5; 32] };
    assert_eq!(pending.credential_commitment(), "fdef1206908c31b0f3b9ecce79bfb437e32deabe702d7f6fbf529b14f44a136f");
    let request = pending.request_body().unwrap();
    assert_eq!(request.as_slice(), include_str!("fixtures/rust-trial-v4-request.json").trim_end().as_bytes());
    let json: serde_json::Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(json["deviceHash"], pending.device_hash);
    assert_eq!(json["operationId"], pending.operation_id);
    assert_eq!(json["trialCredentialId"], "04".repeat(32));
    assert_eq!(json["trialCredentialCommitment"], pending.credential_commitment());
    assert_eq!(json.as_object().unwrap().len(), 6);
    assert!(!String::from_utf8_lossy(&request).contains(&"05".repeat(32)));
}

#[test]
fn trial_response_must_bind_identity_operation_credential_and_fixed_expiry() {
    let pending = PendingTrial { operation_id: "b".repeat(64), device_hash: "a".repeat(64),
        credential_id: [4; 32], credential_secret: [5; 32] };
    let exp = 100 + 16 * 24 * 60 * 60;
    let claims = serde_json::json!({ "license_id":format!("trial-{}", &pending.device_hash[..16]),
        "device_hash":pending.device_hash, "plan":"trial", "features":["paid"], "base_features":[],
        "service_features":["paid"], "service_exp":exp, "iat":100, "exp":exp, "license_exp":exp,
        "iss":"wincommander-license-worker", "sub":"com.servalabs.wincommander", "is_portable":false,
        "trial_operation_id":pending.operation_id, "trial_credential_id":"04".repeat(32),
        "trial_credential_commitment":pending.credential_commitment() });
    let body = |payload: serde_json::Value| serde_json::to_vec(&serde_json::json!({ "ok":true,
        "payload":payload.to_string(), "signature":"classical", "hybrid":"hybrid" })).unwrap();
    let verify = |_: &str, classical: &str, hybrid: &str| {
        if classical == "classical" && hybrid == "hybrid" { Ok(()) } else { Err(INVALID) }
    };
    assert!(verify_response_with(&pending, &body(claims.clone()), 150, verify).is_ok());
    for (field, value) in [
        ("device_hash", serde_json::json!("c".repeat(64))),
        ("trial_operation_id", serde_json::json!("c".repeat(64))),
        ("trial_credential_id", serde_json::json!("c".repeat(64))),
        ("trial_credential_commitment", serde_json::json!("c".repeat(64))),
        ("license_id", serde_json::json!("other")),
        ("plan", serde_json::json!("pro")),
        ("features", serde_json::json!(["advanced"])),
        ("is_portable", serde_json::json!(true)),
        ("exp", serde_json::json!(exp + 1)),
    ] {
        let mut altered = claims.clone(); altered[field] = value;
        assert!(verify_response_with(&pending, &body(altered), 150, verify).is_err(), "{field}");
    }
    assert!(verify_response_with(&pending, &body(claims), 150, |_, _, _| Err(INVALID)).is_err());
}

#[test]
fn trial_settlement_candidate_binds_exact_payload_and_credential() {
    let pending = PendingTrial { operation_id: "b".repeat(64), device_hash: "a".repeat(64),
        credential_id: [4; 32], credential_secret: [5; 32] };
    let trial = |payload: &str| VerifiedTrial { payload: payload.into(), operation_id: pending.operation_id.clone(),
        device_hash: pending.device_hash.clone(), credential_id: pending.credential_id,
        credential_commitment: pending.credential_commitment(), expires_at: 200 };
    let request = settlement_request(&pending, &trial(r#"{"trial":"sample"}"#), 100).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(value["proof"], "VcxsYGX4mkSq7mlhRssBAPi19rA5KAD2_EZmVh5MCvc");
    let payload: serde_json::Value = serde_json::from_str(value["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["trial_payload_digest"], "d728a502237296cc1797ad7c6a43c3fd31b009df9f929a650b306e92f16b0003");
    assert_eq!(payload["credential_id"], URL_SAFE_NO_PAD.encode([4; 32]));
    let changed = settlement_request(&pending, &trial(r#"{"trial":"other"}"#), 100).unwrap();
    assert_ne!(request.as_slice(), changed.as_slice());
    assert!(settlement_request(&pending, &trial(""), 100).is_err());
    assert!(settlement_request(&pending, &trial("valid"), 200).is_err());
}

#[test]
fn trial_retirement_candidate_binds_operation_and_fresh_id() {
    let pending = PendingTrial { operation_id: "b".repeat(64), device_hash: "a".repeat(64),
        credential_id: [4; 32], credential_secret: [5; 32] };
    let request = retirement_request(&pending, &"c".repeat(64)).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(value["proof"], "hPu-0obRVwtd5HRwKQxMOWKSzouRbDOxeeaH58gW-Yw");
    let claims: serde_json::Value = serde_json::from_str(value["payload"].as_str().unwrap()).unwrap();
    assert_eq!(claims["retirement_id"], "c".repeat(64));
    assert_eq!(claims["operation_id"], pending.operation_id);
    assert!(retirement_request(&pending, "invalid").is_err());
    assert_ne!(request.as_slice(), retirement_request(&pending, &"d".repeat(64)).unwrap().as_slice());
}

#[test]
fn trial_response_requires_real_classical_and_hybrid_signatures() {
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};
    use wincmd_pq_experiment::{jws, Purpose, VerifyError};
    let pending = PendingTrial { operation_id: "b".repeat(64), device_hash: "a".repeat(64),
        credential_id: [4; 32], credential_secret: [5; 32] };
    let exp = 100 + 16 * 24 * 60 * 60;
    let payload = serde_json::json!({"license_id":format!("trial-{}", &pending.device_hash[..16]),
        "device_hash":pending.device_hash, "plan":"trial", "features":["paid"], "base_features":[],
        "service_features":["paid"], "service_exp":exp, "iat":100, "exp":exp, "license_exp":exp,
        "iss":"wincommander-license-worker", "sub":"com.servalabs.wincommander", "is_portable":false,
        "trial_operation_id":pending.operation_id, "trial_credential_id":"04".repeat(32),
        "trial_credential_commitment":pending.credential_commitment() }).to_string();
    let classical = SigningKey::from_bytes(&[7; 32]);
    let ed = SigningKey::from_bytes(&[8; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let ed_public = ed.verifying_key().to_bytes();
    let pq_public = pq.public_key().as_ref().try_into().unwrap();
    let trust = jws::Trust { purpose: Purpose::Licence, minimum_epoch: 1, ed_key_id: "ed",
        ed25519_public: &ed_public, pq_key_id: "pq", ml_dsa_65_public: pq_public };
    let signature = URL_SAFE_NO_PAD.encode(classical.sign(payload.as_bytes()).to_bytes());
    let hybrid = String::from_utf8(jws::sign::assemble(payload.as_bytes(), 1, &trust,
        |message| Ok(ed.sign(message).to_bytes().to_vec()),
        |message| { let mut value = vec![0; 3309]; pq.sign(message, &mut value)
            .map_err(|_| VerifyError::InvalidSignature)?; Ok(value) }).unwrap()).unwrap();
    let public = URL_SAFE_NO_PAD.encode(classical.verifying_key().to_bytes());
    let body = |classical_signature: &str, hybrid_signature: &str| serde_json::to_vec(&serde_json::json!({
        "ok":true, "payload":payload, "signature":classical_signature, "hybrid":hybrid_signature})).unwrap();
    let verify = |p: &str, s: &str, h: &str| crate::license_cache::verify_activation_envelope_with(p, s, h, &public, &trust);
    assert!(verify_response_with(&pending, &body(&signature, &hybrid), 150, verify).is_ok());
    assert!(verify_response_with(&pending, &body(&URL_SAFE_NO_PAD.encode([0u8; 64]), &hybrid), 150, verify).is_err());
    assert!(verify_response_with(&pending, &body(&signature, "bad"), 150, verify).is_err());
}

#[test]
fn protected_trial_intent_is_stable_and_fails_closed() {
    let (path, gate_path) = paths();
    let store = Anchor::test_user(path.clone());
    let gate = Anchor::test_user(gate_path.clone());
    let device = "a".repeat(64);
    assert!(ensure_idle_at(&store, &MachineDpapi).is_ok());
    assert!(begin_at(&store, &gate, &MachineDpapi, &device, || Err(INVALID)).is_err());
    assert!(!store.exists().unwrap());
    let first = begin_at(&store, &gate, &MachineDpapi, &device, || Ok(())).unwrap();
    assert!(ensure_idle_at(&store, &MachineDpapi).is_err());
    let bytes = store.read().unwrap().unwrap();
    assert!(!bytes.windows(32).any(|part| part == first.credential_secret));
    let reopened = Anchor::test_user(path.clone());
    let second = begin_at(&reopened, &gate, &MachineDpapi, &device, || Ok(())).unwrap();
    assert_eq!(first.operation_id, second.operation_id);
    assert_eq!(first.credential_id, second.credential_id);
    assert_eq!(first.credential_secret, second.credential_secret);
    assert!(begin_at(&reopened, &gate, &MachineDpapi, &"b".repeat(64), || Ok(())).is_err());
    assert_eq!(store.read().unwrap().unwrap(), bytes);
    reopened.write(b"corrupt").unwrap();
    assert!(ensure_idle_at(&reopened, &MachineDpapi).is_err());
    assert!(begin_at(&reopened, &gate, &MachineDpapi, &device, || Ok(())).is_err());
    super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
}

#[test]
fn trial_intent_does_not_begin_during_release() {
    let (path, gate_path) = paths();
    let store = Anchor::test_user(path);
    let gate = Anchor::test_user(gate_path.clone());
    release_gate::begin_at(&gate, &[3; 32]).unwrap();
    assert!(begin_at(&store, &gate, &MachineDpapi, &"a".repeat(64), || Ok(())).is_err());
    assert!(!store.exists().unwrap());
    super::super::license_cache::licence_device_anchor::remove_test_key(&gate_path).unwrap();
}

#[test]
fn trial_retry_after_process_restart_uses_the_original_intent() {
    const STORE: &str = "WINCMD_TEST_TRIAL_INTENT_PATH";
    const GATE: &str = "WINCMD_TEST_TRIAL_GATE_PATH";
    const EXPECTED: &str = "WINCMD_TEST_TRIAL_OPERATION";
    if let (Ok(path), Ok(gate), Ok(expected)) = (std::env::var(STORE), std::env::var(GATE), std::env::var(EXPECTED)) {
        let pending = begin_at(&Anchor::test_user(path), &Anchor::test_user(gate), &MachineDpapi, &"a".repeat(64), || Ok(())).unwrap();
        assert_eq!(pending.operation_id, expected);
        return;
    }
    let (path, gate_path) = paths();
    let pending = begin_at(&Anchor::test_user(path.clone()), &Anchor::test_user(gate_path.clone()),
        &MachineDpapi, &"a".repeat(64), || Ok(())).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "licence_trial_intent::tests::trial_retry_after_process_restart_uses_the_original_intent"])
        .env(STORE, &path).env(GATE, &gate_path).env(EXPECTED, &pending.operation_id).output().unwrap();
    super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
    assert!(child.status.success());
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}
