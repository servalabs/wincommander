// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn mount_attestation_is_bounded_and_cannot_supply_authority_or_paths() {
    let mut request = VaultCloudBackupAttestationRequest {
        personal: true,
        internal_drive: 12,
        expected_engine_mount_identity: "veracrypt:example:17".into(),
    };
    assert!(request.valid());
    for identity in [String::new(), "a".repeat(193), "generation\n".into()] {
        request.expected_engine_mount_identity = identity;
        assert!(!request.valid());
    }
    request.expected_engine_mount_identity = "generation".into();
    request.internal_drive = 26;
    assert!(!request.valid());
    request.internal_drive = 12;
    for field in ["owner_sid", "drive_letter", "volume_identity", "executable"] {
        let mut wire = serde_json::to_value(&request).unwrap();
        wire[field] = "untrusted".into();
        assert!(serde_json::from_value::<VaultCloudBackupAttestationRequest>(wire).is_err());
    }
}

fn request(action: VaultCloudBackupAction) -> VaultCloudBackupRequest {
    VaultCloudBackupRequest {
        personal: true,
        internal_drive: 8,
        action,
        expected_mount_receipt: None,
        configuration: None,
        restore_relative_path: None,
    }
}

#[test]
fn requires_mount_attestation_for_every_mutation_and_rejects_extra_inputs() {
    assert!(request(VaultCloudBackupAction::Status).valid());
    for action in [
        VaultCloudBackupAction::Prepare,
        VaultCloudBackupAction::Connect,
        VaultCloudBackupAction::Start,
        VaultCloudBackupAction::Stop,
        VaultCloudBackupAction::Verify,
        VaultCloudBackupAction::Disconnect,
        VaultCloudBackupAction::RemoveSetup,
    ] {
        let mut value = request(action);
        assert!(!value.valid());
        value.expected_mount_receipt = Some("a".repeat(64));
        assert!(value.valid());
        value.restore_relative_path = Some("Documents".into());
        assert!(!value.valid());
    }
    let mut wire = serde_json::to_value(request(VaultCloudBackupAction::Status)).unwrap();
    wire["executable"] = "C:\\other.exe".into();
    assert!(serde_json::from_value::<VaultCloudBackupRequest>(wire).is_err());
}

#[test]
fn rejects_reserved_paths_overlapping_sources_and_ambiguous_configuration() {
    let mut config = VaultCloudBackupConfiguration {
        whole_vault: false,
        source_relative_paths: vec!["Documents".into()],
        remote_folder_name: "Personal backup".into(),
        automatic: true,
    };
    assert!(config.valid());
    for path in [
        "..\\Documents",
        "C:\\Documents",
        ".wincommander\\cloud-backup",
        "$RECYCLE.BIN",
        "System Volume Information",
        "Documents.\\Secrets",
        "Documents ",
    ] {
        config.source_relative_paths = vec![path.into()];
        assert!(!config.valid(), "{path}");
    }
    for paths in [
        vec!["Documents", "documents/Child"],
        vec!["Documents", "DOCUMENTS"],
    ] {
        config.source_relative_paths = paths.iter().map(|s| s.to_string()).collect();
        assert!(!config.valid());
    }
    config.source_relative_paths.clear();
    assert!(!config.valid());
    config.whole_vault = true;
    assert!(config.valid());
    config.remote_folder_name = "../Other".into();
    assert!(!config.valid());
}

#[test]
fn restore_target_must_stay_inside_the_vault() {
    let mut value = request(VaultCloudBackupAction::Restore);
    value.expected_mount_receipt = Some("b".repeat(64));
    assert!(!value.valid());
    value.restore_relative_path = Some("Restored\\New folder".into());
    assert!(value.valid());
    value.restore_relative_path = Some("..\\Outside".into());
    assert!(!value.valid());
}

#[test]
fn rejects_false_running_state_and_receipt_with_failures_marked_complete() {
    let mut status = VaultCloudBackupResult::default();
    assert!(status.valid());
    status.running = true;
    assert!(!status.valid());
    status.running = false;
    status.last_run = Some(VaultCloudBackupReceipt {
        action: None,
        remote_folder_name: None,
        restore_relative_path: None,
        confirmation: None,
        checked_files: 0,
        run_id: "run-1".into(),
        outcome: VaultCloudBackupOutcome::Complete,
        started_at: "2026-11-16T12:00:00Z".into(),
        finished_at: None,
        transferred_items: 1,
        transferred_bytes: "1".into(),
        skipped_items: 0,
        failed_items: 1,
    });
    assert!(!status.valid());
    status.last_run.as_mut().unwrap().outcome = VaultCloudBackupOutcome::Partial;
    assert!(status.valid());
}

#[test]
fn confirmation_requires_completed_matching_operation_and_destination() {
    let mut run: VaultCloudBackupReceipt = serde_json::from_value(serde_json::json!({
        "run_id":"run-1", "outcome":"complete", "started_at":"2026-11-17T12:00:00Z",
        "finished_at":"2026-11-17T12:01:00Z", "transferred_items":1,
        "transferred_bytes":"1", "skipped_items":0, "failed_items":0
    }))
    .unwrap();
    assert!(run.valid());
    assert!(run.action.is_none());
    run.action = Some(VaultCloudBackupAction::Restore);
    run.remote_folder_name = Some("Test backup".into());
    run.restore_relative_path = Some("Restored check".into());
    run.confirmation = Some(VaultCloudBackupConfirmation::RestoredSampleMatched);
    run.checked_files = 1;
    assert!(run.valid());
    run.action = Some(VaultCloudBackupAction::Start);
    assert!(!run.valid());
    run.action = Some(VaultCloudBackupAction::Restore);
    run.checked_files = 0;
    assert!(!run.valid());
    run.checked_files = 1;
    run.finished_at = None;
    assert!(!run.valid());
    run.finished_at = Some("2026-11-17T12:01:00Z".into());
    run.outcome = VaultCloudBackupOutcome::Partial;
    assert!(!run.valid());
    run.outcome = VaultCloudBackupOutcome::Complete;
    let mut result = VaultCloudBackupResult {
        last_restore: Some(run.clone()),
        ..Default::default()
    };
    assert!(result.valid());
    result.last_backup = Some(run);
    assert!(!result.valid());
}
#[test]
fn old_status_payload_defaults_live_checks_to_unknown() {
    let original = VaultCloudBackupResult::default();
    let mut value = serde_json::to_value(&original).unwrap();
    let object = value.as_object_mut().unwrap();
    for key in ["connection_check", "remote_check", "checked_at", "remote_path"] { object.remove(key); }
    let restored: VaultCloudBackupResult = serde_json::from_value(value).unwrap();
    assert_eq!(restored.connection_check, VaultCloudConnectionCheck::Unknown);
    assert_eq!(restored.remote_check, VaultCloudRemoteCheck::Unknown);
    assert!(restored.checked_at.is_none() && restored.remote_path.is_none());
    assert!(restored.valid());
}
