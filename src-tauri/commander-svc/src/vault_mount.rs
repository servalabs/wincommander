// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-owned, fail-closed Vault mount broker boundary.
//!
//! This module never launches a public Pro CLI.  A path/password/ACL-bearing
//! mount request is accepted only by an authenticated service-to-Pro broker;
//! until that protected transport is installed, mount fails closed.

#![cfg(windows)]

use std::collections::{HashMap, HashSet};

#[cfg(test)]
pub(crate) use tests::policy_edit_test_broker;
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::vault_access::{ResolvedGrant, VaultAccessStore};
use wincmd_shared::vault_access::{
    PersonalVaultMountRequest, PersonalVaultMountedVolume, PersonalVaultRecord,
    VaultBrokerVolumeRole, VaultContainerKind, VaultMountMode, VaultMountPlan, VaultMountReason,
    VaultMountResult, VaultMountState, VaultPresentation, VaultVolumeRole,
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveMount {
    drive_letter: String,
    internal_drive: u8,
    presentation: VaultPresentation,
    session_id: u32,
    caller_sid: String,
    policy_id: String,
    policy_version: u64,
    #[serde(default)]
    personal: bool,
    /// Set only after the owner-session adapter confirms that this exact
    /// owner-only per-user Vault has an enrolled Syncthing binding. It is
    /// intentionally absent for machine and shared Vaults, where multiple
    /// Windows profiles could otherwise control the same Syncthing folder.
    #[serde(default)]
    syncthing_managed: bool,
    container_identity: String,
    access: wincmd_shared::vault_access::VaultAccess,
    mounted_at: u64,
    #[serde(default)]
    cleanup_required: bool,
    #[serde(default)]
    engine_mount_identity: Option<String>,
    #[serde(default)]
    canonical_container_path: Option<String>,
}

/// Protected mount authority and recovery state. It contains no password, ACL
/// or token. Any backing path is service-derived and projected only after access checks.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableMountRegistry {
    mounts: HashMap<String, ActiveMount>,
}

const MAX_DURABLE_MOUNTS: usize = 64;
// A shared mount remains device-writable so its attested root DACL can
// distinguish the owner/editor from a view-only user.
const SHARED_VAULT_DEVICE_READ_ONLY: bool = false;
static NEXT_INTERNAL_OPERATION_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1 << 62);

fn next_internal_operation_id() -> u64 {
    NEXT_INTERNAL_OPERATION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Service-to-Pro only.  No implementation may be backed by an unauthenticated
/// executable argument/stdio mode: a renderer must never be able to select the
/// container, SDDL, session, internal slot, or broker endpoint.
struct BrokerDismountRequest<'a> {
    operation_id: u64,
    internal_drive: u8,
    presented_drive_letter: Option<&'a str>,
    presentation: VaultPresentation,
    target_session_id: u32,
    caller_sid: &'a str,
    caller_token: Option<windows_sys::Win32::Foundation::HANDLE>,
}

pub(crate) struct AuthorizedDismount<'a> {
    pub operation_id: u64,
    pub entry_id: &'a str,
    pub caller_token: windows_sys::Win32::Foundation::HANDLE,
    pub caller_session: u32,
    pub caller_sid: &'a str,
    pub caller_elevated: bool,
}

trait AuthenticatedVaultBroker: Send + Sync {
    fn observed_slots(&self) -> Result<HashMap<u8, String>, String> {
        #[cfg(not(test))]
        {
            wincmd_volume::mounted_slot_identities()
        }
        #[cfg(test)]
        {
            Ok(HashMap::from([(12, "test-mount:12".into())]))
        }
    }
    fn mount(
        &self,
        request: &mut InternalMountRequest,
    ) -> Result<InternalMountReply, VaultMountReason>;
    fn dismount(&self, request: BrokerDismountRequest<'_>) -> Result<(), VaultMountReason>;
    fn cleanup_orphans(&self) -> Result<(), VaultMountReason>;
    /// Boot recovery runs after an interactive session may have ended. The
    /// encrypted driver's internal slot is machine-owned, so cleanup must not
    /// require the original user's now-unavailable logon token.
    fn recover_dismount(&self, internal_drive: u8) -> Result<(), VaultMountReason>;
}

type CallerMountAttestor =
    fn(windows_sys::Win32::Foundation::HANDLE, &str, u8, bool) -> CallerPresentationAttestation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallerPresentationAttestation {
    Available,
    MappingUnavailable,
    RootAccessDenied,
    RootWriteAccessDenied,
    RootReadFailed,
}

struct ProEnvelopeBroker;

/// Calls the private, per-user adapter through the existing service-created
/// Pro broker. The service owns target identity and never sees the API key or
/// a caller-selected Syncthing endpoint.
fn syncthing_lifecycle_call(
    feature_id: &'static str,
    operation_id: u64,
    entry_id: &str,
    mount: &ActiveMount,
    caller_token: Option<windows_sys::Win32::Foundation::HANDLE>,
) -> Result<bool, VaultMountReason> {
    #[cfg(test)]
    {
        let _ = (feature_id, operation_id, entry_id, mount, caller_token);
        return Ok(false);
    }
    #[cfg(not(test))]
    {
    if mount.presentation != VaultPresentation::PerUser {
        return Ok(false);
    }
    let value = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
            crate::pro_broker::VaultCall {
                request_id: operation_id,
                target_session_id: mount.session_id,
                caller_sid: &mount.caller_sid,
                caller_token,
                caller_authentication_id: None,
                presentation: mount.presentation,
                feature_id,
                args: serde_json::json!({
                    "operation_id": operation_id,
                    "vault_entry_id": entry_id,
                    "owner_sid": mount.caller_sid,
                    "volume_identity": mount.container_identity,
                    "drive_letter": mount.drive_letter,
                    "target_session_id": mount.session_id,
                }),
            },
        ))
    })?;
    Ok(value.get("managed").and_then(serde_json::Value::as_bool) == Some(true))
    }
}

fn syncthing_enroll_call(
    operation_id: u64,
    entry_id: &str,
    mount: &ActiveMount,
    caller_token: windows_sys::Win32::Foundation::HANDLE,
    relative_path: &str,
) -> Result<String, VaultMountReason> {
    let mut hasher = Sha256::new();
    hasher.update(entry_id.as_bytes());
    let folder_id = format!(
        "wcv-{}",
        hasher
            .finalize()[..12]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let value = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
            crate::pro_broker::VaultCall {
                request_id: operation_id,
                target_session_id: mount.session_id,
                caller_sid: &mount.caller_sid,
                caller_token: Some(caller_token),
                caller_authentication_id: None,
                presentation: mount.presentation,
                feature_id: "vault.syncthing.enroll",
                args: serde_json::json!({
                    "operation_id": operation_id,
                    "vault_entry_id": entry_id,
                    "owner_sid": mount.caller_sid,
                    "volume_identity": mount.container_identity,
                    "drive_letter": mount.drive_letter,
                    "target_session_id": mount.session_id,
                    "folder_id": folder_id,
                    "relative_path": relative_path,
                }),
            },
        ))
    })?;
    if value.get("managed").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(VaultMountReason::BrokerRejected);
    }
    value
        .get("gui_url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| valid_syncthing_gui_url(url))
        .map(str::to_owned)
        .ok_or(VaultMountReason::BrokerRejected)
}

impl AuthenticatedVaultBroker for ProEnvelopeBroker {
    fn mount(
        &self,
        request: &mut InternalMountRequest,
    ) -> Result<InternalMountReply, VaultMountReason> {
        let args = broker_mount_args(request)?;
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
                crate::pro_broker::VaultCall {
                    request_id: request.operation_id,
                    target_session_id: request.target_session_id,
                    caller_sid: &request.caller_sid,
                    caller_token: Some(request.caller_token),
                    caller_authentication_id: Some(request.caller_authentication_id),
                    presentation: request.presentation,
                    feature_id: "vault.broker.mount",
                    args,
                },
            ))
        });
        request.zeroize_secrets();
        let value = result?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            drive_letter: String,
            internal_drive: u8,
            acl_attested: bool,
        }
        let reply: Reply =
            serde_json::from_value(value).map_err(|_| VaultMountReason::BrokerRejected)?;
        Ok(InternalMountReply {
            drive_letter: reply.drive_letter,
            internal_drive: reply.internal_drive,
            acl_attested: reply.acl_attested,
        })
    }
    fn dismount(&self, request: BrokerDismountRequest<'_>) -> Result<(), VaultMountReason> {
        let result = tokio::task::block_in_place(|| {
            let _volume_operation =
                wincmd_volume::VolumeOperationGuard::acquire_slot(request.internal_drive)
                    .map_err(|_| VaultMountReason::DismountFailed)?;
            tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
                crate::pro_broker::VaultCall {
                    request_id: request.operation_id,
                    target_session_id: request.target_session_id,
                    caller_sid: request.caller_sid,
                    caller_token: request.caller_token,
                    caller_authentication_id: None,
                    presentation: request.presentation,
                    feature_id: "vault.broker.dismount",
                    args: broker_dismount_args(
                        request.internal_drive,
                        request.presented_drive_letter,
                    ),
                },
            ))
        });
        result
            .map(|_| ())
            .map_err(|_| VaultMountReason::DismountFailed)
    }
    fn cleanup_orphans(&self) -> Result<(), VaultMountReason> {
        Ok(())
    }
    fn recover_dismount(&self, internal_drive: u8) -> Result<(), VaultMountReason> {
        let result = tokio::task::block_in_place(|| {
            let _volume_operation =
                wincmd_volume::VolumeOperationGuard::acquire_slot(internal_drive)
                    .map_err(|_| VaultMountReason::DismountFailed)?;
            tokio::runtime::Handle::current()
                .block_on(crate::pro_broker::vault_recovery_dismount(internal_drive))
        });
        result
            .map(|_| ())
            .map_err(|_| VaultMountReason::DismountFailed)
    }
}

fn broker_dismount_args(
    internal_drive: u8,
    presented_drive_letter: Option<&str>,
) -> serde_json::Value {
    let mut args = serde_json::Map::new();
    args.insert(
        "internal_drive".into(),
        serde_json::Value::from(internal_drive),
    );
    if let Some(letter) = presented_drive_letter.filter(|letter| valid_drive_letter(letter)) {
        let mut letter = letter.to_ascii_uppercase();
        if !letter.ends_with(':') {
            letter.push(':');
        }
        args.insert(
            "presented_drive_letter".into(),
            serde_json::Value::String(letter),
        );
    }
    serde_json::Value::Object(args)
}

fn per_user_presented_drive_letter(
    presentation: VaultPresentation,
    drive_letter: &str,
) -> Option<&str> {
    (presentation == VaultPresentation::PerUser).then_some(drive_letter)
}

fn valid_relative_sync_path(value: &str) -> bool {
    let path = std::path::Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn valid_syncthing_gui_url(value: &str) -> bool {
    value
        .strip_prefix("http://127.0.0.1:")
        .and_then(|port| port.parse::<u16>().ok())
        .is_some_and(|port| port != 0)
}

/// Private input to the authenticated broker.  It is deliberately not serde
/// serializable: the public named-pipe and Tauri wire must never reuse it.
struct InternalMountRequest {
    operation_id: u64,
    container_path: String,
    mount_mode: &'static str,
    volume_kind: &'static str,
    volume_role: &'static str,
    read_only: bool,
    /// Service-derived only; this private request cannot be supplied by UI or
    /// named-pipe callers.
    personal: bool,
    pim: Option<u32>,
    keyfiles: Vec<String>,
    hidden_keyfiles: Vec<String>,
    hidden_pim: Option<u32>,
    removable: bool,
    presentation: VaultPresentation,
    preferred_letter: Option<String>,
    target_session_id: u32,
    caller_sid: String,
    caller_token: windows_sys::Win32::Foundation::HANDLE,
    caller_authentication_id: (u32, i32),
    mounted_root_acl_sddl: MountedRootAclSddl,
    password: String,
    hidden_protection_password: Option<String>,
}

fn broker_mount_args(
    request: &mut InternalMountRequest,
) -> Result<serde_json::Value, VaultMountReason> {
    let mut plan = VaultMountPlan {
        operation_id: request.operation_id,
        container_path: request.container_path.clone(),
        password: std::mem::take(&mut request.password),
        mounted_root_acl_sddl: request.mounted_root_acl_sddl.0.clone(),
        mount_mode: match request.mount_mode {
            "standard" => VaultMountMode::Standard,
            "hidden" => VaultMountMode::Hidden,
            _ => return Err(VaultMountReason::InvalidRequest),
        },
        presentation: request.presentation,
        preferred_letter: request.preferred_letter.clone(),
        read_only: request.read_only,
        personal: request.personal,
        personal_acl_repair_sid: None,
        volume_kind: match request.volume_kind {
            "standard" => VaultContainerKind::Standard,
            "dual" => VaultContainerKind::Dual,
            _ => return Err(VaultMountReason::InvalidRequest),
        },
        volume_role: match request.volume_role {
            "standard" => VaultBrokerVolumeRole::Standard,
            "outer" => VaultBrokerVolumeRole::Outer,
            "hidden" => VaultBrokerVolumeRole::Hidden,
            _ => return Err(VaultMountReason::InvalidRequest),
        },
        hidden_protection_password: std::mem::take(&mut request.hidden_protection_password),
        pim: request.pim,
        keyfiles: std::mem::take(&mut request.keyfiles),
        hidden_keyfiles: std::mem::take(&mut request.hidden_keyfiles),
        hidden_pim: request.hidden_pim,
        removable: request.removable,
        target_session_id: request.target_session_id,
    };
    if plan.validate().is_err() {
        plan.zeroize_secrets();
        return Err(VaultMountReason::InvalidRequest);
    }
    let result = serde_json::to_value(&plan).map_err(|_| VaultMountReason::InvalidRequest);
    plan.zeroize_secrets();
    result
}

impl InternalMountRequest {
    fn zeroize_secrets(&mut self) {
        self.password.zeroize();
        if let Some(hidden_protection_password) = &mut self.hidden_protection_password {
            hidden_protection_password.zeroize();
        }
        self.hidden_protection_password = None;
        self.keyfiles.iter_mut().for_each(Zeroize::zeroize);
        self.keyfiles.clear();
        self.hidden_keyfiles.iter_mut().for_each(Zeroize::zeroize);
        self.hidden_keyfiles.clear();
    }
}

/// The broker attests that it applied and exactly read back the service-provided
/// SDDL while presentation was still closed.  It returns no path/SID/ACL.
struct InternalMountReply {
    drive_letter: String,
    internal_drive: u8,
    acl_attested: bool,
}

/// Internal-only service-to-Pro SDDL; no serde implementation by design.
struct MountedRootAclSddl(String);

pub struct VaultMountBroker {
    active: Mutex<HashMap<String, ActiveMount>>,
    // One policy generation must not interleave with an authorization/mount or
    // unmount. Without this gate a caller could pass authorization against the
    // old policy in the gap between cleanup and the new-policy install.
    operation: Mutex<()>,
    broker: Box<dyn AuthenticatedVaultBroker>,
    caller_mount_attestor: CallerMountAttestor,
    drive_letter_probe: fn() -> Result<HashSet<String>, ()>,
    engine_snapshot: Option<fn() -> Result<HashMap<u8, String>, String>>,
    policy_authorizer: fn(
        &VaultAccessStore,
        &str,
        windows_sys::Win32::Foundation::HANDLE,
    ) -> wincmd_shared::vault_access::VaultAuthorizeMountResponse,
    recovery: Mutex<RecoveryState>,
}

/// A damaged/ambiguous boot registry is machine-wide unknown state and must
/// fail closed. A normal persistence failure after a known dismount is scoped
/// to that entry and automatically clears after the next successful write.
#[derive(Default)]
struct RecoveryState {
    registry_untrusted: bool,
    persistence_pending: HashSet<String>,
    removal_pending: HashSet<String>,
}

impl VaultMountBroker {
    pub(crate) fn personal_mount_failure_code(reason: VaultMountReason) -> &'static str {
        match reason {
            VaultMountReason::NotAuthorized => "vault_not_authorized",
            VaultMountReason::AdministratorRequired => "vault_administrator_required",
            VaultMountReason::PolicyAccessDenied => "vault_policy_access_denied",
            VaultMountReason::PrivateOwnerRequired => "vault_private_owner_required",
            VaultMountReason::MountStateUnknown => "vault_mount_state_unknown",
            VaultMountReason::AlreadyMounted => "vault_already_mounted",
            VaultMountReason::SessionUnavailable => "vault_session_unavailable",
            VaultMountReason::EngineUnlockFailed => "vault_engine_unlock_failed",
            VaultMountReason::EngineDriveLetterUnavailable => {
                "vault_engine_drive_letter_unavailable"
            }
            VaultMountReason::EngineMountFailed => "vault_engine_mount_failed",
            VaultMountReason::AclApplyFailed => "vault_acl_apply_failed",
            VaultMountReason::AclReadbackFailed => "vault_acl_readback_failed",
            VaultMountReason::CallerAccessDenied => "vault_caller_access_denied",
            VaultMountReason::CallerAclRepairFailed => "vault_caller_acl_repair_failed",
            VaultMountReason::InvalidRequest => "vault_validation_failed",
            VaultMountReason::BrokerUnavailable => "vault_broker_unavailable",
            VaultMountReason::ProNotInstalled => "vault_pro_not_installed",
            VaultMountReason::BrokerRejected => "vault_broker_rejected",
            VaultMountReason::BrokerIdentityRejected => "vault_broker_identity_rejected",
            VaultMountReason::BrokerHandshakeRejected => "vault_broker_handshake_rejected",
            VaultMountReason::BrokerReplyRejected => "vault_broker_reply_rejected",
            VaultMountReason::BrokerPlanRejected => "vault_broker_plan_rejected",
            VaultMountReason::PresentationRejected => "vault_presentation_rejected",
            VaultMountReason::EntitlementDenied => "vault_entitlement_denied",
            VaultMountReason::DismountFailed => "vault_cleanup_failed",
        }
    }

    pub fn new() -> Self {
        let mut broker = Self::with_broker(Box::new(ProEnvelopeBroker));
        broker.drive_letter_probe = crate::vault_drive_letters::occupied_letters;
        broker
    }

    fn with_broker(broker: Box<dyn AuthenticatedVaultBroker>) -> Self {
        Self::with_broker_and_attestor(broker, machine_presentation_attestation)
    }

    fn with_broker_and_attestor(
        broker: Box<dyn AuthenticatedVaultBroker>,
        caller_mount_attestor: CallerMountAttestor,
    ) -> Self {
        Self {
            active: Mutex::new(HashMap::new()),
            operation: Mutex::new(()),
            broker,
            caller_mount_attestor,
            policy_authorizer: crate::vault_access::authorize_mount_for_token,
            engine_snapshot: None,
            drive_letter_probe: {
                #[cfg(test)]
                {
                    || Ok(HashSet::new())
                }
                #[cfg(not(test))]
                {
                    crate::vault_drive_letters::occupied_letters
                }
            },
            recovery: Mutex::new(RecoveryState::default()),
        }
    }

    pub fn with_exclusive_operation<T>(&self, operation: impl FnOnce() -> T) -> T {
        let serialized = || {
            let _guard = self
                .operation
                .lock()
                .expect("vault operation lock poisoned");
            operation()
        };
        // Waiting readers must not occupy every I/O worker while a mount holds
        // this lock awaiting its broker. Keep the caller's impersonation thread.
        if tokio::runtime::Handle::try_current().is_ok_and(|runtime| {
            runtime.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
        }) {
            tokio::task::block_in_place(serialized)
        } else {
            serialized()
        }
    }

    fn snapshot(&self) -> Result<HashMap<u8, String>, String> {
        match self.engine_snapshot {
            Some(probe) => probe(),
            None => self.broker.observed_slots(),
        }
    }

    fn live_mount_matches(&self, mount: &ActiveMount) -> Result<bool, VaultMountReason> {
        let snapshot = self
            .snapshot()
            .map_err(|_| VaultMountReason::MountStateUnknown)?;
        match snapshot.get(&mount.internal_drive) {
            None => Ok(false),
            Some(identity) if mount.engine_mount_identity.as_ref() == Some(identity) => Ok(true),
            Some(_) => Err(VaultMountReason::MountStateUnknown),
        }
    }

    /// Called only after the captured named-pipe peer token was revalidated
    /// against this entry's grants.
    #[allow(clippy::too_many_arguments)]
    pub fn mount_authorized(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        entry_id: &str,
        password: &mut String,
        hidden_protection_password: &mut Option<String>,
        volume_role: VaultVolumeRole,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        caller_session: u32,
        caller_sid: &str,
        caller_authentication_id: (u32, i32),
        effective_access: wincmd_shared::vault_access::VaultAccess,
    ) -> VaultMountResult {
        self.with_exclusive_operation(|| {
            self.mount_authorized_locked(
                operation_id,
                store,
                entry_id,
                password,
                hidden_protection_password,
                volume_role,
                caller_token,
                caller_session,
                caller_sid,
                caller_authentication_id,
                effective_access,
            )
        })
    }

    /// Mount a service-registered personal container.  Unlike a managed
    /// policy mount this has no shared policy entry: its owner and per-user
    /// scope were fixed when the service created the backing file.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mount_personal_authorized(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        record: &PersonalVaultRecord,
        request: &mut PersonalVaultMountRequest,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        session_id: u32,
        caller_sid: &str,
        caller_authentication_id: (u32, i32),
    ) -> Result<(String, u8, bool), VaultMountReason> {
        self.with_exclusive_operation(|| {
            self.mount_personal_authorized_locked(
                operation_id,
                store,
                record,
                request,
                caller_token,
                session_id,
                caller_sid,
                caller_authentication_id,
            )
        })
    }

    /// The pipe keeps drive-letter inspection and the broker call inside the
    /// same operation gate, so a second mount cannot invalidate preflight in
    /// the gap before the encrypted driver receives the request.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mount_personal_authorized_locked(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        record: &PersonalVaultRecord,
        request: &mut PersonalVaultMountRequest,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        session_id: u32,
        caller_sid: &str,
        caller_authentication_id: (u32, i32),
    ) -> Result<(String, u8, bool), VaultMountReason> {
        self.mount_personal_with_entry_id_locked(
            operation_id,
            store,
            record,
            request,
            caller_token,
            session_id,
            caller_sid,
            caller_authentication_id,
            personal_mount_entry_id(record),
        )
    }

    /// Mount an ordinary, unmanaged file without creating a durable owner
    /// record. The identity-only key prevents different local accounts from
    /// concurrently mounting the same writable container.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mount_unmanaged_authorized_locked(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        record: &PersonalVaultRecord,
        request: &mut PersonalVaultMountRequest,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        session_id: u32,
        caller_sid: &str,
        caller_authentication_id: (u32, i32),
    ) -> Result<(String, u8, bool), VaultMountReason> {
        self.mount_personal_with_entry_id_locked(
            operation_id,
            store,
            record,
            request,
            caller_token,
            session_id,
            caller_sid,
            caller_authentication_id,
            unmanaged_mount_entry_id(record),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn mount_personal_with_entry_id_locked(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        record: &PersonalVaultRecord,
        request: &mut PersonalVaultMountRequest,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        session_id: u32,
        caller_sid: &str,
        caller_authentication_id: (u32, i32),
        entry_id: String,
    ) -> Result<(String, u8, bool), VaultMountReason> {
        if request.repair_current_account_access {
            request.zeroize_secrets();
            return Err(VaultMountReason::InvalidRequest);
        }
        if record.owner_sid != caller_sid
            || record.scope != request.presentation
            || session_id == 0
            || caller_sid.is_empty()
        {
            request.zeroize_secrets();
            return Err(VaultMountReason::NotAuthorized);
        }
        if !self.recovery_allows_entry(&entry_id, store) {
            request.zeroize_secrets();
            return Err(self.recovery_failure_reason());
        }
        let access = if request.read_only {
            wincmd_shared::vault_access::VaultAccess::Read
        } else {
            wincmd_shared::vault_access::VaultAccess::Write
        };
        let profile = match mount_profile(
            request.volume_kind,
            request.volume_role,
            access,
            request.hidden_protection_password.as_deref(),
        ) {
            Ok(profile) => profile,
            Err(reason) => {
                request.zeroize_secrets();
                return Err(reason);
            }
        };
        if !profile.requires_hidden_protection && request.hidden_protection_password.is_some() {
            request.zeroize_secrets();
            return Err(VaultMountReason::InvalidRequest);
        }
        if let Some(existing) = self
            .active
            .lock()
            .ok()
            .and_then(|active| active.get(&entry_id).cloned())
        {
            if !same_mount_owner(&existing, session_id, caller_sid) {
                request.zeroize_secrets();
                return Err(VaultMountReason::NotAuthorized);
            }
            request.zeroize_secrets();
            if existing.cleanup_required {
                return Err(VaultMountReason::DismountFailed);
            }
            self.attest_existing_mount(&existing, caller_token)?;
            return Err(VaultMountReason::AlreadyMounted);
        }
        request.preferred_letter = match self.select_mount_letter(
            store,
            request.preferred_letter.as_deref(),
            &record.container_identity,
        ) {
            Ok(letter) => Some(letter),
            Err(reason) => {
                request.zeroize_secrets();
                return Err(reason);
            }
        };
        let has_capacity = self
            .active
            .lock()
            .map(|active| active.contains_key(&entry_id) || active.len() < MAX_DURABLE_MOUNTS)
            .unwrap_or(false);
        if !has_capacity {
            request.zeroize_secrets();
            return Err(VaultMountReason::BrokerRejected);
        }
        let mut internal = InternalMountRequest {
            operation_id,
            container_path: record.container_path.clone(),
            mount_mode: profile.mount_mode,
            volume_kind: profile.container_kind,
            volume_role: profile.volume_role,
            read_only: request.read_only,
            personal: true,
            pim: request.pim,
            keyfiles: std::mem::take(&mut request.keyfiles),
            hidden_keyfiles: std::mem::take(&mut request.hidden_keyfiles),
            hidden_pim: request.hidden_pim,
            removable: request.removable,
            presentation: record.scope,
            preferred_letter: request.preferred_letter.clone(),
            target_session_id: session_id,
            caller_sid: caller_sid.to_string(),
            caller_token,
            caller_authentication_id,
            mounted_root_acl_sddl: mounted_root_acl_sddl(&[ResolvedGrant {
                sid: record.owner_sid.clone(),
                access: wincmd_shared::vault_access::VaultAccess::Write,
            }]),
            password: std::mem::take(&mut request.password),
            hidden_protection_password: std::mem::take(&mut request.hidden_protection_password),
        };
        let result = self.broker.mount(&mut internal);
        internal.zeroize_secrets();
        let reply = result?;
        if !valid_drive_letter(&reply.drive_letter) || reply.internal_drive > 25 {
            let cleanup = self.broker.dismount(BrokerDismountRequest {
                operation_id,
                internal_drive: reply.internal_drive,
                presented_drive_letter: None,
                presentation: record.scope,
                target_session_id: session_id,
                caller_sid,
                caller_token: Some(caller_token),
            });
            return Err(if cleanup.is_ok() {
                VaultMountReason::AclReadbackFailed
            } else {
                VaultMountReason::DismountFailed
            });
        }
        // A machine-wide drive can exist in the SYSTEM namespace yet remain
        // unusable to the signed-in account, for example when an encrypted
        // filesystem carries ACLs from another PC. Verify the exact slot and
        // root listing as the caller before reporting the mount as successful.
        if record.scope == VaultPresentation::Machine {
            let attestation = (self.caller_mount_attestor)(
                caller_token,
                &reply.drive_letter,
                reply.internal_drive,
                // Writable personal devices preserve ACLs; root create rights are not required.
                false,
            );
            if attestation != CallerPresentationAttestation::Available {
                let cleanup = self.broker.dismount(BrokerDismountRequest {
                    operation_id,
                    internal_drive: reply.internal_drive,
                    presented_drive_letter: per_user_presented_drive_letter(
                        record.scope,
                        reply.drive_letter.as_str(),
                    ),
                    presentation: record.scope,
                    target_session_id: session_id,
                    caller_sid,
                    caller_token: Some(caller_token),
                });
                if cleanup.is_err() {
                    let mounted_at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|value| value.as_secs())
                        .unwrap_or(0);
                    let mount = ActiveMount {
                        drive_letter: reply.drive_letter.clone(),
                        internal_drive: reply.internal_drive,
                        presentation: record.scope,
                        session_id,
                        caller_sid: caller_sid.to_owned(),
                        policy_id: "personal".into(),
                        policy_version: 1,
                        personal: true,
                        syncthing_managed: false,
                        container_identity: record.container_identity.clone(),
                        access,
                        mounted_at,
                        cleanup_required: true,
                        engine_mount_identity: self
                            .snapshot()
                            .ok()
                            .and_then(|slots| slots.get(&reply.internal_drive).cloned()),
                        canonical_container_path: Some(record.container_path.clone()),
                    };
                    if !self.retain_cleanup_mount(store, &entry_id, mount) {
                        self.mark_registry_untrusted();
                    }
                    return Err(VaultMountReason::DismountFailed);
                }
                return Err(
                    if matches!(
                        attestation,
                        CallerPresentationAttestation::RootAccessDenied
                            | CallerPresentationAttestation::RootWriteAccessDenied
                    ) {
                        VaultMountReason::CallerAccessDenied
                    } else {
                        VaultMountReason::PresentationRejected
                    },
                );
            }
        }
        let mounted_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or(0);
        let mut active = ActiveMount {
            drive_letter: reply.drive_letter.clone(),
            internal_drive: reply.internal_drive,
            presentation: record.scope,
            session_id,
            caller_sid: caller_sid.to_owned(),
            policy_id: "personal".into(),
            policy_version: 1,
            personal: true,
            syncthing_managed: false,
            container_identity: record.container_identity.clone(),
            access,
            mounted_at,
            // Unmanaged mounts preserve the existing filesystem permissions;
            // they never attest a service-installed root ACL.
            cleanup_required: false,
            engine_mount_identity: self
                .snapshot()
                .ok()
                .and_then(|slots| slots.get(&reply.internal_drive).cloned()),
            canonical_container_path: Some(record.container_path.clone()),
        };
        if active.engine_mount_identity.is_some() {
            if let Ok(mut mounts) = self.active.lock() {
                mounts.insert(entry_id.clone(), active.clone());
                if self.persist_active(store, &mounts).is_ok() {
                    // A missing binding is a no-op. An adapter failure must
                    // not invent a managed marker: that would make a normal
                    // dismount depend on a helper which never enrolled this
                    // Vault. The next mount will try the binding again.
                    active.syncthing_managed = syncthing_lifecycle_call(
                        "vault.syncthing.resume",
                        operation_id,
                        &entry_id,
                        &active,
                        Some(caller_token),
                    )
                    .unwrap_or(false);
                    if active.syncthing_managed {
                        mounts.insert(entry_id.clone(), active.clone());
                        if self.persist_active(store, &mounts).is_err() {
                            return Err(VaultMountReason::DismountFailed);
                        }
                    }
                    return Ok((reply.drive_letter, reply.internal_drive, reply.acl_attested));
                }
            }
        }
        let cleanup = self.broker.dismount(BrokerDismountRequest {
            operation_id,
            internal_drive: active.internal_drive,
            presented_drive_letter: per_user_presented_drive_letter(
                active.presentation,
                active.drive_letter.as_str(),
            ),
            presentation: active.presentation,
            target_session_id: active.session_id,
            caller_sid: &active.caller_sid,
            caller_token: Some(caller_token),
        });
        if cleanup.is_ok()
            && self
                .snapshot()
                .is_ok_and(|slots| !slots.contains_key(&active.internal_drive))
        {
            Err(if self.clear_retained_mount(store, &entry_id) {
                if active.engine_mount_identity.is_none() {
                    VaultMountReason::MountStateUnknown
                } else {
                    VaultMountReason::BrokerRejected
                }
            } else {
                VaultMountReason::DismountFailed
            })
        } else {
            self.mark_registry_untrusted();
            active.cleanup_required = true;
            let _ = self.retain_cleanup_mount(store, &entry_id, active);
            Err(VaultMountReason::DismountFailed)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn mount_authorized_locked(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        entry_id: &str,
        password: &mut String,
        hidden_protection_password: &mut Option<String>,
        volume_role: VaultVolumeRole,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        session_id: u32,
        caller_sid: &str,
        caller_authentication_id: (u32, i32),
        effective_access: wincmd_shared::vault_access::VaultAccess,
    ) -> VaultMountResult {
        if !self.recovery_allows_entry(entry_id, store) {
            zeroize_mount_secrets(password, hidden_protection_password);
            return failed(entry_id, None, self.recovery_failure_reason());
        }
        let Some((plan, presentation, preferred_letter, container_identity, container_kind)) =
            store.mount_plan(entry_id)
        else {
            zeroize_mount_secrets(password, hidden_protection_password);
            return denied(entry_id, VaultMountReason::NotAuthorized);
        };
        let profile = match mount_profile(
            container_kind,
            volume_role,
            effective_access,
            hidden_protection_password.as_deref(),
        ) {
            Ok(profile) => profile,
            Err(reason) => {
                zeroize_mount_secrets(password, hidden_protection_password);
                return failed(entry_id, Some(presentation), reason);
            }
        };
        if !profile.requires_hidden_protection && hidden_protection_password.is_some() {
            zeroize_mount_secrets(password, hidden_protection_password);
            return failed(
                entry_id,
                Some(presentation),
                VaultMountReason::InvalidRequest,
            );
        }
        if caller_sid.is_empty() || (presentation == VaultPresentation::PerUser && session_id == 0)
        {
            zeroize_mount_secrets(password, hidden_protection_password);
            return denied(entry_id, VaultMountReason::NotAuthorized);
        }
        let caller_sid = caller_sid.to_owned();
        if let Some(existing) = self
            .active
            .lock()
            .ok()
            .and_then(|active| active.get(entry_id).cloned())
        {
            // A repeated click must not dismount a live volume or disrupt open files.
            if existing.session_id != session_id || existing.caller_sid != caller_sid {
                zeroize_mount_secrets(password, hidden_protection_password);
                return denied(entry_id, VaultMountReason::NotAuthorized);
            }
            zeroize_mount_secrets(password, hidden_protection_password);
            if existing.cleanup_required {
                return failed(
                    entry_id,
                    Some(presentation),
                    VaultMountReason::DismountFailed,
                );
            }
            if let Err(reason) = self.attest_existing_mount(&existing, caller_token) {
                return failed(entry_id, Some(presentation), reason);
            }
            return VaultMountResult {
                entry_id: entry_id.to_owned(),
                state: VaultMountState::Failed,
                presentation: Some(existing.presentation),
                drive_letter: Some(existing.drive_letter),
                reason: Some(VaultMountReason::AlreadyMounted),
            };
        }
        let preferred_letter =
            match self.select_mount_letter(store, preferred_letter.as_deref(), &container_identity)
            {
                Ok(letter) => Some(letter),
                Err(reason) => {
                    zeroize_mount_secrets(password, hidden_protection_password);
                    return failed(entry_id, Some(presentation), reason);
                }
            };
        let has_capacity = self
            .active
            .lock()
            .map(|active| active.contains_key(entry_id) || active.len() < MAX_DURABLE_MOUNTS)
            .unwrap_or(false);
        if !has_capacity {
            zeroize_mount_secrets(password, hidden_protection_password);
            return failed(
                entry_id,
                Some(presentation),
                VaultMountReason::BrokerRejected,
            );
        }
        let mut request = InternalMountRequest {
            operation_id,
            container_path: plan.container.to_string_lossy().into_owned(),
            // Policy fixes the container kind; the caller chooses only a
            // bounded role for that registered dual container.
            mount_mode: profile.mount_mode,
            volume_kind: profile.container_kind,
            volume_role: profile.volume_role,
            read_only: false,
            personal: false,
            pim: None,
            keyfiles: Vec::new(),
            hidden_keyfiles: Vec::new(),
            hidden_pim: None,
            removable: false,
            presentation,
            preferred_letter,
            target_session_id: session_id,
            caller_sid: caller_sid.clone(),
            caller_token,
            caller_authentication_id,
            mounted_root_acl_sddl: mounted_root_acl_sddl(&plan.grants),
            password: std::mem::take(password),
            hidden_protection_password: std::mem::take(hidden_protection_password),
        };
        // A Fleet Vault is one machine-presented volume shared by the owner
        // and its authorized viewers. Mounting the whole device read-only
        // because this caller is a viewer would also block the owner (and an
        // explicitly authorized editor). The exact root DACL below is the
        // per-user access boundary, so leave the device writable and let
        // Windows enforce each principal's resolved grant.
        request.read_only = SHARED_VAULT_DEVICE_READ_ONLY;
        let reply = self.broker.mount(&mut request);
        request.zeroize_secrets();
        let reply = match reply {
            Ok(reply) => reply,
            Err(reason) => return failed(entry_id, Some(presentation), reason),
        };
        if !reply.acl_attested
            || !valid_drive_letter(&reply.drive_letter)
            || reply.internal_drive > 25
        {
            let _ = self.broker.dismount(BrokerDismountRequest {
                operation_id,
                internal_drive: reply.internal_drive,
                presented_drive_letter: None,
                presentation,
                target_session_id: session_id,
                caller_sid: &caller_sid,
                caller_token: Some(caller_token),
            });
            return failed(
                entry_id,
                Some(presentation),
                VaultMountReason::AclReadbackFailed,
            );
        }
        // A global link created in the service namespace is not enough.  It
        // must resolve through the original authenticated caller token and
        // permit the root directory read that File Explorer needs.  This keeps
        // shared Vaults fail-closed without requiring session-zero to discover
        // an Explorer window it cannot see.
        if presentation == VaultPresentation::Machine
            && machine_presentation_attestation(
                caller_token,
                &reply.drive_letter,
                reply.internal_drive,
                false,
            ) != CallerPresentationAttestation::Available
        {
            let cleanup = self.broker.dismount(BrokerDismountRequest {
                operation_id,
                internal_drive: reply.internal_drive,
                presented_drive_letter: None,
                presentation,
                target_session_id: session_id,
                caller_sid: &caller_sid,
                caller_token: Some(caller_token),
            });
            return failed(
                entry_id,
                Some(presentation),
                if cleanup.is_ok() {
                    VaultMountReason::PresentationRejected
                } else {
                    VaultMountReason::DismountFailed
                },
            );
        }
        let Some((policy_id, policy_version)) = store.active_policy_identity() else {
            let _ = self.broker.dismount(BrokerDismountRequest {
                operation_id,
                internal_drive: reply.internal_drive,
                presented_drive_letter: per_user_presented_drive_letter(
                    presentation,
                    reply.drive_letter.as_str(),
                ),
                presentation,
                target_session_id: session_id,
                caller_sid: &caller_sid,
                caller_token: Some(caller_token),
            });
            return failed(
                entry_id,
                Some(presentation),
                VaultMountReason::NotAuthorized,
            );
        };
        let engine_mount_identity = self
            .snapshot()
            .ok()
            .and_then(|slots| slots.get(&reply.internal_drive).cloned());
        if engine_mount_identity.is_none() {
            let cleanup = self.broker.dismount(BrokerDismountRequest {
                operation_id,
                internal_drive: reply.internal_drive,
                presented_drive_letter: per_user_presented_drive_letter(
                    presentation,
                    &reply.drive_letter,
                ),
                presentation,
                target_session_id: session_id,
                caller_sid: &caller_sid,
                caller_token: Some(caller_token),
            });
            if cleanup.is_err()
                || !self
                    .snapshot()
                    .is_ok_and(|slots| !slots.contains_key(&reply.internal_drive))
            {
                self.mark_registry_untrusted();
                let _ = self.retain_cleanup_mount(
                    store,
                    entry_id,
                    ActiveMount {
                        drive_letter: reply.drive_letter.clone(),
                        internal_drive: reply.internal_drive,
                        presentation,
                        session_id,
                        caller_sid,
                        policy_id,
                        policy_version,
                        personal: false,
                        syncthing_managed: false,
                        container_identity,
                        access: effective_access,
                        mounted_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|time| time.as_secs())
                            .unwrap_or(0),
                        cleanup_required: true,
                        engine_mount_identity: None,
                        canonical_container_path: None,
                    },
                );
            }
            return failed(
                entry_id,
                Some(presentation),
                VaultMountReason::MountStateUnknown,
            );
        }
        if let Ok(mut active) = self.active.lock() {
            let mounted_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_secs())
                .unwrap_or(0);
            let mut mount = ActiveMount {
                drive_letter: reply.drive_letter.clone(),
                internal_drive: reply.internal_drive,
                presentation,
                session_id,
                caller_sid,
                policy_id,
                policy_version,
                personal: false,
                syncthing_managed: false,
                container_identity,
                access: effective_access,
                mounted_at,
                cleanup_required: false,
                engine_mount_identity,
                canonical_container_path: None,
            };
            active.insert(entry_id.to_owned(), mount.clone());
            if self.persist_active(store, &active).is_err() {
                let mount = active.remove(entry_id);
                drop(active);
                if let Some(mount) = mount {
                    let _ = self.broker.dismount(BrokerDismountRequest {
                        operation_id,
                        internal_drive: mount.internal_drive,
                        presented_drive_letter: per_user_presented_drive_letter(
                            mount.presentation,
                            mount.drive_letter.as_str(),
                        ),
                        presentation: mount.presentation,
                        target_session_id: mount.session_id,
                        caller_sid: &mount.caller_sid,
                        caller_token: Some(caller_token),
                    });
                }
                return failed(
                    entry_id,
                    Some(presentation),
                    VaultMountReason::DismountFailed,
                );
            }
            // A missing binding is a no-op. Only the owner-only per-user
            // policy shape may resume a binding; a shared policy never gains
            // a local profile simply because it was mounted.
            if store.is_exclusive_per_user_policy_owner(entry_id, &mount.caller_sid) {
                mount.syncthing_managed = syncthing_lifecycle_call(
                    "vault.syncthing.resume",
                    operation_id,
                    entry_id,
                    &mount,
                    Some(caller_token),
                )
                .unwrap_or(false);
                if mount.syncthing_managed {
                    active.insert(entry_id.to_owned(), mount);
                    if self.persist_active(store, &active).is_err() {
                        return failed(
                            entry_id,
                            Some(presentation),
                            VaultMountReason::DismountFailed,
                        );
                    }
                }
            }
        } else {
            let _ = self.broker.dismount(BrokerDismountRequest {
                operation_id,
                internal_drive: reply.internal_drive,
                presented_drive_letter: per_user_presented_drive_letter(
                    presentation,
                    reply.drive_letter.as_str(),
                ),
                presentation,
                target_session_id: session_id,
                caller_sid: &caller_sid,
                caller_token: Some(caller_token),
            });
            return failed(
                entry_id,
                Some(presentation),
                VaultMountReason::DismountFailed,
            );
        }
        VaultMountResult {
            entry_id: entry_id.to_owned(),
            state: VaultMountState::Mounted,
            presentation: Some(presentation),
            drive_letter: Some(reply.drive_letter),
            reason: None,
        }
    }

    fn dismount_entry_locked(&self, store: &VaultAccessStore, entry_id: &str) -> VaultMountResult {
        self.dismount_entry_locked_for_client(next_internal_operation_id(), store, entry_id, None)
    }

    fn dismount_entry_locked_for_client(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        entry_id: &str,
        caller_token: Option<windows_sys::Win32::Foundation::HANDLE>,
    ) -> VaultMountResult {
        self.dismount_entry_locked_for_client_inner(operation_id, store, entry_id, caller_token)
    }

    fn dismount_entry_locked_for_client_inner(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        entry_id: &str,
        caller_token: Option<windows_sys::Win32::Foundation::HANDLE>,
    ) -> VaultMountResult {
        let active = self
            .active
            .lock()
            .ok()
            .and_then(|mut active| active.remove(entry_id));
        let Some(active) = active else {
            return VaultMountResult {
                entry_id: entry_id.to_owned(),
                state: VaultMountState::Unmounted,
                presentation: None,
                drive_letter: None,
                reason: None,
            };
        };
        if active.syncthing_managed {
            let paused = syncthing_lifecycle_call(
                "vault.syncthing.pause",
                operation_id,
                entry_id,
                &active,
                caller_token,
            );
            if !matches!(paused, Ok(true)) && caller_token.is_some() {
                if let Ok(mut mounts) = self.active.lock() {
                    mounts.insert(entry_id.to_owned(), active);
                }
                return failed(
                    entry_id,
                    Some(VaultPresentation::PerUser),
                    VaultMountReason::BrokerUnavailable,
                );
            }
        }
        let live = match self.live_mount_matches(&active) {
            Ok(live) => live,
            Err(reason) => {
                if let Ok(mut mounts) = self.active.lock() {
                    mounts.insert(entry_id.to_owned(), active);
                }
                return failed(entry_id, None, reason);
            }
        };
        if live
            && (self
                .broker
                .dismount(BrokerDismountRequest {
                    operation_id,
                    internal_drive: active.internal_drive,
                    presented_drive_letter: per_user_presented_drive_letter(
                        active.presentation,
                        active.drive_letter.as_str(),
                    ),
                    presentation: active.presentation,
                    target_session_id: active.session_id,
                    caller_sid: &active.caller_sid,
                    caller_token,
                })
                .is_err()
                || self
                    .snapshot()
                .map_or(true, |slots| slots.contains_key(&active.internal_drive)))
        {
            if active.syncthing_managed {
                let _ = syncthing_lifecycle_call(
                    "vault.syncthing.resume",
                    operation_id,
                    entry_id,
                    &active,
                    caller_token,
                );
            }
            if let Ok(mut mounts) = self.active.lock() {
                mounts.insert(entry_id.to_owned(), active.clone());
            }
            return failed(
                entry_id,
                Some(active.presentation),
                VaultMountReason::DismountFailed,
            );
        }
        if let Ok(mounts) = self.active.lock() {
            if self.persist_active(store, &mounts).is_err() {
                // The volume is already closed; keep the durable old record
                // for conservative cleanup on a later service start.
                self.mark_persistence_pending(entry_id);
                return failed(
                    entry_id,
                    Some(active.presentation),
                    VaultMountReason::DismountFailed,
                );
            }
        }
        VaultMountResult {
            entry_id: entry_id.to_owned(),
            state: VaultMountState::Unmounted,
            presentation: Some(active.presentation),
            drive_letter: None,
            reason: None,
        }
    }

    /// An already-authorized Fleet member can close a policy-managed mount
    /// opened in another user's Windows session.  The SYSTEM recovery broker
    /// receives only the service-owned driver slot, never a path, user token,
    /// or renderer-provided drive mapping.
    fn dismount_entry_locked_for_authorized_member(
        &self,
        store: &VaultAccessStore,
        entry_id: &str,
    ) -> VaultMountResult {
        let active = self
            .active
            .lock()
            .ok()
            .and_then(|mut active| active.remove(entry_id));
        let Some(active) = active else {
            return VaultMountResult {
                entry_id: entry_id.to_owned(),
                state: VaultMountState::Unmounted,
                presentation: None,
                drive_letter: None,
                reason: None,
            };
        };
        let live = match self.live_mount_matches(&active) {
            Ok(live) => live,
            Err(reason) => {
                if let Ok(mut mounts) = self.active.lock() {
                    mounts.insert(entry_id.to_owned(), active);
                }
                return failed(entry_id, None, reason);
            }
        };
        if live
            && (self.broker.recover_dismount(active.internal_drive).is_err()
                || self
                    .snapshot()
                    .map_or(true, |slots| slots.contains_key(&active.internal_drive)))
        {
            if let Ok(mut mounts) = self.active.lock() {
                mounts.insert(entry_id.to_owned(), active.clone());
            }
            return failed(
                entry_id,
                Some(active.presentation),
                VaultMountReason::DismountFailed,
            );
        }
        if let Ok(mounts) = self.active.lock() {
            if self.persist_active(store, &mounts).is_err() {
                self.mark_persistence_pending(entry_id);
                return failed(
                    entry_id,
                    Some(active.presentation),
                    VaultMountReason::DismountFailed,
                );
            }
        }
        VaultMountResult {
            entry_id: entry_id.to_owned(),
            state: VaultMountState::Unmounted,
            presentation: Some(active.presentation),
            drive_letter: None,
            reason: None,
        }
    }

    pub fn dismount_authorized(
        &self,
        store: &VaultAccessStore,
        request: AuthorizedDismount<'_>,
    ) -> VaultMountResult {
        self.with_exclusive_operation(|| self.dismount_authorized_locked(store, &request))
    }

    fn dismount_authorized_locked(
        &self,
        store: &VaultAccessStore,
        request: &AuthorizedDismount<'_>,
    ) -> VaultMountResult {
        if self
            .recovery
            .lock()
            .map_or(true, |state| state.registry_untrusted)
        {
            return denied(request.entry_id, VaultMountReason::MountStateUnknown);
        }
        let active = self
            .active
            .lock()
            .ok()
            .and_then(|active| active.get(request.entry_id).cloned());
        let Some(active) = active else {
            return denied(request.entry_id, VaultMountReason::MountStateUnknown);
        };
        if let Err(reason) = self.dismount_permission(store, request.entry_id, &active, request) {
            return denied(request.entry_id, reason);
        }
        let slots = match self.snapshot() {
            Ok(slots) => slots,
            Err(_) => return failed(request.entry_id, None, VaultMountReason::MountStateUnknown),
        };
        match slots.get(&active.internal_drive) {
            None => {
                let Ok(mut mounts) = self.active.lock() else {
                    return failed(request.entry_id, None, VaultMountReason::MountStateUnknown);
                };
                mounts.remove(request.entry_id);
                if self.persist_active(store, &mounts).is_err() {
                    mounts.insert(request.entry_id.to_owned(), active);
                    return failed(request.entry_id, None, VaultMountReason::MountStateUnknown);
                }
                return VaultMountResult {
                    entry_id: request.entry_id.into(),
                    state: VaultMountState::Unmounted,
                    presentation: None,
                    drive_letter: None,
                    reason: None,
                };
            }
            Some(identity) if active.engine_mount_identity.as_ref() == Some(identity) => {}
            Some(_) => return failed(request.entry_id, None, VaultMountReason::MountStateUnknown),
        }
        if !same_mount_owner(&active, request.caller_session, request.caller_sid) {
            return self.dismount_entry_locked_for_authorized_member(store, request.entry_id);
        }
        self.dismount_entry_locked_for_client(
            request.operation_id,
            store,
            request.entry_id,
            Some(request.caller_token),
        )
    }

    fn dismount_permission(
        &self,
        store: &VaultAccessStore,
        entry_id: &str,
        mount: &ActiveMount,
        caller: &AuthorizedDismount<'_>,
    ) -> Result<(), VaultMountReason> {
        if caller.caller_sid.is_empty() || caller.caller_session == 0 {
            return Err(VaultMountReason::MountStateUnknown);
        }
        // A personal mount belongs to its authenticated originating session,
        // even when its drive letter is machine-visible. Visibility and an
        // elevated token do not transfer ownership of another user's mount.
        if (mount.personal || mount.presentation == VaultPresentation::PerUser)
            && !same_mount_owner(mount, caller.caller_session, caller.caller_sid)
        {
            return Err(VaultMountReason::MountStateUnknown);
        }
        if !mount.personal {
            let authorization = (self.policy_authorizer)(store, entry_id, caller.caller_token);
            if !authorization.allowed || authorization.presentation != Some(mount.presentation) {
                return Err(if mount.presentation == VaultPresentation::PerUser {
                    VaultMountReason::MountStateUnknown
                } else {
                    VaultMountReason::PolicyAccessDenied
                });
            }
            if mount.presentation == VaultPresentation::PerUser {
                let owner = store
                    .policy()
                    .and_then(|policy| {
                        policy
                            .entries
                            .into_iter()
                            .find(|entry| entry.id == entry_id)
                    })
                    .and_then(|entry| entry.primary_owner_sid);
                if owner.as_deref() != Some(caller.caller_sid) {
                    return Err(VaultMountReason::MountStateUnknown);
                }
            }
        }
        if mount.presentation == VaultPresentation::Machine
            && !same_mount_owner(mount, caller.caller_session, caller.caller_sid)
            && !caller.caller_elevated
        {
            return Err(VaultMountReason::AdministratorRequired);
        }
        Ok(())
    }

    /// Secure Storage dismounts use the service-owned active-mount record,
    /// rather than asking the renderer to close a driver slot directly.  This
    /// removes the durable active record only after the broker confirms the
    /// same slot closed, so an unavailable presentation cannot linger in the
    /// UI as a false mounted volume.
    pub(crate) fn dismount_personal_for_caller(
        &self,
        store: &VaultAccessStore,
        operation_id: u64,
        internal_drive: u8,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        caller_session: u32,
        caller_sid: &str,
        caller_elevated: bool,
    ) -> VaultMountResult {
        self.with_exclusive_operation(|| {
            let active = self.active.lock().ok();
            let matching = active.as_ref().and_then(|mounts| {
                mounts
                    .iter()
                    .find(|(_, mount)| mount.internal_drive == internal_drive)
                    .map(|(entry_id, _)| entry_id.clone())
            });
            drop(active);

            let synthetic_entry_id = format!("personal:{internal_drive}");
            let Some(entry_id) = matching else {
                return denied(&synthetic_entry_id, VaultMountReason::MountStateUnknown);
            };
            let result = self.dismount_authorized_locked(
                store,
                &AuthorizedDismount {
                    operation_id,
                    entry_id: &entry_id,
                    caller_token,
                    caller_session,
                    caller_sid,
                    caller_elevated,
                },
            );
            // A guessed slot must not disclose the opaque private entry ID.
            VaultMountResult {
                entry_id: synthetic_entry_id,
                ..result
            }
        })
    }

    /// Enrols one child directory of an already-mounted owner-only per-user
    /// Vault. The service supplies the stable entry, owner and mounted
    /// identity; Pro only creates/controls the owner-session profile selected
    /// by those facts.
    pub(crate) fn enroll_personal_syncthing(
        &self,
        store: &VaultAccessStore,
        operation_id: u64,
        internal_drive: u8,
        relative_path: &str,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        caller_session: u32,
        caller_sid: &str,
    ) -> Result<String, VaultMountReason> {
        if !valid_relative_sync_path(relative_path) {
            return Err(VaultMountReason::InvalidRequest);
        }
        self.with_exclusive_operation(|| {
            let (entry_id, active) = self
                .active
                .lock()
                .ok()
                .and_then(|mounts| {
                    mounts
                        .iter()
                        .find(|(_, mount)| mount.internal_drive == internal_drive)
                        .map(|(entry_id, mount)| (entry_id.clone(), mount.clone()))
                })
                .ok_or(VaultMountReason::MountStateUnknown)?;
            let eligible_policy_owner = !active.personal
                && store.is_exclusive_per_user_policy_owner(&entry_id, caller_sid);
            if !(active.personal || eligible_policy_owner)
                || active.presentation != VaultPresentation::PerUser
                || !same_mount_owner(&active, caller_session, caller_sid)
                || !self.live_mount_matches(&active)?
            {
                return Err(VaultMountReason::NotAuthorized);
            }
            let gui_url = syncthing_enroll_call(
                operation_id,
                &entry_id,
                &active,
                caller_token,
                relative_path,
            )?;
            let resumed = syncthing_lifecycle_call(
                "vault.syncthing.resume",
                operation_id,
                &entry_id,
                &active,
                Some(caller_token),
            )?;
            if !resumed {
                return Err(VaultMountReason::BrokerRejected);
            }
            let mut updated = active;
            updated.syncthing_managed = true;
            let mut mounts = self.active.lock().map_err(|_| VaultMountReason::BrokerRejected)?;
            mounts.insert(entry_id, updated);
            self.persist_active(store, &mounts)
                .map_err(|_| VaultMountReason::DismountFailed)?;
            Ok(gui_url)
        })
    }

    pub fn projection(&self, entry_id: &str) -> (VaultMountState, Option<String>) {
        self.active
            .lock()
            .ok()
            .and_then(|active| active.get(entry_id).cloned())
            .map(|mount| (VaultMountState::Mounted, Some(mount.drive_letter)))
            .unwrap_or((VaultMountState::Unmounted, None))
    }

    fn select_mount_letter(
        &self,
        store: &VaultAccessStore,
        preferred_letter: Option<&str>,
        container_identity: &str,
    ) -> Result<String, VaultMountReason> {
        let unavailable = VaultMountReason::EngineDriveLetterUnavailable;
        let occupied = self.occupied_letters_locked()?;
        let letter = match preferred_letter {
            Some(letter) => letter.trim_end_matches(':').to_ascii_uppercase(),
            None => store
                .auto_pick_unreserved_letter(&occupied)
                .ok_or(unavailable)?,
        };
        // A generic refusal discloses no identity or details of the reserving Vault.
        if occupied.contains(&letter)
            || store.reserved_letter_conflicts_with_container(&letter, container_identity)
        {
            return Err(unavailable);
        }
        Ok(letter)
    }

    pub(crate) fn occupied_letters_locked(&self) -> Result<HashSet<String>, VaultMountReason> {
        let unavailable = VaultMountReason::EngineDriveLetterUnavailable;
        let mut occupied = (self.drive_letter_probe)().map_err(|_| unavailable)?;
        occupied.extend(
            self.active
                .lock()
                .map_err(|_| unavailable)?
                .values()
                .map(|mount| {
                    mount
                        .drive_letter
                        .trim_end_matches(':')
                        .to_ascii_uppercase()
                }),
        );
        Ok(occupied)
    }

    fn attest_existing_mount(
        &self,
        mount: &ActiveMount,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
    ) -> Result<(), VaultMountReason> {
        match (self.caller_mount_attestor)(
            caller_token,
            &mount.drive_letter,
            mount.internal_drive,
            !mount.personal && mount.access == wincmd_shared::vault_access::VaultAccess::Write,
        ) {
            CallerPresentationAttestation::Available => Ok(()),
            CallerPresentationAttestation::RootAccessDenied
            | CallerPresentationAttestation::RootWriteAccessDenied => {
                Err(VaultMountReason::CallerAccessDenied)
            }
            _ => Err(VaultMountReason::PresentationRejected),
        }
    }

    pub(crate) fn unavailable_letters_locked(
        &self,
        store: &VaultAccessStore,
        exclude_entry_id: Option<&str>,
    ) -> Result<Vec<String>, VaultMountReason> {
        let mut letters = self.occupied_letters_locked()?;
        letters.extend(
            store
                .reserved_letters(exclude_entry_id)
                .map_err(|_| VaultMountReason::BrokerRejected)?,
        );
        let mut letters = letters.into_iter().collect::<Vec<_>>();
        letters.sort_unstable();
        Ok(letters)
    }

    pub(crate) fn personal_mounts_for_caller(
        &self,
        store: &VaultAccessStore,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        session_id: u32,
        caller_sid: &str,
        caller_elevated: bool,
    ) -> Result<Vec<PersonalVaultMountedVolume>, VaultMountReason> {
        self.with_exclusive_operation(|| {
            self.mount_inventory_locked(
                store,
                caller_token,
                session_id,
                caller_sid,
                caller_elevated,
            )
        })
    }

    fn mount_inventory_locked(
        &self,
        store: &VaultAccessStore,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        session_id: u32,
        caller_sid: &str,
        caller_elevated: bool,
    ) -> Result<Vec<PersonalVaultMountedVolume>, VaultMountReason> {
        if session_id == 0 || caller_sid.is_empty() {
            return Err(VaultMountReason::NotAuthorized);
        }
        if self
            .recovery
            .lock()
            .map_or(true, |state| state.registry_untrusted)
        {
            return Err(VaultMountReason::MountStateUnknown);
        }
        let slots = self
            .snapshot()
            .map_err(|_| VaultMountReason::MountStateUnknown)?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| VaultMountReason::MountStateUnknown)?;
        // An unregistered native slot has unknown scope and ownership. Never
        // invent a public row or report an incomplete inventory as all-clear.
        if slots
            .keys()
            .any(|slot| !active.values().any(|mount| mount.internal_drive == *slot))
        {
            return Err(VaultMountReason::MountStateUnknown);
        }
        let previous = active.clone();
        active.retain(|_, mount| slots.contains_key(&mount.internal_drive));
        if active.len() != previous.len() && self.persist_active(store, &active).is_err() {
            *active = previous;
            return Err(VaultMountReason::MountStateUnknown);
        }
        let mut mounts = Vec::new();
        for (entry_id, mount) in active.iter() {
            let request = AuthorizedDismount {
                operation_id: 0,
                entry_id,
                caller_token,
                caller_session: session_id,
                caller_sid,
                caller_elevated,
            };
            let permission = self.dismount_permission(store, entry_id, mount, &request);
            if matches!(permission, Err(reason) if reason != VaultMountReason::AdministratorRequired)
            {
                continue;
            }
            if mount.engine_mount_identity.as_ref() != slots.get(&mount.internal_drive) {
                return Err(VaultMountReason::MountStateUnknown);
            }
            let path = if mount.personal {
                mount.canonical_container_path.clone()
            } else {
                store
                    .policy()
                    .and_then(|policy| {
                        policy
                            .entries
                            .into_iter()
                            .find(|entry| entry.id == *entry_id)
                    })
                    .map(|entry| entry.container_path)
            };
            mounts.push(PersonalVaultMountedVolume {
                drive_letter: mount.drive_letter.clone(),
                internal_drive: mount.internal_drive,
                presentation: mount.presentation,
                cleanup_required: mount.cleanup_required,
                browse_allowed: true,
                dismount_allowed: permission.is_ok(),
                dismount_reason: permission.err(),
                canonical_container_path: path.and_then(|path| {
                    wincmd_shared::vault_display_path::normalize_vault_display_path(&path)
                }),
            });
        }
        mounts.sort_by_key(|mount| mount.internal_drive);
        Ok(mounts)
    }

    pub fn dismount_all(&self, store: &VaultAccessStore) -> Result<(), VaultMountReason> {
        self.with_exclusive_operation(|| self.dismount_all_locked(store))
    }

    /// Policy ownership is deliberately immutable while a container is live.
    /// Callers use this under `with_exclusive_operation`; it does not dismount
    /// on their behalf because an implicit close would let an administrator
    /// bypass the owner's active session.
    pub(crate) fn has_active_mounts_locked(&self) -> bool {
        if self.recovery.lock().map_or(true, |state| state.registry_untrusted) {
            return true;
        }
        self.active
            .lock()
            .map(|active| !active.is_empty())
            .unwrap_or(true)
    }

    /// Called under the operation lock with targets resolved by the policy
    /// store. Requested renderer identities are not authoritative here.
    pub(crate) fn reject_policy_changes_while_mounted_locked(
        &self,
        changed_entry_ids: &HashSet<String>,
        changed_container_identities: &HashSet<String>,
    ) -> Result<(), VaultMountReason> {
        if changed_entry_ids.is_empty() && changed_container_identities.is_empty() {
            return Ok(());
        }
        let recovery = self
            .recovery
            .lock()
            .map_err(|_| VaultMountReason::MountStateUnknown)?;
        if recovery.registry_untrusted
            || !recovery.persistence_pending.is_empty()
            || !recovery.removal_pending.is_empty()
        {
            return Err(VaultMountReason::MountStateUnknown);
        }
        drop(recovery);
        let slots = self
            .snapshot()
            .map_err(|_| VaultMountReason::MountStateUnknown)?;
        let active = self
            .active
            .lock()
            .map_err(|_| VaultMountReason::MountStateUnknown)?;
        // An unknown or reused slot cannot be assumed to belong to another
        // container. Match the driver identity, not just its reusable index.
        for (slot, identity) in &slots {
            let mut matches = active
                .iter()
                .filter(|(_, mount)| mount.internal_drive == *slot);
            let Some((entry_id, mount)) = matches.next() else {
                return Err(VaultMountReason::MountStateUnknown);
            };
            if matches.next().is_some() || mount.engine_mount_identity.as_ref() != Some(identity) {
                return Err(VaultMountReason::MountStateUnknown);
            }
            if changed_entry_ids.contains(entry_id)
                || changed_container_identities.contains(&mount.container_identity)
            {
                return Err(VaultMountReason::AlreadyMounted);
            }
        }
        Ok(())
    }

    pub(crate) fn dismount_all_locked(
        &self,
        store: &VaultAccessStore,
    ) -> Result<(), VaultMountReason> {
        let entries = self
            .active
            .lock()
            .map(|active| active.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for entry_id in entries {
            if self.dismount_entry_locked(store, &entry_id).state != VaultMountState::Unmounted {
                return Err(VaultMountReason::DismountFailed);
            }
        }
        self.broker.cleanup_orphans()?;
        Ok(())
    }

    pub fn dismount_session(&self, store: &VaultAccessStore, session_id: u32) {
        self.with_exclusive_operation(|| self.dismount_session_locked(store, session_id));
    }

    fn dismount_session_locked(&self, store: &VaultAccessStore, session_id: u32) {
        let entries = self
            .active
            .lock()
            .map(|active| {
                active
                    .iter()
                    .filter(|(_, mount)| mount.session_id == session_id)
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for entry_id in entries {
            let _ = self.dismount_entry_locked(store, &entry_id);
        }
    }

    /// At boot, stale protected records are closed by their exact internal
    /// slot. Ambiguous/corrupt records or a failed cleanup deny new mounts;
    /// this prevents a reboot from silently preserving an old presentation.
    pub fn load_and_cleanup(
        &self,
        store: &VaultAccessStore,
    ) -> Result<HashSet<String>, VaultMountReason> {
        let bytes = match store.read_active_mounts() {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
            Err(_) => {
                self.mark_registry_untrusted();
                return Err(VaultMountReason::DismountFailed);
            }
        };
        let registry: DurableMountRegistry = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => {
                self.mark_registry_untrusted();
                return Err(VaultMountReason::DismountFailed);
            }
        };
        if registry.mounts.len() > MAX_DURABLE_MOUNTS
            || registry
                .mounts
                .iter()
                .any(|(entry, mount)| !valid_durable_mount(entry, mount))
        {
            self.mark_registry_untrusted();
            return Err(VaultMountReason::DismountFailed);
        }
        let mut recovered_identities = HashSet::new();
        for mount in registry.mounts.values() {
            let live = self.live_mount_matches(mount).map_err(|reason| {
                self.mark_registry_untrusted();
                reason
            })?;
            if live
                && (self.broker.recover_dismount(mount.internal_drive).is_err()
                    || self
                        .snapshot()
                        .map_or(true, |slots| slots.contains_key(&mount.internal_drive)))
            {
                self.mark_registry_untrusted();
                return Err(VaultMountReason::DismountFailed);
            }
            recovered_identities.insert(mount.container_identity.clone());
        }
        let empty = serde_json::to_vec(&DurableMountRegistry {
            mounts: HashMap::new(),
        })
        .map_err(|_| VaultMountReason::DismountFailed)?;
        if store.write_active_mounts(&empty).is_err() {
            self.mark_registry_untrusted();
            return Err(VaultMountReason::DismountFailed);
        }
        Ok(recovered_identities)
    }

    fn persist_active(
        &self,
        store: &VaultAccessStore,
        active: &HashMap<String, ActiveMount>,
    ) -> Result<(), ()> {
        if active.len() > MAX_DURABLE_MOUNTS {
            return Err(());
        }
        let bytes = serde_json::to_vec(&DurableMountRegistry {
            mounts: active.clone(),
        })
        .map_err(|_| ())?;
        store.write_active_mounts(&bytes).map_err(|_| ())?;
        if let Ok(mut recovery) = self.recovery.lock() {
            recovery.persistence_pending.clear();
        }
        Ok(())
    }

    fn retain_cleanup_mount(
        &self,
        store: &VaultAccessStore,
        entry_id: &str,
        mount: ActiveMount,
    ) -> bool {
        let Ok(mut active) = self.active.lock() else {
            self.mark_registry_untrusted();
            return false;
        };
        active.insert(entry_id.to_owned(), mount);
        if self.persist_active(store, &active).is_ok() {
            true
        } else {
            self.mark_persistence_pending(entry_id);
            false
        }
    }

    fn clear_retained_mount(&self, store: &VaultAccessStore, entry_id: &str) -> bool {
        let Ok(mut active) = self.active.lock() else {
            self.mark_registry_untrusted();
            return false;
        };
        let mut without = active.clone();
        without.remove(entry_id);
        if self.persist_active(store, &without).is_err() {
            self.mark_removal_pending(entry_id);
            return false;
        }
        active.remove(entry_id);
        true
    }

    fn recovery_failure_reason(&self) -> VaultMountReason {
        if self
            .recovery
            .lock()
            .map_or(true, |state| state.registry_untrusted)
        {
            VaultMountReason::MountStateUnknown
        } else {
            VaultMountReason::DismountFailed
        }
    }

    fn recovery_allows_entry(&self, entry_id: &str, store: &VaultAccessStore) -> bool {
        let blocked = self
            .recovery
            .lock()
            .map(|state| {
                state.registry_untrusted
                    || state.persistence_pending.contains(entry_id)
                    || !state.removal_pending.is_empty()
            })
            .unwrap_or(true);
        if !blocked {
            return true;
        }
        // Retry only a known persistence repair. An ambiguous boot record
        // remains fail-closed until an operator repairs it.
        let registry_untrusted = self
            .recovery
            .lock()
            .map(|state| state.registry_untrusted)
            .unwrap_or(true);
        if registry_untrusted {
            return false;
        }
        let mut desired = match self.active.lock() {
            Ok(active) => active.clone(),
            Err(_) => return false,
        };
        let removals = self
            .recovery
            .lock()
            .map(|state| state.removal_pending.clone())
            .unwrap_or_default();
        for removal in &removals {
            desired.remove(removal);
        }
        if self.persist_active(store, &desired).is_err() {
            return false;
        }
        if let Ok(mut active) = self.active.lock() {
            *active = desired;
        } else {
            self.mark_registry_untrusted();
            return false;
        }
        if let Ok(mut recovery) = self.recovery.lock() {
            recovery.removal_pending.clear();
        }
        true
    }

    fn mark_persistence_pending(&self, entry_id: &str) {
        if let Ok(mut recovery) = self.recovery.lock() {
            recovery.persistence_pending.insert(entry_id.to_owned());
        }
    }

    fn mark_removal_pending(&self, entry_id: &str) {
        if let Ok(mut recovery) = self.recovery.lock() {
            recovery.removal_pending.insert(entry_id.to_owned());
        }
    }

    fn mark_registry_untrusted(&self) {
        if let Ok(mut recovery) = self.recovery.lock() {
            recovery.registry_untrusted = true;
        }
    }
}

fn valid_durable_mount(entry_id: &str, mount: &ActiveMount) -> bool {
    !entry_id.is_empty()
        && entry_id.len() <= 64
        && entry_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && valid_drive_letter(&mount.drive_letter)
        && mount.internal_drive <= 25
        && mount.caller_sid.starts_with("S-")
        && mount.caller_sid.len() <= 184
        && !mount.policy_id.is_empty()
        && mount.policy_id.len() <= 64
        && mount.policy_version > 0
        && !mount.container_identity.is_empty()
        && mount.container_identity.len() <= 256
        && mount
            .engine_mount_identity
            .as_ref()
            .map_or(true, |identity| {
                !identity.is_empty() && identity.len() <= 128 && !identity.contains('\0')
            })
        && mount
            .canonical_container_path
            .as_ref()
            .map_or(true, |path| {
                !path.is_empty() && path.len() <= 32768 && !path.contains('\0')
            })
        && mount.mounted_at > 0
}

#[cfg(test)]
fn personal_mount_entry_id(record: &PersonalVaultRecord) -> String {
    let mut digest = Sha256::new();
    digest.update(record.owner_sid.as_bytes());
    digest.update([0]);
    digest.update(record.container_identity.as_bytes());
    format!(
        "personal-{}",
        digest.finalize()[..24]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn unmanaged_mount_entry_id(record: &PersonalVaultRecord) -> String {
    let mut digest = Sha256::new();
    digest.update(b"unmanaged\0");
    digest.update(record.container_identity.as_bytes());
    format!(
        "unmanaged-{}",
        digest.finalize()[..24]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn mounted_root_acl_sddl(grants: &[ResolvedGrant]) -> MountedRootAclSddl {
    // OI+CI makes the proven root policy flow to files and directories
    // created after mount. Without it, a Partner-created file receives the
    // creator's default DACL and another authorized writer can be denied even
    // though both callers can open the volume root.
    // Do not grant BUILTIN\\Administrators (BA) access here. A policy owner
    // can deliberately make another local administrator view-only; a BA full
    // control ACE would silently override that resolved read grant in Explorer.
    // SYSTEM retains full access for the service/broker. Local administrators
    // can still exercise Windows' privileged ownership recovery outside this
    // product boundary, but receive no ordinary file-write grant from Fleet.
    let mut sddl = String::from("D:P(A;OICI;FA;;;SY)");
    for grant in grants {
        if grant.sid.starts_with("S-")
            && grant
                .sid
                .chars()
                .all(|c| c == 'S' || c.is_ascii_digit() || c == '-')
        {
            let mask = if grant.access == wincmd_shared::vault_access::VaultAccess::Write {
                "0x001301BF"
            } else {
                "0x001200A9"
            };
            sddl.push_str("(A;OICI;");
            sddl.push_str(mask);
            sddl.push_str(";;;");
            sddl.push_str(&grant.sid);
            sddl.push(')');
        }
    }
    MountedRootAclSddl(sddl)
}

fn valid_drive_letter(value: &str) -> bool {
    let value = value.strip_suffix(':').unwrap_or(value);
    value.len() == 1 && value.as_bytes()[0].is_ascii_alphabetic()
}

/// A shared Vault has a machine-wide drive link, but that link is useful only
/// if the exact authenticated pipe caller can resolve it and enumerate its
/// root.  The broker runs in session zero, so it cannot truthfully perform
/// this check by looking for Explorer itself.  The service already owns the
/// caller token; impersonate that token for this narrow, read-only attestation.
fn machine_presentation_attestation(
    caller_token: windows_sys::Win32::Foundation::HANDLE,
    drive_letter: &str,
    internal_drive: u8,
    require_write_access: bool,
) -> CallerPresentationAttestation {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::{ImpersonateLoggedOnUser, RevertToSelf};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, QueryDosDeviceW, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };

    struct RevertGuard;
    impl Drop for RevertGuard {
        fn drop(&mut self) {
            if unsafe { RevertToSelf() } == 0 {
                // Continuing a reusable SYSTEM worker under a caller token is
                // a security-boundary failure.  Fail-stop rather than risk
                // serving another client as the previous one.
                std::process::abort();
            }
        }
    }

    let normalized = drive_letter.strip_suffix(':').unwrap_or(drive_letter);
    let Some(letter) = normalized
        .as_bytes()
        .first()
        .copied()
        .filter(|letter| normalized.len() == 1 && letter.is_ascii_alphabetic())
        .map(|letter| letter.to_ascii_uppercase() as char)
    else {
        return CallerPresentationAttestation::MappingUnavailable;
    };
    if internal_drive > 25 || caller_token.is_null() {
        return CallerPresentationAttestation::MappingUnavailable;
    }
    if unsafe { ImpersonateLoggedOnUser(caller_token) } == 0 {
        return CallerPresentationAttestation::MappingUnavailable;
    }
    let guard = RevertGuard;
    let result = (|| {
        let dos_name = [letter as u16, b':' as u16, 0];
        let mut target = [0u16; 32_768];
        let target_len =
            unsafe { QueryDosDeviceW(dos_name.as_ptr(), target.as_mut_ptr(), target.len() as u32) }
                as usize;
        if target_len == 0 || target_len >= target.len() {
            return CallerPresentationAttestation::MappingUnavailable;
        }
        let Some(first_target_len) = target[..=target_len].iter().position(|unit| *unit == 0)
        else {
            return CallerPresentationAttestation::MappingUnavailable;
        };
        let Ok(target) = String::from_utf16(&target[..first_target_len]) else {
            return CallerPresentationAttestation::MappingUnavailable;
        };
        let expected = format!(
            r"\Device\VeraCryptVolume{}",
            char::from(b'A' + internal_drive)
        );
        if !target.eq_ignore_ascii_case(&expected) {
            return CallerPresentationAttestation::MappingUnavailable;
        }
        let root = format!("{letter}:\\");
        let read_failure = |error: std::io::Error| {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                CallerPresentationAttestation::RootAccessDenied
            } else {
                CallerPresentationAttestation::RootReadFailed
            }
        };
        match std::fs::read_dir(&root) {
            Ok(mut entries) => match entries.next() {
                Some(Ok(_)) | None => {}
                Some(Err(error)) => return read_failure(error),
            },
            Err(error) => return read_failure(error),
        }
        if !require_write_access {
            return CallerPresentationAttestation::Available;
        }
        // Opening the directory with its create-child rights is a read-only
        // access check: it verifies the selected mount mode without creating
        // a probe file in user data.
        let mut root_wide = std::ffi::OsStr::new(&root)
            .encode_wide()
            .collect::<Vec<_>>();
        root_wide.push(0);
        let handle = unsafe {
            CreateFileW(
                root_wide.as_ptr(),
                FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return if unsafe { GetLastError() }
                == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED
            {
                CallerPresentationAttestation::RootWriteAccessDenied
            } else {
                CallerPresentationAttestation::RootReadFailed
            };
        }
        unsafe { CloseHandle(handle) };
        CallerPresentationAttestation::Available
    })();
    drop(guard);
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MountProfile {
    mount_mode: &'static str,
    container_kind: &'static str,
    volume_role: &'static str,
    requires_hidden_protection: bool,
}

fn mount_profile(
    container_kind: VaultContainerKind,
    volume_role: VaultVolumeRole,
    effective_access: wincmd_shared::vault_access::VaultAccess,
    hidden_protection_password: Option<&str>,
) -> Result<MountProfile, VaultMountReason> {
    let (mount_mode, broker_container_kind, broker_volume_role) =
        match (container_kind, volume_role) {
            (VaultContainerKind::Standard, VaultVolumeRole::Outer) => {
                ("standard", "standard", "standard")
            }
            (VaultContainerKind::Dual, VaultVolumeRole::Outer) => ("standard", "dual", "outer"),
            (VaultContainerKind::Dual, VaultVolumeRole::Hidden) => ("hidden", "dual", "hidden"),
            (VaultContainerKind::Standard, VaultVolumeRole::Hidden) => {
                return Err(VaultMountReason::InvalidRequest);
            }
        };
    let requires_hidden_protection = container_kind == VaultContainerKind::Dual
        && volume_role == VaultVolumeRole::Outer
        && effective_access == wincmd_shared::vault_access::VaultAccess::Write;
    if requires_hidden_protection && hidden_protection_password.unwrap_or("").is_empty() {
        return Err(VaultMountReason::InvalidRequest);
    }
    Ok(MountProfile {
        mount_mode,
        container_kind: broker_container_kind,
        volume_role: broker_volume_role,
        requires_hidden_protection,
    })
}

fn zeroize_mount_secrets(password: &mut String, hidden_protection_password: &mut Option<String>) {
    password.zeroize();
    if let Some(hidden_protection_password) = hidden_protection_password {
        hidden_protection_password.zeroize();
    }
    *hidden_protection_password = None;
}

fn same_mount_owner(active: &ActiveMount, session_id: u32, caller_sid: &str) -> bool {
    active.session_id == session_id && active.caller_sid == caller_sid
}

fn denied(entry_id: &str, reason: VaultMountReason) -> VaultMountResult {
    VaultMountResult {
        entry_id: entry_id.to_owned(),
        state: VaultMountState::Denied,
        presentation: None,
        drive_letter: None,
        reason: Some(reason),
    }
}
fn failed(
    entry_id: &str,
    presentation: Option<VaultPresentation>,
    reason: VaultMountReason,
) -> VaultMountResult {
    VaultMountResult {
        entry_id: entry_id.to_owned(),
        state: VaultMountState::Failed,
        presentation,
        drive_letter: None,
        reason: Some(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("vault_dismount_tests.rs");
    include!("vault_policy_mount_tests.rs");
    use crate::vault_access::{AclApplier, AclSnapshot, PrincipalResolver, VaultAclPlan, VaultFs};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use wincmd_shared::vault_access::VaultAccess;

    struct MountFs {
        files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
        fail_next_active_write: Arc<AtomicBool>,
    }
    impl VaultFs for MountFs {
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        }
        fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
            if path.file_name().and_then(|name| name.to_str())
                == Some("vault-active-mounts-v1.json")
                && self.fail_next_active_write.swap(false, Ordering::SeqCst)
            {
                return Err(std::io::Error::from(std::io::ErrorKind::Other));
            }
            self.files.lock().unwrap().insert(path.into(), bytes.into());
            Ok(())
        }
        fn stable_file_identity(
            &self,
            _: &Path,
        ) -> Result<String, crate::vault_access::VaultError> {
            Ok("v:1:i:2".into())
        }
        fn normalize_personal_creation_path(
            &self,
            path: &Path,
        ) -> Result<PathBuf, crate::vault_access::VaultError> {
            Ok(path.to_path_buf())
        }
        fn personal_creation_target_exists(
            &self,
            path: &Path,
        ) -> Result<bool, crate::vault_access::VaultError> {
            Ok(self.files.lock().unwrap().contains_key(path))
        }
        fn validate_dedicated_parent(
            &self,
            _: &Path,
            _: &Path,
        ) -> Result<(), crate::vault_access::VaultError> {
            Ok(())
        }
    }
    struct MountResolver;
    impl PrincipalResolver for MountResolver {
        fn resolve_sid(&self, _: &str) -> Result<String, crate::vault_access::VaultError> {
            Ok("S-1-5-21-owner".into())
        }
    }
    struct MountAcl;
    impl AclApplier for MountAcl {
        fn apply_and_verify(
            &self,
            _: &VaultAclPlan,
        ) -> Result<(), crate::vault_access::VaultError> {
            Ok(())
        }
        fn snapshot(
            &self,
            _: &VaultAclPlan,
        ) -> Result<Vec<AclSnapshot>, crate::vault_access::VaultError> {
            Ok(vec![])
        }
        fn restore(&self, _: &[AclSnapshot]) -> Result<(), crate::vault_access::VaultError> {
            Ok(())
        }
    }
    #[derive(Default)]
    struct BrokerEvents {
        mounted: usize,
        mount_options: Vec<(VaultPresentation, bool, bool)>,
        personal_acl_repair_sids: Vec<Option<String>>,
        dismounted: Vec<u8>,
        recovered: Vec<u8>,
    }
    struct MountBroker(Arc<Mutex<BrokerEvents>>);
    impl AuthenticatedVaultBroker for MountBroker {
        fn observed_slots(&self) -> Result<HashMap<u8, String>, String> {
            let events = self.0.lock().unwrap();
            Ok(
                if events.dismounted.contains(&12) || events.recovered.contains(&12) {
                    HashMap::new()
                } else {
                    HashMap::from([(12, "test-mount:12".into())])
                },
            )
        }
        fn mount(
            &self,
            request: &mut InternalMountRequest,
        ) -> Result<InternalMountReply, VaultMountReason> {
            let mut events = self.0.lock().unwrap();
            events.mounted += 1;
            events
                .mount_options
                .push((request.presentation, request.read_only, request.personal));
            let broker_plan = broker_mount_args(request)?;
            events.personal_acl_repair_sids.push(
                broker_plan
                    .get("personal_acl_repair_sid")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            );
            Ok(InternalMountReply {
                drive_letter: "P:".into(),
                internal_drive: 12,
                acl_attested: true,
            })
        }
        fn dismount(&self, request: BrokerDismountRequest<'_>) -> Result<(), VaultMountReason> {
            self.0
                .lock()
                .unwrap()
                .dismounted
                .push(request.internal_drive);
            Ok(())
        }
        fn cleanup_orphans(&self) -> Result<(), VaultMountReason> {
            Ok(())
        }
        fn recover_dismount(&self, internal_drive: u8) -> Result<(), VaultMountReason> {
            self.0.lock().unwrap().recovered.push(internal_drive);
            Ok(())
        }
    }

    fn caller_can_list_root(
        _: windows_sys::Win32::Foundation::HANDLE,
        _: &str,
        _: u8,
        _: bool,
    ) -> CallerPresentationAttestation {
        CallerPresentationAttestation::Available
    }

    fn caller_cannot_list_root(
        _: windows_sys::Win32::Foundation::HANDLE,
        _: &str,
        _: u8,
        _: bool,
    ) -> CallerPresentationAttestation {
        CallerPresentationAttestation::RootAccessDenied
    }

    fn caller_has_read_only_access(
        _: windows_sys::Win32::Foundation::HANDLE,
        _: &str,
        _: u8,
        require_write_access: bool,
    ) -> CallerPresentationAttestation {
        if require_write_access {
            CallerPresentationAttestation::RootWriteAccessDenied
        } else {
            CallerPresentationAttestation::Available
        }
    }

    fn caller_cannot_resolve_drive(
        _: windows_sys::Win32::Foundation::HANDLE,
        _: &str,
        _: u8,
        _: bool,
    ) -> CallerPresentationAttestation {
        CallerPresentationAttestation::MappingUnavailable
    }

    fn caller_root_read_has_other_error(
        _: windows_sys::Win32::Foundation::HANDLE,
        _: &str,
        _: u8,
        _: bool,
    ) -> CallerPresentationAttestation {
        CallerPresentationAttestation::RootReadFailed
    }

    struct FailingCleanupBroker {
        acl_attested: bool,
    }
    impl AuthenticatedVaultBroker for FailingCleanupBroker {
        fn mount(
            &self,
            _: &mut InternalMountRequest,
        ) -> Result<InternalMountReply, VaultMountReason> {
            Ok(InternalMountReply {
                drive_letter: "P:".into(),
                internal_drive: 12,
                acl_attested: self.acl_attested,
            })
        }
        fn dismount(&self, _: BrokerDismountRequest<'_>) -> Result<(), VaultMountReason> {
            Err(VaultMountReason::DismountFailed)
        }
        fn cleanup_orphans(&self) -> Result<(), VaultMountReason> {
            Ok(())
        }
        fn recover_dismount(&self, _: u8) -> Result<(), VaultMountReason> {
            Err(VaultMountReason::DismountFailed)
        }
    }

    fn mount_store(
        files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
        fail_next_active_write: Arc<AtomicBool>,
    ) -> VaultAccessStore {
        VaultAccessStore::open(
            Box::new(MountFs {
                files,
                fail_next_active_write,
            }),
            Box::new(MountResolver),
            Box::new(MountAcl),
            PathBuf::from("/policy"),
        )
    }

    fn personal_record() -> PersonalVaultRecord {
        PersonalVaultRecord {
            container_path: "C:\\vaults\\personal.hc".into(),
            container_identity: "v:1:i:2".into(),
            owner_sid: "S-1-5-21-owner".into(),
            scope: VaultPresentation::PerUser,
            created_by_session: 7,
        }
    }

    #[test]
    fn queued_vault_observation_does_not_starve_broker_io_and_deadlines() {
        let broker = Arc::new(VaultMountBroker::with_broker(Box::new(MountBroker(
            Arc::new(Mutex::new(BrokerEvents::default())),
        ))));
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let holder = Arc::clone(&broker);
        let holding_thread = std::thread::spawn(move || {
            holder.with_exclusive_operation(|| {
                held_tx.send(()).unwrap();
                // Bounded escape makes a regression fail instead of hanging the suite.
                let _ = release_rx.recv_timeout(std::time::Duration::from_secs(2));
            });
        });
        held_rx.recv().unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let elapsed = runtime.block_on(async move {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let waiter = tokio::spawn(async move {
                let caller_thread = std::thread::current().id();
                started_tx.send(()).unwrap();
                broker.with_exclusive_operation(|| {
                    // Windows impersonation remains on the original request thread.
                    assert_eq!(caller_thread, std::thread::current().id());
                });
            });
            started_rx.await.unwrap();
            let started = std::time::Instant::now();
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let elapsed = started.elapsed();
            let _ = release_tx.send(());
            waiter.await.unwrap();
            elapsed
        });
        holding_thread.join().unwrap();
        assert!(elapsed < std::time::Duration::from_secs(1), "service timer starved for {elapsed:?}");
    }

    fn personal_request() -> PersonalVaultMountRequest {
        PersonalVaultMountRequest {
            container_path: "C:\\vaults\\personal.hc".into(),
            password: "secret".into(),
            volume_kind: VaultContainerKind::Standard,
            volume_role: VaultVolumeRole::Outer,
            presentation: VaultPresentation::PerUser,
            preferred_letter: Some("P".into()),
            read_only: false,
            pim: None,
            keyfiles: vec![],
            hidden_protection_password: None,
            hidden_keyfiles: vec![],
            hidden_pim: None,
            removable: false,
            repair_current_account_access: false,
        }
    }

    fn active_mount_for_owner(session_id: u32, caller_sid: &str) -> ActiveMount {
        ActiveMount {
            drive_letter: "V:".into(),
            internal_drive: 12,
            presentation: VaultPresentation::Machine,
            session_id,
            caller_sid: caller_sid.into(),
            policy_id: "policy".into(),
            policy_version: 1,
            personal: false,
            syncthing_managed: false,
            container_identity: "identity".into(),
            access: wincmd_shared::vault_access::VaultAccess::Write,
            mounted_at: 1,
            cleanup_required: false,
            engine_mount_identity: Some("test-mount:12".into()),
            canonical_container_path: None,
        }
    }

    pub(crate) fn policy_edit_test_broker(entry_id: &str, container_identity: &str) -> VaultMountBroker {
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(Mutex::new(
            BrokerEvents::default(),
        )))));
        let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
        mount.container_identity = container_identity.into();
        broker.active.lock().unwrap().insert(entry_id.into(), mount);
        broker
    }

    #[test]
    fn root_sddl_is_internal_and_uses_only_resolved_sids() {
        let sddl = mounted_root_acl_sddl(&[ResolvedGrant {
            sid: "S-1-5-21-7".into(),
            access: wincmd_shared::vault_access::VaultAccess::Read,
        }]);
        assert!(sddl.0.starts_with("D:P"));
        assert!(sddl.0.contains("S-1-5-21-7"));
        assert!(sddl.0.contains("0x001200A9;;;S-1-5-21-7"));
        assert!(!sddl.0.contains(";;;BA)"));
        assert_eq!(sddl.0.matches(";OICI;").count(), 2);
    }

    #[test]
    fn root_sddl_keeps_owner_write_and_viewer_read_without_a_broad_admin_grant() {
        let sddl = mounted_root_acl_sddl(&[
            ResolvedGrant {
                sid: "S-1-5-21-1001".into(),
                access: wincmd_shared::vault_access::VaultAccess::Write,
            },
            ResolvedGrant {
                sid: "S-1-5-21-1002".into(),
                access: wincmd_shared::vault_access::VaultAccess::Read,
            },
        ]);
        assert!(sddl.0.contains("0x001301BF;;;S-1-5-21-1001"));
        assert!(sddl.0.contains("0x001200A9;;;S-1-5-21-1002"));
        assert!(!sddl.0.contains(";;;BA)"));
    }

    #[test]
    fn a_viewer_mount_does_not_make_the_shared_device_read_only_for_the_owner() {
        assert!(!SHARED_VAULT_DEVICE_READ_ONLY);
    }
    #[test]
    fn broker_drive_reply_is_bounded() {
        assert!(valid_drive_letter("V"));
        assert!(valid_drive_letter("V:"));
        assert!(!valid_drive_letter("V:\\private"));
    }

    #[test]
    fn personal_mount_public_codes_remain_bounded() {
        for (reason, expected) in [
            (VaultMountReason::NotAuthorized, "vault_not_authorized"),
            (VaultMountReason::ProNotInstalled, "vault_pro_not_installed"),
            (
                VaultMountReason::SessionUnavailable,
                "vault_session_unavailable",
            ),
            (
                VaultMountReason::EngineUnlockFailed,
                "vault_engine_unlock_failed",
            ),
            (
                VaultMountReason::EngineDriveLetterUnavailable,
                "vault_engine_drive_letter_unavailable",
            ),
            (
                VaultMountReason::EngineMountFailed,
                "vault_engine_mount_failed",
            ),
            (
                VaultMountReason::CallerAccessDenied,
                "vault_caller_access_denied",
            ),
            (
                VaultMountReason::CallerAclRepairFailed,
                "vault_caller_acl_repair_failed",
            ),
            (
                VaultMountReason::BrokerUnavailable,
                "vault_broker_unavailable",
            ),
            (VaultMountReason::BrokerRejected, "vault_broker_rejected"),
            (VaultMountReason::DismountFailed, "vault_cleanup_failed"),
        ] {
            assert_eq!(
                VaultMountBroker::personal_mount_failure_code(reason),
                expected
            );
        }
    }

    #[test]
    fn unmanaged_mount_key_is_shared_by_users_of_the_same_container() {
        let first = personal_record();
        let mut second = first.clone();
        second.owner_sid = "S-1-5-21-other".into();

        assert_ne!(
            personal_mount_entry_id(&first),
            personal_mount_entry_id(&second)
        );
        assert_eq!(
            unmanaged_mount_entry_id(&first),
            unmanaged_mount_entry_id(&second)
        );
    }

    #[test]
    fn personal_mount_projection_is_owner_scoped_even_for_machine_letters() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(
            Mutex::new(BrokerEvents::default()),
        ))));
        broker.engine_snapshot = Some(|| {
            Ok(HashMap::from([
                (12, "test-mount:12".into()),
                (13, "test-mount:13".into()),
                (14, "test-mount:14".into()),
            ]))
        });
        let mut shared = active_mount_for_owner(7, "S-1-5-21-owner");
        shared.personal = true;
        shared.cleanup_required = true;
        let mut private = shared.clone();
        private.presentation = VaultPresentation::PerUser;
        private.internal_drive = 13;
        private.engine_mount_identity = Some("test-mount:13".into());
        let mut managed = shared.clone();
        managed.personal = false;
        managed.policy_id = "personal".into();
        managed.internal_drive = 14;
        managed.engine_mount_identity = Some("test-mount:14".into());
        broker.active.lock().unwrap().extend([
            ("unmanaged-shared".into(), shared),
            ("personal-legacy".into(), private),
            ("managed".into(), managed),
        ]);

        let other = broker
            .personal_mounts_for_caller(&store, std::ptr::null_mut(), 8, "S-1-5-21-other", true)
            .unwrap();
        assert!(other.is_empty());
        let owner = broker
            .personal_mounts_for_caller(&store, std::ptr::null_mut(), 7, "S-1-5-21-owner", false)
            .unwrap();
        assert_eq!(owner.len(), 2);
        assert!(owner.iter().all(|mount| mount.dismount_allowed));
        assert!(owner.iter().all(|mount| mount.cleanup_required));
        assert!(broker
            .personal_mounts_for_caller(&store, std::ptr::null_mut(), 8, "S-1-5-21-owner", true)
            .unwrap()
            .is_empty());
        assert!(broker
            .personal_mounts_for_caller(&store, std::ptr::null_mut(), 0, "S-1-5-21-owner", true)
            .is_err());
        broker.mark_registry_untrusted();
        assert_eq!(
            broker.personal_mounts_for_caller(
                &store,
                std::ptr::null_mut(),
                7,
                "S-1-5-21-owner",
                true
            ),
            Err(VaultMountReason::MountStateUnknown)
        );
    }

    #[test]
    fn legacy_private_request_cannot_be_exposed_by_a_machine_scoped_record() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        let mut record = personal_record();
        record.scope = VaultPresentation::Machine;
        let mut request = personal_request();
        assert_eq!(request.presentation, VaultPresentation::PerUser);
        assert_eq!(
            broker.mount_unmanaged_authorized_locked(
                41,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                "S-1-5-21-owner",
                (0, 0),
            ),
            Err(VaultMountReason::NotAuthorized)
        );
        assert_eq!(events.lock().unwrap().mounted, 0);
    }

    #[test]
    fn unmanaged_machine_mount_preserves_requested_write_mode_and_existing_permissions() {
        for read_only in [false, true] {
            let store = mount_store(
                Arc::new(Mutex::new(HashMap::new())),
                Arc::new(AtomicBool::new(false)),
            );
            let events = Arc::new(Mutex::new(BrokerEvents::default()));
            let broker = VaultMountBroker::with_broker_and_attestor(
                Box::new(MountBroker(events.clone())),
                caller_can_list_root,
            );
            let mut record = personal_record();
            record.scope = VaultPresentation::Machine;
            let mut request = personal_request();
            request.read_only = read_only;
            request.presentation = VaultPresentation::Machine;

            broker
                .mount_unmanaged_authorized_locked(
                    41,
                    &store,
                    &record,
                    &mut request,
                    std::ptr::null_mut(),
                    7,
                    "S-1-5-21-owner",
                    (0, 0),
                )
                .expect("an unmanaged mount uses the service-selected machine scope");

            assert_eq!(
                events.lock().unwrap().mount_options,
                vec![(VaultPresentation::Machine, read_only, true)]
            );
            let active = broker.active.lock().unwrap();
            let mount = active.get(&unmanaged_mount_entry_id(&record)).unwrap();
            assert_eq!(mount.presentation, VaultPresentation::Machine);
            assert_eq!(
                mount.access,
                if read_only {
                    VaultAccess::Read
                } else {
                    VaultAccess::Write
                }
            );
        }
    }

    #[test]
    fn unmanaged_machine_mount_succeeds_only_after_authenticated_caller_root_readback() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_can_list_root,
        );
        let mut record = personal_record();
        record.scope = VaultPresentation::Machine;
        let mut request = personal_request();
        request.presentation = VaultPresentation::Machine;

        let result = broker.mount_unmanaged_authorized_locked(
            41,
            &store,
            &record,
            &mut request,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-owner",
            (0, 0),
        );

        assert_eq!(result, Ok(("P:".into(), 12, true)));
        assert!(events.lock().unwrap().dismounted.is_empty());
        assert_eq!(
            events.lock().unwrap().personal_acl_repair_sids,
            vec![None],
            "ordinary mounts never request permission changes"
        );
        assert_eq!(
            broker.projection(&unmanaged_mount_entry_id(&record)).0,
            VaultMountState::Mounted
        );
    }

    #[test]
    fn retired_account_repair_is_rejected_without_starting_a_mount() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_can_list_root,
        );
        let mut record = personal_record();
        record.scope = VaultPresentation::Machine;
        record.owner_sid = "S-1-5-21-1111-2222".into();
        let mut request = personal_request();
        request.presentation = VaultPresentation::Machine;
        request.repair_current_account_access = true;

        let result = broker.mount_unmanaged_authorized_locked(
            41,
            &store,
            &record,
            &mut request,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-1111-2222",
            (0, 0),
        );

        assert_eq!(result, Err(VaultMountReason::InvalidRequest));
        assert_eq!(events.lock().unwrap().mounted, 0);
        assert!(events.lock().unwrap().personal_acl_repair_sids.is_empty());
        assert!(request.password.is_empty());
    }

    #[test]
    fn unmanaged_machine_mount_rolls_back_the_exact_slot_when_caller_root_is_denied() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_cannot_list_root,
        );
        let mut record = personal_record();
        record.scope = VaultPresentation::Machine;
        let entry_id = unmanaged_mount_entry_id(&record);
        let mut request = personal_request();
        request.presentation = VaultPresentation::Machine;

        let result = broker.mount_unmanaged_authorized_locked(
            41,
            &store,
            &record,
            &mut request,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-owner",
            (0, 0),
        );

        assert_eq!(result, Err(VaultMountReason::CallerAccessDenied));
        assert_eq!(events.lock().unwrap().dismounted, vec![12]);
        assert_eq!(broker.projection(&entry_id).0, VaultMountState::Unmounted);
    }

    #[test]
    fn unmanaged_writable_mount_keeps_existing_root_write_restrictions() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_has_read_only_access,
        );
        let mut record = personal_record();
        record.scope = VaultPresentation::Machine;
        let mut writable = personal_request();
        writable.presentation = VaultPresentation::Machine;

        let result = broker.mount_unmanaged_authorized_locked(
            41,
            &store,
            &record,
            &mut writable,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-owner",
            (0, 0),
        );
        assert_eq!(result, Ok(("P:".into(), 12, true)));
        assert!(events.lock().unwrap().dismounted.is_empty());
        let active = broker.active.lock().unwrap();
        let mounted = active.get(&unmanaged_mount_entry_id(&record)).unwrap();
        assert_eq!(mounted.access, wincmd_shared::vault_access::VaultAccess::Write);
        assert_eq!(
            broker.attest_existing_mount(mounted, std::ptr::null_mut()),
            Ok(())
        );
        let mut managed = mounted.clone();
        managed.personal = false;
        assert_eq!(
            broker.attest_existing_mount(&managed, std::ptr::null_mut()),
            Err(VaultMountReason::CallerAccessDenied),
            "Fleet write grants must still attest write access"
        );
        drop(active);

        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_has_read_only_access,
        );
        let mut read_only = personal_request();
        read_only.presentation = VaultPresentation::Machine;
        read_only.read_only = true;
        let result = broker.mount_unmanaged_authorized_locked(
            42,
            &store,
            &record,
            &mut read_only,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-owner",
            (0, 0),
        );
        assert_eq!(result, Ok(("P:".into(), 12, true)));
        assert!(events.lock().unwrap().dismounted.is_empty());
    }

    #[test]
    fn unmanaged_machine_mount_reports_mapping_failure_separately_and_rolls_back() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_cannot_resolve_drive,
        );
        let mut record = personal_record();
        record.scope = VaultPresentation::Machine;
        let mut request = personal_request();
        request.presentation = VaultPresentation::Machine;

        let result = broker.mount_unmanaged_authorized_locked(
            41,
            &store,
            &record,
            &mut request,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-owner",
            (0, 0),
        );

        assert_eq!(result, Err(VaultMountReason::PresentationRejected));
        assert_eq!(events.lock().unwrap().dismounted, vec![12]);
    }

    #[test]
    fn unmanaged_machine_mount_does_not_call_other_root_errors_acl_denials() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_root_read_has_other_error,
        );
        let mut record = personal_record();
        record.scope = VaultPresentation::Machine;
        let mut request = personal_request();
        request.presentation = VaultPresentation::Machine;

        let result = broker.mount_unmanaged_authorized_locked(
            41,
            &store,
            &record,
            &mut request,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-owner",
            (0, 0),
        );

        assert_eq!(result, Err(VaultMountReason::PresentationRejected));
        assert_eq!(events.lock().unwrap().dismounted, vec![12]);
    }

    #[test]
    fn unmanaged_machine_mount_retains_cleanup_record_if_denied_readback_cannot_dismount() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(FailingCleanupBroker {
                acl_attested: false,
            }),
            caller_cannot_list_root,
        );
        let mut record = personal_record();
        record.scope = VaultPresentation::Machine;
        let entry_id = unmanaged_mount_entry_id(&record);
        let mut request = personal_request();
        request.presentation = VaultPresentation::Machine;

        let result = broker.mount_unmanaged_authorized_locked(
            41,
            &store,
            &record,
            &mut request,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-owner",
            (0, 0),
        );

        assert_eq!(result, Err(VaultMountReason::DismountFailed));
        let active = broker.active.lock().unwrap();
        let mount = active.get(&entry_id).expect("failed cleanup is tracked");
        assert_eq!(mount.internal_drive, 12);
        assert!(mount.cleanup_required);
        drop(active);
        let durable: DurableMountRegistry =
            serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
        let mount = durable.mounts.get(&entry_id).expect("slot is recoverable");
        assert_eq!(mount.internal_drive, 12);
        assert!(mount.cleanup_required);
    }

    #[test]
    fn duplicate_mounts_never_claim_a_new_read_only_or_hidden_mode_was_applied() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_can_list_root,
        );
        let record = personal_record();
        for operation_id in [41, 42, 43, 44] {
            let mut request = personal_request();
            if operation_id == 43 {
                request.read_only = true;
            }
            if operation_id == 44 {
                request.volume_kind = VaultContainerKind::Dual;
                request.volume_role = VaultVolumeRole::Hidden;
            }
            let result = broker.mount_personal_authorized(
                operation_id,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                &record.owner_sid,
                (0, 0),
            );
            if operation_id == 41 {
                assert_eq!(result, Ok(("P:".into(), 12, true)));
            } else {
                assert_eq!(result, Err(VaultMountReason::AlreadyMounted));
            }
            assert!(request.password.is_empty());
        }
        let events = events.lock().unwrap();
        assert_eq!(events.mounted, 1);
        assert!(events.dismounted.is_empty());
    }

    #[test]
    fn explicit_letter_rejects_foreign_session_mapping_before_engine_dispatch() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.drive_letter_probe = || Ok(HashSet::from(["P".into()]));
        let record = personal_record();
        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                41,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                &record.owner_sid,
                (0, 0)
            ),
            Err(VaultMountReason::EngineDriveLetterUnavailable)
        );
        assert!(request.password.is_empty());
        assert_eq!(events.lock().unwrap().mounted, 0);
    }

    #[test]
    fn duplicate_mount_with_a_missing_mapping_fails_without_replacing_the_volume() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_cannot_resolve_drive,
        );
        let record = personal_record();
        let mut existing = active_mount_for_owner(7, &record.owner_sid);
        existing.drive_letter = "P:".into();
        broker
            .active
            .lock()
            .unwrap()
            .insert(personal_mount_entry_id(&record), existing);
        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                41,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                &record.owner_sid,
                (0, 0)
            ),
            Err(VaultMountReason::PresentationRejected)
        );
        assert!(request.password.is_empty());
        let events = events.lock().unwrap();
        assert_eq!(events.mounted, 0);
        assert!(events.dismounted.is_empty());
    }

    #[test]
    fn duplicate_mount_from_a_different_session_is_denied_without_disruption() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_can_list_root,
        );
        let record = personal_record();
        broker.active.lock().unwrap().insert(
            personal_mount_entry_id(&record),
            active_mount_for_owner(9, &record.owner_sid),
        );
        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                41,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                &record.owner_sid,
                (0, 0)
            ),
            Err(VaultMountReason::NotAuthorized)
        );
        assert!(request.password.is_empty());
        let events = events.lock().unwrap();
        assert_eq!(events.mounted, 0);
        assert!(events.dismounted.is_empty());
    }

    #[test]
    fn unavailable_drive_inventory_fails_closed_before_engine_dispatch() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.drive_letter_probe = || Err(());
        let record = personal_record();
        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                41,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                &record.owner_sid,
                (0, 0)
            ),
            Err(VaultMountReason::EngineDriveLetterUnavailable)
        );
        assert!(request.password.is_empty());
        assert_eq!(events.lock().unwrap().mounted, 0);
    }

    #[test]
    fn another_private_mount_reserves_its_letter_machine_wide() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(Mutex::new(
            BrokerEvents::default(),
        )))));
        broker.active.lock().unwrap().insert(
            "foreign-private".into(),
            active_mount_for_owner(99, "another-sid"),
        );
        assert_eq!(
            broker.select_mount_letter(&store, Some("v"), "another-container"),
            Err(VaultMountReason::EngineDriveLetterUnavailable)
        );
        assert_eq!(
            broker.unavailable_letters_locked(&store, None).unwrap(),
            vec!["V"]
        );
        assert_eq!(
            broker
                .select_mount_letter(&store, None, "another-container")
                .unwrap(),
            "Z"
        );
    }

    #[test]
    fn personal_mount_uses_durable_registry_and_session_cleanup() {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let store = mount_store(files, Arc::new(AtomicBool::new(false)));
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        let record = personal_record();
        let entry_id = personal_mount_entry_id(&record);
        let mut request = personal_request();

        assert_eq!(
            broker.mount_personal_authorized(
                41,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                "S-1-5-21-owner",
                (0, 0),
            ),
            Ok(("P:".into(), 12, true))
        );
        assert_eq!(broker.projection(&entry_id).0, VaultMountState::Mounted);
        assert_eq!(
            events.lock().unwrap().mount_options,
            vec![(VaultPresentation::PerUser, false, true)]
        );
        assert!(store
            .read_active_mounts()
            .unwrap()
            .windows(8)
            .any(|window| window == b"personal"));

        broker.dismount_session(&store, 7);
        assert_eq!(broker.projection(&entry_id).0, VaultMountState::Unmounted);
        assert_eq!(events.lock().unwrap().dismounted, vec![12]);
    }

    #[test]
    fn ordinary_mount_cannot_take_a_saved_letter_and_owner_identity_is_still_checked() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let policy = serde_json::from_value(serde_json::json!({
            "schema_version": 1, "policy_id": "reserved", "version": 1, "expected_previous_version": 0,
            "entries": [{ "id": "reserved", "label": "Reserved", "container_path": "C:\\vaults\\reserved.hc",
                "owner_account": "Owner", "grants": [{"principal_name": "Owner", "access": "write"}],
                "mount": {"presentation": "per-user", "preferred_letter": "P"} }]
        })).unwrap();
        store.apply(policy, 1).unwrap();
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        assert_eq!(
            broker.unavailable_letters_locked(&store, None).unwrap(),
            vec!["P"]
        );
        assert!(broker
            .unavailable_letters_locked(&store, Some("reserved"))
            .unwrap()
            .is_empty());
        let mut record = personal_record();
        record.container_identity = "another-file".into();
        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                41,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                &record.owner_sid,
                (0, 0),
            ),
            Err(VaultMountReason::EngineDriveLetterUnavailable)
        );
        assert!(request.password.is_empty());
        assert_eq!(
            events.lock().unwrap().mounted,
            0,
            "reserved request must never reach the engine"
        );

        record.container_identity = "v:1:i:2".into();
        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                42,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                "another-user",
                (0, 0),
            ),
            Err(VaultMountReason::NotAuthorized)
        );
        assert_eq!(events.lock().unwrap().mounted, 0);

        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                43,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                &record.owner_sid,
                (0, 0),
            ),
            Ok(("P:".into(), 12, true))
        );
        assert_eq!(events.lock().unwrap().mounted, 1);
        assert_eq!(
            broker
                .unavailable_letters_locked(&store, Some("reserved"))
                .unwrap(),
            vec!["P"],
            "excluding a saved reservation cannot hide a live occupied letter"
        );
    }

    #[test]
    fn personal_force_dismount_removes_the_active_record_after_closing_its_exact_slot() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker_and_attestor(
            Box::new(MountBroker(events.clone())),
            caller_can_list_root,
        );
        let mut record = personal_record();
        record.scope = VaultPresentation::Machine;
        let entry_id = unmanaged_mount_entry_id(&record);
        let mut request = personal_request();
        request.presentation = VaultPresentation::Machine;
        broker
            .mount_unmanaged_authorized_locked(
                41,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                "S-1-5-21-owner",
                (0, 0),
            )
            .expect("personal mount");

        let denied = broker.dismount_personal_for_caller(
            &store,
            42,
            12,
            std::ptr::null_mut(),
            11,
            "S-1-5-21-different-user",
            false,
        );
        assert_eq!(denied.state, VaultMountState::Denied);
        assert_eq!(broker.projection(&entry_id).0, VaultMountState::Mounted);

        let result = broker.dismount_personal_for_caller(
            &store,
            43,
            12,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-owner",
            false,
        );
        assert_eq!(result.state, VaultMountState::Unmounted);
        assert_eq!(broker.projection(&entry_id).0, VaultMountState::Unmounted);
        assert_eq!(events.lock().unwrap().dismounted, vec![12]);
    }

    #[test]
    fn elevated_authorized_fleet_member_can_dismount_another_sessions_shared_mount() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.policy_authorizer =
            |_, _, _| wincmd_shared::vault_access::VaultAuthorizeMountResponse {
                allowed: true,
                launch_ready: true,
                denial_reason: None,
                mode: Some(wincmd_shared::vault_access::VaultAccess::Read),
                presentation: Some(VaultPresentation::Machine),
                preferred_letter: None,
            };
        let entry_id = "fleet-shared";
        assert!(broker.retain_cleanup_mount(
            &store,
            entry_id,
            active_mount_for_owner(7, "S-1-5-21-owner"),
        ));

        let result = broker.dismount_authorized(
            &store,
            AuthorizedDismount {
                operation_id: 43,
                entry_id,
                caller_token: std::ptr::null_mut(),
                caller_session: 11,
                caller_sid: "S-1-5-21-authorized-member",
                caller_elevated: true,
            },
        );
        assert_eq!(result.state, VaultMountState::Unmounted);
        assert_eq!(broker.projection(entry_id).0, VaultMountState::Unmounted);
        assert_eq!(events.lock().unwrap().recovered, vec![12]);
    }

    #[test]
    fn outsider_administrator_cannot_dismount_group_mount_or_clear_policy_mount_guard() {
        let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        let entry_id = "fleet-shared";
        assert!(broker.retain_cleanup_mount(&store, entry_id, active_mount_for_owner(7, "S-1-5-21-owner")));
        assert!(broker.with_exclusive_operation(|| broker.has_active_mounts_locked()));
        let result = broker.dismount_authorized(&store, AuthorizedDismount {
            operation_id: 43, entry_id, caller_token: std::ptr::null_mut(), caller_session: 11,
            caller_sid: "S-1-5-21-outsider-admin", caller_elevated: true,
        });
        assert_eq!(result.state, VaultMountState::Denied);
        assert_eq!(result.reason, Some(VaultMountReason::PolicyAccessDenied));
        assert!(broker.with_exclusive_operation(|| broker.has_active_mounts_locked()));
        assert!(events.lock().unwrap().recovered.is_empty());
        assert!(events.lock().unwrap().dismounted.is_empty());
    }

    #[test]
    fn unknown_mount_registry_blocks_policy_edits_and_deletions() {
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(Mutex::new(BrokerEvents::default())))));
        assert!(!broker.with_exclusive_operation(|| broker.has_active_mounts_locked()));
        broker.recovery.lock().unwrap().registry_untrusted = true;
        assert!(broker.with_exclusive_operation(|| broker.has_active_mounts_locked()));
    }

    #[test]
    fn personal_mount_recovers_after_service_restart() {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let store = mount_store(files, Arc::new(AtomicBool::new(false)));
        let initial_events = Arc::new(Mutex::new(BrokerEvents::default()));
        let initial = VaultMountBroker::with_broker(Box::new(MountBroker(initial_events)));
        let record = personal_record();
        let mut request = personal_request();
        initial
            .mount_personal_authorized(
                42,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                "S-1-5-21-owner",
                (0, 0),
            )
            .unwrap();

        let recovery_events = Arc::new(Mutex::new(BrokerEvents::default()));
        let restarted =
            VaultMountBroker::with_broker(Box::new(MountBroker(recovery_events.clone())));
        let recovered = restarted.load_and_cleanup(&store).unwrap();
        assert!(recovered.contains("v:1:i:2"));
        assert_eq!(recovery_events.lock().unwrap().recovered, vec![12]);
        assert!(store
            .read_active_mounts()
            .unwrap()
            .windows(11)
            .any(|window| window == b"\"mounts\":{}"));
    }

    #[test]
    fn personal_mount_rolls_back_when_active_registry_write_fails() {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let fail_active_write = Arc::new(AtomicBool::new(true));
        let store = mount_store(files, fail_active_write);
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        let record = personal_record();
        let mut request = personal_request();

        assert_eq!(
            broker.mount_personal_authorized(
                43,
                &store,
                &record,
                &mut request,
                std::ptr::null_mut(),
                7,
                "S-1-5-21-owner",
                (0, 0),
            ),
            Err(VaultMountReason::BrokerRejected)
        );
        assert_eq!(events.lock().unwrap().dismounted, vec![12]);
    }

    #[test]
    fn personal_mount_rejects_when_durable_registry_is_at_capacity() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        {
            let mut active = broker.active.lock().unwrap();
            for index in 0..MAX_DURABLE_MOUNTS {
                active.insert(
                    format!("slot-{index}"),
                    active_mount_for_owner(7, "S-1-5-21-owner"),
                );
            }
        }
        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                46,
                &store,
                &personal_record(),
                &mut request,
                std::ptr::null_mut(),
                7,
                "S-1-5-21-owner",
                (0, 0),
            ),
            Err(VaultMountReason::BrokerRejected)
        );
        assert_eq!(events.lock().unwrap().mounted, 0);
    }

    #[test]
    fn failed_cleanup_record_removal_retries_the_post_dismount_map() {
        let fail_active_write = Arc::new(AtomicBool::new(false));
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::clone(&fail_active_write),
        );
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(Mutex::new(
            BrokerEvents::default(),
        )))));
        let entry_id = "personal-cleanup";
        let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
        mount.cleanup_required = true;
        assert!(broker.retain_cleanup_mount(&store, entry_id, mount));

        fail_active_write.store(true, Ordering::SeqCst);
        assert!(!broker.clear_retained_mount(&store, entry_id));
        assert!(broker.recovery_allows_entry(entry_id, &store));
        assert!(!broker.active.lock().unwrap().contains_key(entry_id));
        let registry: DurableMountRegistry =
            serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
        assert!(!registry.mounts.contains_key(entry_id));
    }

    #[test]
    fn personal_mount_accepts_an_honest_unattested_filesystem_acl() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let broker = VaultMountBroker::with_broker(Box::new(FailingCleanupBroker {
            acl_attested: false,
        }));
        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                44,
                &store,
                &personal_record(),
                &mut request,
                std::ptr::null_mut(),
                7,
                "S-1-5-21-owner",
                (0, 0),
            ),
            Ok(("P:".into(), 12, false))
        );
        let registry: DurableMountRegistry =
            serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
        assert!(registry
            .mounts
            .values()
            .all(|mount| !mount.cleanup_required));
    }

    #[test]
    fn personal_mount_reports_cleanup_uncertain_when_registry_failure_cannot_dismount() {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(true)),
        );
        let broker =
            VaultMountBroker::with_broker(Box::new(FailingCleanupBroker { acl_attested: true }));
        let mut request = personal_request();
        assert_eq!(
            broker.mount_personal_authorized(
                45,
                &store,
                &personal_record(),
                &mut request,
                std::ptr::null_mut(),
                7,
                "S-1-5-21-owner",
                (0, 0),
            ),
            Err(VaultMountReason::DismountFailed)
        );
        let registry: DurableMountRegistry =
            serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
        assert!(registry.mounts.values().all(|mount| mount.cleanup_required));
    }

    #[test]
    fn broker_dismount_carries_only_the_service_owned_presented_letter() {
        assert_eq!(
            broker_dismount_args(12, Some("v:")),
            serde_json::json!({"internal_drive": 12, "presented_drive_letter": "V:"})
        );
        assert_eq!(
            broker_dismount_args(12, Some("v")),
            serde_json::json!({"internal_drive": 12, "presented_drive_letter": "V:"})
        );
        assert_eq!(
            broker_dismount_args(12, None),
            serde_json::json!({"internal_drive": 12})
        );
        assert_eq!(
            broker_dismount_args(12, Some("V:\\untrusted")),
            serde_json::json!({"internal_drive": 12})
        );
        assert_eq!(
            per_user_presented_drive_letter(VaultPresentation::PerUser, "V:"),
            Some("V:")
        );
        assert_eq!(
            per_user_presented_drive_letter(VaultPresentation::Machine, "V:"),
            None
        );
    }

    #[test]
    fn dual_hidden_mount_routes_to_the_hidden_engine_mode() {
        assert_eq!(
            mount_profile(
                VaultContainerKind::Dual,
                VaultVolumeRole::Hidden,
                wincmd_shared::vault_access::VaultAccess::Write,
                None,
            ),
            Ok(MountProfile {
                mount_mode: "hidden",
                container_kind: "dual",
                volume_role: "hidden",
                requires_hidden_protection: false,
            })
        );
    }

    #[test]
    fn writable_dual_outer_mount_requires_and_forces_inner_protection() {
        assert_eq!(
            mount_profile(
                VaultContainerKind::Dual,
                VaultVolumeRole::Outer,
                wincmd_shared::vault_access::VaultAccess::Write,
                None,
            ),
            Err(VaultMountReason::InvalidRequest)
        );
        let profile = mount_profile(
            VaultContainerKind::Dual,
            VaultVolumeRole::Outer,
            wincmd_shared::vault_access::VaultAccess::Write,
            Some("hidden-secret"),
        )
        .unwrap();
        assert_eq!(profile.mount_mode, "standard");
        assert_eq!(profile.container_kind, "dual");
        assert_eq!(profile.volume_role, "outer");
        assert!(profile.requires_hidden_protection);
    }

    #[test]
    fn standard_entries_keep_the_legacy_outer_mode_and_reject_hidden_role() {
        assert_eq!(
            mount_profile(
                VaultContainerKind::Standard,
                VaultVolumeRole::Outer,
                wincmd_shared::vault_access::VaultAccess::Write,
                None,
            ),
            Ok(MountProfile {
                mount_mode: "standard",
                container_kind: "standard",
                volume_role: "standard",
                requires_hidden_protection: false,
            })
        );
        assert_eq!(
            mount_profile(
                VaultContainerKind::Standard,
                VaultVolumeRole::Hidden,
                wincmd_shared::vault_access::VaultAccess::Write,
                None,
            ),
            Err(VaultMountReason::InvalidRequest)
        );
    }

    #[test]
    fn broker_uses_exact_private_contract_tuples() {
        for (mount_mode, container_kind, volume_role, hidden_protection_password) in [
            ("standard", "standard", "standard", None),
            ("standard", "dual", "outer", Some("hidden-secret")),
            ("hidden", "dual", "hidden", None),
        ] {
            let mut request = InternalMountRequest {
                operation_id: 41,
                container_path: "C:\\vaults\\dual.hc".into(),
                mount_mode,
                volume_kind: container_kind,
                volume_role,
                read_only: false,
                personal: false,
                pim: None,
                keyfiles: Vec::new(),
                hidden_keyfiles: Vec::new(),
                hidden_pim: None,
                removable: false,
                presentation: VaultPresentation::Machine,
                preferred_letter: Some("V".into()),
                target_session_id: 7,
                caller_sid: "S-1-5-21-test".into(),
                caller_token: std::ptr::null_mut(),
                caller_authentication_id: (0, 0),
                mounted_root_acl_sddl: MountedRootAclSddl("D:P".into()),
                password: "outer-secret".into(),
                hidden_protection_password: hidden_protection_password.map(str::to_owned),
            };
            let args = broker_mount_args(&mut request).unwrap();
            assert_eq!(args["mount_mode"], mount_mode);
            assert_eq!(args["volume_kind"], container_kind);
            assert_eq!(args["volume_role"], volume_role);
            assert_eq!(args["personal"], false);
            assert_eq!(args.get("protect_inner"), None);
            assert_eq!(args.get("client_pid"), None);
            assert_eq!(
                args.get("hidden_protection_password")
                    .and_then(serde_json::Value::as_str),
                hidden_protection_password,
            );
        }
    }

    #[test]
    fn private_session_identity_requires_the_exact_mounter() {
        let mount = active_mount_for_owner(4, "S-1-5-21-owner");
        assert!(same_mount_owner(&mount, 4, "S-1-5-21-owner"));
        assert!(!same_mount_owner(&mount, 5, "S-1-5-21-owner"));
        assert!(!same_mount_owner(&mount, 4, "S-1-5-21-other"));
    }

    #[test]
    fn enrollment_exposes_only_a_loopback_syncthing_gui_url() {
        assert!(valid_syncthing_gui_url("http://127.0.0.1:8385"));
        assert!(!valid_syncthing_gui_url("https://127.0.0.1:8385"));
        assert!(!valid_syncthing_gui_url("http://localhost:8385"));
        assert!(!valid_syncthing_gui_url("http://192.168.1.10:8385"));
        assert!(!valid_syncthing_gui_url("http://127.0.0.1:0"));
    }

    #[test]
    fn scoped_persistence_failure_does_not_make_the_registry_untrusted() {
        let mut recovery = RecoveryState::default();
        recovery.persistence_pending.insert("sales".into());
        assert!(!recovery.registry_untrusted);
        assert!(recovery.persistence_pending.contains("sales"));
        recovery.persistence_pending.clear();
        assert!(recovery.persistence_pending.is_empty());
    }
}
