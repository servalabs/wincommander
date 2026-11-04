// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn decimal_counter_strings_preserve_unsafe_js_integers_and_reject_aliases() {
    let v = fixture();
    for n in [0, 9_007_199_254_740_991, 9_007_199_254_740_993, u64::MAX] {
        let head = WindowsOutcomeHeadV2 {
            last_sequence: n,
            previous_hash: (n > 0).then(|| "ab".repeat(32)),
        };
        assert!(head.is_valid());
        let wire = serde_json::to_value(&head).unwrap();
        assert_eq!(wire["last_sequence"], n.to_string());
        assert_eq!(
            serde_json::from_value::<WindowsOutcomeHeadV2>(wire)
                .unwrap()
                .last_sequence,
            n
        );
    }
    let mut cmd = command(&v);
    cmd.epoch_version = i64::MAX;
    let wire = serde_json::to_value(&cmd).unwrap();
    assert_eq!(wire["epoch_version"], i64::MAX.to_string());
    assert_eq!(
        serde_json::from_value::<WindowsCommandV2>(wire)
            .unwrap()
            .epoch_version,
        i64::MAX
    );
    for invalid in [
        json!(1),
        json!("01"),
        json!("+1"),
        json!("-1"),
        json!("1.0"),
        json!("1e3"),
        json!(" 1"),
        json!(""),
        json!("18446744073709551616"),
    ] {
        assert!(serde_json::from_value::<WindowsOutcomeHeadV2>(
            json!({"last_sequence":invalid,"previous_hash":null})
        )
        .is_err());
    }
    let mut invalid = v["command"].clone();
    invalid["epoch_version"] = json!("9223372036854775808");
    assert!(serde_json::from_value::<WindowsCommandV2>(invalid).is_err());
    assert!(WindowsOutcomeHeadV2 {
        last_sequence: 0,
        previous_hash: None
    }
    .is_valid());
}

#[test]
fn opaque_numbers_are_exact_text_and_semantic_conversion_preserves_precision() {
    let text = r#"{"huge":12345678901234567890123456789012345678901234567890,"exp":1e400,"nested":[9007199254740993,-0,1.2300e-200],"unicode":"é"}"#;
    let value = LosslessJson::new(text.into()).unwrap();
    assert_eq!(value.as_str(), text);
    let wire = serde_json::to_string(&value).unwrap();
    assert_eq!(serde_json::from_str::<LosslessJson>(&wire).unwrap(), value);
    let parsed = value.parse_value().unwrap();
    assert_eq!(
        field(&parsed, "huge").as_number().unwrap().as_str(),
        "12345678901234567890123456789012345678901234567890"
    );
    assert_eq!(
        field(&parsed, "nested").as_array().unwrap()[0]
            .as_number()
            .unwrap()
            .as_str(),
        "9007199254740993"
    );
    assert_eq!(field(&parsed, "exp").as_number().unwrap().as_str(), "1e400");
    let other = LosslessJson::new(format!(" {text} ")).unwrap();
    assert_eq!(other.parse_value().unwrap(), parsed);
    assert_ne!(
        windows_result_hash(Some(&value)),
        windows_result_hash(Some(&other))
    );
    assert_ne!(
        windows_result_hash(None),
        windows_result_hash(Some(&LosslessJson::new("null".into()).unwrap()))
    );
}

#[test]
fn literal_private_serde_keys_keep_their_object_identity() {
    for text in [
        r#"{"$serde_json::private::Number":"123","array":[{"$serde_json::private::RawValue":"42"}]}"#,
        r#"{"array":[{"$serde_json::private::RawValue":"42"}],"$serde_json::private::Number":"123"}"#,
        r#"{"\u0024serde_json::private::Number":"123","array":[{"\u0024serde_json::private::RawValue":"42"}]}"#,
    ] {
        let value = LosslessJson::new(text.into())
            .unwrap()
            .parse_value()
            .unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        assert_eq!(
            field(&value, "$serde_json::private::Number").as_str(),
            Some("123")
        );
        assert_eq!(
            field(
                &field(&value, "array").as_array().unwrap()[0],
                "$serde_json::private::RawValue"
            )
            .as_str(),
            Some("42")
        );
        assert!(field(&value, "array").as_array().unwrap()[0].is_object());
    }
    assert!(LosslessJson::new(
        r#"{"\u0024serde_json::private::Number":"1","$serde_json::private::Number":"2"}"#.into()
    )
    .is_err());
    for invalid in ["NaN", "Infinity", "{} {}", r#"{"a":[{"x":1,"x":2}]}"#] {
        assert!(LosslessJson::new(invalid.into()).is_err());
    }
}

fn field<'a>(value: &'a LosslessValue, name: &str) -> &'a LosslessValue {
    value
        .as_object()
        .unwrap()
        .get_unique(name)
        .unwrap()
        .unwrap()
}

#[test]
fn independent_parser_enforces_unicode_duplicates_grammar_and_existing_depth() {
    for invalid in [
        r#""\ud800""#,
        r#""\udc00""#,
        r#""\ud800\ud800""#,
        r#""\x00""#,
        "01",
        "+1",
        "1.",
        ".1",
        "1e",
        "1e+",
        "--1",
        "[1,]",
        r#"{"a":1,}"#,
        "[}",
        "null true",
        "/*x*/null",
        r#"{"a":[{"x":1,"\u0078":2}]}"#,
    ] {
        assert!(LosslessJson::new(invalid.into()).is_err(), "{invalid}");
    }
    for invalid in [b"\xff".as_slice(), b"\"\xc0\xaf\"".as_slice()] {
        assert!(decode_windows_v2::<String>(invalid).is_err());
    }
    let astral = LosslessJson::new(r#""\ud83d\udd12""#.into()).unwrap();
    assert_eq!(astral.parse_value().unwrap().as_str(), Some("🔒"));
    let near = format!("{}0{}", "[".repeat(127), "]".repeat(127));
    assert!(serde_json::from_str::<serde_json::Value>(&near).is_ok());
    assert!(LosslessJson::new(near).is_ok());
    let deep = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    assert!(serde_json::from_str::<serde_json::Value>(&deep).is_err());
    assert!(LosslessJson::new(deep).is_err());
    let brackets_in_text = serde_json::to_string(&"[".repeat(1000)).unwrap();
    assert!(LosslessJson::new(brackets_in_text).is_ok());
}

#[test]
fn terminal_u64_outcome_and_exhausted_head_are_representable() {
    let v = fixture();
    let mut record = outcome(&v);
    record.sequence = u64::MAX;
    record.previous_hash = Some("cd".repeat(32));
    record.record_hash = windows_outcome_hash(&record);
    let hash = crate::outcomes::hex_to_32_bytes(&record.record_hash).unwrap();
    record.signature = B64.encode(SigningKey::from_bytes(&[7; 32]).sign(&hash).to_bytes());
    assert!(verify_windows_outcome(
        &record,
        &record.binding,
        v["device_key"].as_str().unwrap()
    ));
    let head = WindowsOutcomeHeadV2 {
        last_sequence: record.sequence,
        previous_hash: Some(record.record_hash),
    };
    assert!(head.is_valid());
    assert_eq!(head.last_sequence.checked_add(1), None);
    assert!(serde_json::from_value::<WindowsOutcomeHeadV2>(json!({
        "last_sequence":"18446744073709551616", "previous_hash":head.previous_hash
    }))
    .is_err());
}
