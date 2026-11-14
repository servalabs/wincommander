// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{contact_test_support::*, *};
use serde_json::{json, Value};
use std::time::Duration;

fn request_v2(nonce: &str) -> Value {
    let trust = trust();
    json!({"protocol_version":1,"device_id":trust.device_id,"nonce":nonce,"ts":1000,
        "capabilities":["windows_contact_receipt_v1"],"decoy":false,
        "windows_v2":{"version":2,"binding":{"org_id":trust.org_id,"device_id":trust.device_id,
            "enrollment_generation":trust.enrollment_generation,"authority_key_b64":trust.signer_key},
            "acks":[],"outcomes":[]}})
}

#[test]
fn signed_v2_contact_is_one_use_and_persists_only_historical_evidence() {
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let request = serde_json::to_vec(&request_v2(&nonce)).unwrap();
    let response = serde_json::to_vec(&response()).unwrap();
    assert_eq!(
        fixture
            .submit(&nonce, &request, &response, Duration::ZERO, 42)
            .unwrap()
            .observed_unix_ms,
        42
    );
    assert!(!fixture.store.contact_pending(&nonce));
    assert_eq!(
        fixture.submit(&nonce, &request, &response, Duration::ZERO, 43),
        Err("contact_not_pending")
    );
    let reopened =
        FleetMachinePolicy::open(Box::new(fixture.fs.clone()), PathBuf::from("contact-test"))
            .unwrap();
    assert_eq!(
        reopened
            .contact_observation()
            .unwrap()
            .unwrap()
            .1
            .observed_unix_ms,
        42
    );
}

#[test]
fn signed_v2_contact_rejects_mismatched_identity_shape_and_legacy_auth() {
    for field in [
        "org",
        "device",
        "outer_device",
        "generation",
        "signer",
        "nonce",
        "version",
        "protocol",
        "capability",
        "decoy",
        "time",
        "hmac",
        "hmac_version",
        "legacy_acks",
        "legacy_outcomes",
        "null_v2",
        "missing_v2",
    ] {
        let fixture = Fixture::new();
        let nonce = fixture.nonce();
        let mut request = request_v2(&nonce);
        match field {
            "org" => request["windows_v2"]["binding"]["org_id"] = json!("other"),
            "device" => request["windows_v2"]["binding"]["device_id"] = json!("other"),
            "outer_device" => request["device_id"] = json!("other"),
            "generation" => {
                request["windows_v2"]["binding"]["enrollment_generation"] =
                    json!("cccccccc-cccc-4ccc-8ccc-cccccccccccc")
            }
            "signer" => {
                request["windows_v2"]["binding"]["authority_key_b64"] = json!(STANDARD.encode(
                    ed25519_dalek::SigningKey::from_bytes(&[42; 32])
                        .verifying_key()
                        .as_bytes()
                ))
            }
            "nonce" => request["nonce"] = json!("ci-00000000000000000000000000000000"),
            "version" => request["windows_v2"]["version"] = json!(1),
            "protocol" => request["protocol_version"] = json!(0),
            "capability" => request["capabilities"] = json!([]),
            "decoy" => request["decoy"] = json!(true),
            "time" => request["ts"] = Value::Null,
            "hmac" => request["hmac"] = Value::Null,
            "hmac_version" => request["hmac_version"] = json!(2),
            "legacy_acks" => request["acks"] = json!([{}]),
            "legacy_outcomes" => request["action_outcomes"] = json!([{}]),
            "null_v2" => {
                request["windows_v2"] = Value::Null;
                request["hmac"] = json!(STANDARD.encode([7; 32]));
                request["hmac_version"] = json!(2);
            }
            _ => {
                request.as_object_mut().unwrap().remove("windows_v2");
            }
        }
        let request = serde_json::to_vec(&request).unwrap();
        let response = serde_json::to_vec(&response()).unwrap();
        assert!(
            fixture
                .submit(&nonce, &request, &response, Duration::ZERO, 1)
                .is_err(),
            "{field}"
        );
        assert_eq!(
            fixture.store.contact_observation().unwrap(),
            None,
            "{field}"
        );
        assert!(!fixture.store.contact_pending(&nonce));
    }
}

#[test]
fn v2_receipt_binds_exact_bytes_and_does_not_reinterpret_sentinel_keys() {
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let mut request = serde_json::to_vec(&request_v2(&nonce)).unwrap();
    assert_eq!(request.pop(), Some(b'}'));
    request.extend_from_slice(br#", "telemetry":{"$serde_json::private::RawValue":"123","$serde_json::private::Number":"9007199254740993"}}"#);
    let first_response = serde_json::to_vec(&response()).unwrap();
    assert!(fixture
        .submit(&nonce, &request, &first_response, Duration::ZERO, 1)
        .is_ok());

    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let mut request = serde_json::to_vec(&request_v2(&nonce)).unwrap();
    let response = serde_json::to_vec(&response()).unwrap();
    let signed = receipt(&nonce, &request, &response);
    request.push(b' ');
    assert!(fixture
        .store
        .record_contact_with_clock(&nonce, &request, &response, &signed, || (
            fixture.issued,
            Ok(1)
        ))
        .is_err());
    assert_eq!(fixture.store.contact_observation().unwrap(), None);
}

#[test]
fn malformed_v2_never_falls_back_to_v1_and_v1_still_works() {
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let (v1_request, v1_response) = fixture.packet(&nonce);
    assert!(fixture
        .submit(&nonce, &v1_request, &v1_response, Duration::ZERO, 1)
        .is_ok());

    for body in ["null", "{}"] {
        let fixture = Fixture::new();
        let nonce = fixture.nonce();
        let mut request = serde_json::to_vec(&request(&nonce)).unwrap();
        assert_eq!(request.pop(), Some(b'}'));
        request.extend_from_slice(format!(",\"windows_v2\":{body}}}").as_bytes());
        let response = serde_json::to_vec(&response()).unwrap();
        assert!(fixture
            .submit(&nonce, &request, &response, Duration::ZERO, 1)
            .is_err());
        assert_eq!(fixture.store.contact_observation().unwrap(), None);
    }
}

#[test]
fn v2_contact_rejects_duplicate_fields_expiry_and_oversize() {
    for kind in ["duplicate_nonce", "duplicate_v2", "expired", "oversize"] {
        let fixture = Fixture::new();
        let nonce = fixture.nonce();
        let mut request = serde_json::to_vec(&request_v2(&nonce)).unwrap();
        match kind {
            "duplicate_nonce" => {
                let mut duplicate = format!("{{\"nonce\":\"{nonce}\",").into_bytes();
                duplicate.extend_from_slice(&request[1..]);
                request = duplicate;
            }
            "duplicate_v2" => {
                let mut duplicate = b"{\"windows_v2\":null,".to_vec();
                duplicate.extend_from_slice(&request[1..]);
                request = duplicate;
            }
            "oversize" => request.resize(contact::MAX_REQUEST_BYTES + 1, b' '),
            _ => (),
        }
        let response = serde_json::to_vec(&response()).unwrap();
        let age = if kind == "expired" {
            Duration::from_secs(31)
        } else {
            Duration::ZERO
        };
        assert!(
            fixture.submit(&nonce, &request, &response, age, 1).is_err(),
            "{kind}"
        );
        assert_eq!(fixture.store.contact_observation().unwrap(), None);
        assert!(!fixture.store.contact_pending(&nonce));
    }
}

#[test]
fn v2_contact_rejects_forged_receipt_and_wrong_signed_policy() {
    for kind in ["receipt", "policy_generation", "pending"] {
        let fixture = Fixture::new();
        let nonce = fixture.nonce();
        let request = serde_json::to_vec(&request_v2(&nonce)).unwrap();
        let mut response = response();
        if kind == "policy_generation" {
            let mut other = trust();
            other.enrollment_generation = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
            response["policy"] = serde_json::to_value(policy(other)).unwrap();
        } else if kind == "pending" {
            response["pending_approval"] = json!(true);
        }
        let response = serde_json::to_vec(&response).unwrap();
        let mut signature = receipt(&nonce, &request, &response);
        if kind == "receipt" {
            signature = format!("v1.{}", STANDARD.encode([0u8; 64]));
        }
        assert!(
            fixture
                .store
                .record_contact_with_clock(&nonce, &request, &response, &signature, || (
                    fixture.issued,
                    Ok(1)
                ))
                .is_err(),
            "{kind}"
        );
        assert_eq!(fixture.store.contact_observation().unwrap(), None);
    }
}
