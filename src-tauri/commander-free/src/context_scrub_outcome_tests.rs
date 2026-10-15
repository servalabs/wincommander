// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::file_metadata::{ScrubError, ScrubResult, StrippedField};
fn report() -> ScrubReport {
    ScrubReport {
        scrubbed: vec![ScrubResult {
            input_path: "private-original".into(),
            output_path: "private-clean-copy".into(),
            file_type: "pdf".into(),
            bytes_in: 1,
            bytes_out: 1,
            fields_stripped: vec![],
            gps_coords: None,
            sample_values: vec!["secret-author-value".into()],
            dry_run: false,
            residual_fields: vec![],
        }],
        ..Default::default()
    }
}
#[test]
fn only_verified_nonempty_full_output_is_success() {
    assert_eq!(ScrubOutcome::from_result(Ok(report())).exit_code(), 0);
    assert_eq!(
        ScrubOutcome::from_result(Ok(ScrubReport::default())).exit_code(),
        1
    );
    for invalid in [true, false] {
        let mut report = report();
        if invalid {
            report.scrubbed[0].dry_run = true;
        } else {
            report.scrubbed[0].output_path.clear();
        }
        assert_eq!(
            ScrubOutcome::from_result(Ok(report)).state,
            ScrubState::Failed
        );
    }
}
#[test]
fn mixed_outputs_keep_counts_without_exposing_paths_or_raw_errors() {
    let mut report = report();
    report.errors.push(ScrubError {
        input_path: "private-failed-file".into(),
        message: "secret-helper-value".into(),
    });
    report.skipped_count = 1;
    let outcome = ScrubOutcome::from_result(Ok(report));
    assert_eq!(outcome.state, ScrubState::Partial);
    assert_eq!(outcome.exit_code(), 1);
    let message = outcome.message();
    assert!(message.contains("Passed checks: 1"));
    assert!(message.contains("Failed: 1"));
    assert!(message.contains("Unsupported: 1"));
    for private in ["private", "secret"] {
        assert!(!message.contains(private));
    }
}
#[test]
fn residual_fields_fail_closed_even_if_aggregate_count_is_wrong() {
    let mut report = report();
    report.scrubbed[0].residual_fields.push(StrippedField {
        category: "pdfInfo".into(),
        label: "Document properties".into(),
        details: vec!["Author".into()],
        bytes: 1,
        is_identifying: true,
    });
    let outcome = ScrubOutcome::from_result(Ok(report));
    assert_eq!(outcome.state, ScrubState::Failed);
    assert!(outcome.message().contains("Passed checks: 0"));
    assert!(outcome.message().contains("Metadata remains: 1"));
    assert_eq!(outcome.exit_code(), 1);
}
#[test]
fn helper_failure_has_a_bounded_failed_receipt() {
    let outcome = ScrubOutcome::from_result(Err("private path and helper error".into()));
    assert_eq!(outcome.state, ScrubState::Failed);
    assert!(!outcome.message().contains("private path"));
    assert_eq!(outcome.severity(), "danger");
}

#[test]
fn distinct_failures_are_not_misreported_as_one_cause() {
    let mut report = report();
    for message in ["Read failed: private-input", "Write failed: private-output"] {
        report.errors.push(ScrubError {
            input_path: "private-selection".into(),
            message: message.into(),
        });
    }
    let outcome = ScrubOutcome::from_result(Ok(report));
    assert_eq!(outcome.state, ScrubState::Partial);
    assert!(outcome.diagnostic().contains("code=multiple_failures"));
    assert!(!outcome.diagnostic().contains("private"));
    assert!(!outcome.message().contains("private"));
    assert_eq!(outcome.exit_code(), 1);
}

#[test]
fn partial_receipt_describes_only_confirmed_files_and_keeps_the_warning() {
    let mut report = report();
    report.scrubbed[0].fields_stripped.push(StrippedField {
        category: "private-category".into(),
        label: "private-label".into(),
        details: vec!["Document author (Creator)".into()],
        bytes: 1,
        is_identifying: true,
    });
    let mut unsafe_file = report.scrubbed[0].clone();
    unsafe_file.residual_fields = unsafe_file.fields_stripped.clone();
    report.scrubbed.push(unsafe_file);
    let outcome = ScrubOutcome::from_result(Ok(report));
    let message = outcome.message();
    assert_eq!(outcome.state, ScrubState::Partial);
    assert_eq!(outcome.exit_code(), 1);
    assert!(message.contains("File 1 — Removed: author/account/org."));
    assert!(!message.contains("File 2"));
    assert!(message.contains("Selection not verified safe to share"));
    assert!(message.contains("Metadata remains: 1"));
    assert!(!message.contains("private"));
}

#[test]
fn completed_receipt_without_details_does_not_claim_metadata_was_absent() {
    let outcome = ScrubOutcome::from_result(Ok(report()));
    assert_eq!(outcome.exit_code(), 0);
    assert!(outcome
        .message()
        .contains("File 1 — No removal details confirmed."));
    assert!(!outcome.message().contains("No metadata"));
    assert!(!outcome.message().contains("Removed:"));
}
