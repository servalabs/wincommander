// SPDX-License-Identifier: AGPL-3.0-or-later
//! Canonical agent check-in wire envelopes.
//!
//! These top-level envelopes are intentionally tolerant: rolling fleets can
//! add optional fields without making an older peer reject the whole request.
//! Signed nested records such as [`ActionOutcome`] remain strict in their own
//! modules.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ActionOutcome, ActionOutcomeRecoveryCheckpoint, ClipboardEventReport, DeviceResourceSample,
    InkReceiptReport, LocalAlertReport, PostureReport, SignedCommand,
};

/// Current version of the additive check-in envelope.
pub const CHECKIN_PROTOCOL_VERSION: i64 = 1;

fn default_protocol_version() -> i64 {
    CHECKIN_PROTOCOL_VERSION
}

fn default_hmac_version() -> i64 {
    1
}

fn default_max_hits_per_device() -> usize {
    50
}

/// Legacy-compatible command acknowledgement carried on the next check-in.
///
/// The first four fields match deployed agents and servers. The optional
/// reporting fields allow a negotiated peer to deduplicate progress reports
/// without changing the containing envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CheckinAck {
    pub command_id: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
}

/// Latest server-observed cursor for the signed action-outcome chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ActionOutcomeHead {
    pub sequence: u64,
    pub record_hash: String,
}

/// Optional per-ACK confirmation used by peers that negotiate receipt support.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CheckinAckReceipt {
    pub command_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    pub disposition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resulting_status: Option<String>,
}

/// Optional per-outcome confirmation used by peers that negotiate receipt
/// support. This distinguishes acceptance from quarantine/orphan handling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ActionOutcomeReceipt {
    pub receipt_id: String,
    pub sequence: u64,
    pub disposition: String,
}

/// PII-free device health reported with a normal check-in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct HealthSnapshot {
    pub encryption_on: Option<bool>,
    pub patch_state: Option<String>,
    pub av_on: Option<bool>,
    pub os_version: Option<String>,
    pub sovereignty_score: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_facts: Option<Value>,
}

/// One content-search job delivered in a check-in response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct PendingSearchJob {
    pub job_id: String,
    pub query: String,
    #[serde(default = "default_max_hits_per_device")]
    pub max_hits_per_device: usize,
}

/// `POST /v1/agents/checkin` request body.
///
/// Required authentication fields deliberately have no serde defaults.
/// Everything else is additive/defaulted for mixed-version operation. This
/// top-level type must not use `deny_unknown_fields`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CheckinRequest {
    #[serde(default = "default_protocol_version")]
    pub protocol_version: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    pub device_id: String,
    pub ts: i64,
    pub nonce: String,
    #[serde(default = "default_hmac_version")]
    pub hmac_version: i64,
    pub hmac: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acks: Vec<CheckinAck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_outcomes: Vec<ActionOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posture: Option<PostureReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<DeviceResourceSample>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub productivity: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argus: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoy_tripwire: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_status: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_health: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_delivery_opt_out: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub productivity_detail: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inventory: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<LocalAlertReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clipboard_events: Vec<ClipboardEventReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ink_receipts: Vec<InkReceiptReport>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub padding: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub decoy: bool,
}

/// Successful check-in response.
///
/// `Command` is generic so an older agent may retain compatibility-only
/// command metadata while the canonical server form remains the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(bound(
    serialize = "Command: Serialize",
    deserialize = "Command: Deserialize<'de>"
))]
pub struct CheckinResponse<Command = SignedCommand> {
    #[serde(default = "default_protocol_version")]
    pub protocol_version: i64,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub commands: Vec<Command>,
    #[serde(default)]
    pub all_clear: bool,
    /// Optional while old and new server versions overlap.
    #[serde(default)]
    pub policy: Option<Value>,
    #[serde(default)]
    pub padding: String,
    #[serde(default)]
    pub pending_search_jobs: Vec<PendingSearchJob>,
    #[serde(default)]
    pub cancelled_search_job_ids: Vec<String>,
    #[serde(default)]
    pub pending_approval: bool,
    #[serde(default)]
    pub ack_receipts: Vec<CheckinAckReceipt>,
    #[serde(default)]
    pub action_outcome_receipts: Vec<ActionOutcomeReceipt>,
    #[serde(default)]
    pub action_outcome_head: Option<ActionOutcomeHead>,
    #[serde(default)]
    pub action_outcome_recovery_checkpoint: Option<ActionOutcomeRecoveryCheckpoint>,
    #[serde(default)]
    pub productivity_detail_receipt: Option<Value>,
    #[serde(default)]
    pub inventory_receipt: Option<Value>,
}

impl<Command> Default for CheckinResponse<Command> {
    fn default() -> Self {
        Self {
            protocol_version: CHECKIN_PROTOCOL_VERSION,
            capabilities: Vec::new(),
            server_time: None,
            commands: Vec::new(),
            all_clear: false,
            policy: None,
            padding: String::new(),
            pending_search_jobs: Vec::new(),
            cancelled_search_job_ids: Vec::new(),
            pending_approval: false,
            ack_receipts: Vec::new(),
            action_outcome_receipts: Vec::new(),
            action_outcome_head: None,
            action_outcome_recovery_checkpoint: None,
            productivity_detail_receipt: None,
            inventory_receipt: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_minimal_request_defaults_additive_fields() {
        let request: CheckinRequest = serde_json::from_value(serde_json::json!({
            "device_id": "device-1",
            "ts": 1_700_000_000,
            "nonce": "nonce-with-at-least-16-chars",
            "hmac": "mac",
            "future_field": { "ignored": true }
        }))
        .expect("top-level unknown fields must remain compatible");

        assert_eq!(request.protocol_version, CHECKIN_PROTOCOL_VERSION);
        assert_eq!(request.hmac_version, 1);
        assert!(request.acks.is_empty());
        assert!(request.action_outcomes.is_empty());
        assert!(request.posture.is_none());
    }

    #[test]
    fn posture_is_structured_or_absent_never_a_status_string() {
        let request: CheckinRequest = serde_json::from_value(serde_json::json!({
            "device_id": "device-1",
            "ts": 1,
            "nonce": "nonce-with-at-least-16-chars",
            "hmac": "mac",
            "posture": {
                "applied_epoch": 7,
                "settings_hash": "sha256",
                "shield_running": true
            }
        }))
        .unwrap();
        assert_eq!(request.posture.unwrap().applied_epoch, 7);

        let legacy_string = serde_json::from_value::<CheckinRequest>(serde_json::json!({
            "device_id": "device-1",
            "ts": 1,
            "nonce": "nonce-with-at-least-16-chars",
            "hmac": "mac",
            "posture": "nominal"
        }));
        assert!(legacy_string.is_err());
    }

    #[test]
    fn old_response_without_policy_or_new_receipts_still_parses() {
        let response: CheckinResponse = serde_json::from_value(serde_json::json!({
            "commands": [],
            "all_clear": true,
            "config_epoch": null,
            "future_response_field": 42
        }))
        .unwrap();

        assert!(response.policy.is_none());
        assert!(response.ack_receipts.is_empty());
        assert!(response.action_outcome_receipts.is_empty());
        assert_eq!(response.protocol_version, CHECKIN_PROTOCOL_VERSION);
    }
}
