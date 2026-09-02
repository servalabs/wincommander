// SPDX-License-Identifier: AGPL-3.0-or-later
//! Neutral signed capability format for autonomous test builds.
//!
//! This module defines bytes and verification only. Test-run policy, signer
//! custody, scenario implementation, and orchestration remain outside this
//! shared protocol crate.

use serde::{Deserialize, Serialize};

use crate::{write_canonical, verify_signature_b64, ActionClass, DeviceId};

/// A short-lived capability authorizing one test build to run a fixed set of
/// registered scenarios for one Fleet-enrolled device.
///
/// The capability is also bound to the ordinary Fleet command which exercised
/// the normal command gates for this test.  A test build must not treat the
/// capability as authority to execute an arbitrary action merely because it
/// names an allowed scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutonomousTestCapability {
    #[serde(default = "default_version")]
    pub version: u16,
    pub run_id: String,
    pub device_id: DeviceId,
    pub command_id: String,
    pub catalog_id: String,
    pub action_class: ActionClass,
    pub scenarios: Vec<String>,
    pub not_before: String,
    pub expires_at: String,
    pub issuer_key_id: String,
    pub nonce: String,
    pub signature: String,
}

fn default_version() -> u16 {
    2
}

/// Canonical bytes signed by the dedicated Fleet autonomous-test signer.
pub fn autonomous_test_capability_preimage(capability: &AutonomousTestCapability) -> Vec<u8> {
    let value = serde_json::json!({
        "version": capability.version,
        "run_id": capability.run_id,
        "device_id": capability.device_id.0,
        "command_id": capability.command_id,
        "catalog_id": capability.catalog_id,
        "action_class": capability.action_class.as_wire_str(),
        "scenarios": capability.scenarios,
        "not_before": capability.not_before,
        "expires_at": capability.expires_at,
        "issuer_key_id": capability.issuer_key_id,
        "nonce": capability.nonce,
    });
    let mut canonical = String::new();
    write_canonical(&value, &mut canonical);
    canonical.into_bytes()
}

/// Verify that a capability was signed by the expected test signer. Callers
/// must separately validate timestamps, registered scenarios, and local device
/// state before they act on it.
pub fn verify_autonomous_test_capability(
    capability: &AutonomousTestCapability,
    expected_issuer_key_id: &str,
    public_key_b64: &str,
) -> bool {
    capability.version == 2
        && capability.issuer_key_id == expected_issuer_key_id
        && !capability.run_id.is_empty()
        && !capability.device_id.0.is_empty()
        && !capability.command_id.is_empty()
        && !capability.catalog_id.is_empty()
        && !capability.scenarios.is_empty()
        && !capability.nonce.is_empty()
        && verify_signature_b64(
            public_key_b64,
            &autonomous_test_capability_preimage(capability),
            &capability.signature,
        )
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn sample() -> AutonomousTestCapability {
        AutonomousTestCapability {
            version: 2,
            run_id: "e0c14d4a-5a83-4d35-9a0d-a21d9fac23c2".into(),
            device_id: DeviceId("d3d9a411-c29e-42df-a1ea-a50180b952cb".into()),
            command_id: "bdb7653f-8df6-48ad-bb29-bf03a7ce4ce8".into(),
            catalog_id: "mesh.status".into(),
            action_class: ActionClass::Safe,
            scenarios: vec!["fleet.preflight".into()],
            not_before: "2026-08-31T00:00:00Z".into(),
            expires_at: "2026-08-31T01:00:00Z".into(),
            issuer_key_id: "autonomy-lab-1".into(),
            nonce: "8c5c0e3d2e6e4ba4b9d6434cd1dcda58".into(),
            signature: String::new(),
        }
    }

    #[test]
    fn signed_capability_requires_the_expected_signer_and_unmodified_preimage() {
        let signing_key = SigningKey::from_bytes(&[17; 32]);
        let public_key = B64.encode(signing_key.verifying_key().as_bytes());
        let mut capability = sample();
        capability.signature = B64.encode(
            signing_key
                .sign(&autonomous_test_capability_preimage(&capability))
                .to_bytes(),
        );

        assert!(verify_autonomous_test_capability(
            &capability,
            "autonomy-lab-1",
            &public_key
        ));
        assert!(!verify_autonomous_test_capability(
            &capability,
            "other-key",
            &public_key
        ));

        capability.command_id = "0c88ddd5-9819-4194-8099-49b7221b58d9".into();
        assert!(!verify_autonomous_test_capability(
            &capability,
            "autonomy-lab-1",
            &public_key
        ));

        let mut capability = sample();
        capability.signature = B64.encode(
            signing_key
                .sign(&autonomous_test_capability_preimage(&capability))
                .to_bytes(),
        );
        capability.catalog_id = "policy.reapply".into();
        assert!(!verify_autonomous_test_capability(
            &capability,
            "autonomy-lab-1",
            &public_key
        ));
    }

    #[test]
    fn capability_wire_format_is_strict_camel_case() {
        let capability = sample();
        let value = serde_json::to_value(&capability).unwrap();
        assert!(value.get("runId").is_some());
        assert!(value.get("commandId").is_some());
        assert!(value.get("catalogId").is_some());
        assert!(value.get("actionClass").is_some());
        assert!(value.get("run_id").is_none());
        assert!(serde_json::from_value::<AutonomousTestCapability>(value).is_ok());
    }
}
