// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

const BOOT: &str = "12345678-1234-5678-9abc-def012345678/00000039";
const NEXT_BOOT: &str = "12345678-1234-5678-9abc-def012345678/0000003a";

#[test]
fn a_service_restart_in_the_same_boot_cannot_activate_replacement_bytes() {
    let hash = "ab".repeat(32);
    let pending = ActivationRecord::new(&hash, BOOT, ActivationStatus::PendingReboot).unwrap();
    for _ in 0..3 {
        assert_eq!(
            running_action(Some(&pending), &hash, BOOT),
            RunningAction::RestartRequired
        );
    }
    assert_eq!(
        running_action(Some(&pending), &hash, NEXT_BOOT),
        RunningAction::ConfirmActive
    );
}

#[test]
fn unknown_loaded_image_and_changed_payload_require_new_activation_evidence() {
    let hash = "ab".repeat(32);
    let active = ActivationRecord::new(&hash, BOOT, ActivationStatus::Active).unwrap();
    assert_eq!(
        running_action(None, &hash, BOOT),
        RunningAction::RecordPending
    );
    assert_eq!(
        running_action(Some(&active), &"cd".repeat(32), NEXT_BOOT),
        RunningAction::RecordPending
    );
    assert_eq!(
        running_action(Some(&active), &hash, BOOT),
        RunningAction::ConfirmActive
    );
    assert_eq!(
        running_action(Some(&active), &hash, NEXT_BOOT),
        RunningAction::ConfirmActive
    );
}

#[test]
fn legacy_active_records_migrate_to_pending_and_require_a_later_kernel_boot() {
    let hash = "ab".repeat(32);
    for status in [ActivationStatus::PendingReboot, ActivationStatus::Active] {
        let legacy = ActivationRecord {
            schema_version: 1,
            sha256: hash.clone(),
            boot_id: "12345678-1234-5678-9abc-def012345678".into(),
            status,
        };
        let legacy = ActivationRecord::parse(&legacy.encode().unwrap()).unwrap();
        assert_eq!(
            running_action(Some(&legacy), &hash, BOOT),
            RunningAction::RecordPending
        );
        let migrated = ActivationRecord::new(&hash, BOOT, ActivationStatus::PendingReboot).unwrap();
        assert_eq!(migrated.schema_version, 2);
        assert_eq!(
            running_action(Some(&migrated), &hash, BOOT),
            RunningAction::RestartRequired
        );
        assert_eq!(
            running_action(Some(&migrated), &hash, NEXT_BOOT),
            RunningAction::ConfirmActive
        );
    }
}

#[test]
fn sequence_reset_and_another_loader_never_confirm_activation() {
    let hash = "ab".repeat(32);
    for status in [ActivationStatus::PendingReboot, ActivationStatus::Active] {
        let record = ActivationRecord::new(&hash, BOOT, status).unwrap();
        for changed in [
            "12345678-1234-5678-9abc-def012345678/00000038",
            "12345678-1234-5678-9abc-def012345679/0000003a",
        ] {
            assert_eq!(
                running_action(Some(&record), &hash, changed),
                RunningAction::RecordPending
            );
        }
    }
}

#[test]
fn read_only_kernel_boot_identity_is_valid_and_stable() {
    let first = current_boot_id().expect("kernel boot identity must be readable");
    let second = current_boot_id().expect("kernel boot identity must remain readable");
    assert!(
        first == second,
        "kernel boot identity changed between consecutive reads"
    );
}
