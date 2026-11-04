// SPDX-License-Identifier: AGPL-3.0-or-later
//! Staged hybrid proof verification. This module does not enable a route,
//! provision keys, persist rollback floors, or authorize execution by itself.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

/// All fields must come from independently authenticated local policy.
/// Callers must durably advance the epoch floor before accepting later work.
pub struct WindowsHybridTrust<'a> {
    pub minimum_epoch: u64,
    pub ed_key_id: &'a str,
    pub ed25519_public: &'a [u8; 32],
    pub pq_key_id: &'a str,
    pub ml_dsa_65_public: &'a [u8; ML_DSA_65_PUBLIC_KEY_LEN],
}

fn verify_proof(
    proof: &[u8],
    payload: &[u8],
    purpose: Purpose,
    trust: &WindowsHybridTrust<'_>,
) -> Option<u64> {
    if trust.minimum_epoch == 0 {
        return None;
    }
    let verified = jws::verify(
        proof,
        &jws::Trust {
            purpose,
            minimum_epoch: trust.minimum_epoch,
            ed_key_id: trust.ed_key_id,
            ed25519_public: trust.ed25519_public,
            pq_key_id: trust.pq_key_id,
            ml_dsa_65_public: trust.ml_dsa_65_public,
        },
    )
    .ok()?;
    (verified.payload == payload).then_some(verified.epoch)
}

/// Bind the original enrollment request and response to the challenge and
/// HTTP status. Both peers must use the raw, complete transport bytes.
pub fn windows_hybrid_enrollment_preimage(
    challenge: &str,
    request: &[u8],
    status: u16,
    response: &[u8],
) -> Vec<u8> {
    crate::canonical_json_bytes(&serde_json::json!([
        "fleet.windows.enrollment.v2",
        challenge,
        windows_bytes_sha256(request),
        status,
        windows_bytes_sha256(response)
    ]))
}

/// Verify the exact enrollment exchange against independently provisioned
/// trust. The returned epoch must be protected before enrollment is installed.
pub fn verify_windows_hybrid_enrollment(
    challenge: &str,
    request: &[u8],
    status: u16,
    response: &[u8],
    expected_ed_key: &str,
    proof: &[u8],
    trust: &WindowsHybridTrust<'_>,
) -> Option<u64> {
    if status != 200 || STANDARD.encode(trust.ed25519_public) != expected_ed_key {
        return None;
    }
    verify_proof(
        proof,
        &windows_hybrid_enrollment_preimage(challenge, request, status, response),
        Purpose::FleetEnrollment,
        trust,
    )
}

/// Requires both signatures over this exact command and the existing strict
/// v2 action verification. The caller must also authenticate the enclosing
/// response, delivery ID and freshness, and persist the returned epoch.
pub fn verify_windows_hybrid_command(
    command: &WindowsCommandV2,
    expected: &FleetEnrollmentBinding,
    expected_signer: &str,
    proof: &[u8],
    trust: &WindowsHybridTrust<'_>,
) -> Option<u64> {
    if STANDARD.encode(trust.ed25519_public) != expected_signer
        || !verify_windows_command(command, expected, expected_signer)
    {
        return None;
    }
    let preimage = windows_command_preimage(&WindowsCommandSigningInput {
        binding: expected,
        idempotency_key: &command.idempotency_key,
        catalog_id: &command.catalog_id,
        action_class: command.action_class,
        payload: &command.payload,
        epoch_version: command.epoch_version,
    });
    verify_proof(proof, &preimage, Purpose::FleetCommand, trust)
}

/// Requires both signatures over the exact request/response receipt; the
/// enrollment binding and expected bytes are independently supplied.
pub fn verify_windows_hybrid_receipt(
    receipt: &WindowsReceiptProofV2,
    expected: &WindowsReceiptVerification<'_>,
    proof: &[u8],
    trust: &WindowsHybridTrust<'_>,
) -> Option<u64> {
    if STANDARD.encode(trust.ed25519_public) != expected.binding.authority_key_b64
        || !verify_windows_receipt(receipt, expected)
    {
        return None;
    }
    verify_proof(
        proof,
        &windows_receipt_preimage(receipt),
        Purpose::FleetReceipt,
        trust,
    )
}

#[cfg(test)]
#[path = "hybrid_tests.rs"]
mod tests;
