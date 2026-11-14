// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use serde_json::json;

pub use super::validation::windows_bytes_sha256;

/// Detached from the response body to avoid a self-referential body hash.
/// Asserts committed receipt state; cryptographic verification cannot establish
/// physical durability, service liveness, or action success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct WindowsReceiptProofV2 {
    pub version: u16,
    pub binding: FleetEnrollmentBinding,
    pub nonce: String,
    pub request_sha256: String,
    pub http_status: u16,
    pub response_sha256: String,
    pub signature: String,
}

pub fn windows_receipt_preimage(proof: &WindowsReceiptProofV2) -> Vec<u8> {
    crate::canonical_json_bytes(&json!([
        "fleet.windows.outcome-receipt.v2",
        proof.binding,
        proof.nonce,
        proof.request_sha256,
        proof.http_status,
        proof.response_sha256
    ]))
}

/// The caller must retain these bytes/nonce for one live request and fence its
/// lifecycle before applying a verified reply. This module owns no clock/state.
pub struct WindowsReceiptVerification<'a> {
    pub binding: &'a FleetEnrollmentBinding,
    pub nonce: &'a str,
    pub request_bytes: &'a [u8],
    pub http_status: u16,
    pub response_bytes: &'a [u8],
}

pub fn verify_windows_receipt(
    proof: &WindowsReceiptProofV2,
    expected: &WindowsReceiptVerification<'_>,
) -> bool {
    proof.version == WINDOWS_WIRE_VERSION
        && expected.binding.is_valid()
        && proof.binding == *expected.binding
        && !expected.nonce.is_empty()
        && proof.nonce == expected.nonce
        && expected.http_status == 200
        && proof.http_status == expected.http_status
        && proof.request_sha256 == windows_bytes_sha256(expected.request_bytes)
        && proof.response_sha256 == windows_bytes_sha256(expected.response_bytes)
        && canonical_signature(&proof.signature)
        && crate::verify_signature_b64(
            &expected.binding.authority_key_b64,
            &windows_receipt_preimage(proof),
            &proof.signature,
        )
}
