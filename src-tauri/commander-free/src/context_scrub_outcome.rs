// SPDX-License-Identifier: AGPL-3.0-or-later
//! Value-free Explorer scrub receipts; partial output is never whole-batch success.
use crate::context_scrub_failure as failure;
use crate::file_metadata::ScrubReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrubState {
    Completed,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScrubOutcome {
    pub(crate) state: ScrubState,
    verified: usize,
    failed: usize,
    skipped: usize,
    residual: usize,
    code: &'static str,
}

impl ScrubOutcome {
    pub(crate) fn from_result(result: Result<ScrubReport, String>) -> Self {
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                return Self {
                    state: ScrubState::Failed,
                    verified: 0,
                    failed: 1,
                    skipped: 0,
                    residual: 0,
                    code: failure::failure_code(&error),
                };
            }
        };
        let observed_verified = report
            .scrubbed
            .iter()
            .filter(|file| {
                !file.dry_run && !file.output_path.is_empty() && file.residual_fields.is_empty()
            })
            .count();
        let observed_residual = report
            .scrubbed
            .iter()
            .filter(|file| !file.residual_fields.is_empty())
            .count();
        let residual = (report.residual_count as usize).max(observed_residual);
        let verified = observed_verified.min(report.scrubbed.len().saturating_sub(residual));
        let failed = report.errors.len()
            + report
                .scrubbed
                .iter()
                .filter(|file| file.dry_run || file.output_path.is_empty())
                .count();
        let skipped = (report.skipped_count as usize).max(report.skipped_files.len());
        let complete = verified > 0
            && verified == report.scrubbed.len()
            && failed == 0
            && skipped == 0
            && residual == 0;
        let mut codes = report
            .errors
            .iter()
            .map(|error| failure::failure_code(&error.message));
        let first_code = codes.next();
        let code = if first_code.is_some_and(|first| codes.any(|code| code != first)) {
            "multiple_failures"
        } else if let Some(code) = first_code {
            code
        } else if residual > 0 {
            "metadata_remains"
        } else if skipped > 0 {
            "unsupported_selection"
        } else if complete {
            "completed"
        } else {
            "processing_unconfirmed"
        };
        Self {
            state: if complete {
                ScrubState::Completed
            } else if verified > 0 {
                ScrubState::Partial
            } else {
                ScrubState::Failed
            },
            verified,
            failed,
            skipped,
            residual,
            code,
        }
    }

    pub(crate) fn exit_code(&self) -> i32 {
        if self.state == ScrubState::Completed {
            0
        } else {
            1
        }
    }
    pub(crate) fn diagnostic(&self) -> String {
        format!(
            "[ContextScrub] code={} verified={} failed={} unsupported={} residual={}",
            self.code, self.verified, self.failed, self.skipped, self.residual
        )
    }
    pub(crate) fn title(&self) -> &'static str {
        match self.state {
            ScrubState::Completed => "Metadata scrub completed",
            ScrubState::Partial => "Metadata scrub needs attention",
            ScrubState::Failed => "Metadata scrub failed",
        }
    }
    pub(crate) fn severity(&self) -> &'static str {
        match self.state {
            ScrubState::Completed => "info",
            ScrubState::Partial => "warning",
            ScrubState::Failed => "danger",
        }
    }
    pub(crate) fn message(&self) -> String {
        if self.state == ScrubState::Completed {
            return format!("{} file(s) passed metadata checks. Clean copies are in the adjacent _scrubbed folder. Originals were kept.", self.verified);
        }
        format!("Passed checks: {}. Failed: {}. Unsupported: {}. Metadata remains: {}. Originals kept. {} Selection not verified safe to share. Reference: {}.",
            self.verified, self.failed, self.skipped, self.residual, failure::next_action(self.code), self.code)
    }
}

#[cfg(test)]
mod tests {
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
}
