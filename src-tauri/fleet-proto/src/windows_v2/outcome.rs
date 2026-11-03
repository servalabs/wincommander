// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::{ActionClass, ActionOutcomeState, CommandFailureStage};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct WindowsActionOutcomeV2 {
    pub version: u16,
    pub binding: FleetEnrollmentBinding,
    pub receipt_id: String,
    #[serde(with = "super::decimal::unsigned")]
    #[cfg_attr(feature = "ts-codegen", ts(type = "string"))]
    pub sequence: u64,
    pub previous_hash: Option<String>,
    pub command_id: Option<String>,
    pub catalog_id: String,
    pub action_class: ActionClass,
    pub outcome: ActionOutcomeState,
    pub failure_stage: Option<CommandFailureStage>,
    pub observed_at: String,
    pub result_digest: LosslessJson,
    pub record_hash: String,
    pub signature: String,
}

pub fn windows_outcome_preimage(outcome: &WindowsActionOutcomeV2) -> Vec<u8> {
    crate::canonical_json_bytes(&json!([
        "fleet.windows.action-outcome.v2",
        outcome.version,
        outcome.binding,
        outcome.receipt_id,
        outcome.sequence.to_string(),
        outcome.previous_hash,
        outcome.command_id,
        outcome.catalog_id,
        outcome.action_class,
        outcome.outcome,
        outcome.failure_stage,
        outcome.observed_at,
        outcome.result_digest
    ]))
}

pub fn windows_outcome_hash(outcome: &WindowsActionOutcomeV2) -> String {
    hash_hex(&windows_outcome_preimage(outcome))
}

pub fn verify_windows_outcome(
    outcome: &WindowsActionOutcomeV2,
    expected: &FleetEnrollmentBinding,
    enrolled_outcome_key: &str,
) -> bool {
    if outcome.version != WINDOWS_WIRE_VERSION
        || !expected.is_valid()
        || outcome.binding != *expected
        || outcome.receipt_id.is_empty()
        || outcome.catalog_id.is_empty()
        || outcome.observed_at.is_empty()
        || outcome.command_id.as_ref().is_some_and(String::is_empty)
        || !valid_chain_position(outcome.sequence, outcome.previous_hash.as_deref())
        || (outcome.outcome == ActionOutcomeState::Pass) != outcome.failure_stage.is_none()
        || !strong_canonical_key(enrolled_outcome_key)
        || !canonical_signature(&outcome.signature)
        || outcome.record_hash != windows_outcome_hash(outcome)
    {
        return false;
    }
    let Ok(hash) = crate::outcomes::hex_to_32_bytes(&outcome.record_hash) else {
        return false;
    };
    crate::verify_signature_b64(enrolled_outcome_key, &hash, &outcome.signature)
}

pub(super) fn valid_chain_position(sequence: u64, previous: Option<&str>) -> bool {
    match (sequence, previous) {
        (1, None) => true,
        (2.., Some(hash)) => hash_shape(hash),
        _ => false,
    }
}
