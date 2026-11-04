// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/windows_v2_checkin_auth_vectors.json"
    ))
    .unwrap()
}

fn key() -> Vec<u8> {
    (0..32).collect()
}

#[test]
fn independently_generated_vectors_authenticate_the_exact_bytes() {
    let fixture = fixture();
    for vector in fixture["vectors"].as_array().unwrap() {
        let body = vector["body"].as_str().unwrap().as_bytes();
        let header = vector["header"].as_str().unwrap();
        assert_eq!(
            sign_windows_v2_checkin_auth(&key(), "POST", "/v1/agents/checkin", body).as_deref(),
            Some(header)
        );
        assert!(verify_windows_v2_checkin_auth(
            &key(),
            "POST",
            "/v1/agents/checkin",
            body,
            &[header]
        ));
        let mut changed = body.to_vec();
        changed.push(b' ');
        assert!(!verify_windows_v2_checkin_auth(
            &key(),
            "POST",
            "/v1/agents/checkin",
            &changed,
            &[header]
        ));
    }
    assert_ne!(
        fixture["vectors"][1]["header"],
        fixture["vectors"][2]["header"]
    );
}

#[test]
fn both_actual_method_and_path_are_required_and_wrong_key_fails() {
    let body = fixture()["vectors"][1]["body"].as_str().unwrap().to_owned();
    let header =
        sign_windows_v2_checkin_auth(&key(), "POST", "/v1/agents/checkin", body.as_bytes())
            .unwrap();
    for (method, path) in [
        ("GET", "/v1/agents/checkin"),
        ("post", "/v1/agents/checkin"),
        ("POST", "/v1/agents/checkin/"),
        ("POST", "/v1/agents/checkin?x=1"),
        ("POST", "/v1/windows/enroll-v2"),
    ] {
        assert!(sign_windows_v2_checkin_auth(&key(), method, path, body.as_bytes()).is_none());
        assert!(!verify_windows_v2_checkin_auth(
            &key(),
            method,
            path,
            body.as_bytes(),
            &[&header]
        ));
    }
    let mut wrong = key();
    wrong[31] ^= 1;
    assert!(!verify_windows_v2_checkin_auth(
        &wrong,
        "POST",
        "/v1/agents/checkin",
        body.as_bytes(),
        &[&header]
    ));
    for size in [0, 1, 31, 33, 64] {
        let secret = vec![1; size];
        assert!(sign_windows_v2_checkin_auth(
            &secret,
            "POST",
            "/v1/agents/checkin",
            body.as_bytes()
        )
        .is_none());
        assert!(!verify_windows_v2_checkin_auth(
            &secret,
            "POST",
            "/v1/agents/checkin",
            body.as_bytes(),
            &[&header]
        ));
    }
}

#[test]
fn duplicate_folded_non_ascii_and_noncanonical_header_values_fail() {
    let body = b"{}";
    let header = sign_windows_v2_checkin_auth(&key(), "POST", "/v1/agents/checkin", body).unwrap();
    assert!(!verify_windows_v2_checkin_auth(
        &key(),
        "POST",
        "/v1/agents/checkin",
        body,
        &[]
    ));
    assert!(!verify_windows_v2_checkin_auth(
        &key(),
        "POST",
        "/v1/agents/checkin",
        body,
        &[&header, &header]
    ));
    let mut wrong_mac = header.clone().into_bytes();
    wrong_mac[3] = if wrong_mac[3] == b'A' { b'B' } else { b'A' };
    let wrong_mac = String::from_utf8(wrong_mac).unwrap();
    // The last Base64 digit for a 32-byte MAC has two zero pad bits. An
    // alternative spelling of the same decoded bits must not be accepted.
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut noncanonical = header.clone().into_bytes();
    let last = noncanonical.len() - 2;
    let index = alphabet
        .iter()
        .position(|value| *value == noncanonical[last])
        .unwrap();
    assert_eq!(index & 3, 0);
    noncanonical[last] = alphabet[index | 1];
    let noncanonical = String::from_utf8(noncanonical).unwrap();
    let invalid = [
        String::new(),
        header.replace("v2.", "V2."),
        header.replace("v2.", "v1."),
        format!(" {header}"),
        format!("{header} "),
        format!("{header},{header}"),
        format!("{header}\n"),
        format!("{header}λ"),
        header.trim_end_matches('=').to_owned(),
        format!("{header}="),
        "v2.AA==".to_owned(),
        format!("v2.{}", "A".repeat(65_536)),
        noncanonical,
        "v2.!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!=".to_owned(),
        wrong_mac,
    ];
    for value in invalid {
        assert!(
            !verify_windows_v2_checkin_auth(&key(), "POST", "/v1/agents/checkin", body, &[&value]),
            "accepted malformed header"
        );
    }
}

#[test]
fn large_body_uses_the_same_exact_byte_contract_without_a_new_limit() {
    let body = vec![b'x'; 2 * 1024 * 1024];
    let header = sign_windows_v2_checkin_auth(&key(), "POST", "/v1/agents/checkin", &body).unwrap();
    assert_eq!(header, fixture()["large_header"].as_str().unwrap());
    assert!(verify_windows_v2_checkin_auth(
        &key(),
        "POST",
        "/v1/agents/checkin",
        &body,
        &[&header]
    ));
}
