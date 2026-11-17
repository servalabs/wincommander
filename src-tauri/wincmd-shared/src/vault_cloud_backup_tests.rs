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
