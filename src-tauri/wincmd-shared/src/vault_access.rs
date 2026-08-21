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
}
