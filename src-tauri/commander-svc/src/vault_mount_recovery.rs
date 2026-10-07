// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

pub(super) fn owner_logon_ended(mount: &ActiveMount) -> Result<bool, ()> {
    use windows_sys::Win32::System::RemoteDesktop::{
        WTSEnumerateSessionsW, WTSFreeMemory, WTS_CURRENT_SERVER_HANDLE,
    };
    // SAFETY: Successful OS calls own these buffers. Check count bounds and
    // null pointers before borrowing; finish each borrow before LSA/WTS frees it.
    unsafe {
        if let Some(expected) = mount.authentication_id {
            return crate::vault_drive_letters::logon_ended(expected);
        }
        // Legacy journals have no logon LUID. A disconnected or reused session
        // number is still present and cannot prove its old namespace disappeared.
        let mut count = 0;
        let mut sessions = std::ptr::null_mut();
        if WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut sessions, &mut count) == 0 {
            return Err(());
        }
        let result = if count > 65_536 || (count > 0 && sessions.is_null()) {
            Err(())
        } else if count == 0 {
            Ok(true)
        } else {
            Ok(!std::slice::from_raw_parts(sessions, count as usize)
                .iter()
                .any(|session| session.SessionId == mount.session_id))
        };
        if !sessions.is_null() {
            WTSFreeMemory(sessions.cast());
        }
        result
    }
}

impl VaultMountBroker {
    pub(super) fn confirm_unmounted_policy_entry(
        &self,
        store: &VaultAccessStore,
        request: &AuthorizedDismount<'_>,
    ) -> VaultMountResult {
        let uncertain = || denied(request.entry_id, VaultMountReason::MountStateUnknown);
        if request.caller_sid.is_empty() || request.caller_session == 0 {
            return uncertain();
        }
        let authorization = (self.policy_authorizer)(store, request.entry_id, request.caller_token);
        let Some(entry) = store.policy().and_then(|policy| {
            policy.entries.into_iter().find(|entry| entry.id == request.entry_id)
        }) else {
            return uncertain();
        };
        if !authorization.allowed || authorization.presentation != Some(entry.mount.presentation) {
            return uncertain();
        }
        if entry.mount.presentation == VaultPresentation::PerUser
            && entry.primary_owner_sid.as_deref() != Some(request.caller_sid)
        {
            return uncertain();
        }
        if entry.mount.presentation == VaultPresentation::Machine && !request.caller_elevated {
            return denied(request.entry_id, VaultMountReason::AdministratorRequired);
        }
        if self.recovery.lock().map_or(true, |state| {
            state.registry_untrusted || state.recoverable_registry
                || !state.persistence_pending.is_empty() || !state.removal_pending.is_empty()
        }) {
            return uncertain();
        }
        let Ok(slots) = self.snapshot() else {
            return uncertain();
        };
        let Ok(active) = self.active.lock() else {
            return uncertain();
        };
        if active.contains_key(request.entry_id) || slots.iter().any(|(slot, identity)| {
            let mut matches = active.values().filter(|mount| mount.internal_drive == *slot);
            match (matches.next(), matches.next()) {
                (Some(mount), None) => mount.driver_slot_absent
                    || mount.engine_mount_identity.as_ref() != Some(identity),
                _ => true,
            }
        }) {
            return uncertain();
        }
        if self.snapshot().ok().as_ref() != Some(&slots) {
            return uncertain();
        }
        // A retry can arrive after startup already completed the cleanup.
        // Only a fully accounted-for native snapshot proves this is idempotent.
        VaultMountResult {
            entry_id: request.entry_id.into(),
            state: VaultMountState::Unmounted,
            presentation: Some(entry.mount.presentation),
            drive_letter: None,
            reason: None,
            sync_warning: None,
        }
    }

    pub(super) fn pause_expired_owner_binding(
        &self,
        request: &AuthorizedDismount<'_>,
        mount: &ActiveMount,
        owner_logon_ended: bool,
        mut pause: impl FnMut(&ActiveMount) -> Result<SyncthingLifecycleResult, VaultMountReason>,
    ) -> Result<(), VaultMountReason> {
        if mount.caller_sid != request.caller_sid || !owner_logon_ended || !requires_syncthing_pause(mount) {
            return Ok(());
        }
        let mut current_owner_context = mount.clone();
        current_owner_context.session_id = request.caller_session;
        let result = pause(&current_owner_context);
        if syncthing_pause_confirmed(mount.syncthing_managed, &result) { Ok(()) }
        else { Err(result.err().unwrap_or(VaultMountReason::BrokerRejected)) }
    }

    pub(super) fn owner_logon_ended(&self, mount: &ActiveMount) -> bool {
        (self.owner_logon_probe)(mount) == Ok(true)
    }

    pub(super) fn owner_may_recover(&self, mount: &ActiveMount, caller_sid: &str) -> bool {
        mount.caller_sid == caller_sid && self.owner_logon_ended(mount)
    }

    pub(super) fn recovery_request_with_logon_proof<'a>(&self, mount: &'a ActiveMount, owner_logon_ended: bool) -> BrokerDismountRequest<'a> {
        let mut request = recovery_dismount_request(mount);
        if mount.presentation == VaultPresentation::PerUser && owner_logon_ended {
            request.owner_logon_ended = true;
            request.presented_drive_letter = None;
        }
        request
    }

    pub(super) fn reconcile_absent_mounts_locked(&self, store: &VaultAccessStore) -> Result<(), VaultMountReason> {
        let recovery = self.recovery.lock().map_err(|_| VaultMountReason::MountStateUnknown)?;
        if recovery.registry_untrusted && !recovery.recoverable_registry {
            return Err(VaultMountReason::MountStateUnknown);
        }
        if !recovery.registry_untrusted && self.active.lock().map_err(|_| VaultMountReason::MountStateUnknown)?.is_empty() {
            return Ok(());
        }
        drop(recovery);
        let slots = self.snapshot().map_err(|_| VaultMountReason::MountStateUnknown)?;
        let mut active = self.active.lock().map_err(|_| VaultMountReason::MountStateUnknown)?;
        let previous = active.clone();
        let mut changed = false;
        active.retain(|_, mount| {
            if slots.contains_key(&mount.internal_drive) { return true; }
            // A missing driver slot alone does not prove a DOS alias is gone.
            // In particular a Machine presentation can retain its global
            // alias, and a live per-user logon can retain its private alias.
            // Tokens may retain the old namespace after logoff. Never discard
            // cleanup authority until the exact private alias is also absent.
            if mount.presentation == VaultPresentation::PerUser
                && self.owner_logon_ended(mount)
                && (self.private_alias_absent)(mount) == Ok(true)
                && !requires_syncthing_pause(mount) {
                changed = true;
                return false;
            }
            changed |= !mount.cleanup_required || !mount.driver_slot_absent;
            mount.cleanup_required = true;
            mount.driver_slot_absent = true;
            true
        });
        // Recheck before discarding authority; a native query failure is not absence.
        if changed && (self.snapshot().ok().as_ref() != Some(&slots)
            || self.persist_active(store, &active).is_err()) {
            *active = previous;
            return Err(VaultMountReason::MountStateUnknown);
        }
        let trusted = slots.iter().all(|(slot, identity)| active.values().any(|mount| {
            mount.internal_drive == *slot && !mount.driver_slot_absent
                && mount.engine_mount_identity.as_ref() == Some(identity)
        }));
        let mut recovery = self.recovery.lock().map_err(|_| VaultMountReason::MountStateUnknown)?;
        if recovery.recoverable_registry && trusted {
            recovery.registry_untrusted = false;
            recovery.recoverable_registry = false;
        }
        Ok(())
    }
}

fn private_alias_operation(mount: &ActiveMount, cleanup: bool) -> Result<bool, ()> {
    let authentication_id = mount.authentication_id.ok_or(())?;
    let letter = mount.drive_letter.strip_suffix(':').unwrap_or(&mount.drive_letter);
    if letter.len() != 1 || !letter.as_bytes()[0].is_ascii_alphabetic() {
        return Err(());
    }
    let letter = letter.as_bytes()[0].to_ascii_uppercase() as char;
    if !cleanup {
        return crate::vault_drive_letters::ended_logon_encrypted_link_absent(
            authentication_id, letter, mount.internal_drive,
            &mount.caller_sid, mount.session_id,
        );
    }
    use crate::vault_drive_letters::GlobalEncryptedLinkCleanup;
    Ok(matches!(
        crate::vault_drive_letters::cleanup_ended_logon_encrypted_link(
            authentication_id, letter, mount.internal_drive,
            &mount.caller_sid, mount.session_id,
        )?,
        GlobalEncryptedLinkCleanup::Absent | GlobalEncryptedLinkCleanup::Removed
    ))
}

pub(super) fn private_alias_absent(mount: &ActiveMount) -> Result<bool, ()> {
    private_alias_operation(mount, false)
}

pub(super) fn cleanup_private_alias(mount: &ActiveMount) -> Result<bool, ()> {
    private_alias_operation(mount, true)
}
