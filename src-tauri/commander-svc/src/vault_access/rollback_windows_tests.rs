// SPDX-License-Identifier: AGPL-3.0-or-later
#[cfg(windows)]
#[test]
fn windows_disposable_file_acl_rollback_reads_back_exact_snapshot() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "WinCommander-policy-rollback-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("disposable.txt");
    std::fs::write(&file, b"harmless rollback fixture").unwrap();
    let result = (|| {
        for protected in [false, true] {
            if protected {
                apply_one_acl(&file, &[])?;
            }
            let original = snapshot_one_acl(&file)?;
            let identity = WindowsVaultFs.stable_file_identity(&file)?;
            apply_one_acl(
                &file,
                &[ResolvedGrant {
                    sid: BUILTIN_USERS_SID.into(),
                    access: VaultAccess::Read,
                }],
            )?;
            WindowsAclApplier
                .restore_verified(std::slice::from_ref(&original), &[(file.clone(), identity)])?;
            WindowsAclApplier.verify_restored(std::slice::from_ref(&original))?;
        }
        Ok::<(), VaultError>(())
    })();
    let cleanup_file = std::fs::remove_file(&file);
    let cleanup_dir = std::fs::remove_dir(&dir);
    result.unwrap();
    cleanup_file.unwrap();
    cleanup_dir.unwrap();
}
#[cfg(windows)]
#[test]
fn windows_rollback_pins_identity_and_leaves_replacement_permissions_untouched() {
    use std::os::windows::io::AsRawHandle;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "WinCommander-policy-pinned-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("disposable.txt");
    let original_file = dir.join("original.txt");
    let moved = dir.join("moved.txt");
    std::fs::write(&file, b"harmless original").unwrap();
    let result = (|| {
        let original = snapshot_one_acl(&file)?;
        let identity = WindowsVaultFs.stable_file_identity(&file)?;
        std::fs::rename(&file, &original_file).map_err(|_| VaultError::Persistence)?;
        std::fs::write(&file, b"harmless replacement").map_err(|_| VaultError::Persistence)?;
        let replacement = snapshot_one_acl(&file)?;
        // Inject substitution after authorized identity resolution but
        // before snapshot capture. It must be rejected before any write;
        // an old DACL cannot be paired with the replacement's identity.
        let plan = VaultAclPlan {
            parent: dir.clone(),
            container: file.clone(),
            grants: Vec::new(),
            authorization_grants: Vec::new(),
            managed_groups: Vec::new(),
        };
        if !WindowsAclApplier
            .capture_policy_snapshot(&plan, &identity)
            .err()
            .is_some_and(|error| error == VaultError::ContainerIdentity)
        {
            return Err(VaultError::AclReadback);
        }
        let replaced = WindowsAclApplier
            .restore_verified(std::slice::from_ref(&original), &[(file.clone(), identity)]);
        if replaced != Err(VaultError::ContainerIdentity) {
            return Err(VaultError::AclReadback);
        }
        let unchanged = snapshot_one_acl(&file)?;
        if replacement.dacl_protected != unchanged.dacl_protected
            || !same_snapshot_dacl(&replacement.descriptor, &unchanged.descriptor)?
        {
            return Err(VaultError::AclReadback);
        }
        let current_identity = WindowsVaultFs.stable_file_identity(&file)?;
        let captured = WindowsAclApplier.capture_policy_snapshot(&plan, &current_identity)?;
        if std::fs::rename(&file, &moved).is_ok() {
            return Err(VaultError::ContainerIdentity);
        }
        if captured.identities != vec![(file.clone(), current_identity.clone())]
            || !same_snapshot_dacl(&captured.snapshots[0].descriptor, &replacement.descriptor)?
        {
            return Err(VaultError::AclReadback);
        }
        drop(captured);
        let pinned = open_policy_rollback_target(&file, &current_identity)?;
        // Uses the actual production lease; replacement is denied while
        // identity, ACL write, and readback share the same open object.
        if std::fs::rename(&file, &moved).is_ok() {
            return Err(VaultError::ContainerIdentity);
        }
        restore_handle_acl(pinned.as_raw_handle() as _, &replacement)?;
        let readback = snapshot_handle_acl(pinned.as_raw_handle() as _, &file)?;
        if !same_snapshot_dacl(&readback.descriptor, &replacement.descriptor)? {
            return Err(VaultError::AclReadback);
        }
        drop(pinned);
        std::fs::rename(&file, &moved).map_err(|_| VaultError::Persistence)?;
        std::fs::rename(&moved, &file).map_err(|_| VaultError::Persistence)?;
        Ok::<(), VaultError>(())
    })();
    for path in [&file, &original_file, &moved] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => panic!("disposable file cleanup failed"),
        }
    }
    let cleanup_dir = std::fs::remove_dir(&dir);
    result.unwrap();
    cleanup_dir.unwrap();
}
