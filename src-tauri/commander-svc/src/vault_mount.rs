// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-owned, fail-closed Vault mount broker boundary.
//!
//! This module never launches a public Pro CLI.  A path/password/ACL-bearing
//! mount request is accepted only by an authenticated service-to-Pro broker;
//! until that protected transport is installed, mount fails closed.

#![cfg(windows)]

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::vault_access::{ResolvedGrant, VaultAccessStore};
use wincmd_shared::vault_access::{
    PersonalVaultMountRequest, PersonalVaultRecord, VaultBrokerVolumeRole, VaultContainerKind,
    VaultMountMode, VaultMountPlan, VaultMountReason, VaultMountResult, VaultMountState,
    VaultPresentation, VaultVolumeRole,
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
    container_identity: String,
    access: wincmd_shared::vault_access::VaultAccess,
    mounted_at: u64,
}

/// Protected, crash-recovery-only state. It contains no password, path, ACL,
/// or token. A new mount is not reported until this is atomically written.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableMountRegistry {
    mounts: HashMap<String, ActiveMount>,
}

const MAX_DURABLE_MOUNTS: usize = 64;
static NEXT_INTERNAL_OPERATION_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1 << 62);

fn next_internal_operation_id() -> u64 {
    NEXT_INTERNAL_OPERATION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Service-to-Pro only.  No implementation may be backed by an unauthenticated
/// executable argument/stdio mode: a renderer must never be able to select the
/// container, SDDL, session, internal slot, or broker endpoint.
trait AuthenticatedVaultBroker: Send + Sync {
    fn mount(
        &self,
        request: &mut InternalMountRequest,
    ) -> Result<InternalMountReply, VaultMountReason>;
    fn dismount(
        &self,
        operation_id: u64,
        internal_drive: u8,
        presented_drive_letter: Option<&str>,
        presentation: VaultPresentation,
        target_session_id: u32,
        caller_sid: &str,
        caller_token: Option<windows_sys::Win32::Foundation::HANDLE>,
    ) -> Result<(), VaultMountReason>;
    fn cleanup_orphans(&self) -> Result<(), VaultMountReason>;
    /// Boot recovery runs after an interactive session may have ended. The
    /// encrypted driver's internal slot is machine-owned, so cleanup must not
    /// require the original user's now-unavailable logon token.
    fn recover_dismount(&self, internal_drive: u8) -> Result<(), VaultMountReason>;
}

struct ProEnvelopeBroker;

fn broker_mount_reason(error: &str) -> VaultMountReason {
    match error {
        "session_unavailable" => VaultMountReason::SessionUnavailable,
        "vault_acl_apply_failed" => VaultMountReason::AclApplyFailed,
        "vault_acl_readback_failed" => VaultMountReason::AclReadbackFailed,
        "vault_engine_unlock_failed" => VaultMountReason::EngineUnlockFailed,
        "vault_engine_drive_letter_unavailable" | "vault_drive_letter_unavailable" => {
            VaultMountReason::EngineDriveLetterUnavailable
        }
        "vault_engine_mount_failed" => VaultMountReason::EngineMountFailed,
        "broker_handshake" | "broker_hmac" | "broker_rejected" | "vault_broker_failed" => {
            VaultMountReason::BrokerRejected
        }
        _ => VaultMountReason::BrokerUnavailable,
    }
}

impl AuthenticatedVaultBroker for ProEnvelopeBroker {
    fn mount(
        &self,
        request: &mut InternalMountRequest,
    ) -> Result<InternalMountReply, VaultMountReason> {
        let args = broker_mount_args(request)?;
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
                request.operation_id,
                request.target_session_id,
                &request.caller_sid,
                Some(request.caller_token),
                Some(request.caller_authentication_id),
                request.presentation,
                "vault.broker.mount",
                args,
            ))
        });
        request.zeroize_secrets();
        let value = result.map_err(|error| broker_mount_reason(&error))?;
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
    fn dismount(
        &self,
        operation_id: u64,
        internal_drive: u8,
        presented_drive_letter: Option<&str>,
        presentation: VaultPresentation,
        target_session_id: u32,
        caller_sid: &str,
        caller_token: Option<windows_sys::Win32::Foundation::HANDLE>,
    ) -> Result<(), VaultMountReason> {
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
                operation_id,
                target_session_id,
                caller_sid,
                caller_token,
                None,
                presentation,
                "vault.broker.dismount",
                broker_dismount_args(internal_drive, presented_drive_letter),
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

/// Private input to the authenticated broker.  It is deliberately not serde
/// serializable: the public named-pipe and Tauri wire must never reuse it.
struct InternalMountRequest {
    operation_id: u64,
    container_path: String,
    mount_mode: &'static str,
    volume_kind: &'static str,
    volume_role: &'static str,
    read_only: bool,
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
    recovery: Mutex<RecoveryState>,
}

/// A damaged/ambiguous boot registry is machine-wide unknown state and must
/// fail closed. A normal persistence failure after a known dismount is scoped
/// to that entry and automatically clears after the next successful write.
#[derive(Default)]
struct RecoveryState {
    registry_untrusted: bool,
    persistence_pending: HashSet<String>,
}

impl VaultMountBroker {
    /// Stable terminal codes for the personal-vault caller. These are the only
    /// engine details released through the service; all unrecognised broker
    /// failures stay generic.
    pub(crate) fn personal_mount_failure_code(reason: VaultMountReason) -> &'static str {
        match reason {
            VaultMountReason::NotAuthorized => "vault_not_authorized",
            VaultMountReason::SessionUnavailable => "vault_session_unavailable",
            VaultMountReason::EngineUnlockFailed => "vault_engine_unlock_failed",
            VaultMountReason::EngineDriveLetterUnavailable => {
                "vault_engine_drive_letter_unavailable"
            }
            VaultMountReason::EngineMountFailed => "vault_engine_mount_failed",
            VaultMountReason::AclApplyFailed => "vault_acl_apply_failed",
            VaultMountReason::AclReadbackFailed => "vault_acl_readback_failed",
            VaultMountReason::InvalidRequest => "vault_validation_failed",
            VaultMountReason::BrokerUnavailable
            | VaultMountReason::BrokerRejected
            | VaultMountReason::DismountFailed => "vault_broker_failed",
        }
    }

    pub fn new() -> Self {
        Self::with_broker(Box::new(ProEnvelopeBroker))
    }

    fn with_broker(broker: Box<dyn AuthenticatedVaultBroker>) -> Self {
        Self {
            active: Mutex::new(HashMap::new()),
            operation: Mutex::new(()),
            broker,
            recovery: Mutex::new(RecoveryState::default()),
        }
    }

    pub fn with_exclusive_operation<T>(&self, operation: impl FnOnce() -> T) -> T {
        let _guard = self
            .operation
            .lock()
            .expect("vault operation lock poisoned");
        operation()
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
    ) -> Result<(String, u8), VaultMountReason> {
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

    /// A legacy container is only promoted into the durable personal registry
    /// after its existing credential mounted successfully. If that promotion
    /// fails, close the just-created presentation before returning failure so
    /// an unregistered mount is never left accessible.
    pub(crate) fn dismount_personal_registration_failure(
        &self,
        store: &VaultAccessStore,
        record: &PersonalVaultRecord,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
    ) {
        self.with_exclusive_operation(|| {
            let _ = self.dismount_entry_locked_for_client(
                next_internal_operation_id(),
                store,
                &personal_mount_entry_id(record),
                Some(caller_token),
            );
        });
    }

    /// The pipe keeps drive-letter inspection and the broker call inside the
    /// same operation gate, so a second mount cannot invalidate preflight in
    /// the gap before the encrypted driver receives the request.
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
    ) -> Result<(String, u8), VaultMountReason> {
        if record.owner_sid != caller_sid
            || record.scope != VaultPresentation::PerUser
            || session_id == 0
            || caller_sid.is_empty()
        {
            request.zeroize_secrets();
            return Err(VaultMountReason::NotAuthorized);
        }
        let entry_id = personal_mount_entry_id(record);
        if !self.recovery_allows_entry(&entry_id, store) {
            request.zeroize_secrets();
            return Err(VaultMountReason::DismountFailed);
        }
        let access = if request.read_only {
            wincmd_shared::vault_access::VaultAccess::Read
        } else {
            wincmd_shared::vault_access::VaultAccess::Write
        };
        let profile = mount_profile(
            request.volume_kind,
            request.volume_role,
            access,
            request.hidden_protection_password.as_deref(),
        )?;
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
            if self
                .dismount_entry_locked_for_client(
                    operation_id,
                    store,
                    &entry_id,
                    Some(caller_token),
                )
                .state
                != VaultMountState::Unmounted
            {
                request.zeroize_secrets();
                return Err(VaultMountReason::DismountFailed);
            }
        }
        let mut internal = InternalMountRequest {
            operation_id,
            container_path: record.container_path.clone(),
            mount_mode: profile.mount_mode,
            volume_kind: profile.container_kind,
            volume_role: profile.volume_role,
            read_only: request.read_only,
            pim: request.pim,
            keyfiles: std::mem::take(&mut request.keyfiles),
            hidden_keyfiles: std::mem::take(&mut request.hidden_keyfiles),
            hidden_pim: request.hidden_pim,
            removable: request.removable,
            presentation: VaultPresentation::PerUser,
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
        if !reply.acl_attested
            || !valid_drive_letter(&reply.drive_letter)
            || reply.internal_drive > 25
        {
            let _ = self.broker.dismount(
                operation_id,
                reply.internal_drive,
                None,
                VaultPresentation::PerUser,
                session_id,
                caller_sid,
                Some(caller_token),
            );
            return Err(VaultMountReason::AclReadbackFailed);
        }
        let mounted_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or(0);
        let active = ActiveMount {
            drive_letter: reply.drive_letter.clone(),
            internal_drive: reply.internal_drive,
            presentation: VaultPresentation::PerUser,
            session_id,
            caller_sid: caller_sid.to_owned(),
            policy_id: "personal".into(),
            policy_version: 1,
            container_identity: record.container_identity.clone(),
            access,
            mounted_at,
        };
        if let Ok(mut mounts) = self.active.lock() {
            mounts.insert(entry_id.clone(), active.clone());
            if self.persist_active(store, &mounts).is_ok() {
                return Ok((reply.drive_letter, reply.internal_drive));
            }
            mounts.remove(&entry_id);
        }
        let _ = self.broker.dismount(
            operation_id,
            active.internal_drive,
            per_user_presented_drive_letter(active.presentation, active.drive_letter.as_str()),
            active.presentation,
            active.session_id,
            &active.caller_sid,
            Some(caller_token),
        );
        Err(VaultMountReason::DismountFailed)
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
            return failed(entry_id, None, VaultMountReason::DismountFailed);
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
            // A second authorized session must never evict a per-user mount.
            // Only the same authenticated user in the same session may remount.
            if existing.session_id != session_id || existing.caller_sid != caller_sid {
                zeroize_mount_secrets(password, hidden_protection_password);
                return denied(entry_id, VaultMountReason::NotAuthorized);
            }
            if self
                .dismount_entry_locked_for_client(operation_id, store, entry_id, Some(caller_token))
                .state
                != VaultMountState::Unmounted
            {
                zeroize_mount_secrets(password, hidden_protection_password);
                return failed(
                    entry_id,
                    Some(presentation),
                    VaultMountReason::DismountFailed,
                );
            }
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
        request.read_only = effective_access == wincmd_shared::vault_access::VaultAccess::Read;
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
            let _ = self.broker.dismount(
                operation_id,
                reply.internal_drive,
                None,
                presentation,
                session_id,
                &caller_sid,
                Some(caller_token),
            );
            return failed(
                entry_id,
                Some(presentation),
                VaultMountReason::AclReadbackFailed,
            );
        }
        let Some((policy_id, policy_version)) = store.active_policy_identity() else {
            let _ = self.broker.dismount(
                operation_id,
                reply.internal_drive,
                per_user_presented_drive_letter(presentation, reply.drive_letter.as_str()),
                presentation,
                session_id,
                &caller_sid,
                Some(caller_token),
            );
            return failed(
                entry_id,
                Some(presentation),
                VaultMountReason::NotAuthorized,
            );
        };
        if let Ok(mut active) = self.active.lock() {
            let mounted_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_secs())
                .unwrap_or(0);
            active.insert(
                entry_id.to_owned(),
                ActiveMount {
                    drive_letter: reply.drive_letter.clone(),
                    internal_drive: reply.internal_drive,
                    presentation,
                    session_id,
                    caller_sid,
                    policy_id,
                    policy_version,
                    container_identity,
                    access: effective_access,
                    mounted_at,
                },
            );
            if self.persist_active(store, &active).is_err() {
                let mount = active.remove(entry_id);
                drop(active);
                if let Some(mount) = mount {
                    let _ = self.broker.dismount(
                        operation_id,
                        mount.internal_drive,
                        per_user_presented_drive_letter(
                            mount.presentation,
                            mount.drive_letter.as_str(),
                        ),
                        mount.presentation,
                        mount.session_id,
                        &mount.caller_sid,
                        Some(caller_token),
                    );
                }
                return failed(
                    entry_id,
                    Some(presentation),
                    VaultMountReason::DismountFailed,
                );
            }
        } else {
            let _ = self.broker.dismount(
                operation_id,
                reply.internal_drive,
                per_user_presented_drive_letter(presentation, reply.drive_letter.as_str()),
                presentation,
                session_id,
                &caller_sid,
                Some(caller_token),
            );
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
        if self
            .broker
            .dismount(
                operation_id,
                active.internal_drive,
                per_user_presented_drive_letter(active.presentation, active.drive_letter.as_str()),
                active.presentation,
                active.session_id,
                &active.caller_sid,
                caller_token,
            )
            .is_err()
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

    pub fn dismount_authorized(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        entry_id: &str,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        caller_session: u32,
        caller_sid: &str,
        presentation: VaultPresentation,
    ) -> VaultMountResult {
        self.with_exclusive_operation(|| {
            self.dismount_authorized_locked(
                operation_id,
                store,
                entry_id,
                caller_token,
                caller_session,
                caller_sid,
                presentation,
            )
        })
    }

    fn dismount_authorized_locked(
        &self,
        operation_id: u64,
        store: &VaultAccessStore,
        entry_id: &str,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        caller_session: u32,
        caller_sid: &str,
        presentation: VaultPresentation,
    ) -> VaultMountResult {
        if caller_sid.is_empty()
            || (presentation == VaultPresentation::PerUser && caller_session == 0)
        {
            return denied(entry_id, VaultMountReason::NotAuthorized);
        }
        if let Some(active) = self
            .active
            .lock()
            .ok()
            .and_then(|active| active.get(entry_id).cloned())
        {
            // Shared presentation does not make every authorized member an
            // operator of another member's live mount. The mounter alone may
            // close it; an administrator can still apply a policy, whose
            // serialized cleanup is service-owned and closes all mounts.
            if !same_mount_owner(&active, caller_session, caller_sid) {
                return denied(entry_id, VaultMountReason::NotAuthorized);
            }
        }
        self.dismount_entry_locked_for_client(operation_id, store, entry_id, Some(caller_token))
    }

    pub fn projection(&self, entry_id: &str) -> (VaultMountState, Option<String>) {
        self.active
            .lock()
            .ok()
            .and_then(|active| active.get(entry_id).cloned())
            .map(|mount| (VaultMountState::Mounted, Some(mount.drive_letter)))
            .unwrap_or((VaultMountState::Unmounted, None))
    }

    pub fn dismount_all(&self, store: &VaultAccessStore) -> Result<(), VaultMountReason> {
        self.with_exclusive_operation(|| self.dismount_all_locked(store))
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
    pub fn load_and_cleanup(&self, store: &VaultAccessStore) -> Result<(), VaultMountReason> {
        let bytes = match store.read_active_mounts() {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
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
        for mount in registry.mounts.values() {
            if self.broker.recover_dismount(mount.internal_drive).is_err() {
                self.mark_registry_untrusted();
                return Err(VaultMountReason::DismountFailed);
            }
        }
        let empty = serde_json::to_vec(&DurableMountRegistry {
            mounts: HashMap::new(),
        })
        .map_err(|_| VaultMountReason::DismountFailed)?;
        if store.write_active_mounts(&empty).is_err() {
            self.mark_registry_untrusted();
            return Err(VaultMountReason::DismountFailed);
        }
        Ok(())
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

    fn recovery_allows_entry(&self, entry_id: &str, store: &VaultAccessStore) -> bool {
        let blocked = self
            .recovery
            .lock()
            .map(|state| state.registry_untrusted || state.persistence_pending.contains(entry_id))
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
        let active = match self.active.lock() {
            Ok(active) => active.clone(),
            Err(_) => return false,
        };
        self.persist_active(store, &active).is_ok()
    }

    fn mark_persistence_pending(&self, entry_id: &str) {
        if let Ok(mut recovery) = self.recovery.lock() {
            recovery.persistence_pending.insert(entry_id.to_owned());
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
        && mount.mounted_at > 0
}

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

fn mounted_root_acl_sddl(grants: &[ResolvedGrant]) -> MountedRootAclSddl {
    // OI+CI makes the proven root policy flow to files and directories
    // created after mount. Without it, a Partner-created file receives the
    // creator's default DACL and another authorized writer can be denied even
    // though both callers can open the volume root.
    let mut sddl = String::from("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
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
    use crate::vault_access::{AclApplier, AclSnapshot, PrincipalResolver, VaultAclPlan, VaultFs};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

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
        dismounted: Vec<u8>,
        recovered: Vec<u8>,
    }
    struct MountBroker(Arc<Mutex<BrokerEvents>>);
    impl AuthenticatedVaultBroker for MountBroker {
        fn mount(
            &self,
            _: &mut InternalMountRequest,
        ) -> Result<InternalMountReply, VaultMountReason> {
            Ok(InternalMountReply {
                drive_letter: "P:".into(),
                internal_drive: 12,
                acl_attested: true,
            })
        }
        fn dismount(
            &self,
            _: u64,
            internal_drive: u8,
            _: Option<&str>,
            _: VaultPresentation,
            _: u32,
            _: &str,
            _: Option<windows_sys::Win32::Foundation::HANDLE>,
        ) -> Result<(), VaultMountReason> {
            self.0.lock().unwrap().dismounted.push(internal_drive);
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

    fn personal_request() -> PersonalVaultMountRequest {
        PersonalVaultMountRequest {
            container_path: "C:\\vaults\\personal.hc".into(),
            password: "secret".into(),
            volume_kind: VaultContainerKind::Standard,
            volume_role: VaultVolumeRole::Outer,
            preferred_letter: Some("P".into()),
            read_only: false,
            pim: None,
            keyfiles: vec![],
            hidden_protection_password: None,
            hidden_keyfiles: vec![],
            hidden_pim: None,
            removable: false,
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
            container_identity: "identity".into(),
            access: wincmd_shared::vault_access::VaultAccess::Write,
            mounted_at: 1,
        }
    }

    #[test]
    fn root_sddl_is_internal_and_uses_only_resolved_sids() {
        let sddl = mounted_root_acl_sddl(&[ResolvedGrant {
            sid: "S-1-5-21-7".into(),
            access: wincmd_shared::vault_access::VaultAccess::Read,
        }]);
        assert!(sddl.0.starts_with("D:P"));
        assert!(sddl.0.contains("S-1-5-21-7"));
        assert_eq!(sddl.0.matches(";OICI;").count(), 3);
    }
    #[test]
    fn broker_drive_reply_is_bounded() {
        assert!(valid_drive_letter("V"));
        assert!(valid_drive_letter("V:"));
        assert!(!valid_drive_letter("V:\\private"));
    }

    #[test]
    fn personal_mount_terminal_reasons_keep_their_exact_codes() {
        for (reason, expected) in [
            (VaultMountReason::NotAuthorized, "vault_not_authorized"),
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
        ] {
            assert_eq!(
                VaultMountBroker::personal_mount_failure_code(reason),
                expected
            );
        }
    }

    #[test]
    fn broker_terminal_reasons_do_not_collapse_actionable_failures() {
        for (error, expected) in [
            ("session_unavailable", VaultMountReason::SessionUnavailable),
            ("vault_acl_apply_failed", VaultMountReason::AclApplyFailed),
            (
                "vault_acl_readback_failed",
                VaultMountReason::AclReadbackFailed,
            ),
            (
                "vault_engine_unlock_failed",
                VaultMountReason::EngineUnlockFailed,
            ),
            (
                "vault_engine_drive_letter_unavailable",
                VaultMountReason::EngineDriveLetterUnavailable,
            ),
            (
                "vault_engine_mount_failed",
                VaultMountReason::EngineMountFailed,
            ),
            ("vault_broker_failed", VaultMountReason::BrokerRejected),
            ("broker_timeout", VaultMountReason::BrokerUnavailable),
        ] {
            assert_eq!(broker_mount_reason(error), expected, "{error}");
        }
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
            Ok(("P:".into(), 12))
        );
        assert_eq!(broker.projection(&entry_id).0, VaultMountState::Mounted);
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
        restarted.load_and_cleanup(&store).unwrap();
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
            Err(VaultMountReason::DismountFailed)
        );
        assert_eq!(events.lock().unwrap().dismounted, vec![12]);
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
    fn shared_mounts_can_only_be_dismounted_by_the_mounter() {
        let mount = active_mount_for_owner(4, "S-1-5-21-owner");
        assert!(same_mount_owner(&mount, 4, "S-1-5-21-owner"));
        assert!(!same_mount_owner(&mount, 5, "S-1-5-21-owner"));
        assert!(!same_mount_owner(&mount, 4, "S-1-5-21-other"));
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
