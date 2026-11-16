// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn missing_cloud_consent_never_allows_a_write_member_to_export() {
    assert!(!policy_permits_backup(
        None,
        Some("owner"),
        "owner",
        true,
        Some(VaultAccess::Write)
    ));
}

#[test]
fn browser_login_keeps_its_lease_without_claiming_a_backup_is_running() {
    let mut pending = ResultView::default();
    pending.reason_code = Some("cloud_backup_sign_in_pending".into());
    assert!(!pending.running);
    assert!(supervisor::should_renew(&pending));
    pending.reason_code = Some("cloud_backup_not_signed_in".into());
    assert!(!supervisor::should_renew(&pending));
    assert!(!supervisor::should_renew(&ResultView::default()));
}

#[test]
fn service_never_calls_private_backup_without_an_authenticated_token() {
    let broker = crate::vault_mount::policy_edit_test_broker("test-backup", "test-identity");
    let store = crate::vault_access::test_policy_store();
    let request = backup_request(&example_mount(), Action::Status);
    let result = broker.manage_cloud_backup(
        &store,
        41,
        &request,
        std::ptr::null_mut(),
        7,
        "S-1-5-21-owner",
    );
    assert!(matches!(result, Err(VaultMountReason::NotAuthorized)));
}

#[test]
fn syncthing_cannot_enroll_the_backup_session_directory_or_its_parent() {
    for path in [
        ".wincommander",
        ".WINCOMMANDER\\cloud-backup",
        ".wincommander/cloud-backup/u-test",
        ".wincommander.\\cloud-backup ",
    ] {
        assert!(
            !valid_relative_sync_path(path),
            "session state must never enter sync: {path}"
        );
    }
    for path in [
        "Documents",
        ".wincommander/other-settings",
        "Documents/cloud-backup",
    ] {
        assert!(
            valid_relative_sync_path(path),
            "unrelated child remains a valid sync source: {path}"
        );
    }
}

#[test]
fn owner_only_does_not_grant_other_write_members_backup_access() {
    assert!(policy_permits_backup(
        Some(VaultCloudBackupPolicy::OwnerOnly),
        Some("owner"),
        "owner",
        true,
        Some(VaultAccess::Write)
    ));
    assert!(!policy_permits_backup(
        Some(VaultCloudBackupPolicy::OwnerOnly),
        Some("owner"),
        "member",
        true,
        Some(VaultAccess::Write)
    ));
    assert!(!policy_permits_backup(
        Some(VaultCloudBackupPolicy::OwnerOnly),
        None,
        "owner",
        true,
        Some(VaultAccess::Write)
    ));
}

#[test]
fn write_members_still_require_a_current_write_grant() {
    assert!(policy_permits_backup(
        Some(VaultCloudBackupPolicy::WriteMembers),
        Some("owner"),
        "member",
        true,
        Some(VaultAccess::Write)
    ));
    for access in [Some(VaultAccess::Read), None] {
        assert!(!policy_permits_backup(
            Some(VaultCloudBackupPolicy::WriteMembers),
            Some("owner"),
            "member",
            true,
            access
        ));
    }
    assert!(!policy_permits_backup(
        Some(VaultCloudBackupPolicy::WriteMembers),
        Some("owner"),
        "member",
        false,
        Some(VaultAccess::Write)
    ));
}

#[test]
fn status_and_mutations_reject_untrusted_mount_authority_fields() {
    let request = serde_json::json!({"personal":true,"internal_drive":4,"action":"status","expected_mount_receipt":null,"configuration":null,"restore_relative_path":null,"owner_sid":"other"});
    assert!(serde_json::from_value::<Request>(request).is_err());
    let mut request = backup_request(&example_mount(), Action::Start);
    assert!(!request.valid());
    request.expected_mount_receipt = Some("a".repeat(64));
    assert!(request.valid());
}

fn example_mount() -> ActiveMount {
    ActiveMount {
        drive_letter: "V".into(),
        internal_drive: 21,
        presentation: VaultPresentation::Machine,
        session_id: 7,
        authentication_id: Some((11, 12)),
        caller_sid: "S-1-5-21-owner".into(),
        policy_id: "personal".into(),
        policy_version: 1,
        personal: true,
        syncthing_managed: false,
        syncthing_resume_unresolved: false,
        container_identity: "example-identity".into(),
        access: VaultAccess::Write,
        mounted_at: 99,
        cleanup_required: false,
        driver_slot_absent: false,
        engine_mount_identity: Some("driver-a".into()),
        canonical_container_path: None,
    }
}

#[test]
fn mount_receipts_change_when_slot_is_reused_or_logon_changes() {
    let original = example_mount();
    let receipt = sync_management::mount_receipt("id", &original);
    for replacement in [
        ActiveMount {
            container_identity: "replacement".into(),
            ..original.clone()
        },
        ActiveMount {
            authentication_id: Some((13, 12)),
            ..original.clone()
        },
        ActiveMount {
            mounted_at: 100,
            ..original.clone()
        },
    ] {
        assert!(
            sync_management::require_mount_receipt(Some(&receipt), "id", &replacement, true)
                .is_err()
        );
    }
}
