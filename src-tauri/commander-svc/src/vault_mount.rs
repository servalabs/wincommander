// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-owned mount broker boundary.
//!
//! The UI sends only an entry id and one-shot password.  This module resolves
//! the registered container/mode/presentation, passes the password only over
//! the fixed Pro engine's stdin, and returns a deliberately bounded result.
//! A drive is never reported mounted until its protected root DACL has been
//! applied and read back exactly; every later failure triggers dismount.

#![cfg(windows)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use zeroize::Zeroize;

use crate::vault_access::{ResolvedGrant, VaultAccessStore};
use wincmd_shared::vault_access::{
    VaultMountReason, VaultMountResult, VaultMountState, VaultPresentation,
};

const MAX_BROKER_REPLY_BYTES: usize = 4096;

#[derive(Debug, Clone)]
struct ActiveMount {
    drive_letter: String,
    internal_drive: u8,
    presentation: VaultPresentation,
    session_id: u32,
}

/// A stateful registry lets lifecycle handlers clean up mounts without ever
/// reconstructing a path from a renderer value.
pub struct VaultMountBroker {
    active: Mutex<HashMap<String, ActiveMount>>,
    broker: Box<dyn AuthenticatedVaultBroker>,
}

impl VaultMountBroker {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(HashMap::new()),
            broker: Box::new(UnavailableAuthenticatedBroker),
        }
    }

    pub fn mount(
        &self,
        store: &VaultAccessStore,
        entry_id: &str,
        password: &mut String,
        client_pid: u32,
    ) -> VaultMountResult {
        let Some((plan, presentation, preferred_letter)) = store.mount_plan(entry_id) else {
            password.zeroize();
            return denied(entry_id, VaultMountReason::NotAuthorized);
        };
        let session_id = match session_for_pid(client_pid) {
            Some(session_id) => session_id,
            None => {
                password.zeroize();
                return failed(entry_id, Some(presentation), VaultMountReason::SessionUnavailable);
            }
        };

        // Any previous active record is first removed through the fixed
        // broker.  We do not guess a drive or leave a stale presentation.
        if self.active.lock().ok().and_then(|m| m.get(entry_id).cloned()).is_some()
            && self.dismount_entry(entry_id).state != VaultMountState::Unmounted
        {
            password.zeroize();
            return failed(entry_id, Some(presentation), VaultMountReason::DismountFailed);
        }

        let mut request = InternalMountRequest {
            container: plan.container.clone(),
            mode: if plan
                .grants
                .iter()
                .any(|grant| grant.access == wincmd_shared::vault_access::VaultAccess::Write)
            {
                "standard"
            } else {
                "hidden"
            },
            presentation,
            preferred_letter,
            session_id,
            mounted_root_acl_sddl: mounted_root_acl_sddl(&plan.grants),
            password: std::mem::take(password),
        };
        let broker_reply = self.broker.mount(&mut request);
        request.password.zeroize();
        let broker_reply = match broker_reply {
            Ok(reply) => reply,
            Err(reason) => return failed(entry_id, Some(presentation), reason),
        };
        // Pro applies and reads back `mounted_root_acl_sddl` while the mount
        // remains closed. The authenticated broker returns only after that
        // attested step; service must not reopen a per-user presentation from
        // session 0 merely to attempt a second ACL check.
        let drive_letter = broker_reply.drive_letter;

        if let Ok(mut active) = self.active.lock() {
            active.insert(
                entry_id.to_owned(),
                ActiveMount {
                    drive_letter: drive_letter.clone(),
                    internal_drive: broker_reply.internal_drive,
                    presentation,
                    session_id,
                },
            );
        } else {
            let _ = self.broker.dismount(broker_reply.internal_drive, presentation);
            return failed(entry_id, Some(presentation), VaultMountReason::DismountFailed);
        }
        VaultMountResult {
            entry_id: entry_id.to_owned(),
            state: VaultMountState::Mounted,
            presentation: Some(presentation),
            drive_letter: Some(drive_letter),
            reason: None,
        }
    }

    pub fn dismount_entry(&self, entry_id: &str) -> VaultMountResult {
        let active = self.active.lock().ok().and_then(|mut mounts| mounts.remove(entry_id));
        let Some(active) = active else {
            return VaultMountResult {
                entry_id: entry_id.to_owned(),
                state: VaultMountState::Unmounted,
                presentation: None,
                drive_letter: None,
                reason: None,
            };
        };
        if self.broker.dismount(active.internal_drive, active.presentation).is_err() {
            if let Ok(mut mounts) = self.active.lock() {
                mounts.insert(entry_id.to_owned(), active.clone());
            }
            return failed(entry_id, Some(active.presentation), VaultMountReason::DismountFailed);
        }
        VaultMountResult {
            entry_id: entry_id.to_owned(),
            state: VaultMountState::Unmounted,
            presentation: Some(active.presentation),
            drive_letter: None,
            reason: None,
        }
    }

    /// Used on service start, stop, and policy/session reconciliation. The
    /// fixed broker is asked to remove any mount it owns before state is lost.
    pub fn dismount_all(&self) {
        let entry_ids = self
            .active
            .lock()
            .map(|mounts| mounts.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for entry_id in entry_ids {
            let _ = self.dismount_entry(&entry_id);
        }
        let _ = orphan_cleanup_via_fixed_broker();
    }

    pub fn dismount_session(&self, session_id: u32) {
        let entry_ids = self
            .active
            .lock()
            .map(|mounts| {
                mounts
                    .iter()
                    .filter(|(_, mount)| mount.session_id == session_id)
                    .map(|(entry_id, _)| entry_id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for entry_id in entry_ids {
            let _ = self.dismount_entry(&entry_id);
        }
    }
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

fn failed(entry_id: &str, presentation: Option<VaultPresentation>, reason: VaultMountReason) -> VaultMountResult {
    VaultMountResult {
        entry_id: entry_id.to_owned(),
        state: VaultMountState::Failed,
        presentation,
        drive_letter: None,
        reason: Some(reason),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerReply {
    drive_letter: String,
}

/// Internal-only SDDL. This type intentionally has no shared-wire serde
/// implementation: it is a service-to-Pro broker input, never a renderer
/// field or status value.
struct MountedRootAclSddl(String);

#[derive(Serialize)]
struct BrokerMountRequest<'a> {
    action: &'static str,
    container_path: &'a Path,
    mount_mode: &'static str,
    presentation: VaultPresentation,
    session_id: u32,
    mounted_root_acl_sddl: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    preferred_letter: Option<&'a str>,
    password: &'a str,
}

fn fixed_pro_engine_path() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("WinCommander")
        .join("bin")
        .join("wincommander-pro.exe")
}

fn mount_via_fixed_broker(
    plan: &crate::vault_access::VaultAclPlan,
    presentation: VaultPresentation,
    preferred_letter: Option<&str>,
    session_id: u32,
    password: &mut String,
) -> Result<String, VaultMountReason> {
    // `password` is deliberately not an argument. The fixed executable's
    // command line contains only a static capability selector.
    let mut child = Command::new(fixed_pro_engine_path())
        .arg("--vault-broker-stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| VaultMountReason::BrokerUnavailable)?;
    let acl_sddl = mounted_root_acl_sddl(&plan.grants);
    let request = BrokerMountRequest {
        action: "mount",
        container_path: &plan.container,
        mount_mode: if plan
            .grants
            .iter()
            .any(|grant| grant.access == wincmd_shared::vault_access::VaultAccess::Write)
        {
            "write"
        } else {
            "read"
        },
        presentation,
        session_id,
        mounted_root_acl_sddl: &acl_sddl.0,
        preferred_letter,
        password: password.as_str(),
    };
    let write_result = serde_json::to_vec(&request)
        .map_err(|_| VaultMountReason::InvalidRequest)
        .and_then(|mut bytes| {
            let result = child
                .stdin
                .as_mut()
                .ok_or(VaultMountReason::BrokerUnavailable)?
                .write_all(&bytes)
                .map_err(|_| VaultMountReason::BrokerUnavailable);
            bytes.zeroize();
            result
        });
    password.zeroize();
    write_result?;
    drop(child.stdin.take());
    let output = child.wait_with_output().map_err(|_| VaultMountReason::BrokerUnavailable)?;
    if !output.status.success() || output.stdout.len() > MAX_BROKER_REPLY_BYTES {
        return Err(VaultMountReason::BrokerRejected);
    }
    let reply: BrokerReply = serde_json::from_slice(&output.stdout).map_err(|_| VaultMountReason::BrokerRejected)?;
    normalize_drive_letter(&reply.drive_letter).ok_or(VaultMountReason::BrokerRejected)
}

fn mounted_root_acl_sddl(grants: &[ResolvedGrant]) -> MountedRootAclSddl {
    let mut sddl = String::from("D:P(A;;FA;;;SY)(A;;FA;;;BA)");
    for grant in grants {
        let mask = if grant.access == wincmd_shared::vault_access::VaultAccess::Write {
            "0x001301BF"
        } else {
            "0x001200A9"
        };
        // `grant.sid` came from the Windows account resolver, not from the
        // renderer. Rejecting unexpected characters still prevents an unsafe
        // SDDL token if a future resolver ever regresses.
        if grant.sid.starts_with("S-")
            && grant
                .sid
                .chars()
                .all(|character| character.is_ascii_digit() || character == '-')
        {
            sddl.push_str("(A;;");
            sddl.push_str(mask);
            sddl.push_str(";;;");
            sddl.push_str(&grant.sid);
            sddl.push(')');
        }
    }
    MountedRootAclSddl(sddl)
}

fn dismount_via_fixed_broker(
    drive_letter: &str,
    presentation: VaultPresentation,
) -> Result<(), VaultMountReason> {
    let letter = normalize_drive_letter(drive_letter).ok_or(VaultMountReason::DismountFailed)?;
    let internal_drive = letter.as_bytes()[0].saturating_sub(b'A');
    let mut child = Command::new(fixed_pro_engine_path())
        .arg("--vault-broker-stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| VaultMountReason::DismountFailed)?;
    let request = serde_json::json!({
        "action": "dismount",
        "internal_drive": internal_drive,
        "presentation_letter": letter,
        "presentation": presentation,
    });
    let mut bytes = serde_json::to_vec(&request).map_err(|_| VaultMountReason::DismountFailed)?;
    let write = child
        .stdin
        .as_mut()
        .ok_or(VaultMountReason::DismountFailed)?
        .write_all(&bytes)
        .map_err(|_| VaultMountReason::DismountFailed);
    bytes.zeroize();
    write?;
    drop(child.stdin.take());
    let status = child.wait().map_err(|_| VaultMountReason::DismountFailed)?;
    if status.success() { Ok(()) } else { Err(VaultMountReason::DismountFailed) }
}

fn orphan_cleanup_via_fixed_broker() -> Result<(), VaultMountReason> {
    let status = Command::new(fixed_pro_engine_path())
        .arg("--vault-broker-cleanup-orphans")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| VaultMountReason::BrokerUnavailable)?;
    if status.success() { Ok(()) } else { Err(VaultMountReason::BrokerRejected) }
}

fn session_for_pid(pid: u32) -> Option<u32> {
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    let mut session_id = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut session_id) } == 0 { None } else { Some(session_id) }
}

fn normalize_drive_letter(raw: &str) -> Option<String> {
    let trimmed = raw.trim().strip_suffix(':').unwrap_or(raw.trim());
    let mut chars = trimmed.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    if !letter.is_ascii_alphabetic() || chars.next().is_some() { return None; }
    Some(letter.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_letters_are_bounded_before_any_acl_or_dismount_call() {
        assert_eq!(normalize_drive_letter("v:"), Some("V".into()));
        assert!(normalize_drive_letter("V:\\sensitive").is_none());
        assert!(normalize_drive_letter("VV").is_none());
    }

    #[test]
    fn fixed_broker_path_does_not_come_from_a_request() {
        let path = fixed_pro_engine_path();
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("wincommander-pro.exe"));
    }

    #[test]
    fn mounted_root_sddl_is_service_internal_and_contains_only_resolved_sids() {
        let sddl = mounted_root_acl_sddl(&[ResolvedGrant {
            sid: "S-1-5-21-123".into(),
            access: wincmd_shared::vault_access::VaultAccess::Read,
        }]);
        assert!(sddl.0.contains("S-1-5-21-123"));
        assert!(sddl.0.contains("D:P"));
    }
}
