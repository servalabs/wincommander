//! Legacy desktop status/cleanup adapters. All inventory and mutation authority
//! comes from the authenticated service. Desktop identity checks can only deny
//! opening an already authorized Quick Mount; they never grant access.

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

async fn quick_mounts() -> Result<Vec<PersonalVaultMountedVolume>, String> {
    let response = crate::svc_client::call(
        "svc.vault.list_authorized",
        json!({"personal":true,"inventory_version":2,"quick_mount_receipts":true}),
    ).await?;
    wincmd_shared::vault_inventory::parse_mounts(response)
}

fn quick_mount_root(
    mounts: &[PersonalVaultMountedVolume], drive: &str, expected: &str,
) -> Result<String, String> {
    if expected.len() != 68 || !expected.starts_with("qm1:")
        || !expected[4..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("mount_state_unknown".into());
    }
    let root = wincmd_shared::vault_inventory::authorized_root(mounts, drive)?;
    if !matches!(root.as_bytes()[0], b'D'..=b'Z') {
        return Err("not_authorized".into());
    }
    let mount = mounts.iter().find(|mount| mount.drive_letter.eq_ignore_ascii_case(&root[..2]))
        .ok_or("mount_state_unknown")?;
    if mount.quick_mount_receipt.as_deref() != Some(expected) {
        return Err("not_authorized".into());
    }
    Ok(root)
}

#[tauri::command]
pub(crate) async fn open_quick_mount_drive(drive: String, expected_mount_receipt: String) -> Result<(), String> {
    let mounts = quick_mounts().await?;
    let root = quick_mount_root(&mounts, &drive, &expected_mount_receipt)?;
    let slot = mounts.iter().find(|mount| mount.drive_letter.eq_ignore_ascii_case(&root[..2]))
        .ok_or("mount_state_unknown")?.internal_drive;
    #[cfg(windows)]
    let identity = wincmd_volume::mounted_slot_identities()
        .map_err(|_| "mount_state_unknown".to_owned())?
        .remove(&slot).ok_or("mount_state_unknown")?;
    #[cfg(not(windows))]
    let identity = return Err("caller_root_unavailable".into());
    // The second service receipt binds this native snapshot to the authorized mount.
    quick_mount_root(&quick_mounts().await?, &drive, &expected_mount_receipt)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = wincmd_volume::VolumeOperationGuard::acquire_slot(slot)
            .map_err(|_| "mount_state_unknown".to_owned())?;
        let check = || {
            let info = wincmd_volume::inspect_path(std::path::Path::new(&root))
                .map_err(|_| "caller_root_unavailable".to_owned())?;
            if !quick_mount_desktop_matches(&info, &root, slot, &identity) {
                return Err("caller_root_unavailable".to_owned());
            }
            Ok(())
        };
        check()?;
        let mut entries = std::fs::read_dir(&root)
            .map_err(|_| "caller_root_unavailable".to_owned())?;
        if let Some(Err(_)) = entries.next() {
            return Err("caller_root_unavailable".to_owned());
        }
        check()?;
        std::process::Command::new("explorer.exe").arg(&root).spawn()
            .map_err(|_| "caller_root_unavailable".to_owned())?;
        Ok(())
    }).await.map_err(|_| "mount_state_unknown".to_owned())?
}

fn quick_mount_desktop_matches(info: &wincmd_volume::VolumeInfo, root: &str, slot: u8, identity: &str) -> bool {
    slot <= 25 && info.is_private && !identity.is_empty() && info.identity == identity
        && info.root == std::path::Path::new(root)
        && info.device.eq_ignore_ascii_case(&format!("\\Device\\VeraCryptVolume{}", char::from(b'A' + slot)))
}

pub(super) async fn status() -> Result<Value, String> {
    let mounts = match quick_mounts().await {
        Ok(mounts) => mounts,
        Err(error) if error == "vault_validation_failed"
            || error.starts_with("service rejected request: vault_validation_failed (") => mounts().await?,
        Err(error) => return Err(error),
    };
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
    #[test]
    fn quick_mount_explorer_rejects_foreign_alias_slot_and_remounted_identity() {
        let valid = wincmd_volume::VolumeInfo {
            root: "V:\\".into(), stable_id: "fixture".into(), identity: "fixture:1".into(),
            device: "\\Device\\VeraCryptVolumeM".into(), is_private: true, read_only: false,
        };
        assert!(quick_mount_desktop_matches(&valid, "V:\\", 12, "fixture:1"));
        for wrong in [
            wincmd_volume::VolumeInfo { is_private: false, ..valid.clone() },
            wincmd_volume::VolumeInfo { device: "\\Device\\VeraCryptVolumeN".into(), ..valid.clone() },
            wincmd_volume::VolumeInfo { device: "\\Device\\HarddiskVolume12".into(), ..valid.clone() },
            wincmd_volume::VolumeInfo { identity: "fixture:2".into(), ..valid.clone() },
            wincmd_volume::VolumeInfo { root: "W:\\".into(), ..valid.clone() },
        ] {
            assert!(!quick_mount_desktop_matches(&wrong, "V:\\", 12, "fixture:1"));
        }
        assert!(!quick_mount_desktop_matches(&valid, "V:\\", 26, "fixture:1"));
        assert!(!quick_mount_desktop_matches(&valid, "V:\\", 12, ""));
    }
    #[test]
    fn quick_mount_opening_rejects_missing_stale_denied_and_cleanup_receipts() {
        let receipt = format!("qm1:{}", "a".repeat(64));
        let rows = json!([{"drive_letter":"V:","internal_drive":12,"presentation":"machine",
            "cleanup_required":false,"browse_allowed":true,"dismount_allowed":true,
            "quick_mount_receipt":receipt}]);
        assert_eq!(quick_mount_root(&parse_mounts(rows.clone()).unwrap(), "V:", &receipt).unwrap(), "V:\\");
        for (field, value) in [
            ("quick_mount_receipt", Value::Null),
            ("quick_mount_receipt", json!(format!("qm1:{}", "b".repeat(64)))),
            ("browse_allowed", json!(false)), ("cleanup_required", json!(true)),
        ] {
            let mut invalid = rows.clone();
            invalid[0][field] = value;
            assert!(quick_mount_root(&parse_mounts(invalid).unwrap(), "V:", &receipt).is_err());
        }
        assert!(quick_mount_root(&parse_mounts(rows).unwrap(), "C:", &receipt).is_err());
        assert!(quick_mount_root(&[], "V:", &receipt).is_err());
        assert!(quick_mount_root(&[], "V:", "forged").is_err());
    }
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
