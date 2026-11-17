// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn telemetry(sequence: u32) -> Vec<u8> {
    let mut bytes = vec![0u8; TELEMETRY_HEADER_BYTES];
    bytes[0..4].copy_from_slice(&(TELEMETRY_HEADER_BYTES as u32).to_le_bytes());
    bytes[4..8].copy_from_slice(&1234u32.to_le_bytes());
    bytes[60..64].copy_from_slice(&sequence.to_le_bytes());
    bytes
}

#[test]
fn telemetry_requires_successful_complete_self_header_and_nonzero_sequence() {
    let valid = telemetry(57);
    assert_eq!(telemetry_sequence(&valid, 96, 1234), Some(57));
    assert_eq!(telemetry_sequence(&valid, 95, 1234), None);
    assert_eq!(telemetry_sequence(&valid, 97, 1234), None);
    assert_eq!(telemetry_sequence(&valid, 96, 4321), None);
    assert_eq!(telemetry_sequence(&telemetry(0), 96, 1234), None);
    for header in [0u32, 63, 95, 97, u32::MAX] {
        let mut invalid = valid.clone();
        invalid[0..4].copy_from_slice(&header.to_le_bytes());
        assert_eq!(telemetry_sequence(&invalid, 96, 1234), None);
    }
}

#[test]
fn telemetry_resizes_bounded_queries_without_trusting_partial_failure_data() {
    let mut attempts = 0;
    let result = query_sequence(1234, |bytes, returned| {
        attempts += 1;
        bytes[..96].copy_from_slice(&telemetry(99));
        if attempts == 1 {
            *returned = 6000;
            LENGTH_MISMATCH
        } else {
            assert!(bytes.len() >= 6000);
            bytes[..96].copy_from_slice(&telemetry(57));
            *returned = 96;
            0
        }
    });
    assert_eq!(result.unwrap(), 57);
    assert_eq!(attempts, 2);
}

#[test]
fn failed_or_unbounded_telemetry_cannot_supply_boot_authority() {
    assert!(query_sequence(1234, |bytes, returned| {
        bytes[..96].copy_from_slice(&telemetry(57));
        *returned = 96;
        1
    })
    .is_err());
    for failure in [0xc0000022u32 as i32, LENGTH_MISMATCH, BUFFER_TOO_SMALL] {
        let mut attempts = 0;
        let result = query_sequence(1234, |bytes, returned| {
            attempts += 1;
            bytes[..96].copy_from_slice(&telemetry(99));
            *returned = MAX_TELEMETRY_BYTES as u32 + 1;
            failure
        });
        assert!(result.is_err());
        assert_eq!(attempts, 1);
    }
    let mut attempts = 0;
    assert!(query_sequence(1234, |_, returned| {
        attempts += 1;
        *returned = 0;
        LENGTH_MISMATCH
    })
    .is_err());
    assert_eq!(attempts, 4);
}

#[test]
fn boot_environment_layout_and_read_only_session_query_match_windows() {
    assert_eq!(std::mem::size_of::<BootEnvironment>(), 32);
    assert_eq!(std::mem::offset_of!(BootEnvironment, firmware_type), 16);
    assert_eq!(std::mem::offset_of!(BootEnvironment, boot_flags), 24);
    let first = current_boot_id().expect("read-only kernel boot session");
    let second = current_boot_id().expect("repeat read-only kernel boot session");
    assert!(first == second, "same kernel session must remain stable");
    let record =
        ActivationRecord::new(&"ab".repeat(32), &first, ActivationStatus::PendingReboot).unwrap();
    assert!(record.is_current_boot(&second));
    assert!(!record.confirms_later_boot(&second));
}
