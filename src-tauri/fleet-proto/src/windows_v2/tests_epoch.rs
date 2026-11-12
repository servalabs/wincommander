// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn response() -> WindowsCheckinResponseV2 {
    let raw: Value = serde_json::from_str(fixture()["response_bytes"].as_str().unwrap()).unwrap();
    serde_json::from_value(raw["windows_v2"].clone()).unwrap()
}

#[test]
fn signed_epoch_floor_and_commands_preserve_the_entire_i64_range() {
    for value in [
        i64::MIN,
        -9_007_199_254_740_993,
        -1,
        0,
        1,
        9_007_199_254_740_993,
        i64::MAX,
    ] {
        let mut response = response();
        response.command_epoch_floor = value;
        response.commands[0].epoch_version = value;
        let encoded = serde_json::to_value(&response).unwrap();
        assert_eq!(encoded["command_epoch_floor"], value.to_string());
        assert_eq!(encoded["commands"][0]["epoch_version"], value.to_string());
        let decoded: WindowsCheckinResponseV2 = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.command_epoch_floor, value);
        assert_eq!(decoded.commands[0].epoch_version, value);
    }
    let mut response = response();
    response.commands.push(response.commands[0].clone());
    assert_eq!(
        response.commands[0].epoch_version,
        response.command_epoch_floor
    );
    assert!(response.has_expected_binding(&response.binding));
}

#[test]
fn epoch_strings_reject_aliases_missing_floor_and_overflow_without_unsigned_casts() {
    let original = serde_json::to_value(response()).unwrap();
    for value in [
        json!(0),
        json!(-1),
        json!(null),
        json!(""),
        json!("+1"),
        json!("01"),
        json!("-01"),
        json!("-0"),
        json!(" 1"),
        json!("1 "),
        json!("1.0"),
        json!("1e3"),
        json!("--1"),
        json!("9223372036854775808"),
        json!("-9223372036854775809"),
    ] {
        let mut raw = original.clone();
        raw["command_epoch_floor"] = value.clone();
        assert!(
            serde_json::from_value::<WindowsCheckinResponseV2>(raw).is_err(),
            "floor {value}"
        );
        let mut raw = original.clone();
        raw["commands"][0]["epoch_version"] = value.clone();
        assert!(
            serde_json::from_value::<WindowsCheckinResponseV2>(raw).is_err(),
            "command {value}"
        );
    }
    let mut missing = original;
    missing
        .as_object_mut()
        .unwrap()
        .remove("command_epoch_floor");
    assert!(serde_json::from_value::<WindowsCheckinResponseV2>(missing).is_err());
}

#[test]
fn independent_min_max_vectors_bind_floor_and_command_epoch_to_their_signatures() {
    let fixture = fixture();
    for (vector, epoch) in fixture["epoch_boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .zip([i64::MIN, i64::MAX])
    {
        let command: WindowsCommandV2 = serde_json::from_value(vector["command"].clone()).unwrap();
        assert_eq!(command.epoch_version, epoch);
        let bytes = windows_command_preimage(&WindowsCommandSigningInput {
            binding: &command.binding,
            idempotency_key: &command.idempotency_key,
            catalog_id: &command.catalog_id,
            action_class: command.action_class,
            payload: &command.payload,
            epoch_version: command.epoch_version,
        });
        assert_eq!(
            bytes,
            vector["command_preimage"].as_str().unwrap().as_bytes()
        );
        assert!(verify_windows_command(
            &command,
            &command.binding,
            &command.signer_key
        ));
        let response_bytes = vector["response_bytes"].as_str().unwrap().as_bytes();
        let raw: Value = serde_json::from_slice(response_bytes).unwrap();
        let response: WindowsCheckinResponseV2 =
            serde_json::from_value(raw["windows_v2"].clone()).unwrap();
        assert_eq!(response.command_epoch_floor, epoch);
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
            response_bytes,
        };
        assert!(verify_windows_receipt(&proof, &expected));
        let mut changed = raw;
        changed["windows_v2"]["command_epoch_floor"] = json!("0");
        let changed = serde_json::to_vec(&changed).unwrap();
        assert!(!verify_windows_receipt(
            &proof,
            &WindowsReceiptVerification {
                response_bytes: &changed,
                ..expected
            }
        ));
        let mut changed = command.clone();
        changed.epoch_version = 0;
        assert!(!verify_windows_command(
            &changed,
            &changed.binding,
            &changed.signer_key
        ));
    }
}

#[cfg(feature = "ts-codegen")]
#[test]
fn generated_epoch_floor_remains_a_json_string() {
    use ts_rs::TS;
    assert!(WindowsCheckinResponseV2::decl().contains("command_epoch_floor: string"));
    assert!(WindowsCommandV2::decl().contains("epoch_version: string"));
}
