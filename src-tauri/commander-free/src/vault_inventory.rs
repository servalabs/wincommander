//! Legacy desktop status/cleanup adapters. All inventory and mutation authority
//! comes from the authenticated service; never invoke raw drive enumeration.

use serde_json::{json, Value};
use wincmd_shared::vault_access::{PersonalVaultMountedVolume, VaultMountReason};
use wincmd_shared::vault_inventory::{
    bulk_failure, bulk_success, confirmed_slot_dismount, project_mount, query_mounts,
};

async fn mounts() -> Result<Vec<PersonalVaultMountedVolume>, String> {
    mounts_via(|verb, args| crate::svc_client::call(verb, args)).await
}

async fn mounts_via<F, Fut>(call: F) -> Result<Vec<PersonalVaultMountedVolume>, String>
where
    F: FnMut(&'static str, Value) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    query_mounts(call).await
}

pub(super) async fn authorized_root(letter: &str) -> Result<String, String> {
    wincmd_shared::vault_inventory::authorized_root(&mounts().await?, letter)
}

pub(super) async fn open(letter: &str) -> Result<Value, String> {
    let root = authorized_root(letter).await?;
    if !std::path::Path::new(&root).is_dir() {
        return Err("caller_root_unavailable".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(&root)
        .spawn()
        .map_err(|_| "caller_root_unavailable".to_owned())?;
    Ok(json!({"status":"opened","drive":&root[..2]}))
}

pub(super) async fn status() -> Result<Value, String> {
    let mounts = mounts().await?;
    let volumes = mounts
        .iter()
        .map(|mount| {
            let accessible = std::path::Path::new(&format!("{}\\", mount.drive_letter)).is_dir();
            project_mount(mount, accessible)
        })
        .collect::<Vec<_>>();
    // Use the existing bundled-payload status, not an extracted engine artifact
    // (which need not exist before the first mount). No raw engine probe.
    Ok(
        json!({"installed":crate::pro_install::pro_is_installed(), "path":Value::Null, "volumes":volumes}),
    )
}

pub(super) async fn dismount_authorized() -> Result<Value, String> {
    let result = dismount_snapshot(mounts().await?, |slot| async move {
        crate::svc_client::call(
            "svc.vault.dismount_personal",
            json!({"personal":true,"internal_drive":slot}),
        )
        .await
    })
    .await?;
    let verified = crate::svc_client::call(
        "svc.vault.list_authorized",
        json!({"personal":true,"inventory_version":2,"verify_cleanup":true}),
    ).await?;
    wincmd_shared::vault_inventory::parse_mounts(verified)?;
    Ok(result)
}

async fn dismount_snapshot<F, Fut>(
    mounts: Vec<PersonalVaultMountedVolume>,
    mut dismount: F,
) -> Result<Value, String>
where
    F: FnMut(u8) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    let total = mounts.len();
    let denied_reason = mounts
        .iter()
        .find(|mount| !mount.dismount_allowed)
        .map(|mount| {
            mount
                .dismount_reason
                .unwrap_or(VaultMountReason::NotAuthorized)
                .as_str()
        });
    let mut count = 0;
    for mount in mounts.into_iter().filter(|mount| mount.dismount_allowed) {
        match dismount(mount.internal_drive)
            .await
            .and_then(|result| confirmed_slot_dismount(result, mount.internal_drive))
        {
            Ok(_) => count += 1,
            Err(error) => return Err(bulk_failure(&error, count, total - count)),
        }
    }
    if let Some(reason) = denied_reason {
        return Err(bulk_failure(reason, count, total - count));
    }
    Ok(bulk_success(count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincmd_shared::vault_inventory::parse_mounts;
    #[tokio::test]
    async fn production_inventory_adapter_preserves_service_failure_no_raw_fallback() {
        let error = mounts_via(|verb, args| async move {
            assert_eq!(verb, "svc.vault.list_authorized");
            assert_eq!(args, json!({"personal":true,"inventory_version":2}));
            Err("vault_mount_state_unknown".into())
        })
        .await
        .unwrap_err();
        assert_eq!(error, "vault_mount_state_unknown");
        assert!(mounts_via(|_, _| async { Ok(json!({"volumes":[]})) })
            .await
            .is_err());
    }
    #[tokio::test]
    async fn legacy_cleanup_uses_only_service_allowed_slots_and_reports_partial() {
        let mounts=parse_mounts(json!([
            {"drive_letter":"V:","internal_drive":12,"presentation":"machine","cleanup_required":false,"dismount_allowed":true},
            {"drive_letter":"W:","internal_drive":13,"presentation":"machine","cleanup_required":false,"dismount_allowed":false,"dismount_reason":"policy_access_denied"}
        ])).unwrap();
        let result = dismount_snapshot(mounts, |slot| async move {
            assert_eq!(slot, 12);
            Ok(json!({"state":"unmounted","entry_id":"personal:12"}))
        })
        .await;
        assert_eq!(
            result.unwrap_err(),
            "vault_bulk_dismount_partial:policy_access_denied:1:1"
        );
    }

    #[tokio::test]
    async fn unknown_service_receipt_is_not_success() {
        let mounts=parse_mounts(json!([
            {"drive_letter":"V:","internal_drive":12,"presentation":"machine","cleanup_required":false,"dismount_allowed":true}
        ])).unwrap();
        assert_eq!(
            dismount_snapshot(mounts, |_| async { Ok(json!({"ok":true})) })
                .await
                .unwrap_err(),
            "mount_state_unknown"
        );
    }
}
