// SPDX-License-Identifier: AGPL-3.0-or-later
//! Management uses the same attested current personal mount as explicit enrollment.
use super::*;
use wincmd_shared::vault_sync::{
    VaultSyncAction, VaultSyncManagementRequest, VaultSyncManagementResult,
};

impl VaultMountBroker {
    pub(crate) fn manage_personal_syncthing(
        &self,
        store: &VaultAccessStore,
        operation_id: u64,
        request: &VaultSyncManagementRequest,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        caller_session: u32,
        caller_sid: &str,
    ) -> Result<VaultSyncManagementResult, VaultMountReason> {
        if !request.valid() {
            return Err(VaultMountReason::InvalidRequest);
        }
        self.with_exclusive_operation(|| {
            if request.action == VaultSyncAction::Enable {
                let (entry_id, active) = self.eligible_syncthing_mount_locked(
                    store,
                    request.internal_drive,
                    caller_token,
                    caller_session,
                    caller_sid,
                    false,
                )?;
                require_mount_receipt(
                    request.expected_mount_receipt.as_deref(),
                    &entry_id,
                    &active,
                    true,
                )?;
                // A Fleet-private Vault is enabled only by its saved policy.
                // A standalone Vault's cloud button is the explicit consent;
                // write that consent before any private Syncthing call.
                if !active.personal {
                    return Err(VaultMountReason::SyncthingNotEnabled);
                }
                let path = active
                    .canonical_container_path
                    .as_deref()
                    .ok_or(VaultMountReason::SyncthingNotEnabled)?;
                store
                    .set_personal_syncthing_opt_in(
                        path,
                        &active.container_identity,
                        caller_sid,
                        Some(true),
                    )
                    .map_err(|_| VaultMountReason::SyncthingNotEnabled)?;
                return Ok(VaultSyncManagementResult {
                    managed: false,
                    mount_receipt: Some(mount_receipt(&entry_id, &active)),
                    gui_url: None,
                    folders: vec![],
                    removed: false,
                    syncthing_enabled: true,
                    can_enable_syncthing: false,
                });
            }
            let (entry_id, mut active) = self.eligible_syncthing_mount_locked(
                store,
                request.internal_drive,
                caller_token,
                caller_session,
                caller_sid,
                false,
            )?;
            let syncthing_enabled =
                self.syncthing_management_enabled_for_active(store, &entry_id, &active, caller_sid);
            if !syncthing_enabled {
                if request.action == VaultSyncAction::List {
                    return Ok(VaultSyncManagementResult {
                        managed: false,
                        mount_receipt: Some(mount_receipt(&entry_id, &active)),
                        gui_url: None,
                        folders: vec![],
                        removed: false,
                        syncthing_enabled: false,
                        can_enable_syncthing: active.personal,
                    });
                }
                return Err(VaultMountReason::SyncthingNotEnabled);
            }
            if request.action != VaultSyncAction::List {
                require_mount_receipt(request.expected_mount_receipt.as_deref(), &entry_id, &active, true)?;
            }
            if request.action == VaultSyncAction::Remove {
                // A lost reply must not let dismount skip synchronization cleanup.
                active.syncthing_resume_unresolved = true;
                let mut mounts = self.active.lock().map_err(|_| VaultMountReason::BrokerRejected)?;
                mounts.insert(entry_id.clone(), active.clone());
                self.persist_active(store, &mounts).map_err(|_| VaultMountReason::DismountFailed)?;
            }
            let value = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
                    crate::pro_broker::VaultCall {
                        request_id:operation_id, target_session_id:active.session_id,
                        caller_sid:&active.caller_sid, caller_token:Some(caller_token),
                        caller_authentication_id:authenticated_logon_id(caller_token), presentation:active.presentation,
                        feature_id:"vault.syncthing.manage",
                        args:serde_json::json!({"operation_id":operation_id,"vault_entry_id":entry_id,
                            "owner_sid":active.caller_sid,"volume_identity":active.container_identity,
                            "drive_letter":active.drive_letter,"target_session_id":active.session_id,
                            "action":request.action,"relative_path":request.relative_path,"folder_label":request.folder_label,"folder_id":request.folder_id}),
                    },
                ))
            })?;
            let mut result = parse_management_result(value, request.action)?;
            result.mount_receipt = Some(mount_receipt(&entry_id, &active));
            result.syncthing_enabled = true;
            result.can_enable_syncthing = false;
            if request.action == VaultSyncAction::Remove {
                let mut mounts = self.active.lock().map_err(|_| VaultMountReason::BrokerRejected)?;
                commit_removal_receipt(&mut mounts, &entry_id, result.managed, |candidate| {
                    self.persist_active(store, candidate).map_err(|_| VaultMountReason::DismountFailed)
                })?;
            }
            Ok(result)
        })
    }
}

pub(super) fn mount_receipt(entry_id: &str, active: &ActiveMount) -> String {
    let mut hash = Sha256::new();
    // Length-prefixed parts avoid ambiguity, and exclude sync flags so the
    // same receipt remains valid while adding several folders to this mount.
    for part in [
        entry_id,
        active.container_identity.as_str(),
        active.engine_mount_identity.as_deref().unwrap_or(""),
        active.caller_sid.as_str(),
        active.drive_letter.as_str(),
    ] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    hash.update(active.internal_drive.to_le_bytes());
    hash.update(active.session_id.to_le_bytes());
    hash.update(active.mounted_at.to_le_bytes());
    match active.authentication_id {
        Some((low, high)) => {
            hash.update([1]);
            hash.update(low.to_le_bytes());
            hash.update(high.to_le_bytes());
        }
        None => hash.update([0]),
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn require_mount_receipt(
    expected: Option<&str>,
    entry_id: &str,
    active: &ActiveMount,
    required: bool,
) -> Result<(), VaultMountReason> {
    if (required && expected.is_none())
        || expected.is_some_and(|receipt| receipt != mount_receipt(entry_id, active))
    {
        return Err(VaultMountReason::MountStateUnknown);
    }
    Ok(())
}

pub(super) fn commit_removal_receipt(
    mounts: &mut HashMap<String, ActiveMount>,
    entry_id: &str,
    managed: bool,
    persist: impl FnOnce(&HashMap<String, ActiveMount>) -> Result<(), VaultMountReason>,
) -> Result<(), VaultMountReason> {
    let mut candidate = mounts.clone();
    let active = candidate
        .get_mut(entry_id)
        .ok_or(VaultMountReason::MountStateUnknown)?;
    active.syncthing_managed = managed;
    active.syncthing_resume_unresolved = false;
    // Keep the conservative live state until its replacement is durable. A
    // failed final journal write must not let this process skip dismount pause.
    persist(&candidate)?;
    *mounts = candidate;
    Ok(())
}

fn parse_management_result(
    value: serde_json::Value,
    action: VaultSyncAction,
) -> Result<VaultSyncManagementResult, VaultMountReason> {
    let result: VaultSyncManagementResult =
        serde_json::from_value(value).map_err(|_| VaultMountReason::BrokerReplyRejected)?;
    if !result.valid()
        || result
            .gui_url
            .as_deref()
            .is_some_and(|url| !valid_syncthing_gui_url(url))
        || result.removed != (action == VaultSyncAction::Remove)
    {
        return Err(VaultMountReason::BrokerReplyRejected);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mounted_personal(identity: &str) -> ActiveMount {
        ActiveMount {
            drive_letter: "V".into(),
            internal_drive: 21,
            presentation: VaultPresentation::PerUser,
            session_id: 7,
            authentication_id: Some((11, 12)),
            caller_sid: "S-1-5-21-owner".into(),
            policy_id: "personal".into(),
            policy_version: 1,
            personal: true,
            syncthing_managed: false,
            syncthing_resume_unresolved: false,
            container_identity: identity.into(),
            access: wincmd_shared::vault_access::VaultAccess::Write,
            mounted_at: 99,
            cleanup_required: false,
            driver_slot_absent: false,
            engine_mount_identity: Some("driver-a".into()),
            canonical_container_path: Some("C:\\Vaults\\personal.hc".into()),
        }
    }

    #[test]
    fn standalone_enable_rejects_a_receipt_from_a_reused_drive_slot() {
        let original = mounted_personal("volume-a");
        let replacement = mounted_personal("volume-b");
        let receipt = mount_receipt("personal:original", &original);
        assert!(require_mount_receipt(
            Some(&receipt),
            "personal:original",
            &original,
            true,
        )
        .is_ok());
        assert_eq!(
            require_mount_receipt(Some(&receipt), "personal:replacement", &replacement, true),
            Err(VaultMountReason::MountStateUnknown),
            "the enable path checks this receipt before it writes durable consent or calls Pro"
        );
    }

    #[test]
    fn management_receipt_rejects_unverified_remove_and_remote_urls() {
        let empty =
            serde_json::json!({"managed":false,"gui_url":null,"folders":[],"removed":false});
        assert!(parse_management_result(empty.clone(), VaultSyncAction::List).is_ok());
        assert!(parse_management_result(empty.clone(), VaultSyncAction::Remove).is_err());
        let mut remote = empty;
        remote["gui_url"] = serde_json::json!("https://example.com");
        assert!(parse_management_result(remote, VaultSyncAction::List).is_err());
    }
}
