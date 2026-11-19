// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn personal_backup_accepts_only_same_logon_or_its_linked_uac_token() {
    let mount = example_mount();
    let owner = "S-1-5-21-owner";
    assert!(same_backup_mount_owner(&mount, 7, owner, Some((11, 12)), None));
    assert!(same_backup_mount_owner(&mount, 7, owner, Some((21, 22)), Some((11, 12))));
    assert!(!same_backup_mount_owner(&mount, 7, owner, Some((21, 22)), None));
    assert!(!same_backup_mount_owner(&mount, 7, owner, Some((21, 22)), Some((31, 32))));
    assert!(!same_backup_mount_owner(&mount, 7, owner, None, Some((11, 12))));
    assert!(!same_backup_mount_owner(&mount, 8, owner, Some((21, 22)), Some((11, 12))));
    assert!(!same_backup_mount_owner(&mount, 7, "S-1-5-21-other", Some((21, 22)), Some((11, 12))));
}

#[test]
fn backup_presentation_distinguishes_unavailable_drive_from_denied_access() {
    assert_eq!(backup_presentation_result(CallerPresentationAttestation::Available), Ok(()));
    for attestation in [CallerPresentationAttestation::MappingUnavailable, CallerPresentationAttestation::RootReadFailed] {
        assert_eq!(backup_presentation_result(attestation), Err(VaultMountReason::PresentationRejected));
    }
    for attestation in [CallerPresentationAttestation::RootAccessDenied, CallerPresentationAttestation::RootWriteAccessDenied] {
        assert_eq!(backup_presentation_result(attestation), Err(VaultMountReason::CallerAccessDenied));
    }
}

#[test]
fn mount_attestation_requires_a_live_authenticated_caller_before_inspection() {
    use wincmd_shared::vault_cloud_backup::VaultCloudBackupAttestationRequest;
    let broker = crate::vault_mount::policy_edit_test_broker("test-backup", "test-identity");
    let store = crate::vault_access::test_policy_store();
    let mut request = VaultCloudBackupAttestationRequest {
        personal: true, internal_drive: 12,
        expected_engine_mount_identity: "driver-generation".into(),
    };
    assert!(matches!(broker.attest_cloud_backup(&store, &request, std::ptr::null_mut(), 7, "S-1-5-21-owner"), Err(VaultMountReason::NotAuthorized)));
    // Session zero is rejected before an untrusted token can reach any Win32 query.
    assert!(matches!(broker.attest_cloud_backup(&store, &request, 1usize as _, 0, "S-1-5-21-owner"), Err(VaultMountReason::NotAuthorized)));
    request.expected_engine_mount_identity.clear();
    assert!(matches!(broker.attest_cloud_backup(&store, &request, std::ptr::null_mut(), 7, "S-1-5-21-owner"), Err(VaultMountReason::InvalidRequest)));
}

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
    let mut pending = ResultView {
        reason_code: Some("cloud_backup_sign_in_pending".into()),
        ..ResultView::default()
    };
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

#[test]
fn status_snapshot_is_unbound_but_a_followup_receipt_is_verified() {
    let original = example_mount();
    let receipt = sync_management::mount_receipt("id", &original);
    assert!(sync_management::require_mount_receipt(None, "id", &original, false).is_ok());
    assert!(sync_management::require_mount_receipt(Some(&receipt), "id", &original, false).is_ok());
    let replacement = ActiveMount { container_identity: "replacement".into(), ..original };
    assert!(sync_management::require_mount_receipt(Some(&receipt), "id", &replacement, false).is_err());
}
