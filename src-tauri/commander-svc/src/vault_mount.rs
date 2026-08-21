// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-owned, fail-closed Vault mount broker boundary.
//!
//! This module never launches a public Pro CLI.  A path/password/ACL-bearing
//! mount request is accepted only by an authenticated service-to-Pro broker;
//! until that protected transport is installed, mount fails closed.

#![cfg(windows)]

use std::collections::HashMap;
use std::sync::Mutex;

use zeroize::Zeroize;

use crate::vault_access::{ResolvedGrant, VaultAccessStore};
use wincmd_shared::vault_access::{
    VaultMountReason, VaultMountResult, VaultMountState, VaultPresentation,
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

/// Service-to-Pro only.  No implementation may be backed by an unauthenticated
/// executable argument/stdio mode: a renderer must never be able to select the
/// container, SDDL, session, internal slot, or broker endpoint.
trait AuthenticatedVaultBroker: Send + Sync {
    fn mount(&self, request: &mut InternalMountRequest) -> Result<InternalMountReply, VaultMountReason>;
    fn dismount(&self, internal_drive: u8, presentation: VaultPresentation, target_session_id: u32) -> Result<(), VaultMountReason>;
    fn cleanup_orphans(&self) -> Result<(), VaultMountReason>;
}

struct ProEnvelopeBroker;

impl AuthenticatedVaultBroker for ProEnvelopeBroker {
    fn mount(&self, request: &mut InternalMountRequest) -> Result<InternalMountReply, VaultMountReason> {
        let args = serde_json::json!({
            "container_path": &request.container_path,
            "password": &request.password,
            "mounted_root_acl_sddl": &request.mounted_root_acl_sddl.0,
            "mount_mode": request.mount_mode,
            "presentation": request.presentation,
            "preferred_letter": &request.preferred_letter,
            "read_only": request.read_only,
            "target_session_id": request.target_session_id,
        });
        let result = tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(
            crate::pro_broker::vault_call(request.target_session_id, "vault.broker.mount", args),
        ));
        request.password.zeroize();
        let value = result.map_err(|error| match error.as_str() {
            "session_unavailable" => VaultMountReason::SessionUnavailable,
            "vault_acl_apply_failed" => VaultMountReason::AclApplyFailed,
            "vault_acl_readback_failed" => VaultMountReason::AclReadbackFailed,
            "broker_handshake" | "broker_hmac" | "broker_rejected" => VaultMountReason::BrokerRejected,
            _ => VaultMountReason::BrokerUnavailable,
        })?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply { drive_letter: String, internal_drive: u8, acl_attested: bool }
        let reply: Reply = serde_json::from_value(value).map_err(|_| VaultMountReason::BrokerRejected)?;
        Ok(InternalMountReply { drive_letter: reply.drive_letter, internal_drive: reply.internal_drive, acl_attested: reply.acl_attested })
    }
    fn dismount(&self, internal_drive: u8, _presentation: VaultPresentation, target_session_id: u32) -> Result<(), VaultMountReason> {
        let result = tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(
            crate::pro_broker::vault_call(target_session_id, "vault.broker.dismount", serde_json::json!({ "internal_drive": internal_drive })),
        ));
        result.map(|_| ()).map_err(|_| VaultMountReason::DismountFailed)
    }
    fn cleanup_orphans(&self) -> Result<(), VaultMountReason> { Ok(()) }
}

/// Private input to the authenticated broker.  It is deliberately not serde
/// serializable: the public named-pipe and Tauri wire must never reuse it.
struct InternalMountRequest {
    container_path: String,
    mount_mode: &'static str,
    read_only: bool,
    presentation: VaultPresentation,
    preferred_letter: Option<String>,
    target_session_id: u32,
    mounted_root_acl_sddl: MountedRootAclSddl,
    password: String,
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
    broker: Box<dyn AuthenticatedVaultBroker>,
    recovery_failed: std::sync::atomic::AtomicBool,
}

impl VaultMountBroker {
    pub fn new() -> Self {
        Self::with_broker(Box::new(ProEnvelopeBroker))
    }

    fn with_broker(broker: Box<dyn AuthenticatedVaultBroker>) -> Self {
        Self { active: Mutex::new(HashMap::new()), broker, recovery_failed: std::sync::atomic::AtomicBool::new(false) }
    }

    /// Called only after `authorize_mount_for_process` revalidated the named
    /// pipe client's actual token membership for this entry.
    pub fn mount_authorized(
        &self,
        store: &VaultAccessStore,
        entry_id: &str,
        password: &mut String,
        client_pid: u32,
        effective_access: wincmd_shared::vault_access::VaultAccess,
    ) -> VaultMountResult {
        if self.recovery_failed.load(std::sync::atomic::Ordering::Acquire) {
            password.zeroize();
            return failed(entry_id, None, VaultMountReason::DismountFailed);
        }
        let Some((plan, presentation, preferred_letter, container_identity)) = store.mount_plan(entry_id) else {
            password.zeroize();
            return denied(entry_id, VaultMountReason::NotAuthorized);
        };
        let Some(session_id) = session_for_pid(client_pid) else {
            password.zeroize();
            return failed(entry_id, Some(presentation), VaultMountReason::SessionUnavailable);
        };
        let Some(caller_sid) = caller_sid_for_pid(client_pid) else {
            password.zeroize();
            return denied(entry_id, VaultMountReason::NotAuthorized);
        };
        if let Some(existing) = self.active.lock().ok().and_then(|active| active.get(entry_id).cloned()) {
            // A second authorized session must never evict a per-user mount.
            // Only the same authenticated user in the same session may remount.
            if existing.session_id != session_id || existing.caller_sid != caller_sid {
                password.zeroize();
                return denied(entry_id, VaultMountReason::NotAuthorized);
            }
            if self.dismount_entry(store, entry_id).state != VaultMountState::Unmounted {
                password.zeroize();
                return failed(entry_id, Some(presentation), VaultMountReason::DismountFailed);
            }
        }
        let mut request = InternalMountRequest {
            container_path: plan.container.to_string_lossy().into_owned(),
            // Hidden-vs-standard is an immutable registered container fact,
            // never inferred from a member's read/write access. MVP exposes
            // only the registered standard container.
            mount_mode: "standard",
            read_only: false,
            presentation,
            preferred_letter,
            target_session_id: session_id,
            mounted_root_acl_sddl: mounted_root_acl_sddl(&plan.grants),
            password: std::mem::take(password),
        };
        request.read_only = effective_access == wincmd_shared::vault_access::VaultAccess::Read;
        let reply = self.broker.mount(&mut request);
        request.password.zeroize();
        let reply = match reply {
            Ok(reply) => reply,
            Err(reason) => return failed(entry_id, Some(presentation), reason),
        };
        if !reply.acl_attested || !valid_drive_letter(&reply.drive_letter) || reply.internal_drive > 25 {
            let _ = self.broker.dismount(reply.internal_drive, presentation, session_id);
            return failed(entry_id, Some(presentation), VaultMountReason::AclReadbackFailed);
        }
        let Some((policy_id, policy_version)) = store.active_policy_identity() else {
            let _ = self.broker.dismount(reply.internal_drive, presentation, session_id);
            return failed(entry_id, Some(presentation), VaultMountReason::NotAuthorized);
        };
        if let Ok(mut active) = self.active.lock() {
            let mounted_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_secs()).unwrap_or(0);
            active.insert(entry_id.to_owned(), ActiveMount { drive_letter: reply.drive_letter.clone(), internal_drive: reply.internal_drive, presentation, session_id, caller_sid, policy_id, policy_version, container_identity, access: effective_access, mounted_at });
            if self.persist_active(store, &active).is_err() {
                let mount = active.remove(entry_id);
                drop(active);
                if let Some(mount) = mount { let _ = self.broker.dismount(mount.internal_drive, mount.presentation, mount.session_id); }
                return failed(entry_id, Some(presentation), VaultMountReason::DismountFailed);
            }
        } else {
            let _ = self.broker.dismount(reply.internal_drive, presentation, session_id);
            return failed(entry_id, Some(presentation), VaultMountReason::DismountFailed);
        }
        VaultMountResult { entry_id: entry_id.to_owned(), state: VaultMountState::Mounted, presentation: Some(presentation), drive_letter: Some(reply.drive_letter), reason: None }
    }

    pub fn dismount_entry(&self, store: &VaultAccessStore, entry_id: &str) -> VaultMountResult {
        let active = self.active.lock().ok().and_then(|mut active| active.remove(entry_id));
        let Some(active) = active else {
            return VaultMountResult { entry_id: entry_id.to_owned(), state: VaultMountState::Unmounted, presentation: None, drive_letter: None, reason: None };
        };
        if self.broker.dismount(active.internal_drive, active.presentation, active.session_id).is_err() {
            if let Ok(mut mounts) = self.active.lock() { mounts.insert(entry_id.to_owned(), active.clone()); }
            return failed(entry_id, Some(active.presentation), VaultMountReason::DismountFailed);
        }
        if let Ok(mounts) = self.active.lock() {
            if self.persist_active(store, &mounts).is_err() {
                // The volume is already closed; keep the durable old record
                // for conservative cleanup on a later service start.
                self.recovery_failed.store(true, std::sync::atomic::Ordering::Release);
                return failed(entry_id, Some(active.presentation), VaultMountReason::DismountFailed);
            }
        }
        VaultMountResult { entry_id: entry_id.to_owned(), state: VaultMountState::Unmounted, presentation: Some(active.presentation), drive_letter: None, reason: None }
    }

    pub fn dismount_authorized(
        &self,
        store: &VaultAccessStore,
        entry_id: &str,
        client_pid: u32,
        presentation: VaultPresentation,
    ) -> VaultMountResult {
        let Some(caller_session) = session_for_pid(client_pid) else {
            return denied(entry_id, VaultMountReason::SessionUnavailable);
        };
        if let Some(active) = self.active.lock().ok().and_then(|active| active.get(entry_id).cloned()) {
            if active.session_id != caller_session && presentation != VaultPresentation::Machine {
                return denied(entry_id, VaultMountReason::NotAuthorized);
            }
        }
        self.dismount_entry(store, entry_id)
    }

    pub fn projection(&self, entry_id: &str) -> (VaultMountState, Option<String>) {
        self.active.lock().ok().and_then(|active| active.get(entry_id).cloned())
            .map(|mount| (VaultMountState::Mounted, Some(mount.drive_letter)))
            .unwrap_or((VaultMountState::Unmounted, None))
    }

    pub fn dismount_all(&self, store: &VaultAccessStore) -> Result<(), VaultMountReason> {
        let entries = self.active.lock().map(|active| active.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
        for entry_id in entries {
            if self.dismount_entry(store, &entry_id).state != VaultMountState::Unmounted {
                return Err(VaultMountReason::DismountFailed);
            }
        }
        self.broker.cleanup_orphans()?;
        Ok(())
    }

    pub fn dismount_session(&self, store: &VaultAccessStore, session_id: u32) {
        let entries = self.active.lock().map(|active| active.iter().filter(|(_, mount)| mount.session_id == session_id).map(|(id, _)| id.clone()).collect::<Vec<_>>()).unwrap_or_default();
        for entry_id in entries { let _ = self.dismount_entry(store, &entry_id); }
    }

    /// At boot, stale protected records are closed by their exact internal
    /// slot. Ambiguous/corrupt records or a failed cleanup deny new mounts;
    /// this prevents a reboot from silently preserving an old presentation.
    pub fn load_and_cleanup(&self, store: &VaultAccessStore) -> Result<(), VaultMountReason> {
        let bytes = match store.read_active_mounts() {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => { self.recovery_failed.store(true, std::sync::atomic::Ordering::Release); return Err(VaultMountReason::DismountFailed); }
        };
        let registry: DurableMountRegistry = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => { self.recovery_failed.store(true, std::sync::atomic::Ordering::Release); return Err(VaultMountReason::DismountFailed); }
        };
        if registry.mounts.len() > MAX_DURABLE_MOUNTS || registry.mounts.iter().any(|(entry, mount)| !valid_durable_mount(entry, mount)) {
            self.recovery_failed.store(true, std::sync::atomic::Ordering::Release);
            return Err(VaultMountReason::DismountFailed);
        }
        for mount in registry.mounts.values() {
            if self.broker.dismount(mount.internal_drive, mount.presentation, mount.session_id).is_err() {
                self.recovery_failed.store(true, std::sync::atomic::Ordering::Release);
                return Err(VaultMountReason::DismountFailed);
            }
        }
        let empty = serde_json::to_vec(&DurableMountRegistry { mounts: HashMap::new() }).map_err(|_| VaultMountReason::DismountFailed)?;
        if store.write_active_mounts(&empty).is_err() {
            self.recovery_failed.store(true, std::sync::atomic::Ordering::Release);
            return Err(VaultMountReason::DismountFailed);
        }
        Ok(())
    }

    fn persist_active(&self, store: &VaultAccessStore, active: &HashMap<String, ActiveMount>) -> Result<(), ()> {
        if active.len() > MAX_DURABLE_MOUNTS { return Err(()); }
        let bytes = serde_json::to_vec(&DurableMountRegistry { mounts: active.clone() }).map_err(|_| ())?;
        store.write_active_mounts(&bytes).map_err(|_| ())
    }
}

fn valid_durable_mount(entry_id: &str, mount: &ActiveMount) -> bool {
    !entry_id.is_empty() && entry_id.len() <= 64
        && entry_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && valid_drive_letter(&mount.drive_letter) && mount.internal_drive <= 25
        && mount.caller_sid.starts_with("S-") && mount.caller_sid.len() <= 184
        && !mount.policy_id.is_empty() && mount.policy_id.len() <= 64 && mount.policy_version > 0
        && !mount.container_identity.is_empty() && mount.container_identity.len() <= 256
        && mount.mounted_at > 0
}

fn caller_sid_for_pid(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() { return None; }
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 { unsafe { CloseHandle(process) }; return None; }
    let mut size = 0u32;
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size); }
    let mut buffer = vec![0u8; size as usize];
    let ok = size != 0 && unsafe { GetTokenInformation(token, TokenUser, buffer.as_mut_ptr() as _, size, &mut size) } != 0;
    let sid = if ok { unsafe { crate::vault_access::sid_to_string((buffer.as_ptr() as *const TOKEN_USER).read().User.Sid) } } else { None };
    unsafe { CloseHandle(token); CloseHandle(process); }
    buffer.zeroize();
    sid
}

fn mounted_root_acl_sddl(grants: &[ResolvedGrant]) -> MountedRootAclSddl {
    let mut sddl = String::from("D:P(A;;FA;;;SY)(A;;FA;;;BA)");
    for grant in grants {
        if grant.sid.starts_with("S-")
            && grant
                .sid
                .chars()
                .all(|c| c == 'S' || c.is_ascii_digit() || c == '-')
        {
            let mask = if grant.access == wincmd_shared::vault_access::VaultAccess::Write { "0x001301BF" } else { "0x001200A9" };
            sddl.push_str("(A;;"); sddl.push_str(mask); sddl.push_str(";;;"); sddl.push_str(&grant.sid); sddl.push(')');
        }
    }
    MountedRootAclSddl(sddl)
}

fn session_for_pid(pid: u32) -> Option<u32> {
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    let mut session_id = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut session_id) } == 0 { None } else { Some(session_id) }
}

fn valid_drive_letter(value: &str) -> bool {
    let value = value.strip_suffix(':').unwrap_or(value);
    value.len() == 1 && value.as_bytes()[0].is_ascii_alphabetic()
}

fn denied(entry_id: &str, reason: VaultMountReason) -> VaultMountResult {
    VaultMountResult { entry_id: entry_id.to_owned(), state: VaultMountState::Denied, presentation: None, drive_letter: None, reason: Some(reason) }
}
fn failed(entry_id: &str, presentation: Option<VaultPresentation>, reason: VaultMountReason) -> VaultMountResult {
    VaultMountResult { entry_id: entry_id.to_owned(), state: VaultMountState::Failed, presentation, drive_letter: None, reason: Some(reason) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_sddl_is_internal_and_uses_only_resolved_sids() {
        let sddl = mounted_root_acl_sddl(&[ResolvedGrant { sid: "S-1-5-21-7".into(), access: wincmd_shared::vault_access::VaultAccess::Read }]);
        assert!(sddl.0.starts_with("D:P"));
        assert!(sddl.0.contains("S-1-5-21-7"));
    }
    #[test]
    fn broker_drive_reply_is_bounded() {
        assert!(valid_drive_letter("V"));
        assert!(valid_drive_letter("V:"));
        assert!(!valid_drive_letter("V:\\private"));
    }
}
