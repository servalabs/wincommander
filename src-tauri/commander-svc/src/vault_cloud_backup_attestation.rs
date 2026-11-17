// SPDX-License-Identifier: AGPL-3.0-or-later
//! Driver generation is checked by SYSTEM; the backup remains in the caller token.
use super::*;
use wincmd_shared::vault_cloud_backup::VaultCloudBackupAttestationRequest;

impl VaultMountBroker {
    pub(crate) fn attest_cloud_backup(
        &self,
        store: &VaultAccessStore,
        request: &VaultCloudBackupAttestationRequest,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
        caller_session: u32,
        caller_sid: &str,
    ) -> Result<bool, VaultMountReason> {
        if !request.valid() {
            return Err(VaultMountReason::InvalidRequest);
        }
        let selection = Request {
            personal: request.personal,
            internal_drive: request.internal_drive,
            action: Action::Status,
            expected_mount_receipt: None,
            configuration: None,
            restore_relative_path: None,
        };
        self.with_exclusive_operation(|| {
            // This rechecks current driver generation, caller logon, write access,
            // backup policy and the caller's actual filesystem presentation.
            let (_, active) = self.eligible_cloud_backup_mount(
                store, &selection, caller_token, caller_session, caller_sid,
            )?;
            Ok(active.engine_mount_identity.as_deref()
                == Some(request.expected_engine_mount_identity.as_str()))
        })
    }
}
