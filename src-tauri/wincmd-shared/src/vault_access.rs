// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared, untrusted request and bounded response shapes for vault access.
//!
//! These types deliberately contain no Windows SID, ACL, credential, or
//! mounted-path fields.  The SYSTEM service resolves and persists those facts.

use serde::{Deserialize, Serialize};

pub const VAULT_ACCESS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultAccessPolicy {
    pub schema_version: u32,
    pub policy_id: String,
    pub version: u64,
    pub expected_previous_version: u64,
    pub entries: Vec<VaultAccessEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultAccessEntry {
    pub id: String,
    pub label: String,
    pub container_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_identity: Option<String>,
    #[serde(default)]
    pub container_kind: VaultContainerKind,
    pub owner_account: String,
    pub grants: Vec<VaultGrantInput>,
    pub mount: VaultMountPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultAccess {
    Read,
    Write,
}

/// `Dual` is an outer + hidden VeraCrypt container. The service, not the
/// renderer, decides which policy entries may use it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultContainerKind {
    #[default]
    Standard,
    Dual,
}

/// A one-request-only choice for a dual container. It is not policy state and
/// must never be persisted with a credential.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultVolumeRole {
    #[default]
    Outer,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultGrantInput {
    pub principal_name: String,
    pub access: VaultAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VaultPresentation {
    Machine,
    PerUser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultMountPolicy {
    pub presentation: VaultPresentation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_letter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultValidationState {
    NeverApplied,
    Current,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultEntryResult {
    Applied,
    PendingMountBroker,
    ValidationFailed,
    PrincipalResolutionFailed,
    ContainerIdentityFailed,
    AclApplyFailed,
    AclReadbackFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultEntryStatus {
    pub id: String,
    pub result: VaultEntryResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultPolicyStatus {
    pub policy_id: Option<String>,
    pub version: u64,
    pub validation_state: VaultValidationState,
    pub applied_at: Option<i64>,
    pub entries: Vec<VaultEntryStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultAuthorizeMountRequest {
    pub entry_id: String,
}

/// A one-shot unlock request.  `password` is intentionally the only secret on
/// this wire and is never persisted, echoed in a response, or accepted from a
/// command line.  The SYSTEM service resolves every other mount fact from its
/// registered policy and wipes its owned copy after forwarding it on stdin to
/// the fixed broker.
#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultMountRequest {
    pub entry_id: String,
    pub password: String,
    #[serde(default)]
    pub volume_role: VaultVolumeRole,
}

impl std::fmt::Debug for VaultMountRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultMountRequest")
            .field("entry_id", &self.entry_id)
            .field("volume_role", &self.volume_role)
            .field("password", &"[redacted]")
            .finish()
    }
}

/// A request to remove an existing mount.  Drive letters, paths, callers, and
/// broker handles are service-owned facts and must not be client input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultUnmountRequest {
    pub entry_id: String,
}

/// The intentionally small status vocabulary visible outside the SYSTEM
/// service.  It conveys whether a requested action completed without exposing
/// a container path, ACL, SID, session, or credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultMountState {
    Mounted,
    Unmounted,
    Denied,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultMountReason {
    NotAuthorized,
    InvalidRequest,
    BrokerUnavailable,
    BrokerRejected,
    SessionUnavailable,
    AclApplyFailed,
    AclReadbackFailed,
    DismountFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultMountResult {
    pub entry_id: String,
    pub state: VaultMountState,
    pub presentation: Option<VaultPresentation>,
    pub drive_letter: Option<String>,
    pub reason: Option<VaultMountReason>,
}

/// Ordinary-user projection for “My vaults”.  It is deliberately not a
/// policy view: a caller only receives entries its connected Windows token is
/// authorized for, with no paths, grants, SIDs, ACLs, or other entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultAuthorizedEntry {
    pub entry_id: String,
    pub label: String,
    pub access: VaultAccess,
    pub presentation: VaultPresentation,
    #[serde(default)]
    pub container_kind: VaultContainerKind,
    pub mount_state: VaultMountState,
    pub drive_letter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultAuthorizeMountResponse {
    pub allowed: bool,
    /// Eligibility is not mount permission.  This remains false until the
    /// service owns an atomic mount → ACL → read-back → present broker.
    pub launch_ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial_reason: Option<VaultMountDenial>,
    pub mode: Option<VaultAccess>,
    pub presentation: Option<VaultPresentation>,
    pub preferred_letter: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultMountDenial {
    NotAuthorized,
    MountBrokerUnavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_values_are_stable_and_do_not_include_sids() {
        let policy = VaultAccessPolicy {
            schema_version: VAULT_ACCESS_SCHEMA_VERSION,
            policy_id: "policy-a".into(),
            version: 1,
            expected_previous_version: 0,
            entries: vec![VaultAccessEntry {
                id: "shared".into(),
                label: "Shared".into(),
                container_path: "C:\\vault.hc".into(),
                container_identity: None,
                container_kind: VaultContainerKind::Standard,
                owner_account: "Administrator".into(),
                grants: vec![VaultGrantInput {
                    principal_name: "Partner".into(),
                    access: VaultAccess::Write,
                }],
                mount: VaultMountPolicy {
                    presentation: VaultPresentation::Machine,
                    preferred_letter: Some("V".into()),
                },
            }],
        };
        let json = serde_json::to_string(&policy).unwrap();
        assert!(json.contains("\"presentation\":\"machine\""));
        assert!(!json.contains("sid"));
        assert!(!json.contains("acl"));
    }

    #[test]
    fn mount_wire_contains_only_the_ephemeral_password_and_role() {
        let request = VaultMountRequest {
            entry_id: "shared".into(),
            password: "one-shot".into(),
            volume_role: VaultVolumeRole::Outer,
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 3);
        assert!(json.get("entry_id").is_some());
        assert!(json.get("password").is_some());
        assert_eq!(json.get("volume_role"), Some(&serde_json::Value::String("outer".into())));
        assert!(json.get("container_path").is_none());
        assert!(json.get("sid").is_none());

        let unmount = serde_json::to_value(VaultUnmountRequest {
            entry_id: "shared".into(),
        })
        .unwrap();
        assert_eq!(unmount.as_object().unwrap().len(), 1);
    }

    #[test]
    fn mount_request_debug_and_results_never_echo_a_password() {
        let request = VaultMountRequest {
            entry_id: "shared".into(),
            password: "canary-password".into(),
            volume_role: VaultVolumeRole::Hidden,
        };
        assert!(!format!("{request:?}").contains("canary-password"));
        let result = VaultMountResult {
            entry_id: "shared".into(),
            state: VaultMountState::Denied,
            presentation: None,
            drive_letter: None,
            reason: Some(VaultMountReason::NotAuthorized),
        };
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("canary-password"));
    }
}
