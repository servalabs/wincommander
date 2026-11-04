// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use wincmd_pq_experiment::{jws::sign::assemble, VerifyError};

fn sign_proof(
    payload: &[u8],
    purpose: Purpose,
    ed: &SigningKey,
    pq: &PqdsaKeyPair,
    pq_key: &[u8; ML_DSA_65_PUBLIC_KEY_LEN],
) -> Vec<u8> {
    let ed_key = ed.verifying_key().to_bytes();
    let trust = jws::Trust {
        purpose,
        minimum_epoch: 1,
        ed_key_id: "fleet-ed",
        ed25519_public: &ed_key,
        pq_key_id: "fleet-pq",
        ml_dsa_65_public: pq_key,
    };
    assemble(
        payload,
        7,
        &trust,
        |message| Ok(ed.sign(message).to_bytes().to_vec()),
        |message| {
            let mut signature = vec![0; 3309];
            let written = pq
                .sign(message, &mut signature)
                .map_err(|_| VerifyError::InvalidSignature)?;
            signature.truncate(written);
            Ok(signature)
        },
    )
    .unwrap()
}

#[test]
fn hybrid_commands_and_receipts_require_both_keys_exact_bytes_and_purpose() {
    let ed = SigningKey::from_bytes(&[23; 32]);
    let ed_key = ed.verifying_key().to_bytes();
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let pq_key: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = pq.public_key().as_ref().try_into().unwrap();
    let trust = WindowsHybridTrust {
        minimum_epoch: 1,
        ed_key_id: "fleet-ed",
        ed25519_public: &ed_key,
        pq_key_id: "fleet-pq",
        ml_dsa_65_public: &pq_key,
    };
    let binding = FleetEnrollmentBinding {
        org_id: crate::OrgId("org-1".into()),
        device_id: crate::DeviceId("device-1".into()),
        enrollment_generation: "11111111-1111-4111-8111-111111111111".into(),
        authority_key_b64: STANDARD.encode(ed_key),
    };
    let mut command = WindowsCommandV2 {
        version: 2,
        binding: binding.clone(),
        command_id: "command-1".into(),
        idempotency_key: "stable-1".into(),
        catalog_id: "device.checkin_now".into(),
        action_class: crate::ActionClass::Safe,
        payload: LosslessJson::new("{}".into()).unwrap(),
        epoch_version: 4,
        delivery_expires_unix_ms: 100,
        signature: String::new(),
        signer_key: binding.authority_key_b64.clone(),
    };
    let command_bytes = windows_command_preimage(&WindowsCommandSigningInput {
        binding: &binding,
        idempotency_key: &command.idempotency_key,
        catalog_id: &command.catalog_id,
        action_class: command.action_class,
        payload: &command.payload,
        epoch_version: command.epoch_version,
    });
    command.signature = STANDARD.encode(ed.sign(&command_bytes).to_bytes());
    let proof = sign_proof(&command_bytes, Purpose::FleetCommand, &ed, &pq, &pq_key);
    let other_ed = SigningKey::from_bytes(&[24; 32]).verifying_key().to_bytes();
    let wrong_ed = WindowsHybridTrust {
        ed25519_public: &other_ed,
        ..trust
    };
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &proof,
        &wrong_ed
    )
    .is_none());
    let wrong_id = WindowsHybridTrust {
        ed_key_id: "other-ed",
        ..trust
    };
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &proof,
        &wrong_id
    )
    .is_none());
    let mut other_binding = binding.clone();
    other_binding.enrollment_generation = "22222222-2222-4222-8222-222222222222".into();
    assert!(verify_windows_hybrid_command(
        &command,
        &other_binding,
        &binding.authority_key_b64,
        &proof,
        &trust
    )
    .is_none());
    let mut missing: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    missing["signatures"].as_array_mut().unwrap().pop();
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &serde_json::to_vec(&missing).unwrap(),
        &trust
    )
    .is_none());
    let mut bad_ed: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    let signature = bad_ed["signatures"][0]["signature"]
        .as_str()
        .unwrap()
        .to_owned();
    let head = if signature.starts_with('A') { "B" } else { "A" };
    bad_ed["signatures"][0]["signature"] = format!("{}{}", head, &signature[1..]).into();
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &serde_json::to_vec(&bad_ed).unwrap(),
        &trust
    )
    .is_none());
    assert_eq!(
        verify_windows_hybrid_command(
            &command,
            &binding,
            &binding.authority_key_b64,
            &proof,
            &trust
        ),
        Some(7)
    );
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        b"",
        &trust
    )
    .is_none());
    let other_pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let other_pq_key: [u8; ML_DSA_65_PUBLIC_KEY_LEN] =
        other_pq.public_key().as_ref().try_into().unwrap();
    let wrong_key = WindowsHybridTrust {
        ml_dsa_65_public: &other_pq_key,
        ..trust
    };
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &proof,
        &wrong_key
    )
    .is_none());
    let zero_floor = WindowsHybridTrust {
        minimum_epoch: 0,
        ..trust
    };
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &proof,
        &zero_floor
    )
    .is_none());
    let higher_floor = WindowsHybridTrust {
        minimum_epoch: 8,
        ..trust
    };
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &proof,
        &higher_floor
    )
    .is_none());
    let wrong_purpose = sign_proof(&command_bytes, Purpose::FleetReceipt, &ed, &pq, &pq_key);
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &wrong_purpose,
        &trust
    )
    .is_none());
    let mut forged: serde_json::Value = serde_json::from_slice(&proof).unwrap();
    let pq_sig = forged["signatures"][1]["signature"]
        .as_str()
        .unwrap()
        .to_string();
    let replacement = if pq_sig.starts_with('A') { "B" } else { "A" };
    forged["signatures"][1]["signature"] = format!("{}{}", replacement, &pq_sig[1..]).into();
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &serde_json::to_vec(&forged).unwrap(),
        &trust
    )
    .is_none());
    let mut changed_payload = command.clone();
    changed_payload.payload = LosslessJson::new("{ }".into()).unwrap();
    let changed_bytes = windows_command_preimage(&WindowsCommandSigningInput {
        binding: &binding,
        idempotency_key: &changed_payload.idempotency_key,
        catalog_id: &changed_payload.catalog_id,
        action_class: changed_payload.action_class,
        payload: &changed_payload.payload,
        epoch_version: changed_payload.epoch_version,
    });
    changed_payload.signature = STANDARD.encode(ed.sign(&changed_bytes).to_bytes());
    assert_ne!(changed_bytes, command_bytes);
    assert!(verify_windows_hybrid_command(
        &changed_payload,
        &binding,
        &binding.authority_key_b64,
        &proof,
        &trust
    )
    .is_none());
    command.idempotency_key.push('x');
    assert!(verify_windows_hybrid_command(
        &command,
        &binding,
        &binding.authority_key_b64,
        &proof,
        &trust
    )
    .is_none());

    let request = b"request";
    let response = b"response";
    let mut receipt = WindowsReceiptProofV2 {
        version: 2,
        binding: binding.clone(),
        nonce: "nonce".into(),
        request_sha256: windows_bytes_sha256(request),
        http_status: 200,
        response_sha256: windows_bytes_sha256(response),
        signature: String::new(),
    };
    let bytes = windows_receipt_preimage(&receipt);
    receipt.signature = STANDARD.encode(ed.sign(&bytes).to_bytes());
    let receipt_proof = sign_proof(&bytes, Purpose::FleetReceipt, &ed, &pq, &pq_key);
    let expected = WindowsReceiptVerification {
        binding: &binding,
        nonce: "nonce",
        request_bytes: request,
        http_status: 200,
        response_bytes: response,
    };
    assert_eq!(
        verify_windows_hybrid_receipt(&receipt, &expected, &receipt_proof, &trust),
        Some(7)
    );
    let tampered = WindowsReceiptVerification {
        response_bytes: b"changed",
        ..expected
    };
    assert!(verify_windows_hybrid_receipt(&receipt, &tampered, &receipt_proof, &trust).is_none());
    let wrong_nonce = WindowsReceiptVerification {
        nonce: "other",
        ..expected
    };
    assert!(
        verify_windows_hybrid_receipt(&receipt, &wrong_nonce, &receipt_proof, &trust).is_none()
    );
    let wrong_request = WindowsReceiptVerification {
        request_bytes: b"changed",
        ..expected
    };
    assert!(
        verify_windows_hybrid_receipt(&receipt, &wrong_request, &receipt_proof, &trust).is_none()
    );
    let wrong_status = WindowsReceiptVerification {
        http_status: 201,
        ..expected
    };
    assert!(
        verify_windows_hybrid_receipt(&receipt, &wrong_status, &receipt_proof, &trust).is_none()
    );
    assert!(verify_windows_hybrid_receipt(&receipt, &expected, &proof, &trust).is_none());
}
