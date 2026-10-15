// SPDX-License-Identifier: AGPL-3.0-or-later
//! Value-free Explorer scrub receipts; partial output is never whole-batch success.
use crate::context_scrub_failure as failure;
use crate::file_metadata::ScrubReport;
#[path = "context_scrub_summary.rs"]
mod summary;

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
    summary: String,
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
                    summary: String::new(),
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
            summary: summary::confirmed_summary(&report),
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
        let message = if self.state == ScrubState::Completed {
            format!("{} file(s) passed metadata checks. Clean copies are in the adjacent _scrubbed folder. Originals were kept.", self.verified)
        } else {
            format!("Passed checks: {}. Failed: {}. Unsupported: {}. Metadata remains: {}. Originals kept. {} Selection not verified safe to share. Reference: {}.",
                self.verified, self.failed, self.skipped, self.residual, failure::next_action(self.code), self.code)
        };
        if self.summary.is_empty() {
            message
        } else {
            format!("{message}\n{}", self.summary)
        }
    }
}

#[cfg(test)]
#[path = "context_scrub_outcome_tests.rs"]
mod tests;
