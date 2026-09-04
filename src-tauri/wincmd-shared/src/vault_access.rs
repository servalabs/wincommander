// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared, untrusted request and bounded response shapes for vault access.
//!
//! These types deliberately contain no Windows SID, ACL, credential, or
//! mounted-path fields.  The SYSTEM service resolves and persists those facts.

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

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

/// A one-shot unlock request. Secrets on this wire are never persisted, echoed
/// in a response, or accepted from a command line. The SYSTEM service resolves
/// every other mount fact from its registered policy and wipes its owned copies
/// after forwarding them to the fixed broker.
#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultMountRequest {
    pub entry_id: String,
    pub password: String,
    #[serde(default)]
    pub volume_role: VaultVolumeRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_protection_password: Option<String>,
}

/// A personal container request is deliberately separate from the managed
/// access-policy wire.  The service derives the owner SID, session, mounted
/// root DACL and presentation; the caller may only name the backing file and
/// supply credentials/options for its own registered container.
#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalVaultMountRequest {
    pub container_path: String,
    pub password: String,
    pub volume_kind: VaultContainerKind,
    pub volume_role: VaultVolumeRole,
    #[serde(default)]
    pub preferred_letter: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub pim: Option<u32>,
    #[serde(default)]
    pub keyfiles: Vec<String>,
    #[serde(default)]
    pub hidden_protection_password: Option<String>,
    #[serde(default)]
    pub hidden_keyfiles: Vec<String>,
    #[serde(default)]
    pub hidden_pim: Option<u32>,
    #[serde(default)]
    pub removable: bool,
}

impl std::fmt::Debug for PersonalVaultMountRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PersonalVaultMountRequest")
            .field("volume_kind", &self.volume_kind)
            .field("volume_role", &self.volume_role)
            .field("preferred_letter", &self.preferred_letter)
            .field("read_only", &self.read_only)
            .field("container_path", &"[redacted]")
            .field("credentials", &"[redacted]")
            .field("hidden_protection_password", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl PersonalVaultMountRequest {
    pub fn zeroize_secrets(&mut self) {
        self.password.zeroize();
        if let Some(password) = &mut self.hidden_protection_password {
            password.zeroize();
        }
        self.hidden_protection_password = None;
        self.keyfiles.iter_mut().for_each(Zeroize::zeroize);
        self.keyfiles.clear();
        self.hidden_keyfiles.iter_mut().for_each(Zeroize::zeroize);
        self.hidden_keyfiles.clear();
    }
}

/// Service-produced, authenticated service-to-Pro mount plan. Renderer input
/// is never deserialized into this type. The service derives the path, ACL,
/// presentation, session and operation id, validates the complete plan once,
/// then sends it over the signed one-shot broker pipe.
#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultMountPlan {
    pub operation_id: u64,
    pub container_path: String,
    pub password: String,
    pub mounted_root_acl_sddl: String,
    pub mount_mode: VaultMountMode,
    pub presentation: VaultPresentation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_letter: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    /// Service-derived only. Personal mounts may be presented without a
    /// filesystem-root ACL when the encrypted filesystem cannot store one.
    /// Absence defaults to `false` so older plans remain fail-closed.
    #[serde(default)]
    pub personal: bool,
    pub volume_kind: VaultContainerKind,
    pub volume_role: VaultBrokerVolumeRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_protection_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pim: Option<u32>,
    #[serde(default)]
    pub keyfiles: Vec<String>,
    #[serde(default)]
    pub hidden_keyfiles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_pim: Option<u32>,
    #[serde(default)]
    pub removable: bool,
    pub target_session_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultMountMode {
    Standard,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultBrokerVolumeRole {
    Standard,
    Outer,
    Hidden,
}

impl VaultMountPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.operation_id == 0
            || self.container_path.is_empty()
            || self.container_path.len() > 32_767
            || !is_windows_absolute_path(&self.container_path)
            || self.password.is_empty()
            || self.password.len() > 32_767
            || self.mounted_root_acl_sddl.is_empty()
            || self.mounted_root_acl_sddl.len() > 4_096
            || !self.mounted_root_acl_sddl.starts_with("D:")
            || self.mounted_root_acl_sddl.contains(['\r', '\n'])
            || self.target_session_id == 0
        {
            return Err("vault_mount_plan_invalid");
        }
        if self.preferred_letter.as_deref().is_some_and(|letter| {
            !matches!(letter.as_bytes(), [value] | [value, b':'] if value.is_ascii_alphabetic())
        }) {
            return Err("vault_mount_plan_invalid");
        }
        if self.keyfiles.len() > 32
            || self.hidden_keyfiles.len() > 32
            || self
                .keyfiles
                .iter()
                .chain(self.hidden_keyfiles.iter())
                .any(|path| path.is_empty() || path.len() > 32_767)
        {
            return Err("vault_mount_plan_invalid");
        }
        let expected_mode = match (self.volume_kind, self.volume_role) {
            (VaultContainerKind::Standard, VaultBrokerVolumeRole::Standard)
            | (VaultContainerKind::Dual, VaultBrokerVolumeRole::Outer) => VaultMountMode::Standard,
            (VaultContainerKind::Dual, VaultBrokerVolumeRole::Hidden) => VaultMountMode::Hidden,
            _ => return Err("vault_mount_plan_invalid"),
        };
        if self.mount_mode != expected_mode {
            return Err("vault_mount_plan_invalid");
        }
        let hidden_protection_allowed = self.volume_kind == VaultContainerKind::Dual
            && self.volume_role == VaultBrokerVolumeRole::Outer
            && !self.read_only;
        let has_hidden_protection = self.hidden_protection_password.is_some()
            || !self.hidden_keyfiles.is_empty()
            || self.hidden_pim.is_some();
        if has_hidden_protection != hidden_protection_allowed {
            return Err("vault_mount_plan_invalid");
        }
        Ok(())
    }

    pub fn zeroize_secrets(&mut self) {
        self.password.zeroize();
        if let Some(password) = &mut self.hidden_protection_password {
            password.zeroize();
        }
        self.hidden_protection_password = None;
        self.keyfiles.iter_mut().for_each(Zeroize::zeroize);
        self.keyfiles.clear();
        self.hidden_keyfiles.iter_mut().for_each(Zeroize::zeroize);
        self.hidden_keyfiles.clear();
    }
}

fn is_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    let is_drive_rooted = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let is_unc = value.strip_prefix("\\\\").is_some_and(|rest| {
        let mut components = rest.split('\\');
        components.next().is_some_and(|server| !server.is_empty())
            && components.next().is_some_and(|share| !share.is_empty())
    });
    is_drive_rooted || is_unc
}

impl std::fmt::Debug for VaultMountPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultMountPlan")
            .field("operation_id", &self.operation_id)
            .field("mount_mode", &self.mount_mode)
            .field("presentation", &self.presentation)
            .field("volume_kind", &self.volume_kind)
            .field("volume_role", &self.volume_role)
            .field("read_only", &self.read_only)
            .field("target_session_id", &self.target_session_id)
            .field("container_path", &"[redacted]")
            .field("credentials", &"[redacted]")
            .field("mounted_root_acl_sddl", &"[redacted]")
            .finish()
    }
}

/// Persisted only by the SYSTEM service after it has applied and read back the
/// container DACL.  This is not a renderer authority claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalVaultRecord {
    pub container_path: String,
    pub container_identity: String,
    pub owner_sid: String,
    pub scope: VaultPresentation,
    pub created_by_session: u32,
}

impl std::fmt::Debug for VaultMountRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultMountRequest")
            .field("entry_id", &self.entry_id)
            .field("volume_role", &self.volume_role)
            .field("password", &"[redacted]")
            .field("hidden_protection_password", &"[redacted]")
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
    EngineUnlockFailed,
    EngineDriveLetterUnavailable,
    EngineMountFailed,
    AclApplyFailed,
    AclReadbackFailed,
    DismountFailed,
}

impl VaultMountReason {
    pub const ALL: [Self; 11] = [
        Self::NotAuthorized,
        Self::InvalidRequest,
        Self::BrokerUnavailable,
        Self::BrokerRejected,
        Self::SessionUnavailable,
        Self::EngineUnlockFailed,
        Self::EngineDriveLetterUnavailable,
        Self::EngineMountFailed,
        Self::AclApplyFailed,
        Self::AclReadbackFailed,
        Self::DismountFailed,
    ];

    pub const ALL_WIRE_VALUES: [&'static str; 11] = [
        "not_authorized",
        "invalid_request",
        "broker_unavailable",
        "broker_rejected",
        "session_unavailable",
        "engine_unlock_failed",
        "engine_drive_letter_unavailable",
        "engine_mount_failed",
        "acl_apply_failed",
        "acl_readback_failed",
        "dismount_failed",
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotAuthorized => "not_authorized",
            Self::InvalidRequest => "invalid_request",
            Self::BrokerUnavailable => "broker_unavailable",
            Self::BrokerRejected => "broker_rejected",
            Self::SessionUnavailable => "session_unavailable",
            Self::EngineUnlockFailed => "engine_unlock_failed",
            Self::EngineDriveLetterUnavailable => "engine_drive_letter_unavailable",
            Self::EngineMountFailed => "engine_mount_failed",
            Self::AclApplyFailed => "acl_apply_failed",
            Self::AclReadbackFailed => "acl_readback_failed",
            Self::DismountFailed => "dismount_failed",
        }
    }

    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "not_authorized" => Some(Self::NotAuthorized),
            "invalid_request" => Some(Self::InvalidRequest),
            "broker_unavailable" => Some(Self::BrokerUnavailable),
            "broker_rejected" => Some(Self::BrokerRejected),
            "session_unavailable" => Some(Self::SessionUnavailable),
            "engine_unlock_failed" => Some(Self::EngineUnlockFailed),
            "engine_drive_letter_unavailable" => Some(Self::EngineDriveLetterUnavailable),
            "engine_mount_failed" => Some(Self::EngineMountFailed),
            "acl_apply_failed" => Some(Self::AclApplyFailed),
            "acl_readback_failed" => Some(Self::AclReadbackFailed),
            "dismount_failed" => Some(Self::DismountFailed),
            _ => None,
        }
    }
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

/// One Windows local group to reconcile via
/// `svc.vault.reconcile_access_groups`. `local_group` is an admin-chosen
/// Windows local-group name (e.g. `WC_Sales`) from the Access control UI —
/// never one of the deterministic `WC-Vault-*` names the policy-apply path
/// derives for its own per-entry groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultAccessGroupInput {
    pub local_group: String,
    pub member_sids: Vec<String>,
}

/// `svc.vault.reconcile_access_groups` request. Frozen wire shape:
/// `{ "groups": [ { "local_group": "...", "member_sids": ["..."] } ] }`.
/// Privileged (SYSTEM/Admin only, like `svc.vault.apply_policy`) — the
/// service mutates real Windows local-group state on behalf of this call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultReconcileAccessGroupsRequest {
    pub groups: Vec<VaultAccessGroupInput>,
}

/// Per-group outcome of a `svc.vault.reconcile_access_groups` call. A single
/// group's failure is reported here and must never abort the rest of the
/// batch — see that verb's handler in `commander-svc::pipe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultAccessGroupState {
    Created,
    Updated,
    Unchanged,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultAccessGroupResult {
    pub local_group: String,
    pub state: VaultAccessGroupState,
    pub error: Option<String>,
}

/// `svc.vault.reconcile_access_groups` response. Frozen wire shape:
/// `{ "results": [ { "local_group": "...", "state": "created" | "updated" |
/// "unchanged" | "failed", "error": null | "<short reason>" } ] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultReconcileAccessGroupsResponse {
    pub results: Vec<VaultAccessGroupResult>,
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
    fn mount_wire_contains_only_ephemeral_secrets_and_role() {
        let request = VaultMountRequest {
            entry_id: "shared".into(),
            password: "one-shot".into(),
            volume_role: VaultVolumeRole::Outer,
            hidden_protection_password: Some("hidden-one-shot".into()),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 4);
        assert!(json.get("entry_id").is_some());
        assert!(json.get("password").is_some());
        assert_eq!(
            json.get("volume_role"),
            Some(&serde_json::Value::String("outer".into()))
        );
        assert!(json.get("hidden_protection_password").is_some());
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
            hidden_protection_password: Some("hidden-canary-password".into()),
        };
        assert!(!format!("{request:?}").contains("canary-password"));
        assert!(!format!("{request:?}").contains("hidden-canary-password"));
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

    fn sample_mount_plan() -> VaultMountPlan {
        VaultMountPlan {
            operation_id: 41,
            container_path: r"C:\Vaults\private.hc".into(),
            password: "canary-password".into(),
            mounted_root_acl_sddl: "D:P(A;;FA;;;SY)(A;;FA;;;BA)".into(),
            mount_mode: VaultMountMode::Standard,
            presentation: VaultPresentation::PerUser,
            preferred_letter: Some("V:".into()),
            read_only: false,
            personal: false,
            volume_kind: VaultContainerKind::Dual,
            volume_role: VaultBrokerVolumeRole::Outer,
            hidden_protection_password: Some("hidden-canary-password".into()),
            pim: Some(1),
            keyfiles: vec![r"C:\Keys\canary.key".into()],
            hidden_keyfiles: vec![r"C:\Keys\hidden-canary.key".into()],
            hidden_pim: Some(2),
            removable: false,
            target_session_id: 7,
        }
    }

    #[test]
    fn mount_plan_has_a_stable_strict_wire_and_validates_combinations() {
        let plan = sample_mount_plan();
        assert_eq!(plan.validate(), Ok(()));
        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "operation_id": 41,
                "container_path": r"C:\Vaults\private.hc",
                "password": "canary-password",
                "mounted_root_acl_sddl": "D:P(A;;FA;;;SY)(A;;FA;;;BA)",
                "mount_mode": "standard",
                "presentation": "per-user",
                "preferred_letter": "V:",
                "read_only": false,
                "personal": false,
                "volume_kind": "dual",
                "volume_role": "outer",
                "hidden_protection_password": "hidden-canary-password",
                "pim": 1,
                "keyfiles": [r"C:\Keys\canary.key"],
                "hidden_keyfiles": [r"C:\Keys\hidden-canary.key"],
                "hidden_pim": 2,
                "removable": false,
                "target_session_id": 7,
            })
        );

        let mut unknown = json.clone();
        unknown["renderer_override"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<VaultMountPlan>(unknown).is_err());

        let mut invalid = sample_mount_plan();
        invalid.mount_mode = VaultMountMode::Hidden;
        assert_eq!(invalid.validate(), Err("vault_mount_plan_invalid"));
    }

    #[test]
    fn mount_plan_windows_path_validation_is_host_independent() {
        let mut plan = sample_mount_plan();
        for path in [
            r"C:\Vaults\private.hc",
            r"C:/Vaults/private.hc",
            r"\\server\vaults\private.hc",
        ] {
            plan.container_path = path.into();
            assert_eq!(
                plan.validate(),
                Ok(()),
                "expected Windows absolute path: {path}"
            );
        }
        for path in [
            r"Vaults\private.hc",
            r"C:Vaults\private.hc",
            r"\Vaults\private.hc",
            r"\\server",
            r"\\",
        ] {
            plan.container_path = path.into();
            assert_eq!(
                plan.validate(),
                Err("vault_mount_plan_invalid"),
                "expected non-absolute Windows path: {path}"
            );
        }
    }

    #[test]
    fn request_result_and_error_golden_fixtures_keep_the_operation_id_bounded() {
        let request = VaultMountRequest {
            entry_id: "sales".into(),
            password: "one-shot".into(),
            volume_role: VaultVolumeRole::Outer,
            hidden_protection_password: None,
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "entry_id": "sales", "password": "one-shot", "volume_role": "outer"
            })
        );
        let result = VaultMountResult {
            entry_id: "sales".into(),
            state: VaultMountState::Failed,
            presentation: Some(VaultPresentation::Machine),
            drive_letter: None,
            reason: Some(VaultMountReason::EngineDriveLetterUnavailable),
        };
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::json!({
                "entry_id": "sales", "state": "failed", "presentation": "machine",
                "drive_letter": null, "reason": "engine_drive_letter_unavailable"
            })
        );
        let error = crate::Envelope::Error(crate::ErrorReply {
            request_id: 41,
            kind: "unknown_verb".into(),
            message: "service verb is not recognized".into(),
        });
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "error", "request_id": 41, "error_kind": "unknown_verb",
                "message": "service verb is not recognized"
            })
        );
    }

    #[test]
    fn mount_plan_and_personal_request_redact_and_zeroize_all_credentials() {
        let mut plan = sample_mount_plan();
        let debug = format!("{plan:?}");
        for secret in [
            "private.hc",
            "canary-password",
            "hidden-canary-password",
            "canary.key",
            "hidden-canary.key",
            "A;;FA",
        ] {
            assert!(!debug.contains(secret));
        }
        plan.zeroize_secrets();
        assert!(plan.password.chars().all(|value| value == '\0'));
        assert!(plan.hidden_protection_password.is_none());
        assert!(plan.keyfiles.is_empty());
        assert!(plan.hidden_keyfiles.is_empty());

        let mut personal = PersonalVaultMountRequest {
            container_path: r"C:\Vaults\private.hc".into(),
            password: "canary-password".into(),
            volume_kind: VaultContainerKind::Dual,
            volume_role: VaultVolumeRole::Outer,
            preferred_letter: Some("V".into()),
            read_only: false,
            pim: Some(1),
            keyfiles: vec![r"C:\Keys\canary.key".into()],
            hidden_protection_password: Some("hidden-canary-password".into()),
            hidden_keyfiles: vec![r"C:\Keys\hidden-canary.key".into()],
            hidden_pim: Some(2),
            removable: false,
        };
        let debug = format!("{personal:?}");
        assert!(!debug.contains("private.hc"));
        assert!(!debug.contains("canary.key"));
        assert!(!debug.contains("canary-password"));
        personal.zeroize_secrets();
        assert!(personal.password.chars().all(|value| value == '\0'));
        assert!(personal.hidden_protection_password.is_none());
        assert!(personal.keyfiles.is_empty());
        assert!(personal.hidden_keyfiles.is_empty());
    }

    #[test]
    fn renderer_and_rust_mount_reason_vocabularies_are_in_parity() {
        let renderer = include_str!("../../../src/panels/fleet/vaultAccessTypes.ts");
        for (reason, wire_value) in VaultMountReason::ALL
            .into_iter()
            .zip(VaultMountReason::ALL_WIRE_VALUES)
        {
            assert_eq!(reason.as_str(), wire_value);
            assert_eq!(VaultMountReason::from_wire(wire_value), Some(reason));
            assert_eq!(
                serde_json::from_str::<VaultMountReason>(
                    &serde_json::to_string(&reason).expect("serialize reason")
                )
                .expect("deserialize reason"),
                reason,
            );
            assert!(
                renderer.contains(&format!("\"{wire_value}\"")),
                "renderer is missing Rust Vault reason {wire_value}",
            );
        }
        assert_eq!(
            VaultMountReason::from_wire("vault_engine_mount_failed"),
            None
        );
        let declaration = renderer
            .split("export const VAULT_MOUNT_REASONS = [")
            .nth(1)
            .and_then(|tail| tail.split("] as const;").next())
            .expect("renderer reason declaration");
        assert_eq!(
            declaration.matches('"').count() / 2,
            VaultMountReason::ALL_WIRE_VALUES.len(),
            "renderer must not add a reason without the Rust wire enum",
        );
    }

    #[test]
    fn reconcile_access_groups_request_matches_the_frozen_wire_shape() {
        let request = VaultReconcileAccessGroupsRequest {
            groups: vec![VaultAccessGroupInput {
                local_group: "WC_Sales".into(),
                member_sids: vec!["S-1-5-21-1".into(), "S-1-5-21-2".into()],
            }],
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "groups": [
                    { "local_group": "WC_Sales", "member_sids": ["S-1-5-21-1", "S-1-5-21-2"] }
                ]
            })
        );
        // Round-trips through the exact shape the bridge (commander-free)
        // sends untouched from the renderer.
        let round_tripped: VaultReconcileAccessGroupsRequest =
            serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, request);
    }

    #[test]
    fn reconcile_access_groups_response_matches_the_frozen_wire_shape() {
        let response = VaultReconcileAccessGroupsResponse {
            results: vec![
                VaultAccessGroupResult {
                    local_group: "WC_Sales".into(),
                    state: VaultAccessGroupState::Created,
                    error: None,
                },
                VaultAccessGroupResult {
                    local_group: "WC_Bad".into(),
                    state: VaultAccessGroupState::Failed,
                    error: Some("invalid local group name".into()),
                },
            ],
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "results": [
                    { "local_group": "WC_Sales", "state": "created", "error": null },
                    { "local_group": "WC_Bad", "state": "failed", "error": "invalid local group name" }
                ]
            })
        );
    }
}
