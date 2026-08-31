// SPDX-License-Identifier: AGPL-3.0-or-later
//! Signed action-outcome wire contract. B1 owns this module after G-1.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{write_canonical, ActionClass, DeviceId, OrgId};

/// A device-signed, hash-chained record of an action outcome. These records
/// travel only inside the authenticated check-in envelope; `device_id` is
/// nevertheless signed so a captured record cannot be replayed for another
/// device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct ActionOutcome {
    #[serde(default = "default_action_outcome_version")]
    pub version: u16,
    pub receipt_id: String,
    pub device_id: DeviceId,
    pub sequence: u64,
    #[serde(default)]
    pub previous_hash: Option<String>,
    #[serde(default)]
    pub command_id: Option<String>,
    pub catalog_id: String,
    pub action_class: ActionClass,
    pub outcome: ActionOutcomeState,
    /// Required for failed/unknown records; absent only for a confirmed pass.
    pub failure_stage: Option<CommandFailureStage>,
    pub observed_at: String,
    /// Bounded structured data interpreted by the catalog-specific result
    /// seam. The server validates its size and readiness-self-test shape.
    pub result_digest: Value,
    /// Lowercase SHA-256 hex of [`canonical_action_outcome_bytes`].
    pub record_hash: String,
    /// Base64 Ed25519 signature over the 32 raw bytes of `record_hash`.
    pub signature: String,
}

fn default_action_outcome_version() -> u16 {
    1
}

/// Factual outcome state. `Unknown` is deliberately distinct from a success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcomeState {
    Pass,
    Fail,
    Unknown,
}

/// Where a command stopped. This is part of the signed receipt so an operator
/// can distinguish a rejected policy gate from an endpoint execution failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum CommandFailureStage {
    Resolve,
    Gate,
    Dispatch,
    Execute,
    Report,
}

/// Server-signed recovery point for an endpoint that has lost only its local
/// outcome-chain cursor. It preserves every accepted record: the next receipt
/// continues from `previous_hash` at `next_sequence`, rather than replacing or
/// deleting the existing chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct ActionOutcomeRecoveryCheckpoint {
    pub checkpoint_id: String,
    pub org_id: OrgId,
    pub device_id: DeviceId,
    pub next_sequence: u64,
    pub previous_hash: Option<String>,
    pub issued_at: String,
    pub signature: String,
    pub signer_key: String,
}

pub fn action_outcome_recovery_checkpoint_preimage(
    checkpoint: &ActionOutcomeRecoveryCheckpoint,
) -> Vec<u8> {
    let envelope = serde_json::json!({
        "checkpoint_id": checkpoint.checkpoint_id,
        "device_id": checkpoint.device_id.0,
        "issued_at": checkpoint.issued_at,
        "next_sequence": checkpoint.next_sequence,
        "org_id": checkpoint.org_id.0,
        "previous_hash": checkpoint.previous_hash,
    });
    let mut canonical = String::new();
    write_canonical(&envelope, &mut canonical);
    canonical.into_bytes()
}

pub fn verify_action_outcome_recovery_checkpoint(
    checkpoint: &ActionOutcomeRecoveryCheckpoint,
    public_key_b64: &str,
) -> bool {
    if checkpoint.signer_key != public_key_b64 || checkpoint.next_sequence == 0 {
        return false;
    }
    crate::verify_signature_b64(
        public_key_b64,
        &action_outcome_recovery_checkpoint_preimage(checkpoint),
        &checkpoint.signature,
    )
}

/// Per-step status for a non-destructive readiness self-test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum ReadinessState {
    Pass,
    Fail,
    Unknown,
}

/// Structured result required for the `readiness.self_test` outcome catalog.
/// It documents verification only; it never authorizes a destructive action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct ReadinessSelfTestResult {
    pub arm_predicate: ReadinessState,
    pub signed_dispatch: ReadinessState,
    pub simulate_branch: ReadinessState,
    pub recovery_path: ReadinessState,
    pub observed_at: String,
}

pub fn canonical_action_outcome_bytes(outcome: &ActionOutcome) -> Vec<u8> {
    let envelope = serde_json::json!({
        "version": outcome.version,
        "receipt_id": outcome.receipt_id,
        "device_id": outcome.device_id.0,
        "sequence": outcome.sequence,
        "previous_hash": outcome.previous_hash,
        "command_id": outcome.command_id,
        "catalog_id": outcome.catalog_id,
        "action_class": outcome.action_class.as_wire_str(),
        "outcome": match outcome.outcome {
            ActionOutcomeState::Pass => "pass",
            ActionOutcomeState::Fail => "fail",
            ActionOutcomeState::Unknown => "unknown",
        },
        "failure_stage": outcome.failure_stage.map(|stage| match stage {
            CommandFailureStage::Resolve => "resolve",
            CommandFailureStage::Gate => "gate",
            CommandFailureStage::Dispatch => "dispatch",
            CommandFailureStage::Execute => "execute",
            CommandFailureStage::Report => "report",
        }),
        "observed_at": outcome.observed_at,
        "result_digest": outcome.result_digest,
    });
    let mut canonical = String::new();
    write_canonical(&envelope, &mut canonical);
    canonical.into_bytes()
}

pub fn action_outcome_hash(outcome: &ActionOutcome) -> String {
    let digest = Sha256::digest(canonical_action_outcome_bytes(outcome));
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn canonical_json_sha256(value: &Value) -> String {
    let mut canonical = String::new();
    write_canonical(value, &mut canonical);
    let digest = Sha256::digest(canonical.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn verify_action_outcome(outcome: &ActionOutcome, public_key_b64: &str) -> bool {
    if outcome.version != 1 || outcome.record_hash != action_outcome_hash(outcome) {
        return false;
    }
    let Ok(hash) = hex_to_32_bytes(&outcome.record_hash) else {
        return false;
    };
    let Ok(key_bytes) = B64.decode(public_key_b64) else {
        return false;
    };
    let Ok(key_array) = <[u8; 32]>::try_from(key_bytes.as_slice()) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(&key_array) else {
        return false;
    };
    let Ok(signature_bytes) = B64.decode(&outcome.signature) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(&signature_bytes) else {
        return false;
    };
    verifying_key.verify(&hash, &signature).is_ok()
}

pub(crate) fn hex_to_32_bytes(input: &str) -> Result<[u8; 32], ()> {
    if input.len() != 64 || !input.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(());
    }
    let mut bytes = [0u8; 32];
    for (index, chunk) in input.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        bytes[index] = std::str::from_utf8(chunk)
            .ok()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            .ok_or(())?;
    }
    Ok(bytes)
}
