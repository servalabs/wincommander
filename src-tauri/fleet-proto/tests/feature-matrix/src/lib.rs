// SPDX-License-Identifier: AGPL-3.0-or-later
#[cfg(test)]
mod tests {
    use fleet_proto::windows_v2::*;
    use serde_json::{json, Value};

    #[test]
    fn v2_golden_and_lossless_semantics_do_not_depend_on_serde_features() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../fixtures/windows_v2.json")).unwrap();
        let command: WindowsCommandV2 = serde_json::from_value(fixture["command"].clone()).unwrap();
        assert!(verify_windows_command(
            &command,
            &command.binding,
            &command.signer_key
        ));
        let outcome: WindowsActionOutcomeV2 =
            serde_json::from_value(fixture["outcome"].clone()).unwrap();
        assert!(verify_windows_outcome(
            &outcome,
            &outcome.binding,
            fixture["device_key"].as_str().unwrap()
        ));
        assert_eq!(
            outcome.summary,
            WindowsOutcomeSummaryV2::Command {
                result_sha256: fixture["result_hash"].as_str().unwrap().into()
            }
        );
        let readiness: WindowsActionOutcomeV2 =
            serde_json::from_value(fixture["readiness"].clone()).unwrap();
        assert!(verify_windows_outcome(
            &readiness,
            &readiness.binding,
            fixture["device_key"].as_str().unwrap()
        ));
        let mut malformed = fixture["outcome"].clone();
        malformed["summary"]["detail"] = json!("must not be forwarded");
        assert!(serde_json::from_value::<WindowsActionOutcomeV2>(malformed).is_err());
        let proof: WindowsReceiptProofV2 =
            serde_json::from_value(fixture["proof"].clone()).unwrap();
        assert!(verify_windows_receipt(
            &proof,
            &WindowsReceiptVerification {
                binding: &proof.binding,
                nonce: &proof.nonce,
                request_bytes: fixture["request_bytes"].as_str().unwrap().as_bytes(),
                http_status: 200,
                response_bytes: fixture["response_bytes"].as_str().unwrap().as_bytes(),
            }
        ));
        for vector in fixture["epoch_boundaries"].as_array().unwrap() {
            let command: WindowsCommandV2 =
                serde_json::from_value(vector["command"].clone()).unwrap();
            assert!(verify_windows_command(
                &command,
                &command.binding,
                &command.signer_key
            ));
            let raw: Value =
                serde_json::from_str(vector["response_bytes"].as_str().unwrap()).unwrap();
            let response: WindowsCheckinResponseV2 =
                serde_json::from_value(raw["windows_v2"].clone()).unwrap();
            assert_eq!(response.command_epoch_floor, command.epoch_version);
        }
        for (vector, value) in fixture["delivery_boundaries"].as_array().unwrap().iter().zip([i64::MIN, i64::MAX]) {
            let raw: Value = serde_json::from_str(vector["response_bytes"].as_str().unwrap()).unwrap();
            let response: WindowsCheckinResponseV2 = serde_json::from_value(raw["windows_v2"].clone()).unwrap();
            assert_eq!(response.delivery_issued_unix_ms, value);
            assert_eq!(response.commands[0].delivery_expires_unix_ms, value);
        }
        let text = r#"{"large":12345678901234567890123456789012345678901234567890,"exp":1e400,"private":{"$serde_json::private::Number":"123","$serde_json::private::RawValue":"42"}}"#;
        let raw = LosslessJson::new(text.into()).unwrap();
        assert_eq!(raw.as_str(), text);
        let parsed = raw.parse_value().unwrap();
        let object = parsed.as_object().unwrap();
        assert_eq!(
            object
                .get_unique("large")
                .unwrap()
                .unwrap()
                .as_number()
                .unwrap()
                .as_str(),
            "12345678901234567890123456789012345678901234567890"
        );
        assert_eq!(
            object
                .get_unique("exp")
                .unwrap()
                .unwrap()
                .as_number()
                .unwrap()
                .as_str(),
            "1e400"
        );
        assert_eq!(
            object
                .get_unique("private")
                .unwrap()
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            2
        );
        assert!(LosslessJson::new(r#"{"x":1,"\u0078":2}"#.into()).is_err());
    }

    #[test]
    fn fleet_proto_does_not_enable_legacy_serde_features() {
        let numeric: Value = serde_json::from_str(r#"{"number":1.2300e-2}"#).unwrap();
        let expected = if cfg!(feature = "precise") {
            r#"{"number":1.2300e-2}"#
        } else {
            r#"{"number":0.0123}"#
        };
        assert_eq!(
            fleet_proto::canonical_json_bytes(&numeric),
            expected.as_bytes()
        );
        let number: Value =
            serde_json::from_str(r#"{"$serde_json::private::Number":"123"}"#).unwrap();
        assert_eq!(
            number,
            if cfg!(feature = "precise") {
                json!(123)
            } else {
                json!({"$serde_json::private::Number":"123"})
            }
        );
        let raw: Value =
            serde_json::from_str(r#"{"$serde_json::private::RawValue":"42"}"#).unwrap();
        assert_eq!(
            raw,
            if cfg!(feature = "raw") {
                json!(42)
            } else {
                json!({"$serde_json::private::RawValue":"42"})
            }
        );
    }
}
