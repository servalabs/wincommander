// SPDX-License-Identifier: AGPL-3.0-or-later
//! The service derives every backup target from the authenticated current mount.
use super::*;
use wincmd_shared::vault_access::{VaultAccess, VaultCloudBackupPolicy};
use wincmd_shared::vault_cloud_backup::{
    VaultCloudBackupAction as Action, VaultCloudBackupRequest as Request,
    VaultCloudBackupResult as ResultView,
};

#[path = "vault_cloud_backup_supervisor.rs"]
mod supervisor;
#[path = "vault_cloud_backup_attestation.rs"]
mod attestation;
#[path = "vault_cloud_backup_operations.rs"]
mod operations;

impl VaultMountBroker {
    pub(crate) fn manage_cloud_backup(
        &self,
        store: &VaultAccessStore,
        operation_id: u64,
        request: &Request,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        caller_session: u32,
        caller_sid: &str,
    ) -> Result<ResultView, VaultMountReason> {
        if !request.valid() {
            return Err(VaultMountReason::InvalidRequest);
        }
        let operation = operations::for_target(caller_sid, caller_session, request.internal_drive)?;
        let _operation = tokio::task::block_in_place(|| operation.lock())
            .map_err(|_| VaultMountReason::BrokerRejected)?;
        let (entry_id, active) = self.with_exclusive_operation(|| {
            let (id, active) = self.eligible_cloud_backup_mount(
                store,
                request,
                caller_token,
                caller_session,
                caller_sid,
            )?;
            sync_management::require_mount_receipt(
                request.expected_mount_receipt.as_deref(),
                &id,
                &active,
                request.action != Action::Status,
            )?;
            Ok((id, active))
        })?;
        let key = supervisor::key(&active);
        if matches!(
            request.action,
            Action::Stop | Action::Disconnect | Action::RemoveSetup
        ) {
            supervisor::remove(&key);
        }
        // Downloads and browser sign-in never hold the Vault operation lock.
        let mut broker_request = request.clone();
        broker_request.personal = active.personal;
        let value = backup_call(
            "vault.backup.manage",
            operation_id,
            &active,
            caller_token,
            &broker_request,
        )?;
        let mut result: ResultView =
            serde_json::from_value(value).map_err(|_| VaultMountReason::BrokerReplyRejected)?;
        if !result.valid() {
            return Err(VaultMountReason::BrokerReplyRejected);
        }
        self.with_exclusive_operation(|| {
            let (id, observed) = self.eligible_cloud_backup_mount(
                store,
                request,
                caller_token,
                caller_session,
                caller_sid,
            )?;
            if sync_management::mount_receipt(&id, &observed)
                != sync_management::mount_receipt(&entry_id, &active)
            {
                supervisor::remove(&key);
                return Err(VaultMountReason::MountStateUnknown);
            }
            Ok(())
        })?;
        if matches!(request.action, Action::Connect | Action::Configure | Action::Start | Action::Verify | Action::Restore)
            && supervisor::should_renew(&result)
        {
            // Register after launch: a slow setup must not retire its lease before the runner exists.
            supervisor::register(entry_id.clone(), active.clone(), caller_token, false)?;
        }
        result.mount_receipt = Some(sync_management::mount_receipt(&entry_id, &active));
        result.can_configure = true;
        Ok(result)
    }

    fn eligible_cloud_backup_mount(
        &self,
        store: &VaultAccessStore,
        request: &Request,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        caller_session: u32,
        caller_sid: &str,
    ) -> Result<(String, ActiveMount), VaultMountReason> {
        if caller_token.is_null() || caller_session == 0 {
            return Err(VaultMountReason::NotAuthorized);
        }
        let (entry_id, mut active) = {
            let mounts = self
                .active
                .lock()
                .map_err(|_| VaultMountReason::MountStateUnknown)?;
            let id = slot::unique_entry(
                mounts
                    .iter()
                    .map(|(id, mount)| (id.as_str(), mount.internal_drive)),
                request.internal_drive,
            )
            .map_err(|_| VaultMountReason::MountStateUnknown)?
            .ok_or(VaultMountReason::MountStateUnknown)?;
            (
                id.to_owned(),
                mounts
                    .get(id)
                    .ok_or(VaultMountReason::MountStateUnknown)?
                    .clone(),
            )
        };
        if active.cleanup_required || !self.live_mount_matches(&active)? {
            return Err(VaultMountReason::MountStateUnknown);
        }
        if active.personal {
            if active.authentication_id.is_none()
                || !same_mount_owner(
                    &active,
                    caller_session,
                    caller_sid,
                    authenticated_logon_id(caller_token),
                )
                || active.access != VaultAccess::Write
            {
                return Err(VaultMountReason::NotAuthorized);
            }
        } else {
            let authorization = (self.policy_authorizer)(store, &entry_id, caller_token);
            let policy = store.policy().ok_or(VaultMountReason::NotAuthorized)?;
            let entry = policy
                .entries
                .iter()
                .find(|entry| entry.id == entry_id)
                .ok_or(VaultMountReason::NotAuthorized)?;
            if !policy_permits_backup(
                entry.cloud_backup,
                entry.primary_owner_sid.as_deref(),
                caller_sid,
                authorization.allowed,
                authorization.mode,
            ) {
                return Err(VaultMountReason::NotAuthorized);
            }
            if active.presentation == VaultPresentation::PerUser
                && (active.authentication_id.is_none()
                    || !same_mount_owner(
                        &active,
                        caller_session,
                        caller_sid,
                        authenticated_logon_id(caller_token),
                    ))
            {
                return Err(VaultMountReason::NotAuthorized);
            }
        }
        // A shared machine mount still uses this caller's isolated session/state.
        active.caller_sid = caller_sid.to_owned();
        active.session_id = caller_session;
        active.authentication_id = authenticated_logon_id(caller_token);
        if active.authentication_id.is_none() || self.owner_logon_ended(&active) {
            return Err(VaultMountReason::SessionUnavailable);
        }
        backup_presentation_result((self.caller_mount_attestor)(
            caller_token,
            &active.drive_letter,
            active.internal_drive,
            true,
        ))?;
        Ok((entry_id, active))
    }

    pub(super) fn queue_cloud_backup_resume(
        &self,
        entry_id: &str,
        active: &ActiveMount,
        token: windows_sys::Win32::Foundation::HANDLE,
    ) {
        #[cfg(not(test))]
        {
            let _ = supervisor::register(entry_id.to_owned(), active.clone(), token, true);
        }
        #[cfg(test)]
        {
            let _ = (entry_id, active, token);
        }
    }

    pub(super) fn stop_cloud_backup_for_mount(&self, active: &ActiveMount, wait: bool) {
        supervisor::stop_volume(active, wait);
    }

    pub(crate) fn start_cloud_backup_supervisor(
        self: &std::sync::Arc<Self>,
        store: std::sync::Arc<VaultAccessStore>,
    ) {
        supervisor::start(std::sync::Arc::clone(self), store);
    }
}

fn backup_presentation_result(attestation: CallerPresentationAttestation) -> Result<(), VaultMountReason> {
    match attestation {
        CallerPresentationAttestation::Available => Ok(()),
        CallerPresentationAttestation::RootAccessDenied
        | CallerPresentationAttestation::RootWriteAccessDenied => Err(VaultMountReason::CallerAccessDenied),
        CallerPresentationAttestation::MappingUnavailable
        | CallerPresentationAttestation::RootReadFailed => Err(VaultMountReason::PresentationRejected),
    }
}

fn policy_permits_backup(
    policy: Option<VaultCloudBackupPolicy>,
    owner: Option<&str>,
    caller: &str,
    allowed: bool,
    access: Option<VaultAccess>,
) -> bool {
    allowed
        && access == Some(VaultAccess::Write)
        && match policy {
            Some(VaultCloudBackupPolicy::OwnerOnly) => owner == Some(caller),
            Some(VaultCloudBackupPolicy::WriteMembers) => true,
            None => false,
        }
}

pub(super) fn overlaps_backup_state(path: &str) -> bool {
    let mut parts = path
        .split(['\\', '/'])
        .map(|part| part.trim_end_matches(['.', ' ']));
    parts
        .next()
        .is_some_and(|part| part.eq_ignore_ascii_case(".wincommander"))
        && parts
            .next()
            .map_or(true, |part| part.eq_ignore_ascii_case("cloud-backup"))
}

fn backup_request(active: &ActiveMount, action: Action) -> Request {
    Request {
        personal: active.personal,
        internal_drive: active.internal_drive,
        action,
        expected_mount_receipt: None,
        configuration: None,
        restore_relative_path: None,
    }
}

fn backup_call(
    feature: &'static str,
    operation_id: u64,
    active: &ActiveMount,
    token: windows_sys::Win32::Foundation::HANDLE,
    request: &Request,
) -> Result<serde_json::Value, VaultMountReason> {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
            crate::pro_broker::VaultCall {
                request_id: operation_id,
                target_session_id: active.session_id,
                caller_sid: &active.caller_sid,
                caller_token: Some(token),
                caller_authentication_id: active.authentication_id,
                presentation: active.presentation,
                feature_id: feature,
                args: serde_json::json!({"operation_id":operation_id,"owner_sid":active.caller_sid,
            "volume_identity":active.container_identity,"drive_letter":active.drive_letter,
            "target_session_id":active.session_id,"engine_mount_identity":active.engine_mount_identity,
            "request":request}),
            },
        ))
    })
}

#[cfg(test)]
#[path = "vault_cloud_backup_tests.rs"]
mod tests;
