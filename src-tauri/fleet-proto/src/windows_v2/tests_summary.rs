// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn sign(outcome: &mut WindowsActionOutcomeV2) {
    outcome.record_hash = windows_outcome_hash(outcome);
    let hash = crate::outcomes::hex_to_32_bytes(&outcome.record_hash).unwrap();
    outcome.signature = B64.encode(SigningKey::from_bytes(&[7; 32]).sign(&hash).to_bytes());
}
fn valid(outcome: &WindowsActionOutcomeV2) -> bool {
    verify_windows_outcome(
        outcome,
        &outcome.binding,
        fixture()["device_key"].as_str().unwrap(),
    )
}
fn readiness() -> WindowsActionOutcomeV2 {
    serde_json::from_value(fixture()["readiness"].clone()).unwrap()
}

#[test]
fn closed_summary_rejects_details_unknown_tags_extra_fields_and_old_digest() {
    let fixture = fixture();
    for summary in [
        json!({"kind":"command","result_sha256":"ab".repeat(32),"detail":"private text"}),
        json!({"kind":"command","result_sha256":"ab".repeat(32),"result":{}}),
        json!({"kind":"other","result_sha256":"ab".repeat(32)}),
        json!({"kind":"command"}),
        json!("{\"detail\":\"private text\"}"),
    ] {
        let mut raw = fixture["outcome"].clone();
        raw["summary"] = summary;
        assert!(serde_json::from_value::<WindowsActionOutcomeV2>(raw).is_err());
    }
    let mut raw = fixture["readiness"].clone();
    raw["summary"]["result"]["detail"] = json!("private text");
    assert!(serde_json::from_value::<WindowsActionOutcomeV2>(raw).is_err());
    let mut raw = fixture["readiness"].clone();
    raw["summary"]["result"]["arm_predicate"] = json!("custom");
    assert!(serde_json::from_value::<WindowsActionOutcomeV2>(raw).is_err());
    let mut raw = fixture["outcome"].clone();
    raw.as_object_mut().unwrap().remove("summary");
    raw["result_digest"] = json!("{}");
    assert!(serde_json::from_value::<WindowsActionOutcomeV2>(raw).is_err());
    let raw = serde_json::to_string(&fixture["outcome"]).unwrap();
    let duplicate = raw.replacen(
        "\"kind\":\"command\"",
        "\"kind\":\"command\",\"kind\":\"command\"",
        1,
    );
    assert!(decode_windows_v2::<WindowsActionOutcomeV2>(duplicate.as_bytes()).is_err());
}

#[test]
fn command_summary_hash_is_exact_and_unlinked_opaque_reports_remain_representable() {
    let v = fixture();
    let mut outcome = outcome(&v);
    let ack: WindowsAckV2 = serde_json::from_value(v["ack"].clone()).unwrap();
    assert_eq!(
        outcome.summary,
        WindowsOutcomeSummaryV2::Command {
            result_sha256: windows_result_hash(ack.result.as_ref())
        }
    );
    assert_ne!(
        windows_result_hash(None),
        windows_result_hash(Some(&LosslessJson::new("null".into()).unwrap()))
    );
    let spaced = LosslessJson::new(format!(" {}", ack.result.as_ref().unwrap().as_str())).unwrap();
    assert_ne!(
        windows_result_hash(Some(&spaced)),
        windows_result_hash(ack.result.as_ref())
    );
    for hash in ["".into(), "a".repeat(63), "AB".repeat(32), "z".repeat(64)] {
        outcome.summary = WindowsOutcomeSummaryV2::Command {
            result_sha256: hash,
        };
        sign(&mut outcome);
        assert!(!valid(&outcome));
    }
    outcome.summary = WindowsOutcomeSummaryV2::Command {
        result_sha256: windows_result_hash(None),
    };
    outcome.command_id = None;
    outcome.catalog_id = "opaque/catalog?Ł".into();
    outcome.receipt_id = "opaque receipt 🔒".into();
    sign(&mut outcome);
    assert!(valid(&outcome));
    // Classification/quarantine is the server's responsibility, not a new
    // protocol restriction on command summaries or opaque identifiers.
    outcome.catalog_id = "readiness.self_test".into();
    sign(&mut outcome);
    assert!(valid(&outcome));
}

#[test]
fn readiness_golden_preserves_existing_closed_fields_and_distinct_valid_times() {
    let v = fixture();
    let outcome = readiness();
    assert_eq!(
        windows_outcome_preimage(&outcome),
        v["readiness_preimage"].as_str().unwrap().as_bytes()
    );
    assert!(valid(&outcome));
    for field in [
        "arm_predicate",
        "signed_dispatch",
        "simulate_branch",
        "recovery_path",
        "observed_at",
    ] {
        let mut raw = v["readiness"].clone();
        raw["summary"]["result"][field] = if field == "observed_at" {
            json!("2026-01-02T01:02:03Z")
        } else {
            json!("unknown")
        };
        if raw == v["readiness"] {
            raw["summary"]["result"][field] = json!("pass");
        }
        let mut changed: WindowsActionOutcomeV2 = serde_json::from_value(raw).unwrap();
        changed.record_hash = windows_outcome_hash(&changed);
        assert!(!valid(&changed), "signature must bind {field}");
        sign(&mut changed);
        assert!(valid(&changed));
    }
}

#[test]
fn signed_readiness_requires_catalog_class_and_absence_of_command_link() {
    for field in ["catalog", "class", "command"] {
        let mut outcome = readiness();
        match field {
            "catalog" => outcome.catalog_id = "fixture.inspect".into(),
            "class" => outcome.action_class = crate::ActionClass::Destructive,
            _ => outcome.command_id = Some("opaque-command".into()),
        }
        sign(&mut outcome);
        assert!(!valid(&outcome));
    }
}

#[test]
fn timestamps_are_validated_without_normalizing_the_signed_spelling() {
    for timestamp in [
        "",
        "private text",
        "2026-13-01T00:00:00Z",
        "2026-02-30T00:00:00Z",
        "2026-01-01",
    ] {
        let mut outcome = readiness();
        outcome.observed_at = timestamp.into();
        sign(&mut outcome);
        assert!(!valid(&outcome));
        let mut outcome = readiness();
        if let WindowsOutcomeSummaryV2::Readiness { result } = &mut outcome.summary {
            result.observed_at = timestamp.into();
        }
        sign(&mut outcome);
        assert!(!valid(&outcome));
    }
    let mut outcome = readiness();
    let before = windows_outcome_hash(&outcome);
    outcome.observed_at = "2026-01-01T00:00:00.123456789+00:00".into();
    sign(&mut outcome);
    assert!(valid(&outcome));
    assert_ne!(before, outcome.record_hash);
}

#[cfg(feature = "ts-codegen")]
#[test]
fn generated_summary_contract_matches_serde_internal_tags() {
    use ts_rs::TS;
    let declaration = WindowsOutcomeSummaryV2::decl();
    assert!(
        declaration.contains("kind")
            && declaration.contains("\"command\"")
            && declaration.contains("\"readiness\""),
        "{declaration}"
    );
    assert!(
        !declaration.contains("\"Command\"") && !declaration.contains("\"Readiness\""),
        "{declaration}"
    );
}
