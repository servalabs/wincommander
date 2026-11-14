// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::CommandStatus;
use serde_json::json;

/// Persist this binding with the ACK; never relabel an older queued report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct WindowsAckV2 {
    pub binding: FleetEnrollmentBinding,
    pub command_id: String,
    pub report_id: String,
    pub success: bool,
    pub detail: Option<String>,
    pub result: Option<LosslessJson>,
    pub phase: Option<String>,
    pub terminal_reason: Option<String>,
}

pub fn windows_ack_hash(ack: &WindowsAckV2) -> String {
    hash_hex(&crate::canonical_json_bytes(&json!([
        "fleet.windows.ack.v2",
        ack
    ])))
}

/// Carry under windows_v2 in the existing full-body HMAC-v2 check-in envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct WindowsCheckinRequestV2 {
    pub version: u16,
    pub binding: FleetEnrollmentBinding,
    pub acks: Vec<WindowsAckV2>,
    pub outcomes: Vec<WindowsActionOutcomeV2>,
}

impl WindowsCheckinRequestV2 {
    /// Authentication, signatures, sequence and storage limits remain separate.
    pub fn has_expected_binding(&self, expected: &FleetEnrollmentBinding) -> bool {
        self.version == WINDOWS_WIRE_VERSION
            && expected.is_valid()
            && self.binding == *expected
            && self.acks.iter().all(|ack| ack.binding == *expected)
            && self.outcomes.iter().all(|outcome| {
                outcome.version == WINDOWS_WIRE_VERSION && outcome.binding == *expected
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
/// Rejected is definitive refusal, not durable retention. Storage errors omit
/// the receipt so the exact report remains retryable.
pub enum WindowsAckDisposition {
    Accepted,
    Duplicate,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct WindowsAckReceiptV2 {
    pub command_id: String,
    pub report_id: String,
    pub report_sha256: String,
    pub disposition: WindowsAckDisposition,
    pub resulting_status: Option<CommandStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
/// Accepted, Quarantined and Orphaned assert durable exact-record retention.
/// Rejected asserts refusal only; transient storage failures omit the receipt.
pub enum WindowsOutcomeDisposition {
    Accepted,
    Quarantined,
    Orphaned,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct WindowsOutcomeReceiptV2 {
    pub receipt_id: String,
    #[serde(with = "super::decimal::unsigned")]
    #[cfg_attr(feature = "ts-codegen", ts(type = "string"))]
    pub sequence: u64,
    pub record_hash: String,
    pub disposition: WindowsOutcomeDisposition,
}

/// Verified durable per-binding head: 0/None is empty; positive sequence
/// requires its last record hash. u64::MAX is a valid exhausted head. Derive a
/// next cursor only with checked_add; never wrap or discard pending records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct WindowsOutcomeHeadV2 {
    #[serde(with = "super::decimal::unsigned")]
    #[cfg_attr(feature = "ts-codegen", ts(type = "string"))]
    pub last_sequence: u64,
    pub previous_hash: Option<String>,
}

impl WindowsOutcomeHeadV2 {
    pub fn is_valid(&self) -> bool {
        match (self.last_sequence, self.previous_hash.as_deref()) {
            (0, None) => true,
            (1.., Some(hash)) => hash_shape(hash),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct WindowsCheckinResponseV2 {
    pub version: u16,
    pub binding: FleetEnrollmentBinding,
    /// Authenticated server command rollback floor, independent of a targeted
    /// policy revision. Compare every command against this same floor; several
    /// commands may have the same epoch. Full i64 range, encoded losslessly.
    #[serde(with = "super::decimal::epoch")]
    #[cfg_attr(feature = "ts-codegen", ts(type = "string"))]
    pub command_epoch_floor: i64,
    /// Server issuance time for this delivery, authenticated by the exact
    /// response proof. Required even when the response has no commands.
    #[serde(with = "super::decimal::epoch")]
    #[cfg_attr(feature = "ts-codegen", ts(type = "string"))]
    pub delivery_issued_unix_ms: i64,
    pub commands: Vec<WindowsCommandV2>,
    pub ack_receipts: Vec<WindowsAckReceiptV2>,
    pub outcome_receipts: Vec<WindowsOutcomeReceiptV2>,
    pub head: WindowsOutcomeHeadV2,
}

impl WindowsCheckinResponseV2 {
    /// Does not verify command signatures or authorize removing queued evidence.
    pub fn has_expected_binding(&self, expected: &FleetEnrollmentBinding) -> bool {
        self.version == WINDOWS_WIRE_VERSION
            && expected.is_valid()
            && self.binding == *expected
            && self
                .commands
                .iter()
                .all(|cmd| cmd.version == WINDOWS_WIRE_VERSION && cmd.binding == *expected)
            && self.head.is_valid()
    }
}
