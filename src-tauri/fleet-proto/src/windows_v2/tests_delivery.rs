// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn delivery_timestamps_are_required_canonical_full_i64_strings() {
    let original: Value =
        serde_json::from_str(fixture()["response_bytes"].as_str().unwrap()).unwrap();
    for value in [
        i64::MIN,
        -9_007_199_254_740_993,
        -1,
        0,
        1,
        9_007_199_254_740_993,
        i64::MAX,
    ] {
        let mut raw = original["windows_v2"].clone();
        raw["delivery_issued_unix_ms"] = json!(value.to_string());
        raw["commands"][0]["delivery_expires_unix_ms"] = json!(value.to_string());
        let decoded: WindowsCheckinResponseV2 = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(decoded.delivery_issued_unix_ms, value);
        assert_eq!(decoded.commands[0].delivery_expires_unix_ms, value);
        assert_eq!(serde_json::to_value(decoded).unwrap(), raw);
    }
    for field in ["delivery_issued_unix_ms", "delivery_expires_unix_ms"] {
        for value in [
            json!(null),
            json!(0),
            json!(""),
            json!("+1"),
            json!("01"),
            json!("-01"),
            json!("-0"),
            json!(" 1"),
            json!("1 "),
            json!("1.0"),
            json!("1e3"),
            json!("9223372036854775808"),
            json!("-9223372036854775809"),
        ] {
            let mut raw = original["windows_v2"].clone();
            let target = if field == "delivery_issued_unix_ms" {
                &mut raw
            } else {
                &mut raw["commands"][0]
            };
            target[field] = value.clone();
            assert!(
                serde_json::from_value::<WindowsCheckinResponseV2>(raw).is_err(),
                "{field} {value}"
            );
        }
        let mut raw = original["windows_v2"].clone();
        let target = if field == "delivery_issued_unix_ms" {
            &mut raw
        } else {
            &mut raw["commands"][0]
        };
        target.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<WindowsCheckinResponseV2>(raw).is_err());
    }
}

#[test]
fn independent_delivery_vectors_bind_timestamps_only_to_response_proof() {
    // Boundary fixtures prove encoding/signature interoperability, not a valid
    // dispatch interval; runtime freshness checks remain mandatory.
    let fixture = fixture();
    for (vector, value) in fixture["delivery_boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .zip([i64::MIN, i64::MAX])
    {
        let bytes = vector["response_bytes"].as_str().unwrap().as_bytes();
        let raw: Value = serde_json::from_slice(bytes).unwrap();
        let response: WindowsCheckinResponseV2 =
            serde_json::from_value(raw["windows_v2"].clone()).unwrap();
        assert_eq!(response.delivery_issued_unix_ms, value);
        let command = &response.commands[0];
        assert_eq!(command.delivery_expires_unix_ms, value);
        assert!(verify_windows_command(
            command,
            &command.binding,
            &command.signer_key
        ));
        assert_eq!(command.signature, fixture["command"]["signature"]);
        let proof: WindowsReceiptProofV2 = serde_json::from_value(vector["proof"].clone()).unwrap();
        assert_eq!(
            windows_receipt_preimage(&proof),
            vector["proof_preimage"].as_str().unwrap().as_bytes()
        );
        let expected = WindowsReceiptVerification {
            binding: &proof.binding,
            nonce: &proof.nonce,
            request_bytes: fixture["request_bytes"].as_str().unwrap().as_bytes(),
            http_status: 200,
            response_bytes: bytes,
        };
        assert!(verify_windows_receipt(&proof, &expected));
        for field in ["delivery_issued_unix_ms", "delivery_expires_unix_ms"] {
            let mut changed = raw.clone();
            let target = if field == "delivery_issued_unix_ms" {
                &mut changed["windows_v2"]
            } else {
                &mut changed["windows_v2"]["commands"][0]
            };
            target[field] = json!("0");
            let changed_bytes = serde_json::to_vec(&changed).unwrap();
            assert!(!verify_windows_receipt(
                &proof,
                &WindowsReceiptVerification {
                    response_bytes: &changed_bytes,
                    ..expected
                }
            ));
            // Offline action authority cannot establish delivery freshness.
            let changed_command: WindowsCommandV2 =
                serde_json::from_value(changed["windows_v2"]["commands"][0].clone()).unwrap();
            assert!(verify_windows_command(
                &changed_command,
                &changed_command.binding,
                &changed_command.signer_key
            ));
        }
    }
}

#[cfg(feature = "ts-codegen")]
#[test]
fn generated_delivery_timestamps_are_required_strings() {
    use ts_rs::TS;
    assert!(WindowsCheckinResponseV2::decl().contains("delivery_issued_unix_ms: string"));
    assert!(WindowsCommandV2::decl().contains("delivery_expires_unix_ms: string"));
}
