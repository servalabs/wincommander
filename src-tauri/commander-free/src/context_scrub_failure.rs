// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fixed outcome codes; helper messages and selected paths never leave this boundary.
pub(super) fn failure_code(error: &str) -> &'static str {
    if matches!(
        error,
        "scrub_selection_missing" | "scrub_selected_item_missing"
    ) {
        "selection_unavailable"
    } else if error == "scrub_start_failed" {
        "runtime_unavailable"
    } else if error.starts_with("WinCommander Pro entitlement required for:") {
        "entitlement_unverified"
    } else if error.starts_with("PRO_NOT_INSTALLED:") {
        "helper_unavailable"
    } else if error.starts_with("PRO_UPDATE_IN_PROGRESS:") {
        "helper_updating"
    } else if error.starts_with("Pro spawn failed:") {
        "helper_start_failed"
    } else if error.starts_with("Metadata scrub could not finish safely and was not retried:") {
        "helper_completion_unconfirmed"
    } else if error.starts_with("Create output dir failed:") || error.starts_with("Write failed:") {
        "output_write_failed"
    } else if error.starts_with("Read failed:") || error == "Read dir failed" {
        "input_read_failed"
    } else if error.starts_with("PDF full rewrite failed:") {
        "pdf_rewrite_unconfirmed"
    } else if error.starts_with("PDF post-scrub verification found identifying metadata") {
        "metadata_remains"
    } else if error.starts_with("ExifTool post-scrub verification") {
        "verification_unconfirmed"
    } else {
        "processing_unconfirmed"
    }
}

pub(super) fn next_action(code: &str) -> &'static str {
    match code {
        "selection_unavailable" => "Select an existing local file and retry.",
        "entitlement_unverified" => {
            "Open WinCommander and verify the Pro license or trial before retrying."
        }
        "helper_unavailable" | "helper_start_failed" => {
            "Open WinCommander and check the Pro installation before retrying."
        }
        "helper_completion_unconfirmed" => {
            "Wait for scrubbing to stop, then review its output in Share Safely before retrying."
        }
        "helper_updating" => "Wait for the Pro update to finish, then retry.",
        "input_read_failed" => {
            "Check that the selected files are readable and not locked, then retry."
        }
        "output_write_failed" => "Check write access beside the selected files, then retry.",
        "pdf_rewrite_unconfirmed" => {
            "Open Share Safely and check the PDF rewrite engine before retrying."
        }
        _ => "Review in Share Safely before retrying.",
    }
}

pub(super) fn preference_code(error: &str) -> &'static str {
    if error == "notification_preferences_schema_invalid" {
        "preferences_schema_invalid"
    } else if error == "notification_preferences_record_unverified" {
        "preferences_record_unverified"
    } else if error == "Service identity could not be verified" {
        "preferences_service_identity_unverified"
    } else if error.starts_with("service connect failed:") || error == "service connection failed" {
        "preferences_service_unavailable"
    } else if error.starts_with("service rejected request:") {
        "preferences_service_rejected"
    } else if matches!(
        error,
        "service reply timed out"
            | "service Hello acknowledgement timed out"
            | "service Hello write timed out"
    ) {
        "preferences_service_timeout"
    } else {
        "preferences_unverified"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn error_details_never_become_codes_or_next_actions() {
        let private = "C:\\Users\\private-user\\private.pdf token=private-secret";
        for prefix in [
            "",
            "Write failed:",
            "Create output dir failed:",
            "Read failed:",
            "PDF full rewrite failed:",
            "PDF post-scrub verification found identifying metadata",
            "ExifTool post-scrub verification",
            "WinCommander Pro entitlement required for:",
            "PRO_NOT_INSTALLED:",
            "Pro spawn failed:",
            "Metadata scrub could not finish safely and was not retried:",
        ] {
            let code = failure_code(&format!("{prefix}{private}"));
            assert!(!code.contains("private"));
            assert!(!next_action(code).contains("private"));
        }
        assert_eq!(
            failure_code("unknown helper message"),
            "processing_unconfirmed"
        );
        assert_eq!(
            failure_code("untrusted text Write failed:"),
            "processing_unconfirmed"
        );
        assert_eq!(
            preference_code("private helper timed out"),
            "preferences_unverified"
        );
        assert_eq!(
            preference_code("service reply timed out"),
            "preferences_service_timeout"
        );
        for prefix in ["", "service connect failed:", "service rejected request:"] {
            assert!(!preference_code(&format!("{prefix}{private}")).contains("private"));
        }
    }
}
