// SPDX-License-Identifier: AGPL-3.0-or-later
//! Closed signed receipts for debug-only autonomous Fleet test builds.
//!
//! An observation is deliberately not an [`ActionOutcome`]: it can reference
//! a normal command, but it never completes that command or enters its hash
//! chain. This keeps command execution proof distinct from test measurement.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{verify_signature_b64, write_canonical, ActionClass, DeviceId};

/// A separate, short-lived admission pass for a disposable Fleet lab device.
///
/// This is deliberately not an [`AutonomousTestCapability`].  Enrollment
/// happens before a device can be a member of a test run, and accepting a test
/// receipt capability at enrollment would let one authority cross two security
/// boundaries.  The Fleet server creates this value, the Pro agent forwards it
/// once in its normal enrollment request, and Fleet atomically consumes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetLabEnrollmentCapability {
    pub version: u16,
    pub capability_id: String,
    pub org_id: String,
    pub device_id: DeviceId,
    /// Canonical http(s) origin of the isolated Fleet lab instance.
    pub fleet_origin: String,
    pub not_before: String,
    pub expires_at: String,
    pub issuer_key_id: String,
    pub nonce: String,
    pub signature: String,
}

pub fn fleet_lab_enrollment_capability_preimage(
    capability: &FleetLabEnrollmentCapability,
) -> Vec<u8> {
    let value = serde_json::json!({
        "version": capability.version,
        "capability_id": capability.capability_id,
        "org_id": capability.org_id,
        "device_id": capability.device_id.0,
        "fleet_origin": capability.fleet_origin,
        "not_before": capability.not_before,
        "expires_at": capability.expires_at,
        "issuer_key_id": capability.issuer_key_id,
        "nonce": capability.nonce,
    });
    let mut canonical = String::new();
    write_canonical(&value, &mut canonical);
    canonical.into_bytes()
}

pub fn verify_fleet_lab_enrollment_capability(
    capability: &FleetLabEnrollmentCapability,
    expected_issuer_key_id: &str,
    public_key_b64: &str,
) -> bool {
    capability.version == 1
        && capability.issuer_key_id == expected_issuer_key_id
        && !capability.capability_id.is_empty()
        && !capability.org_id.is_empty()
        && !capability.device_id.0.is_empty()
        && !capability.fleet_origin.is_empty()
        && !capability.nonce.is_empty()
        && verify_signature_b64(
            public_key_b64,
            &fleet_lab_enrollment_capability_preimage(capability),
            &capability.signature,
        )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutonomousTestCapability {
    #[serde(default = "default_capability_version")]
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

fn default_capability_version() -> u16 {
    2
}

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

/// Device-signed, redacted measured state for one test-run member. `result`
/// is a closed schema validated by the test-enabled Fleet server; it is kept
/// as JSON here so this neutral protocol crate does not own Pro test policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutonomousTestObservation {
    #[serde(default = "default_observation_version")]
    pub version: u16,
    pub observation_id: String,
    pub device_id: DeviceId,
    pub capability: AutonomousTestCapability,
    pub result: Value,
    pub signature: String,
}

fn default_observation_version() -> u16 {
    1
}

/// Canonical bytes signed by the enrolled device outcome key. Including the
/// full capability prevents a caller from splicing a valid result onto a
/// different run, command, scenario, or expiry window.
pub fn autonomous_test_observation_preimage(observation: &AutonomousTestObservation) -> Vec<u8> {
    let value = serde_json::json!({
        "version": observation.version,
        "observation_id": observation.observation_id,
        "device_id": observation.device_id.0,
        "capability": observation.capability,
        "result": observation.result,
    });
    let mut canonical = String::new();
    write_canonical(&value, &mut canonical);
    canonical.into_bytes()
}

pub fn verify_autonomous_test_observation(
    observation: &AutonomousTestObservation,
    public_key_b64: &str,
) -> bool {
    observation.version == 1
        && !observation.observation_id.is_empty()
        && observation.device_id == observation.capability.device_id
        && verify_signature_b64(
            public_key_b64,
            &autonomous_test_observation_preimage(observation),
            &observation.signature,
        )
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn signed_capability() -> (FleetLabEnrollmentCapability, String) {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public_key = B64.encode(key.verifying_key().as_bytes());
        let mut capability = FleetLabEnrollmentCapability {
            version: 1,
            capability_id: "00000000-0000-4000-8000-000000000001".into(),
            org_id: "lab".into(),
            device_id: DeviceId("00000000-0000-4000-8000-000000000002".into()),
            fleet_origin: "http://127.0.0.1:8788".into(),
            not_before: "2026-01-01T00:00:00Z".into(),
            expires_at: "2026-01-01T00:05:00Z".into(),
            issuer_key_id: "fleet-lab-enrollment-1".into(),
            nonce: "00000000-0000-4000-8000-000000000003".into(),
            signature: String::new(),
        };
        capability.signature = B64.encode(
            key.sign(&fleet_lab_enrollment_capability_preimage(&capability))
                .to_bytes(),
        );
        (capability, public_key)
    }

    #[test]
    fn fleet_lab_enrollment_capability_verifies_only_its_exact_binding() {
        let (capability, public_key) = signed_capability();
        assert!(verify_fleet_lab_enrollment_capability(
            &capability,
            "fleet-lab-enrollment-1",
            &public_key
        ));
        let mut tampered = capability;
        tampered.fleet_origin = "https://other.example".into();
        assert!(!verify_fleet_lab_enrollment_capability(
            &tampered,
            "fleet-lab-enrollment-1",
            &public_key
        ));
    }
}
