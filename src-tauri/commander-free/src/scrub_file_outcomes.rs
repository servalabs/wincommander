// SPDX-License-Identifier: AGPL-3.0-or-later
// Per-file Share Safely outcomes: every file in a run is reported exactly once,
// with a precise reason whenever it was not cleaned.

use crate::file_metadata::{ScrubReport, ScrubResult};
use crate::scrub_content_type as content;
use crate::scrub_file_listing::list_run;
use crate::scrub_reason::{access_reason, classify_error, classify_unprocessed, Probe};
pub use crate::scrub_reason::{ScrubOutcome, ScrubReason};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

const NOT_REPORTED: &str = "Not reported by the scrub engine";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrubFileOutcome {
    pub path: String,
    pub root: String,
    pub extension: String,
    pub detected_type: Option<String>,
    pub type_mismatch: bool,
    pub cleaned_as: Option<String>,
    pub outcome: ScrubOutcome,
    pub reason: Option<ScrubReason>,
    pub message: Option<String>,
    pub output_path: Option<String>,
    pub scrubbed_index: Option<usize>,
    pub removed_categories: Vec<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrubRootSummary {
    pub root: String,
    pub is_directory: bool,
    pub exists: bool,
    pub total: u32,
    pub cleaned: u32,
    pub already_clean: u32,
    pub residual: u32,
    pub not_cleaned: u32,
    pub failed: u32,
}

struct Indexes {
    scrubbed: HashMap<String, usize>,
    errors: HashMap<String, String>,
    skipped: HashSet<String>,
}

fn path_key(path: &str) -> String {
    path.trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_lowercase()
}

fn owning_root(paths: &[String], file: &str) -> String {
    let key = path_key(file);
    paths
        .iter()
        .filter(|root| {
            let r = path_key(root);
            key == r || key.starts_with(&format!("{}\\", r.trim_end_matches('\\')))
        })
        .max_by_key(|root| root.len())
        .cloned()
        .unwrap_or_else(|| file.to_string())
}

fn outcome_for_result(r: &ScrubResult) -> (ScrubOutcome, Option<ScrubReason>) {
    if !r.residual_fields.is_empty() {
        (ScrubOutcome::Residual, None)
    } else if r.fields_stripped.is_empty() {
        (
            ScrubOutcome::AlreadyClean,
            Some(ScrubReason::NothingToRemove),
        )
    } else {
        (ScrubOutcome::Cleaned, None)
    }
}

fn blank(path: &Path, root: String) -> ScrubFileOutcome {
    ScrubFileOutcome {
        path: path.to_string_lossy().to_string(),
        root,
        extension: path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase(),
        detected_type: None,
        type_mismatch: false,
        cleaned_as: None,
        outcome: ScrubOutcome::NotCleaned,
        reason: None,
        message: None,
        output_path: None,
        scrubbed_index: None,
        removed_categories: Vec::new(),
        dry_run: false,
    }
}

fn describe(path: &Path, root: String, report: &ScrubReport, index: &Indexes) -> ScrubFileOutcome {
    let mut item = blank(path, root);
    let key = path_key(&item.path);
    let detected = content::detect_file(path);
    item.detected_type = detected.map(str::to_string);
    item.type_mismatch = content::is_mismatch(&item.extension, detected);
    if let Some(&i) = index.scrubbed.get(&key) {
        let r = &report.scrubbed[i];
        (item.outcome, item.reason) = outcome_for_result(r);
        item.scrubbed_index = Some(i);
        item.removed_categories = r
            .fields_stripped
            .iter()
            .map(|f| f.category.clone())
            .collect();
        item.output_path = Some(r.output_path.clone()).filter(|p| !p.is_empty());
        item.dry_run = r.dry_run;
        return item;
    }
    let probe = Probe::of(path);
    let (outcome, reason) = match index.errors.get(&key) {
        Some(message) => {
            item.message = Some(message.clone());
            classify_error(message, &item.extension, probe)
        }
        None => {
            if !index.skipped.contains(&key) {
                item.message = Some(NOT_REPORTED.to_string());
            }
            classify_unprocessed(&item.extension, detected, probe)
        }
    };
    (item.outcome, item.reason) = (outcome, Some(reason));
    item
}

fn unreadable_folder(path: &Path, root: String, code: i32) -> ScrubFileOutcome {
    let mut item = blank(path, root);
    item.extension = String::new();
    item.detected_type = Some("folder".into());
    item.outcome = ScrubOutcome::Failed;
    item.reason =
        Some(access_reason(Some(code), item.path.len()).unwrap_or(ScrubReason::ReadFailed));
    item.message = Some(format!("Folder could not be read (os error {code})"));
    item
}

/// Build the per-file and per-root view of a finished run. Files are listed
/// with the same rules the engine uses so a file it dropped is still named.
pub fn build(
    paths: &[String],
    recursive: bool,
    report: &ScrubReport,
) -> (Vec<ScrubFileOutcome>, Vec<ScrubRootSummary>) {
    let index = Indexes {
        scrubbed: report
            .scrubbed
            .iter()
            .enumerate()
            .map(|(i, r)| (path_key(&r.input_path), i))
            .collect(),
        errors: report
            .errors
            .iter()
            .map(|e| (path_key(&e.input_path), e.message.clone()))
            .collect(),
        skipped: report.skipped_files.iter().map(|p| path_key(p)).collect(),
    };
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for listed in list_run(paths, recursive) {
        if !seen.insert(path_key(&listed.path.to_string_lossy())) {
            continue;
        }
        files.push(match listed.unreadable_dir {
            Some(code) => unreadable_folder(&listed.path, listed.root, code),
            None => describe(&listed.path, listed.root, report, &index),
        });
    }
    let reported = report
        .scrubbed
        .iter()
        .map(|r| &r.input_path)
        .chain(report.errors.iter().map(|e| &e.input_path))
        .chain(report.skipped_files.iter());
    for path in reported {
        if seen.insert(path_key(path)) {
            files.push(describe(
                Path::new(path),
                owning_root(paths, path),
                report,
                &index,
            ));
        }
    }
    let roots = summarize(paths, &files);
    (files, roots)
}

fn summarize(paths: &[String], files: &[ScrubFileOutcome]) -> Vec<ScrubRootSummary> {
    paths
        .iter()
        .map(|root| {
            let meta = fs::metadata(root).ok();
            let mut s = ScrubRootSummary {
                root: root.clone(),
                is_directory: meta.as_ref().is_some_and(|m| m.is_dir()),
                exists: meta.is_some(),
                ..Default::default()
            };
            for f in files.iter().filter(|f| &f.root == root) {
                s.total += 1;
                *match f.outcome {
                    ScrubOutcome::Cleaned => &mut s.cleaned,
                    ScrubOutcome::AlreadyClean => &mut s.already_clean,
                    ScrubOutcome::Residual => &mut s.residual,
                    ScrubOutcome::NotCleaned => &mut s.not_cleaned,
                    ScrubOutcome::Failed => &mut s.failed,
                } += 1;
            }
            s
        })
        .collect()
}

#[cfg(test)]
#[path = "scrub_file_outcomes_tests.rs"]
mod tests;
