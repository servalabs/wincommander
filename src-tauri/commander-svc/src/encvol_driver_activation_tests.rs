// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

const BOOT: &str = "12345678-1234-5678-9abc-def012345678";
const NEXT_BOOT: &str = "12345678-1234-5678-9abc-def012345679";

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
fn kernel_boot_environment_layout_matches_windows_abi() {
    assert_eq!(std::mem::size_of::<BootEnvironment>(), 32);
    assert_eq!(std::mem::offset_of!(BootEnvironment, firmware_type), 16);
    assert_eq!(std::mem::offset_of!(BootEnvironment, boot_flags), 24);
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
