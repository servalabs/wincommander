// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::ActionClass;
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct WindowsCommandV2 {
    pub version: u16,
    pub binding: FleetEnrollmentBinding,
    pub command_id: String,
    pub idempotency_key: String,
    pub catalog_id: String,
    pub action_class: ActionClass,
    pub payload: LosslessJson,
    #[serde(with = "super::decimal::epoch")]
    #[cfg_attr(feature = "ts-codegen", ts(type = "string"))]
    pub epoch_version: i64,
    pub signature: String,
    pub signer_key: String,
}

/// Offline signers know the idempotency key before a delivery ID is assigned.
/// The separately verified response proof must bind the assigned command_id.
pub struct WindowsCommandSigningInput<'a> {
    pub binding: &'a FleetEnrollmentBinding,
    pub idempotency_key: &'a str,
    pub catalog_id: &'a str,
    pub action_class: ActionClass,
    pub payload: &'a LosslessJson,
    pub epoch_version: i64,
}

pub fn windows_command_preimage(input: &WindowsCommandSigningInput<'_>) -> Vec<u8> {
    crate::canonical_json_bytes(&json!([
        "fleet.windows.command.v2",
        input.binding,
        input.idempotency_key,
        input.catalog_id,
        input.action_class,
        input.payload,
        input.epoch_version.to_string()
    ]))
}

/// This verifies action authorization, not delivery identity or freshness.
/// Select expected_signer independently using the catalog's authority policy.
pub fn verify_windows_command(
    command: &WindowsCommandV2,
    expected: &FleetEnrollmentBinding,
    expected_signer: &str,
) -> bool {
    command.version == WINDOWS_WIRE_VERSION
        && expected.is_valid()
        && command.binding == *expected
        && !command.command_id.is_empty()
        && !command.idempotency_key.is_empty()
        && !command.catalog_id.is_empty()
        && command.epoch_version >= 0
        && command.signer_key == expected_signer
        && strong_canonical_key(expected_signer)
        && canonical_signature(&command.signature)
        && crate::verify_signature_b64(
            expected_signer,
            &windows_command_preimage(&WindowsCommandSigningInput {
                binding: expected,
                idempotency_key: &command.idempotency_key,
                catalog_id: &command.catalog_id,
                action_class: command.action_class,
                payload: &command.payload,
                epoch_version: command.epoch_version,
            }),
            &command.signature,
        )
}
