// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{contact_test_support::*, *};
use serde_json::json;
use std::time::Duration;

#[test]
fn exact_request_response_bytes_and_receipt_version_are_bound() {
    for mutation in 0..4 {
        let fixture = Fixture::new();
        let nonce = fixture.nonce();
        let (mut request, mut response) = fixture.packet(&nonce);
        let mut signed = receipt(&nonce, &request, &response);
        match mutation {
            0 => request.push(b' '),
            1 => response.push(b' '),
            2 => signed.replace_range(..3, "v2."),
            _ => signed.clear(),
        }
        assert!(fixture
            .store
            .record_contact_with_clock(&nonce, &request, &response, &signed, || (
                fixture.issued,
                Ok(1)
            ))
            .is_err());
        assert!(!fixture.store.contact_pending(&nonce));
        assert_eq!(fixture.store.contact_observation().unwrap(), None);
    }
}

#[test]
fn signed_wrong_org_device_key_generation_and_unsigned_policy_are_rejected() {
    for field in [
        "org_id",
        "device_id",
        "signer_key",
        "enrollment_generation",
        "signature",
    ] {
        let fixture = Fixture::new();
        let nonce = fixture.nonce();
        let request = serde_json::to_vec(&request(&nonce)).unwrap();
        let mut binding = trust();
        match field {
            "org_id" => binding.org_id = "other-org".into(),
            "device_id" => binding.device_id = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(),
            "enrollment_generation" => {
                binding.enrollment_generation = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into()
            }
            "signer_key" => {
                binding.signer_key = STANDARD.encode(
                    ed25519_dalek::SigningKey::from_bytes(&[42; 32])
                        .verifying_key()
                        .as_bytes(),
                )
            }
            _ => {}
        }
        let mut signed_policy = policy(binding);
        if field == "signature" {
            signed_policy.signature.clear();
        }
        let mut body = response();
        body["policy"] = serde_json::to_value(signed_policy).unwrap();
        let response = serde_json::to_vec(&body).unwrap();
        assert!(
            fixture
                .submit(&nonce, &request, &response, Duration::ZERO, 1)
                .is_err(),
            "{field}"
        );
        assert_eq!(fixture.store.contact_observation().unwrap(), None);
    }
}

#[test]
fn response_version_approval_duplicate_fields_and_over_limit_bodies_are_rejected() {
    for mutation in 0..5 {
        let fixture = Fixture::new();
        let nonce = fixture.nonce();
        let (mut request, mut response) = fixture.packet(&nonce);
        match mutation {
            0 | 1 => {
                let mut value: serde_json::Value = serde_json::from_slice(&response).unwrap();
                if mutation == 0 {
                    value["protocol_version"] = json!(0);
                } else {
                    value["pending_approval"] = json!(true);
                }
                response = serde_json::to_vec(&value).unwrap();
            }
            2 => {
                let mut duplicate = b"{\"protocol_version\":1,".to_vec();
                duplicate.extend_from_slice(&response[1..]);
                response = duplicate;
            }
            3 => request.resize(contact::MAX_REQUEST_BYTES + 1, b' '),
            _ => response.resize(contact::MAX_RESPONSE_BYTES + 1, b' '),
        }
        assert!(fixture
            .submit(&nonce, &request, &response, Duration::ZERO, 1)
            .is_err());
        assert!(!fixture.store.contact_pending(&nonce));
        assert_eq!(fixture.store.contact_observation().unwrap(), None);
    }
}

#[test]
fn weak_pinned_key_and_unavailable_clock_never_produce_history() {
    let mut weak = trust();
    let mut identity = [0u8; 32];
    identity[0] = 1;
    weak.signer_key = STANDARD.encode(identity);
    assert!(contact::PendingContact::issue(weak, std::time::Instant::now()).is_err());
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    assert_eq!(
        fixture.store.record_contact_with_clock(
            &nonce,
            &request,
            &response,
            &receipt(&nonce, &request, &response),
            || (fixture.issued, Err("contact_clock_unavailable"))
        ),
        Err("contact_clock_unavailable")
    );
    assert_eq!(fixture.store.contact_observation().unwrap(), None);
    assert!(!fixture.store.contact_pending(&nonce));
}

#[test]
fn authenticated_invalid_request_shapes_and_duplicate_bindings_are_rejected() {
    for (field, value) in [
        ("device_id", json!("other")),
        ("nonce", json!("other")),
        ("protocol_version", json!(0)),
        ("hmac_version", json!(1)),
        ("hmac", json!("not-a-mac")),
        ("decoy", json!(true)),
        ("capabilities", json!([])),
        ("ts", json!(null)),
    ] {
        let fixture = Fixture::new();
        let nonce = fixture.nonce();
        let mut body = request(&nonce);
        body[field] = value;
        let request = serde_json::to_vec(&body).unwrap();
        let response = serde_json::to_vec(&response()).unwrap();
        assert!(
            fixture
                .submit(&nonce, &request, &response, Duration::ZERO, 1)
                .is_err(),
            "{field}"
        );
    }
    let fixture = Fixture::new();
    let nonce = fixture.nonce();
    let (request, response) = fixture.packet(&nonce);
    let mut duplicate = format!("{{\"nonce\":\"{nonce}\",").into_bytes();
    duplicate.extend_from_slice(&request[1..]);
    assert!(fixture
        .submit(&nonce, &duplicate, &response, Duration::ZERO, 1)
        .is_err());
}
