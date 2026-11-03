// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use ed25519_dalek::{Signer, SigningKey};

fn trust() -> FleetTrust {
    FleetTrust {
        org_id: "org-a".into(),
        device_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
        enrollment_generation: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
        signer_key: STANDARD.encode(SigningKey::from_bytes(&[11; 32]).verifying_key().as_bytes()),
    }
}
fn signed(trust: &FleetTrust, nonce: &str, request: &[u8]) -> Value {
    let hash = STANDARD.encode(Sha256::digest(request));
    let input = fleet_proto::canonical_json_bytes(&json!([
        "fleet.windows.service-presence.v1",
        trust.org_id,
        trust.device_id,
        trust.enrollment_generation,
        nonce,
        hash
    ]));
    let signature = STANDARD.encode(SigningKey::from_bytes(&[11; 32]).sign(&input).to_bytes());
    json!({"protocol_version":1,"org_id":trust.org_id,"device_id":trust.device_id,
        "enrollment_generation":trust.enrollment_generation,"nonce":nonce,"request_sha256":hash,"signature":signature})
}
#[test]
fn request_hmac_and_server_proof_bind_exact_endpoint_and_bytes() {
    let trust = trust();
    let secret = [3u8; 32];
    let (nonce, request) = make_request(&trust, "https://fleet.test", &secret, 1000).unwrap();
    assert!(nonce.starts_with("sp-") && nonce.len() == 35);
    let body: Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(
        body["hmac"],
        fleet_agent_core::compute_request_hmac_v2(&secret, "POST", PATH, &body).unwrap()
    );
    assert_ne!(
        body["hmac"],
        fleet_agent_core::compute_request_hmac_v2(&secret, "POST", "/v1/agents/checkin", &body)
            .unwrap()
    );
    let response = serde_json::to_vec(&signed(&trust, &nonce, &request)).unwrap();
    assert!(verify_response(&trust, &nonce, &request, &response));
    let mut different = request.clone();
    different.push(b' ');
    assert!(!verify_response(&trust, &nonce, &different, &response));
    let mut different_origin = body.clone();
    different_origin["origin"] = json!("https://other.test");
    different_origin["hmac"] = json!(fleet_agent_core::compute_request_hmac_v2(&secret, "POST", PATH, &different_origin).unwrap());
    assert!(!verify_response(&trust, &nonce, &serde_json::to_vec(&different_origin).unwrap(), &response));
    assert!(!verify_response(&trust, "sp-other", &request, &response));
    for field in [
        "device_id",
        "org_id",
        "enrollment_generation",
        "nonce",
        "request_sha256",
        "signature",
    ] {
        let mut changed = signed(&trust, &nonce, &request);
        changed[field] = json!("different");
        assert!(
            !verify_response(
                &trust,
                &nonce,
                &request,
                &serde_json::to_vec(&changed).unwrap()
            ),
            "{field}"
        );
    }
    let mut changed = signed(&trust, &nonce, &request);
    changed["extra"] = json!(true);
    assert!(!verify_response(
        &trust,
        &nonce,
        &request,
        &serde_json::to_vec(&changed).unwrap()
    ));
    assert!(!verify_response(
        &trust,
        &nonce,
        &request,
        &vec![b' '; MAX_RESPONSE + 1]
    ));
    assert!(make_request(&trust, "https://fleet.test", &[1; 31], 1000).is_err());
}

#[test]
fn opaque_device_identifier_is_preserved_exactly_in_signed_presence() {
    let mut binding = trust();
    binding.device_id = "Device-Alpha_42".into();
    let (nonce, request) = make_request(&binding, "https://fleet.test", &[3u8; 32], 1000).unwrap();
    let response = serde_json::to_vec(&signed(&binding, &nonce, &request)).unwrap();
    assert!(verify_response(&binding, &nonce, &request, &response));
    binding.device_id = "device-alpha_42".into();
    assert!(!verify_response(&binding, &nonce, &request, &response));
}
