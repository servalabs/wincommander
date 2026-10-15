// SPDX-License-Identifier: AGPL-3.0-or-later
//! Anonymous, finite-vocabulary receipts for confirmed scrub outputs only.
use crate::file_metadata::{ScrubReport, ScrubResult};
use std::collections::BTreeSet;

#[derive(Default)]
struct Kinds {
    removed: BTreeSet<&'static str>,
    timestamps_changed: bool,
}

impl Kinds {
    fn include(&mut self, file: &ScrubResult) {
        for field in file
            .fields_stripped
            .iter()
            .filter(|field| field.is_identifying)
        {
            for detail in &field.details {
                // Labels can contain stream names; unknown details can be tag
                // names chosen by a document. Neither is presentation text.
                let kind = match detail.as_str() {
                    "GPS location coordinates" => "GPS location",
                    "Device serial number" => "device serial",
                    "Camera or device model" => "device model",
                    "Lens information" => "lens information",
                    "Capture or edit date and time" => "capture/edit dates",
                    "Author, account, or organisation identity"
                    | "Document author (Creator)"
                    | "Last modified by"
                    | "Company"
                    | "Manager" => "author/account/org",
                    "Copyright or owner information" => "copyright/owner",
                    "Location or address" => "location/address",
                    "Comments, captions, or description" => "comments/captions",
                    "Keywords, subject, or rating" => "keywords/subject",
                    "Website or source link" => "source links",
                    "Embedded preview image" | "Embedded document preview" => "embedded previews",
                    "Custom document properties" => "custom properties",
                    "Revision author, account, or edit date" => "revision authors/dates",
                    "Windows tracking or download marker" => "Windows tracking",
                    "Title" => "document title",
                    "Producer" | "Software" => "software information",
                    "File creation, modification, or access time" => {
                        self.timestamps_changed = true;
                        continue;
                    }
                    _ => continue,
                };
                self.removed.insert(kind);
            }
        }
    }

    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.removed.is_empty() {
            parts.push(format!(
                "Removed: {}",
                self.removed.iter().copied().collect::<Vec<_>>().join(", ")
            ));
        }
        if self.timestamps_changed {
            parts.push("Changed: file timestamps".into());
        }
        if parts.is_empty() {
            "No removal details confirmed".into()
        } else {
            parts.join(". ")
        }
    }
}

pub(super) fn confirmed_summary(report: &ScrubReport) -> String {
    let observed_residual = report
        .scrubbed
        .iter()
        .filter(|file| !file.residual_fields.is_empty())
        .count();
    // An aggregate-only warning cannot identify which seemingly clean output
    // is affected. Keep the existing failure receipt, without assigning kinds.
    if report.residual_count as usize > observed_residual {
        return String::new();
    }
    let mut lines = Vec::new();
    let mut additional = Kinds::default();
    let mut additional_count = 0usize;
    for (index, file) in report.scrubbed.iter().enumerate() {
        if file.dry_run || file.output_path.is_empty() || !file.residual_fields.is_empty() {
            continue;
        }
        if lines.len() < 3 {
            let mut kinds = Kinds::default();
            kinds.include(file);
            lines.push(format!("File {} — {}.", index + 1, kinds.describe()));
        } else {
            additional.include(file);
            additional_count += 1;
        }
    }
    if additional_count > 0 {
        lines.push(format!(
            "{additional_count} more verified file(s) — {}.",
            additional.describe()
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "context_scrub_summary_tests.rs"]
mod tests;
