// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/windows_v2.json")).unwrap()
}
fn command(v: &Value) -> WindowsCommandV2 {
    serde_json::from_value(v["command"].clone()).unwrap()
}
fn outcome(v: &Value) -> WindowsActionOutcomeV2 {
    serde_json::from_value(v["outcome"].clone()).unwrap()
}
fn proof(v: &Value) -> WindowsReceiptProofV2 {
    serde_json::from_value(v["proof"].clone()).unwrap()
}
fn bindings(original: &FleetEnrollmentBinding) -> Vec<FleetEnrollmentBinding> {
    (0..4)
        .map(|field| {
            let mut b = original.clone();
            match field {
                0 => b.org_id.0.push('x'),
                1 => b.device_id.0.push('x'),
                2 => b.enrollment_generation = "22222222-2222-4222-8222-222222222222".into(),
                _ => {
                    b.authority_key_b64 =
                        B64.encode(SigningKey::from_bytes(&[15; 32]).verifying_key().to_bytes())
                }
            }
            b
        })
        .collect()
}

#[test]
fn independent_golden_vectors_pin_every_preimage_hash_and_signature() {
    let v = fixture();
    let cmd = command(&v);
    let o = outcome(&v);
    let p = proof(&v);
    let bytes = windows_command_preimage(&WindowsCommandSigningInput {
        binding: &cmd.binding,
        idempotency_key: &cmd.idempotency_key,
        catalog_id: &cmd.catalog_id,
        action_class: cmd.action_class,
        payload: &cmd.payload,
        epoch_version: cmd.epoch_version,
    });
    assert_eq!(bytes, v["command_preimage"].as_str().unwrap().as_bytes());
    assert_eq!(
        B64.encode(SigningKey::from_bytes(&[13; 32]).sign(&bytes).to_bytes()),
        cmd.signature
    );
    assert!(verify_windows_command(&cmd, &cmd.binding, &cmd.signer_key));
    assert_eq!(
        windows_outcome_preimage(&o),
        v["outcome_preimage"].as_str().unwrap().as_bytes()
    );
    assert_eq!(windows_outcome_hash(&o), o.record_hash);
    assert!(verify_windows_outcome(
        &o,
        &o.binding,
        v["device_key"].as_str().unwrap()
    ));
    let ack: WindowsAckV2 = serde_json::from_value(v["ack"].clone()).unwrap();
    assert_eq!(windows_ack_hash(&ack), v["ack_hash"]);
    assert_eq!(windows_result_hash(ack.result.as_ref()), v["result_hash"]);
    assert_eq!(
        windows_receipt_preimage(&p),
        v["proof_preimage"].as_str().unwrap().as_bytes()
    );
    assert_eq!(
        B64.encode(
            SigningKey::from_bytes(&[11; 32])
                .sign(&windows_receipt_preimage(&p))
                .to_bytes()
        ),
        p.signature
    );
    // Independent dalek2 oracle verifies the external vector too.
    let key: [u8; 32] = B64.decode(&cmd.signer_key).unwrap().try_into().unwrap();
    let sig: [u8; 64] = B64.decode(&cmd.signature).unwrap().try_into().unwrap();
    ed25519_dalek_v2::VerifyingKey::from_bytes(&key)
        .unwrap()
        .verify_strict(&bytes, &ed25519_dalek_v2::Signature::from_bytes(&sig))
        .unwrap();
}

#[test]
fn signatures_cannot_move_between_authorities_devices_orgs_or_generations() {
    let v = fixture();
    let cmd = command(&v);
    let o = outcome(&v);
    for other in bindings(&cmd.binding) {
        assert!(!verify_windows_command(&cmd, &other, &cmd.signer_key));
        assert!(!verify_windows_outcome(
            &o,
            &other,
            v["device_key"].as_str().unwrap()
        ));
        let mut rebound = cmd.clone();
        rebound.binding = other.clone();
        assert!(!verify_windows_command(&rebound, &other, &cmd.signer_key));
        let mut rebound = o.clone();
        rebound.binding = other.clone();
        rebound.record_hash = windows_outcome_hash(&rebound);
        assert!(!verify_windows_outcome(
            &rebound,
            &other,
            v["device_key"].as_str().unwrap()
        ));
    }
    let mut cmd = cmd;
    cmd.version = 1;
    assert!(!verify_windows_command(&cmd, &cmd.binding, &cmd.signer_key));
    let mut o = o;
    o.version = 1;
    assert!(!verify_windows_outcome(
        &o,
        &o.binding,
        v["device_key"].as_str().unwrap()
    ));
}

#[test]
fn response_proof_requires_exact_live_request_status_body_and_context() {
    let v = fixture();
    let p = proof(&v);
    let request = v["request_bytes"].as_str().unwrap().as_bytes();
    let response = v["response_bytes"].as_str().unwrap().as_bytes();
    let mut expected = WindowsReceiptVerification {
        binding: &p.binding,
        nonce: "nonce-1",
        request_bytes: request,
        http_status: 200,
        response_bytes: response,
    };
    assert!(verify_windows_receipt(&p, &expected));
    expected.nonce = "nonce-2";
    assert!(!verify_windows_receipt(&p, &expected));
    expected.nonce = "nonce-1";
    expected.http_status = 503;
    assert!(!verify_windows_receipt(&p, &expected));
    expected.http_status = 200;
    expected.request_bytes = b"{}";
    assert!(!verify_windows_receipt(&p, &expected));
    expected.request_bytes = request;
    let mut changed = response.to_vec();
    changed.push(b' ');
    expected.response_bytes = &changed;
    assert!(!verify_windows_receipt(&p, &expected));
    expected.response_bytes = response;
    for other in bindings(&p.binding) {
        let other_expected = WindowsReceiptVerification {
            binding: &other,
            ..expected
        };
        assert!(!verify_windows_receipt(&p, &other_expected));
    }
    expected.binding = &p.binding;
    let mut downgrade = p.clone();
    downgrade.version = 1;
    assert!(!verify_windows_receipt(&downgrade, &expected));
    let mut response_value: Value = serde_json::from_slice(response).unwrap();
    response_value["windows_v2"]["commands"][0]["command_id"] = json!("changed-delivery-id");
    let changed = serde_json::to_vec(&response_value).unwrap();
    expected.response_bytes = &changed;
    assert!(!verify_windows_receipt(&p, &expected));
}

#[test]
fn wire_records_reject_unknown_duplicate_missing_or_invalid_binding_fields() {
    let v = fixture();
    let mut value = v["outcome"].clone();
    value["surprise"] = json!(true);
    assert!(serde_json::from_value::<WindowsActionOutcomeV2>(value).is_err());
    let text = serde_json::to_string(&v["command"]).unwrap();
    let duplicate = text.replacen("\"version\":2", "\"version\":2,\"version\":2", 1);
    assert!(serde_json::from_str::<WindowsCommandV2>(&duplicate).is_err());
    let mut value = v["outcome"].clone();
    value.as_object_mut().unwrap().remove("binding");
    assert!(serde_json::from_value::<WindowsActionOutcomeV2>(value).is_err());
    let cmd = command(&v);
    for generation in [
        "",
        "00000000-0000-0000-0000-000000000000",
        "ABCDEFAB-1234-4321-8123-123456789abc",
    ] {
        let mut b = cmd.binding.clone();
        b.enrollment_generation = generation.into();
        assert!(!b.is_valid());
    }
    let mut weak = cmd.binding;
    weak.authority_key_b64 = B64.encode([0u8; 32]);
    assert!(!weak.is_valid());
}

#[test]
fn nested_batch_binding_and_head_are_checked_without_relabeling() {
    let v = fixture();
    let raw: Value = serde_json::from_str(v["request_bytes"].as_str().unwrap()).unwrap();
    let mut request: WindowsCheckinRequestV2 =
        serde_json::from_value(raw["windows_v2"].clone()).unwrap();
    let expected = request.binding.clone();
    assert!(request.has_expected_binding(&expected));
    request.acks[0].binding.org_id.0.push('x');
    assert!(!request.has_expected_binding(&expected));
    let raw: Value = serde_json::from_str(v["response_bytes"].as_str().unwrap()).unwrap();
    let mut response: WindowsCheckinResponseV2 =
        serde_json::from_value(raw["windows_v2"].clone()).unwrap();
    assert!(response.has_expected_binding(&expected));
    response.head.previous_hash = None;
    assert!(!response.has_expected_binding(&expected));
    assert!(WindowsOutcomeHeadV2 {
        last_sequence: 0,
        previous_hash: None
    }
    .is_valid());
    assert!(!WindowsOutcomeHeadV2 {
        last_sequence: 1,
        previous_hash: None
    }
    .is_valid());
}

#[path = "tests_adversarial.rs"]
mod adversarial;

#[path = "tests_lossless.rs"]
mod lossless_tests;
