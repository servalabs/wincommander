// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn action_signature_covers_action_fields_but_delivery_id_requires_response_proof() {
    let v = fixture();
    let original = command(&v);
    for field in [
        "idempotency_key",
        "catalog_id",
        "payload",
        "action_class",
        "epoch_version",
    ] {
        let mut raw = v["command"].clone();
        raw[field] = match field {
            "payload" => json!(r#"{"different":true}"#),
            "epoch_version" => json!("5"),
            "action_class" => json!("destructive"),
            _ => json!("changed"),
        };
        let changed: WindowsCommandV2 = serde_json::from_value(raw).unwrap();
        assert!(
            !verify_windows_command(&changed, &original.binding, &original.signer_key),
            "{field}"
        );
    }
    let mut changed = original.clone();
    changed.command_id = "different-delivery".into();
    // Deliberate offline-signing boundary; the response proof test rejects this change.
    assert!(verify_windows_command(
        &changed,
        &original.binding,
        &original.signer_key
    ));
    assert!(!verify_windows_command(
        &original,
        &original.binding,
        &original.binding.authority_key_b64
    ));
}

#[test]
fn all_outcome_evidence_fields_are_hashed_and_signed_without_time_normalization() {
    let v = fixture();
    let original = outcome(&v);
    let key = v["device_key"].as_str().unwrap();
    for field in [
        "receipt_id",
        "sequence",
        "previous_hash",
        "command_id",
        "catalog_id",
        "action_class",
        "outcome",
        "failure_stage",
        "observed_at",
        "summary",
    ] {
        let mut raw = v["outcome"].clone();
        raw[field] = match field {
            "sequence" => json!("2"),
            "previous_hash" => json!("cd".repeat(32)),
            "action_class" => json!("destructive"),
            "outcome" => json!("fail"),
            "failure_stage" => json!("execute"),
            "observed_at" => json!("2026-01-01T00:00:00.123456+00:00"),
            "summary" => json!({"kind":"command","result_sha256":"cd".repeat(32)}),
            _ => json!("changed"),
        };
        let mut changed: WindowsActionOutcomeV2 = serde_json::from_value(raw).unwrap();
        changed.record_hash = windows_outcome_hash(&changed);
        assert!(
            !verify_windows_outcome(&changed, &original.binding, key),
            "{field}"
        );
    }
    let mut wrong = original.clone();
    wrong.record_hash = wrong.record_hash.to_uppercase();
    assert!(!verify_windows_outcome(&wrong, &original.binding, key));
    let mut wrong = original;
    wrong.failure_stage = None;
    assert!(!verify_windows_outcome(&wrong, &wrong.binding, key));
}

#[test]
fn weak_keys_and_noncanonical_or_forged_signatures_are_rejected() {
    let v = fixture();
    let mut cmd = command(&v);
    let mut identity = [0u8; 32];
    identity[0] = 1;
    let mut signature = [0u8; 64];
    signature[0] = 1;
    let weak = B64.encode(identity);
    cmd.signer_key = weak.clone();
    cmd.signature = B64.encode(signature);
    assert!(!verify_windows_command(&cmd, &cmd.binding, &weak));
    let original = command(&v);
    for signature in [
        String::new(),
        original.signature.trim_end_matches('=').to_owned(),
        format!(" {}", original.signature),
        B64.encode([0u8; 64]),
    ] {
        let mut wrong = original.clone();
        wrong.signature = signature;
        assert!(!verify_windows_command(
            &wrong,
            &original.binding,
            &original.signer_key
        ));
    }
    let mut weak_binding = original.binding;
    weak_binding.authority_key_b64 = weak;
    assert!(!weak_binding.is_valid());
}

#[test]
fn exact_payload_text_is_bound_and_duplicate_signed_record_fields_are_rejected() {
    let v = fixture();
    let original = command(&v);
    let mut reordered = original.clone();
    reordered.payload = LosslessJson::new(format!(" {}", original.payload.as_str())).unwrap();
    assert_eq!(
        reordered.payload.parse_value().unwrap(),
        original.payload.parse_value().unwrap()
    );
    assert!(!verify_windows_command(
        &reordered,
        &original.binding,
        &original.signer_key
    ));
    let raw = v["outcome"].to_string();
    let duplicate = raw.replacen(
        "\"org_id\":\"org-alpha\"",
        "\"org_id\":\"org-alpha\",\"org_id\":\"other\"",
        1,
    );
    assert!(serde_json::from_str::<WindowsActionOutcomeV2>(&duplicate).is_err());
    let mut extra = v["proof"].clone();
    extra["signer_key"] = json!(original.signer_key);
    assert!(serde_json::from_value::<WindowsReceiptProofV2>(extra).is_err());
}

#[test]
fn raw_wire_decoder_rejects_duplicate_keys_even_in_arbitrary_payloads() {
    let v = fixture();
    let text = v["command"].to_string();
    assert!(decode_windows_v2::<WindowsCommandV2>(text.as_bytes()).is_ok());
    let mut duplicate_payload = v["command"].clone();
    duplicate_payload["payload"] = json!(r#"{"z":true,"z":false}"#);
    let duplicated = duplicate_payload.to_string();
    assert!(decode_windows_v2::<WindowsCommandV2>(duplicated.as_bytes()).is_err());
    for raw in [
        r#"{"value":1,"value":2}"#,
        r#"{"outer":[{"x":1,"x":2}]}"#,
        "{} {}",
    ] {
        assert!(decode_windows_v2::<Value>(raw.as_bytes()).is_err());
    }
    for raw in [
        r#"{"a":0.25,"b":18446744073709551615,"c":null,"d":[true,"é"]}"#,
        v["request_bytes"].as_str().unwrap(),
        v["response_bytes"].as_str().unwrap(),
    ] {
        assert_eq!(
            decode_windows_v2::<Value>(raw.as_bytes()).unwrap(),
            serde_json::from_str::<Value>(raw).unwrap()
        );
    }
}
