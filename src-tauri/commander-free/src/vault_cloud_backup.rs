// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed desktop transport; target and account authority stay in the service.
use wincmd_shared::vault_cloud_backup::{VaultCloudBackupRequest, VaultCloudBackupResult};

async fn call(
    verb: &'static str,
    request: VaultCloudBackupRequest,
) -> Result<VaultCloudBackupResult, String> {
    if !request.valid() {
        return Err("cloud_backup_invalid_request".into());
    }
    let value = crate::svc_client::call(
        verb,
        serde_json::to_value(request).map_err(|_| "cloud_backup_invalid_request")?,
    )
    .await?;
    let result: VaultCloudBackupResult =
        serde_json::from_value(value).map_err(|_| "cloud_backup_invalid_result")?;
    if !result.valid() {
        return Err("cloud_backup_invalid_result".into());
    }
    Ok(result)
}

#[tauri::command]
pub async fn vault_setup_cloud_backup(
    request: VaultCloudBackupRequest,
) -> Result<VaultCloudBackupResult, String> {
    use wincmd_shared::vault_cloud_backup::VaultCloudBackupAction::{Configure, Connect, Prepare};
    if !matches!(request.action, Prepare | Connect | Configure) {
        return Err("cloud_backup_invalid_request".into());
    }
    call("svc.vault.setup_cloud_backup", request).await
}

#[tauri::command]
pub async fn vault_manage_cloud_backup(
    request: VaultCloudBackupRequest,
) -> Result<VaultCloudBackupResult, String> {
    call("svc.vault.manage_cloud_backup", request).await
}
