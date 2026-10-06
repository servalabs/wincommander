use crate::command_strings::matches_parts;

pub(super) fn require_vault_runtime(feature_id: &str, version: Option<u32>) -> Result<(), String> {
    let lifecycle = [
        &["Create-~", "Encryption~", "Volume~"][..],
        &["New-~", "Encrypted~", "Volume~"][..],
        &["Mount-~", "Encryption~", "Volume~"][..],
        &["Mount-~", "Encrypted~", "Volume~"][..],
        &["Dismount-~", "Encryption~", "Volume~"][..],
        &["Dismount-~", "Encrypted~", "Volume~"][..],
        &["Dismount-~", "AllEncryption~", "Volumes~"][..],
        &["Dismount-~", "AllEncrypted~", "Volumes~"][..],
        &["Get-~", "EncryptedVolume~", "Status~"][..],
        &["Open-~", "Encryption~", "Volume~"][..],
        &["Get-~", "Volume~", "Info~"][..],
        &["Clear-~", "Encryption~", "Keys~"][..],
    ]
    .iter()
    .any(|parts| matches_parts(feature_id, parts));
    if lifecycle && version.unwrap_or(0) < wincmd_shared::vault_inventory::VAULT_RUNTIME_VERSION {
        return Err("vault_runtime_update_required".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_pro_is_rejected_before_vault_commands_can_change_mounts() {
        for command in [
            "Create-EncryptionVolume",
            "New-EncryptedVolume",
            "Mount-EncryptionVolume",
            "Dismount-EncryptionVolume",
            "Dismount-AllEncryptionVolumes",
            "Get-EncryptedVolumeStatus",
            "Open-EncryptionVolume",
            "Get-VolumeInfo",
            "Clear-EncryptionKeys",
            "Mount-EncryptedVolume",
            "Dismount-EncryptedVolume",
            "Dismount-AllEncryptedVolumes",
        ] {
            for version in [None, Some(0), Some(1), Some(2)] {
                assert_eq!(
                    require_vault_runtime(command, version).unwrap_err(),
                    "vault_runtime_update_required"
                );
            }
            assert!(require_vault_runtime(command, Some(3)).is_ok());
        }
    }

    #[test]
    fn unrelated_paid_features_do_not_require_a_vault_upgrade() {
        assert!(require_vault_runtime("Get-MeshVPNStatus", None).is_ok());
        assert!(require_vault_runtime("Get-ProductivityStatus", Some(1)).is_ok());
    }
}
